//! The single entry point for coordinate transformations between two CRSs
//! ("All CRS transformations must pass through a centralized API"). PROJ is an
//! implementation detail: no PROJ type appears here.
//!
//! What `create` does
//!   PROJ is asked for every candidate operation between the two CRSs, ranked
//!   by area of use, accuracy and grid availability. Katana selects the FIRST
//!   candidate that can actually run on this installation and pins it: the
//!   same operation is applied to every point, and it is described by
//!   `operation()`. Candidates that were passed over because a grid file is
//!   missing are listed in `candidates()`; `missing_grids()` says which files
//!   would unlock a better one.
//!
//! Ballpark transformations
//!   When PROJ knows no datum transformation between two datums it can fall
//!   back to a "ballpark" operation that applies NO datum shift at all (errors
//!   of up to hundreds of metres). Surveying cannot tolerate that silently, so
//!   `create` REFUSES ballpark operations unless
//!   `TransformerOptions::allow_ballpark` is set.
//!
//! Axis order and units: see the `coordinate` module. Named coordinate types
//! are checked against the kind of the CRS at run time (a
//! `GeographicCoordinate` handed to a transformer whose source CRS is
//! projected is an `InvalidArgument` error).
//!
//! Threading: a transformer owns a private PROJ context and is NOT
//! thread-safe, not even for concurrent calls of the same transformer on
//! different data - which is why the transform functions take `&mut self`.
//! Use one transformer per thread; `clone()` produces an independent,
//! identically configured instance, which may be moved to another thread
//! (`Send`, not `Sync`). Distinct transformers never share state.
//!
//! The C++ overloads have names of their own here: `forward`/`inverse` take
//! one `Coordinate`, `forward_batch`/`inverse_batch` a slice of them, and
//! `forward_as`/`inverse_as` and `forward_as_batch`/`inverse_as_batch` the
//! named types. There is no moved-from transformer in Rust, so none of the
//! C++'s moved-from checks has a twin.

use std::fmt;

use katana_core::{Error, ErrorCode, Result, Status};
use katana_math::numerics::{DEG_TO_RAD, nearly_equal, tolerance};

use crate::coordinate::{
    BatchReport, Coordinate, GeographicCoordinate, ProjectedCoordinate, UNSPECIFIED_EPOCH,
};
use crate::coordinate_reference_system::{
    self, CoordinateReferenceSystem, CrsKind, GeographicExtent,
};
use crate::ffi::{self, PJ_FWD, PJ_INV, PjDirection};
use crate::proj_internal::{self, PjPtr, ProjContext};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TransformerOptions {
    /// Region the data lies in. Strongly recommended for datum
    /// transformations: it lets PROJ rank the operation that is valid THERE
    /// first (e.g. the right national grid) instead of the one with the widest
    /// coverage.
    pub area_of_interest: Option<GeographicExtent>,

    /// Accept an operation containing a ballpark (no datum shift) step.
    pub allow_ballpark: bool,

    /// Reject candidates whose declared accuracy is worse than this (metres).
    pub required_accuracy_metres: Option<f64>,

    /// Coordinate epoch (decimal year, e.g. 2024.5) assumed for every
    /// coordinate that carries none: all named coordinates, and Coordinates
    /// whose t is `UNSPECIFIED_EPOCH`. Time-dependent operations
    /// (`OperationInfo::requires_epoch`, e.g. ITRF2014 -> ETRF2000) REFUSE
    /// coordinates without an epoch: PROJ would otherwise silently assume the
    /// reference epoch of the operation, which is wrong by the plate motion
    /// accumulated since (about 2.5 cm per year).
    pub coordinate_epoch: Option<f64>,
}

/// A grid (datum shift / geoid) file referenced by an operation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GridFile {
    /// E.g. "us_noaa_conus.tif".
    pub name: String,
    /// Where PROJ says it can be downloaded; may be empty.
    pub url: String,
    pub available: bool,
}

/// Description of one candidate coordinate operation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OperationInfo {
    /// E.g. "Inverse of OSGB36 to WGS 84 (6) + British National Grid".
    pub name: String,
    /// PROJ pipeline text; empty when PROJ cannot express it.
    pub proj_pipeline: String,
    /// Human readable region of validity.
    pub area_of_use: String,
    /// As declared by the authority; 0 for a conversion (a map projection or
    /// unit change is error free by definition); `None` when genuinely
    /// unknown, which is what a ballpark operation reports.
    pub accuracy_metres: Option<f64>,
    /// Contains a step without datum shift.
    pub is_ballpark: bool,
    /// Instantiable here: every required grid is installed.
    pub is_usable: bool,
    /// Time-dependent: each coordinate needs an epoch.
    pub requires_epoch: bool,
    /// Grids the operation needs.
    pub grids: Vec<GridFile>,
}

pub(crate) mod sealed {
    /// The plumbing of the two named coordinate types. x and y are PROJ's
    /// first and second coordinate in normalised (traditional GIS) order:
    /// longitude or easting first.
    pub trait Sealed: Copy + Default {
        const IS_GEOGRAPHIC: bool;
        const TYPE_NAME: &'static str;
        // Byte offsets of x, y and the height: the batch hands PROJ interior
        // pointers at them, with the size of the type as the stride.
        const X_OFFSET: usize;
        const Y_OFFSET: usize;
        const HEIGHT_OFFSET: usize;

        fn x_of(&self) -> f64;
        fn y_of(&self) -> f64;
        fn height_of(&self) -> f64;
        fn set_x(&mut self, value: f64);
        fn set_y(&mut self, value: f64);
        fn set_height(&mut self, value: f64);
        fn is_valid_input(&self) -> bool;
    }
}

/// `GeographicCoordinate` or `ProjectedCoordinate`: the types `forward_as` and
/// its relatives take and give.
pub trait NamedCoordinate: sealed::Sealed {}

impl NamedCoordinate for GeographicCoordinate {}
impl NamedCoordinate for ProjectedCoordinate {}

impl sealed::Sealed for GeographicCoordinate {
    const IS_GEOGRAPHIC: bool = true;
    const TYPE_NAME: &'static str = "GeographicCoordinate";
    const X_OFFSET: usize = std::mem::offset_of!(GeographicCoordinate, longitude);
    const Y_OFFSET: usize = std::mem::offset_of!(GeographicCoordinate, latitude);
    const HEIGHT_OFFSET: usize = std::mem::offset_of!(GeographicCoordinate, height);

    fn x_of(&self) -> f64 {
        self.longitude
    }
    fn y_of(&self) -> f64 {
        self.latitude
    }
    fn height_of(&self) -> f64 {
        self.height
    }
    fn set_x(&mut self, value: f64) {
        self.longitude = value;
    }
    fn set_y(&mut self, value: f64) {
        self.latitude = value;
    }
    fn set_height(&mut self, value: f64) {
        self.height = value;
    }
    fn is_valid_input(&self) -> bool {
        self.latitude.is_finite()
            && self.longitude.is_finite()
            && self.height.is_finite()
            && self.latitude.abs() <= 90.0
    }
}

impl sealed::Sealed for ProjectedCoordinate {
    const IS_GEOGRAPHIC: bool = false;
    const TYPE_NAME: &'static str = "ProjectedCoordinate";
    const X_OFFSET: usize = std::mem::offset_of!(ProjectedCoordinate, easting);
    const Y_OFFSET: usize = std::mem::offset_of!(ProjectedCoordinate, northing);
    const HEIGHT_OFFSET: usize = std::mem::offset_of!(ProjectedCoordinate, height);

    fn x_of(&self) -> f64 {
        self.easting
    }
    fn y_of(&self) -> f64 {
        self.northing
    }
    fn height_of(&self) -> f64 {
        self.height
    }
    fn set_x(&mut self, value: f64) {
        self.easting = value;
    }
    fn set_y(&mut self, value: f64) {
        self.northing = value;
    }
    fn set_height(&mut self, value: f64) {
        self.height = value;
    }
    fn is_valid_input(&self) -> bool {
        self.easting.is_finite() && self.northing.is_finite() && self.height.is_finite()
    }
}

// Number of candidate operations spelled out in an error message.
const MAX_CANDIDATES_IN_ERROR: usize = 4;

fn describe_operation(context: &ProjContext, operation: &PjPtr) -> OperationInfo {
    let mut info = OperationInfo {
        name: proj_internal::get_name(operation),
        ..OperationInfo::default()
    };

    let accuracy = proj_internal::coordoperation_get_accuracy(context, operation);
    if accuracy >= 0.0 {
        // PROJ reports a negative value for "unknown".
        info.accuracy_metres = Some(accuracy);
    } else if proj_internal::get_type(operation) == ffi::PJ_TYPE_CONVERSION {
        // A conversion relates two CRSs that share a datum by an exact
        // formula - a map projection, a unit or axis change - so it introduces
        // no positional uncertainty (ISO 19111; EPSG Guidance Note 7-2 calls a
        // conversion error free, which is why the EPSG registry declares no
        // accuracy for one and PROJ's C API answers "unknown"). The declared
        // accuracy is nevertheless zero, and reporting None here would make
        // "exact" indistinguishable from a ballpark operation, whose error
        // genuinely is unknown.
        info.accuracy_metres = Some(0.0);
    }
    info.is_ballpark =
        proj_internal::coordoperation_has_ballpark_transformation(context, operation);
    info.is_usable = proj_internal::coordoperation_is_instantiable(context, operation);
    info.requires_epoch =
        proj_internal::coordoperation_requires_per_coordinate_input_time(context, operation);

    if let Some(area) = proj_internal::get_area_of_use_name(context, operation) {
        info.area_of_use = area;
    }

    let grid_count = proj_internal::coordoperation_get_grid_used_count(context, operation);
    for index in 0..grid_count {
        if let Some(grid) = proj_internal::coordoperation_get_grid_used(context, operation, index) {
            info.grids.push(grid);
        }
    }

    info.proj_pipeline = proj_internal::as_proj_string(context, operation);
    context.clear_diagnostics(); // probing unusable candidates is expected to log
    info
}

fn describe_candidates(candidates: &[OperationInfo]) -> String {
    let mut text = String::new();
    let shown = candidates.len().min(MAX_CANDIDATES_IN_ERROR);
    for candidate in &candidates[..shown] {
        text.push_str("candidate '");
        text.push_str(&candidate.name);
        text.push('\'');
        for grid in &candidate.grids {
            if !grid.available {
                text.push_str(" needs missing grid ");
                text.push_str(&grid.name);
                if !grid.url.is_empty() {
                    text.push_str(" (");
                    text.push_str(&grid.url);
                    text.push(')');
                }
            }
        }
        text.push_str("; ");
    }
    if candidates.len() > shown {
        text = format!("{text}{} more candidate(s); ", candidates.len() - shown);
    }
    text
}

fn validate_area_of_interest(area: &GeographicExtent) -> Status {
    let finite = area.west.is_finite()
        && area.south.is_finite()
        && area.east.is_finite()
        && area.north.is_finite();
    if !finite
        || area.south > area.north
        || area.south < -90.0
        || area.north > 90.0
        || area.west.abs() > 180.0
        || area.east.abs() > 180.0
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "area of interest must be finite degrees with south <= north, latitudes in [-90, 90] \
             and longitudes in [-180, 180]",
        ));
    }
    Ok(())
}

// CRS angular units per degree: exactly 1 for the (overwhelmingly common)
// degree-based CRS so that such coordinates pass through bit-exactly; e.g.
// 10/9 for the grad-based French NTF systems.
fn units_per_degree(crs: &CoordinateReferenceSystem) -> f64 {
    if !crs.is_geographic()
        || nearly_equal(crs.horizontal_unit_to_si(), DEG_TO_RAD, tolerance::RELATIVE)
    {
        return 1.0;
    }
    DEG_TO_RAD / crs.horizontal_unit_to_si()
}

fn check_kind<N: NamedCoordinate>(crs: &CoordinateReferenceSystem, role: &str) -> Status {
    let matches = if N::IS_GEOGRAPHIC {
        crs.is_geographic()
    } else {
        crs.is_projected()
    };
    if matches {
        return Ok(());
    }
    Err(Error::new(
        ErrorCode::InvalidArgument,
        format!("{} does not match the {role} CRS", N::TYPE_NAME),
    )
    .with_context(format!(
        "crs='{}' kind={}",
        crs.name(),
        coordinate_reference_system::to_string(crs.kind())
    )))
}

fn check_input<N: NamedCoordinate>(coordinate: &N) -> Status {
    if N::is_valid_input(coordinate) {
        return Ok(());
    }
    Err(Error::new(
        ErrorCode::InvalidArgument,
        if N::IS_GEOGRAPHIC {
            "geographic coordinate must be finite with latitude in [-90, 90]"
        } else {
            "projected coordinate must be finite"
        },
    ))
}

fn failed_point<N: NamedCoordinate>() -> N {
    let mut failed = N::default();
    N::set_x(&mut failed, f64::NAN);
    N::set_y(&mut failed, f64::NAN);
    N::set_height(&mut failed, f64::NAN);
    failed
}

// Missing grids of candidates that are MORE accurate than the selected one (or
// of any candidate when its accuracy is unknown), each named once.
fn collect_missing_grids(candidates: &[OperationInfo], selected: &OperationInfo) -> Vec<GridFile> {
    let mut missing: Vec<GridFile> = Vec::new();
    for candidate in candidates {
        let more_accurate = match (candidate.accuracy_metres, selected.accuracy_metres) {
            (_, None) => true,
            (Some(candidate_accuracy), Some(selected_accuracy)) => {
                candidate_accuracy < selected_accuracy
            }
            (None, Some(_)) => false,
        };
        if candidate.is_usable || !more_accurate {
            continue;
        }
        for grid in &candidate.grids {
            let known = missing.iter().any(|g| g.name == grid.name);
            if !grid.available && !known {
                missing.push(grid.clone());
            }
        }
    }
    missing
}

pub struct CoordinateTransformer {
    // Rust drops fields in declaration order, the reverse of C++: the PJ comes
    // first so that it is destroyed before its context.
    operation: PjPtr, // the selected candidate, normalised to traditional GIS order
    context: ProjContext,

    source: CoordinateReferenceSystem,
    target: CoordinateReferenceSystem,
    options: TransformerOptions,

    selected: OperationInfo,
    candidates: Vec<OperationInfo>,
    missing_grids: Vec<GridFile>,

    source_units_per_degree: f64,
    target_units_per_degree: f64,
}

// SAFETY: PROJ lets a context, and every PJ made in it, be used from any
// thread, one thread at a time. A transformer owns its context and its one PJ
// together, shares them with nothing (no clone of a handle exists; `clone()`
// builds new ones), and moving it moves both. It is not Sync: every call that
// reaches PROJ takes `&mut self`, so a shared reference never does - but the
// C++ promises no more than one thread per transformer, and neither does this.
#[allow(
    unsafe_code,
    reason = "A transformer may move to another thread, as the C++'s one-transformer-per-thread model needs."
)]
unsafe impl Send for CoordinateTransformer {}

impl fmt::Debug for CoordinateTransformer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CoordinateTransformer")
            .field("source", &self.source.name())
            .field("target", &self.target.name())
            .field("operation", &self.selected.name)
            .finish_non_exhaustive()
    }
}

impl CoordinateTransformer {
    /// Fails with
    /// * `Unsupported` - a LocalEngineering CRS is involved, no operation
    ///   exists, or the only operations are ballpark / need missing grids
    ///   (`Error::context` lists the grids);
    /// * `InvalidCrs` - a CRS definition no longer instantiates;
    /// * `NotFound` - proj.db is not installed.
    pub fn create(
        source: &CoordinateReferenceSystem,
        target: &CoordinateReferenceSystem,
        options: &TransformerOptions,
    ) -> Result<CoordinateTransformer> {
        let pair_text = format!("source='{}' target='{}'", source.name(), target.name());

        if source.kind() == CrsKind::LocalEngineering || target.kind() == CrsKind::LocalEngineering
        {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "a local engineering CRS has no geodetic datum and cannot be transformed by \
                 PROJ; relate it to a projected CRS with a site calibration (fitSimilarity2D)",
            )
            .with_context(pair_text));
        }
        if let Some(area) = &options.area_of_interest {
            validate_area_of_interest(area)?;
        }
        if let Some(accuracy) = options.required_accuracy_metres
            && !(accuracy.is_finite() && accuracy > 0.0)
        {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "required accuracy must be a positive number of metres",
            ));
        }

        if let Some(epoch) = options.coordinate_epoch
            && !epoch.is_finite()
        {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "coordinate epoch must be a finite decimal year",
            ));
        }

        let context = ProjContext::create()?;

        let source_crs = proj_internal::create_crs(&context, source.definition())?;
        let target_crs = proj_internal::create_crs(&context, target.definition())?;

        let Some(factory) = proj_internal::create_operation_factory_context(&context) else {
            return Err(Error::new(
                ErrorCode::Internal,
                "PROJ could not create an operation factory",
            )
            .with_context(context.consume_diagnostics()));
        };
        // Partial intersection keeps operations whose area of use covers only
        // part of the CRS extents (most national datum transformations).
        // Operations with missing grids stay in the list - ranked after the
        // usable ones - so that they can be reported instead of silently
        // disappearing.
        proj_internal::set_spatial_criterion(
            &context,
            &factory,
            ffi::PROJ_SPATIAL_CRITERION_PARTIAL_INTERSECTION,
        );
        proj_internal::set_grid_availability_use(
            &context,
            &factory,
            ffi::PROJ_GRID_AVAILABILITY_USED_FOR_SORTING,
        );
        proj_internal::set_allow_ballpark_transformations(
            &context,
            &factory,
            options.allow_ballpark,
        );
        if let Some(area) = &options.area_of_interest {
            proj_internal::set_area_of_interest(
                &context,
                &factory,
                [area.west, area.south, area.east, area.north],
            );
        }
        if let Some(accuracy) = options.required_accuracy_metres {
            proj_internal::set_desired_accuracy(&context, &factory, accuracy);
        }

        context.clear_diagnostics();
        let Some(operations) =
            proj_internal::create_operations(&context, &source_crs, &target_crs, &factory)
        else {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "PROJ could not search for coordinate operations",
            )
            .with_context(format!("{pair_text}; {}", context.consume_diagnostics())));
        };

        let mut candidates: Vec<OperationInfo> = Vec::new();
        let mut selected = OperationInfo::default();
        let mut chosen: Option<PjPtr> = None;
        let count = proj_internal::list_get_count(&operations);
        for index in 0..count {
            let Some(candidate) = proj_internal::list_get(&context, &operations, index) else {
                continue;
            };
            let info = describe_operation(&context, &candidate);
            if chosen.is_none() && info.is_usable {
                selected = info.clone();
                chosen = Some(candidate);
            }
            candidates.push(info);
        }

        let Some(chosen) = chosen else {
            let mut reason = format!("{pair_text}; {}", describe_candidates(&candidates));
            if !options.allow_ballpark {
                reason.push_str(
                    "ballpark operations (no datum shift) are disabled; set \
                     TransformerOptions::allowBallpark to accept one",
                );
            }
            return Err(Error::new(
                ErrorCode::Unsupported,
                "no usable coordinate operation between the two CRSs",
            )
            .with_context(reason));
        };

        context.clear_diagnostics();
        let Some(operation) = proj_internal::normalize_for_visualization(&context, &chosen) else {
            return Err(Error::new(
                ErrorCode::Internal,
                "PROJ could not normalise the axis order of the operation",
            )
            .with_context(format!("{pair_text}; {}", context.consume_diagnostics())));
        };
        // Report the pipeline that really runs, axis swaps and unit conversions
        // included.
        selected.proj_pipeline = proj_internal::as_proj_string(&context, &operation);

        let missing_grids = collect_missing_grids(&candidates, &selected);
        let source_units_per_degree = units_per_degree(source);
        let target_units_per_degree = units_per_degree(target);
        context.clear_diagnostics();
        Ok(CoordinateTransformer {
            operation,
            context,
            source: source.clone(),
            target: target.clone(),
            options: options.clone(),
            selected,
            candidates,
            missing_grids,
            source_units_per_degree,
            target_units_per_degree,
        })
    }

    /// Independent transformer with the same CRSs and options (its own PROJ
    /// context). This is how a transformer is handed to another thread.
    #[allow(
        clippy::should_implement_trait,
        reason = "The C++ name; it fails, so it cannot be Clone."
    )]
    pub fn clone(&self) -> Result<CoordinateTransformer> {
        // Re-running the (deterministic) selection in a fresh context is the
        // only way to obtain PROJ objects that share nothing with this
        // instance.
        Self::create(&self.source, &self.target, &self.options)
    }

    pub fn source(&self) -> &CoordinateReferenceSystem {
        &self.source
    }

    pub fn target(&self) -> &CoordinateReferenceSystem {
        &self.target
    }

    /// The operation every transform call applies.
    pub fn operation(&self) -> &OperationInfo {
        &self.selected
    }

    /// All candidates in PROJ's ranking order, usable or not.
    pub fn candidates(&self) -> &[OperationInfo] {
        &self.candidates
    }

    /// Missing grids of candidates that are MORE accurate than `operation()`
    /// (or of any candidate when the accuracy of `operation()` is unknown).
    /// Empty means no better result can be had by installing data.
    pub fn missing_grids(&self) -> &[GridFile] {
        &self.missing_grids
    }

    // ---- single point, generic (traditional GIS order, CRS units) --------------

    /// Errors: `InvalidArgument` for non-finite input or a coordinate PROJ
    /// rejects (outside the projection domain, outside a grid, latitude beyond
    /// the pole); the PROJ message is in `Error::context`. A failed point
    /// never yields numbers.
    pub fn forward(&mut self, coordinate: Coordinate) -> Result<Coordinate> {
        self.transform_one(PJ_FWD, &coordinate)
    }

    pub fn inverse(&mut self, coordinate: Coordinate) -> Result<Coordinate> {
        self.transform_one(PJ_INV, &coordinate)
    }

    // ---- single point, named types ------------------------------------------------

    /// `transformer.forward_as::<ProjectedCoordinate, _>(GeographicCoordinate::new(lat, lon, 0.0))`.
    /// The source type must match the kind of the CRS the point is expressed
    /// in and the target type the kind of the CRS it is converted to,
    /// otherwise `InvalidArgument`. Geographic values are degrees whatever the
    /// angular unit of the CRS; latitude must lie within [-90, 90].
    pub fn forward_as<T: NamedCoordinate, S: NamedCoordinate>(
        &mut self,
        coordinate: S,
    ) -> Result<T> {
        self.transform_named(PJ_FWD, &coordinate)
    }

    pub fn inverse_as<T: NamedCoordinate, S: NamedCoordinate>(
        &mut self,
        coordinate: S,
    ) -> Result<T> {
        self.transform_named(PJ_INV, &coordinate)
    }

    // ---- batch, in place ------------------------------------------------------------

    /// One PROJ call (proj_trans_generic) for the whole slice. Results are
    /// bit-identical to calling `forward`/`inverse` per point. Failed points
    /// get x = y = z = NaN, t = `UNSPECIFIED_EPOCH` and are counted in the
    /// report; the Result itself never fails in Rust, where no transformer is
    /// moved-from (the C++ fails then).
    pub fn forward_batch(&mut self, coordinates: &mut [Coordinate]) -> Result<BatchReport> {
        Ok(self.transform_many(PJ_FWD, coordinates))
    }

    pub fn inverse_batch(&mut self, coordinates: &mut [Coordinate]) -> Result<BatchReport> {
        Ok(self.transform_many(PJ_INV, coordinates))
    }

    // ---- batch, named types -----------------------------------------------------------

    /// `output` must have the length of `input` (`InvalidArgument` otherwise).
    /// Failed points have every field set to NaN.
    pub fn forward_as_batch<T: NamedCoordinate, S: NamedCoordinate>(
        &mut self,
        input: &[S],
        output: &mut [T],
    ) -> Result<BatchReport> {
        self.transform_named_many(PJ_FWD, input, output)
    }

    pub fn inverse_as_batch<T: NamedCoordinate, S: NamedCoordinate>(
        &mut self,
        input: &[S],
        output: &mut [T],
    ) -> Result<BatchReport> {
        self.transform_named_many(PJ_INV, input, output)
    }

    // ---- the work -------------------------------------------------------------------

    fn crs_from(&self, direction: PjDirection) -> &CoordinateReferenceSystem {
        if direction == PJ_FWD {
            &self.source
        } else {
            &self.target
        }
    }

    fn crs_to(&self, direction: PjDirection) -> &CoordinateReferenceSystem {
        if direction == PJ_FWD {
            &self.target
        } else {
            &self.source
        }
    }

    fn units_per_degree_from(&self, direction: PjDirection) -> f64 {
        if direction == PJ_FWD {
            self.source_units_per_degree
        } else {
            self.target_units_per_degree
        }
    }

    fn units_per_degree_to(&self, direction: PjDirection) -> f64 {
        if direction == PJ_FWD {
            self.target_units_per_degree
        } else {
            self.source_units_per_degree
        }
    }

    // Epoch handed to PROJ: the coordinate's own, else the configured default.
    fn effective_epoch(&self, t: f64) -> f64 {
        match self.options.coordinate_epoch {
            Some(epoch) if t == UNSPECIFIED_EPOCH => epoch,
            _ => t,
        }
    }

    // Shared by the single-point and batch paths so that both refuse exactly
    // the same inputs. `epoch` is the effective epoch.
    fn is_transformable(&self, coordinate: &Coordinate, epoch: f64) -> bool {
        // +infinity is UNSPECIFIED_EPOCH; NaN and -infinity are never valid.
        let epoch_valid = !epoch.is_nan() && epoch != f64::NEG_INFINITY;
        let epoch_present = !self.selected.requires_epoch || epoch != UNSPECIFIED_EPOCH;
        coordinate.x.is_finite()
            && coordinate.y.is_finite()
            && coordinate.z.is_finite()
            && epoch_valid
            && epoch_present
    }

    fn transform_one(
        &mut self,
        direction: PjDirection,
        coordinate: &Coordinate,
    ) -> Result<Coordinate> {
        let epoch = self.effective_epoch(coordinate.t);
        if !self.is_transformable(coordinate, epoch) {
            if self.selected.requires_epoch && epoch == UNSPECIFIED_EPOCH {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "the operation is time-dependent and the coordinate has no epoch; set \
                     Coordinate::t or TransformerOptions::coordinateEpoch",
                )
                .with_context(format!("operation='{}'", self.selected.name)));
            }
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "coordinate to transform must be finite",
            ));
        }

        self.context.clear_diagnostics();
        proj_internal::errno_reset(&self.operation);
        let [x, y, z, t] = proj_internal::trans(
            &self.operation,
            direction,
            [coordinate.x, coordinate.y, coordinate.z, epoch],
        );
        let proj_errno = proj_internal::errno(&self.operation);

        // PROJ signals failure with HUGE_VAL (infinity) coordinates and/or an
        // errno. Either one means there is no trustworthy number to hand back.
        if proj_errno != 0 || !x.is_finite() || !y.is_finite() || !z.is_finite() {
            let mut reason = if proj_errno != 0 {
                proj_internal::describe_errno(&self.context, proj_errno)
            } else {
                String::from("PROJ returned no coordinate")
            };
            let log = self.context.consume_diagnostics();
            if !log.is_empty() {
                reason = format!("{reason}; {log}");
            }
            reason = format!("{reason}; operation='{}'", self.selected.name);
            proj_internal::errno_reset(&self.operation);
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "coordinate cannot be transformed (outside the domain of the operation or of its \
                 grids)",
            )
            .with_context(reason));
        }
        Ok(Coordinate { x, y, z, t })
    }

    fn transform_many(
        &mut self,
        direction: PjDirection,
        coordinates: &mut [Coordinate],
    ) -> BatchReport {
        let mut report = BatchReport::default();
        if coordinates.is_empty() {
            return report;
        }

        // Refuse what transform_one() refuses. PROJ answers an x of HUGE_VAL
        // with its error coordinate without evaluating anything, which the
        // loop below turns into a counted failure.
        for coordinate in coordinates.iter_mut() {
            coordinate.t = self.effective_epoch(coordinate.t);
            if !self.is_transformable(coordinate, coordinate.t) {
                coordinate.x = f64::INFINITY;
            }
        }

        proj_internal::errno_reset(&self.operation);
        proj_internal::trans_generic_coordinates(&self.operation, direction, coordinates);

        for (index, coordinate) in coordinates.iter_mut().enumerate() {
            if coordinate.x.is_finite() && coordinate.y.is_finite() && coordinate.z.is_finite() {
                report.succeeded += 1;
                continue;
            }
            *coordinate = Coordinate {
                x: f64::NAN,
                y: f64::NAN,
                z: f64::NAN,
                t: UNSPECIFIED_EPOCH,
            };
            if report.failed == 0 {
                report.first_failed_index = index;
            }
            report.failed += 1;
        }
        proj_internal::errno_reset(&self.operation);
        self.context.clear_diagnostics();
        report
    }

    fn transform_named<T: NamedCoordinate, S: NamedCoordinate>(
        &mut self,
        direction: PjDirection,
        coordinate: &S,
    ) -> Result<T> {
        check_kind::<S>(self.crs_from(direction), "originating")?;
        check_kind::<T>(self.crs_to(direction), "destination")?;
        check_input(coordinate)?;

        let in_scale = self.units_per_degree_from(direction);
        let result = self.transform_one(
            direction,
            &Coordinate::new(
                S::x_of(coordinate) * in_scale,
                S::y_of(coordinate) * in_scale,
                S::height_of(coordinate),
            ),
        )?;
        let out_scale = self.units_per_degree_to(direction);
        let mut out = T::default();
        T::set_x(&mut out, result.x / out_scale);
        T::set_y(&mut out, result.y / out_scale);
        T::set_height(&mut out, result.z);
        Ok(out)
    }

    fn transform_named_many<T: NamedCoordinate, S: NamedCoordinate>(
        &mut self,
        direction: PjDirection,
        input: &[S],
        output: &mut [T],
    ) -> Result<BatchReport> {
        if input.len() != output.len() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "input and output spans must have the same size",
            )
            .with_context(format!("input={} output={}", input.len(), output.len())));
        }
        check_kind::<S>(self.crs_from(direction), "originating")?;
        check_kind::<T>(self.crs_to(direction), "destination")?;
        if self.selected.requires_epoch && self.options.coordinate_epoch.is_none() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "the operation is time-dependent and named coordinates carry no epoch; set \
                 TransformerOptions::coordinateEpoch",
            )
            .with_context(format!("operation='{}'", self.selected.name)));
        }
        let mut report = BatchReport::default();
        if input.is_empty() {
            return Ok(report);
        }

        let in_scale = self.units_per_degree_from(direction);
        let out_scale = self.units_per_degree_to(direction);
        for (source, out) in input.iter().zip(output.iter_mut()) {
            T::set_x(out, S::x_of(source) * in_scale);
            T::set_y(out, S::y_of(source) * in_scale);
            T::set_height(out, S::height_of(source));
        }

        // A single broadcast epoch (length-1 array): without it PROJ would
        // substitute t = 0, i.e. the year 0, into time-dependent operations.
        let mut epoch = self.effective_epoch(UNSPECIFIED_EPOCH);
        proj_internal::errno_reset(&self.operation);
        proj_internal::trans_generic_named(&self.operation, direction, output, &mut epoch);

        for (index, (source, out)) in input.iter().zip(output.iter_mut()).enumerate() {
            let ok = S::is_valid_input(source)
                && T::x_of(out).is_finite()
                && T::y_of(out).is_finite()
                && T::height_of(out).is_finite();
            if ok {
                let x = T::x_of(out) / out_scale;
                let y = T::y_of(out) / out_scale;
                T::set_x(out, x);
                T::set_y(out, y);
                report.succeeded += 1;
                continue;
            }
            *out = failed_point::<T>();
            if report.failed == 0 {
                report.first_failed_index = index;
            }
            report.failed += 1;
        }
        proj_internal::errno_reset(&self.operation);
        self.context.clear_diagnostics();
        Ok(report)
    }
}
