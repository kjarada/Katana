// AVX2 kernels of the 3D scene build (cad::SceneBuilder). Compiled with
// -mavx2 -mfma by katana_add_simd_sources.
//
// A KERNEL FILE (see src/katana_core/simd/text_avx2.cpp and
// cmake/KatanaSimd.cmake): raw pointers only, nothing visible to the linker but
// the katana_avx2_ entries, no includes but intrinsics, <cstddef>, <cstdint>
// and the declarations.
//
// Every lane does the operations of scene.cpp's scalar code in its order:
// Vec3::cross, std::hypot of three (libstdc++'s __hypot3: the largest
// magnitude, three quotients, a square root), Light::intensity, render::shade
// and elevationRampColor. -ffp-contract=off keeps the compiler from fusing a
// multiply and an add, which would round once instead of twice; the object
// check fails the build if it ever did. Conversions to a channel truncate
// (CVTTPD2DQ), as static_cast<std::uint8_t> of a double does.

#include <immintrin.h>

#include <cstddef>
#include <cstdint>

#include "scene_kernels.hpp"

namespace {

namespace k = katana::cad::simd;

inline __m256d add(__m256d a, __m256d b) { return _mm256_add_pd(a, b); }
inline __m256d sub(__m256d a, __m256d b) { return _mm256_sub_pd(a, b); }
inline __m256d mul(__m256d a, __m256d b) { return _mm256_mul_pd(a, b); }
inline __m256d div(__m256d a, __m256d b) { return _mm256_div_pd(a, b); }
inline __m256d lessThan(__m256d a, __m256d b) { return _mm256_cmp_pd(a, b, _CMP_LT_OQ); }

// The three doubles at p, and a zero: never reads the fourth, which may be
// past the end of the array.
inline __m256d load3(const double* p)
{
    return _mm256_maskload_pd(p, _mm256_setr_epi64x(-1, -1, -1, 0));
}
inline void store3(double* p, __m256d v)
{
    _mm256_maskstore_pd(p, _mm256_setr_epi64x(-1, -1, -1, 0), v);
}

inline __m256d magnitude(__m256d v) { return _mm256_andnot_pd(_mm256_set1_pd(-0.0), v); }

// std::clamp(v, 0.0, 1.0): v < 0 ? 0 : (1 < v ? 1 : v). MINPD(1, v) is
// 1 < v ? 1 : v, a NaN in v included.
inline __m256d clamp01(__m256d v)
{
    const __m256d zero = _mm256_setzero_pd();
    return _mm256_blendv_pd(_mm256_min_pd(_mm256_set1_pd(1.0), v), zero, lessThan(v, zero));
}

// std::hypot(x, y, z) as libstdc++ computes it (__hypot3): a is the largest
// magnitude, picked by the same comparisons, and a zero a gives +0.
inline __m256d hypot3(__m256d x, __m256d y, __m256d z)
{
    x = magnitude(x);
    y = magnitude(y);
    z = magnitude(z);
    const __m256d yz = _mm256_blendv_pd(y, z, lessThan(y, z));
    const __m256d xz = _mm256_blendv_pd(x, z, lessThan(x, z));
    const __m256d a = _mm256_blendv_pd(xz, yz, lessThan(x, y));
    const __m256d qx = div(x, a);
    const __m256d qy = div(y, a);
    const __m256d qz = div(z, a);
    const __m256d root = _mm256_sqrt_pd(add(add(mul(qx, qx), mul(qy, qy)), mul(qz, qz)));
    // if (a): nonzero, a NaN counting as true.
    return _mm256_and_pd(mul(a, root), _mm256_cmp_pd(a, _mm256_setzero_pd(), _CMP_NEQ_UQ));
}

// Vec3::cross of the first three lanes: y*o.z - z*o.y, z*o.x - x*o.z,
// x*o.y - y*o.x.
inline __m256d cross(__m256d u, __m256d v)
{
    const __m256d uYZX = _mm256_permute4x64_pd(u, _MM_SHUFFLE(3, 0, 2, 1));
    const __m256d vZXY = _mm256_permute4x64_pd(v, _MM_SHUFFLE(3, 1, 0, 2));
    const __m256d uZXY = _mm256_permute4x64_pd(u, _MM_SHUFFLE(3, 1, 0, 2));
    const __m256d vYZX = _mm256_permute4x64_pd(v, _MM_SHUFFLE(3, 0, 2, 1));
    return sub(mul(uYZX, vZXY), mul(uZXY, vYZX));
}

// The first three lanes of four rows, as columns.
inline void transpose3(__m256d r0, __m256d r1, __m256d r2, __m256d r3, __m256d& x, __m256d& y,
                       __m256d& z)
{
    const __m256d lo01 = _mm256_unpacklo_pd(r0, r1); // r0x r1x r0z r1z
    const __m256d hi01 = _mm256_unpackhi_pd(r0, r1); // r0y r1y r0w r1w
    const __m256d lo23 = _mm256_unpacklo_pd(r2, r3);
    const __m256d hi23 = _mm256_unpackhi_pd(r2, r3);
    x = _mm256_permute2f128_pd(lo01, lo23, 0x20);
    y = _mm256_permute2f128_pd(hi01, hi23, 0x20);
    z = _mm256_permute2f128_pd(lo01, lo23, 0x31);
}

// The fourth lane of four rows.
inline __m256d fourth(__m256d r0, __m256d r1, __m256d r2, __m256d r3)
{
    return _mm256_permute2f128_pd(_mm256_unpackhi_pd(r0, r1), _mm256_unpackhi_pd(r2, r3), 0x31);
}

// (x, y, datum + (z - datum) * factor, 0) of a point (x, y, z, 0): Lift.
inline __m256d liftPoint(__m256d point, __m256d factor, __m256d datum)
{
    return _mm256_blend_pd(point, add(datum, mul(sub(point, datum), factor)), 0b0100);
}

struct Light {
    bool on;
    __m256d sunX, sunY, sunZ, ground, skyMinusGround, sun;
};

Light lightOf(const double* params)
{
    return Light{params[k::kParamLightOn] != 0.0,
                 _mm256_set1_pd(params[k::kParamSunX]),
                 _mm256_set1_pd(params[k::kParamSunY]),
                 _mm256_set1_pd(params[k::kParamSunZ]),
                 _mm256_set1_pd(params[k::kParamGround]),
                 _mm256_set1_pd(params[k::kParamSkyMinusGround]),
                 _mm256_set1_pd(params[k::kParamSun])};
}

// Light::intensity of the normal n / length, which is a unit normal.
inline __m256d intensity(const Light& light, __m256d x, __m256d y, __m256d z, __m256d length)
{
    if (!light.on) {
        return _mm256_set1_pd(1.0);
    }
    const __m256d half = _mm256_set1_pd(0.5);
    x = div(x, length);
    y = div(y, length);
    z = div(z, length);
    const __m256d hemisphere = add(light.ground, mul(light.skyMinusGround, add(half, mul(half, z))));
    const __m256d towards = add(add(mul(x, light.sunX), mul(y, light.sunY)), mul(z, light.sunZ));
    // std::max(0.0, d) is 0 < d ? d : 0; MAXPD(d, 0) is d > 0 ? d : 0.
    return clamp01(add(hemisphere, mul(light.sun, _mm256_max_pd(towards, _mm256_setzero_pd()))));
}

// Channel values of four colours, as int32 lanes.
struct Channels {
    __m128i a, r, g, b;
};

inline Channels channelsOf(__m128i colours)
{
    const __m128i byte = _mm_set1_epi32(0xFF);
    return Channels{_mm_srli_epi32(colours, 24),
                    _mm_and_si128(_mm_srli_epi32(colours, 16), byte),
                    _mm_and_si128(_mm_srli_epi32(colours, 8), byte), _mm_and_si128(colours, byte)};
}

inline __m128i pack(const Channels& c)
{
    return _mm_or_si128(_mm_or_si128(_mm_slli_epi32(c.a, 24), _mm_slli_epi32(c.r, 16)),
                        _mm_or_si128(_mm_slli_epi32(c.g, 8), c.b));
}

// render::shade(colour, factor) in the lanes where `apply` is set; the others
// keep their channel (an exact integer, which truncates to itself).
inline __m128i shadeChannel(__m128i channel, __m256d factor, __m256d apply)
{
    const __m256d value = _mm256_cvtepi32_pd(channel);
    const __m256d shaded = add(mul(value, factor), _mm256_set1_pd(0.5));
    return _mm256_cvttpd_epi32(_mm256_blendv_pd(value, shaded, apply));
}

inline __m128i shade(__m128i colours, __m256d factor, __m256d apply)
{
    Channels c = channelsOf(colours);
    const __m256d f = clamp01(factor);
    c.r = shadeChannel(c.r, f, apply);
    c.g = shadeChannel(c.g, f, apply);
    c.b = shadeChannel(c.b, f, apply);
    return pack(c);
}

// elevationRampColor((z - low) / span) of four elevations.
inline __m128i ramp(__m256d z, const double* params)
{
    __m256d t = div(sub(z, _mm256_set1_pd(params[k::kParamRampLow])),
                    _mm256_set1_pd(params[k::kParamRampSpan]));
    // std::isfinite(t) ? t : 0.0
    t = _mm256_and_pd(t, lessThan(magnitude(t), _mm256_set1_pd(__builtin_inf())));
    const __m256d clamped = mul(clamp01(t), _mm256_set1_pd(4.0));
    const __m128i lower =
        _mm_min_epi32(_mm256_cvttpd_epi32(clamped), _mm_set1_epi32(static_cast<int>(k::kRampSegments) - 1));
    const __m256d f = sub(clamped, _mm256_cvtepi32_pd(lower));
    // Each row of the table is four doubles, one register: picking entry
    // `lower` is a permute of its 32-bit halves (2 lower, 2 lower + 1), which
    // costs a fraction of a gather from memory.
    const __m256i twice = _mm256_slli_epi64(_mm256_cvtepu32_epi64(lower), 1);
    const __m256i pick =
        _mm256_or_si256(twice, _mm256_slli_epi64(_mm256_add_epi64(twice, _mm256_set1_epi64x(1)), 32));
    const double* table = params + k::kParamRampTable;
    const auto row = [&](std::size_t index) {
        const __m256i entries = _mm256_castpd_si256(_mm256_loadu_pd(table + index * k::kRampSegments));
        return _mm256_castsi256_pd(_mm256_permutevar8x32_epi32(entries, pick));
    };
    const auto blend = [&](std::size_t channel) {
        const __m256d a = row(2 * channel);
        const __m256d step = row(2 * channel + 1);
        return _mm256_cvttpd_epi32(add(add(a, mul(step, f)), _mm256_set1_pd(0.5)));
    };
    return pack(Channels{_mm_set1_epi32(0xFF), blend(0), blend(1), blend(2)});
}

// Four whole vertices of katana_avx2_scene_surface_vertices.
inline void surfaceVertices(const double* lifted, const double* normals, const double* params,
                            const Light& light, std::uint32_t flat, bool useRamp, double* positions,
                            std::uint32_t* colours)
{
    const __m256d r0 = _mm256_loadu_pd(lifted);
    const __m256d r1 = _mm256_loadu_pd(lifted + 4);
    const __m256d r2 = _mm256_loadu_pd(lifted + 8);
    const __m256d r3 = _mm256_loadu_pd(lifted + 12);
    // In order, each store's fourth double landing on the next vertex's x
    // before that vertex's own store; the last writes three only.
    _mm256_storeu_pd(positions, r0);
    _mm256_storeu_pd(positions + 3, r1);
    _mm256_storeu_pd(positions + 6, r2);
    store3(positions + 9, r3);

    __m128i colour = useRamp ? ramp(fourth(r0, r1, r2, r3), params)
                             : _mm_set1_epi32(static_cast<int>(flat));
    if (normals != nullptr) {
        __m256d x;
        __m256d y;
        __m256d z;
        transpose3(_mm256_loadu_pd(normals), _mm256_loadu_pd(normals + 4),
                   _mm256_loadu_pd(normals + 8), _mm256_loadu_pd(normals + 12), x, y, z);
        const __m256d length = hypot3(x, y, z);
        const __m256d lit = _mm256_cmp_pd(length, _mm256_setzero_pd(), _CMP_GT_OQ);
        colour = shade(colour, intensity(light, x, y, z, length), lit);
    }
    _mm_storeu_si128(reinterpret_cast<__m128i*>(colours), colour);
}

// Four faces of katana_avx2_scene_mesh_faces: face j's indices at corners[j],
// its number (for its base colour) in number[j].
inline void meshFaces(const double* points, const std::uint32_t* const corners[4],
                      const std::size_t number[4], const double* params, const Light& light,
                      const std::uint32_t* faceColours, std::size_t faceColourCount,
                      std::uint32_t flat, double* positions, std::uint32_t* colours)
{
    const __m256d factor = _mm256_set1_pd(params[k::kParamLiftFactor]);
    const __m256d datum = _mm256_set1_pd(params[k::kParamLiftDatum]);
    __m256d normal[4];
    for (int j = 0; j < 4; ++j) {
        const std::uint32_t* c = corners[j];
        const __m256d a = liftPoint(load3(points + 3 * static_cast<std::size_t>(c[0])), factor, datum);
        const __m256d b = liftPoint(load3(points + 3 * static_cast<std::size_t>(c[1])), factor, datum);
        const __m256d d = liftPoint(load3(points + 3 * static_cast<std::size_t>(c[2])), factor, datum);
        normal[j] = cross(sub(b, a), sub(d, a));
        // Corners in the order c, b, a, as scene.cpp adds them.
        double* out = positions + 9 * j;
        _mm256_storeu_pd(out, d);
        _mm256_storeu_pd(out + 3, b);
        store3(out + 6, a); // the next face's store would cover a fourth, but it may be the last
    }
    __m256d x;
    __m256d y;
    __m256d z;
    transpose3(normal[0], normal[1], normal[2], normal[3], x, y, z);
    const __m256d area = hypot3(x, y, z);
    const __m256d lit = _mm256_cmp_pd(area, _mm256_set1_pd(1.0e-12), _CMP_GT_OQ);
    std::uint32_t base[4];
    for (int j = 0; j < 4; ++j) {
        base[j] = number[j] < faceColourCount ? faceColours[number[j]] : flat;
    }
    const __m128i shaded = shade(_mm_loadu_si128(reinterpret_cast<const __m128i*>(base)),
                                 intensity(light, x, y, z, area), lit);
    // Each face's colour on its three vertices.
    auto* out = reinterpret_cast<__m128i*>(colours);
    _mm_storeu_si128(out, _mm_shuffle_epi32(shaded, _MM_SHUFFLE(1, 0, 0, 0)));
    _mm_storeu_si128(out + 1, _mm_shuffle_epi32(shaded, _MM_SHUFFLE(2, 2, 1, 1)));
    _mm_storeu_si128(out + 2, _mm_shuffle_epi32(shaded, _MM_SHUFFLE(3, 3, 3, 2)));
}

} // namespace

extern "C" void katana_avx2_scene_lift(const double* points, std::size_t count,
                                       const double* params, double* lifted)
{
    const __m256d factor = _mm256_set1_pd(params[k::kParamLiftFactor]);
    const __m256d datum = _mm256_set1_pd(params[k::kParamLiftDatum]);
    for (std::size_t i = 0; i < count; ++i) {
        // (x, y, z, z), then the third lifted.
        const __m256d point =
            _mm256_permute4x64_pd(load3(points + 3 * i), _MM_SHUFFLE(2, 2, 1, 0));
        _mm256_storeu_pd(lifted + 4 * i, liftPoint(point, factor, datum));
    }
}

extern "C" void katana_avx2_scene_surface_normals(const double* lifted,
                                                  const std::uint32_t* triangles,
                                                  std::size_t count, double* normals)
{
    const __m256d zero = _mm256_setzero_pd();
    const __m256d minusOne = _mm256_set1_pd(-1.0);
    for (std::size_t t = 0; t < count; ++t) {
        const std::uint32_t* corner = triangles + 3 * t;
        const __m256d a = _mm256_loadu_pd(lifted + 4 * static_cast<std::size_t>(corner[0]));
        const __m256d b = _mm256_loadu_pd(lifted + 4 * static_cast<std::size_t>(corner[1]));
        const __m256d c = _mm256_loadu_pd(lifted + 4 * static_cast<std::size_t>(corner[2]));
        __m256d n = cross(sub(b, a), sub(c, a));
        // if (n.z < 0.0) n = n * -1.0;
        const __m256d down = lessThan(_mm256_permute4x64_pd(n, _MM_SHUFFLE(2, 2, 2, 2)), zero);
        n = _mm256_blendv_pd(n, mul(n, minusOne), down);
        // One corner at a time, in order: a repeated corner adds twice.
        for (int j = 0; j < 3; ++j) {
            double* sum = normals + 4 * static_cast<std::size_t>(corner[j]);
            _mm256_storeu_pd(sum, add(_mm256_loadu_pd(sum), n));
        }
    }
}

extern "C" void katana_avx2_scene_surface_vertices(const double* lifted, std::size_t count,
                                                   const double* normals, const double* params,
                                                   std::uint32_t flat, int useRamp,
                                                   double* positions, std::uint32_t* colours)
{
    const Light light = lightOf(params);
    std::size_t i = 0;
    for (; i + 4 <= count; i += 4) {
        surfaceVertices(lifted + 4 * i, normals != nullptr ? normals + 4 * i : nullptr, params,
                        light, flat, useRamp != 0, positions + 3 * i, colours + i);
    }
    if (i == count) {
        return;
    }
    // The last one to three through the same code, padded: a zero normal is
    // left unshaded, and the padding's results are thrown away.
    double inLifted[16] = {};
    double inNormals[16] = {};
    double outPositions[12];
    std::uint32_t outColours[4];
    const std::size_t rest = count - i;
    for (std::size_t j = 0; j < 4 * rest; ++j) {
        inLifted[j] = lifted[4 * i + j];
        if (normals != nullptr) {
            inNormals[j] = normals[4 * i + j];
        }
    }
    surfaceVertices(inLifted, normals != nullptr ? inNormals : nullptr, params, light, flat,
                    useRamp != 0, outPositions, outColours);
    for (std::size_t j = 0; j < rest; ++j) {
        positions[3 * (i + j)] = outPositions[3 * j];
        positions[3 * (i + j) + 1] = outPositions[3 * j + 1];
        positions[3 * (i + j) + 2] = outPositions[3 * j + 2];
        colours[i + j] = outColours[j];
    }
}

extern "C" void katana_avx2_scene_mesh_faces(const double* points, std::size_t vertexCount,
                                             const std::uint32_t* faces, std::size_t first,
                                             std::size_t count, const double* params,
                                             const std::uint32_t* faceColours,
                                             std::size_t faceColourCount, std::uint32_t flat,
                                             double* positions, std::uint32_t* colours)
{
    (void)vertexCount; // the caller has checked every index against it
    const Light light = lightOf(params);
    const std::uint32_t* corners[4];
    std::size_t number[4];
    std::size_t done = 0;
    for (; done + 4 <= count; done += 4) {
        for (std::size_t j = 0; j < 4; ++j) {
            number[j] = first + done + j;
            corners[j] = faces + 3 * number[j];
        }
        meshFaces(points, corners, number, params, light, faceColours, faceColourCount, flat,
                  positions + 9 * done, colours + 3 * done);
    }
    if (done == count) {
        return;
    }
    // The last one to three, padded with copies of the last face.
    double outPositions[36];
    std::uint32_t outColours[12];
    const std::size_t rest = count - done;
    for (std::size_t j = 0; j < 4; ++j) {
        number[j] = first + done + (j < rest ? j : rest - 1);
        corners[j] = faces + 3 * number[j];
    }
    meshFaces(points, corners, number, params, light, faceColours, faceColourCount, flat,
              outPositions, outColours);
    for (std::size_t j = 0; j < 9 * rest; ++j) {
        positions[9 * done + j] = outPositions[j];
    }
    for (std::size_t j = 0; j < 3 * rest; ++j) {
        colours[3 * done + j] = outColours[j];
    }
}

// mix(lo, hi, e / 8) of every channel: lo + (hi - lo) * e/8 + 0.5 is exact in a
// double (a multiple of 1/16 below 512), and never below 0.5, so truncating it
// is flooring 8 lo + (hi - lo) e + 4 over 8 - which fits a 16-bit lane.
extern "C" void katana_avx2_scene_fade(const std::uint32_t* base, const std::uint32_t* ink,
                                       std::size_t count, int eighths, std::uint32_t* out)
{
    const __m256i zero = _mm256_setzero_si256();
    const __m256i e = _mm256_set1_epi16(static_cast<short>(eighths));
    const __m256i four = _mm256_set1_epi16(4);
    const auto channel = [&](__m256i lo, __m256i hi) {
        const __m256i moved = _mm256_mullo_epi16(_mm256_sub_epi16(hi, lo), e);
        return _mm256_srli_epi16(_mm256_add_epi16(_mm256_add_epi16(_mm256_slli_epi16(lo, 3), moved), four), 3);
    };
    std::size_t i = 0;
    for (; i + 8 <= count; i += 8) {
        const __m256i b = _mm256_loadu_si256(reinterpret_cast<const __m256i*>(base + i));
        const __m256i n = _mm256_loadu_si256(reinterpret_cast<const __m256i*>(ink + i));
        // Unpacking and packing both work within 128-bit halves, so the
        // bytes come back in their places.
        const __m256i low = channel(_mm256_unpacklo_epi8(b, zero), _mm256_unpacklo_epi8(n, zero));
        const __m256i high = channel(_mm256_unpackhi_epi8(b, zero), _mm256_unpackhi_epi8(n, zero));
        _mm256_storeu_si256(reinterpret_cast<__m256i*>(out + i), _mm256_packus_epi16(low, high));
    }
    for (; i < count; ++i) {
        std::uint32_t result = 0;
        for (int shift = 0; shift < 32; shift += 8) {
            const int lo = static_cast<int>((base[i] >> shift) & 0xFFu);
            const int hi = static_cast<int>((ink[i] >> shift) & 0xFFu);
            result |= static_cast<std::uint32_t>((8 * lo + (hi - lo) * eighths + 4) >> 3) << shift;
        }
        out[i] = result;
    }
}
