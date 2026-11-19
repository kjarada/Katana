//! The named controls: thin wrappers over egui's widgets that keep the C++ object name every Qt
//! control had (CLAUDE.md section 1; the plan's section 4.1). Each call, every frame:
//!
//! 1. takes from the mailbox an act a driver posted for it, and applies it as a person's act
//!    would - a fill writes the value and the response says changed, a press makes `clicked()`
//!    true, a row is selected by its text, a tab brought to the front by its title - or refuses it
//!    with the C++'s sentence (a disabled field, a hidden button; drive::mailbox);
//! 2. draws the egui widget;
//! 3. records what it drew in the frame's registry (drive::registry) and gives its AccessKit node
//!    the name as its author id.
//!
//! The value a control holds stays the caller's (`&mut String`, `&mut bool`), as an immediate-mode
//! widget's does: a dialog's model is its fields, and its controls draw them.
//!
//! An act is applied BEFORE the widget is drawn, so what is drawn is what the act left, and the
//! response's flags are set after: `changed()` for what a fill or a press changed - only when it
//! changed, as Qt signals a change - and `clicked()` for a press. A press of a check box toggles
//! it, as QAbstractButton::click does.
//!
//! A control hidden by its owner (drawn with `Ui::add_visible(false, ..)` or in an invisible `Ui`)
//! still records itself, not visible, so a press of it is refused "is hidden" as the C++ refuses
//! it; a control its owner did not draw at all is not there, and a press of it is refused "has no
//! button".

use std::ops::RangeInclusive;

use egui::{Response, Ui};

use super::mailbox::{
    Act, Taken, fill_disabled, lists_no, no_button, no_choice, no_field, no_tab,
    number_out_of_range, press_disabled, press_hidden, whole_out_of_range,
};
use super::registry::{ControlKind, ControlRecord, settle, take};

// ---- what every control does with an act ------------------------------------------------------

/// Makes `clicked()` true, as egui marks a click that came by the keyboard or an assistive tool.
fn click(response: &mut Response) {
    response
        .flags
        .insert(egui::response::Flags::FAKE_PRIMARY_CLICKED);
}

fn done(ui: &Ui, taken: &Taken) {
    settle(ui, taken, Ok(()));
}

fn refuse(ui: &Ui, taken: &Taken, sentence: String) {
    settle(ui, taken, Err(sentence));
}

/// The tag of a `Ui` drawn by `withdrawn`: what its owner both hid and disabled.
const WITHDRAWN: &str = "katanaWithdrawn";

/// What a widget its owner hid AND disabled is (setVisible(false) with setEnabled(false)): drawn
/// invisible, as `Ui::add_visible(false, ..)` draws one, and a press or a fill of it is refused as
/// disabled, as the C++'s --press and --fill ask isEnabled before isHidden. An invisible egui `Ui`
/// is always disabled, so only this tag tells such a widget from one that is merely hidden.
pub fn withdrawn<R>(ui: &mut Ui, salt: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(("katanaWithdrawn", salt))
            .invisible()
            .ui_stack_info(egui::UiStackInfo::default().with_tag(WITHDRAWN)),
    );
    add(&mut child)
}

/// Whether `ui` is in one `withdrawn` drew.
fn is_withdrawn(ui: &Ui) -> bool {
    ui.stack().iter().any(|frame| frame.tags().contains(WITHDRAWN))
}

/// A press reaches a control a person could click: refused when its owner hid it, and when it is
/// disabled. The --press step asks disabled first; an invisible egui `Ui` is always disabled too,
/// so hidden is asked first here, and a button both hidden and disabled is refused as hidden -
/// unless its owner drew it `withdrawn`, saying it disabled it too.
pub(crate) fn accept_press(ui: &Ui, taken: &Taken) -> bool {
    if !ui.is_visible() {
        let sentence = if is_withdrawn(ui) {
            press_disabled(&taken.posted)
        } else {
            press_hidden(&taken.posted)
        };
        refuse(ui, taken, sentence);
        return false;
    }
    if !ui.is_enabled() {
        refuse(ui, taken, press_disabled(&taken.posted));
        return false;
    }
    true
}

/// A fill reaches a field a person could type into: refused when it is disabled, whatever its
/// kind, before anything else (fillField: "gridToSurface was disabled and --fill filled it,
/// silently"). A hidden field takes a fill, as Qt's setText fills a hidden QLineEdit; since an
/// invisible egui `Ui` is disabled too, only a visible field is refused as disabled, and one its
/// owner drew `withdrawn`.
pub(crate) fn accept_fill(ui: &Ui, taken: &Taken) -> bool {
    if (ui.is_visible() && !ui.is_enabled()) || is_withdrawn(ui) {
        refuse(ui, taken, fill_disabled(&taken.posted));
        return false;
    }
    true
}

/// A fill a control of this kind takes no text by: the C++'s fall-through.
fn refuse_fill(ui: &Ui, taken: &Taken, name: &str, text: &str) {
    if accept_fill(ui, taken) {
        refuse(ui, taken, no_field(name, text));
    }
}

/// After drawing: a scroll into view brings the control into its scroll area's viewport, as
/// QScrollArea::ensureWidgetVisible does, with no margin.
fn scroll_if_asked(ui: &Ui, taken: Option<&Taken>, response: &Response) {
    if let Some(taken) = taken
        && taken.posted.act == Act::ScrollTo
    {
        response.scroll_to_me(None);
        done(ui, taken);
    }
}

/// The record of a control as the `Ui` it is drawn in leaves it.
fn base_record(ui: &Ui, name: &str, kind: ControlKind) -> ControlRecord {
    let mut record = ControlRecord::new(name, kind);
    record.enabled = ui.is_enabled();
    record.visible = ui.is_visible();
    record
}

/// A press of a button-like control, and a fill refused.
fn press_button(ui: &Ui, taken: Option<&Taken>, name: &str, response: &mut Response) {
    let Some(taken) = taken else {
        return;
    };
    match &taken.posted.act {
        Act::Press => {
            if accept_press(ui, taken) {
                click(response);
                done(ui, taken);
            }
        }
        Act::Fill(text) => refuse_fill(ui, taken, name, text),
        Act::ScrollTo => {}
    }
}

// ---- buttons ---------------------------------------------------------------------------------

/// QPushButton (and QToolButton): `text` on it.
pub fn button(ui: &mut Ui, name: &str, text: &str) -> Response {
    button_as(ui, name, text, egui::Button::new(text))
}

/// A dialog's primary action (Run, Import, Compute): `button` drawn in the accent with the text
/// on the accent - one to a dialog, last in its row of buttons (shell::dialog::button_row).
///
/// A primary with nothing to act on is GREYED, never lit: a dialog draws it in
/// `ui.add_enabled_ui(can_run, |ui| primary(..))`, and in a disabled `Ui` it takes the look of any
/// disabled button (`primary_look` with `enabled` false) - an accent fill on a button that does
/// nothing invites the click (Volumes' Measure and Utilities' Run beside a status saying there was
/// nothing to run, the third look audit's A). A press of it is refused as disabled.
pub fn primary(ui: &mut Ui, name: &str, text: &str) -> Response {
    let look = primary_look(text, ui.is_enabled());
    button_as(ui, name, text, look)
}

/// The look of a primary button, for `button_as` and a caller's own `ui.add`.
pub fn primary_button(text: &str) -> egui::Button<'static> {
    primary_look(text, true)
}

/// The look of a primary button showing `text`: in the accent while `enabled`, else as any button
/// is disabled - an accent fill on a button that does nothing would invite the click.
pub fn primary_look(text: &str, enabled: bool) -> egui::Button<'static> {
    if !enabled {
        return egui::Button::new(String::from(text));
    }
    egui::Button::new(
        egui::RichText::new(text).color(crate::theme::palette::accent_text().to_egui()),
    )
    .fill(crate::theme::palette::accent().to_egui())
    .stroke(egui::Stroke::NONE)
}

/// A button drawn as `button` - an icon, a frame of its own - with `text` the name a screen reader
/// gives it, which is what it reports. In a dialog's footer or a row of its buttons it is
/// shell::dialog::FOOTER_BUTTON_WIDTH wide at least, so the row's buttons share one width.
pub fn button_as(ui: &mut Ui, name: &str, text: &str, button: egui::Button<'_>) -> Response {
    let taken = take(ui, name);
    let button = if crate::shell::dialog::in_footer(ui) {
        button.min_size(egui::vec2(
            crate::shell::dialog::FOOTER_BUTTON_WIDTH,
            0.0,
        ))
    } else {
        button
    };
    let mut response = ui.add(button);
    press_button(ui, taken.as_ref(), name, &mut response);
    scroll_if_asked(ui, taken.as_ref(), &response);
    let mut record = base_record(ui, name, ControlKind::Button);
    record.text = String::from(text);
    super::registry::record(ui, &response, record);
    response
}

/// A checkable button (a chip, a toolbar toggle): drawn checked or not; a click - a person's or a
/// press - is `clicked()`, and the caller changes what it shows, as an exclusive group does.
pub fn toggle(ui: &mut Ui, name: &str, text: &str, checked: bool) -> Response {
    let taken = take(ui, name);
    let mut response = ui.add(egui::Button::selectable(checked, text));
    press_button(ui, taken.as_ref(), name, &mut response);
    scroll_if_asked(ui, taken.as_ref(), &response);
    let mut record = base_record(ui, name, ControlKind::Toggle);
    record.text = String::from(text);
    record.checked = Some(checked);
    super::registry::record(ui, &response, record);
    response
}

/// QCheckBox over `on`: a press toggles it, a fill takes "on" or "off".
pub fn check(ui: &mut Ui, name: &str, text: &str, on: &mut bool) -> Response {
    let taken = take(ui, name);
    let mut clicked = false;
    let mut changed = false;
    if let Some(taken) = &taken {
        match &taken.posted.act {
            Act::Press => {
                if accept_press(ui, taken) {
                    *on = !*on;
                    clicked = true;
                    changed = true;
                    done(ui, taken);
                }
            }
            Act::Fill(text) => {
                if accept_fill(ui, taken) {
                    if text == "on" || text == "off" {
                        let wanted = text == "on";
                        changed = *on != wanted;
                        *on = wanted;
                        done(ui, taken);
                    } else {
                        refuse(ui, taken, no_field(name, text));
                    }
                }
            }
            Act::ScrollTo => {}
        }
    }
    let mut response = check_box(ui, on, text);
    if clicked {
        click(&mut response);
    }
    if changed {
        response.mark_changed();
    }
    scroll_if_asked(ui, taken.as_ref(), &response);
    let mut record = base_record(ui, name, ControlKind::Check);
    record.text = String::from(text);
    record.value = String::from(if *on { "on" } else { "off" });
    record.checked = Some(*on);
    super::registry::record(ui, &response, record);
    response
}

/// QRadioButton: drawn chosen or not; a click - a person's or a press - is `clicked()`, and the
/// caller makes it the one chosen.
pub fn radio(ui: &mut Ui, name: &str, text: &str, chosen: bool) -> Response {
    let taken = take(ui, name);
    let mut response = radio_button(ui, chosen, text);
    press_button(ui, taken.as_ref(), name, &mut response);
    scroll_if_asked(ui, taken.as_ref(), &response);
    let mut record = base_record(ui, name, ControlKind::Radio);
    record.text = String::from(text);
    record.checked = Some(chosen);
    super::registry::record(ui, &response, record);
    response
}

/// egui's check box over `on`, in the window's look (`check_look`), with no name of its own: for
/// a box a list draws in its rows (a layer's, a type's), where `check` is for a form's control.
/// A click toggles it, as egui's does.
pub fn check_box(ui: &mut Ui, on: &mut bool, text: impl Into<egui::WidgetText>) -> Response {
    let text = text.into();
    ui.scope(|ui| {
        check_look(ui, *on, false);
        ui.add(egui::Checkbox::new(on, text))
    })
    .inner
}

/// egui's radio button, `chosen` or not, in the window's look (`check_look`), with no name of its
/// own (`radio` is the named one).
pub fn radio_button(ui: &mut Ui, chosen: bool, text: impl Into<egui::WidgetText>) -> Response {
    let text = text.into();
    ui.scope(|ui| {
        check_look(ui, chosen, true);
        ui.add(egui::RadioButton::new(chosen, text))
    })
    .inner
}

/// The look of a check box or a radio button (`radio`) that is `on`, set on the `Ui` it alone is
/// drawn in: egui draws both from the widget's interaction state alone - the same raised grey
/// whether checked or not, a thin tick in the text colour and a radio's dot 5 px across - so on
/// read as off at a glance. On, the box or disc is the accent with a mark on it in the text drawn
/// on the accent (palette::accent_text, 5.8:1 against it); off, it is a field's well with its
/// border. The label keeps the text colour either way: egui takes a label's colour from the same
/// visuals the box is painted with.
fn check_look(ui: &mut Ui, on: bool, radio: bool) {
    use crate::theme::palette;
    let text = palette::text().to_egui();
    let visuals = ui.visuals_mut();
    visuals.override_text_color = Some(text);
    let widgets = &mut visuals.widgets;
    // A disabled control is drawn with egui's noninteractive look, faded as a whole by its Ui.
    for (state, lit) in [
        (&mut widgets.noninteractive, false),
        (&mut widgets.inactive, false),
        (&mut widgets.hovered, true),
        (&mut widgets.active, true),
    ] {
        if on {
            let ground = if lit {
                palette::accent_hover()
            } else {
                palette::accent()
            }
            .to_egui();
            state.bg_fill = ground;
            state.weak_bg_fill = ground;
            state.bg_stroke = egui::Stroke::new(1.0, ground);
            // The tick is two pixels wide: at egui's one it was a hairline on a 16 px box.
            state.fg_stroke = egui::Stroke::new(2.0, palette::accent_text().to_egui());
        } else {
            state.bg_fill = palette::input().to_egui();
            state.weak_bg_fill = state.bg_fill;
        }
    }
    // The mark's square inside the 16 px box: a tick across 10 px of it, where egui's 8 drew a
    // short one; a radio's dot is a third of it across each way of its centre, 4 + 4 = 8 px in
    // the 16 px disc, where egui's was 5.
    ui.spacing_mut().icon_width_inner = if radio { 12.0 } else { 10.0 };
}

// ---- text ------------------------------------------------------------------------------------

/// How a one-line field is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LineOptions<'a> {
    /// QLineEdit::setPlaceholderText.
    pub placeholder: &'a str,
    /// QLineEdit::setReadOnly: shown, not typed into (a fill still writes it, as setText does).
    pub read_only: bool,
    /// QLineEdit::Password.
    pub password: bool,
    /// The width it asks for; None, egui's.
    pub width: Option<f32>,
}

/// QLineEdit over `value`.
pub fn line(ui: &mut Ui, name: &str, value: &mut String) -> Response {
    line_with(ui, name, value, LineOptions::default())
}

/// How far a field's text stands in from its left edge: a choice's (egui's ComboBox puts its text
/// a button's padding, theme::style::GROUP_GAP, in from its edge), so a column of fields and
/// choices reads down one edge. egui's TextEdit stands its text 4 in.
pub const FIELD_INSET: f32 = crate::theme::style::GROUP_GAP;

/// A one-line field's margin round its text: FIELD_INSET across, less the pixel of border egui
/// draws inside it (TextEdit takes the stroke's width off its margin); 2 down, egui's own, the
/// text being centred in the control's height anyway. Public for the fields a table draws in a
/// cell of its own (dialog_layout::field_look), which wear the same look.
pub fn field_margin() -> egui::Margin {
    egui::Margin::symmetric(FIELD_INSET as i8, 2)
}

/// The room an editable choice's list arrow takes at its right end, inside its frame: a square a
/// control high (theme::style::CONTROL_HEIGHT). The arrow was once a ComboBox of its own beside
/// the field, 38 wide (an empty ComboBox's icon, its spacing and its padding), and the two read
/// as two controls; one frame round both is one control, as a person reads it.
pub fn list_arrow_width(ui: &Ui) -> f32 {
    ui.spacing().interact_size.y
}

/// What `editable_choice_frame` drew.
pub struct EditableChoiceShown<R> {
    /// The field typed into.
    pub field: Response,
    /// The list's arrow at the control's right end.
    pub arrow: Response,
    /// What the list's contents gave, while it is open.
    pub list: Option<R>,
}

impl<R> EditableChoiceShown<R> {
    /// The whole control: the field and its arrow, under the field's id (what a driver and the
    /// registry take for the control).
    pub fn whole(&self) -> Response {
        self.field.union(self.arrow.clone())
    }
}

/// An editable choice's look and behaviour without its record (QComboBox::setEditable): ONE framed
/// control as wide as a field (`Spacing::text_edit_width`, held to the row), the field typed into
/// at its left and the list's arrow inside its right end (`list_arrow_width`); a click on the
/// arrow opens the list under the whole control, its rows drawn by `list`, scrolling past
/// `Spacing::combo_height` as a choice's list does. `salt` names the field and the list, so the
/// state of each is kept from one frame to the next. `placeholder` is the field's hint, and
/// `text_color` its text's colour when it has one of its own (a name nothing defines).
///
/// Used by `choice` for an editable choice and by the shared name picker
/// (customisation::name_picker), which draw the same control: one painter for both.
pub fn editable_choice_frame<R>(
    ui: &mut Ui,
    salt: &str,
    text: &mut String,
    placeholder: &str,
    text_color: Option<egui::Color32>,
    list: impl FnOnce(&mut Ui) -> R,
) -> EditableChoiceShown<R> {
    let height = ui.spacing().interact_size.y;
    let arrow_width = list_arrow_width(ui);
    let width = ui
        .spacing()
        .text_edit_width
        .min(ui.available_width())
        .max(2.0 * arrow_width);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    // The frame is painted under the field, once the field has said whether it has the keys.
    let background = ui.painter().add(egui::Shape::Noop);
    let field_rect =
        egui::Rect::from_min_max(rect.min, egui::pos2(rect.right() - arrow_width, rect.bottom()));
    let arrow_rect =
        egui::Rect::from_min_max(egui::pos2(rect.right() - arrow_width, rect.top()), rect.max);
    // The field draws no frame of its own; its text stands FIELD_INSET in, as a field's does, and
    // the pixel of border the frame below draws round it is counted in that.
    let inset = FIELD_INSET as i8 - 1;
    // A placeholder that names the empty value in brackets ("(no symbol)", the name picker's) is
    // shown in the window's one spelling of it, as a choice's empty item is (`empty_item_words`).
    let placeholder = empty_item_words(placeholder).unwrap_or_else(|| String::from(placeholder));
    let mut edit = egui::TextEdit::singleline(text)
        .id_salt(salt)
        .hint_text(placeholder)
        .frame(egui::Frame::new().inner_margin(egui::Margin {
            left: inset,
            right: 2,
            top: 0,
            bottom: 0,
        }))
        .vertical_align(egui::Align::Center)
        .desired_width(field_rect.width())
        .min_size(field_rect.size());
    if let Some(color) = text_color {
        edit = edit.text_color(color);
    }
    let field = ui.place(field_rect, edit);
    let arrow_id = ui.make_persistent_id((salt, "items"));
    let arrow = ui.interact(arrow_rect, arrow_id, egui::Sense::click());
    let popup_id = arrow_id.with("popup");
    // Opened under the whole control, as wide as it, not under the arrow alone.
    let mut anchor = arrow.clone().with_new_rect(rect);
    anchor.interact_rect = rect;
    let shown = egui::Popup::menu(&arrow)
        .id(popup_id)
        .anchor(&anchor)
        .width(rect.width())
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        // A choice's list's look (egui's ComboBox keeps the style it is drawn in), not a menu's.
        .style(egui::style::StyleModifier::default())
        .show(|ui| {
            ui.set_min_width(ui.available_width());
            egui::ScrollArea::vertical()
                .max_height(ui.spacing().combo_height)
                .show(ui, |ui| {
                    // Rows are not wrapped to a narrow control: the list widens to its longest.
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    list(ui)
                })
                .inner
        })
        .map(|shown| shown.inner);
    let open = egui::Popup::is_id_open(ui.ctx(), popup_id);
    if ui.is_rect_visible(rect) {
        let visuals = ui.visuals();
        let hovered = field.hovered() || arrow.hovered();
        // A field's frame: the border lit under the pointer, the field's focus colour while it
        // has the keys or its list is open (egui's TextEdit frames a focused field so).
        let stroke = if field.has_focus() || open {
            visuals.selection.stroke
        } else if hovered {
            visuals.widgets.hovered.bg_stroke
        } else {
            visuals.widgets.inactive.bg_stroke
        };
        let radius = visuals.widgets.inactive.corner_radius;
        let fill = visuals.text_edit_bg_color();
        let arrow_ground = visuals.widgets.hovered.weak_bg_fill;
        let ink = if arrow.hovered() || open {
            crate::theme::palette::text()
        } else {
            crate::theme::palette::text_muted()
        }
        .to_egui();
        let painter = ui.painter();
        painter.set(
            background,
            egui::epaint::RectShape::new(rect, radius, fill, stroke, egui::StrokeKind::Inside),
        );
        if arrow.hovered() || open {
            // The arrow's own square shaded under the pointer, inside the border, so the part a
            // click opens the list from is seen.
            let corner = radius.ne.saturating_sub(1);
            painter.rect_filled(
                arrow_rect.shrink(1.0),
                egui::CornerRadius {
                    nw: 0,
                    sw: 0,
                    ne: corner,
                    se: corner,
                },
                arrow_ground,
            );
        }
        paint_list_arrow(painter, arrow_rect, ink);
    }
    EditableChoiceShown {
        field,
        arrow,
        list: shown,
    }
}

/// The down chevron of a list's arrow, centred in `rect`: 8 px across and 4 deep, a stroke of 1.5
/// in `ink` - the icons' weight at a menu's size (icons.rs's STROKE at 16 px).
pub(crate) fn paint_list_arrow(painter: &egui::Painter, rect: egui::Rect, ink: egui::Color32) {
    let centre = rect.center();
    painter.add(egui::Shape::line(
        vec![
            egui::pos2(centre.x - 4.0, centre.y - 2.0),
            egui::pos2(centre.x, centre.y + 2.0),
            egui::pos2(centre.x + 4.0, centre.y - 2.0),
        ],
        egui::Stroke::new(1.5, ink),
    ));
}

/// Whether the list of the editable choice `salt` drawn in `ui` is open (`editable_choice_frame`):
/// for a list whose rows cost something to make, made only while it shows.
pub fn editable_choice_is_open(ui: &Ui, salt: &str) -> bool {
    egui::Popup::is_id_open(
        ui.ctx(),
        ui.make_persistent_id((salt, "items")).with("popup"),
    )
}

/// A tree row's expander (a QTreeView branch's arrow): egui's collapsing triangle, pointing right
/// while the row is closed and down while it is open, in an EXPANDER_WIDTH cell a control high;
/// `None` for a row that has nothing under it, an empty cell, so the texts of a level line up.
/// The fonts the window carries have no ▸ or ▾ (U+25B8, U+25BE): drawn as text, every tree's
/// expanders were boxes. A click on it is `clicked()`; the tree opens or closes the row.
pub fn tree_expander(ui: &mut Ui, open: Option<bool>) -> Response {
    let size = egui::vec2(EXPANDER_WIDTH, ui.spacing().interact_size.y);
    let Some(open) = open else {
        return ui.allocate_exact_size(size, egui::Sense::hover()).1;
    };
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        // The triangle in a square of EXPANDER_MARK, muted at rest and the text colour under the
        // pointer: an expander is a quiet thing beside the name it opens.
        let mark = response
            .clone()
            .with_new_rect(egui::Rect::from_center_size(
                rect.center(),
                egui::Vec2::splat(EXPANDER_MARK),
            ));
        // Painted from a Ui of its own over the cell, which takes no room from the row.
        let mut painter_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        let muted = crate::theme::palette::text_muted().to_egui();
        painter_ui.visuals_mut().widgets.inactive.fg_stroke.color = muted;
        painter_ui.visuals_mut().widgets.noninteractive.fg_stroke.color = muted;
        let openness = if open { 1.0 } else { 0.0 };
        egui::collapsing_header::paint_default_icon(&mut painter_ui, openness, &mark);
    }
    response
}

/// The width of a tree row's expander cell (`tree_expander`): a menu icon's (16).
pub const EXPANDER_WIDTH: f32 = 16.0;

/// The square egui's triangle is drawn in, three quarters of it inked: 8 of the cell's 16, a
/// triangle 6 px a side - the size of the ▸ it replaces at the window's 13 px text.
const EXPANDER_MARK: f32 = 8.0;

/// QLineEdit over `value`, drawn as `options` say: a fill writes the text as given.
pub fn line_with(
    ui: &mut Ui,
    name: &str,
    value: &mut String,
    options: LineOptions<'_>,
) -> Response {
    let taken = take(ui, name);
    let mut changed = false;
    if let Some(taken) = &taken {
        match &taken.posted.act {
            Act::Fill(text) => {
                if accept_fill(ui, taken) {
                    changed = *value != *text;
                    value.clone_from(text);
                    done(ui, taken);
                }
            }
            Act::Press => refuse(ui, taken, no_button(&taken.posted)),
            Act::ScrollTo => {}
        }
    }
    // A control's height (theme::style::CONTROL_HEIGHT): egui sizes a one-line field to its text
    // (19 points at 13 px), and beside the 24-point buttons of its row - a Browse, a Measure - it
    // stood shorter than they did. Its text centred in that height and FIELD_INSET in from its
    // left, as a choice's is (field_margin).
    let mut edit = egui::TextEdit::singleline(value)
        .id_salt(name)
        .hint_text(String::from(options.placeholder))
        .password(options.password)
        .interactive(!options.read_only)
        .margin(field_margin())
        .vertical_align(egui::Align::Center)
        .min_size(egui::vec2(0.0, ui.spacing().interact_size.y));
    if let Some(width) = options.width {
        edit = edit.desired_width(width);
    }
    let mut response = ui.add(edit);
    if !options.password {
        field_swatch(ui, &response, value);
    }
    if changed {
        response.mark_changed();
    }
    scroll_if_asked(ui, taken.as_ref(), &response);
    let mut record = base_record(ui, name, ControlKind::Line);
    record.text = String::from(options.placeholder);
    record.value = value.clone();
    super::registry::record(ui, &response, record);
    response
}

/// A number typed as text (QLineEdit) over `value`: a field `line_with` draws, as wide as a number
/// box (`spin`, `whole`) showing it would be - its text, or `placeholder` while it is empty, and
/// FIELD_INSET either side, NUMBER_FIELD_WIDTH at least - rather than filling its row. For a
/// number a box cannot hold: one that may be absent (absent is not zero), or that a dialog reads
/// and refuses with its own sentence. Drawn filling the row, a typed number stood beside the short
/// wells of the boxes in one form (Contours' Interval beside its Major every, Smooth's Weight
/// beside its Passes; the third look audit's C), so a number is one short well whichever it is.
pub fn number_line(ui: &mut Ui, name: &str, value: &mut String, placeholder: &str) -> Response {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let shown = if value.is_empty() {
        placeholder
    } else {
        value.as_str()
    };
    let text = ui
        .painter()
        .layout_no_wrap(String::from(shown), font, egui::Color32::PLACEHOLDER)
        .size()
        .x;
    // A caret's room past the text, as a field being typed into needs it.
    let width = (text + 2.0 * FIELD_INSET + CARET_ROOM)
        .max(NUMBER_FIELD_WIDTH)
        .min(ui.available_width().max(NUMBER_FIELD_WIDTH));
    line_with(
        ui,
        name,
        value,
        LineOptions {
            placeholder,
            width: Some(width),
            ..LineOptions::default()
        },
    )
}

/// The room `number_line` keeps after its text for the caret: egui's text cursor is two points
/// wide (Visuals::text_cursor's stroke).
const CARET_ROOM: f32 = 2.0;

// ---- colours ----------------------------------------------------------------------------------

/// The side of a colour's swatch (`colour_swatch`): a menu icon's 16 less two either side, so a
/// swatch sits inside a line of text at the Standard size.
pub const SWATCH_SIZE: f32 = 12.0;

/// The colour `text` names when it is a colour as the window writes one - #RRGGBB, or #RRGGBBAA
/// when it is not opaque (EntityColor::to_hex) - and nothing else: a name ("ByLayer", "red") is a
/// word, not a colour to paint.
pub fn colour_of_text(text: &str) -> Option<egui::Color32> {
    let digits = text.trim().strip_prefix('#')?;
    if !(digits.len() == 6 || digits.len() == 8) || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();
    let alpha = if digits.len() == 8 { byte(6)? } else { u8::MAX };
    Some(egui::Color32::from_rgba_unmultiplied(
        byte(0)?,
        byte(2)?,
        byte(4)?,
        alpha,
    ))
}

/// Paints a colour's swatch over `rect`: the colour, under a pixel of the border token, so a
/// colour as dark as the panel (#000000, the window's own grey) is still seen as a swatch.
pub fn paint_swatch(painter: &egui::Painter, rect: egui::Rect, colour: egui::Color32) {
    painter.rect(
        rect,
        egui::CornerRadius::same(2),
        colour,
        egui::Stroke::new(1.0, crate::theme::palette::border().to_egui()),
        egui::StrokeKind::Inside,
    );
}

/// A colour's swatch, SWATCH_SIZE square, in a cell a line of text high: what is drawn beside a
/// colour written as text, so the colour is seen, not read as six digits (Layers, Surface Styles,
/// Text Styles showed "#3CA03C" alone; the third look audit's H).
pub fn colour_swatch(ui: &mut Ui, colour: egui::Color32) -> Response {
    let height = ui.text_style_height(&egui::TextStyle::Body).max(SWATCH_SIZE);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(SWATCH_SIZE, height), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let square = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(SWATCH_SIZE));
        paint_swatch(ui.painter(), square, colour);
    }
    response
}

/// A colour shown as its text (`hex`, as the window writes one) with its swatch before it, the
/// text a label named `name` (`label`, which a driver reads): what a form or a summary shows a
/// colour with. A text that is not a colour (ByLayer) is shown alone.
pub fn colour_value(ui: &mut Ui, name: &str, hex: &str) -> Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = crate::theme::style::GAP;
        if let Some(colour) = colour_of_text(hex) {
            colour_swatch(ui, colour);
        }
        label(ui, name, hex)
    })
    .inner
}

/// A one-line field holding a colour (`colour_of_text`) shows its swatch inside its right end, so
/// every colour field - Layers' Colour, Surface Styles' seven, the scope's Colour filter once a
/// colour is typed - shows the colour it holds without a helper of its own. At the right end, not
/// before the text: the text would move as the field became a colour while being typed. Not drawn
/// where the field is too narrow for its text and the swatch both.
fn field_swatch(ui: &Ui, field: &Response, value: &str) {
    let Some(colour) = colour_of_text(value) else {
        return;
    };
    let text = ui
        .painter()
        .layout_no_wrap(
            String::from(value),
            egui::TextStyle::Body.resolve(ui.style()),
            egui::Color32::PLACEHOLDER,
        )
        .size()
        .x;
    let rect = field.rect;
    let needed = FIELD_INSET + text + crate::theme::style::GROUP_GAP + SWATCH_SIZE + FIELD_INSET;
    if rect.width() < needed || !ui.is_rect_visible(rect) {
        return;
    }
    let square = egui::Rect::from_center_size(
        egui::pos2(rect.right() - FIELD_INSET - SWATCH_SIZE / 2.0, rect.center().y),
        egui::Vec2::splat(SWATCH_SIZE),
    );
    paint_swatch(ui.painter(), square, colour);
}

/// How a text box is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextOptions {
    /// QPlainTextEdit::setReadOnly: a reply box.
    pub read_only: bool,
    /// The rows it shows at least.
    pub rows: usize,
    /// Fixed pitch, as the logs are.
    pub monospace: bool,
}

impl Default for TextOptions {
    fn default() -> Self {
        Self {
            read_only: false,
            rows: 4,
            monospace: false,
        }
    }
}

/// QPlainTextEdit over `value`: a fill's "\n" (a backslash and an n) is a line break, so a field
/// book fits on a command line (fillField).
pub fn text(ui: &mut Ui, name: &str, value: &mut String) -> Response {
    text_with(ui, name, value, TextOptions::default())
}

/// QPlainTextEdit over `value`, drawn as `options` say.
pub fn text_with(ui: &mut Ui, name: &str, value: &mut String, options: TextOptions) -> Response {
    let taken = take(ui, name);
    let mut changed = false;
    if let Some(taken) = &taken {
        match &taken.posted.act {
            Act::Fill(text) => {
                if accept_fill(ui, taken) {
                    let written = text.replace("\\n", "\n");
                    changed = *value != written;
                    *value = written;
                    done(ui, taken);
                }
            }
            Act::Press => refuse(ui, taken, no_button(&taken.posted)),
            Act::ScrollTo => {}
        }
    }
    // A text box fills the width it is given: a reply or a field book is read across, and a box
    // narrower than the fields above it left the rest of the dialog empty.
    let mut edit = egui::TextEdit::multiline(value)
        .id_salt(name)
        .desired_rows(options.rows)
        .desired_width(f32::INFINITY)
        .interactive(!options.read_only);
    if options.monospace {
        edit = edit.code_editor();
    }
    let mut response = ui.add(edit);
    if changed {
        response.mark_changed();
    }
    scroll_if_asked(ui, taken.as_ref(), &response);
    let mut record = base_record(ui, name, ControlKind::Text);
    record.value = value.clone();
    super::registry::record(ui, &response, record);
    response
}

/// QLabel: `text`, reported as written, never as markup.
pub fn label(ui: &mut Ui, name: &str, text: &str) -> Response {
    label_as(ui, name, text, egui::Label::new(text))
}

/// A label drawn as `label` - coloured, wrapped - whose text is `text`.
///
/// Beside a footer's buttons (the left part of shell::dialog::button_row) a label is a status, and
/// a status is ONE line however its caller asked for it: cut short with an ellipsis, the whole
/// text in its tooltip (egui's Label shows an elided text whole on hover). Wrapped there, a
/// sentence stood two lines high and made its footer 55 points where every other is 49 (21
/// terrain dialogs; the third look audit's A).
pub fn label_as(ui: &mut Ui, name: &str, text: &str, label: egui::Label) -> Response {
    let taken = take(ui, name);
    let label = if crate::shell::dialog::beside_footer_buttons(ui) {
        label.truncate()
    } else {
        label
    };
    let response = ui.add(label);
    if let Some(taken) = &taken {
        match &taken.posted.act {
            Act::Fill(fill) => refuse_fill(ui, taken, name, fill),
            Act::Press => refuse(ui, taken, no_button(&taken.posted)),
            Act::ScrollTo => {}
        }
    }
    scroll_if_asked(ui, taken.as_ref(), &response);
    let mut record = base_record(ui, name, ControlKind::Label);
    record.text = String::from(text);
    super::registry::record(ui, &response, record);
    response
}

// ---- choices ---------------------------------------------------------------------------------

/// What a QComboBox holds: the item chosen, and for an editable one the text typed, which may be
/// a name the list does not hold (how a picker keeps a name nothing defines).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Choice {
    /// The item chosen (QComboBox::currentIndex); None for none.
    pub index: Option<usize>,
    /// An editable choice's text (currentText); kept the chosen item's when one is chosen.
    pub text: String,
    pub editable: bool,
}

impl Choice {
    /// A choice of the item at `index`.
    pub fn at(index: usize) -> Self {
        Self {
            index: Some(index),
            ..Self::default()
        }
    }

    /// An editable choice showing `text`.
    pub fn editable(text: &str) -> Self {
        Self {
            index: None,
            text: String::from(text),
            editable: true,
        }
    }

    /// QComboBox::currentText.
    pub fn current_text(&self, items: &[String]) -> String {
        if self.editable {
            return self.text.clone();
        }
        self.index
            .and_then(|index| items.get(index))
            .cloned()
            .unwrap_or_default()
    }

    /// Chooses the item at `index`, as a click on it does.
    pub fn choose(&mut self, index: usize, items: &[String]) {
        self.index = Some(index);
        if self.editable
            && let Some(item) = items.get(index)
        {
            self.text.clone_from(item);
        }
    }
}

/// What an empty choice says (`choice`): nothing is chosen. The window's ONE word for empty (the
/// fourth look audit's 3: None, none, (none), (no symbol), (no linetype) and No style were six
/// spellings of it, mixed in one form). A hint that names what is missing is `no_value_hint`'s.
pub const NO_CHOICE: &str = "None";

/// What an empty search field says, in every dialog that filters a list by words typed: one
/// spelling where there were "search: ...", "Search: ..." and "Search ..." (the fourth look
/// audit). What the search reads goes in the field's tooltip, not its placeholder.
pub const SEARCH_HINT: &str = "Search...";

/// A hint that names what is missing: "No symbol", "No linetype", "No surface" for `thing`
/// "symbol", "linetype", "surface" - for `choice_with_hint` and a field's placeholder, so an empty
/// value is said one way, in sentence case, never "(no symbol)" or "none".
pub fn no_value_hint(thing: &str) -> String {
    format!("No {}", thing.trim())
}

/// `text` with its first letter a capital, the rest as it is: the sentence case a choice's shown
/// value takes (`ChoiceOptions::sentence_case`) and a status starts with.
pub fn sentence_start(text: &str) -> String {
    let mut letters = text.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => String::new(),
    }
}

/// The words an item that IS the empty value shows - "(none)" is NO_CHOICE, "(no symbol)" is "No
/// symbol" (`no_value_hint`) - or None for an item that is a value. Only the bracketed forms: an
/// item in brackets is a list's own placeholder for nothing, never a name the drawing holds,
/// where a bare "none" may be a verb's token for a real setting (an arrowhead of none), which
/// sentence case shows as "None" all the same.
pub fn empty_item_words(item: &str) -> Option<String> {
    let inner = item.trim().strip_prefix('(')?.strip_suffix(')')?;
    if inner.eq_ignore_ascii_case("none") {
        return Some(String::from(NO_CHOICE));
    }
    let thing = inner.strip_prefix("no ")?;
    let words = !thing.trim().is_empty()
        && thing
            .chars()
            .all(|letter| letter.is_alphabetic() || letter == ' ');
    words.then(|| no_value_hint(thing))
}

/// How a choice shows `item`: an item that is the empty value (`empty_item_words`) in its words,
/// muted as a hint is; otherwise the item, its first letter a capital when `sentence_case`. What
/// is shown only: a choice records and is filled by the item itself.
pub fn shown_item(item: &str, sentence_case: bool) -> egui::WidgetText {
    // An item of no words (a verb's default, unsaid) is the empty value as a list row too.
    let words = if item.trim().is_empty() {
        Some(String::from(NO_CHOICE))
    } else {
        empty_item_words(item)
    };
    if let Some(words) = words {
        return egui::RichText::new(words)
            .color(crate::theme::palette::text_muted().to_egui())
            .into();
    }
    if sentence_case {
        sentence_start(item).into()
    } else {
        item.into()
    }
}

/// The item a fill of a choice that cannot be typed into chooses: the item that is `text`; else
/// the one item that reads `text` ignoring case, or shows it (`empty_item_words`) - a choice shows
/// its tokens in sentence case, and a driver or a person may write either. None where none does,
/// or where two do, which only the item's own spelling can tell apart.
fn item_filled(items: &[String], text: &str) -> Option<usize> {
    if let Some(at) = items.iter().position(|item| item == text) {
        return Some(at);
    }
    let same = |item: &String| {
        crate::command_log::same_ignoring_case(item, text)
            || empty_item_words(item)
                .is_some_and(|words| crate::command_log::same_ignoring_case(&words, text))
    };
    let mut found = items
        .iter()
        .enumerate()
        .filter(|(_, item)| same(item))
        .map(|(at, _)| at);
    match (found.next(), found.next()) {
        (Some(at), None) => Some(at),
        _ => None,
    }
}

/// How a choice is drawn (`choice_with`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChoiceOptions<'a> {
    /// What it says, muted, while nothing is chosen and it shows no text (`choice_with_hint`).
    pub hint: &'a str,
    /// Its items are a verb's tokens ("both", "round", "auto"), shown in sentence case ("Both",
    /// "Round", "Auto") as every other choice's values are; the item - the token - is still what it
    /// records and what a fill names (in either case). Not for a list of names the drawing holds
    /// (layers, surfaces, styles): a name is shown as it was written.
    pub sentence_case: bool,
}

/// QComboBox over `items`: a fill chooses the item whose text is the text given; an editable one
/// takes a text it does not list, as typing does; any other is refused (fillField). While nothing
/// is chosen and it shows no text it says NO_CHOICE, muted (`choice_with_hint`).
pub fn choice(ui: &mut Ui, name: &str, items: &[String], chosen: &mut Choice) -> Response {
    choice_with_hint(ui, name, items, chosen, NO_CHOICE)
}

/// `choice` over a verb's tokens, shown in sentence case (`ChoiceOptions::sentence_case`), saying
/// NO_CHOICE while nothing is chosen.
pub fn token_choice(ui: &mut Ui, name: &str, items: &[String], chosen: &mut Choice) -> Response {
    choice_with(
        ui,
        name,
        items,
        chosen,
        ChoiceOptions {
            hint: NO_CHOICE,
            sentence_case: true,
        },
    )
}

/// `choice`, saying `hint` - muted, in the control - while nothing is chosen and it shows no text:
/// what is missing, in the caller's words ("No surface", "No style"). An empty choice was an empty
/// well, read as a control that failed to fill (Thin's and Smooth's Surface, Global Modify's Layer
/// and Style; the third look audit's I). The hint is the control's look only: its recorded value
/// is still "", as a driver and the tests read it, and a fill of the hint's words is refused as any
/// text the list does not hold. An empty `hint` says nothing.
///
/// A hint, not a field of `Choice`: a dialog's Choice is plain data, written out whole in several
/// dialogs, and what an empty one says is how it is drawn.
pub fn choice_with_hint(
    ui: &mut Ui,
    name: &str,
    items: &[String],
    chosen: &mut Choice,
    hint: &str,
) -> Response {
    choice_with(
        ui,
        name,
        items,
        chosen,
        ChoiceOptions {
            hint,
            sentence_case: false,
        },
    )
}

/// `choice` drawn as `options` say: its hint, and whether its items are shown in sentence case.
///
/// What every choice shows, whatever its options: an item that IS the empty value - "(none)",
/// "(no symbol)" - in the window's one empty word, muted (`empty_item_words`), so a list's own
/// placeholder reads as the hint of an empty choice does. A fill of a choice that cannot be typed
/// into takes the item's own text, or the one item reading it in another case or showing it
/// (`item_filled`); its record keeps the items and the chosen item as they are.
pub fn choice_with(
    ui: &mut Ui,
    name: &str,
    items: &[String],
    chosen: &mut Choice,
    options: ChoiceOptions<'_>,
) -> Response {
    let hint = options.hint;
    let sentence_case = options.sentence_case;
    let taken = take(ui, name);
    let mut changed = false;
    if let Some(taken) = &taken {
        match &taken.posted.act {
            Act::Fill(text) => {
                if accept_fill(ui, taken) {
                    // An editable choice takes a text in another case as a name typed, as typing
                    // it does; one that cannot be typed into reads it as its item.
                    let found = if chosen.editable {
                        items.iter().position(|item| item == text)
                    } else {
                        item_filled(items, text)
                    };
                    if let Some(index) = found {
                        let before = chosen.clone();
                        chosen.choose(index, items);
                        changed = *chosen != before;
                        done(ui, taken);
                    } else if chosen.editable {
                        changed = chosen.text != *text;
                        chosen.text.clone_from(text);
                        done(ui, taken);
                    } else {
                        refuse(ui, taken, no_choice(name, text));
                    }
                }
            }
            Act::Press => refuse(ui, taken, no_button(&taken.posted)),
            Act::ScrollTo => {}
        }
    }
    let mut picked = None;
    let mut response = if chosen.editable {
        // One control, as wide as a field alone: the field and the list's arrow inside its end.
        let index = chosen.index;
        // The field's own hint is drawn while it is empty: the editable choice's empty state.
        editable_choice_frame(ui, name, &mut chosen.text, hint, None, |ui| {
            for (at, item) in items.iter().enumerate() {
                if ui
                    .selectable_label(index == Some(at), shown_item(item, sentence_case))
                    .clicked()
                {
                    picked = Some(at);
                }
            }
        })
        .whole()
    } else {
        let current = chosen.current_text(items);
        let index = chosen.index;
        // Nothing to show - nothing chosen, or an item chosen whose text is "" (DEM Tools'
        // Resolution, Strategy and Resampling, whose first item is the verb's default, unsaid):
        // the hint. A blank well read as a control that failed to fill, chosen or not.
        let shown: egui::WidgetText = if current.is_empty() && !hint.is_empty() {
            egui::RichText::new(empty_item_words(hint).unwrap_or_else(|| String::from(hint)))
                .color(crate::theme::palette::text_muted().to_egui())
                .into()
        } else {
            shown_item(&current, sentence_case)
        };
        egui::ComboBox::from_id_salt(name)
            .selected_text(shown)
            // The editable choice's chevron (paint_list_arrow), so a choice reads the same
            // whether it can be typed into or not; egui's own is a filled triangle.
            .icon(|ui, rect, _, open| {
                let ink = if open {
                    crate::theme::palette::text()
                } else {
                    crate::theme::palette::text_muted()
                };
                paint_list_arrow(ui.painter(), rect, ink.to_egui());
            })
            .show_ui(ui, |ui| {
                for (at, item) in items.iter().enumerate() {
                    if ui
                        .selectable_label(index == Some(at), shown_item(item, sentence_case))
                        .clicked()
                    {
                        picked = Some(at);
                    }
                }
            })
            .response
    };
    if let Some(at) = picked
        && chosen.index != Some(at)
    {
        chosen.choose(at, items);
        changed = true;
    }
    if changed {
        response.mark_changed();
    }
    scroll_if_asked(ui, taken.as_ref(), &response);
    let mut record = base_record(ui, name, ControlKind::Choice);
    record.value = chosen.current_text(items);
    record.items = items.to_vec();
    super::registry::record(ui, &response, record);
    response
}

/// A QDoubleSpinBox's range and look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpinRange {
    pub minimum: f64,
    pub maximum: f64,
    /// QAbstractSpinBox::singleStep.
    pub step: f64,
    /// QDoubleSpinBox::decimals: what it shows and what a value is rounded to.
    pub decimals: usize,
    /// QDoubleSpinBox::suffix (" m").
    pub suffix: &'static str,
    /// QAbstractSpinBox::specialValueText: what the box reads at its minimum, in place of
    /// the number and its suffix ("Auto"). Empty for none.
    pub special: &'static str,
}

impl SpinRange {
    pub const fn new(minimum: f64, maximum: f64, step: f64, decimals: usize) -> Self {
        Self {
            minimum,
            maximum,
            step,
            decimals,
            suffix: "",
            special: "",
        }
    }

    pub const fn with_suffix(self, suffix: &'static str) -> Self {
        Self { suffix, ..self }
    }

    pub const fn with_special_text(self, special: &'static str) -> Self {
        Self { special, ..self }
    }

    /// QAbstractSpinBox::text(): the special text at the minimum, else the number as the
    /// box rounds it, with no suffix (what the records have always carried).
    pub fn shown(&self, value: f64) -> String {
        if !self.special.is_empty() && value == self.minimum {
            String::from(self.special)
        } else {
            crate::format::qt_fixed(value, self.decimals)
        }
    }

    /// QDoubleSpinBox's rounding of a value to its decimals: the number as it would show it, read
    /// back.
    pub fn rounded(&self, value: f64) -> f64 {
        crate::format::qt_fixed(value, self.decimals)
            .parse()
            .unwrap_or(value)
    }
}

/// The least width of a number box (`spin`, `whole`): half a short field
/// (shell::dialog::SHORT_FIELD_WIDTH), what holds a station, a percentage or a size with its unit
/// ("2.50 mm"). egui's least is a control's 40, a box the size of a key.
pub const NUMBER_FIELD_WIDTH: f32 = crate::shell::dialog::SHORT_FIELD_WIDTH / 2.0;

/// A number box's look, set on the `Ui` it alone is drawn in: a field's well (the input token
/// under the field's border, lit under the pointer and framed in the accent while dragged) and
/// its text FIELD_INSET in, where a one-line field's is. egui draws a DragValue as a Button - the
/// raised grey of a key, its text centred - so every number box of every form read as a button
/// beside the fields round it (the second look audit's N4). Public for a number box drawn without
/// `spin` or `whole` (the sheet editor's own).
pub fn number_field_look(ui: &mut Ui) {
    let well = ui.visuals().text_edit_bg_color();
    let widgets = &mut ui.visuals_mut().widgets;
    for state in [
        &mut widgets.noninteractive,
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
    ] {
        state.bg_fill = well;
        state.weak_bg_fill = well;
        state.expansion = 0.0;
    }
    ui.spacing_mut().button_padding.x = FIELD_INSET;
}

/// Draws `drag`, which shows `shown`, as a field (`number_field_look`): a control high, its text
/// centred in that height and FIELD_INSET in from its left, as wide as its text and its insets and
/// NUMBER_FIELD_WIDTH at least. egui sizes a DragValue to the spacing's interact width and lays
/// its text out by the layout it is drawn in, so the box is drawn in a cell of its own, left to
/// right with its contents at the left and centred down - in a row whose items sit at its top (a
/// form's) its text stood at the top of the box, and in egui's own row it stood in the middle.
fn number_field(ui: &mut Ui, shown: &str, drag: egui::DragValue<'_>) -> Response {
    let font = ui.style().drag_value_text_style.resolve(ui.style());
    let text = ui
        .painter()
        .layout_no_wrap(String::from(shown), font, egui::Color32::PLACEHOLDER)
        .size()
        .x;
    let width = (text + 2.0 * FIELD_INSET).max(NUMBER_FIELD_WIDTH);
    let height = ui.spacing().interact_size.y;
    ui.scope(|ui| {
        number_field_look(ui);
        // The width the DragValue takes as its least, both as a box and while typed into.
        ui.spacing_mut().interact_size.x = width;
        // Its text at the left: egui's left-to-right layout centres what it holds along the row
        // ("to center text within a button"), and a button takes its text's place from it.
        ui.allocate_ui_with_layout(
            egui::vec2(width, height),
            egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Min),
            |ui| ui.add(drag),
        )
        .inner
    })
    .inner
}

/// QDoubleSpinBox over `value`: a fill takes a number in the range, read as the box reads typed
/// text (the C locale, whitespace round it ignored), rounded to its decimals as setValue rounds.
pub fn spin(ui: &mut Ui, name: &str, value: &mut f64, range: SpinRange) -> Response {
    let taken = take(ui, name);
    let mut changed = false;
    if let Some(taken) = &taken {
        match &taken.posted.act {
            Act::Fill(text) => {
                if accept_fill(ui, taken) {
                    match katana_core::text::parse_finite_double(text.trim()) {
                        Some(number) if (range.minimum..=range.maximum).contains(&number) => {
                            let rounded = range.rounded(number);
                            changed = *value != rounded;
                            *value = rounded;
                            done(ui, taken);
                        }
                        _ => refuse(
                            ui,
                            taken,
                            number_out_of_range(name, range.minimum, range.maximum, text),
                        ),
                    }
                }
            }
            Act::Press => refuse(ui, taken, no_button(&taken.posted)),
            Act::ScrollTo => {}
        }
    }
    // What the box shows, read before the box borrows the value.
    let shown = if !range.special.is_empty() && *value == range.minimum {
        String::from(range.special)
    } else {
        format!(
            "{}{}",
            crate::format::qt_fixed(*value, range.decimals),
            range.suffix
        )
    };
    // The suffix only where the box shows a number: egui 0.36's DragValue::suffix APPENDS to the
    // atoms after the number (`extend_right`), so a box at its special value showed its words and
    // the unit ("Text style's mm"), and one that set the suffix again inside a formatter showed it
    // twice ("2.500 mm mm"). The formatter writes the number alone; the unit is the box's atom,
    // left off while the special words are shown.
    let at_special = !range.special.is_empty() && *value == range.minimum;
    let mut drag = egui::DragValue::new(value)
        .range(range.minimum..=range.maximum)
        .speed(range.step)
        .fixed_decimals(range.decimals);
    if !at_special && !range.suffix.is_empty() {
        drag = drag.suffix(range.suffix);
    }
    if !range.special.is_empty() {
        let minimum = range.minimum;
        let decimals = range.decimals;
        let special = range.special;
        drag = drag.custom_formatter(move |number, _| {
            if number == minimum {
                String::from(special)
            } else {
                crate::format::qt_fixed(number, decimals)
            }
        });
    }
    let mut response = number_field(ui, &shown, drag);
    if changed {
        response.mark_changed();
    }
    scroll_if_asked(ui, taken.as_ref(), &response);
    let mut record = base_record(ui, name, ControlKind::Spin);
    record.value = range.shown(*value);
    super::registry::record(ui, &response, record);
    response
}

/// QSpinBox over `value`: a fill takes a whole number in `range`.
pub fn whole(ui: &mut Ui, name: &str, value: &mut i64, range: RangeInclusive<i64>) -> Response {
    let taken = take(ui, name);
    let mut changed = false;
    if let Some(taken) = &taken {
        match &taken.posted.act {
            Act::Fill(text) => {
                if accept_fill(ui, taken) {
                    match katana_core::text::parse_integer(text.trim()) {
                        Some(number) if range.contains(&number) => {
                            changed = *value != number;
                            *value = number;
                            done(ui, taken);
                        }
                        _ => refuse(
                            ui,
                            taken,
                            whole_out_of_range(name, *range.start(), *range.end(), text),
                        ),
                    }
                }
            }
            Act::Press => refuse(ui, taken, no_button(&taken.posted)),
            Act::ScrollTo => {}
        }
    }
    let shown = format!("{}", *value);
    let mut response = number_field(ui, &shown, egui::DragValue::new(value).range(range));
    if changed {
        response.mark_changed();
    }
    scroll_if_asked(ui, taken.as_ref(), &response);
    let mut record = base_record(ui, name, ControlKind::Whole);
    record.value = format!("{}", *value);
    super::registry::record(ui, &response, record);
    response
}

// ---- rows and tabs ---------------------------------------------------------------------------

/// A list, a table or a tree, whole rows (each a list of cells), `current` the row selected: a
/// fill selects the row whose first cell is the text given, exactly, as a click on it does
/// (fillField's `match` on column 0).
pub fn rows(
    ui: &mut Ui,
    name: &str,
    rows: &[Vec<String>],
    current: &mut Option<usize>,
) -> Response {
    let taken = take(ui, name);
    let mut changed = false;
    if let Some(taken) = &taken {
        match &taken.posted.act {
            Act::Fill(text) => {
                if accept_fill(ui, taken) {
                    match rows
                        .iter()
                        .position(|row| row.first().is_some_and(|cell| cell == text))
                    {
                        Some(at) => {
                            changed = *current != Some(at);
                            *current = Some(at);
                            done(ui, taken);
                        }
                        None => refuse(ui, taken, lists_no(name, text)),
                    }
                }
            }
            Act::Press => refuse(ui, taken, no_button(&taken.posted)),
            Act::ScrollTo => {}
        }
    }
    let mut response = ui
        .push_id(name, |ui| {
            for (at, row) in rows.iter().enumerate() {
                if ui
                    .selectable_label(*current == Some(at), row.join("   "))
                    .clicked()
                    && *current != Some(at)
                {
                    *current = Some(at);
                    changed = true;
                }
            }
        })
        .response;
    if changed {
        response.mark_changed();
    }
    scroll_if_asked(ui, taken.as_ref(), &response);
    let mut record = base_record(ui, name, ControlKind::Rows);
    record.items = rows.iter().map(|row| row.join(" | ")).collect();
    record.value = current
        .and_then(|at| rows.get(at))
        .and_then(|row| row.first())
        .cloned()
        .unwrap_or_default();
    super::registry::record(ui, &response, record);
    response
}

/// One tab of a strip: its title, the text colour and an accent bar under it when it is in front,
/// the secondary text colour when it is not, the raised colour under the pointer. A tab is a
/// title, not a button: the strip reads as one row of places, with the one in front marked.
fn tab(ui: &mut Ui, title: &str, in_front: bool, inset: f32) -> Response {
    use crate::theme::palette;
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(String::from(title), font, egui::Color32::PLACEHOLDER);
    let padding = egui::vec2(inset, 4.0);
    let size = egui::vec2(
        galley.size().x + 2.0 * padding.x,
        ui.spacing().interact_size.y.max(galley.size().y + 2.0 * padding.y),
    );
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, in_front, title)
    });
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.hovered() && !in_front {
            painter.rect_filled(
                rect,
                egui::CornerRadius::same(4),
                palette::raised().to_egui(),
            );
        }
        let ink = if in_front {
            palette::text()
        } else {
            palette::text_muted()
        };
        let at = egui::pos2(
            rect.left() + padding.x,
            rect.center().y - galley.size().y / 2.0,
        );
        painter.galley_with_override_text_color(at, galley, ink.to_egui());
        if in_front {
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(rect.left(), rect.bottom() - 2.0),
                    rect.right_bottom(),
                ),
                egui::CornerRadius::ZERO,
                palette::accent().to_egui(),
            );
        }
    }
    response
}

/// QTabWidget's tabs, `current` the one in front: a fill brings the tab whose title is the text
/// given to the front, a title's '&' not counted (fillField).
pub fn tabs(ui: &mut Ui, name: &str, titles: &[&str], current: &mut usize) -> Response {
    tabs_with(ui, name, titles, current, true)
}

/// `tabs` drawn as a title bar's title (a group of docks' tabs, dock_chrome's `dock_ui_titled`):
/// as high as the room it is given, with no rule of its own under it - the bar's own border is the
/// rule the tab in front stands on - and set closer, each title the design's GAP in from its
/// tab's sides and the tabs side by side, so two titles fit a panel's bar 240 wide.
pub fn title_tabs(ui: &mut Ui, name: &str, titles: &[&str], current: &mut usize) -> Response {
    tabs_with(ui, name, titles, current, false)
}

fn tabs_with(
    ui: &mut Ui,
    name: &str,
    titles: &[&str],
    current: &mut usize,
    ruled: bool,
) -> Response {
    let taken = take(ui, name);
    let mut changed = false;
    if let Some(taken) = &taken {
        match &taken.posted.act {
            Act::Fill(text) => {
                if accept_fill(ui, taken) {
                    match titles
                        .iter()
                        .position(|title| title.replace('&', "") == *text)
                    {
                        Some(at) => {
                            changed = *current != at;
                            *current = at;
                            done(ui, taken);
                        }
                        None => refuse(ui, taken, no_tab(name, text)),
                    }
                }
            }
            Act::Press => refuse(ui, taken, no_button(&taken.posted)),
            Act::ScrollTo => {}
        }
    }
    let mut response = ui
        .push_id(name, |ui| {
            let strip = ui.horizontal(|ui| {
                let inset = if ruled {
                    crate::theme::style::GROUP_GAP
                } else {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    crate::theme::style::GAP
                };
                for (at, title) in titles.iter().enumerate() {
                    let clicked = tab(ui, &title.replace('&', ""), *current == at, inset).clicked();
                    if clicked && *current != at {
                        *current = at;
                        changed = true;
                    }
                }
            });
            if ruled {
                // A rule under the strip, across what holds it, that the tab in front stands on;
                // in a dialog body's own column, across the body as the last pass drew it: a page
                // wider than the width the body is laid out at had the rule stop short of it.
                let mut across = ui.max_rect().x_range();
                if !crate::shell::dialog::in_a_column(ui)
                    && let Some(body) = crate::shell::dialog::body_span(ui)
                {
                    across.max = across.max.max(body.max);
                }
                let under = strip.response.rect.bottom();
                let painter = ui.painter();
                painter.hline(
                    across,
                    painter.round_to_pixel_center(under),
                    egui::Stroke::new(1.0, crate::theme::palette::border().to_egui()),
                );
                ui.add_space(4.0);
            }
        })
        .response;
    if changed {
        response.mark_changed();
    }
    scroll_if_asked(ui, taken.as_ref(), &response);
    let mut record = base_record(ui, name, ControlKind::Tabs);
    record.items = titles.iter().map(|title| title.replace('&', "")).collect();
    record.value = titles
        .get(*current)
        .map(|title| title.replace('&', ""))
        .unwrap_or_default();
    super::registry::record(ui, &response, record);
    response
}

/// Records a control a module draws itself (a view, a tool button with its own painting) under
/// `name`, taking no act: for such a control the module takes and answers its acts through
/// `registry::take` and `registry::settle` itself.
pub fn custom(ui: &Ui, response: &Response, mut record: ControlRecord) {
    record.enabled = ui.is_enabled();
    record.visible = ui.is_visible();
    super::registry::record(ui, response, record);
}

#[cfg(test)]
mod tests {
    use super::SpinRange;

    // A box of two decimals rounds 0.705 as it shows it: Qt's fixed text of the double nearest
    // 0.705 (0.70499999999999996) is "0.70".
    #[test]
    fn a_spin_rounds_a_value_to_its_decimals_as_it_shows_it() {
        let range = SpinRange::new(0.0, 10.0, 0.1, 2);
        assert_eq!(range.rounded(0.705), 0.70);
        assert_eq!(range.rounded(1.25), 1.25);
        assert_eq!(range.rounded(0.125), 0.13);
    }
}
