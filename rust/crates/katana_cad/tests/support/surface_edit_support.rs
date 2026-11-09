//! Surfaces made by hand for the surface edit verbs' and tools' tests
//! (tests/cad/terrain/surface_edit_support.hpp; docs/surface_editing.md):
//! every level, count and area they give is worked out on paper where a test
//! asserts it.
//!
//! Owned by the port's cad wave J (plan agent J1); a test target includes it
//! with `#[path = "../support/surface_edit_support.rs"] pub mod surface_edit_support;`.
//! The C++ default arguments are said at every call.

#![allow(
    dead_code,
    reason = "Each test target that includes the support uses a part of it."
)]

use std::sync::Arc;

use katana_cad::Document;
use katana_geometry::Point3;
use katana_terrain::{NamedSurface, TinSurface, TinTriangle};

/// The C++ `makeSurface`: the surface TinSurface::create makes of the
/// vertices, triangles and constraint masks; the test fails when it refuses
/// them.
#[track_caller]
pub fn make_surface(
    vertices: Vec<Point3>,
    triangles: Vec<TinTriangle>,
    masks: Vec<u8>,
) -> TinSurface {
    match TinSurface::create(vertices, triangles, masks) {
        Ok(surface) => surface,
        Err(error) => panic!("{}", error.describe()),
    }
}

/// [0,10]^2 as two triangles split along the diagonal (0,0)-(10,10).
#[track_caller]
pub fn square_surface(z0: f64, z1: f64, z2: f64, z3: f64) -> TinSurface {
    make_surface(
        vec![
            Point3::new(0.0, 0.0, z0),
            Point3::new(10.0, 0.0, z1),
            Point3::new(10.0, 10.0, z2),
            Point3::new(0.0, 10.0, z3),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
        Vec::new(),
    )
}

/// [0,10]^2 with the apex (5,5,apex) joined to the four corners at 0.
#[track_caller]
pub fn pyramid_surface(apex: f64) -> TinSurface {
    make_surface(
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(10.0, 0.0, 0.0),
            Point3::new(10.0, 10.0, 0.0),
            Point3::new(0.0, 10.0, 0.0),
            Point3::new(5.0, 5.0, apex),
        ],
        vec![[0, 1, 4], [1, 2, 4], [2, 3, 4], [0, 4, 3]],
        Vec::new(),
    )
}

/// `cells` x `cells` squares of side `cell` from (x0, y0), each split along
/// its rising diagonal, the levels from `level(x, y)`.
#[track_caller]
pub fn grid_surface(
    cells: i32,
    cell: f64,
    level: impl Fn(f64, f64) -> f64,
    x0: f64,
    y0: f64,
) -> TinSurface {
    let mut vertices = Vec::new();
    for j in 0..=cells {
        for i in 0..=cells {
            let x = x0 + f64::from(i) * cell;
            let y = y0 + f64::from(j) * cell;
            vertices.push(Point3::new(x, y, level(x, y)));
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
    make_surface(vertices, triangles, Vec::new())
}

/// The grid at the one level `z`.
#[track_caller]
pub fn flat_grid_surface(cells: i32, cell: f64, z: f64, x0: f64, y0: f64) -> TinSurface {
    grid_surface(cells, cell, move |_, _| z, x0, y0)
}

/// `surface` added to the document's surfaces as `name`, as one undo step;
/// the test fails when the store or the document refuses it.
#[track_caller]
pub fn put_surface(document: &mut Document, name: &str, surface: TinSurface, visible: bool) {
    let mut store = document.surfaces().clone();
    let named = NamedSurface {
        name: String::from(name),
        surface: Some(Arc::new(surface)),
        source: String::from("test"),
        style: String::new(),
        visible,
    };
    if let Err(error) = store.add(named) {
        panic!("{}", error.describe());
    }
    if let Err(error) = document.set_surfaces(store, "TEST_SURFACE") {
        panic!("{}", error.describe());
    }
}

/// The surface the document holds as `name`; the test fails when there is
/// none (the C++ fails it and goes on with an empty surface).
#[track_caller]
pub fn surface_of<'a>(document: &'a Document, name: &str) -> &'a TinSurface {
    match document
        .surfaces()
        .find(name)
        .and_then(|named| named.surface.as_deref())
    {
        Some(surface) => surface,
        None => panic!("no surface {name}"),
    }
}
