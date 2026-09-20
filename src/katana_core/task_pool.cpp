#include "katana/core/task_pool.hpp"

#include <algorithm>

namespace katana::core {

std::size_t TaskPool::defaultWorkerThreads()
{
    const unsigned hardware = std::thread::hardware_concurrency();
    // The calling thread is one of the workers, so one fewer is spawned. A
    // reported concurrency of 0 (unknown) is treated as 1: run inline rather
    // than guess, because oversubscribing is worse than not parallelising.
    return hardware > 1 ? static_cast<std::size_t>(hardware - 1) : 0;
}

TaskPool& TaskPool::shared()
{
    // Deliberately leaked. The pool outlives main() rather than being joined
    // from a static destructor, whose ordering against other translation units'
    // teardown is not something a frame in flight can depend on.
    static TaskPool* const pool = new TaskPool();
    return *pool;
}

TaskPool::TaskPool(std::size_t workerThreads)
{
    threads_.reserve(workerThreads);
    for (std::size_t i = 0; i < workerThreads; ++i) {
        threads_.emplace_back([this](std::stop_token stop) { workerLoop(stop); });
    }
}

TaskPool::~TaskPool()
{
    {
        const std::lock_guard<std::mutex> lock(mutex_);
        running_ = false;
    }
    for (auto& thread : threads_) {
        thread.request_stop();
    }
    wake_.notify_all();
    // ~jthread joins. Nothing else to do: a job cannot be in flight, because
    // parallelRanges does not return until every worker has left runChunks.
}

void TaskPool::workerLoop(const std::stop_token& stop)
{
    std::size_t lastSeen = 0;
    while (true) {
        Job* job = nullptr;
        {
            std::unique_lock<std::mutex> lock(mutex_);
            wake_.wait(lock, [&] {
                return !running_ || stop.stop_requested() ||
                       (job_ != nullptr && generation_ != lastSeen);
            });
            if (!running_ || stop.stop_requested()) {
                return;
            }
            lastSeen = generation_;
            job = job_;
        }
        runChunks(*job);
        // Every worker is charged for exactly one decrement per generation by
        // the submitter BEFORE the job is published, so the count cannot reach
        // zero early and let the submitter destroy a Job a worker still holds.
        if (job->remaining.fetch_sub(1, std::memory_order_acq_rel) == 1) {
            const std::lock_guard<std::mutex> lock(mutex_);
            done_.notify_all();
        }
    }
}

void TaskPool::runChunks(Job& job)
{
    while (!job.failed.load(std::memory_order_relaxed)) {
        const std::size_t lo = job.cursor.fetch_add(job.grain, std::memory_order_relaxed);
        if (lo >= job.end) {
            return;
        }
        const std::size_t hi = std::min(lo + job.grain, job.end);
        try {
            (*job.body)(lo, hi);
        } catch (...) {
            // Stop handing out work and keep the first failure. Later ones are
            // dropped: reporting one cause beats picking arbitrarily among
            // several, and the caller only rethrows one anyway.
            const std::lock_guard<std::mutex> lock(job.errorMutex);
            if (!job.error) {
                job.error = std::current_exception();
            }
            job.failed.store(true, std::memory_order_relaxed);
            return;
        }
    }
}

void TaskPool::parallelRanges(std::size_t begin, std::size_t end, std::size_t grain,
                              const std::function<void(std::size_t, std::size_t)>& body)
{
    if (begin >= end) {
        return;
    }
    grain = std::max<std::size_t>(grain, 1);

    Job job;
    job.body = &body;
    job.end = end;
    job.grain = grain;
    job.cursor.store(begin, std::memory_order_relaxed);

    bool shared = false;
    {
        const std::lock_guard<std::mutex> lock(mutex_);
        // One job at a time. A nested call finds the slot taken and runs the
        // whole range inline, which is slower but cannot wait on a worker that
        // is itself blocked inside the outer job.
        if (job_ == nullptr && !threads_.empty() && running_) {
            // Charged up front, not as each worker wakes: between publishing
            // and a worker actually waking, a count built up on the worker side
            // would read zero and let this function return - destroying `job`
            // while a worker is about to dereference it.
            job.remaining.store(threads_.size(), std::memory_order_relaxed);
            job_ = &job;
            ++generation_;
            shared = true;
        }
    }
    if (shared) {
        wake_.notify_all();
    }

    // The submitting thread is a worker too: with no pool threads, or with all
    // of them busy elsewhere, this alone still completes the range.
    runChunks(job);

    if (shared) {
        std::unique_lock<std::mutex> lock(mutex_);
        done_.wait(lock, [&] { return job.remaining.load(std::memory_order_acquire) == 0; });
        job_ = nullptr;
    }

    if (job.error) {
        std::rethrow_exception(job.error);
    }
}

void TaskPool::parallelFor(std::size_t begin, std::size_t end,
                           const std::function<void(std::size_t)>& body)
{
    parallelRanges(begin, end, 1, [&body](std::size_t lo, std::size_t hi) {
        for (std::size_t i = lo; i < hi; ++i) {
            body(i);
        }
    });
}

} // namespace katana::core
