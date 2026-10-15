// How a surface's faces are coloured (cad/terrain/surface_colouring.hpp,
// docs/surface_display.md).

#include "katana/cad/terrain/surface_colouring.hpp"

#include <algorithm>
#include <charconv>
#include <cmath>
#include <cstdint>
#include <limits>
#include <numbers>

#include "katana/math/numerics.hpp"

namespace katana::cad {

namespace tol = katana::math::tolerance;
using katana::entity::Color;
using katana::geometry::Point3;

namespace {

// A blend of two colours, rounded half up, alpha included.
Color mix(const Color& a, const Color& b, double f)
{
    const auto channel = [f](std::uint8_t lo, std::uint8_t hi) {
        return static_cast<std::uint8_t>(static_cast<double>(lo) +
                                         (static_cast<double>(hi) - static_cast<double>(lo)) * f +
                                         0.5);
    };
    return Color{channel(a.r, b.r), channel(a.g, b.g), channel(a.b, b.b), channel(a.a, b.a)};
}

// The hue wheel the aspect sectors are coloured round: red at north, then
// orange, yellow, green, cyan, azure, blue and magenta clockwise - full
// saturation, so neighbouring sectors are as far apart as eight hues allow,
// and opposite slopes (north and south) are complementary.
constexpr std::array<Color, kAspectSectors> kAspectColours{{
    {230, 40, 40, 255},   // N
    {240, 150, 30, 255},  // NE
    {235, 225, 40, 255},  // E
    {60, 190, 60, 255},   // SE
    {40, 200, 210, 255},  // S
    {40, 140, 230, 255},  // SW
    {50, 60, 220, 255},   // W
    {200, 50, 200, 255},  // NW
}};

constexpr std::array<std::string_view, kAspectSectors> kAspectNames{"N",  "NE", "E", "SE",
                                                                    "S",  "SW", "W", "NW"};

int positiveModulo(int value, int modulus)
{
    const int r = value % modulus;
    return r < 0 ? r + modulus : r;
}

// The corners of `polygon` on the `keepAbove` side of the level `level`
// (above: z >= level; below: z <= level), Sutherland and Hodgman against one
// horizontal plane. A corner where an edge crosses is put on the level
// exactly, so a piece's band edge is the contour itself.
void clipToLevel(std::array<Point3, 5>& polygon, std::size_t& count, double level, bool keepAbove)
{
    std::array<Point3, 5> out{};
    std::size_t kept = 0;
    const auto inside = [&](const Point3& p) { return keepAbove ? p.z >= level : p.z <= level; };
    for (std::size_t i = 0; i < count; ++i) {
        const Point3& p = polygon[i];
        const Point3& q = polygon[(i + 1) % count];
        const bool pIn = inside(p);
        const bool qIn = inside(q);
        if (pIn && kept < out.size()) {
            out[kept++] = p;
        }
        if (pIn != qIn && kept < out.size()) {
            const double t = (level - p.z) / (q.z - p.z);
            out[kept++] = Point3(p.x + (q.x - p.x) * t, p.y + (q.y - p.y) * t, level);
        }
    }
    polygon = out;
    count = kept;
}

} // namespace

// ---- aspect ----------------------------------------------------------------------------

int aspectSector(double aspectRadians)
{
    if (!std::isfinite(aspectRadians)) {
        return 0;
    }
    // Shift by half a sector so north's sector starts at 0, then whole
    // sectors of 45 degrees.
    const double sector = std::numbers::pi / 4.0;
    const double shifted = std::fmod(aspectRadians + sector / 2.0, 2.0 * std::numbers::pi);
    const double wrapped = shifted < 0.0 ? shifted + 2.0 * std::numbers::pi : shifted;
    return std::clamp(static_cast<int>(std::floor(wrapped / sector)), 0, kAspectSectors - 1);
}

Color aspectSectorColour(int sector)
{
    return kAspectColours[static_cast<std::size_t>(positiveModulo(sector, kAspectSectors))];
}

std::string_view aspectSectorName(int sector)
{
    return kAspectNames[static_cast<std::size_t>(positiveModulo(sector, kAspectSectors))];
}

double steepestSlopePercent(const katana::terrain::TinSurface& surface)
{
    double steepest = 0.0;
    for (std::size_t t = 0; t < surface.triangleCount(); ++t) {
        if (const auto slope = surface.triangleSlopeAspect(t);
            slope.has_value() && std::isfinite(slope->slope)) {
            steepest = std::max(steepest, slope->slope * 100.0);
        }
    }
    return steepest;
}

// ---- the colouring ---------------------------------------------------------------------

std::size_t SurfaceColouring::bandCount() const
{
    if (!stops.empty()) {
        return stops.size();
    }
    return static_cast<std::size_t>(std::max(bands, 1));
}

std::size_t SurfaceColouring::bandOf(double value) const
{
    const std::size_t count = bandCount();
    if (!std::isfinite(value) || count <= 1) {
        return 0;
    }
    if (!stops.empty()) {
        // The last stop at or below the value; below them all, the first.
        const auto above = std::ranges::upper_bound(stops, value, {}, &ColourStop::value);
        const auto index = static_cast<std::size_t>(above - stops.begin());
        return index == 0 ? 0 : index - 1;
    }
    const double span = high - low;
    if (!(span > tol::kGeometric)) {
        return count / 2;
    }
    const double scaled = (value - low) / span * static_cast<double>(count);
    if (!(scaled > 0.0)) {
        return 0;
    }
    return std::min(static_cast<std::size_t>(scaled), count - 1);
}

Color SurfaceColouring::bandColour(std::size_t band) const
{
    const std::size_t count = bandCount();
    const std::size_t k = std::min(band, count - 1);
    if (!stops.empty()) {
        return stops[k].colour;
    }
    if (count <= 1) {
        return surfaceRampColour(ramp, 0.5);
    }
    return surfaceRampColour(ramp, static_cast<double>(k) / static_cast<double>(count - 1));
}

std::optional<double> SurfaceColouring::bandLow(std::size_t band) const
{
    const std::size_t count = bandCount();
    const std::size_t k = std::min(band, count - 1);
    if (!stops.empty()) {
        return k == 0 && count > 1 ? std::optional<double>{} : std::optional<double>{stops[k].value};
    }
    return low + (high - low) * static_cast<double>(k) / static_cast<double>(count);
}

std::optional<double> SurfaceColouring::bandHigh(std::size_t band) const
{
    const std::size_t count = bandCount();
    const std::size_t k = std::min(band, count - 1);
    if (!stops.empty()) {
        return k + 1 >= count ? std::optional<double>{} : std::optional<double>{stops[k + 1].value};
    }
    // The last band's top is `high` itself, not low + span * count / count,
    // which rounding could leave a hair short of it.
    if (k + 1 == count) {
        return high;
    }
    return low + (high - low) * static_cast<double>(k + 1) / static_cast<double>(count);
}

Color SurfaceColouring::colourAt(double value) const
{
    if (!std::isfinite(value)) {
        return flat;
    }
    if (banded) {
        return bandColour(bandOf(value));
    }
    if (!stops.empty()) {
        if (value <= stops.front().value) {
            return stops.front().colour;
        }
        if (value >= stops.back().value) {
            return stops.back().colour;
        }
        const auto above = std::ranges::upper_bound(stops, value, {}, &ColourStop::value);
        const ColourStop& b = *above;
        const ColourStop& a = *(above - 1);
        const double width = b.value - a.value;
        return width > 0.0 ? mix(a.colour, b.colour, (value - a.value) / width) : b.colour;
    }
    const double span = high - low;
    return surfaceRampColour(ramp, span > tol::kGeometric ? (value - low) / span : 0.5);
}

SurfaceColouring surfaceColouring(const SurfaceDisplayStyle& style, SurfaceFaces faces,
                                  double defaultLow, double defaultHigh)
{
    SurfaceColouring colouring;
    colouring.faces = faces;
    colouring.rampName = isSurfaceRamp(style.ramp) ? style.ramp : std::string(surfaceRampNames().front());
    colouring.ramp = surfaceRampStops(style.ramp);
    colouring.stops = style.stops;
    // checkSurfaceStyle refuses stops out of order; a set that came in
    // around it is put in order rather than read wrongly.
    std::ranges::stable_sort(colouring.stops, {}, &ColourStop::value);
    colouring.banded = style.banded;
    colouring.bands = std::max(style.bands, 1);
    colouring.flat = style.flatColour;
    colouring.low = style.rangeMin.value_or(defaultLow);
    colouring.high = style.rangeMax.value_or(defaultHigh);
    if (!std::isfinite(colouring.low)) {
        colouring.low = 0.0;
    }
    if (!std::isfinite(colouring.high) || colouring.high < colouring.low) {
        colouring.high = colouring.low;
    }
    return colouring;
}

SurfaceColouring surfaceColouring(const SurfaceDisplayStyle& style, SurfaceFaces faces,
                                  const katana::terrain::TinSurface& surface)
{
    if (faces == SurfaceFaces::Slope) {
        // The steepest face is a pass over every triangle: only when the
        // style leaves the top of the range open.
        const double top = style.rangeMax.has_value() ? *style.rangeMax : steepestSlopePercent(surface);
        return surfaceColouring(style, faces, 0.0, top);
    }
    return surfaceColouring(style, faces, surface.minElevation(), surface.maxElevation());
}

void bandPieces(const SurfaceColouring& colouring, const Point3& a, const Point3& b, const Point3& c,
                std::vector<BandPiece>& out)
{
    BandPiece whole;
    whole.corners[0] = a;
    whole.corners[1] = b;
    whole.corners[2] = c;
    whole.count = 3;
    if (!colouring.banded || colouring.faces != SurfaceFaces::Elevation) {
        out.push_back(whole);
        return;
    }
    const double lowest = std::min({a.z, b.z, c.z});
    const double highest = std::max({a.z, b.z, c.z});
    const std::size_t first = colouring.bandOf(lowest);
    const std::size_t last = colouring.bandOf(highest);
    if (first == last || !std::isfinite(lowest) || !std::isfinite(highest)) {
        whole.band = first;
        out.push_back(whole);
        return;
    }
    for (std::size_t k = first; k <= last; ++k) {
        BandPiece piece = whole;
        piece.band = k;
        // Cut at the band's own edges, except where the triangle does not
        // reach past one: the first piece keeps what lies below its band
        // (none, by bandOf), the last what lies above.
        if (k > first) {
            if (const auto bottom = colouring.bandLow(k)) {
                clipToLevel(piece.corners, piece.count, *bottom, true);
            }
        }
        if (k < last && piece.count >= 3) {
            if (const auto top = colouring.bandHigh(k)) {
                clipToLevel(piece.corners, piece.count, *top, false);
            }
        }
        if (piece.count >= 3) {
            out.push_back(piece);
        }
    }
}

// ---- the legend ------------------------------------------------------------------------

int legendDecimals(std::span<const double> values)
{
    int decimals = 0;
    for (const double value : values) {
        if (!std::isfinite(value)) {
            continue;
        }
        int d = 0;
        for (; d < 3; ++d) {
            // Within a millionth of a unit of a value written with d
            // decimals: 26.284699999999997, a tenth of the sample terrain's
            // range added to its lowest point, is not 26.28 but is 26.285.
            const double power = std::pow(10.0, d);
            const double scaled = value * power;
            if (std::abs(scaled - std::round(scaled)) <= 1.0e-6 * power) {
                break;
            }
        }
        decimals = std::max(decimals, d);
    }
    return decimals;
}

std::string formatLegendValue(double value, int decimals)
{
    char buffer[64];
    // to_chars, not snprintf: a decimal comma under another C locale would
    // change what a legend and a verb reply say.
    const auto result = std::to_chars(buffer, buffer + sizeof buffer, value == 0.0 ? 0.0 : value,
                                      std::chars_format::fixed, std::clamp(decimals, 0, 6));
    return result.ec == std::errc{} ? std::string(buffer, result.ptr) : std::string("?");
}


SurfaceLegend surfaceLegend(const SurfaceColouring& colouring)
{
    SurfaceLegend legend;
    legend.faces = colouring.faces;
    switch (colouring.faces) {
    case SurfaceFaces::None:
        return legend;
    case SurfaceFaces::Flat:
        legend.title = "Flat";
        legend.entries.push_back({colouring.flat, {}, {}, "surface"});
        return legend;
    case SurfaceFaces::Aspect:
        legend.title = "Aspect";
        for (int sector = 0; sector < kAspectSectors; ++sector) {
            legend.entries.push_back(
                {aspectSectorColour(sector), {}, {}, std::string(aspectSectorName(sector))});
        }
        legend.entries.push_back({colouring.flat, {}, {}, "level"});
        return legend;
    case SurfaceFaces::Elevation:
    case SurfaceFaces::Slope:
        break;
    }
    legend.title = colouring.faces == SurfaceFaces::Slope ? "Slope %" : "Elevation";

    if (colouring.banded) {
        const std::size_t count = colouring.bandCount();
        std::vector<double> edges;
        for (std::size_t k = 0; k < count; ++k) {
            for (const auto edge : {colouring.bandLow(k), colouring.bandHigh(k)}) {
                if (edge.has_value()) {
                    edges.push_back(*edge);
                }
            }
        }
        const int decimals = legendDecimals(edges);
        for (std::size_t k = 0; k < count; ++k) {
            SurfaceLegendEntry entry;
            entry.colour = colouring.bandColour(k);
            entry.from = colouring.bandLow(k);
            entry.to = colouring.bandHigh(k);
            if (entry.from && entry.to) {
                entry.label = formatLegendValue(*entry.from, decimals) + " - " +
                              formatLegendValue(*entry.to, decimals);
            } else if (entry.to) {
                entry.label = "< " + formatLegendValue(*entry.to, decimals);
            } else if (entry.from) {
                entry.label = formatLegendValue(*entry.from, decimals) + " +";
            }
            legend.entries.push_back(std::move(entry));
        }
        return legend;
    }

    legend.gradient = true;
    if (!colouring.stops.empty()) {
        std::vector<double> values;
        for (const ColourStop& stop : colouring.stops) {
            values.push_back(stop.value);
        }
        const int decimals = legendDecimals(values);
        for (const ColourStop& stop : colouring.stops) {
            legend.entries.push_back(
                {stop.colour, stop.value, stop.value, formatLegendValue(stop.value, decimals)});
        }
        return legend;
    }
    // The ramp at its five stops and halfway between each: nine samples,
    // enough for a bar blended between them to show every stop's colour.
    constexpr int kSamples = 9;
    const double ends[2] = {colouring.low, colouring.high};
    const int decimals = legendDecimals(ends);
    for (int i = 0; i < kSamples; ++i) {
        const double t = static_cast<double>(i) / (kSamples - 1);
        const double value = colouring.low + (colouring.high - colouring.low) * t;
        SurfaceLegendEntry entry;
        entry.colour = colouring.colourAt(value);
        entry.from = value;
        entry.to = value;
        if (i == 0 || i == kSamples - 1) {
            entry.label = formatLegendValue(i == 0 ? colouring.low : colouring.high, decimals);
        }
        legend.entries.push_back(std::move(entry));
    }
    return legend;
}

} // namespace katana::cad
