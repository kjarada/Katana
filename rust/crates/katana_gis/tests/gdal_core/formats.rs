//! tests/geo/test_formats.cpp, three of its Formats cases: the writer GDAL
//! picks for a name, the paths that are no file, and the drivers' declared
//! option lists. The file's other Formats cases import and export through
//! katana_interop (wave M), and its FORMATS verb cases are the app's.
//!
//! What a format can do is GDAL's to say, so the expectations here are facts
//! of GDAL's drivers as its documentation states them.

use std::path::Path;

use katana_core::ErrorCode;
use katana_gis::FormatOption;
use katana_gis::formats::{format_options, is_remote_path, is_virtual_path, vector_writer_for};
use katana_gis::gdal_adapter::GdalDataset;

use crate::{must, refused};

fn named<'a>(options: &'a [FormatOption], name: &str) -> Option<&'a FormatOption> {
    options.iter().find(|option| option.name == name)
}

mod formats {
    use super::*;

    #[test]
    fn the_writer_for_a_name_is_gdals_own_choice_with_katanas_overlay() {
        let writer = |name: &str| match vector_writer_for(Path::new(name)) {
            Ok(found) => found,
            Err(error) => format!("error: {}", error.describe()),
        };
        // GDAL's own ranking (GDALGetOutputDriversForDatasetName) ...
        assert_eq!(writer("peg.kml"), "LIBKML");
        assert_eq!(writer("peg.KMZ"), "LIBKML");
        assert_eq!(writer("lots.json"), "GeoJSON");
        assert_eq!(writer("lots.fgb"), "FlatGeobuf");
        assert_eq!(writer("lots.shp"), "ESRI Shapefile");
        assert_eq!(
            writer("lots.shp.zip"),
            "ESRI Shapefile",
            "a compound extension is whole"
        );
        // ... and Katana's where it differs: GDAL would write NASA's PDS4.
        assert_eq!(writer("lots.xml"), "GML");

        let none = refused(
            vector_writer_for(Path::new("lots")),
            "a name with no extension",
        );
        assert_eq!(none.code, ErrorCode::InvalidArgument);
        // OSM has a reader and no writer; GPSBabel, which GDAL would pick,
        // needs a program Katana does not ship. A raster format writes no
        // features.
        for name in ["roads.osm", "dem.xyz", "lots.nothing"] {
            let error = refused(vector_writer_for(Path::new(name)), name);
            assert_eq!(error.code, ErrorCode::Unsupported, "{name}");
            assert!(
                error
                    .message
                    .contains("no vector driver is registered for '."),
                "{}",
                error.message
            );
        }
        // GdalDataset's own question is the same one.
        assert_eq!(
            must(GdalDataset::vector_driver_for_path(Path::new("lots.fgb"))),
            "FlatGeobuf"
        );
    }

    #[test]
    fn a_path_that_is_no_file_is_left_to_gdal() {
        assert!(is_virtual_path(Path::new(
            "/vsizip/C:/data/lots.zip/lots.shp"
        )));
        assert!(is_virtual_path(Path::new("https://example.com/dem.tif")));
        assert!(
            is_virtual_path(Path::new("PG:dbname=survey")),
            "a driver's connection prefix"
        );
        assert!(
            !is_virtual_path(Path::new("C:/data/lots.shp")),
            "a drive is no connection prefix"
        );
        assert!(!is_virtual_path(Path::new("lots.shp")));
        assert!(is_remote_path(Path::new(
            "/vsicurl/https://example.com/dem.tif"
        )));
        assert!(is_remote_path(Path::new("s3://bucket/dem.tif")));
        assert!(!is_remote_path(Path::new("/vsizip/lots.zip")));

        // A local file that is not there is still said so plainly ...
        let missing = refused(
            GdalDataset::open(Path::new("C:/no/such/folder/lots.shp")),
            "a missing file",
        );
        assert_eq!(missing.code, ErrorCode::NotFound);
        // ... but a /vsi path is GDAL's to open, not refused as a missing file.
        let in_memory = refused(
            GdalDataset::open(Path::new("/vsimem/katana-formats-nothing.geojson")),
            "an empty in-memory name",
        );
        assert_eq!(
            in_memory.code,
            ErrorCode::FileImportFailure,
            "{}",
            in_memory.describe()
        );
    }

    #[test]
    fn format_options_are_the_drivers_declared_lists() {
        // gdal.org/drivers/vector/gpkg.html: LIST_ALL_TABLES=AUTO/YES/NO, for
        // vector; ZLEVEL for PNG tiles, 1 to 9.
        let gpkg = must(format_options("GPKG"));
        let list_all =
            named(&gpkg.open, "LIST_ALL_TABLES").expect("GPKG opens with LIST_ALL_TABLES");
        assert_eq!(list_all.r#type, "string-select");
        assert_eq!(list_all.default_value, "AUTO");
        assert_eq!(list_all.scope, "vector");
        assert_eq!(list_all.choices, ["AUTO", "YES", "NO"]);
        let zlevel = named(&gpkg.open, "ZLEVEL").expect("GPKG opens with ZLEVEL");
        assert_eq!(zlevel.min, Some(1.0));
        assert_eq!(zlevel.max, Some(9.0));
        // A shapefile's layer takes its encoding (drivers/vector/shapefile.html).
        let shapefile = must(format_options("ESRI Shapefile"));
        assert!(named(&shapefile.layer_creation, "ENCODING").is_some());
        // GeoTIFF's creation options include its compression.
        let tiff = must(format_options("GTiff"));
        let compress = named(&tiff.creation, "COMPRESS").expect("GTiff creates with COMPRESS");
        assert!(compress.choices.iter().any(|choice| choice == "DEFLATE"));

        let unknown = refused(format_options("NoSuchDriver"), "an unknown driver");
        assert_eq!(unknown.code, ErrorCode::NotFound);
    }
}
