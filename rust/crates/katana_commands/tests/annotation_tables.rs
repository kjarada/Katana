//! The twin of tests/commands/test_annotation_tables.cpp: the annotation
//! tables' commands (docs/annotation.md) - create, update and delete through
//! the one table-command implementation, each one undo step, and the guards
//! that refuse to delete a style something still names.

pub mod support;

use katana_commands::*;
use katana_core::error::{ErrorCode, Status};
use katana_entity::annotation::DEFAULT_TEXT_STYLE_NAME;
use katana_entity::{
    AnchorPoint, AnchorRef, DimensionGeometry, Entity, EntityId, Geometry, LabelGeometry,
    LabelKind, LabelRule, LabelStyle, LeaderGeometry, Model, PointGeometry, TextGeometry,
    TextStyle,
};
use katana_geometry::{Point2, Segment2, Vec2};
use support::double_eq;

/// The C++ tests' `Model model; CommandStack stack(model);`.
struct Drawing {
    model: Model,
    stack: CommandStack,
}

impl Drawing {
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

    fn entity(&self, id: EntityId) -> &Entity {
        self.model.entities.find(id).expect("the entity exists")
    }
}

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn code(status: Status) -> ErrorCode {
    status.expect_err("the command is refused").code
}

fn bearing_style() -> LabelStyle {
    LabelStyle {
        name: String::from("Bearing"),
        kind: LabelKind::Segment,
        text: String::from("{bearing:dms} {distance:.3f}"),
        ..LabelStyle::default()
    }
}

fn anchor(entity: EntityId, point: AnchorPoint) -> AnchorRef {
    AnchorRef {
        entity,
        point,
        ..AnchorRef::default()
    }
}

fn point_at(position: Point2) -> Entity {
    Entity {
        geometry: Geometry::Point(PointGeometry { position }),
        ..Entity::default()
    }
}

mod annotation_tables {
    use super::*;

    #[test]
    fn each_edit_is_one_undo_step() {
        let mut d = Drawing::new();
        let mut road = TextStyle {
            name: String::from("Road"),
            paper_height: 3.5,
            ..TextStyle::default()
        };
        assert!(d.execute(create_text_style(road.clone())).is_ok());
        road.paper_height = 5.0;
        assert!(d.execute(update_text_style(road)).is_ok());
        assert!(d.execute(create_label_style(bearing_style())).is_ok());
        let rule = LabelRule {
            name: String::from("all lines"),
            label_style: String::from("Bearing"),
            ..LabelRule::default()
        };
        assert!(d.execute(create_label_rule(rule)).is_ok());
        assert_eq!(d.stack.undo_count(), 4);
        assert_eq!(d.stack.undo_name(), "CreateLabelRule");

        assert!(d.undo().is_ok());
        assert!(!d.model.label_rules.contains("all lines"));
        assert!(d.undo().is_ok());
        assert!(d.undo().is_ok());
        double_eq(
            d.model.text_styles.find("Road").expect("kept").paper_height,
            3.5,
        );
        assert!(d.redo().is_ok());
        double_eq(
            d.model.text_styles.find("Road").expect("kept").paper_height,
            5.0,
        );
    }

    #[test]
    fn a_style_still_named_is_not_deleted() {
        let mut d = Drawing::new();
        let road = TextStyle {
            name: String::from("Road"),
            ..TextStyle::default()
        };
        assert!(d.execute(create_text_style(road)).is_ok());
        let mut text = TextGeometry::new(p(0.0, 0.0), "A", 2.5, 0.0);
        text.style = String::from("Road");
        assert!(
            d.execute(create_text(text, EntityAttributes::default()))
                .is_ok()
        );
        let refused = d
            .execute(delete_text_style("Road"))
            .expect_err("the style is still worn");
        assert_eq!(refused.code, ErrorCode::CommandRejected);
        assert!(refused.context.contains("id="), "names the holder");

        assert!(
            d.execute(delete_text_style(DEFAULT_TEXT_STYLE_NAME))
                .is_err()
        );

        assert!(d.execute(create_label_style(bearing_style())).is_ok());
        let rule = LabelRule {
            name: String::from("lines"),
            label_style: String::from("Bearing"),
            ..LabelRule::default()
        };
        assert!(d.execute(create_label_rule(rule)).is_ok());
        let by_rule = d
            .execute(delete_label_style("Bearing"))
            .expect_err("a rule still names the style");
        assert!(by_rule.context.contains("label rule=lines"));
        assert!(d.execute(delete_label_rule("lines")).is_ok());
        assert!(d.execute(delete_label_style("Bearing")).is_ok());
    }

    #[test]
    fn an_annotation_naming_a_missing_style_is_refused() {
        let mut d = Drawing::new();
        let mut text = TextGeometry::new(p(0.0, 0.0), "A", 2.5, 0.0);
        text.style = String::from("Nowhere");
        assert_eq!(
            code(d.execute(create_text(text, EntityAttributes::default()))),
            ErrorCode::NotFound
        );

        let label = Entity {
            geometry: Geometry::Label(LabelGeometry {
                target: 1,
                style: String::from("Nowhere"),
                ..LabelGeometry::default()
            }),
            ..Entity::default()
        };
        let changes = ChangeSet {
            add: vec![label],
            ..ChangeSet::default()
        };
        assert!(validate_change_set(&changes, &d.model).is_err());
    }

    #[test]
    fn a_template_that_cannot_print_is_refused_by_the_command() {
        let mut d = Drawing::new();
        let style = LabelStyle {
            text: String::from("{area}"),
            ..bearing_style()
        };
        assert!(d.execute(create_label_style(style)).is_err());
        assert_eq!(d.stack.undo_count(), 0, "a refused command is no step");
    }

    // A smart leader in a label style (docs/annotation.md, "Smart leaders"):
    // the style is not deleted from under it, and a leader naming a style
    // that is not there is refused on the way in.
    #[test]
    fn a_label_style_a_leader_reads_is_not_deleted_and_must_exist() {
        let mut d = Drawing::new();
        let invert = LabelStyle {
            name: String::from("Invert"),
            kind: LabelKind::Point,
            text: String::from("IL {prop.invert:.3f}"),
            ..LabelStyle::default()
        };
        assert!(d.execute(create_label_style(invert)).is_ok());
        assert!(
            d.execute(create_entities(vec![point_at(p(0.0, 0.0))]))
                .is_ok()
        );
        let mut leader = Entity {
            geometry: Geometry::Leader(LeaderGeometry {
                vertices: vec![p(0.0, 0.0), p(5.0, 5.0)],
                tip_ref: anchor(1, AnchorPoint::Position),
                label_style: String::from("Invert"),
                ..LeaderGeometry::default()
            }),
            ..Entity::default()
        };
        assert!(d.execute(create_entities(vec![leader.clone()])).is_ok());
        let refused = d
            .execute(delete_label_style("Invert"))
            .expect_err("a leader still reads the style");
        assert_eq!(refused.code, ErrorCode::CommandRejected);
        assert!(
            refused.context.contains("used by 1 leaders, e.g. id=2"),
            "{}",
            refused.describe()
        );

        let Geometry::Leader(geometry) = &mut leader.geometry else {
            panic!("not a leader");
        };
        geometry.label_style = String::from("Nowhere");
        let changes = ChangeSet {
            add: vec![leader],
            ..ChangeSet::default()
        };
        assert_eq!(
            code(validate_change_set(&changes, &d.model)),
            ErrorCode::NotFound
        );
    }

    // An annotation copied with what it refers to refers to the copy; copied
    // alone, to the original (docs/annotation.md, "Associativity"). The ids a
    // copy gets are the next ones, in the order the copies are made - ARRAY's
    // every cell a set of its own.
    #[test]
    fn copies_of_annotation_refer_to_copies_of_what_they_refer_to() {
        let mut d = Drawing::new();
        let pit = point_at(p(0.0, 0.0));
        let line = Entity {
            geometry: Geometry::Line(Segment2::new(p(0.0, 0.0), p(10.0, 0.0))),
            ..Entity::default()
        };
        let leader = Entity {
            geometry: Geometry::Leader(LeaderGeometry {
                vertices: vec![p(0.0, 0.0), p(5.0, 5.0)],
                text: String::from("{id}"),
                tip_ref: anchor(1, AnchorPoint::Position),
                fields: true,
                ..LeaderGeometry::default()
            }),
            ..Entity::default()
        };
        let label = Entity {
            geometry: Geometry::Label(LabelGeometry {
                target: 2,
                style: String::from("S"),
                anchor: p(5.0, 0.0),
                ..LabelGeometry::default()
            }),
            ..Entity::default()
        };
        let mut dimension = DimensionGeometry::new(p(0.0, 0.0), p(10.0, 0.0), 2.0, "");
        dimension.start_ref = anchor(2, AnchorPoint::Start);
        dimension.end_ref = anchor(2, AnchorPoint::End);
        let dim = Entity {
            geometry: Geometry::Dimension(dimension),
            ..Entity::default()
        };
        let style = LabelStyle {
            name: String::from("S"),
            kind: LabelKind::Segment,
            text: String::from("{distance}"),
            ..LabelStyle::default()
        };
        assert!(d.execute(create_label_style(style)).is_ok());
        assert!(
            d.execute(create_entities(vec![pit, line, leader, label, dim]))
                .is_ok()
        );

        // Ids 1 to 5; two cells of copies are 6 to 10 and 11 to 15.
        assert!(
            d.execute(array_entities(
                vec![1, 2, 3, 4, 5],
                1,
                3,
                Vec2::new(100.0, 0.0)
            ))
            .is_ok()
        );
        for base in [5_u64, 10_u64] {
            let Geometry::Leader(leader_copy) = &d.entity(base + 3).geometry else {
                panic!("not a leader");
            };
            assert_eq!(leader_copy.tip_ref.entity, base + 1, "the copied pit");
            let Geometry::Label(label_copy) = &d.entity(base + 4).geometry else {
                panic!("not a label");
            };
            assert_eq!(label_copy.target, base + 2, "the copied line");
            let Geometry::Dimension(dim_copy) = &d.entity(base + 5).geometry else {
                panic!("not a dimension");
            };
            assert_eq!(dim_copy.start_ref.entity, base + 2);
            assert_eq!(dim_copy.end_ref.entity, base + 2);
        }
        assert!(
            d.execute(copy_entities(vec![3], Vec2::new(0.0, 50.0)))
                .is_ok()
        );
        let Geometry::Leader(copied) = &d.entity(16).geometry else {
            panic!("not a leader");
        };
        assert_eq!(
            copied.tip_ref.entity, 1,
            "copied alone: another callout of the same pit"
        );
    }
}
