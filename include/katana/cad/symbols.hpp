#pragma once

// The shapes a point symbol is drawn with (PLAN.MD 20.2, slice 2).
//
// A symbol is a named mark at a point: a circle, a cross, a tree, a manhole.
// `entity::symbolNames()` is the set; this is what each looks like, as plain
// strokes in model units, so that the viewport, the plotter and a preview all
// paint the same thing from one definition. Everything is polylines - a
// circle is chorded here at a fixed count, since a symbol is small on any
// output and the eye cannot tell 24 chords from a circle at that size.
//
// The unit shape fits the square from -1 to 1 about the origin; `size` is
// the model-unit half-width it is scaled to (half of Style::symbolSize,
// which is a width the way 12d's is), `rotation` radians counter-clockwise. A size of 0 means "the viewport's own mark", and the
// caller decides what that is in pixels - a symbol with no size is not
// drawn at no size.

#include <string_view>
#include <vector>

#include "katana/geometry/primitives2d.hpp"

namespace katana::cad {

// Empty for a name that is not a symbol (kNoSymbol among them): the caller
// draws its plain mark instead.
[[nodiscard]] std::vector<katana::geometry::Polyline2>
symbolStrokes(std::string_view symbol, const katana::geometry::Point2& centre, double size,
              double rotation = 0.0);

} // namespace katana::cad
