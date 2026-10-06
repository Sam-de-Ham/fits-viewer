//! Renders a FITS file through the same pipeline as the shell extension and saves it as a BMP,
//! for checking the decoder and stretch without installing anything.
//!
//! ```text
//! cargo run --release --example render -- <input.fits> <output.bmp> [max-side]
//! ```

use std::{env, fs, io::BufReader, process::ExitCode, time::Instant};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let [input, output, rest @ ..] = args.as_slice() else {
        eprintln!("usage: render <input.fits> <output.bmp> [max-side]");
        return ExitCode::FAILURE;
    };
    let max_side = rest.first().and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);

    let start = Instant::now();
    let result = fs::File::open(input)
        .and_then(|file| fits_viewer::bitmap::render(BufReader::new(file), max_side))
        .and_then(|(header, bitmap)| {
            let elapsed = start.elapsed();
            fs::write(output, bitmap.to_bmp())?;
            println!(
                "{}x{} {:?} -> {}x{} in {elapsed:.1?}",
                header.width, header.height, header.bayer, bitmap.width, bitmap.height
            );
            Ok(())
        });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{input}: {error}");
            ExitCode::FAILURE
        }
    }
}
