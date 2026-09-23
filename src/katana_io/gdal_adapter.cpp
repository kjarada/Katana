#include "katana/gis/gdal_adapter.hpp"

#include <algorithm>
#include <cmath>
#include <cstring>
#include <cwctype>
#include <filesystem>
#include <mutex>
#include <system_error>
#include <vector>

#if defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
// PSAPI_VERSION 2 maps EnumProcessModules onto the copy in kernel32, so no
// extra library is linked for it.
#define PSAPI_VERSION 2
#include <psapi.h>
#endif

#include <cpl_conv.h>
#include <cpl_error.h>
#include <cpl_string.h>
#include <gdal_priv.h>
#include <ogrsf_frmts.h>

namespace katana::gis {
namespace {

using katana::core::ErrorCode;
using katana::core::Error;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

// GDAL's driver manager is process-global. Registering it once, lazily, under a
// call_once is the whole of the lifetime management: it is deliberately NEVER
// destroyed. GDALDestroyDriverManager() tears down state shared by every open
// dataset in the process, so calling it when one dataset closes - as this file
// used to - invalidates every other dataset and every later open. The driver
// manager is freed by process exit, which is correct and costs nothing.
// Tells GDAL where its support files are, when nothing else has.
//
// GDAL reads data files at run time - header.dxf and trailer.dxf, the
// templates every DXF it writes is built from, among others - and finds them
// through GDAL_DATA or a directory compiled in when GDAL was built. On MSYS2
// that compiled-in path is useless (the package is relocatable), and only an
// MSYS2 LOGIN SHELL sets GDAL_DATA. Started any other way - from Git Bash, an
// IDE, Explorer, a test runner, or a bundle on someone else's machine - GDAL
// found nothing, and DXF export failed with "failed to find template header
// file header.dxf". It had never once worked outside an MSYS2 shell.
//
// The fix is the one PROJ uses for proj.db: look RELATIVE TO THE LIBRARY.
// The GDAL DLL sits in <prefix>/bin and its data in <prefix>/share/gdal, and
// that holds both for the toolchain (C:/msys64/ucrt64) and for a bundle made
// by `cmake --install`, which is laid out bin/ beside share/ for this reason.
// One mechanism for both, instead of asking each program to know where it was
// installed. A GDAL_DATA the user has set is respected: an explicit choice
// outranks a default.
void locateGdalData()
{
    if (CPLGetConfigOption("GDAL_DATA", nullptr) != nullptr) {
        return;
    }
#if defined(_WIN32)
    // Found by NAME among the loaded modules. The obvious route - ask Windows
    // which module holds the address of GDALAllRegister - gives the wrong
    // answer under MinGW: in an importing program that address is the import
    // THUNK inside our own executable, so Windows truthfully names us, the
    // data directory is looked for beside our own program, and nothing is
    // found. That was tried first, and failed exactly so.
    std::vector<HMODULE> modules(1024);
    DWORD bytes = 0;
    if (EnumProcessModules(GetCurrentProcess(), modules.data(),
                           static_cast<DWORD>(modules.size() * sizeof(HMODULE)), &bytes) == 0) {
        return;
    }
    modules.resize(std::min<std::size_t>(modules.size(), bytes / sizeof(HMODULE)));
    std::wstring buffer;
    for (const HMODULE module : modules) {
        std::wstring candidate(512, L' ');
        for (;;) {
            const DWORD length = GetModuleFileNameW(module, candidate.data(),
                                                    static_cast<DWORD>(candidate.size()));
            if (length == 0) {
                candidate.clear();
                break;
            }
            if (length < candidate.size()) {
                candidate.resize(length);
                break;
            }
            candidate.resize(candidate.size() * 2); // truncated: longer than the buffer
        }
        std::wstring name = std::filesystem::path(candidate).filename().wstring();
        std::transform(name.begin(), name.end(), name.begin(),
                       [](wchar_t c) { return static_cast<wchar_t>(std::towlower(c)); });
        // libgdal-38.dll from MinGW, gdal.dll or gdal308.dll from MSVC.
        const bool isGdal = name.ends_with(L".dll") &&
                            (name.starts_with(L"libgdal") || name.starts_with(L"gdal"));
        if (isGdal) {
            buffer = candidate;
            break;
        }
    }
    if (buffer.empty()) {
        return;
    }
    std::error_code ignored;
    const std::filesystem::path data =
        std::filesystem::path(buffer).parent_path().parent_path() / "share" / "gdal";
    if (std::filesystem::is_directory(data, ignored)) {
        // GDAL takes UTF-8 (GDAL_FILENAME_IS_UTF8 defaults to YES), and
        // forward slashes on every platform.
        const std::u8string utf8 = data.generic_u8string();
        CPLSetConfigOption("GDAL_DATA", std::string(utf8.begin(), utf8.end()).c_str());
    }
#endif
}

void ensureRegistered()
{
    static std::once_flag once;
    std::call_once(once, [] {
        locateGdalData();
        GDALAllRegister();
        // Keep GDAL's chatter off stderr; failures are reported through Result
        // with CPLGetLastErrorMsg() as context instead.
        CPLSetErrorHandler(CPLQuietErrorHandler);
    });
}

// GDAL's last error, for the context field of an Error. Empty when GDAL did not
// set one, in which case the caller's own message has to carry the meaning.
std::string lastGdalError()
{
    const char* message = CPLGetLastErrorMsg();
    if (message == nullptr || *message == '\0') {
        return {};
    }
    return message;
}

GDALDataset* asDataset(void* handle)
{
    return static_cast<GDALDataset*>(handle);
}

std::string lowerExtension(const std::filesystem::path& path)
{
    std::string extension = path.extension().string();
    if (!extension.empty() && extension.front() == '.') {
        extension.erase(extension.begin());
    }
    std::transform(extension.begin(), extension.end(), extension.begin(),
                   [](unsigned char ch) { return static_cast<char>(std::tolower(ch)); });
    return extension;
}

struct DriverForExtension {
    const char* extension;
    const char* driver;
};

// Only formats that are actually writable and that a survey or CAD user would
// reasonably ask for. An extension absent from this table is rejected by name
// rather than guessed at.
constexpr DriverForExtension kVectorDrivers[] = {
    {"shp", "ESRI Shapefile"}, {"geojson", "GeoJSON"}, {"json", "GeoJSON"},
    {"gpkg", "GPKG"},          {"kml", "KML"},         {"gml", "GML"},
    {"dxf", "DXF"},            {"csv", "CSV"},         {"sqlite", "SQLite"},
    {"tab", "MapInfo File"},
};

// ---- OGR geometry -> VectorGeometry ---------------------------------------

std::vector<GeoPoint> pointsOf(const OGRLineString& line)
{
    std::vector<GeoPoint> points;
    const int count = line.getNumPoints();
    points.reserve(static_cast<std::size_t>(std::max(0, count)));
    const bool has3d = line.Is3D() != 0;
    for (int i = 0; i < count; ++i) {
        points.push_back(GeoPoint{line.getX(i), line.getY(i), has3d ? line.getZ(i) : 0.0});
    }
    return points;
}

// Appends one feature per SIMPLE geometry. A multi-geometry or collection is
// recursed into so that every emitted VectorGeometry has exactly one kind,
// which is what keeps the consumer free of nested-variant handling.
void flatten(const OGRGeometry* geometry, const std::map<std::string, std::string>& attributes,
             std::vector<VectorFeature>& out, std::vector<std::string>& warnings,
             int depth = 0)
{
    if (geometry == nullptr || geometry->IsEmpty()) {
        return;
    }
    // Defensive: OGR geometries are trees and a malformed file could in
    // principle nest collections deeply. 32 is far beyond anything meaningful.
    if (depth > 32) {
        warnings.push_back("geometry nested more than 32 levels deep was skipped");
        return;
    }

    switch (wkbFlatten(geometry->getGeometryType())) {
    case wkbPoint: {
        const auto* point = geometry->toPoint();
        VectorFeature feature;
        feature.geometry.kind = GeometryKind::Point;
        feature.geometry.parts.push_back({GeoPoint{point->getX(), point->getY(),
                                                   point->Is3D() != 0 ? point->getZ() : 0.0}});
        feature.geometry.hasZ = point->Is3D() != 0;
        feature.attributes = attributes;
        out.push_back(std::move(feature));
        return;
    }
    case wkbLineString: {
        VectorFeature feature;
        feature.geometry.kind = GeometryKind::LineString;
        feature.geometry.parts.push_back(pointsOf(*geometry->toLineString()));
        feature.geometry.hasZ = geometry->Is3D() != 0;
        feature.attributes = attributes;
        out.push_back(std::move(feature));
        return;
    }
    case wkbPolygon: {
        const auto* polygon = geometry->toPolygon();
        VectorFeature feature;
        feature.geometry.kind = GeometryKind::Polygon;
        if (const auto* exterior = polygon->getExteriorRing()) {
            feature.geometry.parts.push_back(pointsOf(*exterior));
        }
        for (int i = 0; i < polygon->getNumInteriorRings(); ++i) {
            if (const auto* hole = polygon->getInteriorRing(i)) {
                feature.geometry.parts.push_back(pointsOf(*hole));
            }
        }
        if (feature.geometry.parts.empty()) {
            return;
        }
        feature.geometry.hasZ = polygon->Is3D() != 0;
        feature.attributes = attributes;
        out.push_back(std::move(feature));
        return;
    }
    case wkbMultiPoint:
    case wkbMultiLineString:
    case wkbMultiPolygon:
    case wkbGeometryCollection: {
        const auto* collection = geometry->toGeometryCollection();
        for (int i = 0; i < collection->getNumGeometries(); ++i) {
            flatten(collection->getGeometryRef(i), attributes, out, warnings, depth + 1);
        }
        return;
    }
    default:
        warnings.push_back(std::string("unsupported geometry type '") +
                           OGRGeometryTypeToName(geometry->getGeometryType()) + "' was skipped");
        return;
    }
}

// ---- VectorGeometry -> OGR geometry ---------------------------------------

// Adds a vertex in the geometry's own dimension: OGR makes a geometry 3D the
// moment one vertex is given a z, so a 2D one must never be handed a zero.
template <typename Line>
void addVertex(Line& line, const GeoPoint& point, bool hasZ)
{
    if (hasZ) {
        line.addPoint(point.x, point.y, point.z);
    } else {
        line.addPoint(point.x, point.y);
    }
}

OGRLinearRing* makeRing(const std::vector<GeoPoint>& points, bool hasZ)
{
    auto* ring = new OGRLinearRing();
    for (const GeoPoint& point : points) {
        addVertex(*ring, point, hasZ);
    }
    // OGR requires a closed ring; close it if the caller's data does not.
    if (!points.empty()) {
        const GeoPoint& first = points.front();
        const GeoPoint& last = points.back();
        if (first.x != last.x || first.y != last.y) {
            addVertex(*ring, first, hasZ);
        }
    }
    return ring;
}

OGRGeometry* makeGeometry(const VectorGeometry& geometry)
{
    switch (geometry.kind) {
    case GeometryKind::Point: {
        if (geometry.parts.empty() || geometry.parts.front().empty()) {
            return nullptr;
        }
        const GeoPoint& p = geometry.parts.front().front();
        return geometry.hasZ ? new OGRPoint(p.x, p.y, p.z) : new OGRPoint(p.x, p.y);
    }
    case GeometryKind::LineString: {
        if (geometry.parts.empty() || geometry.parts.front().size() < 2) {
            return nullptr;
        }
        auto* line = new OGRLineString();
        for (const GeoPoint& p : geometry.parts.front()) {
            addVertex(*line, p, geometry.hasZ);
        }
        return line;
    }
    case GeometryKind::Polygon: {
        if (geometry.parts.empty() || geometry.parts.front().size() < 3) {
            return nullptr;
        }
        auto* polygon = new OGRPolygon();
        for (const auto& part : geometry.parts) {
            if (part.size() >= 3) {
                polygon->addRingDirectly(makeRing(part, geometry.hasZ));
            }
        }
        return polygon;
    }
    case GeometryKind::Unknown:
        break;
    }
    return nullptr;
}

OGRwkbGeometryType ogrTypeFor(GeometryKind kind)
{
    switch (kind) {
    case GeometryKind::Point:
        return wkbPoint;
    case GeometryKind::LineString:
        return wkbLineString;
    case GeometryKind::Polygon:
        return wkbPolygon;
    case GeometryKind::Unknown:
        break;
    }
    return wkbUnknown;
}

// ---- raster image helpers -------------------------------------------------

struct BandRoles {
    GDALRasterBand* red = nullptr;
    GDALRasterBand* green = nullptr;
    GDALRasterBand* blue = nullptr;
    GDALRasterBand* alpha = nullptr;
    GDALRasterBand* palette = nullptr;
    GDALRasterBand* grey = nullptr;
};

BandRoles classifyBands(GDALDataset& dataset)
{
    BandRoles roles;
    const int count = dataset.GetRasterCount();
    for (int i = 1; i <= count; ++i) {
        GDALRasterBand* band = dataset.GetRasterBand(i);
        switch (band->GetColorInterpretation()) {
        case GCI_RedBand:
            roles.red = band;
            break;
        case GCI_GreenBand:
            roles.green = band;
            break;
        case GCI_BlueBand:
            roles.blue = band;
            break;
        case GCI_AlphaBand:
            roles.alpha = band;
            break;
        case GCI_PaletteIndex:
            roles.palette = band;
            break;
        case GCI_GrayIndex:
            roles.grey = band;
            break;
        default:
            break;
        }
    }
    // Plenty of GeoTIFFs carry three or four undesignated bands; treating the
    // first three as RGB is what every other GIS does and is far better than
    // rendering an aerial photo as a grey ramp of its red channel.
    if (roles.red == nullptr && roles.palette == nullptr && count >= 3) {
        roles.red = dataset.GetRasterBand(1);
        roles.green = dataset.GetRasterBand(2);
        roles.blue = dataset.GetRasterBand(3);
    }
    if (roles.red == nullptr && roles.palette == nullptr && roles.grey == nullptr && count >= 1) {
        roles.grey = dataset.GetRasterBand(1);
    }
    return roles;
}

} // namespace

// ---- lifetime -------------------------------------------------------------

Result<std::unique_ptr<GdalDataset>> GdalDataset::open(const std::filesystem::path& path)
{
    ensureRegistered();

    std::error_code existsError;
    if (!std::filesystem::exists(path, existsError)) {
        return makeError(ErrorCode::NotFound, "file does not exist", path.string());
    }

    void* handle = GDALOpenEx(path.string().c_str(),
                              GDAL_OF_READONLY | GDAL_OF_RASTER | GDAL_OF_VECTOR, nullptr,
                              nullptr, nullptr);
    if (handle == nullptr) {
        return makeError(ErrorCode::FileImportFailure,
                         "GDAL could not open '" + path.string() + "'", lastGdalError());
    }
    // Private constructor, so make_unique is not available here.
    std::unique_ptr<GdalDataset> dataset(new GdalDataset());
    dataset->dataset_ = handle;
    return dataset;
}

GdalDataset::~GdalDataset()
{
    if (dataset_ != nullptr) {
        GDALClose(asDataset(dataset_));
        dataset_ = nullptr;
    }
    // Deliberately no GDALDestroyDriverManager() here; see ensureRegistered().
}

bool GdalDataset::hasRaster() const
{
    return asDataset(dataset_)->GetRasterCount() > 0;
}

bool GdalDataset::hasVector() const
{
    return asDataset(dataset_)->GetLayerCount() > 0;
}

// ---- raster ---------------------------------------------------------------

Result<RasterInfo> GdalDataset::rasterInfo() const
{
    GDALDataset* dataset = asDataset(dataset_);
    if (dataset->GetRasterCount() == 0) {
        return makeError(ErrorCode::InvalidState, "dataset holds no raster bands");
    }

    RasterInfo info;
    info.width = dataset->GetRasterXSize();
    info.height = dataset->GetRasterYSize();
    info.bandCount = dataset->GetRasterCount();
    if (const char* projection = dataset->GetProjectionRef()) {
        info.projectionWkt = projection;
    }
    std::array<double, 6> geotransform{};
    if (dataset->GetGeoTransform(geotransform.data()) == CE_None) {
        info.geotransform = geotransform;
        info.hasGeotransform = true;
    }
    return info;
}

Result<std::vector<double>> GdalDataset::readBand(int bandIndex) const
{
    GDALDataset* dataset = asDataset(dataset_);
    if (bandIndex < 1 || bandIndex > dataset->GetRasterCount()) {
        return makeError(ErrorCode::InvalidArgument, "raster band index is out of range",
                         "requested " + std::to_string(bandIndex) + " of " +
                             std::to_string(dataset->GetRasterCount()));
    }

    const int width = dataset->GetRasterXSize();
    const int height = dataset->GetRasterYSize();
    std::vector<double> values(static_cast<std::size_t>(width) * static_cast<std::size_t>(height));
    GDALRasterBand* band = dataset->GetRasterBand(bandIndex);
    if (band->RasterIO(GF_Read, 0, 0, width, height, values.data(), width, height, GDT_Float64, 0,
                       0, nullptr) != CE_None) {
        return makeError(ErrorCode::FileImportFailure, "GDAL failed to read raster band",
                         lastGdalError());
    }
    return values;
}

Result<RasterImage> GdalDataset::readImage(int maxPixels) const
{
    if (maxPixels < 1) {
        return makeError(ErrorCode::InvalidArgument, "maxPixels must be at least 1");
    }

    auto info = rasterInfo();
    if (!info.ok()) {
        return info.error();
    }
    GDALDataset* dataset = asDataset(dataset_);

    const int width = info->width;
    const int height = info->height;
    if (width <= 0 || height <= 0) {
        return makeError(ErrorCode::InvalidState, "raster has no extent");
    }

    // Decimate on READ: GDAL resamples into the smaller buffer, so a 2 GB
    // GeoTIFF never becomes resident.
    const int longest = std::max(width, height);
    const int step = std::max(1, (longest + maxPixels - 1) / maxPixels);
    const int outWidth = std::max(1, (width + step - 1) / step);
    const int outHeight = std::max(1, (height + step - 1) / step);
    const std::size_t pixels = static_cast<std::size_t>(outWidth) *
                               static_cast<std::size_t>(outHeight);

    RasterImage image;
    image.width = outWidth;
    image.height = outHeight;
    image.rgba.assign(pixels * 4, 255);
    image.projectionWkt = info->projectionWkt;
    image.hasGeotransform = info->hasGeotransform;
    // The affine must be rescaled for the decimated grid: a step of n means one
    // output pixel now spans n input pixels, so the pixel-size and rotation
    // terms scale by the ACTUAL ratio (width/outWidth), not by `step` - those
    // differ whenever the size is not an exact multiple of the step.
    image.geotransform = info->geotransform;
    const double scaleX = static_cast<double>(width) / static_cast<double>(outWidth);
    const double scaleY = static_cast<double>(height) / static_cast<double>(outHeight);
    image.geotransform[1] *= scaleX;
    image.geotransform[2] *= scaleY;
    image.geotransform[4] *= scaleX;
    image.geotransform[5] *= scaleY;

    const BandRoles roles = classifyBands(*dataset);

    auto readByteBand = [&](GDALRasterBand* band, std::vector<std::uint8_t>& out) -> bool {
        out.assign(pixels, 0);
        return band->RasterIO(GF_Read, 0, 0, width, height, out.data(), outWidth, outHeight,
                              GDT_Byte, 0, 0, nullptr) == CE_None;
    };

    if (roles.red != nullptr && roles.green != nullptr && roles.blue != nullptr) {
        std::vector<std::uint8_t> r;
        std::vector<std::uint8_t> g;
        std::vector<std::uint8_t> b;
        if (!readByteBand(roles.red, r) || !readByteBand(roles.green, g) ||
            !readByteBand(roles.blue, b)) {
            return makeError(ErrorCode::FileImportFailure, "GDAL failed to read RGB bands",
                             lastGdalError());
        }
        std::vector<std::uint8_t> a;
        const bool hasAlpha = roles.alpha != nullptr && readByteBand(roles.alpha, a);
        for (std::size_t i = 0; i < pixels; ++i) {
            image.rgba[i * 4 + 0] = r[i];
            image.rgba[i * 4 + 1] = g[i];
            image.rgba[i * 4 + 2] = b[i];
            image.rgba[i * 4 + 3] = hasAlpha ? a[i] : 255;
        }
        return image;
    }

    if (roles.palette != nullptr) {
        std::vector<std::uint8_t> indices;
        if (!readByteBand(roles.palette, indices)) {
            return makeError(ErrorCode::FileImportFailure, "GDAL failed to read palette band",
                             lastGdalError());
        }
        const GDALColorTable* table = roles.palette->GetColorTable();
        const int entries = table != nullptr ? table->GetColorEntryCount() : 0;
        for (std::size_t i = 0; i < pixels; ++i) {
            const int index = indices[i];
            if (table != nullptr && index < entries) {
                const GDALColorEntry* entry = table->GetColorEntry(index);
                image.rgba[i * 4 + 0] = static_cast<std::uint8_t>(entry->c1);
                image.rgba[i * 4 + 1] = static_cast<std::uint8_t>(entry->c2);
                image.rgba[i * 4 + 2] = static_cast<std::uint8_t>(entry->c3);
                image.rgba[i * 4 + 3] = static_cast<std::uint8_t>(entry->c4);
            } else {
                image.rgba[i * 4 + 0] = image.rgba[i * 4 + 1] = image.rgba[i * 4 + 2] =
                    static_cast<std::uint8_t>(index);
                image.rgba[i * 4 + 3] = 255;
            }
        }
        return image;
    }

    if (roles.grey == nullptr) {
        return makeError(ErrorCode::Unsupported, "raster has no band this reader can display");
    }

    // Single band of arbitrary type - elevation, for instance. Read as double
    // and stretch over the band's real range: casting a Float32 DEM straight to
    // a byte would clamp every elevation above 255 to white.
    std::vector<double> values(pixels);
    if (roles.grey->RasterIO(GF_Read, 0, 0, width, height, values.data(), outWidth, outHeight,
                             GDT_Float64, 0, 0, nullptr) != CE_None) {
        return makeError(ErrorCode::FileImportFailure, "GDAL failed to read raster band",
                         lastGdalError());
    }

    int hasNoData = 0;
    const double noData = roles.grey->GetNoDataValue(&hasNoData);

    double minimum = 0.0;
    double maximum = 0.0;
    double mean = 0.0;
    double deviation = 0.0;
    bool haveRange = roles.grey->GetStatistics(TRUE, TRUE, &minimum, &maximum, &mean,
                                               &deviation) == CE_None &&
                     maximum > minimum;
    if (!haveRange) {
        // Statistics can be unavailable (no overviews, approximate scan
        // refused). Derive the range from what was actually read.
        minimum = std::numeric_limits<double>::max();
        maximum = std::numeric_limits<double>::lowest();
        for (const double value : values) {
            if (!std::isfinite(value) || (hasNoData != 0 && value == noData)) {
                continue;
            }
            minimum = std::min(minimum, value);
            maximum = std::max(maximum, value);
        }
        haveRange = maximum > minimum;
    }

    const double span = haveRange ? (maximum - minimum) : 1.0;
    for (std::size_t i = 0; i < pixels; ++i) {
        const double value = values[i];
        if (!std::isfinite(value) || (hasNoData != 0 && value == noData)) {
            image.rgba[i * 4 + 3] = 0; // transparent, so no-data shows the drawing beneath
            continue;
        }
        const double normalised = haveRange ? std::clamp((value - minimum) / span, 0.0, 1.0) : 0.5;
        const auto grey = static_cast<std::uint8_t>(std::lround(normalised * 255.0));
        image.rgba[i * 4 + 0] = grey;
        image.rgba[i * 4 + 1] = grey;
        image.rgba[i * 4 + 2] = grey;
        image.rgba[i * 4 + 3] = 255;
    }
    return image;
}

// ---- vector ---------------------------------------------------------------

Result<std::vector<VectorLayerInfo>> GdalDataset::vectorLayers() const
{
    GDALDataset* dataset = asDataset(dataset_);
    std::vector<VectorLayerInfo> result;
    for (int index = 0; index < dataset->GetLayerCount(); ++index) {
        OGRLayer* layer = dataset->GetLayer(index);
        if (layer == nullptr) {
            continue;
        }
        VectorLayerInfo info;
        info.name = layer->GetName();
        // Force the count: a lazily-counted layer returns -1 otherwise, which
        // would become a huge number in the unsigned field.
        const GIntBig count = layer->GetFeatureCount(TRUE);
        info.featureCount = count > 0 ? static_cast<std::uint64_t>(count) : 0;
        info.geometryType = OGRGeometryTypeToName(layer->GetGeomType());
        if (const OGRSpatialReference* reference = layer->GetSpatialRef()) {
            char* wkt = nullptr;
            if (reference->exportToWkt(&wkt) == OGRERR_NONE && wkt != nullptr) {
                info.projectionWkt = wkt;
            }
            CPLFree(wkt);
        }
        result.push_back(std::move(info));
    }
    return result;
}

Result<std::vector<VectorFeature>> GdalDataset::readFeatures(int layerIndex,
                                                             std::uint64_t maxFeatures) const
{
    GDALDataset* dataset = asDataset(dataset_);
    if (layerIndex < 0 || layerIndex >= dataset->GetLayerCount()) {
        return makeError(ErrorCode::InvalidArgument, "vector layer index is out of range",
                         "requested " + std::to_string(layerIndex) + " of " +
                             std::to_string(dataset->GetLayerCount()));
    }
    OGRLayer* layer = dataset->GetLayer(layerIndex);
    if (layer == nullptr) {
        return makeError(ErrorCode::Internal, "GDAL returned a null layer");
    }

    std::vector<VectorFeature> features;
    std::vector<std::string> warnings; // collected but reported by the caller's importer
    const OGRFeatureDefn* definition = layer->GetLayerDefn();
    const int fieldCount = definition != nullptr ? definition->GetFieldCount() : 0;

    layer->ResetReading();
    while (OGRFeature* feature = layer->GetNextFeature()) {
        std::map<std::string, std::string> attributes;
        for (int i = 0; i < fieldCount; ++i) {
            if (!feature->IsFieldSetAndNotNull(i)) {
                continue;
            }
            const OGRFieldDefn* field = feature->GetFieldDefnRef(i);
            if (field == nullptr) {
                continue;
            }
            const char* value = feature->GetFieldAsString(i);
            attributes.emplace(field->GetNameRef(), value != nullptr ? value : "");
        }
        flatten(feature->GetGeometryRef(), attributes, features, warnings);
        OGRFeature::DestroyFeature(feature);

        if (maxFeatures != 0 && features.size() >= maxFeatures) {
            break;
        }
    }
    return features;
}

// ---- writing --------------------------------------------------------------

Result<std::string> GdalDataset::vectorDriverForPath(const std::filesystem::path& path)
{
    ensureRegistered();
    const std::string extension = lowerExtension(path);
    if (extension.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "cannot choose a vector driver: the path has no extension",
                         path.string());
    }
    for (const DriverForExtension& entry : kVectorDrivers) {
        if (extension == entry.extension) {
            if (GetGDALDriverManager()->GetDriverByName(entry.driver) == nullptr) {
                return makeError(ErrorCode::Unsupported,
                                 std::string("this GDAL build has no '") + entry.driver +
                                     "' driver",
                                 path.string());
            }
            return std::string(entry.driver);
        }
    }
    return makeError(ErrorCode::Unsupported, "no vector driver is registered for '." + extension +
                                                 "'",
                     path.string());
}

Status GdalDataset::writeRaster(const std::filesystem::path& path,
                                const RasterExportOptions& options,
                                const std::vector<double>& values)
{
    ensureRegistered();

    if (options.width <= 0 || options.height <= 0) {
        return makeError(ErrorCode::InvalidArgument, "raster export dimensions must be positive");
    }
    const auto expected = static_cast<std::size_t>(options.width) *
                          static_cast<std::size_t>(options.height);
    if (values.size() != expected) {
        return makeError(ErrorCode::InvalidArgument,
                         "raster export data does not match the stated dimensions",
                         std::to_string(values.size()) + " values for " +
                             std::to_string(options.width) + "x" +
                             std::to_string(options.height));
    }

    GDALDriver* driver = GetGDALDriverManager()->GetDriverByName(options.driver.c_str());
    if (driver == nullptr) {
        return makeError(ErrorCode::Unsupported, "raster driver is unavailable", options.driver);
    }

    GDALDataset* dataset =
        driver->Create(path.string().c_str(), options.width, options.height, 1, GDT_Float64,
                       nullptr);
    if (dataset == nullptr) {
        return makeError(ErrorCode::FileExportFailure,
                         "GDAL could not create '" + path.string() + "'", lastGdalError());
    }

    std::array<double, 6> geotransform = options.geotransform;
    dataset->SetGeoTransform(geotransform.data());
    if (!options.projectionWkt.empty()) {
        dataset->SetProjection(options.projectionWkt.c_str());
    }
    GDALRasterBand* band = dataset->GetRasterBand(1);
    if (options.noDataValue.has_value()) {
        band->SetNoDataValue(*options.noDataValue);
    }
    const CPLErr status =
        band->RasterIO(GF_Write, 0, 0, options.width, options.height,
                       const_cast<double*>(values.data()), options.width, options.height,
                       GDT_Float64, 0, 0, nullptr);
    const std::string message = status != CE_None ? lastGdalError() : std::string();
    GDALClose(dataset);
    if (status != CE_None) {
        return makeError(ErrorCode::FileExportFailure, "GDAL failed to write raster", message);
    }
    return {};
}

Status GdalDataset::writeVector(const std::filesystem::path& path,
                                const std::vector<VectorFeature>& features,
                                const VectorExportOptions& options)
{
    ensureRegistered();

    std::string driverName = options.driver;
    if (driverName.empty()) {
        auto inferred = vectorDriverForPath(path);
        if (!inferred.ok()) {
            return inferred.error();
        }
        driverName = *inferred;
    }
    GDALDriver* driver = GetGDALDriverManager()->GetDriverByName(driverName.c_str());
    if (driver == nullptr) {
        return makeError(ErrorCode::Unsupported, "vector driver is unavailable", driverName);
    }

    // Most drivers refuse to overwrite. Remove an existing file first so that
    // re-exporting to the same name behaves the way a user expects a Save As to.
    std::error_code removeError;
    std::filesystem::remove(path, removeError);

    GDALDataset* dataset = driver->Create(path.string().c_str(), 0, 0, 0, GDT_Unknown, nullptr);
    if (dataset == nullptr) {
        return makeError(ErrorCode::FileExportFailure,
                         "GDAL could not create '" + path.string() + "'", lastGdalError());
    }

    // A half-written export is worse than none: a Shapefile is three or more
    // files, and leaving a truncated set behind invites someone to open it and
    // believe it. Every failure below unwinds through this, which closes the
    // dataset and asks the DRIVER to delete it - the driver knows about the
    // sidecar files, std::filesystem::remove would only take the .shp.
    const auto abandon = [&driver, &path](GDALDataset* handle, Error error) {
        GDALClose(handle);
        CPLPushErrorHandler(CPLQuietErrorHandler);
        driver->Delete(path.string().c_str());
        CPLPopErrorHandler();
        return error;
    };

    OGRSpatialReference reference;
    OGRSpatialReference* referencePtr = nullptr;
    if (!options.projectionWkt.empty() &&
        reference.importFromWkt(options.projectionWkt.c_str()) == OGRERR_NONE) {
        referencePtr = &reference;
    }

    // A shapefile holds exactly one geometry type per layer, so pick the single
    // kind when the data has one and fall back to wkbUnknown otherwise (which
    // the more capable formats accept).
    OGRwkbGeometryType layerType = wkbUnknown;
    if (!features.empty()) {
        const GeometryKind first = features.front().geometry.kind;
        const bool uniform = std::all_of(features.begin(), features.end(),
                                         [first](const VectorFeature& feature) {
                                             return feature.geometry.kind == first;
                                         });
        if (uniform) {
            layerType = ogrTypeFor(first);
        }
        // A layer that is going to hold heights is declared 3D, or a driver
        // with a fixed layer type (a shapefile) drops them. The caller decides
        // what a format that cannot mix 2D and 3D gets (export.cpp).
        const bool anyZ = std::any_of(features.begin(), features.end(),
                                      [](const VectorFeature& feature) {
                                          return feature.geometry.hasZ;
                                      });
        if (anyZ && layerType != wkbUnknown) {
            layerType = wkbSetZ(layerType);
        }
    }

    OGRLayer* layer =
        dataset->CreateLayer(options.layerName.c_str(), referencePtr, layerType, nullptr);
    if (layer == nullptr) {
        return abandon(dataset, makeError(ErrorCode::FileExportFailure,
                                          "GDAL could not create the vector layer",
                                          lastGdalError()));
    }

    // Union of attribute names, in the ordered maps' order, so the field layout
    // is deterministic across runs (Rule 7).
    std::vector<std::string> fieldNames;
    for (const VectorFeature& feature : features) {
        for (const auto& [name, value] : feature.attributes) {
            if (std::find(fieldNames.begin(), fieldNames.end(), name) == fieldNames.end()) {
                fieldNames.push_back(name);
            }
        }
    }
    // Some formats have a FIXED set of fields and refuse any other: a DXF
    // layer has Layer, Linetype, Text and a few more, and CreateField fails
    // on everything else. That used to abort the whole export - so DXF, the
    // one format a CAD program cannot do without, could not be written at
    // all. Such a layer is written with the fields it has; an attribute is
    // kept when the format already has a field of that name (OGR matches
    // names case-insensitively, so ours `layer` lands in DXF's `Layer`) and
    // dropped otherwise, which driverHasFixedFields lets the caller report.
    const bool canCreateFields = layer->TestCapability(OLCCreateField) != 0;
    for (const std::string& name : fieldNames) {
        if (!canCreateFields) {
            break;
        }
        OGRFieldDefn field(name.c_str(), OFTString);
        if (layer->CreateField(&field) != OGRERR_NONE) {
            return abandon(dataset,
                           makeError(ErrorCode::FileExportFailure,
                                     "GDAL could not create field '" + name + "'",
                                     lastGdalError()));
        }
    }

    for (const VectorFeature& source : features) {
        OGRGeometry* geometry = makeGeometry(source.geometry);
        if (geometry == nullptr) {
            continue; // counted by the caller, which knows what it handed over
        }
        OGRFeature* feature = OGRFeature::CreateFeature(layer->GetLayerDefn());
        feature->SetGeometryDirectly(geometry);
        for (const auto& [name, value] : source.attributes) {
            // By index, and only when the field exists: SetField by a name the
            // layer does not have is an error GDAL logs once per feature.
            const int index = feature->GetFieldIndex(name.c_str());
            if (index >= 0) {
                feature->SetField(index, value.c_str());
            }
        }
        const OGRErr status = layer->CreateFeature(feature);
        OGRFeature::DestroyFeature(feature);
        if (status != OGRERR_NONE) {
            return abandon(dataset, makeError(ErrorCode::FileExportFailure,
                                              "GDAL could not write a feature", lastGdalError()));
        }
    }

    GDALClose(dataset);
    return {};
}

bool driverHoldsOneGeometryType(const std::string& driver)
{
    return driver == "ESRI Shapefile" || driver == "MapInfo File";
}

bool driverHasFixedFields(const std::string& driver)
{
    return driver == "DXF";
}

bool driverAssumesWgs84(const std::string& driver)
{
    return driver == "GeoJSON";
}

std::string gdalVersion()
{
    ensureRegistered();
    const char* version = GDALVersionInfo("RELEASE_NAME");
    return version != nullptr ? version : "unknown";
}

} // namespace katana::gis
