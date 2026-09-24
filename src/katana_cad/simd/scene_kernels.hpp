#pragma once

// Entry points of the AVX2 scene kernels, compiled from scene_avx2.cpp, and
// the layout of the parameters scene.cpp hands them. Declarations and
// constants only; see src/katana_core/simd/text_kernels.hpp for why the entries
// have C linkage and a katana_avx2_ prefix.
//
// Each entry equals the scalar code of scene.cpp bit for bit: the same
// operations, in the same order, never fused. The layouts:
//
//   points   3 doubles per point: x, y, z (math::Vec3).
//   lifted   4 doubles per vertex: x, y, the exaggerated z, the TRUE z (what
//            the elevation ramp reads).
//   normals  4 doubles per vertex: the x, y, z of the summed face normals, and
//            a fourth that is never read.
//   params   doubles, indexed by SceneKernelParam.
//   colours  0xAARRGGBB, as render::Rgba.

#include <cstddef>
#include <cstdint>

namespace katana::cad::simd {

// The rows of the elevation ramp a kernel interpolates: for each channel, the
// four lower stops and the step to the next one, as doubles (the step is an
// exact small integer, so the kernel's a + step * f is the scalar
// a + (b - a) * f to the bit).
constexpr std::size_t kRampSegments = 4; // internal linkage: no symbol in a kernel object

enum SceneKernelParam : std::size_t {
    kParamLiftFactor = 0,
    kParamLiftDatum,
    kParamRampLow,
    kParamRampSpan,
    kParamLightOn, // 1.0 or 0.0
    kParamSunX,
    kParamSunY,
    kParamSunZ,
    kParamGround,
    kParamSkyMinusGround,
    kParamSun,
    // Red stops, red steps, green stops, green steps, blue stops, blue steps.
    kParamRampTable,
    kParamCount = kParamRampTable + 6 * kRampSegments,
};

} // namespace katana::cad::simd

extern "C" {

// lifted[4 i ..] = x, y, datum + (z - datum) * factor, z for each of `count`
// points.
void katana_avx2_scene_lift(const double* points, std::size_t count, const double* params,
                            double* lifted);

// For each of `count` triangles in order, the normal (b - a) x (c - a) of its
// lifted corners, negated when its z is below zero, added to the normals of
// its three corners in the order a, b, c. `normals` holds the sums so far.
void katana_avx2_scene_surface_normals(const double* lifted, const std::uint32_t* triangles,
                                       std::size_t count, double* normals);

// Each of `count` vertices: its lifted position into positions (3 doubles
// each), and its colour - `flat`, or the ramp at its true z when useRamp -
// shaded by the hillshade of its normal when normals is not null.
void katana_avx2_scene_surface_vertices(const double* lifted, std::size_t count,
                                        const double* normals, const double* params,
                                        std::uint32_t flat, int useRamp, double* positions,
                                        std::uint32_t* colours);

// Faces [first, first + count) of a mesh, every index of which names one of
// the `vertexCount` points: the lifted corners, c then b then a, into positions (9 doubles a
// face) and the face's colour, shaded by its own normal, three times into
// colours. A face's base colour is faceColours[f] for f < faceColourCount,
// else `flat`.
void katana_avx2_scene_mesh_faces(const double* points, std::size_t vertexCount,
                                  const std::uint32_t* faces, std::size_t first, std::size_t count,
                                  const double* params, const std::uint32_t* faceColours,
                                  std::size_t faceColourCount, std::uint32_t flat,
                                  double* positions, std::uint32_t* colours);

// out[i] = each channel of base[i] moved eighths/8 of the way to ink[i],
// rounded half up, for eighths in 0..8.
void katana_avx2_scene_fade(const std::uint32_t* base, const std::uint32_t* ink, std::size_t count,
                            int eighths, std::uint32_t* out);

} // extern "C"
