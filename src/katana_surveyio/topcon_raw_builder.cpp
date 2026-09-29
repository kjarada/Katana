#include "topcon_raw_builder.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <numbers>
#include <utility>

#include "katana/core/text.hpp"
#include "katana/math/numerics.hpp"

namespace katana::surveyio::topcon {

namespace survey = katana::survey;

namespace {

constexpr double kTwoPi = 2.0 * std::numbers::pi;
constexpr double kRadiansPerArcSecond = std::numbers::pi / 648000.0;

// 10^n for the decimals of a second; exact in binary for n <= 22.
constexpr std::array<double, 16> kPowersOfTen = {1e0, 1e1, 1e2,  1e3,  1e4,  1e5,  1e6,  1e7,
                                                 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15};

bool isDigit(char c)
{
    return c >= '0' && c <= '9';
}

} // namespace

std::optional<double> parseReal(std::string_view text) noexcept
{
    return katana::core::parseFiniteDouble(katana::core::trimmed(text));
}

std::optional<double> packedDegreesToRadians(std::string_view text) noexcept
{
    text = katana::core::trimmed(text);
    bool negative = false;
    if (!text.empty() && (text.front() == '+' || text.front() == '-')) {
        negative = text.front() == '-';
        text.remove_prefix(1);
    }
    const std::size_t dot = text.find('.');
    const std::string_view whole = text.substr(0, dot);
    const std::string_view fraction =
        dot == std::string_view::npos ? std::string_view{} : text.substr(dot + 1);
    if (whole.empty() && fraction.empty()) {
        return std::nullopt;
    }
    // Degrees beyond a few thousand are not an angle anyone recorded; the cap
    // keeps the integer arithmetic below far from overflow.
    if (whole.size() > 6) {
        return std::nullopt;
    }
    std::int64_t degrees = 0;
    for (const char c : whole) {
        if (!isDigit(c)) {
            return std::nullopt;
        }
        degrees = degrees * 10 + (c - '0');
    }
    for (const char c : fraction) {
        if (!isDigit(c)) {
            return std::nullopt;
        }
    }
    const auto digit = [&](std::size_t i) { return i < fraction.size() ? fraction[i] - '0' : 0; };
    const int minutes = digit(0) * 10 + digit(1);
    const int seconds = digit(2) * 10 + digit(3);
    if (minutes >= 60 || seconds >= 60) {
        return std::nullopt;
    }
    // Decimals of a second: digits 5 onwards, read as one integer over a power
    // of ten. More than 15 of them is more precision than a double carries.
    double decimals = 0.0;
    if (fraction.size() > 4) {
        const std::string_view tail = fraction.substr(4, 15);
        std::int64_t value = 0;
        for (const char c : tail) {
            value = value * 10 + (c - '0');
        }
        decimals = static_cast<double>(value) / kPowersOfTen[tail.size()];
    }
    const double arcSeconds =
        (static_cast<double>(degrees) * 60.0 + minutes) * 60.0 + seconds + decimals;
    const double radians = arcSeconds * kRadiansPerArcSecond;
    return negative ? -radians : radians;
}

std::optional<double> gonsToRadians(std::string_view text) noexcept
{
    const std::optional<double> gons = parseReal(text);
    if (!gons) {
        return std::nullopt;
    }
    return *gons * (std::numbers::pi / 200.0);
}

double wrapToCircle(double radians) noexcept
{
    double wrapped = std::fmod(radians, kTwoPi);
    if (wrapped < 0.0) {
        wrapped += kTwoPi;
    }
    // fmod of a value a hair below a whole turn can round up to exactly 2*pi.
    return wrapped >= kTwoPi ? 0.0 : wrapped;
}

survey::Face faceOfZenithReading(double zenithRadians) noexcept
{
    if (zenithRadians > std::numbers::pi) {
        return survey::Face::Right;
    }
    if (zenithRadians > 0.0 && zenithRadians < std::numbers::pi) {
        return survey::Face::Left;
    }
    return survey::Face::Unknown;
}

double zenithInModelRange(double zenithRadians) noexcept
{
    return zenithRadians > std::numbers::pi ? kTwoPi - zenithRadians : zenithRadians;
}

std::optional<ShotReading> offsetShot(const ShotReading& measured, double radial, double tangential,
                                      double height) noexcept
{
    const double along = measured.slope * std::sin(measured.zenith) + radial;
    const double plan = std::hypot(along, tangential);
    const double rise = measured.slope * std::cos(measured.zenith) + height;
    const double slope = std::hypot(plan, rise);
    if (!(slope >= katana::math::tolerance::kCoordinate)) {
        return std::nullopt;
    }
    return ShotReading{wrapToCircle(measured.circle + std::atan2(tangential, along)),
                       std::atan2(plan, rise), slope};
}

// ---- RawProjectBuilder --------------------------------------------------------

RawProjectBuilder::RawProjectBuilder(std::string_view fileName, survey::SourceRecord source,
                                     const ReadOptions& options)
    : fileName_(fileName), sourceTemplate_(std::move(source)), options_(options)
{
    sourceTemplate_.fileName = fileName_;
    sourceTemplate_.recordNumber = 0;
    result_.project.source = sourceTemplate_;
}

survey::SourceRecord RawProjectBuilder::source(std::size_t record) const
{
    survey::SourceRecord copy = sourceTemplate_;
    copy.recordNumber = record;
    return copy;
}

void RawProjectBuilder::warn(std::size_t record, std::string message)
{
    if (result_.warnings.size() < kListedWarnings) {
        result_.warnings.push_back(ReadWarning{fileName_, record, std::move(message)});
        return;
    }
    if (unlistedWarnings_++ == 0) {
        firstUnlistedRecord_ = record;
    }
}

void RawProjectBuilder::skip(std::size_t record, std::string message)
{
    warn(record, std::move(message));
    ++result_.recordsSkipped;
}

void RawProjectBuilder::notCarried(std::string text)
{
    for (const std::string& already : result_.notCarried) {
        if (already == text) {
            return;
        }
    }
    result_.notCarried.push_back(std::move(text));
}

std::uint32_t RawProjectBuilder::findPoint(std::string_view id, std::size_t hash) const
{
    if (pointSlots_.empty()) {
        return kNotFound;
    }
    const std::size_t mask = pointSlots_.size() - 1;
    for (std::size_t slot = hash & mask;; slot = (slot + 1) & mask) {
        const std::uint32_t occupant = pointSlots_[slot];
        if (occupant == 0) {
            return kNotFound;
        }
        const PointEntry& point = points_[occupant - 1];
        if (point.hash == hash && point.id == id) {
            return occupant - 1;
        }
    }
}

void RawProjectBuilder::placePoint(std::uint32_t index)
{
    const std::size_t mask = pointSlots_.size() - 1;
    std::size_t slot = points_[index].hash & mask;
    while (pointSlots_[slot] != 0) {
        slot = (slot + 1) & mask;
    }
    pointSlots_[slot] = index + 1;
}

RawProjectBuilder::PointEntry& RawProjectBuilder::entry(std::string_view id, std::size_t record)
{
    // A shot names its target, then codes it, describes it or notes it: the
    // same point several times in a row, which needs no hashing.
    if (lastPoint_ != kNotFound && points_[lastPoint_].id == id) {
        return points_[lastPoint_];
    }
    const std::size_t hash = std::hash<std::string_view>{}(id);
    if (const std::uint32_t found = findPoint(id, hash); found != kNotFound) {
        lastPoint_ = found;
        return points_[found];
    }
    if ((points_.size() + 1) * 2 > pointSlots_.size()) {
        pointSlots_.assign(std::max<std::size_t>(1024, pointSlots_.size() * 2), 0);
        for (std::uint32_t index = 0; index < points_.size(); ++index) {
            placePoint(index);
        }
    }
    const auto index = static_cast<std::uint32_t>(points_.size());
    PointEntry& created = points_.emplace_back();
    created.id = std::string(id);
    created.hash = hash;
    created.sourceRecord = record;
    placePoint(index);
    lastPoint_ = index;
    return created;
}

void RawProjectBuilder::mentionPoint(std::string_view id, std::size_t record)
{
    (void)entry(id, record);
}

bool RawProjectBuilder::hasPoint(std::string_view id) const
{
    return findPoint(id, std::hash<std::string_view>{}(id)) != kNotFound;
}

void RawProjectBuilder::positionPoint(std::string_view id, double northing, double easting,
                                      std::optional<double> elevation,
                                      survey::CoordinateSource how, std::size_t record)
{
    PointEntry& point = entry(id, record);
    if (!point.positioned) {
        point.positioned = true;
        point.northing = northing;
        point.easting = easting;
        point.elevation = elevation;
        point.coordinateSource = how;
        point.positionRecord = record;
        point.sourceRecord = record;
        return;
    }
    if (point.northing == northing && point.easting == easting &&
        (!elevation || point.elevation == elevation)) {
        return;
    }
    const auto text = [](double n, double e, const std::optional<double>& z) {
        std::string written =
            "N " + katana::core::formatExactReal(n) + ", E " + katana::core::formatExactReal(e);
        if (z) {
            written += ", elevation " + katana::core::formatExactReal(*z);
        }
        return written;
    };
    const std::string restated = text(northing, easting, elevation);
    const std::size_t earlier = point.positionRecord;
    if (restated_ == RestatedCoordinates::LatestKept) {
        point.metadata["coordinates of record " + std::to_string(earlier) + ", superseded"] =
            text(point.northing, point.easting, point.elevation);
        point.northing = northing;
        point.easting = easting;
        point.elevation = elevation;
        point.coordinateSource = how;
        point.positionRecord = record;
        point.sourceRecord = record;
        warn(record, "point '" + point.id + "' is given different coordinates here (" +
                         restated + " m) from those of record " + std::to_string(earlier) +
                         "; these, the latest, are kept, and those are in the point's metadata");
        return;
    }
    point.metadata["coordinates restated at record " + std::to_string(record)] = restated;
    warn(record, "point '" + point.id + "' is given different coordinates here (" + restated +
                     " m) from those of record " + std::to_string(earlier) +
                     "; the first are kept and these are in the point's metadata");
}

std::string RawProjectBuilder::featureKey(std::string_view code, std::string_view stringNumber)
{
    std::string key(code);
    key += '\x1f';
    key += stringNumber;
    return key;
}

void RawProjectBuilder::codePoint(std::string_view id, std::string_view code,
                                  std::string_view description, std::string_view stringNumber,
                                  std::size_t record)
{
    PointEntry& point = entry(id, record);
    if (point.code.empty()) {
        point.code = std::string(code);
    }
    if (point.description.empty()) {
        point.description = std::string(description);
    }
    if (code.empty()) {
        return;
    }
    // A feature is one code (and string number, where the format has one),
    // strung in the order its points were observed.
    std::string key = featureKey(code, stringNumber);
    std::uint32_t featureId = 0;
    if (const auto found = featureIndex_.find(key); found != featureIndex_.end()) {
        featureId = found->second;
    } else {
        featureId = static_cast<std::uint32_t>(result_.project.features.size());
        survey::SurveyFeature& feature = result_.project.features.emplace_back();
        feature.code = std::string(code);
        feature.name = std::string(stringNumber);
        feature.source = source(record);
        featureIndex_.emplace(std::move(key), featureId);
    }
    // A point shot again for the same string (both faces of a set, a check)
    // is still one vertex of it.
    if (point.feature == featureId) {
        return;
    }
    if (point.feature == kNoFeature) {
        point.feature = featureId;
    }
    result_.project.features[featureId].pointIds.push_back(point.id);
}

std::optional<std::size_t> RawProjectBuilder::closeFeature(std::string_view code,
                                                           std::string_view stringNumber)
{
    const auto found = featureIndex_.find(featureKey(code, stringNumber));
    if (found == featureIndex_.end()) {
        return std::nullopt;
    }
    survey::SurveyFeature& feature = result_.project.features[found->second];
    feature.closed = true;
    featureIndex_.erase(found);
    return feature.pointIds.size();
}

std::optional<std::size_t> RawProjectBuilder::closeFeatureOf(std::string_view id)
{
    const std::uint32_t index = findPoint(id, std::hash<std::string_view>{}(id));
    if (index == kNotFound || points_[index].feature == kNoFeature) {
        return std::nullopt;
    }
    // Open features are exactly those in featureIndex_: one that is not
    // closed is the one its code and string number find.
    const survey::SurveyFeature& feature = result_.project.features[points_[index].feature];
    if (feature.closed) {
        return std::nullopt;
    }
    return closeFeature(feature.code, feature.name);
}

std::optional<std::string> RawProjectBuilder::lastPointOf(std::string_view code,
                                                          std::string_view stringNumber) const
{
    const auto found = featureIndex_.find(featureKey(code, stringNumber));
    if (found == featureIndex_.end()) {
        return std::nullopt;
    }
    const survey::SurveyFeature& feature = result_.project.features[found->second];
    if (feature.pointIds.empty()) {
        return std::nullopt;
    }
    return feature.pointIds.back();
}

void RawProjectBuilder::addPointMetadata(std::string_view id, std::string key, std::string value)
{
    PointEntry& point = entry(id, 0);
    point.metadata[std::move(key)] = std::move(value);
}

survey::SurveyStation& RawProjectBuilder::beginStation(std::string_view pointId,
                                                       double instrumentHeight,
                                                       const survey::InstrumentSettings& settings,
                                                       std::size_t record)
{
    mentionPoint(pointId, record);
    // The n-th occupation of a point is "<point> (n)". Counted per point, so
    // a file that occupies one point a hundred thousand times costs a
    // hundred thousand steps, not the square of it; the loop only runs on
    // when the file itself uses such a name for another setup.
    std::uint32_t& occupations = occupations_[std::string(pointId)];
    std::string id(pointId);
    if (occupations > 0 || stationIndex_.contains(id)) {
        do {
            id = std::string(pointId) + " (" + std::to_string(++occupations) + ")";
        } while (stationIndex_.contains(id));
    } else {
        ++occupations;
    }
    stationIndex_.emplace(id, static_cast<std::uint32_t>(result_.project.stations.size()));
    // Setups of one job tend to be alike: room for as many observations as
    // the last one had saves re-growing a vector of 576-byte variants.
    const std::size_t expected =
        result_.project.stations.empty() ? 0 : result_.project.stations.back().observations.size();
    survey::SurveyStation& station = result_.project.stations.emplace_back();
    station.observations.reserve(expected);
    station.setup.id = std::move(id);
    station.setup.pointId = std::string(pointId);
    station.setup.instrumentHeight = instrumentHeight;
    station.instrument = settings;
    station.source = source(record);
    pointingCounters_.push_back(0);
    return station;
}

survey::SurveyStation* RawProjectBuilder::currentStation()
{
    return result_.project.stations.empty() ? nullptr : &result_.project.stations.back();
}

std::size_t RawProjectBuilder::nextPointing()
{
    return pointingCounters_.empty() ? 0 : ++pointingCounters_.back();
}

void RawProjectBuilder::addLooseObservation(survey::Observation observation)
{
    result_.project.observations.push_back(std::move(observation));
}

void RawProjectBuilder::addStationNote(std::string_view note)
{
    std::map<std::string, std::string>& metadata = result_.project.stations.empty()
                                                       ? result_.project.metadata
                                                       : result_.project.stations.back().metadata;
    std::string& notes = metadata["notes"];
    if (!notes.empty()) {
        notes += '\n';
    }
    notes += note;
}

ReadResult RawProjectBuilder::finish()
{
    if (unlistedWarnings_ > 0) {
        result_.warnings.push_back(ReadWarning{
            fileName_, 0,
            std::to_string(unlistedWarnings_) + " more warnings, from record " +
                std::to_string(firstUnlistedRecord_) + " on, are not listed: only the first " +
                std::to_string(kListedWarnings) + " are. The records they are about are "
                "still counted as read or skipped"});
    }
    survey::SurveyProject& project = result_.project;
    std::size_t positioned = 0;
    for (const PointEntry& point : points_) {
        positioned += point.positioned ? 1 : 0;
    }
    project.points.reserve(project.points.size() + positioned);
    project.unpositionedPoints.reserve(project.unpositionedPoints.size() + points_.size() -
                                       positioned);
    for (PointEntry& point : points_) {
        if (point.positioned) {
            survey::SurveyPoint& out = project.points.emplace_back();
            out.id = std::move(point.id);
            out.northing = point.northing;
            out.easting = point.easting;
            out.elevation = point.elevation;
            out.code = std::move(point.code);
            out.description = std::move(point.description);
            out.metadata = std::move(point.metadata);
            out.coordinateSource = point.coordinateSource;
            stampSource(out.source, point.sourceRecord);
        } else {
            survey::UnpositionedPoint& out = project.unpositionedPoints.emplace_back();
            out.id = std::move(point.id);
            out.code = std::move(point.code);
            out.description = std::move(point.description);
            out.metadata = std::move(point.metadata);
            stampSource(out.source, point.sourceRecord);
        }
    }
    points_.clear();
    pointSlots_.clear();
    lastPoint_ = kNotFound;
    return std::move(result_);
}

} // namespace katana::surveyio::topcon
