//! What the settings twins share: the C++ test's customisations written out as text (their counts
//! are in its header and repeated here), its SettingsFixture - a recording runner, the callbacks
//! that count, the code manager's flag - as a host and a context, a bench that draws the dialog in
//! the harness and reads it by the C++ object names, the file dialog answered as the C++'s
//! FileDialogAnswer answered it, and the sessions that start as a front end starts one.
//!
//! The counts, by hand from the text:
//!
//! ```text
//! SITE       1 linestyle (SITE Fence) and 2 symbols (SITE Peg, SITE Post): 3 definitions; 3 rules
//!            over 2 codes (FN* feature, PG* feature, PG* symbol); a notice of two lines
//! ONE_CODE   no definition; 2 rules over 1 code (PG* feature, PG* symbol)
//! MINE       1 linestyle (MINE Hedge) and no symbol: 1 definition; 1 rule over 1 code (HG*
//!            feature); its automation said - survey codes NOT applied, coded points joined
//! PEGS_TWO   "Pegs Two": no definition; 2 rules over 1 code - the second built-in of the
//!            chooser's cases, beside SITE, which is the default
//! ```

#![allow(dead_code, reason = "Each case uses only some of the bench's readers.")]

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use katana_cad::customisation_host::{
    BuiltInCustomisation, BuiltInEntry, CustomisationHost, install_built_in_customisation,
    read_customisation_file, start_customisation,
};
use katana_cad::{CustomisationOrigin, Document};
use katana_core::path_text::generic_path_utf8;
use katana_entity::customisation::customisation_digest;
use katana_entity::{Customisation, CustomisationAutomation};
use katana_window::customisation::settings_dialog::{
    FileDialogAsk, SettingsContext, SettingsDialog, run_through_host,
};
use katana_window::drive::{ControlKind, ControlRecord, Harness};
use katana_window::host::{Answer, Host, HostParts, RecordingHost, VerbOutcome};

use crate::fixture_customisation::customisation_from_text;
use crate::window_support::Scratch;

pub const SITE: &str = r#"{
  "format": "katana-customisation",
  "version": 1,
  "name": "Site",
  "notice": [
    "Site set: for this job only.",
    "Ask before passing it on."
  ],
  "linestyles": [
    {"name": "SITE Fence", "strokes": [
      ["move", 0, 0],
      ["draw", 4, 0]
    ]}
  ],
  "symbols": [
    {"name": "SITE Peg", "atVertices": true, "strokes": [
      ["circle", 0.5]
    ]},
    {"name": "SITE Post", "atVertices": true, "strokes": [
      ["move", -0.25, 0],
      ["draw", 0.25, 0]
    ]}
  ],
  "codes": [
    {"key": "FN*", "sets": "feature", "layer": "SITE FENCES", "linestyle": "SITE Fence"},
    {"key": "PG*", "sets": "feature", "layer": "SITE PEGS", "draw": "point"},
    {"key": "PG*", "sets": "symbol", "symbol": {"name": "SITE Peg"}}
  ]
}
"#;

/// Rules alone, and all of ONE code: SITE's two for PG*.
pub const ONE_CODE: &str = r#"{
  "format": "katana-customisation",
  "version": 1,
  "name": "Pegs",
  "codes": [
    {"key": "PG*", "sets": "feature", "layer": "SITE PEGS", "draw": "point"},
    {"key": "PG*", "sets": "symbol", "symbol": {"name": "SITE Peg"}}
  ]
}
"#;

/// The second built-in of the chooser's cases: ONE_CODE's rules under a name of two words, so that
/// the line it builds has to quote it.
pub const PEGS_TWO: &str = r#"{
  "format": "katana-customisation",
  "version": 1,
  "name": "Pegs Two",
  "description": "Two rules for one code",
  "codes": [
    {"key": "PG*", "sets": "feature", "layer": "SITE PEGS", "draw": "point"},
    {"key": "PG*", "sets": "symbol", "symbol": {"name": "SITE Peg"}}
  ]
}
"#;

/// A person's own customisation, as a kept file holds one.
pub const MINE: &str = r#"{
  "format": "katana-customisation",
  "version": 1,
  "name": "Mine",
  "automation": {"codesOnSurveyImport": false, "lineworkOnSurveyImport": true},
  "linestyles": [
    {"name": "MINE Hedge", "strokes": [
      ["move", 0, 0],
      ["draw", 2, 0]
    ]}
  ],
  "codes": [
    {"key": "HG*", "sets": "feature", "layer": "MINE HEDGES", "linestyle": "MINE Hedge"}
  ]
}
"#;

/// The kept file every fixture's host has: a folder with a blank in its name.
pub const KEPT_FILE: &str = "C:/kept files/customisation.json";

pub const KEEP: &str = "CUSTOMISE KEEP";

/// A line that was carried out, and said nothing.
pub fn carried_out() -> VerbOutcome {
    VerbOutcome::new(true, "", "")
}

pub fn ok(reply: &str) -> VerbOutcome {
    VerbOutcome::new(true, reply, "")
}

pub fn refused(error: &str) -> VerbOutcome {
    VerbOutcome::new(false, "", error)
}

/// The automation switches as the C++'s designated initialisers write them.
pub fn automation(codes: bool, linework: bool) -> CustomisationAutomation {
    CustomisationAutomation {
        codes_on_survey_import: codes,
        linework_on_survey_import: linework,
    }
}

/// SITE installed whole into `document`, in the place of whatever the session held.
pub fn install_site(document: &mut Document, origin: CustomisationOrigin, kept: bool) {
    document
        .install_customisation(customisation_from_text(SITE), origin, kept)
        .unwrap_or_else(|error| panic!("{}", error.describe()));
}

/// A path as text with '/' between its parts, as a QTemporaryDir's paths and pathText give it.
pub fn path_text(path: &Path) -> String {
    String::from_utf8_lossy(&generic_path_utf8(path)).into_owned()
}

/// QDir::toNativeSeparators.
pub fn native(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        String::from(path)
    }
}

/// The customisation the file at `path` holds, read by the format's own reader.
pub fn customisation_in_file(path: &str) -> Customisation {
    read_customisation_file(Path::new(path))
        .unwrap_or_else(|error| panic!("{path}: {}", error.describe()))
        .customisation
}

/// `text` as an entry of the list a host offers, said to have been compiled in from `source`.
pub fn entry_of(text: &str, source: &str) -> BuiltInEntry {
    let customisation = customisation_from_text(text);
    BuiltInEntry {
        name: customisation.name.clone(),
        description: customisation.description.clone(),
        source: String::from(source),
        built_in: BuiltInCustomisation {
            customisation: Some(Arc::new(customisation)),
            digest: customisation_digest(text.as_bytes()),
            ..BuiltInCustomisation::default()
        },
    }
}

/// SITE, the default, and PEGS_TWO: what the chooser offers in its cases.
pub fn site_and_pegs() -> Vec<BuiltInEntry> {
    vec![
        entry_of(SITE, "site.customisation.json"),
        entry_of(PEGS_TWO, "pegs.customisation.json"),
    ]
}

/// SITE as the built-in of a host.
pub fn site_built_in() -> BuiltInCustomisation {
    BuiltInCustomisation {
        customisation: Some(Arc::new(customisation_from_text(SITE))),
        digest: customisation_digest(SITE.as_bytes()),
        ..BuiltInCustomisation::default()
    }
}

// ---- the file dialog ----------------------------------------------------------------------------

/// Every file dialog a click opens, caught as it opens (the C++'s FileDialogAnswer): how many there
/// were, what the last one was asked, and `chosen` chosen in it - or, empty, cancelled.
#[derive(Debug, Default)]
pub struct FileDialogs {
    pub seen: u32,
    pub last: FileDialogAsk,
    pub chosen: String,
}

impl FileDialogs {
    /// The last one's filters, as a QFileDialog lists them.
    pub fn filters(&self) -> Vec<String> {
        self.last.filter.split(";;").map(String::from).collect()
    }
}

// ---- the fixture -------------------------------------------------------------------------------

/// The C++'s SettingsFixture, before its dialog is made: the host (its drawing, its runner, its
/// log), the context, and the cells the context's callbacks count in. A case changes them, then
/// opens the bench.
pub struct Fixture {
    pub host: RecordingHost,
    pub context: SettingsContext,
    pub dirty: Rc<Cell<bool>>,
    pub opened_codes: Rc<Cell<u32>>,
    pub opened_linework: Rc<Cell<u32>>,
    pub opened_symbols: Rc<Cell<u32>>,
    pub files: Rc<RefCell<FileDialogs>>,
}

impl Default for Fixture {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(clippy::type_complexity, reason = "The one boxed callback type the dialog takes.")]
fn counter(count: &Rc<Cell<u32>>) -> Box<dyn FnMut(&mut dyn Host)> {
    let count = Rc::clone(count);
    Box::new(move |_host: &mut dyn Host| count.set(count.get() + 1))
}

impl Fixture {
    pub fn new() -> Self {
        let mut host = RecordingHost::new().answering(Answer::Fixed(carried_out()));
        // A window a person uses: Browse opens the file dialog, which the context answers.
        host.parts.headless = false;
        let dirty = Rc::new(Cell::new(false));
        let opened_codes = Rc::new(Cell::new(0));
        let opened_linework = Rc::new(Cell::new(0));
        let opened_symbols = Rc::new(Cell::new(0));
        let files = Rc::new(RefCell::new(FileDialogs::default()));
        let context = SettingsContext {
            reads_drawing: true,
            run: Some(run_through_host()),
            code_manager_dirty: Some({
                let dirty = Rc::clone(&dirty);
                Box::new(move || dirty.get())
            }),
            open_code_manager: Some(counter(&opened_codes)),
            open_linework: Some(counter(&opened_linework)),
            open_symbol_library: Some(counter(&opened_symbols)),
            has_built_in: true,
            kept_file: String::from(KEPT_FILE),
            // No built-in to choose between, whatever this build compiled in; the cases of the
            // chooser give a list of their own.
            built_ins: Some(Box::new(Vec::<BuiltInEntry>::new)),
            choose_file: Some({
                let files = Rc::clone(&files);
                Box::new(move |ask: &FileDialogAsk| {
                    let mut files = files.borrow_mut();
                    files.seen += 1;
                    files.last = ask.clone();
                    files.chosen.clone()
                })
            }),
            ..SettingsContext::default()
        };
        Self {
            host,
            context,
            dirty,
            opened_codes,
            opened_linework,
            opened_symbols,
            files,
        }
    }

    /// SITE installed whole, in the place of whatever the session held.
    pub fn install(&mut self, origin: CustomisationOrigin, kept: bool) {
        install_site(self.host.document_mut(), origin, kept);
    }

    pub fn document(&self) -> &Document {
        self.host.document()
    }

    pub fn document_mut(&mut self) -> &mut Document {
        self.host.document_mut()
    }

    /// The two built-ins the dialog offers; to be done BEFORE the dialog is made.
    pub fn offer_site_and_pegs(&mut self) {
        let entries = site_and_pegs();
        self.context.built_ins = Some(Box::new(move || entries.clone()));
    }

    /// SITE installed as the built-in of the run, kept, as a start leaves it.
    pub fn start_on_site(&mut self) {
        self.offer_site_and_pegs();
        let entries = site_and_pegs();
        install_built_in_customisation(self.host.document_mut(), &entries[0].built_in, true)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        self.host
            .document_mut()
            .set_built_in_customisation_name(String::from("Site"));
    }

    /// The dialog over this fixture, shown.
    pub fn open(self) -> Bench {
        Bench::new(self)
    }
}

// ---- the dialog on the harness -----------------------------------------------------------------

/// The Settings dialog, shown, on the harness, with its host and the fixture's cells.
pub struct Bench {
    pub harness: Harness,
    pub dialog: SettingsDialog,
    pub host: RecordingHost,
    pub dirty: Rc<Cell<bool>>,
    pub opened_codes: Rc<Cell<u32>>,
    pub opened_linework: Rc<Cell<u32>>,
    pub opened_symbols: Rc<Cell<u32>>,
    pub files: Rc<RefCell<FileDialogs>>,
}

impl Bench {
    pub fn new(fixture: Fixture) -> Self {
        let Fixture {
            host,
            context,
            dirty,
            opened_codes,
            opened_linework,
            opened_symbols,
            files,
        } = fixture;
        let mut dialog = SettingsDialog::new(context, &host);
        dialog.show();
        let mut harness = Harness::new();
        harness.target("settingsDialog");
        let mut bench = Self {
            harness,
            dialog,
            host,
            dirty,
            opened_codes,
            opened_linework,
            opened_symbols,
            files,
        };
        bench.draw();
        bench
    }

    /// Frames until the window is idle: the C++'s processEvents. A refused act fails the case.
    pub fn draw(&mut self) {
        let dialog = &mut self.dialog;
        let host = &mut self.host;
        self.harness
            .run(|ui| dialog.ui(ui, &mut *host))
            .unwrap_or_else(|problem| panic!("{problem}"));
    }

    /// What every line does and answers from now on: lent the host's parts (its drawing).
    pub fn answer(&mut self, answer: impl FnMut(&mut HostParts, &str) -> VerbOutcome + 'static) {
        self.host.answer = Answer::Custom(Box::new(answer));
    }

    /// Every line run by the host's own interpreter on its drawing, as the window's executor runs
    /// it (the C++'s runAsTheWindowDoes).
    pub fn answer_by_the_verb(&mut self) {
        self.host.answer = Answer::Interpreter;
    }

    pub fn document(&self) -> &Document {
        self.host.document()
    }

    pub fn document_mut(&mut self) -> &mut Document {
        self.host.document_mut()
    }

    pub fn install(&mut self, origin: CustomisationOrigin, kept: bool) {
        install_site(self.host.document_mut(), origin, kept);
    }

    pub fn record(&self, name: &str) -> Option<ControlRecord> {
        self.harness.control(name)
    }

    pub fn control(&self, name: &str) -> ControlRecord {
        self.record(name).unwrap_or_else(|| panic!("no {name}"))
    }

    /// What a label shows.
    pub fn shown(&self, label: &str) -> String {
        self.harness
            .report(label)
            .unwrap_or_else(|problem| panic!("{problem}"))
    }

    /// What a field or a text box holds.
    pub fn text(&self, name: &str) -> String {
        self.control(name).value
    }

    pub fn enabled(&self, name: &str) -> bool {
        self.control(name).enabled
    }

    /// Whether the control is shown (the C++'s !isHidden, and isVisibleTo the dialog).
    pub fn visible(&self, name: &str) -> bool {
        self.control(name).visible
    }

    pub fn checked(&self, name: &str) -> bool {
        self.control(name).checked == Some(true)
    }

    pub fn tip(&self, name: &str) -> String {
        self.dialog.tip(name)
    }

    pub fn status(&self) -> String {
        String::from(self.dialog.status())
    }

    pub fn says_a_refusal(&self) -> bool {
        self.dialog.status_is_error()
    }

    /// --fill NAME=TEXT: a field typed into, a box set ("on", "off"), a list's row, a choice.
    pub fn fill(&mut self, name: &str, text: &str) {
        self.harness.fill(name, text);
        self.draw();
    }

    /// QCheckBox::setChecked.
    pub fn tick(&mut self, name: &str, on: bool) {
        self.fill(name, if on { "on" } else { "off" });
    }

    /// A click: nothing for a disabled control, as QAbstractButton::click does nothing then.
    pub fn press(&mut self, name: &str) {
        if !self.enabled(name) {
            return;
        }
        self.harness.press(name);
        self.draw();
    }

    /// The chooser's items (QComboBox::itemText).
    pub fn chooser_items(&self) -> Vec<String> {
        self.control("settingsBuiltIn").items
    }

    /// QComboBox::setCurrentIndex on the chooser: its item of that place chosen.
    pub fn choose(&mut self, index: usize) {
        let item = self.chooser_items()[index].clone();
        self.fill("settingsBuiltIn", &item);
    }

    /// The chooser's current item's data: the built-in's name.
    pub fn chosen_name(&self) -> String {
        self.dialog
            .built_in_index()
            .and_then(|at| self.dialog.built_in_names().get(at).cloned())
            .unwrap_or_default()
    }

    pub fn ran(&self) -> Vec<String> {
        self.host.lines().to_vec()
    }

    pub fn clear_ran(&mut self) {
        let _ = self.host.take_lines();
    }

    pub fn logged(&self) -> Vec<(String, bool)> {
        self.host.logged().to_vec()
    }

    /// The fields the target drew (QLineEdit).
    pub fn fields(&self) -> Vec<String> {
        self.harness
            .snapshot()
            .in_target("settingsDialog")
            .filter(|record| record.kind == ControlKind::Line)
            .map(|record| record.name.clone())
            .collect()
    }

    /// Every button the target drew.
    pub fn buttons(&self) -> Vec<ControlRecord> {
        self.harness
            .snapshot()
            .in_target("settingsDialog")
            .filter(|record| record.kind == ControlKind::Button)
            .cloned()
            .collect()
    }

    /// Every text the dialog and its controls can show a person (the C++'s textsIn): its own
    /// words - title, captions, placeholders, tips - and each control's text, value and items.
    pub fn texts(&self) -> Vec<String> {
        let mut seen = self.dialog.words();
        for record in self.harness.snapshot().in_target("settingsDialog") {
            seen.push(record.text.clone());
            seen.push(record.value.clone());
            seen.extend(record.items.iter().cloned());
        }
        seen.retain(|text| !text.is_empty());
        seen
    }

    /// The drawing goes: another is put in its place, and the dialog's is no more.
    pub fn close_drawing(&mut self) {
        self.host.parts.document = Document::new(None);
    }
}

// ---- sessions as a front end starts them --------------------------------------------------------

/// A session whose user keeps a customisation of their own (the C++'s KeptSession): SITE as the
/// program's built-in, and MINE in the kept file - on disk BEFORE the host is handed over. The
/// runner is the verb itself, the interpreter over the same drawing.
pub struct KeptSession {
    pub scratch: Scratch,
    pub kept_file: String,
    /// Where CUSTOMISE KEEP leaves the kept file that was there.
    pub set_aside: String,
    pub host: CustomisationHost,
    pub fixture: Fixture,
}

impl KeptSession {
    pub fn start(name: &str) -> Self {
        let scratch = Scratch::new(name);
        let folder = scratch.folder("kept here");
        let kept_path = folder.join("customisation.json");
        std::fs::write(&kept_path, MINE).expect("the kept file is written");
        let kept_file = path_text(&kept_path);
        let set_aside = format!("{kept_file}.bak");
        let host = CustomisationHost {
            built_in: site_built_in(),
            kept_file: kept_path,
            built_ins: None,
        };
        let mut fixture = Fixture::new();
        fixture.context.kept_file = kept_file.clone();
        fixture
            .host
            .parts
            .interpreter
            .set_customisation_host(host.clone());
        let started = start_customisation(fixture.host.document_mut(), &host);
        assert_eq!(started.installed, CustomisationOrigin::Kept);
        assert!(started.problems.is_empty(), "{:?}", started.problems);
        assert!(fixture.document().customisation_state().kept);
        fixture.host.answer = Answer::Interpreter;
        Self {
            scratch,
            kept_file,
            set_aside,
            host,
            fixture,
        }
    }
}

/// A session as a front end starts one (the C++'s LibrarySession): SITE the built-in of the run
/// and PEGS_TWO the other the host offers, a kept file named and not written, the interpreter over
/// the same drawing as the runner.
pub struct LibrarySession {
    pub scratch: Scratch,
    pub kept_file: String,
    pub set_aside: String,
    pub host: CustomisationHost,
    pub fixture: Fixture,
}

impl LibrarySession {
    pub fn start(name: &str) -> Self {
        let scratch = Scratch::new(name);
        let kept_path: PathBuf = scratch.path("kept here").join("customisation.json");
        let kept_file = path_text(&kept_path);
        let set_aside = format!("{kept_file}.bak");
        let entries = site_and_pegs();
        let host = CustomisationHost {
            built_in: entries[0].built_in.clone(),
            kept_file: kept_path,
            built_ins: Some(entries.clone()),
        };
        let mut fixture = Fixture::new();
        fixture.context.kept_file = kept_file.clone();
        fixture.context.built_ins = Some(Box::new(move || entries.clone()));
        fixture
            .host
            .parts
            .interpreter
            .set_customisation_host(host.clone());
        let started = start_customisation(fixture.host.document_mut(), &host);
        assert_eq!(started.installed, CustomisationOrigin::BuiltIn);
        assert!(fixture.document().customisation_state().kept);
        fixture.host.answer = Answer::Interpreter;
        Self {
            scratch,
            kept_file,
            set_aside,
            host,
            fixture,
        }
    }
}

/// Whether a file is there (QFileInfo::exists).
pub fn exists(path: &str) -> bool {
    Path::new(path).exists()
}
