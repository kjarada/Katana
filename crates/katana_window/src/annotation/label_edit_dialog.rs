//! Annotate > Edit Label... (docs/annotation.md, "In the window"): one label's style, its own
//! words, where its text is pinned and its layer - what LABEL SET changes. The port of
//! annotation/label_edit_dialog.
//!
//! The dialog changes nothing itself. OK and Apply write the LABEL SET line its fields describe -
//! shown as it will run in labelEditCommand - naming only what differs from the label as it is,
//! and hand it to the window's one executor (host::Host::run), so the line is echoed, kept in the
//! history and one undo step like a typed one; the reply or the refusal comes back into
//! labelEditStatus. There is nothing it can do that an agent cannot do by typing the line it shows.
//! The line is made by `label_set_line`, a pure function of the fields, tested without a window.
//!
//! It opens on the selected label (the action annotateEditLabel). With no label selected, or more
//! than one entity, it says so in labelEditStatus and its buttons are disabled; it never asks in a
//! box, so a headless run drives it like any other.
//!
//! Non-modal, it follows the drawing while it is open (`follow`): a label changed from elsewhere -
//! an undo, a typed LABEL SET - is shown afresh, the edits in the form with it, as Edit Text does,
//! so Apply never names only the differences from a label the model no longer holds; a label
//! erased leaves the form empty and disabled; and a new or opened drawing empties it too, since
//! the id it holds is another drawing's. Left open across an OPEN, it once wrote its label's edits
//! onto whatever the opened project held at that id.
//!
//! Object names:
//!
//! ```text
//! labelEditDialog     the dialog
//! labelEditTarget     which label, and what it labels (read-only)
//! labelEditStyle      the label style
//! labelEditOverride   ticked: the label says labelEditText instead of its style's words;
//!                     unticked: text=none
//! labelEditText       its own words
//! labelEditPinned     ticked: the text is pinned at labelEditEasting, labelEditNorthing;
//!                     unticked: at=none, the placer puts it
//! labelEditEasting, labelEditNorthing
//! labelEditLayer      the layer the label is on
//! labelEditCommand    the line OK and Apply run (read-only)
//! labelEditOk         run the line, and close when it worked
//! labelEditApply      run the line and stay open
//! labelEditCancel     close without running anything
//! labelEditStatus     what happened last
//! ```

use egui::{Context, Ui};
use katana_cad::{Document, DocumentChange};
use katana_core::error::make_error;
use katana_core::text::parse_finite_double;
use katana_core::{Error, ErrorCode, Result};
use katana_entity::entity::to_string;
use katana_entity::{EntityId, Geometry, Model};

use super::dialog_support::{Followed, show_dialog};
use crate::command_word::{annotation_text_word, command_word, exact_number};
use crate::drive::controls::{self, Choice, LineOptions};
use crate::host::Host;
use crate::shell::DialogFrame;

const DIALOG: &str = "labelEditDialog";
const TITLE: &str = "Edit Label";

/// What the dialog's fields hold, as typed; also what a label is when it is loaded, to compare
/// the fields with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LabelEditForm {
    pub id: EntityId,
    pub style: String,
    /// The label says `text` instead of its style's words.
    pub r#override: bool,
    pub text: String,
    /// The text is at easting, northing.
    pub pinned: bool,
    pub easting: String,
    pub northing: String,
    pub layer: String,
}

fn number(text: &str) -> Option<f64> {
    parse_finite_double(text.trim())
}

/// The label as `LabelEditForm` holds it, from the model: its style, its own text, its pinned
/// place - its anchor when it has none, so ticking Pinned starts from where the label is attached
/// - and its layer. NotFound or InvalidArgument when `id` is not a label.
pub fn label_edit_form_of(model: &Model, id: EntityId) -> Result<LabelEditForm> {
    let Some(entity) = model.entities.find(id) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "entity does not exist",
            format!("id={id}"),
        ));
    };
    let Geometry::Label(label) = &entity.geometry else {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "that entity is not a label",
            format!("id={id}"),
        ));
    };
    let at = label.position.unwrap_or(label.anchor);
    Ok(LabelEditForm {
        id,
        style: label.style.clone(),
        r#override: !label.text_override.is_empty(),
        text: label.text_override.clone(),
        pinned: label.position.is_some(),
        easting: exact_number(at.x),
        northing: exact_number(at.y),
        layer: entity.layer.clone(),
    })
}

/// The LABEL SET line that makes label `form.id` what `form` says, naming only what differs from
/// `current` (the label as it is):
///
/// ```text
/// LABEL SET id [style=name] [text="words"|none] [at=x,y|none] [layer=name]
/// ```
///
/// A word holding a blank is double-quoted, as the command line reads it, and a line break in the
/// text is written \n, which the verb reads back as one. An empty string when nothing differs.
/// InvalidArgument, naming the field, for no label, an empty style or layer, own text ticked but
/// empty or the bare word "none" (which the verb reads as "no own text"), a coordinate that is not
/// a number, and a double quote anywhere - the command line has no way to write one inside a
/// quoted word.
pub fn label_set_line(form: &LabelEditForm, current: &LabelEditForm) -> Result<String> {
    if form.id == 0 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "no label is chosen: select one label first",
        ));
    }
    let style = form.style.trim();
    let layer = form.layer.trim();
    if style.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "Style: choose a label style",
        ));
    }
    if layer.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "Layer: choose the layer the label is on",
        ));
    }
    // Every field is written, even one left as it was, so a value no line can carry is refused
    // whichever field changed.
    let style_word = command_word(style, "Style")?;
    let layer_word = command_word(layer, "Layer")?;

    let mut words = vec![
        String::from("LABEL"),
        String::from("SET"),
        format!("{}", form.id),
    ];
    if style != current.style {
        words.push(format!("style={style_word}"));
    }
    if form.r#override {
        if form.text.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "Own text: type the label's words, or untick Own text for its style's",
            ));
        }
        if form.text.trim().eq_ignore_ascii_case("none") {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "Own text: the word none on its own means \"no own text\" to LABEL SET; untick \
                 Own text for the style's words",
            ));
        }
        // LABEL SET's text= reads "\n" as a line break, as TEXT's does, so the text is written as
        // the annotation verbs' texts are - and one holding a typed backslash and n, which would
        // come back a break, is refused.
        let text_word = annotation_text_word(&form.text, "Own text")?;
        if !current.r#override || form.text != current.text {
            words.push(format!("text={text_word}"));
        }
    } else if current.r#override {
        words.push(String::from("text=none"));
    }
    if form.pinned {
        let x = number(&form.easting);
        let y = number(&form.northing);
        let Some(x) = x else {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("Easting: '{}' is not a number", form.easting),
            ));
        };
        let Some(y) = y else {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("Northing: '{}' is not a number", form.northing),
            ));
        };
        let was_x = number(&current.easting);
        let was_y = number(&current.northing);
        if !current.pinned || was_x != Some(x) || was_y != Some(y) {
            words.push(format!("at={},{}", exact_number(x), exact_number(y)));
        }
    } else if current.pinned {
        words.push(String::from("at=none"));
    }
    if layer != current.layer {
        words.push(format!("layer={layer_word}"));
    }
    if words.len() == 3 {
        return Ok(String::new());
    }
    Ok(words.join(" "))
}

/// The Edit Label dialog (see the module comment).
pub struct LabelEditDialog {
    frame: DialogFrame,
    /// False for a dialog given no command line (the C++'s empty runner).
    command_line: bool,
    followed: Followed,
    current: LabelEditForm,
    target: String,
    styles: Vec<String>,
    style: Choice,
    r#override: bool,
    text: String,
    pinned: bool,
    easting: String,
    northing: String,
    layers: Vec<String>,
    layer: Choice,
    command: String,
    status: String,
    ok_pressed: bool,
    apply_pressed: bool,
    cancel_pressed: bool,
}

impl LabelEditDialog {
    /// The dialog over `document`, empty until a label is loaded; its lines run through the host
    /// it is drawn with.
    pub fn new(document: &Document) -> Self {
        Self {
            frame: DialogFrame::new(DIALOG, TITLE),
            command_line: true,
            followed: Followed::new(document),
            current: LabelEditForm::default(),
            target: String::new(),
            styles: Vec::new(),
            style: Choice::default(),
            r#override: false,
            text: String::new(),
            pinned: false,
            easting: String::new(),
            northing: String::new(),
            layers: Vec::new(),
            layer: Choice::editable(""),
            command: String::new(),
            status: String::new(),
            ok_pressed: false,
            apply_pressed: false,
            cancel_pressed: false,
        }
    }

    /// The same dialog given no command line to run on.
    pub fn without_command_line(mut self) -> Self {
        self.command_line = false;
        self
    }

    /// What the drawing did since the dialog last looked, seen now (dialog_support).
    fn look(&mut self, document: &Document) {
        let change = self.followed.take();
        if change.parts != 0 {
            self.follow(document, &change);
        }
    }

    /// What a change to the drawing means for the label shown.
    fn follow(&mut self, document: &Document, change: &DocumentChange<'_>) {
        if self.current.id == 0 {
            return;
        }
        // Another drawing: an id there is another label, or none.
        if change.has(DocumentChange::REPLACED) {
            self.clear(
                document,
                "The drawing was replaced; select one label, then choose Edit Label.",
            );
            return;
        }
        if !change.has(DocumentChange::ENTITIES) {
            return;
        }
        match label_edit_form_of(document.model(), self.current.id) {
            Err(_) => {
                let why = format!(
                    "Label {} is no longer in the drawing; select one label, then choose Edit \
                     Label.",
                    self.current.id
                );
                self.clear(document, &why);
            }
            // The label as it now is, what is typed in the form with it: Apply compares the
            // fields with `current`, which has to be the label the model holds. Another entity's
            // change leaves the form alone.
            Ok(now) => {
                if now != self.current {
                    let id = self.current.id;
                    self.load_form(document, id);
                }
            }
        }
    }

    /// No label: the form emptied and disabled, `why` in the status.
    fn clear(&mut self, document: &Document, why: &str) {
        self.current = LabelEditForm::default();
        let empty = self.current.clone();
        self.show_form(document, &empty);
        self.target = String::from("-");
        self.command.clear();
        self.status = String::from(why);
    }

    /// Shows label `id`; false, with the reason in the status, when it is not a label.
    pub fn load(&mut self, document: &Document, id: EntityId) -> bool {
        self.look(document);
        self.load_form(document, id)
    }

    fn load_form(&mut self, document: &Document, id: EntityId) -> bool {
        let loaded = match label_edit_form_of(document.model(), id) {
            Ok(loaded) => loaded,
            Err(error) => {
                let why = format!("{} ({})", error.message, error.context);
                self.clear(document, &why);
                return false;
            }
        };
        self.current = loaded;
        let model = document.model();
        let Some(Geometry::Label(label)) = model.entities.find(id).map(|entity| &entity.geometry)
        else {
            return false;
        };
        let mut what = if label.target != 0 {
            let mut what = format!("entity {}", label.target);
            if let Some(target) = model.entities.find(label.target) {
                what.push_str(&format!(" ({})", to_string(target.r#type())));
            }
            what
        } else {
            format!("alignment {}", label.alignment)
        };
        if label.part >= 0 {
            what.push_str(&format!(", segment {}", label.part));
        }
        self.target = format!("Label {id} of {what}");
        let current = self.current.clone();
        self.show_form(document, &current);
        self.status.clear();
        self.update_line();
        true
    }

    /// The selection's label: false, with the reason in the status, unless exactly one entity is
    /// selected and it is a label.
    pub fn load_selection(&mut self, document: &Document) -> bool {
        self.look(document);
        let ids = document.selection().ids();
        if ids.len() != 1 {
            let why = if ids.is_empty() {
                String::from("Select one label, then choose Edit Label.")
            } else {
                format!(
                    "{} entities are selected; select one label, then choose Edit Label.",
                    ids.len()
                )
            };
            self.clear(document, &why);
            return false;
        }
        self.load_form(document, ids[0])
    }

    fn show_form(&mut self, document: &Document, form: &LabelEditForm) {
        let model = document.model();
        self.styles = model.label_styles.names();
        // A label whose style has gone still shows it, so OK does not change it unasked; LABEL
        // SET refuses nothing for it, the painter draws nothing.
        if !form.style.is_empty() && !self.styles.contains(&form.style) {
            self.styles.push(form.style.clone());
        }
        self.style = Choice {
            index: self.styles.iter().position(|name| *name == form.style),
            ..Choice::default()
        };
        self.layers = model.layers.names();
        self.layer = match self.layers.iter().position(|name| *name == form.layer) {
            Some(at) => {
                let mut layer = Choice::editable("");
                layer.choose(at, &self.layers);
                layer
            }
            None => Choice::editable(&form.layer),
        };
        self.r#override = form.r#override;
        self.text.clone_from(&form.text);
        self.pinned = form.pinned;
        self.easting.clone_from(&form.easting);
        self.northing.clone_from(&form.northing);
    }

    /// What the fields hold now.
    pub fn form(&self) -> LabelEditForm {
        LabelEditForm {
            id: self.current.id,
            style: self.style.current_text(&self.styles),
            r#override: self.r#override,
            text: self.text.clone(),
            pinned: self.pinned,
            easting: self.easting.clone(),
            northing: self.northing.clone(),
            layer: self.layer.current_text(&self.layers),
        }
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    /// The line Apply would run now; empty when nothing differs or the fields do not make one
    /// (the status then says why).
    pub fn line(&self) -> String {
        label_set_line(&self.form(), &self.current).unwrap_or_default()
    }

    fn update_line(&mut self) {
        if self.current.id == 0 {
            self.command.clear();
            return;
        }
        match label_set_line(&self.form(), &self.current) {
            Err(error) => {
                self.command.clear();
                self.status = error.message;
            }
            Ok(made) => {
                self.status = if made.is_empty() {
                    String::from("Nothing to change.")
                } else {
                    String::new()
                };
                self.command = made;
            }
        }
    }

    /// Runs the line through the host. True when it ran and was carried out, or when there was
    /// nothing to change; the label is then shown afresh.
    pub fn apply(&mut self, host: &mut dyn Host) -> bool {
        self.look(host.document());
        let made = match label_set_line(&self.form(), &self.current) {
            Ok(made) => made,
            Err(error) => {
                self.status = error.message;
                return false;
            }
        };
        if made.is_empty() {
            self.status = String::from("Nothing to change.");
            return true;
        }
        if !self.command_line {
            self.status = String::from("There is no command line to run it on.");
            return false;
        }
        let outcome = host.run(&made);
        // What the line changed is heard as it ran, as the C++'s listener heard it.
        self.look(host.document());
        if !outcome.ok {
            self.status = if outcome.error.is_empty() {
                String::from("The line was refused.")
            } else {
                outcome.error
            };
            return false;
        }
        // The label as it now is, so the next Apply compares against it.
        let id = self.current.id;
        self.load_form(host.document(), id);
        self.status = if outcome.reply.is_empty() {
            String::from("Done.")
        } else {
            outcome.reply
        };
        true
    }

    /// Draws the dialog for this frame; false once it is closed (OK that worked, Cancel, or the
    /// title bar's close button).
    pub fn ui(&mut self, ctx: &Context, host: &mut dyn Host) -> bool {
        self.look(host.document());
        let mut frame = std::mem::replace(&mut self.frame, DialogFrame::new(DIALOG, TITLE));
        let closed = show_dialog(&mut frame, ctx, self, host, Self::body, Self::footer);
        self.frame = frame;
        let mut open = !closed;
        if std::mem::take(&mut self.apply_pressed) {
            self.apply(host);
        }
        if std::mem::take(&mut self.ok_pressed) && self.apply(host) {
            open = false;
        }
        if std::mem::take(&mut self.cancel_pressed) {
            open = false;
        }
        open
    }

    fn body(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        let enabled = self.current.id != 0;
        let mut changed = false;
        egui::Grid::new("labelEditFields")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Label");
                controls::label(ui, "labelEditTarget", &self.target);
                ui.end_row();

                ui.label("Style");
                ui.add_enabled_ui(enabled, |ui| {
                    changed |=
                        controls::choice(ui, "labelEditStyle", &self.styles, &mut self.style)
                            .on_hover_text(
                                "What the label says and where it goes (Format > Label Styles and \
                             Rules)",
                            )
                            .changed();
                });
                ui.end_row();

                ui.add_enabled_ui(enabled, |ui| {
                    changed |=
                        controls::check(ui, "labelEditOverride", "Own text", &mut self.r#override)
                            .on_hover_text(
                                "Say these words instead of the style's; unticked, the label says \
                             what its style works out from its target",
                            )
                            .changed();
                });
                ui.add_enabled_ui(enabled && self.r#override, |ui| {
                    changed |= controls::line(ui, "labelEditText", &mut self.text).changed();
                });
                ui.end_row();

                ui.add_enabled_ui(enabled, |ui| {
                    changed |= controls::check(
                        ui,
                        "labelEditPinned",
                        "Pinned at",
                        &mut self.pinned,
                    )
                    .on_hover_text(
                        "Put the text at this point, with a leader back when its style draws \
                             one; unticked, the placer finds it room at every scale",
                    )
                    .changed();
                });
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(enabled && self.pinned, |ui| {
                        for (name, value, placeholder) in [
                            ("labelEditEasting", &mut self.easting, "Easting"),
                            ("labelEditNorthing", &mut self.northing, "Northing"),
                        ] {
                            changed |= controls::line_with(
                                ui,
                                name,
                                value,
                                LineOptions {
                                    placeholder,
                                    ..LineOptions::default()
                                },
                            )
                            .changed();
                        }
                    });
                });
                ui.end_row();

                ui.label("Layer");
                ui.add_enabled_ui(enabled, |ui| {
                    changed |=
                        controls::choice(ui, "labelEditLayer", &self.layers, &mut self.layer)
                            .changed();
                });
                ui.end_row();

                ui.label("Command");
                controls::line_with(
                    ui,
                    "labelEditCommand",
                    &mut self.command,
                    LineOptions {
                        read_only: true,
                        ..LineOptions::default()
                    },
                )
                .on_hover_text("The line OK and Apply run, as it would be typed");
                ui.end_row();
            });
        if changed {
            self.update_line();
        }
    }

    fn footer(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        let enabled = self.current.id != 0;
        controls::label(ui, "labelEditStatus", &self.status);
        ui.horizontal(|ui| {
            ui.add_enabled_ui(enabled, |ui| {
                if controls::button(ui, "labelEditOk", "OK").clicked() {
                    self.ok_pressed = true;
                }
                if controls::button(ui, "labelEditApply", "Apply").clicked() {
                    self.apply_pressed = true;
                }
            });
            if controls::button(ui, "labelEditCancel", "Cancel").clicked() {
                self.cancel_pressed = true;
            }
        });
    }
}
