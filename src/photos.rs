//! "Open with → Photos (FITS)".
//!
//! The Windows Photos app only opens file types built into it and ignores third-party WIC
//! decoders, so this renders the stretched image to a temporary BMP and opens that in Photos.
//! Registered as `rundll32.exe fits_viewer.dll,OpenInPhotos "<file>"`.

use crate::bitmap;
use std::fs;
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use std::time::Duration;
use windows::Win32::Foundation::{HINSTANCE, HWND};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MessageBoxW, SW_SHOWNORMAL};
use windows::core::{HSTRING, PCWSTR, w};

const MAX_SIDE: usize = 8192;
/// Rendered files older than this are removed from the temp folder on the next open.
const KEEP_FOR: Duration = Duration::from_hours(24);

/// rundll32 entry point (the `W` suffix makes rundll32 pass the command line as UTF-16).
#[unsafe(no_mangle)]
pub extern "system" fn OpenInPhotosW(_window: HWND, _instance: HINSTANCE, command_line: PCWSTR, _show: i32) {
    let argument = unsafe { command_line.to_string() }.unwrap_or_default();
    let path = argument.trim().trim_matches('"');
    if let Err(error) = open_in_photos(Path::new(path)) {
        let message = HSTRING::from(format!("Could not open {path}\n\n{error}"));
        unsafe { MessageBoxW(None, &message, w!("FITS Viewer"), MB_ICONERROR) };
    }
}

fn open_in_photos(path: &Path) -> io::Result<()> {
    let file = BufReader::with_capacity(1 << 20, fs::File::open(path)?);
    let (_, bitmap) = bitmap::render(file, MAX_SIDE)?;

    let dir = temp_dir()?;
    // Not `with_extension`: capture software puts dots in names ("..._-5.00_60.00s_0001").
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let output = dir.join(format!("{stem}.bmp"));
    fs::write(&output, bitmap.to_bmp())?;

    let uri = HSTRING::from(format!("ms-photos:viewer?fileName={}", output.display()));
    let result = unsafe { ShellExecuteW(None, w!("open"), &uri, None, None, SW_SHOWNORMAL) };
    // ShellExecute reports success with any value above 32.
    if result.0 as usize <= 32 {
        return Err(io::Error::other("the Photos app could not be started"));
    }
    Ok(())
}

/// `%TEMP%\FitsViewer`, with renders from previous days cleared out.
fn temp_dir() -> io::Result<PathBuf> {
    let dir = std::env::temp_dir().join("FitsViewer");
    fs::create_dir_all(&dir)?;
    for entry in fs::read_dir(&dir)?.flatten() {
        let age = entry.metadata().and_then(|m| m.modified()).map(|t| t.elapsed().unwrap_or_default());
        if age.is_ok_and(|age| age > KEEP_FOR) {
            _ = fs::remove_file(entry.path());
        }
    }
    Ok(dir)
}
