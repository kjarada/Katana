#include "katana/interop/reference_data.hpp"

#include <algorithm>
#include <array>
#include <cmath>

namespace katana::interop {

using katana::geometry::Box2;
using katana::geometry::Point2;

// ---- raster ---------------------------------------------------------------

const char* toString(RasterRole role)
{
    switch (role) {
    case RasterRole::Imagery:
        return "imagery";
    case RasterRole::Elevation:
        return "elevation";
    case RasterRole::Derived:
        return "derived";
    }
    return "imagery";
}

Point2 RasterOverlay::pixelToWorld(double px, double py) const
{
    const auto& g = geotransform;
    return Point2(g[0] + px * g[1] + py * g[2], g[3] + px * g[4] + py * g[5]);
}

Box2 RasterOverlay::worldBounds() const
{
    Box2 box;
    if (width <= 0 || height <= 0) {
        return box;
    }
    // All four corners, not just two: a geotransform may carry rotation terms
    // (g[2], g[4]), and a north-up image has a NEGATIVE g[5], so assuming the
    // top-left pixel maps to the minimum corner is wrong in both cases.
    box.expand(pixelToWorld(0.0, 0.0));
    box.expand(pixelToWorld(static_cast<double>(width), 0.0));
    box.expand(pixelToWorld(0.0, static_cast<double>(height)));
    box.expand(pixelToWorld(static_cast<double>(width), static_cast<double>(height)));
    return box;
}

// ---- point cloud ----------------------------------------------------------

const char* toString(PointColorMode mode)
{
    switch (mode) {
    case PointColorMode::Elevation:
        return "Elevation";
    case PointColorMode::Intensity:
        return "Intensity";
    case PointColorMode::Classification:
        return "Classification";
    case PointColorMode::SourceColor:
        return "Source colour";
    case PointColorMode::Flat:
        return "Flat";
    }
    return "Elevation";
}

Box2 PointCloudLayer::worldBounds() const
{
    Box2 box;
    if (bounds.empty()) {
        return box;
    }
    box.expand(Point2(bounds.minX, bounds.minY));
    box.expand(Point2(bounds.maxX, bounds.maxY));
    return box;
}

namespace {

// A five-stop ramp: blue, cyan, green, yellow, red. Perceptually ordered enough
// for elevation and intensity, and unambiguous in print.
constexpr std::array<Rgb, 5> kRamp{Rgb{33, 68, 173}, Rgb{43, 172, 190}, Rgb{90, 176, 74},
                                   Rgb{233, 199, 63}, Rgb{201, 62, 48}};

Rgb rampAt(double normalised)
{
    if (!std::isfinite(normalised)) {
        return Rgb{128, 128, 128};
    }
    const double clamped = std::clamp(normalised, 0.0, 1.0);
    const double scaled = clamped * static_cast<double>(kRamp.size() - 1);
    const auto lower = static_cast<std::size_t>(std::floor(scaled));
    const std::size_t upper = std::min(lower + 1, kRamp.size() - 1);
    const double t = scaled - static_cast<double>(lower);

    const auto mix = [t](std::uint8_t a, std::uint8_t b) {
        return static_cast<std::uint8_t>(
            std::lround(static_cast<double>(a) + t * (static_cast<double>(b) - a)));
    };
    return Rgb{mix(kRamp[lower].r, kRamp[upper].r), mix(kRamp[lower].g, kRamp[upper].g),
               mix(kRamp[lower].b, kRamp[upper].b)};
}

} // namespace

Rgb classificationColor(std::uint8_t classification)
{
    // ASPRS LAS 1.4 table 17. Classes the standard leaves reserved, and any
    // vendor-specific class above 18, get a neutral grey rather than an
    // arbitrary colour that would imply a meaning this code cannot know.
    switch (classification) {
    case 0:
        return Rgb{150, 150, 150}; // created, never classified
    case 1:
        return Rgb{180, 180, 180}; // unclassified
    case 2:
        return Rgb{166, 124, 82}; // ground
    case 3:
        return Rgb{150, 200, 120}; // low vegetation
    case 4:
        return Rgb{95, 170, 80}; // medium vegetation
    case 5:
        return Rgb{40, 120, 50}; // high vegetation
    case 6:
        return Rgb{214, 104, 64}; // building
    case 7:
        return Rgb{220, 80, 200}; // low point (noise)
    case 9:
        return Rgb{60, 130, 220}; // water
    case 10:
        return Rgb{130, 100, 160}; // rail
    case 11:
        return Rgb{90, 90, 95}; // road surface
    case 13:
        return Rgb{230, 200, 90}; // wire - guard
    case 14:
        return Rgb{230, 170, 60}; // wire - conductor
    case 15:
        return Rgb{170, 140, 110}; // transmission tower
    case 16:
        return Rgb{200, 180, 140}; // wire structure connector
    case 17:
        return Rgb{120, 140, 190}; // bridge deck
    case 18:
        return Rgb{240, 60, 60}; // high noise
    default:
        return Rgb{128, 128, 128};
    }
}

Rgb colorForPoint(const katana::pointcloud::PointCloudPoint& point, PointColorMode mode,
                  double minValue, double maxValue)
{
    // A degenerate range (a perfectly flat surface, or a cloud with constant
    // intensity) would divide by zero; the midpoint of the ramp is the honest
    // answer, since every point really does have the same value.
    const double span = maxValue - minValue;
    const bool degenerate = !(span > 0.0) || !std::isfinite(span);

    switch (mode) {
    case PointColorMode::Elevation:
        return rampAt(degenerate ? 0.5 : (point.z - minValue) / span);
    case PointColorMode::Intensity:
        return rampAt(degenerate ? 0.5 : (point.intensity - minValue) / span);
    case PointColorMode::Classification:
        return classificationColor(point.classification);
    case PointColorMode::SourceColor:
        // Falls back to the elevation ramp when the file carries no colour, so
        // choosing this mode on a colourless cloud shows something rather than
        // a uniform black.
        if (point.hasColor) {
            return Rgb{point.red, point.green, point.blue};
        }
        return rampAt(degenerate ? 0.5 : (point.z - minValue) / span);
    case PointColorMode::Flat:
        return Rgb{200, 200, 200};
    }
    return Rgb{200, 200, 200};
}

// ---- the collection -------------------------------------------------------

ReferenceId ReferenceData::add(RasterOverlay raster)
{
    raster.id = nextId_++;
    const ReferenceId id = raster.id;
    rasters_.push_back(std::move(raster));
    return id;
}

ReferenceId ReferenceData::add(PointCloudLayer cloud)
{
    cloud.id = nextId_++;
    const ReferenceId id = cloud.id;
    pointClouds_.push_back(std::move(cloud));
    return id;
}

RasterOverlay* ReferenceData::findRaster(ReferenceId id)
{
    const auto it = std::find_if(rasters_.begin(), rasters_.end(),
                                 [id](const RasterOverlay& r) { return r.id == id; });
    return it == rasters_.end() ? nullptr : &*it;
}

PointCloudLayer* ReferenceData::findPointCloud(ReferenceId id)
{
    const auto it = std::find_if(pointClouds_.begin(), pointClouds_.end(),
                                 [id](const PointCloudLayer& c) { return c.id == id; });
    return it == pointClouds_.end() ? nullptr : &*it;
}

bool ReferenceData::remove(ReferenceId id)
{
    const auto raster = std::find_if(rasters_.begin(), rasters_.end(),
                                     [id](const RasterOverlay& r) { return r.id == id; });
    if (raster != rasters_.end()) {
        rasters_.erase(raster);
        return true;
    }
    const auto cloud = std::find_if(pointClouds_.begin(), pointClouds_.end(),
                                    [id](const PointCloudLayer& c) { return c.id == id; });
    if (cloud != pointClouds_.end()) {
        pointClouds_.erase(cloud);
        return true;
    }
    return false;
}

void ReferenceData::clear()
{
    rasters_.clear();
    pointClouds_.clear();
    // nextId_ is deliberately NOT reset: ids are never reused, matching the
    // entity database, so a stale id cannot resolve to a different layer.
}

Box2 ReferenceData::visibleBounds() const
{
    Box2 box;
    for (const RasterOverlay& raster : rasters_) {
        if (raster.visible) {
            box.expand(raster.worldBounds());
        }
    }
    for (const PointCloudLayer& cloud : pointClouds_) {
        if (cloud.visible) {
            box.expand(cloud.worldBounds());
        }
    }
    return box;
}

} // namespace katana::interop
