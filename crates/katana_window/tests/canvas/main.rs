//! The canvas backends' and the theme's cases (C, step 7 wave 1): the twins of tests/qt_widgets'
//! test_theme (the theme's five Theme cases; the title-bar two are D's and Icons is I's) and of
//! plotting/test_plot_output's TiffWriter suite - one module a C++ file, the suites as modules
//! inside - and the backends' own, against scenes worked by hand: a dashed line's ink, a filled
//! ring's hole, a text's ink inside its box, a PDF page's text extractable, a picture read back,
//! the overflow arrow's pixels and viewports through the clipboard.
//!
//! Ink is counted against a baseline drawn the same way, never as an absolute number of pixels:
//! fonts differ between machines (CLAUDE.md, "Testing").

#![allow(
    clippy::float_cmp,
    reason = "Sizes and font sizes are compared exactly where the C++ compared them exactly."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

#[path = "../support/window_support.rs"]
pub mod window_support;

mod backends;
mod plot_output;
mod theme;

use katana_window::canvas::{Color, Image};

/// EXPECT_DOUBLE_EQ: within four units in the last place.
pub fn double_eq(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    if a.is_nan() || b.is_nan() || a.is_sign_negative() != b.is_sign_negative() {
        return false;
    }
    a.to_bits().abs_diff(b.to_bits()) <= 4
}

/// Whether `pixel` is ink on `ground`: a channel more than 24 steps from the ground's, give or
/// take the antialiasing of an edge (test_theme.cpp's inkRows' test).
pub fn is_ink(pixel: Color, ground: Color) -> bool {
    pixel.r.abs_diff(ground.r) > 24
        || pixel.g.abs_diff(ground.g) > 24
        || pixel.b.abs_diff(ground.b) > 24
}

/// The pixels of `image` that are ink on `ground`.
pub fn ink(image: &Image, ground: Color) -> usize {
    let mut count = 0;
    for y in 0..image.height() {
        for x in 0..image.width() {
            if is_ink(image.pixel(x, y), ground) {
                count += 1;
            }
        }
    }
    count
}
