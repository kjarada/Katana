//! How a dialog lays out what is not its form: a footer whose buttons sit at the right with the
//! primary one last, the reply line above them, and the tables' look. The design system's numbers
//! (the owner's "tidy, not rethink" of 2026-11-16) are here, once, so the managers, the plot and
//! sheet dialogs, the start screen and the update dialogs are spaced alike; the colours are the
//! theme's tokens (theme::palette), never a literal.
//!
//! ONE HOME FOR EACH PART. The spacing scale is theme::style's; the dialog frame's padding, footer
//! button width, label colour, footer tag, section heading and FORM - the label column every
//! labelled row of a body shares, its fields filling the rest (`form`, `labelled`, `fill_fields`,
//! `fixed_fields`, `beside_a_button`) - are shell::dialog's; a primary button's look is
//! drive::controls::primary_look. This module adds what those do not have - the footer row drawn
//! with egui::Sides and the tables' look - and names the shared parts again only so its callers
//! read one module. It held a second form once, a grid of fixed-width fields with a label column
//! of its own width (110), written beside shell::dialog's by another hand; it was folded into
//! shell::dialog's, so that every field of every dialog starts at its body's one label edge
//! (docs/desktop.md, "The look of 2026-11-16").
//!
//! WHY THE FOOTER IS DRAWN WITH egui::Sides. An egui dialog is sized by what it holds (its Area is
//! as large as its content was last frame), so the footer's buttons are put at the right with
//! `egui::Sides`, which measures them small in a sizing pass and only then pushes them across the
//! width the body already took. *Rejected:* a right-to-left layout for the footer, which in the
//! first (sizing) pass takes egui's default area width (600 points) and keeps every dialog at
//! least that wide from then on.

use egui::{Response, Sides, Ui};

use crate::drive::controls;
use crate::shell::dialog;
use crate::theme;

pub use crate::shell::dialog::{SECTION_GAP, form_label};

/// The gap between groups (a section and the next, a table and its buttons).
pub const GAP: f32 = theme::style::GROUP_GAP;

/// The gap inside a group (a label and its field, the rows of a form).
pub const INNER: f32 = theme::style::GAP;

/// The least a footer button is wide, so that "OK", "Close" and "Apply" are one size.
pub const BUTTON_MIN_WIDTH: f32 = dialog::FOOTER_BUTTON_WIDTH;

/// The rows a table shows before it scrolls: enough to read a list down without the table
/// pushing the dialog off the screen.
pub const TABLE_ROWS: f32 = 12.0;

/// The dialog's footer row: `left` at the left (a check box, a reply), and `right` at the right,
/// drawn from the right edge inwards - so the PRIMARY button is drawn first and so sits last, at
/// the far right. In the dialog frame's footer, the frame's rule is above it already; drawn in a
/// body, a separator of its own parts it from what is above. Either way the row is a footer's
/// (shell::dialog::footer_ui), so its buttons share the footer's least width.
pub fn footer<L, R>(
    ui: &mut Ui,
    left: impl FnOnce(&mut Ui) -> L,
    right: impl FnOnce(&mut Ui) -> R,
) -> (L, R) {
    if !dialog::in_footer(ui) {
        ui.add_space(INNER);
        ui.separator();
        ui.add_space(INNER);
    }
    dialog::footer_ui(ui, |ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        Sides::new().shrink_left().show(ui, left, |ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            right(ui)
        })
    })
    .inner
}

/// The footer's buttons alone, at the right (`footer` with nothing at the left).
pub fn footer_buttons<R>(ui: &mut Ui, right: impl FnOnce(&mut Ui) -> R) -> R {
    footer(ui, |_| {}, right).1
}

/// A footer button named `name`: at least BUTTON_MIN_WIDTH wide.
pub fn button(ui: &mut Ui, name: &str, text: &str) -> Response {
    controls::button_as(
        ui,
        name,
        text,
        egui::Button::new(text).min_size(egui::vec2(BUTTON_MIN_WIDTH, 0.0)),
    )
}

/// The dialog's primary button named `name` (OK, Start, Plot, Apply): filled in the accent, its
/// text in the accent's text colour - the one accent the design system allows a dialog.
pub fn primary(ui: &mut Ui, name: &str, text: &str) -> Response {
    let drawn = primary_button(text, ui.is_enabled());
    controls::button_as(ui, name, text, drawn)
}

/// The primary button's look (drive::controls::primary_look), showing `shown`, BUTTON_MIN_WIDTH
/// wide at least wherever it is drawn.
pub fn primary_button(shown: &str, enabled: bool) -> egui::Button<'static> {
    controls::primary_look(shown, enabled).min_size(egui::vec2(BUTTON_MIN_WIDTH, 0.0))
}

/// The colour of a reply line: the error token for a refusal, the secondary text otherwise.
pub fn reply_colour(is_error: bool) -> egui::Color32 {
    if is_error {
        theme::error().to_egui()
    } else {
        theme::text_muted().to_egui()
    }
}

/// The height of `rows` rows of a compact table or list at the style's text size.
pub fn rows_height(ui: &Ui, rows: f32) -> f32 {
    let row = ui
        .spacing()
        .interact_size
        .y
        .max(ui.text_style_height(&egui::TextStyle::Body));
    rows * (row + ui.spacing().item_spacing.y)
}

// ---- tables ----------------------------------------------------------------------------------

/// The gap between a table's columns: wide enough that a column of numbers is not read as part of
/// the next one.
pub const TABLE_COLUMN_GAP: f32 = 16.0;

/// The gap between a table's rows: compact (the design system's 4 inside a group).
pub const TABLE_ROW_GAP: f32 = 4.0;

/// A table row's background (for egui's Grid::with_row_color, whose row 0 is the header): the
/// header in the raised token, a chosen row in the accent at SELECTED_ROW_OPACITY (its text stays
/// the text colour, read on it), every other row of the rest in the faint stripe; None for the
/// rest.
pub fn table_row_colour(row: usize, chosen: &[bool], style: &egui::Style) -> Option<egui::Color32> {
    if row == 0 {
        return Some(theme::raised().to_egui());
    }
    if chosen.get(row - 1).copied().unwrap_or(false) {
        return Some(selected_row_colour());
    }
    row.is_multiple_of(2)
        .then_some(style.visuals.faint_bg_color)
}

/// A chosen row's background: the accent at SELECTED_ROW_OPACITY.
pub fn selected_row_colour() -> egui::Color32 {
    theme::accent()
        .to_egui()
        .gamma_multiply(SELECTED_ROW_OPACITY)
}

/// How much of the accent a chosen row is lit with: enough to be seen at a glance across a wide
/// table, little enough that the row's text keeps its contrast.
pub const SELECTED_ROW_OPACITY: f32 = 0.35;
