//! COM plumbing shared by the handlers: class factory, object counting and stream adapters.

use crate::bitmap::{self, Bitmap};
use crate::fits::Header;
use crate::{CLSID_PREVIEW_HANDLER, CLSID_THUMBNAIL_PROVIDER, preview, thumbnail, wic};
use std::ffi::c_void;
use std::io::{self, BufReader, Read};
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::{BI_RGB, BITMAPINFO, BITMAPINFOHEADER};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl, IStream};
use windows::core::*;

static MODULE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static LIVE_OBJECTS: AtomicUsize = AtomicUsize::new(0);

pub fn set_module(module: HINSTANCE) {
    MODULE.store(module.0, Ordering::Relaxed);
}

/// This DLL's module handle.
pub fn module() -> HINSTANCE {
    HINSTANCE(MODULE.load(Ordering::Relaxed))
}

pub fn live_objects() -> usize {
    LIVE_OBJECTS.load(Ordering::Relaxed)
}

/// Embedded in every COM object so the DLL can tell Windows when it is safe to unload.
/// The private field forces construction through `default()`, which does the counting.
pub struct LiveObject(());

impl Default for LiveObject {
    fn default() -> Self {
        LIVE_OBJECTS.fetch_add(1, Ordering::Relaxed);
        LiveObject(())
    }
}

impl Drop for LiveObject {
    fn drop(&mut self) {
        LIVE_OBJECTS.fetch_sub(1, Ordering::Relaxed);
    }
}

#[implement(IClassFactory)]
pub struct ClassFactory {
    clsid: GUID,
    _live: LiveObject,
}

impl ClassFactory {
    pub fn new(clsid: GUID) -> Self {
        Self { clsid, _live: LiveObject::default() }
    }
}

impl IClassFactory_Impl for ClassFactory_Impl {
    fn CreateInstance(&self, outer: Ref<IUnknown>, iid: *const GUID, out: *mut *mut c_void) -> Result<()> {
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let object: IUnknown = match self.clsid {
            CLSID_THUMBNAIL_PROVIDER => thumbnail::ThumbnailProvider::default().into(),
            CLSID_PREVIEW_HANDLER => preview::PreviewHandler::default().into(),
            _ => wic::Decoder::default().into(),
        };
        unsafe { object.query(iid, out).ok() }
    }

    fn LockServer(&self, _lock: BOOL) -> Result<()> {
        Ok(())
    }
}

/// [`Read`] over a COM stream.
struct StreamReader(IStream);

impl Read for StreamReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut read = 0u32;
        let len = buf.len().min(u32::MAX as usize) as u32;
        unsafe { self.0.Read(buf.as_mut_ptr().cast(), len, Some(&mut read)) }.ok()?;
        Ok(read as usize)
    }
}

/// [`bitmap::render`] for a COM stream.
pub fn render_stream(stream: &IStream, max_side: usize) -> Result<(Header, Bitmap)> {
    let reader = BufReader::with_capacity(1 << 20, StreamReader(stream.clone()));
    bitmap::render(reader, max_side).map_err(|e| Error::new(E_FAIL, e.to_string()))
}

/// GDI description of a [`Bitmap`]'s pixel buffer.
pub fn bitmap_info(bitmap: &Bitmap) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: bitmap.width as i32,
            biHeight: -(bitmap.height as i32), // negative: rows stored top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    }
}
