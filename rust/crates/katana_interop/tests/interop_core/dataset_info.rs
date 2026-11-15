//! tests/interop/test_dataset_info.cpp: what a file holds, read without
//! importing it.
//!
//! describe_source is checked against files written here with known contents
//! (so every expected fact is one the test put there), and
//! format_description, which is pure, against descriptions built by hand.
//! The text tests assert on the facts appearing, never on a snapshot of the
//! whole text: a rewording is not a defect, a missing or wrong fact is.

use std::path::{Path, PathBuf};

use katana_core::ErrorCode;
use katana_geometry::{Box2, Point2};
use katana_gis::gdal_adapter::{
    GdalDataset, GeoPoint, GeometryKind, RasterExportOptions, VectorExportOptions, VectorFeature,
    VectorGeometry,
};
use katana_gis::zip_container::write_zip;
use katana_interop::dataset_info::{
    DescribeOptions, PointCloudDescription, RasterDescription, SourceDescription,
    VectorLayerDescription, describe_source, format_description, is_virtual_path,
};
use katana_interop::import::{PointCloudImportOptions, SourceKind, import_point_cloud};
use katana_pointcloud::{PointCloud, PointCloudEngine, PointCloudPoint};

use crate::{TempDir, must, refused, write_file};

fn temp_dir(name: &str) -> TempDir {
    TempDir::new("katana-datasetinfo-", name)
}

/// EPSG:28356, GDA94 / MGA zone 56, as the EPSG registry publishes it in
/// WKT1 (epsg.io/28356.wkt). The same text as the adapter tests use.
const MGA56_WKT: &str = concat!(
    r#"PROJCS["GDA94 / MGA zone 56",GEOGCS["GDA94",DATUM["Geocentric_Datum_of_Australia_1994","#,
    r#"SPHEROID["GRS 1980",6378137,298.257222101,AUTHORITY["EPSG","7019"]],"#,
    r#"TOWGS84[0,0,0,0,0,0,0],AUTHORITY["EPSG","6283"]],PRIMEM["Greenwich",0,"#,
    r#"AUTHORITY["EPSG","8901"]],UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],"#,
    r#"AUTHORITY["EPSG","4283"]],PROJECTION["Transverse_Mercator"],"#,
    r#"PARAMETER["latitude_of_origin",0],PARAMETER["central_meridian",153],"#,
    r#"PARAMETER["scale_factor",0.9996],PARAMETER["false_easting",500000],"#,
    r#"PARAMETER["false_northing",10000000],UNIT["metre",1,AUTHORITY["EPSG","9001"]],"#,
    r#"AXIS["Easting",EAST],AXIS["Northing",NORTH],AUTHORITY["EPSG","28356"]]"#
);
const MGA56_NAME: &str = "GDA94 / MGA zone 56 (EPSG:28356)";

fn describe(path: &Path) -> katana_core::Result<SourceDescription> {
    describe_source(path, &DescribeOptions::default())
}

fn write_raster(path: &Path, options: &RasterExportOptions) {
    let values = vec![1.0; options.width as usize * options.height as usize];
    must(GdalDataset::write_raster(path, options, &values));
}

#[track_caller]
fn assert_holds(text: &str, fact: &str) {
    assert!(text.contains(fact), "missing '{fact}' in:\n{text}");
}

fn point_feature(x: f64, y: f64) -> VectorFeature {
    VectorFeature {
        geometry: VectorGeometry {
            kind: GeometryKind::Point,
            parts: vec![vec![GeoPoint { x, y, z: 0.0 }]],
            ..VectorGeometry::default()
        },
        ..VectorFeature::default()
    }
}

/// 1 234 coloured points on a grid 100 wide: x 250 000 to 250 099, y
/// 7 400 000 to 7 400 012 (rows 0 to 12, the last one partial), z 10 to
/// 10.75. Every coordinate is a multiple of 0.25, exact at the writer's 0.001
/// scale.
fn coloured_cloud() -> PointCloud {
    let points = (0..1234)
        .map(|i: i32| PointCloudPoint {
            x: 250000.0 + f64::from(i % 100),
            y: 7400000.0 + f64::from(i / 100),
            z: 10.0 + 0.25 * f64::from(i % 4),
            classification: 2,
            red: 200,
            green: 100,
            blue: 50,
            has_color: true,
            ..PointCloudPoint::default()
        })
        .collect();
    PointCloud {
        points,
        ..PointCloud::default()
    }
}

/// A 100 m square of ground at 0.25 m spacing, dense enough that a COPC
/// octree has levels to choose between (see the adapter tests' dense
/// ground).
fn dense_ground() -> PointCloud {
    let mut points = Vec::with_capacity(160_000);
    for j in 0..400 {
        for i in 0..400 {
            points.push(PointCloudPoint {
                x: 0.25 * f64::from(i),
                y: 0.25 * f64::from(j),
                z: 10.0,
                classification: 2,
                ..PointCloudPoint::default()
            });
        }
    }
    PointCloud {
        points,
        ..PointCloud::default()
    }
}

fn layer_description(name: &str, count: u64, geometry: &str, crs: &str) -> VectorLayerDescription {
    VectorLayerDescription {
        name: String::from(name),
        feature_count: count,
        geometry_type: String::from(geometry),
        crs: String::from(crs),
        ..VectorLayerDescription::default()
    }
}

mod dataset_info {
    use super::*;

    // ---- rasters --------------------------------------------------------------------------

    #[test]
    fn a_raster_is_described_with_its_georeferencing_and_no_data() {
        let dir = temp_dir("raster");
        let path = dir.file("dem.tif");
        let options = RasterExportOptions {
            width: 3,
            height: 2,
            geotransform: [1000.0, 2.0, 0.0, 2000.0, 0.0, -2.0],
            projection_wkt: String::from(MGA56_WKT),
            no_data_value: Some(-9999.0),
            ..RasterExportOptions::default()
        };
        write_raster(&path, &options);

        let d = must(describe(&path));
        assert_eq!(d.path, path);
        assert_eq!(d.kind, SourceKind::Raster);
        assert_eq!(d.driver, "GTiff");
        assert_eq!(d.crs, MGA56_NAME);
        assert!(!d.projection_wkt.is_empty());
        assert!(d.vector_layers.is_empty());
        assert!(d.point_cloud.is_none());
        let raster = d.raster.as_ref().expect("a raster");
        assert_eq!(raster.width, 3);
        assert_eq!(raster.height, 2);
        assert_eq!(raster.band_count, 1);
        assert!(raster.georeferenced);
        assert_eq!(raster.pixel_width, 2.0);
        assert_eq!(raster.pixel_height, 2.0);
        // 3 pixels of 2 m east of 1000, 2 pixels of 2 m south of 2000.
        assert_eq!(raster.bounds.min, Point2::new(1000.0, 1996.0));
        assert_eq!(raster.bounds.max, Point2::new(1006.0, 2000.0));
        assert_eq!(raster.no_data_value, Some(-9999.0));

        let text = format_description(&d);
        for fact in [
            "dem.tif",
            "raster (GTiff)",
            MGA56_NAME,
            "3 x 2 pixels",
            "1 band",
            "(1000.000, 1996.000) to (1006.000, 2000.000)",
            "No-data value: -9999",
        ] {
            assert_holds(&text, fact);
        }
    }

    #[test]
    fn a_rotated_raster_reports_its_true_pixel_size_and_all_four_corners() {
        // Columns step (3, 4) and rows (4, -3): 5 m pixels, turned. Corners
        // of a 2 x 1 grid through the full affine: (0,0) -> (1000, 2000),
        // (2,0) -> (1006, 2008), (0,1) -> (1004, 1997), (2,1) -> (1010,
        // 2005). Reading only gt[1] and gt[5] would report 3 x 3 pixels and a
        // box missing two of those corners.
        let dir = temp_dir("rotated");
        let path = dir.file("rotated.tif");
        let options = RasterExportOptions {
            width: 2,
            height: 1,
            geotransform: [1000.0, 3.0, 4.0, 2000.0, 4.0, -3.0],
            ..RasterExportOptions::default()
        };
        write_raster(&path, &options);

        let described = must(describe(&path));
        let raster = described.raster.as_ref().expect("a raster");
        assert_eq!(raster.pixel_width, 5.0);
        assert_eq!(raster.pixel_height, 5.0);
        assert_eq!(raster.bounds.min, Point2::new(1000.0, 1997.0));
        assert_eq!(raster.bounds.max, Point2::new(1010.0, 2008.0));
    }

    #[test]
    fn a_raster_without_georeferencing_or_crs_says_so() {
        // Written by hand as a VRT with no <GeoTransform>: GTiff records even
        // GDAL's default transform as real georeferencing, so write_raster
        // cannot make a raster without it.
        let dir = temp_dir("plain");
        let path = dir.file("scan.vrt");
        write_file(
            &path,
            r#"<VRTDataset rasterXSize="4" rasterYSize="3"><VRTRasterBand dataType="Float64" band="1"/></VRTDataset>"#,
        );

        let described = must(describe(&path));
        assert_eq!(described.driver, "VRT");
        let raster = described.raster.as_ref().expect("a raster");
        assert_eq!(raster.width, 4);
        assert_eq!(raster.height, 3);
        assert!(!raster.georeferenced);
        assert!(
            raster.bounds.empty(),
            "the origin is GDAL's guess, not the raster's position"
        );
        assert!(described.crs.is_empty());
        assert!(raster.no_data_value.is_none());

        let text = format_description(&described);
        assert_holds(&text, "not georeferenced");
        assert_holds(&text, "no coordinate system declared");
        assert_holds(&text, "No-data value: none declared");
    }

    // ---- vector layers --------------------------------------------------------------------

    #[test]
    fn every_vector_layer_is_listed_with_its_count_type_and_crs() {
        let dir = temp_dir("vector");
        let path = dir.file("survey.gpkg");
        let features: Vec<VectorFeature> = (0..3)
            .map(|i| point_feature(255440.0 + f64::from(i), 7410850.0))
            .collect();
        let options = VectorExportOptions {
            layer_name: String::from("control"),
            projection_wkt: String::from(MGA56_WKT),
            ..VectorExportOptions::default()
        };
        must(GdalDataset::write_vector(&path, &features, &options));

        let described = must(describe(&path));
        assert_eq!(described.kind, SourceKind::Vector);
        assert_eq!(described.driver, "GPKG");
        assert!(described.raster.is_none());
        assert_eq!(described.vector_layers.len(), 1);
        let layer = &described.vector_layers[0];
        assert_eq!(layer.name, "control");
        assert_eq!(layer.feature_count, 3);
        assert_eq!(layer.geometry_type, "Point");
        assert_eq!(layer.crs, MGA56_NAME);
        // The layers agree, so the file's CRS is theirs.
        assert_eq!(described.crs, MGA56_NAME);

        let text = format_description(&described);
        assert_holds(&text, "vector (GPKG)");
        assert_holds(&text, &format!("control: 3 features, Point, {MGA56_NAME}"));
    }

    #[test]
    fn a_shapefile_without_a_prj_declares_no_coordinate_system() {
        let dir = temp_dir("shapefile");
        let path = dir.file("marks.shp");
        must(GdalDataset::write_vector(
            &path,
            &[point_feature(1.0, 2.0)],
            &VectorExportOptions::default(),
        ));

        let described = must(describe(&path));
        assert_eq!(described.driver, "ESRI Shapefile");
        assert_eq!(described.vector_layers.len(), 1);
        assert_eq!(described.vector_layers[0].feature_count, 1);
        assert!(described.vector_layers[0].crs.is_empty());
        assert!(described.crs.is_empty());
        let text = format_description(&described);
        assert_holds(&text, "Coordinate system: no coordinate system declared");
        assert_holds(&text, "1 feature, Point, no coordinate system declared");
    }

    // ---- point clouds ---------------------------------------------------------------------

    #[test]
    fn a_point_cloud_is_described_from_its_header() {
        let dir = temp_dir("cloud");
        let path = dir.file("scan.las");
        must(PointCloudEngine.write(&path, &coloured_cloud()));

        let described = must(describe(&path));
        assert_eq!(described.kind, SourceKind::PointCloud);
        assert_eq!(described.driver, "readers.las");
        let cloud = described.point_cloud.as_ref().expect("a cloud");
        assert_eq!(cloud.point_count, 1234);
        assert!(cloud.has_color);
        assert!(!cloud.copc);
        assert_eq!(cloud.bounds.min_x, 250000.0);
        assert_eq!(cloud.bounds.max_x, 250099.0);
        assert_eq!(cloud.bounds.min_y, 7400000.0);
        assert_eq!(cloud.bounds.max_y, 7400012.0);
        assert_eq!(cloud.bounds.min_z, 10.0);
        assert_eq!(cloud.bounds.max_z, 10.75);

        let text = format_description(&described);
        for fact in [
            "point cloud (readers.las)",
            "1,234 points",
            "x 250000.000 to 250099.000",
            "y 7400000.000 to 7400012.000",
            "z 10.000 to 10.750",
            "Colour: yes",
            "COPC: no",
            "no coordinate system declared",
        ] {
            assert_holds(&text, fact);
        }
    }

    #[test]
    fn a_copc_file_is_described_as_one_and_is_imported_by_resolution() {
        let dir = temp_dir("copc");
        let las = dir.file("ground.las");
        let copc = dir.file("ground.copc.laz");
        let engine = PointCloudEngine;
        must(engine.write(&las, &dense_ground()));
        must(engine.convert_to_copc(&las, &copc));

        let described = must(describe(&copc));
        assert_eq!(described.driver, "readers.copc");
        let cloud = described.point_cloud.as_ref().expect("a cloud");
        assert!(cloud.copc);
        assert_eq!(cloud.point_count, 160000);
        assert!(format_description(&described).contains("COPC: yes"));

        // What the description promises: a resolution query instead of a
        // decimation. The octree does the thinning, so no decimation step is
        // applied on top, and the coarse read still spans the whole square.
        let coarse = PointCloudImportOptions {
            resolution: Some(5.0),
            ..PointCloudImportOptions::default()
        };
        let layer = must(import_point_cloud(&copc, &coarse));
        assert_eq!(layer.decimation_step, 1);
        assert!(!layer.points.is_empty());
        assert!(
            layer.points.len() * 4 < 160000,
            "a 5 m query on 0.25 m data"
        );
        assert_eq!(layer.source_point_count, 160000);
        assert!(layer.is_decimated());
        assert!(layer.bounds.min_x < 10.0);
        assert!(layer.bounds.max_x > 90.0);

        // The budget still caps what is kept.
        let capped = PointCloudImportOptions {
            resolution: Some(0.25),
            budget: 100,
            ..PointCloudImportOptions::default()
        };
        let small = must(import_point_cloud(&copc, &capped));
        assert!(small.points.len() <= 100);

        // A plain LAS cannot answer a resolution; the import says so rather
        // than quietly reading the whole file.
        let error = refused(import_point_cloud(&las, &coarse), "a resolution of a LAS");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(error.describe().contains("COPC"), "{}", error.describe());
    }

    // ---- refusals -------------------------------------------------------------------------

    #[test]
    fn what_cannot_be_described_is_refused_with_the_reason() {
        let dir = temp_dir("refused");
        assert_eq!(
            refused(describe(&dir.file("missing.tif")), "a missing file").code,
            ErrorCode::NotFound
        );

        let unknown = dir.file("notes.xyzzy");
        write_file(&unknown, "hello");
        assert_eq!(
            refused(describe(&unknown), "an unknown extension").code,
            ErrorCode::Unsupported
        );

        // An archive's header says nothing its contents do not; it is
        // described by importing it.
        let archive = dir.file("job.12da");
        write_file(&archive, "// archive\n");
        assert_eq!(
            refused(describe(&archive), "an archive").code,
            ErrorCode::Unsupported
        );

        let broken_raster = dir.file("broken.tif");
        write_file(&broken_raster, "not a raster at all");
        assert_eq!(
            refused(describe(&broken_raster), "a broken raster").code,
            ErrorCode::FileImportFailure
        );

        let broken_cloud = dir.file("broken.las");
        write_file(&broken_cloud, "not a point cloud at all");
        assert_eq!(
            refused(describe(&broken_cloud), "a broken cloud").code,
            ErrorCode::FileImportFailure
        );
    }

    // ---- format_description ---------------------------------------------------------------

    #[test]
    fn counts_are_grouped_in_thousands_whatever_the_locale() {
        let cloud_text = |count: u64| {
            let d = SourceDescription {
                path: PathBuf::from("scan.laz"),
                kind: SourceKind::PointCloud,
                point_cloud: Some(PointCloudDescription {
                    point_count: count,
                    ..PointCloudDescription::default()
                }),
                ..SourceDescription::default()
            };
            format_description(&d)
        };
        assert!(cloud_text(0).contains("0 points"));
        assert!(cloud_text(1).contains("1 point\n"));
        assert!(cloud_text(999).contains("999 points"));
        assert!(cloud_text(1000).contains("1,000 points"));
        assert!(cloud_text(1234567).contains("1,234,567 points"));
        assert!(cloud_text(18446744073709551615).contains("18,446,744,073,709,551,615 points"));
        // No bounds in a header is said, not printed as infinities.
        assert!(cloud_text(5).contains("Bounds: none declared"));
    }

    #[test]
    fn coordinates_are_printed_to_the_millimetre() {
        let d = SourceDescription {
            path: PathBuf::from("site.tif"),
            kind: SourceKind::Raster,
            raster: Some(RasterDescription {
                georeferenced: true,
                pixel_width: 0.5,
                pixel_height: 0.5,
                bounds: Box2::new(
                    Point2::new(255440.1234, 7410850.4567),
                    Point2::new(255500.0, 7410900.9996),
                ),
                ..RasterDescription::default()
            }),
            ..SourceDescription::default()
        };
        let text = format_description(&d);
        // 0.1234 -> .123, 0.4567 -> .457, 0.9996 -> 1.000 (carrying into the
        // unit).
        assert_holds(
            &text,
            "(255440.123, 7410850.457) to (255500.000, 7410901.000)",
        );
        assert_holds(&text, "Pixel size: 0.5 x 0.5");
    }

    #[test]
    fn layers_that_disagree_are_not_reported_as_having_no_crs() {
        let d = SourceDescription {
            path: PathBuf::from("mixed.gpkg"),
            kind: SourceKind::Vector,
            vector_layers: vec![
                layer_description("a", 1, "Point", MGA56_NAME),
                layer_description("b", 2, "LineString", "WGS 84 (EPSG:4326)"),
            ],
            ..SourceDescription::default()
        };
        let text = format_description(&d);
        assert!(
            !text.contains("Coordinate system: no coordinate system declared"),
            "{text}"
        );
        assert_holds(&text, "differs between layers");
        assert_holds(&text, "b: 2 features, LineString, WGS 84 (EPSG:4326)");
    }

    // ---- GDAL's own reading: fields, bands, /vsi paths ------------------------------------

    #[test]
    fn the_fields_and_bands_are_read_from_gdals_own_description() {
        // A layer whose fields GDAL's GeoJSON driver types from their
        // values: a whole number Integer, a fraction Real (the driver's
        // documentation).
        let dir = temp_dir("fields");
        let path = dir.file("plots.geojson");
        write_file(
            &path,
            r#"{"type":"FeatureCollection","name":"plots","features":[
        {"type":"Feature","properties":{"plot":"P1","count":3,"area":12.5},
         "geometry":{"type":"Point","coordinates":[1,2]}}]}"#,
        );
        let described = must(describe(&path));
        assert_eq!(described.vector_layers.len(), 1);
        let fields = &described.vector_layers[0].fields;
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0].name, "plot");
        assert_eq!(fields[0].r#type, "String");
        assert_eq!(fields[1].r#type, "Integer");
        assert_eq!(fields[2].r#type, "Real");
        assert_eq!(described.vector_layers[0].extent.min, Point2::new(1.0, 2.0));
        assert!(!described.vector_json.is_empty());
        assert!(described.raster_json.is_empty());

        // A band: its type and no-data, and no statistics until asked for.
        let raster = dir.file("dem.tif");
        let options = RasterExportOptions {
            width: 3,
            height: 2,
            geotransform: [1000.0, 2.0, 0.0, 2000.0, 0.0, -2.0],
            no_data_value: Some(-9999.0),
            ..RasterExportOptions::default()
        };
        write_raster(&raster, &options);
        let band = must(describe(&raster));
        let bands = &band.raster.as_ref().expect("a raster").bands;
        assert_eq!(bands.len(), 1);
        assert_eq!(bands[0].band, 1);
        assert_eq!(bands[0].data_type, "Float64");
        assert_eq!(bands[0].no_data, Some(-9999.0));
        assert!(bands[0].min.is_none());
        // write_raster wrote every cell 1.0: the statistics are exactly that.
        let statistics = DescribeOptions {
            statistics: true,
            ..DescribeOptions::default()
        };
        let computed = must(describe_source(&raster, &statistics));
        let one = &computed.raster.as_ref().expect("a raster").bands[0];
        assert_eq!(one.min, Some(1.0));
        assert_eq!(one.max, Some(1.0));
        assert_eq!(one.mean, Some(1.0));
        assert_eq!(one.std_dev, Some(0.0));
        assert!(!dir.file("dem.tif.aux.xml").exists());
    }

    #[test]
    fn a_vsi_path_is_gdals_to_find_and_is_described() {
        // Inside a zip: no such file is on the disk, and GDAL reads it all
        // the same, so it is not refused as missing first.
        let dir = temp_dir("vsizip");
        let archive = dir.file("control.zip");
        must(write_zip(
            &archive,
            "control.geojson",
            br#"{"type":"FeatureCollection","features":[
        {"type":"Feature","properties":{"name":"CP1"},"geometry":{"type":"Point","coordinates":[5,6]}}]}"#,
        ));
        let inside = PathBuf::from(format!(
            "/vsizip/{}/control.geojson",
            archive.to_string_lossy().replace('\\', "/")
        ));
        assert!(is_virtual_path(&inside));
        assert!(!is_virtual_path(&archive));
        let described = must(describe(&inside));
        assert_eq!(described.vector_layers.len(), 1);
        assert_eq!(described.vector_layers[0].feature_count, 1);

        let missing = refused(describe(&dir.file("absent.geojson")), "a missing file");
        assert_eq!(missing.code, ErrorCode::NotFound);
    }
}
