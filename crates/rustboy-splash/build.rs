//! Builds the two title-screen layers from the photo, into OUT_DIR, never into the repository.

use std::path::{Path, PathBuf};

use image::imageops::FilterType;

// These have to match `rustboy_core` and `lib.rs`; a test in `lib.rs` checks them.
const SCREEN_WIDTH: usize = 160;
const SCREEN_HEIGHT: usize = 144;
const LETTER: u8 = 1;
const SHADOW: u8 = 2;
const BYLINE: u8 = 3;

const CROP_TOP: u32 = 430; // where the 10:9 window starts in the tall photo
const COLOURS: usize = 24; // fewer colours means blockier
const SATURATION: f32 = 1.45; // the photo is foggy
const CONTRAST: f32 = 1.25;
const WORDS: &str = "RUST BOY COLOR";
const TEXT_SCALE: usize = 1;
const CREDIT: &str = "by dudustri"; // the smaller line, shown once the picture has gone

const GLYPH_WIDTH: usize = 5;
const GLYPH_HEIGHT: usize = 7;
const GLYPH_GAP: usize = 1;

// The byline uses a smaller alphabet so it sits below the title without competing.
const SMALL_WIDTH: usize = 3;
const SMALL_HEIGHT: usize = 5;
const GAP_BELOW_TITLE: usize = 11;

// A 5 by 7 letter for every character the title needs.
fn glyph(c: char) -> [&'static str; GLYPH_HEIGHT] {
    match c {
        'R' => [
            "11110", "10001", "10001", "11110", "10100", "10010", "10001",
        ],
        'U' => [
            "10001", "10001", "10001", "10001", "10001", "10001", "01110",
        ],
        'S' => [
            "01111", "10000", "10000", "01110", "00001", "00001", "11110",
        ],
        'T' => [
            "11111", "00100", "00100", "00100", "00100", "00100", "00100",
        ],
        'B' => [
            "11110", "10001", "10001", "11110", "10001", "10001", "11110",
        ],
        'O' => [
            "01110", "10001", "10001", "10001", "10001", "10001", "01110",
        ],
        'Y' => [
            "10001", "10001", "01010", "00100", "00100", "00100", "00100",
        ],
        'C' => [
            "01110", "10001", "10000", "10000", "10000", "10001", "01110",
        ],
        'L' => [
            "10000", "10000", "10000", "10000", "10000", "10000", "11111",
        ],
        _ => ["00000"; GLYPH_HEIGHT],
    }
}

// A 3 by 5 box per byline character. Lower case, so most letters only fill the middle rows.
fn small_glyph(c: char) -> [&'static str; SMALL_HEIGHT] {
    match c {
        'b' => ["100", "110", "101", "110", "000"],
        'y' => ["000", "101", "101", "011", "110"],
        'd' => ["001", "011", "101", "011", "000"],
        'u' => ["000", "101", "101", "111", "000"],
        's' => ["000", "011", "010", "110", "000"],
        't' => ["010", "111", "010", "011", "000"],
        'r' => ["000", "110", "101", "100", "000"],
        'i' => ["010", "000", "010", "010", "000"],
        _ => ["000"; SMALL_HEIGHT],
    }
}

fn text_width(words: &str, scale: usize) -> usize {
    (words.chars().count() * (GLYPH_WIDTH + GLYPH_GAP) - GLYPH_GAP) * scale
}

fn small_width(words: &str) -> usize {
    words.chars().count() * (SMALL_WIDTH + GLYPH_GAP) - GLYPH_GAP
}

// Same idea as `stamp`, with the smaller alphabet and no shadow.
fn stamp_small(mask: &mut [u8], words: &str, x0: usize, y0: usize, value: u8) {
    let mut x = x0;
    for c in words.chars() {
        for (row, bits) in small_glyph(c).iter().enumerate() {
            for (column, bit) in bits.bytes().enumerate() {
                if bit == b'1' {
                    let (px, py) = (x + column, y0 + row);
                    if px < SCREEN_WIDTH && py < SCREEN_HEIGHT {
                        mask[py * SCREEN_WIDTH + px] = value;
                    }
                }
            }
        }
        x += SMALL_WIDTH + GLYPH_GAP;
    }
}

// Paint the words into the mask, one glyph pixel at a time.
fn stamp(mask: &mut [u8], words: &str, x0: usize, y0: usize, scale: usize, value: u8) {
    let mut x = x0;
    for c in words.chars() {
        for (row, bits) in glyph(c).iter().enumerate() {
            for (column, bit) in bits.bytes().enumerate() {
                if bit != b'1' {
                    continue;
                }
                for dy in 0..scale {
                    for dx in 0..scale {
                        let px = x + column * scale + dx;
                        let py = y0 + row * scale + dy;
                        if px < SCREEN_WIDTH && py < SCREEN_HEIGHT {
                            mask[py * SCREEN_WIDTH + px] = value;
                        }
                    }
                }
            }
        }
        x += (GLYPH_WIDTH + GLYPH_GAP) * scale;
    }
}

// Pull each channel away from grey, then away from the picture's average.
fn punch_up(pixels: &mut [u8]) {
    let mut total = 0.0;
    for chunk in pixels.chunks_exact(3) {
        total += 0.299 * chunk[0] as f32 + 0.587 * chunk[1] as f32 + 0.114 * chunk[2] as f32;
    }
    let mean = total / (pixels.len() / 3) as f32;

    for chunk in pixels.chunks_exact_mut(3) {
        let grey = 0.299 * chunk[0] as f32 + 0.587 * chunk[1] as f32 + 0.114 * chunk[2] as f32;
        for channel in chunk.iter_mut() {
            let saturated = grey + (*channel as f32 - grey) * SATURATION;
            let contrasted = mean + (saturated - mean) * CONTRAST;
            *channel = contrasted.clamp(0.0, 255.0) as u8;
        }
    }
}

fn main() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/source.jpeg");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed={}", source.display());

    let photo = match image::open(&source) {
        Ok(photo) => photo,
        Err(error) => {
            eprintln!("could not read the photo: {error}");
            std::process::exit(1);
        }
    };

    // Crop a 10:9 window out of the tall photo, then shrink it to screen size.
    let width = photo.width();
    let height = width * SCREEN_HEIGHT as u32 / SCREEN_WIDTH as u32;
    let cropped = photo.crop_imm(0, CROP_TOP, width, height);
    let small = cropped.resize_exact(
        SCREEN_WIDTH as u32,
        SCREEN_HEIGHT as u32,
        FilterType::Lanczos3,
    );

    let mut rgb = small.to_rgb8().into_raw();
    punch_up(&mut rgb);

    // Reduce to a handful of colours, which is what gives the blocky look.
    let quantizer = color_quant::NeuQuant::new(10, COLOURS, &to_rgba(&rgb));
    let mut picture = Vec::with_capacity(SCREEN_WIDTH * SCREEN_HEIGHT * 4);
    for chunk in rgb.chunks_exact(3) {
        let entry = quantizer.index_of(&[chunk[0], chunk[1], chunk[2], 0xFF]);
        let palette = quantizer.color_map_rgba();
        picture.extend_from_slice(&palette[entry * 4..entry * 4 + 3]);
        picture.push(0xFF);
    }

    // The words live in their own layer so the picture can fade out from under them.
    let mut mask = vec![0u8; SCREEN_WIDTH * SCREEN_HEIGHT];
    let text_width = text_width(WORDS, TEXT_SCALE);
    let x0 = (SCREEN_WIDTH - text_width) / 2;
    let y0 = (SCREEN_HEIGHT - GLYPH_HEIGHT * TEXT_SCALE) / 2;
    stamp(
        &mut mask,
        WORDS,
        x0 + TEXT_SCALE,
        y0 + TEXT_SCALE,
        TEXT_SCALE,
        SHADOW,
    );
    stamp(&mut mask, WORDS, x0, y0, TEXT_SCALE, LETTER);

    // Centre the byline just below the title.
    let credit_width = small_width(CREDIT);
    let credit_x = (SCREEN_WIDTH - credit_width) / 2;
    let credit_y = y0 + GLYPH_HEIGHT * TEXT_SCALE + GAP_BELOW_TITLE;
    stamp_small(&mut mask, CREDIT, credit_x, credit_y, BYLINE);

    write(&out.join("splash.rgba"), &picture);
    write(&out.join("splash_text.mask"), &mask);
}

// The quantizer wants four channels, the resize gave us three.
fn to_rgba(rgb: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgb.len() / 3 * 4);
    for chunk in rgb.chunks_exact(3) {
        out.extend_from_slice(chunk);
        out.push(0xFF);
    }
    out
}

fn write(path: &Path, bytes: &[u8]) {
    if let Err(error) = std::fs::write(path, bytes) {
        eprintln!("could not write {}: {error}", path.display());
        std::process::exit(1);
    }
}
