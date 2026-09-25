#pragma once

// Precision input: the drafting aids every point a user gives passes through
// (docs/drawing.md, "Precision input") - the typed coordinate grammar with
// survey bearings and distances in degrees, minutes and seconds, direct
// distance entry along the rubber band, length and angle locks, ortho and
// polar tracking, and object snap tracking.
//
// Headless, like everything a tool decides: the plan view asks constrain()
// what point the cursor gives, the tool host and the command line ask
// parsePrecisePoint() what point some text is, and the settings they read
// are the Document's (Document::drafting()), so a view, the command line and
// an agent's ORTHO / POLAR / SNAP / LOCK verbs all change the same state.
//
// ANGLES. The command line's convention is kept as the default: degrees
// counter-clockwise from east. Survey work reads bearings - clockwise from
// north - so the convention is a setting (AngleConvention::Bearing, the
// UNITS ANGLE BEARING verb), and a QUADRANT bearing (N45d30'15"E) is a
// bearing whatever the setting, since it cannot be read any other way.
// Angles are typed as decimal degrees (45.5), or degrees, minutes and
// seconds with d or the degree sign and ' and " (45d30'15", 45°30'15.25",
// 45d30'); internally everything is radians counter-clockwise from east.

#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/snapping.hpp"
#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad {

enum class AngleConvention {
    Counterclockwise, // degrees counter-clockwise from east (the command line's)
    Bearing,          // degrees clockwise from north (whole-circle bearing)
};

struct DraftingSettings {
    // Ortho: a point from a base goes straight across or straight up.
    bool ortho = false;
    // Polar tracking: a direction within the aperture of a multiple of the
    // increment snaps onto it.
    bool polar = false;
    double polarIncrement = 15.0 * katana::math::kDegToRad; // radians
    // Object snap tracking: points the cursor has rested on are acquired, and
    // the horizontal and vertical through each become paths to snap to.
    bool objectTracking = false;
    AngleConvention angles = AngleConvention::Counterclockwise;
    // Locks for the next point from a base: its direction (radians,
    // counter-clockwise from east) and its distance. Typed as <angle and
    // =distance at a point prompt; they apply until cleared.
    std::optional<double> angleLock;
    std::optional<double> lengthLock;
    // The running object snaps.
    bool snapEnabled = true;
    SnapModes snapModes = kDefaultSnapModes;

    friend bool operator==(const DraftingSettings&, const DraftingSettings&) = default;
};

// ---- angles ------------------------------------------------------------------------------

// Degrees from "45.5", "45d30'15\"", "45°30'15.25\"", "45d30'" or "45d";
// a leading minus is kept. ParseFailure otherwise.
[[nodiscard]] katana::core::Result<double> parseDegrees(std::string_view text);
// A direction, radians counter-clockwise from east in (-pi, pi], from typed text: a
// quadrant bearing ("N45d30'15\"E", "S 12.5 W") always as a bearing; anything
// else as degrees in `convention`.
[[nodiscard]] katana::core::Result<double> parseDirection(std::string_view text,
                                                          AngleConvention convention);
// The whole-circle bearing of a direction, 0 to 360 degrees clockwise from
// north, as D°MM'SS" (seconds to `decimals` places, rounding carried).
[[nodiscard]] std::string formatBearing(double radians, int decimals = 0);
// Degrees as D°MM'SS".
[[nodiscard]] std::string formatDms(double degrees, int decimals = 0);
// A direction as a quadrant bearing, N45°30'15"E.
[[nodiscard]] std::string formatQuadrantBearing(double radians, int decimals = 0);

// ---- typed points -----------------------------------------------------------------------------

struct PrecisePoint {
    katana::geometry::Point2 point;
    std::optional<double> z; // a height given as the third coordinate
};

// A typed point:
//   x,y[,z]                      absolute
//   @dx,dy[,dz]                  relative to `last`
//   @distance<direction          polar from `last`, the direction by
//                                parseDirection (a DMS angle, a quadrant
//                                bearing, or degrees in the convention)
// InvalidState for relative input with no `last`, ParseFailure otherwise.
[[nodiscard]] katana::core::Result<PrecisePoint>
parsePrecisePoint(std::string_view text, std::optional<katana::geometry::Point2> last,
                  const DraftingSettings& settings = {});

// True when `text` looks like a point (a comma, or the @ of relative input).
[[nodiscard]] bool looksLikePoint(std::string_view text);

// Direct distance entry: the point `distance` from `base` towards `cursor`
// (or along the angle lock when there is one; east when the cursor is on
// the base).
[[nodiscard]] katana::geometry::Point2 directDistance(const katana::geometry::Point2& base,
                                                      const katana::geometry::Point2& cursor,
                                                      double distance,
                                                      const DraftingSettings& settings = {});

// ---- constraining the cursor -----------------------------------------------------------------

struct ConstrainedPoint {
    katana::geometry::Point2 point;
    // What constrained it, for the view's tooltip: "Ortho", "Polar 45°",
    // "Angle lock 30°", "Length lock 10", "Tracking"; empty when nothing did.
    std::string label;
    // The direction a tracking or polar path runs from its base, to draw.
    std::optional<katana::geometry::Point2> pathFrom;
};

// The point the cursor gives from `base` under the settings: the angle lock
// (else ortho, else polar within `aperture` of a multiple of the increment)
// fixes the direction, and the length lock the distance. With no base only
// the cursor itself.
[[nodiscard]] ConstrainedPoint constrain(std::optional<katana::geometry::Point2> base,
                                         const katana::geometry::Point2& cursor,
                                         const DraftingSettings& settings, double aperture);

// Object snap tracking: the point on a horizontal or vertical path through
// one of `acquired` (or where two such paths cross) within `aperture` of the
// cursor, nearest first; nullopt when the cursor is on none.
[[nodiscard]] std::optional<ConstrainedPoint>
trackAcquired(const std::vector<katana::geometry::Point2>& acquired,
              const katana::geometry::Point2& cursor, double aperture);

// ---- one-shot snaps ------------------------------------------------------------------------------

// From: the base point plus a typed offset ("@3,4", "@5<30").
[[nodiscard]] katana::core::Result<katana::geometry::Point2>
fromBase(const katana::geometry::Point2& base, std::string_view offset,
         const DraftingSettings& settings = {});
// Midpoint between two points.
[[nodiscard]] katana::geometry::Point2 midBetween(const katana::geometry::Point2& a,
                                                  const katana::geometry::Point2& b);

} // namespace katana::cad
