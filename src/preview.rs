//! Explorer preview pane handler (Alt+P).
//!
//! Hosted by Windows in prevhost.exe. Shows the stretched image fitted to the pane, with a
//! caption of size, sample type, filter, exposure and target.

use crate::bitmap::Bitmap;
use crate::com::{LiveObject, bitmap_info, module, render_stream};
use crate::fits::Header;
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::sync::Once;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::IStream;
use windows::Win32::System::Ole::{IObjectWithSite, IObjectWithSite_Impl, IOleWindow, IOleWindow_Impl};
use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows::Win32::UI::Shell::PropertiesSystem::{IInitializeWithStream, IInitializeWithStream_Impl};
use windows::Win32::UI::Shell::{IPreviewHandler, IPreviewHandler_Impl, IPreviewHandlerFrame};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

const WINDOW_CLASS: PCWSTR = w!("FitsViewerPreview");
/// The pane is rarely larger than this; GDI scales the image to the actual pane size.
const MAX_SIDE: usize = 2048;
const CAPTION_HEIGHT: i32 = 24;
const CAPTION_COLOR: COLORREF = COLORREF(0x00B4_B4B4);

/// What the preview window paints; owned by [`PreviewHandler`], referenced by its window.
struct Content {
    bitmap: Bitmap,
    caption: Vec<u16>,
}

// Not agile: the handler owns a window, so COM must call it on the thread that pumps that
// window's messages. An agile object would be called on RPC worker threads in prevhost.exe,
// leaving the window unpainted.
#[implement(IPreviewHandler, IInitializeWithStream, IObjectWithSite, IOleWindow, Agile = false)]
#[derive(Default)]
pub struct PreviewHandler {
    stream: RefCell<Option<IStream>>,
    site: RefCell<Option<IUnknown>>,
    parent: Cell<HWND>,
    rect: Cell<RECT>,
    window: Cell<HWND>,
    content: RefCell<Option<Box<Content>>>,
    _live: LiveObject,
}

impl PreviewHandler {
    fn close(&self) {
        let window = self.window.replace(HWND::default());
        if !window.is_invalid() {
            unsafe { _ = DestroyWindow(window) };
        }
        // Only after the window that points at it is gone.
        self.content.replace(None);
    }
}

impl Drop for PreviewHandler {
    fn drop(&mut self) {
        self.close();
    }
}

fn caption(header: &Header) -> Vec<u16> {
    let color = match (header.bayer, header.planes) {
        (Some(_), _) => "OSC",
        (None, 3) => "RGB",
        _ => "mono",
    };
    let depth = match header.bitpix {
        bits if bits < 0 => format!("{}-bit float", -bits),
        bits => format!("{bits}-bit"),
    };
    let mut parts = vec![format!("{} × {}", header.width, header.height), format!("{depth} {color}")];
    parts.extend(header.filter.clone());
    parts.extend(header.exposure.map(|seconds| format!("{seconds} s")));
    parts.extend(header.object.clone());
    parts.join("  ·  ").encode_utf16().collect()
}

fn register_window_class() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| unsafe {
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: module(),
            lpszClassName: WINDOW_CLASS,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&class);
    });
}

unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            unsafe { paint(window) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

unsafe fn paint(window: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(window, &mut ps);
        let mut client = RECT::default();
        _ = GetClientRect(window, &mut client);
        FillRect(dc, &client, HBRUSH(GetStockObject(BLACK_BRUSH).0));

        if let Some(content) = (GetWindowLongPtrW(window, GWLP_USERDATA) as *const Content).as_ref() {
            let bitmap = &content.bitmap;
            let (w, h) = (bitmap.width as i32, bitmap.height as i32);
            let (area_w, area_h) = (client.right, (client.bottom - CAPTION_HEIGHT).max(1));
            let scale = (area_w as f32 / w as f32).min(area_h as f32 / h as f32);
            let (dst_w, dst_h) = ((w as f32 * scale) as i32, (h as f32 * scale) as i32);
            let (dst_x, dst_y) = ((area_w - dst_w) / 2, (area_h - dst_h) / 2);

            SetStretchBltMode(dc, HALFTONE);
            _ = SetBrushOrgEx(dc, 0, 0, None);
            let pixels = Some(bitmap.bgra.as_ptr().cast());
            let info = bitmap_info(bitmap);
            StretchDIBits(dc, dst_x, dst_y, dst_w, dst_h, 0, 0, w, h, pixels, &info, DIB_RGB_COLORS, SRCCOPY);

            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, CAPTION_COLOR);
            SelectObject(dc, GetStockObject(DEFAULT_GUI_FONT));
            let mut text_rect = RECT { left: 8, top: area_h, right: area_w - 8, bottom: client.bottom };
            let format = DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS;
            DrawTextW(dc, &mut content.caption.clone(), &mut text_rect, format);
        }
        _ = EndPaint(window, &ps);
    }
}

impl IPreviewHandler_Impl for PreviewHandler_Impl {
    fn SetWindow(&self, parent: HWND, rect: *const RECT) -> Result<()> {
        self.parent.set(parent);
        self.SetRect(rect)?;
        let window = self.window.get();
        if !window.is_invalid() {
            unsafe { SetParent(window, Some(parent))? };
        }
        Ok(())
    }

    fn SetRect(&self, rect: *const RECT) -> Result<()> {
        let rect = unsafe { rect.as_ref() }.copied().ok_or(Error::from(E_INVALIDARG))?;
        self.rect.set(rect);
        let window = self.window.get();
        if !window.is_invalid() {
            let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
            unsafe {
                SetWindowPos(window, None, rect.left, rect.top, w, h, SWP_NOZORDER | SWP_NOACTIVATE)?;
                _ = InvalidateRect(Some(window), None, false);
            }
        }
        Ok(())
    }

    fn DoPreview(&self) -> Result<()> {
        let stream = self.stream.borrow().clone().ok_or(Error::from(E_UNEXPECTED))?;
        let (header, bitmap) = render_stream(&stream, MAX_SIDE)?;
        let content = Box::new(Content { bitmap, caption: caption(&header) });

        self.close();
        register_window_class();
        let rect = self.rect.get();
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        let style = WS_CHILD | WS_VISIBLE;
        let parent = Some(self.parent.get());
        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                WINDOW_CLASS,
                w!(""),
                style,
                rect.left,
                rect.top,
                w,
                h,
                parent,
                None,
                Some(module()),
                None,
            )?
        };
        unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, &*content as *const Content as isize) };
        self.content.replace(Some(content));
        self.window.set(window);
        Ok(())
    }

    fn Unload(&self) -> Result<()> {
        self.close();
        self.stream.replace(None);
        Ok(())
    }

    fn SetFocus(&self) -> Result<()> {
        Ok(())
    }

    fn QueryFocus(&self) -> Result<HWND> {
        Ok(unsafe { GetFocus() })
    }

    fn TranslateAccelerator(&self, message: *const MSG) -> Result<()> {
        // Hand keystrokes back to Explorer (e.g. Tab to return to the file list).
        let frame = self.site.borrow().as_ref().and_then(|site| site.cast::<IPreviewHandlerFrame>().ok());
        match frame {
            Some(frame) => unsafe { frame.TranslateAccelerator(message) },
            None => Err(S_FALSE.into()),
        }
    }
}

impl IInitializeWithStream_Impl for PreviewHandler_Impl {
    fn Initialize(&self, stream: Ref<IStream>, _mode: u32) -> Result<()> {
        self.stream.replace(Some(stream.ok()?.clone()));
        Ok(())
    }
}

impl IObjectWithSite_Impl for PreviewHandler_Impl {
    fn SetSite(&self, site: Ref<IUnknown>) -> Result<()> {
        self.site.replace(site.cloned());
        Ok(())
    }

    fn GetSite(&self, iid: *const GUID, out: *mut *mut c_void) -> Result<()> {
        match self.site.borrow().as_ref() {
            Some(site) => unsafe { site.query(iid, out).ok() },
            None => Err(E_FAIL.into()),
        }
    }
}

impl IOleWindow_Impl for PreviewHandler_Impl {
    fn GetWindow(&self) -> Result<HWND> {
        let window = self.window.get();
        if window.is_invalid() { Err(E_FAIL.into()) } else { Ok(window) }
    }

    fn ContextSensitiveHelp(&self, _enter: BOOL) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
}
