#include "survey/survey_task.hpp"

#include <QCoreApplication>
#include <QHBoxLayout>
#include <QLabel>
#include <QPointer>
#include <QProgressBar>
#include <QPushButton>
#include <QThreadPool>
#include <QTimer>

#include <exception>
#include <string>
#include <utility>

namespace katana::qt {

SurveyTaskBar::SurveyTaskBar(QWidget* parent) : QWidget(parent)
{
    setObjectName("task");
    auto* layout = new QHBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    progress_ = new QProgressBar(this);
    progress_->setObjectName("taskProgress");
    // A busy bar: nothing below reports how far it has got.
    progress_->setRange(0, 0);
    progress_->setMaximumWidth(160);
    progress_->setTextVisible(false);
    status_ = new QLabel(this);
    status_->setObjectName("taskStatus");
    status_->setWordWrap(true);
    cancel_ = new QPushButton("Cancel", this);
    cancel_->setObjectName("cancelTask");
    cancel_->setAutoDefault(false);
    layout->addWidget(progress_);
    layout->addWidget(status_, 1);
    layout->addWidget(cancel_);
    tick_ = new QTimer(this);
    tick_->setInterval(200);
    connect(tick_, &QTimer::timeout, this, [this] {
        status_->setText(QString("%1 - %2 s").arg(what_).arg(
            static_cast<double>(elapsed_.elapsed()) / 1000.0, 0, 'f', 1));
    });
    connect(cancel_, &QPushButton::clicked, this, [this] { cancel(); });
    setVisible(false);
}

void SurveyTaskBar::run(const QString& what, bool inBackground, Work work)
{
    const std::uint64_t ticket = ++ticket_;
    if (!inBackground) {
        // At once, as the page did before there were threads: a small file is
        // read in less time than the row would take to appear.
        setBusy(false);
        if (Finish finish = work()) {
            finish();
        }
        return;
    }
    what_ = what;
    elapsed_.start();
    status_->setText(what + "...");
    setBusy(true);
    const QPointer<SurveyTaskBar> self(this);
    QThreadPool::globalInstance()->start([self, ticket, work = std::move(work)]() mutable {
        Finish finish;
        std::string failure;
        try {
            finish = work();
        } catch (const std::exception& error) {
            // The readers and the reduction report failures through Result;
            // what reaches here is the machine running out of something.
            failure = error.what();
        } catch (...) {
            failure = "an unknown failure";
        }
        // Handed to the application object, which lives on the GUI thread
        // and outlives every page: the row itself may be gone by now.
        QMetaObject::invokeMethod(
            QCoreApplication::instance(),
            [self, ticket, finish = std::move(finish), failure = std::move(failure)] {
                if (self.isNull() || self->ticket_ != ticket) {
                    return; // cancelled, superseded, or the page was closed
                }
                self->setBusy(false);
                if (!failure.empty()) {
                    self->status_->setText(self->what_ + " failed: " +
                                           QString::fromStdString(failure));
                    self->setVisible(true);
                    return;
                }
                if (finish) {
                    finish();
                }
            },
            Qt::QueuedConnection);
    });
}

void SurveyTaskBar::cancel()
{
    if (!busy_) {
        return;
    }
    ++ticket_;
    setBusy(false);
    if (onCancelled) {
        onCancelled();
    }
}

void SurveyTaskBar::setBusy(bool busy)
{
    if (busy_ == busy) {
        setVisible(busy);
        return;
    }
    busy_ = busy;
    setVisible(busy);
    if (busy) {
        tick_->start();
    } else {
        tick_->stop();
    }
    if (onBusyChanged) {
        onBusyChanged(busy);
    }
}

} // namespace katana::qt
