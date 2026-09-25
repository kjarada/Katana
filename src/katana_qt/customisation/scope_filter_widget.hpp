#pragma once

// "Apply to" and "Only those that match": the scope and filter controls every
// dialog that reads or changes drawing data shows (docs/cad.md, "Scope and
// filter: one grammar for every verb on drawing data"; the owner's request of
// 2026-09-26: "i want the utility tools to act on data on view, layer/s,
// elements, filtered elements, like global change"). Taken out of
// Global Modify, whose two groups they were, so there is ONE set of controls
// and ONE reading of them:
//
//   Apply to   the selection | what a view shows (the views open in the
//              workspace, with "only what is on screen" for a plan view) |
//              the checked layers (with or without their sublayers) | the
//              whole drawing
//   Filter     types, layer and style patterns, colour, a property and its
//              value, text, drawn only - each optional
//
// It says what it holds in two forms, which cannot disagree because both are
// read from the same controls:
//   - scope() and filter(): the cad::ModifyScope and cad::ModifyFilter that
//     cad::matchEntities takes - what Global Modify plans with;
//   - verbWords(): the same as the words of the shared grammar
//     (cad::formatScopeWords, include/katana/cad/scope_verbs.hpp), for a
//     dialog that builds a verb line and hands it to the window's command
//     executor - the utilities dialog. A view is named by its id: VIEW <id>
//     for what is on screen, VIEW <id> EXTENTS for everything it draws.
//
// Every control's object name is the prefix given, then its part:
//   <prefix>ScopeGroup, <prefix>FilterGroup     the two group boxes
//   <prefix>ScopeSelection, <prefix>ScopeView, <prefix>ScopeLayers,
//   <prefix>ScopeDrawing                        the scope's radio buttons
//   <prefix>View, <prefix>OnScreen              the view, only what is on screen
//   <prefix>Layers, <prefix>Sublayers           the layers to tick, with sublayers
//   <prefix>Type<Point|Line|...>                the type boxes
//   <prefix>FilterLayer, <prefix>FilterStyle, <prefix>FilterColour,
//   <prefix>FilterProperty, <prefix>FilterValue, <prefix>FilterText,
//   <prefix>DrawnOnly                           the rest of the filter
// and the widget itself is <prefix>Scope. Global Modify passes "globalModify",
// so its controls keep the names they had; the utilities dialog passes
// "utility" (utilityScope, utilityScopeDrawing, utilityFilterLayer ...).
//
// No moc, as the rest of the window: the widget reports an edit through
// onChanged. It never touches the Document; reload is given the model to read
// the layers from.

#include <functional>
#include <optional>
#include <utility>
#include <vector>

#include <QString>
#include <QWidget>

#include "katana/cad/global_modify.hpp"
#include "katana/cad/layer_overrides.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/cad/view_set.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/geometry/primitives2d.hpp"

class QCheckBox;
class QComboBox;
class QLineEdit;
class QListWidget;
class QRadioButton;

namespace katana::entity {
struct Model;
}

namespace katana::qt {

// One open view, as the View scope offers it. `hidden` and `onScreen` are
// read at once by whoever asks for the list and never kept: the view may be
// closed by the next turn of the event loop.
struct ScopeFilterView {
    katana::cad::ViewId id = katana::cad::kNoView;
    QString title{}; // "Plan 1"
    const katana::cad::LayerOverrides* hidden = nullptr;
    // A plan view's visible area; none for a view with no plan extent.
    std::optional<katana::geometry::Box2> onScreen{};
};

// The open views of a workspace as the View scope offers them: each one's
// title, its own hidden layers and, for a plan view, its area on screen. Read
// afresh each time: a view closed since has no state to point at, and a plan
// view's area moves with every pan.
[[nodiscard]] std::vector<ScopeFilterView> scopeFilterViews(katana::cad::ViewSet& views);

// Which scope is chosen, in the order of the radio buttons.
enum class ScopeChoice { Selection, View, Layers, Drawing };

class ScopeFilterWidget final : public QWidget {
  public:
    explicit ScopeFilterWidget(const QString& namePrefix, QWidget* parent = nullptr);

    // The workspace's open views, asked whenever the view list or a View
    // scope is needed. Unset (a test, or no workspace): the View scope offers
    // one "Whole drawing view" hiding nothing, which scope() reads and
    // verbWords() refuses - a line can name only a view that is open.
    std::function<std::vector<ScopeFilterView>()> views{};
    // Called after any control is edited: a choice, a tick, a keystroke.
    std::function<void()> onChanged{};

    // What the View scope offers when `views` is unset: "Whole drawing
    // view", hiding nothing and with no area. For a dialog that keeps a
    // views function of its own and forwards it.
    [[nodiscard]] static std::vector<ScopeFilterView> noWorkspaceViews();

    // Refills the layer list from `model` and the view list from `views`,
    // keeping what is ticked and chosen.
    void reload(const katana::entity::Model& model);
    // The view list alone: for a dialog with no drawing to hand, whose views
    // still come from the window.
    void reloadViews();

    [[nodiscard]] ScopeChoice choice() const;
    void setChoice(ScopeChoice choice);

    // What the controls say. Fails naming the field for what does not read:
    // no layer ticked, a colour that is not #RRGGBB or ByLayer, a view that
    // has closed, "only what is on screen" for a view with no plan extent.
    [[nodiscard]] katana::core::Result<katana::cad::ModifyScope> scope() const;
    [[nodiscard]] katana::core::Result<katana::cad::ModifyFilter> filter() const;
    // The same as plain words (cad::ScopeWords), and as the words a line
    // takes. Fails as scope() and filter() do, and as formatScopeWords does
    // for what a line cannot say (a comma in a layer name, a double quote),
    // and for the fallback view, which is no view a line can name.
    [[nodiscard]] katana::core::Result<katana::cad::ScopeWords> words() const;
    [[nodiscard]] katana::core::Result<QString> verbWords() const;

  private:
    [[nodiscard]] std::vector<ScopeFilterView> openViews() const;
    [[nodiscard]] katana::core::Result<ScopeFilterView> chosenView() const;
    void changed();
    void updateEnabled();

    QString prefix_;
    QRadioButton* scopeSelection_ = nullptr;
    QRadioButton* scopeView_ = nullptr;
    QRadioButton* scopeLayers_ = nullptr;
    QRadioButton* scopeDrawing_ = nullptr;
    QComboBox* view_ = nullptr;
    QCheckBox* onScreen_ = nullptr;
    QListWidget* layers_ = nullptr;
    QCheckBox* sublayers_ = nullptr;

    std::vector<std::pair<katana::entity::EntityType, QCheckBox*>> types_;
    QLineEdit* filterLayer_ = nullptr;
    QLineEdit* filterStyle_ = nullptr;
    QLineEdit* filterColour_ = nullptr;
    QLineEdit* filterProperty_ = nullptr;
    QLineEdit* filterValue_ = nullptr;
    QLineEdit* filterText_ = nullptr;
    QCheckBox* drawnOnly_ = nullptr;
    // A reload refills the lists without calling onChanged for each item.
    bool reloading_ = false;
};

} // namespace katana::qt
