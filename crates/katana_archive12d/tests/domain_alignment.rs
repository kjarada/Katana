//! The twin of tests/archive12d/test_domain_alignment.cpp: super alignments
//! and the superseded alignment strings into Katana's PI-defined alignments,
//! and when they may not be.

#![allow(
    clippy::module_inception,
    clippy::excessive_precision,
    reason = "Inner module names identify the C++ suites for the parity checker; decimal constants are copied verbatim from the C++ oracle."
)]

pub mod support;

use std::f64::consts::PI;

use katana_entity::PointGeometry;
use katana_geometry::alignment::solve_alignment;
use katana_geometry::{AlignmentPi, Point2, Polyline2, ProfilePvi};

use support::{all_warnings, any_warning_contains, holds, import, near, shape};

// A right-angle corner rounded at radius 40, worked by hand: PIs (0,0),
// (100,0), (100,100); the tangent length of a 90 degree curve is R, so the
// curve runs from (60,0) to (100,40) and turns LEFT - a negative radius in
// the archive.
const CORNER_ELEMENTS: &str = r#"
  horizontal_data { name "C" closed 0
    data_2d { 0 0  60 0  100 40  100 100 }
    geometry_data { straight { } arc { radius RADIUS major 0 } straight { } } }"#;

fn corner(radius: &str) -> String {
    CORNER_ELEMENTS.replacen("RADIUS", radius, 1)
}

mod domain_import_alignment {
    use super::*;

    #[test]
    fn the_ip_method_gives_its_pis_directly() {
        let domain = import(
            "model \"Road\" string super_alignment { name \"MC01\" chainage 1000\n\
             \x20spiral_type \"clothoid\" horizontal_parts {\n\
             \x20 ip { id 100 x 0 y 0 } spiral { id 200 r 200 l1 30 l2 40 x 500 y 0 }\n\
             \x20 arc { id 300 r 150 x 500 y 600 } ip { id 400 x 1000 y 600 } } }",
        );
        assert_eq!(domain.alignments.len(), 1, "{}", all_warnings(&domain));
        let alignment = &domain.alignments[0];
        assert_eq!(alignment.name, "MC01");
        assert_eq!(alignment.horizontal.start_station, 1000.0);
        assert_eq!(alignment.horizontal.pis.len(), 4);
        assert_eq!(
            alignment.horizontal.pis[1],
            AlignmentPi {
                point: Point2::new(500.0, 0.0),
                radius: 200.0,
                spiral_in: 30.0,
                spiral_out: 40.0,
            }
        );
        assert_eq!(alignment.horizontal.pis[2].radius, 150.0);
        assert_eq!(alignment.horizontal.pis[2].spiral_in, 0.0);
        assert!(alignment.vertical.is_none());
        // ... and the centreline is on the drawing as well.
        assert_eq!(domain.entities.len(), 1);
        assert!(holds::<Polyline2>(&domain.entities[0].geometry));
        assert_eq!(domain.entities[0].layer, "Road");
    }

    #[test]
    fn pis_are_reconstructed_from_solved_elements_and_the_turn_is_read_the_right_way() {
        let domain = import(&format!(
            "string super_alignment {{ name \"C\"{} }}",
            corner("-40")
        ));
        assert_eq!(domain.alignments.len(), 1, "{}", all_warnings(&domain));
        let pis = &domain.alignments[0].horizontal.pis;
        assert_eq!(pis.len(), 3);
        near(pis[1].point.x, 100.0, 1e-6);
        near(pis[1].point.y, 0.0, 1e-6);
        assert_eq!(pis[1].radius, 40.0);

        // What Katana solves from them is the curve that was written: a
        // quarter circle of radius 40 between two 60 m tangents.
        let solved = solve_alignment(&domain.alignments[0].horizontal)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        near(solved.length(), 60.0 + 40.0 * PI / 2.0 + 60.0, 1e-9);
    }

    #[test]
    fn a_reconstruction_that_does_not_match_twelve_ds_geometry_is_rejected() {
        // The same corner with the radius's sign flipped: the arc now bulges
        // the other way, its tangents meet nowhere near (100,0), and the
        // alignment solved from that does not pass through the archive's
        // vertices. It must arrive as a polyline only - with its shape, and a
        // reason - not as a wrong Alignment.
        let domain = import(&format!(
            "string super_alignment {{ name \"C\"{} }}",
            corner("40")
        ));
        assert!(domain.alignments.is_empty());
        assert_eq!(domain.entities.len(), 1);
        assert!(
            any_warning_contains(&domain, "imported as a polyline only"),
            "{}",
            all_warnings(&domain)
        );
    }

    #[test]
    fn transitions_katana_does_not_have_keep_the_alignment_a_polyline() {
        let domain = import(
            r#"string super_alignment { name "Rail" horizontal_data { closed 0
  data_2d { 1262.0 1760.41  1342.85432099 1789.30813759  1419.06779208 1813.40212949 }
  geometry_data { straight { }
    spiral { type "cubic parabola" leading 1 l1 0 r1 0 a1 20.20620846 l2 80 r2 294.56299992 a2 12.2642972 } } } }"#,
        );
        assert!(domain.alignments.is_empty());
        assert_eq!(domain.entities.len(), 1);
        assert!(
            shape::<Polyline2>(&domain.entities[0].geometry)
                .vertices
                .len()
                > 10,
            "the transition is still drawn as the curve it is"
        );
        assert!(
            any_warning_contains(&domain, "cubic parabola"),
            "{}",
            all_warnings(&domain)
        );
    }

    #[test]
    fn compound_curves_and_alignments_ending_on_a_curve_are_not_pi_definable() {
        let compound = import(
            r#"string super_alignment { name "Compound" horizontal_data { closed 0
  data_2d { 0 0  60 0  100 40  100 100 }
  geometry_data { straight { } arc { radius -40 major 0 } arc { radius -5000 major 0 } } } }"#,
        );
        assert!(compound.alignments.is_empty());
        assert_eq!(compound.entities.len(), 1);
        assert!(
            any_warning_contains(&compound, "polyline only"),
            "{}",
            all_warnings(&compound)
        );

        let ends_on_curve = import(
            r#"string super_alignment { name "Stub" horizontal_data { closed 0
  data_2d { 0 0  60 0  100 40 } geometry_data { straight { } arc { radius -40 major 0 } } } }"#,
        );
        assert!(ends_on_curve.alignments.is_empty());
        assert!(
            any_warning_contains(&ends_on_curve, "ends on a curve"),
            "{}",
            all_warnings(&ends_on_curve)
        );
    }

    #[test]
    fn the_grade_line_comes_from_the_solved_vertical_geometry() {
        // VIPs (0,30), (100,33) with a 40 m parabola, (220,31). The curve runs
        // from chainage 80 to 120: 33 - 0.03*20 = 32.4 in, 33 - (2/120)*20 =
        // 32.6667 out.
        let domain = import(&format!(
            "string super_alignment {{ name \"C\"{}\n  vertical_data {{ closed 0\n    \
             data_2d {{ 0 30  80 32.4  120 32.666666666666664  220 31 }}\n    \
             geometry_data {{ straight {{ }} parabola {{ chainage 100 height 33 }} straight {{ }} }} }} }}",
            corner("-40")
        ));
        assert_eq!(domain.alignments.len(), 1, "{}", all_warnings(&domain));
        let vertical = domain.alignments[0]
            .vertical
            .as_ref()
            .unwrap_or_else(|| panic!("{}", all_warnings(&domain)));
        let pvis = &vertical.pvis;
        assert_eq!(pvis.len(), 3);
        assert_eq!(
            pvis[1],
            ProfilePvi {
                station: 100.0,
                elevation: 33.0,
                curve_length: 40.0,
            }
        );
        assert_eq!(
            pvis[2],
            ProfilePvi {
                station: 220.0,
                elevation: 31.0,
                curve_length: 0.0,
            }
        );
    }

    #[test]
    fn a_grade_line_that_does_not_match_is_left_out_and_the_alignment_kept() {
        // The parabola's VIP is recorded at height 40, which the vertices
        // either side of it contradict.
        let domain = import(&format!(
            "string super_alignment {{ name \"C\"{}\n  vertical_data {{ closed 0\n    \
             data_2d {{ 0 30  80 32.4  120 32.666666666666664  220 31 }}\n    \
             geometry_data {{ straight {{ }} parabola {{ chainage 100 height 40 }} straight {{ }} }} }} }}",
            corner("-40")
        ));
        assert_eq!(domain.alignments.len(), 1);
        assert!(domain.alignments[0].vertical.is_none());
        assert!(
            any_warning_contains(&domain, "vertical geometry was not imported"),
            "{}",
            all_warnings(&domain)
        );
    }

    #[test]
    fn an_old_alignment_strings_vips_become_the_grade_line() {
        // K = L / |change of grade in percent|. Grades +3% then -1.6667%: a
        // change of 4.6667%, so K = 10 asks for a curve of 46.667 m. A
        // circular curve of radius 1000 over the same change is 1000 *
        // 0.046667 = 46.667 m too.
        let domain = import(
            r#"string alignment { name "AL"
  hipdata { 0 0 0  100 0 40  100 100 0 }
  vipdata { 0 30 0  100 33 40 parabola  220 31 0 } }
string super_alignment { name "K" horizontal_parts { ip { id 100 x 0 y 0 } ip { id 200 x 300 y 0 } }
  vertical_parts { ip { id 300 x 0 y 30 } kvalue { id 400 k 10 x 100 y 33 } ip { id 500 x 220 y 31 } } }
string super_alignment { name "R" horizontal_parts { ip { id 100 x 0 y 0 } ip { id 200 x 300 y 0 } }
  vertical_parts { ip { id 300 x 0 y 30 } radius { id 400 r 1000 x 100 y 33 } ip { id 500 x 220 y 31 } } }"#,
        );
        assert_eq!(domain.alignments.len(), 3, "{}", all_warnings(&domain));
        let first = domain.alignments[0]
            .vertical
            .as_ref()
            .expect("a grade line");
        assert_eq!(first.pvis[1].curve_length, 40.0);
        assert_eq!(domain.alignments[0].horizontal.pis[1].radius, 40.0);
        let change = (3.0 / 100.0 + 2.0 / 120.0) * 100.0;
        let second = domain.alignments[1]
            .vertical
            .as_ref()
            .expect("a grade line");
        near(second.pvis[1].curve_length, 10.0 * change, 1e-9);
        let third = domain.alignments[2]
            .vertical
            .as_ref()
            .expect("a grade line");
        near(third.pvis[1].curve_length, 1000.0 * change / 100.0, 1e-9);
    }

    #[test]
    fn names_are_made_unique_and_a_blank_one_is_given_one() {
        let two = "horizontal_parts { ip { id 100 x 0 y 0 } ip { id 200 x 10 y 0 } }";
        let domain = import(&format!(
            "string super_alignment {{ name \"A\" {two} }}\n\
             string super_alignment {{ name \"a\" {two} }}\n\
             string super_alignment {{ name \"\" {two} }}"
        ));
        assert_eq!(domain.alignments.len(), 3);
        assert_eq!(domain.alignments[0].name, "A");
        assert_eq!(
            domain.alignments[1].name, "a (2)",
            "names differing only in case would collide"
        );
        assert_eq!(domain.alignments[2].name, "Alignment");
    }

    #[test]
    fn an_alignment_katana_cannot_solve_is_still_drawn_through_its_ips() {
        // One IP: nothing to solve, and no solved geometry. The IP itself is
        // still where the designer put it, and is drawn - as a point here, as
        // the tangent polygon when there are more - with a warning that says
        // that is all it is. Silently drawing nothing while saying "polyline
        // only" was a defect found by review.
        let one = import(
            "string super_alignment { name \"One\" horizontal_parts { ip { id 100 x 5 y 6 } } }",
        );
        assert!(one.alignments.is_empty());
        assert_eq!(one.entities.len(), 1);
        assert_eq!(
            shape::<PointGeometry>(&one.entities[0].geometry).position,
            Point2::new(5.0, 6.0)
        );
        assert!(
            any_warning_contains(&one, "\"One\""),
            "{}",
            all_warnings(&one)
        );
        assert!(
            any_warning_contains(&one, "polygon through its IPs only"),
            "{}",
            all_warnings(&one)
        );

        // A reversal - the alignment doubles back through 180 degrees - cannot
        // be rounded by any curve (alignment.rs), so the three IPs are all
        // there is.
        let reversal =
            import("string alignment { name \"Back\" hipdata { 0 0 0  100 0 40  0 0 0 } }");
        assert!(reversal.alignments.is_empty());
        assert_eq!(reversal.entities.len(), 1);
        let polygon = shape::<Polyline2>(&reversal.entities[0].geometry);
        assert_eq!(polygon.vertices.len(), 3);
        assert_eq!(polygon.vertices[1], Point2::new(100.0, 0.0));
        assert!(
            any_warning_contains(&reversal, "polygon through its IPs only"),
            "{}",
            all_warnings(&reversal)
        );
    }

    #[test]
    fn an_ip_definition_with_another_transition_type_is_approximated_and_said_to_be() {
        // Manual 1.5.16: an alignment string's spiral_type may be cubic
        // parabola, westrail-cubic or cubic spiral as well as clothoid. With no
        // solved geometry to check against, the IPs are taken with clothoids
        // of the same lengths - which is what the warning must say.
        let domain = import(
            r#"string alignment { name "rail" spiral_type "cubic parabola"
  hipdata { 0 0 0   500 0 200 spil1 40 spil2 40   900 400 0 }
  vipdata { 0 10 0  450 16 100  900 13 0 } }"#,
        );
        assert_eq!(domain.alignments.len(), 1, "{}", all_warnings(&domain));
        assert_eq!(domain.alignments[0].horizontal.pis[1].spiral_in, 40.0);
        assert!(domain.alignments[0].vertical.is_some());
        assert_eq!(domain.entities.len(), 1);
        assert!(
            any_warning_contains(&domain, "represented by clothoids of the same length"),
            "{}",
            all_warnings(&domain)
        );
    }

    #[test]
    fn a_vertical_arcs_major_flag_is_ignored_as_the_manual_says() {
        // Manual 1.5.9.2.3(b): "major ... is ignored since only minor arcs are
        // used". Identical files but for the flag must give identical
        // profiles.
        let head = "string super_alignment { name \"V\" horizontal_data { closed 0 \
                    data_2d { 0 0  1000 0 } geometry_data { straight { } } } \
                    vertical_data { closed 0 data_2d { 0 10  400 18  600 20  1000 20 } \
                    geometry_data { straight { } arc { radius 10000.5 major ";
        let minor = import(&format!("{head}0 }} straight {{ }} }} }} }}"));
        let major = import(&format!("{head}1 }} straight {{ }} }} }} }}"));
        assert_eq!(minor.alignments.len(), 1, "{}", all_warnings(&minor));
        assert_eq!(major.alignments.len(), 1, "{}", all_warnings(&major));
        let minor_profile = minor.alignments[0]
            .vertical
            .as_ref()
            .unwrap_or_else(|| panic!("{}", all_warnings(&minor)));
        let major_profile = major.alignments[0]
            .vertical
            .as_ref()
            .unwrap_or_else(|| panic!("{}", all_warnings(&major)));
        assert_eq!(major_profile.pvis, minor_profile.pvis);
    }
}
