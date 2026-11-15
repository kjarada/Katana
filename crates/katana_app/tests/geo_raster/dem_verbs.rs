//! tests/geo/test_dem_verbs.cpp: the DEM tools (src/katana_app/geo/dem_verbs.rs,
//! docs/terrain.md "The DEM tools"): RASTER MOSAIC, CLIP, FILL, FOOTPRINT,
//! REPROJECT and DIFFERENCE through the executor, as the window runs them.
//! Every expected value is worked by hand from tests/geo/data/plane.asc: 40 x
//! 30 cells of 1 m from (0,0), z = 100 + 0.05x at the cell centres, read by
//! GDAL as Float32. Its DemVerbsContract suite is step 5's, and DemSession,
//! which drives the MCP server, is the MCP's (step 6, agent C).

mod dem_verbs {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicBool;

    use katana_app::geo::dem_support::{BandValues, holds_value, read_band_one_from_file};
    use katana_app::geo::replies::line_word;
    use katana_app::geo::{Context, Prepared, handles, help_text, prepare, run_now};
    use katana_core::{ErrorCode, Result};
    use katana_entity::Geometry;
    use katana_entity::entity_geometry::bounding_box;
    use katana_gis::{GdalDataset, RasterExportOptions};
    use katana_interop::RasterOverlay;

    use crate::geo_fixture::{GeoFixture, geo_data, holds, must, near, refused, shared_data};
    use crate::raster_support::double_eq;

    /// tests/geo/data/plane.asc.
    fn plane() -> String {
        format!("{}/plane.asc", geo_data())
    }

    /// The C++ fixture: a scratch folder named after the test, the context's
    /// own scratch - where derived rasters go - its "derived" folder.
    struct Dem {
        fixture: GeoFixture,
        derived: PathBuf,
    }

    impl Dem {
        fn new(name: &str) -> Self {
            let fixture = GeoFixture::new("katana-dem-verbs-", name);
            let derived = fixture.scratch.path().join("derived");
            Self { fixture, derived }
        }

        /// A file of the scratch folder, as a line names it.
        fn at(&self, name: &str) -> String {
            self.fixture.scratch.file(name)
        }

        fn context(&mut self) -> Context<'_> {
            Context::new(
                &mut self.fixture.document,
                &self.fixture.interpreter,
                &mut self.fixture.reference,
                &self.derived,
                &self.fixture.front_end,
            )
        }

        fn run(&mut self, line: &str) -> Result<String> {
            run_now(&mut self.context(), line, None, None)
        }

        fn prepare(&mut self, line: &str) -> Result<Prepared> {
            prepare(&mut self.context(), line)
        }

        #[track_caller]
        fn ok(&mut self, line: &str) -> String {
            match self.run(line) {
                Ok(reply) => reply,
                Err(error) => panic!("{line}: {}", error.describe()),
            }
        }

        fn rasters(&self) -> &[RasterOverlay] {
            self.fixture.reference.rasters()
        }

        /// The last reference raster's file.
        fn last_source(&self) -> PathBuf {
            self.rasters()
                .last()
                .expect("a reference raster")
                .source
                .clone()
        }

        /// A folder of the scratch folder, made.
        fn folder(&self, name: &str) {
            if let Err(error) = std::fs::create_dir_all(self.fixture.scratch.path().join(name)) {
                panic!("{name}: {error}");
            }
        }
    }

    /// The first record of `kind` in a reply.
    fn record(reply: &str, kind: &str) -> Option<katana_app::geo::replies::Record> {
        GeoFixture::record(reply, kind)
    }

    fn band(file: &Path) -> BandValues {
        must(read_band_one_from_file(file))
    }

    /// plane.asc's values, as GDAL reads them, written with `change` applied
    /// to each (value, column, row): Float64, the plane's own grid.
    fn write_plane(file: &str, change: impl Fn(f64, i32, i32) -> f64, no_data: Option<f64>) {
        let plane = band(Path::new(&plane()));
        let mut values = vec![0.0; plane.values.len()];
        for row in 0..plane.height {
            for column in 0..plane.width {
                let i = row as usize * plane.width as usize + column as usize;
                values[i] = change(plane.values[i], column, row);
            }
        }
        let options = RasterExportOptions {
            width: plane.width,
            height: plane.height,
            geotransform: plane.geotransform,
            no_data_value: no_data,
            ..RasterExportOptions::default()
        };
        must(GdalDataset::write_raster(
            Path::new(file),
            &options,
            &values,
        ));
    }

    // ---- MOSAIC ---------------------------------------------------------------------------------

    #[test]
    fn mosaic_of_two_halves_equals_the_whole() {
        // The plane cut into its west and east halves (columns 0-19 and
        // 20-39), and whole, by the same GDAL clip so all three are the same
        // kind of file; the halves joined again are the whole, value for
        // value.
        let _shared = shared_data();
        let mut dem = Dem::new("MosaicOfTwoHalvesEqualsTheWhole");
        dem.folder("tiles");
        dem.ok(&format!(
            "GDAL raster clip --window=0,0,20,30 FROM FILE \"{}\" TO FILE \"{}\"",
            plane(),
            dem.at("tiles/west.tif")
        ));
        dem.ok(&format!(
            "GDAL raster clip --window=20,0,20,30 FROM FILE \"{}\" TO FILE \"{}\"",
            plane(),
            dem.at("tiles/east.tif")
        ));
        dem.ok(&format!(
            "GDAL raster clip --window=0,0,40,30 FROM FILE \"{}\" TO FILE \"{}\"",
            plane(),
            dem.at("whole.tif")
        ));

        let reply = dem.ok(&format!(
            "RASTER MOSAIC FILE \"{}\" FILE \"{}\" NAME joined",
            dem.at("tiles/west.tif"),
            dem.at("tiles/east.tif")
        ));
        let mosaic = record(&reply, "mosaic").unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(mosaic.get("tiles"), Some("2"));
        assert_eq!(mosaic.get("virtual"), Some("yes"));
        let output = record(&reply, "output").unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(output.get("name"), Some("joined"));
        assert_eq!(output.get("raster"), Some("40x30"));
        assert_eq!(dem.rasters().len(), 1);
        assert_eq!(
            dem.rasters()[0]
                .source
                .extension()
                .and_then(|extension| extension.to_str()),
            Some("vrt")
        );

        let compared = dem.ok(&format!(
            "GDAL raster compare --skip-binary FROM input RASTER joined FROM reference FILE \
             \"{}\"",
            dem.at("whole.tif")
        ));
        holds(&compared, "return code=0");
    }

    #[test]
    fn a_folder_or_a_pattern_is_its_rasters_in_name_order() {
        let _shared = shared_data();
        let mut dem = Dem::new("AFolderOrAPatternIsItsRastersInNameOrder");
        dem.folder("tiles");
        dem.ok(&format!(
            "GDAL raster clip --window=0,0,20,30 FROM FILE \"{}\" TO FILE \"{}\"",
            plane(),
            dem.at("tiles/a.tif")
        ));
        dem.ok(&format!(
            "GDAL raster clip --window=20,0,20,30 FROM FILE \"{}\" TO FILE \"{}\"",
            plane(),
            dem.at("tiles/b.tif")
        ));
        // A file GDAL does not read as a raster is no tile of a folder.
        let note = dem.at("tiles/readme.txt");
        if let Err(error) = std::fs::write(&note, "survey tiles\n") {
            panic!("{note}: {error}");
        }
        let folder = dem.ok(&format!("RASTER MOSAIC FILE \"{}\"", dem.at("tiles")));
        assert_eq!(
            record(&folder, "mosaic")
                .unwrap_or_else(|| panic!("{folder}"))
                .get("tiles"),
            Some("2"),
            "{folder}"
        );
        assert_eq!(
            record(&folder, "output")
                .unwrap_or_else(|| panic!("{folder}"))
                .get("raster"),
            Some("40x30"),
            "{folder}"
        );
        let pattern = dem.ok(&format!("RASTER MOSAIC FILE \"{}\"", dem.at("tiles/*.tif")));
        assert_eq!(
            record(&pattern, "mosaic")
                .unwrap_or_else(|| panic!("{pattern}"))
                .get("tiles"),
            Some("2"),
            "{pattern}"
        );
        // The second takes the next name: mosaic-2.
        assert_eq!(
            record(&pattern, "output")
                .unwrap_or_else(|| panic!("{pattern}"))
                .get("name"),
            Some("mosaic-2"),
            "{pattern}"
        );
        let none = refused(
            dem.run(&format!("RASTER MOSAIC FILE \"{}\"", dem.at("tiles/*.jp2"))),
            "a pattern that names no file",
        );
        assert_eq!(none.code, ErrorCode::NotFound);
    }

    #[test]
    fn a_mixed_colour_interpretation_is_refused_in_gdals_words() {
        // plane.asc's band says nothing of its colour (Undefined); a GeoTIFF
        // GDAL writes says Gray. GDAL's mosaic refuses the mix, and says so.
        let _shared = shared_data();
        let mut dem = Dem::new("AMixedColourInterpretationIsRefusedInGdalsWords");
        dem.ok(&format!(
            "GDAL raster clip --window=20,0,20,30 FROM FILE \"{}\" TO FILE \"{}\"",
            plane(),
            dem.at("east.tif")
        ));
        let error = refused(
            dem.run(&format!(
                "RASTER MOSAIC FILE \"{}\" FILE \"{}\"",
                plane(),
                dem.at("east.tif")
            )),
            "a mosaic of a mixed colour interpretation",
        );
        assert_eq!(error.code, ErrorCode::CommandRejected);
        holds(&error.describe(), "color interpretation");
        assert!(dem.rasters().is_empty());
    }

    #[test]
    fn save_writes_the_mosaic_and_keeps_that_file() {
        let _shared = shared_data();
        let mut dem = Dem::new("SaveWritesTheMosaicAndKeepsThatFile");
        dem.ok(&format!(
            "GDAL raster clip --window=0,0,20,30 FROM FILE \"{}\" TO FILE \"{}\"",
            plane(),
            dem.at("west.tif")
        ));
        dem.ok(&format!(
            "GDAL raster clip --window=20,0,20,30 FROM FILE \"{}\" TO FILE \"{}\"",
            plane(),
            dem.at("east.tif")
        ));
        let saved = dem.at("site.tif");
        let reply = dem.ok(&format!(
            "RASTER MOSAIC FILE \"{}\" FILE \"{}\" SAVE \"{saved}\"",
            dem.at("west.tif"),
            dem.at("east.tif")
        ));
        assert_eq!(
            record(&reply, "mosaic")
                .unwrap_or_else(|| panic!("{reply}"))
                .get("virtual"),
            Some("no"),
            "{reply}"
        );
        assert_eq!(dem.rasters().len(), 1);
        assert_eq!(dem.rasters()[0].source, PathBuf::from(&saved));
        assert_eq!(
            record(&reply, "output")
                .unwrap_or_else(|| panic!("{reply}"))
                .get("persisted"),
            Some("yes")
        );
        // A file in the way is kept unless the line says OVERWRITE.
        let again = refused(
            dem.run(&format!(
                "RASTER MOSAIC FILE \"{}\" SAVE \"{saved}\"",
                dem.at("west.tif")
            )),
            "a SAVE over a file without OVERWRITE",
        );
        assert_eq!(again.code, ErrorCode::AlreadyExists);
    }

    // ---- CLIP -----------------------------------------------------------------------------------

    #[test]
    fn clip_to_an_area_has_hand_computed_size() {
        // 0..20 x 0..15 on 1 m cells: 20 x 15 cells, by hand.
        let _shared = shared_data();
        let mut dem = Dem::new("ClipToAnAreaHasHandComputedSize");
        let reply = dem.ok(&format!("RASTER CLIP FILE \"{}\" AREA 0,0,20,15", plane()));
        let clip = record(&reply, "clip").unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(clip.get("by"), Some("area"));
        assert_eq!(clip.get("area"), Some("0,0,20,15"));
        let output = record(&reply, "output").unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(output.get("raster"), Some("20x15"), "{reply}");
        assert_eq!(output.get("name"), Some("clip"));
    }

    #[test]
    fn clip_to_a_drawn_boundary_keeps_the_cells_inside_it() {
        // A 10 x 10 m rectangle on whole metres: the 100 cells inside it,
        // whose values are the plane's (x = 10.5 at its first column:
        // 100.525).
        let _shared = shared_data();
        let mut dem = Dem::new("ClipToADrawnBoundaryKeepsTheCellsInsideIt");
        dem.fixture.type_line("RECT 10,5 20,15");
        dem.fixture.type_line("LINE 0,0 40,30");
        let reply = dem.ok(&format!(
            "RASTER CLIP FILE \"{}\" DRAWING NAME site",
            plane()
        ));
        let clip = record(&reply, "clip").unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(clip.get("by"), Some("boundaries"));
        // The line bounds nothing: one boundary, the rectangle.
        assert_eq!(clip.get("boundaries"), Some("1"));
        assert_eq!(
            record(&reply, "scope")
                .expect("a scope record")
                .get("matched"),
            Some("2")
        );
        assert_eq!(
            record(&reply, "output")
                .expect("an output record")
                .get("raster"),
            Some("10x10"),
            "{reply}"
        );
        let clipped = band(&dem.last_source());
        assert_eq!(clipped.values.len(), 100);
        // Float32 near 100: ulp 7.6e-6.
        near(clipped.values[0], 100.525, 7.6e-6);
    }

    #[test]
    fn cells_outside_a_boundary_hold_no_value_when_the_raster_has_no_no_data() {
        // plane.asc declares no no-data value. GDAL's raster clip keeps every
        // cell a boundary touches (the rule
        // SlopeVerbs.AScopeKeepsTheAnalysisInsideItsClosedShapes counts by
        // hand: 66 of 11 x 11). A right triangle with legs of 20.25 m from the
        // origin touches cell (i, j) when its lower-left corner is inside, i +
        // j < 20.25: i + j <= 20, 21 + 20 + ... + 1 = 231 cells, no corner on
        // the edge. The cells of its box outside it hold no value - not 0,
        // which read as a height of 0 m.
        let _shared = shared_data();
        let mut dem = Dem::new("CellsOutsideABoundaryHoldNoValueWhenTheRasterHasNoNoData");
        dem.fixture.type_line("PLINE 0,0 20.25,0 0,20.25 C");
        dem.ok(&format!(
            "RASTER CLIP FILE \"{}\" DRAWING NAME tri",
            plane()
        ));
        let clipped = band(&dem.last_source());
        let holding = clipped
            .values
            .iter()
            .filter(|&&cell| holds_value(&clipped, cell))
            .count();
        assert_eq!(holding, 231);
        for row in 0..clipped.height {
            for column in 0..clipped.width {
                let cell = clipped.values[row as usize * clipped.width as usize + column as usize];
                let x =
                    clipped.geotransform[0] + (f64::from(column) + 0.5) * clipped.geotransform[1];
                if holds_value(&clipped, cell) {
                    // The plane: 100 + 0.05 x (plane.asc), Float32 near 100.
                    near(cell, 100.0 + x / 20.0, 7.6e-6);
                }
            }
        }
    }

    #[test]
    fn a_reference_named_preview_is_made_not_previewed_once_quoted() {
        // TO REFERENCE PREVIEW read PREVIEW as the flag: nothing was made, and
        // the reply said so as a preview. Quoted, it is the name.
        let _shared = shared_data();
        let mut dem = Dem::new("AReferenceNamedPreviewIsMadeNotPreviewedOnceQuoted");
        let clip = format!(
            "RASTER CLIP FILE \"{}\" AREA 0,0,20,15 TO REFERENCE ",
            plane()
        );
        holds(&dem.ok(&format!("{clip}PREVIEW")), "preview=yes");
        assert!(dem.rasters().is_empty());
        let quoted = line_word("PREVIEW").expect("a word a line can carry");
        let reply = dem.ok(&format!("{clip}{quoted}"));
        assert_eq!(
            record(&reply, "output")
                .unwrap_or_else(|| panic!("{reply}"))
                .get("name"),
            Some("PREVIEW"),
            "{reply}"
        );
        assert_eq!(dem.rasters().len(), 1);
    }

    #[test]
    fn a_scope_with_no_closed_boundary_is_reported_and_nothing_runs() {
        let _shared = shared_data();
        let mut dem = Dem::new("AScopeWithNoClosedBoundaryIsReportedAndNothingRuns");
        dem.fixture.type_line("LINE 0,0 40,30");
        let reply = dem.ok(&format!("RASTER CLIP FILE \"{}\" DRAWING", plane()));
        let clip = record(&reply, "clip").unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(clip.get("ran"), Some("no"), "{reply}");
        assert_eq!(clip.get("boundaries"), Some("0"));
        assert!(dem.rasters().is_empty());
    }

    // ---- FILL -----------------------------------------------------------------------------------

    #[test]
    fn fill_of_a_hole_in_a_plane_restores_the_plane_within_tolerance() {
        // A 3 x 3 hole, columns 19-21 and rows 14-16, in the plane. GDAL fills
        // a cell with an inverse-distance mean of the nearest valid cells it
        // finds round it - positive weights, so a value between the least and
        // the greatest of those. They lie on the one-cell ring round the hole,
        // columns 18-22, where the plane runs from 100 + 0.05 x 18.5 = 100.925
        // to 100 + 0.05 x 22.5 = 101.125: every filled cell lies in that range
        // (the weights are not symmetric, so the centre is not the plane's own
        // value).
        let _shared = shared_data();
        let mut dem = Dem::new("FillOfAHoleInAPlaneRestoresThePlaneWithinTolerance");
        let no_data = -9999.0;
        let in_hole =
            |column: i32, row: i32| (19..=21).contains(&column) && (14..=16).contains(&row);
        write_plane(
            &dem.at("hole.tif"),
            |value, column, row| {
                if in_hole(column, row) { no_data } else { value }
            },
            Some(no_data),
        );
        let reply = dem.ok(&format!("RASTER FILL FILE \"{}\"", dem.at("hole.tif")));
        let fill = record(&reply, "fill").unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(fill.get("empty"), Some("9"));
        assert_eq!(fill.get("filled"), Some("9"));
        assert_eq!(fill.get("remaining"), Some("0"));
        let filled = band(&dem.last_source());
        let original = band(Path::new(&plane()));
        assert_eq!(filled.values.len(), original.values.len());
        for row in 0..filled.height {
            for column in 0..filled.width {
                let i = row as usize * filled.width as usize + column as usize;
                if in_hole(column, row) {
                    assert!(filled.values[i] >= 100.925 - 1e-9, "{column},{row}");
                    assert!(filled.values[i] <= 101.125 + 1e-9, "{column},{row}");
                } else {
                    // Everything else is the input, untouched.
                    assert_eq!(filled.values[i], original.values[i], "{column},{row}");
                }
            }
        }
    }

    #[test]
    fn a_raster_with_nothing_to_fill_says_so_and_makes_nothing() {
        let _shared = shared_data();
        let mut dem = Dem::new("ARasterWithNothingToFillSaysSoAndMakesNothing");
        let reply = dem.ok(&format!("RASTER FILL FILE \"{}\"", plane()));
        let fill = record(&reply, "fill").unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(fill.get("empty"), Some("0"), "{reply}");
        assert_eq!(fill.get("ran"), Some("no"));
        assert!(dem.rasters().is_empty());
    }

    // ---- FOOTPRINT ------------------------------------------------------------------------------

    #[test]
    fn footprint_of_a_full_raster_is_its_extent_rectangle() {
        // Every cell of the plane holds a value, so where it has data is its
        // extent: the rectangle 0,0 - 40,30, 1200 m2.
        let _shared = shared_data();
        let mut dem = Dem::new("FootprintOfAFullRasterIsItsExtentRectangle");
        let reply = dem.ok(&format!("RASTER FOOTPRINT FILE \"{}\"", plane()));
        let output = record(&reply, "output").unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(output.get("layer"), Some("gis/footprint"));
        assert_eq!(output.get("created"), Some("1"));
        assert_eq!(dem.fixture.entity_count(), 1);
        let entities = &dem.fixture.document.model().entities;
        let footprint = entities
            .find(entities.ids()[0])
            .expect("the footprint's entity");
        let Geometry::Polyline(polyline) = &footprint.geometry else {
            panic!("the footprint is a polyline");
        };
        assert!(polyline.closed);
        let bounds = bounding_box(&footprint.geometry);
        double_eq(bounds.min.x, 0.0);
        double_eq(bounds.min.y, 0.0);
        double_eq(bounds.max.x, 40.0);
        double_eq(bounds.max.y, 30.0);
        // The shoelace formula.
        let mut twice: f64 = 0.0;
        let vertices = &polyline.vertices;
        for (i, p) in vertices.iter().enumerate() {
            let q = &vertices[(i + 1) % vertices.len()];
            twice += p.x * q.y - q.x * p.y;
        }
        double_eq(twice.abs() / 2.0, 1200.0);
    }

    // ---- REPROJECT ------------------------------------------------------------------------------

    #[test]
    fn reproject_to_the_same_crs_keeps_values() {
        // The plane placed in GDA94 / MGA zone 56, then reprojected into the
        // system it is in: the same grid, the same values.
        let _shared = shared_data();
        let mut dem = Dem::new("ReprojectToTheSameCrsKeepsValues");
        dem.ok(&format!(
            "GDAL raster reproject --input-crs=EPSG:28356 --output-crs=EPSG:28356 FROM FILE \
             \"{}\" TO FILE \"{}\"",
            plane(),
            dem.at("mga.tif")
        ));
        let reply = dem.ok(&format!(
            "RASTER REPROJECT FILE \"{}\" crs=EPSG:28356",
            dem.at("mga.tif")
        ));
        assert_eq!(
            record(&reply, "reproject")
                .unwrap_or_else(|| panic!("{reply}"))
                .get("crs"),
            Some("EPSG:28356"),
            "{reply}"
        );
        let before = band(Path::new(&dem.at("mga.tif")));
        let after = band(&dem.last_source());
        assert_eq!(after.width, before.width);
        assert_eq!(after.height, before.height);
        assert_eq!(after.geotransform, before.geotransform);
        assert_eq!(after.values, before.values);
    }

    #[test]
    fn a_raster_with_no_coordinate_system_is_refused_rather_than_guessed() {
        // plane.asc has no .prj. GDAL would hand it back unchanged as if it
        // were in the system asked for; the verb asks where it is instead.
        let _shared = shared_data();
        let mut dem = Dem::new("ARasterWithNoCoordinateSystemIsRefusedRatherThanGuessed");
        let error = refused(
            dem.run(&format!(
                "RASTER REPROJECT FILE \"{}\" crs=EPSG:28356",
                plane()
            )),
            "a raster with no coordinate system",
        );
        assert_eq!(error.code, ErrorCode::InvalidCrs);
        holds(&error.describe(), "from=");
        assert!(dem.rasters().is_empty());
        // Said where it is, it runs.
        let reply = dem.ok(&format!(
            "RASTER REPROJECT FILE \"{}\" from=EPSG:28356 crs=EPSG:28356",
            plane()
        ));
        assert_eq!(
            record(&reply, "output")
                .unwrap_or_else(|| panic!("{reply}"))
                .get("raster"),
            Some("40x30"),
            "{reply}"
        );
        // And a drawing with no coordinate system has none to reproject to.
        let nowhere = refused(
            dem.run(&format!("RASTER REPROJECT FILE \"{}\"", plane())),
            "a drawing with no coordinate system",
        );
        assert_eq!(nowhere.code, ErrorCode::InvalidCrs);
    }

    // ---- DIFFERENCE -----------------------------------------------------------------------------

    #[test]
    fn difference_of_a_dem_and_itself_raised_three_tenths_is0_point3_everywhere() {
        // The plane raised 0.3 m, minus the plane: 0.3 in every cell. The
        // raised copy is written from the very values GDAL reads the plane as,
        // so each cell is (v + 0.3) - v in Float64: 0.3 to within an ulp of
        // 100 (1.4e-14).
        let _shared = shared_data();
        let mut dem = Dem::new("DifferenceOfADemAndItselfRaisedThreeTenthsIs0Point3Everywhere");
        write_plane(&dem.at("raised.tif"), |value, _, _| value + 0.3, None);
        let reply = dem.ok(&format!(
            "RASTER DIFFERENCE FILE \"{}\" FILE \"{}\"",
            dem.at("raised.tif"),
            plane()
        ));
        let difference = record(&reply, "difference").unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(difference.get("aligned"), Some("yes"));
        let result = band(&dem.last_source());
        assert_eq!(result.values.len(), 1200);
        for &cell in &result.values {
            near(cell, 0.3, 1e-9);
        }
    }

    #[test]
    fn its_fill_volume_is_point3_times_the_area() {
        // 0.3 m over the plane's 40 x 30 m: 0.3 x 1200 = 360 m3 of fill, no
        // cut.
        let _shared = shared_data();
        let mut dem = Dem::new("ItsFillVolumeIsPoint3TimesTheArea");
        write_plane(&dem.at("raised.tif"), |value, _, _| value + 0.3, None);
        let reply = dem.ok(&format!(
            "RASTER DIFFERENCE FILE \"{}\" FILE \"{}\"",
            dem.at("raised.tif"),
            plane()
        ));
        let difference = record(&reply, "difference").unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(difference.get("cells"), Some("1200"));
        assert_eq!(difference.get("area"), Some("1200.000"));
        assert_eq!(difference.get("fill"), Some("360.000"));
        assert_eq!(difference.get("cut"), Some("0.000"));
        assert_eq!(difference.get("net"), Some("360.000"));
        assert_eq!(difference.get("cell"), Some("1"));
        assert_eq!(difference.get("label"), Some("grid method, cell 1 m"));
        // The other way round it is cut.
        let reversed = dem.ok(&format!(
            "RASTER DIFFERENCE FILE \"{}\" FILE \"{}\"",
            plane(),
            dem.at("raised.tif")
        ));
        let other = record(&reversed, "difference").unwrap_or_else(|| panic!("{reversed}"));
        assert_eq!(other.get("cut"), Some("360.000"), "{reversed}");
        assert_eq!(other.get("net"), Some("-360.000"));
    }

    #[test]
    fn minus_between_the_rasters_says_which_is_taken_from_which() {
        // The design's grammar: DIFFERENCE <source> MINUS <source>. With or
        // without the word it is the first minus the second: 0.3 m over 1200
        // m2 is 360 m3 of fill.
        let _shared = shared_data();
        let mut dem = Dem::new("MinusBetweenTheRastersSaysWhichIsTakenFromWhich");
        write_plane(&dem.at("raised.tif"), |value, _, _| value + 0.3, None);
        let raised = format!("FILE \"{}\"", dem.at("raised.tif"));
        let source = format!("FILE \"{}\"", plane());
        let reply = dem.ok(&format!("RASTER DIFFERENCE {raised} MINUS {source}"));
        let difference = record(&reply, "difference").unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(difference.get("fill"), Some("360.000"), "{reply}");
        assert_eq!(difference.get("cut"), Some("0.000"));
        for line in [
            format!("RASTER DIFFERENCE MINUS {raised} {source}"),
            format!("RASTER DIFFERENCE {raised} {source} MINUS"),
            format!("RASTER DIFFERENCE {raised} MINUS MINUS {source}"),
            format!("RASTER CLIP {raised} MINUS AREA 0,0,1,1"),
        ] {
            let error = refused(dem.run(&line), &line);
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{line}");
        }
    }

    #[test]
    fn a_scope_limits_the_difference_to_its_boundaries() {
        // A 10 x 10 m rectangle: 100 cells of 0.3 m, 30 m3.
        let _shared = shared_data();
        let mut dem = Dem::new("AScopeLimitsTheDifferenceToItsBoundaries");
        write_plane(&dem.at("raised.tif"), |value, _, _| value + 0.3, None);
        dem.fixture.type_line("RECT 0,0 10,10");
        let reply = dem.ok(&format!(
            "RASTER DIFFERENCE FILE \"{}\" FILE \"{}\" DRAWING",
            dem.at("raised.tif"),
            plane()
        ));
        let difference = record(&reply, "difference").unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(difference.get("cells"), Some("100"));
        assert_eq!(difference.get("fill"), Some("30.000"));
        assert!(record(&reply, "scope").is_some());
    }

    #[test]
    fn a_boundary_on_rasters_with_no_no_data_sums_only_the_cells_inside_it() {
        // Neither raster declares a no-data value. The triangle of the clip
        // test above touches 231 cells of 0.3 m: 231 m2 and 69.3 m3 of fill.
        // The cells of its box outside it are no part of the difference, not
        // cells of 0.
        let _shared = shared_data();
        let mut dem = Dem::new("ABoundaryOnRastersWithNoNoDataSumsOnlyTheCellsInsideIt");
        write_plane(&dem.at("raised.tif"), |value, _, _| value + 0.3, None);
        dem.fixture.type_line("PLINE 0,0 20.25,0 0,20.25 C");
        let reply = dem.ok(&format!(
            "RASTER DIFFERENCE FILE \"{}\" FILE \"{}\" DRAWING",
            dem.at("raised.tif"),
            plane()
        ));
        let difference = record(&reply, "difference").unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(difference.get("cells"), Some("231"), "{reply}");
        assert_eq!(difference.get("area"), Some("231.000"));
        assert_eq!(difference.get("fill"), Some("69.300"));
    }

    #[test]
    fn a_second_raster_on_another_grid_is_aligned_to_the_first() {
        // The raised plane on 2 m cells over the same 40 x 30 m. Aligned to
        // the plane's 1 m grid by bilinear interpolation, which reproduces a
        // linear function exactly between the centres it interpolates: every
        // 1 m cell whose centre has 2 m centres on both sides (x from 1 to 39)
        // differs by 0.3. Both are written from the plane's exact values in
        // Float64, so only the interpolation's own rounding is left (1e-9).
        let _shared = shared_data();
        let mut dem = Dem::new("ASecondRasterOnAnotherGridIsAlignedToTheFirst");
        let mut coarse = Vec::new();
        for _ in 0..15 {
            for column in 0..20 {
                coarse.push(100.0 + 0.05 * (2.0 * f64::from(column) + 1.0) + 0.3);
            }
        }
        let options = RasterExportOptions {
            width: 20,
            height: 15,
            geotransform: [0.0, 2.0, 0.0, 30.0, 0.0, -2.0],
            ..RasterExportOptions::default()
        };
        must(GdalDataset::write_raster(
            Path::new(&dem.at("coarse.tif")),
            &options,
            &coarse,
        ));
        write_plane(
            &dem.at("fine.tif"),
            |_, column, _| 100.0 + 0.05 * (f64::from(column) + 0.5),
            None,
        );
        let reply = dem.ok(&format!(
            "RASTER DIFFERENCE FILE \"{}\" FILE \"{}\"",
            dem.at("coarse.tif"),
            dem.at("fine.tif")
        ));
        // The first is the grid: the coarse one.
        let difference = record(&reply, "difference").unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(difference.get("aligned"), Some("no"));
        assert_eq!(difference.get("cell"), Some("2"));
        let back = dem.ok(&format!(
            "RASTER DIFFERENCE FILE \"{}\" FILE \"{}\"",
            dem.at("fine.tif"),
            dem.at("coarse.tif")
        ));
        let difference = record(&back, "difference").unwrap_or_else(|| panic!("{back}"));
        assert_eq!(difference.get("aligned"), Some("no"));
        assert_eq!(difference.get("resampling"), Some("bilinear"));
        let result = band(&dem.last_source());
        assert_eq!(result.width, 40);
        for row in 1..29 {
            for column in 1..39 {
                near(result.values[row * 40 + column], -0.3, 1e-9);
            }
        }
    }

    // ---- what every tool shares ---------------------------------------------------------------

    #[test]
    fn what_the_tools_cannot_do_is_refused_before_anything_runs() {
        let _shared = shared_data();
        let mut dem = Dem::new("WhatTheToolsCannotDoIsRefusedBeforeAnythingRuns");
        let mut refuses = |line: String, code: ErrorCode, says: &str| {
            let error = refused(dem.run(&line), &line);
            assert_eq!(error.code, code, "{line}: {}", error.describe());
            holds(&error.describe(), says);
        };
        let source = format!("FILE \"{}\"", plane());
        refuses(
            format!("RASTER CLIP {source}"),
            ErrorCode::InvalidArgument,
            "AREA",
        );
        refuses(
            format!("RASTER CLIP {source} AREA 0,0,10,10 WHERE TYPE=polyline"),
            ErrorCode::InvalidArgument,
            "box",
        );
        refuses(
            format!("RASTER CLIP {source} AREA 0,0,10,10 TO LAYER dem"),
            ErrorCode::Unsupported,
            "raster",
        );
        refuses(
            format!("RASTER FOOTPRINT {source} TO REFERENCE fp"),
            ErrorCode::Unsupported,
            "LAYER",
        );
        refuses(
            format!("RASTER DIFFERENCE {source}"),
            ErrorCode::InvalidArgument,
            "2 rasters",
        );
        refuses(
            format!("RASTER DIFFERENCE {source} {source} {source}"),
            ErrorCode::InvalidArgument,
            "takes 2",
        );
        refuses(
            format!("RASTER FILL {source} distance=0"),
            ErrorCode::InvalidArgument,
            "whole number",
        );
        refuses(
            format!("RASTER FILL {source} bogus"),
            ErrorCode::InvalidArgument,
            "bogus",
        );
        refuses(
            format!("RASTER REPROJECT {source} crs=EPSG:28356 like=dem"),
            ErrorCode::InvalidArgument,
            "give one",
        );
        refuses(
            String::from("RASTER MOSAIC SURFACE ground"),
            ErrorCode::InvalidArgument,
            "surface",
        );
        refuses(
            String::from("RASTER CLIP RASTER nosuch AREA 0,0,10,10"),
            ErrorCode::NotFound,
            "nosuch",
        );
        refuses(
            String::from("RASTER FILL"),
            ErrorCode::InvalidArgument,
            "needs a raster",
        );
        assert!(dem.rasters().is_empty());
        assert_eq!(dem.fixture.entity_count(), 0);
    }

    #[test]
    fn a_preview_checks_the_line_and_changes_nothing() {
        let _shared = shared_data();
        let mut dem = Dem::new("APreviewChecksTheLineAndChangesNothing");
        let reply = dem.ok(&format!(
            "RASTER CLIP FILE \"{}\" AREA 0,0,20,15 PREVIEW",
            plane()
        ));
        assert_eq!(
            record(&reply, "clip")
                .unwrap_or_else(|| panic!("{reply}"))
                .get("preview"),
            Some("yes"),
            "{reply}"
        );
        assert_eq!(
            record(&reply, "preview")
                .expect("a preview record")
                .get("valid"),
            Some("yes")
        );
        // GDAL's own check opens what the line names: a file that is not
        // there is refused before anything runs.
        let missing = dem.run(&format!(
            "RASTER CLIP FILE \"{}\" AREA 0,0,20,15 PREVIEW",
            dem.at("nosuch.tif")
        ));
        assert!(missing.is_err());
        assert!(dem.rasters().is_empty());
    }

    #[test]
    fn a_cancelled_tool_adds_nothing() {
        let _shared = shared_data();
        let mut dem = Dem::new("ACancelledToolAddsNothing");
        let prepared =
            must(dem.prepare(&format!("RASTER CLIP FILE \"{}\" AREA 0,0,20,15", plane())));
        assert!(prepared.reply.is_none());
        let stop = AtomicBool::new(true);
        let work = prepared.work.expect("a run has work");
        let error = refused(work(Some(&stop), None), "a stopped tool");
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert!(dem.rasters().is_empty());
    }

    #[test]
    fn the_tools_are_in_the_help_every_front_end_shows() {
        for verb in [
            "MOSAIC",
            "CLIP",
            "FILL",
            "FOOTPRINT",
            "REPROJECT",
            "DIFFERENCE",
        ] {
            assert!(handles(&format!("RASTER {verb}")), "{verb}");
            holds(&help_text(), &format!("RASTER {verb}"));
        }
    }
}
