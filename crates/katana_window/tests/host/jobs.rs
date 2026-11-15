//! The background job runner (src/katana_window/src/jobs.rs; jobs.hpp): work on its own thread,
//! apply and finish on the window's thread, progress read while it runs, cancel that discards,
//! failures reported rather than lost, and frames that keep coming while a job computes.
//!
//! Every wait here is on an event or a flag, never a fixed sleep standing in for "long enough";
//! the sleeps that remain are the WORK being timed, or the time between two frames that an event
//! loop would sleep through. A worker that spins on a flag the test sets also leaves its loop when
//! it is asked to stop, or the flag is set on the way out of the test (ReleaseOnExit), so a failed
//! assertion does not leave a worker the runner's drop would join for ever.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

use katana_core::{Error, ErrorCode};
use katana_window::drive::Harness;
use katana_window::host::{
    Host, JobControl, JobId, JobOutcome, JobReport, Jobs, RecordingHost, job_apply, job_finished,
    job_work,
};
use katana_window::jobs::{JobRunner, TICK};

/// Waits on the runner until `done` holds, for at most `limit`, completing every job that ends, so
/// a broken runner fails the test instead of hanging it (the C++'s pumpUntil).
fn pump_until(
    runner: &mut JobRunner,
    host: &mut RecordingHost,
    done: impl Fn() -> bool,
    limit: Duration,
) -> bool {
    let deadline = Instant::now() + limit;
    while !done() {
        if Instant::now() > deadline {
            return false;
        }
        for job in runner.wait_completed(Duration::from_millis(10)) {
            runner.finish(job, host);
        }
    }
    true
}

const THIRTY_SECONDS: Duration = Duration::from_secs(30);

/// Sets a flag when the test leaves its scope, however it leaves: for a worker that deliberately
/// does not look at its stop. Declared after the runner, so it is dropped first and the join that
/// follows finds the worker released.
struct ReleaseOnExit(Arc<AtomicBool>);

impl Drop for ReleaseOnExit {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// One frame of the window: the runner's turn, the jobs that ended completed with `host`, and the
/// time until the frame egui asks for next.
fn frame(harness: &mut Harness, runner: &mut JobRunner, host: &mut RecordingHost) -> Duration {
    let mut ended = Vec::new();
    let output = harness.frame(|ui| ended = runner.frame(ui.ctx()));
    for job in ended {
        runner.finish(job, host);
    }
    output
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .map_or(TICK, |viewport| viewport.repaint_delay)
}

/// The window's frames until `done` holds, for at most `limit`: each frame, then the sleep an
/// event loop would sleep until the frame egui asked for.
fn frames_until(
    harness: &mut Harness,
    runner: &mut JobRunner,
    host: &mut RecordingHost,
    done: impl Fn(&JobRunner) -> bool,
    limit: Duration,
) -> bool {
    let deadline = Instant::now() + limit;
    loop {
        let next = frame(harness, runner, host);
        if done(runner) {
            return true;
        }
        if Instant::now() > deadline {
            return false;
        }
        thread::sleep(next.min(TICK));
    }
}

mod job_runner {
    use super::*;

    #[test]
    fn work_runs_off_the_gui_thread_and_apply_and_finished_run_on_it() {
        let gui = thread::current().id();
        let worker: Arc<Mutex<Option<ThreadId>>> = Arc::new(Mutex::new(None));
        let applier: Arc<Mutex<Option<ThreadId>>> = Arc::new(Mutex::new(None));
        let applied = Arc::new(AtomicI32::new(0));
        let finisher: Rc<Cell<Option<ThreadId>>> = Rc::new(Cell::new(None));
        let report = Rc::new(RefCell::new(JobReport::default()));
        let mut host = RecordingHost::new();
        let mut runner = JobRunner::new();

        let id = runner.start(
            "sum",
            job_work({
                let worker = Arc::clone(&worker);
                let applier = Arc::clone(&applier);
                let applied = Arc::clone(&applied);
                move |_control: &JobControl| {
                    *worker.lock().unwrap() = Some(thread::current().id());
                    let value = 6 * 7;
                    Ok(Some(job_apply(move |_host: &mut dyn Host| {
                        *applier.lock().unwrap() = Some(thread::current().id());
                        applied.store(value, Ordering::SeqCst);
                    })))
                }
            }),
            Some(job_finished({
                let finisher = Rc::clone(&finisher);
                let report = Rc::clone(&report);
                move |ended: &JobReport, _host: &mut dyn Host| {
                    finisher.set(Some(thread::current().id()));
                    *report.borrow_mut() = ended.clone();
                }
            })),
        );
        assert!(runner.is_active(id));
        runner.wait_for(id, &mut host);

        assert!(!runner.is_active(id));
        assert_eq!(runner.active_count(), 0);
        assert_ne!(*worker.lock().unwrap(), Some(gui));
        assert!(worker.lock().unwrap().is_some());
        assert_eq!(*applier.lock().unwrap(), Some(gui));
        assert_eq!(finisher.get(), Some(gui));
        assert_eq!(applied.load(Ordering::SeqCst), 42);
        let report = report.borrow();
        assert_eq!(report.id, id);
        assert_eq!(report.title, "sum");
        assert_eq!(report.outcome, JobOutcome::Finished);
        assert!(report.error.is_empty());
    }

    #[test]
    fn progress_and_stage_reported_by_the_worker_are_readable_while_it_runs() {
        let reported = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let mut host = RecordingHost::new();
        let mut runner = JobRunner::new();
        let _release_on_exit = ReleaseOnExit(Arc::clone(&release));
        let id = runner.start(
            "staged",
            job_work({
                let reported = Arc::clone(&reported);
                let release = Arc::clone(&release);
                move |control: &JobControl| {
                    control.set_stage("Triangulating");
                    control.set_progress(0.25);
                    reported.store(true, Ordering::SeqCst);
                    while !release.load(Ordering::SeqCst) && !control.stop_requested() {
                        thread::yield_now();
                    }
                    Ok(None)
                }
            }),
            None,
        );

        assert!(pump_until(
            &mut runner,
            &mut host,
            || reported.load(Ordering::SeqCst),
            THIRTY_SECONDS
        ));
        let items = runner.progress();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, id);
        assert_eq!(items[0].title, "staged");
        // Stored as 250 thousandths.
        assert_eq!(items[0].fraction, 0.25);
        assert_eq!(items[0].stage, "Triangulating");
        release.store(true, Ordering::SeqCst);
        runner.wait_for(id, &mut host);
    }

    #[test]
    fn a_job_that_has_not_reported_progress_is_shown_as_busy_and_out_of_range_progress_is_clamped()
    {
        let step = Arc::new(AtomicI32::new(0));
        let go = Arc::new(AtomicI32::new(0));
        let mut host = RecordingHost::new();
        let mut runner = JobRunner::new();
        let id = runner.start(
            "busy",
            job_work({
                let step = Arc::clone(&step);
                let go = Arc::clone(&go);
                move |control: &JobControl| {
                    step.store(1, Ordering::SeqCst);
                    while go.load(Ordering::SeqCst) < 1 && !control.stop_requested() {
                        thread::yield_now();
                    }
                    // Clamps to 1.
                    control.set_progress(7.0);
                    step.store(2, Ordering::SeqCst);
                    while go.load(Ordering::SeqCst) < 2 && !control.stop_requested() {
                        thread::yield_now();
                    }
                    Ok(None)
                }
            }),
            None,
        );
        assert!(pump_until(
            &mut runner,
            &mut host,
            || step.load(Ordering::SeqCst) == 1,
            THIRTY_SECONDS
        ));
        assert!(runner.progress()[0].fraction < 0.0);
        go.store(1, Ordering::SeqCst);
        assert!(pump_until(
            &mut runner,
            &mut host,
            || step.load(Ordering::SeqCst) == 2,
            THIRTY_SECONDS
        ));
        assert_eq!(runner.progress()[0].fraction, 1.0);
        go.store(2, Ordering::SeqCst);
        runner.wait_for(id, &mut host);
    }

    #[test]
    fn a_cancelled_job_that_watches_its_stop_token_ends_early_and_applies_nothing() {
        let running = Arc::new(AtomicBool::new(false));
        let applied = Arc::new(AtomicBool::new(false));
        let report = Rc::new(RefCell::new(JobReport::default()));
        let mut host = RecordingHost::new();
        let mut runner = JobRunner::new();
        let id = runner.start(
            "endless",
            job_work({
                let running = Arc::clone(&running);
                let applied = Arc::clone(&applied);
                move |control: &JobControl| {
                    running.store(true, Ordering::SeqCst);
                    while !control.stop_requested() {
                        thread::yield_now();
                    }
                    Ok(Some(job_apply(move |_host: &mut dyn Host| {
                        applied.store(true, Ordering::SeqCst);
                    })))
                }
            }),
            Some(job_finished({
                let report = Rc::clone(&report);
                move |ended: &JobReport, _host: &mut dyn Host| {
                    *report.borrow_mut() = ended.clone();
                }
            })),
        );
        assert!(pump_until(
            &mut runner,
            &mut host,
            || running.load(Ordering::SeqCst),
            THIRTY_SECONDS
        ));
        assert!(runner.cancel(id));
        runner.wait_for(id, &mut host);
        assert_eq!(report.borrow().outcome, JobOutcome::Cancelled);
        assert!(!applied.load(Ordering::SeqCst));
        assert!(!runner.cancel(id), "an ended job cannot be cancelled again");
    }

    #[test]
    fn a_job_that_cannot_be_interrupted_is_discarded_when_cancelled_while_it_runs() {
        // A triangulation cannot look at its stop. Its result must still not land once the person
        // has said no.
        let running = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let applied = Arc::new(AtomicBool::new(false));
        let report = Rc::new(RefCell::new(JobReport::default()));
        let mut host = RecordingHost::new();
        let mut runner = JobRunner::new();
        let _release_on_exit = ReleaseOnExit(Arc::clone(&release));
        let id = runner.start(
            "stubborn",
            job_work({
                let running = Arc::clone(&running);
                let release = Arc::clone(&release);
                let applied = Arc::clone(&applied);
                move |_control: &JobControl| {
                    running.store(true, Ordering::SeqCst);
                    while !release.load(Ordering::SeqCst) {
                        thread::yield_now();
                    }
                    Ok(Some(job_apply(move |_host: &mut dyn Host| {
                        applied.store(true, Ordering::SeqCst);
                    })))
                }
            }),
            Some(job_finished({
                let report = Rc::clone(&report);
                move |ended: &JobReport, _host: &mut dyn Host| {
                    *report.borrow_mut() = ended.clone();
                }
            })),
        );
        assert!(pump_until(
            &mut runner,
            &mut host,
            || running.load(Ordering::SeqCst),
            THIRTY_SECONDS
        ));
        runner.cancel(id);
        release.store(true, Ordering::SeqCst);
        runner.wait_for(id, &mut host);
        assert_eq!(report.borrow().outcome, JobOutcome::Cancelled);
        assert!(!applied.load(Ordering::SeqCst));
    }

    /// A job whose report is all the test reads.
    fn report_of(title: &str, work: katana_window::host::JobWork) -> JobReport {
        let report = Rc::new(RefCell::new(JobReport::default()));
        let mut host = RecordingHost::new();
        let mut runner = JobRunner::new();
        let id = runner.start(
            title,
            work,
            Some(job_finished({
                let report = Rc::clone(&report);
                move |ended: &JobReport, _host: &mut dyn Host| {
                    *report.borrow_mut() = ended.clone();
                }
            })),
        );
        runner.wait_for(id, &mut host);
        report.take()
    }

    #[test]
    fn an_exception_thrown_by_the_work_is_reported_as_a_failure_with_its_message() {
        let report = report_of(
            "throws",
            job_work(|_control: &JobControl| -> katana_core::Result<_> {
                panic!("out of triangles");
            }),
        );
        assert_eq!(report.outcome, JobOutcome::Failed);
        assert!(
            report.error.contains("out of triangles"),
            "{}",
            report.error
        );
    }

    #[test]
    fn an_exception_of_an_unknown_type_is_still_reported_as_a_failure() {
        let report = report_of(
            "throws an int",
            job_work(|_control: &JobControl| -> katana_core::Result<_> {
                std::panic::panic_any(7_i32);
            }),
        );
        assert_eq!(report.outcome, JobOutcome::Failed);
        assert!(!report.error.is_empty());
    }

    #[test]
    fn an_error_returned_by_the_work_is_reported_with_its_description() {
        let report = report_of(
            "refuses",
            job_work(|_control: &JobControl| {
                Err(Error::new(
                    ErrorCode::TriangulationFailure,
                    "fewer than three points",
                ))
            }),
        );
        assert_eq!(report.outcome, JobOutcome::Failed);
        assert!(
            report.error.contains("TriangulationFailure"),
            "{}",
            report.error
        );
        assert!(
            report.error.contains("fewer than three points"),
            "{}",
            report.error
        );
    }

    #[test]
    fn an_exception_thrown_while_applying_is_reported_as_a_failure() {
        let report = report_of(
            "bad apply",
            job_work(|_control: &JobControl| {
                Ok(Some(job_apply(|_host: &mut dyn Host| {
                    panic!("no such surface");
                })))
            }),
        );
        assert_eq!(report.outcome, JobOutcome::Failed);
        assert!(report.error.contains("no such surface"), "{}", report.error);
    }

    #[test]
    fn destroying_the_runner_stops_and_joins_its_jobs_and_calls_nothing_afterwards() {
        let running = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let applied = Arc::new(AtomicBool::new(false));
        let finished = Rc::new(Cell::new(false));
        {
            let mut host = RecordingHost::new();
            let mut runner = JobRunner::new();
            runner.start(
                "abandoned",
                job_work({
                    let running = Arc::clone(&running);
                    let stopped = Arc::clone(&stopped);
                    let applied = Arc::clone(&applied);
                    move |control: &JobControl| {
                        running.store(true, Ordering::SeqCst);
                        while !control.stop_requested() {
                            thread::yield_now();
                        }
                        stopped.store(true, Ordering::SeqCst);
                        Ok(Some(job_apply(move |_host: &mut dyn Host| {
                            applied.store(true, Ordering::SeqCst);
                        })))
                    }
                }),
                Some(job_finished({
                    let finished = Rc::clone(&finished);
                    move |_ended: &JobReport, _host: &mut dyn Host| finished.set(true)
                })),
            );
            assert!(pump_until(
                &mut runner,
                &mut host,
                || running.load(Ordering::SeqCst),
                THIRTY_SECONDS
            ));
        }
        // The drop joined, so the worker has seen its stop by now.
        assert!(stopped.load(Ordering::SeqCst));
        assert!(!finished.get());
        assert!(!applied.load(Ordering::SeqCst));
    }

    #[test]
    fn several_jobs_run_together_and_each_finishes_once() {
        let mut host = RecordingHost::new();
        let mut runner = JobRunner::new();
        let finished_count = Rc::new(Cell::new(0));
        let sum = Rc::new(Cell::new(0));
        for i in 1..=4 {
            runner.start(
                &format!("job {i}"),
                job_work(|_control: &JobControl| Ok(None)),
                Some(job_finished({
                    let finished_count = Rc::clone(&finished_count);
                    let sum = Rc::clone(&sum);
                    move |ended: &JobReport, _host: &mut dyn Host| {
                        finished_count.set(finished_count.get() + 1);
                        sum.set(
                            sum.get()
                                + if ended.outcome == JobOutcome::Finished {
                                    i
                                } else {
                                    100
                                },
                        );
                    }
                })),
            );
        }
        assert_eq!(runner.active_count(), 4);
        runner.wait_for_all(&mut host);
        assert_eq!(finished_count.get(), 4);
        assert_eq!(sum.get(), 1 + 2 + 3 + 4);
    }

    #[test]
    fn the_event_loop_keeps_running_while_a_job_computes() {
        // Frames keep coming: the work sleeps for 400 ms - the job being timed - and with the work
        // off the window's thread the frames keep coming a tick apart; a frame time anywhere near
        // the job's length would mean the window was waiting for it.
        let mut harness = Harness::new();
        let mut host = RecordingHost::new();
        let mut runner = JobRunner::new();
        runner.set_repaint(Some(harness.context().clone()));
        let id: JobId = runner.start(
            "sleeps",
            job_work(|_control: &JobControl| {
                thread::sleep(Duration::from_millis(400));
                Ok(None)
            }),
            None,
        );
        assert!(frames_until(
            &mut harness,
            &mut runner,
            &mut host,
            |runner| !runner.is_active(id),
            THIRTY_SECONDS
        ));
        assert!(
            runner.longest_event_loop_pause() < 0.2,
            "the window stalled {} ms during a 400 ms job",
            runner.longest_event_loop_pause() * 1000.0
        );
    }

    #[test]
    fn the_event_loop_pause_measure_sees_the_gui_thread_blocked() {
        // The control for the test above: block the window's thread for 250 ms while a job runs,
        // and the measure must show at least that. Without this, a measure that always read zero
        // would pass the test above. The block is timed by the steady clock the measure itself
        // reads, so the pause is at least 250 ms by construction: the frame before it read the
        // clock before `blocked_from` was read, and the frame after it reads it after
        // `blocked_until`.
        let release = Arc::new(AtomicBool::new(false));
        let mut harness = Harness::new();
        let mut host = RecordingHost::new();
        let mut runner = JobRunner::new();
        let _release_on_exit = ReleaseOnExit(Arc::clone(&release));
        let id = runner.start(
            "waits",
            job_work({
                let release = Arc::clone(&release);
                move |control: &JobControl| {
                    while !release.load(Ordering::SeqCst) && !control.stop_requested() {
                        thread::yield_now();
                    }
                    Ok(None)
                }
            }),
            None,
        );
        frame(&mut harness, &mut runner, &mut host);
        let blocked_from = Instant::now();
        let blocked_until = blocked_from + Duration::from_millis(250);
        let mut now = blocked_from;
        while now < blocked_until {
            // The window's thread, blocked.
            thread::sleep(blocked_until - now);
            now = Instant::now();
        }
        assert!(
            frames_until(
                &mut harness,
                &mut runner,
                &mut host,
                |runner| runner.longest_event_loop_pause() >= 0.25,
                Duration::from_secs(5)
            ),
            "longest pause {} ms",
            runner.longest_event_loop_pause() * 1000.0
        );
        release.store(true, Ordering::SeqCst);
        runner.wait_for(id, &mut host);
        assert!(runner.longest_event_loop_pause() >= 0.25);
    }

    #[test]
    fn a_triangulation_run_as_a_job_leaves_the_event_loop_running() {
        // The real work of Surface From Drawing, at a size that takes a while: 60k scattered
        // points. What the test holds is the RATIO - the window paused for well under half of the
        // work's time - because absolute times on a shared machine are not a property of the code.
        // The work is repeated to a floor of 400 ms, so the bound is 200 ms, as for the 400 ms job
        // above, and a triangulation run on the window's thread would still pause it for all of it.
        use katana_geometry::Point3;
        use katana_terrain::{TinBuildOptions, TinInput, build_tin};

        let mut input = TinInput::default();
        let mut state: u32 = 12345;
        let mut next = || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            f64::from(state) / 4294967296.0
        };
        for _ in 0..60_000 {
            let x = next() * 1000.0;
            let y = next() * 1000.0;
            input
                .points
                .push(Point3::new(x, y, 40.0 + 0.01 * x - 0.02 * y));
        }
        let mut harness = Harness::new();
        let mut host = RecordingHost::new();
        let mut runner = JobRunner::new();
        runner.set_repaint(Some(harness.context().clone()));
        let report = Rc::new(RefCell::new(JobReport::default()));
        let id = runner.start(
            "Triangulating",
            job_work(move |control: &JobControl| {
                let began = Instant::now();
                let surface = loop {
                    let built = build_tin(&input, &TinBuildOptions::default())?;
                    if began.elapsed() >= Duration::from_millis(400) || control.stop_requested() {
                        break built.surface;
                    }
                };
                Ok(Some(job_apply(move |_host: &mut dyn Host| {
                    let _kept = surface;
                })))
            }),
            Some(job_finished({
                let report = Rc::clone(&report);
                move |ended: &JobReport, _host: &mut dyn Host| {
                    *report.borrow_mut() = ended.clone();
                }
            })),
        );
        assert!(frames_until(
            &mut harness,
            &mut runner,
            &mut host,
            |runner| !runner.is_active(id),
            Duration::from_secs(120)
        ));
        let report = report.borrow();
        assert_eq!(report.outcome, JobOutcome::Finished, "{}", report.error);
        assert!(
            runner.longest_event_loop_pause() < report.work_seconds * 0.5,
            "work {} ms, window pause {} ms",
            report.work_seconds * 1000.0,
            runner.longest_event_loop_pause() * 1000.0
        );
    }

    #[test]
    fn the_status_widget_shows_the_running_job_and_its_cancel_button_cancels_it() {
        let running = Arc::new(AtomicBool::new(false));
        let mut harness = Harness::new();
        let mut host = RecordingHost::new();
        let mut runner = JobRunner::new();
        runner.set_repaint(Some(harness.context().clone()));
        harness
            .show(|ui| runner.status_ui(ui))
            .expect("the status bar is drawn");
        assert!(
            !harness
                .control("JobStatusLabel")
                .expect("the label is drawn, hidden")
                .visible,
            "nothing runs, so nothing is shown"
        );

        let report = Rc::new(RefCell::new(JobReport::default()));
        let id = runner.start(
            "Surface From Drawing",
            job_work({
                let running = Arc::clone(&running);
                move |control: &JobControl| {
                    control.set_stage("Triangulating");
                    running.store(true, Ordering::SeqCst);
                    while !control.stop_requested() {
                        thread::yield_now();
                    }
                    Ok(None)
                }
            }),
            Some(job_finished({
                let report = Rc::clone(&report);
                move |ended: &JobReport, _host: &mut dyn Host| {
                    *report.borrow_mut() = ended.clone();
                }
            })),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        let shows_stage = |harness: &Harness| {
            harness
                .control("JobStatusLabel")
                .is_some_and(|label| label.text.contains("Triangulating"))
        };
        while !(running.load(Ordering::SeqCst) && shows_stage(&harness)) {
            assert!(
                Instant::now() < deadline,
                "the label never showed the stage"
            );
            let mut ended = Vec::new();
            harness.frame(|ui| {
                ended = runner.frame(ui.ctx());
                runner.status_ui(ui);
            });
            for job in ended {
                runner.finish(job, &mut host);
            }
            thread::sleep(TICK);
        }
        let label = harness.control("JobStatusLabel").expect("the label");
        assert!(label.visible);
        assert!(
            label.text.starts_with("Surface From Drawing"),
            "{}",
            label.text
        );
        assert_eq!(
            harness.control("JobStatusProgress").expect("the bar").value,
            "busy",
            "no progress reported: a busy bar"
        );

        harness
            .press("JobStatusCancel")
            .run(|ui| runner.status_ui(ui))
            .expect("Cancel is pressed");
        runner.wait_for(id, &mut host);
        assert_eq!(report.borrow().outcome, JobOutcome::Cancelled);
        harness
            .show(|ui| runner.status_ui(ui))
            .expect("the status bar is drawn");
        assert!(
            !harness
                .control("JobStatusLabel")
                .expect("the label")
                .visible,
            "hidden again once nothing runs"
        );
    }
}
