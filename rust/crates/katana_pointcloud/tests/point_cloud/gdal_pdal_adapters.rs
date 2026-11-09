//! tests/io/test_gdal_pdal_adapters.cpp: the PointCloudEngine suite (PLAN.MD
//! Phase 17).

mod point_cloud_engine {
    use std::path::Path;

    use katana_core::ErrorCode;
    use katana_pointcloud::{PointCloud, PointCloudEngine, PointCloudPoint, PointCloudReadOptions};

    use crate::{Scratch, near, ok};

    // The C++ aggregate {x, y, z, intensity, classification, red, green, blue,
    // hasColor}, the colour as one triple.
    fn point(
        x: f64,
        y: f64,
        z: f64,
        intensity: f64,
        classification: u8,
        [red, green, blue]: [u8; 3],
        has_color: bool,
    ) -> PointCloudPoint {
        PointCloudPoint {
            x,
            y,
            z,
            intensity,
            classification,
            red,
            green,
            blue,
            has_color,
        }
    }

    fn cloud_of(points: Vec<PointCloudPoint>) -> PointCloud {
        PointCloud {
            points,
            ..PointCloud::default()
        }
    }

    fn read_all(engine: &PointCloudEngine, path: &Path) -> PointCloud {
        ok(engine.read(path, &PointCloudReadOptions::default()))
    }

    #[test]
    fn rejects_zero_decimation_step_without_throwing() {
        let options = PointCloudReadOptions {
            decimation_step: 0,
            ..PointCloudReadOptions::default()
        };
        let result = PointCloudEngine.read(Path::new("missing.las"), &options);
        let error = result.expect_err("a zero decimation step is refused");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn missing_file_is_not_found() {
        let result = PointCloudEngine.read(
            Path::new("definitely-not-here.las"),
            &PointCloudReadOptions::default(),
        );
        let error = result.expect_err("a missing file is refused");
        assert_eq!(error.code, ErrorCode::NotFound);

        let header = PointCloudEngine.read_header(Path::new("definitely-not-here.las"));
        let error = header.expect_err("a missing file has no header");
        assert_eq!(error.code, ErrorCode::NotFound);
    }

    #[test]
    fn empty_bounds_are_empty_not_zero_sized() {
        // A default bounds must report empty rather than claiming to be a
        // degenerate box at the origin, or an empty cloud would drag Zoom
        // Extents to 0,0.
        let cloud = PointCloud::default();
        assert!(cloud.points.is_empty());
        assert!(cloud.bounds.empty());
    }

    #[test]
    fn decimation_for_budget_rounds_up_and_never_returns_zero() {
        // A step of 0 would be an infinite loop or a division by zero
        // downstream, so the floor is 1 in every case.
        assert_eq!(PointCloudEngine::decimation_for_budget(0, 1000), 1);
        assert_eq!(PointCloudEngine::decimation_for_budget(500, 1000), 1);
        assert_eq!(PointCloudEngine::decimation_for_budget(1000, 1000), 1);
        assert_eq!(PointCloudEngine::decimation_for_budget(1000, 0), 1); // 0 = no budget

        // Rounded UP, so the result is at or under the budget rather than
        // just over: 2001/1000 must be 3, not 2, since a step of 2 would yield
        // 1001 points.
        assert_eq!(PointCloudEngine::decimation_for_budget(2001, 1000), 3);
        assert_eq!(PointCloudEngine::decimation_for_budget(2000, 1000), 2);

        // A billion-point file against a two-million budget: this is the case
        // Phase 17 exists for.
        let step = PointCloudEngine::decimation_for_budget(1_000_000_000, 2_000_000);
        assert_eq!(step, 500);
        assert!(1_000_000_000_u64 / u64::from(step) <= 2_000_000_u64);
    }

    #[test]
    fn writes_and_reads_las_preserving_fields() {
        let scratch = Scratch::new("writes_and_reads_las_preserving_fields");
        let file = scratch.file("cloud.las");
        let source = cloud_of(vec![
            point(1.0, 2.0, 3.0, 10.0, 2, [0, 0, 0], false),
            point(4.0, 5.0, 6.0, 20.0, 5, [0, 0, 0], false),
        ]);

        let engine = PointCloudEngine;
        ok(engine.write(&file, &source));
        assert!(file.exists());

        let loaded = read_all(&engine, &file);
        assert_eq!(loaded.points.len(), 2);

        // LAS stores coordinates as scaled integers, so exact equality is not
        // guaranteed by the format. The writer sets a 0.001 scale (PDAL's own
        // default is 0.01; audit IO-17), and whole numbers are representable
        // at either - SurveyCoordinatesSurviveToTheMillimetre is the test of
        // the scale.
        near(loaded.points[0].x, 1.0, 1e-6);
        near(loaded.points[1].z, 6.0, 1e-6);
        assert_eq!(loaded.points[0].classification, 2);
        assert_eq!(loaded.points[1].classification, 5);

        // Bounds are derived from the points that were actually read.
        near(loaded.bounds.min_x, 1.0, 1e-6);
        near(loaded.bounds.max_x, 4.0, 1e-6);
        assert!(!loaded.bounds.empty());
    }

    #[test]
    fn color_survives_the_round_trip() {
        let scratch = Scratch::new("color_survives_the_round_trip");
        let file = scratch.file("colour.las");
        let source = cloud_of(vec![point(0.0, 0.0, 0.0, 1.0, 2, [255, 128, 0], true)]);

        let engine = PointCloudEngine;
        ok(engine.write(&file, &source));
        let loaded = read_all(&engine, &file);
        assert_eq!(loaded.points.len(), 1);

        // Colour goes out as 16-bit and comes back scaled to 8-bit, so it is
        // preserved to within the rounding of that conversion rather than
        // exactly.
        assert!(loaded.points[0].has_color);
        near(f64::from(loaded.points[0].red), 255.0, 1.0);
        near(f64::from(loaded.points[0].green), 128.0, 1.0);
        assert_eq!(loaded.points[0].blue, 0);
    }

    #[test]
    fn header_reports_the_count_without_reading_points() {
        let scratch = Scratch::new("header_reports_the_count_without_reading_points");
        let file = scratch.file("header.las");
        let source = cloud_of(
            (0..500)
                .map(|i| point(f64::from(i), 0.0, 0.0, 0.0, 2, [0, 0, 0], false))
                .collect(),
        );
        let engine = PointCloudEngine;
        ok(engine.write(&file, &source));

        let header = ok(engine.read_header(&file));
        assert_eq!(header.point_count, 500);
        near(header.bounds.max_x, 499.0, 1e-6);
    }

    #[test]
    fn decimation_and_classification_filters_apply() {
        let scratch = Scratch::new("decimation_and_classification_filters_apply");
        let file = scratch.file("filtered.las");
        let source = cloud_of(
            (0..100)
                .map(|i| {
                    let classification = if i % 2 == 0 { 2 } else { 5 };
                    point(
                        f64::from(i),
                        0.0,
                        0.0,
                        0.0,
                        classification,
                        [0, 0, 0],
                        false,
                    )
                })
                .collect(),
        );
        let engine = PointCloudEngine;
        ok(engine.write(&file, &source));

        let ground_only = PointCloudReadOptions {
            classification: Some(2),
            ..PointCloudReadOptions::default()
        };
        let ground = ok(engine.read(&file, &ground_only));
        assert_eq!(ground.points.len(), 50);
        assert!(ground.points.iter().all(|p| p.classification == 2));
        // The source count reports the FILE's size, not the filtered result,
        // so a caller can say "50 of 100" honestly.
        assert_eq!(ground.source_point_count, 100);

        let every_tenth = PointCloudReadOptions {
            decimation_step: 10,
            ..PointCloudReadOptions::default()
        };
        let sampled = ok(engine.read(&file, &every_tenth));
        assert!(sampled.points.len() <= 11);
        assert!(sampled.points.len() >= 9);
    }

    #[test]
    fn refuses_to_write_an_empty_cloud() {
        let scratch = Scratch::new("refuses_to_write_an_empty_cloud");
        let file = scratch.file("empty.las");
        let status = PointCloudEngine.write(&file, &PointCloud::default());
        let error = status.expect_err("an empty cloud is refused");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(!file.exists());
    }

    // ---- COPC: level of detail as a query (PLAN.MD Phase 17) --------------------------------

    // A 100 m square of ground at 0.25 m spacing: 160 000 points.
    //
    // The spacing is what makes this a test of level of detail at all. A COPC
    // octree node keeps at most one point per cell of a 128-cell grid across
    // its span, so on a 100 m root a point every 0.78 m or coarser lands
    // ENTIRELY in the root node, and every resolution query then returns the
    // whole file - a fixture that could never show a coarse read being
    // coarser. A quarter of a metre forces the octree to at least two further
    // levels.
    fn dense_ground() -> PointCloud {
        let mut points = Vec::with_capacity(400 * 400);
        for j in 0..400 {
            for i in 0..400 {
                let x = 0.25 * f64::from(i);
                let y = 0.25 * f64::from(j);
                points.push(point(x, y, 10.0 + 0.01 * x, 1.0, 2, [0, 0, 0], false));
            }
        }
        cloud_of(points)
    }

    #[test]
    fn a_coarse_copc_query_returns_fewer_points_over_the_same_extent() {
        let scratch = Scratch::new("a_coarse_copc_query_returns_fewer_points_over_the_same_extent");
        let las = scratch.file("lod-source.las");
        let copc = scratch.file("lod.copc.laz");
        let engine = PointCloudEngine;
        ok(engine.write(&las, &dense_ground()));
        ok(engine.convert_to_copc(&las, &copc));

        // Converting loses nothing: with no resolution asked, every point is
        // there.
        let full = read_all(&engine, &copc);
        assert_eq!(full.points.len(), 160_000);

        let coarse = PointCloudReadOptions {
            resolution: Some(5.0),
            ..PointCloudReadOptions::default()
        };
        let medium = PointCloudReadOptions {
            resolution: Some(0.5),
            ..PointCloudReadOptions::default()
        };
        let coarse_read = ok(engine.read(&copc, &coarse));
        let medium_read = ok(engine.read(&copc, &medium));

        // The properties of level of detail, not the counts one PDAL version
        // happens to produce: coarser is never more, the coarse read is a
        // real reduction, and it is still a sample of the WHOLE extent rather
        // than the first corner of it - which is the difference between a
        // level of detail and the `max_points` truncation this replaces.
        assert!(!coarse_read.points.is_empty());
        assert!(coarse_read.points.len() <= medium_read.points.len());
        assert!(medium_read.points.len() <= full.points.len());
        assert!(
            coarse_read.points.len() * 4 < full.points.len(),
            "a 5 m query on 0.25 m data should be a small fraction of it"
        );
        assert!(coarse_read.bounds.min_x < 10.0);
        assert!(coarse_read.bounds.max_x > 90.0);
        assert!(coarse_read.bounds.min_y < 10.0);
        assert!(coarse_read.bounds.max_y > 90.0);
        // The honest count is unchanged by the query: it is what the FILE
        // holds.
        assert_eq!(coarse_read.source_point_count, 160_000);
    }

    #[test]
    fn only_a_copc_file_is_reported_as_one() {
        let scratch = Scratch::new("only_a_copc_file_is_reported_as_one");
        let las = scratch.file("plain.las");
        let copc = scratch.file("real.copc.laz");
        let engine = PointCloudEngine;
        ok(engine.write(&las, &dense_ground()));
        ok(engine.convert_to_copc(&las, &copc));

        let plain = engine.is_copc(&las);
        assert!(plain.is_ok());
        assert!(!ok(plain));
        let real = ok(engine.is_copc(&copc));
        assert!(real);
        assert_eq!(
            engine
                .is_copc(Path::new("definitely-not-here.copc.laz"))
                .expect_err("a missing file is not found")
                .code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn a_resolution_asked_of_a_plain_las_is_refused_rather_than_ignored() {
        // readers.las has no resolution option. Passing one through would
        // return the whole file to a caller who asked for a coarse sample of a
        // billion-point cloud - the silent failure PLAN.MD section 36 forbids.
        let scratch =
            Scratch::new("a_resolution_asked_of_a_plain_las_is_refused_rather_than_ignored");
        let las = scratch.file("no-lod.las");
        let engine = PointCloudEngine;
        ok(engine.write(&las, &dense_ground()));
        let options = PointCloudReadOptions {
            resolution: Some(5.0),
            ..PointCloudReadOptions::default()
        };
        let refused = engine
            .read(&las, &options)
            .expect_err("a resolution of a plain LAS is refused");
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
        assert!(
            refused.describe().contains("COPC"),
            "{}",
            refused.describe()
        );
    }

    #[test]
    fn copc_conversion_refuses_what_it_cannot_do() {
        let scratch = Scratch::new("copc_conversion_refuses_what_it_cannot_do");
        let las = scratch.file("convert-source.las");
        let copc = scratch.file("convert.copc.laz");
        let wrong_name = scratch.file("convert-wrong.laz");
        let engine = PointCloudEngine;
        ok(engine.write(&las, &dense_ground()));

        // The extension is what makes every later read infer readers.copc; a
        // COPC file called .laz would be read as plain LAZ and never answer a
        // query.
        assert_eq!(
            engine
                .convert_to_copc(&las, &wrong_name)
                .expect_err("a COPC file must be named .copc.laz")
                .code,
            ErrorCode::InvalidArgument
        );
        assert!(!wrong_name.exists());
        assert_eq!(
            engine
                .convert_to_copc(Path::new("definitely-not-here.las"), &copc)
                .expect_err("a missing source is not found")
                .code,
            ErrorCode::NotFound
        );

        ok(engine.convert_to_copc(&las, &copc));
        let zero = PointCloudReadOptions {
            resolution: Some(0.0),
            ..PointCloudReadOptions::default()
        };
        assert_eq!(
            engine
                .read(&copc, &zero)
                .expect_err("a zero resolution is refused")
                .code,
            ErrorCode::InvalidArgument
        );
        let negative = PointCloudReadOptions {
            resolution: Some(-1.0),
            ..PointCloudReadOptions::default()
        };
        assert_eq!(
            engine
                .read(&copc, &negative)
                .expect_err("a negative resolution is refused")
                .code,
            ErrorCode::InvalidArgument
        );
    }

    // ---- coordinate resolution (audit IO-17) -----------------------------------------------

    // Projected coordinates of survey magnitude - an MGA northing is seven
    // digits - quoted to the millimetre, with fractions that no centimetre
    // grid holds. More than one point, and not all on one grid line: with an
    // automatic offset a LONE point is stored as zero from its own offset and
    // comes back exact at any scale, which would make the test pass over the
    // defect.
    fn survey_magnitude_cloud() -> PointCloud {
        cloud_of(vec![
            point(255440.1234, 7410850.4567, 12.3456, 1.0, 2, [0, 0, 0], false),
            point(255441.9876, 7410851.0021, 13.0009, 1.0, 2, [0, 0, 0], false),
            point(255439.5555, 7410849.7777, 11.1111, 1.0, 2, [0, 0, 0], false),
        ])
    }

    // ASPRS LAS 1.4 R15 stores a coordinate as round((value - offset) /
    // scale), so at a 0.001 scale it comes back within half a millimetre. The
    // 1e-9 is the rounding of reconstructing int * scale + offset in doubles:
    // one half-ulp at 7.4 x 10^6 is 4.7e-10. PDAL's default 0.01 scale misses
    // it on every one of these coordinates, by 0.9 to 4.5 mm (measured with
    // the scale removed: 255440.1234 came back as 255440.12).
    const MILLIMETRE_ROUND_TRIP: f64 = 0.0005 + 1e-9;

    fn expect_millimetres(source: &PointCloud, loaded: &PointCloud) {
        assert_eq!(loaded.points.len(), source.points.len());
        for expected in &source.points {
            // Matched by position, not by index: nothing in LAS promises order.
            let found = loaded
                .points
                .iter()
                .find(|p| (p.x - expected.x).abs() < 0.1 && (p.y - expected.y).abs() < 0.1)
                .unwrap_or_else(|| panic!("no point near x = {}", expected.x));
            near(found.x, expected.x, MILLIMETRE_ROUND_TRIP);
            near(found.y, expected.y, MILLIMETRE_ROUND_TRIP);
            near(found.z, expected.z, MILLIMETRE_ROUND_TRIP);
        }
    }

    #[test]
    fn survey_coordinates_survive_to_the_millimetre() {
        let scratch = Scratch::new("survey_coordinates_survive_to_the_millimetre");
        let file = scratch.file("survey.las");
        let engine = PointCloudEngine;
        let source = survey_magnitude_cloud();
        ok(engine.write(&file, &source));
        let loaded = read_all(&engine, &file);
        expect_millimetres(&source, &loaded);
    }

    #[test]
    fn copc_conversion_keeps_the_sources_millimetres() {
        // writers.copc defaults to a 0.01 scale too. Converting a millimetre
        // LAS must not coarsen it: the whole point of the conversion is to
        // index the data, not to lose some of it.
        let scratch = Scratch::new("copc_conversion_keeps_the_sources_millimetres");
        let las = scratch.file("survey-source.las");
        let copc = scratch.file("survey.copc.laz");
        let engine = PointCloudEngine;
        let source = survey_magnitude_cloud();
        ok(engine.write(&las, &source));
        ok(engine.convert_to_copc(&las, &copc));
        let loaded = read_all(&engine, &copc);
        expect_millimetres(&source, &loaded);
    }

    #[test]
    fn a_laz_is_compressed_and_reads_back_as_the_same_points() {
        // The extension is all a caller says; writers.las has to be told to
        // compress, and a .laz that is really an uncompressed LAS is a file
        // every other reader rejects.
        let scratch = Scratch::new("a_laz_is_compressed_and_reads_back_as_the_same_points");
        let las = scratch.file("compress.las");
        let laz = scratch.file("compress.laz");
        let engine = PointCloudEngine;
        let source = dense_ground();
        ok(engine.write(&las, &source));
        ok(engine.write(&laz, &source));

        let las_size = std::fs::metadata(&las).expect("the LAS is there").len();
        let laz_size = std::fs::metadata(&laz).expect("the LAZ is there").len();
        // A regular grid compresses by far more than this; half is a floor
        // that no uncompressed file of the same points can meet.
        assert!(
            laz_size * 2 < las_size,
            "LAS {las_size} bytes, LAZ {laz_size}"
        );

        let from_las = read_all(&engine, &las);
        let from_laz = read_all(&engine, &laz);
        assert_eq!(from_laz.points.len(), source.points.len());
        assert_eq!(from_laz.points.len(), from_las.points.len());
        // Compression is lossless: the same integers, so the same doubles.
        for (i, (laz_point, las_point)) in from_laz.points.iter().zip(&from_las.points).enumerate()
        {
            assert_eq!(laz_point.x, las_point.x, "{i}");
            assert_eq!(laz_point.y, las_point.y, "{i}");
            assert_eq!(laz_point.z, las_point.z, "{i}");
        }
    }
}
