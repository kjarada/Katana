#pragma once

// A key plan, live (docs/plotting.md, "The coordinate grid and the live key
// plan").
//
// A key plan shows where every sheet's plan lies on the ground, each outline
// numbered. Its outlines are worked out from the sheets as they are when it
// is drawn - not stored when the key plan was made - so a sheet added,
// removed, reordered, rescaled or panned shows at once, numbered as the set
// numbers it now. The plans on the key plan's own sheet are flagged, for the
// painter to shade: "you are here".
//
// Pure functions on the sheet set; the painter only draws what they return.

#include <cstddef>
#include <functional>
#include <span>
#include <string>
#include <vector>

#include "katana/cad/plotting/plan_grid.hpp"
#include "katana/cad/plotting/sheet_set.hpp"

namespace katana::entity {
struct Model;
}

namespace katana::cad::plotting {

// One plan viewport's ground on a key plan.
struct KeyPlanOutline {
    std::string sheetId;
    std::size_t sheetIndex = 0;
    std::string viewportId;
    // The sheet's number as it prints now: its typed override, else the set's
    // numbering (printedSheetNumber).
    std::string label;
    // The ground under the plan's rectangle, world coordinates, counter-
    // clockwise from the corner under its bottom-left (planFootprint).
    std::vector<Point2> corners;
    // On the key plan's own sheet: "you are here".
    bool current = false;
    // False for a plan whose centre lies inside a larger outline of the same
    // sheet - an inset of it - whose number is written there already.
    bool labelled = true;

    friend bool operator==(const KeyPlanOutline&, const KeyPlanOutline&) = default;
};

// Decides where an automatic plan viewport's ground is (the Qt painter's
// resolvePlanViewport).
using PlanPlacer = std::function<PlanPlacement(const Viewport&)>;

// The number sheet `index` prints: its own "sheet_number" field when it has
// one, else the set's numbering pattern (formatSheetNumber). Empty for an
// index past the end.
[[nodiscard]] std::string printedSheetNumber(const SheetSet& set, std::size_t index);

// Every placed plan viewport (ViewportKind::Plan; key plans and sections are
// not outlined) on every sheet of `set`, in sheet order and then back to
// front, those on sheet `sheetIndex` flagged current. `place` decides each
// automatic viewport's scale and centre; without it, and for a fixed
// viewport, the stored ones are used. A viewport whose scale is not positive
// and finite is left out.
[[nodiscard]] std::vector<KeyPlanOutline>
keyPlanOutlines(const SheetSet& set, std::size_t sheetIndex, const PlanPlacer& place = {});

// Where an automatic plan viewport goes to show `points`, by the rule the
// painter applies to every automatic plan: autoCentre centres their extent as
// the viewport is turned; autoScale takes the largest scale of kSheetScales
// at which they fit with 4% to spare. What is not automatic is kept, and so is
// everything when there are no points.
[[nodiscard]] PlanPlacement fitPlanPlacement(const Viewport& viewport,
                                             std::span<const Point2> points);

// Where an automatic key plan goes: fitted to every outline and to `drawing`
// (the drawing's extent; empty for none), so no sheet falls off the edge.
[[nodiscard]] PlanPlacement fitKeyPlan(const Viewport& keyPlan,
                                       std::span<const KeyPlanOutline> outlines,
                                       const Box2& drawing);

// ---- the same, headless, over a model ---------------------------------------------
//
// The painter places an automatic plan over everything the window shows
// (resolvePlanViewport in the Qt layer): the drawing and also the imagery,
// point clouds and meshes only the application holds. These place it over the
// model alone, by the same rule, so a command line or an agent with no window
// gets the same outlines, key plan and grid for a drawing of entities and
// alignments.

// Where plan viewport `viewport` shows `model`: its stretch of its alignment
// (Viewport::source: from/to chainage, the whole alignment when they are
// equal), else what the model draws with the viewport's hidden layers hidden
// (cad::drawnExtent, and the alignments), fitted by fitPlanPlacement. A fixed
// viewport, an unplaced one, and one with nothing to fit keep their stored
// scale and centre.
[[nodiscard]] PlanPlacement placePlan(const entity::Model& model, const Viewport& viewport);

// The same for a viewport on sheet `sheetIndex` of `set`: an automatic key plan
// is fitted to every outline (keyPlanOutlines, each automatic plan placed by
// placePlan) and to the drawing (fitKeyPlan). Any other viewport is placed as
// above.
[[nodiscard]] PlanPlacement placePlan(const entity::Model& model, const SheetSet& set,
                                      std::size_t sheetIndex, const Viewport& viewport);

// placePlan over `model`, as keyPlanOutlines takes it. The model must outlive
// the placer.
[[nodiscard]] PlanPlacer modelPlacer(const entity::Model& model);

} // namespace katana::cad::plotting
