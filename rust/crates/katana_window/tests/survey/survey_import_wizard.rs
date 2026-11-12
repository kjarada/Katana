//! The twins of tests/qt_widgets/survey/test_survey_import_wizard.cpp: the import wizard's
//! instrument path, driven by its object names as a person clicks it - the Content step shows
//! what each format's reader read (checked against the reader called directly, and against what
//! the fixtures hold, counted by hand), a format with no published layout is refused on the
//! Format step with what to export instead, an import with an adjustment is one undo step that
//! leaves a survey job, and a large file is read off the interface thread, which keeps answering.
//!
//! And the FINISH: with survey codes loaded the import codes its points and strings them inside
//! its one command. The codes are the hand-written fixture
//! tests/data/field_codes/test_field_codes.customisation.json, read by hand:
//!
//! ```text
//! KB*   a LINE on FIELD KERB, linestyle "FIELD Kerb", colour "field kerb"
//! EB*   a LINE on FIELD EDGE, a plain line, colour "blue"
//! CTRL  a POINT on FIELD CONTROL, a plain line, colour "red"
//! ```
//!
//! and no rule for anything else (it has no "*" rule). A style is named after its linestyle, so a
//! kerb's style is "FIELD Kerb".

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use katana_app::survey_verbs::run_survey_line;
use katana_cad::survey_job::{SurveyJob, read_survey_job_options};
use katana_cad::survey_points::drawing_survey_points;
use katana_cad::{Document, SurveyImportOptions};
use katana_core::path_text::read_file_bytes;
use katana_core::text::format_exact_real;
use katana_entity::tables::{Layer, Style};
use katana_entity::{CustomisationAutomation, Entity, Geometry, PropertyValue};
use katana_geometry::{Point2, Polyline2};
use katana_survey::reduction_settings::serialise_reduction_settings;
use katana_survey::{
    AdjustmentMethod, ControlOrigin, ControlPoint, ControlSelection, FaceHandling,
    ReductionSettings, SurveyProject,
};
use katana_surveyio::{
    MAX_SURVEY_FILE_BYTES, ReadOptions, ReadResult, detect_format, format_registry, probe_of,
    read_survey, siblings_in_folder,
};
use katana_window::RecordingHost;
use katana_window::drive::Harness;
use katana_window::survey::survey_import_wizard::{SurveyImportWizard, survey_file_filter};

use crate::fixture_customisation::{
    customisation_fixture_directory, customisation_from_text, install_customisations,
};
use crate::window_support::Scratch;
use crate::{cells, logged, path_text, survey_ui_data, surveyio_data};

const DIALOG: &str = "surveyImportDialog";

struct Session {
    harness: Harness,
    host: RecordingHost,
    wizard: SurveyImportWizard,
}

impl Session {
    fn new() -> Self {
        let mut wizard = SurveyImportWizard::new();
        wizard.show();
        let mut session = Self {
            harness: Harness::new(),
            host: RecordingHost::new(),
            wizard,
        };
        session.harness.target(DIALOG);
        session.run();
        session
    }

    fn run(&mut self) {
        let wizard = &mut self.wizard;
        let host = &mut self.host;
        if let Err(error) = self.harness.run(|ui| wizard.ui(ui, &mut *host)) {
            panic!("{error}");
        }
    }

    /// One frame, timed: what the interface thread spent on it.
    fn frame(&mut self) -> Duration {
        let wizard = &mut self.wizard;
        let host = &mut self.host;
        let started = Instant::now();
        self.harness.frame(|ui| wizard.ui(ui, &mut *host));
        started.elapsed()
    }

    fn fill(&mut self, name: &str, text: &str) {
        self.harness.fill(name, text);
        self.run();
    }

    fn click(&mut self, name: &str) {
        self.harness.press(name);
        self.run();
    }

    fn report(&self, name: &str) -> String {
        self.harness
            .report(name)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// The report pane's text after an Import, which closes the wizard: no frame draws the pane
    /// then, so it is read from the wizard, where the C++ read the hidden dialog's widget.
    fn report_after_import(&self) -> String {
        String::from(self.wizard.report_text())
    }

    fn checked(&self, name: &str) -> bool {
        self.harness
            .control(name)
            .unwrap_or_else(|| panic!("no {name}"))
            .checked
            .unwrap_or(false)
    }

    fn enabled(&self, name: &str) -> bool {
        self.harness
            .control(name)
            .unwrap_or_else(|| panic!("no {name}"))
            .enabled
    }

    /// File -> Format -> (Content, read at once for a small file).
    fn open_and_read(&mut self, path: &Path) {
        self.fill("file", &path_text(path));
        self.click("next");
        self.click("next");
    }

    fn step(&self) -> String {
        self.report("step")
    }

    fn logged(&self, pattern: &str) -> bool {
        logged(&self.host, pattern)
    }

    fn document(&self) -> &Document {
        self.host.document()
    }

    /// The Content step's rows, by what they count.
    fn content_rows(&self) -> BTreeMap<String, String> {
        self.wizard
            .content()
            .iter()
            .map(|row| (row.what.clone(), row.detail.clone()))
            .collect()
    }
}

/// What the reader returns for the file, called directly - the Content step must show exactly
/// this.
fn read_directly(path: &Path) -> ReadResult {
    let bytes = std::fs::read(path).expect("the fixture");
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let detection = detect_format(&probe_of(&bytes, &name, false));
    let format = detection
        .format()
        .unwrap_or_else(|error| panic!("{}", error.describe()));
    let folder = path.parent().map(PathBuf::from).unwrap_or_default();
    let options = ReadOptions {
        siblings: Some(siblings_in_folder(folder, MAX_SURVEY_FILE_BYTES)),
        ..ReadOptions::default()
    };
    read_survey(format_registry(), &format.id, &bytes, &name, &options)
        .unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn observations_of(project: &SurveyProject) -> usize {
    project.observations.len()
        + project
            .stations
            .iter()
            .map(|station| station.observations.len())
            .sum::<usize>()
}

fn expect_content_as_read(rows: &BTreeMap<String, String>, read: &ReadResult) {
    let p = &read.project;
    let row = |what: &str| rows.get(what).cloned().unwrap_or_default();
    assert_eq!(row("Setups"), format!("{}", p.stations.len()));
    assert_eq!(row("Observations"), format!("{}", observations_of(p)));
    assert_eq!(
        row("Points with coordinates"),
        format!("{}", p.points.len())
    );
    assert_eq!(
        row("Points without coordinates"),
        format!("{}", p.unpositioned_points.len())
    );
    assert_eq!(row("Coded features"), format!("{}", p.features.len()));
    assert_eq!(
        row("Control the file declares"),
        format!("{}", p.control_points.len())
    );
    assert_eq!(row("GNSS sessions"), format!("{}", p.gnss_sessions.len()));
    assert_eq!(
        row("Files read beside it"),
        format!("{}", read.siblings_read.len())
    );
    assert_eq!(
        row("Not in the file"),
        format!("{}", read.not_carried.len())
    );
    assert_eq!(row("Warnings"), format!("{}", read.warnings.len()));
}

/// A GSI-16 job of `setups` setups of `shots` shots each, written the way the reader's own
/// throughput test writes one: a station block, then a line per shot with Hz, V and a slope
/// distance.
fn synthetic_gsi16(setups: usize, shots: usize) -> String {
    let mut out = String::with_capacity(setups * (shots + 1) * 100);
    let mut block = 0_usize;
    for s in 0..setups {
        block += 1;
        out.push_str(&format!(
            "*11{:04}+{:0>16} 84..16+{:016} 85..16+{:016} 86..16+{:016} 88..16+{:016} \n",
            block % 10000,
            format!("S{s}"),
            10_000_000 + s * 1000,
            20_000_000 + s * 1000,
            500_000,
            15_000
        ));
        for i in 0..shots {
            block += 1;
            out.push_str(&format!(
                "*11{:04}+{:0>16} 21.103+{:016} 22.103+{:016} 31..16+{:016} 87..16+{:016} \n",
                block % 10000,
                format!("P{s}N{i}"),
                (i * 1_234_567) % 36_000_000,
                9_000_000 + (i % 100) * 1000,
                100_000 + (i % 5000) * 37,
                15_000
            ));
        }
    }
    out
}

/// A job with what the clock decides taken out: when it was imported and when its report was
/// made, which the report states.
fn timeless(mut job: SurveyJob) -> SurveyJob {
    for when in [job.report_created_utc.clone(), job.imported_utc.clone()] {
        if when.is_empty() {
            continue;
        }
        job.report_text = job.report_text.replace(&when, "<when>");
        job.report_html = job.report_html.replace(&when, "<when>");
    }
    job.report_created_utc.clear();
    job.imported_utc.clear();
    job
}

/// The drawing's survey points, a line each, every number exactly.
fn points_text(document: &Document) -> String {
    drawing_survey_points(document, &SurveyImportOptions::default())
        .iter()
        .map(|point| {
            format!(
                "{} {} {} {} {} {}\n",
                point.entity,
                point.id,
                point.layer,
                format_exact_real(point.easting),
                format_exact_real(point.northing),
                point
                    .elevation
                    .map_or_else(|| String::from("-"), format_exact_real)
            )
        })
        .collect()
}

/// CP1 drawn by a FORWARD line, where the traverse's file gives it no coordinates.
fn place(document: &mut Document, lines: &[&str]) {
    let mut interpreter = katana_cad::CommandInterpreter::new();
    for line in lines {
        interpreter
            .run(document, line)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
    }
}

// ---- the finish: the survey codes and the linework ---------------------------------------

/// The fixture of field codes, loaded into the session as a person's customisation is. It has a
/// folder of its own, beside the three of tests/data/customisation.
fn install_field_codes(document: &mut Document) {
    let file = customisation_fixture_directory()
        .parent()
        .expect("tests/data")
        .join("field_codes")
        .join("test_field_codes.customisation.json");
    let bytes = read_file_bytes(&file).unwrap_or_else(|error| panic!("{}", error.describe()));
    let text = String::from_utf8(bytes).expect("the fixture is UTF-8");
    install_customisations(document, &[customisation_from_text(&text)]);
    assert_eq!(document.survey_map().size(), 3);
}

/// Everything of a drawing a survey import may change.
#[derive(Debug, PartialEq)]
struct Drawing {
    entities: Vec<Entity>,
    layers: Vec<Layer>,
    styles: Vec<Style>,
}

fn drawing_of(document: &Document) -> Drawing {
    let mut entities = Vec::new();
    document
        .model()
        .entities
        .for_each(|entity| entities.push(entity.clone()));
    Drawing {
        entities,
        layers: document.model().layers.all(),
        styles: document.model().styles.all(),
    }
}

fn text_of(entity: &Entity, key: &str) -> String {
    match entity.properties.get(key) {
        None => String::from("<absent>"),
        Some(PropertyValue::Text(text)) => text.clone(),
        Some(_) => String::from("<not text>"),
    }
}

fn point_numbered(document: &Document, number: &str) -> Option<Entity> {
    let mut found = None;
    document.model().entities.for_each(|entity| {
        if matches!(entity.geometry, Geometry::Point(_)) && text_of(entity, "point") == number {
            found = Some(entity.clone());
        }
    });
    found
}

fn lines_drawn(document: &Document) -> Vec<Entity> {
    let mut lines = Vec::new();
    document.model().entities.for_each(|entity| {
        if matches!(entity.geometry, Geometry::Polyline(_)) {
            lines.push(entity.clone());
        }
    });
    lines
}

fn polyline_of(entity: &Entity) -> &Polyline2 {
    match &entity.geometry {
        Geometry::Polyline(polyline) => polyline,
        _ => panic!("a polyline"),
    }
}

/// The RTK job tests/surveyio/data/fld/gnss.fld through the wizard, from its File step to the
/// Options step: File, Format (identified), Content, System, Reduction and adjustment.
fn drive_to_options(session: &mut Session, file: &Path) {
    session.open_and_read(file);
    session.click("next"); // System
    session.click("next"); // Reduction and adjustment
    session.click("next"); // Options
    assert!(session.step().contains("Options"), "{}", session.step());
}

/// A point list read as delimited points with its header naming the columns, and driven to the
/// Options step in metres.
fn drive_point_list_to_options(session: &mut Session, path: &Path) {
    session.fill("file", &path_text(path));
    session.click("next"); // Format
    session.fill("format", "Delimited text points (CSV, TXT)");
    session.click("next"); // Columns and delimiter
    session.click("next"); // Units and coordinate system
    session.fill("unit", "metres");
    session.click("next"); // Options
    assert!(session.step().contains("Options"), "{}", session.step());
}

mod survey_import_wizard {
    use super::*;

    #[test]
    fn the_format_step_lists_every_registered_format_with_its_record() {
        let mut session = Session::new();
        session.fill("file", &path_text(&surveyio_data("leica/tps_gsi8.gsi")));
        session.click("next");
        let candidates = session
            .harness
            .control("candidates")
            .expect("the candidates");
        let formats = format_registry().formats();
        assert_eq!(candidates.items.len(), formats.len());
        // The detected one first, with its confidence; every row with its record.
        let gsi = format_registry()
            .find("leica-gsi")
            .expect("the GSI format")
            .human_name;
        assert_eq!(cells(&candidates.items[0])[0], gsi);
        for row in &candidates.items {
            let record = cells(row).last().cloned().unwrap_or_default();
            assert!(record.contains(", parser: "), "{record}");
        }
        assert_eq!(
            session.harness.control("format").expect("the format").value,
            gsi
        );
    }

    #[test]
    fn the_content_step_shows_what_each_formats_reader_read() {
        // Setups counted by hand in each fixture: tps_gsi8.gsi has two blocks with word 84 (a
        // station), one_setup.jxl one StationRecord, setup_metres.rw5 one OC record, setup.gt7
        // one STN line, setup.fld one 03 record, gnss.fld none (RTK coordinates only),
        // resection.fld a 128 and a 03, rtk_setup.fld one 03 on an RTK mark, traverse.sdr two
        // live 02 records (a third is deleted); a RINEX observation file has no setups and one
        // GNSS session, and its navigation file of the same name (.24n) is read beside it.
        let files = [
            ("leica/tps_gsi8.gsi", "2"),
            ("trimble_jxl/one_setup.jxl", "1"),
            ("rw5/setup_metres.rw5", "1"),
            ("gts/setup.gt7", "1"),
            ("fld/setup.fld", "1"),
            ("fld/gnss.fld", "0"),
            ("fld/resection.fld", "2"),
            ("fld/rtk_setup.fld", "1"),
            ("sdr/traverse.sdr", "2"),
            ("rinex/test2560.24o", "0"),
        ];
        for (file, setups) in files {
            let mut session = Session::new();
            session.open_and_read(&surveyio_data(file));
            assert!(
                session.step().contains("What the file holds"),
                "{file}: {}",
                session.step()
            );
            let rows = session.content_rows();
            assert!(!rows.is_empty(), "{file}");
            assert_eq!(
                rows.get("Setups").map(String::as_str),
                Some(setups),
                "{file}"
            );
            expect_content_as_read(&rows, &read_directly(&surveyio_data(file)));
        }
    }

    #[test]
    fn the_browse_filter_offers_every_registered_extension_and_the_unregistered_ones() {
        // The filter was a hand-written list that had fallen behind the registry. It is now
        // built from the registry, so every extension a format names is in it, and the patterns
        // no format names stay.
        let filter = survey_file_filter();
        let survey_files = filter.split(";;").next().unwrap_or_default();
        assert!(
            survey_files.starts_with("Survey files (") && survey_files.ends_with(')'),
            "{filter}"
        );
        let offered: Vec<&str> = survey_files["Survey files (".len()..survey_files.len() - 1]
            .split(' ')
            .collect();
        for descriptor in format_registry().formats() {
            for extension in &descriptor.extensions {
                assert!(
                    offered.contains(&format!("*.{extension}").as_str()),
                    "{} .{extension}",
                    descriptor.id
                );
            }
        }
        for pattern in [
            "*.sdr", "*.fld", "*.gts", "*.gts7", "*.dat", "*.pnt", "*.xyz", "*.gt6", "*.x01",
            "*.??o", "*.??d",
        ] {
            assert!(offered.contains(&pattern), "{pattern}");
        }
        let mut unique = offered.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), offered.len());
        assert!(filter.ends_with(";;All files (*)"));
    }

    #[test]
    fn the_content_step_names_the_files_found_beside_the_chosen_one() {
        let mut session = Session::new();
        session.open_and_read(&surveyio_data("rinex/test2560.24o"));
        let found: Vec<_> = session
            .wizard
            .content()
            .iter()
            .filter(|row| row.what == "Files read beside it")
            .collect();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].children.len(), 1);
        assert_eq!(found[0].children[0].0, "test2560.24n");
        assert_eq!(
            session
                .content_rows()
                .get("GNSS sessions")
                .map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn a_format_with_no_published_layout_is_refused_with_what_to_export_instead() {
        let files = [
            ("leica/dbx/JOB1.X01", "GSI-16 or LandXML"),
            ("trimble_dc/synthetic_job.dc", "JobXML"),
        ];
        for (file, instead) in files {
            let mut session = Session::new();
            session.open_and_read(&surveyio_data(file));
            // Still on the Format step, saying what to export.
            assert!(
                session.step().contains("The file's format"),
                "{file}: {}",
                session.step()
            );
            let message = session.report("message");
            assert!(message.contains(instead), "{file}: {message}");
            assert!(session.document().survey_jobs().is_empty());
        }
    }

    #[test]
    fn an_import_with_a_network_adjustment_is_one_undo_step_that_leaves_a_survey_job() {
        let mut session = Session::new();
        session.open_and_read(&survey_ui_data("network_gsi8.gsi"));
        session.click("next"); // System
        session.click("next"); // Reduction and adjustment
        session.fill("method", "network least squares");
        session.fill("controlPick", "A");
        session.click("addControl");
        session.fill("controlPick", "B");
        session.click("addControl");
        session.click("previewReduction");
        assert!(session.harness.control("previewReportSections").is_some());
        let preview = session.report("message");
        assert!(
            preview.contains("network least squares (horizontal): variance factor"),
            "{preview}"
        );
        session.click("next"); // Options
        session.click("next"); // Report
        assert!(session.step().contains("Report"), "{}", session.step());
        session.click("import");

        let jobs = session.document().survey_jobs();
        assert_eq!(jobs.len(), 1);
        let job = jobs[0].clone();
        assert_eq!(job.format_id, "leica-gsi");
        assert_eq!(job.source_file_name, "network_gsi8.gsi");
        assert_eq!(job.settings.method, AdjustmentMethod::Network);
        assert_eq!(job.settings.control.len(), 2);
        // A and B from the file, P and Q computed: four points drawn.
        assert_eq!(job.placed_points.len(), 4);
        assert!(!job.report_html.is_empty());
        assert!(session.logged(&format!(
            "Imported survey job {} from network_gsi8.gsi",
            job.id
        )));
        assert!(session.logged("network least squares (horizontal): variance factor"));

        session.host.document_mut().undo().expect("undone");
        assert!(session.document().survey_jobs().is_empty());
        session.host.document_mut().redo().expect("redone");
        assert_eq!(session.document().survey_jobs().len(), 1);
    }

    /// The opcode field file with every step left at its defaults: the reduction's radiation
    /// from the entered CP1, oriented on CP2, draws CP1, CP2 and the shots 101 and 102 - four
    /// points - and keeps the job, as one undo step (SURVEY IMPORT does the same from a line).
    #[test]
    fn an_opcode_field_file_is_imported_as_a_job_with_its_defaults() {
        let mut session = Session::new();
        session.open_and_read(&surveyio_data("fld/setup.fld"));
        session.click("next"); // System
        session.click("next"); // Reduction and adjustment
        session.click("next"); // Options
        session.click("next"); // Report
        assert!(session.step().contains("Report"), "{}", session.step());
        session.click("import");

        let jobs = session.document().survey_jobs();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].format_id, "opcode-field-file");
        assert_eq!(jobs[0].source_file_name, "setup.fld");
        assert_eq!(jobs[0].placed_points.len(), 4);
        session.host.document_mut().undo().expect("undone");
        assert!(session.document().survey_jobs().is_empty());
    }

    #[test]
    fn a_delimited_coordinate_file_still_takes_its_six_steps() {
        let mut session = Session::new();
        session.fill(
            "file",
            &path_text(&surveyio_data("survey_points_pnezd.csv")),
        );
        session.click("next");
        session.fill("format", "Delimited text points (CSV, TXT)");
        session.click("next");
        assert!(
            session
                .step()
                .contains("Step 3 of 6: Columns and delimiter"),
            "{}",
            session.step()
        );
    }

    #[test]
    fn a_large_file_is_read_off_the_gui_thread_which_keeps_answering_and_can_be_cancelled() {
        // 8 MB by default so the test stays quick in a Debug build; set
        // KATANA_SURVEY_UI_MEGABYTES=50 to measure the 50 MB case.
        let megabytes: usize = std::env::var("KATANA_SURVEY_UI_MEGABYTES")
            .ok()
            .and_then(|asked| asked.trim().parse().ok())
            .unwrap_or(8);
        // About 100 bytes a line: 1 MB is some 10 000 lines.
        let setups = megabytes * 50;
        let bytes = synthetic_gsi16(setups, 199);
        let folder = Scratch::new(
            "a_large_file_is_read_off_the_gui_thread_which_keeps_answering_and_can_be_cancelled",
        );
        let path = folder.file("big.gsi", &bytes);

        for cancel in [true, false] {
            let mut session = Session::new();
            session.fill("file", &path_text(&path));
            session.click("next");
            // The interface thread's longest frame from Next on the Format step until the
            // Content step shows.
            let clock = Instant::now();
            session.click("next");
            assert!(
                session.wizard.task_busy(),
                "a {megabytes} MB file is read in the background"
            );
            assert!(!session.enabled("next"));
            if cancel {
                session.click("cancelTask");
                assert!(!session.wizard.task_busy());
                assert!(
                    session.step().contains("The file's format"),
                    "{}",
                    session.step()
                );
                assert!(session.enabled("next"));
                continue;
            }
            let mut longest = Duration::ZERO;
            while session.wizard.task_busy() && clock.elapsed() < Duration::from_secs(600) {
                longest = longest.max(session.frame());
                std::thread::sleep(Duration::from_millis(5));
            }
            let total = clock.elapsed();
            assert!(!session.wizard.task_busy());
            session.run();
            assert!(
                session.step().contains("What the file holds"),
                "{}",
                session.step()
            );
            assert_eq!(
                session.content_rows().get("Setups").cloned(),
                Some(format!("{setups}"))
            );
            eprintln!(
                "[ measured ] {} MB read in {:.2} s; the interface thread's longest frame {} ms",
                bytes.len() >> 20,
                total.as_secs_f64(),
                longest.as_millis()
            );
            // A tenth of a second is where a person notices a stall; the read itself takes
            // seconds, so this is the thread doing its job.
            assert!(
                longest < Duration::from_millis(250),
                "the interface thread waited {} ms",
                longest.as_millis()
            );
        }
    }

    /// The wizard's Import and the SURVEY IMPORT line, given the same settings, make the same
    /// import: the same job - settings, points, report but for its time - and the same points on
    /// the drawing, bit for bit. The wizard runs its command itself rather than handing the line
    /// to the window's executor (docs/survey.md, "Not done"), so this is what holds the two
    /// together: the file with no coordinates, CP1 held where the drawing has it, and two
    /// settings changed from the defaults, the faces and the curvature and refraction.
    #[test]
    fn its_import_is_the_survey_import_lines_for_the_same_settings() {
        let file = surveyio_data("sdr/traverse_without_coordinates.sdr");
        let place_cp1 = ["FORWARD 500000,4999999,100 0 1 0 CP1"];

        let mut session = Session::new();
        place(session.host.document_mut(), &place_cp1);
        session.open_and_read(&file);
        session.click("next"); // System
        session.click("next"); // Reduction and adjustment
        session.fill("faces", "face left only");
        session.fill("curvatureRefraction", "off");
        session.fill("controlFrom", "the drawing");
        session.fill("controlPick", "CP1");
        session.fill("controlHorizontal", "fixed");
        session.fill("controlVertical", "fixed");
        session.click("addControl");
        session.click("next"); // Options
        session.click("next"); // Report
        assert!(session.step().contains("Report"), "{}", session.step());
        session.click("import");
        assert_eq!(session.document().survey_jobs().len(), 1);

        let mut by_line = Document::new(None);
        place(&mut by_line, &place_cp1);
        run_survey_line(
            &mut by_line,
            &format!(
                "SURVEY IMPORT \"{}\" SET faces=face-left-only curvature_refraction=false \
                 control=CP1;drawing;fixed;0;fixed;0;fixed;0",
                path_text(&file)
            ),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(by_line.survey_jobs().len(), 1);

        let wizard = timeless(session.document().survey_jobs()[0].clone());
        let line = timeless(by_line.survey_jobs()[0].clone());
        assert_eq!(
            serialise_reduction_settings(&wizard.settings),
            serialise_reduction_settings(&line.settings)
        );
        // The settings are the ones asked for, not merely equal to each other.
        let asked = ReductionSettings {
            faces: FaceHandling::FaceLeftOnly,
            curvature_and_refraction: false,
            control: vec![ControlSelection {
                point: ControlPoint::fixed3d("CP1"),
                origin: ControlOrigin::Drawing,
            }],
            ..ReductionSettings::default()
        };
        assert!(
            wizard.settings == asked,
            "{}",
            serialise_reduction_settings(&wizard.settings)
        );
        assert_eq!(wizard.placed_points.len(), 3); // CP2, T1, T2; CP1 is the drawing's
        assert_eq!(wizard.report_text, line.report_text);
        assert!(wizard == line);
        assert_eq!(points_text(session.document()), points_text(&by_line));
        assert_eq!(
            session.document().model().entities.size(),
            by_line.model().entities.size()
        );
    }

    /// The wizard reduces as the line does (reduce_for_drawing): with CP1 drawn at two places
    /// 100 km apart, holding it would put the job on whichever was drawn first, so its preview
    /// refuses, naming both - and nothing is imported.
    #[test]
    fn its_preview_refuses_to_hold_a_point_the_drawing_has_at_two_places() {
        let mut session = Session::new();
        place(
            session.host.document_mut(),
            &[
                "FORWARD 500000,4999999,100 0 1 0 CP1",
                "FORWARD 600000,4999999,100 0 1 0 CP1",
            ],
        );
        session.open_and_read(&surveyio_data("sdr/traverse_without_coordinates.sdr"));
        session.click("next"); // System
        session.click("next"); // Reduction and adjustment
        session.fill("controlFrom", "the drawing");
        session.fill("controlPick", "CP1");
        session.fill("controlHorizontal", "fixed");
        session.fill("controlVertical", "fixed");
        session.click("addControl");
        session.click("previewReduction");
        let message = session.report("message");
        assert!(
            message.contains(
                "Control point CP1 is on the drawing 2 times, at E 500000.0000 N 5000000.0000 Z \
                 100.0000 and E 600000.0000 N 5000000.0000 Z 100.0000, so which one to hold is \
                 not known"
            ),
            "{message}"
        );
        assert!(session.document().survey_jobs().is_empty());
    }

    /// The equivalence above, WITH survey codes: the fixture of field codes loaded into both
    /// drawings, and the RTK job gnss.fld, whose positions the file states:
    ///
    /// ```text
    /// CM1   entered, no code            E 500000  N 6200000
    /// R001  KB, string 1                E 500010  N 6200020
    /// R002  KB, string 1                E 500020  N 6200020
    /// R003  KB, string 1                E 500020  N 6200030
    /// 20    closes the string KB 1
    /// R004  KB, string 1, begun again   E 500040  N 6200040  (alone: no line)
    /// ```
    ///
    /// Both boxes start ticked - the customisation's two switches are on until something turns
    /// one off - so the wizard asks what the line asks when it says nothing: the five points, the
    /// four KB ones on FIELD KERB in the style FIELD Kerb, and the file's one closed string
    /// through R001, R002 and R003. The same job, the same entities, layers and styles.
    #[test]
    fn with_survey_codes_loaded_its_import_is_still_the_survey_import_lines() {
        let file = surveyio_data("fld/gnss.fld");

        let mut session = Session::new();
        install_field_codes(session.host.document_mut());
        drive_to_options(&mut session, &file);
        assert!(session.checked("applyCodes"));
        assert!(session.checked("drawLinework"));
        // A layer at the top of the tree, as the line below names it.
        session.fill("layer", "fieldwork");
        session.click("next"); // Report
        assert!(session.step().contains("Report"), "{}", session.step());
        session.click("import");
        assert_eq!(session.document().survey_jobs().len(), 1);

        let mut by_line = Document::new(None);
        install_field_codes(&mut by_line);
        run_survey_line(
            &mut by_line,
            &format!("SURVEY IMPORT \"{}\" LAYER fieldwork", path_text(&file)),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(by_line.survey_jobs().len(), 1);

        // The import worked by hand above, and not merely two equal drawings.
        let drawn = session.document();
        assert_eq!(
            drawn.model().entities.size(),
            6,
            "five points and the one line"
        );
        for number in ["R001", "R002", "R003", "R004"] {
            let point = point_numbered(drawn, number).expect(number);
            assert_eq!(point.layer, "FIELD KERB", "{number}");
            assert_eq!(point.style, "FIELD Kerb", "{number}");
        }
        let uncoded = point_numbered(drawn, "CM1").expect("CM1");
        assert_eq!(uncoded.layer, "fieldwork");
        assert_eq!(uncoded.style, "");
        let lines = lines_drawn(drawn);
        assert_eq!(lines.len(), 1);
        let kerb = polyline_of(&lines[0]);
        assert!(kerb.closed, "the file closed the string");
        assert_eq!(
            kerb.vertices,
            [
                Point2::new(500_010.0, 6_200_020.0),
                Point2::new(500_020.0, 6_200_020.0),
                Point2::new(500_020.0, 6_200_030.0),
            ]
        );
        assert_eq!(lines[0].layer, "FIELD KERB");
        assert_eq!(lines[0].style, "FIELD Kerb");

        // ... and the line's, to the last property of the last entity.
        let wizard = timeless(drawn.survey_jobs()[0].clone());
        let line = timeless(by_line.survey_jobs()[0].clone());
        assert_eq!(wizard.import_options, line.import_options);
        assert_eq!(wizard.report_text, line.report_text);
        assert!(wizard == line);
        assert_eq!(drawing_of(drawn), drawing_of(&by_line));
        // The job remembers what it was asked, for its next re-adjustment.
        let stored = read_survey_job_options(&wizard.import_options, &wizard.id)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(stored.apply_codes);
        assert!(stored.draw_linework);
    }

    /// The whole of it is ONE undo step, and the selection is left alone.
    #[test]
    fn a_field_files_import_with_its_codes_and_lines_is_one_undo_step() {
        let mut session = Session::new();
        install_field_codes(session.host.document_mut());
        let before = drawing_of(session.document());
        let steps = session.document().history().undo_count();

        drive_to_options(&mut session, &surveyio_data("fld/gnss.fld"));
        session.fill("layer", "fieldwork");
        session.click("next"); // Report
        // Before the import the report says what is asked of the survey codes.
        let asked = session.report("report");
        assert!(
            asked.contains("Survey codes: applied to the imported points."),
            "{asked}"
        );
        assert!(
            asked.contains("Linework: the imported points are joined into lines"),
            "{asked}"
        );
        session.click("import");

        let document = session.document();
        assert_eq!(document.history().undo_count(), steps + 1);
        assert_eq!(document.model().entities.size(), 6);
        assert!(
            document.selection().empty(),
            "nothing is selected to code it"
        );
        // What the finish did, in cad's describe sentences, by hand: five points drawn, the four
        // KB ones carry a code and all four have a rule (KB*), one layer (FIELD KERB) and one
        // style (FIELD Kerb) made; one line drawn; the second KB 1, of one point, in no line;
        // and CM1, with no code.
        let did = session.report_after_import();
        for sentence in [
            "Survey codes were applied to the 5 point(s) drawn: 4 carry a code and 4 of those \
             have a rule; 1 layer(s) and 1 style(s) were created.",
            "Linework: 1 line(s) drawn.",
            "1 string(s) of the file are in no line",
            "1 point(s) are in no line (no code: 1).",
        ] {
            assert!(did.contains(sentence), "{sentence}\n{did}");
            assert!(session.logged(sentence), "{sentence}");
        }

        session.host.document_mut().undo().expect("undone");
        assert_eq!(drawing_of(session.document()), before);
        assert!(session.document().survey_jobs().is_empty());
        assert_eq!(session.document().history().undo_count(), steps);
    }

    /// A point list is coded and strung by its codes in its one command too. Five points:
    ///
    /// ```text
    /// 1, 2, 3  KB1   (500010,6200020) (500020,6200020) (500020,6200030)
    /// 4        CTRL  (500000,6200000)
    /// 5        ZZ    (500050,6200050)
    /// ```
    ///
    /// By the fixture: KB1 is answered by KB* - FIELD KERB, the style FIELD Kerb, a line, so 1,
    /// 2 and 3 in point order are one open line; CTRL is a point on FIELD CONTROL and in no line
    /// because it is a point code; ZZ has no rule, so point 5 stays on the import's layer,
    /// unstyled, in no line, and is named. A layer per field code is ticked as well, and is not
    /// used while the codes are applied: no layer is made beneath the import's.
    #[test]
    fn a_point_lists_import_with_its_codes_and_lines_is_one_undo_step() {
        let folder = Scratch::new("a_point_lists_import_with_its_codes_and_lines_is_one_undo_step");
        let path = folder.file(
            "kerb.csv",
            "Point,Northing,Easting,Elevation,Code\n\
             1,6200020.000,500010.000,12.500,KB1\n\
             2,6200020.000,500020.000,12.600,KB1\n\
             3,6200030.000,500020.000,12.700,KB1\n\
             4,6200000.000,500000.000,10.000,CTRL\n\
             5,6200050.000,500050.000,11.000,ZZ\n",
        );

        let mut session = Session::new();
        install_field_codes(session.host.document_mut());
        let before = drawing_of(session.document());
        let steps = session.document().history().undo_count();

        drive_point_list_to_options(&mut session, &path);
        session.fill("layer", "fieldwork");
        assert!(session.checked("applyCodes"));
        session.fill("applyCodes", "off");
        assert!(session.enabled("layerPerCode"));
        session.fill("layerPerCode", "on");
        session.fill("applyCodes", "on");
        assert!(!session.enabled("layerPerCode"));
        assert!(
            session.checked("layerPerCode"),
            "the tick is kept for when the codes are off again"
        );
        session.click("next"); // Report
        session.click("import");

        let document = session.document();
        assert_eq!(document.history().undo_count(), steps + 1);
        assert!(document.selection().empty());
        assert_eq!(
            document.model().entities.size(),
            6,
            "five points and the one line"
        );
        for number in ["1", "2", "3"] {
            let point = point_numbered(document, number).expect(number);
            assert_eq!(point.layer, "FIELD KERB", "{number}");
            assert_eq!(point.style, "FIELD Kerb", "{number}");
        }
        let mark = point_numbered(document, "4").expect("4");
        assert_eq!(mark.layer, "FIELD CONTROL");
        let stray = point_numbered(document, "5").expect("5");
        assert_eq!(stray.layer, "fieldwork");
        assert_eq!(stray.style, "");
        let lines = lines_drawn(document);
        assert_eq!(lines.len(), 1);
        let kerb = polyline_of(&lines[0]);
        assert!(!kerb.closed);
        assert_eq!(
            kerb.vertices,
            [
                Point2::new(500_010.0, 6_200_020.0),
                Point2::new(500_020.0, 6_200_020.0),
                Point2::new(500_020.0, 6_200_030.0),
            ]
        );
        assert_eq!(lines[0].layer, "FIELD KERB");
        assert_eq!(lines[0].style, "FIELD Kerb");
        // No layer per code: only the import's own and the two the rules name.
        for layer in ["fieldwork/KB1", "fieldwork/CTRL", "fieldwork/ZZ"] {
            assert!(!document.model().layers.contains(layer), "{layer}");
        }
        for layer in ["fieldwork", "FIELD KERB", "FIELD CONTROL"] {
            assert!(document.model().layers.contains(layer), "{layer}");
        }

        // Said in the log and at the end of the report pane.
        let did = session.report_after_import();
        for sentence in [
            "5 carry a code and 4 of those have a rule",
            "No rule for the code(s): ZZ.",
            "Linework: 1 line(s) drawn.",
            "2 point(s) are in no line",
        ] {
            assert!(did.contains(sentence), "{sentence}\n{did}");
            assert!(session.logged(sentence), "{sentence}");
        }
        assert!(session.logged(
            "Imported 5 point(s) from kerb.csv, drawn on fieldwork and then finished by the \
             survey codes as follows"
        ));

        session.host.document_mut().undo().expect("undone");
        assert_eq!(drawing_of(session.document()), before);
        assert_eq!(session.document().history().undo_count(), steps);
    }

    #[test]
    fn the_options_step_says_how_survey_codes_are_loaded_by_the_line_that_loads_them() {
        // The note under the Options step's boxes says what Import will do, and with no survey
        // codes loaded what brings some: a Katana customisation file, File > Settings > Import,
        // or the CUSTOMISE <file> line - the words CODE itself refuses with when none are
        // loaded. Worked by hand from that sentence, not from a run.
        let mut session = Session::new();
        drive_to_options(&mut session, &surveyio_data("fld/gnss.fld"));
        let text = session.report("finishNote");
        assert!(text.contains("Katana customisation file"), "{text}");
        assert!(
            text.contains("load one in File > Settings > Import"),
            "{text}"
        );
        assert!(
            text.contains("type CUSTOMISE <file> on the command line"),
            "{text}"
        );
        assert!(!text.contains("Load Customisation"), "{text}");
        assert!(!text.contains("survey code file"), "{text}");
        assert!(!text.contains("Format >"), "{text}");
        // Shown as it is written: "<file>" is not markup (an egui label is plain text, and it
        // is reported as written).
        assert!(text.contains("<file>"), "{text}");
    }

    /// The two boxes are the customisation's two switches when the Options step is first reached
    /// for a file, and the person's from then on; and a layer per field code is offered only
    /// while the codes are not applied, with the reason.
    #[test]
    fn the_finish_boxes_start_as_the_customisation_has_them_and_a_layer_per_code_gives_way_to_the_codes()
     {
        let mut session = Session::new();
        install_field_codes(session.host.document_mut());
        // Codes off, linework on: as CUSTOMISE SET auto.codes=off leaves them.
        session
            .host
            .document_mut()
            .set_automation(CustomisationAutomation {
                codes_on_survey_import: false,
                linework_on_survey_import: true,
            });

        drive_to_options(&mut session, &surveyio_data("fld/gnss.fld"));
        assert!(session.enabled("applyCodes"));
        assert!(session.enabled("drawLinework"));
        assert!(!session.checked("applyCodes"));
        assert!(session.checked("drawLinework"));
        assert!(session.enabled("layerPerCode"), "the codes are not applied");
        let note = session.report("finishNote");
        assert!(note.contains("Survey codes: not applied."), "{note}");
        assert!(
            note.contains("Linework: the imported points are joined into lines"),
            "{note}"
        );
        assert!(
            !note.contains("Load Customisation"),
            "no such menu item exists"
        );

        // Ticked: the codes choose the layers, so a layer per code is not offered, and its tip
        // says why.
        session.fill("applyCodes", "on");
        assert!(!session.enabled("layerPerCode"));
        assert!(
            session
                .wizard
                .layer_per_code_tip()
                .contains("the codes choose each point's layer"),
            "{}",
            session.wizard.layer_per_code_tip()
        );
        let note = session.report("finishNote");
        assert!(
            note.contains("Survey codes: applied to the imported points."),
            "{note}"
        );

        // Back and forth over the same file keeps what the person ticked: the customisation's
        // switch still says off.
        session.click("back");
        session.click("next");
        assert!(session.step().contains("Options"), "{}", session.step());
        assert!(session.checked("applyCodes"));

        // Unticked again, a layer per code is offered again.
        session.fill("applyCodes", "off");
        assert!(session.enabled("layerPerCode"));
        assert!(
            !session
                .wizard
                .layer_per_code_tip()
                .contains("the codes choose each point's layer")
        );

        // Imported so - codes off, linework on - the job is kept as one that was strung and not
        // coded: its points stay on the import's layer, unstyled, and the file's closed string is
        // drawn on its rule's layer.
        session.fill("layer", "fieldwork");
        session.click("next"); // Report
        session.click("import");
        let jobs = session.document().survey_jobs();
        assert_eq!(jobs.len(), 1);
        let stored = read_survey_job_options(&jobs[0].import_options, &jobs[0].id)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(!stored.apply_codes);
        assert!(stored.draw_linework);
        let shot = point_numbered(session.document(), "R001").expect("R001");
        assert_eq!(shot.layer, "fieldwork");
        assert_eq!(shot.style, "");
        let lines = lines_drawn(session.document());
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].layer, "FIELD KERB");
    }

    /// With no survey codes there is nothing to apply or to string by: both boxes are disabled
    /// and unticked, the note says why in today's words, a layer per code is offered as it always
    /// was, and the import is the plain one.
    #[test]
    fn with_no_survey_codes_the_finish_boxes_are_disabled_and_the_note_says_why() {
        let mut session = Session::new();
        drive_to_options(&mut session, &surveyio_data("fld/gnss.fld"));
        assert!(!session.enabled("applyCodes"));
        assert!(!session.enabled("drawLinework"));
        assert!(!session.checked("applyCodes"));
        assert!(!session.checked("drawLinework"));
        assert!(session.enabled("layerPerCode"));
        let note = session.report("finishNote");
        assert!(
            note.contains("Survey codes: the session has none"),
            "{note}"
        );
        assert!(!note.contains("Load Customisation"));
        assert!(!note.contains("survey code file"));

        session.fill("layer", "fieldwork");
        session.click("next"); // Report
        session.click("import");
        // The five points on the import's layer and nothing else; a job that was asked for
        // neither step.
        assert_eq!(session.document().model().entities.size(), 5);
        assert!(lines_drawn(session.document()).is_empty());
        let jobs = session.document().survey_jobs();
        assert_eq!(jobs.len(), 1);
        let stored = read_survey_job_options(&jobs[0].import_options, &jobs[0].id)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(!stored.apply_codes);
        assert!(!stored.draw_linework);

        // Survey codes loaded since, and another file read: the boxes are offered and start as
        // the customisation has them switched - both on.
        install_field_codes(session.host.document_mut());
        session.wizard.show();
        session.run();
        drive_to_options(&mut session, &surveyio_data("fld/gnss.fld"));
        assert!(session.enabled("applyCodes"));
        assert!(session.checked("applyCodes"));
        assert!(session.enabled("drawLinework"));
        assert!(session.checked("drawLinework"));
        assert!(!session.enabled("layerPerCode"));
    }
}
