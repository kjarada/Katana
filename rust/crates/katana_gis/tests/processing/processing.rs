//! tests/geo/test_processing.cpp: the GDAL algorithm bridge
//! (katana_gis::processing, docs/geoprocessing.md "The bridge"): the
//! catalogue, argument specs, runs with in-memory datasets, cancel, errors,
//! staging and sidecars, each against a value worked out by hand and quoted
//! beside it.

use katana_gis::processing::{
    AlgorithmSpec, ArgSpec, ArgValue, DatasetPath, DatasetValue, Feature, FeatureTable, RasterGrid,
    RunRequest,
};
use katana_gis::{GeoPoint, GeometryKind, RasterInfo, VectorGeometry};

/// A north-up grid of `cell` squares whose top-left corner is (0, height).
fn grid(width: i32, height: i32, cell: f64, z_of: impl Fn(f64, f64) -> f64) -> RasterGrid {
    let mut values = Vec::with_capacity(width as usize * height as usize);
    for row in 0..height {
        for column in 0..width {
            values.push(z_of(
                (f64::from(column) + 0.5) * cell,
                (f64::from(height - row) - 0.5) * cell,
            ));
        }
    }
    RasterGrid {
        info: RasterInfo {
            width,
            height,
            band_count: 1,
            geotransform: [0.0, cell, 0.0, f64::from(height) * cell, 0.0, -cell],
            has_geotransform: true,
            ..RasterInfo::default()
        },
        bands: vec![values],
        no_data: vec![None],
        ..RasterGrid::default()
    }
}

fn at(raster: &RasterGrid, column: i32, row: i32) -> f64 {
    raster.bands[0][row as usize * raster.info.width as usize + column as usize]
}

fn table(name: &str, kind: GeometryKind) -> FeatureTable {
    FeatureTable {
        name: String::from(name),
        kind,
        ..FeatureTable::default()
    }
}

fn feature(kind: GeometryKind, points: Vec<GeoPoint>, has_z: bool) -> Feature {
    Feature {
        parts: vec![VectorGeometry {
            kind,
            parts: vec![points],
            has_z,
            ..VectorGeometry::default()
        }],
        values: Vec::new(),
    }
}

fn point(x: f64, y: f64, z: f64) -> GeoPoint {
    GeoPoint { x, y, z }
}

/// Shoelace area of a polygon's exterior less its holes.
fn area(polygon: &VectorGeometry) -> f64 {
    let mut total = 0.0;
    for (r, ring) in polygon.parts.iter().enumerate() {
        let mut twice = 0.0;
        for (i, a) in ring.iter().enumerate() {
            let b = &ring[(i + 1) % ring.len()];
            twice += a.x * b.y - b.x * a.y;
        }
        total += (if r == 0 { 1.0 } else { -1.0 }) * f64::abs(twice) / 2.0;
    }
    total
}

fn request(path: &[&str]) -> RunRequest {
    RunRequest {
        path: crate::strings(path),
        ..RunRequest::default()
    }
}

fn arg_named<'a>(spec: &'a AlgorithmSpec, name: &str) -> Option<&'a ArgSpec> {
    spec.args.iter().find(|arg| arg.name == name)
}

/// A dataset GDAL opens by name.
fn by_name(path: String) -> ArgValue {
    ArgValue::Dataset(DatasetValue::Path(DatasetPath {
        path,
        ..DatasetPath::default()
    }))
}

/// GDAL 3.13.2 as MSYS2 builds it has 121 leaf algorithms. One of them,
/// `gdal driver pdf list-layers`, is registered by the PDF driver and exists
/// only in a GDAL built with it: conda-forge's 3.13.2 has no PDF driver, and
/// its own `gdal driver --help` lists no pdf subcommand, so it has 120.
fn expected_leaves() -> usize {
    if katana_gis::formats::find_format("PDF").is_some() {
        121
    } else {
        120
    }
}

fn rolling_ground(size: i32) -> RasterGrid {
    grid(size, size, 1.0, |x, y| {
        20.0 + 10.0 * (x * 0.02).sin() * (y * 0.017).cos()
    })
}

// ---- the catalogue --------------------------------------------------------------------------

mod gdal_catalogue {
    use katana_core::ErrorCode;
    use katana_gis::processing::{catalogue, resolve};

    use super::expected_leaves;
    use crate::{must, refused, strings};

    #[test]
    fn holds_every_leaf_from_the_root_including_the_driver_family() {
        // 121 leaves in GDAL 3.13.2, walked from the root "gdal": the
        // registry's own top-level list stops at convert, dataset, info,
        // mdim, pipeline, raster, vector and vsi, and the driver family is
        // missed.
        let mut leaves = 0;
        let mut driver_gpkg_validate = false;
        let mut raster_group = false;
        for info in catalogue() {
            leaves += usize::from(!info.container);
            driver_gpkg_validate =
                driver_gpkg_validate || info.path == ["driver", "gpkg", "validate"];
            raster_group = raster_group || (info.container && info.path == ["raster"]);
        }
        assert!(leaves >= expected_leaves(), "{leaves} leaves");
        assert!(driver_gpkg_validate);
        assert!(raster_group);
    }

    #[test]
    fn an_alias_resolves_to_its_algorithm() {
        let mut consumed = 0;
        // "warp" is a hidden alias, "neighbours" a public one.
        let warp = must(resolve(&strings(&["raster", "warp"]), &mut consumed));
        assert_eq!(warp, ["raster", "reproject"]);
        let neighbours = must(resolve(&strings(&["raster", "neighbours"]), &mut consumed));
        assert_eq!(neighbours, ["raster", "neighbors"]);
        let shouted = must(resolve(&strings(&["RASTER", "HillShade"]), &mut consumed));
        assert_eq!(shouted, ["raster", "hillshade"]);
    }

    #[test]
    fn resolve_takes_the_longest_path_and_leaves_the_rest() {
        let mut consumed = 0;
        let path = must(resolve(
            &strings(&["vector", "grid", "linear", "--resolution", "1,1"]),
            &mut consumed,
        ));
        assert_eq!(path, ["vector", "grid", "linear"]);
        assert_eq!(consumed, 3);
    }

    #[test]
    fn a_container_is_refused_listing_its_children() {
        let mut consumed = 0;
        let group = refused(
            resolve(&strings(&["vector", "grid"]), &mut consumed),
            "a group",
        );
        assert_eq!(group.code, ErrorCode::InvalidArgument);
        assert!(group.message.contains("linear"), "{}", group.message);
        assert!(group.message.contains("invdist"));
        assert_eq!(consumed, 2);
    }

    #[test]
    fn an_algorithm_of_no_such_name_is_not_found() {
        let mut consumed = 0;
        let nothing = refused(
            resolve(&strings(&["hillshade"]), &mut consumed),
            "a leaf without its group",
        );
        assert_eq!(nothing.code, ErrorCode::NotFound);
        assert_eq!(consumed, 0);
    }
}

// ---- describing arguments ---------------------------------------------------------------------

mod gdal_describe {
    use katana_core::ErrorCode;
    use katana_gis::processing::{ArgType, DatasetKind, Scalar, describe};

    use super::arg_named;
    use crate::{must, refused, strings};

    #[test]
    fn reports_bounds_with_their_inclusivity() {
        // GDAL 3.13: hillshade --altitude [0, 90]; contour --interval (0, ...).
        let hillshade = must(describe(&strings(&["raster", "hillshade"])));
        let altitude = arg_named(&hillshade, "altitude").expect("hillshade has --altitude");
        let (min, max) = (
            altitude.min.expect("--altitude has a minimum"),
            altitude.max.expect("--altitude has a maximum"),
        );
        assert_eq!(min.value, 0.0);
        assert!(min.inclusive);
        assert_eq!(max.value, 90.0);
        assert!(max.inclusive);

        let contour = must(describe(&strings(&["raster", "contour"])));
        let interval = arg_named(&contour, "interval").expect("contour has --interval");
        let min = interval.min.expect("--interval has a minimum");
        assert_eq!(min.value, 0.0);
        assert!(!min.inclusive);
    }

    #[test]
    fn a_missing_bound_is_absent_not_zero() {
        let hillshade = must(describe(&strings(&["raster", "hillshade"])));
        let azimuth = arg_named(&hillshade, "azimuth").expect("hillshade has --azimuth");
        assert!(azimuth.min.is_none());
        assert!(azimuth.max.is_none());
        assert_eq!(azimuth.default_value, Some(Scalar::Real(315.0)));
    }

    #[test]
    fn arguments_hidden_from_the_api_are_not_listed() {
        let hillshade = must(describe(&strings(&["raster", "hillshade"])));
        for hidden in ["help", "help-doc", "json-usage", "config", "progress"] {
            assert!(arg_named(&hillshade, hidden).is_none(), "{hidden}");
        }
        let input = arg_named(&hillshade, "input").expect("hillshade has an input");
        assert_eq!(input.r#type, ArgType::DatasetList);
        assert_eq!(input.dataset_kinds, DatasetKind::Raster as u32);
        assert!(input.accepts_name && input.accepts_object);
        assert!(!hillshade.usage_json.is_empty());
        assert!(hillshade.usage_json.contains("input_arguments"));
    }

    #[test]
    fn an_algorithm_of_no_such_path_is_not_found() {
        let nothing = refused(describe(&strings(&["raster", "no-such"])), "no-such");
        assert_eq!(nothing.code, ErrorCode::NotFound);
    }
}

// ---- runs, against values worked by hand ---------------------------------------------------------

mod gdal_run {
    use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

    use katana_core::ErrorCode;
    use katana_core::text::format_exact_real;
    use katana_gis::GdalDataset;
    use katana_gis::processing::{
        ArgValue, DatasetPath, DatasetValue, FeatureSet, Scalar, describe, run,
        staged_dataset_count,
    };

    use super::{
        area, arg_named, at, by_name, feature, grid, point, request, rolling_ground, table,
    };
    use crate::{Scratch, double_eq, geo_data, must, near, refused, shared_data, strings};
    use katana_gis::GeometryKind;

    #[test]
    fn hillshade_of_flat_ground_at_altitude45_is181() {
        // Flat ground: slope 0, so GDAL's shade is 1 + 254 * sin(altitude)
        // = 1 + 254 * sin 45 deg = 180.605, rounded to the nearest byte: 181.
        let mut shade = request(&["raster", "hillshade"]);
        shade.values.push((
            String::from("input"),
            ArgValue::Dataset(DatasetValue::Raster(grid(10, 10, 1.0, |_, _| 50.0))),
        ));
        let outputs = must(run(&shade, None, None));
        let raster = outputs.raster.expect("a raster in memory");
        assert_eq!(raster.data_type, "Byte");
        assert_eq!(at(&raster, 5, 5), 181.0);
        assert!(outputs.seconds >= 0.0);
    }

    #[test]
    fn slope_of_a_plane_rising_one_in_twenty_is_five_percent() {
        // plane.asc: z = 100 + 0.05 x, so the slope is 0.05 = 5 %. GDAL reads
        // the grid as Float32 (ulp 7.6e-6 near 100) and Horn's gradient sums
        // six values over 8 cell widths: its dz/dx is within 8 * 3.8e-6 / 8 =
        // 3.8e-6 of 0.05, and the percent within 100 * 3.8e-6 = 3.8e-4.
        // Hence 5e-4.
        let _data = shared_data();
        let mut slope = request(&["raster", "slope"]);
        slope
            .values
            .push((String::from("input"), by_name(geo_data("plane.asc"))));
        slope.tokens = strings(&["--unit=percent"]);
        let outputs = must(run(&slope, None, None));
        let raster = outputs.raster.expect("a raster in memory");
        near(at(&raster, 20, 15), 5.0, 5e-4);
    }

    #[test]
    fn aspect_of_a_plane_falling_west_is270_degrees() {
        // The plane rises to the east and every row is the same, so dz/dy is
        // exactly 0 and the ground faces due west: azimuth 270. Float32
        // carries 270 to 3e-5 (half an ulp 1.5e-5); 1e-4 allows the degree
        // conversion.
        let _data = shared_data();
        let mut aspect = request(&["raster", "aspect"]);
        aspect
            .values
            .push((String::from("input"), by_name(geo_data("plane.asc"))));
        let outputs = must(run(&aspect, None, None));
        let raster = outputs.raster.expect("a raster in memory");
        near(at(&raster, 20, 15), 270.0, 1e-4);
    }

    #[test]
    fn buffer_of_a_hundred_metre_line_one_metre_with_flat_caps_is200_square_metres() {
        // Flat caps end the buffer at the line's ends: a 100 x 2 rectangle.
        let mut lines = table("lines", GeometryKind::LineString);
        lines.features.push(feature(
            GeometryKind::LineString,
            vec![point(0.0, 0.0, 0.0), point(100.0, 0.0, 0.0)],
            false,
        ));
        let mut buffer = request(&["vector", "buffer"]);
        buffer.values.push((
            String::from("input"),
            ArgValue::Dataset(DatasetValue::Features(FeatureSet {
                tables: vec![lines],
            })),
        ));
        buffer.tokens = strings(&["--distance=1", "--endcap-style=flat"]);
        let outputs = must(run(&buffer, None, None));
        let features = outputs.features.expect("features in memory");
        assert_eq!(features.tables.len(), 1);
        assert_eq!(features.tables[0].features.len(), 1);
        let buffered = &features.tables[0].features[0];
        assert_eq!(buffered.parts.len(), 1);
        assert_eq!(buffered.parts[0].kind, GeometryKind::Polygon);
        near(area(&buffered.parts[0]), 200.0, 1e-9);
    }

    #[test]
    fn intersection_of_a_four_metre_corridor_with_a_fifty_metre_lot_is200_square_metres() {
        // lots.geojson: lots A (0,0)-(50,40) and B (50,0)-(100,40), and a
        // corridor 4 m wide across both, y 18 to 22: 50 x 4 = 200 m2 in each.
        let _data = shared_data();
        let filtered = |kind: &str| {
            let mut filter = request(&["vector", "filter"]);
            filter
                .values
                .push((String::from("input"), by_name(geo_data("lots.geojson"))));
            filter.tokens = vec![String::from("--where"), format!("kind = '{kind}'")];
            run(&filter, None, None)
        };
        let lots = must(filtered("lot"));
        let corridor = must(filtered("corridor"));
        let mut algebra = request(&["vector", "layer-algebra"]);
        algebra.values.push((
            String::from("input"),
            ArgValue::Dataset(DatasetValue::Features(
                lots.features.expect("the lots as features"),
            )),
        ));
        algebra.values.push((
            String::from("method"),
            ArgValue::Dataset(DatasetValue::Features(
                corridor.features.expect("the corridor as features"),
            )),
        ));
        algebra.tokens = strings(&["intersection"]);
        let outputs = must(run(&algebra, None, None));
        let features = outputs.features.expect("features in memory");
        assert_eq!(features.tables.len(), 1);
        let pieces = &features.tables[0].features;
        assert_eq!(pieces.len(), 2);
        for piece in pieces {
            let total: f64 = piece.parts.iter().map(area).sum();
            near(total, 200.0, 1e-9);
        }
    }

    #[test]
    fn a_linear_grid_of_points_on_a_plane_reproduces_the_plane() {
        // Points every 10 m on z = 100 + x/10 + y/20. Linear interpolation
        // over a triangulation reproduces a plane, so the cell centred on
        // (52.5, 47.5) holds 100 + 5.25 + 2.375 = 107.625, in Float64 to
        // rounding (1e-9).
        let mut points = table("points", GeometryKind::Point);
        points.has_z = true;
        for i in 0..=10 {
            for j in 0..=10 {
                let x = 10.0 * f64::from(i);
                let y = 10.0 * f64::from(j);
                points.features.push(feature(
                    GeometryKind::Point,
                    vec![point(x, y, 100.0 + x / 10.0 + y / 20.0)],
                    true,
                ));
            }
        }
        let mut linear = request(&["vector", "grid", "linear"]);
        linear.values.push((
            String::from("input"),
            ArgValue::Dataset(DatasetValue::Features(FeatureSet {
                tables: vec![points],
            })),
        ));
        linear.tokens = strings(&["--extent=0,0,100,100", "--resolution=5,5"]);
        let outputs = must(run(&linear, None, None));
        let raster = outputs.raster.expect("a raster in memory");
        let g = raster.info.geotransform;
        let column = ((52.5 - g[0]) / g[1]).floor() as i32;
        let row = ((47.5 - g[3]) / g[5]).floor() as i32;
        // The cell's centre is the point asked about, so the hand value
        // applies.
        double_eq(g[0] + (f64::from(column) + 0.5) * g[1], 52.5);
        double_eq(g[3] + (f64::from(row) + 0.5) * g[5], 47.5);
        near(at(&raster, column, row), 107.625, 1e-9);
    }

    #[test]
    fn an_info_algorithm_prints_its_text() {
        let _data = shared_data();
        let mut info = request(&["raster", "info"]);
        info.values
            .push((String::from("input"), by_name(geo_data("plane.asc"))));
        info.tokens = strings(&["--format=text"]);
        let outputs = must(run(&info, None, None));
        let text = outputs.text.expect("the text it printed");
        assert!(text.contains("Size is 40, 30"), "{text}");
        assert!(outputs.raster.is_none());
    }

    // ---- cancel ---------------------------------------------------------------------------

    #[test]
    fn a_cancelled_run_is_cancelled_even_when_gdal_reports_success() {
        // raster hillshade returns true after its progress was refused, with
        // a partial output (measured): the run must still say "cancelled"
        // and hand back nothing.
        let stop = AtomicBool::new(false);
        let calls = AtomicI32::new(0);
        let progress: &(dyn Fn(f64) + Sync) = &|_| {
            if calls.fetch_add(1, Ordering::SeqCst) + 1 == 2 {
                stop.store(true, Ordering::SeqCst);
            }
        };
        let mut shade = request(&["raster", "hillshade"]);
        shade.values.push((
            String::from("input"),
            ArgValue::Dataset(DatasetValue::Raster(rolling_ground(600))),
        ));
        let error = refused(run(&shade, Some(&stop), Some(progress)), "a stopped run");
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert_eq!(error.message, "cancelled");
        assert!(calls.load(Ordering::SeqCst) >= 2);
    }

    #[test]
    fn a_stop_before_the_run_starts_runs_nothing() {
        let stop = AtomicBool::new(true);
        let calls = AtomicI32::new(0);
        let progress: &(dyn Fn(f64) + Sync) = &|_| {
            calls.fetch_add(1, Ordering::SeqCst);
        };
        let mut shade = request(&["raster", "hillshade"]);
        shade.values.push((
            String::from("input"),
            ArgValue::Dataset(DatasetValue::Raster(rolling_ground(50))),
        ));
        let error = refused(run(&shade, Some(&stop), Some(progress)), "a stopped run");
        assert_eq!(error.message, "cancelled");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_viewshed_cancelled_on_gdals_own_threads_still_says_cancelled() {
        // The viewshed works on threads GDAL starts, whose errors skip a
        // run's own collector and reach the process-wide quiet handler. What
        // the run says must not depend on them.
        let stop = AtomicBool::new(false);
        let calls = AtomicI32::new(0);
        let progress: &(dyn Fn(f64) + Sync) = &|_| {
            if calls.fetch_add(1, Ordering::SeqCst) + 1 == 2 {
                stop.store(true, Ordering::SeqCst);
            }
        };
        let mut viewshed = request(&["raster", "viewshed"]);
        viewshed.values.push((
            String::from("input"),
            ArgValue::Dataset(DatasetValue::Raster(rolling_ground(600))),
        ));
        viewshed.tokens = strings(&["--position=300,300", "--height=1.7"]);
        let error = refused(
            run(&viewshed, Some(&stop), Some(progress)),
            "a stopped viewshed",
        );
        assert_eq!(error.message, "cancelled");
    }

    // ---- refusals ------------------------------------------------------------------------

    #[test]
    fn an_unknown_argument_is_refused_by_name() {
        let mut shade = request(&["raster", "hillshade"]);
        shade.values.push((
            String::from("input"),
            ArgValue::Dataset(DatasetValue::Raster(rolling_ground(10))),
        ));
        shade.values.push((
            String::from("no-such-arg"),
            ArgValue::Scalar(Scalar::Real(1.0)),
        ));
        let error = refused(run(&shade, None, None), "an unknown argument");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(error.message.contains("no-such-arg"), "{}", error.message);
    }

    #[test]
    fn an_unknown_option_in_gdals_words_is_refused_by_name() {
        let mut shade = request(&["raster", "hillshade"]);
        shade.values.push((
            String::from("input"),
            ArgValue::Dataset(DatasetValue::Raster(rolling_ground(10))),
        ));
        shade.tokens = strings(&["--nosuch=2"]);
        let error = refused(run(&shade, None, None), "an unknown option");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(error.message.contains("nosuch"), "{}", error.message);
    }

    #[test]
    fn a_value_outside_the_choices_is_refused_with_gdals_message() {
        let mut shade = request(&["raster", "hillshade"]);
        shade.values.push((
            String::from("input"),
            ArgValue::Dataset(DatasetValue::Raster(rolling_ground(10))),
        ));
        shade.values.push((
            String::from("variant"),
            ArgValue::Scalar(Scalar::Text(String::from("bogus"))),
        ));
        let error = refused(run(&shade, None, None), "a bogus variant");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(
            error.message.contains("Should be one among"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_value_beyond_its_bound_is_refused() {
        let mut shade = request(&["raster", "hillshade"]);
        shade.values.push((
            String::from("input"),
            ArgValue::Dataset(DatasetValue::Raster(rolling_ground(10))),
        ));
        shade.values.push((
            String::from("altitude"),
            ArgValue::Scalar(Scalar::Real(100.0)),
        ));
        let error = refused(run(&shade, None, None), "an altitude of 100");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(error.message.contains("90"), "{}", error.message);
    }

    #[test]
    fn the_config_token_is_refused() {
        let mut shade = request(&["raster", "hillshade"]);
        shade.values.push((
            String::from("input"),
            ArgValue::Dataset(DatasetValue::Raster(rolling_ground(10))),
        ));
        shade.tokens = strings(&["--config", "GDAL_ENABLE_EXTERNAL=YES"]);
        let error = refused(run(&shade, None, None), "--config");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(error.context, "--config");
        shade.tokens = strings(&["--config=GDAL_ENABLE_EXTERNAL=YES"]);
        assert!(run(&shade, None, None).is_err());
    }

    #[test]
    fn only_allow_listed_configuration_is_set_for_a_run() {
        let mut shade = request(&["raster", "hillshade"]);
        shade.values.push((
            String::from("input"),
            ArgValue::Dataset(DatasetValue::Raster(rolling_ground(10))),
        ));
        shade.config = vec![(String::from("SPATIALITE_SECURITY"), String::from("relaxed"))];
        let error = refused(run(&shade, None, None), "SPATIALITE_SECURITY");
        assert_eq!(error.context, "SPATIALITE_SECURITY");
        shade.config = vec![(String::from("GDAL_NUM_THREADS"), String::from("2"))];
        assert!(run(&shade, None, None).is_ok());
    }

    #[test]
    fn a_pipeline_with_an_external_step_is_refused() {
        let mut pipeline = request(&["pipeline"]);
        pipeline.values.push((
            String::from("input"),
            ArgValue::Dataset(DatasetValue::Raster(rolling_ground(10))),
        ));
        pipeline.tokens = strings(&["read ! external --command \"calc.exe\" ! write"]);
        let by_words = refused(run(&pipeline, None, None), "an external step");
        assert_eq!(by_words.code, ErrorCode::InvalidArgument);
        pipeline.tokens.clear();
        pipeline.values.push((
            String::from("pipeline"),
            ArgValue::Scalar(Scalar::Text(String::from("read ! [ external ] ! write"))),
        ));
        assert!(run(&pipeline, None, None).is_err());
    }

    #[test]
    fn a_group_is_refused_listing_what_is_in_it() {
        let error = refused(run(&request(&["vector", "grid"]), None, None), "a group");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(error.message.contains("linear"));
    }

    // ---- staging, spilling, sidecars ----------------------------------------------------------

    #[test]
    fn a_name_only_input_is_staged_and_removed() {
        // raster calc takes its inputs by name only, so a grid in memory is
        // written under /vsimem/katana for it and removed when the run ends.
        // The builtin dialect's sum of one input is that input.
        let calc = must(describe(&strings(&["raster", "calc"])));
        let input = arg_named(&calc, "input").expect("calc has an input");
        assert!(
            input.accepts_name && !input.accepts_object,
            "raster calc now takes objects"
        );

        let mut sum = request(&["raster", "calc"]);
        sum.values.push((
            String::from("input"),
            ArgValue::Datasets(vec![DatasetValue::Raster(grid(8, 6, 1.0, |_, _| 7.5))]),
        ));
        sum.tokens = strings(&["--dialect=builtin", "--calc=sum"]);
        let outputs = must(run(&sum, None, None));
        let raster = outputs.raster.expect("a raster in memory");
        assert_eq!(at(&raster, 3, 2), 7.5);
        assert_eq!(staged_dataset_count(), 0);
    }

    #[test]
    fn a_raster_larger_than_the_memory_limit_spills_to_a_tiled_geo_tiff() {
        let folder = Scratch::new("spill");
        let mut shade = request(&["raster", "hillshade"]);
        shade.values.push((
            String::from("input"),
            ArgValue::Dataset(DatasetValue::Raster(rolling_ground(20))),
        ));
        shade.max_memory_cells = 399; // 20 x 20 x 1 = 400 cells
        shade.spill_directory = folder.path.to_string_lossy().into_owned();
        let outputs = must(run(&shade, None, None));
        assert!(outputs.raster.is_none());
        let file = outputs.file.expect("the spilled file");
        let written = must(GdalDataset::open(std::path::Path::new(&file)));
        assert_eq!(written.driver_name(), "GTiff");
        assert_eq!(must(written.raster_info()).width, 20);
    }

    #[test]
    fn a_run_on_a_user_file_leaves_no_sidecar_beside_it() {
        // raster info --stats makes GDAL write plane.asc.aux.xml when the
        // file closes; a person's folder must not gain one from a run that
        // only read.
        let folder = Scratch::new("sidecar");
        must(
            std::fs::copy(geo_data("plane.asc"), folder.path.join("plane.asc"))
                .map_err(|error| katana_core::Error::new(ErrorCode::Internal, format!("{error}"))),
        );
        let mut info = request(&["raster", "info"]);
        info.values
            .push((String::from("input"), by_name(folder.file("plane.asc"))));
        info.tokens = strings(&["--stats"]);
        let outputs = must(run(&info, None, None));
        let text = outputs.text.expect("the text it printed");
        assert!(text.contains("STATISTICS_MEAN"), "{text}");
        assert!(!folder.path.join("plane.asc.aux.xml").exists());
        // The same through GDAL's own words, the file named in them.
        let mut words = request(&["raster", "info"]);
        words.tokens = vec![String::from("--stats"), folder.file("plane.asc")];
        assert!(run(&words, None, None).is_ok());
        assert!(!folder.path.join("plane.asc.aux.xml").exists());
    }

    #[test]
    fn a_pipelines_read_step_leaves_no_sidecar_beside_its_file() {
        // A file a pipeline's read step names is bound to no dataset
        // argument, so it was not watched: info --stats left plane.asc.aux.xml
        // beside it.
        let folder = Scratch::new("sidecar-pipeline");
        must(
            std::fs::copy(geo_data("plane.asc"), folder.path.join("plane.asc"))
                .map_err(|error| katana_core::Error::new(ErrorCode::Internal, format!("{error}"))),
        );
        let file = folder.file("plane.asc");
        let sidecar = folder.path.join("plane.asc.aux.xml");
        // One quoted text, and words one by one.
        for tokens in [
            vec![format!("read {file} ! info --stats")],
            strings(&["read", file.as_str(), "!", "info", "--stats"]),
        ] {
            let mut pipeline = request(&["raster", "pipeline"]);
            pipeline.tokens = tokens.clone();
            let outputs = must(run(&pipeline, None, None));
            let text = outputs.text.expect("the text it printed");
            assert!(text.contains("STATISTICS_MEAN"), "{text}");
            assert!(!sidecar.exists(), "{}", tokens[0]);
        }
    }

    #[test]
    fn a_file_sources_layer_is_the_layer_of_its_own_dataset() {
        // like.gpkg holds two areas: decoy, far away, and m, the square
        // 5..15. Clipping (0,0) and (10,10) by like's layer m keeps (10,10)
        // alone. The layer was set as --input-layer, the points' own, which
        // have no m.
        let _data = shared_data();
        let folder = Scratch::new("layer-of-like");
        let square = |x0: f64, x1: f64| {
            let (a, b) = (format_exact_real(x0), format_exact_real(x1));
            format!(
                r#"{{"type":"FeatureCollection","features":[{{"type":"Feature","properties":{{}},"geometry":{{"type":"Polygon","coordinates":[[[{a},{a}],[{b},{a}],[{b},{b}],[{a},{b}],[{a},{a}]]]}}}}]}}"#
            )
        };
        let write = |name: &str, text: String| {
            must(
                std::fs::write(folder.path.join(name), text).map_err(|error| {
                    katana_core::Error::new(ErrorCode::Internal, format!("{error}"))
                }),
            );
        };
        write("decoy.geojson", square(100.0, 101.0));
        write("m.geojson", square(5.0, 15.0));
        write(
            "points.geojson",
            String::from(concat!(
                r#"{"type":"FeatureCollection","features":["#,
                r#"{"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[0,0]}},"#,
                r#"{"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[10,10]}}]}"#
            )),
        );
        let like = folder.file("like.gpkg");
        for layer in ["decoy", "m"] {
            let mut made = request(&["vector", "convert"]);
            made.tokens = vec![
                folder.file(&format!("{layer}.geojson")),
                like.clone(),
                format!("--output-layer={layer}"),
            ];
            if layer == "m" {
                made.tokens.push(String::from("--update"));
            }
            assert!(run(&made, None, None).is_ok(), "{layer}");
        }
        let mut clip = request(&["vector", "clip"]);
        clip.values.push((
            String::from("input"),
            by_name(folder.file("points.geojson")),
        ));
        clip.values.push((
            String::from("like"),
            ArgValue::Dataset(DatasetValue::Path(DatasetPath {
                path: like.clone(),
                open_options: Vec::new(),
                layer: String::from("m"),
            })),
        ));
        let outputs = must(run(&clip, None, None));
        let features = outputs.features.expect("features in memory");
        let kept: usize = features
            .tables
            .iter()
            .map(|table| table.features.len())
            .sum();
        assert_eq!(kept, 1);
        // A dataset argument with no layer argument of its own refuses a
        // layer rather than giving it to another dataset: raster reproject's
        // like has none, and its input is a raster.
        let mut reproject = request(&["raster", "reproject"]);
        reproject
            .values
            .push((String::from("input"), by_name(geo_data("plane.asc"))));
        reproject.values.push((
            String::from("like"),
            ArgValue::Dataset(DatasetValue::Path(DatasetPath {
                path: like,
                open_options: Vec::new(),
                layer: String::from("m"),
            })),
        ));
        let no = refused(run(&reproject, None, None), "a layer for like");
        assert_eq!(no.code, ErrorCode::InvalidArgument);
        assert_eq!(no.context, "like");
    }

    #[test]
    fn a_sidecar_already_beside_a_file_is_read_and_left_as_it_was() {
        // Turning GDAL's sidecars off for a run would stop this one being
        // READ (measured): its metadata must reach the run, and the file stay
        // as it was.
        let folder = Scratch::new("sidecar-kept");
        must(
            std::fs::copy(geo_data("plane.asc"), folder.path.join("plane.asc"))
                .map_err(|error| katana_core::Error::new(ErrorCode::Internal, format!("{error}"))),
        );
        let sidecar =
            "<PAMDataset><Metadata><MDI key=\"sidecar_mark\">kept</MDI></Metadata></PAMDataset>\n";
        must(
            std::fs::write(folder.path.join("plane.asc.aux.xml"), sidecar)
                .map_err(|error| katana_core::Error::new(ErrorCode::Internal, format!("{error}"))),
        );
        let mut info = request(&["raster", "info"]);
        info.values
            .push((String::from("input"), by_name(folder.file("plane.asc"))));
        info.tokens = strings(&["--stats"]);
        let outputs = must(run(&info, None, None));
        let text = outputs.text.expect("the text it printed");
        assert!(text.contains("sidecar_mark"));
        let now = std::fs::read(folder.path.join("plane.asc.aux.xml")).unwrap_or_default();
        assert_eq!(now, sidecar.as_bytes());
    }

    // ---- concurrency --------------------------------------------------------------------------

    #[test]
    fn concurrent_runs_on_their_own_datasets_match_the_single_threaded_result() {
        // One shared input gave 63 wrong results in 120 (measured); each run
        // makes its own, so 4 threads x 10 runs must all equal one run alone.
        let ground = rolling_ground(64);
        let shade = || {
            let mut shade = request(&["raster", "hillshade"]);
            shade.values.push((
                String::from("input"),
                ArgValue::Dataset(DatasetValue::Raster(ground.clone())),
            ));
            shade.tokens = strings(&["--zfactor=2"]);
            run(&shade, None, None)
        };
        let reference = must(shade()).raster.expect("a raster in memory");
        let identical = AtomicI32::new(0);
        let failed = AtomicI32::new(0);
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    for _ in 0..10 {
                        match shade() {
                            Ok(again) => match again.raster {
                                Some(raster) if raster.bands == reference.bands => {
                                    identical.fetch_add(1, Ordering::SeqCst);
                                }
                                Some(_) => {}
                                None => {
                                    failed.fetch_add(1, Ordering::SeqCst);
                                }
                            },
                            Err(_) => {
                                failed.fetch_add(1, Ordering::SeqCst);
                            }
                        }
                    }
                });
            }
        });
        assert_eq!(failed.load(Ordering::SeqCst), 0);
        assert_eq!(identical.load(Ordering::SeqCst), 40);
    }
}

// ---- the tail's words -----------------------------------------------------------------------------

mod gdal_arguments {
    use katana_gis::processing::arguments_given;

    use crate::{must, strings};

    #[test]
    fn what_a_tail_gives_is_read_from_its_positionals_and_options() {
        let hillshade = strings(&["raster", "hillshade"]);
        let both = must(arguments_given(
            &hillshade,
            &strings(&["dem.tif", "shade.tif", "--zfactor", "2"]),
            &[],
        ));
        assert!(both.iter().any(|name| name == "input"));
        assert!(both.iter().any(|name| name == "output"));
        assert!(both.iter().any(|name| name == "zfactor"));
        // With the input bound, GDAL gives the one positional to the output.
        let output_only = must(arguments_given(
            &hillshade,
            &strings(&["shade.tif"]),
            &strings(&["input"]),
        ));
        assert_eq!(output_only, ["output"]);
        // An option's value is not a positional.
        let options = must(arguments_given(
            &hillshade,
            &strings(&["-z", "2"]),
            &strings(&["input"]),
        ));
        assert_eq!(options, ["zfactor"]);
    }

    #[test]
    fn a_pipeline_names_its_input_and_output_in_its_read_and_write_steps() {
        let named = must(arguments_given(
            &strings(&["raster", "pipeline"]),
            &strings(&["read dem.tif ! slope ! write out.tif"]),
            &[],
        ));
        assert!(named.iter().any(|name| name == "input"));
        assert!(named.iter().any(|name| name == "output"));
        let bare = must(arguments_given(
            &strings(&["raster", "pipeline"]),
            &strings(&["read ! slope ! write"]),
            &strings(&["input"]),
        ));
        assert!(!bare.iter().any(|name| name == "output"));
    }
}

mod gdal_versions {
    use katana_gis::processing::versions;

    use super::expected_leaves;

    #[test]
    fn name_the_library_and_count_its_algorithms() {
        let versions = versions();
        assert!(versions.gdal.starts_with("3."), "{}", versions.gdal);
        assert!(!versions.proj.is_empty());
        assert!(versions.raster_drivers > 0);
        assert!(versions.vector_drivers > 0);
        assert!(versions.algorithms as usize >= expected_leaves());
    }
}

mod gdal_words {
    use katana_gis::processing::{DatasetKind, Scalar, dataset_kinds_text, to_string};

    #[test]
    fn a_value_is_written_as_gdals_command_line_writes_it() {
        assert_eq!(to_string(&Scalar::Boolean(true)), "true");
        assert_eq!(to_string(&Scalar::Real(0.1)), "0.1");
        assert_eq!(to_string(&Scalar::IntegerList(vec![100, 50])), "100,50");
        assert_eq!(
            to_string(&Scalar::TextList(vec![
                String::from("a"),
                String::from("b")
            ])),
            "a,b"
        );
        assert_eq!(
            dataset_kinds_text(DatasetKind::Raster as u32 | DatasetKind::Vector as u32),
            "raster,vector"
        );
    }
}
