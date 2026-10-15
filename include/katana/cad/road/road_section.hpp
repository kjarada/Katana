#pragma once

// The corridor engine (docs/road_design.md, "Building a corridor"): a road
// template set across an alignment at the design level, section by section,
// and what is made from the sections - the earthworks, the strings, the
// finished and subgrade surfaces, and the 3D objects of the road model.
//
// ONE ENGINE. The old one-carriageway functions (cad/corridor.hpp:
// corridorSection, corridorQuantities, corridorSurface) are this engine run
// on templateFromAssembly, so a quantity from the old window dialog and one
// from CORRIDOR QUANTITIES cannot come to differ.
//
// A SECTION, side by side. Out from the centreline at the design level, each
// component of a side in turn: a lane or a shoulder (its width at its
// crossfall, or at the superelevation table's for the station when it is
// superelevated), a kerb (its shape, cad/road/road_template.hpp), a table
// drain (a V of its width, its invert `depth` below its inner edge), a
// footpath, a verge, a median - and last a batter, marched out from the
// hinge (the last top point) to the target surface as the old corridor did:
// half-metre steps until the batter and the ground change order, then forty
// bisections. Its benches are flats of their width every bench height. A
// batter that does not meet the ground within its maximum width, or meets no
// ground at all, leaves its side NOT REACHED and the section incomplete.
//
// THE FORMATION (the subgrade) is the bottom of each component's courses, a
// kerb's base, the top of what has no courses, then the batter. Where two
// neighbours' bottoms differ, or the courses end at the hinge, the line steps:
// the shallower corner is set kFaceRun into its own component, so the line
// stays a function of offset - which a TIN of it, and the end areas, need. A
// side of a batter alone starts its line at the hinge, the centreline.
//
// EARTHWORKS are between the formation and the ground across the section:
// cut where the ground is above, fill where it is below - the pavement box
// is cut, the courses sit on the fill - by the end-area method between two
// complete sections, as corridorQuantities always measured them.

#include <cstddef>
#include <optional>
#include <string>
#include <vector>

#include "katana/cad/bim/bim_object.hpp"
#include "katana/cad/corridor.hpp"
#include "katana/cad/road/corridor_model.hpp"
#include "katana/cad/road/road_template.hpp"
#include "katana/core/error.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/geometry/primitives3d.hpp"
#include "katana/geometry/profile.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::cad {
class Document;
}

namespace katana::cad::road {

// A vertical face of the formation is given this much run, so the line is a
// function of offset: more than the TIN builder's duplicate tolerance
// (math::tolerance::kCoordinate, 0.1 mm), so a TIN keeps both corners, and
// less than any construction tolerance on a subgrade. The area it moves is
// under a thousandth of a square metre for a metre-deep pavement.
inline constexpr double kFaceRun = 0.001;
// The columns an earthworks solid has across each side - its look, not its
// quantity, which is the end area of the true lines: spread over the stretch
// of the side where it has earthworks, every 2.5 % of it.
inline constexpr std::size_t kEarthworksColumns = 41;

// A point of a section's line: what it is ("CL", "left/lane/outer",
// "right/batter/daylight"), its offset (positive left) and its level.
struct SectionPoint {
    std::string code;
    double offset = 0.0;
    double level = 0.0;

    friend bool operator==(const SectionPoint&, const SectionPoint&) = default;
};

// A stretch of one side, as distances OUT from the centreline (from < to), over
// which the earthworks have thickness.
struct OutSpan {
    double from = 0.0;
    double to = 0.0;

    friend bool operator==(const OutSpan&, const OutSpan&) = default;
};

// One vertical of a solid in a section: offset, top level, bottom level.
struct SolidColumn {
    double offset = 0.0;
    double top = 0.0;
    double bottom = 0.0;

    friend bool operator==(const SolidColumn&, const SolidColumn&) = default;
};

// A course or a kerb in one section. Every section of a template has the
// same solids with the same columns, so a run of them lofts into one closed
// solid (loftSolid).
struct SectionSolid {
    std::string key;                   // "left/lane/AC14", "left/kerb"
    std::size_t component = 0;         // in RoadTemplate::components
    std::optional<std::size_t> course; // in its courses; none for a kerb
    std::vector<SolidColumn> columns;  // outward from the centreline, at least two

    friend bool operator==(const SectionSolid&, const SectionSolid&) = default;
};

struct RoadSection {
    double station = 0.0;
    katana::geometry::Point2 centre{};
    double direction = 0.0; // of the alignment, radians from +x
    double designLevel = 0.0;
    // Left outermost to right outermost: the daylights where the batters
    // met the ground, else the hinges.
    std::vector<SectionPoint> top;
    std::vector<SectionPoint> formation;
    std::vector<SectionSolid> solids;
    // The ground across the section, left to right: at every vertex of the
    // two lines and every half metre between. Empty when the section is not
    // complete.
    std::vector<SectionVertex> ground;
    // A side is reached when its batter met the ground, or it has no batter.
    bool leftReached = false;
    bool rightReached = false;
    // Both reached and the ground found everywhere across.
    bool complete = false;
    // Square metres between the formation and the ground (zero when not
    // complete): cut where the ground is above it, fill where below, each
    // side's split at the centreline.
    double cutArea = 0.0;
    double fillArea = 0.0;
    double leftCut = 0.0;
    double leftFill = 0.0;
    double rightCut = 0.0;
    double rightFill = 0.0;
    // Where on each side the ground is above the formation (cut) or below it
    // (fill) by more than math::tolerance::kCoordinate (0.1 mm) at some point
    // of the stretch, left to right as the section is: the stretches the
    // earthworks occupy in plan. A side with none has no cut (or fill) here,
    // and its area above is exactly zero: the daylight is found to about
    // 1e-13 m, so it lands on the wrong side of the ground half the time,
    // and that sliver of 1e-26 m2 is not earthworks. One place decides what
    // "none" means (buildSection); the objects and the quantities follow it.
    // Empty when the section is not complete.
    std::vector<OutSpan> leftCutSpans;
    std::vector<OutSpan> leftFillSpans;
    std::vector<OutSpan> rightCutSpans;
    std::vector<OutSpan> rightFillSpans;
};

// The section of `roadTemplate` at `station`. `ground` may be null: the
// batters then reach nothing and only the pavement is built.
// `superelevation` is the corridor's table at the station (superelevationAt).
// InvalidArgument for a template checkTemplate refuses, or a station outside
// the alignment or the profile.
[[nodiscard]] katana::core::Result<RoadSection>
roadSection(const RoadTemplate& roadTemplate, const katana::geometry::SolvedAlignment& alignment,
            const katana::geometry::SolvedProfile& profile,
            const katana::terrain::TinSurface* ground, double station,
            std::optional<Crossfalls> superelevation = std::nullopt);

// The template across a straight, level road at level 0 with no ground -
// its pavement as designed: what ROADTEMPLATE SECTION prints and the road
// template manager draws.
[[nodiscard]] katana::core::Result<RoadSection> templateSection(const RoadTemplate& roadTemplate);

// The stations a corridor is sectioned at from `start` to `end`: start + k *
// interval (multiplied, so the hundredth is not off by a hundred roundings),
// both ends, and each of `keys` inside, sorted, any two within
// math::tolerance::kGeometric made one.
[[nodiscard]] std::vector<double> sectionStations(double start, double end, double interval,
                                                  const std::vector<double>& keys);

// ---- a whole corridor -------------------------------------------------------------------

// The stretch of a corridor built with one template: a region of its own,
// or the corridor's template between and around the regions. Numbered from
// 1 in station order; the number is in its objects' ids.
struct CorridorRun {
    std::size_t region = 1;
    RoadTemplate roadTemplate;
    double start = 0.0;
    double end = 0.0;
    std::vector<RoadSection> sections; // ascending station, both ends included
    // End-area cut and fill each side, over the intervals between two
    // complete sections; the sections that are not complete.
    double leftCut = 0.0;
    double leftFill = 0.0;
    double rightCut = 0.0;
    double rightFill = 0.0;
    std::size_t incompleteSections = 0;
};

struct BuiltCorridor {
    std::string name;
    std::string surface; // the target surface it names, empty for none
    std::optional<katana::geometry::SolvedAlignment> alignment;
    double start = 0.0;
    double end = 0.0;
    std::vector<CorridorRun> runs;
    // What could not be built, a sentence each: an alignment, a profile, a
    // template or a surface that is not there, a station range outside the
    // alignment, sections that do not reach the ground.
    std::vector<std::string> problems;
    double cut = 0.0; // cubic metres, both sides, every run
    double fill = 0.0;
    std::size_t sections = 0;
    std::size_t incompleteSections = 0;
};

// Builds what it can. A corridor whose alignment or profile is missing has
// no runs; a template that is missing or refused leaves its stretch out; no
// target surface builds the pavement alone. Every such thing is a problem.
[[nodiscard]] BuiltCorridor buildCorridor(const Corridor& corridor,
                                          const RoadTemplateSet& templates,
                                          const katana::geometry::SolvedAlignment& alignment,
                                          const katana::geometry::SolvedProfile& profile,
                                          const katana::terrain::TinSurface* ground);
// What a corridor names, found in the drawing: its alignment and design
// profile solved, its target surface (null when it names none, or one the
// drawing lacks), and a sentence for each that is missing or does not solve.
// No alignment or no profile: both absent.
struct CorridorInputs {
    std::optional<katana::geometry::SolvedAlignment> alignment;
    std::optional<katana::geometry::SolvedProfile> profile;
    const katana::terrain::TinSurface* ground = nullptr;
    std::vector<std::string> problems;
};
[[nodiscard]] CorridorInputs corridorInputs(const Document& document, const Corridor& corridor);

// The same from the drawing: its inputs (corridorInputs) and the drawing's
// road templates (Document::roadTemplates).
[[nodiscard]] BuiltCorridor buildCorridor(const Document& document, const Corridor& corridor);

// ---- what is made from it ------------------------------------------------------------------

// A closed solid from a solid's columns at a run of stations: a top and a
// bottom face over the columns, the inner and outer faces down the first and
// last column, and a cap at each end - each edge shared by exactly two
// triangles, the faces turned outward. `columns[k]` are station `k`'s, all
// with the same count (two at least); `stations` has two at least.
// InvalidArgument otherwise.
[[nodiscard]] katana::core::Result<katana::geometry::TriangleMesh>
loftSolid(const katana::geometry::SolvedAlignment& alignment, const std::vector<double>& stations,
          const std::vector<std::vector<SolidColumn>>& columns);

// A closed, outward-turned mesh's volume is geometry::meshVolume.

// The road model's objects of a built corridor (cad/bim/bim_object.hpp): a
// closed solid per course of each component per side per run, per kerb,
// and the earthworks cut and fill of each side of each run - a piece for each
// stretch of stations where that side has any, inside a stretch of complete
// sections - with their quantities, and its problems.
// Object ids (stable, docs/road_design.md, "Object ids"):
//   corridor/<corridor>/<side>/<component>/<course>/<run>
//   corridor/<corridor>/<side>/<component>/<run>                a kerb
//   corridor/<corridor>/<side>/earthworks/cut|fill/<run>[-<piece>]   (<piece> from 2)
[[nodiscard]] bim::BimModel corridorSolids(const BuiltCorridor& built);

// The sections of every run in station order, each station once: where two
// runs meet, the first run's section stands for the station.
[[nodiscard]] std::vector<const RoadSection*> corridorSections(const BuiltCorridor& built);

// One longitudinal string of the finished surface (or the formation): the
// points of one code in consecutive sections.
struct CorridorString {
    std::string code;
    std::vector<katana::geometry::Point3> points;
};
// Every string, each broken where a section lacks its code; a section that
// repeats the station of the one before (where two runs meet) is left out.
[[nodiscard]] std::vector<CorridorString> corridorStrings(const BuiltCorridor& built,
                                                          bool formation);

// The finished surface, or the formation, as a TIN: every section's points,
// its strings as breaklines, and its outermost points as the boundary.
// `completeOnly` leaves the incomplete sections out, breaking the strings
// there, and then uses no boundary if any was left out - what the old
// corridorSurface did. InvalidGeometry when fewer than two sections go in
// (naming how many were incomplete), else whatever buildTin reports.
struct SectionsSurface {
    katana::terrain::TinSurface surface;
    std::size_t sections = 0;
    std::size_t leftOut = 0;
};
[[nodiscard]] katana::core::Result<SectionsSurface>
sectionsSurface(const std::vector<const RoadSection*>& sections,
                const katana::geometry::SolvedAlignment& alignment, bool formation,
                bool completeOnly);

} // namespace katana::cad::road
