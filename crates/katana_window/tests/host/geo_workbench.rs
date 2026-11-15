//! The geoprocessing verbs in the window (src/katana_window/src/geo/geo_workbench.rs;
//! geo/geo_workbench.hpp): a line prepared on the window's thread, run as a background job, its
//! reply logged and handed to whoever listens, and a cancelled job leaving nothing. The executor
//! itself is tested in katana_app's geo twins; here, only what the window adds.

use std::cell::RefCell;
use std::rc::Rc;

use katana_window::geo::geo_workbench::{GeoWorkbench, started_job, started_record};
use katana_window::host::{Host, JobId, Jobs, NO_JOB, VerbOutcome};

use crate::hosts::ThreadedHost;

/// The window's side, as the main window hands it over: its drawing, interpreter and reference data
/// (the host's), its log, whether it is headless, and what `finished` was told.
pub struct Window {
    pub host: ThreadedHost,
    pub workbench: GeoWorkbench,
    pub finished: Rc<RefCell<Option<(JobId, VerbOutcome)>>>,
}

impl Window {
    pub fn new() -> Self {
        let finished = Rc::new(RefCell::new(None));
        let mut workbench = GeoWorkbench::new();
        let told = Rc::clone(&finished);
        workbench.set_finished(Some(Box::new(
            move |id: JobId, outcome: &VerbOutcome, _host: &mut dyn Host| {
                *told.borrow_mut() = Some((id, outcome.clone()));
            },
        )));
        Self {
            host: ThreadedHost::new(),
            workbench,
            finished,
        }
    }

    pub fn set_headless(&mut self, headless: bool) {
        self.host.recording.parts.headless = headless;
    }

    pub fn run_line(&mut self, line: &str) -> bool {
        self.workbench.run_line(&mut self.host, line)
    }

    /// What `finished` was told: the job's id and outcome.
    pub fn finished(&self) -> Option<(JobId, VerbOutcome)> {
        self.finished.borrow().clone()
    }
}

impl Default for Window {
    fn default() -> Self {
        Self::new()
    }
}

mod geo_workbench {
    use super::*;

    #[test]
    fn a_line_of_another_verb_is_left_to_the_window() {
        let mut window = Window::new();
        assert!(!window.run_line("LINE 0,0 10,0"));
        assert!(!window.run_line("ONLINE PROVIDERS"));
        assert!(window.host.logged().is_empty());
    }

    #[test]
    fn what_answers_at_once_is_logged_without_a_job() {
        let mut window = Window::new();
        assert!(window.run_line("GDAL VERSION"));
        assert_eq!(window.workbench.last_job(), NO_JOB);
        let logged = window.host.logged();
        assert_eq!(logged.len(), 1);
        assert!(logged[0].starts_with("gdal version=3."), "{}", logged[0]);
    }

    #[test]
    fn a_refusal_is_logged_as_an_error() {
        let mut window = Window::new();
        assert!(window.run_line("GDAL raster hillshade --config X=Y"));
        let errors = window.host.errors();
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].starts_with("error: InvalidArgument: --config"),
            "{}",
            errors[0]
        );
    }

    #[test]
    fn a_run_is_a_job_whose_reply_is_logged_and_handed_to_whoever_listens() {
        let mut window = Window::new();
        window.host.type_line("LINE 0,0 100,0");
        let heard: Rc<RefCell<Option<VerbOutcome>>> = Rc::new(RefCell::new(None));
        let told = Rc::clone(&heard);
        let key = window.workbench.add_finished_listener(Box::new(
            move |_id: JobId, outcome: &VerbOutcome, _host: &mut dyn Host| {
                *told.borrow_mut() = Some(outcome.clone());
            },
        ));
        // Headless, the line waits for its job, as --command and a script do.
        assert!(window.run_line("GDAL vector buffer distance=1 FROM DRAWING"));
        assert_ne!(window.workbench.last_job(), NO_JOB);
        let (id, finished) = window.finished().expect("finished was told");
        assert_eq!(id, window.workbench.last_job());
        assert!(finished.ok, "{}", finished.error);
        assert!(finished.reply.contains("created=1"), "{}", finished.reply);
        let heard = heard.borrow().clone().expect("the listener heard");
        assert_eq!(heard.reply, finished.reply);
        let logged = window.host.logged();
        assert!(
            logged
                .last()
                .is_some_and(|last| last.contains("layer=gis/buffer created=1")),
            "{logged:?}"
        );
        assert_eq!(window.host.document().model().entities.size(), 2);
        window.workbench.remove_finished_listener(key);
    }

    #[test]
    fn an_interactive_run_says_it_started_and_a_cancel_applies_nothing() {
        let mut window = Window::new();
        window.set_headless(false);
        window.host.type_line("LINE 0,0 100,0");
        assert!(window.run_line("GDAL vector buffer distance=1 FROM DRAWING"));
        let id = window.workbench.last_job();
        assert_ne!(id, NO_JOB);
        let logged = window.host.logged();
        let started = format!("job id={id} title=\"GDAL vector buffer\" state=started");
        assert!(
            logged.last().is_some_and(|last| last.starts_with(&started)),
            "{logged:?}"
        );
        // Cancelled before its completion is delivered: whenever the work ended, a cancelled job
        // never applies.
        assert!(Jobs::cancel(window.host.jobs(), id));
        window.host.wait_for(id);
        let (_, finished) = window.finished().expect("finished was told");
        assert!(!finished.ok);
        assert_eq!(finished.error, "error: InvalidState: cancelled");
        assert_eq!(window.host.document().model().entities.size(), 1);
        assert!(!window.host.document().model().layers.contains("gis/buffer"));
    }

    // The one reader of the record an interactive run logs reads the one writer's line back, on
    // whichever line of a reply it stands, and a title with a quote in it, which the writer escapes
    // as any record's value.
    #[test]
    fn the_started_record_reads_back_as_its_job() {
        assert_eq!(
            started_record(7, "GDAL vector buffer"),
            "job id=7 title=\"GDAL vector buffer\" state=started"
        );
        assert_eq!(
            started_job(&started_record(7, "GDAL vector buffer")),
            Some(7)
        );
        assert_eq!(
            started_job(&started_record(12, "EXPORT \"a b\".gpkg")),
            Some(12)
        );
        assert_eq!(
            started_job(&format!(
                "scope scope=drawing matched=1\n{}",
                started_record(3, "x")
            )),
            Some(3)
        );
        // A reply that started none - a headless run's records - is no job.
        assert_eq!(
            started_job("gdal algorithm=\"vector buffer\" seconds=0.001"),
            None
        );
        assert_eq!(started_job("job id=4 title=x state=finished"), None);
        assert_eq!(started_job(""), None);
    }
}
