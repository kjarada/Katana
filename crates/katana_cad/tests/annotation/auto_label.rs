// Labels made by hand and by rule (annotation/auto_label.rs, docs/annotation.md
// "Auto-labelling"; tests/cad/annotation/test_auto_label.cpp): which entities a
// rule matches, one undo step per run, re-running as a no-op, keeping what a
// person dragged, and clearing only what the rules made.

use katana_cad::Document;
use katana_cad::annotation::auto_label::{
    AutoLabelReport, LabelRequest, auto_label, clear_auto_labels, create_label,
    default_label_styles, rule_matches,
};
use katana_commands::{
    create_alignment, create_entities, create_label_rule, create_label_style, create_layer,
    set_entity_geometry, set_entity_property,
};
use katana_entity::{
    Alignment, Entity, EntityId, Geometry, LabelGeometry, LabelRule, Layer, PointGeometry,
    PropertyValue, validate,
};
use katana_geometry::{AlignmentPi, Point2, Polyline2, Segment2};

use crate::ok;

// A drawing with the default label styles, three coded points on
// survey/points, a boundary on cadastre and a line on design.
struct Site {
    document: Document,
    points: Vec<EntityId>,
    design: EntityId,
}

impl Site {
    fn new() -> Self {
        let mut document = Document::default();
        for style in default_label_styles() {
            ok(document.execute(create_label_style(style)));
        }
        for name in ["survey/points", "cadastre", "design"] {
            ok(document.execute(create_layer(Layer {
                name: String::from(name),
                ..Layer::default()
            })));
        }
        let codes = ["EP", "EP", "TREE"];
        let mut points = Vec::new();
        for (i, code) in codes.iter().enumerate() {
            let mut point = Entity {
                geometry: Geometry::Point(PointGeometry {
                    position: Point2::new(10.0 * i as f64, 0.0),
                }),
                layer: String::from("survey/points"),
                ..Entity::default()
            };
            point.properties.insert(
                String::from("code"),
                PropertyValue::Text(String::from(*code)),
            );
            point.properties.insert(
                String::from("point"),
                PropertyValue::Integer(100 + i as i64),
            );
            point.properties.insert(
                String::from("elevation"),
                PropertyValue::Real(20.0 + i as f64),
            );
            ok(document.execute(create_entities(vec![point])));
            points.push(document.last_created_entities()[0]);
        }
        let lot = Entity {
            geometry: Geometry::Polyline(Polyline2::new(
                vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(30.0, 0.0),
                    Point2::new(30.0, 20.0),
                    Point2::new(0.0, 20.0),
                ],
                true,
            )),
            layer: String::from("cadastre"),
            ..Entity::default()
        };
        ok(document.execute(create_entities(vec![lot])));
        let line = Entity {
            geometry: Geometry::Line(Segment2::new(
                Point2::new(0.0, 50.0),
                Point2::new(40.0, 50.0),
            )),
            layer: String::from("design"),
            ..Entity::default()
        };
        ok(document.execute(create_entities(vec![line])));
        let design = document.last_created_entities()[0];
        Self {
            document,
            points,
            design,
        }
    }

    fn rule(&mut self, rule: LabelRule) {
        ok(self.document.execute(create_label_rule(rule)));
    }

    fn labels(&self) -> Vec<LabelGeometry> {
        let mut found = Vec::new();
        self.document.model().entities.for_each(|entity| {
            if let Geometry::Label(label) = &entity.geometry {
                found.push(label.clone());
            }
        });
        found
    }

    #[track_caller]
    fn entity(&self, id: EntityId) -> &Entity {
        self.document.model().entities.find(id).expect("the entity")
    }
}

fn rule(name: &str, label_style: &str) -> LabelRule {
    LabelRule {
        name: String::from(name),
        label_style: String::from(label_style),
        ..LabelRule::default()
    }
}

fn names(rules: &[&str]) -> Vec<String> {
    rules.iter().map(|&name| String::from(name)).collect()
}

mod auto_label {
    use super::*;

    #[test]
    fn rules_match_by_layer_code_and_type() {
        let site = Site::new();
        let spot = site
            .document
            .model()
            .label_styles
            .find("Spot Level")
            .expect("the style");
        let mut ep = LabelRule {
            layer: String::from("survey"),
            code: String::from("ep"),
            ..rule("ep", "Spot Level")
        };
        assert!(
            rule_matches(&ep, spot, site.entity(site.points[0])),
            "the layer's ancestor matches, and the code without case"
        );
        assert!(
            !rule_matches(&ep, spot, site.entity(site.points[2])),
            "TREE"
        );
        ep.code.clear();
        ep.layer = String::from("design");
        assert!(!rule_matches(&ep, spot, site.entity(site.points[0])));
        ep.layer.clear();
        assert!(
            !rule_matches(&ep, spot, site.entity(site.design)),
            "a point style cannot label a line"
        );
        ep.enabled = false;
        assert!(!rule_matches(&ep, spot, site.entity(site.points[0])));
    }

    #[test]
    fn a_run_is_one_step_and_a_rerun_changes_nothing() {
        let mut site = Site::new();
        site.rule(LabelRule {
            code: String::from("EP"),
            label_layer: String::from("labels/levels"),
            ..rule("levels", "Spot Level")
        });
        site.rule(LabelRule {
            layer: String::from("cadastre"),
            ..rule("bearings", "Bearing Distance")
        });
        let steps = site.document.history().undo_count();
        let mut report = AutoLabelReport::default();
        let run = auto_label(site.document.model(), &[], Some(&mut report))
            .unwrap_or_else(|error| panic!("{}", error.describe()))
            .expect("a command");
        ok(site.document.execute(run));
        assert_eq!(report.created, 3, "two EP points and the boundary");
        assert_eq!(report.per_rule.get("levels"), Some(&2));
        assert_eq!(site.document.history().undo_count(), steps + 1);
        assert!(
            site.document.model().layers.contains("labels/levels"),
            "the rule's layer made in the same step"
        );

        // Again: nothing to do, and no empty undo step.
        let again = auto_label(site.document.model(), &[], Some(&mut report))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(again.is_none());
        assert_eq!(report.kept, 3);

        ok(site.document.undo());
        assert!(site.labels().is_empty());
        assert!(!site.document.model().layers.contains("labels/levels"));
    }

    #[test]
    fn a_rerun_keeps_a_dragged_label_and_drops_ones_no_longer_asked_for() {
        let mut site = Site::new();
        site.rule(LabelRule {
            code: String::from("EP"),
            ..rule("levels", "Spot Level")
        });
        let run = auto_label(site.document.model(), &[], None)
            .unwrap_or_else(|error| panic!("{}", error.describe()))
            .expect("a command");
        ok(site.document.execute(run));
        // Drag one label, recode one point.
        let mut dragged_id: EntityId = 0;
        site.document.model().entities.for_each(|entity| {
            if matches!(entity.geometry, Geometry::Label(_)) && dragged_id == 0 {
                dragged_id = entity.id;
            }
        });
        let mut dragged = site.entity(dragged_id).clone();
        let mut dragged_target = 0;
        if let Geometry::Label(label) = &mut dragged.geometry {
            label.position = Some(Point2::new(-5.0, -5.0));
            dragged_target = label.target;
        }
        ok(site
            .document
            .execute(set_entity_geometry(dragged_id, dragged.geometry)));
        let other = if dragged_target == site.points[0] {
            site.points[1]
        } else {
            site.points[0]
        };
        ok(site.document.execute(set_entity_property(
            vec![other],
            "code",
            PropertyValue::Text(String::from("KERB")),
        )));

        let mut report = AutoLabelReport::default();
        let rerun = auto_label(site.document.model(), &[], Some(&mut report))
            .unwrap_or_else(|error| panic!("{}", error.describe()))
            .expect("a command");
        ok(site.document.execute(rerun));
        assert_eq!(report.kept, 1);
        assert_eq!(report.removed, 1);
        assert!(
            site.document.model().entities.contains(dragged_id),
            "the same label, same id"
        );
        let Geometry::Label(label) = &site.entity(dragged_id).geometry else {
            panic!("a label");
        };
        assert_eq!(label.position, Some(Point2::new(-5.0, -5.0)));
    }

    #[test]
    fn clearing_removes_only_what_the_rules_made() {
        let mut site = Site::new();
        site.rule(LabelRule {
            code: String::from("EP"),
            ..rule("levels", "Spot Level")
        });
        let run = auto_label(site.document.model(), &[], None)
            .unwrap_or_else(|error| panic!("{}", error.describe()))
            .expect("a command");
        ok(site.document.execute(run));
        // One by hand.
        let request = LabelRequest {
            target: site.points[2],
            style: String::from("Point Number"),
            ..LabelRequest::default()
        };
        let by_hand = create_label(site.document.model(), &request)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        ok(site.document.execute(by_hand));
        assert_eq!(site.labels().len(), 3);

        let mut removed = 0;
        let clear = clear_auto_labels(
            site.document.model(),
            &names(&["levels"]),
            Some(&mut removed),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()))
        .expect("a command");
        ok(site.document.execute(clear));
        assert_eq!(removed, 2);
        let labels = site.labels();
        assert_eq!(labels.len(), 1);
        assert!(labels[0].rule.is_empty(), "the hand-placed one stays");
        assert!(clear_auto_labels(site.document.model(), &names(&["nothing"]), None).is_err());
    }

    #[test]
    fn a_label_that_would_say_nothing_is_not_made() {
        let mut site = Site::new();
        // A point with no level cannot carry a spot level.
        let bare = Entity {
            geometry: Geometry::Point(PointGeometry {
                position: Point2::new(5.0, 5.0),
            }),
            layer: String::from("survey/points"),
            ..Entity::default()
        };
        ok(site.document.execute(create_entities(vec![bare])));
        let id = site.document.last_created_entities()[0];
        let request = |style: &str| LabelRequest {
            target: id,
            style: String::from(style),
            ..LabelRequest::default()
        };
        let Err(refused) = create_label(site.document.model(), &request("Spot Level")) else {
            panic!("a label that says nothing is refused");
        };
        assert!(
            refused.message.contains("say nothing"),
            "{}",
            refused.describe()
        );
        assert!(
            create_label(site.document.model(), &request("Bearing Distance")).is_err(),
            "a segment style on a point"
        );
        assert!(create_label(site.document.model(), &request("Missing")).is_err());
    }

    #[test]
    fn chainage_rules_label_alignments() {
        let mut site = Site::new();
        let mut road = Alignment {
            name: String::from("road"),
            ..Alignment::default()
        };
        road.horizontal.pis = vec![
            AlignmentPi {
                point: Point2::new(0.0, 0.0),
                ..AlignmentPi::default()
            },
            AlignmentPi {
                point: Point2::new(100.0, 0.0),
                ..AlignmentPi::default()
            },
        ];
        ok(site.document.execute(create_alignment(road)));
        site.rule(rule("chainages", "Chainage"));
        let mut report = AutoLabelReport::default();
        let run = auto_label(
            site.document.model(),
            &names(&["chainages"]),
            Some(&mut report),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()))
        .expect("a command");
        ok(site.document.execute(run));
        assert_eq!(report.created, 1);
        assert_eq!(site.labels()[0].alignment, "road");
    }

    #[test]
    fn the_default_styles_are_all_valid() {
        for style in default_label_styles() {
            assert!(validate(&style).is_ok(), "{}", style.name);
        }
    }
}
