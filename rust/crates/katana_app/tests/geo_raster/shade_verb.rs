//! tests/geo/test_shade_verb.cpp: the RASTER SHADE verb
//! (src/katana_app/geo/shade_verbs.rs, docs/terrain.md "Shading"): an
//! elevation source rendered by GDAL's hillshade, color-map and blend into a
//! derived reference raster, drawn as it was rendered, with its legend.
//!
//! Fixtures: tests/geo/data/plane.asc (40 x 30 cells of 1 m from (0,0), z =
//! 100 + 0.05 x at the cell centres, so 100.025 to 101.975), and flat grids
//! written here. Its ShadeContract and ColourRamps suites are step 5's.

mod shade_verb {
    use std::path::Path;
    use std::sync::atomic::AtomicBool;

    use katana_core::ErrorCode;
    use katana_core::text::{format_exact_real, format_general};
    use katana_gis::GdalDataset;
    use katana_interop::geo::colour_ramps::built_in_ramp;
    use katana_interop::import::import_raster;
    use katana_interop::{RasterDisplayStyle, RasterImportOptions, RasterOverlay, RasterRole};

    use crate::geo_fixture::{
        GeoFixture, TempDir, geo_data, holds, must, near, refused, shared_data,
    };
    use crate::raster_support::{ascii_grid, is_empty_dir, records, stod};

    /// The C++ fixture's: its scratch folder named after the test.
    fn fixture(name: &str) -> GeoFixture {
        GeoFixture::new("katana-shade-verb-", name)
    }

    /// The C++ fixture's second folder, for the files a case writes.
    fn files(name: &str) -> TempDir {
        TempDir::new("katana-shade-verb-", &format!("{name}-files"))
    }

    /// tests/geo/data/plane.asc.
    fn plane() -> String {
        format!("{}/plane.asc", geo_data())
    }

    /// An ESRI ASCII grid of `columns` x `rows` 1 m cells from (0,0), every
    /// cell `height`, written as the C++'s stream writes a double.
    fn flat_grid(folder: &TempDir, name: &str, columns: i32, rows: i32, height: f64) -> String {
        ascii_grid(folder, name, columns, rows, |_, _| {
            format_general(height, 6)
        })
    }

    struct Rgba {
        r: i32,
        g: i32,
        b: i32,
        a: i32,
    }

    fn pixel(raster: &RasterOverlay, column: i32, row: i32) -> Rgba {
        let at = (row as usize * raster.width as usize + column as usize) * 4;
        Rgba {
            r: i32::from(raster.rgba[at]),
            g: i32::from(raster.rgba[at + 1]),
            b: i32::from(raster.rgba[at + 2]),
            a: i32::from(raster.rgba[at + 3]),
        }
    }

    /// The one reference raster the line made.
    fn made(fixture: &GeoFixture) -> &RasterOverlay {
        assert_eq!(fixture.reference.rasters().len(), 1);
        fixture
            .reference
            .rasters()
            .last()
            .expect("the reference raster the line made")
    }

    // ---- the styles -----------------------------------------------------------------------------

    #[test]
    fn flat_ground_hillshades_to181_everywhere_at_altitude45() {
        // GDAL's hillshade is 1 + 254 x (sin(altitude) cos(slope) + ...); on
        // flat ground the slope is 0, so 1 + 254 x sin 45 degrees = 180.6:
        // 181, the edges too (they are interpolated, and flat).
        let name = "FlatGroundHillshadesTo181EverywhereAtAltitude45";
        let mut fixture = fixture(name);
        let files = files(name);
        let flat = flat_grid(&files, "flat.asc", 12, 9, 100.0);
        let reply = fixture.ok(&format!("RASTER SHADE FILE \"{flat}\" style=hillshade"));
        let raster = made(&fixture);
        assert_eq!(raster.width, 12);
        assert_eq!(raster.height, 9);
        for row in 0..raster.height {
            for column in 0..raster.width {
                let shade = pixel(raster, column, row);
                assert_eq!(shade.r, 181, "{column},{row}");
                assert_eq!(shade.g, 181);
                assert_eq!(shade.b, 181);
                assert_eq!(shade.a, 255);
            }
        }
        assert_eq!(raster.role, RasterRole::Derived);
        assert_eq!(raster.display_style, RasterDisplayStyle::Hillshade);
        assert_eq!(raster.name, "flat-hillshade");
        assert!(raster.derivation.contains("RASTER SHADE"));
        assert!(
            records(&reply, "legend").is_empty(),
            "a hillshade is grey: no legend"
        );
    }

    #[test]
    fn a_ramp_maps_its_endpoints_exactly() {
        // Over the data's range, the lowest cells (the column at x = 0.5)
        // take the terrain ramp's first colour and the highest (x = 39.5) its
        // last.
        let _shared = shared_data();
        let mut fixture = fixture("ARampMapsItsEndpointsExactly");
        fixture.ok(&format!("RASTER SHADE FILE \"{}\" style=relief", plane()));
        let raster = made(&fixture);
        assert_eq!(raster.display_style, RasterDisplayStyle::Relief);
        let terrain = built_in_ramp("terrain").expect("the terrain ramp");
        let first = terrain.stops[0];
        let last = terrain.stops[terrain.stops.len() - 1];
        for row in 0..raster.height {
            let low = pixel(raster, 0, row);
            assert_eq!(low.r, i32::from(first.r));
            assert_eq!(low.g, i32::from(first.g));
            assert_eq!(low.b, i32::from(first.b));
            assert_eq!(low.a, 255);
            let high = pixel(raster, raster.width - 1, row);
            assert_eq!(high.r, i32::from(last.r));
            assert_eq!(high.g, i32::from(last.g));
            assert_eq!(high.b, i32::from(last.b));
        }
    }

    #[test]
    fn legend_records_list_the_ramp_stops() {
        // The terrain ramp's five stops at 0, 1/4, 1/2, 3/4 and 1 of the
        // data's range. The range is the file's heights as GDAL reads them,
        // Float32: 100.025f and 101.975f, which are 1.5e-6 off the decimals.
        // So the stops are about 100.025, 100.5125, 101.0, 101.4875 and
        // 101.975, and each record is its stop rounded to three decimals:
        // within half a thousandth of the stop worked from the Float32 ends.
        let _shared = shared_data();
        let mut fixture = fixture("LegendRecordsListTheRampStops");
        let reply = fixture.ok(&format!("RASTER SHADE FILE \"{}\" style=relief", plane()));
        let legend = records(&reply, "legend");
        let terrain = built_in_ramp("terrain").expect("the terrain ramp");
        assert_eq!(legend.len(), terrain.stops.len(), "{reply}");
        let low = f64::from(100.025_f32);
        let high = f64::from(101.975_f32);
        for (entry, stop) in legend.iter().zip(&terrain.stops) {
            let value = low + stop.position * (high - low);
            near(stod(entry.get("value")), value, 5e-4);
            assert_eq!(entry.get("r").unwrap_or(""), format!("{}", stop.r));
            assert_eq!(entry.get("g").unwrap_or(""), format!("{}", stop.g));
            assert_eq!(entry.get("b").unwrap_or(""), format!("{}", stop.b));
        }
        let ramp = records(&reply, "ramp");
        assert_eq!(ramp.len(), 1);
        assert_eq!(ramp[0].get("name").unwrap_or(""), "terrain");
        assert_eq!(ramp[0].get("from").unwrap_or(""), "data");
    }

    #[test]
    fn a_ramp_only_the_surface_styles_had_is_accepted_and_legended_in_its_own_colours() {
        // earth and spectral were the surface styles' alone: RASTER SHADE
        // refused them. Spectral is ColorBrewer's five-class Spectral
        // reversed, #2b83ba (43, 131, 186) low to #d7191c (215, 25, 28) high;
        // its legend is five stops, the first and the last at the range's
        // ends.
        let _shared = shared_data();
        let mut fixture =
            fixture("ARampOnlyTheSurfaceStylesHadIsAcceptedAndLegendedInItsOwnColours");
        let reply = fixture.ok(&format!(
            "RASTER SHADE FILE \"{}\" style=relief ramp=spectral",
            plane()
        ));
        let ramp = records(&reply, "ramp");
        assert_eq!(ramp.len(), 1, "{reply}");
        assert_eq!(ramp[0].get("name").unwrap_or(""), "spectral");
        let legend = records(&reply, "legend");
        assert_eq!(legend.len(), 5, "{reply}");
        assert_eq!(legend[0].get("r").unwrap_or(""), "43");
        assert_eq!(legend[0].get("g").unwrap_or(""), "131");
        assert_eq!(legend[0].get("b").unwrap_or(""), "186");
        assert_eq!(legend[4].get("r").unwrap_or(""), "215");
        assert_eq!(legend[4].get("g").unwrap_or(""), "25");
        assert_eq!(legend[4].get("b").unwrap_or(""), "28");
        // The lowest cells take the ramp's first colour exactly, as terrain's
        // do.
        let raster = made(&fixture);
        let low = pixel(raster, 0, 0);
        assert_eq!(low.r, 43);
        assert_eq!(low.g, 131);
        assert_eq!(low.b, 186);
    }

    #[test]
    fn relief_over_hillshade_on_flat_ground_keeps_the_ramp_hue() {
        // range=100,200 puts the flat ground's 100 at the terrain ramp's first
        // colour, (40, 70, 140): the one table's, the surface styles' (the
        // shading's own green, (38, 115, 0), until the two tables were made
        // one). hsv-value keeps its hue and saturation and takes the
        // hillshade's 181 as the value: the colour scaled so its greatest
        // channel, the blue, is 181, (40 x 181 / 140, 70 x 181 / 140, 181) =
        // (51.71, 90.5, 181). An 8-bit channel rounds it: within 1.
        let name = "ReliefOverHillshadeOnFlatGroundKeepsTheRampHue";
        let mut fixture = fixture(name);
        let files = files(name);
        let flat = flat_grid(&files, "flat.asc", 8, 6, 100.0);
        fixture.ok(&format!(
            "RASTER SHADE FILE \"{flat}\" style=relief+hillshade range=100,200"
        ));
        let raster = made(&fixture);
        assert_eq!(raster.display_style, RasterDisplayStyle::ReliefHillshade);
        for row in 0..raster.height {
            for column in 0..raster.width {
                let shade = pixel(raster, column, row);
                near(f64::from(shade.r), 40.0 * 181.0 / 140.0, 1.0);
                near(f64::from(shade.g), 70.0 * 181.0 / 140.0, 1.0);
                assert_eq!(shade.b, 181, "{column},{row}");
            }
        }
    }

    #[test]
    fn slope_shading_colours_the_slope_in_degrees() {
        // The plane rises 1 in 20: atan(0.05) = 2.8624052 degrees wherever
        // Horn's 3 x 3 window lies on it. A range of twice that puts it at the
        // slope ramp's middle stop. The Float32 slope is within about 1e-6
        // degree of it, which moves the colour by 1e-6 / 5.72 of the ramp:
        // none, to 8 bits.
        let _shared = shared_data();
        let mut fixture = fixture("SlopeShadingColoursTheSlopeInDegrees");
        let slope = 0.05_f64.atan() * 180.0 / std::f64::consts::PI;
        let reply = fixture.ok(&format!(
            "RASTER SHADE FILE \"{}\" style=slope range=0,{}",
            plane(),
            format_exact_real(2.0 * slope)
        ));
        let shade = records(&reply, "shade");
        assert_eq!(shade.len(), 1, "{reply}");
        assert_eq!(shade[0].get("unit").unwrap_or(""), "degree");
        let ramp = records(&reply, "ramp");
        assert_eq!(ramp.len(), 1);
        assert_eq!(ramp[0].get("name").unwrap_or(""), "slope");
        assert_eq!(ramp[0].get("from").unwrap_or(""), "range");
        let raster = made(&fixture);
        assert_eq!(raster.display_style, RasterDisplayStyle::Slope);
        let middle = built_in_ramp("slope").expect("the slope ramp").stops[2];
        let inside = pixel(raster, 20, 15);
        near(f64::from(inside.r), f64::from(middle.r), 1.0);
        near(f64::from(inside.g), f64::from(middle.g), 1.0);
        near(f64::from(inside.b), f64::from(middle.b), 1.0);
    }

    #[test]
    fn save_writes_a_geo_tiff_with_the_source_geotransform() {
        let _shared = shared_data();
        let name = "SaveWritesAGeoTiffWithTheSourceGeotransform";
        let mut fixture = fixture(name);
        let files = files(name);
        let save = files.file("shade.tif");
        let reply = fixture.ok(&format!(
            "RASTER SHADE FILE \"{}\" style=relief save=\"{save}\"",
            plane()
        ));
        assert!(!records(&reply, "saved").is_empty(), "{reply}");
        {
            let opened = must(GdalDataset::open(Path::new(&save)));
            assert_eq!(opened.driver_name(), "GTiff");
            let info = must(opened.raster_info());
            assert_eq!(info.width, 40);
            assert_eq!(info.height, 30);
            // Red, green, blue and alpha.
            assert_eq!(info.band_count, 4);
            // plane.asc's own: its lower-left corner at (0,0), 30 rows of 1 m.
            assert_eq!(info.geotransform, [0.0, 1.0, 0.0, 30.0, 0.0, -1.0]);
        }
        // A file in the way is replaced only with OVERWRITE.
        let error = refused(
            fixture.run(&format!(
                "RASTER SHADE FILE \"{}\" save=\"{save}\"",
                plane()
            )),
            "a save over a file without OVERWRITE",
        );
        assert_eq!(error.code, ErrorCode::AlreadyExists);
        fixture.ok(&format!(
            "RASTER SHADE FILE \"{}\" save=\"{save}\" OVERWRITE",
            plane()
        ));
    }

    #[test]
    fn a_surface_source_is_rasterised_at_its_cell() {
        // The surface of plane.asc spans its cell centres, 0.5 to 39.5 by 0.5
        // to 29.5: 39 x 29 m, so a 2 m cell makes ceil(19.5) x ceil(14.5) =
        // 20 x 15.
        let _shared = shared_data();
        let mut fixture = fixture("ASurfaceSourceIsRasterisedAtItsCell");
        fixture.ok(&format!("SURFACE FROM FILE \"{}\" NAME plane", plane()));
        let reply = fixture.ok("RASTER SHADE SURFACE plane CELL 2");
        let output = records(&reply, "output");
        assert_eq!(output.len(), 1, "{reply}");
        assert_eq!(output[0].get("raster").unwrap_or(""), "20x15");
        assert_eq!(made(&fixture).name, "plane-hillshade");
    }

    #[test]
    fn a_reference_raster_is_shaded_from_its_file() {
        let _shared = shared_data();
        let mut fixture = fixture("AReferenceRasterIsShadedFromItsFile");
        let imported = must(import_raster(
            Path::new(&plane()),
            &RasterImportOptions::default(),
        ));
        let id = fixture.reference.add(imported);
        let reply = fixture.ok(&format!(
            "RASTER SHADE RASTER {id} style=plain NAME grey-plane"
        ));
        assert_eq!(fixture.reference.rasters().len(), 2);
        let last = fixture
            .reference
            .rasters()
            .last()
            .expect("the shaded raster");
        assert_eq!(last.name, "grey-plane");
        assert_eq!(last.display_style, RasterDisplayStyle::Plain);
        // The grey ramp's five stops (the table's: 32 to 224 in steps of 48;
        // it was two, black and white, in the shading's own table).
        assert_eq!(records(&reply, "legend").len(), 5);
    }

    #[test]
    fn a_picture_longer_than_the_display_copy_is_averaged_down_to_it() {
        // 5000 cells is more than the 4096 a reference raster's display copy
        // keeps, so the DEM is averaged by ceil(5000 / 4096) = 2 first: 2500
        // x 3.
        let name = "APictureLongerThanTheDisplayCopyIsAveragedDownToIt";
        let mut fixture = fixture(name);
        let files = files(name);
        let wide = flat_grid(&files, "wide.asc", 5000, 6, 50.0);
        let reply = fixture.ok(&format!("RASTER SHADE FILE \"{wide}\""));
        let resampled = records(&reply, "resampled");
        assert_eq!(resampled.len(), 1, "{reply}");
        assert_eq!(resampled[0].get("from").unwrap_or(""), "5000x6");
        assert_eq!(resampled[0].get("to").unwrap_or(""), "2500x3");
        assert_eq!(made(&fixture).width, 2500);
    }

    #[test]
    fn a_persons_colour_map_file_is_painted_and_is_the_legend() {
        // Percentages are placed on the data's range, as GDAL places them.
        let _shared = shared_data();
        let name = "APersonsColourMapFileIsPaintedAndIsTheLegend";
        let mut fixture = fixture(name);
        let files = files(name);
        let map = files.file("blue-red.txt");
        if let Err(error) = std::fs::write(&map, "0% 0 0 255\n100% 255 0 0\n") {
            panic!("{map}: {error}");
        }
        let reply = fixture.ok(&format!(
            "RASTER SHADE FILE \"{}\" style=relief ramp=\"{map}\"",
            plane()
        ));
        let legend = records(&reply, "legend");
        assert_eq!(legend.len(), 2, "{reply}");
        near(stod(legend[0].get("value")), 100.025, 5e-4);
        near(stod(legend[1].get("value")), 101.975, 5e-4);
        let raster = made(&fixture);
        let low = pixel(raster, 0, 0);
        assert_eq!(low.b, 255);
        assert_eq!(low.r, 0);
        let high = pixel(raster, raster.width - 1, 0);
        assert_eq!(high.r, 255);
        assert_eq!(high.b, 0);
    }

    #[test]
    fn preview_and_a_cancelled_run_add_nothing() {
        let _shared = shared_data();
        let mut fixture = fixture("PreviewAndACancelledRunAddNothing");
        let reply = fixture.ok(&format!("RASTER SHADE FILE \"{}\" PREVIEW", plane()));
        holds(&reply, "preview valid=yes changed=no");
        let prepared =
            must(fixture.prepare(&format!("RASTER SHADE FILE \"{}\" style=relief", plane())));
        let stop = AtomicBool::new(true);
        let work = prepared.work.expect("a run has work");
        let error = refused(work(Some(&stop), None), "a stopped run");
        assert_eq!(error.message, "cancelled");
        assert!(fixture.reference.rasters().is_empty());
        // Nothing of the chain's is left in the scratch folder.
        assert!(is_empty_dir(fixture.scratch.path()));
    }

    #[test]
    fn what_the_verb_cannot_do_is_refused_naming_it() {
        let _shared = shared_data();
        let mut fixture = fixture("WhatTheVerbCannotDoIsRefusedNamingIt");
        let line = format!("RASTER SHADE FILE \"{}\"", plane());
        for words in [
            " style=glow",
            " azimuth=400",
            " altitude=95",
            " z=0",
            " variant=soft",
            " range=5,1",
            " range=5",
            " save=picture.png",
            " style=relief azimuth=270",
            " style=hillshade ramp=terrain",
            " colour=red",
        ] {
            let error = refused(fixture.run(&format!("{line}{words}")), words);
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{words}");
        }
        let unknown = refused(
            fixture.run(&format!("{line} style=relief ramp=no-such-ramp")),
            "a ramp of no name and no file",
        );
        assert_eq!(unknown.code, ErrorCode::NotFound);
        // The refusal names every ramp of the table, the style's two as well.
        holds(
            &unknown.message,
            "terrain, earth, diverging, slope, grey or spectral",
        );
        assert_eq!(
            refused(fixture.run("RASTER SHADE DRAWING"), "a scope as the source").code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            refused(
                fixture.run("RASTER SHADE SURFACE nothing"),
                "no such surface"
            )
            .code,
            ErrorCode::NotFound
        );
        assert!(fixture.reference.rasters().is_empty());
    }
}

// ---- through a Session, as katana_cli and katana_mcp run it -------------------------------------

mod shade_session {
    use katana_app::{Captured, Session};

    use crate::geo_fixture::{geo_data, holds, shared_data};

    #[test]
    fn the_session_adds_the_shading_as_a_reference_raster() {
        let _shared = shared_data();
        let mut session = Session::new(None, &mut Captured::default());
        let mut captured = Captured::default();
        let shaded = session.run(
            format!(
                "RASTER SHADE FILE \"{}/plane.asc\" style=relief+hillshade",
                geo_data()
            ),
            &mut captured,
        );
        let listed = session.run("SURFACE LIST", &mut captured);
        assert!(shaded);
        assert!(listed);
        holds(
            &captured.out,
            "output arg=output kind=raster target=reference id=1 name=plane-relief-hillshade \
             raster=40x30",
        );
        holds(
            &captured.out,
            "raster id=1 name=plane-relief-hillshade kind=derived",
        );
    }
}
