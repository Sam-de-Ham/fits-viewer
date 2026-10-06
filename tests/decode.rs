mod common;

use common::*;
use fits_viewer::{bitmap, fits, stretch};

fn load(file: &[u8], max_side: usize) -> fits::Image {
    fits::load(file, max_side).expect("valid FITS").1
}

#[test]
fn applies_bzero_and_keeps_top_down_rows() {
    // Unsigned 16-bit as stored by capture software: value - 32768, BZERO = 32768.
    let rows = [1000, 2000, 3000];
    let data = be_i16(rows.iter().flat_map(|&v| [(v - 32768) as i16; 4]));
    let file = image("16", 4, 3, &[("BZERO", "32768"), ("ROWORDER", "'TOP-DOWN'")], &data);

    let img = load(&file, 100);
    assert_eq!((img.width, img.height, img.channels), (4, 3, 1));
    assert_eq!(img.data[0], 1000.0);
    assert_eq!(img.data[8], 3000.0);
}

#[test]
fn flips_bottom_up_rows_by_default() {
    let data = be_f32([1.0, 1.0, 2.0, 2.0, 3.0, 3.0]);
    let img = load(&image("-32", 2, 3, &[], &data), 100);
    assert_eq!(img.data, [3.0, 3.0, 2.0, 2.0, 1.0, 1.0]);
}

#[test]
fn interleaves_rgb_planes() {
    let data = be_f32([[1.0; 4], [2.0; 4], [3.0; 4]].concat());
    let cards = [("SIMPLE", "T"), ("BITPIX", "-32"), ("NAXIS", "3"), ("NAXIS1", "2"), ("NAXIS2", "2"), ("NAXIS3", "3")];
    let img = load(&common::hdu(&cards, &data), 100);
    assert_eq!(img.channels, 3);
    assert_eq!(img.data, [1.0, 2.0, 3.0].repeat(4));
}

#[test]
fn bins_bayer_cells_into_rgb_pixels() {
    // One RGGB cell: R=10, G=20 / G=30, B=40.
    let file = image("8", 2, 2, &[("BAYERPAT", "'RGGB'"), ("ROWORDER", "'TOP-DOWN'")], &[10, 20, 30, 40]);
    let img = load(&file, 100);
    assert_eq!((img.width, img.height, img.channels), (1, 1, 3));
    assert_eq!(img.data, [10.0, 25.0, 40.0]);
}

#[test]
fn honours_bayer_offsets() {
    // Shifting RGGB by one column gives GRBG.
    let cards = [("BAYERPAT", "'RGGB'"), ("XBAYROFF", "1"), ("ROWORDER", "'TOP-DOWN'")];
    let img = load(&image("8", 2, 2, &cards, &[10, 20, 30, 40]), 100);
    assert_eq!(img.data, [20.0, 25.0, 30.0]);
}

#[test]
fn box_averages_down_to_max_side() {
    let data = be_f32((0..16).map(|v| v as f32));
    let img = load(&image("-32", 4, 4, &[("ROWORDER", "'TOP-DOWN'")], &data), 2);
    assert_eq!((img.width, img.height), (2, 2));
    assert_eq!(img.data, [2.5, 4.5, 10.5, 12.5]);
}

#[test]
fn ignores_non_finite_samples() {
    let data = be_f32([1.0, f32::NAN, 3.0, f32::INFINITY]);
    let img = load(&image("-32", 2, 2, &[], &data), 1);
    assert_eq!(img.data, [2.0]);
}

#[test]
fn finds_image_in_extension() {
    let mut file = common::hdu(&[("SIMPLE", "T"), ("BITPIX", "8"), ("NAXIS", "0"), ("EXTEND", "T")], &[]);
    let cards = [("XTENSION", "'IMAGE'"), ("BITPIX", "8"), ("NAXIS", "2"), ("NAXIS1", "2"), ("NAXIS2", "2")];
    file.extend(common::hdu(&cards, &[5, 5, 5, 5]));
    assert_eq!(load(&file, 100).data, [5.0; 4]);
}

#[test]
fn reads_header_metadata() {
    let cards = [("OBJECT", "'NGC 7000'"), ("FILTER", "'Ha'"), ("EXPTIME", "300.0")];
    let (header, _) = fits::load(&image("8", 2, 2, &cards, &[0; 4])[..], 100).unwrap();
    assert_eq!(header.object.as_deref(), Some("NGC 7000"));
    assert_eq!(header.filter.as_deref(), Some("Ha"));
    assert_eq!(header.exposure, Some(300.0));
}

#[test]
fn rejects_invalid_files() {
    let valid = image("16", 4, 4, &[], &[0; 32]);
    let cases: [(&str, &[u8]); 4] = [
        ("empty", &[]),
        ("not FITS", &[b'x'; 5760]),
        ("truncated data", &valid[..2880 + 8]),
        ("unsupported BITPIX", &image("12", 2, 2, &[], &[0; 8])),
    ];
    for (name, file) in cases {
        assert!(fits::load(file, 100).is_err(), "{name} should fail");
    }
    let huge = image("16", 1 << 21, 2, &[], &[]);
    assert!(fits::load(&huge[..], 100).is_err(), "huge dimensions should fail");
}

#[test]
fn stretch_lifts_background_to_target() {
    // Dim linear "sky": a low background with a little noise.
    let pixels: Vec<f32> = noise(10_000).iter().map(|n| 0.02 + 0.005 * n).collect();
    let img = fits::Image { width: 100, height: 100, channels: 1, data: pixels };
    let bgra = stretch::to_bgra(&img);

    let mut values: Vec<u8> = bgra.chunks(4).map(|px| px[0]).collect();
    values.sort_unstable();
    let median = values[values.len() / 2];
    assert!((54..=74).contains(&median), "background at {median}, expected about 64");
    assert!(bgra.chunks(4).all(|px| px[0] == px[1] && px[1] == px[2] && px[3] == 255));
}

#[test]
fn encodes_bmp() {
    let file = image("8", 3, 2, &[], &[1, 2, 3, 4, 5, 6]);
    let (_, mono) = bitmap::render(&file[..], 100).unwrap();
    let bmp = mono.to_bmp();
    assert_eq!(&bmp[..2], b"BM");
    assert_eq!(u16::from_le_bytes([bmp[28], bmp[29]]), 8);
    assert_eq!(bmp.len(), 54 + 1024 + 4 * 2); // 3-pixel rows padded to 4 bytes

    let color = bitmap::Bitmap { width: 3, height: 2, mono: false, bgra: vec![7; 24] };
    let bmp = color.to_bmp();
    assert_eq!(u16::from_le_bytes([bmp[28], bmp[29]]), 24);
    assert_eq!(bmp.len(), 54 + 12 * 2);
}
