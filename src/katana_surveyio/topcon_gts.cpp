// Topcon GTS raw data: GTS-7 read, GTS-6 recognised and refused.
//
// Specification this reader implements, and nothing beyond it:
//   [LINK] Topcon Positioning Systems, "Topcon Link Reference Manual", P/N
//          7010-0522, Appendix C "Sample File Formats": "GTS-7 Raw Format"
//          (pages C-6 to C-9) - the record list (JOB, DATE, NAME, INST, UNITS,
//          SCALE, ATMOS, STN, XYZ, BKB, BS, FS, SS, CTL, HV, SD, HD, OFFSET,
//          NOTE), their fields and their order rules, and a worked sample.
//
// GTS-6 is NOT read. [LINK] says of it only "See the GTS-6 interface manual
// for details" and prints a sample; that manual is not public, and reading a
// field layout off one sample would be inventing it. A GTS-6 file is
// recognised so that it can be refused by name, saying what to export instead.
//
// Readings of [LINK] that are this reader's own, stated so they can be checked:
//   * Each record is a control word, white space, then comma-separated fields.
//     The sample's first line ("TTools v1.0") is not a control word; a first
//     line that is not one is kept as the project's "header" metadata.
//   * XYZ is NORTHING, easting, elevation, although the record list labels it
//     "X(easting), Y(northing)". The manual's own sample settles it: from
//     station MARK (10, 10) the backsight ST1 is written "XYZ 13.85600,
//     7.04700" and observed at circle reading -37 26' 44" (= 322 33' 16", the
//     BKB angle) and 4.902 m at zenith 97 57' 06"; its bearing from MARK is
//     322 33' only if 13.856 is the northing (atan2(-2.953, 3.856)), and
//     127 27' if it were the easting. The sideshot to ST2 (7 56' 17", 4.956 m
//     at 97 13' 46", HI 1.52, HR 1.60) reproduces "XYZ 14.87000,10.67900,
//     -0.20400" to the millimetre the same way (tests/surveyio/test_topcon_gts.cpp).
//   * The sample writes the weather record as "TEMP", the list as "ATMOS";
//     both are read. Neither states its units, so temperature and pressure
//     are kept as text in metadata and no atmospheric state is claimed.
//   * VA is a zenith angle (the sample's 97 57' 06" looks slightly down at a
//     target 0.26 m below the instrument). Readings above 180 degrees are face
//     right and are held as 360 - z (survey::Face::Right).
//   * Horizontal angles may be negative (the sample's -37.26440): they are
//     circle readings and are wrapped into [0, 360).
//   * BKB's third field ("backsight angle") is the circle reading on the
//     backsight - the sample's BS observation reads exactly it - and is the
//     setup's backsightAzimuth, which the model defines as that reading. The
//     second ("backsight bearing") is kept in the setup's metadata.
//   * HD's VD is the instrument-to-target height difference; the ground-to-
//     ground difference VD + HI - HT becomes a LevelDifferenceObservation.
//   * "F" (feet) does not say which foot. It is read as the international
//     foot, with a warning: a US survey foot file differs by 2 ppm.
//   * XYZ after STN or BKB is a known coordinate (CoordinateSource::Unknown);
//     after FS or SS it is one Topcon Link computed ([LINK]: GTS-7+ "saves
//     measured points SideShots coordinates after calculating coordinates"),
//     so Calculated.

#include <array>
#include <cmath>
#include <numbers>
#include <optional>
#include <string>
#include <string_view>

#include "katana/core/text.hpp"
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
using namespace katana::surveyio::topcon;
namespace survey = katana::survey;

constexpr std::string_view kGts7Id = "topcon-gts7";
constexpr std::string_view kGts6Id = "topcon-gts6";
constexpr double kPi = std::numbers::pi;

constexpr std::array<std::string_view, 20> kControlWords = {
    "JOB", "DATE", "NAME", "INST", "UNITS", "SCALE", "ATMOS", "TEMP", "STN",  "XYZ",
    "BKB", "BS",   "FS",   "SS",   "CTL",   "HV",    "SD",    "HD",   "NOTE", "OFFSET"};

bool isControlWord(std::string_view word)
{
    for (const std::string_view known : kControlWords) {
        if (known == word) {
            return true;
        }
    }
    return false;
}

// Up to 8 comma-separated fields; no record in [LINK] has more than four.
struct Fields {
    std::array<std::string_view, 8> values{};
    std::size_t count = 0;
    bool overflow = false;

    [[nodiscard]] std::optional<std::string_view> at(std::size_t i) const
    {
        if (i < count && !values[i].empty()) {
            return values[i];
        }
        return std::nullopt;
    }
};

Fields splitFields(std::string_view text)
{
    Fields fields;
    std::size_t start = 0;
    while (true) {
        const std::size_t cut = text.find(',', start);
        const std::string_view value =
            trimmed(text.substr(start, cut == std::string_view::npos ? text.npos : cut - start));
        if (fields.count == fields.values.size()) {
            fields.overflow = true;
        } else {
            fields.values[fields.count++] = value;
        }
        if (cut == std::string_view::npos) {
            break;
        }
        start = cut + 1;
    }
    return fields;
}

// The point the next XYZ record gives coordinates to.
enum class Subject { None, Station, Backsight, Target };

class Gts7Reader {
  public:
    Gts7Reader(std::string_view fileName, const ReadOptions& options)
        : builder_(fileName,
                   survey::SourceRecord{"Topcon", "GTS-7", "Link 7010-0522", std::string(fileName),
                                        0},
                   options)
    {
    }

    Result<ReadResult> read(std::string_view bytes);

  private:
    void record(std::string_view word, const Fields& f, std::size_t n);
    void units(const Fields& f, std::size_t n);
    void station(const Fields& f, std::size_t n);
    void coordinates(const Fields& f, std::size_t n);
    void backsight(const Fields& f, std::size_t n);
    void header(std::string_view word, const Fields& f, std::size_t n);
    void measurement(std::string_view word, const Fields& f, std::size_t n);
    void offset(const Fields& f, std::size_t n);

    std::optional<double> linear(std::optional<std::string_view> value, std::string_view what,
                                 std::size_t n);
    std::optional<double> angle(std::optional<std::string_view> value, std::string_view what,
                                std::size_t n);
    bool unitsKnown(std::size_t n);

    RawProjectBuilder builder_;
    std::optional<katana::core::Error> fatal_;
    bool unitsSeen_ = false;
    double metresPerUnit_ = 1.0;
    bool gons_ = false;
    survey::InstrumentSettings settings_;
    std::string atmosphere_; // "temperature, pressure" as written, units unstated
    bool atmosphereSeen_ = false;
    bool anyTargetHeight_ = false;

    Subject subject_ = Subject::None;
    std::string subjectPoint_;
    // The shot header (BS / FS / SS) the next HV, SD or HD belongs to.
    bool headerOpen_ = false;
    std::string target_;
    double targetHeight_ = 0.0;
    std::string lastObserved_;
};

bool Gts7Reader::unitsKnown(std::size_t n)
{
    if (!unitsSeen_ && !fatal_) {
        fatal_ = makeError(ErrorCode::FileImportFailure,
                           "record " + std::to_string(n) + " of " + builder_.fileName() +
                               " gives a measurement before any UNITS record has said whether "
                               "the file is in metres or feet and degrees or gons, and Katana "
                               "will not guess",
                           "GTS-7 [LINK] C-6: UNITS Meter/Feet, Degree/Gon");
    }
    return unitsSeen_;
}

std::optional<double> Gts7Reader::linear(std::optional<std::string_view> value,
                                         std::string_view what, std::size_t n)
{
    if (!value || !unitsKnown(n)) {
        return std::nullopt;
    }
    const std::optional<double> parsed = parseReal(*value);
    if (!parsed) {
        builder_.warn(n, std::string(what) + " '" + std::string(*value) + "' is not a number");
        return std::nullopt;
    }
    return *parsed * metresPerUnit_;
}

std::optional<double> Gts7Reader::angle(std::optional<std::string_view> value,
                                        std::string_view what, std::size_t n)
{
    if (!value || !unitsKnown(n)) {
        return std::nullopt;
    }
    const std::optional<double> parsed =
        gons_ ? gonsToRadians(*value) : packedDegreesToRadians(*value);
    if (!parsed) {
        builder_.warn(n, std::string(what) + " '" + std::string(*value) + "' is not an angle in " +
                             (gons_ ? "gons" : "degrees (DDD.MMSS)"));
    }
    return parsed;
}

Result<ReadResult> Gts7Reader::read(std::string_view bytes)
{
    std::size_t n = 0;
    std::size_t start = bytes.starts_with("\xEF\xBB\xBF") ? 3 : 0;
    bool first = true;
    while (start < bytes.size() && !fatal_) {
        std::size_t end = bytes.find('\n', start);
        if (end == std::string_view::npos) {
            end = bytes.size();
        }
        std::string_view line = bytes.substr(start, end - start);
        start = end + 1;
        ++n;
        const std::string_view text = trimmed(line);
        if (text.empty()) {
            continue;
        }
        std::size_t space = 0;
        while (space < text.size() && !katana::core::isAsciiSpace(text[space])) {
            ++space;
        }
        const std::string_view word = text.substr(0, space);
        if (!isControlWord(word)) {
            if (first) {
                builder_.project().metadata["header"] = std::string(text.substr(0, 200));
                builder_.countRead();
            } else {
                builder_.skip(n, "'" + std::string(word.substr(0, 20)) +
                                     "' is not a GTS-7 record (the Topcon Link manual lists JOB, "
                                     "DATE, NAME, INST, UNITS, SCALE, ATMOS, STN, XYZ, BKB, BS, "
                                     "FS, SS, CTL, HV, SD, HD, OFFSET and NOTE)");
            }
            first = false;
            continue;
        }
        first = false;
        if (word == "NOTE") {
            // A comment is free text: its commas are not field separators.
            builder_.addStationNote(trimmed(text.substr(space)));
            builder_.countRead();
            continue;
        }
        const Fields fields = splitFields(trimmed(text.substr(space)));
        if (fields.overflow) {
            builder_.warn(n, "record has more fields than any GTS-7 record defines; the extra "
                             "fields were not read");
        }
        record(word, fields, n);
    }
    if (fatal_) {
        return *fatal_;
    }
    if (!builder_.project().stations.empty()) {
        if (!anyTargetHeight_) {
            builder_.notCarried("no target heights");
        }
        builder_.notCarried("no prism constant: the GTS-7 format has no field for one");
        builder_.notCarried(atmosphereSeen_
                                ? "atmospheric correction unknown: the file gives temperature "
                                  "and pressure without units and does not say whether the "
                                  "instrument applied a ppm correction"
                                : "no atmospheric settings");
    }
    ReadResult result = builder_.finish();
    const survey::SurveyProject& project = result.project;
    if (project.stations.empty() && project.observations.empty() && project.points.empty() &&
        project.unpositionedPoints.empty()) {
        return makeError(ErrorCode::FileImportFailure,
                         project.source.fileName +
                             " holds no GTS-7 setup, observation or point Katana could read",
                         std::to_string(result.warnings.size()) + " warning(s)" +
                             (result.warnings.empty()
                                  ? std::string{}
                                  : ", the first: " +
                                        katana::surveyio::describe(result.warnings.front())));
    }
    return result;
}

void Gts7Reader::record(std::string_view word, const Fields& f, std::size_t n)
{
    survey::SurveyProject& project = builder_.project();
    if (word == "JOB") {
        // The sample's job name is a path ("C:\Download\777.raw"): only the
        // name part is kept, for the reason given at survey::sourceFileName.
        if (const std::optional<std::string_view> name = f.at(0)) {
            project.name = survey::sourceFileName(*name);
        }
        if (const std::optional<std::string_view> description = f.at(1)) {
            project.metadata["job description"] = std::string(*description);
        }
        builder_.countRead();
    } else if (word == "DATE") {
        // Day and month order is not documented; kept as written.
        std::string date(f.at(0).value_or(std::string_view{}));
        if (const std::optional<std::string_view> time = f.at(1)) {
            date += " " + std::string(*time);
        }
        project.metadata["date"] = date;
        builder_.countRead();
    } else if (word == "NAME") {
        project.metadata["surveyor"] = std::string(f.at(0).value_or(std::string_view{}));
        builder_.countRead();
    } else if (word == "INST") {
        settings_.model = std::string(f.at(0).value_or(std::string_view{}));
        builder_.countRead();
    } else if (word == "UNITS") {
        units(f, n);
    } else if (word == "SCALE") {
        // "grid factor, scale factor, elevation": what the collector would
        // apply when it computes coordinates, not what is in a raw reading.
        project.metadata["grid factor"] = std::string(f.at(0).value_or(std::string_view{}));
        project.metadata["scale factor"] = std::string(f.at(1).value_or(std::string_view{}));
        project.metadata["scale elevation"] = std::string(f.at(2).value_or(std::string_view{}));
        builder_.countRead();
    } else if (word == "ATMOS" || word == "TEMP") {
        atmosphere_ = std::string(f.at(0).value_or(std::string_view{})) + ", " +
                      std::string(f.at(1).value_or(std::string_view{}));
        atmosphereSeen_ = true;
        if (survey::SurveyStation* station = builder_.currentStation();
            station && station->observations.empty()) {
            station->metadata["temperature, pressure (units not stated)"] = atmosphere_;
        }
        builder_.countRead();
    } else if (word == "STN") {
        station(f, n);
    } else if (word == "XYZ") {
        coordinates(f, n);
    } else if (word == "BKB") {
        backsight(f, n);
    } else if (word == "BS" || word == "FS" || word == "SS") {
        header(word, f, n);
    } else if (word == "CTL") {
        if (!headerOpen_) {
            builder_.skip(n, "CTL record with no BS, FS or SS header before it");
            return;
        }
        builder_.addPointMetadata(target_, "control code",
                                  std::string(f.at(0).value_or(std::string_view{})));
        if (const std::optional<std::string_view> code = f.at(1)) {
            builder_.addPointMetadata(target_, "second code", std::string(*code));
        }
        if (const std::optional<std::string_view> string = f.at(2)) {
            builder_.addPointMetadata(target_, "second string number", std::string(*string));
        }
        builder_.countRead();
    } else if (word == "HV" || word == "SD" || word == "HD") {
        measurement(word, f, n);
    } else if (word == "OFFSET") {
        offset(f, n);
    }
}

void Gts7Reader::units(const Fields& f, std::size_t n)
{
    const std::string_view length = f.at(0).value_or(std::string_view{});
    const std::string_view angular = f.at(1).value_or(std::string_view{});
    survey::DeclaredUnits declared;
    if (length == "M") {
        metresPerUnit_ = 1.0;
        declared.linear = survey::LinearUnit::Metres;
    } else if (length == "F") {
        metresPerUnit_ = kMetresPerFoot;
        declared.linear = survey::LinearUnit::Feet;
        builder_.warn(n, "the file is in feet and the GTS-7 format does not say which foot; it "
                         "is read as the international foot (0.3048 m) - a file in US survey "
                         "feet would be 2 ppm different, which matters for large coordinates");
    } else {
        fatal_ = makeError(ErrorCode::FileImportFailure,
                           "record " + std::to_string(n) + " of " + builder_.fileName() +
                               " gives the length unit as '" + std::string(length) +
                               "'; the GTS-7 format defines M (metres) and F (feet)",
                           "GTS-7 [LINK] C-6: UNITS Meter/Feet, Degree/Gon");
        return;
    }
    if (angular == "D") {
        gons_ = false;
        declared.angular = survey::AngularUnit::DegreesMinutesSeconds;
    } else if (angular == "G") {
        gons_ = true;
        declared.angular = survey::AngularUnit::Gons;
    } else {
        fatal_ = makeError(ErrorCode::FileImportFailure,
                           "record " + std::to_string(n) + " of " + builder_.fileName() +
                               " gives the angle unit as '" + std::string(angular) +
                               "'; the GTS-7 format defines D (degrees) and G (gons)",
                           "GTS-7 [LINK] C-6: UNITS Meter/Feet, Degree/Gon");
        return;
    }
    if (unitsSeen_ && builder_.project().units != declared) {
        builder_.warn(n, "the units change here; values after this record are converted with the "
                         "new units");
    }
    if (!unitsSeen_) {
        builder_.project().units = declared;
    }
    unitsSeen_ = true;
    builder_.countRead();
}

void Gts7Reader::station(const Fields& f, std::size_t n)
{
    const std::optional<std::string_view> point = f.at(0);
    if (!point) {
        builder_.skip(n, "STN record names no point");
        return;
    }
    const std::optional<double> height = linear(f.at(1), "instrument height", n);
    if (fatal_) {
        return;
    }
    if (!height) {
        builder_.warn(n, "the setup states no instrument height; 0 is used");
    }
    survey::SurveyStation& setup = builder_.beginStation(*point, height.value_or(0.0), settings_, n);
    if (atmosphereSeen_) {
        setup.metadata["temperature, pressure (units not stated)"] = atmosphere_;
    }
    if (const std::optional<std::string_view> code = f.at(2)) {
        builder_.codePoint(*point, *code, {}, {}, n);
    }
    subject_ = Subject::Station;
    subjectPoint_ = std::string(*point);
    headerOpen_ = false;
    builder_.countRead();
}

void Gts7Reader::coordinates(const Fields& f, std::size_t n)
{
    if (subject_ == Subject::None) {
        builder_.skip(n, "XYZ record with no STN, BKB, FS or SS before it to say whose it is");
        return;
    }
    const std::optional<double> northing = linear(f.at(0), "northing", n);
    const std::optional<double> easting = linear(f.at(1), "easting", n);
    const std::optional<double> elevation = linear(f.at(2), "elevation", n);
    if (fatal_) {
        return;
    }
    if (!northing || !easting) {
        builder_.skip(n, "XYZ record without both northing and easting");
        return;
    }
    builder_.positionPoint(subjectPoint_, *northing, *easting, elevation,
                           subject_ == Subject::Target ? survey::CoordinateSource::Calculated
                                                       : survey::CoordinateSource::Unknown,
                           n);
    builder_.countRead();
}

void Gts7Reader::backsight(const Fields& f, std::size_t n)
{
    survey::SurveyStation* setup = builder_.currentStation();
    const std::optional<std::string_view> point = f.at(0);
    if (!setup || !point) {
        builder_.skip(n, "BKB record with no STN before it, or naming no point");
        return;
    }
    const std::optional<double> bearing = angle(f.at(1), "backsight bearing", n);
    const std::optional<double> circle = angle(f.at(2), "backsight angle", n);
    if (fatal_) {
        return;
    }
    if (*point == setup->setup.pointId) {
        builder_.skip(n, "the backsight is the occupied point");
        return;
    }
    builder_.mentionPoint(*point, n);
    setup->backsightPointId = std::string(*point);
    if (circle) {
        setup->backsightAzimuth = wrapToCircle(*circle);
    }
    if (bearing) {
        setup->metadata["backsight bearing (radians)"] =
            katana::core::formatExactReal(wrapToCircle(*bearing));
    }
    subject_ = Subject::Backsight;
    subjectPoint_ = std::string(*point);
    builder_.countRead();
}

void Gts7Reader::header(std::string_view word, const Fields& f, std::size_t n)
{
    const std::optional<std::string_view> point = f.at(0);
    survey::SurveyStation* setup = builder_.currentStation();
    if (!point || !setup) {
        headerOpen_ = false;
        builder_.skip(n, std::string(word) + " record with no STN before it, or naming no point");
        return;
    }
    if (*point == setup->setup.pointId) {
        headerOpen_ = false;
        builder_.skip(n, "a shot from '" + std::string(*point) + "' to itself");
        return;
    }
    // "BS ptno[,target height]": a backsight without one keeps the last.
    if (const std::optional<double> height = linear(f.at(1), "target height", n)) {
        targetHeight_ = *height;
        anyTargetHeight_ = true;
    }
    if (fatal_) {
        return;
    }
    builder_.mentionPoint(*point, n);
    target_ = std::string(*point);
    headerOpen_ = true;
    if (word != "BS") {
        const std::string_view code = f.at(2).value_or(std::string_view{});
        const std::string_view string = f.at(3).value_or(std::string_view{});
        if (!code.empty()) {
            builder_.codePoint(*point, code, {}, string, n);
        }
        if (word == "FS") {
            builder_.addPointMetadata(*point, "traverse foresight", "record " + std::to_string(n));
        }
    } else if (setup->backsightPointId.empty()) {
        setup->backsightPointId = target_;
    }
    subject_ = Subject::Target;
    subjectPoint_ = target_;
    builder_.countRead();
}

void Gts7Reader::measurement(std::string_view word, const Fields& f, std::size_t n)
{
    if (!headerOpen_) {
        builder_.skip(n, std::string(word) +
                             " record with no BS, FS or SS header before it to say what was "
                             "observed");
        return;
    }
    survey::SurveyStation* setup = builder_.currentStation();
    const std::optional<double> horizontal = angle(f.at(0), "horizontal angle", n);
    std::optional<double> zenith;
    std::optional<double> slope;
    std::optional<double> flat;
    std::optional<double> rise;
    if (word == "HD") {
        flat = linear(f.at(1), "horizontal distance", n);
        rise = linear(f.at(2), "vertical distance", n);
    } else {
        zenith = angle(f.at(1), "vertical angle", n);
        if (word == "SD") {
            slope = linear(f.at(2), "slope distance", n);
        }
    }
    if (fatal_) {
        return;
    }
    const survey::ObservationPrecision& precision = builder_.options().precision;
    const std::string& at = setup->setup.pointId;
    const double hi = setup->setup.instrumentHeight;
    const double th = targetHeight_;

    survey::Face face = survey::Face::Unknown;
    if (zenith) {
        if (*zenith < 0.0 || *zenith >= 2.0 * kPi) {
            builder_.warn(n, "zenith reading outside a full circle; the zenith is not imported");
            zenith.reset();
        } else {
            face = faceOfZenithReading(*zenith);
            zenith = zenithInModelRange(*zenith);
        }
    }
    const survey::Pointing pointing{builder_.nextPointing(), face};
    bool any = false;
    if (horizontal) {
        auto& observation = builder_.stationObservation<survey::HorizontalDirectionObservation>(n);
        observation.at = at;
        observation.to = target_;
        observation.direction = wrapToCircle(*horizontal);
        observation.sigma = precision.direction;
        observation.pointing = pointing;
        any = true;
    }
    if (zenith) {
        auto& observation = builder_.stationObservation<survey::ZenithAngleObservation>(n);
        observation.from = at;
        observation.to = target_;
        observation.angle = *zenith;
        observation.sigma = precision.zenith;
        observation.instrumentHeight = hi;
        observation.targetHeight = th;
        observation.pointing = pointing;
        any = true;
    }
    const auto distance = [&](double value, survey::DistanceKind kind) {
        if (!(value > 0.0)) {
            builder_.warn(n, "distance of " + katana::core::formatExactReal(value) +
                                 " m read as no distance measured");
            return;
        }
        auto& observation = builder_.stationObservation<survey::DistanceObservation>(n);
        observation.from = at;
        observation.to = target_;
        observation.distance = value;
        observation.sigma = survey::distanceSigma(precision, value);
        observation.kind = kind;
        observation.instrumentHeight = hi;
        observation.targetHeight = th;
        observation.pointing = pointing;
    };
    if (slope) {
        distance(*slope, survey::DistanceKind::Slope);
        any = true;
    }
    if (flat) {
        distance(*flat, survey::DistanceKind::Horizontal);
        any = true;
    }
    if (rise) {
        const double heightDifference = *rise + hi - th;
        if (std::isfinite(heightDifference)) {
            auto& difference = builder_.stationObservation<survey::LevelDifferenceObservation>(n);
            difference.from = at;
            difference.to = target_;
            difference.heightDifference = heightDifference;
            difference.sigma = std::hypot(precision.heightMeasurement, precision.heightMeasurement);
            difference.length = flat.value_or(0.0) > 0.0 ? *flat : 0.0;
            any = true;
        } else {
            builder_.warn(n, "the vertical distance and heights do not add up to a finite "
                             "height difference; it is not imported");
        }
    }
    if (!any) {
        builder_.skip(n, std::string(word) + " record with no readable value");
        return;
    }
    lastObserved_ = target_;
    builder_.countRead();
}

void Gts7Reader::offset(const Fields& f, std::size_t n)
{
    if (lastObserved_.empty()) {
        builder_.skip(n, "OFFSET record with no SD or HD record before it");
        return;
    }
    const std::optional<double> radial = linear(f.at(0), "radial offset", n);
    const std::optional<double> tangential = linear(f.at(1), "tangential offset", n);
    const std::optional<double> vertical = linear(f.at(2), "vertical offset", n);
    if (fatal_) {
        return;
    }
    const auto text = [](const std::optional<double>& value) {
        return value ? katana::core::formatExactReal(*value) : std::string("-");
    };
    builder_.addPointMetadata(lastObserved_, "offset radial, tangential, vertical (m)",
                              text(radial) + ", " + text(tangential) + ", " + text(vertical));
    builder_.warn(n, "the offsets to '" + lastObserved_ +
                         "' are kept in its metadata but not applied: the model holds the "
                         "observation to the target, and moving the point is the reduction's "
                         "work");
    builder_.countRead();
}

// ---- GTS-7 registration -------------------------------------------------------------

FormatSignature probeGts7(const ProbeInput& input)
{
    const std::string_view bytes = katana::surveyio::withoutByteOrderMark(input.bytes);
    if (!katana::surveyio::looksLikeText(bytes)) {
        return katana::surveyio::ruledOut();
    }
    std::size_t lines = 0;
    std::size_t records = 0;
    std::size_t measurements = 0;
    bool first = true;
    for (const std::string_view line : katana::surveyio::probeLines(input, 200)) {
        const std::string_view text = trimmed(line);
        if (text.empty()) {
            continue;
        }
        std::size_t space = 0;
        while (space < text.size() && !katana::core::isAsciiSpace(text[space])) {
            ++space;
        }
        const std::string_view word = text.substr(0, space);
        // A control word is followed by white space ("STN           MARK,..."),
        // which is what tells "SS,OP1,..." (RW5) from "SS  1,1.6,TREE".
        const bool spaced = space < text.size();
        if (isControlWord(word) && spaced) {
            ++records;
            ++lines;
            if (word == "STN" || word == "SD" || word == "HV" || word == "HD") {
                ++measurements;
            }
        } else if (!first) {
            ++lines;
        }
        first = false;
    }
    if (records == 0 || measurements == 0) {
        return katana::surveyio::ruledOut();
    }
    const double share = static_cast<double>(records) / static_cast<double>(lines);
    const std::string evidence = std::to_string(records) + " of " + std::to_string(lines) +
                                 " lines start with a GTS-7 control word (STN, BS, SS, SD ...)";
    if (share < 0.8) {
        return {0.3, evidence};
    }
    const std::string_view ext = input.extension;
    if (ext == "gt7" || ext == "gts7") {
        return {0.97, "extension ." + std::string(ext) + " and " + evidence};
    }
    return {0.9, evidence};
}

FormatDescriptor gts7Descriptor()
{
    FormatDescriptor format;
    format.id = std::string(kGts7Id);
    format.humanName = "Topcon GTS-7 raw";
    format.manufacturer = katana::surveyio::Manufacturer::Topcon;
    format.reads = {.points = true,
                    .observations = true,
                    .stations = true,
                    .features = true,
                    .coordinateSystem = false,
                    .instrumentSettings = true,
                    .gnss = false};
    format.canImport = true;
    format.parserVersion = "1.0";
    // [LINK]: "GTS-7 Raw (*.raw; *.dat; *.gts; *.gts7; *.gt7)".
    format.extensions = {"gt7", "gts7", "gts", "raw", "dat"};
    return format;
}

Result<ReadResult> readGts7(std::string_view bytes, std::string_view fileName,
                            const ReadOptions& options)
{
    Gts7Reader reader(fileName, options);
    return reader.read(bytes);
}

const katana::surveyio::FormatRegistration kGts7Registration{gts7Descriptor(), &probeGts7,
                                                             &readGts7};

// ---- GTS-6: recognised, refused ----------------------------------------------------

// The words of a GTS-6 file as [LINK]'s sample shows them: "_'" opens a
// station, "_+" a point, "_(" and "_*" a code, and "_ ?+" / "_ W+" a
// measurement - all joined by underscores. Enough to recognise the file; far
// from enough to read it.
FormatSignature probeGts6(const ProbeInput& input)
{
    const std::string_view bytes = katana::surveyio::withoutByteOrderMark(input.bytes);
    if (!katana::surveyio::looksLikeText(bytes)) {
        return katana::surveyio::ruledOut();
    }
    const std::string_view start = trimmed(bytes.substr(0, 64));
    if (!(start.starts_with("_'") || start.starts_with("_+"))) {
        return katana::surveyio::ruledOut();
    }
    std::size_t words = 0;
    for (std::size_t at = bytes.find('_'); at != std::string_view::npos && words < 1000;
         at = bytes.find('_', at + 1)) {
        ++words;
    }
    const bool measurement = bytes.find("?+") != std::string_view::npos ||
                             bytes.find(" W+") != std::string_view::npos;
    if (!measurement || words < 4) {
        return {0.3, "starts with a GTS-6 word but holds no GTS-6 measurement"};
    }
    return {0.9, "underscore-separated GTS-6 words (_' station, _+ point, ?+ measurement)"};
}

FormatDescriptor gts6Descriptor()
{
    FormatDescriptor format;
    format.id = std::string(kGts6Id);
    format.humanName = "Topcon GTS-6 raw (recognised, not readable)";
    format.manufacturer = katana::surveyio::Manufacturer::Topcon;
    format.reads = {}; // nothing: see readGts6
    format.canImport = true;
    format.parserVersion = "1.0";
    format.extensions = {"raw", "dat", "gts"};
    return format;
}

Result<ReadResult> readGts6(std::string_view, std::string_view fileName, const ReadOptions&)
{
    return makeError(ErrorCode::Unsupported,
                     std::string(fileName) +
                         " is Topcon GTS-6 raw data, whose record layout Topcon publishes only "
                         "in the GTS-6 interface manual, which is not public; Katana does not "
                         "read a layout it would have to guess. Export the job as GTS-7 raw "
                         "(Topcon Link or Topcon Tools: Export, 'GTS-7 Raw') or as RW5, and "
                         "import that",
                     "Topcon Link Reference Manual 7010-0522, C-5: \"See the GTS-6 interface "
                     "manual for details\"");
}

const katana::surveyio::FormatRegistration kGts6Registration{gts6Descriptor(), &probeGts6,
                                                             &readGts6};

} // namespace
