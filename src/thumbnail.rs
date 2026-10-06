//! Explorer thumbnail provider.
//!
//! Windows runs it in an isolated surrogate process and caches the result, so each file is
//! decoded once; afterwards Explorer serves the thumbnail from its cache.

use crate::com::{LiveObject, bitmap_info, render_stream};
use std::cell::RefCell;
use windows::Win32::Foundation::E_UNEXPECTED;
use windows::Win32::Graphics::Gdi::{CreateDIBSection, DIB_RGB_COLORS, HBITMAP};
use windows::Win32::System::Com::IStream;
use windows::Win32::UI::Shell::PropertiesSystem::{IInitializeWithStream, IInitializeWithStream_Impl};
use windows::Win32::UI::Shell::{IThumbnailProvider, IThumbnailProvider_Impl, WTS_ALPHATYPE, WTSAT_RGB};
use windows::core::*;

#[implement(IInitializeWithStream, IThumbnailProvider)]
#[derive(Default)]
pub struct ThumbnailProvider {
    stream: RefCell<Option<IStream>>,
    _live: LiveObject,
}

impl IInitializeWithStream_Impl for ThumbnailProvider_Impl {
    fn Initialize(&self, stream: Ref<IStream>, _mode: u32) -> Result<()> {
        self.stream.replace(Some(stream.ok()?.clone()));
        Ok(())
    }
}

impl IThumbnailProvider_Impl for ThumbnailProvider_Impl {
    fn GetThumbnail(&self, size: u32, out: *mut HBITMAP, alpha: *mut WTS_ALPHATYPE) -> Result<()> {
        let stream = self.stream.borrow().clone().ok_or(Error::from(E_UNEXPECTED))?;
        let (_, bitmap) = render_stream(&stream, size as usize)?;
        unsafe {
            let mut bits = std::ptr::null_mut();
            let hbitmap = CreateDIBSection(None, &bitmap_info(&bitmap), DIB_RGB_COLORS, &mut bits, None, 0)?;
            std::ptr::copy_nonoverlapping(bitmap.bgra.as_ptr(), bits.cast::<u8>(), bitmap.bgra.len());
            *out = hbitmap;
            *alpha = WTSAT_RGB;
        }
        Ok(())
    }
}
