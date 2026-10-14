#pragma once

// Terrain > Surface Styles... (docs/surface_display.md, "In the window"): the
// drawing's surface display styles in one window - every field of a style
// (surface_style.hpp) editable, a live preview of a sample surface drawn in
// it through the plan view's own painter, New, Copy, Rename, Delete and Set
// Default, and a surface's style chosen.
//
// NOTHING IS CHANGED HERE. Every edit builds the SURFACESTYLE (or SURFACE
// STYLE) line a person would type and hands it to the window's one executor
// (command_runner.hpp): echoed in the command log, kept in the history,
// undone as one step - and an agent does the same through the verbs. The
// form is a buffer, as the Alignment Manager's grids are: edited freely,
// previewed as it is, applied as one line naming only what changed.
//
// NON-MODAL, kept by the window. It hears the Document through a
// DocumentWatcher, so a SURFACESTYLE typed on the command line, an undo or an
// opened project reloads it from the event loop, keeping the chosen style -
// and the form's unapplied edits while the style they were made to is as it
// was.
//
// Object names, which the widget tests and the headless --dialog, --fill and
// --press steps drive it by:
//   surfaceStyleManagerDialog  the dialog (the action terrainSurfaceStyles' data)
//   surfaceStyleList           the styles, by name
//   surfaceStyleDefaultNote    which style is the default
//   surfaceStyleName           the name New, Copy and Rename take
//   surfaceStyleNew            SURFACESTYLE NEW "name"
//   surfaceStyleCopy           SURFACESTYLE NEW "name" FROM "chosen"
//   surfaceStyleRename         SURFACESTYLE RENAME "chosen" "name"
//   surfaceStyleDelete         SURFACESTYLE DELETE "chosen"
//   surfaceStyleSetDefault     SURFACESTYLE DEFAULT "chosen"
//   surfaceStyleTabs           Plan, Colours, Contours, 3D
//   Plan:     surfaceStyleDescription, surfaceStyleTriangles, surfaceStyleTriangleColour,
//             surfaceStyleBoundary, surfaceStyleBoundaryColour, surfaceStylePoints,
//             surfaceStylePointColour, surfaceStyleBreaklines, surfaceStyleBreaklineColour,
//             surfaceStyleFaces (none, flat, elevation, slope, aspect), surfaceStyleOpacity
//   Colours:  surfaceStyleRamp, surfaceStyleBanded, surfaceStyleBands, surfaceStyleStops
//             ("v:#RRGGBB;v:#RRGGBB ..."), surfaceStyleMin, surfaceStyleMax (empty: the
//             surface's own), surfaceStyleFlatColour
//   Contours: surfaceStyleContours, surfaceStyleInterval, surfaceStyleMajor, surfaceStyleBase,
//             surfaceStyleMinorColour, surfaceStyleMajorColour, surfaceStyleLabels
//   3D:       surfaceStyleShading (automatic, shaded, edges, wireframe, hidden),
//             surfaceStyleFaces3d (flat, elevation, slope, aspect)
//   surfaceStylePreview        a sample surface drawn in the form's style, with its legend
//   surfaceStyleLine           the SURFACESTYLE SET line Apply would run, or why none
//   surfaceStyleApply          runs it
//   surfaceStyleRevert         the form back to the drawing's style
//   surfaceStyleSurface        the drawing's surfaces
//   surfaceStyleAssign         SURFACE STYLE "surface" "chosen"
//   surfaceStyleStatus         what the last line replied, or why it was refused
//   surfaceStyleClose

#include <QDialog>

#include <memory>
#include <optional>

#include "command_runner.hpp"
#include "katana/cad/terrain/surface_style.hpp"
#include "katana/core/error.hpp"

namespace katana::cad {
class Document;
}

namespace katana::qt {

struct SurfaceStyleManagerContext {
    // The drawing. It may be destroyed before the dialog, which then does
    // nothing more (DocumentWatcher).
    katana::cad::Document* document = nullptr;
    // The window's one executor. Empty: the dialog says it cannot run a line.
    CommandRunner run{};
};

// No Q_OBJECT: it connects to lambdas, which keeps moc out of this target.
class SurfaceStyleManagerDialog final : public QDialog {
  public:
    explicit SurfaceStyleManagerDialog(SurfaceStyleManagerContext context, QWidget* parent = nullptr);
    ~SurfaceStyleManagerDialog() override;

    // Chooses style `name` in the list (case-insensitive); false when there
    // is none.
    bool selectStyle(const QString& name);
    [[nodiscard]] QString currentStyle() const;
    // The style the form says, unapplied edits and all: what the preview
    // draws. Absent when no style is chosen.
    [[nodiscard]] std::optional<katana::cad::SurfaceDisplayStyle> formStyle() const;
    // The SURFACESTYLE SET line Apply would run; empty when the form says
    // what the drawing holds, or cannot say it (surfaceStyleLine says why).
    [[nodiscard]] QString applyLine() const;

  private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

// The SURFACESTYLE SET words that turn `from` into `to`, one key=value a
// field that differs, in the order of the verb's keys - the line Apply runs,
// worked out apart from the dialog so it is tested on its own. Empty when
// they are equal. Reals are written exactly (exactNumber), colours as
// #RRGGBB (#RRGGBBAA when not opaque), names through commandWord.
// InvalidArgument for what a line cannot say: a double quote or a line break
// in the description.
[[nodiscard]] katana::core::Result<QString>
surfaceStyleSetWords(const katana::cad::SurfaceDisplayStyle& from,
                     const katana::cad::SurfaceDisplayStyle& to);

} // namespace katana::qt
