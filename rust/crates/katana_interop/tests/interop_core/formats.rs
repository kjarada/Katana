//! tests/geo/test_formats.cpp, the Formats cases of the interop core: the
//! formats come from GDAL's own registry, a file is routed by what it holds,
//! and /vsi paths and archives open (docs/interop.md, "Formats").
//!
//! What a format can do is GDAL's to say, so the expectations here are either
//! read from GDAL's registry at run time (where a build may differ) or are
//! facts of GDAL's drivers as its documentation states them - a GeoPackage
//! holds rasters and vectors and GDAL writes both; GDAL has no ECW reader
//! without the vendor's SDK - and the geometry read back is compared with the
//! coordinates written, those of KMZ and GPX through PROJ.
//!
//! The file's other cases are elsewhere: three Formats cases of the registry
//! alone are katana_gis's (tests/gdal_core); FormatsVerb and
//! AGpkgHoldingOnlyARasterRoutesToRasterImport drive the app's verbs (step
//! 6).

use std::fs;
use std::path::Path;

use katana_core::ErrorCode;
use katana_entity::{Entity, Geometry, Layer, Model, PropertyValue};
use katana_geometry::{Point2, Polyline2, Segment2};
use katana_gis::formats::{
    DataKind, find_format, identify_content, readable_extensions, vector_save_choices,
    vector_writer_for,
};
use katana_gis::gdal_adapter::GdalDataset;
use katana_gis::reproject::transform_point;
use katana_gis::zip_container::{ZipContent, list_zip, write_zip_members};
use katana_interop::export::{VectorExportOptions, export_vector, vector_export_formats};
use katana_interop::import::{
    SourceKind, VectorImportOptions, import_raster, import_vector, kind_for_path,
    raster_extensions, vector_extensions,
};

use crate::{TempDir, bytes_of, geo_fixture, must, near, refused, warned, write_file};

/// The MGA zone 56 of GDA94: projected, metres, what a survey in Sydney is
/// in.
const MGA56: &str = "EPSG:28356";

fn temp_dir(name: &str) -> TempDir {
    TempDir::new("katana-formats-", name)
}

fn contains(values: &[String], value: &str) -> bool {
    values.iter().any(|each| each.as_str() == value)
}

fn import(path: &Path) -> katana_core::Result<katana_interop::VectorImportResult> {
    import_vector(path, &VectorImportOptions::default())
}

fn append_little_endian(out: &mut Vec<u8>, value: u32, bytes: usize) {
    out.extend_from_slice(&value.to_le_bytes()[..bytes]);
}

/// One gzip member (RFC 1952) holding the bytes in stored deflate blocks (RFC
/// 1951, 3.2.4), built here so that what GDAL reads is not what GDAL wrote.
fn gzipped(bytes: &[u8]) -> Vec<u8> {
    // RFC 1952, 8: the CRC-32 of ISO 3309.
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & 0_u32.wrapping_sub(crc & 1));
        }
    }
    crc ^= 0xFFFF_FFFF;
    // ID1 ID2, CM = deflate, no flags, no time, no extra flags, OS unknown.
    let mut out: Vec<u8> = vec![0x1f, 0x8b, 0x08, 0, 0, 0, 0, 0, 0, 0xff];
    let mut at = 0;
    loop {
        let size = (bytes.len() - at).min(65535);
        // BFINAL; BTYPE 00, stored.
        out.push(u8::from(at + size == bytes.len()));
        append_little_endian(&mut out, size as u32, 2);
        append_little_endian(&mut out, !(size as u32) & 0xFFFF, 2);
        out.extend_from_slice(&bytes[at..at + size]);
        at += size;
        if at >= bytes.len() {
            break;
        }
    }
    append_little_endian(&mut out, crc, 4);
    append_little_endian(&mut out, bytes.len() as u32, 4);
    out
}

/// A tar of one file, in POSIX's ustar format (IEEE 1003.1-2001, "pax",
/// ustar Interchange Format): a 512-byte header, the bytes to a whole block,
/// two empty blocks.
fn tarred(name: &str, bytes: &[u8]) -> Vec<u8> {
    let mut header = vec![0_u8; 512];
    let put = |header: &mut [u8], at: usize, text: &[u8]| {
        header[at..at + text.len()].copy_from_slice(text);
    };
    let octal = |mut value: u64, digits: usize| {
        let mut text = vec![b'0'; digits];
        let mut i = digits;
        while i > 0 && value != 0 {
            i -= 1;
            text[i] = b'0' + (value % 8) as u8;
            value /= 8;
        }
        text
    };
    put(&mut header, 0, name.as_bytes());
    put(&mut header, 100, &octal(0o644, 7)); // mode
    put(&mut header, 108, &octal(0, 7)); // uid
    put(&mut header, 116, &octal(0, 7)); // gid
    put(&mut header, 124, &octal(bytes.len() as u64, 11));
    put(&mut header, 136, &octal(0, 11)); // mtime
    put(&mut header, 148, b"        "); // the checksum counts itself as spaces
    header[156] = b'0'; // a regular file
    put(&mut header, 257, b"ustar");
    put(&mut header, 263, b"00");
    let sum: u64 = header.iter().map(|&byte| u64::from(byte)).sum();
    put(&mut header, 148, &octal(sum, 6));
    header[154] = 0;
    header[155] = b' ';
    let mut out = header;
    out.extend_from_slice(bytes);
    out.resize(out.len() + (512 - bytes.len() % 512) % 512, 0);
    out.resize(out.len() + 1024, 0);
    out
}

fn model_with(entities: Vec<Entity>) -> Model {
    let mut model = Model::default();
    for entity in entities {
        if !model.layers.contains(&entity.layer) {
            let layer = Layer {
                name: entity.layer.clone(),
                ..Layer::default()
            };
            assert!(model.layers.add(layer).is_ok(), "{}", entity.layer);
        }
        assert!(model.entities.add(entity).is_ok());
    }
    model
}

/// A kerb (a segment) and a fence (a polyline) at survey coordinates, with a
/// text, a real and an integer property each.
fn kerb_and_fence() -> (Entity, Entity, Model) {
    let mut kerb = Entity {
        geometry: Geometry::Line(Segment2::new(
            Point2::new(330000.0, 6250000.0),
            Point2::new(330030.0, 6250040.0),
        )),
        ..Entity::default()
    };
    kerb.properties.insert(
        String::from("code"),
        PropertyValue::Text(String::from("KERB")),
    );
    kerb.properties
        .insert(String::from("width"), PropertyValue::Real(0.15));
    kerb.properties
        .insert(String::from("count"), PropertyValue::Integer(3));
    let mut fence = Entity {
        geometry: Geometry::Polyline(Polyline2::new(
            vec![
                Point2::new(330000.0, 6250100.0),
                Point2::new(330010.0, 6250105.0),
                Point2::new(330020.0, 6250100.0),
            ],
            false,
        )),
        ..Entity::default()
    };
    fence.properties.insert(
        String::from("code"),
        PropertyValue::Text(String::from("FENCE")),
    );
    fence
        .properties
        .insert(String::from("width"), PropertyValue::Real(0.05));
    fence
        .properties
        .insert(String::from("count"), PropertyValue::Integer(12));
    let model = model_with(vec![kerb.clone(), fence.clone()]);
    (kerb, fence, model)
}

fn points_of(entity: &Entity) -> Vec<Point2> {
    match &entity.geometry {
        Geometry::Line(segment) => vec![segment.start, segment.end],
        Geometry::Polyline(line) => line.vertices.clone(),
        _ => Vec::new(),
    }
}

mod formats {
    use super::*;

    // ---- the registry ---------------------------------------------------------------------

    #[test]
    fn formats_lists_gpkg_as_read_write() {
        // GDAL's GeoPackage driver reads and writes both tiles and features
        // (gdal.org/drivers/raster/gpkg.html, drivers/vector/gpkg.html).
        let gpkg = find_format("gpkg").expect("a driver is found whatever the case it is named in");
        assert_eq!(gpkg.driver, "GPKG");
        assert_eq!(gpkg.description, "GeoPackage");
        assert!(gpkg.read_raster && gpkg.read_vector && gpkg.write_raster && gpkg.write_vector);
        assert!(gpkg.virtual_io);
        assert!(contains(&gpkg.extensions, "gpkg"));
        assert!(contains(&vector_extensions(), "gpkg"));
        assert!(contains(&raster_extensions(), "gpkg"));
    }

    #[test]
    fn ecw_is_not_offered() {
        // The hand-kept list offered .ecw, which only a GDAL built with the
        // vendor's SDK opens; the file dialog now offers what the registry
        // has.
        let has_reader = find_format("ECW").is_some();
        assert_eq!(contains(&raster_extensions(), "ecw"), has_reader);
        assert_eq!(
            contains(&readable_extensions(DataKind::Raster), "ecw"),
            has_reader
        );
        if !has_reader {
            assert_eq!(
                kind_for_path(Path::new("ortho.ecw")),
                SourceKind::Unknown,
                "an extension no reader claims routes nowhere by its name"
            );
        }
    }

    #[test]
    fn drivers_that_are_no_format_are_not_offered() {
        for driver in ["MEM", "DERIVED", "HTTP", "AIVector", "GPSBabel", "GNMFile"] {
            assert!(find_format(driver).is_none(), "{driver}");
        }
        // Sidecars and generic extensions are opened through their data file.
        let vector = readable_extensions(DataKind::Vector);
        for extension in ["dbf", "shx", "prj", "txt", "xml"] {
            assert!(!contains(&vector, extension), "{extension}");
        }
        assert!(contains(&vector, "shp"));
        assert!(contains(&vector, "fgb"));
        assert!(vector.is_sorted());
        // What another importer takes by its extension is not offered as
        // GDAL's.
        assert!(!contains(&raster_extensions(), "e57"));
    }

    #[test]
    fn the_save_dialog_offers_every_writer_with_the_common_ones_first() {
        let choices = vector_save_choices();
        assert!(!choices.is_empty());
        assert_eq!(choices[0].extension, "shp");
        let mut extensions: Vec<String> = Vec::new();
        for choice in &choices {
            assert!(
                !contains(&extensions, &choice.extension),
                "each extension once: {}",
                choice.extension
            );
            extensions.push(choice.extension.clone());
            // What the dialog offers is what EXPORT writes that name with.
            let writer = must(vector_writer_for(Path::new(&format!(
                "katana.{}",
                choice.extension
            ))));
            assert_eq!(writer, choice.driver, "{}", choice.extension);
        }
        for extension in [
            "geojson", "gpkg", "kml", "kmz", "dxf", "csv", "fgb", "gpx", "shp.zip", "tab",
        ] {
            assert!(contains(&extensions, extension), "{extension}");
        }
        // The hand-kept table's six were always there; now they come first.
        let interop_choices = vector_export_formats();
        assert_eq!(interop_choices.len(), choices.len());
        assert_eq!(interop_choices[0].extension, "shp");
    }

    // ---- formats the old tables could not reach -------------------------------------------

    #[test]
    fn flat_geobuf_geo_parquet_kmz_gpx_and_csv_wkt_round_trip() {
        // Written by the writer each NAME picks - no driver given - and read
        // back: the geometry to the micrometre, or, in longitude and latitude
        // (KMZ, GPX), to 1e-9 degrees (0.1 mm) of PROJ's conversion; the
        // fields with their types where the format has them.
        struct Case {
            file: &'static str,
            driver: &'static str,
            lon_lat: bool,
            typed: bool,
        }
        let cases = [
            Case {
                file: "lines.fgb",
                driver: "FlatGeobuf",
                lon_lat: false,
                typed: true,
            },
            Case {
                file: "lines.parquet",
                driver: "Parquet",
                lon_lat: false,
                typed: true,
            },
            Case {
                file: "lines.kmz",
                driver: "LIBKML",
                lon_lat: true,
                typed: true,
            },
            Case {
                file: "lines.gpx",
                driver: "GPX",
                lon_lat: true,
                typed: false,
            },
            Case {
                file: "lines.csv",
                driver: "CSV",
                lon_lat: false,
                typed: true,
            },
        ];
        let (kerb, fence, model) = kerb_and_fence();
        for test in &cases {
            if find_format(test.driver).is_none() {
                // Linux distributions build GDAL without some drivers
                // (Parquet needs Arrow); a missing driver is the build's, not
                // a defect.
                eprintln!("[ SKIPPED  ] {} is not in this GDAL build", test.driver);
                continue;
            }
            let dir = temp_dir(&format!("round-trip-{}", test.file));
            let options = VectorExportOptions {
                projection_wkt: String::from(MGA56),
                ..VectorExportOptions::default()
            };
            let exported = must(export_vector(&model, &dir.file(test.file), &options));
            assert_eq!(exported.driver, test.driver, "{}", test.file);
            assert_eq!(exported.features_written, 2, "{}", test.file);
            assert_eq!(
                kind_for_path(&dir.file(test.file)),
                SourceKind::Vector,
                "{}",
                test.file
            );

            let imported = must(import(&dir.file(test.file)));
            // Two lines out, two entities in: GDAL reads a GPX's routes a
            // second time as a layer of their vertices (route_points), which
            // is left out and said, not made five points beside the lines.
            assert_eq!(imported.entities.len(), 2, "{}", test.file);
            if test.driver == "GPX" {
                assert!(warned(
                    &imported.warnings,
                    "GPX layer 'route_points' left out"
                ));
            }
            let lines: Vec<&Entity> = imported
                .entities
                .iter()
                .filter(|entity| points_of(entity).len() >= 2)
                .collect();
            assert_eq!(lines.len(), 2, "{}", test.file);
            for back in lines {
                let got = points_of(back);
                let original = if got.len() == 2 { &kerb } else { &fence };
                let expected = points_of(original);
                assert_eq!(got.len(), expected.len(), "{}", test.file);
                for (point, want) in got.iter().zip(&expected) {
                    let mut want = *want;
                    if test.lon_lat {
                        let moved = must(transform_point(want.x, want.y, MGA56, "EPSG:4326"));
                        want = Point2::new(moved[0], moved[1]);
                    }
                    let tolerance = if test.lon_lat { 1e-9 } else { 1e-6 };
                    near(point.x, want.x, tolerance);
                    near(point.y, want.y, tolerance);
                }
                // The code comes back wherever the format keeps it.
                let coded = back
                    .properties
                    .values()
                    .any(|value| *value == original.properties["code"]);
                assert!(coded, "{}: the code field is kept", test.file);
                if !test.typed {
                    continue;
                }
                let Some(PropertyValue::Real(width)) = back.properties.get("width") else {
                    panic!("{}: the width is not a real", test.file);
                };
                let Some(count) = back.properties.get("count") else {
                    panic!("{}: no count", test.file);
                };
                let PropertyValue::Real(original_width) = original.properties["width"] else {
                    panic!("the original width is a real");
                };
                near(*width, original_width, 1e-12);
                assert_eq!(*count, original.properties["count"], "{}", test.file);
            }
        }
    }

    #[test]
    fn a_shapefile_inside_a_zip_imports_through_vsizip() {
        let dir = temp_dir("zipped-shapefile");
        let (kerb, fence, model) = kerb_and_fence();
        let options = VectorExportOptions {
            projection_wkt: String::from(MGA56),
            ..VectorExportOptions::default()
        };
        // GDAL writes a zipped shapefile itself, named .shp.zip.
        let exported = must(export_vector(&model, &dir.file("lots.shp.zip"), &options));
        assert_eq!(exported.driver, "ESRI Shapefile");

        // The same archive as a plain .zip, which GDAL does not open as it
        // is.
        fs::copy(dir.file("lots.shp.zip"), dir.file("lots.zip")).expect("the archive is copied");
        let members = must(list_zip(&dir.file("lots.zip")));
        // The last member that is one, as the C++ loop keeps it.
        let shp = members
            .iter()
            .rev()
            .find(|member| member.name.ends_with(".shp"))
            .map(|member| member.name.clone())
            .expect("the archive holds a .shp");

        // Routed by its inside, and imported through it.
        assert_eq!(kind_for_path(&dir.file("lots.zip")), SourceKind::Vector);
        let whole = must(import(&dir.file("lots.zip")));
        assert_eq!(whole.entities.len(), 2);

        // Named inside the archive, as GDAL names it: a /vsi path, which the
        // exists check used to refuse as a missing file.
        let inside = format!(
            "/vsizip/{}/{shp}",
            dir.file("lots.zip").to_string_lossy().replace('\\', "/")
        );
        assert_eq!(kind_for_path(Path::new(&inside)), SourceKind::Vector);
        let named = must(import(Path::new(&inside)));
        assert_eq!(named.entities.len(), 2);
        for entity in &named.entities {
            let got = points_of(entity);
            let want = points_of(if got.len() == 2 { &kerb } else { &fence });
            assert_eq!(got.len(), want.len());
            near(got[0].x, want[0].x, 1e-6);
            near(got[0].y, want[0].y, 1e-6);
        }
    }

    #[test]
    fn an_archive_of_several_datasets_is_refused_naming_them() {
        let dir = temp_dir("two-datasets");
        let lots = bytes_of(&geo_fixture("lots.geojson"));
        assert!(!lots.is_empty());
        let members = [
            ZipContent {
                name: "a.geojson",
                bytes: &lots,
            },
            ZipContent {
                name: "b.geojson",
                bytes: &lots,
            },
        ];
        must(write_zip_members(&dir.file("two.zip"), &members));

        let opened = refused(
            GdalDataset::open(&dir.file("two.zip")),
            "an archive of two datasets",
        );
        assert_eq!(opened.code, ErrorCode::InvalidArgument);
        assert!(
            opened.message.contains("2 datasets (a.geojson, b.geojson)"),
            "{}",
            opened.message
        );
        assert!(
            opened.message.contains("/vsizip/"),
            "the refusal says how to name one"
        );

        // Named, one opens.
        let one = format!(
            "/vsizip/{{{}}}/b.geojson",
            dir.file("two.zip").to_string_lossy().replace('\\', "/")
        );
        let imported = must(import(Path::new(&one)));
        assert!(!imported.entities.is_empty());
    }

    #[test]
    fn a_gzipped_file_and_a_tar_open_by_what_they_hold() {
        // GDAL opens neither as it is: a .gz is read through /vsigzip - which
        // takes no braces, the defect this found - and a tar as a folder.
        let dir = temp_dir("compressed");
        let lots = bytes_of(&geo_fixture("lots.geojson"));
        assert!(!lots.is_empty());
        write_file(&dir.file("lots.geojson.gz"), gzipped(&lots));
        write_file(&dir.file("lots.tar"), tarred("lots.geojson", &lots));
        for name in ["lots.geojson.gz", "lots.tar"] {
            assert_eq!(kind_for_path(&dir.file(name)), SourceKind::Vector, "{name}");
            let imported = must(import(&dir.file(name)));
            assert_eq!(
                imported.entities.len(),
                3,
                "{name}: the three lots of lots.geojson"
            );
        }
    }

    #[test]
    fn an_unknown_extension_is_routed_by_content() {
        let dir = temp_dir("unknown-extension");
        fs::copy(geo_fixture("lot_with_hole.geojson"), dir.file("lot.data"))
            .expect("the lot is copied");
        fs::copy(geo_fixture("plane.asc"), dir.file("plane.grid")).expect("the plane is copied");
        assert_eq!(kind_for_path(&dir.file("lot.data")), SourceKind::Vector);
        assert_eq!(kind_for_path(&dir.file("plane.grid")), SourceKind::Raster);
        let imported = must(import(&dir.file("lot.data")));
        assert_eq!(imported.entities.len(), 2, "the lot and its hole");
        let raster = must(import_raster(&dir.file("plane.grid"), &Default::default()));
        assert_eq!(raster.width, 40);

        // Text no reader recognises routes nowhere, and says so.
        write_file(&dir.file("notes.data"), "these are not coordinates\n");
        assert_eq!(kind_for_path(&dir.file("notes.data")), SourceKind::Unknown);
        let nothing = refused(
            identify_content(&dir.file("notes.data")),
            "text no reader recognises",
        );
        assert_eq!(nothing.code, ErrorCode::FileImportFailure);
    }
}
