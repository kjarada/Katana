//! The Legend viewport's rows in the sheet editor's properties (docs/plotting.md, "The smart
//! legend"): what it lists, and what that comes to now. The port of plotting/legend_properties.
//!
//! A thin front end: the choice is cad's set_legend_scope, one undoable step, and the summary is
//! the legend the painter prints (gather_legend).
//!
//! The C++ committed the choice itself, on the document it was handed, in a queued call (the edit
//! rebuilds the properties, and so deleted the box inside its own signal). Here the rows are drawn
//! while the sheet's source borrows the drawing, so the choice is answered to the editor, which
//! makes it with set_legend_scope once the frame's drawing has let the drawing go and says a
//! refusal where it says its others.

use egui::Ui;
use katana_cad::plotting::legend::Legend;
use katana_cad::plotting::sheet_set::{LegendScope, SheetSet, Viewport};

use crate::drive::controls::{self, Choice};
use crate::plotting::legend_painter::gather_legend;
use crate::sheet_painter::SheetSource;

fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// "12 entries from this sheet's 2 plans"; "3 entries from 5 plans of the set - this sheet has no
/// plan"; "1 entry from the whole drawing". Says where a fall-back led, since a legend listing
/// the whole set when it was asked for this sheet is otherwise a puzzle.
pub fn legend_summary(legend: &Legend, asked: LegendScope) -> String {
    let entries = counted(legend.entries.len(), "entry", "entries");
    let plans = counted(legend.windows, "plan", "plans");
    match legend.scope {
        LegendScope::ThisSheet => format!("{entries} from this sheet's {plans}"),
        LegendScope::WholeSet => format!(
            "{entries} from {plans} of the set{}",
            if asked == LegendScope::ThisSheet {
                " - this sheet has no plan"
            } else {
                ""
            }
        ),
        LegendScope::WholeDrawing => format!(
            "{entries} from the whole drawing{}",
            if asked == LegendScope::WholeDrawing {
                ""
            } else {
                " - no sheet has a plan"
            }
        ),
    }
}

/// The scopes, in the order of the list.
const SCOPES: [(&str, LegendScope); 3] = [
    ("What this sheet's plans show", LegendScope::ThisSheet),
    ("What every sheet's plans show", LegendScope::WholeSet),
    ("Everything in the drawing", LegendScope::WholeDrawing),
];

/// Adds to the form `ui` is laying out a choice of scope (object name "sheetLegendScope") and a
/// line saying what the legend lists (object name "sheetLegendSummary"). A change of scope is
/// answered, for the editor to commit at once.
pub fn add_legend_properties(
    ui: &mut Ui,
    set: &SheetSet,
    sheet_index: usize,
    legend: &Viewport,
    source: &SheetSource<'_>,
) -> Option<LegendScope> {
    ui.label("Lists");
    let items: Vec<String> = SCOPES.iter().map(|(name, _)| String::from(*name)).collect();
    let mut scope = Choice {
        index: SCOPES
            .iter()
            .position(|(_, value)| *value == legend.legend_scope),
        ..Choice::default()
    };
    let picked = controls::choice(ui, "sheetLegendScope", &items, &mut scope).on_hover_text(
        "A sheet with no plan lists what the set's plans show, and a set with none the whole \
         drawing",
    );
    let chosen = if picked.changed() {
        scope
            .index
            .and_then(|at| SCOPES.get(at))
            .map(|(_, value)| *value)
    } else {
        None
    };
    ui.end_row();

    ui.label("");
    let summary = match gather_legend(set, sheet_index, legend, source) {
        Ok(gathered) => legend_summary(&gathered, legend.legend_scope),
        Err(error) => error.describe(),
    };
    controls::label_as(
        ui,
        "sheetLegendSummary",
        &summary,
        egui::Label::new(summary.as_str()).wrap(),
    );
    ui.end_row();
    chosen
}
