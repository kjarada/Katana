#pragma once

// Work that may take long - reading a large field file, reducing and
// adjusting it - run OFF the GUI thread, with a row that says it is running
// and a button that cancels it.
//
// Why a row of its own rather than a progress dialog: a modal box would stop
// the person reading the page they are on, and a headless run must never open
// one. Why no percentage: the readers and the reduction report no progress
// (their contract has no callback), so the bar is a busy bar with the time so
// far - honest about what is known.
//
// Cancel ABANDONS the work: the GUI is free again at once and the result is
// thrown away when it arrives. The work itself cannot be interrupted inside a
// reader or the reduction (neither takes a stop token), so it runs to its end
// on a pool thread and nothing waits for it; QCoreApplication waits for the
// global pool when it is destroyed, so no work outlives the application.
//
// Small inputs do not go to a thread at all (`inBackground` false): they are
// done at once, on the GUI thread, exactly as before, so a headless run that
// presses Next and reads the page straight after sees the page complete.
//
// The work runs on another thread, so it may touch NOTHING of the GUI or the
// Document: it is given copies (or shared, immutable data) and returns a
// continuation that runs on the GUI thread - if and only if the task was not
// cancelled or superseded and this row still exists.
//
// Object names: task (the row), taskProgress, taskStatus, cancelTask.

#include <QElapsedTimer>
#include <QWidget>

#include <cstdint>
#include <functional>

class QLabel;
class QProgressBar;
class QPushButton;
class QTimer;

namespace katana::qt {

class SurveyTaskBar final : public QWidget {
  public:
    // What the work returns: the step to take on the GUI thread with its
    // result.
    using Finish = std::function<void()>;
    using Work = std::function<Finish()>;

    explicit SurveyTaskBar(QWidget* parent);

    // Runs `work` - on a pool thread when `inBackground`, at once otherwise -
    // then its Finish on the GUI thread. `what` is the sentence the row shows
    // while it runs ("Reading DAY1.GSI (50.2 MB)"). A task started while
    // another runs supersedes it: the older result is thrown away.
    void run(const QString& what, bool inBackground, Work work);
    // Abandons the running task, if any; `onCancelled` is told.
    void cancel();
    [[nodiscard]] bool busy() const { return busy_; }

    // Called when the running task is cancelled (not when it finishes), so
    // the page can put its buttons back.
    std::function<void()> onCancelled;
    // Called whenever busy() changes.
    std::function<void(bool busy)> onBusyChanged;

  private:
    void setBusy(bool busy);

    QProgressBar* progress_ = nullptr;
    QLabel* status_ = nullptr;
    QPushButton* cancel_ = nullptr;
    QTimer* tick_ = nullptr;
    QElapsedTimer elapsed_;
    QString what_;
    // Bumped by every run() and cancel(): a result whose ticket is not the
    // current one belongs to a task nobody is waiting for any more.
    std::uint64_t ticket_ = 0;
    bool busy_ = false;
};

} // namespace katana::qt
