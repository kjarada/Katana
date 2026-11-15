//! tests/interop/test_terrain_io.cpp: terrain to and from GDAL and PDAL data -
//! the elevations of a DEM, the ground of a point cloud, and a surface
//! written out as a DEM.
//!
//! Real files through GDAL and PDAL, as everywhere in this suite: the
//! formats' own rules (a pixel's centre, an ASCII grid read back as Float32,
//! a no-data value in a header) are exactly what needs checking. Every
//! expected value is worked out by hand from inputs exact in binary.

use std::path::Path;

use katana_core::ErrorCode;
use katana_geometry::{Box2, Point2, Point3};
use katana_gis::gdal_adapter::{
    GdalDataset, RasterExportOptions, RasterInfo, RasterSamples, describe_crs,
};
use katana_interop::export::{
    export_point_cloud, point_cloud_export_formats, raster_export_formats,
};
use katana_interop::import::import_point_cloud;
use katana_interop::reference_data::PointCloudLayer;
use katana_interop::terrain_io::{
    RasterElevationOptions, SurfaceRasterOptions, export_surface_raster, read_raster_elevations,
    stride_for_cap, suggested_cell_size, surface_points,
};
use katana_pointcloud::PointCloudPoint;
use katana_terrain::{TinBuildOptions, TinInput, TinSurface, build_tin};

use crate::{TempDir, must, near, refused, write_file};

fn temp_dir(name: &str) -> TempDir {
    TempDir::new("katana-terrainio-", name)
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

// ---- stride_for_cap (audit QT-24) -------------------------------------------------------------

mod terrain_io_stride {
    use super::*;

    #[test]
    fn the_stride_is_the_smallest_that_meets_the_cap() {
        // The audit's case: 1200 x 999 = 1 198 800 pixels against 400 000.
        // The GUI's floor(sqrt(ceil(1 198 800 / 400 000))) = floor(sqrt 3) = 1
        // kept every pixel. Stride 2 keeps 600 x 500 = 300 000, which is
        // under the cap.
        assert_eq!(stride_for_cap(1200, 999, 400_000), 2);
        // Its other case: 2000 x 2000 = 4 M. Stride 3 keeps 667 x 667 =
        // 444 889, over the cap (the GUI's floor(sqrt 10) = 3 gave exactly
        // that); stride 4 keeps 500 x 500 = 250 000.
        assert_eq!(stride_for_cap(2000, 2000, 400_000), 4);

        // Exactly at the cap is within it; one under is not.
        assert_eq!(stride_for_cap(100, 100, 10_000), 1);
        assert_eq!(stride_for_cap(100, 100, 9_999), 2); // 50 x 50 = 2 500

        // A strip, where the ceilings put the answer far above
        // sqrt(w*h / cap): ceil(10^9 / 10^8) = 10 meets a cap of 10,
        // ceil(10^9 / (10^8 - 1)) = 11 does not.
        assert_eq!(stride_for_cap(1, 1_000_000_000, 10), 100_000_000);

        // The largest grid GDAL can describe, down to one sample: only the
        // whole side as the stride leaves one, since INT_MAX - 1 leaves 2 x 2.
        let largest = i32::MAX;
        assert_eq!(stride_for_cap(largest, largest, 1), largest);

        // Nothing to sample, and a cap nothing meets (callers refuse it
        // first).
        assert_eq!(stride_for_cap(0, 5, 10), 1);
        assert_eq!(stride_for_cap(5, 0, 10), 1);
        assert_eq!(stride_for_cap(30, 7, 0), 30);
    }

    #[test]
    fn the_stride_agrees_with_a_brute_force_search_over_every_small_grid() {
        let samples =
            |w: i32, h: i32, s: i32| -> u64 { ((w + s - 1) / s) as u64 * ((h + s - 1) / s) as u64 };
        let mut cases: u64 = 0;
        let mut old_formula_over_cap: u64 = 0;
        for w in 1..=30 {
            for h in 1..=30 {
                for cap in 1..=(w * h) as u64 + 1 {
                    let mut expected = 1;
                    while samples(w, h, expected) > cap {
                        expected += 1;
                    }
                    assert_eq!(
                        stride_for_cap(w, h, cap as usize),
                        expected,
                        "{w} x {h} capped at {cap}"
                    );
                    cases += 1;

                    // The formula the GUI used (audit QT-24), to show this
                    // sweep reaches the cases that broke it rather than only
                    // easy ones.
                    let pixels = (w * h) as u64;
                    let thinning = pixels.div_ceil(cap);
                    let old = 1.max((thinning as f64).sqrt().floor() as i32);
                    if samples(w, h, old) > cap {
                        old_formula_over_cap += 1;
                    }
                }
            }
        }
        // sum over w, h of (w h + 1) = (1 + ... + 30)^2 + 900 = 465^2 + 900.
        assert_eq!(cases, 217_125);
        // A sweep that never found the old formula over the cap would prove
        // nothing about the fix. Measured (2026-09-23, and by an independent
        // Python replay of the same loops): 199 085 of the 217 125 cases.
        assert!(old_formula_over_cap > 100_000, "of {cases}");
    }
}

// ---- read_raster_elevations (audit QT-23) ---------------------------------------------------

const DEM_WIDTH: i32 = 10;
const DEM_HEIGHT: i32 = 9;
const DEM_NO_DATA: f64 = -9999.0;

/// Every value a multiple of 0.25, exact in binary: v(i, j) = 10.5 + 0.75 i
/// + 0.25 j.
fn dem_value(column: i32, row: i32) -> f64 {
    10.5 + 0.75 * f64::from(column) + 0.25 * f64::from(row)
}

fn is_no_data_pixel(column: i32, row: i32) -> bool {
    column == 3 && row == 3
}

/// A 10 x 9 GeoTIFF whose top-left corner is (1000, 2000) with 2 m pixels,
/// holding dem_value except at pixel (3, 3), which is no-data.
fn write_dem(path: &Path) {
    let options = RasterExportOptions {
        width: DEM_WIDTH,
        height: DEM_HEIGHT,
        geotransform: [1000.0, 2.0, 0.0, 2000.0, 0.0, -2.0],
        no_data_value: Some(DEM_NO_DATA),
        projection_wkt: String::from(MGA56_WKT),
        ..RasterExportOptions::default()
    };
    let mut values = Vec::new();
    for row in 0..DEM_HEIGHT {
        for column in 0..DEM_WIDTH {
            values.push(if is_no_data_pixel(column, row) {
                DEM_NO_DATA
            } else {
                dem_value(column, row)
            });
        }
    }
    must(GdalDataset::write_raster(path, &options, &values));
}

/// The centre of pixel (i, j): 1000 + 2 (i + 0.5) east, 2000 - 2 (j + 0.5)
/// north.
fn dem_point(column: i32, row: i32) -> Point3 {
    Point3::new(
        1001.0 + 2.0 * f64::from(column),
        1999.0 - 2.0 * f64::from(row),
        dem_value(column, row),
    )
}

mod terrain_io_dem {
    use super::*;

    #[test]
    fn elevations_are_pixel_centres_carrying_the_bands_own_values() {
        let dir = temp_dir("dem-centres");
        let path = dir.file("dem.tif");
        write_dem(&path);

        let read = must(read_raster_elevations(
            &path,
            &RasterElevationOptions::default(),
        ));
        assert_eq!(read.stride, 1);
        assert_eq!(read.sampled, 90);
        assert_eq!(read.no_data, 1);
        assert!(describe_crs(&read.projection_wkt).contains("EPSG:28356"));

        // Every point exactly: its position is the pixel CENTRE through the
        // file's own geotransform, and its height is the band value
        // unchanged - not an 8-bit grey rescaled between two typed numbers
        // (audit QT-23).
        let mut expected = Vec::new();
        for row in 0..DEM_HEIGHT {
            for column in 0..DEM_WIDTH {
                if !is_no_data_pixel(column, row) {
                    expected.push(dem_point(column, row));
                }
            }
        }
        assert_eq!(read.points.len(), expected.len());
        for (i, (got, want)) in read.points.iter().zip(&expected).enumerate() {
            assert_eq!(got.x, want.x, "point {i}");
            assert_eq!(got.y, want.y, "point {i}");
            assert_eq!(got.z, want.z, "point {i}");
        }
    }

    #[test]
    fn a_cap_is_met_by_a_stride_across_the_whole_extent_not_by_truncation() {
        let dir = temp_dir("dem-cap");
        let path = dir.file("dem.tif");
        write_dem(&path);

        // 90 pixels capped at 20: stride 2 keeps 5 x 5 = 25, stride 3 keeps
        // ceil(10/3) x ceil(9/3) = 4 x 3 = 12 - columns 0, 3, 6, 9 and rows 0,
        // 3, 6, of which (3, 3) is the no-data pixel.
        let options = RasterElevationOptions {
            max_points: 20,
            ..RasterElevationOptions::default()
        };
        let read = must(read_raster_elevations(&path, &options));
        assert_eq!(read.stride, 3);
        assert_eq!(read.sampled, 12);
        assert_eq!(read.no_data, 1);
        assert_eq!(read.points.len(), 11);
        assert!(read.points.len() <= options.max_points);

        let mut expected = Vec::new();
        for row in [0, 3, 6] {
            for column in [0, 3, 6, 9] {
                if !is_no_data_pixel(column, row) {
                    expected.push(dem_point(column, row));
                }
            }
        }
        for (i, (got, want)) in read.points.iter().zip(&expected).enumerate() {
            assert_eq!(got.x, want.x, "point {i}");
            assert_eq!(got.y, want.y, "point {i}");
            assert_eq!(got.z, want.z, "point {i}");
        }
        // The far corner is there: pixel (9, 6), within one stride of the
        // last row and on the last column - not the first 20 pixels of the
        // file.
        let last = read.points.last().expect("a last point");
        assert_eq!(last.x, 1019.0);
        assert_eq!(last.y, 1987.0);
        assert_eq!(last.z, 18.75);
    }

    #[test]
    fn what_cannot_be_placed_on_the_ground_is_refused() {
        let dir = temp_dir("dem-refused");
        let dem = dir.file("dem.tif");
        write_dem(&dem);

        let read =
            |path: &Path, options: &RasterElevationOptions| read_raster_elevations(path, options);
        let defaults = RasterElevationOptions::default();
        assert_eq!(
            refused(read(&dir.file("missing.tif"), &defaults), "a missing DEM").code,
            ErrorCode::NotFound
        );

        let zero = RasterElevationOptions {
            max_points: 0,
            ..RasterElevationOptions::default()
        };
        assert_eq!(
            refused(read(&dem, &zero), "a zero cap").code,
            ErrorCode::InvalidArgument
        );

        for band in [0, 2] {
            let options = RasterElevationOptions {
                band,
                ..RasterElevationOptions::default()
            };
            assert_eq!(
                refused(read(&dem, &options), "a band out of range").code,
                ErrorCode::InvalidArgument,
                "band {band}"
            );
        }

        // A raster with no georeferencing: its pixels would land at (0, 0)
        // one unit each, a surface in the wrong place. Written by hand as a
        // VRT with no <GeoTransform> - GTiff records even GDAL's default
        // transform as real georeferencing, so write_raster cannot make this
        // fixture.
        let plain = dir.file("plain.vrt");
        write_file(
            &plain,
            r#"<VRTDataset rasterXSize="4" rasterYSize="3"><VRTRasterBand dataType="Float64" band="1"/></VRTDataset>"#,
        );
        assert!(
            !must(must(GdalDataset::open(&plain)).raster_info()).has_geotransform,
            "the fixture must really be without georeferencing"
        );
        let error = refused(read(&plain, &defaults), "a raster without georeferencing");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(error.message.contains("georeferenced"), "{}", error.message);

        // A file GDAL cannot read at all.
        let broken = dir.file("broken.tif");
        write_file(&broken, "not a raster");
        assert_eq!(
            refused(read(&broken, &defaults), "a broken raster").code,
            ErrorCode::FileImportFailure
        );
    }
}

// ---- surface_points (audit QT-10) -----------------------------------------------------------

fn cloud_point(x: f64, y: f64, z: f64, classification: u8) -> PointCloudPoint {
    PointCloudPoint {
        x,
        y,
        z,
        classification,
        ..PointCloudPoint::default()
    }
}

mod terrain_io_cloud {
    use super::*;

    #[test]
    fn a_classified_cloud_gives_its_ground_returns_alone() {
        // ASPRS LAS 1.4 R15 table 17: 2 ground, 5 high vegetation, 6
        // building, 7 low noise. A canopy or a roof in a ground surface is
        // what QT-10 found.
        let cloud = PointCloudLayer {
            points: vec![
                cloud_point(0.0, 0.0, 10.0, 2),
                cloud_point(1.0, 0.0, 25.0, 5),
                cloud_point(2.0, 0.0, 11.0, 2),
                cloud_point(3.0, 0.0, 18.0, 6),
                cloud_point(4.0, 0.0, -40.0, 7),
            ],
            ..PointCloudLayer::default()
        };
        let surface = surface_points(&cloud);
        assert!(surface.ground_only);
        assert_eq!(surface.excluded, 3);
        assert_eq!(surface.points.len(), 2);
        assert_eq!(surface.points[0], Point3::new(0.0, 0.0, 10.0));
        assert_eq!(surface.points[1], Point3::new(2.0, 0.0, 11.0));
    }

    #[test]
    fn an_unclassified_cloud_gives_every_return_and_says_so() {
        // Photogrammetry and terrestrial scans carry no classes (0 never
        // classified, 1 unclassified). Refusing them would leave them no way
        // to a surface; taking every return is right, provided the caller is
        // TOLD.
        let cloud = PointCloudLayer {
            points: vec![
                cloud_point(0.0, 0.0, 10.0, 0),
                cloud_point(1.0, 0.0, 25.0, 1),
                cloud_point(2.0, 0.0, 11.0, 1),
            ],
            ..PointCloudLayer::default()
        };
        let surface = surface_points(&cloud);
        assert!(!surface.ground_only);
        assert_eq!(surface.excluded, 0);
        assert_eq!(surface.points.len(), 3);
        assert_eq!(surface.points[1], Point3::new(1.0, 0.0, 25.0));
    }

    #[test]
    fn an_empty_cloud_gives_nothing_and_claims_nothing() {
        let surface = surface_points(&PointCloudLayer::default());
        assert!(surface.points.is_empty());
        assert!(
            !surface.ground_only,
            "no ground was found, so none can be claimed"
        );
        assert_eq!(surface.excluded, 0);
    }
}

// ---- export_surface_raster ------------------------------------------------------------------

/// The plane z = 0.5 x + 0.25 y + 3. Linear interpolation reproduces a plane
/// exactly, so every cell of the exported grid has a value worked out by
/// hand, and at the half-integer cell centres used here it is a multiple of
/// 1/8 - exact in Float64 and in Float32 alike.
fn plane(x: f64, y: f64) -> f64 {
    0.5 * x + 0.25 * y + 3.0
}

/// The TIN's interpolation error. At these small integer vertices the edge
/// functions and their sum are exact, so each barycentric weight is one
/// correctly rounded division; the value is then three products and two
/// sums. Per operation the relative error is at most u = DBL_EPSILON / 2,
/// which bounds the whole by about 4u x max|z| = 2 DBL_EPSILON x max|z|. The
/// plane peaks at 9 on the 8 m square (0.5 x 8 + 0.25 x 8 + 3); twice that
/// bound is the tolerance.
const PLANE_TOLERANCE: f64 = 4.0 * f64::EPSILON * 9.0;

/// Survey points on the plane at every whole metre of the 8 m square, or of
/// the triangle below its diagonal x + y = 8 when `triangle`.
fn plane_surface(triangle: bool) -> TinSurface {
    let mut input = TinInput::default();
    for j in 0..=8 {
        for i in 0..=8 {
            if !triangle || i + j <= 8 {
                let (x, y) = (f64::from(i), f64::from(j));
                input.points.push(Point3::new(x, y, plane(x, y)));
            }
        }
    }
    must(build_tin(&input, &TinBuildOptions::default())).surface
}

struct ReadBack {
    info: RasterInfo,
    samples: RasterSamples,
}

fn read_back(path: &Path) -> ReadBack {
    let dataset = must(GdalDataset::open(path));
    ReadBack {
        info: must(dataset.raster_info()),
        samples: must(dataset.read_band_sampled(1, 1)),
    }
}

const FLOAT_NO_DATA: f64 = f32::MIN as f64;

const SQUARE_GEOTRANSFORM: [f64; 6] = [0.0, 1.0, 0.0, 8.0, 0.0, -1.0];

fn export(
    surface: &TinSurface,
    path: &Path,
    options: &SurfaceRasterOptions,
) -> katana_core::Result<katana_interop::SurfaceRasterResult> {
    export_surface_raster(surface, path, options, None)
}

mod terrain_io_surface_raster {
    use super::*;

    #[test]
    fn each_cell_is_the_surface_at_its_centre() {
        let dir = temp_dir("surface-square");
        let path = dir.file("surface.tif");
        let surface = plane_surface(false);
        let options = SurfaceRasterOptions {
            projection_wkt: String::from(MGA56_WKT),
            ..SurfaceRasterOptions::default()
        };
        let written = must(export(&surface, &path, &options));

        // 8 m at 1 m cells, north-up from the top-left corner (0, 8).
        assert_eq!(written.columns, 8);
        assert_eq!(written.rows, 8);
        assert_eq!(written.cells_with_data, 64);
        assert_eq!(written.driver, "GTiff");
        assert_eq!(written.no_data_value, FLOAT_NO_DATA);
        assert_eq!(written.geotransform, SQUARE_GEOTRANSFORM);

        let back = read_back(&path);
        assert_eq!(back.info.geotransform, written.geotransform);
        assert_eq!(back.info.no_data_value, Some(FLOAT_NO_DATA));
        assert!(describe_crs(&back.info.projection_wkt).contains("EPSG:28356"));
        assert_eq!(back.samples.values.len(), 64);
        for row in 0..8 {
            for column in 0..8 {
                let x = f64::from(column) + 0.5;
                let y = 8.0 - (f64::from(row) + 0.5);
                near(
                    back.samples.values[(row * 8 + column) as usize],
                    plane(x, y),
                    PLANE_TOLERANCE,
                );
            }
        }
    }

    #[test]
    fn cells_off_the_surface_are_no_data_in_every_format() {
        // The triangle below x + y = 8. Cell (c, r) has its centre at
        // (c + 0.5, 7.5 - r), so x + y = 8 + c - r: inside for c < r, exactly
        // on the rim for c == r (on the surface: the TIN takes the rim as
        // part of it), outside for c > r. 28 + 8 = 36 of the 64 cells.
        let surface = plane_surface(true);
        for name in ["triangle.tif", "triangle.asc", "triangle.img"] {
            let dir = temp_dir(&format!("surface-{name}"));
            let path = dir.file(name);
            let written = must(export(&surface, &path, &SurfaceRasterOptions::default()));
            assert_eq!(written.cells_with_data, 36, "{name}");

            let back = read_back(&path);
            assert_eq!(back.info.no_data_value, Some(FLOAT_NO_DATA), "{name}");
            assert_eq!(back.info.geotransform, SQUARE_GEOTRANSFORM, "{name}");
            assert_eq!(back.samples.values.len(), 64, "{name}");
            let mut with_data: u64 = 0;
            for row in 0..8 {
                for column in 0..8 {
                    let value = back.samples.values[(row * 8 + column) as usize];
                    if column > row {
                        assert_eq!(value, FLOAT_NO_DATA, "{name}: cell ({column}, {row})");
                        continue;
                    }
                    with_data += 1;
                    near(
                        value,
                        plane(f64::from(column) + 0.5, 7.5 - f64::from(row)),
                        PLANE_TOLERANCE,
                    );
                }
            }
            assert_eq!(with_data, 36, "{name}");
        }
    }

    #[test]
    fn every_offered_format_is_one_the_exporter_writes() {
        // The save dialog offers raster_export_formats(); each must be an
        // extension the exporter can then write, and read back as the driver
        // it named.
        let surface = plane_surface(false);
        let formats = raster_export_formats();
        assert_eq!(formats.len(), 3);
        for format in &formats {
            let dir = temp_dir(&format!("offered-{}", format.extension));
            let path = dir.file(&format!("surface.{}", format.extension));
            let written = must(export(&surface, &path, &SurfaceRasterOptions::default()));
            assert_eq!(
                written.driver,
                must(GdalDataset::raster_driver_for_path(&path)),
                "{}",
                format.description
            );
            let dataset = must(GdalDataset::open(&path));
            assert_eq!(
                dataset.driver_name(),
                written.driver,
                "{}",
                format.description
            );
        }
    }

    #[test]
    fn an_explicit_driver_overrides_the_extension() {
        let dir = temp_dir("surface-driver");
        let path = dir.file("surface.grid");
        let options = SurfaceRasterOptions {
            driver: String::from("AAIGrid"),
            ..SurfaceRasterOptions::default()
        };
        let written = must(export(&plane_surface(false), &path, &options));
        assert_eq!(written.driver, "AAIGrid");
        assert_eq!(must(GdalDataset::open(&path)).driver_name(), "AAIGrid");
    }

    #[test]
    fn what_cannot_be_written_is_refused_and_nothing_is_left_behind() {
        let dir = temp_dir("surface-refused");
        let surface = plane_surface(false);
        let path = dir.file("surface.tif");

        assert_eq!(
            refused(
                export(
                    &TinSurface::default(),
                    &path,
                    &SurfaceRasterOptions::default()
                ),
                "an empty surface"
            )
            .code,
            ErrorCode::InvalidArgument
        );
        for cell in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let options = SurfaceRasterOptions {
                cell_size: cell,
                ..SurfaceRasterOptions::default()
            };
            assert_eq!(
                refused(export(&surface, &path, &options), "a bad cell size").code,
                ErrorCode::InvalidArgument,
                "cell size {cell}"
            );
        }

        // 8 x 8 = 64 cells against a limit of 63: refused, and the count
        // named.
        let mut small = SurfaceRasterOptions {
            max_cells: 63,
            ..SurfaceRasterOptions::default()
        };
        let too_many = refused(export(&surface, &path, &small), "too many cells");
        assert_eq!(too_many.code, ErrorCode::InvalidArgument);
        assert!(too_many.message.contains("64"), "{}", too_many.message);
        small.max_cells = 64;
        assert!(
            export(&surface, &dir.file("exactly.tif"), &small).is_ok(),
            "exactly the limit is within it"
        );

        // A cell so fine the count overflows every integer type is still
        // named, not wrapped round to something small.
        let absurd = SurfaceRasterOptions {
            cell_size: 1e-12,
            ..SurfaceRasterOptions::default()
        };
        assert_eq!(
            refused(export(&surface, &path, &absurd), "an absurd cell").code,
            ErrorCode::InvalidArgument
        );

        let unknown = refused(
            export(
                &surface,
                &dir.file("surface.xyzzy"),
                &SurfaceRasterOptions::default(),
            ),
            "an unknown extension",
        );
        assert_eq!(unknown.code, ErrorCode::Unsupported);

        assert!(!path.exists());
        assert!(!dir.file("surface.xyzzy").exists());
    }

    // ---- suggested_cell_size --------------------------------------------------------------

    #[test]
    fn the_suggested_cell_is_a_round_number_near_the_target() {
        let square = |side: f64| Box2::new(Point2::new(0.0, 0.0), Point2::new(side, side));
        // side / 1000 cells, to the nearest of 1, 2, 5 x 10^n by ratio.
        assert_eq!(suggested_cell_size(&square(1000.0), 1000), 1.0); // 1
        assert_eq!(suggested_cell_size(&square(2300.0), 1000), 2.0); // 2.3
        assert_eq!(suggested_cell_size(&square(480.0), 1000), 0.5); // 0.48
        assert_eq!(suggested_cell_size(&square(12.0), 1000), 0.01); // 0.012

        // The boundaries are sqrt(2), sqrt(10) and sqrt(50) - geometric, not
        // the arithmetic 1.5, 3.5 and 7.5 - so each of these rounds UP.
        assert_eq!(suggested_cell_size(&square(1450.0), 1000), 2.0); // 1.45 > 1.414
        assert_eq!(suggested_cell_size(&square(3300.0), 1000), 5.0); // 3.3 > 3.162
        assert_eq!(suggested_cell_size(&square(7300.0), 1000), 10.0); // 7.3 > 7.071
        // ...and these down.
        assert_eq!(suggested_cell_size(&square(1400.0), 1000), 1.0); // 1.4 < 1.414
        assert_eq!(suggested_cell_size(&square(7000.0), 1000), 5.0); // 7.0 < 7.071

        // The LONGER side decides, whichever it is.
        assert_eq!(
            suggested_cell_size(
                &Box2::new(Point2::new(0.0, 0.0), Point2::new(100.0, 5000.0)),
                1000
            ),
            5.0
        );
        assert_eq!(
            suggested_cell_size(
                &Box2::new(Point2::new(0.0, 0.0), Point2::new(5000.0, 100.0)),
                1000
            ),
            5.0
        );
        assert_eq!(suggested_cell_size(&square(1000.0), 100), 10.0);

        // Nothing to size: 1.0.
        assert_eq!(suggested_cell_size(&Box2::default(), 1000), 1.0);
        assert_eq!(
            suggested_cell_size(
                &Box2::new(Point2::new(5.0, 5.0), Point2::new(5.0, 5.0)),
                1000
            ),
            1.0
        );
        assert_eq!(suggested_cell_size(&square(1000.0), 0), 1.0);
    }
}

// ---- point-cloud export formats -------------------------------------------------------------

mod terrain_io_cloud_export {
    use super::*;

    #[test]
    fn every_offered_point_cloud_format_is_written_and_read_back() {
        let mut cloud = PointCloudLayer {
            name: String::from("grid"),
            ..PointCloudLayer::default()
        };
        for j in 0..100 {
            for i in 0..100 {
                cloud.points.push(cloud_point(
                    0.5 * f64::from(i),
                    0.5 * f64::from(j),
                    10.0 + 0.125 * f64::from(i),
                    2,
                ));
            }
        }
        let formats = point_cloud_export_formats();
        assert_eq!(formats.len(), 2);
        let dir = temp_dir("cloud-formats");
        let mut sizes: Vec<u64> = Vec::new();
        for format in &formats {
            let path = dir.file(&format!("cloud.{}", format.extension));
            must(export_point_cloud(&cloud, &path));
            let back = must(import_point_cloud(&path, &Default::default()));
            assert_eq!(
                back.points.len(),
                cloud.points.len(),
                "{}",
                format.description
            );
            sizes.push(std::fs::metadata(path).expect("the file is there").len());
        }
        // LAS then LAZ: "compressed LAS" must be compressed. A regular grid
        // compresses far better than two to one.
        assert!(
            sizes[1] * 2 < sizes[0],
            "LAS {} bytes, LAZ {}",
            sizes[0],
            sizes[1]
        );
    }
}
