#pragma once

// Minimal property-testing support: a fixed-seed generator so that every run
// explores the same inputs (deterministic, reproducible failures).

#include <cstdint>
#include <random>

namespace katana::test {

class Random {
  public:
    explicit Random(std::uint64_t seed = 0x4B4154414E41ULL) : engine_(seed) {}

    [[nodiscard]] double real(double lo, double hi)
    {
        return std::uniform_real_distribution<double>(lo, hi)(engine_);
    }

    [[nodiscard]] int integer(int lo, int hi)
    {
        return std::uniform_int_distribution<int>(lo, hi)(engine_);
    }

  private:
    std::mt19937_64 engine_;
};

inline constexpr int kPropertyIterations = 500;

} // namespace katana::test
