#pragma once

// How a surface's faces are coloured, resolved once from its display style
// (surface_style.hpp) and the values it spans, and asked by every picture of
// it - the plan view (surface_plan.hpp), the 3D scene (scene.hpp), the
// legends and the SURFACE LEGEND verb - so that no two of them can disagree
// about what colour a height, a slope or an aspect is (docs/surface_display.md).
//
// The value a face is coloured by is its ELEVATION in drawing units, or its
// SLOPE IN PERCENT (rise over run times 100: 100 is 45 degrees), or the
// direction it faces (aspect, in eight sectors). The ramps themselves are in
// surface_ramps.cpp: the surface styles' ramp table, which the 3D scene's own
// elevation and slope colours are read from too.

#include <array>
#include <cstddef>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/terrain/surface_style.hpp"
#include "katana/entity/entity.hpp"
#include "katana/geometry/primitives3d.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::cad {

// ---- the ramps -------------------------------------------------------------------------

// Every ramp is five colours evenly spaced over 0 to 1, blended linearly
// between them: the form the 3D scene's vector kernels read
// (src/katana_cad/simd/scene_kernels.hpp: four segments a channel), so a
// surface coloured by ANY ramp takes the fast path, and a ramp is a row of
// five colours a person can read in the table.
inline constexpr std::size_t kSurfaceRampStops = 5;
using SurfaceRampStops = std::array<katana::entity::Color, kSurfaceRampStops>;

// The stops of `ramp` (case-insensitive); an unknown name is the first ramp's,
// as surfaceRampColour says.
[[nodiscard]] const SurfaceRampStops& surfaceRampStops(std::string_view ramp);
// Whether `ramp` names one of surfaceRampNames(), in any case.
[[nodiscard]] bool isSurfaceRamp(std::string_view ramp);
// The colour at `t` along `stops`: t clamped to 0 to 1 (not finite: 0), and
// each channel a + (b - a) f rounded half up - the arithmetic of the scene's
// elevation ramp before the table, to the bit, which its kernels equal.
[[nodiscard]] katana::entity::Color surfaceRampColour(const SurfaceRampStops& stops, double t);

// ---- aspect ----------------------------------------------------------------------------

// A face's aspect is one of eight sectors of 45 degrees, centred on north,
// north-east ... north-west, as a compass rose is read.
inline constexpr int kAspectSectors = 8;
// The sector of an aspect in radians clockwise from north (TinSurface's
// SlopeAspect::aspect): 0 north, from 337.5 to 22.5 degrees, then clockwise;
// a sector's lower edge belongs to it. Not finite: 0.
[[nodiscard]] int aspectSector(double aspectRadians);
// Red at north round a hue wheel in 45-degree steps (docs/surface_display.md,
// "Aspect"); an out-of-range sector is taken modulo eight.
[[nodiscard]] katana::entity::Color aspectSectorColour(int sector);
// "N", "NE", "E", "SE", "S", "SW", "W", "NW".
[[nodiscard]] std::string_view aspectSectorName(int sector);

// ---- what a surface spans --------------------------------------------------------------

// The steepest face, in percent; 0 for an empty or flat surface. A triangle
// vertical in plan (no slope) is not counted.
[[nodiscard]] double steepestSlopePercent(const katana::terrain::TinSurface& surface);

// ---- the colouring ---------------------------------------------------------------------

// What colours one surface's faces in one picture: the style's rule with its
// range resolved. Plain data, cheap to copy.
struct SurfaceColouring {
    SurfaceFaces faces = SurfaceFaces::None;
    // Elevation and Slope: the ramp, stretched over [low, high] - or, when
    // `stops` is not empty, the stops, which override it and are absolute.
    SurfaceRampStops ramp = surfaceRampStops("terrain");
    std::string rampName = "terrain";
    std::vector<ColourStop> stops{};
    bool banded = false;
    int bands = 10;
    double low = 0.0;
    double high = 0.0;
    // The data runs past an end of the range the style set: min= above the
    // lowest value (openBelow), max= below the highest (openAbove). A banded
    // ramp's end band then takes in everything past its edge, and says so
    // (bandLow, bandHigh) rather than naming a closed range its colour also
    // covers. surfaceColouring sets them.
    bool openBelow = false;
    bool openAbove = false;
    // Flat faces, a flat (level) face under Aspect, and a value that is not
    // finite.
    katana::entity::Color flat{170, 170, 160, 255};

    [[nodiscard]] bool coloursFaces() const { return faces != SurfaceFaces::None; }
    // Elevation or Slope: a value decides the colour.
    [[nodiscard]] bool byValue() const
    {
        return faces == SurfaceFaces::Elevation || faces == SurfaceFaces::Slope;
    }

    // The colour of `value` (an elevation, or a slope in percent):
    //   smooth ramp   the ramp at (value - low) / (high - low), clamped. A
    //                 range narrower than tolerance::kGeometric is a point
    //                 (collapsed): below it is the ramp's start and above it
    //                 its end, as for any range; at it the ramp's middle, as
    //                 the scene's one ramp always was for a level surface -
    //                 or, for slope, its start, because the point is a face
    //                 with no steepness;
    //   smooth stops  blended between the two stops either side, the first
    //                 below them all and the last above;
    //   banded        its band's colour (bandOf, bandColour).
    // Not finite: `flat`.
    [[nodiscard]] katana::entity::Color colourAt(double value) const;

    // Banded only. The bands:
    //   a ramp        `bands` (at least one) of equal width over [low, high];
    //                 below low is the first, above high the last. Band k is
    //                 the ramp at k / (bands - 1), so the first and last
    //                 bands are the ramp's two ends (one band: its middle).
    //                 A collapsed range is one point, and takes the same
    //                 sides: below it the first band, above it the last, at
    //                 it the middle (slope: the first).
    //   stops         one a stop: band k runs from stop k's value up to the
    //                 next stop's, in stop k's colour; below the first stop
    //                 is the first band, from the last stop up is the last.
    [[nodiscard]] std::size_t bandCount() const;
    // A ramp (no stops) whose range is narrower than tolerance::kGeometric: a
    // point, not something to stretch a ramp or cut bands over.
    [[nodiscard]] bool collapsed() const;
    [[nodiscard]] std::size_t bandOf(double value) const;
    [[nodiscard]] katana::entity::Color bandColour(std::size_t band) const;
    // Where band k starts and ends; absent at an open end: the first band of
    // stops starts nowhere below and the last ends nowhere above (one stop is
    // open both ways), and a ramp's end bands are open where the data runs
    // past the range (openBelow, openAbove). Where it does not they are
    // closed at low and high.
    [[nodiscard]] std::optional<double> bandLow(std::size_t band) const;
    [[nodiscard]] std::optional<double> bandHigh(std::size_t band) const;
};

// `style`'s rule for `faces` (its planFaces or its faces3d), the ramp
// stretched over the style's range - each end the style gives - else over
// [defaultLow, defaultHigh]. A range given back to front is taken as the
// point at its low end (collapsed). An unknown ramp name is the first ramp, as
// surfaceRampColour takes it.
[[nodiscard]] SurfaceColouring surfaceColouring(const SurfaceDisplayStyle& style, SurfaceFaces faces,
                                                double defaultLow, double defaultHigh);
// The same, with the defaults a surface gives on its own: its lowest and
// highest vertex for Elevation, 0 to its steepest face for Slope.
[[nodiscard]] SurfaceColouring surfaceColouring(const SurfaceDisplayStyle& style, SurfaceFaces faces,
                                                const katana::terrain::TinSurface& surface);

// The parts of a triangle in each band of a BANDED colouring by elevation:
// convex polygons of three to five corners on the triangle's own plane, each
// wholly in one band, covering the triangle exactly - a band edge is a
// contour across it, and a corner on one has the edge's level exactly. The
// 3D scene draws banded faces so, a band's colour stopping at its contour
// rather than smeared across a triangle.
struct BandPiece {
    std::size_t band = 0;
    std::array<katana::geometry::Point3, 5> corners{};
    std::size_t count = 0;
};
// Appends `a`, `b`, `c`'s pieces to `out`, lowest band first. A colouring
// that is not banded by value gives the whole triangle in band 0. A banded
// style has at most kMaximumSurfaceBands bands (checkSurfaceStyle), so a
// triangle is cut into that many pieces at the most.
void bandPieces(const SurfaceColouring& colouring, const katana::geometry::Point3& a,
                const katana::geometry::Point3& b, const katana::geometry::Point3& c,
                std::vector<BandPiece>& out);

// ---- the legend ------------------------------------------------------------------------

struct SurfaceLegendEntry {
    katana::entity::Color colour{};
    // The values it stands for; an open end is absent. A gradient's entries
    // are points (from == to).
    std::optional<double> from{};
    std::optional<double> to{};
    // What a person reads beside the swatch: "24.892 - 26.285", "< 2",
    // "33 +", "NE", "level". Empty on a gradient's inner samples.
    std::string label{};
};

struct SurfaceLegend {
    SurfaceFaces faces = SurfaceFaces::None;
    // "Elevation", "Slope %", "Aspect", "Flat".
    std::string title{};
    // A smooth ramp or smooth stops: `entries` are colours from the lowest
    // value to the highest, to be blended into one bar, labelled at its ends
    // (each at its own value: `from`, which need not be evenly spaced).
    // Otherwise one swatch an entry, lowest first. A collapsed range is not a
    // gradient: it has the swatches below, at and above its point.
    bool gradient = false;
    std::vector<SurfaceLegendEntry> entries{};
};

// What `colouring` means, to be read off a picture. Empty entries for faces
// that are not coloured (None).
[[nodiscard]] SurfaceLegend surfaceLegend(const SurfaceColouring& colouring);

// The fewest decimals, at most three, that write every one of `values`
// exactly to a part in a million of a unit: a legend's labels, so bands of
// whole metres read "25 - 26" and the sample terrain's tenths of its range
// read to the millimetre.
[[nodiscard]] int legendDecimals(std::span<const double> values);
// `value` with `decimals` decimals, locale-independent ("." always).
[[nodiscard]] std::string formatLegendValue(double value, int decimals);

// The SURFACE word of each face kind, as the SURFACESTYLE verbs write it, is
// surfaceFacesWord (surface_style.hpp).

} // namespace katana::cad
