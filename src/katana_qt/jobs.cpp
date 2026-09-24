#include "jobs.hpp"

#include <QApplication>
#include <QEventLoop>
#include <QHBoxLayout>
#include <QLabel>
#include <QMainWindow>
#include <QMetaObject>
#include <QObject>
#include <QProgressBar>
#include <QStatusBar>
#include <QTimer>
#include <QToolButton>

#include <algorithm>
#include <chrono>
#include <cmath>
#include <exception>
#include <optional>
#include <thread>
#include <utility>

namespace katana::qt {

using katana::core::ErrorCode;
using katana::core::makeError;

namespace {

using Clock = std::chrono::steady_clock;

[[nodiscard]] double secondsSince(Clock::time_point start)
{
    return std::chrono::duration<double>(Clock::now() - start).count();
}

[[nodiscard]] std::int64_t nowNs()
{
    return std::chrono::duration_cast<std::chrono::nanoseconds>(Clock::now().time_since_epoch())
        .count();
}

// Short enough that a pause is measured to about a hundredth of a second and
// the progress bar moves smoothly; long enough to cost nothing measurable.
constexpr int kTickMs = 10;
constexpr const char* kRunnerObjectName = "katanaJobRunner";

// Holds a window's runner as a child object, so that JobRunner::of() finds it
// by name and the window needs no member for it, and so that it is destroyed
// with the window (joining any job still running).
class RunnerHolder final : public QObject {
  public:
    JobRunner runner;
};

} // namespace

// ---- worker-side state --------------------------------------------------------------------

struct JobControl::Shared {
    // Thousandths done; -1 while the job has not reported (shown as busy).
    std::atomic<int> permille{-1};
    std::mutex stageMutex;
    std::string stage;
    // Written by the worker before it posts its completion and read on the GUI
    // thread after the post is delivered. The event queue's own lock is the
    // synchronisation, so these need none.
    std::optional<katana::core::Result<JobRunner::Apply>> result;
    double workSeconds = 0.0;
};

void JobControl::setProgress(double fraction) noexcept
{
    if (std::isnan(fraction)) {
        return;
    }
    const double clamped = std::clamp(fraction, 0.0, 1.0);
    shared_.permille.store(static_cast<int>(std::lround(clamped * 1000.0)),
                           std::memory_order_relaxed);
}

void JobControl::setStage(std::string stage)
{
    const std::lock_guard<std::mutex> lock(shared_.stageMutex);
    shared_.stage = std::move(stage);
}

struct JobRunner::Job {
    JobId id = kNoJob;
    QString title;
    Finished finished;
    std::shared_ptr<JobControl::Shared> shared;
    // Last, so it is destroyed first: ~jthread requests the stop and joins
    // while everything the thread shares is still alive.
    std::jthread thread;
};

// ---- the runner ---------------------------------------------------------------------------

JobRunner::JobRunner() : context_(std::make_unique<QObject>())
{
    timer_ = new QTimer(context_.get());
    timer_->setTimerType(Qt::PreciseTimer);
    timer_->setInterval(kTickMs);
    QObject::connect(timer_, &QTimer::timeout, context_.get(), [this] { tick(); });
}

JobRunner::~JobRunner()
{
    // Stop everything first, then join: a request to all before waiting for
    // any lets the jobs that can stop early do so in parallel.
    for (const auto& job : jobs_) {
        job->thread.request_stop();
    }
    jobs_.clear(); // joins
    // context_ goes next (it was declared first), discarding any completion
    // the threads posted to it before they ended.
}

JobRunner& JobRunner::of(QMainWindow& window)
{
    auto* holder = dynamic_cast<RunnerHolder*>(
        window.findChild<QObject*>(kRunnerObjectName, Qt::FindDirectChildrenOnly));
    if (holder == nullptr) {
        holder = new RunnerHolder;
        holder->setObjectName(kRunnerObjectName);
        holder->setParent(&window);
        window.statusBar()->addPermanentWidget(
            holder->runner.createStatusWidget(window.statusBar()));
    }
    return holder->runner;
}

JobId JobRunner::start(QString title, Work work, Finished finished)
{
    auto job = std::make_unique<Job>();
    job->id = nextId_++;
    job->title = std::move(title);
    job->finished = std::move(finished);
    job->shared = std::make_shared<JobControl::Shared>();

    const JobId id = job->id;
    QObject* const context = context_.get();
    job->thread = std::jthread(
        [this, id, context, shared = job->shared, work = std::move(work)](
            std::stop_token stop) mutable {
            const Clock::time_point started = Clock::now();
            JobControl control(stop, *shared);
            try {
                shared->result.emplace(work(control));
            } catch (const std::exception& error) {
                shared->result.emplace(
                    makeError(ErrorCode::Internal, std::string("the job failed: ") + error.what()));
            } catch (...) {
                shared->result.emplace(makeError(
                    ErrorCode::Internal, "the job failed with an exception of an unknown type"));
            }
            // Its inputs can be hundreds of megabytes: freed here, on the
            // worker, rather than on the GUI thread when the job is reaped.
            work = nullptr;
            shared->workSeconds = secondsSince(started);
            // Posted, never called: complete() touches the runner and the
            // callers' widgets, which belong to the GUI thread. If the runner
            // is destroyed first, the post dies with context_ undelivered.
            QMetaObject::invokeMethod(context, [this, id] { complete(id); }, Qt::QueuedConnection);
        });
    jobs_.push_back(std::move(job));
    startTicking();
    syncStatusWidget();
    return id;
}

bool JobRunner::cancel(JobId id)
{
    for (const auto& job : jobs_) {
        if (job->id == id) {
            job->thread.request_stop();
            return true;
        }
    }
    return false;
}

void JobRunner::cancelAll()
{
    for (const auto& job : jobs_) {
        job->thread.request_stop();
    }
}

bool JobRunner::isActive(JobId id) const
{
    return std::ranges::any_of(jobs_, [id](const auto& job) { return job->id == id; });
}

std::vector<JobProgress> JobRunner::progress() const
{
    std::vector<JobProgress> out;
    out.reserve(jobs_.size());
    for (const auto& job : jobs_) {
        JobProgress item;
        item.id = job->id;
        item.title = job->title;
        const int permille = job->shared->permille.load(std::memory_order_relaxed);
        item.fraction = permille < 0 ? -1.0 : static_cast<double>(permille) / 1000.0;
        {
            const std::lock_guard<std::mutex> lock(job->shared->stageMutex);
            item.stage = job->shared->stage;
        }
        out.push_back(std::move(item));
    }
    return out;
}

void JobRunner::complete(JobId id)
{
    const auto found =
        std::ranges::find_if(jobs_, [id](const auto& job) { return job->id == id; });
    if (found == jobs_.end()) {
        return;
    }
    // Out of the list before anything runs: Apply and Finished may start
    // another job or pump events, and neither must see this one as running.
    std::unique_ptr<Job> job = std::move(*found);
    jobs_.erase(found);
    const bool stopRequested = job->thread.get_stop_token().stop_requested();
    // The worker's last act was the post that brought us here, so this join
    // waits for at most the return from its lambda.
    job->thread.join();

    JobReport report;
    report.id = job->id;
    report.title = job->title;
    report.workSeconds = job->shared->workSeconds;
    auto& result = job->shared->result;
    if (stopRequested) {
        // Whatever the work produced, a cancelled job applies nothing: the
        // user said no, and a result arriving afterwards must not undo that.
        report.outcome = JobOutcome::Cancelled;
    } else if (!result.has_value() || !result->ok()) {
        report.outcome = JobOutcome::Failed;
        report.error = result.has_value() ? result->error().describe()
                                          : std::string("the job produced no result");
    } else {
        const Clock::time_point applying = Clock::now();
        try {
            if (const Apply& apply = result->value(); apply) {
                apply();
            }
            report.outcome = JobOutcome::Finished;
        } catch (const std::exception& error) {
            report.outcome = JobOutcome::Failed;
            report.error = std::string("applying the result failed: ") + error.what();
        } catch (...) {
            report.outcome = JobOutcome::Failed;
            report.error = "applying the result failed with an exception of an unknown type";
        }
        report.applySeconds = secondsSince(applying);
        // The Apply step is GUI-thread time a user waits through, and the
        // timer below cannot see it once the last job has gone.
        longestPause_ = std::max(longestPause_, report.applySeconds);
    }
    result.reset(); // the result (a whole surface, say) is not kept past its use

    if (jobs_.empty()) {
        timer_->stop();
    }
    syncStatusWidget();
    if (job->finished) {
        job->finished(report);
    }
    // Every waiter re-checks what it waits for; those still waiting run
    // their loop again.
    for (QEventLoop* loop : waiting_) {
        loop->quit();
    }
}

void JobRunner::startTicking()
{
    if (!timer_->isActive()) {
        lastTickNs_ = nowNs();
        timer_->start();
    }
}

void JobRunner::tick()
{
    const std::int64_t now = nowNs();
    longestPause_ = std::max(longestPause_, static_cast<double>(now - lastTickNs_) * 1.0e-9);
    lastTickNs_ = now;
    syncStatusWidget();
}

void JobRunner::waitUntil(const std::function<bool()>& done)
{
    // A real event loop, quit by complete(). Not processEvents(
    // WaitForMoreEvents): on Windows, outside exec(), that slept through the
    // worker's post and the timer alike and never returned (measured: the
    // first test here hung in MsgWaitForMultipleObjectsEx with the job long
    // finished). exec() blocks without spinning, so the wait costs no
    // processor either.
    while (!done()) {
        QEventLoop loop;
        waiting_.push_back(&loop);
        loop.exec();
        std::erase(waiting_, &loop);
    }
}

void JobRunner::waitFor(JobId id)
{
    waitUntil([this, id] { return !isActive(id); });
}

void JobRunner::waitForAll()
{
    waitUntil([this] { return jobs_.empty(); });
}

// ---- the status-bar widget ----------------------------------------------------------------

QWidget* JobRunner::createStatusWidget(QWidget* parent)
{
    auto* widget = new QWidget(parent);
    widget->setObjectName("JobStatus");
    auto* layout = new QHBoxLayout(widget);
    layout->setContentsMargins(0, 0, 0, 0);
    auto* label = new QLabel(widget);
    label->setObjectName("JobStatusLabel");
    auto* bar = new QProgressBar(widget);
    bar->setObjectName("JobStatusProgress");
    bar->setRange(0, 1000);
    bar->setTextVisible(false);
    bar->setMaximumWidth(140);
    bar->setMaximumHeight(14);
    auto* cancelButton = new QToolButton(widget);
    cancelButton->setObjectName("JobStatusCancel");
    cancelButton->setText("Cancel");
    cancelButton->setToolTip("Stop the job shown here; nothing it has computed is applied");
    cancelButton->setAutoRaise(true);
    layout->addWidget(label);
    layout->addWidget(bar);
    layout->addWidget(cancelButton);
    // context_ is the receiver, so the connection dies with the runner even
    // when the widget outlives it.
    QObject::connect(cancelButton, &QToolButton::clicked, context_.get(), [this] {
        if (!jobs_.empty()) {
            cancel(jobs_.front()->id);
        }
    });
    status_ = widget;
    syncStatusWidget();
    return widget;
}

void JobRunner::syncStatusWidget()
{
    if (status_.isNull()) {
        return;
    }
    if (jobs_.empty()) {
        status_->hide();
        return;
    }
    const std::vector<JobProgress> items = progress();
    const JobProgress& first = items.front();
    QString text = first.title;
    if (!first.stage.empty()) {
        text += QString::fromUtf8(": ") + QString::fromStdString(first.stage);
    }
    if (items.size() > 1) {
        text += QString(" (+%1 more)").arg(items.size() - 1);
    }
    if (jobs_.front()->thread.get_stop_token().stop_requested()) {
        text += " - cancelling";
    }
    if (auto* label = status_->findChild<QLabel*>("JobStatusLabel"); label != nullptr) {
        if (label->text() != text) {
            label->setText(text);
        }
    }
    if (auto* bar = status_->findChild<QProgressBar*>("JobStatusProgress"); bar != nullptr) {
        if (first.fraction < 0.0) {
            bar->setRange(0, 0); // busy: the job cannot say how far it is
        } else {
            bar->setRange(0, 1000);
            bar->setValue(static_cast<int>(std::lround(first.fraction * 1000.0)));
        }
    }
    status_->show();
}

} // namespace katana::qt
