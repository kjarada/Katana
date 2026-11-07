//! Local coordinates: 2D conformal (four-parameter Helmert / similarity)
//! transformation and its least-squares determination from control points -
//! the "site calibration" that ties a local engineering grid to a projected
//! CRS.
//!
//! ```text
//! X = tx + a x - b y          a = s cos(theta)
//! Y = ty + b x + a y          b = s sin(theta)
//! ```
//!
//! s is the scale and theta the rotation, COUNTER-CLOCKWISE positive in the
//! (x = easting, y = northing) plane, consistent with the rest of Katana. A
//! grid bearing (clockwise from north) therefore changes by -theta.
//!
//! Numerical design (see docs/geodesy.md for the derivation)
//! * The transformation is stored about two origins,
//!   target = target_origin + M (source - source_origin), M = [a -b; b a],
//!   which `fit_similarity_2d` places at the centroids of the control.
//!   Differences of neighbouring coordinates are formed first, so points at
//!   UTM magnitudes (1e6..1e7 m) keep their full precision: the transformed
//!   coordinate is accurate to about one unit in its last place (~1e-9 m).
//! * The least-squares solution is closed form; with centred coordinates the
//!   translation decouples and the 2x2 normal matrix is diagonal, so there is
//!   nothing to invert and no conditioning problem.
//! * Reflections are not modelled. Control with swapped axes (E/N confusion)
//!   shows up as residuals of the size of the site.

use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result};
use katana_math::Vec2;
use katana_math::numerics::tolerance;

/// The same ground mark in both systems. x = easting, y = northing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ControlPointPair {
    pub source: Vec2,
    pub target: Vec2,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SimilarityTransform2D {
    source_origin: Vec2,
    target_origin: Vec2,
    a: f64,
    b: f64,
}

/// The identity.
impl Default for SimilarityTransform2D {
    fn default() -> Self {
        Self {
            source_origin: Vec2::default(),
            target_origin: Vec2::default(),
            a: 1.0,
            b: 0.0,
        }
    }
}

// M v with M = [a -b; b a].
fn rotate_scale(a: f64, b: f64, v: Vec2) -> Vec2 {
    Vec2::new(a * v.x - b * v.y, b * v.x + a * v.y)
}

// std::max: the first unless it is less than the second.
fn std_max(first: f64, second: f64) -> f64 {
    if first < second { second } else { first }
}

impl SimilarityTransform2D {
    /// Fails with `InvalidArgument` unless scale is finite and > 0 and the
    /// other parameters are finite. `translation` is (tx, ty) of the equations
    /// above.
    pub fn from_parameters(
        scale: f64,
        rotation_radians: f64,
        translation: Vec2,
    ) -> Result<SimilarityTransform2D> {
        if !scale.is_finite()
            || scale <= 0.0
            || !rotation_radians.is_finite()
            || !translation.is_finite()
        {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "similarity parameters must be finite with a positive scale",
            ));
        }
        Self::from_coefficients(
            scale * rotation_radians.cos(),
            scale * rotation_radians.sin(),
            Vec2::default(),
            translation,
        )
    }

    /// Exact reconstruction of a stored transformation from `a()`, `b()` and
    /// the two origins (persisting scale/rotation/translation instead would
    /// not round trip bit-exactly). `InvalidArgument` unless everything is
    /// finite and (a, b) != (0, 0).
    pub fn from_coefficients(
        a: f64,
        b: f64,
        source_origin: Vec2,
        target_origin: Vec2,
    ) -> Result<SimilarityTransform2D> {
        let finite = a.is_finite()
            && b.is_finite()
            && source_origin.is_finite()
            && target_origin.is_finite();
        if !finite || (a == 0.0 && b == 0.0) {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "similarity coefficients must be finite and not both zero",
            ));
        }
        Ok(SimilarityTransform2D {
            source_origin,
            target_origin,
            a,
            b,
        })
    }

    pub fn apply(&self, source: Vec2) -> Vec2 {
        // Difference first: for nearby points it is (nearly) exact, and only
        // the small site-sized vector is scaled and rotated.
        self.target_origin + rotate_scale(self.a, self.b, source - self.source_origin)
    }

    pub fn apply_inverse(&self, target: Vec2) -> Vec2 {
        // M^-1 = [a b; -b a] / (a^2 + b^2).
        let determinant = self.a * self.a + self.b * self.b;
        self.source_origin
            + rotate_scale(
                self.a / determinant,
                -self.b / determinant,
                target - self.target_origin,
            )
    }

    pub fn inverse(&self) -> SimilarityTransform2D {
        let determinant = self.a * self.a + self.b * self.b;
        SimilarityTransform2D {
            source_origin: self.target_origin,
            target_origin: self.source_origin,
            a: self.a / determinant,
            b: -self.b / determinant,
        }
    }

    pub fn scale(&self) -> f64 {
        self.a.hypot(self.b)
    }

    /// Radians in (-pi, pi], counter-clockwise.
    pub fn rotation(&self) -> f64 {
        self.b.atan2(self.a)
    }

    pub fn a(&self) -> f64 {
        self.a
    }

    pub fn b(&self) -> f64 {
        self.b
    }

    /// (tx, ty): the image of the source origin (0, 0). For display and
    /// export; at UTM magnitudes it absorbs the rounding of M * source_origin
    /// (~1e-9 m), which `apply` itself avoids.
    pub fn translation(&self) -> Vec2 {
        self.target_origin - rotate_scale(self.a, self.b, self.source_origin)
    }

    pub fn source_origin(&self) -> Vec2 {
        self.source_origin
    }

    pub fn target_origin(&self) -> Vec2 {
        self.target_origin
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SimilarityFit2D {
    pub transform: SimilarityTransform2D,

    /// `residuals[i] = pairs[i].target - transform.apply(pairs[i].source)`, in
    /// target units: what must be added to a transformed point to reach its
    /// control value.
    pub residuals: Vec<Vec2>,

    /// sqrt( sum |residual|^2 / n ), horizontal, target units.
    pub rms: f64,
    /// Largest |residual|.
    pub max_residual: f64,

    /// A-posteriori standard deviation of one coordinate,
    /// sqrt( sum |residual|^2 / (2n - 4) ). `None` for n == 2, where the four
    /// parameters fit exactly and nothing is left to judge the control by.
    pub coordinate_std_dev: Option<f64>,
}

/// Equal-weight least-squares fit. Errors (all `InvalidArgument`):
/// * fewer than two pairs, or a non-finite coordinate;
/// * source (or target) points that all coincide within
///   `tolerance::COORDINATE` - scale and rotation are then undetermined;
/// * control that fits no proper similarity (best scale collapses to zero,
///   typically mirrored point sets).
pub fn fit_similarity_2d(pairs: &[ControlPointPair]) -> Result<SimilarityFit2D> {
    let count = pairs.len();
    if count < 2 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a 2D similarity fit needs at least two control point pairs",
        )
        .with_context(format!("pairs={count}")));
    }
    for (index, pair) in pairs.iter().enumerate() {
        if !pair.source.is_finite() || !pair.target.is_finite() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "control point coordinates must be finite",
            )
            .with_context(format!("pair index={index}")));
        }
    }
    let n = count as f64;
    let first = pairs[0];

    // Origins at the centroids. The centroid is accumulated relative to the
    // first point so that the sums hold site-sized numbers, not 1e7-sized
    // ones.
    let mut source_mean = Vec2::default();
    let mut target_mean = Vec2::default();
    for pair in pairs {
        source_mean += pair.source - first.source;
        target_mean += pair.target - first.target;
    }
    let source_origin = first.source + source_mean / n;
    let target_origin = first.target + target_mean / n;

    // The stored origins are rounded to the coordinate grid (~1e-9 m), so the
    // reduced coordinates do not sum to exactly zero. Remove what is left; it
    // is folded back into the target origin below.
    let mut source_residual_mean = Vec2::default();
    let mut target_residual_mean = Vec2::default();
    for pair in pairs {
        source_residual_mean += pair.source - source_origin;
        target_residual_mean += pair.target - target_origin;
    }
    source_residual_mean /= n;
    target_residual_mean /= n;

    // Normal equations in centred coordinates (x, y) -> (X, Y):
    //   a = sum(x X + y Y) / sum(x^2 + y^2),  b = sum(x Y - y X) / sum(x^2 + y^2).
    let mut sum_squares = 0.0;
    let mut sum_dot = 0.0;
    let mut sum_cross = 0.0;
    let mut source_spread = 0.0;
    let mut target_spread = 0.0;
    for pair in pairs {
        let s = (pair.source - source_origin) - source_residual_mean;
        let t = (pair.target - target_origin) - target_residual_mean;
        sum_squares += s.length_squared();
        sum_dot += s.dot(t);
        sum_cross += s.cross(t);
        source_spread = std_max(source_spread, s.length());
        target_spread = std_max(target_spread, t.length());
    }
    if source_spread <= tolerance::COORDINATE || target_spread <= tolerance::COORDINATE {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "control points coincide; scale and rotation are undetermined",
        )
        .with_context(format!(
            "source spread={} target spread={}",
            format_exact_real(source_spread),
            format_exact_real(target_spread)
        )));
    }

    let a = sum_dot / sum_squares;
    let b = sum_cross / sum_squares;
    if a.hypot(b) * source_spread <= tolerance::COORDINATE {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "control points fit no proper similarity (best scale is zero); check for mirrored \
             or mismatched point pairs",
        ));
    }

    let transform = SimilarityTransform2D::from_coefficients(
        a,
        b,
        source_origin,
        target_origin + (target_residual_mean - rotate_scale(a, b, source_residual_mean)),
    )?;

    let mut fit = SimilarityFit2D {
        transform,
        residuals: Vec::with_capacity(count),
        ..SimilarityFit2D::default()
    };
    let mut sum_residual_squares = 0.0;
    for pair in pairs {
        let residual = pair.target - fit.transform.apply(pair.source);
        sum_residual_squares += residual.length_squared();
        fit.max_residual = std_max(fit.max_residual, residual.length());
        fit.residuals.push(residual);
    }
    fit.rms = (sum_residual_squares / n).sqrt();
    if count > 2 {
        fit.coordinate_std_dev = Some((sum_residual_squares / (2.0 * n - 4.0)).sqrt());
    }
    Ok(fit)
}
