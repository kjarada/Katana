#include "katana/geodesy/coordinate_reference_system.hpp"

#include <array>
#include <charconv>
#include <memory>
#include <system_error>
#include <utility>

#include "katana/math/numerics.hpp"
#include "proj_internal.hpp"

namespace katana::geodesy {

namespace {

using detail::PjPtr;
using detail::ProjContext;
using detail::toStdString;

std::string_view trimmed(std::string_view text)
{
    constexpr std::string_view kWhitespace = " \t\r\n";
    const std::size_t first = text.find_first_not_of(kWhitespace);
    if (first == std::string_view::npos) {
        return {};
    }
    const std::size_t last = text.find_last_not_of(kWhitespace);
    return text.substr(first, last - first + 1);
}

bool looksLikeProjString(std::string_view text)
{
    return text.starts_with('+') || text.starts_with("proj=") || text.starts_with("init=");
}

// A PROJ string without "+type=crs" denotes a coordinate OPERATION (radians in,
// metres out), not a CRS. Katana only deals in CRSs here.
std::string asCrsProjString(std::string_view text)
{
    std::string definition(text);
    if (definition.find("type=crs") == std::string::npos) {
        definition += " +type=crs";
    }
    return definition;
}

// A BoundCRS (a CRS carrying a +towgs84 / +nadgrids hint) is classified by the
// CRS it wraps.
PjPtr unwrapBound(ProjContext& context, const PJ* crs)
{
    if (proj_get_type(crs) == PJ_TYPE_BOUND_CRS) {
        return PjPtr(proj_get_source_crs(context.get(), crs));
    }
    return nullptr;
}

CrsKind kindOf(PJ_TYPE type)
{
    switch (type) {
    case PJ_TYPE_GEOGRAPHIC_2D_CRS:
        return CrsKind::Geographic2D;
    case PJ_TYPE_GEOGRAPHIC_3D_CRS:
        return CrsKind::Geographic3D;
    case PJ_TYPE_GEOCENTRIC_CRS:
        return CrsKind::Geocentric;
    case PJ_TYPE_PROJECTED_CRS:
    case PJ_TYPE_DERIVED_PROJECTED_CRS:
        return CrsKind::Projected;
    case PJ_TYPE_VERTICAL_CRS:
        return CrsKind::Vertical;
    case PJ_TYPE_COMPOUND_CRS:
        return CrsKind::Compound;
    case PJ_TYPE_ENGINEERING_CRS:
        return CrsKind::LocalEngineering;
    default:
        return CrsKind::Other;
    }
}

void appendAxes(ProjContext& context, const PJ* crs, std::vector<AxisInfo>& axes)
{
    const PjPtr unwrapped = unwrapBound(context, crs);
    const PJ* base = unwrapped ? unwrapped.get() : crs;

    if (proj_get_type(base) == PJ_TYPE_COMPOUND_CRS) {
        for (int index = 0;; ++index) {
            const PjPtr component(proj_crs_get_sub_crs(context.get(), base, index));
            if (!component) {
                break;
            }
            appendAxes(context, component.get(), axes);
        }
        return;
    }

    const PjPtr system(proj_crs_get_coordinate_system(context.get(), base));
    if (!system) {
        return;
    }
    const int count = proj_cs_get_axis_count(context.get(), system.get());
    for (int index = 0; index < count; ++index) {
        const char* name = nullptr;
        const char* abbreviation = nullptr;
        const char* direction = nullptr;
        const char* unitName = nullptr;
        double unitToSi = 1.0;
        if (proj_cs_get_axis_info(context.get(), system.get(), index, &name, &abbreviation,
                                  &direction, &unitToSi, &unitName, nullptr, nullptr) != 0) {
            axes.push_back(AxisInfo{toStdString(name), toStdString(abbreviation),
                                    toStdString(direction), toStdString(unitName), unitToSi});
        }
    }
}

PJ_WKT_TYPE projWktType(WktVersion version)
{
    switch (version) {
    case WktVersion::Wkt2_2019:
        return PJ_WKT2_2019;
    case WktVersion::Wkt1Gdal:
        return PJ_WKT1_GDAL;
    case WktVersion::Wkt1Esri:
        return PJ_WKT1_ESRI;
    }
    return PJ_WKT2_2019;
}

// Single-line output: the canonical form is stored and compared, not read.
core::Result<std::string> exportWkt(ProjContext& context, const PJ* crs, WktVersion version)
{
    constexpr std::array<const char*, 2> kOptions = {"MULTILINE=NO", nullptr};
    context.clearDiagnostics();
    const char* wkt = proj_as_wkt(context.get(), crs, projWktType(version), kOptions.data());
    if (wkt == nullptr) {
        return core::makeError(core::ErrorCode::Unsupported,
                               "PROJ cannot express this CRS in the requested WKT flavour",
                               context.consumeDiagnostics());
    }
    return std::string(wkt);
}

// Shortest decimal text that parses back to exactly `value`.
std::string exactDecimal(double value)
{
    std::array<char, 32> buffer{};
    const std::to_chars_result result =
        std::to_chars(buffer.data(), buffer.data() + buffer.size(), value);
    return result.ec == std::errc{} ? std::string(buffer.data(), result.ptr) : std::string{};
}

std::string escapeWktQuotes(std::string_view text)
{
    std::string escaped;
    escaped.reserve(text.size());
    for (const char ch : text) {
        escaped.push_back(ch);
        if (ch == '"') {
            escaped.push_back('"'); // WKT doubles an embedded quote
        }
    }
    return escaped;
}

} // namespace

// ---- Builder -----------------------------------------------------------------

struct CoordinateReferenceSystem::Builder {
    static core::Result<CoordinateReferenceSystem> fromDefinition(std::string definition)
    {
        auto context = ProjContext::create();
        if (!context) {
            return context.error();
        }
        auto object = detail::createCrs(**context, definition);
        if (!object) {
            return object.error();
        }
        return fromObject(**context, object->get(), std::move(definition));
    }

    static core::Result<CoordinateReferenceSystem> fromObject(ProjContext& context, const PJ* crs,
                                                              std::string definition)
    {
        CoordinateReferenceSystem result;
        result.definition_ = std::move(definition);
        result.name_ = toStdString(proj_get_name(crs));

        const PjPtr unwrapped = unwrapBound(context, crs);
        const PJ* base = unwrapped ? unwrapped.get() : crs;
        result.kind_ = kindOf(proj_get_type(base));
        classifyHorizontal(context, base, result);

        // A BoundCRS has no identifier of its own; the CRS it wraps may.
        for (const PJ* candidate : {crs, base}) {
            if (const char* authority = proj_get_id_auth_name(candidate, 0)) {
                result.authority_ = authority;
                result.code_ = toStdString(proj_get_id_code(candidate, 0));
                break;
            }
        }

        appendAxes(context, base, result.axes_);
        if (!result.axes_.empty()) {
            result.horizontalUnitName_ = result.axes_.front().unitName;
            result.horizontalUnitToSi_ = result.axes_.front().unitToSi;
            if (!result.horizontalIsGeographic_) {
                result.lengthUnit_ = lengthUnitFromMetresPerUnit(result.horizontalUnitToSi_);
            }
        }

        readAreaOfUse(context, crs, result);
        if (result.kind_ != CrsKind::LocalEngineering && result.kind_ != CrsKind::Vertical &&
            result.kind_ != CrsKind::Other) {
            readEllipsoid(context, base, result);
        }

        auto wkt = exportWkt(context, crs, WktVersion::Wkt2_2019);
        if (wkt) {
            result.wkt_ = std::move(*wkt);
        } else {
            result.wktError_ = wkt.error().context;
        }
        context.clearDiagnostics();
        return result;
    }

    static void classifyHorizontal(ProjContext& context, const PJ* base,
                                   CoordinateReferenceSystem& result)
    {
        CrsKind horizontal = result.kind_;
        if (result.kind_ == CrsKind::Compound) {
            const PjPtr component(proj_crs_get_sub_crs(context.get(), base, 0));
            if (component) {
                const PjPtr unwrapped = unwrapBound(context, component.get());
                horizontal = kindOf(proj_get_type(unwrapped ? unwrapped.get() : component.get()));
            }
        }
        result.horizontalIsGeographic_ =
            horizontal == CrsKind::Geographic2D || horizontal == CrsKind::Geographic3D;
        result.horizontalIsProjected_ = horizontal == CrsKind::Projected;
    }

    static void readAreaOfUse(ProjContext& context, const PJ* crs,
                              CoordinateReferenceSystem& result)
    {
        // PROJ reports -1000 for every bound of an unknown area.
        constexpr double kUnknownBound = -1000.0;
        GeographicExtent extent;
        if (proj_get_area_of_use(context.get(), crs, &extent.west, &extent.south, &extent.east,
                                 &extent.north, nullptr) != 0 &&
            extent.west != kUnknownBound && extent.south != kUnknownBound &&
            extent.east != kUnknownBound && extent.north != kUnknownBound) {
            result.areaOfUse_ = extent;
        }
    }

    static void readEllipsoid(ProjContext& context, const PJ* base,
                              CoordinateReferenceSystem& result)
    {
        const PjPtr ellipsoid(proj_get_ellipsoid(context.get(), base));
        if (!ellipsoid) {
            return;
        }
        double semiMajor = 0.0;
        double inverseFlattening = 0.0;
        if (proj_ellipsoid_get_parameters(context.get(), ellipsoid.get(), &semiMajor, nullptr,
                                          nullptr, &inverseFlattening) != 0) {
            result.ellipsoid_ = Ellipsoid{semiMajor, inverseFlattening};
        }
    }
};

// ---- Factories ---------------------------------------------------------------

core::Result<CoordinateReferenceSystem> CoordinateReferenceSystem::fromEpsg(int code)
{
    if (code <= 0) {
        return core::makeError(core::ErrorCode::InvalidCRS, "EPSG codes are positive integers",
                               "code=" + std::to_string(code));
    }
    return Builder::fromDefinition("EPSG:" + std::to_string(code));
}

core::Result<CoordinateReferenceSystem> CoordinateReferenceSystem::fromWkt(std::string_view wkt)
{
    const std::string_view text = trimmed(wkt);
    // Every WKT CRS is KEYWORD[...]; this keeps "EPSG:4326" and PROJ strings,
    // which PROJ would happily accept, out of the WKT entry point.
    if (text.find_first_of("[(") == std::string_view::npos || looksLikeProjString(text)) {
        return core::makeError(core::ErrorCode::InvalidCRS, "the text is not WKT",
                               "wkt='" + detail::abbreviate(std::string(text)) + "'");
    }
    return Builder::fromDefinition(std::string(text));
}

core::Result<CoordinateReferenceSystem>
CoordinateReferenceSystem::fromProjString(std::string_view projString)
{
    const std::string_view text = trimmed(projString);
    if (!looksLikeProjString(text)) {
        return core::makeError(core::ErrorCode::InvalidCRS,
                               "the text is not a PROJ string (expected '+proj=...')",
                               "projString='" + detail::abbreviate(std::string(text)) + "'");
    }
    return Builder::fromDefinition(asCrsProjString(text));
}

core::Result<CoordinateReferenceSystem>
CoordinateReferenceSystem::fromUserInput(std::string_view input)
{
    const std::string_view text = trimmed(input);
    if (text.empty()) {
        return core::makeError(core::ErrorCode::InvalidCRS, "the CRS definition is empty");
    }
    if (looksLikeProjString(text)) {
        return Builder::fromDefinition(asCrsProjString(text));
    }
    return Builder::fromDefinition(std::string(text));
}

core::Result<CoordinateReferenceSystem>
CoordinateReferenceSystem::localEngineering(std::string_view name, LengthUnit unit)
{
    const std::string_view text = trimmed(name);
    if (text.empty()) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "a local engineering CRS needs a name");
    }

    const std::string unitName(toString(unit));
    const double unitToSi = metresPerUnit(unit);
    const std::string quotedName = escapeWktQuotes(text);
    const std::string lengthUnit =
        "LENGTHUNIT[\"" + unitName + "\"," + exactDecimal(unitToSi) + "]";

    CoordinateReferenceSystem result;
    // Written in the layout PROJ itself exports for an ENGCRS, so that the text
    // survives a round trip through fromWkt().
    result.wkt_ = "ENGCRS[\"" + quotedName + "\",EDATUM[\"" + quotedName +
                  " datum\"],CS[Cartesian,2],AXIS[\"(E)\",east,ORDER[1]," + lengthUnit +
                  "],AXIS[\"(N)\",north,ORDER[2]," + lengthUnit + "]]";
    result.definition_ = result.wkt_;
    result.name_ = std::string(text);
    result.kind_ = CrsKind::LocalEngineering;
    result.horizontalUnitName_ = unitName;
    result.horizontalUnitToSi_ = unitToSi;
    result.lengthUnit_ = unit;
    result.axes_ = {AxisInfo{"Easting", "E", "east", unitName, unitToSi},
                    AxisInfo{"Northing", "N", "north", unitName, unitToSi}};
    return result;
}

// ---- Queries -----------------------------------------------------------------

std::optional<int> CoordinateReferenceSystem::epsgCode() const
{
    if (authority_ != "EPSG" || code_.empty()) {
        return std::nullopt;
    }
    int value = 0;
    const char* const end = code_.data() + code_.size();
    const std::from_chars_result parsed = std::from_chars(code_.data(), end, value);
    if (parsed.ec != std::errc{} || parsed.ptr != end) {
        return std::nullopt;
    }
    return value;
}

core::Result<std::string> CoordinateReferenceSystem::toWkt(WktVersion version) const
{
    if (version == WktVersion::Wkt2_2019) {
        if (wkt_.empty()) {
            return core::makeError(core::ErrorCode::Unsupported,
                                   "PROJ could not export this CRS as WKT2:2019", wktError_);
        }
        return wkt_;
    }
    auto context = ProjContext::create();
    if (!context) {
        return context.error();
    }
    auto object = detail::createCrs(**context, definition_);
    if (!object) {
        return object.error();
    }
    return exportWkt(**context, object->get(), version);
}

core::Result<bool> CoordinateReferenceSystem::isEquivalentTo(const CoordinateReferenceSystem& other,
                                                             bool ignoreAxisOrder) const
{
    if (*this == other) {
        return true;
    }
    auto context = ProjContext::create();
    if (!context) {
        return context.error();
    }
    auto mine = detail::createCrs(**context, definition_);
    if (!mine) {
        return mine.error();
    }
    auto theirs = detail::createCrs(**context, other.definition_);
    if (!theirs) {
        return theirs.error();
    }
    const PJ_COMPARISON_CRITERION criterion =
        ignoreAxisOrder ? PJ_COMP_EQUIVALENT_EXCEPT_AXIS_ORDER_GEOGCRS : PJ_COMP_EQUIVALENT;
    return proj_is_equivalent_to_with_ctx((*context)->get(), mine->get(), theirs->get(),
                                          criterion) != 0;
}

bool operator==(const CoordinateReferenceSystem& a, const CoordinateReferenceSystem& b)
{
    return a.definition_ == b.definition_ || (!a.wkt_.empty() && a.wkt_ == b.wkt_);
}

std::string_view toString(CrsKind kind)
{
    switch (kind) {
    case CrsKind::Geographic2D:
        return "Geographic2D";
    case CrsKind::Geographic3D:
        return "Geographic3D";
    case CrsKind::Geocentric:
        return "Geocentric";
    case CrsKind::Projected:
        return "Projected";
    case CrsKind::Vertical:
        return "Vertical";
    case CrsKind::Compound:
        return "Compound";
    case CrsKind::LocalEngineering:
        return "LocalEngineering";
    case CrsKind::Other:
        return "Other";
    }
    return "Unknown";
}

core::Result<ProjRuntimeInfo> projRuntimeInfo()
{
    auto context = ProjContext::create();
    if (!context) {
        return context.error();
    }
    const PJ_INFO info = proj_info();
    return ProjRuntimeInfo{toStdString(info.version),
                           toStdString(proj_context_get_database_path((*context)->get())),
                           toStdString(info.searchpath)};
}

} // namespace katana::geodesy
