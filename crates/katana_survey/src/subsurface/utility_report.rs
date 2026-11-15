//! Plain-text reports of a subsurface utility investigation, for the command
//! line and for a person to read beside the drawing.
//!
//! Numbers are metres to three decimals unless the report says otherwise. Each
//! report ends with its findings - the lines a reviewer has to act on - so that
//! a report with none says so in words rather than by omission.

use std::collections::BTreeMap;

use katana_core::Result;

use super::clearance::{
    Clash, ClearanceRequirement, ClearanceResult, ClearanceStatus, DesignAlignment, clash_of,
};
use super::quality_level::{QualityLevel, Tolerance, to_string};
use super::utility_network::{
    GradingSettings, UtilityAttributes, UtilityLine, UtilityStatus, UtilityType, depth_of_cover,
    grade_line,
};
use super::verification::VerificationReport;

const BEST_FIRST: [QualityLevel; 4] = [
    QualityLevel::A,
    QualityLevel::B,
    QualityLevel::C,
    QualityLevel::D,
];

/// `std::format("{:.Nf}", value)`. Rust's fixed digits are the C++'s for
/// every finite value and an infinity (core's number oracle holds
/// format_fixed, which formats the same way, to them); unlike format_fixed,
/// std::format keeps the sign of a value that rounds to zero ("-0.000"), and
/// it spells a NaN "nan" or "-nan" where Rust writes "NaN".
fn fixed(value: f64, decimals: usize) -> String {
    if value.is_nan() {
        return String::from(if value.is_sign_negative() {
            "-nan"
        } else {
            "nan"
        });
    }
    format!("{value:.decimals$}")
}

fn metres(value: f64) -> String {
    fixed(value, 3)
}

fn millimetres(value: f64) -> String {
    format!("{} mm", fixed(value * 1000.0, 0))
}

fn tolerance_text(tolerance: &Tolerance) -> String {
    if tolerance.horizontal.is_none() && tolerance.vertical.is_none() {
        return String::from("none stated");
    }
    let mut text = String::new();
    if let Some(horizontal) = tolerance.horizontal {
        text += &format!("+/-{} H", millimetres(horizontal));
    }
    if let Some(vertical) = tolerance.vertical {
        if !text.is_empty() {
            text += ", ";
        }
        text += &format!("+/-{} V", millimetres(vertical));
    }
    text
}

fn settings_text(settings: &GradingSettings) -> String {
    let mut text = String::from("Tolerances: ");
    text += &format!("QL-A {}", tolerance_text(&settings.tolerances.a));
    text += &format!("; QL-B {}", tolerance_text(&settings.tolerances.b));
    if settings.maximum_detected_spacing.is_finite() {
        text += &format!(
            "; detected spacing <= {} m",
            metres(settings.maximum_detected_spacing)
        );
    }
    text + "\n"
}

fn attributes_text(attributes: &UtilityAttributes) -> String {
    let mut text = String::from(to_string(attributes.r#type));
    if !attributes.owner.is_empty() {
        text += &format!(", {}", attributes.owner);
    }
    if !attributes.material.is_empty() {
        text += &format!(", {}", attributes.material);
    }
    if attributes.diameter > 0.0 {
        text += &format!(", {}", millimetres(attributes.diameter));
    }
    if !attributes.configuration.is_empty() {
        text += &format!(", {}", attributes.configuration);
    }
    text += &format!(", {}", to_string(attributes.status));
    text
}

fn missing_attributes(attributes: &UtilityAttributes) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if attributes.r#type == UtilityType::Unknown {
        missing.push("type");
    }
    if attributes.owner.is_empty() {
        missing.push("owner");
    }
    if attributes.status == UtilityStatus::Unknown {
        missing.push("status");
    }
    if attributes.material.is_empty() {
        missing.push("material");
    }
    // Negated so that a NaN diameter is missing too.
    if !(attributes.diameter > 0.0) {
        missing.push("size");
    }
    missing
}

fn lengths_text(length_at: &[f64; 4]) -> String {
    let mut text = String::new();
    for level in BEST_FIRST {
        text += &format!("{:>12}", metres(length_at[level as usize]));
    }
    text
}

/// Every line graded (vertices, segments, lengths at each quality level), depth
/// of cover where it can be computed, a summary of length by utility type and
/// quality level, and the findings: over-claimed quality levels, cover below
/// `minimum_cover`, levels that are not qualified, and the attributes AS 5488
/// asks for that were not recorded (type, owner, status, material, size).
pub fn render_investigation_report(
    lines: &[UtilityLine],
    settings: &GradingSettings,
    minimum_cover: Option<f64>,
) -> Result<String> {
    let mut body = String::new();
    let mut findings: Vec<String> = Vec::new();
    let mut by_type: BTreeMap<&'static str, [f64; 4]> = BTreeMap::new();
    let mut total = [0.0_f64; 4];
    let mut vertex_count = 0_usize;

    for line in lines {
        let graded = grade_line(line, settings)?;
        let cover = depth_of_cover(line, minimum_cover, settings)?;
        vertex_count += line.vertices.len();

        body += &format!(
            "\nLine {}: {}\n",
            line.id,
            attributes_text(&line.attributes)
        );
        body += &format!("  length {} m:", metres(graded.length()));
        for level in BEST_FIRST {
            body += &format!(
                " {} {}",
                to_string(level),
                metres(graded.length_at[level as usize])
            );
        }
        body += "\n";

        for (i, vertex) in line.vertices.iter().enumerate() {
            let grade = &graded.vertices[i];
            let depth = &cover[i];
            body += &format!(
                "  {:<10} {:<28} {}",
                vertex.id,
                to_string(vertex.evidence.method),
                to_string(grade.classification.level)
            );
            if let Some(cover) = depth.cover {
                body += &format!("  cover {}", metres(cover));
            }
            body += "\n";
            for reason in &grade.classification.reasons {
                body += &format!("             {reason}\n");
            }

            let place = format!("{} / {}", line.id, vertex.id);
            if !grade.over_claim.is_empty() {
                findings.push(format!("{place}: {}", grade.over_claim));
            }
            // Below the minimum only where both a cover and a minimum are.
            if depth.below_minimum
                && let (Some(cover), Some(minimum)) = (depth.cover, minimum_cover)
            {
                findings.push(format!(
                    "{place}: cover {} m is below the minimum {} m",
                    metres(cover),
                    metres(minimum)
                ));
            }
            if depth.cover.is_some() && !depth.note.is_empty() {
                findings.push(format!("{place}: {}", depth.note));
            }
        }
        for segment in &graded.segments {
            body += &format!(
                "  {} -> {}  {} m  {}",
                line.vertices[segment.from].id,
                line.vertices[segment.from + 1].id,
                metres(segment.length),
                to_string(segment.level)
            );
            if !segment.limited_by.is_empty() {
                body += &format!("  ({})", segment.limited_by);
            }
            body += "\n";
        }

        // A delivery schema claims one level for a whole asset, so the claim
        // is tested along it too: a segment between two vertices that both
        // claim a level is claimed at the weaker of the two.
        let mut over_claimed_length = 0.0_f64;
        let mut over_claimed: Vec<String> = Vec::new();
        for segment in &graded.segments {
            let (Some(from), Some(to)) = (
                line.vertices[segment.from].claimed,
                line.vertices[segment.from + 1].claimed,
            ) else {
                continue;
            };
            let claimed = from.min(to);
            if claimed <= segment.level {
                continue;
            }
            over_claimed_length += segment.length;
            if over_claimed.len() < 3 {
                over_claimed.push(format!(
                    "{} -> {} claimed {}, grades {}",
                    line.vertices[segment.from].id,
                    line.vertices[segment.from + 1].id,
                    to_string(claimed),
                    to_string(segment.level)
                ));
            }
        }
        if !over_claimed.is_empty() {
            findings.push(format!(
                "{}: {} m is claimed better than it grades: {}",
                line.id,
                metres(over_claimed_length),
                over_claimed.join("; ")
            ));
        }

        let missing = missing_attributes(&line.attributes);
        if !missing.is_empty() {
            findings.push(format!("{}: not recorded: {}", line.id, missing.join(", ")));
        }

        let lengths = by_type
            .entry(to_string(line.attributes.r#type))
            .or_insert([0.0; 4]);
        for level in 0..4 {
            lengths[level] += graded.length_at[level];
            total[level] += graded.length_at[level];
        }
    }

    let mut report = format!(
        "AS 5488 subsurface utility investigation: {} lines, {vertex_count} vertices\n",
        lines.len()
    );
    report += &settings_text(settings);
    if let Some(minimum) = minimum_cover {
        report += &format!("Minimum cover: {} m\n", metres(minimum));
    }
    report += &body;

    report += "\nLength by type and quality level (m)\n";
    report += &format!(
        "  {:<20}{:>12}{:>12}{:>12}{:>12}{:>12}\n",
        "type", "QL-A", "QL-B", "QL-C", "QL-D", "total"
    );
    for (name, lengths) in &by_type {
        report += &format!(
            "  {:<20}{}{:>12}\n",
            name,
            lengths_text(lengths),
            metres(lengths[0] + lengths[1] + lengths[2] + lengths[3])
        );
    }
    report += &format!(
        "  {:<20}{}{:>12}\n",
        "all",
        lengths_text(&total),
        metres(total[0] + total[1] + total[2] + total[3])
    );

    report += &format!("\nFindings: {}\n", findings.len());
    for finding in &findings {
        report += &format!("  {finding}\n");
    }
    if findings.is_empty() {
        report += "  none\n";
    }
    Ok(report)
}

/// The counts by status, then every segment that is not clear, worst first,
/// and where to pothole next.
pub fn render_clearance_report(
    design: &DesignAlignment,
    results: &[ClearanceResult],
    requirement: &ClearanceRequirement,
) -> String {
    let mut order: Vec<&ClearanceResult> = Vec::new();
    let mut services: Vec<&str> = Vec::new();
    let mut counts = [0_usize; 4];
    for result in results {
        counts[result.status as usize] += 1;
        if !services.contains(&result.utility_id.as_str()) {
            services.push(result.utility_id.as_str());
        }
        if result.status != ClearanceStatus::Clear {
            order.push(result);
        }
    }
    // Stable, as the C++'s std::stable_sort is. The gaps are differences of
    // finite distances and sizes, never a NaN or a negative zero, so the
    // total order is the C++'s `<`.
    order.sort_by(|a, b| {
        a.status
            .cmp(&b.status)
            .then_with(|| a.horizontal_gap.total_cmp(&b.horizontal_gap))
    });

    let name = if design.id.is_empty() {
        "the design"
    } else {
        design.id.as_str()
    };
    let mut report = format!(
        "Clearance of {name} (half-width {} m) from {} services, {} segments\n",
        metres(design.half_width),
        services.len(),
        results.len()
    );
    report += &format!(
        "Required: {} m horizontal, {} m vertical; QL-C/QL-D margin {} m\n",
        metres(requirement.horizontal),
        metres(requirement.vertical),
        metres(requirement.unverified_margin)
    );
    report += &format!(
        "Segments: {} conflict, {} unconfirmed, {} within tolerance, {} clear\n",
        counts[0], counts[1], counts[2], counts[3]
    );
    // The worst clash of each service, for a delivery schema's Clash attribute.
    let mut clashes: Vec<(&str, Clash)> = Vec::new();
    for result in results {
        let clash = clash_of(result);
        if let Some(index) = clashes
            .iter()
            .position(|(service, _)| *service == result.utility_id)
        {
            clashes[index].1 = clashes[index].1.min(clash);
        } else {
            clashes.push((result.utility_id.as_str(), clash));
        }
    }
    report += "Suggested Clash attribute:";
    for (index, (service, clash)) in clashes.iter().enumerate() {
        report += &format!(
            " {service} {}{}",
            to_string(*clash),
            if index + 1 == clashes.len() { "" } else { "," }
        );
    }
    report += "\n";
    if order.is_empty() {
        return report + "Every segment is clear.\n";
    }

    report += "\n";
    report += &format!(
        "  {:<26}{:<6}{:<18}{:>10}{:>8}{:>10}\n",
        "service segment", "QL", "status", "plan gap", "tol H", "vert gap"
    );
    let mut to_locate: Vec<String> = Vec::new();
    for result in &order {
        report += &format!(
            "  {:<26}{:<6}{:<18}{:>10}{:>8}{:>10}",
            format!(
                "{} {} -> {}",
                result.utility_id, result.from_id, result.to_id
            ),
            to_string(result.level),
            to_string(result.status),
            metres(result.horizontal_gap),
            result
                .horizontal_tolerance
                .map_or_else(|| String::from("-"), metres),
            result
                .vertical_gap
                .map_or_else(|| String::from("-"), metres)
        );
        if !result.note.is_empty() {
            report += &format!("  {}", result.note);
        }
        report += "\n";
        if result.status != ClearanceStatus::Conflict {
            to_locate.push(format!(
                "{} near N {} E {}",
                result.utility_id,
                fixed(result.nearest.northing, 3),
                fixed(result.nearest.easting, 3)
            ));
        }
    }
    if !to_locate.is_empty() {
        report += "\nPothole (QL-A) before relying on the design:\n";
        for entry in &to_locate {
            report += &format!("  {entry}\n");
        }
    }
    report
}

/// Each comparison, the pass rates and the RMS, and a conclusion.
pub fn render_verification_report(
    report: &VerificationReport,
    settings: &GradingSettings,
) -> String {
    let mut text = format!(
        "Verification of detections by exposure: {} comparisons\n",
        report.results.len()
    );
    text += &settings_text(settings);
    text += "\n";
    text += &format!(
        "  {:<10}{:<12}{:<12}{:>10}{:>6}{:>10}{:>6}\n",
        "line", "detected", "exposed", "dH (m)", "", "dV (m)", ""
    );
    for result in &report.results {
        text += &format!(
            "  {:<10}{:<12}{:<12}{:>10}{:>6}{:>10}{:>6}\n",
            result.line_id,
            result.detected_id,
            result.exposed_id,
            metres(result.horizontal_deviation),
            if result.horizontal_within {
                "ok"
            } else {
                "FAIL"
            },
            result
                .vertical_deviation
                .map_or_else(|| String::from("-"), metres),
            match result.vertical_within {
                Some(true) => "ok",
                Some(false) => "FAIL",
                None => "",
            }
        );
    }
    let n = report.results.len();
    text += &format!(
        "\nHorizontal: {} of {n} within QL-B",
        report.horizontal_passed
    );
    if n > 0 {
        text += &format!(
            ", max {} m, RMS {} m",
            metres(report.maximum_horizontal),
            metres(report.rms_horizontal)
        );
    }
    text += &format!(
        "\nVertical: {} of {} within QL-B",
        report.vertical_passed, report.vertical_checked
    );
    if let (Some(maximum), Some(rms)) = (report.maximum_vertical, report.rms_vertical) {
        text += &format!(", max {} m, RMS {} m", metres(maximum), metres(rms));
    }
    text += "\n";

    if !report.problems.is_empty() {
        text += &format!("\nNot used: {}\n", report.problems.len());
        for problem in &report.problems {
            text += &format!("  {problem}\n");
        }
    }

    let all_passed =
        report.horizontal_passed == n && report.vertical_passed == report.vertical_checked;
    if n == 0 {
        text += "\nConclusion: nothing was verified; the QL-B grades rest on the locator's \
                 stated uncertainty alone.\n";
    } else if all_passed {
        text += "\nConclusion: every checked detection is within the QL-B tolerance.\n";
    } else {
        text += "\nConclusion: some detections are outside the QL-B tolerance; review the QL-B \
                 grades of the lines they belong to.\n";
    }
    text
}
