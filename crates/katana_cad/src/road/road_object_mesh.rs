//! The solids a road object is made of
//! (include/katana/cad/road/road_object_mesh.hpp; docs/road_objects.md, "How
//! an object is made"): a polygon extruded, a cross-section swept along a
//! path, a tube. Every function here ADDS a closed, outward-oriented shell to
//! a mesh - each edge used by exactly two triangles, once in each direction -
//! so the 3D view can cull back faces, the IFC export can write the mesh as a
//! closed face set, and a volume is the divergence theorem over the
//! triangles.
//!
//! The caps of a section or a plate are cut by geometry's triangulate with
//! CollinearVertices::Keep: ear clipping, which is quadratic and meant for the
//! few vertices of a cross-section or a sign's plate, and which keeps every
//! corner, so a cap meets the quads of the sides at each of them. The polygon
//! may be concave (an F-shape barrier's knee) and either way round. A polygon
//! that cannot be capped - fewer than three corners, no area, a corner that
//! is not finite, edges that give no ear - adds NOTHING, and the object that
//! asked is listed as one that could not be made into a solid (road_objects'
//! shape_problem).
//!
//! A cross-section is given in (u, v): u across, positive to the LEFT of the
//! direction it is swept in, v up. It stays VERTICAL wherever it is swept - a
//! barrier or a wall stands upright on a grade, it does not lean with it -
//! and is mitred at a bend of the path, so a constant section keeps its width
//! round a corner. A bend sharper than a mitre of four times the half-width
//! allows (a turn of more than about 151 degrees) is bevelled instead: each
//! leg keeps its full width to the joint, and the two ends of the joint are
//! joined side to side, so the volume error is the size of the corner and
//! never spreads along the leg. A path that comes back to its start is swept
//! as a loop - mitred at its start like any other corner, with no end faces.
//!
//! A closed mesh's volume is geometry's mesh_volume.

use std::collections::BTreeMap;

use katana_geometry::TriangleMesh;
use katana_geometry::polygon::{CollinearVertices, triangulate_with_collinear};
use katana_geometry::{Point2, Point3, Polyline2, Vec2, Vec3};
use katana_math::numerics::PI;

type Face = [u32; 3];

// Points of a path closer than this in plan are one point: a millimetre, far
// below anything a road object is set out to, and far above the noise of a
// station computed twice.
const SAME_POINT: f64 = 1e-3;

// A mitre longer than this many times the section's half-width is not cut: at
// a hairpin of the path the true mitre runs off to infinity. Past it the
// joint is a bevel - the section ends square to the leg that arrives and
// starts square to the leg that leaves, the two rings joined side to side -
// as SVG and PostScript draw a join past the same limit (stroke-miterlimit's
// default of four). Shortening the mitre instead would narrow the section
// along the whole leg, because the leg's sides run from a full-width ring at
// its far end to the narrowed one at the joint.
const MITRE_LIMIT: f64 = 4.0;

/// A point of a path a section is swept along: where it is in plan and the
/// level the section's v = 0 stands at there.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PathPoint {
    pub position: Point2,
    pub z: f64,
}

impl PathPoint {
    pub const fn new(position: Point2, z: f64) -> Self {
        Self { position, z }
    }
}

// A vertex count as a face index; a mesh's indices are 32-bit, as the C++'s.
fn index(value: usize) -> u32 {
    value as u32
}

// The triangles that cap a section or fill a plate, as indices into `polygon`
// (counter-clockwise, and a clockwise polygon is triangulated the same): the
// shared ear clipper, geometry's triangulate, with every corner kept - a
// corner of no turn stays a vertex of its cap, so the cap meets the side
// quads that use it, and the solid closes. Empty for a polygon that does not
// triangulate (fewer than three corners, no area, a vertex that is not
// finite, or edges that cross); the callers then add nothing, and the object
// is listed as one that could not be made into a solid.
fn cap_triangles(polygon: &[Point2]) -> Vec<Face> {
    let outline = Polyline2::new(polygon.to_vec(), true);
    let Ok(triangles) = triangulate_with_collinear(&outline, CollinearVertices::Keep) else {
        return Vec::new();
    };
    let mut faces = Vec::with_capacity(triangles.len());
    for triangle in &triangles {
        faces.push([triangle[0] as u32, triangle[1] as u32, triangle[2] as u32]);
    }
    faces
}

// The cleaned path: points closer than SAME_POINT in plan to the one before
// left out.
fn distinct(path: &[PathPoint]) -> Vec<PathPoint> {
    let mut out: Vec<PathPoint> = Vec::new();
    for &point in path {
        if let Some(last) = out.last()
            && last.position.distance_to(point.position) < SAME_POINT
        {
            continue;
        }
        out.push(point);
    }
    out
}

// The path a sweep follows, cleaned: when it comes back to its start (the
// same plan position and level, within SAME_POINT) the closing point is
// dropped and `is_loop` says so, so the last segment runs on to the first
// point and the sweep wraps round with no ends. A fence, a wall or a line of
// barrier round a closed polyline then has its start corner mitred like every
// other, instead of two square end faces that leave a notch outside the
// corner and overlap inside it - or, closed midway along a side, two
// coincident caps.
struct SweepPath {
    points: Vec<PathPoint>,
    is_loop: bool,
}

fn sweep_path_of(path: &[PathPoint]) -> SweepPath {
    let mut points = distinct(path);
    let mut is_loop = false;
    // Four points at least: three distinct corners and the one that closes
    // them.
    if points.len() >= 4 {
        let first = points[0];
        let last = points[points.len() - 1];
        let before = points[points.len() - 2];
        if first.position.distance_to(last.position) < SAME_POINT
            && (first.z - last.z).abs() < SAME_POINT
            && first.position.distance_to(before.position) >= SAME_POINT
        {
            points.pop();
            is_loop = true;
        }
    }
    SweepPath { points, is_loop }
}

// One ring of a swept section: the path point it stands at, and the
// horizontal vector its u is measured along there.
#[derive(Clone, Copy)]
struct Ring {
    point: usize,
    axis: Vec2,
}

// The rings of a sweep, in order. A point of the path is one ring - its axis
// the left normal of the path there, mitred at a bend so that a section u to
// the left of both segments stays u from each - except where the bend is past
// MITRE_LIMIT (or the path doubles back): there it is two, one square to the
// segment that arrives and one square to the segment that leaves, so each leg
// keeps its full width right up to the joint. An open path's ends are square
// to their one segment.
#[allow(
    clippy::needless_range_loop,
    reason = "the index is the segment's number: it names both of its ends, the closing one wrapping"
)]
fn rings_of(path: &[PathPoint], is_loop: bool) -> Vec<Ring> {
    let count = path.len();
    let segments = if is_loop { count } else { count - 1 };
    let mut normals: Vec<Vec2> = Vec::with_capacity(segments);
    for i in 0..segments {
        normals.push(
            (path[(i + 1) % count].position - path[i].position)
                .normalized()
                .perpendicular(),
        );
    }
    let mut rings = Vec::with_capacity(count + 1);
    for k in 0..count {
        if !is_loop && k == 0 {
            rings.push(Ring {
                point: k,
                axis: normals[0],
            });
            continue;
        }
        if !is_loop && k + 1 == count {
            rings.push(Ring {
                point: k,
                axis: normals[normals.len() - 1],
            });
            continue;
        }
        let arriving = normals[(k + segments - 1) % segments];
        let leaving = normals[k % segments];
        let bisector = arriving + leaving;
        let length = bisector.length();
        // Two unit normals sum to 2 cos(half the turn): the mitre's length
        // over the half-width, 1 / cos, is within the limit exactly when the
        // sum is at least 2 / limit. A path that doubles back sums to 0.
        if length >= 2.0 / MITRE_LIMIT {
            let unit = bisector / length;
            rings.push(Ring {
                point: k,
                axis: unit * (1.0 / unit.dot(leaving)),
            });
        } else {
            rings.push(Ring {
                point: k,
                axis: arriving,
            });
            rings.push(Ring {
                point: k,
                axis: leaving,
            });
        }
    }
    rings
}

// Appends the rings of `profile` at each ring of the sweep, P vertices a
// ring; returns the index of the first.
fn add_rings(
    mesh: &mut TriangleMesh,
    path: &[PathPoint],
    rings: &[Ring],
    profile: &[Point2],
) -> u32 {
    let first = index(mesh.vertices.len());
    for ring in rings {
        let at = path[ring.point];
        for corner in profile {
            let plan = at.position + ring.axis * corner.x;
            mesh.vertices
                .push(Point3::new(plan.x, plan.y, at.z + corner.y));
        }
    }
    first
}

// The quads joining ring k to ring k + 1 round the whole profile, facing out
// of a counter-clockwise profile - or into it, for the inside of a tube. A
// loop's last ring joins the first.
fn add_sides(
    mesh: &mut TriangleMesh,
    first: u32,
    rings: usize,
    corners: usize,
    facing_in: bool,
    is_loop: bool,
) {
    let pairs = if is_loop { rings } else { rings - 1 };
    for k in 0..pairs {
        let following = (k + 1) % rings;
        for j in 0..corners {
            let next = (j + 1) % corners;
            let a = first + index(k * corners + j);
            let b = first + index(k * corners + next);
            let c = first + index(following * corners + next);
            let d = first + index(following * corners + j);
            if facing_in {
                mesh.faces.push([a, c, b]);
                mesh.faces.push([a, d, c]);
            } else {
                mesh.faces.push([a, b, c]);
                mesh.faces.push([a, c, d]);
            }
        }
    }
}

/// The polygon counter-clockwise: as given, or reversed when it was
/// clockwise.
pub fn counter_clockwise(polygon: &[Point2]) -> Vec<Point2> {
    let mut outline = Polyline2::new(polygon.to_vec(), true);
    if outline.signed_area() < 0.0 {
        outline.vertices.reverse();
    }
    outline.vertices
}

/// A regular polygon of `sides` vertices on a circle of `radius` about
/// `centre`, counter-clockwise, the first at `start_radians` (the C++ default
/// is 0). The round things - posts, a pole, a pipe - are drawn so: a polygon
/// of enough sides reads as round, and its volume is known exactly.
pub fn regular_polygon(centre: Point2, radius: f64, sides: i32, start_radians: f64) -> Vec<Point2> {
    let mut out = Vec::new();
    if sides < 3 {
        return out;
    }
    out.reserve(sides as usize);
    for k in 0..sides {
        let angle = start_radians + 2.0 * PI * f64::from(k) / f64::from(sides);
        out.push(Point2::new(
            centre.x + radius * angle.cos(),
            centre.y + radius * angle.sin(),
        ));
    }
    out
}

/// `polygon`, in the plane through `origin` spanned by the unit vectors `u`
/// and `v`, extruded `depth` along u x v. `u` and `v` must be perpendicular
/// unit vectors; the polygon may be either way round. Adds nothing for a
/// polygon that does not triangulate or a depth that is not positive.
pub fn add_extrusion(
    mesh: &mut TriangleMesh,
    polygon: &[Point2],
    origin: Point3,
    u: Vec3,
    v: Vec3,
    depth: f64,
) {
    if !(depth > 0.0) {
        return;
    }
    let profile = counter_clockwise(polygon);
    let caps = cap_triangles(&profile);
    if caps.is_empty() {
        return;
    }
    // (u, v, w) right-handed: the same frame as a sweep's (t, u, v) with the
    // extrusion as t, so the faces are wound as a sweep's are.
    let w = u.cross(v);
    let first = index(mesh.vertices.len());
    for along in [0.0, depth] {
        for corner in &profile {
            mesh.vertices
                .push(origin + u * corner.x + v * corner.y + w * along);
        }
    }
    let top = first + index(profile.len());
    for face in &caps {
        mesh.faces
            .push([first + face[0], first + face[2], first + face[1]]);
        mesh.faces
            .push([top + face[0], top + face[1], top + face[2]]);
    }
    add_sides(mesh, first, 2, profile.len(), false, false);
}

/// A vertical prism: `outline` in plan, from level `bottom` up to `top`.
pub fn add_prism(mesh: &mut TriangleMesh, outline: &[Point2], bottom: f64, top: f64) {
    add_extrusion(
        mesh,
        outline,
        Point3::new(0.0, 0.0, bottom),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        top - bottom,
    );
}

/// A box standing on `base_centre`: `length` along the horizontal unit vector
/// `along`, `width` across it, `height` up.
pub fn add_box(
    mesh: &mut TriangleMesh,
    base_centre: Point3,
    along: Vec2,
    length: f64,
    width: f64,
    height: f64,
) {
    let a = along.normalized();
    let left = a.perpendicular();
    let centre = Point2::new(base_centre.x, base_centre.y);
    let corners = [
        centre - a * (length / 2.0) - left * (width / 2.0),
        centre + a * (length / 2.0) - left * (width / 2.0),
        centre + a * (length / 2.0) + left * (width / 2.0),
        centre - a * (length / 2.0) + left * (width / 2.0),
    ];
    add_prism(mesh, &corners, base_centre.z, base_centre.z + height);
}

/// `profile`, in (u, v), swept along `path`: a closed solid with a flat end
/// at each end of the path - or, when the path returns to its first point
/// (same plan position and level, within a millimetre), a closed ring with no
/// ends. Points of the path closer than a millimetre to the one before are
/// passed over. Adds nothing for fewer than two distinct points or a profile
/// that does not triangulate.
pub fn add_sweep(mesh: &mut TriangleMesh, path: &[PathPoint], profile: &[Point2]) {
    let swept = sweep_path_of(path);
    let points = &swept.points;
    if points.len() < 2 {
        return;
    }
    let section = counter_clockwise(profile);
    let caps = cap_triangles(&section);
    if caps.is_empty() {
        return;
    }
    let rings = rings_of(points, swept.is_loop);
    let first = add_rings(mesh, points, &rings, &section);
    let corners = section.len();
    if !swept.is_loop {
        // The end faces forward, along the path, which in the (t, u, v) frame
        // is the side a counter-clockwise profile is seen counter-clockwise
        // from; the start faces back.
        let last = first + index((rings.len() - 1) * corners);
        for face in &caps {
            mesh.faces
                .push([first + face[0], first + face[2], first + face[1]]);
            mesh.faces
                .push([last + face[0], last + face[1], last + face[2]]);
        }
    }
    add_sides(mesh, first, rings.len(), corners, false, swept.is_loop);
}

/// A hollow sweep - a pipe, a box culvert's barrel: the solid between `outer`
/// and `inner`, which have the same number of vertices, correspond vertex by
/// vertex, and are both counter-clockwise with `inner` inside `outer`. Adds
/// nothing when they differ in size or there are fewer than two distinct
/// points.
pub fn add_hollow_sweep(
    mesh: &mut TriangleMesh,
    path: &[PathPoint],
    outer: &[Point2],
    inner: &[Point2],
) {
    let swept = sweep_path_of(path);
    let points = &swept.points;
    if points.len() < 2 || outer.len() != inner.len() || outer.len() < 3 {
        return;
    }
    let rings = rings_of(points, swept.is_loop);
    let corners = outer.len();
    let outside = add_rings(mesh, points, &rings, outer);
    let inside = add_rings(mesh, points, &rings, inner);
    add_sides(mesh, outside, rings.len(), corners, false, swept.is_loop);
    add_sides(mesh, inside, rings.len(), corners, true, swept.is_loop);
    if swept.is_loop {
        return; // a ring of tube with no ends
    }
    // The ends: the ring between the outer and the inner loop, a quad an
    // edge, wound as the solid sweep's ends are.
    let last_ring = (rings.len() - 1) * corners;
    for j in 0..corners {
        let next = (j + 1) % corners;
        let o0 = outside + index(j);
        let o1 = outside + index(next);
        let i0 = inside + index(j);
        let i1 = inside + index(next);
        mesh.faces.push([o0, i1, o1]);
        mesh.faces.push([o0, i0, i1]);
        let e0 = outside + index(last_ring + j);
        let e1 = outside + index(last_ring + next);
        let f0 = inside + index(last_ring + j);
        let f1 = inside + index(last_ring + next);
        mesh.faces.push([e0, e1, f1]);
        mesh.faces.push([e0, f1, f0]);
    }
}

/// True when every edge of every face is used by exactly one other face in
/// the opposite direction: a closed, consistently oriented surface - what a
/// solid needs to be. False for an empty mesh.
#[allow(
    clippy::needless_range_loop,
    reason = "the index is the edge's number: it names the corner the edge starts at and the next"
)]
pub fn is_closed_mesh(mesh: &TriangleMesh) -> bool {
    if mesh.faces.is_empty() {
        return false;
    }
    let mut directed: BTreeMap<(u32, u32), i32> = BTreeMap::new();
    for face in &mesh.faces {
        for e in 0..3 {
            *directed.entry((face[e], face[(e + 1) % 3])).or_default() += 1;
        }
    }
    for (&(from, to), &count) in &directed {
        if count != 1 {
            return false;
        }
        if directed.get(&(to, from)) != Some(&1) {
            return false;
        }
    }
    true
}
