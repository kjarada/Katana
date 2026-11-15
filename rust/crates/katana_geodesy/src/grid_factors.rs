//! Grid scale factor and grid convergence of a projected CRS at a point - the
//! quantities a surveyor needs to move between grid and ground:
//!
//! ```text
//! grid distance  = ellipsoidal distance * point_scale_factor
//! grid bearing   = geodetic azimuth - convergence   (+ arc-to-chord, long lines)
//! ```
//!
//! Values come from PROJ's proj_factors(), which differentiates the projection
//! numerically (relative accuracy around 1e-9; see docs/geodesy.md).
//!
//! Threading: like `CoordinateTransformer`, a calculator owns a private PROJ
//! context, is not thread-safe (`at` takes `&mut self` for that reason) and is
//! given to another thread with `clone()`.

use std::fmt;

use katana_core::{Error, ErrorCode, Result};
use katana_math::numerics::{DEG_TO_RAD, RAD_TO_DEG, nearly_equal, tolerance};

use crate::coordinate::{GeographicCoordinate, ProjectedCoordinate, UNSPECIFIED_EPOCH};
use crate::coordinate_reference_system::{self, CoordinateReferenceSystem, CrsKind};
use crate::ffi;
use crate::proj_internal::{self, PjPtr, ProjContext};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridFactors {
    /// h: scale along the meridian.
    pub meridional_scale: f64,
    /// k: scale along the parallel.
    pub parallel_scale: f64,
    /// s: area distortion.
    pub areal_scale: f64,
    /// omega: maximum angular distortion.
    pub angular_distortion_degrees: f64,

    /// Grid convergence (gamma): the geodetic azimuth of GRID north, i.e. the
    /// angle at the point from true north clockwise to grid north. Positive
    /// east of the central meridian in the northern hemisphere, approximately
    /// (lon - lon0) * sin(lat).
    pub convergence_degrees: f64,
}

impl Default for GridFactors {
    fn default() -> Self {
        Self {
            meridional_scale: 1.0,
            parallel_scale: 1.0,
            areal_scale: 1.0,
            angular_distortion_degrees: 0.0,
            convergence_degrees: 0.0,
        }
    }
}

impl GridFactors {
    /// The projection is conformal (every survey grid is: Transverse Mercator,
    /// Lambert Conformal Conic, stereographic, oblique Mercator) when h == k
    /// and the angular distortion vanishes; the point scale factor is then the
    /// same in every direction and equals h. h is preferred over k because k
    /// divides by cos(latitude) and degrades towards the poles.
    pub fn point_scale_factor(&self) -> f64 {
        self.meridional_scale
    }
}

mod sealed {
    use katana_core::Result;

    use super::{GridFactorCalculator, GridFactors};

    pub trait Sealed {
        fn factors_on(&self, calculator: &mut GridFactorCalculator) -> Result<GridFactors>;
    }
}

/// A position `GridFactorCalculator::at` takes: geographic on the base CRS,
/// or a grid position, as the C++ overloads of `at` do.
pub trait GridPosition: sealed::Sealed {}

impl GridPosition for GeographicCoordinate {}
impl GridPosition for ProjectedCoordinate {}

impl sealed::Sealed for GeographicCoordinate {
    fn factors_on(&self, calculator: &mut GridFactorCalculator) -> Result<GridFactors> {
        calculator.at_geographic(self)
    }
}

impl sealed::Sealed for ProjectedCoordinate {
    fn factors_on(&self, calculator: &mut GridFactorCalculator) -> Result<GridFactors> {
        calculator.at_projected(self)
    }
}

pub struct GridFactorCalculator {
    // Rust drops fields in declaration order, the reverse of C++: the PJ
    // objects come first so that they are destroyed before their context.
    projected: PjPtr,     // the projected CRS (a BoundCRS wrapper removed)
    to_geographic: PjPtr, // projected -> base geographic CRS, normalised lon/lat order
    context: ProjContext,
    crs: CoordinateReferenceSystem,
    radians_per_geographic_unit: f64,
}

// SAFETY: as for CoordinateTransformer - the calculator owns its context and
// the PJs made in it together and shares them with nothing, so it may move to
// another thread, where it is used by that one thread; it is not Sync.
#[allow(
    unsafe_code,
    reason = "A calculator may move to another thread, as the C++'s clone-per-thread model needs."
)]
unsafe impl Send for GridFactorCalculator {}

impl fmt::Debug for GridFactorCalculator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GridFactorCalculator")
            .field("crs", &self.crs.name())
            .field(
                "radians_per_geographic_unit",
                &self.radians_per_geographic_unit,
            )
            .finish_non_exhaustive()
    }
}

impl GridFactorCalculator {
    /// `projected` must be of kind Projected (`InvalidArgument` otherwise).
    pub fn create(projected: &CoordinateReferenceSystem) -> Result<GridFactorCalculator> {
        if projected.kind() != CrsKind::Projected {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "grid factors are defined for projected CRSs only",
            )
            .with_context(format!(
                "crs='{}' kind={}",
                projected.name(),
                coordinate_reference_system::to_string(projected.kind())
            )));
        }

        let context = ProjContext::create()?;

        let object = proj_internal::create_crs(&context, projected.definition())?;
        let projected_object = if proj_internal::get_type(&object) == ffi::PJ_TYPE_BOUND_CRS {
            let source = proj_internal::get_source_crs(&context, &object);
            drop(object);
            source
        } else {
            Some(object)
        };

        // Inverse of the map projection alone: projected CRS -> its own base
        // geographic CRS. No datum change is involved, so there is exactly one
        // candidate operation.
        let geographic = projected_object
            .as_ref()
            .and_then(|crs| proj_internal::crs_get_geodetic_crs(&context, crs));
        let inverse_projection = match (&projected_object, &geographic) {
            (Some(crs), Some(base)) => {
                proj_internal::create_crs_to_crs_from_pj(&context, crs, base)
                    .and_then(|raw| proj_internal::normalize_for_visualization(&context, &raw))
            }
            _ => None,
        };
        // A derived operation implies both CRSs it was derived from.
        let (Some(projected_object), Some(geographic), Some(to_geographic)) =
            (projected_object, geographic, inverse_projection)
        else {
            return Err(Error::new(
                ErrorCode::InvalidCrs,
                "PROJ could not derive the base geographic CRS of the projected CRS",
            )
            .with_context(format!(
                "crs='{}'; {}",
                projected.name(),
                context.consume_diagnostics()
            )));
        };

        // Angular unit of the base CRS (grads for the French NTF systems).
        let system = proj_internal::crs_get_coordinate_system(&context, &geographic);
        let mut unit_to_radians = DEG_TO_RAD;
        let reported = system.as_ref().is_some_and(|system| {
            proj_internal::cs_get_axis_unit_factor(&context, system, 0, &mut unit_to_radians)
        });
        if !reported {
            return Err(Error::new(
                ErrorCode::InvalidCrs,
                "PROJ could not report the angular unit of the base geographic CRS",
            )
            .with_context(format!(
                "crs='{}'; {}",
                projected.name(),
                context.consume_diagnostics()
            )));
        }
        // Same snapping as the transformer: a degree is exactly DEG_TO_RAD.
        let radians_per_geographic_unit =
            if nearly_equal(unit_to_radians, DEG_TO_RAD, tolerance::RELATIVE) {
                DEG_TO_RAD
            } else {
                unit_to_radians
            };

        context.clear_diagnostics();
        Ok(GridFactorCalculator {
            projected: projected_object,
            to_geographic,
            context,
            crs: projected.clone(),
            radians_per_geographic_unit,
        })
    }

    #[allow(
        clippy::should_implement_trait,
        reason = "The C++ name; it fails, so it cannot be Clone."
    )]
    pub fn clone(&self) -> Result<GridFactorCalculator> {
        Self::create(&self.crs)
    }

    pub fn crs(&self) -> &CoordinateReferenceSystem {
        &self.crs
    }

    /// Factors at a position given on the BASE geographic CRS of the projected
    /// CRS (same datum; longitude relative to that CRS's prime meridian, which
    /// is Greenwich for practically every modern system), or at a grid
    /// position, whose projection is inverted first. `InvalidArgument` when
    /// the position is invalid or outside the domain of the projection.
    pub fn at<P: GridPosition>(&mut self, position: &P) -> Result<GridFactors> {
        P::factors_on(position, self)
    }

    fn at_geographic(&mut self, position: &GeographicCoordinate) -> Result<GridFactors> {
        if !position.latitude.is_finite()
            || !position.longitude.is_finite()
            || position.latitude.abs() > 90.0
        {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "geographic position must be finite with latitude in [-90, 90]",
            ));
        }
        self.evaluate(
            position.longitude * DEG_TO_RAD,
            position.latitude * DEG_TO_RAD,
        )
    }

    fn at_projected(&mut self, position: &ProjectedCoordinate) -> Result<GridFactors> {
        if !position.easting.is_finite() || !position.northing.is_finite() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "projected position must be finite",
            ));
        }

        self.context.clear_diagnostics();
        proj_internal::errno_reset(&self.to_geographic);
        let (x, y) = proj_internal::trans_xy(
            &self.to_geographic,
            ffi::PJ_FWD,
            [position.easting, position.northing, 0.0, UNSPECIFIED_EPOCH],
        );
        let proj_errno = proj_internal::errno(&self.to_geographic);
        if proj_errno != 0 || !x.is_finite() || !y.is_finite() {
            let reason = if proj_errno != 0 {
                proj_internal::describe_errno(&self.context, proj_errno)
            } else {
                String::from("PROJ returned no coordinate")
            };
            proj_internal::errno_reset(&self.to_geographic);
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "grid position cannot be converted to latitude/longitude",
            )
            .with_context(reason));
        }
        self.evaluate(
            x * self.radians_per_geographic_unit,
            y * self.radians_per_geographic_unit,
        )
    }

    fn evaluate(&mut self, longitude_radians: f64, latitude_radians: f64) -> Result<GridFactors> {
        self.context.clear_diagnostics();
        proj_internal::errno_reset(&self.projected);
        let factors = proj_internal::factors(&self.projected, longitude_radians, latitude_radians);
        let proj_errno = proj_internal::errno(&self.projected);

        // On failure PROJ returns an all-zero struct and sets the errno.
        if proj_errno != 0
            || !factors.meridional_scale.is_finite()
            || !(factors.meridional_scale > 0.0)
            || !factors.meridian_convergence.is_finite()
        {
            let mut reason = if proj_errno != 0 {
                proj_internal::describe_errno(&self.context, proj_errno)
            } else {
                String::from("PROJ returned no factors")
            };
            let log = self.context.consume_diagnostics();
            if !log.is_empty() {
                reason = format!("{reason}; {log}");
            }
            proj_internal::errno_reset(&self.projected);
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "grid factors cannot be evaluated at this position (outside the domain of the \
                 projection)",
            )
            .with_context(reason));
        }

        Ok(GridFactors {
            meridional_scale: factors.meridional_scale,
            parallel_scale: factors.parallel_scale,
            areal_scale: factors.areal_scale,
            angular_distortion_degrees: factors.angular_distortion * RAD_TO_DEG,
            convergence_degrees: factors.meridian_convergence * RAD_TO_DEG,
        })
    }
}
