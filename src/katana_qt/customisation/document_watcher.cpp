#include "document_watcher.hpp"

#include <utility>

#include <QObject>
#include <QTimer>

namespace katana::qt {

DocumentWatcher::DocumentWatcher(katana::cad::Document& document, Callback onChanged)
    : document_(&document), onChanged_(std::move(onChanged)),
      receiver_(std::make_unique<QObject>())
{
    lastMark_ = markModel();
    baseline_ = takeBaseline();
    registration_ = document.addListener([this] { notified(); });
}

// The registration goes first (it is the last member), then the receiver,
// which takes any queued delivery with it.
DocumentWatcher::~DocumentWatcher() = default;

bool DocumentWatcher::documentAlive() const { return registration_.active(); }

DocumentWatcher::ModelMark DocumentWatcher::markModel() const
{
    const katana::entity::Model& model = document_->model();
    ModelMark mark;
    mark.undo = document_->history().undoCount();
    mark.redo = document_->history().redoCount();
    mark.entities = model.entities.size();
    mark.layers = model.layers.size();
    mark.styles = model.styles.size();
    mark.linetypes = model.linetypes.size();
    mark.hatches = model.hatchPatterns.size();
    mark.dimensionStyles = model.dimensionStyles.size();
    mark.alignments = model.alignments.size();
    mark.project = document_->projectDirectory();
    return mark;
}

DocumentWatcher::Baseline DocumentWatcher::takeBaseline() const
{
    Baseline baseline;
    baseline.library = document_->libraryGeneration();
    baseline.surveyMap = document_->surveyMapGeneration();
    baseline.selection = document_->selection().ids(); // ascending: a std::set's order
    baseline.currentLayer = document_->currentLayer();
    baseline.currentStyle = document_->currentStyle();
    return baseline;
}

void DocumentWatcher::notified()
{
    // The model is marked at every notification, not at delivery: see the
    // header - one turn can undo and then execute, and end where it began.
    ModelMark mark = markModel();
    if (mark != lastMark_) {
        modelMoved_ = true;
        lastMark_ = std::move(mark);
    }
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
    changes.model = modelMoved_;
    changes.library = now.library != baseline_.library;
    changes.surveyMap = now.surveyMap != baseline_.surveyMap;
    changes.selection = now.selection != baseline_.selection;
    changes.current = now.currentLayer != baseline_.currentLayer ||
                      now.currentStyle != baseline_.currentStyle;
    baseline_ = std::move(now);
    modelMoved_ = false;
    // Last, and through a copy: the callback may delete the dialog that owns
    // this watcher, and with it the std::function that is running.
    if (const Callback callback = onChanged_) {
        callback(changes);
    }
}

} // namespace katana::qt
