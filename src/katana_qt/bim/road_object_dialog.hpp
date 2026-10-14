#pragma once

// BIM > Road Objects > Barrier, Guide Posts, Sign, Light Poles, Culvert,
// Wall, Fence, Line Marking (docs/road_objects.md, "In the window"): one
// dialog, a page per kind, that places a road object.
//
// NOTHING IS PLACED HERE. The dialog writes the ROADOBJECT ADD line its
// fields describe - shown, as it will run, in roadObjectLine - and Add hands
// it to the window's one executor (command_runner.hpp) exactly as if it had
// been typed: echoed in the command log, kept in the history, one undo step.
// Preview runs the same line with PREVIEW, which changes nothing. So there is
// nothing the dialog can do that an agent cannot by typing the line it shows.
//
// The line is made by roadObjectAddLine, a pure function of the form, so it
// is tested without a window. The object's own keys stand before its
// placement, so a scope's WHERE, which takes every key=value after it, can
// end the line. A field left empty is left out - the kind's default, which
// its placeholder shows. A number that does not read, a placement with no
// alignment or no points, or a value a line cannot say (a double quote)
// refuses the whole line, saying which field; nothing runs.
//
// Each page shows the kind's parameters, built from its own table
// (cad::road::parametersOf) - so a parameter added there appears here - and
// the preview below draws the object those parameters make, by the same code
// the drawing is made by, on a straight level road: a barrier, a wall, a
// fence or a marking seen along its line, which is its cross-section; a
// post, a sign or a light pole from the front; a culvert across its barrel.
//
// Object names, which the widget tests and the headless --dialog, --fill and
// --press steps drive it by:
//   roadObjectDialog           the dialog (every Road Objects action's data)
//   roadObjectKinds            a tab per kind, in cad's order
//   roadObject_<kind>_<key>    a parameter: a field, or a choice whose first
//                              item is the default ("roadObject_barrier_type")
//   roadObjectKindSource       where the kind's defaults come from
//   roadObjectPreview          the preview
//   roadObjectPlaceAlignment, roadObjectPlaceLines, roadObjectPlacePoints,
//   roadObjectPlaceDrawnPoints the placement: along an alignment, along the
//                              lines the scope takes, at points typed, at the
//                              points the scope takes
//   roadObjectAlignment        the alignment (a choice of the drawing's)
//   roadObjectStation, roadObjectTo, roadObjectOffset, roadObjectSpacing
//   roadObjectPoints           "x,y[,z] x,y[,z] ..."
//   roadObjectSourceScope      the scope and filter (ScopeFilterWidget, its
//                              controls roadObjectSourceScopeSelection,
//                              roadObjectSourceScopeDrawing,
//                              roadObjectSourceLayers ...)
//   roadObjectHeights          automatic, profile, surface or line
//   roadObjectSurface          the surface (an editable choice)
//   roadObjectCrossfall, roadObjectDz
//   roadObjectName, roadObjectLayer, roadObjectColour, roadObjectMaterial,
//   roadObjectFacility
//   roadObjectLine             the exact line Add will run (read-only)
//   roadObjectPreviewRun       run it with PREVIEW
//   roadObjectAdd              run it
//   roadObjectReply            what the line replied, or why it was refused
//   roadObjectStatus           what happened last
//   roadObjectClose

#include <QDialog>
#include <QString>

#include <functional>
#include <memory>
#include <string>
#include <utility>
#include <vector>

#include "command_runner.hpp"
#include "customisation/scope_filter_widget.hpp"
#include "katana/cad/bim/bim_object.hpp"
#include "katana/cad/road/road_objects.hpp"
#include "katana/core/error.hpp"

class QShowEvent;

namespace katana::cad {
class Document;
}

namespace katana::qt {

// What the form holds, as text, for roadObjectAddLine.
struct RoadObjectForm {
    enum class Place { Alignment, Lines, Points, DrawnPoints };

    katana::cad::road::RoadObjectKind kind = katana::cad::road::RoadObjectKind::Barrier;
    Place place = Place::Alignment;
    QString alignment{};
    QString station{};
    QString to{};
    QString offset{};
    QString spacing{};
    QString points{};
    // The scope's words (ScopeFilterWidget::verbWords), for Lines and
    // DrawnPoints.
    QString scope{};
    // "", profile, surface or line.
    QString heights{};
    QString surface{};
    QString crossfall{};
    QString dz{};
    QString name{};
    QString layer{};
    QString colour{};
    QString material{};
    QString facility{};
    // The kind's parameters given, key and value; empty values are left out.
    std::vector<std::pair<QString, QString>> parameters{};
    bool preview = false;
};

// The ROADOBJECT ADD line `form` describes. InvalidArgument naming the field
// for an alignment placement with no alignment or station, points with none,
// a number that does not read, a scope missing where one is needed, or a
// value with a double quote in it.
[[nodiscard]] katana::core::Result<QString> roadObjectAddLine(const RoadObjectForm& form);

// The object `kind` and `parameters` make on a straight, level road - what
// the preview draws: a linear kind 10 m along it, a point kind 2 m off it, a
// culvert across it. Parameters that do not read are left at their defaults.
[[nodiscard]] katana::cad::bim::BimModel
roadObjectPreview(katana::cad::road::RoadObjectKind kind,
                  const std::vector<std::pair<std::string, std::string>>& parameters);

struct RoadObjectDialogContext {
    // The drawing: its alignments, surfaces and layers. It may be destroyed
    // before the dialog, which then does nothing more (DocumentWatcher).
    katana::cad::Document* document = nullptr;
    // The window's one executor. Empty: the dialog says it cannot run.
    CommandRunner run{};
    // The open views, for the scope's View choice. Empty: none.
    std::function<std::vector<ScopeFilterView>()> views{};
};

// No Q_OBJECT: it connects to lambdas, which keeps moc out of this target.
class RoadObjectDialog final : public QDialog {
  public:
    RoadObjectDialog(RoadObjectDialogContext context, QWidget* parent = nullptr);
    ~RoadObjectDialog() override;

    // The page of `kind`.
    void showKind(katana::cad::road::RoadObjectKind kind);
    [[nodiscard]] katana::cad::road::RoadObjectKind currentKind() const;
    // The form as the fields hold it.
    [[nodiscard]] RoadObjectForm form() const;
    // The alignments, surfaces, layers and views again.
    void reload();

  protected:
    void showEvent(QShowEvent* event) override;

  private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

} // namespace katana::qt
