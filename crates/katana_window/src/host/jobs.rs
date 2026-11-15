//! Background jobs, as a dialog and a workbench see them: the port of jobs.hpp's types and the
//! rules of a job's end (jobs.cpp's `complete`). The runner itself - a thread a job, the
//! completion channel drained each frame, the status-bar widget with its Cancel - is H's, over
//! the `Jobs` trait here.
//!
//! WHY. Building a surface from 400k points takes 1.7 s and comparing two surfaces 2 s
//! (docs/terrain.md, measured). Run on the interface thread, that is a window that stops
//! repainting and answering the mouse. A job runs the computation on its own thread instead, shows
//! progress in the status bar, and can be cancelled.
//!
//! THE RULE THAT MAKES IT SAFE. The Document is single-threaded (docs/architecture.md, Threading),
//! and so is every widget. A job therefore never touches either: its work computes a PURE result
//! from inputs the job owns - copied on the interface thread before it starts - and returns a
//! `JobApply`, which runs on the interface thread with the window as its host, where it changes
//! the document as ONE command or hands the result to the window. Nothing a job computes is
//! visible until it is applied, so a cancelled or failed job leaves no trace.
//!
//! FAILURE IS REPORTED, NEVER LOST. A work fails by returning an Error, or by panicking; both
//! arrive at the job's `JobFinished` as `JobOutcome::Failed` with the message. A panic out of the
//! apply is caught and reported the same way.
//!
//! CANCELLATION is cooperative. `Jobs::cancel` requests a stop through the job's stop flag; a work
//! that asks `JobControl::stop_requested` ends early, and one that cannot be interrupted (a
//! triangulation) runs to the end and is then DISCARDED - a cancelled job never applies, whenever
//! the request arrived.
//!
//! The Rust shape. The C++'s Apply and Finished capture the window (`this`) and run on its thread;
//! here they are handed the window as `&mut dyn Host` when they run, since nothing can hold the
//! window across a frame. The runner never runs them itself: the window takes each completed job
//! out of it at the start of a frame and calls `complete` with itself as the host (a headless run
//! does the same in `Host::wait_for`).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use katana_core::{Error, ErrorCode, Result};

use super::command_runner::Host;

pub type JobId = u64;
/// No job: the C++'s kNoJob.
pub const NO_JOB: JobId = 0;

/// The state the runner and the worker share: progress in thousandths (-1 while the job has not
/// reported, shown as busy) and the stage's phrase.
#[derive(Debug)]
pub struct JobShared {
    permille: AtomicI32,
    stage: Mutex<String>,
}

impl Default for JobShared {
    fn default() -> Self {
        Self {
            permille: AtomicI32::new(-1),
            stage: Mutex::new(String::new()),
        }
    }
}

impl JobShared {
    pub fn new() -> Self {
        Self::default()
    }

    /// The fraction done, 0 to 1; below zero while the job has reported none.
    pub fn fraction(&self) -> f64 {
        let permille = self.permille.load(Ordering::Relaxed);
        if permille < 0 {
            -1.0
        } else {
            f64::from(permille) / 1000.0
        }
    }

    pub fn stage(&self) -> String {
        self.stage
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// What a work sees: its stop request and a place to report progress. Every member is safe to
/// call from the worker thread.
#[derive(Clone, Debug)]
pub struct JobControl {
    stop: Arc<AtomicBool>,
    shared: Arc<JobShared>,
}

impl JobControl {
    pub fn new(stop: Arc<AtomicBool>, shared: Arc<JobShared>) -> Self {
        Self { stop, shared }
    }

    /// The C++'s `stop_token().stop_requested()`.
    pub fn stop_requested(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }

    /// The stop itself, for a work that hands it on: the geo executor's `Work` takes it as
    /// `Option<&AtomicBool>` (katana_app::geo).
    pub fn stop_flag(&self) -> &AtomicBool {
        &self.stop
    }

    /// Fraction done in [0, 1]; values outside are clamped and NaN is ignored. A job that never
    /// calls this is shown as busy with no measure, which is the honest display for a
    /// triangulation that cannot say how far it is.
    pub fn set_progress(&self, fraction: f64) {
        if fraction.is_nan() {
            return;
        }
        let clamped = katana_math::cpp::clamp(fraction, 0.0, 1.0);
        // std::lround: half away from zero, which f64::round is; at most 1000.
        self.shared
            .permille
            .store((clamped * 1000.0).round() as i32, Ordering::Relaxed);
    }

    /// A short phrase for what the job is doing now ("Triangulating").
    pub fn set_stage(&self, stage: &str) {
        *self
            .shared
            .stage
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = String::from(stage);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum JobOutcome {
    #[default]
    Finished,
    Cancelled,
    Failed,
}

/// Delivered to a job's `JobFinished` on the interface thread, once per job.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JobReport {
    pub id: JobId,
    pub title: String,
    pub outcome: JobOutcome,
    /// Failed: what the work returned or panicked with, or what the apply panicked with.
    pub error: String,
    /// Wall time of the work on its thread, and of the apply on the interface thread. The second
    /// is the part of the job the window still waits for, which is why it is measured apart.
    pub work_seconds: f64,
    pub apply_seconds: f64,
}

/// What the status bar shows for one running job.
#[derive(Clone, Debug, PartialEq)]
pub struct JobProgress {
    pub id: JobId,
    pub title: String,
    /// Below zero: busy, no measure.
    pub fraction: f64,
    pub stage: String,
}

impl Default for JobProgress {
    fn default() -> Self {
        Self {
            id: NO_JOB,
            title: String::new(),
            fraction: -1.0,
            stage: String::new(),
        }
    }
}

/// The third phase: run on the interface thread with the window, once, unless the job was
/// cancelled or failed.
pub type JobApply = Box<dyn FnOnce(&mut dyn Host) + Send>;
/// The second phase: run on a job thread. None is "nothing to apply" (the C++'s empty Apply).
pub type JobWork = Box<dyn FnOnce(&JobControl) -> Result<Option<JobApply>> + Send>;
/// Told the job's end on the interface thread, with the window, after the apply has run (or
/// instead of it, when the job was cancelled or failed).
pub type JobFinished = Box<dyn FnOnce(&JobReport, &mut dyn Host)>;

/// A JobWork from a closure, its control's lifetime left open as JobWork's is.
pub fn job_work<F>(work: F) -> JobWork
where
    F: FnOnce(&JobControl) -> Result<Option<JobApply>> + Send + 'static,
{
    Box::new(work)
}

/// A JobApply from a closure.
pub fn job_apply<F>(apply: F) -> JobApply
where
    F: FnOnce(&mut dyn Host) + Send + 'static,
{
    Box::new(apply)
}

/// A JobFinished from a closure.
pub fn job_finished<F>(finished: F) -> JobFinished
where
    F: FnOnce(&JobReport, &mut dyn Host) + 'static,
{
    Box::new(finished)
}

/// The window's job runner, as a workbench starts and cancels jobs (the C++ JobRunner's public
/// calls but the status widget and the waits, which are the runner's and the host's).
pub trait Jobs {
    /// Starts `work` on a thread of its own, titled for the status bar; `finished`, when given,
    /// is called on the interface thread after the apply has run (or instead of it, when the job
    /// was cancelled or failed). Must be called on the interface thread.
    fn start(&mut self, title: &str, work: JobWork, finished: Option<JobFinished>) -> JobId;
    /// Requests a stop. False when the job is not running (it already ended).
    fn cancel(&mut self, id: JobId) -> bool;
    fn cancel_all(&mut self);
    /// The jobs started and not yet completed: a job is active until its finish was delivered.
    fn active_count(&self) -> usize;
    fn is_active(&self, id: JobId) -> bool;
    /// What the status bar shows, one row a running job, in the order they were started.
    fn progress(&self) -> Vec<JobProgress>;
}

/// A job whose work has ended, waiting to be completed on the interface thread.
pub struct CompletedJob {
    pub id: JobId,
    pub title: String,
    /// Whether a stop was requested, whenever it was: a cancelled job applies nothing.
    pub stop_requested: bool,
    /// What the work returned; None when it produced nothing.
    pub result: Option<Result<Option<JobApply>>>,
    pub work_seconds: f64,
    pub finished: Option<JobFinished>,
}

/// The text of a panic's payload, or None for a payload that is no text.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> Option<String> {
    payload
        .downcast_ref::<&str>()
        .map(|text| String::from(*text))
        .or_else(|| payload.downcast_ref::<String>().cloned())
}

/// Runs a work on the calling thread - the job's own, in the window's runner - as the C++'s job
/// thread does: a panic is caught and becomes the failure "the job failed: <text>", and the time
/// it took is measured. The work's inputs are freed here, on the worker.
pub fn run_work(work: JobWork, control: &JobControl) -> (Result<Option<JobApply>>, f64) {
    let started = Instant::now();
    let result = match catch_unwind(AssertUnwindSafe(move || work(control))) {
        Ok(result) => result,
        Err(payload) => Err(match panic_text(payload.as_ref()) {
            Some(text) => Error::new(ErrorCode::Internal, format!("the job failed: {text}")),
            None => Error::new(
                ErrorCode::Internal,
                "the job failed with an exception of an unknown type",
            ),
        }),
    };
    (result, started.elapsed().as_secs_f64())
}

/// A job's end, on the interface thread (jobs.cpp's `complete`): a job whose stop was requested is
/// Cancelled and applies nothing, whatever it produced; one that failed or produced nothing is
/// Failed with the reason; otherwise its apply runs with `host` - a panic out of it caught as
/// "applying the result failed: <text>" - and it is Finished. Then its `finished`, with the report,
/// which is also answered (the runner keeps its longest pause from `apply_seconds`).
pub fn complete(job: CompletedJob, host: &mut dyn Host) -> JobReport {
    let CompletedJob {
        id,
        title,
        stop_requested,
        result,
        work_seconds,
        finished,
    } = job;
    let mut report = JobReport {
        id,
        title,
        work_seconds,
        ..JobReport::default()
    };
    if stop_requested {
        // Whatever the work produced, a cancelled job applies nothing: the user said no, and a
        // result arriving afterwards must not undo that.
        report.outcome = JobOutcome::Cancelled;
    } else {
        match result {
            None => {
                report.outcome = JobOutcome::Failed;
                report.error = String::from("the job produced no result");
            }
            Some(Err(error)) => {
                report.outcome = JobOutcome::Failed;
                report.error = error.describe();
            }
            Some(Ok(apply)) => {
                let applying = Instant::now();
                let applied = catch_unwind(AssertUnwindSafe(|| {
                    if let Some(apply) = apply {
                        apply(&mut *host);
                    }
                }));
                match applied {
                    Ok(()) => report.outcome = JobOutcome::Finished,
                    Err(payload) => {
                        report.outcome = JobOutcome::Failed;
                        report.error = match panic_text(payload.as_ref()) {
                            Some(text) => format!("applying the result failed: {text}"),
                            None => String::from(
                                "applying the result failed with an exception of an unknown type",
                            ),
                        };
                    }
                }
                report.apply_seconds = applying.elapsed().as_secs_f64();
            }
        }
    }
    if let Some(finished) = finished {
        finished(&report, host);
    }
    report
}
