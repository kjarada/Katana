//! tests/io/test_gdal_pdal_adapters.cpp, its GdalAdapter cases: the GDAL
//! adapter (PLAN.MD Phases 17 and 20).
//!
//! The seam with a large third-party library, so the tests drive real files
//! rather than mocks: what needs verifying is how GDAL actually behaves, not
//! how we imagine it does. Failures are reported as Result, never thrown.

#![allow(
    unsafe_code,
    reason = "The two configuration cases read GDAL's option through a CPLGetConfigOption declared here, as the GDAL spike declares its own; on Windows the certificates case asks for libcurl's module."
)]

use std::ffi::{CStr, c_char};
use std::fs;
use std::path::{Path, PathBuf};
use std::ptr;

use katana_core::ErrorCode;
use katana_gis::gdal_adapter::{
    GdalDataset, describe_crs, driver_assumes_wgs84, driver_holds_one_geometry_type, gdal_version,
};
use katana_gis::{
    GeoPoint, GeometryKind, RasterExportOptions, VectorExportOptions, VectorFeature, VectorGeometry,
};

use crate::{double_eq, must, refused};

fn auxiliary_of(path: &Path) -> PathBuf {
    let mut aux = path.as_os_str().to_owned();
    aux.push(".aux.xml");
    PathBuf::from(aux)
}

/// Unique per test and per process, so the cases can run side by side.
struct TempFile {
    path: PathBuf,
}

impl TempFile {
    fn new(name: &str) -> Self {
        let file = Self {
            path: std::env::temp_dir().join(format!("katana-io-{}-{name}", std::process::id())),
        };
        file.remove();
        file
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn remove(&self) {
        let _ = fs::remove_file(&self.path);
        // Sidecars of the shapefile family, and the .prj an Esri ASCII grid
        // keeps its coordinate system in.
        for extension in ["shx", "dbf", "prj", "cpg"] {
            let _ = fs::remove_file(self.path.with_extension(extension));
        }
        // GDAL's auxiliary metadata, which is named by APPENDING to the whole
        // file name (dem.tif.aux.xml). One left behind by a run of an older
        // build would be trusted by the next run (audit IO-13), so it goes too.
        let _ = fs::remove_file(auxiliary_of(&self.path));
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        self.remove();
    }
}

fn point_at(x: f64, y: f64) -> GeoPoint {
    GeoPoint { x, y, z: 0.0 }
}

fn point_feature(x: f64, y: f64) -> VectorFeature {
    VectorFeature {
        geometry: VectorGeometry {
            kind: GeometryKind::Point,
            parts: vec![vec![point_at(x, y)]],
            ..VectorGeometry::default()
        },
        ..VectorFeature::default()
    }
}

#[link(name = "gdal")]
unsafe extern "C" {
    fn CPLGetConfigOption(key: *const c_char, default: *const c_char) -> *const c_char;
}

/// GDAL's configuration option, as GDAL holds it; None when it has none.
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
fn config_option(key: &CStr) -> Option<PathBuf> {
    // SAFETY: a NUL-terminated key and a NULL default.
    let value = unsafe { CPLGetConfigOption(key.as_ptr(), ptr::null()) };
    if value.is_null() {
        return None;
    }
    // SAFETY: GDAL's own NUL-terminated value, copied at once.
    let bytes = unsafe { CStr::from_ptr(value) }.to_bytes();
    Some(katana_core::path_text::path_from_utf8(bytes))
}

/// `std::filesystem::equivalent`: the same file.
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
fn equivalent(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (fs::metadata(a), fs::metadata(b)) {
            (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        match (fs::canonicalize(a), fs::canonicalize(b)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
    }
}

// A relocated Linux or macOS tree (docs/release.md) keeps GDAL's data in
// share/gdal and libcurl's certificates in ssl/cacert.pem beside the lib/ the
// libraries are in, while the paths compiled into them name the machine that
// built them. Registering GDAL points it at both, unless the environment
// already has. Where to look is worked out here from where the libraries
// were loaded, independently of the adapter.
#[cfg(target_os = "macos")]
const GDAL_LIBRARY: &str = "libgdal.";
#[cfg(target_os = "macos")]
const CURL_LIBRARY: &str = "libcurl.";
#[cfg(target_os = "linux")]
const GDAL_LIBRARY: &str = "libgdal.so";
#[cfg(target_os = "linux")]
const CURL_LIBRARY: &str = "libcurl.so";

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut std::ffi::c_void;
    fn GetModuleFileNameW(module: *mut std::ffi::c_void, name: *mut u16, size: u32) -> u32;
}

// EPSG:28356, GDA94 / MGA zone 56, as the EPSG registry publishes it in WKT1
// (epsg.io/28356.wkt): its name and code are the independent answer
// describe_crs has to find.
const MGA56_WKT: &str = concat!(
    r#"PROJCS["GDA94 / MGA zone 56",GEOGCS["GDA94",DATUM["Geocentric_Datum_of_Australia_1994","#,
    r#"SPHEROID["GRS 1980",6378137,298.257222101,AUTHORITY["EPSG","7019"]],"#,
    r#"TOWGS84[0,0,0,0,0,0,0],AUTHORITY["EPSG","6283"]],PRIMEM["Greenwich",0,"#,
    r#"AUTHORITY["EPSG","8901"]],UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],"#,
    r#"AUTHORITY["EPSG","4283"]],PROJECTION["Transverse_Mercator"],"#,
    r#"PARAMETER["latitude_of_origin",0],PARAMETER["central_meridian",153],"#,
    r#"PARAMETER["scale_factor",0.9996],PARAMETER["false_easting",500000],"#,
    r#"PARAMETER["false_northing",10000000],UNIT["metre",1,AUTHORITY["EPSG","9001"]],"#,
    r#"AXIS["Easting",EAST],AXIS["Northing",NORTH],AUTHORITY["EPSG","28356"]]"#,
);

/// A band whose value names its pixel: 10 * row + column. Every sample then
/// says which source pixel it came from, exactly, in any binary format.
fn named_pixels(width: i32, height: i32) -> Vec<f64> {
    let mut values = Vec::new();
    for row in 0..height {
        for column in 0..width {
            values.push(10.0 * f64::from(row) + f64::from(column));
        }
    }
    values
}

mod gdal_adapter {
    use super::*;

    #[test]
    fn gdal_is_pointed_at_the_data_beside_its_library() {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            if std::env::var_os("GDAL_DATA").is_some() {
                eprintln!("GDAL_DATA names GDAL's data already");
                return;
            }
            gdal_version(); // registers GDAL
            let Some(data) = katana_core::library_data::data_beside_library(
                GDAL_LIBRARY,
                Path::new("share/gdal"),
            ) else {
                eprintln!("a distribution's GDAL: no share/gdal beside its directory");
                return;
            };
            let given = config_option(c"GDAL_DATA").expect("GDAL was given GDAL_DATA");
            assert!(equivalent(&given, &data), "{}", given.display());
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        eprintln!("Windows finds GDAL's data by the DLL's name");
    }

    #[test]
    fn gdal_is_pointed_at_the_certificates_beside_libcurl() {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            if std::env::var_os("CURL_CA_BUNDLE").is_some()
                || std::env::var_os("SSL_CERT_FILE").is_some()
            {
                eprintln!("the environment names the certificates already");
                return;
            }
            gdal_version(); // registers GDAL
            let Some(certificates) = katana_core::library_data::data_beside_library(
                CURL_LIBRARY,
                Path::new("ssl/cacert.pem"),
            ) else {
                eprintln!("a distribution's libcurl: no ssl/cacert.pem beside its directory");
                return;
            };
            let given = config_option(c"CURL_CA_BUNDLE").expect("GDAL was given CURL_CA_BUNDLE");
            assert!(equivalent(&given, &certificates), "{}", given.display());
        }
        #[cfg(windows)]
        {
            // MSYS2's libcurl works out where its certificates are through the
            // ANSI code page, so an install under a folder with a letter
            // outside it looked for a file under a path of '?'s and every
            // https:// request failed. The adapter now hands libcurl the
            // path, wide, in UTF-8: etc/ssl/certs/ca-bundle.crt beside the
            // DLL's bin folder, wherever that folder is.
            use katana_core::path_text::environment_variable;
            use std::os::windows::ffi::OsStringExt;
            if !environment_variable(Some("CURL_CA_BUNDLE")).is_empty()
                || !environment_variable(Some("SSL_CERT_FILE")).is_empty()
            {
                eprintln!("the environment names the certificates already");
                return;
            }
            gdal_version(); // registers GDAL
            let name: Vec<u16> = "libcurl-4.dll".encode_utf16().chain([0]).collect();
            // SAFETY: a NUL-terminated wide name; the handle is not freed.
            let curl = unsafe { GetModuleHandleW(name.as_ptr()) };
            if curl.is_null() {
                eprintln!("libcurl is not MSYS2's libcurl-4.dll in this build");
                return;
            }
            let mut dll = vec![0u16; 1024];
            // SAFETY: a loaded module and a buffer of dll.len() wide characters.
            let length = unsafe { GetModuleFileNameW(curl, dll.as_mut_ptr(), dll.len() as u32) };
            dll.truncate(length as usize);
            let dll = PathBuf::from(std::ffi::OsString::from_wide(&dll));
            let Some(expected) = dll
                .parent()
                .and_then(Path::parent)
                .map(|root| {
                    root.join("etc")
                        .join("ssl")
                        .join("certs")
                        .join("ca-bundle.crt")
                })
                .filter(|expected| expected.is_file())
            else {
                eprintln!("no etc/ssl/certs/ca-bundle.crt beside libcurl's bin folder");
                return;
            };
            let given = config_option(c"CURL_CA_BUNDLE")
                .expect("GDAL was not told where the certificates are");
            // Given as UTF-8, and the file it names is the one the DLL's
            // folder holds.
            assert!(equivalent(&given, &expected), "{}", given.display());
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        eprintln!("no platform's rule for the certificates is written for this one");
    }

    // ---- GDAL raster ----------------------------------------------------------------------

    #[test]
    fn missing_file_is_not_found() {
        let error = refused(
            GdalDataset::open(Path::new("definitely-not-here.tif")),
            "a missing file",
        );
        assert_eq!(error.code, ErrorCode::NotFound);
    }

    #[test]
    fn writes_and_reads_raster() {
        let file = TempFile::new("raster.tif");
        let options = RasterExportOptions {
            width: 2,
            height: 2,
            geotransform: [100.0, 1.0, 0.0, 200.0, 0.0, -1.0],
            ..RasterExportOptions::default()
        };
        must(GdalDataset::write_raster(
            file.path(),
            &options,
            &[1.0, 2.0, 3.0, 4.0],
        ));

        let dataset = must(GdalDataset::open(file.path()));
        assert!(dataset.has_raster());

        let info = must(dataset.raster_info());
        assert_eq!(info.width, 2);
        assert_eq!(info.height, 2);
        assert!(info.has_geotransform);
        double_eq(info.geotransform[0], 100.0);
        double_eq(info.geotransform[5], -1.0);

        let values = must(dataset.read_band(1));
        assert_eq!(values.len(), 4);
        double_eq(values[2], 3.0);
    }

    #[test]
    fn band_index_out_of_range_is_rejected() {
        let file = TempFile::new("oneband.tif");
        let options = RasterExportOptions {
            width: 1,
            height: 1,
            ..RasterExportOptions::default()
        };
        must(GdalDataset::write_raster(file.path(), &options, &[1.0]));

        let dataset = must(GdalDataset::open(file.path()));
        for index in [0, 2, -1] {
            let error = refused(dataset.read_band(index), &format!("band {index}"));
            assert_eq!(error.code, ErrorCode::InvalidArgument);
        }
    }

    #[test]
    fn raster_export_checks_its_dimensions() {
        let file = TempFile::new("bad.tif");
        let options = RasterExportOptions {
            width: 3,
            height: 3,
            ..RasterExportOptions::default()
        };
        // Nine cells claimed, four supplied.
        let error = refused(
            GdalDataset::write_raster(file.path(), &options, &[1.0, 2.0, 3.0, 4.0]),
            "four values for nine cells",
        );
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn display_image_is_decimated_and_keeps_its_georeferencing() {
        let file = TempFile::new("big.tif");
        const SIZE: i32 = 64;
        let options = RasterExportOptions {
            width: SIZE,
            height: SIZE,
            geotransform: [1000.0, 2.0, 0.0, 5000.0, 0.0, -2.0],
            ..RasterExportOptions::default()
        };
        let values: Vec<f64> = (0..(SIZE * SIZE) as usize)
            .map(|i| (i % 256) as f64)
            .collect();
        must(GdalDataset::write_raster(file.path(), &options, &values));

        let dataset = must(GdalDataset::open(file.path()));

        let image = must(dataset.read_image(16));
        assert!(image.width <= 16);
        assert!(image.height <= 16);
        assert_eq!(
            image.rgba.len(),
            image.width as usize * image.height as usize * 4
        );

        // The geotransform must be rescaled for the decimated grid, or the
        // overlay would be drawn at a quarter of its true size. The image
        // still has to span the same ground: origin unchanged, and width *
        // pixel size preserved.
        double_eq(image.geotransform[0], 1000.0);
        double_eq(image.geotransform[3], 5000.0);
        double_eq(
            f64::from(image.width) * image.geotransform[1],
            f64::from(SIZE) * 2.0,
        );
        double_eq(
            f64::from(image.height) * image.geotransform[5],
            f64::from(SIZE) * -2.0,
        );

        let full = must(dataset.read_image(4096));
        assert_eq!(
            full.width, SIZE,
            "a raster below the budget is not decimated at all"
        );
        double_eq(full.geotransform[1], 2.0);
    }

    #[test]
    fn read_image_rejects_a_nonsense_pixel_budget() {
        let file = TempFile::new("tiny.tif");
        let options = RasterExportOptions {
            width: 1,
            height: 1,
            ..RasterExportOptions::default()
        };
        must(GdalDataset::write_raster(file.path(), &options, &[1.0]));
        let dataset = must(GdalDataset::open(file.path()));

        let error = refused(dataset.read_image(0), "a budget of no pixels");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn raster_info_reports_the_bands_no_data_value_only_when_one_is_declared() {
        let with = TempFile::new("nodata.tif");
        let without = TempFile::new("no-nodata.tif");
        let mut options = RasterExportOptions {
            width: 2,
            height: 1,
            ..RasterExportOptions::default()
        };
        must(GdalDataset::write_raster(
            without.path(),
            &options,
            &[1.0, 2.0],
        ));
        options.no_data_value = Some(-9999.0);
        must(GdalDataset::write_raster(
            with.path(),
            &options,
            &[1.0, -9999.0],
        ));

        let declared = must(GdalDataset::open(with.path()));
        let declared_info = must(declared.raster_info());
        assert_eq!(declared_info.no_data_value, Some(-9999.0));

        // Absent is not a value: a DEM without a no-data value has every
        // pixel meaningful, and zero is a real height.
        let undeclared = must(GdalDataset::open(without.path()));
        let undeclared_info = must(undeclared.raster_info());
        assert!(undeclared_info.no_data_value.is_none());
    }

    #[test]
    fn the_driver_is_reported_by_its_short_name() {
        let raster = TempFile::new("driver.tif");
        let vector = TempFile::new("driver.geojson");
        let options = RasterExportOptions {
            width: 1,
            height: 1,
            ..RasterExportOptions::default()
        };
        must(GdalDataset::write_raster(raster.path(), &options, &[1.0]));
        must(GdalDataset::write_vector(
            vector.path(),
            &[point_feature(1.0, 2.0)],
            &VectorExportOptions::default(),
        ));

        assert_eq!(
            must(GdalDataset::open(raster.path())).driver_name(),
            "GTiff"
        );
        assert_eq!(
            must(GdalDataset::open(vector.path())).driver_name(),
            "GeoJSON"
        );
    }

    #[test]
    fn a_sampled_read_keeps_exact_source_pixels_on_the_stride() {
        let file = TempFile::new("sampled.tif");
        let options = RasterExportOptions {
            width: 7,
            height: 5,
            ..RasterExportOptions::default()
        };
        must(GdalDataset::write_raster(
            file.path(),
            &options,
            &named_pixels(7, 5),
        ));
        let dataset = must(GdalDataset::open(file.path()));

        // Stride 3 keeps columns 0, 3, 6 and rows 0, 3: ceil(7/3) = 3 by
        // ceil(5/3) = 2, and each value is its source pixel's, not an average.
        let every_third = must(dataset.read_band_sampled(1, 3));
        assert_eq!(every_third.columns, 3);
        assert_eq!(every_third.rows, 2);
        assert_eq!(every_third.stride, 3);
        assert_eq!(every_third.values, [0.0, 3.0, 6.0, 30.0, 33.0, 36.0]);
        assert!(every_third.no_data_value.is_none());

        // Stride 1 is the whole band, identical to read_band.
        let all = must(dataset.read_band_sampled(1, 1));
        assert_eq!(all.columns, 7);
        assert_eq!(all.rows, 5);
        assert_eq!(all.values, must(dataset.read_band(1)));

        // A stride at or beyond both sides keeps the first pixel alone.
        for stride in [7, 1000] {
            let one = must(dataset.read_band_sampled(1, stride));
            assert_eq!(one.columns, 1);
            assert_eq!(one.rows, 1);
            assert_eq!(one.values, [0.0]);
        }
    }

    #[test]
    fn a_sampled_read_refuses_a_bad_band_or_stride_and_reports_no_data() {
        let file = TempFile::new("sampled-bad.tif");
        let options = RasterExportOptions {
            width: 2,
            height: 2,
            no_data_value: Some(-32768.0),
            ..RasterExportOptions::default()
        };
        must(GdalDataset::write_raster(
            file.path(),
            &options,
            &[1.0, -32768.0, 3.0, 4.0],
        ));
        let dataset = must(GdalDataset::open(file.path()));

        for band in [0, 2, -1] {
            let error = refused(dataset.read_band_sampled(band, 1), &format!("band {band}"));
            assert_eq!(error.code, ErrorCode::InvalidArgument);
        }
        for stride in [0, -3] {
            let error = refused(
                dataset.read_band_sampled(1, stride),
                &format!("stride {stride}"),
            );
            assert_eq!(error.code, ErrorCode::InvalidArgument);
        }

        // The sentinel is delivered as the file holds it, with the band's
        // value beside it: deciding what no-data means is the caller's
        // business.
        let samples = must(dataset.read_band_sampled(1, 1));
        assert_eq!(samples.no_data_value, Some(-32768.0));
        assert_eq!(samples.values, [1.0, -32768.0, 3.0, 4.0]);
    }

    #[test]
    fn raster_driver_is_chosen_by_extension_and_unknown_ones_are_named() {
        let driver = |name: &str| GdalDataset::raster_driver_for_path(Path::new(name));
        assert_eq!(must(driver("dem.tif")), "GTiff");
        assert_eq!(
            must(driver("dem.TIFF")),
            "GTiff",
            "extension matching must be case insensitive"
        );
        assert_eq!(must(driver("dem.asc")), "AAIGrid");
        assert_eq!(must(driver("dem.img")), "HFA");

        let unknown = refused(driver("dem.xyzzy"), "an unknown extension");
        assert_eq!(unknown.code, ErrorCode::Unsupported);
        assert!(unknown.message.contains("xyzzy"), "{}", unknown.message);

        let none = refused(driver("dem"), "no extension");
        assert_eq!(none.code, ErrorCode::Unsupported);
    }

    #[test]
    fn a_coordinate_system_is_described_by_its_name_and_authority_code() {
        assert_eq!(describe_crs(MGA56_WKT), "GDA94 / MGA zone 56 (EPSG:28356)");

        // No AUTHORITY node anywhere, as an Esri .prj is written, yet each is
        // an EPSG system by its parameters, and GDAL can say which. The codes
        // are the EPSG registry's: 4326 is WGS 84 geographic (a = 6378137,
        // 1/f = 298.257223563); 28356 is GDA94 / MGA zone 56 (GRS 1980,
        // transverse Mercator, central meridian 153, k 0.9996, false origin
        // 500 000 m E, 10 000 000 m N). `gdalsrsinfo -e` names the same codes.
        const BARE_WGS84: &str = concat!(
            r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]],"#,
            r#"PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433]]"#,
        );
        assert_eq!(describe_crs(BARE_WGS84), "WGS 84 (EPSG:4326)");
        const ESRI_MGA56: &str = concat!(
            r#"PROJCS["GDA_1994_MGA_Zone_56",GEOGCS["GCS_GDA_1994",DATUM["D_GDA_1994","#,
            r#"SPHEROID["GRS_1980",6378137.0,298.257222101]],PRIMEM["Greenwich",0.0],"#,
            r#"UNIT["Degree",0.0174532925199433]],PROJECTION["Transverse_Mercator"],"#,
            r#"PARAMETER["False_Easting",500000.0],PARAMETER["False_Northing",10000000.0],"#,
            r#"PARAMETER["Central_Meridian",153.0],PARAMETER["Scale_Factor",0.9996],"#,
            r#"PARAMETER["Latitude_Of_Origin",0.0],UNIT["Meter",1.0]]"#,
        );
        let esri = describe_crs(ESRI_MGA56);
        assert!(esri.contains("(EPSG:28356)"), "{esri}");

        // A local site grid is no EPSG system: its name, and no invented code.
        assert_eq!(
            describe_crs(r#"LOCAL_CS["site grid",UNIT["metre",1]]"#),
            "site grid"
        );

        // Nothing declared is nothing; something unreadable is NOT nothing.
        assert_eq!(describe_crs(""), "");
        let unreadable = describe_crs("this is not a coordinate system");
        assert!(!unreadable.is_empty());
        assert!(unreadable.contains("unrecognised"), "{unreadable}");
    }

    #[test]
    fn a_format_that_can_only_copy_is_written_through_an_in_memory_dataset() {
        // AAIGrid has no Create - GDAL can only copy a finished dataset into
        // it. Values exact in binary and at Float32 (AAIGrid reads decimals as
        // Float32 by default), so they must come back exactly.
        let file = TempFile::new("copied.asc");
        let options = RasterExportOptions {
            driver: String::from("AAIGrid"),
            width: 3,
            height: 2,
            geotransform: [1000.0, 2.0, 0.0, 2000.0, 0.0, -2.0],
            no_data_value: Some(-9999.0),
            projection_wkt: MGA56_WKT.to_owned(),
        };
        must(GdalDataset::write_raster(
            file.path(),
            &options,
            &[10.5, 11.25, -9999.0, 12.125, 13.0, 14.75],
        ));
        // The grid and its .prj, and nothing else: GDAL copying from a
        // GeoTIFF leaves a .asc.aux.xml of metadata the format cannot hold,
        // but the in-memory dataset has none to leave.
        assert!(!auxiliary_of(file.path()).exists());

        let dataset = must(GdalDataset::open(file.path()));
        assert_eq!(dataset.driver_name(), "AAIGrid");
        let info = must(dataset.raster_info());
        assert_eq!(info.width, 3);
        assert_eq!(info.height, 2);
        assert!(info.has_geotransform);
        assert_eq!(info.geotransform, [1000.0, 2.0, 0.0, 2000.0, 0.0, -2.0]);
        assert_eq!(info.no_data_value, Some(-9999.0));
        assert!(
            describe_crs(&info.projection_wkt).contains("MGA zone 56"),
            "the .prj beside the grid carries the coordinate system"
        );
        assert_eq!(
            must(dataset.read_band(1)),
            [10.5, 11.25, -9999.0, 12.125, 13.0, 14.75]
        );
    }

    #[test]
    fn an_erdas_imagine_raster_is_written_with_its_georeferencing() {
        let file = TempFile::new("created.img");
        let options = RasterExportOptions {
            driver: String::from("HFA"),
            width: 2,
            height: 2,
            geotransform: [500.0, 0.5, 0.0, 800.0, 0.0, -0.5],
            ..RasterExportOptions::default()
        };
        must(GdalDataset::write_raster(
            file.path(),
            &options,
            &[1.5, 2.5, 3.5, 4.5],
        ));
        let dataset = must(GdalDataset::open(file.path()));
        assert_eq!(dataset.driver_name(), "HFA");
        let info = must(dataset.raster_info());
        assert_eq!(info.geotransform, [500.0, 0.5, 0.0, 800.0, 0.0, -0.5]);
        assert_eq!(must(dataset.read_band(1)), [1.5, 2.5, 3.5, 4.5]);
    }

    #[test]
    fn a_raster_whose_coordinate_system_cannot_be_set_is_refused_and_not_left_behind() {
        // Regression, audit IO-14: the georeferencing setters' answers were
        // ignored, so this wrote a GeoTIFF with no coordinate system and
        // reported success.
        let file = TempFile::new("bad-crs.tif");
        let mut options = RasterExportOptions {
            width: 1,
            height: 1,
            projection_wkt: String::from("this is not a coordinate system"),
            ..RasterExportOptions::default()
        };
        let error = refused(
            GdalDataset::write_raster(file.path(), &options, &[1.0]),
            "an unreadable coordinate system",
        );
        assert_eq!(error.code, ErrorCode::FileExportFailure);
        assert!(
            !file.path().exists(),
            "a raster that failed to write must not be left looking finished"
        );

        // The in-memory route (a copy-only format) refuses it the same way.
        let copied = TempFile::new("bad-crs.asc");
        options.driver = String::from("AAIGrid");
        let via_copy = refused(
            GdalDataset::write_raster(copied.path(), &options, &[1.0]),
            "an unreadable coordinate system, copied",
        );
        assert_eq!(via_copy.code, ErrorCode::FileExportFailure);
        assert!(!copied.path().exists());
    }

    #[test]
    fn a_vector_whose_coordinate_system_cannot_be_read_is_refused_before_writing() {
        // The WKT used to be dropped when GDAL could not parse it: the file
        // was written with no CRS and the export reported success.
        let file = TempFile::new("bad-crs.gpkg");
        let options = VectorExportOptions {
            projection_wkt: String::from("this is not a coordinate system"),
            ..VectorExportOptions::default()
        };
        let error = refused(
            GdalDataset::write_vector(file.path(), &[point_feature(1.0, 2.0)], &options),
            "an unreadable coordinate system",
        );
        assert_eq!(error.code, ErrorCode::InvalidCrs);
        assert!(!file.path().exists());
    }

    #[test]
    fn displaying_a_raster_writes_nothing_beside_the_users_file() {
        // Regression, audit IO-13: read_image forced GDAL's band statistics,
        // and GDAL persisted them to <file>.aux.xml on close - into the
        // user's data folder, on a read-only open.
        let file = TempFile::new("no-sidecar.tif");
        let options = RasterExportOptions {
            width: 4,
            height: 4,
            geotransform: [100.0, 1.0, 0.0, 200.0, 0.0, -1.0],
            ..RasterExportOptions::default()
        };
        let values: Vec<f64> = (0..16).map(|i| 300.0 + f64::from(i)).collect();
        must(GdalDataset::write_raster(file.path(), &options, &values));
        assert!(!auxiliary_of(file.path()).exists());
        {
            let dataset = must(GdalDataset::open(file.path()));
            must(dataset.read_image(16));
            must(dataset.read_band_sampled(1, 2));
        } // closed here, which is when GDAL would write it
        assert!(
            !auxiliary_of(file.path()).exists(),
            "{} was written",
            auxiliary_of(file.path()).display()
        );
    }

    #[test]
    fn a_replaced_dem_is_stretched_over_its_own_values() {
        // Regression, audit IO-13: the persisted statistics were trusted on
        // the next open, so a DEM replaced under the same name was stretched
        // by the OLD one's range. 300 and 400 against a stale 0-255 both
        // clamp to white.
        let file = TempFile::new("replaced.tif");
        let replacement = TempFile::new("replacement.tif");
        let options = RasterExportOptions {
            width: 2,
            height: 1,
            ..RasterExportOptions::default()
        };
        must(GdalDataset::write_raster(
            file.path(),
            &options,
            &[0.0, 255.0],
        ));
        {
            let first = must(GdalDataset::open(file.path()));
            must(first.read_image(16));
        }
        // Replaced the way a person replaces a file - copied over it - and NOT
        // through GDAL, whose Create deletes a dataset's sidecars along with
        // it and so would hide the defect.
        must(GdalDataset::write_raster(
            replacement.path(),
            &options,
            &[300.0, 400.0],
        ));
        fs::copy(replacement.path(), file.path()).expect("the replacement is copied over the file");
        let second = must(GdalDataset::open(file.path()));
        let image = must(second.read_image(16));
        assert_eq!(image.rgba.len(), 8);
        // Its own range is 300 to 400: the lower pixel is black, the upper
        // white.
        assert_eq!(
            image.rgba[0], 0,
            "stretched by a range that is not this file's"
        );
        assert_eq!(image.rgba[4], 255);
    }

    // ---- GDAL vector ----------------------------------------------------------------------

    #[test]
    fn writes_and_reads_vector_features() {
        let file = TempFile::new("points.geojson");
        let mut point = point_feature(1.0, 2.0);
        point
            .attributes
            .insert(String::from("name"), String::from("origin"));

        let options = VectorExportOptions {
            layer_name: String::from("points"),
            ..VectorExportOptions::default()
        };
        must(GdalDataset::write_vector(file.path(), &[point], &options));

        let dataset = must(GdalDataset::open(file.path()));
        assert!(dataset.has_vector());

        let layers = must(dataset.vector_layers());
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].feature_count, 1);

        // Reading the GEOMETRY back, not merely the layer's metadata: the
        // adapter previously reported layer names and nothing else, which
        // made importing vector data impossible.
        let features = must(dataset.read_features(0, 0));
        assert_eq!(features.len(), 1);
        assert_eq!(features[0].geometry.kind, GeometryKind::Point);
        assert_eq!(features[0].geometry.parts.len(), 1);
        assert_eq!(features[0].geometry.parts[0].len(), 1);
        double_eq(features[0].geometry.parts[0][0].x, 1.0);

        let name = features[0]
            .attributes
            .get("name")
            .expect("the attribute survives");
        assert_eq!(name, "origin");
    }

    #[test]
    fn polygon_rings_round_trip_with_their_holes() {
        let file = TempFile::new("rings.geojson");
        let polygon = VectorFeature {
            geometry: VectorGeometry {
                kind: GeometryKind::Polygon,
                parts: vec![
                    vec![
                        point_at(0.0, 0.0),
                        point_at(10.0, 0.0),
                        point_at(10.0, 10.0),
                        point_at(0.0, 10.0),
                    ],
                    // A hole.
                    vec![
                        point_at(3.0, 3.0),
                        point_at(7.0, 3.0),
                        point_at(7.0, 7.0),
                        point_at(3.0, 7.0),
                    ],
                ],
                ..VectorGeometry::default()
            },
            ..VectorFeature::default()
        };

        must(GdalDataset::write_vector(
            file.path(),
            &[polygon],
            &VectorExportOptions::default(),
        ));

        let dataset = must(GdalDataset::open(file.path()));
        let features = must(dataset.read_features(0, 0));
        assert_eq!(features.len(), 1);
        assert_eq!(features[0].geometry.kind, GeometryKind::Polygon);
        assert_eq!(features[0].geometry.parts.len(), 2, "the hole must survive");
        // Rings come back closed, so the first point repeats at the end.
        assert_eq!(features[0].geometry.parts[0].len(), 5);
    }

    #[test]
    fn layer_index_out_of_range_is_rejected() {
        let file = TempFile::new("one.geojson");
        must(GdalDataset::write_vector(
            file.path(),
            &[point_feature(0.0, 0.0)],
            &VectorExportOptions::default(),
        ));

        let dataset = must(GdalDataset::open(file.path()));
        for index in [-1, 1, 99] {
            let error = refused(dataset.read_features(index, 0), &format!("layer {index}"));
            assert_eq!(error.code, ErrorCode::InvalidArgument);
        }
    }

    #[test]
    fn driver_is_chosen_by_extension_and_unknown_ones_are_named() {
        let driver = |name: &str| GdalDataset::vector_driver_for_path(Path::new(name));
        assert_eq!(must(driver("a.shp")), "ESRI Shapefile");

        let geojson = driver("a.GeoJSON");
        assert!(
            geojson.is_ok(),
            "extension matching must be case insensitive"
        );
        assert_eq!(must(geojson), "GeoJSON");

        let unknown = refused(driver("a.xyzzy"), "an unknown extension");
        assert_eq!(unknown.code, ErrorCode::Unsupported);
        assert!(unknown.message.contains("xyzzy"), "{}", unknown.message);

        let none = refused(driver("a"), "no extension");
        assert_eq!(none.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn format_capabilities_are_reported() {
        // A shapefile fixes its shape type in the header; GeoPackage and
        // GeoJSON do not. The exporter relies on this to refuse a mixed
        // collection up front rather than failing halfway through writing one.
        assert!(driver_holds_one_geometry_type("ESRI Shapefile"));
        assert!(!driver_holds_one_geometry_type("GPKG"));
        assert!(!driver_holds_one_geometry_type("GeoJSON"));

        // RFC 7946 pins GeoJSON to WGS 84, so coordinates in any other system
        // are silently relabelled by it and the user has to be warned.
        assert!(driver_assumes_wgs84("GeoJSON"));
        assert!(!driver_assumes_wgs84("GPKG"));
    }

    #[test]
    fn several_datasets_can_be_open_at_once() {
        // Regression. The destructor used to call GDALDestroyDriverManager(),
        // which tears down state shared by the whole process: closing one
        // dataset broke every other open dataset and every subsequent open.
        let first = TempFile::new("a.geojson");
        let second = TempFile::new("b.geojson");
        let point = point_feature(1.0, 1.0);
        must(GdalDataset::write_vector(
            first.path(),
            std::slice::from_ref(&point),
            &VectorExportOptions::default(),
        ));
        must(GdalDataset::write_vector(
            second.path(),
            std::slice::from_ref(&point),
            &VectorExportOptions::default(),
        ));

        let a = must(GdalDataset::open(first.path()));
        {
            let _b = must(GdalDataset::open(second.path()));
        } // b closes here

        // a must still work...
        if let Err(error) = a.read_features(0, 0) {
            panic!("closing one dataset broke another: {}", error.describe());
        }

        // ...and opening another afterwards must still work.
        if let Err(error) = GdalDataset::open(second.path()) {
            panic!("the driver manager was destroyed: {}", error.describe());
        }
    }

    #[test]
    fn versions_are_reported() {
        assert!(!gdal_version().is_empty());
        assert!(!katana_pointcloud::pdal_version().is_empty());
    }
}
