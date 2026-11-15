//! Orbiting 3D camera (PLAN.MD Phase 15).
//!
//! CONVENTIONS, all of which the rasteriser and the pick ray depend on:
//!
//! - World: right-handed, Z UP. Survey and civil data is Z-up - elevations
//!   are z, not y - and converting at the door would mean every picked
//!   coordinate had to be converted back.
//! - Azimuth: radians CCW from +X about +Z. Elevation is measured from the XY
//!   plane, positive looking down at the ground from above.
//! - Eye space: right-handed, looking down -Z, +Y up, +X right.
//! - Clip: x, y in [-w, w]; z in [0, w] with REVERSED Z: NDC depth 1 at the
//!   near plane and 0 at the far plane, nearer is LARGER. Float depth is dense
//!   near 0 and sparse near 1, and a perspective divide crowds the far scene
//!   towards one end; reversing the range puts the dense end of the float
//!   where the divide crowds, so the step between depths stays about 2^-23 of
//!   the distance from the eye at any distance and almost whatever the near
//!   plane. Measured for an eye inside the scene (the case of
//!   RenderDepth.WithTheEyeInsideTheScene...: near = far * 1e-6, far 2 km, a
//!   strip 5 cm above the ground 0.7-1.3 km ahead): the strip and the ground
//!   behind it round to the SAME float depth at 93.9% of distances in the
//!   standard range, and at none in the reversed one. The GPU convention
//!   (D32F, compare Greater) is the same.
//! - Screen: pixels, origin TOP-LEFT, +y DOWN (so NDC y is flipped).
//!
//! The camera is a value: copying one is how a viewport records the view it
//! had before an orbit, and how four tiled viewports each keep their own.

#![allow(
    clippy::excessive_precision,
    clippy::approx_constant,
    reason = "The angles keep the C++'s decimal literals digit for digit, so they parse to the same doubles."
)]

use crate::scalar::{clamp, max, min};
use katana_geometry::Point3;
use katana_math::numerics::tolerance;
use katana_math::{AABB, Mat4, Ray, Vec3, Vec4};

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Projection {
    #[default]
    Perspective,
    Orthographic,
}

/// The standard CAD view directions. Plan is what a 2D drawing shows.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum StandardView {
    /// Plan.
    Top,
    Bottom,
    /// Looking north, i.e. from -Y.
    Front,
    Back,
    Left,
    Right,
    IsoSouthWest,
    IsoSouthEast,
    IsoNorthEast,
    IsoNorthWest,
}

pub const fn to_string(view: StandardView) -> &'static str {
    match view {
        StandardView::Top => "Top",
        StandardView::Bottom => "Bottom",
        StandardView::Front => "Front",
        StandardView::Back => "Back",
        StandardView::Left => "Left",
        StandardView::Right => "Right",
        StandardView::IsoSouthWest => "SW Isometric",
        StandardView::IsoSouthEast => "SE Isometric",
        StandardView::IsoNorthEast => "NE Isometric",
        StandardView::IsoNorthWest => "NW Isometric",
    }
}

// How close to straight up or straight down the camera may get. Exactly at the
// pole the view direction is parallel to world +Z and `up` is undefined, so the
// basis collapses; a milliradian short of it is visually indistinguishable and
// always well conditioned.
const POLE_GUARD: f64 = 1.0e-3;
const HALF_PI: f64 = 1.570796326794896619231;
const TWO_PI: f64 = 6.283185307179586476925;

const ISOMETRIC_ELEVATION: f64 = 0.615479708670387341067; // atan(1/sqrt(2))

fn wrap_angle(radians: f64) -> f64 {
    if !radians.is_finite() {
        return 0.0;
    }
    // Rust's % on floats is fmod.
    let mut wrapped = radians % TWO_PI;
    if wrapped <= -HALF_PI * 2.0 {
        wrapped += TWO_PI;
    } else if wrapped > HALF_PI * 2.0 {
        wrapped -= TWO_PI;
    }
    wrapped
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    target: Point3,
    distance: f64,
    azimuth: f64,
    elevation: f64,
    projection: Projection,
    fov_y: f64,
    ortho_height: f64,
    near: f64,
    far: f64,
    width: i32,
    height: i32,
    standoff: f64, // orthographic only; see fit_depth_range
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            target: Point3::new(0.0, 0.0, 0.0),
            distance: 100.0,
            // -45 degrees: the eye to the south-east, looking north-west
            // (forward()), as StandardView::IsoSouthEast.
            azimuth: -0.785398163397448309616,
            elevation: 0.615479708670387341067, // ~35.264 degrees: true isometric
            projection: Projection::Perspective,
            fov_y: 0.785398163397448309616, // 45 degrees
            ortho_height: 100.0,
            near: 0.1,
            far: 100000.0,
            width: 0,
            height: 0,
            standoff: 0.0,
        }
    }
}

impl Camera {
    /// Half-diagonal below which frame() grows the box: one survey point, or a
    /// drawing whose every entity is coincident, is framed as a 2 m patch
    /// rather than a 1e-7 m one that no amount of zooming out recovers.
    pub const MINIMUM_FRAME_RADIUS: f64 = 1.0;

    /// The near plane's floor as a fraction of the far plane, for an eye
    /// inside the box (fit_depth_range).
    pub const NEAR_FAR_FLOOR: f64 = 1.0e-6;

    /// A thousandth, as for a box wholly behind the eye: what the pivot keeps
    /// between itself and the near plane, so that the ground round what a
    /// zoom went towards - nearer than it towards the bottom of the view - is
    /// not cut either. Flat ground at the bottom edge of a 45 degree view
    /// looking 5 degrees below level at the pivot is a tenth of the pivot's
    /// distance away: sin 5 / sin 50 = 0.11.
    pub const NEAR_PIVOT_FLOOR: f64 = 1.0e-3;

    /// What one wheel notch zooms by: 2x in five notches. Both 3D views and
    /// the cad layer's zoom at a pixel count in it, so the software and GPU
    /// views cannot drift apart.
    pub const ZOOM_PER_NOTCH: f64 = 1.15;

    // ---- framing ---------------------------------------------------------------

    /// Orbit pivot: what pan moves and what dolly moves towards.
    pub fn target(&self) -> Point3 {
        self.target
    }

    pub fn set_target(&mut self, target: Point3) {
        if target.is_finite() {
            self.target = target;
        }
    }

    pub fn distance(&self) -> f64 {
        self.distance
    }

    /// Clamped to a positive minimum: a zero distance has no view direction.
    pub fn set_distance(&mut self, distance: f64) {
        if distance.is_finite() {
            self.distance = max(distance, tolerance::GEOMETRIC);
        }
    }

    pub fn azimuth(&self) -> f64 {
        self.azimuth
    }

    pub fn elevation(&self) -> f64 {
        self.elevation
    }

    /// Elevation is clamped just inside the poles so `up` never becomes
    /// parallel to the view direction; azimuth wraps.
    pub fn set_orientation(&mut self, azimuth: f64, elevation: f64) {
        if !azimuth.is_finite() || !elevation.is_finite() {
            return;
        }
        self.azimuth = wrap_angle(azimuth);
        self.elevation = clamp(elevation, -HALF_PI + POLE_GUARD, HALF_PI - POLE_GUARD);
    }

    pub fn set_standard_view(&mut self, view: StandardView) {
        match view {
            StandardView::Top => self.set_orientation(-HALF_PI, HALF_PI), // straight down, north up
            StandardView::Bottom => self.set_orientation(-HALF_PI, -HALF_PI),
            // The eye to the south, looking north.
            StandardView::Front => self.set_orientation(-HALF_PI, 0.0),
            StandardView::Back => self.set_orientation(HALF_PI, 0.0),
            StandardView::Left => self.set_orientation(3.141592653589793238463, 0.0),
            StandardView::Right => self.set_orientation(0.0, 0.0),
            StandardView::IsoSouthWest => {
                self.set_orientation(-2.356194490192344928847, ISOMETRIC_ELEVATION) // -135 degrees
            }
            StandardView::IsoSouthEast => {
                self.set_orientation(-0.785398163397448309616, ISOMETRIC_ELEVATION) // -45 degrees
            }
            StandardView::IsoNorthEast => {
                self.set_orientation(0.785398163397448309616, ISOMETRIC_ELEVATION)
            }
            StandardView::IsoNorthWest => {
                self.set_orientation(2.356194490192344928847, ISOMETRIC_ELEVATION)
            }
        }
    }

    /// frame_with_margin with the default margin of 6%.
    pub fn frame(&mut self, bounds: AABB) -> bool {
        self.frame_with_margin(bounds, 0.06)
    }

    /// Frames `bounds` so the whole box is visible with a small margin, from
    /// the current direction: the eight corners are projected and the distance
    /// (perspective) and height (orthographic) chosen so they just fit, on the
    /// tighter of the two screen axes. It used to fit the bounding SPHERE,
    /// which left a long corridor as a sliver in the middle of the view (1.5%
    /// of the pixels on a 12 km archive). A box smaller than
    /// MINIMUM_FRAME_RADIUS is grown to it first (audit REN-07), and the depth
    /// range is fitted to the box (fit_depth_range). A box that is empty or
    /// not finite leaves the camera alone and returns false.
    pub fn frame_with_margin(&mut self, bounds: AABB, margin_fraction: f64) -> bool {
        if bounds.empty() || !bounds.min.is_finite() || !bounds.max.is_finite() {
            return false;
        }
        let centre = bounds.center();
        let mut half = (bounds.max - bounds.min) * 0.5;
        if half.length() < Self::MINIMUM_FRAME_RADIUS {
            // Grown on every axis to at least r / sqrt(3), so the half-diagonal
            // reaches the minimum whatever shape the box had (audit REN-07).
            let least = Self::MINIMUM_FRAME_RADIUS / 1.7320508075688772;
            half = Vec3::new(max(half.x, least), max(half.y, least), max(half.z, least));
        }
        let fitted = AABB::new(centre - half, centre + half);
        let margin = 1.0 + max(margin_fraction, 0.0);

        self.set_target(centre);

        // Fit the eight projected corners, not the bounding sphere: a sphere is
        // orientation independent, but it fits a 12 km corridor as if it were
        // 12 km tall as well. A box projects inside the hull of its corners, so
        // the corners alone decide. Before the view has a size the aspect is
        // unknown and taken as square; the view frames again at its first size.
        let a = if self.aspect() > 0.0 {
            self.aspect()
        } else {
            1.0
        };
        let tan_v = (self.fov_y * 0.5).tan();
        let tan_h = tan_v * a;
        let s = self.right();
        let u = self.up();
        let f = self.forward();
        let mut needed = 0.0;
        let mut half_width = 0.0;
        let mut half_height = 0.0;
        for corner in 0..8 {
            let p = corner_of(&fitted, corner);
            let rel = p - centre;
            let x = rel.dot(s).abs();
            let y = rel.dot(u).abs();
            let z = rel.dot(f); // positive: further from the eye than the target
            // At eye distance d the corner is d + z in front of the eye and must
            // sit inside the frustum with the margin to spare.
            needed = max(max(needed, margin * x / tan_h - z), margin * y / tan_v - z);
            half_width = max(half_width, x);
            half_height = max(half_height, y);
        }
        self.set_distance(max(needed, half.length() * 1.0e-3));
        // The tighter of the two axes, so a portrait view does not crop the
        // sides (audit REN-08: this ignored the aspect).
        self.set_orthographic_height(2.0 * margin * max(half_height, half_width / a));
        self.fit_depth_range(fitted);
        true
    }

    /// Fits the near and far planes to `bounds` for the camera as it now is.
    /// Call it every frame, after any orbit, pan or zoom: a depth range fixed
    /// at frame() let a zoom-out push the model past the far plane (blank view
    /// after 8 wheel notches) and wasted the whole depth buffer on the space
    /// in front of the model (audit REN-02, REN-05).
    ///
    /// Perspective: near and far bracket the box's corners along the view
    /// direction; when the eye is inside the box the near plane falls back to
    /// far * NEAR_FAR_FLOOR, which reversed Z can afford (see Clip above), or
    /// to distance() * NEAR_PIVOT_FLOOR when that is nearer. A zoom into a
    /// large scene brings the pivot, which is what it zoomed towards, closer
    /// than a millionth of the far plane, and a floor tied to the far plane
    /// alone cut it away: one stray entity at the origin of a survey at MGA
    /// coordinates put the floor 5 m out (docs/render.md, "Zooming towards the
    /// cursor"). Never below the geometric tolerance.
    ///
    /// Orthographic: the eye is moved back along the view direction until the
    /// whole box is in front of it (orthographic_standoff), because an
    /// orthographic view has no reason to cut away what lies behind an eye
    /// that a zoom-in moved into the model - the elevation views lost their
    /// front that way. What the view shows does not change: under an
    /// orthographic projection the eye's distance only moves the planes.
    ///
    /// False, with the camera untouched, for an empty or non-finite box.
    pub fn fit_depth_range(&mut self, bounds: AABB) -> bool {
        if bounds.empty() || !bounds.min.is_finite() || !bounds.max.is_finite() {
            return false;
        }
        let f = self.forward();
        // A little slack either side, so a vertex exactly on a corner is never
        // cut by rounding and a flat box still has a depth range.
        let pad = max((bounds.max - bounds.min).length() * 1.0e-3, 1.0e-3);

        if self.projection == Projection::Orthographic {
            // How far the box reaches back towards the eye from the target. The
            // eye must be further back than that or the near plane cuts the
            // model, so it is moved back: only the depths change, never what is
            // seen, because an orthographic ray does not depend on where along
            // the view direction its origin sits.
            let mut reach = f64::NEG_INFINITY;
            for corner in 0..8 {
                reach = max(reach, -(corner_of(&bounds, corner) - self.target).dot(f));
            }
            self.standoff = max(0.0, reach + 2.0 * pad - self.distance);
        } else {
            self.standoff = 0.0;
        }

        let e = self.eye();
        let mut nearest = f64::INFINITY;
        let mut furthest = f64::NEG_INFINITY;
        for corner in 0..8 {
            let depth = (corner_of(&bounds, corner) - e).dot(f);
            nearest = min(nearest, depth);
            furthest = max(furthest, depth);
        }
        if !(furthest > 0.0) {
            // The whole box is behind the eye: nothing of it can be seen, but the
            // range must stay valid for whatever else is drawn.
            self.set_depth_range(
                max(self.distance * 1.0e-3, tolerance::GEOMETRIC),
                max(self.distance, 1.0),
            );
            return true;
        }
        let far = furthest + pad;
        // With the eye inside the box `nearest` is negative and the floor decides;
        // reversed Z keeps its precision down there (module comment, Clip). The
        // pivot's share of the floor decides only once the pivot is nearer than
        // a thousandth of the far plane, so a view not zoomed that deep keeps
        // the planes it always had.
        let near = max(
            nearest - pad,
            min(
                far * Self::NEAR_FAR_FLOOR,
                self.distance * Self::NEAR_PIVOT_FLOOR,
            ),
        );
        self.set_depth_range(max(near, tolerance::GEOMETRIC), far);
        true
    }

    /// How far behind `distance()` fit_depth_range had to put an orthographic
    /// eye to keep the scene in front of it; 0 when it did not need to.
    pub fn orthographic_standoff(&self) -> f64 {
        self.standoff
    }

    // ---- projection ------------------------------------------------------------

    pub fn projection(&self) -> Projection {
        self.projection
    }

    pub fn set_projection(&mut self, projection: Projection) {
        self.projection = projection;
    }

    pub fn field_of_view(&self) -> f64 {
        self.fov_y
    }

    pub fn set_field_of_view(&mut self, radians: f64) {
        if radians.is_finite() {
            self.fov_y = clamp(radians, 1.0e-3, 3.0);
        }
    }

    /// World units visible vertically under an orthographic projection.
    pub fn orthographic_height(&self) -> f64 {
        self.ortho_height
    }

    pub fn set_orthographic_height(&mut self, height: f64) {
        if height.is_finite() {
            self.ortho_height = max(height, tolerance::GEOMETRIC);
        }
    }

    pub fn near_plane(&self) -> f64 {
        self.near
    }

    pub fn far_plane(&self) -> f64 {
        self.far
    }

    /// Both must be positive with near < far; ignored otherwise.
    pub fn set_depth_range(&mut self, near_plane: f64, far_plane: f64) {
        if !near_plane.is_finite() || !far_plane.is_finite() {
            return;
        }
        if near_plane <= 0.0 || far_plane <= near_plane {
            return;
        }
        self.near = near_plane;
        self.far = far_plane;
    }

    /// Pixels. A zero or negative size makes the camera produce no matrices.
    pub fn set_viewport_size(&mut self, width: i32, height: i32) {
        self.width = max(width, 0);
        self.height = max(height, 0);
    }

    pub fn viewport_width(&self) -> i32 {
        self.width
    }

    pub fn viewport_height(&self) -> i32 {
        self.height
    }

    pub fn aspect(&self) -> f64 {
        if self.width <= 0 || self.height <= 0 {
            return 0.0;
        }
        f64::from(self.width) / f64::from(self.height)
    }

    // ---- derived ---------------------------------------------------------------

    /// Unit, eye -> target.
    pub fn forward(&self) -> Vec3 {
        let ce = self.elevation.cos();
        // The eye sits at target + distance * (away from the scene); forward is
        // the negation of that, so a positive elevation looks DOWN.
        Vec3::new(
            -ce * self.azimuth.cos(),
            -ce * self.azimuth.sin(),
            -self.elevation.sin(),
        )
    }

    /// Unit, screen +x.
    pub fn right(&self) -> Vec3 {
        // Elevation is clamped off the poles, so forward is never parallel to +Z
        // and this cross product is always well conditioned.
        self.forward().cross(Vec3::new(0.0, 0.0, 1.0)).normalized()
    }

    /// Unit, screen +y (i.e. screen UP).
    pub fn up(&self) -> Vec3 {
        self.right().cross(self.forward()).normalized()
    }

    pub fn eye(&self) -> Point3 {
        // The orthographic standoff moves only the planes (fit_depth_range), so
        // it applies only while the projection is orthographic.
        let back = if self.projection == Projection::Orthographic {
            self.standoff
        } else {
            0.0
        };
        self.target - self.forward() * (self.distance + back)
    }

    pub fn view_matrix(&self) -> Mat4 {
        let f = self.forward();
        let s = self.right();
        let u = self.up();
        let e = self.eye();
        // Rows are the eye-space basis; the translation column is -basis . eye.
        // Eye space looks down -Z, hence the negated third row.
        Mat4::new(
            s.x,
            s.y,
            s.z,
            -s.dot(e),
            u.x,
            u.y,
            u.z,
            -u.dot(e),
            -f.x,
            -f.y,
            -f.z,
            f.dot(e),
            0.0,
            0.0,
            0.0,
            1.0,
        )
    }

    pub fn projection_matrix(&self) -> Mat4 {
        let a = self.aspect();
        if a <= 0.0 {
            return Mat4::default();
        }
        if self.projection == Projection::Perspective {
            let t = 1.0 / (self.fov_y * 0.5).tan();
            let range = self.far - self.near;
            // REVERSED Z (module comment): z_clip = near (z_eye + far) / range
            // and w = -z_eye, so depth = near (far + z_eye) / (range * -z_eye):
            // 1 at z_eye = -near, 0 at z_eye = -far.
            return Mat4::new(
                t / a,
                0.0,
                0.0,
                0.0,
                0.0,
                t,
                0.0,
                0.0,
                0.0,
                0.0,
                self.near / range,
                self.near * self.far / range,
                0.0,
                0.0,
                -1.0,
                0.0,
            );
        }
        let half_height = self.ortho_height * 0.5;
        let half_width = half_height * a;
        let range = self.far - self.near;
        // Reversed and linear: depth = (z_eye + far) / range, 1 at -near, 0 at -far.
        Mat4::new(
            1.0 / half_width,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0 / half_height,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0 / range,
            self.far / range,
            0.0,
            0.0,
            0.0,
            1.0,
        )
    }

    pub fn view_projection(&self) -> Mat4 {
        self.projection_matrix() * self.view_matrix()
    }

    /// World -> screen pixels with depth in [0, 1] (1 at the near plane, 0 at
    /// the far: reversed Z). None when the point is behind the eye plane
    /// (where a projection is meaningless), so callers cannot silently draw a
    /// mirrored point.
    pub fn project(&self, world: Point3) -> Option<Point3> {
        if self.width <= 0 || self.height <= 0 || !world.is_finite() {
            return None;
        }
        let clip = self.view_projection() * Vec4::new(world.x, world.y, world.z, 1.0);
        if clip.w <= 0.0 {
            return None; // at or behind the eye plane
        }
        let ndc_x = clip.x / clip.w;
        let ndc_y = clip.y / clip.w;
        let ndc_z = clip.z / clip.w;
        Some(Point3::new(
            (ndc_x * 0.5 + 0.5) * f64::from(self.width),
            (0.5 - ndc_y * 0.5) * f64::from(self.height), // screen +y is DOWN
            ndc_z,
        ))
    }

    /// Ray from the eye through the centre of pixel (x, y), for picking. Under
    /// an orthographic projection the origin moves and the direction is fixed.
    pub fn ray_through_pixel(&self, x: f64, y: f64) -> Ray {
        let f = self.forward();
        if self.width <= 0 || self.height <= 0 {
            return Ray::new(self.eye(), f);
        }
        // Pixel centres, so pixel (0, 0) samples the middle of the top-left pixel
        // rather than its corner.
        let ndc_x = ((x + 0.5) / f64::from(self.width)) * 2.0 - 1.0;
        let ndc_y = 1.0 - ((y + 0.5) / f64::from(self.height)) * 2.0;
        let s = self.right();
        let u = self.up();

        if self.projection == Projection::Orthographic {
            let half_height = self.ortho_height * 0.5;
            let half_width = half_height * self.aspect();
            let origin = self.eye() + s * (ndc_x * half_width) + u * (ndc_y * half_height);
            return Ray::new(origin, f);
        }
        let half_height = (self.fov_y * 0.5).tan();
        let half_width = half_height * self.aspect();
        let direction = (f + s * (ndc_x * half_width) + u * (ndc_y * half_height)).normalized();
        Ray::new(self.eye(), direction)
    }

    /// World units per pixel at the distance of the target. The number a
    /// viewport needs to turn a pixel aperture into a model-space tolerance.
    pub fn world_per_pixel(&self) -> f64 {
        if self.height <= 0 {
            return 0.0;
        }
        let visible = if self.projection == Projection::Orthographic {
            self.ortho_height
        } else {
            2.0 * self.distance * (self.fov_y * 0.5).tan()
        };
        visible / f64::from(self.height)
    }

    /// World units per pixel at a given distance in front of the eye: the
    /// size of a pixel's footprint there. Constant under an orthographic
    /// projection, proportional to the depth under a perspective one.
    pub fn world_per_pixel_at(&self, depth: f64) -> f64 {
        if self.height <= 0 {
            return 0.0;
        }
        let visible = if self.projection == Projection::Orthographic {
            self.ortho_height
        } else {
            2.0 * max(depth, 0.0) * (self.fov_y * 0.5).tan()
        };
        visible / f64::from(self.height)
    }

    // ---- interaction ------------------------------------------------------------

    pub fn orbit(&mut self, delta_azimuth: f64, delta_elevation: f64) {
        self.set_orientation(
            self.azimuth + delta_azimuth,
            self.elevation + delta_elevation,
        );
    }

    /// Slides the target in the view plane by a screen displacement in pixels:
    /// a point on the target's plane moves exactly with the drag.
    pub fn pan_pixels(&mut self, dx: f64, dy: f64) {
        if !dx.is_finite() || !dy.is_finite() {
            return;
        }
        let scale = self.world_per_pixel();
        // Dragging right moves the SCENE right, so the target moves left; screen
        // +y is down, so a positive dy moves the target up.
        self.set_target(self.target - self.right() * (dx * scale) + self.up() * (dy * scale));
    }

    /// Multiplies the viewing distance (and the orthographic height with it, so
    /// an orthographic view zooms rather than sitting still). Values <= 0
    /// ignored.
    pub fn dolly(&mut self, factor: f64) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        self.set_distance(self.distance * factor);
        // Scaled together, so switching projection mid-session does not change
        // how much of the scene is on screen.
        self.set_orthographic_height(self.ortho_height * factor);
    }

    /// Zoom centred on a pixel: the point under that pixel OF THE TARGET'S
    /// PLANE (through the target, facing the eye) stays under it. Under a
    /// perspective projection that point is not the ground under the pixel
    /// but in the air in front of it or under it, and notch after notch the
    /// eye closes on it - the magnification of the ground stalls, or the eye
    /// goes through the ground. So a 3D view moves the pivot to what the
    /// cursor points at first - what is drawn there, else the datum or the
    /// depth the scene reaches (set_pivot_depth; docs/render.md, "Zooming
    /// towards the cursor").
    pub fn dolly_at_pixel(&mut self, factor: f64, x: f64, y: f64) {
        if !factor.is_finite() || factor <= 0.0 || self.width <= 0 || self.height <= 0 {
            return;
        }
        // Anchor on the point of the target plane under the cursor: zoom, then
        // slide the target so that same world point lands back under the same
        // pixel.
        let before = self.ray_through_pixel(x, y);
        let f = self.forward();
        let denominator = before.direction.dot(f);
        if denominator.abs() < 1.0e-12 {
            self.dolly(factor);
            return;
        }
        let t = (self.target - before.origin).dot(f) / denominator;
        let anchor = before.origin + before.direction * t;

        self.dolly(factor);

        let after = self.ray_through_pixel(x, y);
        let denominator_after = after.direction.dot(f);
        if denominator_after.abs() < 1.0e-12 {
            return;
        }
        let t_after = (self.target - after.origin).dot(f) / denominator_after;
        let moved = after.origin + after.direction * t_after;
        self.set_target(self.target + (anchor - moved));
    }

    /// Moves the pivot along the view axis to `depth` in front of the eye: the
    /// target to eye() + forward() * depth and the distance to `depth`. The eye
    /// stays where it is, so nothing on screen moves; what moves is everything
    /// the pivot's distance governs - the plane dolly_at_pixel anchors on and a
    /// pan drags, the point an orbit turns about, the scale the edges fade by.
    /// Under a perspective projection the orthographic height becomes what
    /// the view shows at that depth, 2 depth tan(fov / 2), so switching to
    /// orthographic keeps the scale there; under an orthographic one the
    /// height is the picture and is kept, and the standoff is folded into the
    /// distance. A depth that is not positive and finite is ignored.
    pub fn set_pivot_depth(&mut self, depth: f64) {
        if !depth.is_finite() || !(depth > 0.0) {
            return;
        }
        // set_distance's floor, applied before the target is placed so that the
        // eye stays exactly where it was.
        let distance = max(depth, tolerance::GEOMETRIC);
        let pivot = self.eye() + self.forward() * distance;
        if !pivot.is_finite() {
            return;
        }
        self.target = pivot;
        self.distance = distance;
        if self.projection == Projection::Perspective {
            self.set_orthographic_height(2.0 * self.distance * (self.fov_y * 0.5).tan());
        } else {
            // eye() is target - forward * (distance + standoff): with the
            // standoff folded into the depth it is the same point.
            // fit_depth_range works the standoff out again at the next frame.
            self.standoff = 0.0;
        }
    }
}

/// Corner `corner` of `bounds`: bit 0 picks max x, bit 1 max y, bit 2 max z.
fn corner_of(bounds: &AABB, corner: i32) -> Vec3 {
    Vec3::new(
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
    )
}
