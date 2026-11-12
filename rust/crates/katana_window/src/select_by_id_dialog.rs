//! Edit > Select by ID (select_by_id_dialog.hpp; docs/desktop.md, "Select by ID"): the ids that
//! LIST, INFO, AREA and a refusal print, turned back into a selection and found in the drawing.
//!
//! A front end over the interpreter's SELECT id id ...: it builds that line and hands it to the
//! window's one executor (`Host::run`), so the log shows what was run and a refusal - an id that
//! does not exist, or is on a hidden or locked layer - is SELECT's own. Non-modal and kept, so
//! several lookups are made without reopening it.
//!
//! Object names: selectByIdDialog, selectByIdIds (the ids, separated by commas or blanks, #12 as
//! well as 12), selectByIdAdd (add to the current selection), selectByIdZoom (frame what was
//! selected; on by default), selectByIdSelect, selectByIdStatus (what SELECT answered) and
//! selectByIdClose.

use egui::Ui;
use katana_cad::CommandInterpreter;
use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result};
use katana_entity::EntityId;

use crate::drive::{LineOptions, button, check, label_as, line_with};
use crate::host::Host;
use crate::shell::DialogFrame;

/// What separates two ids: a comma or a blank (the C++'s `[,\s]+`, whose `\s` is ASCII's six).
fn separates(character: char) -> bool {
    matches!(
        character,
        ',' | ' ' | '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r'
    )
}

/// The ids in `text` - separated by commas or blanks, each a positive whole number, written 12 or
/// #12 - in the order given, each once. InvalidArgument naming the first word that is not an id,
/// or saying there is none.
pub fn parse_entity_ids(text: &str) -> Result<Vec<EntityId>> {
    let mut ids = Vec::new();
    for word in text.split(separates).filter(|word| !word.is_empty()) {
        // The interpreter's own test of an id, so what is accepted here is what INFO and SELECT
        // accept.
        if !CommandInterpreter::is_entity_id(word) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "not an entity id",
                word,
            ));
        }
        let digits = word.strip_prefix('#').unwrap_or(word);
        let id: EntityId = digits.parse().unwrap_or(0);
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    if ids.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "there is no id to select",
        ));
    }
    Ok(ids)
}

/// The SELECT line for `ids`: with `add`, the ones in `current` first and then the new ones, each
/// once, as SELECT sets the selection to exactly what it names.
pub fn select_by_id_line(ids: &[EntityId], current: &[EntityId], add: bool) -> String {
    let mut wanted: Vec<EntityId> = if add { current.to_vec() } else { Vec::new() };
    for &id in ids {
        if !wanted.contains(&id) {
            wanted.push(id);
        }
    }
    let mut line = String::from("SELECT");
    for id in wanted {
        line.push_str(&format!(" {id}"));
    }
    line
}

/// After SELECT succeeded: frame the selection when the flag says so, and show its properties. The
/// window's (through its views); a test's own.
pub type OnSelected = Box<dyn FnMut(bool, &mut dyn Host)>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct SelectByIdState {
    ids: String,
    add: bool,
    zoom: bool,
    status: String,
    select: bool,
    close: bool,
}

impl Default for SelectByIdState {
    fn default() -> Self {
        Self {
            ids: String::new(),
            add: false,
            // Framing is the default.
            zoom: true,
            status: String::new(),
            select: false,
            close: false,
        }
    }
}

impl SelectByIdState {
    fn body(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label("Ids:");
            line_with(
                ui,
                "selectByIdIds",
                &mut self.ids,
                LineOptions {
                    placeholder: "12, 15 #40",
                    ..LineOptions::default()
                },
            )
            .on_hover_text(
                "The entity ids LIST, INFO and AREA print, separated by commas or spaces; #12 is \
                 12",
            );
        });
        check(
            ui,
            "selectByIdAdd",
            "Add to the current selection",
            &mut self.add,
        );
        check(
            ui,
            "selectByIdZoom",
            "Zoom to what is selected",
            &mut self.zoom,
        );
        label_as(
            ui,
            "selectByIdStatus",
            &self.status,
            egui::Label::new(self.status.as_str()).wrap(),
        );
    }

    fn footer(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            // Select keeps the dialog open, for the next id; Close closes it.
            if button(ui, "selectByIdSelect", "Select").clicked() {
                self.select = true;
            }
            if button(ui, "selectByIdClose", "Close").clicked() {
                self.close = true;
            }
        });
    }
}

/// Edit > Select by ID (see the module comment).
pub struct SelectByIdDialog {
    frame: DialogFrame,
    state: SelectByIdState,
    /// Told after SELECT succeeded, with whether to frame the selection.
    pub on_selected: Option<OnSelected>,
    /// Whether it is shown.
    pub open: bool,
}

impl Default for SelectByIdDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl SelectByIdDialog {
    pub fn new() -> Self {
        Self {
            frame: DialogFrame::new("selectByIdDialog", "Select by ID"),
            state: SelectByIdState::default(),
            on_selected: None,
            open: true,
        }
    }

    pub fn object_name(&self) -> &str {
        self.frame.name()
    }

    /// What selectByIdStatus says.
    pub fn status(&self) -> &str {
        &self.state.status
    }

    /// What Select does: parse, run SELECT, report. False when nothing was selected - the ids did
    /// not parse or SELECT refused them - with the reason in the status line.
    pub fn select_ids(&mut self, host: &mut dyn Host) -> bool {
        let ids = match parse_entity_ids(&self.state.ids) {
            Ok(ids) => ids,
            Err(error) => {
                self.state.status = error.describe();
                return false;
            }
        };
        let current = host.document().selection().ids();
        let line = select_by_id_line(&ids, &current, self.state.add);
        let outcome = host.run(&line);
        if !outcome.ok {
            self.state.status = outcome.error;
            return false;
        }
        self.state.status = outcome.reply;
        let zoom = self.state.zoom;
        if let Some(on_selected) = self.on_selected.as_mut() {
            on_selected(zoom, host);
        }
        true
    }

    /// Draws the dialog while it is open.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.open {
            return;
        }
        let ctx = ui.ctx().clone();
        let shown = self.frame.show(
            &ctx,
            &mut self.state,
            |ui, state| state.body(ui),
            |ui, state| state.footer(ui),
        );
        if std::mem::take(&mut self.state.select) {
            self.select_ids(host);
        }
        if shown.closed || std::mem::take(&mut self.state.close) {
            self.open = false;
        }
    }
}
