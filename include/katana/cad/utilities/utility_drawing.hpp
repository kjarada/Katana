#pragma once

// A graded utility schedule as drawing (docs/subsurface_utilities.md,
// "Drawing the services"): what UTILITY DRAW adds to a document.
//
// The survey library grades located services by AS 5488 quality level
// (katana/survey/subsurface); this turns the grading into layers, linetypes
// and entities a person can see, plot and query. It is split in two so that
// each half can be tested on its own:
//
//   drawUtilities       the schedule -> a UtilityDrawing: every layer,
//                       linetype and entity the drawing needs, as values.
//                       Pure: it sees no document.
//   utilityDrawCommand  the drawing -> ONE command for a model: what the
//                       model lacks is created, what it has is reused.
//
// A service is drawn as one polyline per maximal run of consecutive segments
// graded at the same level, so the plan shows exactly where the level
// changes, on "<prefix>/<type>/QL-A" .. "QL-D", and one point per located
// vertex on "<prefix>/<type>/points". The quality level is in the layer's
// linetype - QL-A continuous, then dashed, dash-dot and dotted as the
// evidence weakens - so it survives a monochrome plot; the kind of service
// is in the layer's colour. Everything the grading found is on the entities
// as "utility.*" properties, so an agent or the property panel can ask why a
// stretch is QL-C without re-running the report.

#include <array>
#include <cstddef>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/commands/command.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/model.hpp"
#include "katana/entity/tables.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/survey/subsurface/utility_network.hpp"

namespace katana::cad::utilities {

// The layer every drawing goes under unless LAYER names another.
inline constexpr std::string_view kDefaultUtilityLayerPrefix = "utilities";

// The undo step's name: what Edit > Undo and the history say.
inline constexpr std::string_view kUtilityDrawStep = "UTILITY DRAW";

// The properties a drawn service carries. One place for the names, since the
// window, an agent and the tests all read them. A property is absent when
// its value was not recorded or cannot be computed: absent is not zero. Two
// are always there when their entity is: the type, which names the layer
// ("unknown" when not recorded), and a level's reference, which the level is
// read on (see kLevelReference).
namespace keys {
// On both the polylines and the points.
inline constexpr std::string_view kLine = "utility.line";   // the schedule's line id
inline constexpr std::string_view kType = "utility.type";   // utilityTypeWord
// The level the evidence supports: the run's level on a polyline, the
// vertex's own on a point. "QL-A" .. "QL-D".
inline constexpr std::string_view kQualityLevel = "utility.quality_level";
// On a polyline: the service and the run.
inline constexpr std::string_view kOwner = "utility.owner";
inline constexpr std::string_view kMaterial = "utility.material";
inline constexpr std::string_view kDiameter = "utility.diameter"; // metres
inline constexpr std::string_view kDiameterInside = "utility.diameter_inside";
inline constexpr std::string_view kConfiguration = "utility.configuration";
inline constexpr std::string_view kDescription = "utility.description";
// "in service", "disused", "abandoned", "proposed"; absent when the schedule
// gives none, which the report lists as a missing attribute.
inline constexpr std::string_view kStatus = "utility.status";
// Why the run is below the better of its segments' ends, each distinct
// reason once, joined by "; ".
inline constexpr std::string_view kLimitedBy = "utility.limited_by";
inline constexpr std::string_view kLength = "utility.length"; // the run's plan length, metres
inline constexpr std::string_view kFrom = "utility.from";     // the run's first vertex id
inline constexpr std::string_view kTo = "utility.to";         // and its last
// On a point: the vertex.
inline constexpr std::string_view kVertex = "utility.vertex";
inline constexpr std::string_view kMethod = "utility.method";
inline constexpr std::string_view kClaimed = "utility.claimed";
inline constexpr std::string_view kOverClaim = "utility.over_claim";
inline constexpr std::string_view kServiceLevel = "utility.service_level";
// With a service level, always: the part of the service it is on, "top",
// "centre", "invert" or "unknown" - the reference the grading and the cover
// took it on. A level given with no reference is read as on the top
// (parseUtilityCsv), so "top" is also what an unrecorded one reads as: the
// parsed schedule does not keep the difference.
inline constexpr std::string_view kLevelReference = "utility.level_ref";
inline constexpr std::string_view kLevelQualified = "utility.level_qualified";
inline constexpr std::string_view kSurfaceLevel = "utility.surface_level";
inline constexpr std::string_view kCover = "utility.cover";
inline constexpr std::string_view kCoverNote = "utility.cover_note";
inline constexpr std::string_view kCoverBelowMinimum = "utility.cover_below_minimum";
inline constexpr std::string_view kVerifies = "utility.verifies";
// A delivery schema's attributes that nothing interprets, kept by their own
// name after this prefix: "utility.field.AssetStatus".
inline constexpr std::string_view kFieldPrefix = "utility.field.";
} // namespace keys

// "water", "electricity", "telecommunications", "gas", "recycled-water",
// "fire-service", "sewer", "stormwater", "fuel", "its", "other", "unknown":
// one lower-case word per kind, for a layer name and a reply, where the
// report's "recycled water" would need quoting.
[[nodiscard]] std::string_view utilityTypeWord(katana::survey::subsurface::UtilityType type);

// The colour a kind of service's layers are made in. Katana's defaults,
// following the colours Australian locators commonly mark services in -
// AS 5488 classifies information and sets no colours - adjusted so each reads
// on the dark plan view AND on white paper: yellow gas is drawn dark gold and
// cream sewer a darker cream, since the marking colours vanish on paper.
// Telecommunications is white, which the plot prints black (PlotSettings::
// whiteToBlack). docs/subsurface_utilities.md has the table. Only a layer the
// drawing creates takes it; a person's own colour on an existing layer stays.
[[nodiscard]] katana::entity::Color
utilityTypeColour(katana::survey::subsurface::UtilityType type);

// The linetype a quality level's layers are drawn in: "continuous" for QL-A,
// "utility-ql-b", "utility-ql-c" and "utility-ql-d" for the others.
[[nodiscard]] std::string_view
qualityLevelLinetypeName(katana::survey::subsurface::QualityLevel level);

// That linetype's definition, in model metres for plans at 1:200 to 1:500:
// QL-B dashed (1.5 dash, 0.75 gap), QL-C dash-dot (1.5 dash, 0.5 gap, dot,
// 0.5 gap), QL-D dotted (dot, 0.6 gap). At 1:500 a QL-B dash is 3 mm on
// paper; at 1:200, 7.5 mm. QL-A's is the built-in continuous line.
[[nodiscard]] katana::entity::Linetype
qualityLevelLinetype(katana::survey::subsurface::QualityLevel level);

// "<prefix>/<type>/QL-B", and "<prefix>/<type>/points".
[[nodiscard]] std::string qualityLevelLayerName(std::string_view prefix,
                                                katana::survey::subsurface::UtilityType type,
                                                katana::survey::subsurface::QualityLevel level);
[[nodiscard]] std::string pointsLayerName(std::string_view prefix,
                                          katana::survey::subsurface::UtilityType type);

struct UtilityDrawOptions {
    // The grading, as REPORT grades: SPACING sets maximumDetectedSpacing.
    katana::survey::subsurface::GradingSettings grading{};
    std::string layerPrefix{kDefaultUtilityLayerPrefix};
    // When given, each point whose cover is computed says whether it is below
    // this (keys::kCoverBelowMinimum).
    std::optional<double> minimumCover{};
};

// One service as drawn, for the reply.
struct DrawnUtilityLine {
    std::string id;
    katana::survey::subsurface::UtilityType type = katana::survey::subsurface::UtilityType::Unknown;
    double length = 0.0;
    // Plan length at each level, indexed by static_cast<int>(QualityLevel).
    std::array<double, 4> lengthAt{};
    std::size_t polylines = 0;
};

struct UtilityDrawing {
    // Every layer the entities are on and every ancestor of one, parents
    // first, as the drawing would create them.
    std::vector<katana::entity::Layer> layers;
    // The linetypes those layers name, other than "continuous".
    std::vector<katana::entity::Linetype> linetypes;
    // Per service in schedule order: its runs, then its points. Ids unset.
    std::vector<katana::entity::Entity> entities;
    std::vector<DrawnUtilityLine> lines;
    std::size_t vertices = 0;
    std::size_t segments = 0;
    // How many distinct layers hold an entity: created or reused, not the
    // ancestors that only group them.
    std::size_t drawnLayers = 0;
    // Every located vertex.
    katana::geometry::Box2 bounds{};
};

// The drawing of `lines`, graded with `options.grading`. All or nothing:
// InvalidArgument naming the line when any line cannot be graded (fewer than
// two vertices, a non-finite coordinate); naming the prefix when it is not a
// layer path, or leaves a layer drawn under it too deep or too long for the
// model; and when there are no lines at all. A run of no plan length to the
// model's tolerance - two records at one place, or a rounding error apart,
// graded unlike their neighbours - has no polyline to draw; its vertices are
// still drawn as points and its segments still counted. So a drawing this
// returns holds nothing utilityDrawCommand's model would refuse as geometry
// or as a layer name.
[[nodiscard]] katana::core::Result<UtilityDrawing>
drawUtilities(const std::vector<katana::survey::subsurface::UtilityLine>& lines,
              const UtilityDrawOptions& options = {});

// `drawing` added to `model` as ONE command named kUtilityDrawStep: the
// linetypes the model lacks that a layer to be created names, the layers it
// lacks (parents first, so undo takes every one back), then the entities. A
// layer or linetype the model already has is reused as it is - its colour,
// linetype and lock are the person's - so an existing locked layer refuses
// the whole step when it runs.
[[nodiscard]] katana::commands::CommandPtr utilityDrawCommand(const katana::entity::Model& model,
                                                              const UtilityDrawing& drawing);

// The reply UTILITY DRAW gives, one record a line:
//   utilities drawn lines=4 vertices=14 segments=10 entities=21 layers=11 bounds=x0,y0,x1,y1
//   line id=W1 type=water length=30.024 ql_a=1.420 ql_b=16.102 ql_c=12.502 ql_d=0.000
// Metres and coordinates to three decimals; an id with a blank, '"' or '='
// in quotes.
[[nodiscard]] std::string formatUtilityDrawing(const UtilityDrawing& drawing);

} // namespace katana::cad::utilities
