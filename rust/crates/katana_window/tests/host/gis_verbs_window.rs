//! IMPORT, EXPORT and COPC in the window (docs/interop.md, "IMPORT, EXPORT, INFO, REFS and COPC on
//! every front end"): lines of the executor, so the geo workbench runs them as background jobs - an
//! IMPORT's read on its own thread, a cancel that imports nothing, an EXPORT that writes nothing
//! once cancelled. What each writes is tested in katana_app's geo twins; here, the job.

use std::fs;

use katana_core::path_text::generic_path_utf8;
use katana_window::host::{Host, Jobs, NO_JOB};

use crate::geo_workbench::Window;
use crate::hosts::shared_data;
use crate::window_support::{Scratch, samples};

/// samples/gis, as the text a line names it by, with forward slashes.
fn gis_samples() -> String {
    String::from_utf8_lossy(&generic_path_utf8(&samples("gis"))).into_owned()
}

mod gis_verbs_window {
    use super::*;

    #[test]
    fn an_import_is_a_job_whose_records_are_logged() {
        let _shared = shared_data();
        let mut window = Window::new();
        let line = format!("IMPORT \"{}/parcels.geojson\" LOCAL", gis_samples());
        assert!(window.run_line(&line));
        assert_ne!(window.workbench.last_job(), NO_JOB);
        let (_, finished) = window.finished().expect("finished was told");
        assert!(finished.ok, "{}", finished.error);
        assert!(
            finished.reply.starts_with("imported file="),
            "{}",
            finished.reply
        );
        assert!(
            finished
                .reply
                .contains("placed placement=local east=-180 north=0"),
            "{}",
            finished.reply
        );
        // parcels.geojson: 8 features, the spoil heaps a MultiPolygon of two.
        assert_eq!(window.host.document().model().entities.size(), 9);
    }

    #[test]
    fn an_interactive_import_says_it_started_and_a_cancel_imports_nothing() {
        let _shared = shared_data();
        let mut window = Window::new();
        window.set_headless(false);
        let line = format!("IMPORT \"{}/parcels.geojson\"", gis_samples());
        assert!(window.run_line(&line));
        let id = window.workbench.last_job();
        assert_ne!(id, NO_JOB);
        let logged = window.host.logged();
        let started = format!("job id={id} title=\"IMPORT parcels.geojson\" state=started");
        assert!(
            logged.last().is_some_and(|last| last.starts_with(&started)),
            "{logged:?}"
        );
        // Cancelled before its completion is delivered: whenever the read ended, a cancelled job
        // never applies.
        assert!(Jobs::cancel(window.host.jobs(), id));
        window.host.wait_for(id);
        let (_, finished) = window.finished().expect("finished was told");
        assert!(!finished.ok);
        assert_eq!(finished.error, "error: InvalidState: cancelled");
        assert_eq!(window.host.document().model().entities.size(), 0);
        assert!(!window.host.document().model().layers.contains("parcels"));
    }

    #[test]
    fn a_cancelled_export_leaves_no_file() {
        let mut window = Window::new();
        window.set_headless(false);
        window.host.type_line("RECT 0,0 10,5");
        let folder = Scratch::new("gis verbs window export");
        let file = folder.path("cancelled.geojson");
        let line = format!(
            "EXPORT \"{}\"",
            String::from_utf8_lossy(&generic_path_utf8(&file))
        );
        assert!(window.run_line(&line));
        let id = window.workbench.last_job();
        assert_ne!(id, NO_JOB);
        assert!(Jobs::cancel(window.host.jobs(), id));
        window.host.wait_for(id);
        let (_, finished) = window.finished().expect("finished was told");
        assert!(!finished.ok);
        // Nothing in the folder at all: not the file, and not the folder it was written in before
        // its apply would have moved it into place.
        assert!(
            fs::read_dir(&folder.root)
                .expect("the folder is there")
                .next()
                .is_none()
        );
    }

    #[test]
    fn refs_answers_at_once_without_a_job() {
        let mut window = Window::new();
        assert!(window.run_line("REFS"));
        assert_eq!(window.workbench.last_job(), NO_JOB);
        assert_eq!(
            window.host.logged(),
            ["references rasters=0 clouds=0 missing=0"]
        );
    }

    #[test]
    fn info_of_an_entity_is_left_to_the_window() {
        let mut window = Window::new();
        assert!(!window.run_line("INFO #1"));
        assert!(window.host.logged().is_empty());
        assert!(window.host.errors().is_empty());
    }
}
