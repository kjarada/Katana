//! The `survey` layer: the survey model, least squares and network adjustment, subsurface utilities.
//!
//! The port of src/katana_survey and include/katana/survey (docs/rust.md).

pub mod angles;
pub mod cogo;
pub mod coordinate;
pub mod data_model;
mod ellipse_core;
pub mod error_propagation;
pub mod least_squares;
pub mod leveling;
mod lsq_core;
pub mod matrix;
pub mod network;
pub mod network_adjustment;
pub mod reduction;
mod reduction_adjust;
mod reduction_engine;
mod reduction_formulas;
mod reduction_gnss;
pub mod reduction_report;
mod reduction_report_render;
pub mod reduction_settings;
pub mod report_summary;
pub mod statistics;
pub mod subsurface;
pub mod traverse;

pub use angles::{BearingMeridian, BearingSide, DmsAngle, QuadrantBearing};
pub use cogo::InverseResult;
pub use coordinate::Coordinate2;
pub use data_model::{
    AngularUnit, AntennaHeightMethod, AzimuthObservation, ControlComponent, ControlConstraint,
    ControlPoint, CoordinateSource, CorrectionState, DeclaredCoordinateSystem, DeclaredUnits,
    DistanceKind, DistanceObservation, Face, GeocentricCoordinate, GeodeticCoordinate, GnssAntenna,
    GnssBaselineObservation, GnssCovariance3, GnssGeocentricBaselineObservation,
    GnssGlobalPositionObservation, GnssPositionObservation, GnssSession, GnssSolution,
    HorizontalAngleObservation, HorizontalDirectionObservation, InstrumentSettings,
    LevelDifferenceObservation, LinearUnit, Observation, ObservationPrecision, Pointing,
    SourceRecord, Station, SurveyFeature, SurveyPoint, SurveyProject, SurveyStation, SurveyText,
    SurveyTimestamp, TargetInfo, Traverse, TraverseClosingAngle, TraverseKind, TraverseSetup,
    UnpositionedPoint, VerticalAngleObservation, ZenithAngleObservation,
};
pub use error_propagation::{Covariance2, ErrorEllipse, InverseUncertainty};
pub use least_squares::{CovarianceScaling, LeastSquaresOptions, LeastSquaresSolution};
pub use leveling::{LevelAdjustment, LevelPointResult, LevelRun, LevelRunResult, LevelSetup};
pub use matrix::Matrix;
pub use network::SurveyNetwork;
pub use network_adjustment::{
    AdjustedElevation, AdjustedOrientation, AdjustedParameter, AdjustedStation, AdjustmentOptions,
    AdjustmentStatistics, AppliedAdjustment, CoordinateComponent, GlobalTest,
    HorizontalAdjustmentResult, LevelAdjustmentResult, ResidualComponent, ResidualRecord,
    ResidualSource, TraverseStochasticModel,
};
pub use reduction::{
    ComputedPoint, GridPosition, ReductionContext, ReductionOutcome, reduce_and_adjust,
};
pub use reduction_report::{
    AdjustmentReport, AppliedCorrection, ComputationMethod, CoordinateReport, CorrectionKind,
    FacePairCheck, MisclosureReport, ReductionReport, ReportEllipse, ReportGlobalTest, ReportInput,
    ReportMessage, ReportObservation, ReportResidual, ReportSetup, ResectionReport, render_html,
    render_text,
};
pub use reduction_settings::{
    AdjustmentMethod, AtmosphericCorrection, ControlOrigin, ControlSelection, FaceHandling,
    FaceTolerances, GridScale, HeightReduction, NetworkDimension, OutlierTest, PrismConstantPolicy,
    ReductionSettings, TraverseRule,
};
pub use traverse::{
    LinearMisclosure, TraverseAdjustment, TraverseLegResult, TraverseOptions, TraverseResult,
    TraverseStationResult,
};
