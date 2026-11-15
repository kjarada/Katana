//! tests/cad/drawing/test_drawing_verbs.cpp: the drawing system's verbs of the
//! command line (drawing/drawing_verbs.rs, docs/drawing.md "The command
//! line"): each verb driven as an agent drives it, its reply read back as
//! key=value records, and each edit taken back by ONE undo.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use katana_cad::drawing::construction::{CONSTRUCTION_LAYER, CONSTRUCTION_REACH};
use katana_cad::drawing::vertex_editing::read_polyline;
use katana_cad::snapping::{ALL_SNAP_MODES, has_mode};
use katana_cad::{AngleConvention, CommandInterpreter, SnapMode};
use katana_core::ErrorCode;
use katana_entity::{EntityId, EntityType, Geometry};
use katana_geometry::{CurvePolyline2, Ellipse2, Segment2, Spline2};
use katana_math::{DEG_TO_RAD, PI};

use crate::{Session, double_eq, leading_number, near, pt};

// The polyline an id names, read the way the verbs read it.
#[track_caller]
fn polyline(s: &Session, id: EntityId) -> CurvePolyline2 {
    read_polyline(&s.entity(id)).unwrap_or_default()
}

#[track_caller]
fn kind(s: &Session, id: EntityId) -> EntityType {
    s.entity(id).r#type()
}

#[track_caller]
fn segment_of(s: &Session, id: EntityId) -> Segment2 {
    let entity = s.entity(id);
    match entity.geometry {
        Geometry::Line(segment) => segment,
        other => panic!("not a line: {other:?}"),
    }
}

#[track_caller]
fn spline_of(s: &Session, id: EntityId) -> Spline2 {
    let entity = s.entity(id);
    match entity.geometry {
        Geometry::Spline(spline) => spline,
        other => panic!("not a spline: {other:?}"),
    }
}

#[track_caller]
fn ellipse_of(s: &Session, id: EntityId) -> Ellipse2 {
    let entity = s.entity(id);
    match entity.geometry {
        Geometry::Ellipse(ellipse) => ellipse,
        other => panic!("not an ellipse: {other:?}"),
    }
}

// The height of vertex `index`, failing the test when it has none.
#[track_caller]
fn height(polyline: &CurvePolyline2, index: usize) -> f64 {
    match polyline.vertices[index].height {
        Some(height) => height,
        None => panic!("vertex {index} has no height"),
    }
}

// The lines of a reply, as std::getline reads them.
fn reply_lines(text: &str) -> Vec<String> {
    text.lines().map(String::from).collect()
}

// The value of `key` in a key=value record, or "" when it has none.
fn value(record: &str, key: &str) -> String {
    let prefix = format!("{key}=");
    String::from(
        record
            .split_ascii_whitespace()
            .find_map(|token| token.strip_prefix(prefix.as_str()))
            .unwrap_or(""),
    )
}

#[track_caller]
fn id_of(record: &str) -> EntityId {
    leading_number(&value(record, "id"))
}

// A three-vertex polyline east then north, made through the verb.
#[track_caller]
fn corner(s: &mut Session) -> EntityId {
    id_of(&s.run("PLINE 0,0,100 10,0,101 10,10"))
}

mod drawing_verbs {
    use super::*;

    // ---- routing -------------------------------------------------------------------------------

    #[test]
    fn a_plain_pline_replies_with_its_id_and_open_still_opens_a_project() {
        let mut s = Session::default();
        let reply = s.run("PLINE 0,0 10,0 10,10 C");
        assert_eq!(value(&reply, "kind"), "polyline");
        assert_eq!(value(&reply, "closed"), "yes");
        assert_eq!(kind(&s, id_of(&reply)), EntityType::Polyline);
        // OPEN with a directory is the file verb's (and fails: there is none).
        assert_eq!(s.fails("OPEN /no/such/katana/project"), ErrorCode::NotFound);
    }

    #[test]
    fn help_lists_the_drawing_verbs() {
        let help = CommandInterpreter::help_text();
        for verb in [
            "VERTEX LIST",
            "WEED",
            "DENSIFY",
            "STRAIGHTEN",
            "STARTVERTEX",
            "VERTEXZ",
            "PLINE3D",
            "SPLINE",
            "ELLIPSE",
            "XLINE",
            "RAY",
            "DLINE",
            "ORTHO",
            "POLAR",
            "SNAP",
            "LOCK",
            "ANGLES",
        ] {
            assert!(help.contains(verb), "{verb}");
        }
    }

    // ---- VERTEX ----------------------------------------------------------------------------------

    #[test]
    fn vertex_list_gives_a_summary_and_a_record_per_vertex() {
        let mut s = Session::default();
        let id = corner(&mut s);
        let reply = reply_lines(&s.run(&format!("VERTEX LIST {id}")));
        assert_eq!(reply.len(), 4);
        assert_eq!(value(&reply[0], "kind"), "polyline");
        assert_eq!(value(&reply[0], "vertices"), "3");
        assert_eq!(value(&reply[0], "heights"), "2");
        assert_eq!(value(&reply[0], "length"), "20");
        assert_eq!(value(&reply[1], "index"), "0");
        assert_eq!(value(&reply[1], "z"), "100");
        assert_eq!(
            value(&reply[1], "bearing"),
            "90",
            "east is a whole-circle bearing of 90"
        );
        assert_eq!(value(&reply[2], "bearing"), "0");
        assert_eq!(value(&reply[2], "distance"), "10");
        assert_eq!(value(&reply[3], "z"), "none", "no height is not zero");
        assert_eq!(value(&reply[3], "bearing"), "none");
        assert_eq!(
            s.run(&format!("VERTEX LIST #{id}")),
            s.run(&format!("VERTEX LIST {id}"))
        );
    }

    #[test]
    fn vertex_insert_splits_the_nearest_segment_or_the_one_named() {
        let mut s = Session::default();
        let id = corner(&mut s);
        let reply = s.run(&format!("VERTEX INSERT {id} 10,5"));
        assert_eq!(value(&reply, "inserted"), "2");
        assert_eq!(value(&reply, "vertices"), "4");
        assert_eq!(polyline(&s, id).vertices[2].position, pt(10.0, 5.0));

        s.run(&format!("VERTEX INSERT {id} 5,-1,100.5 after=0"));
        let p = polyline(&s, id);
        assert_eq!(p.vertices.len(), 5);
        assert_eq!(p.vertices[1].position, pt(5.0, -1.0));
        // A typed z is the new vertex's height.
        double_eq(height(&p, 1), 100.5);

        assert_eq!(
            s.fails(&format!("VERTEX INSERT {id} 1,1 after=9")),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            s.fails(&format!("VERTEX INSERT {id} 1,1 before=1")),
            ErrorCode::InvalidArgument,
            "an unknown option is refused, not ignored"
        );
    }

    #[test]
    fn vertex_insert_at_a_point_of_the_polyline_takes_that_segment_and_a_height() {
        let mut s = Session::default();
        // A polyline that doubles back, (0,0) (10,0) (0,0): the middle of
        // segment 1, (5,0), lies on segment 0 too, which the nearest segment
        // would take. "#id.s1" names segment 1 itself: the vertex after it.
        let back = id_of(&s.run("PLINE 0,0 10,0 0,0"));
        assert_eq!(
            value(
                &s.run(&format!("VERTEX INSERT {back} #{back}.s1")),
                "inserted"
            ),
            "2"
        );
        assert_eq!(
            polyline(&s, back).positions(),
            [pt(0.0, 0.0), pt(10.0, 0.0), pt(5.0, 0.0), pt(0.0, 0.0)]
        );

        // "#id@x,y,z": on the line nearest x,y, with z its height.
        let id = corner(&mut s);
        assert_eq!(
            value(
                &s.run(&format!("VERTEX INSERT {id} #{id}@4,0.3,101.5")),
                "inserted"
            ),
            "1"
        );
        let on = polyline(&s, id).vertices[1];
        near(on.position.x, 4.0, 1.0e-12);
        near(on.position.y, 0.0, 1.0e-12);
        let Some(on_height) = on.height else {
            panic!("the inserted vertex has no height");
        };
        double_eq(on_height, 101.5);

        // A segment it has not got: refused, and in words about the polyline's
        // point, not a dimension's.
        let no_segment = s
            .attempt(&format!("VERTEX INSERT {id} #{id}.s9"))
            .expect_err("a segment the polyline has not got");
        assert_eq!(no_segment.code, ErrorCode::InvalidArgument);
        assert!(
            !no_segment.describe().contains("dimension"),
            "{}",
            no_segment.describe()
        );
        // An entity there is not.
        assert_eq!(
            s.fails(&format!("VERTEX INSERT {id} #999@1,1")),
            ErrorCode::NotFound
        );
        // A point on a vertex already: refused, the polyline as it was.
        let count = polyline(&s, id).vertices.len();
        let _ = s.fails(&format!("VERTEX INSERT {id} #{id}@10,0"));
        assert_eq!(polyline(&s, id).vertices.len(), count);
        // A point of ANOTHER entity is that entity's: the end (3,2) of this
        // line is nearest (3,5), and goes into the corner's nearest segment, 0.
        let other = id_of(&s.run("PLINE 3,-2 3,2"));
        assert_eq!(
            value(
                &s.run(&format!("VERTEX INSERT {id} #{other}@3,5")),
                "inserted"
            ),
            "1"
        );
        assert_eq!(polyline(&s, id).vertices[1].position, pt(3.0, 2.0));
    }

    #[test]
    fn vertex_insert_refuses_an_after_that_is_not_the_segment_of_its_point() {
        // "#id@4,0.3" is on segment 0 of (0,0) (10,0) (10,10). after=1 put that
        // point after vertex 1 - (0,0) (10,0) (4,0) (10,10) - and the polyline
        // doubled back, 27.66 long for 20. The point names its segment; an
        // after= that agrees (the grip menu's "#id.sN after=N") is taken.
        let mut s = Session::default();
        let id = corner(&mut s);
        let folded = s
            .attempt(&format!("VERTEX INSERT {id} #{id}@4,0.3 after=1"))
            .expect_err("an after= that is not the point's segment");
        assert_eq!(folded.code, ErrorCode::InvalidArgument);
        assert!(
            folded
                .describe()
                .contains("is on segment 0, not after vertex 1"),
            "{}",
            folded.describe()
        );
        assert_eq!(
            polyline(&s, id).positions(),
            [pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)]
        );
        assert_eq!(
            value(
                &s.run(&format!("VERTEX INSERT {id} #{id}.s1 after=1")),
                "inserted"
            ),
            "2"
        );
        assert_eq!(
            polyline(&s, id).positions(),
            [pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 5.0), pt(10.0, 10.0)]
        );
    }

    #[test]
    fn straighten_and_grade_walk_forward_unless_the_short_or_long_side_is_asked_for() {
        // The hexagon (0,0) (10,0) (20,0) (20,10) (10,10) (0,10): from vertex 3
        // to vertex 1, forward is 4, 5 and 0 and the other way only 2. The
        // verbs walk forward from the first number, as they always have, so a
        // line written for them takes out the three it did; the window's rule
        // - the shorter side, and O for the other - is side=short and
        // side=long. Made the verbs' default, it silently changed which
        // vertices an existing script or agent line took out.
        let mut s = Session::default();
        let h = id_of(&s.run("PLINE 0,0 10,0 20,0 20,10 10,10 0,10 CLOSE"));
        let forward = [pt(10.0, 0.0), pt(20.0, 0.0), pt(20.0, 10.0)];
        let short_side = [
            pt(0.0, 0.0),
            pt(10.0, 0.0),
            pt(20.0, 10.0),
            pt(10.0, 10.0),
            pt(0.0, 10.0),
        ];
        assert_eq!(
            value(&s.run(&format!("STRAIGHTEN {h} 3 1")), "vertices"),
            "3"
        );
        assert_eq!(polyline(&s, h).positions(), forward);
        assert!(s.document.undo().is_ok());
        assert_eq!(
            value(
                &s.run(&format!("STRAIGHTEN {h} 3 1 side=short")),
                "vertices"
            ),
            "5"
        );
        assert_eq!(polyline(&s, h).positions(), short_side);
        assert!(s.document.undo().is_ok());
        // The long side from 1 to 3 is the forward walk from 3 to 1.
        assert_eq!(
            value(&s.run(&format!("STRAIGHTEN {h} 1 3 side=long")), "vertices"),
            "3"
        );
        assert_eq!(polyline(&s, h).positions(), forward);
        assert!(s.document.undo().is_ok());
        // And forward from 1 to 3 is the short side here.
        assert_eq!(
            value(&s.run(&format!("STRAIGHTEN {h} 1 3")), "vertices"),
            "5"
        );
        assert_eq!(polyline(&s, h).positions(), short_side);
        assert_eq!(
            s.fails(&format!("STRAIGHTEN {h} 0 1 side=sideways")),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            s.fails(&format!("STRAIGHTEN {h} 0 3 side=other")),
            ErrorCode::InvalidArgument,
            "short or long, not the window's letter"
        );
        // An open polyline has one way between two vertices: side=short walks
        // it, side=long has nothing to walk.
        let open = id_of(&s.run("PLINE 0,0 10,0 20,0"));
        assert_eq!(
            s.fails(&format!("STRAIGHTEN {open} 0 2 side=long")),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            value(
                &s.run(&format!("STRAIGHTEN {open} 2 0 side=short")),
                "vertices"
            ),
            "2"
        );

        // Grade on the same hexagon with heights: 0 at vertex 1, 10 at vertex
        // 3. Forward from 3 to 1 through 4, 5 and 0 is 40 long in four steps of
        // 10: 7.5, 5 and 2.5. The short way, 1 to 3, puts vertex 2 halfway by
        // length, at 5.
        let z = id_of(&s.run("PLINE3D 0,0,0 10,0,0 20,0,0 20,10,10 10,10,0 0,10,0 CLOSE"));
        s.run(&format!("VERTEXZ {z} GRADE 3 1"));
        let graded = polyline(&s, z);
        double_eq(height(&graded, 4), 7.5);
        double_eq(height(&graded, 5), 5.0);
        double_eq(height(&graded, 0), 2.5);
        // The short way is left alone.
        double_eq(height(&graded, 2), 0.0);
        assert!(s.document.undo().is_ok());
        s.run(&format!("VERTEXZ {z} GRADE 3 1 side=short"));
        let graded = polyline(&s, z);
        double_eq(height(&graded, 2), 5.0);
        // The long way is left alone.
        double_eq(height(&graded, 4), 0.0);
        assert!(s.document.undo().is_ok());
        s.run(&format!("VERTEXZ {z} GRADE 1 3 side=long"));
        let graded = polyline(&s, z);
        double_eq(height(&graded, 4), 7.5);
        double_eq(height(&graded, 0), 2.5);
    }

    #[test]
    fn vertex_delete_removes_several_at_once_as_one_undo_step() {
        let mut s = Session::default();
        let id = id_of(&s.run("PLINE 0,0 1,0 2,0 3,0 4,0 ARC 5,1"));
        assert_eq!(
            value(&s.run(&format!("VERTEX DELETE {id} 1 3")), "vertices"),
            "4"
        );
        assert!(s.document.undo().is_ok());
        assert_eq!(
            polyline(&s, id).vertices.len(),
            6,
            "one undo takes back the whole delete"
        );
        assert_eq!(
            s.fails(&format!("VERTEX DELETE {id} 6")),
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn vertex_move_takes_absolute_relative_and_polar_points_from_the_vertex() {
        let mut s = Session::default();
        let id = corner(&mut s);
        let reply = reply_lines(&s.run(&format!("VERTEX MOVE {id} 2 12,12")));
        assert_eq!(reply.len(), 2);
        assert_eq!(value(&reply[1], "x"), "12");
        s.run(&format!("VERTEX MOVE {id} 2 @1,-2"));
        assert_eq!(
            polyline(&s, id).vertices[2].position,
            pt(13.0, 10.0),
            "@ is from the vertex"
        );
        s.run(&format!("VERTEX MOVE {id} 1 @0,0,0.5"));
        // A relative dz changes the height.
        double_eq(height(&polyline(&s, id), 1), 101.5);
        s.run(&format!("VERTEX MOVE {id} 0 0,0,99"));
        double_eq(height(&polyline(&s, id), 0), 99.0);
        assert_eq!(
            s.fails(&format!("VERTEX MOVE {id} 2 @0,0,1")),
            ErrorCode::InvalidArgument,
            "no height for a dz to change"
        );
    }

    #[test]
    fn vertex_set_edits_the_columns_the_vertices_panel_shows() {
        let mut s = Session::default();
        let id = corner(&mut s);
        s.run(&format!("VERTEX SET {id} 1 x=11 z=none"));
        let p = polyline(&s, id);
        assert_eq!(p.vertices[1].position, pt(11.0, 0.0));
        assert!(p.vertices[1].height.is_none());
        // A bearing and a distance move the NEXT vertex, as a traverse is
        // edited.
        s.run(&format!("VERTEX SET {id} 0 bearing=90 distance=20"));
        let p = polyline(&s, id);
        near(p.vertices[1].position.x, 20.0, 1e-9);
        near(p.vertices[1].position.y, 0.0, 1e-9);
        let arc = s.run(&format!("VERTEX SET {id} 1 bulge=0.5"));
        assert_eq!(
            value(&arc, "kind"),
            "curvepolyline",
            "a bulge makes it a curve polyline"
        );
        assert_eq!(kind(&s, id), EntityType::CurvePolyline);
        assert!(s.document.undo().is_ok());
        assert_eq!(kind(&s, id), EntityType::Polyline);
        assert_eq!(
            s.fails(&format!("VERTEX SET {id} 0 colour=red")),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            s.fails(&format!("VERTEX SET {id} 2 distance=4")),
            ErrorCode::InvalidArgument,
            "the last vertex of an open polyline starts no segment"
        );
    }

    #[test]
    fn vertex_verbs_refuse_what_is_not_a_polyline() {
        let mut s = Session::default();
        s.run("LINE 0,0 1,1");
        let line = s.last();
        assert_eq!(
            s.fails(&format!("VERTEX LIST {line}")),
            ErrorCode::InvalidArgument
        );
        assert_eq!(s.fails("VERTEX LIST 999"), ErrorCode::NotFound);
        assert_eq!(s.fails("VERTEX FROB 1"), ErrorCode::ParseFailure);
    }

    // ---- whole-polyline edits ------------------------------------------------------------------

    #[test]
    fn weed_simplifies_within_the_tolerance_and_says_how_many_were_there() {
        let mut s = Session::default();
        let id = id_of(&s.run("PLINE 0,0 1,0.001 2,0 3,0.5 4,0"));
        let reply = s.run(&format!("WEED {id} tolerance=0.01"));
        assert_eq!(value(&reply, "before"), "5");
        assert_eq!(value(&reply, "vertices"), "4");
        assert_eq!(s.fails(&format!("WEED {id}")), ErrorCode::InvalidArgument);
        assert_eq!(
            s.fails(&format!("WEED {id} tolerance=0")),
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn weed_and_densify_act_on_the_selection_as_one_step() {
        let mut s = Session::default();
        let a = id_of(&s.run("PLINE 0,0 10,0 10,10 LINE"));
        let b = id_of(&s.run("PLINE 0,20 10,20 L"));
        s.run("SELECT ALL");
        let reply = reply_lines(&s.run("DENSIFY SELECTION interval=2.5"));
        assert_eq!(reply.len(), 2);
        assert_eq!(polyline(&s, a).vertices.len(), 9);
        assert_eq!(polyline(&s, b).vertices.len(), 5);
        assert!(s.document.undo().is_ok());
        assert_eq!(polyline(&s, a).vertices.len(), 3);
        assert_eq!(polyline(&s, b).vertices.len(), 2);
    }

    #[test]
    fn densify_chords_arcs_when_given_a_chord_tolerance() {
        let mut s = Session::default();
        let id = id_of(&s.run("PLINE 0,0 10,0 ARC 20,10"));
        assert_eq!(kind(&s, id), EntityType::CurvePolyline);
        s.run(&format!("DENSIFY {id} chord=0.01"));
        assert_eq!(
            kind(&s, id),
            EntityType::Polyline,
            "chorded, it is straight"
        );
        assert!(polyline(&s, id).vertices.len() > 10);
    }

    #[test]
    fn close_and_open_on_ids_the_selection_or_nothing() {
        let mut s = Session::default();
        let id = corner(&mut s);
        assert_eq!(value(&s.run(&format!("CLOSE {id}")), "closed"), "yes");
        assert_eq!(value(&s.run(&format!("OPEN #{id}")), "closed"), "no");
        s.run(&format!("SELECT {id}"));
        assert_eq!(value(&s.run("CLOSE"), "closed"), "yes");
        assert_eq!(value(&s.run("OPEN"), "closed"), "no");
        assert_eq!(
            value(&s.run("OPEN SELECTION"), "closed"),
            "no",
            "opening an open one leaves it"
        );
        s.run("SELECT NONE");
        assert_eq!(s.fails("CLOSE"), ErrorCode::InvalidState);
    }

    #[test]
    fn straighten_start_vertex_and_the_height_verbs() {
        let mut s = Session::default();
        let id = id_of(&s.run("PLINE3D 0,0,10 1,1 2,-1 3,0,13"));
        assert_eq!(
            value(&s.run(&format!("VERTEXZ {id} INTERPOLATE")), "heights"),
            "4"
        );
        let p = polyline(&s, id);
        assert!(height(&p, 1) > 10.0);
        assert!(height(&p, 2) < 13.0);
        s.run(&format!("VERTEXZ {id} 1 none"));
        assert!(polyline(&s, id).vertices[1].height.is_none());
        s.run(&format!("VERTEXZ {id} 1 50"));
        s.run(&format!("VERTEXZ {id} GRADE 0 3"));
        assert!(
            height(&polyline(&s, id), 1) < 13.0,
            "graded back between the ends"
        );
        assert_eq!(
            value(&s.run(&format!("STRAIGHTEN {id} 0 3")), "vertices"),
            "2"
        );

        let square = id_of(&s.run("PLINE 0,0 1,0 1,1 0,1 CLOSE"));
        s.run(&format!("STARTVERTEX {square} 2"));
        assert_eq!(polyline(&s, square).vertices[0].position, pt(1.0, 1.0));
        assert_eq!(
            s.fails(&format!("STARTVERTEX {square} 7")),
            ErrorCode::InvalidArgument
        );
    }

    // ---- the draw verbs ---------------------------------------------------------------------------

    #[test]
    fn pline_arc_draws_tangent_arcs_and_line_goes_back_to_straight() {
        let mut s = Session::default();
        let reply = s.run("PLINE 0,0 10,0 ARC 20,10 LINE 20,20");
        assert_eq!(value(&reply, "kind"), "curvepolyline");
        assert_eq!(value(&reply, "arcs"), "yes");
        let p = polyline(&s, id_of(&reply));
        assert_eq!(p.vertices.len(), 4);
        double_eq(p.vertices[0].bulge, 0.0);
        // Leaving east, ending north at 20,10: a counter-clockwise quarter
        // circle, bulge tan(90/4).
        near(p.vertices[1].bulge, (PI / 8.0).tan(), 1e-12);
        double_eq(p.vertices[2].bulge, 0.0);
        assert!(s.document.undo().is_ok());
        assert_eq!(s.document.model().entities.size(), 0);
    }

    #[test]
    fn pline_closed_in_arc_mode_closes_with_a_tangent_arc() {
        let mut s = Session::default();
        let reply = s.run("PLINE 0,0 10,0 ARC 10,10 CLOSE");
        let p = polyline(&s, id_of(&reply));
        assert!(p.closed);
        let Some(last) = p.vertices.last() else {
            panic!("the polyline has no vertex");
        };
        assert_ne!(last.bulge, 0.0);
    }

    #[test]
    fn pline3d_gives_each_point_its_height_or_the_default() {
        let mut s = Session::default();
        let reply = s.run("PLINE3D 0,0,5 10,0 10,10,7 z=6");
        assert_eq!(
            value(&reply, "kind"),
            "polyline",
            "a straight 3D string stays a plain polyline"
        );
        let p = polyline(&s, id_of(&reply));
        double_eq(height(&p, 0), 5.0);
        double_eq(height(&p, 1), 6.0);
        double_eq(height(&p, 2), 7.0);
        assert_eq!(s.fails("PLINE3D 0,0 0,0"), ErrorCode::InvalidGeometry);
    }

    #[test]
    fn spline_through_points_or_by_control_points() {
        let mut s = Session::default();
        let fit = s.run("SPLINE 0,0 5,5 10,0 15,5");
        assert_eq!(value(&fit, "kind"), "spline");
        let spline = spline_of(&s, id_of(&fit));
        assert_eq!(spline.fit_points.len(), 4);
        let control = s.run("SPL 0,0 5,5 10,0 control=on degree=2");
        let by_control = spline_of(&s, id_of(&control));
        assert_eq!(by_control.degree, 2);
        assert!(by_control.fit_points.is_empty());
        assert_eq!(
            s.fails("SPLINE 0,0 1,1 degree=11"),
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn ellipse_by_centre_axis_and_minor_or_ratio_and_as_an_arc() {
        let mut s = Session::default();
        let full = s.run("ELLIPSE 0,0 10,0 minor=4");
        let e = ellipse_of(&s, id_of(&full));
        double_eq(e.major_radius(), 10.0);
        double_eq(e.minor_radius(), 4.0);
        assert!(e.is_full());
        let arc = s.run("EL 0,0 0,10 ratio=0.5 start=0 end=90");
        let a = ellipse_of(&s, id_of(&arc));
        near(a.sweep, PI / 2.0, 1e-12);
        assert_eq!(s.fails("ELLIPSE 0,0 10,0"), ErrorCode::InvalidArgument);
        assert_eq!(
            s.fails("ELLIPSE 0,0 10,0 minor=4 start=10"),
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn xline_and_ray_go_on_the_construction_layer_with_it_in_one_step() {
        let mut s = Session::default();
        let reply = reply_lines(&s.run("XLINE 0,0 1,1 angle=90"));
        assert_eq!(reply.len(), 2);
        assert_eq!(value(&reply[0], "layer"), CONSTRUCTION_LAYER);
        assert!(s.document.model().layers.find(CONSTRUCTION_LAYER).is_some());
        let line = segment_of(&s, id_of(&reply[0]));
        near(line.length(), 2.0 * CONSTRUCTION_REACH, 1e-6);
        assert!(s.document.undo().is_ok());
        assert!(
            s.document.model().layers.find(CONSTRUCTION_LAYER).is_none(),
            "the layer made for the lines goes with them"
        );

        let ray = reply_lines(&s.run("RAY 5,5 5,6"));
        assert_eq!(ray.len(), 1);
        let r = segment_of(&s, id_of(&ray[0]));
        assert_eq!(r.start, pt(5.0, 5.0));
        near(r.end.y, 5.0 + CONSTRUCTION_REACH, 1e-6);
        assert_eq!(s.fails("RAY 5,5 5,5"), ErrorCode::InvalidGeometry);
    }

    #[test]
    fn xline_angles_follow_the_angle_convention() {
        let mut s = Session::default();
        s.run("ANGLES bearing");
        let reply = reply_lines(&s.run("XLINE 0,0 angle=0"));
        let line = segment_of(&s, id_of(&reply[0]));
        // A bearing of 0 is north: the line runs up the y axis.
        near(line.start.x, 0.0, 1e-6);
        let quadrant = reply_lines(&s.run("XLINE 0,0 angle=N90dE"));
        let east = segment_of(&s, id_of(&quadrant[0]));
        near(east.start.y, 0.0, 1e-6);
    }

    #[test]
    fn dline_draws_both_sides_of_the_path() {
        let mut s = Session::default();
        let reply = reply_lines(&s.run("DLINE 0,0 10,0 10,10 width=2"));
        assert_eq!(reply.len(), 2);
        let left = polyline(&s, id_of(&reply[0]));
        let right = polyline(&s, id_of(&reply[1]));
        near(left.vertices[0].position.y, 1.0, 1e-9);
        near(right.vertices[0].position.y, -1.0, 1e-9);
        assert!(s.document.undo().is_ok());
        assert_eq!(
            s.document.model().entities.size(),
            0,
            "both sides are one step"
        );
        assert_eq!(s.fails("DLINE 0,0 10,0"), ErrorCode::InvalidArgument);
    }

    // ---- drafting settings --------------------------------------------------------------------------

    #[test]
    fn drafting_verbs_set_the_shared_settings_and_reply_with_them_all() {
        let mut s = Session::default();
        // The C++ counts in a local the listener captures by reference; a
        // Document listener is Send and 'static, so the count is shared.
        let changes = Arc::new(AtomicUsize::new(0));
        let heard = Arc::clone(&changes);
        let _listener = s.document.add_listener(move || {
            heard.fetch_add(1, Ordering::SeqCst);
        });
        let reply = s.run("ORTHO on");
        assert!(s.document.drafting().ortho);
        assert_eq!(value(&reply, "ortho"), "on");
        assert_eq!(value(&reply, "polar"), "off");
        assert!(changes.load(Ordering::SeqCst) >= 1, "the views hear of it");
        let reply = s.run("POLAR on increment=22.5");
        near(
            s.document.drafting().polar_increment,
            22.5 * DEG_TO_RAD,
            1e-12,
        );
        assert_eq!(value(&reply, "increment"), "22.5");
        s.run("TRACKING on");
        assert!(s.document.drafting().object_tracking);
        s.run("ANGLES bearing");
        assert_eq!(s.document.drafting().angles, AngleConvention::Bearing);
        let reply = s.run("LOCK angle=90 length=12.5");
        let Some(angle_lock) = s.document.drafting().angle_lock else {
            panic!("no angle lock");
        };
        // A bearing of 90 is east.
        near(angle_lock, 0.0, 1e-12);
        assert_eq!(value(&reply, "lengthlock"), "12.5");
        s.run("LOCK OFF");
        assert!(s.document.drafting().angle_lock.is_none());
        assert!(s.document.drafting().length_lock.is_none());
        assert_eq!(s.fails("ORTHO sideways"), ErrorCode::ParseFailure);
        assert_eq!(s.fails("POLAR increment=400"), ErrorCode::InvalidArgument);
        assert_eq!(s.run("DRAFTING"), s.run("ORTHO"));
    }

    #[test]
    fn snap_sets_adds_and_removes_modes_by_name() {
        let mut s = Session::default();
        let reply = s.run("SNAP modes=endpoint,quadrant,node");
        assert_eq!(
            s.document.drafting().snap_modes,
            SnapMode::Endpoint | SnapMode::Quadrant | SnapMode::Node
        );
        assert_eq!(value(&reply, "modes"), "endpoint,quadrant,node");
        s.run("SNAP add=apparentintersection,extension remove=node");
        assert!(has_mode(
            s.document.drafting().snap_modes,
            SnapMode::ApparentIntersection
        ));
        assert!(has_mode(
            s.document.drafting().snap_modes,
            SnapMode::Extension
        ));
        assert!(!has_mode(s.document.drafting().snap_modes, SnapMode::Node));
        s.run("OSNAP off");
        assert!(!s.document.drafting().snap_enabled);
        s.run("SNAP on modes=all");
        assert_eq!(s.document.drafting().snap_modes, ALL_SNAP_MODES);
        assert_eq!(
            s.fails("SNAP modes=endpoint,sideways"),
            ErrorCode::ParseFailure
        );
    }

    #[test]
    fn the_drafting_record_reads_back_as_the_same_settings() {
        // Every mode's name is one word (a blank would end the value for any
        // key=value reader), and SNAP modes= takes the record's own list.
        let mut s = Session::default();
        let all = value(&s.run("SNAP on modes=all"), "modes");
        assert!(!all.contains(' '), "{all}");
        assert!(all.contains("apparentintersection"), "{all}");
        let written = s.document.drafting().snap_modes;
        s.run("SNAP modes=none");
        s.run(&format!("SNAP modes={all}"));
        assert_eq!(
            s.document.drafting().snap_modes,
            written,
            "the record did not read back"
        );
        // The default increment is 15 degrees, held in radians; said as 15.
        let mut fresh = Session::default();
        assert_eq!(value(&fresh.run("DRAFTING"), "increment"), "15");
    }

    #[test]
    fn draw_verb_points_follow_the_angle_convention_too() {
        let mut s = Session::default();
        s.run("ANGLES bearing");
        let reply = s.run("PLINE3D 0,0,1 @10<90");
        let p = polyline(&s, id_of(&reply));
        // A bearing of 90 is east.
        near(p.vertices[1].position.x, 10.0, 1e-9);
        near(p.vertices[1].position.y, 0.0, 1e-9);
    }
}
