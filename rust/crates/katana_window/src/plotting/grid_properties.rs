//! A plan viewport's coordinate grid in the sheet editor's properties (docs/plotting.md, "The
//! coordinate grid and the live key plan"): its style and its spacing. A thin front end: what it
//! collects goes to cad's set_plan_grid, which an agent calls directly. The port of
//! plotting/grid_properties.
//!
//! The C++ added two rows to a form and called back with the style and the spacing. Here the rows
//! are drawn each frame into the editor's form (an egui Grid of two columns) and the change, when
//! there is one, is answered: the editor makes it, as one undoable step, and the rows are drawn
//! from the edited viewport at the next frame.
//!
//! WHAT DIFFERS FROM THE C++. The spacing box shows 0 as "0.000 m", not "Auto" (the foundation's
//! number box has no special text for its minimum); its tooltip still says what Auto comes to.

use egui::Ui;
use katana_cad::plotting::sheet_set::{GridStyle, Viewport};
use katana_core::text::format_general;

use crate::drive::controls::{self, Choice, SpinRange};

/// The styles, in the order of the list: each item's wording, and the style it stands for. The
/// wording can change without touching what is saved.
const STYLES: [(&str, GridStyle); 4] = [
    ("None", GridStyle::None),
    ("Ticks at the edges", GridStyle::Ticks),
    ("Crosses", GridStyle::Crosses),
    ("Lines", GridStyle::Lines),
];

/// The grid's two rows, and the spacing being typed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GridRows {
    /// The viewport the spacing was read from.
    viewport: String,
    /// The spacing as the box holds it: the viewport's, until one is typed.
    interval: f64,
    /// A spacing typed and not yet handed over: one step when the typing is done, not one per key
    /// (QAbstractSpinBox::setKeyboardTracking(false)).
    typing: bool,
}

impl GridRows {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the grid's two rows to the form `ui` is laying out, filled from `viewport`: the style
    /// (a list, object name "sheetGridStyle") and the spacing in metres, 0 for automatic (a number
    /// box, "sheetGridInterval"). `automatic_interval_m` is what automatic comes to at the
    /// viewport's scale now, for its tooltip. A change to either answers both.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        viewport: &Viewport,
        automatic_interval_m: f64,
    ) -> Option<(GridStyle, f64)> {
        if !self.typing || self.viewport != viewport.id {
            self.viewport.clone_from(&viewport.id);
            self.interval = viewport.grid_interval;
            self.typing = false;
        }
        let mut chosen = None;

        ui.label("Grid");
        let items: Vec<String> = STYLES.iter().map(|(name, _)| String::from(*name)).collect();
        let mut style = Choice::at(
            STYLES
                .iter()
                .position(|(_, value)| *value == viewport.grid_style)
                .unwrap_or(0),
        );
        let picked = controls::choice(ui, "sheetGridStyle", &items, &mut style).on_hover_text(
            "The ground's eastings and northings, labelled where they meet the view's edges",
        );
        let style_now = style
            .index
            .and_then(|at| STYLES.get(at))
            .map_or(GridStyle::None, |(_, value)| *value);
        if picked.changed() {
            chosen = Some((style_now, self.interval));
        }
        ui.end_row();

        ui.label("Grid interval");
        let spacing = ui
            .add_enabled_ui(viewport.grid_style != GridStyle::None, |ui| {
                controls::spin(
                    ui,
                    "sheetGridInterval",
                    &mut self.interval,
                    SpinRange::new(0.0, 1e6, 1.0, 3).with_suffix(" m"),
                )
            })
            .inner
            .on_hover_text(if automatic_interval_m > 0.0 {
                format!(
                    "The spacing in metres; Auto is about 50 mm apart on the paper - {} m now",
                    format_general(automatic_interval_m, 6)
                )
            } else {
                String::from("The spacing in metres; Auto is about 50 mm apart on the paper")
            });
        if spacing.changed() {
            self.typing = true;
        }
        // Handed over when the typing is done: the box has let go of the keyboard and the pointer.
        if self.typing && !spacing.has_focus() && !spacing.dragged() {
            self.typing = false;
            if self.interval != viewport.grid_interval {
                chosen = Some((style_now, self.interval));
            }
        }
        ui.end_row();
        chosen
    }
}
