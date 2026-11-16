//! How a dialog lays out what is not its form or its footer: the tables' look and a reply kept in a
//! footer as muted text. The design system's numbers
//! (the owner's "tidy, not rethink" of 2026-11-16) are here, once, so the managers, the plot and
//! sheet dialogs, the start screen and the update dialogs are spaced alike; the colours are the
//! theme's tokens (theme::palette), never a literal.
//!
//! ONE HOME FOR EACH PART. The spacing scale is theme::style's; the dialog frame's padding, footer
//! button width, label colour, footer tag, section heading and FORM - the label column every
//! labelled row of a body shares, its fields filling the rest (`form`, `labelled`, `fill_fields`,
//! `fixed_fields`, `beside_a_button`) - are shell::dialog's; a primary button's look is
//! drive::controls::primary_look. This module adds what those do not have - the tables' look and
//! the reply line - and names the shared parts again only so its callers read one module. It held a second form once, a grid of fixed-width fields with a label column
//! of its own width (110), written beside shell::dialog's by another hand; it was folded into
//! shell::dialog's, so that every field of every dialog starts at its body's one label edge
//! (docs/desktop.md, "The look of 2026-11-16").
//!
//! ONE FOOTER. A dialog's footer is shell::dialog::button_row with drive::controls::{button,
//! primary}: what reads the drawing and a status at the left, Close, then the primary action last
//! at the right. This module held a second footer once (`footer`, `footer_buttons`, `button`,
//! `primary`, drawn with egui::Sides), written in the look's first round beside button_row; every
//! dialog moved onto button_row in its third round (2026-11-16) and the second was deleted.

use egui::{Response, RichText, Ui};

use crate::drive::controls;
use crate::drive::mailbox::{Act, fill_disabled, no_button, no_field};
use crate::drive::registry::{ControlKind, ControlRecord, settle, take};
use crate::shell::dialog;
use crate::theme;

/// A one-line field or a spin box in a form's row made as wide as the row, as a choice beside it
/// is: egui sizes a number box to its figures, so a form of choices and numbers had two right
/// edges (the Leaders' Look group: 400 against 75; the second look audit's N13). Draws `add` in a
/// row whose one widget is justified across it.
pub fn full_width<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.with_layout(
        egui::Layout::left_to_right(egui::Align::Center).with_main_justify(true),
        add,
    )
    .inner
}

pub use crate::shell::dialog::{SECTION_GAP, form_label};

/// The gap between groups (a section and the next, a table and its buttons).
pub const GAP: f32 = theme::style::GROUP_GAP;

/// The gap inside a group (a label and its field, the rows of a form).
pub const INNER: f32 = theme::style::GAP;

/// The rows a table shows before it scrolls: enough to read a list down without the table
/// pushing the dialog off the screen.
pub const TABLE_ROWS: f32 = 12.0;

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

/// What an empty table says under its header when its owner says nothing more fitting: a header
/// strip over nothing read as a table that failed to draw (the look audit's T9).
pub const EMPTY_TABLE: &str = "Nothing to list";

/// A table's header row, drawn into an egui Grid as its row 0 (table_row_colour paints it in the
/// raised token): each title strong and in the secondary text colour. With `fill`, the last title's
/// cell reaches the right edge of the room the grid was lent, so the header - and every row's
/// stripe, which egui paints as wide as the grid's columns - spans the table's place rather than
/// ending where its titles do: three tables of one column of a dialog were three strips of three
/// widths (the Road Template Manager's 325, 513 and 275).
pub fn table_header<S: AsRef<str>>(ui: &mut Ui, headers: &[S], fill: bool) {
    let last = headers.len().saturating_sub(1);
    for (at, header) in headers.iter().enumerate() {
        let title = RichText::new(header.as_ref())
            .strong()
            .color(theme::text_muted().to_egui());
        if fill && at == last {
            // egui's Grid lends its last column the room to the right edge of what holds it (from
            // its second frame), so a cell that takes it all makes the column that wide.
            let room = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
            ui.allocate_ui_with_layout(
                room,
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(room.x);
                    ui.label(title);
                },
            );
        } else {
            ui.label(title);
        }
    }
}

/// An empty table's line under its header: `text` in the secondary text colour, at its own width
/// from the table's left edge, so the table says it has nothing rather than showing a header over
/// a void. Not sized to the row: a table in a column of no width of its own would take the
/// column's room for its note, and push what is beside it away (the Leaders' Look group).
pub fn empty_table(ui: &mut Ui, text: &str) -> Response {
    ui.add(egui::Label::new(RichText::new(text).color(theme::text_muted().to_egui())).wrap())
}

/// A reply a dialog keeps where its drivers and tests read it - the line its last action ran, a
/// verb's answer - drawn as muted text in a footer rather than in a field's well, and taking no
/// room while it is empty: a blank read-only well read as a field to type in, and a reply box of
/// four empty rows was most of a fresh dialog's footer (the Leaders footer's 220 px field, Road
/// Objects' command field and reply well; the look audit's T5). It is recorded as the read-only
/// field or text box it replaced (`kind`, ControlKind::Line or Text), its `value` the text, and a
/// fill writes it as QLineEdit::setText wrote the read-only field. A refusal (`is_error`) is in the
/// error token's colour (shell::dialog::status_colour).
pub fn reply_text(
    ui: &mut Ui,
    name: &str,
    value: &mut String,
    kind: ControlKind,
    is_error: bool,
) -> Response {
    let taken = take(ui, name);
    if let Some(taken) = &taken {
        match &taken.posted.act {
            Act::Fill(text) => {
                if ui.is_visible() && !ui.is_enabled() {
                    settle(ui, taken, Err(fill_disabled(&taken.posted)));
                } else {
                    value.clone_from(text);
                    settle(ui, taken, Ok(()));
                }
            }
            Act::Press => settle(ui, taken, Err(no_button(&taken.posted))),
            Act::ScrollTo => {}
        }
    }
    let response = if value.is_empty() {
        // Nothing to say: a place in the footer, the row's width and no height.
        let width = ui.available_width();
        ui.allocate_exact_size(egui::vec2(width, 0.0), egui::Sense::hover())
            .1
    } else {
        ui.add(
            egui::Label::new(RichText::new(value.as_str()).color(dialog::status_colour(is_error))).wrap(),
        )
    };
    if let Some(taken) = &taken
        && taken.posted.act == Act::ScrollTo
    {
        response.scroll_to_me(None);
        settle(ui, taken, Ok(()));
    }
    let mut record = ControlRecord::new(name, kind);
    record.value = value.clone();
    controls::custom(ui, &response, record);
    response
}

/// A label a dialog keeps for its drivers - a template's check, a scope's count - that takes no
/// room while it has nothing to say: an empty label in a form's row still took the row, a
/// control high, and left a blank band in the form (Leaders' 30 points above "Says", Label
/// Styles' row between Template and Text style; the second look audit's N13 and N15). Recorded
/// as the label it is (ControlKind::Label), its text the text, empty or not; a fill or a press is
/// refused as a label refuses them.
pub fn quiet_label(ui: &mut Ui, name: &str, text: &str, colour: egui::Color32) -> Response {
    if !text.is_empty() {
        return controls::label_as(
            ui,
            name,
            text,
            egui::Label::new(RichText::new(text).color(colour)).wrap(),
        );
    }
    let taken = take(ui, name);
    let response = ui
        .allocate_exact_size(egui::vec2(ui.available_width(), 0.0), egui::Sense::hover())
        .1;
    if let Some(taken) = &taken {
        match &taken.posted.act {
            // As controls::label_as refuses one.
            Act::Fill(text) => {
                if controls::accept_fill(ui, taken) {
                    settle(ui, taken, Err(no_field(name, text)));
                }
            }
            Act::Press => settle(ui, taken, Err(no_button(&taken.posted))),
            Act::ScrollTo => settle(ui, taken, Ok(())),
        }
    }
    controls::custom(ui, &response, ControlRecord::new(name, ControlKind::Label));
    response
}

/// A list a person picks one row of (`controls::rows`), each row as wide as the column it is
/// drawn in, as a table's chosen row is: egui sizes a selectable to its text, so a list's chosen
/// row was a pill as wide as its words ("Standard" in Text Styles, the Settings pages, the sheet
/// list) beside tables whose chosen row spans them - two looks of one thing (the second look
/// audit's N10). The column must have a width of its own (`Ui::set_width`): a justified row
/// takes all the room it is lent.
pub fn pick_list(
    ui: &mut Ui,
    name: &str,
    rows: &[Vec<String>],
    current: &mut Option<usize>,
) -> Response {
    ui.with_layout(
        egui::Layout::top_down_justified(egui::Align::LEFT),
        |ui| controls::rows(ui, name, rows, current),
    )
    .inner
}

/// A `pick_list` in a well `height` high - a field's fill under a border, its rows scrolling in
/// it - the list beside a form (Text Styles, Label Styles): a list of one name over a column of
/// nothing beside a form of twenty rows read as a void (the second look audit's N15). The column
/// it is drawn in must have a width of its own, as `pick_list`'s.
pub fn list_well(
    ui: &mut Ui,
    name: &str,
    rows: &[Vec<String>],
    current: &mut Option<usize>,
    height: f32,
) -> Response {
    egui::Frame::new()
        .fill(theme::palette::input().to_egui())
        .stroke(egui::Stroke::new(1.0, theme::border().to_egui()))
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::same(2))
        .show(ui, |ui| {
            let inner = (height - 6.0).max(rows_height(ui, 1.0));
            egui::ScrollArea::vertical()
                .id_salt((name, "listWell"))
                .max_height(inner)
                .min_scrolled_height(inner)
                .auto_shrink([false, false])
                .show(ui, |ui| pick_list(ui, name, rows, current))
                .inner
        })
        .inner
}

/// The height a part of a dialog was drawn at the last frame, kept under `id` - what a column
/// beside it is made as tall as (`list_well` beside its form). None before it is first drawn.
pub fn last_height(ui: &Ui, id: egui::Id) -> Option<f32> {
    ui.data(|data| data.get_temp::<f32>(id))
}

/// Keeps `height` under `id` for `last_height`; a change has the frame laid out again at once
/// (Context::request_discard), as the dialog frame does for its own measure, so what is drawn
/// beside the part is never a frame behind it.
pub fn keep_height(ui: &Ui, id: egui::Id, height: f32) {
    if last_height(ui, id).is_none_or(|before| (before - height).abs() > 0.5) {
        ui.data_mut(|data| data.insert_temp(id, height));
        ui.ctx()
            .request_discard("a column was measured for what is beside it");
    }
}

/// A preview's place with nothing in it yet says so: `text`, muted and centred in `rect`, wrapped
/// to its width less a margin - where an empty dark box read as a preview that failed to draw
/// (the look audits' T6).
pub fn preview_placeholder(ui: &Ui, rect: egui::Rect, text: &str) {
    let margin = theme::style::GROUP_GAP;
    let width = (rect.width() - 2.0 * margin).max(1.0);
    let galley = ui.painter().layout(
        String::from(text),
        egui::TextStyle::Body.resolve(ui.style()),
        theme::text_muted().to_egui(),
        width,
    );
    let at = rect.center() - galley.size() / 2.0;
    ui.painter()
        .galley(at, galley, theme::text_muted().to_egui());
}

/// `edit` - a one-line field a table or a grid draws in a cell of its own, which drive::controls'
/// named fields cannot be - given their look: a control's height, its text centred in it and
/// drive::controls::FIELD_INSET in from its left as a choice's is, where egui's own field stands
/// its text 4 in and at the top. The margin is line_with's (controls::field_margin).
pub fn field_look<'t>(ui: &Ui, edit: egui::TextEdit<'t>) -> egui::TextEdit<'t> {
    edit.margin(controls::field_margin())
        .vertical_align(egui::Align::Center)
        .min_size(egui::vec2(0.0, ui.spacing().interact_size.y))
}
