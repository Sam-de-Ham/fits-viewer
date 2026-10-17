//! Automatic screen stretch, modelled on PixInsight's AutoSTF.
//!
//! Raw astro frames are linear: nearly everything sits just above black. For each channel the
//! background level (median) and noise (MAD) are measured, the black point is placed just below
//! the background, and a midtones transfer curve lifts the background to a fixed grey level.

use crate::fits::Image;

/// Black point, in normalized MADs below the median.
const SHADOWS_CLIP: f32 = -2.8;
/// Brightness the background is mapped to.
const TARGET_BACKGROUND: f32 = 0.25;

/// Midtones transfer function: maps `m` to 0.5 while keeping 0 and 1 fixed.
fn mtf(m: f32, x: f32) -> f32 {
    match x {
        ..=0.0 => 0.0,
        1.0.. => 1.0,
        _ => (m - 1.0) * x / ((2.0 * m - 1.0) * x - m),
    }
}

fn median(values: &mut [f32]) -> f32 {
    let mid = values.len() / 2;
    *values.select_nth_unstable_by(mid, f32::total_cmp).1
}

/// Black point and midtones balance for one channel, from its full-resolution samples.
fn parameters(samples: &[f32], normalize: impl Fn(f32) -> f32) -> (f32, f32) {
    let mut values: Vec<f32> = samples.iter().map(|&v| normalize(v)).collect();
    if values.is_empty() {
        return (0.0, 0.5);
    }
    let med = median(&mut values);
    values.iter_mut().for_each(|v| *v = (*v - med).abs());
    let mad = 1.4826 * median(&mut values);
    let black = (med + SHADOWS_CLIP * mad).clamp(0.0, 1.0);
    (black, mtf(TARGET_BACKGROUND, med - black))
}

/// Stretches `image` to 8-bit BGRA pixels (alpha 255), rows top to bottom.
///
/// The stretch is computed from the full-resolution samples, so a thumbnail and a full-size
/// rendering of the same file look the same. Channels share one normalization, preserving their
/// relative brightness, but are stretched independently, which neutralizes the sky background
/// color of one-shot-color frames.
#[must_use]
pub fn to_bgra(image: &Image) -> Vec<u8> {
    let (lo, hi) = image.data.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    let range = if hi > lo { hi - lo } else { 1.0 };
    let normalize = |v: f32| (v - lo) / range;

    let mut bgra = vec![255u8; image.width * image.height * 4];
    for channel in 0..image.channels {
        let (black, midtones) = parameters(&image.samples[channel], normalize);
        // Mono fills B, G and R; color channels arrive as R, G, B and land in BGRA order.
        let targets: &[usize] = if image.channels == 1 { &[0, 1, 2] } else { &[[2, 1, 0][channel]] };
        for (pixel, &v) in image.data.iter().skip(channel).step_by(image.channels).enumerate() {
            let x = (normalize(v) - black) / (1.0 - black);
            let out = (mtf(midtones, x) * 255.0).round() as u8;
            for &t in targets {
                bgra[pixel * 4 + t] = out;
            }
        }
    }
    bgra
}
