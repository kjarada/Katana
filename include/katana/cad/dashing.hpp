#pragma once

// Walking a path and emitting its pen-down spans (PLAN.MD Phase 09).
//
// THE RULE THIS FILE EXISTS TO ENFORCE: a dash length is a MODEL length. A
// 0.5 m dash is half a metre of ground at every zoom, and the number of dashes
// on a 10 km boundary is twenty thousand times the number on a 0.5 m one.
//
// The easy implementation is the wrong one: hand the renderer a fixed pattern
// in pixels. That looks correct in a screenshot and is exactly backwards - the
// dashes stay the same size as you zoom, so a 10 m fence and a 10 km boundary
// get identical dashes. Nothing in this pipeline may express a pattern in
// pixels except the single final conversion in the 2D viewport, which is where
// Qt's units force it.
//
// PHASE starts at the first vertex of the entity's own path and is measured
// along that path. It is a property of the entity, not of the view, so dashes
// do not crawl while panning and do not redistribute while zooming. Both
// renderers must therefore traverse the vertices in the same order and close a
// closed path the same way.
//
// WHEN NOT TO DASH. Below roughly one device pixel per element, every dash and
// gap falls inside a single pixel and the antialiased result is a uniformly
// paler line: the user sees the wrong COLOUR, not a dash pattern. That is a
// display decision, not a geometric tolerance, so it is NOT in math::tolerance.

#include <cstddef>
#include <functional>
#include <vector>

#include "katana/entity/tables.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad {

using katana::geometry::Point2;

// Everything that decides how a pattern is laid down, in one place, so that
// LTSCALE and a per-entity scale become parameters here rather than a hunt
// through two renderers. Both are pinned at 1 today.
struct DashOptions {
    // Multiplies every element length. DXF $LTSCALE times the entity's own
    // scale (group code 48); the product, not either factor.
    double patternScale = 1.0;
    // Pixels per model unit, from the view transform. Used only to decide
    // whether the pattern is resolvable, never to size a dash.
    double viewScale = 1.0;
    // A pattern whose shortest element falls below this many device pixels is
    // drawn solid instead. One pixel is the point below which dashes stop
    // being visible as dashes and start being a paler line.
    double minimumElementPixels = 1.0;
    // Upper bound on emitted spans for one path. A dashed 10 km boundary at a
    // 0.1 m pattern is 100 000 spans; beyond the budget the caller is told to
    // draw the whole path solid rather than being handed a truncated one.
    std::size_t maximumSpans = 20000;
};

// Whether a pattern should be laid down at all at this scale.
[[nodiscard]] bool shouldDash(const katana::entity::Linetype& linetype,
                              const DashOptions& options);

// Walks `points` (plus the closing segment when `closed`) in MODEL space and
// calls `emit(from, to)` once per pen-down span. A dot is emitted as a
// zero-length span (from == to) and the caller decides how to draw one.
//
// Returns false when the pattern is continuous, is not resolvable at this
// scale, or would exceed `maximumSpans`. In every one of those cases NOTHING
// has been emitted and the caller must draw the path solid.
//
// The all-or-nothing contract is deliberate. An earlier design truncated at the
// budget, which silently shortened a long dashed line - a 900 m fence drawn as
// 300 m, with no error anywhere.
bool forEachDash(const std::vector<Point2>& points, bool closed,
                 const katana::entity::Linetype& linetype, const DashOptions& options,
                 const std::function<void(const Point2& from, const Point2& to)>& emit);

// Qt's QPen dash array, in units of PEN WIDTH - not pixels, which is the trap:
// the array is multiplied by the pen width when it is drawn, so a 2 px pen
// doubles every entry. Alternates on/off starting with on, as QPen requires.
//
// Empty when the pattern should not be dashed at this scale (see shouldDash),
// in which case the caller uses a solid pen.
//
// A dot becomes a short on-span rather than a zero, because a zero-length
// on-span draws nothing at all with a flat cap.
[[nodiscard]] std::vector<double> qtDashPattern(const katana::entity::Linetype& linetype,
                                                const DashOptions& options,
                                                double penWidthPixels);

} // namespace katana::cad
