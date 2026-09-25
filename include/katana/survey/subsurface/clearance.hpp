#pragma once

// Clearance of proposed works from located services, with each service
// widened by the positional tolerance of its quality level.
//
// This is the question a subsurface utility investigation exists to answer -
// "can we build this here?" - and the reason the quality level matters. A
// service shown 400 mm from a proposed pipe is clear if it is QL-A (+/-50 mm),
// may or may not be clear if it is QL-B (+/-300 mm), and says nothing at all
// if it is QL-C or QL-D, whose drawn position is not a measurement. So the
// answer has four values, not two, and the third and fourth tell the designer
// which services to pothole next.

#include <optional>
#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/survey/coordinate.hpp"
#include "katana/survey/subsurface/utility_network.hpp"

namespace katana::survey::subsurface {

struct DesignVertex {
    Coordinate2 position;
    std::optional<double> level; // of the centre line of the proposed works
};

// Proposed works as a centre line: a pipe, a pile row, a trench.
struct DesignAlignment {
    std::string id;
    std::vector<DesignVertex> vertices;
    // Half the width of the works: a pipe's outside radius, half a trench's
    // width. Used horizontally and, for the level, vertically.
    double halfWidth = 0.0;
};

// The clearance the asset owners require. Not from AS 5488, which classifies
// information and sets no separations: these come from each owner's rules.
struct ClearanceRequirement {
    double horizontal = 0.3; // metres, face to face
    double vertical = 0.3;   // metres, face to face
    // A QL-C or QL-D service within this plan distance of the works (beyond the
    // horizontal clearance) is Unconfirmed: its drawn position is not a
    // measurement, so nothing short of this margin can be called clear.
    double unverifiedMargin = 2.0;
};

enum class ClearanceStatus {
    Conflict,        // closer than required even at the drawn position
    Unconfirmed,     // QL-C / QL-D near the works: locate it before relying on it
    WithinTolerance, // clear at the drawn position, not across the tolerance
    Clear,           // clear everywhere the service could be
};

[[nodiscard]] const char* toString(ClearanceStatus status);

struct ClearanceResult {
    std::string utilityId;
    std::size_t segment = 0; // index of the segment's first vertex
    std::string fromId;      // the segment's vertices
    std::string toId;
    QualityLevel level = QualityLevel::D;
    ClearanceStatus status = ClearanceStatus::Clear;
    Coordinate2 nearest;        // on the utility, where the gap is least
    double planDistance = 0.0;  // centre line to centre line
    double horizontalGap = 0.0; // face to face at the drawn position
    std::optional<double> horizontalTolerance;
    std::optional<double> verticalGap; // face to face; nullopt without usable levels
    std::optional<double> verticalTolerance;
    std::string note;
};

// For every segment of every utility line, in order, the governing (worst,
// then closest) result against the segments of `design`. Per segment rather
// than per line because the grade changes along a line: a line whose worst
// part is a QL-C tail far from the works may cross them on a QL-B stretch,
// and the designer needs both.
//
// Horizontal clearance is tested first; where it fails and both the works and
// the service have qualified levels at the closest approach, the vertical
// separation there can still clear it - a service crossing under a pipe at
// depth is not a conflict.
//
// A service with no recorded diameter is treated as a line: the gap is to its
// centre, and the note says so.
//
// InvalidArgument for a design with fewer than two vertices, a negative
// half-width or requirement, or any line gradeLine() refuses.
[[nodiscard]] core::Result<std::vector<ClearanceResult>>
checkClearance(const DesignAlignment& design, const std::vector<UtilityLine>& utilities,
               const ClearanceRequirement& requirement = {}, const GradingSettings& settings = {});

// The value a delivery schema's Clash attribute should take (TfNSW's is one of
// No, Hard, Soft, Unknown), worst first. Hard: the service and the works
// overlap at the drawn position, in plan and, where levels say, in level.
// Soft: they do not touch but the required clearance is not met, or is met
// only at the drawn position. Unknown: the service is QL-C or QL-D and near.
// A SUGGESTION for the attribute, from geometry alone - the schema's clash is
// the designer's call.
enum class Clash {
    Hard,
    Soft,
    Unknown,
    No,
};

[[nodiscard]] const char* toString(Clash clash); // "Hard", "Soft", "Unknown", "No"
[[nodiscard]] Clash clashOf(const ClearanceResult& result);

} // namespace katana::survey::subsurface
