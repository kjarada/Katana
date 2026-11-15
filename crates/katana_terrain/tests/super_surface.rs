//! Twin of tests/terrain/test_super_surface.cpp: combining surfaces in rank
//! order, the archive format's super tin.
//!
//! The surfaces here are GRIDS rather than a sheet of two huge triangles,
//! because that is what a super tin is made of in practice - a survey tin of
//! thousands of small triangles with a design pad over it - and the rule
//! these tests pin (a triangle of a lower member is dropped whole if a higher
//! member covers any AREA of it) only behaves well when the pad is larger than
//! the triangles it covers.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

pub mod support;

use katana_geometry::{Point2, Point3};
use katana_terrain::{TinSurface, TinTriangle, combine_surfaces};
use support::*;

/// A flat grid at `level` over [x0,x1] x [y0,y1], `cells` squares each way,
/// every square split into two triangles.
fn grid(x0: f64, y0: f64, x1: f64, y1: f64, level: f64, cells: i32) -> TinSurface {
    let mut vertices: Vec<Point3> = Vec::new();
    let dx = (x1 - x0) / f64::from(cells);
    let dy = (y1 - y0) / f64::from(cells);
    for j in 0..=cells {
        for i in 0..=cells {
            vertices.push(p3(x0 + f64::from(i) * dx, y0 + f64::from(j) * dy, level));
        }
    }
    let index = |i: i32, j: i32| (j * (cells + 1) + i) as u32;
    let mut triangles: Vec<TinTriangle> = Vec::new();
    for j in 0..cells {
        for i in 0..cells {
            triangles.push([index(i, j), index(i + 1, j), index(i + 1, j + 1)]);
            triangles.push([index(i, j), index(i + 1, j + 1), index(i, j + 1)]);
        }
    }
    TinSurface::create(vertices, triangles, vec![])
        .unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn combine(members: &[&TinSurface]) -> TinSurface {
    let members: Vec<Option<&TinSurface>> = members.iter().copied().map(Some).collect();
    combine_surfaces(&members).unwrap_or_else(|error| panic!("{}", error.describe()))
}

/// How far p lies to the inside (the left) of the directed edge from -> to.
fn inward_distance(from: Point3, to: Point3, p: Point3) -> f64 {
    let length = (to.x - from.x).hypot(to.y - from.y);
    ((to.x - from.x) * (p.y - from.y) - (to.y - from.y) * (p.x - from.x)) / length
}

/// Whether a line along an edge of `t` parts `u` from it: every vertex of u on
/// the outer side of the line, or within the coincidence tolerance of it.
fn parted_by_an_edge_of(t: &[Point3; 3], u: &[Point3; 3]) -> bool {
    const TOLERANCE: f64 = 1e-7; // math::tolerance::GEOMETRIC
    for k in 0..3 {
        let all_outside = u
            .iter()
            .all(|&vertex| inward_distance(t[k], t[(k + 1) % 3], vertex) <= TOLERANCE);
        if all_outside {
            return true;
        }
    }
    false
}

/// The rule, stated the plain way and pair by pair, with no index and no
/// bounding box: a triangle is covered when some triangle of the higher member
/// shares an AREA with it - two counter-clockwise triangles share none when an
/// edge line of the one parts them (the separating axis theorem), which is also
/// what touching along an edge or at a corner is.
fn covers_exhaustively(higher: &TinSurface, a: Point3, b: Point3, c: Point3) -> bool {
    higher.triangles().iter().any(|tri| {
        let v = higher.vertices();
        let u = [v[tri[0] as usize], v[tri[1] as usize], v[tri[2] as usize]];
        let t = [a, b, c];
        !parted_by_an_edge_of(&t, &u) && !parted_by_an_edge_of(&u, &t)
    })
}

/// The cheap half the old rule had: the triangle's own corners and middle
/// against the higher surface. A triangle that only the pairwise test drops is
/// one this misses.
fn sample_inside_exhaustively(higher: &TinSurface, a: Point3, b: Point3, c: Point3) -> bool {
    let samples = [
        p2(a.x, a.y),
        p2(b.x, b.y),
        p2(c.x, c.y),
        p2((a.x + b.x + c.x) / 3.0, (a.y + b.y + c.y) / 3.0),
    ];
    samples
        .iter()
        .any(|&sample| higher.elevation_at(sample).is_some())
}

/// A band two vertices deep: `columns` + 1 across, 2 up. Coarse across and thin
/// up the page, which is what puts a lone strip of it across a base triangle,
/// between its corners and its middle, with its vertices ON the triangle's
/// edges.
fn band(x0: f64, y0: f64, x1: f64, y1: f64, level: f64, columns: i32) -> TinSurface {
    let mut vertices: Vec<Point3> = Vec::new();
    let dx = (x1 - x0) / f64::from(columns);
    for j in 0..2 {
        for i in 0..=columns {
            vertices.push(p3(
                x0 + f64::from(i) * dx,
                if j == 0 { y0 } else { y1 },
                level,
            ));
        }
    }
    let mut triangles: Vec<TinTriangle> = Vec::new();
    for i in 0..columns {
        let lower = i as u32;
        let upper = (columns + 1 + i) as u32;
        triangles.push([lower, lower + 1, upper + 1]);
        triangles.push([lower, upper + 1, upper]);
    }
    TinSurface::create(vertices, triangles, vec![])
        .unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn at(surface: &TinSurface, x: f64, y: f64) -> Option<f64> {
    surface.elevation_at(Point2::new(x, y))
}

mod super_surface {
    use super::*;

    #[test]
    fn a_pad_substitutes_the_base_over_its_own_footprint_and_nowhere_else() {
        // A 100 x 100 natural surface at 10 in 10 m triangles, with a 20 x 20
        // pad at 15 in the middle of it - the sample archive's "COMBINED
        // SURFACE", which names the base first.
        let base = grid(0.0, 0.0, 100.0, 100.0, 10.0, 10);
        let pad = grid(40.0, 40.0, 60.0, 60.0, 15.0, 2);
        let combined = combine(&[&base, &pad]);

        assert_eq!(
            at(&combined, 50.0, 50.0),
            Some(15.0),
            "inside the pad, the pad"
        );
        assert_eq!(at(&combined, 45.0, 55.0), Some(15.0));
        assert_eq!(
            at(&combined, 5.0, 5.0),
            Some(10.0),
            "well away from it, the base"
        );
        assert_eq!(at(&combined, 95.0, 95.0), Some(10.0));
        assert_eq!(at(&combined, 50.0, 5.0), Some(10.0));
        // Off both of them there is no answer at all, rather than a zero.
        assert!(at(&combined, -5.0, -5.0).is_none());
    }

    #[test]
    fn no_point_ever_has_two_answers_which_is_the_whole_point_of_the_rule() {
        // The defect this rule exists to prevent: two sheets over one point make
        // elevation_at() answer with whichever triangle it meets first, which is
        // the base as often as the pad. Everywhere the pad covers, the combined
        // surface must give the PAD's level and never the base's.
        // The pad does NOT line up with the base's grid: 45 to 55 over 10 m
        // triangles, so a base triangle like (40,40)-(50,40)-(50,50) has its
        // middle outside the pad while part of it lies within. Dropping only
        // the triangles whose CENTRE is covered would leave that one, and the
        // base would answer at (48, 46) - inside the pad.
        let base = grid(0.0, 0.0, 100.0, 100.0, 10.0, 10);
        let pad = grid(45.0, 45.0, 55.0, 55.0, 15.0, 1);
        let combined = combine(&[&base, &pad]);

        let mut inside_pad = 0;
        let mut x = 45.5;
        while x < 55.0 {
            let mut y = 45.5;
            while y < 55.0 {
                let level = at(&combined, x, y).unwrap_or_else(|| panic!("{x},{y}"));
                assert_eq!(level, 15.0, "the base answered at {x},{y}");
                inside_pad += 1;
                y += 0.5;
            }
            x += 0.5;
        }
        assert!(
            inside_pad > 300,
            "the sweep must actually reach inside the pad"
        );
    }

    #[test]
    fn the_last_member_wins_which_is_what_the_order_means() {
        // The comparison to reverse if the format turns out to rank the list the
        // other way: the manual does not say, and this follows the sample
        // archive's own comment ("base surface first, then the pads that
        // substitute it over their footprints").
        let first = grid(0.0, 0.0, 10.0, 10.0, 1.0, 2);
        let second = grid(0.0, 0.0, 10.0, 10.0, 2.0, 2);
        let later = combine(&[&first, &second]);
        assert_eq!(at(&later, 5.0, 5.0), Some(2.0));

        let reversed = combine(&[&second, &first]);
        assert_eq!(at(&reversed, 5.0, 5.0), Some(1.0));
    }

    #[test]
    fn the_seam_is_ragged_outward_by_at_most_one_triangle_of_the_lower_member() {
        // The documented approximation, pinned so that an exact clip later is a
        // deliberate change. A base triangle that the pad OVERLAPS goes whole, so
        // a ring of ground just outside the pad has no level - which is honest,
        // where a wrong level would not be.
        let base = grid(0.0, 0.0, 100.0, 100.0, 10.0, 10);
        let pad = grid(45.0, 45.0, 55.0, 55.0, 15.0, 1);
        let combined = combine(&[&base, &pad]);

        assert_eq!(at(&combined, 50.0, 50.0), Some(15.0));
        // The base's 10 m triangles touching the pad went with it: the hole
        // reaches the grid line at 40, one triangle outside the pad's 45.
        assert!(
            at(&combined, 44.0, 50.0).is_none(),
            "inside the dropped base triangle, outside the pad"
        );
        assert_eq!(
            at(&combined, 35.0, 50.0),
            Some(10.0),
            "one triangle further out the base is untouched"
        );
    }

    #[test]
    fn empty_and_missing_members_are_not_an_error() {
        // A super tin whose member tins the archive did not carry is a fact
        // about the file, not a failure of the reader.
        let none = combine_surfaces(&[]).expect("no error");
        assert!(none.empty());

        let base = grid(0.0, 0.0, 10.0, 10.0, 1.0, 2);
        let nothing = TinSurface::new();
        let some = combine_surfaces(&[None, Some(&base), Some(&nothing)]).expect("no error");
        assert_eq!(some.triangle_count(), base.triangle_count());
        assert_eq!(at(&some, 5.0, 5.0), Some(1.0));
    }

    #[test]
    fn combining_is_pure_and_leaves_the_members_as_they_were() {
        let base = grid(0.0, 0.0, 100.0, 100.0, 10.0, 10);
        let pad = grid(40.0, 40.0, 60.0, 60.0, 15.0, 2);
        let before = base.triangle_count();
        let _combined = combine(&[&base, &pad]);
        assert_eq!(base.triangle_count(), before);
        assert_eq!(
            at(&base, 50.0, 50.0),
            Some(10.0),
            "the base still answers for its own ground"
        );
    }

    // ---- the overlap test is indexed, and must answer what the pairwise scan answered -----

    #[test]
    fn the_indexed_overlap_test_keeps_exactly_the_triangles_the_pairwise_scan_kept() {
        // The pads are FINE grids over a coarse base. That is what makes this test
        // sharp: the bucket grid sizes its cells for about two triangles each, so a
        // 1250-triangle pad over 30 m has cells of a metre or so, and a query box
        // even slightly too small loses triangles the scan would have found.
        //
        // One pad is placed off the base's 10 m grid lines, so its triangles fall
        // across the middle of base triangles; one is placed exactly ON them, which
        // is the case where touching along an edge must not count.
        let base = grid(0.0, 0.0, 100.0, 100.0, 10.0, 10);
        let off_grid = grid(33.3, 41.7, 63.3, 71.7, 15.0, 25);
        let on_grid = grid(10.0, 10.0, 40.0, 40.0, 12.0, 20);
        // The knife edge. This band crosses the base's triangles between y = 47 and
        // y = 48, missing every corner and every centroid of them, and its vertices
        // are 10 m apart across - so a base triangle like (40,40)-(50,50)-(40,50)
        // has the band's vertices (40,47) and (40,48) ON its left edge and the band
        // between them running across it. Nothing but the pair test answers there.
        let knife_edge = band(0.0, 47.0, 100.0, 48.0, 20.0, 10);
        let members = [&base, &off_grid, &on_grid, &knife_edge];

        let combined = combine(&members);

        let mut expected: Vec<[Point3; 3]> = Vec::new();
        let mut dropped = 0;
        let mut dropped_by_the_pair_test_alone = 0;
        for (rank, member) in members.iter().enumerate() {
            for triangle in member.triangles() {
                let a = member.vertices()[triangle[0] as usize];
                let b = member.vertices()[triangle[1] as usize];
                let c = member.vertices()[triangle[2] as usize];
                let mut overridden = false;
                for higher in &members[rank + 1..] {
                    if !covers_exhaustively(higher, a, b, c) {
                        continue;
                    }
                    overridden = true;
                    if !sample_inside_exhaustively(higher, a, b, c) {
                        dropped_by_the_pair_test_alone += 1;
                    }
                    break;
                }
                if overridden {
                    dropped += 1;
                } else {
                    expected.push([a, b, c]);
                }
            }
        }

        // A cull that never fires proves nothing, and neither does one that only
        // the cheap sample test ever answers: triangles must exist that ONLY the
        // pair test drops. The knife-edge band crosses the base's row of cells at
        // y = 40..50; the six of them at x = 0..30 and 70..100 are reached by no
        // other pad (the off-grid one covers 30..70, the on-grid one only touches
        // the row along y = 40), and the band is the one thing over their two
        // triangles each: twelve.
        assert!(dropped > 20);
        assert!(dropped_by_the_pair_test_alone >= 12);

        assert_eq!(combined.triangle_count(), expected.len());
        for (t, want_triangle) in expected.iter().enumerate() {
            let triangle = combined.triangles()[t];
            for k in 0..3 {
                let got = combined.vertices()[triangle[k] as usize];
                let want = want_triangle[k];
                assert_eq!(got.x, want.x, "triangle {t} corner {k}");
                assert_eq!(got.y, want.y, "triangle {t} corner {k}");
                assert_eq!(got.z, want.z, "triangle {t} corner {k}");
            }
        }
    }

    #[test]
    fn tiles_that_only_abut_are_both_kept() {
        // Two 10 x 10 squares at 1 and 2 sharing the edge x = 10: nothing of
        // either is covered by the other, so the merge is both - four triangles,
        // area 200 - in either order. The lower one's two triangles used to go
        // with the first, because the higher member's rim vertices are ON their
        // edge and the boundary counted as inside.
        let west = grid(0.0, 0.0, 10.0, 10.0, 1.0, 1);
        let east = grid(10.0, 0.0, 20.0, 10.0, 2.0, 1);
        for members in [[&west, &east], [&east, &west]] {
            let combined = combine(&members);
            assert_eq!(combined.triangle_count(), 4);
            near(combined.plan_area(), 200.0, 1e-12);
            assert_eq!(at(&combined, 5.0, 5.0), Some(1.0));
            assert_eq!(at(&combined, 15.0, 5.0), Some(2.0));
        }
        // Touching at one corner is touching too.
        let diagonal = grid(10.0, 10.0, 20.0, 20.0, 3.0, 1);
        let corner = combine(&[&west, &diagonal]);
        assert_eq!(corner.triangle_count(), 4);
    }

    #[test]
    fn an_overlap_thinner_than_the_tolerance_is_touching_and_one_beyond_it_is_cover() {
        let base = grid(0.0, 0.0, 10.0, 10.0, 1.0, 1);
        // A pad overlapping by 1e-9 (rounding of the coordinates of a seam made
        // twice): the two triangles of the base stay and the pad's two are added.
        let sliver = grid(10.0 - 1e-9, 0.0, 20.0, 10.0, 2.0, 1);
        let kept = combine(&[&base, &sliver]);
        assert_eq!(kept.triangle_count(), 4);
        // A pad overlapping by 1 m: x in [9,10] is shared by both of the base's
        // triangles ((9.5,0.5) is in the lower, (9.5,9.8) in the upper), and the
        // base is covered where the pad reaches - only the pad's two remain.
        let overlapping = grid(9.0, 0.0, 20.0, 10.0, 2.0, 1);
        let covered = combine(&[&base, &overlapping]);
        assert_eq!(covered.triangle_count(), 2);
        // The pad is flat at 2: barycentric weights sum to one within a few ulps
        // (2.2e-16 each at this size), so the height is 2 to 1e-12, not bit for bit.
        near(at(&covered, 9.5, 0.5).expect("covered"), 2.0, 1e-12);
        near(at(&covered, 9.5, 9.8).expect("covered"), 2.0, 1e-12);
    }

    #[test]
    fn a_top_that_touches_a_face_only_at_a_point_on_its_edge_leaves_the_face_be() {
        // A pyramid on [0,20]^2 with its apex at (10,10,5) and a flat top 15..25 x
        // 5..15 at 3. The top's corners (15,5) and (15,15) are on the edges of the
        // south and north faces (the diagonals from the corners to the apex:
        // (20,0)-(10,10) holds (15,5), (10,10)-(20,20) holds (15,15)) and the top
        // reaches those faces at no other point. The east face is under it. Three
        // faces of 100 and the top's 100: 400, not the 200 the faces touched at a
        // point used to leave.
        let pyramid = TinSurface::create(
            vec![
                p3(0.0, 0.0, 0.0),
                p3(20.0, 0.0, 0.0),
                p3(20.0, 20.0, 0.0),
                p3(0.0, 20.0, 0.0),
                p3(10.0, 10.0, 5.0),
            ],
            vec![[0, 1, 4], [1, 2, 4], [2, 3, 4], [3, 0, 4]],
            vec![],
        )
        .expect("a pyramid");
        let top = TinSurface::create(
            vec![
                p3(15.0, 5.0, 3.0),
                p3(25.0, 5.0, 3.0),
                p3(25.0, 15.0, 3.0),
                p3(15.0, 15.0, 3.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
            vec![],
        )
        .expect("a top");
        let combined = combine(&[&pyramid, &top]);
        assert_eq!(combined.triangle_count(), 5);
        near(combined.plan_area(), 400.0, 1e-9);
        assert!(at(&combined, 10.0, 3.0).is_some(), "the south face");
        assert!(at(&combined, 10.0, 17.0).is_some(), "the north face");
        assert_eq!(
            at(&combined, 18.0, 10.0),
            Some(3.0),
            "the east face is the top's"
        );
    }

    #[test]
    fn a_vertex_on_the_far_side_of_the_pad_from_the_triangle_is_not_mistaken_for_one_inside() {
        // The grid query is a box around the triangle; a pad whose vertices are all
        // far outside that box must leave the triangle alone even though the pad's
        // whole-surface bounding box overlaps the base. A pad along one edge of the
        // base tests exactly that: the base triangles at the opposite edge share the
        // pad's bounding box in one axis and nothing else.
        let base = grid(0.0, 0.0, 100.0, 100.0, 10.0, 10);
        let strip = grid(0.0, 0.0, 100.0, 10.0, 15.0, 10);
        let combined = combine(&[&base, &strip]);

        assert_eq!(
            at(&combined, 50.0, 5.0),
            Some(15.0),
            "the strip owns the bottom band"
        );
        assert_eq!(
            at(&combined, 50.0, 95.0),
            Some(10.0),
            "and the base still owns the far edge"
        );
    }
}
