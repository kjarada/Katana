//! The twins of tests/qt_widgets/plotting/test_plot_drawing_dialog.cpp: the PLOT line read and
//! written (katana_window::plotting::plot_drawing_dialog), and File > Plot to PDF driven by its
//! controls' object names - the dialog writes the line and hands it to the window's executor,
//! which here is a recording host answering as the C++ tests' runners did.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use katana_cad::plot::{PaperSize, PlotColourMode, PlotSettings};
use katana_window::host::VerbOutcome;
use katana_window::plotting::plot_drawing_dialog::{
    PlotDrawingDialog, PlotDrawingRequest, parse_plot_drawing, plot_drawing_command_line,
};

use crate::support::Bench;
use crate::window_support::{Scratch, fixed_host, ok, refused};

/// QDir::toNativeSeparators.
fn native(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        String::from(path)
    }
}

/// The dialog on `suggested`, shown, on the harness; every line it runs answered `outcome`.
fn bench(dialog: PlotDrawingDialog, outcome: VerbOutcome) -> Bench<PlotDrawingDialog> {
    let mut dialog = dialog;
    dialog.show();
    Bench::new("plotDrawingDialog", dialog, fixed_host(outcome))
}

mod plot_drawing {
    use super::*;

    #[test]
    fn what_is_not_given_takes_the_plot_defaults() {
        let request = parse_plot_drawing("PLOT site.pdf")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let defaults = PlotSettings::default();
        assert_eq!(request.path, "site.pdf");
        assert!(request.fit);
        assert_eq!(request.settings.paper, defaults.paper);
        assert_eq!(request.settings.landscape, defaults.landscape);
        assert_eq!(request.settings.dpi, defaults.dpi);
        assert_eq!(request.settings.margin_mm, defaults.margin_mm);
        assert_eq!(request.settings.colour_mode, PlotColourMode::Colour);
        assert_eq!(request.settings.line_weight_scale, 1.0);
    }

    #[test]
    fn every_option_is_read() {
        let request = parse_plot_drawing(
            "plot \"C:/my plots/site.pdf\" paper=a1 PORTRAIT scale=1:250 dpi=150 style=mono \
             lineweight=0.7 margin=5",
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(request.path, "C:/my plots/site.pdf");
        assert!(!request.fit);
        assert_eq!(request.settings.paper, PaperSize::A1);
        assert!(!request.settings.landscape);
        assert_eq!(request.settings.scale_denominator, 250.0);
        assert_eq!(request.settings.dpi, 150.0);
        assert_eq!(request.settings.colour_mode, PlotColourMode::Monochrome);
        assert_eq!(request.settings.line_weight_scale, 0.7);
        assert_eq!(request.settings.margin_mm, 5.0);
    }

    #[test]
    fn fit_after_a_scale_fits_again() {
        let request = parse_plot_drawing("PLOT a.pdf scale=500 fit").expect("the line reads");
        assert!(request.fit);
    }

    #[test]
    fn what_the_grammar_does_not_take_is_refused_by_its_word() {
        for line in [
            "PLOT",
            "PLOT \"\"",
            "PLOT a.pdf paper=B2",
            "PLOT a.pdf scale=0",
            "PLOT a.pdf scale=-5",
            "PLOT a.pdf scale=big",
            "PLOT a.pdf dpi=10",
            "PLOT a.pdf dpi=5000",
            "PLOT a.pdf style=sepia",
            "PLOT a.pdf lineweight=0.05",
            "PLOT a.pdf lineweight=9",
            "PLOT a.pdf margin=60",
            "PLOT a.pdf colour",
            "PLOT a.pdf size=A3",
            "PLOT \"never closed",
        ] {
            assert!(parse_plot_drawing(line).is_err(), "{line}");
        }
        let paper = parse_plot_drawing("PLOT a.pdf paper=B2")
            .expect_err("B2 is no paper")
            .describe();
        assert!(paper.contains("paper=B2"), "{paper}");
        assert!(paper.contains("usage: PLOT"), "{paper}");
    }

    #[test]
    fn the_line_it_writes_is_the_line_it_reads() {
        let mut request = PlotDrawingRequest {
            // The path as the platform's file dialog gives it: "C:\plots\site plan.pdf" on
            // Windows. On POSIX '\' is part of a file name, not a separator (POSIX.1-2017, 3.170),
            // so each platform is given its own native form.
            path: native("C:/plots/site plan.pdf"),
            fit: false,
            ..PlotDrawingRequest::default()
        };
        request.settings.paper = PaperSize::A2;
        request.settings.landscape = false;
        request.settings.scale_denominator = 750.0;
        request.settings.dpi = 200.0;
        request.settings.colour_mode = PlotColourMode::Greyscale;
        request.settings.line_weight_scale = 1.4;
        request.settings.margin_mm = 12.5;
        let line = plot_drawing_command_line(&request);
        assert_eq!(
            line,
            "PLOT \"C:/plots/site plan.pdf\" paper=A2 portrait scale=750 dpi=200 style=greyscale \
             lineweight=1.4 margin=12.5"
        );
        let read =
            parse_plot_drawing(&line).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(read.path, "C:/plots/site plan.pdf");
        assert!(!read.fit);
        assert_eq!(read.settings.paper, PaperSize::A2);
        assert_eq!(read.settings.scale_denominator, 750.0);
        assert_eq!(read.settings.colour_mode, PlotColourMode::Greyscale);
        assert_eq!(read.settings.line_weight_scale, 1.4);
        assert_eq!(read.settings.margin_mm, 12.5);
    }
}

mod plot_drawing_dialog {
    use super::*;

    #[test]
    fn the_fields_write_the_line_and_plot_runs_it() {
        let mut bench = bench(
            PlotDrawingDialog::new(""),
            ok("Plotted to out.pdf\nfile=\"out.pdf\" paper=A4"),
        );
        assert!(
            bench
                .harness
                .target_record("plotDrawingDialog")
                .is_some()
        );
        assert!(
            !bench.enabled("plotDrawingPlot"),
            "nothing to plot to yet"
        );
        bench.fill("plotDrawingPath", "out.pdf");
        bench.fill("plotDrawingPaper", "A4");
        bench.fill("plotDrawingOrientation", "Portrait");
        bench.fill("plotDrawingFit", "off");
        assert!(bench.enabled("plotDrawingScale"));
        bench.fill("plotDrawingScale", "250");
        bench.fill("plotDrawingDpi", "150");
        bench.fill("plotDrawingMargin", "5");
        bench.fill("plotDrawingColourMode", "Monochrome");
        bench.fill("plotDrawingLineWeightScale", "0.7");
        let expected = "PLOT \"out.pdf\" paper=A4 portrait scale=250 dpi=150 style=monochrome \
                        lineweight=0.7 margin=5";
        assert_eq!(bench.value("plotDrawingCommand"), expected);
        assert!(bench.enabled("plotDrawingPlot"));
        bench.press("plotDrawingPlot");
        assert_eq!(bench.host.lines(), [expected]);
        assert_eq!(
            bench.text("plotDrawingStatus"),
            "Plotted: file=\"out.pdf\" paper=A4"
        );
    }

    #[test]
    fn fitting_leaves_the_scale_out_and_a_refusal_is_said() {
        let mut bench = bench(
            PlotDrawingDialog::new("site.pdf"),
            refused("InvalidState: no plan viewport to plot from"),
        );
        assert!(!bench.enabled("plotDrawingScale"));
        assert_eq!(
            bench.value("plotDrawingCommand"),
            "PLOT \"site.pdf\" paper=A3 landscape fit dpi=300 style=colour lineweight=1 margin=10"
        );
        bench.dialog.plot(&mut bench.host);
        bench.draw();
        assert_eq!(
            bench.text("plotDrawingStatus"),
            "Not plotted: InvalidState: no plan viewport to plot from"
        );
    }

    // Kept while the window lives, the dialog's file follows the drawing (suggest_path) until one
    // is typed - it once plotted every drawing into the first project's folder - and a file that
    // is there is written over only when the person says so.
    #[test]
    fn the_suggested_file_follows_the_drawing_until_one_is_typed_and_a_replace_is_asked() {
        let scratch = Scratch::new("plot_drawing_dialog_replace");
        let there = scratch
            .file("there.pdf", "")
            .to_string_lossy()
            .into_owned();

        let asked: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let replace = Rc::new(Cell::new(false));
        let mut dialog = PlotDrawingDialog::new("C:/projA/projA.pdf");
        {
            let asked = Rc::clone(&asked);
            let replace = Rc::clone(&replace);
            dialog.set_confirm_replace(Box::new(move |path: &str| {
                asked.borrow_mut().push(String::from(path));
                replace.get()
            }));
        }
        let mut bench = bench(dialog, ok("file=\"there.pdf\""));
        bench.dialog.suggest_path("C:/projB/projB.pdf");
        bench.draw();
        assert_eq!(bench.value("plotDrawingPath"), "C:/projB/projB.pdf");

        bench.fill("plotDrawingPath", &there);
        bench.dialog.suggest_path("C:/projC/projC.pdf");
        bench.draw();
        assert_eq!(bench.value("plotDrawingPath"), there, "a file typed is kept");

        bench.dialog.plot(&mut bench.host);
        bench.draw();
        assert_eq!(*asked.borrow(), vec![there.clone()]);
        assert!(
            bench.host.lines().is_empty(),
            "kept when the person says no"
        );
        assert!(bench.text("plotDrawingStatus").starts_with("Not plotted:"));
        replace.set(true);
        bench.dialog.plot(&mut bench.host);
        assert_eq!(bench.host.lines().len(), 1);
        // A file that is not there is written without a question.
        bench.fill(
            "plotDrawingPath",
            &scratch.path("new.pdf").to_string_lossy(),
        );
        bench.dialog.plot(&mut bench.host);
        assert_eq!(asked.borrow().len(), 2);
        assert_eq!(bench.host.lines().len(), 2);
    }

    // The PLOT reply says what a plot is without as `warning` records, before the file record that
    // stays its last line (the window's plot to PDF); the dialog shows those records as they came,
    // and only while the last plot had some.
    #[test]
    fn the_warning_records_of_the_reply_are_shown_and_the_file_record_stays_the_status() {
        let first = "warning surface=\"ground1\" style=\"Fine\" reason=\"contours: too fine\"";
        let second = "warning surface=\"ground2\" style=\"Fine\" reason=\"contours: too fine\"";
        let reply_with = |warnings: usize| {
            let mut reply = String::from("Plotted to out.pdf");
            if warnings >= 1 {
                reply.push('\n');
                reply.push_str(first);
            }
            if warnings >= 2 {
                reply.push('\n');
                reply.push_str(second);
            }
            reply.push_str("\nfile=\"out.pdf\" paper=A4");
            reply
        };
        let mut bench = bench(PlotDrawingDialog::new("out.pdf"), ok(&reply_with(2)));
        assert!(
            !bench.control("plotDrawingWarnings").visible,
            "nothing to say before a plot"
        );

        bench.dialog.plot(&mut bench.host);
        bench.draw();
        assert!(bench.control("plotDrawingWarnings").visible);
        assert_eq!(
            bench.text("plotDrawingWarnings"),
            format!("{first}\n{second}")
        );
        assert_eq!(
            bench.text("plotDrawingStatus"),
            "Plotted: file=\"out.pdf\" paper=A4"
        );

        // A plot that lost nothing says nothing, and the last one's words go.
        bench.host = fixed_host(ok(&reply_with(0)));
        bench.dialog.plot(&mut bench.host);
        bench.draw();
        assert!(!bench.control("plotDrawingWarnings").visible);
        assert!(bench.text("plotDrawingWarnings").is_empty());
    }

    #[test]
    fn a_headless_session_is_told_to_fill_the_path() {
        // The recording host is a headless session, as the C++ test's context said it was.
        let mut bench = bench(PlotDrawingDialog::new(""), ok(""));
        bench.press("plotDrawingBrowse");
        assert_eq!(
            bench.text("plotDrawingStatus"),
            "A headless session opens no file dialog: fill plotDrawingPath with the path instead."
        );
    }
}
