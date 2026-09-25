#pragma once

// A leader and its callout, drawn (docs/annotation.md, "Leaders and
// callouts").
//
// The line runs through the leader's vertices from the arrow at its tip;
// from the last vertex a horizontal LANDING of `landing` paper millimetres
// runs away from the line (to the right when the last stretch heads right or
// straight up or down, to the left when it heads left), and the note is set
// beyond it, its middle on the landing's line: left-justified off a
// right-going landing, right-justified off a left-going one, as AutoCAD's
// MLEADER sets it. A Box callout frames the note with a margin of a third of
// its height; a Circle callout (a numbered balloon) centres the note in a
// circle that clears it by the same margin, and the landing ends on the
// circle. Every size is paper millimetres at 1 : scale.

#include "katana/cad/annotation/drawing.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/model.hpp"

namespace katana::cad::annotation {

[[nodiscard]] Drawing buildLeader(const katana::entity::Model& model,
                                  const katana::entity::LeaderGeometry& leader, double scale,
                                  const TextMeasure& measure);

// The arrowhead `head` of `size` model units at `tip` pointing along `along`
// (a unit vector, away from the line): the shapes dimension_draw.cpp draws,
// as strokes and fills. Exposed for the tests.
void appendArrowhead(Drawing& drawing, katana::entity::ArrowHead head, const Point2& tip,
                     const Vec2& along, double size);

} // namespace katana::cad::annotation
