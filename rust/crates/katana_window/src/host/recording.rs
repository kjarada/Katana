//! A host of a test's own: what the C++ widget tests hand a dialog in place of the window - a
//! runner that records each line and answers as the test says (a `FakeRunner` with its fixed
//! answer, fixture_customisation.hpp's `InterpreterRunner` that runs it on the drawing), a drawing,
//! a log, an in-memory clipboard and settings, and jobs that run where they are started.
//!
//! It is also the model of how the window's executor is a host: its parts are fields of one value,
//! lent apart (the drawing to a geo phase, itself to a job's apply).

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use katana_app::geo::{Context, FrontEnd};
use katana_cad::{CommandInterpreter, Document};
use katana_interop::ReferenceData;

use super::clipboard::{Clipboard, MemoryClipboard};
use super::command_runner::{GeoPhase, Host, JobAwait, VerbOutcome};
use super::jobs::{
    CompletedJob, JobControl, JobFinished, JobId, JobProgress, JobShared, JobWork, Jobs, NO_JOB,
    complete, run_work,
};
use super::user_settings::UserSettings;
use super::view_access::ViewAccess;

/// How a recording host answers a line.
pub enum Answer {
    /// Every line answered so: the C++ tests' FakeRunner and its `answer`.
    Fixed(VerbOutcome),
    /// The lines answered in turn; once they run out, not ok, saying so.
    Queue(VecDeque<VerbOutcome>),
    /// Run by the host's own interpreter on its drawing, the reply or the refusal handed back:
    /// fixture_customisation.hpp's InterpreterRunner.
    Interpreter,
    /// A test's own runner, lent the host's parts (a geo workbench's, a script's).
    Custom(CustomRunner),
}

/// A test's own runner of lines: lent the recording host's parts and the line, it answers.
pub type CustomRunner = Box<dyn FnMut(&mut HostParts, &str) -> VerbOutcome>;

/// What a recording host holds, apart from how it answers, so a custom runner can be lent it.
pub struct HostParts {
    pub document: Document,
    pub interpreter: CommandInterpreter,
    pub reference: ReferenceData,
    /// Where a geo phase's derived rasters go when the drawing has no project.
    pub scratch: PathBuf,
    pub front_end: FrontEnd,
    pub headless: bool,
    /// What was logged, in order, each with whether it was an error.
    pub logged: Vec<(String, bool)>,
    /// What `warn_user` was asked to say: the title and the text.
    pub warnings: Vec<(String, String)>,
    pub clipboard: MemoryClipboard,
    pub settings: UserSettings,
    pub jobs: InlineJobs,
    pub views: Option<Box<dyn ViewAccess>>,
}

/// A host that records every line it is handed (see the module comment). Headless, as a test is.
pub struct RecordingHost {
    pub parts: HostParts,
    pub answer: Answer,
    lines: Vec<String>,
    outcomes: Vec<VerbOutcome>,
    /// Whether `await_job` takes a started outcome (false: every outcome is the answer).
    pub awaits_jobs: bool,
    awaiting: Vec<(VerbOutcome, JobAwait)>,
}

impl Default for RecordingHost {
    fn default() -> Self {
        Self::new()
    }
}

impl RecordingHost {
    /// An empty drawing, every line answered with a default outcome (not ok, nothing said), as a
    /// FakeRunner answers before a test sets its answer.
    pub fn new() -> Self {
        Self::with_document(Document::new(None))
    }

    pub fn with_document(document: Document) -> Self {
        Self {
            parts: HostParts {
                document,
                interpreter: CommandInterpreter::new(),
                reference: ReferenceData::default(),
                scratch: std::env::temp_dir().join("katana-window-recording-host"),
                front_end: FrontEnd::default(),
                headless: true,
                logged: Vec::new(),
                warnings: Vec::new(),
                clipboard: MemoryClipboard::new(),
                settings: UserSettings::in_memory(),
                jobs: InlineJobs::default(),
                views: None,
            },
            answer: Answer::Fixed(VerbOutcome::default()),
            lines: Vec::new(),
            outcomes: Vec::new(),
            awaits_jobs: false,
            awaiting: Vec::new(),
        }
    }

    /// The same host answering as `answer` says.
    pub fn answering(mut self, answer: Answer) -> Self {
        self.answer = answer;
        self
    }

    /// The same host with the workspace's views `views`.
    pub fn with_views(mut self, views: Box<dyn ViewAccess>) -> Self {
        self.parts.views = Some(views);
        self
    }

    /// Every line handed to `run`, in order.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// The lines handed over so far, and none kept.
    pub fn take_lines(&mut self) -> Vec<String> {
        std::mem::take(&mut self.lines)
    }

    /// What each line was answered, in order.
    pub fn outcomes(&self) -> &[VerbOutcome] {
        &self.outcomes
    }

    /// The last line's reply when it was ok, else its refusal (InterpreterRunner's lastReply);
    /// empty before any line.
    pub fn last_reply(&self) -> String {
        self.outcomes
            .last()
            .map(|outcome| {
                if outcome.ok {
                    outcome.reply.clone()
                } else {
                    outcome.error.clone()
                }
            })
            .unwrap_or_default()
    }

    pub fn logged(&self) -> &[(String, bool)] {
        &self.parts.logged
    }

    pub fn document(&self) -> &Document {
        &self.parts.document
    }

    pub fn document_mut(&mut self) -> &mut Document {
        &mut self.parts.document
    }

    /// A frame's turn for the jobs: every job whose work has ended is completed with this host,
    /// in the order they were started.
    pub fn deliver_finished_jobs(&mut self) {
        for job in self.parts.jobs.take(None) {
            complete(job, self);
        }
    }

    /// The outcomes `await_job` took, waiting to be told their job's end.
    pub fn awaited(&self) -> Vec<&VerbOutcome> {
        self.awaiting.iter().map(|(started, _)| started).collect()
    }

    /// Ends the first awaited job with `outcome`: its `done` is called with it.
    pub fn finish_awaited(&mut self, outcome: &VerbOutcome) -> bool {
        if self.awaiting.is_empty() {
            return false;
        }
        let (_, done) = self.awaiting.remove(0);
        done(outcome, self);
        true
    }
}

impl Host for RecordingHost {
    fn run(&mut self, line: &str) -> VerbOutcome {
        self.lines.push(String::from(line));
        let outcome = match &mut self.answer {
            Answer::Fixed(outcome) => outcome.clone(),
            Answer::Queue(outcomes) => outcomes.pop_front().unwrap_or_else(|| {
                VerbOutcome::new(false, "", "the test gave no answer for this line")
            }),
            Answer::Interpreter => {
                let parts = &mut self.parts;
                match parts.interpreter.run(&mut parts.document, line) {
                    Ok(reply) => VerbOutcome::new(true, &reply, ""),
                    Err(error) => VerbOutcome::new(false, "", &error.describe()),
                }
            }
            Answer::Custom(runner) => runner(&mut self.parts, line),
        };
        self.outcomes.push(outcome.clone());
        outcome
    }

    fn document(&self) -> &Document {
        &self.parts.document
    }

    fn document_mut(&mut self) -> &mut Document {
        &mut self.parts.document
    }

    fn interpreter(&self) -> &CommandInterpreter {
        &self.parts.interpreter
    }

    fn reference(&self) -> &ReferenceData {
        &self.parts.reference
    }

    fn reference_mut(&mut self) -> &mut ReferenceData {
        &mut self.parts.reference
    }

    fn geo(&mut self, phase: GeoPhase<'_>) {
        let parts = &mut self.parts;
        let mut context = Context::new(
            &mut parts.document,
            &parts.interpreter,
            &mut parts.reference,
            &parts.scratch,
            &parts.front_end,
        );
        phase(&mut context);
    }

    fn headless(&self) -> bool {
        self.parts.headless
    }

    fn log(&mut self, text: &str, is_error: bool) {
        self.parts.logged.push((String::from(text), is_error));
    }

    fn warn_user(&mut self, title: &str, text: &str) {
        self.parts
            .warnings
            .push((String::from(title), String::from(text)));
    }

    fn views(&mut self) -> Option<&mut dyn ViewAccess> {
        self.parts
            .views
            .as_mut()
            .map(|views| -> &mut dyn ViewAccess { views.as_mut() })
    }

    fn jobs(&mut self) -> &mut dyn Jobs {
        &mut self.parts.jobs
    }

    fn wait_for(&mut self, id: JobId) {
        for job in self.parts.jobs.take(Some(id)) {
            complete(job, self);
        }
    }

    fn wait_for_all(&mut self) {
        self.deliver_finished_jobs();
    }

    fn await_job(&mut self, started: &VerbOutcome, done: JobAwait) -> bool {
        if !self.awaits_jobs {
            return false;
        }
        self.awaiting.push((started.clone(), done));
        true
    }

    fn clipboard(&mut self) -> &mut dyn Clipboard {
        &mut self.parts.clipboard
    }

    fn settings(&mut self) -> &mut UserSettings {
        &mut self.parts.settings
    }
}

/// Jobs that run where they are started, on the calling thread: a test's runner. A job's work runs
/// inside `start`; its completion waits, the job still active, until the host delivers it
/// (`Host::wait_for`, or a frame's turn), so a cancel between the start and the delivery discards
/// the result as the window's runner does.
#[derive(Default)]
pub struct InlineJobs {
    next_id: JobId,
    waiting: Vec<(CompletedJob, Arc<AtomicBool>, Arc<JobShared>)>,
}

impl InlineJobs {
    /// The completions of job `id`, or of every job, in the order the jobs were started; they are
    /// no longer active.
    pub fn take(&mut self, id: Option<JobId>) -> Vec<CompletedJob> {
        let mut taken = Vec::new();
        let mut kept = Vec::new();
        for (mut job, stop, shared) in std::mem::take(&mut self.waiting) {
            if id.is_none_or(|id| id == job.id) {
                job.stop_requested = stop.load(Ordering::Acquire);
                taken.push(job);
            } else {
                kept.push((job, stop, shared));
            }
        }
        self.waiting = kept;
        taken
    }
}

impl Jobs for InlineJobs {
    fn start(&mut self, title: &str, work: JobWork, finished: Option<JobFinished>) -> JobId {
        self.next_id += 1;
        let id = self.next_id;
        debug_assert_ne!(id, NO_JOB);
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(JobShared::new());
        let control = JobControl::new(Arc::clone(&stop), Arc::clone(&shared));
        let (result, work_seconds) = run_work(work, &control);
        let job = CompletedJob {
            id,
            title: String::from(title),
            stop_requested: false,
            result: Some(result),
            work_seconds,
            finished,
        };
        self.waiting.push((job, stop, shared));
        id
    }

    fn cancel(&mut self, id: JobId) -> bool {
        match self.waiting.iter().find(|(job, _, _)| job.id == id) {
            Some((_, stop, _)) => {
                stop.store(true, Ordering::Release);
                true
            }
            None => false,
        }
    }

    fn cancel_all(&mut self) {
        for (_, stop, _) in &self.waiting {
            stop.store(true, Ordering::Release);
        }
    }

    fn active_count(&self) -> usize {
        self.waiting.len()
    }

    fn is_active(&self, id: JobId) -> bool {
        self.waiting.iter().any(|(job, _, _)| job.id == id)
    }

    fn progress(&self) -> Vec<JobProgress> {
        self.waiting
            .iter()
            .map(|(job, _, shared)| JobProgress {
                id: job.id,
                title: job.title.clone(),
                fraction: shared.fraction(),
                stage: shared.stage(),
            })
            .collect()
    }
}
