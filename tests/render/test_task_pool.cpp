// Task pool (PLAN.MD Phase 19).
//
// The pool exists to make rendering parallel WITHOUT making it
// non-reproducible, so the properties asserted here are coverage (every index
// exactly once), safety (no deadlock when nested, no exception crossing a
// thread) and equivalence with the serial run.

#include <gtest/gtest.h>

#include <atomic>
#include <cmath>
#include <numeric>
#include <stdexcept>
#include <vector>

#include "katana/core/task_pool.hpp"

using katana::core::TaskPool;

TEST(TaskPool, VisitsEveryIndexExactlyOnce)
{
    for (std::size_t workers : {std::size_t{0}, std::size_t{1}, std::size_t{5}}) {
        TaskPool pool(workers);
        constexpr std::size_t kCount = 10'000;
        std::vector<int> visits(kCount, 0);

        pool.parallelFor(0, kCount, [&](std::size_t i) { ++visits[i]; });

        for (std::size_t i = 0; i < kCount; ++i) {
            ASSERT_EQ(visits[i], 1) << "index " << i << " with " << workers << " workers";
        }
    }
}

TEST(TaskPool, RangesCoverTheWholeIntervalWithoutOverlap)
{
    TaskPool pool(4);
    constexpr std::size_t kCount = 5000;
    std::vector<int> visits(kCount, 0);
    std::atomic<std::size_t> calls{0};

    pool.parallelRanges(0, kCount, 64, [&](std::size_t lo, std::size_t hi) {
        ++calls;
        EXPECT_LT(lo, hi);
        EXPECT_LE(hi, kCount);
        for (std::size_t i = lo; i < hi; ++i) {
            ++visits[i];
        }
    });

    EXPECT_EQ(std::accumulate(visits.begin(), visits.end(), 0), static_cast<int>(kCount));
    for (std::size_t i = 0; i < kCount; ++i) {
        ASSERT_EQ(visits[i], 1) << "index " << i;
    }
    EXPECT_GT(calls.load(), 0u);
}

TEST(TaskPool, AnEmptyOrInvertedRangeDoesNothing)
{
    TaskPool pool(2);
    int calls = 0;
    pool.parallelFor(5, 5, [&](std::size_t) { ++calls; });
    pool.parallelFor(9, 3, [&](std::size_t) { ++calls; });
    EXPECT_EQ(calls, 0);
}

TEST(TaskPool, AGrainOfZeroIsTreatedAsOneRatherThanLoopingForever)
{
    TaskPool pool(2);
    std::atomic<int> total{0};
    pool.parallelRanges(0, 100, 0, [&](std::size_t lo, std::size_t hi) {
        total += static_cast<int>(hi - lo);
    });
    EXPECT_EQ(total.load(), 100);
}

TEST(TaskPool, ResultsAreIdenticalWhateverTheWorkerCount)
{
    // The accumulation is into disjoint slots, which is the contract callers
    // must satisfy; under it the answer cannot depend on scheduling.
    constexpr std::size_t kCount = 4096;
    std::vector<double> reference;
    for (std::size_t workers : {std::size_t{0}, std::size_t{1}, std::size_t{3},
                                std::size_t{8}}) {
        TaskPool pool(workers);
        std::vector<double> out(kCount, 0.0);
        pool.parallelRanges(0, kCount, 33, [&](std::size_t lo, std::size_t hi) {
            for (std::size_t i = lo; i < hi; ++i) {
                out[i] = std::sin(static_cast<double>(i) * 0.001) * 1.0e6;
            }
        });
        if (reference.empty()) {
            reference = out;
        } else {
            EXPECT_EQ(out, reference) << workers << " workers";
        }
    }
}

TEST(TaskPool, AnExceptionInABodyReachesTheCallerAndStopsTheRest)
{
    TaskPool pool(4);
    std::atomic<int> ran{0};

    EXPECT_THROW(
        {
            pool.parallelFor(0, 100'000, [&](std::size_t i) {
                ++ran;
                if (i == 10) {
                    throw std::runtime_error("boom");
                }
            });
        },
        std::runtime_error);

    // Not all of them: the failure must stop work being handed out, or a bad
    // frame keeps burning cores.
    EXPECT_LT(ran.load(), 100'000);

    // And the pool is still usable afterwards - a job left half-claimed would
    // hang here instead.
    std::atomic<int> after{0};
    pool.parallelFor(0, 1000, [&](std::size_t) { ++after; });
    EXPECT_EQ(after.load(), 1000);
}

TEST(TaskPool, NestedCallsRunInlineInsteadOfDeadlocking)
{
    TaskPool pool(3);
    std::atomic<int> inner{0};

    pool.parallelFor(0, 8, [&](std::size_t) {
        // A worker submitting to its own pool is the classic deadlock. Here it
        // must simply run on this thread.
        pool.parallelFor(0, 10, [&](std::size_t) { ++inner; });
    });

    EXPECT_EQ(inner.load(), 80);
}

TEST(TaskPool, ConcurrencyCountsTheCallingThread)
{
    EXPECT_EQ(TaskPool(0).concurrency(), 1u) << "with no workers the caller still runs the work";
    EXPECT_EQ(TaskPool(3).concurrency(), 4u);
}

TEST(TaskPool, ManyConsecutiveJobsAllComplete)
{
    // Exercises the generation counter: a worker that mistook a new job for one
    // it had already drained would leave later jobs short.
    TaskPool pool(4);
    for (int round = 0; round < 200; ++round) {
        std::vector<int> visits(256, 0);
        pool.parallelFor(0, visits.size(), [&](std::size_t i) { ++visits[i]; });
        for (std::size_t i = 0; i < visits.size(); ++i) {
            ASSERT_EQ(visits[i], 1) << "round " << round << " index " << i;
        }
    }
}
