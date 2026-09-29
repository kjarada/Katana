// The opcode field file (.fld): a total-station job as tab-separated records,
// each opening with a numeric operation code.
//
// Specification this reader implements:
//   [FLD] "Field File Format", the format publisher's reference manual
//         chapter (June 2025 edition), sections 1.2 "Structure of the .fld
//         File", 1.3 "Point Description" and 1.8 "Full Description of ...
//         Opcodes". A file of this kind names itself on its first line,
//         "{Version 6.0}", and in its opening comments ("Field File
//         Version 6").
//
// What [FLD] says, and this reader relies on:
//   * One record per line: the opcode, then tab-separated values.
//   * Most records carry a POINT DESCRIPTION - feature code, string number,
//     point ID, point name, point comment - as five tab-separated values in
//     that order, a missing one still taking its tab.
//   * 100 units: angle unit, distance unit, pressure unit, temperature unit;
//     "Currently there is only one choice for each unit": decimal degrees and
//     metres. Any other unit is refused rather than guessed at.
//   * 02 coordinate: description, X, Y, Z - a "directly entered coordinate"
//     that needs no reduction.
//   * 03 station: description, instrument height - a setup on the point the
//     description names.
//   * 04 backsight: description, horizontal circle, vertical circle, slope
//     distance[, azimuth] - the measurement to the backsight.
//   * 05 target height, for opcodes 4, 6, 7 and 140 after it.
//   * 06 check measurement: description, HA, VA, SD to a known point.
//   * 07 EDM measurement: description, HA, VA, SD.
//   * 09 slope distance scale factor "to apply to subsequent slope distances".
//   * 29 memo, 41 additional text for the current point, 72 and 73 a real and
//     a text attribute (name, value) of the current point, -2 a comment.
//   * Circle readings are decimal degrees; the vertical circle is a zenith
//     reading.
//
// Readings that are this reader's own, stated so they can be checked:
//   * The COLUMN AFTER THE OPCODE. Every record of the files this reader was
//     written against holds an empty value between the opcode and the rest
//     ("07<tab><tab>KJ<tab>01<tab>KJ01..."); [FLD]'s syntax lines do not show
//     one. The records whose opcode has a fixed number of values (02, 03, 04,
//     06, 07) decide it for the whole file, before any is read: one field
//     more than [FLD] gives, the first of them empty, votes for the column;
//     exactly [FLD]'s count with a non-empty feature code votes against. A
//     record is not trusted to decide alone because one of each can look the
//     same: a record in the column's layout that has lost a value has
//     [FLD]'s count, and a feature code left empty makes its first field
//     empty. A fixed record whose count does not fit the file's layout is a
//     warning and is skipped - a guess at which value is the slope distance
//     reads a plausible survey that is wrong. A record with free text (05,
//     09, 29, 41, 72, 73, 100) takes an empty second field as the column.
//   * X is the easting and Y the northing, the map-grid convention [FLD]'s
//     "(x, y, z) coordinates" follows (the files it was checked against put
//     six-figure eastings in X and seven-figure MGA northings in Y).
//   * A point is identified by its point name where it has one and by its
//     point ID otherwise: [FLD] 1.5 finds a setup or a backsight by either.
//     A record with neither is skipped.
//   * A zenith reading past 180 degrees is face right, held as 360 - z.
//   * The file's opening "//" comments are not records. A "Key : value"
//     comment is kept in the project's metadata as "header: Key", and a
//     "Coordinate System" comment (with its "Zone", where there is one) is
//     the coordinate system the file declares - by name, exactly as written,
//     never turned into an EPSG code here.
//   * 72 "Target height" and "Prism constant" are attributes of the point
//     like any other: the target height that applies is 05's, which the
//     format defines as such, and the prism constant has no unit in the file.

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <numbers>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/text.hpp"
#include "katana/core/text_encoding.hpp"
#include "katana/surveyio/detect.hpp"
#include "katana/surveyio/format.hpp"
#include "topcon_raw_builder.hpp"

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::trimmed;
using katana::surveyio::FormatDescriptor;
using katana::surveyio::FormatSignature;
using katana::surveyio::ProbeInput;
using katana::surveyio::ReadOptions;
using katana::surveyio::ReadResult;
using katana::surveyio::topcon::faceOfZenithReading;
using katana::surveyio::topcon::parseReal;
using katana::surveyio::topcon::RawProjectBuilder;
using katana::surveyio::topcon::wrapToCircle;
using katana::surveyio::topcon::zenithInModelRange;
namespace survey = katana::survey;

constexpr std::string_view kFormatId = "opcode-field-file";
constexpr std::string_view kSpecification = "Field File Format 1.8";
constexpr double kPi = std::numbers::pi;
constexpr double kRadiansPerDegree = kPi / 180.0;

// The tab-separated fields of one line, the opcode first.
std::vector<std::string_view> splitTabs(std::string_view line)
{
    std::vector<std::string_view> fields;
    std::size_t start = 0;
    while (true) {
        const std::size_t tab = line.find('\t', start);
        fields.push_back(line.substr(start, tab == std::string_view::npos ? line.npos : tab - start));
        if (tab == std::string_view::npos) {
            return fields;
        }
        start = tab + 1;
    }
}

// An opcode: an optional minus and one to three digits ("02", "100", "-2").
std::optional<int> opcodeOf(std::string_view text)
{
    const bool negative = text.starts_with('-');
    const std::string_view digits = negative ? text.substr(1) : text;
    if (digits.empty() || digits.size() > 3) {
        return std::nullopt;
    }
    int value = 0;
    for (const char c : digits) {
        if (c < '0' || c > '9') {
            return std::nullopt;
        }
        value = value * 10 + (c - '0');
    }
    return negative ? -value : value;
}

// The values after a point description that [FLD] gives each opcode with one;
// nullopt for an opcode without a description.
std::optional<std::size_t> valuesAfterDescription(int opcode)
{
    switch (opcode) {
    case 2: return 3;  // X Y Z
    case 3: return 1;  // instrument height
    case 4: return 3;  // HA VA SD, and an optional azimuth
    case 6: return 3;  // HA VA SD
    case 7: return 3;  // HA VA SD
    default: return std::nullopt;
    }
}

struct Description {
    std::string_view featureCode;
    std::string_view stringNumber;
    std::string_view pointId;
    std::string_view pointName;
    std::string_view comment;

    // [FLD] 1.5: a setup or a backsight is found by its name or its ID.
    [[nodiscard]] std::string_view id() const
    {
        return pointName.empty() ? pointId : pointName;
    }
};

class FieldFileReader {
  public:
    FieldFileReader(std::string_view fileName, const ReadOptions& options)
        : builder_(fileName,
                   survey::SourceRecord{"Other", "Opcode field file", "fld", std::string(fileName),
                                        0},
                   options)
    {
    }

    Result<ReadResult> read(std::string_view bytes);

  private:
    // Whether the file's records carry the empty column after the opcode
    // (see the top of this file). Decided by a first pass over every line.
    enum class Layout { Undecided, Column, NoColumn };
    void decideLayout(std::string_view text);

    void record(int opcode, const std::vector<std::string_view>& fields, std::size_t n);
    void comment(std::string_view text);
    void units(const std::vector<std::string_view>& values, std::size_t n);
    void coordinate(const Description& d, const std::vector<std::string_view>& v, std::size_t n);
    void station(const Description& d, const std::vector<std::string_view>& v, std::size_t n);
    void measurement(int opcode, const Description& d, const std::vector<std::string_view>& v,
                     std::size_t n);
    void attribute(int opcode, const std::vector<std::string_view>& values, std::size_t n);

    std::optional<double> number(std::string_view text, std::string_view what, std::size_t n);

    RawProjectBuilder builder_;
    std::optional<katana::core::Error> fatal_;
    Layout layout_ = Layout::Undecided;
    double targetHeight_ = 0.0;
    bool targetHeightSeen_ = false;
    double distanceScale_ = 1.0;
    bool scaleWarned_ = false;
    // The point the last 02, 04, 06 or 07 was about: 41, 72 and 73 describe it.
    std::string currentPoint_;
    std::string coordinateSystem_;
    std::string zone_;
};

std::optional<double> FieldFileReader::number(std::string_view text, std::string_view what,
                                              std::size_t n)
{
    if (trimmed(text).empty()) {
        return std::nullopt;
    }
    const std::optional<double> value = parseReal(text);
    if (!value) {
        builder_.warn(n, std::string(what) + " '" + std::string(trimmed(text).substr(0, 40)) +
                             "' is not a number");
    }
    return value;
}

void FieldFileReader::comment(std::string_view text)
{
    text = trimmed(text.substr(2));
    const std::size_t colon = text.find(':');
    if (colon == std::string_view::npos) {
        return;
    }
    const std::string_view key = trimmed(text.substr(0, colon));
    const std::string_view value = trimmed(text.substr(colon + 1));
    if (key.empty() || value.empty() || key.size() > 40) {
        return;
    }
    builder_.project().metadata["header: " + std::string(key)] = std::string(value);
    if (key == "Coordinate System") {
        coordinateSystem_ = std::string(value);
    } else if (key == "Zone") {
        zone_ = std::string(value);
    }
}

void FieldFileReader::units(const std::vector<std::string_view>& values, std::size_t n)
{
    const std::string_view angle = values.empty() ? std::string_view{} : trimmed(values[0]);
    const std::string_view distance = values.size() < 2 ? std::string_view{} : trimmed(values[1]);
    if (angle != "degrees" || distance != "metres") {
        fatal_ = makeError(ErrorCode::FileImportFailure,
                           "record " + std::to_string(n) + " of " + builder_.fileName() +
                               " gives the units as angle '" + std::string(angle) +
                               "', distance '" + std::string(distance) +
                               "'; the format defines only degrees (decimal) and metres, and "
                               "Katana will not guess what another unit means",
                           std::string(kSpecification) + ", opcode 100");
        return;
    }
    builder_.countRead();
}

void FieldFileReader::coordinate(const Description& d, const std::vector<std::string_view>& v,
                                 std::size_t n)
{
    const std::optional<double> x = number(v[0], "X", n);
    const std::optional<double> y = number(v[1], "Y", n);
    const std::optional<double> z = number(v[2], "Z", n);
    if (!x || !y) {
        builder_.skip(n, "coordinate record without both X and Y");
        return;
    }
    builder_.positionPoint(d.id(), *y, *x, z, survey::CoordinateSource::Entered, n);
    builder_.codePoint(d.id(), d.featureCode, d.comment, d.stringNumber, n);
    currentPoint_ = std::string(d.id());
    builder_.countRead();
}

void FieldFileReader::station(const Description& d, const std::vector<std::string_view>& v,
                              std::size_t n)
{
    const std::optional<double> height = number(v[0], "instrument height", n);
    if (!height) {
        builder_.warn(n, "the setup states no instrument height; 0 is used");
    }
    survey::InstrumentSettings settings;
    if (distanceScale_ != 1.0) {
        settings.scaleFactor = distanceScale_;
        settings.scaleFactorState = survey::CorrectionState::NotApplied;
    }
    builder_.beginStation(d.id(), height.value_or(0.0), settings, n);
    if (!d.featureCode.empty() || !d.comment.empty()) {
        builder_.codePoint(d.id(), d.featureCode, d.comment, d.stringNumber, n);
    }
    builder_.countRead();
}

void FieldFileReader::measurement(int opcode, const Description& d,
                                  const std::vector<std::string_view>& v, std::size_t n)
{
    survey::SurveyStation* setup = builder_.currentStation();
    const char* const kind =
        opcode == 4 ? "backsight" : (opcode == 6 ? "check measurement" : "measurement");
    if (setup == nullptr) {
        builder_.skip(n, std::string(kind) + " with no station (opcode 03) before it");
        return;
    }
    const std::string target(d.id());
    if (target == setup->setup.pointId) {
        builder_.skip(n, std::string("a ") + kind + " from '" + target + "' to itself");
        return;
    }
    const std::optional<double> horizontal = number(v[0], "horizontal circle", n);
    const std::optional<double> vertical = number(v[1], "vertical circle", n);
    const std::optional<double> slope = number(v[2], "slope distance", n);

    // A plain double and a flag, not a reassigned optional: GCC 16 at -O3
    // reports the optional's payload "maybe uninitialized" (a false positive
    // that -Werror turns into a build failure).
    survey::Face face = survey::Face::Unknown;
    bool hasZenith = false;
    double zenith = 0.0;
    if (vertical) {
        const double radians = *vertical * kRadiansPerDegree;
        if (radians < 0.0 || radians >= 2.0 * kPi) {
            builder_.warn(n, "vertical circle reading outside a full circle; the zenith is not "
                             "imported");
        } else {
            face = faceOfZenithReading(radians);
            zenith = zenithInModelRange(radians);
            hasZenith = true;
        }
    }
    builder_.mentionPoint(target, n);
    if (opcode == 7 || opcode == 6) {
        builder_.codePoint(target, d.featureCode, d.comment, d.stringNumber, n);
    }
    if (opcode == 4) {
        setup->backsightPointId = target;
        if (horizontal) {
            setup->backsightAzimuth = wrapToCircle(*horizontal * kRadiansPerDegree);
        }
        // The azimuth [FLD] allows "when no coordinate for the backsight
        // point exists": kept, not applied - the reduction orients on the
        // backsight's coordinates.
        if (v.size() > 3) {
            if (const std::optional<double> azimuth = number(v[3], "backsight azimuth", n)) {
                setup->metadata["backsight azimuth (radians)"] =
                    katana::core::formatExactReal(wrapToCircle(*azimuth * kRadiansPerDegree));
            }
        }
    } else if (opcode == 6) {
        builder_.addPointMetadata(target, "check measurement", "record " + std::to_string(n));
    }

    const survey::ObservationPrecision& precision = builder_.options().precision;
    const double hi = setup->setup.instrumentHeight;
    const survey::Pointing pointing{builder_.nextPointing(), face};
    bool any = false;
    if (horizontal) {
        auto& observation = builder_.stationObservation<survey::HorizontalDirectionObservation>(n);
        observation.at = setup->setup.pointId;
        observation.to = target;
        observation.direction = wrapToCircle(*horizontal * kRadiansPerDegree);
        observation.sigma = precision.direction;
        observation.pointing = pointing;
        any = true;
    }
    if (hasZenith) {
        auto& observation = builder_.stationObservation<survey::ZenithAngleObservation>(n);
        observation.from = setup->setup.pointId;
        observation.to = target;
        observation.angle = zenith;
        observation.sigma = precision.zenith;
        observation.instrumentHeight = hi;
        observation.targetHeight = targetHeight_;
        observation.pointing = pointing;
        any = true;
    }
    if (slope) {
        const double distance = *slope * distanceScale_;
        if (!(distance > 0.0)) {
            builder_.warn(n, "slope distance of " + katana::core::formatExactReal(*slope) +
                                 " m read as no distance measured");
        } else {
            auto& observation = builder_.stationObservation<survey::DistanceObservation>(n);
            observation.from = setup->setup.pointId;
            observation.to = target;
            observation.distance = distance;
            observation.sigma = survey::distanceSigma(precision, distance);
            observation.kind = survey::DistanceKind::Slope;
            observation.instrumentHeight = hi;
            observation.targetHeight = targetHeight_;
            observation.pointing = pointing;
            any = true;
        }
    }
    currentPoint_ = target;
    if (!any) {
        builder_.skip(n, std::string(kind) + " with no readable value");
        return;
    }
    builder_.countRead();
}

void FieldFileReader::attribute(int opcode, const std::vector<std::string_view>& values,
                                std::size_t n)
{
    if (currentPoint_.empty()) {
        builder_.skip(n, (opcode == 41 ? std::string("additional text")
                                       : std::string("an attribute")) +
                             " with no point before it to describe");
        return;
    }
    if (opcode == 41) {
        std::string text;
        for (std::size_t i = 0; i < values.size(); ++i) {
            text += (i > 0 ? "\t" : "") + std::string(values[i]);
        }
        builder_.addPointMetadata(currentPoint_, "additional text " + std::to_string(n),
                                  std::string(trimmed(text)));
        builder_.countRead();
        return;
    }
    const std::string_view name = values.empty() ? std::string_view{} : trimmed(values[0]);
    if (name.empty()) {
        builder_.skip(n, "attribute with no name");
        return;
    }
    const std::string_view value = values.size() < 2 ? std::string_view{} : trimmed(values[1]);
    if (opcode == 72 && !value.empty() && !parseReal(value)) {
        builder_.warn(n, "real attribute '" + std::string(name) + "' holds '" +
                             std::string(value.substr(0, 40)) +
                             "', which is not a number; it is kept as text");
    }
    builder_.addPointMetadata(currentPoint_, std::string(name), std::string(value));
    builder_.countRead();
}

namespace {

// 1 (opcode) + 5 (description) + the values: the field count [FLD] gives a
// fixed record, without the column after the opcode. 04's azimuth is optional.
bool countFits(int opcode, std::size_t count, std::size_t columns)
{
    const std::optional<std::size_t> arity = valuesAfterDescription(opcode);
    if (!arity) {
        return false;
    }
    const std::size_t plain = 1 + 5 + *arity + columns;
    return count == plain || (opcode == 4 && count == plain + 1);
}

} // namespace

void FieldFileReader::decideLayout(std::string_view text)
{
    std::size_t column = 0;
    std::size_t noColumn = 0;
    std::size_t start = 0;
    while (start < text.size()) {
        std::size_t end = text.find('\n', start);
        if (end == std::string_view::npos) {
            end = text.size();
        }
        std::string_view line = text.substr(start, end - start);
        start = end + 1;
        if (line.ends_with('\r')) {
            line.remove_suffix(1);
        }
        const std::size_t tab = line.find('\t');
        if (tab == std::string_view::npos) {
            continue;
        }
        const std::optional<int> opcode = opcodeOf(trimmed(line.substr(0, tab)));
        if (!opcode || !valuesAfterDescription(*opcode)) {
            continue;
        }
        const std::vector<std::string_view> fields = splitTabs(line);
        if (countFits(*opcode, fields.size(), 1) && fields[1].empty()) {
            ++column;
        } else if (countFits(*opcode, fields.size(), 0) && !fields[1].empty()) {
            ++noColumn;
        }
    }
    if (column > 0 || noColumn > 0) {
        layout_ = column >= noColumn ? Layout::Column : Layout::NoColumn;
    }
    if (column > 0 && noColumn > 0) {
        builder_.warn(0, std::to_string(column) + " fixed record(s) have the empty column after "
                             "the opcode and " + std::to_string(noColumn) +
                             " do not; the file is read as " +
                             (layout_ == Layout::Column ? "having it" : "not having it") +
                             ", and a record that does not fit is skipped");
    }
}

void FieldFileReader::record(int opcode, const std::vector<std::string_view>& fields,
                             std::size_t n)
{
    if (const std::optional<std::size_t> arity = valuesAfterDescription(opcode)) {
        // A file no fixed record decided (none has an unambiguous count) is
        // read as [FLD] writes it, without the column.
        const std::size_t columns = layout_ == Layout::Column ? 1 : 0;
        if (!countFits(opcode, fields.size(), columns) || (columns == 1 && !fields[1].empty())) {
            builder_.skip(n, "opcode " + std::string(fields[0]) + " has " +
                                 std::to_string(fields.size() - 1) + " values where the format "
                                 "gives " + std::to_string(5 + *arity + columns) +
                                 (columns == 1 ? " in this file (with the empty column after the "
                                                 "opcode)"
                                               : "") +
                                 "; which value is which cannot be told");
            return;
        }
        const std::size_t offset = 1 + columns;
        const Description d{fields[offset], fields[offset + 1], fields[offset + 2],
                            fields[offset + 3], fields[offset + 4]};
        const std::vector<std::string_view> values(fields.begin() + static_cast<std::ptrdiff_t>(offset + 5),
                                                   fields.end());
        if (trimmed(d.id()).empty()) {
            builder_.skip(n, "opcode " + std::string(fields[0]) +
                                 " names its point by neither a point name nor a point ID");
            return;
        }
        switch (opcode) {
        case 2: coordinate(d, values, n); break;
        case 3: station(d, values, n); break;
        default: measurement(opcode, d, values, n); break;
        }
        // Named by its name, the point keeps its ID too - once the record
        // has made it, so its provenance is this record.
        if (!d.pointName.empty() && !d.pointId.empty() && builder_.hasPoint(d.pointName)) {
            builder_.addPointMetadata(d.pointName, "point ID", std::string(d.pointId));
        }
        return;
    }
    const std::size_t offset = fields.size() > 2 && fields[1].empty() ? 2 : 1;
    const std::vector<std::string_view> values(
        fields.begin() + static_cast<std::ptrdiff_t>(std::min(offset, fields.size())), fields.end());
    const std::string_view first = values.empty() ? std::string_view{} : trimmed(values[0]);
    switch (opcode) {
    case 100:
        units(values, n);
        return;
    case 5:
        if (const std::optional<double> height = number(first, "target height", n)) {
            targetHeight_ = *height;
            targetHeightSeen_ = true;
            builder_.countRead();
        } else {
            builder_.skip(n, "target height record with no height");
        }
        return;
    case 9:
        if (const std::optional<double> factor = number(first, "scale factor", n);
            factor && *factor > 0.0) {
            distanceScale_ = *factor;
            if (*factor != 1.0 && !scaleWarned_) {
                scaleWarned_ = true;
                builder_.warn(n, "slope distances after this record are multiplied by the "
                                 "file's scale factor " +
                                     katana::core::formatExactReal(*factor) +
                                     ", as the format says to");
            }
            builder_.countRead();
        } else {
            builder_.skip(n, "scale factor record without a positive factor");
        }
        return;
    case 29:
    case -2: {
        std::string text;
        for (std::size_t i = 0; i < values.size(); ++i) {
            text += (i > 0 ? " " : "") + std::string(trimmed(values[i]));
        }
        builder_.addStationNote(trimmed(text));
        builder_.countRead();
        return;
    }
    case 41:
    case 72:
    case 73:
        attribute(opcode, values, n);
        return;
    default:
        builder_.skip(n, "opcode " + std::string(fields[0]) + " is not one this reader imports");
        return;
    }
}

Result<ReadResult> FieldFileReader::read(std::string_view bytes)
{
    Result<katana::core::DecodedText> decoded = katana::core::decodeText(bytes);
    if (!decoded) {
        return decoded.error();
    }
    if (decoded->guessed) {
        builder_.warn(0, "the file is not UTF-8; it was read as " +
                             std::string(katana::core::toString(decoded->encoding)) +
                             ", so an accented name may be wrong");
    }
    const std::string_view text = decoded->text;
    decideLayout(text);
    std::size_t n = 0;
    std::size_t start = 0;
    while (start < text.size() && !fatal_) {
        std::size_t end = text.find('\n', start);
        if (end == std::string_view::npos) {
            end = text.size();
        }
        std::string_view line = text.substr(start, end - start);
        start = end + 1;
        ++n;
        if (line.ends_with('\r')) {
            line.remove_suffix(1);
        }
        if (trimmed(line).empty()) {
            continue;
        }
        if (line.starts_with("//")) {
            comment(line);
            continue;
        }
        if (line.starts_with('{')) {
            // "{Version 6.0}": the file's own version line.
            builder_.project().metadata["version"] = std::string(trimmed(line).substr(0, 60));
            builder_.countRead();
            continue;
        }
        const std::vector<std::string_view> fields = splitTabs(line);
        const std::optional<int> opcode = opcodeOf(trimmed(fields[0]));
        if (!opcode) {
            builder_.skip(n, "'" + std::string(trimmed(fields[0]).substr(0, 20)) +
                                 "' is not an opcode: a record opens with a number");
            continue;
        }
        record(*opcode, fields, n);
    }
    if (fatal_) {
        return *fatal_;
    }
    survey::SurveyProject& project = builder_.project();
    project.units.linear = survey::LinearUnit::Metres;
    project.units.angular = survey::AngularUnit::DecimalDegrees;
    if (!coordinateSystem_.empty()) {
        project.coordinateSystem = survey::DeclaredCoordinateSystem::named(
            zone_.empty() ? coordinateSystem_ : coordinateSystem_ + ", " + zone_);
    }
    if (!project.stations.empty()) {
        if (!targetHeightSeen_) {
            builder_.notCarried("no target heights");
        }
        builder_.notCarried("no prism constant the reduction can use: the format carries none "
                            "for the instrument, and a \"Prism constant\" attribute on a point "
                            "states no unit");
        builder_.notCarried("no atmospheric settings");
    }
    ReadResult result = builder_.finish();
    if (result.project.stations.empty() && result.project.points.empty() &&
        result.project.unpositionedPoints.empty()) {
        return makeError(ErrorCode::FileImportFailure,
                         result.project.source.fileName +
                             " holds no station, measurement or coordinate Katana could read",
                         std::to_string(result.warnings.size()) + " warning(s)" +
                             (result.warnings.empty()
                                  ? std::string{}
                                  : ", the first: " +
                                        katana::surveyio::describe(result.warnings.front())));
    }
    return result;
}

// ---- Registration -----------------------------------------------------------------

FormatSignature probe(const ProbeInput& input)
{
    const std::string_view bytes = katana::surveyio::withoutByteOrderMark(input.bytes);
    if (!katana::surveyio::looksLikeText(bytes)) {
        return katana::surveyio::ruledOut();
    }
    bool versionLine = false;
    std::size_t lines = 0;
    std::size_t records = 0;
    std::size_t measurements = 0;
    bool first = true;
    for (std::string_view line : katana::surveyio::probeLines(input, 200)) {
        if (line.ends_with('\r')) {
            line.remove_suffix(1);
        }
        if (trimmed(line).empty() || line.starts_with("//")) {
            continue;
        }
        if (first && line.starts_with("{Version ")) {
            versionLine = true;
            first = false;
            continue;
        }
        first = false;
        ++lines;
        const std::size_t tab = line.find('\t');
        if (tab == std::string_view::npos) {
            continue;
        }
        const std::optional<int> opcode = opcodeOf(line.substr(0, tab));
        if (!opcode) {
            continue;
        }
        ++records;
        if (*opcode == 2 || *opcode == 3 || *opcode == 7) {
            ++measurements;
        }
    }
    if (records == 0 || measurements == 0) {
        return katana::surveyio::ruledOut();
    }
    const double share = static_cast<double>(records) / static_cast<double>(lines);
    const std::string evidence = std::to_string(records) + " of " + std::to_string(lines) +
                                 " records open with a numeric opcode and a tab";
    const bool fld = input.extension == "fld";
    if (share < 0.9) {
        return {fld ? 0.3 : 0.1, evidence};
    }
    if (versionLine) {
        return {fld ? 0.98 : 0.9,
                std::string(fld ? "extension .fld, " : "") + "a {Version ...} line and " +
                    evidence};
    }
    return {fld ? 0.85 : 0.5, std::string(fld ? "extension .fld and " : "") + evidence};
}

FormatDescriptor descriptor()
{
    FormatDescriptor format;
    format.id = std::string(kFormatId);
    format.humanName = "Opcode field file (.fld)";
    format.manufacturer = katana::surveyio::Manufacturer::Other;
    format.reads = {.points = true,
                    .observations = true,
                    .stations = true,
                    .features = true,
                    .coordinateSystem = true,
                    .instrumentSettings = false,
                    .gnss = false};
    format.canImport = true;
    format.parserVersion = "1.0";
    format.extensions = {"fld"};
    return format;
}

Result<ReadResult> readFieldFile(std::string_view bytes, std::string_view fileName,
                                 const ReadOptions& options)
{
    FieldFileReader reader(fileName, options);
    return reader.read(bytes);
}

const katana::surveyio::FormatRegistration kRegistration{descriptor(), &probe, &readFieldFile};

} // namespace
