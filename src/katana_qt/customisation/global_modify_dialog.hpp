#pragma once

// Format > Global Modify...: 12d's "Change" panel and AutoCAD's Quick Select
// and Properties in one non-modal window over the drawing (the owner's
// request of 2026-09-25; the logic is cad::planGlobalModify,
// include/katana/cad/global_modify.hpp, and this dialog only states it).
//
//   Apply to   the selection | a view (the views open in the workspace, with
//              "only what is on screen" for a plan view) | the checked layers
//              (with or without their sublayers) | the whole drawing
//   Filter     types, layer and style patterns, colour, a property and its
//              value, text, drawn only - each optional
//   Modify     three tabs: Entities (layer, colour, style, shown, symbol,
//              text height, set and remove a property), Layers (colour,
//              linetype, weight, hatch, dimension style, shown, locked) and
//              Styles (colour, linetype, weight, hatch, symbol, symbol size).
//              A field is changed only when its box is ticked: an unticked
//              field is left as each entity, layer or style has it.
//
// The summary under the form is the plan's own (GlobalModifyPlan::summary),
// refreshed as the form is edited and as the drawing changes, so what Apply
// will do - and what it will leave alone and why - is read before it is
// pressed. Apply is ONE undo step. Select Matches selects what the scope and
// filter take, and shows it in the active plan view.
//
// The rules of docs/desktop.md, "The rules a dialog or panel follows": no
// moc; every control has an object name ("globalModify..."); a colour is
// typed as #RRGGBB or picked in a colour dialog opened with open(); nothing is
// asked in a modal box; the Document is watched through a DocumentWatcher,
// the last member, so the dialog may outlive it and then does nothing.

#include <functional>
#include <memory>
#include <optional>
#include <vector>

#include <QDialog>
#include <QString>

#include "customisation_context.hpp"
#include "katana/cad/global_modify.hpp"
#include "katana/cad/layer_overrides.hpp"
#include "katana/cad/view_set.hpp"
#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::qt {

// One open view, as the View scope offers it. `hidden` and `onScreen` are
// read at once by whoever asks for the list and never kept: the view may be
// closed by the next turn of the event loop.
struct GlobalModifyView {
    katana::cad::ViewId id = katana::cad::kNoView;
    QString title{}; // "Plan 1"
    const katana::cad::LayerOverrides* hidden = nullptr;
    // A plan view's visible area; none for a view with no plan extent.
    std::optional<katana::geometry::Box2> onScreen{};
};

class GlobalModifyDialog : public QDialog {
  public:
    explicit GlobalModifyDialog(const CustomisationContext& context, QWidget* parent = nullptr);
    ~GlobalModifyDialog() override;

    GlobalModifyDialog(const GlobalModifyDialog&) = delete;
    GlobalModifyDialog& operator=(const GlobalModifyDialog&) = delete;

    // The workspace's open views, asked whenever the view list or a View
    // scope is needed. Unset (a test, or no workspace): the View scope offers
    // one "Whole drawing view" hiding nothing.
    std::function<std::vector<GlobalModifyView>()> views{};

    // Refills the lists - layers, styles, linetypes, symbols, views -
    // keeping what is checked and typed. The watcher calls it (deferred); a
    // caller that needs it at once may too.
    void reload();

    // What the form says. Fails naming the field for text that does not
    // read - a colour that is not #RRGGBB or ByLayer, a view that has closed.
    [[nodiscard]] katana::core::Result<katana::cad::ModifyScope> scope() const;
    [[nodiscard]] katana::core::Result<katana::cad::ModifyFilter> filter() const;
    [[nodiscard]] katana::core::Result<katana::cad::GlobalModify> modification() const;

    // Each shows its outcome in the summary and the log, and returns whether
    // it did what it says: preview whether the request is valid, apply
    // whether the drawing changed, selectMatches whether anything matched.
    bool preview();
    bool apply();
    bool selectMatches();

    [[nodiscard]] QString summaryText() const;

  private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

} // namespace katana::qt
