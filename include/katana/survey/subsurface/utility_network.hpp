#pragma once

// Buried services as surveyed lines, graded by AS 5488.1 quality level.
//
// A UtilityLine is one service run - a water main, a duct bank, a gas service
// - as the vertices the survey located, in order along the run. Each vertex
// carries the evidence it was located with and is classified on its own
// (quality_level.hpp). What this file adds is the grade of the path BETWEEN
// vertices, which is where most over-claiming happens: a line drawn through
// two potholes is not QL-A between them, because nobody saw the service there.
//
// Conventions follow survey/data_model.hpp: metres, (northing, easting), and
// "absent is not zero" - a vertex with no level is a vertex whose level nobody
// measured, and a surface level of 0.0 is a real surface at the datum.

#include <array>
#include <cstddef>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/survey/coordinate.hpp"
#include "katana/survey/subsurface/quality_level.hpp"

namespace katana::survey::subsurface {

// The kinds of service the standard's attribute schedule distinguishes (AS
// 5488.2 Table A.4, which delivery schemas such as TfNSW's code by a letter).
// Other is a service whose kind is stated but not listed here; Unknown is one
// whose kind nobody established, which is a finding in its own right.
enum class UtilityType {
    Unknown,
    Electricity,
    Telecommunications,
    Gas,
    Water,
    RecycledWater,
    FireService,
    Sewer,
    Stormwater,
    Fuel,
    IntelligentTransport, // ITS: traffic signals, cameras, loops, signalling
    Other,
};

[[nodiscard]] const char* toString(UtilityType type);

// Enumerator names and common abbreviations ("elec", "power", "comms", "telco",
// "nbn", "sw", "storm", "ww", "sewerage", "reuse", "oil" ...), the asset type
// names of AS 5488.2 ("Communication", "Drainage", "Fire Service", "ITS",
// "Petroleum", "Not Specified") and their one-letter codes (C D E F G I P S W
// N), any letter case. nullopt for text that names nothing, so that a typo is
// an error and not an Unknown service.
[[nodiscard]] std::optional<UtilityType> parseUtilityType(std::string_view text);

enum class UtilityStatus {
    Unknown,
    InService,
    Disused,   // out of use, still the owner's and possibly still charged
    Abandoned, // given up by its owner
    Proposed,
};

[[nodiscard]] const char* toString(UtilityStatus status);
[[nodiscard]] std::optional<UtilityStatus> parseUtilityStatus(std::string_view text);

// Cells as one file spelt them, by that file's header: provenance for a
// deliverable that must repeat a schedule's own words (UtilityAttributes::
// written), not part of what a line IS. Two lines that say the same thing -
// one read from a schedule, one from what was drawn, or the same schedule
// written back in Katana's own spelling - are the same line, so this map
// takes no part in the equality of what holds it: it is always equal. Its
// entries are compared, where that matters, as the map it is.
struct WrittenCells : std::map<std::string, std::string> {
    using std::map<std::string, std::string>::map;
    friend bool operator==(const WrittenCells&, const WrittenCells&) { return true; }
};

// The descriptive attributes the standard asks to be recorded with a service.
// Empty strings are "not recorded"; a zero diameter is "not recorded" too,
// because no service has none.
struct UtilityAttributes {
    UtilityType type = UtilityType::Unknown;
    UtilityStatus status = UtilityStatus::Unknown;
    std::string owner;         // asset owner / operator
    std::string material;      // "PVC", "DICL", "copper" ...
    double diameter = 0.0;     // size of the service across, metres; see below
    std::string configuration; // "4 x 100 conduits", "2 x 2", "Single" ...
    std::string description;
    // True when `diameter` is an INSIDE dimension - a delivery schema's pipe
    // size usually is (TfNSW's Size is the inside diameter, a culvert's inside
    // width by height). The top of the service found from an invert or a centre
    // is then the inside top, below the outside by the wall, and a cover
    // computed from it is larger than the real one by that wall. Reports say so
    // (depthOfCover's note); false means an outside dimension.
    bool diameterIsInside = false;
    // Attributes a delivery schema carries that nothing here interprets -
    // subtype, feature, capacity, limitation, condition, clash, treatment ... -
    // by the schema's own attribute name. Kept so that they can be checked
    // (delivery_schema.hpp) and reported, never dropped.
    std::map<std::string, std::string> fields;
    // The attributes that ARE interpreted above - the service's id, type,
    // owner, material, size, status, configuration, description - exactly as
    // the schedule wrote them, by the header they were written under:
    // "AssetTypeCode" -> "W", "AssetStatus" -> "In service". Reading turns
    // "In service" into UtilityStatus::InService; a deliverable written back
    // out (the IFC export's delivery property set, docs/ifc.md) must carry
    // the schedule's own value, misspelling and all, or it would quietly
    // correct what `UTILITY CHECK` reports as wrong. Empty for a line built
    // in code.
    WrittenCells written;

    // Cells the schedule reader reads as "not recorded", or reads only in
    // part, kept as the schedule wrote them by the reader's column name: a
    // "type" that names no kind ("Not Specified", "N"), a "status" of
    // "Unknown", and any "size" that a diameter written back would not say
    // again - "Not Applicable", "Unknown", "1200 x 900", or a size given
    // beside diameter_mm. So a schedule written back (writeUtilityCsv) says
    // what this one said and meets a delivery schema this one met. Nothing
    // grades from them; the writer takes one back only while it still reads
    // as the value held above, so an edit made since wins.
    std::map<std::string, std::string> recorded;

    friend bool operator==(const UtilityAttributes&, const UtilityAttributes&) = default;
};

// Which part of the service a recorded level is ON. A level without this is
// ambiguous by up to a pipe diameter, which on a 600 mm main is more than
// QL-B's whole vertical tolerance.
enum class LevelReference {
    Top,     // top of the service: crown, top of pipe or cable, or the first
             // thing above it that is met - an encasement, a cover slab
    Centre,  // centre line
    Invert,  // bottom of the bore
    Unknown, // stated as unknown or "other": the level cannot be placed on the
             // service, so no top and no cover are computed from it
};

[[nodiscard]] const char* toString(LevelReference reference);
// "top", "crown", "obvert", "centre", "invert", "unknown" and the Depth
// Location names of AS 5488.2 as delivery schemas spell them: "Top of Pipe",
// "Obvert", "Top of Concrete Encasement", "Plastic Cover Protection
// Encountered" and "Ground Level" are Top, "Top Row Invert" is Invert,
// "Other" is Unknown.
[[nodiscard]] std::optional<LevelReference> parseLevelReference(std::string_view text);

struct UtilityVertex {
    std::string id;
    Coordinate2 position;
    std::optional<double> level; // of the service, on `levelReference`
    LevelReference levelReference = LevelReference::Top;
    std::optional<double> surfaceLevel; // finished surface directly above
    // Depth of the `levelReference` point below the surface, metres, as a
    // delivery schema records it instead of (or beside) a level. With a
    // surface level it gives a level (serviceLevel); alone it still gives a
    // cover. Where a level and a depth are both given the level is used.
    std::optional<double> depth;
    PositionEvidence evidence;
    // The level the deliverable CLAIMS for this vertex, when it states one.
    // Checked against what the evidence supports; nullopt means "grade it".
    std::optional<QualityLevel> claimed;
    // For an exposure made to check a detection: the id of the detected vertex
    // it checks (verification.hpp). Empty for any other vertex.
    std::string verifies;
    // Per-vertex attributes of a delivery schema that nothing here interprets
    // (a depth description, the date obtained, a pothole report), by the
    // schema's attribute name.
    std::map<std::string, std::string> fields;
    // The vertex's interpreted cells - point, coordinates, method, level,
    // level reference, depth, surface, uncertainties, claimed level, path,
    // verifies - as written, by header: "LocateMethod" -> "Electronic
    // Detection". See UtilityAttributes::written for why.
    WrittenCells written;

    // As UtilityAttributes::recorded, for a vertex's cells: a "ql" of
    // "Unknown" (no claim), and a "level_ref" given with no level and no
    // depth that reads as the top - which an empty cell reads as too.
    std::map<std::string, std::string> recorded;

    friend bool operator==(const UtilityVertex&, const UtilityVertex&) = default;
};

// The level of the recorded point on the service: `level`, or else surface
// level minus depth. nullopt when neither can be had.
[[nodiscard]] std::optional<double> serviceLevel(const UtilityVertex& vertex);

// True when anything vertical was measured at the vertex - a level, or a depth.
[[nodiscard]] bool hasVerticalMeasurement(const UtilityVertex& vertex);

// What is known about the service between one vertex and the next.
enum class PathEvidence {
    Detected, // traced continuously, or detected at intervals no longer than the
              // project's maximum spacing
    Exposed,  // seen along its whole length: an open trench
    Assumed,  // joined up from the ends and the records; nothing observed between
};

[[nodiscard]] const char* toString(PathEvidence evidence);

struct UtilityLine {
    std::string id;
    UtilityAttributes attributes;
    std::vector<UtilityVertex> vertices; // in order along the run
    // One per segment, i.e. vertices.size() - 1 entries; empty means every
    // segment is Detected. Any other size is an InvalidArgument.
    std::vector<PathEvidence> pathEvidence;

    // Field by field, doubles exactly: what a schedule written from the
    // drawing and read back is held to (writeUtilityCsv, utility_csv.hpp).
    friend bool operator==(const UtilityLine&, const UtilityLine&) = default;
};

struct GradingSettings {
    QualityLevelTolerances tolerances;
    // The longest segment a Detected path may span and keep QL-B. Set by the
    // project specification, not by the standard: a line interpolated across a
    // gap longer than this was not traced, it was assumed. Infinity switches the
    // rule off.
    double maximumDetectedSpacing = 10.0;
};

struct GradedVertex {
    Classification classification;
    // Set when `claimed` is better than the evidence supports: the deliverable
    // over-states this vertex. Empty otherwise.
    std::string overClaim;
};

struct GradedSegment {
    std::size_t from = 0; // vertex index; the segment runs to from + 1
    double length = 0.0;  // plan length, metres
    QualityLevel level = QualityLevel::D;
    std::string limitedBy; // why the segment is below the better of its ends
};

struct GradedLine {
    std::vector<GradedVertex> vertices;
    std::vector<GradedSegment> segments;
    // Plan length at each level, indexed by static_cast<int>(QualityLevel).
    std::array<double, 4> lengthAt{};

    [[nodiscard]] double length() const;
};

// Grades every vertex and every segment of `line`. A segment is never better
// than the worse of its two ends, and further:
//   * Exposed keeps that level;
//   * Detected is capped at QL-B, and at QL-C when longer than
//     settings.maximumDetectedSpacing;
//   * Assumed is capped at QL-C.
// InvalidArgument for a line of fewer than two vertices, a non-finite
// coordinate, or a pathEvidence list of the wrong size.
[[nodiscard]] core::Result<GradedLine> gradeLine(const UtilityLine& line,
                                                 const GradingSettings& settings = {});

// How far the top of the service is above the point a level or depth was
// recorded on: 0 for Top, half the diameter for Centre, the diameter for
// Invert. nullopt for Unknown, or for Centre and Invert without a diameter.
[[nodiscard]] std::optional<double> topOffset(LevelReference reference, double diameter);

// The level of the TOP of the service at a vertex (serviceLevel plus
// topOffset). With an outside diameter, from an invert the diameter is added
// whole, which ignores the wall and so places the top slightly high - the
// direction that under-states cover. With an inside diameter it places the
// top at the inside top, below the outside by the wall
// (UtilityAttributes::diameterIsInside). nullopt when either is unknown.
[[nodiscard]] std::optional<double> topLevel(const UtilityVertex& vertex, double diameter);

struct CoverResult {
    std::string vertexId;
    std::optional<double> cover; // surface level - top of service; nullopt if not computable
    std::string note;            // why cover is missing, or why it is not to be relied on
    bool belowMinimum = false;   // cover < the minimum asked for
};

// Depth of cover at every vertex of `line`: surface level minus top of
// service, or, where only a depth was recorded, that depth less topOffset.
// A cover computed from a level that is not qualified (Classification::
// levelQualified) is reported with a note, because the number is only as good
// as the level under it, and so is one found from an inside diameter below
// the top. `minimumCover`, when given, flags each vertex whose cover is below
// it.
[[nodiscard]] core::Result<std::vector<CoverResult>>
depthOfCover(const UtilityLine& line, std::optional<double> minimumCover = std::nullopt,
             const GradingSettings& settings = {});

} // namespace katana::survey::subsurface
