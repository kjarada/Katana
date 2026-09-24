// Throughput: the owner's first requirement. A synthetic job of 1 000 setups,
// each shooting 17 targets on both faces (HA, ZA and SD per pointing):
// 102 000 raw observations plus the backsights. The test checks every target
// came out and records the time; the bound is loose because a Debug build
// with checks on is several times slower than the release the numbers in the
// commit message were measured with.

#include <gtest/gtest.h>

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <string>

#include "katana/survey/reduction.hpp"
#include "test_reduction_support.hpp"

using namespace katana::survey;
using namespace reduction_test;

namespace {

SurveyProject largeJob(std::size_t setups, std::size_t targets, bool foresights = false)
{
    SurveyProject project;
    project.points.reserve(setups + 1);
    project.points.push_back(point("S0", 0.0, -100.0, 10.0));
    project.unpositionedPoints.reserve(setups * targets);
    project.stations.reserve(setups);
    for (std::size_t s = 1; s <= setups; ++s) {
        const std::string station = "S" + std::to_string(s);
        const std::string back = "S" + std::to_string(s - 1);
        // Stations along a line east, 100 m apart; the backsight is due west.
        project.points.push_back(point(station, 0.0, 100.0 * (static_cast<double>(s) - 1.0), 10.0));
        std::vector<Shot> shots;
        shots.reserve(2 * targets + 2);
        std::size_t index = 1;
        shots.push_back(Shot{back, index++, Face::Left, deg(270), deg(90), 100.0, 1.5});
        shots.push_back(Shot{back, index++, Face::Right, deg(90), deg(90), 100.0, 1.5});
        if (foresights) {
            // The next station, due east: what makes the stations a network
            // (an angle back -> forward and two distances at each).
            const std::string forward = "S" + std::to_string(s + 1);
            shots.push_back(Shot{forward, index++, Face::Left, deg(90), deg(90), 100.0, 1.5});
            shots.push_back(Shot{forward, index++, Face::Right, deg(270), deg(90), 100.0, 1.5});
        }
        for (std::size_t t = 0; t < targets; ++t) {
            const std::string target = "P" + std::to_string(s) + "_" + std::to_string(t);
            project.unpositionedPoints.push_back(unpositioned(target));
            const double direction = deg(10.0 + 20.0 * static_cast<double>(t));
            const double zenith = deg(88.0 + 0.2 * static_cast<double>(t));
            const double distance = 5.0 + 2.5 * static_cast<double>(t);
            shots.push_back(Shot{target, index++, Face::Left, direction, zenith, distance, 1.8});
            shots.push_back(Shot{target, index++, Face::Right,
                                 katana::math::normalizeAngle(direction + deg(180, 0, 2)),
                                 zenith + arcSeconds(4.0), distance + 0.001, 1.8});
        }
        project.stations.push_back(setup("setup" + std::to_string(s), station, 1.55, back, shots));
    }
    if (foresights) {
        project.points.push_back(
            point("S" + std::to_string(setups + 1), 0.0, 100.0 * static_cast<double>(setups), 10.0));
    }
    return project;
}

} // namespace

TEST(ReductionPerformance, AHundredThousandObservationsReduceAndEveryTargetIsComputed)
{
    const SurveyProject project = largeJob(1000, 17);
    std::size_t observations = 0;
    for (const SurveyStation& station : project.stations) {
        observations += station.observations.size();
    }
    ASSERT_GE(observations, 100000U);

    const auto start = std::chrono::steady_clock::now();
    const auto outcome = reduceAndAdjust(project, ReductionSettings{}, {});
    const auto reduced = std::chrono::steady_clock::now();
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    const std::string text = renderText(outcome->report);
    const auto rendered = std::chrono::steady_clock::now();

    const double reduceSeconds = std::chrono::duration<double>(reduced - start).count();
    const double renderSeconds = std::chrono::duration<double>(rendered - reduced).count();
    std::printf("[ reduction ] %zu observations reduced in %.3f s (%.0f per second); report text "
                "%zu bytes in %.3f s\n",
                observations, reduceSeconds, static_cast<double>(observations) / reduceSeconds,
                text.size(), renderSeconds);
    RecordProperty("observations", std::to_string(observations));
    RecordProperty("reduce_seconds", std::to_string(reduceSeconds));

    // Every target, plus the stations used as known: 17 000 radiated points.
    std::size_t radiated = 0;
    for (const ComputedPoint& point : outcome->points) {
        radiated += point.method == ComputationMethod::Radiation ? 1 : 0;
    }
    EXPECT_EQ(radiated, 17000U);
    EXPECT_EQ(outcome->report.facePairs.size(), 1000U * 18U);
    EXPECT_LT(reduceSeconds, 30.0);
}

TEST(ReductionPerformance, ANetworkOfSetupsWithSideShotsAdjustsOnlyTheStations)
{
    // Stations S0 .. S(n+1) on a line, S0 and S1 held; every setup sees the
    // station behind and the one ahead on both faces and 17 side shots. The
    // least squares takes the n stations only - the side shots are radiated
    // after - so its size is 2n unknowns whatever the number of shots.
    // KATANA_REDUCTION_NETWORK_SETUPS sets n for a measurement (default 200:
    // quick in a Debug build).
    std::size_t setups = 200;
    if (const char* text = std::getenv("KATANA_REDUCTION_NETWORK_SETUPS")) {
        setups = static_cast<std::size_t>(std::strtoul(text, nullptr, 10));
    }
    const SurveyProject project = largeJob(setups, 17, true);
    std::size_t observations = 0;
    for (const SurveyStation& station : project.stations) {
        observations += station.observations.size();
    }
    ReductionSettings settings;
    settings.method = AdjustmentMethod::Network;
    settings.control = {ControlSelection{ControlPoint::fixedHorizontal("S0")},
                        ControlSelection{ControlPoint::fixedHorizontal("S1")}};

    const auto start = std::chrono::steady_clock::now();
    const auto outcome = reduceAndAdjust(project, settings, {});
    const double seconds =
        std::chrono::duration<double>(std::chrono::steady_clock::now() - start).count();
    ASSERT_TRUE(outcome.ok()) << outcome.error().describe();
    ASSERT_EQ(outcome->report.adjustments.size(), 1U);
    const AdjustmentReport& adjustment = outcome->report.adjustments[0];
    std::printf("[ reduction ] network: %zu raw observations, %zu adjusted in %zu unknowns, "
                "reduced and adjusted in %.3f s\n",
                observations, adjustment.observations, adjustment.unknowns, seconds);
    RecordProperty("network_seconds", std::to_string(seconds));
    // S2 .. S(n+1) free: 2n unknowns. Each setup gives two distances and one
    // angle (the backsight is the reference direction).
    EXPECT_EQ(adjustment.unknowns, 2 * setups);
    EXPECT_EQ(adjustment.observations, 3 * setups);
    // The side shots came out, radiated from the adjusted stations.
    std::size_t radiated = 0;
    for (const ComputedPoint& point : outcome->points) {
        radiated += point.method == ComputationMethod::Radiation ? 1 : 0;
    }
    EXPECT_EQ(radiated, 17 * setups);
}
