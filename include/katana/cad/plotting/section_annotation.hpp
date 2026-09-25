#pragma once

// What a section says besides its lines (docs/plotting.md, "Section smarts"):
// the cut and fill between the design and the ground, the level and depth of
// each thing it crosses, and where every label goes so that none is drawn
// over another or outside its plot.
//
// Pure functions on a cut section (cad/section.hpp) and on paper
// millimetres: the painter measures its text and draws what these decide, so
// each decision is tested without a pixel and an agent can read the same
// numbers the sheet prints.

#include <cstddef>
#include <limits>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/cad/layer_overrides.hpp"
#include "katana/cad/section.hpp"
#include "katana/entity/model.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad::plotting {

using katana::geometry::Box2;
using katana::geometry::Point2;

// ---- the series ------------------------------------------------------------------------

// A series is the design when its name starts with "design", in any case:
// the long section's grade line ("design ROAD") or a design surface
// ("DESIGN FINISHED SURFACE"). Every other series is ground.
[[nodiscard]] bool isDesignSeries(std::string_view name);
// The section's first design series, and its first other one - the ground
// that cut, fill and depth are measured from. Nothing when it has none.
[[nodiscard]] const SectionSurface* designSeries(const Section& section);
[[nodiscard]] const SectionSurface* groundSeries(const Section& section);

// How far past its first or last sample a station still reads that end: a
// micrometre, so a range whose end is worked out from the section's length
// is not a gap at its last column.
inline constexpr double kSectionEndSlack = 1e-6;

// The level of `surface` at `station`, straight between its samples;
// nothing in a gap or past either end (by more than kSectionEndSlack).
[[nodiscard]] std::optional<double> levelAt(const SectionSurface& surface, double station);

// The levels a section has between stations `from` and `to` - every series'
// samples and every crossing's level (crossingNote), with the series
// interpolated at the two ends - as (lowest, highest); nothing when none has
// a level there. `model` gives the crossings their own levels; a crossing on
// a layer `hidden` hides is not drawn, so it does not count.
[[nodiscard]] std::optional<std::pair<double, double>>
levelRange(const Section& section, double from, double to, const entity::Model* model = nullptr,
           const LayerOverrides* hidden = nullptr);

// ---- cut and fill ------------------------------------------------------------------------

// Design less ground at `station`: positive is fill, negative is cut;
// nothing where either has no level.
[[nodiscard]] std::optional<double> cutFillAt(const SectionSurface& design,
                                              const SectionSurface& ground, double station);
// A cut or fill as a data band writes it: signed, three decimals, "+0.250",
// "-1.200", "0.000".
[[nodiscard]] std::string cutFillText(double designLessGround);

enum class Earthwork { Cut, Fill };

struct EarthworkRegion {
    Earthwork kind = Earthwork::Cut;
    // (station, level), closed without repeating its first point: along the
    // design from the region's first station to its last, then back along
    // the ground.
    std::vector<Point2> outline;

    friend bool operator==(const EarthworkRegion&, const EarthworkRegion&) = default;
};

// The regions between the design and the ground over [from, to], in
// station order: CUT where the ground is above the design, FILL where it is
// below. They are split where the two cross (at the crossing, found on the
// straight lines between samples) and where either has a gap; pieces of one
// kind that follow each other are one region. Where the two coincide there
// is nothing to shade.
[[nodiscard]] std::vector<EarthworkRegion>
earthworkRegions(const SectionSurface& design, const SectionSurface& ground,
                 double from = -std::numeric_limits<double>::infinity(),
                 double to = std::numeric_limits<double>::infinity());

// The area of a closed outline (shoelace), always positive: an earthwork
// region's in square metres of section.
[[nodiscard]] double outlineArea(std::span<const Point2> outline);

// ---- crossings ------------------------------------------------------------------------------

// The level an entity carries where the section crosses it: its own heights
// (entity::heightsOf - a circle's one, a line's or string's at each vertex,
// an arc's at its two ends) straight between the two either side of the
// crossing. Nothing when it has none there: a line drawn flat has no level
// of its own, and the section drapes it on the ground.
[[nodiscard]] std::optional<double> ownLevel(const entity::Model& model,
                                             const SectionCrossing& crossing);

struct CrossingNote {
    // Its own level, else the level the section found under it (the
    // ground's); nothing when neither.
    std::optional<double> level;
    bool ownLevel = false;
    // Ground less its own level at the crossing: how deep it is buried,
    // negative when it is above the ground (an overhead line). Only for an
    // own level over ground: a draped line's is zero by definition.
    std::optional<double> depth;
    // "WATER RL 24.10 D 1.20"; "POWER RL 36.00 H 6.00" above the ground;
    // "KERB RL 23.95" draped; "FENCE" with no level. And the layer alone, for
    // when the whole does not fit.
    std::string text;
    std::string shortText;
};

// What is written where `crossing` cuts `section`: its layer, level and
// depth, to two decimals as a drafter writes a service. `model` gives the
// crossing its own level; without one every level is the ground's.
[[nodiscard]] CrossingNote crossingNote(const Section& section, const SectionCrossing& crossing,
                                        const entity::Model* model);

// ---- placing labels ---------------------------------------------------------------------------

struct LabelCandidate {
    // Where the label could go on the paper, best first. A label with a
    // shorter form lists the long form's places, then the short form's.
    std::vector<Box2> boxes;
    // Lower is placed first; the painter uses the distance from the plot's
    // middle, so the labels nearest the centreline are the ones kept.
    double priority = 0.0;
};

// Places `labels` in priority order (ties in the order given): each takes the
// first of its boxes that lies inside `bounds` and keeps `gapMm` clear of
// every obstacle and every label placed before it. The index of the box each
// took; nothing for a label that has none free, which is not drawn - a label
// is dropped rather than drawn over another or outside its plot.
[[nodiscard]] std::vector<std::optional<std::size_t>>
placeLabels(std::span<const LabelCandidate> labels, const Box2& bounds,
            std::span<const Box2> obstacles = {}, double gapMm = 0.3);

} // namespace katana::cad::plotting
