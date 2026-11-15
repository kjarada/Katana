//! tests/cad/annotation/test_smart_leaders.cpp: smart leaders
//! (docs/annotation.md, "Smart leaders"): leaders whose tip is on an entity
//! and whose note is read off it, driven as an agent drives them - the LEADER
//! verbs, one undo step per edit - and followed through the edits that move,
//! copy and erase what they are on.
//!
//! Expected values are worked out by hand in the comments beside them.

use std::collections::BTreeMap;

use katana_cad::annotation::drawing::estimated_measure;
use katana_cad::annotation::leader_draw::build_leader;
use katana_commands::{copy_entities, move_entities, set_entity_geometry};
use katana_entity::leader_values::{is_smart, leader_note};
use katana_entity::{
    AnchorPoint, ArrowHead, CalloutShape, DimensionGeometry, Entity, EntityId, Geometry,
    LeaderContext, LeaderGeometry, PointGeometry, PropertyMap, PropertyValue,
};
use katana_geometry::{Circle2, Point2, Polyline2, Segment2, Vec2};
use katana_math::PI;

use crate::{Session, double_eq, near, pt, real_of};

// Makes `geometry` with `properties` as one step and gives its id.
#[track_caller]
fn add(s: &mut Session, geometry: Geometry, properties: PropertyMap) -> EntityId {
    s.add(Entity {
        geometry,
        properties,
        ..Entity::default()
    })
}

fn point_at(at: Point2) -> Geometry {
    Geometry::Point(PointGeometry { position: at })
}

fn line_of(start: Point2, end: Point2) -> Geometry {
    Geometry::Line(Segment2::new(start, end))
}

fn closed(vertices: Vec<Point2>) -> Geometry {
    Geometry::Polyline(Polyline2::new(vertices, true))
}

// One property.
fn with(key: &str, value: PropertyValue) -> PropertyMap {
    PropertyMap::from([(String::from(key), value)])
}

// The note leader `id` says, read off its target with no surface or
// alignment: entity's leader_note with the C++ default arguments.
#[track_caller]
fn note(s: &Session, id: EntityId) -> String {
    leader_note(
        s.document.model(),
        &s.leader(id),
        &[],
        &LeaderContext::default(),
    )
}

// The first vertex: the arrow's tip.
#[track_caller]
fn tip(s: &Session, id: EntityId) -> Point2 {
    s.leader(id).vertices[0]
}

// The last vertex: where the note hangs.
#[track_caller]
fn hang(s: &Session, id: EntityId) -> Point2 {
    match s.leader(id).vertices.last() {
        Some(&at) => at,
        None => panic!("leader {id} has no vertex"),
    }
}

mod smart_leaders {
    use super::*;

    // ---- making one ----------------------------------------------------------------------------

    #[test]
    fn a_template_is_read_off_the_pit_and_follows_its_attributes() {
        let mut s = Session::default();
        let pit = add(
            &mut s,
            point_at(pt(10.0, 10.0)),
            with("elevation", PropertyValue::Real(12.5)),
        );
        let made = s.run(
            &(format!("LEADER #{pit} 20,20 ")
                + "template=\"PIT {id}\\nRL {rl:.2f}\\nIL {prop.invert:.2f}\""),
        );
        let leader = s.last();
        // The IL line names a property the pit does not carry yet: dropped.
        assert_eq!(note(&s, leader), format!("PIT {pit}\nRL 12.50"));
        assert!(made.contains("associative=yes"), "{made}");
        assert!(made.contains(&format!("target={pit}")), "{made}");
        assert!(made.contains("template="), "{made}");
        assert!(s.leader(leader).fields);

        // Setting the attribute through the leader is one step on the pit, and
        // the note says it at once.
        let before = s.steps();
        let set = s.run(&format!("LEADER PROP {leader} SET invert 10.5"));
        assert_eq!(s.steps(), before + 1);
        assert!(set.contains("IL 10.50"), "{set}");
        let properties = s.entity(pit).properties;
        assert!(properties.contains_key("invert"));
        assert_eq!(real_of(properties.get("invert")), 10.5);
        assert_eq!(note(&s, leader), format!("PIT {pit}\nRL 12.50\nIL 10.50"));
        s.run(&format!("LEADER PROP {leader} SET invert 9 integer"));
        assert_eq!(note(&s, leader), format!("PIT {pit}\nRL 12.50\nIL 9.00"));
        s.run(&format!("LEADER PROP {leader} DELETE invert"));
        assert_eq!(note(&s, leader), format!("PIT {pit}\nRL 12.50"));
        s.run("UNDO");
        assert_eq!(note(&s, leader), format!("PIT {pit}\nRL 12.50\nIL 9.00"));
    }

    #[test]
    fn what_cannot_be_read_is_refused_with_what_is_missing() {
        let mut s = Session::default();
        let pit = add(&mut s, point_at(pt(0.0, 0.0)), PropertyMap::new());
        let at = format!("LEADER #{pit} 5,5 ");
        let line = |rest: &str| format!("{at}{rest}");
        assert!(
            s.refusal(&line("template=\"IL {prop.invrt}\""))
                .contains("no prop.invrt")
        );
        assert!(
            s.refusal(&line("template=\"{invert}\""))
                .contains("a leader has no value 'invert'")
        );
        assert!(
            s.refusal("LEADER 0,0 5,5 template=\"{type}\"")
                .contains("reads the entity its tip is on")
        );
        assert!(
            s.refusal(&line("text=A template=\"{type}\""))
                .contains("one of text=")
        );
        assert!(
            s.refusal(&line("labelstyle=Nope"))
                .contains("label style does not exist")
        );
        assert_eq!(s.document.model().entities.size(), 1, "nothing made");

        // A plain note is what was typed, braces and all.
        s.run("LEADER 0,0 5,5 text=\"{not a field}\"");
        assert_eq!(note(&s, s.last()), "{not a field}");
    }

    #[test]
    fn a_tip_on_a_line_stays_at_its_place_along_it() {
        let mut s = Session::default();
        let line = add(
            &mut s,
            line_of(pt(0.0, 0.0), pt(40.0, 0.0)),
            PropertyMap::new(),
        );
        // #line@10,3 is the point of the line nearest (10, 3): (10, 0), a
        // quarter of the way along.
        s.run(&(format!("LEADER #{line}@10,3 20,10 ") + "template=\"CH {chainage:.1f}\""));
        let leader = s.last();
        assert_eq!(tip(&s, leader), pt(10.0, 0.0));
        assert_eq!(s.leader(leader).tip_ref.point, AnchorPoint::Along);
        double_eq(s.leader(leader).tip_ref.parameter, 0.25);
        assert_eq!(note(&s, leader), "CH 10.0");

        // Stretched to 80 m, the tip keeps its quarter - (20, 0), chainage 20 -
        // in the same step as the stretch.
        let before = s.steps();
        assert!(
            s.document
                .execute(set_entity_geometry(
                    line,
                    line_of(pt(0.0, 0.0), pt(80.0, 0.0))
                ))
                .is_ok()
        );
        assert_eq!(s.steps(), before + 1);
        assert_eq!(tip(&s, leader), pt(20.0, 0.0));
        assert_eq!(note(&s, leader), "CH 20.0");
        // Moved, the tip goes with it.
        assert!(
            s.document
                .execute(move_entities(vec![line], Vec2::new(0.0, 5.0)))
                .is_ok()
        );
        assert_eq!(tip(&s, leader), pt(20.0, 5.0));
        assert!(s.document.undo().is_ok());
        assert!(s.document.undo().is_ok());
        assert_eq!(tip(&s, leader), pt(10.0, 0.0), "one undo each");
    }

    #[test]
    fn an_exact_place_along_is_typed_as_long_as_the_entity_has_it() {
        let mut s = Session::default();
        // A closed 100 x 100 square: segment 3 (numbered from 0; the value
        // "segment" counts from 1) closes it, from (0,100) down to (0,0).
        let square = add(
            &mut s,
            closed(vec![
                pt(0.0, 0.0),
                pt(100.0, 0.0),
                pt(100.0, 100.0),
                pt(0.0, 100.0),
            ]),
            PropertyMap::new(),
        );
        // A quarter of the way down segment 3 is (0,75), chainage 300 + 25.
        s.run(
            &(format!("LEADER #{square}.along3:0.25 -20,75 ")
                + "template=\"S{segment} CH {chainage:.0f}\""),
        );
        let quarter = s.last();
        assert_eq!(tip(&s, quarter), pt(0.0, 75.0));
        assert_eq!(s.leader(quarter).tip_ref.index, 3);
        assert_eq!(s.leader(quarter).tip_ref.parameter, 0.25);
        assert_eq!(note(&s, quarter), "S4 CH 325");
        assert!(
            s.run(&format!("LEADER LIST {quarter}"))
                .contains("anchor=\"along 3 0.25\""),
            "LIST prints the place as it is typed back"
        );

        // The end of segment 3 and the start of segment 0 are one point, (0,0);
        // only .along names which: chainage 400, or the nearest place's 0.
        s.run(&(format!("LEADER #{square}.along3:1 -20,-20 ") + "template=\"CH {chainage:.0f}\""));
        assert_eq!(note(&s, s.last()), "CH 400");
        s.run(&(format!("LEADER #{square}@0,0 -20,-20 ") + "template=\"CH {chainage:.0f}\""));
        assert_eq!(note(&s, s.last()), "CH 0");

        // A circle's whole turn is where it starts, as the turn is [0, 1).
        let ring = add(
            &mut s,
            Geometry::Circle(Circle2::new(pt(0.0, 0.0), 5.0)),
            PropertyMap::new(),
        );
        s.run(&format!("LEADER #{ring}.along0:1 10,10 text=R"));
        assert_eq!(s.leader(s.last()).tip_ref.parameter, 0.0);

        // Refused: a fraction off the segment or no number, a segment the
        // square has not, the fraction left out, a segment of a line.
        let line = add(
            &mut s,
            line_of(pt(0.0, 0.0), pt(10.0, 0.0)),
            PropertyMap::new(),
        );
        for place in [
            ".along3:1.5",
            ".along3:nan",
            ".along3:-0.1",
            ".along4:0.5",
            ".along",
            ".along3",
            ".alongx:0.5",
        ] {
            assert!(
                !s.refusal(&format!("LEADER #{square}{place} 5,5 text=X"))
                    .is_empty(),
                "{place}"
            );
        }
        assert!(
            s.refusal(&format!("LEADER #{line}.along1:0.5 5,5 text=X"))
                .contains("only a polyline has segments")
        );
    }

    #[test]
    fn a_backslash_is_typed_as_two_so_a_note_may_hold_backslash_n() {
        let mut s = Session::default();
        // Typed: C:\\new and a line break. Stored: C:\new on one line, then B.
        s.run("LEADER 0,0 5,5 text=\"C:\\\\new\\nB\"");
        let leader = s.last();
        assert_eq!(s.leader(leader).text, "C:\\new\nB");
        // LIST writes it as it is typed, so its words typed back are the same.
        let listed = s.run(&format!("LEADER LIST {leader}"));
        assert!(listed.contains("text=\"C:\\\\new\\nB\""), "{listed}");
        s.run(&format!("LEADER SET {leader} text=\"C:\\\\new\\nB\""));
        assert_eq!(s.leader(leader).text, "C:\\new\nB");
        // A template too.
        let pit = add(&mut s, point_at(pt(9.0, 9.0)), PropertyMap::new());
        s.run(&(format!("LEADER #{pit} 15,15 ") + "template=\"\\\\{id}\""));
        assert_eq!(note(&s, s.last()), format!("\\{pit}"));
    }

    #[test]
    fn inside_a_lot_the_arrow_is_a_dot_and_the_note_its_area() {
        let mut s = Session::default();
        let lot = add(
            &mut s,
            closed(vec![
                pt(0.0, 0.0),
                pt(20.0, 0.0),
                pt(20.0, 10.0),
                pt(0.0, 10.0),
            ]),
            with("lot", PropertyValue::Integer(12)),
        );
        s.run(
            &(format!("LEADER #{lot}.inside 30,20 ")
                + "template=\"LOT {prop.lot}\\n{area:.0f} m2\""),
        );
        let leader = s.leader(s.last());
        assert_eq!(leader.vertices[0], pt(10.0, 5.0), "the lot's inside point");
        assert_eq!(
            leader.arrow,
            ArrowHead::Dot,
            "a leader ending inside an outline ends in a dot"
        );
        assert_eq!(note(&s, s.last()), "LOT 12\n200 m2");
        s.run(&format!("LEADER #{lot}.inside 30,20 text=X arrow=open"));
        assert_eq!(
            s.leader(s.last()).arrow,
            ArrowHead::Open,
            "unless the arrow is given"
        );
    }

    #[test]
    fn a_label_styles_template_is_read_live_and_the_style_is_kept() {
        let mut s = Session::default();
        let pit = add(
            &mut s,
            point_at(pt(0.0, 0.0)),
            with("invert", PropertyValue::Real(10.25)),
        );
        s.run("TEXTSTYLE NEW Notes paper=3.5");
        s.run(concat!(
            "LABELSTYLE NEW Invert kind=point text=\"INV {prop.invert:.3f}\" textstyle=Notes ",
            "paper=3"
        ));
        let made = s.run(&format!("LEADER #{pit} 5,5 labelstyle=Invert"));
        assert!(made.contains("labelstyle=Invert"), "{made}");
        let leader = s.last();
        assert_eq!(s.leader(leader).label_style, "Invert");
        assert!(s.leader(leader).text.is_empty());
        assert_eq!(
            s.leader(leader).style,
            "Notes",
            "the style's face is lent when it is made"
        );
        assert_eq!(s.leader(leader).paper_height, 3.0);
        assert_eq!(note(&s, leader), "INV 10.250");

        s.run("LABELSTYLE SET Invert text=\"IL {prop.invert:.2f}\"");
        assert_eq!(
            note(&s, leader),
            "IL 10.25",
            "one style edited, every leader in it follows"
        );
        assert!(s.refusal("LABELSTYLE DELETE Invert").contains("leaders"));
    }

    // ---- asking and changing -----------------------------------------------------------------

    #[test]
    fn values_lists_what_a_note_there_could_say() {
        let mut s = Session::default();
        let line = add(
            &mut s,
            line_of(pt(0.0, 0.0), pt(40.0, 0.0)),
            with("size", PropertyValue::Text(String::from("150"))),
        );
        let place = s.run(&format!("LEADER VALUES #{line}@10,3"));
        assert!(
            place.contains(&format!("target={line} type=Line")),
            "{place}"
        );
        assert!(place.contains("\nvalue=chainage text=10.000"), "{place}");
        assert!(place.contains("\nvalue=length text=40.000"), "{place}");
        assert!(place.contains("\nvalue=bearing text="), "{place}");
        assert!(place.contains("\nvalue=prop.size text=150"), "{place}");
        assert!(!place.contains("value=z "), "no level, so no z");

        s.run(&(format!("LEADER #{line}@30,0 35,5 ") + "template=\"{prop.size} PVC\""));
        let leader = s.last();
        let of_leader = s.run(&format!("LEADER VALUES {leader}"));
        assert!(
            of_leader.contains(&format!("leader={leader} target={line}")),
            "{of_leader}"
        );
        assert!(
            of_leader.contains("value=chainage text=30.000"),
            "{of_leader}"
        );
        s.run("LEADER 0,0 1,1 text=X");
        let plain = s.last();
        assert!(
            s.refusal(&format!("LEADER VALUES {plain}"))
                .contains("on no entity")
        );
    }

    #[test]
    fn leaders_can_be_listed_retargeted_attached_frozen_and_let_go_keeping_their_words() {
        let mut s = Session::default();
        let pit = add(
            &mut s,
            point_at(pt(0.0, 0.0)),
            with("invert", PropertyValue::Real(10.25)),
        );
        let other = add(
            &mut s,
            point_at(pt(50.0, 0.0)),
            with("invert", PropertyValue::Real(8.0)),
        );
        s.run(&(format!("LEADER #{pit} 5,5 ") + "template=\"IL {prop.invert:.2f}\""));
        let smart = s.last();
        s.run("LEADER 100,100 105,105 text=PLAIN");
        let plain = s.last();

        let list = s.run("LEADER LIST");
        assert!(list.contains(&format!("id={smart}")), "{list}");
        assert!(list.contains("smart=yes"), "{list}");
        assert!(list.contains("text=\"IL 10.25\""), "{list}");
        assert!(list.contains(&format!("id={plain}")), "{list}");
        let on_pit = s.run(&format!("LEADER LIST target={pit}"));
        assert!(!on_pit.contains(&format!("id={plain}")), "{on_pit}");

        // SET: a new template; the tip moved to another pit reads that one.
        s.run(&(format!("LEADER SET {smart} ") + "template=\"INV {prop.invert:.1f}\""));
        assert_eq!(
            note(&s, smart),
            "INV 10.3",
            "10.25 is exact in binary: half away from zero"
        );
        s.run(&format!("LEADER SET {smart} tip=#{other}"));
        assert_eq!(tip(&s, smart), pt(50.0, 0.0));
        assert_eq!(note(&s, smart), "INV 8.0");
        s.run(&format!("LEADER SET {smart} at=60,20"));
        assert_eq!(hang(&s, smart), pt(60.0, 20.0));
        assert!(
            s.refusal(&format!("LEADER SET {smart} {plain} at=1,1"))
                .contains("name one leader")
        );
        assert!(
            s.refusal(&format!("LEADER SET {smart} tip=70,70"))
                .contains("reads the entity its tip is on")
        );
        assert!(
            s.refusal(&format!("LEADER SET {smart} tip=#{smart}"))
                .contains("itself")
        );

        // ATTACH a plain leader: it follows the pit, its note is still its own.
        s.run(&format!("LEADER ATTACH {plain} #{pit}"));
        assert_eq!(tip(&s, plain), pt(0.0, 0.0));
        assert_eq!(s.leader(plain).tip_ref.entity, pit);
        assert_eq!(note(&s, plain), "PLAIN");

        // FREEZE keeps the words and the tip's place; DETACH lets the tip go.
        assert_eq!(
            s.run(&format!("LEADER FREEZE {smart} {plain}")),
            "frozen leaders=1"
        );
        assert!(!is_smart(&s.leader(smart)));
        assert_eq!(s.leader(smart).text, "INV 8.0");
        assert_eq!(s.leader(smart).tip_ref.entity, other);
        assert_eq!(
            s.run(&format!("LEADER DETACH {smart} {plain}")),
            "detached leaders=2 frozen=0"
        );
        assert!(!s.leader(plain).tip_ref.associated());
        assert_eq!(
            s.run(&format!("LEADER DETACH {plain}")),
            "detached leaders=0 frozen=0",
            "nothing to let go is no step"
        );
    }

    #[test]
    fn detaching_a_smart_leader_freezes_its_note_first() {
        let mut s = Session::default();
        let pit = add(
            &mut s,
            point_at(pt(0.0, 0.0)),
            with("invert", PropertyValue::Real(10.25)),
        );
        s.run(&(format!("LEADER #{pit} 5,5 ") + "template=\"IL {prop.invert:.2f}\""));
        let leader = s.last();
        assert_eq!(
            s.run(&format!("LEADER DETACH {leader}")),
            "detached leaders=1 frozen=1"
        );
        assert_eq!(s.leader(leader).text, "IL 10.25");
        assert!(!s.leader(leader).fields);
        assert!(!s.leader(leader).tip_ref.associated());
        s.run("UNDO");
        assert!(is_smart(&s.leader(leader)), "one step back");
    }

    #[test]
    fn for_makes_one_leader_per_entity_in_one_step() {
        let mut s = Session::default();
        s.run("ANNOSCALE 500");
        let pit = add(&mut s, point_at(pt(0.0, 0.0)), PropertyMap::new());
        let line = add(
            &mut s,
            line_of(pt(100.0, 0.0), pt(140.0, 0.0)),
            PropertyMap::new(),
        );
        let lot = add(
            &mut s,
            closed(vec![
                pt(200.0, 0.0),
                pt(220.0, 0.0),
                pt(220.0, 10.0),
                pt(200.0, 10.0),
            ]),
            PropertyMap::new(),
        );
        let circle = add(
            &mut s,
            Geometry::Circle(Circle2::new(pt(300.0, 0.0), 5.0)),
            PropertyMap::new(),
        );
        let dimension = add(
            &mut s,
            Geometry::Dimension(DimensionGeometry::new(
                pt(0.0, 50.0),
                pt(10.0, 50.0),
                2.0,
                "",
            )),
            PropertyMap::new(),
        );
        let before = s.steps();
        let made = s.run(
            &(format!("LEADER FOR {pit} {line} {lot} {circle} {dimension} ")
                + "template=\"{type}\""),
        );
        assert!(made.contains("created leaders=4"), "{made}");
        assert!(
            made.contains("skipped=1"),
            "a dimension offers no place: {made}"
        );
        assert_eq!(s.steps(), before + 1);

        let mut by_target: BTreeMap<EntityId, LeaderGeometry> = BTreeMap::new();
        s.document.model().entities.for_each(|entity| {
            if let Geometry::Leader(leader) = &entity.geometry {
                by_target.insert(leader.tip_ref.entity, leader.clone());
            }
        });
        assert_eq!(by_target.len(), 4);
        // 10 mm at 1:500 is 5 m, at 45 degrees: (5 cos 45, 5 sin 45) past the tip.
        let reach = 5.0 * (0.25 * PI).cos();
        let first = |target: EntityId| by_target[&target].vertices[0];
        let last = |target: EntityId| match by_target[&target].vertices.last() {
            Some(&at) => at,
            None => panic!("the leader on {target} has no vertex"),
        };
        assert_eq!(first(pit), pt(0.0, 0.0));
        near(last(pit).x, reach, 1e-12);
        near(last(pit).y, reach, 1e-12);
        assert_eq!(first(line), pt(120.0, 0.0), "the middle of the line");
        assert_eq!(first(lot), pt(210.0, 5.0), "inside the lot");
        assert_eq!(by_target[&lot].arrow, ArrowHead::Dot);
        // On the circle, on the side the note goes: 45 degrees round.
        near(first(circle).x, 300.0 + reach, 1e-12);
        near(first(circle).y, reach, 1e-12);
        s.run("UNDO");
        let mut leaders = 0;
        s.document.model().entities.for_each(|entity| {
            if matches!(entity.geometry, Geometry::Leader(_)) {
                leaders += 1;
            }
        });
        assert_eq!(leaders, 0);
        assert!(
            s.refusal(&format!("LEADER FOR {dimension}"))
                .contains("no leader made")
        );
    }

    #[test]
    fn balloons_for_the_selection_are_numbered_on() {
        let mut s = Session::default();
        s.run("BALLOON 0,0 5,5");
        let a = add(&mut s, point_at(pt(10.0, 0.0)), PropertyMap::new());
        let b = add(&mut s, point_at(pt(20.0, 0.0)), PropertyMap::new());
        s.run(&format!("SELECT {a} {b}"));
        let made = s.run("BALLOON FOR SELECTION");
        assert!(made.contains("created balloons=2"), "{made}");
        let ids = s.document.last_created_entities();
        assert_eq!(ids.len(), 2);
        assert_eq!(s.leader(ids[0]).text, "2");
        assert_eq!(s.leader(ids[1]).text, "3");
        assert_eq!(s.leader(ids[0]).callout, CalloutShape::Circle);
        // A smart balloon says its entity's item and is no number in the run.
        s.run(&(format!("BALLOON #{a} 30,30 ") + "template=\"{id}\""));
        assert!(s.run("BALLOON 40,40 45,45").contains("text=4"));
    }

    // ---- following the geometry --------------------------------------------------------------

    #[test]
    fn a_smart_leader_goes_with_its_target_and_comes_back_on_undo() {
        let mut s = Session::default();
        let pit = add(&mut s, point_at(pt(0.0, 0.0)), PropertyMap::new());
        s.run(&(format!("LEADER #{pit} 5,5 ") + "template=\"PIT {id}\""));
        let smart = s.last();
        // A smart leader on the smart leader's end: a chain.
        s.run(&(format!("LEADER #{smart}.end 9,9 ") + "template=\"{type}\""));
        let chained = s.last();
        s.run(&format!("LEADER #{pit} -5,5 text=PLAIN"));
        let plain = s.last();

        s.run(&format!("SELECT {pit}"));
        s.run("ERASE");
        assert!(
            !s.document.model().entities.contains(smart),
            "it read the pit"
        );
        assert!(
            !s.document.model().entities.contains(chained),
            "it read the leader that went"
        );
        assert!(s.document.model().entities.contains(plain));
        assert!(
            !s.leader(plain).tip_ref.associated(),
            "a plain one lets the reference go"
        );

        s.run("UNDO");
        assert!(s.document.model().entities.contains(smart));
        assert!(s.document.model().entities.contains(chained));
        assert_eq!(note(&s, smart), format!("PIT {pit}"));
        assert_eq!(s.leader(plain).tip_ref.entity, pit);
    }

    #[test]
    fn a_copy_of_a_pit_with_its_callout_gets_a_callout_of_its_own() {
        let mut s = Session::default();
        let pit = add(&mut s, point_at(pt(0.0, 0.0)), PropertyMap::new());
        s.run(&(format!("LEADER #{pit} 5,5 ") + "template=\"PIT {id}\""));
        let leader = s.last();
        assert!(
            s.document
                .execute(copy_entities(vec![pit, leader], Vec2::new(100.0, 0.0)))
                .is_ok()
        );
        let copies = s.document.last_created_entities();
        assert_eq!(copies.len(), 2);
        let pit_copy = copies[0];
        let copy = s.leader(copies[1]);
        assert_eq!(copy.tip_ref.entity, pit_copy);
        assert_eq!(copy.vertices[0], pt(100.0, 0.0));
        assert_eq!(note(&s, copies[1]), format!("PIT {pit_copy}"));
        assert_eq!(
            note(&s, leader),
            format!("PIT {pit}"),
            "the original is untouched"
        );

        // A callout copied without its pit is another callout of the same pit.
        assert!(
            s.document
                .execute(copy_entities(vec![leader], Vec2::new(0.0, 50.0)))
                .is_ok()
        );
        let alone = s.leader(s.last());
        assert_eq!(alone.tip_ref.entity, pit);
        assert_eq!(alone.vertices[0], pt(0.0, 0.0), "its tip stays on the pit");
        // The copy is undone and redone with its reference intact.
        assert!(s.document.undo().is_ok());
        assert!(s.document.undo().is_ok());
        assert!(s.document.redo().is_ok());
        assert_eq!(s.leader(copies[1]).tip_ref.entity, pit_copy);
    }

    #[test]
    fn the_drawn_note_is_the_one_read_off() {
        let mut s = Session::default();
        let pit = add(
            &mut s,
            point_at(pt(0.0, 0.0)),
            with("invert", PropertyValue::Real(10.5)),
        );
        s.run(&(format!("LEADER #{pit} 5,5 ") + "template=\"IL {prop.invert:.2f}\\nRL {rl}\""));
        let leader = s.last();
        let drawing = build_leader(
            s.document.model(),
            &s.leader(leader),
            1000.0,
            &estimated_measure(),
            &LeaderContext::default(),
        );
        assert_eq!(drawing.texts.len(), 1, "the RL line has nothing to say");
        assert_eq!(drawing.texts[0].text, "IL 10.50");

        // INFO and LIST say it too.
        let info = s.run(&format!("INFO {leader}"));
        assert!(info.contains("\"IL 10.50\""), "{info}");
        assert!(info.contains("template="), "{info}");
        assert!(info.contains(&format!("on {pit}")), "{info}");
    }

    #[test]
    fn a_dimension_can_be_on_a_place_along_a_line_too() {
        let mut s = Session::default();
        let line = add(
            &mut s,
            line_of(pt(0.0, 0.0), pt(40.0, 0.0)),
            PropertyMap::new(),
        );
        s.run(&format!("DIM ALIGNED #{line}.start #{line}@10,1 at=5,5"));
        let dimension = s.last();
        let measured = |s: &Session| {
            let entity = s.entity(dimension);
            match entity.geometry {
                Geometry::Dimension(measure) => measure.measurement(),
                other => panic!("not a dimension: {other:?}"),
            }
        };
        double_eq(measured(&s), 10.0);
        assert!(
            s.document
                .execute(set_entity_geometry(
                    line,
                    line_of(pt(0.0, 0.0), pt(80.0, 0.0))
                ))
                .is_ok()
        );
        // A quarter of the stretched line.
        double_eq(measured(&s), 20.0);
    }

    // ---- arranging -------------------------------------------------------------------------------

    #[test]
    fn align_puts_the_notes_in_a_column_top_down() {
        let mut s = Session::default();
        s.run("ANNOSCALE 500");
        s.run("LEADER 0,0 10,30 text=A");
        let a = s.last();
        s.run("LEADER 5,0 22,40 text=B");
        let b = s.last();
        s.run("LEADER 9,0 13,20 20,10 text=C");
        let c = s.last();
        // The top note (B, at y 40) sets the column's x; 8 mm at 1:500 is 4 m
        // between them, top down: B 40, A 36, C 32.
        let before = s.steps();
        let reply = s.run(&format!("LEADER ALIGN {a} {b} {c} spacing=8"));
        assert_eq!(reply, "aligned leaders=3 x=22 spacing=4");
        assert_eq!(s.steps(), before + 1);
        assert_eq!(hang(&s, b), pt(22.0, 40.0));
        assert_eq!(hang(&s, a), pt(22.0, 36.0));
        assert_eq!(hang(&s, c), pt(22.0, 32.0));
        assert_eq!(
            s.leader(c).vertices[1],
            pt(13.0, 20.0),
            "only where the note hangs moves"
        );
        // x= alone keeps each one's height.
        s.run(&format!("LEADER ALIGN {a} {c} x=-5"));
        assert_eq!(hang(&s, a), pt(-5.0, 36.0));
        assert_eq!(hang(&s, c), pt(-5.0, 32.0));
        assert!(!s.refusal(&format!("LEADER ALIGN {a} spacing=0")).is_empty());
    }

    #[test]
    fn renumbering_numbers_the_balloons_in_the_order_asked() {
        let mut s = Session::default();
        s.run("BALLOON 30,0 35,5 n=7");
        let right = s.last();
        s.run("BALLOON 10,20 15,25 n=3");
        let top = s.last();
        s.run("BALLOON 20,-10 25,-5 n=9");
        let bottom = s.last();
        let pit = add(&mut s, point_at(pt(0.0, 0.0)), PropertyMap::new());
        s.run(&(format!("BALLOON #{pit} 5,5 ") + "template=\"{id}\""));
        let smart = s.last();

        assert_eq!(s.run("BALLOON RENUMBER"), "balloons=3 renumbered=3");
        assert_eq!(s.leader(right).text, "1");
        assert_eq!(s.leader(top).text, "2");
        assert_eq!(s.leader(bottom).text, "3");
        assert!(
            s.leader(smart).fields,
            "a smart balloon is no number in the run"
        );
        // Left to right by the arrows' tips: 10, 20, 30.
        s.run("BALLOON RENUMBER order=x start=101");
        assert_eq!(s.leader(top).text, "101");
        assert_eq!(s.leader(bottom).text, "102");
        assert_eq!(s.leader(right).text, "103");
        // Top down: 20, 0, -10.
        s.run("BALLOON RENUMBER order=y");
        assert_eq!(s.leader(top).text, "1");
        assert_eq!(s.leader(right).text, "2");
        assert_eq!(s.leader(bottom).text, "3");
        assert_eq!(
            s.run("BALLOON RENUMBER order=y"),
            "balloons=3 renumbered=0",
            "numbered as asked already: no step"
        );
        assert!(!s.refusal("BALLOON RENUMBER order=z").is_empty());
    }
}
