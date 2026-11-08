//! The formulas of the reduction, one function each, so that every number the
//! report prints comes from one place and the report can name the formula.
//! Private to the crate: the public face of these is the report
//! (reduction_report.rs), which records what each one did to each value.

// The C library's erfc, the one std::erfc calls: the normal quantile below
// bisects on it, so its digits decide the quantile's last bit. Rust has no
// stable erfc of its own.
use katana_math::libm::erfc;
use katana_math::numerics::PI;

use crate::data_model::{GeocentricCoordinate, GeodeticCoordinate, GnssCovariance3};

// ---- Atmospheric correction --------------------------------------------------------
//
// Manufacturer-neutral: the group refractivity of air for the EDM's carrier by
// the IUGG 1999 resolution (Ciddor & Hill 1999, the formula the IAG adopted for
// electronic distance measurement):
//
//   N_gr   = 287.6155 + 4.88660 / L^2 + 0.06800 / L^4        (L in micrometres;
//            standard air: 0 C, 1013.25 hPa, dry, 375 ppm CO2)
//   N_L    = (273.15 / 1013.25) N_gr p / T  -  11.27 e / T   (T kelvin, p and e hPa)
//   e      = RH / 100 * 6.1078 * 10^(7.5 t / (237.3 + t))   (Magnus-Tetens, over water)
//   ppm    = N_ref - N_L
//
// where N_ref is N_L at the instrument's reference atmosphere, the atmosphere
// in which its displayed distance needs no correction. Nothing in a field file
// states the carrier or the reference atmosphere, so both are constants here:
// a 658 nm carrier and 12 C, 1013.25 hPa, 60 % relative humidity, which gives
// N_ref = 286.338, the reference refractivity Leica publishes for its
// instruments, reproduced by the formula, not copied. An instrument with
// another reference (a 15 C, dry reference is about 2.6 ppm away) wants
// AtmosphericCorrection::Fixed with the value its own manual gives.
//
// Compared with the owner's NeuralSurvey engine (engine/src/corrections.rs,
// atmospheric_ppm): it uses 286.34 - 0.29525 P / (1 + 0.00366 t), which is the
// same dry-air term with a coefficient 0.01 low (0.29535 is what the IUGG
// refractivity gives at 658 nm) and no humidity term. At 20 C, 1013.25 hPa and
// 60 % the two differ by about 0.45 ppm (0.45 mm per km).
pub(crate) const CARRIER_WAVELENGTH_MICROMETRES: f64 = 0.658;
pub(crate) const REFERENCE_TEMPERATURE_CELSIUS: f64 = 12.0;
pub(crate) const REFERENCE_PRESSURE_HECTOPASCALS: f64 = 1013.25;
pub(crate) const REFERENCE_HUMIDITY_PERCENT: f64 = 60.0;
/// Used when a setup states temperature and pressure but no humidity. The
/// humidity term is under 1 ppm below 25 C, so a guess of 60 % costs at most a
/// fraction of that, and the report says it was guessed.
pub(crate) const ASSUMED_HUMIDITY_PERCENT: f64 = 60.0;

const KELVIN_OFFSET: f64 = 273.15;
const STANDARD_PRESSURE: f64 = 1013.25; // hPa

// N_gr of standard air for the carrier, IUGG 1999.
fn standard_group_refractivity() -> f64 {
    let l2 = CARRIER_WAVELENGTH_MICROMETRES * CARRIER_WAVELENGTH_MICROMETRES;
    287.6155 + 4.88660 / l2 + 0.06800 / (l2 * l2)
}

// Bisection to the resolution of a double: a monotone function, a bracket, and
// no derivative that could send Newton out of it.
fn solve_increasing(f: impl Fn(f64) -> f64, target: f64, mut low: f64, mut high: f64) -> f64 {
    let mut i = 0;
    while i < 200 && low < high {
        let middle = 0.5 * (low + high);
        if middle <= low || middle >= high {
            break;
        }
        if f(middle) < target {
            low = middle;
        } else {
            high = middle;
        }
        i += 1;
    }
    0.5 * (low + high)
}

// P(|T| < t) for Student's t with nu degrees of freedom, exactly, by
// Abramowitz & Stegun 26.7.3 (nu odd) and 26.7.4 (nu even).
fn student_central_probability(t: f64, nu: usize) -> f64 {
    let theta = (t / (nu as f64).sqrt()).atan();
    let s = theta.sin();
    let c = theta.cos();
    let c2 = c * c;
    if nu.is_multiple_of(2) {
        // sin(theta) (1 + 1/2 cos^2 + 1*3/(2*4) cos^4 + ... ) to cos^(nu-2)
        let mut term = 1.0;
        let mut sum = 1.0;
        let mut k = 2;
        while k + 2 <= nu {
            term *= c2 * (k - 1) as f64 / k as f64;
            sum += term;
            k += 2;
        }
        return s * sum;
    }
    if nu == 1 {
        return 2.0 * theta / PI;
    }
    // 2/pi (theta + sin cos (1 + 2/3 cos^2 + 2*4/(3*5) cos^4 + ...)) to cos^(nu-3)
    let mut term = 1.0;
    let mut sum = 1.0;
    let mut k = 2;
    while k + 3 <= nu {
        term *= c2 * k as f64 / (k + 1) as f64;
        sum += term;
        k += 2;
    }
    2.0 / PI * (theta + s * c * sum)
}

/// Saturation vapour pressure over water, hPa (Magnus-Tetens).
pub(crate) fn saturation_vapour_pressure(temperature_celsius: f64) -> f64 {
    6.1078 * 10.0_f64.powf(7.5 * temperature_celsius / (237.3 + temperature_celsius))
}

/// N_L of the IUGG 1999 formula for the carrier above.
pub(crate) fn group_refractivity(
    temperature_celsius: f64,
    pressure_hectopascals: f64,
    relative_humidity_percent: f64,
) -> f64 {
    let kelvin = temperature_celsius + KELVIN_OFFSET;
    let vapour =
        relative_humidity_percent / 100.0 * saturation_vapour_pressure(temperature_celsius);
    KELVIN_OFFSET / STANDARD_PRESSURE * standard_group_refractivity() * pressure_hectopascals
        / kelvin
        - 11.27 * vapour / kelvin
}

/// ppm = N_ref - N_L: the first velocity correction, positive when the air is
/// thinner than the reference (warm, low pressure) and the distance must grow.
pub(crate) fn atmospheric_ppm(
    temperature_celsius: f64,
    pressure_hectopascals: f64,
    relative_humidity_percent: f64,
) -> f64 {
    let reference = group_refractivity(
        REFERENCE_TEMPERATURE_CELSIUS,
        REFERENCE_PRESSURE_HECTOPASCALS,
        REFERENCE_HUMIDITY_PERCENT,
    );
    reference
        - group_refractivity(
            temperature_celsius,
            pressure_hectopascals,
            relative_humidity_percent,
        )
}

// ---- Slope to horizontal with curvature and refraction ------------------------------
//
// The reduction every total station applies (as the Leica TPS1200 and TS16
// manuals print it, and the textbooks derive it):
//
//   X  = S cos z            the measured vertical component
//   Y  = S sin z            the measured horizontal component
//   HD = Y - A X Y          A = (1 - k/2) / R
//   VD = X + B Y^2          B = (1 - k) / (2 R)
//
// A X Y is the shortening of the horizontal distance because the level
// surface at the target is not the instrument's horizon; B Y^2 is the
// curvature of that surface less the refraction of the line of sight. The
// height difference mark to mark is VD + HI - HT. NeuralSurvey applies B Y^2
// to the height and nothing to the distance: A X Y is 12.7 mm on a 1 km line
// at a 5 degree slope (the target's plumb line is tilted by Y / R against the
// instrument's), and zero on a level line.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct SlopeReduction {
    /// Y
    pub measured_horizontal: f64,
    /// X
    pub measured_vertical: f64,
    /// -A X Y (0 without curvature and refraction).
    pub horizontal_correction: f64,
    /// +B Y^2 (0 without curvature and refraction).
    pub vertical_correction: f64,
}

pub(crate) fn reduce_slope(
    slope_distance: f64,
    zenith_angle: f64,
    curvature_and_refraction: bool,
    refraction: f64,
    earth_radius: f64,
) -> SlopeReduction {
    let mut reduction = SlopeReduction {
        measured_vertical: slope_distance * zenith_angle.cos(),
        measured_horizontal: slope_distance * zenith_angle.sin(),
        ..Default::default()
    };
    if curvature_and_refraction {
        let a = (1.0 - 0.5 * refraction) / earth_radius;
        let b = (1.0 - refraction) / (2.0 * earth_radius);
        reduction.horizontal_correction =
            -a * reduction.measured_vertical * reduction.measured_horizontal;
        reduction.vertical_correction =
            b * reduction.measured_horizontal * reduction.measured_horizontal;
    }
    reduction
}

/// R / (R + h): a horizontal distance at mean height h above the datum, taken
/// down to the datum.
pub(crate) fn height_reduction_factor(mean_height: f64, earth_radius: f64) -> f64 {
    earth_radius / (earth_radius + mean_height)
}

// ---- GNSS: global coordinates --------------------------------------------------------
//
// A receiver's global values are on the GRS80 ellipsoid (WGS 84 differs from
// it by 0.1 mm in the semi-minor axis). Only what the reduction itself needs:
// the drawing's projection comes in through ReductionContext.
pub(crate) const GRS80_SEMI_MAJOR: f64 = 6378137.0;
pub(crate) const GRS80_FLATTENING: f64 = 1.0 / 298.257222101;

/// Latitude, longitude (radians) and ellipsoidal height to X, Y, Z.
pub(crate) fn geocentric_from_geodetic(geodetic: &GeodeticCoordinate) -> GeocentricCoordinate {
    let e2 = GRS80_FLATTENING * (2.0 - GRS80_FLATTENING);
    let sin_latitude = geodetic.latitude.sin();
    let cos_latitude = geodetic.latitude.cos();
    // Radius of curvature in the prime vertical.
    let nu = GRS80_SEMI_MAJOR / (1.0 - e2 * sin_latitude * sin_latitude).sqrt();
    let h = geodetic.ellipsoidal_height;
    GeocentricCoordinate {
        x: (nu + h) * cos_latitude * geodetic.longitude.cos(),
        y: (nu + h) * cos_latitude * geodetic.longitude.sin(),
        z: (nu * (1.0 - e2) + h) * sin_latitude,
    }
}

/// X, Y, Z to latitude and longitude (radians) and ellipsoidal height, by
/// Bowring's iteration (converged to 1e-12 rad in three steps anywhere near
/// the earth's surface). Undefined at the centre of the earth.
pub(crate) fn geodetic_from_geocentric(geocentric: &GeocentricCoordinate) -> GeodeticCoordinate {
    let a = GRS80_SEMI_MAJOR;
    let f = GRS80_FLATTENING;
    let b = a * (1.0 - f);
    let e2 = f * (2.0 - f);
    let ep2 = e2 / (1.0 - e2);
    let p = geocentric.x.hypot(geocentric.y);
    let longitude = geocentric.y.atan2(geocentric.x);
    // Bowring: iterate on the parametric latitude.
    let mut beta = (geocentric.z * a).atan2(p * b);
    let mut latitude = 0.0;
    for _ in 0..4 {
        let sin_beta = beta.sin();
        let cos_beta = beta.cos();
        latitude = (geocentric.z + ep2 * b * sin_beta * sin_beta * sin_beta)
            .atan2(p - e2 * a * cos_beta * cos_beta * cos_beta);
        beta = ((1.0 - f) * f64::tan(latitude)).atan();
    }
    let sin_latitude = f64::sin(latitude);
    let nu = a / (1.0 - e2 * sin_latitude * sin_latitude).sqrt();
    // Near the poles p / cos(latitude) loses its digits; the z form does not.
    let ellipsoidal_height = if f64::abs(latitude) < 0.25 * PI {
        p / f64::cos(latitude) - nu
    } else {
        geocentric.z / sin_latitude - nu * (1.0 - e2)
    };
    GeodeticCoordinate {
        latitude,
        longitude,
        ellipsoidal_height,
    }
}

/// The local north, east and up variances of an X/Y/Z covariance at the
/// latitude and longitude of `at`: R C R^T with R the rows north, east, up.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct LocalVariances {
    pub north: f64,
    pub east: f64,
    pub up: f64,
}

pub(crate) fn local_variances(c: &GnssCovariance3, at: &GeodeticCoordinate) -> LocalVariances {
    let sin_latitude = at.latitude.sin();
    let cos_latitude = at.latitude.cos();
    let sin_longitude = at.longitude.sin();
    let cos_longitude = at.longitude.cos();
    let rows = [
        // north
        [
            -sin_latitude * cos_longitude,
            -sin_latitude * sin_longitude,
            cos_latitude,
        ],
        // east
        [-sin_longitude, cos_longitude, 0.0],
        // up
        [
            cos_latitude * cos_longitude,
            cos_latitude * sin_longitude,
            sin_latitude,
        ],
    ];
    let matrix = [[c.xx, c.xy, c.xz], [c.xy, c.yy, c.yz], [c.xz, c.yz, c.zz]];
    let mut variances = [0.0; 3];
    for (variance, row) in variances.iter_mut().zip(&rows) {
        for (i, matrix_row) in matrix.iter().enumerate() {
            for (j, entry) in matrix_row.iter().enumerate() {
                *variance += row[i] * entry * row[j];
            }
        }
    }
    LocalVariances {
        north: variances[0],
        east: variances[1],
        up: variances[2],
    }
}

// ---- Distributions for the outlier tests ---------------------------------------------

/// z with P(Z <= z) = probability for a standard normal Z; probability in (0, 1).
pub(crate) fn normal_quantile(probability: f64) -> f64 {
    // Solved on the tail that holds the probability, so that 1 - 1e-9 is not
    // rounded to a cruder number before the search starts.
    if probability < 0.5 {
        return -normal_quantile(1.0 - probability);
    }
    let tail = 1.0 - probability;
    // Upper tail Q(z) = erfc(z / sqrt 2) / 2 decreases, so search on -Q.
    solve_increasing(|z| -0.5 * erfc(z / 2.0_f64.sqrt()), -tail, 0.0, 40.0)
}

/// t with P(T <= t) = probability for Student's t with `degrees_of_freedom` >= 1;
/// probability in (0.5, 1). Exact (Abramowitz & Stegun 26.7.3 and 26.7.4, the
/// finite series in theta = atan(t / sqrt(nu))) up to 200 degrees of freedom,
/// the Cornish-Fisher expansion A&S 26.7.5 above that, whose truncation error is
/// below 1e-8 there.
pub(crate) fn student_t_quantile(probability: f64, degrees_of_freedom: usize) -> f64 {
    if degrees_of_freedom > 200 {
        // A&S 26.7.5.
        let z = normal_quantile(probability);
        let nu = degrees_of_freedom as f64;
        let z2 = z * z;
        let z3 = z2 * z;
        let z5 = z3 * z2;
        let z7 = z5 * z2;
        let z9 = z7 * z2;
        let g1 = (z3 + z) / 4.0;
        let g2 = (5.0 * z5 + 16.0 * z3 + 3.0 * z) / 96.0;
        let g3 = (3.0 * z7 + 19.0 * z5 + 17.0 * z3 - 15.0 * z) / 384.0;
        let g4 = (79.0 * z9 + 776.0 * z7 + 1482.0 * z5 - 1920.0 * z3 - 945.0 * z) / 92160.0;
        return z + g1 / nu + g2 / (nu * nu) + g3 / (nu * nu * nu) + g4 / (nu * nu * nu * nu);
    }
    // P(T <= t) = (1 + P(|T| < t)) / 2 for t >= 0.
    let central = 2.0 * probability - 1.0;
    solve_increasing(
        |t| student_central_probability(t, degrees_of_freedom),
        central,
        0.0,
        1e7,
    )
}

/// Critical value of Baarda's w for a two-sided test at `significance`:
/// the normal quantile at 1 - significance / 2 (3.29 for 0.001).
pub(crate) fn baarda_critical(significance: f64) -> f64 {
    normal_quantile(1.0 - 0.5 * significance)
}

/// Critical value of Pope's tau for redundancy r >= 2 at `significance`:
///   tau_c = t sqrt(r / (r - 1 + t^2)),  t = t_{r-1}(1 - significance / 2).
/// Absent for r < 2, where tau cannot exceed its own maximum sqrt(r) in any
/// informative way.
pub(crate) fn tau_critical(significance: f64, redundancy: usize) -> Option<f64> {
    if redundancy < 2 {
        return None;
    }
    let t = student_t_quantile(1.0 - 0.5 * significance, redundancy - 1);
    let r = redundancy as f64;
    Some(t * (r / (r - 1.0 + t * t)).sqrt())
}
