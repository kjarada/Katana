//! Work that may take long - reading a large field file, reducing and adjusting it - run OFF the
//! interface thread, with a row that says it is running and a button that cancels it
//! (survey_task.hpp/.cpp).
//!
//! Why a row of its own rather than a progress dialog: a modal box would stop the person reading
//! the page they are on, and a headless run must never open one. Why no percentage: the readers
//! and the reduction report no progress (their contract has no callback), so the bar is a busy
//! bar with the time so far - honest about what is known.
//!
//! Cancel ABANDONS the work: the interface is free again at once and the result is thrown away
//! when it arrives. The work itself cannot be interrupted inside a reader or the reduction
//! (neither takes a stop token), so it runs to its end on its own thread and nothing waits for
//! it.
//!
//! Small inputs do not go to a thread at all (`in_background` false): they are done at once, on
//! the interface thread, so a headless run that presses Next and reads the page straight after
//! sees the page complete.
//!
//! The work runs on another thread, so it may touch NOTHING of the window or the Document: it is
//! given copies (or shared, immutable data) and returns a value, which the page applies on the
//! interface thread - if and only if the task was not cancelled or superseded. The C++'s work
//! returned a continuation that captured the page; a Rust page is not shared with a thread, so
//! the work returns the RESULT and the page, polling at each frame, takes it (`poll`).
//!
//! The thread is the task's own, as the C++'s was the global thread pool's and not the window's
//! job runner (jobs.cpp): the window's runner applies through the window, where this result
//! belongs to one page.
//!
//! Object names: task (the row), taskProgress, taskStatus, cancelTask.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use egui::{Context, Label, Ui};

use super::survey_dialogs::hidden;
use crate::drive::{self, ControlKind, ControlRecord};
use crate::format::qt_fixed;

/// How often the row says how long the task has run: the C++'s 200 ms tick.
const TICK: Duration = Duration::from_millis(200);

/// The text of a panic's payload; the C++ caught an exception's what(), and anything else as "an
/// unknown failure".
fn failure_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| String::from(*text))
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| String::from("an unknown failure"))
}

type Delivery<T> = (u64, std::result::Result<T, String>);

/// The row of a page that runs a task (the C++ SurveyTaskBar), over the result type `T` the
/// page's work returns.
pub struct SurveyTaskBar<T> {
    busy: bool,
    visible: bool,
    what: String,
    status: String,
    started: Option<Instant>,
    /// Bumped by every run and cancel: a result whose ticket is not the current one belongs to a
    /// task nobody is waiting for any more.
    ticket: u64,
    pending: Option<Receiver<Delivery<T>>>,
}

impl<T: Send + 'static> Default for SurveyTaskBar<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Send + 'static> SurveyTaskBar<T> {
    /// Hidden, and running nothing.
    pub fn new() -> Self {
        Self {
            busy: false,
            visible: false,
            what: String::new(),
            status: String::new(),
            started: None,
            ticket: 0,
            pending: None,
        }
    }

    pub fn busy(&self) -> bool {
        self.busy
    }

    /// Whether the row is shown: while a task runs, and after one failed, to say so.
    pub fn visible(&self) -> bool {
        self.visible
    }

    /// What the row says.
    pub fn status(&self) -> &str {
        &self.status
    }

    /// Runs `work` - on a thread of its own when `in_background`, at once otherwise. At once, its
    /// result is answered here; in the background, None, and the result comes from `poll` when
    /// the work is done. `what` is the sentence the row shows while it runs ("Reading DAY1.GSI
    /// (50.2 MB)"). A task started while another runs supersedes it: the older result is thrown
    /// away. `ctx` is asked for a frame when the work is done, so the result is taken at once.
    pub fn run(
        &mut self,
        what: &str,
        in_background: bool,
        ctx: &Context,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Option<T> {
        self.ticket += 1;
        let ticket = self.ticket;
        if !in_background {
            // At once, as the page did before there were threads: a small file is read in less
            // time than the row would take to appear.
            self.pending = None;
            self.set_busy(false);
            return Some(work());
        }
        self.what = String::from(what);
        self.started = Some(Instant::now());
        self.status = format!("{what}...");
        self.set_busy(true);
        let (sender, receiver) = channel();
        self.pending = Some(receiver);
        let ctx = ctx.clone();
        let _ = std::thread::spawn(move || {
            // What reaches here is the machine running out of something: the readers and the
            // reduction report their failures in their results.
            let result =
                catch_unwind(AssertUnwindSafe(work)).map_err(|payload| failure_text(&*payload));
            // The page may be gone by now, and its receiver with it: nobody is told.
            let _ = sender.send((ticket, result));
            ctx.request_repaint();
        });
        None
    }

    /// The result of the task running in the background, once it is done and still wanted: None
    /// while it runs, after it was cancelled or superseded, and when it failed - the row then
    /// says so.
    pub fn poll(&mut self) -> Option<T> {
        let receiver = self.pending.as_ref()?;
        let delivered = receiver.try_recv();
        let (ticket, result) = match delivered {
            Ok(delivery) => delivery,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => {
                self.pending = None;
                return None;
            }
        };
        self.pending = None;
        if ticket != self.ticket {
            return None; // cancelled or superseded
        }
        self.set_busy(false);
        match result {
            Ok(value) => Some(value),
            Err(failure) => {
                self.status = format!("{} failed: {failure}", self.what);
                self.visible = true;
                None
            }
        }
    }

    /// Abandons the running task, if any: true when there was one, for the page to say so (the
    /// C++'s onCancelled).
    pub fn cancel(&mut self) -> bool {
        if !self.busy {
            return false;
        }
        self.ticket += 1;
        self.pending = None;
        self.set_busy(false);
        true
    }

    fn set_busy(&mut self, busy: bool) {
        self.busy = busy;
        self.visible = busy;
    }

    /// Draws the row, hidden while no task runs and none has failed. True when its Cancel was
    /// pressed and a task was abandoned.
    pub fn ui(&mut self, ui: &mut Ui) -> bool {
        if self.busy
            && let Some(started) = self.started
        {
            let elapsed = started.elapsed();
            if elapsed >= TICK {
                let seconds = elapsed.as_millis() as f64 / 1000.0;
                self.status = format!("{} - {} s", self.what, qt_fixed(seconds, 1));
            }
            // The time so far, at the C++'s tick: not a frame at once, so a window whose task
            // runs still comes idle between two ticks.
            ui.ctx().request_repaint_after(TICK);
        }
        // A busy bar with no measure: the share of a two-second sweep the time so far is.
        let sweep = self
            .started
            .filter(|_| self.busy)
            .map_or(0.0, |started| (started.elapsed().as_secs_f32() % 2.0) / 2.0);
        let status = self.status.clone();
        let mut pressed = false;
        let mut draw = |ui: &mut Ui| {
            let row = ui.horizontal(|ui| {
                let progress = ui.add(egui::ProgressBar::new(sweep).desired_width(160.0));
                drive::custom(
                    ui,
                    &progress,
                    ControlRecord::new("taskProgress", ControlKind::Other),
                );
                drive::label_as(
                    ui,
                    "taskStatus",
                    &status,
                    Label::new(status.as_str()).wrap(),
                );
                if drive::button(ui, "cancelTask", "Cancel").clicked() {
                    pressed = true;
                }
            });
            drive::custom(
                ui,
                &row.response,
                ControlRecord::new("task", ControlKind::Other),
            );
        };
        if self.visible {
            draw(ui);
        } else {
            hidden(ui, "task", draw);
        }
        pressed && self.cancel()
    }
}
