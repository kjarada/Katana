#include "katana/interop/reference_data.hpp"

#include <algorithm>
#include <array>
#include <cctype>
#include <cmath>
#include <string>
#include <system_error>

#include <nlohmann/json.hpp>

#include "katana/interop/archive12d.hpp"
#include "katana/interop/import.hpp"

namespace katana::interop {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
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
    missing_.clear();
    // nextId_ is deliberately NOT reset: ids are never reused, matching the
    // entity database, so a stale id cannot resolve to a different layer.
}

bool ReferenceData::forgetMissing(std::string_view name)
{
    const auto found = std::ranges::find_if(missing_, [name](const std::string& record) {
        const auto parsed = parseReferenceRecord(record);
        if (!parsed || parsed->name.size() != name.size()) {
            return false;
        }
        return std::ranges::equal(parsed->name, name, [](char a, char b) {
            return std::tolower(static_cast<unsigned char>(a)) ==
                   std::tolower(static_cast<unsigned char>(b));
        });
    });
    if (found == missing_.end()) {
        return false;
    }
    missing_.erase(found);
    return true;
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

// ---- the words a line and a record use ------------------------------------

namespace {

struct ColourWord {
    PointColorMode mode;
    const char* word;
};
constexpr std::array<ColourWord, 5> kColourWords{{
    {PointColorMode::Elevation, "elevation"},
    {PointColorMode::Intensity, "intensity"},
    {PointColorMode::Classification, "classification"},
    {PointColorMode::SourceColor, "rgb"},
    {PointColorMode::Flat, "flat"},
}};

struct StyleWord {
    RasterDisplayStyle style;
    const char* word;
};
constexpr std::array<StyleWord, 5> kStyleWords{{
    {RasterDisplayStyle::Plain, "plain"},
    {RasterDisplayStyle::Hillshade, "hillshade"},
    {RasterDisplayStyle::Relief, "relief"},
    {RasterDisplayStyle::ReliefHillshade, "relief+hillshade"},
    {RasterDisplayStyle::Slope, "slope"},
}};

std::string loweredCopy(std::string_view word)
{
    std::string out(word);
    for (char& c : out) {
        c = static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
    }
    return out;
}

std::string utf8(const std::filesystem::path& path)
{
    const std::u8string text = path.generic_u8string();
    return std::string(text.begin(), text.end());
}

std::filesystem::path pathOf(const std::string& text)
{
    return std::filesystem::path(std::u8string(text.begin(), text.end()));
}

// The record's version: a record of a newer one is refused, not half read.
constexpr int kRecordVersion = 1;

} // namespace

const char* toWord(PointColorMode mode)
{
    for (const ColourWord& each : kColourWords) {
        if (each.mode == mode) {
            return each.word;
        }
    }
    return "elevation";
}

std::optional<PointColorMode> pointColorModeFromWord(std::string_view word)
{
    const std::string lowered = loweredCopy(word);
    for (const ColourWord& each : kColourWords) {
        if (lowered == each.word) {
            return each.mode;
        }
    }
    return std::nullopt;
}

const char* toWord(RasterDisplayStyle style)
{
    for (const StyleWord& each : kStyleWords) {
        if (each.style == style) {
            return each.word;
        }
    }
    return "plain";
}

std::optional<RasterDisplayStyle> displayStyleFromWord(std::string_view word)
{
    const std::string lowered = loweredCopy(word);
    for (const StyleWord& each : kStyleWords) {
        if (lowered == each.word) {
            return each.style;
        }
    }
    return std::nullopt;
}

std::optional<RasterRole> rasterRoleFromWord(std::string_view word)
{
    const std::string lowered = loweredCopy(word);
    for (const RasterRole role : {RasterRole::Imagery, RasterRole::Elevation, RasterRole::Derived}) {
        if (lowered == toString(role)) {
            return role;
        }
    }
    return std::nullopt;
}

// ---- what a project keeps -------------------------------------------------

ReferenceSource sourceOf(const RasterOverlay& raster)
{
    ReferenceSource source;
    source.kind = ReferenceSource::Kind::Raster;
    source.name = raster.name;
    source.source = raster.source;
    source.visible = raster.visible;
    source.opacity = raster.opacity;
    source.role = raster.role;
    source.displayStyle = raster.displayStyle;
    source.derivation = raster.derivation;
    source.sourceUrl = raster.sourceUrl;
    source.licence = raster.licence;
    source.attribution = raster.attribution;
    source.maxPixels = std::max({1, raster.width, raster.height});
    return source;
}

ReferenceSource sourceOf(const PointCloudLayer& cloud)
{
    ReferenceSource source;
    source.kind = ReferenceSource::Kind::PointCloud;
    source.name = cloud.name;
    source.source = cloud.source;
    source.visible = cloud.visible;
    source.colorMode = cloud.colorMode;
    source.pointSize = cloud.pointSize;
    source.budget = std::max<std::uint64_t>(1, cloud.points.size());
    return source;
}

std::string toRecord(const ReferenceSource& source)
{
    nlohmann::ordered_json record{{"version", kRecordVersion},
                                  {"kind", source.kind == ReferenceSource::Kind::Raster
                                               ? "raster"
                                               : "pointcloud"},
                                  {"name", source.name},
                                  {"source", utf8(source.source)},
                                  {"visible", source.visible}};
    if (source.kind == ReferenceSource::Kind::Raster) {
        record["opacity"] = source.opacity;
        record["role"] = toString(source.role);
        record["display"] = toWord(source.displayStyle);
        record["derivation"] = source.derivation;
        record["url"] = source.sourceUrl;
        record["licence"] = source.licence;
        record["attribution"] = source.attribution;
        record["max_pixels"] = source.maxPixels;
    } else {
        record["color"] = toWord(source.colorMode);
        record["point_size"] = source.pointSize;
        record["budget"] = source.budget;
    }
    // Compact, so a record is one line; a line break inside a value is
    // escaped by the JSON itself.
    return record.dump(-1, ' ', false, nlohmann::ordered_json::error_handler_t::replace);
}

Result<ReferenceSource> parseReferenceRecord(std::string_view text)
{
    const auto refused = [&text](const std::string& why) {
        return makeError(ErrorCode::InvalidArgument, "a reference layer's record " + why,
                         std::string(text.substr(0, 200)));
    };
    const nlohmann::json record = nlohmann::json::parse(text, nullptr, false);
    if (record.is_discarded() || !record.is_object()) {
        return refused("is not JSON");
    }
    const int version = record.value("version", 0);
    if (version < 1) {
        return refused("has no version");
    }
    if (version > kRecordVersion) {
        return refused("was written by a newer Katana (version " + std::to_string(version) + ")");
    }
    ReferenceSource source;
    const std::string kind = record.value("kind", std::string());
    if (kind == "raster") {
        source.kind = ReferenceSource::Kind::Raster;
    } else if (kind == "pointcloud") {
        source.kind = ReferenceSource::Kind::PointCloud;
    } else {
        return refused("names no kind of layer this build reads");
    }
    source.name = record.value("name", std::string());
    source.source = pathOf(record.value("source", std::string()));
    if (source.source.empty()) {
        return refused("names no source");
    }
    source.visible = record.value("visible", true);
    source.opacity = std::clamp(record.value("opacity", 1.0), 0.0, 1.0);
    source.role = rasterRoleFromWord(record.value("role", std::string("imagery")))
                      .value_or(RasterRole::Imagery);
    source.displayStyle = displayStyleFromWord(record.value("display", std::string("plain")))
                              .value_or(RasterDisplayStyle::Plain);
    source.derivation = record.value("derivation", std::string());
    source.sourceUrl = record.value("url", std::string());
    source.licence = record.value("licence", std::string());
    source.attribution = record.value("attribution", std::string());
    source.maxPixels = std::max(1, record.value("max_pixels", 4096));
    source.colorMode = pointColorModeFromWord(record.value("color", std::string("elevation")))
                           .value_or(PointColorMode::Elevation);
    source.pointSize = record.value("point_size", 1.0);
    source.budget = std::max<std::uint64_t>(1, record.value("budget", std::uint64_t{2'000'000}));
    return source;
}

std::vector<std::string> referenceRecords(const ReferenceData& reference)
{
    std::vector<std::string> records;
    for (const RasterOverlay& raster : reference.rasters()) {
        records.push_back(toRecord(sourceOf(raster)));
    }
    for (const PointCloudLayer& cloud : reference.pointClouds()) {
        records.push_back(toRecord(sourceOf(cloud)));
    }
    records.insert(records.end(), reference.missing().begin(), reference.missing().end());
    return records;
}

Result<ReferenceLayer> readReference(const ReferenceSource& source)
{
    const std::string name = utf8(source.source);
    const bool onDisk = !name.starts_with("/vsi") && name.find("://") == std::string::npos;
    std::error_code error;
    if (onDisk && !std::filesystem::exists(source.source, error)) {
        return makeError(ErrorCode::NotFound, "the source of reference layer \"" + source.name +
                                                  "\" is gone",
                         name);
    }
    if (source.kind == ReferenceSource::Kind::Raster) {
        RasterImportOptions options;
        options.maxPixels = source.maxPixels;
        options.name = source.name;
        auto raster = importRaster(source.source, options);
        if (!raster) {
            return raster.error();
        }
        raster->name = source.name;
        raster->visible = source.visible;
        raster->opacity = source.opacity;
        raster->role = source.role;
        raster->displayStyle = source.displayStyle;
        raster->derivation = source.derivation;
        raster->sourceUrl = source.sourceUrl;
        raster->licence = source.licence;
        raster->attribution = source.attribution;
        return ReferenceLayer(std::move(*raster));
    }
    PointCloudLayer cloud;
    if (kindForPath(source.source) == SourceKind::Archive12d) {
        // A cloud that came in with a 12d archive: the archive read again,
        // and its cloud of this name taken. The rest of it is the drawing's,
        // which the project already holds.
        auto archive = importArchive12d(source.source);
        if (!archive) {
            return archive.error();
        }
        const auto found = std::ranges::find_if(
            archive->clouds, [&source](const PointCloudLayer& each) { return each.name == source.name; });
        if (found == archive->clouds.end()) {
            return makeError(ErrorCode::NotFound,
                             "the 12d archive holds no point cloud \"" + source.name + "\" now",
                             name);
        }
        cloud = std::move(*found);
    } else {
        PointCloudImportOptions options;
        options.budget = source.budget;
        options.name = source.name;
        auto read = importPointCloud(source.source, options);
        if (!read) {
            return read.error();
        }
        cloud = std::move(*read);
    }
    cloud.name = source.name;
    cloud.visible = source.visible;
    cloud.colorMode = source.colorMode;
    cloud.pointSize = source.pointSize;
    return ReferenceLayer(std::move(cloud));
}

} // namespace katana::interop
