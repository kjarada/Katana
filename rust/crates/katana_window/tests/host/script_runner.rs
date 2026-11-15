//! File > Run Script and the SCRIPT verb (src/katana_window/src/script_runner.rs;
//! script_runner.hpp): how a script is read, how its lines are run and when the run stops, and the
//! dialog, driven by its object names as a person drives it.
//!
//! The runners here are the tests' own - they record the lines and refuse the ones a test names -
//! so what is tested is the order, the stopping and the record, not the verbs. The headless
//! qt_script_* checks run real scripts in the real window.

use std::cell::RefCell;
use std::rc::Rc;

use katana_core::ErrorCode;
use katana_window::drive::Harness;
use katana_window::host::{Answer, HostParts, RecordingHost, VerbOutcome};
use katana_window::script_runner::{
    ScriptLine, ScriptOptions, ScriptReport, ScriptRunDialog, format_script_report,
    is_script_comment, parse_script_command, read_script, run_script_lines, script_command_line,
    script_lines,
};

use crate::window_support::{Scratch, qt_widgets_data};

/// tests/qt_widgets/data/scripts, as the text a path is typed as.
fn scripts() -> String {
    qt_widgets_data()
        .join("scripts")
        .to_string_lossy()
        .into_owned()
}

/// Records each line it is given, and refuses those in `refused`.
#[derive(Default)]
struct RecordingRunner {
    lines: Vec<String>,
    refused: Vec<String>,
}

impl RecordingRunner {
    fn run(&mut self, line: &str) -> VerbOutcome {
        self.lines.push(String::from(line));
        if self.refused.iter().any(|refused| refused == line) {
            return VerbOutcome::new(false, "", &format!("refused: {line}"));
        }
        VerbOutcome::new(true, &format!("ran {line}"), "")
    }
}

/// A host whose runner is a RecordingRunner refusing `refused`: the dialog's executor.
fn recording_host(refused: Vec<String>) -> RecordingHost {
    let runner = Rc::new(RefCell::new(RecordingRunner {
        lines: Vec::new(),
        refused,
    }));
    RecordingHost::new().answering(Answer::Custom(Box::new(
        move |_parts: &mut HostParts, line: &str| runner.borrow_mut().run(line),
    )))
}

fn lines_of(texts: &[&str]) -> Vec<ScriptLine> {
    (1..)
        .zip(texts)
        .map(|(number, text)| ScriptLine {
            number,
            text: String::from(*text),
        })
        .collect()
}

mod script_runner {
    use super::*;

    #[test]
    fn blank_lines_and_comments_are_skipped_and_the_file_line_numbers_kept() {
        let lines =
            script_lines("# heading\n\nRECT 0,0 10,5\n   # indented\n  CIRCLE 5,5 2  \n\t\nLIST");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].number, 3);
        assert_eq!(lines[0].text, "RECT 0,0 10,5");
        assert_eq!(lines[1].number, 5);
        assert_eq!(lines[1].text, "CIRCLE 5,5 2");
        assert_eq!(lines[2].number, 7);
        assert_eq!(lines[2].text, "LIST");
    }

    #[test]
    fn windows_and_old_mac_line_ends_are_line_ends() {
        let crlf = script_lines("LINE 0,0 1,1\r\nLIST\r\n");
        assert_eq!(crlf.len(), 2);
        assert_eq!(crlf[0].text, "LINE 0,0 1,1");
        assert_eq!(crlf[1].number, 2);
        let cr = script_lines("LINE 0,0 1,1\rLIST");
        assert_eq!(cr.len(), 2);
        assert_eq!(cr[1].text, "LIST");
    }

    #[test]
    fn text_with_no_commands_has_no_lines() {
        assert!(script_lines("").is_empty());
        assert!(script_lines("\n\n# only a comment\n   \n").is_empty());
    }

    #[test]
    fn an_info_of_an_anchored_id_is_a_command_not_a_comment() {
        assert!(is_script_comment("  # a note"));
        assert!(!is_script_comment("INFO #12"));
    }

    #[test]
    fn the_fixture_script_is_read_as_katana_cli_reads_it() {
        let lines = read_script(&format!("{}/good.kcs", scripts()))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        // good.kcs: three comment lines, then commands on lines 4, 6 and 8.
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].number, 4);
        assert_eq!(lines[0].text, "RECT 0,0 10,5");
        assert_eq!(lines[1].number, 6);
        assert_eq!(lines[2].number, 8);
        assert_eq!(lines[2].text, "LINE 0,0 10,0");
    }

    #[test]
    fn a_file_written_with_windows_line_ends_reads_the_same() {
        let folder = Scratch::new("crlf-script");
        let path = folder.file(
            "crlf.kcs",
            "# written on Windows\r\nRECT 0,0 10,5\r\n\r\nLIST\r\n",
        );
        let lines = read_script(&path.to_string_lossy()).expect("the script reads");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "RECT 0,0 10,5");
        assert_eq!(lines[1].number, 4);
        assert_eq!(lines[1].text, "LIST");
    }

    #[test]
    fn a_missing_script_is_not_found_by_its_path() {
        let error = read_script(&format!("{}/no_such_script.kcs", scripts()))
            .expect_err("there is no such script");
        assert_eq!(error.code, ErrorCode::NotFound);
        assert!(error.describe().contains("no_such_script.kcs"));
    }

    #[test]
    fn the_script_verb_reads_a_quoted_path_and_continue() {
        let quoted = parse_script_command("SCRIPT \"C:/my scripts/site plan.kcs\"")
            .expect("a quoted path reads");
        assert_eq!(quoted.path, "C:/my scripts/site plan.kcs");
        assert!(!quoted.continue_on_error);
        let going = parse_script_command("script setup.kcs continue").expect("CONTINUE reads");
        assert_eq!(going.path, "setup.kcs");
        assert!(going.continue_on_error);
    }

    #[test]
    fn the_script_verb_refuses_no_file_and_a_word_it_does_not_know() {
        for line in [
            "SCRIPT",
            "SCRIPT \"\"",
            "SCRIPT a.kcs AGAIN",
            "SCRIPT a b CONTINUE",
            "SCRIPT \"never closed",
        ] {
            assert!(parse_script_command(line).is_err(), "{line}");
        }
        let usage = parse_script_command("SCRIPT").expect_err("no file");
        assert!(
            usage
                .describe()
                .contains("usage: SCRIPT <file.kcs> [CONTINUE]")
        );
    }

    #[test]
    fn the_line_it_writes_is_the_line_it_reads() {
        // The dialog hands over the path the platform's file dialog gave, in the platform's own
        // separators: "C:\my scripts\site.kcs" on Windows. The line writes it with '/', which every
        // platform's file API reads. On a POSIX system '\' is an ordinary character of a file
        // name, not a separator (POSIX.1-2017, 3.170 "Filename"), so a Windows path is not a
        // native path there and the test gives each platform its own.
        let native = if cfg!(windows) {
            "C:\\my scripts\\site.kcs"
        } else {
            "C:/my scripts/site.kcs"
        };
        let line = script_command_line(native, true);
        assert_eq!(line, "SCRIPT \"C:/my scripts/site.kcs\" CONTINUE");
        let command = parse_script_command(&line).expect("the line reads back");
        assert_eq!(command.path, "C:/my scripts/site.kcs");
        assert!(command.continue_on_error);
        assert_eq!(script_command_line("a.kcs", false), "SCRIPT \"a.kcs\"");
    }

    #[test]
    fn every_line_runs_in_order_when_none_is_refused() {
        let mut recording = RecordingRunner::default();
        let lines = lines_of(&["RECT 0,0 10,5", "CIRCLE 5,5 2", "LIST"]);
        let report = run_script_lines(
            &lines,
            &mut |line: &str| recording.run(line),
            ScriptOptions::default(),
        );
        assert_eq!(recording.lines, ["RECT 0,0 10,5", "CIRCLE 5,5 2", "LIST"]);
        assert_eq!(report.lines, 3);
        assert_eq!(report.ran, 3);
        assert_eq!(report.failed, 0);
        assert_eq!(report.stopped_at, 0);
        assert!(report.ok());
        assert_eq!(
            format_script_report("a.kcs", &report),
            "script=\"a.kcs\" lines=3 ran=3 failed=0"
        );
    }

    #[test]
    fn the_run_stops_at_the_first_refused_line() {
        let mut recording = RecordingRunner {
            refused: vec![String::from("CIRCLE 0,0 -1")],
            ..RecordingRunner::default()
        };
        let lines = lines_of(&["RECT 0,0 10,5", "CIRCLE 0,0 -1", "LINE 0,0 10,0"]);
        let report = run_script_lines(
            &lines,
            &mut |line: &str| recording.run(line),
            ScriptOptions::default(),
        );
        assert_eq!(
            recording.lines.len(),
            2,
            "the line after the refused one must not run"
        );
        assert_eq!(report.ran, 2);
        assert_eq!(report.failed, 1);
        assert_eq!(report.stopped_at, 2);
        assert!(!report.ok());
        assert_eq!(
            format_script_report("bad.kcs", &report),
            "script=\"bad.kcs\" lines=3 ran=2 failed=1 stopped_at=2"
        );
    }

    #[test]
    fn continuing_runs_every_line_and_counts_the_refused() {
        let mut recording = RecordingRunner {
            refused: vec![String::from("CIRCLE 0,0 -1"), String::from("BOGUS")],
            ..RecordingRunner::default()
        };
        let lines = lines_of(&["RECT 0,0 10,5", "CIRCLE 0,0 -1", "BOGUS", "LINE 0,0 10,0"]);
        let report = run_script_lines(
            &lines,
            &mut |line: &str| recording.run(line),
            ScriptOptions {
                continue_on_error: true,
                progress: None,
            },
        );
        assert_eq!(recording.lines.len(), 4);
        assert_eq!(report.ran, 4);
        assert_eq!(report.failed, 2);
        assert_eq!(report.stopped_at, 0);
        assert!(!report.ok());
    }

    #[test]
    fn quit_ends_the_script_without_failing_it_as_in_katana_cli() {
        let mut recording = RecordingRunner::default();
        let lines = lines_of(&["RECT 0,0 10,5", "quit now", "LIST"]);
        let report = run_script_lines(
            &lines,
            &mut |line: &str| recording.run(line),
            ScriptOptions::default(),
        );
        assert_eq!(
            recording.lines,
            ["RECT 0,0 10,5"],
            "QUIT is never run: it would close the window under the person who ran the script"
        );
        assert!(report.quit);
        assert!(report.ok());
        assert_eq!(report.stopped_at, 2);
        assert_eq!(
            format_script_report("", &report),
            "script=pasted lines=3 ran=1 failed=0 quit_at=2"
        );
    }

    #[test]
    fn cancelling_stops_before_the_next_line() {
        let mut recording = RecordingRunner::default();
        let lines = lines_of(&["RECT 0,0 10,5", "CIRCLE 5,5 2", "LIST"]);
        let mut asked: Vec<(usize, usize)> = Vec::new();
        let mut progress = |done: usize, total: usize| {
            asked.push((done, total));
            // Cancel pressed while the second line ran.
            done < 2
        };
        let report: ScriptReport = run_script_lines(
            &lines,
            &mut |line: &str| recording.run(line),
            ScriptOptions {
                continue_on_error: false,
                progress: Some(&mut progress),
            },
        );
        assert_eq!(recording.lines.len(), 2);
        assert_eq!(asked, [(0, 3), (1, 3), (2, 3)]);
        assert!(report.cancelled);
        assert!(!report.ok());
        assert_eq!(report.stopped_at, 3);
        assert_eq!(
            format_script_report("a.kcs", &report),
            "script=\"a.kcs\" lines=3 ran=2 failed=0 cancelled_at=3"
        );
    }

    #[test]
    fn an_empty_script_runs_nothing_and_is_ok() {
        let mut recording = RecordingRunner::default();
        let report = run_script_lines(
            &[],
            &mut |line: &str| recording.run(line),
            ScriptOptions::default(),
        );
        assert!(recording.lines.is_empty());
        assert_eq!(report.lines, 0);
        assert!(report.ok());
    }
}

// ---- the dialog -------------------------------------------------------------------------------

/// The dialog drawn in a harness whose acts go to it.
fn shown(dialog: &mut ScriptRunDialog, host: &mut RecordingHost) -> Harness {
    let mut harness = Harness::new();
    harness.target("fileRunScriptDialog");
    harness
        .show(|ui| dialog.ui(ui, &mut *host))
        .expect("the dialog is drawn");
    harness
}

mod script_run_dialog {
    use super::*;

    #[test]
    fn it_shows_the_scripts_commands_and_runs_the_line_it_shows() {
        let mut host = recording_host(Vec::new());
        let mut dialog = ScriptRunDialog::new();
        let mut harness = shown(&mut dialog, &mut host);
        assert!(harness.target_record("fileRunScriptDialog").is_some());
        assert!(
            !harness.control("scriptRun").expect("Run").enabled,
            "nothing to run before a script is named"
        );

        let good = format!("{}/good.kcs", scripts());
        harness
            .fill("scriptPath", &good)
            .run(|ui| dialog.ui(ui, &mut host))
            .expect("the path is typed");
        assert_eq!(
            harness.control("scriptPreview").expect("the preview").value,
            "   4  RECT 0,0 10,5\n   6  CIRCLE 5,5 2\n   8  LINE 0,0 10,0"
        );
        harness
            .fill("scriptContinueOnError", "on")
            .run(|ui| dialog.ui(ui, &mut host))
            .expect("Continue after errors is ticked");
        let expected = format!("SCRIPT \"{good}\" CONTINUE");
        assert_eq!(
            harness.control("scriptCommand").expect("the command").value,
            expected
        );

        assert!(harness.control("scriptRun").expect("Run").enabled);
        harness
            .press("scriptRun")
            .run(|ui| dialog.ui(ui, &mut host))
            .expect("Run is pressed");
        assert_eq!(host.lines(), std::slice::from_ref(&expected));
        assert_eq!(
            harness.report("scriptStatus").expect("the status"),
            format!("Done: ran {expected}")
        );
    }

    #[test]
    fn a_refusal_is_said_in_the_status() {
        let bad = format!("{}/bad.kcs", scripts());
        let line = format!("SCRIPT \"{bad}\"");
        let mut host = recording_host(vec![line.clone()]);
        let mut dialog = ScriptRunDialog::new();
        dialog.set_script_path(&bad);
        dialog.run(&mut host);
        let harness = shown(&mut dialog, &mut host);
        assert_eq!(
            harness.report("scriptStatus").expect("the status"),
            format!("Stopped: refused: {line}")
        );
    }

    #[test]
    fn a_file_that_cannot_be_read_says_so_in_the_preview() {
        let mut host = RecordingHost::new();
        let mut dialog = ScriptRunDialog::new();
        let mut harness = shown(&mut dialog, &mut host);
        harness
            .fill("scriptPath", &format!("{}/no_such_script.kcs", scripts()))
            .run(|ui| dialog.ui(ui, &mut host))
            .expect("the path is typed");
        assert!(
            harness
                .control("scriptPreview")
                .expect("the preview")
                .value
                .starts_with("NotFound: no such script")
        );
        // The line is still the verb's to refuse, by its path, once it runs.
        assert!(harness.control("scriptRun").expect("Run").enabled);
    }

    #[test]
    fn a_headless_session_is_told_to_fill_the_path_rather_than_browse() {
        let mut host = RecordingHost::new();
        assert!(host.parts.headless);
        let mut dialog = ScriptRunDialog::new();
        let mut harness = shown(&mut dialog, &mut host);
        harness
            .press("scriptBrowse")
            .run(|ui| dialog.ui(ui, &mut host))
            .expect("Browse is pressed");
        assert_eq!(
            harness.report("scriptStatus").expect("the status"),
            "A headless session opens no file dialog: fill scriptPath with the path instead."
        );
    }
}
