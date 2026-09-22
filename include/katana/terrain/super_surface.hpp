#pragma once

// Combining surfaces in rank order (PLAN.MD 20.2, slice 8).
//
// A 12d "super tin" is a list of tins that behave as one surface: a base
// survey tin with design pads laid over it, so that where a pad has a level
// the pad's level is the answer and everywhere else the base's is. The 12da
// carries only the list and its order (manual 1.4.8); the surface itself is
// something the reader has to build.
//
// RANK ORDER: a LATER member overrides an earlier one wherever it can give a
// height. The manual does not state which end wins - it says only that the
// tins are listed - so this follows the order the sample archive spells out
// in its own words ("Rank order: base surface first, then the pads that
// substitute it over their footprints") and the plan written from it. It is
// one comparison to reverse if 12d Model turns out to disagree, and the test
// that pins it says as much.

#include <vector>

#include "katana/core/error.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::terrain {

// One surface from several, later members winning. Null members are
// ignored; an empty list, or one with nothing in it that has a triangle,
// gives an empty surface rather than an error, because a super tin whose
// members the archive did not carry is a fact about the file and not a
// failure of the reader.
//
// A triangle of a lower-ranked member is kept or dropped WHOLE, by whether
// a higher-ranked member can give a height at its centroid. The seam is
// therefore ragged to within one triangle of the pad's edge, which is
// stated rather than hidden: clipping each triangle against the higher
// surface's boundary and re-triangulating the offcuts would be exact, and
// would also mean re-running a constrained triangulation for every overlap -
// paid on every import of every archive, to move a seam by less than the
// triangle size of the data that drew it.
[[nodiscard]] katana::core::Result<TinSurface>
combineSurfaces(const std::vector<const TinSurface*>& membersInRankOrder);

} // namespace katana::terrain
