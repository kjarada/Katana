// Per-view layer visibility (layer_overrides.rs; tests/cad/test_layer_overrides.cpp).
//
// Two halves. The first pins LayerOverrides itself: what hide, show, isolate
// and prune_missing return and leave behind, worked out by hand from the
// contract. The second puts two views on ONE document - a plan that hides a
// layer and a 3D view that does not - and asserts that every consumer of the
// visibility rule (drawing, picking, box selection, snapping, the 3D scene,
// framing) answers per view, while the document itself never learns that a
// view hid anything.
//
// Every expected value below is derived from the stated contract, not
// captured from a run: the layer names are chosen so that the whole-segment
// rule, the ancestor rule and the "subtractive only" rule each decide at
// least one assertion.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use katana_cad::drawing::grips::{DEFAULT_GRIP_LIMIT, grips_of_selection};
use katana_cad::layer_overrides::NO_LAYER_OVERRIDES;
use katana_cad::scene::{SceneBuilder, SceneOptions, scene_bounds};
use katana_cad::section::{SectionOptions, extract_section};
use katana_cad::selection::{
    BoxSelectionMode, SelectionFilter, drawn_extent, is_drawn, is_drawn_resolved, is_ghost,
    is_selectable, pick_entity, pick_in_box,
};
use katana_cad::snapping::{SnapMode, SnapModes, SnapRequest, snap};
use katana_cad::{Document, LayerOverrides};
use katana_commands::{
    EntityAttributes, create_arc, create_layer, create_line, set_entity_visible, update_layer,
};
use katana_core::ErrorCode;
use katana_entity::entity::INVALID_ENTITY_ID;
use katana_entity::{Entity, EntityId, Geometry, Layer, LayerDatabase, Model};
use katana_geometry::{Arc2, Box2, Point2, Polyline2, Segment2, SpatialIndex};
use katana_math::numerics::PI;
use katana_render::DrawList;

use crate::{near, ok};

fn entries(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|&name| String::from(name)).collect()
}

fn names(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|&path| String::from(path)).collect()
}

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

// ---- LayerOverrides on its own ------------------------------------------------------

// design, design/surface and design/surface/tin1 (the ancestors are created by
// add), survey, and the default layer 0.
fn job_layers() -> LayerDatabase {
    let mut layers = LayerDatabase::default();
    for name in ["design/surface/tin1", "survey"] {
        ok(layers.add(Layer {
            name: String::from(name),
            ..Layer::default()
        }));
    }
    layers
}

mod layer_overrides {
    use super::*;

    #[test]
    fn an_untouched_view_hides_nothing() {
        let view = LayerOverrides::default();
        assert!(view.empty());
        assert_eq!(view.size(), 0);
        assert!(!view.hides("0"));
        assert!(!view.hides("design/surface/tin1"));
        assert!(!view.hides(""));
        assert!(!view.hides_directly("design"));
        assert_eq!(view, NO_LAYER_OVERRIDES);
    }

    #[test]
    fn hide_reports_true_only_when_the_path_was_not_already_an_entry() {
        let mut view = LayerOverrides::default();
        assert!(view.hide("design"));
        assert!(
            !view.hide("design"),
            "already hidden directly: nothing to repaint"
        );
        assert_eq!(view.size(), 1);

        // A child of a hidden parent is hidden already, but not DIRECTLY -
        // the contract counts entries, so this is a new one.
        assert!(view.hide("design/surface"));
        assert_eq!(view.size(), 2);
    }

    #[test]
    fn show_reports_true_only_when_it_removed_an_entry() {
        let mut view = LayerOverrides::default();
        assert!(!view.show("design"), "nothing to show in an untouched view");
        assert!(view.hide("design"));
        assert!(view.show("design"));
        assert!(!view.show("design"));
        assert!(view.empty());
    }

    #[test]
    fn hiding_a_path_hides_everything_beneath_it_but_not_its_parent() {
        let mut view = LayerOverrides::default();
        assert!(view.hide("design/surface"));

        assert!(view.hides("design/surface"));
        assert!(view.hides("design/surface/tin1"));
        assert!(view.hides("design/surface/tin1/edges"));
        assert!(
            !view.hides("design"),
            "hiding a child must not hide its parent"
        );
        assert!(!view.hides("design/road"), "nor its sibling");

        assert!(view.hides_directly("design/surface"));
        assert!(
            !view.hides_directly("design/surface/tin1"),
            "hidden by its parent, which a check box shows as greyed, not unticked"
        );
    }

    #[test]
    fn ancestors_are_compared_by_whole_segments_not_by_text() {
        // The same bug the entity layer's layer tree test guards the document
        // rule against: "designs" and "design 2" start with "design" as TEXT
        // but are other trees.
        let mut view = LayerOverrides::default();
        assert!(view.hide("design"));

        assert!(view.hides("design"));
        assert!(view.hides("design/surface/tin1"));
        assert!(!view.hides("designs"));
        assert!(!view.hides("designs/x"));
        assert!(!view.hides("design 2"));
        assert!(!view.hides("design-old/x"));
        assert!(!view.hides("desig"));
    }

    #[test]
    fn showing_a_child_of_a_hidden_parent_still_leaves_it_hidden() {
        let mut view = LayerOverrides::default();
        assert!(view.hide("design"));
        assert!(view.hide("design/surface/tin1"));

        assert!(view.show("design/surface/tin1"), "its own entry does go");
        assert!(!view.hides_directly("design/surface/tin1"));
        assert!(
            view.hides("design/surface/tin1"),
            "the parent still hides it"
        );

        assert!(
            !view.show("design/surface"),
            "never an entry, so nothing to remove"
        );
        assert!(view.hides("design/surface"));

        assert!(view.show("design"));
        assert!(!view.hides("design/surface/tin1"));
    }

    #[test]
    fn the_empty_path_is_never_an_entry() {
        // No layer has the empty name, and hides() never asks about it (the
        // walk up the ancestors stops before it), so an entry for it would be
        // dead.
        let mut view = LayerOverrides::default();
        assert!(!view.hide(""));
        assert!(view.empty());
        assert!(!view.hides("0"));
        assert!(!view.show(""));
    }

    #[test]
    fn clear_shows_everything_the_document_shows() {
        let mut view = LayerOverrides::default();
        assert!(view.hide("design"));
        assert!(view.hide("survey/points"));
        view.clear();
        assert!(view.empty());
        assert!(!view.hides("design/surface"));
        assert!(!view.hides("survey/points"));
    }

    #[test]
    fn two_views_are_equal_exactly_when_they_hide_the_same_entries() {
        let mut a = LayerOverrides::default();
        let mut b = LayerOverrides::default();
        assert!(a.hide("design"));
        assert!(a.hide("survey"));
        assert!(b.hide("survey"));
        assert!(b.hide("design"));
        assert_eq!(a, b, "the order things were hidden in is not state");

        assert!(b.hide("design/surface"));
        assert_ne!(
            a, b,
            "an entry under a hidden parent draws nothing different but is still state"
        );

        assert!(b.show("design/surface"));
        assert_eq!(a, b);
        a.clear();
        assert_eq!(a, NO_LAYER_OVERRIDES);
    }

    // ---- isolate ----------------------------------------------------------------

    #[test]
    fn isolating_hides_the_fewest_entries_that_leave_only_that_branch() {
        // Worked by hand. The tree of {a/b/c, a/x, a/b/y, k/y, m} is
        //
        //   a -- b -- c        k -- y        m
        //   |    `--- y
        //   `- x
        //
        // Isolating a/b keeps a/b, everything beneath it and its ancestor a.
        // At the root the siblings of "a" are k and m; under "a" the sibling
        // of "b" is x. Hiding k covers k/y. So exactly {a/x, k, m}.
        let all = names(&["a/b/c", "a/x", "a/b/y", "k/y", "m"]);
        let mut view = LayerOverrides::default();
        ok(view.isolate("a/b", &all));

        assert_eq!(view.hidden(), &entries(&["a/x", "k", "m"]));
        assert!(!view.hides("a"));
        assert!(!view.hides("a/b"));
        assert!(!view.hides("a/b/c"));
        assert!(!view.hides("a/b/y"));
        assert!(view.hides("a/x"));
        assert!(view.hides("k/y"));
        assert!(view.hides("m"));
    }

    #[test]
    fn isolating_a_leaf_hides_its_siblings_at_every_level() {
        // Root level: siblings of "design" are "0" and "asbuilt". Under
        // design: the sibling of "surface" is "road". Under design/surface:
        // "tin2".
        let all = names(&[
            "0",
            "asbuilt/road/kerb",
            "design/road/kerb",
            "design/surface/tin1",
            "design/surface/tin2",
        ]);
        let mut view = LayerOverrides::default();
        ok(view.isolate("design/surface/tin1", &all));
        assert_eq!(
            view.hidden(),
            &entries(&["0", "asbuilt", "design/road", "design/surface/tin2"])
        );
    }

    #[test]
    fn isolating_compares_whole_segments() {
        // "designs" and "design 2" are roots of their own, so they are
        // siblings of "design" to hide - not part of it to keep.
        let all = names(&["design/x", "designs/y", "design 2"]);
        let mut view = LayerOverrides::default();
        ok(view.isolate("design", &all));
        assert_eq!(view.hidden(), &entries(&["design 2", "designs"]));
        assert!(!view.hides("design/x"));
    }

    #[test]
    fn a_node_that_exists_only_as_a_prefix_can_be_isolated() {
        // No name in the list IS "design/surface"; it is a node of the derived
        // tree because "design/surface/tin1" lies beneath it (layer_path).
        let all = names(&["design/surface/tin1", "design/road", "survey"]);
        let mut view = LayerOverrides::default();
        ok(view.isolate("design/surface", &all));
        assert_eq!(view.hidden(), &entries(&["design/road", "survey"]));
    }

    #[test]
    fn isolating_the_only_tree_hides_nothing() {
        let mut view = LayerOverrides::default();
        ok(view.isolate("a", &names(&["a", "a/b", "a/c/d"])));
        assert!(view.empty());
    }

    #[test]
    fn isolating_something_that_is_not_there_fails_and_changes_nothing() {
        let all = names(&["design/surface/tin1", "survey"]);
        let mut view = LayerOverrides::default();
        assert!(view.hide("survey"));
        let before = view.clone();

        for missing in ["nothing", "design/surface/tin1/deeper", "desig", ""] {
            let error = view.isolate(missing, &all).expect_err(missing);
            assert_eq!(error.code, ErrorCode::NotFound, "'{missing}'");
            // Isolating nothing would hide the whole drawing; a failure must
            // not leave half of that behind either.
            assert_eq!(view, before, "'{missing}'");
        }
    }

    #[test]
    fn isolating_replaces_whatever_the_view_hid_before() {
        let all = names(&["a/b/c", "a/x", "a/b/y", "k/y", "m"]);
        let mut view = LayerOverrides::default();
        assert!(view.hide("a/b/c")); // inside the branch about to be isolated
        assert!(view.hide("zzz")); // names no layer at all
        ok(view.isolate("a/b", &all));
        assert_eq!(view.hidden(), &entries(&["a/x", "k", "m"]));
        assert!(!view.hides("a/b/c"));
    }

    // ---- prune_missing ----------------------------------------------------------

    #[test]
    fn pruning_drops_only_entries_that_name_no_layer() {
        let layers = job_layers();
        let mut view = LayerOverrides::default();
        for name in [
            "design",
            "design/surface/tin1",
            "survey",
            "gone",
            "design/gone",
            "designs",
            "survey/points",
        ] {
            assert!(view.hide(name), "{name}");
        }

        // gone, design/gone, designs (whole segments: not "design") and
        // survey/points (survey has no children) name nothing: four.
        assert_eq!(view.prune_missing(&layers), 4);
        assert_eq!(
            view.hidden(),
            &entries(&["design", "design/surface/tin1", "survey"])
        );

        assert_eq!(
            view.prune_missing(&layers),
            0,
            "a second prune has nothing left to drop"
        );
    }

    #[test]
    fn pruning_keeps_an_entry_for_a_parent_whose_children_exist() {
        // "design/surface" is kept because a layer lies beneath it. (Through
        // the LayerDatabase API the parent always has a record of its own too -
        // add() creates missing ancestors - so the "only a prefix" case of
        // isolate cannot be built here; the rule that keeps it is the same.)
        let layers = job_layers();
        let mut view = LayerOverrides::default();
        assert!(view.hide("design/surface"));
        assert_eq!(view.prune_missing(&layers), 0);
        assert!(view.hides("design/surface/tin1"));
    }

    #[test]
    fn pruning_an_untouched_view_drops_nothing() {
        let mut view = LayerOverrides::default();
        assert_eq!(view.prune_missing(&job_layers()), 0);
        assert_eq!(view.prune_missing(&LayerDatabase::default()), 0);
    }

    #[test]
    fn pruning_after_a_rename_stops_the_old_name_hiding_a_later_layer() {
        // The reason prune_missing exists: a view hid design/surface, the
        // branch was renamed away, and a new layer that later takes the old
        // name must not be born hidden in this view.
        let mut layers = job_layers();
        let mut view = LayerOverrides::default();
        assert!(view.hide("design/surface"));

        assert!(
            layers
                .rename_subtree("design/surface", "archive/surface")
                .is_ok()
        );
        assert_eq!(view.prune_missing(&layers), 1);
        assert!(view.empty());

        let reborn = Layer {
            name: String::from("design/surface/new"),
            ..Layer::default()
        };
        ok(layers.add(reborn.clone()));
        assert!(!view.hides(&reborn.name));
    }
}

// ---- the rule with a view, on one document ------------------------------------------

const TIN: &str = "design/surface/tin1";
const SIBLING: &str = "design 2"; // sorts between design and design/...
const TEXT_PREFIX: &str = "designs/x";
const SURVEY: &str = "survey/points";

// One document, four horizontal lines ten units apart, each on its own layer:
//
//   y = 30   survey/points
//   y = 20   designs/x
//   y = 10   design 2
//   y =  0   design/surface/tin1
//
// and two views of it: `plan` hides the PARENT "design", `model3d` hides
// nothing. Each test owns one.
struct TwoViews {
    document: Document,
    tin: EntityId,
    sibling: EntityId,
    text_prefix: EntityId,
    survey: EntityId,
    plan: LayerOverrides,
    model3d: LayerOverrides,
}

impl TwoViews {
    fn new() -> Self {
        let mut views = TwoViews {
            document: Document::default(),
            tin: INVALID_ENTITY_ID,
            sibling: INVALID_ENTITY_ID,
            text_prefix: INVALID_ENTITY_ID,
            survey: INVALID_ENTITY_ID,
            plan: LayerOverrides::default(),
            model3d: LayerOverrides::default(),
        };
        views.tin = views.line(TIN, 0.0);
        views.sibling = views.line(SIBLING, 10.0);
        views.text_prefix = views.line(TEXT_PREFIX, 20.0);
        views.survey = views.line(SURVEY, 30.0);
        assert!(views.plan.hide("design"));
        views
    }

    fn line(&mut self, layer_name: &str, y: f64) -> EntityId {
        if !self.document.model().layers.contains(layer_name) {
            ok(self.document.execute(create_layer(Layer {
                name: String::from(layer_name),
                ..Layer::default()
            })));
        }
        let on = EntityAttributes {
            layer: String::from(layer_name),
            ..EntityAttributes::default()
        };
        ok(self
            .document
            .execute(create_line(p(0.0, y), p(10.0, y), on)));
        *self
            .document
            .model()
            .entities
            .ids()
            .last()
            .expect("the line")
    }

    #[track_caller]
    fn entity(&self, id: EntityId) -> &Entity {
        self.document.model().entities.find(id).expect("the entity")
    }

    // Through a command, as the Layers panel does. An update that changes
    // nothing is refused by the command, so it is not sent.
    fn set_layer(&mut self, name: &str, visible: bool, locked: bool) {
        let mut layer = self
            .document
            .model()
            .layers
            .find(name)
            .expect("the layer")
            .clone();
        if layer.visible == visible && layer.locked == locked {
            return;
        }
        layer.visible = visible;
        layer.locked = locked;
        ok(self.document.execute(update_layer(layer)));
    }
}

fn drawn(views: &TwoViews, id: EntityId, view: &LayerOverrides) -> bool {
    is_drawn(views.document.model(), views.entity(id), view)
}

fn selectable(views: &TwoViews, id: EntityId, view: &LayerOverrides) -> bool {
    is_selectable(views.document.model(), views.entity(id), view)
}

fn indexes(document: &Document) -> [Option<&SpatialIndex>; 2] {
    [None, Some(document.spatial_index())]
}

fn in_view(view: &LayerOverrides) -> SelectionFilter<'_> {
    SelectionFilter {
        view: Some(view),
        ..SelectionFilter::default()
    }
}

mod view_layer_rule {
    use super::*;

    #[test]
    fn a_view_hiding_the_parent_hides_the_childs_entity_there_and_nowhere_else() {
        let views = TwoViews::new();

        assert!(
            !drawn(&views, views.tin, &views.plan),
            "hidden by the parent, in the plan"
        );
        assert!(!selectable(&views, views.tin, &views.plan));
        assert!(
            drawn(&views, views.tin, &views.model3d),
            "the other view is untouched"
        );
        assert!(selectable(&views, views.tin, &views.model3d));
        assert!(
            drawn(&views, views.tin, &NO_LAYER_OVERRIDES),
            "and so is the document"
        );

        // Whole segments: the plan hid "design", not these.
        assert!(drawn(&views, views.sibling, &views.plan));
        assert!(drawn(&views, views.text_prefix, &views.plan));
        assert!(drawn(&views, views.survey, &views.plan));
    }

    #[test]
    fn hiding_in_a_view_leaves_the_document_untouched() {
        // A view's layers are view state: not saved, not undoable, not a
        // change. Measured on a saved document so "modified" has a baseline
        // of false. Its own directory, by test and process: the cases run on
        // parallel threads.
        let parent = std::env::temp_dir().join(format!(
            "katana-cad-tests-view-layers-{}",
            std::process::id()
        ));
        let directory = parent.join("views.katana");
        let _ = std::fs::remove_dir_all(&parent);
        {
            let mut views = TwoViews::new();
            ok(views.document.save_as(&directory));
            assert!(!views.document.is_modified());

            let layers_before = views.document.model().layers.all();
            let undo_before = views.document.history().undo_count();
            let notified = Arc::new(AtomicI32::new(0));
            let heard = Arc::clone(&notified);
            let _listening = views.document.add_listener(move || {
                heard.fetch_add(1, Ordering::SeqCst);
            });

            assert!(views.model3d.hide(SURVEY));
            let all = views.document.model().layers.names();
            ok(views.plan.isolate(SURVEY, &all));
            assert!(views.plan.show("design"));
            assert_eq!(views.plan.prune_missing(&views.document.model().layers), 0);
            views.model3d.clear();

            assert_eq!(
                views.document.model().layers.all(),
                layers_before,
                "no Layer::visible flag may be flipped to implement a view"
            );
            assert_eq!(views.document.history().undo_count(), undo_before);
            assert!(!views.document.is_modified());
            assert_eq!(
                notified.load(Ordering::SeqCst),
                0,
                "the document heard nothing, so nothing else repaints"
            );
        }
        let _ = std::fs::remove_dir_all(parent);
    }

    #[test]
    fn a_view_cannot_show_what_the_document_hides() {
        let mut views = TwoViews::new();
        views.set_layer("survey", false, false);

        assert!(!drawn(&views, views.survey, &views.plan));
        assert!(!drawn(&views, views.survey, &views.model3d));

        // There is nothing a view can do to bring it back: show() removes only
        // a hide of the view's own, and an untouched view is the most it can
        // show.
        assert!(!views.model3d.show("survey"));
        assert!(!views.model3d.show(SURVEY));
        views.plan.clear();
        assert!(!drawn(&views, views.survey, &views.plan));
        assert!(!drawn(&views, views.survey, &views.model3d));
    }

    #[test]
    fn a_locked_layer_is_drawn_but_not_selectable_in_a_view_that_shows_it() {
        let mut views = TwoViews::new();
        views.set_layer("survey", true, true);

        assert!(drawn(&views, views.survey, &views.model3d));
        assert!(!selectable(&views, views.survey, &views.model3d));

        // And a view that hides it neither draws nor selects it.
        assert!(views.model3d.hide("survey"));
        assert!(!drawn(&views, views.survey, &views.model3d));
        assert!(!selectable(&views, views.survey, &views.model3d));
    }

    #[test]
    fn no_overrides_is_exactly_the_document_rule_it_replaced() {
        // The rule before per-view layers was
        //   drawn      = entity.visible && layer effectively visible
        //   selectable = drawn && !layer effectively locked
        // NO_LAYER_OVERRIDES and an untouched view must give exactly that, in
        // every combination of the document's own switches.
        let mut views = TwoViews::new();
        let ids = [views.tin, views.sibling, views.text_prefix, views.survey];
        let mut combinations = 0;
        for parent_visible in [true, false] {
            for parent_locked in [false, true] {
                for survey_visible in [true, false] {
                    views.set_layer("design", parent_visible, parent_locked);
                    views.set_layer("survey", survey_visible, !survey_visible);
                    ok(views.document.execute(set_entity_visible(
                        vec![views.text_prefix],
                        parent_visible != survey_visible,
                    )));
                    let model = views.document.model();
                    let untouched = LayerOverrides::default();
                    for &id in &ids {
                        let entity = views.entity(id);
                        let old_drawn =
                            entity.visible && model.layers.effectively_visible(&entity.layer);
                        let old_selectable =
                            old_drawn && !model.layers.effectively_locked(&entity.layer);
                        assert_eq!(
                            is_drawn(model, entity, &NO_LAYER_OVERRIDES),
                            old_drawn,
                            "{}",
                            entity.layer
                        );
                        assert_eq!(
                            is_drawn_resolved(
                                &model.layers.resolve(&entity.layer),
                                entity,
                                &NO_LAYER_OVERRIDES
                            ),
                            old_drawn,
                            "{}",
                            entity.layer
                        );
                        assert_eq!(
                            is_selectable(model, entity, &NO_LAYER_OVERRIDES),
                            old_selectable,
                            "{}",
                            entity.layer
                        );
                        assert_eq!(is_drawn(model, entity, &untouched), old_drawn);
                        assert_eq!(is_selectable(model, entity, &untouched), old_selectable);
                    }
                    combinations += 1;
                }
            }
        }
        assert_eq!(combinations, 8);
    }

    #[test]
    fn an_entity_on_a_layer_that_does_not_exist_is_drawn_in_no_view() {
        // ResolvedLayer's contract: a missing layer is neither shown nor
        // editable. A view adds nothing to that - it can only subtract.
        let model = Model::default();
        let orphan = Entity {
            layer: String::from("no/such/layer"),
            geometry: Geometry::Line(Segment2::new(p(0.0, 0.0), p(1.0, 0.0))),
            ..Entity::default()
        };
        let untouched = LayerOverrides::default();
        assert!(!is_drawn(&model, &orphan, &untouched));
        assert!(!is_selectable(&model, &orphan, &untouched));
    }

    // ---- every consumer honours the view ------------------------------------------

    #[test]
    fn picking_honours_the_view_it_is_made_in() {
        let views = TwoViews::new();
        let model = views.document.model();
        let on_the_tin = p(5.0, 0.0);
        let in_plan = in_view(&views.plan);
        let in3d = in_view(&views.model3d);

        for index in indexes(&views.document) {
            assert!(
                pick_entity(model, on_the_tin, 1.0, &in_plan, index).is_none(),
                "a line the plan does not show cannot be picked there"
            );
            assert_eq!(
                pick_entity(model, on_the_tin, 1.0, &in3d, index),
                Some(views.tin)
            );
            assert_eq!(
                pick_entity(model, on_the_tin, 1.0, &SelectionFilter::default(), index),
                Some(views.tin),
                "no view: the document rule"
            );
            // The plan still picks what it shows.
            assert_eq!(
                pick_entity(model, p(5.0, 10.0), 1.0, &in_plan, index),
                Some(views.sibling)
            );
        }
    }

    #[test]
    fn box_selection_honours_the_view_it_is_made_in() {
        let views = TwoViews::new();
        let model = views.document.model();
        let everything = Box2::new(p(-1.0, -1.0), p(11.0, 31.0));
        let in_plan = in_view(&views.plan);
        let in3d = in_view(&views.model3d);

        let all = vec![views.tin, views.sibling, views.text_prefix, views.survey];
        let all_but_tin = vec![views.sibling, views.text_prefix, views.survey];
        for mode in [BoxSelectionMode::Window, BoxSelectionMode::Crossing] {
            for index in indexes(&views.document) {
                assert_eq!(
                    pick_in_box(model, everything, mode, &in_plan, index),
                    all_but_tin
                );
                assert_eq!(pick_in_box(model, everything, mode, &in3d, index), all);
                assert_eq!(
                    pick_in_box(model, everything, mode, &SelectionFilter::default(), index),
                    all
                );
            }
        }
    }

    #[test]
    fn snapping_honours_the_view_the_cursor_is_in() {
        let views = TwoViews::new();
        let model = views.document.model();

        for index in indexes(&views.document) {
            // The tin line's end; the next line is 10 away.
            let mut request = SnapRequest {
                cursor: p(10.0, 0.0),
                aperture: 1.0,
                modes: SnapMode::Endpoint as SnapModes,
                view: Some(&views.plan),
                ..SnapRequest::default()
            };
            assert!(
                snap(model, &request, index).is_none(),
                "a snap to a line the view does not show"
            );

            request.view = Some(&views.model3d);
            let snapped = snap(model, &request, index).expect("a snap");
            assert_eq!(snapped.mode, SnapMode::Endpoint);
            assert_eq!(snapped.entity, views.tin);
            assert_eq!(snapped.point, p(10.0, 0.0));
        }
    }

    #[test]
    fn a_ghost_is_neither_picked_nor_snapped_to_nor_given_grips() {
        // Selected, the tin line is drawn in the plan view as a ghost
        // (selection_style.rs) - drawn, but still on a layer that view hides:
        // a click, a snap or a grip there must not reach what the view hides,
        // or Delete would erase, and a drag move, something the user cannot
        // see.
        let mut views = TwoViews::new();
        let tin = views.tin;
        views.document.selection_mut().add(tin);
        let model = views.document.model();
        // A ghost there by the one rule, and drawn in the 3D view, which hides
        // nothing: it is the plan view's rule that keeps it out of reach.
        assert!(is_ghost(model, views.entity(views.tin), &views.plan));
        assert!(!is_ghost(model, views.entity(views.tin), &views.model3d));
        let selected = views.document.selection().ids();
        assert!(
            grips_of_selection(&views.document, &selected, &views.plan, DEFAULT_GRIP_LIMIT)
                .is_empty()
        );
        assert_eq!(
            grips_of_selection(
                &views.document,
                &selected,
                &views.model3d,
                DEFAULT_GRIP_LIMIT
            )
            .len(),
            3,
            "a line's two ends and middle, where the view shows it"
        );
        let in_plan = in_view(&views.plan);
        let request = SnapRequest {
            cursor: p(10.0, 0.0),
            aperture: 1.0,
            modes: SnapMode::Endpoint as SnapModes,
            view: Some(&views.plan),
            ..SnapRequest::default()
        };
        let around = Box2::new(p(-1.0, -1.0), p(11.0, 1.0));
        for index in indexes(&views.document) {
            assert!(pick_entity(model, p(5.0, 0.0), 1.0, &in_plan, index).is_none());
            assert!(snap(model, &request, index).is_none());
            assert!(
                pick_in_box(model, around, BoxSelectionMode::Crossing, &in_plan, index).is_empty()
            );
        }
    }

    #[test]
    fn a_hidden_line_is_not_half_of_an_intersection_snap() {
        // An intersection needs two drawn curves. A vertical survey line
        // crosses the tin at (3, 0); in the plan the tin is hidden, so there
        // is nothing to intersect with.
        let mut views = TwoViews::new();
        let on = EntityAttributes {
            layer: String::from(SURVEY),
            ..EntityAttributes::default()
        };
        ok(views
            .document
            .execute(create_line(p(3.0, -5.0), p(3.0, 5.0), on)));
        let crossing = *views
            .document
            .model()
            .entities
            .ids()
            .last()
            .expect("the crossing line");
        assert_ne!(crossing, INVALID_ENTITY_ID);

        let mut request = SnapRequest {
            cursor: p(3.2, 0.1),
            aperture: 1.0,
            modes: SnapMode::Intersection as SnapModes,
            view: Some(&views.model3d),
            ..SnapRequest::default()
        };
        let shown = snap(views.document.model(), &request, None).expect("a snap");
        assert_eq!(shown.mode, SnapMode::Intersection);
        near(shown.point.x, 3.0, 1e-12);
        near(shown.point.y, 0.0, 1e-12);

        request.view = Some(&views.plan);
        assert!(snap(views.document.model(), &request, None).is_none());
    }

    #[test]
    fn the_three_d_scene_honours_the_view_it_is_built_for() {
        // Each straight line on a continuous linetype is one DrawLine (the
        // layer inheritance test counts the same way).
        let views = TwoViews::new();
        let mut builder = SceneBuilder::new();
        let mut list = DrawList::default();

        let in3d = SceneOptions {
            draw_grid: false,
            layers: Some(&views.model3d),
            ..SceneOptions::default()
        };
        builder.build(&views.document, &[], &in3d, &mut list, &[]);
        assert_eq!(list.lines.len(), 4);

        let in_plan = SceneOptions {
            draw_grid: false,
            layers: Some(&views.plan),
            ..SceneOptions::default()
        };
        builder.build(&views.document, &[], &in_plan, &mut list, &[]);
        assert_eq!(list.lines.len(), 3, "the tin line is hidden in this view");

        let no_view = SceneOptions {
            draw_grid: false,
            layers: None,
            ..SceneOptions::default()
        };
        builder.build(&views.document, &[], &no_view, &mut list, &[]);
        assert_eq!(list.lines.len(), 4, "no view: the document rule");
    }

    #[test]
    fn scene_bounds_frame_only_what_the_view_draws() {
        let mut views = TwoViews::new();

        let in3d = SceneOptions {
            draw_grid: false,
            layers: Some(&views.model3d),
            ..SceneOptions::default()
        };
        let bounds = scene_bounds(&views.document, &[], &in3d, &[]);
        assert!(!bounds.empty());
        assert_eq!(bounds.min.y, 0.0);
        assert_eq!(bounds.max.y, 30.0);

        // The line at y = 0 is hidden here.
        let in_plan = SceneOptions {
            draw_grid: false,
            layers: Some(&views.plan),
            ..SceneOptions::default()
        };
        let bounds = scene_bounds(&views.document, &[], &in_plan, &[]);
        assert!(!bounds.empty());
        assert_eq!(bounds.min.y, 10.0);
        assert_eq!(bounds.max.y, 30.0);
        assert_eq!(bounds.min.x, 0.0);
        assert_eq!(bounds.max.x, 10.0);

        assert!(views.plan.hide("survey"));
        let in_plan = SceneOptions {
            draw_grid: false,
            layers: Some(&views.plan),
            ..SceneOptions::default()
        };
        assert_eq!(
            scene_bounds(&views.document, &[], &in_plan, &[]).max.y,
            20.0
        );

        views.plan.clear();
        for name in ["design", SIBLING, "designs", "survey"] {
            assert!(views.plan.hide(name));
        }
        let in_plan = SceneOptions {
            draw_grid: false,
            layers: Some(&views.plan),
            ..SceneOptions::default()
        };
        assert!(
            scene_bounds(&views.document, &[], &in_plan, &[]).empty(),
            "a view that hides everything frames nothing"
        );
    }

    #[test]
    fn a_section_crossing_carries_the_layer_a_view_filters_it_by() {
        // The section is cut once with the document rule and shared between
        // views (section.rs); a view that hides a layer drops that layer's
        // crossings when it paints them. That needs the crossing to name the
        // layer of the entity it came from.
        let views = TwoViews::new();
        // Crosses all four lines.
        let across = Polyline2::new(vec![p(5.0, -5.0), p(5.0, 35.0)], false);
        let section = extract_section(
            &across,
            &[],
            Some(views.document.model()),
            &SectionOptions::default(),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(section.crossings.len(), 4);

        let mut shown_in_plan = 0;
        for crossing in &section.crossings {
            assert_eq!(crossing.layer, views.entity(crossing.entity).layer);
            if !views.plan.hides(&crossing.layer) {
                shown_in_plan += 1;
            }
        }
        assert_eq!(
            shown_in_plan, 3,
            "the plan hides the one on design/surface/tin1"
        );
    }
}

// ---- drawn_extent ------------------------------------------------------------------------

mod drawn_extent {
    use super::*;

    #[test]
    fn an_empty_model_has_an_empty_extent() {
        let model = Model::default();
        assert!(drawn_extent(&model, &NO_LAYER_OVERRIDES).empty());
    }

    #[test]
    fn the_extent_leaves_out_everything_the_view_does_not_draw() {
        // Strays far outside the drawing, each hidden a different way; only
        // the locked one counts, because a locked layer still draws.
        let mut views = TwoViews::new();
        let hidden_entity = views.line("strays/entity", 1000.0);
        views.line("strays/layer", -1000.0);
        views.line("strays/view", 500.0);
        let locked = views.line("locked", 40.0);
        ok(views
            .document
            .execute(set_entity_visible(vec![hidden_entity], false)));
        views.set_layer("strays/layer", false, false);
        views.set_layer("locked", true, true);
        assert!(views.model3d.hide("strays/view"));
        assert_ne!(locked, INVALID_ENTITY_ID);

        // model3d draws y = 0, 10, 20, 30 and the locked line at 40.
        let in3d = drawn_extent(views.document.model(), &views.model3d);
        assert_eq!(in3d, Box2::new(p(0.0, 0.0), p(10.0, 40.0)));

        // The plan also hides the tin at y = 0, but shows strays/view at 500.
        let in_plan = drawn_extent(views.document.model(), &views.plan);
        assert_eq!(in_plan, Box2::new(p(0.0, 10.0), p(10.0, 500.0)));

        // A view hiding every layer that is drawn frames nothing.
        let mut nothing = LayerOverrides::default();
        for name in views.document.model().layers.roots() {
            nothing.hide(&name);
        }
        assert!(drawn_extent(views.document.model(), &nothing).empty());
    }

    #[test]
    fn an_arc_contributes_its_own_extent_not_its_circles() {
        // The upper half of a circle of radius 10 about the origin: from
        // (10, 0) through (0, 10) to (-10, 0). Its own box is [-10, 10] x
        // [0, 10]; its circle's would reach down to y = -10.
        let mut document = Document::default();
        let upper = Arc2::new(p(0.0, 0.0), 10.0, 0.0, PI);
        let attributes = document.current_attributes();
        ok(document.execute(create_arc(upper, attributes)));

        let bounds = drawn_extent(document.model(), &NO_LAYER_OVERRIDES);
        assert!(!bounds.empty());
        // sin(pi) is 1.2e-16 in doubles, not 0: the tolerance is that
        // rounding.
        near(bounds.min.x, -10.0, 1e-12);
        near(bounds.max.x, 10.0, 1e-12);
        // An arc framed by its circle would reach y = -10.
        near(bounds.min.y, 0.0, 1e-12);
        near(bounds.max.y, 10.0, 1e-12);
    }
}
