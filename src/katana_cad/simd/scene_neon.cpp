// NEON kernels of the 3D scene build (cad::SceneBuilder), for 64-bit ARM.
// Compiled by katana_add_simd_sources.
//
// A KERNEL FILE (see src/katana_core/simd/text_neon.cpp and
// cmake/KatanaSimd.cmake): raw pointers only, nothing visible to the linker but
// the katana_neon_ entries, no includes but <arm_neon.h>, <cstddef>, <cstdint>
// and the declarations.
//
// Every lane does the operations of scene.cpp's scalar code in its order:
// Vec3::cross, std::hypot of three, Light::intensity, render::shade and
// elevationRampColor, each product and sum a separate FMUL and FADD (FMLA is a
// baseline AArch64 instruction; -ffp-contract=off keeps the compiler from
// using it, and the object check fails the build if it ever did). std::min,
// std::max and std::clamp are compares and BSL selects, because FMIN/FMAX
// answer otherwise for a NaN and for signed zeros. Conversions to a channel
// truncate (FCVTZS), as static_cast<std::uint8_t> of a double does.
//
// Where scene_avx2.cpp keeps a point in one 256-bit register as x y z w, two
// doubles a register make that awkward, so here the lanes are two points (or
// two faces) and each coordinate has its register: LD3/LD4 split the arrays
// into them and ST3/ST4 put them back.

#include <arm_neon.h>

#include <cstddef>
#include <cstdint>

#include "scene_kernels.hpp"

namespace {

namespace k = katana::cad::simd;

inline float64x2_t add(float64x2_t a, float64x2_t b) { return vaddq_f64(a, b); }
inline float64x2_t sub(float64x2_t a, float64x2_t b) { return vsubq_f64(a, b); }
inline float64x2_t mul(float64x2_t a, float64x2_t b) { return vmulq_f64(a, b); }
inline float64x2_t div(float64x2_t a, float64x2_t b) { return vdivq_f64(a, b); }
inline float64x2_t splat(double v) { return vdupq_n_f64(v); }

// std::clamp(v, 0.0, 1.0): v < 0 ? 0 : (1 < v ? 1 : v), a NaN coming through.
inline float64x2_t clamp01(float64x2_t v)
{
    const float64x2_t zero = splat(0.0);
    const float64x2_t one = splat(1.0);
    return vbslq_f64(vcltq_f64(v, zero), zero, vbslq_f64(vcltq_f64(one, v), one, v));
}

// std::hypot(x, y, z) exactly as the standard library this file is compiled
// against computes it, for the scalar code beside it calls that one.
inline float64x2_t hypot3(float64x2_t x, float64x2_t y, float64x2_t z)
{
#if defined(_LIBCPP_VERSION) && _LIBCPP_VERSION >= 190000
    // libc++ (macOS) from LLVM 19: scaled by a power of two when the squares
    // could overflow or underflow, then sqrt(x x + y y + z z) / scale. Its
    // largest magnitude is taken with fmax, which FMAXNM is: a NaN operand
    // gives the other, and after fabs there are no signed zeros to order.
    // 2^512 and 2^-532 are its numeric_limits<double>::max_exponent / 2 and
    // -(max_exponent / 2 + 20).
    const float64x2_t largest = vmaxnmq_f64(vabsq_f64(x), vmaxnmq_f64(vabsq_f64(y), vabsq_f64(z)));
    const float64x2_t threshold = splat(0x1p512);
    const float64x2_t shrink = splat(0x1p-532);
    const float64x2_t grow = splat(1.0 / 0x1p-532);
    const float64x2_t scale = vbslq_f64(vcgtq_f64(largest, threshold), shrink,
                                        vbslq_f64(vcltq_f64(largest, div(splat(1.0), threshold)),
                                                  grow, splat(1.0)));
    x = mul(x, scale);
    y = mul(y, scale);
    z = mul(z, scale);
    return div(vsqrtq_f64(add(add(mul(x, x), mul(y, y)), mul(z, z))), scale);
#elif defined(_LIBCPP_VERSION)
    // libc++ before LLVM 19: the plain formula.
    return vsqrtq_f64(add(add(mul(x, x), mul(y, y)), mul(z, z)));
#else
    // libstdc++ (__hypot3): a is the largest magnitude, picked by the same
    // comparisons, and a zero a gives +0.
    x = vabsq_f64(x);
    y = vabsq_f64(y);
    z = vabsq_f64(z);
    const float64x2_t yz = vbslq_f64(vcltq_f64(y, z), z, y);
    const float64x2_t xz = vbslq_f64(vcltq_f64(x, z), z, x);
    const float64x2_t a = vbslq_f64(vcltq_f64(x, y), yz, xz);
    const float64x2_t qx = div(x, a);
    const float64x2_t qy = div(y, a);
    const float64x2_t qz = div(z, a);
    const float64x2_t root = vsqrtq_f64(add(add(mul(qx, qx), mul(qy, qy)), mul(qz, qz)));
    // if (a): nonzero, a NaN counting as true. BIC clears the lanes where a
    // is zero, which leaves +0.
    return vreinterpretq_f64_u64(
        vbicq_u64(vreinterpretq_u64_f64(mul(a, root)), vceqq_f64(a, splat(0.0))));
#endif
}

// Lift: datum + (z - datum) * factor.
inline float64x2_t lift(float64x2_t z, float64x2_t factor, float64x2_t datum)
{
    return add(datum, mul(sub(z, datum), factor));
}

struct Light {
    bool on;
    float64x2_t sunX, sunY, sunZ, ground, skyMinusGround, sun;
};

Light lightOf(const double* params)
{
    return Light{params[k::kParamLightOn] != 0.0,     splat(params[k::kParamSunX]),
                 splat(params[k::kParamSunY]),        splat(params[k::kParamSunZ]),
                 splat(params[k::kParamGround]),      splat(params[k::kParamSkyMinusGround]),
                 splat(params[k::kParamSun])};
}

// Light::intensity of the normal n / length, which is a unit normal.
inline float64x2_t intensity(const Light& light, float64x2_t x, float64x2_t y, float64x2_t z,
                             float64x2_t length)
{
    if (!light.on) {
        return splat(1.0);
    }
    const float64x2_t half = splat(0.5);
    const float64x2_t zero = splat(0.0);
    x = div(x, length);
    y = div(y, length);
    z = div(z, length);
    const float64x2_t hemisphere = add(light.ground, mul(light.skyMinusGround, add(half, mul(half, z))));
    const float64x2_t towards = add(add(mul(x, light.sunX), mul(y, light.sunY)), mul(z, light.sunZ));
    // std::max(0.0, d) is 0 < d ? d : 0, so a NaN d gives 0.
    const float64x2_t lit = vbslq_f64(vcgtq_f64(towards, zero), towards, zero);
    return clamp01(add(hemisphere, mul(light.sun, lit)));
}

// A channel of two colours as doubles, and back: the value is an exact small
// integer, and FCVTZS of value * f + 0.5 truncates as the scalar cast does.
inline float64x2_t channelOf(const std::uint32_t (&colours)[2], int shift)
{
    const double pair[2] = {static_cast<double>((colours[0] >> shift) & 0xFFu),
                            static_cast<double>((colours[1] >> shift) & 0xFFu)};
    return vld1q_f64(pair);
}

// render::shade(colour, factor) of two colours in the lanes where `apply` is
// set; the others keep their colour. The alpha byte is never touched.
inline void shade(std::uint32_t (&colours)[2], float64x2_t factor, uint64x2_t apply)
{
    const float64x2_t f = clamp01(factor);
    std::uint64_t applied[2];
    vst1q_u64(applied, apply);
    std::uint32_t shaded[2] = {colours[0] & 0xFF000000u, colours[1] & 0xFF000000u};
    for (int shift = 16; shift >= 0; shift -= 8) {
        std::int64_t channel[2];
        vst1q_s64(channel, vcvtq_s64_f64(add(mul(channelOf(colours, shift), f), splat(0.5))));
        for (int j = 0; j < 2; ++j) {
            shaded[j] |= (static_cast<std::uint32_t>(channel[j]) & 0xFFu) << shift;
        }
    }
    for (int j = 0; j < 2; ++j) {
        if (applied[j] != 0) {
            colours[j] = shaded[j];
        }
    }
}

// elevationRampColor((z - low) / span) of two elevations.
inline void ramp(float64x2_t z, const double* params, std::uint32_t (&colours)[2])
{
    float64x2_t t = div(sub(z, splat(params[k::kParamRampLow])), splat(params[k::kParamRampSpan]));
    // std::isfinite(t) ? t : 0.0 - |t| < inf is false for inf and NaN alike.
    const uint64x2_t finite = vcltq_f64(vabsq_f64(t), splat(__builtin_inf()));
    t = vreinterpretq_f64_u64(vandq_u64(vreinterpretq_u64_f64(t), finite));
    const float64x2_t clamped = mul(clamp01(t), splat(4.0));
    // std::min(static_cast<int>(clamped), kStops - 2): clamped is in [0, 4].
    std::int64_t whole[2];
    vst1q_s64(whole, vcvtq_s64_f64(clamped));
    const std::int64_t last = static_cast<std::int64_t>(k::kRampSegments) - 1;
    const std::size_t lower[2] = {static_cast<std::size_t>(whole[0] < last ? whole[0] : last),
                                  static_cast<std::size_t>(whole[1] < last ? whole[1] : last)};
    const double lowerAsDouble[2] = {static_cast<double>(lower[0]), static_cast<double>(lower[1])};
    const float64x2_t f = sub(clamped, vld1q_f64(lowerAsDouble));
    const double* table = params + k::kParamRampTable;
    colours[0] = 0xFF000000u;
    colours[1] = 0xFF000000u;
    for (std::size_t channel = 0; channel < 3; ++channel) {
        const double* stops = table + 2 * channel * k::kRampSegments;
        const double* steps = stops + k::kRampSegments;
        const double a[2] = {stops[lower[0]], stops[lower[1]]};
        const double step[2] = {steps[lower[0]], steps[lower[1]]};
        std::int64_t value[2];
        vst1q_s64(value, vcvtq_s64_f64(
                             add(add(vld1q_f64(a), mul(vld1q_f64(step), f)), splat(0.5))));
        const int shift = 16 - 8 * static_cast<int>(channel);
        for (int j = 0; j < 2; ++j) {
            colours[j] |= (static_cast<std::uint32_t>(value[j]) & 0xFFu) << shift;
        }
    }
}

// Two whole vertices of katana_neon_scene_surface_vertices.
inline void surfaceVertices(const double* lifted, const double* normals, const double* params,
                            const Light& light, std::uint32_t flat, bool useRamp, double* positions,
                            std::uint32_t* colours)
{
    // x, y, the lifted z and the true z of each vertex.
    const float64x2x4_t v = vld4q_f64(lifted);
    const float64x2x3_t xyz = {{v.val[0], v.val[1], v.val[2]}};
    vst3q_f64(positions, xyz);

    std::uint32_t colour[2] = {flat, flat};
    if (useRamp) {
        ramp(v.val[3], params, colour);
    }
    if (normals != nullptr) {
        const float64x2x4_t n = vld4q_f64(normals);
        const float64x2_t length = hypot3(n.val[0], n.val[1], n.val[2]);
        const uint64x2_t lit = vcgtq_f64(length, splat(0.0));
        shade(colour, intensity(light, n.val[0], n.val[1], n.val[2], length), lit);
    }
    colours[0] = colour[0];
    colours[1] = colour[1];
}

// Two faces of katana_neon_scene_mesh_faces: face j's indices at corners[j],
// its number (for its base colour) in number[j].
inline void meshFaces(const double* points, const std::uint32_t* const corners[2],
                      const std::size_t number[2], const double* params, const Light& light,
                      const std::uint32_t* faceColours, std::size_t faceColourCount,
                      std::uint32_t flat, double* positions, std::uint32_t* colours)
{
    const float64x2_t factor = splat(params[k::kParamLiftFactor]);
    const float64x2_t datum = splat(params[k::kParamLiftDatum]);
    // Corner `c` of both faces, coordinate by coordinate, lifted.
    const auto corner = [&](int c, float64x2_t& x, float64x2_t& y, float64x2_t& z) {
        const double* p0 = points + 3 * static_cast<std::size_t>(corners[0][c]);
        const double* p1 = points + 3 * static_cast<std::size_t>(corners[1][c]);
        const double xs[2] = {p0[0], p1[0]};
        const double ys[2] = {p0[1], p1[1]};
        const double zs[2] = {p0[2], p1[2]};
        x = vld1q_f64(xs);
        y = vld1q_f64(ys);
        z = lift(vld1q_f64(zs), factor, datum);
    };
    float64x2_t ax;
    float64x2_t ay;
    float64x2_t az;
    float64x2_t bx;
    float64x2_t by;
    float64x2_t bz;
    float64x2_t cx;
    float64x2_t cy;
    float64x2_t cz;
    corner(0, ax, ay, az);
    corner(1, bx, by, bz);
    corner(2, cx, cy, cz);

    // (b - a).cross(c - a): y*o.z - z*o.y, z*o.x - x*o.z, x*o.y - y*o.x.
    const float64x2_t ux = sub(bx, ax);
    const float64x2_t uy = sub(by, ay);
    const float64x2_t uz = sub(bz, az);
    const float64x2_t vx = sub(cx, ax);
    const float64x2_t vy = sub(cy, ay);
    const float64x2_t vz = sub(cz, az);
    const float64x2_t nx = sub(mul(uy, vz), mul(uz, vy));
    const float64x2_t ny = sub(mul(uz, vx), mul(ux, vz));
    const float64x2_t nz = sub(mul(ux, vy), mul(uy, vx));

    // Corners in the order c, b, a, as scene.cpp adds them.
    const float64x2_t out[9] = {cx, cy, cz, bx, by, bz, ax, ay, az};
    for (int j = 0; j < 9; ++j) {
        positions[j] = vgetq_lane_f64(out[j], 0);
        positions[9 + j] = vgetq_lane_f64(out[j], 1);
    }

    const float64x2_t area = hypot3(nx, ny, nz);
    const uint64x2_t lit = vcgtq_f64(area, splat(1.0e-12));
    std::uint32_t colour[2] = {number[0] < faceColourCount ? faceColours[number[0]] : flat,
                               number[1] < faceColourCount ? faceColours[number[1]] : flat};
    shade(colour, intensity(light, nx, ny, nz, area), lit);
    // Each face's colour on its three vertices.
    for (int j = 0; j < 3; ++j) {
        colours[j] = colour[0];
        colours[3 + j] = colour[1];
    }
}

} // namespace

extern "C" void katana_neon_scene_lift(const double* points, std::size_t count,
                                       const double* params, double* lifted)
{
    const double factorValue = params[k::kParamLiftFactor];
    const double datumValue = params[k::kParamLiftDatum];
    const float64x2_t factor = splat(factorValue);
    const float64x2_t datum = splat(datumValue);
    std::size_t i = 0;
    for (; i + 2 <= count; i += 2) {
        const float64x2x3_t p = vld3q_f64(points + 3 * i);
        const float64x2x4_t out = {{p.val[0], p.val[1], lift(p.val[2], factor, datum), p.val[2]}};
        vst4q_f64(lifted + 4 * i, out);
    }
    for (; i < count; ++i) {
        const double* p = points + 3 * i;
        double* out = lifted + 4 * i;
        out[0] = p[0];
        out[1] = p[1];
        out[2] = datumValue + (p[2] - datumValue) * factorValue;
        out[3] = p[2];
    }
}

extern "C" void katana_neon_scene_surface_normals(const double* lifted,
                                                  const std::uint32_t* triangles,
                                                  std::size_t count, double* normals)
{
    // One triangle at a time, in order: each adds to its corners' running
    // sums, and a later triangle may share them. The normal's x and y are
    // one register - (y, z) * (o.z, o.x) - (z, x) * (o.y, o.z) - and z is
    // x * o.y - y * o.x on its own.
    for (std::size_t t = 0; t < count; ++t) {
        const std::uint32_t* corner = triangles + 3 * t;
        const double* a = lifted + 4 * static_cast<std::size_t>(corner[0]);
        const double* b = lifted + 4 * static_cast<std::size_t>(corner[1]);
        const double* c = lifted + 4 * static_cast<std::size_t>(corner[2]);
        const float64x2_t uxy = sub(vld1q_f64(b), vld1q_f64(a));
        const float64x2_t vxy = sub(vld1q_f64(c), vld1q_f64(a));
        const double uz = b[2] - a[2];
        const double vz = c[2] - a[2];
        const float64x2_t uyz = vcombine_f64(vget_high_f64(uxy), vdup_n_f64(uz));
        const float64x2_t vyz = vcombine_f64(vget_high_f64(vxy), vdup_n_f64(vz));
        const float64x2_t uzx = vcombine_f64(vdup_n_f64(uz), vget_low_f64(uxy));
        const float64x2_t vzx = vcombine_f64(vdup_n_f64(vz), vget_low_f64(vxy));
        float64x2_t nxy = sub(mul(uyz, vzx), mul(uzx, vyz));
        double nz = vgetq_lane_f64(uxy, 0) * vgetq_lane_f64(vxy, 1) -
                    vgetq_lane_f64(uxy, 1) * vgetq_lane_f64(vxy, 0);
        // if (n.z < 0.0) n = n * -1.0; a multiply, not a negation, as the
        // scalar code has it (the two differ in the sign of a NaN).
        if (nz < 0.0) {
            nxy = mul(nxy, splat(-1.0));
            nz = nz * -1.0;
        }
        // One corner at a time, in order: a repeated corner adds twice. The
        // fourth double of each sum is never read, and is left alone.
        for (int j = 0; j < 3; ++j) {
            double* sum = normals + 4 * static_cast<std::size_t>(corner[j]);
            vst1q_f64(sum, add(vld1q_f64(sum), nxy));
            sum[2] = sum[2] + nz;
        }
    }
}

extern "C" void katana_neon_scene_surface_vertices(const double* lifted, std::size_t count,
                                                   const double* normals, const double* params,
                                                   std::uint32_t flat, int useRamp,
                                                   double* positions, std::uint32_t* colours)
{
    const Light light = lightOf(params);
    std::size_t i = 0;
    for (; i + 2 <= count; i += 2) {
        surfaceVertices(lifted + 4 * i, normals != nullptr ? normals + 4 * i : nullptr, params,
                        light, flat, useRamp != 0, positions + 3 * i, colours + i);
    }
    if (i == count) {
        return;
    }
    // The last one through the same code, padded: a zero normal is left
    // unshaded, and the padding's results are thrown away.
    double inLifted[8] = {};
    double inNormals[8] = {};
    double outPositions[6];
    std::uint32_t outColours[2];
    for (std::size_t j = 0; j < 4; ++j) {
        inLifted[j] = lifted[4 * i + j];
        if (normals != nullptr) {
            inNormals[j] = normals[4 * i + j];
        }
    }
    surfaceVertices(inLifted, normals != nullptr ? inNormals : nullptr, params, light, flat,
                    useRamp != 0, outPositions, outColours);
    positions[3 * i] = outPositions[0];
    positions[3 * i + 1] = outPositions[1];
    positions[3 * i + 2] = outPositions[2];
    colours[i] = outColours[0];
}

extern "C" void katana_neon_scene_mesh_faces(const double* points, std::size_t vertexCount,
                                             const std::uint32_t* faces, std::size_t first,
                                             std::size_t count, const double* params,
                                             const std::uint32_t* faceColours,
                                             std::size_t faceColourCount, std::uint32_t flat,
                                             double* positions, std::uint32_t* colours)
{
    (void)vertexCount; // the caller has checked every index against it
    const Light light = lightOf(params);
    const std::uint32_t* corners[2];
    std::size_t number[2];
    std::size_t done = 0;
    for (; done + 2 <= count; done += 2) {
        for (std::size_t j = 0; j < 2; ++j) {
            number[j] = first + done + j;
            corners[j] = faces + 3 * number[j];
        }
        meshFaces(points, corners, number, params, light, faceColours, faceColourCount, flat,
                  positions + 9 * done, colours + 3 * done);
    }
    if (done == count) {
        return;
    }
    // The last one, padded with a copy of itself.
    double outPositions[18];
    std::uint32_t outColours[6];
    number[0] = first + done;
    number[1] = first + done;
    corners[0] = faces + 3 * number[0];
    corners[1] = corners[0];
    meshFaces(points, corners, number, params, light, faceColours, faceColourCount, flat,
              outPositions, outColours);
    for (std::size_t j = 0; j < 9; ++j) {
        positions[9 * done + j] = outPositions[j];
    }
    for (std::size_t j = 0; j < 3; ++j) {
        colours[3 * done + j] = outColours[j];
    }
}

// mix(lo, hi, e / 8) of every channel, as scene_avx2.cpp has it: flooring
// 8 lo + (hi - lo) e + 4 over 8, which fits a 16-bit lane (and wraps
// harmlessly through a negative (hi - lo) e on the way).
extern "C" void katana_neon_scene_fade(const std::uint32_t* base, const std::uint32_t* ink,
                                       std::size_t count, int eighths, std::uint32_t* out)
{
    const uint16x8_t e = vdupq_n_u16(static_cast<std::uint16_t>(eighths));
    const uint16x8_t four = vdupq_n_u16(4);
    const auto channel = [&](uint8x8_t lo8, uint8x8_t hi8) {
        const uint16x8_t lo = vmovl_u8(lo8);
        const uint16x8_t moved = vmulq_u16(vsubq_u16(vmovl_u8(hi8), lo), e);
        return vmovn_u16(vshrq_n_u16(vaddq_u16(vaddq_u16(vshlq_n_u16(lo, 3), moved), four), 3));
    };
    std::size_t i = 0;
    // Four colours, sixteen channel bytes, a step: each byte is one channel,
    // and every channel is mixed alike.
    for (; i + 4 <= count; i += 4) {
        const uint8x16_t b = vreinterpretq_u8_u32(vld1q_u32(base + i));
        const uint8x16_t n = vreinterpretq_u8_u32(vld1q_u32(ink + i));
        const uint8x16_t mixed = vcombine_u8(channel(vget_low_u8(b), vget_low_u8(n)),
                                             channel(vget_high_u8(b), vget_high_u8(n)));
        vst1q_u32(out + i, vreinterpretq_u32_u8(mixed));
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
