//! render_text and render_html: the reduction report for people.
//!
//! One walk over the report feeds both: each section is built as a table of
//! cells (header, rows), then written as fixed-width text or as an HTML table.
//! The text pads by displayed characters, not bytes, so a degree sign or an
//! accented point name does not push its column out of line.

use katana_math::numerics::PI;

use crate::data_model::{ControlComponent, ControlConstraint, Face, SourceRecord, SurveyText};
use crate::reduction::format_number;
use crate::reduction_report::{
    AdjustmentReport, AppliedCorrection, ReductionReport, ResectionReport,
};
use crate::reduction_settings::{
    AdjustmentMethod, AtmosphericCorrection, GridScale, PrismConstantPolicy, ReductionSettings,
};

// The C++ writes these with std::to_chars rather than snprintf (a report of
// 100 000 observations formats about a million numbers); the two write the same
// digits, which reduction::format_number gives.
fn fixed(value: f64, decimals: usize) -> String {
    format_number(value, decimals)
}

// "+0.0" for anything that rounds to zero, whatever its sign: a residual of
// -1e-15 is not a negative millimetre.
fn signed_fixed(value: f64, decimals: usize) -> String {
    let digits = fixed(value.abs(), decimals);
    let zero = digits.bytes().all(|byte| byte == b'0' || byte == b'.');
    format!("{}{digits}", if value < 0.0 && !zero { "-" } else { "+" })
}

// 123°45'06.7" (seconds to `decimals`), rounding carried into the minutes.
fn dms(radians: f64, decimals: usize) -> String {
    if !radians.is_finite() {
        return String::from("-");
    }
    let negative = radians < 0.0;
    let scale = 10.0_f64.powf(decimals as f64);
    let total_tenths = (radians.abs() * 648000.0 / PI * scale).round();
    // A cast the C++ makes unguarded (undefined there out of range); Rust
    // saturates, for an angle of more than 10^12 degrees.
    let whole = total_tenths as i64;
    let units_per_minute = (60.0 * scale) as i64;
    let units_per_degree = units_per_minute * 60;
    let degrees = whole / units_per_degree;
    let minutes = (whole % units_per_degree) / units_per_minute;
    let seconds = (whole % units_per_minute) as f64 / scale;
    let mut text = String::from(if negative { "-" } else { "" });
    text.push_str(&format!("{degrees}"));
    text.push('\u{00B0}');
    text.push_str(if minutes < 10 { "0" } else { "" });
    text.push_str(&format!("{minutes}"));
    text.push('\'');
    text.push_str(if seconds < 10.0 { "0" } else { "" });
    text.push_str(&fixed(seconds, decimals));
    text.push('"');
    text
}

fn seconds(radians: f64) -> String {
    signed_fixed(radians * 648000.0 / PI, 1) + "\""
}

fn millimetres(metres: f64) -> String {
    signed_fixed(metres * 1000.0, 1) + " mm"
}

fn metres(value: f64, decimals: usize) -> String {
    fixed(value, decimals)
}

fn optional_metres(value: Option<f64>) -> String {
    value.map_or_else(|| String::from("-"), |value| metres(value, 3))
}

fn optional_millimetres(value: Option<f64>) -> String {
    value.map_or_else(|| String::from("-"), millimetres)
}

fn yes_no(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn source_text(source: &SourceRecord) -> String {
    if !source.known() {
        return String::new();
    }
    let mut text = source.file_name.clone();
    if source.record_number != 0 {
        text.push_str(if text.is_empty() { "record " } else { ":" });
        text.push_str(&format!("{}", source.record_number));
    }
    text
}

// A correction's size in the unit a surveyor reads it in: seconds and
// millimetres for the small ones, degrees-minutes-seconds and metres for an
// orientation or a slope reduction ("-100 00 01", not "-360001.0\"").
fn correction_amount(amount: f64, angular: bool) -> String {
    if angular {
        if amount.abs() < 60.0 * PI / 648000.0 {
            return seconds(amount);
        }
        return format!(
            "{}{}",
            if amount < 0.0 { "-" } else { "+" },
            dms(amount.abs(), 1)
        );
    }
    if amount.abs() < 1.0 {
        return millimetres(amount);
    }
    signed_fixed(amount, 4) + " m"
}

fn correction_text(correction: &AppliedCorrection, angular: bool) -> String {
    let mut text = String::from(correction.kind.text());
    text.push(' ');
    text.push_str(&correction_amount(correction.amount, angular));
    if let Some(factor) = correction.factor {
        text.push_str(&format!(" (x{})", fixed(factor, 8)));
    }
    if !correction.note.is_empty() {
        text.push_str(&format!(" [{}]", correction.note));
    }
    text
}

fn value_text(value: f64, angular: bool) -> String {
    if angular {
        dms(value, 1)
    } else {
        metres(value, 4)
    }
}

// ---- Sections as tables ----------------------------------------------------------

struct Table {
    title: String,
    header: Vec<String>,
    rows: Vec<Vec<String>>,
    // Key/value sections are rendered without a header line.
    key_value: bool,
    empty: String,
}

impl Table {
    fn new(title: impl Into<String>, header: &[&str]) -> Self {
        Self {
            title: title.into(),
            header: header.iter().map(|cell| String::from(*cell)).collect(),
            rows: Vec::new(),
            key_value: false,
            empty: String::from("none"),
        }
    }

    fn pairs(title: impl Into<String>) -> Self {
        Self {
            key_value: true,
            ..Self::new(title, &["", ""])
        }
    }

    fn push(&mut self, cells: Vec<String>) {
        self.rows.push(cells);
    }

    fn pair(&mut self, key: &str, value: impl Into<String>) {
        self.rows.push(vec![String::from(key), value.into()]);
    }
}

fn input_table(report: &ReductionReport) -> Table {
    let input = &report.input;
    let mut table = Table::pairs("Input");
    table.pair("File", input.file_name.clone());
    let mut format = input.format_name.clone();
    if !input.format_id.is_empty() {
        format.push_str(&format!(" ({})", input.format_id));
    }
    if !input.parser_version.is_empty() {
        format.push_str(&format!(", parser {}", input.parser_version));
    }
    table.pair("Format", format);
    table.pair(
        "Records",
        format!(
            "{} read, {} skipped",
            input.records_read, input.records_skipped
        ),
    );
    for sibling in &input.sibling_files {
        table.pair("Also read", sibling.clone());
    }
    for missing in &input.not_carried {
        table.pair("Not in the file", missing.clone());
    }
    for warning in &input.warnings {
        let place = source_text(&warning.source);
        table.pair(
            "Reader warning",
            if place.is_empty() {
                warning.text.clone()
            } else {
                format!("{place}: {}", warning.text)
            },
        );
    }
    table.pair("Created", report.created_utc.clone());
    table
}

fn constraint_text(component: &ControlComponent) -> String {
    match component.constraint {
        ControlConstraint::Free => String::from("free"),
        ControlConstraint::Fixed => String::from("fixed"),
        ControlConstraint::Weighted => {
            format!("weighted {} mm", fixed(component.sigma * 1000.0, 1))
        }
    }
}

fn settings_table(s: &ReductionSettings) -> Table {
    let mut table = Table::pairs("Settings used");
    let mut atmospheric = String::from(s.atmospheric.text());
    if s.atmospheric == AtmosphericCorrection::Fixed {
        atmospheric.push_str(&format!(" {} ppm", signed_fixed(s.fixed_ppm, 1)));
    }
    table.pair("Atmospheric correction", atmospheric);
    let mut prism = String::from(s.prism_constant_policy.text());
    if s.prism_constant_policy == PrismConstantPolicy::Override {
        prism.push_str(&format!(" {}", millimetres(s.prism_constant)));
    }
    table.pair("Prism constant", prism);
    table.pair(
        "Faces",
        format!(
            "{}; tolerances {}\" horizontal, {}\" zenith, {} mm; outside: {}",
            s.faces.text(),
            fixed(s.face_tolerances.horizontal * 648000.0 / PI, 1),
            fixed(s.face_tolerances.zenith * 648000.0 / PI, 1),
            fixed(s.face_tolerances.distance * 1000.0, 1),
            if s.face_tolerances.exclude_outside {
                "excluded"
            } else {
                "used and flagged"
            }
        ),
    );
    table.pair("Slope to horizontal", yes_no(s.slope_to_horizontal));
    table.pair(
        "Curvature and refraction",
        format!(
            "{}, k = {}, R = {} m",
            yes_no(s.curvature_and_refraction),
            fixed(s.refraction_coefficient, 3),
            fixed(s.earth_radius, 0)
        ),
    );
    if s.use_combined_factor {
        table.pair(
            "Combined scale factor",
            format!(
                "{} (in place of height and grid)",
                fixed(s.combined_factor, 8)
            ),
        );
    } else {
        table.pair("Height reduction", s.height_reduction.text());
        let mut grid = String::from(s.grid_scale.text());
        if s.grid_scale == GridScale::Fixed {
            grid.push_str(&format!(" {}", fixed(s.fixed_grid_scale_factor, 8)));
        }
        table.pair("Grid scale", grid);
    }
    let mut method = String::from(s.method.text());
    if s.method == AdjustmentMethod::None {
        method.push_str(" (radiation)");
    } else if s.method == AdjustmentMethod::Traverse {
        method.push_str(&format!(", {}", s.traverse_rule.text()));
    } else {
        method.push_str(&format!(", {}", s.network_dimension.text()));
    }
    table.pair("Adjustment", method);
    let a = &s.apriori;
    table.pair(
        "A-priori precision",
        format!(
            "direction {}\", zenith {}\", distance {} mm + {} ppm, centring {} / {} mm, heights {} mm",
            fixed(a.direction * 648000.0 / PI, 1),
            fixed(a.zenith * 648000.0 / PI, 1),
            fixed(a.distance_constant * 1000.0, 1),
            fixed(a.distance_ppm, 1),
            fixed(a.instrument_centring * 1000.0, 1),
            fixed(a.target_centring * 1000.0, 1),
            fixed(a.height_measurement * 1000.0, 1)
        ),
    );
    for control in &s.control {
        table.pair(
            "Control",
            format!(
                "{} ({}): N {}, E {}, H {}",
                control.point.point_id,
                control.origin.text(),
                constraint_text(&control.point.northing),
                constraint_text(&control.point.easting),
                constraint_text(&control.point.elevation)
            ),
        );
    }
    if s.control.is_empty() {
        table.pair("Control", "none chosen");
    }
    table.pair(
        "Confidence level",
        format!("{} %", fixed(s.confidence_level * 100.0, 1)),
    );
    table.pair(
        "Outlier test",
        format!(
            "{} at {}, auto-reject {}",
            s.outlier_test.text(),
            fixed(s.outlier_significance, 4),
            yes_no(s.auto_reject_outliers)
        ),
    );
    table
}

fn setup_table(report: &ReductionReport) -> Table {
    let mut table = Table::new(
        "Setups",
        &[
            "Setup",
            "Point",
            "HI",
            "Instrument",
            "Backsight",
            "BS azimuth",
            "BS reading",
            "Orientation",
            "BS dist check",
            "BS height check",
            "Source",
        ],
    );
    for setup in &report.setups {
        let mut instrument = setup.instrument.make.clone();
        if !setup.instrument.model.is_empty() {
            if !instrument.is_empty() {
                instrument.push(' ');
            }
            instrument.push_str(&setup.instrument.model);
        }
        table.push(vec![
            setup.station_id.clone(),
            setup.point_id.clone(),
            metres(setup.instrument_height, 3),
            instrument,
            setup.backsight_point_id.clone(),
            setup
                .backsight_azimuth
                .map_or_else(|| String::from("-"), |value| dms(value, 1)),
            setup
                .backsight_reading
                .map_or_else(|| String::from("-"), |value| dms(value, 1)),
            setup
                .orientation_correction
                .map_or_else(|| String::from("not oriented"), |value| dms(value, 1)),
            optional_millimetres(setup.backsight_distance_difference),
            optional_millimetres(setup.backsight_height_difference),
            source_text(&setup.source),
        ]);
    }
    table
}

fn observation_table(report: &ReductionReport) -> Table {
    let mut table = Table::new(
        "Observations: raw value, each correction, reduced value",
        &[
            "Setup",
            "Kind",
            "From",
            "To",
            "Pointing",
            "Raw",
            "Corrections",
            "Reduced",
            "Source",
        ],
    );
    table.rows.reserve(report.observations.len());
    for observation in &report.observations {
        let mut corrections = String::new();
        for correction in &observation.corrections {
            if !corrections.is_empty() {
                corrections.push_str("; ");
            }
            corrections.push_str(&correction_text(correction, observation.angular));
        }
        let mut pointing = String::new();
        if observation.pointing.index != 0 || observation.pointing.face != Face::Unknown {
            pointing = format!("{}", observation.pointing.index);
            if observation.pointing.face != Face::Unknown {
                pointing.push_str(if observation.pointing.face == Face::Left {
                    " FL"
                } else {
                    " FR"
                });
            }
        }
        let reduced = if observation.rejected {
            format!("REJECTED: {}", observation.rejection_reason)
        } else if let Some(reduced) = observation.reduced {
            value_text(reduced, observation.angular)
        } else {
            String::from("-")
        };
        table.push(vec![
            observation.station_id.clone(),
            observation.kind.clone(),
            observation.from.clone(),
            observation.to.clone(),
            pointing,
            value_text(observation.raw, observation.angular),
            corrections,
            reduced,
            source_text(&observation.source),
        ]);
    }
    table
}

fn face_pair_table(report: &ReductionReport) -> Table {
    let mut table = Table::new(
        "Face left / face right pairs",
        &[
            "Setup",
            "Target",
            "FL",
            "FR",
            "Horizontal",
            "Zenith (2 x index)",
            "Distance",
            "Check",
        ],
    );
    for pair in &report.face_pairs {
        table.push(vec![
            pair.station_id.clone(),
            pair.target_id.clone(),
            format!("{}", pair.left_pointing),
            format!("{}", pair.right_pointing),
            pair.horizontal_spread
                .map_or_else(|| String::from("-"), seconds),
            pair.zenith_spread
                .map_or_else(|| String::from("-"), seconds),
            optional_millimetres(pair.distance_spread),
            String::from(if pair.within_tolerance {
                "within tolerance"
            } else {
                "OUTSIDE TOLERANCE"
            }),
        ]);
    }
    table
}

fn misclosure_table(report: &ReductionReport) -> Table {
    let mut table = Table::new(
        "Misclosures and checks",
        &[
            "Name",
            "Angular",
            "Allowable",
            "dN",
            "dE",
            "Linear",
            "Length",
            "1 : N",
            "Height",
            "Check",
        ],
    );
    for m in &report.misclosures {
        table.push(vec![
            m.name.clone(),
            m.angular.map_or_else(|| String::from("-"), seconds),
            m.angular_allowable
                .map_or_else(|| String::from("-"), seconds),
            optional_millimetres(m.northing),
            optional_millimetres(m.easting),
            optional_millimetres(m.linear),
            metres(m.length, 3),
            m.precision_ratio.map_or_else(
                || String::from("-"),
                |ratio| format!("1 : {}", fixed(ratio, 0)),
            ),
            optional_millimetres(m.height),
            String::from(match m.within_tolerance {
                Some(true) => "within",
                Some(false) => "OUTSIDE",
                None => "-",
            }),
        ]);
    }
    table
}

// One least squares' residuals, as the adjustments and the resections print them.
fn residual_table(adjustment: &AdjustmentReport) -> Table {
    let mut residuals = Table::new(
        format!("Residuals: {}", adjustment.method),
        &[
            "Observation",
            "Residual",
            "A-priori sigma",
            "Redundancy",
            "Standardised",
            "Flag",
            "Source",
        ],
    );
    for residual in &adjustment.residuals {
        residuals.push(vec![
            residual.observation.clone(),
            if residual.angular {
                seconds(residual.residual)
            } else {
                millimetres(residual.residual)
            },
            if residual.angular {
                seconds(residual.sigma)
            } else {
                millimetres(residual.sigma)
            },
            fixed(residual.redundancy_number, 2),
            residual
                .standardised
                .map_or_else(|| String::from("-"), |value| signed_fixed(value, 2)),
            String::from(if residual.rejected {
                "REJECTED"
            } else if residual.flagged {
                "FLAGGED"
            } else {
                ""
            }),
            source_text(&residual.source),
        ]);
    }
    residuals
}

fn unit_weight(adjustment: &AdjustmentReport) -> String {
    adjustment
        .variance_factor
        .map_or_else(|| String::from("-"), |factor| fixed(factor.sqrt(), 2))
}

// Which way a resection's global test failed, as the network's summary says it
// with its bounds: a fit too good fails it as surely as one too bad, and the
// two mean different things (weights too loose, or an error).
fn failed_global_test(what: &str, adjustment: &AdjustmentReport) -> Option<String> {
    let test = adjustment.global_test.filter(|test| !test.passed)?;
    let below = test.statistic < test.lower_critical;
    Some(format!(
        "{what} global test FAILED, {}{}",
        fixed(test.statistic, 3),
        if below {
            format!(" below {} (fits too well)", fixed(test.lower_critical, 3))
        } else {
            format!(" above {}", fixed(test.upper_critical, 3))
        }
    ))
}

fn resection_check(resection: &ResectionReport) -> String {
    let mut check = String::new();
    let mut add = |part: &str| {
        if !check.is_empty() {
            check.push_str("; ");
        }
        check.push_str(part);
    };
    let horizontal = &resection.horizontal;
    let height = resection.height.as_ref();
    if !horizontal.flagged_outliers.is_empty()
        || height.is_some_and(|height| !height.flagged_outliers.is_empty())
    {
        add("FLAGGED");
    }
    if let Some(failed) = failed_global_test("plan", horizontal) {
        add(&failed);
    }
    if let Some(failed) = height.and_then(|height| failed_global_test("height", height)) {
        add(&failed);
    }
    if horizontal.redundancy == 0 {
        add("no redundancy");
    }
    if resection.weak_geometry {
        add("WEAK GEOMETRY");
    }
    if let (Some(northing), Some(easting)) = (
        resection.file_northing_difference,
        resection.file_easting_difference,
    ) {
        add(&format!(
            "the file's own coordinates {} away",
            millimetres(northing.hypot(easting))
        ));
    }
    if let (Some(northing), Some(easting)) = (
        resection.radiated_northing_difference,
        resection.radiated_easting_difference,
    ) {
        add(&format!(
            "radiated from {} {} away",
            resection.radiated_from,
            millimetres(northing.hypot(easting))
        ));
    }
    if let Some(scale) = resection.unapplied_scale_factor {
        add(&format!(
            "ground distances on grid coordinates (scale factor {} not applied)",
            fixed(scale, 6)
        ));
    }
    check
}

// The setups positioned by resection: one row each (where it put the station,
// how well, from which points) and then each one's residuals. The summary is a
// table, not key-value rows: the Survey Jobs dialog reads the network's
// "Variance factor" rows out of the text, and a resection's are not the
// network's.
fn resection_tables(report: &ReductionReport) -> Vec<Table> {
    let mut tables = Vec::new();
    if report.resections.is_empty() {
        return tables;
    }
    let mut summary = Table::new(
        "Resections: setups positioned from the points they observe",
        &[
            "Setup",
            "Point",
            "From",
            "Northing",
            "Easting",
            "Height",
            "sN",
            "sE",
            "sH",
            "Orientation",
            "s orientation",
            "Redundancy",
            "Unit weight",
            "Dilution",
            "Checks",
            "Check",
        ],
    );
    for resection in &report.resections {
        // As the C++ joins them: an empty id adds no separator.
        let mut from = String::new();
        for target in &resection.targets {
            if !from.is_empty() {
                from.push_str(", ");
            }
            from.push_str(target);
        }
        let horizontal = &resection.horizontal;
        let height = resection.height.as_ref();
        let check = resection_check(resection);
        summary.push(vec![
            resection.station_id.clone(),
            resection.point_id.clone(),
            from,
            metres(resection.northing, 3),
            metres(resection.easting, 3),
            optional_metres(resection.elevation),
            fixed(resection.sigma_northing * 1000.0, 1),
            fixed(resection.sigma_easting * 1000.0, 1),
            resection
                .sigma_elevation
                .map_or_else(|| String::from("-"), |sigma| fixed(sigma * 1000.0, 1)),
            dms(resection.orientation, 1),
            format!(
                "{}\"",
                fixed(resection.sigma_orientation * 648000.0 / PI, 1)
            ),
            match height {
                Some(height) => format!("{} + {}", horizontal.redundancy, height.redundancy),
                None => format!("{}", horizontal.redundancy),
            },
            match height {
                Some(height) => format!("{} / {}", unit_weight(horizontal), unit_weight(height)),
                None => unit_weight(horizontal),
            },
            fixed(resection.dilution, 2),
            format!("{}", resection.checks),
            check,
        ]);
    }
    tables.push(summary);
    for resection in &report.resections {
        tables.push(residual_table(&resection.horizontal));
        if let Some(height) = &resection.height {
            tables.push(residual_table(height));
        }
    }
    tables
}

fn adjustment_tables(report: &ReductionReport) -> Vec<Table> {
    let mut tables = Vec::new();
    for adjustment in &report.adjustments {
        let mut summary = Table::pairs(format!("Adjustment: {}", adjustment.method));
        summary.pair("Unknowns", format!("{}", adjustment.unknowns));
        summary.pair("Observations", format!("{}", adjustment.observations));
        summary.pair("Redundancy", format!("{}", adjustment.redundancy));
        summary.pair("Iterations", format!("{}", adjustment.iterations));
        summary.pair(
            "Variance factor",
            match adjustment.variance_factor {
                Some(factor) => format!(
                    "{} (standard error of unit weight {})",
                    fixed(factor, 3),
                    fixed(factor.sqrt(), 3)
                ),
                None => String::from("undefined (no redundancy)"),
            },
        );
        if let Some(test) = &adjustment.global_test {
            summary.pair(
                "Chi-square global test",
                format!(
                    "{} against [{}, {}] at {}: {}",
                    fixed(test.statistic, 3),
                    fixed(test.lower_critical, 3),
                    fixed(test.upper_critical, 3),
                    fixed(test.significance_level, 3),
                    if test.passed { "passed" } else { "FAILED" }
                ),
            );
        }
        for flagged in &adjustment.flagged_outliers {
            summary.pair("Flagged outlier", flagged.clone());
        }
        for rejected in &adjustment.rejected_outliers {
            summary.pair("Rejected outlier", rejected.clone());
        }
        tables.push(summary);
        tables.push(residual_table(adjustment));

        if !adjustment.ellipses.is_empty() {
            let mut ellipses = Table::new(
                format!("Error ellipses: {}", adjustment.method),
                &[
                    "Point",
                    "Semi-major (1 sigma)",
                    "Semi-minor (1 sigma)",
                    "Orientation",
                    "Confidence scale",
                    "Semi-major at confidence",
                    "Semi-minor at confidence",
                ],
            );
            for ellipse in &adjustment.ellipses {
                ellipses.push(vec![
                    ellipse.point_id.clone(),
                    millimetres(ellipse.standard.semi_major),
                    millimetres(ellipse.standard.semi_minor),
                    dms(ellipse.standard.orientation, 0),
                    fixed(ellipse.confidence_scale, 4),
                    millimetres(ellipse.standard.semi_major * ellipse.confidence_scale),
                    millimetres(ellipse.standard.semi_minor * ellipse.confidence_scale),
                ]);
            }
            tables.push(ellipses);
        }
    }
    tables
}

fn coordinate_table(report: &ReductionReport) -> Table {
    let mut table = Table::new(
        "Coordinates",
        &[
            "Point", "Northing", "Easting", "Height", "sN", "sE", "sH", "Method", "Shift N",
            "Shift E", "Shift H",
        ],
    );
    table.rows.reserve(report.coordinates.len());
    let sigma = |value: Option<f64>| {
        value.map_or_else(|| String::from("-"), |sigma| fixed(sigma * 1000.0, 1))
    };
    for c in &report.coordinates {
        table.push(vec![
            c.point_id.clone(),
            metres(c.northing, 3),
            metres(c.easting, 3),
            optional_metres(c.elevation),
            sigma(c.sigma_northing),
            sigma(c.sigma_easting),
            sigma(c.sigma_elevation),
            String::from(c.method.text()),
            optional_millimetres(c.shift_northing),
            optional_millimetres(c.shift_easting),
            optional_millimetres(c.shift_elevation),
        ]);
    }
    table
}

fn warning_table(report: &ReductionReport) -> Table {
    let mut table = Table::new("Warnings", &["Warning", "Source"]);
    for warning in &report.warnings {
        table.push(vec![warning.text.clone(), source_text(&warning.source)]);
    }
    table
}

fn all_tables(report: &ReductionReport) -> Vec<Table> {
    let mut tables = vec![
        input_table(report),
        settings_table(&report.settings),
        warning_table(report),
        setup_table(report),
        face_pair_table(report),
        observation_table(report),
        misclosure_table(report),
    ];
    tables.extend(resection_tables(report));
    tables.extend(adjustment_tables(report));
    tables.push(coordinate_table(report));
    tables
}

// ---- Text ------------------------------------------------------------------------

// Displayed characters: UTF-8 continuation bytes do not count.
fn display_width(text: &str) -> usize {
    text.bytes().filter(|byte| (byte & 0xC0) != 0x80).count()
}

fn pad(out: &mut String, text: &str, width: usize) {
    out.push_str(text);
    for _ in display_width(text)..width {
        out.push(' ');
    }
}

// A cell wider than this is not allowed to widen its whole column: the long
// notes (corrections, warnings) go last in their row and run on.
const MAX_COLUMN: usize = 28;

fn render_text_table(out: &mut String, table: &Table) {
    out.push_str(&table.title);
    out.push('\n');
    out.push_str(&"=".repeat(display_width(&table.title)));
    out.push('\n');
    if table.rows.is_empty() {
        out.push_str(&format!("  {}\n\n", table.empty));
        return;
    }
    let columns = table.header.len();
    let mut widths = vec![0; columns];
    for (c, width) in widths.iter_mut().enumerate() {
        if !table.key_value {
            *width = display_width(&table.header[c]);
        }
        for cells in &table.rows {
            *width = (*width).max(display_width(&cells[c]).min(MAX_COLUMN));
        }
    }
    let line = |out: &mut String, cells: &[String]| {
        let mut text = String::from("  ");
        for (c, cell) in cells.iter().enumerate().take(columns) {
            if c + 1 == columns {
                text.push_str(cell);
            } else {
                pad(&mut text, cell, widths[c]);
                text.push_str("  ");
            }
        }
        while text.ends_with(' ') {
            text.pop();
        }
        out.push_str(&text);
        out.push('\n');
    };
    if !table.key_value {
        line(out, &table.header);
        let mut rule = String::from("  ");
        for (c, width) in widths.iter().enumerate() {
            let width = if c + 1 == columns {
                display_width(&table.header[c])
            } else {
                *width
            };
            rule.push_str(&"-".repeat(width));
            if c + 1 != columns {
                rule.push_str("  ");
            }
        }
        out.push_str(&rule);
        out.push('\n');
    }
    for cells in &table.rows {
        line(out, cells);
    }
    out.push('\n');
}

// ---- HTML ------------------------------------------------------------------------

fn escape(out: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(character),
        }
    }
}

fn is_alarm(cell: &str) -> bool {
    cell.starts_with("REJECTED")
        || cell == "FLAGGED"
        || cell == "OUTSIDE TOLERANCE"
        || cell == "OUTSIDE"
        || cell.contains("FAILED")
}

fn render_html_table(out: &mut String, table: &Table) {
    out.push_str("<section><h2>");
    escape(out, &table.title);
    out.push_str("</h2>\n");
    if table.rows.is_empty() {
        out.push_str("<p class=\"empty\">");
        escape(out, &table.empty);
        out.push_str("</p></section>\n");
        return;
    }
    out.push_str(if table.key_value {
        "<table class=\"kv\">\n"
    } else {
        "<table>\n<thead><tr>"
    });
    if !table.key_value {
        for heading in &table.header {
            out.push_str("<th>");
            escape(out, heading);
            out.push_str("</th>");
        }
        out.push_str("</tr></thead>\n");
    }
    out.push_str("<tbody>\n");
    for cells in &table.rows {
        out.push_str("<tr>");
        for (c, cell) in cells.iter().enumerate() {
            let head = table.key_value && c == 0;
            // A long cell (a correction chain, a sentence) wraps instead of
            // pushing the reduced value off a printed page. Its length in
            // bytes, as the C++ measures it.
            let note = cell.len() > MAX_COLUMN;
            if head {
                out.push_str("<th>");
            } else if is_alarm(cell) {
                out.push_str(if note {
                    "<td class=\"alarm note\">"
                } else {
                    "<td class=\"alarm\">"
                });
            } else {
                out.push_str(if note { "<td class=\"note\">" } else { "<td>" });
            }
            escape(out, cell);
            out.push_str(if head { "</th>" } else { "</td>" });
        }
        out.push_str("</tr>\n");
    }
    out.push_str("</tbody></table></section>\n");
}

/// Plain text, for the job's log and a terminal: sections in the order of the
/// report, angles in degrees-minutes-seconds, lengths in metres to the
/// millimetre.
pub fn render_text(report: &ReductionReport) -> String {
    let mut out = String::with_capacity(256 + report.observations.len() * 160);
    out.push_str("REDUCTION REPORT\n\n");
    for table in &all_tables(report) {
        render_text_table(&mut out, table);
    }
    out
}

/// A self-contained HTML document (inline CSS, no scripts, no external
/// resources) for the wizard's report page and a saved report. Every string
/// from the file is escaped: a point id is user data.
pub fn render_html(report: &ReductionReport) -> String {
    let mut out = String::with_capacity(2048 + report.observations.len() * 260);
    out.push_str(
        "<!DOCTYPE html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><title>Reduction report",
    );
    if !report.input.file_name.is_empty() {
        out.push_str(" - ");
        escape(&mut out, &report.input.file_name);
    }
    out.push_str(concat!(
        "</title>\n<style>\n",
        "body{font-family:Segoe UI,Helvetica,Arial,sans-serif;font-size:10pt;margin:1.5em;",
        "color:#1a1a1a;background:#fff}\n",
        "h1{font-size:16pt;margin:0 0 .3em}h2{font-size:12pt;margin:1.4em 0 .4em;",
        "border-bottom:1px solid #999}\n",
        "table{border-collapse:collapse;font-variant-numeric:tabular-nums;width:auto}\n",
        "th,td{border:1px solid #ccc;padding:2px 6px;text-align:left;vertical-align:top;",
        "white-space:nowrap}\n",
        "td:nth-last-child(2),td:last-child{white-space:normal}\n",
        "td.note{white-space:normal;min-width:22em}\n",
        "thead th{background:#eee}table.kv th{background:#f5f5f5;font-weight:600}\n",
        "td.alarm{color:#a00;font-weight:600}p.empty{color:#666}\n",
        "@media print{body{margin:0;font-size:8pt}section{break-inside:auto}",
        "thead{display:table-header-group}tr{break-inside:avoid}}\n",
        "</style></head>\n<body>\n<h1>Reduction report",
    ));
    if !report.input.file_name.is_empty() {
        out.push_str(": ");
        escape(&mut out, &report.input.file_name);
    }
    out.push_str("</h1>\n");
    for table in &all_tables(report) {
        render_html_table(&mut out, table);
    }
    out.push_str("</body></html>\n");
    out
}
