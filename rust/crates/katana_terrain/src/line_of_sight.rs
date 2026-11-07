//! Line of sight between two points over the ground (docs/terrain.md,
//! "Viewshed and line of sight"): whether a target can be seen from an
//! observer, where the ground first hides it, and how far above the ground
//! the sight passes at its lowest. What the LOS verb reports.
//!
//! Native, not GDAL's: no algorithm of GDAL's answers it (`raster viewshed`
//! answers every cell from one observer, not one sight line with its
//! clearance). The ground is a callback, so the same walk reads a TIN on its
//! triangles or a raster through GDAL's interpolation, and this module never
//! sees either.
//!
//! The walk: the observer's eye is `observer_height` above the ground under
//! it, the aim `target_height` above the ground under the target, and the
//! sight the straight line between them. It is checked at stations `step`
//! apart (the last spacing shortened so they end exactly at the target),
//! between the ends only. A station's clearance is the sight's height less
//! the ground's there; the target is hidden where the clearance first falls
//! below zero (a grazing sight, clearance exactly zero, still sees). A
//! station off the ground cannot hide anything: it is counted as unknown,
//! never read as ground at 0.
//!
//! Earth curvature and refraction lower the ground by `curvature` x d^2 /
//! 12 741 994 m at a distance d from the observer - GDAL's viewshed rule and
//! sphere diameter, so a sight line and a viewshed of the same ground agree.
//! 0 turns it off; GDAL's default is 0.85714 (refraction of 1/7).

use std::sync::atomic::{AtomicBool, Ordering};

use katana_core::{Error, ErrorCode, Result};
use katana_geometry::Point2;

/// The height of the ground at a plan position; None off it.
pub type GroundAt<'a> = &'a dyn Fn(Point2) -> Option<f64>;

/// The sphere GDAL's viewshed corrects heights over (gdal_viewshed:
/// "Sphere_Diameter", 12 741 994 m, the Earth's mean diameter).
pub const EARTH_DIAMETER: f64 = 12_741_994.0;
/// GDAL's default curvature-and-refraction coefficient: 1 - 1/7.
pub const DEFAULT_CURVATURE: f64 = 0.85714;

/// More stations than this is a step typed in the wrong unit, not a sight
/// line anyone needs: 10 million samples of a raster is minutes of work.
const MAX_STATIONS: f64 = 1e7;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SightOptions {
    /// eye above the ground under the observer
    pub observer_height: f64,
    /// aim above the ground under the target
    pub target_height: f64,
    /// between stations, > 0
    pub step: f64,
    /// coefficient, >= 0; 0 off
    pub curvature: f64,
}

impl Default for SightOptions {
    fn default() -> Self {
        Self {
            observer_height: 1.7,
            target_height: 0.0,
            step: 1.0,
            curvature: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SightLine {
    pub visible: bool,
    /// plan, observer to target
    pub distance: f64,
    /// the eye
    pub observer_z: f64,
    /// the aim, curvature applied
    pub target_z: f64,
    /// The first station where the ground hides the target.
    pub blocked_at: Option<Point2>,
    /// from the observer
    pub blocked_distance: Option<f64>,
    /// the ground there, curvature applied
    pub blocked_ground: Option<f64>,
    /// The least clearance between the ends, where it is, and how far out;
    /// None when no station between them had ground.
    pub clearance: Option<f64>,
    pub clearance_at: Option<Point2>,
    pub clearance_distance: Option<f64>,
    /// between the ends
    pub stations: usize,
    /// of which off the ground
    pub unknown: usize,
}

impl Default for SightLine {
    fn default() -> Self {
        Self {
            visible: true,
            distance: 0.0,
            observer_z: 0.0,
            target_z: 0.0,
            blocked_at: None,
            blocked_distance: None,
            blocked_ground: None,
            clearance: None,
            clearance_at: None,
            clearance_distance: None,
            stations: 0,
            unknown: 0,
        }
    }
}

fn stop_requested(stop: Option<&AtomicBool>) -> bool {
    stop.is_some_and(|flag| flag.load(Ordering::Relaxed))
}

/// InvalidArgument for coincident ends, a step that is not positive and
/// finite (or makes more than 10 million stations), a curvature below 0 or
/// heights that are not finite, and - naming which - for an observer or a
/// target off the ground. InvalidState "cancelled" when `stop` is set (the
/// C++ std::stop_token).
pub fn line_of_sight(
    ground: GroundAt,
    observer: Point2,
    target: Point2,
    options: &SightOptions,
    stop: Option<&AtomicBool>,
) -> Result<SightLine> {
    if !options.observer_height.is_finite() || !options.target_height.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the heights must be numbers",
        ));
    }
    if !options.curvature.is_finite() || options.curvature < 0.0 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "curvature is a coefficient of 0 or more",
        ));
    }
    let dx = target.x - observer.x;
    let dy = target.y - observer.y;
    let mut line = SightLine {
        distance: dx.hypot(dy),
        ..SightLine::default()
    };
    if !(line.distance > 0.0) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the observer and the target are one point",
        ));
    }
    if !options.step.is_finite()
        || !(options.step > 0.0)
        || line.distance / options.step > MAX_STATIONS
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the step is a positive distance, no finer than a 10 millionth of the sight line",
        ));
    }
    let drop_at = |distance: f64| options.curvature * distance * distance / EARTH_DIAMETER;
    let Some(at_observer) = ground(observer) else {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the observer is off the ground",
        ));
    };
    let Some(at_target) = ground(target) else {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the target is off the ground",
        ));
    };
    line.observer_z = at_observer + options.observer_height;
    line.target_z = at_target - drop_at(line.distance) + options.target_height;

    let count = (line.distance / options.step).ceil() as usize;
    for i in 1..count {
        if (i & 0xFFF) == 0 && stop_requested(stop) {
            return Err(Error::new(ErrorCode::InvalidState, "cancelled"));
        }
        // Measured from the observer each time, never accumulated, so the
        // last station is as exact as the first.
        let t = i as f64 / count as f64;
        let at = Point2::new(observer.x + dx * t, observer.y + dy * t);
        let distance = line.distance * t;
        line.stations += 1;
        let Some(height) = ground(at) else {
            line.unknown += 1;
            continue;
        };
        let ground_z = height - drop_at(distance);
        let sight = line.observer_z + (line.target_z - line.observer_z) * t;
        let clearance = sight - ground_z;
        if line.clearance.is_none_or(|least| clearance < least) {
            line.clearance = Some(clearance);
            line.clearance_at = Some(at);
            line.clearance_distance = Some(distance);
        }
        if clearance < 0.0 && line.visible {
            line.visible = false;
            line.blocked_at = Some(at);
            line.blocked_distance = Some(distance);
            line.blocked_ground = Some(ground_z);
        }
    }
    if stop_requested(stop) {
        return Err(Error::new(ErrorCode::InvalidState, "cancelled"));
    }
    Ok(line)
}
