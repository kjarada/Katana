//! The named colour ramps: the ONE table (docs/surface_display.md, "The
//! ramps"). A surface's display style names one of them, the 3D scene's
//! elevation and slope colours are two of them, and RASTER SHADE and RASTER
//! SLOPE paint with the same five colours, so `ramp=terrain` is one map on a
//! surface style, on a shaded raster and in a legend.
//!
//! It is plain data: three bytes a colour and no colour type of the drawing's.
//! That is what lets it sit in the lowest layer that both `cad` and `interop`
//! may see (tools/check_layering.cmake), so neither has a table of its own.
//!
//! Every ramp is five colours evenly spaced over 0 to 1 and blended linearly
//! between them. Five, because that is the form the 3D scene's vector kernels
//! read (four segments a channel), so a surface coloured smoothly by ANY ramp
//! takes the kernel path. How a colour is blended at a value stays with the
//! caller.

use katana_core::text::equals_ignoring_case;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RampColour {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

pub const COLOUR_RAMP_STOPS: usize = 5;
pub type ColourRampStops = [RampColour; COLOUR_RAMP_STOPS];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NamedColourRamp {
    pub name: &'static str,
    pub stops: ColourRampStops,
    /// Where the five colours come from: a published scheme, or the program's
    /// own choice and why. A ramp is data with a source, not a taste.
    pub source: &'static str,
}

const fn rgb(r: u8, g: u8, b: u8) -> RampColour {
    RampColour { r, g, b }
}

const RAMP_TABLE: [NamedColourRamp; 6] = [
    // FIRST, so it is the default and what an unknown name falls back to.
    // Unchanged to the bit, so the scene's colours and its kernels' are what
    // they were.
    NamedColourRamp {
        name: "terrain",
        stops: [
            rgb(40, 70, 140),
            rgb(60, 160, 90),
            rgb(235, 220, 130),
            rgb(200, 130, 70),
            rgb(245, 245, 245),
        ],
        source: "the 3D view's one elevation ramp since 2026-09-24 (docs/render.md, \"Surfaces\"): deep blue, green, sand, brown, snow - a hypsometric tint whose neighbouring bands stay distinguishable",
    },
    NamedColourRamp {
        name: "earth",
        stops: [
            rgb(255, 255, 212),
            rgb(254, 217, 142),
            rgb(254, 153, 41),
            rgb(217, 95, 14),
            rgb(153, 52, 4),
        ],
        source: "ColorBrewer YlOrBr, five classes (Cynthia Brewer, colorbrewer2.org): pale yellow low to dark brown high, sequential and legible in grey",
    },
    NamedColourRamp {
        name: "diverging",
        stops: [
            rgb(5, 113, 176),
            rgb(146, 197, 222),
            rgb(247, 247, 247),
            rgb(244, 165, 130),
            rgb(202, 0, 32),
        ],
        source: "ColorBrewer RdBu, five classes, reversed so that low (cut, a difference below zero) is blue and high (fill) red, white between: a cut/fill map's convention",
    },
    NamedColourRamp {
        name: "slope",
        stops: [
            rgb(0, 255, 38),
            rgb(128, 255, 38),
            rgb(255, 255, 38),
            rgb(255, 128, 38),
            rgb(255, 0, 38),
        ],
        source: "the 3D view's slope colours since 2026-09-24: green flat, yellow at half, red at the end of the range; the quarter stops are that line's value there, rounded",
    },
    NamedColourRamp {
        name: "grey",
        stops: [
            rgb(32, 32, 32),
            rgb(80, 80, 80),
            rgb(128, 128, 128),
            rgb(176, 176, 176),
            rgb(224, 224, 224),
        ],
        source: "chosen here: dark to light in equal steps of 48, so the order of the values survives a monochrome plot",
    },
    NamedColourRamp {
        name: "spectral",
        stops: [
            rgb(43, 131, 186),
            rgb(171, 221, 164),
            rgb(255, 255, 191),
            rgb(253, 174, 97),
            rgb(215, 25, 28),
        ],
        source: "ColorBrewer Spectral, five classes, reversed: blue low to red high, the most hues of any ramp here for telling many heights apart",
    },
];

static RAMPS: [NamedColourRamp; 6] = RAMP_TABLE;

static NAMES: [&str; 6] = [
    RAMP_TABLE[0].name,
    RAMP_TABLE[1].name,
    RAMP_TABLE[2].name,
    RAMP_TABLE[3].name,
    RAMP_TABLE[4].name,
    RAMP_TABLE[5].name,
];

/// Every ramp, the default first ("terrain"): terrain, earth, diverging (cut
/// blue to fill red), slope (green to red), grey, spectral. The order is the
/// order the verbs and the style form list them in.
pub fn colour_ramps() -> &'static [NamedColourRamp] {
    &RAMPS
}

/// The names of colour_ramps(), in its order.
pub fn colour_ramp_names() -> &'static [&'static str] {
    &NAMES
}

/// The ramp of that name, in any case; None for none. The one lookup: a
/// surface style's ramp and RASTER SHADE's ramp= both find a ramp through it.
pub fn find_colour_ramp(name: &str) -> Option<&'static NamedColourRamp> {
    RAMPS
        .iter()
        .find(|ramp| equals_ignoring_case(ramp.name.as_bytes(), name.as_bytes()))
}
