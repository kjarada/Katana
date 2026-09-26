#pragma once

// Construction lines and rays (docs/drawing.md, "Draw tools"): drawing aids
// that run on for ever, which the model holds as lines KILOMETRES long on
// the construction layer, since it has no infinite line. The layer is the
// marker: an entity on "construction", or on a layer beneath it, is drawn on
// screen and snapped to, but never plotted and never counted in the drawing's
// extents - Zoom Extents frames the drawing, not the 100 km reach of an aid.

#include <string_view>

namespace katana::cad {

inline constexpr std::string_view kConstructionLayer = "construction";
// How far a construction line or ray reaches from its base point, in model
// units: past any drawing's extent, short of where coordinates lose their
// millimetres (1e5 m at double precision still resolves 1e-11 m).
inline constexpr double kConstructionReach = 1.0e5;

// True for the construction layer and every layer beneath it.
[[nodiscard]] constexpr bool isConstructionLayer(std::string_view layer)
{
    return layer == kConstructionLayer ||
           (layer.size() > kConstructionLayer.size() && layer.starts_with(kConstructionLayer) &&
            layer[kConstructionLayer.size()] == '/');
}

} // namespace katana::cad
