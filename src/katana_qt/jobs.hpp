#pragma once

// Background jobs for the desktop application.
//
// WHY. Building a surface from 400k points takes 1.7 s and comparing two
// surfaces 2 s (docs/terrain.md, measured). Run on the GUI thread behind a
// wait cursor, that is a window that stops repainting, stops answering the
// mouse and, after five seconds, is offered to the user as "not responding".
// A job runs the computation on its own thread instead, shows progress in the
// status bar, and can be cancelled.
//
// THE RULE THAT MAKES IT SAFE. The Document is single-threaded
// (docs/architecture.md, Threading), and so is every widget. A job therefore
// never touches either: its work function computes a PURE result from inputs
// the job owns - copied on the GUI thread before it starts - and returns an
// `Apply` step. The runner calls that step on the GUI thread, where it changes
// the document as ONE command or hands the result to the window. Nothing a job
// computes is visible until it is applied, so a cancelled or failed job leaves
// no trace.
//
// NO MOC (the project builds Qt without it). Completion reaches the GUI thread
// through QMetaObject::invokeMethod with a lambda, on a context object the
// runner owns. Progress is not posted at all: the worker stores it in atomics
// and a status-bar timer reads them ten times a second, so a job that reports
// progress a million times costs the event loop nothing.
//
// FAILURE IS REPORTED, NEVER LOST. A work function fails by returning an
// Error, or by throwing; both arrive at `Finished` as JobOutcome::Failed with
// the message. An exception thrown by the Apply step is caught and reported
// the same way.
//
// CANCELLATION is cooperative. cancel() requests a stop through the job's
// std::stop_token; a work function that checks JobControl::stopRequested()
// ends early, and one that cannot be interrupted (a CGAL triangulation) runs
// to the end and is then DISCARDED - a cancelled job never applies, whenever
// the request arrived.

#include <QPointer>
#include <QString>
#include <QWidget>

#include <atomic>
#include <cstddef>
#include <cstdint>
#include <functional>
#include <memory>
#include <mutex>
#include <stop_token>
#include <string>
#include <vector>

#include "katana/core/error.hpp"

class QEventLoop;
class QMainWindow;
class QObject;
class QTimer;

namespace katana::qt {

using JobId = std::uint64_t;
inline constexpr JobId kNoJob = 0;

// What a work function sees: its stop request and a place to report progress.
// Every member is safe to call from the worker thread.
class JobControl {
  public:
    struct Shared; // the state the runner and the worker share (jobs.cpp)

    JobControl(std::stop_token stop, Shared& shared) : stop_(std::move(stop)), shared_(shared) {}

    [[nodiscard]] bool stopRequested() const noexcept { return stop_.stop_requested(); }
    [[nodiscard]] const std::stop_token& stopToken() const noexcept { return stop_; }

    // Fraction done in [0, 1]; values outside are clamped and NaN is ignored.
    // A job that never calls this is shown as busy with no measure, which is
    // the honest display for a triangulation that cannot say how far it is.
    void setProgress(double fraction) noexcept;
    // A short phrase for what the job is doing now ("Triangulating").
    void setStage(std::string stage);

  private:
    std::stop_token stop_;
    Shared& shared_;
};

enum class JobOutcome { Finished, Cancelled, Failed };

// Delivered to a job's Finished callback on the GUI thread, once per job.
struct JobReport {
    JobId id = kNoJob;
    QString title;
    JobOutcome outcome = JobOutcome::Finished;
    // Failed: what the work function returned or threw, or what Apply threw.
    std::string error;
    // Wall time of the work function on its thread, and of the Apply step on
    // the GUI thread. The second is the part of the job the window still waits
    // for, which is why it is measured separately.
    double workSeconds = 0.0;
    double applySeconds = 0.0;
};

// What the status bar shows for one running job.
struct JobProgress {
    JobId id = kNoJob;
    QString title;
    double fraction = -1.0; // < 0: busy, no measure
    std::string stage;
};

class JobRunner {
  public:
    using Apply = std::function<void()>;
    using Work = std::function<katana::core::Result<Apply>(JobControl&)>;
    using Finished = std::function<void(const JobReport&)>;

    JobRunner();
    // Requests a stop of every job and joins its thread. Completions not yet
    // delivered are dropped with the context object they were posted to, so
    // no Apply or Finished runs after this - which is what makes it safe for
    // an Apply to capture the window that owns the runner.
    ~JobRunner();

    JobRunner(const JobRunner&) = delete;
    JobRunner& operator=(const JobRunner&) = delete;

    // The runner of a main window, created on first use as a child of it with
    // its progress widget in the window's status bar. Found again by object
    // name, so the window needs no member for it.
    [[nodiscard]] static JobRunner& of(QMainWindow& window);

    // Starts `work` on a new thread. `finished`, when given, is called on the
    // GUI thread after Apply has run (or instead of it, when the job was
    // cancelled or failed). Must be called on the GUI thread.
    JobId start(QString title, Work work, Finished finished = {});

    // Requests a stop. False when the job is not running (it already ended).
    bool cancel(JobId id);
    void cancelAll();

    [[nodiscard]] std::size_t activeCount() const { return jobs_.size(); }
    [[nodiscard]] bool isActive(JobId id) const;
    [[nodiscard]] std::vector<JobProgress> progress() const;

    // Runs the GUI event loop until the job has ended and its Finished has
    // been called. For a headless session, which has nobody to wait for a
    // result and must not screenshot before it lands, and for tests. An
    // interactive caller never waits: that would be the freeze jobs exist to
    // remove.
    void waitFor(JobId id);
    void waitForAll();

    // The longest the GUI event loop went without running while jobs were
    // active, in seconds: the longest gap between ticks of a 10 ms timer the
    // runner keeps only while it has jobs, or the longest Apply step if that
    // was longer. It is the freeze a user would feel, and it is what proves
    // the work is off the GUI thread; reset to measure afresh.
    [[nodiscard]] double longestEventLoopPause() const { return longestPause_; }
    void resetEventLoopPause() { longestPause_ = 0.0; }

    // The status-bar widget: a label, a progress bar and a Cancel button,
    // hidden while nothing runs. Owned by its Qt parent once given one.
    [[nodiscard]] QWidget* createStatusWidget(QWidget* parent);

  private:
    struct Job;

    void complete(JobId id);
    void waitUntil(const std::function<bool()>& done);
    void tick();
    void syncStatusWidget();
    void startTicking();

    // Declared first so that it is destroyed LAST: the jobs' threads post to
    // it, so they must all have been joined (by ~Job) before it goes, and its
    // destruction is what discards anything they posted.
    std::unique_ptr<QObject> context_;
    std::vector<std::unique_ptr<Job>> jobs_;
    JobId nextId_ = 1;
    double longestPause_ = 0.0;
    std::int64_t lastTickNs_ = 0;
    QTimer* timer_ = nullptr; // a child of context_
    std::vector<QEventLoop*> waiting_; // loops of waitFor/waitForAll, innermost last
    // Owned by its Qt parent, which may be destroyed before the runner (a
    // window deletes its status bar before later children), hence QPointer.
    QPointer<QWidget> status_;
};

} // namespace katana::qt
