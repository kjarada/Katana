// The SIMD kernels, each run at every level: BM_x/scalar and BM_x/avx2.
//
// Read a pair as "the same work, the same result, two instruction sets" - the
// tests prove the results equal bit for bit, so the pair differs only in time.
// The avx2 member is skipped with an error on a processor without AVX2 rather
// than silently timing the scalar path under the wrong name.
//
// The file also builds against a tree WITHOUT the SIMD layer (no
// katana/geometry/point_batch.hpp): both members of each pair then time the
// code as it was before the layer, point by point, so one binary of each tree
// gives a before-and-after comparison under tools/compare_benchmarks.py
// --alternate with identical benchmark names.
//
// The inputs are generated, deterministically, in the shape of the real data:
// an archive is keywords and numbers with a rare degree sign, a drawing's
// strings are short names, a transformed or bounded array is survey
// coordinates at MGA magnitudes.

#include <benchmark/benchmark.h>

#include <cstddef>
#include <cstdint>
#include <string>
#include <vector>

#include "katana/core/text_encoding.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/math/mat3.hpp"
#include "katana/math/mat4.hpp"
#include "katana/math/primitives.hpp"

#if __has_include("katana/geometry/point_batch.hpp")
#define KATANA_BENCH_SIMD_LAYER 1
#include "katana/core/cpu_features.hpp"
#include "katana/geometry/point_batch.hpp"
#endif

namespace {

using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::math::AABB;
using katana::math::Mat3;
using katana::math::Mat4;
using katana::math::Vec3;

// Puts the process at `level` for one benchmark and back afterwards. False
// (and the benchmark marked as skipped) when the processor cannot run it.
class LevelScope {
  public:
    LevelScope(benchmark::State& state, int level)
    {
#if defined(KATANA_BENCH_SIMD_LAYER)
        const auto previous = katana::core::setSimdLevel(static_cast<katana::core::SimdLevel>(level));
        if (!previous) {
            state.SkipWithError(previous.error().message.c_str());
            ok_ = false;
            return;
        }
        previous_ = static_cast<int>(*previous);
#else
        (void)state;
        (void)level;
#endif
    }
    ~LevelScope()
    {
#if defined(KATANA_BENCH_SIMD_LAYER)
        if (ok_) {
            (void)katana::core::setSimdLevel(static_cast<katana::core::SimdLevel>(previous_));
        }
#endif
    }
    LevelScope(const LevelScope&) = delete;
    LevelScope& operator=(const LevelScope&) = delete;
    [[nodiscard]] bool ok() const { return ok_; }

  private:
    bool ok_ = true;
    int previous_ = 0;
};

// Lines in the shape of an archive's: a keyword, a quoted name, numbers. Every
// `nonAsciiEvery`-th line carries a degree sign, which a bearing or a
// description does now and then.
std::string archiveText(std::size_t lines, std::size_t nonAsciiEvery)
{
    std::string text;
    text.reserve(lines * 64);
    std::uint32_t state = 12345u;
    for (std::size_t line = 0; line < lines; ++line) {
        state = state * 1664525u + 1013904223u;
        text += "    point \"P";
        text += std::to_string(state % 100000u);
        text += "\" x 3";
        text += std::to_string(10000u + state % 89999u);
        text += ".123 y 62";
        text += std::to_string(10000u + (state >> 8) % 89999u);
        text += ".456 z ";
        text += std::to_string(state % 97u);
        text += ".78";
        if (nonAsciiEvery != 0 && line % nonAsciiEvery == 0) {
            text += " bearing 12\xC2\xB0"; // U+00B0 DEGREE SIGN
        }
        text += "\r\n";
    }
    return text;
}

std::string utf16le(const std::string& utf8)
{
    auto encoded = katana::core::encodeUtf16LittleEndian(utf8);
    return encoded ? std::move(*encoded) : std::string();
}

void BM_DecodeUtf16Archive(benchmark::State& state, int level)
{
    LevelScope scope(state, level);
    if (!scope.ok()) {
        return;
    }
    // About 8 MB of UTF-16: a 58 MB archive is seven of these.
    static const std::string bytes = utf16le(archiveText(64'000, 40));
    for (auto _ : state) {
        auto decoded = katana::core::decodeText(bytes);
        benchmark::DoNotOptimize(decoded);
    }
    state.SetBytesProcessed(state.iterations() * static_cast<std::int64_t>(bytes.size()));
}
BENCHMARK_CAPTURE(BM_DecodeUtf16Archive, scalar, 0)->Unit(benchmark::kMillisecond);
BENCHMARK_CAPTURE(BM_DecodeUtf16Archive, avx2, 1)->Unit(benchmark::kMillisecond);

// The adversarial shape for an ASCII fast path: a non-ASCII character every
// few units, so runs are short and the fast path is entered and left
// constantly. Measured so that a regression there is seen, not assumed away.
void BM_DecodeUtf16Mixed(benchmark::State& state, int level)
{
    LevelScope scope(state, level);
    if (!scope.ok()) {
        return;
    }
    static const std::string bytes = [] {
        std::string text;
        for (int i = 0; i < 400'000; ++i) {
            text += "ab\xC3\xA9 "; // "abé "
        }
        return utf16le(text);
    }();
    for (auto _ : state) {
        auto decoded = katana::core::decodeText(bytes);
        benchmark::DoNotOptimize(decoded);
    }
    state.SetBytesProcessed(state.iterations() * static_cast<std::int64_t>(bytes.size()));
}
BENCHMARK_CAPTURE(BM_DecodeUtf16Mixed, scalar, 0)->Unit(benchmark::kMillisecond);
BENCHMARK_CAPTURE(BM_DecodeUtf16Mixed, avx2, 1)->Unit(benchmark::kMillisecond);

void BM_IsValidUtf8Archive(benchmark::State& state, int level)
{
    LevelScope scope(state, level);
    if (!scope.ok()) {
        return;
    }
    static const std::string text = archiveText(64'000, 40);
    for (auto _ : state) {
        benchmark::DoNotOptimize(katana::core::isValidUtf8(text));
    }
    state.SetBytesProcessed(state.iterations() * static_cast<std::int64_t>(text.size()));
}
BENCHMARK_CAPTURE(BM_IsValidUtf8Archive, scalar, 0)->Unit(benchmark::kMillisecond);
BENCHMARK_CAPTURE(BM_IsValidUtf8Archive, avx2, 1)->Unit(benchmark::kMillisecond);

// What the entity model checks one at a time: names and short texts. The fast
// path must cost these nothing.
void BM_IsValidUtf8Names(benchmark::State& state, int level)
{
    LevelScope scope(state, level);
    if (!scope.ok()) {
        return;
    }
    static const std::vector<std::string> names = [] {
        std::vector<std::string> out;
        for (int i = 0; i < 10'000; ++i) {
            out.push_back("TREE_" + std::to_string(i * 7919 % 100000));
        }
        return out;
    }();
    for (auto _ : state) {
        std::size_t valid = 0;
        for (const std::string& name : names) {
            valid += katana::core::isValidUtf8(name) ? 1 : 0;
        }
        benchmark::DoNotOptimize(valid);
    }
    state.SetItemsProcessed(state.iterations() * static_cast<std::int64_t>(names.size()));
}
BENCHMARK_CAPTURE(BM_IsValidUtf8Names, scalar, 0)->Unit(benchmark::kMicrosecond);
BENCHMARK_CAPTURE(BM_IsValidUtf8Names, avx2, 1)->Unit(benchmark::kMicrosecond);

constexpr std::size_t kPoints = 1 << 16;

std::vector<Vec3> points3()
{
    std::vector<Vec3> points;
    points.reserve(kPoints);
    for (std::size_t i = 0; i < kPoints; ++i) {
        const double t = static_cast<double>(i);
        points.emplace_back(300000.0 + t * 0.25, 6250000.0 + t * 0.5, 30.0 + t * 0.001);
    }
    return points;
}

std::vector<Point2> points2()
{
    std::vector<Point2> points;
    points.reserve(kPoints);
    for (std::size_t i = 0; i < kPoints; ++i) {
        const double t = static_cast<double>(i);
        points.emplace_back(300000.0 + t * 0.25, 6250000.0 - t * 0.5);
    }
    return points;
}

void BM_TransformPoints3Batch(benchmark::State& state, int level)
{
    LevelScope scope(state, level);
    if (!scope.ok()) {
        return;
    }
    // In place, as the batch call works: a small rotation about a point in the
    // cloud keeps the coordinates at survey magnitudes however many times the
    // loop runs.
    auto points = points3();
    const Vec3 centre(308192.0, 6266384.0, 62.0);
    const Mat4 m = Mat4::translation(centre) * Mat4::rotation(Vec3(0.0, 0.0, 1.0), 1e-6) *
                   Mat4::translation(Vec3(-centre.x, -centre.y, -centre.z));
    for (auto _ : state) {
#if defined(KATANA_BENCH_SIMD_LAYER)
        katana::geometry::transformPoints(m, points);
#else
        for (Vec3& p : points) {
            p = katana::math::transformPoint(m, p);
        }
#endif
        benchmark::DoNotOptimize(points.data());
        benchmark::ClobberMemory();
    }
    state.SetItemsProcessed(state.iterations() * static_cast<std::int64_t>(points.size()));
}
BENCHMARK_CAPTURE(BM_TransformPoints3Batch, scalar, 0);
BENCHMARK_CAPTURE(BM_TransformPoints3Batch, avx2, 1);

void BM_TransformPoints2Batch(benchmark::State& state, int level)
{
    LevelScope scope(state, level);
    if (!scope.ok()) {
        return;
    }
    auto points = points2();
    const Point2 centre(308192.0, 6233616.0);
    const Mat3 m = Mat3::translation(centre) * Mat3::rotation(1e-6) * Mat3::translation(centre * -1.0);
    for (auto _ : state) {
#if defined(KATANA_BENCH_SIMD_LAYER)
        katana::geometry::transformPoints(m, points);
#else
        for (Point2& p : points) {
            p = katana::math::transformPoint(m, p);
        }
#endif
        benchmark::DoNotOptimize(points.data());
        benchmark::ClobberMemory();
    }
    state.SetItemsProcessed(state.iterations() * static_cast<std::int64_t>(points.size()));
}
BENCHMARK_CAPTURE(BM_TransformPoints2Batch, scalar, 0);
BENCHMARK_CAPTURE(BM_TransformPoints2Batch, avx2, 1);

void BM_BoundsOfPoints2(benchmark::State& state, int level)
{
    LevelScope scope(state, level);
    if (!scope.ok()) {
        return;
    }
    const auto points = points2();
    for (auto _ : state) {
#if defined(KATANA_BENCH_SIMD_LAYER)
        const Box2 box = katana::geometry::boundsOf(points);
#else
        Box2 box;
        for (const Point2& p : points) {
            box.expand(p);
        }
#endif
        benchmark::DoNotOptimize(box);
    }
    state.SetItemsProcessed(state.iterations() * static_cast<std::int64_t>(points.size()));
}
BENCHMARK_CAPTURE(BM_BoundsOfPoints2, scalar, 0);
BENCHMARK_CAPTURE(BM_BoundsOfPoints2, avx2, 1);

void BM_BoundsOfPoints3(benchmark::State& state, int level)
{
    LevelScope scope(state, level);
    if (!scope.ok()) {
        return;
    }
    const auto points = points3();
    for (auto _ : state) {
#if defined(KATANA_BENCH_SIMD_LAYER)
        const AABB box = katana::geometry::boundsOf(points);
#else
        AABB box;
        for (const Vec3& p : points) {
            box.expand(p);
        }
#endif
        benchmark::DoNotOptimize(box);
    }
    state.SetItemsProcessed(state.iterations() * static_cast<std::int64_t>(points.size()));
}
BENCHMARK_CAPTURE(BM_BoundsOfPoints3, scalar, 0);
BENCHMARK_CAPTURE(BM_BoundsOfPoints3, avx2, 1);

// A drawing's strings: 28k polylines of about six vertices each, the shape
// of the largest real survey drawing. Polyline2::boundingBox goes through
// boundsOf, and short arrays must not pay for a dispatch they cannot win.
void BM_PolylineBoundingBoxes(benchmark::State& state, int level)
{
    LevelScope scope(state, level);
    if (!scope.ok()) {
        return;
    }
    static const std::vector<katana::geometry::Polyline2> lines = [] {
        std::vector<katana::geometry::Polyline2> out;
        std::uint32_t seed = 7u;
        for (int i = 0; i < 28'000; ++i) {
            katana::geometry::Polyline2 line;
            seed = seed * 1664525u + 1013904223u;
            const int count = 2 + static_cast<int>(seed % 9u);
            for (int k = 0; k < count; ++k) {
                line.vertices.emplace_back(300000.0 + i * 3.0 + k, 6250000.0 + k * 0.5);
            }
            out.push_back(std::move(line));
        }
        return out;
    }();
    for (auto _ : state) {
        double width = 0.0;
        for (const auto& line : lines) {
            width += line.boundingBox().width();
        }
        benchmark::DoNotOptimize(width);
    }
    state.SetItemsProcessed(state.iterations() * static_cast<std::int64_t>(lines.size()));
}
BENCHMARK_CAPTURE(BM_PolylineBoundingBoxes, scalar, 0)->Unit(benchmark::kMicrosecond);
BENCHMARK_CAPTURE(BM_PolylineBoundingBoxes, avx2, 1)->Unit(benchmark::kMicrosecond);

} // namespace
