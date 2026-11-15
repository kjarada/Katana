//! The twins of tests/qt_widgets/plotting/test_sheet_view_options.cpp: the view's place on the
//! paper typed as VIEW SET rect=, the line handed to the window's executor, cross sections cut
//! every interval or at chainages, a section's hidden layers, an Image view's picture chosen and
//! copied, and Choose Paper's advice.

mod sheet_view_options {
    use std::cell::Cell;
    use std::rc::Rc;

    use katana_cad::Document;
    use katana_cad::plot::PaperSize;
    use katana_cad::plotting::layout::tiling_area;
    use katana_cad::plotting::section_fit::viewport_stations;
    use katana_cad::plotting::sheet_commands::add_sheet;
    use katana_cad::plotting::sheet_set::{Sheet, Viewport, ViewportKind};
    use katana_cad::plotting::sheet_verbs::run_sheet_verb;
    use katana_commands::{EntityAttributes, create_alignment, create_layer, create_line};
    use katana_entity::{Alignment, Layer};
    use katana_geometry::{AlignmentPi, HorizontalAlignment, Point2};
    use katana_window::sheet_editor::FileAsk;

    use crate::support::{Shown, bx, runner_answering, source_of, viewport};
    use crate::window_support::{Scratch, ok};

    fn add_sheet_with(document: &mut Document, viewports: Vec<Viewport>) {
        let sheet = Sheet {
            id: String::from("s1"),
            name: String::from("TEST"),
            viewports,
            ..Sheet::default()
        };
        add_sheet(document, sheet, None).expect("the sheet is added");
    }

    fn only_viewport(shown: &Shown) -> Viewport {
        shown.document().sheet_set().sheets[0].viewports[0].clone()
    }

    /// A road east from (0, 0) to (1000, 0): chainages 0 to 1000.
    fn road(document: &mut Document) {
        let pi = |x: f64| AlignmentPi {
            point: Point2::new(x, 0.0),
            ..AlignmentPi::default()
        };
        let alignment = Alignment {
            name: String::from("ROAD"),
            horizontal: HorizontalAlignment {
                pis: vec![pi(0.0), pi(1000.0)],
                ..HorizontalAlignment::default()
            },
            ..Alignment::default()
        };
        document
            .execute(create_alignment(alignment))
            .expect("the road is added");
    }

    fn add_layer(document: &mut Document, name: &str) {
        document
            .execute(create_layer(Layer {
                name: String::from(name),
                ..Layer::default()
            }))
            .expect("the layer is added");
    }

    /// An editor shown with `select` selected, collecting what it says.
    fn shown(document: Document, select: Option<&str>) -> Shown {
        let mut shown = Shown::bare(document, source_of());
        if let Some(id) = select {
            shown.canvas_mut().select(id);
            shown.draw();
        }
        shown
    }

    /// A number box given a value and left, as a person tabs out of it.
    fn type_number(shown: &mut Shown, name: &str, value: &str) {
        shown.fill(name, value);
    }

    fn value_of(shown: &Shown, name: &str) -> String {
        shown
            .control(name)
            .unwrap_or_else(|| panic!("no {name}"))
            .value
    }

    fn notes_sheet() -> Document {
        let mut document = Document::new(None);
        add_sheet_with(
            &mut document,
            vec![viewport("vp1", ViewportKind::Notes, bx(30.0, 40.0, 80.0, 80.0))],
        );
        document
    }

    #[test]
    fn the_views_place_on_the_paper_is_typed_as_view_set_rect_in_one_step() {
        let mut shown = shown(notes_sheet(), Some("vp1"));
        assert_eq!(value_of(&shown, "sheetViewportX"), "30.0");
        assert_eq!(value_of(&shown, "sheetViewportY"), "40.0");
        assert_eq!(value_of(&shown, "sheetViewportW"), "50.0");
        assert_eq!(value_of(&shown, "sheetViewportH"), "40.0");
        let steps = shown.steps();

        // Left as it was: nothing written.
        type_number(&mut shown, "sheetViewportX", "30");
        assert_eq!(shown.steps(), steps);
        assert!(shown.messages().is_empty());

        // Wider by 50 mm: the right edge moves, the left stays.
        type_number(&mut shown, "sheetViewportW", "100");
        assert_eq!(only_viewport(&shown).rect, bx(30.0, 40.0, 130.0, 80.0));
        assert_eq!(shown.steps(), steps + 1);
        assert_eq!(shown.document().history().undo_name(), "EDIT_VIEWPORT");
        assert!(shown.said("> VIEW SET vp1 rect=30,40,130,80"));
        assert_eq!(shown.errors(), 0);

        // Off the paper: refused as the typed line is, and nothing changes.
        type_number(&mut shown, "sheetViewportX", "5000");
        assert_eq!(only_viewport(&shown).rect, bx(30.0, 40.0, 130.0, 80.0));
        assert_eq!(shown.steps(), steps + 1);
        assert_eq!(shown.errors(), 1);
        // The panel shows the stored place again.
        assert_eq!(value_of(&shown, "sheetViewportX"), "30.0");

        shown.undo();
        assert_eq!(only_viewport(&shown).rect, bx(30.0, 40.0, 80.0, 80.0));
    }

    #[test]
    fn an_edit_is_handed_to_the_windows_executor_when_there_is_one() {
        let mut shown = shown(notes_sheet(), Some("vp1"));
        runner_answering(&mut shown, ok("view id=vp1"));
        type_number(&mut shown, "sheetViewportH", "60");
        assert_eq!(shown.host.lines(), ["VIEW SET vp1 rect=30,40,80,100"]);
        // The runner, not the editor, did the work: here it did none.
        assert_eq!(only_viewport(&shown).rect, bx(30.0, 40.0, 80.0, 80.0));
        assert!(
            shown.messages().is_empty(),
            "the window logs the line itself"
        );
    }

    #[test]
    fn cross_sections_are_cut_every_interval_over_a_range_or_at_chainages() {
        let mut document = Document::new(None);
        road(&mut document);
        let mut sections = viewport(
            "vp1",
            ViewportKind::CrossSections,
            bx(30.0, 40.0, 300.0, 280.0),
        );
        sections.source.alignment = String::from("ROAD");
        sections.source.stations = vec![500.0];
        sections.source.section_half_width = 20.0;
        add_sheet_with(&mut document, vec![sections]);
        let mut shown = shown(document, Some("vp1"));
        assert_eq!(
            shown.control("sheetSectionsAtChainages").expect("the radio").checked,
            Some(true)
        );
        assert!(!shown.control("sheetSectionInterval").expect("the box").enabled);
        // The chainage range is offered for cross sections now, as for a long section.
        assert!(shown.has_control("sheetChainageFrom"));
        assert!(shown.has_control("sheetChainageTo"));
        let steps = shown.steps();

        // Every 20 m: the list cleared, and with no range the whole road, 0 to 1000 - 51
        // sections, one each 20 m from 0 (viewport_stations).
        shown.press_button("sheetSectionsEvery");
        let cut = only_viewport(&shown).source;
        assert!(cut.stations.is_empty());
        assert_eq!(cut.section_interval, 20.0);
        assert_eq!(cut.chainage_from, 0.0);
        assert_eq!(cut.chainage_to, 1000.0);
        assert_eq!(shown.steps(), steps + 1);
        assert!(shown.said("> VIEW SET vp1 stations=\"\" interval=20 from=0 to=1000"));
        let stations = viewport_stations(&cut);
        assert!(!stations.is_empty());
        assert_eq!(stations[0], 0.0);
        assert_eq!(stations[1], 20.0);

        // The rebuilt panel shows the interval chosen; a new one and a range.
        assert_eq!(
            shown.control("sheetSectionsEvery").expect("the radio").checked,
            Some(true)
        );
        assert!(shown.control("sheetSectionInterval").expect("the box").enabled);
        type_number(&mut shown, "sheetSectionInterval", "50");
        type_number(&mut shown, "sheetChainageFrom", "100");
        type_number(&mut shown, "sheetChainageTo", "300");
        assert_eq!(only_viewport(&shown).source.section_interval, 50.0);
        assert_eq!(
            viewport_stations(&only_viewport(&shown).source),
            vec![100.0, 150.0, 200.0, 250.0, 300.0]
        );
        assert_eq!(shown.steps(), steps + 4);

        // Back to a list: the interval off, the chainages typed.
        shown.press_button("sheetSectionsAtChainages");
        assert!(shown.control("sheetSectionStations").expect("the list").enabled);
        shown.fill("sheetSectionStations", "120, 480;760");
        assert_eq!(only_viewport(&shown).source.stations, vec![120.0, 480.0, 760.0]);
        assert_eq!(only_viewport(&shown).source.section_interval, 0.0);
        assert!(shown.said("> VIEW SET vp1 interval=0 stations=120,480,760"));

        // Words are not chainages: said, and nothing run.
        let before = shown.steps();
        shown.fill("sheetSectionStations", "120, soon");
        assert_eq!(shown.steps(), before);
        assert!(shown.errors() >= 1);
    }

    #[test]
    fn section_views_have_their_hidden_layers_as_plans_do() {
        let mut document = Document::new(None);
        road(&mut document);
        add_layer(&mut document, "TREES");
        add_layer(&mut document, "FENCES");
        let mut profile = viewport("vp1", ViewportKind::LongSection, bx(30.0, 40.0, 300.0, 150.0));
        profile.source.alignment = String::from("ROAD");
        let mut sections = viewport(
            "vp2",
            ViewportKind::CrossSections,
            bx(30.0, 160.0, 300.0, 280.0),
        );
        sections.source.alignment = String::from("ROAD");
        add_sheet_with(&mut document, vec![profile, sections]);
        let mut shown = shown(document, Some("vp1"));
        assert!(shown.has_control("sheetViewportHiddenLayers"));
        let steps = shown.steps();
        shown.press_button("sheetViewportHiddenLayers");
        let dialog = shown
            .editor
            .hidden_layers_dialog()
            .expect("the hidden layers dialog");
        assert!(dialog.set_shown("TREES", false));
        shown.press_in("sheetHiddenLayersDialog", "sheetHiddenLayersOk");
        assert!(only_viewport(&shown).hidden_layers.hides_directly("TREES"));
        assert!(!only_viewport(&shown).hidden_layers.hides_directly("FENCES"));
        assert_eq!(shown.steps(), steps + 1);
        assert!(shown.said("> VIEW SET vp1 hide=\"TREES\""));
        // The button counts them.
        assert_eq!(
            shown.control("sheetViewportHiddenLayers").expect("the button").text,
            "Hidden layers (1)..."
        );

        // The cross sections have the button too; OK with nothing changed writes nothing.
        shown.canvas_mut().select("vp2");
        shown.draw();
        shown.press_button("sheetViewportHiddenLayers");
        assert!(shown.editor.hidden_layers_dialog().is_some());
        shown.press_in("sheetHiddenLayersDialog", "sheetHiddenLayersOk");
        assert_eq!(shown.steps(), steps + 1);
    }

    #[test]
    fn an_image_views_picture_is_chosen_and_copied_in_one_step() {
        let scratch = Scratch::new("view-options-image");
        let image = scratch.path("site photo.png");
        // The PNG signature and a little after it: recognised by its content.
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(b"not really pixels");
        std::fs::write(&image, bytes).expect("the picture is written");
        let mut document = Document::new(None);
        document
            .save_as(&scratch.path("project"))
            .expect("the project is saved");
        let mut picture = viewport("vp1", ViewportKind::Image, bx(30.0, 40.0, 130.0, 120.0));
        picture.text = String::from("old.png");
        add_sheet_with(&mut document, vec![picture]);
        let mut shown = shown(document, Some("vp1"));
        assert!(shown.has_control("sheetImageName"));
        assert!(shown.has_control("sheetImageBrowse"));
        let steps = shown.steps();
        let seen = Rc::new(Cell::new(false));
        {
            let seen = Rc::clone(&seen);
            let chosen = image.to_string_lossy().into_owned();
            shown.editor.choose_file = Some(Box::new(move |_: &FileAsk| {
                // The first file dialog is answered; another is cancelled, so that a choice that
                // did not take fails the test.
                if seen.replace(true) {
                    String::new()
                } else {
                    chosen.clone()
                }
            }));
        }
        shown.press_button("sheetImageBrowse");
        assert!(seen.get());
        assert_eq!(only_viewport(&shown).text, "site_photo.png");
        assert!(
            scratch
                .path("project")
                .join("assets")
                .join("site_photo.png")
                .exists()
        );
        assert_eq!(shown.steps(), steps + 1);
        assert_eq!(shown.errors(), 0);

        // A headless session opens no file dialog: it names the verb instead.
        shown.editor.set_headless(true);
        shown.press_button("sheetImageBrowse");
        assert_eq!(shown.errors(), 1);
        let last = shown.messages().last().cloned().unwrap_or_default();
        assert!(last.contains("VIEW SET vp1 file="), "{last}");
        assert_eq!(shown.steps(), steps + 1);
    }

    #[test]
    fn choose_paper_shows_the_advice_at_a_scale_before_applying_it() {
        // 500 x 300 m drawn by its four sides, in a plan at 1 : 1000 filling the sheet: 520 x 312
        // mm with 4% to spare, which A3 cannot hold and A2 landscape can (the editor's Arrange
        // test works it out).
        let mut document = Document::new(None);
        for (a, b) in [
            (Point2::new(0.0, 0.0), Point2::new(500.0, 0.0)),
            (Point2::new(500.0, 0.0), Point2::new(500.0, 300.0)),
            (Point2::new(500.0, 300.0), Point2::new(0.0, 300.0)),
            (Point2::new(0.0, 300.0), Point2::new(0.0, 0.0)),
        ] {
            document
                .execute(create_line(a, b, EntityAttributes::default()))
                .expect("a side");
        }
        let mut plan = viewport("vp1", ViewportKind::Plan, tiling_area(&Sheet::default()));
        plan.scale = 1000.0;
        add_sheet_with(&mut document, vec![plan]);
        let mut shown = shown(document, None);
        assert!(shown.has_control("sheetChoosePaper"));
        let steps = shown.steps();
        shown.press_button("sheetChoosePaper");
        let dialog = shown.editor.suggest_paper_dialog().expect("Choose Paper");
        assert_eq!(dialog.scale_text(), "As drawn");
        assert!(dialog.advice().starts_with("A2 landscape, "), "{}", dialog.advice());
        assert!(dialog.advice().ends_with("full at 1:1000"), "{}", dialog.advice());

        // At 1 : 2000 the verb's own advice, shown, and nothing changed yet.
        shown.fill_in("sheetSuggestPaperDialog", "sheetSuggestPaperScale", "1:2000");
        let tokens: Vec<String> = ["SHEET", "SUGGESTPAPER", "1", "scale=2000"]
            .iter()
            .map(|word| String::from(*word))
            .collect();
        let reply = run_sheet_verb(shown.document_mut(), &tokens, None)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let at = reply.find("paper=").expect("a paper");
        let rest = &reply[at + 6..];
        let paper = &rest[..rest.find(' ').unwrap_or(rest.len())];
        let advice = String::from(shown.editor.suggest_paper_dialog().expect("Choose Paper").advice());
        assert!(advice.starts_with(&format!("{paper} ")), "{advice}");
        assert_eq!(shown.steps(), steps);
        // A scale that cannot be read cannot be applied.
        shown.fill_in("sheetSuggestPaperDialog", "sheetSuggestPaperScale", "large");
        assert!(!shown.control("sheetSuggestPaperApply").expect("Apply").enabled);

        shown.fill_in("sheetSuggestPaperDialog", "sheetSuggestPaperScale", "As drawn");
        shown.press_in("sheetSuggestPaperDialog", "sheetSuggestPaperApply");
        assert_eq!(shown.steps(), steps + 1);
        assert_eq!(shown.sheet_set().sheets[0].paper, PaperSize::A2);
        assert!(shown.sheet_set().sheets[0].landscape);
        assert!(shown.said("> SHEET SUGGESTPAPER 1 scale=auto apply=on"));
    }
}
