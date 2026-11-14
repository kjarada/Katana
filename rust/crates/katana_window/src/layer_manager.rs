//! The layer manager (Format > Layers). The port of layer_manager.
//!
//! The dock beside the drawing is the quick view: what is on, what is locked, what colour. This is
//! the whole table - linetype, line weight, dimension style, hatch and how many entities are on
//! each - with the operations that need more room than a dock has: move a layer (and its subtree,
//! and the entities on it) under another parent, and put the selection on a layer.
//!
//! A layer name is a PATH ("design/surface/tin1"), so "move" and "rename" are the same command:
//! rename_layer takes the subtree and the entities with it.
//!
//! NON-MODAL, as the styles manager is: Format > Layers shows it beside the drawing. It hears the
//! Document through a DocumentWatcher, so an undo, an import or a command typed anywhere reloads
//! it once, at its next frame, keeping the selected layer - and the form's unsaved edits, when the
//! layer itself did not change underneath them. No button opens a modal box: New, New Child and
//! Rename or Move ask for a name in a prompt row inside the dialog, a colour is typed as #RRGGBB
//! or picked, and every refusal goes to the status line and the log. So a headless session and a
//! test drive every action by object name.
//!
//! The Document may be dropped before the dialog: the dialog then reads and changes nothing, and
//! every control but Close is disabled the first time a person reaches for one.
//!
//! Object names: "layerTable"; the form "layerLinetype", "layerWeight", "layerColourText",
//! "layerColour" (the Choose... button), "layerHatch", "layerDimensionStyle", "layerVisible",
//! "layerLocked"; the buttons "layerNew", "layerNewChild", "layerSave", "layerRevert",
//! "layerMove", "layerDelete", "layerAssign", "layerCurrent", "closeButton"; the prompt
//! "layerPrompt", "layerPromptLabel", "layerPromptName", "layerPromptOk", "layerPromptCancel";
//! "layerStatus"; the colour picked beside the field, "layerColourDialog".
//!
//! What is the C++'s and not here. The C++ dialog has no object name of its own (the headless
//! driver holds it by pointer); a drive target needs one, and it is "layerManager", beside the
//! styles manager's "styleManager". The colour dialog Choose... opened is egui's colour button,
//! shown beside the field while it is open. The current layer's row is not set in bold. As in the
//! C++, this dialog runs its commands on the Document itself, not as lines through the executor.

use egui::{Context, Ui};
use katana_cad::document::Document;
use katana_cad::style_catalogue::NameRole;
use katana_commands::{
    CommandPtr, create_layer, delete_layer, rename_layer, set_entity_layer, update_layer,
};
use katana_entity::layer_path::join_layer_path;
use katana_entity::{Color, Layer};

use crate::annotation::dialog_support::{show_dialog, spin_value, table};
use crate::customisation::customisation_context::CustomisationContext;
use crate::customisation::document_watcher::DocumentWatcher;
use crate::customisation::name_picker::NamePicker;
use crate::drive::controls::{self, LineOptions, SpinRange, custom};
use crate::drive::registry::{ControlKind, ControlRecord};
use crate::format::qt_fixed;
use crate::host::Host;
use crate::kept_name_combo::{KeptChoice, KeptNameCombo};
use crate::shell::DialogFrame;

/// The dialog's name as a drive target (see the module comment).
pub const LAYER_MANAGER: &str = "layerManager";
const TITLE: &str = "Layers";
const DEFAULT_DIMENSION_STYLE: &str = "(default)";
const ERROR_COLOUR: egui::Color32 = egui::Color32::from_rgb(0xE0, 0x5A, 0x4F);
const WEIGHT: SpinRange = SpinRange::new(0.0, 10.0, 0.05, 2).with_suffix(" mm on paper");
const HEADERS: [&str; 8] = [
    "Layer", "On", "Locked", "Colour", "Linetype", "Weight", "Hatch", "Entities",
];

/// What the prompt row's answer is for.
#[derive(Clone, Debug, PartialEq, Eq)]
enum PromptFor {
    New,
    /// A new layer under this one.
    NewChild(String),
    /// A new full path for this one.
    Move(String),
}

/// See the module comment.
pub struct LayerManagerDialog {
    frame: DialogFrame,
    /// The table's rows, and the layer each is, by full path.
    rows: Vec<Vec<String>>,
    names: Vec<String>,
    selected: Option<usize>,

    // A NamePicker, not a plain choice: a layer may name a linestyle no loaded library defines,
    // and a choice that cannot show it wrote the previous row's name back on Save (audit QT-02's
    // twin). It has no thumbnails, so the linetype list has no pictures here.
    linetype: NamePicker,
    weight: f64,
    colour_text: String,
    picked_colour: Color,
    colour_dialog_open: bool,
    hatch: KeptNameCombo,
    dimension: KeptNameCombo,
    visible: bool,
    locked: bool,

    prompt: Option<PromptFor>,
    prompt_label: String,
    prompt_name: String,
    status: String,
    status_is_error: bool,

    /// What a person changed since the form was loaded: a number box shows a weight rounded to
    /// its decimals, and writing that back unasked would change a layer nobody edited.
    weight_edited: bool,
    colour_edited: bool,
    orphan: bool,
    /// The layer as the form was loaded from it. A reload leaves the form - and its unsaved edits
    /// - alone while the stored layer still equals this.
    loaded: Option<Layer>,
    /// What the next reload selects, after a command this dialog ran.
    select_after_reload: Option<String>,
    closing: bool,
    // Last, so it goes first: no delivery reaches a half-dropped dialog.
    watcher: DocumentWatcher,
}

impl LayerManagerDialog {
    pub fn new(document: &Document) -> Self {
        let mut dialog = Self {
            frame: DialogFrame::new(LAYER_MANAGER, TITLE),
            rows: Vec::new(),
            names: Vec::new(),
            selected: None,
            linetype: NamePicker::new(
                &CustomisationContext::default(),
                document,
                NameRole::Linetype,
                false,
            ),
            weight: 0.0,
            colour_text: String::new(),
            picked_colour: Color::default(),
            colour_dialog_open: false,
            hatch: KeptNameCombo::new(),
            dimension: KeptNameCombo::new(),
            visible: false,
            locked: false,
            prompt: None,
            prompt_label: String::new(),
            prompt_name: String::new(),
            status: String::new(),
            status_is_error: false,
            weight_edited: false,
            colour_edited: false,
            orphan: false,
            loaded: None,
            select_after_reload: None,
            closing: false,
            watcher: DocumentWatcher::new(document),
        };
        dialog.reload(document);
        dialog
    }

    // ---- the Document, which may have gone -----------------------------------------------------

    fn alive(&self) -> bool {
        self.watcher.document_alive()
    }

    /// `alive`, and when the Document has gone, the dialog shut down as well. What a person can
    /// reach asks this before it reads the Document.
    fn live(&mut self) -> bool {
        if self.alive() {
            return true;
        }
        self.orphaned();
        false
    }

    /// Once: every control but Close is disabled and the prompt and the colour picker close, so
    /// nothing left on screen can reach the Document. Said on the status line, not through the
    /// log: that belongs to whoever owned the Document, and may have gone with it.
    fn orphaned(&mut self) {
        if self.orphan {
            return;
        }
        self.orphan = true;
        self.prompt = None;
        self.colour_dialog_open = false;
        self.set_status(
            "The drawing this dialog edited has closed: nothing here acts on anything any more.",
            false,
        );
    }

    fn set_status(&mut self, message: &str, is_error: bool) {
        self.status = String::from(message);
        self.status_is_error = is_error;
    }

    fn say(&mut self, host: &mut dyn Host, message: &str, is_error: bool) {
        self.set_status(message, is_error);
        if self.alive() {
            host.log(message, is_error);
        }
    }

    /// Runs a command and says what it did, or why not. The reload that shows its effect comes
    /// from the watcher, once, at the next frame.
    fn run(&mut self, host: &mut dyn Host, command: CommandPtr, done: &str) -> bool {
        if !self.live() {
            return false;
        }
        if let Err(error) = host.document_mut().execute(command) {
            self.say(host, &error.describe(), true);
            return false;
        }
        self.say(host, done, false);
        true
    }

    // ---- the table and the form ----------------------------------------------------------------

    /// What the status line says.
    pub fn status(&self) -> &str {
        &self.status
    }

    /// The layers the table lists, by full path, in its order.
    pub fn table_names(&self) -> &[String] {
        &self.names
    }

    /// The layer selected in the table, by full path; "" for none.
    pub fn selected_layer(&self) -> String {
        self.selected
            .and_then(|row| self.names.get(row))
            .cloned()
            .unwrap_or_default()
    }

    /// Selects a layer (a name the table lacks selects nothing), as a click on its row does. For
    /// the workbench ("open on this layer") and for tests.
    pub fn select_layer(&mut self, document: &Document, name: &str) {
        let row = self.names.iter().position(|listed| listed == name);
        self.select_row(document, row);
    }

    /// For the headless screenshot: selects the first row so the form below is filled in.
    pub fn show_first_row(&mut self, document: &Document) {
        if !self.names.is_empty() {
            self.select_row(document, Some(0));
        }
    }

    fn select_row(&mut self, document: &Document, row: Option<usize>) {
        if self.selected == row {
            return; // a selection that did not change signals nothing
        }
        self.selected = row;
        self.load_selected(document);
    }

    /// The linetype picker, for a test reading what it keeps.
    pub fn linetype(&self) -> &NamePicker {
        &self.linetype
    }

    /// Whether the prompt row is shown.
    pub fn prompt_open(&self) -> bool {
        self.prompt.is_some()
    }

    /// Whether the colour picker Choose... opens is shown.
    pub fn colour_dialog_open(&self) -> bool {
        self.colour_dialog_open
    }

    /// Rebuilds the table and the lists from the Document, keeping the selected layer (or
    /// selecting the one a command just made), and the form's edits unless its layer changed
    /// underneath them.
    pub fn reload(&mut self, document: &Document) {
        if !self.alive() {
            return;
        }
        let select = self
            .select_after_reload
            .take()
            .unwrap_or_else(|| self.selected_layer());
        let model = document.model();
        self.rows.clear();
        self.names.clear();
        // names() is ascending by full path, which IS a pre-order walk of the tree (layer_path),
        // so the table reads as the tree does.
        for name in model.layers.names() {
            let Some(layer) = model.layers.find(&name) else {
                continue;
            };
            let yes_no = |on: bool| String::from(if on { "yes" } else { "no" });
            self.rows.push(vec![
                name.clone(),
                yes_no(layer.visible),
                yes_no(layer.locked),
                layer.color.to_hex(),
                layer.linetype.clone(),
                qt_fixed(layer.line_weight, 2),
                layer.hatch_pattern.clone(),
                format!("{}", model.entities.count_on_layer(&name)),
            ]);
            self.names.push(name);
        }
        self.selected = self.names.iter().position(|listed| *listed == select);

        // Each list is rebuilt from the model and then shows what it showed, kept if the model no
        // longer lists it.
        let fill = |combo: &mut KeptNameCombo, names: Vec<String>, extra: Option<&str>| {
            let was = combo.current();
            let mut choices: Vec<KeptChoice> = Vec::new();
            if let Some(extra) = extra {
                choices.push(KeptChoice::new(extra, ""));
            }
            choices.extend(names.iter().map(|name| KeptChoice::new(name, name)));
            combo.fill(&choices);
            combo.show(&was.unwrap_or_default());
        };
        self.linetype.refresh(document);
        fill(&mut self.hatch, model.hatch_patterns.names(), None);
        fill(
            &mut self.dimension,
            model.dimension_styles.names(),
            Some(DEFAULT_DIMENSION_STYLE),
        );

        let Some(stored) = model.layers.find(&self.selected_layer()) else {
            self.loaded = None;
            return;
        };
        if self.loaded.as_ref() != Some(stored) {
            self.load_form(document, stored);
        }
    }

    fn load_form(&mut self, document: &Document, layer: &Layer) {
        self.linetype.set_current_name(document, &layer.linetype);
        self.weight = spin_value(&WEIGHT, layer.line_weight);
        self.show_colour(layer.color);
        self.hatch.show(&layer.hatch_pattern);
        self.dimension.show(&layer.dimension_style);
        self.visible = layer.visible;
        self.locked = layer.locked;
        self.weight_edited = false;
        self.colour_edited = false;
        self.loaded = Some(layer.clone());
    }

    fn load_selected(&mut self, document: &Document) {
        if !self.live() {
            return;
        }
        match document.model().layers.find(&self.selected_layer()) {
            Some(layer) => self.load_form(document, layer),
            None => self.loaded = None,
        }
    }

    fn show_colour(&mut self, colour: Color) {
        self.picked_colour = colour;
        self.colour_text = colour.to_hex();
    }

    /// A colour the person chose, by typing or in the colour picker.
    fn choose_colour(&mut self, colour: Color) {
        if colour == self.picked_colour {
            self.show_colour(colour); // tidies the text: "#FF8000" for "#ff8000"
            return;
        }
        self.show_colour(colour);
        self.colour_edited = true;
    }

    /// The colour field left, or Enter pressed in it: a typed colour is taken; one that is not a
    /// colour is refused and the field shows the last good one again.
    pub fn finish_colour_text(&mut self, host: &mut dyn Host) {
        if !self.live() {
            return;
        }
        match Color::from_hex(self.colour_text.trim()) {
            Ok(colour) => self.choose_colour(colour),
            Err(_) => {
                let message = format!(
                    "Not a colour: \"{}\". Type #RRGGBB, e.g. #FF8000.",
                    self.colour_text
                );
                self.say(host, &message, true);
                self.show_colour(self.picked_colour);
            }
        }
    }

    /// Choose...: the colour picker opened on the form's colour. Nothing waits on it.
    pub fn open_colour_dialog(&mut self) {
        if !self.live() {
            return;
        }
        self.colour_dialog_open = true;
    }

    /// The colour picker accepted with `colour`.
    pub fn pick_colour(&mut self, colour: Color) {
        self.colour_dialog_open = false;
        if self.live() {
            self.choose_colour(colour);
        }
    }

    /// Save Changes: the form written to the layer, one command - or none, for a form nobody
    /// edited.
    pub fn save(&mut self, host: &mut dyn Host) {
        if !self.live() {
            return;
        }
        let name = self.selected_layer();
        let Some(stored) = host.document().model().layers.find(&name).cloned() else {
            self.say(host, "Select a layer first.", true);
            return;
        };
        let mut changed = stored.clone();
        // Every name exactly as shown, which for a name no list holds is the stored name itself,
        // kept by its picker; numbers only when edited.
        changed.linetype = self.linetype.current_name();
        if self.weight_edited {
            changed.line_weight = self.weight;
        }
        if self.colour_edited {
            changed.color = self.picked_colour;
        }
        changed.hatch_pattern = self
            .hatch
            .current()
            .unwrap_or_else(|| stored.hatch_pattern.clone());
        changed.dimension_style = self
            .dimension
            .current()
            .unwrap_or_else(|| stored.dimension_style.clone());
        changed.visible = self.visible;
        changed.locked = self.locked;
        if changed == stored {
            // Not an edit, and so not an undo step (audit QT-01's shape).
            self.say(
                host,
                &format!("Nothing changed: layer {name} is as it was."),
                false,
            );
            return;
        }
        if self.run(
            host,
            update_layer(changed),
            &format!("Layer {name} updated."),
        ) {
            self.select_after_reload = Some(name);
        }
    }

    /// Revert: the form as the layer is.
    pub fn revert(&mut self, document: &Document) {
        self.load_selected(document);
    }

    // ---- the prompt row ------------------------------------------------------------------------

    fn ask_name(&mut self, question: &str, suggestion: &str, what_for: PromptFor) {
        if !self.live() {
            return;
        }
        self.prompt_label = String::from(question);
        self.prompt_name = String::from(suggestion);
        self.prompt = Some(what_for);
    }

    /// The prompt's name field, typed into.
    pub fn set_prompt_name(&mut self, text: &str) {
        self.prompt_name = String::from(text);
    }

    pub fn prompt_name(&self) -> &str {
        &self.prompt_name
    }

    /// OK, or Enter in the prompt's name.
    pub fn accept_prompt(&mut self, host: &mut dyn Host) {
        if !self.live() {
            return;
        }
        // Blanks either end of a typed path are never meant, and a layer level may not start or
        // end with one.
        let answer = String::from(self.prompt_name.trim_matches(|c: char| c.is_ascii_whitespace()));
        if answer.is_empty() {
            self.say(host, "Give a name first.", true);
            return;
        }
        let Some(what_for) = self.prompt.take() else {
            return;
        };
        match what_for {
            PromptFor::New => {
                let layer = Layer {
                    name: answer.clone(),
                    ..Layer::default()
                };
                if self.run(
                    host,
                    create_layer(layer),
                    &format!("Layer {answer} created."),
                ) {
                    self.select_after_reload = Some(answer);
                }
            }
            PromptFor::NewChild(under) => {
                let full = join_layer_path(&under, &answer);
                let layer = Layer {
                    name: full.clone(),
                    ..Layer::default()
                };
                if self.run(host, create_layer(layer), &format!("Layer {full} created.")) {
                    self.select_after_reload = Some(full);
                }
            }
            PromptFor::Move(from) => {
                if answer == from {
                    self.say(host, "Nothing changed: the path is the same.", false);
                    return;
                }
                if self.run(
                    host,
                    rename_layer(from.clone(), answer.clone()),
                    &format!(
                        "Layer {from} is now {answer}; everything nested under it and the \
                         entities on them came too."
                    ),
                ) {
                    self.select_after_reload = Some(answer);
                }
            }
        }
    }

    /// Cancel: the prompt put away, nothing asked.
    pub fn cancel_prompt(&mut self) {
        self.prompt = None;
    }

    // ---- the buttons ---------------------------------------------------------------------------

    /// New...
    pub fn ask_new(&mut self) {
        self.ask_name(
            "New layer (use / to nest, e.g. design/surface/tin1):",
            "",
            PromptFor::New,
        );
    }

    /// New Child...
    pub fn ask_new_child(&mut self, host: &mut dyn Host) {
        if !self.live() {
            return;
        }
        let under = self.selected_layer();
        if under.is_empty() {
            self.say(
                host,
                "Select the layer to nest the new one under first.",
                true,
            );
            return;
        }
        self.ask_name(
            &format!("New layer under {under}:"),
            "",
            PromptFor::NewChild(under.clone()),
        );
    }

    /// Rename or Move...
    pub fn ask_move(&mut self, host: &mut dyn Host) {
        if !self.live() {
            return;
        }
        let from = self.selected_layer();
        if from.is_empty() {
            self.say(host, "Select the layer to rename or move first.", true);
            return;
        }
        self.ask_name(
            &format!("New full path for {from} (use / to nest):"),
            &from,
            PromptFor::Move(from.clone()),
        );
    }

    /// Delete.
    pub fn delete(&mut self, host: &mut dyn Host) {
        if !self.live() {
            return;
        }
        let name = self.selected_layer();
        if name.is_empty() {
            self.say(host, "Select the layer to delete first.", true);
            return;
        }
        self.run(
            host,
            delete_layer(name.clone()),
            &format!("Layer {name} deleted."),
        );
    }

    /// Put Selection Here.
    pub fn assign(&mut self, host: &mut dyn Host) {
        if !self.live() {
            return;
        }
        let name = self.selected_layer();
        if name.is_empty() {
            self.say(
                host,
                "Select the layer to move the entities onto first.",
                true,
            );
            return;
        }
        // The drawing's selection as it is now, not as it was when the dialog opened: a non-modal
        // dialog sits beside the drawing.
        let ids = host.document().selection().ids();
        if ids.is_empty() {
            self.say(host, "Select the entities to move first.", true);
            return;
        }
        let count = ids.len();
        if self.run(
            host,
            set_entity_layer(ids, name.clone()),
            &format!("{count} entities moved to {name}."),
        ) {
            self.select_after_reload = Some(name);
        }
    }

    /// Make Current.
    pub fn make_current(&mut self, host: &mut dyn Host) {
        if !self.live() {
            return;
        }
        let name = self.selected_layer();
        if name.is_empty() {
            self.say(host, "Select the layer to make current first.", true);
            return;
        }
        // Not a command: the current layer is session state, like the selection, and is not on the
        // undo stack.
        if let Err(error) = host.document_mut().set_current_layer(&name) {
            self.say(host, &error.describe(), true);
            return;
        }
        self.say(host, &format!("Current layer is now {name}."), false);
    }

    // ---- the frame -----------------------------------------------------------------------------

    /// Draws the dialog for this frame - the drawing's changes since the last first; false once it
    /// is closed, by its Close button or its title bar's.
    pub fn ui(&mut self, ctx: &Context, host: &mut dyn Host) -> bool {
        // Only what the table shows moves it: the model, the library (which decides whether a
        // linetype is defined) and the current layer (which its row says).
        if let Some(changes) = self.watcher.deliver(host.document())
            && (changes.model || changes.library || changes.current)
        {
            self.reload(host.document());
        }
        let mut frame = std::mem::replace(&mut self.frame, DialogFrame::new(LAYER_MANAGER, TITLE));
        let mut closed = show_dialog(&mut frame, ctx, self, host, Self::body, Self::footer);
        self.frame = frame;
        closed |= std::mem::take(&mut self.closing);
        !closed
    }

    fn body(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.add_enabled_ui(!self.orphan, |ui| {
            // Read-only: every change goes through the form and a command, so nothing here can
            // run one from inside a view's own change - the shape of the layer-panel crash
            // recorded in docs/cad.md.
            let shown = table(
                ui,
                "layerTable",
                &HEADERS,
                &self.rows,
                self.selected,
                &[],
                None,
            );
            if let Some(row) = shown.chosen {
                self.select_row(host.document(), Some(row));
            }
            self.form(ui, host);
            self.prompt_row(ui, host);
            self.buttons(ui, host);
        });
    }

    fn form(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.group(|ui| {
            let heading = ui.strong("Definition");
            custom(
                ui,
                &heading,
                ControlRecord::new("layerForm", ControlKind::Other),
            );
            egui::Grid::new("layerForm").num_columns(2).show(ui, |ui| {
                ui.label("Linetype");
                // A name chosen in the picker is read at Save: nothing runs from the choice.
                let _ = self.linetype.ui(ui, "layerLinetype");
                ui.end_row();

                ui.label("Line weight");
                if controls::spin(ui, "layerWeight", &mut self.weight, WEIGHT).changed() {
                    self.weight_edited = true;
                }
                ui.end_row();

                ui.label("Colour");
                ui.horizontal(|ui| {
                    let field = controls::line_with(
                        ui,
                        "layerColourText",
                        &mut self.colour_text,
                        LineOptions {
                            placeholder: "#RRGGBB",
                            ..LineOptions::default()
                        },
                    )
                    .on_hover_text("Type a colour as #RRGGBB (or #RRGGBBAA), or choose one");
                    if field.lost_focus() {
                        self.finish_colour_text(host);
                    }
                    if controls::button(ui, "layerColour", "Choose...")
                        .on_hover_text("Pick the colour in a colour dialog")
                        .clicked()
                    {
                        self.open_colour_dialog();
                    }
                    if self.colour_dialog_open {
                        let picked = self.picked_colour;
                        let mut rgb = [picked.r, picked.g, picked.b];
                        let response = ui.color_edit_button_srgb(&mut rgb);
                        custom(
                            ui,
                            &response,
                            ControlRecord::new("layerColourDialog", ControlKind::Other),
                        );
                        if response.changed() {
                            self.choose_colour(Color::new(rgb[0], rgb[1], rgb[2], picked.a));
                        }
                    }
                });
                ui.end_row();

                ui.label("Hatch");
                self.hatch.ui(ui, "layerHatch");
                ui.end_row();

                ui.label("Dimension style");
                self.dimension.ui(ui, "layerDimensionStyle");
                ui.end_row();

                ui.label("");
                ui.horizontal(|ui| {
                    controls::check(ui, "layerVisible", "Visible", &mut self.visible);
                    controls::check(ui, "layerLocked", "Locked", &mut self.locked);
                });
                ui.end_row();
            });
        });
    }

    fn prompt_row(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let open = self.prompt.is_some();
        ui.scope(|ui| {
            if !open {
                ui.set_invisible();
            }
            let row = ui.horizontal(|ui| {
                controls::label(ui, "layerPromptLabel", &self.prompt_label);
                let name = controls::line(ui, "layerPromptName", &mut self.prompt_name);
                let entered =
                    name.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                if controls::button(ui, "layerPromptOk", "OK").clicked() || entered {
                    self.accept_prompt(host);
                }
                if controls::button(ui, "layerPromptCancel", "Cancel").clicked() {
                    self.cancel_prompt();
                }
            });
            custom(
                ui,
                &row.response,
                ControlRecord::new("layerPrompt", ControlKind::Other),
            );
        });
    }

    fn buttons(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.horizontal_wrapped(|ui| {
            if controls::button(ui, "layerNew", "New...")
                .on_hover_text("A new layer; a name with / nests it")
                .clicked()
            {
                self.ask_new();
            }
            if controls::button(ui, "layerNewChild", "New Child...")
                .on_hover_text("A new layer under the selected one")
                .clicked()
            {
                self.ask_new_child(host);
            }
            if controls::button(ui, "layerSave", "Save Changes")
                .on_hover_text("Write the form to the layer")
                .clicked()
            {
                self.save(host);
            }
            if controls::button(ui, "layerRevert", "Revert")
                .on_hover_text("Put the form back as the layer is")
                .clicked()
            {
                self.revert(host.document());
            }
            if controls::button(ui, "layerMove", "Rename or Move...")
                .on_hover_text(
                    "A layer name is a path, so renaming it to another path moves it, with \
                     everything nested under it and the entities on them",
                )
                .clicked()
            {
                self.ask_move(host);
            }
            if controls::button(ui, "layerDelete", "Delete")
                .on_hover_text("Refused while entities are on it or layers are under it")
                .clicked()
            {
                self.delete(host);
            }
            if controls::button(ui, "layerAssign", "Put Selection Here")
                .on_hover_text("Move the entities selected in the drawing onto this layer")
                .clicked()
            {
                self.assign(host);
            }
            if controls::button(ui, "layerCurrent", "Make Current")
                .on_hover_text("Draw new work on this layer")
                .clicked()
            {
                self.make_current(host);
            }
        });
    }

    fn footer(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        let mut said = egui::RichText::new(self.status.as_str());
        if self.status_is_error {
            said = said.color(ERROR_COLOUR);
        }
        controls::label_as(ui, "layerStatus", &self.status, egui::Label::new(said).wrap());
        if controls::button(ui, "closeButton", "Close").clicked() {
            self.closing = true;
        }
    }
}
