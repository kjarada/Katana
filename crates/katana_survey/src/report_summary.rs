//! A reduction's outcome in a line: the words the import wizard and the Survey
//! Jobs dialog show after a preview, an import and a re-adjustment, and the
//! count SURVEY IMPORT's reply gives (the app layer's survey verbs).
//!
//! Here, below both, because the window and the line report one outcome: the
//! wizard's "3186 observation(s) rejected" and the reply's rejected=3186 are
//! one count, taken one way. They began in the window, where the line could
//! not reach them.

use crate::reduction::format_number;
use crate::reduction_report::ReductionReport;
use crate::reduction_settings::{
    AdjustmentMethod, NetworkDimension, ReductionSettings, TraverseRule,
};

/// "network least squares (horizontal): variance factor 1.023, global test
/// passed" / "radiation: nothing adjusted" / "traverse, Bowditch: no adjustment
/// was run": each adjustment the report holds, "; " between.
pub fn adjustment_summary(report: &ReductionReport) -> String {
    if report.adjustments.is_empty() {
        return if report.settings.method == AdjustmentMethod::None {
            String::from("radiation: nothing adjusted")
        } else {
            format!("{}: no adjustment was run", method_text(&report.settings))
        };
    }
    let mut text = String::new();
    for adjustment in &report.adjustments {
        let mut line = format!("{}: ", adjustment.method);
        match adjustment.variance_factor {
            // std::format's {:.3f}: format_number writes its digits, and its
            // "nan" and "inf" where Rust's {:.3} would write "NaN".
            Some(factor) => line.push_str(&format!("variance factor {}", format_number(factor, 3))),
            None => line.push_str("no redundancy, so no variance factor"),
        }
        if let Some(test) = &adjustment.global_test {
            line.push_str(if test.passed {
                ", global test passed"
            } else {
                ", global test FAILED"
            });
        }
        if !adjustment.flagged_outliers.is_empty() {
            line.push_str(&format!(
                ", {} flagged outlier(s)",
                adjustment.flagged_outliers.len()
            ));
        }
        if text.is_empty() {
            text = line;
        } else {
            text.push_str("; ");
            text.push_str(&line);
        }
    }
    text
}

/// How many observations the report says were rejected: its rows marked
/// rejected, and the outliers each adjustment rejected. An outlier an
/// adjustment rejects marks its rows too, so it is counted twice: one
/// distance rejected reads as 2 (docs/survey.md, "Not done" of "The
/// reduction settings of SURVEY IMPORT").
pub fn rejected_observations(report: &ReductionReport) -> usize {
    let rows = report
        .observations
        .iter()
        .filter(|observation| observation.rejected)
        .count();
    let outliers: usize = report
        .adjustments
        .iter()
        .map(|adjustment| adjustment.rejected_outliers.len())
        .sum();
    rows + outliers
}

/// The settings' adjustment as a person reads it: "radiation", "traverse,
/// Bowditch", "network, horizontal and levels".
pub fn method_text(settings: &ReductionSettings) -> String {
    String::from(match settings.method {
        AdjustmentMethod::None => "radiation",
        AdjustmentMethod::Traverse => match settings.traverse_rule {
            TraverseRule::Bowditch => "traverse, Bowditch",
            TraverseRule::Transit => "traverse, Transit",
            TraverseRule::LeastSquares => "traverse, least squares",
        },
        AdjustmentMethod::Network => match settings.network_dimension {
            NetworkDimension::Horizontal => "network, horizontal",
            NetworkDimension::Levels => "network, levels",
            NetworkDimension::HorizontalAndLevels => "network, horizontal and levels",
        },
    })
}
