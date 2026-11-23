//! Decoding UTF-16 text, measured (CLAUDE.md section 5; the practice of docs/testing.md,
//! "Benchmarks"; docs/performance.md, "The safe UTF-16 append"): what an archive or a survey file
//! saved as UTF-16 pays to become the program's text, and so what the append of its runs of ASCII
//! costs - the part that was made safe on 2026-11-22 at the price of a second pass over each run.
//!
//! The texts are generated here, 4 000 000 units each (8 MB): lines as a point file's are, with
//! another character put in at a fixed spacing. Only `decode_text` is used, so this source
//! measures the base build and the changed one alike. Cells, alternated round by round with the
//! order reversed every round, each round taking `REPEATS` samples of each:
//!
//! * `ascii_le` and `ascii_le_again`: little-endian, no other character at all - one run, the
//!   whole text - under two names, the A/A control;
//! * `sparse_le`: little-endian, a degree sign every 200 units, as a file of bearings has;
//! * `dense_le`: little-endian, another character every 8 units, where a run is shorter than a
//!   kernel's block and the append is asked for at every one;
//! * `sparse_be`: big-endian, as `sparse_le`.
//!
//! Prints one `cell=... min_ms=... median_ms=... samples=...` line a cell. Run in Release:
//! `cargo bench --locked -p katana_core --bench decode_utf16` (or through
//! `cargo test --release`, which runs it too, one round); `KATANA_BENCH_ROUNDS` sets the rounds
//! (default 7); `KATANA_SIMD` chooses the kernels' level as it does for the program.

use std::hint::black_box;
use std::time::Instant;

use katana_core::text_encoding::decode_text;

const UNITS: usize = 4_000_000;
const REPEATS: usize = 3;
/// The line every text repeats: a point file's, 40 units.
const LINE: &str = "1001,305123.456,6250789.012,45.678,KERB\n";
/// U+00B0, the degree sign: one unit of UTF-16 that is not ASCII.
const DEGREE: u16 = 0x00b0;

/// A text of `UNITS` units behind its byte order mark, a degree sign at every `spacing`th unit
/// (none for a spacing of 0).
fn text(little_endian: bool, spacing: usize) -> Vec<u8> {
    let line: Vec<u16> = LINE.encode_utf16().collect();
    let mut bytes = Vec::with_capacity(2 * UNITS + 2);
    let mark: u16 = 0xfeff;
    let mut put = |unit: u16| {
        bytes.extend_from_slice(&if little_endian {
            unit.to_le_bytes()
        } else {
            unit.to_be_bytes()
        });
    };
    put(mark);
    for index in 0..UNITS {
        if spacing != 0 && index % spacing == spacing - 1 {
            put(DEGREE);
        } else {
            put(line[index % line.len()]);
        }
    }
    bytes
}

/// One sample: `bytes` decoded once, in milliseconds.
fn sample(bytes: &[u8]) -> f64 {
    let started = Instant::now();
    let decoded = decode_text(black_box(bytes)).expect("the generated text is sound UTF-16");
    let taken = started.elapsed().as_secs_f64() * 1000.0;
    // Every unit is one character here, so the count is the units'.
    assert_eq!(black_box(decoded.text.chars().count()), UNITS);
    taken
}

fn main() {
    let rounds: usize = std::env::var("KATANA_BENCH_ROUNDS")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(7);
    // `cargo test --release` runs a bench target as a test with `--bench` absent: once, quickly.
    let quick = !std::env::args().any(|argument| argument == "--bench");
    let rounds = if quick { 1 } else { rounds.max(1) };
    let ascii = text(true, 0);
    let sparse = text(true, 200);
    let dense = text(true, 8);
    let sparse_big = text(false, 200);
    let mut cells = [
        "ascii_le",
        "sparse_le",
        "dense_le",
        "sparse_be",
        "ascii_le_again",
    ];
    let mut samples: Vec<(&str, Vec<f64>)> = cells.iter().map(|cell| (*cell, Vec::new())).collect();
    for round in 0..rounds {
        if round % 2 == 1 {
            cells.reverse();
        }
        for cell in cells {
            for _ in 0..REPEATS {
                let taken = sample(match cell {
                    "sparse_le" => &sparse,
                    "dense_le" => &dense,
                    "sparse_be" => &sparse_big,
                    _ => &ascii,
                });
                if let Some((_, kept)) = samples.iter_mut().find(|(name, _)| *name == cell) {
                    kept.push(taken);
                }
            }
        }
        if round % 2 == 1 {
            cells.reverse();
        }
    }
    println!("units={UNITS}");
    for (cell, mut kept) in samples {
        kept.sort_by(f64::total_cmp);
        let median = kept[kept.len() / 2];
        println!(
            "cell={cell} min_ms={:.3} median_ms={:.3} samples={}",
            kept[0],
            median,
            kept.len()
        );
    }
}
