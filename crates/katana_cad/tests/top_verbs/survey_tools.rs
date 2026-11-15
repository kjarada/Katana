// The survey tools (survey_tools.rs) and the command-line verbs built on them
// (INVERSE, FORWARD, AREA) (tests/cad/test_survey_tools.cpp).
//
// Every expected value is worked out here from the geometry, not taken from a
// run:
//   * the inverse of a 3-4-5 triangle is 5 at azimuth atan2(3, 4) =
//     36.869897645844021 degrees; 0.869897645844021 * 60 = 52.19385875064126',
//     0.19385875064126 * 60 = 11.6315250384756" -> 36°52'11.63";
//   * a 100 x 50 rectangle is 5000 m^2 = 0.5 ha (1 ha = 10 000 m^2, SI
//     Brochure 9th ed., table 8) with a perimeter of 300;
//   * the traverses are built from stations with KNOWN coordinates, so the
//     observed angles and distances are the exact geometry of the figure and
//     the closure is exact; the perturbed one lengthens one leg by 0.100 and
//     its Bowditch and transit corrections are worked out below by hand from
//     the rules' definitions (Ghilani & Wolf, "Elementary Surveying", the
//     compass and transit rules: correction = -misclosure * L / sum L, and
//     -misclosure * |lat| / sum |lat|). No textbook example is reproduced
//     because none could be checked against its source here; a constructed
//     figure whose answer is known exactly is the stronger test anyway;
//   * the level book is reduced by hand in the height-of-collimation layout.
//
// Coordinates are chosen so that easting and northing differ in magnitude
// (E ~ 5 000, N ~ 2 000 or E ~ 1 000, N ~ 5 000): the one mistake this module
// could make that still draws a plausible picture is transposing them.

#![allow(
    clippy::excessive_precision,
    reason = "Expected values are copied verbatim from the C++ oracle."
)]

use katana_cad::survey_tools::{
    compute_area, compute_forward, compute_inverse, compute_level_book, compute_traverse_tool,
    conversion_lines_for_selection, convert_angle, convert_coordinates, ends_of_line,
    format_angle_conversion, format_area_report, format_coordinate_conversion,
    format_inverse_report, format_level_book_report, format_traverse_report, forward_point_command,
    is_metre_unit, parse_level_book, parse_survey_angle, parse_survey_direction,
    parse_survey_number, parse_survey_position, parse_traverse_legs, position_of_point,
    selected_point_positions, traverse_command,
};
use katana_cad::{
    AngleInputUnit, CommandInterpreter, Document, ForwardInput, LevelBookSpec, SurveyPosition,
    TraverseMethod, TraverseOrientation, TraverseSpec,
};
use katana_commands::{
    EntityAttributes, create_circle, create_entities, create_line, create_polyline,
};
use katana_core::ErrorCode;
use katana_entity::entity::{ELEVATION_PROPERTY, heights_of, property_to_string, set_heights};
use katana_entity::{Entity, EntityId, Geometry, PointGeometry, PropertyValue};
use katana_geometry::{Point2, Polyline2, Segment2};
use katana_math::numerics::PI;
use katana_survey::angles::normalize_azimuth;
use katana_survey::{LevelAdjustment, TraverseKind, TraverseSetup};

use crate::{Session, create, done, err, must, near, ok};

fn at(easting: f64, northing: f64, elevation: Option<f64>) -> SurveyPosition {
    SurveyPosition {
        point: Point2::new(easting, northing),
        elevation,
        ..SurveyPosition::default()
    }
}

// A point entity with a name and, when given, a height - as the survey import
// writes them.
fn add_point(
    document: &mut Document,
    position: Point2,
    name: &str,
    elevation: Option<f64>,
) -> EntityId {
    let mut entity = Entity {
        geometry: Geometry::Point(PointGeometry { position }),
        layer: String::from(document.current_layer()),
        ..Entity::default()
    };
    if !name.is_empty() {
        entity
            .properties
            .insert(String::from("point"), PropertyValue::from(name));
    }
    set_heights(&mut entity.properties, &[elevation]);
    create(document, create_entities(vec![entity]))
}

fn polyline_of(corners: &[(f64, f64)], closed: bool) -> Polyline2 {
    Polyline2::new(
        corners.iter().map(|&(x, y)| Point2::new(x, y)).collect(),
        closed,
    )
}

fn azimuth_of(d_e: f64, d_n: f64) -> f64 {
    normalize_azimuth(d_e.atan2(d_n))
}

// Where a point entity stands, failing the test for any other entity.
fn position_of(entity: &Entity) -> Point2 {
    match &entity.geometry {
        Geometry::Point(point) => point.position,
        _ => panic!("entity {} is not a point", entity.id),
    }
}

// The entity `id`, failing the test when there is none.
fn entity_of(document: &Document, id: EntityId) -> Entity {
    match document.model().entities.find(id) {
        Some(entity) => entity.clone(),
        None => panic!("no entity with id {id}"),
    }
}

// The first entity the last command made.
fn last_created(document: &Document) -> EntityId {
    match document.last_created_entities().first() {
        Some(&id) => id,
        None => panic!("nothing was created"),
    }
}

// ---- the constructed traverse -----------------------------------------------------------
//
// A(5000, 2000) -> B(5120, 2160) -> C(5360, 2090) -> D(5280, 1898) -> A, as
// (easting, northing). Every leg is a Pythagorean triple, so the distances are
// exact integers:
//   AB (+120, +160) = 200     (3-4-5 x 40)
//   BC (+240,  -70) = 250     (24-7-25 x 10)
//   CD ( -80, -192) = 208     (5-12-13 x 16)
//   DA (-280, +102) = 298     sqrt(78400 + 10404) = sqrt(88804) = 298
// and the four sum to (0, 0). The circuit runs clockwise on the map.
struct Station {
    id: &'static str,
    e: f64,
    n: f64,
}

const A: Station = Station {
    id: "A",
    e: 5000.0,
    n: 2000.0,
};
const B: Station = Station {
    id: "B",
    e: 5120.0,
    n: 2160.0,
};
const C: Station = Station {
    id: "C",
    e: 5360.0,
    n: 2090.0,
};
const D: Station = Station {
    id: "D",
    e: 5280.0,
    n: 1898.0,
};

fn azimuth(from: &Station, to: &Station) -> f64 {
    azimuth_of(to.e - from.e, to.n - from.n)
}

// The clockwise angle turned at `at` from the back station to the forward one,
// by the definition katana_survey's Traverse uses: az(at->forward) - az(at->back).
fn turned(back: &Station, at: &Station, forward: &Station) -> f64 {
    normalize_azimuth(azimuth(at, forward) - azimuth(at, back))
}

fn leg(station: &str, angle: f64, distance: f64) -> TraverseSetup {
    TraverseSetup {
        station_id: String::from(station),
        angle,
        distance,
    }
}

fn sighting(mark: Point2) -> TraverseOrientation {
    TraverseOrientation {
        reference_mark: Some(mark),
        ..TraverseOrientation::default()
    }
}

fn loop_spec() -> TraverseSpec {
    TraverseSpec {
        name: String::from("Loop"),
        kind: TraverseKind::ClosedLoop,
        start: Point2::new(A.e, A.n),
        // The first leg's direction, given by sighting B's known position.
        start_orientation: sighting(Point2::new(B.e, B.n)),
        legs: vec![
            leg("A", turned(&D, &A, &B), 200.0),
            leg("B", turned(&A, &B, &C), 250.0),
            leg("C", turned(&B, &C, &D), 208.0),
            leg("D", turned(&C, &D, &A), 298.0),
        ],
        ..TraverseSpec::default()
    }
}

// A -> B -> C -> D between control A and D. Oriented at A on a mark due north
// (azimuth 0) and closed at D on a mark due south (azimuth 180 degrees), with
// the angles the figure turns:
//   at A, from north to B:            az(AB) - 0
//   at B and C: back to forward, as in the loop
//   at D, from C to the south mark:   180 degrees - az(DC)
fn link_spec() -> TraverseSpec {
    let north = 0.0;
    TraverseSpec {
        name: String::from("Link"),
        kind: TraverseKind::Link,
        start: Point2::new(A.e, A.n),
        start_orientation: sighting(Point2::new(A.e, A.n + 500.0)),
        legs: vec![
            leg("A", normalize_azimuth(azimuth(&A, &B) - north), 200.0),
            leg("B", turned(&A, &B, &C), 250.0),
            leg("C", turned(&B, &C, &D), 208.0),
        ],
        end_station: String::from("D"),
        end: Point2::new(D.e, D.n),
        closing_angle: Some(normalize_azimuth(PI - azimuth(&D, &C))),
        closing_orientation: sighting(Point2::new(D.e, D.n - 100.0)),
        ..TraverseSpec::default()
    }
}

// ---- level book -------------------------------------------------------------------------

// Reduced by hand, height of collimation:
//   BM1   BS 1.500               HPC 100.000 + 1.500 = 101.500   RL 100.000
//   A     IS 2.000                                     101.500 - 2.000 =  99.500
//   CP1   FS 0.500  BS 2.250     RL 101.500 - 0.500 = 101.000, HPC 103.250
//   B     IS 1.250                                     103.250 - 1.250 = 102.000
//   BM2   FS 3.260                                     103.250 - 3.260 =  99.990
// Check: sum BS 3.750 - sum FS 3.760 = -0.010 = 99.990 - 100.000.
// BM2 is known at 100.000, so the misclosure is 99.990 - 100.000 = -0.010.
const BOOK: &str = "# point  BS     IS     FS\n\
                    BM1  1.500  -      -\n\
                    A    -      2.000  -\n\
                    CP1  2.250  -      0.500  100\n\
                    B    -      1.250  -\n\
                    BM2  -      -      3.260  150\n";

fn book_spec(adjustment: LevelAdjustment) -> LevelBookSpec {
    LevelBookSpec {
        name: String::from("Hand book"),
        start_level: 100.0,
        closing_level: Some(100.0),
        lines: ok(parse_level_book(BOOK)),
        adjustment,
        ..LevelBookSpec::default()
    }
}

// The first `count` ids, as the reports list them: "7, 8, 9".
fn listed(ids: &[EntityId], count: usize) -> String {
    let words: Vec<String> = ids[..count].iter().map(|id| format!("{id}")).collect();
    words.join(", ")
}

mod survey_tools {
    use super::*;

    // ---- angle text -------------------------------------------------------------------------

    #[test]
    fn directions_are_read_as_azimuths_or_quadrant_bearings() {
        // S 45 W is 180 + 45 = 225 degrees; N 90 E is due east.
        let south_west = ok(parse_survey_direction("S 45d00m00s W"));
        near(south_west, 1.25 * PI, 1e-15);
        let east = ok(parse_survey_direction("n90e"));
        near(east, 0.5 * PI, 1e-15);
        // 36:52:11.63 is 36 + 52/60 + 11.63/3600 degrees.
        let dms = ok(parse_survey_direction("36:52:11.63"));
        near(
            dms,
            (36.0 + 52.0 / 60.0 + 11.63 / 3600.0) * PI / 180.0,
            1e-15,
        );
        // A negative azimuth is the same direction one turn on.
        let negative = ok(parse_survey_direction("-90"));
        near(negative, 1.5 * PI, 1e-15);
    }

    #[test]
    fn an_unreadable_angle_says_which_formats_are_accepted() {
        let angle = err(parse_survey_angle("36x52"));
        assert_eq!(angle.code, ErrorCode::ParseFailure);
        assert!(angle.message.contains("36d52m11.63s"), "{}", angle.message);
        assert!(parse_survey_direction("Q 45 E").is_err());
        assert!(
            parse_survey_number("1,5", "distance").is_err(),
            "a decimal comma is not a point"
        );
    }

    // ---- positions --------------------------------------------------------------------------

    #[test]
    fn typed_positions_are_easting_first_with_an_optional_height() {
        let document = Document::default();
        let flat = ok(parse_survey_position(&document, "1000.5,5000.25"));
        assert_eq!(flat.point, Point2::new(1000.5, 5000.25));
        assert!(
            flat.elevation.is_none(),
            "no Z typed is no height, not a height of 0"
        );

        let with_height = ok(parse_survey_position(&document, " 1,2,-3.5 "));
        assert_eq!(with_height.elevation, Some(-3.5));

        assert_eq!(
            err(parse_survey_position(&document, "1,2,3,4")).code,
            ErrorCode::ParseFailure
        );
        assert_eq!(
            err(parse_survey_position(&document, "1,north")).code,
            ErrorCode::ParseFailure
        );
        assert_eq!(
            err(parse_survey_position(&document, "42")).code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn point_entities_give_their_position_name_and_height() {
        let mut document = Document::default();
        let id = add_point(
            &mut document,
            Point2::new(1000.0, 5000.0),
            "CP-01",
            Some(31.25),
        );
        let position = ok(parse_survey_position(&document, &format!("{id}")));
        assert_eq!(position.point, Point2::new(1000.0, 5000.0));
        assert_eq!(position.elevation, Some(31.25));
        assert_eq!(position.name, "CP-01");
        assert_eq!(position.label(), format!("point {id} (CP-01)"));

        let line = create(
            &mut document,
            create_line(
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 1.0),
                EntityAttributes::default(),
            ),
        );
        let not_a_point = err(position_of_point(&document, line));
        assert_eq!(not_a_point.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn only_the_selected_points_are_survey_positions() {
        let mut document = Document::default();
        let p1 = add_point(&mut document, Point2::new(1.0, 2.0), "P1", None);
        let line = create(
            &mut document,
            create_line(
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 1.0),
                EntityAttributes::default(),
            ),
        );
        let p2 = add_point(&mut document, Point2::new(3.0, 4.0), "P2", None);
        document.selection_mut().set(vec![p2, line, p1]);
        let positions = selected_point_positions(&document);
        assert_eq!(positions.len(), 2, "the line is not a point");
        assert_eq!(positions[0].name, "P1", "in id order");
        assert_eq!(positions[1].name, "P2");
    }

    // ---- inverse ----------------------------------------------------------------------------

    #[test]
    fn the_inverse_of_a_three_four_five_triangle_is_five_at_azimuth36d52m11s63() {
        let result = ok(compute_inverse(
            &at(1000.0, 5000.0, None),
            &at(1003.0, 5004.0, None),
        ));
        near(result.horizontal_distance, 5.0, 1e-12);
        near(result.delta_easting, 3.0, 1e-12);
        near(result.delta_northing, 4.0, 1e-12);
        // atan2(3, 4) in degrees is 36.869897645844021 (see the file comment).
        near(result.azimuth * 180.0 / PI, 36.869897645844021, 1e-12);
        assert!(result.height_difference.is_none());

        let report = format_inverse_report(&result);
        assert!(report.contains("Horizontal distance 5.000"), "{report}");
        assert!(report.contains("Azimuth 36\u{b0}52'11.63\""), "{report}");
        assert!(
            report.contains("Bearing N 36\u{b0}52'11.63\" E"),
            "{report}"
        );
        // Back azimuth: 36°52'11.63" + 180°.
        assert!(
            report.contains("back azimuth 216\u{b0}52'11.63\""),
            "{report}"
        );
        assert!(
            report.contains("No height difference: neither end has an elevation"),
            "{report}"
        );
    }

    #[test]
    fn due_north_is_azimuth_zero_so_easting_and_northing_are_not_transposed() {
        // 100 north of the start. Transposed, it would be 100 EAST - azimuth 90.
        let result = ok(compute_inverse(
            &at(1000.0, 5000.0, None),
            &at(1000.0, 5100.0, None),
        ));
        near(result.azimuth, 0.0, 1e-15);
        near(result.delta_northing, 100.0, 1e-12);
        assert_eq!(result.delta_easting, 0.0);
    }

    #[test]
    fn the_inverse_to_the_south_west_is_azimuth225_and_bearing_south45_west() {
        let result = ok(compute_inverse(&at(0.0, 0.0, None), &at(-1.0, -1.0, None)));
        near(result.azimuth, 1.25 * PI, 1e-15);
        near(result.horizontal_distance, 2.0_f64.sqrt(), 1e-15);
        let report = format_inverse_report(&result);
        assert!(report.contains("Azimuth 225\u{b0}00'00.00\""), "{report}");
        assert!(
            report.contains("Bearing S 45\u{b0}00'00.00\" W"),
            "{report}"
        );
    }

    #[test]
    fn heights_at_both_ends_give_height_difference_slope_distance_and_grade() {
        // dz = 12 - 10 = 2; slope = sqrt(5^2 + 2^2) = sqrt(29); grade = 2/5 = 40 %.
        let result = ok(compute_inverse(
            &at(1000.0, 5000.0, Some(10.0)),
            &at(1003.0, 5004.0, Some(12.0)),
        ));
        let (Some(height), Some(slope), Some(grade)) = (
            result.height_difference,
            result.slope_distance,
            result.grade,
        ) else {
            panic!("both ends have a height");
        };
        near(height, 2.0, 1e-12);
        near(slope, 29.0_f64.sqrt(), 1e-12);
        near(grade, 0.4, 1e-12);
        let report = format_inverse_report(&result);
        assert!(report.contains("Height difference +2.000"), "{report}");
        assert!(report.contains("slope distance 5.385"), "{report}");
        assert!(report.contains("grade +40.000 %"), "{report}");
    }

    #[test]
    fn an_end_without_a_height_gives_no_height_difference_rather_than_one_from_zero() {
        let result = ok(compute_inverse(
            &at(1000.0, 5000.0, Some(10.0)),
            &at(1003.0, 5004.0, None),
        ));
        assert!(result.height_difference.is_none());
        assert!(result.slope_distance.is_none());
        assert!(result.grade.is_none());
        let report = format_inverse_report(&result);
        // The END without a height is named, not the one with it.
        assert!(
            report.contains("No height difference: E 1003.000 N 5004.000 has no elevation"),
            "{report}"
        );
    }

    #[test]
    fn coincident_positions_are_refused_rather_than_given_an_azimuth() {
        let refusal = err(compute_inverse(
            &at(1000.0, 5000.0, None),
            &at(1000.0, 5000.0, None),
        ));
        assert_eq!(refusal.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn the_inverse_of_a_line_runs_from_its_start_to_its_end_with_its_heights() {
        let mut document = Document::default();
        let mut line = Entity {
            geometry: Geometry::Line(Segment2::new(
                Point2::new(1000.0, 5000.0),
                Point2::new(1003.0, 5004.0),
            )),
            ..Entity::default()
        };
        set_heights(&mut line.properties, &[Some(10.0), Some(12.0)]);
        let id = create(&mut document, create_entities(vec![line]));

        let (start, end) = ok(ends_of_line(&document, id));
        let result = ok(compute_inverse(&start, &end));
        near(result.horizontal_distance, 5.0, 1e-12);
        match result.height_difference {
            Some(height) => near(height, 2.0, 1e-12),
            None => panic!("both ends of the line have a height"),
        }
        assert!(format_inverse_report(&result).contains(&format!("line {id} start")));

        let circle = create(
            &mut document,
            create_circle(Point2::new(0.0, 0.0), 1.0, EntityAttributes::default()),
        );
        assert_eq!(
            err(ends_of_line(&document, circle)).code,
            ErrorCode::InvalidArgument
        );
    }

    // ---- forward ----------------------------------------------------------------------------

    #[test]
    fn forward_along_the_three_four_five_direction_reaches_its_far_corner() {
        let mut input = ForwardInput {
            from: at(1000.0, 5000.0, Some(20.0)),
            azimuth: 3.0_f64.atan2(4.0),
            distance: 5.0,
            height_difference: Some(-1.5),
            ..ForwardInput::default()
        };
        let result = ok(compute_forward(&input));
        near(result.point.x, 1003.0, 1e-9);
        near(result.point.y, 5004.0, 1e-9);
        match result.elevation {
            Some(elevation) => near(elevation, 18.5, 1e-12),
            None => panic!("the start has a height"),
        }

        // Due east by bearing: N 90 E, 10 along.
        input.azimuth = ok(parse_survey_direction("N 90 E"));
        input.distance = 10.0;
        let east = ok(compute_forward(&input));
        near(east.point.x, 1010.0, 1e-9);
        near(east.point.y, 5000.0, 1e-9);
    }

    #[test]
    fn a_height_difference_from_a_start_with_no_height_is_refused() {
        let mut input = ForwardInput {
            from: at(1000.0, 5000.0, None),
            azimuth: 0.0,
            distance: 10.0,
            height_difference: Some(2.0),
            ..ForwardInput::default()
        };
        // The new point would get a height of 2 above a datum nobody observed.
        let refusal = err(compute_forward(&input));
        assert_eq!(refusal.code, ErrorCode::InvalidArgument);

        input.height_difference = None;
        let flat = ok(compute_forward(&input));
        assert!(flat.elevation.is_none());

        input.distance = 0.0;
        assert!(
            compute_forward(&input).is_err(),
            "a zero distance is a point on top of the start"
        );
    }

    #[test]
    fn a_forward_point_is_one_undoable_command_on_the_current_layer() {
        let mut session = Session::default();
        session.ok("LAYER NEW stakeout");
        session.ok("LAYER SET stakeout");

        let input = ForwardInput {
            from: at(1000.0, 5000.0, Some(20.0)),
            azimuth: 0.0,
            distance: 25.0,
            height_difference: Some(1.0),
            name: String::from("P7"),
        };
        let result = ok(compute_forward(&input));
        let command = ok(forward_point_command(&session.document, &result));
        must(&mut session.document, command);

        assert_eq!(session.document.model().entities.size(), 1);
        let point = entity_of(&session.document, last_created(&session.document));
        assert_eq!(point.layer, "stakeout");
        assert_eq!(position_of(&point), Point2::new(1000.0, 5025.0));
        assert_eq!(property_to_string(&point.properties["point"]), "P7");
        assert_eq!(heights_of(&point.properties, 1)[0], Some(21.0));

        done(session.document.undo());
        assert!(
            session.document.model().entities.empty(),
            "one undo removes the whole operation"
        );
    }

    #[test]
    fn a_forward_point_without_a_height_carries_no_elevation_property() {
        let mut document = Document::default();
        let input = ForwardInput {
            from: at(1000.0, 5000.0, None),
            azimuth: PI,
            distance: 10.0,
            ..ForwardInput::default()
        };
        let result = ok(compute_forward(&input));
        let command = ok(forward_point_command(&document, &result));
        must(&mut document, command);
        let point = entity_of(&document, last_created(&document));
        assert!(!point.properties.contains_key(ELEVATION_PROPERTY));
        assert_eq!(position_of(&point), Point2::new(1000.0, 4990.0));
    }

    // ---- area -------------------------------------------------------------------------------

    #[test]
    fn a_rectangle_is_five_thousand_square_metres_or_half_a_hectare() {
        let mut document = Document::default();
        // UTM-sized: the shoelace sum of these raw coordinates is ~1e13 and its
        // rounding would show in the third decimal; katana_survey's polygon_area
        // works relative to the first vertex and must not.
        let rectangle = create(
            &mut document,
            create_polyline(
                polyline_of(
                    &[
                        (500000.0, 7000000.0),
                        (500100.0, 7000000.0),
                        (500100.0, 7000050.0),
                        (500000.0, 7000050.0),
                    ],
                    true,
                ),
                EntityAttributes::default(),
            ),
        );
        let result = ok(compute_area(&document, &[rectangle]));
        assert_eq!(result.items.len(), 1);
        near(result.items[0].area, 5000.0, 1e-9);
        near(result.items[0].perimeter, 300.0, 1e-9);
        assert!(result.metres, "a new project's unit is the metre");

        let report = format_area_report(&result);
        assert!(
            report.contains("area 5000.000 m\u{b2} (0.5000 ha)"),
            "{report}"
        );
        assert!(report.contains("perimeter 300.000 m"), "{report}");
    }

    #[test]
    fn closed_outlines_and_circles_are_measured_each_and_in_total() {
        let mut document = Document::default();
        let rectangle = create(
            &mut document,
            create_polyline(
                polyline_of(
                    &[(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)],
                    true,
                ),
                EntityAttributes::default(),
            ),
        );
        let circle = create(
            &mut document,
            create_circle(Point2::new(500.0, 500.0), 10.0, EntityAttributes::default()),
        );
        let open = create(
            &mut document,
            create_polyline(
                polyline_of(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)], false),
                EntityAttributes::default(),
            ),
        );
        let line = create(
            &mut document,
            create_line(
                Point2::new(0.0, 0.0),
                Point2::new(5.0, 5.0),
                EntityAttributes::default(),
            ),
        );

        let result = ok(compute_area(&document, &[rectangle, circle, open, line]));
        assert_eq!(result.items.len(), 2);
        // A circle of radius 10: 100 pi of area, 20 pi round.
        near(result.items[1].area, 100.0 * PI, 1e-9);
        near(result.items[1].perimeter, 20.0 * PI, 1e-9);
        near(result.total_area, 5000.0 + 100.0 * PI, 1e-9);
        near(result.total_perimeter, 300.0 + 20.0 * PI, 1e-9);
        assert_eq!(result.skipped.len(), 2, "the open polyline and the line");
        assert_eq!(result.skipped[0].id, open);
        assert!(result.skipped[0].reason.contains("open polyline"));

        let report = format_area_report(&result);
        // 5000 + 314.159265... = 5314.159 m^2 = 0.5314 ha.
        assert!(
            report.contains("Total  area 5314.159 m\u{b2} (0.5314 ha)"),
            "{report}"
        );
        assert!(
            report.contains(&format!("Skipped 1: a line has no area - {line}")),
            "{report}"
        );
        assert!(
            report.contains(&format!("Skipped 1: an open polyline has no area - {open}")),
            "{report}"
        );
    }

    #[test]
    fn skipped_entities_are_counted_by_reason_and_their_ids_listed_up_to_a_dozen() {
        // Fourteen points and one rectangle: the points are one group of 14, of
        // which the first 12 ids are listed and "and 2 more" says the rest.
        let mut document = Document::default();
        let mut ids = Vec::new();
        for i in 0..14 {
            ids.push(add_point(
                &mut document,
                Point2::new(f64::from(i), 0.0),
                "",
                None,
            ));
        }
        ids.push(create(
            &mut document,
            create_polyline(
                polyline_of(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)], true),
                EntityAttributes::default(),
            ),
        ));
        let result = ok(compute_area(&document, &ids));
        assert_eq!(result.skipped.len(), 14);
        let report = format_area_report(&result);
        assert!(
            report.contains(&format!(
                "Skipped 14: a point has no area - {} and 2 more",
                listed(&ids, 12)
            )),
            "{report}"
        );
        assert!(!report.contains(&format!("{},", ids[12])), "{report}");
    }

    #[test]
    fn an_id_listed_twice_is_measured_once_and_the_repeat_is_reported() {
        // The rectangle and circle above with the rectangle listed again, as AREA
        // passes typed ids through. The total is still 5000 + 100 pi = 5314.159 m^2
        // (0.5314 ha) and 300 + 20 pi = 300 + 62.832 = 362.832 m round; counting
        // the rectangle twice would give 10314.159 m^2 and 662.832 m.
        let mut document = Document::default();
        let rectangle = create(
            &mut document,
            create_polyline(
                polyline_of(
                    &[(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)],
                    true,
                ),
                EntityAttributes::default(),
            ),
        );
        let circle = create(
            &mut document,
            create_circle(Point2::new(500.0, 500.0), 10.0, EntityAttributes::default()),
        );
        let result = ok(compute_area(&document, &[rectangle, circle, rectangle]));
        assert_eq!(result.items.len(), 2);
        assert_eq!(result.items[0].id, rectangle);
        assert_eq!(result.items[1].id, circle);
        near(result.total_area, 5000.0 + 100.0 * PI, 1e-9);
        near(result.total_perimeter, 300.0 + 20.0 * PI, 1e-9);
        let report = format_area_report(&result);
        assert!(
            report.contains("Total  area 5314.159 m\u{b2} (0.5314 ha)   perimeter 362.832 m"),
            "{report}"
        );
        assert!(
            report.contains(&format!("Skipped 1: listed more than once - {rectangle}")),
            "{report}"
        );
    }

    #[test]
    fn nothing_with_an_area_among_many_points_is_one_short_error_not_an_entry_per_point() {
        // Fifteen points and nothing else, as Area of Selection after Select All
        // on a drawing of points: the error groups them as the report does - one
        // reason, its count of 15, the first 12 ids and "and 3 more" (15 - 12).
        let mut document = Document::default();
        let mut ids = Vec::new();
        for i in 0..15 {
            ids.push(add_point(
                &mut document,
                Point2::new(f64::from(i), f64::from(i)),
                "",
                None,
            ));
        }
        let refusal = err(compute_area(&document, &ids));
        assert_eq!(refusal.code, ErrorCode::InvalidGeometry);
        let expected = format!(
            "skipped 15: a point has no area - {} and 3 more",
            listed(&ids, 12)
        );
        assert_eq!(refusal.context, expected);
    }

    #[test]
    fn nothing_with_an_area_is_an_error_that_says_why() {
        let mut document = Document::default();
        let line = create(
            &mut document,
            create_line(
                Point2::new(0.0, 0.0),
                Point2::new(5.0, 5.0),
                EntityAttributes::default(),
            ),
        );
        let refusal = err(compute_area(&document, &[line]));
        assert_eq!(refusal.code, ErrorCode::InvalidGeometry);
        assert!(
            refusal.context.contains("has no area"),
            "{}",
            refusal.context
        );
        assert_eq!(
            err(compute_area(&document, &[])).code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            err(compute_area(&document, &[999])).code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn a_drawing_in_feet_gets_square_feet_and_no_hectares() {
        let mut document = Document::default();
        let mut metadata = document.metadata().clone();
        metadata.linear_unit = String::from("foot");
        document.set_metadata(metadata);
        let square = create(
            &mut document,
            create_polyline(
                polyline_of(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)], true),
                EntityAttributes::default(),
            ),
        );
        let result = ok(compute_area(&document, &[square]));
        assert!(!result.metres);
        let report = format_area_report(&result);
        assert!(report.contains("area 100.000 square foot"), "{report}");
        assert!(!report.contains(" ha)"), "{report}");
        assert!(report.contains("No hectares"), "{report}");

        assert!(is_metre_unit(" Metres "));
        assert!(is_metre_unit("m"));
        assert!(!is_metre_unit("mm"));
    }

    // ---- traverse ---------------------------------------------------------------------------

    #[test]
    fn a_closed_loop_built_from_known_coordinates_closes_exactly() {
        let result = ok(compute_traverse_tool(&loop_spec()));
        let classical = &result.classical;
        match classical.angular_misclosure {
            Some(misclosure) => near(misclosure, 0.0, 1e-12),
            None => panic!("a loop checks its angles"),
        }
        match &classical.linear_misclosure {
            Some(linear) => near(linear.length, 0.0, 1e-9),
            None => panic!("a loop checks its position"),
        }
        near(classical.total_length, 956.0, 1e-12);

        assert_eq!(result.stations.len(), 4);
        for (station, expected) in result.stations.iter().zip([&A, &B, &C, &D]) {
            assert_eq!(station.id, expected.id);
            near(station.adjusted.x, expected.e, 1e-9);
            near(station.adjusted.y, expected.n, 1e-9);
        }
        assert!(result.stations[0].control);
        assert!(!result.stations[1].control);
    }

    #[test]
    fn a_perturbed_loop_is_adjusted_by_the_bowditch_rule_as_worked_by_hand() {
        // BC booked as 250.100 instead of 250.000, angles exact. The computed
        // figure overshoots by 0.100 along BC's direction, whose unit vector is
        // (dE, dN) = (240, -70) / 250 = (0.96, -0.28):
        //   misclosure dN = 0.100 * -0.28 = -0.028, dE = 0.100 * 0.96 = +0.096,
        //   length 0.100, total length 200 + 250.1 + 208 + 298 = 956.1,
        //   precision 1 : 956.1 / 0.1 = 1 : 9561.
        // Compass rule: each leg is corrected by -misclosure * L / 956.1, so a
        // station's correction is -misclosure * (length run so far) / 956.1.
        let mut spec = loop_spec();
        spec.legs[1].distance = 250.1;
        spec.method = TraverseMethod::Compass;
        let result = ok(compute_traverse_tool(&spec));
        let Some(linear) = result.classical.linear_misclosure else {
            panic!("a loop checks its position");
        };
        near(linear.latitude, -0.028, 1e-12);
        near(linear.departure, 0.096, 1e-12);
        near(linear.length, 0.1, 1e-12);
        match linear.precision_denominator {
            Some(denominator) => near(denominator, 9561.0, 1e-6),
            None => panic!("the closure is not exact"),
        }

        let total = 956.1;
        // B: the computed B is exact (AB untouched); corrected by 200/956.1.
        let to_b = 200.0 / total;
        // C and D: carry the 0.100 overshoot, corrected by the run so far.
        let to_c = 450.1 / total;
        let to_d = 658.1 / total;
        let stations = &result.stations;
        near(stations[1].adjusted.x, B.e - 0.096 * to_b, 1e-9);
        near(stations[1].adjusted.y, B.n + 0.028 * to_b, 1e-9);
        near(stations[2].adjusted.x, C.e + 0.096 - 0.096 * to_c, 1e-9);
        near(stations[2].adjusted.y, C.n - 0.028 + 0.028 * to_c, 1e-9);
        near(stations[3].adjusted.x, D.e + 0.096 - 0.096 * to_d, 1e-9);
        near(stations[3].adjusted.y, D.n - 0.028 + 0.028 * to_d, 1e-9);
        // Unadjusted C is the overshoot itself.
        near(stations[2].unadjusted.x, C.e + 0.096, 1e-9);
        near(stations[2].unadjusted.y, C.n - 0.028, 1e-9);

        let report = format_traverse_report(&result);
        assert!(report.contains("precision 1 : 9561"), "{report}");
        assert!(
            report.contains("Linear misclosure dN -0.028  dE +0.096  length 0.100"),
            "{report}"
        );
        assert!(report.contains("Bowditch"), "{report}");
    }

    #[test]
    fn the_transit_rule_corrects_in_proportion_to_latitude_and_departure() {
        // The same perturbed loop. Latitudes (dN) of the observed legs: 160,
        // 250.1 * -0.28 = -70.028, -192, 102; sum of magnitudes 524.028.
        // Departures: 120, 250.1 * 0.96 = 240.096, -80, -280; sum 720.096.
        // Transit rule on AB: dN correction +0.028 * 160 / 524.028, dE correction
        // -0.096 * 120 / 720.096.
        let mut spec = loop_spec();
        spec.legs[1].distance = 250.1;
        spec.method = TraverseMethod::Transit;
        let result = ok(compute_traverse_tool(&spec));
        near(
            result.stations[1].adjusted.y,
            B.n + 0.028 * 160.0 / 524.028,
            1e-9,
        );
        near(
            result.stations[1].adjusted.x,
            B.e - 0.096 * 120.0 / 720.096,
            1e-9,
        );
        assert!(format_traverse_report(&result).contains("transit rule"));
    }

    #[test]
    fn a_link_traverse_between_control_checks_both_its_angles_and_its_position() {
        let result = ok(compute_traverse_tool(&link_spec()));
        near(result.start_azimuth, 0.0, 1e-15);
        match result.closing_reference_azimuth {
            Some(azimuth) => near(azimuth, PI, 1e-15),
            None => panic!("the link closes on a reference mark"),
        }
        // 3 setup angles + the closing angle take part in the check.
        assert_eq!(result.classical.angle_count, 4);
        match result.classical.angular_misclosure {
            Some(misclosure) => near(misclosure, 0.0, 1e-12),
            None => panic!("the link checks its angles"),
        }
        match &result.classical.linear_misclosure {
            Some(linear) => near(linear.length, 0.0, 1e-9),
            None => panic!("the link checks its position"),
        }

        assert_eq!(result.stations.len(), 4);
        assert!(result.stations[0].control);
        assert!(result.stations[3].control, "D is closing control");
        assert!(!result.stations[1].control);
        near(result.stations[2].adjusted.x, C.e, 1e-9);
        near(result.stations[2].adjusted.y, C.n, 1e-9);
    }

    #[test]
    fn least_squares_holds_the_control_and_counts_its_redundancy() {
        let mut spec = link_spec();
        spec.method = TraverseMethod::LeastSquares;

        // Exact observations: the adjustment has nothing to distribute and must
        // return the constructed coordinates.
        let exact = ok(compute_traverse_tool(&spec));
        let Some(least_squares) = &exact.least_squares else {
            panic!("least squares was asked for");
        };
        near(exact.stations[1].adjusted.x, B.e, 1e-6);
        near(exact.stations[1].adjusted.y, B.n, 1e-6);
        near(exact.stations[2].adjusted.x, C.e, 1e-6);
        near(exact.stations[2].adjusted.y, C.n, 1e-6);
        // Equations: 3 distances, 2 angles at the interior stations B and C, and
        // the two angles turned from reference directions (at A and at D) as
        // azimuths of their legs - 7. Unknowns: B and C, 2 coordinates each - 4.
        // Degrees of freedom 7 - 4 = 3 (katana_survey's build_traverse_network's model).
        assert_eq!(least_squares.statistics.degrees_of_freedom, 3);

        // Perturbed: the control stays exactly where it is published.
        spec.legs[1].distance = 250.05;
        let perturbed = ok(compute_traverse_tool(&spec));
        let stations = &perturbed.stations;
        assert_eq!(stations[0].adjusted, Point2::new(A.e, A.n));
        assert_eq!(stations[stations.len() - 1].adjusted, Point2::new(D.e, D.n));
        match stations[1].sigma_easting {
            Some(sigma) => assert!(sigma > 0.0),
            None => panic!("least squares gives a station's sigma"),
        }
        assert_eq!(
            stations[0].sigma_easting,
            Some(0.0),
            "fixed control has no uncertainty"
        );
        // A 50 mm blunder shared among three legs cannot move a station further
        // than the blunder itself.
        near(stations[2].adjusted.x, C.e, 0.05);
        near(stations[2].adjusted.y, C.n, 0.05);
        let report = format_traverse_report(&perturbed);
        assert!(
            report.contains("angles 5.0\", distances 2.0 mm + 2.0 ppm"),
            "{report}"
        );
        assert!(report.contains("3 degrees of freedom"), "{report}");
    }

    #[test]
    fn an_open_traverse_has_no_check_and_says_so() {
        let mut spec = link_spec();
        spec.kind = TraverseKind::Open;
        spec.closing_angle = None;
        let result = ok(compute_traverse_tool(&spec));
        assert!(result.classical.linear_misclosure.is_none());
        let last = &result.stations[result.stations.len() - 1];
        assert!(!last.control, "an open traverse's end is computed");
        near(last.adjusted.x, D.e, 1e-9);
        near(last.adjusted.y, D.n, 1e-9);
        assert!(format_traverse_report(&result).contains("nothing checks it"));
    }

    #[test]
    fn a_reference_mark_on_top_of_its_station_gives_no_direction() {
        let mut spec = link_spec();
        spec.start_orientation.reference_mark = Some(spec.start);
        let refusal = err(compute_traverse_tool(&spec));
        assert_eq!(refusal.code, ErrorCode::InvalidArgument);
        assert!(refusal.message.contains("backsight"), "{}", refusal.message);
    }

    #[test]
    fn adding_a_traverse_is_one_undo_step_of_new_stations_and_one_polyline() {
        let mut document = Document::default();
        let result = ok(compute_traverse_tool(&loop_spec()));
        let command = ok(traverse_command(&document, &result));
        must(&mut document, command);

        // B, C and D are new; A is the control it hung on.
        let mut points = 0;
        let mut polylines = 0;
        for id in document.last_created_entities() {
            let entity = entity_of(&document, id);
            if let Geometry::Polyline(line) = &entity.geometry {
                polylines += 1;
                assert!(line.closed);
                assert_eq!(line.vertices.len(), 4);
                near(line.vertices[2].x, C.e, 1e-9);
                near(line.vertices[2].y, C.n, 1e-9);
            } else {
                points += 1;
                assert_ne!(property_to_string(&entity.properties["point"]), "A");
            }
        }
        assert_eq!(points, 3);
        assert_eq!(polylines, 1);
        done(document.undo());
        assert!(document.model().entities.empty());
    }

    #[test]
    fn traverse_legs_are_read_from_a_field_book_and_a_bad_line_is_named() {
        let legs = ok(parse_traverse_legs(
            "# station angle distance\n\
             A 286d53m11.63s 200.000\n\
             \n\
             B, 249:23:26.4, 250\n",
        ));
        assert_eq!(legs.len(), 2);
        assert_eq!(legs[1].station_id, "B");
        near(
            legs[1].angle,
            (249.0 + 23.0 / 60.0 + 26.4 / 3600.0) * PI / 180.0,
            1e-15,
        );
        assert_eq!(legs[1].distance, 250.0);

        let bad = err(parse_traverse_legs("A 10d 100\nB 12x 100\n"));
        assert_eq!(bad.code, ErrorCode::ParseFailure);
        assert!(bad.context.contains("line 2"), "{}", bad.context);
        assert!(
            parse_traverse_legs("A 10 20 30 100").is_err(),
            "a blank inside the angle"
        );
        assert_eq!(
            err(parse_traverse_legs("# nothing\n")).code,
            ErrorCode::InvalidArgument
        );
    }

    // ---- level book -------------------------------------------------------------------------

    #[test]
    fn a_hand_reduced_level_book_is_reproduced_line_by_line() {
        let result = ok(compute_level_book(&book_spec(LevelAdjustment::None)));
        let rows = &result.rows;
        assert_eq!(rows.len(), 5);
        let collimation = |row: usize| match rows[row].height_of_collimation {
            Some(height) => height,
            None => panic!("row {row} sets up the level"),
        };
        near(collimation(0), 101.5, 1e-12);
        near(rows[1].reduced_level, 99.5, 1e-12);
        near(rows[2].reduced_level, 101.0, 1e-12);
        near(collimation(2), 103.25, 1e-12);
        near(rows[3].reduced_level, 102.0, 1e-12);
        near(rows[4].reduced_level, 99.99, 1e-12);
        assert!(
            rows[1].height_of_collimation.is_none(),
            "an intersight sets up nothing"
        );

        near(result.sum_backsights, 3.75, 1e-12);
        near(result.sum_foresights, 3.76, 1e-12);
        near(result.rise_minus_fall, -0.01, 1e-12);
        near(result.last_minus_first, -0.01, 1e-12);
        match result.misclosure {
            Some(misclosure) => near(misclosure, -0.01, 1e-12),
            None => panic!("the run closes on a known level"),
        }

        let report = format_level_book_report(&result);
        assert!(
            report.contains(
                "Check: sum BS 3.750 - sum FS 3.760 = -0.010; last RL - first RL = -0.010"
            ),
            "{report}"
        );
        assert!(
            report.contains("Misclosure -0.0100 (computed - known)"),
            "{report}"
        );
    }

    #[test]
    fn adjusting_by_setups_gives_every_reading_of_a_setup_its_share() {
        // Two setups share +0.010: setup 1 (A, CP1) +0.005, setup 2 (B, BM2) +0.010.
        let result = ok(compute_level_book(&book_spec(LevelAdjustment::BySetups)));
        let rows = &result.rows;
        near(rows[1].correction, 0.005, 1e-12);
        near(rows[1].adjusted_level, 99.505, 1e-12);
        near(rows[2].adjusted_level, 101.005, 1e-12);
        near(rows[3].correction, 0.010, 1e-12);
        near(rows[3].adjusted_level, 102.010, 1e-12);
        // The run closes on the known level.
        near(rows[4].adjusted_level, 100.0, 1e-12);
    }

    #[test]
    fn adjusting_by_distance_weighs_each_setup_by_the_length_levelled() {
        // Setup lengths 100 and 150, total 250: setup 1 gets 0.010 * 100/250 =
        // +0.004 and setup 2 the whole +0.010.
        let result = ok(compute_level_book(&book_spec(LevelAdjustment::ByDistance)));
        let rows = &result.rows;
        near(rows[1].correction, 0.004, 1e-12);
        near(rows[2].adjusted_level, 101.004, 1e-12);
        near(rows[3].correction, 0.010, 1e-12);
        // Allowable 12 mm * sqrt(0.250 km) = 6 mm; the 10 mm misclosure exceeds it.
        match result.allowable {
            Some(allowable) => near(allowable, 0.006, 1e-12),
            None => panic!("every setup has its length"),
        }
        assert!(format_level_book_report(&result).contains("EXCEEDS it"));
    }

    #[test]
    fn a_misclosure_inside_the_allowance_is_reported_as_within_it() {
        // Two kilometres levelled: 12 mm * sqrt(2) = 16.97 mm > 10 mm.
        let mut spec = book_spec(LevelAdjustment::None);
        spec.lines[2].distance = Some(1000.0);
        spec.lines[4].distance = Some(1000.0);
        let result = ok(compute_level_book(&spec));
        match result.allowable {
            Some(allowable) => near(allowable, 0.012 * 2.0_f64.sqrt(), 1e-12),
            None => panic!("every setup has its length"),
        }
        assert!(format_level_book_report(&result).contains("within it"));
    }

    #[test]
    fn a_length_missing_from_one_setup_is_not_taken_as_zero() {
        let mut spec = book_spec(LevelAdjustment::ByDistance);
        spec.lines[4].distance = None;
        // Weighting by a length of zero nobody measured.
        let refused = err(compute_level_book(&spec));
        assert_eq!(refused.code, ErrorCode::InvalidArgument);

        spec.adjustment = LevelAdjustment::None;
        let unadjusted = ok(compute_level_book(&spec));
        assert!(unadjusted.allowable.is_none());
        assert!(unadjusted.total_distance.is_none());
        assert!(format_level_book_report(&unadjusted).contains("no allowable misclosure"));
    }

    #[test]
    fn a_misbooked_level_book_is_refused_at_the_line_that_is_wrong() {
        let refuse = |text: &str| -> String {
            let spec = LevelBookSpec {
                lines: ok(parse_level_book(text)),
                ..LevelBookSpec::default()
            };
            match compute_level_book(&spec) {
                Ok(_) => panic!("{text} was reduced"),
                Err(error) => error.context,
            }
        };
        // Must open on a backsight.
        assert_eq!(refuse("A - 1.0 -\nB - - 1.0\n"), "line 1 (A)");
        // An intersight line with a backsight.
        assert_eq!(refuse("A 1.0 - -\nB 1.0 1.0 -\nC - - 1.0\n"), "line 2 (B)");
        // A backsight with no foresight is not a change point.
        assert_eq!(refuse("A 1.0 - -\nB 1.0 - -\nC - - 1.0\n"), "line 2 (B)");
        // Ends on an open setup.
        assert_eq!(refuse("A 1.0 - -\nB 1.0 - 1.0\n"), "line 2 (B)");
        // A distance on an intersight.
        assert_eq!(refuse("A 1.0 - -\nB - 1.0 - 50\nC - - 1.0\n"), "line 2 (B)");

        // What cannot be read is refused by the parser, naming the line: in a
        // forty-line book "the backsight must be a number" alone says nothing.
        let parse_refusal = |text: &str| -> String {
            match parse_level_book(text) {
                Ok(_) => panic!("{text} was read"),
                Err(error) => {
                    assert_eq!(error.code, ErrorCode::ParseFailure, "{text}");
                    error.context
                }
            }
        };
        let no_reading = parse_refusal("A - - -\n");
        assert!(no_reading.contains("line 1"), "no reading: {no_reading}");
        let too_few = parse_refusal("A 1.0 -\n");
        assert!(too_few.contains("line 1"), "too few columns: {too_few}");
        let negative = parse_refusal("A 1.0 - - -5\n");
        assert!(
            negative.contains("line 1"),
            "a negative distance: {negative}"
        );
        // A reading that is not a number, on a line that is not the first.
        let unreadable = parse_refusal("A 1.0 - -\nB x - 1.0\n");
        assert!(
            unreadable.contains("line 2"),
            "an unreadable reading: {unreadable}"
        );
    }

    // ---- angle calculator -------------------------------------------------------------------

    #[test]
    fn angles_convert_by_the_definitions_of_their_units() {
        // A right angle is 90 degrees, 100 gon (400 to the circle), pi/2 rad.
        let right = ok(convert_angle("90", AngleInputUnit::Degrees));
        near(right.gons, 100.0, 1e-12);
        near(right.radians, 0.5 * PI, 1e-15);
        near(right.back_azimuth, 1.5 * PI, 1e-15);
        let report = format_angle_conversion(&right);
        assert!(report.contains("100.00000000 gon"), "{report}");
        assert!(
            report.contains("bearing N 90\u{b0}00'00.00\" E"),
            "{report}"
        );
        assert!(report.contains("reverse 270\u{b0}00'00.00\""), "{report}");

        let gons = ok(convert_angle("200", AngleInputUnit::Gons));
        near(gons.degrees, 180.0, 1e-12);

        let bearing = ok(convert_angle("S 45 W", AngleInputUnit::Bearing));
        near(bearing.degrees, 225.0, 1e-12);

        // -30 degrees read as a direction is 330.
        let negative = ok(convert_angle("-30", AngleInputUnit::Degrees));
        near(negative.azimuth, 11.0 * PI / 6.0, 1e-15);
        assert!(convert_angle("pi", AngleInputUnit::Radians).is_err());
    }

    // ---- coordinate converter ---------------------------------------------------------------
    //
    // UTM by definition (NGA.SIG.0012, the UTM grid): zone n has its central
    // meridian at 6n - 183 degrees, so zone 30's is 3 degrees WEST; the false
    // easting is 500 000 m, the false northing 0 m in the northern hemisphere, and
    // the scale factor on the central meridian is 0.9996. So latitude 0, longitude
    // -3 is E 500 000, N 0 exactly, with k = 0.9996 and no convergence there.

    #[test]
    fn the_equator_on_zone30s_central_meridian_is_the_utm_false_origin() {
        let conversion = ok(convert_coordinates("4326", "EPSG:32630", "origin 0 -3\n"));
        assert_eq!(conversion.rows.len(), 1);
        let row = &conversion.rows[0];
        assert_eq!(row.label, "origin");
        assert!(conversion.source_geographic);
        assert!(!conversion.target_geographic);
        // Easting, then northing.
        near(row.out_first, 500000.0, 1e-6);
        near(row.out_second, 0.0, 1e-6);
        match &row.target_factors {
            Some(factors) => {
                near(factors.point_scale_factor(), 0.9996, 1e-9);
                near(factors.convergence_degrees, 0.0, 1e-9);
            }
            None => panic!("a projected system has grid factors"),
        }
        assert!(
            row.source_factors.is_none(),
            "a geographic system has no grid factors"
        );
        let report = format_coordinate_conversion(&conversion);
        assert!(report.contains("500000.000 0.000"), "{report}");
        assert!(report.contains("k 0.99960000"), "{report}");

        // And back: grid to geographic, latitude first in the answer.
        let back = ok(convert_coordinates("32630", "4326", "500000 0"));
        near(back.rows[0].out_first, 0.0, 1e-9); // latitude
        near(back.rows[0].out_second, -3.0, 1e-9); // longitude
        assert_eq!(back.rows[0].label, "P1");
    }

    #[test]
    fn a_geographic_point_may_be_typed_in_degrees_minutes_and_seconds() {
        // -3 degrees written as -3:00:00.
        let conversion = ok(convert_coordinates("4326", "32630", "0:00:00 -3:00:00"));
        near(conversion.rows[0].out_first, 500000.0, 1e-6);
    }

    #[test]
    fn an_angle_with_blanks_inside_it_is_refused_rather_than_read_as_the_last_two_fields() {
        // -33°51'24.5" 151°12'55" typed with blanks. Blanks also separate the
        // label from the two coordinates, so read as written this is the label
        // "CP1 -33 51 24.5 151" at latitude 12, longitude 55 - a point on another
        // continent, reported without complaint. Two numbers in the label are the
        // mark of it, and of a third coordinate (E N height) typed after a name.
        let blanks = match convert_coordinates("4326", "32756", "CP1 -33 51 24.5 151 12 55\n") {
            Ok(conversion) => panic!("read as {}", format_coordinate_conversion(&conversion)),
            Err(error) => error,
        };
        assert_eq!(blanks.code, ErrorCode::ParseFailure);
        assert!(blanks.context.contains("line 1"), "{}", blanks.context);
        // The latitude without blanks and the longitude with them: the label
        // "CP1 -33:51:24.5 151" holds two angles.
        assert_eq!(
            err(convert_coordinates(
                "4326",
                "32756",
                "CP1 -33:51:24.5 151 12 55"
            ))
            .code,
            ErrorCode::ParseFailure
        );
        // Easting, northing and a height after a numeric point name: read as
        // written, the label "102 500000" at E 5000, N 25.
        let height = err(convert_coordinates(
            "32630",
            "4326",
            "500000 0\n102 500000 5000 25\n",
        ));
        assert!(height.context.contains("line 2"), "{}", height.context);

        // One number in a label is a point name: "102" and "CP 1" still work.
        let named = ok(convert_coordinates(
            "32630",
            "4326",
            "102 500000 0\nCP 1 500000 0\n",
        ));
        assert_eq!(named.rows[0].label, "102");
        assert_eq!(named.rows[1].label, "CP 1");
    }

    #[test]
    fn the_converter_refuses_what_it_cannot_do_properly() {
        assert!(
            convert_coordinates("999999", "4326", "0 0").is_err(),
            "no such EPSG code"
        );
        assert_eq!(
            err(convert_coordinates("", "4326", "0 0")).code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            err(convert_coordinates("4326", "32630", "\n# nothing\n")).code,
            ErrorCode::InvalidArgument
        );
        let bad = err(convert_coordinates("32630", "4326", "P1 500000 north"));
        assert_eq!(bad.code, ErrorCode::ParseFailure);
        assert!(bad.context.contains("line 1"));
    }

    #[test]
    fn selected_points_become_converter_lines_only_for_a_projected_source() {
        let mut document = Document::default();
        let p = add_point(&mut document, Point2::new(500000.25, 12.5), "CP 1", None);
        document.selection_mut().set(vec![p]);
        let lines = ok(conversion_lines_for_selection(&document, "32630"));
        assert_eq!(lines, "CP 1 500000.25 12.5\n");
        let conversion = ok(convert_coordinates("32630", "4326", &lines));
        assert_eq!(
            conversion.rows[0].label, "CP 1",
            "a name with a blank survives"
        );

        assert_eq!(
            err(conversion_lines_for_selection(&document, "4326")).code,
            ErrorCode::InvalidArgument,
            "the drawing's x is an easting, not a latitude"
        );
        document.selection_mut().clear();
        assert_eq!(
            err(conversion_lines_for_selection(&document, "32630")).code,
            ErrorCode::InvalidState
        );
    }

    #[test]
    fn a_point_named_with_two_numbers_is_joined_so_use_selection_does_not_make_a_line_it_refuses() {
        // "STN 3 4" written out as it is would be a label with two numbers in it,
        // which convert_coordinates refuses as an angle typed with blanks. The
        // blanks become '_', as '#' and ',' already do.
        let mut document = Document::default();
        let p = add_point(&mut document, Point2::new(500000.25, 12.5), "STN 3 4", None);
        document.selection_mut().set(vec![p]);
        let lines = ok(conversion_lines_for_selection(&document, "32630"));
        assert_eq!(lines, "STN_3_4 500000.25 12.5\n");
        let conversion = ok(convert_coordinates("32630", "4326", &lines));
        assert_eq!(conversion.rows[0].label, "STN_3_4");
    }
}

// ---- the command line -------------------------------------------------------------------

mod survey_tools_interpreter {
    use super::*;

    #[test]
    fn inverse_prints_the_same_report_as_the_dialog() {
        let mut session = Session::default();
        let reply = session.ok("INVERSE 1000,5000 1003,5004");
        assert_eq!(
            reply,
            format_inverse_report(&ok(compute_inverse(
                &at(1000.0, 5000.0, None),
                &at(1003.0, 5004.0, None),
            )))
        );
        assert!(reply.contains("Horizontal distance 5.000"));
    }

    #[test]
    fn inverse_takes_point_ids_and_the_ends_of_a_line() {
        let mut session = Session::default();
        let a = add_point(
            &mut session.document,
            Point2::new(1000.0, 5000.0),
            "A",
            Some(10.0),
        );
        let b = add_point(
            &mut session.document,
            Point2::new(1003.0, 5004.0),
            "B",
            Some(12.0),
        );
        let points = session.ok(&format!("INVERSE {a} {b}"));
        assert!(points.contains("Height difference +2.000"), "{points}");
        assert!(points.contains("(A)"), "{points}");

        session.ok("LINE 0,0 -1,-1");
        let line = last_created(&session.document);
        let ends = session.ok(&format!("INVERSE {line}"));
        assert!(ends.contains("Bearing S 45\u{b0}00'00.00\" W"), "{ends}");

        assert!(session.run("INVERSE").is_err());
        assert!(session.run("INVERSE 1,1 1,1").is_err(), "coincident");
        assert!(
            session.run(&format!("INVERSE {a}")).is_err(),
            "a point has no two ends"
        );
    }

    #[test]
    fn forward_creates_a_named_point_that_one_undo_removes() {
        let mut session = Session::default();
        // N 36°52'11.63" E is the 3-4-5 direction to 0.005" (0.1 micrometre at 5 m).
        let reply = session.ok("FORWARD 1000,5000,20 N36d52m11.63sE 5 -1 P7");
        assert!(reply.contains("Created on layer 0"), "{reply}");
        assert_eq!(session.document.model().entities.size(), 1);
        let point = entity_of(&session.document, last_created(&session.document));
        let there = position_of(&point);
        near(there.x, 1003.0, 1e-6);
        near(there.y, 5004.0, 1e-6);
        assert_eq!(property_to_string(&point.properties["point"]), "P7");
        assert_eq!(heights_of(&point.properties, 1)[0], Some(19.0));
        session.ok("UNDO");
        assert!(session.document.model().entities.empty());

        // RADIATE, a quoted bearing, no height difference, a numeric name.
        session.ok("RADIATE 0,0 \"S 45 00 00 W\" 10 - 102");
        let second = entity_of(&session.document, last_created(&session.document));
        let there = position_of(&second);
        near(there.x, -10.0 / 2.0_f64.sqrt(), 1e-9);
        near(there.y, -10.0 / 2.0_f64.sqrt(), 1e-9);
        assert_eq!(
            property_to_string(&second.properties["point"]),
            "102",
            "the fifth field is the name, never a height"
        );
        assert!(!second.properties.contains_key(ELEVATION_PROPERTY));

        assert!(session.run("FORWARD 0,0 45").is_err());
        assert!(
            session.run("FORWARD 0,0 45 10 2").is_err(),
            "a height difference from a start with no height"
        );
        assert!(session.run("FORWARD 0,0 X45 10").is_err());
    }

    #[test]
    fn area_measures_the_selection_or_the_ids_given() {
        let mut session = Session::default();
        assert_eq!(
            session.fails("AREA"),
            ErrorCode::InvalidState,
            "nothing selected"
        );
        session.ok("RECT 0,0 100,50");
        let rectangle = last_created(&session.document);
        session.ok("SELECT ALL");
        let selected = session.ok("AREA");
        assert!(
            selected.contains("area 5000.000 m\u{b2} (0.5000 ha)"),
            "{selected}"
        );
        let by_id = session.ok(&format!("AREA {rectangle}"));
        assert_eq!(by_id, selected);
        assert!(session.run("AREA x").is_err());
    }

    #[test]
    fn area_counts_an_id_typed_twice_once() {
        // A 100 x 50 rectangle is 5000 m^2 (0.5 ha) and 300 m round, however many
        // times its id is typed.
        let mut session = Session::default();
        session.ok("RECT 0,0 100,50");
        let id = last_created(&session.document);
        let reply = session.ok(&format!("AREA {id} {id}"));
        assert!(
            reply.contains("Total  area 5000.000 m\u{b2} (0.5000 ha)   perimeter 300.000 m"),
            "{reply}"
        );
        assert!(
            reply.contains(&format!("Skipped 1: listed more than once - {id}")),
            "{reply}"
        );
    }

    #[test]
    fn help_documents_the_survey_verbs() {
        let help = CommandInterpreter::help_text();
        for verb in ["INVERSE", "FORWARD", "RADIATE", "AREA"] {
            assert!(help.contains(verb), "{verb}");
        }
    }
}
