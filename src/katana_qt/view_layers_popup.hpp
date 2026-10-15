#pragma once

// The layers ONE view shows (the user's request of 2026-09-23 for "per view
// controls to add and remove layers"; docs/cad.md, "The workspace", for the
// rule a view's hidden layers take part in).
//
// Opened from the Layers button on a view's title bar. It sets that view's
// cad::ViewState - its LayerOverrides, its hiddenReferences and its ghosts of
// the selection (selectionGhosts) - and nothing else: never an undoable
// command, never the document's modified flag, never the undo history. And it
// does not set them itself: every box and button builds the VIEWS line a
// person would type (VIEWS HIDE, SHOW, ISOLATE, SET: viewLayersLine and
// viewReferencesLine below) and hands it to the window's command runner, so a
// click is logged as a typed line is, runs the one code path an agent's line
// does, and is refused by it the same way. What a view hides is how the user
// is LOOKING at the drawing, like its zoom; the Layers panel, which edits the
// document, is where a layer is switched off for every view and saved with the
// project.
//
// The tree is the document's layer tree, derived from the '/'-separated names
// (include/katana/entity/layer_path.hpp); a path segment that is not itself a
// layer is still a node, and hiding it hides what lies beneath. A box is
// ticked when this view does not hide that layer itself. It is greyed, and
// cannot be ticked, when the layer cannot show here whatever its box says -
// the document hides it (its own box in the Layers panel, or a parent's), or
// a parent is hidden in this view. That is the Layers panel's own greying
// (MainWindow::refreshLayers greys by effectivelyVisible) for the same reason:
// the box keeps its own setting, and the grey says the setting is overruled.
// A view can never show what the document hides (LayerOverrides is
// subtractive), so a greyed box offers nothing to tick.

#include <cstdint>
#include <string>
#include <vector>

#include <QFrame>
#include <QString>

#include "katana/cad/view_set.hpp"
#include "katana/core/error.hpp"

class QCheckBox;
class QLabel;
class QLineEdit;
class QToolButton;
class QTreeWidget;
class QTreeWidgetItem;

namespace katana::qt {

class ViewWorkspace;

// The VIEWS line that does `action` - HIDE, SHOW or ISOLATE - to `layers` in
// view `view`: what a click in the popup runs. The names are as the layer
// tree gives them, a name with a blank is quoted (commandWord), and a name no
// line can carry - a double quote or a line break in it, which the tokenizer
// has no escape for - is refused, naming it. Each name is a word, which the
// verb reads as a comma list; a name with a comma in it, or one the verb
// reads as its own word (ALL, LAYER, REFERENCE), is written after LAYER
// instead, which takes every word after it whole. ISOLATE takes one layer.
// InvalidArgument for no layer, an action that is none of the three, or two
// layers to isolate. A pure function of its arguments, tested without a window.
[[nodiscard]] katana::core::Result<QString> viewLayersLine(const std::string& action,
                                                           katana::cad::ViewId view,
                                                           const std::vector<std::string>& layers);

// VIEWS HIDE or SHOW <view> REFERENCE <ids>: the reference layers (the Reference
// Data panel's ids) a view hides. `action` is HIDE or SHOW.
[[nodiscard]] QString viewReferencesLine(const std::string& action, katana::cad::ViewId view,
                                         const std::vector<std::uint64_t>& references);

class ViewLayersPopup final : public QFrame {
  public:
    // A popup window (Qt::Popup: it closes on a click outside it or on Esc)
    // for the view `view` of `workspace`. Built from the document and the
    // view as they are now.
    ViewLayersPopup(ViewWorkspace& workspace, katana::cad::ViewId view, QWidget* parent = nullptr);

    // Shows the popup under `anchor` - the view's Layers button - moved to
    // stay on the anchor's screen, above the anchor when there is no room
    // below it.
    void popup(const QWidget* anchor);

    [[nodiscard]] katana::cad::ViewId view() const { return view_; }

    // What a test drives, as a person does. The layer items carry their full
    // path under kPathRole; the reference items their ReferenceId under
    // kReferenceRole.
    static constexpr int kPathRole = Qt::UserRole + 1;
    static constexpr int kReferenceRole = Qt::UserRole + 2;
    [[nodiscard]] QTreeWidget* layerTree() const { return layers_; }
    [[nodiscard]] QTreeWidget* referenceTree() const { return references_; }
    [[nodiscard]] QLineEdit* filterBox() const { return filter_; }
    // "Show the selection on hidden layers": the view's ghosts
    // (ViewState::selectionGhosts), set by VIEWS SET through the command
    // runner, as a typed line is.
    [[nodiscard]] QCheckBox* ghostsBox() const { return ghosts_; }
    // The layer item for `path`, or null.
    [[nodiscard]] QTreeWidgetItem* itemFor(const QString& path) const;

    // The three buttons, each by its line (above). Show All shows everything
    // the document shows, references included (VIEWS SHOW <id> ALL). Isolate
    // hides everything that is not the selected layer, its parents or beneath
    // it, replacing whatever this view hid (VIEWS ISOLATE); Hide Others does the
    // same but keeps what the view already hid beneath the selected layer: that
    // line, and then VIEWS HIDE of what it had hidden there. Both fail with
    // NotFound when no layer is selected, and with the line's own refusal when
    // the line is refused.
    void showAll();
    [[nodiscard]] katana::core::Status isolateSelected();
    [[nodiscard]] katana::core::Status hideOthers();

    // Shows only the layers and references whose path contains `text`, with
    // the parents that lead to them; case-insensitive. Empty shows all.
    void setFilter(const QString& text);

    // Rebuilds both lists from the document and the view.
    void rebuild();

  private:
    // Null when the view has closed; every change asks afresh rather than
    // holding a pointer into the ViewSet.
    [[nodiscard]] katana::cad::ViewState* state() const;
    // Re-reads the boxes, the greying and the buttons from the view, in
    // place: never rebuilds the tree from inside its own itemChanged.
    void refreshStates();
    void onLayerChanged(QTreeWidgetItem* item);
    void onReferenceChanged(QTreeWidgetItem* item);

    ViewWorkspace& workspace_;
    katana::cad::ViewId view_;
    QLabel* heading_ = nullptr;
    QLineEdit* filter_ = nullptr;
    QTreeWidget* layers_ = nullptr;
    QLabel* referenceHeading_ = nullptr;
    QTreeWidget* references_ = nullptr;
    QToolButton* isolate_ = nullptr;
    QToolButton* hideOthers_ = nullptr;
    QCheckBox* ghosts_ = nullptr;
    // True while the lists are being written from the view, so that the
    // itemChanged those writes raise is not taken for a click.
    bool updating_ = false;
};

} // namespace katana::qt
