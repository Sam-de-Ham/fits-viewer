//! Builds small FITS files in memory for the tests.

// Each test crate uses a different subset of these helpers.
#![allow(dead_code)]

/// A FITS header-and-data unit. Card values are written verbatim, so quote strings: `"'RGGB'"`.
pub fn hdu(cards: &[(&str, &str)], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for (key, value) in cards {
        out.extend(format!("{key:<8}= {value:>20}{:50}", "").bytes().take(80));
    }
    out.extend(format!("{:<80}", "END").bytes());
    pad(&mut out, b' ');
    out.extend(data);
    pad(&mut out, 0);
    out
}

fn pad(out: &mut Vec<u8>, fill: u8) {
    out.resize(out.len().next_multiple_of(2880), fill);
}

/// A 2D image with the given BITPIX and extra cards.
pub fn image(bitpix: &str, width: usize, height: usize, extra: &[(&str, &str)], data: &[u8]) -> Vec<u8> {
    let (w, h) = (width.to_string(), height.to_string());
    let mut cards = vec![("SIMPLE", "T"), ("BITPIX", bitpix), ("NAXIS", "2"), ("NAXIS1", &w), ("NAXIS2", &h)];
    cards.extend_from_slice(extra);
    hdu(&cards, data)
}

pub fn be_i16(values: impl IntoIterator<Item = i16>) -> Vec<u8> {
    values.into_iter().flat_map(i16::to_be_bytes).collect()
}

pub fn be_f32(values: impl IntoIterator<Item = f32>) -> Vec<u8> {
    values.into_iter().flat_map(f32::to_be_bytes).collect()
}

/// Deterministic pseudo-random noise in `0..1`.
pub fn noise(len: usize) -> Vec<f32> {
    let mut state = 0x2545_f491_u32;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as f32 / u32::MAX as f32
        })
        .collect()
}
