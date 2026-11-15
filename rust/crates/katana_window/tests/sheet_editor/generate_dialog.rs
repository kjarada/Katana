//! The twins of tests/qt_widgets/plotting/test_generate_dialog.cpp: Generate Sheets builds the
//! GENERATE line a person would type, every option the layout takes written out, and runs it -
//! so the sheets it lays out are the typed line's, compared here with the line typed into a
//! drawing of its own.

mod generate_dialog {
    use katana_cad::plotting::sheet_set::SheetSet;
    use katana_cad::{CommandInterpreter, Document};
    use katana_commands::{EntityAttributes, create_alignment, create_line};
    use katana_entity::Alignment;
    use katana_geometry::{AlignmentPi, Box2, HorizontalAlignment, Point2};
    use katana_window::sheet_editor::sheet_verb_context_for;

    use crate::support::{Shown, near, runner_answering, source_with_document};
    use crate::window_support::ok;

    const DIALOG: &str = "sheetGenerateDialog";

    /// A road east from (0, 0) to (1000, 0), and a site beside it.
    fn road_and_site(document: &mut Document) {
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
        document
            .execute(create_line(
                Point2::new(0.0, 20.0),
                Point2::new(400.0, 220.0),
                EntityAttributes::default(),
            ))
            .expect("the site is added");
    }

    /// The same drawing laid out from the typed line, in a document of its own, given what the
    /// window gives its command line.
    fn typed(line: &str) -> SheetSet {
        let mut twin = Document::new(None);
        road_and_site(&mut twin);
        let mut interpreter = CommandInterpreter::new();
        let provider = source_with_document();
        interpreter.set_sheet_context(Some(Box::new(move |document: &Document| {
            sheet_verb_context_for(document, &provider)
        })));
        if let Err(error) = interpreter.run(&mut twin, line) {
            panic!("{line}\n  -> {}", error.describe());
        }
        SheetSet::clone(&twin.sheet_set())
    }

    fn shown() -> Shown {
        let mut document = Document::new(None);
        road_and_site(&mut document);
        Shown::bare(document, source_with_document())
    }

    /// The dialog, opened as the toolbar opens it.
    fn open(shown: &mut Shown) {
        let _ = shown.editor.open_generate_dialog(&shown.host);
        shown.draw();
    }

    fn choose(shown: &mut Shown, name: &str, text: &str) {
        let items = shown
            .control(name)
            .unwrap_or_else(|| panic!("no {name}"))
            .items;
        assert!(
            items.iter().any(|item| item == text) || name == "sheetGenerateScale",
            "{name} offers no {text}"
        );
        shown.fill_in(DIALOG, name, text);
    }

    fn fill(shown: &mut Shown, name: &str, text: &str) {
        shown.fill_in(DIALOG, name, text);
    }

    fn tick(shown: &mut Shown, name: &str, on: bool) {
        shown.fill_in(DIALOG, name, if on { "on" } else { "off" });
    }

    fn press(shown: &mut Shown, name: &str) {
        shown.press_in(DIALOG, name);
    }

    fn line(shown: &Shown) -> String {
        shown.control("sheetGenerateLine").expect("the line").text
    }

    fn enabled(shown: &Shown, name: &str) -> bool {
        shown
            .control(name)
            .unwrap_or_else(|| panic!("no {name}"))
            .enabled
    }

    fn checked(shown: &Shown, name: &str) -> bool {
        shown
            .control(name)
            .unwrap_or_else(|| panic!("no {name}"))
            .checked
            == Some(true)
    }

    fn can_generate(shown: &Shown) -> bool {
        enabled(shown, "sheetGenerateOk")
    }

    /// OK, as a person clicks it; the dialog closes and goes.
    fn generate(shown: &mut Shown) {
        press(shown, "sheetGenerateOk");
        assert!(shown.editor.generate_dialog().is_none());
    }

    /// Lays the drawing out through the dialog with `choices`, checks the line it made is
    /// `expected` and was run, and that the typed line gives the same sheets.
    fn expect_same_as_typed(choices: impl FnOnce(&mut Shown), expected: &str) {
        let mut shown = shown();
        open(&mut shown);
        choices(&mut shown);
        assert_eq!(line(&shown), expected);
        assert!(can_generate(&shown));
        let steps = shown.steps();
        generate(&mut shown);
        assert_eq!(
            shown.errors(),
            0,
            "{}",
            shown.messages().last().cloned().unwrap_or_default()
        );
        let messages = shown.messages();
        assert!(!messages.is_empty());
        assert_eq!(messages[0], format!("> {expected}"));
        assert_eq!(shown.steps(), steps + 1);
        assert!(!shown.sheet_set().sheets.is_empty());
        assert_eq!(shown.sheet_set(), typed(expected));
    }

    #[test]
    fn every_control_has_an_object_name() {
        let mut shown = shown();
        open(&mut shown);
        assert!(shown.harness.target_record(DIALOG).is_some());
        for name in [
            "sheetGenerateLayout",
            "sheetGeneratePaper",
            "sheetGenerateOrientation",
            "sheetGenerateFrame",
            "sheetGenerateArea",
            "sheetGenerateAreaX0",
            "sheetGenerateAreaY0",
            "sheetGenerateAreaX1",
            "sheetGenerateAreaY1",
            "sheetGenerateScale",
            "sheetGenerateAlignment",
            "sheetGenerateFrom",
            "sheetGenerateTo",
            "sheetGenerateOverlap",
            "sheetGenerateKeyPlan",
            "sheetGenerateDrawnOnly",
            "sheetGenerateNoSections",
            "sheetGenerateEvery",
            "sheetGenerateInterval",
            "sheetGenerateAtStations",
            "sheetGenerateStations",
            "sheetGenerateHalfWidth",
            "sheetGenerateRows",
            "sheetGenerateColumns",
            "sheetGenerateVe",
            "sheetGenerateModel3d",
            "sheetGenerateLegend",
            "sheetGenerateRotate",
            "sheetGenerateReplace",
            "sheetGenerateLine",
            "sheetGenerateOk",
            "sheetGenerateCancel",
        ] {
            assert!(shown.has_control(name), "{name}");
        }
        // The dialog opens on a fit of the drawing, and says so before it is run.
        assert_eq!(
            line(&shown),
            "GENERATE fit paper=A3 landscape frame=on scale=auto model3d=off legend=on"
        );
        press(&mut shown, "sheetGenerateCancel");
    }

    #[test]
    fn strips_along_a_range_on_portrait_paper_are_the_typed_lines_strips() {
        expect_same_as_typed(
            |shown| {
                choose(shown, "sheetGenerateLayout", "Plan strips along an alignment");
                // A strip follows its alignment; the first is chosen for it.
                assert_eq!(
                    shown.control("sheetGenerateAlignment").expect("the alignment").value,
                    "ROAD"
                );
                // At an automatic scale the road goes on one sheet: no range, no overlap.
                assert!(!enabled(shown, "sheetGenerateFrom"));
                assert!(!enabled(shown, "sheetGenerateOverlap"));
                choose(shown, "sheetGenerateScale", "1:500");
                assert!(enabled(shown, "sheetGenerateFrom"));
                fill(shown, "sheetGenerateFrom", "100");
                fill(shown, "sheetGenerateTo", "900");
                choose(shown, "sheetGeneratePaper", "A1");
                choose(shown, "sheetGenerateOrientation", "Portrait");
                tick(shown, "sheetGenerateFrame", false);
                tick(shown, "sheetGenerateReplace", true);
                assert!(!enabled(shown, "sheetGenerateRotate"));
            },
            "GENERATE strips paper=A1 portrait frame=off alignment=\"ROAD\" scale=500 overlap=10 \
             keyplan=on from=100 to=900 replace=on",
        );
    }

    #[test]
    fn a_fit_of_a_window_with_a_snapshot_is_the_typed_lines_fit() {
        expect_same_as_typed(
            |shown| {
                choose(shown, "sheetGenerateArea", "A window");
                assert!(enabled(shown, "sheetGenerateAreaX0"));
                fill(shown, "sheetGenerateAreaX0", "0");
                fill(shown, "sheetGenerateAreaY0", "0");
                fill(shown, "sheetGenerateAreaX1", "200");
                fill(shown, "sheetGenerateAreaY1", "100");
                tick(shown, "sheetGenerateModel3d", true);
                tick(shown, "sheetGenerateLegend", false);
            },
            "GENERATE fit paper=A3 landscape frame=on area=0,0,200,100 scale=auto model3d=on \
             legend=off",
        );
    }

    #[test]
    fn the_current_plan_view_is_offered_only_when_there_is_one() {
        let mut shown = shown();
        open(&mut shown);
        assert!(
            !shown
                .control("sheetGenerateArea")
                .expect("the area")
                .items
                .iter()
                .any(|item| item == "The current plan view")
        );
        press(&mut shown, "sheetGenerateCancel");

        shown.editor.plan_view_area = Some(Box::new(|| {
            Some(Box2::new(Point2::new(-50.0, -25.0), Point2::new(450.0, 275.5)))
        }));
        open(&mut shown);
        choose(&mut shown, "sheetGenerateArea", "The current plan view");
        choose(
            &mut shown,
            "sheetGenerateLayout",
            "Tiles over the drawing, with a key plan",
        );
        choose(&mut shown, "sheetGenerateScale", "1:500");
        fill(&mut shown, "sheetGenerateOverlap", "20");
        tick(&mut shown, "sheetGenerateKeyPlan", false);
        let expected = "GENERATE grid paper=A3 landscape frame=on area=-50,-25,450,275.5 \
                        scale=500 overlap=20 keyplan=off empty=off";
        assert_eq!(line(&shown), expected);
        generate(&mut shown);
        assert_eq!(shown.errors(), 0);
        assert_eq!(shown.sheet_set(), typed(expected));
    }

    #[test]
    fn tiles_with_nothing_drawn_in_them_are_left_out_unless_the_box_is_cleared() {
        // The road and site tiled over -50,-25 .. 450,275.5 at 1:500 with 20 m of overlap: tiles
        // 192.5 x 125 m stepping 172.5 and 105, 3 x 3 of them centred on (200, 125.25). Columns x
        // -68.75..123.75, 103.75..296.25 and 276.25..468.75; rows y 167.75..292.75,
        // 62.75..187.75 and -42.25..82.75. The road (y 0) crosses the bottom row, and the line
        // y = 20 + x / 2 every other tile but the top-left, where it is no higher than 81.9.
        let mut shown = shown();
        let choose_tiles = |shown: &mut Shown| {
            choose(shown, "sheetGenerateLayout", "Tiles over the drawing, with a key plan");
            choose(shown, "sheetGenerateArea", "A window");
            fill(shown, "sheetGenerateAreaX0", "-50");
            fill(shown, "sheetGenerateAreaY0", "-25");
            fill(shown, "sheetGenerateAreaX1", "450");
            fill(shown, "sheetGenerateAreaY1", "275.5");
            choose(shown, "sheetGenerateScale", "1:500");
            fill(shown, "sheetGenerateOverlap", "20");
        };
        open(&mut shown);
        // On by default, and only where there are tiles: a fit at an automatic scale is one sheet;
        // at a fixed one it may be tiles.
        assert!(checked(&shown, "sheetGenerateDrawnOnly"));
        assert!(!enabled(&shown, "sheetGenerateDrawnOnly"));
        choose(&mut shown, "sheetGenerateScale", "1:500");
        assert!(enabled(&shown, "sheetGenerateDrawnOnly"));
        choose_tiles(&mut shown);
        assert!(enabled(&shown, "sheetGenerateDrawnOnly"));
        let left_out = "GENERATE grid paper=A3 landscape frame=on area=-50,-25,450,275.5 \
                        scale=500 overlap=20 keyplan=on empty=off";
        assert_eq!(line(&shown), left_out);
        generate(&mut shown);
        assert_eq!(shown.errors(), 0);
        // The key plan and eight tiles; the top-left is left out.
        assert_eq!(shown.sheet_set().sheets.len(), 9);
        assert_eq!(shown.sheet_set(), typed(left_out));
        let centre = shown.sheet_set().sheets[1].viewports[0].centre;
        assert!(near(centre.x, 200.0, 1e-9));
        assert!(near(centre.y, 230.25, 1e-9));

        // Cleared, every tile is laid out.
        open(&mut shown);
        choose_tiles(&mut shown);
        tick(&mut shown, "sheetGenerateDrawnOnly", false);
        tick(&mut shown, "sheetGenerateReplace", true);
        assert_eq!(
            line(&shown),
            "GENERATE grid paper=A3 landscape frame=on area=-50,-25,450,275.5 scale=500 \
             overlap=20 keyplan=on empty=on replace=on"
        );
        generate(&mut shown);
        assert_eq!(shown.errors(), 0);
        assert_eq!(shown.sheet_set().sheets.len(), 10);
    }

    #[test]
    fn cross_sections_at_chainages_with_an_exaggeration_are_the_typed_lines() {
        expect_same_as_typed(
            |shown| {
                choose(shown, "sheetGenerateLayout", "Cross sections along an alignment");
                // Sections are cut every interval or at chainages, never none.
                assert!(!enabled(shown, "sheetGenerateNoSections"));
                assert!(checked(shown, "sheetGenerateEvery"));
                press(shown, "sheetGenerateAtStations");
                assert!(!enabled(shown, "sheetGenerateInterval"));
                fill(shown, "sheetGenerateStations", "100, 500 900");
                choose(shown, "sheetGenerateVe", "5 x");
                fill(shown, "sheetGenerateRows", "2");
                fill(shown, "sheetGenerateColumns", "1");
            },
            "GENERATE sections paper=A3 landscape frame=on alignment=\"ROAD\" scale=auto \
             stations=100,500,900 halfwidth=20 rows=2 columns=1 ve=5",
        );
    }

    #[test]
    fn a_plan_and_profile_with_sections_is_the_typed_lines_plan_and_profile() {
        expect_same_as_typed(
            |shown| {
                choose(shown, "sheetGenerateLayout", "Plan and profile along an alignment");
                // A profile's plan may have sections after it, but not at a list.
                assert!(!enabled(shown, "sheetGenerateAtStations"));
                press(shown, "sheetGenerateEvery");
                fill(shown, "sheetGenerateInterval", "50");
                fill(shown, "sheetGenerateHalfWidth", "15");
            },
            "GENERATE profile paper=A3 landscape frame=on alignment=\"ROAD\" scale=auto \
             interval=50 halfwidth=15 model3d=off legend=on",
        );
    }

    #[test]
    fn the_register_and_the_plot_frames_take_only_what_their_verbs_take() {
        let mut shown = shown();
        open(&mut shown);
        choose(&mut shown, "sheetGenerateLayout", "One sheet per imported plot frame");
        assert!(!enabled(&shown, "sheetGeneratePaper"));
        assert!(!enabled(&shown, "sheetGenerateScale"));
        assert_eq!(line(&shown), "GENERATE frames frame=on");

        choose(&mut shown, "sheetGenerateLayout", "Drawing register (cover sheet)");
        // A register goes in front of the sheets: it replaces none.
        assert!(!enabled(&shown, "sheetGenerateReplace"));
        choose(&mut shown, "sheetGenerateOrientation", "Portrait");
        tick(&mut shown, "sheetGenerateFrame", false);
        let expected = "GENERATE register paper=A3 portrait frame=off";
        assert_eq!(line(&shown), expected);
        generate(&mut shown);
        assert_eq!(shown.errors(), 0);
        assert_eq!(shown.sheet_set().sheets.len(), 1);
        assert!(!shown.sheet_set().sheets[0].landscape);
        assert_eq!(shown.sheet_set(), typed(expected));
    }

    #[test]
    fn a_line_that_cannot_be_typed_cannot_be_generated() {
        let mut shown = shown();
        open(&mut shown);
        choose(&mut shown, "sheetGenerateLayout", "Cross sections along an alignment");
        press(&mut shown, "sheetGenerateAtStations");
        fill(&mut shown, "sheetGenerateStations", "100, soon");
        assert!(!can_generate(&shown));
        assert_eq!(line(&shown), "list the chainages to cut, separated by commas");
        fill(&mut shown, "sheetGenerateStations", "");
        assert!(!can_generate(&shown));

        // A window with no breadth is no area.
        choose(
            &mut shown,
            "sheetGenerateLayout",
            "Tiles over the drawing, with a key plan",
        );
        choose(&mut shown, "sheetGenerateArea", "A window");
        fill(&mut shown, "sheetGenerateAreaX0", "10");
        fill(&mut shown, "sheetGenerateAreaX1", "10");
        assert!(!can_generate(&shown));
        assert_eq!(line(&shown), "the window needs a width and a height");

        // A strip's range that is not a number.
        choose(&mut shown, "sheetGenerateLayout", "Plan strips along an alignment");
        choose(&mut shown, "sheetGenerateScale", "1:500");
        fill(&mut shown, "sheetGenerateFrom", "start");
        assert!(!can_generate(&shown));
        press(&mut shown, "sheetGenerateCancel");
        assert!(shown.sheet_set().sheets.is_empty());
    }

    #[test]
    fn rotate_is_offered_only_for_a_plan_of_an_area() {
        let mut shown = shown();
        open(&mut shown);
        assert!(enabled(&shown, "sheetGenerateRotate"));
        tick(&mut shown, "sheetGenerateRotate", true);
        assert!(line(&shown).ends_with(" rotate=on"));
        // Along the alignment, or with sections after the plan, it is not turned.
        choose(&mut shown, "sheetGenerateAlignment", "ROAD");
        assert!(!enabled(&shown, "sheetGenerateRotate"));
        assert!(!line(&shown).contains("rotate"));
        assert!(!enabled(&shown, "sheetGenerateArea"));
        choose(&mut shown, "sheetGenerateAlignment", "(none)");
        press(&mut shown, "sheetGenerateEvery");
        assert!(!enabled(&shown, "sheetGenerateRotate"));
        press(&mut shown, "sheetGenerateCancel");
    }

    #[test]
    fn the_line_is_handed_to_the_windows_executor() {
        let mut shown = shown();
        runner_answering(&mut shown, ok("generated 1 sheet: 1"));
        open(&mut shown);
        choose(&mut shown, "sheetGenerateScale", "1:1000");
        generate(&mut shown);
        assert_eq!(
            shown.host.lines(),
            [
                "GENERATE fit paper=A3 landscape frame=on scale=1000 empty=off model3d=off \
                 legend=on"
            ]
        );
        assert!(
            shown.sheet_set().sheets.is_empty(),
            "the runner did the work, and here did none"
        );
        assert!(
            shown.messages().is_empty(),
            "the window logs the line itself"
        );
    }
}
