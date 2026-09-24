#include "katana/survey/reduction_settings.hpp"

#include <array>
#include <charconv>
#include <cmath>
#include <optional>
#include <set>
#include <system_error>
#include <utility>

namespace katana::survey {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

constexpr std::string_view kHeaderKey = "katana-reduction-settings";
constexpr std::string_view kControlKey = "control";

// ---- Enum names ------------------------------------------------------------------
//
// One list of values per enum, so that parsing is "the value whose name this
// is" and a value added to an enum and not to its list fails the round-trip
// test rather than being written and then refused.

constexpr std::array kAtmosphericValues{AtmosphericCorrection::None, AtmosphericCorrection::Auto,
                                        AtmosphericCorrection::Recompute,
                                        AtmosphericCorrection::Fixed};
constexpr std::array kPrismValues{PrismConstantPolicy::Auto, PrismConstantPolicy::Override,
                                  PrismConstantPolicy::None};
constexpr std::array kFaceValues{FaceHandling::Average, FaceHandling::FaceLeftOnly,
                                 FaceHandling::Separate};
constexpr std::array kHeightValues{HeightReduction::None, HeightReduction::Ellipsoid,
                                   HeightReduction::Geoid};
constexpr std::array kGridValues{GridScale::None, GridScale::Fixed, GridScale::FromProjection};
constexpr std::array kMethodValues{AdjustmentMethod::None, AdjustmentMethod::Traverse,
                                   AdjustmentMethod::Network};
constexpr std::array kTraverseValues{TraverseRule::Bowditch, TraverseRule::Transit,
                                     TraverseRule::LeastSquares};
constexpr std::array kNetworkValues{NetworkDimension::Horizontal, NetworkDimension::Levels,
                                    NetworkDimension::HorizontalAndLevels};
constexpr std::array kOutlierValues{OutlierTest::None, OutlierTest::Baarda, OutlierTest::Tau};
constexpr std::array kOriginValues{ControlOrigin::File, ControlOrigin::Drawing};

const char* constraintName(ControlConstraint constraint)
{
    switch (constraint) {
    case ControlConstraint::Free:
        return "free";
    case ControlConstraint::Fixed:
        return "fixed";
    case ControlConstraint::Weighted:
        return "weighted";
    }
    return "free";
}

constexpr std::array kConstraintValues{ControlConstraint::Free, ControlConstraint::Fixed,
                                       ControlConstraint::Weighted};

template <typename Enum, std::size_t N, typename Namer>
std::optional<Enum> fromName(std::string_view name, const std::array<Enum, N>& values, Namer namer)
{
    for (const Enum value : values) {
        if (name == namer(value)) {
            return value;
        }
    }
    return std::nullopt;
}

template <typename Enum, std::size_t N>
std::optional<Enum> fromName(std::string_view name, const std::array<Enum, N>& values)
{
    return fromName(name, values, [](Enum value) { return toString(value); });
}

// ---- Numbers -----------------------------------------------------------------------

std::string formatDouble(double value)
{
    // Shortest text that reads back as the same double: the round trip is
    // exact, and 0.13 is written "0.13", not "0.13000000000000000444".
    std::array<char, 64> buffer{};
    const auto [end, error] = std::to_chars(buffer.data(), buffer.data() + buffer.size(), value);
    if (error != std::errc{}) {
        return "nan"; // unreachable for a 64-byte buffer; refused on the way back in
    }
    return std::string(buffer.data(), end);
}

std::optional<double> parseDouble(std::string_view text)
{
    double value = 0.0;
    const auto [end, error] = std::from_chars(text.data(), text.data() + text.size(), value);
    if (error != std::errc{} || end != text.data() + text.size() || text.empty()) {
        return std::nullopt;
    }
    return value;
}

std::optional<std::size_t> parseSize(std::string_view text)
{
    std::size_t value = 0;
    const auto [end, error] = std::from_chars(text.data(), text.data() + text.size(), value);
    if (error != std::errc{} || end != text.data() + text.size() || text.empty()) {
        return std::nullopt;
    }
    return value;
}

std::optional<bool> parseBool(std::string_view text)
{
    if (text == "true") {
        return true;
    }
    if (text == "false") {
        return false;
    }
    return std::nullopt;
}

// ---- Percent encoding for control ids ----------------------------------------------

bool needsEscape(unsigned char c)
{
    return c < 0x20 || c == 0x7F || c == '%' || c == ';' || c == '=';
}

std::string percentEncode(std::string_view text)
{
    constexpr std::string_view kHex = "0123456789ABCDEF";
    std::string out;
    out.reserve(text.size());
    for (const char ch : text) {
        const auto c = static_cast<unsigned char>(ch);
        if (needsEscape(c)) {
            out += '%';
            out += kHex[c >> 4];
            out += kHex[c & 0x0F];
        } else {
            out += ch;
        }
    }
    return out;
}

std::optional<std::string> percentDecode(std::string_view text)
{
    auto nibble = [](char c) -> int {
        if (c >= '0' && c <= '9') {
            return c - '0';
        }
        if (c >= 'A' && c <= 'F') {
            return c - 'A' + 10;
        }
        if (c >= 'a' && c <= 'f') {
            return c - 'a' + 10;
        }
        return -1;
    };
    std::string out;
    out.reserve(text.size());
    for (std::size_t i = 0; i < text.size(); ++i) {
        if (text[i] != '%') {
            out += text[i];
            continue;
        }
        if (i + 2 >= text.size()) {
            return std::nullopt; // a '%' needs two hex digits after it
        }
        const int high = nibble(text[i + 1]);
        const int low = nibble(text[i + 2]);
        if (high < 0 || low < 0) {
            return std::nullopt;
        }
        out += static_cast<char>((high << 4) | low);
        i += 2;
    }
    return out;
}

// ---- The keys ----------------------------------------------------------------------
//
// Tables rather than a hand-written writer and a hand-written reader, so that
// the two cannot disagree about a key's name: each key is spelled once.

struct DoubleKey {
    std::string_view key;
    double& (*field)(ReductionSettings&);
};

struct BoolKey {
    std::string_view key;
    bool& (*field)(ReductionSettings&);
};

struct EnumKey {
    std::string_view key;
    const char* (*get)(const ReductionSettings&);
    bool (*set)(ReductionSettings&, std::string_view);
};

template <typename Enum, std::size_t N>
bool assignEnum(Enum& target, std::string_view name, const std::array<Enum, N>& values)
{
    if (const std::optional<Enum> value = fromName(name, values)) {
        target = *value;
        return true;
    }
    return false;
}

// The order of these tables is the order of the text, which is the order of
// the struct, so a diff of two saved settings reads like the dialog.
constexpr std::array kEnumKeys{
    EnumKey{"atmospheric", [](const ReductionSettings& s) { return toString(s.atmospheric); },
            [](ReductionSettings& s, std::string_view v) {
                return assignEnum(s.atmospheric, v, kAtmosphericValues);
            }},
    EnumKey{"prism_constant.policy",
            [](const ReductionSettings& s) { return toString(s.prismConstantPolicy); },
            [](ReductionSettings& s, std::string_view v) {
                return assignEnum(s.prismConstantPolicy, v, kPrismValues);
            }},
    EnumKey{"faces", [](const ReductionSettings& s) { return toString(s.faces); },
            [](ReductionSettings& s, std::string_view v) {
                return assignEnum(s.faces, v, kFaceValues);
            }},
    EnumKey{"height_reduction",
            [](const ReductionSettings& s) { return toString(s.heightReduction); },
            [](ReductionSettings& s, std::string_view v) {
                return assignEnum(s.heightReduction, v, kHeightValues);
            }},
    EnumKey{"grid_scale", [](const ReductionSettings& s) { return toString(s.gridScale); },
            [](ReductionSettings& s, std::string_view v) {
                return assignEnum(s.gridScale, v, kGridValues);
            }},
    EnumKey{"adjustment.method", [](const ReductionSettings& s) { return toString(s.method); },
            [](ReductionSettings& s, std::string_view v) {
                return assignEnum(s.method, v, kMethodValues);
            }},
    EnumKey{"adjustment.traverse_rule",
            [](const ReductionSettings& s) { return toString(s.traverseRule); },
            [](ReductionSettings& s, std::string_view v) {
                return assignEnum(s.traverseRule, v, kTraverseValues);
            }},
    EnumKey{"adjustment.network",
            [](const ReductionSettings& s) { return toString(s.networkDimension); },
            [](ReductionSettings& s, std::string_view v) {
                return assignEnum(s.networkDimension, v, kNetworkValues);
            }},
    EnumKey{"outliers.test", [](const ReductionSettings& s) { return toString(s.outlierTest); },
            [](ReductionSettings& s, std::string_view v) {
                return assignEnum(s.outlierTest, v, kOutlierValues);
            }},
};

constexpr std::array kDoubleKeys{
    DoubleKey{"atmospheric.fixed_ppm", [](ReductionSettings& s) -> double& { return s.fixedPpm; }},
    DoubleKey{"prism_constant.m", [](ReductionSettings& s) -> double& { return s.prismConstant; }},
    DoubleKey{"faces.tolerance.horizontal_rad",
              [](ReductionSettings& s) -> double& { return s.faceTolerances.horizontal; }},
    DoubleKey{"faces.tolerance.zenith_rad",
              [](ReductionSettings& s) -> double& { return s.faceTolerances.zenith; }},
    DoubleKey{"faces.tolerance.distance_m",
              [](ReductionSettings& s) -> double& { return s.faceTolerances.distance; }},
    DoubleKey{"refraction.k",
              [](ReductionSettings& s) -> double& { return s.refractionCoefficient; }},
    DoubleKey{"earth_radius_m", [](ReductionSettings& s) -> double& { return s.earthRadius; }},
    DoubleKey{"grid_scale.fixed_factor",
              [](ReductionSettings& s) -> double& { return s.fixedGridScaleFactor; }},
    DoubleKey{"combined_factor.value",
              [](ReductionSettings& s) -> double& { return s.combinedFactor; }},
    DoubleKey{"apriori.direction_rad",
              [](ReductionSettings& s) -> double& { return s.apriori.direction; }},
    DoubleKey{"apriori.zenith_rad",
              [](ReductionSettings& s) -> double& { return s.apriori.zenith; }},
    DoubleKey{"apriori.distance_constant_m",
              [](ReductionSettings& s) -> double& { return s.apriori.distanceConstant; }},
    DoubleKey{"apriori.distance_ppm",
              [](ReductionSettings& s) -> double& { return s.apriori.distancePpm; }},
    DoubleKey{"apriori.instrument_centring_m",
              [](ReductionSettings& s) -> double& { return s.apriori.instrumentCentring; }},
    DoubleKey{"apriori.target_centring_m",
              [](ReductionSettings& s) -> double& { return s.apriori.targetCentring; }},
    DoubleKey{"apriori.height_m",
              [](ReductionSettings& s) -> double& { return s.apriori.heightMeasurement; }},
    DoubleKey{"apriori.levelling_per_sqrt_km_m",
              [](ReductionSettings& s) -> double& { return s.apriori.levellingPerSqrtKilometre; }},
    DoubleKey{"apriori.gnss_horizontal_m",
              [](ReductionSettings& s) -> double& { return s.apriori.gnssHorizontal; }},
    DoubleKey{"apriori.gnss_vertical_m",
              [](ReductionSettings& s) -> double& { return s.apriori.gnssVertical; }},
    DoubleKey{"confidence_level",
              [](ReductionSettings& s) -> double& { return s.confidenceLevel; }},
    DoubleKey{"outliers.significance",
              [](ReductionSettings& s) -> double& { return s.outlierSignificance; }},
};

constexpr std::array kBoolKeys{
    BoolKey{"curvature_refraction",
            [](ReductionSettings& s) -> bool& { return s.curvatureAndRefraction; }},
    BoolKey{"faces.tolerance.exclude",
            [](ReductionSettings& s) -> bool& { return s.faceTolerances.excludeOutside; }},
    BoolKey{"slope_to_horizontal",
            [](ReductionSettings& s) -> bool& { return s.slopeToHorizontal; }},
    BoolKey{"combined_factor.use",
            [](ReductionSettings& s) -> bool& { return s.useCombinedFactor; }},
    BoolKey{"apriori.use_file_covariances",
            [](ReductionSettings& s) -> bool& { return s.useFileCovariances; }},
    BoolKey{"outliers.auto_reject",
            [](ReductionSettings& s) -> bool& { return s.autoRejectOutliers; }},
};

constexpr std::string_view kMaxIterationsKey = "iterations.max";

std::string controlLine(const ControlSelection& selection)
{
    std::string line(kControlKey);
    line += '=';
    line += percentEncode(selection.point.pointId);
    line += ';';
    line += toString(selection.origin);
    for (const ControlComponent* component :
         {&selection.point.northing, &selection.point.easting, &selection.point.elevation}) {
        line += ';';
        line += constraintName(component->constraint);
        line += ';';
        line += formatDouble(component->sigma);
    }
    return line;
}

std::optional<ControlSelection> parseControl(std::string_view value)
{
    std::vector<std::string_view> fields;
    std::size_t start = 0;
    while (true) {
        const std::size_t cut = value.find(';', start);
        fields.push_back(value.substr(start, cut == std::string_view::npos ? cut : cut - start));
        if (cut == std::string_view::npos) {
            break;
        }
        start = cut + 1;
        if (fields.size() > 8) {
            return std::nullopt;
        }
    }
    if (fields.size() != 8) {
        return std::nullopt;
    }
    ControlSelection selection;
    std::optional<std::string> id = percentDecode(fields[0]);
    const std::optional<ControlOrigin> origin = fromName(fields[1], kOriginValues);
    if (!id || !origin) {
        return std::nullopt;
    }
    selection.point.pointId = std::move(*id);
    selection.origin = *origin;
    ControlComponent* components[] = {&selection.point.northing, &selection.point.easting,
                                      &selection.point.elevation};
    for (std::size_t i = 0; i < 3; ++i) {
        const std::optional<ControlConstraint> constraint =
            fromName(fields[2 + 2 * i], kConstraintValues, constraintName);
        const std::optional<double> sigma = parseDouble(fields[3 + 2 * i]);
        if (!constraint || !sigma) {
            return std::nullopt;
        }
        components[i]->constraint = *constraint;
        components[i]->sigma = *sigma;
    }
    return selection;
}

katana::core::Error invalidSetting(std::string message)
{
    return makeError(ErrorCode::InvalidArgument, std::move(message), "reduction settings");
}

katana::core::Error parseError(std::size_t line, std::string message)
{
    return makeError(ErrorCode::ParseFailure,
                     "line " + std::to_string(line) + " of the reduction settings: " +
                         std::move(message));
}

} // namespace

// ---- Names -----------------------------------------------------------------------

const char* toString(AtmosphericCorrection value)
{
    switch (value) {
    case AtmosphericCorrection::None:
        return "none";
    case AtmosphericCorrection::Auto:
        return "auto";
    case AtmosphericCorrection::Recompute:
        return "recompute";
    case AtmosphericCorrection::Fixed:
        return "fixed";
    }
    return "auto";
}

const char* toString(PrismConstantPolicy value)
{
    switch (value) {
    case PrismConstantPolicy::Auto:
        return "auto";
    case PrismConstantPolicy::Override:
        return "override";
    case PrismConstantPolicy::None:
        return "none";
    }
    return "auto";
}

const char* toString(FaceHandling value)
{
    switch (value) {
    case FaceHandling::Average:
        return "average";
    case FaceHandling::FaceLeftOnly:
        return "face-left-only";
    case FaceHandling::Separate:
        return "separate";
    }
    return "average";
}

const char* toString(HeightReduction value)
{
    switch (value) {
    case HeightReduction::None:
        return "none";
    case HeightReduction::Ellipsoid:
        return "ellipsoid";
    case HeightReduction::Geoid:
        return "geoid";
    }
    return "none";
}

const char* toString(GridScale value)
{
    switch (value) {
    case GridScale::None:
        return "none";
    case GridScale::Fixed:
        return "fixed";
    case GridScale::FromProjection:
        return "projection";
    }
    return "none";
}

const char* toString(AdjustmentMethod value)
{
    switch (value) {
    case AdjustmentMethod::None:
        return "none";
    case AdjustmentMethod::Traverse:
        return "traverse";
    case AdjustmentMethod::Network:
        return "network";
    }
    return "none";
}

const char* toString(TraverseRule value)
{
    switch (value) {
    case TraverseRule::Bowditch:
        return "bowditch";
    case TraverseRule::Transit:
        return "transit";
    case TraverseRule::LeastSquares:
        return "least-squares";
    }
    return "bowditch";
}

const char* toString(NetworkDimension value)
{
    switch (value) {
    case NetworkDimension::Horizontal:
        return "horizontal";
    case NetworkDimension::Levels:
        return "levels";
    case NetworkDimension::HorizontalAndLevels:
        return "horizontal-and-levels";
    }
    return "horizontal";
}

const char* toString(OutlierTest value)
{
    switch (value) {
    case OutlierTest::None:
        return "none";
    case OutlierTest::Baarda:
        return "baarda";
    case OutlierTest::Tau:
        return "tau";
    }
    return "baarda";
}

const char* toString(ControlOrigin value)
{
    switch (value) {
    case ControlOrigin::File:
        return "file";
    case ControlOrigin::Drawing:
        return "drawing";
    }
    return "file";
}

// ---- Validation --------------------------------------------------------------------

Status validateReductionSettings(const ReductionSettings& settings)
{
    auto finite = [](double value) { return std::isfinite(value); };
    auto positive = [](double value) { return std::isfinite(value) && value > 0.0; };

    if (!finite(settings.fixedPpm)) {
        return invalidSetting("the fixed atmospheric correction is not a number");
    }
    if (!finite(settings.prismConstant)) {
        return invalidSetting("the prism constant is not a number");
    }
    if (!positive(settings.faceTolerances.horizontal) || !positive(settings.faceTolerances.zenith) ||
        !positive(settings.faceTolerances.distance)) {
        return invalidSetting("the face left / face right tolerances must be positive");
    }
    if (!finite(settings.refractionCoefficient) || settings.refractionCoefficient < -1.0 ||
        settings.refractionCoefficient > 1.0) {
        return invalidSetting("the coefficient of refraction must lie between -1 and 1");
    }
    if (!positive(settings.earthRadius)) {
        return invalidSetting("the earth radius must be positive");
    }
    if (!positive(settings.fixedGridScaleFactor)) {
        return invalidSetting("the grid scale factor must be positive");
    }
    if (!positive(settings.combinedFactor)) {
        return invalidSetting("the combined scale factor must be positive");
    }
    const ObservationPrecision& apriori = settings.apriori;
    for (const auto& [name, value] :
         {std::pair{"direction", apriori.direction}, std::pair{"zenith angle", apriori.zenith},
          std::pair{"distance (constant part)", apriori.distanceConstant},
          std::pair{"levelling", apriori.levellingPerSqrtKilometre},
          std::pair{"GNSS horizontal", apriori.gnssHorizontal},
          std::pair{"GNSS vertical", apriori.gnssVertical}}) {
        if (!positive(value)) {
            return invalidSetting(std::string("the a-priori standard deviation of a ") + name +
                                  " must be positive");
        }
    }
    for (const auto& [name, value] :
         {std::pair{"distance (ppm part)", apriori.distancePpm},
          std::pair{"instrument centring", apriori.instrumentCentring},
          std::pair{"target centring", apriori.targetCentring},
          std::pair{"instrument and target heights", apriori.heightMeasurement}}) {
        if (!finite(value) || value < 0.0) {
            return invalidSetting(std::string("the a-priori standard deviation of the ") + name +
                                  " must not be negative");
        }
    }
    if (!finite(settings.confidenceLevel) || settings.confidenceLevel <= 0.0 ||
        settings.confidenceLevel >= 1.0) {
        return invalidSetting("the confidence level must lie strictly between 0 and 1");
    }
    if (!finite(settings.outlierSignificance) || settings.outlierSignificance <= 0.0 ||
        settings.outlierSignificance >= 1.0) {
        return invalidSetting("the outlier test significance must lie strictly between 0 and 1");
    }
    if (settings.maxIterations == 0) {
        return invalidSetting("the adjustment needs at least one iteration");
    }
    std::set<std::string, std::less<>> seen;
    for (const ControlSelection& selection : settings.control) {
        const ControlPoint& point = selection.point;
        if (point.pointId.empty()) {
            return invalidSetting("a control point has no point id");
        }
        if (!seen.insert(point.pointId).second) {
            return invalidSetting("control point '" + point.pointId + "' is listed twice");
        }
        for (const auto& [name, component] :
             {std::pair{"northing", &point.northing}, std::pair{"easting", &point.easting},
              std::pair{"elevation", &point.elevation}}) {
            if (!finite(component->sigma)) {
                return invalidSetting("control point '" + point.pointId + "': the " + name +
                                      " standard deviation is not a number");
            }
            if (component->constraint == ControlConstraint::Weighted && !(component->sigma > 0.0)) {
                return invalidSetting("control point '" + point.pointId + "': a weighted " + name +
                                      " needs a positive standard deviation");
            }
        }
    }
    return {};
}

std::vector<ControlSelection> controlFromFile(const SurveyProject& project)
{
    std::vector<ControlSelection> selection;
    selection.reserve(project.controlPoints.size());
    for (const ControlPoint& point : project.controlPoints) {
        selection.push_back(ControlSelection{point, ControlOrigin::File});
    }
    return selection;
}

// ---- Text form ---------------------------------------------------------------------

std::string serialiseReductionSettings(const ReductionSettings& settings)
{
    // The accessors in the tables hand out references into a settings value;
    // writing reads through a copy rather than casting const away.
    ReductionSettings copy = settings;
    std::string text;
    text.reserve(2048);
    auto line = [&text](std::string_view key, std::string_view value) {
        text += key;
        text += '=';
        text += value;
        text += '\n';
    };
    line(kHeaderKey, std::to_string(kReductionSettingsVersion));
    for (const EnumKey& key : kEnumKeys) {
        line(key.key, key.get(copy));
    }
    for (const DoubleKey& key : kDoubleKeys) {
        line(key.key, formatDouble(key.field(copy)));
    }
    for (const BoolKey& key : kBoolKeys) {
        line(key.key, key.field(copy) ? "true" : "false");
    }
    line(kMaxIterationsKey, std::to_string(copy.maxIterations));
    for (const ControlSelection& selection : copy.control) {
        text += controlLine(selection);
        text += '\n';
    }
    return text;
}

Result<ReductionSettings> parseReductionSettings(std::string_view text,
                                                 std::vector<std::string>* warnings)
{
    ReductionSettings settings;
    settings.control.clear();
    bool sawHeader = false;
    std::set<std::string, std::less<>> seen;
    std::size_t lineNumber = 0;
    std::size_t start = 0;
    while (start <= text.size()) {
        const std::size_t cut = text.find('\n', start);
        std::string_view line =
            text.substr(start, cut == std::string_view::npos ? std::string_view::npos : cut - start);
        start = cut == std::string_view::npos ? text.size() + 1 : cut + 1;
        ++lineNumber;
        if (!line.empty() && line.back() == '\r') {
            line.remove_suffix(1);
        }
        if (line.empty() || line.front() == '#') {
            continue;
        }
        const std::size_t equals = line.find('=');
        if (equals == std::string_view::npos) {
            return parseError(lineNumber, "expected key=value, found '" + std::string(line) + "'");
        }
        const std::string_view key = line.substr(0, equals);
        const std::string_view value = line.substr(equals + 1);

        if (!sawHeader) {
            if (key != kHeaderKey) {
                return makeError(ErrorCode::ParseFailure,
                                 "this text is not a set of Katana reduction settings",
                                 "it must begin with " + std::string(kHeaderKey) + "=<version>");
            }
            const std::optional<std::size_t> version = parseSize(value);
            if (!version || *version == 0) {
                return parseError(lineNumber, "the settings version '" + std::string(value) +
                                                  "' is not a positive whole number");
            }
            if (*version > static_cast<std::size_t>(kReductionSettingsVersion)) {
                return makeError(ErrorCode::Unsupported,
                                 "these reduction settings were written by a newer Katana "
                                 "(settings version " +
                                     std::to_string(*version) + "); this one reads up to version " +
                                     std::to_string(kReductionSettingsVersion));
            }
            sawHeader = true;
            continue;
        }

        if (key == kControlKey) {
            std::optional<ControlSelection> selection = parseControl(value);
            if (!selection) {
                return parseError(lineNumber,
                                  "a control line needs id;origin;then constraint;sigma for "
                                  "northing, easting and elevation");
            }
            settings.control.push_back(std::move(*selection));
            continue;
        }
        if (!seen.insert(std::string(key)).second) {
            return parseError(lineNumber, "'" + std::string(key) + "' is given twice");
        }

        bool known = false;
        for (const EnumKey& entry : kEnumKeys) {
            if (entry.key == key) {
                known = true;
                if (!entry.set(settings, value)) {
                    return parseError(lineNumber, "'" + std::string(value) +
                                                      "' is not a choice for " + std::string(key));
                }
            }
        }
        for (const DoubleKey& entry : kDoubleKeys) {
            if (entry.key == key) {
                known = true;
                const std::optional<double> number = parseDouble(value);
                if (!number) {
                    return parseError(lineNumber, "'" + std::string(value) + "' is not a number (" +
                                                      std::string(key) + ")");
                }
                entry.field(settings) = *number;
            }
        }
        for (const BoolKey& entry : kBoolKeys) {
            if (entry.key == key) {
                known = true;
                const std::optional<bool> flag = parseBool(value);
                if (!flag) {
                    return parseError(lineNumber, std::string(key) + " must be true or false");
                }
                entry.field(settings) = *flag;
            }
        }
        if (key == kMaxIterationsKey) {
            known = true;
            const std::optional<std::size_t> count = parseSize(value);
            if (!count) {
                return parseError(lineNumber, "the iteration limit must be a whole number");
            }
            settings.maxIterations = *count;
        }
        if (!known && warnings != nullptr) {
            warnings->push_back("line " + std::to_string(lineNumber) + ": setting '" +
                                std::string(key) + "' is not known to this version and was ignored");
        }
    }
    if (!sawHeader) {
        return makeError(ErrorCode::ParseFailure, "the reduction settings text is empty");
    }
    if (Status status = validateReductionSettings(settings); !status.ok()) {
        return status.error();
    }
    return settings;
}

} // namespace katana::survey
