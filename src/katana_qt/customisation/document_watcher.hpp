#pragma once

// A Document listener for a customisation dialog: one call per event-loop
// turn, however many notifications arrived, saying WHAT changed.
//
// Why a dialog must not simply addListener: a view reacting to a Document
// listener DEFERS and COALESCES its reload (docs/cad.md). A table rebuilt
// inside the listener is rebuilt inside the command that notified, possibly
// inside its own item's change signal - which is how the layer panel once
// crashed - and a script of 500 commands would rebuild it 500 times. The
// watcher records each notification and delivers once, from the event loop
// (QTimer::singleShot(0)).
//
// What changed is worked out, not guessed from "a listener fired" (which a
// selection click also does):
//   * library / surveyMap: Document::libraryGeneration / surveyMapGeneration
//     differ from the last delivery. They only ever count up.
//   * selection: the selected ids differ from the last delivery.
//   * current: the current layer or current style differs.
//   * model: Document::modelRevision differs from the last delivery. It
//     counts every execute, undo and redo and every new or opened drawing,
//     and only ever counts up, so "undo, then a new command" in one turn -
//     which leaves the history's counts where they began - and a reopened
//     project that looks exactly like the drawing it replaced are both seen.
//
// Safe if the Document dies first: the registration is a ListenerHandle,
// which holds the Document's registry weakly, and a delivery already queued
// when the Document goes checks that registry before touching the Document -
// it then delivers nothing. Safe if the watcher dies first: the queued call
// is bound to an object the watcher owns, so Qt drops it.
//
// GUI thread only, as the Document is.

#include <cstdint>
#include <functional>
#include <memory>
#include <string>
#include <vector>

#include "katana/cad/document.hpp"

class QObject;

namespace katana::qt {

struct DocumentChanges {
    bool model = false;     // an execute, undo or redo; a new or opened drawing
    bool library = false;   // setStyleLibrary: libraryGeneration moved
    bool surveyMap = false; // setSurveyMap: surveyMapGeneration moved
    bool selection = false; // the selected entity ids differ
    bool current = false;   // the current layer or current style differs

    [[nodiscard]] bool any() const
    {
        return model || library || surveyMap || selection || current;
    }
    friend bool operator==(const DocumentChanges&, const DocumentChanges&) = default;
};

class DocumentWatcher {
  public:
    using Callback = std::function<void(const DocumentChanges& changes)>;

    // Registers with `document` at once. `onChanged` is called from the event
    // loop, at most once per turn and only in a turn after at least one
    // notification, with the changes since the previous call (or since
    // construction). It may be called with no flag set: the Document also
    // notifies for things none of the flags name (a save, the project's
    // metadata); a dialog that does not care checks any().
    DocumentWatcher(katana::cad::Document& document, Callback onChanged);
    ~DocumentWatcher();

    DocumentWatcher(const DocumentWatcher&) = delete;
    DocumentWatcher& operator=(const DocumentWatcher&) = delete;
    DocumentWatcher(DocumentWatcher&&) = delete;
    DocumentWatcher& operator=(DocumentWatcher&&) = delete;

    // False once the Document has been destroyed. A dialog that outlives its
    // Document asks this before touching it anywhere else too.
    [[nodiscard]] bool documentAlive() const;
    // A delivery is queued and has not yet run.
    [[nodiscard]] bool pending() const { return queued_; }

  private:
    // What a delivery compares with the delivery before.
    struct Baseline {
        std::uint64_t model = 0;
        std::uint64_t library = 0;
        std::uint64_t surveyMap = 0;
        std::vector<katana::entity::EntityId> selection{};
        std::string currentLayer{};
        std::string currentStyle{};
    };

    [[nodiscard]] Baseline takeBaseline() const;
    void notified();
    void deliver();

    katana::cad::Document* document_ = nullptr;
    Callback onChanged_{};
    Baseline baseline_{};
    bool queued_ = false;
    // The receiver of the queued delivery: deleting it with the watcher is
    // what makes Qt drop a delivery the watcher can no longer make.
    std::unique_ptr<QObject> receiver_;
    // Last, so it is destroyed first: no notification can arrive at a
    // half-destroyed watcher.
    katana::cad::Document::ListenerHandle registration_{};
};

} // namespace katana::qt
