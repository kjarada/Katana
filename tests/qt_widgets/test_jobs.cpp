// The background job runner (src/katana_qt/jobs.hpp): work on its own
// thread, Apply and Finished on the GUI thread, progress read while it runs,
// cancel that discards, failures reported rather than lost, and a GUI event
// loop that keeps running while a job computes.
//
// Every wait here is on an event or a flag, never a fixed sleep standing in
// for "long enough": this laptop is shared and a sleep that suffices today is
// a flaky test tomorrow. The two sleeps that remain are the WORK being timed.

#include <gtest/gtest.h>

#include <QApplication>
#include <QLabel>
#include <QProgressBar>
#include <QToolButton>
#include <QWidget>

#include <atomic>
#include <chrono>
#include <memory>
#include <stdexcept>
#include <thread>

#include "jobs.hpp"
#include "katana/terrain/tin_builder.hpp"

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::qt::JobControl;
using katana::qt::JobId;
using katana::qt::JobOutcome;
using katana::qt::JobReport;
using katana::qt::JobRunner;

// Pumps the GUI event loop until `done` holds, for at most `limit`, so a
// broken runner fails the test instead of hanging it.
template <typename Predicate>
bool pumpUntil(Predicate done, std::chrono::milliseconds limit = std::chrono::seconds(30))
{
    const auto deadline = std::chrono::steady_clock::now() + limit;
    while (!done()) {
        if (std::chrono::steady_clock::now() > deadline) {
            return false;
        }
        QCoreApplication::processEvents(QEventLoop::AllEvents, 10);
    }
    return true;
}

TEST(JobRunner, WorkRunsOffTheGuiThreadAndApplyAndFinishedRunOnIt)
{
    JobRunner runner;
    const std::thread::id gui = std::this_thread::get_id();
    std::thread::id worker;
    std::thread::id applier;
    std::thread::id finisher;
    int applied = 0;
    JobReport report;

    const JobId id = runner.start(
        "sum",
        [&](JobControl&) -> katana::core::Result<JobRunner::Apply> {
            worker = std::this_thread::get_id();
            const int value = 6 * 7;
            return JobRunner::Apply([&, value] {
                applier = std::this_thread::get_id();
                applied = value;
            });
        },
        [&](const JobReport& r) {
            finisher = std::this_thread::get_id();
            report = r;
        });
    EXPECT_TRUE(runner.isActive(id));
    runner.waitFor(id);

    EXPECT_FALSE(runner.isActive(id));
    EXPECT_EQ(runner.activeCount(), 0u);
    EXPECT_NE(worker, gui);
    EXPECT_EQ(applier, gui);
    EXPECT_EQ(finisher, gui);
    EXPECT_EQ(applied, 42);
    EXPECT_EQ(report.id, id);
    EXPECT_EQ(report.title, QString("sum"));
    EXPECT_EQ(report.outcome, JobOutcome::Finished);
    EXPECT_TRUE(report.error.empty());
}

TEST(JobRunner, ProgressAndStageReportedByTheWorkerAreReadableWhileItRuns)
{
    JobRunner runner;
    std::atomic<bool> reported{false};
    std::atomic<bool> release{false};
    const JobId id = runner.start("staged", [&](JobControl& control)
                                                -> katana::core::Result<JobRunner::Apply> {
        control.setStage("Triangulating");
        control.setProgress(0.25);
        reported = true;
        while (!release) {
            std::this_thread::yield();
        }
        return JobRunner::Apply{};
    });

    ASSERT_TRUE(pumpUntil([&] { return reported.load(); }));
    const auto items = runner.progress();
    ASSERT_EQ(items.size(), 1u);
    EXPECT_EQ(items[0].id, id);
    EXPECT_EQ(items[0].title, QString("staged"));
    EXPECT_DOUBLE_EQ(items[0].fraction, 0.25); // stored as 250 thousandths
    EXPECT_EQ(items[0].stage, "Triangulating");
    release = true;
    runner.waitFor(id);
}

TEST(JobRunner, AJobThatHasNotReportedProgressIsShownAsBusyAndOutOfRangeProgressIsClamped)
{
    JobRunner runner;
    std::atomic<int> step{0};
    std::atomic<int> go{0};
    const JobId id = runner.start("busy", [&](JobControl& control)
                                              -> katana::core::Result<JobRunner::Apply> {
        step = 1;
        while (go < 1) {
            std::this_thread::yield();
        }
        control.setProgress(7.0); // clamps to 1
        step = 2;
        while (go < 2) {
            std::this_thread::yield();
        }
        return JobRunner::Apply{};
    });
    ASSERT_TRUE(pumpUntil([&] { return step.load() == 1; }));
    EXPECT_LT(runner.progress().at(0).fraction, 0.0);
    go = 1;
    ASSERT_TRUE(pumpUntil([&] { return step.load() == 2; }));
    EXPECT_DOUBLE_EQ(runner.progress().at(0).fraction, 1.0);
    go = 2;
    runner.waitFor(id);
}

TEST(JobRunner, ACancelledJobThatWatchesItsStopTokenEndsEarlyAndAppliesNothing)
{
    JobRunner runner;
    std::atomic<bool> running{false};
    bool applied = false;
    JobReport report;
    const JobId id = runner.start(
        "endless",
        [&](JobControl& control) -> katana::core::Result<JobRunner::Apply> {
            running = true;
            while (!control.stopRequested()) {
                std::this_thread::yield();
            }
            return JobRunner::Apply([&] { applied = true; });
        },
        [&](const JobReport& r) { report = r; });
    ASSERT_TRUE(pumpUntil([&] { return running.load(); }));
    EXPECT_TRUE(runner.cancel(id));
    runner.waitFor(id);
    EXPECT_EQ(report.outcome, JobOutcome::Cancelled);
    EXPECT_FALSE(applied);
    EXPECT_FALSE(runner.cancel(id)) << "an ended job cannot be cancelled again";
}

TEST(JobRunner, AJobThatCannotBeInterruptedIsDiscardedWhenCancelledWhileItRuns)
{
    // A CGAL triangulation cannot look at a stop token. Its result must still
    // not land once the user has said no.
    JobRunner runner;
    std::atomic<bool> running{false};
    std::atomic<bool> release{false};
    bool applied = false;
    JobReport report;
    const JobId id = runner.start(
        "stubborn",
        [&](JobControl&) -> katana::core::Result<JobRunner::Apply> {
            running = true;
            while (!release) {
                std::this_thread::yield();
            }
            return JobRunner::Apply([&] { applied = true; });
        },
        [&](const JobReport& r) { report = r; });
    ASSERT_TRUE(pumpUntil([&] { return running.load(); }));
    runner.cancel(id);
    release = true;
    runner.waitFor(id);
    EXPECT_EQ(report.outcome, JobOutcome::Cancelled);
    EXPECT_FALSE(applied);
}

TEST(JobRunner, AnExceptionThrownByTheWorkIsReportedAsAFailureWithItsMessage)
{
    JobRunner runner;
    JobReport report;
    const JobId id = runner.start(
        "throws",
        [](JobControl&) -> katana::core::Result<JobRunner::Apply> {
            throw std::runtime_error("out of triangles");
        },
        [&](const JobReport& r) { report = r; });
    runner.waitFor(id);
    EXPECT_EQ(report.outcome, JobOutcome::Failed);
    EXPECT_NE(report.error.find("out of triangles"), std::string::npos) << report.error;
}

TEST(JobRunner, AnExceptionOfAnUnknownTypeIsStillReportedAsAFailure)
{
    JobRunner runner;
    JobReport report;
    const JobId id = runner.start(
        "throws an int",
        [](JobControl&) -> katana::core::Result<JobRunner::Apply> { throw 7; },
        [&](const JobReport& r) { report = r; });
    runner.waitFor(id);
    EXPECT_EQ(report.outcome, JobOutcome::Failed);
    EXPECT_FALSE(report.error.empty());
}

TEST(JobRunner, AnErrorReturnedByTheWorkIsReportedWithItsDescription)
{
    JobRunner runner;
    JobReport report;
    const JobId id = runner.start(
        "refuses",
        [](JobControl&) -> katana::core::Result<JobRunner::Apply> {
            return makeError(ErrorCode::TriangulationFailure, "fewer than three points");
        },
        [&](const JobReport& r) { report = r; });
    runner.waitFor(id);
    EXPECT_EQ(report.outcome, JobOutcome::Failed);
    EXPECT_NE(report.error.find("TriangulationFailure"), std::string::npos) << report.error;
    EXPECT_NE(report.error.find("fewer than three points"), std::string::npos) << report.error;
}

TEST(JobRunner, AnExceptionThrownWhileApplyingIsReportedAsAFailure)
{
    JobRunner runner;
    JobReport report;
    const JobId id = runner.start(
        "bad apply",
        [](JobControl&) -> katana::core::Result<JobRunner::Apply> {
            return JobRunner::Apply([] { throw std::logic_error("no such surface"); });
        },
        [&](const JobReport& r) { report = r; });
    runner.waitFor(id);
    EXPECT_EQ(report.outcome, JobOutcome::Failed);
    EXPECT_NE(report.error.find("no such surface"), std::string::npos) << report.error;
}

TEST(JobRunner, DestroyingTheRunnerStopsAndJoinsItsJobsAndCallsNothingAfterwards)
{
    std::atomic<bool> running{false};
    std::atomic<bool> stopped{false};
    bool finished = false;
    bool applied = false;
    {
        JobRunner runner;
        runner.start(
            "abandoned",
            [&](JobControl& control) -> katana::core::Result<JobRunner::Apply> {
                running = true;
                while (!control.stopRequested()) {
                    std::this_thread::yield();
                }
                stopped = true;
                return JobRunner::Apply([&] { applied = true; });
            },
            [&](const JobReport&) { finished = true; });
        ASSERT_TRUE(pumpUntil([&] { return running.load(); }));
    }
    // The destructor joined, so the worker has seen its stop by now.
    EXPECT_TRUE(stopped.load());
    QCoreApplication::processEvents();
    EXPECT_FALSE(finished);
    EXPECT_FALSE(applied);
}

TEST(JobRunner, SeveralJobsRunTogetherAndEachFinishesOnce)
{
    JobRunner runner;
    int finishedCount = 0;
    int sum = 0;
    for (int i = 1; i <= 4; ++i) {
        runner.start(
            QString("job %1").arg(i),
            [i](JobControl&) -> katana::core::Result<JobRunner::Apply> {
                return JobRunner::Apply{};
            },
            [&, i](const JobReport& r) {
                ++finishedCount;
                sum += r.outcome == JobOutcome::Finished ? i : 100;
            });
    }
    EXPECT_EQ(runner.activeCount(), 4u);
    runner.waitForAll();
    EXPECT_EQ(finishedCount, 4);
    EXPECT_EQ(sum, 1 + 2 + 3 + 4);
}

TEST(JobRunner, TheEventLoopKeepsRunningWhileAJobComputes)
{
    // The work sleeps for 400 ms - the job being timed. With the work off the
    // GUI thread the runner's 10 ms timer keeps ticking; a pause anywhere near
    // the job's length would mean the GUI thread was waiting for it.
    JobRunner runner;
    const JobId id = runner.start("sleeps", [](JobControl&)
                                                -> katana::core::Result<JobRunner::Apply> {
        std::this_thread::sleep_for(std::chrono::milliseconds(400));
        return JobRunner::Apply{};
    });
    runner.waitFor(id);
    RecordProperty("longest_pause_ms",
                   static_cast<int>(runner.longestEventLoopPause() * 1000.0));
    EXPECT_LT(runner.longestEventLoopPause(), 0.2)
        << "the GUI thread stalled " << runner.longestEventLoopPause() * 1000.0
        << " ms during a 400 ms job";
}

TEST(JobRunner, TheEventLoopPauseMeasureSeesTheGuiThreadBlocked)
{
    // The control for the test above: block the GUI thread for 250 ms while a
    // job runs, and the measure must show at least that. Without this, a
    // measure that always read zero would pass the test above.
    JobRunner runner;
    std::atomic<bool> release{false};
    const JobId id = runner.start("waits", [&](JobControl&)
                                               -> katana::core::Result<JobRunner::Apply> {
        while (!release) {
            std::this_thread::yield();
        }
        return JobRunner::Apply{};
    });
    QCoreApplication::processEvents();
    std::this_thread::sleep_for(std::chrono::milliseconds(250)); // the GUI thread, blocked
    ASSERT_TRUE(pumpUntil([&] { return runner.longestEventLoopPause() >= 0.25; },
                          std::chrono::seconds(5)));
    release = true;
    runner.waitFor(id);
    EXPECT_GE(runner.longestEventLoopPause(), 0.25);
}

TEST(JobRunner, ATriangulationRunAsAJobLeavesTheEventLoopRunning)
{
    // The real work of Surface From Drawing, at a size that takes a while in a
    // Debug build: 60k scattered points. What the test holds is the RATIO -
    // the GUI thread paused for well under half of the work's time - because
    // absolute times on a shared machine are not a property of the code.
    katana::terrain::TinInput input;
    std::uint32_t state = 12345u;
    const auto next = [&state] {
        state = state * 1664525u + 1013904223u;
        return static_cast<double>(state) / 4294967296.0;
    };
    for (int i = 0; i < 60000; ++i) {
        const double x = next() * 1000.0;
        const double y = next() * 1000.0;
        input.points.emplace_back(x, y, 40.0 + 0.01 * x - 0.02 * y);
    }
    JobRunner runner;
    JobReport report;
    const JobId id = runner.start(
        "Triangulating",
        [input = std::move(input)](JobControl&) mutable
            -> katana::core::Result<JobRunner::Apply> {
            auto built = katana::terrain::buildTin(input);
            if (!built) {
                return built.error();
            }
            auto surface = std::make_shared<katana::terrain::TinSurface>(
                std::move(built->surface));
            return JobRunner::Apply([surface] {});
        },
        [&](const JobReport& r) { report = r; });
    runner.waitFor(id);
    ASSERT_EQ(report.outcome, JobOutcome::Finished) << report.error;
    RecordProperty("work_ms", static_cast<int>(report.workSeconds * 1000.0));
    RecordProperty("longest_pause_ms",
                   static_cast<int>(runner.longestEventLoopPause() * 1000.0));
    EXPECT_LT(runner.longestEventLoopPause(), report.workSeconds * 0.5)
        << "work " << report.workSeconds * 1000.0 << " ms, GUI pause "
        << runner.longestEventLoopPause() * 1000.0 << " ms";
}

TEST(JobRunner, TheStatusWidgetShowsTheRunningJobAndItsCancelButtonCancelsIt)
{
    QWidget host;
    JobRunner runner;
    QWidget* status = runner.createStatusWidget(&host);
    host.show();
    QCoreApplication::processEvents();
    EXPECT_FALSE(status->isVisible()) << "nothing runs, so nothing is shown";

    std::atomic<bool> running{false};
    JobReport report;
    const JobId id = runner.start(
        "Surface From Drawing",
        [&](JobControl& control) -> katana::core::Result<JobRunner::Apply> {
            control.setStage("Triangulating");
            running = true;
            while (!control.stopRequested()) {
                std::this_thread::yield();
            }
            return JobRunner::Apply{};
        },
        [&](const JobReport& r) { report = r; });
    ASSERT_TRUE(pumpUntil([&] { return running.load(); }));
    // One tick after the stage was set, so the label has been refreshed.
    const auto label = status->findChild<QLabel*>("JobStatusLabel");
    ASSERT_NE(label, nullptr);
    ASSERT_TRUE(pumpUntil([&] { return label->text().contains("Triangulating"); },
                          std::chrono::seconds(5)));
    EXPECT_TRUE(status->isVisible());
    EXPECT_TRUE(label->text().startsWith("Surface From Drawing")) << label->text().toStdString();
    const auto bar = status->findChild<QProgressBar*>("JobStatusProgress");
    ASSERT_NE(bar, nullptr);
    EXPECT_EQ(bar->maximum(), 0) << "no progress reported: a busy bar";

    const auto cancel = status->findChild<QToolButton*>("JobStatusCancel");
    ASSERT_NE(cancel, nullptr);
    cancel->click();
    runner.waitFor(id);
    EXPECT_EQ(report.outcome, JobOutcome::Cancelled);
    EXPECT_FALSE(status->isVisible()) << "hidden again once nothing runs";
}

} // namespace
