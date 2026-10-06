//! FITS Viewer: Windows integration for FITS astrophotography images.
//!
//! A single in-process COM server (DLL) providing:
//! - an Explorer thumbnail provider,
//! - an Explorer preview pane handler,
//! - a WIC image decoder, so Windows Photo Viewer and other WIC apps can open FITS files,
//! - an "Open in Photos" command for the Windows Photos app.
//!
//! All of them share one pipeline: [`fits`] decodes, [`stretch`] autostretches and [`bitmap`]
//! produces the 8-bit image. Windows loads the DLL only while one of these is in use; nothing
//! runs in the background.

pub mod bitmap;
mod com;
pub mod fits;
mod photos;
mod preview;
mod registry;
pub mod stretch;
mod thumbnail;
mod wic;

use std::ffi::c_void;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::IClassFactory;
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
use windows::core::{BOOL, GUID, HRESULT, Interface};

pub const CLSID_THUMBNAIL_PROVIDER: GUID = GUID::from_u128(0x452d8b5f_6241_43d7_a3ef_bd2cb031c08d);
pub const CLSID_PREVIEW_HANDLER: GUID = GUID::from_u128(0x92346f5b_a52e_46f0_9684_34e8602b16cf);
pub const CLSID_WIC_DECODER: GUID = GUID::from_u128(0xee3753a0_29e0_4619_a9a1_5bd2ddd27cbe);

#[unsafe(no_mangle)]
extern "system" fn DllMain(module: HINSTANCE, reason: u32, _reserved: *mut c_void) -> BOOL {
    const DLL_PROCESS_ATTACH: u32 = 1;
    if reason == DLL_PROCESS_ATTACH {
        com::set_module(module);
    }
    TRUE
}

/// # Safety
/// Called by COM with valid pointers to a CLSID, an IID and an output location.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(
    clsid: *const GUID,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    let clsid = unsafe { *clsid };
    if ![CLSID_THUMBNAIL_PROVIDER, CLSID_PREVIEW_HANDLER, CLSID_WIC_DECODER].contains(&clsid) {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = com::ClassFactory::new(clsid).into();
    unsafe { factory.query(iid, out) }
}

#[unsafe(no_mangle)]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    if com::live_objects() == 0 { S_OK } else { S_FALSE }
}

#[unsafe(no_mangle)]
extern "system" fn DllRegisterServer() -> HRESULT {
    let mut path = [0u16; MAX_PATH as usize];
    let len = unsafe { GetModuleFileNameW(Some(com::module().into()), &mut path) } as usize;
    let result = registry::register(&String::from_utf16_lossy(&path[..len]));
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
    result.map_or_else(|e| e.code(), |()| S_OK)
}

#[unsafe(no_mangle)]
extern "system" fn DllUnregisterServer() -> HRESULT {
    registry::unregister();
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
    S_OK
}
