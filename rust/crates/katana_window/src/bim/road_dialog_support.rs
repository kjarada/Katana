//! What the road model's dialogs (road_template_manager, corridor_dialog,
//! corridor_quantities_dialog, and the alignment manager and road object dialogs beside them) are
//! built from: their buttons, fields, choices and tables made the one way, and the one way they
//! run a line and say what it answered. The port of bim/road_dialog_support.hpp, with what the
//! Qt widgets those dialogs used kept between frames (QTableWidget, QComboBox) as plain data here.
//!
//! Every dialog here changes nothing itself: it builds the line a person would type and hands it
//! to the window's one executor (host::Host::run), so this is also where that rule lives for them.
//!
//! THE TABLES. A QTableWidget held its cells, its selection and its current row between frames,
//! and the dialogs' handlers ran from its signals. Here a `Table` holds the same, a dialog draws it
//! each frame, and what changed in the frame - the row a person or a driver chose, the cells typed
//! into - comes back from `Table::ui` for the dialog to run its handler, as the signal did. A cell
//! edited in place is a buffer the dialog applies by one line; nothing runs from a table's own
//! change. The table is one named control (the C++ object name): a fill chooses the row whose first
//! cell is the text, as main.cpp's fillField does for a QTableWidget.

use std::collections::BTreeSet;

use egui::{Label, Response, RichText, Ui};
use katana_core::text::parse_finite_double;

use crate::canvas::{Canvas, Image};
use crate::command_word::command_word;
use crate::drive::controls::{
    Choice, LineOptions, accept_fill, accept_press, button_as, choice, custom, label_as, line_with,
};
use crate::drive::mailbox::{Act, lists_no, no_button, no_field};
use crate::drive::registry::{ControlKind, ControlRecord, settle, take};
use crate::host::Host;
use crate::theme::palette;

// ---- text with a mnemonic ----------------------------------------------------------------------

/// The text a person sees of a Qt text with a mnemonic: "&&" an ampersand, a single '&' marking the
/// letter Alt reaches and not shown. egui has no mnemonics; the text with its '&' is what the
/// registry records, so a check of the letters reads them as the C++ test read `text()`.
pub fn shown_text(text: &str) -> String {
    let mut shown = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '&' {
            if characters.peek() == Some(&'&') {
                characters.next();
                shown.push('&');
            }
            continue;
        }
        shown.push(character);
    }
    shown
}

/// The letter a text's mnemonic marks, lower case; None for a text with none.
pub fn mnemonic(text: &str) -> Option<char> {
    let at = text.find('&')?;
    text[at + 1..]
        .chars()
        .next()
        .map(|letter| letter.to_lowercase().next().unwrap_or(letter))
}

// ---- buttons, fields, labels ---------------------------------------------------------------------

/// road_ui::button: a push button by its object name, its text and its tip. (Qt's autoDefault was
/// turned off so Enter in a field stays the field's; egui has no default button to turn off.)
pub fn button(ui: &mut Ui, name: &str, text: &str, tip: &str) -> Response {
    let response = button_as(ui, name, text, egui::Button::new(shown_text(text)));
    if tip.is_empty() {
        response
    } else {
        response.on_hover_text(tip)
    }
}

/// A radio button whose text has a mnemonic: drawn without the '&', recorded with it. A press is
/// `clicked()`, refused hidden or disabled as every named control refuses it; the caller makes it
/// the one chosen. (F0's `radio` records the text as drawn, which would show the '&'.)
pub fn radio(ui: &mut Ui, name: &str, text: &str, chosen: bool) -> Response {
    let taken = take(ui, name);
    let mut response = ui.add(egui::RadioButton::new(chosen, shown_text(text)));
    if let Some(taken) = &taken {
        match &taken.posted.act {
            Act::Press => {
                if accept_press(ui, taken) {
                    response
                        .flags
                        .insert(egui::response::Flags::FAKE_PRIMARY_CLICKED);
                    settle(ui, taken, Ok(()));
                }
            }
            Act::Fill(fill) => {
                if accept_fill(ui, taken) {
                    settle(ui, taken, Err(no_field(name, fill)));
                }
            }
            Act::ScrollTo => {
                response.scroll_to_me(None);
                settle(ui, taken, Ok(()));
            }
        }
    }
    let mut record = ControlRecord::new(name, ControlKind::Radio);
    record.text = String::from(text);
    record.checked = Some(chosen);
    custom(ui, &response, record);
    response
}

/// road_ui::field: a one-line field by its object name, its placeholder and its tip.
pub fn field(
    ui: &mut Ui,
    name: &str,
    value: &mut String,
    placeholder: &str,
    tip: &str,
) -> Response {
    let response = line_with(
        ui,
        name,
        value,
        LineOptions {
            placeholder,
            ..LineOptions::default()
        },
    );
    if tip.is_empty() {
        response
    } else {
        response.on_hover_text(tip)
    }
}

/// road_ui::muted: a word-wrapped label in the muted colour, named.
pub fn muted(ui: &mut Ui, name: &str, text: &str) -> Response {
    let shown = Label::new(RichText::new(text).color(palette::text_muted().to_egui())).wrap();
    label_as(ui, name, text, shown)
}

/// The same with no name: a dialog's explanation, which nothing drives.
pub fn note(ui: &mut Ui, text: &str) {
    ui.add(Label::new(RichText::new(text).color(palette::text_muted().to_egui())).wrap());
}

/// A dialog's status line: what the last line replied, or why it was refused, in the error colour
/// or the muted one (road_ui::setStatus).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusLine {
    pub text: String,
    pub is_error: bool,
}

impl StatusLine {
    /// road_ui::setStatus.
    pub fn set(&mut self, text: impl Into<String>, is_error: bool) {
        self.text = text.into();
        self.is_error = is_error;
    }

    /// QLabel::clear: the text gone, the colour it was given kept.
    pub fn clear(&mut self) {
        self.text.clear();
    }

    /// Draws it as the label `name`, word-wrapped.
    pub fn ui(&self, ui: &mut Ui, name: &str) -> Response {
        let colour = if self.is_error {
            palette::error()
        } else {
            palette::text_muted()
        };
        let shown = Label::new(RichText::new(self.text.as_str()).color(colour.to_egui())).wrap();
        label_as(ui, name, &self.text, shown)
    }
}

// ---- words and lines ---------------------------------------------------------------------------

/// road_ui::word: a name as the command line reads it (command_word), or None with the refusal on
/// the status line.
pub fn word(status: &mut StatusLine, text: &str, what: &str) -> Option<String> {
    match command_word(text, what) {
        Ok(word) => Some(word),
        Err(error) => {
            status.set(error.message, true);
            None
        }
    }
}

/// road_ui::option: key=value with the value quoted when it needs to be ("material=crushed rock");
/// an empty value is "key=".
pub fn option(status: &mut StatusLine, key: &str, value: &str) -> Option<String> {
    if value.is_empty() {
        return Some(format!("{key}="));
    }
    word(status, &format!("{key}={value}"), key)
}

/// QString::split('\n', Qt::SkipEmptyParts): the reply's lines, empty ones passed over.
pub fn reply_lines(reply: &str) -> Vec<&str> {
    reply.split('\n').filter(|line| !line.is_empty()).collect()
}

/// road_ui::run: `line` through the executor, the status saying what it did - the first line of
/// its reply, or why it was refused. True when it was carried out. `executor` false is a dialog
/// built with no executor (the C++'s empty CommandRunner): it says it cannot run the line.
pub fn run(host: &mut dyn Host, executor: bool, status: &mut StatusLine, line: &str) -> bool {
    if !executor {
        status.set(
            format!("Nothing here can run {line}: the dialog has no command line."),
            true,
        );
        return false;
    }
    let outcome = host.run(line);
    if outcome.ok {
        let lines = reply_lines(&outcome.reply);
        status.set(lines.first().copied().unwrap_or(line), false);
    } else if outcome.error.is_empty() {
        status.set(format!("{line} was refused."), true);
    } else {
        status.set(outcome.error.as_str(), true);
    }
    outcome.ok
}

/// road_ui::number: a number a cell holds, read as the command line reads one (trimmed first);
/// None for anything else.
pub fn number(text: &str) -> Option<f64> {
    parse_finite_double(text.trim())
}

/// QString::compare(.., Qt::CaseInsensitive): equal letter for letter, case folded. Qt folds by
/// Unicode's simple case folding; this lowers each character, which agrees with it for every
/// letter a name of the drawing is likely to hold.
pub fn same_ignoring_case(first: &str, second: &str) -> bool {
    first
        .chars()
        .flat_map(char::to_lowercase)
        .eq(second.chars().flat_map(char::to_lowercase))
}

// ---- a QComboBox -------------------------------------------------------------------------------

/// What a QComboBox held between frames: its items, each item's data (QVariant's text; empty when
/// it was given none), and the item chosen - or, editable, the text typed. Qt's rules kept: the
/// first item added to an empty box is chosen; clear() chooses none.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Combo {
    items: Vec<String>,
    data: Vec<String>,
    choice: Choice,
}

impl Combo {
    pub fn new() -> Self {
        Self::default()
    }

    /// An editable box (QComboBox::setEditable(true)): a name it does not list may be typed.
    pub fn editable() -> Self {
        Self {
            choice: Choice::editable(""),
            ..Self::default()
        }
    }

    pub fn items(&self) -> &[String] {
        &self.items
    }

    pub fn count(&self) -> usize {
        self.items.len()
    }

    pub fn item_text(&self, index: usize) -> Option<&str> {
        self.items.get(index).map(String::as_str)
    }

    pub fn current_index(&self) -> Option<usize> {
        self.choice.index.filter(|&at| at < self.items.len())
    }

    /// QComboBox::currentText.
    pub fn current_text(&self) -> String {
        self.choice.current_text(&self.items)
    }

    /// QComboBox::currentData().toString(): empty for an item given no data, and for none.
    pub fn current_data(&self) -> String {
        self.current_index()
            .and_then(|at| self.data.get(at))
            .cloned()
            .unwrap_or_default()
    }

    /// QComboBox::findText, exactly and in the same case.
    pub fn find_text(&self, text: &str) -> Option<usize> {
        self.items.iter().position(|item| item == text)
    }

    /// QComboBox::findText with Qt::MatchFixedString: the same text ignoring case.
    pub fn find_text_ignoring_case(&self, text: &str) -> Option<usize> {
        self.items
            .iter()
            .position(|item| same_ignoring_case(item, text))
    }

    /// QComboBox::findData.
    pub fn find_data(&self, data: &str) -> Option<usize> {
        self.data.iter().position(|item| item == data)
    }

    /// QComboBox::addItem(text): chosen when it is the first.
    pub fn add_item(&mut self, text: &str) {
        self.add_item_with_data(text, "");
    }

    /// QComboBox::addItem(text, data).
    pub fn add_item_with_data(&mut self, text: &str, data: &str) {
        self.items.push(String::from(text));
        self.data.push(String::from(data));
        if self.choice.index.is_none() {
            self.choice.choose(self.items.len() - 1, &self.items);
        }
    }

    /// QComboBox::addItems.
    pub fn add_items(&mut self, texts: &[String]) {
        for text in texts {
            self.add_item(text);
        }
    }

    /// QComboBox::clear: no items, none chosen, nothing typed.
    pub fn clear(&mut self) {
        self.items.clear();
        self.data.clear();
        self.choice.index = None;
        self.choice.text.clear();
    }

    /// QComboBox::setCurrentIndex; None (-1) chooses none.
    pub fn set_current_index(&mut self, index: Option<usize>) {
        match index.filter(|&at| at < self.items.len()) {
            Some(at) => self.choice.choose(at, &self.items),
            None => {
                self.choice.index = None;
                if self.choice.editable {
                    self.choice.text.clear();
                }
            }
        }
    }

    /// QComboBox::setCurrentText: an editable box shows the text; another chooses the item of
    /// that text, and keeps what it showed when it lists none.
    pub fn set_current_text(&mut self, text: &str) {
        if self.choice.editable {
            self.choice.text = String::from(text);
        } else if let Some(at) = self.find_text(text) {
            self.choice.choose(at, &self.items);
        }
    }

    /// Draws it as the choice `name` (F0's QComboBox): true when a person or a driver changed it.
    pub fn ui(&mut self, ui: &mut Ui, name: &str) -> bool {
        choice(ui, name, &self.items, &mut self.choice).changed()
    }
}

// ---- a QTableWidget ----------------------------------------------------------------------------

/// One cell: its text, and whether it is edited in place (Qt::ItemIsEditable).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    pub text: String,
    pub editable: bool,
}

impl Cell {
    pub fn new(text: impl Into<String>, editable: bool) -> Self {
        Self {
            text: text.into(),
            editable,
        }
    }
}

/// road_ui::cell(text): a cell that is not edited.
pub fn cell(text: impl Into<String>) -> Cell {
    Cell::new(text, false)
}

/// road_ui::cell(text, true): a cell edited in place.
pub fn editable(text: impl Into<String>) -> Cell {
    Cell::new(text, true)
}

/// What a frame did to a table: the selection changed (itemSelectionChanged), and the cells a
/// person typed into (cellChanged), each once, in the order typed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableEvents {
    pub selection_changed: bool,
    pub edited: Vec<(usize, usize)>,
}

/// A QTableWidget of whole rows: its object name, headers, cells, the rows selected and the
/// current row (see the module comment).
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    name: String,
    headers: Vec<String>,
    rows: Vec<Vec<Cell>>,
    selected: BTreeSet<usize>,
    current: Option<usize>,
    /// QAbstractItemView::ExtendedSelection: more than one row may be chosen.
    extended: bool,
    /// Edit triggers set: a cell marked editable is typed into in place.
    editing: bool,
    /// setMaximumHeight: the most it is shown at, in points.
    max_height: Option<f32>,
    /// A height it keeps whatever it holds (setMinimumHeight with room for rows).
    fixed_height: Option<f32>,
}

impl Table {
    /// road_ui::table: rows selected whole, one at a time, nothing edited.
    pub fn new(name: &str, headers: &[&str]) -> Self {
        Self {
            name: String::from(name),
            headers: headers.iter().map(|header| String::from(*header)).collect(),
            rows: Vec::new(),
            selected: BTreeSet::new(),
            current: None,
            extended: false,
            editing: false,
            max_height: None,
            fixed_height: None,
        }
    }

    /// The same, several rows chosen at once (ExtendedSelection).
    pub fn extended(mut self) -> Self {
        self.extended = true;
        self
    }

    /// The same, its editable cells typed into in place.
    pub fn editing(mut self) -> Self {
        self.editing = true;
        self
    }

    /// The same, shown no taller than `height` points.
    pub fn max_height(mut self, height: f32) -> Self {
        self.max_height = Some(height);
        self
    }

    /// Keeps `height` points of room, however few rows it holds.
    pub fn set_fixed_height(&mut self, height: f32) {
        self.fixed_height = Some(height);
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn headers(&self) -> &[String] {
        &self.headers
    }

    pub fn column_count(&self) -> usize {
        self.headers.len()
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// setRowCount(0): no rows, none selected, none current. True when rows were selected, which
    /// is when Qt says the selection changed.
    pub fn clear(&mut self) -> bool {
        let had = !self.selected.is_empty();
        self.rows.clear();
        self.selected.clear();
        self.current = None;
        had
    }

    /// A row more at the end (insertRow at rowCount, then setItem of each cell): no selection
    /// changes, no edit is reported.
    pub fn push_row(&mut self, cells: Vec<Cell>) -> usize {
        self.rows.push(cells);
        self.rows.len() - 1
    }

    /// removeRow: the rows below move up; a row selected goes with it, and the current row stays
    /// where it was, on the row that took its place (or the one above, at the end).
    pub fn remove_row(&mut self, row: usize) {
        if row >= self.rows.len() {
            return;
        }
        self.rows.remove(row);
        self.selected = self
            .selected
            .iter()
            .filter(|&&at| at != row)
            .map(|&at| if at > row { at - 1 } else { at })
            .collect();
        self.current = match self.current {
            Some(at) if at > row => Some(at - 1),
            Some(at) if at == row => {
                if row < self.rows.len() {
                    Some(row)
                } else {
                    row.checked_sub(1)
                }
            }
            kept => kept,
        };
    }

    pub fn cell(&self, row: usize, column: usize) -> Option<&Cell> {
        self.rows.get(row).and_then(|cells| cells.get(column))
    }

    /// A cell's text; empty when there is no such cell (the C++'s null item).
    pub fn text(&self, row: usize, column: usize) -> String {
        self.cell(row, column)
            .map(|cell| cell.text.clone())
            .unwrap_or_default()
    }

    /// QTableWidgetItem::setText: true when the cell is there and its text changed - when Qt says
    /// the item changed.
    pub fn set_text(&mut self, row: usize, column: usize, text: &str) -> bool {
        match self
            .rows
            .get_mut(row)
            .and_then(|cells| cells.get_mut(column))
        {
            Some(cell) if cell.text != text => {
                cell.text = String::from(text);
                true
            }
            _ => false,
        }
    }

    /// Every row, each its cells' texts.
    pub fn texts(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|cells| cells.iter().map(|cell| cell.text.clone()).collect())
            .collect()
    }

    /// QTableWidget::currentRow; None for -1.
    pub fn current_row(&self) -> Option<usize> {
        self.current.filter(|&at| at < self.rows.len())
    }

    /// The rows selected, ascending.
    pub fn selected_rows(&self) -> Vec<usize> {
        self.selected.iter().copied().collect()
    }

    pub fn is_selected(&self, row: usize) -> bool {
        self.selected.contains(&row)
    }

    /// QTableView::selectRow (and setCurrentCell): the row alone selected, and current. True when
    /// the selection changed.
    pub fn select_row(&mut self, row: usize) -> bool {
        if row >= self.rows.len() {
            return false;
        }
        let before = std::mem::take(&mut self.selected);
        self.selected.insert(row);
        self.current = Some(row);
        before != self.selected
    }

    /// QItemSelectionModel::select(row, Select | Rows): the row added to the selection. True when
    /// it was not selected.
    pub fn select_also(&mut self, row: usize) -> bool {
        row < self.rows.len() && self.selected.insert(row)
    }

    /// QAbstractItemView::clearSelection. True when something was selected.
    pub fn clear_selection(&mut self) -> bool {
        let had = !self.selected.is_empty();
        self.selected.clear();
        had
    }

    /// Draws the table and takes what a person or a driver did to it this frame (see the module
    /// comment): a fill chooses the row whose first cell is the text; a click chooses a row; an
    /// editable cell is typed into in place.
    pub fn ui(&mut self, ui: &mut Ui) -> TableEvents {
        let mut events = TableEvents::default();
        let name = self.name.clone();
        let taken = take(ui, &name);
        if let Some(taken) = &taken {
            match &taken.posted.act {
                Act::Fill(text) => {
                    if accept_fill(ui, taken) {
                        let found = self
                            .rows
                            .iter()
                            .position(|row| row.first().is_some_and(|first| first.text == *text));
                        match found {
                            Some(at) => {
                                if self.select_row(at) {
                                    events.selection_changed = true;
                                }
                                settle(ui, taken, Ok(()));
                            }
                            None => settle(ui, taken, Err(lists_no(&name, text))),
                        }
                    }
                }
                Act::Press => settle(ui, taken, Err(no_button(&taken.posted))),
                Act::ScrollTo => {}
            }
        }
        let mut clicked = None;
        let mut area = egui::ScrollArea::both().id_salt((name.as_str(), "scroll"));
        area = match (self.fixed_height, self.max_height) {
            (Some(height), _) => area
                .min_scrolled_height(height)
                .max_height(height)
                .auto_shrink([false, false]),
            (None, Some(height)) => area.max_height(height).auto_shrink([false, true]),
            (None, None) => area.auto_shrink([false, true]),
        };
        let editing = self.editing;
        let headers = &self.headers;
        let rows = &mut self.rows;
        let selected = &self.selected;
        let output = area.show(ui, |ui| {
            egui::Grid::new((name.as_str(), "grid"))
                .striped(true)
                .show(ui, |ui| {
                    for header in headers {
                        ui.strong(header.as_str());
                    }
                    ui.end_row();
                    for (row, cells) in rows.iter_mut().enumerate() {
                        let chosen = selected.contains(&row);
                        for (column, cell) in cells.iter_mut().enumerate() {
                            if editing && cell.editable {
                                let response = ui.add(
                                    egui::TextEdit::singleline(&mut cell.text)
                                        .id_salt((name.as_str(), row, column))
                                        .desired_width(90.0),
                                );
                                if response.changed() && !events.edited.contains(&(row, column)) {
                                    events.edited.push((row, column));
                                }
                                if response.gained_focus() {
                                    clicked = Some(row);
                                }
                            } else if ui.selectable_label(chosen, cell.text.as_str()).clicked() {
                                clicked = Some(row);
                            }
                        }
                        ui.end_row();
                    }
                });
        });
        if let Some(row) = clicked
            && self.select_row(row)
        {
            events.selection_changed = true;
        }
        let response = ui.interact(
            output.inner_rect,
            ui.id().with((name.as_str(), "table")),
            egui::Sense::hover(),
        );
        if let Some(taken) = &taken
            && taken.posted.act == Act::ScrollTo
        {
            response.scroll_to_me(None);
            settle(ui, taken, Ok(()));
        }
        let mut record = ControlRecord::new(&name, ControlKind::Rows);
        record.items = self
            .rows
            .iter()
            .map(|cells| {
                cells
                    .iter()
                    .map(|cell| cell.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" | ")
            })
            .collect();
        record.value = self
            .current_row()
            .and_then(|at| self.rows.get(at))
            .and_then(|cells| cells.first())
            .map(|first| first.text.clone())
            .unwrap_or_default();
        custom(ui, &response, record);
        events
    }

    /// The table as text for the clipboard: the headers, then each row, cells tab separated and
    /// rows a line each (the dialogs' Copy).
    pub fn tab_separated(&self) -> Vec<String> {
        let mut lines = vec![self.headers.join("\t")];
        for cells in &self.rows {
            let row: Vec<&str> = (0..self.headers.len())
                .map(|column| cells.get(column).map_or("", |cell| cell.text.as_str()))
                .collect();
            lines.push(row.join("\t"));
        }
        lines
    }
}

/// The height of `rows` rows of a table drawn in `style`, its header one of them: a row is an
/// interactive control's height and the spacing under it, as egui's grid lays one out.
pub fn rows_height(style: &egui::Style, rows: f32) -> f32 {
    (style.spacing.interact_size.y + style.spacing.item_spacing.y) * rows
}

// ---- a preview painted by code ---------------------------------------------------------------------

/// Paints a preview of `width` by `height` device pixels at `ratio` device pixels a point into an
/// image: the window's raster canvas (C's), lent to a road dialog so its preview is seen on screen.
/// The callback is given the canvas, its world transform scaled by the ratio, to paint in points.
pub type PreviewRaster = Box<dyn Fn(u32, u32, f64, &mut dyn FnMut(&mut dyn Canvas)) -> Image>;

/// A preview's place in a dialog, `size` points, named `name`: painted by `paint` through `raster`
/// when the window has lent one, and kept as a texture for the frame; empty until then.
pub fn preview(
    ui: &mut Ui,
    name: &str,
    size: egui::Vec2,
    raster: Option<&PreviewRaster>,
    texture: &mut Option<egui::TextureHandle>,
    paint: &mut dyn FnMut(&mut dyn Canvas),
) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    if let Some(raster) = raster
        && ui.is_rect_visible(rect)
    {
        let ratio = ui.ctx().pixels_per_point();
        let width = (rect.width() * ratio).round().max(1.0) as u32;
        let height = (rect.height() * ratio).round().max(1.0) as u32;
        let image = raster(width, height, f64::from(ratio), paint);
        let handle =
            ui.ctx()
                .load_texture(name, image.to_color_image(), egui::TextureOptions::LINEAR);
        ui.painter().image(
            handle.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        *texture = Some(handle);
    }
    custom(ui, &response, ControlRecord::new(name, ControlKind::Other));
    response
}

/// A file to save, chosen in the system's file dialog (QFileDialog::getSaveFileName): `title`, the
/// path the field holds to start from, and one filter. Never called in a headless session, which
/// has nobody to ask; None when the person cancelled.
pub fn save_file_dialog(
    title: &str,
    start: &str,
    filter_name: &str,
    extension: &str,
) -> Option<String> {
    let mut dialog = rfd::FileDialog::new()
        .set_title(title)
        .add_filter(filter_name, &[extension]);
    let start_path = std::path::Path::new(start);
    if let Some(folder) = start_path.parent()
        && !folder.as_os_str().is_empty()
    {
        dialog = dialog.set_directory(folder);
    }
    if let Some(file) = start_path.file_name() {
        dialog = dialog.set_file_name(file.to_string_lossy().into_owned());
    }
    dialog
        .save_file()
        .map(|path| path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::{Combo, Table, cell, mnemonic, shown_text};

    #[test]
    fn a_mnemonic_is_recorded_and_not_shown() {
        assert_eq!(shown_text("A&dd"), "Add");
        assert_eq!(shown_text("Save && Close"), "Save & Close");
        assert_eq!(mnemonic("Delete Matchin&g"), Some('g'));
        assert_eq!(mnemonic("Close"), None);
    }

    // QComboBox: the first item added is chosen, clear() chooses none.
    #[test]
    fn a_combo_keeps_qts_rules_for_what_is_chosen() {
        let mut combo = Combo::new();
        assert_eq!(combo.current_text(), "");
        combo.add_item("road");
        combo.add_item("access");
        assert_eq!(combo.current_text(), "road");
        combo.set_current_text("access");
        assert_eq!(combo.current_index(), Some(1));
        combo.set_current_text("nothing");
        assert_eq!(combo.current_text(), "access");
        combo.clear();
        assert_eq!(combo.current_index(), None);
    }

    #[test]
    fn a_tables_selection_follows_qts_rows() {
        let mut table = Table::new("t", &["a"]);
        for text in ["0", "1", "2"] {
            table.push_row(vec![cell(text)]);
        }
        assert!(table.select_row(1));
        assert!(!table.select_row(1), "the same row again changes nothing");
        table.remove_row(0);
        assert_eq!(table.selected_rows(), [0]);
        assert_eq!(table.current_row(), Some(0));
        assert!(table.clear());
        assert_eq!(table.current_row(), None);
    }
}
