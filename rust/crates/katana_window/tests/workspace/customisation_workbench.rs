//! The twins of tests/qt_widgets/customisation/test_customisation_workbench.cpp: the Format menu's
//! workbench - the menu and toolbar it fills, the managers it opens (non-modal, one of each, kept
//! between uses, gone with it), the context it builds them from, Purge Unused, closing the window
//! over unapplied edits, an editor's commit keeping a kept session kept, and File > Settings.
//!
//! The workbench is driven through its actions (Workbench::trigger), as a click on the menu drives
//! it, and drawn in the harness where a manager's controls are pressed by their object names. The
//! customisation the managers open on is the three hand-written fixtures of tests/data/
//! customisation, loaded by the CUSTOMISE line as a person loads them: 3 + 4 definitions and 11
//! survey code rules, counted by hand from the files.
//!
//! The drawing Purge reads, set up by hand in purge_drawing: linetype fence named only by style
//! Spare; styles Kerb (worn by the one point), Spare (fence; worn by nothing) and Current (worn by
//! nothing, but the current style). So a purge takes Spare and - because Spare goes - fence; it
//! keeps Kerb and Current. 1 style, 1 linetype, 0 hatch patterns.
//!
//! What the C++ answers from inside a question box's exec is answered here by `confirm` (the
//! purge's question) or, for the code manager's own Apply / Discard / Cancel, by pressing its
//! footer's buttons: an egui dialog asks in its frame and is answered at a later frame, so an
//! interactive close over unapplied edits is refused at once and allowed once they are answered.

mod customisation_workbench {
    use std::path::PathBuf;
    use std::rc::Rc;

    use katana_cad::customisation_host::{read_customisation_file, start_customisation};
    use katana_cad::{
        BuiltInCustomisation, CustomisationHost, CustomisationOrigin, Document,
    };
    use katana_commands::{
        EntityAttributes, create_line, create_linetype, create_point, create_style,
    };
    use katana_entity::{Linetype, LinetypeElement, Style, SurveyRule};
    use katana_geometry::Point2;
    use katana_window::customisation::customisation_workbench::{
        CODES_ACTION, CustomisationServices, CustomisationWorkbench, FORMAT_MENU, PURGE_ACTION,
        SETTINGS_ACTION, STYLES_ACTION, SYMBOLS_ACTION,
    };
    use katana_window::drive::Harness;
    use katana_window::drive::action::{MenuItem, ToolBarItem};
    use katana_window::host::{Answer, RecordingHost, ViewAccess};
    use katana_window::shell::Workbench;
    use katana_window::view_workspace::ViewWorkspace;

    use crate::fixture_customisation::{customisation_fixture_file, quoted_file};

    /// A window with a Format menu and toolbar and the window's own Layers action, over a drawing
    /// whose executor records each line and runs it on the drawing's interpreter.
    struct Bench {
        bench: CustomisationWorkbench,
        host: RecordingHost,
        harness: Harness,
        /// The window's views, when a test asks for them.
        views: Option<ViewWorkspace>,
    }

    impl Bench {
        fn new() -> Self {
            Self::built(false)
        }

        fn with_views() -> Self {
            Self::built(true)
        }

        fn built(with_views: bool) -> Self {
            let document = Document::new(None);
            let services = CustomisationServices {
                layers: Some(katana_window::drive::Action::new("formatLayers", "Layers...")),
                has_runner: true,
                ..CustomisationServices::default()
            };
            let mut host = RecordingHost::with_document(document).answering(Answer::Interpreter);
            let mut views = None;
            if with_views {
                let workspace = ViewWorkspace::new(host.document());
                workspace.set_area(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                ));
                host = host
                    .answering(Answer::Custom(workspace.lines().answer()))
                    .with_views(Box::new(workspace.clone()));
                views = Some(workspace);
            }
            Self {
                bench: CustomisationWorkbench::new(services),
                host,
                harness: Harness::new(),
                views,
            }
        }

        fn trigger(&mut self, name: &str) {
            assert!(self.bench.trigger(&mut self.host, name), "{name}");
        }

        /// processEvents: the workbench's dialogs drawn until the window is idle.
        fn draw(&mut self) {
            self.try_draw().unwrap_or_else(|problem| panic!("{problem}"));
        }

        /// The same, saying why an act posted was refused rather than failing.
        fn try_draw(&mut self) -> Result<(), String> {
            let (bench, host) = (&mut self.bench, &mut self.host);
            self.harness.run(|ui| bench.ui(ui, host))
        }

        /// The three fixtures in one CUSTOMISE line, each path quoted.
        fn load_fixture(&mut self) {
            let mut line = String::from("CUSTOMISE");
            for name in ["test_linestyles", "test_survey", "test_symbols"] {
                line += &format!(" {}", quoted_file(&customisation_fixture_file(name)));
            }
            let parts = &mut self.host.parts;
            let loaded = parts.interpreter.run(&mut parts.document, &line);
            assert!(loaded.is_ok(), "{}", loaded.err().map(|e| e.describe()).unwrap_or_default());
            assert_eq!(self.host.document().style_library().size(), 7);
            assert_eq!(self.host.document().survey_map().size(), 11);
        }

        fn log(&self) -> Vec<String> {
            self.host
                .logged()
                .iter()
                .map(|(text, _)| text.clone())
                .collect()
        }
    }

    fn must(document: &mut Document, command: katana_commands::CommandPtr) {
        let done = document.execute(command);
        assert!(done.is_ok(), "{}", done.err().map(|e| e.describe()).unwrap_or_default());
    }

    fn purge_drawing(document: &mut Document) {
        let fence = Linetype {
            name: String::from("fence"),
            pattern: vec![
                LinetypeElement { length: 1.0 },
                LinetypeElement { length: -0.5 },
            ],
            ..Linetype::default()
        };
        must(document, create_linetype(fence));
        for (name, linetype) in [("Kerb", "continuous"), ("Spare", "fence"), ("Current", "continuous")]
        {
            let style = Style {
                name: String::from(name),
                linetype: String::from(linetype),
                ..Style::default()
            };
            must(document, create_style(style));
        }
        let attributes = EntityAttributes {
            style: String::from("Kerb"),
            ..EntityAttributes::default()
        };
        must(document, create_point(Point2::new(0.0, 0.0), attributes));
        assert!(document.set_current_style("Current").is_ok());
    }

    #[test]
    fn the_format_menu_offers_layers_the_managers_global_modify_and_purge_in_that_order() {
        let bench = Bench::new();
        let menus = bench.bench.menus();
        assert_eq!(menus.len(), 1);
        assert_eq!(menus[0].name, FORMAT_MENU);
        let items: Vec<String> = menus[0]
            .items
            .iter()
            .map(|item| match item {
                MenuItem::Action(action) => action.name.clone(),
                MenuItem::Section(_) | MenuItem::Separator => String::from("---"),
                MenuItem::Menu(menu) => menu.name.clone(),
            })
            .collect();
        assert_eq!(
            items,
            [
                "---",
                "formatLayers",
                "formatStyles",
                "formatSymbols",
                "formatSurveyCodes",
                "---",
                "formatGlobalModify",
                "formatPurge"
            ]
        );
        let titles: Vec<String> = menus[0]
            .items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Section(title) => Some(title.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(titles, ["Tables and Libraries", "Across the Drawing"]);
        let bars = bench.bench.tool_bars();
        let tools: Vec<String> = bars[0]
            .items
            .iter()
            .filter_map(|item| match item {
                ToolBarItem::Action(action) => Some(action.name.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            tools,
            [
                "formatStyles",
                "formatSymbols",
                "formatSurveyCodes",
                "formatGlobalModify"
            ]
        );
        assert_eq!(
            CustomisationWorkbench::code_manager_action().name,
            CODES_ACTION,
            "the action the Survey menu shows too"
        );
    }

    #[test]
    fn each_manager_opens_beside_the_drawing_and_the_same_one_comes_back() {
        let mut bench = Bench::new();
        bench.load_fixture();
        for name in [STYLES_ACTION, SYMBOLS_ACTION, CODES_ACTION] {
            bench.trigger(name);
            bench.draw();
            let action = bench
                .bench
                .menus()
                .iter()
                .flat_map(|menu| menu.actions().into_iter().cloned().collect::<Vec<_>>())
                .find(|action| action.name == name)
                .expect("the action");
            let dialog = action.dialog.clone().expect("the action names the dialog it opens");
            // Drawn under the name the action carries: the style manager too, which the workbench
            // names styleManagerDialog as the C++ workbench did (kStyleManagerName).
            let target = bench.harness.target_record(&dialog).expect("shown");
            assert!(!target.modal, "{name} is beside the drawing, not over it");
            match name {
                SYMBOLS_ACTION => {
                    let symbols = bench.bench.symbol_library().expect("made");
                    assert!(symbols.is_open());
                }
                CODES_ACTION => {
                    let codes = bench.bench.code_manager().expect("made");
                    assert!(codes.is_open());
                }
                _ => assert!(bench.bench.style_manager_shown()),
            }
            bench.trigger(name);
            bench.draw();
            assert!(
                bench.harness.target_record(&dialog).is_some(),
                "{name}: the same one, shown again"
            );
        }
        assert!(bench.bench.style_manager().is_some());
        assert!(bench.bench.symbol_library().is_some());
        assert!(bench.bench.code_manager().is_some());
    }

    #[test]
    fn the_code_managers_unapplied_edits_survive_closing_and_reopening_it() {
        // By hand: the fixture map has 11 rules; a duplicate of rule 0 makes 12 in the buffer and
        // none in the drawing's map until Apply.
        let mut bench = Bench::new();
        bench.load_fixture();
        bench.trigger(CODES_ACTION);
        let Bench { bench: work, host, .. } = &mut bench;
        let codes = work.code_manager().expect("made");
        assert!(codes.duplicate_rule(host, 0).is_ok());
        assert!(codes.dirty());
        codes.reject(host);
        bench.trigger(CODES_ACTION);
        let codes = bench.bench.code_manager().expect("the same one");
        assert!(codes.dirty());
        assert_eq!(codes.buffer().size(), 12);
        assert_eq!(bench.host.document().survey_map().size(), 11, "nothing applied");
    }

    #[test]
    fn the_linework_tab_strings_with_the_customisations_control_codes_and_follows_a_change() {
        // By hand. The fixture's rule WM* draws a line, so four points coded WM1 are one string,
        // joined in the order made: (0,0), (10,0), (20,0), (30,0). The third is coded "WM1 GO".
        // While the start code is the default ST, GO is a token nothing knows: ONE line through the
        // four, and one note. Once the customisation's start code is GO, the third point begins a
        // second line: TWO lines of two points, and no note.
        let mut bench = Bench::new();
        bench.load_fixture();
        for line in [
            "POINT 0,0",
            "POINT 10,0",
            "POINT 20,0",
            "POINT 30,0",
            "SELECT ALL",
            "PROP SET code WM1",
            "SELECT 3",
            "PROP SET code \"WM1 GO\"",
            "SELECT NONE",
        ] {
            // The C++'s bench.interpreter.run: straight to the interpreter, not through the
            // window's executor, so not one of the lines it ran.
            let parts = &mut bench.host.parts;
            let ran = parts.interpreter.run(&mut parts.document, line);
            assert!(
                ran.is_ok(),
                "{line}: {}",
                ran.err().map(|e| e.describe()).unwrap_or_default()
            );
        }
        bench.trigger(CODES_ACTION);
        bench.draw();
        bench.harness.target("surveyCodeManagerDialog");
        for name in [
            "lineworkStart",
            "lineworkOrder",
            "lineworkPreview",
            "lineworkExecute",
            "lineworkSummary",
        ] {
            assert!(bench.harness.control(name).is_some(), "no {name}");
        }
        let control = |bench: &Bench, name: &str| {
            bench
                .harness
                .control(name)
                .unwrap_or_else(|| panic!("no {name}"))
        };
        // In the order observed: the points were typed, and carry no number (the C++'s
        // setCurrentIndex(1)).
        bench.harness.fill("lineworkOrder", "In the order observed");
        bench.draw();

        // The summary begins with what the scope took - the drawing's 4 points.
        assert_eq!(control(&bench, "lineworkStart").value, "ST");
        bench.harness.press("lineworkPreview");
        bench.draw();
        assert_eq!(
            control(&bench, "lineworkSummary").text,
            "4 matched. Preview only: 1 lines from 4 points; 0 points not placed; 1 notes. \
             Execute builds them as one undoable step."
        );

        {
            let parts = &mut bench.host.parts;
            let set = parts
                .interpreter
                .run(&mut parts.document, "CUSTOMISE SET linework.start=GO");
            assert!(
                set.is_ok(),
                "{}",
                set.err().map(|e| e.describe()).unwrap_or_default()
            );
        }
        bench.draw();

        // The tab reads the Document: there is no copy of the codes in the window to ask, and
        // nothing is said in the log for a change somebody else's line made - that line was
        // answered where it ran.
        assert_eq!(
            control(&bench, "lineworkStart").value,
            "GO",
            "the tab shows the customisation's code"
        );
        for line in bench.log() {
            assert!(!line.contains("control codes set"), "{line}");
        }
        assert!(bench.host.lines().is_empty(), "a preview runs no line");
        // Execute with no Preview pressed since: it is the LINEWORK line, which strings with the
        // codes the customisation has now, not the plan made with ST.
        bench.harness.press("lineworkExecute");
        bench.draw();
        assert_eq!(bench.host.lines(), ["LINEWORK DRAWING ORDER entity"]);
        assert_eq!(
            bench.host.document().model().entities.size(),
            6,
            "the 4 points and 2 lines"
        );
        assert_eq!(bench.host.document().last_created_entities().len(), 2);
        // In the verb's own first record: the 4 points looked at, 2 lines, and GO no longer a
        // token nothing knows.
        assert_eq!(
            control(&bench, "lineworkSummary").text,
            "Executed as one undoable step: 2 lines drawn. linework scope=drawing matched=4 \
             considered=4 lines=2 unplaced=0 notes=0"
        );
    }

    #[test]
    fn a_code_manager_opened_later_is_built_on_the_customisations_linework_codes() {
        let mut bench = Bench::new();
        {
            let parts = &mut bench.host.parts;
            let set = parts
                .interpreter
                .run(&mut parts.document, "CUSTOMISE SET linework.start=BEGIN linework.end=STOP");
            assert!(set.is_ok());
        }
        // Opened in the same turn: no frame has run since the line.
        bench.trigger(CODES_ACTION);
        bench.draw();
        bench.harness.target("surveyCodeManagerDialog");
        let value = |bench: &Bench, name: &str| bench.harness.control(name).map(|c| c.value);
        assert_eq!(value(&bench, "lineworkStart").as_deref(), Some("BEGIN"));
        assert_eq!(value(&bench, "lineworkEnd").as_deref(), Some("STOP"));
        assert_eq!(
            value(&bench, "lineworkClose").as_deref(),
            Some("CL"),
            "a code the line did not name is as it was"
        );

        // The tab's own Use These Codes sets the CUSTOMISATION's code, by the line a person would
        // type.
        bench.harness.fill("lineworkStart", "MINE");
        bench.draw();
        assert_eq!(
            bench.host.document().customisation_state().linework.start,
            "BEGIN",
            "typed is not set"
        );
        bench.harness.press("lineworkCodesUse");
        bench.draw();
        assert_eq!(bench.host.lines(), ["CUSTOMISE SET linework.start=MINE"]);
        assert_eq!(bench.host.document().customisation_state().linework.start, "MINE");
        assert_eq!(bench.host.document().customisation_state().linework.end, "STOP");
        // What is typed and not yet used stays through a change of the customisation that is not
        // of its control codes ...
        bench.harness.fill("lineworkStart", "DRAFT");
        bench.draw();
        {
            let parts = &mut bench.host.parts;
            assert!(parts.interpreter.run(&mut parts.document, "CUSTOMISE SET auto.codes=off").is_ok());
        }
        bench.draw();
        assert_eq!(value(&bench, "lineworkStart").as_deref(), Some("DRAFT"));
        assert_eq!(bench.host.document().customisation_state().linework.start, "MINE");
        // ... and a change of them takes over.
        {
            let parts = &mut bench.host.parts;
            assert!(parts.interpreter.run(&mut parts.document, "CUSTOMISE SET linework.start=GO").is_ok());
        }
        bench.draw();
        assert_eq!(value(&bench, "lineworkStart").as_deref(), Some("GO"));
        assert_eq!(value(&bench, "lineworkEnd").as_deref(), Some("STOP"));
    }

    #[test]
    fn the_managers_ask_nothing_in_a_headless_session_and_may_in_an_interactive_one() {
        let mut bench = Bench::new();
        bench.trigger(CODES_ACTION);
        bench.trigger(SYMBOLS_ACTION);
        {
            let Bench { bench: work, host, .. } = &mut bench;
            assert!(!work.code_manager().expect("made").interactive(&*host));
            assert!(work.symbol_library().expect("made").headless(&*host));
        }
        bench.host.parts.headless = false;
        bench.trigger(CODES_ACTION);
        bench.trigger(SYMBOLS_ACTION);
        let Bench { bench: work, host, .. } = &mut bench;
        assert!(work.code_manager().expect("made").interactive(&*host));
        assert!(!work.symbol_library().expect("made").headless(&*host));
    }

    #[test]
    fn the_managers_go_with_the_workbench_before_the_document() {
        // Every manager holds the workbench's picture cache: once the workbench has gone, nothing
        // but this test holds it, so none of them is left.
        let mut bench = Bench::new();
        for name in [STYLES_ACTION, SYMBOLS_ACTION, CODES_ACTION] {
            bench.trigger(name);
        }
        let thumbnails = bench.bench.thumbnails();
        assert!(Rc::strong_count(&thumbnails) > 2, "the managers hold it");
        let Bench { bench: work, .. } = bench;
        drop(work);
        assert_eq!(Rc::strong_count(&thumbnails), 1);
    }

    #[test]
    fn showing_what_uses_a_thing_selects_it_and_frames_it_in_the_active_plan_view() {
        // A line from (1000, 1000) to (1010, 1000): framed, the active plan view's centre is the
        // line's middle, (1005, 1000), by that view's Zoom to Selection line through the window's
        // executor.
        let mut bench = Bench::with_views();
        must(
            bench.host.document_mut(),
            create_line(
                Point2::new(1000.0, 1000.0),
                Point2::new(1010.0, 1000.0),
                EntityAttributes::default(),
            ),
        );
        let ids = bench.host.document().last_created_entities();
        assert_eq!(ids.len(), 1);
        let show = bench
            .bench
            .context()
            .select_and_show
            .expect("the workbench frames what it selects");
        show(&mut bench.host, &ids);

        assert_eq!(bench.host.document().selection().ids(), ids);
        let views = bench.views.as_ref().expect("the window's views");
        let active = ViewAccess::active_plan_view(views).expect("a plan view");
        assert_eq!(bench.host.lines(), [format!("ZOOM SELECTION view={active}")]);
        let centre = views.state(active).expect("open").plan.center;
        assert!((centre.x - 1005.0).abs() <= 1e-6, "{}", centre.x);
        assert!((centre.y - 1000.0).abs() <= 1e-6, "{}", centre.y);
    }

    #[test]
    fn purge_unused_deletes_what_nothing_uses_as_one_undo_step_and_names_it() {
        let mut bench = Bench::new();
        purge_drawing(bench.host.document_mut());
        let steps = bench.host.document().history().undo_count();

        bench.trigger(PURGE_ACTION);

        let document = bench.host.document();
        assert_eq!(document.history().undo_count(), steps + 1, "one undo step");
        assert!(document.model().styles.find("Spare").is_none());
        assert!(document.model().linetypes.find("fence").is_none());
        assert!(document.model().styles.find("Kerb").is_some(), "worn");
        assert!(document.model().styles.find("Current").is_some(), "current");
        assert_eq!(
            bench.log().last().map(String::as_str),
            Some(
                "Purge Unused: purged 1 style, 1 linetype and 0 hatch patterns (one Undo \
                 restores them). Styles: Spare. Linetypes: fence."
            )
        );
        assert!(bench.host.document_mut().undo().is_ok());
        assert!(bench.host.document().model().styles.find("Spare").is_some());
        assert!(bench.host.document().model().linetypes.find("fence").is_some());
    }

    #[test]
    fn purge_with_nothing_unused_makes_no_undo_step_and_says_so() {
        let mut bench = Bench::new();
        let steps = bench.host.document().history().undo_count();
        assert!(!bench.bench.purge_unused(&mut bench.host));
        assert_eq!(bench.host.document().history().undo_count(), steps);
        assert_eq!(
            bench.log().last().map(String::as_str),
            Some("Purge Unused: nothing to purge - every style, linetype and hatch pattern is used.")
        );
    }

    #[test]
    fn an_interactive_purge_is_asked_first_and_a_no_deletes_nothing() {
        let mut bench = Bench::new();
        bench.host.parts.headless = false;
        purge_drawing(bench.host.document_mut());
        let asked = Rc::new(std::cell::RefCell::new(String::new()));
        let heard = Rc::clone(&asked);
        bench.bench.confirm = Some(Box::new(move |question| {
            *heard.borrow_mut() = String::from(question);
            false
        }));
        let steps = bench.host.document().history().undo_count();

        assert!(!bench.bench.purge_unused(&mut bench.host));

        assert_eq!(
            *asked.borrow(),
            "Delete 1 style, 1 linetype and 0 hatch patterns that nothing uses? One Undo restores \
             them."
        );
        assert_eq!(bench.host.document().history().undo_count(), steps);
        assert!(bench.host.document().model().styles.find("Spare").is_some());

        bench.bench.confirm = Some(Box::new(|_| true));
        assert!(bench.bench.purge_unused(&mut bench.host));
        assert!(bench.host.document().model().styles.find("Spare").is_none());
    }

    #[test]
    fn the_window_may_close_when_no_manager_holds_unapplied_edits() {
        let mut bench = Bench::new();
        assert!(bench.bench.confirm_close(&mut bench.host), "no manager opened");
        bench.load_fixture();
        bench.trigger(CODES_ACTION);
        assert!(bench.bench.confirm_close(&mut bench.host), "opened, nothing edited");
        assert!(bench.log().is_empty());
    }

    #[test]
    fn a_headless_close_with_unapplied_code_edits_is_refused_and_said() {
        let mut bench = Bench::new();
        bench.load_fixture();
        bench.trigger(CODES_ACTION);
        {
            let Bench { bench: work, host, .. } = &mut bench;
            assert!(work.code_manager().expect("made").duplicate_rule(host, 0).is_ok());
        }

        assert!(!bench.bench.confirm_close(&mut bench.host));

        assert_eq!(
            bench.log().last().map(String::as_str),
            Some(
                "Unapplied Edits: the Survey Code Manager has rule edits that are not on the \
                 drawing, and a headless run has nobody to ask whether to discard them; Apply or \
                 Revert them first."
            )
        );
        {
            let codes = bench.bench.code_manager().expect("made");
            assert!(codes.dirty());
            assert_eq!(codes.buffer().size(), 12);
        }
        assert_eq!(bench.host.document().survey_map().size(), 11);

        {
            let Bench { bench: work, host, .. } = &mut bench;
            work.code_manager().expect("made").revert(host);
        }
        assert!(
            bench.bench.confirm_close(&mut bench.host),
            "reverted: nothing left to lose"
        );
    }

    #[test]
    fn the_code_manager_of_a_window_with_views_names_a_view_on_its_two_action_lines() {
        let mut bench = Bench::with_views();
        let id = bench
            .views
            .as_ref()
            .and_then(ViewAccess::active_plan_view)
            .expect("a plan view");
        must(
            bench.host.document_mut(),
            create_point(Point2::new(0.0, 0.0), EntityAttributes::default()),
        );
        bench.trigger(CODES_ACTION);
        bench.draw();
        bench.harness.target("surveyCodeManagerDialog");

        for (prefix, verb, shown) in [
            ("apply", "CODE", "applyCommand"),
            ("linework", "LINEWORK", "lineworkCommand"),
        ] {
            bench.harness.press(&format!("{prefix}ScopeView"));
            bench.draw();
            bench
                .harness
                .fill(&format!("{prefix}View"), &format!("Plan 1 (VIEW {id})"));
            bench.draw();
            assert_eq!(
                bench.harness.report(shown).as_deref(),
                Ok(format!("{verb} VIEW {id} EXTENTS").as_str()),
                "{prefix}"
            );
            bench.harness.press(&format!("{prefix}OnScreen"));
            bench.draw();
            assert_eq!(
                bench.harness.report(shown).as_deref(),
                Ok(format!("{verb} VIEW {id}").as_str()),
                "{prefix}"
            );
        }

        let before = bench.host.lines().len();
        bench.harness.press("applyExecute");
        bench.draw();
        assert_eq!(bench.host.lines().len(), before + 1);
        assert_eq!(bench.host.lines().last().cloned(), Some(format!("CODE VIEW {id}")));
        let before = bench.host.lines().len();
        bench.harness.press("lineworkExecute");
        bench.draw();
        assert_eq!(bench.host.lines().len(), before + 1);
        assert_eq!(
            bench.host.lines().last().cloned(),
            Some(format!("LINEWORK VIEW {id}"))
        );

        let mut without = Bench::new();
        without.trigger(CODES_ACTION);
        without.draw();
        without.harness.target("surveyCodeManagerDialog");
        without.harness.press("applyScopeView");
        without.draw();
        let views = without.harness.control("applyView").expect("the view list");
        assert_eq!(views.items, ["Whole drawing view"]);
        // ... which no line can name: Execute is refused before anything is run (a press of a
        // button held disabled for it is refused too).
        without.harness.press("applyExecute");
        let _ = without.try_draw();
        assert!(without.host.lines().is_empty());
    }

    #[test]
    fn an_interactive_close_asks_over_the_code_manager_and_cancel_keeps_its_edits() {
        // The manager was closed with its edits kept, so it is hidden: it comes back to ask, in
        // its footer. Cancel leaves it open and the window with it; Discard lets the window go with
        // the drawing's 11 rules; Apply puts the 12 on it.
        let mut bench = Bench::new();
        bench.host.parts.headless = false;
        bench.load_fixture();
        bench.trigger(CODES_ACTION);
        {
            let Bench { bench: work, host, .. } = &mut bench;
            let codes = work.code_manager().expect("made");
            assert!(codes.interactive(&*host));
            assert!(codes.duplicate_rule(host, 0).is_ok());
        }

        assert!(!bench.bench.confirm_close(&mut bench.host), "asked, not yet answered");
        bench.draw();
        bench.harness.target("surveyCodeManagerDialog").press("unappliedCancel");
        bench.draw();
        assert!(!bench.bench.confirm_close(&mut bench.host));
        {
            let codes = bench.bench.code_manager().expect("made");
            assert!(codes.is_open(), "shown to ask, and left open");
            assert!(codes.dirty());
            assert_eq!(codes.buffer().size(), 12);
        }

        bench.draw();
        bench.harness.press("unappliedDiscard");
        bench.draw();
        assert!(bench.bench.confirm_close(&mut bench.host));
        {
            let codes = bench.bench.code_manager().expect("made");
            assert!(!codes.is_open());
            assert!(!codes.dirty());
        }
        assert_eq!(bench.host.document().survey_map().size(), 11);

        {
            let Bench { bench: work, host, .. } = &mut bench;
            assert!(work.code_manager().expect("made").duplicate_rule(host, 0).is_ok());
        }
        assert!(!bench.bench.confirm_close(&mut bench.host));
        bench.draw();
        bench.harness.press("unappliedApply");
        bench.draw();
        assert!(bench.bench.confirm_close(&mut bench.host));
        assert!(!bench.bench.code_manager().expect("made").dirty());
        assert_eq!(bench.host.document().survey_map().size(), 12);
    }

    // ---- an editor's own commit keeps a kept session kept --------------------------------------
    //
    // The session of these tests starts as a program's does (start_customisation through a host),
    // with the fixture test_symbols as its built-in: 4 symbols, no rule. With no kept file on disk
    // the built-in is what starts, and it is "kept" - it is what the next start would give.

    struct KeptBench {
        bench: CustomisationWorkbench,
        host: RecordingHost,
        harness: Harness,
        kept_file: PathBuf,
        folder: PathBuf,
        session: CustomisationHost,
    }

    impl KeptBench {
        fn new(with_kept_file: bool, test: &str) -> Self {
            let folder = std::env::temp_dir().join(format!(
                "katana-window-kept-{}-{test}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&folder);
            std::fs::create_dir_all(&folder).expect("the folder is made");
            let kept_file = folder.join("customisation.json");
            let read = read_customisation_file(&customisation_fixture_file("test_symbols"))
                .unwrap_or_else(|error| panic!("{}", error.describe()));
            let mut session = CustomisationHost {
                built_in: BuiltInCustomisation {
                    customisation: Some(std::sync::Arc::new(read.customisation)),
                    digest: read.digest,
                    ..BuiltInCustomisation::default()
                },
                ..CustomisationHost::default()
            };
            if with_kept_file {
                session.kept_file = kept_file.clone();
            }
            let mut document = Document::new(None);
            let start = start_customisation(&mut document, &session);
            assert_eq!(start.installed, CustomisationOrigin::BuiltIn);
            assert!(document.customisation_state().kept);
            let mut host = RecordingHost::with_document(document).answering(Answer::Interpreter);
            host.parts.interpreter.set_customisation_host(session.clone());
            let kept_text = format!("{}/customisation.json", folder.to_string_lossy().replace('\\', "/"));
            let services = CustomisationServices {
                headless: Some(Box::new(|| true)),
                has_runner: true,
                has_kept_file: Some(Rc::new(move || with_kept_file)),
                has_built_in: Some(Box::new(|| true)),
                kept_file: Some(Box::new(move || {
                    if with_kept_file {
                        kept_text.clone()
                    } else {
                        String::new()
                    }
                })),
                ..CustomisationServices::default()
            };
            Self {
                bench: CustomisationWorkbench::new(services),
                host,
                harness: Harness::new(),
                kept_file,
                folder,
                session,
            }
        }

        fn keep_lines(&self) -> usize {
            self.host
                .lines()
                .iter()
                .filter(|line| *line == "CUSTOMISE KEEP")
                .count()
        }

        /// The line the Symbol Library's Import Definitions runs for the fixture of three
        /// linestyles: the verb, and the file in quotes with '/'.
        fn import_line() -> String {
            format!(
                "CUSTOMISE DEFINITIONS {}",
                quoted_file(&customisation_fixture_file("test_linestyles"))
            )
        }

        fn draw(&mut self) {
            let (bench, host) = (&mut self.bench, &mut self.host);
            self.harness
                .run(|ui| bench.ui(ui, host))
                .unwrap_or_else(|problem| panic!("{problem}"));
        }

        /// The three commits an editor makes, in turn: the code manager's Apply of one new rule,
        /// the Symbol Library's Import Definitions of the fixture's three linestyles, and the
        /// definition editor's Save of a new symbol of two strokes.
        fn commit_through_each_editor(&mut self, after_each: &mut dyn FnMut(&Self, &str)) {
            let rule = SurveyRule {
                key: String::from("KQ*"),
                model: String::from("KEPT"),
                ..SurveyRule::default()
            };
            {
                let Self { bench, host, .. } = self;
                let codes = bench.show_code_manager(host);
                assert!(codes.add_rule(host, rule).is_ok());
                assert!(codes.apply(host).is_ok());
            }
            assert_eq!(self.host.document().survey_map().size(), 1);
            after_each(self, "the Survey Code Manager's Apply");

            {
                let Self { bench, host, .. } = self;
                assert!(bench.import_definitions(
                    host,
                    &customisation_fixture_file("test_linestyles")
                ));
            }
            assert_eq!(
                self.host.document().style_library().size(),
                7,
                "4 symbols and 3 linestyles"
            );
            after_each(self, "the Symbol Library's Import Definitions");

            {
                let Self { bench, host, .. } = self;
                let editor = bench.show_definition_editor(host);
                assert!(editor.new_definition(host, true));
                assert!(editor.type_into(host, "definitionName", "TEST Post"));
                assert!(editor.type_into(
                    host,
                    "definitionStrokes",
                    "[\"move\", 0, 0],\n[\"draw\", 0, 2]"
                ));
                assert!(editor.save(host));
            }
            assert_eq!(self.host.document().style_library().size(), 8);
            after_each(self, "the definition editor's Save");
        }
    }

    impl Drop for KeptBench {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.folder);
        }
    }

    #[test]
    fn an_editors_commit_of_a_kept_session_runs_the_keep_line_once_and_it_stays_kept() {
        let mut bench = KeptBench::new(true, "commit-kept");
        assert!(!bench.kept_file.exists());
        let mut commits = 0;
        bench.commit_through_each_editor(&mut |bench, which| {
            commits += 1;
            // ONE KEEP line more after each commit, and it is the last line run.
            assert_eq!(bench.keep_lines(), commits, "{which}");
            assert_eq!(
                bench.host.lines().last().map(String::as_str),
                Some("CUSTOMISE KEEP"),
                "{which}"
            );
            assert!(bench.host.document().customisation_state().kept, "{which}");
            assert!(bench.kept_file.exists(), "{which}");
        });
        assert_eq!(commits, 3);
        assert_eq!(
            bench.host.lines(),
            [
                String::from("CUSTOMISE KEEP"),
                KeptBench::import_line(),
                String::from("CUSTOMISE KEEP"),
                String::from("CUSTOMISE KEEP")
            ]
        );

        // The definition editor's Delete is a line of its own, and is kept too.
        {
            let KeptBench { bench: work, host, .. } = &mut bench;
            let editor = work.definition_editor().expect("made");
            assert!(editor.remove(host, false), "nothing names TEST Post");
        }
        assert_eq!(bench.host.lines().len(), 6);
        assert_eq!(bench.host.lines()[4], "CUSTOMISE REMOVE \"TEST Post\"");
        assert_eq!(bench.host.lines()[5], "CUSTOMISE KEEP");

        // What the next start reads is what the editors left: the built-in's 4 symbols and the 3
        // imported linestyles, and the one rule.
        let mut next = Document::new(None);
        let start = start_customisation(&mut next, &bench.session);
        assert_eq!(start.installed, CustomisationOrigin::Kept);
        assert_eq!(start.definitions, 7);
        assert_eq!(start.symbols, 4);
        assert_eq!(start.rules, 1);
        assert!(start.problems.is_empty());
    }

    #[test]
    fn a_session_that_was_not_the_kept_one_is_not_kept_by_an_editors_commit() {
        let mut bench = KeptBench::new(true, "commit-loaded");
        {
            let line = format!(
                "CUSTOMISE {}",
                quoted_file(&customisation_fixture_file("test_symbols"))
            );
            let parts = &mut bench.host.parts;
            assert!(parts.interpreter.run(&mut parts.document, &line).is_ok());
        }
        assert!(!bench.host.document().customisation_state().kept);

        let mut commits = 0;
        bench.commit_through_each_editor(&mut |bench, which| {
            commits += 1;
            assert_eq!(bench.keep_lines(), 0, "{which} ran {:?}", bench.host.lines());
            assert!(!bench.host.document().customisation_state().kept, "{which}");
        });
        assert_eq!(commits, 3);
        assert_eq!(bench.host.lines(), [KeptBench::import_line()]);
        assert!(!bench.kept_file.exists());
    }

    #[test]
    fn with_no_kept_file_an_editors_commit_runs_no_line_and_logs_no_refusal() {
        let mut bench = KeptBench::new(false, "commit-unkept");
        let mut commits = 0;
        bench.commit_through_each_editor(&mut |bench, which| {
            commits += 1;
            assert_eq!(bench.keep_lines(), 0, "{which} ran {:?}", bench.host.lines());
        });
        assert_eq!(commits, 3);
        assert_eq!(bench.host.lines(), [KeptBench::import_line()]);
        for (line, _) in bench.host.logged() {
            assert!(!line.contains("KEEP"), "{line}");
        }
    }

    // ---- File > Settings: the workbench's action and its one dialog ---------------------------

    #[test]
    fn the_settings_action_is_files_item_and_opens_the_one_non_modal_dialog_it_names() {
        let mut bench = Bench::new();
        let action = CustomisationWorkbench::settings_action();
        assert_eq!(action.name, SETTINGS_ACTION);
        assert_eq!(action.text, "Settin&gs...");
        // How the headless --dialog finds what the item opened.
        assert_eq!(action.dialog.as_deref(), Some("settingsDialog"));
        assert!(!action.icon.is_empty());
        // File's item: the workbench puts it in neither Format nor its toolbar.
        let in_format = bench
            .bench
            .menus()
            .iter()
            .any(|menu| menu.actions().iter().any(|each| each.name == SETTINGS_ACTION));
        assert!(!in_format);
        let on_bar = bench
            .bench
            .tool_bars()
            .iter()
            .any(|bar| bar.actions().iter().any(|each| each.name == SETTINGS_ACTION));
        assert!(!on_bar);
        assert!(bench.bench.settings().is_none(), "made the first time it is asked for");

        bench.trigger(SETTINGS_ACTION);
        bench.draw();
        let target = bench
            .harness
            .target_record("settingsDialog")
            .expect("the dialog it names");
        assert!(!target.modal, "beside the drawing, not over it");
        assert!(bench.bench.settings().expect("made").is_visible());

        bench.bench.settings().expect("made").close();
        assert!(!bench.bench.settings().expect("made").is_visible());
        bench.trigger(SETTINGS_ACTION);
        assert!(
            bench.bench.settings().expect("made").is_visible(),
            "the same one, shown again"
        );
    }

    #[test]
    fn settings_is_told_when_the_code_manager_holds_unapplied_edits_and_refuses_to_load() {
        // By hand. The session starts as the built-in, test_symbols: no rule. One rule added in
        // the code manager and not applied is in its buffer alone, so Import is refused in the
        // dialog's own words and NO line is run; reverted, the same press runs the line, and the
        // fixture's 11 rules are on the drawing.
        let mut bench = KeptBench::new(false, "settings-import");
        {
            let KeptBench { bench: work, host, .. } = &mut bench;
            let _ = work.show_settings(host);
        }
        let file = customisation_fixture_file("test_survey")
            .to_string_lossy()
            .replace('\\', "/");
        bench.draw();
        bench.harness.target("settingsDialog").fill("settingsImportPath", &file);
        bench.draw();

        // The manager made AFTER the dialog: what is asked is asked at the press.
        {
            let KeptBench { bench: work, host, .. } = &mut bench;
            let codes = work.show_code_manager(host);
            let rule = SurveyRule {
                key: String::from("KQ*"),
                model: String::from("KEPT"),
                ..SurveyRule::default()
            };
            assert!(codes.add_rule(host, rule).is_ok());
            assert!(codes.dirty());
        }

        bench.harness.target("settingsDialog").press("settingsImport");
        bench.draw();
        let refusal = "Import was not run: the Survey Code Manager has rule edits that are not on \
                       the drawing, and its Apply would then put the rules it holds back over what \
                       Import brought. Apply or Revert them first.";
        assert_eq!(bench.harness.report("settingsStatus").as_deref(), Ok(refusal));
        assert!(bench.host.lines().is_empty(), "{:?}", bench.host.lines());
        assert_eq!(
            bench.host.logged().last().map(|(text, _)| text.as_str()),
            Some(refusal),
            "said in the window's log too"
        );
        assert_eq!(bench.host.document().survey_map().size(), 0);

        {
            let KeptBench { bench: work, host, .. } = &mut bench;
            let codes = work.code_manager().expect("made");
            codes.revert(host);
            assert!(!codes.dirty());
        }
        bench.draw();
        bench.harness.target("settingsDialog").press("settingsImport");
        bench.draw();
        assert_eq!(bench.host.lines(), [format!("CUSTOMISE \"{file}\"")]);
        assert_eq!(bench.host.document().survey_map().size(), 11);
    }

    #[test]
    fn settings_opens_the_editors_and_the_code_manager_at_its_linework_tab() {
        let mut bench = Bench::new();
        bench.load_fixture();
        {
            let Bench { bench: work, host, .. } = &mut bench;
            let _ = work.show_settings(host);
        }
        assert!(bench.bench.code_manager().is_none());
        assert!(bench.bench.symbol_library().is_none());
        bench.draw();

        // Edit Linework Codes: the manager, made, shown, and on the tab that is the control codes'
        // one editor - its last, where it opens on its first.
        bench.harness.target("settingsDialog").press("settingsEditLinework");
        bench.draw();
        {
            let codes = bench.bench.code_manager().expect("made");
            assert!(codes.is_open());
            assert_eq!(codes.current_tab(), "lineworkTab");
        }

        // Survey Codes shows the same manager as it was left - here put back on its first tab -
        // and does not move it.
        bench.harness.target("surveyCodeManagerDialog").fill("codeManagerTabs", "Code Table");
        bench.draw();
        {
            let Bench { bench: work, host, .. } = &mut bench;
            work.code_manager().expect("made").reject(host);
        }
        bench.harness.target("settingsDialog").press("settingsOpenCodes");
        bench.draw();
        {
            let codes = bench.bench.code_manager().expect("the same one");
            assert!(codes.is_open());
            assert_eq!(codes.current_tab(), "codeTableTab");
        }

        bench.harness.target("settingsDialog").press("settingsOpenSymbols");
        bench.draw();
        assert!(bench.bench.symbol_library().expect("made").is_open());
        // Nothing was said of a tab that could not be found.
        for line in bench.log() {
            assert!(!line.contains("Linework tab"), "{line}");
        }
    }

    #[test]
    fn settings_shows_and_uses_what_the_windows_host_offers() {
        // A window that tells of no host: no built-in to reset to and no kept file, and the page
        // says so in the dialog's words.
        {
            let mut bench = Bench::new();
            bench.trigger(SETTINGS_ACTION);
            bench.draw();
            bench.harness.target("settingsDialog");
            for name in ["settingsReset", "settingsKeep", "settingsRevert"] {
                assert!(
                    !bench.harness.control(name).expect(name).enabled,
                    "{name}"
                );
            }
            assert_eq!(
                bench.harness.report("settingsActiveKept").as_deref(),
                Ok("Nothing is kept: the next start gives no customisation. The environment \
                    variable KATANA_CUSTOMISATION names a file to keep one in.")
            );
        }
        // One whose host has a built-in and a kept file. The session is the built-in as started,
        // which is kept, so Keep has nothing to do yet.
        let mut bench = KeptBench::new(true, "settings-host");
        {
            let KeptBench { bench: work, host, .. } = &mut bench;
            let _ = work.show_settings(host);
        }
        bench.draw();
        bench.harness.target("settingsDialog");
        assert!(bench.harness.control("settingsReset").expect("Reset").enabled);
        assert!(!bench.harness.control("settingsKeep").expect("Keep").enabled);
        assert!(bench.harness.control("settingsRevert").expect("Revert").enabled);
        let kept = format!(
            "Kept: the next start gives this customisation.\nKept file: {}/customisation.json",
            bench.folder.to_string_lossy().replace('\\', "/")
        );
        assert_eq!(bench.harness.report("settingsActiveKept").as_deref(), Ok(kept.as_str()));

        // An edit made in Settings goes through the window's executor and - the session having
        // been the kept one, with a kept file - is followed by the KEEP line there too.
        assert!(!bench.kept_file.exists());
        assert_eq!(
            bench.harness.control("settingsAutoCodes").and_then(|c| c.checked),
            Some(true),
            "on by default"
        );
        bench.harness.press("settingsAutoCodes");
        bench.draw();
        assert_eq!(
            bench.host.lines(),
            ["CUSTOMISE SET auto.codes=off", "CUSTOMISE KEEP"]
        );
        assert!(
            !bench
                .host
                .document()
                .customisation_state()
                .automation
                .codes_on_survey_import
        );
        assert!(bench.host.document().customisation_state().kept);
        assert!(bench.kept_file.exists());
    }

}
