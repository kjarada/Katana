//! The plot style on the canvas (docs/plotting.md, "Plot styles and output"): the port of
//! plotting/plot_style.
//!
//! The rule itself is the model's - cad's paper_colour for a pen and paper_fill_colour for a fill,
//! both in plot.rs - so that the plan painter, the sheet painter and anything drawn later agree on
//! what a colour becomes. These are its canvas Color and Image forms, for the painters: the sheet
//! painter's pens, text and fills all pass through plot_ink and plot_fill, and every image it
//! places (the logo, an image viewport, the 3D snapshot) through plot_image.

use katana_cad::plot::{PlotColourMode, PlotSettings, luminance, paper_colour, paper_fill_colour};
use katana_entity::Color as EntityColor;

use crate::canvas::{Color, Image};

fn model_colour(colour: Color) -> EntityColor {
    EntityColor {
        r: colour.r,
        g: colour.g,
        b: colour.b,
        a: colour.a,
    }
}

fn canvas_colour(colour: EntityColor) -> Color {
    Color::rgba(colour.r, colour.g, colour.b, colour.a)
}

/// The one grey a pixel becomes, whatever the image.
fn grey_of(pixel: Color) -> u8 {
    luminance(EntityColor {
        r: pixel.r,
        g: pixel.g,
        b: pixel.b,
        a: 255,
    })
}

/// A pen's or a letter's colour on paper under `plot` (cad's paper_colour).
pub fn plot_ink(colour: Color, plot: &PlotSettings) -> Color {
    canvas_colour(paper_colour(model_colour(colour), plot))
}

/// An area fill's colour on paper (cad's paper_fill_colour). The paper's own white - a knock-out
/// behind a label, the page - is not a fill and is never passed through this.
pub fn plot_fill(colour: Color, plot: &PlotSettings) -> Color {
    canvas_colour(paper_fill_colour(model_colour(colour), plot))
}

/// An image as the style prints it: unchanged in Colour; in Greyscale and Monochrome each pixel
/// the grey of its luminance (cad's luminance), with its alpha kept, so a logo's transparent
/// ground stays transparent. A photograph is not thresholded to black and white in Monochrome -
/// that loses the picture - but printed grey, as a one-ink printer's halftone prints it. A null
/// image stays null.
pub fn plot_image(image: &Image, plot: &PlotSettings) -> Image {
    if image.is_null() || plot.colour_mode == PlotColourMode::Colour {
        return image.clone();
    }
    // Read straight (Image::pixel divides the alpha out), so the luminance is the colour's and not
    // the colour darkened by its own transparency; written back premultiplied.
    let mut grey = Image::new(image.width(), image.height());
    grey.set_device_pixel_ratio(image.device_pixel_ratio());
    for y in 0..image.height() {
        for x in 0..image.width() {
            let pixel = image.pixel(x, y);
            let level = grey_of(pixel);
            grey.set_pixel(x, y, Color::rgba(level, level, level, pixel.a));
        }
    }
    grey
}

/// A painted sheet as one grey channel, each pixel the cad luminance of its colour: what a PNG or
/// TIFF of a greyscale or monochrome plot is written as, a third the size of RGB. Computed here
/// rather than by a library's conversion, whose grey follows the image's colour space and the
/// library's version, so the same sheet always gives the same bytes. Alpha is ignored: a sheet
/// is opaque. A null image stays null. (The C++ also carries the image's dots per metre across;
/// Image keeps none, and the writers are given the resolution.)
pub fn greyscale_raster(image: &Image) -> Image {
    if image.is_null() {
        return image.clone();
    }
    let mut grey = Image::new_grey(image.width(), image.height());
    grey.set_device_pixel_ratio(image.device_pixel_ratio());
    let width = image.width() as usize;
    for y in 0..image.height() {
        for x in 0..image.width() {
            let level = grey_of(image.pixel(x, y));
            grey.bytes_mut()[y as usize * width + x as usize] = level;
        }
    }
    grey
}
