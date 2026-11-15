//! What the tests of the drawing's surfaces share (docs/terrain.md, "Surfaces
//! in the drawing"; tests/cad/terrain/surface_test_support.hpp): small
//! surfaces whose every height is known by construction, and a scratch
//! project directory of a test's own.
//!
//! Owned by the Document core's tests (wave F1); a test target includes it
//! with `#[path = "../support/surface_test_support.rs"] pub mod surface_test_support;`.

use std::path::PathBuf;
use std::sync::Arc;

use katana_geometry::Point3;
use katana_terrain::{NamedSurface, SurfaceStore, TinSurface};

/// The square [x0, x0 + size] x [y0, y0 + size] as two counter-clockwise
/// triangles on the plane z = z0 + dz * x / size: its heights run from z0 on
/// the west edge to z0 + dz on the east. The C++ `tiltedSquare` with every
/// argument said.
pub fn tilted_square(z0: f64, dz: f64, size: f64, x0: f64, y0: f64) -> Arc<TinSurface> {
    let made = TinSurface::create(
        vec![
            Point3::new(x0, y0, z0),
            Point3::new(x0 + size, y0, z0 + dz),
            Point3::new(x0 + size, y0 + size, z0 + dz),
            Point3::new(x0, y0 + size, z0),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
        Vec::new(),
    );
    match made {
        Ok(surface) => Arc::new(surface),
        Err(error) => panic!("{}", error.describe()),
    }
}

/// The C++ `tiltedSquare(z0)`: the flat 10 x 10 square at the origin.
pub fn flat_square(z0: f64) -> Arc<TinSurface> {
    tilted_square(z0, 0.0, 10.0, 0.0, 0.0)
}

/// `store` with one more surface, visible and from "test"; the test fails if
/// the store refuses it.
pub fn with(
    store: SurfaceStore,
    name: &str,
    surface: Arc<TinSurface>,
    style: &str,
) -> SurfaceStore {
    with_details(store, name, surface, style, true, "test")
}

/// The C++ `with` with every argument said.
pub fn with_details(
    mut store: SurfaceStore,
    name: &str,
    surface: Arc<TinSurface>,
    style: &str,
    visible: bool,
    source: &str,
) -> SurfaceStore {
    let named = NamedSurface {
        name: String::from(name),
        surface: Some(surface),
        source: String::from(source),
        style: String::from(style),
        visible,
    };
    if let Err(error) = store.add(named) {
        panic!("{}", error.describe());
    }
    store
}

/// A directory of the test's own, removed when it goes: the tests of one
/// binary run at the same time, and those of two binaries may too, so the
/// name carries the test's and the process's.
pub struct ScratchDirectory {
    pub path: PathBuf,
}

impl ScratchDirectory {
    pub fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("katana-cad-surfaces-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        let _ = std::fs::create_dir_all(&path);
        Self { path }
    }
}

impl Drop for ScratchDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
