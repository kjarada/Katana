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
// the software path's 289 distinct depth values across a framed TIN
// (map_view3d). Perspective uses an INFINITE far plane - nothing can be clipped
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
