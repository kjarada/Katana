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
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/survey/coordinate.hpp"
#include "katana/survey/subsurface/quality_level.hpp"

namespace katana::survey::subsurface {

// The kinds of service the standard's attribute schedule distinguishes. Other
// is a service whose kind is stated but not listed here; Unknown is one whose
// kind nobody established, which is a finding in its own right.
enum class UtilityType {
    Unknown,
    Electricity,
    Telecommunications,
    Gas,
    Water,
    RecycledWater,
    Sewer,
    Stormwater,
    Fuel,
    Other,
};

[[nodiscard]] const char* toString(UtilityType type);

// Enumerator names and common abbreviations ("elec", "power", "comms", "telco",
// "nbn", "sw", "storm", "ww", "sewerage", "reuse", "oil" ...), any letter case.
// nullopt for text that names nothing, so that a typo is an error and not an
// Unknown service.
[[nodiscard]] std::optional<UtilityType> parseUtilityType(std::string_view text);

enum class UtilityStatus {
    Unknown,
    InService,
    Abandoned,
    Proposed,
};

[[nodiscard]] const char* toString(UtilityStatus status);
[[nodiscard]] std::optional<UtilityStatus> parseUtilityStatus(std::string_view text);

// The descriptive attributes the standard asks to be recorded with a service.
// Empty strings are "not recorded"; a zero diameter is "not recorded" too,
// because no service has none.
struct UtilityAttributes {
    UtilityType type = UtilityType::Unknown;
    UtilityStatus status = UtilityStatus::Unknown;
    std::string owner;         // asset owner / operator
    std::string material;      // "PVC", "DICL", "copper" ...
    double diameter = 0.0;     // outside diameter or width of the service, metres
    std::string configuration; // "4 x 100 conduits", "direct buried" ...
    std::string description;
};

// Which part of the service a recorded level is ON. A level without this is
// ambiguous by up to a pipe diameter, which on a 600 mm main is more than
// QL-B's whole vertical tolerance.
enum class LevelReference {
    Top,    // top of the service: crown, obvert of a duct, top of cable
    Centre, // centre line
    Invert, // bottom of the bore
};

[[nodiscard]] const char* toString(LevelReference reference);
[[nodiscard]] std::optional<LevelReference> parseLevelReference(std::string_view text);

struct UtilityVertex {
    std::string id;
    Coordinate2 position;
    std::optional<double> level; // of the service, on `levelReference`
    LevelReference levelReference = LevelReference::Top;
    std::optional<double> surfaceLevel; // finished surface directly above
    PositionEvidence evidence;
    // The level the deliverable CLAIMS for this vertex, when it states one.
    // Checked against what the evidence supports; nullopt means "grade it".
    std::optional<QualityLevel> claimed;
    // For an exposure made to check a detection: the id of the detected vertex
    // it checks (verification.hpp). Empty for any other vertex.
    std::string verifies;
};

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

// The level of the TOP of the service at a vertex, from whatever part of it
// the level was recorded on. From an invert the diameter is added whole, which
// ignores the wall and so places the top slightly high - the direction that
// under-states cover, never over-states it. nullopt without a level, or when
// a Centre or Invert level has no diameter to convert with.
[[nodiscard]] std::optional<double> topLevel(const UtilityVertex& vertex, double diameter);

struct CoverResult {
    std::string vertexId;
    std::optional<double> cover; // surface level - top of service; nullopt if not computable
    std::string note;            // why cover is missing, or why it is not to be relied on
    bool belowMinimum = false;   // cover < the minimum asked for
};

// Depth of cover at every vertex of `line`: surface level minus top of service.
// A cover computed from a level that is not qualified (Classification::
// levelQualified) is reported with a note, because the number is only as good
// as the level under it. `minimumCover`, when given, flags each vertex whose
// cover is below it.
[[nodiscard]] core::Result<std::vector<CoverResult>>
depthOfCover(const UtilityLine& line, std::optional<double> minimumCover = std::nullopt,
             const GradingSettings& settings = {});

} // namespace katana::survey::subsurface
