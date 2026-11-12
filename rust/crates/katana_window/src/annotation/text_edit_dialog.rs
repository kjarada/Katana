//! Annotate > Edit Text... (docs/annotation.md, "In the window"): the one text selected in the
//! drawing, every field of it in a form - its words over several lines, style, height on paper,
//! model height, justification, rotation and position. The port of annotation/text_edit_dialog.
//!
//! Non-modal and following the selection: with no text, or more than one thing, selected, it says
//! so and waits for one, rather than asking in a box of its own - so a headless run is never
//! stopped by it. It changes nothing itself: Apply builds ONE TEXTEDIT line of the fields the form
//! changed and runs it through the window's one executor (host::Host::run), so the edit is one
//! undo step, echoed in the command log, and refused in the verb's own words. A value no line can
//! carry (a double quote) is refused in the dialog before anything runs
//! (cad's annotation::command_words).
//!
//! Object names, for the headless driver and the tests:
//!
//! ```text
//! textEditDialog      the dialog
//! textEditStatus      which text it edits, or what to select
//! textEditText        the words, a line a line
//! textEditStyle       the text style, "(none)" for the plain face
//! textEditPaper       height on paper in mm; 0 for a model height
//! textEditHeight      model height (a paper-sized text's is worked out)
//! textEditJustify     TL TC TR ML MC MR BL BC BR
//! textEditRotation    degrees counter-clockwise from east
//! textEditPosition    the justification point, E,N
//! textEditApply, textEditRevert
//! textEditProblem     the last refusal
//! ```

use egui::{Context, RichText, Ui};
use katana_cad::annotation::command_words::{annotation_text_word, command_word};
use katana_cad::{Document, DocumentChange};
use katana_core::text::{format_general, parse_finite_double};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::entity::to_string;
use katana_entity::{EntityId, Geometry, TextGeometry, TextJustify};
use katana_math::numerics::RAD_TO_DEG;

use super::dialog_support::{Followed, show_dialog};
use super::form_widgets::describe;
use crate::drive::controls::{self, Choice, TextOptions};
use crate::host::Host;
use crate::shell::DialogFrame;
use crate::theme::palette;

const DIALOG: &str = "textEditDialog";
const TITLE: &str = "Edit Text";

/// The style box's first item: the plain face.
const NO_STYLE: &str = "(none)";

/// The nine, in reading order, by the names the verb takes.
const JUSTIFICATIONS: [TextJustify; 9] = [
    TextJustify::TopLeft,
    TextJustify::TopCentre,
    TextJustify::TopRight,
    TextJustify::MiddleLeft,
    TextJustify::MiddleCentre,
    TextJustify::MiddleRight,
    TextJustify::BottomLeft,
    TextJustify::BottomCentre,
    TextJustify::BottomRight,
];

/// A number as the form shows it: twelve significant figures (QString::number's 'g'), which holds
/// a survey coordinate to the millimetre and turns a stored 30.000000000000004 degrees into 30.
/// Only a changed field is sent, so the rounding never reaches the drawing.
fn shown(value: f64) -> String {
    format_general(value, 12)
}

/// `text` read as a number for `what`, refused unless `allowed`.
fn number_word(text: &str, what: &str, allowed: fn(f64) -> bool, rule: &str) -> Result<String> {
    let trimmed = text.trim();
    match parse_finite_double(trimmed) {
        Some(value) if allowed(value) => Ok(String::from(trimmed)),
        _ => Err(Error::new(
            ErrorCode::ParseFailure,
            format!("{what} must be {rule}, not '{trimmed}'"),
        )),
    }
}

/// The fields as the form shows them, to tell an edit from what is stored.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Shown {
    text: String,
    style: String,
    paper: String,
    height: String,
    justify: String,
    rotation: String,
    position: String,
}

/// The Edit Text dialog (see the module comment).
pub struct TextEditDialog {
    frame: DialogFrame,
    /// False for a dialog given no command line (the C++'s empty runner).
    command_line: bool,
    followed: Followed,
    status: String,
    text: String,
    styles: Vec<String>,
    style: Choice,
    paper: String,
    height: String,
    justifications: Vec<String>,
    justify: Choice,
    rotation: String,
    position: String,
    editable: bool,
    problem: String,
    id: EntityId,
    /// What was loaded, to keep the form's edits through a change that leaves the text alone and
    /// to replace them when it does not.
    loaded: TextGeometry,
    shown: Shown,
    apply_pressed: bool,
    revert_pressed: bool,
}

impl TextEditDialog {
    /// The dialog over `document`, on the text selected now if one is; its lines run through the
    /// host it is drawn with.
    pub fn new(document: &Document) -> Self {
        let mut dialog = Self {
            frame: DialogFrame::new(DIALOG, TITLE),
            command_line: true,
            followed: Followed::new(document),
            status: String::new(),
            text: String::new(),
            styles: Vec::new(),
            style: Choice::default(),
            paper: String::new(),
            height: String::new(),
            justifications: JUSTIFICATIONS
                .iter()
                .map(|&justify| String::from(to_string(justify)))
                .collect(),
            justify: Choice::default(),
            rotation: String::new(),
            position: String::new(),
            editable: false,
            problem: String::new(),
            id: 0,
            loaded: TextGeometry::default(),
            shown: Shown::default(),
            apply_pressed: false,
            revert_pressed: false,
        };
        dialog.fill_styles(document);
        dialog.follow(document);
        dialog
    }

    /// The same dialog given no command line to run on.
    pub fn without_command_line(mut self) -> Self {
        self.command_line = false;
        self
    }

    /// What the drawing did since the dialog last looked, seen now (dialog_support).
    fn look(&mut self, document: &Document) {
        let change = self.followed.take();
        if change.has(DocumentChange::ANNOTATION_STYLES | DocumentChange::REPLACED) {
            self.fill_styles(document);
        }
        if change
            .has(DocumentChange::SELECTION | DocumentChange::ENTITIES | DocumentChange::REPLACED)
        {
            self.follow(document);
        }
    }

    fn fill_styles(&mut self, document: &Document) {
        let keep = self.style.current_text(&self.styles);
        self.styles = vec![String::from(NO_STYLE)];
        document.model().text_styles.for_each(|style| {
            self.styles.push(style.name.clone());
        });
        let index = self.styles.iter().position(|name| *name == keep);
        self.style = Choice::at(index.unwrap_or(0));
    }

    fn follow(&mut self, document: &Document) {
        let ids = document.selection().ids();
        let text = if ids.len() == 1 {
            document
                .model()
                .entities
                .find(ids[0])
                .and_then(|entity| match &entity.geometry {
                    Geometry::Text(text) => Some(text),
                    _ => None,
                })
        } else {
            None
        };
        let Some(text) = text else {
            self.id = 0;
            self.status = if ids.is_empty() {
                String::from("Select one text in the drawing to edit it.")
            } else {
                format!(
                    "Select one text in the drawing to edit it: the selection is {} entit{}{}.",
                    ids.len(),
                    if ids.len() == 1 { "y" } else { "ies" },
                    if ids.len() == 1 {
                        " that is not a text"
                    } else {
                        ""
                    }
                )
            };
            self.editable = false;
            return;
        };
        // The same text, stored as it was loaded: keep what is being typed.
        if ids[0] == self.id && *text == self.loaded {
            return;
        }
        let text = text.clone();
        self.load(ids[0], &text);
    }

    fn load(&mut self, id: EntityId, text: &TextGeometry) {
        self.id = id;
        self.loaded = text.clone();
        self.status = format!("Text {id}");
        self.text.clone_from(&text.text);
        let style = self.styles.iter().position(|name| *name == text.style);
        self.style = Choice::at(match style {
            Some(at) if !text.style.is_empty() => at,
            _ => 0,
        });
        self.paper = shown(text.paper_height);
        self.height = shown(text.height);
        let justify = to_string(text.justify);
        self.justify = Choice {
            index: self.justifications.iter().position(|name| name == justify),
            ..Choice::default()
        };
        self.rotation = shown(text.rotation * RAD_TO_DEG);
        self.position = format!("{},{}", shown(text.position.x), shown(text.position.y));
        self.shown = self.form();
        self.problem.clear();
        self.editable = true;
    }

    fn form(&self) -> Shown {
        Shown {
            text: self.text.clone(),
            style: self.style.current_text(&self.styles),
            paper: String::from(self.paper.trim()),
            height: String::from(self.height.trim()),
            justify: self.justify.current_text(&self.justifications),
            rotation: String::from(self.rotation.trim()),
            position: String::from(self.position.trim()),
        }
    }

    /// The text being edited; 0 when the selection is not one text.
    pub fn text_id(&self) -> EntityId {
        self.id
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn problem(&self) -> &str {
        &self.problem
    }

    /// The line Apply runs: empty when the form changes nothing; an error naming a field that does
    /// not read, or when there is no text to edit.
    pub fn apply_line(&self) -> Result<String> {
        if self.id == 0 {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "select one text in the drawing first",
            ));
        }
        let now = self.form();
        let mut options = Vec::new();
        if now.text != self.shown.text {
            if now.text.trim().is_empty() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "a text must say something; erase it to remove it",
                ));
            }
            options.push(format!("text={}", annotation_text_word(&now.text)?));
        }
        if now.style != self.shown.style {
            let style = if now.style == NO_STYLE {
                ""
            } else {
                now.style.as_str()
            };
            options.push(format!("style={}", command_word(style)?));
        }
        if now.paper != self.shown.paper {
            let word = number_word(
                &now.paper,
                "the height on paper",
                |value| value >= 0.0,
                "0 or more millimetres",
            )?;
            options.push(format!("paper={word}"));
        }
        if now.height != self.shown.height {
            let word = number_word(
                &now.height,
                "the model height",
                |value| value > 0.0,
                "greater than zero",
            )?;
            options.push(format!("height={word}"));
        }
        if now.justify != self.shown.justify {
            options.push(format!("justify={}", now.justify));
        }
        if now.rotation != self.shown.rotation {
            let word = number_word(
                &now.rotation,
                "the rotation",
                |_| true,
                "a number of degrees",
            )?;
            options.push(format!("rotation={word}"));
        }
        if now.position != self.shown.position {
            let at: String = now.position.chars().filter(|&c| c != ' ').collect();
            let parts: Vec<&str> = at.split(',').collect();
            if parts.len() != 2
                || parse_finite_double(parts[0]).is_none()
                || parse_finite_double(parts[1]).is_none()
            {
                return Err(Error::new(
                    ErrorCode::ParseFailure,
                    format!("the position must be E,N, not '{}'", now.position),
                ));
            }
            options.push(format!("at={at}"));
        }
        if options.is_empty() {
            return Ok(String::new());
        }
        Ok(format!("TEXTEDIT {} {}", self.id, options.join(" ")))
    }

    /// Runs the line Apply makes through the host: true when it was carried out, or when the form
    /// changed nothing.
    pub fn apply(&mut self, host: &mut dyn Host) -> bool {
        self.look(host.document());
        let line = match self.apply_line() {
            Ok(line) => line,
            Err(error) => {
                self.problem = describe(&Err(error));
                return false;
            }
        };
        if line.is_empty() {
            self.problem.clear();
            return true; // nothing changed: no step
        }
        if !self.command_line {
            self.problem = String::from("there is no command line to run the TEXTEDIT line on");
            return false;
        }
        let outcome = host.run(&line);
        self.look(host.document());
        self.problem = if outcome.ok {
            String::new()
        } else if outcome.error.is_empty() {
            format!("refused: {line}")
        } else {
            outcome.error.clone()
        };
        outcome.ok
    }

    /// Throws the form's edits away.
    pub fn revert(&mut self) {
        self.problem.clear();
        if self.id != 0 {
            let loaded = self.loaded.clone();
            self.load(self.id, &loaded);
        }
    }

    /// Draws the dialog for this frame; false once its title bar's close button is clicked.
    pub fn ui(&mut self, ctx: &Context, host: &mut dyn Host) -> bool {
        self.look(host.document());
        let mut frame = std::mem::replace(&mut self.frame, DialogFrame::new(DIALOG, TITLE));
        let closed = show_dialog(&mut frame, ctx, self, host, Self::body, Self::footer);
        self.frame = frame;
        if std::mem::take(&mut self.revert_pressed) {
            self.revert();
        }
        if std::mem::take(&mut self.apply_pressed) {
            self.apply(host);
        }
        !closed
    }

    fn body(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        controls::label(ui, "textEditStatus", &self.status);
        ui.add_enabled_ui(self.editable, |ui| {
            egui::Grid::new("textEditFields")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label("Text");
                    controls::text_with(
                        ui,
                        "textEditText",
                        &mut self.text,
                        TextOptions {
                            rows: 3,
                            ..TextOptions::default()
                        },
                    )
                    .on_hover_text("The words, a line a line: one text of several lines");
                    ui.end_row();
                    ui.label("Style");
                    controls::choice(ui, "textEditStyle", &self.styles, &mut self.style)
                        .on_hover_text("The text style it is set in; (none) for the plain face");
                    ui.end_row();
                    let lines = [
                        (
                            "Height on paper (mm)",
                            "textEditPaper",
                            &mut self.paper,
                            "Height on paper in mm, drawn at the annotation scale; 0 for the \
                             style's, or a model height with no style",
                        ),
                        (
                            "Model height",
                            "textEditHeight",
                            &mut self.height,
                            "Height in model units; a paper-sized text's is worked out for the \
                             annotation scale",
                        ),
                    ];
                    for (title, name, value, tip) in lines {
                        ui.label(title);
                        controls::line(ui, name, value).on_hover_text(tip);
                        ui.end_row();
                    }
                    ui.label("Justification");
                    controls::choice(
                        ui,
                        "textEditJustify",
                        &self.justifications,
                        &mut self.justify,
                    )
                    .on_hover_text(
                        "The point the position is: T, M or B for top, middle or bottom (the \
                         baseline), then L, C or R for left, centre or right",
                    );
                    ui.end_row();
                    let lines = [
                        (
                            "Rotation (degrees)",
                            "textEditRotation",
                            &mut self.rotation,
                            "Degrees counter-clockwise from east",
                        ),
                        (
                            "Position (E,N)",
                            "textEditPosition",
                            &mut self.position,
                            "The justification point, as E,N",
                        ),
                    ];
                    for (title, name, value, tip) in lines {
                        ui.label(title);
                        controls::line(ui, name, value).on_hover_text(tip);
                        ui.end_row();
                    }
                });
        });
    }

    fn footer(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        controls::label_as(
            ui,
            "textEditProblem",
            &self.problem,
            egui::Label::new(
                RichText::new(self.problem.as_str()).color(palette::error().to_egui()),
            )
            .wrap(),
        );
        ui.horizontal(|ui| {
            ui.add_enabled_ui(self.editable, |ui| {
                if controls::button(ui, "textEditRevert", "Revert")
                    .on_hover_text("Throw the form's edits away")
                    .clicked()
                {
                    self.revert_pressed = true;
                }
                if controls::button(ui, "textEditApply", "Apply")
                    .on_hover_text("Store what the form changed, as one undo step (TEXTEDIT)")
                    .clicked()
                {
                    self.apply_pressed = true;
                }
            });
        });
    }
}
