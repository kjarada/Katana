// The solids road objects are made of (tests/cad/road/test_road_object_mesh.cpp).
//
// Every expected volume is worked out by hand from the shape: a box is its
// three sides multiplied, a prism its section's area times its length, a
// hollow sweep the ring between its loops times its length. A mitred corner
// keeps a section symmetric about its line at exactly its area times the
// length of that line: what the outside of the bend gains, the inside loses.

use crate::near;
use katana_cad::road::road_object_mesh::{
    PathPoint, add_box, add_extrusion, add_hollow_sweep, add_prism, add_sweep, counter_clockwise,
    is_closed_mesh, regular_polygon,
};
use katana_geometry::mesh::mesh_volume;
use katana_geometry::{Point2, Point3, TriangleMesh, Vec2, Vec3};
use katana_math::cpp;
use katana_math::numerics::PI;

// Whether the mesh has a vertex at (x, y, z), to a picometre.
fn has_vertex(mesh: &TriangleMesh, x: f64, y: f64, z: f64) -> bool {
    mesh.vertices
        .iter()
        .any(|v| (v.x - x).abs() < 1e-12 && (v.y - y).abs() < 1e-12 && (v.z - z).abs() < 1e-12)
}

// The point `length` along the heading `degrees` counter-clockwise from east,
// from `from`.
fn along(from: Point2, degrees: f64, length: f64) -> Point2 {
    let radians = degrees * PI / 180.0;
    Point2::new(
        from.x + length * radians.cos(),
        from.y + length * radians.sin(),
    )
}

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn at(x: f64, y: f64, z: f64) -> PathPoint {
    PathPoint::new(p(x, y), z)
}

fn polygon(points: &[(f64, f64)]) -> Vec<Point2> {
    points.iter().map(|&(x, y)| p(x, y)).collect()
}

// A section 1 wide and `height` tall, centred on the line.
fn centred(height: f64) -> Vec<Point2> {
    polygon(&[(-0.5, 0.0), (0.5, 0.0), (0.5, height), (-0.5, height)])
}

fn square_loop(outer: f64, inner: f64) -> (Vec<Point2>, Vec<Point2>) {
    (
        polygon(&[
            (-outer, -outer),
            (outer, -outer),
            (outer, outer),
            (-outer, outer),
        ]),
        polygon(&[
            (-inner, -inner),
            (inner, -inner),
            (inner, inner),
            (-inner, inner),
        ]),
    )
}

mod road_object_mesh {
    use super::*;

    #[test]
    fn a_box_is_closed_and_encloses_its_three_sides_multiplied() {
        let mut mesh = TriangleMesh::default();
        add_box(
            &mut mesh,
            Point3::new(5.0, 5.0, 1.0),
            Vec2::new(1.0, 0.0),
            2.0,
            3.0,
            4.0,
        );
        assert!(is_closed_mesh(&mesh));
        assert_eq!(mesh.triangle_count(), 12);
        near(mesh_volume(&mesh), 2.0 * 3.0 * 4.0, 1e-12);
        let bounds = mesh.bounds();
        near(bounds.min.x, 4.0, 1e-12);
        near(bounds.max.x, 6.0, 1e-12);
        near(bounds.min.y, 3.5, 1e-12);
        near(bounds.max.y, 6.5, 1e-12);
        near(bounds.min.z, 1.0, 1e-12);
        near(bounds.max.z, 5.0, 1e-12);
    }

    #[test]
    fn a_volume_far_from_the_origin_keeps_its_digits() {
        // A box of 24 m3 at map coordinates of seven million metres: summed
        // about the origin, each term would be ~1e20 and the answer noise.
        let mut mesh = TriangleMesh::default();
        add_box(
            &mut mesh,
            Point3::new(500000.0, 7000000.0, 100.0),
            Vec2::new(0.6, 0.8),
            2.0,
            3.0,
            4.0,
        );
        assert!(is_closed_mesh(&mesh));
        near(mesh_volume(&mesh), 24.0, 1e-6);
    }

    #[test]
    fn a_concave_polygon_is_capped_into_its_area_with_two_fewer_triangles_than_corners() {
        // An inverted T - a retaining wall's stem on its footing: the footing
        // 1.2 x 0.4 and the stem 0.25 x 2.0 make 0.48 + 0.5 = 0.98. A prism of
        // it 2 high is 0.98 x 2 = 1.96 m3; each cap is 8 - 2 = 6 triangles
        // and the eight sides two each, 2 x 6 + 8 x 2 = 28.
        let tee = polygon(&[
            (-0.6, -0.4),
            (0.6, -0.4),
            (0.6, 0.0),
            (0.125, 0.0),
            (0.125, 2.0),
            (-0.125, 2.0),
            (-0.125, 0.0),
            (-0.6, 0.0),
        ]);
        let mut mesh = TriangleMesh::default();
        add_prism(&mut mesh, &tee, 0.0, 2.0);
        assert!(is_closed_mesh(&mesh));
        assert_eq!(mesh.triangle_count(), 28);
        near(mesh_volume(&mesh), 1.96, 1e-12);
    }

    #[test]
    fn a_clockwise_polygon_is_capped_as_its_counter_clockwise_twin_is() {
        // The unit square, 3 high, is 3 m3 outward whichever way its corners
        // run.
        let clockwise = polygon(&[(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0)]);
        let turned = counter_clockwise(&clockwise);
        assert_eq!(turned.len(), 4);
        assert_eq!(turned[0], p(1.0, 0.0));
        assert_eq!(counter_clockwise(&turned)[0], p(1.0, 0.0)); // already counter-clockwise
        for outline in [&clockwise, &turned] {
            let mut mesh = TriangleMesh::default();
            add_prism(&mut mesh, outline, 0.0, 3.0);
            assert!(is_closed_mesh(&mesh));
            near(mesh_volume(&mesh), 3.0, 1e-12);
        }
    }

    #[test]
    fn a_polygon_that_cannot_be_capped_adds_no_solid_at_all() {
        let cannot = [
            polygon(&[(0.0, 0.0), (1.0, 1.0), (1.0, 0.0), (0.0, 1.0)]), // a bow-tie: its two halves cancel
            polygon(&[(0.0, 0.0), (1.0, 1.0)]),                         // two corners
            Vec::new(),                                                 // none
            polygon(&[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)]),             // all on one line: no area
            polygon(&[(1.0, 1.0), (1.0, 1.0), (1.0, 1.0)]),             // one point three times
            polygon(&[(0.0, 0.0), (1.0, 0.0), (f64::NAN, 1.0)]),        // not finite
            polygon(&[(0.0, 0.0), (1.0, 0.0), (f64::INFINITY, 1.0)]),
        ];
        let path = [at(0.0, 0.0, 0.0), at(5.0, 0.0, 0.0)];
        for (i, outline) in cannot.iter().enumerate() {
            let mut prism = TriangleMesh::default();
            add_prism(&mut prism, outline, 0.0, 1.0);
            assert!(prism.vertices.is_empty(), "polygon {i}");
            assert!(prism.faces.is_empty(), "polygon {i}");
            let mut swept = TriangleMesh::default();
            add_sweep(&mut swept, &path, outline);
            assert!(swept.vertices.is_empty(), "section {i}");
            assert!(swept.faces.is_empty(), "section {i}");
        }
    }

    #[test]
    fn a_straight_run_in_a_polygon_stays_in_the_cap_so_the_prism_closes() {
        // (1,0) lies on the bottom edge, a corner of no turn. The edges
        // (0,0)-(1,0) and (1,0)-(2,0) must both be in the cap for it to meet
        // the sides of a prism: five corners, three triangles a cap (two of
        // them the ears at (0,0) and (1,0), (0,2),(0,0),(1,0) = 1 and
        // (0,2),(1,0),(2,0) = 1, and (2,0),(2,2),(0,2) = 2 what is left), five
        // sides of two triangles each: 2 x 3 + 5 x 2 = 16, closed, and
        // 4 x 1 = 4 m3.
        let square = polygon(&[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]);
        let mut mesh = TriangleMesh::default();
        add_prism(&mut mesh, &square, 0.0, 1.0);
        assert!(is_closed_mesh(&mesh));
        assert_eq!(mesh.triangle_count(), 16);
        near(mesh_volume(&mesh), 4.0, 1e-12);
    }

    #[test]
    fn a_repeated_corner_still_closes_the_solid() {
        // (2,0) twice, as an edited section can have: a corner of no length.
        // The cap keeps both copies - the side quads stand on the edge
        // between them, of no width but still an edge - so the prism closes:
        // 2 x (5 - 2) + 5 x 2 = 16 triangles, 4 x 1 = 4 m3.
        let repeated = polygon(&[(0.0, 0.0), (2.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]);
        let mut mesh = TriangleMesh::default();
        add_prism(&mut mesh, &repeated, 0.0, 1.0);
        assert!(is_closed_mesh(&mesh));
        assert_eq!(mesh.triangle_count(), 16);
        near(mesh_volume(&mesh), 4.0, 1e-12);
    }

    #[test]
    fn an_extrusion_faces_outward_whichever_way_its_polygon_runs() {
        let mut mesh = TriangleMesh::default();
        let clockwise = polygon(&[(0.0, 0.0), (0.0, 2.0), (3.0, 2.0), (3.0, 0.0)]);
        add_extrusion(
            &mut mesh,
            &clockwise,
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            0.5,
        );
        assert!(is_closed_mesh(&mesh));
        // Outward: the volume is positive, 3 x 2 x 0.5.
        near(mesh_volume(&mesh), 3.0, 1e-12);
        // Extruded along u x v = (1,0,0) x (0,0,1) = (0,-1,0).
        near(mesh.bounds().min.y, -0.5, 1e-12);
        near(mesh.bounds().max.y, 0.0, 1e-12);
    }

    #[test]
    fn a_sweep_along_a_straight_path_is_its_section_times_its_length() {
        let mut mesh = TriangleMesh::default();
        let path = [at(0.0, 0.0, 10.0), at(4.0, 0.0, 10.0), at(10.0, 0.0, 10.0)];
        let section = polygon(&[(-0.25, 0.0), (0.25, 0.0), (0.25, 2.0), (-0.25, 2.0)]);
        add_sweep(&mut mesh, &path, &section);
        assert!(is_closed_mesh(&mesh));
        near(mesh_volume(&mesh), 0.5 * 2.0 * 10.0, 1e-9);
        near(mesh.bounds().min.z, 10.0, 1e-12);
        near(mesh.bounds().max.z, 12.0, 1e-12);
    }

    #[test]
    fn a_sweep_round_a_corner_is_mitred_and_keeps_its_width() {
        // (0,0) to (10,0) to (10,10): a left turn of 90 degrees. The section
        // 1 wide, 1 tall, centred on the line: volume 1 x 20.
        let mut mesh = TriangleMesh::default();
        let path = [at(0.0, 0.0, 0.0), at(10.0, 0.0, 0.0), at(10.0, 10.0, 0.0)];
        add_sweep(&mut mesh, &path, &centred(1.0));
        assert!(is_closed_mesh(&mesh));
        near(mesh_volume(&mesh), 20.0, 1e-9);
        // The outside of the corner is at (10.5, -0.5): half a unit from both
        // legs, where the mitre puts it, not at (10, -0.5).
        let bounds = mesh.bounds();
        near(bounds.max.x, 10.5, 1e-12);
        near(bounds.min.y, -0.5, 1e-12);
    }

    #[test]
    fn a_hollow_sweep_is_the_ring_between_its_loops_times_its_length() {
        let mut mesh = TriangleMesh::default();
        let path = [at(0.0, 0.0, 0.0), at(0.0, 5.0, 1.0)];
        let (outer, inner) = square_loop(1.0, 0.5);
        add_hollow_sweep(&mut mesh, &path, &outer, &inner);
        assert!(is_closed_mesh(&mesh));
        // The sections stand upright on a path rising 1 in 5, so the solid is
        // a sheared prism: its volume is the ring's area, 4 - 1, times the
        // plan length 5 (Cavalieri).
        near(mesh_volume(&mesh), 3.0 * 5.0, 1e-9);
    }

    #[test]
    fn a_hollow_sweep_refuses_loops_that_do_not_correspond() {
        let mut mesh = TriangleMesh::default();
        let path = [at(0.0, 0.0, 0.0), at(0.0, 5.0, 0.0)];
        let (outer, _) = square_loop(1.0, 0.5);
        add_hollow_sweep(
            &mut mesh,
            &path,
            &outer,
            &polygon(&[(0.0, 0.0), (0.5, 0.0), (0.0, 0.5)]),
        );
        assert!(mesh.empty());
    }

    #[test]
    fn a_sweep_of_one_point_makes_nothing() {
        let mut mesh = TriangleMesh::default();
        // Two points a tenth of a millimetre apart are one point.
        add_sweep(
            &mut mesh,
            &[at(0.0, 0.0, 0.0), at(0.0001, 0.0, 0.0)],
            &polygon(&[(-1.0, 0.0), (1.0, 0.0), (1.0, 1.0), (-1.0, 1.0)]),
        );
        assert!(mesh.empty());
        assert!(!is_closed_mesh(&mesh));
    }

    #[test]
    fn an_open_sheet_is_not_closed() {
        let mut mesh = TriangleMesh {
            vertices: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            faces: vec![[0, 1, 2]],
        };
        assert!(!is_closed_mesh(&mesh));
        // With its back face too, each edge is used once each way: a closed,
        // if flat, surface.
        mesh.faces.push([0, 2, 1]);
        assert!(is_closed_mesh(&mesh));
        // The same face twice uses each edge twice in one direction.
        mesh.faces = vec![[0, 1, 2], [0, 1, 2]];
        assert!(!is_closed_mesh(&mesh));
    }

    #[test]
    fn a_regular_polygons_corners_lie_on_its_circle() {
        let octagon = regular_polygon(p(3.0, 4.0), 2.0, 8, PI / 8.0);
        assert_eq!(octagon.len(), 8);
        for corner in &octagon {
            near(corner.distance_to(p(3.0, 4.0)), 2.0, 1e-12);
        }
        // Started at 22.5 degrees, its flats face the axes: the lowest corner
        // is 2 cos 22.5 below the centre.
        let mut lowest = 1e9;
        for corner in &octagon {
            lowest = cpp::min(lowest, corner.y);
        }
        near(lowest, 4.0 - 2.0 * (PI / 8.0).cos(), 1e-12);
        assert!(regular_polygon(p(0.0, 0.0), 1.0, 2, 0.0).is_empty());
    }

    // ---- the sweep past the mitre limit, and round a closed path ------------
    //
    // A mitre longer than four times the half-width (a turn of more than
    // about 151 degrees) used to be shortened, which narrowed the section
    // along the whole leg: the joint ring was 0.695 of full width at a 160
    // degree turn and the leg tapered to it. Past the limit the joint is now
    // two rings, square to the leg that arrives and the leg that leaves.

    #[test]
    fn a_sweep_round_a_turn_past_the_mitre_limit_keeps_its_full_width_along_both_legs() {
        // East 10 m, then 10 m on a heading of 160 degrees: a left turn of
        // 160. The mitre would be 1 / cos 80 = 5.76 times the half-width,
        // past the limit of 4. A section 1 wide and 1 tall, centred on the
        // line, keeps its area times the length of the line - 1 x 1 x 20 -
        // whatever the turn: the two lobes the joint makes, one each side of
        // the line, are equal and opposite.
        let end = along(p(10.0, 0.0), 160.0, 10.0);
        let path = [
            at(0.0, 0.0, 0.0),
            at(10.0, 0.0, 0.0),
            PathPoint::new(end, 0.0),
        ];
        let mut mesh = TriangleMesh::default();
        add_sweep(&mut mesh, &path, &centred(1.0));
        assert!(is_closed_mesh(&mesh));
        near(mesh_volume(&mesh), 20.0, 1e-9);
        // The first leg ends in a full-width ring square to it, at
        // (10, +-0.5); the second starts in one square to itself: from (10, 0)
        // along its left normal (-sin 160, cos 160) half a metre each way.
        assert!(has_vertex(&mesh, 10.0, 0.5, 0.0));
        assert!(has_vertex(&mesh, 10.0, -0.5, 0.0));
        let turn = 160.0 * PI / 180.0;
        assert!(has_vertex(
            &mesh,
            10.0 - 0.5 * turn.sin(),
            0.5 * turn.cos(),
            0.0
        ));
        assert!(has_vertex(
            &mesh,
            10.0 + 0.5 * turn.sin(),
            -0.5 * turn.cos(),
            0.0
        ));
        // Two rings at the joint and one at each end: four rings of four
        // corners.
        assert_eq!(mesh.vertices.len(), 16);
    }

    #[test]
    fn a_sweep_round_a_turn_just_inside_the_mitre_limit_is_still_one_mitred_ring() {
        // 150 degrees: 1 / cos 75 = 3.86, inside the limit. One ring at the
        // joint (three rings of four corners) and the same volume, 1 x 1 x
        // 20.
        let end = along(p(10.0, 0.0), 150.0, 10.0);
        let path = [
            at(0.0, 0.0, 0.0),
            at(10.0, 0.0, 0.0),
            PathPoint::new(end, 0.0),
        ];
        let mut mesh = TriangleMesh::default();
        add_sweep(&mut mesh, &path, &centred(1.0));
        assert!(is_closed_mesh(&mesh));
        assert_eq!(mesh.vertices.len(), 12);
        near(mesh_volume(&mesh), 20.0, 1e-9);
    }

    #[test]
    fn a_sweep_that_doubles_back_on_itself_keeps_its_width_to_the_turn() {
        // East 10 m and back 5: the normals cancel, there is no mitre at all.
        // 1 x 1 x 15, the joint's ring in two, square to each leg.
        let path = [at(0.0, 0.0, 0.0), at(10.0, 0.0, 0.0), at(5.0, 0.0, 0.0)];
        let mut mesh = TriangleMesh::default();
        add_sweep(&mut mesh, &path, &centred(1.0));
        assert!(is_closed_mesh(&mesh));
        near(mesh_volume(&mesh), 15.0, 1e-9);
        assert!(has_vertex(&mesh, 10.0, 0.5, 0.0));
        assert!(has_vertex(&mesh, 10.0, -0.5, 0.0));
    }

    #[test]
    fn a_hollow_sweep_round_a_hairpin_keeps_its_ring_and_stays_closed() {
        // The ring between a 2 x 2 and a 1 x 1 loop is 4 - 1 = 3, along 20 m.
        let end = along(p(10.0, 0.0), 160.0, 10.0);
        let path = [
            at(0.0, 0.0, 0.0),
            at(10.0, 0.0, 0.0),
            PathPoint::new(end, 0.0),
        ];
        let mut mesh = TriangleMesh::default();
        let (outer, inner) = square_loop(1.0, 0.5);
        add_hollow_sweep(&mut mesh, &path, &outer, &inner);
        assert!(is_closed_mesh(&mesh));
        near(mesh_volume(&mesh), 3.0 * 20.0, 1e-9);
    }

    #[test]
    fn a_path_that_returns_to_its_start_is_swept_as_a_loop_mitred_at_the_start_with_no_end_faces() {
        // A 10 m square, the first corner repeated at the end as a closed
        // polyline is stored. The section 1 wide and 2 tall, centred: the
        // band between a 11 m square and a 9 m one, (121 - 81) x 2 = 80. Four
        // rings of four corners and no end faces: 16 vertices, 4 x 4 x 2 = 32
        // triangles (an open sweep of five rings would have 36).
        let path = [
            at(0.0, 0.0, 0.0),
            at(10.0, 0.0, 0.0),
            at(10.0, 10.0, 0.0),
            at(0.0, 10.0, 0.0),
            at(0.0, 0.0, 0.0),
        ];
        let mut mesh = TriangleMesh::default();
        add_sweep(&mut mesh, &path, &centred(2.0));
        assert!(is_closed_mesh(&mesh));
        near(mesh_volume(&mesh), 80.0, 1e-9);
        assert_eq!(mesh.vertices.len(), 16);
        assert_eq!(mesh.triangle_count(), 32);
        // The start corner is mitred like the other three: the outside of it
        // at (-0.5, -0.5), the inside at (0.5, 0.5). It was two square end
        // faces, one at (0, +-0.5) and one at (+-0.5, 0).
        assert!(has_vertex(&mesh, -0.5, -0.5, 0.0));
        assert!(has_vertex(&mesh, 0.5, 0.5, 0.0));
        assert!(has_vertex(&mesh, -0.5, -0.5, 2.0));
    }

    #[test]
    fn a_section_off_centre_on_a_loop_has_the_area_between_the_line_and_its_offset() {
        // Section u from 0 to 1, left of a counter-clockwise square, which is
        // inside it: the band between the 10 m square and the 8 m one inside
        // it, 100 - 64 = 36, 1 tall. A notch at the start corner and an
        // overlap at the same corner, which the old end faces made, gave 37.
        let path = [
            at(0.0, 0.0, 0.0),
            at(10.0, 0.0, 0.0),
            at(10.0, 10.0, 0.0),
            at(0.0, 10.0, 0.0),
            at(0.0, 0.0, 0.0),
        ];
        let section = polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]);
        let mut mesh = TriangleMesh::default();
        add_sweep(&mut mesh, &path, &section);
        assert!(is_closed_mesh(&mesh));
        near(mesh_volume(&mesh), 36.0, 1e-9);
    }

    #[test]
    fn a_path_closed_midway_along_a_side_is_one_loop_with_no_coincident_end_faces() {
        // The same 40 m of square, begun and ended at (5, 0): five rings, the
        // first on a straight, 2 x 4 x 5 = 40 triangles and 1 x 2 x 40 = 80.
        let path = [
            at(5.0, 0.0, 0.0),
            at(10.0, 0.0, 0.0),
            at(10.0, 10.0, 0.0),
            at(0.0, 10.0, 0.0),
            at(0.0, 0.0, 0.0),
            at(5.0, 0.0, 0.0),
        ];
        let mut mesh = TriangleMesh::default();
        add_sweep(&mut mesh, &path, &centred(2.0));
        assert!(is_closed_mesh(&mesh));
        near(mesh_volume(&mesh), 80.0, 1e-9);
        assert_eq!(mesh.vertices.len(), 20);
        assert_eq!(mesh.triangle_count(), 40);
    }

    #[test]
    fn a_triangle_of_path_is_the_smallest_loop() {
        // 6, 10 and 8 round: 24 m. Every turn is inside the mitre limit (127,
        // 143 and 90 degrees), so three rings of four corners: 1 x 1 x 24.
        let path = [
            at(0.0, 0.0, 0.0),
            at(6.0, 0.0, 0.0),
            at(0.0, 8.0, 0.0),
            at(0.0, 0.0, 0.0),
        ];
        let mut mesh = TriangleMesh::default();
        add_sweep(&mut mesh, &path, &centred(1.0));
        assert!(is_closed_mesh(&mesh));
        assert_eq!(mesh.vertices.len(), 12);
        assert_eq!(mesh.triangle_count(), 24);
        near(mesh_volume(&mesh), 24.0, 1e-9);
    }

    #[test]
    fn a_path_that_ends_at_the_start_but_at_another_level_is_not_a_loop() {
        // Back above its first point, a metre higher: a spiral's turn, with
        // its two ends. Five rings, and the end faces (36 triangles), as
        // before.
        let path = [
            at(0.0, 0.0, 0.0),
            at(10.0, 0.0, 0.0),
            at(10.0, 10.0, 0.0),
            at(0.0, 10.0, 0.0),
            at(0.0, 0.0, 1.0),
        ];
        let mut mesh = TriangleMesh::default();
        add_sweep(&mut mesh, &path, &centred(1.0));
        assert!(is_closed_mesh(&mesh));
        assert_eq!(mesh.triangle_count(), 36);
    }

    #[test]
    fn a_line_drawn_out_and_back_is_not_a_loop_and_keeps_its_ends() {
        // (0,0) to (10,0) and back: three points, so no triangle of path to
        // wrap round. The turn is a full 180 degrees, split at the joint;
        // 1 x 1 x 20.
        let path = [at(0.0, 0.0, 0.0), at(10.0, 0.0, 0.0), at(0.0, 0.0, 0.0)];
        let mut mesh = TriangleMesh::default();
        add_sweep(&mut mesh, &path, &centred(1.0));
        assert!(is_closed_mesh(&mesh));
        near(mesh_volume(&mesh), 20.0, 1e-9);
    }
}
