//! Checking QL-B detections against QL-A exposures.
//!
//! AS 5488 leaves a detection's QL-B grade resting on the locator's stated
//! uncertainty. The only independent test of that statement is to dig: expose
//! the service at a detected position, survey it, and compare. An investigation
//! that has done so can say how its detections actually performed; one that
//! has not can only repeat what the equipment claims.
//!
//! An exposure names the detection it checks through UtilityVertex::verifies.
//! The comparison is between the two surveyed positions of the SAME service,
//! so both levels are brought to the top of the service first
//! (utility_network, top_level) - a detection recorded to the centre and an
//! exposure recorded to the crown differ by half a diameter that is not error.

use super::quality_level::{QualityLevel, classify, to_string};
use super::utility_network::{
    GradingSettings, UtilityLine, UtilityVertex, has_vertical_measurement, top_level,
};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VerificationResult {
    pub line_id: String,
    pub detected_id: String,
    pub exposed_id: String,
    /// Plan distance between the two, metres.
    pub horizontal_deviation: f64,
    /// Exposed top minus detected top; None when either has no level.
    pub vertical_deviation: Option<f64>,
    /// Within the QL-B tolerances.
    pub horizontal_within: bool,
    pub vertical_within: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VerificationReport {
    pub results: Vec<VerificationResult>,
    /// Exposures that could not be used, one sentence each: the detection they
    /// name does not exist, or the exposure is not itself QL-A and so is not a
    /// check of anything.
    pub problems: Vec<String>,

    pub horizontal_passed: usize,
    pub vertical_checked: usize,
    pub vertical_passed: usize,
    pub maximum_horizontal: f64,
    pub rms_horizontal: f64,
    /// Largest |vertical deviation|.
    pub maximum_vertical: Option<f64>,
    pub rms_vertical: Option<f64>,
}

/// `std::max(a, b)`: `b` only when `a < b`, so a NaN in `b` is passed over as
/// the C++ passes it over.
fn larger(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

/// The vertex `id` on `own` first, then on any other line.
fn find<'a>(
    lines: &'a [UtilityLine],
    own: &'a UtilityLine,
    id: &str,
) -> Option<(&'a UtilityLine, &'a UtilityVertex)> {
    if let Some(vertex) = own.vertices.iter().find(|vertex| vertex.id == id) {
        return Some((own, vertex));
    }
    for line in lines {
        if std::ptr::eq(line, own) {
            continue;
        }
        if let Some(vertex) = line.vertices.iter().find(|vertex| vertex.id == id) {
            return Some((line, vertex));
        }
    }
    None
}

/// Every exposure in `lines` that names a detection is compared with it. The
/// detection is looked for on the exposure's own line first, then on any line,
/// so a check dug where two services were confused is still made - and then
/// reported against the line the detection belongs to.
pub fn verify_detections(lines: &[UtilityLine], settings: &GradingSettings) -> VerificationReport {
    let mut report = VerificationReport::default();
    let b = &settings.tolerances.b;
    let mut sum_horizontal = 0.0_f64;
    let mut sum_vertical = 0.0_f64;

    for line in lines {
        for exposure in &line.vertices {
            if exposure.verifies.is_empty() {
                continue;
            }
            let place = format!("{} / {}", line.id, exposure.id);
            let mut evidence = exposure.evidence;
            evidence.has_level = has_vertical_measurement(exposure);
            let grade = classify(&evidence, &settings.tolerances);
            if grade.level != QualityLevel::A {
                report.problems.push(format!(
                    "{place} checks {} but is {}, not QL-A: only an exposure verifies a detection",
                    exposure.verifies,
                    to_string(grade.level)
                ));
                continue;
            }
            let Some((detected_line, detected)) = find(lines, line, &exposure.verifies) else {
                report.problems.push(format!(
                    "{place} checks {}, which no line has",
                    exposure.verifies
                ));
                continue;
            };

            let mut result = VerificationResult {
                line_id: detected_line.id.clone(),
                detected_id: detected.id.clone(),
                exposed_id: exposure.id.clone(),
                horizontal_deviation: (exposure.position.northing - detected.position.northing)
                    .hypot(exposure.position.easting - detected.position.easting),
                ..Default::default()
            };
            result.horizontal_within = b
                .horizontal
                .is_some_and(|tolerance| result.horizontal_deviation <= tolerance);

            let exposed_top = top_level(exposure, line.attributes.diameter);
            let detected_top = top_level(detected, detected_line.attributes.diameter);
            if let (Some(exposed_top), Some(detected_top)) = (exposed_top, detected_top) {
                let deviation = exposed_top - detected_top;
                result.vertical_deviation = Some(deviation);
                if let Some(tolerance) = b.vertical {
                    result.vertical_within = Some(deviation.abs() <= tolerance);
                }
            }

            report.horizontal_passed += usize::from(result.horizontal_within);
            report.maximum_horizontal =
                larger(report.maximum_horizontal, result.horizontal_deviation);
            sum_horizontal += result.horizontal_deviation * result.horizontal_deviation;
            if let Some(deviation) = result.vertical_deviation {
                let magnitude = deviation.abs();
                report.vertical_checked += 1;
                report.vertical_passed += usize::from(result.vertical_within.unwrap_or(false));
                report.maximum_vertical =
                    Some(larger(report.maximum_vertical.unwrap_or(0.0), magnitude));
                sum_vertical += magnitude * magnitude;
            }
            report.results.push(result);
        }
    }

    if !report.results.is_empty() {
        report.rms_horizontal = (sum_horizontal / report.results.len() as f64).sqrt();
    }
    if report.vertical_checked > 0 {
        report.rms_vertical = Some((sum_vertical / report.vertical_checked as f64).sqrt());
    }
    report
}
