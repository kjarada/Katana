// The scope and filter words every verb on drawing data takes (scope_verbs.rs,
// docs/cad.md): what the one parser reads, what it refuses, how the words are
// written back, and what they take from a drawing - through match_entities, as
// Global Modify takes it (tests/cad/test_scope_verbs.cpp). Every expectation
// is worked out by hand from the drawing the fixture lays out.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::Arc;

use katana_cad::scope_verbs::{
    count_by_type, format_scope_words, is_scope_word, match_scope, parse_scope_words, record_value,
    resolve_scope, scope_record, scope_view_of, type_tally,
};
use katana_cad::{
    CommandInterpreter, Document, LayerOverrides, ModifyFilter, ScopeSource, ScopeView,
    ScopeViewFn, ScopeViewProvider, ScopeWords, ViewKind, ViewSet,
};
use katana_commands::{
    EntityAttributes, create_layer, create_line, create_point, create_text, set_entity_property,
    set_entity_visible, update_layer,
};
use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result};
use katana_entity::{Color, EntityId, EntityType, Layer, PropertyValue, TextGeometry};
use katana_geometry::{Box2, Point2};

use crate::{create, done, err, must, ok, strings};

// Splits a line as the interpreter does, so a test types what a person types.
fn words(line: &str) -> Vec<String> {
    match CommandInterpreter::tokenize(line) {
        Ok(tokens) => tokens,
        Err(error) => panic!("{line}: {}", error.describe()),
    }
}

// The words of `line` read from its first, and what is left for the verb.
#[track_caller]
fn parsed_with_rest(line: &str) -> (ScopeWords, String) {
    let all = words(line);
    let mut at = 0;
    let scope = match parse_scope_words(&all, &mut at) {
        Ok(scope) => scope,
        Err(error) => panic!("{line}: {}", error.describe()),
    };
    let mut rest = String::new();
    for word in &all[at..] {
        if !rest.is_empty() {
            rest.push(' ');
        }
        rest.push_str(word);
    }
    (scope, rest)
}

#[track_caller]
fn parsed(line: &str) -> ScopeWords {
    parsed_with_rest(line).0
}

#[track_caller]
fn refusal(line: &str) -> Error {
    let all = words(line);
    let mut at = 0;
    match parse_scope_words(&all, &mut at) {
        Ok(_) => panic!("{line} was read"),
        Err(error) => error,
    }
}

fn on_layer(layer: &str) -> EntityAttributes {
    EntityAttributes {
        layer: String::from(layer),
        ..EntityAttributes::default()
    }
}

fn red() -> Color {
    Color::new(255, 0, 0, 255)
}

// The drawing:
//
//   layer          entity    where           property
//   survey         pA point  (0,0)           code=TREE1
//   survey         pB point  (10,0)          code=POLE
//   survey/trees   pC point  (20,0)          code=TREE2
//   roads          ln line   (0,5)-(30,5)    -
//   roads          tx text   at (0,10)       -           "CH 100"
struct ScopeVerbsTest {
    document: Document,
    pa: EntityId,
    pb: EntityId,
    pc: EntityId,
    ln: EntityId,
    tx: EntityId,
}

impl ScopeVerbsTest {
    fn new() -> Self {
        let mut document = Document::default();
        for name in ["survey", "survey/trees", "roads"] {
            let layer = Layer {
                name: String::from(name),
                ..Layer::default()
            };
            must(&mut document, create_layer(layer));
        }
        let pa = create(
            &mut document,
            create_point(Point2::new(0.0, 0.0), on_layer("survey")),
        );
        let pb = create(
            &mut document,
            create_point(Point2::new(10.0, 0.0), on_layer("survey")),
        );
        let pc = create(
            &mut document,
            create_point(Point2::new(20.0, 0.0), on_layer("survey/trees")),
        );
        let ln = create(
            &mut document,
            create_line(
                Point2::new(0.0, 5.0),
                Point2::new(30.0, 5.0),
                on_layer("roads"),
            ),
        );
        let text = TextGeometry {
            position: Point2::new(0.0, 10.0),
            text: String::from("CH 100"),
            ..TextGeometry::default()
        };
        let tx = create(&mut document, create_text(text, on_layer("roads")));
        for (id, code) in [(pa, "TREE1"), (pb, "POLE"), (pc, "TREE2")] {
            must(
                &mut document,
                set_entity_property(vec![id], "code", PropertyValue::from(code)),
            );
        }
        Self {
            document,
            pa,
            pb,
            pc,
            ln,
            tx,
        }
    }

    #[track_caller]
    fn matched(&self, line: &str, views: ScopeViewProvider<'_>) -> Vec<EntityId> {
        match match_scope(&self.document, &parsed(line), views) {
            Ok(matched) => matched.matched,
            Err(error) => panic!("{line}: {}", error.describe()),
        }
    }

    #[track_caller]
    fn record(&self, line: &str, views: ScopeViewProvider<'_>) -> String {
        match match_scope(&self.document, &parsed(line), views) {
            Ok(matched) => scope_record(&matched),
            Err(error) => panic!("{line}: {}", error.describe()),
        }
    }

    fn colour_of(&self, id: EntityId) -> Option<Color> {
        match self.document.model().entities.find(id) {
            Some(entity) => entity.color,
            None => panic!("no entity with id {id}"),
        }
    }

    fn selected(&self) -> Vec<EntityId> {
        self.document.selection().ids()
    }

    fn run(&mut self, interpreter: &mut CommandInterpreter, line: &str) -> Result<String> {
        interpreter.run(&mut self.document, line)
    }
}

// ---- reading the words ------------------------------------------------------------------

mod scope_words_parse {
    use super::*;

    #[test]
    fn each_scope_word_is_read_and_the_verbs_own_words_are_left_for_it() {
        let (scope, rest) = parsed_with_rest("SELECTION MINCOVER 0.6");
        assert_eq!(scope.source, ScopeSource::Selection);
        assert_eq!(rest, "MINCOVER 0.6");
        assert_eq!(parsed("sel").source, ScopeSource::Selection);
        let (scope, rest) = parsed_with_rest("DRAWING SET COLOUR=#FF0000");
        assert_eq!(scope.source, ScopeSource::Drawing);
        assert_eq!(rest, "SET COLOUR=#FF0000");
        assert_eq!(parsed("all").source, ScopeSource::Drawing);

        let (active, rest) = parsed_with_rest("VIEW SPACING 20");
        assert_eq!(active.source, ScopeSource::View);
        assert!(active.view.is_none());
        assert_eq!(rest, "SPACING 20");
        let (third, rest) = parsed_with_rest("view 3 SPACING 20");
        assert_eq!(third.view, Some(3));
        assert_eq!(rest, "SPACING 20");

        // Corners in either order are one window, min first.
        let (area, rest) = parsed_with_rest("AREA 10,20,0,5 SCHEMA s.csv");
        assert_eq!(area.source, ScopeSource::Area);
        assert_eq!(area.area.min, Point2::new(0.0, 5.0));
        assert_eq!(area.area.max, Point2::new(10.0, 20.0));
        assert_eq!(rest, "SCHEMA s.csv");

        let (layers, rest) = parsed_with_rest("LAYERS survey,roads ONLY DESIGN d.csv");
        assert_eq!(layers.source, ScopeSource::Layers);
        assert_eq!(layers.layers, ["survey", "roads"]);
        assert!(!layers.sublayers);
        assert_eq!(rest, "DESIGN d.csv");
        let one = parsed("layer \"Site Services\"");
        assert_eq!(one.layers, ["Site Services"]);
        assert!(one.sublayers);

        // No scope word is the selection, as MODIFY always read it.
        let (scope, rest) = parsed_with_rest("WHERE DRAWN SET COLOUR=#FF0000");
        assert_eq!(scope.source, ScopeSource::Selection);
        assert_eq!(rest, "SET COLOUR=#FF0000");
        let (scope, rest) = parsed_with_rest("");
        assert_eq!(scope.source, ScopeSource::Selection);
        assert_eq!(rest, "");
    }

    #[test]
    fn every_where_key_is_read_and_the_filter_ends_at_the_first_word_that_is_no_condition() {
        let (scope, rest) = parsed_with_rest(
            "DRAWING WHERE TYPE=point,Line LAYER=survey/*,roads STYLE=Kerb COLOUR=#FF0000 \
             PROP=code:TREE* \"TEXT=CH 1*\" DRAWN MINCOVER 0.6",
        );
        let filter = &scope.filter;
        assert_eq!(
            filter.types,
            BTreeSet::from([EntityType::Point, EntityType::Line])
        );
        assert_eq!(filter.layers, ["survey/*", "roads"]);
        assert_eq!(filter.style.as_deref(), Some("Kerb"));
        assert_eq!(filter.colour, Some(Some(red())));
        assert_eq!(filter.property.as_deref(), Some("code"));
        assert_eq!(filter.property_value.as_deref(), Some("TREE*"));
        assert_eq!(filter.text.as_deref(), Some("CH 1*"));
        assert!(filter.drawn_only);
        assert_eq!(rest, "MINCOVER 0.6");

        // ByLayer is the inner None; COLOR is COLOUR; a property with no value
        // is "carries it"; a second WHERE reads on into the same filter.
        let (by_layer, rest) = parsed_with_rest("WHERE color=bylayer PROP=code where TYPE=text");
        assert_eq!(by_layer.filter.colour, Some(None));
        assert_eq!(by_layer.filter.property.as_deref(), Some("code"));
        assert!(by_layer.filter.property_value.is_none());
        assert_eq!(by_layer.filter.types, BTreeSet::from([EntityType::Text]));
        assert_eq!(rest, "");
    }

    #[test]
    fn what_cannot_be_read_is_refused_naming_the_word() {
        assert!(
            refusal("SELECTION DRAWING")
                .describe()
                .contains("give one scope")
        );
        assert!(
            refusal("LAYERS")
                .describe()
                .contains("LAYERS needs the layers")
        );
        assert!(
            refusal("LAYERS ,")
                .describe()
                .contains("LAYERS needs the layers")
        );
        assert!(
            refusal("DRAWING ONLY")
                .describe()
                .contains("ONLY follows a LAYERS list")
        );
        assert!(refusal("AREA").describe().contains("four numbers"));
        assert!(refusal("AREA 1,2,3").describe().contains("1,2,3"));
        assert!(
            refusal("AREA 1,2,3,4,5")
                .describe()
                .contains("four numbers")
        );
        assert!(refusal("AREA a,b,c,d").describe().contains("a,b,c,d"));
        assert!(
            refusal("AREA 0,0,inf,1")
                .describe()
                .contains("four numbers")
        );
        assert!(
            refusal("VIEW 0")
                .describe()
                .contains("a view id is a whole number from 1")
        );
        assert!(
            refusal("VIEW 99999999999")
                .describe()
                .contains("99999999999")
        );
        assert!(
            refusal("WHERE SHADE=blue")
                .describe()
                .contains("not a WHERE key")
        );
        // A type that is no entity type is entity_type_from_string's refusal: a
        // word that does not read, like the colour below.
        let blob = refusal("WHERE TYPE=blob");
        assert_eq!(blob.code, ErrorCode::ParseFailure);
        // The word as it was typed: a title-cased "Blob" was the parser's own
        // spelling, from when it title-cased the name to match the enumerator.
        assert!(blob.describe().contains("blob"), "{}", blob.describe());
        assert!(refusal("WHERE COLOUR=red").describe().contains("red"));
    }

    #[test]
    fn only_the_scope_words_and_where_begin_a_scope() {
        for word in [
            "SELECTION",
            "sel",
            "Drawing",
            "ALL",
            "view",
            "AREA",
            "LAYERS",
            "layer",
            "where",
        ] {
            assert!(is_scope_word(word), "{word}");
        }
        for word in [
            "schedule.csv",
            "./drawing",
            "MINCOVER",
            "SET",
            "ONLY",
            "DRAWN",
            "TYPE=point",
            "",
        ] {
            assert!(!is_scope_word(word), "{word}");
        }
    }

    #[test]
    fn the_words_written_back_read_as_the_same_scope() {
        let layers = ScopeWords {
            source: ScopeSource::Layers,
            layers: strings(&["Site Services/Located", "roads"]),
            sublayers: false,
            filter: ModifyFilter {
                types: BTreeSet::from([EntityType::Point, EntityType::Polyline]),
                layers: strings(&["survey/*"]),
                style: Some(String::new()),
                colour: Some(None),
                property: Some(String::from("utility.line")),
                property_value: Some(String::from("W 1")),
                text: Some(String::from("CH 100")),
                drawn_only: true,
            },
            ..ScopeWords::default()
        };
        let line = ok(format_scope_words(&layers));
        assert_eq!(
            line,
            "LAYERS \"Site Services/Located,roads\" ONLY WHERE TYPE=point,polyline \
             LAYER=survey/* STYLE= COLOUR=ByLayer \"PROP=utility.line:W 1\" \
             \"TEXT=CH 100\" DRAWN"
        );
        let back = parsed(&line);
        assert_eq!(back.source, layers.source);
        assert_eq!(back.layers, layers.layers);
        assert_eq!(back.sublayers, layers.sublayers);
        assert_eq!(back.filter.types, layers.filter.types);
        assert_eq!(back.filter.layers, layers.filter.layers);
        assert_eq!(back.filter.style, layers.filter.style);
        assert_eq!(back.filter.colour, layers.filter.colour);
        assert_eq!(back.filter.property, layers.filter.property);
        assert_eq!(back.filter.property_value, layers.filter.property_value);
        assert_eq!(back.filter.text, layers.filter.text);
        assert_eq!(back.filter.drawn_only, layers.filter.drawn_only);

        let area = ScopeWords {
            source: ScopeSource::Area,
            area: Box2::new(
                Point2::new(334000.125, 6250000.0),
                Point2::new(334040.0, 6250007.2),
            ),
            filter: ModifyFilter {
                colour: Some(Some(Color::new(0x2F, 0x80, 0xED, 255))),
                ..ModifyFilter::default()
            },
            ..ScopeWords::default()
        };
        let line = ok(format_scope_words(&area));
        assert_eq!(
            line,
            "AREA 334000.125,6250000,334040,6250007.2 WHERE COLOUR=#2F80ED"
        );
        let area_back = parsed(&line);
        assert_eq!(area_back.area.min, area.area.min);
        assert_eq!(area_back.area.max, area.area.max);
        assert_eq!(area_back.filter.colour, area.filter.colour);

        let mut view = ScopeWords {
            source: ScopeSource::View,
            view: Some(4),
            ..ScopeWords::default()
        };
        assert_eq!(ok(format_scope_words(&view)), "VIEW 4");
        view.view = None;
        assert_eq!(ok(format_scope_words(&view)), "VIEW");
        assert_eq!(ok(format_scope_words(&ScopeWords::default())), "SELECTION");

        // What a line cannot say is refused, not mangled.
        let quoted = ScopeWords {
            source: ScopeSource::Drawing,
            filter: ModifyFilter {
                text: Some(String::from("say \"hello\"")),
                ..ModifyFilter::default()
            },
            ..ScopeWords::default()
        };
        assert_eq!(
            err(format_scope_words(&quoted)).code,
            ErrorCode::InvalidArgument
        );
        let mut comma = ScopeWords {
            source: ScopeSource::Layers,
            layers: strings(&["a,b"]),
            ..ScopeWords::default()
        };
        assert_eq!(
            err(format_scope_words(&comma)).code,
            ErrorCode::InvalidArgument
        );
        comma.layers = Vec::new();
        assert_eq!(
            err(format_scope_words(&comma)).code,
            ErrorCode::InvalidArgument
        );
        // PROP= ends a property's name at the first ':', so a name with one
        // would be written as another filter: refused instead.
        let colon = ScopeWords {
            source: ScopeSource::Drawing,
            filter: ModifyFilter {
                property: Some(String::from("addr:street")),
                ..ModifyFilter::default()
            },
            ..ScopeWords::default()
        };
        let colon_line = err(format_scope_words(&colon));
        assert_eq!(colon_line.code, ErrorCode::InvalidArgument);
        assert_eq!(colon_line.context, "addr:street");
        let empty_name = ScopeWords {
            filter: ModifyFilter {
                property: Some(String::new()),
                property_value: Some(String::from("x")),
                ..ModifyFilter::default()
            },
            ..ScopeWords::default()
        };
        assert_eq!(
            err(format_scope_words(&empty_name)).code,
            ErrorCode::InvalidArgument
        );

        // A value with no property is a value of any property: PROP=:pat.
        let value_alone = ScopeWords {
            filter: ModifyFilter {
                property_value: Some(String::from("TREE*")),
                ..ModifyFilter::default()
            },
            ..ScopeWords::default()
        };
        let line = ok(format_scope_words(&value_alone));
        assert_eq!(line, "SELECTION WHERE PROP=:TREE*");
        let value_back = parsed(&line);
        assert!(value_back.filter.property.is_none());
        assert_eq!(value_back.filter.property_value.as_deref(), Some("TREE*"));
    }
}

// ---- what the words take ----------------------------------------------------------------

mod scope_verbs_test {
    use super::*;

    #[test]
    fn the_scopes_take_what_global_modifys_takes() {
        let mut t = ScopeVerbsTest::new();
        t.document.selection_mut().set(vec![t.pb, t.ln]);
        assert_eq!(t.matched("SELECTION", None), [t.pb, t.ln]);
        assert_eq!(t.matched("DRAWING", None), [t.pa, t.pb, t.pc, t.ln, t.tx]);
        assert_eq!(t.matched("LAYERS survey", None), [t.pa, t.pb, t.pc]);
        assert_eq!(t.matched("LAYERS survey ONLY", None), [t.pa, t.pb]);
        assert_eq!(
            t.matched("LAYERS survey/trees,roads", None),
            [t.pc, t.ln, t.tx]
        );
        // The box x 0..8, y 4..6 holds part of the road line and nothing else.
        assert_eq!(t.matched("AREA 8,6,0,4", None), [t.ln]);
        // x -1..12, y -1..1: the two survey points.
        assert_eq!(t.matched("AREA -1,-1,12,1", None), [t.pa, t.pb]);
        assert_eq!(
            t.matched("DRAWING WHERE PROP=code:TREE*", None),
            [t.pa, t.pc]
        );
        // A value any property holds, as Global Modify's controls take it with
        // the property left empty.
        assert_eq!(t.matched("DRAWING WHERE PROP=:TREE*", None), [t.pa, t.pc]);
        assert_eq!(
            t.matched("LAYERS survey WHERE TYPE=point PROP=code:P*", None),
            [t.pb]
        );
        assert_eq!(t.matched("DRAWING WHERE \"TEXT=CH *\"", None), [t.tx]);

        // AREA is what a view hiding nothing of its own shows: an entity switched
        // off is in no view's window.
        must(&mut t.document, set_entity_visible(vec![t.pb], false));
        assert_eq!(t.matched("AREA -1,-1,12,1", None), [t.pa]);
        // Taking nothing is an answer, not a failure.
        assert!(t.matched("AREA 100,100,200,200", None).is_empty());
        // A layer the drawing lacks is match_entities' refusal, by name.
        let missing = err(match_scope(&t.document, &parsed("LAYERS nowhere"), None));
        assert_eq!(missing.code, ErrorCode::NotFound);
        assert_eq!(missing.context, "nowhere");
    }

    #[test]
    fn view_asks_the_window_for_its_layers_and_its_area_and_headless_is_refused_naming_area() {
        let t = ScopeVerbsTest::new();
        // A window of two plan views: 1 hides "survey", 2 hides nothing and
        // shows x 5..25, y -1..1. The active one is 2.
        let asked: RefCell<Vec<Option<u32>>> = RefCell::new(Vec::new());
        let answer = |id: Option<u32>| -> Result<ScopeView> {
            asked.borrow_mut().push(id);
            match id {
                Some(1) => {
                    let mut layers = LayerOverrides::default();
                    layers.hide("survey");
                    Ok(ScopeView {
                        id: 1,
                        layers,
                        ..ScopeView::default()
                    })
                }
                None | Some(2) => Ok(ScopeView {
                    id: 2,
                    area: Some(Box2::new(Point2::new(5.0, -1.0), Point2::new(25.0, 1.0))),
                    ..ScopeView::default()
                }),
                Some(other) => Err(make_error(
                    ErrorCode::NotFound,
                    "no plan view",
                    format!("{other}"),
                )),
            }
        };
        let views: &ScopeViewFn<'_> = &answer;
        // View 1 hides "survey" and with it "survey/trees": the roads are left.
        assert_eq!(t.matched("VIEW 1", Some(views)), [t.ln, t.tx]);
        // The active view: what lies in its visible area.
        assert_eq!(t.matched("VIEW", Some(views)), [t.pb, t.pc]);
        assert_eq!(t.matched("VIEW WHERE PROP=code:TREE*", Some(views)), [t.pc]);
        assert_eq!(*asked.borrow(), [Some(1), None, None]);
        // The provider's refusal is the verb's.
        let unknown = err(match_scope(&t.document, &parsed("VIEW 7"), Some(views)));
        assert_eq!(unknown.code, ErrorCode::NotFound);

        // Headless there is no view: refused, pointing at the window typed in.
        let headless = err(match_scope(&t.document, &parsed("VIEW"), None));
        assert_eq!(headless.code, ErrorCode::InvalidState);
        assert!(
            headless.message.contains("AREA x0,y0,x1,y1"),
            "{}",
            headless.message
        );

        // The resolved scope owns the view's layers: it may outlive the provider's
        // answer, be copied and be moved.
        let resolved = ok(resolve_scope(&parsed("VIEW 1"), Some(views)));
        let copy = resolved.clone();
        let moved = resolved;
        let Some(copied_layers) = &copy.scope.view else {
            panic!("VIEW 1 resolves to that view's layers");
        };
        assert!(copied_layers.hides("survey/trees"));
        match &moved.scope.view {
            Some(moved_layers) => assert!(Arc::ptr_eq(moved_layers, copied_layers)),
            None => panic!("the moved scope keeps the view's layers"),
        }
        assert_eq!(copy.view, Some(1));
    }

    #[test]
    fn view_extents_takes_what_the_view_draws_anywhere_not_only_what_is_on_screen() {
        let t = ScopeVerbsTest::new();
        // Global Modify's "What a view shows" without "Only what is on screen":
        // the view's own hidden layers, and no area.
        let (active, rest) = parsed_with_rest("VIEW EXTENTS SPACING 20");
        assert_eq!(active.source, ScopeSource::View);
        assert!(active.view.is_none());
        assert!(active.extents);
        assert_eq!(rest, "SPACING 20");
        let third = parsed("view 3 extents WHERE DRAWN");
        assert_eq!(third.view, Some(3));
        assert!(third.extents);
        assert!(third.filter.drawn_only);
        assert!(!parsed("VIEW 3").extents);
        // Anywhere else it is refused by the word, as ONLY is.
        assert!(
            refusal("DRAWING EXTENTS")
                .describe()
                .contains("EXTENTS follows VIEW")
        );
        assert!(
            refusal("EXTENTS")
                .describe()
                .contains("EXTENTS follows VIEW")
        );

        // Written back and read as the same.
        let scope = ScopeWords {
            source: ScopeSource::View,
            view: Some(4),
            extents: true,
            ..ScopeWords::default()
        };
        let line = ok(format_scope_words(&scope));
        assert_eq!(line, "VIEW 4 EXTENTS");
        let back = parsed(&line);
        assert_eq!(back.view, scope.view);
        assert!(back.extents);

        // One plan view, hiding nothing, showing x 5..25, y -1..1: pB and pC on
        // screen; all five drawn somewhere.
        let answer = |_: Option<u32>| -> Result<ScopeView> {
            Ok(ScopeView {
                id: 2,
                area: Some(Box2::new(Point2::new(5.0, -1.0), Point2::new(25.0, 1.0))),
                ..ScopeView::default()
            })
        };
        let views: &ScopeViewFn<'_> = &answer;
        assert_eq!(t.matched("VIEW", Some(views)), [t.pb, t.pc]);
        assert_eq!(
            t.matched("VIEW EXTENTS", Some(views)),
            [t.pa, t.pb, t.pc, t.ln, t.tx]
        );
        assert_eq!(
            t.matched("VIEW 2 EXTENTS WHERE TYPE=point", Some(views)),
            [t.pa, t.pb, t.pc]
        );
        // The record says where "on screen" was, or that the area was dropped.
        let on_screen = ok(match_scope(&t.document, &parsed("VIEW"), Some(views)));
        assert_eq!(
            scope_record(&on_screen),
            "scope=view view=2 area=5,-1,25,1 matched=2"
        );
        let anywhere = ok(match_scope(
            &t.document,
            &parsed("VIEW EXTENTS"),
            Some(views),
        ));
        assert_eq!(
            scope_record(&anywhere),
            "scope=view view=2 extents=yes matched=5"
        );
        // Headless it is still the window's word, refused naming AREA.
        let headless = err(match_scope(&t.document, &parsed("VIEW EXTENTS"), None));
        assert!(headless.message.contains("AREA"), "{}", headless.message);
    }

    #[test]
    fn a_workspaces_view_set_answers_view_as_the_window_does() {
        let t = ScopeVerbsTest::new();
        let mut views = ViewSet::new();
        // No plan view open, and none named: refused, naming AREA.
        let none = err(scope_view_of(&views, None));
        assert_eq!(none.code, ErrorCode::InvalidState);
        assert!(
            none.message.contains("AREA x0,y0,x1,y1"),
            "{}",
            none.message
        );

        // A plan view centred on (15, 0) at 10 px a unit in 200 x 20 px shows
        // x 15 -+ 10 and y 0 -+ 1, by hand; it hides the roads. A 3D view hides
        // the survey layer (and so survey/trees).
        let plan_id = {
            let plan = views.add(ViewKind::Plan);
            plan.plan.center = Point2::new(15.0, 0.0);
            plan.plan.scale = 10.0;
            plan.plan.width_pixels = 200.0;
            plan.plan.height_pixels = 20.0;
            plan.layers.hide("roads");
            plan.id
        };
        let model_id = {
            let model = views.add(ViewKind::Model3D);
            model.layers.hide("survey");
            model.id
        };

        let active = ok(scope_view_of(&views, None));
        assert_eq!(active.id, plan_id);
        match active.area {
            Some(area) => {
                assert_eq!(area.min, Point2::new(5.0, -1.0));
                assert_eq!(area.max, Point2::new(25.0, 1.0));
            }
            None => panic!("a plan view answers its visible area"),
        }
        assert!(active.layers.hides("roads"));

        let by_id = ok(scope_view_of(&views, Some(model_id)));
        assert_eq!(by_id.id, model_id);
        assert!(by_id.area.is_none());
        assert!(by_id.layers.hides("survey/trees"));

        let unknown = err(scope_view_of(&views, Some(999)));
        assert_eq!(unknown.code, ErrorCode::NotFound);
        assert_eq!(unknown.context, "999");

        // As the interpreter's provider: on screen pB and pC; anywhere every
        // point, the roads being hidden; the 3D view the roads alone.
        let answer = |id: Option<u32>| scope_view_of(&views, id);
        let provider: &ScopeViewFn<'_> = &answer;
        assert_eq!(t.matched("VIEW", Some(provider)), [t.pb, t.pc]);
        assert_eq!(
            t.matched("VIEW EXTENTS", Some(provider)),
            [t.pa, t.pb, t.pc]
        );
        assert_eq!(
            t.matched(&format!("VIEW {model_id}"), Some(provider)),
            [t.ln, t.tx]
        );
    }

    #[test]
    fn the_record_says_what_the_scope_took() {
        let t = ScopeVerbsTest::new();
        assert_eq!(t.record("DRAWING", None), "scope=drawing matched=5");
        assert_eq!(t.record("SELECTION", None), "scope=selection matched=0");
        assert_eq!(
            t.record("LAYERS survey,roads ONLY WHERE TYPE=point", None),
            "scope=layers layers=survey,roads sublayers=no where=\"TYPE=point\" matched=2"
        );
        assert_eq!(
            t.record("AREA 0,4,8,6", None),
            "scope=area area=0,4,8,6 matched=1"
        );
        assert_eq!(
            t.record("DRAWING WHERE \"TEXT=CH *\" DRAWN", None),
            "scope=drawing where=\"TEXT=CH * DRAWN\" matched=1"
        );
        let answer = |_: Option<u32>| -> Result<ScopeView> {
            Ok(ScopeView {
                id: 3,
                ..ScopeView::default()
            })
        };
        let views: &ScopeViewFn<'_> = &answer;
        assert_eq!(t.record("VIEW", Some(views)), "scope=view view=3 matched=5");
        assert_eq!(record_value("W1"), "W1");
        assert_eq!(record_value("Site Services"), "\"Site Services\"");
        assert_eq!(record_value("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(record_value(""), "\"\"");
    }

    // ---- MODIFY reads them --------------------------------------------------------------

    #[test]
    fn modify_takes_the_area_and_the_windows_view() {
        let mut t = ScopeVerbsTest::new();
        let mut interpreter = CommandInterpreter::new();
        let reply = ok(t.run(&mut interpreter, "MODIFY AREA 0,4,8,6 SET COLOUR=#0000FF"));
        // Led by what the window took, as the UTILITY replies are.
        assert_eq!(
            reply,
            "scope=area area=0,4,8,6 matched=1\n\
             1 entity matched; changing 1 entity. One UNDO restores it."
        );
        assert_eq!(t.colour_of(t.ln), Some(Color::new(0, 0, 255, 255)));
        assert_eq!(t.colour_of(t.tx), None);

        // Headless, VIEW is refused naming AREA, and nothing changes.
        let steps = t.document.history().undo_count();
        let headless = err(t.run(&mut interpreter, "MODIFY VIEW SET COLOUR=#00FF00"));
        assert!(headless.message.contains("AREA"), "{}", headless.message);
        assert_eq!(t.document.history().undo_count(), steps);

        // The window's view: here one hiding the roads, so the points are taken.
        interpreter.set_scope_context(Some(Box::new(|_: Option<u32>| -> Result<ScopeView> {
            let mut layers = LayerOverrides::default();
            layers.hide("roads");
            Ok(ScopeView {
                id: 3,
                layers,
                area: Some(Box2::new(Point2::new(-1.0, -1.0), Point2::new(30.0, 1.0))),
                ..ScopeView::default()
            })
        })));
        // The reply says which view, by the id a line takes, and which part of
        // the drawing it showed when the line ran.
        let reply = ok(t.run(
            &mut interpreter,
            "MODIFY VIEW WHERE PROP=code:TREE* SET COLOUR=#00FF00 PREVIEW",
        ));
        assert_eq!(
            reply,
            "scope=view view=3 area=-1,-1,30,1 where=\"PROP=code:TREE*\" matched=2\n\
             preview: 2 entities matched; changing 2 entities."
        );
        ok(t.run(
            &mut interpreter,
            "GM VIEW WHERE PROP=code:TREE* SET COLOUR=#00FF00",
        ));
        let green = Some(Color::new(0, 255, 0, 255));
        assert_eq!(t.colour_of(t.pa), green);
        assert_eq!(t.colour_of(t.pc), green);
        assert_eq!(t.colour_of(t.pb), None);

        // The scope words' refusals are MODIFY's.
        assert!(
            t.run(
                &mut interpreter,
                "MODIFY SELECTION DRAWING SET COLOUR=#FF0000"
            )
            .is_err()
        );
        assert!(
            t.run(&mut interpreter, "MODIFY AREA 1,2 SET COLOUR=#FF0000")
                .is_err()
        );
        assert!(
            t.run(&mut interpreter, "MODIFY DRAWING WHERE TYPE=point")
                .is_err()
        );
    }

    #[test]
    fn modify_takes_its_own_words_wherever_they_stood_before_the_shared_parser() {
        let mut t = ScopeVerbsTest::new();
        let mut interpreter = CommandInterpreter::new();

        // A WHERE after SET takes the filter up again: the three points.
        let reply = ok(t.run(
            &mut interpreter,
            "MODIFY DRAWING SET COLOUR=#FF0000 WHERE TYPE=point",
        ));
        assert_eq!(
            reply,
            "3 entities matched; changing 3 entities. One UNDO restores it."
        );
        assert_eq!(t.colour_of(t.pa), Some(red()));
        assert_eq!(t.colour_of(t.pc), Some(red()));
        assert_eq!(t.colour_of(t.ln), None);

        // A second SET goes on setting.
        ok(t.run(
            &mut interpreter,
            "MODIFY DRAWING SET COLOUR=#00FF00 SET VISIBLE=yes WHERE TYPE=line",
        ));
        assert_eq!(t.colour_of(t.ln), Some(Color::new(0, 255, 0, 255)));

        // A LAYERS list is the word after LAYERS, even PREVIEW or SET.
        for name in ["preview", "set"] {
            let layer = Layer {
                name: String::from(name),
                ..Layer::default()
            };
            must(&mut t.document, create_layer(layer));
            let point = create(
                &mut t.document,
                create_point(Point2::new(50.0, 50.0), on_layer(name)),
            );
            let reply = match t.run(
                &mut interpreter,
                &format!("MODIFY LAYERS {name} SET COLOUR=#0000FF"),
            ) {
                Ok(reply) => reply,
                Err(error) => panic!("{name}: {}", error.describe()),
            };
            assert_eq!(
                reply, "1 entity matched; changing 1 entity. One UNDO restores it.",
                "{name}"
            );
            assert_eq!(
                t.colour_of(point),
                Some(Color::new(0, 0, 255, 255)),
                "{name}"
            );
        }
        // PREVIEW anywhere else is PREVIEW: nothing changes.
        let steps = t.document.history().undo_count();
        let reply = ok(t.run(
            &mut interpreter,
            "MODIFY PREVIEW LAYERS preview SET COLOUR=#FF0000",
        ));
        assert_eq!(reply, "preview: 1 entity matched; changing 1 entity.");
        assert_eq!(t.document.history().undo_count(), steps);

        // A word in the filter that is no condition is named as one.
        let bad = err(t.run(
            &mut interpreter,
            "MODIFY DRAWING WHERE TYPE point SET COLOUR=#FF0000",
        ));
        assert_eq!(bad.code, ErrorCode::ParseFailure);
        assert_eq!(bad.message, "a WHERE condition is key=value");
        assert_eq!(bad.context, "TYPE");
        let bad = err(t.run(
            &mut interpreter,
            "MODIFY DRAWING SET COLOUR=#FF0000 WHERE TYPE",
        ));
        assert_eq!(bad.context, "TYPE");
        assert_eq!(t.document.history().undo_count(), steps);
    }

    // ---- SELECT reads them --------------------------------------------------------------

    // Selecting by kind across a scope, as Global Modify's Select Matches does (the
    // owner's "select elements by their type such as selecting leaders",
    // 2026-11-03). Every count is read off the drawing laid out above.
    #[test]
    fn select_takes_the_scope_and_filter_and_says_what_it_took_by_kind() {
        let mut t = ScopeVerbsTest::new();
        let mut interpreter = CommandInterpreter::new();

        let reply = ok(t.run(&mut interpreter, "SELECT DRAWING WHERE TYPE=point"));
        assert_eq!(
            reply,
            "scope=drawing where=\"TYPE=point\" matched=3 selected=3 types=point:3"
        );
        assert_eq!(t.selected(), [t.pa, t.pb, t.pc]);

        // Every kind the drawing holds, in the kind byte's order: point 0, line 1,
        // text 5.
        let reply = ok(t.run(&mut interpreter, "SELECT DRAWING"));
        assert_eq!(
            reply,
            "scope=drawing matched=5 selected=5 types=point:3,line:1,text:1"
        );
        assert_eq!(t.selected(), [t.pa, t.pb, t.pc, t.ln, t.tx]);

        // WHERE alone narrows the selection, as it does for every verb.
        t.document.selection_mut().set(vec![t.pb, t.ln]);
        let reply = ok(t.run(&mut interpreter, "SELECT WHERE TYPE=point"));
        assert_eq!(
            reply,
            "scope=selection where=\"TYPE=point\" matched=1 selected=1 types=point:1"
        );
        assert_eq!(t.selected(), [t.pb]);

        let reply = ok(t.run(
            &mut interpreter,
            "SELECT LAYERS survey ONLY WHERE PROP=code:TREE*",
        ));
        assert_eq!(
            reply,
            "scope=layers layers=survey sublayers=no where=\"PROP=code:TREE*\" \
             matched=1 selected=1 types=point:1"
        );
        assert_eq!(t.selected(), [t.pa]);

        // A scope that takes nothing is an answer, and leaves nothing selected.
        let reply = ok(t.run(&mut interpreter, "SELECT AREA 100,100,200,200"));
        assert_eq!(
            reply,
            "scope=area area=100,100,200,200 matched=0 selected=0"
        );
        assert!(t.selected().is_empty());
    }

    #[test]
    fn select_counts_what_it_cannot_select_and_keeps_the_forms_it_had() {
        let mut t = ScopeVerbsTest::new();
        let mut interpreter = CommandInterpreter::new();

        // The line and the text are on a locked layer: matched and counted, not
        // selected, as no other SELECT would select them.
        let mut roads = match t.document.model().layers.find("roads") {
            Some(layer) => layer.clone(),
            None => panic!("no layer roads"),
        };
        roads.locked = true;
        must(&mut t.document, update_layer(roads.clone()));
        let reply = ok(t.run(&mut interpreter, "SELECT DRAWING WHERE TYPE=line,text"));
        assert_eq!(
            reply,
            "scope=drawing where=\"TYPE=line,text\" matched=2 selected=0 \
             unselectable=2 types=line:1,text:1"
        );
        assert!(t.selected().is_empty());
        roads.locked = false;
        done(t.document.execute(update_layer(roads)));

        // ALL alone and LAYER name are SELECT's own, with their reply: LAYER name
        // is that layer alone, where the scope word LAYERS takes survey/trees too.
        assert_eq!(
            t.run(&mut interpreter, "SELECT ALL").unwrap_or_default(),
            "5 selected"
        );
        assert_eq!(
            t.run(&mut interpreter, "SELECT LAYER survey")
                .unwrap_or_default(),
            "2 selected"
        );
        assert_eq!(
            t.run(&mut interpreter, "SELECT LAYERS survey")
                .unwrap_or_default(),
            "scope=layers layers=survey sublayers=yes matched=3 selected=3 types=point:3"
        );
        assert_eq!(
            t.run(&mut interpreter, "SELECT TYPE point")
                .unwrap_or_default(),
            "3 selected"
        );

        // Refused, naming what does not read, with the selection left as it was.
        let before = t.selected();
        let bad = err(t.run(&mut interpreter, "SELECT DRAWING leader"));
        assert_eq!(bad.code, ErrorCode::ParseFailure);
        assert_eq!(bad.context, "leader");
        assert!(
            t.run(&mut interpreter, "SELECT DRAWING WHERE TYPE=blob")
                .is_err()
        );
        // Headless there is no view: VIEW is refused naming AREA.
        let bad = err(t.run(&mut interpreter, "SELECT VIEW WHERE TYPE=point"));
        assert!(bad.describe().contains("AREA"), "{}", bad.describe());
        assert_eq!(t.selected(), before);
    }

    #[test]
    fn the_type_tally_counts_each_kind_in_the_kind_bytes_order_and_skips_what_is_gone() {
        let t = ScopeVerbsTest::new();
        let model = t.document.model();
        // Out of order, with an id no entity has.
        let ids = [t.tx, t.pc, t.ln, t.pa, 9999];
        assert_eq!(
            count_by_type(model, &ids),
            [
                (EntityType::Point, 2),
                (EntityType::Line, 1),
                (EntityType::Text, 1)
            ]
        );
        assert_eq!(type_tally(model, &ids), "point:2,line:1,text:1");
        assert!(count_by_type(model, &[]).is_empty());
        assert_eq!(type_tally(model, &[]), "");
        assert_eq!(type_tally(model, &[9999]), "");
    }
}
