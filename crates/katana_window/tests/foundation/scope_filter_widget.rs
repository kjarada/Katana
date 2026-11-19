//! The twins of tests/qt_widgets/customisation/test_scope_filter_widget.cpp: the "Apply to" and
//! "Only those that match" controls Global Modify and the utilities dialog share. What is tested:
//! every control carries the name its dialog gives it; the controls read into the ModifyScope and
//! ModifyFilter match_entities takes; every scope and every filter is said in the words of the
//! shared grammar (format_scope_words); and those words, read back by the one parser, take exactly
//! what the controls take - so a dialog's line cannot act on something else than its controls
//! show. Driven by object name through the harness, as a person or the headless driver would, on a
//! small drawing laid out in the fixture's comment; every expectation is worked out by hand from
//! it (copied from the C++).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use katana_cad::global_modify::match_entities;
use katana_cad::layer_overrides::LayerOverrides;
use katana_cad::scope_verbs::{ScopeView, ScopeViewFn, match_scope, parse_scope_words};
use katana_cad::view_set::ViewSet;
use katana_cad::viewport_layout::ViewKind;
use katana_cad::{CommandInterpreter, Document};
use katana_commands::{
    CommandPtr, EntityAttributes, create_layer, create_line, create_point, create_text,
    set_entity_property,
};
use katana_core::{ErrorCode, Result};
use katana_entity::tables::Layer;
use katana_entity::entity::ENTITY_TYPES;
use katana_entity::{EntityId, EntityType, PropertyValue, TextGeometry};
use katana_geometry::{Box2, Point2};
use katana_window::customisation::{
    ScopeChoice, ScopeFilterView, ScopeFilterWidget, scope_filter_views,
};
use katana_window::customisation::scope_filter_widget::kind_words;
use katana_window::drive::{ControlKind, Harness};

mod scope_filter_widget {
    use super::*;

    fn attributes(layer: &str) -> EntityAttributes {
        EntityAttributes {
            layer: String::from(layer),
            style: String::new(),
            color: None,
        }
    }

    // The drawing:
    //
    //   layer          entity    where           property
    //   survey         pA point  (0,0)           code=TREE1
    //   survey         pB point  (10,0)          code=POLE
    //   survey/trees   pC point  (20,0)          code=TREE2
    //   roads          ln line   (0,5)-(30,5)    -
    //   roads          tx text   at (0,10)       -           "CH 100"
    //
    // and two views: Plan 1 (id 1) hides "survey" and shows x -1..12, y -1..6; 3D 1 (id 2) hides
    // nothing and has no area on screen.
    struct Fixture {
        document: Document,
        pa: EntityId,
        pb: EntityId,
        pc: EntityId,
        ln: EntityId,
        tx: EntityId,
        /// The open views, shared with the widget's views function, so a test closes one.
        open: Rc<RefCell<Vec<ScopeFilterView>>>,
    }

    impl Fixture {
        fn new() -> Self {
            let mut document = Document::new(None);
            for name in ["survey", "survey/trees", "roads"] {
                document
                    .execute(create_layer(Layer {
                        name: String::from(name),
                        ..Layer::default()
                    }))
                    .expect("the layer is made");
            }
            let pa = create(
                &mut document,
                create_point(Point2::new(0.0, 0.0), attributes("survey")),
            );
            let pb = create(
                &mut document,
                create_point(Point2::new(10.0, 0.0), attributes("survey")),
            );
            let pc = create(
                &mut document,
                create_point(Point2::new(20.0, 0.0), attributes("survey/trees")),
            );
            let ln = create(
                &mut document,
                create_line(
                    Point2::new(0.0, 5.0),
                    Point2::new(30.0, 5.0),
                    attributes("roads"),
                ),
            );
            let text = TextGeometry {
                position: Point2::new(0.0, 10.0),
                text: String::from("CH 100"),
                ..TextGeometry::default()
            };
            let tx = create(&mut document, create_text(text, attributes("roads")));
            for (id, code) in [(pa, "TREE1"), (pb, "POLE"), (pc, "TREE2")] {
                document
                    .execute(set_entity_property(
                        vec![id],
                        "code",
                        PropertyValue::from(code),
                    ))
                    .expect("the code is set");
            }
            let mut hides_survey = LayerOverrides::default();
            hides_survey.hide("survey");
            let open = vec![
                ScopeFilterView {
                    id: 1,
                    title: String::from("Plan 1"),
                    hidden: Some(Arc::new(hides_survey)),
                    on_screen: Some(Box2::new(Point2::new(-1.0, -1.0), Point2::new(12.0, 6.0))),
                },
                ScopeFilterView {
                    id: 2,
                    title: String::from("3D 1"),
                    hidden: None,
                    on_screen: None,
                },
            ];
            Self {
                document,
                pa,
                pb,
                pc,
                ln,
                tx,
                open: Rc::new(RefCell::new(open)),
            }
        }

        /// The window's answer for VIEW, from the same views the widget lists.
        fn provider(&self) -> impl Fn(Option<u32>) -> Result<ScopeView> + use<> {
            let open = Rc::clone(&self.open);
            move |id| {
                for view in open.borrow().iter() {
                    if id == Some(view.id) {
                        return Ok(ScopeView {
                            id: view.id,
                            layers: view.hidden.as_deref().cloned().unwrap_or_default(),
                            area: view.on_screen,
                            frame: None,
                        });
                    }
                }
                Err(katana_core::Error::new(ErrorCode::NotFound, "no such view"))
            }
        }
    }

    fn create(document: &mut Document, command: CommandPtr) -> EntityId {
        document.execute(command).expect("the entity is made");
        document
            .last_created_entities()
            .first()
            .copied()
            .unwrap_or(0)
    }

    /// A widget named "gm" over the fixture's drawing and views, and the harness it is drawn by.
    struct Bench {
        fixture: Fixture,
        widget: ScopeFilterWidget,
        harness: Harness,
    }

    impl Bench {
        fn new() -> Self {
            let fixture = Fixture::new();
            let mut widget = ScopeFilterWidget::new("gm");
            let open = Rc::clone(&fixture.open);
            widget.views = Some(Box::new(move || open.borrow().clone()));
            widget.reload(fixture.document.model());
            let mut bench = Self {
                fixture,
                widget,
                harness: Harness::new(),
            };
            bench.draw();
            bench
        }

        /// Frames until idle; how many reported an edit.
        fn draw(&mut self) -> usize {
            let mut changes = 0;
            let widget = &mut self.widget;
            self.harness
                .run(|ui| {
                    if widget.ui(ui) {
                        changes += 1;
                    }
                })
                .expect("the widget is drawn");
            changes
        }

        /// A click on the control `name` (a radio button).
        fn press(&mut self, name: &str) -> usize {
            self.harness.press(name);
            self.draw()
        }

        /// A check box ticked or cleared.
        fn tick(&mut self, name: &str, on: bool) -> usize {
            self.harness.fill(name, if on { "on" } else { "off" });
            self.draw()
        }

        /// A field typed into, or a choice made, by its text.
        fn type_text(&mut self, name: &str, text: &str) -> usize {
            self.harness.fill(name, text);
            self.draw()
        }

        fn enabled(&self, name: &str) -> bool {
            self.harness
                .control(name)
                .unwrap_or_else(|| panic!("{name} is drawn"))
                .enabled
        }
    }

    fn words(widget: &ScopeFilterWidget) -> String {
        widget
            .verb_words()
            .unwrap_or_else(|error| panic!("{}", error.describe()))
    }

    fn refusal(widget: &ScopeFilterWidget) -> String {
        match widget.verb_words() {
            Ok(line) => panic!("said as {line}"),
            Err(error) => error.message,
        }
    }

    #[test]
    fn every_control_carries_the_name_its_dialog_gives_it() {
        for prefix in ["globalModify", "utility"] {
            let mut widget = ScopeFilterWidget::new(prefix);
            assert_eq!(widget.object_name(), format!("{prefix}Scope"));
            let mut harness = Harness::new();
            harness
                .run(|ui| {
                    widget.ui(ui);
                })
                .expect("the widget is drawn");
            let drawn = harness.snapshot();
            assert!(drawn.control_anywhere(&format!("{prefix}Scope")).is_some());
            for part in [
                "ScopeGroup",
                "FilterGroup",
                "ScopeSelection",
                "ScopeView",
                "ScopeLayers",
                "ScopeDrawing",
                "View",
                "OnScreen",
                "Layers",
                "Sublayers",
                "TypePoint",
                "TypeLine",
                "TypeArc",
                "TypePolyline",
                "TypeCircle",
                "TypeText",
                "TypeDimension",
                "FilterLayer",
                "FilterStyle",
                "FilterColour",
                "FilterProperty",
                "FilterValue",
                "FilterText",
                "DrawnOnly",
            ] {
                assert!(
                    drawn.control_anywhere(&format!("{prefix}{part}")).is_some(),
                    "{prefix}{part}"
                );
            }
        }
    }

    #[test]
    fn every_geometry_kind_has_a_type_box_and_says_its_name_in_the_words_the_grammar_reads() {
        // The boxes are counted off the variant: the list once stopped at Dimension, so a label, a
        // leader and the drawing system's curve polyline, ellipse and spline could be filtered by
        // TYPE= but not here.
        let mut bench = Bench::new();
        for part in [
            "TypeLabel",
            "TypeLeader",
            "TypeCurvePolyline",
            "TypeEllipse",
            "TypeSpline",
        ] {
            let control = bench
                .harness
                .control(&format!("gm{part}"))
                .unwrap_or_else(|| panic!("gm{part}"));
            assert_eq!(control.kind, ControlKind::Check, "{part}");
        }
        bench.press("gmScopeDrawing");
        bench.tick("gmTypeCurvePolyline", true);
        bench.tick("gmTypeSpline", true);
        let said = words(&bench.widget);
        assert_eq!(said, "DRAWING WHERE TYPE=curvepolyline,spline");
        let tokens = CommandInterpreter::tokenize(&said).expect("the words tokenize");
        let mut at = 0;
        let read = parse_scope_words(&tokens, &mut at)
            .unwrap_or_else(|error| panic!("{said}: {}", error.describe()));
        let filter = bench.widget.filter().expect("the filter reads");
        assert_eq!(
            read.filter.types, filter.types,
            "the words read back as the boxes say"
        );
    }

    #[test]
    fn a_type_box_says_its_kind_in_words_and_keeps_the_name_the_grammar_reads() {
        // The sixth look audit's H: the curve polyline's box read "CurvePolyline", the variant's
        // spelling among words. Its label is words; its name and its TYPE= word are as they were.
        let mut bench = Bench::new();
        let curve = bench.harness.control("gmTypeCurvePolyline").expect("drawn");
        assert_eq!(curve.text, "Curved polyline");
        assert_eq!(
            bench.harness.control("gmTypeLeader").expect("drawn").text,
            "Leader"
        );
        // Counted, the words and the count.
        bench.widget.show_type_counts(&[(EntityType::CurvePolyline, 3)]);
        bench.draw();
        let counted = bench.harness.control("gmTypeCurvePolyline").expect("drawn");
        assert_eq!(counted.text, "Curved polyline (3)");
        // No label is a code's CamelCase.
        for kind in ENTITY_TYPES {
            let words = kind_words(kind);
            assert!(
                !words.chars().skip(1).any(char::is_uppercase),
                "{kind:?}: {words}"
            );
        }
        bench.press("gmScopeDrawing");
        bench.tick("gmTypeCurvePolyline", true);
        assert_eq!(words(&bench.widget), "DRAWING WHERE TYPE=curvepolyline");
    }

    #[test]
    fn a_scope_that_keeps_its_shape_is_drawn_whole_and_greyed_saying_why() {
        // The sixth look audit's G: beside Subsurface Utilities' tabs the unused scope's one line
        // left its column empty, or the tabs moved across it. Kept in shape, it is every control,
        // greyed and refused, with the dialog's words under its heading - as tall as in use but
        // for that line.
        let mut bench = Bench::new();
        bench.widget.keep_shape_when_unused(true);
        bench.widget.set_unused_note("Not used: the services come from the file");
        bench.draw();
        let used = bench.harness.control("gmScope").expect("drawn").rect;
        let widget = &mut bench.widget;
        let mut off = |ui: &mut egui::Ui| {
            ui.add_enabled_ui(false, |ui| {
                widget.ui(ui);
            });
        };
        bench.harness.run(&mut off).expect("the widget is drawn disabled");
        let unused = bench.harness.control("gmScope").expect("recorded").rect;
        assert!(
            unused.height() > used.height() && unused.height() < used.height() + 40.0,
            "kept in shape it is {} high, in use {}",
            unused.height(),
            used.height()
        );
        for name in ["gmScopeDrawing", "gmTypeLine", "gmFilterLayer", "gmDrawnOnly"] {
            let control = bench.harness.control(name).expect("drawn");
            assert!(control.visible && !control.enabled, "{name}: {control:?}");
        }
        bench.harness.fill("gmFilterLayer", "survey/*");
        let refused = bench.harness.run(&mut off).expect_err("a fill of it is refused");
        assert!(refused.contains("is disabled"), "{refused}");
        let widget = &mut bench.widget;
        let texts = painted_texts(&mut bench.harness, |ui: &mut egui::Ui| {
            ui.add_enabled_ui(false, |ui| {
                widget.ui(ui);
            });
        });
        for said in ["Apply to", "Not used: the services come from the file", "The whole drawing"] {
            assert!(texts.iter().any(|text| text == said), "{said}: {texts:?}");
        }
        // Measured as drawn, in use and not, without drawing.
        let widget = &mut bench.widget;
        let mut measured = (0.0, 0.0);
        bench
            .harness
            .run(|ui| {
                measured = (
                    widget.measure(ui, 400.0, true, true),
                    widget.measure(ui, 400.0, true, false),
                );
            })
            .expect("measured");
        assert!((measured.0 - used.height()).abs() <= 1.0, "{measured:?} {used:?}");
        assert!((measured.1 - unused.height()).abs() <= 1.0, "{measured:?} {unused:?}");
    }

    #[test]
    fn only_the_chosen_scopes_own_controls_are_live() {
        let mut bench = Bench::new();
        assert_eq!(bench.widget.choice(), ScopeChoice::Selection);
        assert!(!bench.enabled("gmView"));
        assert!(!bench.enabled("gmLayers"));
        bench.press("gmScopeView");
        assert_eq!(bench.widget.choice(), ScopeChoice::View);
        assert!(bench.enabled("gmView"));
        assert!(bench.enabled("gmOnScreen"));
        assert!(!bench.enabled("gmLayers"));
        bench.widget.set_choice(ScopeChoice::Layers);
        bench.draw();
        assert!(bench.enabled("gmLayers"));
        assert!(bench.enabled("gmSublayers"));
        assert!(!bench.enabled("gmView"));
    }

    #[test]
    fn each_scope_is_said_in_the_words_the_grammar_reads() {
        let mut bench = Bench::new();
        assert_eq!(words(&bench.widget), "SELECTION");
        bench.press("gmScopeDrawing");
        assert_eq!(words(&bench.widget), "DRAWING");

        // The checked layers, in the list's order (names ascending), with or without their
        // sublayers; none ticked is refused, as scope() refuses it.
        bench.press("gmScopeLayers");
        assert!(refusal(&bench.widget).contains("tick at least one layer"));
        bench.widget.tick_layer("survey", true);
        bench.widget.tick_layer("roads", true);
        assert_eq!(words(&bench.widget), "LAYERS roads,survey");
        bench.tick("gmSublayers", false);
        assert_eq!(words(&bench.widget), "LAYERS roads,survey ONLY");

        // A view by its id: what it shows on screen, or its layers anywhere. The list shows the id
        // beside the title, since the number in a title counts only the views of its kind: 3D 1
        // is VIEW 2.
        bench.press("gmScopeView");
        let views = bench
            .harness
            .control("gmView")
            .expect("the view list is drawn");
        // "(view 1)", as Zoom To's and Paper Style Scale's view lists say it (the fourth look
        // audit: the C++'s "(VIEW 1)" here stood beside their "(view 1)").
        assert_eq!(views.items, ["Plan 1 (view 1)", "3D 1 (view 2)"]);
        bench.type_text("gmView", "Plan 1 (view 1)");
        assert_eq!(words(&bench.widget), "VIEW 1 EXTENTS");
        bench.tick("gmOnScreen", true);
        assert_eq!(words(&bench.widget), "VIEW 1");
        // The 3D view has no area on screen: asked for one, refused as scope() refuses it;
        // without, its layers anywhere.
        bench.type_text("gmView", "3D 1 (VIEW 2)");
        assert!(refusal(&bench.widget).contains("only a plan view has an area on screen"));
        assert!(bench.widget.scope().is_err());
        bench.tick("gmOnScreen", false);
        assert_eq!(words(&bench.widget), "VIEW 2 EXTENTS");
        // A view closed since it was chosen names nothing.
        bench.fixture.open.borrow_mut().pop();
        let closed = bench
            .widget
            .verb_words()
            .expect_err("a closed view is no view to name");
        assert_eq!(closed.code, ErrorCode::NotFound);
    }

    #[test]
    fn with_no_workspace_the_view_scope_is_read_but_names_no_view_a_line_can_carry() {
        let mut widget = ScopeFilterWidget::new("gm");
        let document = Document::new(None);
        widget.reload(document.model());
        let mut harness = Harness::new();
        let draw = |harness: &mut Harness, widget: &mut ScopeFilterWidget| {
            harness
                .run(|ui| {
                    widget.ui(ui);
                })
                .expect("the widget is drawn");
        };
        draw(&mut harness, &mut widget);
        let views = harness.control("gmView").expect("the view list is drawn");
        assert_eq!(views.items, ["Whole drawing view"]);
        harness.press("gmScopeView");
        draw(&mut harness, &mut widget);
        // What Global Modify reads: a view that hides nothing of its own.
        let scope = widget.scope().expect("the scope reads");
        assert!(scope.view.is_none());
        assert!(scope.area.is_none());
        let line = widget
            .verb_words()
            .expect_err("no line can name the whole drawing's view");
        assert_eq!(line.code, ErrorCode::InvalidState);
    }

    #[test]
    fn each_filter_is_said_in_the_words_the_grammar_reads() {
        let mut bench = Bench::new();
        bench.press("gmScopeDrawing");
        bench.tick("gmTypePoint", true);
        bench.tick("gmTypeLine", true);
        assert_eq!(words(&bench.widget), "DRAWING WHERE TYPE=point,line");
        bench.tick("gmTypePoint", false);
        bench.tick("gmTypeLine", false);

        bench.type_text("gmFilterLayer", " survey/*, roads ");
        assert_eq!(words(&bench.widget), "DRAWING WHERE LAYER=survey/*,roads");
        bench.type_text("gmFilterLayer", "");
        bench.type_text("gmFilterStyle", "ByLayer");
        assert_eq!(words(&bench.widget), "DRAWING WHERE STYLE=ByLayer");
        bench.type_text("gmFilterStyle", "");
        bench.type_text("gmFilterColour", "#ff8000");
        assert_eq!(words(&bench.widget), "DRAWING WHERE COLOUR=#FF8000");
        bench.type_text("gmFilterColour", "bylayer");
        assert_eq!(words(&bench.widget), "DRAWING WHERE COLOUR=ByLayer");
        bench.type_text("gmFilterColour", "orange");
        assert!(refusal(&bench.widget).contains("the filter's colour: type #RRGGBB or ByLayer"));
        bench.type_text("gmFilterColour", "");

        // A value with no property is a value any property holds, as Global Modify reads it.
        bench.type_text("gmFilterValue", "TREE*");
        assert_eq!(words(&bench.widget), "DRAWING WHERE PROP=:TREE*");
        // A property whose name has a ':' cannot be said: PROP= ends the name at the first one,
        // so the words would take another filter. Refused.
        bench.type_text("gmFilterValue", "");
        bench.type_text("gmFilterProperty", "addr:street");
        let said = refusal(&bench.widget);
        assert!(said.contains("cannot be written as PROP="), "{said}");
        bench.type_text("gmFilterValue", "TREE*");
        bench.type_text("gmFilterProperty", "code");
        assert_eq!(words(&bench.widget), "DRAWING WHERE PROP=code:TREE*");
        bench.type_text("gmFilterValue", "");
        assert_eq!(words(&bench.widget), "DRAWING WHERE PROP=code");
        bench.type_text("gmFilterProperty", "");

        // A word with a blank in it is quoted whole, as the command line reads it.
        bench.type_text("gmFilterText", "CH *");
        assert_eq!(words(&bench.widget), "DRAWING WHERE \"TEXT=CH *\"");
        bench.type_text("gmFilterText", "");
        bench.tick("gmDrawnOnly", true);
        assert_eq!(words(&bench.widget), "DRAWING WHERE DRAWN");

        // All at once, in the grammar's order, after the scope.
        bench.press("gmScopeLayers");
        bench.widget.tick_layer("survey", true);
        bench.tick("gmTypePoint", true);
        bench.type_text("gmFilterLayer", "survey/*");
        bench.type_text("gmFilterStyle", "ByLayer");
        bench.type_text("gmFilterColour", "ByLayer");
        bench.type_text("gmFilterProperty", "code");
        bench.type_text("gmFilterValue", "TREE*");
        bench.type_text("gmFilterText", "*");
        assert_eq!(
            words(&bench.widget),
            "LAYERS survey WHERE TYPE=point LAYER=survey/* STYLE=ByLayer COLOUR=ByLayer \
             PROP=code:TREE* TEXT=* DRAWN"
        );
    }

    // The point of verb_words: the line a dialog runs acts on exactly what its controls show.
    // Each configuration's words are read back by the grammar's one parser and resolved with the
    // window's answer for VIEW, and take the same entities as the controls' own scope and filter.
    #[test]
    fn the_words_take_what_the_controls_take() {
        let mut bench = Bench::new();
        let (pa, pb, pc, ln, tx) = (
            bench.fixture.pa,
            bench.fixture.pb,
            bench.fixture.pc,
            bench.fixture.ln,
            bench.fixture.tx,
        );
        bench.fixture.document.selection_mut().set(vec![pb, ln]);
        let provider = bench.fixture.provider();
        let views: &ScopeViewFn<'_> = &provider;

        let same = |bench: &Bench, expected: &[EntityId], what: &str| {
            let scope = bench
                .widget
                .scope()
                .unwrap_or_else(|error| panic!("{what}: {}", error.describe()));
            let filter = bench
                .widget
                .filter()
                .unwrap_or_else(|error| panic!("{what}: {}", error.describe()));
            let by_controls = match_entities(&bench.fixture.document, &scope, &filter)
                .unwrap_or_else(|error| panic!("{what}: {}", error.describe()));
            assert_eq!(by_controls, expected, "{what}");

            let line = bench
                .widget
                .verb_words()
                .unwrap_or_else(|error| panic!("{what}: {}", error.describe()));
            let tokens = CommandInterpreter::tokenize(&line)
                .unwrap_or_else(|error| panic!("{what}: {}", error.describe()));
            let mut at = 0;
            let parsed = parse_scope_words(&tokens, &mut at)
                .unwrap_or_else(|error| panic!("{what}: {line}: {}", error.describe()));
            assert_eq!(at, tokens.len(), "{what}: every word is the scope's");
            let by_words = match_scope(&bench.fixture.document, &parsed, Some(views))
                .unwrap_or_else(|error| panic!("{what}: {}", error.describe()));
            assert_eq!(by_words.matched, expected, "{what}: {line}");
        };

        same(&bench, &[pb, ln], "the selection");
        bench.press("gmScopeDrawing");
        same(&bench, &[pa, pb, pc, ln, tx], "the drawing");
        bench.tick("gmTypePoint", true);
        bench.type_text("gmFilterProperty", "code");
        bench.type_text("gmFilterValue", "tree*");
        same(&bench, &[pa, pc], "the drawing's trees");
        bench.tick("gmTypePoint", false);
        bench.type_text("gmFilterProperty", "");
        same(&bench, &[pa, pc], "a value any property holds");
        bench.type_text("gmFilterValue", "");

        bench.press("gmScopeLayers");
        bench.widget.tick_layer("survey", true);
        same(&bench, &[pa, pb, pc], "survey and its sublayer");
        bench.tick("gmSublayers", false);
        same(&bench, &[pa, pb], "survey only");

        // Plan 1 hides survey: its layers anywhere are the roads; on screen (x -1..12, y -1..6)
        // the line alone, the text at y 10 being off it.
        bench.press("gmScopeView");
        // Filled in the spelling the list had (a driver's, the utilities' process test's): a
        // choice's fill reads its item in either case.
        bench.type_text("gmView", "Plan 1 (VIEW 1)");
        same(&bench, &[ln, tx], "Plan 1 anywhere");
        bench.tick("gmOnScreen", true);
        same(&bench, &[ln], "Plan 1 on screen");
        bench.type_text("gmFilterText", "CH *");
        same(
            &bench,
            &[],
            "Plan 1 on screen, texts CH *: nothing, which is an answer",
        );
        bench.tick("gmOnScreen", false);
        same(&bench, &[tx], "Plan 1 anywhere, texts CH *");
        bench.type_text("gmFilterText", "");
        bench.type_text("gmView", "3D 1 (VIEW 2)");
        same(&bench, &[pa, pb, pc, ln, tx], "3D 1 anywhere");
    }

    #[test]
    fn an_edit_is_reported_a_reload_is_not_and_keeps_what_is_ticked() {
        let mut bench = Bench::new();
        let mut changes = bench.type_text("gmFilterText", "CH");
        assert_eq!(changes, 1);
        changes = bench.press("gmScopeLayers");
        assert_eq!(changes, 1);
        bench.widget.tick_layer("roads", true);
        changes += bench.draw();
        assert_eq!(changes, 2);

        // A layer made since is listed; roads stays ticked, and nothing is reported for the
        // refill.
        bench
            .fixture
            .document
            .execute(create_layer(Layer {
                name: String::from("kerbs"),
                ..Layer::default()
            }))
            .expect("the layer is made");
        bench.widget.reload(bench.fixture.document.model());
        let changes = bench.draw();
        assert_eq!(changes, 0);
        assert!(bench.widget.layer_names().contains(&String::from("kerbs")));
        assert_eq!(bench.widget.ticked_layers(), ["roads"]);
        assert_eq!(words(&bench.widget), "LAYERS roads WHERE TEXT=CH");
    }

    #[test]
    fn a_workspaces_views_are_offered_with_a_plan_views_area_on_screen() {
        let mut set = ViewSet::new();
        let plan_id = {
            let plan = set.add(ViewKind::Plan);
            plan.plan.center = Point2::new(15.0, 0.0);
            plan.plan.scale = 10.0;
            plan.plan.width_pixels = 200.0;
            plan.plan.height_pixels = 20.0;
            plan.layers.hide("roads");
            plan.id
        };
        let model_id = set.add(ViewKind::Model3D).id;
        let views = scope_filter_views(&set);
        assert_eq!(views.len(), 2);
        let plan = set.find(plan_id).expect("the plan view is open");
        let model = set.find(model_id).expect("the 3D view is open");
        assert_eq!(views[0].id, plan.id);
        assert_eq!(views[0].title, "Plan 1");
        // The C++ points at the view's own layers; here the view's layers are shared as they
        // were read.
        assert_eq!(views[0].hidden.as_deref(), Some(&plan.layers));
        // x 15 -+ 100/10, y 0 -+ 10/10, by hand.
        let on_screen = views[0]
            .on_screen
            .expect("a plan view has an area on screen");
        assert_eq!(on_screen.min, Point2::new(5.0, -1.0));
        assert_eq!(on_screen.max, Point2::new(25.0, 1.0));
        assert_eq!(views[1].id, model.id);
        assert_eq!(views[1].hidden.as_deref(), Some(&model.layers));
        assert!(views[1].on_screen.is_none());
    }

    /// The texts painted in a frame of `draw`.
    fn painted_texts(harness: &mut Harness, draw: impl FnMut(&mut egui::Ui)) -> Vec<String> {
        let output = harness.frame(draw);
        crate::control_look::flat(&output.shapes)
            .into_iter()
            .filter_map(|shape| match shape {
                egui::Shape::Text(text) => Some(String::from(text.galley.text())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_scope_that_does_not_apply_is_one_line_with_every_control_still_recorded_and_refused() {
        // The fourth look audit ("For 10"): drawn in a disabled Ui - Contours' with its clip off,
        // Volumes' with corners for its boundary - the widget was a greyed block some 550 px high,
        // most of its dialog. It is its heading and one muted line; its controls are still read by
        // their names, and a press or a fill of one is refused as disabled, as when they were
        // drawn greyed.
        let mut bench = Bench::new();
        let used = bench.harness.control("gmScope").expect("drawn").rect;
        let widget = &mut bench.widget;
        let mut off = |ui: &mut egui::Ui| {
            ui.add_enabled_ui(false, |ui| {
                widget.ui(ui);
            });
        };
        bench.harness.run(&mut off).expect("the widget is drawn disabled");
        let unused = bench.harness.control("gmScope").expect("still recorded");
        assert!(!unused.enabled);
        assert!(
            unused.rect.height() < used.height() / 4.0,
            "the unused scope is {} high, the scope in use {}",
            unused.rect.height(),
            used.height()
        );
        assert!(
            unused.rect.height() < 3.0 * katana_window::theme::style::CONTROL_HEIGHT,
            "a heading and a line: {}",
            unused.rect.height()
        );
        let texts = painted_texts(&mut bench.harness, &mut off);
        assert!(texts.iter().any(|text| text == "Apply to"), "{texts:?}");
        assert!(
            texts
                .iter()
                .any(|text| text == katana_window::customisation::scope_filter_widget::UNUSED_NOTE),
            "{texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text == "The whole drawing"),
            "no control is painted: {texts:?}"
        );
        for part in ["ScopeGroup", "ScopeDrawing", "View", "Layers", "FilterLayer", "DrawnOnly"] {
            let control = bench
                .harness
                .control(&format!("gm{part}"))
                .unwrap_or_else(|| panic!("gm{part} is recorded"));
            assert!(!control.enabled, "gm{part}");
        }
        assert_eq!(
            bench.harness.control("gmScopeSelection").expect("recorded").checked,
            Some(true),
            "a control's record keeps what it holds"
        );
        bench.harness.press("gmScopeDrawing");
        let refused = bench.harness.run(&mut off).expect_err("a press of it is refused");
        assert!(refused.contains("is disabled"), "{refused}");
        assert_eq!(bench.widget.choice(), ScopeChoice::Selection);
        let widget = &mut bench.widget;
        let mut off = |ui: &mut egui::Ui| {
            ui.add_enabled_ui(false, |ui| {
                widget.ui(ui);
            });
        };
        bench.harness.fill("gmFilterLayer", "survey/*");
        let refused = bench.harness.run(&mut off).expect_err("a fill of it is refused");
        assert!(refused.contains("is disabled"), "{refused}");

        // The dialog says why in its own words.
        bench
            .widget
            .set_unused_note("The corners above bound the volume");
        let widget = &mut bench.widget;
        let texts = painted_texts(&mut bench.harness, |ui: &mut egui::Ui| {
            ui.add_enabled_ui(false, |ui| {
                widget.ui(ui);
            });
        });
        assert!(
            texts
                .iter()
                .any(|text| text == "The corners above bound the volume"),
            "{texts:?}"
        );

        // Enabled again, every control is drawn and takes a press as before.
        bench.draw();
        assert!(bench.enabled("gmScopeDrawing"));
        bench.press("gmScopeDrawing");
        assert_eq!(bench.widget.choice(), ScopeChoice::Drawing);
    }

    #[test]
    fn a_scope_that_picks_a_boundary_is_headed_so_and_not_apply_to() {
        // The fourth look audit: Clip's boundary scope was a second group headed "Apply to" beside
        // the dialog's own. A dialog names the group; unnamed, it is "Apply to".
        let mut bench = Bench::new();
        let widget = &mut bench.widget;
        let texts = painted_texts(&mut bench.harness, |ui: &mut egui::Ui| {
            widget.ui(ui);
        });
        assert!(texts.iter().any(|text| text == "Apply to"), "{texts:?}");
        bench.widget.set_heading("Boundary from");
        assert_eq!(bench.widget.heading_text(), "Boundary from");
        let widget = &mut bench.widget;
        let texts = painted_texts(&mut bench.harness, |ui: &mut egui::Ui| {
            widget.ui(ui);
        });
        assert!(texts.iter().any(|text| text == "Boundary from"), "{texts:?}");
        assert!(!texts.iter().any(|text| text == "Apply to"), "{texts:?}");
        // Not an edit, and the controls keep their names.
        assert_eq!(bench.draw(), 0);
        assert!(bench.harness.control("gmScopeDrawing").is_some());
    }

    #[test]
    fn the_layer_list_is_a_well_whose_scroll_bar_is_drawn_inside_it() {
        // The fourth look audit: the list was a bare scroll area as wide as the widget's column,
        // its bar a stray vertical line at the column's far edge (Grid's at x 407). It is a well -
        // a field's fill under a border - with the list and its bar inside.
        let mut bench = Bench::new();
        bench.widget.set_choice(ScopeChoice::Layers);
        bench.draw();
        let list = bench.harness.control("gmLayers").expect("drawn").rect;
        let widget = &mut bench.widget;
        let output = bench.harness.frame(|ui| {
            widget.ui(ui);
        });
        let input = katana_window::theme::palette::input().to_egui();
        let wells: Vec<egui::Rect> = crate::control_look::flat(&output.shapes)
            .into_iter()
            .filter_map(|shape| match shape {
                egui::Shape::Rect(rect) if rect.fill == input && rect.stroke.width > 0.0 => {
                    Some(rect.rect)
                }
                _ => None,
            })
            .collect();
        // The list is the well: its record's rectangle is the well's, and the bar is drawn in it.
        assert!(
            wells.iter().any(|well| (well.left() - list.left()).abs() <= 1.0
                && (well.right() - list.right()).abs() <= 1.0
                && (well.top() - list.top()).abs() <= 1.0),
            "the list {list:?} is a well: {wells:?}"
        );
    }
}
