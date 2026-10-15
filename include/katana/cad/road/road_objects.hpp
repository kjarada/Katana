#pragma once

// Road objects: the furniture and structures of a road placed along an
// alignment or a drawn line - safety barriers, guide posts, signs, light
// poles, culverts, retaining and noise walls, fences and line markings
// (docs/road_objects.md). Horizontal BIM: what stands beside and crosses a
// road, never a building.
//
// An object is a DEFINITION - a kind, where it is placed, and the parameters
// a person gave - kept in the drawing as versioned JSON under the metadata
// key "road_objects" (Document::roadObjects), edited by the ROADOBJECT verbs
// as one undo step each. Its 3D objects (cad/bim/bim_object.hpp) are MADE
// from it every time, by buildRoadObject and bim::placedObjects, from the
// alignment and its design profile or a surface as they are now: move the
// alignment and the barrier follows it.
//
// Every solid is closed and outward-oriented (road_object_mesh.hpp), so the
// IFC export writes it as a closed face set and its volume is exact for the
// mesh. (The 3D view draws both faces of every triangle: the winding is for
// the export and the volume, not the display.)
//
// Heights. An object stands on the ground of its placement:
//   - Profile: along an alignment, the design profile's level at the
//     station, carried out to the object's offset at `crossfall` percent
//     (negative falls away from the centreline);
//   - Surface: the named surface of the drawing under each point, draped;
//   - Line: the heights of the line or points it was placed with.
// What cannot be placed - a station off the alignment, an alignment with no
// profile, a point off the surface, a vertex with no height - is refused by
// the verb that places it, and once placed (the alignment shortened since,
// the surface removed) the object is left out of the model and the problem
// listed (BimModel::problems), never drawn at a guessed level.

#include <cstddef>
#include <cstdint>
#include <functional>
#include <map>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <variant>
#include <vector>

#include "katana/cad/bim/bim_object.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/tables.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/geometry/primitives3d.hpp"

namespace katana::terrain {
class TinSurface;
}

namespace katana::cad {
class Document;
}

namespace katana::cad::road {

// ---- kinds --------------------------------------------------------------------------------

// Append-only: the JSON stores the word, never the number, but the dialog's
// pages and the KINDS reply follow this order.
enum class RoadObjectKind : std::uint8_t {
    Barrier,
    GuidePost,
    Sign,
    LightPole,
    Culvert,
    Wall,
    Fence,
    Marking,
};
inline constexpr std::size_t kRoadObjectKindCount = 8;

// Every kind, in declaration order.
[[nodiscard]] std::span<const RoadObjectKind> roadObjectKinds();
// "barrier", "guidepost", "sign", "light", "culvert", "wall", "fence",
// "marking": the word ROADOBJECT ADD takes, the JSON stores and
// bim::BimObject::kind says.
[[nodiscard]] std::string_view kindWord(RoadObjectKind kind);
// The word in any case, or its plural ("barriers", "guideposts", "lights",
// "markings" ...). NotFound naming the words there are.
[[nodiscard]] katana::core::Result<RoadObjectKind> kindFromWord(std::string_view word);
// "Safety barrier", "Guide posts", "Sign", "Light poles", "Culvert", "Wall",
// "Fence", "Line marking": what a person reads.
[[nodiscard]] std::string_view kindTitle(RoadObjectKind kind);

// How a kind takes its placement.
enum class KindShape {
    // Runs along it: a barrier, a wall, a fence, a line marking.
    Linear,
    // Stands at points of it: one at a station, one at each point, or one
    // every `spacing` along it - guide posts, signs, light poles.
    Point,
    // Crosses it: a culvert, across the alignment at a station, or from the
    // first point to the last.
    Crossing,
};
[[nodiscard]] KindShape shapeOf(RoadObjectKind kind);

// ---- parameters ---------------------------------------------------------------------------

enum class ParameterType : std::uint8_t { Number, Choice, Text };

struct ParameterSpec {
    std::string_view key;
    ParameterType type = ParameterType::Number;
    // "m", "deg", "%", "" for a count or a word.
    std::string_view unit;
    // What it is when not given, as a person reads it: "0.787", "wbeam",
    // "by type" (it follows another parameter), "auto" (worked out from the
    // placement).
    std::string_view defaultText;
    // A Choice's words, lower case.
    std::span<const std::string_view> choices;
    // A Number's range, inclusive.
    double minimum = 0.0;
    double maximum = 0.0;
    std::string_view description;
    // A Number that is a count: a whole number.
    bool whole = false;
};

// The parameters `kind` takes beyond the ones every object takes (below), in
// the order the dialog shows them and KINDS lists them.
[[nodiscard]] std::span<const ParameterSpec> parametersOf(RoadObjectKind kind);
// nullptr for a key `kind` does not take.
[[nodiscard]] const ParameterSpec* findParameter(RoadObjectKind kind, std::string_view key);
// Where the kind's defaults come from: the standard, or "typical", said.
[[nodiscard]] std::string_view kindSource(RoadObjectKind kind);

using ParameterValue = std::variant<double, std::string>;
using Parameters = std::map<std::string, ParameterValue, std::less<>>;

// `text` read as `key` of `kind`: a number in range, a choice in the list
// (folded to lower case), a text as it is. NotFound for a key the kind does
// not take; ParseFailure or InvalidArgument naming the key and the value.
[[nodiscard]] katana::core::Result<ParameterValue>
parseParameter(RoadObjectKind kind, std::string_view key, std::string_view text);
// A value as a reply or a line says it back: a number exactly, a word as it is.
[[nodiscard]] std::string parameterText(const ParameterValue& value);

// ---- the definition -----------------------------------------------------------------------

enum class PlacementKind : std::uint8_t {
    // ALIGNMENT <name> station= [to=] offset= [spacing=]
    Alignment,
    // ALONG a drawn line, its vertices copied when it was placed
    Line,
    // AT points
    Points,
};
[[nodiscard]] std::string_view placementWord(PlacementKind kind); // "alignment", "line", "points"

enum class HeightSource : std::uint8_t { Profile, Surface, Line };
[[nodiscard]] std::string_view heightWord(HeightSource source); // "profile", "surface", "line"
[[nodiscard]] katana::core::Result<HeightSource> heightFromWord(std::string_view word);

struct PlacedVertex {
    katana::geometry::Point2 position;
    // Absent where the line or the point had no height: absent is not zero.
    std::optional<double> z{};

    friend bool operator==(const PlacedVertex&, const PlacedVertex&) = default;
};

struct RoadObjectPlacement {
    PlacementKind kind = PlacementKind::Alignment;
    // Alignment: by name, looked up each time the object is made.
    std::string alignment{};
    double station = 0.0;
    // The end station of a linear object, or of a run of point objects.
    std::optional<double> to{};
    // Left of the centreline is positive, looking along increasing station.
    double offset = 0.0;
    // Line, Points: the vertices, copied in when the object was placed - a
    // line deleted or edited afterwards leaves the object where it was.
    std::vector<PlacedVertex> vertices{};
    // Line: the entity they were copied from, for a person; 0 for none.
    katana::entity::EntityId source = katana::entity::kInvalidEntityId;
    // A point kind: one every `spacing` from the start to `to` (or along the
    // line). Not given: one at the station, or one at each vertex.
    std::optional<double> spacing{};
    // Not given: Profile along an alignment and Line otherwise - Surface
    // when a surface is named (heightSourceOf).
    std::optional<HeightSource> heights{};
    std::string surface{};
    // Percent, from the centreline out: the profile's level at |offset| is
    // the level at the centreline plus |offset| x crossfall / 100.
    double crossfall = 0.0;
    // Raised (or lowered, negative) from the ground the heights give.
    double dz = 0.0;

    friend bool operator==(const RoadObjectPlacement&, const RoadObjectPlacement&) = default;
};

struct RoadObject {
    // "barrier-3": the kind's word and the set's next number, never reused,
    // so a reply, the IFC GlobalId and a person's notes name it alike.
    std::string id{};
    RoadObjectKind kind = RoadObjectKind::Barrier;
    // Empty: the kind's title and the id.
    std::string name{};
    // Empty: road/objects/<kind word>, made in the same undo step when the
    // drawing lacks it.
    std::string layer{};
    // Absent: the kind's colour.
    std::optional<katana::entity::Color> colour{};
    // Empty: the kind's (and its type's) material.
    std::string material{};
    // The road it belongs to - written as the IfcRoad it is part of. Empty:
    // an object of the site.
    std::string facility{};
    RoadObjectPlacement placement{};
    // Only what was given; the rest are the kind's defaults when it is made,
    // so changing a barrier's type= gives the new type's dimensions rather
    // than keeping the old one's.
    Parameters parameters{};

    friend bool operator==(const RoadObject&, const RoadObject&) = default;
};

struct RoadObjectSet {
    std::vector<RoadObject> objects{};
    // The number the next object's id takes.
    std::uint64_t nextNumber = 1;

    // Case-insensitive; nullptr when there is none.
    [[nodiscard]] const RoadObject* find(std::string_view id) const;
    [[nodiscard]] RoadObject* find(std::string_view id);
    // A new id for `kind`, taking the next number.
    [[nodiscard]] std::string takeId(RoadObjectKind kind);

    friend bool operator==(const RoadObjectSet&, const RoadObjectSet&) = default;
};

// The source the heights of `placement` come from.
[[nodiscard]] HeightSource heightSourceOf(const RoadObjectPlacement& placement);
// The layer, material, colour and display name `object` is made with.
[[nodiscard]] std::string layerOf(const RoadObject& object);
[[nodiscard]] std::string materialOf(const RoadObject& object);
[[nodiscard]] katana::entity::Color colourOf(const RoadObject& object);
[[nodiscard]] std::string displayName(const RoadObject& object);

// Every parameter of the kind with the value it is made with: the given one,
// else the default for the object's kind and type. "auto" ones are absent.
[[nodiscard]] Parameters effectiveParameters(const RoadObject& object);

// One key=value onto `object`: the keys every object takes - name, layer,
// colour (#RRGGBB or a standard colour name; empty for the kind's), material,
// facility - the placement's - alignment (an alignment placement's), station,
// to, offset, spacing, heights, surface, crossfall, dz (to=, spacing= and
// heights= empty clear them; offset=, crossfall= and dz= empty go back to 0) -
// and the kind's own (parametersOf; empty clears it back to the default).
// Refused, naming the key: a key neither takes, a value that does not read,
// station=/to=/offset=/alignment= on a placement that is not along an
// alignment, spacing= on a kind that is not a point kind or a placement at
// points, crossfall= on a placement that is not along an alignment, and
// station= or alignment= given empty (a placement along an alignment has to
// say where).
[[nodiscard]] katana::core::Status applyKey(RoadObject& object, std::string_view key,
                                            std::string_view value);

// ---- making the objects -------------------------------------------------------------------

// What making an object looks up: an alignment by name, a surface by name.
// Either may answer nullptr.
struct PlacementContext {
    std::function<const katana::entity::Alignment*(std::string_view)> alignment;
    std::function<const katana::terrain::TinSurface*(std::string_view)> surface;
};
// The document's alignments and surfaces (Document::surfaces).
[[nodiscard]] PlacementContext placementContext(const Document& document);

// The 3D objects of one road object, in order, or nothing and the problem
// that stopped it (source "object <id>"). Deterministic: the same definition
// in the same drawing gives the same objects, ids and all.
[[nodiscard]] bim::BimModel buildRoadObject(const PlacementContext& context,
                                            const RoadObject& object);

// Why `parts` - the solids a maker gave for one object - are not an object, as
// the sentence buildRoadObject lists it with; nullopt when there is at least one
// part and every part has faces. No part at all is a placement that gives
// nowhere to stand the object; a part with no faces is a section that did not
// triangulate or a sweep of a single point, which would be counted and written
// to IFC as an element with no shape. A function of plain data, so a test gives
// it the empty part that no maker makes any more and sees the sentence.
[[nodiscard]] std::optional<std::string> shapeProblem(std::span<const bim::BimObject> parts);

// What an edit asks of `built`, buildRoadObject's answer for `object`, before it
// stores the definition: a definition the model would only list as a problem is
// never stored. InvalidArgument naming the first problem it was left with (and,
// for an object placed along a line, the line: "barrier-3 (#12)"), or the part
// that is not a closed solid (isClosedMesh, road_object_mesh.hpp) - the IFC
// export writes each solid as a closed face set and its volume is read off the
// triangles, so one that does not close is refused here, once per edit, and not
// checked on every rebuild of the model. Also plain data in, so a test hands it
// the open mesh no maker makes any more.
[[nodiscard]] katana::core::Status checkBuilt(const RoadObject& object,
                                              const bim::BimModel& built);

// The quantities of `objects` together, by name in the order first met: the
// volumes and weights summed, a height, width or depth the largest. A Length,
// a Count and a GrossArea are those of the objects themselves, not of what
// stands under or hangs from them: a wall's footing (IfcFooting) and a light
// pole's luminaire (IfcLightFixture) say their own, as IFC reads them off the
// element, but add nothing here - a 50 m wall is 50 m long however deep its
// footing, and three poles are three however many lanterns they carry.
[[nodiscard]] std::vector<bim::Quantity> totalQuantities(std::span<const bim::BimObject> objects);

// A culvert's two ends, inlet first, at its inverts: where buildRoadObject
// puts the barrel, for a reply and a test. InvalidArgument for any other
// kind; whatever the placement cannot give otherwise.
struct CulvertEnds {
    katana::geometry::Point3 inlet;
    katana::geometry::Point3 outlet;
};
[[nodiscard]] katana::core::Result<CulvertEnds> culvertEnds(const PlacementContext& context,
                                                            const RoadObject& object);

// ---- JSON (the drawing's "road_objects" metadata, ROADOBJECT LIST JSON) ---------------------

// Version 1. A newer version is refused with Unsupported, never read by
// guesswork.
inline constexpr int kRoadObjectsVersion = 1;
[[nodiscard]] katana::core::Result<std::string> roadObjectsToJson(const RoadObjectSet& set);
[[nodiscard]] katana::core::Result<RoadObjectSet> roadObjectsFromJson(std::string_view text);

} // namespace katana::cad::road
