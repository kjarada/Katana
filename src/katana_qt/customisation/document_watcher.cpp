#include "document_watcher.hpp"

#include <utility>

#include <QObject>
#include <QTimer>

namespace katana::qt {

DocumentWatcher::DocumentWatcher(katana::cad::Document& document, Callback onChanged)
    : document_(&document), onChanged_(std::move(onChanged)),
      receiver_(std::make_unique<QObject>())
{
    baseline_ = takeBaseline();
    registration_ = document.addListener([this] { notified(); });
}

// The registration goes first (it is the last member), then the receiver,
// which takes any queued delivery with it.
DocumentWatcher::~DocumentWatcher() = default;

bool DocumentWatcher::documentAlive() const { return registration_.active(); }

DocumentWatcher::Baseline DocumentWatcher::takeBaseline() const
{
    Baseline baseline;
    baseline.model = document_->modelRevision();
    baseline.library = document_->libraryGeneration();
    baseline.surveyMap = document_->surveyMapGeneration();
    baseline.selection = document_->selection().ids(); // ascending: a std::set's order
    baseline.currentLayer = document_->currentLayer();
    baseline.currentStyle = document_->currentStyle();
    return baseline;
}

void DocumentWatcher::notified()
{
    if (queued_) {
        return; // this turn's delivery is already on its way
    }
    queued_ = true;
    QTimer::singleShot(0, receiver_.get(), [this] { deliver(); });
}

void DocumentWatcher::deliver()
{
    queued_ = false;
    // The Document can have gone between the notification and this turn: a
    // dialog closing with the application, a test's Document leaving scope.
    // Its registry went with it, and nothing of it may be read.
    if (!documentAlive()) {
        return;
    }
    Baseline now = takeBaseline();
    DocumentChanges changes;
    changes.model = now.model != baseline_.model;
    changes.library = now.library != baseline_.library;
    changes.surveyMap = now.surveyMap != baseline_.surveyMap;
    changes.selection = now.selection != baseline_.selection;
    changes.current = now.currentLayer != baseline_.currentLayer ||
                      now.currentStyle != baseline_.currentStyle;
    baseline_ = std::move(now);
    // Last, and through a copy: the callback may delete the dialog that owns
    // this watcher, and with it the std::function that is running.
    if (const Callback callback = onChanged_) {
        callback(changes);
    }
}

} // namespace katana::qt
