// Visibility and lock inherit DOWN the layer tree (PLAN.MD section 10, 5.1;
// tests/cad/test_layer_inheritance.cpp): a layer switched off or locked by an
// ancestor must be undrawn, unpickable and uneditable exactly as though it had
// been switched off or locked itself.
//
// Until 2026-09-23 only the layer panel honoured that: it greyed the child,
// while the viewport, the 3D scene, sections, snapping, picking and the
// commands all looked at the entity's own layer and nothing above it (audit
// MOD-01, MOD-02, MOD-03, REN-03, REN-04). Each test below hides or locks the
// PARENT and asserts on an entity on the CHILD.

use crate::ok;
use katana_cad::document::Document;
use katana_cad::layer_overrides::NO_LAYER_OVERRIDES;
use katana_cad::scene::{SceneBuilder, SceneOptions, scene_bounds};
use katana_cad::section::{SectionOptions, extract_section};
use katana_cad::selection::{
    BoxSelectionMode, SelectionFilter, is_drawn, is_selectable, pick_entity, pick_in_box,
};
use katana_cad::snapping::{SnapRequest, snap};
use katana_commands::{
    EntityAttributes, create_layer, create_line, delete_entities, move_entities, update_layer,
};
use katana_core::ErrorCode;
use katana_entity::{Entity, EntityId, Layer};
use katana_geometry::{Box2, Point2, Polyline2, Vec2};
use katana_render::DrawList;

const PARENT: &str = "design";
const CHILD: &str = "design/surface/tin1";

/// A document with one line, (0,0)-(10,0), on the grandchild layer.
struct Drawing {
    document: Document,
    line: EntityId,
}

impl Drawing {
    fn new() -> Self {
        let mut document = Document::default();
        let child = Layer {
            name: String::from(CHILD),
            ..Layer::default()
        };
        ok(document.execute(create_layer(child)));
        let on = EntityAttributes {
            layer: String::from(CHILD),
            ..EntityAttributes::default()
        };
        ok(document.execute(create_line(
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 0.0),
            on,
        )));
        let line = *document.model().entities.ids().last().expect("the line");
        Self { document, line }
    }

    fn line_of(&self) -> &Entity {
        self.document
            .model()
            .entities
            .find(self.line)
            .expect("the line")
    }
}

fn set_parent(document: &mut Document, visible: bool, locked: bool) {
    let mut parent = document
        .model()
        .layers
        .find(PARENT)
        .expect("the parent layer")
        .clone();
    parent.visible = visible;
    parent.locked = locked;
    ok(document.execute(update_layer(parent)));
}

fn picked_at(document: &Document, point: Point2, tolerance: f64) -> Option<EntityId> {
    pick_entity(
        document.model(),
        point,
        tolerance,
        &SelectionFilter::default(),
        None,
    )
}

mod cad_layer_inheritance {
    use super::*;

    #[test]
    fn the_childs_own_flags_are_clear_so_only_the_parent_can_be_responsible() {
        // The premise of every test below: the child layer itself is visible
        // and unlocked, and adding it created the parent and middle layer.
        let drawing = Drawing::new();
        let layers = &drawing.document.model().layers;
        assert!(layers.find(PARENT).is_some());
        assert!(layers.find("design/surface").is_some());
        assert!(layers.find(CHILD).expect("the child").visible);
        assert!(!layers.find(CHILD).expect("the child").locked);
        assert!(is_drawn(
            drawing.document.model(),
            drawing.line_of(),
            &NO_LAYER_OVERRIDES
        ));
        assert!(is_selectable(
            drawing.document.model(),
            drawing.line_of(),
            &NO_LAYER_OVERRIDES
        ));
    }

    #[test]
    fn hiding_the_parent_hides_the_grandchild_from_drawing_picking_and_snapping() {
        let mut drawing = Drawing::new();
        set_parent(&mut drawing.document, false, false);
        let model = drawing.document.model();

        assert!(!is_drawn(model, drawing.line_of(), &NO_LAYER_OVERRIDES));
        assert!(!is_selectable(
            model,
            drawing.line_of(),
            &NO_LAYER_OVERRIDES
        ));
        assert!(picked_at(&drawing.document, Point2::new(5.0, 0.0), 1.0).is_none());
        assert!(
            pick_in_box(
                model,
                Box2::new(Point2::new(-1.0, -1.0), Point2::new(11.0, 1.0)),
                BoxSelectionMode::Window,
                &SelectionFilter::default(),
                None,
            )
            .is_empty()
        );
        let request = SnapRequest {
            cursor: Point2::new(10.0, 0.0),
            aperture: 1.0,
            ..SnapRequest::default()
        };
        assert!(
            snap(model, &request, None).is_none(),
            "a hidden line's endpoint is not something to snap to"
        );

        // And switching the parent back on restores all of it: the inherited
        // state is recomputed when the parent changes, not frozen at creation.
        set_parent(&mut drawing.document, true, false);
        assert!(is_drawn(
            drawing.document.model(),
            drawing.line_of(),
            &NO_LAYER_OVERRIDES
        ));
        assert!(picked_at(&drawing.document, Point2::new(5.0, 0.0), 1.0).is_some());
    }

    #[test]
    fn locking_the_parent_protects_the_grandchild_from_every_edit() {
        // The audit's case: lock "design", SELECT ALL, ERASE - the line on
        // design/surface/tin1 was erased.
        let mut drawing = Drawing::new();
        set_parent(&mut drawing.document, true, true);

        // Still drawn - a locked layer reads as background, it does not
        // vanish - but not selectable.
        assert!(is_drawn(
            drawing.document.model(),
            drawing.line_of(),
            &NO_LAYER_OVERRIDES
        ));
        assert!(!is_selectable(
            drawing.document.model(),
            drawing.line_of(),
            &NO_LAYER_OVERRIDES
        ));

        // Every edit is refused by the model, whatever path asked for it, and
        // the refusal says which layer holds the lock.
        let erase = drawing
            .document
            .execute(delete_entities(vec![drawing.line]))
            .expect_err("the erase is refused");
        assert_eq!(erase.code, ErrorCode::CommandRejected);
        assert!(erase.message.contains("design"), "{}", erase.message);
        assert!(
            drawing
                .document
                .execute(move_entities(vec![drawing.line], Vec2::new(1.0, 0.0)))
                .is_err()
        );
        assert!(
            drawing
                .document
                .model()
                .entities
                .find(drawing.line)
                .is_some()
        );

        // Nothing new may be put on it either.
        let on = EntityAttributes {
            layer: String::from(CHILD),
            ..EntityAttributes::default()
        };
        assert!(
            drawing
                .document
                .execute(create_line(
                    Point2::new(0.0, 1.0),
                    Point2::new(1.0, 1.0),
                    on
                ))
                .is_err()
        );

        set_parent(&mut drawing.document, true, false);
        ok(drawing
            .document
            .execute(delete_entities(vec![drawing.line])));
    }

    #[test]
    fn the_three_d_scene_leaves_out_what_the_parent_hides() {
        let mut drawing = Drawing::new();
        let mut builder = SceneBuilder::new();
        let options = SceneOptions {
            draw_grid: false,
            ..SceneOptions::default()
        };
        let mut shown = DrawList::default();
        builder.build(&drawing.document, &[], &options, &mut shown, &[]);
        assert_eq!(shown.lines.len(), 1);

        set_parent(&mut drawing.document, false, false);
        let mut hidden = DrawList::default();
        builder.build(&drawing.document, &[], &options, &mut hidden, &[]);
        assert!(hidden.lines.is_empty());

        // Nor does it frame around it: a hidden stray used to pull
        // zoom-extents.
        assert!(scene_bounds(&drawing.document, &[], &options, &[]).empty());
    }

    #[test]
    fn a_section_does_not_cross_what_the_parent_hides() {
        let mut drawing = Drawing::new();
        // Crosses the line at (5, 0).
        let across = Polyline2::new(vec![Point2::new(5.0, -5.0), Point2::new(5.0, 5.0)], false);

        let before = match extract_section(
            &across,
            &[],
            Some(drawing.document.model()),
            &SectionOptions::default(),
        ) {
            Ok(section) => section,
            Err(error) => panic!("{}", error.describe()),
        };
        assert_eq!(before.crossings.len(), 1);

        set_parent(&mut drawing.document, false, false);
        let after = match extract_section(
            &across,
            &[],
            Some(drawing.document.model()),
            &SectionOptions::default(),
        ) {
            Ok(section) => section,
            Err(error) => panic!("{}", error.describe()),
        };
        assert!(after.crossings.is_empty());
    }

    #[test]
    fn moving_a_branch_under_a_hidden_parent_hides_it() {
        // A rename changes a subtree's ancestors, so its inherited state must
        // be recomputed from its NEW parents.
        let mut drawing = Drawing::new();
        let archive = Layer {
            name: String::from("archive"),
            visible: false,
            ..Layer::default()
        };
        ok(drawing.document.execute(create_layer(archive)));

        // A copy to rename, so the document is untouched.
        let mut layers = drawing.document.model().layers.clone();
        if let Err(error) = layers.rename_subtree("design/surface", "archive/surface") {
            panic!("{}", error.describe());
        }
        assert!(!layers.effectively_visible("archive/surface/tin1"));
        assert!(layers.effectively_visible("design"));
    }
}
