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

/// A press reaches a control a person could click: refused when its owner hid it, and when it is
/// disabled. The --press step asks disabled first; an invisible egui `Ui` is always disabled too,
/// so hidden is asked first here, and a button both hidden and disabled is refused as hidden.
pub(crate) fn accept_press(ui: &Ui, taken: &Taken) -> bool {
    if !ui.is_visible() {
        refuse(ui, taken, press_hidden(&taken.posted));
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
/// invisible egui `Ui` is disabled too, only a visible field is refused as disabled.
pub(crate) fn accept_fill(ui: &Ui, taken: &Taken) -> bool {
    if ui.is_visible() && !ui.is_enabled() {
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

/// A button drawn as `button` - an icon, a frame of its own - with `text` the name a screen reader
/// gives it, which is what it reports.
pub fn button_as(ui: &mut Ui, name: &str, text: &str, button: egui::Button<'_>) -> Response {
    let taken = take(ui, name);
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
    let mut response = ui.add(egui::Checkbox::new(on, text));
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
    let mut response = ui.add(egui::RadioButton::new(chosen, text));
    press_button(ui, taken.as_ref(), name, &mut response);
    scroll_if_asked(ui, taken.as_ref(), &response);
    let mut record = base_record(ui, name, ControlKind::Radio);
    record.text = String::from(text);
    record.checked = Some(chosen);
    super::registry::record(ui, &response, record);
    response
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
    let mut edit = egui::TextEdit::singleline(value)
        .id_salt(name)
        .hint_text(String::from(options.placeholder))
        .password(options.password)
        .interactive(!options.read_only);
    if let Some(width) = options.width {
        edit = edit.desired_width(width);
    }
    let mut response = ui.add(edit);
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
    let mut edit = egui::TextEdit::multiline(value)
        .id_salt(name)
        .desired_rows(options.rows)
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
pub fn label_as(ui: &mut Ui, name: &str, text: &str, label: egui::Label) -> Response {
    let taken = take(ui, name);
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

/// QComboBox over `items`: a fill chooses the item whose text is the text given; an editable one
/// takes a text it does not list, as typing does; any other is refused (fillField).
pub fn choice(ui: &mut Ui, name: &str, items: &[String], chosen: &mut Choice) -> Response {
    let taken = take(ui, name);
    let mut changed = false;
    if let Some(taken) = &taken {
        match &taken.posted.act {
            Act::Fill(text) => {
                if accept_fill(ui, taken) {
                    if let Some(index) = items.iter().position(|item| item == text) {
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
        let index = chosen.index;
        let text = &mut chosen.text;
        ui.horizontal(|ui| {
            let edit = ui.add(egui::TextEdit::singleline(text).id_salt(name));
            let list = egui::ComboBox::from_id_salt((name, "items"))
                .selected_text("")
                .show_ui(ui, |ui| {
                    for (at, item) in items.iter().enumerate() {
                        if ui.selectable_label(index == Some(at), item).clicked() {
                            picked = Some(at);
                        }
                    }
                });
            edit.union(list.response)
        })
        .inner
    } else {
        let current = chosen.current_text(items);
        let index = chosen.index;
        egui::ComboBox::from_id_salt(name)
            .selected_text(current)
            .show_ui(ui, |ui| {
                for (at, item) in items.iter().enumerate() {
                    if ui.selectable_label(index == Some(at), item).clicked() {
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
}

impl SpinRange {
    pub const fn new(minimum: f64, maximum: f64, step: f64, decimals: usize) -> Self {
        Self {
            minimum,
            maximum,
            step,
            decimals,
            suffix: "",
        }
    }

    pub const fn with_suffix(self, suffix: &'static str) -> Self {
        Self { suffix, ..self }
    }

    /// QDoubleSpinBox's rounding of a value to its decimals: the number as it would show it, read
    /// back.
    pub fn rounded(&self, value: f64) -> f64 {
        crate::format::qt_fixed(value, self.decimals)
            .parse()
            .unwrap_or(value)
    }
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
    let mut response = ui.add(
        egui::DragValue::new(value)
            .range(range.minimum..=range.maximum)
            .speed(range.step)
            .fixed_decimals(range.decimals)
            .suffix(range.suffix),
    );
    if changed {
        response.mark_changed();
    }
    scroll_if_asked(ui, taken.as_ref(), &response);
    let mut record = base_record(ui, name, ControlKind::Spin);
    record.value = crate::format::qt_fixed(*value, range.decimals);
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
    let mut response = ui.add(egui::DragValue::new(value).range(range));
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

/// QTabWidget's tabs, `current` the one in front: a fill brings the tab whose title is the text
/// given to the front, a title's '&' not counted (fillField).
pub fn tabs(ui: &mut Ui, name: &str, titles: &[&str], current: &mut usize) -> Response {
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
            ui.horizontal(|ui| {
                for (at, title) in titles.iter().enumerate() {
                    if ui
                        .selectable_label(*current == at, title.replace('&', ""))
                        .clicked()
                        && *current != at
                    {
                        *current = at;
                        changed = true;
                    }
                }
            });
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
