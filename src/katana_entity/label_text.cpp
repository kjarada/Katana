#include "katana/entity/label_text.hpp"

#include <algorithm>
#include <charconv>
#include <cmath>
#include <optional>

#include "katana/entity/dimension_text.hpp"
#include "katana/entity/tables.hpp"
#include "katana/entity/text_block.hpp"
#include "katana/math/numerics.hpp"

namespace katana::entity {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;
using katana::math::kPi;
using katana::math::kRadToDeg;
using katana::math::kTwoPi;

namespace {

// The value names by kind. The common ones come first for every kind.
constexpr std::string_view kCommonValues[] = {"id", "layer", "code", "point", "description"};
constexpr std::string_view kPointValues[] = {"x", "y", "easting", "northing", "z", "rl"};
constexpr std::string_view kSegmentValues[] = {"bearing", "distance", "length", "dx",
                                               "dy",      "dz",       "grade",  "segment"};
constexpr std::string_view kArcValues[] = {"radius", "length", "chord", "delta", "bearing",
                                           "tangent"};
constexpr std::string_view kAreaValues[] = {"area", "perimeter", "x", "y"};
constexpr std::string_view kChainageValues[] = {"chainage", "x", "y", "alignment"};

// A square metre per acre: the international acre is exactly 4046.8564224
// m2 (66 x 660 international feet of 0.3048 m).
constexpr double kSquareMetresPerAcre = 4046.8564224;
// A foot is exactly 0.3048 m (the international yard and pound agreement of 1959).
constexpr double kMetresPerFoot = 0.3048;

bool isPropertyName(std::string_view name)
{
    return name.size() > 5 && name.substr(0, 5) == "prop.";
}

// One field's parts: the value name and its steps.
struct Field {
    std::string_view name;
    std::vector<std::string_view> steps;
};

Field parseField(std::string_view inside)
{
    Field field;
    std::size_t start = 0;
    bool first = true;
    while (true) {
        const std::size_t colon = inside.find(':', start);
        const std::string_view part =
            inside.substr(start, colon == std::string_view::npos ? std::string_view::npos
                                                                  : colon - start);
        if (first) {
            field.name = part;
            first = false;
        } else {
            field.steps.push_back(part);
        }
        if (colon == std::string_view::npos) {
            break;
        }
        start = colon + 1;
    }
    return field;
}

// ".3f" -> 3; nullopt when it is not a number format.
std::optional<int> fixedDecimals(std::string_view step)
{
    if (step.size() < 3 || step.front() != '.' || step.back() != 'f') {
        return std::nullopt;
    }
    int decimals = 0;
    const std::string_view digits = step.substr(1, step.size() - 2);
    const auto [end, error] = std::from_chars(digits.data(), digits.data() + digits.size(), decimals);
    if (error != std::errc{} || end != digits.data() + digits.size() || decimals < 0 ||
        decimals > 12) {
        return std::nullopt;
    }
    return decimals;
}

// "dms" -> 0, "dms.2" -> 2; nullopt when it is not a DMS step.
std::optional<int> dmsDecimals(std::string_view step, std::string_view stem)
{
    if (step == stem) {
        return 0;
    }
    if (step.size() <= stem.size() + 1 || step.substr(0, stem.size()) != stem ||
        step[stem.size()] != '.') {
        return std::nullopt;
    }
    int decimals = 0;
    const std::string_view digits = step.substr(stem.size() + 1);
    const auto [end, error] = std::from_chars(digits.data(), digits.data() + digits.size(), decimals);
    if (error != std::errc{} || end != digits.data() + digits.size() || decimals < 0 ||
        decimals > 6) {
        return std::nullopt;
    }
    return decimals;
}

bool isAngular(LabelQuantity quantity)
{
    return quantity == LabelQuantity::Angle || quantity == LabelQuantity::Bearing;
}

// Applies the steps to a value. Returns the text, or nullopt with `problem`
// set when a step does not apply.
std::optional<std::string> apply(const LabelValue& value, const std::vector<std::string_view>& steps,
                                 std::string* problem)
{
    const auto fail = [&](std::string why) -> std::optional<std::string> {
        if (problem != nullptr) {
            *problem = std::move(why);
        }
        return std::nullopt;
    };
    if (value.quantity == LabelQuantity::Text) {
        std::string text = value.text;
        for (const std::string_view step : steps) {
            if (step == "upper" || step == "lower") {
                for (char& c : text) {
                    if (step == "upper" && c >= 'a' && c <= 'z') {
                        c = static_cast<char>(c - 'a' + 'A');
                    } else if (step == "lower" && c >= 'A' && c <= 'Z') {
                        c = static_cast<char>(c - 'A' + 'a');
                    }
                }
            } else {
                return fail("'" + std::string(step) + "' does not apply to text");
            }
        }
        return text;
    }

    double number = value.number;
    LabelQuantity quantity = value.quantity;
    // Once an angle has been turned into a plain number of degrees (deg,
    // gon, rad) it is printed as a number.
    bool angleAsNumber = false;
    for (std::size_t i = 0; i < steps.size(); ++i) {
        const std::string_view step = steps[i];
        const bool last = i + 1 == steps.size();
        if (const auto decimals = fixedDecimals(step)) {
            if (!last) {
                return fail("a number format must be the last step");
            }
            if (isAngular(quantity) && !angleAsNumber) {
                return fail("an angle needs deg, rad or gon before a number format");
            }
            if (quantity == LabelQuantity::Chainage) {
                return formatFixed(number, *decimals);
            }
            return formatFixed(number, *decimals);
        }
        if (isAngular(quantity) && !angleAsNumber) {
            if (const auto d = dmsDecimals(step, "dms")) {
                if (!last) {
                    return fail("dms must be the last step");
                }
                return formatDms(quantity == LabelQuantity::Bearing
                                     ? katana::math::normalizeAngle(number)
                                     : number,
                                 *d);
            }
            if (step == "dm") {
                if (!last) {
                    return fail("dm must be the last step");
                }
                return formatDm(quantity == LabelQuantity::Bearing
                                    ? katana::math::normalizeAngle(number)
                                    : number);
            }
            if (const auto d = dmsDecimals(step, "qb")) {
                if (quantity != LabelQuantity::Bearing) {
                    return fail("qb applies to a bearing only");
                }
                if (!last) {
                    return fail("qb must be the last step");
                }
                return formatQuadrantBearing(number, *d);
            }
            if (step == "deg") {
                number = (quantity == LabelQuantity::Bearing ? katana::math::normalizeAngle(number)
                                                             : number) *
                         kRadToDeg;
                angleAsNumber = true;
                continue;
            }
            if (step == "gon") {
                number = (quantity == LabelQuantity::Bearing ? katana::math::normalizeAngle(number)
                                                             : number) *
                         200.0 / kPi;
                angleAsNumber = true;
                continue;
            }
            if (step == "rad") {
                angleAsNumber = true;
                continue;
            }
            return fail("'" + std::string(step) + "' does not apply to an angle");
        }
        if (quantity == LabelQuantity::Length || quantity == LabelQuantity::Number ||
            quantity == LabelQuantity::Chainage) {
            if (step == "m") {
                continue;
            }
            if (step == "mm") {
                number *= 1000.0;
                continue;
            }
            if (step == "km") {
                number /= 1000.0;
                continue;
            }
            if (step == "ft") {
                number /= kMetresPerFoot;
                continue;
            }
        }
        if (quantity == LabelQuantity::Chainage) {
            if (const auto d = dmsDecimals(step, "ch")) {
                if (!last) {
                    return fail("ch must be the last step");
                }
                return formatChainage(number, *d == 0 ? 3 : *d);
            }
        }
        if (quantity == LabelQuantity::Area) {
            if (step == "m2") {
                continue;
            }
            if (step == "ha") {
                number /= 10000.0;
                continue;
            }
            if (step == "km2") {
                number /= 1.0e6;
                continue;
            }
            if (step == "ac") {
                number /= kSquareMetresPerAcre;
                continue;
            }
        }
        return fail("unknown or inapplicable step '" + std::string(step) + "'");
    }

    // No format step: the quantity's own format.
    switch (quantity) {
    case LabelQuantity::Integer:
        return formatFixed(number, 0);
    case LabelQuantity::Area:
        return formatFixed(number, 1);
    case LabelQuantity::Angle:
    case LabelQuantity::Bearing:
        if (angleAsNumber) {
            return formatFixed(number, 4);
        }
        return formatDms(quantity == LabelQuantity::Bearing ? katana::math::normalizeAngle(number)
                                                            : number,
                         0);
    case LabelQuantity::Text:
    case LabelQuantity::Number:
    case LabelQuantity::Length:
    case LabelQuantity::Chainage:
        break;
    }
    return formatFixed(number, 3);
}

// The quantity a value name has for checking a template, with no target to
// ask. prop.NAME may be anything, so it is checked as text-or-number: only
// steps that apply to neither are refused.
std::optional<LabelQuantity> quantityOf(std::string_view name, LabelKind kind)
{
    if (name == "id" || name == "segment") {
        return LabelQuantity::Integer;
    }
    if (name == "layer" || name == "code" || name == "point" || name == "description" ||
        name == "alignment") {
        return LabelQuantity::Text;
    }
    if (name == "bearing") {
        return LabelQuantity::Bearing;
    }
    if (name == "delta") {
        return LabelQuantity::Angle;
    }
    if (name == "area") {
        return LabelQuantity::Area;
    }
    if (name == "chainage") {
        return LabelQuantity::Chainage;
    }
    if (name == "grade" || name == "dz" || name == "z" || name == "rl") {
        return LabelQuantity::Number;
    }
    (void)kind;
    return LabelQuantity::Length;
}

bool kindHas(LabelKind kind, std::string_view name)
{
    for (const std::string_view common : kCommonValues) {
        if (common == name) {
            return true;
        }
    }
    for (const std::string_view known : labelValueNames(kind)) {
        if (known == name) {
            return true;
        }
    }
    return false;
}

// Walks a template line: literal text and fields, calling `field` for each
// field. False when the braces do not balance.
template <typename OnText, typename OnField>
bool walk(std::string_view line, OnText&& onText, OnField&& onField)
{
    std::size_t i = 0;
    while (i < line.size()) {
        const char c = line[i];
        if (c == '{') {
            if (i + 1 < line.size() && line[i + 1] == '{') {
                onText(std::string_view("{"));
                i += 2;
                continue;
            }
            const std::size_t close = line.find('}', i + 1);
            if (close == std::string_view::npos) {
                return false;
            }
            const std::string_view inside = line.substr(i + 1, close - i - 1);
            if (inside.find('{') != std::string_view::npos) {
                return false;
            }
            onField(inside);
            i = close + 1;
            continue;
        }
        if (c == '}') {
            if (i + 1 < line.size() && line[i + 1] == '}') {
                onText(std::string_view("}"));
                i += 2;
                continue;
            }
            return false;
        }
        const std::size_t next = line.find_first_of("{}", i);
        const std::size_t end = next == std::string_view::npos ? line.size() : next;
        onText(line.substr(i, end - i));
        i = end;
    }
    return true;
}

} // namespace

std::vector<std::string_view> labelValueNames(LabelKind kind)
{
    const auto list = [](const auto& names) {
        return std::vector<std::string_view>(std::begin(names), std::end(names));
    };
    switch (kind) {
    case LabelKind::Point:
        return list(kPointValues);
    case LabelKind::Segment:
        return list(kSegmentValues);
    case LabelKind::Arc:
        return list(kArcValues);
    case LabelKind::Area:
        return list(kAreaValues);
    case LabelKind::Chainage:
        return list(kChainageValues);
    }
    return {};
}

Status
checkTemplate(std::string_view templateText,
              const std::function<std::optional<LabelQuantity>(std::string_view)>& quantityOfName,
              const std::function<std::string(std::string_view)>& unknownValue)
{
    std::string problem;
    for (const std::string_view line : textLines(templateText)) {
        const bool balanced = walk(
            line, [](std::string_view) {},
            [&](std::string_view inside) {
                if (!problem.empty()) {
                    return;
                }
                const Field field = parseField(inside);
                if (field.name.empty()) {
                    problem = "an empty field {}";
                    return;
                }
                if (isPropertyName(field.name)) {
                    // A property may be a number or text: refuse only steps
                    // that apply to neither.
                    std::string asNumber;
                    std::string asText;
                    if (!apply(LabelValue::of(LabelQuantity::Number, 0.0), field.steps, &asNumber) &&
                        !apply(LabelValue::ofText(""), field.steps, &asText)) {
                        problem = "{" + std::string(inside) + "}: " + asNumber;
                    }
                    return;
                }
                const auto quantity = quantityOfName(field.name);
                if (!quantity) {
                    problem = unknownValue(field.name);
                    return;
                }
                std::string why;
                if (!apply(LabelValue{*quantity, 0.0, {}}, field.steps, &why)) {
                    problem = "{" + std::string(inside) + "}: " + why;
                }
            });
        if (!balanced) {
            return makeError(ErrorCode::InvalidArgument,
                             "a label template's braces do not balance (use {{ and }} for a brace)",
                             std::string(line));
        }
        if (!problem.empty()) {
            return makeError(ErrorCode::InvalidArgument, "the label template is not valid",
                             problem);
        }
    }
    return {};
}

Status checkLabelTemplate(std::string_view templateText, LabelKind kind)
{
    return checkTemplate(
        templateText,
        [kind](std::string_view name) -> std::optional<LabelQuantity> {
            if (!kindHas(kind, name)) {
                return std::nullopt;
            }
            return quantityOf(name, kind);
        },
        [kind](std::string_view name) {
            return "a " + std::string(toString(kind)) + " label has no value '" +
                   std::string(name) + "'";
        });
}

std::vector<std::string> templateFields(std::string_view templateText)
{
    std::vector<std::string> names;
    for (const std::string_view line : textLines(templateText)) {
        std::vector<std::string> found;
        const bool balanced = walk(
            line, [](std::string_view) {},
            [&](std::string_view inside) { found.emplace_back(parseField(inside).name); });
        if (!balanced) {
            return {};
        }
        for (std::string& name : found) {
            if (!name.empty() && std::find(names.begin(), names.end(), name) == names.end()) {
                names.push_back(std::move(name));
            }
        }
    }
    return names;
}

std::string formatLabel(std::string_view templateText, const LabelValues& values)
{
    std::string result;
    bool anyLine = false;
    for (const std::string_view line : textLines(templateText)) {
        std::string text;
        bool absent = false;
        const bool balanced = walk(
            line, [&](std::string_view literal) { text += literal; },
            [&](std::string_view inside) {
                const Field field = parseField(inside);
                const auto found = values.find(field.name);
                if (found == values.end()) {
                    absent = true;
                    return;
                }
                const auto formatted = apply(found->second, field.steps, nullptr);
                text += formatted ? *formatted : std::string("?");
            });
        if (!balanced) {
            text = std::string(line);
        }
        if (absent) {
            continue;
        }
        if (anyLine) {
            result += '\n';
        }
        result += text;
        anyLine = true;
    }
    return result;
}

std::string formatValue(const LabelValue& value)
{
    return apply(value, {}, nullptr).value_or("?");
}

std::string formatFixed(double value, int decimals)
{
    DimensionStyle style;
    style.decimals = std::clamp(decimals, 0, 12);
    std::string text = formatMeasurement(value, style);
    // formatMeasurement keeps the sign of a value that rounds to zero at
    // these decimals ("-0.000"); a label never says minus nothing.
    if (!text.empty() && text.front() == '-' &&
        text.find_first_not_of("-0.") == std::string::npos) {
        text.erase(0, 1);
    }
    return text;
}

std::string formatDms(double radians, int secondDecimals)
{
    if (!std::isfinite(radians)) {
        return "?";
    }
    const int decimals = std::clamp(secondDecimals, 0, 6);
    const bool negative = radians < 0.0;
    // Rounded ONCE, in units of the last printed digit of a second, then
    // split: rounding the seconds on their own printed 59.9996" as 60".
    double unitsPerSecond = 1.0;
    for (int i = 0; i < decimals; ++i) {
        unitsPerSecond *= 10.0;
    }
    const double totalUnits = std::abs(radians) * kRadToDeg * 3600.0 * unitsPerSecond;
    const long long units = std::llround(totalUnits);
    const long long perMinute = static_cast<long long>(60.0 * unitsPerSecond);
    const long long perDegree = 60 * perMinute;
    const long long degrees = units / perDegree;
    const long long minutes = (units % perDegree) / perMinute;
    const long long secondUnits = units % perMinute;

    std::string text = negative && units != 0 ? "-" : "";
    text += std::to_string(degrees);
    text += "°";
    text += minutes < 10 ? "0" : "";
    text += std::to_string(minutes);
    text += "'";
    const long long wholeSeconds = secondUnits / static_cast<long long>(unitsPerSecond);
    text += wholeSeconds < 10 ? "0" : "";
    text += std::to_string(wholeSeconds);
    if (decimals > 0) {
        std::string fraction =
            std::to_string(secondUnits % static_cast<long long>(unitsPerSecond));
        fraction.insert(0, static_cast<std::size_t>(decimals) - fraction.size(), '0');
        text += "." + fraction;
    }
    text += "\"";
    return text;
}

std::string formatDm(double radians)
{
    if (!std::isfinite(radians)) {
        return "?";
    }
    const bool negative = radians < 0.0;
    const long long minutesTotal = std::llround(std::abs(radians) * kRadToDeg * 60.0);
    std::string text = negative && minutesTotal != 0 ? "-" : "";
    text += std::to_string(minutesTotal / 60);
    text += "°";
    const long long minutes = minutesTotal % 60;
    text += minutes < 10 ? "0" : "";
    text += std::to_string(minutes);
    text += "'";
    return text;
}

std::string formatQuadrantBearing(double bearing, int secondDecimals)
{
    if (!std::isfinite(bearing)) {
        return "?";
    }
    const double b = katana::math::normalizeAngle(bearing);
    const double quarter = 0.5 * kPi;
    // N x E for [0, 90), S x E for [90, 180), S x W for [180, 270), N x W.
    if (b < quarter) {
        return "N " + formatDms(b, secondDecimals) + " E";
    }
    if (b < kPi) {
        return "S " + formatDms(kPi - b, secondDecimals) + " E";
    }
    if (b < 3.0 * quarter) {
        return "S " + formatDms(b - kPi, secondDecimals) + " W";
    }
    return "N " + formatDms(kTwoPi - b, secondDecimals) + " W";
}

std::string formatChainage(double chainage, int decimals)
{
    if (!std::isfinite(chainage)) {
        return "?";
    }
    // Rounded first and split after, so 999.9996 at three decimals is 1+000.000
    // and not 0+1000.000.
    const std::string fixed = formatFixed(std::abs(chainage), decimals);
    const std::size_t point = fixed.find('.');
    const std::string whole = fixed.substr(0, point);
    const std::string fraction = point == std::string::npos ? std::string() : fixed.substr(point);
    std::string metres = whole.size() > 3 ? whole.substr(whole.size() - 3) : whole;
    metres.insert(0, 3 - metres.size(), '0');
    const std::string kilometres = whole.size() > 3 ? whole.substr(0, whole.size() - 3) : "0";
    const bool negative = chainage < 0.0 && fixed.find_first_not_of("0.") != std::string::npos;
    return (negative ? "-" : "") + kilometres + "+" + metres + fraction;
}

} // namespace katana::entity
