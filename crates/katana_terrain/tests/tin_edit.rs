//! Twin of tests/terrain/test_tin_edit.cpp: editing a surface (tin_edit,
//! docs/surface_editing.md). Every surface here is small and made by hand, so
//! each expected count, area and level is worked out on paper in the comment
//! beside it.

#![allow(
    clippy::excessive_precision,
    clippy::approx_constant,
    reason = "Preserve decimal constants and expected values copied verbatim from the C++ oracle."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

pub mod support;

use std::collections::BTreeSet;

use katana_core::ErrorCode;
use katana_core::task_pool::TaskPool;
use katana_geometry::{Point2, Point3, Polyline2};
use katana_terrain::tin_edit::{
    add_breakline, add_breaklines, add_points, change_heights, clip_to_boundary, cut_hole,
    cut_holes, delete_breaklines, delete_points, delete_triangles, flip_edge, move_point,
    on_constrained_edge, paste_surface, smooth_surface, thin_surface, triangle_at,
    triangles_by_polygon, triangles_longer_than, triangles_thinner_than, vertex_at,
};
use katana_terrain::{
    Breakline, BreaklineInput, BreaklineVertex, HeightChange, HeightOperation, HeldPointRule,
    HoleCutStats, SmoothOptions, TinBuildOptions, TinEditResult, TinInput, TinSurface, TinTriangle,
    build_tin,
};
use support::*;

fn make(vertices: Vec<Point3>, triangles: Vec<TinTriangle>, masks: Vec<u8>) -> TinSurface {
    TinSurface::create(vertices, triangles, masks)
        .unwrap_or_else(|error| panic!("{}", error.describe()))
}

/// The square [0,10]^2 as two triangles split along the diagonal 0-2, the
/// corners at the levels given.
fn square_at(z0: f64, z1: f64, z2: f64, z3: f64, diagonal_is_breakline: bool) -> TinSurface {
    // Triangle {0,1,2} runs the diagonal as its edge 2 (2->0), {0,2,3} as
    // its edge 0 (0->2).
    let masks = if diagonal_is_breakline {
        vec![4, 1]
    } else {
        vec![]
    };
    make(
        vec![
            p3(0.0, 0.0, z0),
            p3(10.0, 0.0, z1),
            p3(10.0, 10.0, z2),
            p3(0.0, 10.0, z3),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
        masks,
    )
}

fn square() -> TinSurface {
    square_at(0.0, 0.0, 0.0, 0.0, false)
}

/// The square [0,10]^2 with a vertex at its centre at `apex`, the corners at
/// 0: four triangles meeting at the centre (a pyramid).
fn pyramid_at(apex: f64) -> TinSurface {
    make(
        vec![
            p3(0.0, 0.0, 0.0),
            p3(10.0, 0.0, 0.0),
            p3(10.0, 10.0, 0.0),
            p3(0.0, 10.0, 0.0),
            p3(5.0, 5.0, apex),
        ],
        vec![[0, 1, 4], [1, 2, 4], [2, 3, 4], [0, 4, 3]],
        vec![],
    )
}

fn pyramid() -> TinSurface {
    pyramid_at(10.0)
}

/// A grid of `cells` x `cells` squares of side `cell` from the origin, each
/// split along its rising diagonal, the levels from `level(x, y)`.
fn grid(cells: i32, cell: f64, level: impl Fn(f64, f64) -> f64) -> TinSurface {
    let mut vertices: Vec<Point3> = Vec::new();
    for j in 0..=cells {
        for i in 0..=cells {
            let x = f64::from(i) * cell;
            let y = f64::from(j) * cell;
            vertices.push(p3(x, y, level(x, y)));
        }
    }
    let at = |i: i32, j: i32| (j * (cells + 1) + i) as u32;
    let mut triangles: Vec<TinTriangle> = Vec::new();
    for j in 0..cells {
        for i in 0..cells {
            triangles.push([at(i, j), at(i + 1, j), at(i + 1, j + 1)]);
            triangles.push([at(i, j), at(i + 1, j + 1), at(i, j + 1)]);
        }
    }
    make(vertices, triangles, vec![])
}

fn flat_grid(cells: i32, cell: f64, z: f64) -> TinSurface {
    grid(cells, cell, |_, _| z)
}

fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64) -> Polyline2 {
    Polyline2::new(vec![p2(x0, y0), p2(x1, y0), p2(x1, y1), p2(x0, y1)], true)
}

fn polyline(points: &[(f64, f64)], closed: bool) -> Polyline2 {
    Polyline2::new(points.iter().map(|&(x, y)| p2(x, y)).collect(), closed)
}

/// The plan length of the surface's constrained edges, each counted once.
fn constrained_length(surface: &TinSurface) -> f64 {
    let mut seen: BTreeSet<(u32, u32)> = BTreeSet::new();
    let mut length = 0.0;
    for (t, tri) in surface.triangles().iter().enumerate() {
        for k in 0..3 {
            if !surface.is_edge_constrained(t, k) {
                continue;
            }
            let a = tri[k];
            let b = tri[(k + 1) % 3];
            if seen.insert((a.min(b), a.max(b))) {
                let p = surface.vertices()[a as usize];
                let q = surface.vertices()[b as usize];
                length += (q.x - p.x).hypot(q.y - p.y);
            }
        }
    }
    length
}

fn has_edge(surface: &TinSurface, a: u32, b: u32) -> bool {
    surface.triangles().iter().any(|tri| {
        (0..3).any(|k| {
            (tri[k] == a && tri[(k + 1) % 3] == b) || (tri[k] == b && tri[(k + 1) % 3] == a)
        })
    })
}

/// The vertex of `surface` at (x, y), or -1.
fn vertex_index(surface: &TinSurface, x: f64, y: f64) -> i32 {
    surface
        .vertices()
        .iter()
        .position(|v| v.x == x && v.y == y)
        .map_or(-1, |v| v as i32)
}

fn z_at(surface: &TinSurface, x: f64, y: f64) -> f64 {
    surface
        .elevation_at(Point2::new(x, y))
        .unwrap_or_else(|| panic!("no level at {x}, {y}"))
}

fn ok(result: katana_core::Result<TinEditResult>) -> TinEditResult {
    result.unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn code<T>(result: katana_core::Result<T>) -> Option<ErrorCode> {
    result.err().map(|error| error.code)
}

fn vertex(x: f64, y: f64, z: Option<f64>) -> BreaklineVertex {
    BreaklineVertex {
        position: p2(x, y),
        z,
    }
}

fn change(operation: HeightOperation, value: f64, base: Option<f64>) -> HeightChange {
    HeightChange {
        operation,
        value,
        base,
    }
}

fn smooth(passes: i32, weight: f64) -> SmoothOptions {
    SmoothOptions { passes, weight }
}

/// A 4 x 4 grid of unit cells on z = 0.5 x + 0.25 y (exact in binary), the
/// vertex (2,2) raised by 1.
fn bumped_plane() -> TinSurface {
    grid(4, 1.0, |x, y| {
        0.5 * x + 0.25 * y + if x == 2.0 && y == 2.0 { 1.0 } else { 0.0 }
    })
}

/// Calls `visit(x, y)` for x and y from `start` below `end` in steps of `step`,
/// as the C++ loops `for (double x = start; x < end; x += step)` do.
fn sweep(start: f64, end: f64, step: f64, mut visit: impl FnMut(f64, f64)) {
    let mut x = start;
    while x < end {
        let mut y = start;
        while y < end {
            visit(x, y);
            y += step;
        }
        x += step;
    }
}

mod tin_edit {
    use super::*;

    // ---- flipping ------------------------------------------------------------------------------

    #[test]
    fn flipping_the_diagonal_of_a_square_of_two_triangles_gives_the_other_diagonal() {
        // Corners 1 and 3 at 1, corners 0 and 2 at 0: along the diagonal 0-2 the
        // centre is at 0, along the other diagonal 1-3 it is at 1.
        let before = square_at(0.0, 1.0, 0.0, 1.0, false);
        near(z_at(&before, 5.0, 5.0), 0.0, 1e-12);
        let flipped = ok(flip_edge(&before, 0, 2)); // triangle {0,1,2}, edge 2->0
        let after = &flipped.surface;
        assert_eq!(after.triangle_count(), 2);
        assert_eq!(after.vertex_count(), 4);
        assert!(has_edge(after, 1, 3));
        assert!(!has_edge(after, 0, 2));
        near(z_at(after, 5.0, 5.0), 1.0, 1e-12);
        near(after.plan_area(), 100.0, 1e-9);
    }

    #[test]
    fn flipping_an_edge_on_the_rim_is_refused() {
        let error = flip_edge(&square(), 0, 0).expect_err("refused"); // 0->1, the bottom side
        assert_eq!(error.code, ErrorCode::InvalidGeometry);
        assert!(error.message.contains("rim"));
    }

    #[test]
    fn flipping_a_breakline_is_refused() {
        let error = flip_edge(&square_at(0.0, 0.0, 0.0, 0.0, true), 0, 2).expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidGeometry);
        assert!(error.message.contains("breakline"));
    }

    #[test]
    fn flipping_where_the_quadrilateral_is_not_convex_is_refused() {
        // P(0,0) Q(10,0) R(5,5) above, S(12,-1) below and beyond Q: the quad
        // P S Q R bends inwards at Q, so the diagonal R-S passes outside it.
        // The triangle S, Q, R turns clockwise:
        // (Q - S) x (R - S) = (-2,1) x (-7,6) = -12 + 7 = -5.
        let dart = make(
            vec![
                p3(0.0, 0.0, 0.0),
                p3(10.0, 0.0, 0.0),
                p3(5.0, 5.0, 0.0),
                p3(12.0, -1.0, 0.0),
            ],
            vec![[0, 1, 2], [1, 0, 3]],
            vec![],
        );
        let error = flip_edge(&dart, 0, 0).expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidGeometry);
        assert!(error.message.contains("convex"));
    }

    #[test]
    fn flipping_an_edge_that_does_not_exist_is_refused() {
        assert_eq!(
            code(flip_edge(&square(), 2, 0)),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            code(flip_edge(&square(), 0, 3)),
            Some(ErrorCode::InvalidArgument)
        );
    }

    // ---- adding points ---------------------------------------------------------------------------

    #[test]
    fn a_point_added_in_the_middle_of_a_square_gives_four_triangles_at_the_right_heights() {
        // The centre lies on the diagonal, so both triangles are split: four
        // triangles meeting at (5,5,10). (5,2.5) is 0.25 (0,0) + 0.25 (10,0) +
        // 0.5 (5,5), so its level is 0.5 * 10 = 5.
        let added = ok(add_points(&square(), &[p3(5.0, 5.0, 10.0)]));
        let after = &added.surface;
        assert_eq!(after.triangle_count(), 4);
        assert_eq!(after.vertex_count(), 5);
        assert_eq!(added.report.points_added, 1);
        assert_eq!(added.report.triangles_before, 2);
        assert_eq!(added.report.triangles_after, 4);
        double_eq(z_at(after, 5.0, 5.0), 10.0);
        near(z_at(after, 5.0, 2.5), 5.0, 1e-12);
        near(z_at(after, 2.5, 2.5), 5.0, 1e-12);
        near(after.plan_area(), 100.0, 1e-9);
    }

    #[test]
    fn a_point_inside_a_triangle_is_inserted_where_it_lies() {
        // (7,3) is inside {0,1,2}; the triangle across the diagonal has (7,3)
        // inside its circumcircle (centre (5,5), radius 7.07; |(7,3)-(5,5)| =
        // 2.83), so both are rebuilt: five points, four triangles.
        let added = ok(add_points(&square(), &[p3(7.0, 3.0, 2.0)]));
        assert_eq!(added.surface.triangle_count(), 4);
        double_eq(z_at(&added.surface, 7.0, 3.0), 2.0);
    }

    #[test]
    fn a_point_at_an_existing_vertex_gives_it_its_level_and_changes_no_triangle() {
        let before = square();
        let added = ok(add_points(&before, &[p3(10.0, 10.0, 7.0)]));
        assert_eq!(added.surface.triangles(), before.triangles());
        assert_eq!(added.report.points_relevelled, 1);
        assert_eq!(added.report.points_added, 0);
        double_eq(z_at(&added.surface, 10.0, 10.0), 7.0);
    }

    #[test]
    fn a_point_off_the_surface_extends_it_by_the_triangles_joining_it_to_the_rim() {
        // (15,5) sees only the side x = 10: one new triangle (10,0) (15,5)
        // (10,10) of base 10 and height 5, area 25.
        let added = ok(add_points(&square(), &[p3(15.0, 5.0, 3.0)]));
        assert_eq!(added.surface.triangle_count(), 3);
        near(added.surface.plan_area(), 125.0, 1e-9);
        double_eq(z_at(&added.surface, 15.0, 5.0), 3.0);
        // Halfway from the side to the point: half its level.
        near(z_at(&added.surface, 12.5, 5.0), 1.5, 1e-12);
    }

    #[test]
    fn a_point_on_a_breakline_splits_it_and_keeps_its_own_level() {
        // The diagonal is a breakline; (5,5) on it splits it in two, both halves
        // still constrained: the constrained length stays the diagonal's 10 sqrt 2.
        let added = ok(add_points(
            &square_at(0.0, 0.0, 0.0, 0.0, true),
            &[p3(5.0, 5.0, 9.0)],
        ));
        assert_eq!(added.surface.triangle_count(), 4);
        double_eq(z_at(&added.surface, 5.0, 5.0), 9.0);
        near(
            constrained_length(&added.surface),
            10.0 * 2.0f64.sqrt(),
            1e-9,
        );
    }

    #[test]
    fn two_points_at_one_position_with_different_levels_are_refused() {
        let added = add_points(&square(), &[p3(3.0, 3.0, 1.0), p3(3.0, 3.0, 2.0)]);
        assert_eq!(code(added), Some(ErrorCode::InvalidGeometry));
    }

    #[test]
    fn adding_no_point_or_a_non_finite_one_is_refused() {
        assert_eq!(
            code(add_points(&square(), &[])),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            code(add_points(&square(), &[p3(1.0, f64::NAN, 0.0)])),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            code(add_points(&TinSurface::new(), &[p3(1.0, 1.0, 0.0)])),
            Some(ErrorCode::InvalidArgument)
        );
    }

    // ---- deleting ----------------------------------------------------------------------------------

    #[test]
    fn deleting_the_centre_of_a_pyramid_gives_the_flat_square_back() {
        let before = pyramid();
        let deleted = ok(delete_points(&before, &[4], HeldPointRule::Refuse));
        assert_eq!(deleted.surface.triangle_count(), 2);
        assert_eq!(deleted.surface.vertex_count(), 4);
        assert_eq!(deleted.report.points_removed, 1);
        double_eq(deleted.surface.max_elevation(), 0.0);
        near(deleted.surface.plan_area(), 100.0, 1e-9);
    }

    #[test]
    fn a_point_on_a_breakline_is_refused_alone_and_held_inside_a_polygon() {
        // The pyramid with its spoke 0-4 a breakline: {0,1,4} runs it as edge 2
        // (4->0), {0,4,3} as edge 0 (0->4).
        let spoked = make(
            vec![
                p3(0.0, 0.0, 0.0),
                p3(10.0, 0.0, 0.0),
                p3(10.0, 10.0, 0.0),
                p3(0.0, 10.0, 0.0),
                p3(5.0, 5.0, 10.0),
            ],
            vec![[0, 1, 4], [1, 2, 4], [2, 3, 4], [0, 4, 3]],
            vec![4, 0, 0, 1],
        );
        assert_eq!(
            code(delete_points(&spoked, &[4], HeldPointRule::Refuse)),
            Some(ErrorCode::InvalidGeometry)
        );
        let held = ok(delete_points(&spoked, &[4], HeldPointRule::Skip));
        assert_eq!(held.surface, spoked);
        assert_eq!(held.report.points_held, 1);
    }

    #[test]
    fn deleting_a_corner_of_the_rim_never_grows_the_footprint() {
        // A 2 x 2 grid of unit cells, 8 triangles, area 4. The corner (0,0)
        // belongs to both triangles of its cell; without it the cell keeps only
        // the triangle (1,0) (1,1) (0,1): 7 triangles, area 3.5.
        let before = flat_grid(2, 1.0, 0.0);
        let deleted = ok(delete_points(&before, &[0], HeldPointRule::Refuse));
        assert_eq!(deleted.surface.triangle_count(), 7);
        near(deleted.surface.plan_area(), 3.5, 1e-12);
        assert!(deleted.surface.elevation_at_xy(0.2, 0.2).is_none());
    }

    #[test]
    fn deleting_every_point_is_refused_for_leaving_nothing() {
        assert_eq!(
            code(delete_points(
                &square(),
                &[0, 1, 2, 3],
                HeldPointRule::Refuse
            )),
            Some(ErrorCode::InvalidState)
        );
    }

    #[test]
    fn deleting_a_point_out_of_range_is_refused() {
        assert_eq!(
            code(delete_points(&square(), &[9], HeldPointRule::Refuse)),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            code(delete_points(&square(), &[], HeldPointRule::Refuse)),
            Some(ErrorCode::InvalidArgument)
        );
    }

    #[test]
    fn deleting_triangles_removes_exactly_those() {
        // A 4 x 4 grid of unit cells: 32 triangles, area 16. The triangle under
        // (0.6, 0.2) is the lower half of the first cell, area 0.5.
        let before = flat_grid(4, 1.0, 0.0);
        let under = triangle_at(&before, p2(0.6, 0.2)).expect("a triangle");
        let deleted = ok(delete_triangles(&before, &[under]));
        assert_eq!(deleted.surface.triangle_count(), 31);
        near(deleted.surface.plan_area(), 15.5, 1e-12);
        assert!(deleted.surface.elevation_at_xy(0.6, 0.2).is_none());
        assert!(deleted.surface.elevation_at_xy(0.2, 0.6).is_some());
    }

    #[test]
    fn triangles_inside_and_outside_a_polygon_are_taken_by_their_centroids() {
        // The 4 x 4 grid: cell (i, j)'s triangles have their centroids at
        // (i + 2/3, j + 1/3) and (i + 1/3, j + 2/3). Inside [0.9, 2.1]^2 a
        // coordinate i + 1/3 or i + 2/3 lies only for i = 1, so only cell
        // (1, 1)'s two triangles are inside: 2 inside, 30 outside.
        let before = flat_grid(4, 1.0, 0.0);
        let polygon = rectangle(0.9, 0.9, 2.1, 2.1);
        assert_eq!(triangles_by_polygon(&before, &polygon, true).len(), 2);
        assert_eq!(triangles_by_polygon(&before, &polygon, false).len(), 30);
    }

    #[test]
    fn long_and_thin_triangles_are_found_by_their_sides_and_angles() {
        // A unit cell's triangles have sides 1, 1 and sqrt 2 and angles 45, 45
        // and 90 degrees.
        let cells = flat_grid(2, 1.0, 0.0);
        let degree = std::f64::consts::PI / 180.0;
        assert_eq!(triangles_longer_than(&cells, 1.5).len(), 0);
        assert_eq!(triangles_longer_than(&cells, 1.4).len(), 8);
        assert_eq!(triangles_thinner_than(&cells, 44.0 * degree).len(), 0);
        assert_eq!(triangles_thinner_than(&cells, 46.0 * degree).len(), 8);
    }

    #[test]
    fn deleting_every_triangle_is_refused() {
        assert_eq!(
            code(delete_triangles(&square(), &[0, 1])),
            Some(ErrorCode::InvalidState)
        );
        assert_eq!(
            code(delete_triangles(&square(), &[5])),
            Some(ErrorCode::InvalidArgument)
        );
    }

    // ---- breaklines -----------------------------------------------------------------------------------

    #[test]
    fn a_breakline_across_a_square_becomes_constrained_edges_at_its_levels() {
        // From (0,5) to (10,5) at 2: the lower half is the plane through (0,0,0),
        // (10,0,0) and (10,5,2), z = 0.4 y, so (5,2.5) is at 1.
        let line = [vertex(0.0, 5.0, Some(2.0)), vertex(10.0, 5.0, Some(2.0))];
        let added = ok(add_breakline(&square(), &line, false));
        let after = &added.surface;
        near(constrained_length(after), 10.0, 1e-9);
        near(z_at(after, 5.0, 5.0), 2.0, 1e-12);
        near(z_at(after, 5.0, 2.5), 1.0, 1e-12);
        near(z_at(after, 5.0, 0.0), 0.0, 1e-12);
        near(after.plan_area(), 100.0, 1e-9);
    }

    #[test]
    fn a_draped_breakline_keeps_the_shape_of_the_surface() {
        // y = 2.5 across the pyramid crosses the spokes 0-4 and 1-4 at (2.5,2.5)
        // and (7.5,2.5), halfway up them: level 5. The surface must be what it
        // was everywhere; only the edges are new.
        let before = pyramid();
        let line = [vertex(0.0, 2.5, None), vertex(10.0, 2.5, None)];
        let added = ok(add_breakline(&before, &line, false));
        let after = &added.surface;
        assert_eq!(added.report.crossings, 2);
        near(constrained_length(after), 10.0, 1e-9);
        near(z_at(after, 2.5, 2.5), 5.0, 1e-12);
        sweep(0.25, 10.0, 0.5, |x, y| {
            near(z_at(after, x, y), z_at(&before, x, y), 1e-9);
        });
    }

    #[test]
    fn a_breakline_crossing_a_breakline_meets_it_at_its_level() {
        let first = [vertex(0.0, 5.0, Some(2.0)), vertex(10.0, 5.0, Some(2.0))];
        let one = ok(add_breakline(&square(), &first, false));
        // The second runs at 0 from end to end, but where it crosses the first
        // the first's level, 2, holds.
        let second = [vertex(5.0, 0.0, Some(0.0)), vertex(5.0, 10.0, Some(0.0))];
        let two = ok(add_breakline(&one.surface, &second, false));
        assert_eq!(two.report.crossings, 1);
        near(z_at(&two.surface, 5.0, 5.0), 2.0, 1e-12);
        near(constrained_length(&two.surface), 20.0, 1e-9);
    }

    #[test]
    fn a_breakline_off_the_surface_or_across_a_hole_is_refused() {
        let off = [vertex(5.0, 5.0, Some(1.0)), vertex(20.0, 5.0, Some(1.0))];
        assert_eq!(
            code(add_breakline(&square(), &off, false)),
            Some(ErrorCode::InvalidGeometry)
        );

        let input = TinInput {
            points: vec![
                p3(0.0, 0.0, 0.0),
                p3(10.0, 0.0, 0.0),
                p3(10.0, 10.0, 0.0),
                p3(0.0, 10.0, 0.0),
            ],
            holes: vec![rectangle(4.0, 4.0, 6.0, 6.0)],
            ..TinInput::default()
        };
        let holed = build_tin(&input, &TinBuildOptions::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let across = [vertex(1.0, 5.0, None), vertex(9.0, 5.0, None)];
        let error = add_breakline(&holed.surface, &across, false).expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidGeometry);
        assert!(error.message.contains("leaves the surface"));
    }

    #[test]
    fn a_breakline_with_too_few_vertices_or_no_length_is_refused() {
        let one = [vertex(5.0, 5.0, Some(1.0))];
        assert_eq!(
            code(add_breakline(&square(), &one, false)),
            Some(ErrorCode::InvalidArgument)
        );
        let none = [vertex(5.0, 5.0, Some(1.0)), vertex(5.0, 5.0, Some(1.0))];
        assert_eq!(
            code(add_breakline(&square(), &none, false)),
            Some(ErrorCode::InvalidGeometry)
        );
    }

    // ---- boundaries and holes -------------------------------------------------------------------------

    #[test]
    fn a_boundary_clips_exactly_and_keeps_the_shape_inside() {
        // [2,8]^2 out of the pyramid: area 36, the rim its 24 m perimeter, all of
        // it constrained, and every level inside what it was.
        let before = pyramid();
        let clipped = ok(clip_to_boundary(&before, &rectangle(2.0, 2.0, 8.0, 8.0)));
        let after = &clipped.surface;
        near(after.plan_area(), 36.0, 1e-9);
        near(constrained_length(after), 24.0, 1e-9);
        assert!(after.elevation_at_xy(1.0, 1.0).is_none());
        let mut x = 2.25;
        while x < 8.0 {
            let mut y = 2.25;
            while y < 8.0 {
                near(z_at(after, x, y), z_at(&before, x, y), 1e-9);
                y += 0.5;
            }
            x += 0.5;
        }
    }

    #[test]
    fn a_hole_removes_exactly_its_area() {
        // A 10 x 10 grid of unit cells less [3,6] x [3,7]: 100 - 12 = 88.
        let before = flat_grid(10, 1.0, 4.0);
        let holed = ok(cut_hole(&before, &rectangle(3.0, 3.0, 6.0, 7.0)));
        near(holed.surface.plan_area(), 88.0, 1e-9);
        assert!(holed.surface.elevation_at_xy(4.5, 5.0).is_none());
        double_eq(z_at(&holed.surface, 1.0, 1.0), 4.0);
    }

    #[test]
    fn a_hole_not_on_the_grid_lines_is_cut_exactly() {
        // A tilted plane, z = 0.5 x + 0.25 y, and a hole [2.3, 4.6] x [1.2, 3.9]
        // that cuts through cells: area 100 - 2.3 * 2.7 = 93.79.
        let before = grid(10, 1.0, |x, y| 0.5 * x + 0.25 * y);
        let holed = ok(cut_hole(&before, &rectangle(2.3, 1.2, 4.6, 3.9)));
        near(holed.surface.plan_area(), 100.0 - 2.3 * 2.7, 1e-9);
        // The new rim's vertices lie on the plane.
        for v in holed.surface.vertices() {
            near(v.z, 0.5 * v.x + 0.25 * v.y, 1e-12);
        }
    }

    #[test]
    fn a_boundary_around_the_whole_surface_changes_nothing() {
        let before = pyramid();
        let clipped = ok(clip_to_boundary(
            &before,
            &rectangle(-5.0, -5.0, 15.0, 15.0),
        ));
        assert_eq!(clipped.surface, before);
    }

    #[test]
    fn a_boundary_beside_the_surface_is_refused_for_leaving_nothing() {
        assert_eq!(
            code(clip_to_boundary(
                &pyramid(),
                &rectangle(20.0, 20.0, 30.0, 30.0)
            )),
            Some(ErrorCode::InvalidState)
        );
    }

    #[test]
    fn a_polygon_that_crosses_itself_or_has_no_area_is_refused() {
        // (9,2)-(2,9) and (8,8)-(1,1) cross at (5.5, 5.5), on the surface.
        let crossing = polyline(&[(1.0, 1.0), (9.0, 2.0), (2.0, 9.0), (8.0, 8.0)], false);
        assert_eq!(
            code(cut_hole(&flat_grid(10, 1.0, 0.0), &crossing)),
            Some(ErrorCode::InvalidGeometry)
        );

        let flat = polyline(&[(1.0, 1.0), (5.0, 1.0), (9.0, 1.0)], false);
        assert_eq!(
            code(cut_hole(&flat_grid(10, 1.0, 0.0), &flat)),
            Some(ErrorCode::InvalidGeometry)
        );
        let two = polyline(&[(1.0, 1.0), (5.0, 5.0)], false);
        assert_eq!(
            code(clip_to_boundary(&flat_grid(10, 1.0, 0.0), &two)),
            Some(ErrorCode::InvalidGeometry)
        );
    }

    // ---- levels ---------------------------------------------------------------------------------------

    #[test]
    fn raising_by_one_adds_exactly_one_to_every_height() {
        let before = grid(4, 2.5, |x, y| 0.3 * x + 0.2 * y + 17.1);
        let raised = ok(change_heights(
            &before,
            &change(HeightOperation::Raise, 1.0, None),
            None,
        ));
        assert_eq!(raised.surface.vertex_count(), before.vertex_count());
        for (after, original) in raised.surface.vertices().iter().zip(before.vertices()) {
            assert_eq!(after.z, original.z + 1.0);
        }
        assert_eq!(raised.surface.triangles(), before.triangles());
        assert_eq!(raised.report.points_moved, before.vertex_count());
    }

    #[test]
    fn a_height_change_inside_a_polygon_moves_only_the_vertices_inside() {
        // A 4 x 4 grid of unit cells at 0: inside [0.5, 2.5]^2 lie the vertices
        // (1..2, 1..2), four of them.
        let before = flat_grid(4, 1.0, 0.0);
        let set = ok(change_heights(
            &before,
            &change(HeightOperation::SetLevel, 3.0, None),
            Some(&rectangle(0.5, 0.5, 2.5, 2.5)),
        ));
        assert_eq!(set.report.points_moved, 4);
        double_eq(z_at(&set.surface, 1.0, 1.0), 3.0);
        double_eq(z_at(&set.surface, 2.0, 2.0), 3.0);
        double_eq(z_at(&set.surface, 3.0, 3.0), 0.0);
        double_eq(z_at(&set.surface, 0.0, 0.0), 0.0);
    }

    #[test]
    fn scaling_stretches_heights_about_the_base() {
        // z = base + 2 (z - base): about 1, the level 3 goes to 5; with no base,
        // the lowest level (the corners at 0) stays and the apex 10 goes to 20.
        let about = ok(change_heights(
            &pyramid_at(3.0),
            &change(HeightOperation::Scale, 2.0, Some(1.0)),
            None,
        ));
        double_eq(z_at(&about.surface, 5.0, 5.0), 5.0);
        double_eq(z_at(&about.surface, 0.0, 0.0), -1.0);
        let lowest = ok(change_heights(
            &pyramid_at(10.0),
            &change(HeightOperation::Scale, 2.0, None),
            None,
        ));
        double_eq(z_at(&lowest.surface, 5.0, 5.0), 20.0);
        double_eq(z_at(&lowest.surface, 0.0, 0.0), 0.0);
    }

    #[test]
    fn a_height_change_that_cannot_be_made_is_refused() {
        assert_eq!(
            code(change_heights(
                &pyramid(),
                &change(HeightOperation::Raise, f64::NAN, None),
                None
            )),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            code(change_heights(
                &pyramid(),
                &change(HeightOperation::Raise, 1.0, None),
                Some(&rectangle(20.0, 20.0, 30.0, 30.0))
            )),
            Some(ErrorCode::InvalidArgument)
        );
    }

    #[test]
    fn one_smoothing_pass_moves_a_vertex_towards_the_mean_of_its_neighbours() {
        // The apex at 10 has the four corners, all on the rim and held, as its
        // neighbours: mean 0. Weight 1 puts it at 0, weight 0.5 halfway, at 5.
        let all = ok(smooth_surface(&pyramid(), &smooth(1, 1.0), None));
        double_eq(z_at(&all.surface, 5.0, 5.0), 0.0);
        assert_eq!(all.report.points_moved, 1);
        assert_eq!(all.report.points_held, 4);
        let half = ok(smooth_surface(&pyramid(), &smooth(1, 0.5), None));
        double_eq(z_at(&half.surface, 5.0, 5.0), 5.0);
    }

    #[test]
    fn smoothing_holds_breakline_vertices() {
        // A 4 x 4 grid with a spike of 8 at (2,2) and a breakline along y = 1 at
        // the grid's level 0. The vertices on the breakline and on the rim must
        // not move; the spike must come down.
        let mut input = TinInput::default();
        for j in 0..=4 {
            for i in 0..=4 {
                let z = if i == 2 && j == 2 { 8.0 } else { 0.0 };
                input.points.push(p3(f64::from(i), f64::from(j), z));
            }
        }
        input.breaklines.push(Breakline {
            vertices: vec![p3(0.0, 1.0, 0.0), p3(4.0, 1.0, 0.0)],
            closed: false,
        });
        let built = build_tin(&input, &TinBuildOptions::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let smoothed = ok(smooth_surface(&built.surface, &smooth(3, 0.5), None));
        for i in 0..=4 {
            double_eq(z_at(&smoothed.surface, f64::from(i), 1.0), 0.0);
        }
        assert!(z_at(&smoothed.surface, 2.0, 2.0) < 8.0);
        assert!(z_at(&smoothed.surface, 2.0, 3.0) > 0.0);
    }

    #[test]
    fn smoothing_options_out_of_range_are_refused() {
        for options in [smooth(0, 0.5), smooth(3, 0.0), smooth(3, 1.5)] {
            assert_eq!(
                code(smooth_surface(&pyramid(), &options, None)),
                Some(ErrorCode::InvalidArgument)
            );
        }
    }

    // ---- thinning ----------------------------------------------------------------------------------------

    #[test]
    fn thinning_with_tolerance_zero_removes_only_the_points_that_change_nothing() {
        // The interior vertices are (1..3, 1..3). The bump (2,2) and its six
        // neighbours along the grid's edges - (1,1) (2,1) (1,2) (3,2) (2,3) (3,3)
        // - sit where the surface bends; the other two, (1,3) and (3,1), have
        // every neighbour on the plane and are the only ones that can go.
        let before = bumped_plane();
        let thinned = ok(thin_surface(&before, 0.0, None));
        assert_eq!(thinned.report.points_removed, 2);
        assert_eq!(thinned.surface.vertex_count(), 23);
        assert_eq!(vertex_index(&thinned.surface, 1.0, 3.0), -1);
        assert_eq!(vertex_index(&thinned.surface, 3.0, 1.0), -1);
        assert_ne!(vertex_index(&thinned.surface, 2.0, 2.0), -1);
        sweep(0.1, 4.0, 0.2, |x, y| {
            near(z_at(&thinned.surface, x, y), z_at(&before, x, y), 1e-9);
        });
    }

    #[test]
    fn thinning_stays_within_its_tolerance_of_the_original_everywhere() {
        // A rolling surface: the bound is checked at a dense sample, not only at
        // the vertices removed.
        let before = grid(12, 1.0, |x, y| {
            0.3 * (0.7 * x).sin() + 0.2 * (0.5 * y).cos() + 0.01 * x * y
        });
        let tolerance = 0.1;
        let thinned = ok(thin_surface(&before, tolerance, None));
        assert!(thinned.report.points_removed > 0);
        assert!(thinned.report.largest_change <= tolerance + 1e-7);
        sweep(0.05, 12.0, 0.1, |x, y| {
            assert!(
                (z_at(&thinned.surface, x, y) - z_at(&before, x, y)).abs() <= tolerance + 1e-7,
                "at {x}, {y}"
            );
        });
        near(thinned.surface.plan_area(), before.plan_area(), 1e-9);
    }

    #[test]
    fn thinning_gives_the_same_surface_at_any_thread_count() {
        let before = grid(10, 1.0, |x, y| 0.2 * x.sin() + 0.1 * (1.3 * y).cos());
        let inline0 = TaskPool::new(0);
        let four = TaskPool::new(3);
        let one = ok(thin_surface(&before, 0.02, Some(&inline0)));
        let many = ok(thin_surface(&before, 0.02, Some(&four)));
        assert_eq!(one.surface, many.surface);
    }

    #[test]
    fn thinning_refuses_a_negative_or_non_finite_tolerance_and_an_empty_surface() {
        assert_eq!(
            code(thin_surface(&pyramid(), -0.1, None)),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            code(thin_surface(&pyramid(), f64::NAN, None)),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            code(thin_surface(&TinSurface::new(), 0.1, None)),
            Some(ErrorCode::InvalidArgument)
        );
    }

    #[test]
    fn an_edit_leaves_the_surface_it_was_given_as_it_was() {
        let before = pyramid();
        let copy = before.clone();
        let _ = add_points(&before, &[p3(2.0, 2.0, 1.0)]);
        let _ = cut_hole(&before, &rectangle(4.0, 4.0, 6.0, 6.0));
        let _ = smooth_surface(&before, &SmoothOptions::default(), None);
        assert_eq!(before, copy);
    }

    // ---- junctions within rounding of an existing edge ------------------------------------------
    //
    // A point decimal arithmetic puts on a line is a few ulps off it. The kernel
    // decides only exactly, so such a point does not split the edge it is "on",
    // and a new edge that ends there crosses the old one a few ulps from its own
    // end - where CGAL moves a crossing it cannot place to the end of the edge it
    // crosses, which can be tens of metres away. Coincidence is
    // tolerance::GEOMETRIC (1e-7): the old edge is bent through the point.

    /// A flat square [0,100]^2 at 0 with a 3D breakline from (0,0,30) to
    /// (100,30,30): the line y = 0.3 x, one long edge of the surface.
    fn square_with_an_old_breakline() -> TinSurface {
        let square = make(
            vec![
                p3(0.0, 0.0, 0.0),
                p3(100.0, 0.0, 0.0),
                p3(100.0, 100.0, 0.0),
                p3(0.0, 100.0, 0.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
            vec![],
        );
        let line = [
            vertex(0.0, 0.0, Some(30.0)),
            vertex(100.0, 30.0, Some(30.0)),
        ];
        ok(add_breakline(&square, &line, false)).surface
    }

    /// How many distinct constrained edges end at vertex `v`.
    fn constrained_degree(surface: &TinSurface, v: i32) -> i32 {
        let mut edges: BTreeSet<(u32, u32)> = BTreeSet::new();
        for (t, tri) in surface.triangles().iter().enumerate() {
            for k in 0..3 {
                let a = tri[k];
                let b = tri[(k + 1) % 3];
                if surface.is_edge_constrained(t, k) && (a as i32 == v || b as i32 == v) {
                    edges.insert((a.min(b), a.max(b)));
                }
            }
        }
        edges.len() as i32
    }

    fn smallest_triangle_area(surface: &TinSurface) -> f64 {
        let mut smallest = 1e300;
        for t in 0..surface.triangle_count() {
            let area = surface.plan_triangle(t).signed_area();
            if area < smallest {
                smallest = area;
            }
        }
        smallest
    }

    #[test]
    fn a_breakline_ending_within_rounding_of_an_old_breakline_splits_it_and_keeps_its_level_on_either_side()
     {
        // (14.3, 4.29) is 1.8e-14 below the line y = 0.3 x (the cross product with
        // its direction, worked in exact rationals: -1.78e-14 / 104.4); (30.7, 9.21)
        // is 1.1e-13 / 104.4 above it; (10, 3) is exactly on it. All three are "on"
        // the old breakline to within GEOMETRIC, so each is a T-junction: the old
        // breakline runs (0,0) - end - (100,30) through it, the new one ends there.
        let old = square_with_an_old_breakline();
        for end in [p2(14.3, 4.29), p2(30.7, 9.21), p2(10.0, 3.0)] {
            let line = [
                vertex(40.0, 70.0, Some(35.0)),
                vertex(end.x, end.y, Some(35.0)),
            ];
            let added = ok(add_breakline(&old, &line, false));
            let after = &added.surface;
            let v = vertex_index(after, end.x, end.y);
            assert!(v >= 0, "the end is a vertex, where it was typed: {end:?}");
            // The two halves of the old breakline and the new line meet there.
            assert_eq!(constrained_degree(after, v), 3, "end at {end:?}");
            assert_eq!(
                added.report.crossings, 0,
                "a junction, not a crossing: {end:?}"
            );
            near(after.vertices()[v as usize].z, 35.0, 1e-12);
            // The new line is level at 35 from end to end (no ramp to the old level).
            for s in [0.25, 0.5, 0.75, 0.95] {
                near(
                    z_at(after, 40.0 + s * (end.x - 40.0), 70.0 + s * (end.y - 70.0)),
                    35.0,
                    1e-6,
                );
            }
            // The old breakline bent to 35 at the junction, as it does for an exact
            // one: halfway along either half it is half way between 30 and 35.
            near(z_at(after, end.x / 2.0, end.y / 2.0), 32.5, 1e-6);
            near(
                z_at(after, (end.x + 100.0) / 2.0, (end.y + 30.0) / 2.0),
                32.5,
                1e-6,
            );
            // No sliver of no width beside the old line.
            assert!(smallest_triangle_area(after) > 1e-3, "end at {end:?}");
            // Its length is kept: the old line is 104.4 m, the new one runs from (40,70).
            near(
                constrained_length(after),
                100.0f64.hypot(30.0) + (40.0 - end.x).hypot(70.0 - end.y),
                1e-6,
            );
        }
    }

    #[test]
    fn a_breakline_ending_clear_of_an_old_breakline_still_crosses_it() {
        // The same line ending 1e-3 below the old one (a thousand times
        // GEOMETRIC): a real crossing, at the old line's level 30. The new line
        // from (40,70) to (14.3, 4.289) meets y = 0.3 x where
        // 70 - 65.711 t = 12 - 7.71 t, t = 58 / 58.001.
        let old = square_with_an_old_breakline();
        let line = [
            vertex(40.0, 70.0, Some(35.0)),
            vertex(14.3, 4.289, Some(35.0)),
        ];
        let added = ok(add_breakline(&old, &line, false));
        assert_eq!(added.report.crossings, 1);
        let t = 58.0 / 58.001;
        near(
            z_at(
                &added.surface,
                40.0 + t * (14.3 - 40.0),
                70.0 + t * (4.289 - 70.0),
            ),
            30.0,
            1e-6,
        );
        let end = vertex_index(&added.surface, 14.3, 4.289);
        assert!(end >= 0);
        assert_eq!(
            constrained_degree(&added.surface, end),
            1,
            "the old line does not pass through it"
        );
    }

    #[test]
    fn a_decimal_polygon_with_an_edge_along_a_breakline_is_cut_exactly() {
        // A parallelogram A B C D with A, B on y = 0.3 x in decimal arithmetic
        // (14.3, 4.29 and 33.9, 10.17) and C, D three to the left and ten up. Its
        // area is |AB x AD| = 19.6 * 10 + 5.88 * 3 = 213.64; its edge AB lies along
        // the old breakline. Whether the cut was refused ("touches or crosses
        // itself", at the breakline's far end) depended on the last bit of A and B.
        let old = square_with_an_old_breakline();
        let polygon = polyline(
            &[(14.3, 4.29), (33.9, 10.17), (30.9, 20.17), (11.3, 14.29)],
            false,
        );
        let ab = (33.9f64 - 14.3).hypot(10.17 - 4.29);
        let bc = 3.0f64.hypot(10.0);
        let perimeter = 2.0 * (ab + bc);

        let clipped = ok(clip_to_boundary(&old, &polygon));
        near(clipped.surface.plan_area(), 213.64, 1e-6);
        // Its rim, with the breakline along AB being that edge (counted once).
        near(constrained_length(&clipped.surface), perimeter, 1e-6);

        let holed = ok(cut_hole(&old, &polygon));
        near(holed.surface.plan_area(), 10000.0 - 213.64, 1e-6);
        // The rim of the hole, and the rest of the breakline either side of AB.
        near(
            constrained_length(&holed.surface),
            perimeter + 100.0f64.hypot(30.0) - ab,
            1e-6,
        );
    }

    #[test]
    fn a_pad_with_an_edge_along_a_breakline_is_pasted_with_a_clean_seam() {
        let old = square_with_an_old_breakline();
        let pad = make(
            vec![
                p3(14.3, 4.29, 20.0),
                p3(33.9, 10.17, 20.0),
                p3(30.9, 20.17, 20.0),
                p3(11.3, 14.29, 20.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
            vec![],
        );
        let pasted = ok(paste_surface(&old, &pad));
        near(pasted.surface.plan_area(), 10000.0, 1e-6); // no gap and no overlap
        near(z_at(&pasted.surface, 24.0, 12.0), 20.0, 1e-9); // inside the pad
    }

    #[test]
    fn a_point_within_rounding_of_an_old_breakline_splits_it_and_keeps_its_own_level() {
        // ADD POINT at (14.3, 4.29, 35) on the old breakline of the finding above.
        let old = square_with_an_old_breakline();
        let added = ok(add_points(&old, &[p3(14.3, 4.29, 35.0)]));
        let v = vertex_index(&added.surface, 14.3, 4.29);
        assert!(v >= 0);
        assert_eq!(
            constrained_degree(&added.surface, v),
            2,
            "the breakline passes through it"
        );
        near(added.surface.vertices()[v as usize].z, 35.0, 1e-12);
        near(
            constrained_length(&added.surface),
            100.0f64.hypot(30.0),
            1e-6,
        );
    }

    // ---- a 3D breakline is straight in 3D through the vertices it passes -------------------------

    #[test]
    fn a_breakline_through_existing_vertices_takes_their_levels_from_the_line() {
        // A flat 10 x 10 grid of unit cells at 0 and the line (0.5,0.5,5) -
        // (9.5,9.5,14) along the rising diagonals y = x: it passes through the
        // vertices (1,1) ... (9,9) exactly. It rises 9 over 9, so (k,k) is at
        // 5 + (k - 0.5) = 4.5 + k, and half way between two (2.5) at 7. These
        // vertices kept 0 while only the two ends moved.
        let before = flat_grid(10, 1.0, 0.0);
        let line = [vertex(0.5, 0.5, Some(5.0)), vertex(9.5, 9.5, Some(14.0))];
        let added = ok(add_breakline(&before, &line, false));
        for k in 1..=9 {
            let k = f64::from(k);
            near(z_at(&added.surface, k, k), 4.5 + k, 1e-9);
        }
        near(z_at(&added.surface, 0.5, 0.5), 5.0, 1e-9);
        near(z_at(&added.surface, 9.5, 9.5), 14.0, 1e-9);
        near(z_at(&added.surface, 2.5, 2.5), 7.0, 1e-9);
        // The nine vertices are the surface's, relevelled; the two ends are new.
        assert_eq!(added.report.points_relevelled, 9);
        assert_eq!(added.report.points_added, 2);
        assert_eq!(added.report.crossings, 0);
    }

    #[test]
    fn a_vertex_on_an_old_breakline_keeps_its_level_when_a_new3_d_line_passes_through() {
        // The row y = 5 made a breakline at 3 (its vertices (1,5) ... (9,5) are on
        // it and at 3). A second line along it at 8: the old breakline's levels
        // hold (constrained edges are kept, the new line bends there); only its two
        // ends, typed on the old line, keep their own level, 8.
        let before = flat_grid(10, 1.0, 0.0);
        let first = [vertex(0.0, 5.0, Some(3.0)), vertex(10.0, 5.0, Some(3.0))];
        let one = ok(add_breakline(&before, &first, false));
        for k in 0..=10 {
            near(z_at(&one.surface, f64::from(k), 5.0), 3.0, 1e-9);
        }
        let second = [vertex(0.5, 5.0, Some(8.0)), vertex(9.5, 5.0, Some(8.0))];
        let two = ok(add_breakline(&one.surface, &second, false));
        for k in 1..=9 {
            near(z_at(&two.surface, f64::from(k), 5.0), 3.0, 1e-9);
        }
        near(z_at(&two.surface, 0.5, 5.0), 8.0, 1e-9);
        near(z_at(&two.surface, 9.5, 5.0), 8.0, 1e-9);
        assert_eq!(two.report.points_relevelled, 0);
    }

    fn line_of(points: &[(f64, f64, Option<f64>)], closed: bool) -> BreaklineInput {
        BreaklineInput {
            vertices: points.iter().map(|&(x, y, z)| vertex(x, y, z)).collect(),
            closed,
        }
    }

    #[test]
    fn two_new3_d_lines_through_one_vertex_must_agree_there() {
        // (5,5) is a vertex of the grid on the diagonal y = x and on the other
        // diagonal. The first line gives it 5 + 4.5 = 9.5; the second, from
        // (0.5,9.5) to (9.5,0.5), 9.5 at its middle if it runs 5 to 14, and 4.5 if
        // it runs 0 to 9.
        let before = flat_grid(10, 1.0, 0.0);
        let first = line_of(&[(0.5, 0.5, Some(5.0)), (9.5, 9.5, Some(14.0))], false);
        let agrees = line_of(&[(0.5, 9.5, Some(5.0)), (9.5, 0.5, Some(14.0))], false);
        let clashes = line_of(&[(0.5, 9.5, Some(0.0)), (9.5, 0.5, Some(9.0))], false);

        let accepted = ok(add_breaklines(&before, &[first.clone(), agrees]));
        near(z_at(&accepted.surface, 5.0, 5.0), 9.5, 1e-9);

        let error =
            add_breaklines(&before, &[first.clone(), clashes.clone()]).expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidGeometry);
        assert!(error.message.contains("different levels"));
        assert_eq!(
            code(add_breaklines(&before, &[clashes, first])),
            Some(ErrorCode::InvalidGeometry)
        );
    }

    // ---- several breaklines in one edit ----------------------------------------------------------

    #[test]
    fn two_new_breaklines_crossing_at_different_levels_are_refused_whichever_is_named_first() {
        // The square at 10; (1,5,12)-(9,5,12) and (5,1,8)-(5,9,8) cross at (5,5),
        // 12 on the one and 8 on the other. Added one after the other the first
        // would have won and the second bent to it, and which was first was an
        // accident of the order they were named in.
        let ground = square_at(10.0, 10.0, 10.0, 10.0, false);
        let along = line_of(&[(1.0, 5.0, Some(12.0)), (9.0, 5.0, Some(12.0))], false);
        let across = line_of(&[(5.0, 1.0, Some(8.0)), (5.0, 9.0, Some(8.0))], false);
        for lines in [
            [along.clone(), across.clone()],
            [across.clone(), along.clone()],
        ] {
            let error = add_breaklines(&ground, &lines).expect_err("refused");
            assert_eq!(error.code, ErrorCode::InvalidGeometry);
            assert!(
                error.message.contains("cross at two different levels"),
                "{}",
                error.message
            );
        }
    }

    #[test]
    fn two_new_breaklines_that_agree_where_they_cross_are_accepted_and_a_draped_one_meets_them() {
        let ground = square_at(10.0, 10.0, 10.0, 10.0, false);
        let along = line_of(&[(1.0, 5.0, Some(12.0)), (9.0, 5.0, Some(12.0))], false);
        let level = line_of(&[(5.0, 1.0, Some(12.0)), (5.0, 9.0, Some(12.0))], false);
        let agreed = ok(add_breaklines(&ground, &[along.clone(), level]));
        assert_eq!(agreed.report.crossings, 1);
        near(z_at(&agreed.surface, 5.0, 5.0), 12.0, 1e-9);

        // A draped line across the 3D one meets it at its level, 12, and is the
        // ground's 10 at its own ends.
        let draped = line_of(&[(5.0, 1.0, None), (5.0, 9.0, None)], false);
        let met = ok(add_breaklines(&ground, &[along, draped]));
        near(z_at(&met.surface, 5.0, 5.0), 12.0, 1e-9);
        near(z_at(&met.surface, 5.0, 1.0), 10.0, 1e-9);
    }

    #[test]
    fn adding_no_breakline_or_one_with_too_few_vertices_is_refused() {
        let ground = square_at(10.0, 10.0, 10.0, 10.0, false);
        assert_eq!(
            code(add_breaklines(&ground, &[])),
            Some(ErrorCode::InvalidArgument)
        );
        let one = line_of(&[(5.0, 5.0, Some(1.0))], false);
        let fine = line_of(&[(1.0, 5.0, Some(12.0)), (9.0, 5.0, Some(12.0))], false);
        assert_eq!(
            code(add_breaklines(&ground, &[fine, one])),
            Some(ErrorCode::InvalidArgument)
        );
    }

    #[test]
    fn a_breakline_that_returns_to_a_position_takes_the_level_given_on_either_visit() {
        // On a square at 10: a closed loop whose last vertex returns to (2,2) and
        // gives it 5, or whose first does. Either way (2,2) is at 5; the segments
        // arriving at and leaving it are straight 3D to the ground's 10 at their
        // other ends, so half way along each (2,5) and (5,2) is 7.5.
        let ground = square_at(10.0, 10.0, 10.0, 10.0, false);
        let later = vec![
            vertex(2.0, 2.0, None),
            vertex(8.0, 2.0, None),
            vertex(8.0, 8.0, None),
            vertex(2.0, 8.0, None),
            vertex(2.0, 2.0, Some(5.0)),
        ];
        let mut earlier = later.clone();
        earlier[0].z = Some(5.0);
        let last = earlier.len() - 1;
        earlier[last].z = None;
        for line in [&later, &earlier] {
            let added = ok(add_breakline(&ground, line, false));
            near(z_at(&added.surface, 2.0, 2.0), 5.0, 1e-9);
            near(z_at(&added.surface, 2.0, 5.0), 7.5, 1e-9);
            near(z_at(&added.surface, 5.0, 2.0), 7.5, 1e-9);
        }
        // Two levels that differ are refused, as they always were.
        let mut clash = later.clone();
        clash[0].z = Some(6.0);
        assert_eq!(
            code(add_breakline(&ground, &clash, false)),
            Some(ErrorCode::InvalidGeometry)
        );
    }

    // ---- on the surface means what TinSurface::locate says ---------------------------------------

    #[test]
    fn a_position_within_tolerance_beyond_the_rim_is_on_the_surface_not_off_it() {
        // The square [0,10]^2 at 10. (-1e-12, 5) is 1e-12 beyond the rim edge x = 0:
        // elevation_at says it is on the surface (within GEOMETRIC of an edge).
        let flat = square_at(10.0, 10.0, 10.0, 10.0, false);
        let beyond = -1e-12;
        assert!(flat.elevation_at_xy(beyond, 5.0).is_some());

        // A breakline from it is not "off the surface".
        let line = [vertex(beyond, 5.0, None), vertex(5.0, 5.0, None)];
        assert!(add_breakline(&flat, &line, false).is_ok());

        // An added point there is a vertex of the rim, not a fin: the same three
        // triangles as the point exactly on the rim, whose area is worked out here
        // - (0,5,12) with the corners at 10 gives triangles (P,0,1) and (P,2,3) of
        // |(0,-20,50)| / 2 each and (P,1,2) of |(20,0,100)| / 2: sqrt(2900) +
        // sqrt(10400) / 2 = 104.8418 (a wall of the fin's 10 m2 would read 110).
        let on = ok(add_points(&flat, &[p3(0.0, 5.0, 12.0)]));
        let close = ok(add_points(&flat, &[p3(beyond, 5.0, 12.0)]));
        assert_eq!(on.surface.triangle_count(), 3);
        assert_eq!(close.surface.triangle_count(), 3);
        assert_eq!(close.report.points_added, 1);
        let expected = 2900.0f64.sqrt() + 10400.0f64.sqrt() / 2.0;
        near(on.surface.surface_area(), expected, 1e-6);
        near(close.surface.surface_area(), expected, 1e-6);
        near(close.surface.plan_area(), 100.0, 1e-9);
        near(z_at(&close.surface, 0.0, 5.0), 12.0, 1e-9); // the rim there is the point's level
    }

    #[test]
    fn two_points_on_the_rim_at_one_position_with_different_levels_are_refused() {
        // (10,5) and a point 5e-8 beyond the rim are one position by GEOMETRIC:
        // 3 and 7 differ by more than COORDINATE, as inside the square.
        let flat = square_at(10.0, 10.0, 10.0, 10.0, false);
        assert_eq!(
            code(add_points(
                &flat,
                &[p3(10.0, 5.0, 3.0), p3(10.00000005, 5.0, 7.0)]
            )),
            Some(ErrorCode::InvalidGeometry)
        );
        assert_eq!(
            code(add_points(
                &flat,
                &[p3(5.0, 3.0, 3.0), p3(5.00000005, 3.0, 7.0)]
            )),
            Some(ErrorCode::InvalidGeometry)
        );
    }

    #[test]
    fn a_position_a_thousand_times_tolerance_beyond_the_rim_is_still_off_the_surface() {
        let flat = square_at(10.0, 10.0, 10.0, 10.0, false);
        let line = [vertex(-1e-4, 5.0, None), vertex(5.0, 5.0, None)];
        let error = add_breakline(&flat, &line, false).expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidGeometry);
        assert!(error.message.contains("off the surface"));
        // A point there extends the surface by the triangle joining it to the rim.
        let extended = ok(add_points(&flat, &[p3(-1e-4, 5.0, 12.0)]));
        assert_eq!(extended.surface.triangle_count(), 3);
        near(
            extended.surface.plan_area(),
            100.0 + 0.5 * 1e-4 * 10.0,
            1e-9,
        );
    }

    // ---- a polygon that touches itself --------------------------------------------------------------

    #[test]
    fn a_polygon_that_visits_a_corner_twice_is_refused_whichever_way_its_lobes_run() {
        let ground = flat_grid(10, 1.0, 0.0);
        // Two triangles joined at (5,5), both counter-clockwise: the total signed
        // area is 8 + 8 and the crossing check, which looks at the interior vertices
        // of an edge, never saw the corner.
        let same_way = polyline(
            &[
                (5.0, 5.0),
                (1.0, 1.0),
                (5.0, 1.0),
                (5.0, 5.0),
                (9.0, 9.0),
                (5.0, 9.0),
            ],
            false,
        );
        // A figure of eight crossing at the vertex (4,4): lobe (2,2) (6,2) (4,4) is
        // counter-clockwise, area 4; lobe (4,4) (3.5,5) (4.5,5) clockwise, area
        // -0.5. The two lobes run opposite ways, so the inside-on-the-left rule
        // that decides which side of a cut edge is kept turned the second lobe
        // inside out: ground outside the polygon was kept and ground inside lost.
        let figure_of_eight = polyline(
            &[
                (2.0, 2.0),
                (6.0, 2.0),
                (4.0, 4.0),
                (3.5, 5.0),
                (4.5, 5.0),
                (4.0, 4.0),
            ],
            false,
        );
        for polygon in [&same_way, &figure_of_eight] {
            let error = clip_to_boundary(&ground, polygon).expect_err("refused");
            assert_eq!(error.code, ErrorCode::InvalidGeometry);
            assert!(
                error.message.contains("touches or crosses itself"),
                "{}",
                error.message
            );
            assert_eq!(
                code(cut_hole(&ground, polygon)),
                Some(ErrorCode::InvalidGeometry)
            );
        }
    }

    #[test]
    fn a_polygon_with_a_corner_on_another_edge_is_refused_and_a_simple_one_is_not() {
        let ground = flat_grid(10, 1.0, 0.0);
        // (5,2) lies on the edge (2,2)-(8,2): a touch, not a crossing.
        let touching = polyline(
            &[(2.0, 2.0), (8.0, 2.0), (8.0, 6.0), (5.0, 2.0), (2.0, 6.0)],
            false,
        );
        assert_eq!(
            code(cut_hole(&ground, &touching)),
            Some(ErrorCode::InvalidGeometry)
        );
        // The same ring with (5,3) instead of (5,2) is a simple concave polygon.
        // Shoelace, x_i y_{i+1} - x_{i+1} y_i over (2,2) (8,2) (8,6) (5,3) (2,6):
        // 2*2-8*2 = -12; 8*6-8*2 = 32; 8*3-5*6 = -6; 5*6-2*3 = 24; 2*2-2*6 = -8;
        // the sum is 30, the area 15.
        let simple = polyline(
            &[(2.0, 2.0), (8.0, 2.0), (8.0, 6.0), (5.0, 3.0), (2.0, 6.0)],
            false,
        );
        let holed = ok(cut_hole(&ground, &simple));
        near(holed.surface.plan_area(), 100.0 - 15.0, 1e-9);
    }

    #[test]
    fn a_bow_tie_whose_lobes_cancel_is_refused_as_touching_itself_not_as_having_no_area() {
        let ground = flat_grid(10, 1.0, 0.0);
        // (2,2) (8,8) (8,2) (2,8): two triangles of equal and opposite area
        let bow_tie = polyline(&[(2.0, 2.0), (8.0, 8.0), (8.0, 2.0), (2.0, 8.0)], false);
        let error = cut_hole(&ground, &bow_tie).expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidGeometry);
        assert!(
            error.message.contains("touches or crosses itself"),
            "{}",
            error.message
        );
        // three collinear points: no area, and says so
        let flat = polyline(&[(1.0, 1.0), (5.0, 1.0), (9.0, 1.0)], false);
        let none = cut_hole(&ground, &flat).expect_err("refused");
        assert!(none.message.contains("no area"), "{}", none.message);
    }

    // ---- a point off the surface joins the pockets it shuts in -----------------------------------

    #[test]
    fn a_point_in_a_square_hole_fills_the_whole_hole() {
        // A 3 x 3 grid of points 10 m apart (area 400) with [5,15]^2 cut out:
        // 300. A point at its centre is joined to the hole's whole rim: the four
        // fans cover the square, 100, and the area is back to 400. Cocircular rim
        // vertices let the kernel choose a diagonal across a corner of the hole
        // instead, and the triangle of three rim vertices that makes was dropped
        // as not having the point as a corner - leaving two ears of 12.5.
        let ground = grid(2, 10.0, |x, y| x + y);
        let holed = ok(cut_hole(&ground, &rectangle(5.0, 5.0, 15.0, 15.0)));
        near(holed.surface.plan_area(), 300.0, 1e-9);
        let filled = ok(add_points(&holed.surface, &[p3(10.0, 10.0, 20.0)]));
        near(filled.surface.plan_area(), 400.0, 1e-9);
        for spot in [
            p2(6.0, 6.0),
            p2(14.0, 14.0),
            p2(14.0, 6.0),
            p2(6.0, 14.0),
            p2(10.0, 7.0),
        ] {
            assert!(
                filled.surface.elevation_at(spot).is_some(),
                "no ground at {spot:?}"
            );
        }
        near(z_at(&filled.surface, 10.0, 10.0), 20.0, 1e-9);
        // Off centre: the hole is just as full.
        let aside = ok(add_points(&holed.surface, &[p3(10.5, 10.5, 20.0)]));
        near(aside.surface.plan_area(), 400.0, 1e-9);
    }

    #[test]
    fn a_point_beyond_a_notch_leaves_no_enclosed_pocket() {
        // The 3 x 3 grid less its corner cell [10,20]^2: an L of area 300. A point
        // at (25,25) is joined to the rim edges it sees: (20,0)-(20,10) and
        // (0,20)-(10,20), 25 each, and over the notch's mouth (20,10)-(25,25)-
        // (10,20), 100 - and the triangle (10,10) (20,10) (10,20) of the notch
        // behind them, 50, is shut in. 300 + 25 + 25 + 100 + 50 = 500.
        let ground = grid(2, 10.0, |_, _| 0.0);
        let cells = triangles_by_polygon(&ground, &rectangle(10.0, 10.0, 20.0, 20.0), true);
        assert_eq!(cells.len(), 2);
        let notched = ok(delete_triangles(&ground, &cells));
        near(notched.surface.plan_area(), 300.0, 1e-9);
        let extended = ok(add_points(&notched.surface, &[p3(25.0, 25.0, 50.0)]));
        near(extended.surface.plan_area(), 500.0, 1e-9);
        assert!(
            extended.surface.elevation_at_xy(13.0, 13.0).is_some(),
            "the pocket behind the bridge"
        );
        assert!(extended.surface.elevation_at_xy(18.0, 18.0).is_some());
    }

    #[test]
    fn concavities_the_point_does_not_shut_in_are_left_as_they_were() {
        let ground = grid(2, 10.0, |_, _| 0.0);
        // A hole with a point elsewhere stays a hole: the point at (30,10) joins
        // the outer rim only, 0.5 x 20 x 10 = 100 more.
        let holed = ok(cut_hole(&ground, &rectangle(5.0, 5.0, 15.0, 15.0)));
        let aside = ok(add_points(&holed.surface, &[p3(30.0, 10.0, 0.0)]));
        near(aside.surface.plan_area(), 400.0, 1e-9);
        assert!(aside.surface.elevation_at_xy(10.0, 10.0).is_none());
        assert!(aside.surface.elevation_at_xy(25.0, 10.0).is_some());
        // A notch open to the outside, with a point on the far side of the L: the
        // notch is not shut in and stays out.
        let cells = triangles_by_polygon(&ground, &rectangle(10.0, 10.0, 20.0, 20.0), true);
        let notched = ok(delete_triangles(&ground, &cells));
        let far = ok(add_points(&notched.surface, &[p3(5.0, -5.0, 0.0)]));
        assert!(far.surface.elevation_at_xy(15.0, 15.0).is_none());
        assert!(far.surface.elevation_at_xy(13.0, 13.0).is_none());
    }

    // ---- smaller things ------------------------------------------------------------------------------

    #[test]
    fn thinning_that_removes_nothing_returns_the_surface_as_it_was() {
        // The square's triangles written rotated: (1,2,0) and (3,0,2) are the
        // canonical (0,1,2) and (0,2,3) started at another corner. Every vertex is
        // on the rim and held, so nothing can go - and the surface must come back
        // as it was, not sorted into the canonical order.
        let rotated = make(
            vec![
                p3(0.0, 0.0, 0.0),
                p3(10.0, 0.0, 0.0),
                p3(10.0, 10.0, 0.0),
                p3(0.0, 10.0, 0.0),
            ],
            vec![[1, 2, 0], [3, 0, 2]],
            vec![],
        );
        let thinned = ok(thin_surface(&rotated, 0.0, None));
        assert_eq!(thinned.report.points_removed, 0);
        assert_eq!(thinned.surface, rotated);
    }

    #[test]
    fn smoothing_inside_a_polygon_with_no_vertex_says_no_vertex_lies_there_rather_than_all_are_held()
     {
        let surface = pyramid();
        // Inside one triangle, holding no vertex at all.
        let empty = smooth_surface(
            &surface,
            &SmoothOptions::default(),
            Some(&rectangle(1.0, 4.0, 2.0, 5.0)),
        )
        .expect_err("refused");
        assert_eq!(empty.code, ErrorCode::InvalidArgument);
        assert!(
            empty
                .message
                .contains("no vertex of the surface lies inside the polygon"),
            "{}",
            empty.message
        );
        // Round the corner (0,0) only, which is on the rim and held.
        let held = smooth_surface(
            &surface,
            &SmoothOptions::default(),
            Some(&rectangle(-1.0, -1.0, 1.0, 1.0)),
        )
        .expect_err("refused");
        assert!(held.message.contains("free to move"), "{}", held.message);
    }

    #[test]
    fn a_vertex_within_reach_is_found_wherever_it_is_relative_to_the_triangle_under_the_point() {
        // Off the surface and in a thin triangle whose own corners are far.
        let flat = flat_grid(2, 1.0, 0.0);
        // The corner (0,0) is 0.07 from (-0.05,-0.05): found with a reach of 0.2,
        // not found without one (the point is off the surface), nor with 0.05.
        assert_eq!(vertex_at(&flat, p2(-0.05, -0.05), Some(0.2)), Some(0));
        assert!(vertex_at(&flat, p2(-0.05, -0.05), None).is_none());
        assert!(vertex_at(&flat, p2(-0.05, -0.05), Some(0.05)).is_none());
        assert!(vertex_at(&flat, p2(-5.0, -5.0), Some(0.2)).is_none());

        // A thin triangle A(0,0) B(10,0) C(5,0.1) over D(9,-0.2): the point (9,0.01)
        // is inside ABC, whose corners are 1.0 (B), 9 and 4 away; D is 0.21 away.
        let thin = make(
            vec![
                p3(0.0, 0.0, 0.0),
                p3(10.0, 0.0, 0.0),
                p3(5.0, 0.1, 0.0),
                p3(9.0, -0.2, 0.0),
            ],
            vec![[0, 1, 2], [0, 3, 1]],
            vec![],
        );
        assert_eq!(vertex_at(&thin, p2(9.0, 0.01), Some(0.5)), Some(3));
        assert_eq!(
            vertex_at(&thin, p2(9.0, 0.01), None),
            Some(1),
            "no reach: the nearest corner"
        );
        assert_eq!(
            vertex_at(&thin, p2(9.0, 0.01), Some(1.5)),
            Some(1),
            "a corner within reach"
        );
    }

    #[test]
    fn a_vertex_on_a_breakline_or_boundary_edge_is_the_one_delete_point_refuses() {
        let with_line = square_with_an_old_breakline();
        // The old breakline's end (100,30) is on a constrained edge; the square's
        // corner (100,100) is on the rim only, which delete_points does delete.
        let on_line = vertex_index(&with_line, 100.0, 30.0);
        let corner = vertex_index(&with_line, 100.0, 100.0);
        assert!(on_line >= 0);
        assert!(corner >= 0);
        assert!(on_constrained_edge(&with_line, on_line as u32));
        assert!(!on_constrained_edge(&with_line, corner as u32));
        assert!(delete_points(&with_line, &[on_line as u32], HeldPointRule::Refuse).is_err());
        assert!(delete_points(&with_line, &[corner as u32], HeldPointRule::Refuse).is_ok());
    }

    #[test]
    fn a_number_beyond_the_most_an_edit_accepts_is_refused_where_it_is_made() {
        // 1e308 passed the finite-only check, and then the plan area read nan.
        let flat = square_at(10.0, 10.0, 10.0, 10.0, false);
        let most = katana_terrain::MOST_MAGNITUDE;
        assert_eq!(
            code(add_points(&flat, &[p3(1e308, 1.0, 1.0)])),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            code(add_points(&flat, &[p3(5.0, 5.0, 1e308)])),
            Some(ErrorCode::InvalidArgument)
        );
        for height in [
            change(HeightOperation::Raise, 1e308, None),
            change(HeightOperation::SetLevel, 1e308, None),
            change(HeightOperation::Scale, 1e308, Some(0.0)),
        ] {
            assert_eq!(
                code(change_heights(&flat, &height, None)),
                Some(ErrorCode::InvalidArgument)
            );
        }
        let high = [vertex(1.0, 1.0, Some(1e308)), vertex(9.0, 9.0, Some(1.0))];
        assert_eq!(
            code(add_breakline(&flat, &high, false)),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            code(cut_hole(&flat, &rectangle(1.0, 1.0, 1e308, 9.0))),
            Some(ErrorCode::InvalidArgument)
        );
        // The limit itself is accepted, and areas stay numbers.
        let at = ok(add_points(&flat, &[p3(5.0, 5.0, most)]));
        assert!(at.surface.surface_area().is_finite());
        assert_eq!(
            code(add_points(&flat, &[p3(5.0, 5.0, most * 1.0001)])),
            Some(ErrorCode::InvalidArgument)
        );
    }

    // ---- the indexed polygon test answers what Polyline2::classify answers -----------------------

    #[test]
    fn the_banded_polygon_test_gives_every_answer_the_plain_one_gives() {
        use katana_geometry::{Containment, PolygonClassifier};
        // A twelve-pointed star about (50,50) with radii 20 and 8, concave and
        // with edges at every slope, asked about a lattice of points 0.7 apart, its
        // corners, the middles of its edges and points 1e-8 and 1e-6 off them.
        let mut star = Polyline2::new(Vec::new(), true);
        for i in 0..24 {
            let radius = if i % 2 == 0 { 20.0 } else { 8.0 };
            let angle = f64::from(i) * std::f64::consts::PI / 12.0;
            star.vertices
                .push(p2(50.0 + radius * angle.cos(), 50.0 + radius * angle.sin()));
        }
        let banded = PolygonClassifier::new(&star);
        let mut queries: Vec<Point2> = Vec::new();
        let mut x = 20.0;
        while x < 80.0 {
            let mut y = 20.0;
            while y < 80.0 {
                queries.push(p2(x, y));
                y += 0.7;
            }
            x += 0.7;
        }
        let count = star.vertices.len();
        for i in 0..count {
            let a = star.vertices[i];
            let b = star.vertices[(i + 1) % count];
            for t in [0.0, 0.5] {
                let on = p2(a.x + t * (b.x - a.x), a.y + t * (b.y - a.y));
                for off in [0.0, 1e-8, -1e-8, 1e-6, -1e-6] {
                    queries.push(p2(on.x + off, on.y));
                    queries.push(p2(on.x, on.y + off));
                }
            }
        }
        let mut inside = 0;
        let mut outside = 0;
        let mut boundary = 0;
        for &q in &queries {
            let plain = star.classify(q);
            assert_eq!(banded.classify(q), plain, "at {q:?}");
            match plain {
                Containment::Inside => inside += 1,
                Containment::Outside => outside += 1,
                Containment::OnBoundary => boundary += 1,
            }
        }
        // The sweep reaches all three answers.
        assert!(inside > 100);
        assert!(outside > 100);
        assert!(boundary > 20);
        // Fewer than three vertices bound nothing, as an open or degenerate polyline does.
        let two = polyline(&[(0.0, 0.0), (1.0, 1.0)], false);
        assert_eq!(
            PolygonClassifier::new(&two).classify(p2(0.5, 0.5)),
            Containment::Outside
        );
    }

    #[test]
    fn a_boundary_or_a_hole_round_a_circle_costs_exactly_the_circles_area() {
        // The triangles outside the region are asked about a piece at a time. A
        // 64-gon of radius 7 about (10.3, 9.7) in a 20 x 20 grid: its area
        // (64 / 2) r^2 sin(2 pi / 64) is the boundary's whole result and the
        // hole's 400 less it.
        let ground = flat_grid(20, 1.0, 0.0);
        let mut circle = Polyline2::new(Vec::new(), true);
        for i in 0..64 {
            let angle = f64::from(i) * 2.0 * std::f64::consts::PI / 64.0;
            circle
                .vertices
                .push(p2(10.3 + 7.0 * angle.cos(), 9.7 + 7.0 * angle.sin()));
        }
        let area = 32.0 * 49.0 * (2.0 * std::f64::consts::PI / 64.0).sin();
        let clipped = ok(clip_to_boundary(&ground, &circle));
        near(clipped.surface.plan_area(), area, 1e-9);
        let holed = ok(cut_hole(&ground, &circle));
        near(holed.surface.plan_area(), 400.0 - area, 1e-9);
    }

    // ---- several holes at once -------------------------------------------------------------------

    fn triangle_ring(a: Point2, b: Point2, c: Point2) -> Polyline2 {
        Polyline2::new(vec![a, b, c], true)
    }

    /// What cut_hole in a loop gives - each hole cut on the surface the one before
    /// left, the reports summed as the verb summed them before cut_holes existed:
    /// the first step's "before", the last's "after", the counts of all.
    fn cut_in_turn(surface: &TinSurface, holes: &[Polyline2]) -> TinEditResult {
        let mut total = TinEditResult {
            surface: surface.clone(),
            ..TinEditResult::default()
        };
        let mut first = true;
        for hole in holes {
            let step = ok(cut_hole(&total.surface, hole));
            let r = step.report;
            if first {
                total.report = r;
            } else {
                total.report.triangles_after = r.triangles_after;
                total.report.vertices_after = r.vertices_after;
                total.report.points_added += r.points_added;
                total.report.points_removed += r.points_removed;
                total.report.points_relevelled += r.points_relevelled;
                total.report.points_moved += r.points_moved;
                total.report.points_held += r.points_held;
                total.report.crossings += r.crossings;
                total.report.largest_change = total.report.largest_change.max(r.largest_change);
            }
            first = false;
            total.surface = step.surface;
        }
        total
    }

    fn expect_same_report(a: &katana_terrain::TinEditReport, b: &katana_terrain::TinEditReport) {
        assert_eq!(a.triangles_before, b.triangles_before);
        assert_eq!(a.triangles_after, b.triangles_after);
        assert_eq!(a.vertices_before, b.vertices_before);
        assert_eq!(a.vertices_after, b.vertices_after);
        assert_eq!(a.points_added, b.points_added);
        assert_eq!(a.points_removed, b.points_removed);
        assert_eq!(a.points_relevelled, b.points_relevelled);
        assert_eq!(a.points_moved, b.points_moved);
        assert_eq!(a.points_held, b.points_held);
        assert_eq!(a.crossings, b.crossings);
        near(a.largest_change, b.largest_change, 1e-12);
    }

    /// A 40 x 40 m ground of unit cells on the plane z = 0.5 x + 0.25 y.
    fn plane_ground40() -> TinSurface {
        grid(40, 1.0, |x, y| 0.5 * x + 0.25 * y)
    }

    #[test]
    fn holes_apart_from_each_other_are_cut_in_one_patch_and_give_what_cutting_them_one_by_one_gives()
     {
        let ground = plane_ground40();
        // Four footprints in general position, none within a metre of another, in
        // a ground of 1600 m2. Their areas, worked out here:
        //   [3.3, 8.6]  x [4.7, 9.1]    5.3 x 4.4                       = 23.32
        //   [15.2, 19.9] x [6.1, 12.8]  4.7 x 6.7                       = 31.49
        //   (25.4,22.2) (33.1,24.7) (28.3,31.9): half of
        //     |(33.1-25.4)(31.9-22.2) - (28.3-25.4)(24.7-22.2)|
        //     = half of |7.7 x 9.7 - 2.9 x 2.5| = half of 67.44          = 33.72
        //   [5.5, 11.7] x [30.3, 36.4]  6.2 x 6.1                       = 37.82
        // 126.35 together, so 1473.65 of ground is left.
        let holes = vec![
            rectangle(3.3, 4.7, 8.6, 9.1),
            rectangle(15.2, 6.1, 19.9, 12.8),
            triangle_ring(p2(25.4, 22.2), p2(33.1, 24.7), p2(28.3, 31.9)),
            rectangle(5.5, 30.3, 11.7, 36.4),
        ];

        let mut stats = HoleCutStats::default();
        let together = ok(cut_holes(&ground, &holes, Some(&mut stats)));
        // The point of it: four footprints, ONE rebuild of the whole surface.
        assert_eq!(stats.patches, 1);
        near(together.surface.plan_area(), 1600.0 - 126.35, 1e-9);
        for inside in [p2(6.0, 7.0), p2(17.0, 9.0), p2(28.9, 26.3), p2(8.0, 33.0)] {
            assert!(
                together.surface.elevation_at(inside).is_none(),
                "{inside:?}"
            );
        }
        near(z_at(&together.surface, 1.0, 1.0), 0.75, 1e-9);
        near(z_at(&together.surface, 38.0, 38.0), 28.5, 1e-9);

        // And the loop's surface and the loop's report.
        let in_turn = cut_in_turn(&ground, &holes);
        assert!(same_surface_geometry(&together.surface, &in_turn.surface));
        expect_same_report(&together.report, &in_turn.report);
        assert!(
            together.report.points_added > 0,
            "the cuts made vertices, so the sums mean something"
        );
    }

    #[test]
    fn holes_that_overlap_touch_nest_or_repeat_are_cut_one_after_the_other_as_they_always_were() {
        let ground = plane_ground40();
        let first = rectangle(3.3, 4.7, 8.6, 9.1); // 5.3 x 4.4 = 23.32
        let check = |holes: Vec<Polyline2>, patches: usize, removed: f64, what: &str| {
            let mut stats = HoleCutStats::default();
            let together = cut_holes(&ground, &holes, Some(&mut stats))
                .unwrap_or_else(|error| panic!("{what}: {}", error.describe()));
            assert_eq!(stats.patches, patches, "{what}");
            near(together.surface.plan_area(), 1600.0 - removed, 1e-9);
            let in_turn = cut_in_turn(&ground, &holes);
            assert!(
                same_surface_geometry(&together.surface, &in_turn.surface),
                "{what}"
            );
            expect_same_report(&together.report, &in_turn.report);
            if patches == holes.len() {
                // One patch each: it IS the loop, bit for bit.
                assert_eq!(together.surface, in_turn.surface, "{what}");
            }
        };

        // [6, 12.2] x [7, 12] is 6.2 x 5 = 31 and meets the first in
        // [6, 8.6] x [7, 9.1] = 2.6 x 2.1 = 5.46: the union is 23.32 + 31 - 5.46.
        check(
            vec![first.clone(), rectangle(6.0, 7.0, 12.2, 12.0)],
            2,
            48.86,
            "overlapping",
        );
        // [3, 7] x [4, 9] and [7, 11] x [4, 9] share the edge x = 7: 4 x 5 = 20 each,
        // 40 between them. On the lattice's lines, so that the vertices the first
        // cut left on the shared edge are lattice vertices and not where the kernel
        // computed a crossing: the second outline then runs through them exactly.
        check(
            vec![
                rectangle(3.0, 4.0, 7.0, 9.0),
                rectangle(7.0, 4.0, 11.0, 9.0),
            ],
            2,
            40.0,
            "touching along an edge",
        );
        // [3.3, 16.9] x [4.7, 19.3] is 13.6 x 14.6 = 198.56 and holds [6.1, 9.7] x
        // [8.2, 12.4]; the inner one adds nothing, whichever is named first.
        let outer = rectangle(3.3, 4.7, 16.9, 19.3);
        let inner = rectangle(6.1, 8.2, 9.7, 12.4);
        check(
            vec![outer.clone(), inner.clone()],
            2,
            198.56,
            "nested, the outer first",
        );
        check(vec![inner, outer], 2, 198.56, "nested, the inner first");
        check(
            vec![rectangle(3.0, 4.0, 7.0, 9.0), rectangle(3.0, 4.0, 7.0, 9.0)],
            2,
            20.0,
            "the same footprint twice",
        );
        // Two squares of 0.1 m inside ONE triangle of the ground, the lower one of
        // the cell (10,10) - (11,11) - 0.01 m2 each, apart from each other, but
        // both in the triangle the patch rebuilds.
        check(
            vec![
                rectangle(10.60, 10.20, 10.70, 10.30),
                rectangle(10.80, 10.35, 10.90, 10.45),
            ],
            2,
            0.02,
            "two in one triangle",
        );
        // The second overlaps the first and the third is apart from both: the
        // first is cut alone, the second and third together, 2 patches. The union
        // of the first two is 48.86, and [5.5, 11.7] x [30.3, 36.4] adds 37.82.
        check(
            vec![
                first,
                rectangle(6.0, 7.0, 12.2, 12.0),
                rectangle(5.5, 30.3, 11.7, 36.4),
            ],
            2,
            86.68,
            "a dependent pair, then an independent one",
        );
    }

    #[test]
    fn a_sweep_of_separated_footprints_with_one_twin_among_them_is_two_patches_and_the_loops_surface()
     {
        // One footprint in each 10 m cell of a 4 x 4 lattice over the ground,
        // jittered by a fixed-seed generator (Numerical Recipes' constants, the
        // benchmarks' own), 2.1 to 4.9 m on a side and at most 1 m off the cell's
        // centre: they lie within [1.55, 8.45] of their cell, so the footprints of
        // neighbouring cells are 3.1 m apart. After the sixth a twin: the sixth
        // moved by (1.3, 0.9), overlapping it and still 1.8 m from the others -
        // more than a triangle of the ground (1.42 m across) can span.
        let mut state: u32 = 12345;
        let mut next = |low: f64, high: f64| {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            low + (high - low) * (f64::from(state) / 4294967296.0)
        };
        let mut holes: Vec<Polyline2> = Vec::new();
        let mut removed = 0.0;
        for j in 0..4 {
            for i in 0..4 {
                let cx = 10.0 * f64::from(i) + 5.0 + next(-1.0, 1.0);
                let cy = 10.0 * f64::from(j) + 5.0 + next(-1.0, 1.0);
                let w = next(2.1, 4.9);
                let h = next(2.1, 4.9);
                holes.push(rectangle(
                    cx - w / 2.0,
                    cy - h / 2.0,
                    cx + w / 2.0,
                    cy + h / 2.0,
                ));
                removed += w * h;
                if j * 4 + i == 5 {
                    holes.push(rectangle(
                        cx - w / 2.0 + 1.3,
                        cy - h / 2.0 + 0.9,
                        cx + w / 2.0 + 1.3,
                        cy + h / 2.0 + 0.9,
                    ));
                    // Two equal rectangles shifted by (1.3, 0.9) share (w - 1.3) x (h - 0.9).
                    removed += w * h - (w - 1.3) * (h - 0.9);
                }
            }
        }
        assert_eq!(holes.len(), 17);

        let ground = plane_ground40();
        let mut stats = HoleCutStats::default();
        let together = ok(cut_holes(&ground, &holes, Some(&mut stats)));
        // The first six together; the twin and the ten after it together.
        assert_eq!(stats.patches, 2);
        near(together.surface.plan_area(), 1600.0 - removed, 1e-8);
        let in_turn = cut_in_turn(&ground, &holes);
        assert!(same_surface_geometry(&together.surface, &in_turn.surface));
        expect_same_report(&together.report, &in_turn.report);
    }

    #[test]
    fn a_hundred_small_holes_on_a_hundred_metre_ground_are_one_patch_and_the_loops_surface() {
        // 100 footprints of 2.3 x 1.7 m, one in each 10 m cell of a 10 x 10
        // lattice: 3.91 m2 each, 391 together, so 10000 - 391 = 9609 is left.
        let ground = grid(100, 1.0, |x, y| 0.5 * x + 0.25 * y);
        let mut holes: Vec<Polyline2> = Vec::new();
        for j in 0..10 {
            for i in 0..10 {
                let x = 10.0 * f64::from(i) + 3.3;
                let y = 10.0 * f64::from(j) + 4.2;
                holes.push(rectangle(x, y, x + 2.3, y + 1.7));
            }
        }
        let mut stats = HoleCutStats::default();
        let together = ok(cut_holes(&ground, &holes, Some(&mut stats)));
        assert_eq!(stats.patches, 1);
        near(together.surface.plan_area(), 9609.0, 1e-7);
        let in_turn = cut_in_turn(&ground, &holes);
        assert!(same_surface_geometry(&together.surface, &in_turn.surface));
        expect_same_report(&together.report, &in_turn.report);
    }

    #[test]
    fn cutting_holes_refuses_what_cutting_them_one_by_one_refuses_and_says_the_same() {
        let ground = plane_ground40();
        let a = rectangle(3.3, 4.7, 8.6, 9.1);
        let b = rectangle(15.2, 6.1, 19.9, 12.8);
        let c = rectangle(5.5, 30.3, 11.7, 36.4);

        // Nothing to cut, nothing to cut from.
        assert_eq!(
            code(cut_holes(&ground, &[], None)),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            code(cut_holes(
                &TinSurface::new(),
                std::slice::from_ref(&a),
                None
            )),
            Some(ErrorCode::InvalidArgument)
        );

        // One footprint is cut_hole itself, bit for bit, report and all.
        let mut one = HoleCutStats::default();
        let alone = ok(cut_holes(&ground, std::slice::from_ref(&a), Some(&mut one)));
        let single = ok(cut_hole(&ground, &a));
        assert_eq!(one.patches, 1);
        assert_eq!(alone.surface, single.surface);
        expect_same_report(&alone.report, &single.report);

        // A bow tie in the middle of three that are fine and apart: its edges
        // BC (34,3)-(27,10) and DA (33,9)-(26,2) cross at (30.5, 6.5), its signed
        // area is 14 / 2 = 7 so it is not refused as flat, and the kernel finds the
        // crossing. The batch fails; the answer is the loop's - the same refusal.
        let bow_tie = polyline(&[(26.0, 2.0), (34.0, 3.0), (27.0, 10.0), (33.0, 9.0)], true);
        let batch = cut_holes(&ground, &[b, bow_tie.clone(), c], None).expect_err("refused");
        let alongside = cut_hole(&ground, &bow_tie).expect_err("refused");
        assert_eq!(batch.code, ErrorCode::InvalidGeometry);
        assert_eq!(batch.code, alongside.code);
        assert_eq!(batch.message, alongside.message);
        assert!(
            batch.message.contains("crosses itself"),
            "{}",
            batch.message
        );

        // A coordinate that is not finite, after one that is fine: the fine one is
        // cut first, then the refusal is the polygon's, as in a loop.
        let mut broken = rectangle(20.0, 20.0, 24.0, 24.0);
        broken.vertices[2].x = f64::NAN;
        let not_finite =
            cut_holes(&ground, &[a.clone(), broken.clone()], None).expect_err("refused");
        assert_eq!(not_finite.code, ErrorCode::InvalidArgument);
        assert_eq!(
            not_finite.message,
            cut_hole(&ground, &broken).expect_err("refused").message
        );

        // A footprint round the whole ground, named after one inside it: the
        // refusal is that nothing would be left, as cut_hole's, and not a batch's.
        let everything = rectangle(-5.0, -5.0, 45.0, 45.0);
        let all = cut_holes(&ground, &[a, everything.clone()], None).expect_err("refused");
        assert_eq!(all.code, ErrorCode::InvalidState);
        assert_eq!(
            all.message,
            cut_hole(&ground, &everything).expect_err("refused").message
        );

        // A footprint beside the ground changes nothing, and is one patch.
        let mut beside = HoleCutStats::default();
        let apart = ok(cut_holes(
            &ground,
            &[rectangle(100.0, 100.0, 110.0, 110.0)],
            Some(&mut beside),
        ));
        assert_eq!(apart.surface, ground);
        assert_eq!(beside.patches, 1);
    }

    // ---- deleting a breakline ----------------------------------------------------------------------

    /// `ground` with a draped breakline along the straight run from `from` to `to`
    /// (a row or a column of the unit lattice, so its edges are the lattice's).
    fn with_breakline(ground: &TinSurface, from: Point2, to: Point2) -> TinSurface {
        let line = [
            BreaklineVertex {
                position: from,
                z: None,
            },
            BreaklineVertex {
                position: to,
                z: None,
            },
        ];
        ok(add_breakline(ground, &line, false)).surface
    }

    fn open_line(points: &[(f64, f64)]) -> Polyline2 {
        polyline(points, false)
    }

    #[test]
    fn deleting_a_breakline_makes_its_edges_ordinary_and_changes_no_triangle_nor_vertex() {
        // The row y = 4 from x = 1 to 7 of a 10 x 10 unit grid: six lattice edges.
        let before = with_breakline(&flat_grid(10, 1.0, 0.0), p2(1.0, 4.0), p2(7.0, 4.0));
        near(constrained_length(&before), 6.0, 1e-9); // the setup: six edges of one metre
        // The line may run past the breakline at either end and need not pass
        // through the surface's vertices to lie along its edges.
        let cleared = ok(delete_breaklines(
            &before,
            &[open_line(&[(0.5, 4.0), (7.5, 4.0)])],
        ));
        assert_eq!(cleared.report.edges_cleared, 6);
        assert_eq!(cleared.report.edges_held, 0);
        near(constrained_length(&cleared.surface), 0.0, 1e-12);
        // Nothing else changed: the same vertices, the same triangles in the same
        // order, the same area.
        assert_eq!(cleared.surface.vertices(), before.vertices());
        assert_eq!(cleared.surface.triangles(), before.triangles());
        assert_eq!(
            cleared.report.triangles_before,
            cleared.report.triangles_after
        );
        assert_eq!(
            cleared.report.vertices_before,
            cleared.report.vertices_after
        );
        assert_eq!(cleared.report.points_added, 0);
        assert_eq!(cleared.report.points_removed, 0);
        near(cleared.surface.plan_area(), 100.0, 1e-9);

        // The vertex (4,4) was on a breakline, and DELETE POINT refused it; now it
        // is an ordinary vertex of six triangles, and goes.
        let vertex = vertex_index(&before, 4.0, 4.0);
        assert!(vertex >= 0);
        let vertices = [vertex as u32];
        assert_eq!(
            code(delete_points(&before, &vertices, HeldPointRule::Refuse)),
            Some(ErrorCode::InvalidGeometry)
        );
        assert!(on_constrained_edge(&before, vertices[0]));
        assert!(!on_constrained_edge(&cleared.surface, vertices[0]));
        let deleted = ok(delete_points(
            &cleared.surface,
            &vertices,
            HeldPointRule::Refuse,
        ));
        assert_eq!(deleted.report.points_removed, 1);
        near(deleted.surface.plan_area(), 100.0, 1e-9);
    }

    #[test]
    fn a_line_over_part_of_a_breakline_clears_the_edges_of_that_part_and_no_other() {
        // The line from (1,4) to (4,4) lies along three of the six edges: (1..2),
        // (2..3), (3..4). The other three, (4..5), (5..6), (6..7), stay.
        let before = with_breakline(&flat_grid(10, 1.0, 0.0), p2(1.0, 4.0), p2(7.0, 4.0));
        let cleared = ok(delete_breaklines(
            &before,
            &[open_line(&[(1.0, 4.0), (4.0, 4.0)])],
        ));
        assert_eq!(cleared.report.edges_cleared, 3);
        near(constrained_length(&cleared.surface), 3.0, 1e-9);
        let at = |x: f64| vertex_index(&cleared.surface, x, 4.0) as u32;
        // (1,4), (2,4) and (3,4) have only cleared edges; (4,4) is still the end of
        // the edge to (5,4), which stays.
        assert!(!on_constrained_edge(&cleared.surface, at(1.0)));
        assert!(!on_constrained_edge(&cleared.surface, at(2.0)));
        assert!(!on_constrained_edge(&cleared.surface, at(3.0)));
        assert!(on_constrained_edge(&cleared.surface, at(4.0)));
        assert!(on_constrained_edge(&cleared.surface, at(5.0)));
        // And (4,4) is refused as before.
        assert_eq!(
            code(delete_points(
                &cleared.surface,
                &[at(4.0)],
                HeldPointRule::Refuse
            )),
            Some(ErrorCode::InvalidGeometry)
        );
    }

    #[test]
    fn a_line_along_no_breakline_changes_nothing_and_says_so() {
        let before = with_breakline(&flat_grid(10, 1.0, 0.0), p2(1.0, 4.0), p2(7.0, 4.0));
        // A row with no breakline, one across the breakline, one that stops short
        // of the first edge's far end, and one 2e-7 above it - beyond the
        // coincidence tolerance, 1e-7 - each lies along none of its edges.
        for line in [
            open_line(&[(1.0, 5.0), (7.0, 5.0)]),
            open_line(&[(3.0, 3.0), (5.0, 5.0)]),
            open_line(&[(1.0, 4.0), (1.5, 4.0)]),
            open_line(&[(1.0, 4.0000002), (7.0, 4.0000002)]),
        ] {
            let result = ok(delete_breaklines(&before, std::slice::from_ref(&line)));
            assert_eq!(result.report.edges_cleared, 0);
            assert_eq!(
                result.surface, before,
                "the surface as it was, not a copy rebuilt"
            );
        }
        // 5e-8 above it is within the tolerance: the same six edges.
        let within = ok(delete_breaklines(
            &before,
            &[open_line(&[(1.0, 4.00000005), (7.0, 4.00000005)])],
        ));
        assert_eq!(within.report.edges_cleared, 6);
    }

    #[test]
    fn a_closed_lines_closing_side_lies_along_edges_and_an_open_ones_does_not() {
        // A breakline up the column x = 1 from y = 4 to 7: three edges. The ring
        // (1,4) (5,4) (5,7) (1,7) comes back to (1,4) by the closing side (1,7)-(1,4),
        // which is that column; opened, the same four vertices stop at (1,7).
        let before = with_breakline(&flat_grid(10, 1.0, 0.0), p2(1.0, 4.0), p2(1.0, 7.0));
        near(constrained_length(&before), 3.0, 1e-9);
        let mut ring = open_line(&[(1.0, 4.0), (5.0, 4.0), (5.0, 7.0), (1.0, 7.0)]);
        let opened = ok(delete_breaklines(&before, std::slice::from_ref(&ring)));
        assert_eq!(opened.report.edges_cleared, 0);
        ring.closed = true;
        let closed = ok(delete_breaklines(&before, std::slice::from_ref(&ring)));
        assert_eq!(closed.report.edges_cleared, 3);
        near(constrained_length(&closed.surface), 0.0, 1e-12);
    }

    #[test]
    fn several_lines_clear_the_edges_along_any_of_them() {
        // Two breaklines: the row y = 4 from 1 to 3 (two edges) and the column
        // x = 6 from 2 to 5 (three). Two lines clear both, one clears its own.
        let mut before = with_breakline(&flat_grid(10, 1.0, 0.0), p2(1.0, 4.0), p2(3.0, 4.0));
        before = with_breakline(&before, p2(6.0, 2.0), p2(6.0, 5.0));
        near(constrained_length(&before), 5.0, 1e-9);
        let both = [
            open_line(&[(0.0, 4.0), (4.0, 4.0)]),
            open_line(&[(6.0, 0.0), (6.0, 9.0)]),
        ];
        let all = ok(delete_breaklines(&before, &both));
        assert_eq!(all.report.edges_cleared, 5);
        let one = ok(delete_breaklines(&before, &both[..1]));
        assert_eq!(one.report.edges_cleared, 2);
        near(constrained_length(&one.surface), 3.0, 1e-9);
    }

    #[test]
    fn the_constrained_rim_is_the_boundary_not_a_breakline_and_is_held() {
        // A boundary makes its rim edges constrained: [2.5, 7.5]^2 cut from a 10 x
        // 10 grid has a rim of 4 x 5 = 20 m, all constrained. A line along its
        // bottom side clears nothing and counts the edges it left.
        let clipped = ok(clip_to_boundary(
            &flat_grid(10, 1.0, 0.0),
            &rectangle(2.5, 2.5, 7.5, 7.5),
        ));
        near(constrained_length(&clipped.surface), 20.0, 1e-9);
        let result = ok(delete_breaklines(
            &clipped.surface,
            &[open_line(&[(2.5, 2.5), (7.5, 2.5)])],
        ));
        assert_eq!(result.report.edges_cleared, 0);
        assert!(
            result.report.edges_held >= 5,
            "the side of 5 m is cut by the lattice's vertical lines at least five times"
        );
        assert_eq!(result.surface, clipped.surface);
    }

    #[test]
    fn deleting_a_breakline_needs_a_line_with_length_and_finite_coordinates() {
        let ground = with_breakline(&flat_grid(10, 1.0, 0.0), p2(1.0, 4.0), p2(7.0, 4.0));
        let refused = |lines: &[Polyline2]| code(delete_breaklines(&ground, lines));
        assert_eq!(refused(&[]), Some(ErrorCode::InvalidArgument));
        assert_eq!(
            refused(&[open_line(&[(1.0, 4.0)])]),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            refused(&[open_line(&[(1.0, 4.0), (f64::NAN, 4.0)])]),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            refused(&[open_line(&[(1.0, 4.0), (1e12, 4.0)])]),
            Some(ErrorCode::InvalidArgument)
        );
        // Two vertices that coincide are no length at all.
        assert_eq!(
            refused(&[open_line(&[(3.0, 4.0), (3.0, 4.0)])]),
            Some(ErrorCode::InvalidGeometry)
        );
        assert_eq!(
            code(delete_breaklines(
                &TinSurface::new(),
                &[open_line(&[(1.0, 4.0), (2.0, 4.0)])]
            )),
            Some(ErrorCode::InvalidArgument)
        );
    }

    // ---- moving a point ----------------------------------------------------------------------------

    /// The plane z = 0.5 x + 0.25 y on a 4 x 4 grid of unit cells: (2,2) is at 1.5.
    fn plane4() -> TinSurface {
        grid(4, 1.0, |x, y| 0.5 * x + 0.25 * y)
    }

    #[test]
    fn a_moved_vertex_keeps_its_level_and_the_footprint_wherever_it_is_taken() {
        // The vertex (2,2) of the plane, at 1.5, taken to six places: inside its own
        // star (the hexagon (3,2) (3,3) (2,3) (1,2) (1,1) (2,1)), across the ring
        // onto a triangle beyond it, onto the ring edge x = 3, onto the diagonal it
        // is an end of, two cells away, and to the diagonal of a corner cell. In
        // each the surface is still [0,4]^2 - the area 16, no triangle overlapping
        // another or folded (TinSurface::create refuses a clockwise one, and the
        // areas of triangles that overlapped would sum to more than 16) - with the
        // same 25 vertices, the moved one at its old level (1.5), not the plane's
        // there, and every other vertex where it was.
        let ground = plane4();
        let vertex = vertex_index(&ground, 2.0, 2.0);
        assert!(vertex >= 0);
        for target in [
            p2(2.4, 1.7),
            p2(3.5, 2.2),
            p2(3.0, 2.5),
            p2(2.5, 2.5),
            p2(0.5, 0.2),
            p2(3.9, 3.9),
        ] {
            let moved = move_point(&ground, vertex as u32, target)
                .unwrap_or_else(|error| panic!("to {target:?}: {}", error.describe()));
            let after = &moved.surface;
            near(after.plan_area(), 16.0, 1e-9);
            assert_eq!(after.vertex_count(), ground.vertex_count(), "to {target:?}");
            // a polygon of n corners round k points has n + 2k - 2 triangles whoever joins them
            assert_eq!(
                after.triangle_count(),
                ground.triangle_count(),
                "to {target:?}"
            );
            assert_eq!(vertex_index(after, 2.0, 2.0), -1);
            let at = vertex_index(after, target.x, target.y);
            assert!(at >= 0, "to {target:?}");
            double_eq(after.vertices()[at as usize].z, 1.5);
            near(z_at(after, target.x, target.y), 1.5, 1e-12);
            for (other, original) in ground.vertices().iter().enumerate() {
                if other != vertex as usize {
                    assert!(
                        vertex_index(after, original.x, original.y) >= 0,
                        "vertex {other} to {target:?}"
                    );
                }
            }
            // The report: one vertex made, one taken out, one moved.
            assert_eq!(moved.report.points_added, 1);
            assert_eq!(moved.report.points_removed, 1);
            assert_eq!(moved.report.points_moved, 1);
            assert_eq!(moved.report.vertices_before, moved.report.vertices_after);
        }
    }

    #[test]
    fn a_vertex_moved_across_a_breakline_leaves_the_breakline_where_it_was() {
        // The row y = 4 from x = 1 to 7 is a breakline of six edges. The vertex
        // (4,5) above it is taken to (4.2, 3.6), below it: the breakline stays six
        // edges of one metre, and the ground its 100 m2.
        let before = with_breakline(&flat_grid(10, 1.0, 0.0), p2(1.0, 4.0), p2(7.0, 4.0));
        near(constrained_length(&before), 6.0, 1e-9);
        let vertex = vertex_index(&before, 4.0, 5.0);
        assert!(vertex >= 0);
        let moved = ok(move_point(&before, vertex as u32, p2(4.2, 3.6)));
        near(constrained_length(&moved.surface), 6.0, 1e-9);
        near(moved.surface.plan_area(), 100.0, 1e-9);
        assert_eq!(vertex_index(&moved.surface, 4.0, 5.0), -1);
        assert!(vertex_index(&moved.surface, 4.2, 3.6) >= 0);
    }

    #[test]
    fn a_vertex_cannot_be_moved_from_a_breakline_the_rim_or_a_hole_and_the_refusal_says_which() {
        let refusal = |surface: &TinSurface, x: f64, y: f64, target: Point2| {
            let vertex = vertex_index(surface, x, y);
            assert!(vertex >= 0);
            move_point(surface, vertex as u32, target)
        };
        // On a breakline: the vertex (4,4) of the row y = 4; and once the line is
        // deleted, it moves.
        let lined = with_breakline(&flat_grid(10, 1.0, 0.0), p2(1.0, 4.0), p2(7.0, 4.0));
        let on_line = refusal(&lined, 4.0, 4.0, p2(4.2, 4.3)).expect_err("refused");
        assert_eq!(on_line.code, ErrorCode::InvalidGeometry);
        assert!(on_line.message.contains("breakline"), "{}", on_line.message);
        assert!(on_line.message.contains("DELETE BREAKLINE"));
        let cleared = ok(delete_breaklines(
            &lined,
            &[open_line(&[(0.0, 4.0), (8.0, 4.0)])],
        ));
        assert!(refusal(&cleared.surface, 4.0, 4.0, p2(4.2, 4.3)).is_ok());

        // On the rim: the corner (0,0); and one between corners, (2,0).
        let ground = flat_grid(4, 1.0, 0.0);
        for rim in [p2(0.0, 0.0), p2(2.0, 0.0)] {
            let on_rim = refusal(&ground, rim.x, rim.y, p2(1.2, 1.3))
                .err()
                .unwrap_or_else(|| panic!("{rim:?} refused"));
            assert_eq!(on_rim.code, ErrorCode::InvalidGeometry);
            assert!(on_rim.message.contains("rim"), "{}", on_rim.message);
        }
        // On the edge of a hole: (3,3), a corner of [3,6] x [3,7] cut from the grid.
        let holed = ok(cut_hole(
            &flat_grid(10, 1.0, 0.0),
            &rectangle(3.0, 3.0, 6.0, 7.0),
        ));
        let on_hole = refusal(&holed.surface, 3.0, 3.0, p2(2.2, 2.3)).expect_err("refused");
        assert_eq!(on_hole.code, ErrorCode::InvalidGeometry);
        assert!(on_hole.message.contains("hole"), "{}", on_hole.message);
    }

    #[test]
    fn a_vertex_cannot_be_moved_off_the_surface_onto_another_vertex_or_without_a_finite_target() {
        let ground = plane4();
        let vertex = vertex_index(&ground, 2.0, 2.0) as u32;
        let off = move_point(&ground, vertex, p2(9.0, 9.0)).expect_err("refused");
        assert_eq!(off.code, ErrorCode::InvalidGeometry);
        assert!(off.message.contains("off the surface"), "{}", off.message);
        // Within the coincidence tolerance (1e-7) of the vertex (3,3): 5e-8 off it.
        let onto = move_point(&ground, vertex, p2(3.0, 3.00000005)).expect_err("refused");
        assert_eq!(onto.code, ErrorCode::InvalidGeometry);
        assert!(onto.message.contains("another vertex"), "{}", onto.message);
        // 2e-7 off it is clear of it.
        assert!(move_point(&ground, vertex, p2(3.0, 3.0000002)).is_ok());
        assert_eq!(
            code(move_point(&ground, vertex, p2(f64::NAN, 2.0))),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            code(move_point(&ground, vertex, p2(1e12, 2.0))),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            code(move_point(&ground, 999, p2(2.5, 2.5))),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            code(move_point(&TinSurface::new(), 0, p2(2.5, 2.5))),
            Some(ErrorCode::InvalidArgument)
        );
        // A vertex no triangle uses is nothing to move.
        let loose = make(
            vec![
                p3(0.0, 0.0, 0.0),
                p3(10.0, 0.0, 0.0),
                p3(10.0, 10.0, 0.0),
                p3(0.0, 10.0, 0.0),
                p3(5.0, 5.0, 7.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
            vec![],
        );
        assert_eq!(
            code(move_point(&loose, 4, p2(6.0, 6.0))),
            Some(ErrorCode::InvalidArgument)
        );
    }

    #[test]
    fn a_move_of_less_than_the_coincidence_tolerance_is_no_move() {
        let ground = plane4();
        let vertex = vertex_index(&ground, 2.0, 2.0) as u32;
        let stayed = ok(move_point(&ground, vertex, p2(2.00000005, 2.0)));
        assert_eq!(
            stayed.surface, ground,
            "the surface as it was, not a copy rebuilt"
        );
        assert_eq!(stayed.report.points_moved, 0);
        // And the same place exactly.
        let same = ok(move_point(&ground, vertex, p2(2.0, 2.0)));
        assert_eq!(same.surface, ground);
    }
}
