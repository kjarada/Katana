//! The dialog frame: keeping the window's own windows reachable on the screen they are shown on.
//! The port of screen_fit.hpp/.cpp, and the half of theme.cpp that makes a menu taller than the
//! screen one scrolling column.
//!
//! A dialog is laid out for the controls it holds, not for the screen it is shown on: Terrain >
//! Volumes was about 1550 pixels tall, the style manager and the road template manager are as
//! tall, and a 1366 x 768 laptop leaves a window 728 pixels of room under its task bar. A window
//! taller than that has its bottom - the Close button, the results - off the screen. Every dialog
//! that was written to fit is one more that a smaller screen then proves wrong, so this is not
//! left to each dialog:
//!
//! * A dialog is shown no larger than its screen's room: the available area less a margin.
//! * A dialog whose controls cannot be laid out in that room scrolls, so that every control is
//!   reachable by scrolling and the window is as large as the screen lets it be.
//! * The dialog's own buttons are not scrolled with the rest. Its footer - its row of buttons and
//!   what it says under them, the reply to what they did - is kept below the scrolling body. A
//!   footer that needs more than half the room is not kept out, and then everything scrolls.
//! * A dialog that fits is left exactly as it is drawn.
//! * A dialog placed by hand stays where it was put, brought back onto the screen if it was off
//!   it; one that was not is centred in the room.
//!
//! What is the C++'s and not here. In Qt the frame is fitted onto a dialog already laid out, so
//! the footer was FOUND, by reading the dialog's layout (its last row of buttons), and the fitting
//! was done once as the dialog was shown, by an application-wide filter. In egui a dialog is drawn
//! each frame inside its frame, so the footer is DECLARED by the dialog (the second part it gives
//! the frame), and the frame fits it at every frame. The eleven ScreenFit cases about finding a
//! footer in a Qt layout, the filter, and showing a dialog again are exempt for that (the plan's
//! section 3.3).
//!
//! THE MEASURE. The frame does not know a dialog's size before it is drawn, so it measures what it
//! drew: what the frame takes beyond the body (its title bar, borders, footer and spacing) is kept
//! from one frame to the next, and the body is given the room less that. A frame whose measure
//! changed asks egui to lay the frame out again at once (`Context::request_discard`), so the
//! dialog a person sees is the fitted one.
//!
//! Each dialog is a drive target named as its C++ object (drive::registry), its controls recorded
//! in its body or its footer, and its rectangle - the whole of it with its title bar - what a
//! screenshot of it crops to.
//!
//! THE LOOK (the design of 2026-11-16). Three bands edge to edge: a title bar (the title in the
//! heading style, a close cross at the right), the body with PADDING round it, and the footer under
//! a rule. The body is laid out BODY_WIDTH wide at most unless the dialog says otherwise, so a
//! dialog is never as wide as the screen for want of a width. What a dialog lays its controls out
//! with is here too, so a dialog mostly says what it holds: `form` and `labelled` (a label column
//! every row of a body shares, the fields filling the rest - or, where one must not fill,
//! `fixed_fields`, `beside_a_button`), `button_row` (what reads the drawing at the left; Close and
//! the dialog's primary action, drive::controls::primary, last at the right), `note` and `section`.
//! A button in a footer is FOOTER_BUTTON_WIDTH at least (drive::controls reads `in_footer`). This
//! is the window's ONE form: dialog_layout's grid of fixed-width fields, written beside it, was
//! folded into it.

use egui::{
    Align, Align2, Area, Context, CursorIcon, Frame, Id, Layout, Order, Pos2, Rect, RichText,
    ScrollArea, Sense, Stroke, StrokeKind, TextStyle, Ui, UiBuilder, UiKind, UiStackInfo, Vec2,
    WidgetInfo, WidgetType, pos2, vec2,
};

use crate::drive::registry::{
    Part, ScrollRecord, TargetKind, TargetSpec, begin_part, begin_target, end_part, end_target,
    set_target_frame,
};
use crate::theme::palette;

/// What is kept free round a window on every side of its screen's available area, so that its
/// frame never touches the task bar or the edge of the display.
pub const MARGIN: f32 = 8.0;

/// How small a scrolling dialog may be made: a scroll area has no minimum of its own worth the
/// name, and a dialog the size of its title bar is no use to anyone. Held to the room there is, on
/// a screen smaller than this.
pub const LEAST_SCROLLING: Vec2 = Vec2::new(360.0, 240.0);

/// The most of the room (half, as a divisor) a footer may need and still be kept out of the
/// scrolling body: a Close row, a reply box and a status line are some 170 px of a 700 px room,
/// and more than half would leave too little to scroll the rest in.
pub const FOOTER_SHARE: f32 = 2.0;

/// The name of a dialog's scrolling body (the C++'s scroll area, kScreenFitScroll).
pub const SCREEN_FIT_SCROLL: &str = "screenFitScroll";

/// The name of a dialog's footer (kScreenFitFooter): the part its controls are recorded in.
pub const SCREEN_FIT_FOOTER: &str = "screenFitFooter";

// ---- the look of the frame (the design system of 2026-11-16: title, body, footer) -------------
//
// A dialog is three bands edge to edge: a title bar, the body, and the footer under a rule. The
// numbers are the design system's spacing scale (2 / 4 / 8 / 12 / 16): the dialog's padding is 12
// on every side of its body and its footer, the title bar is a 24-point row with 4 above and below
// it, and a footer's buttons share one least width, so that a row of Close, Run and Add reads as
// one row rather than three widths of word.

/// The room between a dialog's edge and what its body or footer holds, on every side.
pub const PADDING: f32 = crate::theme::style::DIALOG_PADDING as f32;

/// The title bar's height: a 24-point row with 4 above and below it.
pub const TITLE_HEIGHT: f32 = 32.0;

/// The close button in the title bar: its square, and its cross's half-width inside it.
const CLOSE_SIZE: f32 = 20.0;
const CLOSE_CROSS: f32 = 4.0;

/// The width a dialog's body is laid out at, at most, unless the dialog asks for another
/// (`DialogFrame::width`, or its own `Ui::set_max_width`): what a note wraps at and a form's
/// fields fill. Without it a body was as wide as the screen - a wrapped note ran 1,300 points
/// across a laptop, a scope's group filled it - and every dialog looked empty.
pub const BODY_WIDTH: f32 = 520.0;

/// The width of a body of two columns side by side (a scope beside its options, a source beside
/// its tool's fields): two columns of 400 points, a scope's group and a form of short labels each.
pub const TWO_COLUMN_WIDTH: f32 = 800.0;

/// The least width of a dialog's body: a dialog of one field is not a sliver under its title.
pub const LEAST_BODY_WIDTH: f32 = 320.0;

/// The least width of a button in a dialog's footer (Close, Run, OK): one width for a row of
/// short words.
pub const FOOTER_BUTTON_WIDTH: f32 = 72.0;

/// The widest a form's label column grows (`labelled`): the longest label of the survey and
/// terrain forms ("Closing reference mark:") at the Standard text size. A longer label wraps in
/// it, so one long label does not push every field of its dialog to the right. It is also the
/// column's width where a row is drawn outside a dialog's body, which has no column to share.
pub const LABEL_WIDTH: f32 = 150.0;

/// The tag of a dialog's body `Ui`, whose labelled rows share one label column (`labelled`).
const BODY_TAG: &str = "katanaDialogBody";

/// A dialog body's label column, kept between passes in the context's memory: where its rows'
/// fields start, from the body's left edge, and the furthest a label of this pass has reached.
#[derive(Clone, Copy, Debug, Default)]
struct LabelColumn {
    pass: u64,
    used: f32,
    widest: f32,
}

/// The gaps of a form (`form`): 12 between a label and its field, 4 between rows (the
/// design system's gap inside a group).
pub const FORM_SPACING: Vec2 = Vec2::new(PADDING, crate::theme::style::GAP);

/// The space between two groups of a dialog's body (the design system's gap between groups).
pub const GROUP_GAP: f32 = crate::theme::style::GROUP_GAP;

/// The tag of a dialog's footer `Ui` and of a `button_row`'s, which `in_footer` reads: a button
/// drawn in one is given FOOTER_BUTTON_WIDTH at least (drive::controls).
const FOOTER_TAG: &str = "katanaDialogFooter";

/// Whether `ui` is in a dialog's footer or a `button_row`.
pub fn in_footer(ui: &Ui) -> bool {
    ui.stack()
        .iter()
        .any(|frame| frame.tags().contains(FOOTER_TAG))
}

/// The rectangle a window's frame may take on a screen whose available area (what a task bar and
/// a dock leave it) is `available`: that area less MARGIN on every side. An empty rectangle when
/// there is no screen to measure, which a caller takes as no limit.
pub fn room_on_screen(available: Rect) -> Rect {
    if !available.is_positive() {
        return Rect::NOTHING;
    }
    available.shrink(MARGIN)
}

/// What the dialog frame adds round a dialog's content, by `style`: its border, the padding round
/// the body and the title bar above. Never zero: a title bar is allowed for. A footer is not
/// counted - a dialog may have none - and is measured once drawn (THE MEASURE, above).
pub fn frame_of(style: &egui::Style) -> Vec2 {
    let border = 2.0 * style.visuals.window_stroke().width;
    Vec2::new(
        border + 2.0 * PADDING,
        border + TITLE_HEIGHT + 2.0 * PADDING,
    )
}

/// The frame round the whole dialog: the theme's window fill, border, corners and shadow, and no
/// margin of its own - the title bar, the body and the footer each keep theirs.
fn outer_frame(style: &egui::Style) -> Frame {
    Frame::new()
        .fill(style.visuals.window_fill())
        .stroke(style.visuals.window_stroke())
        .corner_radius(style.visuals.window_corner_radius)
        .shadow(style.visuals.window_shadow)
}

/// The least size of a title bar showing `title`: the padding, the title, a gap and the close
/// button. A dialog is never narrower than its title.
fn title_bar_size(ui: &Ui, title: &str) -> Vec2 {
    let font = TextStyle::Heading.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(String::from(title), font, egui::Color32::PLACEHOLDER);
    Vec2::new(
        PADDING + galley.size().x + PADDING + CLOSE_SIZE + (PADDING - CLOSE_CROSS),
        TITLE_HEIGHT,
    )
}

/// Paints the title bar over `rect` - the title in the heading style - and draws its close
/// button: a cross in the secondary text colour, in the text colour on a raised square when the
/// pointer is over it. Whether the close button was clicked.
fn title_bar(ui: &Ui, rect: Rect, title: &str, id: Id) -> bool {
    let font = TextStyle::Heading.resolve(ui.style());
    let colour = ui.style().visuals.strong_text_color();
    let galley = ui
        .painter()
        .layout_no_wrap(String::from(title), font, colour);
    let at = pos2(
        rect.left() + PADDING,
        rect.center().y - galley.size().y / 2.0,
    );
    ui.painter().galley(at, galley, colour);

    let square = Rect::from_center_size(
        pos2(
            rect.right() - (PADDING - CLOSE_CROSS) - CLOSE_SIZE / 2.0,
            rect.center().y,
        ),
        Vec2::splat(CLOSE_SIZE),
    );
    let close = ui
        .interact(square, id.with("close"), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    close.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Close"));
    let ink = if close.hovered() {
        ui.painter().rect(
            square,
            egui::CornerRadius::same(4),
            palette::hover().to_egui(),
            Stroke::NONE,
            StrokeKind::Inside,
        );
        palette::text().to_egui()
    } else {
        palette::text_muted().to_egui()
    };
    let centre = square.center();
    let cross = Stroke::new(1.5, ink);
    for (from, to) in [
        (
            vec2(-CLOSE_CROSS, -CLOSE_CROSS),
            vec2(CLOSE_CROSS, CLOSE_CROSS),
        ),
        (
            vec2(-CLOSE_CROSS, CLOSE_CROSS),
            vec2(CLOSE_CROSS, -CLOSE_CROSS),
        ),
    ] {
        ui.painter()
            .line_segment([centre + from, centre + to], cross);
    }
    close.clicked()
}

/// A rule a pixel high across `x` at `y`, in the border colour: under the title bar, over the
/// footer.
fn rule(ui: &Ui, x: egui::Rangef, y: f32) {
    let y = ui.painter().round_to_pixel_center(y);
    ui.painter()
        .hline(x, y, Stroke::new(1.0, palette::border().to_egui()));
}

/// Draws `add` in a child of `ui`, top down, tagged as a dialog's footer: a button drawn in it is
/// FOOTER_BUTTON_WIDTH at least (dialog_layout::footer draws its row in one).
pub(crate) fn footer_ui<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    ui.scope_builder(
        UiBuilder::new()
            .layout(Layout::top_down(Align::Min))
            .ui_stack_info(UiStackInfo::default().with_tag(FOOTER_TAG)),
        add,
    )
}

// ---- what a dialog lays its controls out with ---------------------------------------------------

/// A dialog's row of buttons, as wide as what holds it: `leading` at its left - what reads the
/// drawing into the form, a Preview - then `trailing` against its right edge, written in the order
/// they show: Close, then the dialog's primary action last (drive::controls::primary). Each button
/// is FOOTER_BUTTON_WIDTH at least.
///
/// egui lays a row out left to right as it is drawn, so what pushes `trailing` to the right is its
/// width as the last frame measured it (kept in the context's memory); a frame that finds it
/// changed asks for the frame to be laid out again (`Context::request_discard`), as the dialog
/// frame does for its own measure, so the row a person sees is the settled one.
pub fn button_row<L, R>(
    ui: &mut Ui,
    leading: impl FnOnce(&mut Ui) -> L,
    trailing: impl FnOnce(&mut Ui) -> R,
) -> (L, R) {
    ui.scope_builder(
        UiBuilder::new().ui_stack_info(UiStackInfo::default().with_tag(FOOTER_TAG)),
        |ui| {
            ui.horizontal(|ui| {
                let id = ui.make_persistent_id("katanaButtonRowTrailing");
                let wanted: f32 = ui.data(|data| data.get_temp(id)).unwrap_or(0.0);
                // What is at the left - buttons, or a status that wraps - is held to the row less
                // the right end, so it never runs under the buttons.
                let room = Vec2::new(
                    (ui.available_width() - wanted - ui.spacing().item_spacing.x).max(0.0),
                    ui.spacing().interact_size.y,
                );
                let left = ui.allocate_ui(room, leading).inner;
                // The spacing after what is at the left is already behind the cursor.
                let gap = ui.available_width() - wanted;
                if gap > 0.0 {
                    ui.add_space(gap);
                }
                let drawn = ui.horizontal(trailing);
                let width = drawn.response.rect.width();
                if (width - wanted).abs() > 0.5 {
                    ui.data_mut(|data| data.insert_temp(id, width));
                    ui.ctx()
                        .request_discard("a dialog's row of buttons measured its right end");
                }
                (left, drawn.inner)
            })
            .inner
        },
    )
    .inner
}

/// A dialog's form (QFormLayout): its rows (`Form::row`) one under another, FORM_SPACING apart.
/// Each row is a label in the body's label column (`labelled`) and its field beside it, running
/// to the body's right edge, so every field of a dialog starts where the others do.
///
/// Rows that find their column through the body, rather than egui's Grid: a row is the same
/// whatever is drawn between two rows (a Grid takes every widget drawn into it as a cell, so a
/// check box between two rows shifted every row after it by a column), the rows of a dialog line
/// up whichever helper or group drew them (a Grid lines up only its own cells), and a field fills
/// its row rather than being one fixed width (a Grid's field column is as wide as its widest
/// field, so its fields were each given a width of their own). A label longer than the column
/// wraps in it.
pub fn form<R>(ui: &mut Ui, add: impl FnOnce(&mut Form<'_>) -> R) -> R {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = FORM_SPACING.y;
        add(&mut Form { ui })
    })
    .inner
}

/// A labelled row (`Form::row`), wherever it is drawn: what a dialog's shared helpers draw their
/// rows with (a survey dialog's form_row, a GIS dialog's field), so that they line up in a form
/// or out of one.
///
/// THE LABEL COLUMN. Every labelled row of one dialog's body shares one label column, as wide as
/// the widest label of the body (LABEL_WIDTH at most, a longer label wrapping in it), so the
/// fields of the whole dialog start at one edge - those inside a group and those under it, those
/// a dialog draws itself and those its shared helpers draw - and a dialog of short labels ("Ids:")
/// has no wide gap before its fields. The widest is measured as the rows are drawn and used from
/// the next pass, as egui's Grid measures its columns; a dialog's first pass is laid out again at
/// once (`Context::request_discard`), so the column a person sees is the measured one. A row
/// drawn outside a dialog's body has a column LABEL_WIDTH wide.
pub fn labelled<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    row_with_text(
        ui,
        label,
        |ui| {
            if !label.is_empty() {
                ui.add(
                    egui::Label::new(RichText::new(label).color(palette::text_muted().to_egui()))
                        .wrap(),
                );
            }
        },
        add,
    )
}

/// A form's row whose label cell `label` draws itself (a named caption a driver reads) showing
/// `text`, and its field `add` beside it, as `labelled` lays them out; the column is measured by
/// `text` laid out on one line, as `labelled`'s is.
pub fn row_with_text<R>(
    ui: &mut Ui,
    text: &str,
    label: impl FnOnce(&mut Ui),
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    let natural = if text.is_empty() {
        0.0
    } else {
        ui.painter()
            .layout_no_wrap(
                String::from(text),
                TextStyle::Body.resolve(ui.style()),
                egui::Color32::PLACEHOLDER,
            )
            .size()
            .x
    };
    row_inner(ui, Some(natural), label, add)
}

/// A form's row whose label cell `label` draws itself, measured as drawn. Prefer `row_with_text`:
/// a label measured as drawn is measured in the cell the column already gives it, wrapped to that
/// width, so one shown after the column was measured narrower (a row that appears later, a page
/// turned) never widens it and stays wrapped, a word to a line. Kept only while the View Scale
/// dialog draws its captions with it; moved to `row_with_text`, it goes.
pub fn row_with<R>(ui: &mut Ui, label: impl FnOnce(&mut Ui), add: impl FnOnce(&mut Ui) -> R) -> R {
    row_inner(ui, None, label, add)
}

/// The label column a row drawn in `ui` starting `start` (its left edge) shares, and where the row
/// starts from its body's left edge: the nearest dialog body it is in, and of that body the rows
/// that start within COLUMN_REACH of each other. A row in a group starts a group's margin further
/// in and shares the column; a row of a second column of the body, beside the first, starts half
/// the body further in and has a label column of its own.
fn label_column_body(ui: &Ui, start: f32) -> Option<(Id, f32)> {
    ui.stack()
        .iter()
        .find(|frame| frame.tags().contains(BODY_TAG))
        .map(|frame| {
            let offset = start - frame.max_rect.left();
            let band = (offset / COLUMN_REACH).round() as i64;
            (frame.id.with(("katanaLabelColumn", band)), offset)
        })
}

/// How far apart two rows of a body may start and share a label column: three group margins
/// (egui's group frame is 6 in and a pixel of border), so a group in a group still shares it, and
/// far less than a column of a body laid out in two (TWO_COLUMN_WIDTH).
const COLUMN_REACH: f32 = 48.0;

/// Where the label column of a row drawn in `ui` ends this pass, from its body's left edge (THE
/// LABEL COLUMN): the furthest a label of its body reached the last pass. LABEL_WIDTH, from the
/// row's own start, before the body has been measured, and for a row in no body.
fn label_column(ui: &Ui, body: Option<Id>, offset: f32) -> (Option<Id>, f32) {
    let Some(key) = body else {
        return (None, offset + LABEL_WIDTH);
    };
    let pass = ui.ctx().cumulative_pass_nr();
    let mut column = ui
        .data(|data| data.get_temp::<LabelColumn>(key))
        .unwrap_or_default();
    if column.pass != pass {
        // The first row of a pass: what the last pass measured is the edge from now.
        if column.widest > 0.0 {
            column.used = column.widest;
        }
        column.widest = 0.0;
        column.pass = pass;
        ui.data_mut(|data| data.insert_temp(key, column));
    }
    let edge = if column.used > 0.0 {
        column.used
    } else {
        offset + LABEL_WIDTH
    };
    (Some(key), edge)
}

/// Tells the label column `key` a label reached `reach` from its body's left edge (its row's
/// start and its own width, held to LABEL_WIDTH), the column ending at `edge`: a label past the
/// column's edge, or a column not yet measured, has the pass laid out again.
fn widen_label_column(ui: &Ui, key: Id, reach: f32, edge: f32) {
    let mut column = ui
        .data(|data| data.get_temp::<LabelColumn>(key))
        .unwrap_or_default();
    column.widest = column.widest.max(reach);
    let unmeasured = column.used <= 0.0;
    ui.data_mut(|data| data.insert_temp(key, column));
    if unmeasured || reach > edge + 0.5 {
        ui.ctx()
            .request_discard("a dialog's label column measured its labels");
    }
}

/// A labelled row whose label `label` draws, `natural` wide on one line (None: as it is drawn).
fn row_inner<R>(
    ui: &mut Ui,
    natural: Option<f32>,
    label: impl FnOnce(&mut Ui),
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    // Where the row starts in its body: a row inside a group starts further in, and its label
    // cell is that much narrower, so its field starts where the body's do.
    let start = ui.available_rect_before_wrap().left();
    let body = label_column_body(ui, start);
    let offset = body.map_or(0.0, |(_, offset)| offset);
    let (key, edge) = label_column(ui, body.map(|(key, _)| key), offset);
    let width = (edge - offset).max(0.0);
    ui.horizontal_top(|ui| {
        let spacing = ui.spacing().item_spacing.x;
        ui.spacing_mut().item_spacing.x = FORM_SPACING.x;
        // The label's cell: as high as a control, so a one-line label sits level with the field
        // beside it, and at the top of a taller one (a text box) rather than at its middle.
        let height = ui.spacing().interact_size.y;
        let drawn = ui
            .allocate_ui_with_layout(
                Vec2::new(width, height),
                Layout::left_to_right(Align::Center).with_main_wrap(true),
                |ui| {
                    // The cell's size first: set after the label, egui would add it beyond the
                    // label as the room for the next widget.
                    ui.set_min_size(Vec2::new(width, height));
                    ui.scope(label).response.rect.width()
                },
            )
            .inner;
        if let Some(key) = key {
            let reach = offset + natural.unwrap_or(drawn).min(LABEL_WIDTH);
            widen_label_column(ui, key, reach, edge);
        }
        ui.spacing_mut().item_spacing.x = spacing;
        fill_fields(ui);
        add(ui)
    })
    .inner
}

/// The rows of a `form`.
pub struct Form<'a> {
    ui: &'a mut Ui,
}

impl Form<'_> {
    /// A row: `label` in the secondary text colour in the label column, and what `add` draws
    /// beside it, a one-line field and a choice list filling what is left of the row. A row of two
    /// fields gives the first a width of its own (LineOptions::width) to leave the second room.
    pub fn row<R>(&mut self, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
        labelled(self.ui, label, add)
    }

    /// A row with no label: a check box or a note under the fields, lined up with them.
    pub fn under<R>(&mut self, add: impl FnOnce(&mut Ui) -> R) -> R {
        labelled(self.ui, "", add)
    }

    /// The form's `Ui`, for what is drawn across the whole row.
    pub fn ui(&mut self) -> &mut Ui {
        self.ui
    }
}

/// A form's label: the secondary text colour, so the values read first.
pub fn form_label(ui: &mut Ui, label: &str) -> egui::Response {
    ui.label(RichText::new(label).color(palette::text_muted().to_egui()))
}

/// Makes a one-line field and a choice list drawn in `ui` with no width of their own fill what is
/// left of it: egui sizes both from its spacing.
pub fn fill_fields(ui: &mut Ui) {
    let width = ui.available_width();
    ui.spacing_mut().text_edit_width = width;
    ui.spacing_mut().combo_width = width;
}

/// The width a field may take in what is left of `ui`'s row and still leave a button reading
/// `text` its room beside it (a file's field and its Browse...): a field filling the row would
/// push the button past the dialog's edge.
pub fn beside_a_button(ui: &Ui, text: &str) -> f32 {
    beside_buttons(ui, &[text])
}

/// The width a field may take in what is left of `ui`'s row and still leave buttons reading
/// `texts` their room beside it, before or after it (`beside_a_button`).
pub fn beside_buttons(ui: &Ui, texts: &[&str]) -> f32 {
    let buttons: f32 = texts
        .iter()
        .map(|text| {
            let galley = ui.painter().layout_no_wrap(
                String::from(*text),
                TextStyle::Button.resolve(ui.style()),
                egui::Color32::PLACEHOLDER,
            );
            (galley.size().x + 2.0 * ui.spacing().button_padding.x)
                .max(ui.spacing().interact_size.x)
                + ui.spacing().item_spacing.x
        })
        .sum();
    (ui.available_width() - buttons).max(LEAST_FIELD_WIDTH)
}

/// The least width `beside_a_button` leaves a field.
const LEAST_FIELD_WIDTH: f32 = 80.0;

/// Makes a one-line field and a choice list drawn in `ui` with no width of their own `width` wide,
/// held to what is left of the row: what a row of a form draws in place of `fill_fields` when its
/// field must not fill - one of several fields side by side, or a short one beside a wide one.
pub fn fixed_fields(ui: &mut Ui, width: f32) {
    let width = width.min(ui.available_width()).max(0.0);
    ui.spacing_mut().text_edit_width = width;
    ui.spacing_mut().combo_width = width;
}

/// The width of a field of a short word or a number in a row of fields that is not a form's - the
/// row under a table that adds an entry to it ("from", "to", a name) - where filling the row would
/// leave the row's buttons no room: a name of a dozen characters, or a number of eight digits with
/// its unit, at the Standard text size. Half of it holds a station or a percentage.
pub const SHORT_FIELD_WIDTH: f32 = 130.0;

/// The gap above a group's heading (`section`): the dialog's padding, the next step of the spacing
/// scale after GROUP_GAP, so that groups read as blocks without boxes round them.
pub const SECTION_GAP: f32 = PADDING;

/// A group's heading inside a dialog's body ("Customisation", "Results"): strong text, SECTION_GAP
/// above it unless it is the first thing in its `ui` (the body's padding is room enough there).
/// The group's rows follow it (`form`, `labelled`).
pub fn section(ui: &mut Ui, title: &str) -> egui::Response {
    if ui.cursor().min.y > ui.max_rect().min.y + 0.5 {
        ui.add_space((SECTION_GAP - ui.spacing().item_spacing.y).max(0.0));
    }
    ui.label(RichText::new(title).strong())
}

/// A note in the secondary text colour, wrapped at the body's width: what a dialog says of what it
/// does, at its top.
pub fn note(ui: &mut Ui, text: &str) -> egui::Response {
    ui.add(egui::Label::new(RichText::new(text).color(palette::text_muted().to_egui())).wrap())
}

/// What a dialog's frame drew this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct DialogShown<R> {
    /// What the dialog's body answered.
    pub body: R,
    /// The whole dialog, its title bar included, in points.
    pub rect: Rect,
    /// Whether its body scrolls: its controls do not fit the room.
    pub scrolls: bool,
    /// Whether the title bar's close button was clicked.
    pub closed: bool,
}

/// A dialog's frame and what it keeps between frames: its object name and title, whether it is
/// modal, where it was put by hand, and what it measured.
#[derive(Clone, Debug, PartialEq)]
pub struct DialogFrame {
    name: String,
    title: String,
    modal: bool,
    deletes_on_close: bool,
    placed: Option<Pos2>,
    chrome: Option<Vec2>,
    footer_height: Option<f32>,
    scrolled: bool,
    rect: Option<Rect>,
    minimum_height: Option<f32>,
    width: f32,
}

impl DialogFrame {
    /// The frame of the dialog the C++ named `name`, titled `title`; not modal, centred, its body
    /// BODY_WIDTH wide at most.
    pub fn new(name: &str, title: &str) -> Self {
        Self {
            name: String::from(name),
            title: String::from(title),
            modal: false,
            deletes_on_close: false,
            placed: None,
            chrome: None,
            footer_height: None,
            scrolled: false,
            rect: None,
            minimum_height: None,
            width: BODY_WIDTH,
        }
    }

    /// The same frame, its body laid out `width` points wide at most (BODY_WIDTH unless said):
    /// what its notes wrap at and its forms' fields fill. Held to the screen's room.
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// The width the body is laid out at, at most.
    pub fn body_width(&self) -> f32 {
        self.width
    }

    /// Sets the width the body is laid out at, at most (`width`).
    pub fn set_body_width(&mut self, width: f32) {
        self.width = width;
    }

    /// The same frame, modal (QDialog::setModal): the rest of the window takes no input while it
    /// is shown. A headless run is never stopped by it.
    pub fn modal(mut self, modal: bool) -> Self {
        self.modal = modal;
        self
    }

    /// The same frame, its dialog gone when it closes (Qt::WA_DeleteOnClose): a step after it is
    /// told the target has closed, where a dialog the window keeps was only hidden and stayed the
    /// C++ steps' target (TargetSpec::deletes_on_close).
    pub fn deletes_on_close(mut self, deletes: bool) -> Self {
        self.deletes_on_close = deletes;
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// QObject::setObjectName: the dialog drawn, and found by a driver, as `name` from the next
    /// frame - how the C++ window named its copy of a manager (formatLayersDialog).
    pub fn set_name(&mut self, name: &str) {
        self.name = String::from(name);
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: &str) {
        self.title = String::from(title);
    }

    pub fn is_modal(&self) -> bool {
        self.modal
    }

    /// QWidget::move: the dialog put by hand at `at` (its top left), where it stays - brought back
    /// onto the screen if `at` is off it.
    pub fn place(&mut self, at: Pos2) {
        self.placed = Some(at);
    }

    /// Where it was put by hand, as the last frame kept it on the screen; None when it is centred.
    pub fn placed(&self) -> Option<Pos2> {
        self.placed
    }

    /// QWidget::setMinimumHeight: the dialog, its title bar and footer included, no shorter than
    /// `height` points. Its body is made up to it, and scrolls as any other body does where the
    /// screen has not the room.
    pub fn set_minimum_height(&mut self, height: f32) {
        self.minimum_height = Some(height);
    }

    /// The whole dialog as the last frame drew it.
    pub fn rect(&self) -> Option<Rect> {
        self.rect
    }

    /// Draws the dialog in its frame: `body`, which scrolls when the dialog does not fit, and
    /// `footer` - its button row and what it says under it - kept below the body, both lent
    /// `state` (the dialog's model) in turn.
    pub fn show<T: ?Sized, R>(
        &mut self,
        ctx: &Context,
        state: &mut T,
        body: impl FnOnce(&mut Ui, &mut T) -> R,
        footer: impl FnOnce(&mut Ui, &mut T),
    ) -> DialogShown<R> {
        self.show_parts(ctx, state, body, Some(Box::new(footer)))
    }

    /// Draws a dialog with no footer: everything it holds is its body, which scrolls.
    pub fn show_body<T: ?Sized, R>(
        &mut self,
        ctx: &Context,
        state: &mut T,
        body: impl FnOnce(&mut Ui, &mut T) -> R,
    ) -> DialogShown<R> {
        self.show_parts(ctx, state, body, None)
    }

    #[allow(
        clippy::type_complexity,
        reason = "the footer is the one optional closure of show and show_body"
    )]
    fn show_parts<T: ?Sized, R>(
        &mut self,
        ctx: &Context,
        state: &mut T,
        body: impl FnOnce(&mut Ui, &mut T) -> R,
        footer: Option<Box<dyn FnOnce(&mut Ui, &mut T) + '_>>,
    ) -> DialogShown<R> {
        let room = room_on_screen(ctx.content_rect());
        let style = ctx.global_style();
        let frame_size = frame_of(&style);
        let client = (room.size() - frame_size).max(Vec2::ZERO);
        // The frame beyond the body, as the last frame measured it; the frame alone at first.
        let chrome = self.chrome.unwrap_or(frame_size);
        let body_room = (room.size() - chrome).max(Vec2::ZERO);
        let footer_in_body = footer.is_some()
            && self
                .footer_height
                .is_some_and(|height| height > client.y / FOOTER_SHARE);
        // A dialog that scrolls is kept no smaller than the least a scrolling one may be.
        let least_body = if self.scrolled {
            (Vec2::new(
                LEAST_SCROLLING.x.min(client.x),
                LEAST_SCROLLING.y.min(client.y),
            ) - chrome)
                .max(Vec2::ZERO)
        } else {
            Vec2::ZERO
        };

        let id = Id::new(("katanaDialog", self.name.as_str()));
        let order = if self.modal {
            Order::Foreground
        } else {
            Order::Middle
        };
        let mut area = Area::new(id)
            .kind(UiKind::Window)
            .order(order)
            .constrain_to(room)
            .movable(false)
            .fade_in(false);
        area = match self.placed {
            Some(at) => area.pivot(Align2::LEFT_TOP).current_pos(at),
            None => area.pivot(Align2::CENTER_CENTER).current_pos(room.center()),
        };
        if self.modal {
            let layer = area.layer();
            ctx.memory_mut(|memory| memory.set_modal_layer(layer));
        }
        let spec = TargetSpec::new(&self.name, TargetKind::Dialog, &self.title)
            .modal(self.modal)
            .deletes_on_close(self.deletes_on_close);
        let title = self.title.clone();
        let mut footer = footer;
        // The body's part of a minimum height: what the frame takes beyond it, as last measured,
        // taken off.
        let least_content = self
            .minimum_height
            .map(|height| (height - chrome.y).max(0.0));

        // The width the body is laid out at: the dialog's, held to the room there is.
        let width = self.width.min(body_room.x).max(0.0);

        let shown = area.show(ctx, |ui| {
            let origin = ui.max_rect().min;
            begin_target(ui, spec);
            let style = ui.style().clone();
            let framed = outer_frame(&style).show(ui, |ui| {
                // The frame's three bands meet edge to edge; each sets the theme's spacing back for
                // what it holds.
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                let least_title = title_bar_size(ui, &title);
                let (title_rect, _) = ui.allocate_exact_size(least_title, Sense::hover());

                let padded = Frame::new().inner_margin(PADDING).show(ui, |ui| {
                    ui.set_style(style.clone());
                    begin_part(ui, Part::Body);
                    let body_rect = Rect::from_min_size(ui.cursor().min, body_room);
                    let scope = ui.scope_builder(UiBuilder::new().max_rect(body_rect), |ui| {
                        ScrollArea::both()
                            .id_salt(SCREEN_FIT_SCROLL)
                            .max_width(body_room.x)
                            .max_height(body_room.y)
                            .min_scrolled_width(least_body.x)
                            .min_scrolled_height(least_body.y)
                            .auto_shrink([true, true])
                            .animated(false)
                            .show(ui, |ui| {
                                ui.set_max_width(width);
                                ui.set_min_width(LEAST_BODY_WIDTH.min(width));
                                // Tagged as a body, whose labelled rows share a label column.
                                let answer = ui
                                    .scope_builder(
                                        UiBuilder::new().ui_stack_info(
                                            UiStackInfo::default().with_tag(BODY_TAG),
                                        ),
                                        |ui| body(ui, &mut *state),
                                    )
                                    .inner;
                                if footer_in_body && let Some(footer) = footer.take() {
                                    ui.add_space(PADDING);
                                    footer_ui(ui, |ui| footer(ui, &mut *state));
                                }
                                if let Some(least) = least_content {
                                    let short = least - ui.min_rect().height();
                                    if short > 0.0 {
                                        ui.add_space(short);
                                    }
                                }
                                answer
                            })
                    });
                    end_part(ui);
                    scope
                });
                let scope = padded.inner;

                let mut footer_rect = None;
                let mut footer_top = None;
                if let Some(footer) = footer.take() {
                    footer_top = Some(ui.cursor().min.y);
                    let row_width = scope
                        .response
                        .rect
                        .width()
                        .max(least_title.x - 2.0 * PADDING);
                    let drawn = Frame::new().inner_margin(PADDING).show(ui, |ui| {
                        ui.set_style(style.clone());
                        // The footer's rows are as wide as the body (or the title, if wider), so
                        // a row of buttons ends at the dialog's right edge less its padding.
                        ui.set_max_width(row_width);
                        begin_part(ui, Part::Footer);
                        let drawn = footer_ui(ui, |ui| footer(ui, &mut *state));
                        end_part(ui);
                        drawn.response.rect
                    });
                    footer_rect = Some(drawn.inner);
                }

                // The title bar across the whole dialog, now that its width is known; the rules
                // under it and over the footer.
                let whole = ui.min_rect();
                let title_rect = Rect::from_min_max(
                    title_rect.min,
                    pos2(whole.right().max(title_rect.right()), title_rect.bottom()),
                );
                let drag = ui.interact(title_rect, id.with("title"), Sense::drag());
                let moved = drag.dragged().then(|| origin + drag.drag_delta());
                let closed = title_bar(ui, title_rect, &title, id);
                rule(ui, title_rect.x_range(), title_rect.bottom());
                if let Some(top) = footer_top {
                    rule(ui, title_rect.x_range(), top);
                }
                (closed, moved, scope.response.rect, scope.inner, footer_rect)
            });
            let (closed, moved, body_outer, output, footer_rect) = framed.inner;
            let frame_rect = framed.response.rect;
            let scroll = ScrollRecord {
                viewport: output.inner_rect,
                content: output.content_size,
                offset: output.state.offset,
            };
            let scrolls = scroll.scrolls();
            set_target_frame(
                ui,
                scrolls.then_some(scroll),
                footer_rect.filter(|_| scrolls && !footer_in_body),
            );
            end_target(ui, frame_rect);
            (
                closed,
                moved,
                body_outer,
                output.inner,
                scrolls,
                footer_rect,
                frame_rect,
            )
        });

        let (closed, moved, body_outer, answer, scrolls, footer_rect, frame_rect) = shown.inner;
        // What the frame took beyond the body: a change is laid out again at once.
        let chrome_now = frame_rect.size() - body_outer.size();
        let measured_again = self
            .chrome
            .is_none_or(|before| (before - chrome_now).length() > 0.5);
        self.chrome = Some(chrome_now);
        // A footer drawn in the body this frame was not measured apart: its height stays.
        if !footer_in_body {
            self.footer_height = footer_rect.map(|rect| rect.height());
        }
        if measured_again || self.scrolled != scrolls {
            ctx.request_discard("the dialog frame measured what it holds");
        }
        self.scrolled = scrolls;
        if let Some(at) = moved {
            self.placed = Some(at);
        } else if self.placed.is_some() {
            // Kept where the screen brought it back to, as QWidget::move keeps it.
            self.placed = Some(shown.response.rect.min);
        }
        self.rect = Some(frame_rect);
        DialogShown {
            body: answer,
            rect: frame_rect,
            scrolls,
            closed,
        }
    }
}

/// What a popup menu drew.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuShown {
    /// The whole menu, in points.
    pub rect: Rect,
    /// Its column of items: scrolled when the menu does not fit the screen.
    pub scroll: ScrollRecord,
}

/// A menu's items in ONE column the height of the room there is at most, scrolled when they need
/// more (theme.cpp's SH_Menu_Scrollable): Fusion laid a menu taller than its screen out in several
/// columns side by side, which a menu of sections and submenus turns into a wall nobody can read
/// down. For a menu's contents wherever a menu is drawn; the wheel and the keys scroll it.
pub fn menu_column(ui: &mut Ui, add: impl FnOnce(&mut Ui)) -> ScrollRecord {
    let screen = ui.ctx().content_rect();
    let frame = Frame::menu(ui.style()).total_margin().sum();
    let height = (screen.height() - frame.y).max(0.0);
    let output = ScrollArea::vertical()
        .id_salt("katanaMenuColumn")
        .max_height(height)
        .auto_shrink([true, true])
        .animated(false)
        .show(ui, |ui| {
            ui.with_layout(Layout::top_down_justified(Align::LEFT), add);
        });
    ScrollRecord {
        viewport: output.inner_rect,
        content: output.content_size,
        offset: output.state.offset,
    }
}

/// A menu popped up at `at` (QMenu::popup): its items drawn by `add` in one column
/// (`menu_column`), the menu kept on the screen, and a Menu target named `name` whose record
/// carries the column's scroll, scrolled or not.
pub fn popup_menu(ctx: &Context, name: &str, at: Pos2, add: impl FnOnce(&mut Ui)) -> MenuShown {
    let screen = ctx.content_rect();
    let area = Area::new(Id::new(("katanaMenu", name)))
        .kind(UiKind::Menu)
        .order(Order::Foreground)
        .constrain_to(screen)
        .movable(false)
        .fade_in(false)
        .pivot(Align2::LEFT_TOP)
        .current_pos(at);
    let spec = TargetSpec::new(name, TargetKind::Menu, "");
    let shown = area.show(ctx, |ui| {
        begin_target(ui, spec);
        let framed = Frame::menu(ui.style()).show(ui, |ui| menu_column(ui, add));
        let rect = framed.response.rect;
        set_target_frame(ui, Some(framed.inner), None);
        end_target(ui, rect);
        MenuShown {
            rect,
            scroll: framed.inner,
        }
    });
    shown.inner
}
