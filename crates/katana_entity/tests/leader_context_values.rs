//! The twin of tests/entity/annotation/test_leader_context_values.cpp: what a
//! leader reads beyond the entity its tip is on - the drawing's surfaces and
//! alignments, through a LeaderContext - and the +.Nf step that writes a
//! difference with its sign.
//!
//! The context here is a fake made of planes and a straight line, so every
//! expected value is worked out by hand in the comment beside it:
//!
//!   ground           z = 10 + 0.1 x + 0.2 y over the square 0..100 x 0..100,
//!                    nothing outside it
//!   Existing Ground  z = 5 over the same square (a name with a blank)
//!   MC01             the x axis from (0, 0) to (100, 0): station = x, offset
//!                    = y (north is LEFT looking east), and a design profile
//!                    20 + 0.01 x from station 0 to 80 only

use std::sync::Arc;

use katana_entity::label_text::{check_label_template, format_label, format_value};
use katana_entity::leader_values::{
    anchor_values, check_leader_says_something, check_leader_template, context_value_name,
    leader_context_kinds, leader_note, leader_value_quantity, leader_values, why_absent,
};
use katana_entity::{
    AlignmentPlace, AnchorPoint, AnchorRef, Entity, EntityId, Geometry, LabelKind, LabelQuantity,
    LabelValue, LabelValues, LeaderContext, LeaderGeometry, Model, PointGeometry, PropertyValue,
};
use katana_geometry::{Point2, Vec2};

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn in_square(at: Point2) -> bool {
    (0.0..=100.0).contains(&at.x) && (0.0..=100.0).contains(&at.y)
}

fn fake_context() -> LeaderContext {
    LeaderContext {
        surfaces: vec![String::from("ground"), String::from("Existing Ground")],
        alignments: vec![String::from("MC01")],
        surface_level: Some(Arc::new(|name: &str, at: Point2| -> Option<f64> {
            if !in_square(at) {
                return None;
            }
            if name == "ground" {
                return Some(10.0 + 0.1 * at.x + 0.2 * at.y);
            }
            if name == "Existing Ground" {
                return Some(5.0);
            }
            None
        })),
        alignment_place: Some(Arc::new(
            |name: &str, at: Point2| -> Option<AlignmentPlace> {
                if name != "MC01" || !(0.0..=100.0).contains(&at.x) {
                    return None;
                }
                let mut place = AlignmentPlace {
                    station: at.x,
                    offset: at.y,
                    profile: None,
                };
                if at.x <= 80.0 {
                    place.profile = Some(20.0 + 0.01 * at.x);
                }
                Some(place)
            },
        )),
    }
}

/// A model holding one point at `at`, with a level when `z` is given; its id.
struct OnePoint {
    model: Model,
    id: EntityId,
}

fn point_at(at: Point2, z: Option<f64>) -> OnePoint {
    let mut point = Entity {
        geometry: Geometry::Point(PointGeometry { position: at }),
        ..Entity::default()
    };
    if let Some(z) = z {
        point
            .properties
            .insert(String::from("elevation"), PropertyValue::Real(z));
    }
    let mut model = Model::new();
    assert!(model.entities.add(point).is_ok());
    let id = model.entities.ids()[0];
    OnePoint { model, id }
}

fn smart_leader(on: &OnePoint, template_text: &str) -> LeaderGeometry {
    let Geometry::Point(point) = &on.model.entities.find(on.id).unwrap().geometry else {
        panic!("not a point")
    };
    let at = point.position;
    LeaderGeometry {
        vertices: vec![at, at + Vec2::new(5.0, 5.0)],
        tip_ref: AnchorRef {
            entity: on.id,
            point: AnchorPoint::Position,
            ..AnchorRef::default()
        },
        text: String::from(template_text),
        fields: true,
        ..LeaderGeometry::default()
    }
}

fn note(on: &OnePoint, template_text: &str, context: &LeaderContext) -> String {
    leader_note(&on.model, &smart_leader(on, template_text), &[], context)
}

fn said(values: &LabelValues, name: &str) -> String {
    values
        .get(name)
        .map_or_else(|| String::from("absent"), format_value)
}

fn refusal_of(on: &OnePoint, template_text: &str, context: &LeaderContext) -> String {
    let status =
        check_leader_says_something(&on.model, &smart_leader(on, template_text), &[], context);
    match status {
        Ok(()) => panic!("{template_text} was not refused"),
        Err(error) => error.message,
    }
}

mod leader_context_values {
    use super::*;

    #[test]
    fn a_surface_is_read_under_the_tip_and_the_targets_height_above_it_is_positive() {
        // (30, 40) at RL 25: ground there is 10 + 3 + 8 = 21, so the point is 4
        // above it - a fill of 4, and no cut.
        let on = point_at(p(30.0, 40.0), Some(25.0));
        assert_eq!(
            note(
                &on,
                "GL {surface.ground:.3f}\n{dsurface.ground:+.3f}\nCUT {cut.ground:.2f}\nFILL {fill.ground:.2f}",
                &fake_context()
            ),
            "GL 21.000\n+4.000\nFILL 4.00"
        );
    }

    #[test]
    fn cut_and_fill_say_only_the_one_that_holds_and_on_the_surface_is_a_fill_of_nothing() {
        let context = fake_context();
        let cut_fill = "CUT {cut.ground:.2f}\nFILL {fill.ground:.2f}";
        // RL 18 under ground 21: 3 below, a cut of 3; its difference -3.
        let below = point_at(p(30.0, 40.0), Some(18.0));
        assert_eq!(note(&below, cut_fill, &context), "CUT 3.00");
        assert_eq!(note(&below, "{dsurface.ground:+.3f}", &context), "-3.000");
        // RL 21 exactly on ground 21: neither above nor below, said as a fill
        // of nothing, and a difference of nothing has no sign.
        let on = point_at(p(30.0, 40.0), Some(21.0));
        assert_eq!(note(&on, cut_fill, &context), "FILL 0.00");
        assert_eq!(note(&on, "{dsurface.ground:+.3f}", &context), "0.000");
    }

    #[test]
    fn a_name_with_a_blank_is_written_in_the_template_as_it_is() {
        // Existing Ground is 5 everywhere in the square: RL 25 is 20 above.
        let on = point_at(p(30.0, 40.0), Some(25.0));
        assert_eq!(
            note(
                &on,
                "{surface.Existing Ground:.1f} {dsurface.Existing Ground:+.1f}",
                &fake_context()
            ),
            "5.0 +20.0"
        );
        assert!(check_leader_template("{dsurface.Existing Ground:+.3f}").is_ok());
    }

    #[test]
    fn a_station_is_read_along_the_alignment_with_its_offset_and_the_profile() {
        // (30, -4) at RL 25 on MC01: station 30, 4 to the right (south of an
        // eastward line), design level 20 + 0.3 = 20.3, so 4.7 above the design.
        let on = point_at(p(30.0, -4.0), Some(25.0));
        assert_eq!(
            note(
                &on,
                "CH {station.MC01:ch}\nOS {offset.MC01:+.3f}\nDL {profile.MC01:.3f}\n{dprofile.MC01:+.3f}",
                &fake_context()
            ),
            "CH 0+030.000\nOS -4.000\nDL 20.300\n+4.700"
        );
    }

    #[test]
    fn what_cannot_be_read_drops_its_line_and_a_refusal_says_why() {
        let context = fake_context();
        // Off the surface: (150, 40) is outside the square.
        let off = point_at(p(150.0, 40.0), Some(25.0));
        assert!(
            refusal_of(&off, "{dsurface.ground:+.3f}", &context)
                .contains("dsurface.ground (the tip is off surface ground)")
        );
        // No level on the point: the ground under it is still read, the
        // difference is not.
        let flat = point_at(p(30.0, 40.0), None);
        assert_eq!(
            note(&flat, "{surface.ground:.3f}\n{dsurface.ground}", &context),
            "21.000"
        );
        assert!(refusal_of(&flat, "{fill.ground}", &context).contains("the target has no level"));
        // A surface the drawing has not, and an alignment.
        let on = point_at(p(30.0, 40.0), Some(25.0));
        assert!(
            refusal_of(&on, "{surface.grnd}", &context).contains("the drawing has no surface grnd")
        );
        assert!(
            refusal_of(&on, "{station.MC02}", &context)
                .contains("the drawing has no alignment MC02")
        );
        // The cut of a point above the surface.
        assert!(
            refusal_of(&on, "{cut.ground}", &context)
                .contains("the target is not below surface ground")
        );
        // Beyond the alignment's end, and past its profile's.
        let beyond = point_at(p(120.0, 5.0), Some(25.0));
        assert!(
            refusal_of(&beyond, "{station.MC01}", &context)
                .contains("the tip is beyond the ends of alignment MC01")
        );
        let late = point_at(p(90.0, 0.0), Some(25.0));
        assert!(
            refusal_of(&late, "{profile.MC01}", &context)
                .contains("alignment MC01 has no design profile at station 90.000")
        );
    }

    #[test]
    fn with_no_context_a_surfaces_value_is_absent_never_zero() {
        let on = point_at(p(30.0, 40.0), Some(25.0));
        let none = LeaderContext::default();
        let leader = smart_leader(&on, "RL {rl:.3f}\n{dsurface.ground:+.3f}");
        assert_eq!(leader_note(&on.model, &leader, &[], &none), "RL 25.000");
        let values = leader_values(&on.model, &leader, &[], &none).unwrap();
        assert_eq!(said(&values, "dsurface.ground"), "absent");
        assert_eq!(
            why_absent("dsurface.ground", &values, p(30.0, 40.0), &none),
            "no surfaces were given to read here"
        );
    }

    #[test]
    fn anchor_values_list_every_value_of_each_surface_and_alignment_that_holds() {
        let on = point_at(p(30.0, 40.0), Some(25.0));
        let point = on.model.entities.find(on.id).unwrap();
        let reference = AnchorRef {
            entity: on.id,
            point: AnchorPoint::Position,
            ..AnchorRef::default()
        };
        let values = anchor_values(point, &reference, p(30.0, 40.0), &[], &fake_context());
        assert_eq!(said(&values, "surface.ground"), "21.000");
        assert_eq!(said(&values, "dsurface.ground"), "4.000");
        assert_eq!(said(&values, "fill.ground"), "4.000");
        assert_eq!(
            said(&values, "cut.ground"),
            "absent",
            "above the surface: no cut"
        );
        assert_eq!(said(&values, "surface.Existing Ground"), "5.000");
        assert_eq!(said(&values, "dsurface.Existing Ground"), "20.000");
        // Station 30 printed as a chainage's own (a plain 3-decimal number),
        // offset 40 left, design 20.3, 4.7 above it.
        assert_eq!(said(&values, "station.MC01"), "30.000");
        assert_eq!(said(&values, "offset.MC01"), "40.000");
        assert_eq!(said(&values, "profile.MC01"), "20.300");
        assert_eq!(said(&values, "dprofile.MC01"), "4.700");
        // Without a context, none of them.
        let bare = anchor_values(
            point,
            &reference,
            p(30.0, 40.0),
            &[],
            &LeaderContext::default(),
        );
        assert_eq!(said(&bare, "surface.ground"), "absent");
        assert_eq!(said(&bare, "rl"), "25.000");
    }

    #[test]
    fn the_template_check_knows_every_kind_whatever_the_name_and_its_quantity() {
        for kind in leader_context_kinds() {
            assert!(
                leader_value_quantity(&format!("{kind}.anything")).is_some(),
                "{kind}"
            );
            assert!(
                leader_value_quantity(&format!("{kind}.")).is_none(),
                "{kind}"
            );
        }
        assert_eq!(leader_context_kinds().len(), 8);
        assert_eq!(
            leader_value_quantity("station.MC01"),
            Some(LabelQuantity::Chainage)
        );
        assert_eq!(
            leader_value_quantity("offset.MC01"),
            Some(LabelQuantity::Length)
        );
        assert_eq!(
            leader_value_quantity("dsurface.ground"),
            Some(LabelQuantity::Number)
        );
        assert!(leader_value_quantity("ground.surface").is_none());
        assert!(
            check_leader_template("CH {station.MC01:ch}").is_ok(),
            "a station is a chainage"
        );
        assert!(
            check_leader_template("{offset.MC01:ch}").is_err(),
            "an offset is not"
        );
        assert!(
            check_leader_template("{surface.ground:dms}").is_err(),
            "a level is no angle"
        );
        assert!(
            check_leader_template("{surface.:.3f}").is_err(),
            "a kind with no name"
        );
        let parsed = context_value_name("dsurface.Existing Ground").unwrap();
        assert_eq!(parsed.kind, "dsurface");
        assert_eq!(parsed.name, "Existing Ground");
        assert!(context_value_name("prop.invert").is_none());
    }

    #[test]
    fn a_plus_format_writes_the_sign_of_a_value_above_nothing_only() {
        let with = |dz: f64| {
            let mut values = LabelValues::new();
            values.insert(
                String::from("dz"),
                LabelValue::of(LabelQuantity::Number, dz),
            );
            format_label("{dz:+.3f}", &values)
        };
        assert_eq!(with(0.25), "+0.250");
        assert_eq!(with(-0.25), "-0.250");
        // Both round to nothing at three decimals, and nothing has no sign.
        assert_eq!(with(0.0004), "0.000");
        assert_eq!(with(-0.0004), "0.000");
        // Rounded half away from zero first: 2.5 is +3.
        let mut half = LabelValues::new();
        half.insert(
            String::from("dz"),
            LabelValue::of(LabelQuantity::Number, 2.5),
        );
        assert_eq!(format_label("{dz:+.0f}", &half), "+3");
        assert!(check_label_template("{dz:+.3f}", LabelKind::Segment).is_ok());
        assert!(
            check_label_template("{layer:+.3f}", LabelKind::Segment).is_err(),
            "a text has no sign to write"
        );
        assert!(
            check_label_template("{dz:+.3f:m}", LabelKind::Segment).is_err(),
            "a number format is the last step"
        );
    }

    #[test]
    fn a_point_on_the_surface_within_the_tolerance_is_a_fill_of_nothing_not_a_cut_of_the_rounding_error()
     {
        // The surface's level is interpolated, so a point exactly on it can
        // come out a few bits either side. Here the surface is one ulp above
        // the point's 21 at (30, 40), so level - ground is -3.55e-15, and the
        // exact comparison called it a cut. Within the geometric tolerance
        // (1e-7) is on it.
        let context_at = |ground: f64| LeaderContext {
            surface_level: Some(Arc::new(move |name: &str, at: Point2| -> Option<f64> {
                (name == "ground" && in_square(at)).then_some(ground)
            })),
            ..fake_context()
        };
        let on = point_at(p(30.0, 40.0), Some(21.0));
        let cut_fill = "CUT {cut.ground:.2f}\nFILL {fill.ground:.2f}";
        // std::nextafter(21.0, 100.0) and (21.0, 0.0): the neighbouring
        // doubles.
        let a_ulp_above = f64::from_bits(21.0_f64.to_bits() + 1);
        let a_ulp_below = f64::from_bits(21.0_f64.to_bits() - 1);
        assert!(a_ulp_above > 21.0);
        assert!(a_ulp_above - 21.0 < 1.0e-14);
        assert_eq!(note(&on, cut_fill, &context_at(a_ulp_above)), "FILL 0.00");
        // And the other way round, a ulp below: a fill, as it always was.
        assert_eq!(note(&on, cut_fill, &context_at(a_ulp_below)), "FILL 0.00");
        // A fill-only note on such a point is accepted, not refused as "below".
        assert!(
            check_leader_says_something(
                &on.model,
                &smart_leader(&on, "FILL {fill.ground:.3f}"),
                &[],
                &context_at(a_ulp_above)
            )
            .is_ok()
        );
        // Inside the tolerance (5e-8 of the 1e-7) is on the surface; outside it
        // (1e-6) is a real cut of a micrometre, said as one - and not a fill.
        assert_eq!(note(&on, cut_fill, &context_at(21.0 + 5.0e-8)), "FILL 0.00");
        assert_eq!(note(&on, cut_fill, &context_at(21.0 + 1.0e-6)), "CUT 0.00");
        assert!(
            check_leader_says_something(
                &on.model,
                &smart_leader(&on, "FILL {fill.ground:.3f}"),
                &[],
                &context_at(21.0 + 1.0e-6)
            )
            .is_err()
        );
    }
}
