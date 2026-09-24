#pragma once

// Portable SIMD vectors: std::simd where the standard library provides it, and
// std::experimental::simd (the Parallelism TS v2) otherwise.
//
// WHY A FACADE. libstdc++ 16 ships <simd> only in C++26 mode and does not yet
// advertise it (__cpp_lib_simd is undefined; its internal __glibcxx_simd is
// set), while the project can also be built as C++23 (KATANA_CXX_STANDARD).
// The TS version works in both. Code written against the few names below
// builds either way, and the day the standard one is complete the TS branch
// can go.
//
// WHERE IT MAY BE USED. In ordinary source files, which are compiled for the
// baseline: there a Vec<double> is 2 lanes, Vec<float> 4 and Vec<char> 16 (SSE2)
// - portable, needing no dispatch, and safe. NOT in an *_avx2.cpp kernel file:
// the lane count is fixed by the -m flags the file is compiled with, and in an
// unoptimised build both libraries leave helpers out of line - std::span::data,
// integral_constant::operator(), the vector's size() - which in a kernel file
// would be AVX2 code shared with the rest of the program (the inline-copy
// hazard; docs/performance.md). Kernel files use intrinsics, and
// tools/check_simd_kernels.cmake refuses this header in them.
//
// minOf/maxOf are defined here by comparison and select, NOT with the
// libraries' min/max: the TS marks those finite-math-only and no-signed-zeros,
// which leaves the result for a NaN or a pair of zeros to the optimiser, and
// determinism (bit-identical results) needs it pinned. They are std::min and
// std::max lane by lane: (b < a) ? b : a and (a < b) ? b : a.

#include <cstddef>

#if __has_include(<simd>)
#include <simd>
#endif

#if defined(__cpp_lib_simd) || defined(__glibcxx_simd)
#define KATANA_SIMD_STANDARD 1
#else
#include <experimental/simd>
#define KATANA_SIMD_STANDARD 0
#endif

namespace katana::core::simd {

#if KATANA_SIMD_STANDARD

inline constexpr const char* kBackend = "std::simd";

template <typename T> using Vec = std::simd::vec<T>;
template <typename T> using Mask = typename Vec<T>::mask_type;

// Reads lanes<T> elements from `from`, which need not be aligned.
template <typename T> [[nodiscard]] inline Vec<T> load(const T* from)
{
    return std::simd::unchecked_load<Vec<T>>(from, Vec<T>::size());
}

// Writes lanes<T> elements to `to`, which need not be aligned.
template <typename T> inline void store(const Vec<T>& value, T* to)
{
    std::simd::unchecked_store(value, to, Vec<T>::size());
}

// Lane by lane, `whenTrue` where `mask` is set and `whenFalse` elsewhere.
template <typename T>
[[nodiscard]] inline Vec<T> select(const Mask<T>& mask, const Vec<T>& whenTrue, const Vec<T>& whenFalse)
{
    return std::simd::select(mask, whenTrue, whenFalse);
}

#else

inline constexpr const char* kBackend = "std::experimental::simd";

template <typename T> using Vec = std::experimental::native_simd<T>;
template <typename T> using Mask = typename Vec<T>::mask_type;

template <typename T> [[nodiscard]] inline Vec<T> load(const T* from)
{
    Vec<T> value;
    value.copy_from(from, std::experimental::element_aligned);
    return value;
}

template <typename T> inline void store(const Vec<T>& value, T* to)
{
    value.copy_to(to, std::experimental::element_aligned);
}

template <typename T>
[[nodiscard]] inline Vec<T> select(const Mask<T>& mask, const Vec<T>& whenTrue, const Vec<T>& whenFalse)
{
    Vec<T> result = whenFalse;
    std::experimental::where(mask, result) = whenTrue;
    return result;
}

#endif

// Lanes in one Vec<T> as this file is compiled.
template <typename T> inline constexpr std::size_t lanes = static_cast<std::size_t>(Vec<T>::size());

// std::min lane by lane: (b < a) ? b : a. A NaN in b keeps a; of two zeros, a.
template <typename T> [[nodiscard]] inline Vec<T> minOf(const Vec<T>& a, const Vec<T>& b)
{
    return select<T>(b < a, b, a);
}

// std::max lane by lane: (a < b) ? b : a. A NaN in b keeps a; of two zeros, a.
template <typename T> [[nodiscard]] inline Vec<T> maxOf(const Vec<T>& a, const Vec<T>& b)
{
    return select<T>(a < b, b, a);
}

} // namespace katana::core::simd
