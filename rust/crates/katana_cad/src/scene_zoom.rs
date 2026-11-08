//! Zooming a 3D view towards what it draws (docs/render.md, "Zooming towards
//! the cursor"; include/katana/cad/scene_zoom.hpp).
//!
//! A wheel notch used to dolly towards the point under the cursor of the plane
//! through the orbit target, facing the eye - a point in the air in front of
//! the ground, or under it, not the ground itself. The eye closed in on that
//! point 13% of the remaining way each notch, so over ground beyond the plane
//! the magnification of the ground tended to depth / (depth - distance) and
//! stopped: after about 30 notches nothing on screen moved, and the pan and
//! the orbit, which go by the same collapsed distance, stopped with it. Over
//! ground nearer than the plane the eye went through the ground and the view
//! went blank.
//!
//! So a notch now finds what the cursor points at (zoom_anchor: what is drawn
//! there, else the datum's plane or the depth the scene reaches) and moves the
//! pivot to its depth first (Camera::set_pivot_depth), which moves nothing on
//! screen. The dolly then magnifies exactly that point by the notch's factor,
//! the eye comes the same fraction nearer to it every notch and never reaches
//! it, and the pan, the orbit and the edges' fade, which all go by the pivot's
//! distance, go by that point's.
//!
//! Headless and free of any window: the 3D view calls zoom_at_pixel for the
//! wheel over either renderer, and so can anything else that zooms a 3D view.

use katana_math::numerics::tolerance;
use katana_math::{AABB, Vec3, cpp};
use katana_render::{Camera, DrawList, Projection};

use crate::scene::{SceneLayers, scene_depth_box};

/// Where a pick came from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ScenePickSource {
    /// A triangle, line or point of a surface or a mesh.
    Terrain,
    /// A triangle, line or point of the drawing.
    Drawing,
    /// Nothing drawn: the datum's plane, where the grid stands.
    #[default]
    Datum,
    /// Nothing drawn, and the datum's plane (if the ray meets it at all)
    /// further than the scene reaches: the ray's point as deep as the
    /// furthest corner of scene_depth_box (zoom_anchor only).
    Reach,
}

/// What is under a pixel (the C++ `ScenePick`, whose `Source` is
/// ScenePickSource).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScenePick {
    /// Where the ray through the pixel meets a triangle or the datum's plane;
    /// for a line, its point nearest the ray; for a point, the point.
    pub point: Vec3,
    /// In front of the eye along the view direction: always positive.
    pub depth: f64,
    pub source: ScenePickSource,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PickOptions {
    /// How far from the pixel's centre, in pixels, the centre line of a line
    /// or the centre of a point may lie and still count as under the pixel:
    /// exactly on a 1 px line is hard to be. It is an angle about the ray -
    /// that many pixels in the middle of a perspective view, and more towards
    /// its edges, where a pixel spans less of the field of view: 1 / cos^2 of
    /// the angle off the axis, 1.5 times in the corner of a 4:3 view at 45
    /// degrees, 1.7 in a 16:9 one.
    pub aperture: f64,
    /// Tests only the primitives that can be under the pixel: a pass over the
    /// vertices finds which side of the ray and of the aperture each lies,
    /// and a primitive whose vertices all lie beyond one side is skipped.
    /// Off, every primitive is tested - what the tests compare the culled
    /// pick with (docs/render.md measures both).
    pub cull: bool,
}

impl Default for PickOptions {
    fn default() -> Self {
        Self {
            aperture: 3.0,
            cull: true,
        }
    }
}

/// The pixel's ray and a frame about it: w along the ray, u across it as near
/// the screen's right as w allows, v = u x w as near the screen's up. In this
/// frame the ray is the positive w axis: it meets a triangle where the
/// triangle's shadow along w on the (u, v) plane covers the origin, and how
/// far a point lies from the ray is its (u, v) part.
#[derive(Clone, Copy, Debug, Default)]
struct RayFrame {
    origin: Vec3,
    u: Vec3,
    v: Vec3,
    w: Vec3,
}

impl RayFrame {
    fn of(&self, p: Vec3) -> Vec3 {
        let d = p - self.origin;
        Vec3::new(d.dot(self.u), d.dot(self.v), d.dot(self.w))
    }
}

/// How far across the ray, at a distance s along it, a line or a point still
/// counts as under the pixel: a cone under a perspective projection, a prism
/// under an orthographic one.
#[derive(Clone, Copy, Debug, Default)]
struct Aperture {
    slope: f64,
    base: f64,
}

impl Aperture {
    fn at(&self, s: f64) -> f64 {
        self.slope * s + self.base
    }
}

/// Which side of the ray's two planes (u = 0, v = 0) a point lies, and which
/// side of the aperture's four. A primitive whose vertices all share one of
/// these bits lies wholly beyond that plane, so it cannot meet the ray (the
/// low four bits) or come within the aperture (the high four).
const RAY_SIDES: u8 = 0x0F;
const APERTURE_SIDES: u8 = 0xF0;

fn ray_sides(q: Vec3) -> u8 {
    (if q.x > 0.0 { 0x01 } else { 0 })
        | (if q.x < 0.0 { 0x02 } else { 0 })
        | (if q.y > 0.0 { 0x04 } else { 0 })
        | (if q.y < 0.0 { 0x08 } else { 0 })
}

/// q.x > r is the same comparison as r - q.x < 0, which is how
/// within_aperture reads it: the sign of a rounded difference is the sign of
/// the exact one. So a line culled here is one within_aperture would refuse.
fn aperture_sides(q: Vec3, aperture: &Aperture) -> u8 {
    let r = aperture.at(q.z);
    (if q.x > r { 0x10 } else { 0 })
        | (if q.x < -r { 0x20 } else { 0 })
        | (if q.y > r { 0x40 } else { 0 })
        | (if q.y < -r { 0x80 } else { 0 })
}

/// Where the ray - the positive w axis - meets the triangle abc, given in the
/// ray's frame: the distance along the ray. Nothing when it misses, lies in
/// the triangle's plane or meets it behind the origin.
fn along_ray(a: Vec3, b: Vec3, c: Vec3) -> Option<f64> {
    // The cull's own test, made here as well, so that a culled pick and a
    // full one decide every triangle alike.
    if (ray_sides(a) & ray_sides(b) & ray_sides(c)) != 0 {
        return None;
    }
    // Twice the signed areas the origin makes with each edge in the (u, v)
    // plane. Two triangles sharing an edge compute its area from the same two
    // vertices, exactly negated (Rust never fuses), so the ray cannot slip
    // between them.
    let ab = a.x * b.y - a.y * b.x;
    let bc = b.x * c.y - b.y * c.x;
    let ca = c.x * a.y - c.y * a.x;
    // Inside when none of the three disagrees in sign; a zero is on an edge
    // and counts.
    let negative = ab < 0.0 || bc < 0.0 || ca < 0.0;
    let positive = ab > 0.0 || bc > 0.0 || ca > 0.0;
    if negative && positive {
        return None;
    }
    let sum = ab + bc + ca;
    if sum == 0.0 {
        return None; // the ray lies in the triangle's plane
    }
    // The barycentric weights of a, b and c are bc, ca and ab over the sum.
    let s = (bc * a.z + ca * b.z + ab * c.z) / sum;
    if !(s >= 0.0) {
        return None;
    }
    Some(s)
}

/// The point of the segment ab, given in the ray's frame, nearest the ray
/// among the part that lies within the aperture and in front of the origin
/// (Liang-Barsky against the aperture's four sides and the plane across the
/// ray through the origin): its parameter along ab. Nothing when no part does.
fn within_aperture(a: Vec3, b: Vec3, aperture: &Aperture) -> Option<f64> {
    let mut enter = 0.0;
    let mut leave = 1.0;
    // Keeps the part where g >= 0, g running from ga at a to gb at b.
    let mut keep = |ga: f64, gb: f64| {
        if ga < 0.0 && gb < 0.0 {
            return false;
        }
        if ga < 0.0 {
            enter = cpp::max(enter, ga / (ga - gb));
        } else if gb < 0.0 {
            leave = cpp::min(leave, ga / (ga - gb));
        }
        enter <= leave
    };
    let ra = aperture.at(a.z);
    let rb = aperture.at(b.z);
    if !keep(ra - a.x, rb - b.x)
        || !keep(ra + a.x, rb + b.x)
        || !keep(ra - a.y, rb - b.y)
        || !keep(ra + a.y, rb + b.y)
        || !keep(a.z, b.z)
    {
        return None;
    }
    // Across the ray the distance is |(a + t (b - a)) in (u, v)|, least where
    // its square's derivative is zero; clamped to the part kept. A segment
    // running along the ray is as near all the way: its nearer end.
    let du = b.x - a.x;
    let dv = b.y - a.y;
    let across = du * du + dv * dv;
    if !(across > 0.0) {
        return Some(if a.z <= b.z { enter } else { leave });
    }
    Some(cpp::clamp(-(a.x * du + a.y * dv) / across, enter, leave))
}

/// The nearest candidate so far, by depth. A tie keeps the one found first -
/// the terrain before the drawing, a lower index before a higher - so equal
/// depths resolve the same way every time.
struct Nearest {
    eye: Vec3,
    forward: Vec3,
    pick: Option<ScenePick>,
}

impl Nearest {
    fn new(eye: Vec3, forward: Vec3) -> Self {
        Self {
            eye,
            forward,
            pick: None,
        }
    }

    fn offer(&mut self, point: Vec3, source: ScenePickSource) {
        let depth = (point - self.eye).dot(self.forward);
        if !(depth > 0.0) {
            return; // at or behind the eye: nothing to zoom towards
        }
        if self.pick.is_none_or(|pick| depth < pick.depth) {
            self.pick = Some(ScenePick {
                point,
                depth,
                source,
            });
        }
    }
}

/// Offers everything of `list` under the pixel. `sides` is scratch.
fn scan(
    list: &DrawList,
    source: ScenePickSource,
    frame: &RayFrame,
    aperture: &Aperture,
    cull: bool,
    sides: &mut Vec<u8>,
    nearest: &mut Nearest,
) {
    if list.empty() {
        return;
    }
    let positions = &list.positions;
    if cull {
        // A surface's list is triangles alone: the aperture's sides are for
        // lines and points, and cost a third of this pass.
        let linework = !list.lines.is_empty() || !list.points.is_empty();
        sides.clear();
        sides.extend(positions.iter().map(|&position| {
            let q = frame.of(position);
            if linework {
                ray_sides(q) | aperture_sides(q, aperture)
            } else {
                ray_sides(q)
            }
        }));
    }
    for triangle in &list.triangles {
        let (a, b, c) = (
            triangle.a as usize,
            triangle.b as usize,
            triangle.c as usize,
        );
        if cull && (sides[a] & sides[b] & sides[c] & RAY_SIDES) != 0 {
            continue;
        }
        if let Some(s) = along_ray(
            frame.of(positions[a]),
            frame.of(positions[b]),
            frame.of(positions[c]),
        ) {
            nearest.offer(frame.origin + frame.w * s, source);
        }
    }
    for line in &list.lines {
        let (a, b) = (line.a as usize, line.b as usize);
        if cull && (sides[a] & sides[b] & APERTURE_SIDES) != 0 {
            continue;
        }
        let pa = positions[a];
        let pb = positions[b];
        if let Some(t) = within_aperture(frame.of(pa), frame.of(pb), aperture) {
            nearest.offer(pa + (pb - pa) * t, source);
        }
    }
    for point in &list.points {
        let a = point.a as usize;
        if cull && (sides[a] & APERTURE_SIDES) != 0 {
            continue;
        }
        let q = frame.of(positions[a]);
        if q.z > 0.0 && aperture_sides(q, aperture) == 0 {
            nearest.offer(positions[a], source);
        }
    }
}

/// The nearest thing the terrain and drawing layers draw under the centre of
/// pixel (x, y) of `camera` (in its viewport's pixels, as ray_through_pixel):
/// a triangle the pixel's ray passes through, or a line or a point within
/// `options.aperture` pixels of it, whichever is nearest the eye. With none,
/// where the ray meets the datum's plane (SceneLayers::datum), when that is in
/// front of the eye and inside scene_depth_box in plan: the grid stands there,
/// and linework with no height lies there. Else nothing - a pixel of sky, or
/// of ground beyond anything drawn. The grid, the faded edges and the
/// selection overlay are not looked at: the grid is a backdrop, and the other
/// two draw again what the terrain and the drawing already hold.
///
/// Also nothing for a camera with no viewport or a pixel that is not finite.
#[allow(
    clippy::manual_range_contains,
    reason = "The C++'s comparisons, which keep a NaN coordinate inside the box where a range's contains would put it outside."
)]
pub fn pick_drawn_point(
    layers: &SceneLayers,
    camera: &Camera,
    x: f64,
    y: f64,
    options: &PickOptions,
) -> Option<ScenePick> {
    if camera.viewport_width() <= 0
        || camera.viewport_height() <= 0
        || !x.is_finite()
        || !y.is_finite()
    {
        return None;
    }
    let ray = camera.ray_through_pixel(x, y);
    let mut frame = RayFrame {
        origin: ray.origin,
        w: ray.direction.normalized(),
        ..RayFrame::default()
    };
    let right = camera.right();
    frame.u = (right - frame.w * right.dot(frame.w)).normalized();
    frame.v = frame.u.cross(frame.w);

    // A pixel across the ray: an angle (its tangent) under a perspective
    // projection, a length under an orthographic one.
    let height = f64::from(camera.viewport_height());
    let pixels = if options.aperture.is_finite() {
        cpp::max(options.aperture, 0.0)
    } else {
        0.0
    };
    let mut aperture = Aperture::default();
    if camera.projection() == Projection::Perspective {
        aperture.slope = pixels * 2.0 * (camera.field_of_view() * 0.5).tan() / height;
    } else {
        aperture.base = pixels * camera.orthographic_height() / height;
    }

    let mut nearest = Nearest::new(camera.eye(), camera.forward());
    let mut sides = Vec::new();
    scan(
        &layers.terrain,
        ScenePickSource::Terrain,
        &frame,
        &aperture,
        options.cull,
        &mut sides,
        &mut nearest,
    );
    scan(
        &layers.entities,
        ScenePickSource::Drawing,
        &frame,
        &aperture,
        options.cull,
        &mut sides,
        &mut nearest,
    );
    if nearest.pick.is_some() {
        return nearest.pick;
    }

    // Nothing drawn under the pixel: the datum's plane, but only inside the
    // scene's box in plan, where the grid is drawn. Further out nothing is
    // drawn; zoom_anchor goes on from there.
    if frame.w.z == 0.0 {
        return None;
    }
    let s = (layers.datum - frame.origin.z) / frame.w.z;
    if !(s >= 0.0) {
        return None;
    }
    let point = frame.origin + frame.w * s;
    let bounds = scene_depth_box(layers);
    if bounds.empty()
        || point.x < bounds.min.x
        || point.x > bounds.max.x
        || point.y < bounds.min.y
        || point.y > bounds.max.y
    {
        return None;
    }
    nearest.offer(point, ScenePickSource::Datum);
    nearest.pick
}

/// What a zoom about pixel (x, y) goes towards: what is drawn there
/// (pick_drawn_point); with nothing drawn there (two thirds of a docked view
/// as it opens, the ground beyond the grid and the background round the
/// model), where the ray meets the datum's plane, however far out, or the
/// ray's point as deep as the scene reaches (the furthest corner of
/// scene_depth_box along the view direction), whichever is nearer. The datum keeps the eye
/// above the ground a zoom goes towards, where the target's plane put the
/// anchor under it and the eye dived; the reach keeps a pixel near the
/// horizon from anchoring kilometres off in one notch, and leaves nothing
/// drawn beyond the anchor, so nothing drawn stalls: it all grows until it
/// leaves the view. A level or rising ray takes the reach. Nothing when the
/// whole scene is behind the eye, or for what pick_drawn_point refuses.
pub fn zoom_anchor(layers: &SceneLayers, camera: &Camera, x: f64, y: f64) -> Option<ScenePick> {
    if let Some(drawn) = pick_drawn_point(layers, camera, x, y, &PickOptions::default()) {
        return Some(drawn);
    }
    if camera.viewport_width() <= 0
        || camera.viewport_height() <= 0
        || !x.is_finite()
        || !y.is_finite()
    {
        return None;
    }
    let bounds: AABB = scene_depth_box(layers);
    if bounds.empty() {
        return None;
    }
    let eye = camera.eye();
    let forward = camera.forward();
    // How deep the scene reaches: its box's furthest corner along the view.
    let mut reach = f64::NEG_INFINITY;
    for corner in 0..8 {
        let p = Vec3::new(
            if (corner & 1) != 0 {
                bounds.max.x
            } else {
                bounds.min.x
            },
            if (corner & 2) != 0 {
                bounds.max.y
            } else {
                bounds.min.y
            },
            if (corner & 4) != 0 {
                bounds.max.z
            } else {
                bounds.min.z
            },
        );
        reach = cpp::max(reach, (p - eye).dot(forward));
    }
    let ray = camera.ray_through_pixel(x, y);
    let w = ray.direction.normalized();
    // Depth per unit along the ray: positive for every pixel's ray, which
    // lies inside the frustum.
    let slope = w.dot(forward);
    if !(reach > 0.0) || !(slope > 0.0) {
        return None; // the whole scene is behind the eye
    }
    if w.z != 0.0 {
        let s = (layers.datum - ray.origin.z) / w.z;
        let point = ray.origin + w * s;
        let depth = (point - eye).dot(forward);
        if s >= 0.0 && depth > 0.0 && depth <= reach {
            return Some(ScenePick {
                point,
                depth,
                source: ScenePickSource::Datum,
            });
        }
    }
    // As deep as the scene reaches, measured from where the ray starts: the
    // eye, or its own point of an orthographic view's plane.
    let start = (ray.origin - eye).dot(forward);
    let along = (reach - start) / slope;
    if !(along > 0.0) {
        return None;
    }
    Some(ScenePick {
        point: ray.origin + w * along,
        depth: reach,
        source: ScenePickSource::Reach,
    })
}

/// How far out, in pixels, the double arithmetic that projects a point may be
/// where minimum_approach stops a zoom.
pub const ZOOM_PRECISION_PIXELS: f64 = 0.1;

/// How near a zoom may bring the eye to `point` (a pivot's distance): where
/// what draws the view stops drawing it right, and no nearer than that
/// whatever the scene around it - a limit tied to the scene's box stopped the
/// zoom 930 m from a site at MGA coordinates whose drawing held one stray
/// entity at the origin. The larger of:
///
/// * tolerance::GEOMETRIC / Camera::NEAR_PIVOT_FLOOR, 0.1 mm: below it the
///   near plane, which fit_depth_range keeps a thousandth of the pivot's
///   distance in front of the eye, would reach its own floor of
///   tolerance::GEOMETRIC and stop keeping clear of what the zoom went
///   towards;
/// * where the double arithmetic that projects a point (the view's
///   translation, as far from the world's origin as the point, taken from
///   it) is out by ZOOM_PRECISION_PIXELS of a pixel. Each rounding is at most
///   u = 2^-53 of what it holds, and a dot product of n terms is out by at
///   most n u of the sum of their sizes (Higham, Accuracy and Stability of
///   Numerical Algorithms, 2nd ed., section 3.1); the view's translation (3
///   terms), the projection times the view (4) and the matrix times the point
///   (4, of the point and the translation, each |point| in size) add to at
///   most 16 u |point|: 0.11 mm at an MGA northing of 6.2e6 m on an 800 px
///   tall view, and the same length whatever the drawing's unit, which is
///   what binds a survey drawn in millimetres.
///
/// The GPU draws in float relative to a scene origin, whose error closes in
/// far sooner; the GPU view keeps that origin next to the pivot instead
/// (docs/gpu.md), so its limit is this one.
pub fn minimum_approach(camera: &Camera, point: Vec3) -> f64 {
    let near_plane = tolerance::GEOMETRIC / Camera::NEAR_PIVOT_FLOOR;
    let height = f64::from(std::cmp::max(camera.viewport_height(), 1));
    // The pixel's angle: a footprint at depth d is d times it.
    let angle = 2.0 * (camera.field_of_view() * 0.5).tan() / height;
    // 2^-53.
    const UNIT_ROUNDOFF: f64 = 0.5 * f64::EPSILON;
    const ROUNDINGS: f64 = 16.0;
    let size = if point.is_finite() {
        point.length()
    } else {
        0.0
    };
    let precision = ROUNDINGS * UNIT_ROUNDOFF * size / (angle * ZOOM_PRECISION_PIXELS);
    cpp::max(near_plane, precision)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoomResult {
    /// What the zoom went towards; nothing when it went about the pivot's
    /// plane, as it always had: under an orthographic projection, where every
    /// point of a pixel's ray stays under the pixel, or with the whole scene
    /// behind the eye.
    pub anchor: Option<ScenePick>,
    /// What the distance and the orthographic height were multiplied by:
    /// below 1 is nearer, 1 is no move.
    pub factor: f64,
    /// The minimum approach held the zoom short of the factor asked for.
    pub limited: bool,
}

impl Default for ZoomResult {
    fn default() -> Self {
        Self {
            anchor: None,
            factor: 1.0,
            limited: false,
        }
    }
}

/// Zooms `camera` by `notches` wheel notches, positive in, each
/// Camera::ZOOM_PER_NOTCH, about pixel (x, y). Under a perspective projection
/// the pivot first moves to the depth of what the pixel points at
/// (zoom_anchor, Camera::set_pivot_depth), so that point stays under the pixel
/// and its footprint shrinks by exactly the factor. Zooming in stops at
/// minimum_approach of that point - of the target without one - and under an
/// orthographic projection at the view a perspective view shows there,
/// 2 d tan(fov / 2) tall, so P shows the same limit. Zooming out is not
/// limited. Zero, non-finite notches, a pixel that is not finite or a camera
/// with no viewport change nothing.
pub fn zoom_at_pixel(
    layers: &SceneLayers,
    camera: &mut Camera,
    notches: f64,
    x: f64,
    y: f64,
) -> ZoomResult {
    let mut result = ZoomResult::default();
    if !notches.is_finite()
        || notches == 0.0
        || !x.is_finite()
        || !y.is_finite()
        || camera.viewport_width() <= 0
        || camera.viewport_height() <= 0
    {
        return result;
    }
    // The least factor the minimum approach allows (above 1: none).
    let least = if camera.projection() == Projection::Perspective {
        result.anchor = zoom_anchor(layers, camera, x, y);
        if let Some(anchor) = result.anchor {
            camera.set_pivot_depth(anchor.depth);
        }
        let towards = match result.anchor {
            Some(anchor) => anchor.point,
            None => camera.target(),
        };
        minimum_approach(camera, towards) / camera.distance()
    } else {
        2.0 * minimum_approach(camera, camera.target()) * (camera.field_of_view() * 0.5).tan()
            / camera.orthographic_height()
    };
    // Underflows to 0 for an enormous zoom in, which the limit then catches;
    // an enormous zoom out overflows and is refused below.
    let mut factor = (1.0 / Camera::ZOOM_PER_NOTCH).powf(notches);
    if factor < 1.0 && factor < least {
        factor = cpp::min(1.0, least);
        result.limited = true;
    }
    if !factor.is_finite() || factor == 1.0 {
        return result;
    }
    camera.dolly_at_pixel(factor, x, y);
    result.factor = factor;
    result
}
