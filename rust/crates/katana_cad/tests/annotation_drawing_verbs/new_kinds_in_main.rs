//! tests/cad/drawing/test_new_kinds_in_main.cpp: the drawing system's kinds
//! (docs/drawing.md) in the code main gained while the drawing system was on
//! its own branch: smart leaders' anchors and values, the labels, the snaps
//! that make annotation associative, AREA and PARCEL, the one scope grammar's
//! TYPE=, the utilities' design line, the label keep-out, and the verb lines
//! that replace the drawing. Each of these was written before CurvePolyline2,
//! Ellipse2 and Spline2 existed and took them as "nothing": a note with no
//! value, a label that never appeared, a filter that refused the name.
//!
//! The figure most of them use is a lot with one curved side, worked by hand:
//! (0,0) -> (10,0) straight, (10,0) -> (10,10) a semicircle of radius 5
//! bulging east through (15,5) (bulge 1, counter-clockwise), then (0,10) and
//! back. Area 10 x 10 + half of pi 5^2 = 100 + 12.5 pi; perimeter three sides
//! of 10 and half of 2 pi 5 = 30 + 5 pi.

use std::collections::BTreeSet;

use katana_cad::annotation::drawing::estimated_measure;
use katana_cad::annotation::label_layout::label_keep_out;
use katana_cad::annotation::leader_edit::{leader_place_on, tip_place_on};
use katana_cad::scope_verbs::{format_scope_words, parse_scope_words, parse_where_condition};
use katana_cad::snapping::{SnapResult, snap_anchor};
use katana_cad::survey_tools::compute_area;
use katana_cad::utilities::utility_data::design_from_entity;
use katana_cad::{CommandInterpreter, ModifyFilter, ScopeWords, SnapMode};
use katana_core::ErrorCode;
use katana_entity::anchor::{nearest_anchor, resolve_anchor};
use katana_entity::label_values::label_pieces_for;
use katana_entity::leader_values::anchor_values;
use katana_entity::{
    AnchorPoint, AnchorRef, Entity, EntityId, EntityType, Geometry, LabelKind, LabelStyle,
    LabelValues, LeaderContext,
};
use katana_geometry::{CurvePolyline2, CurveVertex, Ellipse2, Point2, Spline2, Vec2};
use katana_math::PI;

use crate::{Session, near, pt};

const LOT_AREA: f64 = 100.0 + 12.5 * PI;
const LOT_PERIMETER: f64 = 30.0 + 5.0 * PI;

fn lot() -> CurvePolyline2 {
    let vertex = |position: Point2, bulge: f64| CurveVertex {
        position,
        bulge,
        height: None,
    };
    CurvePolyline2 {
        vertices: vec![
            vertex(pt(0.0, 0.0), 0.0),
            vertex(pt(10.0, 0.0), 1.0),
            vertex(pt(10.0, 10.0), 0.0),
            vertex(pt(0.0, 10.0), 0.0),
        ],
        closed: true,
    }
}

// The whole ellipse with semi-axes 4 east and 2 north about `centre`.
fn ellipse(centre: Point2) -> Ellipse2 {
    Ellipse2 {
        center: centre,
        major_axis: Vec2::new(4.0, 0.0),
        ratio: 0.5,
        start_parameter: 0.0,
        sweep: 2.0 * PI,
    }
}

#[track_caller]
fn spline_through(points: Vec<Point2>) -> Spline2 {
    match Spline2::through_points(points, 3) {
        Ok(spline) => spline,
        Err(error) => panic!("{}", error.describe()),
    }
}

// Makes `geometry` as one step and gives its id (the C++ Drawing::add).
#[track_caller]
fn add(d: &mut Session, geometry: Geometry) -> EntityId {
    d.add(Entity {
        geometry,
        ..Entity::default()
    })
}

#[track_caller]
fn number(values: &LabelValues, name: &str) -> f64 {
    match values.get(name) {
        Some(value) => value.number,
        None => panic!("no value '{name}'"),
    }
}

mod new_kinds_in_main {
    use super::*;

    // ---- smart leaders: anchors and the values a note reads ------------------------------------

    #[test]
    fn a_curve_polyline_offers_a_place_along_its_arc_and_inside_it() {
        let mut d = Session::default();
        let id = add(&mut d, Geometry::CurvePolyline(lot()));
        let entity = d.entity(id);
        let Some(nearest) = nearest_anchor(&entity, pt(16.0, 5.0)) else {
            panic!("a curve polyline offered no place to attach to");
        };
        assert_eq!(nearest.point, AnchorPoint::Along);
        assert_eq!(nearest.index, 1, "the arc is segment 1");
        // Due east of the centre is half its sweep.
        near(nearest.parameter, 0.5, 1e-12);
        let Some(at) = resolve_anchor(&entity, &nearest) else {
            panic!("the place does not resolve");
        };
        // ON the arc, not on its chord at x = 10.
        near(at.x, 15.0, 1e-9);
        near(at.y, 5.0, 1e-9);

        let inside_ref = AnchorRef {
            entity: id,
            point: AnchorPoint::Inside,
            ..AnchorRef::default()
        };
        let Some(inside) = resolve_anchor(&entity, &inside_ref) else {
            panic!("the inside does not resolve");
        };
        assert!(inside.x > 0.0);
        assert!(inside.x < 15.0);
        assert!(inside.y > 0.0);
        assert!(inside.y < 10.0);
    }

    #[test]
    fn an_ellipse_offers_a_place_along_it_and_a_spline_refuses_one() {
        let mut d = Session::default();
        // Semi-axes 4 east and 2 north; the point due north is a quarter turn
        // of eccentric anomaly, (0, 2).
        let id = add(&mut d, Geometry::Ellipse(ellipse(pt(0.0, 0.0))));
        let entity = d.entity(id);
        let Some(nearest) = nearest_anchor(&entity, pt(0.0, 3.0)) else {
            panic!("an ellipse offered no place to attach to");
        };
        near(nearest.parameter, 0.25, 1e-9);
        let Some(at) = resolve_anchor(&entity, &nearest) else {
            panic!("the place does not resolve");
        };
        near(at.x, 0.0, 1e-9);
        near(at.y, 2.0, 1e-9);

        let spline = add(
            &mut d,
            Geometry::Spline(spline_through(vec![
                pt(0.0, 0.0),
                pt(5.0, 3.0),
                pt(10.0, 0.0),
            ])),
        );
        assert!(
            nearest_anchor(&d.entity(spline), pt(5.0, 3.0)).is_none(),
            "a spline's parameter is not its length: no place along it"
        );
        let refusal = d
            .attempt(&format!("LEADER #{spline}@5,3 20,20 text=note"))
            .expect_err("a leader on a place along a spline");
        assert!(
            refusal.message.contains("spline"),
            "the refusal says why: {}",
            refusal.describe()
        );
    }

    #[test]
    fn a_leader_on_a_curve_polyline_reads_its_arc_length_and_area() {
        let mut d = Session::default();
        let id = add(&mut d, Geometry::CurvePolyline(lot()));
        let place = AnchorRef {
            entity: id,
            point: AnchorPoint::Along,
            index: 1,
            parameter: 0.5,
        };
        let values = anchor_values(
            &d.entity(id),
            &place,
            pt(15.0, 5.0),
            &[],
            &LeaderContext::default(),
        );
        // The tip is on the arc: an arc's values.
        near(number(&values, "radius"), 5.0, 1e-9);
        near(number(&values, "area"), LOT_AREA, 1e-9);
        near(number(&values, "perimeter"), LOT_PERIMETER, 1e-9);
        // The whole line's length.
        near(number(&values, "length"), LOT_PERIMETER, 1e-9);
        assert_eq!(number(&values, "vertices"), 4.0);
        // Ten metres of the first side, then half the arc's 5 pi.
        near(number(&values, "chainage"), 10.0 + 2.5 * PI, 1e-9);
    }

    #[test]
    fn leaders_for_and_attach_put_the_tip_inside_a_closed_curve_polyline() {
        let mut d = Session::default();
        let id = add(&mut d, Geometry::CurvePolyline(lot()));
        let Some(placed) = leader_place_on(&d.entity(id), 0.0) else {
            panic!("LEADER FOR had nowhere to put the tip");
        };
        assert_eq!(placed.r#ref.point, AnchorPoint::Inside);
        let Some(attached) = tip_place_on(&d.entity(id), pt(12.0, 5.0)) else {
            panic!("Attach found no place on the lot");
        };
        assert_eq!(
            attached.r#ref.point,
            AnchorPoint::Inside,
            "(12,5) is inside the curved side"
        );

        // A whole ellipse, semi-axes 4 east and 2: (3,1) is inside (9/16 + 1/4
        // < 1), (3,1.5) outside (9/16 + 9/16 > 1).
        let whole = add(&mut d, Geometry::Ellipse(ellipse(pt(0.0, 0.0))));
        let Some(inside) = tip_place_on(&d.entity(whole), pt(3.0, 1.0)) else {
            panic!("Attach found no place on the ellipse");
        };
        assert_eq!(inside.r#ref.point, AnchorPoint::Inside);
        let Some(outside) = tip_place_on(&d.entity(whole), pt(3.0, 1.5)) else {
            panic!("Attach found no place on the ellipse");
        };
        assert_eq!(outside.r#ref.point, AnchorPoint::Along);

        let spline = add(
            &mut d,
            Geometry::Spline(spline_through(vec![
                pt(0.0, 0.0),
                pt(5.0, 3.0),
                pt(10.0, 0.0),
            ])),
        );
        let Some(start) = tip_place_on(&d.entity(spline), pt(5.0, 3.0)) else {
            panic!("Attach refused a spline");
        };
        assert_eq!(start.r#ref.point, AnchorPoint::Start);
    }

    // ---- labels -----------------------------------------------------------------------------------

    #[test]
    fn a_curve_polyline_is_labelled_by_area_segment_and_arc_styles() {
        let mut d = Session::default();
        let id = add(&mut d, Geometry::CurvePolyline(lot()));
        let entity = d.entity(id);
        let model = d.document.model();

        let area = LabelStyle {
            name: String::from("Lot"),
            kind: LabelKind::Area,
            text: String::from("{area}"),
            ..LabelStyle::default()
        };
        let areas = label_pieces_for(model, &entity, -1, &area, &[]);
        assert_eq!(areas.len(), 1, "a lot with a curved side got no area label");
        // Exact, the arc's segment of area included.
        near(number(&areas[0].values, "area"), LOT_AREA, 1e-9);

        let segment = LabelStyle {
            name: String::from("Side"),
            kind: LabelKind::Segment,
            text: String::from("{length}"),
            ..LabelStyle::default()
        };
        assert_eq!(
            label_pieces_for(model, &entity, -1, &segment, &[]).len(),
            4,
            "three straight sides and the arc"
        );
        let arc = LabelStyle {
            name: String::from("Curve"),
            kind: LabelKind::Arc,
            text: String::from("{radius}"),
            ..LabelStyle::default()
        };
        let arcs = label_pieces_for(model, &entity, -1, &arc, &[]);
        assert_eq!(arcs.len(), 1);
        near(number(&arcs[0].values, "radius"), 5.0, 1e-9);
        near(number(&arcs[0].values, "length"), 5.0 * PI, 1e-9);
    }

    #[test]
    fn labels_keep_off_the_new_kinds() {
        let mut d = Session::default();
        add(&mut d, Geometry::CurvePolyline(lot()));
        // Three straight sides, and the semicircle chorded as an arc is:
        // sixteen chords a turn, so eight.
        assert_eq!(
            label_keep_out(
                d.document.model(),
                1000.0,
                &estimated_measure(),
                1000,
                &LeaderContext::default()
            )
            .len(),
            11
        );
        let mut e = Session::default();
        add(&mut e, Geometry::Ellipse(ellipse(pt(0.0, 0.0))));
        assert!(
            !label_keep_out(
                e.document.model(),
                1000.0,
                &estimated_measure(),
                1000,
                &LeaderContext::default()
            )
            .is_empty(),
            "a label could stand on an ellipse"
        );
    }

    // ---- snaps that make a dimension or leader follow ---------------------------------------------

    #[test]
    fn a_snap_to_a_curve_polylines_arc_middle_names_its_segment() {
        let mut d = Session::default();
        let id = add(&mut d, Geometry::CurvePolyline(lot()));
        let middle = SnapResult {
            point: pt(15.0, 5.0),
            mode: SnapMode::Midpoint,
            entity: id,
        };
        let Some(anchor) = snap_anchor(d.document.model(), &middle) else {
            panic!("the snap named nothing: the note would not follow");
        };
        assert_eq!(anchor.point, AnchorPoint::SegmentMid);
        assert_eq!(anchor.index, 1);

        let whole = add(&mut d, Geometry::Ellipse(ellipse(pt(50.0, 50.0))));
        let centre_snap = SnapResult {
            point: pt(50.0, 50.0),
            mode: SnapMode::Center,
            entity: whole,
        };
        let Some(centre) = snap_anchor(d.document.model(), &centre_snap) else {
            panic!("the snap named nothing");
        };
        assert_eq!(centre.point, AnchorPoint::Centre);
    }

    // ---- AREA and PARCEL ----------------------------------------------------------------------------

    #[test]
    fn area_measures_a_closed_curve_polyline_and_an_ellipse_exactly() {
        let mut d = Session::default();
        let curved = add(&mut d, Geometry::CurvePolyline(lot()));
        // pi a b, with a = 4 and b = 2.
        let whole = add(&mut d, Geometry::Ellipse(ellipse(pt(50.0, 50.0))));
        let result = match compute_area(&d.document, &[curved, whole]) {
            Ok(result) => result,
            Err(error) => panic!("{}", error.describe()),
        };
        assert_eq!(result.items.len(), 2, "skipped as having no area");
        near(result.items[0].area, LOT_AREA, 1e-9);
        near(result.items[0].perimeter, LOT_PERIMETER, 1e-9);
        near(result.items[1].area, 8.0 * PI, 1e-9);
    }

    #[test]
    fn parcel_refuses_arc_courses_saying_so_rather_than_chording_them() {
        let mut d = Session::default();
        let id = add(&mut d, Geometry::CurvePolyline(lot()));
        let refusal = d
            .attempt(&format!("PARCEL {id}"))
            .expect_err("a parcel with an arc course");
        assert_eq!(refusal.code, ErrorCode::Unsupported);
        assert!(refusal.message.contains("arc"), "{}", refusal.describe());
    }

    // ---- the one scope grammar ------------------------------------------------------------------

    #[test]
    fn type_filters_name_the_new_kinds_in_any_case() {
        for word in [
            "TYPE=curvepolyline",
            "TYPE=CurvePolyline",
            "TYPE=CURVEPOLYLINE",
        ] {
            let mut filter = ModifyFilter::default();
            assert!(parse_where_condition(word, &mut filter).is_ok(), "{word}");
            assert_eq!(
                filter.types,
                BTreeSet::from([EntityType::CurvePolyline]),
                "{word}"
            );
        }
        let mut filter = ModifyFilter::default();
        assert!(parse_where_condition("TYPE=ellipse,spline", &mut filter).is_ok());
        assert_eq!(
            filter.types,
            BTreeSet::from([EntityType::Ellipse, EntityType::Spline])
        );

        // And what the words are written as reads back as the same filter.
        let words = ScopeWords {
            filter: ModifyFilter {
                types: BTreeSet::from([EntityType::CurvePolyline]),
                ..ModifyFilter::default()
            },
            ..ScopeWords::default()
        };
        let written = match format_scope_words(&words) {
            Ok(written) => written,
            Err(error) => panic!("{}", error.describe()),
        };
        let tokens = match CommandInterpreter::tokenize(&written) {
            Ok(tokens) => tokens,
            Err(error) => panic!("{written}: {}", error.describe()),
        };
        let mut at = 0;
        let read = match parse_scope_words(&tokens, &mut at) {
            Ok(read) => read,
            Err(error) => panic!("{written}: {}", error.describe()),
        };
        assert_eq!(read.filter.types, words.filter.types, "{written}");

        let mut d = Session::default();
        add(&mut d, Geometry::CurvePolyline(lot()));
        add(&mut d, Geometry::Ellipse(ellipse(pt(50.0, 50.0))));
        assert!(d.attempt("SELECT TYPE CURVEPOLYLINE").is_ok());
        assert_eq!(d.document.selection().ids().len(), 1);
    }

    // ---- the utilities' design line -----------------------------------------------------------------

    #[test]
    fn a_utility_design_line_may_have_arcs() {
        let mut d = Session::default();
        // A quarter circle of radius 10 from (0,0) to (10,10), centre (0,10),
        // at 50 then 51: bulge tan(90/4).
        let id = add(
            &mut d,
            Geometry::CurvePolyline(CurvePolyline2 {
                vertices: vec![
                    CurveVertex {
                        position: pt(0.0, 0.0),
                        bulge: (PI / 8.0).tan(),
                        height: Some(50.0),
                    },
                    CurveVertex {
                        position: pt(10.0, 10.0),
                        bulge: 0.0,
                        height: Some(51.0),
                    },
                ],
                closed: false,
            }),
        );
        let design = match design_from_entity(&d.entity(id), None) {
            Ok(design) => design,
            Err(error) => panic!("{}", error.describe()),
        };
        assert!(design.vertices.len() > 2, "the arc was taken as its chord");
        for vertex in &design.vertices {
            // Every chord's end is ON the arc: 10 from the centre (0, 10). The
            // design vertices are northing, easting.
            let east = vertex.position.easting;
            let north = vertex.position.northing;
            near(east.hypot(north - 10.0), 10.0, 1e-9);
        }
        assert_eq!(design.vertices[0].level, Some(50.0));
        assert_eq!(
            design.vertices.last().and_then(|vertex| vertex.level),
            Some(51.0)
        );
    }

    // ---- the lines that replace the drawing -----------------------------------------------------------

    #[test]
    fn only_an_open_of_a_project_replaces_the_drawing() {
        assert!(CommandInterpreter::replaces_document("NEW"));
        assert!(CommandInterpreter::replaces_document("OPEN /projects/site"));
        assert!(CommandInterpreter::replaces_document(
            "open \"C:/my projects/site\""
        ));
        // OPEN of polylines (docs/drawing.md) is an edit: the window must not
        // ask to discard the drawing for it, nor katana_mcp refuse it for
        // unsaved changes.
        assert!(!CommandInterpreter::replaces_document("OPEN #12"));
        assert!(!CommandInterpreter::replaces_document("OPEN #12 #13"));
        assert!(!CommandInterpreter::replaces_document("open selection"));
        assert!(!CommandInterpreter::replaces_document("OPEN"));
        assert!(!CommandInterpreter::replaces_document(
            "SAVE /projects/site"
        ));
        assert!(!CommandInterpreter::replaces_document(""));
    }
}
