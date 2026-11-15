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
use katana_entity::{EntityId, PropertyValue, TextGeometry};
use katana_geometry::{Box2, Point2};
use katana_window::customisation::{
    ScopeChoice, ScopeFilterView, ScopeFilterWidget, scope_filter_views,
};
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
        assert_eq!(views.items, ["Plan 1 (VIEW 1)", "3D 1 (VIEW 2)"]);
        bench.type_text("gmView", "Plan 1 (VIEW 1)");
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
}
