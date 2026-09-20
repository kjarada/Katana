#pragma once

// Data-parallel task pool (PLAN.MD Phase 19).
//
// The one primitive everything here needs is "run this index range across the
// cores"; futures and task graphs are not, so they are not built. The pool is
// std::jthread based, created once and kept warm, because the workloads it
// serves (rasterising a frame, transforming a vertex buffer) are measured in
// hundreds of microseconds and cannot afford thread creation.
//
// DETERMINISM (Rule 7). Parallelism here must never change a result. The
// contract is that `body` is called exactly once for every index in the range
// and that concurrently-running calls touch disjoint state; the ORDER in which
// indices are visited is unspecified and must not matter. Every caller in this
// repository satisfies that by partitioning its output - one screen tile, one
// vertex, one triangle bin per index - so the bytes produced are identical to
// the serial run, and `TaskPool(1)` is used in tests to prove it.
//
// Nesting is safe: a parallelRanges() issued while the pool is already busy
// runs serially on the calling thread rather than waiting for workers that may
// themselves be waiting. It degrades throughput, never correctness, and it
// cannot deadlock.

#include <atomic>
#include <condition_variable>
#include <cstddef>
#include <exception>
#include <functional>
#include <mutex>
#include <thread>
#include <vector>

namespace katana::core {

class TaskPool {
  public:
    // hardware_concurrency() - 1 worker threads (the caller is the last worker),
    // clamped to at least 0 so a single-core machine runs everything inline.
    [[nodiscard]] static std::size_t defaultWorkerThreads();

    // Process-wide pool. Constructed on first use and never destroyed, so a
    // static destructor running on another thread cannot join it mid-frame.
    [[nodiscard]] static TaskPool& shared();

    explicit TaskPool(std::size_t workerThreads = defaultWorkerThreads());
    ~TaskPool();

    TaskPool(const TaskPool&) = delete;
    TaskPool& operator=(const TaskPool&) = delete;

    // Threads available to a parallelRanges(), including the calling thread.
    // Always >= 1.
    [[nodiscard]] std::size_t concurrency() const noexcept { return threads_.size() + 1; }

    // Calls body(lo, hi) over half-open sub-ranges covering [begin, end).
    // Sub-ranges hold at least `grain` indices except possibly the last.
    // Blocks until every index has been visited.
    //
    // An exception escaping `body` is caught, the remaining chunks are skipped,
    // and the first one captured is rethrown to the caller once the workers have
    // stopped - so an exception never crosses a thread boundary unhandled and
    // never leaves a worker holding the job.
    void parallelRanges(std::size_t begin, std::size_t end, std::size_t grain,
                        const std::function<void(std::size_t, std::size_t)>& body);

    // One call per index. Convenience over parallelRanges with grain 1; prefer
    // parallelRanges when the per-index work is small, so the dispatch cost is
    // amortised.
    void parallelFor(std::size_t begin, std::size_t end,
                     const std::function<void(std::size_t)>& body);

  private:
    struct Job {
        const std::function<void(std::size_t, std::size_t)>* body = nullptr;
        std::size_t end = 0;
        std::size_t grain = 1;
        std::atomic<std::size_t> cursor{0};
        std::atomic<std::size_t> remaining{0}; // pool workers still to report in
        std::atomic<bool> failed{false};       // set once an exception was caught
        std::mutex errorMutex;
        std::exception_ptr error;
    };

    void runChunks(Job& job);
    void workerLoop(const std::stop_token& stop);

    std::mutex mutex_;
    std::condition_variable wake_;
    std::condition_variable done_;
    Job* job_ = nullptr;      // at most one at a time; see the nesting note above
    std::size_t generation_ = 0; // bumped per job so workers do not re-run one
    bool running_ = true;

    // Declared last: the threads run workerLoop, which touches every member
    // above, so those must be alive before the first thread starts and must
    // outlive the join that ~jthread performs.
    std::vector<std::jthread> threads_;
};

} // namespace katana::core
