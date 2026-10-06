//! The rendering pipeline shared by every integration: FITS file → autostretch → 8-bit bitmap.

use crate::{fits, stretch};
use std::io::{self, Read};

/// An 8-bit BGRA image, rows top to bottom.
#[derive(Debug)]
pub struct Bitmap {
    pub width: usize,
    pub height: usize,
    /// The source was single-channel, so B, G and R are equal.
    pub mono: bool,
    pub bgra: Vec<u8>,
}

/// Decodes and stretches a FITS stream into a bitmap no larger than `max_side` on either side.
pub fn render(reader: impl Read, max_side: usize) -> io::Result<(fits::Header, Bitmap)> {
    let (header, image) = fits::load(reader, max_side)?;
    let bgra = stretch::to_bgra(&image);
    Ok((header, Bitmap { width: image.width, height: image.height, mono: image.channels == 1, bgra }))
}

impl Bitmap {
    /// Encodes as an uncompressed BMP file: 8-bit grayscale for mono, 24-bit otherwise.
    ///
    /// Noisy astro data barely compresses, so this is about as small as PNG and far faster.
    #[must_use]
    pub fn to_bmp(&self) -> Vec<u8> {
        const HEADER_LEN: usize = 54;
        let (bits, palette_len) = if self.mono { (8, 256 * 4) } else { (24, 0) };
        let stride = (self.width * bits / 8).next_multiple_of(4);
        let offset = HEADER_LEN + palette_len;
        let file_len = offset + stride * self.height;

        let mut bmp = Vec::with_capacity(file_len);
        bmp.extend(b"BM");
        bmp.extend((file_len as u32).to_le_bytes());
        bmp.extend(0u32.to_le_bytes());
        bmp.extend((offset as u32).to_le_bytes());
        bmp.extend(40u32.to_le_bytes()); // BITMAPINFOHEADER size
        bmp.extend((self.width as i32).to_le_bytes());
        bmp.extend((-(self.height as i32)).to_le_bytes()); // negative height: rows stored top-down
        bmp.extend(1u16.to_le_bytes());
        bmp.extend((bits as u16).to_le_bytes());
        bmp.extend([0u8; 24]); // no compression, default resolution and palette size
        if self.mono {
            bmp.extend((0..=255u8).flat_map(|i| [i, i, i, 0]));
        }
        for row in self.bgra.chunks_exact(self.width * 4) {
            let start = bmp.len();
            for &[b, g, r, _] in row.as_chunks::<4>().0 {
                if self.mono {
                    bmp.push(b);
                } else {
                    bmp.extend([b, g, r]);
                }
            }
            bmp.resize(start + stride, 0);
        }
        bmp
    }
}
