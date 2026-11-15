//! Twin of tests/terrain/test_colour_ramps.cpp: the one table of colour ramps
//! (docs/surface_display.md, "The ramps"): what is in it, where each ramp's
//! colours come from, and how a ramp is found.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

use std::collections::BTreeSet;

use katana_terrain::colour_ramps::colour_ramps;
use katana_terrain::{
    COLOUR_RAMP_STOPS, ColourRampStops, RampColour, colour_ramp_names, find_colour_ramp,
};

/// A colour from its web hex, as the schemes publish it.
const fn hex(value: u32) -> RampColour {
    RampColour {
        r: ((value >> 16) & 0xFF) as u8,
        g: ((value >> 8) & 0xFF) as u8,
        b: (value & 0xFF) as u8,
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> RampColour {
    RampColour { r, g, b }
}

mod colour_ramps {
    use super::*;

    #[test]
    fn the_table_has_six_ramps_of_five_stops_each_named_once_the_default_first() {
        let expected = ["terrain", "earth", "diverging", "slope", "grey", "spectral"];
        let ramps = colour_ramps();
        let names = colour_ramp_names();
        assert_eq!(ramps.len(), expected.len());
        assert_eq!(names.len(), expected.len());
        let mut seen = BTreeSet::new();
        for (i, name) in expected.iter().enumerate() {
            assert_eq!(ramps[i].name, *name);
            assert_eq!(names[i], ramps[i].name, "{i}");
            assert!(seen.insert(ramps[i].name), "{}", ramps[i].name);
            // A ramp is data with a source: no ramp is without its reason.
            assert!(!ramps[i].source.is_empty(), "{}", ramps[i].name);
            assert_eq!(ramps[i].stops.len(), COLOUR_RAMP_STOPS);
        }
        assert_eq!(COLOUR_RAMP_STOPS, 5);
    }

    #[test]
    fn a_ramp_is_found_in_any_case_and_an_unknown_or_empty_name_is_none() {
        let ramps = colour_ramps();
        assert!(find_colour_ramp("terrain").is_some_and(|ramp| std::ptr::eq(ramp, &ramps[0])));
        assert!(find_colour_ramp("TERRAIN").is_some_and(|ramp| std::ptr::eq(ramp, &ramps[0])));
        assert!(find_colour_ramp("Spectral").is_some_and(|ramp| std::ptr::eq(ramp, &ramps[5])));
        assert!(find_colour_ramp("gReY").is_some_and(|ramp| std::ptr::eq(ramp, &ramps[4])));
        assert!(find_colour_ramp("rainbow").is_none());
        assert!(find_colour_ramp("").is_none());
        // The whole name, not a prefix or a padded one.
        assert!(find_colour_ramp("terra").is_none());
        assert!(find_colour_ramp(" terrain").is_none());
        assert!(find_colour_ramp("terrain ").is_none());
    }

    #[test]
    fn the_brewer_ramps_are_their_published_five_class_schemes() {
        // Cynthia Brewer's ColorBrewer (colorbrewer2.org), five classes. YlOrBr
        // is published light to dark: #ffffd4 #fed98e #fe9929 #d95f0e #993404. RdBu
        // is published red to blue (#ca0020 #f4a582 #f7f7f7 #92c5de #0571b0) and
        // Spectral red to blue (#d7191c #fdae61 #ffffbf #abdda4 #2b83ba); a cut and
        // fill map has cut blue and low, so both are used reversed.
        let earth = find_colour_ramp("earth").expect("earth");
        let yellow_orange_brown: ColourRampStops = [
            hex(0xffffd4),
            hex(0xfed98e),
            hex(0xfe9929),
            hex(0xd95f0e),
            hex(0x993404),
        ];
        assert_eq!(earth.stops, yellow_orange_brown);

        let diverging = find_colour_ramp("diverging").expect("diverging");
        let rd_bu_reversed: ColourRampStops = [
            hex(0x0571b0),
            hex(0x92c5de),
            hex(0xf7f7f7),
            hex(0xf4a582),
            hex(0xca0020),
        ];
        assert_eq!(diverging.stops, rd_bu_reversed);

        let spectral = find_colour_ramp("spectral").expect("spectral");
        let spectral_reversed: ColourRampStops = [
            hex(0x2b83ba),
            hex(0xabdda4),
            hex(0xffffbf),
            hex(0xfdae61),
            hex(0xd7191c),
        ];
        assert_eq!(spectral.stops, spectral_reversed);
    }

    #[test]
    fn the_programs_own_ramps_are_what_their_sources_say() {
        // terrain: the 3D view's elevation ramp since 2026-09-24, deep blue to
        // snow, five stops (docs/render.md, "Surfaces").
        let terrain = find_colour_ramp("terrain").expect("terrain");
        let elevation: ColourRampStops = [
            rgb(40, 70, 140),
            rgb(60, 160, 90),
            rgb(235, 220, 130),
            rgb(200, 130, 70),
            rgb(245, 245, 245),
        ];
        assert_eq!(terrain.stops, elevation);

        // slope: green flat, yellow at half, red at the end of the range, the
        // blue held at 38; the quarter stops are the old line's value there,
        // rounded: red 2t is 0.5 at t = 1/4, which is 127.5, rounded half up to
        // 128, and green 2 - 2t at t = 3/4 likewise.
        let slope = find_colour_ramp("slope").expect("slope");
        let line: ColourRampStops = [
            rgb(0, 255, 38),
            rgb(128, 255, 38),
            rgb(255, 255, 38),
            rgb(255, 128, 38),
            rgb(255, 0, 38),
        ];
        assert_eq!(slope.stops, line);

        // grey: dark to light in equal steps of 48, so the order of the values
        // survives a monochrome plot: 32 + 48 k for k = 0 to 4 is 32, 80, 128,
        // 176, 224, and the three channels are equal.
        let grey = find_colour_ramp("grey").expect("grey");
        for (k, stop) in grey.stops.iter().enumerate() {
            let level = (32 + 48 * k) as u8;
            assert_eq!(*stop, rgb(level, level, level), "{k}");
        }
    }
}
