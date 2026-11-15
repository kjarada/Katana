// The ramp table and the colouring rule a surface's style gives its faces
// (terrain/surface_colouring.rs, docs/surface_display.md;
// tests/cad/terrain/test_surface_colouring.cpp). Every expected colour is a
// published ramp's stop, the old scene constant it replaces, or a blend worked
// out by hand below it.

use crate::{near, tin};
use katana_cad::scene::elevation_ramp_color;
use katana_cad::terrain::surface_colouring::{
    BandPiece, SurfaceColouring, aspect_sector, aspect_sector_name, band_pieces,
    format_legend_value, is_surface_ramp, legend_decimals, steepest_slope_percent,
    surface_colouring, surface_colouring_for, surface_legend, surface_ramp_colour_of,
    surface_ramp_stops,
};
use katana_cad::terrain::surface_style::{
    ColourStop, MAXIMUM_SURFACE_BANDS, SurfaceDisplayStyle, SurfaceFaces, surface_ramp_names,
};
use katana_entity::Color;
use katana_geometry::Point3;
use katana_terrain::TinSurface;

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::new(r, g, b, 255)
}

fn banded_ramp(low: f64, high: f64, bands: i32) -> SurfaceColouring {
    SurfaceColouring {
        faces: SurfaceFaces::Elevation,
        ramp: *surface_ramp_stops("terrain"),
        banded: true,
        bands,
        low,
        high,
        ..SurfaceColouring::default()
    }
}

fn piece_area(piece: &BandPiece) -> f64 {
    // The shoelace formula in plan.
    let mut twice = 0.0;
    for i in 0..piece.count {
        let a = piece.corners[i];
        let b = piece.corners[(i + 1) % piece.count];
        twice += a.x * b.y - b.x * a.y;
    }
    0.5 * f64::abs(twice)
}

fn stop(value: f64, colour: Color) -> ColourStop {
    ColourStop { value, colour }
}

fn square_rising(z: [f64; 4]) -> TinSurface {
    tin(
        vec![
            Point3::new(0.0, 0.0, z[0]),
            Point3::new(10.0, 0.0, z[1]),
            Point3::new(10.0, 10.0, z[2]),
            Point3::new(0.0, 10.0, z[3]),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
    )
}

mod surface_ramps {
    use super::*;

    #[test]
    fn the_terrain_ramp_is_the_scenes_old_elevation_ramp_to_the_bit() {
        // The scene's palette before the table: ramp red {40, 60, 235, 200,
        // 245}, green {70, 160, 220, 130, 245}, blue {140, 90, 130, 70, 245}.
        assert_eq!(surface_ramp_colour_of("terrain", 0.0), rgb(40, 70, 140));
        assert_eq!(surface_ramp_colour_of("terrain", 0.25), rgb(60, 160, 90));
        assert_eq!(surface_ramp_colour_of("terrain", 0.5), rgb(235, 220, 130));
        assert_eq!(surface_ramp_colour_of("terrain", 0.75), rgb(200, 130, 70));
        assert_eq!(surface_ramp_colour_of("terrain", 1.0), rgb(245, 245, 245));
        // Between stops: at 0.125, half way from (40, 70, 140) to (60, 160,
        // 90), each channel rounded half up: 50, 115, 115.
        assert_eq!(surface_ramp_colour_of("terrain", 0.125), rgb(50, 115, 115));
        // Clamped, and not finite is the start.
        assert_eq!(surface_ramp_colour_of("terrain", -3.0), rgb(40, 70, 140));
        assert_eq!(surface_ramp_colour_of("terrain", 7.0), rgb(245, 245, 245));
        assert_eq!(
            surface_ramp_colour_of("terrain", f64::NAN),
            rgb(40, 70, 140)
        );
        // And the scene's own function is the table's, everywhere.
        for i in 0..=1000 {
            let t = f64::from(i) / 1000.0;
            let colour = surface_ramp_colour_of("terrain", t);
            assert_eq!(
                elevation_ramp_color(t),
                katana_render::rgba(colour.r, colour.g, colour.b),
                "{t}"
            );
        }
    }

    #[test]
    fn the_table_names_six_ramps_and_an_unknown_name_is_the_first() {
        let expected = ["terrain", "earth", "diverging", "slope", "grey", "spectral"];
        let names = surface_ramp_names();
        assert_eq!(names.len(), expected.len());
        for (name, expected) in names.iter().zip(expected) {
            assert_eq!(*name, expected);
        }
        assert!(is_surface_ramp("SPECTRAL"));
        assert!(!is_surface_ramp("rainbow"));
        assert_eq!(surface_ramp_colour_of("rainbow", 0.0), rgb(40, 70, 140));
    }

    #[test]
    fn the_diverging_ramp_is_color_brewers_rd_bu_reversed_blue_low_white_middle_red_high() {
        // RdBu, five classes: #ca0020 #f4a582 #f7f7f7 #92c5de #0571b0, reversed.
        assert_eq!(
            surface_ramp_colour_of("diverging", 0.0),
            rgb(0x05, 0x71, 0xb0)
        );
        assert_eq!(
            surface_ramp_colour_of("diverging", 0.5),
            rgb(0xf7, 0xf7, 0xf7)
        );
        assert_eq!(
            surface_ramp_colour_of("diverging", 1.0),
            rgb(0xca, 0x00, 0x20)
        );
    }

    #[test]
    fn the_slope_ramp_keeps_the_scenes_old_slope_colours_at_its_stops() {
        // The old line: red = 2t, green = 2 - 2t, blue = 0.15, each clamped to
        // 0..1, times 255 and rounded half up - so blue is 38 (38.75 truncated
        // after the half is added to 38.25), and at t = 0.25 red is 128.
        assert_eq!(surface_ramp_colour_of("slope", 0.0), rgb(0, 255, 38));
        assert_eq!(surface_ramp_colour_of("slope", 0.25), rgb(128, 255, 38));
        assert_eq!(surface_ramp_colour_of("slope", 0.5), rgb(255, 255, 38));
        assert_eq!(surface_ramp_colour_of("slope", 0.75), rgb(255, 128, 38));
        assert_eq!(surface_ramp_colour_of("slope", 1.0), rgb(255, 0, 38));
    }

    #[test]
    fn every_style_ramp_is_the_one_tables_ramp_at_full_alpha_and_its_names_are_the_tables() {
        // The styles hold no colours of their own: each ramp is the terrain
        // layer's table row (katana_terrain's colour_ramps), the same one
        // RASTER SHADE paints with, with alpha 255 - and the names a style may
        // use are its names, in its order.
        let table = katana_terrain::colour_ramps::colour_ramps();
        let names = surface_ramp_names();
        assert_eq!(names.len(), table.len());
        for (r, ramp) in table.iter().enumerate() {
            assert_eq!(names[r], ramp.name);
            let stops = surface_ramp_stops(ramp.name);
            for (i, stop) in stops.iter().enumerate() {
                assert_eq!(
                    *stop,
                    rgb(ramp.stops[i].r, ramp.stops[i].g, ramp.stops[i].b),
                    "{} {i}",
                    ramp.name
                );
            }
        }
    }

    #[test]
    fn a_ramps_stops_are_one_object_whatever_the_case_of_the_name_and_an_unknown_name_is_the_firsts_object()
     {
        // The scene keeps a reference to a ramp's stops (surface_ramp_stops),
        // so a lookup must hand back the one object, not a copy that dies.
        assert!(std::ptr::eq(
            surface_ramp_stops("terrain"),
            surface_ramp_stops("TERRAIN")
        ));
        assert!(!std::ptr::eq(
            surface_ramp_stops("terrain"),
            surface_ramp_stops("grey")
        ));
        assert!(std::ptr::eq(
            surface_ramp_stops("rainbow"),
            surface_ramp_stops("terrain")
        ));
        assert!(std::ptr::eq(
            surface_ramp_stops(""),
            surface_ramp_stops("terrain")
        ));
    }
}

mod surface_aspect {
    use super::*;

    #[test]
    fn each_sector_is_centred_on_its_compass_point() {
        const DEGREE: f64 = std::f64::consts::PI / 180.0;
        assert_eq!(aspect_sector(0.0), 0);
        // Either side of each edge by a tenth of a degree: exactly on an edge
        // is a matter of the last bit of pi.
        assert_eq!(aspect_sector(22.4 * DEGREE), 0);
        assert_eq!(aspect_sector(22.6 * DEGREE), 1);
        assert_eq!(aspect_sector(45.0 * DEGREE), 1);
        assert_eq!(aspect_sector(90.0 * DEGREE), 2);
        assert_eq!(aspect_sector(180.0 * DEGREE), 4);
        assert_eq!(aspect_sector(270.0 * DEGREE), 6);
        assert_eq!(aspect_sector(337.4 * DEGREE), 7);
        assert_eq!(aspect_sector(337.6 * DEGREE), 0);
        assert_eq!(aspect_sector(354.0 * DEGREE), 0);
        assert_eq!(aspect_sector(-90.0 * DEGREE), 6);
        assert_eq!(aspect_sector(f64::NAN), 0);
        assert_eq!(aspect_sector_name(0), "N");
        assert_eq!(aspect_sector_name(6), "W");
        assert_eq!(aspect_sector_name(9), "NE"); // modulo eight
    }
}

mod surface_colouring {
    use super::*;

    #[test]
    fn a_banded_ramp_has_equal_bands_and_its_end_bands_are_the_ramps_ends() {
        let colouring = banded_ramp(0.0, 10.0, 5);
        assert_eq!(colouring.band_count(), 5);
        assert_eq!(colouring.band_of(0.0), 0);
        assert_eq!(colouring.band_of(1.99), 0);
        assert_eq!(colouring.band_of(2.0), 1);
        assert_eq!(colouring.band_of(9.99), 4);
        assert_eq!(colouring.band_of(10.0), 4);
        assert_eq!(colouring.band_of(-5.0), 0); // below the range: the first band
        assert_eq!(colouring.band_of(15.0), 4); // above it: the last
        // Band k is the ramp at k / 4.
        assert_eq!(colouring.band_colour(0), rgb(40, 70, 140));
        assert_eq!(colouring.band_colour(2), rgb(235, 220, 130));
        assert_eq!(colouring.band_colour(4), rgb(245, 245, 245));
        assert_eq!(colouring.colour_at(5.0), rgb(235, 220, 130));
        assert_eq!(colouring.band_low(1), Some(2.0));
        assert_eq!(colouring.band_high(4), Some(10.0));
    }

    #[test]
    fn stops_override_the_ramp_and_a_band_runs_from_its_stop_to_the_next() {
        let mut colouring = banded_ramp(0.0, 100.0, 10);
        colouring.stops = vec![
            stop(0.0, rgb(255, 0, 0)),
            stop(2.0, rgb(0, 255, 0)),
            stop(5.0, rgb(0, 0, 255)),
        ];
        assert_eq!(colouring.band_count(), 3);
        assert_eq!(colouring.colour_at(1.0), rgb(255, 0, 0));
        assert_eq!(colouring.colour_at(2.0), rgb(0, 255, 0));
        assert_eq!(colouring.colour_at(4.99), rgb(0, 255, 0));
        assert_eq!(colouring.colour_at(100.0), rgb(0, 0, 255));
        assert_eq!(colouring.colour_at(-1.0), rgb(255, 0, 0));
        assert!(colouring.band_low(0).is_none());
        assert_eq!(colouring.band_high(0), Some(2.0));
        assert_eq!(colouring.band_low(2), Some(5.0));
        assert!(colouring.band_high(2).is_none());
    }

    #[test]
    fn smooth_stops_blend_between_the_two_stops_either_side() {
        let colouring = SurfaceColouring {
            faces: SurfaceFaces::Elevation,
            stops: vec![stop(0.0, rgb(0, 0, 0)), stop(10.0, rgb(200, 100, 50))],
            ..SurfaceColouring::default()
        };
        // Half way: 100, 50, 25 (100.5, 50.5 and 25.5 truncated after adding
        // the half to 100, 50, 25).
        assert_eq!(colouring.colour_at(5.0), rgb(100, 50, 25));
        assert_eq!(colouring.colour_at(-1.0), rgb(0, 0, 0));
        assert_eq!(colouring.colour_at(20.0), rgb(200, 100, 50));
    }

    #[test]
    fn a_range_narrower_than_the_tolerance_is_the_ramps_middle_and_not_finite_is_flat() {
        let colouring = SurfaceColouring {
            faces: SurfaceFaces::Elevation,
            low: 5.0,
            high: 5.0,
            flat: rgb(1, 2, 3),
            ..SurfaceColouring::default()
        };
        assert_eq!(colouring.colour_at(5.0), rgb(235, 220, 130));
        assert_eq!(colouring.colour_at(f64::NAN), rgb(1, 2, 3));
        assert_eq!(colouring.colour_at(f64::INFINITY), rgb(1, 2, 3));
    }

    #[test]
    fn the_styles_range_replaces_the_surfaces_own_end_by_end() {
        let mut style = SurfaceDisplayStyle {
            range_min: Some(10.0),
            ..SurfaceDisplayStyle::default()
        };
        let low_given = surface_colouring(&style, SurfaceFaces::Elevation, 0.0, 20.0);
        assert_eq!(low_given.low, 10.0);
        assert_eq!(low_given.high, 20.0);
        style.range_max = Some(12.0);
        let both = surface_colouring(&style, SurfaceFaces::Elevation, 0.0, 20.0);
        assert_eq!(both.low, 10.0);
        assert_eq!(both.high, 12.0);
        // Back to front: the point at its low end.
        style.range_min = Some(30.0);
        let backwards = surface_colouring(&style, SurfaceFaces::Elevation, 0.0, 20.0);
        assert_eq!(backwards.low, 30.0);
        assert_eq!(backwards.high, 30.0);
    }

    #[test]
    fn slope_is_in_percent_and_its_default_range_ends_at_the_steepest_face() {
        // The plane z = x / 2 rises 1 in 2: 50 percent on every face.
        let surface = square_rising([0.0, 5.0, 5.0, 0.0]);
        near(steepest_slope_percent(&surface), 50.0, 1e-9);
        let style = SurfaceDisplayStyle::default();
        let colouring = surface_colouring_for(&style, SurfaceFaces::Slope, &surface);
        assert_eq!(colouring.low, 0.0);
        near(colouring.high, 50.0, 1e-9);
        assert_eq!(steepest_slope_percent(&TinSurface::default()), 0.0);
    }

    #[test]
    fn a_collapsed_range_is_a_point_and_a_value_keeps_the_side_of_it_it_is_on() {
        // low = high = 40: below the point is the first band (the ramp's
        // start), above it the last (its end), and at it the middle - band
        // 5 / 2 = 2, the ramp at 1/2. One middle for every value, whichever
        // side it was on, painted a terrain wholly below min=40 in the middle
        // band.
        let mut colouring = banded_ramp(40.0, 40.0, 5);
        assert!(colouring.collapsed());
        assert_eq!(colouring.band_of(30.0), 0);
        assert_eq!(colouring.band_of(50.0), 4);
        assert_eq!(colouring.band_of(40.0), 2);
        // Within the geometric tolerance (1e-7) is at it; past it is a side.
        assert_eq!(colouring.band_of(40.0 + 1.0e-9), 2);
        assert_eq!(colouring.band_of(40.0 + 1.0e-6), 4);
        assert_eq!(colouring.band_of(40.0 - 1.0e-6), 0);
        assert_eq!(colouring.colour_at(30.0), rgb(40, 70, 140));
        assert_eq!(colouring.colour_at(50.0), rgb(245, 245, 245));
        assert_eq!(colouring.colour_at(40.0), rgb(235, 220, 130));
        // The same for a smooth ramp.
        colouring.banded = false;
        assert_eq!(colouring.colour_at(30.0), rgb(40, 70, 140));
        assert_eq!(colouring.colour_at(50.0), rgb(245, 245, 245));
        assert_eq!(colouring.colour_at(40.0), rgb(235, 220, 130));
        // Not finite is still flat; a range that is not collapsed is not one.
        assert_eq!(colouring.colour_at(f64::NAN), colouring.flat);
        assert!(!banded_ramp(0.0, 10.0, 5).collapsed());
    }

    #[test]
    fn a_level_face_under_slope_is_the_ramps_start_for_slope_is_never_below_its_point() {
        // Slope's range is 0 to the steepest face: for a level surface the
        // point 0. A face with no steepness is the slope ramp's start, green
        // (0, 255, 38) = what the 3D view drew it before styles - not the
        // middle, yellow (255, 255, 38), which reads as half way to the steep
        // end.
        let level = square_rising([5.0, 5.0, 5.0, 5.0]);
        assert_eq!(steepest_slope_percent(&level), 0.0);
        let mut style = SurfaceDisplayStyle {
            ramp: String::from("slope"),
            ..SurfaceDisplayStyle::default()
        };
        let smooth = surface_colouring_for(&style, SurfaceFaces::Slope, &level);
        assert!(smooth.collapsed());
        assert_eq!(smooth.colour_at(0.0), rgb(0, 255, 38));
        // A face steeper than the point (a surface edited since, or a range
        // given) is the end.
        assert_eq!(smooth.colour_at(10.0), rgb(255, 0, 38));
        style.banded = true;
        style.bands = 4;
        let banded = surface_colouring_for(&style, SurfaceFaces::Slope, &level);
        assert_eq!(banded.band_of(0.0), 0);
        assert_eq!(banded.colour_at(0.0), rgb(0, 255, 38));
        assert_eq!(banded.band_of(10.0), 3);
    }
}

mod surface_band_pieces {
    use super::*;

    #[test]
    fn a_triangle_across_five_bands_is_cut_into_five_pieces_that_cover_it_exactly() {
        // z runs 0 to 10 across the triangle; bands of 2 from 0 to 10.
        let colouring = banded_ramp(0.0, 10.0, 5);
        let a = Point3::new(0.0, 0.0, 0.0);
        let b = Point3::new(10.0, 0.0, 10.0);
        let c = Point3::new(0.0, 10.0, 5.0);
        let mut pieces = Vec::new();
        band_pieces(&colouring, a, b, c, &mut pieces);
        assert_eq!(pieces.len(), 5);
        let mut area = 0.0;
        for (k, piece) in pieces.iter().enumerate() {
            assert_eq!(piece.band, k);
            assert!(piece.count >= 3);
            area += piece_area(piece);
            for corner in &piece.corners[..piece.count] {
                assert!(corner.z >= 2.0 * k as f64 - 1e-12);
                assert!(corner.z <= 2.0 * (k + 1) as f64 + 1e-12);
            }
        }
        // The triangle's own area: half of 10 x 10.
        near(area, 50.0, 1e-9);
    }

    #[test]
    fn a_triangle_in_one_band_is_one_piece_and_an_unbanded_colouring_never_cuts() {
        let colouring = banded_ramp(0.0, 10.0, 5);
        let mut pieces = Vec::new();
        band_pieces(
            &colouring,
            Point3::new(0.0, 0.0, 2.5),
            Point3::new(1.0, 0.0, 3.0),
            Point3::new(0.0, 1.0, 3.5),
            &mut pieces,
        );
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0].band, 1);
        assert_eq!(pieces[0].count, 3);
        let mut smooth = colouring.clone();
        smooth.banded = false;
        pieces.clear();
        band_pieces(
            &smooth,
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(10.0, 0.0, 10.0),
            Point3::new(0.0, 10.0, 5.0),
            &mut pieces,
        );
        assert_eq!(pieces.len(), 1);
    }

    #[test]
    fn a_triangle_across_the_maximum_number_of_bands_is_at_most_that_many_pieces() {
        // The 3D view's geometry is the pieces: a triangle crossing every one
        // of the style's bands is that many pieces and no more, so the maximum
        // the style check allows (100) bounds a surface's pieces at 100 a
        // triangle, however fine the bands. z runs 0 to 10 across the triangle
        // (0, 0, 0) (10, 0, 10) (0, 10, 5), 0.1 a band: all 100 bands are
        // crossed and the pieces, being a partition of the triangle, add up to
        // its area, 50.
        let colouring = banded_ramp(0.0, 10.0, MAXIMUM_SURFACE_BANDS);
        let mut pieces = Vec::new();
        band_pieces(
            &colouring,
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(10.0, 0.0, 10.0),
            Point3::new(0.0, 10.0, 5.0),
            &mut pieces,
        );
        assert_eq!(pieces.len(), 100);
        let mut area = 0.0;
        for piece in &pieces {
            assert!(piece.count >= 3);
            assert!(piece.count <= 5);
            area += piece_area(piece);
        }
        near(area, 50.0, 1e-9);
    }
}

mod surface_legend {
    use super::*;

    #[test]
    fn bands_have_an_entry_each_labelled_by_their_edges() {
        let legend = surface_legend(&banded_ramp(25.0, 35.0, 5));
        assert_eq!(legend.title, "Elevation");
        assert!(!legend.gradient);
        assert_eq!(legend.entries.len(), 5);
        assert_eq!(legend.entries[0].label, "25 - 27");
        assert_eq!(legend.entries[4].label, "33 - 35");
        assert_eq!(legend.entries[0].colour, rgb(40, 70, 140));
    }

    #[test]
    fn banded_stops_open_the_first_band_below_and_the_last_above() {
        let mut colouring = banded_ramp(0.0, 100.0, 10);
        colouring.faces = SurfaceFaces::Slope;
        for value in [0.0, 2.0, 5.0, 10.0, 20.0, 33.0] {
            colouring.stops.push(stop(value, rgb(value as u8, 0, 0)));
        }
        let legend = surface_legend(&colouring);
        assert_eq!(legend.title, "Slope %");
        assert_eq!(legend.entries.len(), 6);
        assert_eq!(legend.entries[0].label, "< 2");
        assert_eq!(legend.entries[1].label, "2 - 5");
        assert_eq!(legend.entries[5].label, "33 +");
    }

    #[test]
    fn aspect_is_the_eight_sectors_and_level_and_no_faces_has_none() {
        let mut colouring = SurfaceColouring {
            faces: SurfaceFaces::Aspect,
            ..SurfaceColouring::default()
        };
        let legend = surface_legend(&colouring);
        assert_eq!(legend.entries.len(), 9);
        assert_eq!(legend.entries[0].label, "N");
        assert_eq!(legend.entries[7].label, "NW");
        assert_eq!(legend.entries[8].label, "level");
        colouring.faces = SurfaceFaces::None;
        assert!(surface_legend(&colouring).entries.is_empty());
    }

    #[test]
    fn a_smooth_ramp_is_a_gradient_labelled_at_its_ends() {
        let mut colouring = banded_ramp(24.892, 38.819, 10);
        colouring.banded = false;
        let legend = surface_legend(&colouring);
        assert!(legend.gradient);
        assert_eq!(legend.entries.len(), 9);
        assert_eq!(legend.entries[0].label, "24.892");
        assert_eq!(legend.entries[8].label, "38.819");
        assert!(legend.entries[4].label.is_empty());
        assert_eq!(legend.entries[0].colour, rgb(40, 70, 140));
        assert_eq!(legend.entries[8].colour, rgb(245, 245, 245));
    }

    #[test]
    fn decimals_are_the_fewest_that_write_every_value() {
        assert_eq!(legend_decimals(&[25.0, 26.0]), 0);
        assert_eq!(legend_decimals(&[0.5, 3.0]), 1);
        assert_eq!(legend_decimals(&[24.892, 26.284699999999997]), 3);
        assert_eq!(legend_decimals(&[1.0 / 3.0]), 3); // at most three
        assert_eq!(format_legend_value(26.284699999999997, 3), "26.285");
        assert_eq!(format_legend_value(-0.0, 1), "0.0");
    }

    #[test]
    fn a_collapsed_range_says_below_at_and_above_its_point_not_n_bands_of_zero_width() {
        // Five swatches labelled "40 - 40" in five colours, none saying which
        // one is drawn, was the legend of min=40 over a terrain wholly below
        // it.
        let banded = surface_legend(&banded_ramp(40.0, 40.0, 5));
        assert!(!banded.gradient);
        assert_eq!(banded.entries.len(), 3);
        assert_eq!(banded.entries[0].label, "< 40");
        assert_eq!(banded.entries[1].label, "40");
        assert_eq!(banded.entries[2].label, "> 40");
        assert_eq!(banded.entries[0].colour, rgb(40, 70, 140)); // the first band
        assert_eq!(banded.entries[1].colour, rgb(235, 220, 130)); // the middle band
        assert_eq!(banded.entries[2].colour, rgb(245, 245, 245)); // the last
        assert!(banded.entries[0].from.is_none());
        assert_eq!(banded.entries[0].to, Some(40.0));
        assert_eq!(banded.entries[1].from, Some(40.0));
        assert_eq!(banded.entries[1].to, Some(40.0));
        assert_eq!(banded.entries[2].from, Some(40.0));
        assert!(banded.entries[2].to.is_none());
        // A smooth ramp says the same three, not nine of one colour.
        let mut smooth = banded_ramp(40.0, 40.0, 5);
        smooth.banded = false;
        let gradient = surface_legend(&smooth);
        assert!(!gradient.gradient);
        assert_eq!(gradient.entries.len(), 3);
        assert_eq!(gradient.entries[0].colour, rgb(40, 70, 140));
        assert_eq!(gradient.entries[2].colour, rgb(245, 245, 245));
        // Slope is never below its point 0: a level pad has the point and
        // above.
        let mut slope = banded_ramp(0.0, 0.0, 4);
        slope.faces = SurfaceFaces::Slope;
        slope.ramp = *surface_ramp_stops("slope");
        let level = surface_legend(&slope);
        assert_eq!(level.entries.len(), 2);
        assert_eq!(level.title, "Slope %");
        assert_eq!(level.entries[0].label, "0");
        assert_eq!(level.entries[0].colour, rgb(0, 255, 38));
        assert_eq!(level.entries[1].label, "> 0");
        assert_eq!(level.entries[1].colour, rgb(255, 0, 38));
    }

    #[test]
    fn a_banded_ramp_the_data_runs_past_takes_those_values_in_its_end_bands_and_says_so() {
        // min=30 max=35 over a terrain of 24.892 to 38.819, five bands: the
        // first band is drawn on everything below 31 and the last on
        // everything above 34, so "30 - 31" and "34 - 35" misread the colours
        // that cover 24.9 to 30 and 35 to 38.8.
        let mut style = SurfaceDisplayStyle {
            banded: true,
            bands: 5,
            range_min: Some(30.0),
            range_max: Some(35.0),
            ..SurfaceDisplayStyle::default()
        };
        let open = surface_colouring(&style, SurfaceFaces::Elevation, 24.892, 38.819);
        assert!(open.open_below);
        assert!(open.open_above);
        assert!(open.band_low(0).is_none());
        assert!(open.band_high(4).is_none());
        assert_eq!(open.band_high(0), Some(31.0));
        assert_eq!(open.band_low(4), Some(34.0));
        let legend = surface_legend(&open);
        assert_eq!(legend.entries.len(), 5);
        assert_eq!(legend.entries[0].label, "< 31");
        assert_eq!(legend.entries[1].label, "31 - 32");
        assert_eq!(legend.entries[3].label, "33 - 34");
        assert_eq!(legend.entries[4].label, "34 +");
        assert!(legend.entries[0].from.is_none());
        assert!(legend.entries[4].to.is_none());
        // The picture is unchanged by what the legend says: band_of still
        // puts 25 in the first band and 38 in the last.
        assert_eq!(open.band_of(25.0), 0);
        assert_eq!(open.band_of(38.0), 4);
        // A range the data lies inside leaves both ends closed, as before: the
        // sample terrain's own bands read "20 - 26" ... "44 - 50".
        style.range_min = Some(20.0);
        style.range_max = Some(50.0);
        let closed = surface_colouring(&style, SurfaceFaces::Elevation, 24.892, 38.819);
        assert!(!closed.open_below);
        assert!(!closed.open_above);
        let wide = surface_legend(&closed);
        assert_eq!(wide.entries.len(), 5);
        assert_eq!(wide.entries[0].label, "20 - 26");
        assert_eq!(wide.entries[4].label, "44 - 50");
        // An end the style leaves to the surface is closed whatever the other
        // does.
        style.range_min = None;
        style.range_max = Some(35.0);
        let half = surface_colouring(&style, SurfaceFaces::Elevation, 24.892, 38.819);
        assert!(!half.open_below);
        assert!(half.open_above);
    }

    #[test]
    fn one_banded_stop_is_every_value_not_anything_from_it_up() {
        // band_of puts every value in the only band, so "30 +" claimed what the
        // colour does not mean: ground below 30 is drawn in it as well.
        let mut colouring = banded_ramp(0.0, 100.0, 10);
        colouring.stops = vec![stop(30.0, rgb(255, 0, 0))];
        assert_eq!(colouring.band_count(), 1);
        assert!(colouring.band_low(0).is_none());
        assert!(colouring.band_high(0).is_none());
        assert_eq!(colouring.colour_at(10.0), rgb(255, 0, 0));
        let legend = surface_legend(&colouring);
        assert_eq!(legend.entries.len(), 1);
        assert_eq!(legend.entries[0].label, "all");
        assert!(legend.entries[0].from.is_none());
        assert!(legend.entries[0].to.is_none());
        assert_eq!(legend.entries[0].colour, rgb(255, 0, 0));
    }
}
