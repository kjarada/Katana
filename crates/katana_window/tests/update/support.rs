//! What the update twins share: the C++ test's DialogFixture (a recording runner and callbacks that
//! count), its views (the design's 0.2.0 installed, 0.3.0 released), a bench that draws the update
//! dialog in the harness and reads it by the C++ object names, and the Updates page's fixture - a
//! credential store that records what is written to it, and the state the page is told of.
//!
//! WHERE THE BENCH READS THE DIALOG'S OWN ANSWER. Qt's `isEnabled` of a button its state hides
//! still says whether a click would reach its slot, and `click()` on a hidden button reaches it;
//! in egui a hidden control is drawn in an invisible `Ui`, which is disabled too, and the harness
//! refuses to press it. So for a button that is not shown the bench asks the dialog itself
//! (`button_enabled`, `click`); for one that is shown it reads and presses what the frame drew.

#![allow(dead_code, reason = "Each case uses only some of the bench's readers.")]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use katana_core::{Error, ErrorCode, Result, Status};
use katana_update::CredentialStore;
use katana_window::customisation::settings_dialog::{
    SettingsContext, SettingsDialog, SettingsUpdates, run_through_host,
};
use katana_window::drive::{ControlKind, ControlRecord, Harness};
use katana_window::host::{Answer, Host, HostParts, RecordingHost, VerbOutcome};
use katana_window::update_dialog::{
    UPDATE_BUTTONS, UpdateButton, UpdateCall, UpdateDialog, UpdateDialogContext,
    UpdateDialogState, UpdateOpenNotes, UpdateRun, UpdateView, update_detail_text,
    update_state_text,
};

/// A line that was carried out, and said nothing (the C++'s carriedOut).
pub fn carried_out() -> VerbOutcome {
    VerbOutcome::new(true, "", "")
}

/// A line refused with `error`.
pub fn refused(error: &str) -> VerbOutcome {
    VerbOutcome::new(false, "", error)
}

// ---- the design's views ---------------------------------------------------------------------

/// The design's example: Katana 0.2.0 installed, 0.3.0 released.
pub fn view_of(state: UpdateDialogState) -> UpdateView {
    UpdateView {
        state,
        installed_version: String::from("0.2.0"),
        latest_version: String::from("0.3.0"),
        ..UpdateView::default()
    }
}

pub fn available() -> UpdateView {
    UpdateView {
        changed_files: 23,
        total_files: 411,
        download_bytes: 71_000_000,
        notes_url: String::from("https://github.com/kjarada/Katana/releases/tag/v0.3.0"),
        ..view_of(UpdateDialogState::Available)
    }
}

pub fn downloading() -> UpdateView {
    UpdateView {
        changed_files: 23,
        total_files: 411,
        download_bytes: 71_000_000,
        downloaded_bytes: 31_000_000,
        downloaded_files: Some(12),
        ..view_of(UpdateDialogState::Downloading)
    }
}

pub fn view_of_version(state: UpdateDialogState, version: &str) -> UpdateView {
    UpdateView {
        latest_version: String::from(version),
        ..view_of(state)
    }
}

/// A view's two sentences, as the dialog shows them and the log says them (the C++'s saidBy).
pub fn said_by(view: &UpdateView) -> String {
    let state = update_state_text(view);
    let detail = update_detail_text(view);
    if detail.is_empty() {
        state
    } else {
        format!("{state} {detail}")
    }
}

// ---- what a press can reach, recorded ---------------------------------------------------------

/// Every line handed to the executor, and how often each callback was called.
#[derive(Debug, Default)]
pub struct Called {
    pub ran: Vec<String>,
    pub updates: u32,
    pub cancels: u32,
    pub laters: u32,
    pub opened: Vec<String>,
}

type AnswerFn = Box<dyn Fn(&str) -> VerbOutcome>;

/// The C++'s DialogFixture: a context whose every member records, shared by each dialog made from
/// it. What a line answers is `answer`'s; unset, it was carried out.
#[derive(Clone, Default)]
pub struct DialogFixture {
    pub record: Rc<RefCell<Called>>,
    answer: Rc<RefCell<Option<AnswerFn>>>,
}

impl DialogFixture {
    pub fn new() -> Self {
        Self::default()
    }

    /// What every line answers from now on.
    pub fn answer(&self, answer: impl Fn(&str) -> VerbOutcome + 'static) {
        *self.answer.borrow_mut() = Some(Box::new(answer));
    }

    /// A context whose members all record into this fixture.
    pub fn context(&self) -> UpdateDialogContext {
        let ran = Rc::clone(&self.record);
        let answer = Rc::clone(&self.answer);
        let run: UpdateRun = Box::new(move |_host: &mut dyn Host, line: &str| {
            ran.borrow_mut().ran.push(String::from(line));
            answer
                .borrow()
                .as_ref()
                .map_or_else(carried_out, |answer| answer(line))
        });
        let updates = Rc::clone(&self.record);
        let update: UpdateCall = Box::new(move |_host: &mut dyn Host| {
            updates.borrow_mut().updates += 1;
        });
        let cancels = Rc::clone(&self.record);
        let cancel: UpdateCall = Box::new(move |_host: &mut dyn Host| {
            cancels.borrow_mut().cancels += 1;
        });
        let opened = Rc::clone(&self.record);
        let open_notes: UpdateOpenNotes = Box::new(move |_host: &mut dyn Host, url: &str| {
            opened.borrow_mut().opened.push(String::from(url));
        });
        let laters = Rc::clone(&self.record);
        let later: UpdateCall = Box::new(move |_host: &mut dyn Host| {
            laters.borrow_mut().laters += 1;
        });
        UpdateDialogContext {
            run: Some(run),
            update: Some(update),
            cancel: Some(cancel),
            open_notes: Some(open_notes),
            later: Some(later),
        }
    }

    /// What has been called so far, as one text to compare: the lines, then each callback that
    /// was called with how often.
    pub fn called(&self) -> String {
        let record = self.record.borrow();
        let mut parts = record.ran.clone();
        if record.updates != 0 {
            parts.push(format!("update x{}", record.updates));
        }
        if record.cancels != 0 {
            parts.push(format!("cancel x{}", record.cancels));
        }
        if record.laters != 0 {
            parts.push(format!("later x{}", record.laters));
        }
        for url in &record.opened {
            parts.push(format!("notes {url}"));
        }
        parts.join(" | ")
    }
}

// ---- the update dialog on the harness ---------------------------------------------------------

/// The update dialog, shown, on the harness, with a recording host for the window it would be
/// lent. The runner is the context's, so the host is never asked to run a line.
pub struct UpdateBench {
    pub harness: Harness,
    pub dialog: UpdateDialog,
    pub host: RecordingHost,
}

impl UpdateBench {
    pub fn new(context: UpdateDialogContext) -> Self {
        let mut dialog = UpdateDialog::new(context);
        dialog.show();
        let mut harness = Harness::new();
        harness.target("updateDialog");
        let mut bench = Self {
            harness,
            dialog,
            host: RecordingHost::new(),
        };
        bench.draw();
        bench
    }

    /// Frames until the window is idle; a refused act fails the case.
    pub fn draw(&mut self) {
        let dialog = &mut self.dialog;
        let host = &mut self.host;
        self.harness
            .run(|ui| dialog.ui(ui, &mut *host))
            .unwrap_or_else(|problem| panic!("{problem}"));
    }

    /// UpdateDialog::setView, and the frames that show it.
    pub fn set_view(&mut self, view: UpdateView) {
        self.dialog.set_view(view);
        self.draw();
    }

    pub fn record(&self, name: &str) -> Option<ControlRecord> {
        self.harness.control(name)
    }

    pub fn control(&self, name: &str) -> ControlRecord {
        self.record(name).unwrap_or_else(|| panic!("no {name}"))
    }

    /// What a label shows (QLabel::text).
    pub fn shown(&self, label: &str) -> String {
        self.harness
            .report(label)
            .unwrap_or_else(|problem| panic!("{problem}"))
    }

    /// QWidget::isVisibleTo the dialog.
    pub fn visible(&self, name: &str) -> bool {
        self.control(name).visible
    }

    /// The two sentences as a person reads them, one after the other.
    pub fn said(&self) -> String {
        let state = self.shown("updateStateText");
        let detail = self.shown("updateDetailText");
        if detail.is_empty() {
            state
        } else {
            format!("{state} {detail}")
        }
    }

    /// The buttons the dialog shows, by name, in the order of the alphabet.
    pub fn shown_buttons(&self) -> Vec<String> {
        let mut names: Vec<String> = UPDATE_BUTTONS
            .into_iter()
            .map(UpdateButton::name)
            .filter(|name| self.record(name).is_some_and(|record| record.visible))
            .map(String::from)
            .collect();
        names.sort();
        names
    }

    /// QWidget::isEnabled: what the frame drew for a button that is shown, and the dialog's own
    /// answer for one that is not (see the module comment).
    pub fn enabled(&mut self, name: &str) -> bool {
        match self.record(name) {
            Some(record) if record.visible => record.enabled,
            _ => {
                let button = UpdateButton::of_name(name).unwrap_or_else(|| panic!("no {name}"));
                self.dialog.button_enabled(button)
            }
        }
    }

    /// A click, as a person's or the headless driver's --press: nothing for a disabled button, as
    /// QAbstractButton::click does nothing then; the dialog's own click for one its state hides,
    /// which Qt's click reaches.
    pub fn press(&mut self, name: &str) {
        let button = UpdateButton::of_name(name).unwrap_or_else(|| panic!("no button {name}"));
        if !self.enabled(name) {
            return;
        }
        if self.record(name).is_some_and(|record| record.visible) {
            self.harness.press(name);
        } else {
            self.dialog.click(&mut self.host, button);
        }
        self.draw();
    }

    /// updateProgress: whether it is shown, its range, and its value (None while it is busy).
    pub fn progress(&mut self) -> (bool, (i32, i32), Option<i32>) {
        let shown = self.visible("updateProgress");
        (
            shown,
            self.dialog.progress_range(),
            self.dialog.progress_value(),
        )
    }

    /// Every button the target drew.
    pub fn buttons(&self) -> Vec<ControlRecord> {
        self.harness
            .snapshot()
            .in_target("updateDialog")
            .filter(|record| record.kind == ControlKind::Button)
            .cloned()
            .collect()
    }
}

// ---- the Updates page of Settings -------------------------------------------------------------

/// The C++'s RecordingStore: a credential store that records what was written to it and holds one
/// token. What `write` answers in the place of storing is `refusal`; None: it stores.
#[derive(Debug, Default)]
pub struct RecordingStore {
    pub token: RefCell<Option<String>>,
    pub written: RefCell<Vec<String>>,
    pub cleared: Cell<u32>,
    pub refusal: RefCell<Option<Error>>,
}

impl CredentialStore for RecordingStore {
    fn read(&self) -> Result<String> {
        self.token
            .borrow()
            .clone()
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "no access token is stored"))
    }

    fn write(&self, token: &[u8]) -> Status {
        if let Some(refusal) = self.refusal.borrow().clone() {
            return Err(refusal);
        }
        let value = String::from_utf8_lossy(token).into_owned();
        self.written.borrow_mut().push(value.clone());
        *self.token.borrow_mut() = Some(value);
        Ok(())
    }

    fn clear(&self) -> Status {
        self.cleared.set(self.cleared.get() + 1);
        *self.token.borrow_mut() = None;
        Ok(())
    }

    fn r#where(&self) -> &str {
        "credential manager"
    }
}

/// A token of the shape a release host issues; no real one.
pub const TOKEN: &str = "ghp_NotARealTokenOnlyATestsWord0123456789";

/// The C++'s UpdatesFixture: Settings with its Updates page filled and NO drawing - the page is of
/// the program, so none of these cases gives it one. The state the page is told of is the cells
/// here; the lines it runs go to the bench's host, which records them (the C++'s `ran`) and logs
/// what the dialog refuses (its `logged`).
pub struct UpdatesFixture {
    pub store: Rc<RecordingStore>,
    pub automatic: Rc<Cell<bool>>,
    pub skipped: Rc<RefCell<String>>,
    pub put_back: Rc<Cell<bool>>,
    pub token_changes: Rc<Cell<u32>>,
    /// The page's services, as the fixture fills them; a case changes them before it makes the
    /// dialog.
    pub updates: SettingsUpdates,
    /// Whether the dialog is given the window's executor.
    pub runs: bool,
}

impl Default for UpdatesFixture {
    fn default() -> Self {
        Self::new()
    }
}

impl UpdatesFixture {
    pub fn new() -> Self {
        let store = Rc::new(RecordingStore::default());
        let automatic = Rc::new(Cell::new(true));
        let skipped = Rc::new(RefCell::new(String::new()));
        let put_back = Rc::new(Cell::new(false));
        let token_changes = Rc::new(Cell::new(0));
        let updates = SettingsUpdates {
            automatic: Some({
                let automatic = Rc::clone(&automatic);
                Rc::new(move || automatic.get())
            }),
            skipped_version: Some({
                let skipped = Rc::clone(&skipped);
                Rc::new(move || Some(skipped.borrow().clone()))
            }),
            put_back: Some({
                let put_back = Rc::clone(&put_back);
                Rc::new(move || Some(put_back.get()))
            }),
            credentials: Some(Rc::clone(&store) as Rc<dyn CredentialStore>),
            token_changed: Some({
                let token_changes = Rc::clone(&token_changes);
                Rc::new(move || token_changes.set(token_changes.get() + 1))
            }),
            user_agent: String::from("Katana/0.2.0 (win64)"),
            server: String::from("GitHub"),
            note: String::new(),
        };
        Self {
            store,
            automatic,
            skipped,
            put_back,
            token_changes,
            updates,
            runs: true,
        }
    }

    /// The dialog's context: no drawing, the window's executor unless `runs` is off, and the
    /// page's services.
    pub fn context(&self) -> SettingsContext {
        SettingsContext {
            run: self.runs.then(run_through_host),
            updates: self.updates.clone(),
            ..SettingsContext::default()
        }
    }

    /// A host that answers every line as carried out: the C++'s runner with no answer set.
    pub fn host() -> RecordingHost {
        RecordingHost::new().answering(Answer::Fixed(carried_out()))
    }
}

/// The Settings dialog, shown, on the harness, with its host.
pub struct SettingsBench {
    pub harness: Harness,
    pub dialog: SettingsDialog,
    pub host: RecordingHost,
}

impl SettingsBench {
    pub fn new(context: SettingsContext, host: RecordingHost) -> Self {
        let mut dialog = SettingsDialog::new(context, &host);
        dialog.show();
        let mut harness = Harness::new();
        harness.target("settingsDialog");
        let mut bench = Self {
            harness,
            dialog,
            host,
        };
        bench.draw();
        bench
    }

    /// The fixture's dialog over a host that carries every line out.
    pub fn of(fixture: &UpdatesFixture) -> Self {
        Self::new(fixture.context(), UpdatesFixture::host())
    }

    pub fn draw(&mut self) {
        let dialog = &mut self.dialog;
        let host = &mut self.host;
        self.harness
            .run(|ui| dialog.ui(ui, &mut *host))
            .unwrap_or_else(|problem| panic!("{problem}"));
    }

    /// What every line does and answers from now on.
    pub fn answer(&mut self, mut answer: impl FnMut(&str) -> VerbOutcome + 'static) {
        self.host.answer = Answer::Custom(Box::new(move |_parts: &mut HostParts, line: &str| {
            answer(line)
        }));
    }

    pub fn record(&self, name: &str) -> Option<ControlRecord> {
        self.harness.control(name)
    }

    pub fn control(&self, name: &str) -> ControlRecord {
        self.record(name).unwrap_or_else(|| panic!("no {name}"))
    }

    pub fn shown(&self, label: &str) -> String {
        self.harness
            .report(label)
            .unwrap_or_else(|problem| panic!("{problem}"))
    }

    pub fn enabled(&self, name: &str) -> bool {
        self.control(name).enabled
    }

    pub fn checked(&self, name: &str) -> bool {
        self.control(name).checked == Some(true)
    }

    /// What a field holds.
    pub fn text(&self, name: &str) -> String {
        self.control(name).value
    }

    pub fn status(&self) -> String {
        String::from(self.dialog.status())
    }

    /// Whether the status is in the colour of a refusal.
    pub fn says_a_refusal(&self) -> bool {
        self.dialog.status_is_error()
    }

    /// --fill NAME=TEXT: a field typed into (QLineEdit::setText), a box set (QCheckBox::setChecked,
    /// "on" or "off"), a list's row chosen by its text.
    pub fn fill(&mut self, name: &str, text: &str) {
        self.harness.fill(name, text);
        self.draw();
    }

    /// The list's row of that page brought forward (QListWidget::setCurrentRow).
    pub fn turn_to(&mut self, page: &str) {
        self.fill("settingsPages", page);
    }

    /// A click: nothing for a disabled control, as QAbstractButton::click does nothing then.
    pub fn press(&mut self, name: &str) {
        if !self.enabled(name) {
            return;
        }
        self.harness.press(name);
        self.draw();
    }

    /// The lines the host was handed, in order (the C++'s `ran`).
    pub fn ran(&self) -> Vec<String> {
        self.host.lines().to_vec()
    }

    /// What the dialog refused and logged.
    pub fn logged(&self) -> Vec<(String, bool)> {
        self.host.logged().to_vec()
    }

    /// Everything the dialog said anywhere a token could leak to: its status, every control's
    /// words but what a field holds (the C++ read labels, buttons and tips, never a QLineEdit's
    /// text), every text it can show (its tips among them), the log and the lines it ran.
    pub fn everything_said(&self) -> String {
        let mut said = vec![self.status()];
        for record in self.harness.snapshot().in_target("settingsDialog") {
            said.push(record.text.clone());
            if record.kind != ControlKind::Line {
                said.push(record.value.clone());
            }
            said.extend(record.items.iter().cloned());
        }
        said.extend(self.dialog.words());
        said.extend(self.logged().into_iter().map(|(message, _)| message));
        said.extend(self.ran());
        said.join("\n")
    }
}
