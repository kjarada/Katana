// What the DEM verbs share (dem_support.hpp).

#include "dem_support.hpp"

#include <algorithm>
#include <cmath>
#include <memory>
#include <string>
#include <utility>
#include <variant>

#include "katana/core/text.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/interop/geo/raster_products.hpp"
#include "replies.hpp"

namespace katana::app::geo {

namespace gp = katana::gis::processing;
namespace igeo = katana::interop::geo;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

std::filesystem::path pathOfUtf8(const std::string& utf8)
{
    return std::filesystem::path(std::u8string(utf8.begin(), utf8.end()));
}

std::string utf8OfPath(const std::filesystem::path& path)
{
    const std::u8string text = path.generic_u8string();
    return std::string(text.begin(), text.end());
}

Result<VerbWords> splitOptions(const Tokens& tokens, std::size_t from,
                               std::span<const std::string_view> keys, std::string_view verb)
{
    VerbWords words;
    for (std::size_t i = from; i < tokens.size(); ++i) {
        const std::string& word = tokens[i];
        const std::size_t equals = word.find('=');
        std::string key;
        if (!tokens.quoted[i] && equals != std::string::npos && equals != 0) {
            key = katana::core::lowered(std::string_view(word).substr(0, equals));
        }
        const bool option =
            !key.empty() && std::ranges::any_of(keys, [&](std::string_view k) { return k == key; });
        if (!option) {
            words.rest.words.push_back(word);
            words.rest.quoted.push_back(tokens.quoted[i]);
            continue;
        }
        const std::string text = word.substr(equals + 1);
        if (text.empty()) {
            return makeError(ErrorCode::InvalidArgument,
                             std::string(verb) + ": " + key + "= needs a value", word);
        }
        if (!words.options.emplace(key, text).second) {
            return makeError(ErrorCode::InvalidArgument,
                             std::string(verb) + " takes " + key + "= once", word);
        }
    }
    return words;
}

Result<double> positiveOption(std::string_view key, const std::string& text)
{
    const auto number = katana::core::parseFiniteDouble(text);
    if (!number || !(*number > 0.0)) {
        return makeError(ErrorCode::InvalidArgument,
                         std::string(key) + "= is a positive number", std::string(key) + "=" + text);
    }
    return *number;
}

Result<std::array<double, 4>> boxOption(std::string_view key, const std::string& text)
{
    std::array<double, 4> box{};
    std::size_t start = 0;
    for (std::size_t i = 0; i < box.size(); ++i) {
        const std::size_t comma = text.find(',', start);
        const bool last = i + 1 == box.size();
        if ((comma == std::string::npos) != last) {
            return makeError(ErrorCode::InvalidArgument,
                             std::string(key) + "= is four numbers, x0,y0,x1,y1",
                             std::string(key) + "=" + text);
        }
        const auto number = katana::core::parseFiniteDouble(
            std::string_view(text).substr(start, last ? std::string::npos : comma - start));
        if (!number) {
            return makeError(ErrorCode::InvalidArgument,
                             std::string(key) + "= is four numbers, x0,y0,x1,y1",
                             std::string(key) + "=" + text);
        }
        box[i] = *number;
        start = comma + 1;
    }
    // Min before max, whichever corners were typed first, as AREA reads them.
    if (box[0] > box[2]) {
        std::swap(box[0], box[2]);
    }
    if (box[1] > box[3]) {
        std::swap(box[1], box[3]);
    }
    if (!(box[2] > box[0]) || !(box[3] > box[1])) {
        return makeError(ErrorCode::InvalidArgument, std::string(key) + "= encloses no area",
                         std::string(key) + "=" + text);
    }
    return box;
}

std::string joinRecords(const std::vector<std::string>& records)
{
    std::string text;
    for (const std::string& record : records) {
        if (!record.empty()) {
            text += (text.empty() ? "" : "\n") + record;
        }
    }
    return text;
}

Result<BandValues> readBandOne(const std::filesystem::path& file)
{
    auto dataset = katana::gis::GdalDataset::open(file);
    if (!dataset) {
        return dataset.error();
    }
    auto info = (*dataset)->rasterInfo();
    if (!info) {
        return info.error();
    }
    auto values = (*dataset)->readBand(1);
    if (!values) {
        return values.error();
    }
    BandValues band;
    band.width = info->width;
    band.height = info->height;
    band.geotransform = info->geotransform;
    band.noData = info->noDataValue;
    band.values = std::move(values).value();
    return band;
}

Result<BandValues> readBandOne(const gp::DatasetValue& dataset)
{
    if (const auto* grid = std::get_if<gp::RasterGrid>(&dataset)) {
        if (grid->bands.empty()) {
            return makeError(ErrorCode::InvalidArgument, "the raster has no band");
        }
        BandValues band;
        band.width = grid->info.width;
        band.height = grid->info.height;
        band.geotransform = grid->info.geotransform;
        band.noData = grid->noData.empty() ? std::nullopt : grid->noData.front();
        band.values = grid->bands.front();
        return band;
    }
    if (const auto* path = std::get_if<gp::DatasetPath>(&dataset)) {
        return readBandOne(pathOfUtf8(path->path));
    }
    return makeError(ErrorCode::InvalidArgument, "a feature set is not a raster");
}

bool holdsValue(const BandValues& band, double cell)
{
    // A no-data value is compared exactly, as GDAL compares it: it is the
    // bit pattern the file holds, not a measurement.
    return std::isfinite(cell) && !(band.noData && cell == *band.noData);
}

double cellArea(const std::array<double, 6>& geotransform)
{
    return std::abs(geotransform[1] * geotransform[5] - geotransform[2] * geotransform[4]);
}

Result<std::string> keepInPlace(Context& context, const std::filesystem::path& file,
                                const std::string& name, const std::string& derivation,
                                std::string_view arg)
{
    const auto taken = [&](const std::string& candidate) {
        return std::ranges::any_of(context.reference.rasters(),
                                   [&](const katana::interop::RasterOverlay& raster) {
                                       return katana::core::equalsIgnoringCase(raster.name, candidate);
                                   });
    };
    std::string unique = name;
    for (int copy = 2; taken(unique); ++copy) {
        unique = name + "-" + std::to_string(copy);
    }
    auto overlay = igeo::derivedOverlay(file, unique, derivation);
    if (!overlay) {
        return overlay.error();
    }
    const int width = overlay->width;
    const int height = overlay->height;
    const katana::geometry::Box2 bounds = overlay->worldBounds();
    const katana::interop::ReferenceId id = context.reference.add(std::move(*overlay));
    if (context.changed) {
        context.changed();
    }
    if (context.frame && !bounds.empty()) {
        context.frame(bounds);
    }
    return "output arg=" + value(arg) + " kind=raster target=reference id=" + std::to_string(id) +
           " name=" + value(unique) + " raster=" + std::to_string(width) + "x" +
           std::to_string(height) + " file=" + value(utf8OfPath(file)) + " persisted=yes";
}

} // namespace katana::app::geo
