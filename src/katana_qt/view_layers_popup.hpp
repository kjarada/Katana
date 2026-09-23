#pragma once

// The layers ONE view shows (the user's request of 2026-09-23 for "per view
// controls to add and remove layers"; docs/cad.md, "The workspace", for the
// rule a view's hidden layers take part in).
//
// Opened from the Layers button on a view's title bar. It edits that view's
// cad::ViewState - its LayerOverrides and hiddenReferences - and nothing
// else: never a command, never the document's modified flag, never the undo
// history. What a view hides is how the user is LOOKING at the drawing, like
// its zoom; the Layers panel, which edits the document, is where a layer is
// switched off for every view and saved with the project.
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

#include <QFrame>

#include "katana/cad/view_set.hpp"
#include "katana/core/error.hpp"

class QLabel;
class QLineEdit;
class QToolButton;
class QTreeWidget;
class QTreeWidgetItem;

namespace katana::qt {

class ViewWorkspace;

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
    // The layer item for `path`, or null.
    [[nodiscard]] QTreeWidgetItem* itemFor(const QString& path) const;

    // The three buttons. Show All shows everything the document shows,
    // references included. Isolate hides everything that is not the selected
    // layer, its parents or beneath it, replacing whatever this view hid;
    // Hide Others does the same but keeps what the view already hid beneath
    // the selected layer. Both fail with NotFound when no layer is selected.
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
    // Tells the workspace this view's hidden set changed, which repaints it
    // and updates its Layers button.
    void changed();

    ViewWorkspace& workspace_;
    katana::cad::ViewId view_;
    QLabel* heading_ = nullptr;
    QLineEdit* filter_ = nullptr;
    QTreeWidget* layers_ = nullptr;
    QLabel* referenceHeading_ = nullptr;
    QTreeWidget* references_ = nullptr;
    QToolButton* isolate_ = nullptr;
    QToolButton* hideOthers_ = nullptr;
    // True while the lists are being written from the view, so that the
    // itemChanged those writes raise is not taken for a click.
    bool updating_ = false;
};

} // namespace katana::qt
