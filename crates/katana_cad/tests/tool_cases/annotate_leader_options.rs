// The dimension and leader tools' options and their associative points: the
// Leader's Arrow, Callout, Style, Paper, SIze and Landing, the Balloon tool
// with its Number, Style and Paper, the Linear Dimension's Rotated option and
// its dimension line between the origins' levels, the Ordinate's Datum, and
// points snapped to an entity's end, middle, centre or vertex, which the
// annotation made from them then follows. Each tool is held to the verb line
// that makes the same thing (LEADER, BALLOON, DIM LINEAR, DIM ORDINATE),
// typed into a second drawing made the same way; every other expected value
// is worked out by hand beside it
// (tests/cad/tools/test_annotate_leader_options.cpp).

use katana_cad::{Document, SnapMode, SnapResult};
use katana_entity::{DimensionGeometry, Entity, EntityId, EntityType, Geometry, LeaderGeometry};
use katana_geometry::Point2;

fn entities_of(document: &Document, kind: EntityType) -> Vec<Entity> {
    let mut out = Vec::new();
    document.model().entities.for_each(|entity| {
        if entity.r#type() == kind {
            out.push(entity.clone());
        }
    });
    out
}

// The newest leader, or a default one (and a failure) when there is none.
#[track_caller]
fn newest_leader(document: &Document) -> LeaderGeometry {
    let all = entities_of(document, EntityType::Leader);
    assert!(!all.is_empty());
    match all.last().map(|entity| &entity.geometry) {
        Some(Geometry::Leader(leader)) => leader.clone(),
        _ => LeaderGeometry::default(),
    }
}

// The newest dimension, or a default one (and a failure) when there is none.
#[track_caller]
fn newest_dimension(document: &Document) -> DimensionGeometry {
    let all = entities_of(document, EntityType::Dimension);
    assert!(!all.is_empty());
    match all.last().map(|entity| &entity.geometry) {
        Some(Geometry::Dimension(dimension)) => dimension.clone(),
        _ => DimensionGeometry::default(),
    }
}

// The entities of each drawing of `kind` are the same entities: their
// geometry, layer, style and colour.
#[track_caller]
fn expect_same(drawn: &Document, typed: &Document, kind: EntityType) {
    let made = entities_of(drawn, kind);
    let expected = entities_of(typed, kind);
    assert_eq!(made.len(), expected.len());
    for (i, (a, b)) in made.iter().zip(&expected).enumerate() {
        assert_eq!(a.geometry, b.geometry, "{i}");
        assert_eq!(a.layer, b.layer, "{i}");
        assert_eq!(a.style, b.style, "{i}");
        assert_eq!(a.color, b.color, "{i}");
    }
}

fn snapped(point: Point2, mode: SnapMode, entity: EntityId) -> SnapResult {
    SnapResult {
        point,
        mode,
        entity,
    }
}

// ---- which point of an entity a snap names ----------------------------------------------

mod snap_anchor {
    use katana_cad::snapping::snap_anchor;
    use katana_cad::{Document, SnapMode};
    use katana_entity::{AnchorPoint, EntityId};
    use katana_geometry::Point2;

    use super::snapped;
    use crate::{anchor, pt, run};

    #[test]
    fn an_end_a_middle_a_centre_or_a_vertex_is_named_and_nothing_else_is() {
        let mut document = Document::default();
        run(&mut document, "LINE 0,0 10,0"); // 1
        run(&mut document, "CIRCLE 20,0 5"); // 2
        run(&mut document, "PLINE 0,10 10,10 10,20"); // 3
        run(&mut document, "POINT 30,30"); // 4
        let model = document.model();
        let name =
            |at: Point2, mode: SnapMode, id: EntityId| snap_anchor(model, &snapped(at, mode, id));
        assert_eq!(
            name(pt(0.0, 0.0), SnapMode::Endpoint, 1),
            Some(anchor(1, AnchorPoint::Start, 0))
        );
        assert_eq!(
            name(pt(10.0, 0.0), SnapMode::Endpoint, 1),
            Some(anchor(1, AnchorPoint::End, 0))
        );
        assert_eq!(
            name(pt(5.0, 0.0), SnapMode::Midpoint, 1),
            Some(anchor(1, AnchorPoint::Mid, 0))
        );
        assert_eq!(
            name(pt(20.0, 0.0), SnapMode::Center, 2),
            Some(anchor(2, AnchorPoint::Centre, 0))
        );
        // A polyline's corner by its vertex, the middle of its second segment
        // (10,10 -> 10,20) by that segment.
        assert_eq!(
            name(pt(10.0, 10.0), SnapMode::Endpoint, 3),
            Some(anchor(3, AnchorPoint::Vertex, 1))
        );
        assert_eq!(
            name(pt(10.0, 15.0), SnapMode::Midpoint, 3),
            Some(anchor(3, AnchorPoint::SegmentMid, 1))
        );
        assert_eq!(
            name(pt(30.0, 30.0), SnapMode::Endpoint, 4),
            Some(anchor(4, AnchorPoint::Position, 0))
        );
        // Somewhere along a line, where two lines cross, the grid: no point of
        // an entity that it could follow.
        assert!(name(pt(3.0, 0.0), SnapMode::Nearest, 1).is_none());
        assert!(name(pt(0.0, 0.0), SnapMode::Intersection, 1).is_none());
        assert!(name(pt(0.0, 0.0), SnapMode::Grid, 0).is_none());
        // A snap that names a point the entity does not have there, or an
        // entity that is gone.
        assert!(name(pt(4.0, 0.0), SnapMode::Endpoint, 1).is_none());
        assert!(name(pt(0.0, 0.0), SnapMode::Endpoint, 99).is_none());
    }
}

// ---- Leader options ----------------------------------------------------------------------

mod leader_options {
    use katana_cad::Document;
    use katana_cad::interactive_tool::ToolInput;
    use katana_entity::{ArrowHead, CalloutShape, EntityType};

    use super::{expect_same, newest_leader};
    use crate::tool_driver::ToolDriver;
    use crate::{CONTINUE, DONE, REJECTED, run};

    #[test]
    fn the_arrow_callout_style_and_paper_are_the_leader_verbs_own() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.leader");
        assert_eq!(
            driver.prompt(),
            "Specify leader start point or [Arrow/Callout/Style/Paper/SIze/Landing]"
        );
        driver.r#type("A");
        assert_eq!(
            driver.prompt(),
            "Enter arrowhead [Closed/Open/Tick/Dot/None] <Closed>"
        );
        assert_eq!(driver.r#type("zigzag").outcome, REJECTED);
        assert_eq!(driver.r#type("o").outcome, CONTINUE);
        driver.r#type("C");
        assert_eq!(driver.prompt(), "Enter callout [None/Box/Circle] <None>");
        driver.r#type("Box");
        driver.r#type("S");
        assert_eq!(
            driver.r#type("Nowhere").outcome,
            REJECTED,
            "no such text style"
        );
        driver.r#type("Standard");
        driver.r#type("P");
        assert_eq!(
            driver.prompt(),
            "Enter the note's height on paper, mm <2.5>"
        );
        assert_eq!(driver.r#type("0").outcome, REJECTED);
        driver.r#type("3.5");
        assert_eq!(
            driver.prompt(),
            "Specify leader start point or [Arrow/Callout/Style/Paper/SIze/Landing] (Open \
             arrow, Box callout, style Standard, 3.5 mm)"
        );
        driver.click(0.0, 0.0);
        driver.click(8.0, 6.0);
        driver.enter();
        driver.r#type("AB");
        assert_eq!(driver.enter().outcome, DONE);

        // The Standard dimension style's arrow, 2.5 model units at 1 : 1000, is
        // 2.5 mm; the last segment rises 36.9 degrees, more than 15, so the
        // landing is one arrowhead long.
        let mut typed = Document::default();
        run(
            &mut typed,
            "LEADER 0,0 8,6 text=AB arrow=open callout=box style=Standard paper=3.5 \
             arrowsize=2.5 landing=2.5",
        );
        expect_same(driver.document(), &typed, EntityType::Leader);

        // Only that leader: the next takes the dimension style's again.
        driver.click(0.0, 0.0);
        driver.click(8.0, 6.0);
        driver.enter();
        assert_eq!(driver.enter().outcome, DONE);
        let next = newest_leader(driver.document());
        assert_eq!(next.arrow, ArrowHead::ClosedFilled);
        assert_eq!(next.callout, CalloutShape::None);
        assert_eq!(next.paper_height, 2.5);
        assert!(next.style.is_empty());
    }

    #[test]
    fn enter_at_an_option_keeps_what_it_has_and_undo_leaves_it() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.leader");
        driver.r#type("A");
        assert_eq!(driver.enter().outcome, CONTINUE);
        assert_eq!(driver.expects(), ToolInput::Point);
        driver.r#type("A");
        assert_eq!(driver.undo().outcome, CONTINUE);
        assert_eq!(
            driver.prompt(),
            "Specify leader start point or [Arrow/Callout/Style/Paper/SIze/Landing]"
        );
        assert_eq!(
            driver.r#type("-2").outcome,
            REJECTED,
            "not an option, not a point"
        );
        assert_eq!(driver.executed(), 0);
    }

    // SIze and Landing are LEADER's arrowsize= and landing=, which the tool
    // once had no way to give: only the verb could, and nothing after the
    // leader was drawn. A landing given is drawn as given, on a level leader
    // too; Auto gives the rule back - one arrowhead where the last segment
    // slopes.
    #[test]
    fn size_and_landing_are_the_leader_verbs_arrowsize_and_landing() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.leader");
        driver.r#type("S");
        assert_eq!(
            driver.prompt(),
            "Enter text style name <Standard>",
            "S is Style's"
        );
        driver.enter();
        driver.r#type("si");
        assert_eq!(
            driver.prompt(),
            "Enter the arrowhead's size on paper, mm <2.5>"
        );
        assert_eq!(driver.r#type("0").outcome, REJECTED);
        assert_eq!(driver.r#type("big").outcome, REJECTED);
        driver.r#type("4");
        driver.r#type("L");
        assert_eq!(
            driver.prompt(),
            "Enter the landing's length on paper, mm, 0 for none, or Auto <Auto>"
        );
        assert_eq!(driver.r#type("-1").outcome, REJECTED);
        driver.r#type("5");
        assert_eq!(
            driver.prompt(),
            "Specify leader start point or [Arrow/Callout/Style/Paper/SIze/Landing] \
             (arrowhead 4 mm, landing 5 mm)"
        );
        driver.click(0.0, 0.0);
        driver.click(10.0, 0.0);
        driver.enter();
        driver.r#type("AB");
        assert_eq!(driver.enter().outcome, DONE);

        // The Standard dimension style's closed arrow; its 2.5 model-unit text
        // height is 2.5 mm at 1 : 1000.
        let mut typed = Document::default();
        run(
            &mut typed,
            "LEADER 0,0 10,0 text=AB arrow=closed paper=2.5 arrowsize=4 landing=5",
        );
        expect_same(driver.document(), &typed, EntityType::Leader);

        // Auto: the rule again. The last segment rises at atan(6/8) = 36.9
        // degrees, more than 15, so the landing is one arrowhead: the 4 given.
        driver.r#type("SI");
        driver.r#type("4");
        driver.r#type("L");
        driver.r#type("5");
        driver.r#type("L");
        assert_eq!(
            driver.prompt(),
            "Enter the landing's length on paper, mm, 0 for none, or Auto <5>"
        );
        driver.r#type("auto");
        driver.click(0.0, 0.0);
        driver.click(8.0, 6.0);
        driver.enter();
        driver.r#type("CD");
        assert_eq!(driver.enter().outcome, DONE);
        let sloped = newest_leader(driver.document());
        assert_eq!(sloped.arrow_size, 4.0);
        assert_eq!(sloped.landing, 4.0);
    }
}

// ---- Balloon -------------------------------------------------------------------------------

mod balloon {
    use katana_cad::Document;
    use katana_entity::{CalloutShape, EntityType};

    use super::{expect_same, newest_leader};
    use crate::tool_driver::ToolDriver;
    use crate::{DONE, REJECTED, run};

    #[test]
    fn it_is_the_balloon_verbs_numbered_circle_counting_on_from_the_highest() {
        let mut driver = ToolDriver::new();
        run(driver.document_mut(), "BALLOON 0,0 5,5");
        driver.start("annotate.balloon");
        assert_eq!(
            driver.prompt(),
            "Specify the balloon's arrow point or [Number/Style/Paper]"
        );
        driver.click(10.0, 0.0);
        assert_eq!(
            driver.enter().outcome,
            REJECTED,
            "a balloon needs its circle's place"
        );
        driver.click(15.0, 5.0);
        let done = driver.enter();
        assert_eq!(done.outcome, DONE);
        assert_eq!(done.message, "balloon 2");

        let mut typed = Document::default();
        run(&mut typed, "BALLOON 0,0 5,5");
        run(&mut typed, "BALLOON 10,0 15,5");
        expect_same(driver.document(), &typed, EntityType::Leader);
        let balloon = newest_leader(driver.document());
        assert_eq!(balloon.callout, CalloutShape::Circle);
        assert_eq!(balloon.text, "2");
    }

    #[test]
    fn number_gives_another_and_the_next_counts_on_from_the_highest_whole_number() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.balloon");
        driver.r#type("N");
        assert_eq!(driver.prompt(), "Enter the balloon's number <1>");
        driver.r#type("7");
        driver.click(0.0, 0.0);
        driver.click(5.0, 5.0);
        assert_eq!(driver.enter().outcome, DONE);
        driver.click(10.0, 0.0);
        driver.r#type("N");
        driver.r#type("A1"); // not a whole number: counts for nothing
        driver.click(15.0, 5.0);
        assert_eq!(driver.enter().outcome, DONE);
        driver.click(20.0, 0.0);
        driver.click(25.0, 5.0);
        let third = driver.enter();
        assert_eq!(third.outcome, DONE);
        assert_eq!(
            third.message, "balloon 8",
            "one more than 7; A1 is not counted"
        );
        assert_eq!(
            driver.enter().outcome,
            DONE,
            "Enter at the first prompt ends it"
        );
        assert!(driver.finished());
    }

    // Style and Paper are BALLOON's style= and paper=, which the tool once
    // had no way to give: a balloon in a text style, at a height on paper,
    // made by the tool is the entity the verb makes. The style is found
    // ignoring case, as the Leader's is.
    #[test]
    fn style_and_paper_are_the_balloon_verbs_own() {
        let mut driver = ToolDriver::new();
        run(driver.document_mut(), "TEXTSTYLE NEW Notes");
        driver.start("annotate.balloon");
        driver.r#type("S");
        assert_eq!(driver.prompt(), "Enter text style name <Standard>");
        assert_eq!(
            driver.r#type("Nowhere").outcome,
            REJECTED,
            "no such text style"
        );
        driver.r#type("notes");
        driver.click(0.0, 0.0);
        driver.r#type("P");
        assert_eq!(
            driver.prompt(),
            "Enter the number's height on paper, mm <the style's>"
        );
        assert_eq!(driver.r#type("0").outcome, REJECTED);
        assert_eq!(
            driver.click(3.0, 3.0).outcome,
            REJECTED,
            "a height is asked for"
        );
        driver.r#type("3.5");
        driver.click(5.0, 5.0);
        assert_eq!(driver.enter().outcome, DONE);

        let mut typed = Document::default();
        run(&mut typed, "TEXTSTYLE NEW Notes");
        run(&mut typed, "BALLOON 0,0 5,5 style=Notes paper=3.5");
        expect_same(driver.document(), &typed, EntityType::Leader);
        let balloon = newest_leader(driver.document());
        assert_eq!(balloon.style, "Notes");
        assert_eq!(balloon.paper_height, 3.5);
    }
}

// ---- Linear: Rotated, and a line between the levels ---------------------------------------

mod linear_rotated {
    use katana_cad::Document;
    use katana_entity::{DimensionKind, EntityType};
    use katana_math::numerics::PI;

    use super::{expect_same, newest_dimension};
    use crate::tool_driver::ToolDriver;
    use crate::{CONTINUE, DONE, REJECTED, contains, near, run};

    #[test]
    fn a_typed_angle_makes_the_linear_kind_dim_linear_makes() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimlinear");
        driver.click(0.0, 0.0);
        driver.click(10.0, 5.0);
        assert!(
            contains(&driver.prompt(), "[Text/Horizontal/Vertical/Rotated/Undo]"),
            "{}",
            driver.prompt()
        );
        driver.r#type("R");
        assert_eq!(
            driver.prompt(),
            "Specify angle of dimension line or two points on it <0>"
        );
        assert_eq!(driver.r#type("30").outcome, CONTINUE);
        assert!(
            contains(&driver.prompt(), "rotated 30 degrees"),
            "{}",
            driver.prompt()
        );
        let done = driver.click(2.0, 8.0);
        assert_eq!(done.outcome, DONE, "{}", done.message);

        let mut typed = Document::default();
        run(&mut typed, "DIM LINEAR 0,0 10,5 at=2,8 angle=30");
        expect_same(driver.document(), &typed, EntityType::Dimension);
        let dimension = newest_dimension(driver.document());
        assert_eq!(dimension.kind, DimensionKind::Linear);
        // Along 30 degrees: 10 cos 30 + 5 sin 30 = 8.660254... + 2.5.
        near(
            dimension.measurement(),
            10.0 * 3.0_f64.sqrt() / 2.0 + 2.5,
            1e-12,
        );
    }

    #[test]
    fn two_points_give_the_angle_and_undo_takes_it_back() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimlinear");
        driver.click(0.0, 0.0);
        driver.click(10.0, 5.0);
        driver.r#type("Rotated");
        driver.click(1.0, 1.0);
        assert_eq!(driver.prompt(), "Specify second point");
        assert_eq!(
            driver.click(1.0, 1.0).outcome,
            REJECTED,
            "two points, one place"
        );
        driver.click(3.0, 3.0); // 45 degrees
        assert!(
            contains(&driver.prompt(), "rotated 45 degrees"),
            "{}",
            driver.prompt()
        );
        driver.undo(); // the angle
        assert!(
            !contains(&driver.prompt(), "rotated"),
            "{}",
            driver.prompt()
        );
        driver.r#type("R");
        driver.click(0.0, 0.0);
        driver.click(4.0, 4.0);
        assert_eq!(driver.click(-5.0, 10.0).outcome, DONE);
        let dimension = newest_dimension(driver.document());
        assert_eq!(dimension.kind, DimensionKind::Linear);
        near(dimension.angle, 0.25 * PI, 1e-15);
        // Along 45 degrees: (10 + 5) / sqrt 2.
        near(dimension.measurement(), 15.0 / 2.0_f64.sqrt(), 1e-12);
    }

    #[test]
    fn a_line_between_the_origins_levels_is_the_linear_kind_dim_horizontal_makes() {
        // Once refused: the aligned projection the tool stores cannot draw a
        // line between the two levels, whose extension lines leave in
        // opposite directions. A Linear-kind dimension draws each from its
        // own origin, as AutoCAD's DIMLINEAR does, so that is what is made
        // there.
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimlinear");
        driver.click(0.0, 0.0);
        driver.click(8.0, 6.0);
        driver.r#type("H");
        let done = driver.click(12.0, 3.0);
        assert_eq!(done.outcome, DONE, "{}", done.message);
        assert_eq!(done.message, "linear dimension measuring 8");
        let mut typed = Document::default();
        run(&mut typed, "DIM HORIZONTAL 0,0 8,6 at=12,3");
        expect_same(driver.document(), &typed, EntityType::Dimension);
        assert_eq!(
            newest_dimension(driver.document()).kind,
            DimensionKind::Linear
        );
    }
}

// ---- Ordinate: Datum -----------------------------------------------------------------------

mod ordinate_datum {
    use katana_cad::Document;
    use katana_entity::EntityType;

    use super::expect_same;
    use crate::tool_driver::ToolDriver;
    use crate::{CONTINUE, DONE, run};

    #[test]
    fn a_datum_measures_from_it_as_dim_ordinate_datum_does_and_is_kept_for_the_next() {
        let mut driver = ToolDriver::new();
        driver.start("annotate.dimordinate");
        assert_eq!(driver.prompt(), "Specify feature location or [Datum]");
        driver.r#type("D");
        assert_eq!(driver.prompt(), "Specify the datum point <0,0>");
        assert_eq!(driver.r#type("100,200").outcome, CONTINUE);
        assert_eq!(
            driver.prompt(),
            "Specify feature location or [Datum] (datum 100,200)"
        );
        driver.click(112.0, 207.0);
        let up = driver.click(112.0, 215.0);
        assert_eq!(up.outcome, DONE);
        assert_eq!(up.message, "X ordinate dimension measuring 12");
        let mut typed = Document::default();
        run(&mut typed, "DIM ORDINATE 112,207 at=112,215 datum=100,200");
        expect_same(driver.document(), &typed, EntityType::Dimension);

        // Started again, it measures from the newest ordinate's datum: the
        // drawing keeps it, not the program.
        assert_eq!(
            driver.prompt(),
            "Specify feature location or [Datum] (datum 100,200)"
        );
        driver.click(130.0, 210.0);
        let across = driver.click(140.0, 210.0);
        assert_eq!(across.message, "Y ordinate dimension measuring 10");
        // Enter at the datum prompt keeps it; 0,0 goes back to the origin.
        driver.r#type("D");
        assert_eq!(driver.enter().outcome, CONTINUE);
        assert_eq!(
            driver.prompt(),
            "Specify feature location or [Datum] (datum 100,200)"
        );
        driver.r#type("D");
        driver.r#type("0,0");
        assert_eq!(driver.prompt(), "Specify feature location or [Datum]");
    }
}

// ---- associative points ------------------------------------------------------------------

mod associative_points {
    use katana_cad::Document;
    use katana_entity::{AnchorPoint, EntityType};

    use super::{expect_same, newest_dimension, newest_leader};
    use crate::tool_driver::ToolDriver;
    use crate::{DONE, anchor, pt, run};

    #[test]
    fn an_aligned_dimension_snapped_to_a_lines_ends_follows_the_line() {
        let mut driver = ToolDriver::new();
        run(driver.document_mut(), "LINE 0,0 10,0"); // 1
        driver.start("annotate.dimaligned");
        driver.click_snapped(0.1, 0.1); // its start, 0.14 away
        driver.click_snapped(9.9, -0.1); // its end
        assert_eq!(driver.click(5.0, 4.0).outcome, DONE);
        let made = newest_dimension(driver.document());
        assert_eq!(made.start, pt(0.0, 0.0));
        assert_eq!(made.end, pt(10.0, 0.0));
        assert_eq!(made.start_ref, anchor(1, AnchorPoint::Start, 0));
        assert_eq!(made.end_ref, anchor(1, AnchorPoint::End, 0));
        // As DIM ALIGNED #1.start #1.end at=5,4.
        let mut typed = Document::default();
        run(&mut typed, "LINE 0,0 10,0");
        run(&mut typed, "DIM ALIGNED #1.start #1.end at=5,4");
        expect_same(driver.document(), &typed, EntityType::Dimension);

        // Moved 3 up, the dimension goes with it.
        run(driver.document_mut(), "SELECT 1");
        run(driver.document_mut(), "MOVE 0,3");
        let followed = newest_dimension(driver.document());
        assert_eq!(followed.start, pt(0.0, 3.0));
        assert_eq!(followed.end, pt(10.0, 3.0));
        // A click with nothing to snap to is a plain point, as before.
        driver.start("annotate.dimaligned");
        driver.click_snapped(50.0, 50.0);
        driver.click_snapped(60.0, 50.0);
        assert_eq!(driver.click(55.0, 55.0).outcome, DONE);
        assert!(!newest_dimension(driver.document()).start_ref.associated());
    }

    #[test]
    fn a_snapped_linear_dimension_is_the_linear_kind_and_follows() {
        // The aligned projection's start and end are not the origins, so it
        // could not follow them; a snapped origin makes the Linear kind, which
        // measures from the origins themselves.
        let mut driver = ToolDriver::new();
        run(driver.document_mut(), "LINE 0,0 8,6"); // 1
        driver.start("annotate.dimlinear");
        driver.click_snapped(0.0, 0.1);
        driver.click_snapped(8.0, 6.1);
        assert_eq!(driver.click(4.0, 10.0).outcome, DONE);
        let mut typed = Document::default();
        run(&mut typed, "LINE 0,0 8,6");
        run(&mut typed, "DIM HORIZONTAL #1.start #1.end at=4,10");
        expect_same(driver.document(), &typed, EntityType::Dimension);
        run(driver.document_mut(), "SELECT 1");
        run(driver.document_mut(), "MOVE 2,0");
        let followed = newest_dimension(driver.document());
        assert_eq!(followed.start, pt(2.0, 0.0));
        assert_eq!(followed.measurement(), 8.0);
    }

    #[test]
    fn an_ordinate_an_angle_and_a_leader_tip_follow_what_they_were_snapped_to() {
        let mut driver = ToolDriver::new();
        run(driver.document_mut(), "LINE 0,0 10,0"); // 1
        run(driver.document_mut(), "LINE 0,0 0,10"); // 2
        run(driver.document_mut(), "CIRCLE 20,20 2"); // 3

        // An ordinate of the circle's centre.
        driver.start("annotate.dimordinate");
        driver.click_snapped(20.1, 19.9);
        assert_eq!(driver.click(20.0, 30.0).outcome, DONE);
        assert_eq!(
            newest_dimension(driver.document()).start_ref,
            anchor(3, AnchorPoint::Centre, 0)
        );

        // An angle at the shared corner between the two lines' far ends.
        driver.start("annotate.dimangular");
        driver.enter();
        driver.click_snapped(0.1, 0.1); // line 1's start
        driver.click_snapped(9.9, 0.1); // line 1's end
        driver.click_snapped(0.1, 9.9); // line 2's end
        assert_eq!(driver.click(3.0, 3.0).outcome, DONE);
        let angle = newest_dimension(driver.document());
        assert_eq!(angle.vertex_ref.entity, 1);
        assert_eq!(angle.start_ref, anchor(1, AnchorPoint::End, 0));
        assert_eq!(angle.end_ref, anchor(2, AnchorPoint::End, 0));

        // A leader whose tip is on line 2's middle.
        driver.start("annotate.leader");
        driver.click_snapped(0.1, 5.1);
        driver.click(-5.0, 8.0);
        driver.enter();
        assert_eq!(driver.enter().outcome, DONE);
        assert_eq!(
            newest_leader(driver.document()).tip_ref,
            anchor(2, AnchorPoint::Mid, 0)
        );

        // Everything moves with what it was snapped to.
        run(driver.document_mut(), "SELECT ALL");
        run(driver.document_mut(), "SELECT 1 2 3");
        run(driver.document_mut(), "MOVE 100,0");
        assert_eq!(newest_dimension(driver.document()).vertex, pt(100.0, 0.0));
        assert_eq!(newest_leader(driver.document()).vertices[0], pt(100.0, 5.0));
    }
}
