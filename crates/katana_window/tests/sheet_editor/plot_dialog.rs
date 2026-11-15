//! The twins of tests/qt_widgets/plotting/test_plot_dialog.cpp: the Plot dialog's controls and the
//! request they make, the progress and its Cancel, the choice kept as the page setup, and the
//! editor's Plot buttons.

mod plot_dialog {
    use std::rc::Rc;

    use katana_cad::Document;
    use katana_cad::plot::PlotColourMode;
    use katana_cad::plotting::sheet_commands::add_sheet;
    use katana_cad::plotting::sheet_set::{Sheet, SheetSet};
    use katana_window::drive::{ControlRecord, Harness};
    use katana_window::plan_painter::plan_source_of;
    use katana_window::plotting::plot_dialog::{
        PlotAnswer, PlotDialog, PlotDialogMode, PlotProgressDialog, PlotSheets, finish_plot,
        plot_interactively, plot_with_progress, suggested_plot_file,
    };
    use katana_window::plotting::plot_output::{
        NoPrinters, PlotFormat, PlotRequest, page_setup_for,
    };
    use katana_window::sheet_editor::SourceProvider;
    use katana_window::sheet_painter::SheetSource;

    use crate::support::{Shown, double_eq};
    use crate::window_support::Scratch;

    /// Three A3 sheets, ONE, TWO and THREE, with nothing on them.
    fn three_sheets() -> SheetSet {
        let sheets = ["ONE", "TWO", "THREE"]
            .iter()
            .enumerate()
            .map(|(i, name)| Sheet {
                id: format!("s{}", i + 1),
                name: String::from(*name),
                ..Sheet::default()
            })
            .collect();
        SheetSet {
            sheets,
            ..SheetSet::default()
        }
    }

    /// The dialog on a harness of its own: its controls driven by their object names.
    struct Bench {
        harness: Harness,
        dialog: PlotDialog,
        answer: PlotAnswer,
    }

    impl Bench {
        fn new(dialog: PlotDialog) -> Self {
            let mut harness = Harness::new();
            harness.target(&String::from(dialog.name()));
            let mut bench = Self {
                harness,
                dialog,
                answer: PlotAnswer::Open,
            };
            bench.draw();
            bench
        }

        fn draw(&mut self) {
            let dialog = &mut self.dialog;
            let answer = &mut self.answer;
            self.harness
                .run(|ui| {
                    let said = dialog.ui(ui.ctx(), true);
                    if said != PlotAnswer::Open {
                        *answer = said;
                    }
                })
                .unwrap_or_else(|problem| panic!("{problem}"));
        }

        fn fill(&mut self, name: &str, text: &str) {
            self.harness.fill(name, text);
            self.draw();
        }

        fn press(&mut self, name: &str) {
            self.harness.press(name);
            self.draw();
        }

        fn control(&self, name: &str) -> ControlRecord {
            self.harness
                .control(name)
                .unwrap_or_else(|| panic!("no {name}"))
        }

        fn text(&self, name: &str) -> String {
            self.control(name).text
        }
    }

    fn plot_dialog(set: &SheetSet, current: usize, all: bool) -> Bench {
        Bench::new(PlotDialog::new(
            set,
            current,
            all,
            "C:/plots/set.pdf",
            PlotDialogMode::Plot,
        ))
    }

    fn empty_source(document: &Document) -> SheetSource<'_> {
        SheetSource {
            plan: plan_source_of(document),
            ..SheetSource::default()
        }
    }

    fn files_in(folder: &std::path::Path) -> usize {
        std::fs::read_dir(folder)
            .map(|entries| {
                entries
                    .filter_map(std::result::Result::ok)
                    .filter(|entry| entry.path().is_file())
                    .count()
            })
            .unwrap_or(0)
    }

    #[test]
    fn every_control_has_its_object_name() {
        let bench = plot_dialog(&three_sheets(), 0, true);
        assert_eq!(bench.dialog.name(), "plotDialog");
        for name in [
            "plotSheetsAll",
            "plotSheetsCurrent",
            "plotSheetsList",
            "plotSheetsText",
            "plotFile",
            "plotFolder",
            "plotPattern",
            "plotFormat",
            "plotColourMode",
            "plotPrinter",
            "plotSheetsSummary",
            "plotPatternPreview",
            "plotProblem",
            "plotBrowseFile",
            "plotBrowseFolder",
            "plotRun",
            "plotCancel",
            "plotLineWeightScale",
            "plotDpi",
            "plotOpenAfter",
            "plotKeepSetup",
        ] {
            assert!(bench.harness.control(name).is_some(), "{name}");
        }
    }

    #[test]
    fn it_starts_from_the_page_setup() {
        let mut set = three_sheets();
        set.page_setup.colour_mode = PlotColourMode::Greyscale;
        set.page_setup.line_weight_scale = 0.7;
        set.page_setup.dpi = 150.0;
        set.page_setup.file_name_pattern = String::from("{number} {name}");
        set.page_setup.file_per_sheet = true;
        let bench = plot_dialog(&set, 0, true);
        let dialog = &bench.dialog;
        assert_eq!(dialog.format_name(), "pdfs");
        assert_eq!(dialog.colour_mode_name(), "greyscale");
        assert!(double_eq(dialog.line_weight_scale, 0.7));
        assert_eq!(dialog.dpi, 150);

        let request = dialog.request();
        assert_eq!(request.format, PlotFormat::PdfPerSheet);
        assert_eq!(request.colour_mode, PlotColourMode::Greyscale);
        assert!(double_eq(request.line_weight_scale, 0.7));
        assert!(double_eq(request.dpi, 150.0));
        assert_eq!(request.file_name_pattern, "{number} {name}");
        assert_eq!(request.sheets, "");
        // A file a sheet goes in the suggested file's folder.
        assert_eq!(request.destination, "C:/plots");
        assert!(!dialog.to_printer());
        assert!(dialog.keep_as_page_setup());
        assert!(!dialog.open_afterwards());
        assert!(dialog.check().is_ok());
        assert!(bench.control("plotRun").enabled);
        // Kept as it is, it is the page setup it came from.
        assert_eq!(page_setup_for(&request, &set.page_setup), set.page_setup);
    }

    #[test]
    fn the_sheets_chosen_become_the_selection() {
        let mut bench = plot_dialog(&three_sheets(), 1, false);
        // Opened from Plot Sheet: the current sheet.
        assert_eq!(bench.control("plotSheetsCurrent").checked, Some(true));
        assert_eq!(bench.dialog.request().sheets, "2");
        assert_eq!(bench.text("plotSheetsSummary"), "1 sheet: 2");
        assert_eq!(bench.dialog.request().destination, "C:/plots/set.pdf");

        bench.press("plotSheetsAll");
        assert_eq!(bench.dialog.request().sheets, "");
        assert_eq!(bench.text("plotSheetsSummary"), "3 sheets");

        // Typing a list chooses it.
        bench.fill("plotSheetsText", "3, 1");
        assert_eq!(bench.control("plotSheetsList").checked, Some(true));
        assert_eq!(bench.dialog.request().sheets, "3, 1");
        assert_eq!(bench.text("plotSheetsSummary"), "2 sheets: 3,1");

        // A sheet that is not there: said, and Plot is off.
        bench.fill("plotSheetsText", "9");
        assert_eq!(
            bench.text("plotProblem"),
            "there is no sheet 9: the set has 3 sheets"
        );
        assert!(!bench.control("plotRun").enabled);
        assert!(bench.dialog.check().is_err());
    }

    #[test]
    fn a_file_per_sheet_shows_the_folder_and_the_first_files_name() {
        let mut set = three_sheets();
        set.defaults.set_number = String::from("C");
        let mut bench = plot_dialog(&set, 0, true);
        assert!(bench.control("plotFile").visible);
        assert!(!bench.control("plotFolder").visible);

        bench.fill("plotFormat", "PNG images, one per sheet");
        assert!(!bench.control("plotFile").visible);
        assert!(bench.control("plotFolder").visible);
        assert!(bench.control("plotPattern").visible);
        assert_eq!(bench.text("plotPatternPreview"), "First file: C01 ONE.png");

        bench.fill("plotPattern", "{name}-{n}");
        assert_eq!(bench.text("plotPatternPreview"), "First file: ONE-1.png");
        bench.fill("plotPattern", "{nope}");
        assert!(!bench.control("plotRun").enabled);
        assert!(bench.text("plotProblem").contains("{nope}"));

        // A raster too large to make is refused before it is tried.
        bench.fill("plotPattern", "{n}");
        bench.fill("plotDpi", "1200");
        assert!(!bench.control("plotRun").enabled);
        assert!(bench.text("plotProblem").contains("megapixels"));
        bench.fill("plotDpi", "300");
        assert!(bench.control("plotRun").enabled);
        assert!(bench.text("plotProblem").is_empty());
    }

    #[test]
    fn a_printer_needs_no_file_but_must_be_there() {
        let mut bench = plot_dialog(&three_sheets(), 0, true);
        bench.fill("plotFormat", "Printer");
        assert!(bench.dialog.to_printer());
        assert!(bench.dialog.request().destination.is_empty());
        assert!(!bench.control("plotFile").visible);
        assert!(bench.control("plotPrinter").visible);
        assert!(!bench.control("plotOpenAfter").enabled);
        if bench.control("plotPrinter").items.is_empty() {
            assert_eq!(bench.text("plotProblem"), "there is no printer to print on");
            assert!(!bench.control("plotRun").enabled);
        } else {
            assert!(bench.dialog.check().is_ok());
        }
    }

    #[test]
    fn the_progress_dialogs_cancel_stops_the_plot_before_the_next_sheet() {
        let document = Document::new(None);
        let set = three_sheets();
        let scratch = Scratch::new("plot-progress");
        let mut request = PlotRequest {
            format: PlotFormat::Png,
            dpi: 50.0,
            destination: scratch.path("png").to_string_lossy().into_owned(),
            ..PlotRequest::default()
        };

        // Uninterrupted: every sheet.
        let progress = PlotProgressDialog::new();
        let whole = plot_with_progress(
            &set,
            &request,
            &mut || empty_source(&document),
            None,
            &progress,
        )
        .expect("plotted");
        assert_eq!(whole.files.len(), 3);
        assert!(!whole.cancelled);

        // Cancel pressed while the first sheet is painted: the source is asked for as each sheet
        // starts, and that is when the progress dialog's Cancel is pressed.
        request.destination = scratch.path("cancelled").to_string_lossy().into_owned();
        let mut sheets_started = 0;
        let mut pressed = false;
        let cancelled = plot_with_progress(
            &set,
            &request,
            &mut || {
                sheets_started += 1;
                if sheets_started == 1 && progress.is_shown() {
                    progress.cancel();
                    pressed = true;
                }
                empty_source(&document)
            },
            None,
            &progress,
        )
        .expect("plotted");
        assert!(pressed);
        assert!(cancelled.cancelled);
        assert_eq!(cancelled.sheets.len(), 1);
        assert_eq!(files_in(&scratch.path("cancelled")), 1);
        // The progress dialog is gone with the plot.
        assert!(!progress.is_shown());
    }

    fn native(path: &str) -> String {
        if cfg!(windows) {
            path.replace('/', "\\")
        } else {
            String::from(path)
        }
    }

    #[test]
    fn plotting_keeps_the_choice_as_the_page_setup_in_one_step() {
        let mut document = Document::new(None);
        for sheet in three_sheets().sheets {
            add_sheet(&mut document, sheet, None).expect("the sheet is added");
        }
        let scratch = Scratch::new("plot-keeps");
        let folder = scratch.path("mono").to_string_lossy().into_owned();
        let suggested = scratch.path("set.pdf").to_string_lossy().into_owned();
        let steps_before = document.history().undo_count();
        let provider: SourceProvider = Rc::new(|document: &Document| empty_source(document));
        let mut messages: Vec<(String, bool)> = Vec::new();

        let set = SheetSet::clone(&document.sheet_set());
        let dialog = plot_interactively(&set, 0, true, &suggested, &mut |text: &str, error: bool| {
            messages.push((String::from(text), error));
        })
        .expect("the dialog opens");
        let mut bench = Bench::new(dialog);
        bench.fill("plotFormat", "PNG images, one per sheet");
        bench.fill("plotColourMode", "Monochrome (all ink black)");
        bench.fill("plotDpi", "50");
        bench.fill("plotFolder", &folder);
        bench.press("plotRun");
        assert_eq!(bench.answer, PlotAnswer::Accepted);
        let _ = finish_plot(
            &bench.dialog,
            &set,
            "TEST",
            &provider,
            &mut document,
            &NoPrinters,
            &PlotProgressDialog::new(),
            &mut |text: &str, error: bool| messages.push((String::from(text), error)),
        );
        assert_eq!(files_in(std::path::Path::new(&folder)), 3);
        // The choice is the page setup now, as one step.
        let kept = document.sheet_set().page_setup.clone();
        assert_eq!(kept.colour_mode, PlotColourMode::Monochrome);
        assert_eq!(kept.dpi, 50.0);
        // A raster is not a page setup's format.
        assert!(!kept.file_per_sheet);
        assert_eq!(document.history().undo_count(), steps_before + 1);
        assert_eq!(document.history().undo_name(), "PAGE_SETUP");
        let last = messages.last().cloned().expect("a message");
        assert_eq!(
            last.0,
            format!("Plotted 3 sheets to 3 PNG files in {}.", native(&folder))
        );
        assert!(!last.1);

        // Cancelled, the dialog does nothing at all.
        messages.clear();
        let set = SheetSet::clone(&document.sheet_set());
        let dialog = plot_interactively(&set, 0, true, &suggested, &mut |text: &str, error: bool| {
            messages.push((String::from(text), error));
        })
        .expect("the dialog opens");
        let mut bench = Bench::new(dialog);
        bench.press("plotCancel");
        assert_eq!(bench.answer, PlotAnswer::Cancelled);
        assert!(messages.is_empty());
        assert_eq!(document.history().undo_count(), steps_before + 1);
    }

    #[test]
    fn the_editors_plot_buttons_open_it() {
        let mut document = Document::new(None);
        for sheet in three_sheets().sheets {
            add_sheet(&mut document, sheet, None).expect("the sheet is added");
        }
        let provider: SourceProvider = Rc::new(|document: &Document| empty_source(document));
        let mut shown = Shown::bare(document, provider);
        shown.editor.set_current_sheet(&shown.host, 2);
        for (action, chosen) in [
            ("sheetPlotAll", PlotSheets::All),
            ("sheetPlotSheet", PlotSheets::Current),
        ] {
            shown.trigger(action);
            let dialog = shown.editor.plot_dialog().expect("the dialog is open");
            assert_eq!(dialog.sheets, chosen, "{action}");
            let sheets = dialog.request().sheets;
            assert_eq!(sheets, if action == "sheetPlotAll" { "" } else { "3" });
        }
    }

    #[test]
    fn the_suggested_file_is_named_after_the_project() {
        let mut document = Document::new(None);
        // A new drawing is "Untitled".
        assert!(suggested_plot_file(&document).ends_with("/Untitled.pdf"));
        let mut metadata = document.metadata().clone();
        metadata.name = String::from("Main Street: Stage 2");
        document.set_metadata(metadata.clone());
        assert!(suggested_plot_file(&document).ends_with("/Main Street- Stage 2.pdf"));
        metadata.name.clear();
        document.set_metadata(metadata);
        assert!(suggested_plot_file(&document).ends_with("/sheets.pdf"));
    }
}
