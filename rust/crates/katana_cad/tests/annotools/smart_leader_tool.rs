// The Leader tool's smart leaders (tests/cad/tools/test_smart_leader_tool.cpp;
// docs/annotation.md, "Smart leaders"; docs/tools.md): a tip clicked on an
// entity is put on it and follows it, and a note typed with fields in braces
// is read off it - driven through ToolDriver as the plan view drives the
// tool.

use katana_cad::interactive_tool::ToolStepOutcome;
use katana_commands::{create_entities, move_entities, set_entity_geometry};
use katana_entity::leader_values::leader_note;
use katana_entity::{
    AnchorPoint, Entity, EntityId, Geometry, LeaderContext, LeaderGeometry, PropertyValue,
};
use katana_geometry::{Point2, Segment2, Vec2};

use crate::tool_driver::ToolDriver;
use crate::{assert_contains, double_eq, leaders, ok};

// The one leader in the drawing.
#[track_caller]
fn the_leader(driver: &ToolDriver) -> LeaderGeometry {
    let mut found = leaders(driver);
    assert_eq!(found.len(), 1);
    found.pop().unwrap_or_default()
}

fn add_line(driver: &mut ToolDriver) -> EntityId {
    let mut line = Entity {
        geometry: Geometry::Line(Segment2::new(Point2::new(0.0, 0.0), Point2::new(40.0, 0.0))),
        ..Entity::default()
    };
    line.properties
        .insert(String::from("size"), PropertyValue::from("150"));
    driver.add(create_entities(vec![line]))
}

// The note `leader` says in `driver`'s drawing (entity's leader_note with its
// C++ defaults: no code properties, no surfaces or alignments).
fn note_of(driver: &ToolDriver, leader: &LeaderGeometry) -> String {
    leader_note(
        driver.document().model(),
        leader,
        &[],
        &LeaderContext::default(),
    )
}

mod smart_leader_tool {
    use super::*;

    #[test]
    fn a_tip_clicked_on_a_line_is_put_on_it_and_a_note_with_fields_is_read_off_it() {
        let mut driver = ToolDriver::new();
        let line = add_line(&mut driver);
        driver.start("annotate.leader");
        // Within the driver's 0.01 pick aperture of the line, a quarter along.
        let tip = driver.click(10.0, 0.005);
        assert_eq!(tip.outcome, ToolStepOutcome::Continue);
        assert_contains(&tip.message, &format!("leader on Line {line}"));
        driver.click(20.0, 10.0);
        driver.enter();
        driver.r#type("{prop.size} PVC");
        driver.r#type("CH {chainage:.1f}");
        let done = driver.enter();
        assert_eq!(done.outcome, ToolStepOutcome::Done, "{}", done.message);
        assert_contains(&done.message, &format!("read off entity {line}"));

        let leader = the_leader(&driver);
        assert_eq!(
            leader.vertices[0],
            Point2::new(10.0, 0.0),
            "put on the line"
        );
        assert_eq!(leader.tip_ref.entity, line);
        assert_eq!(leader.tip_ref.point, AnchorPoint::Along);
        double_eq(leader.tip_ref.parameter, 0.25);
        assert!(leader.fields);
        assert_eq!(note_of(&driver, &leader), "150 PVC\nCH 10.0");

        // It follows the line.
        ok(driver.document_mut().execute(set_entity_geometry(
            line,
            Geometry::Line(Segment2::new(Point2::new(0.0, 0.0), Point2::new(80.0, 0.0))),
        )));
        assert_eq!(the_leader(&driver).vertices[0], Point2::new(20.0, 0.0));
    }

    #[test]
    fn a_field_typed_wrong_is_refused_as_it_is_typed() {
        let mut driver = ToolDriver::new();
        add_line(&mut driver);
        driver.start("annotate.leader");
        driver.click(10.0, 0.0);
        driver.click(20.0, 10.0);
        driver.enter();
        let wrong = driver.r#type("{sise}");
        assert_eq!(wrong.outcome, ToolStepOutcome::Rejected);
        assert_contains(&wrong.message, "no value 'sise'");
        let unbalanced = driver.r#type("{size");
        assert_eq!(unbalanced.outcome, ToolStepOutcome::Rejected);
        // A literal brace is doubled.
        assert_eq!(
            driver.r#type("{{150}} {prop.size}").outcome,
            ToolStepOutcome::Continue
        );
        assert_eq!(driver.enter().outcome, ToolStepOutcome::Done);
        assert_eq!(note_of(&driver, &the_leader(&driver)), "{150} 150");
    }

    #[test]
    fn a_note_that_would_say_nothing_is_refused_until_a_line_is_taken_back() {
        let mut driver = ToolDriver::new();
        add_line(&mut driver);
        driver.start("annotate.leader");
        driver.click(10.0, 0.0);
        driver.click(20.0, 10.0);
        driver.enter();
        driver.r#type("IL {prop.invert}");
        let refused = driver.enter();
        assert_eq!(refused.outcome, ToolStepOutcome::Rejected);
        assert_contains(&refused.message, "no prop.invert");
        assert_eq!(driver.executed(), 0);
        driver.undo();
        driver.r#type("{length:.1f} m");
        assert_eq!(driver.enter().outcome, ToolStepOutcome::Done);
        assert_eq!(note_of(&driver, &the_leader(&driver)), "40.0 m");
    }

    #[test]
    fn a_tip_on_nothing_keeps_its_note_as_typed() {
        let mut driver = ToolDriver::new();
        add_line(&mut driver);
        driver.start("annotate.leader");
        driver.click(10.0, 5.0); // 5 from the line: outside the aperture
        driver.click(20.0, 10.0);
        driver.enter();
        driver.r#type("{not a field}");
        let done = driver.enter();
        assert_eq!(done.outcome, ToolStepOutcome::Done);
        assert_eq!(done.message, "leader with 1 line of text");
        let leader = the_leader(&driver);
        assert!(!leader.tip_ref.associated());
        assert!(!leader.fields);
        assert_eq!(leader.text, "{not a field}");
    }

    #[test]
    fn a_tip_on_an_entity_with_a_plain_note_follows_it() {
        let mut driver = ToolDriver::new();
        let line = add_line(&mut driver);
        driver.start("annotate.leader");
        driver.click(40.0, 0.0);
        driver.click(50.0, 10.0);
        driver.enter();
        driver.r#type("KERB");
        let done = driver.enter();
        assert_contains(&done.message, &format!("on entity {line}"));
        let leader = the_leader(&driver);
        assert_eq!(leader.tip_ref.entity, line);
        assert!(!leader.fields);
        ok(driver
            .document_mut()
            .execute(move_entities(vec![line], Vec2::new(0.0, 3.0))));
        assert_eq!(the_leader(&driver).vertices[0], Point2::new(40.0, 3.0));

        // Undo in the tip step lets the entity go as well as the point.
        driver.start("annotate.leader");
        driver.click(40.0, 3.0);
        driver.undo();
        driver.click(100.0, 100.0);
        driver.click(110.0, 110.0);
        driver.enter();
        driver.r#type("FREE");
        assert_eq!(driver.enter().outcome, ToolStepOutcome::Done);
        let free = leaders(&driver)
            .iter()
            .filter(|leader| !leader.tip_ref.associated())
            .count();
        assert_eq!(free, 1);
    }
}
