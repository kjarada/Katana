#pragma once

// What a label says, and about which piece of which geometry: a label and
// its style turned into pieces - one per point, segment, arc, area or
// chainage mark - each with its template values and the geometry the placer
// places it against (docs/annotation.md, "Labels").
//
// In the entity layer, next to the template language, because the values
// are the model's own numbers (a bearing is a direction between two stored
// points, an RL a stored property) and every consumer - the painters through
// the placer, the command line's LABEL LIST, an exporter writing labels as
// text - must read the same ones.

#include <optional>
#include <span>
#include <string>
#include <vector>

#include "katana/entity/annotation.hpp"
#include "katana/entity/label_text.hpp"
#include "katana/entity/model.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::entity {

struct LabelPiece {
    enum class Shape {
        Point,   // a point: the anchor is the point
        Segment, // a straight piece from `from` to `to`; the anchor is its middle
        Arc,     // `arc`; the anchor is its middle
        Area,    // a closed figure; the anchor is a point inside it
        Station, // a chainage on an alignment; the anchor is on the line
    };
    Shape shape = Shape::Point;
    LabelValues values{};
    katana::geometry::Point2 anchor{};
    // The direction of the line at the anchor, radians counter-clockwise from
    // east: a segment's, an arc's tangent (in its direction of sweep), the
    // alignment's. 0 for a point and an area.
    double direction = 0.0;
    // The piece's length in model units: a segment's, an arc's; 0 otherwise.
    double length = 0.0;
    katana::geometry::Point2 from{};
    katana::geometry::Point2 to{};
    katana::geometry::Arc2 arc{};
    // A chainage tick with no text (LabelStyle::tickInterval between labels).
    bool tickOnly = false;
};

// The pieces `label` makes under `style` from the model as it is now. Empty
// when the target is gone, is not a kind of entity the style's kind labels
// (a Segment style on a circle), or has no such part - the painter then
// draws nothing for it and counts it, rather than inventing a place.
//
//   Point      a point entity: x y easting northing z rl
//   Segment    a line, or each segment of a polyline (or segment `part`):
//              bearing distance length dx dy dz grade segment
//   Arc        an arc, or a circle (delta a full turn): radius length chord
//              delta bearing tangent
//   Area       a closed polyline or a circle: area perimeter x y
//   Chainage   an alignment, every `interval` from its start (and a tick
//              every `tickInterval`): chainage x y alignment
//
// Every piece also has id, layer and, when the target carries them, point,
// description and prop.NAME for each property, and `code`: the first of
// `codeProperties` the target carries, in its properties or its metadata.
// The names are the caller's to give - survey coding owns where a code is
// kept (cad::codePropertyCandidates) and this layer cannot see it - and with
// none there is no code value. A chainage label has only its own values.
[[nodiscard]] std::vector<LabelPiece> labelPieces(const Model& model, const LabelGeometry& label,
                                                  const LabelStyle& style,
                                                  std::span<const std::string> codeProperties = {});

// The same, for a target that is not (yet) a label entity.
[[nodiscard]] std::vector<LabelPiece> labelPiecesFor(const Model& model, const Entity& target,
                                                     std::int32_t part, const LabelStyle& style,
                                                     std::span<const std::string> codeProperties = {});

// Whether a label style of `kind` can label `geometry` at all.
[[nodiscard]] bool labels(LabelKind kind, const Geometry& geometry);

// Where a new label of `style` on `target` attaches (LabelGeometry::anchor):
// the first piece's anchor, or the target's first point when there is none.
[[nodiscard]] std::optional<katana::geometry::Point2>
labelAnchor(const Model& model, const LabelGeometry& label, const LabelStyle& style);

} // namespace katana::entity
