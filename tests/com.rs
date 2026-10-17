//! Drives the COM objects through the same interfaces Windows uses, without registering them.

mod common;

use common::*;
use fits_viewer::{CLSID_PREVIEW_HANDLER, CLSID_THUMBNAIL_PROVIDER, CLSID_WIC_DECODER, DllGetClassObject, bitmap};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::{BITMAP, GetObjectW, HBITMAP};
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Ole::IOleWindow;
use windows::Win32::UI::Shell::PropertiesSystem::IInitializeWithStream;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

/// A 64x48 mono frame with a bright band, as an in-memory COM stream.
fn sample() -> (Vec<u8>, IStream) {
    let pixels: Vec<i16> =
        noise(64 * 48).iter().enumerate().map(|(i, n)| (n * 1000.0) as i16 + (i / 64 % 16) as i16 * 100).collect();
    let file = image("16", 64, 48, &[("BZERO", "32768")], &be_i16(pixels));
    let stream = unsafe { SHCreateMemStream(Some(&file)) }.expect("memory stream");
    (file, stream)
}

fn create<T: Interface>(clsid: GUID) -> T {
    unsafe {
        _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let mut factory = std::ptr::null_mut();
        DllGetClassObject(&clsid, &IClassFactory::IID, &mut factory).ok().expect("class factory");
        IClassFactory::from_raw(factory).CreateInstance(None).expect("instance")
    }
}

#[test]
fn thumbnail_provider_fits_requested_size() {
    let (_, stream) = sample();
    let provider: IThumbnailProvider = create(CLSID_THUMBNAIL_PROVIDER);
    unsafe {
        provider.cast::<IInitializeWithStream>().unwrap().Initialize(&stream, 0).unwrap();
        let (mut hbitmap, mut alpha) = (HBITMAP::default(), WTS_ALPHATYPE::default());
        provider.GetThumbnail(32, &mut hbitmap, &mut alpha).unwrap();

        let mut info = BITMAP::default();
        GetObjectW(hbitmap.into(), size_of::<BITMAP>() as i32, Some((&raw mut info).cast()));
        assert_eq!((info.bmWidth, info.bmHeight.abs()), (32, 24));
        assert_eq!(alpha, WTSAT_RGB);
    }
}

#[test]
fn wic_decoder_matches_rendered_bitmap() {
    let (file, stream) = sample();
    let decoder: IWICBitmapDecoder = create(CLSID_WIC_DECODER);
    unsafe {
        assert_eq!(decoder.QueryCapability(&stream).unwrap(), WICBitmapDecoderCapabilityCanDecodeAllImages.0 as u32);
        let not_fits = SHCreateMemStream(Some(b"\x89PNG\r\n\x1a\n.....")).unwrap();
        assert_eq!(decoder.QueryCapability(&not_fits).unwrap(), 0);

        decoder.Initialize(&stream, WICDecodeMetadataCacheOnDemand).unwrap();
        let frame = decoder.GetFrame(0).unwrap();
        let (mut width, mut height) = (0, 0);
        frame.GetSize(&mut width, &mut height).unwrap();
        assert_eq!((width, height), (64, 48));
        assert_eq!(frame.GetPixelFormat().unwrap(), GUID_WICPixelFormat32bppBGRA);

        let mut pixels = vec![0u8; 64 * 48 * 4];
        frame.CopyPixels(std::ptr::null(), 64 * 4, &mut pixels).unwrap();
        let (_, expected) = bitmap::render(&file[..], usize::MAX).unwrap();
        assert_eq!(pixels, expected.bgra);

        let partial = WICRect { X: 60, Y: 0, Width: 8, Height: 1 };
        assert!(frame.CopyPixels(&partial, 8 * 4, &mut pixels).is_err(), "out-of-bounds rect must fail");
        assert!(decoder.GetFrame(1).is_err());
    }
}

#[test]
fn preview_handler_creates_child_window() {
    let (_, stream) = sample();
    let handler: IPreviewHandler = create(CLSID_PREVIEW_HANDLER);
    unsafe {
        let host = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!(""),
            WS_POPUP,
            0,
            0,
            400,
            300,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        handler.cast::<IInitializeWithStream>().unwrap().Initialize(&stream, 0).unwrap();
        handler.SetWindow(host, &RECT { left: 0, top: 0, right: 400, bottom: 300 }).unwrap();
        handler.DoPreview().unwrap();

        let child = handler.cast::<IOleWindow>().unwrap().GetWindow().unwrap();
        assert_eq!(GetParent(child).unwrap(), host);
        handler.Unload().unwrap();
        assert!(handler.cast::<IOleWindow>().unwrap().GetWindow().is_err());
        DestroyWindow(host).unwrap();
    }
}

#[test]
fn shell_handlers_are_not_agile() {
    // Agile objects get called on RPC worker threads out of process (prevhost.exe, dllhost.exe),
    // where the preview window would never receive messages and never paint.
    const IID_IAGILEOBJECT: GUID = GUID::from_u128(0x94ea2b94_e9cc_49e0_c0ff_ee64ca8f5b90);
    const IID_IMARSHAL: GUID = GUID::from_u128(0x00000003_0000_0000_c000_000000000046);
    for clsid in [CLSID_THUMBNAIL_PROVIDER, CLSID_PREVIEW_HANDLER] {
        let object: IUnknown = create(clsid);
        for iid in [IID_IAGILEOBJECT, IID_IMARSHAL] {
            let mut out = std::ptr::null_mut();
            assert!(unsafe { object.query(&iid, &mut out) }.is_err(), "{clsid:?} must not expose {iid:?}");
        }
    }
}
