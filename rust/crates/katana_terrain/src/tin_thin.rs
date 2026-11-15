//! Thinning a surface (tin_edit, thin_surface; docs/surface_editing.md,
//! "Thinning with a bound that holds").
//!
//! A vertex is removed by re-triangulating the polygon of its neighbours (its
//! "link") with the one constrained Delaunay kernel. What the removal changes
//! is measured exactly: the surfaces before and after are both linear on every
//! piece of their overlay, so the largest difference is at a vertex of the
//! overlay - the vertex removed itself, or where a new diagonal of the link
//! crosses an old spoke to the vertex (the link's own corners do not move).
//! Each triangle carries a bound on how far it lies from the ORIGINAL surface;
//! a removal's new triangles inherit the largest bound of the star they
//! replace plus the change it makes. So a vertex goes only when that sum is
//! within the tolerance, and the bound holds for the whole result, not for
//! each step alone.

use katana_core::task_pool::TaskPool;
use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result};
use katana_geometry::Triangle2;
use katana_math::cpp;
use katana_math::numerics::{lerp, tolerance};

use crate::cdt_backend::{self, CdtConstraint, CdtInput, NO_INDEX};
use crate::tin_edit::{TinEditResult, held_vertices};
use crate::tin_patch::{VertexStars, finish_surface, report_of, unchanged};
use crate::tin_surface::{Point3, TinSurface, TinTriangle, compute_by_index, plan_of};

/// Candidates per parallel chunk of the first measurement: each is a small
/// triangulation (tens of microseconds), so 64 is a few milliseconds of work,
/// far above the cost of handing a chunk to a thread.
const THIN_GRAIN: usize = 64;

struct Removal {
    /// the link re-triangulated
    triangles: Vec<TinTriangle>,
    masks: Vec<u8>,
    /// Per new triangle, a bit for each edge that is a side of the link (the
    /// others are new diagonals).
    sides: Vec<u8>,
    /// the most the surface moves, vertically, anywhere in the star
    change: f64,
}

#[derive(Clone, Copy)]
struct LinkEdge {
    from: u32,
    to: u32,
    flag: u8,
}

/// Removing `v`, whose live triangles are `star`, from the triangulation in
/// `triangles`: None when it cannot be removed cleanly - on the rim (its
/// link does not close), a star that is not a simple fan, or a link the kernel
/// has to split or add to.
fn removal_of(
    v: u32,
    vertices: &[Point3],
    triangles: &[TinTriangle],
    masks: &[u8],
    star: &[u32],
) -> Option<Removal> {
    let m = star.len();
    if m < 3 {
        return None;
    }
    let mut link: Vec<LinkEdge> = Vec::with_capacity(m);
    for &t in star {
        let tri = triangles[t as usize];
        let r = tri.iter().position(|&corner| corner == v)?;
        let edge = (r + 1) % 3; // the side opposite v
        link.push(LinkEdge {
            from: tri[edge],
            to: tri[(edge + 1) % 3],
            flag: (masks[t as usize] >> edge) & 1,
        });
    }
    // The link as a ring, in order: v lies on the left of every edge.
    let mut ring: Vec<u32> = vec![link[0].from];
    let mut flags: Vec<u8> = Vec::new();
    let mut used_edge = vec![0u8; m];
    for _ in 0..m {
        let at = ring[ring.len() - 1];
        // the link does not close: v is on the rim
        let found = (0..m).find(|&e| used_edge[e] == 0 && link[e].from == at)?;
        used_edge[found] = 1;
        flags.push(link[found].flag);
        ring.push(link[found].to);
    }
    if ring[ring.len() - 1] != ring[0] {
        return None;
    }
    ring.pop();
    let mut sorted = ring.clone();
    sorted.sort_unstable();
    if sorted.windows(2).any(|pair| pair[0] == pair[1]) {
        return None; // a vertex twice round the link: not a simple fan
    }

    let positions: Vec<_> = ring
        .iter()
        .map(|&r| plan_of(vertices[r as usize]))
        .collect();
    let constraints: Vec<CdtConstraint> = (0..m)
        .map(|i| CdtConstraint {
            from: i as u32,
            to: ((i + 1) % m) as u32,
            tag: 1,
        })
        .collect();
    let cdt = cdt_backend::triangulate(&CdtInput {
        points: &positions,
        constraints: &constraints,
        export_constraint_vertices: true,
    })
    .ok()?;
    if !cdt.generated_points.is_empty() {
        return None;
    }
    for c in 0..m {
        if cdt.constraint_vertex_offsets[c + 1] - cdt.constraint_vertex_offsets[c] != 2 {
            return None;
        }
    }
    let count = cdt.triangles.len();
    let mut inside = vec![0u8; count];
    let mut queue: Vec<u32> = Vec::new();
    for (t, flag) in inside.iter_mut().enumerate() {
        let tri = cdt.triangles[t];
        for k in 0..3 {
            if cdt.edge_tags[t][k] != 0 && tri[(k + 1) % 3] == (tri[k] + 1) % m as u32 {
                *flag = 1;
            }
        }
        if *flag != 0 {
            queue.push(t as u32);
        }
    }
    while let Some(t) = queue.pop() {
        for k in 0..3 {
            let next = cdt.neighbors[t as usize][k];
            if cdt.edge_tags[t as usize][k] == 0 && next != NO_INDEX && inside[next as usize] == 0 {
                inside[next as usize] = 1;
                queue.push(next);
            }
        }
    }
    let mut removal = Removal {
        triangles: Vec::new(),
        masks: Vec::new(),
        sides: Vec::new(),
        change: 0.0,
    };
    for (t, &is_inside) in inside.iter().enumerate() {
        if is_inside == 0 {
            continue;
        }
        let tri = cdt.triangles[t];
        let mut mask = 0u8;
        let mut side = 0u8;
        for k in 0..3 {
            if cdt.edge_tags[t][k] != 0 {
                mask |= flags[tri[k] as usize] << k;
                side |= 1 << k;
            }
        }
        removal.triangles.push([
            ring[tri[0] as usize],
            ring[tri[1] as usize],
            ring[tri[2] as usize],
        ]);
        removal.masks.push(mask);
        removal.sides.push(side);
    }
    if removal.triangles.len() + 2 != m {
        return None; // not a simple polygon's triangulation
    }

    // The change at v: its level against the new triangle it falls in.
    let centre = vertices[v as usize];
    let at = plan_of(centre);
    let mut best_weight = f64::NEG_INFINITY;
    let mut level = centre.z;
    for tri in &removal.triangles {
        let plan = Triangle2::new(
            plan_of(vertices[tri[0] as usize]),
            plan_of(vertices[tri[1] as usize]),
            plan_of(vertices[tri[2] as usize]),
        );
        let Some(weights) = plan.barycentric(at) else {
            continue;
        };
        let least = cpp::min3(weights[0], weights[1], weights[2]);
        if least > best_weight {
            best_weight = least;
            level = weights[0] * vertices[tri[0] as usize].z
                + weights[1] * vertices[tri[1] as usize].z
                + weights[2] * vertices[tri[2] as usize].z;
        }
    }
    removal.change = (centre.z - level).abs();

    // And where a new diagonal crosses an old spoke from v.
    for (t, tri) in removal.triangles.iter().enumerate() {
        for k in 0..3 {
            let a = tri[k];
            let b = tri[(k + 1) % 3];
            if ((removal.sides[t] >> k) & 1) != 0 || a > b {
                continue; // a side of the link, or a diagonal met from its other triangle
            }
            let pa = plan_of(vertices[a as usize]);
            let d1 = plan_of(vertices[b as usize]) - pa;
            for &r in &ring {
                if r == a || r == b {
                    continue;
                }
                let d2 = plan_of(vertices[r as usize]) - at;
                let denominator = d1.cross(d2);
                if denominator == 0.0 {
                    continue;
                }
                let s = (at - pa).cross(d2) / denominator;
                let u = (at - pa).cross(d1) / denominator;
                if !(s > 0.0 && s < 1.0 && u > 0.0 && u < 1.0) {
                    continue;
                }
                let before = lerp(centre.z, vertices[r as usize].z, u);
                let after = lerp(vertices[a as usize].z, vertices[b as usize].z, s);
                removal.change = cpp::max(removal.change, (before - after).abs());
            }
        }
    }
    Some(removal)
}

/// The surface with the vertices removed that change it by no more than
/// `tolerance` vertically, anywhere - not only at the vertex: each removal is
/// measured as the largest difference between the surface before and after
/// it over the triangles it replaces, and the differences of successive
/// removals are added, so the bound holds against the ORIGINAL surface. The
/// flattest vertices go first. Held vertices (rim, constrained edges) stay.
/// A tolerance of 0 removes only the vertices whose removal changes nothing,
/// to the length tolerance (tolerance::GEOMETRIC) by which lengths are
/// equal. Runs its first measurement on `pool` (None: TaskPool::shared());
/// the result is the same at any thread count. Fails with InvalidArgument for
/// a negative or non-finite tolerance and an empty surface.
pub fn thin_surface(
    surface: &TinSurface,
    tolerance: f64,
    pool: Option<&TaskPool>,
) -> Result<TinEditResult> {
    if !(tolerance >= 0.0) || !tolerance.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the thinning tolerance must be a finite number, 0 or more",
        )
        .with_context(format!("tolerance={}", format_exact_real(tolerance))));
    }
    if surface.empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the surface is empty",
        ));
    }
    let vertices = surface.vertices();
    let held = held_vertices(surface);
    let stars = VertexStars::new(surface);
    let candidates: Vec<u32> = (0..surface.vertex_count() as u32)
        .filter(|&v| held[v as usize] == 0 && !stars.of(v).is_empty())
        .collect();

    // A difference of the length tolerance is no difference (lengths_equal),
    // so a tolerance of 0 removes what changes nothing but the last bits.
    let limit = tolerance + tolerance::GEOMETRIC;

    // First measurement, on the original, in parallel: each candidate's answer
    // depends only on the surface, written by index, so any thread count
    // gives the same order.
    let workers: &TaskPool = match pool {
        Some(pool) => pool,
        None => TaskPool::shared(),
    };
    let first = compute_by_index(workers, candidates.len(), THIN_GRAIN, f64::INFINITY, |i| {
        let v = candidates[i];
        removal_of(
            v,
            vertices,
            surface.triangles(),
            surface.constrained_edges(),
            stars.of(v),
        )
        .map_or(f64::INFINITY, |removal| removal.change)
    });

    let mut order: Vec<usize> = (0..candidates.len())
        .filter(|&i| first[i] <= limit)
        .collect();
    order.sort_by(|&x, &y| {
        if first[x] != first[y] {
            first[x]
                .partial_cmp(&first[y])
                .unwrap_or(std::cmp::Ordering::Equal)
        } else {
            candidates[x].cmp(&candidates[y])
        }
    });

    // Then one at a time, flattest first, on the surface as thinned so far.
    let mut triangles: Vec<TinTriangle> = surface.triangles().clone();
    let mut masks: Vec<u8> = surface.constrained_edges().clone();
    let mut alive: Vec<u8> = vec![1; triangles.len()];
    let mut bound: Vec<f64> = vec![0.0; triangles.len()];
    let mut around: Vec<Vec<u32>> = (0..surface.vertex_count() as u32)
        .map(|v| stars.of(v).to_vec())
        .collect();
    let mut removed = 0usize;
    for &i in &order {
        let v = candidates[i];
        let star = around[v as usize].clone();
        let Some(removal) = removal_of(v, vertices, &triangles, &masks, &star) else {
            continue;
        };
        let prior = star
            .iter()
            .fold(0.0, |most, &t| cpp::max(most, bound[t as usize]));
        if prior + removal.change > limit {
            continue;
        }
        for &t in &star {
            alive[t as usize] = 0;
            for &x in &triangles[t as usize] {
                if x != v {
                    around[x as usize].retain(|&entry| entry != t);
                }
            }
        }
        around[v as usize].clear();
        for (n, tri) in removal.triangles.iter().enumerate() {
            let index = triangles.len() as u32;
            triangles.push(*tri);
            masks.push(removal.masks[n]);
            alive.push(1);
            bound.push(prior + removal.change);
            for &x in tri {
                around[x as usize].push(index);
            }
        }
        removed += 1;
    }

    // Nothing removed: the surface as it was. Rebuilding it would sort its
    // triangles, and a surface that is not in that order (an archive's, an
    // overlay's) would come back different - a change in the reply, and an
    // undo step for an edit that did nothing.
    if removed == 0 {
        return Ok(unchanged(surface));
    }

    let mut kept: Vec<TinTriangle> = Vec::new();
    let mut kept_masks: Vec<u8> = Vec::new();
    let mut largest = 0.0;
    for (t, &is_alive) in alive.iter().enumerate() {
        if is_alive != 0 {
            kept.push(triangles[t]);
            kept_masks.push(masks[t]);
            largest = cpp::max(largest, bound[t]);
        }
    }
    let thinned = finish_surface(vertices.clone(), kept, kept_masks)?;
    let mut report = report_of(surface, &thinned);
    report.points_removed = removed;
    report.largest_change = largest;
    Ok(TinEditResult {
        surface: thinned,
        report,
    })
}
