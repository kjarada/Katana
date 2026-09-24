// AVX2 terrain kernels. Compiled with -mavx2 -mfma by katana_add_simd_sources.
//
// A KERNEL FILE (see src/katana_core/simd/text_avx2.cpp and
// cmake/KatanaSimd.cmake): raw pointers only, nothing visible to the linker but
// the katana_avx2_ entries, no includes but intrinsics, <cstddef>, <cstdint>
// and the declarations.
//
// Four triangles per step, one per lane. Each lane does the operations of the
// scalar reference in the scalar reference's order, so every value rounds as
// it does there; -ffp-contract=off keeps the compiler from fusing a multiply
// with an add, and the object check fails the build if it ever did.
//
// No scalar tail: the last, partial step repeats the last valid triangle (or
// candidate) in the lanes past the end, which reads only memory the caller
// owns, and those lanes' answers are dropped.

#include <immintrin.h>

#include <cstddef>
#include <cstdint>

#include "terrain_kernels.hpp"

namespace {

inline __m256d sub(__m256d a, __m256d b) { return _mm256_sub_pd(a, b); }
inline __m256d mul(__m256d a, __m256d b) { return _mm256_mul_pd(a, b); }

// Offsets 3 * min(first + lane, last) for the four lanes: the start of each
// lane's triple in a stride-3 array, with the lanes past `last` repeating it.
inline __m128i tripleOffsets(std::size_t first, std::size_t last)
{
    const __m128i lane = _mm_setr_epi32(0, 1, 2, 3);
    const __m128i index =
        _mm_min_epi32(_mm_add_epi32(_mm_set1_epi32(static_cast<int>(first)), lane),
                      _mm_set1_epi32(static_cast<int>(last)));
    return _mm_add_epi32(_mm_add_epi32(index, index), index);
}

inline __m128i gatherIndex(const std::uint32_t* base, __m128i offsets)
{
    return _mm_i32gather_epi32(reinterpret_cast<const int*>(base), offsets, 4);
}

inline __m128i times3(__m128i v) { return _mm_add_epi32(_mm_add_epi32(v, v), v); }

// Component `axis` of the vertices whose offsets (3 * index) are given.
inline __m256d gatherCoordinate(const double* vertices, __m128i offsets, int axis)
{
    return _mm256_i32gather_pd(vertices + axis, offsets, 8);
}

// firstLevelAbove of contours.cpp, lane by lane, on the active lanes: the
// first k with base + k * interval > z, from floor((z - base) / interval) and
// settled by the same two loops. k is kept as an integral double rather than
// an int64: the reference's static_cast<double>(k) of the int64 it cast from
// that same floor is that double again (|k| < 2^53, which contours() checks),
// and k - 1 and k + 1 are exact there too, so every level compared is the
// reference's level. A zero may come out as -0.0 where the reference has 0;
// it only ever meets a comparison and the final cast, which do not see signs.
inline __m256d firstLevelAbove(__m256d z, __m256d base, __m256d interval, __m256d active)
{
    const __m256d one = _mm256_set1_pd(1.0);
    __m256d k = _mm256_floor_pd(_mm256_div_pd(sub(z, base), interval));
    for (;;) {
        const __m256d level = _mm256_add_pd(base, mul(k, interval));
        const __m256d above = _mm256_and_pd(_mm256_cmp_pd(level, z, _CMP_GT_OQ), active);
        if (_mm256_movemask_pd(above) == 0) {
            break;
        }
        k = sub(k, _mm256_and_pd(above, one));
    }
    for (;;) {
        const __m256d level = _mm256_add_pd(base, mul(k, interval));
        // !(level > z): true when unordered too, as the reference's negation is.
        const __m256d notAbove = _mm256_and_pd(_mm256_cmp_pd(level, z, _CMP_NGT_UQ), active);
        if (_mm256_movemask_pd(notAbove) == 0) {
            break;
        }
        k = _mm256_add_pd(k, _mm256_and_pd(notAbove, one));
    }
    return k;
}

} // namespace

extern "C" void katana_avx2_contour_level_ranges(const double* vertices,
                                                 const std::uint32_t* triangles,
                                                 std::size_t begin, std::size_t end, double base,
                                                 double interval, std::int64_t* ranges)
{
    if (begin >= end) {
        return;
    }
    const __m256d baseV = _mm256_set1_pd(base);
    const __m256d intervalV = _mm256_set1_pd(interval);
    for (std::size_t t = begin; t < end; t += 4) {
        const __m128i tri = tripleOffsets(t, end - 1);
        const __m256d z0 = gatherCoordinate(vertices, times3(gatherIndex(triangles, tri)), 2);
        const __m256d z1 = gatherCoordinate(
            vertices, times3(gatherIndex(triangles, _mm_add_epi32(tri, _mm_set1_epi32(1)))), 2);
        const __m256d z2 = gatherCoordinate(
            vertices, times3(gatherIndex(triangles, _mm_add_epi32(tri, _mm_set1_epi32(2)))), 2);
        // std::min({z0, z1, z2}) keeps the running value unless the next is
        // strictly less; MINPD with the new value first is that exactly, NaNs
        // and zeros included. Likewise std::max and MAXPD.
        const __m256d low = _mm256_min_pd(z2, _mm256_min_pd(z1, z0));
        const __m256d high = _mm256_max_pd(z2, _mm256_max_pd(z1, z0));
        const __m256d active = _mm256_cmp_pd(low, high, _CMP_LT_OQ);
        const int activeMask = _mm256_movemask_pd(active);
        if (activeMask == 0) {
            continue;
        }
        alignas(32) double first[4];
        alignas(32) double pastLast[4];
        _mm256_store_pd(first, firstLevelAbove(low, baseV, intervalV, active));
        _mm256_store_pd(pastLast, firstLevelAbove(high, baseV, intervalV, active));
        const std::size_t lanes = end - t < 4 ? end - t : 4;
        for (std::size_t lane = 0; lane < lanes; ++lane) {
            if ((activeMask >> lane) & 1) {
                ranges[2 * (t + lane)] = static_cast<std::int64_t>(first[lane]);
                ranges[2 * (t + lane) + 1] = static_cast<std::int64_t>(pastLast[lane]) - 1;
            }
        }
    }
}

extern "C" std::size_t katana_avx2_first_enclosing(const double* vertices,
                                                   const std::uint32_t* triangles,
                                                   const std::uint32_t* candidates,
                                                   std::size_t start, std::size_t count, double px,
                                                   double py, double* d)
{
    const __m256d x = _mm256_set1_pd(px);
    const __m256d y = _mm256_set1_pd(py);
    for (std::size_t i = start; i < count; i += 4) {
        // Candidate ids through the same clamped offsets, divided back by 3:
        // the lanes past the end repeat the last candidate.
        const __m128i lane = _mm_setr_epi32(0, 1, 2, 3);
        const __m128i index =
            _mm_min_epi32(_mm_add_epi32(_mm_set1_epi32(static_cast<int>(i)), lane),
                          _mm_set1_epi32(static_cast<int>(count - 1)));
        const __m128i tri = times3(gatherIndex(candidates, index));
        const __m128i va = times3(gatherIndex(triangles, tri));
        const __m128i vb = times3(gatherIndex(triangles, _mm_add_epi32(tri, _mm_set1_epi32(1))));
        const __m128i vc = times3(gatherIndex(triangles, _mm_add_epi32(tri, _mm_set1_epi32(2))));
        const __m256d ax = gatherCoordinate(vertices, va, 0);
        const __m256d ay = gatherCoordinate(vertices, va, 1);
        const __m256d bx = gatherCoordinate(vertices, vb, 0);
        const __m256d by = gatherCoordinate(vertices, vb, 1);
        const __m256d cx = gatherCoordinate(vertices, vc, 0);
        const __m256d cy = gatherCoordinate(vertices, vc, 1);
        // Vec2::cross is x * other.y - y * other.x, on the differences the
        // reference forms: (b - a).cross(p - a) and so on round the edges.
        const __m256d d0 = sub(mul(sub(bx, ax), sub(y, ay)), mul(sub(by, ay), sub(x, ax)));
        const __m256d d1 = sub(mul(sub(cx, bx), sub(y, by)), mul(sub(cy, by), sub(x, bx)));
        const __m256d d2 = sub(mul(sub(ax, cx), sub(y, cy)), mul(sub(ay, cy), sub(x, cx)));
        const __m256d zero = _mm256_setzero_pd();
        // The reference rejects on `d0 < 0.0 || d1 < 0.0 || d2 < 0.0`, so a
        // NaN edge function does not reject: ordered less-than, then negated.
        const __m256d rejected = _mm256_or_pd(
            _mm256_or_pd(_mm256_cmp_pd(d0, zero, _CMP_LT_OQ), _mm256_cmp_pd(d1, zero, _CMP_LT_OQ)),
            _mm256_cmp_pd(d2, zero, _CMP_LT_OQ));
        const std::size_t lanes = count - i < 4 ? count - i : 4;
        const int accepted =
            ~_mm256_movemask_pd(rejected) & ((1 << static_cast<int>(lanes)) - 1);
        if (accepted != 0) {
            const int first = __builtin_ctz(static_cast<unsigned>(accepted));
            alignas(32) double e0[4];
            alignas(32) double e1[4];
            alignas(32) double e2[4];
            _mm256_store_pd(e0, d0);
            _mm256_store_pd(e1, d1);
            _mm256_store_pd(e2, d2);
            d[0] = e0[first];
            d[1] = e1[first];
            d[2] = e2[first];
            return i + static_cast<std::size_t>(first);
        }
    }
    return count;
}
