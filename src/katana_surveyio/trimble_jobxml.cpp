// Trimble JobXML (.jxl) reader.
//
// SPECIFICATION: Trimble, "Job XML Schema Definition Version 5.72"
// (JobXMLSchema-5.72.xsd, published at
// https://ww2.trimble.com/schema/JobXML/5_7/JobXMLSchema-5.72.xsd). Element
// names, units and the meaning of every flag below are taken from that
// document; where a comment says "the schema says", it is quoting it.
//
// What a JobXML file is. A <JOBFile> root holding three sections:
//   <FieldBook>   every record the controller wrote, in the order it wrote
//                 them: instrument, atmosphere, station, backsight, target and
//                 point records, each with an ID that later records refer to.
//                 The schema says the raw terrestrial observations in it
//                 (circleType) have had NO corrections applied: no atmospheric
//                 ppm (unless the AtmosphereRecord's ApplyPPMToRawDistances is
//                 false), no prism constant, no curvature or refraction, no
//                 orientation. That is what the correction states below say.
//   <Reductions>  the controller's own "best" grid coordinates for each point.
//                 This reader does NOT use them as positions - the reduction
//                 in katana::survey computes positions from the raw
//                 observations, and a point that already had the controller's
//                 answer would be held rather than computed. They are kept in
//                 each point's metadata ("trimble.reductions.*", the file's
//                 own digits) so a report can compare the two.
//   <Environment> the units and coordinate system current at export. Kept as
//                 project metadata; the coordinate system names become the
//                 DeclaredCoordinateSystem (declared, never transformed).
//
// UNITS. The schema says every angle, latitude and longitude is in decimal
// degrees and every distance and height in metres, whatever the job's display
// units were; temperatures are degrees Celsius and pressures hPa. So nothing
// here depends on the UnitsRecord, and DeclaredUnits says metres and decimal
// degrees. Degrees become radians on the way in.
//
// FACES. Face1 is face left, Face2 face right. A VerticalCircle reading is
// the zenith-referenced circle reading Trimble instruments record: about 90
// degrees on face 1 and about 270 on face 2. survey::ZenithAngleObservation
// holds a zenith ANGLE in [0, pi], so a reading r above 180 degrees is stored
// as 360 - r, the zenith angle it measures; the Pointing keeps the face, and
// the difference between that and the face-1 reading is the index error the
// reduction's face averaging removes. Horizontal circle readings are stored
// as read (HorizontalDirectionObservation), face 2 included.
//
// DELETED RECORDS. JobXML keeps records the surveyor deleted, flagged
// <Deleted>true</Deleted>. They are not imported, and one warning names how
// many there were and which.
//
// WHAT IS NOT READ is said, never dropped quietly: record kinds this reader
// does not interpret are counted per kind in a warning; elements inside a
// record it does interpret, but that the survey model has no place for, are
// listed per element group in a warning with the number of records carrying
// them; configuration records and the Environment go to project metadata.
//
// The file is read as a stream (xml_pull.hpp): a 200 MB job is never held as
// a tree. Each FieldBook record is gathered into a small reusable buffer of
// (path, value) pairs, interpreted, and the buffer reused.

#include <algorithm>
#include <array>
#include <charconv>
#include <cmath>
#include <cstdint>
#include <map>
#include <numbers>
#include <optional>
#include <set>
#include <string>
#include <string_view>
#include <unordered_map>
#include <utility>
#include <vector>

#include "katana/surveyio/detect.hpp"
#include "katana/surveyio/format.hpp"
#include "xml_pull.hpp"

namespace katana::surveyio {
namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
namespace survey = katana::survey;
using detail::XmlPullReader;

constexpr std::string_view kFormatId = "trimble-jobxml";
constexpr std::string_view kHumanName = "Trimble JobXML";
constexpr double kRadiansPerDegree = std::numbers::pi / 180.0;
constexpr double kTwoPi = 2.0 * std::numbers::pi;

// ---- Small text helpers -------------------------------------------------------

bool isSpace(char c)
{
    return c == ' ' || c == '\t' || c == '\r' || c == '\n';
}

std::string_view trimmed(std::string_view text)
{
    while (!text.empty() && isSpace(text.front())) {
        text.remove_prefix(1);
    }
    while (!text.empty() && isSpace(text.back())) {
        text.remove_suffix(1);
    }
    return text;
}

// A decimal number as XML Schema writes a double: optional sign, digits,
// optional fraction and exponent. std::from_chars does not take a leading
// '+', which xsd:double allows, so it is stepped over here.
std::optional<double> parseNumber(std::string_view text)
{
    text = trimmed(text);
    if (!text.empty() && text.front() == '+') {
        text.remove_prefix(1);
    }
    if (text.empty()) {
        return std::nullopt;
    }
    double value = 0.0;
    const auto [end, ec] = std::from_chars(text.data(), text.data() + text.size(), value);
    if (ec != std::errc{} || end != text.data() + text.size() || !std::isfinite(value)) {
        return std::nullopt;
    }
    return value;
}

bool positiveFinite(double value)
{
    return std::isfinite(value) && value > 0.0;
}

// An angle in degrees as a circle reading in [0, 360). Almost every reading
// already is one, and fmod is the slowest step of a shot, so it is skipped
// for those.
double circleDegrees(double degrees)
{
    if (degrees >= 0.0 && degrees < 360.0) {
        return degrees;
    }
    degrees = std::fmod(degrees, 360.0);
    if (degrees < 0.0) {
        degrees += 360.0;
    }
    return degrees >= 360.0 ? 0.0 : degrees;
}

double wrapTwoPi(double angle)
{
    if (angle >= 0.0 && angle < kTwoPi) {
        return angle;
    }
    angle = std::fmod(angle, kTwoPi);
    if (angle < 0.0) {
        angle += kTwoPi;
    }
    // fmod of a value a hair under 2*pi can round back up to it.
    return angle >= kTwoPi ? 0.0 : angle;
}

// "2024-03-05T10:15:30.5", optionally followed by 'Z' or a +hh:mm offset.
// A timestamp with no zone is the controller's clock; which clock the file
// does not say, so the time system is left empty rather than guessed.
survey::SurveyTimestamp parseTimestamp(std::string_view text)
{
    survey::SurveyTimestamp time;
    text = trimmed(text);
    auto integer = [](std::string_view digits, int& out) {
        const auto [end, ec] = std::from_chars(digits.data(), digits.data() + digits.size(), out);
        return ec == std::errc{} && end == digits.data() + digits.size();
    };
    if (text.size() < 19 || text[4] != '-' || text[7] != '-' || text[10] != 'T' ||
        text[13] != ':' || text[16] != ':') {
        return time;
    }
    int year = 0;
    int month = 0;
    int day = 0;
    int hour = 0;
    int minute = 0;
    if (!integer(text.substr(0, 4), year) || !integer(text.substr(5, 2), month) ||
        !integer(text.substr(8, 2), day) || !integer(text.substr(11, 2), hour) ||
        !integer(text.substr(14, 2), minute)) {
        return time;
    }
    std::size_t secondsEnd = 17;
    while (secondsEnd < text.size() &&
           ((text[secondsEnd] >= '0' && text[secondsEnd] <= '9') || text[secondsEnd] == '.')) {
        ++secondsEnd;
    }
    const std::optional<double> second = parseNumber(text.substr(17, secondsEnd - 17));
    if (!second || month < 1 || month > 12 || day < 1 || day > 31 || hour > 23 || minute > 59 ||
        *second >= 61.0) {
        return time;
    }
    time.year = year;
    time.month = month;
    time.day = day;
    time.hour = hour;
    time.minute = minute;
    time.second = *second;
    const std::string_view zone = text.substr(secondsEnd);
    if (zone == "Z") {
        time.timeSystem = "UTC";
    } else if (!zone.empty()) {
        time.timeSystem = "UTC" + std::string(zone);
    }
    return time;
}

// ---- One record, gathered ---------------------------------------------------------

// The leaf values of one element (a FieldBook record, a Reductions point, the
// Environment), as (path relative to the element, decoded text) pairs in
// document order. Attributes of nested elements are kept as "path@name".
// Reused from record to record, so after the first few records it allocates
// nothing - and most values are not copied at all: a value that is one run of
// text with no entity in it is a view into the document, which outlives every
// record, and so is the path of a direct child, which is its element name.
// Decoded values and the paths of nested elements are copied into an arena.
class RecordBuffer {
  public:
    // Where a path or value lives: `document` bytes when not null, otherwise
    // arena_ from `offset` - an offset, not a view, because the arena moves
    // as it grows.
    struct Span {
        const char* document;
        std::uint32_t offset;
        std::uint32_t size;
    };
    struct Field {
        Span path;
        Span value;
        bool used;
    };

    void reset(std::size_t line)
    {
        arena_.clear();
        fields_.clear();
        path_.clear();
        depth_ = 0;
        overflow_ = 0;
        pending_.clear();
        pendingMode_ = Pending::None;
        line_ = line;
    }

    // `element` must be a view into the document (XmlPullReader::name()).
    void open(std::string_view element)
    {
        if (depth_ > 0) {
            hadChild_[depth_ - 1] = true;
        }
        // The pull reader refuses nesting deeper than core::kXmlMaxDepth
        // from the root, so a record (which starts below the root) never
        // reaches the end of these arrays; the check keeps that true if the
        // reader's limit and this one ever part.
        if (depth_ >= depthStarts_.size()) {
            ++overflow_;
            return;
        }
        depthStarts_[depth_] = static_cast<std::uint32_t>(path_.size());
        hadChild_[depth_] = false;
        names_[depth_] = element;
        ++depth_;
        if (!path_.empty()) {
            path_.push_back('/');
        }
        path_.append(element);
        pending_.clear();
        pendingMode_ = Pending::None;
    }

    // Attribute of the element just opened.
    void attribute(std::string_view name, std::string_view decoded)
    {
        const auto pathBegin = static_cast<std::uint32_t>(arena_.size());
        arena_.append(path_);
        arena_.push_back('@');
        arena_.append(name);
        const auto valueBegin = static_cast<std::uint32_t>(arena_.size());
        arena_.append(decoded);
        fields_.push_back(Field{Span{nullptr, pathBegin, valueBegin - pathBegin},
                                Span{nullptr, valueBegin,
                                     static_cast<std::uint32_t>(arena_.size() - valueBegin)},
                                false});
    }

    // A run of character data in the element open now; `raw` must be a view
    // into the document. Entities are decoded (and refused, see xml_pull.hpp)
    // only when the reader saw a reference in it. False when one cannot be
    // decoded, and decodeError() says why - a bool, not a Status, because
    // this runs for every piece of text in the file and a Status carries an
    // Error even when all is well.
    bool addText(std::string_view raw, bool hasReference)
    {
        const bool plain = !hasReference;
        if (pendingMode_ == Pending::None && plain) {
            pendingView_ = raw;
            pendingMode_ = Pending::View;
            return true;
        }
        if (pendingMode_ == Pending::View) {
            pending_.assign(pendingView_);
        }
        pendingMode_ = Pending::Copied;
        if (plain) {
            pending_.append(raw);
            return true;
        }
        if (katana::core::Status status = detail::appendXmlText(raw, pending_); !status.ok()) {
            decodeError_ = status.error();
            return false;
        }
        return true;
    }

    [[nodiscard]] const katana::core::Error& decodeError() const { return decodeError_; }

    void close()
    {
        if (overflow_ > 0) {
            --overflow_;
            return;
        }
        if (depth_ == 0) {
            return;
        }
        --depth_;
        if (!hadChild_[depth_]) {
            Field field{};
            if (depth_ == 0) {
                field.path = Span{names_[0].data(), 0, static_cast<std::uint32_t>(names_[0].size())};
            } else {
                field.path = Span{nullptr, static_cast<std::uint32_t>(arena_.size()),
                                  static_cast<std::uint32_t>(path_.size())};
                arena_.append(path_);
            }
            if (pendingMode_ == Pending::View) {
                const std::string_view value = trimmed(pendingView_);
                field.value = Span{value.data(), 0, static_cast<std::uint32_t>(value.size())};
            } else if (pendingMode_ == Pending::Copied) {
                const std::string_view value = trimmed(pending_);
                field.value = Span{nullptr, static_cast<std::uint32_t>(arena_.size()),
                                   static_cast<std::uint32_t>(value.size())};
                arena_.append(value);
            }
            fields_.push_back(field);
        }
        path_.resize(depthStarts_[depth_]);
        pending_.clear();
        pendingMode_ = Pending::None;
    }

    [[nodiscard]] std::size_t line() const { return line_; }
    [[nodiscard]] std::size_t size() const { return fields_.size(); }
    [[nodiscard]] std::string_view path(std::size_t i) const { return resolve(fields_[i].path); }
    [[nodiscard]] std::string_view value(std::size_t i) const { return resolve(fields_[i].value); }
    [[nodiscard]] bool used(std::size_t i) const { return fields_[i].used; }
    void markUsed(std::size_t i) { fields_[i].used = true; }
    void markAllUsed()
    {
        for (Field& field : fields_) {
            field.used = true;
        }
    }

    // The first field at `wanted`, marked as read; nullopt when there is none.
    std::optional<std::string_view> take(std::string_view wanted)
    {
        for (Field& field : fields_) {
            if (field.path.size == wanted.size() && detail::sameText(resolve(field.path), wanted)) {
                field.used = true;
                return resolve(field.value);
            }
        }
        return std::nullopt;
    }

    // True when any field lies under `prefix` ("Circle" finds "Circle/...").
    [[nodiscard]] bool has(std::string_view prefix) const
    {
        for (const Field& field : fields_) {
            if (field.path.size < prefix.size()) {
                continue;
            }
            const std::string_view p = resolve(field.path);
            if (p.starts_with(prefix) && (p.size() == prefix.size() || p[prefix.size()] == '/')) {
                return true;
            }
        }
        return false;
    }

  private:
    enum class Pending { None, View, Copied };

    [[nodiscard]] std::string_view resolve(const Span& span) const
    {
        return span.document != nullptr ? std::string_view(span.document, span.size)
                                        : std::string_view(arena_).substr(span.offset, span.size);
    }

    std::string arena_;
    std::vector<Field> fields_;
    std::string path_;
    std::array<std::uint32_t, katana::core::kXmlMaxDepth> depthStarts_{};
    std::array<bool, katana::core::kXmlMaxDepth> hadChild_{};
    std::array<std::string_view, katana::core::kXmlMaxDepth> names_{};
    std::size_t depth_ = 0;
    std::size_t overflow_ = 0; // elements opened past the arrays' end, never expected
    std::string_view pendingView_{};
    std::string pending_;
    Pending pendingMode_ = Pending::None;
    katana::core::Error decodeError_{};
    std::size_t line_ = 0;
};

// ---- What earlier records said, by record ID ----------------------------------------

struct InstrumentRecord {
    std::string type;
    std::string model;
    std::string serial;
    std::string firmware;
};

struct AtmosphereRecord {
    std::optional<double> pressure;
    std::optional<double> temperature;
    std::optional<double> ppm;
    std::optional<double> refractionCoefficient;
    std::optional<bool> applyCurvature;
    std::optional<bool> applyRefraction;
    std::optional<bool> applyPpmToRawDistances;
};

struct TargetRecord {
    std::optional<double> prismConstant;
    std::optional<double> targetHeight;
    std::string prismType;
};

struct AntennaRecord {
    std::optional<double> measuredHeight;
    std::optional<double> reducedHeight;
    std::string measurementType;
    std::string equipmentId;
};

struct EquipmentRecord {
    std::string antennaType;
    std::string antennaSerial;
    std::string measurementMethod;
};

struct ReferenceRecord {
    std::string pointName;
    std::string antennaId;
};

struct StationState {
    std::size_t index = 0; // into project.stations
    std::size_t nextPointing = 1;
    bool heightKnown = false;
    bool atmosphereDifferenceWarned = false;
    bool reorientationWarned = false;
    bool backsightSet = false;
};

// Hashed, and looked up by string_view without building a string: a large
// job looks a setup, a target and a point up by name for every shot.
struct NameHash {
    using is_transparent = void;
    std::size_t operator()(std::string_view text) const noexcept
    {
        return std::hash<std::string_view>{}(text);
    }
};
template <typename T> using ById = std::unordered_map<std::string, T, NameHash, std::equal_to<>>;

constexpr std::size_t kNone = static_cast<std::size_t>(-1);

// Where a point name stands: its coordinate record (if any), its named-only
// entry (if any - made for an observation before or without coordinates),
// and whether a point record has described it yet.
struct PointSlot {
    std::size_t positioned = kNone;
    std::size_t named = kNone;
    bool control = false;   // the kept coordinate record is a control one
    bool described = false; // its method, survey method and class are in its metadata
};

// ---- The reader ------------------------------------------------------------------

class JobXmlReader {
  public:
    JobXmlReader(std::string_view bytes, std::string_view fileName, const ReadOptions& options)
        : bytes_(bytes), fileName_(fileName), options_(options), xml_(bytes), lines_(bytes)
    {
    }

    Result<ReadResult> read();

  private:
    // ---- plumbing
    void warn(std::size_t line, std::string message)
    {
        result_.warnings.push_back(ReadWarning{fileName_, line, std::move(message)});
    }
    survey::SourceRecord sourceAt(std::size_t line) const
    {
        survey::SourceRecord source = baseSource_;
        source.recordNumber = line;
        return source;
    }
    std::optional<double> number(RecordBuffer& record, std::string_view path);
    std::optional<bool> flag(RecordBuffer& record, std::string_view path);
    std::string text(RecordBuffer& record, std::string_view path)
    {
        return std::string(trimmed(record.take(path).value_or(std::string_view{})));
    }
    void noteUnread(std::string_view recordName, RecordBuffer& record);

    Result<bool> gather(RecordBuffer& record, std::size_t elementDepth);

    // ---- records
    void readFieldBookRecord(std::string_view name, std::string_view id,
                             std::string_view timeStamp, RecordBuffer& record);
    void readPointRecord(std::string_view id, RecordBuffer& record);
    void readPointRecordBody(std::string_view id, RecordBuffer& record, bool fresh);
    void keepOrRefuse(std::vector<survey::Observation>& list, bool plainlyValid, std::size_t line);
    void readStationRecord(std::string_view id, std::string_view timeStamp, RecordBuffer& record);
    void readBackBearingRecord(std::string_view id, RecordBuffer& record);
    void readLineRecord(std::string_view id, RecordBuffer& record);
    void readReductionsPoint(RecordBuffer& record);
    void readEnvironment(RecordBuffer& record);
    void keepAsMetadata(std::string_view prefix, RecordBuffer& record);

    // ---- points
    PointSlot* slotOf(std::string_view name);
    PointSlot& slotFor(std::string_view name);
    void ensureNamed(std::string_view name, std::size_t line);
    survey::UnpositionedPoint* named(std::string_view name);
    survey::SurveyPoint* positioned(std::string_view name);
    std::map<std::string, std::string>* metadataOf(std::string_view name);
    void addPositioned(survey::SurveyPoint point, bool control, std::string_view recordId);
    void addObservation(survey::SurveyStation* station, survey::Observation observation,
                        std::size_t line);
    survey::GnssAntenna antennaFor(std::string_view antennaId) const;

    void finish();

    std::string_view bytes_;
    std::string fileName_;
    const ReadOptions& options_;
    XmlPullReader xml_;
    detail::LineCounter lines_;
    ReadResult result_;
    survey::SourceRecord baseSource_;

    ById<InstrumentRecord> instruments_;
    ById<AtmosphereRecord> atmospheres_;
    ById<TargetRecord> targets_;
    ById<AntennaRecord> antennas_;
    ById<EquipmentRecord> equipment_;
    ById<ReferenceRecord> references_;
    ById<StationState> stations_;
    ById<std::string> backBearingStations_; // BackBearingRecord ID -> StationRecord ID
    ById<std::size_t> occupations_; // station name -> setups on it

    std::vector<survey::SurveyPoint> positioned_;
    std::vector<survey::UnpositionedPoint> named_;
    ById<PointSlot> points_;
    std::size_t lastSetupShots_ = 0; // observations on the previous setup, a capacity hint
    PointSlot* touched_ = nullptr;    // the slot the point record being read named or positioned
    std::string lastStationId_;       // the setup and target the previous shot named
    StationState* lastStation_ = nullptr;
    std::string lastTargetId_;
    const TargetRecord* lastTarget_ = nullptr;

    // What was not read, for the warnings at the end.
    std::map<std::string, std::size_t, std::less<>> skippedKinds_;
    struct Unread {
        std::size_t records = 0;
        std::size_t lastSerial = 0;
        std::set<std::string, std::less<>> leaves;
    };
    std::map<std::string, Unread, std::less<>> unread_;
    std::string unreadKey_;
    std::size_t recordSerial_ = 0;
    std::vector<std::string> deletedIds_;
    std::size_t deletedCount_ = 0;
    std::size_t meanTurnedAngles_ = 0;
    std::size_t keyedGlobal_ = 0;
    std::size_t reductionsUnknown_ = 0;
    std::size_t setupsWithoutAtmosphere_ = 0;
    std::size_t shotsWithoutTargetHeight_ = 0;
};

std::optional<double> JobXmlReader::number(RecordBuffer& record, std::string_view path)
{
    const std::optional<std::string_view> found = record.take(path);
    if (!found || trimmed(*found).empty()) {
        return std::nullopt; // absent, or the schema's "null" (an empty element)
    }
    const std::optional<double> value = parseNumber(*found);
    if (!value) {
        warn(record.line(), std::string(path) + " '" + std::string(found->substr(0, 40)) +
                                "' is not a number and was read as absent");
    }
    return value;
}

std::optional<bool> JobXmlReader::flag(RecordBuffer& record, std::string_view path)
{
    const std::optional<std::string_view> found = record.take(path);
    if (!found) {
        return std::nullopt;
    }
    const std::string_view value = trimmed(*found);
    if (value == "true") {
        return true;
    }
    if (value == "false") {
        return false;
    }
    if (!value.empty()) {
        warn(record.line(), std::string(path) + " '" + std::string(value.substr(0, 20)) +
                                "' is neither true nor false and was read as absent");
    }
    return std::nullopt;
}

void JobXmlReader::noteUnread(std::string_view recordName, RecordBuffer& record)
{
    ++recordSerial_;
    for (std::size_t i = 0; i < record.size(); ++i) {
        if (record.used(i)) {
            continue;
        }
        const std::string_view path = record.path(i);
        const std::size_t slash = path.find('/');
        unreadKey_.assign(recordName);
        unreadKey_.push_back(' ');
        unreadKey_.append(path.substr(0, slash));
        auto found = unread_.find(unreadKey_);
        if (found == unread_.end()) {
            found = unread_.emplace(unreadKey_, Unread{}).first;
        }
        Unread& entry = found->second;
        if (entry.lastSerial != recordSerial_) {
            entry.lastSerial = recordSerial_;
            ++entry.records;
        }
        if (slash != std::string_view::npos && entry.leaves.size() < 12) {
            const std::string_view leaf = path.substr(path.rfind('/') + 1);
            if (!entry.leaves.contains(leaf)) {
                entry.leaves.emplace(leaf);
            }
        }
    }
}

// Reads the element just started (at `elementDepth`) to its end into
// `record`. False (not an error) never happens; an error is the XML's.
Result<bool> JobXmlReader::gather(RecordBuffer& record, std::size_t elementDepth)
{
    for (;;) {
        switch (xml_.next()) {
        case XmlPullReader::Event::StartElement: {
            record.open(xml_.name());
            if (!xml_.hasAttributes()) {
                break;
            }
            // The schema puts attributes below record level on two elements
            // only - Feature@Name and InformationGroup@name - and both are
            // gathered, so a feature's name is read like any other value.
            for (std::string_view attributeName : {std::string_view("Name"), std::string_view("name")}) {
                std::string_view raw;
                if (!xml_.attribute(attributeName, raw)) {
                    continue;
                }
                std::string decoded;
                if (katana::core::Status status = detail::appendXmlText(raw, decoded);
                    !status.ok()) {
                    return makeError(ErrorCode::FileImportFailure,
                                     fileName_ + " line " +
                                         std::to_string(lines_.lineAt(xml_.offset())) + ": " +
                                         status.error().message,
                                     status.error().context);
                }
                record.attribute(attributeName, decoded);
            }
            break;
        }
        case XmlPullReader::Event::Text:
            if (!record.addText(xml_.text(), xml_.textHasReference())) {
                return makeError(ErrorCode::FileImportFailure,
                                 fileName_ + " line " +
                                     std::to_string(lines_.lineAt(xml_.offset())) + ": " +
                                     record.decodeError().message,
                                 record.decodeError().context);
            }
            break;
        case XmlPullReader::Event::EndElement:
            if (xml_.depth() == elementDepth) {
                return true;
            }
            record.close();
            break;
        case XmlPullReader::Event::EndOfDocument:
            return makeError(ErrorCode::FileImportFailure,
                             fileName_ + " ends inside a record, so it is incomplete");
        case XmlPullReader::Event::Error:
            return makeError(ErrorCode::FileImportFailure,
                             fileName_ + ": " + xml_.error().message, xml_.error().context);
        }
    }
}

// ---- Points ---------------------------------------------------------------------------

PointSlot* JobXmlReader::slotOf(std::string_view name)
{
    const auto found = points_.find(name);
    return found == points_.end() ? nullptr : &found->second;
}

PointSlot& JobXmlReader::slotFor(std::string_view name)
{
    if (const auto found = points_.find(name); found != points_.end()) {
        return found->second;
    }
    return points_.emplace(std::string(name), PointSlot{}).first->second;
}

survey::UnpositionedPoint* JobXmlReader::named(std::string_view name)
{
    const PointSlot* slot = slotOf(name);
    return slot == nullptr || slot->named == kNone ? nullptr : &named_[slot->named];
}

survey::SurveyPoint* JobXmlReader::positioned(std::string_view name)
{
    const PointSlot* slot = slotOf(name);
    return slot == nullptr || slot->positioned == kNone ? nullptr : &positioned_[slot->positioned];
}

std::map<std::string, std::string>* JobXmlReader::metadataOf(std::string_view name)
{
    if (survey::SurveyPoint* point = positioned(name)) {
        return &point->metadata;
    }
    if (survey::UnpositionedPoint* point = named(name)) {
        return &point->metadata;
    }
    return nullptr;
}

void JobXmlReader::ensureNamed(std::string_view name, std::size_t line)
{
    if (name.empty()) {
        return;
    }
    PointSlot& slot = slotFor(name);
    if (slot.positioned != kNone || slot.named != kNone) {
        return;
    }
    slot.named = named_.size();
    survey::UnpositionedPoint& point = named_.emplace_back();
    point.id = name;
    point.source = sourceAt(line);
}

void JobXmlReader::addPositioned(survey::SurveyPoint point, bool control, std::string_view recordId)
{
    PointSlot& slot = slotFor(point.id);
    if (slot.positioned == kNone) {
        slot.positioned = positioned_.size();
        slot.control = control;
        positioned_.push_back(std::move(point));
        return;
    }
    // Two coordinate records for one name. Trimble's own search rules prefer a
    // control point to any other and otherwise the first stored; the one not
    // kept is named in the warning, coordinates and all, so it is not lost.
    survey::SurveyPoint& kept = positioned_[slot.positioned];
    const bool replace = control && !slot.control;
    const survey::SurveyPoint& dropped = replace ? kept : point;
    std::string message = "point " + point.id + " has coordinates in more than one record; kept " +
                          (replace ? std::string("the control record ") + std::string(recordId)
                                   : std::string("the first")) +
                          " and not the one at N " + std::to_string(dropped.northing) + " E " +
                          std::to_string(dropped.easting);
    if (dropped.elevation) {
        message += " H " + std::to_string(*dropped.elevation);
    }
    warn(point.source.recordNumber, std::move(message));
    if (replace) {
        kept = std::move(point);
        slot.control = true;
    }
}

void JobXmlReader::addObservation(survey::SurveyStation* station, survey::Observation observation,
                                  std::size_t line)
{
    // A value the survey model would refuse (validateProject) is a warning
    // about this record, not a reason to lose the whole file.
    if (katana::core::Status status = survey::validateObservation(observation); !status.ok()) {
        warn(line, survey::observationKindName(observation) + " not imported: " +
                       status.error().message);
        ++result_.recordsSkipped;
        return;
    }
    if (station != nullptr) {
        station->observations.push_back(std::move(observation));
    } else {
        result_.project.observations.push_back(std::move(observation));
    }
}

survey::GnssAntenna JobXmlReader::antennaFor(std::string_view antennaId) const
{
    survey::GnssAntenna antenna;
    const auto found = antennas_.find(antennaId);
    if (found == antennas_.end()) {
        return antenna;
    }
    const AntennaRecord& record = found->second;
    antenna.height = record.measuredHeight.value_or(0.0);
    std::string method;
    if (const auto gear = equipment_.find(record.equipmentId); gear != equipment_.end()) {
        antenna.type = gear->second.antennaType;
        antenna.serialNumber = gear->second.antennaSerial;
        method = gear->second.measurementMethod;
    }
    antenna.measuredTo = method;
    if (record.reducedHeight) {
        // ReducedHeight is the controller's vertical height from the mark to
        // the antenna phase centre - measured against a real export: the APC
        // of an ECEF position lies exactly ReducedHeight above the mark the
        // controller reduced it to (0.0000 m horizontally, 0.0000 m in
        // height). It is what a reduction needs, whatever the words for the
        // measuring point were, so it is carried as a phase-centre height and
        // the measured one is kept in the words.
        antenna.height = *record.reducedHeight;
        antenna.method = survey::AntennaHeightMethod::PhaseCentre;
        antenna.measuredTo = (method.empty() ? std::string("measured") : method + ", measured") +
                             (record.measuredHeight ? " " + std::to_string(*record.measuredHeight) + " m"
                                                    : std::string{}) +
                             "; height reduced to the phase centre by the controller";
        return antenna;
    }
    // Without it, the words decide, and they are Trimble's own - and
    // translated: a Polish controller writes "Spod mocowania anteny" for the
    // bottom of the antenna mount. Only unambiguous English names are mapped;
    // anything else is Other with the words kept in measuredTo, because a
    // slant height read as a vertical one is an error nobody sees.
    std::string squeezed;
    for (const char c : method) {
        if (c != ' ') {
            squeezed.push_back(c >= 'A' && c <= 'Z' ? static_cast<char>(c - 'A' + 'a') : c);
        }
    }
    if (method.empty()) {
        antenna.method = survey::AntennaHeightMethod::Unknown;
    } else if (squeezed.find("phasecent") != std::string::npos) {
        antenna.method = survey::AntennaHeightMethod::PhaseCentre;
    } else if (squeezed.find("bumper") != std::string::npos ||
               squeezed.find("slant") != std::string::npos ||
               squeezed.find("notch") != std::string::npos) {
        antenna.method = survey::AntennaHeightMethod::Slant;
    } else if (squeezed == "bottomofantennamount") {
        antenna.method = survey::AntennaHeightMethod::Vertical;
    } else {
        antenna.method = survey::AntennaHeightMethod::Other;
    }
    return antenna;
}

// ---- FieldBook records ------------------------------------------------------------

void JobXmlReader::keepAsMetadata(std::string_view prefix, RecordBuffer& record)
{
    // Configuration a later record may override: the latest wins, which is
    // what the job's own state was at export.
    for (std::size_t i = 0; i < record.size(); ++i) {
        record.markUsed(i);
        std::string key(prefix);
        key.push_back('.');
        key.append(record.path(i));
        result_.project.metadata[std::move(key)] = std::string(record.value(i));
    }
}

void JobXmlReader::readFieldBookRecord(std::string_view name, std::string_view id,
                                       std::string_view timeStamp, RecordBuffer& record)
{
    if (name == "PointRecord") {
        readPointRecord(id, record);
    } else if (name == "StationRecord") {
        readStationRecord(id, timeStamp, record);
    } else if (name == "BackBearingRecord") {
        readBackBearingRecord(id, record);
    } else if (name == "TargetRecord") {
        TargetRecord target;
        target.prismType = text(record, "PrismType");
        target.prismConstant = number(record, "PrismConstant");
        target.targetHeight = number(record, "TargetHeight");
        targets_[std::string(id)] = std::move(target);
    } else if (name == "InstrumentRecord") {
        InstrumentRecord instrument;
        instrument.type = text(record, "Type");
        instrument.model = text(record, "Model");
        instrument.serial = text(record, "Serial");
        instrument.firmware = text(record, "FirmwareVersion");
        instruments_[std::string(id)] = std::move(instrument);
    } else if (name == "AtmosphereRecord") {
        AtmosphereRecord atmosphere;
        atmosphere.pressure = number(record, "Pressure");
        atmosphere.temperature = number(record, "Temperature");
        atmosphere.ppm = number(record, "PPM");
        atmosphere.refractionCoefficient = number(record, "RefractionCoefficient");
        atmosphere.applyCurvature = flag(record, "ApplyEarthCurvatureCorrection");
        atmosphere.applyRefraction = flag(record, "ApplyRefractionCorrection");
        atmosphere.applyPpmToRawDistances = flag(record, "ApplyPPMToRawDistances");
        atmospheres_[std::string(id)] = atmosphere;
    } else if (name == "AntennaRecord") {
        AntennaRecord antenna;
        antenna.measuredHeight = number(record, "MeasuredHeight");
        antenna.reducedHeight = number(record, "ReducedHeight");
        antenna.measurementType = text(record, "MeasurementType");
        antenna.equipmentId = text(record, "GPSEquipmentID");
        antennas_[std::string(id)] = std::move(antenna);
    } else if (name == "GPSEquipmentRecord") {
        EquipmentRecord gear;
        gear.antennaType = text(record, "AntennaType");
        gear.antennaSerial = text(record, "AntennaSerialNumber");
        gear.measurementMethod = text(record, "AntennaMeasurementMethod");
        equipment_[std::string(id)] = std::move(gear);
    } else if (name == "ReferenceRecord") {
        ReferenceRecord reference;
        reference.pointName = text(record, "PointName");
        reference.antennaId = text(record, "AntennaID");
        references_[std::string(id)] = std::move(reference);
    } else if (name == "NoteRecord") {
        std::string notes;
        for (std::size_t i = 0; i < record.size(); ++i) {
            if (record.path(i) == "Notes/Note") {
                record.markUsed(i);
                if (!notes.empty()) {
                    notes += '\n';
                }
                notes += record.value(i);
            }
        }
        result_.project.metadata["jxl.note." + std::string(id)] = std::move(notes);
    } else if (name == "LineRecord") {
        readLineRecord(id, record);
    } else if (name == "UnitsRecord" || name == "CorrectionsRecord" ||
               name == "CoordinateSystemRecord" || name == "JobPropertiesRecord" ||
               name == "FeatureCodingRecord" || name == "TimeZoneRecord") {
        keepAsMetadata("jxl." + std::string(name), record);
    } else {
        ++skippedKinds_[std::string(name)];
        ++result_.recordsSkipped;
        return;
    }
    ++result_.recordsRead;
    noteUnread(name, record);
}

void JobXmlReader::readStationRecord(std::string_view id, std::string_view timeStamp,
                                     RecordBuffer& record)
{
    const std::size_t line = record.line();
    const std::string stationName = text(record, "StationName");
    if (stationName.empty()) {
        warn(line, "a station record with no station name was not read");
        record.markAllUsed();
        return;
    }
    ensureNamed(stationName, line);

    survey::SurveyStation station;
    const std::size_t occupation = ++occupations_[stationName];
    station.setup.id = stationName + "#" + std::to_string(occupation);
    station.setup.pointId = stationName;
    const std::optional<double> height = number(record, "TheodoliteHeight");
    station.setup.instrumentHeight = height.value_or(0.0);
    station.source = sourceAt(line);
    station.metadata["jxl.recordId"] = std::string(id);

    survey::InstrumentSettings& settings = station.instrument;
    settings.time = parseTimestamp(timeStamp);
    const std::string instrumentId = text(record, "InstrumentID");
    if (const auto found = instruments_.find(instrumentId); found != instruments_.end()) {
        const InstrumentRecord& instrument = found->second;
        settings.model = instrument.model.empty() ? instrument.type : instrument.model;
        settings.serialNumber = instrument.serial;
        if (instrument.type.starts_with("Trimble")) {
            settings.make = "Trimble";
        }
        station.metadata["jxl.instrumentType"] = instrument.type;
        if (!instrument.firmware.empty()) {
            station.metadata["jxl.firmwareVersion"] = instrument.firmware;
        }
    } else if (!instrumentId.empty()) {
        warn(line, "setup on " + stationName + " names instrument record " + instrumentId +
                       ", which is not in the file");
    }
    const std::string atmosphereId = text(record, "AtmosphereID");
    if (const auto found = atmospheres_.find(atmosphereId); found != atmospheres_.end()) {
        const AtmosphereRecord& atmosphere = found->second;
        settings.pressureHectopascals = atmosphere.pressure;
        settings.temperatureCelsius = atmosphere.temperature;
        settings.atmosphericPpm = atmosphere.ppm;
        // The schema: raw EDM distances carry no atmospheric correction unless
        // ApplyPPMToRawDistances is false, which says it is already in them.
        settings.atmosphericPpmState = atmosphere.applyPpmToRawDistances.value_or(true)
                                           ? survey::CorrectionState::NotApplied
                                           : survey::CorrectionState::Applied;
        settings.refractionCoefficient = atmosphere.refractionCoefficient;
        if (atmosphere.applyCurvature) {
            station.metadata["jxl.applyEarthCurvatureCorrection"] =
                *atmosphere.applyCurvature ? "true" : "false";
        }
        if (atmosphere.applyRefraction) {
            station.metadata["jxl.applyRefractionCorrection"] =
                *atmosphere.applyRefraction ? "true" : "false";
        }
    } else {
        ++setupsWithoutAtmosphere_;
    }
    // The schema: raw vertical circle readings carry no curvature or refraction
    // correction and raw EDM distances no scale factor - both are applied only
    // in the controller's own reductions.
    settings.curvatureRefractionState = survey::CorrectionState::NotApplied;
    settings.scaleFactor = number(record, "ScaleFactor");
    settings.scaleFactorState = survey::CorrectionState::NotApplied;
    for (std::string_view key : {"StationType", "ScaleType", "NumberOfBacksightMeasurements",
                                 "NumberOfBacksightPoints", "ScaleFactorStandardError"}) {
        if (const std::optional<std::string_view> value = record.take(key)) {
            station.metadata["jxl." + std::string(key)] = std::string(trimmed(*value));
        }
    }
    if (!height) {
        warn(line, "setup on " + stationName + " has no instrument height; 0 was used");
    }

    // Setups in one job tend to have alike numbers of shots, so the last
    // one's count saves growing the list a dozen times over, each a copy of
    // every observation so far. Capped, so one very long setup followed by
    // many short ones reserves too much only once.
    if (!result_.project.stations.empty()) {
        lastSetupShots_ = result_.project.stations.back().observations.size();
    }
    station.observations.reserve(std::min<std::size_t>(lastSetupShots_, 1024));

    StationState state;
    state.index = result_.project.stations.size();
    state.heightKnown = height.has_value();
    stations_[std::string(id)] = state;
    result_.project.stations.push_back(std::move(station));
}

void JobXmlReader::readBackBearingRecord(std::string_view id, RecordBuffer& record)
{
    const std::size_t line = record.line();
    const std::string stationId = text(record, "StationRecordID");
    const std::string backsight = text(record, "BackSight");
    const std::optional<double> face1 = number(record, "Face1HorizontalCircle");
    const std::optional<double> face2 = number(record, "Face2HorizontalCircle");
    const std::optional<std::string_view> correction = record.take("OrientationCorrection");
    record.take("Station"); // repeats the StationRecord's name

    const auto found = stations_.find(stationId);
    if (found == stations_.end()) {
        warn(line, "backsight record " + std::string(id) + " names setup record " + stationId +
                       ", which is not in the file");
        return;
    }
    backBearingStations_[std::string(id)] = stationId;
    StationState& state = found->second;
    survey::SurveyStation& station = result_.project.stations[state.index];
    if (state.backsightSet) {
        // One SurveyStation holds one orientation. A re-orientation within a
        // setup is kept in metadata for the reduction and said aloud.
        station.metadata["jxl.reorientation." + std::string(id)] =
            backsight + " " + (face1 ? std::to_string(*face1) : std::string("-"));
        if (!state.reorientationWarned) {
            state.reorientationWarned = true;
            warn(line, "setup " + station.setup.id +
                           " was oriented more than once; the first orientation is used and the "
                           "later ones are in the setup's metadata");
        }
        return;
    }
    state.backsightSet = true;
    if (!backsight.empty() && backsight != station.setup.pointId) {
        ensureNamed(backsight, line);
        station.backsightPointId = backsight;
    }
    // The circle reading observed on the backsight (face 1), which is what
    // SurveyStation::backsightAzimuth holds: the reading, not the azimuth.
    if (face1) {
        station.backsightAzimuth = wrapTwoPi(*face1 * kRadiansPerDegree);
    }
    if (face2) {
        station.metadata["jxl.backsightFace2Circle_deg"] = std::to_string(*face2);
    }
    if (correction && !trimmed(*correction).empty()) {
        station.metadata["jxl.orientationCorrection_deg"] = std::string(trimmed(*correction));
    }
}

void JobXmlReader::readLineRecord(std::string_view id, RecordBuffer& record)
{
    const std::size_t line = record.line();
    if (flag(record, "Deleted").value_or(false)) {
        ++deletedCount_;
        if (deletedIds_.size() < 10) {
            deletedIds_.push_back(std::string(id));
        }
        record.markAllUsed();
        return;
    }
    survey::SurveyFeature feature;
    feature.name = text(record, "Name");
    feature.code = text(record, "Code");
    const std::string method = text(record, "Method");
    const std::string start = text(record, "StartPoint");
    const std::string end = text(record, "EndPoint");
    if (method != "TwoPoints" || start.empty() || end.empty() || start == end) {
        warn(line, "line " + feature.name + " is defined by " +
                       (method.empty() ? std::string("no method") : method) +
                       ", not by two points, and was not read");
        record.markAllUsed();
        return;
    }
    ensureNamed(start, line);
    ensureNamed(end, line);
    feature.pointIds = {start, end};
    feature.source = sourceAt(line);
    feature.metadata["jxl.recordId"] = std::string(id);
    result_.project.features.push_back(std::move(feature));
}

void JobXmlReader::readPointRecord(std::string_view id, RecordBuffer& record)
{
    // Feature and attribute library values, notes and how the point was
    // measured go to point metadata, and are built only for a point no point
    // record has described yet (or a record that carries features or notes):
    // a later shot to the same point would only try to add keys the point
    // already has, and building them per shot to throw them away was a fifth
    // of the read time of a large job.
    const std::string_view name = trimmed(record.take("Name").value_or(std::string_view{}));
    const PointSlot* slot = slotOf(name);
    const bool fresh = slot == nullptr || !slot->described;
    touched_ = nullptr;
    readPointRecordBody(id, record, fresh);
    if (fresh && !name.empty()) {
        // The body leaves the slot it named or positioned in touched_, so it
        // is not looked up again; a record it refused may leave none.
        if (PointSlot* after = touched_ != nullptr ? touched_ : slotOf(name)) {
            const std::map<std::string, std::string>* metadata =
                after->positioned != kNone ? &positioned_[after->positioned].metadata
                : after->named != kNone    ? &named_[after->named].metadata
                                           : nullptr;
            after->described = metadata != nullptr && metadata->contains("jxl.method");
        }
    }
}

void JobXmlReader::keepOrRefuse(std::vector<survey::Observation>& list, bool plainlyValid,
                                std::size_t line)
{
    if (plainlyValid) {
        return;
    }
    // A value the survey model would refuse (validateProject) is a warning
    // about this record, not a reason to lose the whole file.
    if (katana::core::Status status = survey::validateObservation(list.back()); !status.ok()) {
        warn(line, survey::observationKindName(list.back()) + " not imported: " +
                       status.error().message);
        ++result_.recordsSkipped;
        list.pop_back();
    }
}

void JobXmlReader::readPointRecordBody(std::string_view id, RecordBuffer& record, bool fresh)
{
    const std::size_t line = record.line();
    if (flag(record, "Deleted").value_or(false)) {
        ++deletedCount_;
        if (deletedIds_.size() < 10) {
            deletedIds_.push_back(std::string(id));
        }
        record.markAllUsed();
        return;
    }
    // Views into the record buffer, which holds still until the next record.
    auto view = [&record](std::string_view path) {
        return trimmed(record.take(path).value_or(std::string_view{}));
    };
    const std::string name(view("Name"));
    if (name.empty()) {
        warn(line, "a point record with no name was not read");
        record.markAllUsed();
        return;
    }
    const std::string_view code = view("Code");
    const std::string_view method = view("Method");
    const std::string_view surveyMethod = view("SurveyMethod");
    const std::string_view classification = view("Classification");
    const std::string_view description1 = view("Description1");
    const std::string_view description2 = view("Description2");

    std::map<std::string, std::string> extra;
    if (fresh || record.has("Features") || record.has("Notes")) {
        std::string attributeName;
        for (std::size_t i = 0; i < record.size(); ++i) {
            const std::string_view path = record.path(i);
            if (path == "Features/Feature@Name") {
                record.markUsed(i);
                std::string& features = extra["feature"];
                if (!features.empty()) {
                    features += ", ";
                }
                features += record.value(i);
            } else if (path == "Features/Feature/Attribute/Name") {
                record.markUsed(i);
                attributeName = record.value(i);
            } else if (path == "Features/Feature/Attribute/Value") {
                record.markUsed(i);
                extra["attribute." + attributeName] = std::string(record.value(i));
            } else if (path == "Features/Feature/Attribute/Type" ||
                       path == "Features/Feature/Attribute/ItemIndex") {
                record.markUsed(i);
            } else if (path == "Notes/Note") {
                record.markUsed(i);
                std::string& notes = extra["note"];
                if (!notes.empty()) {
                    notes += '\n';
                }
                notes += record.value(i);
            }
        }
    }
    if (!description2.empty()) {
        extra["description2"] = std::string(description2);
    }
    // The file a keyed-in point was imported from, as the controller recorded
    // it: a name to show, never a path to open.
    if (const std::optional<std::string_view> imported = record.take("Source");
        imported && !trimmed(*imported).empty()) {
        extra["jxl.source"] = survey::sourceFileName(trimmed(*imported));
    }
    if (fresh && !method.empty()) {
        extra["jxl.method"] = std::string(method);
    }
    if (fresh && !surveyMethod.empty()) {
        extra["jxl.surveyMethod"] = std::string(surveyMethod);
    }
    if (fresh && !classification.empty()) {
        extra["jxl.classification"] = std::string(classification);
    }

    // Names the point (unless it has coordinates) and gives its named entry
    // the code, description and metadata of this record where it has none.
    auto describeNamed = [&]() {
        PointSlot& slot = slotFor(name);
        touched_ = &slot;
        if (slot.positioned == kNone && slot.named == kNone) {
            slot.named = named_.size();
            survey::UnpositionedPoint& added = named_.emplace_back();
            added.id = name;
            added.source = sourceAt(line);
        }
        if (slot.named == kNone) {
            return;
        }
        survey::UnpositionedPoint& point = named_[slot.named];
        if (point.code.empty()) {
            point.code = code;
        }
        if (point.description.empty()) {
            point.description = description1;
        }
        if (point.metadata.empty()) {
            point.metadata = std::move(extra);
        } else {
            for (auto& [key, value] : extra) {
                point.metadata.try_emplace(key, value);
            }
        }
    };

    // ---- Grid coordinates: keyed in, copied, or computed by a COGO routine.
    if (record.has("Grid")) {
        const std::optional<double> north = number(record, "Grid/North");
        const std::optional<double> east = number(record, "Grid/East");
        const std::optional<double> elevation = number(record, "Grid/Elevation");
        if (north && east) {
            survey::SurveyPoint point;
            point.id = name;
            point.northing = *north;
            point.easting = *east;
            point.elevation = elevation;
            point.code = code;
            point.description = description1;
            point.metadata = std::move(extra);
            point.metadata["jxl.recordId"] = std::string(id);
            point.source = sourceAt(line);
            point.coordinateSource = (surveyMethod == "KeyedIn" || method == "CopiedPoint")
                                         ? survey::CoordinateSource::Entered
                                         : survey::CoordinateSource::Calculated;
            const bool control = classification == "Control";
            addPositioned(std::move(point), control, id);
            touched_ = &slotFor(name);
            if (control && touched_->control &&
                std::none_of(result_.project.controlPoints.begin(),
                             result_.project.controlPoints.end(),
                             [&](const survey::ControlPoint& c) { return c.pointId == name; })) {
                result_.project.controlPoints.push_back(
                    elevation ? survey::ControlPoint::fixed3d(name)
                              : survey::ControlPoint::fixedHorizontal(name));
            }
            return;
        }
        warn(line, "point " + name + " has a grid position with no northing or easting; it "
                                     "was read as a named point without coordinates");
    }

    // ---- Terrestrial observation.
    if (record.has("Circle")) {
        describeNamed();
        // Shots come in runs from one setup to one kind of target, so the
        // last setup and target found are kept and a shot naming the same
        // ones skips the look-up. The pointers stay good: the maps are
        // node-based, and a later record with the same ID replaces the value
        // in the same node.
        const std::string_view stationId = view("StationID");
        if (lastStation_ == nullptr || !detail::sameText(lastStationId_, stationId)) {
            const auto stationFound = stations_.find(stationId);
            if (stationFound == stations_.end()) {
                warn(line, "observation to " + name + " names setup record " +
                               std::string(stationId) + ", which is not in the file; it was not read");
                record.markAllUsed();
                return;
            }
            lastStationId_.assign(stationId);
            lastStation_ = &stationFound->second;
        }
        StationState& state = *lastStation_;
        survey::SurveyStation& station = result_.project.stations[state.index];
        const std::string& at = station.setup.pointId;
        if (name == at) {
            warn(line, "observation from " + at + " to itself was not read");
            record.markAllUsed();
            return;
        }
        (void)record.take("BackBearingID"); // the setup's orientation, already on the station

        const std::string_view targetId = view("TargetID");
        if (lastTarget_ == nullptr || !detail::sameText(lastTargetId_, targetId)) {
            const auto targetFound = targets_.find(targetId);
            lastTarget_ = targetFound == targets_.end() ? nullptr : &targetFound->second;
            lastTargetId_.assign(targetId);
        }
        const TargetRecord* target = lastTarget_;
        if (target == nullptr && !targetId.empty()) {
            warn(line, "observation to " + name + " names target record " + std::string(targetId) +
                           ", which is not in the file; target height 0 was used");
        }
        const double targetHeight =
            target != nullptr && target->targetHeight ? *target->targetHeight : 0.0;
        if (target == nullptr || !target->targetHeight) {
            ++shotsWithoutTargetHeight_;
        }

        // Atmosphere as recorded with the shot: a change within a setup is
        // something the setup-level settings cannot hold, so it is said.
        const std::optional<double> pressure = number(record, "Pressure");
        const std::optional<double> temperature = number(record, "Temperature");
        const survey::InstrumentSettings& settings = station.instrument;
        const bool pressureChanged = pressure && settings.pressureHectopascals &&
                                     std::abs(*pressure - *settings.pressureHectopascals) > 0.05;
        const bool temperatureChanged =
            temperature && settings.temperatureCelsius &&
            std::abs(*temperature - *settings.temperatureCelsius) > 0.05;
        if ((pressureChanged || temperatureChanged) && !state.atmosphereDifferenceWarned) {
            state.atmosphereDifferenceWarned = true;
            warn(line, "the pressure or temperature recorded with shots from setup " +
                           station.setup.id +
                           " differs from the setup's atmosphere record; the setup's is used");
        }

        const std::string_view face = view("Circle/Face");
        survey::Pointing pointing;
        pointing.index = state.nextPointing++;
        pointing.face = face == "Face1"   ? survey::Face::Left
                        : face == "Face2" ? survey::Face::Right
                                          : survey::Face::Unknown;
        const std::optional<double> horizontal = number(record, "Circle/HorizontalCircle");
        const std::optional<double> vertical = number(record, "Circle/VerticalCircle");
        const std::optional<double> distance = number(record, "Circle/EDMDistance");
        const std::optional<double> horizontalError =
            number(record, "Circle/HorizontalCircleStandardError");
        const std::optional<double> verticalError =
            number(record, "Circle/VerticalCircleStandardError");
        const std::optional<double> distanceError = number(record, "Circle/EDMDistanceStandardError");
        const std::string_view mode = view("Circle/EDMMeasurementMode");
        survey::SourceRecord source = sourceAt(line);

        // Built where they are kept rather than built and moved there: an
        // observation is several hundred bytes and a large job has millions.
        // Each is held to the survey model's rules for its kind
        // (survey::validateObservation), which here come to a few
        // comparisons - the ids are already known to be distinct and not
        // empty, and every number came through parseNumber - and the model's
        // own check is asked only for the wording of a refusal.
        std::vector<survey::Observation>& list = station.observations;
        if (horizontal) {
            auto& direction = std::get<survey::HorizontalDirectionObservation>(
                list.emplace_back(std::in_place_type<survey::HorizontalDirectionObservation>));
            direction.at = at;
            direction.to = name;
            direction.direction = wrapTwoPi(circleDegrees(*horizontal) * kRadiansPerDegree);
            direction.sigma = horizontalError && *horizontalError > 0.0
                                  ? *horizontalError * kRadiansPerDegree
                                  : options_.precision.direction;
            direction.source = (vertical || distance) ? source : std::move(source);
            direction.pointing = pointing;
            keepOrRefuse(list, std::isfinite(direction.direction) && positiveFinite(direction.sigma),
                         line);
        }
        if (vertical) {
            auto& zenith = std::get<survey::ZenithAngleObservation>(
                list.emplace_back(std::in_place_type<survey::ZenithAngleObservation>));
            const double reading = circleDegrees(*vertical);
            zenith.from = at;
            zenith.to = name;
            // A face-2 reading (above 180 degrees) measures the zenith angle
            // 360 - reading; see the note at the top of this file.
            zenith.angle = (reading > 180.0 ? 360.0 - reading : reading) * kRadiansPerDegree;
            zenith.sigma = verticalError && *verticalError > 0.0 ? *verticalError * kRadiansPerDegree
                                                                 : options_.precision.zenith;
            zenith.instrumentHeight = station.setup.instrumentHeight;
            zenith.targetHeight = targetHeight;
            zenith.source = distance ? source : std::move(source);
            zenith.pointing = pointing;
            keepOrRefuse(list,
                         zenith.angle >= 0.0 && zenith.angle <= std::numbers::pi &&
                             std::isfinite(zenith.instrumentHeight) &&
                             std::isfinite(zenith.targetHeight) && positiveFinite(zenith.sigma),
                         line);
        }
        if (distance) {
            auto& slope = std::get<survey::DistanceObservation>(
                list.emplace_back(std::in_place_type<survey::DistanceObservation>));
            slope.from = at;
            slope.to = name;
            slope.distance = *distance;
            slope.kind = survey::DistanceKind::Slope;
            slope.sigma = distanceError && *distanceError > 0.0
                              ? *distanceError
                              : survey::distanceSigma(options_.precision, *distance);
            slope.instrumentHeight = station.setup.instrumentHeight;
            slope.targetHeight = targetHeight;
            slope.source = std::move(source);
            slope.pointing = pointing;
            if (target != nullptr) {
                slope.target.prismConstant = target->prismConstant;
                // The schema: no prism constant is in the raw EDM distance.
                slope.target.prismConstantState = survey::CorrectionState::NotApplied;
                slope.target.targetType = target->prismType;
            }
            if (!mode.empty() && slope.target.targetType.empty()) {
                slope.target.targetType = mode;
            }
            keepOrRefuse(list,
                         positiveFinite(slope.distance) && std::isfinite(slope.instrumentHeight) &&
                             std::isfinite(slope.targetHeight) && positiveFinite(slope.sigma),
                         line);
        }
        if (!horizontal && !vertical && !distance) {
            warn(line, "observation to " + name + " records no circle reading and no distance");
        }
        return;
    }

    if (record.has("MTA")) {
        // A mean turned angle is the controller's mean of shots that are
        // already in the file as their own records; reading it too would
        // count them twice.
        describeNamed();
        ++meanTurnedAngles_;
        record.markAllUsed();
        return;
    }

    // ---- GNSS.
    survey::GnssSolution solution = survey::GnssSolution::Unknown;
    if (surveyMethod == "Fix" || surveyMethod == "NetworkFix" || surveyMethod == "RTK" ||
        surveyMethod == "NetworkRTK" || surveyMethod == "xFillFix" || surveyMethod == "RTX" ||
        surveyMethod == "RTXFast" || surveyMethod == "RTKxFill" || surveyMethod == "xFillX") {
        solution = survey::GnssSolution::Fixed;
    } else if (surveyMethod == "Float" || surveyMethod == "NetworkFloat" ||
               surveyMethod == "xFillFloat") {
        solution = survey::GnssSolution::Float;
    } else if (surveyMethod == "Code" || surveyMethod == "WAAS" || surveyMethod == "OmniSTARHP" ||
               surveyMethod == "OmniSTARVBS") {
        solution = survey::GnssSolution::Differential;
    } else if (surveyMethod == "Autonomous") {
        solution = survey::GnssSolution::Autonomous;
    }
    const std::string antennaId = text(record, "AntennaID");

    // QualityControl2 is an X/Y/Z variance-covariance matrix (m^2, 1 sigma,
    // the schema says), which is the frame of an ECEF value; QualityControl3
    // is north/east/elevation, the frame of a latitude/longitude one.
    auto covarianceXyz = [&]() {
        survey::GnssCovariance3 covariance;
        if (!record.has("QualityControl2")) {
            return covariance;
        }
        const std::optional<double> xx = number(record, "QualityControl2/VCVxx");
        const std::optional<double> xy = number(record, "QualityControl2/VCVxy");
        const std::optional<double> xz = number(record, "QualityControl2/VCVxz");
        const std::optional<double> yy = number(record, "QualityControl2/VCVyy");
        const std::optional<double> yz = number(record, "QualityControl2/VCVyz");
        const std::optional<double> zz = number(record, "QualityControl2/VCVzz");
        if (xx && xy && xz && yy && yz && zz && *xx > 0.0 && *yy > 0.0 && *zz > 0.0) {
            covariance = {*xx, *yy, *zz, *xy, *xz, *yz};
        } else {
            warn(line, "the variance-covariance matrix of " + name +
                           " is incomplete or not positive and was not used");
        }
        return covariance;
    };

    if (record.has("ECEFDeltas")) {
        describeNamed();
        std::string base = text(record, "RTK_Base");
        const std::string referenceId = text(record, "ReferenceRecordID");
        std::string baseAntennaId;
        if (const auto found = references_.find(referenceId); found != references_.end()) {
            if (base.empty()) {
                base = found->second.pointName;
            }
            baseAntennaId = found->second.antennaId;
        }
        const std::optional<double> dx = number(record, "ECEFDeltas/DeltaX");
        const std::optional<double> dy = number(record, "ECEFDeltas/DeltaY");
        const std::optional<double> dz = number(record, "ECEFDeltas/DeltaZ");
        if (base.empty() || base == name || !dx || !dy || !dz) {
            warn(line, "the GNSS vector to " + name +
                           " has no base point or an incomplete delta and was not read");
            return;
        }
        ensureNamed(base, line);
        survey::GnssGeocentricBaselineObservation vector;
        vector.from = base;
        vector.to = name;
        vector.delta = {*dx, *dy, *dz};
        vector.covariance = covarianceXyz();
        // The schema: ECEF values are "with respect to the job's WGS-84
        // reference frame".
        vector.referenceFrame = "WGS 84";
        vector.fromAntenna = antennaFor(baseAntennaId);
        vector.toAntenna = antennaFor(antennaId);
        vector.solution = solution;
        vector.source = sourceAt(line);
        addObservation(nullptr, std::move(vector), line);
        return;
    }

    auto globalPosition = [&](survey::GnssGlobalPositionObservation position) {
        describeNamed();
        position.point = name;
        position.antenna = antennaFor(antennaId);
        position.solution = solution;
        position.source = sourceAt(line);
        addObservation(nullptr, std::move(position), line);
    };

    if (record.has("ECEF") || record.has("RTXECEF")) {
        const bool rtx = !record.has("ECEF");
        const std::string_view group = rtx ? "RTXECEF" : "ECEF";
        const std::optional<double> x = number(record, std::string(group) + "/X");
        const std::optional<double> y = number(record, std::string(group) + "/Y");
        const std::optional<double> z = number(record, std::string(group) + "/Z");
        if (!x || !y || !z) {
            describeNamed();
            warn(line, "the ECEF position of " + name + " is incomplete and was not read");
            return;
        }
        survey::GnssGlobalPositionObservation position;
        position.geocentric = survey::GeocentricCoordinate{*x, *y, *z};
        position.covariance = covarianceXyz();
        position.referenceFrame = "WGS 84";
        if (rtx) {
            const std::string frame = text(record, "RTXECEF/ReferenceFrame");
            const std::string epoch = text(record, "RTXECEF/ITRFEpoch");
            position.referenceFrame = frame.empty() ? std::string("ITRF2008") : frame;
            if (!epoch.empty()) {
                position.referenceFrame += " epoch " + epoch;
            }
        }
        globalPosition(std::move(position));
        return;
    }

    if (record.has("WGS84") || record.has("Local")) {
        const bool local = !record.has("WGS84");
        const std::string group = local ? "Local" : "WGS84";
        const std::optional<double> latitude = number(record, group + "/Latitude");
        const std::optional<double> longitude = number(record, group + "/Longitude");
        const std::optional<double> height = number(record, group + "/Height");
        const bool measured = !local && surveyMethod != "KeyedIn" && surveyMethod != "Copied" &&
                              method != "Coordinates" && method != "CopiedPoint";
        if (!latitude || !longitude || !height || !measured) {
            // Keyed in, copied or on a local ellipsoid: a position, but not
            // a grid one and not a GNSS observation. Named, with the values
            // in metadata for whatever projects it.
            describeNamed();
            if (std::map<std::string, std::string>* metadata = metadataOf(name)) {
                const std::string prefix = local ? "local" : "wgs84";
                if (latitude) {
                    (*metadata)[prefix + ".latitude_deg"] = std::to_string(*latitude);
                }
                if (longitude) {
                    (*metadata)[prefix + ".longitude_deg"] = std::to_string(*longitude);
                }
                if (height) {
                    (*metadata)[prefix + ".height"] = std::to_string(*height);
                }
            }
            ++keyedGlobal_;
            return;
        }
        survey::GnssGlobalPositionObservation position;
        position.geodetic = survey::GeodeticCoordinate{*latitude * kRadiansPerDegree,
                                                       *longitude * kRadiansPerDegree, *height};
        if (record.has("QualityControl3")) {
            const std::optional<double> north = number(record, "QualityControl3/SigmaNorth");
            const std::optional<double> east = number(record, "QualityControl3/SigmaEast");
            const std::optional<double> up = number(record, "QualityControl3/SigmaElevation");
            const std::optional<double> northEast =
                number(record, "QualityControl3/CovarianceNorthEast");
            if (north && east && up && *north > 0.0 && *east > 0.0 && *up > 0.0) {
                position.covariance = {*north * *north, *east * *east, *up * *up,
                                       northEast.value_or(0.0), 0.0, 0.0};
            }
        }
        position.referenceFrame = "WGS 84";
        globalPosition(std::move(position));
        return;
    }

    // Anything else - a polar COGO point, a laser offset, a level reading -
    // names the point and says what it did not read.
    describeNamed();
    warn(line, "point " + name + " was recorded by the method " +
                   (method.empty() ? std::string("(none stated)") : std::string(method)) +
                   ", which this reader does not turn into coordinates or observations");
}

// ---- Reductions and Environment -------------------------------------------------------

void JobXmlReader::readReductionsPoint(RecordBuffer& record)
{
    const std::string name = text(record, "Name");
    std::map<std::string, std::string>* metadata = metadataOf(name);
    if (metadata == nullptr) {
        ++reductionsUnknown_;
        record.markAllUsed();
        return;
    }
    // The file's own digits, not a re-formatted double, so a comparison in a
    // report is against exactly what the controller wrote.
    static constexpr std::pair<std::string_view, std::string_view> kKept[] = {
        {"ID", "trimble.reductions.recordId"},
        {"Grid/North", "trimble.reductions.north"},
        {"Grid/East", "trimble.reductions.east"},
        {"Grid/Elevation", "trimble.reductions.elevation"},
        {"WGS84/Latitude", "trimble.reductions.latitude_deg"},
        {"WGS84/Longitude", "trimble.reductions.longitude_deg"},
        {"WGS84/Height", "trimble.reductions.height"},
    };
    for (const auto& [path, key] : kKept) {
        if (const std::optional<std::string_view> value = record.take(path);
            value && !trimmed(*value).empty()) {
            (*metadata)[std::string(key)] = std::string(trimmed(*value));
        }
    }
    // The rest (code, descriptions, features) is the schema's copy of the
    // FieldBook record the point came from, already read there.
    record.markAllUsed();
}

void JobXmlReader::readEnvironment(RecordBuffer& record)
{
    const std::string system = text(record, "CoordinateSystem/SystemName");
    const std::string zone = text(record, "CoordinateSystem/ZoneName");
    const std::string datum = text(record, "CoordinateSystem/DatumName");
    if (!system.empty() || !zone.empty()) {
        std::string declared = system;
        if (!zone.empty()) {
            declared += declared.empty() ? zone : " / " + zone;
        }
        result_.project.coordinateSystem = survey::DeclaredCoordinateSystem::named(declared);
    }
    if (!system.empty()) {
        result_.project.metadata["jxl.environment.CoordinateSystem/SystemName"] = system;
    }
    if (!zone.empty()) {
        result_.project.metadata["jxl.environment.CoordinateSystem/ZoneName"] = zone;
    }
    if (!datum.empty()) {
        result_.project.metadata["jxl.environment.CoordinateSystem/DatumName"] = datum;
    }
    keepAsMetadata("jxl.environment", record);
}

// ---- The whole file ----------------------------------------------------------------

void JobXmlReader::finish()
{
    survey::SurveyProject& project = result_.project;

    // A name both positioned and named is positioned: the named entry was
    // made for an observation before the coordinate record appeared, or after.
    for (survey::UnpositionedPoint& point : named_) {
        if (survey::SurveyPoint* found = positioned(point.id)) {
            for (auto& [key, value] : point.metadata) {
                found->metadata.try_emplace(key, value);
            }
            if (found->code.empty()) {
                found->code = point.code;
            }
            continue;
        }
        project.unpositionedPoints.push_back(std::move(point));
    }
    project.points = std::move(positioned_);

    if (deletedCount_ > 0) {
        std::string ids;
        for (const std::string& id : deletedIds_) {
            ids += ids.empty() ? id : ", " + id;
        }
        if (deletedCount_ > deletedIds_.size()) {
            ids += ", ...";
        }
        warn(0, std::to_string(deletedCount_) +
                    " record(s) the surveyor deleted in the controller were not imported (record "
                    "IDs " +
                    ids + ")");
        result_.recordsSkipped += deletedCount_;
    }
    if (meanTurnedAngles_ > 0) {
        warn(0, std::to_string(meanTurnedAngles_) +
                    " mean turned angle record(s) were not read as observations: each is the "
                    "controller's mean of shots that are read from their own records");
        result_.recordsSkipped += meanTurnedAngles_;
    }
    if (keyedGlobal_ > 0) {
        warn(0, std::to_string(keyedGlobal_) +
                    " point(s) given as keyed-in or local latitude and longitude have no grid "
                    "position; their values are in the point's metadata");
    }
    if (reductionsUnknown_ > 0) {
        warn(0, std::to_string(reductionsUnknown_) +
                    " point(s) in the controller's Reductions section have no record in the field "
                    "book that was read; their reduced coordinates were not kept");
    }
    for (const auto& [kind, count] : skippedKinds_) {
        warn(0, std::to_string(count) + " " + kind + " record(s) were not read: this reader "
                                                     "does not import them");
    }
    for (const auto& [key, unread] : unread_) {
        std::string message = key + " in " + std::to_string(unread.records) +
                              " record(s) was read but the survey model has no place for it";
        if (!unread.leaves.empty()) {
            message += " (";
            bool first = true;
            for (const std::string& leaf : unread.leaves) {
                message += first ? leaf : ", " + leaf;
                first = false;
            }
            message += ")";
        }
        warn(0, std::move(message));
    }

    if (setupsWithoutAtmosphere_ > 0) {
        result_.notCarried.push_back(std::to_string(setupsWithoutAtmosphere_) +
                                     " setup(s) name no atmosphere record: the atmospheric "
                                     "correction state is unknown for them");
    }
    std::size_t setupsWithoutHeight = 0;
    for (const auto& [id, state] : stations_) {
        if (!state.heightKnown) {
            ++setupsWithoutHeight;
        }
    }
    if (setupsWithoutHeight > 0) {
        result_.notCarried.push_back(std::to_string(setupsWithoutHeight) +
                                     " setup(s) without an instrument height");
    }
    if (shotsWithoutTargetHeight_ > 0) {
        result_.notCarried.push_back(std::to_string(shotsWithoutTargetHeight_) +
                                     " shot(s) without a target height");
    }
}

Result<ReadResult> JobXmlReader::read()
{
    // The root.
    XmlPullReader::Event event = xml_.next();
    if (event == XmlPullReader::Event::Error) {
        return makeError(ErrorCode::FileImportFailure, fileName_ + " is not readable as XML: " +
                                                           xml_.error().message);
    }
    if (event != XmlPullReader::Event::StartElement || xml_.name() != "JOBFile") {
        return makeError(ErrorCode::FileImportFailure,
                         fileName_ + " is not a Trimble JobXML file: its first element is <" +
                             std::string(xml_.name()) + ">, not <JOBFile>");
    }
    auto rootAttribute = [&](std::string_view attributeName) {
        std::string_view raw;
        std::string decoded;
        if (xml_.attribute(attributeName, raw) && detail::appendXmlText(raw, decoded).ok()) {
            return decoded;
        }
        return std::string{};
    };
    const std::string version = rootAttribute("version");
    survey::SurveyProject& project = result_.project;
    project.name = rootAttribute("jobName");
    project.units = {survey::LinearUnit::Metres, survey::AngularUnit::DecimalDegrees};
    for (std::string_view key : {"version", "product", "productVersion", "productDBVersion",
                                 "controllerSerialNumber", "TimeStamp"}) {
        if (std::string value = rootAttribute(key); !value.empty()) {
            project.metadata["jxl." + std::string(key)] = std::move(value);
        }
    }
    baseSource_.manufacturer = "Trimble";
    baseSource_.format = std::string(kHumanName);
    baseSource_.formatVersion = version.empty() ? std::string("JobXML") : "JobXML " + version;
    baseSource_.fileName = fileName_;
    project.source = baseSource_;
    if (!version.empty() && !version.starts_with("5.") && !version.starts_with("4.")) {
        warn(1, "JobXML version " + version +
                    " is not one this reader was written against (4.x and 5.x, schema 5.72)");
    }

    RecordBuffer record;
    std::string section; // the depth-2 element being walked
    for (;;) {
        event = xml_.next();
        if (event == XmlPullReader::Event::Error) {
            return makeError(ErrorCode::FileImportFailure,
                             fileName_ + ": " + xml_.error().message);
        }
        if (event == XmlPullReader::Event::EndOfDocument) {
            break;
        }
        if (event == XmlPullReader::Event::EndElement) {
            if (xml_.depth() == 2) {
                section.clear();
            }
            continue;
        }
        if (event != XmlPullReader::Event::StartElement) {
            continue; // white space between records
        }
        const std::size_t depth = xml_.depth();
        if (depth == 2) {
            section = xml_.name();
            if (section == "Environment") {
                record.reset(lines_.lineAt(xml_.offset()));
                Result<bool> gathered = gather(record, 2);
                if (!gathered) {
                    return gathered.error();
                }
                readEnvironment(record);
                section.clear();
            } else if (section != "FieldBook" && section != "Reductions") {
                ++skippedKinds_[section + " section"];
            }
            continue;
        }
        if (depth != 3 || (section != "FieldBook" && section != "Reductions")) {
            continue; // inside a section this reader does not read (counted above)
        }
        // Views into the document, which outlives the record: a record ID
        // and time stamp are read for every record, and a time stamp is too
        // long to copy without a heap allocation.
        const std::string_view name = xml_.name();
        std::string_view id;
        if (xml_.attribute("ID", id)) {
            id = trimmed(id);
        }
        std::string_view timeStamp;
        if (xml_.attribute("TimeStamp", timeStamp)) {
            timeStamp = trimmed(timeStamp);
        }
        record.reset(lines_.lineAt(xml_.offset()));
        Result<bool> gathered = gather(record, 3);
        if (!gathered) {
            return gathered.error();
        }
        if (section == "Reductions") {
            if (name == "Point") {
                readReductionsPoint(record);
            }
            continue;
        }
        readFieldBookRecord(name, id, timeStamp, record);
    }
    finish();
    return std::move(result_);
}

// ---- Registration ----------------------------------------------------------------------

Result<ReadResult> readJobXml(std::string_view bytes, std::string_view fileName,
                              const ReadOptions& options)
{
    JobXmlReader reader(withoutByteOrderMark(bytes), fileName, options);
    return reader.read();
}

// The root element of a JobXML file is <JOBFile>; nothing else uses that
// name. The probe steps over the XML declaration, comments and white space to
// the first element and looks at its name - it does not parse anything.
FormatSignature probeJobXml(const ProbeInput& input)
{
    const std::string_view bytes = withoutByteOrderMark(input.bytes);
    std::size_t p = 0;
    for (int guard = 0; guard < 64; ++guard) {
        while (p < bytes.size() && isSpace(bytes[p])) {
            ++p;
        }
        if (p + 1 >= bytes.size() || bytes[p] != '<') {
            break;
        }
        if (bytes.substr(p).starts_with("<!--")) {
            const std::size_t close = bytes.find("-->", p + 4);
            if (close == std::string_view::npos) {
                break;
            }
            p = close + 3;
            continue;
        }
        if (bytes[p + 1] == '?') {
            const std::size_t close = bytes.find("?>", p + 2);
            if (close == std::string_view::npos) {
                break;
            }
            p = close + 2;
            continue;
        }
        const std::string_view rest = bytes.substr(p);
        if (rest.starts_with("<JOBFile") &&
            (rest.size() == 8 || isSpace(rest[8]) || rest[8] == '>' || rest[8] == '/')) {
            const std::size_t tagEnd = rest.find('>');
            const std::string_view tag = rest.substr(0, tagEnd);
            const bool trimble = tag.find("product=\"Trimble") != std::string_view::npos ||
                                 tag.find("product='Trimble") != std::string_view::npos;
            if (input.extension == "jxl") {
                return {0.98, trimble ? "extension .jxl, root element JOBFile, product Trimble"
                                      : "extension .jxl and root element JOBFile"};
            }
            return {trimble ? 0.95 : 0.9, trimble ? "root element JOBFile, product Trimble"
                                                  : "root element JOBFile"};
        }
        break;
    }
    if (input.extension == "jxl") {
        return {0.2, "extension .jxl, but the first element is not JOBFile"};
    }
    return ruledOut();
}

FormatDescriptor descriptor()
{
    FormatDescriptor format;
    format.id = std::string(kFormatId);
    format.humanName = std::string(kHumanName);
    format.manufacturer = Manufacturer::Trimble;
    format.reads = {.points = true,
                    .observations = true,
                    .stations = true,
                    .features = true,
                    .coordinateSystem = true,
                    .instrumentSettings = true,
                    .gnss = true};
    format.canImport = true;
    format.canExport = false;
    format.parserVersion = "1.0";
    format.extensions = {"jxl"};
    return format;
}

const FormatRegistration kRegistration{descriptor(), &probeJobXml, &readJobXml};

} // namespace
} // namespace katana::surveyio
