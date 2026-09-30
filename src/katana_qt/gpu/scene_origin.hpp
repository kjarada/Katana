#pragma once

// Relative-to-centre coordinates and the GPU's projection (docs/gpu.md,
// "Precision").
//
// THE PRECISION RULE. A GPU takes positions and matrices as 32-bit floats. At
// an MGA northing of 6 250 000 m a float has a 24-bit mantissa and so steps in
// 0.5 m: survey data uploaded as it is would snap to a half-metre lattice and
// shimmer as the camera moves. The software rasteriser never meets this because
// it multiplies by the camera in double all the way to clip space
// (render::Rasterizer, transformVertices). The GPU cannot, so:
//
//   * every position is uploaded as a float OFFSET from a scene origin that is
//     kept in double on the CPU (the centre of the scene's box, so the largest
//     offset is half the scene - 6 km on a 12 km corridor, where a float steps
//     in 0.5 mm);
//   * the camera matrix is built in double against that same origin - the
//     eye's position is subtracted from the origin in double BEFORE anything
//     is rounded - and only the finished matrix, whose translation is now
//     scene-sized rather than earth-sized, is rounded to float.
//
// Nothing here knows about Qt: it is plain double arithmetic, testable without
// a GPU, and the one place the rule is written down in code.
//
// DEPTH. The GPU draws with REVERSED Z (the near plane at depth 1, the far at
// 0, cleared to 0, test "greater"). A float's exponent then spends its
// precision where the perspective divide throws it away, and the two cancel:
// depth resolution becomes roughly constant RELATIVE to distance, instead of
// the few hundred distinct depth values the software path's standard Z
// gives a framed TIN (docs/gpu.md). Perspective uses an INFINITE far plane - nothing can be clipped
// away by zooming out (audit REN-05) - so the camera's far plane only matters
// to the orthographic projection, which has no divide and takes it as given.

#include <array>

#include "katana/math/mat4.hpp"
#include "katana/math/primitives.hpp"
#include "katana/render/camera.hpp"

namespace katana::qt::gpu {

using katana::math::Mat4;
using katana::math::Vec3;

// The origin positions are uploaded relative to: the centre of `bounds`, or
// (0, 0, 0) for an empty or non-finite box, which has nothing to be precise
// about.
[[nodiscard]] Vec3 chooseSceneOrigin(const katana::math::AABB& bounds);

// THE ORIGIN FOLLOWS A DEEP ZOOM. The centre of the scene bounds the error
// only while the view is as large as the scene: zoomed in close to a point
// far from it, the float offsets there round to more than a pixel - 0.25 m
// steps on a site 3000 km from its origin, which one stray entity at (0, 0)
// under a survey at MGA coordinates makes the centre of the scene's box. So
// the host packs its layers again against the camera's pivot once this
// bound, at the pivot, passes kOriginErrorPixels (RenderViewWidget::
// prepareGpuFrame; docs/gpu.md, "Precision").
//
// The bound, in the camera's pixels: a float rounds to at most u = 2^-24 of
// what it holds, and a dot product of n terms is out by at most n u of the
// sum of their sizes (Higham, Accuracy and Stability of Numerical
// Algorithms, 2nd ed., section 3.1). At the pivot a vertex's offset from the
// origin (one rounding) meets the matrix's translation, the eye's offset
// from the origin, about as large (one rounding), in the shader's four-term
// dot product (four roundings of the two): 10 u |target - origin|, over a
// pixel's footprint at the pivot (Camera::worldPerPixel). Zero for a camera
// with no viewport.
[[nodiscard]] double originErrorPixels(const katana::render::Camera& camera, const Vec3& origin);
// A twentieth of a pixel, of a bound on the worst case: what the view draws
// with the origin it has cannot be seen to differ from what the double
// arithmetic of the software view draws. Measured with the shader's float
// arithmetic, the error stays under the bound for origins from a metre to
// 3000 km from the pivot, and with the origin at the pivot it is under a
// thousandth of a pixel
// (RelativeViewProjection.AtTheOriginsErrorBoundThePivotIsDrawnWithinItAndItsOwnOriginDrawsItExactly).
// Re-packing is a pass over every vertex, so it happens when this is
// crossed and not before. The new origin is the pivot itself, so it is
// crossed again only once the pivot has drifted 0.05 / (10 x 2^-24), about
// 84 000, of the view's pixels from it.
inline constexpr double kOriginErrorPixels = 0.05;

// What the graphics API's clip space looks like. Direct3D, the only backend
// the runtime HLSL shaders serve, is the default: y up in NDC and depth in
// [0, 1] - the camera's own convention (render/camera.hpp), so its correction
// is the identity. Carried so that a precompiled-shader build targeting Vulkan
// (y down) or OpenGL (depth in [-1, 1]) corrects here and nowhere else.
struct ClipSpace {
    bool yUpInNdc = true;
    bool depthZeroToOne = true;
};

struct ProjectionOptions {
    // Perspective only; see DEPTH above.
    bool infiniteFar = true;
    ClipSpace clip{};
};

// World-to-eye for positions given relative to `origin`: the camera's view
// matrix times a translation by `origin`, computed as one step in double so
// the eye-minus-origin subtraction happens before any rounding.
[[nodiscard]] Mat4 relativeViewMatrix(const katana::render::Camera& camera, const Vec3& origin);

// Reversed-Z projection of the camera's frustum: near plane at depth 1; at 0
// for the far plane (orthographic, or perspective with infiniteFar false) or
// for infinity. Identity when the camera has no viewport size.
[[nodiscard]] Mat4 reversedZProjection(const katana::render::Camera& camera,
                                       const ProjectionOptions& options = {});

// Projection times relative view, in double: the one matrix the vertex shader
// multiplies by.
[[nodiscard]] Mat4 relativeViewProjection(const katana::render::Camera& camera,
                                          const Vec3& origin,
                                          const ProjectionOptions& options = {});

// `position - origin`, subtracted in double and only then rounded.
[[nodiscard]] std::array<float, 3> relativePosition(const Vec3& position, const Vec3& origin);

// A matrix as the shaders read it: column-major floats (HLSL's default packing
// for a float4x4 in a constant buffer, used with mul(matrix, vector)).
[[nodiscard]] std::array<float, 16> toFloatColumnMajor(const Mat4& matrix);

} // namespace katana::qt::gpu
