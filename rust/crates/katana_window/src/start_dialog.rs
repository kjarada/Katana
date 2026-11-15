//! The start screen (docs/desktop.md, "The start screen"): the port of start_dialog. What this
//! session starts with, chosen as it starts - the customisation its survey codes are drawn by, and
//! the coordinate system the drawing is in. An interactive start with no project to open shows it,
//! until "Show this at start" is cleared; File > Start Screen opens it at any time.
//!
//! A thin front end over two verbs, as every dialog is: Start builds the lines - CUSTOMISE BUILTIN
//! <name>, or CUSTOMISE [REPLACE] "<file>", then CUSTOMISE KEEP when the choice is to be what every
//! start gives, then CRS SET <text> or CRS CLEAR - shows them (startLine) and hands them one by one
//! to the window's executor. Each is logged and undone as if it had been typed, and katana_cli and
//! katana_mcp make the same choices with the same lines. A choice that is what the session already
//! has makes no line.
//!
//! It reads no settings and writes none: the window gives it what was chosen last
//! (`StartChoices`) and keeps what is chosen now (`coordinate_system`, `show_at_start`). Non-modal,
//! so a headless run fills it by its object names.
//!
//! THE RUST SHAPE. The C++ dialog holds the Document and an optional runner; here the drawing is
//! lent to each call, and Start runs its lines through the host it is lent (`start`) or, with no
//! window, through an interpreter of its own on the drawing (`start_on`), the C++'s runner left
//! empty. A built-in that cannot be used is listed with why, and refused as a choice, where the C++
//! greyed its item.

use egui::{RichText, Ui};
use katana_cad::customisation_host::built_in_choices;
use katana_cad::project_crs::{CrsChoice, common_coordinate_systems, normalise_coordinate_system};
use katana_cad::{BuiltInEntry, CommandInterpreter, CustomisationOrigin, Document};
use katana_core::error::make_error;
use katana_core::{ErrorCode, Result};

use crate::canvas::Image;
use crate::command_word::command_word;
use crate::customisation::code_manager_support::typed_customisation_file_word;
use crate::drive::controls::{self, Choice, LineOptions};
use crate::host::{Host, VerbOutcome};
use crate::icons::{CanvasRasteriser, application_icon_image};
use crate::project_crs_dialog::crs_check_text;
use crate::shell::DialogFrame;
use crate::theme::palette;

/// What the window gives the screen of what was chosen last.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartChoices {
    /// The coordinate system chosen last, as the CRS verb takes it; empty: local coordinates.
    pub coordinate_system: String,
    /// Whether an interactive start shows the screen.
    pub show_at_start: bool,
    /// Where the window keeps the session's customisation between runs. Empty when it keeps none,
    /// and "Use it at every start" is then not offered.
    pub kept_file: String,
}

impl Default for StartChoices {
    fn default() -> Self {
        Self {
            coordinate_system: String::new(),
            show_at_start: true,
            kept_file: String::new(),
        }
    }
}

/// What a customisation item is: the session as it stands, a built-in by name, or a file to load.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Kind {
    Now,
    BuiltIn(String),
    File,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Item {
    text: String,
    kind: Kind,
    tip: String,
    /// False for a built-in that cannot be used: shown, and why, but not to be chosen.
    usable: bool,
}

/// The coordinate system item that is local coordinates.
const LOCAL: &str = "Local coordinates (no coordinate system)";

/// How a session that is no built-in as installed came to be, in words.
fn origin_words(origin: CustomisationOrigin) -> &'static str {
    match origin {
        CustomisationOrigin::Kept => "kept from the last session",
        CustomisationOrigin::Loaded => "loaded in this session",
        CustomisationOrigin::Edited => "edited in this session",
        CustomisationOrigin::BuiltIn => "built in",
        CustomisationOrigin::None => "",
    }
}

/// A common system as the list shows it: its code, then its name.
fn crs_item_text(choice: &CrsChoice) -> String {
    format!("{}  {}", choice.id, choice.name)
}

/// The start screen (see the module comment).
pub struct StartDialog {
    frame: DialogFrame,
    visible: bool,
    /// Accepted (Start ran every line), rejected (Skip, or closed), or neither yet.
    result: Option<bool>,
    kept_file: String,
    /// The built-in the session is as installed; empty when it is none.
    active_built_in: String,
    items: Vec<Item>,
    customisation: Choice,
    file: String,
    replace: bool,
    keep: bool,
    /// The list's items: the text shown and the system's code ("" for local coordinates).
    crs_items: Vec<(String, String)>,
    crs: Choice,
    status: String,
    show_at_start: bool,
    logo: Option<egui::TextureHandle>,
}

impl StartDialog {
    /// The screen over `document`'s session as it is now, offering `built_ins` - those compiled
    /// into the program (katana_cad's built_ins) unless a test says otherwise.
    pub fn new(document: &Document, last: StartChoices, built_ins: &[BuiltInEntry]) -> Self {
        let state = document.customisation_state();
        let choices = built_in_choices(document, built_ins);
        let active_built_in = choices
            .iter()
            .find(|choice| choice.is_active)
            .map(|choice| choice.name.clone())
            .unwrap_or_default();
        let mut items = Vec::new();
        // A session that is no built-in as installed - a kept copy, a file loaded, or none at all -
        // is offered first as it stands, which makes no line: Start on the screen as it opens
        // changes nothing.
        if active_built_in.is_empty() {
            let text = if state.origin == CustomisationOrigin::None {
                String::from("None - as it is, no customisation installed")
            } else {
                format!("{} - as it is, {}", state.name, origin_words(state.origin))
            };
            items.push(Item {
                text,
                kind: Kind::Now,
                tip: String::new(),
                usable: true,
            });
        }
        let mut selected = (!items.is_empty()).then_some(0);
        for choice in &choices {
            let at = items.len();
            items.push(Item {
                text: format!(
                    "{}{}",
                    choice.name,
                    if choice.is_default {
                        " (built in, the default)"
                    } else {
                        " (built in)"
                    }
                ),
                kind: Kind::BuiltIn(choice.name.clone()),
                tip: if choice.problem.is_empty() {
                    choice.description.clone()
                } else {
                    choice.problem.clone()
                },
                usable: choice.problem.is_empty(),
            });
            if choice.is_active
                || (selected.is_none() && choice.is_default && choice.problem.is_empty())
            {
                selected = Some(at);
            }
        }
        items.push(Item {
            text: String::from("From a customisation file..."),
            kind: Kind::File,
            tip: String::new(),
            usable: true,
        });

        let mut crs_items = vec![(String::from(LOCAL), String::new())];
        for choice in common_coordinate_systems() {
            crs_items.push((crs_item_text(choice), choice.id.clone()));
        }
        // The drawing's own when it has one, else the one chosen last.
        let now = document.metadata().coordinate_system.clone();
        let wanted = if now.is_empty() {
            String::from(last.coordinate_system.trim())
        } else {
            now
        };
        let crs = if wanted.is_empty() {
            Choice::at(0)
        } else {
            match crs_items.iter().position(|(_, id)| *id == wanted) {
                Some(at) => Choice::at(at),
                None => Choice::editable(&wanted),
            }
        };
        let mut dialog = Self {
            frame: DialogFrame::new("startDialog", "Start"),
            visible: true,
            result: None,
            // As Settings' Use keeps what was kept: a session that the next start would give
            // again stays so.
            keep: !last.kept_file.is_empty() && state.kept,
            kept_file: last.kept_file,
            active_built_in,
            items,
            customisation: Choice::at(selected.unwrap_or(0)),
            file: String::new(),
            replace: false,
            crs_items,
            crs,
            status: String::new(),
            show_at_start: last.show_at_start,
            logo: None,
        };
        dialog.sync_crs_text();
        dialog
    }

    /// An editable choice shows its item's text when an item is chosen.
    fn sync_crs_text(&mut self) {
        if let Some(at) = self.crs.index
            && let Some((text, _)) = self.crs_items.get(at)
        {
            self.crs.text.clone_from(text);
        }
    }

    fn kind(&self) -> Kind {
        self.customisation
            .index
            .and_then(|at| self.items.get(at))
            .map_or(Kind::Now, |item| item.kind.clone())
    }

    // ---- what a test and a driver set and read -----------------------------------------------

    /// The customisation list's items, as shown.
    pub fn customisation_items(&self) -> Vec<String> {
        self.items.iter().map(|item| item.text.clone()).collect()
    }

    pub fn customisation_index(&self) -> Option<usize> {
        self.customisation.index
    }

    /// Chooses the customisation item at `index`, as a person chooses it; an item that cannot be
    /// used is not chosen.
    pub fn set_customisation(&mut self, index: usize) {
        if self.items.get(index).is_some_and(|item| item.usable) {
            self.customisation = Choice::at(index);
        }
    }

    pub fn set_file(&mut self, text: &str) {
        self.file = String::from(text);
    }

    pub fn set_replace(&mut self, replace: bool) {
        self.replace = replace;
    }

    pub fn set_keep(&mut self, keep: bool) {
        self.keep = keep;
    }

    pub fn keep_checked(&self) -> bool {
        self.keep
    }

    /// The file's field, Browse and Replace are for a file chosen.
    pub fn file_enabled(&self) -> bool {
        self.kind() == Kind::File
    }

    /// Keeping is for a choice that changes the session, in a window that keeps one.
    pub fn keep_enabled(&self) -> bool {
        let changes = match self.kind() {
            Kind::File => true,
            Kind::BuiltIn(name) => name != self.active_built_in,
            Kind::Now => false,
        };
        !self.kept_file.is_empty() && changes
    }

    /// The coordinate system item chosen: QComboBox::currentIndex, None when what is shown was
    /// typed.
    pub fn crs_index(&self) -> Option<usize> {
        self.crs.index
    }

    /// What the coordinate system box shows.
    pub fn crs_text(&self) -> &str {
        &self.crs.text
    }

    /// The coordinate system item at `index` chosen.
    pub fn set_crs_index(&mut self, index: usize) {
        if index < self.crs_items.len() {
            self.crs = Choice::at(index);
            self.sync_crs_text();
        }
    }

    /// The coordinate system box's text set as typed (QComboBox::setEditText).
    pub fn set_crs_text(&mut self, text: &str) {
        self.crs = Choice::editable(text);
        self.crs.index = self.crs_items.iter().position(|(shown, _)| shown == text);
    }

    /// The check line under the box: what the system typed is.
    pub fn crs_check(&self) -> String {
        crs_check_text(&self.coordinate_system(), None)
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    /// Accepted when Start ran every line; rejected by Skip or a close; None while it is open.
    pub fn result(&self) -> Option<bool> {
        self.result
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn show(&mut self) {
        self.visible = true;
        self.result = None;
    }

    /// The coordinate system chosen, as the CRS verb takes it (empty: local coordinates), for the
    /// window to keep. An item of the list chosen and left as the list wrote it is its code;
    /// anything else is what was typed.
    pub fn coordinate_system(&self) -> String {
        let typed = self.crs.text.trim();
        if let Some((_, id)) = self.crs_items.iter().find(|(shown, _)| shown == typed) {
            return id.clone();
        }
        if typed == LOCAL {
            String::new()
        } else {
            String::from(typed)
        }
    }

    /// Whether the screen is to show at the next start: for the window to keep.
    pub fn show_at_start(&self) -> bool {
        self.show_at_start
    }

    pub fn set_show_at_start(&mut self, show: bool) {
        self.show_at_start = show;
    }

    // ---- the lines ---------------------------------------------------------------------------

    /// The lines the choices make, in the order Start runs them; none when they change nothing.
    /// What is wrong with the choices, when something is: no file named, a coordinate system that
    /// is none.
    pub fn lines(&self, document: &Document) -> Result<Vec<String>> {
        let mut out = Vec::new();
        let mut changed = false;
        match self.kind() {
            Kind::BuiltIn(name) => {
                if name != self.active_built_in {
                    let word = command_word(&name, "the built-in customisation")?;
                    out.push(format!("CUSTOMISE BUILTIN {word}"));
                    changed = true;
                }
            }
            Kind::File => {
                let word = typed_customisation_file_word(&self.file)?;
                out.push(format!(
                    "{}{word}",
                    if self.replace {
                        "CUSTOMISE REPLACE "
                    } else {
                        "CUSTOMISE "
                    }
                ));
                changed = true;
            }
            Kind::Now => {}
        }
        if changed && self.keep_enabled() && self.keep {
            out.push(String::from("CUSTOMISE KEEP"));
        }

        let chosen = self.coordinate_system();
        let mut stored = String::new();
        if !chosen.is_empty() {
            stored = normalise_coordinate_system(&chosen).map_err(|error| {
                make_error(
                    ErrorCode::InvalidCrs,
                    format!("the coordinate system: {}", error.message),
                    chosen.clone(),
                )
            })?;
        }
        if stored != document.metadata().coordinate_system {
            // The text goes on the line as typed: CRS SET reads everything after SET verbatim.
            out.push(if chosen.is_empty() {
                String::from("CRS CLEAR")
            } else {
                format!("CRS SET {chosen}")
            });
        }
        Ok(out)
    }

    /// What startLine says: the lines, one a line; why there are none; or what is wrong.
    pub fn line_text(&self, document: &Document) -> String {
        match self.lines(document) {
            Ok(lines) if lines.is_empty() => {
                String::from("Nothing to change: the session starts as it is.")
            }
            Ok(lines) => lines.join("\n"),
            Err(error) => error.message,
        }
    }

    /// Start is offered while the choices make lines that can run.
    pub fn start_enabled(&self, document: &Document) -> bool {
        self.lines(document).is_ok()
    }

    /// Runs the lines through the window's executor, stopping at the first refused, every reply
    /// in startStatus; the screen closes when all ran. False when a line was refused or the
    /// choices are wrong (said in startStatus too).
    pub fn start(&mut self, host: &mut dyn Host) -> bool {
        let made = self.lines(host.document());
        self.run_lines(made, &mut |line| host.run(line))
    }

    /// Start with no window: the lines through an interpreter on `document`, the same verbs.
    pub fn start_on(&mut self, document: &mut Document) -> bool {
        let made = self.lines(document);
        let mut interpreter = CommandInterpreter::new();
        self.run_lines(made, &mut |line| match interpreter.run(document, line) {
            Ok(reply) => VerbOutcome::new(true, &reply, ""),
            Err(error) => VerbOutcome::new(false, "", &error.describe()),
        })
    }

    fn run_lines(
        &mut self,
        made: Result<Vec<String>>,
        run: &mut dyn FnMut(&str) -> VerbOutcome,
    ) -> bool {
        let lines = match made {
            Ok(lines) => lines,
            Err(error) => {
                self.status = format!("Not started: {}", error.message);
                return false;
            }
        };
        let mut said = Vec::new();
        for line in &lines {
            let outcome = run(line);
            said.push(format!("> {line}"));
            if !outcome.ok {
                said.push(outcome.error);
                self.status = said.join("\n");
                return false;
            }
            if let Some(first) = outcome.reply.lines().next().filter(|first| !first.is_empty()) {
                said.push(String::from(first));
            }
        }
        self.status = said.join("\n");
        self.result = Some(true);
        self.visible = false;
        true
    }

    /// Skip: the session as it is.
    pub fn skip(&mut self) {
        self.result = Some(false);
        self.visible = false;
    }

    // ---- the frame ---------------------------------------------------------------------------

    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.visible {
            return;
        }
        let ctx = ui.ctx().clone();
        let mut frame = self.frame.clone();
        let mut lent = (&mut *self, &mut *host);
        let shown = frame.show(
            &ctx,
            &mut lent,
            |ui, (dialog, host)| dialog.body(ui, &**host),
            |ui, (dialog, host)| dialog.footer(ui, &mut **host),
        );
        self.frame = frame;
        if shown.closed && self.visible {
            self.skip();
        }
    }

    fn logo(&mut self, ctx: &egui::Context) -> egui::TextureId {
        self.logo
            .get_or_insert_with(|| {
                let image: Image = application_icon_image(&CanvasRasteriser, 64);
                ctx.load_texture("katanaStartLogo", image.to_color_image(), egui::TextureOptions::LINEAR)
            })
            .id()
    }

    fn body(&mut self, ui: &mut Ui, host: &dyn Host) {
        ui.set_min_width(560.0);
        let document = host.document();
        // Which program, and what is asked.
        ui.horizontal_top(|ui| {
            let logo = self.logo(ui.ctx());
            let shown = ui.image((logo, egui::vec2(64.0, 64.0)));
            controls::custom(
                ui,
                &shown,
                crate::drive::registry::ControlRecord::new(
                    "startLogo",
                    crate::drive::registry::ControlKind::Label,
                ),
            );
            let words = "Choose what this session starts with: the customisation the survey \
                         codes are drawn by, and the coordinate system the drawing is in. Both \
                         can be changed later, in File > Settings and File > Project Coordinate \
                         System.";
            let title = format!("Katana\n{words}");
            ui.vertical(|ui| {
                ui.heading("Katana");
                controls::label_as(
                    ui,
                    "startTitle",
                    &title,
                    egui::Label::new(RichText::new(words).color(palette::text_muted().to_egui()))
                        .wrap(),
                );
            });
        });

        // The customisation.
        ui.group(|ui| {
            ui.strong("Customisation");
            ui.horizontal(|ui| {
                ui.label("Start with");
                let texts = self.customisation_items();
                let before = self.customisation.index;
                controls::choice(ui, "startCustomisation", &texts, &mut self.customisation);
                // An item that cannot be used is shown, and why, but not chosen.
                if let Some(at) = self.customisation.index
                    && !self.items.get(at).is_some_and(|item| item.usable)
                {
                    self.customisation.index = before;
                }
            });
            let from_file = self.file_enabled();
            ui.add_enabled_ui(from_file, |ui| {
                ui.horizontal(|ui| {
                    ui.label("File");
                    controls::line_with(
                        ui,
                        "startCustomisationFile",
                        &mut self.file,
                        LineOptions {
                            placeholder: "a Katana customisation file (.json)",
                            ..LineOptions::default()
                        },
                    );
                    // A headless session opens no file dialog; Browse is the window's to answer.
                    let _ = controls::button(ui, "startCustomisationBrowse", "Browse...");
                });
                controls::check(
                    ui,
                    "startCustomisationReplace",
                    "Replace instead of merging",
                    &mut self.replace,
                )
                .on_hover_text(
                    "Without it the file's definitions and rules are merged onto the session's \
                     (CUSTOMISE REPLACE)",
                );
            });
            let keep_tip = if self.kept_file.is_empty() {
                String::from("This window keeps no customisation between runs")
            } else {
                format!(
                    "Write it to the kept file, which the next start reads (CUSTOMISE KEEP): {}",
                    self.kept_file
                )
            };
            ui.add_enabled_ui(self.keep_enabled(), |ui| {
                controls::check(
                    ui,
                    "startCustomisationKeep",
                    "Use it at every start",
                    &mut self.keep,
                )
                .on_hover_text(keep_tip);
            });
        });

        // The coordinate system.
        ui.group(|ui| {
            ui.strong("Coordinate system");
            let texts: Vec<String> = self.crs_items.iter().map(|(text, _)| text.clone()).collect();
            controls::choice(ui, "startCrs", &texts, &mut self.crs).on_hover_text(
                "Pick a common system, or type an EPSG code, WKT or a PROJ string (CRS SET)",
            );
            let check = self.crs_check();
            controls::label_as(ui, "startCrsCheck", &check, egui::Label::new(check.as_str()).wrap());
        });

        // The lines Start runs, as they will be logged: what a person learns the verbs from, and
        // what an agent types instead.
        let made = self.lines(document);
        let text = self.line_text(document);
        let shown = RichText::new(text.as_str());
        let shown = if made.is_err() {
            shown.color(palette::error().to_egui())
        } else {
            shown
        };
        controls::label_as(
            ui,
            "startLine",
            &text,
            egui::Label::new(shown).wrap().selectable(true),
        );
        let status = self.status.clone();
        controls::label_as(
            ui,
            "startStatus",
            &status,
            egui::Label::new(status.as_str()).wrap().selectable(true),
        );
    }

    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.horizontal(|ui| {
            controls::check(
                ui,
                "startShowAtStart",
                "Show this at start",
                &mut self.show_at_start,
            )
            .on_hover_text("File > Start Screen opens it at any time");
            let can_start = self.start_enabled(host.document());
            let pressed = ui
                .add_enabled_ui(can_start, |ui| controls::button(ui, "startOk", "Start").clicked())
                .inner;
            if pressed {
                let _ = self.start(host);
                ui.ctx().request_repaint();
            }
            if controls::button(ui, "startSkip", "Skip")
                .on_hover_text("Start with the session as it is")
                .clicked()
            {
                self.skip();
            }
        });
    }
}
