//! The reduction and adjustment options of a survey job, as a person sets them: simple by
//! default, complete when opened (reduction_options_widget.hpp/.cpp).
//!
//! Shown by the import wizard's Reduction & Adjustment step and by Survey Jobs' Edit Adjustment,
//! so a job's settings are changed later exactly where they were first chosen.
//!
//! ```text
//! Basic     the adjustment (radiation, a traverse and its rule, a network and its dimension);
//!           the corrections, each with what the INSTRUMENT already did to the numbers shown
//!           beside it, read from the file's setups (survey's InstrumentSettings) - "applied by
//!           the instrument at 2 of 2 setups (+12.4 ppm)"; and the control: points picked from
//!           the file's positioned points or the drawing's survey points, each held Fixed or
//!           Weighted, horizontally and vertically.
//! Advanced  folded until opened: face tolerances, the a-priori standard deviations, the
//!           refraction coefficient and earth radius, the fixed ppm and prism constant, the scale
//!           factor (none, fixed or from the projection; or a combined factor), the height
//!           reduction, the confidence level, the outlier test, its significance and
//!           auto-reject, and the iterations.
//! ```
//!
//! The widget holds NO survey logic: it maps survey's ReductionSettings to fields and back, in
//! the units a surveyor reads - seconds of arc, millimetres, parts per million - while the
//! settings stay in radians and metres. A field left as it was shown gives back the setting's own
//! value, bit for bit, so opening a job's options and applying them unchanged changes nothing;
//! only a field the person edited is parsed (katana_core's text, never the locale).
//!
//! Object names (an interface: the headless --fill and --press use them):
//!
//! ```text
//! basic     method traverseRule networkDimension atmospheric atmosphericState prismConstant
//!           prismState faces curvatureRefraction curvatureState slopeToHorizontal
//! control   controlFrom controlPick controlHorizontal controlVertical controlSigmaHorizontal
//!           controlSigmaVertical addControl removeControl control (the table of chosen control)
//! advanced  advanced (the fold button) advancedOptions (what it folds) toleranceHorizontal
//!           toleranceZenith toleranceDistance excludeOutside sigmaDirection sigmaZenith
//!           sigmaDistance sigmaPpm sigmaInstrumentCentring sigmaTargetCentring sigmaHeight
//!           sigmaLevelling sigmaGnssHorizontal sigmaGnssVertical useFileCovariances refractionK
//!           earthRadius fixedPpm prismConstantValue gridScale gridScaleFactor useCombinedFactor
//!           combinedFactor heightReduction confidence outlierTest outlierSignificance autoReject
//!           iterations
//! ```

use egui::{Align, Ui};
use katana_core::error::make_error;
use katana_core::text::{format_general, parse_finite_double};
use katana_core::{ErrorCode, Result};
use katana_survey::reduction_settings::validate_reduction_settings;
use katana_survey::{
    AdjustmentMethod, AtmosphericCorrection, ControlComponent, ControlConstraint, ControlOrigin,
    ControlPoint, ControlSelection, CorrectionState, FaceHandling, GridScale, HeightReduction,
    InstrumentSettings, NetworkDimension, OutlierTest, PrismConstantPolicy, ReductionSettings,
    SurveyPoint, SurveyProject, TraverseRule,
};

use super::survey_dialogs::{choice_of, coloured_label, form_row, hidden};
use crate::drive::{self, Choice, ControlKind, ControlRecord, LineOptions};
use crate::theme::palette;

/// Seconds of arc in a radian: 648000 / pi.
const SECONDS_PER_RADIAN: f64 = 648_000.0 / std::f64::consts::PI;

/// A number as a person reads it: up to ten significant figures, no trailing zeros - 10" shows
/// as "10", not as the 10.000000000000002 its radians give back. Never the locale: a field must
/// read back as it was written.
fn shown_number(value: f64) -> String {
    format_general(value, 10)
}

// The labels a person reads - and what a headless --fill chooses by.
const METHODS: [(&str, AdjustmentMethod); 3] = [
    ("radiation (no adjustment)", AdjustmentMethod::None),
    ("traverse", AdjustmentMethod::Traverse),
    ("network least squares", AdjustmentMethod::Network),
];
const TRAVERSE_RULES: [(&str, TraverseRule); 3] = [
    ("Bowditch (compass rule)", TraverseRule::Bowditch),
    ("Transit", TraverseRule::Transit),
    ("least squares", TraverseRule::LeastSquares),
];
const DIMENSIONS: [(&str, NetworkDimension); 3] = [
    ("horizontal", NetworkDimension::Horizontal),
    ("levels", NetworkDimension::Levels),
    (
        "horizontal and levels",
        NetworkDimension::HorizontalAndLevels,
    ),
];
const ATMOSPHERIC: [(&str, AtmosphericCorrection); 4] = [
    (
        "auto: only where the instrument did not",
        AtmosphericCorrection::Auto,
    ),
    (
        "recompute from the recorded weather",
        AtmosphericCorrection::Recompute,
    ),
    ("a fixed ppm (advanced)", AtmosphericCorrection::Fixed),
    ("none", AtmosphericCorrection::None),
];
const PRISM: [(&str, PrismConstantPolicy); 3] = [
    (
        "auto: only where the instrument did not",
        PrismConstantPolicy::Auto,
    ),
    (
        "override with one constant (advanced)",
        PrismConstantPolicy::Override,
    ),
    ("none: distances as recorded", PrismConstantPolicy::None),
];
const FACES: [(&str, FaceHandling); 3] = [
    ("mean of face left and right", FaceHandling::Average),
    ("face left only", FaceHandling::FaceLeftOnly),
    ("every pointing on its own", FaceHandling::Separate),
];
const GRID_SCALE: [(&str, GridScale); 3] = [
    ("none (ground distances)", GridScale::None),
    ("a fixed factor", GridScale::Fixed),
    ("from the drawing's projection", GridScale::FromProjection),
];
const HEIGHTS: [(&str, HeightReduction); 3] = [
    ("none", HeightReduction::None),
    ("to the ellipsoid", HeightReduction::Ellipsoid),
    ("to the geoid (sea level)", HeightReduction::Geoid),
];
const OUTLIER_TESTS: [(&str, OutlierTest); 3] = [
    ("Baarda (data snooping)", OutlierTest::Baarda),
    ("Pope's tau", OutlierTest::Tau),
    ("none", OutlierTest::None),
];
const CONSTRAINTS: [(&str, ControlConstraint); 3] = [
    ("fixed", ControlConstraint::Fixed),
    ("weighted", ControlConstraint::Weighted),
    ("free", ControlConstraint::Free),
];
const FROM_FILE: &str = "the file";
const FROM_DRAWING: &str = "the drawing";

fn labels<E>(table: &[(&'static str, E)]) -> Vec<&'static str> {
    table.iter().map(|(label, _)| *label).collect()
}

/// The item of `table` that holds `value`; the first when none does (QComboBox::findData, then
/// max(0, ...)).
fn index_of<E: PartialEq + Copy>(table: &[(&str, E)], value: E) -> usize {
    table
        .iter()
        .position(|(_, each)| *each == value)
        .unwrap_or(0)
}

fn choose<E: PartialEq + Copy>(table: &[(&str, E)], value: E) -> Choice {
    Choice::at(index_of(table, value))
}

fn chosen<E: Copy>(table: &[(&str, E)], choice: &Choice) -> E {
    table[choice.index.unwrap_or(0).min(table.len() - 1)].1
}

/// A point that may be taken as control, and whose coordinates it would be held at.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ControlCandidate {
    pub id: String,
    pub origin: ControlOrigin,
    pub has_elevation: bool,
}

/// Counts of a correction's state across the setups of a file, and the values the instrument used
/// where it applied it.
#[derive(Default)]
struct StateCounts {
    applied: usize,
    not_applied: usize,
    unknown: usize,
    /// Where applied and stated.
    values: Vec<f64>,
}

fn count_states(
    raw: &SurveyProject,
    get: impl Fn(&InstrumentSettings) -> (CorrectionState, Option<f64>),
) -> StateCounts {
    let mut counts = StateCounts::default();
    for station in &raw.stations {
        let (state, value) = get(&station.instrument);
        match state {
            CorrectionState::Applied => {
                counts.applied += 1;
                if let Some(value) = value {
                    counts.values.push(value);
                }
            }
            CorrectionState::NotApplied => counts.not_applied += 1,
            CorrectionState::Unknown => counts.unknown += 1,
        }
    }
    counts
}

/// "applied by the instrument at 2 of 3 setups (+12.4 ppm); not applied at 1"
#[allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "std::minmax_element's own test for the last greatest, !(x < greatest)"
)]
fn state_text(counts: &StateCounts, setups: usize, value: impl Fn(f64) -> String) -> String {
    if setups == 0 {
        return String::from("the file has no instrument setups");
    }
    let of = |n: usize| {
        if n == setups {
            if setups == 1 {
                String::from("the setup")
            } else {
                format!("all {setups} setups")
            }
        } else {
            format!("{n} of {setups} setups")
        }
    };
    let mut parts = Vec::new();
    if counts.applied > 0 {
        let mut part = format!("applied by the instrument at {}", of(counts.applied));
        if !counts.values.is_empty() {
            // std::minmax_element: the first least and the last greatest.
            let mut low = counts.values[0];
            let mut high = counts.values[0];
            for &each in &counts.values[1..] {
                if each < low {
                    low = each;
                }
                if !(each < high) {
                    high = each;
                }
            }
            if low == high {
                part.push_str(&format!(" ({})", value(low)));
            } else {
                part.push_str(&format!(" ({} to {})", value(low), value(high)));
            }
        }
        parts.push(part);
    }
    if counts.not_applied > 0 {
        parts.push(format!(
            "not applied by the instrument at {}",
            of(counts.not_applied)
        ));
    }
    if counts.unknown > 0 {
        parts.push(format!("not stated by the file at {}", of(counts.unknown)));
    }
    parts.join("; ")
}

/// std::format("{:+.{}f} {}"): the sign always, `decimals` places.
fn signed_fixed(value: f64, decimals: usize, unit: &str) -> String {
    format!("{value:+.decimals$} {unit}")
}

/// What a control row says of how a component is held: `a` and `b` are the northing and easting
/// (the widget sets both alike), or the elevation twice.
fn constraint_text(a: &ControlComponent, b: &ControlComponent) -> String {
    let one = |component: &ControlComponent| match component.constraint {
        ControlConstraint::Fixed => String::from("fixed"),
        ControlConstraint::Weighted => {
            format!(
                "weighted {} mm",
                format_general(component.sigma * 1000.0, 10)
            )
        }
        ControlConstraint::Free => String::from("free"),
    };
    let first = one(a);
    let second = one(b);
    if first == second {
        first
    } else {
        format!("N {first}, E {second}")
    }
}

/// Words for what the instrument did to the atmospheric correction across the setups of a file:
/// "applied by the instrument at 2 of 2 setups (+12.4 ppm)", "not stated by the file at 3
/// setups".
pub fn atmospheric_state_text(raw: &SurveyProject) -> String {
    let counts = count_states(raw, |instrument| {
        (instrument.atmospheric_ppm_state, instrument.atmospheric_ppm)
    });
    state_text(&counts, raw.stations.len(), |ppm| {
        signed_fixed(ppm, 1, "ppm")
    })
}

/// The same for the prism constant.
pub fn prism_state_text(raw: &SurveyProject) -> String {
    let counts = count_states(raw, |instrument| {
        (instrument.prism_constant_state, instrument.prism_constant)
    });
    state_text(&counts, raw.stations.len(), |metres| {
        signed_fixed(metres * 1000.0, 1, "mm")
    })
}

/// The same for curvature and refraction.
pub fn curvature_state_text(raw: &SurveyProject) -> String {
    let counts = count_states(raw, |instrument| {
        (
            instrument.curvature_refraction_state,
            instrument.refraction_coefficient,
        )
    });
    state_text(&counts, raw.stations.len(), |k| {
        format!("k = {}", format_general(k, 3))
    })
}

/// A number field: what it shows, and the setting's value it stands for while it shows that.
#[derive(Clone, Debug, Default, PartialEq)]
struct NumberField {
    text: String,
    shown: String,
    stored: f64,
}

impl NumberField {
    fn show(&mut self, shown: f64, stored: f64) {
        self.text = shown_number(shown);
        self.shown = self.text.clone();
        self.stored = stored;
    }

    /// Untouched: the setting's own value, not its rounded text read back. Otherwise the text as
    /// a number, divided - not multiplied by a reciprocal: 95 / 100 is 0.95, while 95 * 0.01 is
    /// 0.9500000000000001 - from the unit shown to the setting's. ParseFailure naming `what`.
    fn number(&self, divisor: f64, what: &str) -> Result<f64> {
        let text = self.text.trim();
        if text == self.shown {
            return Ok(self.stored);
        }
        match parse_finite_double(text) {
            Some(value) => Ok(value / divisor),
            None => Err(make_error(
                ErrorCode::ParseFailure,
                format!("{what} is not a number"),
                text,
            )),
        }
    }
}

/// A number field drawn: 120 points wide at most, enabled or not, its change answered.
fn number_ui(ui: &mut Ui, name: &str, field: &mut NumberField, enabled: bool, tip: &str) -> bool {
    ui.add_enabled_ui(enabled, |ui| {
        drive::line_with(
            ui,
            name,
            &mut field.text,
            LineOptions {
                width: Some(120.0),
                ..LineOptions::default()
            },
        )
        .on_hover_text(tip)
        .changed()
    })
    .inner
}

/// The options (the C++ ReductionOptionsWidget).
#[derive(Clone, Debug, PartialEq)]
pub struct ReductionOptionsWidget {
    /// What set_settings gave: what the fields do not show is kept as it was.
    shown: ReductionSettings,
    control: Vec<ControlSelection>,
    file_candidates: Vec<ControlCandidate>,
    drawing_candidates: Vec<ControlCandidate>,
    // basic
    method: Choice,
    traverse_rule: Choice,
    network_dimension: Choice,
    atmospheric: Choice,
    atmospheric_state: String,
    prism: Choice,
    prism_state: String,
    faces: Choice,
    curvature: bool,
    curvature_state: String,
    slope: bool,
    // control
    control_from: Choice,
    control_pick: Choice,
    pick_items: Vec<String>,
    pick_tip: String,
    control_horizontal: Choice,
    control_vertical: Choice,
    control_sigma_horizontal: String,
    control_sigma_vertical: String,
    control_rows: Vec<Vec<String>>,
    control_current: Option<usize>,
    control_tip: String,
    // advanced
    advanced: bool,
    scroll_to_fold: bool,
    tolerance_horizontal: NumberField,
    tolerance_zenith: NumberField,
    tolerance_distance: NumberField,
    exclude_outside: bool,
    sigma_direction: NumberField,
    sigma_zenith: NumberField,
    sigma_distance: NumberField,
    sigma_ppm: NumberField,
    sigma_instrument_centring: NumberField,
    sigma_target_centring: NumberField,
    sigma_height: NumberField,
    sigma_levelling: NumberField,
    sigma_gnss_horizontal: NumberField,
    sigma_gnss_vertical: NumberField,
    use_file_covariances: bool,
    refraction_k: NumberField,
    earth_radius: NumberField,
    fixed_ppm: NumberField,
    prism_constant_value: NumberField,
    grid_scale: Choice,
    grid_scale_factor: NumberField,
    use_combined_factor: bool,
    combined_factor: NumberField,
    height_reduction: Choice,
    confidence: NumberField,
    outlier_test: Choice,
    outlier_significance: NumberField,
    auto_reject: bool,
    iterations: i64,
}

impl Default for ReductionOptionsWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl ReductionOptionsWidget {
    /// The fields showing survey's default settings, the advanced section folded.
    pub fn new() -> Self {
        let mut widget = Self {
            shown: ReductionSettings::default(),
            control: Vec::new(),
            file_candidates: Vec::new(),
            drawing_candidates: Vec::new(),
            method: Choice::at(0),
            traverse_rule: Choice::at(0),
            network_dimension: Choice::at(0),
            atmospheric: Choice::at(0),
            atmospheric_state: String::new(),
            prism: Choice::at(0),
            prism_state: String::new(),
            faces: Choice::at(0),
            curvature: false,
            curvature_state: String::new(),
            slope: false,
            control_from: Choice::at(0),
            control_pick: Choice::default(),
            pick_items: Vec::new(),
            pick_tip: String::from("The point to hold"),
            control_horizontal: Choice::at(0),
            control_vertical: Choice::at(0),
            control_sigma_horizontal: String::from("5"),
            control_sigma_vertical: String::from("10"),
            control_rows: Vec::new(),
            control_current: None,
            control_tip: String::new(),
            advanced: false,
            scroll_to_fold: false,
            tolerance_horizontal: NumberField::default(),
            tolerance_zenith: NumberField::default(),
            tolerance_distance: NumberField::default(),
            exclude_outside: false,
            sigma_direction: NumberField::default(),
            sigma_zenith: NumberField::default(),
            sigma_distance: NumberField::default(),
            sigma_ppm: NumberField::default(),
            sigma_instrument_centring: NumberField::default(),
            sigma_target_centring: NumberField::default(),
            sigma_height: NumberField::default(),
            sigma_levelling: NumberField::default(),
            sigma_gnss_horizontal: NumberField::default(),
            sigma_gnss_vertical: NumberField::default(),
            use_file_covariances: false,
            refraction_k: NumberField::default(),
            earth_radius: NumberField::default(),
            fixed_ppm: NumberField::default(),
            prism_constant_value: NumberField::default(),
            grid_scale: Choice::at(0),
            grid_scale_factor: NumberField::default(),
            use_combined_factor: false,
            combined_factor: NumberField::default(),
            height_reduction: Choice::at(0),
            confidence: NumberField::default(),
            outlier_test: Choice::at(0),
            outlier_significance: NumberField::default(),
            auto_reject: false,
            iterations: 1,
        };
        widget.set_settings(&ReductionSettings::default());
        widget
    }

    /// The fields show `settings`; what the widget gives back until a field is edited.
    pub fn set_settings(&mut self, settings: &ReductionSettings) {
        self.shown = settings.clone();
        self.method = choose(&METHODS, settings.method);
        self.traverse_rule = choose(&TRAVERSE_RULES, settings.traverse_rule);
        self.network_dimension = choose(&DIMENSIONS, settings.network_dimension);
        self.atmospheric = choose(&ATMOSPHERIC, settings.atmospheric);
        self.prism = choose(&PRISM, settings.prism_constant_policy);
        self.faces = choose(&FACES, settings.faces);
        self.curvature = settings.curvature_and_refraction;
        self.slope = settings.slope_to_horizontal;

        let arc = SECONDS_PER_RADIAN;
        let tolerances = &settings.face_tolerances;
        self.tolerance_horizontal
            .show(tolerances.horizontal * arc, tolerances.horizontal);
        self.tolerance_zenith
            .show(tolerances.zenith * arc, tolerances.zenith);
        self.tolerance_distance
            .show(tolerances.distance * 1000.0, tolerances.distance);
        self.exclude_outside = tolerances.exclude_outside;
        let p = &settings.apriori;
        self.sigma_direction.show(p.direction * arc, p.direction);
        self.sigma_zenith.show(p.zenith * arc, p.zenith);
        self.sigma_distance
            .show(p.distance_constant * 1000.0, p.distance_constant);
        self.sigma_ppm.show(p.distance_ppm, p.distance_ppm);
        self.sigma_instrument_centring
            .show(p.instrument_centring * 1000.0, p.instrument_centring);
        self.sigma_target_centring
            .show(p.target_centring * 1000.0, p.target_centring);
        self.sigma_height
            .show(p.height_measurement * 1000.0, p.height_measurement);
        self.sigma_levelling.show(
            p.levelling_per_sqrt_kilometre * 1000.0,
            p.levelling_per_sqrt_kilometre,
        );
        self.sigma_gnss_horizontal
            .show(p.gnss_horizontal * 1000.0, p.gnss_horizontal);
        self.sigma_gnss_vertical
            .show(p.gnss_vertical * 1000.0, p.gnss_vertical);
        self.use_file_covariances = settings.use_file_covariances;
        self.refraction_k.show(
            settings.refraction_coefficient,
            settings.refraction_coefficient,
        );
        self.earth_radius
            .show(settings.earth_radius, settings.earth_radius);
        self.fixed_ppm.show(settings.fixed_ppm, settings.fixed_ppm);
        self.prism_constant_value
            .show(settings.prism_constant * 1000.0, settings.prism_constant);
        self.grid_scale = choose(&GRID_SCALE, settings.grid_scale);
        self.grid_scale_factor.show(
            settings.fixed_grid_scale_factor,
            settings.fixed_grid_scale_factor,
        );
        self.use_combined_factor = settings.use_combined_factor;
        self.combined_factor
            .show(settings.combined_factor, settings.combined_factor);
        self.height_reduction = choose(&HEIGHTS, settings.height_reduction);
        self.confidence
            .show(settings.confidence_level * 100.0, settings.confidence_level);
        self.outlier_test = choose(&OUTLIER_TESTS, settings.outlier_test);
        self.outlier_significance
            .show(settings.outlier_significance, settings.outlier_significance);
        self.auto_reject = settings.auto_reject_outliers;
        self.iterations = settings.max_iterations.clamp(1, 1000) as i64;
        self.control = settings.control.clone();
        self.show_control();
    }

    /// The settings as the fields say, checked by survey's validate_reduction_settings.
    /// ParseFailure naming the field for a number that does not read.
    pub fn settings(&self) -> Result<ReductionSettings> {
        // Everything the fields do not show is kept as it was given.
        let mut s = self.shown.clone();
        s.method = chosen(&METHODS, &self.method);
        s.traverse_rule = chosen(&TRAVERSE_RULES, &self.traverse_rule);
        s.network_dimension = chosen(&DIMENSIONS, &self.network_dimension);
        s.atmospheric = chosen(&ATMOSPHERIC, &self.atmospheric);
        s.prism_constant_policy = chosen(&PRISM, &self.prism);
        s.faces = chosen(&FACES, &self.faces);
        s.curvature_and_refraction = self.curvature;
        s.slope_to_horizontal = self.slope;

        let arc = SECONDS_PER_RADIAN;
        s.face_tolerances.horizontal = self
            .tolerance_horizontal
            .number(arc, "the horizontal face tolerance")?;
        s.face_tolerances.zenith = self
            .tolerance_zenith
            .number(arc, "the zenith face tolerance")?;
        s.face_tolerances.distance = self
            .tolerance_distance
            .number(1000.0, "the distance face tolerance")?;
        s.apriori.direction = self
            .sigma_direction
            .number(arc, "the direction standard deviation")?;
        s.apriori.zenith = self
            .sigma_zenith
            .number(arc, "the zenith standard deviation")?;
        s.apriori.distance_constant = self
            .sigma_distance
            .number(1000.0, "the distance standard deviation")?;
        s.apriori.distance_ppm = self.sigma_ppm.number(1.0, "the distance ppm")?;
        s.apriori.instrument_centring = self
            .sigma_instrument_centring
            .number(1000.0, "the instrument centring")?;
        s.apriori.target_centring = self
            .sigma_target_centring
            .number(1000.0, "the target centring")?;
        s.apriori.height_measurement = self
            .sigma_height
            .number(1000.0, "the height standard deviation")?;
        s.apriori.levelling_per_sqrt_kilometre = self
            .sigma_levelling
            .number(1000.0, "the levelling standard deviation")?;
        s.apriori.gnss_horizontal = self
            .sigma_gnss_horizontal
            .number(1000.0, "the GNSS horizontal standard deviation")?;
        s.apriori.gnss_vertical = self
            .sigma_gnss_vertical
            .number(1000.0, "the GNSS vertical standard deviation")?;
        s.refraction_coefficient = self
            .refraction_k
            .number(1.0, "the refraction coefficient")?;
        s.earth_radius = self.earth_radius.number(1.0, "the earth radius")?;
        s.fixed_ppm = self.fixed_ppm.number(1.0, "the fixed ppm")?;
        s.prism_constant = self
            .prism_constant_value
            .number(1000.0, "the prism constant")?;
        s.fixed_grid_scale_factor = self
            .grid_scale_factor
            .number(1.0, "the grid scale factor")?;
        s.combined_factor = self.combined_factor.number(1.0, "the combined factor")?;
        s.confidence_level = self.confidence.number(100.0, "the confidence level")?;
        s.outlier_significance = self
            .outlier_significance
            .number(1.0, "the outlier significance")?;
        s.face_tolerances.exclude_outside = self.exclude_outside;
        s.use_file_covariances = self.use_file_covariances;
        s.grid_scale = chosen(&GRID_SCALE, &self.grid_scale);
        s.use_combined_factor = self.use_combined_factor;
        s.height_reduction = chosen(&HEIGHTS, &self.height_reduction);
        s.outlier_test = chosen(&OUTLIER_TESTS, &self.outlier_test);
        s.auto_reject_outliers = self.auto_reject;
        s.max_iterations = self.iterations.max(1) as usize;
        s.control = self.control.clone();
        validate_reduction_settings(&s)?;
        Ok(s)
    }

    /// What the file's setups say the instrument already applied, and the file's positioned
    /// points as control candidates.
    pub fn set_project(&mut self, raw: &SurveyProject) {
        self.atmospheric_state = atmospheric_state_text(raw);
        self.prism_state = prism_state_text(raw);
        self.curvature_state = curvature_state_text(raw);
        self.file_candidates = raw
            .points
            .iter()
            .map(|point| ControlCandidate {
                id: point.id.clone(),
                origin: ControlOrigin::File,
                has_elevation: point.elevation.is_some(),
            })
            .collect();
        self.fill_control_pick();
    }

    /// The drawing's survey points as control candidates.
    pub fn set_drawing_points(&mut self, points: &[SurveyPoint]) {
        self.drawing_candidates = points
            .iter()
            .map(|point| ControlCandidate {
                id: point.id.clone(),
                origin: ControlOrigin::Drawing,
                has_elevation: point.elevation.is_some(),
            })
            .collect();
        self.fill_control_pick();
    }

    pub fn advanced_shown(&self) -> bool {
        self.advanced
    }

    /// Opens or folds the advanced section. Opened below the fold of a page that scrolls, the
    /// section would open out of sight: the page is scrolled to put the fold at the top, once the
    /// section has been laid out.
    pub fn show_advanced(&mut self, shown: bool) {
        self.advanced = shown;
        self.scroll_to_fold = shown;
    }

    /// The control table's rows, as the table shows them.
    pub fn control_rows(&self) -> &[Vec<String>] {
        &self.control_rows
    }

    fn reads_drawing(&self) -> bool {
        self.control_from.index == Some(1)
    }

    fn source_text(&self) -> &'static str {
        if self.reads_drawing() {
            FROM_DRAWING
        } else {
            FROM_FILE
        }
    }

    fn fill_control_pick(&mut self) {
        let candidates = if self.reads_drawing() {
            &self.drawing_candidates
        } else {
            &self.file_candidates
        };
        let ids: Vec<String> = candidates
            .iter()
            .map(|candidate| candidate.id.clone())
            .collect();
        let current = self.control_pick.current_text(&self.pick_items);
        let index = ids.iter().position(|id| *id == current).unwrap_or(0);
        self.control_pick = if ids.is_empty() {
            Choice::default()
        } else {
            Choice::at(index)
        };
        self.pick_tip = if ids.is_empty() {
            format!("{} has no positioned points to hold", self.source_text())
        } else {
            format!("{} point(s) of {}", ids.len(), self.source_text())
        };
        self.pick_items = ids;
    }

    fn show_control(&mut self) {
        self.control_rows = self
            .control
            .iter()
            .map(|selection| {
                vec![
                    selection.point.point_id.clone(),
                    String::from(if selection.origin == ControlOrigin::File {
                        FROM_FILE
                    } else {
                        FROM_DRAWING
                    }),
                    constraint_text(&selection.point.northing, &selection.point.easting),
                    constraint_text(&selection.point.elevation, &selection.point.elevation),
                ]
            })
            .collect();
        if self
            .control_current
            .is_some_and(|row| row >= self.control_rows.len())
        {
            self.control_current = None;
        }
    }

    /// Choosing a held point puts it in the editor, to change how it is held.
    fn control_chosen(&mut self, row: usize) {
        let Some(selection) = self.control.get(row).cloned() else {
            return;
        };
        let from = if selection.origin == ControlOrigin::File {
            0
        } else {
            1
        };
        if self.control_from.index != Some(from) {
            self.control_from = Choice::at(from);
            self.fill_control_pick();
        }
        self.control_pick.index = self
            .pick_items
            .iter()
            .position(|id| *id == selection.point.point_id);
        self.control_horizontal = choose(&CONSTRAINTS, selection.point.northing.constraint);
        self.control_vertical = choose(&CONSTRAINTS, selection.point.elevation.constraint);
        if selection.point.northing.constraint == ControlConstraint::Weighted {
            self.control_sigma_horizontal = shown_number(selection.point.northing.sigma * 1000.0);
        }
        if selection.point.elevation.constraint == ControlConstraint::Weighted {
            self.control_sigma_vertical = shown_number(selection.point.elevation.sigma * 1000.0);
        }
    }

    fn component(choice: &Choice, sigma: &str) -> Result<ControlComponent> {
        let constraint = chosen(&CONSTRAINTS, choice);
        let mut component = ControlComponent {
            constraint,
            sigma: 0.0,
        };
        if constraint == ControlConstraint::Weighted {
            match parse_finite_double(sigma.trim()) {
                Some(value) if value > 0.0 => component.sigma = value / 1000.0,
                _ => {
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        "a weighted control component needs a standard deviation above 0 mm",
                        sigma,
                    ));
                }
            }
        }
        Ok(component)
    }

    /// Hold: the point as the editor shows it, or how a held point is held changed. True when
    /// the control changed.
    fn add_control(&mut self) -> bool {
        let id = self.control_pick.current_text(&self.pick_items);
        if id.is_empty() {
            return false;
        }
        let horizontal =
            match Self::component(&self.control_horizontal, &self.control_sigma_horizontal) {
                Ok(component) => component,
                Err(error) => {
                    self.control_tip = error.describe();
                    return false;
                }
            };
        let vertical = match Self::component(&self.control_vertical, &self.control_sigma_vertical) {
            Ok(component) => component,
            Err(error) => {
                self.control_tip = error.describe();
                return false;
            }
        };
        let selection = ControlSelection {
            point: ControlPoint {
                point_id: id,
                northing: horizontal,
                easting: horizontal,
                elevation: vertical,
            },
            origin: if self.reads_drawing() {
                ControlOrigin::Drawing
            } else {
                ControlOrigin::File
            },
        };
        match self
            .control
            .iter_mut()
            .find(|held| held.point.point_id == selection.point.point_id)
        {
            Some(held) => *held = selection,
            None => self.control.push(selection),
        }
        self.show_control();
        true
    }

    /// Release: the point chosen in the table, or with none chosen the point the editor names.
    /// True when the control changed.
    fn remove_control(&mut self) -> bool {
        let mut row = self.control_current;
        if row.is_none() {
            let id = self.control_pick.current_text(&self.pick_items);
            for (at, held) in self.control.iter().enumerate() {
                if held.point.point_id == id {
                    row = Some(at);
                }
            }
        }
        let Some(row) = row.filter(|row| *row < self.control.len()) else {
            return false;
        };
        self.control.remove(row);
        self.show_control();
        true
    }

    /// Draws the options. True when a setting changed (the C++'s onChanged), so a page can mark a
    /// preview as out of date.
    pub fn ui(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;
        let shown = ui.vertical(|ui| {
            changed |= self.basic_ui(ui);
            changed |= self.control_ui(ui);
            let fold = drive::toggle(
                ui,
                "advanced",
                "Advanced: tolerances, standard deviations, scale, statistics",
                self.advanced,
            );
            if fold.clicked() {
                self.show_advanced(!self.advanced);
            } else if self.scroll_to_fold {
                self.scroll_to_fold = false;
                fold.scroll_to_me(Some(Align::TOP));
            }
            let open = self.advanced;
            let mut section = |ui: &mut Ui| {
                let section = ui.vertical(|ui| self.advanced_ui(ui));
                drive::custom(
                    ui,
                    &section.response,
                    ControlRecord::new("advancedOptions", ControlKind::Other),
                );
                section.inner
            };
            changed |= if open {
                section(ui)
            } else {
                hidden(ui, "advancedOptions", section)
            };
        });
        drive::custom(
            ui,
            &shown.response,
            ControlRecord::new("reductionOptions", ControlKind::Other),
        );
        changed
    }

    fn basic_ui(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;
        ui.group(|ui| {
            ui.strong("Adjustment and corrections");
            let method = chosen(&METHODS, &self.method);
            form_row(ui, "Adjustment:", |ui| {
                changed |= choice_of(ui, "method", &labels(&METHODS), &mut self.method)
                    .on_hover_text(
                        "Radiation computes each point from its setup and adjusts nothing; a \
                         traverse or a network adjusts, and needs control",
                    )
                    .changed();
                let mut rule = |ui: &mut Ui| {
                    choice_of(
                        ui,
                        "traverseRule",
                        &labels(&TRAVERSE_RULES),
                        &mut self.traverse_rule,
                    )
                    .on_hover_text("How a traverse's misclosure is shared out")
                    .changed()
                };
                changed |= if method == AdjustmentMethod::Traverse {
                    rule(ui)
                } else {
                    hidden(ui, "traverseRule", rule)
                };
                let mut dimension = |ui: &mut Ui| {
                    choice_of(
                        ui,
                        "networkDimension",
                        &labels(&DIMENSIONS),
                        &mut self.network_dimension,
                    )
                    .on_hover_text(
                        "What the least squares adjusts: there is no combined 3D network, so \
                         heights are a level network of their own",
                    )
                    .changed()
                };
                changed |= if method == AdjustmentMethod::Network {
                    dimension(ui)
                } else {
                    hidden(ui, "networkDimension", dimension)
                };
            });
            let muted = palette::text_muted().to_egui();
            form_row(ui, "Atmospheric ppm:", |ui| {
                ui.vertical(|ui| {
                    changed |= choice_of(
                        ui,
                        "atmospheric",
                        &labels(&ATMOSPHERIC),
                        &mut self.atmospheric,
                    )
                    .changed();
                    coloured_label(ui, "atmosphericState", &self.atmospheric_state, muted);
                });
            });
            form_row(ui, "Prism constant:", |ui| {
                ui.vertical(|ui| {
                    changed |=
                        choice_of(ui, "prismConstant", &labels(&PRISM), &mut self.prism).changed();
                    coloured_label(ui, "prismState", &self.prism_state, muted);
                });
            });
            form_row(ui, "Faces:", |ui| {
                changed |= choice_of(ui, "faces", &labels(&FACES), &mut self.faces).changed();
            });
            form_row(ui, "Reduction:", |ui| {
                ui.vertical(|ui| {
                    changed |= drive::check(
                        ui,
                        "curvatureRefraction",
                        "Curvature and refraction",
                        &mut self.curvature,
                    )
                    .changed();
                    coloured_label(ui, "curvatureState", &self.curvature_state, muted);
                    changed |= drive::check(
                        ui,
                        "slopeToHorizontal",
                        "Slope distances to horizontal",
                        &mut self.slope,
                    )
                    .changed();
                });
            });
        });
        changed
    }

    fn control_ui(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;
        ui.group(|ui| {
            ui.strong("Control");
            // Two rows, so the options fit the narrow pane of Survey Jobs as well as the wizard's
            // page: which point, then how it is held.
            ui.horizontal(|ui| {
                ui.label("Point from");
                let from = choice_of(
                    ui,
                    "controlFrom",
                    &[FROM_FILE, FROM_DRAWING],
                    &mut self.control_from,
                )
                .on_hover_text(
                    "Whose coordinates the point is held at: the file's, or those of the survey \
                     point of that number already on the drawing",
                );
                if from.changed() {
                    self.fill_control_pick();
                }
                drive::choice(ui, "controlPick", &self.pick_items, &mut self.control_pick)
                    .on_hover_text(self.pick_tip.as_str());
                if drive::button(ui, "addControl", "Hold")
                    .on_hover_text("Hold the point as shown, or change how a held point is held")
                    .clicked()
                {
                    changed |= self.add_control();
                }
                if drive::button(ui, "removeControl", "Release")
                    .on_hover_text("Stop holding the point chosen in the table")
                    .clicked()
                {
                    changed |= self.remove_control();
                }
            });
            ui.horizontal(|ui| {
                ui.label("Northing, easting");
                choice_of(
                    ui,
                    "controlHorizontal",
                    &labels(&CONSTRAINTS),
                    &mut self.control_horizontal,
                )
                .on_hover_text(
                    "Northing and easting: fixed, weighted by the standard deviation beside it, \
                     or free",
                );
                let weighted =
                    chosen(&CONSTRAINTS, &self.control_horizontal) == ControlConstraint::Weighted;
                ui.add_enabled_ui(weighted, |ui| {
                    drive::line_with(
                        ui,
                        "controlSigmaHorizontal",
                        &mut self.control_sigma_horizontal,
                        LineOptions {
                            width: Some(60.0),
                            ..LineOptions::default()
                        },
                    )
                    .on_hover_text("Millimetres, for a weighted northing and easting")
                });
                ui.label("mm   Elevation");
                choice_of(
                    ui,
                    "controlVertical",
                    &labels(&CONSTRAINTS),
                    &mut self.control_vertical,
                )
                .on_hover_text("Elevation: fixed, weighted or free");
                let weighted =
                    chosen(&CONSTRAINTS, &self.control_vertical) == ControlConstraint::Weighted;
                ui.add_enabled_ui(weighted, |ui| {
                    drive::line_with(
                        ui,
                        "controlSigmaVertical",
                        &mut self.control_sigma_vertical,
                        LineOptions {
                            width: Some(60.0),
                            ..LineOptions::default()
                        },
                    )
                    .on_hover_text("Millimetres, for a weighted elevation")
                });
                ui.label("mm");
            });
            ui.horizontal(|ui| {
                ui.weak("Point | From | Northing, easting | Elevation");
            });
            let mut current = self.control_current;
            let table = drive::rows(ui, "control", &self.control_rows, &mut current);
            let table = if self.control_tip.is_empty() {
                table
            } else {
                table.on_hover_text(self.control_tip.as_str())
            };
            self.control_current = current;
            if table.changed()
                && let Some(row) = current
            {
                self.control_chosen(row);
            }
        });
        changed
    }

    fn advanced_ui(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;
        let curvature = self.curvature;
        let fixed_ppm = chosen(&ATMOSPHERIC, &self.atmospheric) == AtmosphericCorrection::Fixed;
        let prism_value = chosen(&PRISM, &self.prism) == PrismConstantPolicy::Override;
        let combined = self.use_combined_factor;
        let grid_factor = !combined && chosen(&GRID_SCALE, &self.grid_scale) == GridScale::Fixed;
        let testing = chosen(&OUTLIER_TESTS, &self.outlier_test) != OutlierTest::None;
        ui.group(|ui| {
            ui.strong("Face tolerances");
            form_row(ui, "Horizontal (\"):", |ui| {
                changed |= number_ui(
                    ui,
                    "toleranceHorizontal",
                    &mut self.tolerance_horizontal,
                    true,
                    "Largest spread of a face-left / face-right pair, seconds",
                );
            });
            form_row(ui, "Zenith (\"):", |ui| {
                changed |= number_ui(
                    ui,
                    "toleranceZenith",
                    &mut self.tolerance_zenith,
                    true,
                    "Largest zenith spread (twice the index error), seconds",
                );
            });
            form_row(ui, "Distance (mm):", |ui| {
                changed |= number_ui(
                    ui,
                    "toleranceDistance",
                    &mut self.tolerance_distance,
                    true,
                    "Largest distance spread, millimetres",
                );
            });
            changed |= drive::check(
                ui,
                "excludeOutside",
                "Exclude a pair outside them",
                &mut self.exclude_outside,
            )
            .changed();
        });
        ui.group(|ui| {
            ui.strong("A-priori standard deviations");
            form_row(ui, "Direction (\"):", |ui| {
                changed |= number_ui(
                    ui,
                    "sigmaDirection",
                    &mut self.sigma_direction,
                    true,
                    "A direction, seconds",
                );
            });
            form_row(ui, "Zenith (\"):", |ui| {
                changed |= number_ui(
                    ui,
                    "sigmaZenith",
                    &mut self.sigma_zenith,
                    true,
                    "A zenith angle, seconds",
                );
            });
            form_row(ui, "Distance:", |ui| {
                changed |= number_ui(
                    ui,
                    "sigmaDistance",
                    &mut self.sigma_distance,
                    true,
                    "The constant part of a distance, millimetres",
                );
                ui.label("mm +");
                changed |= number_ui(
                    ui,
                    "sigmaPpm",
                    &mut self.sigma_ppm,
                    true,
                    "The part in proportion to the distance, ppm",
                );
                ui.label("ppm");
            });
            form_row(ui, "Centring (mm):", |ui| {
                changed |= number_ui(
                    ui,
                    "sigmaInstrumentCentring",
                    &mut self.sigma_instrument_centring,
                    true,
                    "Millimetres",
                );
                ui.label("instrument,");
                changed |= number_ui(
                    ui,
                    "sigmaTargetCentring",
                    &mut self.sigma_target_centring,
                    true,
                    "Millimetres",
                );
                ui.label("target");
            });
            form_row(ui, "Heights (mm):", |ui| {
                changed |= number_ui(
                    ui,
                    "sigmaHeight",
                    &mut self.sigma_height,
                    true,
                    "Instrument and target heights, millimetres",
                );
            });
            form_row(ui, "Levelling (mm/sqrt km):", |ui| {
                changed |= number_ui(
                    ui,
                    "sigmaLevelling",
                    &mut self.sigma_levelling,
                    true,
                    "Millimetres per square root of a kilometre",
                );
            });
            form_row(ui, "GNSS (mm):", |ui| {
                changed |= number_ui(
                    ui,
                    "sigmaGnssHorizontal",
                    &mut self.sigma_gnss_horizontal,
                    true,
                    "Where the file gives no covariance, millimetres",
                );
                ui.label("horizontal,");
                changed |= number_ui(
                    ui,
                    "sigmaGnssVertical",
                    &mut self.sigma_gnss_vertical,
                    true,
                    "Where the file gives no covariance, millimetres",
                );
                ui.label("vertical");
            });
            changed |= drive::check(
                ui,
                "useFileCovariances",
                "Use the covariances a GNSS file states",
                &mut self.use_file_covariances,
            )
            .changed();
        });
        ui.group(|ui| {
            ui.strong("Corrections and scale");
            form_row(ui, "Refraction k:", |ui| {
                changed |= number_ui(
                    ui,
                    "refractionK",
                    &mut self.refraction_k,
                    curvature,
                    "Coefficient of refraction k, 0.13 by day",
                );
            });
            form_row(ui, "Earth radius (m):", |ui| {
                changed |= number_ui(ui, "earthRadius", &mut self.earth_radius, true, "Metres");
            });
            form_row(ui, "Fixed ppm:", |ui| {
                changed |= number_ui(
                    ui,
                    "fixedPpm",
                    &mut self.fixed_ppm,
                    fixed_ppm,
                    "Applied to every distance the instrument did not correct, when the \
                     atmospheric choice is a fixed ppm",
                );
            });
            form_row(ui, "Prism constant (mm):", |ui| {
                changed |= number_ui(
                    ui,
                    "prismConstantValue",
                    &mut self.prism_constant_value,
                    prism_value,
                    "Millimetres, when the prism constant is overridden",
                );
            });
            form_row(ui, "Grid scale:", |ui| {
                changed |= ui
                    .add_enabled_ui(!combined, |ui| {
                        choice_of(ui, "gridScale", &labels(&GRID_SCALE), &mut self.grid_scale)
                            .changed()
                    })
                    .inner;
                changed |= number_ui(
                    ui,
                    "gridScaleFactor",
                    &mut self.grid_scale_factor,
                    grid_factor,
                    "The factor, when it is fixed",
                );
            });
            form_row(ui, "", |ui| {
                changed |= drive::check(
                    ui,
                    "useCombinedFactor",
                    "A combined factor instead of height and grid",
                    &mut self.use_combined_factor,
                )
                .changed();
                changed |= number_ui(
                    ui,
                    "combinedFactor",
                    &mut self.combined_factor,
                    combined,
                    "The combined scale factor, e.g. from a control certificate",
                );
            });
            form_row(ui, "Height reduction:", |ui| {
                changed |= ui
                    .add_enabled_ui(!combined, |ui| {
                        choice_of(
                            ui,
                            "heightReduction",
                            &labels(&HEIGHTS),
                            &mut self.height_reduction,
                        )
                        .changed()
                    })
                    .inner;
            });
        });
        ui.group(|ui| {
            ui.strong("Statistics");
            form_row(ui, "Confidence (%):", |ui| {
                changed |= number_ui(
                    ui,
                    "confidence",
                    &mut self.confidence,
                    true,
                    "Of the error ellipses and the global test, percent",
                );
            });
            form_row(ui, "Outlier test:", |ui| {
                changed |= choice_of(
                    ui,
                    "outlierTest",
                    &labels(&OUTLIER_TESTS),
                    &mut self.outlier_test,
                )
                .changed();
            });
            form_row(ui, "Significance:", |ui| {
                changed |= number_ui(
                    ui,
                    "outlierSignificance",
                    &mut self.outlier_significance,
                    testing,
                    "Per observation: 0.001 is Baarda's, critical value 3.29",
                );
            });
            changed |= ui
                .add_enabled_ui(testing, |ui| {
                    drive::check(
                        ui,
                        "autoReject",
                        "Reject the worst outlier and adjust again, one at a time",
                        &mut self.auto_reject,
                    )
                    .changed()
                })
                .inner;
            form_row(ui, "Iterations:", |ui| {
                changed |= drive::whole(ui, "iterations", &mut self.iterations, 1..=1000).changed();
            });
        });
        changed
    }
}
