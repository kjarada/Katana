//! How a dialog lays out what it holds inside the dialog frame (shell::dialog): sections under a
//! small heading, two-column forms whose labels line up and whose fields are one width, a footer
//! whose buttons sit at the right with the primary one last, and the reply line above them. The
//! design system's numbers (the owner's "tidy, not rethink" of 2026-11-16) are here, once, so the
//! managers, the plot and sheet dialogs, the start screen and the update dialogs are spaced alike;
//! the colours are the theme's tokens (theme::palette), never a literal.
//!
//! WHY FIXED WIDTHS. An egui dialog is sized by what it holds (its Area is as large as its content
//! was last frame), so a field told to fill "the available width" fills the screen on the first
//! frame and then holds the dialog that wide. A form therefore gives its fields one width of its
//! own (FIELD_WIDTH, or what the dialog asks for) through the style's `text_edit_width` and
//! `combo_width`, which every named line and choice (drive::controls) reads; the footer's buttons
//! are put at the right with `egui::Sides`, which measures them small in a sizing pass and only
//! then pushes them across the width the body already took. *Rejected:* a right-to-left layout
//! for the footer, which in the first (sizing) pass takes egui's default area width (600 points)
//! and keeps every dialog at least that wide from then on.

use egui::{Align, Layout, Response, RichText, Sides, Ui};

use crate::drive::controls;
use crate::theme;

/// The gap between groups (a section and the next, a table and its buttons).
pub const GAP: f32 = 8.0;

/// The gap inside a group (a label and its field, the rows of a form).
pub const INNER: f32 = 4.0;

/// The gap above a section's heading, so that sections read as blocks without boxes round them.
pub const SECTION_GAP: f32 = 12.0;

/// The width of a form's label column: a label of about sixteen characters at 13 px fits, and a
/// longer one widens the column (egui's Grid takes the widest cell).
pub const LABEL_WIDTH: f32 = 110.0;

/// The width of a form's fields: one line field, choice or path, with room for a Browse button
/// beside it inside a 480-point dialog.
pub const FIELD_WIDTH: f32 = 260.0;

/// The least a footer button is wide, so that "OK", "Close" and "Apply" are one size.
pub const BUTTON_MIN_WIDTH: f32 = 72.0;

/// The rows a table shows before it scrolls: enough to read a list down without the table
/// pushing the dialog off the screen.
pub const TABLE_ROWS: f32 = 12.0;

/// A section: `title` as a small strong heading over what `add` draws, a SECTION_GAP above it
/// unless it is the first thing in its `ui`.
pub fn section<R>(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    if ui.cursor().min.y > ui.max_rect().min.y + 0.5 {
        ui.add_space(SECTION_GAP - ui.spacing().item_spacing.y);
    }
    ui.label(RichText::new(title).strong());
    ui.add_space(INNER - ui.spacing().item_spacing.y);
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = INNER;
        add(ui)
    })
    .inner
}

/// A two-column form named `salt`: labels on the left, left-aligned and vertically centred on
/// their row; fields on the right, each `field_width` wide (lines and choices). `add` draws the
/// rows with `row` (or by hand, ending each with `ui.end_row()`).
pub fn form_with<R>(
    ui: &mut Ui,
    salt: &str,
    field_width: f32,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    ui.scope(|ui| {
        ui.spacing_mut().text_edit_width = field_width;
        ui.spacing_mut().combo_width = field_width;
        egui::Grid::new(salt)
            .num_columns(2)
            .spacing([GAP, INNER])
            .min_col_width(LABEL_WIDTH)
            .show(ui, add)
            .inner
    })
    .inner
}

/// Sets `ui` up for an EDITABLE choice `width` wide in all: drive::controls draws one as a field
/// and a list button beside it, the field `text_edit_width` and the button `combo_width` wide, so
/// in a form (which makes `combo_width` the field width) the button would be as wide as a field.
pub fn editable_choice_width(ui: &mut Ui, width: f32) {
    ui.spacing_mut().combo_width = EDITABLE_CHOICE_BUTTON;
    ui.spacing_mut().text_edit_width =
        (width - EDITABLE_CHOICE_BUTTON - ui.spacing().item_spacing.x).max(0.0);
}

/// The width of an editable choice's list button: its arrow and the button's padding.
pub const EDITABLE_CHOICE_BUTTON: f32 = 24.0;

/// A form of FIELD_WIDTH fields (`form_with`).
pub fn form<R>(ui: &mut Ui, salt: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    form_with(ui, salt, FIELD_WIDTH, add)
}

/// A form's label: in the secondary text colour, so the values are what the eye reads first.
pub fn form_label(ui: &mut Ui, text: &str) -> Response {
    ui.label(RichText::new(text).color(theme::text_muted().to_egui()))
}

/// A label `width` wide, in the secondary text colour, for a row of a form drawn as rows of its own
/// (a label, a field and its buttons) rather than as a grid: the fields after it line up.
pub fn fixed_label(ui: &mut Ui, text: &str, width: f32) {
    let height = ui.spacing().interact_size.y;
    ui.allocate_ui_with_layout(
        egui::vec2(width, height),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_min_width(width);
            form_label(ui, text);
        },
    );
}

/// What `add` draws, moved right by `width` and the row's gap: a check box under the fields of a
/// form drawn with `fixed_label`, lined up with them.
pub fn indented<R>(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.add_space(width);
        add(ui)
    })
    .inner
}

/// One row of a form: `label`, then what `add` draws in the field column, then the row's end.
pub fn row<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    form_label(ui, label);
    let answer = ui
        .with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = INNER;
            add(ui)
        })
        .inner;
    ui.end_row();
    answer
}

/// Fields side by side in one row of a form or a section (a field and its Browse button): the
/// gap between them INNER.
pub fn inline<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = INNER;
        add(ui)
    })
    .inner
}

/// The dialog's footer row: `left` at the left (a check box, a reply), and `right` at the right,
/// drawn from the right edge inwards - so the PRIMARY button is drawn first and so sits last, at
/// the far right. A separator above parts it from the body.
pub fn footer<L, R>(
    ui: &mut Ui,
    left: impl FnOnce(&mut Ui) -> L,
    right: impl FnOnce(&mut Ui) -> R,
) -> (L, R) {
    ui.add_space(INNER);
    ui.separator();
    ui.add_space(INNER);
    ui.scope(|ui| {
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

/// The primary button's look, showing `shown`: in the accent while `enabled`, else as any button
/// is disabled (an accent fill on a button that does nothing would invite the click).
pub fn primary_button(shown: &str, enabled: bool) -> egui::Button<'static> {
    let words = if enabled {
        RichText::new(shown).color(theme::accent_text().to_egui())
    } else {
        RichText::new(shown)
    };
    let drawn = egui::Button::new(words).min_size(egui::vec2(BUTTON_MIN_WIDTH, 0.0));
    if enabled {
        drawn.fill(theme::accent().to_egui())
    } else {
        drawn
    }
}

/// A heading over a part of a dialog that is not drawn as a `section` (its controls are drawn
/// round it by the dialog itself): a SECTION_GAP above unless it is the first thing, then the
/// title in strong text.
pub fn heading(ui: &mut Ui, title: &str) {
    if ui.cursor().min.y > ui.max_rect().min.y + 0.5 {
        ui.add_space(SECTION_GAP - ui.spacing().item_spacing.y);
    }
    ui.label(RichText::new(title).strong());
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
    (row % 2 == 0).then_some(style.visuals.faint_bg_color)
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
