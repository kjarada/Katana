#pragma once

// A point picked in a plan view for a dialog (docs/geoprocessing.md, "The
// window"; docs/terrain.md, "Sampling and drape"): the sample points, a
// viewshed's observers, a sight line's ends. GeoServices::pickPoint.
//
// The next left click in any plan view of the workspace is the point, in
// ground units, read through that view's own transform (the one its
// clicks are read through); the click is taken by the pick and selects
// nothing. A right click or Esc in the view gives no point. Middle-button
// panning and the wheel pass through, so the person can move the view to
// the place first. Asking again while a pick is waiting ends the first with
// no point.
//
// What the pick hands back is only a point: the dialog writes it into its
// line (AT x,y, OBSERVER x,y), so what runs is still a line an agent could
// type.

#include <functional>
#include <optional>

#include "katana/geometry/primitives2d.hpp"

namespace katana::qt {

class ViewWorkspace;

using PickedPoint = std::function<void(std::optional<katana::geometry::Point2>)>;
using PointPicker = std::function<void(PickedPoint picked)>;

// A picker over the plan views `views` has when it is asked; with none open
// it answers at once with no point.
[[nodiscard]] PointPicker planPointPicker(ViewWorkspace& views);

} // namespace katana::qt
