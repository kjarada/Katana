//! Shared support for the geodesy tests - the port of
//! tests/geodesy/geodesy_test_support.hpp and tests/support/property.hpp: the
//! result checks, the fixed-seed generator, and the INDEPENDENT ORACLES that
//! expected values are taken from. Nothing in this file calls PROJ or
//! katana_geodesy for a number it then asserts against; the formulas are
//! coded here from the literature so that a wrong answer from the library
//! cannot validate itself.

use katana_core::{Error, Result};
use katana_geodesy::{
    CoordinateReferenceSystem, CoordinateTransformer, GeographicCoordinate, TransformerOptions,
};
use katana_math::numerics::{DEG_TO_RAD, RAD_TO_DEG, tolerance};

// ---- results -------------------------------------------------------------------

/// ASSERT_OK: the value, or a panic naming the error.
#[track_caller]
pub fn assert_ok<T>(result: Result<T>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{}", error.describe()),
    }
}

/// ASSERT_FALSE(result.ok()): the error, or a panic.
#[track_caller]
pub fn assert_err<T>(result: Result<T>) -> Error {
    match result {
        Ok(_) => panic!("expected an error, the call succeeded"),
        Err(error) => error,
    }
}

/// EXPECT_NEAR.
#[track_caller]
pub fn near(actual: f64, expected: f64, epsilon: f64) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
    );
}

/// EXPECT_NEAR with the C++'s streamed explanation.
#[track_caller]
pub fn near_in(actual: f64, expected: f64, epsilon: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "{what}: actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
    );
}

/// EXPECT_DOUBLE_EQ: within four units in the last place.
#[track_caller]
pub fn double_eq(actual: f64, expected: f64) {
    fn biased(value: f64) -> u64 {
        let bits = value.to_bits();
        if bits & (1 << 63) != 0 {
            (!bits).wrapping_add(1)
        } else {
            bits | (1 << 63)
        }
    }
    assert!(
        !actual.is_nan() && !expected.is_nan() && biased(actual).abs_diff(biased(expected)) <= 4,
        "actual={actual:?}, expected={expected:?}"
    );
}

/// Bit pattern equality: distinguishes -0.0 from 0.0 and compares NaN
/// payloads, which == cannot.
pub fn same_bits(first: f64, second: f64) -> bool {
    first.to_bits() == second.to_bits()
}

// ---- the property generator ----------------------------------------------------------

pub const PROPERTY_ITERATIONS: usize = 500;

fn real_from_bits(mut next: impl FnMut() -> u64, low: f64, high: f64) -> f64 {
    loop {
        let fraction = next() as f64 / 18446744073709551616.0;
        if fraction < 1.0 {
            return fraction * (high - low) + low;
        }
    }
}

/// katana::test::Random: std::mt19937_64 seeded with 0x4B4154414E41, with
/// GCC's uniform_real_distribution<double> and uniform_int_distribution<int>
/// (as ported for katana_geometry's tests), so that every case explores the
/// inputs the C++ explores.
pub struct Random {
    state: [u64; 312],
    index: usize,
}

impl Default for Random {
    fn default() -> Self {
        Self::new(0x4B4154414E41)
    }
}

impl Random {
    pub fn new(seed: u64) -> Self {
        let mut state = [0; 312];
        state[0] = seed;
        for index in 1..312 {
            state[index] = 6364136223846793005_u64
                .wrapping_mul(state[index - 1] ^ (state[index - 1] >> 62))
                .wrapping_add(index as u64);
        }
        Self { state, index: 312 }
    }

    pub fn next_u64(&mut self) -> u64 {
        if self.index == 312 {
            for index in 0..312 {
                let value = (self.state[index] & 0xffffffff80000000)
                    | (self.state[(index + 1) % 312] & 0x7fffffff);
                self.state[index] = self.state[(index + 156) % 312]
                    ^ (value >> 1)
                    ^ if value & 1 != 0 {
                        0xb5026f5aa96619e9
                    } else {
                        0
                    };
            }
            self.index = 0;
        }
        let mut value = self.state[self.index];
        self.index += 1;
        value ^= (value >> 29) & 0x5555555555555555;
        value ^= (value << 17) & 0x71d67fffeda60000;
        value ^= (value << 37) & 0xfff7eee000000000;
        value ^ (value >> 43)
    }

    pub fn real(&mut self, low: f64, high: f64) -> f64 {
        real_from_bits(|| self.next_u64(), low, high)
    }

    pub fn integer(&mut self, low: i32, high: i32) -> i32 {
        let range = (high as i64 - low as i64 + 1) as u64;
        let threshold = range.wrapping_neg() % range;
        loop {
            let product = self.next_u64() as u128 * range as u128;
            if product as u64 >= threshold {
                return low + (product >> 64) as i32;
            }
        }
    }
}

// ---- comparison bounds ---------------------------------------------------------------
// Numerical bounds come from the central policy (tolerance). The two helpers
// below are not tolerances of Katana but properties of reference DATA.

/// A published value rounded to `last_digit` (0.001 for millimetres) agrees
/// with the true value to half a unit in its last place.
pub const fn half_unit(last_digit: f64) -> f64 {
    0.5 * last_digit
}

/// The longest degree on the WGS 84 ellipsoid: the degree of latitude at the
/// pole, where the meridional radius of curvature is a/(1-f) = 6 399 594 m.
/// Every other degree, of latitude or of longitude, is shorter.
pub const LONGEST_DEGREE_METRES: f64 = 111694.0;

/// tolerance::COORDINATE (0.1 mm on the ground) expressed as an angle.
/// Measured with the longest degree it is at most 0.1 mm on the ground
/// everywhere, in latitude and in longitude. About 9.0e-10 degrees.
pub const COORDINATE_AS_DEGREES: f64 = tolerance::COORDINATE / LONGEST_DEGREE_METRES;

/// Upper bound of the ground distance between two nearby geographic
/// positions: the angular separation, longitude weighted by cos(latitude),
/// measured with the longest degree. It never understates the distance.
pub fn ground_separation_metres(a: &GeographicCoordinate, b: &GeographicCoordinate) -> f64 {
    let cos_latitude = (a.latitude * DEG_TO_RAD).cos();
    LONGEST_DEGREE_METRES
        * (a.latitude - b.latitude).hypot((a.longitude - b.longitude) * cos_latitude)
}

// ---- WGS 84 defining constants (NIMA TR8350.2) ------------------------------------------
pub const WGS84_A: f64 = 6378137.0;
pub const WGS84_INVERSE_F: f64 = 298.257223563;

// ---- Oracle 1: Transverse Mercator by the Krueger series ----------------------------------
// L. Krueger (1912) as revived by C. F. F. Karney, "Transverse Mercator with an
// accuracy of a few nanometers", J. Geodesy 85 (2011), eqs. (7)-(11), (35),
// series to the 6th order in the third flattening n. Valid for a latitude of
// origin of 0 (every UTM zone). Truncation error is below 5 nm within 35
// degrees of the central meridian.
#[derive(Clone, Copy, Debug)]
pub struct TransverseMercator {
    pub a: f64,
    pub inverse_flattening: f64,
    /// UTM.
    pub scale_factor: f64,
    /// Zone 30: -3.
    pub central_meridian_deg: f64,
    /// UTM.
    pub false_easting: f64,
    /// UTM, northern hemisphere.
    pub false_northing: f64,
}

impl Default for TransverseMercator {
    fn default() -> Self {
        Self {
            a: WGS84_A,
            inverse_flattening: WGS84_INVERSE_F,
            scale_factor: 0.9996,
            central_meridian_deg: 0.0,
            false_easting: 500000.0,
            false_northing: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GridPosition {
    pub easting: f64,
    pub northing: f64,
}

// Rust's asinh and atanh are computed by std itself, not by the C library's,
// so the oracle may differ from the C++'s by a few units in the last place -
// nanometres below the 0.1 micrometre it is compared at.
pub fn kruger_forward(
    tm: &TransverseMercator,
    latitude_deg: f64,
    longitude_deg: f64,
) -> GridPosition {
    let f = 1.0 / tm.inverse_flattening;
    let n = f / (2.0 - f);
    let n2 = n * n;
    let n3 = n2 * n;
    let n4 = n3 * n;
    let n5 = n4 * n;
    let n6 = n5 * n;

    // Rectifying radius A and the alpha coefficients, Karney (2011) eqs. (14), (35).
    let rectifying_radius = tm.a / (1.0 + n) * (1.0 + n2 / 4.0 + n4 / 64.0 + n6 / 256.0);
    let alpha = [
        n / 2.0 - 2.0 * n2 / 3.0 + 5.0 * n3 / 16.0 + 41.0 * n4 / 180.0 - 127.0 * n5 / 288.0
            + 7891.0 * n6 / 37800.0,
        13.0 * n2 / 48.0 - 3.0 * n3 / 5.0 + 557.0 * n4 / 1440.0 + 281.0 * n5 / 630.0
            - 1983433.0 * n6 / 1935360.0,
        61.0 * n3 / 240.0 - 103.0 * n4 / 140.0 + 15061.0 * n5 / 26880.0 + 167603.0 * n6 / 181440.0,
        49561.0 * n4 / 161280.0 - 179.0 * n5 / 168.0 + 6601661.0 * n6 / 7257600.0,
        34729.0 * n5 / 80640.0 - 3418889.0 * n6 / 1995840.0,
        212378941.0 * n6 / 319334400.0,
    ];

    let e = (f * (2.0 - f)).sqrt();
    let phi = latitude_deg * DEG_TO_RAD;
    let lambda = (longitude_deg - tm.central_meridian_deg) * DEG_TO_RAD;

    // Conformal latitude through tau' (eqs. 7-9), stable at all latitudes.
    let tau = phi.tan();
    let sigma = (e * (e * tau / (1.0 + tau * tau).sqrt()).atanh()).sinh();
    let tau_prime = tau * (1.0 + sigma * sigma).sqrt() - sigma * (1.0 + tau * tau).sqrt();

    let cos_lambda = lambda.cos();
    let xi_prime = tau_prime.atan2(cos_lambda);
    let eta_prime =
        (lambda.sin() / (tau_prime * tau_prime + cos_lambda * cos_lambda).sqrt()).asinh();

    let mut xi = xi_prime;
    let mut eta = eta_prime;
    for (j, coefficient) in alpha.iter().enumerate() {
        let k = 2.0 * (j + 1) as f64;
        xi += coefficient * (k * xi_prime).sin() * (k * eta_prime).cosh();
        eta += coefficient * (k * xi_prime).cos() * (k * eta_prime).sinh();
    }
    GridPosition {
        easting: tm.false_easting + tm.scale_factor * rectifying_radius * eta,
        northing: tm.false_northing + tm.scale_factor * rectifying_radius * xi,
    }
}

/// Point scale factor and grid convergence of the same projection, obtained
/// by differentiating `kruger_forward` numerically along the meridian:
///
/// ```text
/// k     = |d(E,N)/d(phi)| / M(phi)         M = meridional radius of curvature
/// gamma = -atan2(dE/dphi, dN/dphi)         azimuth of grid north
/// ```
///
/// (the projection is conformal, so the scale along the meridian is the scale
/// in every direction). Central differences with a step of 1e-6 rad:
/// truncation ~1e-12 relative, rounding ~1e-9 relative at UTM magnitudes.
#[derive(Clone, Copy, Debug)]
pub struct OracleGridFactors {
    pub scale: f64,
    pub convergence_deg: f64,
}

pub fn kruger_factors(
    tm: &TransverseMercator,
    latitude_deg: f64,
    longitude_deg: f64,
) -> OracleGridFactors {
    const STEP_RAD: f64 = 1e-6;
    let step_deg = STEP_RAD * RAD_TO_DEG;
    let north = kruger_forward(tm, latitude_deg + step_deg, longitude_deg);
    let south = kruger_forward(tm, latitude_deg - step_deg, longitude_deg);
    let d_easting = (north.easting - south.easting) / (2.0 * STEP_RAD);
    let d_northing = (north.northing - south.northing) / (2.0 * STEP_RAD);

    let f = 1.0 / tm.inverse_flattening;
    let e2 = f * (2.0 - f);
    let sin_phi = (latitude_deg * DEG_TO_RAD).sin();
    let w2 = 1.0 - e2 * sin_phi * sin_phi;
    let meridional_radius = tm.a * (1.0 - e2) / (w2 * w2.sqrt());

    OracleGridFactors {
        scale: d_easting.hypot(d_northing) / meridional_radius,
        convergence_deg: -d_easting.atan2(d_northing) * RAD_TO_DEG,
    }
}

// ---- Oracle 2: geodetic -> geocentric, closed form ------------------------------------------
//     X = (N + h) cos(phi) cos(lambda)      N = a / sqrt(1 - e^2 sin^2(phi))
//     Y = (N + h) cos(phi) sin(lambda)
//     Z = (N (1 - e^2) + h) sin(phi)
#[derive(Clone, Copy, Debug, Default)]
pub struct Geocentric {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

pub fn geodetic_to_geocentric(
    a: f64,
    inverse_flattening: f64,
    latitude_deg: f64,
    longitude_deg: f64,
    height: f64,
) -> Geocentric {
    let f = 1.0 / inverse_flattening;
    let e2 = f * (2.0 - f);
    let phi = latitude_deg * DEG_TO_RAD;
    let lambda = longitude_deg * DEG_TO_RAD;
    let prime_vertical = a / (1.0 - e2 * phi.sin() * phi.sin()).sqrt();
    Geocentric {
        x: (prime_vertical + height) * phi.cos() * lambda.cos(),
        y: (prime_vertical + height) * phi.cos() * lambda.sin(),
        z: (prime_vertical * (1.0 - e2) + height) * phi.sin(),
    }
}

/// Quarter meridian (equator to pole) = rectifying radius * pi / 2.
pub fn quarter_meridian(a: f64, inverse_flattening: f64) -> f64 {
    let f = 1.0 / inverse_flattening;
    let n = f / (2.0 - f);
    let n2 = n * n;
    a / (1.0 + n)
        * (1.0 + n2 / 4.0 + n2 * n2 / 64.0 + n2 * n2 * n2 / 256.0)
        * katana_math::numerics::HALF_PI
}

/// Degrees from degrees, minutes, seconds (sign applied to the whole value).
pub const fn dms(degrees: f64, minutes: f64, seconds: f64) -> f64 {
    degrees + minutes / 60.0 + seconds / 3600.0
}

// ---- helpers -----------------------------------------------------------------------------

pub fn make_transformer_with(
    source_epsg: i32,
    target_epsg: i32,
    options: &TransformerOptions,
) -> Result<CoordinateTransformer> {
    let source = CoordinateReferenceSystem::from_epsg(source_epsg)?;
    let target = CoordinateReferenceSystem::from_epsg(target_epsg)?;
    CoordinateTransformer::create(&source, &target, options)
}

pub fn make_transformer(source_epsg: i32, target_epsg: i32) -> Result<CoordinateTransformer> {
    make_transformer_with(source_epsg, target_epsg, &TransformerOptions::default())
}
