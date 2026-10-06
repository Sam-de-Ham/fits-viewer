//! Streaming FITS reader.
//!
//! The image is box-averaged down to the requested size while it is read, one row at a time,
//! so memory use is a single source row plus the output image, regardless of the file size.

use std::io::{self, Read};

const BLOCK_LEN: usize = 2880;
const CARD_LEN: usize = 80;
/// Rejects corrupt headers before they turn into huge allocations.
const MAX_SIDE: usize = 1 << 20;

/// The header keywords needed to decode and describe an image.
#[derive(Debug, Default)]
pub struct Header {
    /// Sample type: 8, 16, 32, 64 (integers) or -32, -64 (IEEE floats).
    pub bitpix: i32,
    pub width: usize,
    pub height: usize,
    /// 3 for RGB cubes, otherwise 1.
    pub planes: usize,
    pub bzero: f64,
    pub bscale: f64,
    /// Color channel (0 = R, 1 = G, 2 = B) of each pixel in a 2x2 Bayer cell, row-major.
    pub bayer: Option<[usize; 4]>,
    /// FITS stores the bottom row first unless `ROWORDER = 'TOP-DOWN'` says otherwise.
    pub top_down: bool,
    pub object: Option<String>,
    pub filter: Option<String>,
    /// Exposure time in seconds.
    pub exposure: Option<f64>,
}

/// A decoded image with `channels` interleaved samples per pixel, rows ordered top to bottom.
#[derive(Debug)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub channels: usize,
    pub data: Vec<f32>,
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

/// The value part of a header card, without quotes or trailing comment.
fn card_value(card: &str) -> &str {
    let value = card.get(10..).unwrap_or_default().trim_start();
    match value.strip_prefix('\'') {
        Some(quoted) => quoted.split('\'').next().unwrap_or_default().trim_end(),
        None => value.split('/').next().unwrap_or_default().trim(),
    }
}

fn non_empty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

/// Parses one header unit and returns it with the size of the data that follows it.
fn read_header(reader: &mut impl Read) -> io::Result<(Header, u64)> {
    let mut header = Header { bscale: 1.0, ..Default::default() };
    let (mut naxis, mut axes) = (0usize, [1usize; 3]);
    let (mut pattern, mut x_offset, mut y_offset) = (None::<String>, 0usize, 0usize);

    let mut block = [0u8; BLOCK_LEN];
    reader.read_exact(&mut block)?;
    if !block.starts_with(b"SIMPLE  =") && !block.starts_with(b"XTENSION=") {
        return Err(invalid("not a FITS file"));
    }
    'blocks: loop {
        for card in block.chunks(CARD_LEN).filter_map(|c| std::str::from_utf8(c).ok().filter(|_| c.is_ascii())) {
            let (key, value) = (card[..8].trim_end(), card_value(card));
            match key {
                "END" => break 'blocks,
                "BITPIX" => header.bitpix = value.parse().map_err(|_| invalid("bad BITPIX"))?,
                "NAXIS" => naxis = value.parse().map_err(|_| invalid("bad NAXIS"))?,
                "NAXIS1" | "NAXIS2" | "NAXIS3" => {
                    let axis = (key.as_bytes()[5] - b'1') as usize;
                    axes[axis] = value.parse().map_err(|_| invalid("bad NAXISn"))?;
                }
                "BZERO" => header.bzero = value.parse().unwrap_or(0.0),
                "BSCALE" => header.bscale = value.parse().unwrap_or(1.0),
                "BAYERPAT" | "COLORTYP" => pattern = Some(value.to_ascii_uppercase()),
                "XBAYROFF" => x_offset = value.parse::<f64>().unwrap_or(0.0) as usize,
                "YBAYROFF" => y_offset = value.parse::<f64>().unwrap_or(0.0) as usize,
                "ROWORDER" => header.top_down = value.eq_ignore_ascii_case("TOP-DOWN"),
                "OBJECT" => header.object = non_empty(value),
                "FILTER" => header.filter = non_empty(value),
                "EXPTIME" | "EXPOSURE" => header.exposure = header.exposure.or(value.parse().ok()),
                _ => {}
            }
        }
        reader.read_exact(&mut block)?;
    }

    let samples: u64 = axes[..naxis.min(3)].iter().map(|&a| a as u64).product();
    let data_len = if naxis == 0 { 0 } else { samples * (header.bitpix.unsigned_abs() as u64 / 8) };
    header.width = axes[0];
    header.height = if naxis >= 2 { axes[1] } else { 1 };
    header.planes = if naxis >= 3 && axes[2] >= 3 { 3 } else { 1 };
    if header.planes == 1 {
        header.bayer = pattern.and_then(|p| {
            let cell: Vec<usize> = p.chars().filter_map(|c| "RGB".find(c)).collect();
            (cell.len() == 4).then(|| {
                let at = |x: usize, y: usize| cell[((y + y_offset) % 2) * 2 + (x + x_offset) % 2];
                [at(0, 0), at(1, 0), at(0, 1), at(1, 1)]
            })
        });
    }
    Ok((header, data_len))
}

/// Reads the first image in the file, box-averaged so that neither side exceeds `max_side`.
/// Bayer (one-shot color) data is first binned into one RGB pixel per 2x2 cell.
pub fn load(mut reader: impl Read, max_side: usize) -> io::Result<(Header, Image)> {
    let header = loop {
        let (header, data_len) = read_header(&mut reader)?;
        if header.width > 1 && header.height > 1 {
            break header;
        }
        // Not an image (typically an empty primary unit before an image extension): skip it.
        let padded = data_len.div_ceil(BLOCK_LEN as u64) * BLOCK_LEN as u64;
        io::copy(&mut (&mut reader).take(padded), &mut io::sink())?;
    };
    let sample_len = match header.bitpix {
        8 | 16 | 32 | 64 | -32 | -64 => header.bitpix.unsigned_abs() as usize / 8,
        _ => return Err(invalid("unsupported BITPIX")),
    };
    if header.width > MAX_SIDE || header.height > MAX_SIDE {
        return Err(invalid("implausible image size"));
    }

    let bin = if header.bayer.is_some() { 2 } else { 1 };
    let channels = if header.bayer.is_some() { 3 } else { header.planes };
    let (binned_w, binned_h) = (header.width / bin, header.height / bin);
    let factor = binned_w.max(binned_h).div_ceil(max_side.max(1)).max(1);
    let (out_w, out_h) = (binned_w.div_ceil(factor), binned_h.div_ceil(factor));
    let stride = out_w * channels;

    let mut data = vec![0f32; out_h * stride];
    let (mut sum, mut count) = (vec![0f32; stride], vec![0u32; stride]);
    let mut raw = vec![0u8; header.width * sample_len];
    // Columns outside the binned grid (a trailing odd Bayer column) are ignored.
    let mut row = vec![0f32; binned_w * bin];
    let out_col: Vec<usize> = (0..row.len()).map(|x| x / bin / factor).collect();
    let used_rows = binned_h * bin;
    let (scale, zero) = (header.bscale as f32, header.bzero as f32);

    for plane in 0..header.planes {
        for y in 0..header.height {
            reader.read_exact(&mut raw)?;
            if y >= used_rows {
                continue;
            }
            decode_row(&raw, header.bitpix, &mut row);
            for (x, &v) in row.iter().enumerate().filter(|(_, v)| v.is_finite()) {
                let channel = header.bayer.map_or(plane, |cell| cell[(y % 2) * 2 + x % 2]);
                let i = out_col[x] * channels + channel;
                sum[i] += v * scale + zero;
                count[i] += 1;
            }

            let out_y = y / bin / factor;
            if y + 1 == used_rows || (y + 1) / bin / factor != out_y {
                let dst = if header.top_down { out_y } else { out_h - 1 - out_y };
                // Channels absent from this plane keep the values written by earlier planes.
                for (i, (s, c)) in sum.iter_mut().zip(&mut count).enumerate() {
                    if *c > 0 {
                        data[dst * stride + i] = *s / *c as f32;
                    }
                    (*s, *c) = (0.0, 0);
                }
            }
        }
    }
    Ok((header, Image { width: out_w, height: out_h, channels, data }))
}

/// Converts big-endian samples to f32, filling `out` (which may be shorter than the row).
fn decode_row(raw: &[u8], bitpix: i32, out: &mut [f32]) {
    macro_rules! convert {
        ($t:ty) => {
            for (o, b) in out.iter_mut().zip(raw.chunks_exact(size_of::<$t>())) {
                *o = <$t>::from_be_bytes(b.try_into().unwrap()) as f32;
            }
        };
    }
    match bitpix {
        8 => convert!(u8),
        16 => convert!(i16),
        32 => convert!(i32),
        64 => convert!(i64),
        -32 => convert!(f32),
        _ => convert!(f64),
    }
}
