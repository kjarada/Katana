//! Whether two TIN surfaces are the same surface: the same vertices and the
//! same triangles, with the same constrained edges, wherever their vertices
//! are numbered (tests/support/surface_geometry.hpp). An edit that makes new
//! vertices in a different order - one patch for several footprints, or one
//! patch each - makes the same surface, which TinSurface's `==` (index for
//! index) would call different.
//!
//! Coordinates are compared in micrometres: a vertex two runs make by
//! arithmetic that differs in the last bit is one vertex, and no two vertices
//! of a test's surface are a micrometre apart.
//!
//! Owned by the port's cad wave J (plan agent J1); a test target includes it
//! with `#[path = "../support/surface_geometry.rs"] pub mod surface_geometry;`.

use katana_geometry::Point3;
use katana_terrain::TinSurface;

type Corner = [i64; 3];

// std::llround: to the nearest, a half away from zero, as f64::round.
fn corner_of(p: &Point3) -> Corner {
    [
        (p.x * 1e6).round() as i64,
        (p.y * 1e6).round() as i64,
        (p.z * 1e6).round() as i64,
    ]
}

#[derive(PartialEq)]
struct Canonical {
    vertices: Vec<Corner>,
    // Three corners as x, y, z, then bit j set when the j-th edge from the
    // first corner is a breakline or boundary edge.
    triangles: Vec<[i64; 10]>,
}

fn canonical(surface: &TinSurface) -> Canonical {
    let mut vertices: Vec<Corner> = surface.vertices().iter().map(corner_of).collect();
    vertices.sort_unstable();
    let mut triangles: Vec<[i64; 10]> = Vec::with_capacity(surface.triangle_count());
    let points = surface.vertices();
    for (t, &[first, second, third]) in surface.triangles().iter().enumerate() {
        let corners: [Corner; 3] = [
            corner_of(&points[first as usize]),
            corner_of(&points[second as usize]),
            corner_of(&points[third as usize]),
        ];
        // std::min_element: the first of equal least corners.
        let start = (0..3).min_by_key(|&k| corners[k]).unwrap_or(0);
        let mut key = [0_i64; 10];
        for j in 0..3 {
            let at = corners[(j + start) % 3];
            key[3 * j] = at[0];
            key[3 * j + 1] = at[1];
            key[3 * j + 2] = at[2];
            if surface.is_edge_constrained(t, (j + start) % 3) {
                key[9] |= 1_i64 << j;
            }
        }
        triangles.push(key);
    }
    triangles.sort_unstable();
    Canonical {
        vertices,
        triangles,
    }
}

/// Ok when `a` and `b` are the same surface; else what differs (the C++
/// `sameSurfaceGeometry`'s AssertionResult).
pub fn same_surface_geometry(a: &TinSurface, b: &TinSurface) -> Result<(), String> {
    let ca = canonical(a);
    let cb = canonical(b);
    if ca == cb {
        return Ok(());
    }
    Err(format!(
        "vertices {} and {}, triangles {} and {}, {}, {}",
        a.vertex_count(),
        b.vertex_count(),
        a.triangle_count(),
        b.triangle_count(),
        if ca.vertices == cb.vertices {
            "the same vertices"
        } else {
            "different vertices"
        },
        if ca.triangles == cb.triangles {
            "the same triangles"
        } else {
            "different triangles"
        }
    ))
}
