//! The window's job runner (jobs.hpp/.cpp): each job's work on a thread of its own, its end
//! brought back to the interface thread, its progress in the status bar with a Cancel.
//!
//! The rules of a job - a pure work on its thread, an apply on the interface thread with the window,
//! a cancelled job discarded whenever the cancel arrived, a failure reported and never lost - are
//! F0's `host::jobs` (`run_work` and `complete`), the one home of jobs.cpp's catching and
//! completion rules. This module is the runner over them, and never calls an apply or a finish
//! itself: it hands each ended job back (`CompletedJob`) and the window completes it with itself
//! as the host (`finish`, or `JobHost`'s helpers when the window owns its runner).
//!
//! THE SHAPE (decided here, the plan's section 4.2). The C++ posted each job's completion to a
//! context object with `QMetaObject::invokeMethod`, and the event loop delivered it. egui has no
//! event loop of its own to post to, so:
//!
//! * a worker sends its job's end down a channel and asks the egui context for a repaint
//!   (`request_repaint`), so a frame comes to take it - a window asleep with nothing to draw wakes
//!   for it;
//! * every frame begins with `frame`, which drains the channel and hands back the jobs that ended;
//! * while a job runs, `frame` asks for the next frame a tick away (`request_repaint_after`), so
//!   frames keep coming - the status bar's progress moves, as the C++'s ten-a-second timer moved
//!   it - and the runner measures the frame time: the wall time from one frame's start to the
//!   next's, the freeze a person would feel. That is the C++'s "longest event-loop pause", read
//!   from frames where the C++ read it from a 10 ms timer: `TheEventLoopKeepsRunningWhileAJobComputes`
//!   becomes "frames keep coming while a job computes", and its control, that a blocked interface
//!   thread is seen, holds the frame time to the same 250 ms;
//! * a headless run, and a test, which have no frames to wait in, wait in `wait_completed`: a
//!   receive on the channel that wakes at least once a tick, so the measure keeps being taken, as
//!   the C++'s `waitFor` ran a real event loop in which the timer kept ticking.
//!
//! Dropping the runner requests a stop of every job, then joins every thread - a request to all
//! before waiting for any, so the jobs that can stop early do so together - and the ends not yet
//! taken are dropped with the channel, undelivered: no apply and no finish runs after the runner
//! is gone, which is what makes it safe for a finish to reach the window that owned the runner.
//!
//! Rejected: a job polled for its end each frame (a `JoinHandle::is_finished` scan): it could not
//! wake a window asleep, and a headless wait would spin; an async executor: one more machinery for
//! what one channel does.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use egui::Ui;
use katana_core::{Error, ErrorCode, Result};
use katana_math::cpp;

use crate::drive::{ControlKind, ControlRecord, button, custom, label};
use crate::host::{
    CompletedJob, Host, JobApply, JobControl, JobFinished, JobId, JobProgress, JobReport,
    JobShared, JobWork, Jobs, complete, run_work,
};

/// How often frames come while a job runs, and how long a headless wait sleeps at most between
/// two looks at the measure: short enough that a pause is measured to about a hundredth of a
/// second and the progress bar moves smoothly; long enough to cost nothing measurable (the C++'s
/// kTickMs).
pub const TICK: Duration = Duration::from_millis(10);

/// What a worker sends back when its work has ended.
struct Ended {
    id: JobId,
    result: Result<Option<JobApply>>,
    work_seconds: f64,
}

/// A job started and not yet handed back.
struct Job {
    id: JobId,
    title: String,
    finished: Option<JobFinished>,
    stop: Arc<AtomicBool>,
    shared: Arc<JobShared>,
    /// None when no thread could be started (its failure is already in the channel).
    thread: Option<JoinHandle<()>>,
}

/// The window's job runner (see the module comment). It implements F0's `Jobs`, which is what a
/// workbench starts and cancels jobs through.
pub struct JobRunner {
    jobs: Vec<Job>,
    next_id: JobId,
    sender: Sender<Ended>,
    receiver: Receiver<Ended>,
    /// The context a worker wakes when its job ends; None before the first frame, and in a
    /// headless wait or a test with no frames.
    repaint: Option<egui::Context>,
    longest_pause: f64,
    /// When the frame before was begun, while jobs are active: the C++'s lastTickNs_, with its
    /// timer stopped (None) while nothing runs.
    last_tick: Option<Instant>,
}

impl Default for JobRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl JobRunner {
    pub fn new() -> Self {
        let (sender, receiver) = channel();
        Self {
            jobs: Vec::new(),
            next_id: 1,
            sender,
            receiver,
            repaint: None,
            longest_pause: 0.0,
            last_tick: None,
        }
    }

    /// The context a worker asks for a repaint when its job ends, so a frame comes to take it.
    /// `frame` sets it for the jobs started after the first frame; a runner made for a window sets
    /// it before then.
    pub fn set_repaint(&mut self, context: Option<egui::Context>) {
        self.repaint = context;
    }

    /// The start of a frame: the frame time measured, frames kept coming while a job runs, and
    /// the jobs whose work has ended handed back, in the order they ended, for the window to
    /// complete with itself as the host (`finish`).
    pub fn frame(&mut self, context: &egui::Context) -> Vec<CompletedJob> {
        if self.repaint.is_none() {
            self.repaint = Some(context.clone());
        }
        self.tick();
        let ended = self.take_completed();
        if !self.jobs.is_empty() {
            context.request_repaint_after(TICK);
        }
        ended
    }

    /// The jobs whose work has ended, taken without waiting.
    pub fn take_completed(&mut self) -> Vec<CompletedJob> {
        let mut taken = Vec::new();
        while let Ok(ended) = self.receiver.try_recv() {
            if let Some(job) = self.completed(ended) {
                taken.push(job);
            }
        }
        taken
    }

    /// Waits up to `limit` for a job to end, measuring the pause as it wakes, and hands back every
    /// job that has ended: a headless run's and a test's turn of the event loop. Nothing to wait
    /// for when no job is active.
    pub fn wait_completed(&mut self, limit: Duration) -> Vec<CompletedJob> {
        self.tick();
        let mut taken = Vec::new();
        if self.jobs.is_empty() {
            return taken;
        }
        if let Ok(ended) = self.receiver.recv_timeout(limit)
            && let Some(job) = self.completed(ended)
        {
            taken.push(job);
        }
        taken.extend(self.take_completed());
        taken
    }

    /// A job's end, completed with `host` (F0's `complete`: its apply, or its cancellation or
    /// failure, then its finish), and its apply's time taken into the measure.
    pub fn finish(&mut self, job: CompletedJob, host: &mut dyn Host) -> JobReport {
        let report = complete(job, host);
        self.note_applied(&report);
        report
    }

    /// The apply's time is interface-thread time a person waits through, and the frames cannot
    /// see it once the last job has gone: it counts as a pause.
    pub fn note_applied(&mut self, report: &JobReport) {
        self.longest_pause = cpp::max(self.longest_pause, report.apply_seconds);
    }

    /// JobRunner::waitFor, for a runner held apart from its host (a test's): the window waited
    /// in until job `id` has ended and its finish was delivered. A host that owns its runner uses
    /// `wait_for_job`. An interactive caller never waits: that would be the freeze jobs exist to
    /// remove.
    pub fn wait_for(&mut self, id: JobId, host: &mut dyn Host) {
        while self.is_active(id) {
            for job in self.wait_completed(TICK) {
                self.finish(job, host);
            }
        }
    }

    /// JobRunner::waitForAll, for a runner held apart from its host.
    pub fn wait_for_all(&mut self, host: &mut dyn Host) {
        while !self.jobs.is_empty() {
            for job in self.wait_completed(TICK) {
                self.finish(job, host);
            }
        }
    }

    /// The longest the window went without a frame while jobs were active, in seconds, or the
    /// longest apply if that was longer: the freeze a person would feel, and what proves the work
    /// is off the interface thread.
    pub fn longest_event_loop_pause(&self) -> f64 {
        self.longest_pause
    }

    pub fn reset_event_loop_pause(&mut self) {
        self.longest_pause = 0.0;
    }

    /// One measure of the time since the frame before, while jobs are active.
    pub fn tick(&mut self) {
        if let Some(last) = self.last_tick {
            let now = Instant::now();
            self.longest_pause =
                cpp::max(self.longest_pause, now.duration_since(last).as_secs_f64());
            self.last_tick = Some(now);
        }
    }

    /// A job's end matched to the job it ends: taken out of the list before anything runs, since
    /// its apply and finish may start another job or wait, and neither must see this one as
    /// running. Its thread's last act was the send that brought it here, so the join waits for at
    /// most the return from it.
    fn completed(&mut self, ended: Ended) -> Option<CompletedJob> {
        let at = self.jobs.iter().position(|job| job.id == ended.id)?;
        let mut job = self.jobs.remove(at);
        let stop_requested = job.stop.load(Ordering::Acquire);
        if let Some(thread) = job.thread.take() {
            let _ = thread.join();
        }
        if self.jobs.is_empty() {
            self.last_tick = None;
        }
        Some(CompletedJob {
            id: job.id,
            title: job.title,
            stop_requested,
            result: Some(ended.result),
            work_seconds: ended.work_seconds,
            finished: job.finished,
        })
    }

    /// What the status bar's label says: the first job's title, its stage, how many more run, and
    /// whether it is being cancelled; None while nothing runs.
    pub fn status_text(&self) -> Option<String> {
        let first = self.jobs.first()?;
        let mut text = first.title.clone();
        let stage = first.shared.stage();
        if !stage.is_empty() {
            text.push_str(": ");
            text.push_str(&stage);
        }
        if self.jobs.len() > 1 {
            text.push_str(&format!(" (+{} more)", self.jobs.len() - 1));
        }
        if first.stop.load(Ordering::Acquire) {
            text.push_str(" - cancelling");
        }
        Some(text)
    }

    /// The status-bar widget (createStatusWidget): a label, a progress bar and a Cancel button,
    /// hidden while nothing runs. Its Cancel cancels the first job. Named as the C++'s:
    /// `JobStatus`, `JobStatusLabel`, `JobStatusProgress` (recorded with the value "busy" while the
    /// job has reported no progress - the C++'s bar of range 0 to 0 - else its thousandths) and
    /// `JobStatusCancel`.
    pub fn status_ui(&mut self, ui: &mut Ui) {
        let text = self.status_text();
        let running = text.is_some();
        let text = text.unwrap_or_default();
        let fraction = self
            .jobs
            .first()
            .map_or(-1.0, |first| first.shared.fraction());
        let shown = ui.scope(|ui| {
            if !running {
                ui.set_invisible();
            }
            let row = ui.horizontal(|ui| {
                label(ui, "JobStatusLabel", &text);
                let (shown_fraction, value) = if fraction < 0.0 {
                    // Busy: the job cannot say how far it is. The bar sweeps with the frames
                    // that keep coming while it runs, and asks for none of its own.
                    let time = ui.input(|input| input.time);
                    ((time * 0.5).fract() as f32, String::from("busy"))
                } else {
                    (
                        fraction as f32,
                        format!("{}", (fraction * 1000.0).round() as i64),
                    )
                };
                let bar = ui.add(
                    egui::ProgressBar::new(shown_fraction)
                        .desired_width(140.0)
                        .desired_height(14.0),
                );
                let mut record = ControlRecord::new("JobStatusProgress", ControlKind::Other);
                record.value = value;
                custom(ui, &bar, record);
                button(ui, "JobStatusCancel", "Cancel")
                    .on_hover_text("Stop the job shown here; nothing it has computed is applied")
                    .clicked()
            });
            let mut record = ControlRecord::new("JobStatus", ControlKind::Other);
            record.text = text.clone();
            custom(ui, &row.response, record);
            row.inner
        });
        if shown.inner
            && let Some(first) = self.jobs.first()
        {
            let id = first.id;
            self.cancel(id);
        }
    }
}

impl Jobs for JobRunner {
    fn start(&mut self, title: &str, work: JobWork, finished: Option<JobFinished>) -> JobId {
        let id = self.next_id;
        self.next_id += 1;
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(JobShared::new());
        let control = JobControl::new(Arc::clone(&stop), Arc::clone(&shared));
        let sender = self.sender.clone();
        let repaint = self.repaint.clone();
        let spawned = std::thread::Builder::new()
            .name(format!("katana job {id}"))
            .spawn(move || {
                let (result, work_seconds) = run_work(work, &control);
                // Sent, never called: the apply and the finish belong to the interface thread.
                // If the runner is gone, the end dies undelivered with its channel.
                let _ = sender.send(Ended {
                    id,
                    result,
                    work_seconds,
                });
                if let Some(context) = repaint {
                    context.request_repaint();
                }
            });
        let thread = match spawned {
            Ok(thread) => Some(thread),
            Err(error) => {
                // The C++'s std::jthread throws std::system_error here, which nothing catches: a
                // job that cannot have a thread is reported as failed instead, the work dropped
                // unrun. Not reached by a test (a process out of threads).
                let _ = self.sender.send(Ended {
                    id,
                    result: Err(Error::new(
                        ErrorCode::Internal,
                        format!("the job failed: {error}"),
                    )),
                    work_seconds: 0.0,
                });
                None
            }
        };
        self.jobs.push(Job {
            id,
            title: String::from(title),
            finished,
            stop,
            shared,
            thread,
        });
        if self.last_tick.is_none() {
            self.last_tick = Some(Instant::now());
        }
        id
    }

    fn cancel(&mut self, id: JobId) -> bool {
        match self.jobs.iter().find(|job| job.id == id) {
            Some(job) => {
                job.stop.store(true, Ordering::Release);
                true
            }
            None => false,
        }
    }

    fn cancel_all(&mut self) {
        for job in &self.jobs {
            job.stop.store(true, Ordering::Release);
        }
    }

    fn active_count(&self) -> usize {
        self.jobs.len()
    }

    fn is_active(&self, id: JobId) -> bool {
        self.jobs.iter().any(|job| job.id == id)
    }

    fn progress(&self) -> Vec<JobProgress> {
        self.jobs
            .iter()
            .map(|job| JobProgress {
                id: job.id,
                title: job.title.clone(),
                fraction: job.shared.fraction(),
                stage: job.shared.stage(),
            })
            .collect()
    }
}

impl Drop for JobRunner {
    fn drop(&mut self) {
        // Stop everything first, then join: a request to all before waiting for any.
        for job in &self.jobs {
            job.stop.store(true, Ordering::Release);
        }
        for job in &mut self.jobs {
            if let Some(thread) = job.thread.take() {
                let _ = thread.join();
            }
        }
        // The channel goes with the runner, and with it every end the threads sent and nobody
        // took: their applies and finishes are dropped, never run.
    }
}

/// A host that owns its job runner: the main window's executor. The helpers below are the one way
/// such a host completes its jobs, so its `Host::wait_for` and its frame do as `JobRunner::wait_for`
/// does for a runner held apart.
pub trait JobHost: Host {
    fn job_runner(&mut self) -> &mut JobRunner;
}

/// One job's end completed with the host that owns the runner, its apply's time measured.
fn finish_job<H: JobHost>(host: &mut H, job: CompletedJob) -> JobReport {
    let report = complete(job, &mut *host);
    host.job_runner().note_applied(&report);
    report
}

/// A frame's turn for the jobs (the start of every frame of the window): the measure taken, frames
/// kept coming while jobs run, and every job that has ended completed with `host`.
pub fn deliver_jobs<H: JobHost>(host: &mut H, context: &egui::Context) -> Vec<JobReport> {
    let ended = host.job_runner().frame(context);
    ended
        .into_iter()
        .map(|job| finish_job(&mut *host, job))
        .collect()
}

/// `Host::wait_for` of a host that owns its runner: waits until job `id` has ended and its finish
/// was delivered, completing every job that ends meanwhile.
pub fn wait_for_job<H: JobHost>(host: &mut H, id: JobId) {
    while host.job_runner().is_active(id) {
        let ended = host.job_runner().wait_completed(TICK);
        for job in ended {
            finish_job(&mut *host, job);
        }
    }
}

/// `Host::wait_for_all` of a host that owns its runner.
pub fn wait_for_all_jobs<H: JobHost>(host: &mut H) {
    while host.job_runner().active_count() > 0 {
        let ended = host.job_runner().wait_completed(TICK);
        for job in ended {
            finish_job(&mut *host, job);
        }
    }
}
