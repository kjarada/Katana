// RINEX observation files: detection and the reader (what an import produces is
// set out in include/katana/surveyio/rinex.hpp).
//
// Specifications followed, column for column:
//   * "RINEX: The Receiver Independent Exchange Format, Version 2.11"
//     (W. Gurtner, Astronomical Institute, University of Bern, and L. Estey,
//     UNAVCO; 10 December 2007): tables A1 (observation header) and A2
//     (observation data). 2.10 differs only in details this reader does not
//     depend on.
//   * "RINEX: The Receiver Independent Exchange Format, Version 3.05"
//     (IGS/RTCM RINEX WG, I. Romero ed., 1 December 2020): tables A2
//     (observation header) and A3 (observation data). 3.00 to 3.04 lay out the
//     records this reader uses identically.
//   * "RINEX: The Receiver Independent Exchange Format, Version 4.00"
//     (IGS/RTCM RINEX WG, I. Romero ed., 1 December 2021): observation files
//     are 3.05 plus the optional DOI, LICENSE OF USE and STATION INFORMATION
//     header records; the data records are unchanged.
//
// How the file is read, and why:
//   * One pass over the bytes, a line at a time (memchr), fields cut at the
//     specified columns and read with from_chars. Epochs are COUNTED, not
//     stored, so a day of 1 Hz multi-system data costs its reading time and a
//     few kilobytes, not a copy of itself.
//   * Every satellite record is still checked - its satellite, its length
//     against the observation types the header declares, and that it holds
//     only what an F14.3/I1/I1 field can - so a damaged file is reported record
//     by record instead of being counted as though it were whole.
//   * Records are found by their structure, never by trusting a count alone:
//     an epoch that promises more satellites than follow is reported and the
//     next epoch is read where it actually starts. RINEX 3 and 4 mark every
//     epoch with '>'; a RINEX 2 epoch line has a decimal point in column 19
//     and blanks in columns 27-28, which no observation line can have (its
//     decimal points are in columns 11, 27, 43, ...).
//   * Event flags (table A2 / A3): 1 (power failure) is an epoch and a
//     warning; 2 (antenna starts moving) ends the occupation, and the epochs
//     until 3 are counted as kinematic; 3 (new site occupation) starts a new
//     session from the MARKER NAME and other header records that follow; 4
//     (header records follow) updates the header, starting a new session when
//     it changes the marker, antenna or position under an occupation that
//     already has epochs; 5 (external event) is counted; 6 (cycle slip
//     records) is read through and counted.

#include <algorithm>
#include <array>
#include <bitset>
#include <charconv>
#include <cmath>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/core/text.hpp"
#include "katana/surveyio/format.hpp"
#include "katana/surveyio/rinex.hpp"
#include "rinex_internal.hpp"

namespace katana::surveyio {

namespace {

namespace survey = katana::survey;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using rinex::LineCursor;

constexpr std::string_view kHumanName = "RINEX observation";
constexpr std::string_view kParserVersion = "1.0";

// A damaged 200 MB file could otherwise produce a warning per epoch; past this
// many the rest are counted in one closing warning rather than listed.
constexpr std::size_t kMaxListedWarnings = 500;

// Satellite numbers are two digits (A1,I2 / A1,I2.2).
constexpr std::size_t kSatelliteNumbers = 100;

// A header position further than this from the earth's centre, or nearer, is
// not a position on or above the ground: it is zero, in the wrong unit, or
// damaged. The earth's radius runs from 6 356.8 km (poles) to 6 378.1 km
// (equator); the band leaves room for the Dead Sea and for aircraft.
constexpr double kNearestPlausibleRadius = 6'300'000.0;
constexpr double kFurthestPlausibleRadius = 6'500'000.0;

// Two epochs this close in time are the same epoch written twice.
constexpr double kSameEpochSeconds = 1e-4;

// ---- Characters an observation field may hold ---------------------------------

// m(F14.3,I1,I1): digits, the sign, the decimal point and blanks. Anything
// else - letters, '*' from a Fortran overflow, a stray control byte - means
// the record is not what the header says it is.
constexpr std::array<bool, 256> kObservationCharacters = [] {
    std::array<bool, 256> table{};
    for (char c = '0'; c <= '9'; ++c) {
        table[static_cast<unsigned char>(c)] = true;
    }
    table[static_cast<unsigned char>(' ')] = true;
    table[static_cast<unsigned char>('.')] = true;
    table[static_cast<unsigned char>('-')] = true;
    return table;
}();

bool onlyObservationCharacters(std::string_view text)
{
    for (const char c : text) {
        if (!kObservationCharacters[static_cast<unsigned char>(c)]) {
            return false;
        }
    }
    return true;
}

bool isDigit(char c)
{
    return c >= '0' && c <= '9';
}

bool isBlank(std::string_view text)
{
    return text.find_first_not_of(' ') == std::string_view::npos;
}

std::string formatNumber(double value, int decimals)
{
    std::array<char, 64> buffer{};
    const auto [end, error] = std::to_chars(buffer.data(), buffer.data() + buffer.size(), value,
                                            std::chars_format::fixed, decimals);
    return error == std::errc{} ? std::string(buffer.data(), end) : std::string("?");
}

// ---- Time -----------------------------------------------------------------------

struct EpochTime {
    int year = 0;
    int month = 0;
    int day = 0;
    int hour = 0;
    int minute = 0;
    double second = 0.0;
};

bool isLeapYear(int year)
{
    return (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
}

int daysInMonth(int year, int month)
{
    static constexpr std::array<int, 12> kDays{31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31};
    return month == 2 && isLeapYear(year) ? 29 : kDays[static_cast<std::size_t>(month - 1)];
}

// Days from 1970-01-01 in the proleptic Gregorian calendar (H. Hinnant's
// days_from_civil): only differences are used, to find the interval.
long long daysFromCivil(int year, int month, int day)
{
    year -= month <= 2 ? 1 : 0;
    const long long era = (year >= 0 ? year : year - 399) / 400;
    const long long yearOfEra = year - era * 400;
    const long long dayOfYear = (153 * (month + (month > 2 ? -3 : 9)) + 2) / 5 + day - 1;
    const long long dayOfEra = yearOfEra * 365 + yearOfEra / 4 - yearOfEra / 100 + dayOfYear;
    return era * 146097 + dayOfEra - 719468;
}

// Empty when the fields are a time; otherwise what is wrong with them.
std::string timeProblem(const EpochTime& time)
{
    // GPS time starts in 1980 and RINEX in 1989; a year outside this span is a
    // damaged field, not a survey.
    if (time.year < 1980 || time.year > 2200) {
        return "year " + std::to_string(time.year) + " is not a plausible observation year";
    }
    if (time.month < 1 || time.month > 12) {
        return "month " + std::to_string(time.month) + " does not exist";
    }
    if (time.day < 1 || time.day > daysInMonth(time.year, time.month)) {
        return "day " + std::to_string(time.day) + " does not exist in that month";
    }
    if (time.hour < 0 || time.hour > 23 || time.minute < 0 || time.minute > 59) {
        return "the hour or minute is out of range";
    }
    // 60.x is a leap second; RINEX times may carry one.
    if (!(time.second >= 0.0 && time.second < 61.0)) {
        return "the seconds are out of range";
    }
    return {};
}

double secondsOf(const EpochTime& time)
{
    return static_cast<double>(daysFromCivil(time.year, time.month, time.day)) * 86400.0 +
           time.hour * 3600.0 + time.minute * 60.0 + time.second;
}

survey::SurveyTimestamp timestampOf(const EpochTime& time, const std::string& timeSystem)
{
    return survey::SurveyTimestamp{time.year,   time.month,  time.day,  time.hour,
                                   time.minute, time.second, timeSystem};
}

// ---- Epoch records ---------------------------------------------------------------

struct Epoch {
    bool timed = false; // the date and time fields were filled
    EpochTime time{};
    int flag = 0;
    int count = 0; // satellites, or special records for flags 2-5
};

// Why an epoch line could not be used, or empty when it could.
struct EpochParse {
    bool isEpoch = false;  // the line is shaped as an epoch record
    std::string problem{}; // shaped as one but unusable: bad time, bad count
};

bool readInt(std::string_view field, int& value)
{
    const std::optional<long long> read = rinex::integerField(field);
    if (!read || *read < -1'000'000 || *read > 1'000'000) {
        return false;
    }
    value = static_cast<int>(*read);
    return true;
}

// The time fields shared by both layouts, once cut out.
std::string readTime(std::string_view year, std::string_view month, std::string_view day,
                     std::string_view hour, std::string_view minute, std::string_view second,
                     bool twoDigitYear, EpochTime& time)
{
    if (!readInt(year, time.year) || !readInt(month, time.month) || !readInt(day, time.day) ||
        !readInt(hour, time.hour) || !readInt(minute, time.minute)) {
        return "its date or time is not a number";
    }
    const std::optional<double> seconds = rinex::realField(second);
    if (!seconds) {
        return "its seconds are not a number";
    }
    time.second = *seconds;
    if (twoDigitYear) {
        // RINEX 2.11 section 6.5: 80-99 are 1980-1999, 00-79 are 2000-2079.
        if (time.year < 0 || time.year > 99) {
            return "its two-digit year is not two digits";
        }
        time.year += time.year >= 80 ? 1900 : 2000;
    }
    return timeProblem(time);
}

// RINEX 2 (table A2): 1X,I2.2,4(1X,I2),F11.7,2X,I1,I3,12(A1,I2),F12.9.
EpochParse parseEpochV2(std::string_view line, Epoch& epoch)
{
    EpochParse parse;
    if (line.size() < 29 || !isDigit(line[28]) || line[26] != ' ' || line[27] != ' ') {
        return parse;
    }
    const std::string_view timeFields = line.substr(0, 26);
    epoch.flag = line[28] - '0';
    epoch.timed = !isBlank(timeFields);
    if (epoch.timed && (line[0] != ' ' || line[3] != ' ' || line[6] != ' ' || line[9] != ' ' ||
                        line[12] != ' ' || line[18] != '.')) {
        return parse;
    }
    parse.isEpoch = true;
    const std::string_view count = rinex::columns(line, 30, 3);
    if (isBlank(count)) {
        epoch.count = 0;
    } else if (!readInt(count, epoch.count) || epoch.count < 0) {
        parse.problem = "its satellite count is not a number";
        return parse;
    }
    if (epoch.timed) {
        parse.problem =
            readTime(rinex::columns(line, 2, 2), rinex::columns(line, 5, 2),
                     rinex::columns(line, 8, 2), rinex::columns(line, 11, 2),
                     rinex::columns(line, 14, 2), rinex::columns(line, 16, 11), true, epoch.time);
    }
    return parse;
}

// RINEX 3 and 4 (table A3): A1,1X,I4,4(1X,I2.2),F11.7,2X,I1,I3,6X,F15.12.
EpochParse parseEpochV3(std::string_view line, Epoch& epoch)
{
    EpochParse parse;
    if (line.empty() || line[0] != '>') {
        return parse;
    }
    parse.isEpoch = true;
    if (line.size() < 32 || !isDigit(line[31])) {
        parse.problem = "its event flag (column 32) is missing";
        return parse;
    }
    epoch.flag = line[31] - '0';
    const std::string_view count = rinex::columns(line, 33, 3);
    if (isBlank(count)) {
        epoch.count = 0;
    } else if (!readInt(count, epoch.count) || epoch.count < 0) {
        parse.problem = "its satellite count (columns 33-35) is not a number";
        return parse;
    }
    const std::string_view timeFields = line.substr(1, 30);
    epoch.timed = !isBlank(timeFields);
    if (epoch.timed) {
        parse.problem =
            readTime(rinex::columns(line, 3, 4), rinex::columns(line, 8, 2),
                     rinex::columns(line, 11, 2), rinex::columns(line, 14, 2),
                     rinex::columns(line, 17, 2), rinex::columns(line, 19, 11), false, epoch.time);
    }
    return parse;
}

// ---- What the header says ----------------------------------------------------------

// The part of the header that describes an occupation. Header records -
// whether in the header or after an event flag - update this; an occupation
// takes a copy of it when it starts.
struct Setup {
    survey::GnssSession session{};
    std::string markerType{};
    std::optional<double> interval{};
    std::size_t markerRecord = 0;
    std::size_t positionRecord = 0;
    bool antennaStated = false; // ANTENNA: DELTA H/E/N seen since the last new site

    // Whether two setups describe the same occupation: the same marker, the
    // same antenna at the same height, the same receiver and position.
    [[nodiscard]] bool sameOccupation(const Setup& other) const
    {
        return session.markerName == other.session.markerName &&
               session.markerNumber == other.session.markerNumber &&
               session.antenna == other.session.antenna &&
               session.approximatePosition == other.session.approximatePosition &&
               session.receiverType == other.session.receiverType &&
               session.receiverSerial == other.session.receiverSerial;
    }
};

// The most common gap between consecutive epochs, in milliseconds: the
// interval when the header does not state one, and the check when it does.
class GapCounter {
  public:
    void add(double seconds)
    {
        const long long key = std::llround(seconds * 1000.0);
        for (std::size_t i = 0; i < used_; ++i) {
            if (slots_[i].first == key) {
                ++slots_[i].second;
                return;
            }
        }
        // A handful of distinct gaps is all a real file has (the interval
        // and its multiples where epochs are missing); an eighth kind of gap
        // is not going to be the most common one.
        if (used_ < slots_.size()) {
            slots_[used_++] = {key, 1};
        }
    }

    [[nodiscard]] std::optional<double> mostCommon() const
    {
        if (used_ == 0) {
            return std::nullopt;
        }
        std::size_t best = 0;
        for (std::size_t i = 1; i < used_; ++i) {
            if (slots_[i].second > slots_[best].second) {
                best = i;
            }
        }
        return static_cast<double>(slots_[best].first) / 1000.0;
    }

  private:
    std::array<std::pair<long long, std::size_t>, 8> slots_{};
    std::size_t used_ = 0;
};

struct Occupation {
    Setup setup{};
    std::size_t startRecord = 0;
    bool haveEpoch = false;
    double first = 0.0;
    double last = 0.0;
    double previous = 0.0;
    EpochTime firstTime{};
    EpochTime lastTime{};
    std::size_t epochs = 0;
    std::array<std::bitset<kSatelliteNumbers>, 26> satellites{};
    GapCounter gaps{};
};

// A satellite as a RINEX 2 epoch lists it; number 0 for an unreadable entry.
struct Satellite {
    char system = ' ';
    int number = 0;
};

// The observation types of one system (RINEX 3/4), or of the whole file (2).
struct ObservationTypes {
    int declared = 0; // the count the record gives: what the data records follow
    std::vector<std::string> codes{};
};

// Header record labels kept as metadata, verbatim, under a plainer name.
struct MetadataRecord {
    std::string_view label;
    std::string_view key;
};
constexpr std::array<MetadataRecord, 22> kMetadataRecords{{
    {"SIGNAL STRENGTH UNIT", "signal strength unit"},
    {"RCV CLOCK OFFS APPL", "receiver clock offset applied"},
    {"SYS / DCBS APPLIED", "differential code biases applied"},
    {"SYS / PCVS APPLIED", "phase centre variations applied"},
    {"SYS / SCALE FACTOR", "observation scale factors"},
    {"SYS / PHASE SHIFT", "phase shifts"},
    {"GLONASS COD/PHS/BIS", "GLONASS code-phase biases"},
    {"GLONASS SLOT / FRQ #", "GLONASS slots and frequency numbers"},
    {"LEAP SECONDS", "leap seconds"},
    {"# OF SATELLITES", "satellites (header)"},
    {"PRN / # OF OBS", "observations per satellite (header)"},
    {"WAVELENGTH FACT L1/2", "wavelength factors"},
    {"DOI", "DOI"},
    {"LICENSE OF USE", "licence of use"},
    {"STATION INFORMATION", "station information"},
    {"ANTENNA: DELTA X/Y/Z", "antenna delta X/Y/Z (vehicle)"},
    {"ANTENNA: PHASECENTER", "antenna phase centre"},
    {"ANTENNA: B.SIGHT XYZ", "antenna boresight"},
    {"ANTENNA: ZERODIR AZI", "antenna zero direction azimuth"},
    {"ANTENNA: ZERODIR XYZ", "antenna zero direction"},
    {"CENTER OF MASS: XYZ", "centre of mass (vehicle)"},
    {"MARKER TYPE", "marker type"},
}};

// ---- The reader --------------------------------------------------------------------

class ObservationReader {
  public:
    ObservationReader(std::string_view bytes, std::string_view fileName, const ReadOptions& options)
        : bytes_(bytes), fileName_(fileName), options_(options), cursor_(bytes)
    {
    }

    Result<ReadResult> read();

  private:
    // Header.
    katana::core::Status readHeader();
    void applyHeaderRecord(std::string_view line, std::size_t record, bool inBody);
    void readObservationTypesV2(std::string_view content, std::size_t record, bool inBody);
    void readObservationTypesV3(std::string_view content, std::size_t record, bool inBody);
    std::optional<std::array<double, 3>> threeReals(std::string_view content) const;
    void appendMetadata(const std::string& key, std::string value);

    // Body.
    void readBody();
    // What the satellite records of an epoch are for.
    enum class RecordUse {
        Counted,     // an observation epoch: its satellites count
        ReadThrough, // cycle slips (flag 6): checked, not counted
        Skipped,     // an epoch that could not be read: stepped over
    };
    // The records of an epoch with flag 0, 1 or 6.
    void readEpochRecords(const Epoch& epoch, std::string_view epochLine, std::size_t epochRecord,
                          RecordUse use);
    void readSpecialRecords(const Epoch& epoch, std::size_t epochRecord);
    bool readSatelliteV3(std::string_view line, std::size_t record, bool counted);
    bool readSatelliteListV2(std::string_view list, std::size_t record,
                             std::vector<Satellite>& satellites);
    bool looksLikeEpoch(std::string_view line) const;
    void skipToNextEpoch(std::size_t firstRecord, const std::string& why);

    // Occupations.
    void startOccupation(std::size_t record);
    void closeOccupation();
    void countEpoch(const Epoch& epoch, std::size_t record);
    void markSatellite(char system, int number);

    // Result.
    void warn(std::size_t record, std::string message);
    void finish();
    void readNavigation();
    [[nodiscard]] survey::SourceRecord sourceAt(std::size_t record) const;
    [[nodiscard]] std::string markerIdFromFileName() const;

    std::string_view bytes_;
    std::string fileName_;
    const ReadOptions& options_;
    LineCursor cursor_;

    rinex::VersionRecord version_{};
    bool versionTwo_ = false;
    std::string timeSystem_{}; // as RINEX writes it: "GPS", "GLO", ...
    std::string timeLabel_{};  // as a SurveyTimestamp says it: GLO is "UTC"
    std::optional<EpochTime> headerFirst_{};
    std::optional<EpochTime> headerLast_{};

    ObservationTypes typesV2_{};
    std::array<ObservationTypes, 26> typesV3_{};
    char continuingSystem_ = 0; // the system a blank-led SYS / # / OBS TYPES line continues
    std::vector<Satellite> satellites_{}; // one RINEX 2 epoch's list, reused

    Setup setup_{};
    std::optional<Occupation> active_{};
    std::vector<Occupation> finished_{};
    bool moving_ = false;
    std::size_t firstMovingRecord_ = 0; // the first event flag 2, for the warning

    std::map<std::string, std::string> metadata_{};
    std::vector<std::string> comments_{};
    std::size_t kinematicEpochs_ = 0;
    std::size_t powerFailures_ = 0;
    std::size_t cycleSlipRecords_ = 0;
    std::size_t satelliteRecords_ = 0;
    std::vector<std::string> externalEvents_{};
    std::size_t externalEventCount_ = 0;
    bool textCleaned_ = false;

    ReadResult result_{};
    std::size_t suppressedWarnings_ = 0;
};

void ObservationReader::warn(std::size_t record, std::string message)
{
    if (result_.warnings.size() < kMaxListedWarnings) {
        result_.warnings.push_back(ReadWarning{fileName_, record, std::move(message)});
    } else {
        ++suppressedWarnings_;
    }
}

survey::SourceRecord ObservationReader::sourceAt(std::size_t record) const
{
    return survey::SourceRecord{toString(Manufacturer::OpenStandard), std::string(kHumanName),
                                version_.versionText, fileName_, record};
}

std::string ObservationReader::markerIdFromFileName() const
{
    // A short name's first four characters and a long name's first nine are
    // the station; either way it is what comes before the first '.' or '_'.
    const std::size_t cut = fileName_.find_first_of("._");
    std::string stem = fileName_.substr(0, cut);
    return stem.empty() ? std::string("RINEX marker") : stem;
}

void ObservationReader::appendMetadata(const std::string& key, std::string value)
{
    auto [where, inserted] = metadata_.try_emplace(key, value);
    if (!inserted) {
        where->second += "; ";
        where->second += value;
    }
}

std::optional<std::array<double, 3>> ObservationReader::threeReals(std::string_view content) const
{
    // 3F14.4, but a hand-edited header is often only roughly in its columns;
    // three numbers separated by blanks mean the same thing.
    std::array<double, 3> values{};
    bool fixed = true;
    for (std::size_t i = 0; i < 3 && fixed; ++i) {
        const std::optional<double> value =
            rinex::realField(rinex::columns(content, 1 + 14 * i, 14));
        fixed = value.has_value();
        if (value) {
            values[i] = *value;
        }
    }
    if (fixed) {
        return values;
    }
    const std::vector<std::string_view> fields = rinex::tokens(content);
    if (fields.size() != 3) {
        return std::nullopt;
    }
    for (std::size_t i = 0; i < 3; ++i) {
        const std::optional<double> value = rinex::realField(fields[i]);
        if (!value) {
            return std::nullopt;
        }
        values[i] = *value;
    }
    return values;
}

// ---- Header ------------------------------------------------------------------------

katana::core::Status ObservationReader::readHeader()
{
    std::string_view line;
    while (cursor_.next(line)) {
        const std::size_t record = cursor_.lineNumber();
        if (rinex::headerLabel(line) == "END OF HEADER") {
            ++result_.recordsRead;
            return {};
        }
        applyHeaderRecord(line, record, false);
    }
    return makeError(ErrorCode::FileImportFailure,
                     fileName_ + " has no END OF HEADER record: the file is cut short inside "
                                 "its header, or is not a RINEX observation file");
}

void ObservationReader::applyHeaderRecord(std::string_view line, std::size_t record, bool inBody)
{
    const std::string_view label = rinex::headerLabel(line);
    const std::string_view content = rinex::headerContent(line);
    const auto clean = [&](std::string_view text) { return rinex::cleanText(text, &textCleaned_); };
    if (label.empty()) {
        warn(record, "not a header record: there is no label in columns 61-80");
        ++result_.recordsSkipped;
        return;
    }
    ++result_.recordsRead;
    survey::GnssSession& session = setup_.session;

    if (label == "COMMENT") {
        comments_.push_back(clean(content));
    } else if (label == "PGM / RUN BY / DATE") {
        appendMetadata("program", clean(rinex::columns(content, 1, 20)));
        appendMetadata("run by", clean(rinex::columns(content, 21, 20)));
        appendMetadata("file created", clean(rinex::columns(content, 41, 20)));
    } else if (label == "MARKER NAME") {
        session.markerName = clean(content);
        setup_.markerRecord = record;
    } else if (label == "MARKER NUMBER") {
        session.markerNumber = clean(rinex::columns(content, 1, 20));
    } else if (label == "MARKER TYPE") {
        setup_.markerType = clean(rinex::columns(content, 1, 20));
    } else if (label == "OBSERVER / AGENCY") {
        session.observer = clean(rinex::columns(content, 1, 20));
        session.agency = clean(rinex::columns(content, 21, 40));
    } else if (label == "REC # / TYPE / VERS") {
        session.receiverSerial = clean(rinex::columns(content, 1, 20));
        session.receiverType = clean(rinex::columns(content, 21, 20));
        session.receiverFirmware = clean(rinex::columns(content, 41, 20));
    } else if (label == "ANT # / TYPE") {
        session.antenna.serialNumber = clean(rinex::columns(content, 1, 20));
        // The IGS antenna name is the model in columns 21-36 and the radome
        // in 37-40; the blanks between them are part of the name.
        session.antenna.type = clean(rinex::columns(content, 21, 20));
    } else if (label == "APPROX POSITION XYZ") {
        const std::optional<std::array<double, 3>> xyz = threeReals(content);
        if (!xyz) {
            warn(record, "APPROX POSITION XYZ is not three numbers; the marker has no position "
                         "from this record");
            session.approximatePosition.reset();
        } else {
            session.approximatePosition =
                survey::GeocentricCoordinate{(*xyz)[0], (*xyz)[1], (*xyz)[2]};
            setup_.positionRecord = record;
        }
    } else if (label == "ANTENNA: DELTA H/E/N") {
        const std::optional<std::array<double, 3>> hen = threeReals(content);
        if (!hen) {
            warn(record, "ANTENNA: DELTA H/E/N is not three numbers; the antenna height is "
                         "not known from this record");
        } else {
            // RINEX 2.11 table A1 and 3.05 table A2: the height of the antenna
            // reference point above the marker, and its east and north
            // eccentricity - a vertical height to the ARP.
            session.antenna.height = (*hen)[0];
            session.antenna.eastOffset = (*hen)[1];
            session.antenna.northOffset = (*hen)[2];
            session.antenna.method = survey::AntennaHeightMethod::Vertical;
            session.antenna.measuredTo = "antenna reference point (ARP)";
            setup_.antennaStated = true;
        }
    } else if (label == "# / TYPES OF OBSERV") {
        if (!versionTwo_) {
            warn(record, "# / TYPES OF OBSERV is a RINEX 2 record in a RINEX " +
                             version_.versionText + " file; ignored");
        } else {
            readObservationTypesV2(content, record, inBody);
        }
    } else if (label == "SYS / # / OBS TYPES") {
        if (versionTwo_) {
            warn(record, "SYS / # / OBS TYPES is a RINEX 3 record in a RINEX " +
                             version_.versionText + " file; ignored");
        } else {
            readObservationTypesV3(content, record, inBody);
        }
    } else if (label == "INTERVAL") {
        const std::optional<double> interval = rinex::realField(rinex::columns(content, 1, 10));
        if (!interval || *interval <= 0.0) {
            // Zero is not an interval; some writers put it there for "unknown".
            if (!interval || *interval < 0.0) {
                warn(record, "INTERVAL is not a positive number of seconds; ignored");
            }
        } else {
            setup_.interval = *interval;
        }
    } else if (label == "TIME OF FIRST OBS" || label == "TIME OF LAST OBS") {
        // 5I6,F13.7,5X,A3
        EpochTime time;
        const std::string problem =
            readTime(rinex::columns(content, 1, 6), rinex::columns(content, 7, 6),
                     rinex::columns(content, 13, 6), rinex::columns(content, 19, 6),
                     rinex::columns(content, 25, 6), rinex::columns(content, 31, 13), false, time);
        if (!problem.empty()) {
            warn(record, std::string(label) + " is not a time (" + problem + "); ignored");
        } else if (label == "TIME OF FIRST OBS") {
            headerFirst_ = time;
            const std::string system = clean(rinex::columns(content, 49, 3));
            if (!system.empty()) {
                timeSystem_ = system;
            }
        } else {
            headerLast_ = time;
        }
    } else if (label == "RINEX VERSION / TYPE" || label == "END OF HEADER") {
        if (inBody) {
            warn(record, std::string(label) + " after an event flag; ignored");
        } else {
            warn(record, "a second " + std::string(label) + " record; ignored");
        }
    } else {
        const auto known =
            std::find_if(kMetadataRecords.begin(), kMetadataRecords.end(),
                         [&](const MetadataRecord& entry) { return entry.label == label; });
        if (known != kMetadataRecords.end()) {
            appendMetadata(std::string(known->key), clean(content));
        } else {
            appendMetadata("header record " + clean(label), clean(content));
            warn(record, "header record '" + clean(label) + "' is not one RINEX " +
                             version_.versionText + " defines; kept in the metadata");
        }
    }
}

void ObservationReader::readObservationTypesV2(std::string_view content, std::size_t record,
                                               bool inBody)
{
    // I6, 9(4X,A2), continuation lines 6X,9(4X,A2).
    const std::string_view countField = rinex::columns(content, 1, 6);
    if (!isBlank(countField)) {
        int count = 0;
        if (!readInt(countField, count) || count < 0 || count > 999) {
            warn(record, "# / TYPES OF OBSERV does not start with a number of types");
            return;
        }
        if (inBody && count != typesV2_.declared) {
            warn(record, "the observation types change here from " +
                             std::to_string(typesV2_.declared) + " to " + std::to_string(count));
        }
        typesV2_ = ObservationTypes{count, {}};
    }
    for (const std::string_view code :
         rinex::tokens(content.substr(std::min<std::size_t>(6, content.size())))) {
        typesV2_.codes.push_back(rinex::cleanText(code, &textCleaned_));
    }
}

void ObservationReader::readObservationTypesV3(std::string_view content, std::size_t record,
                                               bool inBody)
{
    // A1,2X,I3,13(1X,A3), continuation lines 6X,13(1X,A3).
    if (!content.empty() && content[0] != ' ') {
        const char system = content[0];
        const std::string_view countField = rinex::columns(content, 4, 3);
        int count = 0;
        if (rinex::systemName(system) == nullptr || system == 'T') {
            warn(record, std::string("SYS / # / OBS TYPES names satellite system '") + system +
                             "', which RINEX does not define; its records will be reported");
            continuingSystem_ = 0;
            return;
        }
        if (!readInt(countField, count) || count < 0 || count > 999) {
            warn(record, "SYS / # / OBS TYPES does not give a number of types in columns 4-6");
            continuingSystem_ = 0;
            return;
        }
        ObservationTypes& types = typesV3_[static_cast<std::size_t>(system - 'A')];
        if (inBody && count != types.declared) {
            warn(record, std::string("the observation types of ") + rinex::systemName(system) +
                             " change here from " + std::to_string(types.declared) + " to " +
                             std::to_string(count));
        }
        types = ObservationTypes{count, {}};
        continuingSystem_ = system;
    } else if (continuingSystem_ == 0) {
        warn(record, "a SYS / # / OBS TYPES continuation line with no system before it");
        return;
    }
    ObservationTypes& types = typesV3_[static_cast<std::size_t>(continuingSystem_ - 'A')];
    for (const std::string_view code :
         rinex::tokens(content.substr(std::min<std::size_t>(6, content.size())))) {
        types.codes.push_back(rinex::cleanText(code, &textCleaned_));
    }
}

// ---- Occupations -------------------------------------------------------------------

void ObservationReader::startOccupation(std::size_t record)
{
    Occupation occupation;
    occupation.setup = setup_;
    occupation.startRecord = record;
    active_ = std::move(occupation);
}

void ObservationReader::closeOccupation()
{
    if (active_) {
        finished_.push_back(std::move(*active_));
        active_.reset();
    }
}

void ObservationReader::markSatellite(char system, int number)
{
    if (active_ && !moving_ && system >= 'A' && system <= 'Z' && number > 0 &&
        number < static_cast<int>(kSatelliteNumbers)) {
        active_->satellites[static_cast<std::size_t>(system - 'A')].set(
            static_cast<std::size_t>(number));
    }
}

void ObservationReader::countEpoch(const Epoch& epoch, std::size_t record)
{
    if (epoch.flag == 1) {
        ++powerFailures_;
        warn(record, "event flag 1: the receiver lost power between the previous epoch and this "
                     "one");
    }
    if (moving_ || !active_) {
        ++kinematicEpochs_;
        return;
    }
    Occupation& occupation = *active_;
    const double seconds = secondsOf(epoch.time);
    if (!occupation.haveEpoch) {
        occupation.haveEpoch = true;
        occupation.first = occupation.last = seconds;
        occupation.firstTime = occupation.lastTime = epoch.time;
    } else {
        const double gap = seconds - occupation.previous;
        if (std::abs(gap) < kSameEpochSeconds) {
            warn(record, "this epoch repeats the time of the epoch before it");
        } else if (gap < 0.0) {
            warn(record, "this epoch is earlier than the epoch before it");
        } else {
            occupation.gaps.add(gap);
        }
        if (seconds < occupation.first) {
            occupation.first = seconds;
            occupation.firstTime = epoch.time;
        }
        if (seconds > occupation.last) {
            occupation.last = seconds;
            occupation.lastTime = epoch.time;
        }
    }
    occupation.previous = seconds;
    ++occupation.epochs;
}

// ---- Body --------------------------------------------------------------------------

bool ObservationReader::looksLikeEpoch(std::string_view line) const
{
    Epoch epoch;
    return versionTwo_ ? parseEpochV2(line, epoch).isEpoch : parseEpochV3(line, epoch).isEpoch;
}

void ObservationReader::skipToNextEpoch(std::size_t firstRecord, const std::string& why)
{
    std::size_t skipped = 1;
    std::string_view line;
    while (!cursor_.atEnd()) {
        const LineCursor::Mark mark = cursor_.mark();
        cursor_.next(line);
        if (looksLikeEpoch(line)) {
            cursor_.reset(mark);
            break;
        }
        ++skipped;
    }
    result_.recordsSkipped += skipped;
    const std::size_t lastRecord = firstRecord + skipped - 1;
    std::string what = "; skipped it";
    if (skipped == 2) {
        what += " and the next line";
    } else if (skipped > 2) {
        what += " and the next " + std::to_string(skipped - 1) + " lines, to line " +
                std::to_string(lastRecord);
    }
    warn(firstRecord, why + what);
}

void ObservationReader::readBody()
{
    std::string_view line;
    while (cursor_.next(line)) {
        const std::size_t record = cursor_.lineNumber();
        Epoch epoch;
        const EpochParse parse =
            versionTwo_ ? parseEpochV2(line, epoch) : parseEpochV3(line, epoch);
        if (!parse.isEpoch) {
            if (isBlank(line) && cursor_.atEnd()) {
                continue; // a blank last line: an editor's, not the writer's
            }
            skipToNextEpoch(record, "this line is not an epoch record where one should start");
            continue;
        }
        if (!parse.problem.empty()) {
            // The count still says how many records belong to it: read past
            // them, so the next epoch is found where it starts.
            warn(record, "an epoch record that cannot be read: " + parse.problem);
            ++result_.recordsSkipped;
            if (epoch.flag <= 1 || epoch.flag == 6) {
                readEpochRecords(epoch, line, record, RecordUse::Skipped);
            }
            continue;
        }
        switch (epoch.flag) {
        case 0:
        case 1:
            if (!epoch.timed) {
                warn(record, "an observation epoch with no date and time; its records are "
                             "skipped");
                ++result_.recordsSkipped;
                readEpochRecords(epoch, line, record, RecordUse::Skipped);
                break;
            }
            ++result_.recordsRead;
            countEpoch(epoch, record);
            readEpochRecords(epoch, line, record, RecordUse::Counted);
            break;
        case 6:
            ++result_.recordsRead;
            cycleSlipRecords_ += static_cast<std::size_t>(epoch.count);
            readEpochRecords(epoch, line, record, RecordUse::ReadThrough);
            break;
        case 2:
        case 3:
        case 4:
        case 5:
            ++result_.recordsRead;
            readSpecialRecords(epoch, record);
            break;
        default:
            skipToNextEpoch(record, "event flag " + std::to_string(epoch.flag) +
                                        " is not one RINEX defines (0 to 6)");
            break;
        }
    }
}

bool ObservationReader::readSatelliteV3(std::string_view line, std::size_t record, bool counted)
{
    if (line.size() < 3 || rinex::systemName(line[0]) == nullptr ||
        !(isDigit(line[1]) || line[1] == ' ') || !isDigit(line[2])) {
        warn(record, "not a satellite record: it does not start with a system letter and a "
                     "two-digit satellite number");
        return false;
    }
    const char system = line[0];
    const std::string satellite(line.substr(0, 3));
    const int number = (line[1] == ' ' ? 0 : (line[1] - '0') * 10) + (line[2] - '0');
    const ObservationTypes& types = typesV3_[static_cast<std::size_t>(system - 'A')];
    if (types.declared == 0) {
        warn(record, "satellite " + satellite + ": the header declares no observation types for " +
                         rinex::systemName(system));
        return false;
    }
    const std::size_t fields = static_cast<std::size_t>(types.declared) * 16;
    std::string_view values = line.substr(3);
    if (values.size() > fields) {
        if (!isBlank(values.substr(fields))) {
            warn(record, "satellite " + satellite + " has more than the " +
                             std::to_string(types.declared) +
                             " observations the header declares for " + rinex::systemName(system));
            return false;
        }
        values = values.substr(0, fields);
    }
    if (!onlyObservationCharacters(values)) {
        warn(record, "satellite " + satellite +
                         " holds characters an observation field (F14.3, I1, I1) cannot");
        return false;
    }
    if (number == 0) {
        warn(record, "satellite " + satellite + ": there is no satellite number 0");
        return false;
    }
    if (counted) {
        markSatellite(system, number);
    }
    return true;
}

bool ObservationReader::readSatelliteListV2(std::string_view list, std::size_t record,
                                            std::vector<Satellite>& satellites)
{
    // 12(A1,I2). A blank system letter is GPS (RINEX 2.11 section 5.1).
    bool readable = true;
    for (std::size_t at = 0; at < list.size(); at += 3) {
        const std::string_view entry = list.substr(at, 3);
        if (isBlank(entry)) {
            continue;
        }
        const char system = entry[0] == ' ' ? 'G' : entry[0];
        if (entry.size() < 3 || rinex::systemName(system) == nullptr ||
            !(isDigit(entry[1]) || entry[1] == ' ') || !isDigit(entry[2])) {
            readable = false;
            satellites.push_back(Satellite{});
            continue;
        }
        const int number = (entry[1] == ' ' ? 0 : (entry[1] - '0') * 10) + (entry[2] - '0');
        satellites.push_back(Satellite{system, number});
    }
    if (!readable) {
        warn(record, "the satellite list holds an entry that is not a satellite (A1,I2)");
    }
    return readable;
}

void ObservationReader::readEpochRecords(const Epoch& epoch, std::string_view epochLine,
                                         std::size_t epochRecord, RecordUse use)
{
    const bool counted = use == RecordUse::Counted;
    std::string_view line;
    const auto ended = [&](int missing) {
        warn(epochRecord, "the file ends inside this epoch: " + std::to_string(missing) +
                              " of its " + std::to_string(epoch.count) +
                              " satellite records are missing");
    };
    const auto cutShort = [&](int found) {
        warn(epochRecord, "the epoch lists " + std::to_string(epoch.count) +
                              " satellites but only " + std::to_string(found) +
                              " records follow it");
    };

    if (!versionTwo_) {
        for (int i = 0; i < epoch.count; ++i) {
            const LineCursor::Mark mark = cursor_.mark();
            if (!cursor_.next(line)) {
                ended(epoch.count - i);
                return;
            }
            if (!line.empty() && line[0] == '>') {
                cursor_.reset(mark);
                cutShort(i);
                return;
            }
            if (use == RecordUse::Skipped) {
                ++result_.recordsSkipped;
            } else if (readSatelliteV3(line, cursor_.lineNumber(), counted)) {
                ++result_.recordsRead;
                satelliteRecords_ += counted ? 1 : 0;
            } else {
                ++result_.recordsSkipped;
            }
        }
        return;
    }

    // RINEX 2: up to twelve satellites on the epoch line, twelve more on each
    // continuation line (32X,12(A1,I2)); then each satellite has one line per
    // five observation types.
    satellites_.clear();
    if (use != RecordUse::Skipped) {
        readSatelliteListV2(rinex::columns(epochLine, 33, 36), epochRecord, satellites_);
    }
    const int continuationLines = epoch.count > 12 ? (epoch.count - 1) / 12 : 0;
    for (int i = 0; i < continuationLines; ++i) {
        const LineCursor::Mark mark = cursor_.mark();
        if (!cursor_.next(line)) {
            ended(epoch.count);
            return;
        }
        if (!isBlank(line.substr(0, std::min<std::size_t>(32, line.size()))) ||
            looksLikeEpoch(line)) {
            cursor_.reset(mark);
            warn(epochRecord, "the epoch lists " + std::to_string(epoch.count) +
                                  " satellites but its satellite list is cut short");
            return;
        }
        if (use == RecordUse::Skipped) {
            ++result_.recordsSkipped;
            continue;
        }
        ++result_.recordsRead;
        readSatelliteListV2(rinex::columns(line, 33, 36), cursor_.lineNumber(), satellites_);
    }
    if (use != RecordUse::Skipped && satellites_.size() != static_cast<std::size_t>(epoch.count)) {
        warn(epochRecord, "the epoch says " + std::to_string(epoch.count) +
                              " satellites but lists " + std::to_string(satellites_.size()));
    }
    const int linesPerSatellite = (typesV2_.declared + 4) / 5;
    for (int i = 0; i < epoch.count; ++i) {
        bool whole = true;
        for (int j = 0; j < linesPerSatellite; ++j) {
            const LineCursor::Mark mark = cursor_.mark();
            if (!cursor_.next(line)) {
                ended(epoch.count - i);
                return;
            }
            if (looksLikeEpoch(line)) {
                cursor_.reset(mark);
                cutShort(i);
                return;
            }
            if (use == RecordUse::Skipped) {
                ++result_.recordsSkipped;
            } else if (line.size() <= 80 && onlyObservationCharacters(line)) {
                ++result_.recordsRead;
            } else {
                whole = false;
                ++result_.recordsSkipped;
                warn(cursor_.lineNumber(),
                     line.size() > 80
                         ? std::string("an observation line longer than the 80 columns RINEX 2 "
                                       "allows")
                         : std::string("holds characters an observation field (F14.3, I1, I1) "
                                       "cannot"));
            }
        }
        const std::size_t index = static_cast<std::size_t>(i);
        if (whole && counted && index < satellites_.size() && satellites_[index].number > 0) {
            markSatellite(satellites_[index].system, satellites_[index].number);
            ++satelliteRecords_;
        }
    }
}

void ObservationReader::readSpecialRecords(const Epoch& epoch, std::size_t epochRecord)
{
    if (epoch.flag == 2) {
        if (!moving_) {
            closeOccupation();
            moving_ = true;
            if (firstMovingRecord_ == 0) {
                firstMovingRecord_ = epochRecord;
            }
        }
    } else if (epoch.flag == 3) {
        // A new site: what identified the old one no longer applies. The
        // antenna and receiver carry on (it is the same equipment moved), and
        // the height is carried with a warning if the new site does not
        // restate it.
        survey::GnssSession& session = setup_.session;
        session.markerName.clear();
        session.markerNumber.clear();
        session.approximatePosition.reset();
        setup_.markerType.clear();
        setup_.markerRecord = 0;
        setup_.positionRecord = 0;
        setup_.antennaStated = false;
    } else if (epoch.flag == 5) {
        ++externalEventCount_;
        // A list, not a count alone: the events are what a person looks for
        // (a camera exposure, a photogrammetry mark). Capped so a file of
        // events cannot grow the metadata without bound.
        if (externalEvents_.size() < 100) {
            externalEvents_.push_back(epoch.timed ? toString(timestampOf(epoch.time, timeLabel_))
                                                  : std::string("(no time)"));
        }
    }

    std::string_view line;
    for (int i = 0; i < epoch.count; ++i) {
        const LineCursor::Mark mark = cursor_.mark();
        if (!cursor_.next(line)) {
            warn(epochRecord, "the file ends before the " + std::to_string(epoch.count) +
                                  " header records this event promises");
            break;
        }
        if (rinex::headerLabel(line).empty() && looksLikeEpoch(line)) {
            cursor_.reset(mark);
            warn(epochRecord, "the event promises " + std::to_string(epoch.count) +
                                  " header records but only " + std::to_string(i) + " follow");
            break;
        }
        applyHeaderRecord(line, cursor_.lineNumber(), true);
    }

    if (epoch.flag == 3) {
        closeOccupation();
        moving_ = false;
        if (setup_.markerRecord == 0) {
            warn(epochRecord, "a new site occupation (event flag 3) without a MARKER NAME; the "
                              "site is named after the file");
        }
        if (!setup_.antennaStated) {
            warn(epochRecord, "the new site occupation does not restate ANTENNA: DELTA H/E/N; "
                              "the previous antenna height (" +
                                  formatNumber(setup_.session.antenna.height, 4) +
                                  " m) is kept, as RINEX carries header values forward");
        }
        startOccupation(epochRecord);
        return;
    }
    if (!moving_ && active_ && !active_->setup.sameOccupation(setup_)) {
        if (active_->epochs > 0) {
            closeOccupation();
            startOccupation(epochRecord);
            warn(epochRecord, "the header records after this event change the marker, antenna "
                              "or position: a new session starts here");
            return;
        }
    }
    if (!moving_ && active_) {
        active_->setup = setup_;
    }
}

// ---- The result --------------------------------------------------------------------

void ObservationReader::finish()
{
    closeOccupation();
    survey::SurveyProject& project = result_.project;
    project.source = sourceAt(0);
    project.units.linear = survey::LinearUnit::Metres;

    std::map<std::string, std::size_t, std::less<>> markers; // id -> index in unpositionedPoints
    std::size_t positions = 0;
    std::vector<std::string> unplaced;
    const Occupation* lastWithEpochs = nullptr;

    for (std::size_t index = 0; index < finished_.size(); ++index) {
        Occupation& occupation = finished_[index];
        survey::GnssSession session = occupation.setup.session;
        const std::size_t record = occupation.setup.markerRecord != 0
                                       ? occupation.setup.markerRecord
                                       : occupation.startRecord;
        if (session.markerName.empty()) {
            std::string id = markerIdFromFileName();
            if (index > 0) {
                id += " occupation " + std::to_string(index + 1);
            }
            if (occupation.setup.markerRecord == 0 && index == 0) {
                warn(0, "the header gives no MARKER NAME; the marker is named '" + id +
                            "' after the file");
            }
            session.markerName = std::move(id);
        }
        session.formatVersion = version_.versionText;
        session.source = sourceAt(record);
        session.epochCount = occupation.epochs;
        if (occupation.haveEpoch) {
            lastWithEpochs = &occupation;
            session.firstEpoch = timestampOf(occupation.firstTime, timeLabel_);
            session.lastEpoch = timestampOf(occupation.lastTime, timeLabel_);
        }
        const std::optional<double> observed = occupation.gaps.mostCommon();
        session.intervalSeconds = occupation.setup.interval ? occupation.setup.interval : observed;
        if (occupation.setup.interval && observed &&
            std::abs(*occupation.setup.interval - *observed) > 5e-4) {
            warn(record, "the header gives an interval of " +
                             formatNumber(*occupation.setup.interval, 3) +
                             " s, but the epochs of " + session.markerName + " are mostly " +
                             formatNumber(*observed, 3) + " s apart");
        }
        for (std::size_t letter = 0; letter < occupation.satellites.size(); ++letter) {
            const std::size_t seen = occupation.satellites[letter].count();
            if (seen > 0) {
                session.satellitesPerSystem[rinex::systemName(static_cast<char>('A' + letter))] =
                    seen;
            }
        }
        if (session.epochCount == 0) {
            warn(record, "no observation epochs were recorded at " + session.markerName);
        }

        // The marker, once, however many sessions occupy it.
        if (!markers.contains(session.markerName)) {
            survey::UnpositionedPoint point;
            point.id = session.markerName;
            point.description = "GNSS marker (RINEX observation file)";
            if (!session.markerNumber.empty()) {
                point.metadata["marker number"] = session.markerNumber;
            }
            if (!occupation.setup.markerType.empty()) {
                point.metadata["marker type"] = occupation.setup.markerType;
            }
            point.source = sourceAt(record);
            markers.emplace(point.id, project.unpositionedPoints.size());
            project.unpositionedPoints.push_back(std::move(point));
        }

        // The header's approximate position, if it is one.
        if (session.approximatePosition) {
            const survey::GeocentricCoordinate& xyz = *session.approximatePosition;
            const double radius = std::sqrt(xyz.x * xyz.x + xyz.y * xyz.y + xyz.z * xyz.z);
            const std::size_t positionRecord = occupation.setup.positionRecord;
            if (radius == 0.0) {
                unplaced.push_back(session.markerName + " (its header position is 0, 0, 0)");
                session.approximatePosition.reset();
            } else if (radius < kNearestPlausibleRadius || radius > kFurthestPlausibleRadius) {
                warn(positionRecord,
                     "APPROX POSITION XYZ is " + formatNumber(radius / 1000.0, 1) +
                         " km from the earth's centre, which is not on or near the ground; " +
                         session.markerName + " is imported without a position");
                unplaced.push_back(session.markerName + " (its header position is implausible)");
                session.approximatePosition.reset();
            } else {
                survey::GnssGlobalPositionObservation position;
                position.point = session.markerName;
                position.geocentric = xyz;
                constexpr double variance =
                    kRinexApproximatePositionSigma * kRinexApproximatePositionSigma;
                position.covariance = survey::GnssCovariance3{variance, variance, variance};
                position.antenna = session.antenna;
                position.solution = survey::GnssSolution::Autonomous;
                position.source = sourceAt(positionRecord);
                project.observations.emplace_back(std::move(position));
                ++positions;
                const auto where = markers.find(session.markerName);
                project.unpositionedPoints[where->second].metadata["position"] =
                    "approximate only: the RINEX header's APPROX POSITION XYZ";
            }
        } else {
            unplaced.push_back(session.markerName + " (the header gives no APPROX POSITION XYZ)");
        }
        project.gnssSessions.push_back(std::move(session));
    }
    if (!project.gnssSessions.empty()) {
        project.name = project.gnssSessions.front().markerName;
    }

    // The header's own first and last times, checked against the epochs.
    if (headerFirst_ && !finished_.empty() && finished_.front().haveEpoch &&
        std::abs(secondsOf(*headerFirst_) - finished_.front().first) > 5e-4) {
        warn(0, "TIME OF FIRST OBS in the header (" +
                    toString(timestampOf(*headerFirst_, timeLabel_)) +
                    ") is not the time of the first epoch (" +
                    toString(project.gnssSessions.front().firstEpoch) + ")");
    }
    if (headerLast_ && lastWithEpochs != nullptr &&
        std::abs(secondsOf(*headerLast_) - lastWithEpochs->last) > 5e-4) {
        warn(0, "TIME OF LAST OBS in the header (" +
                    toString(timestampOf(*headerLast_, timeLabel_)) +
                    ") is not the time of the last epoch (" +
                    toString(timestampOf(lastWithEpochs->lastTime, timeLabel_)) + ")");
    }

    // Metadata: what the header says that the model has no field for.
    std::map<std::string, std::string>& metadata = project.metadata;
    metadata = std::move(metadata_);
    metadata["RINEX version"] = version_.versionText;
    metadata["time system"] = timeSystem_.empty() ? std::string("not stated") : timeSystem_;
    if (!comments_.empty()) {
        std::string joined;
        for (const std::string& comment : comments_) {
            if (!joined.empty()) {
                joined += '\n';
            }
            joined += comment;
        }
        metadata["comments"] = std::move(joined);
    }
    const auto joinCodes = [](const std::vector<std::string>& codes) {
        std::string joined;
        for (const std::string& code : codes) {
            if (!joined.empty()) {
                joined += ' ';
            }
            joined += code;
        }
        return joined;
    };
    if (versionTwo_) {
        metadata["observation types"] = joinCodes(typesV2_.codes);
    } else {
        for (std::size_t letter = 0; letter < typesV3_.size(); ++letter) {
            if (typesV3_[letter].declared > 0) {
                metadata[std::string("observation types ") +
                         rinex::systemName(static_cast<char>('A' + letter))] =
                    joinCodes(typesV3_[letter].codes);
            }
        }
    }
    metadata["satellite records"] = std::to_string(satelliteRecords_);
    if (powerFailures_ > 0) {
        metadata["power failures (event flag 1)"] = std::to_string(powerFailures_);
    }
    if (cycleSlipRecords_ > 0) {
        metadata["cycle slip records (event flag 6)"] = std::to_string(cycleSlipRecords_);
    }
    if (externalEventCount_ > 0) {
        std::string events = std::to_string(externalEventCount_) + ":";
        for (const std::string& when : externalEvents_) {
            events += ' ' + when;
        }
        if (externalEventCount_ > externalEvents_.size()) {
            events += " ...";
        }
        metadata["external events (event flag 5)"] = std::move(events);
    }
    if (kinematicEpochs_ > 0) {
        metadata["kinematic epochs"] = std::to_string(kinematicEpochs_);
        warn(firstMovingRecord_,
             std::to_string(kinematicEpochs_) +
                 " epochs were recorded with the antenna moving (event flag 2); "
                 "they are counted, not imported as positions");
        result_.notCarried.push_back("positions for the " + std::to_string(kinematicEpochs_) +
                                     " epochs recorded while the antenna was moving");
    }
    if (textCleaned_) {
        warn(0, "some header text held control characters or bytes that are not UTF-8; they "
                "were replaced");
    }

    // What the file does not give an import, in words.
    result_.notCarried.push_back(
        "the raw GNSS observations: " + std::to_string(satelliteRecords_) +
        " satellite records (pseudorange, carrier phase, Doppler and signal strength) were "
        "checked and counted, not imported - Katana does not process raw GNSS data");
    if (positions > 0) {
        result_.notCarried.push_back(
            "a survey-grade position: the marker position is the header's approximate one, "
            "weighted with a " +
            formatNumber(kRinexApproximatePositionSigma, 0) + " m standard deviation per axis");
        result_.notCarried.push_back(
            "the reference frame of the approximate position: the file does not state one "
            "(RINEX 2 calls it WGS 84, RINEX 3 and 4 recommend ITRS)");
    }
    for (const std::string& marker : unplaced) {
        result_.notCarried.push_back("a position for " + marker);
    }
}

void ObservationReader::readNavigation()
{
    std::map<std::string, std::string>& metadata = result_.project.metadata;
    const std::vector<std::string> candidates = rinex::navigationFileCandidates(fileName_);
    if (!options_.siblings) {
        metadata["navigation files"] = "not looked for: the file was read on its own";
        return;
    }
    std::vector<std::string> read;
    std::map<std::string, std::size_t> ephemerides;
    for (const std::string& name : candidates) {
        Result<std::string> bytes = options_.siblings(name);
        if (!bytes) {
            if (bytes.error().code != ErrorCode::NotFound) {
                warn(0, "the navigation file " + name +
                            " could not be read: " + bytes.error().message);
            }
            continue;
        }
        Result<rinex::NavigationSummary> summary = rinex::summariseNavigation(*bytes, name);
        if (!summary) {
            warn(0, "the navigation file " + name + " is not used: " + summary.error().message);
            continue;
        }
        read.push_back(name + " (RINEX " + summary->versionText + ")");
        for (const auto& [system, count] : summary->ephemerides) {
            ephemerides[system] += count;
        }
        for (ReadWarning& warning : summary->warnings) {
            if (result_.warnings.size() < kMaxListedWarnings) {
                result_.warnings.push_back(std::move(warning));
            } else {
                ++suppressedWarnings_;
            }
        }
        if (!summary->leapSeconds.empty() && !metadata.contains("leap seconds")) {
            metadata["leap seconds"] = summary->leapSeconds;
        }
    }
    if (read.empty()) {
        std::string looked;
        for (const std::string& name : candidates) {
            looked += looked.empty() ? name : ", " + name;
        }
        metadata["navigation files"] =
            "none beside the observation file (looked for " + looked + ")";
        return;
    }
    std::string names;
    for (const std::string& name : read) {
        names += names.empty() ? name : "; " + name;
    }
    metadata["navigation files"] = std::move(names);
    for (const auto& [system, count] : ephemerides) {
        metadata["broadcast ephemerides " + system] = std::to_string(count);
    }
}

Result<ReadResult> ObservationReader::read()
{
    if (bytes_.empty()) {
        return makeError(ErrorCode::FileImportFailure, fileName_ + " is empty");
    }
    if (const rinex::Packing packing = rinex::packingOf(bytes_); packing != rinex::Packing::Plain) {
        return makeError(ErrorCode::Unsupported, rinex::packingRefusal(packing, fileName_));
    }
    std::string_view line;
    cursor_.next(line);
    const std::optional<rinex::VersionRecord> version = rinex::versionRecord(line);
    if (!version) {
        return makeError(ErrorCode::FileImportFailure,
                         fileName_ + " is not a RINEX file: its first line is not a RINEX "
                                     "VERSION / TYPE record (label in columns 61-80)");
    }
    version_ = *version;
    versionTwo_ = version_.major == 2;
    const char type = version_.fileType;
    if (type != 'O') {
        const bool navigation = type == 'N' || type == 'G' || type == 'H' || type == 'L' ||
                                (versionTwo_ && type == 'E');
        if (navigation) {
            return makeError(ErrorCode::FileImportFailure,
                             fileName_ + " is a RINEX navigation file: it holds satellite "
                                         "orbits, not an occupation. Import the observation file "
                                         "(.rnx ending in O, or .YYo) and keep this one beside "
                                         "it.");
        }
        if (type == 'M') {
            return makeError(ErrorCode::FileImportFailure,
                             fileName_ + " is a RINEX meteorological file: it holds pressure, "
                                         "temperature and humidity, not an occupation. Import "
                                         "the observation file.");
        }
        return makeError(ErrorCode::FileImportFailure, fileName_ + " is a RINEX '" +
                                                           version_.typeText +
                                                           "' file, not an observation file");
    }
    if (version_.major < 2 || version_.major > 4) {
        return makeError(ErrorCode::Unsupported,
                         fileName_ + " is RINEX version " + version_.versionText +
                             "; Katana reads RINEX observation files of versions 2, 3 and 4");
    }
    ++result_.recordsRead;

    if (katana::core::Status header = readHeader(); !header.ok()) {
        return header.error();
    }
    const bool anyTypes =
        versionTwo_ ? typesV2_.declared > 0
                    : std::any_of(typesV3_.begin(), typesV3_.end(),
                                  [](const ObservationTypes& t) { return t.declared > 0; });
    if (!anyTypes) {
        return makeError(ErrorCode::FileImportFailure,
                         fileName_ + " declares no observation types (" +
                             (versionTwo_ ? std::string("# / TYPES OF OBSERV")
                                          : std::string("SYS / # / OBS TYPES")) +
                             "), so its observation records cannot be read");
    }
    const auto checkCount = [&](const ObservationTypes& types, const std::string& what) {
        if (types.declared > 0 && types.codes.size() != static_cast<std::size_t>(types.declared)) {
            warn(0, what + " declares " + std::to_string(types.declared) + " types but lists " +
                        std::to_string(types.codes.size()));
        }
    };
    if (versionTwo_) {
        checkCount(typesV2_, "# / TYPES OF OBSERV");
    } else {
        for (std::size_t letter = 0; letter < typesV3_.size(); ++letter) {
            if (typesV3_[letter].declared > 0) {
                checkCount(typesV3_[letter],
                           std::string("SYS / # / OBS TYPES for ") +
                               rinex::systemName(static_cast<char>('A' + letter)));
            }
        }
    }

    // The time system of every epoch (RINEX 3.05 section 5.2.8, 2.11 table
    // A1): stated in TIME OF FIRST OBS, compulsory in mixed files, otherwise
    // that of the file's one system. GLO is UTC.
    if (timeSystem_.empty()) {
        switch (version_.system) {
        case ' ':
        case 'G':
        case 'S':
            timeSystem_ = "GPS";
            break;
        case 'R':
            timeSystem_ = "GLO";
            break;
        case 'E':
            timeSystem_ = "GAL";
            break;
        case 'J':
            timeSystem_ = "QZS";
            break;
        case 'C':
            timeSystem_ = "BDT";
            break;
        case 'I':
            timeSystem_ = "IRN";
            break;
        default:
            warn(0, "the file mixes satellite systems but TIME OF FIRST OBS does not say which "
                    "time system its epochs are in; they are given without one");
            break;
        }
    }
    timeLabel_ = timeSystem_ == "GLO" ? std::string("UTC") : timeSystem_;

    startOccupation(setup_.markerRecord != 0 ? setup_.markerRecord : 1);
    readBody();
    finish();
    readNavigation();
    if (suppressedWarnings_ > 0) {
        result_.warnings.push_back(ReadWarning{fileName_, 0,
                                               std::to_string(suppressedWarnings_) +
                                                   " more warnings like these are not listed"});
    }
    return std::move(result_);
}

// ---- Registration ------------------------------------------------------------------

Result<ReadResult> readRinexObservation(std::string_view bytes, std::string_view fileName,
                                        const ReadOptions& options)
{
    ObservationReader reader(bytes, fileName, options);
    return reader.read();
}

// The name inside a packed file's name ("x.rnx.gz" -> "x.rnx"), or the name.
std::string_view withoutPackingExtension(std::string_view name)
{
    for (const std::string_view suffix :
         {".gz", ".GZ", ".Z", ".z", ".bz2", ".BZ2", ".zip", ".ZIP"}) {
        if (name.size() > suffix.size() && name.ends_with(suffix)) {
            return name.substr(0, name.size() - suffix.size());
        }
    }
    return name;
}

// True for the names RINEX observation files are given: .rnx / .crx (long
// names, RINEX 3.05 table A1) and .YYo / .YYd (short names; d is Hatanaka).
bool observationFileName(std::string_view name)
{
    const std::size_t dot = name.rfind('.');
    if (dot == std::string_view::npos) {
        return false;
    }
    const std::string extension = katana::core::lowered(name.substr(dot + 1));
    if (extension == "rnx" || extension == "crx") {
        return true;
    }
    return extension.size() == 3 && isDigit(extension[0]) && isDigit(extension[1]) &&
           (extension[2] == 'o' || extension[2] == 'd');
}

FormatSignature probeRinexObservation(const ProbeInput& input)
{
    const std::string_view bytes = input.bytes;
    const rinex::Packing packing = rinex::packingOf(bytes);
    switch (packing) {
    case rinex::Packing::Gzip:
    case rinex::Packing::UnixCompress:
    case rinex::Packing::Bzip2:
    case rinex::Packing::Zip: {
        // Nothing inside a packed file can be read here, so only its name
        // can say it holds RINEX - and the reader then says to unpack it.
        const std::string_view inner = withoutPackingExtension(input.fileName);
        if (inner.size() != input.fileName.size() && observationFileName(inner)) {
            return {0.8, "a compressed file named like a RINEX observation file ('" +
                             input.fileName + "'); it has to be decompressed first"};
        }
        return ruledOut();
    }
    case rinex::Packing::Hatanaka:
        return {0.95, "Compact RINEX (Hatanaka) header 'CRINEX VERS / TYPE'; it has to be "
                      "converted with CRX2RNX first"};
    case rinex::Packing::Plain:
        break;
    }
    const std::size_t end = bytes.find('\n');
    std::string_view first = bytes.substr(0, end);
    if (!first.empty() && first.back() == '\r') {
        first.remove_suffix(1);
    }
    const std::optional<rinex::VersionRecord> version = rinex::versionRecord(first);
    if (!version) {
        return ruledOut();
    }
    const std::string described =
        "first record RINEX VERSION / TYPE, version " + version->versionText;
    if (version->fileType != 'O') {
        return {0.75, described + ", type '" + version->typeText +
                          "' - RINEX, but not an observation file"};
    }
    if (version->major < 2 || version->major > 4) {
        return {0.75, described + " (observation data) - a version this reader does not take"};
    }
    const bool headerEnds = bytes.find("END OF HEADER") != std::string_view::npos;
    if (!headerEnds && !input.truncated) {
        return {0.8, described + " (observation data), but no END OF HEADER"};
    }
    return {0.99, described + " (observation data)" +
                      (observationFileName(input.fileName) ? ", RINEX file name" : "")};
}

FormatDescriptor rinexDescriptor()
{
    FormatDescriptor format;
    format.id = std::string(kRinexObservationFormatId);
    format.humanName = std::string(kHumanName);
    format.manufacturer = Manufacturer::OpenStandard;
    // GNSS sessions, their markers and an approximate position: no surveyed
    // points, no total-station observations, no coded strings.
    format.reads = {.gnss = true};
    format.canImport = true;
    format.canExport = false;
    format.parserVersion = std::string(kParserVersion);
    // Short names (.24o) cannot be listed as extensions; detection reads the
    // content, so they are recognised all the same.
    format.extensions = {"rnx", "obs", "crx"};
    return format;
}

const FormatRegistration kRegistration{rinexDescriptor(), &probeRinexObservation,
                                       &readRinexObservation};

} // namespace

} // namespace katana::surveyio
