#include "katana/gis/gdal_adapter.hpp"

#include <algorithm>
#include <cmath>
#include <cstring>
#include <cwctype>
#include <filesystem>
#include <functional>
#include <limits>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <set>
#include <string>
#include <string_view>
#include <system_error>
#include <variant>
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
#include <ogr_spatialref.h>
#include <ogrsf_frmts.h>

#include "cpl_error_collector.hpp"
#include "gdal_registry.hpp"
#include "geo/formats_detail.hpp"
#include "katana/core/text.hpp"
#include "katana/gis/formats.hpp"
#include "katana/gis/processing.hpp"
#include "katana/gis/reproject.hpp"
#include "ogr_detail.hpp"
#include "proj_search_paths.hpp"

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
        katana::io_detail::pointGdalAtProjData();
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
    // ASCII only, whatever locale the GUI toolkit has set (core/text.hpp).
    return katana::core::lowered(extension);
}

struct DriverForExtension {
    const char* extension;
    const char* driver;
};

// The raster formats a DEM is written in: the three every GIS and every
// survey package reads. AAIGrid can only COPY a finished dataset, which is
// what writeRaster's in-memory build is for.
constexpr DriverForExtension kRasterDrivers[] = {
    {"tif", "GTiff"},
    {"tiff", "GTiff"},
    {"asc", "AAIGrid"},
    {"img", "HFA"},
};

// A failed write must not leave a file behind that looks like a finished one:
// the driver is asked to delete it (it knows a format's sidecars), and when it
// cannot - its generic Delete OPENS the file to list them, which a truncated
// file may not survive - the file itself is removed. Quietly, because this runs
// only on a path that is already reporting a failure, whose message matters
// more than GDAL's complaint about the clean-up.
void discardOutput(GDALDriver& driver, const std::filesystem::path& path)
{
    CPLPushErrorHandler(CPLQuietErrorHandler);
    const CPLErr deleted = driver.Delete(path.string().c_str());
    CPLPopErrorHandler();
    if (deleted != CE_None) {
        std::error_code ignored;
        std::filesystem::remove(path, ignored);
    }
}

// Parses a coordinate system the way GDAL's own SetProjection would - WKT1,
// WKT2, PROJJSON - but refuses to fetch anything: a string read from a file is
// not permission to open another file or a URL.
bool parseCrs(const std::string& text, OGRSpatialReference& reference)
{
    reference.SetAxisMappingStrategy(OAMS_TRADITIONAL_GIS_ORDER);
    if (reference.importFromWkt(text.c_str()) == OGRERR_NONE) {
        return true;
    }
    return reference.SetFromUserInput(
               text.c_str(), OGRSpatialReference::SET_FROM_USER_INPUT_LIMITATIONS_get()) ==
           OGRERR_NONE;
}

// ---- OGR geometry -> VectorGeometry ---------------------------------------

// The points of a line or of a CircularString (both are simple curves): a
// CircularString's points are the ends and the middles of its arcs.
std::vector<GeoPoint> pointsOf(const OGRSimpleCurve& line)
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

// A curve's points appended to `points`, and the index each of its arcs
// starts at appended to `arcs` (VectorGeometry::arcs). A CompoundCurve's
// members meet end to start, so each after the first gives its points but
// its first.
void appendCurve(const OGRCurve& curve, std::vector<GeoPoint>& points,
                 std::vector<std::size_t>& arcs)
{
    const OGRwkbGeometryType type = wkbFlatten(curve.getGeometryType());
    if (type == wkbCompoundCurve) {
        const auto* compound = curve.toCompoundCurve();
        for (int i = 0; i < compound->getNumCurves(); ++i) {
            appendCurve(*compound->getCurve(i), points, arcs);
        }
        return;
    }
    const std::vector<GeoPoint> own = pointsOf(*curve.toSimpleCurve());
    const std::size_t start = points.empty() ? 0 : points.size() - 1;
    points.insert(points.end(), own.begin() + (points.empty() || own.empty() ? 0 : 1), own.end());
    if (type == wkbCircularString) {
        // (0, 1, 2), (2, 3, 4) ...: an arc every two points.
        for (std::size_t k = 0; k + 2 < own.size(); k += 2) {
            arcs.push_back(start + k);
        }
    }
}

bool isCurve(OGRwkbGeometryType type)
{
    switch (wkbFlatten(type)) {
    case wkbCircularString:
    case wkbCompoundCurve:
    case wkbCurvePolygon:
    case wkbMultiCurve:
    case wkbMultiSurface:
        return true;
    default:
        return false;
    }
}

// Appends one VectorGeometry per SIMPLE geometry. A multi-geometry or
// collection is recursed into so that every emitted VectorGeometry has
// exactly one kind, which is what keeps the consumer free of nested-variant
// handling. A curve is kept as its arcs when `keepArcs` is set (IMPORT turns
// them into Katana's arcs and circles), and made chords at GDAL's own step
// otherwise; what has no Katana counterpart - a TIN, a polyhedral surface -
// is counted in `report`, never silently dropped.
void flatten(const OGRGeometry* geometry, std::vector<VectorGeometry>& out, bool keepArcs,
             VectorReadReport& report, int depth = 0)
{
    if (geometry == nullptr || geometry->IsEmpty()) {
        return;
    }
    // Defensive: OGR geometries are trees and a malformed file could in
    // principle nest collections deeply. 32 is far beyond anything meaningful.
    if (depth > 32) {
        ++report.skipped["nested too deep"];
        return;
    }

    const OGRwkbGeometryType type = wkbFlatten(geometry->getGeometryType());
    if (!keepArcs && isCurve(type)) {
        // The chords GDAL makes at its own step (OGR_ARC_STEPSIZE, 4 degrees):
        // what an algorithm is handed. IMPORT keeps the arcs and makes its
        // chords by the one rule EXPORT uses (docs/interop.md, "Curves").
        const std::unique_ptr<OGRGeometry> linear(geometry->getLinearGeometry());
        if (linear != nullptr) {
            ++report.curvesMadeChords;
            flatten(linear.get(), out, keepArcs, report, depth + 1);
        }
        return;
    }

    switch (type) {
    case wkbPoint: {
        const auto* point = geometry->toPoint();
        VectorGeometry part;
        part.kind = GeometryKind::Point;
        part.parts.push_back({GeoPoint{point->getX(), point->getY(),
                                       point->Is3D() != 0 ? point->getZ() : 0.0}});
        part.hasZ = point->Is3D() != 0;
        out.push_back(std::move(part));
        return;
    }
    case wkbLineString:
    case wkbCircularString:
    case wkbCompoundCurve: {
        VectorGeometry part;
        part.kind = GeometryKind::LineString;
        part.parts.emplace_back();
        part.arcs.emplace_back();
        appendCurve(*geometry->toCurve(), part.parts.back(), part.arcs.back());
        part.hasZ = geometry->Is3D() != 0;
        if (!part.hasArcs()) {
            part.arcs.clear();
        }
        out.push_back(std::move(part));
        return;
    }
    // A Triangle is a polygon of three sides (OGRTriangle is an OGRPolygon).
    case wkbPolygon:
    case wkbTriangle:
    case wkbCurvePolygon: {
        const auto* polygon = geometry->toCurvePolygon();
        VectorGeometry part;
        part.kind = GeometryKind::Polygon;
        const auto addRing = [&part](const OGRCurve* ring) {
            if (ring == nullptr || ring->IsEmpty()) {
                return;
            }
            part.parts.emplace_back();
            part.arcs.emplace_back();
            appendCurve(*ring, part.parts.back(), part.arcs.back());
        };
        addRing(polygon->getExteriorRingCurve());
        for (int i = 0; i < polygon->getNumInteriorRings(); ++i) {
            addRing(polygon->getInteriorRingCurve(i));
        }
        if (part.parts.empty()) {
            return;
        }
        part.hasZ = polygon->Is3D() != 0;
        if (!part.hasArcs()) {
            part.arcs.clear();
        }
        out.push_back(std::move(part));
        return;
    }
    case wkbMultiPoint:
    case wkbMultiLineString:
    case wkbMultiPolygon:
    case wkbMultiCurve:
    case wkbMultiSurface:
    case wkbGeometryCollection: {
        const auto* collection = geometry->toGeometryCollection();
        for (int i = 0; i < collection->getNumGeometries(); ++i) {
            flatten(collection->getGeometryRef(i), out, keepArcs, report, depth + 1);
        }
        return;
    }
    // Faces of a 3D model: Katana has no entity that is one, and a TIN
    // taken apart into triangles would be thousands of closed polylines that
    // are no longer a surface. Counted and said by name instead.
    case wkbTIN:
        ++report.skipped["tin"];
        return;
    case wkbPolyhedralSurface:
        ++report.skipped["polyhedral surface"];
        return;
    default:
        ++report.skipped[std::string("unsupported ") + OGRGeometryTypeToName(type)];
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

// A part with arcs as the curve it was read from: straight runs as line
// strings, each arc a CircularString of its three points, joined in a
// CompoundCurve. A driver without curves makes its own chords of it.
OGRCompoundCurve* makeCurve(const std::vector<GeoPoint>& points,
                            const std::vector<std::size_t>& arcs, bool hasZ, bool closed)
{
    auto* compound = new OGRCompoundCurve();
    std::size_t at = 0;
    const auto straightTo = [&](std::size_t end) {
        if (end <= at) {
            return;
        }
        auto* line = new OGRLineString();
        for (std::size_t i = at; i <= end; ++i) {
            addVertex(*line, points[i], hasZ);
        }
        (void)compound->addCurveDirectly(line);
        at = end;
    };
    for (const std::size_t arc : arcs) {
        if (arc + 2 >= points.size() || arc < at) {
            continue; // an index that names no arc of this part
        }
        straightTo(arc);
        auto* circular = new OGRCircularString();
        for (std::size_t i = arc; i <= arc + 2; ++i) {
            addVertex(*circular, points[i], hasZ);
        }
        (void)compound->addCurveDirectly(circular);
        at = arc + 2;
    }
    straightTo(points.size() - 1);
    const GeoPoint& first = points.front();
    const GeoPoint& last = points.back();
    if (closed && (first.x != last.x || first.y != last.y)) {
        auto* closing = new OGRLineString();
        addVertex(*closing, last, hasZ);
        addVertex(*closing, first, hasZ);
        (void)compound->addCurveDirectly(closing);
    }
    return compound;
}

OGRGeometry* makeGeometry(const VectorGeometry& geometry)
{
    const auto arcsOf = [&geometry](std::size_t part) -> const std::vector<std::size_t>* {
        return part < geometry.arcs.size() && !geometry.arcs[part].empty() ? &geometry.arcs[part]
                                                                           : nullptr;
    };
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
        if (const auto* arcs = arcsOf(0)) {
            return makeCurve(geometry.parts.front(), *arcs, geometry.hasZ, false);
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
        if (geometry.hasArcs()) {
            auto* polygon = new OGRCurvePolygon();
            for (std::size_t r = 0; r < geometry.parts.size(); ++r) {
                const auto& part = geometry.parts[r];
                if (part.size() < 3) {
                    continue;
                }
                const auto* arcs = arcsOf(r);
                (void)polygon->addRingDirectly(makeCurve(part, arcs != nullptr ? *arcs
                                                                               : std::vector<std::size_t>{},
                                                         geometry.hasZ, true));
            }
            return polygon;
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

// ---- typed fields ---------------------------------------------------------

namespace gp = katana::gis::processing;

gp::FieldType fieldTypeOf(const OGRFieldDefn& field)
{
    switch (field.GetType()) {
    case OFTInteger:
        return field.GetSubType() == OFSTBoolean ? gp::FieldType::Boolean
                                                 : gp::FieldType::Integer64;
    case OFTInteger64:
        return gp::FieldType::Integer64;
    case OFTReal:
        return gp::FieldType::Real;
    case OFTDate:
        return gp::FieldType::Date;
    case OFTDateTime:
        return gp::FieldType::DateTime;
    default:
        // Text, a time of day, lists and binary: read as GDAL writes them out.
        return gp::FieldType::String;
    }
}

// An ISO 8601 date or date-time, as FieldType::Date and DateTime carry them.
std::string isoDate(const OGRFeature& feature, int index, bool withTime)
{
    int year = 0, month = 0, day = 0, hour = 0, minute = 0, zone = 0;
    float second = 0.0F;
    if (feature.GetFieldAsDateTime(index, &year, &month, &day, &hour, &minute, &second, &zone) ==
        FALSE) {
        return feature.GetFieldAsString(index);
    }
    // Whole numbers only: printf's %f would follow the C locale's decimal point.
    const auto two = [](int value) { return (value < 10 ? "0" : "") + std::to_string(value); };
    std::string text = std::to_string(year) + "-" + two(month) + "-" + two(day);
    if (withTime) {
        const int whole = static_cast<int>(second);
        const int millis =
            static_cast<int>(std::lround((second - static_cast<float>(whole)) * 1000.0F));
        text += "T" + two(hour) + ":" + two(minute) + ":" + two(whole);
        if (millis > 0) {
            text += "." + std::string(millis < 100 ? (millis < 10 ? "00" : "0") : "") +
                    std::to_string(millis);
        }
    }
    return text;
}

gp::FieldValue fieldValue(const OGRFeature& feature, int index, gp::FieldType type)
{
    if (!feature.IsFieldSetAndNotNull(index)) {
        return std::monostate{};
    }
    switch (type) {
    case gp::FieldType::Boolean:
        return feature.GetFieldAsInteger(index) != 0;
    case gp::FieldType::Integer64:
        return static_cast<std::int64_t>(feature.GetFieldAsInteger64(index));
    case gp::FieldType::Real:
        return feature.GetFieldAsDouble(index);
    case gp::FieldType::Date:
        return isoDate(feature, index, false);
    case gp::FieldType::DateTime:
        return isoDate(feature, index, true);
    case gp::FieldType::String:
        break;
    }
    const char* text = feature.GetFieldAsString(index);
    return std::string(text != nullptr ? text : "");
}

// A value as text, for readFeatures: what GDAL's GetFieldAsString gives for
// the same field, a real at the digits that read back as the same double.
std::string textOf(const gp::FieldValue& value)
{
    return std::visit(
        [](const auto& held) -> std::string {
            using Held = std::decay_t<decltype(held)>;
            if constexpr (std::is_same_v<Held, std::monostate>) {
                return {};
            } else if constexpr (std::is_same_v<Held, bool>) {
                return held ? "1" : "0";
            } else if constexpr (std::is_same_v<Held, std::int64_t>) {
                return std::to_string(held);
            } else if constexpr (std::is_same_v<Held, double>) {
                return katana::core::formatExactReal(held);
            } else {
                return held;
            }
        },
        value);
}

GeometryKind kindOfLayer(OGRwkbGeometryType type)
{
    switch (wkbFlatten(type)) {
    case wkbPoint:
    case wkbMultiPoint:
        return GeometryKind::Point;
    case wkbLineString:
    case wkbMultiLineString:
    case wkbCircularString:
    case wkbCompoundCurve:
    case wkbMultiCurve:
        return GeometryKind::LineString;
    case wkbPolygon:
    case wkbMultiPolygon:
    case wkbCurvePolygon:
    case wkbMultiSurface:
    case wkbTriangle:
        return GeometryKind::Polygon;
    default:
        return GeometryKind::Unknown;
    }
}

std::string wktOfLayer(const OGRLayer& layer)
{
    std::string text;
    if (const OGRSpatialReference* reference = layer.GetSpatialRef()) {
        char* wkt = nullptr;
        if (reference->exportToWkt(&wkt) == OGRERR_NONE && wkt != nullptr) {
            text = wkt;
        }
        CPLFree(wkt);
    }
    return text;
}

// KML's altitude is a height only in its "absolute" mode. Its default,
// clampToGround, puts every coordinate on the ground whatever it says, and
// GDAL's KMZ writer gives a 2D coordinate an altitude of 0 - read as a
// height, that would be every feature surveyed at the datum.
bool heightIsAltitude(const OGRFeature& feature, int altitudeField)
{
    if (altitudeField < 0 || !feature.IsFieldSetAndNotNull(altitudeField)) {
        return false;
    }
    return katana::core::equalsIgnoringCase(feature.GetFieldAsString(altitudeField), "absolute");
}

void dropHeights(VectorGeometry& part)
{
    part.hasZ = false;
    for (auto& ring : part.parts) {
        for (GeoPoint& point : ring) {
            point.z = 0.0;
        }
    }
}

// A read's attribute and spatial filters set on `layer` (VectorReadOptions).
// InvalidArgument, with GDAL's reason, for an attribute filter the driver
// cannot parse - said, never read as "no features match".
Status setFilters(OGRLayer& layer, const VectorReadOptions& options)
{
    if (options.spatialFilter) {
        const auto& box = *options.spatialFilter;
        layer.SetSpatialFilterRect(box[0], box[1], box[2], box[3]);
    }
    if (!options.attributeFilter.empty()) {
        CPLErrorReset();
        if (layer.SetAttributeFilter(options.attributeFilter.c_str()) != OGRERR_NONE) {
            const std::string why = lastGdalError();
            layer.SetSpatialFilter(nullptr);
            return makeError(ErrorCode::InvalidArgument,
                             "GDAL could not read the filter" + (why.empty() ? "" : ": " + why),
                             options.attributeFilter);
        }
    }
    return {};
}

void clearFilters(OGRLayer& layer)
{
    layer.SetSpatialFilter(nullptr);
    layer.SetAttributeFilter(nullptr);
}

// Every feature of `layer` as a typed table (GdalDataset::readTable).
gp::FeatureTable readLayer(OGRLayer& layer, const std::string& driver,
                           const VectorReadOptions& options, VectorReadReport& report)
{
    gp::FeatureTable table;
    table.name = layer.GetName();
    table.kind = kindOfLayer(layer.GetGeomType());
    table.hasZ = wkbHasZ(layer.GetGeomType()) != 0;
    table.crsWkt = wktOfLayer(layer);
    const OGRFeatureDefn* definition = layer.GetLayerDefn();
    const int fieldCount = definition != nullptr ? definition->GetFieldCount() : 0;
    for (int f = 0; f < fieldCount; ++f) {
        const OGRFieldDefn* field = definition->GetFieldDefn(f);
        table.fields.push_back(gp::FieldDef{field->GetNameRef(), fieldTypeOf(*field)});
    }
    const bool kml = driver == "KML" || driver == "LIBKML";
    const int altitudeField = kml && definition != nullptr
                                  ? definition->GetFieldIndex("altitudeMode")
                                  : -1;

    layer.ResetReading();
    std::uint64_t read = 0;
    while (OGRFeature* ogr = layer.GetNextFeature()) {
        ++read;
        ++report.featuresRead;
        gp::Feature feature;
        flatten(ogr->GetGeometryRef(), feature.parts, options.keepArcs, report);
        if (ogr->GetGeometryRef() == nullptr || ogr->GetGeometryRef()->IsEmpty()) {
            ++report.skipped["no geometry"];
        }
        if (kml && !heightIsAltitude(*ogr, altitudeField)) {
            for (VectorGeometry& part : feature.parts) {
                dropHeights(part);
            }
        }
        for (const VectorGeometry& part : feature.parts) {
            table.hasZ = table.hasZ || part.hasZ;
        }
        feature.values.reserve(table.fields.size());
        for (int f = 0; f < fieldCount; ++f) {
            feature.values.push_back(
                fieldValue(*ogr, f, table.fields[static_cast<std::size_t>(f)].type));
        }
        table.features.push_back(std::move(feature));
        OGRFeature::DestroyFeature(ogr);
        if (options.maxFeatures != 0 && read >= options.maxFeatures) {
            break;
        }
    }
    return table;
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

// A GDAL configuration option set for this thread only, until the guard goes,
// and then put back as it was. Thread-local so that an export does not change
// how another thread's GDAL call behaves, and restored so that it does not
// change how the next one on this thread does.
class ScopedThreadConfig {
  public:
    ScopedThreadConfig(const char* key, const char* value) : key_(key)
    {
        if (const char* previous = CPLGetThreadLocalConfigOption(key, nullptr)) {
            previous_ = previous;
            hadPrevious_ = true;
        }
        CPLSetThreadLocalConfigOption(key, value);
    }
    ~ScopedThreadConfig()
    {
        CPLSetThreadLocalConfigOption(key_, hadPrevious_ ? previous_.c_str() : nullptr);
    }
    ScopedThreadConfig(const ScopedThreadConfig&) = delete;
    ScopedThreadConfig& operator=(const ScopedThreadConfig&) = delete;

  private:
    const char* key_;
    std::string previous_;
    bool hadPrevious_ = false;
};

} // namespace

namespace detail {

void ensureGdalRegistered()
{
    ensureRegistered();
}

OGRGeometry* makeOgrGeometry(const VectorGeometry& geometry)
{
    return ::katana::gis::makeGeometry(geometry);
}

void flattenOgrGeometry(const OGRGeometry* geometry, std::vector<VectorGeometry>& out,
                        std::vector<std::string>& warnings)
{
    VectorReadReport report;
    ::katana::gis::flatten(geometry, out, false, report);
    for (const auto& [what, count] : report.skipped) {
        warnings.push_back(std::to_string(count) + " " + what + " geometries were skipped");
    }
    if (report.curvesMadeChords != 0) {
        warnings.push_back(std::to_string(report.curvesMadeChords) +
                           " curves were made chords at GDAL's own step");
    }
}

bool parseCrs(const std::string& text, OGRSpatialReference& reference)
{
    return ::katana::gis::parseCrs(text, reference);
}

} // namespace detail

// ---- lifetime -------------------------------------------------------------

Result<std::unique_ptr<GdalDataset>> GdalDataset::open(const std::filesystem::path& path)
{
    return open(path, {});
}

Result<std::unique_ptr<GdalDataset>> GdalDataset::open(const std::filesystem::path& path,
                                                       const std::vector<std::string>& openOptions)
{
    ensureRegistered();

    // A local file that is not there is said so plainly. A /vsi path, a URL
    // or a connection string is no file on this disk, and whether it exists
    // is GDAL's to find out: std::filesystem::exists refused every one.
    std::error_code existsError;
    if (!isVirtualPath(path) && !std::filesystem::exists(path, existsError)) {
        return makeError(ErrorCode::NotFound, "file does not exist", path.string());
    }

    CPLStringList options;
    for (const std::string& option : openOptions) {
        options.AddString(option.c_str());
    }
    // The path as it is, then - for an archive GDAL does not open as it is,
    // a .zip of a shapefile - its inside (formats_detail.hpp). The first
    // failure's message is the one worth reporting.
    const detail::OpenNames names = detail::openNames(path);
    void* handle = nullptr;
    std::string failure;
    for (const std::string& name : names.names) {
        CPLErrorReset();
        handle = GDALOpenEx(name.c_str(), GDAL_OF_READONLY | GDAL_OF_RASTER | GDAL_OF_VECTOR,
                            nullptr, options.List(), nullptr);
        if (handle != nullptr) {
            break;
        }
        if (failure.empty()) {
            failure = lastGdalError();
        }
    }
    if (handle == nullptr) {
        if (!names.ambiguity.empty()) {
            return makeError(ErrorCode::InvalidArgument, names.ambiguity, path.string());
        }
        return makeError(ErrorCode::FileImportFailure,
                         "GDAL could not open '" + path.string() + "'", failure);
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
    int hasNoData = 0;
    const double noData = dataset->GetRasterBand(1)->GetNoDataValue(&hasNoData);
    if (hasNoData != 0) {
        info.noDataValue = noData;
    }
    return info;
}

std::string GdalDataset::driverName() const
{
    const GDALDriver* driver = asDataset(dataset_)->GetDriver();
    // GDAL keeps a driver's short name in its description.
    return driver != nullptr ? std::string(driver->GetDescription()) : std::string();
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

Result<RasterSamples> GdalDataset::readBandSampled(int bandIndex, int stride) const
{
    GDALDataset* dataset = asDataset(dataset_);
    if (bandIndex < 1 || bandIndex > dataset->GetRasterCount()) {
        return makeError(ErrorCode::InvalidArgument, "raster band index is out of range",
                         "requested " + std::to_string(bandIndex) + " of " +
                             std::to_string(dataset->GetRasterCount()));
    }
    if (stride < 1) {
        return makeError(ErrorCode::InvalidArgument, "the sampling stride must be at least 1",
                         "stride " + std::to_string(stride));
    }

    const int width = dataset->GetRasterXSize();
    const int height = dataset->GetRasterYSize();
    // In 64 bits: width + stride - 1 overflows an int for a stride near
    // INT_MAX, which strideForCap can legitimately return for a one-sample cap.
    const auto samplesAlong = [stride](int extent) {
        return static_cast<int>((static_cast<std::int64_t>(extent) + stride - 1) / stride);
    };

    RasterSamples samples;
    samples.stride = stride;
    samples.columns = samplesAlong(width);
    samples.rows = samplesAlong(height);
    GDALRasterBand* band = dataset->GetRasterBand(bandIndex);
    int hasNoData = 0;
    const double noData = band->GetNoDataValue(&hasNoData);
    if (hasNoData != 0) {
        samples.noDataValue = noData;
    }

    // One whole source row per kept row, and the kept pixels picked out of it.
    // Asking RasterIO for a smaller buffer instead would make GDAL RESAMPLE -
    // nearest neighbour at pixel (i + 0.5) * step, not i * step - which moves
    // every sample off the pixel the caller will georeference it to.
    samples.values.reserve(static_cast<std::size_t>(samples.columns) *
                           static_cast<std::size_t>(samples.rows));
    std::vector<double> row(static_cast<std::size_t>(width));
    for (int j = 0; j < samples.rows; ++j) {
        // (rows - 1) * stride < height, so this stays within an int.
        const int sourceRow = j * stride;
        if (band->RasterIO(GF_Read, 0, sourceRow, width, 1, row.data(), width, 1, GDT_Float64, 0,
                           0, nullptr) != CE_None) {
            return makeError(ErrorCode::FileImportFailure, "GDAL failed to read raster band",
                             "row " + std::to_string(sourceRow) + ": " + lastGdalError());
        }
        const auto step = static_cast<std::size_t>(stride);
        for (std::size_t i = 0; i < static_cast<std::size_t>(samples.columns); ++i) {
            samples.values.push_back(row[i * step]);
        }
    }
    return samples;
}

Result<RasterImage> GdalDataset::readImage(int maxPixels) const
{
    return readImage(maxPixels, 0);
}

Result<RasterImage> GdalDataset::readImage(int maxPixels, int onlyBand) const
{
    if (onlyBand < 0 || onlyBand > asDataset(dataset_)->GetRasterCount()) {
        return makeError(ErrorCode::InvalidArgument,
                         "the raster has " + std::to_string(asDataset(dataset_)->GetRasterCount()) +
                             " bands",
                         "band " + std::to_string(onlyBand));
    }
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

    // One band asked for is that band alone, through its colour table when
    // it has one - never mixed into a colour the file says its bands make.
    BandRoles roles;
    if (onlyBand > 0) {
        GDALRasterBand* chosen = dataset->GetRasterBand(onlyBand);
        if (chosen->GetColorInterpretation() == GCI_PaletteIndex &&
            chosen->GetColorTable() != nullptr) {
            roles.palette = chosen;
        } else {
            roles.grey = chosen;
        }
    } else {
        roles = classifyBands(*dataset);
    }

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

    // The stretch is the range of the values just read, never GDAL's band
    // statistics (audit IO-13). Forcing those (GetStatistics with bForce) makes
    // GDAL's persistent auxiliary metadata write <file>.aux.xml beside the
    // user's file - into their data folder, even on a read-only open - and on
    // the next open GDAL trusts that cache without checking it against the
    // file, so a DEM replaced under the same name was stretched by the OLD
    // DEM's range and clamped to white. The decimated copy is the image being
    // drawn, so its own range is the right one to stretch over anyway.
    double minimum = std::numeric_limits<double>::max();
    double maximum = std::numeric_limits<double>::lowest();
    for (const double value : values) {
        if (!std::isfinite(value) || (hasNoData != 0 && value == noData)) {
            continue;
        }
        minimum = std::min(minimum, value);
        maximum = std::max(maximum, value);
    }
    const bool haveRange = maximum > minimum;

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

Result<gp::FeatureTable> GdalDataset::readTable(int layerIndex, const VectorReadOptions& options,
                                                VectorReadReport* report) const
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
    VectorReadReport local;
    VectorReadReport& out = report != nullptr ? *report : local;
    // Warnings GDAL raises while this read runs, on this thread; the
    // process-wide quiet handler keeps them off stderr, and this keeps them
    // from being lost (they used to be).
    const detail::CplErrorCollector errors;
    auto filtered = setFilters(*layer, options);
    if (!filtered) {
        return filtered.error();
    }
    gp::FeatureTable table = readLayer(*layer, driverName(), options, out);
    // The layer belongs to the dataset, which may be read again: a filter
    // left on it would quietly narrow the next read.
    clearFilters(*layer);
    for (std::string& warning : errors.warnings()) {
        out.warnings.push_back(std::move(warning));
    }
    return table;
}

Result<gp::FeatureTable> GdalDataset::readSql(const std::string& statement,
                                              const std::string& dialect,
                                              const VectorReadOptions& options,
                                              VectorReadReport* report) const
{
    // IMPORT reads; it never writes to the file it imports. The dataset is
    // open read-only, so a write would fail anyway - but with a driver's
    // message about permissions, not the reason.
    const std::string_view text = katana::core::trimmed(statement);
    const std::size_t end = text.find_first_of(" \t\r\n(");
    const std::string first = katana::core::lowered(std::string(text.substr(0, end)));
    if (first != "select" && first != "with") {
        return makeError(ErrorCode::InvalidArgument,
                         "only a SELECT statement is read: an import never changes its file",
                         std::string(text));
    }
    const std::string folded = katana::core::lowered(dialect);
    if (!folded.empty() && folded != "ogrsql" && folded != "sqlite") {
        return makeError(ErrorCode::InvalidArgument, "the SQL dialect is ogrsql or sqlite",
                         dialect);
    }
    GDALDataset* dataset = asDataset(dataset_);
    VectorReadReport local;
    VectorReadReport& out = report != nullptr ? *report : local;
    const detail::CplErrorCollector errors;
    std::unique_ptr<OGRPolygon> area;
    if (options.spatialFilter) {
        const auto& box = *options.spatialFilter;
        auto ring = std::make_unique<OGRLinearRing>();
        ring->addPoint(box[0], box[1]);
        ring->addPoint(box[2], box[1]);
        ring->addPoint(box[2], box[3]);
        ring->addPoint(box[0], box[3]);
        ring->addPoint(box[0], box[1]);
        area = std::make_unique<OGRPolygon>();
        area->addRingDirectly(ring.release());
    }
    CPLErrorReset();
    OGRLayer* rows = dataset->ExecuteSQL(std::string(text).c_str(), area.get(),
                                         folded.empty() ? nullptr
                                                        : (folded == "sqlite" ? "SQLITE"
                                                                              : "OGRSQL"));
    if (rows == nullptr) {
        const std::string why = lastGdalError();
        return makeError(ErrorCode::InvalidArgument,
                         why.empty() ? std::string("the statement gives no rows to import")
                                     : "GDAL could not run the statement: " + why,
                         std::string(text));
    }
    // The result set is the dataset's to free, whatever happens below.
    const std::unique_ptr<OGRLayer, std::function<void(OGRLayer*)>> held(
        rows, [dataset](OGRLayer* layer) { dataset->ReleaseResultSet(layer); });
    VectorReadOptions rest = options;
    rest.spatialFilter.reset(); // ExecuteSQL has it
    auto filtered = setFilters(*rows, rest);
    if (!filtered) {
        return filtered.error();
    }
    gp::FeatureTable table = readLayer(*rows, driverName(), rest, out);
    for (std::string& warning : errors.warnings()) {
        out.warnings.push_back(std::move(warning));
    }
    return table;
}

Result<std::vector<VectorFeature>> GdalDataset::readFeatures(int layerIndex,
                                                             std::uint64_t maxFeatures) const
{
    VectorReadReport report;
    return readFeatures(layerIndex, maxFeatures, report);
}

Result<std::vector<VectorFeature>> GdalDataset::readFeatures(int layerIndex,
                                                             std::uint64_t maxFeatures,
                                                             VectorReadReport& report) const
{
    VectorReadOptions options;
    options.maxFeatures = maxFeatures;
    auto table = readTable(layerIndex, options, &report);
    if (!table) {
        return table.error();
    }
    std::vector<VectorFeature> features;
    for (gp::Feature& feature : table->features) {
        std::map<std::string, std::string> attributes;
        for (std::size_t f = 0; f < table->fields.size() && f < feature.values.size(); ++f) {
            if (!std::holds_alternative<std::monostate>(feature.values[f])) {
                attributes.emplace(table->fields[f].name, textOf(feature.values[f]));
            }
        }
        for (VectorGeometry& part : feature.parts) {
            features.push_back(VectorFeature{std::move(part), attributes});
        }
    }
    return features;
}

// ---- writing --------------------------------------------------------------

Result<std::string> GdalDataset::vectorDriverForPath(const std::filesystem::path& path)
{
    // GDAL's registry, and its own choice among the writers that claim a
    // name, with Katana's overlay (formats.hpp): the hand-kept table of ten
    // extensions this replaced could not write a FlatGeobuf, a GeoParquet,
    // a KMZ or a GPX that GDAL has writers for.
    return vectorWriterFor(path);
}

Result<std::string> GdalDataset::rasterDriverForPath(const std::filesystem::path& path)
{
    ensureRegistered();
    const std::string extension = lowerExtension(path);
    for (const DriverForExtension& entry : kRasterDrivers) {
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
    if (extension.empty()) {
        return makeError(ErrorCode::Unsupported,
                         "cannot choose a raster driver: the path has no extension",
                         path.string());
    }
    return makeError(ErrorCode::Unsupported,
                     "no raster driver is registered for '." + extension + "'", path.string());
}

Status GdalDataset::writeRaster(const std::filesystem::path& path,
                                const RasterExportOptions& options,
                                const std::vector<double>& values)
{
    ensureRegistered();
    // A message GDAL left from an earlier call must not become this call's
    // explanation of a failure GDAL did not describe.
    CPLErrorReset();

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

    // Two kinds of driver. GTiff and HFA CREATE a dataset and take its pixels
    // afterwards; AAIGrid (and PNG, JPEG...) can only COPY a dataset that is
    // already complete, because they write the file in one pass. For the
    // second kind the dataset is built in GDAL's in-memory driver and copied,
    // so a caller names the format it wants without knowing which kind it is.
    const CSLConstList capabilities = driver->GetMetadata();
    const bool canCreate = CPLFetchBool(capabilities, GDAL_DCAP_CREATE, false);
    const bool canCopy = CPLFetchBool(capabilities, GDAL_DCAP_CREATECOPY, false);
    if (!canCreate && !canCopy) {
        return makeError(ErrorCode::Unsupported, "GDAL cannot write this raster format",
                         options.driver);
    }
    GDALDriver* builder = canCreate ? driver : GetGDALDriverManager()->GetDriverByName("MEM");
    if (builder == nullptr) {
        return makeError(ErrorCode::Unsupported,
                         "this GDAL build has no in-memory driver to build the raster in",
                         options.driver);
    }

    GDALDataset* dataset = builder->Create(canCreate ? path.string().c_str() : "", options.width,
                                           options.height, 1, GDT_Float64, nullptr);
    if (dataset == nullptr) {
        return makeError(ErrorCode::FileExportFailure,
                         "GDAL could not create '" + path.string() + "'", lastGdalError());
    }

    // Every failure from here unwinds through this: close, then delete what
    // the driver created, so a raster without its georeferencing - pixels
    // that read back at the origin, one unit each - is never left looking
    // like a finished export (audit IO-14). Setters used to be called and
    // their answers ignored.
    const auto abandon = [&](GDALDataset* handle, const char* what) -> Error {
        const std::string message = lastGdalError();
        GDALClose(handle); // already failing: the first error is the one to report
        if (canCreate) {
            discardOutput(*driver, path);
        }
        return makeError(ErrorCode::FileExportFailure, what, message);
    };

    std::array<double, 6> geotransform = options.geotransform;
    if (dataset->SetGeoTransform(geotransform.data()) != CE_None) {
        return abandon(dataset, "GDAL could not set the raster's georeferencing");
    }
    if (!options.projectionWkt.empty()) {
        OGRSpatialReference reference;
        if (!parseCrs(options.projectionWkt, reference)) {
            return abandon(dataset, "GDAL could not read the coordinate system to write");
        }
        if (dataset->SetSpatialRef(&reference) != CE_None) {
            return abandon(dataset, "GDAL could not set the raster's coordinate system");
        }
    }
    GDALRasterBand* band = dataset->GetRasterBand(1);
    if (options.noDataValue.has_value() && band->SetNoDataValue(*options.noDataValue) != CE_None) {
        return abandon(dataset, "GDAL could not set the raster's no-data value");
    }
    if (band->RasterIO(GF_Write, 0, 0, options.width, options.height,
                       const_cast<double*>(values.data()), options.width, options.height,
                       GDT_Float64, 0, 0, nullptr) != CE_None) {
        return abandon(dataset, "GDAL failed to write raster");
    }

    if (!canCreate) {
        GDALDataset* copy =
            driver->CreateCopy(path.string().c_str(), dataset, FALSE, nullptr, nullptr, nullptr);
        if (copy == nullptr) {
            const std::string message = lastGdalError();
            GDALClose(dataset);
            discardOutput(*driver, path);
            return makeError(ErrorCode::FileExportFailure,
                             "GDAL could not write '" + path.string() + "'", message);
        }
        // Closing an in-memory dataset only frees it; there is nothing it
        // could fail to write.
        GDALClose(dataset);
        dataset = copy;
    }

    // GDAL (3.7 on) reports here what it could only find out while flushing
    // the last blocks and the header: a full disk, a network share gone.
    if (GDALClose(dataset) != CE_None) {
        const std::string message = lastGdalError();
        discardOutput(*driver, path);
        return makeError(ErrorCode::FileExportFailure,
                         "GDAL could not finish writing '" + path.string() + "'", message);
    }
    return {};
}

namespace {

// KEY=VALUE options, Katana's first and the caller's over them: a key the
// caller gives wins, whatever its case.
CPLStringList mergedOptions(const std::vector<std::pair<std::string, std::string>>& katana,
                            const std::vector<std::string>& caller)
{
    CPLStringList merged;
    for (const auto& [key, value] : katana) {
        merged.SetNameValue(key.c_str(), value.c_str());
    }
    for (const std::string& option : caller) {
        const std::size_t equals = option.find('=');
        if (equals == std::string::npos || equals == 0) {
            merged.AddString(option.c_str()); // GDAL says what it makes of it
            continue;
        }
        merged.SetNameValue(option.substr(0, equals).c_str(), option.substr(equals + 1).c_str());
    }
    return merged;
}

bool listed(const char* list, std::string_view word)
{
    if (list == nullptr) {
        return false;
    }
    const CPLStringList words(CSLTokenizeString2(list, " ", 0));
    for (int i = 0; i < words.size(); ++i) {
        if (word == words[i]) {
            return true;
        }
    }
    return false;
}

// The OGR type a field is written as: its own where the driver has it, else
// the nearest the driver has that holds every value - an Integer64 that
// fits in 32 bits is an Integer in KML, whose writer knows no Integer64 and
// would write it as text. A driver that lists nothing takes what it is given.
struct OgrField {
    OGRFieldType type = OFTString;
    OGRFieldSubType subType = OFSTNone;
};

OgrField ogrFieldFor(const gp::FieldDef& field, const gp::FeatureTable& table, std::size_t index,
                     GDALDriver& driver)
{
    const char* types = driver.GetMetadataItem(GDAL_DMD_CREATIONFIELDDATATYPES);
    const char* subTypes = driver.GetMetadataItem(GDAL_DMD_CREATIONFIELDDATASUBTYPES);
    const bool anyType = types == nullptr || *types == '\0';
    const auto has = [&](std::string_view name) { return anyType || listed(types, name); };
    // KML's schema has no 64-bit integer (OGC KML 2.2, 9.5: a SimpleField is
    // int, uint, short, ushort, float, double, bool or string), and LIBKML,
    // which declares Integer64 all the same, writes one as a string: a count
    // came back "3". So for it an Integer64 goes as an int where it fits.
    const bool integer64AsText = std::string_view(driver.GetDescription()) == "LIBKML";
    switch (field.type) {
    case gp::FieldType::Boolean:
        if (has("Integer")) {
            return {OFTInteger, anyType || listed(subTypes, "Boolean") ? OFSTBoolean : OFSTNone};
        }
        break;
    case gp::FieldType::Integer64: {
        if (has("Integer64") && !integer64AsText) {
            return {OFTInteger64, OFSTNone};
        }
        const bool fits = std::ranges::all_of(table.features, [&](const gp::Feature& feature) {
            const auto* value = index < feature.values.size()
                                    ? std::get_if<std::int64_t>(&feature.values[index])
                                    : nullptr;
            return value == nullptr || (*value >= std::numeric_limits<std::int32_t>::min() &&
                                        *value <= std::numeric_limits<std::int32_t>::max());
        });
        if (fits && has("Integer")) {
            return {OFTInteger, OFSTNone};
        }
        break;
    }
    case gp::FieldType::Real:
        if (has("Real")) {
            return {OFTReal, OFSTNone};
        }
        break;
    case gp::FieldType::Date:
        if (has("Date")) {
            return {OFTDate, OFSTNone};
        }
        break;
    case gp::FieldType::DateTime:
        if (has("DateTime")) {
            return {OFTDateTime, OFSTNone};
        }
        break;
    case gp::FieldType::String:
        break;
    }
    return {OFTString, OFSTNone};
}

OGRwkbGeometryType layerTypeOf(const gp::FeatureTable& table)
{
    const bool multi = std::ranges::any_of(
        table.features, [](const gp::Feature& feature) { return feature.parts.size() > 1; });
    const bool curved = std::ranges::any_of(table.features, [](const gp::Feature& feature) {
        return std::ranges::any_of(feature.parts,
                                   [](const VectorGeometry& part) { return part.hasArcs(); });
    });
    OGRwkbGeometryType type = wkbUnknown;
    switch (table.kind) {
    case GeometryKind::Point:
        type = multi ? wkbMultiPoint : wkbPoint;
        break;
    case GeometryKind::LineString:
        type = curved ? (multi ? wkbMultiCurve : wkbCompoundCurve)
                      : (multi ? wkbMultiLineString : wkbLineString);
        break;
    case GeometryKind::Polygon:
        type = curved ? (multi ? wkbMultiSurface : wkbCurvePolygon)
                      : (multi ? wkbMultiPolygon : wkbPolygon);
        break;
    case GeometryKind::Unknown:
        return wkbUnknown;
    }
    return table.hasZ ? wkbSetZ(type) : type;
}

OGRGeometry* featureGeometry(const gp::Feature& feature, GeometryKind kind)
{
    if (feature.parts.empty()) {
        return nullptr;
    }
    if (feature.parts.size() == 1) {
        return makeGeometry(feature.parts.front());
    }
    OGRGeometryCollection* collection = nullptr;
    switch (kind) {
    case GeometryKind::Point:
        collection = new OGRMultiPoint();
        break;
    case GeometryKind::LineString:
        collection = new OGRMultiLineString();
        break;
    case GeometryKind::Polygon:
        collection = new OGRMultiPolygon();
        break;
    case GeometryKind::Unknown:
        collection = new OGRGeometryCollection();
        break;
    }
    for (const VectorGeometry& part : feature.parts) {
        if (OGRGeometry* member = makeGeometry(part)) {
            if (collection->addGeometryDirectly(member) != OGRERR_NONE) {
                // A curve cannot join a MultiLineString: the collection that
                // can hold anything takes the whole feature instead.
                delete member;
                delete collection;
                auto* anything = new OGRGeometryCollection();
                for (const VectorGeometry& again : feature.parts) {
                    if (OGRGeometry* one = makeGeometry(again)) {
                        (void)anything->addGeometryDirectly(one);
                    }
                }
                return anything;
            }
        }
    }
    return collection;
}

// The extent of a table's coordinates: MapInfo's bounds.
struct Extent {
    double minX = std::numeric_limits<double>::infinity();
    double minY = std::numeric_limits<double>::infinity();
    double maxX = -std::numeric_limits<double>::infinity();
    double maxY = -std::numeric_limits<double>::infinity();
    [[nodiscard]] bool valid() const { return minX <= maxX && minY <= maxY; }
};

Extent extentOf(const gp::FeatureTable& table)
{
    Extent extent;
    for (const gp::Feature& feature : table.features) {
        for (const VectorGeometry& part : feature.parts) {
            for (const auto& ring : part.parts) {
                for (const GeoPoint& point : ring) {
                    if (!std::isfinite(point.x) || !std::isfinite(point.y)) {
                        continue;
                    }
                    extent.minX = std::min(extent.minX, point.x);
                    extent.minY = std::min(extent.minY, point.y);
                    extent.maxX = std::max(extent.maxX, point.x);
                    extent.maxY = std::max(extent.maxY, point.y);
                }
            }
        }
    }
    return extent;
}

// A MapInfo table keeps each coordinate as a 32-bit integer across its
// bounds, and GDAL's default bounds for a table without a known projection
// are wide enough to make that step about a centimetre: 330100 read back as
// 330099.99 (measured, docs/interop.md "Fidelity"). The data's own extent,
// widened by a tenth on each side so an edit that moves a vertex out a
// little still fits, makes the step span / 2^32: 5e-8 m across a 200 m site,
// 5e-4 m across 2000 km.
std::string mapInfoBounds(const Extent& extent)
{
    const double span = std::max(extent.maxX - extent.minX, extent.maxY - extent.minY);
    const double margin = 0.1 * span + 1.0;
    return katana::core::formatExactReal(extent.minX - margin) + "," +
           katana::core::formatExactReal(extent.minY - margin) + "," +
           katana::core::formatExactReal(extent.maxX + margin) + "," +
           katana::core::formatExactReal(extent.maxY + margin);
}

std::vector<std::pair<std::string, std::string>> katanaLayerOptions(const std::string& driver,
                                                                    const gp::FeatureTable& table)
{
    std::vector<std::pair<std::string, std::string>> options;
    if (driver == "CSV") {
        // Without a geometry option the CSV writer drops the geometry and
        // writes the fields alone (measured: "katana_id,layer", reported as
        // exported). Points as X, Y (and Z when every one has a height), for
        // a spreadsheet; anything else, or points of mixed dimension, as WKT.
        const bool points = table.kind == GeometryKind::Point &&
                            std::ranges::none_of(table.features, [](const gp::Feature& feature) {
                                return feature.parts.size() > 1;
                            });
        std::size_t heighted = 0, total = 0;
        for (const gp::Feature& feature : table.features) {
            for (const VectorGeometry& part : feature.parts) {
                ++total;
                heighted += part.hasZ ? 1u : 0u;
            }
        }
        const char* geometry = !points                             ? "AS_WKT"
                               : heighted == 0                     ? "AS_XY"
                               : heighted == total                 ? "AS_XYZ"
                                                                   : "AS_WKT";
        options.emplace_back("GEOMETRY", geometry);
        // The .csvt beside it keeps each field's type, and says which columns
        // are X and Y; without it every field reads back as text.
        options.emplace_back("CREATE_CSVT", "YES");
    } else if (driver == "MapInfo File") {
        const Extent extent = extentOf(table);
        if (extent.valid()) {
            options.emplace_back("BOUNDS", mapInfoBounds(extent));
        }
    }
    return options;
}

std::string layerNameFor(const gp::FeatureSet& set, std::size_t index,
                         const VectorExportOptions& options)
{
    const gp::FeatureTable& table = set.tables[index];
    if (set.tables.size() == 1 && !options.layerName.empty()) {
        return options.layerName;
    }
    return table.name.empty() ? "features" + std::to_string(index + 1) : table.name;
}

bool isLonLat(const OGRSpatialReference& reference)
{
    OGRSpatialReference wgs84;
    wgs84.SetWellKnownGeogCS("WGS84");
    wgs84.SetAxisMappingStrategy(OAMS_TRADITIONAL_GIS_ORDER);
    return reference.IsGeographic() != 0 && reference.IsSame(&wgs84) != 0;
}

// Every coordinate of `table` from `crs` to longitude and latitude on WGS 84,
// by the one reprojection (gis::reprojectFeatures), heights untouched.
Status toLonLat(gp::FeatureTable& table, const std::string& crs)
{
    std::vector<VectorFeature> geometries;
    for (const gp::Feature& feature : table.features) {
        for (const VectorGeometry& part : feature.parts) {
            geometries.push_back(VectorFeature{part, {}});
        }
    }
    auto moved = reprojectFeatures(std::move(geometries), crs, "EPSG:4326");
    if (!moved) {
        return moved.error();
    }
    std::size_t next = 0;
    for (gp::Feature& feature : table.features) {
        for (VectorGeometry& part : feature.parts) {
            part = std::move((*moved)[next++].geometry);
        }
    }
    return {};
}

} // namespace

Result<VectorWriteReport> GdalDataset::writeTables(const std::filesystem::path& path,
                                                   const gp::FeatureSet& set,
                                                   const VectorExportOptions& options)
{
    ensureRegistered();
    CPLErrorReset(); // see writeRaster

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
    VectorWriteReport report;
    const detail::CplErrorCollector errors;

    // Everything that can refuse does so before a file exists.
    //
    // A coordinate system that cannot be read is refused. It used to be
    // dropped: the file was written with no CRS at all and the export
    // reported success, which is the silent failure PLAN.MD section 36
    // forbids - the layer then reads back as "no coordinate system",
    // indistinguishable from data that never had one.
    gp::FeatureSet converted;
    const gp::FeatureSet* tables = &set;
    std::vector<std::unique_ptr<OGRSpatialReference>> references(set.tables.size());
    for (std::size_t t = 0; t < set.tables.size(); ++t) {
        const std::string& crs =
            set.tables[t].crsWkt.empty() ? options.projectionWkt : set.tables[t].crsWkt;
        if (crs.empty()) {
            continue;
        }
        references[t] = std::make_unique<OGRSpatialReference>();
        if (!parseCrs(crs, *references[t])) {
            return makeError(ErrorCode::InvalidCRS,
                             "GDAL could not read the coordinate system to write", lastGdalError());
        }
    }
    if (driverHoldsOnlyLonLat(driverName)) {
        // Said in the format's name: LIBKML, which writes a .kml and a .kmz
        // (formats.hpp), is the name of GDAL's library, not of what a person
        // asked for.
        const std::string format = driverName == "LIBKML" ? std::string("KML") : driverName;
        converted = set;
        tables = &converted;
        for (std::size_t t = 0; t < converted.tables.size(); ++t) {
            gp::FeatureTable& table = converted.tables[t];
            if (references[t] == nullptr) {
                return makeError(
                    ErrorCode::InvalidCRS,
                    format + " holds longitude and latitude on WGS 84 and nothing else, and "
                             "these coordinates are in no coordinate system Katana was told "
                             "of, so they cannot be converted: set the project's coordinate "
                             "system (CRS SET <code>) and export again",
                    path.string());
            }
            if (!isLonLat(*references[t])) {
                const std::string& crs = table.crsWkt.empty() ? options.projectionWkt : table.crsWkt;
                if (auto moved = toLonLat(table, crs); !moved) {
                    return moved.error();
                }
                report.warnings.push_back("coordinates were converted from " + describeCrs(crs) +
                                          " to longitude and latitude on WGS 84 (EPSG:4326), the "
                                          "only coordinates " + format + " holds");
            }
            references[t] = std::make_unique<OGRSpatialReference>();
            references[t]->SetWellKnownGeogCS("WGS84");
            references[t]->SetAxisMappingStrategy(OAMS_TRADITIONAL_GIS_ORDER);
            table.crsWkt.clear();
        }
    }
    if (driverHoldsNoAreas(driverName)) {
        for (const gp::FeatureTable& table : tables->tables) {
            if (table.kind != GeometryKind::Point && table.kind != GeometryKind::LineString &&
                !table.features.empty()) {
                return makeError(ErrorCode::Unsupported,
                                 driverName + " holds points and lines, a layer of each; this "
                                              "table holds areas or a mix: write areas as their "
                                              "closed lines",
                                 table.name);
            }
        }
    }

    // GDAL's DXF writer turns every polygon into a HATCH with a SOLID fill
    // unless told otherwise, so a closed polyline or a circle - which the
    // exporter hands over as a polygon, the right thing for a GeoPackage or a
    // shapefile - opened in a CAD program as a filled solid: every parcel of
    // a drawing handed to a client was a black shape. With the hatch off, the
    // same driver writes each ring as an LWPOLYLINE with its closed flag set.
    std::optional<ScopedThreadConfig> outlineNotHatch;
    if (driverName == "DXF") {
        outlineNotHatch.emplace("DXF_WRITE_HATCH", "NO");
    }

    const bool anyZ = std::ranges::any_of(tables->tables, [](const gp::FeatureTable& table) {
        return table.hasZ;
    });
    std::vector<std::pair<std::string, std::string>> katanaDataset;
    if (driverName == "GPX") {
        // GPX's own schema has no place for a field of ours; its extensions
        // element does, and GDAL reads it back.
        katanaDataset.emplace_back("GPX_USE_EXTENSIONS", "YES");
    } else if (driverName == "KML" && anyZ) {
        // KML's default, clampToGround, puts every coordinate on the ground
        // whatever its altitude says: a height is only a height in
        // "absolute".
        katanaDataset.emplace_back("AltitudeMode", "absolute");
    }
    const CPLStringList datasetOptions = mergedOptions(katanaDataset, options.creationOptions);

    std::error_code existsError;
    const bool appending = options.append && std::filesystem::exists(path, existsError);
    GDALDataset* dataset = nullptr;
    // The layers this call made, so a failed append takes away only those.
    std::vector<std::string> made;
    if (appending) {
        // The file as it is, opened to be added to: its own layers stay.
        dataset = static_cast<GDALDataset*>(GDALOpenEx(path.string().c_str(),
                                                       GDAL_OF_VECTOR | GDAL_OF_UPDATE, nullptr,
                                                       nullptr, nullptr));
        if (dataset == nullptr) {
            return makeError(ErrorCode::FileExportFailure,
                             "GDAL could not open '" + path.string() + "' to add to it",
                             lastGdalError());
        }
        if (dataset->TestCapability(ODsCCreateLayer) == 0) {
            GDALClose(dataset);
            return makeError(ErrorCode::Unsupported,
                             driverName + " adds no layer to a file that exists: write a new "
                                          "file, or a GeoPackage",
                             path.string());
        }
        for (std::size_t t = 0; t < tables->tables.size(); ++t) {
            const std::string name = layerNameFor(*tables, t, options);
            if (dataset->GetLayerByName(name.c_str()) != nullptr) {
                GDALClose(dataset);
                return makeError(ErrorCode::InvalidArgument,
                                 "the file has a layer '" + name +
                                     "' already: name the new one with layername=",
                                 path.string());
            }
        }
    } else {
        // Most drivers refuse to overwrite. Remove an existing file first so
        // that re-exporting to the same name behaves the way a user expects a
        // Save As to.
        std::error_code removeError;
        std::filesystem::remove(path, removeError);
        dataset = driver->Create(path.string().c_str(), 0, 0, 0, GDT_Unknown,
                                 datasetOptions.List());
    }
    if (dataset == nullptr) {
        return makeError(ErrorCode::FileExportFailure,
                         "GDAL could not create '" + path.string() + "'", lastGdalError());
    }

    // A half-written export is worse than none: a Shapefile is three or more
    // files, and leaving a truncated set behind invites someone to open it and
    // believe it. Every failure below unwinds through this, which closes the
    // dataset and asks the DRIVER to delete it - the driver knows about the
    // sidecar files, std::filesystem::remove would only take the .shp. An
    // append deletes the layers it made and leaves the file's own.
    const auto abandon = [&driver, &path, appending, &made](GDALDataset* handle, Error error) {
        if (appending) {
            for (const std::string& name : made) {
                for (int i = handle->GetLayerCount() - 1; i >= 0; --i) {
                    OGRLayer* layer = handle->GetLayer(i);
                    if (layer != nullptr && name == layer->GetName()) {
                        (void)handle->DeleteLayer(i);
                        break;
                    }
                }
            }
            GDALClose(handle);
            return error;
        }
        GDALClose(handle);
        discardOutput(*driver, path);
        return error;
    };

    // Every layer and its fields first, then every feature in one
    // transaction: a GeoPackage creates its tables outside it.
    struct Written {
        OGRLayer* layer = nullptr;
        std::vector<int> fieldIndex; // per table field; -1 when the layer has none for it
        int altitudeIndex = -1;
        int styleField = -1; // the table's OGR_STYLE field, when it has one
    };
    std::vector<Written> written(tables->tables.size());
    for (std::size_t t = 0; t < tables->tables.size(); ++t) {
        const gp::FeatureTable& table = tables->tables[t];
        const std::string name = layerNameFor(*tables, t, options);
        const CPLStringList layerOptions =
            mergedOptions(katanaLayerOptions(driverName, table), options.layerCreationOptions);
        OGRLayer* layer = dataset->CreateLayer(name.c_str(), references[t].get(),
                                               layerTypeOf(table),
                                               layerOptions.List());
        if (layer == nullptr) {
            return abandon(dataset, makeError(ErrorCode::FileExportFailure,
                                              "GDAL could not create the vector layer",
                                              lastGdalError()));
        }
        made.push_back(layer->GetName());
        Written& out = written[t];
        out.layer = layer;
        for (std::size_t f = 0; f < table.fields.size(); ++f) {
            if (table.fields[f].name == "OGR_STYLE") {
                out.styleField = static_cast<int>(f);
            }
        }
        // Some formats have a FIXED set of fields and refuse any other: a DXF
        // layer has Layer, Linetype, Text and a few more, and CreateField fails
        // on everything else. That used to abort the whole export - so DXF, the
        // one format a CAD program cannot do without, could not be written at
        // all. Such a layer is written with the fields it has; a value is kept
        // when the format already has a field of that name (OGR matches
        // names case-insensitively, so ours `layer` lands in DXF's `Layer`) and
        // dropped otherwise, and the report names what was dropped.
        const bool canCreateFields = layer->TestCapability(OLCCreateField) != 0;
        for (std::size_t f = 0; f < table.fields.size(); ++f) {
            const gp::FieldDef& field = table.fields[f];
            if (!canCreateFields) {
                const int index = layer->GetLayerDefn()->GetFieldIndex(field.name.c_str());
                out.fieldIndex.push_back(index);
                if (index < 0 && std::ranges::find(report.fieldsNotWritten, field.name) ==
                                     report.fieldsNotWritten.end()) {
                    report.fieldsNotWritten.push_back(field.name);
                }
                continue;
            }
            const OgrField type = ogrFieldFor(field, table, f, *driver);
            OGRFieldDefn definition(field.name.c_str(), type.type);
            definition.SetSubType(type.subType);
            const int before = layer->GetLayerDefn()->GetFieldCount();
            if (layer->CreateField(&definition) != OGRERR_NONE) {
                return abandon(dataset, makeError(ErrorCode::FileExportFailure,
                                                  "GDAL could not create field '" + field.name + "'",
                                                  lastGdalError()));
            }
            // By position, not by name: a shapefile shortens a long name
            // (and says so, a warning in the report).
            out.fieldIndex.push_back(layer->GetLayerDefn()->GetFieldCount() > before ? before
                                                                                     : -1);
        }
        if (driverName == "LIBKML" && table.hasZ) {
            // KMZ's writer takes a feature's altitude mode from a field of
            // this name, and gives a 2D coordinate an altitude of 0 that its
            // default mode, clampToGround, keeps on the ground.
            out.altitudeIndex = layer->GetLayerDefn()->GetFieldIndex("altitudeMode");
            if (out.altitudeIndex < 0) {
                OGRFieldDefn altitude("altitudeMode", OFTString);
                if (layer->CreateField(&altitude) == OGRERR_NONE) {
                    out.altitudeIndex = layer->GetLayerDefn()->GetFieldIndex("altitudeMode");
                }
            }
        }
    }

    // Every feature in ONE transaction, where the format has them. A
    // GeoPackage is SQLite, and SQLite commits - and so syncs the file to disk
    // - once per transaction; given none, every feature was a transaction of
    // its own, and the owner's 27,886-entity corridor drawing took 67 s to
    // export where ogr2ogr, batching, took 3.2 s for the same features. Not
    // forced: a driver without real transactions (a shapefile) would emulate
    // one by copying the whole file, which is the opposite of the point.
    bool inTransaction = false;
    if (dataset->TestCapability(ODsCTransactions) != 0) {
        if (dataset->StartTransaction(FALSE) != OGRERR_NONE) {
            return abandon(dataset,
                           makeError(ErrorCode::FileExportFailure,
                                     "GDAL could not start a transaction", lastGdalError()));
        }
        inTransaction = true;
    }
    // A failure part-way leaves nothing: the transaction is rolled back and
    // then the file is deleted, as a failure outside one always was.
    const auto abandonFeatures = [&](Error error) {
        if (inTransaction) {
            (void)dataset->RollbackTransaction();
        }
        return abandon(dataset, std::move(error));
    };

    for (std::size_t t = 0; t < tables->tables.size(); ++t) {
        const gp::FeatureTable& table = tables->tables[t];
        const Written& out = written[t];
        for (const gp::Feature& source : table.features) {
            OGRGeometry* geometry = featureGeometry(source, table.kind);
            if (geometry == nullptr) {
                ++report.featuresSkipped;
                continue;
            }
            OGRFeature* feature = OGRFeature::CreateFeature(out.layer->GetLayerDefn());
            feature->SetGeometryDirectly(geometry);
            for (std::size_t f = 0; f < source.values.size() && f < out.fieldIndex.size(); ++f) {
                const int index = out.fieldIndex[f];
                if (index < 0) {
                    continue;
                }
                std::visit(
                    [&](const auto& value) {
                        using Held = std::decay_t<decltype(value)>;
                        if constexpr (std::is_same_v<Held, std::monostate>) {
                            feature->SetFieldNull(index);
                        } else if constexpr (std::is_same_v<Held, bool>) {
                            feature->SetField(index, value ? 1 : 0);
                        } else if constexpr (std::is_same_v<Held, std::int64_t>) {
                            feature->SetField(index, static_cast<GIntBig>(value));
                        } else if constexpr (std::is_same_v<Held, double>) {
                            feature->SetField(index, value);
                        } else {
                            feature->SetField(index, value.c_str());
                        }
                    },
                    source.values[f]);
            }
            if (out.altitudeIndex >= 0 && geometry->Is3D() != 0) {
                feature->SetField(out.altitudeIndex, "absolute");
            }
            if (out.styleField >= 0 &&
                static_cast<std::size_t>(out.styleField) < source.values.size()) {
                if (const auto* style =
                        std::get_if<std::string>(&source.values[static_cast<std::size_t>(
                            out.styleField)])) {
                    feature->SetStyleString(style->c_str());
                }
            }
            const std::size_t before = errors.size();
            const OGRErr status = out.layer->CreateFeature(feature);
            OGRFeature::DestroyFeature(feature);
            // A writer that cannot write a geometry may say so and succeed
            // anyway: KML writes the placemark without it. What it says is the
            // failure of the write.
            const detail::CplErrorCollector::Message* failure = errors.firstFailure(before);
            if (status != OGRERR_NONE || failure != nullptr) {
                return abandonFeatures(makeError(ErrorCode::FileExportFailure,
                                                 "GDAL could not write a feature",
                                                 failure != nullptr ? failure->text
                                                                    : lastGdalError()));
            }
            ++report.featuresWritten;
        }
    }
    // Where the features reach the file: a full disk shows here.
    if (inTransaction && dataset->CommitTransaction() != OGRERR_NONE) {
        inTransaction = false; // a failed commit has already ended it
        return abandonFeatures(makeError(
            ErrorCode::FileExportFailure,
            "GDAL could not commit the features to '" + path.string() + "'", lastGdalError()));
    }

    // A shapefile flushes its .dbf here, and a GeoPackage writes its last
    // metadata (the extent, the spatial index), so a full disk can show here
    // as well as at the commit above (audit IO-14). GDAL reports it
    // through the return value since 3.7; it used to be ignored and the
    // export reported success over a truncated file.
    if (GDALClose(dataset) != CE_None) {
        const std::string message = lastGdalError();
        if (!appending) {
            discardOutput(*driver, path);
        }
        return makeError(ErrorCode::FileExportFailure,
                         "GDAL could not finish writing '" + path.string() + "'", message);
    }
    for (std::string& warning : errors.warnings()) {
        report.warnings.push_back(std::move(warning));
    }
    return report;
}

Status GdalDataset::writeVector(const std::filesystem::path& path,
                                const std::vector<VectorFeature>& features,
                                const VectorExportOptions& options)
{
    // One table of text fields, the attribute names in the order the
    // features first give them, so the field layout is deterministic across
    // runs (Rule 7).
    gp::FeatureTable table;
    table.name = options.layerName;
    if (!features.empty()) {
        const GeometryKind first = features.front().geometry.kind;
        const bool uniform = std::ranges::all_of(features, [first](const VectorFeature& feature) {
            return feature.geometry.kind == first;
        });
        table.kind = uniform ? first : GeometryKind::Unknown;
    }
    for (const VectorFeature& feature : features) {
        table.hasZ = table.hasZ || feature.geometry.hasZ;
        for (const auto& [name, value] : feature.attributes) {
            if (std::ranges::none_of(table.fields,
                                     [&](const gp::FieldDef& field) { return field.name == name; })) {
                table.fields.push_back(gp::FieldDef{name, gp::FieldType::String});
            }
        }
    }
    for (const VectorFeature& source : features) {
        gp::Feature feature;
        feature.parts.push_back(source.geometry);
        for (const gp::FieldDef& field : table.fields) {
            const auto found = source.attributes.find(field.name);
            feature.values.push_back(found == source.attributes.end()
                                         ? gp::FieldValue(std::monostate{})
                                         : gp::FieldValue(found->second));
        }
        table.features.push_back(std::move(feature));
    }
    gp::FeatureSet set;
    set.tables.push_back(std::move(table));
    auto written = writeTables(path, set, options);
    if (!written) {
        return written.error();
    }
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

bool driverHoldsOnlyLonLat(const std::string& driver)
{
    return driver == "KML" || driver == "LIBKML" || driver == "GPX";
}

bool driverHoldsNoAreas(const std::string& driver)
{
    return driver == "GPX";
}

std::string describeCrs(const std::string& wkt)
{
    if (wkt.empty()) {
        return {};
    }
    ensureRegistered();
    OGRSpatialReference reference;
    if (!parseCrs(wkt, reference)) {
        // Never "": that would read as "no coordinate system", and a file that
        // declares one we cannot read is a different problem for its owner.
        return "unrecognised coordinate system";
    }
    const char* name = reference.GetName();
    std::string text = name != nullptr && *name != '\0' ? name : "unnamed coordinate system";

    const auto authorityCode = [](const OGRSpatialReference& crs) -> std::string {
        const char* authority = crs.GetAuthorityName(nullptr);
        const char* code = crs.GetAuthorityCode(nullptr);
        if (authority == nullptr || code == nullptr || *authority == '\0' || *code == '\0') {
            return {};
        }
        return std::string(authority) + ":" + code;
    };
    std::string identified = authorityCode(reference);

    // A WKT written without AUTHORITY nodes - every Esri .prj beside a
    // shapefile - is usually still an EPSG system, recognisable by its
    // parameters. When nothing can be recognised the name alone is the honest
    // answer, so a failure below is not an error.
    CPLPushErrorHandler(CPLQuietErrorHandler);
    if (identified.empty()) {
        (void)reference.AutoIdentifyEPSG();
        identified = authorityCode(reference);
    }
    if (identified.empty()) {
        // AutoIdentifyEPSG knows a handful of datums by name, and in GDAL
        // 3.13.2 identified none of WGS 84, WGS 84 / UTM 56S or an Esri .prj of
        // GDA94 / MGA 56 (measured 2026-09-23; it returned
        // OGRERR_UNSUPPORTED_SRS for each). FindMatches asks PROJ's database,
        // which named all three at confidence 100 - it is what
        // `gdalsrsinfo -e` reports. PROJ's scale (proj_identify): 100 and 90
        // are the same CRS with matching or similar names, 70 the same CRS by
        // another name, below that only a similarity. A code is shown only
        // for one candidate that is the same CRS; two at the top would make
        // the choice a guess.
        int entries = 0;
        int* confidence = nullptr;
        OGRSpatialReferenceH* matches = reference.FindMatches(nullptr, &entries, &confidence);
        constexpr int kSameCrs = 70;
        if (matches != nullptr && confidence != nullptr && entries >= 1 &&
            confidence[0] >= kSameCrs && (entries == 1 || confidence[1] < confidence[0])) {
            identified = authorityCode(*OGRSpatialReference::FromHandle(matches[0]));
        }
        OSRFreeSRSArray(matches);
        CPLFree(confidence);
    }
    CPLPopErrorHandler();

    if (!identified.empty()) {
        text += " (" + identified + ")";
    }
    return text;
}

std::string gdalVersion()
{
    ensureRegistered();
    const char* version = GDALVersionInfo("RELEASE_NAME");
    return version != nullptr ? version : "unknown";
}

} // namespace katana::gis
