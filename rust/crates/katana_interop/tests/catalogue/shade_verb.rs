//! tests/geo/test_shade_verb.cpp: the ColourRamps suite, the ramps terrain
//! shading draws with (katana_interop::geo::colour_ramps). The file's other
//! suites drive RASTER SHADE through the app layer (step 6) or bind GDAL's
//! arguments (wave L).

mod colour_ramps {
    use katana_cad::terrain::surface_colouring::surface_ramp_stops;
    use katana_cad::terrain::surface_style::surface_ramp_names;
    use katana_core::ErrorCode;
    use katana_interop::geo::LegendEntry;
    use katana_interop::geo::colour_ramps::{
        built_in_ramp, built_in_ramp_names, colour_map_text, read_colour_map, spread,
    };

    use crate::Scratch;

    #[test]
    fn a_spread_ramp_hits_its_ends_exactly_and_writes_gdals_text() {
        let slope = built_in_ramp("SLOPE").expect("slope is a built-in ramp");
        // 0.1 + 1 x (0.3 - 0.1) is not 0.3 in binary; the end is taken as
        // given.
        let entries = spread(slope, 0.1, 0.3);
        assert_eq!(entries.len(), 5);
        assert_eq!(entries[0].value, 0.1);
        assert_eq!(entries[entries.len() - 1].value, 0.3);
        let one = LegendEntry {
            value: 1.5,
            r: 1,
            g: 2,
            b: 3,
        };
        assert_eq!(
            colour_map_text(std::slice::from_ref(&one), true),
            "1.5 1 2 3 255\nnv 0 0 0 0\n"
        );
        let below = LegendEntry {
            value: -2.0,
            r: 0,
            g: 0,
            b: 0,
        };
        assert_eq!(
            colour_map_text(std::slice::from_ref(&below), false),
            "-2 0 0 0 255\n"
        );
        assert_eq!(
            built_in_ramp_names(),
            ["terrain", "earth", "diverging", "slope", "grey", "spectral"]
        );
        assert!(built_in_ramp("rainbow").is_none());
    }

    #[test]
    fn the_shadings_ramps_are_the_surface_styles_ramps_stop_for_stop() {
        // One table: RASTER SHADE's ramp=terrain is the surface style's
        // ramp=terrain, whichever of the two a person types. For every ramp
        // of the table, in its order, the shading's stops are the style's, at
        // 0, 1/4, 1/2, 3/4 and 1 - five, evenly spaced.
        let names = surface_ramp_names();
        assert_eq!(names.len(), 6);
        for &name in names {
            let shading = built_in_ramp(name).unwrap_or_else(|| panic!("{name}"));
            let style = surface_ramp_stops(name);
            assert_eq!(shading.stops.len(), style.len(), "{name}");
            for (i, (stop, colour)) in shading.stops.iter().zip(style).enumerate() {
                assert_eq!(stop.position, i as f64 / 4.0, "{name} {i}");
                assert_eq!(stop.r, colour.r, "{name} {i}");
                assert_eq!(stop.g, colour.g, "{name} {i}");
                assert_eq!(stop.b, colour.b, "{name} {i}");
            }
        }
        // And the colours themselves, from outside this program: terrain is
        // the 3D view's elevation ramp, deep blue (40, 70, 140) to snow (245,
        // 245, 245); diverging is ColorBrewer's RdBu five-class, reversed,
        // #0571b0 ... #ca0020; grey runs 32 to 224. The older shading table
        // had terrain from (38, 115, 0) to white, diverging from (33, 102,
        // 172) to (178, 24, 43) and grey from black to white.
        let terrain = built_in_ramp("TERRAIN").expect("terrain is a built-in ramp");
        let first = terrain.stops[0];
        assert_eq!(first.r, 40);
        assert_eq!(first.g, 70);
        assert_eq!(first.b, 140);
        assert_eq!(terrain.stops[terrain.stops.len() - 1].r, 245);
        let diverging = built_in_ramp("diverging").expect("diverging is a built-in ramp");
        let first = diverging.stops[0];
        let last = diverging.stops[diverging.stops.len() - 1];
        assert_eq!(first.r, 0x05);
        assert_eq!(first.g, 0x71);
        assert_eq!(first.b, 0xb0);
        assert_eq!(last.r, 0xca);
        assert_eq!(last.g, 0x00);
        assert_eq!(last.b, 0x20);
        let grey = built_in_ramp("grey").expect("grey is a built-in ramp");
        assert_eq!(grey.stops[0].r, 32);
        assert_eq!(grey.stops[grey.stops.len() - 1].r, 224);
    }

    #[test]
    fn a_colour_map_file_is_read_as_gdal_reads_it() {
        let folder = Scratch::new("colour-map");
        let map = folder.path.join("map.txt");
        std::fs::write(
            &map,
            "# a comment\nnv 0 0 0 0\n50% 10,20,30\n200 1 2 3 255\nwhite 255 255 255\n0\t4 5 6\n",
        )
        .expect("the map is written");
        let entries = match read_colour_map(&map, 100.0, 300.0) {
            Ok(entries) => entries,
            Err(error) => panic!("{}", error.describe()),
        };
        assert_eq!(entries.len(), 3); // sorted by value
        assert_eq!(entries[0].value, 0.0);
        assert_eq!(entries[1].value, 200.0); // 50% of 100 to 300
        assert_eq!(entries[1].r, 10);
        assert_eq!(entries[2].value, 200.0);
        assert_eq!(
            read_colour_map(&folder.path.join("none.txt"), 0.0, 1.0)
                .expect_err("no file there")
                .code,
            ErrorCode::NotFound
        );
    }
}
