#pragma once

// The layer manager (PLAN.MD 20.2, slice 6).
//
// The dock beside the drawing is the quick view: what is on, what is locked,
// what colour. This is the whole table - linetype, line weight, dimension
// style, hatch and how many entities are on each - with the operations that
// need more room than a dock has: move a layer (and its subtree, and the
// entities on it) under another parent, and put the selection on a layer.
//
// A layer name is a PATH ("design/surface/tin1"), so "move" and "rename" are
// the same command: renameLayer takes the subtree and the entities with it.
//
// NON-MODAL, as the styles manager is. It may be shown beside the drawing or
// exec()'d (Edit > Layers). It hears the Document through a DocumentWatcher,
// so an undo, an import or a command typed anywhere reloads it once, from the
// event loop, keeping the selected layer - and the form's unsaved edits, when
// the layer itself did not change underneath them. No button opens a modal
// box: New, New Child and Rename or Move ask for a name in a prompt row inside
// the dialog, a colour is typed as #RRGGBB or picked in a colour dialog that
// is opened, not exec()'d, and every refusal goes to the status line and the
// log. So a headless session and a test drive every action by objectName.
//
// The Document may be destroyed before the dialog: the dialog then reads and
// changes nothing, and every control but Close is disabled the first time a
// person reaches for one.
//
// objectNames: "layerTable"; the form "layerLinetype", "layerWeight",
// "layerColourText", "layerColour" (the Choose... button), "layerHatch",
// "layerDimensionStyle", "layerVisible", "layerLocked"; the buttons
// "layerNew", "layerNewChild", "layerSave", "layerRevert", "layerMove",
// "layerDelete", "layerAssign", "layerCurrent", "closeButton"; the prompt
// "layerPrompt", "layerPromptLabel", "layerPromptName", "layerPromptOk",
// "layerPromptCancel"; "layerStatus"; the colour dialog "layerColourDialog".

#include <functional>
#include <memory>
#include <string>

#include <QDialog>
#include <QString>

namespace katana::cad {
class Document;
}

namespace katana::qt {

// No Q_OBJECT: no signals or slots of its own (src/katana_qt/CMakeLists.txt).
class LayerManagerDialog : public QDialog {
  public:
    using Log = std::function<void(const QString& message, bool isError)>;

    LayerManagerDialog(katana::cad::Document& document, Log log, QWidget* parent = nullptr);
    ~LayerManagerDialog() override;

    // For the headless screenshot: selects the first row so the form below
    // is filled in.
    void showFirstRow();

    // The layer selected in the table, by full path; "" for none. And
    // selecting one (a name the table lacks selects nothing). For the
    // workbench ("open on this layer") and for tests.
    [[nodiscard]] std::string selectedLayer() const;
    void selectLayer(const std::string& name);

  private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

} // namespace katana::qt
