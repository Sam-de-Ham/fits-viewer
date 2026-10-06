//! Machine-wide (HKLM) registration, written by `regsvr32` and removed by `regsvr32 /u`.
//!
//! Registers the thumbnail provider, preview handler and WIC decoder, and adds "Open with"
//! entries for Windows Photo Viewer and the Photos app. The default app for FITS files is left
//! untouched.

use crate::{CLSID_PREVIEW_HANDLER, CLSID_THUMBNAIL_PROVIDER, CLSID_WIC_DECODER, wic};
use windows::Win32::Graphics::Imaging::CATID_WICBitmapDecoders;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Shell::{IPreviewHandler, IThumbnailProvider};
use windows::core::{GUID, HSTRING, Interface, Result};

const NAME: &str = "FITS Viewer";
const EXTENSIONS: [&str; 3] = [".fits", ".fit", ".fts"];
const VENDOR: GUID = GUID::from_u128(0xaaeea9ab_f263_4c88_9ad9_fc8ec0cfda8b);
/// The 64-bit preview handler surrogate, prevhost.exe.
const PREVHOST_APPID: GUID = GUID::from_u128(0x6d2b5079_2f0b_48dd_ab7f_97cec514d30b);
/// Windows Photo Viewer's drop target, as used by its own file associations.
const PHOTO_VIEWER_DROP_TARGET: GUID = GUID::from_u128(0xffe2a43c_56b9_4bf5_9a79_cc6d4285608a);
const PHOTO_VIEWER_PROGID: &str = "FitsViewer.PhotoViewer";
const PHOTOS_PROGID: &str = "FitsViewer.Photos";
const PREVIEW_HANDLERS: &str = r"Software\Microsoft\Windows\CurrentVersion\PreviewHandlers";
const IMAGE_ICON: &str = r"%SystemRoot%\System32\imageres.dll,-122";

enum Value {
    String(String),
    ExpandString(&'static str),
    Dword(u32),
    Binary(Vec<u8>),
    /// A zero-length value; with an empty name, the key is created without a default value.
    Empty,
}

fn string(s: &str) -> Value {
    Value::String(s.to_string())
}

/// `{XXXXXXXX-XXXX-...}`, the registry form of a GUID.
fn braced(guid: GUID) -> String {
    format!("{{{guid:?}}}")
}

fn class_key(clsid: GUID) -> String {
    format!(r"Software\Classes\CLSID\{}", braced(clsid))
}

fn decoder_instance_key() -> String {
    format!(r"{}\Instance\{}", class_key(CATID_WICBitmapDecoders), braced(CLSID_WIC_DECODER))
}

/// Every registry value to write, as (key under HKLM, value name, value).
fn entries(dll: &str) -> Vec<(String, String, Value)> {
    let mut entries = Vec::new();
    let mut add = |key: String, name: &str, value: Value| entries.push((key, name.to_string(), value));

    for (clsid, threading) in
        [(CLSID_THUMBNAIL_PROVIDER, "Apartment"), (CLSID_PREVIEW_HANDLER, "Apartment"), (CLSID_WIC_DECODER, "Both")]
    {
        add(class_key(clsid), "", string(NAME));
        add(format!(r"{}\InprocServer32", class_key(clsid)), "", string(dll));
        add(format!(r"{}\InprocServer32", class_key(clsid)), "ThreadingModel", string(threading));
    }

    let preview = class_key(CLSID_PREVIEW_HANDLER);
    add(preview.clone(), "AppID", string(&braced(PREVHOST_APPID)));
    add(preview, "DisplayName", string(NAME));
    add(PREVIEW_HANDLERS.into(), &braced(CLSID_PREVIEW_HANDLER), string(NAME));

    let decoder = class_key(CLSID_WIC_DECODER);
    for (name, value) in [
        ("FriendlyName", string(NAME)),
        ("Author", string(NAME)),
        ("Description", string("Autostretched FITS astrophotography images")),
        ("ContainerFormat", string(&braced(wic::CONTAINER_FORMAT))),
        ("Vendor", string(&braced(VENDOR))),
        ("Version", string("1.0.0.0")),
        ("SpecVersion", string("1.0.0.0")),
        ("FileExtensions", string(&EXTENSIONS.join(","))),
        ("MimeTypes", string("image/fits")),
        ("SupportAnimation", Value::Dword(0)),
        ("SupportChromakey", Value::Dword(0)),
        ("SupportLossless", Value::Dword(1)),
        ("SupportMultiframe", Value::Dword(0)),
    ] {
        add(decoder.clone(), name, value);
    }
    add(format!(r"{decoder}\Formats\{}", braced(wic::PIXEL_FORMAT)), "", Value::Empty);
    let pattern = format!(r"{decoder}\Patterns\0");
    add(pattern.clone(), "Position", Value::Dword(0));
    add(pattern.clone(), "Length", Value::Dword(wic::SIGNATURE.len() as u32));
    add(pattern.clone(), "Pattern", Value::Binary(wic::SIGNATURE.to_vec()));
    add(pattern, "Mask", Value::Binary(vec![0xFF; wic::SIGNATURE.len()]));
    add(decoder_instance_key(), "CLSID", string(&braced(CLSID_WIC_DECODER)));
    add(decoder_instance_key(), "FriendlyName", string(NAME));

    // Photo Viewer: the same command line Windows uses for its own Photo Viewer file types.
    let photo_viewer = format!(r"Software\Classes\{PHOTO_VIEWER_PROGID}");
    add(photo_viewer.clone(), "", string("FITS image"));
    add(format!(r"{photo_viewer}\DefaultIcon"), "", Value::ExpandString(IMAGE_ICON));
    add(format!(r"{photo_viewer}\shell\open"), "FriendlyAppName", string("Windows Photo Viewer"));
    add(
        format!(r"{photo_viewer}\shell\open\command"),
        "",
        Value::ExpandString(
            r#"%SystemRoot%\System32\rundll32.exe "%ProgramFiles%\Windows Photo Viewer\PhotoViewer.dll", ImageView_Fullscreen %1"#,
        ),
    );
    add(format!(r"{photo_viewer}\shell\open\DropTarget"), "Clsid", string(&braced(PHOTO_VIEWER_DROP_TARGET)));

    // Photos app: see photos.rs.
    let photos = format!(r"Software\Classes\{PHOTOS_PROGID}");
    add(photos.clone(), "", string("FITS image"));
    add(format!(r"{photos}\DefaultIcon"), "", Value::ExpandString(IMAGE_ICON));
    add(format!(r"{photos}\shell\open"), "FriendlyAppName", string("Photos (FITS)"));
    add(format!(r"{photos}\shell\open\command"), "", string(&format!(r#"rundll32.exe "{dll}",OpenInPhotos "%1""#)));

    for ext in EXTENSIONS {
        let shell_ex = format!(r"Software\Classes\{ext}\ShellEx");
        add(format!(r"{shell_ex}\{}", braced(IThumbnailProvider::IID)), "", string(&braced(CLSID_THUMBNAIL_PROVIDER)));
        add(format!(r"{shell_ex}\{}", braced(IPreviewHandler::IID)), "", string(&braced(CLSID_PREVIEW_HANDLER)));
        add(format!(r"Software\Classes\{ext}\OpenWithProgids"), PHOTO_VIEWER_PROGID, Value::Empty);
        add(format!(r"Software\Classes\{ext}\OpenWithProgids"), PHOTOS_PROGID, Value::Empty);
    }
    entries
}

fn set(key: &str, name: &str, value: &Value) -> Result<()> {
    let utf16 = |s: &str| s.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect::<Vec<u8>>();
    let (kind, data) = match value {
        Value::String(s) => (REG_SZ, utf16(s)),
        Value::ExpandString(s) => (REG_EXPAND_SZ, utf16(s)),
        Value::Dword(d) => (REG_DWORD, d.to_le_bytes().to_vec()),
        Value::Binary(b) => (REG_BINARY, b.clone()),
        Value::Empty => (REG_NONE, Vec::new()),
    };
    unsafe {
        let mut hkey = HKEY::default();
        RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(key),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut hkey,
            None,
        )
        .ok()?;
        let result = match (name, value) {
            ("", Value::Empty) => Ok(()),
            _ => RegSetValueExW(hkey, &HSTRING::from(name), None, kind, Some(&data)).ok(),
        };
        _ = RegCloseKey(hkey);
        result
    }
}

pub fn register(dll: &str) -> Result<()> {
    entries(dll).iter().try_for_each(|(key, name, value)| set(key, name, value))
}

pub fn unregister() {
    let delete_key = |key: &str| unsafe { _ = RegDeleteTreeW(HKEY_LOCAL_MACHINE, &HSTRING::from(key)) };
    let delete_value = |key: &str, name: &str| unsafe {
        _ = RegDeleteKeyValueW(HKEY_LOCAL_MACHINE, &HSTRING::from(key), &HSTRING::from(name));
    };

    for clsid in [CLSID_THUMBNAIL_PROVIDER, CLSID_PREVIEW_HANDLER, CLSID_WIC_DECODER] {
        delete_key(&class_key(clsid));
    }
    delete_key(&decoder_instance_key());
    delete_value(PREVIEW_HANDLERS, &braced(CLSID_PREVIEW_HANDLER));
    for progid in [PHOTO_VIEWER_PROGID, PHOTOS_PROGID] {
        delete_key(&format!(r"Software\Classes\{progid}"));
    }
    for ext in EXTENSIONS {
        delete_key(&format!(r"Software\Classes\{ext}\ShellEx\{}", braced(IThumbnailProvider::IID)));
        delete_key(&format!(r"Software\Classes\{ext}\ShellEx\{}", braced(IPreviewHandler::IID)));
        for progid in [PHOTO_VIEWER_PROGID, PHOTOS_PROGID] {
            delete_value(&format!(r"Software\Classes\{ext}\OpenWithProgids"), progid);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn guids_use_registry_format() {
        assert_eq!(super::braced(super::PREVHOST_APPID), "{6D2B5079-2F0B-48DD-AB7F-97CEC514D30B}");
    }
}
