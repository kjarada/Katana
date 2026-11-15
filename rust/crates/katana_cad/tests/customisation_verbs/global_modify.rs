//! Global Modify: what a scope and a filter take, and the one command that changes the entities,
//! the layers they sit on and the styles they wear (tests/cad/customisation/test_global_modify.cpp).
//! Every expectation is worked out by hand from the drawing GlobalModifyTest::new builds, which
//! the comment on the fixture lays out.

use std::collections::BTreeSet;
use std::sync::Arc;

use katana_cad::global_modify::{match_entities, matches_pattern, plan_global_modify};
use katana_cad::{
    CommandInterpreter, Document, EntityModify, GlobalModify, GlobalModifyPlan, LayerModify,
    LayerOverrides, ModifyFilter, ModifyScope, ScopeKind, StyleModify, SymbolModify,
};
use katana_commands::{
    CommandPtr, EntityAttributes, create_layer, create_line, create_point, create_style,
    create_text, set_entity_property, set_entity_style, set_entity_visible, update_layer,
};
use katana_core::ErrorCode;
use katana_entity::{
    Color, Entity, EntityId, EntityType, Geometry, Layer, PropertyValue, Style, TextGeometry,
};
use katana_geometry::{Box2, Point2};

use crate::must;

const RED: Color = Color::new(255, 0, 0, 255);
const BLUE: Color = Color::new(0, 0, 255, 255);

fn attributes(layer: &str, style: &str, color: Option<Color>) -> EntityAttributes {
    EntityAttributes {
        layer: String::from(layer),
        style: String::from(style),
        color,
    }
}

fn text_value(text: &str) -> PropertyValue {
    PropertyValue::Text(String::from(text))
}

#[track_caller]
fn add_layer(document: &mut Document, name: &str) {
    let layer = Layer {
        name: String::from(name),
        ..Layer::default()
    };
    must(document.execute(create_layer(layer)));
}

#[track_caller]
fn create(document: &mut Document, command: CommandPtr) -> EntityId {
    must(document.execute(command));
    document
        .last_created_entities()
        .first()
        .copied()
        .unwrap_or(0)
}

// The drawing:
//
//   layer             entity    where         style   colour  property
//   survey            pa point  (0,0)         Tree    -       code=TREE1
//   survey            pb point  (10,0)        -       red     code=POLE
//   survey/trees      pc point  (20,0)        Tree    -       code=TREE2
//   roads             ln line   (0,5)-(30,5)  Kerb    -       -
//   roads             tx text   at (0,10)     -       -       -       "CH 100"
//   locked            lk line   (0,20)-(5,20) -       -       -       (layer locked)
//
// Styles Tree (symbol "tree") and Kerb (linetype continuous); a line qn on
// "survey" wearing Tree too, so a Tree definition change reaches beyond
// three selected points.
struct GlobalModifyTest {
    document: Document,
    pa: EntityId,
    pb: EntityId,
    pc: EntityId,
    ln: EntityId,
    tx: EntityId,
    lk: EntityId,
    qn: EntityId,
}

impl GlobalModifyTest {
    #[track_caller]
    fn new() -> Self {
        let mut document = Document::default();
        for name in ["survey", "survey/trees", "roads", "locked"] {
            add_layer(&mut document, name);
        }
        let tree = Style {
            name: String::from("Tree"),
            symbol: String::from("tree"),
            ..Style::default()
        };
        must(document.execute(create_style(tree)));
        let kerb = Style {
            name: String::from("Kerb"),
            ..Style::default()
        };
        must(document.execute(create_style(kerb)));

        let pa = create(
            &mut document,
            create_point(Point2::new(0.0, 0.0), attributes("survey", "Tree", None)),
        );
        let pb = create(
            &mut document,
            create_point(Point2::new(10.0, 0.0), attributes("survey", "", Some(RED))),
        );
        let pc = create(
            &mut document,
            create_point(
                Point2::new(20.0, 0.0),
                attributes("survey/trees", "Tree", None),
            ),
        );
        let ln = create(
            &mut document,
            create_line(
                Point2::new(0.0, 5.0),
                Point2::new(30.0, 5.0),
                attributes("roads", "Kerb", None),
            ),
        );
        let text = TextGeometry::new(Point2::new(0.0, 10.0), "CH 100", 2.5, 0.0);
        let tx = create(
            &mut document,
            create_text(text, attributes("roads", "", None)),
        );
        let lk = create(
            &mut document,
            create_line(
                Point2::new(0.0, 20.0),
                Point2::new(5.0, 20.0),
                attributes("locked", "", None),
            ),
        );
        let qn = create(
            &mut document,
            create_line(
                Point2::new(0.0, -5.0),
                Point2::new(5.0, -5.0),
                attributes("survey", "Tree", None),
            ),
        );
        must(document.execute(set_entity_property(vec![pa], "code", text_value("TREE1"))));
        must(document.execute(set_entity_property(vec![pb], "code", text_value("POLE"))));
        must(document.execute(set_entity_property(vec![pc], "code", text_value("TREE2"))));

        let mut locked = document
            .model()
            .layers
            .find("locked")
            .expect("the layer locked")
            .clone();
        locked.locked = true;
        must(document.execute(update_layer(locked)));
        Self {
            document,
            pa,
            pb,
            pc,
            ln,
            tx,
            lk,
            qn,
        }
    }

    #[track_caller]
    fn entity(&self, id: EntityId) -> &Entity {
        self.document
            .model()
            .entities
            .find(id)
            .unwrap_or_else(|| panic!("no entity {id}"))
    }

    #[track_caller]
    fn layer(&self, name: &str) -> &Layer {
        self.document
            .model()
            .layers
            .find(name)
            .unwrap_or_else(|| panic!("no layer {name}"))
    }

    #[track_caller]
    fn style(&self, name: &str) -> &Style {
        self.document
            .model()
            .styles
            .find(name)
            .unwrap_or_else(|| panic!("no style {name}"))
    }

    #[track_caller]
    fn matched_by(&self, scope: &ModifyScope, filter: &ModifyFilter) -> Vec<EntityId> {
        match_entities(&self.document, scope, filter)
            .unwrap_or_else(|error| panic!("{}", error.describe()))
    }

    #[track_caller]
    fn matched(&self, scope: &ModifyScope) -> Vec<EntityId> {
        self.matched_by(scope, &ModifyFilter::default())
    }

    // Plans, checks the plan is good, and runs its command when it has one.
    #[track_caller]
    fn apply(
        &mut self,
        scope: &ModifyScope,
        filter: &ModifyFilter,
        change: &GlobalModify,
    ) -> GlobalModifyPlan {
        let mut plan = plan_global_modify(&self.document, scope, filter, change)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        if let Some(command) = plan.command.take() {
            must(self.document.execute(command));
        }
        plan
    }

    // What planning `change` on the selection is refused with; empty when it is not.
    fn refusal(&self, change: &GlobalModify) -> String {
        match plan_global_modify(
            &self.document,
            &scope_of(ScopeKind::Selection),
            &ModifyFilter::default(),
            change,
        ) {
            Ok(_) => String::new(),
            Err(error) => error.describe(),
        }
    }

    fn select(&mut self, ids: Vec<EntityId>) {
        self.document.selection_mut().set(ids);
        self.document.notify_selection_changed();
    }

    fn undo_count(&self) -> usize {
        self.document.history().undo_count()
    }

    #[track_caller]
    fn run(&mut self, interpreter: &mut CommandInterpreter, line: &str) -> String {
        interpreter
            .run(&mut self.document, line)
            .unwrap_or_else(|error| panic!("{line}: {}", error.describe()))
    }
}

fn scope_of(kind: ScopeKind) -> ModifyScope {
    ModifyScope {
        kind,
        ..ModifyScope::default()
    }
}

fn layers_scope(layers: &[&str], sublayers: bool) -> ModifyScope {
    ModifyScope {
        kind: ScopeKind::Layers,
        layers: layers.iter().map(|layer| String::from(*layer)).collect(),
        sublayers,
        ..ModifyScope::default()
    }
}

fn no_filter() -> ModifyFilter {
    ModifyFilter::default()
}

// ---- patterns --------------------------------------------------------------------------------

mod global_modify_pattern {
    use super::*;

    #[test]
    fn star_and_question_mark_match_as_a_file_name_pattern_does_ignoring_case() {
        assert!(matches_pattern("TREE1", "tree*"));
        assert!(matches_pattern("TREE1", "TREE?"));
        assert!(matches_pattern("TREE1", "*1"));
        assert!(matches_pattern("survey/trees", "survey/*"));
        assert!(matches_pattern("", "*"));
        assert!(matches_pattern("abc", "a*b*c"));
        assert!(matches_pattern("abbbc", "a*b*c"));
        // Exact apart from case when there is no wildcard.
        assert!(matches_pattern("Kerb", "KERB"));
        assert!(!matches_pattern("Kerbs", "Kerb"));
        assert!(!matches_pattern("TREE", "TREE?"));
        assert!(!matches_pattern("survey", "survey/*"));
        assert!(!matches_pattern("", "?"));
        // A back-tracking pattern that fails: the star must not stop at the
        // first 'b'.
        assert!(!matches_pattern("abcbd", "a*bc"));
        assert!(matches_pattern("abcbc", "a*bc"));
    }
}

mod global_modify_test {
    use super::*;

    // ---- what a scope takes --------------------------------------------------------------------

    #[test]
    fn the_selection_scope_takes_the_selection_and_the_drawing_scope_takes_everything() {
        let mut t = GlobalModifyTest::new();
        let chosen = vec![t.pb, t.ln];
        t.select(chosen);
        assert_eq!(t.matched(&scope_of(ScopeKind::Selection)), [t.pb, t.ln]);
        assert_eq!(
            t.matched(&scope_of(ScopeKind::Drawing)),
            [t.pa, t.pb, t.pc, t.ln, t.tx, t.lk, t.qn]
        );
    }

    #[test]
    fn a_layer_takes_its_sublayers_only_when_asked() {
        let t = GlobalModifyTest::new();
        // "survey" holds pa, pb and qn; "survey/trees" holds pc.
        assert_eq!(
            t.matched(&layers_scope(&["survey"], true)),
            [t.pa, t.pb, t.pc, t.qn]
        );
        assert_eq!(
            t.matched(&layers_scope(&["survey"], false)),
            [t.pa, t.pb, t.qn]
        );
        assert_eq!(
            t.matched(&layers_scope(&["survey/trees", "roads"], false)),
            [t.pc, t.ln, t.tx]
        );
    }

    #[test]
    fn a_missing_layer_or_no_layer_at_all_is_refused() {
        let t = GlobalModifyTest::new();
        assert_eq!(
            match_entities(&t.document, &layers_scope(&["nowhere"], true), &no_filter())
                .expect_err("a layer the drawing lacks")
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            match_entities(&t.document, &layers_scope(&[], true), &no_filter())
                .expect_err("no layer at all")
                .code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn a_view_scope_takes_what_that_view_draws_and_only_what_is_on_screen_when_given_an_area() {
        let mut t = GlobalModifyTest::new();
        // The view hides "survey", and with it "survey/trees": left are the
        // roads' line and text and the locked layer's line (a locked layer still
        // draws).
        let mut hidden = LayerOverrides::default();
        hidden.hide("survey");
        let mut scope = scope_of(ScopeKind::View);
        scope.view = Some(Arc::new(hidden));
        assert_eq!(t.matched(&scope), [t.ln, t.tx, t.lk]);

        // On screen, the box x 0..8, y 4..6: the road line (0..30 at y 5)
        // crosses it; the text at y 10 and the line at y 20 lie above it.
        scope.area = Some(Box2::new(Point2::new(0.0, 4.0), Point2::new(8.0, 6.0)));
        assert_eq!(t.matched(&scope), [t.ln]);

        // A view hiding nothing of its own draws what the document draws; an
        // entity switched off is drawn by no view.
        must(t.document.execute(set_entity_visible(vec![t.pb], false)));
        assert_eq!(
            t.matched(&scope_of(ScopeKind::View)),
            [t.pa, t.pc, t.ln, t.tx, t.lk, t.qn]
        );
    }

    // ---- which the filter keeps ------------------------------------------------------------------

    #[test]
    fn the_filter_narrows_by_type_style_colour_property_text_and_layer_pattern() {
        let t = GlobalModifyTest::new();
        let all = scope_of(ScopeKind::Drawing);

        let points = ModifyFilter {
            types: BTreeSet::from([EntityType::Point]),
            ..ModifyFilter::default()
        };
        assert_eq!(t.matched_by(&all, &points), [t.pa, t.pb, t.pc]);

        let by_layer = ModifyFilter {
            style: Some(String::from("ByLayer")),
            ..ModifyFilter::default()
        };
        assert_eq!(t.matched_by(&all, &by_layer), [t.pb, t.tx, t.lk]);

        let tree = ModifyFilter {
            style: Some(String::from("tr*")),
            ..ModifyFilter::default()
        };
        assert_eq!(t.matched_by(&all, &tree), [t.pa, t.pc, t.qn]);

        let red = ModifyFilter {
            colour: Some(Some(RED)),
            ..ModifyFilter::default()
        };
        assert_eq!(t.matched_by(&all, &red), [t.pb]);
        let colour_by_layer = ModifyFilter {
            colour: Some(None),
            ..ModifyFilter::default()
        };
        assert_eq!(
            t.matched_by(&all, &colour_by_layer),
            [t.pa, t.pc, t.ln, t.tx, t.lk, t.qn]
        );

        let tree_codes = ModifyFilter {
            property: Some(String::from("code")),
            property_value: Some(String::from("TREE*")),
            ..ModifyFilter::default()
        };
        assert_eq!(t.matched_by(&all, &tree_codes), [t.pa, t.pc]);
        let any_code = ModifyFilter {
            property: Some(String::from("code")),
            ..ModifyFilter::default()
        };
        assert_eq!(t.matched_by(&all, &any_code), [t.pa, t.pb, t.pc]);
        let any_pole = ModifyFilter {
            property_value: Some(String::from("pole")),
            ..ModifyFilter::default()
        };
        assert_eq!(t.matched_by(&all, &any_pole), [t.pb]);

        let chainage = ModifyFilter {
            text: Some(String::from("CH *")),
            ..ModifyFilter::default()
        };
        assert_eq!(t.matched_by(&all, &chainage), [t.tx]);

        let under_survey = ModifyFilter {
            layers: vec![String::from("survey/*"), String::from("roads")],
            ..ModifyFilter::default()
        };
        assert_eq!(t.matched_by(&all, &under_survey), [t.pc, t.ln, t.tx]);
    }

    #[test]
    fn drawn_only_leaves_out_what_is_switched_off_or_on_a_hidden_layer() {
        let mut t = GlobalModifyTest::new();
        let mut roads = t.layer("roads").clone();
        roads.visible = false;
        must(t.document.execute(update_layer(roads)));
        must(t.document.execute(set_entity_visible(vec![t.pa], false)));
        let drawn = ModifyFilter {
            drawn_only: true,
            ..ModifyFilter::default()
        };
        assert_eq!(
            t.matched_by(&scope_of(ScopeKind::Drawing), &drawn),
            [t.pb, t.pc, t.lk, t.qn]
        );
    }

    // ---- changing the entities -----------------------------------------------------------------

    #[test]
    fn the_selection_is_recoloured_restyled_and_moved_in_one_undo_step() {
        let mut t = GlobalModifyTest::new();
        let chosen = vec![t.pa, t.pb, t.ln];
        t.select(chosen);
        let change = GlobalModify {
            entities: EntityModify {
                colour: Some(Some(BLUE)),
                style: Some(String::from("Kerb")),
                layer: Some(String::from("roads")),
                ..EntityModify::default()
            },
            ..GlobalModify::default()
        };
        let undo_before = t.undo_count();

        let plan = t.apply(&scope_of(ScopeKind::Selection), &no_filter(), &change);

        // All three change: pa and pb in every field, ln in its colour only.
        assert_eq!(plan.changed, [t.pa, t.pb, t.ln]);
        for id in [t.pa, t.pb, t.ln] {
            assert_eq!(t.entity(id).color, Some(BLUE));
            assert_eq!(t.entity(id).style, "Kerb");
            assert_eq!(t.entity(id).layer, "roads");
        }
        assert_eq!(t.entity(t.pc).style, "Tree"); // not selected
        assert_eq!(t.undo_count(), undo_before + 1);

        must(t.document.undo());
        assert!(t.entity(t.pa).color.is_none());
        assert_eq!(t.entity(t.pa).style, "Tree");
        assert_eq!(t.entity(t.pa).layer, "survey");
        assert_eq!(t.entity(t.pb).color, Some(RED));
        assert!(t.entity(t.ln).color.is_none());
    }

    #[test]
    fn moving_onto_a_layer_the_drawing_lacks_makes_it_and_undo_takes_it_away() {
        let mut t = GlobalModifyTest::new();
        let chosen = vec![t.pa, t.pc];
        t.select(chosen);
        let change = GlobalModify {
            entities: EntityModify {
                layer: Some(String::from("asbuilt/trees")),
                ..EntityModify::default()
            },
            ..GlobalModify::default()
        };

        let plan = t.apply(&scope_of(ScopeKind::Selection), &no_filter(), &change);

        assert_eq!(plan.creates_layer.as_deref(), Some("asbuilt/trees"));
        assert!(t.document.model().layers.contains("asbuilt/trees"));
        assert_eq!(t.entity(t.pa).layer, "asbuilt/trees");
        assert_eq!(t.entity(t.pc).layer, "asbuilt/trees");
        must(t.document.undo());
        assert!(!t.document.model().layers.contains("asbuilt/trees"));
        assert_eq!(t.entity(t.pc).layer, "survey/trees");
    }

    #[test]
    fn properties_are_set_and_removed_and_text_height_reaches_only_text() {
        let mut t = GlobalModifyTest::new();
        let change = GlobalModify {
            entities: EntityModify {
                set_properties: vec![(String::from("checked"), PropertyValue::Boolean(true))],
                remove_properties: vec![String::from("code")],
                text_height: Some(5.0),
                ..EntityModify::default()
            },
            ..GlobalModify::default()
        };

        let plan = t.apply(
            &layers_scope(&["survey", "roads"], false),
            &no_filter(),
            &change,
        );

        // pa, pb, qn (survey) and ln, tx (roads) all gain "checked", so all five
        // change; of them only tx is text, so four are counted as not text.
        assert_eq!(plan.changed, [t.pa, t.pb, t.ln, t.tx, t.qn]);
        assert_eq!(plan.not_text, 4);
        match &t.entity(t.tx).geometry {
            Geometry::Text(text) => assert_eq!(text.height, 5.0),
            other => panic!("the text is no text: {other:?}"),
        }
        assert!(!t.entity(t.pa).properties.contains_key("code"));
        assert_eq!(
            t.entity(t.pa).properties.get("checked"),
            Some(&PropertyValue::Boolean(true))
        );
        assert!(t.entity(t.pc).properties.contains_key("code")); // survey/trees: not in scope
    }

    #[test]
    fn an_entity_on_a_locked_layer_is_counted_and_left_unless_the_same_request_unlocks_it() {
        let mut t = GlobalModifyTest::new();
        let mut change = GlobalModify {
            entities: EntityModify {
                colour: Some(Some(BLUE)),
                ..EntityModify::default()
            },
            ..GlobalModify::default()
        };
        let left = t.apply(
            &layers_scope(&["roads", "locked"], false),
            &no_filter(),
            &change,
        );
        assert_eq!(left.locked, 1);
        assert_eq!(left.changed, [t.ln, t.tx]);
        assert!(t.entity(t.lk).color.is_none());
        assert!(
            left.summary()
                .contains("1 entity on a locked layer left as it is"),
            "{}",
            left.summary()
        );

        // Unlocking the layer in the same request unlocks it FIRST.
        change.entities.colour = Some(Some(RED));
        change.layers.locked = Some(false);
        let unlocked = t.apply(&layers_scope(&["locked"], false), &no_filter(), &change);
        assert_eq!(unlocked.locked, 0);
        assert_eq!(t.entity(t.lk).color, Some(RED));
        assert!(!t.layer("locked").locked);
    }

    #[test]
    fn locking_a_layer_locks_it_after_its_entities_are_changed() {
        let mut t = GlobalModifyTest::new();
        let change = GlobalModify {
            entities: EntityModify {
                colour: Some(Some(BLUE)),
                ..EntityModify::default()
            },
            layers: LayerModify {
                locked: Some(true),
                ..LayerModify::default()
            },
            ..GlobalModify::default()
        };
        let plan = t.apply(&layers_scope(&["roads"], false), &no_filter(), &change);
        assert_eq!(plan.changed, [t.ln, t.tx]);
        assert_eq!(plan.layers_changed, ["roads"]);
        assert_eq!(t.entity(t.ln).color, Some(BLUE));
        assert!(t.layer("roads").locked);
        must(t.document.undo());
        assert!(!t.layer("roads").locked);
        assert!(t.entity(t.ln).color.is_none());
    }

    #[test]
    fn moving_onto_a_locked_layer_is_refused_before_anything_runs() {
        let mut t = GlobalModifyTest::new();
        let chosen = vec![t.pa];
        t.select(chosen);
        let change = GlobalModify {
            entities: EntityModify {
                layer: Some(String::from("locked")),
                ..EntityModify::default()
            },
            ..GlobalModify::default()
        };
        let refused = plan_global_modify(
            &t.document,
            &scope_of(ScopeKind::Selection),
            &no_filter(),
            &change,
        )
        .expect_err("a move onto a locked layer");
        assert_eq!(refused.code, ErrorCode::InvalidState);
    }

    // ---- changing the layers and the styles ------------------------------------------------------

    #[test]
    fn layer_fields_change_on_the_layers_the_matched_entities_sit_on() {
        // Points coded TREE*: pa on "survey", pc on "survey/trees". Those two
        // layers are recoloured and reweighted; "roads" is not.
        let mut t = GlobalModifyTest::new();
        let trees = ModifyFilter {
            property: Some(String::from("code")),
            property_value: Some(String::from("TREE*")),
            ..ModifyFilter::default()
        };
        let change = GlobalModify {
            layers: LayerModify {
                colour: Some(BLUE),
                line_weight: Some(0.5),
                ..LayerModify::default()
            },
            ..GlobalModify::default()
        };
        let plan = t.apply(&scope_of(ScopeKind::Drawing), &trees, &change);
        assert_eq!(plan.layers_changed, ["survey", "survey/trees"]);
        assert!(plan.changed.is_empty());
        assert_eq!(t.layer("survey").color, BLUE);
        assert_eq!(t.layer("survey/trees").line_weight, 0.5);
        assert_eq!(t.layer("roads").color, Color::default());
    }

    #[test]
    fn the_layers_scope_changes_its_layers_even_with_nothing_on_them() {
        let mut t = GlobalModifyTest::new();
        add_layer(&mut t.document, "empty");
        let change = GlobalModify {
            layers: LayerModify {
                visible: Some(false),
                ..LayerModify::default()
            },
            ..GlobalModify::default()
        };
        let plan = t.apply(&layers_scope(&["empty"], true), &no_filter(), &change);
        assert!(plan.matched.is_empty());
        assert_eq!(plan.layers_changed, ["empty"]);
        assert!(!t.layer("empty").visible);
    }

    #[test]
    fn a_style_definition_change_says_who_else_it_redraws() {
        // The two TREE points wear Tree; so does the line qn, outside the
        // filter - one entity the change reaches beyond its scope.
        let mut t = GlobalModifyTest::new();
        let points = ModifyFilter {
            types: BTreeSet::from([EntityType::Point]),
            style: Some(String::from("Tree")),
            ..ModifyFilter::default()
        };
        let change = GlobalModify {
            styles: StyleModify {
                symbol: Some(String::from("cross")),
                symbol_size: Some(1.5),
                ..StyleModify::default()
            },
            ..GlobalModify::default()
        };
        let plan = t.apply(&scope_of(ScopeKind::Drawing), &points, &change);
        assert_eq!(plan.styles_changed, ["Tree"]);
        assert_eq!(plan.style_reaches_others, 1);
        assert_eq!(t.style("Tree").symbol, "cross");
        assert_eq!(t.style("Tree").symbol_size, 1.5);
        assert!(
            plan.summary()
                .contains("also redraw 1 entity outside the scope"),
            "{}",
            plan.summary()
        );
    }

    #[test]
    fn a_symbol_goes_on_the_points_through_a_style_and_other_entities_are_counted() {
        // Survey layer and beneath: pa, pb, pc are points, qn is a line.
        let mut t = GlobalModifyTest::new();
        let change = GlobalModify {
            entities: EntityModify {
                symbol: Some(SymbolModify {
                    name: String::from("target"),
                    size: 0.0,
                }),
                ..EntityModify::default()
            },
            ..GlobalModify::default()
        };
        let plan = t.apply(&layers_scope(&["survey"], true), &no_filter(), &change);
        assert_eq!(plan.not_points, 1);
        assert_eq!(plan.changed, [t.pa, t.pb, t.pc]);
        let made = plan
            .creates_style
            .clone()
            .expect("a style is made for the symbol");
        assert_eq!(t.style(&made).symbol, "target");
        for id in [t.pa, t.pb, t.pc] {
            assert_eq!(t.entity(id).style, made);
        }
        assert_eq!(t.entity(t.qn).style, "Tree");
        must(t.document.undo());
        assert!(!t.document.model().styles.contains(&made));
        assert_eq!(t.entity(t.pa).style, "Tree");
        assert_eq!(t.entity(t.pb).style, "");
    }

    // The owner's "change the symbols size" (2026-11-03): one factor on every
    // symbol size the scope's styles hold. pa wears Tree (symbol "tree" at its
    // own size, 0); pb is given Pit (a square 0.3 wide) and tx Pole (a circle 1.0
    // wide). Doubling makes them 0.6 and 2.0 - exactly, since doubling a binary
    // fraction only moves its exponent - and leaves Tree, which has no number.
    #[test]
    fn the_symbol_scale_multiplies_each_styles_size_and_leaves_an_own_size_alone() {
        let mut t = GlobalModifyTest::new();
        let styles: [(&str, &str, f64); 2] = [("Pit", "square", 0.3), ("Pole", "circle", 1.0)];
        for (name, symbol, size) in styles {
            let style = Style {
                name: String::from(name),
                symbol: String::from(symbol),
                symbol_size: size,
                ..Style::default()
            };
            must(t.document.execute(create_style(style)));
        }
        must(t.document.execute(set_entity_style(vec![t.pb], "Pit")));
        must(t.document.execute(set_entity_style(vec![t.tx], "Pole")));
        let chosen = vec![t.pa, t.pb, t.tx];
        t.select(chosen);
        let undo_before = t.undo_count();

        let mut change = GlobalModify {
            styles: StyleModify {
                symbol_scale: Some(2.0),
                ..StyleModify::default()
            },
            ..GlobalModify::default()
        };
        let plan = t.apply(&scope_of(ScopeKind::Selection), &no_filter(), &change);
        assert_eq!(plan.styles_changed, ["Pit", "Pole"]);
        assert_eq!(plan.symbols_at_own_size, 1);
        assert!(
            plan.changed.is_empty(),
            "a style's size, not an entity, changes"
        );
        assert_eq!(t.style("Pit").symbol_size, 0.6);
        assert_eq!(t.style("Pole").symbol_size, 2.0);
        assert_eq!(t.style("Tree").symbol_size, 0.0);
        assert_eq!(
            plan.summary(),
            concat!(
                "3 entities matched; changing 2 styles; 1 style with a symbol at ",
                "its own size left as it is: give it a size to scale."
            )
        );
        assert_eq!(t.undo_count(), undo_before + 1);

        // Halving the doubled sizes gives the first ones back; one Undo too.
        change.styles.symbol_scale = Some(0.5);
        t.apply(&scope_of(ScopeKind::Selection), &no_filter(), &change);
        assert_eq!(t.style("Pit").symbol_size, 0.3);
        must(t.document.undo());
        must(t.document.undo());
        assert_eq!(t.style("Pit").symbol_size, 0.3);
        assert_eq!(t.style("Pole").symbol_size, 1.0);

        // Kerb has no symbol: it is neither changed nor counted.
        let chosen = vec![t.ln];
        t.select(chosen);
        let kerb = t.apply(&scope_of(ScopeKind::Selection), &no_filter(), &change);
        assert!(kerb.styles_changed.is_empty());
        assert_eq!(kerb.symbols_at_own_size, 0);
        assert!(kerb.command.is_none());
    }

    #[test]
    fn a_symbol_scale_that_is_no_factor_or_is_given_with_a_size_is_refused() {
        let mut t = GlobalModifyTest::new();
        let chosen = vec![t.pa];
        t.select(chosen);
        for bad in [0.0, -2.0, f64::NAN, f64::INFINITY] {
            let change = GlobalModify {
                styles: StyleModify {
                    symbol_scale: Some(bad),
                    ..StyleModify::default()
                },
                ..GlobalModify::default()
            };
            assert!(
                t.refusal(&change).contains("symbol scale must be above 0"),
                "{bad}"
            );
        }
        let both = GlobalModify {
            styles: StyleModify {
                symbol_size: Some(1.0),
                symbol_scale: Some(2.0),
                ..StyleModify::default()
            },
            ..GlobalModify::default()
        };
        let refused = t.refusal(&both);
        assert!(refused.contains("not both"), "{refused}");

        // A size near the largest double, doubled, is no number at all.
        let huge = Style {
            name: String::from("Huge"),
            symbol: String::from("square"),
            symbol_size: f64::MAX / 1.5,
            ..Style::default()
        };
        must(t.document.execute(create_style(huge)));
        must(t.document.execute(set_entity_style(vec![t.pa], "Huge")));
        let twice = GlobalModify {
            styles: StyleModify {
                symbol_scale: Some(2.0),
                ..StyleModify::default()
            },
            ..GlobalModify::default()
        };
        let overflow = t.refusal(&twice);
        assert!(overflow.contains("too large"), "{overflow}");
        assert!(overflow.contains("Huge"), "{overflow}");
    }

    // ---- what is refused, and what is no step at all ---------------------------------------------

    #[test]
    fn a_request_the_drawing_would_refuse_is_refused_naming_the_field() {
        let mut t = GlobalModifyTest::new();
        let chosen = vec![t.pa];
        t.select(chosen);
        assert!(
            t.refusal(&GlobalModify::default())
                .contains("nothing to change")
        );

        let missing_style = GlobalModify {
            entities: EntityModify {
                style: Some(String::from("Nope")),
                ..EntityModify::default()
            },
            ..GlobalModify::default()
        };
        assert!(t.refusal(&missing_style).contains("style does not exist"));

        let bad_layer = GlobalModify {
            entities: EntityModify {
                layer: Some(String::from("a//b")),
                ..EntityModify::default()
            },
            ..GlobalModify::default()
        };
        assert!(!t.refusal(&bad_layer).is_empty());

        let flat_text = GlobalModify {
            entities: EntityModify {
                text_height: Some(0.0),
                ..EntityModify::default()
            },
            ..GlobalModify::default()
        };
        assert!(t.refusal(&flat_text).contains("text height"));

        let missing_linetype = GlobalModify {
            layers: LayerModify {
                linetype: Some(String::from("no such dashes")),
                ..LayerModify::default()
            },
            ..GlobalModify::default()
        };
        assert!(t.refusal(&missing_linetype).contains("no linetype"));

        let missing_symbol = GlobalModify {
            styles: StyleModify {
                symbol: Some(String::from("no such symbol")),
                ..StyleModify::default()
            },
            ..GlobalModify::default()
        };
        assert!(t.refusal(&missing_symbol).contains("no symbol"));

        let missing_hatch = GlobalModify {
            layers: LayerModify {
                hatch_pattern: Some(String::from("no such hatch")),
                ..LayerModify::default()
            },
            ..GlobalModify::default()
        };
        assert!(t.refusal(&missing_hatch).contains("hatch pattern"));
    }

    #[test]
    fn matching_nothing_or_changing_nothing_pushes_no_undo_step() {
        let mut t = GlobalModifyTest::new();
        let undo_before = t.undo_count();
        let change = GlobalModify {
            entities: EntityModify {
                colour: Some(Some(RED)),
                ..EntityModify::default()
            },
            ..GlobalModify::default()
        };

        let none = ModifyFilter {
            text: Some(String::from("no such text")),
            ..ModifyFilter::default()
        };
        let nothing =
            plan_global_modify(&t.document, &scope_of(ScopeKind::Drawing), &none, &change)
                .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(nothing.command.is_none());
        assert_eq!(
            nothing.summary(),
            "Nothing matched: no entity is in the scope and passes the filter."
        );

        // pb is red already.
        let chosen = vec![t.pb];
        t.select(chosen);
        let already = plan_global_modify(
            &t.document,
            &scope_of(ScopeKind::Selection),
            &no_filter(),
            &change,
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(already.command.is_none());
        assert!(already.summary().contains("nothing to change"));
        assert_eq!(t.undo_count(), undo_before);
    }

    #[test]
    fn the_summary_counts_what_changes() {
        let mut t = GlobalModifyTest::new();
        let chosen = vec![t.pa, t.pb];
        t.select(chosen);
        let change = GlobalModify {
            entities: EntityModify {
                colour: Some(Some(BLUE)),
                ..EntityModify::default()
            },
            layers: LayerModify {
                colour: Some(BLUE),
                ..LayerModify::default()
            },
            ..GlobalModify::default()
        };
        let plan = t.apply(&scope_of(ScopeKind::Selection), &no_filter(), &change);
        assert_eq!(
            plan.summary(),
            "2 entities matched; changing 2 entities and 1 layer."
        );
    }

    // ---- the command line ----------------------------------------------------------------------

    #[test]
    fn modify_on_the_command_line_filters_sets_and_is_one_undo_step() {
        let mut t = GlobalModifyTest::new();
        let mut interpreter = CommandInterpreter::new();
        let undo_before = t.undo_count();

        // Every point on "survey" and beneath whose code starts TREE: pa and pc.
        let reply = t.run(
            &mut interpreter,
            concat!(
                "MODIFY LAYERS survey WHERE TYPE=point PROP=code:tree* SET COLOUR=#0000FF ",
                "LAYER.WEIGHT=0.35 PROP=checked:true"
            ),
        );
        assert_eq!(
            reply,
            "2 entities matched; changing 2 entities and 2 layers. One UNDO restores it."
        );
        assert_eq!(t.entity(t.pa).color, Some(BLUE));
        assert_eq!(t.entity(t.pc).color, Some(BLUE));
        assert_eq!(t.entity(t.pb).color, Some(RED)); // coded POLE
        assert_eq!(
            t.entity(t.pa).properties.get("checked"),
            Some(&PropertyValue::Boolean(true))
        );
        assert_eq!(t.layer("survey/trees").line_weight, 0.35);
        assert_eq!(t.undo_count(), undo_before + 1);

        t.run(&mut interpreter, "UNDO");
        assert!(t.entity(t.pa).color.is_none());
        assert_eq!(t.layer("survey/trees").line_weight, 0.25);
    }

    #[test]
    fn modify_preview_says_what_would_change_and_changes_nothing() {
        let mut t = GlobalModifyTest::new();
        let mut interpreter = CommandInterpreter::new();
        let undo_before = t.undo_count();
        let reply = t.run(
            &mut interpreter,
            "MODIFY DRAWING WHERE STYLE=ByLayer SET STYLE=Kerb PREVIEW",
        );
        // pb, tx and lk wear no style; lk is on the locked layer.
        assert_eq!(
            reply,
            concat!(
                "preview: 3 entities matched; changing 2 entities; 1 entity on a locked ",
                "layer left as it is."
            )
        );
        assert_eq!(t.entity(t.pb).style, "");
        assert_eq!(t.undo_count(), undo_before);
    }

    #[test]
    fn modify_on_the_selection_takes_a_quoted_symbol_with_a_size() {
        let mut t = GlobalModifyTest::new();
        let mut interpreter = CommandInterpreter::new();
        let chosen = vec![t.pa, t.ln];
        t.select(chosen);
        let reply = t.run(&mut interpreter, "GM SET \"SYMBOL=manhole@2.5\"");
        let style = t.style(&t.entity(t.pa).style);
        assert_eq!(style.symbol, "manhole");
        assert_eq!(style.symbol_size, 2.5);
        assert_eq!(t.entity(t.ln).style, "Kerb");
        assert!(reply.contains("1 entity is not a point"), "{reply}");
    }

    #[test]
    fn modify_refuses_what_it_cannot_read() {
        let mut t = GlobalModifyTest::new();
        let mut interpreter = CommandInterpreter::new();
        // No SET at all; an unknown key; nothing selected for the default scope.
        for line in [
            "MODIFY DRAWING",
            "MODIFY DRAWING SET SHADE=blue",
            "MODIFY DRAWING WHERE TYPE=blob SET COLOUR=#FF0000",
            "MODIFY SET COLOUR=#FF0000",
            "MODIFY LAYERS nowhere SET COLOUR=#FF0000",
        ] {
            assert!(interpreter.run(&mut t.document, line).is_err(), "{line}");
        }
    }

    // STYLE.SYMBOLSCALE= on the command line: Tree given a size of 0.5, then
    // tripled through the points that wear it - 1.5, exactly in binary - and the
    // line qn, which wears Tree outside the filter, redrawn with them.
    #[test]
    fn modify_scales_the_symbol_sizes_of_the_styles_its_scope_wears() {
        let mut t = GlobalModifyTest::new();
        let mut interpreter = CommandInterpreter::new();
        t.run(&mut interpreter, "STYLE SET Tree symbolsize 0.5");
        let undo_before = t.undo_count();
        let reply = t.run(
            &mut interpreter,
            "MODIFY DRAWING WHERE TYPE=point SET STYLE.SYMBOLSCALE=3",
        );
        assert_eq!(
            reply,
            concat!(
                "3 entities matched; changing 1 style; the changed styles also redraw 1 ",
                "entity outside the scope. One UNDO restores it."
            )
        );
        assert_eq!(t.style("Tree").symbol_size, 1.5);
        assert_eq!(t.undo_count(), undo_before + 1);
        t.run(&mut interpreter, "UNDO");
        assert_eq!(t.style("Tree").symbol_size, 0.5);

        for bad in [
            "MODIFY DRAWING SET STYLE.SYMBOLSCALE=abc",
            "MODIFY DRAWING SET STYLE.SYMBOLSCALE=0",
            "MODIFY DRAWING SET STYLE.SYMBOLSCALE=2 STYLE.SYMBOLSIZE=1",
        ] {
            assert!(interpreter.run(&mut t.document, bad).is_err(), "{bad}");
        }
        assert_eq!(t.style("Tree").symbol_size, 0.5);
    }
}
