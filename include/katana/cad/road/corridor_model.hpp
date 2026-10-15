#pragma once

// Corridors (docs/road_design.md, "Corridors"): a road template swept along
// an alignment and its design profile.
//
// A corridor names what it is made from - the alignment, the template, the
// target surface its batters daylight on - and holds only what is its own:
// the station range, the interval it is sectioned at, the REGIONS where
// another template applies, and the SUPERELEVATION table. Nothing built from
// it is kept: the sections, the strings, the surfaces and the 3D objects are
// made from it again (cad/road/road_section.hpp), so moving a PI or editing
// a template is seen at once.
//
// Kept, as the templates are, as versioned JSON under the metadata key
// "corridors" (Document::corridors), every edit one undo step through the
// CORRIDOR verbs.

#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::cad::road {

// From `start` to `end` (stations, start < end) the corridor is built with
// `templateName` instead of its own template.
struct TemplateRegion {
    double start = 0.0;
    double end = 0.0;
    std::string templateName;

    friend bool operator==(const TemplateRegion&, const TemplateRegion&) = default;
};

// At `station`, the superelevated components (lanes and shoulders by
// default) of each side fall or rise at these crossfalls - outward percent,
// as a template's: (+6, -6) is full superelevation falling to the right,
// for a curve to the right. Linear between two entries; outside the table
// the template's own crossfalls apply. A table is entered with its normal
// crown at both ends of each transition, as a superelevation diagram is
// drawn.
struct SuperelevationPoint {
    double station = 0.0;
    double left = 0.0;
    double right = 0.0;

    friend bool operator==(const SuperelevationPoint&, const SuperelevationPoint&) = default;
};

// The relative error a corridor allows in a course's plan area and volume on a
// curve when it does not say: a thousandth, 0.1 %. A section is lofted to the
// next with flat quads, so over a step that turns through theta radians the
// area and the volume come out short by 1 - sin(theta) / theta, about
// theta^2 / 6, and a kerb's length by 1 - sin(theta / 2) / (theta / 2), about
// theta^2 / 24, a quarter as much. The corridor puts sections inside a curve
// until no step turns through more than sqrt(6 x tolerance), which holds the
// area and the volume to the tolerance (road::sectionStations). The number is a
// choice and not a standard's: it makes the steps 3.9 m on R = 50 m and 15.5 m on
// R = 200 m, so a tight curve is sectioned finer and a gentle one by its
// interval.
inline constexpr double kDefaultCurveTolerance = 0.001;
// The largest a corridor may ask: a tenth. Beyond it the "tolerance" is no
// longer a small error (the step would turn through 0.77 radians), and 1 would
// allow steps of 2.4 radians, which a flat quad does not approximate at all.
inline constexpr double kLargestCurveTolerance = 0.1;

struct Corridor {
    std::string name;
    std::string alignment;    // an alignment of the drawing, with a design profile
    std::string templateName; // a template of the drawing's road templates
    // The surface the batters daylight on and the earthworks are measured
    // against (Document::surfaces); empty: none, and the corridor is its
    // pavement alone.
    std::string surface;
    // Metres between sections. The alignment's and the profile's key
    // stations, the regions' ends and the superelevation stations are
    // sectioned as well.
    double interval = 10.0;
    // The relative error of a course's area and volume on a curve, at most:
    // sections are added inside the curves until the chords between them are
    // that close (kDefaultCurveTolerance says why a thousandth). 0 adds none,
    // which is every section the interval and the key stations give and the
    // chord error that leaves (road::sectionStations).
    double curveTolerance = kDefaultCurveTolerance;
    // The station range; each absent is that end of the alignment and the
    // profile's overlap.
    std::optional<double> start;
    std::optional<double> end;
    std::vector<TemplateRegion> regions;             // ascending, not overlapping
    std::vector<SuperelevationPoint> superelevation; // ascending station

    friend bool operator==(const Corridor&, const Corridor&) = default;
};

struct CorridorSet {
    std::vector<Corridor> corridors; // in the order they were made

    [[nodiscard]] const Corridor* find(std::string_view name) const;
    [[nodiscard]] Corridor* find(std::string_view name);

    friend bool operator==(const CorridorSet&, const CorridorSet&) = default;
};

// What a corridor must be on its own, without the drawing: a name, an
// alignment and a template named, a positive finite interval, a range with
// start before end, regions each start before end, not overlapping and
// naming a template, the table's stations distinct and every value finite
// with crossfalls within +-100 %. InvalidArgument naming the corridor and
// what is wrong. Regions and the table are sorted by the verbs that add to
// them; a corridor read with them out of order is refused.
[[nodiscard]] katana::core::Status checkCorridor(const Corridor& corridor);

// The superelevation table at `station`: nullopt outside it (the template's
// own crossfalls then apply), the entry's values at an entry, linear
// between two.
struct Crossfalls {
    double left = 0.0;
    double right = 0.0;

    friend bool operator==(const Crossfalls&, const Crossfalls&) = default;
};
[[nodiscard]] std::optional<Crossfalls> superelevationAt(const Corridor& corridor, double station);

// The template that applies at a station: a region's, or the corridor's.
[[nodiscard]] const std::string& templateAt(const Corridor& corridor, double station);

// {"format":"katana-corridors","version":1,"corridors":[...]}, doubles
// exact. ParseFailure for text that is not one, Unsupported for a newer
// version, checkCorridor's refusal for a corridor that is not sensible.
inline constexpr int kCorridorsVersion = 1;
[[nodiscard]] katana::core::Result<std::string> corridorsToJson(const CorridorSet& set);
[[nodiscard]] katana::core::Result<CorridorSet> corridorsFromJson(std::string_view text);

} // namespace katana::cad::road
