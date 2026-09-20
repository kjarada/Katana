#pragma once

// Turning a hatch pattern and a boundary into what a renderer draws
// (PLAN.MD Phase 09).
//
// The same division of labour as `dashing.hpp`: the geometry of the line
// family lives in `geometry::hatchLines`, and what lives here is the decision
// about whether drawing it is worth doing at the current zoom.
//
// That decision matters more for hatching than for dashes. A 0.2 m pattern
// over a 200 m parcel is a thousand lines; zoomed out to where the parcel is
// forty pixels across, those thousand lines land on forty pixels and read as a
// solid block - so the honest drawing is a solid block, and it costs one filled
// polygon instead of a thousand clipped segments. This is the same rule
// `cad::shouldDash` applies to a linetype that has become finer than the
// screen can show.

#include <vector>

#include "katana/entity/model.hpp"
#include "katana/entity/tables.hpp"
#include "katana/geometry/polygon.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad {

using katana::geometry::Polyline2;
using katana::geometry::Segment2;

struct HatchOptions {
    // Screen pixels per model unit, as the viewport's transform gives it.
    double viewScale = 1.0;
    // Below this on-screen spacing a family is drawn as a solid fill instead.
    // Three pixels: at two the lines merge into a tone with no gaps visible,
    // and at one they alias into moire. Chosen to match the threshold
    // `cad::shouldDash` uses for the same reason.
    double minimumSpacingPixels = 3.0;
};

enum class HatchDrawing {
    None,  // the pattern draws nothing - the built-in "none"
    Solid, // a solid fill, either asked for or too fine to resolve
    Lines, // the line families, drawn individually
};

// What `pattern` should be drawn as at this zoom. A pattern with any family
// finer than the threshold is drawn solid in its entirety rather than
// per-family, so a crosshatch does not become half solid and half lines as the
// user zooms out.
[[nodiscard]] HatchDrawing hatchDrawing(const katana::entity::HatchPattern& pattern,
                                        const HatchOptions& options);

// Every family of `pattern` clipped to `boundary`, concatenated in family
// order.
//
// Returns an empty list rather than an error when `boundary` cannot be hatched
// - an open polyline, fewer than three vertices, a pattern that is solid or
// draws nothing. A renderer asks this question once per entity per frame and
// has nothing useful to do with a failure; the model has already refused an
// invalid pattern on the way in, and `geometry::hatchLines` is where a caller
// that wants the reason should look.
[[nodiscard]] std::vector<Segment2> hatchSegments(const Polyline2& boundary,
                                                  const katana::entity::HatchPattern& pattern);

// The pattern an entity resolves to, or nullptr when it is not hatched.
// Follows the same ByLayer chain as `entity::resolveDisplay`, which is what
// resolves the NAME; this looks that name up in the document's table.
[[nodiscard]] const katana::entity::HatchPattern*
resolveHatchPattern(const katana::entity::Model& model, const katana::entity::Entity& entity);

} // namespace katana::cad
