//! The twin of tests/commands/test_commands.cpp: the commands and the stack
//! that runs them - creation, undo and redo, the transforms, the curve edits,
//! the attributes, the layers, the save point, the events and the log, the
//! transactions, and renaming a table item.

pub mod support;

use std::sync::{Arc, Mutex};

use katana_commands::*;
use katana_core::error::{ErrorCode, Status};
use katana_core::log::{LogRecord, Logger};
use katana_entity::{
    ChangeKind, Color, DimensionGeometry, Entity, EntityId, EntityType, Geometry, Layer, Linetype,
    LinetypeElement, Model, PointGeometry, PropertyDefinition, PropertyType, PropertyValue, Style,
    TextGeometry,
};
use katana_geometry::{Arc2, Circle2, Point2, Polyline2, Segment2, Vec2};
use katana_math::numerics::{HALF_PI, PI, tolerance};
use katana_math::vec2::nearly_equal;
use support::{Random, double_eq, near};

/// The C++ fixture: a model and the stack that edits it.
struct Fixture {
    model: Model,
    stack: CommandStack,
}

impl Fixture {
    fn new() -> Self {
        let mut model = Model::new();
        let stack = CommandStack::new(&mut model, None);
        Self { model, stack }
    }

    fn execute(&mut self, command: CommandPtr) -> Status {
        self.stack.execute(&mut self.model, command)
    }

    fn undo(&mut self) -> Status {
        self.stack.undo(&mut self.model)
    }

    fn redo(&mut self) -> Status {
        self.stack.redo(&mut self.model)
    }

    fn must_create(&mut self, command: CommandPtr) -> EntityId {
        if let Err(error) = self.execute(command) {
            panic!("{}", error.describe());
        }
        let created = self.stack.last_created_entities();
        assert_eq!(created.len(), 1);
        created.first().copied().unwrap_or(0)
    }

    fn entity(&self, id: EntityId) -> &Entity {
        self.model.entities.find(id).expect("the entity exists")
    }

    fn line_of(&self, id: EntityId) -> Segment2 {
        match &self.entity(id).geometry {
            Geometry::Line(segment) => *segment,
            other => panic!("not a line: {other:?}"),
        }
    }

    fn circle_of(&self, id: EntityId) -> Circle2 {
        match &self.entity(id).geometry {
            Geometry::Circle(circle) => *circle,
            other => panic!("not a circle: {other:?}"),
        }
    }

    fn last_created(&self) -> EntityId {
        self.stack.last_created_entities()[0]
    }

    // Snapshot of every entity, for exact before/after comparison.
    fn snapshot(&self) -> Vec<Entity> {
        let mut entities = Vec::new();
        self.model
            .entities
            .for_each(|entity| entities.push(entity.clone()));
        entities
    }
}

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn none() -> EntityAttributes {
    EntityAttributes::default()
}

fn attributes(layer: &str, style: &str) -> EntityAttributes {
    EntityAttributes {
        layer: String::from(layer),
        style: String::from(style),
        color: None,
    }
}

fn layer(name: &str) -> Layer {
    Layer {
        name: String::from(name),
        ..Layer::default()
    }
}

/// The C++ `nearlyEqual(a, b)` on points, with its default relative epsilon.
fn same_point(actual: Point2, expected: Point2) -> bool {
    nearly_equal(actual, expected, tolerance::RELATIVE)
}

fn code(status: Status) -> ErrorCode {
    status.expect_err("the command is refused").code
}

// ---- creation ------------------------------------------------------------------

mod command_create {
    use super::*;

    #[test]
    fn every_create_command_adds_one_entity_of_its_type() {
        let mut f = Fixture::new();
        let id = f.must_create(create_point(p(1.0, 2.0), none()));
        assert_eq!(f.entity(id).r#type(), EntityType::Point);
        let id = f.must_create(create_line(p(0.0, 0.0), p(5.0, 5.0), none()));
        assert_eq!(f.entity(id).r#type(), EntityType::Line);
        let id = f.must_create(create_circle(p(0.0, 0.0), 3.0, none()));
        assert_eq!(f.entity(id).r#type(), EntityType::Circle);
        let id = f.must_create(create_arc(Arc2::new(p(0.0, 0.0), 2.0, 0.0, PI), none()));
        assert_eq!(f.entity(id).r#type(), EntityType::Arc);
        let id = f.must_create(create_polyline(
            Polyline2::new(vec![p(0.0, 0.0), p(1.0, 0.0), p(1.0, 1.0)], false),
            none(),
        ));
        assert_eq!(f.entity(id).r#type(), EntityType::Polyline);
        let id = f.must_create(create_text(
            TextGeometry::new(p(0.0, 0.0), "BM1", 2.5, 0.0),
            none(),
        ));
        assert_eq!(f.entity(id).r#type(), EntityType::Text);
        let id = f.must_create(create_dimension(
            DimensionGeometry::new(p(0.0, 0.0), p(10.0, 0.0), 2.0, ""),
            none(),
        ));
        assert_eq!(f.entity(id).r#type(), EntityType::Dimension);
        assert_eq!(f.model.entities.size(), 7);
        assert_eq!(f.stack.undo_count(), 7);
    }

    #[test]
    fn invalid_geometry_is_rejected_before_touching_the_model() {
        let mut f = Fixture::new();
        let status = f.execute(create_line(p(3.0, 3.0), p(3.0, 3.0), none()));
        assert_eq!(code(status), ErrorCode::InvalidGeometry);
        assert!(f.model.entities.empty());
        assert!(!f.stack.can_undo()); // rejected commands never enter the history
        assert_eq!(f.model.entities.next_id(), 1);

        assert!(f.execute(create_circle(p(0.0, 0.0), -1.0, none())).is_err());
        // The C++ also executes a null command and is refused; a CommandPtr
        // cannot be null.
    }

    #[test]
    fn unknown_or_locked_layer_is_rejected() {
        let mut f = Fixture::new();
        assert_eq!(
            code(f.execute(create_line(
                p(0.0, 0.0),
                p(1.0, 1.0),
                attributes("Missing", "")
            ))),
            ErrorCode::NotFound
        );

        let frozen = Layer {
            name: String::from("Frozen"),
            visible: true,
            locked: true,
            ..Layer::default()
        };
        assert!(f.execute(create_layer(frozen)).is_ok());
        assert_eq!(
            code(f.execute(create_line(
                p(0.0, 0.0),
                p(1.0, 1.0),
                attributes("Frozen", "")
            ))),
            ErrorCode::CommandRejected
        );
        assert_eq!(
            code(f.execute(create_line(
                p(0.0, 0.0),
                p(1.0, 1.0),
                attributes("0", "NoSuchStyle")
            ))),
            ErrorCode::NotFound
        );
        assert!(f.model.entities.empty());
    }
}

// ---- undo / redo -----------------------------------------------------------------

mod command_undo_redo {
    use super::*;

    #[test]
    fn undo_removes_and_redo_restores_the_same_id() {
        let mut f = Fixture::new();
        let id = f.must_create(create_line(p(0.0, 0.0), p(10.0, 0.0), none()));
        let original = f.entity(id).clone();

        assert!(f.undo().is_ok());
        assert!(!f.model.entities.contains(id));
        assert!(f.stack.can_redo());

        assert!(f.redo().is_ok());
        // Not a fresh id: later history may refer to it.
        assert!(f.model.entities.contains(id));
        assert_eq!(*f.entity(id), original);
        assert_eq!(f.stack.last_created_entities(), vec![id]);
    }

    #[test]
    fn redo_history_referring_to_an_entity_survives_undoing_its_creation() {
        let mut f = Fixture::new();
        let id = f.must_create(create_line(p(0.0, 0.0), p(10.0, 0.0), none()));
        assert!(
            f.execute(move_entities(vec![id], Vec2::new(5.0, 5.0)))
                .is_ok()
        );
        assert!(f.undo().is_ok()); // undo MOVE
        assert!(f.undo().is_ok()); // undo CREATE
        assert!(f.model.entities.empty());
        assert!(f.redo().is_ok()); // CREATE again, same id
        assert!(f.redo().is_ok()); // MOVE still finds it
        assert_eq!(f.line_of(id), Segment2::new(p(5.0, 5.0), p(15.0, 5.0)));
    }

    #[test]
    fn new_command_discards_redo_history() {
        let mut f = Fixture::new();
        f.must_create(create_point(p(0.0, 0.0), none()));
        assert!(f.undo().is_ok());
        assert!(f.stack.can_redo());
        f.must_create(create_point(p(1.0, 1.0), none()));
        assert!(!f.stack.can_redo());
        assert_eq!(code(f.redo()), ErrorCode::InvalidState);
    }

    #[test]
    fn empty_stacks_report_invalid_state() {
        let mut f = Fixture::new();
        assert_eq!(code(f.undo()), ErrorCode::InvalidState);
        assert_eq!(code(f.redo()), ErrorCode::InvalidState);
        assert!(f.stack.undo_name().is_empty());
    }

    #[test]
    fn undo_is_bit_exact_after_chains_of_transforms() {
        // Inverse transforms would accumulate rounding error; before-images do
        // not.
        let mut f = Fixture::new();
        let line = f.must_create(create_line(
            p(500000.123, 5000000.456),
            p(500010.0, 5000010.0),
            none(),
        ));
        let arc = f.must_create(create_arc(
            Arc2::new(p(500005.0, 5000005.0), 3.3, 0.7, 1.9),
            none(),
        ));
        let original = f.snapshot();

        let mut random = Random::default();
        let steps = 50;
        for _ in 0..steps {
            // Drawn right to left, the order GCC evaluates the C++ call's
            // arguments in; the property holds whatever is drawn.
            let radians = random.real(-PI, PI);
            let y = random.real(4e6, 6e6);
            let x = random.real(4e5, 6e5);
            assert!(
                f.execute(rotate_entities(vec![line, arc], p(x, y), radians))
                    .is_ok()
            );
            let factor = random.real(0.5, 2.0);
            assert!(
                f.execute(scale_entities(
                    vec![line, arc],
                    p(500000.0, 5000000.0),
                    factor
                ))
                .is_ok()
            );
        }
        let transformed_state = f.snapshot();
        for _ in 0..2 * steps {
            assert!(f.undo().is_ok());
        }
        assert_eq!(f.snapshot(), original); // exact ==, no tolerance
        for _ in 0..2 * steps {
            assert!(f.redo().is_ok());
        }
        assert_eq!(f.snapshot(), transformed_state);
    }
}

// ---- transforms --------------------------------------------------------------------

mod command_transform {
    use super::*;

    #[test]
    fn move_rotate_scale_mirror() {
        let mut f = Fixture::new();
        let id = f.must_create(create_line(p(0.0, 0.0), p(10.0, 0.0), none()));

        assert!(
            f.execute(move_entities(vec![id], Vec2::new(1.0, 2.0)))
                .is_ok()
        );
        assert_eq!(f.line_of(id), Segment2::new(p(1.0, 2.0), p(11.0, 2.0)));

        assert!(
            f.execute(rotate_entities(vec![id], p(1.0, 2.0), HALF_PI))
                .is_ok()
        );
        assert!(same_point(f.line_of(id).end, p(1.0, 12.0)));

        assert!(
            f.execute(scale_entities(vec![id], p(1.0, 2.0), 0.5))
                .is_ok()
        );
        assert!(same_point(f.line_of(id).end, p(1.0, 7.0)));

        assert!(
            f.execute(mirror_entities(vec![id], p(0.0, 0.0), p(0.0, 1.0), false))
                .is_ok()
        );
        assert!(same_point(f.line_of(id).start, p(-1.0, 2.0)));
        assert_eq!(f.model.entities.size(), 1);
    }

    #[test]
    fn copy_mirror_copy_and_array_add_entities() {
        let mut f = Fixture::new();
        let id = f.must_create(create_circle(p(0.0, 0.0), 1.0, none()));

        assert!(
            f.execute(copy_entities(vec![id], Vec2::new(5.0, 0.0)))
                .is_ok()
        );
        assert_eq!(f.stack.last_created_entities().len(), 1);
        let copy = f.last_created();
        assert_eq!(f.circle_of(copy).center, p(5.0, 0.0));
        assert_eq!(f.circle_of(id).center, p(0.0, 0.0));

        assert!(
            f.execute(mirror_entities(vec![copy], p(0.0, 0.0), p(0.0, 1.0), true))
                .is_ok()
        );
        assert_eq!(f.model.entities.size(), 3);

        assert!(
            f.execute(array_entities(vec![id], 2, 3, Vec2::new(10.0, 20.0)))
                .is_ok()
        );
        // 2x3 cells minus the original.
        assert_eq!(f.stack.last_created_entities().len(), 5);
        assert_eq!(f.model.entities.size(), 8);
        assert!(f.undo().is_ok()); // one undo removes the whole array
        assert_eq!(f.model.entities.size(), 3);
    }

    #[test]
    fn invalid_arguments_are_rejected() {
        let mut f = Fixture::new();
        let id = f.must_create(create_circle(p(0.0, 0.0), 1.0, none()));
        assert!(
            f.execute(scale_entities(vec![id], p(0.0, 0.0), 0.0))
                .is_err()
        );
        assert!(
            f.execute(scale_entities(vec![id], p(0.0, 0.0), -2.0))
                .is_err()
        );
        assert!(
            f.execute(mirror_entities(vec![id], p(1.0, 1.0), p(1.0, 1.0), false))
                .is_err()
        );
        assert!(
            f.execute(array_entities(vec![id], 1, 1, Vec2::new(1.0, 1.0)))
                .is_err()
        );
        assert!(
            f.execute(array_entities(vec![id], 0, 5, Vec2::new(1.0, 1.0)))
                .is_err()
        );
        // An empty selection, an unknown entity and a duplicate id.
        assert!(
            f.execute(move_entities(Vec::new(), Vec2::new(1.0, 1.0)))
                .is_err()
        );
        assert!(
            f.execute(move_entities(vec![999], Vec2::new(1.0, 1.0)))
                .is_err()
        );
        assert!(
            f.execute(move_entities(vec![id, id], Vec2::new(1.0, 1.0)))
                .is_err()
        );
        assert_eq!(f.stack.undo_count(), 1);
    }

    #[test]
    fn multi_entity_commands_are_atomic() {
        let mut f = Fixture::new();
        let good = f.must_create(create_line(p(0.0, 0.0), p(10.0, 0.0), none()));
        let tiny = f.must_create(create_circle(p(0.0, 0.0), 1e-6, none()));
        let before = f.snapshot();
        // Scaling by 1e-3 drives the tiny circle below the geometric
        // tolerance. The whole command must fail and the perfectly scalable
        // line must not move.
        let status = f.execute(scale_entities(vec![good, tiny], p(0.0, 0.0), 1e-3));
        assert!(status.is_err());
        assert_eq!(f.snapshot(), before);
    }
}

// ---- delete, and trim / extend / offset / fillet / chamfer -----------------------------

mod command_editing {
    use super::*;

    #[test]
    fn delete_is_destructive_and_undoable() {
        let mut f = Fixture::new();
        let a = f.must_create(create_point(p(0.0, 0.0), none()));
        let b = f.must_create(create_point(p(1.0, 1.0), none()));
        let before = f.snapshot();

        let command = delete_entities(vec![a, b]);
        assert!(command.is_destructive());
        assert!(!create_point(p(0.0, 0.0), none()).is_destructive());
        assert!(f.execute(command).is_ok());
        assert!(f.model.entities.empty());

        assert!(f.undo().is_ok());
        assert_eq!(f.snapshot(), before);
        // All or nothing.
        assert!(f.execute(delete_entities(vec![a, 12345])).is_err());
        assert_eq!(f.model.entities.size(), 2);
    }

    #[test]
    fn trim_splits_a_line_and_keeps_its_attributes() {
        let mut f = Fixture::new();
        assert!(f.execute(create_layer(layer("Roads"))).is_ok());
        let target = f.must_create(create_line(
            p(0.0, 0.0),
            p(10.0, 0.0),
            attributes("Roads", ""),
        ));
        let cut_a = f.must_create(create_line(p(3.0, -1.0), p(3.0, 1.0), none()));
        let cut_b = f.must_create(create_line(p(7.0, -1.0), p(7.0, 1.0), none()));

        assert!(
            f.execute(trim_entity(target, vec![cut_a, cut_b], p(5.0, 0.0)))
                .is_ok()
        );
        // The identity is kept.
        assert_eq!(f.line_of(target), Segment2::new(p(0.0, 0.0), p(3.0, 0.0)));
        assert_eq!(f.stack.last_created_entities().len(), 1);
        let rest = f.entity(f.last_created()).clone();
        assert_eq!(
            rest.geometry,
            Geometry::Line(Segment2::new(p(7.0, 0.0), p(10.0, 0.0)))
        );
        assert_eq!(rest.layer, "Roads");

        assert!(f.undo().is_ok());
        assert_eq!(f.line_of(target), Segment2::new(p(0.0, 0.0), p(10.0, 0.0)));
        assert_eq!(f.model.entities.size(), 3);
    }

    #[test]
    fn trim_a_circle_against_a_polyline_cutter() {
        let mut f = Fixture::new();
        let circle = f.must_create(create_circle(p(0.0, 0.0), 5.0, none()));
        let cutter = f.must_create(create_polyline(
            Polyline2::new(vec![p(-10.0, 0.0), p(10.0, 0.0)], false),
            none(),
        ));
        assert!(
            f.execute(trim_entity(circle, vec![cutter], p(0.0, 5.0)))
                .is_ok()
        );
        let result = f.entity(circle);
        // The same entity, a new geometry kind.
        assert_eq!(result.r#type(), EntityType::Arc);
        let Geometry::Arc(arc) = &result.geometry else {
            panic!("not an arc");
        };
        assert!(same_point(arc.midpoint(), p(0.0, -5.0)));
    }

    #[test]
    fn trim_and_extend_failures_leave_the_model_alone() {
        let mut f = Fixture::new();
        let target = f.must_create(create_line(p(0.0, 0.0), p(10.0, 0.0), none()));
        let far = f.must_create(create_line(p(50.0, 50.0), p(60.0, 60.0), none()));
        let text = f.must_create(create_text(
            TextGeometry::new(p(0.0, 0.0), "T", 1.0, 0.0),
            none(),
        ));
        let before = f.snapshot();

        assert!(
            f.execute(trim_entity(target, vec![far], p(5.0, 0.0)))
                .is_err()
        );
        assert!(
            f.execute(extend_entity(target, vec![far], p(10.0, 0.0)))
                .is_err()
        );
        assert_eq!(
            code(f.execute(trim_entity(text, vec![target], p(0.0, 0.0)))),
            ErrorCode::Unsupported
        );
        assert_eq!(f.snapshot(), before);
    }

    #[test]
    fn extend_reaches_the_boundary() {
        let mut f = Fixture::new();
        let target = f.must_create(create_line(p(0.0, 0.0), p(4.0, 0.0), none()));
        let wall = f.must_create(create_line(p(9.0, -5.0), p(9.0, 5.0), none()));
        assert!(
            f.execute(extend_entity(target, vec![wall], p(4.0, 0.0)))
                .is_ok()
        );
        assert_eq!(f.line_of(target), Segment2::new(p(0.0, 0.0), p(9.0, 0.0)));
    }

    #[test]
    fn offset_creates_a_new_entity_on_the_picked_side() {
        let mut f = Fixture::new();
        let line = f.must_create(create_line(p(0.0, 0.0), p(10.0, 0.0), none()));
        assert!(f.execute(offset_entity(line, 2.0, p(5.0, -9.0))).is_ok());
        assert_eq!(
            f.line_of(f.last_created()),
            Segment2::new(p(0.0, -2.0), p(10.0, -2.0))
        );
        // The source is untouched.
        assert_eq!(f.line_of(line), Segment2::new(p(0.0, 0.0), p(10.0, 0.0)));

        let ring = f.must_create(create_polyline(
            Polyline2::new(
                vec![p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(0.0, 10.0)],
                true,
            ),
            none(),
        ));
        let area_of = |f: &Fixture, id: EntityId| match &f.entity(id).geometry {
            Geometry::Polyline(polyline) => polyline.area(),
            other => panic!("not a polyline: {other:?}"),
        };
        // Towards the inside.
        assert!(f.execute(offset_entity(ring, 1.0, p(5.0, 5.0))).is_ok());
        near(area_of(&f, f.last_created()), 64.0, 1e-9);
        // Towards the outside.
        assert!(f.execute(offset_entity(ring, 1.0, p(50.0, 5.0))).is_ok());
        near(area_of(&f, f.last_created()), 144.0, 1e-9);
    }

    #[test]
    fn fillet_and_chamfer_modify_both_lines_and_add_the_join() {
        let mut f = Fixture::new();
        let a = f.must_create(create_line(p(10.0, 0.0), p(0.0, 0.0), none()));
        let b = f.must_create(create_line(p(0.0, 0.0), p(0.0, 10.0), none()));

        assert!(f.execute(fillet_entities(a, b, 2.0)).is_ok());
        assert_eq!(f.stack.last_created_entities().len(), 1);
        assert_eq!(f.entity(f.last_created()).r#type(), EntityType::Arc);
        assert!(same_point(f.line_of(a).end, p(2.0, 0.0)));
        assert!(same_point(f.line_of(b).start, p(0.0, 2.0)));

        assert!(f.undo().is_ok());
        assert_eq!(f.line_of(a), Segment2::new(p(10.0, 0.0), p(0.0, 0.0)));
        assert_eq!(f.model.entities.size(), 2);

        assert!(f.execute(chamfer_entities(a, b, 1.0, 3.0)).is_ok());
        assert_eq!(
            f.line_of(f.last_created()),
            Segment2::new(p(1.0, 0.0), p(0.0, 3.0))
        );

        assert!(f.execute(fillet_entities(a, a, 1.0)).is_err());
        assert!(f.execute(fillet_entities(a, b, 500.0)).is_err());
    }
}

// ---- attributes ---------------------------------------------------------------------

mod command_attributes {
    use super::*;

    #[test]
    fn layer_color_visibility_and_properties() {
        let mut f = Fixture::new();
        assert!(f.execute(create_layer(layer("Survey"))).is_ok());
        let id = f.must_create(create_point(p(0.0, 0.0), none()));

        assert!(f.execute(set_entity_layer(vec![id], "Survey")).is_ok());
        assert!(
            f.execute(set_entity_color(vec![id], Some(Color::new(255, 0, 0, 255))))
                .is_ok()
        );
        assert!(f.execute(set_entity_visible(vec![id], false)).is_ok());
        assert!(
            f.execute(set_entity_property(
                vec![id],
                "code",
                PropertyValue::from("IP")
            ))
            .is_ok()
        );

        {
            let entity = f.entity(id);
            assert_eq!(entity.layer, "Survey");
            assert_eq!(entity.color.map(Color::to_hex).as_deref(), Some("#FF0000"));
            assert!(!entity.visible);
            assert_eq!(
                entity.properties.get("code"),
                Some(&PropertyValue::from("IP"))
            );
        }

        assert_eq!(
            code(f.execute(set_entity_layer(vec![id], "Nowhere"))),
            ErrorCode::NotFound
        );
        assert!(f.execute(remove_entity_property(vec![id], "code")).is_ok());
        assert_eq!(
            code(f.execute(remove_entity_property(vec![id], "code"))),
            ErrorCode::NotFound
        );
        assert!(
            f.execute(set_entity_property(vec![id], "", PropertyValue::from("x")))
                .is_err()
        );
    }

    #[test]
    fn defined_properties_are_type_checked() {
        let mut f = Fixture::new();
        assert!(
            f.model
                .properties
                .define(PropertyDefinition {
                    name: String::from("elevation"),
                    r#type: PropertyType::Real,
                    description: String::from("Ground level"),
                    default_value: None,
                })
                .is_ok()
        );
        let id = f.must_create(create_point(p(0.0, 0.0), none()));
        assert!(
            f.execute(set_entity_property(
                vec![id],
                "elevation",
                PropertyValue::Real(101.25)
            ))
            .is_ok()
        );
        assert!(
            f.execute(set_entity_property(
                vec![id],
                "elevation",
                PropertyValue::from("high")
            ))
            .is_err()
        );
        match f.entity(id).properties.get("elevation") {
            Some(PropertyValue::Real(elevation)) => double_eq(*elevation, 101.25),
            other => panic!("not a real: {other:?}"),
        }
    }

    #[test]
    fn entities_on_locked_layers_cannot_be_edited() {
        let mut f = Fixture::new();
        assert!(f.execute(create_layer(layer("Control"))).is_ok());
        let id = f.must_create(create_point(p(0.0, 0.0), attributes("Control", "")));
        let mut locked = f.model.layers.find("Control").expect("made").clone();
        locked.locked = true;
        assert!(f.execute(update_layer(locked)).is_ok());

        assert_eq!(
            code(f.execute(move_entities(vec![id], Vec2::new(1.0, 1.0)))),
            ErrorCode::CommandRejected
        );
        assert_eq!(
            code(f.execute(delete_entities(vec![id]))),
            ErrorCode::CommandRejected
        );
        assert_eq!(
            code(f.execute(set_entity_layer(vec![id], "0"))),
            ErrorCode::CommandRejected
        );
        assert!(f.model.entities.contains(id));
    }

    #[test]
    fn set_geometry_replaces_the_shape() {
        let mut f = Fixture::new();
        let id = f.must_create(create_circle(p(0.0, 0.0), 1.0, none()));
        assert!(
            f.execute(set_entity_geometry(
                id,
                Geometry::Circle(Circle2::new(p(2.0, 2.0), 4.0))
            ))
            .is_ok()
        );
        double_eq(f.circle_of(id).radius, 4.0);
        assert!(
            f.execute(set_entity_geometry(
                id,
                Geometry::Circle(Circle2::new(p(2.0, 2.0), 0.0))
            ))
            .is_err()
        );
    }
}

// ---- layers -------------------------------------------------------------------------

mod command_layers {
    use super::*;

    #[test]
    fn create_update_delete_with_undo() {
        let mut f = Fixture::new();
        assert!(f.execute(create_layer(layer("Terrain"))).is_ok());
        assert_eq!(
            code(f.execute(create_layer(layer("Terrain")))),
            ErrorCode::AlreadyExists
        );

        let mut hidden = f.model.layers.find("Terrain").expect("made").clone();
        hidden.visible = false;
        assert!(f.execute(update_layer(hidden.clone())).is_ok());
        assert!(!f.model.layers.find("Terrain").expect("kept").visible);
        // A no-op.
        assert_eq!(
            code(f.execute(update_layer(hidden))),
            ErrorCode::CommandRejected
        );
        assert!(f.undo().is_ok());
        assert!(f.model.layers.find("Terrain").expect("kept").visible);

        assert!(f.execute(delete_layer("Terrain")).is_ok());
        assert!(!f.model.layers.contains("Terrain"));
        assert!(f.undo().is_ok());
        assert!(f.model.layers.contains("Terrain"));
    }

    #[test]
    fn layers_in_use_and_the_default_layer_cannot_be_deleted() {
        let mut f = Fixture::new();
        assert!(f.execute(create_layer(layer("Busy"))).is_ok());
        f.must_create(create_point(p(0.0, 0.0), attributes("Busy", "")));
        assert_eq!(
            code(f.execute(delete_layer("Busy"))),
            ErrorCode::CommandRejected
        );
        assert_eq!(
            code(f.execute(delete_layer("0"))),
            ErrorCode::CommandRejected
        );
        assert_eq!(code(f.execute(delete_layer("Ghost"))), ErrorCode::NotFound);
        assert!(delete_layer("Busy").is_destructive());
    }

    #[test]
    fn deleting_a_branch_is_as_destructive_as_deleting_a_leaf() {
        // Audit MOD-08: the confirmation gate (command.rs) asked before
        // DELETE_LAYER of one empty leaf and not before a whole branch.
        assert!(delete_layer_tree("design").is_destructive());
        assert!(!create_layer(layer("design")).is_destructive());
        assert!(
            !rename_layer("design", "plan").is_destructive(),
            "nothing is lost"
        );
    }

    // execute() called directly, not through the stack, which validates first
    // and would report validate()'s answer twice.
    fn same_refusal(model: &mut Model, mut command: CommandPtr, expected: ErrorCode, why: &str) {
        let mut context = CommandContext::new(model);
        let validated = command.validate(&context);
        assert_eq!(validated.expect_err(why).code, expected, "{why}");
        let executed = command.execute(&mut context);
        assert_eq!(executed.expect_err(why).code, expected, "{why}");
    }

    #[test]
    fn a_layer_commands_validate_refuses_exactly_what_its_execute_would() {
        // Audit MOD-09: a caller that validates first was told these were
        // fine, and execute() then refused them. Each refusal below is the
        // table's own (LayerDatabase::check_add / check_update /
        // check_remove), so validate() and execute() give the SAME error code.
        let mut f = Fixture::new();
        same_refusal(
            &mut f.model,
            create_layer(layer("a//b")),
            ErrorCode::InvalidArgument,
            "an empty level",
        );
        same_refusal(
            &mut f.model,
            create_layer(layer("a/ b")),
            ErrorCode::InvalidArgument,
            "a leading blank",
        );
        let heavy = Layer {
            name: String::from("heavy"),
            line_weight: -0.25,
            ..Layer::default()
        };
        same_refusal(
            &mut f.model,
            create_layer(heavy),
            ErrorCode::InvalidArgument,
            "a negative weight",
        );
        same_refusal(
            &mut f.model,
            create_layer(layer("0")),
            ErrorCode::AlreadyExists,
            "the default layer",
        );

        // A weight made negative by an update, and a layer with a nested one.
        assert!(f.execute(create_layer(layer("design/surface"))).is_ok());
        let mut design = f.model.layers.find("design").expect("made").clone();
        design.line_weight = -1.0;
        same_refusal(
            &mut f.model,
            update_layer(design),
            ErrorCode::InvalidArgument,
            "a negative weight",
        );
        same_refusal(
            &mut f.model,
            delete_layer("design"),
            ErrorCode::InvalidArgument,
            "nested layers",
        );
        // The branch itself can go, as the refusal says.
        assert!(
            delete_layer_tree("design")
                .validate(&CommandContext::new(&mut f.model))
                .is_ok()
        );
        assert_eq!(
            f.model.layers.names(),
            vec!["0", "design", "design/surface"],
            "no refusal changed anything"
        );
    }

    #[test]
    fn a_layer_name_or_reference_that_is_not_utf8_is_refused_before_it_reaches_the_model() {
        // Audit MOD-12: "Caf\xE9" is what LAYER NEW Café typed on a CP1252
        // console gives - 0xE9 alone is not UTF-8 - and it was accepted, after
        // which no entity on it could be written as JSON. A String cannot hold
        // those bytes (docs/rust.md, "Text in the model is validated UTF-8"),
        // so the C++'s refusals of them have no Rust form; what is twinned is
        // the order of the checks and the UTF-8 half.
        let mut f = Fixture::new();
        assert_eq!(
            code(f.execute(rename_layer("0", "Caf\u{e9}"))),
            ErrorCode::CommandRejected,
            "the default layer is refused first"
        );
        assert!(f.execute(create_layer(layer("Cafe"))).is_ok());
        // UTF-8 itself is a name like any other: "Café" as two bytes, C3 A9.
        assert!(f.execute(create_layer(layer("Caf\u{e9}"))).is_ok());
        assert!(f.model.layers.contains("Caf\u{e9}"));
    }
}

// ---- stack state ----------------------------------------------------------------------

mod command_stack_state {
    use super::*;

    #[test]
    fn save_point_tracks_modification_through_undo_and_redo() {
        let mut f = Fixture::new();
        assert!(!f.stack.is_modified());
        f.must_create(create_point(p(0.0, 0.0), none()));
        assert!(f.stack.is_modified());

        f.stack.mark_saved();
        assert!(!f.stack.is_modified());

        assert!(f.undo().is_ok());
        assert!(f.stack.is_modified()); // behind the save point
        assert!(f.redo().is_ok());
        assert!(!f.stack.is_modified()); // back at the save point

        assert!(f.undo().is_ok());
        f.must_create(create_point(p(9.0, 9.0), none())); // the saved state is now unreachable
        assert!(f.stack.is_modified());
        assert!(f.undo().is_ok());
        assert!(f.stack.is_modified()); // an empty stack is NOT the saved state any more

        f.stack.clear();
        assert!(!f.stack.is_modified());
        assert!(!f.stack.can_undo());
    }

    #[test]
    fn publishes_events_with_entity_level_changes() {
        let mut f = Fixture::new();
        let events = Arc::new(Mutex::new(Vec::<CommandEvent>::new()));
        let recorder = Arc::clone(&events);
        f.stack.add_listener(move |event: &CommandEvent| {
            recorder.lock().unwrap().push(event.clone());
        });

        let id = f.must_create(create_line(p(0.0, 0.0), p(1.0, 1.0), none()));
        assert!(
            f.execute(move_entities(vec![id], Vec2::new(1.0, 0.0)))
                .is_ok()
        );
        assert!(f.undo().is_ok());
        assert!(f.redo().is_ok());
        // No event for failures.
        assert!(
            f.execute(move_entities(vec![404], Vec2::new(1.0, 0.0)))
                .is_err()
        );

        let events = events.lock().unwrap();
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].kind, CommandEventKind::Executed);
        assert_eq!(events[0].command_name, "CREATE_LINE");
        assert_eq!(events[0].changes.len(), 1);
        assert_eq!(events[0].changes[0].kind, ChangeKind::EntityAdded);
        assert_eq!(events[1].changes[0].kind, ChangeKind::EntityModified);
        assert_eq!(events[2].kind, CommandEventKind::Undone);
        assert_eq!(events[2].command_name, "MOVE");
        assert_eq!(events[3].kind, CommandEventKind::Redone);
    }

    #[test]
    fn logs_execution_and_rejection() {
        let logger = Arc::new(Logger::new());
        let lines = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = Arc::clone(&lines);
        logger.add_sink(move |record: &LogRecord| {
            sink.lock().unwrap().push(record.message.clone());
        });
        let mut logged_model = Model::new();
        let mut logged_stack = CommandStack::new(&mut logged_model, Some(Arc::clone(&logger)));

        assert!(
            logged_stack
                .execute(&mut logged_model, create_point(p(0.0, 0.0), none()))
                .is_ok()
        );
        assert!(
            logged_stack
                .execute(&mut logged_model, create_circle(p(0.0, 0.0), -1.0, none()))
                .is_err()
        );
        assert_eq!(*lines.lock().unwrap(), vec!["executed", "rejected"]);
    }
}

// ---- transactions -----------------------------------------------------------------------

mod command_transaction {
    use super::*;

    #[test]
    fn groups_commands_into_one_undo_step() {
        let mut f = Fixture::new();
        let mut transaction = Transaction::new("DRAW_RECTANGLE");
        transaction.add(create_line(p(0.0, 0.0), p(4.0, 0.0), none()));
        transaction.add(create_line(p(4.0, 0.0), p(4.0, 3.0), none()));
        transaction.add(create_line(p(4.0, 3.0), p(0.0, 3.0), none()));
        transaction.add(create_line(p(0.0, 3.0), p(0.0, 0.0), none()));
        assert!(f.execute(Box::new(transaction)).is_ok());

        assert_eq!(f.model.entities.size(), 4);
        assert_eq!(f.stack.undo_count(), 1);
        assert_eq!(f.stack.last_created_entities().len(), 4);
        assert!(f.undo().is_ok());
        assert!(f.model.entities.empty());
        assert!(f.redo().is_ok());
        assert_eq!(f.model.entities.size(), 4);
    }

    #[test]
    fn rolls_back_everything_when_a_later_step_fails() {
        let mut f = Fixture::new();
        let existing = f.must_create(create_line(p(0.0, 0.0), p(10.0, 0.0), none()));
        let before = f.snapshot();

        let mut transaction = Transaction::new("BAD_BATCH");
        transaction.add(create_circle(p(0.0, 0.0), 5.0, none()));
        transaction.add(move_entities(vec![existing], Vec2::new(100.0, 100.0)));
        transaction.add(create_circle(p(0.0, 0.0), -1.0, none())); // fails
        let status = f.execute(Box::new(transaction));

        let error = status.expect_err("the third step fails");
        assert!(error.context.contains("step=3"), "{}", error.context);
        assert_eq!(f.snapshot(), before);
        assert_eq!(f.stack.undo_count(), 1);
    }

    #[test]
    fn later_steps_may_depend_on_earlier_ones() {
        // The layer does not exist when the transaction is validated, only
        // once its first step has run.
        let mut f = Fixture::new();
        let mut transaction = Transaction::new("IMPORT");
        transaction.add(create_layer(layer("Imported")));
        transaction.add(create_point(p(1.0, 1.0), attributes("Imported", "")));
        assert!(f.execute(Box::new(transaction)).is_ok());
        assert_eq!(f.model.entities.count_on_layer("Imported"), 1);

        assert!(f.undo().is_ok());
        assert!(!f.model.layers.contains("Imported"));
        assert!(f.execute(Box::new(Transaction::new("EMPTY"))).is_err());
    }
}

// ---- metadata is validated, not just properties --------------------------------------------

mod command_fixture {
    use super::*;

    // Regression. create_entities() exists so importers can add fully
    // specified entities, metadata included. Property values were validated
    // but metadata was not, and the two are serialised by the SAME writer: a
    // non-finite double is written as JSON null, which the loader then
    // refuses. The result was a project that saved without complaint and
    // could never be opened again - so the gap had to be closed at the command
    // boundary, where the rest of the entity is checked.
    #[test]
    fn non_finite_metadata_is_rejected_like_non_finite_properties() {
        let mut f = Fixture::new();
        let with_value = |key: &str, value: f64, as_metadata: bool| {
            let mut entity = Entity {
                geometry: Geometry::Point(PointGeometry {
                    position: p(1.0, 2.0),
                }),
                ..Entity::default()
            };
            let values = if as_metadata {
                &mut entity.metadata
            } else {
                &mut entity.properties
            };
            values.insert(String::from(key), PropertyValue::Real(value));
            entity
        };

        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for as_metadata in [false, true] {
                let status = f.execute(create_entities(vec![with_value(
                    "elevation",
                    bad,
                    as_metadata,
                )]));
                let which = if as_metadata {
                    "metadata"
                } else {
                    "properties"
                };
                assert!(status.is_err(), "{which} accepted {bad}");
                assert_eq!(code(status), ErrorCode::InvalidArgument);
                assert!(f.model.entities.empty());
            }
        }

        // A finite value is still accepted on both, so the guard has not
        // simply banned doubles from metadata.
        assert!(
            f.execute(create_entities(vec![with_value("elevation", 42.5, true)]))
                .is_ok()
        );
        assert!(
            f.execute(create_entities(vec![with_value("elevation", 42.5, false)]))
                .is_ok()
        );
        assert_eq!(f.model.entities.size(), 2);
    }
}

// ---- renaming a table item ----------------------------------------------------------------

mod command_table_rename {
    use super::*;

    #[test]
    fn renaming_a_style_carries_every_entity_that_wore_it() {
        let mut f = Fixture::new();
        let style = Style {
            name: String::from("SEWR Manhole Cover"),
            symbol: String::from("manhole"),
            symbol_size: 1.2,
            ..Style::default()
        };
        assert!(f.execute(create_style(style)).is_ok());
        let marked = f.must_create(create_point(p(1.0, 2.0), none()));
        let plain = f.must_create(create_point(p(3.0, 4.0), none()));
        assert!(
            f.execute(set_entity_style(vec![marked], "SEWR Manhole Cover"))
                .is_ok()
        );

        assert!(
            f.execute(rename_style("SEWR Manhole Cover", "Manhole"))
                .is_ok()
        );
        assert!(!f.model.styles.contains("SEWR Manhole Cover"));
        let renamed = f.model.styles.find("Manhole").expect("renamed");
        assert_eq!(
            renamed.symbol, "manhole",
            "the definition travels with the name"
        );
        double_eq(renamed.symbol_size, 1.2);
        assert_eq!(f.entity(marked).style, "Manhole");
        assert!(
            f.entity(plain).style.is_empty(),
            "ByLayer is not a style being renamed"
        );

        // One undo step puts back the name AND everything that wore it.
        assert!(f.undo().is_ok());
        assert!(f.model.styles.contains("SEWR Manhole Cover"));
        assert!(!f.model.styles.contains("Manhole"));
        assert_eq!(f.entity(marked).style, "SEWR Manhole Cover");
        assert!(f.redo().is_ok());
        assert_eq!(f.entity(marked).style, "Manhole");
    }

    #[test]
    fn renaming_a_linetype_carries_the_layers_and_styles_that_named_it() {
        let mut f = Fixture::new();
        let fence = Linetype {
            name: String::from("fence"),
            pattern: vec![
                LinetypeElement { length: 1.0 },
                LinetypeElement { length: -0.5 },
            ],
            ..Linetype::default()
        };
        assert!(f.execute(create_linetype(fence)).is_ok());
        let boundary = Layer {
            name: String::from("Boundary"),
            linetype: String::from("fence"),
            ..Layer::default()
        };
        assert!(f.execute(create_layer(boundary)).is_ok());
        let style = Style {
            name: String::from("boundary"),
            linetype: String::from("fence"),
            ..Style::default()
        };
        assert!(f.execute(create_style(style)).is_ok());

        assert!(f.execute(rename_linetype("fence", "post and rail")).is_ok());
        assert_eq!(
            f.model.layers.find("Boundary").expect("kept").linetype,
            "post and rail"
        );
        assert_eq!(
            f.model.styles.find("boundary").expect("kept").linetype,
            "post and rail"
        );
        assert!(!f.model.linetypes.contains("fence"));
        // The delete guard reads the same holders the rename repointed, so it
        // now answers about the new name and no longer knows the old one -
        // which is the proof that the holders really moved rather than the
        // table alone.
        assert_eq!(
            code(f.execute(delete_linetype("post and rail"))),
            ErrorCode::CommandRejected
        );
        assert_eq!(
            code(f.execute(delete_linetype("fence"))),
            ErrorCode::NotFound
        );

        assert!(f.undo().is_ok());
        assert_eq!(
            f.model.layers.find("Boundary").expect("kept").linetype,
            "fence"
        );
        assert_eq!(
            f.model.styles.find("boundary").expect("kept").linetype,
            "fence"
        );
        assert!(f.model.linetypes.contains("fence"));
    }

    #[test]
    fn a_rename_onto_an_existing_name_or_of_a_protected_item_is_refused() {
        let mut f = Fixture::new();
        let a = Style {
            name: String::from("a"),
            ..Style::default()
        };
        let b = Style {
            name: String::from("b"),
            ..Style::default()
        };
        assert!(f.execute(create_style(a)).is_ok());
        assert!(f.execute(create_style(b)).is_ok());

        assert_eq!(
            code(f.execute(rename_style("a", "b"))),
            ErrorCode::AlreadyExists
        );
        assert_eq!(
            code(f.execute(rename_style("nope", "c"))),
            ErrorCode::NotFound
        );
        assert_eq!(
            code(f.execute(rename_style("a", ""))),
            ErrorCode::InvalidArgument
        );
        // Merging two tables by renaming one onto the other is refused for the
        // same reason a rename in any table is: the two definitions differ and
        // silently picking one would change how everything wearing it draws.
        assert_eq!(
            code(f.execute(rename_linetype("continuous", "solid"))),
            ErrorCode::CommandRejected,
            "the continuous linetype is what an unset ByLayer chain resolves to"
        );
        assert!(
            f.model.styles.contains("a"),
            "a refused rename changes nothing"
        );
        assert!(f.model.linetypes.contains("continuous"));
    }
}
