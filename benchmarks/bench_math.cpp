#include <benchmark/benchmark.h>

#include <vector>

#include "katana/math/mat4.hpp"
#include "katana/math/quaternion.hpp"
#include "katana/math/vec3.hpp"

using namespace katana::math;

namespace {

std::vector<Vec3> makePoints(std::size_t count)
{
    std::vector<Vec3> points;
    points.reserve(count);
    for (std::size_t i = 0; i < count; ++i) {
        const double t = static_cast<double>(i);
        points.emplace_back(500000.0 + t * 0.25, 5000000.0 + t * 0.5, 100.0 + t * 0.001);
    }
    return points;
}

void BM_TransformPoints(benchmark::State& state)
{
    const auto points = makePoints(static_cast<std::size_t>(state.range(0)));
    const Mat4 m = Mat4::translation(Vec3(10.0, 20.0, 30.0)) *
                   Mat4::rotation(Vec3(0.0, 0.0, 1.0), 0.3);
    for (auto _ : state) {
        Vec3 sum;
        for (const Vec3& p : points) {
            sum += transformPoint(m, p);
        }
        benchmark::DoNotOptimize(sum);
    }
    state.SetItemsProcessed(state.iterations() * state.range(0));
}
BENCHMARK(BM_TransformPoints)->Arg(1 << 10)->Arg(1 << 16);

void BM_Mat4Inverse(benchmark::State& state)
{
    const Mat4 m = Mat4::translation(Vec3(1.0, 2.0, 3.0)) *
                   Mat4::rotation(Vec3(1.0, 1.0, 0.0), 0.7) * Mat4::scaling(Vec3(2.0, 3.0, 4.0));
    for (auto _ : state) {
        benchmark::DoNotOptimize(m.inverse());
    }
}
BENCHMARK(BM_Mat4Inverse);

void BM_QuaternionRotate(benchmark::State& state)
{
    const auto points = makePoints(1 << 12);
    const Quaternion q = Quaternion::fromAxisAngle(Vec3(0.0, 0.0, 1.0), 0.3);
    for (auto _ : state) {
        Vec3 sum;
        for (const Vec3& p : points) {
            sum += q.rotate(p);
        }
        benchmark::DoNotOptimize(sum);
    }
    state.SetItemsProcessed(state.iterations() * (1 << 12));
}
BENCHMARK(BM_QuaternionRotate);

} // namespace
