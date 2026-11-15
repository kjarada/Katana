//! Shared fixtures for the terrain tests (the twin of tests/terrain/
//! terrain_test_support.hpp and tests/support/property.hpp). Everything here
//! produces INPUT; expected values are derived analytically inside the tests,
//! never from the module.

use katana_geometry::{Point2, Point3};
use katana_terrain::{Breakline, TinBuildOptions, TinBuildResult, TinInput, TinSurface, build_tin};

pub const PROPERTY_ITERATIONS: usize = 500;

fn real_from_bits(mut next: impl FnMut() -> u64, low: f64, high: f64) -> f64 {
    loop {
        let fraction = next() as f64 / 18446744073709551616.0;
        if fraction < 1.0 {
            return fraction * (high - low) + low;
        }
    }
}

/// EXPECT_NEAR.
pub fn near(actual: f64, expected: f64, epsilon: f64) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "actual={actual:?}, expected={expected:?}, epsilon={epsilon:?}"
    );
}

/// EXPECT_DOUBLE_EQ: within four ulps.
pub fn double_eq(actual: f64, expected: f64) {
    fn biased(value: f64) -> u64 {
        let bits = value.to_bits();
        if bits & (1 << 63) != 0 {
            (!bits).wrapping_add(1)
        } else {
            bits | (1 << 63)
        }
    }
    assert!(
        !actual.is_nan() && !expected.is_nan() && biased(actual).abs_diff(biased(expected)) <= 4,
        "actual={actual:?}, expected={expected:?}"
    );
}

/// GCC's std::mt19937_64 with std::uniform_real_distribution and
/// std::uniform_int_distribution: the C++ tests' katana::test::Random, draw
/// for draw (copied from katana_geometry's test support).
pub struct Random {
    state: [u64; 312],
    index: usize,
}

impl Default for Random {
    fn default() -> Self {
        Self::new(0x4B4154414E41)
    }
}

impl Random {
    pub fn new(seed: u64) -> Self {
        let mut state = [0; 312];
        state[0] = seed;
        for index in 1..312 {
            state[index] = 6364136223846793005_u64
                .wrapping_mul(state[index - 1] ^ (state[index - 1] >> 62))
                .wrapping_add(index as u64);
        }
        Self { state, index: 312 }
    }

    pub fn next_u64(&mut self) -> u64 {
        if self.index == 312 {
            for index in 0..312 {
                let value = (self.state[index] & 0xffffffff80000000)
                    | (self.state[(index + 1) % 312] & 0x7fffffff);
                self.state[index] = self.state[(index + 156) % 312]
                    ^ (value >> 1)
                    ^ if value & 1 != 0 {
                        0xb5026f5aa96619e9
                    } else {
                        0
                    };
            }
            self.index = 0;
        }
        let mut value = self.state[self.index];
        self.index += 1;
        value ^= (value >> 29) & 0x5555555555555555;
        value ^= (value << 17) & 0x71d67fffeda60000;
        value ^= (value << 37) & 0xfff7eee000000000;
        value ^ (value >> 43)
    }

    pub fn real(&mut self, low: f64, high: f64) -> f64 {
        real_from_bits(|| self.next_u64(), low, high)
    }

    pub fn integer(&mut self, low: i32, high: i32) -> i32 {
        let range = (high as i64 - low as i64 + 1) as u64;
        let threshold = range.wrapping_neg() % range;
        loop {
            let product = self.next_u64() as u128 * range as u128;
            if product as u64 >= threshold {
                return low + (product >> 64) as i32;
            }
        }
    }
}

pub fn p2(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

pub fn p3(x: f64, y: f64, z: f64) -> Point3 {
    Point3::new(x, y, z)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Plane {
    /// dz/dx
    pub a: f64,
    /// dz/dy
    pub b: f64,
    pub c: f64,
}

impl Plane {
    pub const fn new(a: f64, b: f64, c: f64) -> Self {
        Self { a, b, c }
    }
    pub fn at(&self, x: f64, y: f64) -> f64 {
        self.a * x + self.b * y + self.c
    }
}

/// The four corners of [x0, x0 + size] x [y0, y0 + size] plus `interior` random
/// points strictly inside, all lifted onto `plane`. The corners make the convex
/// hull the square itself, so the plan area is known exactly.
pub fn plane_points_at(
    plane: &Plane,
    size: f64,
    interior: usize,
    random: &mut Random,
    x0: f64,
    y0: f64,
) -> Vec<Point3> {
    let mut points = Vec::new();
    let xs = [x0, x0 + size, x0 + size, x0];
    let ys = [y0, y0, y0 + size, y0 + size];
    for i in 0..4 {
        points.push(Point3::new(xs[i], ys[i], plane.at(xs[i], ys[i])));
    }
    for _ in 0..interior {
        let x = random.real(x0 + 0.01 * size, x0 + 0.99 * size);
        let y = random.real(y0 + 0.01 * size, y0 + 0.99 * size);
        points.push(Point3::new(x, y, plane.at(x, y)));
    }
    points
}

pub fn plane_points(plane: &Plane, size: f64, interior: usize, random: &mut Random) -> Vec<Point3> {
    plane_points_at(plane, size, interior, random, 0.0, 0.0)
}

/// Regular grid of (count x count) points with unit spacing starting at the
/// origin, lifted onto `plane`. Grid points are cocircular in fours: the
/// degenerate case for Delaunay, deliberately.
pub fn grid_points(plane: &Plane, count: i32) -> Vec<Point3> {
    let mut points = Vec::new();
    for j in 0..count {
        for i in 0..count {
            let (x, y) = (f64::from(i), f64::from(j));
            points.push(Point3::new(x, y, plane.at(x, y)));
        }
    }
    points
}

/// Builds or fails the calling test.
pub fn build_with(input: &TinInput, options: &TinBuildOptions) -> TinBuildResult {
    match build_tin(input, options) {
        Ok(result) => result,
        Err(error) => panic!("buildTin failed: {}", error.describe()),
    }
}

pub fn build(input: &TinInput) -> TinBuildResult {
    build_with(input, &TinBuildOptions::default())
}

pub fn build_from_points(points: Vec<Point3>) -> TinSurface {
    let input = TinInput {
        points,
        ..TinInput::default()
    };
    build(&input).surface
}

/// Square pyramid: base [0, base]^2 at z = 0, apex above the centre at z = height.
/// The four ridges are breaklines, so extra points on the faces cannot make a
/// triangle straddle a ridge and the TIN is the pyramid exactly.
pub fn pyramid_input(
    base: f64,
    height: f64,
    extra_face_points: usize,
    random: &mut Random,
) -> TinInput {
    let half = 0.5 * base;
    let mut input = TinInput {
        points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(base, 0.0, 0.0),
            Point3::new(base, base, 0.0),
            Point3::new(0.0, base, 0.0),
            Point3::new(half, half, height),
        ],
        ..TinInput::default()
    };
    for corner in 0..4 {
        input.breaklines.push(Breakline {
            vertices: vec![input.points[corner], input.points[4]],
            closed: false,
        });
    }
    for _ in 0..extra_face_points {
        let x = random.real(0.0, base);
        let y = random.real(0.0, base);
        // Height of the pyramid: falls linearly with the Chebyshev distance from the centre.
        let distance = (x - half).abs().max((y - half).abs());
        input
            .points
            .push(Point3::new(x, y, height * (1.0 - distance / half)));
    }
    input
}

/// A vertex in micrometres (std::llround of the coordinate times 1e6).
fn corner_of(p: Point3) -> [i64; 3] {
    [
        (p.x * 1e6).round() as i64,
        (p.y * 1e6).round() as i64,
        (p.z * 1e6).round() as i64,
    ]
}

/// The surface as sorted corners and sorted triangles of corners, with the
/// constrained edges: the same for two surfaces that differ only in how their
/// vertices are numbered (tests/support/surface_geometry.hpp).
fn canonical(surface: &TinSurface) -> (Vec<[i64; 3]>, Vec<[i64; 10]>) {
    let mut vertices: Vec<[i64; 3]> = surface.vertices().iter().map(|&v| corner_of(v)).collect();
    vertices.sort_unstable();
    let mut triangles: Vec<[i64; 10]> = Vec::new();
    for (t, tri) in surface.triangles().iter().enumerate() {
        let corners = tri.map(|v| corner_of(surface.vertices()[v as usize]));
        let start = (1..3).fold(
            0,
            |best, k| if corners[k] < corners[best] { k } else { best },
        );
        let mut key = [0i64; 10];
        for j in 0..3 {
            let at = corners[(j + start) % 3];
            key[3 * j] = at[0];
            key[3 * j + 1] = at[1];
            key[3 * j + 2] = at[2];
            if surface.is_edge_constrained(t, (j + start) % 3) {
                key[9] |= 1 << j;
            }
        }
        triangles.push(key);
    }
    triangles.sort_unstable();
    (vertices, triangles)
}

/// Whether `a` and `b` are the same surface: the same vertices and the same
/// triangles, with the same constrained edges, wherever their vertices are
/// numbered. Coordinates are compared in micrometres.
pub fn same_surface_geometry(a: &TinSurface, b: &TinSurface) -> bool {
    let first = canonical(a);
    let second = canonical(b);
    if first == second {
        return true;
    }
    eprintln!(
        "vertices {} and {}, triangles {} and {}, {}, {}",
        a.vertex_count(),
        b.vertex_count(),
        a.triangle_count(),
        b.triangle_count(),
        if first.0 == second.0 {
            "the same vertices"
        } else {
            "different vertices"
        },
        if first.1 == second.1 {
            "the same triangles"
        } else {
            "different triangles"
        }
    );
    false
}
