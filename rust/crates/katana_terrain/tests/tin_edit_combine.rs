//! Twin of tests/terrain/test_tin_edit_combine.cpp: surfaces made of others
//! (paste_surface, clip_to_boundary as a copy, offset_surface, merge_surfaces;
//! docs/surface_editing.md, "Mixed terrain"). The surfaces are grids of a known
//! size and level, so areas and levels are worked out by hand.

pub mod support;

use std::collections::BTreeSet;

use katana_core::ErrorCode;
use katana_geometry::{Point3, Polyline2};
use katana_terrain::tin_edit::{clip_to_boundary, merge_surfaces, offset_surface, paste_surface};
use katana_terrain::{TinEditResult, TinSurface, TinTriangle, combine_surfaces};
use support::*;

/// A flat grid at `level` over [x0, x0 + nx dx] x [y0, y0 + ny dy], each cell
/// split along its rising diagonal.
fn grid(x0: f64, y0: f64, nx: i32, ny: i32, dx: f64, dy: f64, level: f64) -> TinSurface {
    let mut vertices: Vec<Point3> = Vec::new();
    for j in 0..=ny {
        for i in 0..=nx {
            vertices.push(p3(x0 + f64::from(i) * dx, y0 + f64::from(j) * dy, level));
        }
    }
    let at = |i: i32, j: i32| (j * (nx + 1) + i) as u32;
    let mut triangles: Vec<TinTriangle> = Vec::new();
    for j in 0..ny {
        for i in 0..nx {
            triangles.push([at(i, j), at(i + 1, j), at(i + 1, j + 1)]);
            triangles.push([at(i, j), at(i + 1, j + 1), at(i, j + 1)]);
        }
    }
    TinSurface::create(vertices, triangles, vec![])
        .unwrap_or_else(|error| panic!("{}", error.describe()))
}

/// The ends of the surface's constrained edges, each once.
fn constrained_ends(surface: &TinSurface) -> Vec<Point3> {
    let mut ends: BTreeSet<u32> = BTreeSet::new();
    for (t, tri) in surface.triangles().iter().enumerate() {
        for k in 0..3 {
            if surface.is_edge_constrained(t, k) {
                ends.insert(tri[k]);
                ends.insert(tri[(k + 1) % 3]);
            }
        }
    }
    ends.into_iter()
        .map(|v| surface.vertices()[v as usize])
        .collect()
}

fn constrained_length(surface: &TinSurface) -> f64 {
    let mut seen: BTreeSet<(u32, u32)> = BTreeSet::new();
    let mut length = 0.0;
    for (t, tri) in surface.triangles().iter().enumerate() {
        for k in 0..3 {
            let a = tri[k];
            let b = tri[(k + 1) % 3];
            if surface.is_edge_constrained(t, k) && seen.insert((a.min(b), a.max(b))) {
                let p = surface.vertices()[a as usize];
                let q = surface.vertices()[b as usize];
                length += (q.x - p.x).hypot(q.y - p.y);
            }
        }
    }
    length
}

fn on_rectangle_rim(p: Point3, x0: f64, y0: f64, x1: f64, y1: f64) -> bool {
    const NEAR: f64 = 1e-9;
    let along_x = (x0 - NEAR..=x1 + NEAR).contains(&p.x);
    let along_y = (y0 - NEAR..=y1 + NEAR).contains(&p.y);
    (along_y && ((p.x - x0).abs() <= NEAR || (p.x - x1).abs() <= NEAR))
        || (along_x && ((p.y - y0).abs() <= NEAR || (p.y - y1).abs() <= NEAR))
}

fn ok(result: katana_core::Result<TinEditResult>) -> TinEditResult {
    result.unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn z_at(surface: &TinSurface, x: f64, y: f64) -> f64 {
    surface
        .elevation_at_xy(x, y)
        .unwrap_or_else(|| panic!("no level at {x}, {y}"))
}

fn make(vertices: Vec<Point3>, triangles: Vec<TinTriangle>) -> TinSurface {
    TinSurface::create(vertices, triangles, vec![])
        .unwrap_or_else(|error| panic!("{}", error.describe()))
}

mod tin_edit_combine {
    use super::*;

    #[test]
    fn a_pasted_pad_replaces_the_ground_inside_its_footprint_with_a_clean_seam() {
        // Ground: [0,100]^2 at 10 in 10 m cells. Pad: [33,67] x [37,63] at 15,
        // 2 x 2 cells, its rim off the ground's grid lines so the seam has to cut
        // the ground's triangles.
        let ground = grid(0.0, 0.0, 10, 10, 10.0, 10.0, 10.0);
        let pad = grid(33.0, 37.0, 2, 2, 17.0, 13.0, 15.0);
        let pasted = ok(paste_surface(&ground, &pad));
        let result = &pasted.surface;

        // No gap and no overlap: the plan area is the ground's.
        near(result.plan_area(), 10000.0, 1e-6);
        // The pad inside its footprint, the ground well away from it.
        let mut x = 34.0;
        while x < 67.0 {
            let mut y = 38.0;
            while y < 63.0 {
                double_eq(z_at(result, x, y), 15.0);
                y += 2.0;
            }
            x += 2.0;
        }
        double_eq(z_at(result, 5.0, 5.0), 10.0);
        double_eq(z_at(result, 95.0, 95.0), 10.0);
        // The seam is the pad's perimeter, 2 (34 + 26) = 120 m, made constrained,
        // and every vertex on it lies on the pad's rim at the pad's level.
        near(constrained_length(result), 120.0, 1e-9);
        let seam = constrained_ends(result);
        assert!(!seam.is_empty());
        for v in &seam {
            assert!(
                on_rectangle_rim(*v, 33.0, 37.0, 67.0, 63.0),
                "seam vertex at {}, {}",
                v.x,
                v.y
            );
            double_eq(v.z, 15.0);
        }
        // Nothing but the two levels: the ground's vertices keep theirs.
        for v in result.vertices() {
            assert!(v.z == 10.0 || v.z == 15.0, "vertex at {}, {}", v.x, v.y);
        }
        assert_eq!(pasted.report.triangles_before, 200);
    }

    #[test]
    fn a_pasted_top_is_kept_where_it_reaches_past_the_base() {
        // The top [80,120] x [40,60] at 12 hangs 20 m past the base's edge at
        // x = 100: the union is 10000 + 20 x 20 = 10400.
        let base = grid(0.0, 0.0, 10, 10, 10.0, 10.0, 10.0);
        let top = grid(80.0, 40.0, 2, 2, 20.0, 10.0, 12.0);
        let pasted = ok(paste_surface(&base, &top));
        near(pasted.surface.plan_area(), 10400.0, 1e-6);
        double_eq(z_at(&pasted.surface, 110.0, 50.0), 12.0);
        double_eq(z_at(&pasted.surface, 90.0, 50.0), 12.0);
        double_eq(z_at(&pasted.surface, 50.0, 50.0), 10.0);
    }

    #[test]
    fn pasting_an_empty_surface_is_refused() {
        let base = grid(0.0, 0.0, 2, 2, 10.0, 10.0, 10.0);
        assert_eq!(
            paste_surface(&base, &TinSurface::new())
                .err()
                .map(|error| error.code),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            paste_surface(&TinSurface::new(), &base)
                .err()
                .map(|error| error.code),
            Some(ErrorCode::InvalidArgument)
        );
    }

    #[test]
    fn a_clip_is_the_exact_part_inside_the_polygon() {
        // The triangle (1,1) (9,1) (5,8) out of a 10 x 10 grid: area 8 x 7 / 2 = 28.
        let surface = grid(0.0, 0.0, 10, 10, 1.0, 1.0, 3.0);
        let polygon = Polyline2::new(vec![p2(1.0, 1.0), p2(9.0, 1.0), p2(5.0, 8.0)], false);
        let clipped = ok(clip_to_boundary(&surface, &polygon));
        near(clipped.surface.plan_area(), 28.0, 1e-9);
        double_eq(z_at(&clipped.surface, 5.0, 3.0), 3.0);
        assert!(clipped.surface.elevation_at_xy(1.0, 7.0).is_none());
    }

    #[test]
    fn an_offset_copy_is_raised_by_exactly_dz() {
        let surface = grid(0.0, 0.0, 3, 3, 2.0, 2.0, 7.25);
        let offset = ok(offset_surface(&surface, -2.5));
        assert_eq!(offset.surface.vertex_count(), surface.vertex_count());
        for (after, before) in offset.surface.vertices().iter().zip(surface.vertices()) {
            assert_eq!(after.z, before.z - 2.5);
        }
        assert_eq!(offset.surface.triangles(), surface.triangles());
    }

    #[test]
    fn a_merge_lets_the_later_surface_win() {
        // combine_surfaces' rule: where the pad can give a level, the pad's.
        let ground = grid(0.0, 0.0, 10, 10, 10.0, 10.0, 10.0);
        let pad = grid(40.0, 40.0, 4, 4, 5.0, 5.0, 15.0);
        let merged = ok(merge_surfaces(&[Some(&ground), Some(&pad)]));
        double_eq(z_at(&merged.surface, 50.0, 50.0), 15.0);
        double_eq(z_at(&merged.surface, 5.0, 5.0), 10.0);
        // The same answer combine_surfaces gives wherever either says anything.
        let direct = combine_surfaces(&[Some(&ground), Some(&pad)])
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let mut x = 2.5;
        while x < 100.0 {
            let mut y = 2.5;
            while y < 100.0 {
                assert_eq!(
                    merged.surface.elevation_at_xy(x, y),
                    direct.elevation_at_xy(x, y),
                    "at {x}, {y}"
                );
                y += 5.0;
            }
            x += 5.0;
        }
        assert_eq!(
            merged.report.triangles_before,
            ground.triangle_count() + pad.triangle_count()
        );
    }

    #[test]
    fn a_merge_of_fewer_than_two_surfaces_or_an_empty_one_is_refused() {
        let ground = grid(0.0, 0.0, 2, 2, 10.0, 10.0, 10.0);
        let empty = TinSurface::new();
        assert_eq!(
            merge_surfaces(&[Some(&ground)])
                .err()
                .map(|error| error.code),
            Some(ErrorCode::InvalidArgument)
        );
        assert_eq!(
            merge_surfaces(&[Some(&ground), Some(&empty)])
                .err()
                .map(|error| error.code),
            Some(ErrorCode::InvalidArgument)
        );
    }

    #[test]
    fn a_paste_counts_each_new_vertex_once_centroid_fans_included() {
        // Ground: [0,4]^2 at 0 in unit cells, 25 vertices all used. Top: the
        // triangle (0.5,0.5) (3.5,0.7) (2,3.3) at 5, whose three sides each cross
        // grid lines (the first x = 1, 2, 3; the others y = 1, 2, 3), so the paste
        // fans it from its centroid, (2, 1.5) at 5 - a vertex the result has and
        // the ground did not. points_added said one more than the vertices there
        // are for every such fan: the vertices the result uses beyond the ground's
        // are what was added, and the ground's that it no longer uses what was
        // removed, whatever made them.
        let ground = grid(0.0, 0.0, 4, 4, 1.0, 1.0, 0.0);
        let top = make(
            vec![p3(0.5, 0.5, 5.0), p3(3.5, 0.7, 5.0), p3(2.0, 3.3, 5.0)],
            vec![[0, 1, 2]],
        );
        let pasted = ok(paste_surface(&ground, &top));
        let report = &pasted.report;
        assert_eq!(report.vertices_before, 25);
        assert_eq!(
            report.vertices_after + report.points_removed,
            report.vertices_before + report.points_added
        );

        let has_centre = pasted.surface.vertices().iter().any(|v| {
            (v.x - 2.0).abs() < 1e-9 && (v.y - 1.5).abs() < 1e-9 && (v.z - 5.0).abs() < 1e-9
        });
        assert!(has_centre, "the fan's centre is a vertex of the result");
        near(pasted.surface.plan_area(), 16.0, 1e-9);
    }

    #[test]
    fn two_clips_of_one_surface_that_abut_merge_into_all_of_it() {
        // A [0,10]^2 surface at 3 in unit cells clipped at x = 5 (along grid lines)
        // and at x = 4.3 (across the triangles, the seam's vertices computed): the
        // two halves share a seam and neither covers any of the other. The merge
        // is both whole - their triangles, the area 100 - where it dropped the
        // row of the lower half's triangles along the seam.
        let ground = grid(0.0, 0.0, 10, 10, 1.0, 1.0, 3.0);
        let a_box = |x0: f64, x1: f64| {
            Polyline2::new(
                vec![p2(x0, -1.0), p2(x1, -1.0), p2(x1, 11.0), p2(x0, 11.0)],
                true,
            )
        };
        for seam in [5.0, 4.3] {
            let west = ok(clip_to_boundary(&ground, &a_box(-1.0, seam)));
            let east = ok(clip_to_boundary(&ground, &a_box(seam, 11.0)));
            near(
                west.surface.plan_area() + east.surface.plan_area(),
                100.0,
                1e-9,
            );
            for members in [
                [Some(&west.surface), Some(&east.surface)],
                [Some(&east.surface), Some(&west.surface)],
            ] {
                let merged = ok(merge_surfaces(&members));
                assert_eq!(
                    merged.surface.triangle_count(),
                    west.surface.triangle_count() + east.surface.triangle_count(),
                    "seam at x = {seam}"
                );
                near(merged.surface.plan_area(), 100.0, 1e-9);
                double_eq(z_at(&merged.surface, 2.0, 5.0), 3.0);
                double_eq(z_at(&merged.surface, 8.0, 5.0), 3.0);
            }
        }
    }

    #[test]
    fn a_paste_whose_top_has_two_rim_vertices_closer_than_the_tolerance_says_so_not_which_triangle()
    {
        // The top is the rectangle [2.3, 12.3] x [3.7, 13.7] fanned from its
        // corner 0, and its upper side runs through two vertices 1e-9 m apart,
        // (8.6, 13.7) and (8.600000001, 13.7) - closer than the coincidence
        // tolerance, 1e-7 m, so the patch takes them for one point and the sliver
        // between them, the triangle {0, 5, 4}, names it twice. TinSurface::create
        // refused that as "triangle names a vertex twice", with the number of a
        // triangle of the result: the triangle, not the cause. Counter-clockwise,
        // worked out from corner 0:
        // {0,2,5} turns by (10,10) x (6.300000001,10) = 37.0 > 0, {0,5,4} by
        // (6.300000001,10) x (6.3,10) = 1e-8 > 0, {0,4,3} by (6.3,10) x (0,10) = 63.
        let ground = grid(0.0, 0.0, 20, 20, 1.0, 1.0, 0.0);
        let top = make(
            vec![
                p3(2.3, 3.7, 1.0),
                p3(12.3, 3.7, 1.0),
                p3(12.3, 13.7, 1.0),
                p3(2.3, 13.7, 1.0),
                p3(8.6, 13.7, 1.0),
                p3(8.600000001, 13.7, 1.0),
            ],
            vec![[0, 1, 2], [0, 2, 5], [0, 5, 4], [0, 4, 3]],
        );
        let error = paste_surface(&ground, &top).expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidGeometry);
        assert!(error.message.contains("rim"), "{}", error.message);
        assert!(
            error.message.contains("coincidence tolerance"),
            "{}",
            error.message
        );
        assert!(
            !error.message.contains("names a vertex twice"),
            "{}",
            error.message
        );
        // And which vertices, where.
        assert!(error.context.contains("(8.6, 13.7)"), "{}", error.context);
        assert!(
            error.context.contains("(8.600000001, 13.7)"),
            "{}",
            error.context
        );
    }

    #[test]
    fn a_top_whose_rim_touches_itself_at_a_duplicated_vertex_is_still_pasted() {
        // Two triangles meeting at a point that each give a vertex of their own,
        // (7.3, 8.7) and (7.300000001, 8.7): they merge into one vertex of the
        // result, but no top triangle has both, so nothing is degenerate and the
        // paste is as good as any. Each triangle turns by 50 > 0: (5,5) x (0,10)
        // and (5,-5) x (5,5). The ground [0,20]^2 is covered all the same: 400.
        let ground = grid(0.0, 0.0, 20, 20, 1.0, 1.0, 0.0);
        let top = make(
            vec![
                p3(2.3, 3.7, 1.0),
                p3(7.3, 8.7, 1.0),
                p3(2.3, 13.7, 1.0),
                p3(7.300000001, 8.7, 1.0),
                p3(12.3, 3.7, 1.0),
                p3(12.3, 13.7, 1.0),
            ],
            vec![[0, 1, 2], [3, 4, 5]],
        );
        let pasted = ok(paste_surface(&ground, &top));
        near(pasted.surface.plan_area(), 400.0, 1e-9);
        let at_the_pinch = pasted
            .surface
            .vertices()
            .iter()
            .filter(|v| (v.x - 7.3).abs() < 1e-6 && (v.y - 8.7).abs() < 1e-6)
            .count();
        assert_eq!(at_the_pinch, 1, "the two vertices are one");
        near(z_at(&pasted.surface, 4.0, 8.7), 1.0, 1e-12);
    }
}
