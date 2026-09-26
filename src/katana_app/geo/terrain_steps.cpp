// The raster steps, sources and areas the terrain analysis verbs share
// (terrain_verbs.hpp): CONTOUR, RASTER SHADE and RASTER SLOPE read their
// raster, cut it to the drawing's areas and hand it from one GDAL algorithm
// to the next here, so each verb is its own words and records only.

#include <algorithm>
#include <atomic>
#include <cmath>
#include <filesystem>
#include <fstream>
#include <limits>
#include <memory>
#include <optional>
#include <string>
#include <system_error>
#include <utility>
#include <variant>
#include <vector>

#include "katana/core/text.hpp"
#include "katana/interop/terrain_io.hpp"
#include "replies.hpp"
#include "terrain_verbs.hpp"

namespace katana::app::geo {

namespace gp = katana::gis::processing;
namespace igeo = katana::interop::geo;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

std::filesystem::path pathOf(const std::string& utf8)
{
    return std::filesystem::path(std::u8string(utf8.begin(), utf8.end()));
}

std::string utf8Of(const std::filesystem::path& path)
{
    const std::u8string text = path.generic_u8string();
    return std::string(text.begin(), text.end());
}

std::filesystem::path chainFolder(const std::filesystem::path& scratch)
{
    // One folder per chain, so two jobs of one process never share a file.
    static std::atomic<std::uint64_t> made{0};
    return scratch / ("katana-chain-" + std::to_string(++made));
}

// A ring as WKT's coordinate list, closed as WKT closes it.
std::string ringWkt(const katana::geometry::Polyline2& ring)
{
    std::string text = "(";
    for (const katana::geometry::Point2& vertex : ring.vertices) {
        text += gdalNumber(vertex.x) + " " + gdalNumber(vertex.y) + ",";
    }
    const katana::geometry::Point2& first = ring.vertices.front();
    return text + gdalNumber(first.x) + " " + gdalNumber(first.y) + ")";
}

} // namespace

// ---- RasterChain ----------------------------------------------------------------------------

RasterChain::RasterChain(gp::DatasetValue input, const std::filesystem::path& scratch)
    : current_(std::move(input)), folder_(chainFolder(scratch))
{
}

RasterChain::~RasterChain()
{
    std::error_code error;
    std::filesystem::remove_all(folder_, error);
}

Status RasterChain::step(const std::vector<std::string>& path, std::vector<std::string> tokens,
                         const std::stop_token& stop, const Progress& progress,
                         std::vector<std::pair<std::string, gp::ArgValue>> values)
{
    std::error_code error;
    std::filesystem::create_directories(folder_, error);
    gp::RunRequest request;
    request.path = path;
    request.values = std::move(values);
    request.values.emplace_back("input", gp::ArgValue(current_));
    request.tokens = std::move(tokens);
    // Every raster to a file of the chain's: a DEM is never held whole in
    // memory, and an RGBA picture keeps its colours, which a grid of doubles
    // would not.
    request.outputTo = gp::OutputTo::Memory;
    request.maxMemoryCells = 0;
    request.spillDirectory = utf8Of(folder_);
    auto outputs = gp::run(request, stop, progress);
    if (!outputs) {
        return outputs.error();
    }
    seconds += outputs->seconds;
    warnings.insert(warnings.end(), outputs->diagnostics.begin(), outputs->diagnostics.end());
    if (!outputs->file) {
        return makeError(ErrorCode::CommandRejected,
                         gp::pathText(path) + " wrote no raster", gp::pathText(path));
    }
    gp::DatasetPath next;
    next.path = *outputs->file;
    current_ = std::move(next);
    stepped_ = true;
    return {};
}

Result<gp::FeatureSet> RasterChain::features(const std::vector<std::string>& path,
                                             std::vector<std::string> tokens,
                                             const std::stop_token& stop,
                                             const Progress& progress)
{
    gp::RunRequest request;
    request.path = path;
    request.values.emplace_back("input", gp::ArgValue(current_));
    request.tokens = std::move(tokens);
    request.outputTo = gp::OutputTo::Memory;
    auto outputs = gp::run(request, stop, progress);
    if (!outputs) {
        return outputs.error();
    }
    seconds += outputs->seconds;
    warnings.insert(warnings.end(), outputs->diagnostics.begin(), outputs->diagnostics.end());
    return outputs->features ? std::move(*outputs->features) : gp::FeatureSet{};
}

Result<katana::gis::RasterInfo> RasterChain::info() const
{
    return rasterInfoOf(current_);
}

Result<std::filesystem::path> RasterChain::keep(const std::filesystem::path& destination)
{
    const auto* file = std::get_if<gp::DatasetPath>(&current_);
    if (!stepped_ || file == nullptr) {
        return makeError(ErrorCode::InvalidState, "no step of the chain has written a raster");
    }
    std::error_code error;
    std::filesystem::create_directories(destination.parent_path(), error);
    std::filesystem::path target = destination;
    for (int copy = 2; std::filesystem::exists(target, error); ++copy) {
        target = destination.parent_path() /
                 (destination.stem().string() + "-" + std::to_string(copy) +
                  destination.extension().string());
    }
    const std::filesystem::path from = pathOf(file->path);
    error.clear();
    std::filesystem::rename(from, target, error);
    if (error) {
        // Another volume: copied, and the chain's copy goes with its folder.
        error.clear();
        std::filesystem::copy_file(from, target, error);
        if (error) {
            return makeError(ErrorCode::FileExportFailure,
                             "could not keep the raster: " + error.message(), utf8Of(target));
        }
    }
    gp::DatasetPath kept;
    kept.path = utf8Of(target);
    current_ = std::move(kept);
    return target;
}

Result<std::filesystem::path> RasterChain::write(const std::string& name, const std::string& text)
{
    std::error_code error;
    std::filesystem::create_directories(folder_, error);
    const std::filesystem::path file = folder_ / name;
    std::ofstream stream(file, std::ios::binary | std::ios::trunc);
    stream << text;
    stream.close();
    if (!stream) {
        return makeError(ErrorCode::FileExportFailure, "could not write the chain's file",
                         utf8Of(file));
    }
    return file;
}

// ---- facts, sources, areas ---------------------------------------------------------------

Result<katana::gis::RasterInfo> rasterInfoOf(const gp::DatasetValue& dataset)
{
    if (const auto* grid = std::get_if<gp::RasterGrid>(&dataset)) {
        katana::gis::RasterInfo info = grid->info;
        if (!grid->noData.empty() && grid->noData.front()) {
            info.noDataValue = grid->noData.front();
        }
        return info;
    }
    if (const auto* file = std::get_if<gp::DatasetPath>(&dataset)) {
        auto opened = katana::gis::GdalDataset::open(pathOf(file->path));
        if (!opened) {
            return opened.error();
        }
        return (*opened)->rasterInfo();
    }
    return makeError(ErrorCode::InvalidArgument, "features are no raster");
}

std::optional<double> squareCellOf(const katana::gis::RasterInfo& info)
{
    const auto& gt = info.geotransform;
    if (!info.hasGeotransform || gt[2] != 0.0 || gt[4] != 0.0 ||
        std::abs(gt[1]) != std::abs(gt[5])) {
        return std::nullopt;
    }
    return std::abs(gt[1]);
}

katana::geometry::Box2 rasterExtent(const katana::gis::RasterInfo& info)
{
    const auto& gt = info.geotransform;
    katana::geometry::Box2 box;
    for (const auto& [px, py] : {std::pair{0.0, 0.0}, std::pair{1.0, 0.0}, std::pair{0.0, 1.0},
                                 std::pair{1.0, 1.0}}) {
        const double column = px * info.width;
        const double row = py * info.height;
        box.expand(katana::geometry::Point2(gt[0] + column * gt[1] + row * gt[2],
                                            gt[3] + column * gt[4] + row * gt[5]));
    }
    return box;
}

Result<std::optional<std::pair<double, double>>> valueRange(const gp::DatasetValue& dataset,
                                                           std::size_t maxSamples)
{
    double low = std::numeric_limits<double>::infinity();
    double high = -low;
    const auto take = [&](const std::vector<double>& values, const std::optional<double>& noData) {
        for (const double value : values) {
            if (!std::isfinite(value) || (noData && value == *noData)) {
                continue;
            }
            low = std::min(low, value);
            high = std::max(high, value);
        }
    };
    if (const auto* grid = std::get_if<gp::RasterGrid>(&dataset)) {
        if (!grid->bands.empty()) {
            take(grid->bands.front(), grid->noData.empty() ? std::nullopt : grid->noData.front());
        }
    } else if (const auto* file = std::get_if<gp::DatasetPath>(&dataset)) {
        auto opened = katana::gis::GdalDataset::open(pathOf(file->path));
        if (!opened) {
            return opened.error();
        }
        auto info = (*opened)->rasterInfo();
        if (!info) {
            return info.error();
        }
        const double cells = static_cast<double>(info->width) * static_cast<double>(info->height);
        const int stride =
            maxSamples == 0
                ? 1
                : std::max(1, static_cast<int>(std::ceil(
                                  std::sqrt(cells / static_cast<double>(maxSamples)))));
        auto samples = (*opened)->readBandSampled(1, stride);
        if (!samples) {
            return samples.error();
        }
        take(samples->values, samples->noDataValue);
    } else {
        return makeError(ErrorCode::InvalidArgument, "features are no raster");
    }
    if (!(low <= high)) {
        return std::optional<std::pair<double, double>>{};
    }
    return std::optional<std::pair<double, double>>{std::pair{low, high}};
}

Result<TerrainSource> bindTerrainSource(Context& context, const Tokens& tokens, std::size_t& at,
                                        std::string_view verb)
{
    if (at >= tokens.size() ||
        (!tokens.is(at, "RASTER") && !tokens.is(at, "SURFACE") && !tokens.is(at, "FILE"))) {
        return makeError(ErrorCode::InvalidArgument,
                         std::string(verb) +
                             " reads a SURFACE <name>, a RASTER <id|name> or a FILE <path>; a "
                             "scope of the drawing comes after it",
                         at < tokens.size() ? tokens[at] : std::string());
    }
    auto source = parseSource(tokens, at);
    if (!source) {
        return source.error();
    }
    TerrainSource bound;
    bound.source = std::move(source).value();
    if (bound.source.kind == Source::Kind::Surface) {
        const katana::terrain::NamedSurface* named = context.surfaces.find(bound.source.surface);
        if (named == nullptr) {
            return makeError(ErrorCode::NotFound, "no surface has that name; SURFACE LIST names them",
                             bound.source.surface);
        }
        bound.surface = named->surface;
        bound.cellGiven = bound.source.cell.has_value();
        if (!bound.source.cell) {
            // Chosen now, so the reply says the cell the surface was read at.
            bound.source.cell = katana::interop::suggestedCellSize(named->surface->bounds());
        }
    }
    auto dataset = bindRaster(context, bound.source);
    if (!dataset) {
        return dataset.error();
    }
    bound.dataset = std::move(dataset).value();
    bound.record = inputRecord("input", bound.source);
    return bound;
}

Result<ScopeAreas> bindAreas(Context& context, const katana::cad::ScopeWords& scope,
                             std::string_view arg)
{
    igeo::DrawingDatasetOptions options;
    options.crsWkt = projectCrs(context);
    auto bound = bindDrawing(context, scope, options);
    if (!bound) {
        return bound.error();
    }
    ScopeAreas areas;
    areas.areas = igeo::boundariesOf(bound->dataset.set);
    for (const igeo::ContourBoundary& area : areas.areas) {
        for (const katana::geometry::Polyline2& ring : area.rings) {
            for (const katana::geometry::Point2& vertex : ring.vertices) {
                areas.box.expand(vertex);
            }
        }
    }
    const igeo::DrawingDatasetStats& stats = bound->dataset.stats;
    areas.records.push_back(scopeRecord(arg, *bound));
    areas.records.push_back("areas used=" + std::to_string(areas.areas.size()) +
                            " skipped.open=" + std::to_string(stats.lines) +
                            " skipped.points=" + std::to_string(stats.points));
    if (stats.lines + stats.points != 0) {
        areas.records.push_back(warningRecord(
            std::to_string(stats.lines + stats.points) +
            " of what the scope took are not closed shapes, so bound no area, and were left out"));
    }
    for (const std::string& warning : stats.warnings) {
        areas.records.push_back(warningRecord(warning));
    }
    return areas;
}

std::string areasWkt(const std::vector<igeo::ContourBoundary>& areas)
{
    std::string text = "MULTIPOLYGON(";
    for (std::size_t a = 0; a < areas.size(); ++a) {
        text += a == 0 ? "(" : ",(";
        for (std::size_t r = 0; r < areas[a].rings.size(); ++r) {
            text += (r == 0 ? "" : ",") + ringWkt(areas[a].rings[r]);
        }
        text += ")";
    }
    return text + ")";
}

std::string gdalNumber(double value)
{
    return katana::core::formatExactReal(value);
}

} // namespace katana::app::geo
