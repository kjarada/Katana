#include "katana/gis/reproject.hpp"

#include <algorithm>
#include <atomic>
#include <cmath>
#include <filesystem>
#include <memory>
#include <random>
#include <string>
#include <system_error>
#include <utility>
#include <vector>

#include <cpl_conv.h>
#include <cpl_error.h>
#include <cpl_string.h>
#include <gdal_priv.h>
#include <gdal_utils.h>
#include <ogr_spatialref.h>

#include "gdal_registry.hpp"

namespace katana::gis {
namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

// No-data for a warped elevation: below any ground on Earth (the Dead Sea
// shore is -430 m, the deepest trench about -11 000 m) and exactly
// representable in Float32, so nothing real is ever mistaken for it.
constexpr double kElevationNoData = -32767.0;

// Reads a CRS the way the rest of katana_io does (gdal_adapter.cpp,
// parseCrs): with the network and file access that SetFromUserInput would
// otherwise allow switched off, and in traditional GIS order.
bool readCrs(const std::string& text, OGRSpatialReference& reference)
{
    reference.SetAxisMappingStrategy(OAMS_TRADITIONAL_GIS_ORDER);
    if (text.empty()) {
        return false;
    }
    if (reference.SetFromUserInput(
            text.c_str(), OGRSpatialReference::SET_FROM_USER_INPUT_LIMITATIONS_get()) ==
        OGRERR_NONE) {
        return true;
    }
    return reference.importFromWkt(text.c_str()) == OGRERR_NONE;
}

Result<OGRSpatialReference> crsOf(const std::string& text)
{
    detail::ensureGdalRegistered();
    OGRSpatialReference reference;
    if (!readCrs(text, reference)) {
        return makeError(ErrorCode::InvalidCRS, "GDAL cannot read this coordinate system",
                         text.substr(0, 200));
    }
    return reference;
}

struct TransformDeleter {
    void operator()(OGRCoordinateTransformation* transform) const
    {
        OGRCoordinateTransformation::DestroyCT(transform);
    }
};
using Transform = std::unique_ptr<OGRCoordinateTransformation, TransformDeleter>;

Result<Transform> makeTransform(const std::string& from, const std::string& to)
{
    auto source = crsOf(from);
    if (!source) {
        return source.error();
    }
    auto target = crsOf(to);
    if (!target) {
        return target.error();
    }
    CPLPushErrorHandler(CPLQuietErrorHandler);
    Transform transform(OGRCreateCoordinateTransformation(&*source, &*target));
    CPLPopErrorHandler();
    if (!transform) {
        return makeError(ErrorCode::InvalidCRS, "PROJ has no transformation between these systems",
                         from.substr(0, 100) + " -> " + to.substr(0, 100));
    }
    return transform;
}

// Sets GDAL configuration for the calling thread only and puts it back when
// the scope ends, so a warp's User-Agent and timeouts cannot leak into
// another thread's reads.
class ThreadConfig {
  public:
    void set(const char* key, const std::string& value)
    {
        const char* previous = CPLGetThreadLocalConfigOption(key, nullptr);
        saved_.emplace_back(key, previous != nullptr ? std::optional<std::string>(previous)
                                                     : std::nullopt);
        CPLSetThreadLocalConfigOption(key, value.c_str());
    }
    ~ThreadConfig()
    {
        for (auto it = saved_.rbegin(); it != saved_.rend(); ++it) {
            CPLSetThreadLocalConfigOption(it->first,
                                          it->second ? it->second->c_str() : nullptr);
        }
    }

  private:
    std::vector<std::pair<const char*, std::optional<std::string>>> saved_;
};

struct ProgressState {
    const std::stop_token* stop = nullptr;
    const std::function<void(double)>* progress = nullptr;
};

int warpProgress(double fraction, const char*, void* state)
{
    auto& shared = *static_cast<ProgressState*>(state);
    if (shared.progress != nullptr && *shared.progress) {
        (*shared.progress)(fraction);
    }
    return shared.stop != nullptr && shared.stop->stop_requested() ? FALSE : TRUE;
}

std::string number(double value)
{
    char buffer[64];
    std::snprintf(buffer, sizeof buffer, "%.17g", value);
    return buffer;
}

struct DatasetCloser {
    void operator()(GDALDatasetH dataset) const
    {
        if (dataset != nullptr) {
            GDALClose(dataset);
        }
    }
};
using DatasetHandle = std::unique_ptr<void, DatasetCloser>;

// A source as the warp should see it: an image a service returned for a box
// becomes an in-memory VRT carrying that box and CRS, and an imagery source
// becomes four bands, red, green, blue and alpha, whatever it was - a
// paletted PNG from an ArcGIS export, a grey JPEG, an RGB COG - because one
// warp cannot mix band counts.
//
// The VRT refers to the opened source rather than reopening it by name, so
// the source must outlive it: it is handed to `keepOpen`, which the caller
// destroys after the VRT (a VRT read after its source was closed crashed
// GDAL's overview lookup, found by the file-URL test).
Result<DatasetHandle> prepareSource(const WarpSource& source, bool elevation,
                                    std::vector<DatasetHandle>& keepOpen)
{
    CPLPushErrorHandler(CPLQuietErrorHandler);
    DatasetHandle opened(GDALOpenEx(source.path.c_str(), GDAL_OF_RASTER | GDAL_OF_READONLY,
                                    nullptr, nullptr, nullptr));
    CPLPopErrorHandler();
    if (!opened) {
        return makeError(ErrorCode::FileImportFailure, "GDAL could not open a downloaded raster",
                         std::string(CPLGetLastErrorMsg()));
    }
    auto* dataset = static_cast<GDALDataset*>(opened.get());
    const int bands = dataset->GetRasterCount();
    if (bands < 1) {
        return makeError(ErrorCode::FileImportFailure, "a downloaded raster has no bands");
    }

    CPLStringList arguments;
    arguments.AddString("-of");
    arguments.AddString("VRT");
    if (source.bounds) {
        const CrsBox& box = *source.bounds;
        arguments.AddString("-a_ullr");
        arguments.AddString(number(box.minX).c_str());
        arguments.AddString(number(box.maxY).c_str());
        arguments.AddString(number(box.maxX).c_str());
        arguments.AddString(number(box.minY).c_str());
        auto wkt = crsToWkt(source.crs);
        if (!wkt) {
            return wkt.error();
        }
        arguments.AddString("-a_srs");
        arguments.AddString(wkt->c_str());
    }
    if (elevation) {
        arguments.AddString("-b");
        arguments.AddString("1");
        // An image a service returned for a box may not declare the no-data
        // it was asked to use (exportImage's noData=-32767): declared here,
        // so the warp does not blend it into real heights at the edges.
        int hasNoData = FALSE;
        dataset->GetRasterBand(1)->GetNoDataValue(&hasNoData);
        if (hasNoData == FALSE && source.bounds) {
            arguments.AddString("-a_nodata");
            arguments.AddString(number(kElevationNoData).c_str());
        }
    } else {
        GDALRasterBand* first = dataset->GetRasterBand(1);
        const bool paletted = first->GetColorTable() != nullptr;
        const bool hasAlpha =
            bands >= 4 || (bands == 2 && dataset->GetRasterBand(2)->GetColorInterpretation() ==
                                             GCI_AlphaBand);
        if (paletted) {
            arguments.AddString("-expand");
            arguments.AddString("rgba");
        } else {
            const int red = 1;
            const int green = bands >= 3 ? 2 : 1;
            const int blue = bands >= 3 ? 3 : 1;
            for (const int band : {red, green, blue}) {
                arguments.AddString("-b");
                arguments.AddString(std::to_string(band).c_str());
            }
            arguments.AddString("-b");
            if (hasAlpha) {
                arguments.AddString(bands >= 4 ? "4" : "2");
            } else {
                // The dataset's mask: its no-data, or everything valid.
                arguments.AddString("mask");
            }
            arguments.AddString("-colorinterp");
            arguments.AddString("red,green,blue,alpha");
            // A 16-bit or float RGB is not display imagery; it is scaled by
            // buildTrueColourVrt before it gets here. Bytes are passed as is.
            arguments.AddString("-ot");
            arguments.AddString("Byte");
        }
    }
    GDALTranslateOptions* options = GDALTranslateOptionsNew(arguments.List(), nullptr);
    CPLPushErrorHandler(CPLQuietErrorHandler);
    DatasetHandle prepared(GDALTranslate("", opened.get(), options, nullptr));
    CPLPopErrorHandler();
    GDALTranslateOptionsFree(options);
    if (!prepared) {
        return makeError(ErrorCode::FileImportFailure, "a downloaded raster could not be read",
                         std::string(CPLGetLastErrorMsg()));
    }
    keepOpen.push_back(std::move(opened));
    return prepared;
}

} // namespace

Result<std::string> crsToWkt(const std::string& crs)
{
    auto reference = crsOf(crs);
    if (!reference) {
        return reference.error();
    }
    char* wkt = nullptr;
    const char* const options[] = {"FORMAT=WKT2_2019", nullptr};
    if (reference->exportToWkt(&wkt, options) != OGRERR_NONE || wkt == nullptr) {
        CPLFree(wkt);
        return makeError(ErrorCode::InvalidCRS, "GDAL could not write this coordinate system",
                         crs.substr(0, 200));
    }
    std::string text = wkt;
    CPLFree(wkt);
    return text;
}

Result<bool> crsAxisIsYX(const std::string& crs)
{
    auto reference = crsOf(crs);
    if (!reference) {
        return reference.error();
    }
    // The AUTHORITY's order, which a traditional-GIS-order mapping hides:
    // the data axis mapping says where the CRS's first axis goes, and a
    // swap means the CRS itself is northing/latitude first.
    const std::vector<int> mapping = reference->GetDataAxisToSRSAxisMapping();
    return mapping.size() >= 2 && mapping[0] == 2 && mapping[1] == 1;
}

Result<bool> crsIsGeographic(const std::string& crs)
{
    auto reference = crsOf(crs);
    if (!reference) {
        return reference.error();
    }
    return reference->IsGeographic() != 0;
}

std::optional<int> crsEpsgCode(const std::string& crs)
{
    auto reference = crsOf(crs);
    if (!reference) {
        return std::nullopt;
    }
    if (reference->AutoIdentifyEPSG() != OGRERR_NONE) {
        // AutoIdentifyEPSG knows the common cases only; a WKT that
        // FindMatches ties to one EPSG entry with full confidence counts too.
        int count = 0;
        int* confidence = nullptr;
        OGRSpatialReferenceH* matches =
            reference->FindMatches(nullptr, &count, &confidence);
        std::optional<int> code;
        if (count >= 1 && confidence != nullptr && confidence[0] == 100) {
            const auto* best = OGRSpatialReference::FromHandle(matches[0]);
            const char* authority = best->GetAuthorityName(nullptr);
            const char* value = best->GetAuthorityCode(nullptr);
            if (authority != nullptr && value != nullptr && std::string(authority) == "EPSG") {
                code = std::atoi(value);
            }
        }
        OSRFreeSRSArray(matches);
        CPLFree(confidence);
        return code;
    }
    const char* authority = reference->GetAuthorityName(nullptr);
    const char* value = reference->GetAuthorityCode(nullptr);
    if (authority == nullptr || value == nullptr || std::string(authority) != "EPSG") {
        return std::nullopt;
    }
    return std::atoi(value);
}

Result<CrsBox> transformBox(const CrsBox& box, const std::string& fromCrs,
                            const std::string& toCrs)
{
    if (!box.valid()) {
        return makeError(ErrorCode::InvalidArgument, "the area is empty or inverted",
                         number(box.minX) + "," + number(box.minY) + " " + number(box.maxX) +
                             "," + number(box.maxY));
    }
    auto transform = makeTransform(fromCrs, toCrs);
    if (!transform) {
        return transform.error();
    }
    CrsBox out;
    CPLPushErrorHandler(CPLQuietErrorHandler);
    const int ok = (*transform)->TransformBounds(box.minX, box.minY, box.maxX, box.maxY,
                                                 &out.minX, &out.minY, &out.maxX, &out.maxY, 21);
    CPLPopErrorHandler();
    if (ok == FALSE || !std::isfinite(out.minX) || !std::isfinite(out.maxY) || !out.valid()) {
        return makeError(ErrorCode::InvalidCRS,
                         "the area cannot be expressed in the service's coordinate system",
                         std::string(CPLGetLastErrorMsg()));
    }
    return out;
}

Result<std::array<double, 2>> transformPoint(double x, double y, const std::string& fromCrs,
                                             const std::string& toCrs)
{
    auto transform = makeTransform(fromCrs, toCrs);
    if (!transform) {
        return transform.error();
    }
    double px = x;
    double py = y;
    if ((*transform)->Transform(1, &px, &py) == FALSE || !std::isfinite(px) ||
        !std::isfinite(py)) {
        return makeError(ErrorCode::InvalidCRS, "the point cannot be transformed",
                         number(x) + "," + number(y));
    }
    return std::array<double, 2>{px, py};
}

Result<std::vector<VectorFeature>> reprojectFeatures(std::vector<VectorFeature> features,
                                                     const std::string& fromCrs,
                                                     const std::string& toCrs)
{
    auto transform = makeTransform(fromCrs, toCrs);
    if (!transform) {
        return transform.error();
    }
    std::vector<double> xs;
    std::vector<double> ys;
    for (std::size_t index = 0; index < features.size(); ++index) {
        for (std::vector<GeoPoint>& part : features[index].geometry.parts) {
            xs.resize(part.size());
            ys.resize(part.size());
            for (std::size_t i = 0; i < part.size(); ++i) {
                xs[i] = part[i].x;
                ys[i] = part[i].y;
            }
            std::vector<int> success(part.size(), FALSE);
            (*transform)->Transform(part.size(), xs.data(), ys.data(), nullptr, success.data());
            for (std::size_t i = 0; i < part.size(); ++i) {
                if (success[i] == FALSE || !std::isfinite(xs[i]) || !std::isfinite(ys[i])) {
                    return makeError(ErrorCode::InvalidCRS,
                                     "a feature lies outside where the project's coordinate "
                                     "system is defined",
                                     "feature " + std::to_string(index + 1) + " vertex " +
                                         number(part[i].x) + "," + number(part[i].y));
                }
                part[i].x = xs[i];
                part[i].y = ys[i];
            }
        }
    }
    return features;
}

Result<RasterProbe> probeRaster(const std::string& path, const std::string& userAgent,
                                int timeoutSeconds)
{
    detail::ensureGdalRegistered();
    ThreadConfig config;
    if (!userAgent.empty()) {
        config.set("GDAL_HTTP_USERAGENT", userAgent);
    }
    config.set("GDAL_HTTP_TIMEOUT", std::to_string(std::max(1, timeoutSeconds)));
    config.set("GDAL_DISABLE_READDIR_ON_OPEN", "EMPTY_DIR");
    CPLPushErrorHandler(CPLQuietErrorHandler);
    DatasetHandle opened(
        GDALOpenEx(path.c_str(), GDAL_OF_RASTER | GDAL_OF_READONLY, nullptr, nullptr, nullptr));
    CPLPopErrorHandler();
    if (!opened) {
        return makeError(ErrorCode::FileImportFailure, "GDAL could not open the raster",
                         std::string(CPLGetLastErrorMsg()));
    }
    auto* dataset = static_cast<GDALDataset*>(opened.get());
    RasterProbe probe;
    probe.width = dataset->GetRasterXSize();
    probe.height = dataset->GetRasterYSize();
    probe.bandCount = dataset->GetRasterCount();
    if (probe.bandCount > 0) {
        probe.dataType = GDALGetDataTypeName(dataset->GetRasterBand(1)->GetRasterDataType());
    }
    if (const OGRSpatialReference* reference = dataset->GetSpatialRef()) {
        char* wkt = nullptr;
        if (reference->exportToWkt(&wkt) == OGRERR_NONE && wkt != nullptr) {
            probe.crsWkt = wkt;
        }
        CPLFree(wkt);
    }
    double geotransform[6] = {};
    if (!probe.crsWkt.empty() && dataset->GetGeoTransform(geotransform) == CE_None) {
        CrsBox native;
        const double x0 = geotransform[0];
        const double x1 = geotransform[0] + geotransform[1] * probe.width;
        const double y0 = geotransform[3];
        const double y1 = geotransform[3] + geotransform[5] * probe.height;
        native.minX = std::min(x0, x1);
        native.maxX = std::max(x0, x1);
        native.minY = std::min(y0, y1);
        native.maxY = std::max(y0, y1);
        if (auto box = transformBox(native, probe.crsWkt, "OGC:CRS84")) {
            probe.lonLatBounds = *box;
        }
    }
    return probe;
}

Result<WarpResult> warpToGeoTiff(const std::vector<WarpSource>& sources, const WarpOptions& options,
                                 const std::filesystem::path& output, const std::stop_token& stop,
                                 const std::function<void(double)>& progress)
{
    detail::ensureGdalRegistered();
    if (sources.empty()) {
        return makeError(ErrorCode::InvalidArgument, "there is nothing to warp");
    }
    if (!options.targetBounds.valid()) {
        return makeError(ErrorCode::InvalidArgument, "the area is empty or inverted");
    }
    if (!(options.resolution > 0.0) || !std::isfinite(options.resolution)) {
        return makeError(ErrorCode::InvalidArgument, "the ground resolution must be positive",
                         number(options.resolution));
    }
    const double columns = std::ceil(options.targetBounds.width() / options.resolution);
    const double rows = std::ceil(options.targetBounds.height() / options.resolution);
    if (columns * rows > static_cast<double>(options.maxPixels)) {
        const double coarser =
            options.resolution * std::sqrt(columns * rows / static_cast<double>(options.maxPixels));
        char advice[256];
        std::snprintf(advice, sizeof advice,
                      "the result would be %.0f x %.0f pixels, more than the %llu allowed; "
                      "use a resolution of %.3g or coarser, or a smaller area",
                      columns, rows, static_cast<unsigned long long>(options.maxPixels),
                      std::ceil(coarser * 1000.0) / 1000.0);
        return makeError(ErrorCode::InvalidArgument, advice);
    }
    auto targetWkt = crsToWkt(options.targetCrs);
    if (!targetWkt) {
        return targetWkt.error();
    }

    ThreadConfig config;
    if (!options.userAgent.empty()) {
        config.set("GDAL_HTTP_USERAGENT", options.userAgent);
    }
    config.set("GDAL_HTTP_TIMEOUT", std::to_string(std::max(1, options.timeoutSeconds)));
    config.set("GDAL_HTTP_CONNECTTIMEOUT", "20");
    config.set("GDAL_HTTP_MAX_RETRY", "3");
    config.set("GDAL_HTTP_RETRY_DELAY", "1");
    // A COG is one file: listing its directory to look for sidecars costs a
    // request per open and finds nothing.
    config.set("GDAL_DISABLE_READDIR_ON_OPEN", "EMPTY_DIR");
    config.set("CPL_VSIL_CURL_ALLOWED_EXTENSIONS", ".tif,.tiff,.TIF,.vrt,.jp2");

    WarpResult result;
    // Declared first, destroyed last: the prepared VRTs read from these.
    std::vector<DatasetHandle> opened;
    std::vector<DatasetHandle> prepared;
    for (const WarpSource& source : sources) {
        if (stop.stop_requested()) {
            return makeError(ErrorCode::InvalidState, "cancelled");
        }
        auto dataset = prepareSource(source, options.elevation, opened);
        if (!dataset) {
            if (options.skipMissingSources) {
                result.warnings.push_back("skipped a source that could not be opened: " +
                                          dataset.error().context);
                continue;
            }
            return dataset.error();
        }
        prepared.push_back(std::move(*dataset));
    }
    if (prepared.empty()) {
        return makeError(ErrorCode::NotFound, "no source covers the area");
    }
    result.sourcesUsed = static_cast<int>(prepared.size());

    CPLStringList arguments;
    const auto add = [&](const std::string& value) { arguments.AddString(value.c_str()); };
    add("-of");
    add("GTiff");
    add("-t_srs");
    add(*targetWkt);
    add("-te");
    add(number(options.targetBounds.minX));
    add(number(options.targetBounds.minY));
    add(number(options.targetBounds.minX + columns * options.resolution));
    add(number(options.targetBounds.minY + rows * options.resolution));
    add("-ts");
    add(std::to_string(static_cast<long long>(columns)));
    add(std::to_string(static_cast<long long>(rows)));
    add("-co");
    add("COMPRESS=DEFLATE");
    add("-co");
    add("TILED=YES");
    add("-overwrite");
    if (options.elevation) {
        add("-ot");
        add("Float32");
        add("-r");
        add("bilinear");
        add("-dstnodata");
        add(number(kElevationNoData));
    } else {
        add("-r");
        add("cubic");
        add("-srcalpha");
        add("-dstalpha");
    }
    GDALWarpAppOptions* warpOptions = GDALWarpAppOptionsNew(arguments.List(), nullptr);
    ProgressState shared{&stop, &progress};
    GDALWarpAppOptionsSetProgress(warpOptions, warpProgress, &shared);

    std::vector<GDALDatasetH> handles;
    handles.reserve(prepared.size());
    for (const DatasetHandle& dataset : prepared) {
        handles.push_back(dataset.get());
    }
    const std::string target = output.string();
    std::error_code ignored;
    std::filesystem::create_directories(output.parent_path(), ignored);
    int usageError = FALSE;
    CPLErrorReset();
    CPLPushErrorHandler(CPLQuietErrorHandler);
    GDALDatasetH warped = GDALWarp(target.c_str(), nullptr, static_cast<int>(handles.size()),
                                   handles.data(), warpOptions, &usageError);
    CPLPopErrorHandler();
    GDALWarpAppOptionsFree(warpOptions);
    const std::string warpError = CPLGetLastErrorMsg();
    if (warped == nullptr || stop.stop_requested()) {
        if (warped != nullptr) {
            GDALClose(warped);
        }
        std::filesystem::remove(output, ignored);
        if (stop.stop_requested()) {
            return makeError(ErrorCode::InvalidState, "cancelled");
        }
        return makeError(ErrorCode::FileImportFailure, "GDAL could not warp the downloaded data",
                         warpError);
    }
    result.width = GDALGetRasterXSize(warped);
    result.height = GDALGetRasterYSize(warped);
    GDALClose(warped);
    return result;
}

Status clipVectorFile(const std::filesystem::path& input, const CrsBox& box,
                      const std::filesystem::path& output)
{
    detail::ensureGdalRegistered();
    if (!box.valid()) {
        return makeError(ErrorCode::InvalidArgument, "the clip box is empty or inverted");
    }
    CPLPushErrorHandler(CPLQuietErrorHandler);
    DatasetHandle source(GDALOpenEx(input.string().c_str(), GDAL_OF_VECTOR | GDAL_OF_READONLY,
                                    nullptr, nullptr, nullptr));
    CPLPopErrorHandler();
    if (!source) {
        return makeError(ErrorCode::FileImportFailure, "GDAL could not open the downloaded file",
                         std::string(CPLGetLastErrorMsg()));
    }
    CPLStringList arguments;
    const auto add = [&](const std::string& value) { arguments.AddString(value.c_str()); };
    add("-f");
    add("GPKG");
    add("-clipsrc");
    add(number(box.minX));
    add(number(box.minY));
    add(number(box.maxX));
    add(number(box.maxY));
    // A polygon cut by the box can become several; keep each part's type
    // general rather than refuse the layer.
    add("-nlt");
    add("PROMOTE_TO_MULTI");
    add("-skipfailures");
    GDALVectorTranslateOptions* options = GDALVectorTranslateOptionsNew(arguments.List(), nullptr);
    std::error_code ignored;
    std::filesystem::create_directories(output.parent_path(), ignored);
    // Written under a name of its own and renamed into place, as the cache's
    // other files are (online_fetch.cpp, writeAtomically): the output's name
    // is a hash of the file and the area, so two imports of the same area -
    // two windows, or two tests run side by side - name the same file, and on
    // Windows the second writer found it held open by the first and failed.
    const std::filesystem::path partial =
        output.string() + ".part-" + std::to_string(std::random_device{}()) + ".gpkg";
    GDALDatasetH sources[] = {source.get()};
    int usageError = FALSE;
    CPLErrorReset();
    CPLPushErrorHandler(CPLQuietErrorHandler);
    DatasetHandle clipped(GDALVectorTranslate(partial.string().c_str(), nullptr, 1, sources,
                                              options, &usageError));
    CPLPopErrorHandler();
    GDALVectorTranslateOptionsFree(options);
    if (!clipped) {
        std::filesystem::remove(partial, ignored);
        return makeError(ErrorCode::FileImportFailure, "GDAL could not clip the downloaded file",
                         std::string(CPLGetLastErrorMsg()));
    }
    clipped.reset(); // closed, so the rename can move it
    std::error_code renamed;
    std::filesystem::rename(partial, output, renamed);
    if (renamed) {
        std::filesystem::remove(partial, ignored);
        // Another import of the same area got there first: its file is the
        // same clip, and it is the one to read.
        if (!std::filesystem::is_regular_file(output, ignored)) {
            return makeError(ErrorCode::FileImportFailure, "could not keep the clipped file",
                             renamed.message());
        }
    }
    return {};
}

Status buildTrueColourVrt(const std::vector<std::string>& bandPaths, double low, double high,
                          const std::filesystem::path& output, const std::string& userAgent,
                          int timeoutSeconds)
{
    detail::ensureGdalRegistered();
    // The bands are opened over HTTP here, so the warp's limits apply: a
    // stalled connection must end, and each open is one request, not a
    // directory listing first.
    ThreadConfig config;
    if (!userAgent.empty()) {
        config.set("GDAL_HTTP_USERAGENT", userAgent);
    }
    config.set("GDAL_HTTP_TIMEOUT", std::to_string(std::max(1, timeoutSeconds)));
    config.set("GDAL_HTTP_CONNECTTIMEOUT", "20");
    config.set("GDAL_DISABLE_READDIR_ON_OPEN", "EMPTY_DIR");
    if (bandPaths.size() != 3) {
        return makeError(ErrorCode::InvalidArgument, "a true-colour composite needs three bands",
                         std::to_string(bandPaths.size()));
    }
    if (!(high > low)) {
        return makeError(ErrorCode::InvalidArgument, "the stretch's high must exceed its low");
    }
    std::vector<const char*> names;
    for (const std::string& path : bandPaths) {
        names.push_back(path.c_str());
    }
    CPLStringList stackArguments;
    stackArguments.AddString("-separate");
    GDALBuildVRTOptions* stackOptions = GDALBuildVRTOptionsNew(stackArguments.List(), nullptr);
    CPLPushErrorHandler(CPLQuietErrorHandler);
    DatasetHandle stacked(GDALBuildVRT("", static_cast<int>(names.size()), nullptr, names.data(),
                                       stackOptions, nullptr));
    CPLPopErrorHandler();
    GDALBuildVRTOptionsFree(stackOptions);
    if (!stacked) {
        return makeError(ErrorCode::FileImportFailure, "the three bands could not be stacked",
                         std::string(CPLGetLastErrorMsg()));
    }
    CPLStringList arguments;
    const auto add = [&](const std::string& value) { arguments.AddString(value.c_str()); };
    add("-of");
    add("VRT");
    add("-ot");
    add("Byte");
    add("-scale");
    add(number(low));
    add(number(high));
    add("1");
    add("255");
    // With an exponent (of 1: still linear) gdal_translate CLAMPS a value
    // outside low..high to the ends of the output range. Without it a value
    // below `low` - dark water - came out 0, the no-data value, and the
    // harbour was transparent.
    add("-exponent");
    add("1");
    add("-a_nodata");
    add("0");
    add("-colorinterp");
    add("red,green,blue");
    GDALTranslateOptions* options = GDALTranslateOptionsNew(arguments.List(), nullptr);
    std::error_code ignored;
    std::filesystem::create_directories(output.parent_path(), ignored);
    CPLPushErrorHandler(CPLQuietErrorHandler);
    DatasetHandle composite(
        GDALTranslate(output.string().c_str(), stacked.get(), options, nullptr));
    CPLPopErrorHandler();
    GDALTranslateOptionsFree(options);
    if (!composite) {
        return makeError(ErrorCode::FileImportFailure, "the composite could not be written",
                         std::string(CPLGetLastErrorMsg()));
    }
    return {};
}

} // namespace katana::gis
