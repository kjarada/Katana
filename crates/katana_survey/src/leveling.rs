//! Differential levelling.
//!
//! A level run starts on a benchmark of known elevation. At every setup the
//! height of instrument is HI = elevation(back point) + backsight, and the
//! forward point gets elevation = HI - foresight. When the run closes on a
//! benchmark (the start itself for a loop) the misclosure
//!     misclosure = computed closing elevation - known closing elevation
//! is distributed proportionally to the number of setups or to the distance
//! levelled. Intermediate sights are not modelled; record them as separate runs.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection."
)]

use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LevelSetup {
    /// Point that receives an elevation; may be empty.
    pub foresight_point_id: String,
    /// Staff reading on the point of known elevation, metres.
    pub backsight: f64,
    /// Staff reading on the forward point, metres.
    pub foresight: f64,
    /// Backsight + foresight sight lengths, metres; 0 when unknown.
    pub distance: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LevelRun {
    pub name: String,
    /// Opening benchmark.
    pub start_elevation: f64,
    pub setups: Vec<LevelSetup>,
    /// Known elevation of the last forward point.
    pub closing_elevation: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LevelAdjustment {
    #[default]
    None,
    /// Equal correction per setup.
    BySetups,
    /// Correction proportional to the distance levelled.
    ByDistance,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LevelPointResult {
    pub point_id: String,
    pub height_of_instrument: f64,
    /// Unadjusted.
    pub elevation: f64,
    /// Cumulative correction applied to this point.
    pub correction: f64,
    pub adjusted_elevation: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LevelRunResult {
    /// One per setup, in order.
    pub points: Vec<LevelPointResult>,
    pub sum_backsights: f64,
    pub sum_foresights: f64,
    pub total_distance: f64,
    /// None without a closing elevation.
    pub misclosure: Option<f64>,
}

const METRES_PER_KILOMETRE: f64 = 1000.0;

fn malformed(run: &LevelRun, message: impl Into<String>) -> Error {
    Error::new(ErrorCode::InvalidArgument, message)
        .with_context(format!("level run '{}'", run.name))
}

fn scaled_by_sqrt_kilometres(factor: f64, length_metres: f64, factor_name: &str) -> Result<f64> {
    if !factor.is_finite() || factor < 0.0 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("{factor_name} must be finite and not negative"),
        ));
    }
    if !length_metres.is_finite() || length_metres < 0.0 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "length must be finite and not negative",
        )
        .with_context(format!("length {}", format_exact_real(length_metres))));
    }
    Ok(factor * (length_metres / METRES_PER_KILOMETRE).sqrt())
}

/// InvalidArgument for an empty run, non-finite readings, negative distances, an
/// adjustment without a closing elevation, or ByDistance with zero total distance.
pub fn compute_level_run(run: &LevelRun, adjustment: LevelAdjustment) -> Result<LevelRunResult> {
    if run.setups.is_empty() {
        return Err(malformed(run, "a level run needs at least one setup"));
    }
    if !run.start_elevation.is_finite() {
        return Err(malformed(run, "start elevation is not finite"));
    }
    if let Some(closing) = run.closing_elevation
        && !closing.is_finite()
    {
        return Err(malformed(run, "closing elevation is not finite"));
    }
    for (i, setup) in run.setups.iter().enumerate() {
        if !setup.backsight.is_finite() || !setup.foresight.is_finite() {
            return Err(malformed(
                run,
                format!("staff reading of setup {i} is not finite"),
            ));
        }
        if !setup.distance.is_finite() || setup.distance < 0.0 {
            return Err(malformed(
                run,
                format!("distance of setup {i} must be finite and not negative"),
            ));
        }
    }
    if adjustment != LevelAdjustment::None && run.closing_elevation.is_none() {
        return Err(malformed(run, "an adjustment needs a closing elevation"));
    }

    let mut result = LevelRunResult {
        points: Vec::with_capacity(run.setups.len()),
        ..Default::default()
    };
    let mut elevation = run.start_elevation;
    for setup in &run.setups {
        let height_of_instrument = elevation + setup.backsight;
        let point_elevation = height_of_instrument - setup.foresight;
        elevation = point_elevation;
        result.sum_backsights += setup.backsight;
        result.sum_foresights += setup.foresight;
        result.total_distance += setup.distance;
        result.points.push(LevelPointResult {
            point_id: setup.foresight_point_id.clone(),
            height_of_instrument,
            elevation: point_elevation,
            correction: 0.0,
            adjusted_elevation: point_elevation,
        });
    }
    if let Some(closing) = run.closing_elevation {
        result.misclosure = Some(elevation - closing);
    }
    if adjustment == LevelAdjustment::None {
        return Ok(result);
    }
    if adjustment == LevelAdjustment::ByDistance && !(result.total_distance > 0.0) {
        return Err(malformed(
            run,
            "adjustment by distance needs setup distances",
        ));
    }

    // The correction grows along the run and reaches -misclosure at the closing
    // point, so the adjusted closing elevation equals the known one.
    let misclosure = result.misclosure.unwrap_or_default();
    let setup_count = run.setups.len() as f64;
    let total_distance = result.total_distance;
    let mut distance = 0.0;
    for (i, (point, setup)) in result.points.iter_mut().zip(&run.setups).enumerate() {
        distance += setup.distance;
        let fraction = if adjustment == LevelAdjustment::BySetups {
            (i + 1) as f64 / setup_count
        } else {
            distance / total_distance
        };
        point.correction = -misclosure * fraction;
        point.adjusted_elevation = point.elevation + point.correction;
    }
    Ok(result)
}

/// Allowable misclosure k * sqrt(K) of a level line of `length_metres`, with K
/// in kilometres. `k` is in metres per sqrt(km): 0.012 for the common
/// 12 mm * sqrt(K) third-order criterion. The result is in metres.
pub fn allowable_level_misclosure(k: f64, length_metres: f64) -> Result<f64> {
    scaled_by_sqrt_kilometres(k, length_metres, "k")
}

/// A-priori standard deviation of a levelled height difference over
/// `length_metres`: sigma_per_sqrt_km * sqrt(K). Metres.
pub fn level_difference_sigma(sigma_per_sqrt_km: f64, length_metres: f64) -> Result<f64> {
    scaled_by_sqrt_kilometres(sigma_per_sqrt_km, length_metres, "sigma per sqrt(km)")
}
