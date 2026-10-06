# FITS Viewer

Native Windows support for FITS astrophotography images (`.fits`, `.fit`, `.fts`), with an
automatic screen stretch so raw linear frames look like they do in your processing software
instead of a black rectangle.

- **Explorer thumbnails** in every icon view
- **Explorer preview pane** (Alt+P) with a caption: size, sample type, filter, exposure, target
- **Windows Photo Viewer**, Paint and other WIC apps open FITS files at full resolution
- **Windows Photos** via *Open with → Photos (FITS)*

It is a single ~280 KB DLL with no dependencies beyond Windows itself. Nothing runs in the
background: Windows loads it only while it is producing a thumbnail, preview or image.

## Supported files

- Mono, RGB (3-plane cubes) and one-shot-color Bayer frames (`BAYERPAT`, with `XBAYROFF` /
  `YBAYROFF`), debayered by binning each 2×2 cell into one pixel
- `BITPIX` 8, 16, 32, 64, -32 and -64, with `BZERO` / `BSCALE`
- `ROWORDER = 'TOP-DOWN'` (N.I.N.A. and others); bottom-up otherwise, per the FITS standard
- Images in the primary unit or in the first image extension

Compressed files (`.fz`) are not supported.

## Install

Requires 64-bit Windows 10 or 11 and the [Rust toolchain](https://rustup.rs) (MSVC).

```powershell
cargo build --release
.\install.ps1
```

`install.ps1` asks for administrator rights, copies the DLL to `C:\Program Files\FITS Viewer`
and registers it. Run it again after rebuilding to update; `uninstall.ps1` removes everything.

Then:

- **Thumbnails**: open a folder of FITS files and choose *View → Large icons*.
- **Preview pane**: press Alt+P and select a file.
- **Full size**: right-click a file → *Open with* → *Windows Photo Viewer* (arrow keys step
  through the folder) or *Photos (FITS)*. Tick *Always* to make either the default.

Installing does not change which app opens FITS files on double-click.

## How it works

Every integration shares one pipeline:

1. **Decode** ([`src/fits.rs`](src/fits.rs)). The file is streamed once; rows are box-averaged
   into the output size as they are read, so memory use is one source row plus the output image.
2. **Stretch** ([`src/stretch.rs`](src/stretch.rs)). Per channel, the median and median absolute
   deviation of the background are measured; the black point is set 2.8 MADs below the median
   and a midtones transfer function maps the background to 25% grey, as in PixInsight's AutoSTF.
3. **Present** the 8-bit result through the relevant Windows interface:

| Module | Interface | Used by |
|---|---|---|
| [`thumbnail.rs`](src/thumbnail.rs) | `IThumbnailProvider` | Explorer (isolated process, cached) |
| [`preview.rs`](src/preview.rs) | `IPreviewHandler` | Explorer preview pane (`prevhost.exe`) |
| [`wic.rs`](src/wic.rs) | `IWICBitmapDecoder` | Photo Viewer, Paint and other WIC apps |
| [`photos.rs`](src/photos.rs) | `rundll32` entry point | Photos app, via a temporary BMP |

The Photos app only opens file types built into it, so *Photos (FITS)* renders to
`%TEMP%\FitsViewer` (cleared after a day) and opens that file instead.

Registry entries are defined in [`src/registry.rs`](src/registry.rs) and written or removed by
`regsvr32`; COM plumbing lives in [`src/com.rs`](src/com.rs).

## Development

```powershell
cargo test --release
cargo run --release --example render -- input.fits output.bmp 256
```

The tests build FITS files in memory and exercise both the decoder and the COM objects through
the same interfaces Windows uses, without registering anything. The `render` example runs a file
through the pipeline and writes a BMP, which is handy for checking the stretch.

## License

Free to use and modify for personal or internal purposes; redistribution is not permitted.
See [LICENSE](LICENSE).
