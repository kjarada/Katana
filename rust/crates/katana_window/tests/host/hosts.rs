//! What this target's twins share: a host whose jobs run on the window's threaded runner (the C++
//! tests used the real JobRunner), its log read apart, and the lock of the cases that run GDAL on a
//! shared file.

use std::sync::{Mutex, MutexGuard};

use katana_cad::{CommandInterpreter, Document};
use katana_interop::ReferenceData;
use katana_window::host::{
    Clipboard, GeoPhase, Host, JobAwait, JobId, Jobs, RecordingHost, UserSettings, VerbOutcome,
    ViewAccess,
};
use katana_window::jobs::{JobHost, JobRunner, wait_for_all_jobs, wait_for_job};

static SHARED_DATA: Mutex<()> = Mutex::new(());

/// The lock of the cases that run GDAL on a file of samples/gis: libtest runs a target's cases on
/// threads of one process, where ctest ran each in its own, and GDAL's .aux.xml sidecars of a
/// shared file must never race.
pub fn shared_data() -> MutexGuard<'static, ()> {
    SHARED_DATA
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A recording host whose jobs run on threads, as the window's do: everything but the jobs is the
/// recording host's, and its waits complete the jobs with itself.
pub struct ThreadedHost {
    pub recording: RecordingHost,
    pub runner: JobRunner,
}

impl Default for ThreadedHost {
    fn default() -> Self {
        Self::new()
    }
}

impl ThreadedHost {
    pub fn new() -> Self {
        Self {
            recording: RecordingHost::new(),
            runner: JobRunner::new(),
        }
    }

    /// The window's drawing and interpreter: a line run straight on them, as the C++ fixture's
    /// `interpreter.run`.
    pub fn type_line(&mut self, line: &str) {
        let parts = &mut self.recording.parts;
        parts
            .interpreter
            .run(&mut parts.document, line)
            .unwrap_or_else(|error| panic!("{line}: {}", error.describe()));
    }

    /// What was logged as a reply, in order.
    pub fn logged(&self) -> Vec<String> {
        self.recording
            .logged()
            .iter()
            .filter(|(_, error)| !error)
            .map(|(text, _)| text.clone())
            .collect()
    }

    /// What was logged as an error, in order.
    pub fn errors(&self) -> Vec<String> {
        self.recording
            .logged()
            .iter()
            .filter(|(_, error)| *error)
            .map(|(text, _)| text.clone())
            .collect()
    }
}

impl Host for ThreadedHost {
    fn run(&mut self, line: &str) -> VerbOutcome {
        self.recording.run(line)
    }
    fn document(&self) -> &Document {
        Host::document(&self.recording)
    }
    fn document_mut(&mut self) -> &mut Document {
        Host::document_mut(&mut self.recording)
    }
    fn interpreter(&self) -> &CommandInterpreter {
        self.recording.interpreter()
    }
    fn reference(&self) -> &ReferenceData {
        self.recording.reference()
    }
    fn reference_mut(&mut self) -> &mut ReferenceData {
        self.recording.reference_mut()
    }
    fn geo(&mut self, phase: GeoPhase<'_>) {
        self.recording.geo(phase);
    }
    fn headless(&self) -> bool {
        self.recording.headless()
    }
    fn log(&mut self, text: &str, is_error: bool) {
        self.recording.log(text, is_error);
    }
    fn warn_user(&mut self, title: &str, text: &str) {
        self.recording.warn_user(title, text);
    }
    fn views(&mut self) -> Option<&mut dyn ViewAccess> {
        self.recording.views()
    }
    fn jobs(&mut self) -> &mut dyn Jobs {
        &mut self.runner
    }
    fn wait_for(&mut self, id: JobId) {
        wait_for_job(self, id);
    }
    fn wait_for_all(&mut self) {
        wait_for_all_jobs(self);
    }
    fn await_job(&mut self, _started: &VerbOutcome, _done: JobAwait) -> bool {
        false
    }
    fn clipboard(&mut self) -> &mut dyn Clipboard {
        self.recording.clipboard()
    }
    fn settings(&mut self) -> &mut UserSettings {
        self.recording.settings()
    }
}

impl JobHost for ThreadedHost {
    fn job_runner(&mut self) -> &mut JobRunner {
        &mut self.runner
    }
}
