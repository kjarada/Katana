#pragma once

// Domain-independent entity model (PLAN.MD Phase 05).
//
// An Entity is a plain value: identity + geometry + presentation attributes +
// open-ended properties. It knows nothing about rendering, storage, Qt or
// surveying. The renderer consumes entities; it never owns them (Rule 3).

#include <cstdint>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <variant>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/core/text_encoding.hpp"
#include "katana/geometry/curves2d.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::entity {

using EntityId = std::uint64_t;
inline constexpr EntityId kInvalidEntityId = 0;

// Order matches the alternatives of Geometry.
enum class EntityType {
    Point,
    Line,
    Arc,
    Polyline,
    Circle,
    Text,
    Dimension,
    Label,
    Leader,
    // The drawing system's kinds (docs/drawing.md), appended after the
    // annotation system's Label and Leader so the kind bytes never collide.
    CurvePolyline,
    Ellipse,
    Spline,
};

[[nodiscard]] std::string_view toString(EntityType type);
[[nodiscard]] katana::core::Result<EntityType> entityTypeFromString(std::string_view name);

// True when `text` is well-formed UTF-8. Defined in core (text_encoding.hpp),
// which the text decoder needs it in, and named here as well because the model
// is where it is enforced.
//
// Every string that reaches the model is checked, because the JSON writer used
// by project storage THROWS on invalid UTF-8 - and it is reached from a path
// that returns Status and promises not to throw. Text typed on a CP1252 console
// or read from an ANSI file is the ordinary way to produce such bytes, so this
// is a routine input to reject, not a hypothetical one.
using katana::core::isValidUtf8;

struct PointGeometry {
    katana::geometry::Point2 position;
    friend constexpr bool operator==(const PointGeometry&, const PointGeometry&) = default;
};

// How the arrow at the end of a dimension line or a leader is drawn. Declared
// here rather than beside DimensionStyle (tables.hpp) because a leader
// carries its own; toString and arrowHeadFromString are still in tables.hpp.
enum class ArrowHead { None, Tick, ClosedFilled, Open, Dot };

// Where a text's `position` sits on its block of lines: a row (bottom, middle,
// top) and a column (left, centre, right), the nine points every drafting
// package offers. The bottom row is the BASELINE of the last line - not the
// descenders - so BottomLeft is exactly the "left end of the baseline" every
// text had before justification existed, and is the default, which is what
// keeps a text written before it where it was. The top row is the top of the
// first line's em box (its height above its baseline); the middle row is
// halfway between the two. docs/annotation.md has the pictures.
//
// The enumerator values are stored (geometry_blob.cpp, serialization.cpp by
// name), so they are append-only like the geometry kinds.
enum class TextJustify : std::uint8_t {
    BottomLeft = 0,
    BottomCentre = 1,
    BottomRight = 2,
    MiddleLeft = 3,
    MiddleCentre = 4,
    MiddleRight = 5,
    TopLeft = 6,
    TopCentre = 7,
    TopRight = 8,
};

// "BL", "BC", "BR", "ML", "MC", "MR", "TL", "TC", "TR": the names the command
// line and the JSON use. fromString also takes the long forms ("MiddleCentre",
// "middle-center") and "L", "C", "R" for the baseline row, which is how a
// person coming from another CAD package writes them.
[[nodiscard]] std::string_view toString(TextJustify justify);
[[nodiscard]] katana::core::Result<TextJustify> textJustifyFromString(std::string_view name);

struct TextGeometry {
    katana::geometry::Point2 position; // the justification point (BottomLeft: left end of the baseline)
    // One or more lines, separated by '\n'. The first line is on top.
    std::string text;
    // MODEL units. For a paper-sized text (paperHeight, or a style with a
    // paper height) this is the height it had at the annotation scale it
    // was made or last scaled at: what the bounding box, picking and the
    // spatial index go by, since the entity layer cannot know which view a
    // text is looked at through. The painters draw such a text from its
    // paper height at THEIR scale (docs/annotation.md).
    double height = 2.5;
    double rotation = 0.0; // radians, counter-clockwise

    // Appended 2026-09-25, each defaulting to what a text written before then
    // was: no style, a model-unit height, the baseline-left anchor. A text
    // with every one of them at its default is stored exactly as before
    // (geometry_blob.cpp), so an older build still opens it.
    //
    // The text style (entity/annotation.hpp) it is set in; empty for none -
    // the application's plain face at `height`.
    std::string style{};
    // Millimetres on paper; > 0 makes the text paper-sized, drawn at
    // paperHeight x scale / 1000 model units for the scale of the view or
    // sheet viewport showing it. 0 takes the style's paper height, and with
    // no style (or a style with none) the text is a model-unit text at
    // `height`, exactly as before.
    double paperHeight = 0.0;
    TextJustify justify = TextJustify::BottomLeft;

    friend bool operator==(const TextGeometry&, const TextGeometry&) = default;
};

// A point on ANOTHER entity, named rather than copied, so that a dimension or
// a leader that refers to it follows it when it moves (the associative update,
// cad/annotation/associative.hpp). `entity` 0 is no reference: the point is
// the annotation's own, as every point was before.
//
// `point` says which point of the entity, and `index` which vertex or segment
// where that needs saying; entity/anchor.hpp resolves one against an entity
// and says which kinds of entity offer which points.
enum class AnchorPoint : std::uint8_t {
    Position = 0, // a point's position, a text's insertion point
    Start = 1,    // a line's or an arc's start, a polyline's first vertex
    End = 2,      // a line's or an arc's end, a polyline's last vertex
    Mid = 3,      // a line's or an arc's midpoint
    Centre = 4,   // an arc's or a circle's centre
    Vertex = 5,   // a polyline's vertex `index`
    SegmentMid = 6, // the midpoint of a polyline's segment `index`
};

[[nodiscard]] std::string_view toString(AnchorPoint point);
[[nodiscard]] katana::core::Result<AnchorPoint> anchorPointFromString(std::string_view name);

struct AnchorRef {
    std::uint64_t entity = 0; // an EntityId; 0 means "not associated"
    AnchorPoint point = AnchorPoint::Position;
    std::uint32_t index = 0;

    [[nodiscard]] constexpr bool associated() const { return entity != 0; }
    friend constexpr bool operator==(const AnchorRef&, const AnchorRef&) = default;
};

// What a dimension measures. Aligned is the only kind there was before
// 2026-09-25, and the default, so a dimension written before then is an
// Aligned one with no references. Append-only: the value is stored.
//
//   Aligned     |end - start|, the dimension line parallel to start->end and
//               `offset` to its left (negative: right).
//   Linear      the length of end - start along the direction `angle` - a
//               horizontal dimension is angle 0, a vertical one pi/2, and any
//               other angle is what AutoCAD calls a rotated one. The line is
//               parallel to that direction, `offset` to its left through start.
//   Angular     the angle at `vertex` swept counter-clockwise from the ray
//               through `start` to the ray through `end`; the arc is drawn at
//               radius `offset` (the shorter ray when 0).
//   Radius      |start - vertex|: `vertex` is the centre and `start` the point
//               on the curve; the line runs from the centre through start and
//               on for `offset` to the text.
//   Diameter    twice that: the line crosses the whole circle through start.
//   OrdinateX   (start - vertex) along `angle` (east when 0): `vertex` is the
//               datum, `start` the feature, and `end` where the leader ends
//               and the text sits. Signed: a feature west of the datum is
//               negative.
//   OrdinateY   the same across `angle` (north when 0).
enum class DimensionKind : std::uint8_t {
    Aligned = 0,
    Linear = 1,
    Angular = 2,
    Radius = 3,
    Diameter = 4,
    OrdinateX = 5,
    OrdinateY = 6,
};

[[nodiscard]] std::string_view toString(DimensionKind kind);
[[nodiscard]] katana::core::Result<DimensionKind> dimensionKindFromString(std::string_view name);

struct DimensionGeometry {
    katana::geometry::Point2 start;
    katana::geometry::Point2 end;
    double offset = 0.0;
    std::string textOverride; // empty: show the measured value

    // Appended 2026-09-25 with the kinds above; a dimension with every one of
    // them at its default is stored exactly as before (geometry_blob.cpp).
    DimensionKind kind = DimensionKind::Aligned;
    // The direction a Linear or ordinate dimension measures along, radians
    // counter-clockwise from east.
    double angle = 0.0;
    // The apex of an angular dimension, the centre of a radial one, the datum
    // of an ordinate one; unused by Aligned and Linear.
    katana::geometry::Point2 vertex{};
    // The entities `start`, `end` and `vertex` follow (the associative update).
    AnchorRef startRef{};
    AnchorRef endRef{};
    AnchorRef vertexRef{};

    // The measured value: a length in model units, or for Angular an angle in
    // radians in [0, 2 pi). Signed for the ordinate kinds, never negative
    // otherwise.
    [[nodiscard]] double measurement() const;
    // Whether `vertex` is one of the dimension's points.
    [[nodiscard]] bool usesVertex() const
    {
        return kind != DimensionKind::Aligned && kind != DimensionKind::Linear;
    }
    friend bool operator==(const DimensionGeometry&, const DimensionGeometry&) = default;
};

// A label that says something about another entity or an alignment - a
// point's number and level, a line's bearing and distance, a lot's area, the
// chainages along an alignment - through a label style's template
// (entity/annotation.hpp, entity/label_text.hpp). It stores WHICH thing it
// labels and HOW, never the text: the text is worked out from the target
// every time it is drawn, so a label cannot disagree with the geometry it
// describes, however that geometry was edited. Where it is drawn depends on
// the scale it is looked at (the placer, cad/annotation/label_layout.hpp).
//
// A label is an entity so that it lives on a layer (labels switched off by
// layer, as a surveyor expects), is erased, undone and saved like any other
// entity, and is found by LIST.
struct LabelGeometry {
    std::uint64_t target = 0;  // the EntityId labelled; 0 when `alignment` names the target
    std::string alignment{};   // a chainage label's alignment
    // Which piece of the target: a polyline's segment, -1 for every one. Only
    // a Segment label style of a polyline uses it.
    std::int32_t part = -1;
    std::string style{};       // the label style (required)
    // Where the label attaches, worked out from the target when it is made
    // and kept up to date by the associative update: the label's position for
    // the bounding box, the spatial index and picking, and where it is drawn
    // if its target is gone.
    katana::geometry::Point2 anchor{};
    // Dragged by hand: the text is put HERE, with a leader to the anchor
    // when the style asks for one, instead of where the placer would put it.
    std::optional<katana::geometry::Point2> position{};
    // Replaces the template's text, verbatim, when not empty.
    std::string textOverride{};
    // The auto-label rule that made it (cad/annotation/auto_label.hpp); empty
    // for one placed by hand. AUTOLABEL RUN replaces a rule's labels and
    // AUTOLABEL CLEAR removes them, and neither touches a hand-placed one.
    std::string rule{};

    friend bool operator==(const LabelGeometry&, const LabelGeometry&) = default;
};

// How a leader's note is framed: a callout's box, or a numbered balloon's
// circle. Append-only: the value is stored.
enum class CalloutShape : std::uint8_t { None = 0, Box = 1, Circle = 2 };

[[nodiscard]] std::string_view toString(CalloutShape shape);
[[nodiscard]] katana::core::Result<CalloutShape> calloutShapeFromString(std::string_view name);

// A leader: an arrow from a feature through any number of bends to a note.
// Sizes are PAPER millimetres, drawn at the scale of the view or sheet
// viewport looking at it, as every annotation added on 2026-09-25 is; the
// leader tool drew a polyline, an arrowhead and texts before, which drifted
// apart when one was moved.
struct LeaderGeometry {
    // The arrow's tip first, then each bend; the last vertex is where the
    // landing - the short horizontal the note sits at - begins. At least two.
    std::vector<katana::geometry::Point2> vertices;
    std::string text{};        // lines separated by '\n'; empty for a bare leader
    ArrowHead arrow = ArrowHead::ClosedFilled;
    CalloutShape callout = CalloutShape::None;
    std::string style{};       // the text style; empty for the default one
    double paperHeight = 0.0;  // mm; 0 takes the style's (2.5 mm when it has none)
    double arrowSize = 2.5;    // mm on paper
    double landing = 2.5;      // mm on paper; 0 for none
    // The entity the tip follows (the associative update); not associated by
    // default.
    AnchorRef tipRef{};

    friend bool operator==(const LeaderGeometry&, const LeaderGeometry&) = default;
};

using Geometry =
    std::variant<PointGeometry, katana::geometry::Segment2, katana::geometry::Arc2,
                 katana::geometry::Polyline2, katana::geometry::Circle2, TextGeometry,
                 DimensionGeometry, LabelGeometry, LeaderGeometry,
                 // 9, 10, 11: a polyline with arc segments and heights, the
                 // ellipse and elliptical arc, the spline (curves2d.hpp).
                 katana::geometry::CurvePolyline2, katana::geometry::Ellipse2,
                 katana::geometry::Spline2>;

[[nodiscard]] constexpr EntityType typeOf(const Geometry& geometry)
{
    return static_cast<EntityType>(geometry.index());
}

// ADDING A GEOMETRY KIND? READ THIS.
//
// 1. APPEND ONLY. Never insert an alternative in the middle and never reorder.
//    The variant INDEX is the on-disk kind byte (geometry_blob.cpp writes
//    `geometry.index()`), so reordering silently reinterprets every project
//    ever saved. Most shifts are caught by the payload-length checks, but two
//    kinds of equal payload size swap with no complaint at all and the drawing
//    reloads as the wrong shapes. GeometryBlobWireFormat pins the mapping.
// 2. Add the EntityType enumerator at the SAME ordinal - typeOf() casts the
//    index straight to it, so the two orders are one thing, not two.
// 3. docs/model.md lists every place that must then change. The exhaustive
//    visitors (validate, boundingBox, distanceTo, transformed, and the closed
//    visitor structs) will not compile until they are complete, which is
//    deliberate; the places that opted out of that discipline are listed there
//    because the compiler cannot help with them.
//
// This assert exists so that step 3 is not something you have to remember.
static_assert(std::variant_size_v<Geometry> == 12,
              "A geometry kind was added or removed. See docs/model.md for every place that "
              "must change - several of them fail SILENTLY, not at compile time. Update this "
              "count once you have been through the list.");

struct Color {
    std::uint8_t r = 255;
    std::uint8_t g = 255;
    std::uint8_t b = 255;
    std::uint8_t a = 255;

    // "#RRGGBB" or "#RRGGBBAA".
    [[nodiscard]] static katana::core::Result<Color> fromHex(std::string_view text);
    [[nodiscard]] std::string toHex() const; // alpha omitted when opaque

    friend constexpr bool operator==(const Color&, const Color&) = default;
};

using PropertyValue = std::variant<bool, std::int64_t, double, std::string>;
// Ordered so that iteration, serialisation and diffs are deterministic.
using PropertyMap = std::map<std::string, PropertyValue, std::less<>>;

// A property value as a person reads it: "true", "42", "31.25", the text
// itself. ONE definition, because the property panel, the command line and
// the attribute manager must not disagree about what a value says. A real
// is written with enough digits to read back as the same double, since a
// level shown as 31.2 that is actually 31.249 is a lie in survey work.
[[nodiscard]] std::string toString(const PropertyValue& value);
// "text", "integer", "real" or "boolean": what the attribute manager and
// `PROP SET ... <type>` name the four kinds.
[[nodiscard]] std::string_view typeName(const PropertyValue& value);

// Property keys that more than one part of the application agrees on.
//
// A 2D drawing has no Z of its own, so a point's height lives in a property.
// Three separate places need to say which property: the 12d import and export
// that write it, the surface builder in the application that reads it to
// triangulate a drawing, and the survey import that writes it from field data.
// They were a constant in archive12d and two string literals in the Qt layer,
// which is two chances to disagree about a name silently - and the symptom
// would be a surface built flat on the datum with no error anywhere.
//
// `kElevationProperty` is one height, for a point. `kElevationsProperty` is the
// per-vertex list for a 3D string, where a NULL entry means "not surveyed" and
// is not the same as zero.
inline constexpr std::string_view kElevationProperty = "elevation";
inline constexpr std::string_view kElevationsProperty = "elevations";

// The ONE writer and the ONE reader of those two properties. The 12d archive,
// the survey import and vector interchange all go through them: until
// 2026-09-23 there were two writers and a reader in two modules, and a third
// pair was about to be written for the GDAL formats (audit IO-01), which is
// how three spellings of "a list with a gap in it" would have begun.
//
// setHeights makes the properties describe exactly `heights`, one per vertex:
// nothing when no vertex has a height (absent is not zero), one `elevation`
// when every vertex has the same height, otherwise an `elevations` list with
// "null" where a vertex has none. A non-finite height is no height. The
// property that does not apply is removed, so a rewrite cannot leave behind a
// stale one that the reader would prefer.
void setHeights(PropertyMap& properties, const std::vector<std::optional<double>>& heights);

// The heights of `count` vertices: the `elevations` list when it has exactly
// `count` entries, else `elevation` for every vertex, else none. A list of the
// wrong length describes some other geometry - a vertex has been added or
// removed since - so it is passed over rather than stretched to fit.
[[nodiscard]] std::vector<std::optional<double>> heightsOf(const PropertyMap& properties,
                                                           std::size_t count);

struct Entity {
    EntityId id = kInvalidEntityId;
    Geometry geometry{};
    std::string layer = "0";
    std::string style{};          // empty: ByLayer
    std::optional<Color> color{}; // empty: ByLayer
    bool visible = true;
    PropertyMap properties{}; // user / application attributes
    PropertyMap metadata{};   // provenance: source file, import time, author...

    [[nodiscard]] EntityType type() const { return typeOf(geometry); }

    friend bool operator==(const Entity&, const Entity&) = default;
};

} // namespace katana::entity
