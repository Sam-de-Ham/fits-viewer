//! Windows Imaging Component (WIC) decoder.
//!
//! Lets Windows Photo Viewer, Paint and any other WIC-based app open FITS files: each file is
//! presented as a single autostretched 32-bit BGRA frame at (up to) full resolution.

use crate::CLSID_WIC_DECODER;
use crate::bitmap::Bitmap;
use crate::com::{LiveObject, render_stream};
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IStream, STREAM_SEEK_CUR, STREAM_SEEK_SET};
use windows::core::*;

pub const CONTAINER_FORMAT: GUID = GUID::from_u128(0x5350dd37_6200_4d76_9157_44fa1868fd8b);
/// Every FITS file starts with this; WIC uses it to pick this decoder.
pub const SIGNATURE: &[u8] = b"SIMPLE  =";
pub const PIXEL_FORMAT: GUID = GUID_WICPixelFormat32bppBGRA;
/// Full resolution for any common sensor; larger images are averaged down to this.
const MAX_SIDE: usize = 8192;

#[implement(IWICBitmapDecoder)]
#[derive(Default)]
pub struct Decoder {
    // WIC codecs are free-threaded, so state is behind a lock.
    bitmap: Mutex<Option<Arc<Bitmap>>>,
    _live: LiveObject,
}

#[implement(IWICBitmapFrameDecode)]
struct Frame {
    bitmap: Arc<Bitmap>,
    _live: LiveObject,
}

fn no_color_contexts(actual: *mut u32) -> Result<()> {
    if let Some(actual) = unsafe { actual.as_mut() } {
        *actual = 0;
    }
    Ok(())
}

impl IWICBitmapDecoder_Impl for Decoder_Impl {
    fn QueryCapability(&self, stream: Ref<IStream>) -> Result<u32> {
        let stream = stream.ok()?;
        let mut magic = [0u8; SIGNATURE.len()];
        unsafe {
            // Peek at the signature, then restore the position WIC handed us.
            let mut start = 0u64;
            stream.Seek(0, STREAM_SEEK_CUR, Some(&mut start))?;
            let read = stream.Read(magic.as_mut_ptr().cast(), magic.len() as u32, None);
            stream.Seek(start as i64, STREAM_SEEK_SET, None)?;
            read.ok()?;
        }
        let capable = WICBitmapDecoderCapabilityCanDecodeAllImages.0 as u32;
        Ok(if magic == SIGNATURE { capable } else { 0 })
    }

    fn Initialize(&self, stream: Ref<IStream>, _options: WICDecodeOptions) -> Result<()> {
        let (_, bitmap) = render_stream(stream.ok()?, MAX_SIDE)?;
        *self.bitmap.lock().unwrap() = Some(Arc::new(bitmap));
        Ok(())
    }

    fn GetContainerFormat(&self) -> Result<GUID> {
        Ok(CONTAINER_FORMAT)
    }

    fn GetDecoderInfo(&self) -> Result<IWICBitmapDecoderInfo> {
        unsafe {
            let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            factory.CreateComponentInfo(&CLSID_WIC_DECODER)?.cast()
        }
    }

    fn CopyPalette(&self, _palette: Ref<IWICPalette>) -> Result<()> {
        Err(WINCODEC_ERR_PALETTEUNAVAILABLE.into())
    }

    fn GetMetadataQueryReader(&self) -> Result<IWICMetadataQueryReader> {
        Err(WINCODEC_ERR_UNSUPPORTEDOPERATION.into())
    }

    fn GetPreview(&self) -> Result<IWICBitmapSource> {
        Err(WINCODEC_ERR_UNSUPPORTEDOPERATION.into())
    }

    fn GetColorContexts(&self, _count: u32, _contexts: *mut Option<IWICColorContext>, actual: *mut u32) -> Result<()> {
        no_color_contexts(actual)
    }

    fn GetThumbnail(&self) -> Result<IWICBitmapSource> {
        Err(WINCODEC_ERR_CODECNOTHUMBNAIL.into())
    }

    fn GetFrameCount(&self) -> Result<u32> {
        Ok(1)
    }

    fn GetFrame(&self, index: u32) -> Result<IWICBitmapFrameDecode> {
        let bitmap = self.bitmap.lock().unwrap().clone().ok_or(Error::from(WINCODEC_ERR_NOTINITIALIZED))?;
        if index != 0 {
            return Err(WINCODEC_ERR_FRAMEMISSING.into());
        }
        Ok(Frame { bitmap, _live: LiveObject::default() }.into())
    }
}

impl IWICBitmapSource_Impl for Frame_Impl {
    fn GetSize(&self, width: *mut u32, height: *mut u32) -> Result<()> {
        unsafe { (*width, *height) = (self.bitmap.width as u32, self.bitmap.height as u32) };
        Ok(())
    }

    fn GetPixelFormat(&self) -> Result<GUID> {
        Ok(PIXEL_FORMAT)
    }

    fn GetResolution(&self, dpi_x: *mut f64, dpi_y: *mut f64) -> Result<()> {
        unsafe { (*dpi_x, *dpi_y) = (96.0, 96.0) };
        Ok(())
    }

    fn CopyPalette(&self, _palette: Ref<IWICPalette>) -> Result<()> {
        Err(WINCODEC_ERR_PALETTEUNAVAILABLE.into())
    }

    fn CopyPixels(&self, rect: *const WICRect, stride: u32, buffer_len: u32, buffer: *mut u8) -> Result<()> {
        let bitmap = &self.bitmap;
        let full = WICRect { X: 0, Y: 0, Width: bitmap.width as i32, Height: bitmap.height as i32 };
        let rect = unsafe { rect.as_ref() }.copied().unwrap_or(full);
        if rect.X < 0 || rect.Y < 0 || rect.Width < 0 || rect.Height < 0 {
            return Err(E_INVALIDARG.into());
        }
        let (x, y, w, h) = (rect.X as usize, rect.Y as usize, rect.Width as usize, rect.Height as usize);
        let (stride, row_len) = (stride as usize, w * 4);
        if x + w > bitmap.width || y + h > bitmap.height || stride < row_len {
            return Err(E_INVALIDARG.into());
        }
        if h > 0 && (buffer_len as usize) < stride * (h - 1) + row_len {
            return Err(WINCODEC_ERR_INSUFFICIENTBUFFER.into());
        }
        for row in 0..h {
            let src = &bitmap.bgra[((y + row) * bitmap.width + x) * 4..][..row_len];
            unsafe { std::ptr::copy_nonoverlapping(src.as_ptr(), buffer.add(row * stride), row_len) };
        }
        Ok(())
    }
}

impl IWICBitmapFrameDecode_Impl for Frame_Impl {
    fn GetMetadataQueryReader(&self) -> Result<IWICMetadataQueryReader> {
        Err(WINCODEC_ERR_UNSUPPORTEDOPERATION.into())
    }

    fn GetColorContexts(&self, _count: u32, _contexts: *mut Option<IWICColorContext>, actual: *mut u32) -> Result<()> {
        no_color_contexts(actual)
    }

    fn GetThumbnail(&self) -> Result<IWICBitmapSource> {
        Err(WINCODEC_ERR_CODECNOTHUMBNAIL.into())
    }
}
