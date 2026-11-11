//! Relative-to-centre coordinates and the GPU's projection (docs/gpu.md,
//! "Precision"; scene_origin.hpp).
//!
//! THE PRECISION RULE. A GPU takes positions and matrices as 32-bit floats. At
//! an MGA northing of 6 250 000 m a float has a 24-bit mantissa and so steps in
//! 0.5 m: survey data uploaded as it is would snap to a half-metre lattice and
//! shimmer as the camera moves. The software rasteriser never meets this because
//! it multiplies by the camera in double all the way to clip space
//! (katana_render's Rasterizer, transform_vertices). The GPU cannot, so:
//!
//! * every position is uploaded as a float OFFSET from a scene origin that is
//!   kept in double on the CPU (the centre of the scene's box, so the largest
//!   offset is half the scene: 6 km on a 12 km corridor, where a float steps
//!   in 0.5 mm);
//! * the camera matrix is built in double against that same origin - the
//!   eye's position is subtracted from the origin in double BEFORE anything
//!   is rounded - and only the finished matrix, whose translation is now
//!   scene-sized rather than earth-sized, is rounded to float.
//!
//! Nothing here knows about wgpu: it is plain double arithmetic, testable
//! without a GPU, and the one place the rule is written down in code.
//!
//! DEPTH. The GPU draws with REVERSED Z (the near plane at depth 1, the far at
//! 0, cleared to 0, test "greater"). A float's exponent then spends its
//! precision where the perspective divide throws it away, and the two cancel:
//! depth resolution becomes roughly constant RELATIVE to distance, instead of
//! the few hundred distinct depth values the software path's standard Z gives
//! a framed TIN (docs/gpu.md). Perspective uses an INFINITE far plane, so
//! nothing can be clipped away by zooming out (audit REN-05), and the camera's
//! far plane only matters to the orthographic projection, which has no divide
//! and takes it as given.

use katana_math::{AABB, Mat4, Vec3};
use katana_render::{Camera, Projection};

/// The origin positions are uploaded relative to: the centre of `bounds`, or
/// (0, 0, 0) for an empty or non-finite box, which has nothing to be precise
/// about.
pub fn choose_scene_origin(bounds: AABB) -> Vec3 {
    if bounds.empty() || !bounds.min.is_finite() || !bounds.max.is_finite() {
        return Vec3::new(0.0, 0.0, 0.0);
    }
    bounds.center()
}

/// THE ORIGIN FOLLOWS A DEEP ZOOM. The centre of the scene bounds the error
/// only while the view is as large as the scene: zoomed in close to a point
/// far from it, the float offsets there round to more than a pixel - 0.25 m
/// steps on a site 3000 km from its origin, which one stray entity at (0, 0)
/// under a survey at MGA coordinates makes the centre of the scene's box. So
/// the host packs its layers again against the camera's pivot once this
/// bound, at the pivot, passes ORIGIN_ERROR_PIXELS (docs/gpu.md, "Precision").
///
/// The bound, in the camera's pixels: a float rounds to at most u = 2^-24 of
/// what it holds, and a dot product of n terms is out by at most n u of the
/// sum of their sizes (Higham, Accuracy and Stability of Numerical
/// Algorithms, 2nd ed., section 3.1). At the pivot a vertex's offset from the
/// origin (one rounding) meets the matrix's translation, the eye's offset
/// from the origin, about as large (one rounding), in the shader's four-term
/// dot product (four roundings of the two): 10 u |target - origin|, over a
/// pixel's footprint at the pivot (Camera::world_per_pixel). Zero for a
/// camera with no viewport.
pub fn origin_error_pixels(camera: &Camera, origin: Vec3) -> f64 {
    let footprint = camera.world_per_pixel();
    if !(footprint > 0.0) {
        return 0.0;
    }
    // 2^-24, written as the exact quotient: Rust has no hexadecimal float.
    const FLOAT_ROUNDOFF: f64 = 1.0 / 16_777_216.0;
    const ROUNDINGS: f64 = 10.0;
    ROUNDINGS * FLOAT_ROUNDOFF * (camera.target() - origin).length() / footprint
}

/// A twentieth of a pixel, of a bound on the worst case: what the view draws
/// with the origin it has cannot be seen to differ from what the double
/// arithmetic of the software view draws. Measured with the shader's float
/// arithmetic, the error stays under the bound for origins from a metre to
/// 3000 km from the pivot, and with the origin at the pivot it is under a
/// thousandth of a pixel
/// (RelativeViewProjection.AtTheOriginsErrorBoundThePivotIsDrawnWithinItAndItsOwnOriginDrawsItExactly).
/// Re-packing is a pass over every vertex, so it happens when this is crossed
/// and not before. The new origin is the pivot itself, so it is crossed again
/// only once the pivot has drifted 0.05 / (10 x 2^-24), about 84 000, of the
/// view's pixels from it.
pub const ORIGIN_ERROR_PIXELS: f64 = 0.05;

/// What the graphics API's clip space looks like. y up in NDC with depth in
/// [0, 1] is the camera's own convention (katana_render's camera) and
/// WebGPU's, which wgpu presents on every backend - it flips Vulkan's y
/// itself - so the renderer always draws with the default and the correction
/// is the identity. The other forms are kept so that a projection for a clip
/// space that differs (y down, or OpenGL's depth in [-1, 1]) is corrected
/// here and nowhere else.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClipSpace {
    pub y_up_in_ndc: bool,
    pub depth_zero_to_one: bool,
}

impl Default for ClipSpace {
    fn default() -> Self {
        Self {
            y_up_in_ndc: true,
            depth_zero_to_one: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectionOptions {
    /// Perspective only; see DEPTH above.
    pub infinite_far: bool,
    pub clip: ClipSpace,
}

impl Default for ProjectionOptions {
    fn default() -> Self {
        Self {
            infinite_far: true,
            clip: ClipSpace::default(),
        }
    }
}

/// World-to-eye for positions given relative to `origin`: the camera's view
/// matrix times a translation by `origin`, computed as one step in double so
/// the eye-minus-origin subtraction happens before any rounding.
pub fn relative_view_matrix(camera: &Camera, origin: Vec3) -> Mat4 {
    let f = camera.forward();
    let s = camera.right();
    let u = camera.up();
    // The eye relative to the origin, in double: this subtraction is the whole
    // precision rule. Written out rather than as view_matrix() * translation(o)
    // because that product forms s.eye and s.origin separately - two
    // earth-sized numbers - and only then cancels them.
    let e = camera.eye() - origin;
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

/// Reversed-Z projection of the camera's frustum: near plane at depth 1; at 0
/// for the far plane (orthographic, or perspective with infinite_far false) or
/// for infinity. Identity when the camera has no viewport size.
pub fn reversed_z_projection(camera: &Camera, options: &ProjectionOptions) -> Mat4 {
    let a = camera.aspect();
    if a <= 0.0 {
        return Mat4::default();
    }
    let n = camera.near_plane();
    let f = camera.far_plane();
    let projection = if camera.projection() == Projection::Perspective {
        let t = 1.0 / (camera.field_of_view() * 0.5).tan();
        // Eye-space z is negative in front of the eye and clip w = -z. For
        // depth = (A z + B) / -z to be 1 at z = -n and 0 at z = -f:
        //   (-A n + B) / n = 1  and  -A f + B = 0
        //   => A = n / (f - n), B = n f / (f - n).
        // As f goes to infinity A -> 0 and B -> n: depth = n / distance.
        let mut depth_a = 0.0;
        let mut depth_b = n;
        if !options.infinite_far {
            depth_a = n / (f - n);
            depth_b = n * f / (f - n);
        }
        Mat4::new(
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
            depth_a,
            depth_b,
            0.0,
            0.0,
            -1.0,
            0.0,
        )
    } else {
        let half_height = camera.orthographic_height() * 0.5;
        let half_width = half_height * a;
        // No divide: depth = A z + B with A(-n) + B = 1 and A(-f) + B = 0
        //   => A = 1 / (f - n), B = f / (f - n).
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
            1.0 / (f - n),
            f / (f - n),
            0.0,
            0.0,
            0.0,
            1.0,
        )
    };

    // Clip-space corrections, applied after the projection; WebGPU's needs
    // none. The two touch different rows, so their order does not matter.
    let mut correction = Mat4::default();
    if !options.clip.depth_zero_to_one {
        // [0, 1] -> [-1, 1]: z' = 2z - w.
        correction = Mat4::new(
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 2.0, -1.0, //
            0.0, 0.0, 0.0, 1.0,
        );
    }
    if !options.clip.y_up_in_ndc {
        correction[(1, 1)] = -1.0;
    }
    correction * projection
}

/// Projection times relative view, in double: the one matrix the vertex shader
/// multiplies by.
pub fn relative_view_projection(
    camera: &Camera,
    origin: Vec3,
    options: &ProjectionOptions,
) -> Mat4 {
    reversed_z_projection(camera, options) * relative_view_matrix(camera, origin)
}

/// `position - origin`, subtracted in double and only then rounded.
pub fn relative_position(position: Vec3, origin: Vec3) -> [f32; 3] {
    [
        (position.x - origin.x) as f32,
        (position.y - origin.y) as f32,
        (position.z - origin.z) as f32,
    ]
}

/// A matrix as the shaders read it: column-major floats, WGSL's layout of a
/// `mat4x4<f32>` in a uniform block, used as `mvp * v`.
pub fn to_float_column_major(matrix: Mat4) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for column in 0..4 {
        for row in 0..4 {
            out[column * 4 + row] = matrix[(row, column)] as f32;
        }
    }
    out
}
