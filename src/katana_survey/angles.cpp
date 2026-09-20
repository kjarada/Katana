#include "katana/survey/angles.hpp"

#include <algorithm>
#include <array>
#include <charconv>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <limits>
#include <optional>

namespace katana::survey {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::math::kHalfPi;
using katana::math::kPi;
using katana::math::kTwoPi;

namespace {

// Largest magnitude below which every integer is exactly representable (2^53).
// Not a tolerance: it bounds the integer arc-second arithmetic of the formatter.
constexpr double kMaxExactInteger = 9007199254740992.0;

constexpr int kMaxSecondsDecimals = 9;

constexpr bool isBlank(char c)
{
    return c == ' ' || c == '\t' || c == '\n' || c == '\r';
}

std::string_view trimmed(std::string_view text)
{
    while (!text.empty() && isBlank(text.front())) {
        text.remove_prefix(1);
    }
    while (!text.empty() && isBlank(text.back())) {
        text.remove_suffix(1);
    }
    return text;
}

struct NumberToken {
    double value = 0.0;
    bool hasFraction = false;
};

// Cursor over the text of one sexagesimal angle.
class Scanner {
  public:
    explicit Scanner(std::string_view text) : text_(text) {}

    [[nodiscard]] bool atEnd() const { return position_ >= text_.size(); }

    void skipBlanks()
    {
        while (!atEnd() && isBlank(text_[position_])) {
            ++position_;
        }
    }

    bool consume(std::string_view token)
    {
        if (text_.substr(position_, token.size()) != token) {
            return false;
        }
        position_ += token.size();
        return true;
    }

    // Unsigned decimal number without exponent: digits[.digits] or .digits.
    // from_chars alone would also accept "1e5", "inf" and "nan".
    std::optional<NumberToken> number()
    {
        const std::size_t begin = position_;
        std::size_t end = begin;
        std::size_t digitCount = 0;
        bool hasFraction = false;
        while (end < text_.size() && text_[end] >= '0' && text_[end] <= '9') {
            ++end;
            ++digitCount;
        }
        if (end < text_.size() && text_[end] == '.') {
            hasFraction = true;
            ++end;
            while (end < text_.size() && text_[end] >= '0' && text_[end] <= '9') {
                ++end;
                ++digitCount;
            }
        }
        if (digitCount == 0) {
            return std::nullopt;
        }
        NumberToken token;
        token.hasFraction = hasFraction;
        const auto parsed = std::from_chars(text_.data() + begin, text_.data() + end, token.value);
        if (parsed.ec != std::errc{} || !std::isfinite(token.value)) {
            return std::nullopt;
        }
        position_ = end;
        return token;
    }

  private:
    std::string_view text_;
    std::size_t position_ = 0;
};

// Field marks in UTF-8. Longer alternatives first ("''" before "'").
constexpr std::array<std::string_view, 6> kDegreeMarks = {"\xC2\xB0", "\xC2\xBA", "d",
                                                          "D",        ":",        "-"};
constexpr std::array<std::string_view, 6> kMinuteMarks = {"\xE2\x80\xB2", "'", "m", "M", ":", "-"};
constexpr std::array<std::string_view, 5> kSecondMarks = {"\xE2\x80\xB3", "\"", "''", "s", "S"};

template <std::size_t N>
void consumeOneMark(Scanner& scanner, const std::array<std::string_view, N>& marks)
{
    scanner.skipBlanks();
    for (const std::string_view mark : marks) {
        if (scanner.consume(mark)) {
            break;
        }
    }
    scanner.skipBlanks();
}

Result<double> parseFailure(std::string_view text, std::string message)
{
    return makeError(ErrorCode::ParseFailure, std::move(message), std::string(text));
}

// Parses an unsigned sexagesimal angle into arc-seconds.
Result<double> parseUnsignedArcSeconds(std::string_view text, std::string_view original)
{
    Scanner scanner(text);
    std::array<NumberToken, 3> fields{};
    std::size_t fieldCount = 0;

    while (fieldCount < fields.size()) {
        scanner.skipBlanks();
        const auto token = scanner.number();
        if (!token) {
            break;
        }
        fields[fieldCount] = *token;
        if (fieldCount == 0) {
            consumeOneMark(scanner, kDegreeMarks);
        } else if (fieldCount == 1) {
            consumeOneMark(scanner, kMinuteMarks);
        } else {
            consumeOneMark(scanner, kSecondMarks);
        }
        ++fieldCount;
    }

    if (fieldCount == 0) {
        return parseFailure(original, "expected an angle such as 45°30'15\"");
    }
    if (!scanner.atEnd()) {
        return parseFailure(original, "unexpected characters in angle");
    }
    for (std::size_t i = 0; i + 1 < fieldCount; ++i) {
        if (fields[i].hasFraction) {
            return parseFailure(original, "only the last angle field may carry a fraction");
        }
    }
    if (fieldCount >= 2 && !(fields[1].value < 60.0)) {
        return parseFailure(original, "minutes must be below 60");
    }
    if (fieldCount == 3 && !(fields[2].value < 60.0)) {
        return parseFailure(original, "seconds must be below 60");
    }
    return fields[0].value * 3600.0 + fields[1].value * 60.0 + fields[2].value;
}

std::string formatFields(const DmsAngle& dms, int secondsDecimals)
{
    // Large enough for "SS." plus kMaxSecondsDecimals digits and the terminator.
    std::array<char, 32> seconds{};
    const int width = secondsDecimals > 0 ? secondsDecimals + 3 : 2;
    std::snprintf(seconds.data(), seconds.size(), "%0*.*f", width, secondsDecimals, dms.seconds);

    std::string text;
    if (dms.negative) {
        text += '-';
    }
    text += std::to_string(dms.degrees);
    text += "\xC2\xB0";
    if (dms.minutes < 10) {
        text += '0';
    }
    text += std::to_string(dms.minutes);
    text += '\'';
    text += seconds.data();
    text += '"';
    return text;
}

} // namespace

Result<double> dmsToRadians(const DmsAngle& dms)
{
    if (dms.minutes >= 60) {
        return makeError(ErrorCode::InvalidArgument, "minutes must be below 60",
                         "minutes=" + std::to_string(dms.minutes));
    }
    if (!std::isfinite(dms.seconds) || dms.seconds < 0.0 || !(dms.seconds < 60.0)) {
        return makeError(ErrorCode::InvalidArgument, "seconds must lie in [0, 60)",
                         "seconds=" + std::to_string(dms.seconds));
    }
    const double arcSeconds = static_cast<double>(dms.degrees) * 3600.0 +
                              static_cast<double>(dms.minutes) * 60.0 + dms.seconds;
    const double radians = arcSeconds * kArcSecondToRad;
    return dms.negative ? -radians : radians;
}

Result<DmsAngle> radiansToDms(double radians, int secondsDecimals)
{
    if (!std::isfinite(radians)) {
        return makeError(ErrorCode::InvalidArgument, "angle is not finite");
    }
    if (secondsDecimals < 0 || secondsDecimals > kMaxSecondsDecimals) {
        return makeError(ErrorCode::InvalidArgument, "seconds decimals must lie in 0..9",
                         "secondsDecimals=" + std::to_string(secondsDecimals));
    }
    std::int64_t unitsPerSecond = 1;
    for (int i = 0; i < secondsDecimals; ++i) {
        unitsPerSecond *= 10;
    }
    const double totalUnits =
        std::abs(radians) * kRadToArcSecond * static_cast<double>(unitsPerSecond);
    if (!(totalUnits < kMaxExactInteger)) {
        return makeError(ErrorCode::InvalidArgument,
                         "angle is too large for the requested seconds resolution");
    }

    // Integer arithmetic from here on: the carry of a rounded 59.999... seconds
    // into minutes and degrees is exact.
    const std::int64_t units = std::llround(totalUnits);
    const std::int64_t unitsPerMinute = 60 * unitsPerSecond;
    const std::int64_t unitsPerDegree = 60 * unitsPerMinute;
    const std::int64_t degrees = units / unitsPerDegree;
    const std::int64_t remainder = units % unitsPerDegree;
    if (degrees > static_cast<std::int64_t>(std::numeric_limits<unsigned>::max())) {
        return makeError(ErrorCode::InvalidArgument, "angle is too large to split into DMS");
    }

    DmsAngle dms;
    dms.negative = radians < 0.0 && units != 0; // never "-0°00'00""
    dms.degrees = static_cast<unsigned>(degrees);
    dms.minutes = static_cast<unsigned>(remainder / unitsPerMinute);
    dms.seconds = static_cast<double>(remainder % unitsPerMinute) /
                  static_cast<double>(unitsPerSecond);
    return dms;
}

Result<double> parseDms(std::string_view text)
{
    std::string_view body = trimmed(text);
    bool negative = false;
    if (!body.empty() && (body.front() == '+' || body.front() == '-')) {
        negative = body.front() == '-';
        body.remove_prefix(1);
    }
    const auto arcSeconds = parseUnsignedArcSeconds(body, text);
    if (!arcSeconds) {
        return arcSeconds.error();
    }
    const double radians = *arcSeconds * kArcSecondToRad;
    return negative ? -radians : radians;
}

Result<std::string> formatDms(double radians, int secondsDecimals)
{
    const auto dms = radiansToDms(radians, secondsDecimals);
    if (!dms) {
        return dms.error();
    }
    return formatFields(*dms, secondsDecimals);
}

Result<QuadrantBearing> azimuthToBearing(double azimuth)
{
    if (!std::isfinite(azimuth)) {
        return makeError(ErrorCode::InvalidArgument, "azimuth is not finite");
    }
    const double wrapped = normalizeAzimuth(azimuth);
    if (wrapped <= kHalfPi) {
        return QuadrantBearing{BearingMeridian::North, wrapped, BearingSide::East};
    }
    if (wrapped <= kPi) {
        return QuadrantBearing{BearingMeridian::South, kPi - wrapped, BearingSide::East};
    }
    if (wrapped <= kPi + kHalfPi) {
        return QuadrantBearing{BearingMeridian::South, wrapped - kPi, BearingSide::West};
    }
    return QuadrantBearing{BearingMeridian::North, kTwoPi - wrapped, BearingSide::West};
}

Result<double> bearingToAzimuth(const QuadrantBearing& bearing)
{
    if (!std::isfinite(bearing.angle) || bearing.angle < 0.0 || bearing.angle > kHalfPi) {
        return makeError(ErrorCode::InvalidArgument, "bearing angle must lie in [0, pi/2]",
                         "angle=" + std::to_string(bearing.angle));
    }
    const bool north = bearing.meridian == BearingMeridian::North;
    const bool east = bearing.side == BearingSide::East;
    double azimuth = 0.0;
    if (north) {
        azimuth = east ? bearing.angle : kTwoPi - bearing.angle;
    } else {
        azimuth = east ? kPi - bearing.angle : kPi + bearing.angle;
    }
    return normalizeAzimuth(azimuth); // N 0 W is azimuth 0, not 2*pi
}

Result<double> parseBearing(std::string_view text)
{
    const std::string_view body = trimmed(text);
    if (body.size() < 3) {
        return parseFailure(text, "expected a bearing such as N 45°30'15\" E");
    }
    const char first = body.front();
    const char last = body.back();

    QuadrantBearing bearing;
    if (first == 'N' || first == 'n') {
        bearing.meridian = BearingMeridian::North;
    } else if (first == 'S' || first == 's') {
        bearing.meridian = BearingMeridian::South;
    } else {
        return parseFailure(text, "bearing must start with N or S");
    }
    if (last == 'E' || last == 'e') {
        bearing.side = BearingSide::East;
    } else if (last == 'W' || last == 'w') {
        bearing.side = BearingSide::West;
    } else {
        return parseFailure(text, "bearing must end with E or W");
    }

    const std::string_view angleText = trimmed(body.substr(1, body.size() - 2));
    const auto arcSeconds = parseUnsignedArcSeconds(angleText, text);
    if (!arcSeconds) {
        return arcSeconds.error();
    }
    // Compared in arc-seconds so that exactly 90° is accepted despite the
    // rounding of the conversion to radians.
    if (*arcSeconds > 90.0 * 3600.0) {
        return parseFailure(text, "bearing angle must not exceed 90 degrees");
    }
    bearing.angle = std::min(*arcSeconds * kArcSecondToRad, kHalfPi);
    return bearingToAzimuth(bearing);
}

Result<std::string> formatBearing(double azimuth, int secondsDecimals)
{
    const auto bearing = azimuthToBearing(azimuth);
    if (!bearing) {
        return bearing.error();
    }
    const auto angle = formatDms(bearing->angle, secondsDecimals);
    if (!angle) {
        return angle.error();
    }
    std::string text = bearing->meridian == BearingMeridian::North ? "N " : "S ";
    text += *angle;
    text += bearing->side == BearingSide::East ? " E" : " W";
    return text;
}

} // namespace katana::survey
