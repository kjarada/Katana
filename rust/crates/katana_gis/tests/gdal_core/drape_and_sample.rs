//! tests/geo/test_drape_and_sample.cpp, its RasterSampler cases: a raster's
//! value at a point. The file's RASTER SAMPLE and DRAPE cases drive the app
//! (step 6); its Drape case is katana_cad's.
//!
//! Fixture: tests/geo/data/plane.asc, 40 x 30 cells of 1 m from (0,0),
//! z = 100 + 0.05 x at the cell centres. Bilinear interpolation of a linear
//! function is the function itself, so between any four cell centres the
//! sample is exactly the plane: z(x, y) = 100 + 0.05 x. The file is read as
//! Float32, each height within 3.815e-6 of its value near 100 (half an ulp,
//! 2^-17), and a bilinear sample is a convex combination of four of them:
//! within 3.815e-6 as well. 1e-5 allows that.

use std::path::Path;

use katana_core::ErrorCode;
use katana_gis::raster_sampling::{RasterSampler, Resampling, resampling_named};

use crate::{checkout, double_eq, must, near, refused, shared_data};

const FLOAT32: f64 = 1e-5;

fn plane(x: f64) -> f64 {
    100.0 + 0.05 * x
}

mod raster_sampler {
    use super::*;

    #[test]
    fn bilinear_on_a_plane_is_the_plane_and_off_it_is_nothing() {
        let _shared = shared_data();
        let sampler = must(RasterSampler::open(
            &checkout("tests/geo/data/plane.asc"),
            1,
        ));
        let bilinear = Resampling::Bilinear;
        near(
            sampler.at(12.3, 7.7, bilinear).unwrap_or(0.0),
            plane(12.3),
            FLOAT32,
        );
        // Nearest: the cell's centre value, column 12's: 100 + 0.05 x 12.5.
        near(
            sampler.at(12.3, 7.7, Resampling::Nearest).unwrap_or(0.0),
            plane(12.5),
            FLOAT32,
        );
        assert!(sampler.at(-0.5, 10.0, bilinear).is_none());
        assert!(sampler.at(40.5, 10.0, bilinear).is_none());
        assert!(sampler.at(10.0, 30.5, bilinear).is_none());
        double_eq(sampler.cell_size(), 1.0);
    }

    #[test]
    fn a_missing_file_and_an_unknown_method_are_refused() {
        let mut missing = checkout("tests/geo/data/plane.asc").into_os_string();
        missing.push(".nothing");
        let error = refused(
            RasterSampler::open(Path::new(&missing), 1),
            "a missing file",
        );
        assert_eq!(error.code, ErrorCode::NotFound);
        assert!(resampling_named("lanczos").is_err());
        assert_eq!(
            must(resampling_named("CubicSpline")),
            Resampling::CubicSpline
        );
    }
}
