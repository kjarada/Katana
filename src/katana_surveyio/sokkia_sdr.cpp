// Sokkia SDR (.sdr): the SDR electronic field book's Comms output - a job as
// fixed-width ASCII records, in either of the two layouts the field book
// writes: SDR33 (14-character point names) and SDR2x (4-digit point numbers).
//
// Specifications this reader implements:
//   [SDR]   Sokkia Technology, "Interfacing with the SOKKIA SDR Electronic
//           Field Book", software version 04-04.xx, October 1999. Chapter 3:
//           the transmission and the record structure; 3.1-3.3 the field
//           types (a field of blanks is null, "not measured"); 3.3.2 the
//           13DU distance-unit note; 3.4 the derivation codes; 3.5 the
//           option values; 3.6.1 and 3.6.2 the SDR2x and SDR33 record
//           layouts. Chapter 5, "General notes on SDR files".
//   [SETX]  Sokkia, "SDR Software Reference Manual (SETX)": 8.2 (the
//           back-bearing record "orients subsequent observations until
//           another back-bearing record is stored"), chapter 28 "SDR
//           Database" (what each record is) and chapter 29 "Observational
//           Calculations" (29.2.3 the faces, 29.2.6 the orientation
//           correction A = H + BKB azimuth - BKB h.obs).
//   [L5]    Sokkia, "SDR Level 5 Reference Manual" (750-1-0073 rev 1),
//           appendix A.1 (the atmospheric correction is set "either on the
//           instrument or via SDR pressure and temperature entries at the
//           station setup, but NOT both") and appendix B ("The SDR applies
//           the prism constant and atmospheric parts per million (PPM)
//           corrections as soon as the observation is accepted").
//   [TA]    Trimble, "SDR33 Observations.xsl" (2022), a published writer of
//           the format: its header leaves out the serial number (42
//           characters), it writes 3 in the distance-unit option for US
//           survey feet and in the coordinate-order option for east first,
//           its time stamp note is "Time Date MM/DD/YYYY Time HH:MM:SS",
//           and its slope distances carry the prism constant and no
//           atmospheric correction.
//   [NIKON] Nikon, "Total Station DTM-322 Instruction Manual", pages
//           169-172, the SDR2x and SDR33 columns side by side: 4 in the
//           angle-unit option for mils, and the coordinate-order option
//           named as such ("1 NEZ, 2 ENZ").
//
// What those say, and this reader relies on:
//   * Bytes 1-2 are the record type, 3-4 the derivation code. The SDR33 and
//     SDR2x layouts list the same fields in the same order: a point id is 16
//     characters or 4, a real 16 or 10, and every other field is the same
//     width in both. The header's version ("SDR33 ...", "SDR20 ...") says
//     which layout the rest of the file is in ([SDR] 3.2.5 and chapter 5).
//   * The header's options are the units of everything after it: angles in
//     degrees, gons or mils (decimal - [SDR] 3.3.1 gives angles 8 decimal
//     places, and 6400 mils to the circle), distances in metres or feet,
//     pressure in mmHg, inHg or mbar, temperature in Celsius or Fahrenheit.
//     13DU names the distance unit again, and can narrow feet to US survey
//     feet ([SDR] 3.3.2). Any unit the format does not define is refused.
//   * 01 is the instrument, 02 a setup (point, coordinates, instrument
//     height), 03 the target height for what follows, 04 the collimation
//     corrections, 05 the pressure and temperature, 06 the job's scale
//     factor, 07 the backsight, 08 a point's coordinates, 09 an
//     observation (slope distance, vertical reading, horizontal circle
//     reading), 10 the job, 11 a reduced observation, 12 a set, 13 a note.
//   * 09 F1 and F2 are the two faces; MD, a mean of several distances, is
//     on the face its vertical reading says ([SETX] 29.2.3: 0 to 180
//     degrees is face 1, 180 to 360 face 2). MC is a CORRECTED observation
//     - oriented, reduced for the heights - derived from the raw ones.
//   * The vertical reading is a zenith angle or one "measured upwards from
//     horizontal", as the latest instrument record says ([SDR] 3.5); the
//     second is taken to its zenith equivalent, 90 degrees less it
//     ([SETX] 29.2.3: the reading "is converted to an equivalent zenith
//     angle").
//
// Readings that are this reader's own, stated so they can be checked:
//   * A line opening with "DD" is a DELETED record. No published document
//     describes the prefix, but after it every such line is a complete
//     record, and a record type is two digits, so "DD" cannot open a live
//     one. A deleted record is not imported, is counted as skipped with a
//     warning naming it, and changes nothing: a deleted 03 sets no target
//     height, a deleted 02 begins no setup. "DDDD" is read the same way.
//   * 07's AZIMUTH is the setup's backsightAzimuth, and its horizontal
//     observation is kept in the setup's metadata, one per round. The
//     reduction takes backsightAzimuth as the backsight's azimuth when no
//     coordinates give one and orients the setup by it less the mean of the
//     setup's own readings on the backsight; [SETX] 29.2.6 orients an
//     observation by the 07's azimuth less its horizontal observation - the
//     same correction, since that observation is the circle reading on the
//     backsight. Taking the horizontal observation as backsightAzimuth would
//     turn the correction to nothing: the ten seconds of [SDR] chapter 2's
//     traverse (azimuth 269 59 50, circle 270 00 00) and the 14 degrees of
//     its chapter 4 sample (azimuth 14, circle 0) would be lost without a
//     word. Where the circle was set to the azimuth - every round of the
//     files this reader was written against - the two agree.
//   * One setup per 02, with every 07 after it that names the same
//     backsight, the same azimuth and a circle reading within one minute of
//     arc of the first as another ROUND of that setup: rounds on one
//     orientation are what the reduction means together, face by face
//     ([SETX] 8.2: a 07 orients what follows until the next). A 07 that
//     names another backsight or azimuth, or a circle moved further, begins
//     a new setup on the same point, as the TDS RW5 reader does for a
//     repeated backsight. The minute: the face-mean circle readings on one
//     backsight across six rounds spread by 6" at most in the files met,
//     while a circle moved between rounds on purpose moves by degrees.
//   * The atmospheric correction's state is Unknown whatever the job says.
//     [L5] has the field book apply it to each distance as it is accepted
//     when the job turns it on (and the instrument apply it when the job
//     does not); [TA] turns the job's switch on and writes distances
//     WITHOUT it. A file does not say which program wrote it, so the
//     reduction is told it does not know, and says so.
//   * The prism constant is in the distances (Applied): [L5] appendix B
//     for the field book, and [TA] adds the target's constant to every
//     distance and writes 0 in the instrument record. The instrument
//     record's value (mm, [SDR] 3.3.4) is the constant.
//   * Raw distances carry no scale factor and no curvature and refraction:
//     [SETX] 29 applies those between an observation and its corrected
//     (MC) and reduced (RED) views.
//   * Option 45 is the order of the coordinates in 02 and 08: 1 north
//     first, 2 east first ([NIKON] "coordinate order"; [TA] writes east
//     first for 2, and for its own 3, "Y-X-Z"). [SDR] calls the fields
//     "Northing" and "Easting" and the option a "prompt" order; its printed
//     report of a file set to E-N-Elev lists East first. A file that states
//     coordinates under any other option is refused.
//   * Coordinates keyed in - a 02 or 08 whose derivation code is KI - are
//     Entered; an 08 TP (a shot stored as a position) FieldObserved; an 08
//     AJ, TV or RS (the traverse adjustment, the traverse, the resection)
//     Calculated; any other Unknown. The FIRST coordinates a file gives a
//     point are kept, as the shared builder keeps them, although the field
//     book's own rule is that the latest win ([SDR] 2.3): the reduction
//     recomputes observed points itself, and control is stated first.
//   * 11 with an azimuth and no distance (a keyed orientation, [SDR] 2.4's
//     "RED KI ... Azimuth ... H.dist <Null> V.Dist <Null> Code BS AZ") is
//     kept in the setup's metadata. An 11 with distances, and a 09 MC, are
//     not imported: both are derived views of observations, and read beside
//     the raw ones they would count them twice.
//   * A 12 SET whose bad marker is 2 ("Bad set", [SDR] 3.5) is not used
//     "for further averaging" ([SDR] chapter 2): its observations, up to
//     the next set or setup, are skipped with a warning.
//   * A 13 note is kept in the notes of the setup it falls in (the
//     project's before the first); a 13TS time stamp also dates the setup
//     it is the first of. Its text is read as [TA]'s "Time Date MM/DD/YYYY
//     Time HH:MM:SS" or as the field book's "DD-Mon-YY HH:MM" ([SDR]
//     chapters 2 and 4) or "Mon-DD-YY HH:MM" (its V04-04.30 example), a
//     two-digit year by the POSIX strptime %y rule (69-99 are 1969-1999,
//     00-68 2000-2068).
//   * Blank lines are not records and are passed over; the record numbers
//     in warnings are line numbers, blank lines counted.
//   * Point ids are trimmed at both ends: [SDR] 3.2 pads alpha fields on the
//     right, and writers are met that pad them on the left.

#include <algorithm>
#include <array>
#include <cmath>
#include <cstddef>
#include <initializer_list>
#include <map>
#include <numbers>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/core/text.hpp"
#include "katana/core/text_encoding.hpp"
#include "katana/math/unit_ratio.hpp"
#include "katana/survey/angles.hpp"
#include "katana/surveyio/detect.hpp"
#include "katana/surveyio/format.hpp"
#include "topcon_raw_builder.hpp"

namespace {

using katana::core::ErrorCode;
using katana::core::formatExactReal;
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

constexpr std::string_view kFormatId = "sokkia-sdr";
constexpr double kPi = std::numbers::pi;

// [SDR] 3.3.1: 6400 mils to the circle.
constexpr double kRadiansPerMil = kPi / 3200.0;

// Hectopascals in one conventional millimetre of mercury: 13.5951 g/cm3 x
// 9.80665 m/s2 x 1 mm = 133.322387415 Pa, the definition NIST Special
// Publication 811 (2008), appendix B.8, gives as 1.333 224 E+02 Pa. An inch
// of mercury is 25.4 of them (3.386 389 E+03 Pa there).
constexpr double kHectopascalsPerMillimetreOfMercury = 1.33322387415;
constexpr double kHectopascalsPerInchOfMercury = 25.4 * kHectopascalsPerMillimetreOfMercury;

// Rounds of one setup whose circle readings on the backsight differ by more
// than this were not observed on one orientation (see the top of this file).
constexpr double kSameCircle = kPi / 10800.0; // one minute of arc

enum class Variant { Sdr33, Sdr2x };
enum class AngleUnit { Degrees, Gons, Mils };
enum class PressureUnit { Unknown, MillimetresOfMercury, InchesOfMercury, Millibars };
enum class TemperatureUnit { Unknown, Celsius, Fahrenheit };
enum class CoordinateOrder { Unknown, NorthFirst, EastFirst };
enum class VerticalReference { Zenith, Horizon, Unknown };

bool isDigit(char c)
{
    return c >= '0' && c <= '9';
}

bool isCodeChar(char c)
{
    return (c >= 'A' && c <= 'Z') || isDigit(c) || c == ' ';
}

// Two digits of record type and two characters of derivation code ([SDR]
// chapter 3), the derivation possibly blank ("used in reports", 3.4).
bool isRecord(std::string_view line)
{
    return line.size() >= 4 && isDigit(line[0]) && isDigit(line[1]) && isCodeChar(line[2]) &&
           isCodeChar(line[3]);
}

// `line` with every leading "DD" (the deleted-record mark) stepped over.
std::string_view withoutDeletionMarks(std::string_view line, std::size_t& marks)
{
    marks = 0;
    while (line.starts_with("DD")) {
        line.remove_prefix(2);
        ++marks;
    }
    return line;
}

// What each record type is, in [SDR] 3.6.2's words, for the warnings that
// name a record - a deleted one, or one of a type this reader does not
// import; empty for a type [SDR] does not list.
std::string_view recordName(std::string_view type)
{
    struct Named {
        std::string_view type;
        std::string_view name;
    };
    static constexpr Named kNames[] = {
        {"00", "header"},
        {"01", "instrument"},
        {"02", "station"},
        {"03", "target height"},
        {"04", "collimation"},
        {"05", "atmosphere"},
        {"06", "scale factor"},
        {"07", "backsight"},
        {"08", "coordinates"},
        {"09", "observation"},
        {"10", "job"},
        {"11", "reduced observation"},
        {"12", "set"},
        {"13", "note"},
        {"14", "GPS instrument"},
        {"15", "GPS station"},
        {"16", "GPS observation"},
        {"17", "GPS reduced observation"},
        {"18", "GPS position"},
        {"19", "GPS projection"},
        {"21", "GPS vertical adjustment"},
        {"24", "GPS latitude and longitude"},
        {"25", "road station"},
        {"26", "road position"},
        {"27", "road check"},
        {"28", "road name"},
        {"29", "horizontal alignment"},
        {"30", "horizontal alignment point"},
        {"31", "horizontal straight"},
        {"32", "horizontal arc"},
        {"33", "horizontal spiral"},
        {"34", "vertical alignment"},
        {"35", "circular vertical curve"},
        {"36", "parabolic vertical curve"},
        {"37", "vertical alignment point"},
        {"38", "cross section"},
        {"39", "template"},
        {"40", "template offset and height difference"},
        {"41", "template grade and distance"},
        {"42", "template side slope"},
        {"44", "apply superelevation"},
        {"45", "define superelevation"},
        {"46", "template element"},
        {"47", "template side slope"},
        {"50", "GPS horizontal adjustment"},
        {"57", "antenna height"},
        {"60", "levelling"},
        {"61", "levelling"},
        {"62", "levelling"},
        {"63", "levelling"},
        {"64", "levelling"},
        {"65", "levelling offset"},
        {"66", "GPS raw observation"},
        {"96", "latitude and longitude station"},
        {"97", "transformation"},
        {"98", "WGS84 latitude and longitude"},
        {"99", "local latitude and longitude"},
    };
    for (const Named& named : kNames) {
        if (named.type == type) {
            return named.name;
        }
    }
    return {};
}

// "01NM (instrument)": a record's type, derivation code and name.
std::string recordLabel(std::string_view record)
{
    const std::string_view name = recordName(record.substr(0, 2));
    return std::string(record.substr(0, 4)) +
           (name.empty() ? std::string{} : " (" + std::string(name) + ")");
}

// `text` with every run of blanks made one: "SDR33     V04-03" is
// "SDR33 V04-03".
std::string collapsedBlanks(std::string_view text)
{
    std::string collapsed;
    for (const char c : trimmed(text)) {
        if (c != ' ' || collapsed.back() != ' ') {
            collapsed += c;
        }
    }
    return collapsed;
}

// The fields of one record, read left to right from its fifth character
// ([SDR] chapter 3: characters 1-4 are the type and the derivation code).
// A column is a CHARACTER: the file was decoded to UTF-8 first, so a name
// typed in another code page is more than one byte and the fields after it
// would shift if bytes were counted. A record shortened by dropped trailing
// blanks ([SDR] chapter 5) simply has shorter fields, empty past its end.
class Fields {
  public:
    Fields(std::string_view record, std::size_t pointIdWidth, std::size_t realWidth)
        : record_(record), pointIdWidth_(pointIdWidth), realWidth_(realWidth)
    {
        for (const char c : record) {
            if (static_cast<unsigned char>(c) >= 0x80) {
                ascii_ = false;
                break;
            }
        }
        if (!ascii_) {
            for (std::size_t i = 0; i < record.size(); ++i) {
                if ((static_cast<unsigned char>(record[i]) & 0xC0) != 0x80) {
                    starts_.push_back(i);
                }
            }
        }
    }

    std::string_view take(std::size_t width)
    {
        const std::size_t count = ascii_ ? record_.size() : starts_.size();
        const std::size_t first = std::min(column_, count);
        const std::size_t last = std::min(column_ + width, count);
        column_ += width;
        return record_.substr(offset(first), offset(last) - offset(first));
    }
    std::string_view pointId() { return take(pointIdWidth_); }
    std::string_view real() { return take(realWidth_); }

  private:
    [[nodiscard]] std::size_t offset(std::size_t character) const
    {
        if (ascii_) {
            return character;
        }
        return character < starts_.size() ? starts_[character] : record_.size();
    }

    std::string_view record_;
    std::size_t pointIdWidth_ = 16;
    std::size_t realWidth_ = 16;
    std::size_t column_ = 4;
    bool ascii_ = true;
    std::vector<std::size_t> starts_;
};

// A text field as something to keep: trimmed, control characters blanked.
std::string cleaned(std::string_view field)
{
    std::string text(trimmed(field));
    for (char& c : text) {
        const auto byte = static_cast<unsigned char>(c);
        if (byte < 0x20 || byte == 0x7F) {
            c = ' ';
        }
    }
    return text;
}

// The first word of a description, which is its field code.
std::string_view firstWord(std::string_view description)
{
    const std::size_t blank = description.find(' ');
    return blank == std::string_view::npos ? description : description.substr(0, blank);
}

// ---- Time stamps --------------------------------------------------------------

std::optional<int> digitsValue(std::string_view text)
{
    if (text.empty() || text.size() > 4) {
        return std::nullopt;
    }
    int value = 0;
    for (const char c : text) {
        if (!isDigit(c)) {
            return std::nullopt;
        }
        value = value * 10 + (c - '0');
    }
    return value;
}

std::optional<int> monthNamed(std::string_view text)
{
    static constexpr std::array<std::string_view, 12> kMonths = {
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"};
    for (std::size_t i = 0; i < kMonths.size(); ++i) {
        if (katana::core::equalsIgnoringCase(text, kMonths[i])) {
            return static_cast<int>(i) + 1;
        }
    }
    return std::nullopt;
}

// "HH:MM" or "HH:MM:SS" into `time`.
bool readClock(std::string_view text, survey::SurveyTimestamp& time)
{
    const std::size_t first = text.find(':');
    if (first == std::string_view::npos) {
        return false;
    }
    const std::size_t second = text.find(':', first + 1);
    const bool withSeconds = second != std::string_view::npos;
    const std::optional<int> hour = digitsValue(text.substr(0, first));
    const std::optional<int> minute = digitsValue(
        text.substr(first + 1, withSeconds ? second - first - 1 : text.npos));
    const std::optional<int> seconds =
        withSeconds ? digitsValue(text.substr(second + 1)) : std::optional<int>(0);
    if (!hour || !minute || !seconds || *hour > 23 || *minute > 59 || *seconds > 60) {
        return false;
    }
    time.hour = *hour;
    time.minute = *minute;
    time.second = *seconds;
    return true;
}

bool plausibleDate(int year, int month, int day)
{
    return year > 0 && month >= 1 && month <= 12 && day >= 1 && day <= 31;
}

// A 13TS note's date and time (see the top of this file for the forms).
std::optional<survey::SurveyTimestamp> timeStampOf(std::string_view text)
{
    survey::SurveyTimestamp time;
    text = trimmed(text);
    // [TA]: "Time Date 07/21/2022 Time 10:39:05".
    if (const std::size_t date = text.find("Date "); date != std::string_view::npos) {
        std::string_view rest = trimmed(text.substr(date + 5));
        const std::size_t end = rest.find(' ');
        const std::string_view day = rest.substr(0, end);
        const std::size_t slash1 = day.find('/');
        const std::size_t slash2 =
            slash1 == std::string_view::npos ? slash1 : day.find('/', slash1 + 1);
        if (slash2 == std::string_view::npos) {
            return std::nullopt;
        }
        const std::optional<int> month = digitsValue(day.substr(0, slash1));
        const std::optional<int> dayOfMonth =
            digitsValue(day.substr(slash1 + 1, slash2 - slash1 - 1));
        const std::optional<int> year = digitsValue(day.substr(slash2 + 1));
        if (!month || !dayOfMonth || !year || day.substr(slash2 + 1).size() != 4 ||
            !plausibleDate(*year, *month, *dayOfMonth)) {
            return std::nullopt;
        }
        time.year = *year;
        time.month = *month;
        time.day = *dayOfMonth;
        rest = end == std::string_view::npos ? std::string_view{} : trimmed(rest.substr(end));
        if (rest.starts_with("Time ")) {
            if (!readClock(trimmed(rest.substr(5)), time)) {
                return std::nullopt;
            }
        }
        return time;
    }
    // The field book's own: "18-Jan-80 20:14", and "May-12-97 10:53".
    const std::size_t blank = text.find(' ');
    const std::string_view day = text.substr(0, blank);
    const std::size_t dash1 = day.find('-');
    const std::size_t dash2 = dash1 == std::string_view::npos ? dash1 : day.find('-', dash1 + 1);
    if (dash2 == std::string_view::npos) {
        return std::nullopt;
    }
    const std::string_view a = day.substr(0, dash1);
    const std::string_view b = day.substr(dash1 + 1, dash2 - dash1 - 1);
    const std::string_view c = day.substr(dash2 + 1);
    const bool dayFirst = monthNamed(b).has_value();
    const std::optional<int> month = dayFirst ? monthNamed(b) : monthNamed(a);
    const std::optional<int> dayOfMonth = digitsValue(dayFirst ? a : b);
    const std::optional<int> shortYear = c.size() == 2 ? digitsValue(c) : std::nullopt;
    if (!month || !dayOfMonth || !shortYear) {
        return std::nullopt;
    }
    // POSIX strptime %y: 69-99 are 1969-1999, 00-68 are 2000-2068.
    const int year = *shortYear >= 69 ? 1900 + *shortYear : 2000 + *shortYear;
    if (!plausibleDate(year, *month, *dayOfMonth)) {
        return std::nullopt;
    }
    time.year = year;
    time.month = *month;
    time.day = *dayOfMonth;
    if (blank != std::string_view::npos && !readClock(trimmed(text.substr(blank)), time)) {
        return std::nullopt;
    }
    return time;
}

// ---- The reader -----------------------------------------------------------------

// One setup's backsight, as its 07 records gave it.
struct Orientation {
    bool set = false;
    std::string backsight;
    std::optional<double> azimuth; // radians
    std::optional<double> reading; // the first round's circle reading, radians
    std::size_t rounds = 0;
    std::size_t record = 0;
};

class SdrReader {
  public:
    SdrReader(std::string_view fileName, survey::SourceRecord source, const ReadOptions& options)
        : builder_(fileName, std::move(source), options)
    {
        // [SETX] 29: curvature and refraction are applied between a raw
        // observation and its corrected (MC) view, so never in an F1, F2 or
        // MD reading - whatever the job's switch, which is about that view.
        settings_.curvatureRefractionState = survey::CorrectionState::NotApplied;
    }

    Result<ReadResult> read(std::string_view text, bool guessedEncoding,
                            katana::core::TextEncoding encoding);

  private:
    void dispatch(std::string_view line, std::size_t n);
    void readHeader(std::string_view line, std::size_t n);
    void readInstrument(Fields& fields, std::size_t n);
    void readStation(Fields& fields, std::string_view derivation, std::size_t n);
    void readTarget(Fields& fields, std::size_t n);
    void readCollimation(Fields& fields, std::size_t n);
    void readAtmosphere(Fields& fields, std::size_t n);
    void readScale(Fields& fields, std::size_t n);
    void readBacksight(Fields& fields, std::size_t n);
    void readPosition(Fields& fields, std::string_view derivation, std::size_t n);
    void readObservation(Fields& fields, std::string_view derivation, std::size_t n);
    void readJob(Fields& fields, std::size_t n);
    void readReduced(Fields& fields, std::size_t n);
    void readSet(Fields& fields, std::size_t n);
    void readNote(Fields& fields, std::string_view derivation, std::size_t n);
    void readDistanceUnitNote(std::string_view text, std::size_t n);
    void readCorrectionNote(std::string_view text);

    std::optional<double> number(std::string_view field, std::string_view what, std::size_t n);
    std::optional<double> angle(std::string_view field, std::string_view what, std::size_t n);
    std::optional<double> length(std::string_view field, std::string_view what, std::size_t n);
    std::string pointIdOf(std::string_view field);
    void coordinates(std::string_view id, std::string_view first, std::string_view second,
                     std::string_view elevation, survey::CoordinateSource how, std::size_t n);
    // The settings a setup begun now starts with, and the current setup's
    // when it has no observation yet (an 01, 05 or 06 read between its 02
    // and its first shot belongs to it).
    void settingsChanged(std::size_t n, std::string_view what);
    void finishSetup();
    void refuse(std::size_t n, std::string message);

    RawProjectBuilder builder_;
    std::optional<katana::core::Error> fatal_;

    // The header.
    bool headerRead_ = false;
    std::size_t headerRecord_ = 0;
    std::string headerOptions_;
    Variant variant_ = Variant::Sdr33;
    std::size_t pointIdWidth_ = 16;
    std::size_t realWidth_ = 16;
    AngleUnit angleUnit_ = AngleUnit::Degrees;
    survey::LinearUnit linearUnit_ = survey::LinearUnit::Metres;
    katana::math::UnitRatio metres_ = katana::math::units::kMetre;
    PressureUnit pressureUnit_ = PressureUnit::Unknown;
    TemperatureUnit temperatureUnit_ = TemperatureUnit::Unknown;
    CoordinateOrder order_ = CoordinateOrder::Unknown;
    char orderOption_ = ' ';

    // The job.
    bool jobRead_ = false;
    bool recordElevations_ = true;
    std::string atmosphericSwitch_; // "on", "off" or empty when the file does not say

    // What applies to the next observation.
    survey::InstrumentSettings settings_{};
    bool instrumentSeen_ = false;
    bool instrumentWarned_ = false;
    VerticalReference vertical_ = VerticalReference::Zenith;
    double targetHeight_ = 0.0;
    bool targetHeightSeen_ = false;
    bool targetHeightWarned_ = false;
    bool badSet_ = false;
    std::string badSetName_;
    std::size_t badSetRecord_ = 0;

    // The current setup.
    Orientation orientation_{};
    std::size_t setupObservations_ = 0;
    // An 01, 05 or 06 read after the current setup's first observation:
    // it belongs to the next setup unless another shot of this one follows.
    std::string pendingChange_;
    std::size_t pendingChangeRecord_ = 0;
    bool weatherSeen_ = false;
    bool nonAscii_ = false;
};

void SdrReader::refuse(std::size_t n, std::string message)
{
    if (!fatal_) {
        fatal_ = makeError(ErrorCode::FileImportFailure,
                           "record " + std::to_string(n) + " of " + builder_.fileName() + ": " +
                               std::move(message),
                           variant_ == Variant::Sdr33 ? "Sokkia SDR33" : "Sokkia SDR2x");
    }
}

// [SDR] 3.3 (i): "an optional minus character, followed by a sequence of at
// least one digit ..., optionally followed by a decimal point ..., optionally
// followed by one or more digits" - no plus, no exponent. A leading point
// (".5") is let through: it is one number however it is read.
bool isSdrReal(std::string_view text)
{
    if (text.starts_with('-')) {
        text.remove_prefix(1);
    }
    std::size_t digits = 0;
    bool point = false;
    for (const char c : text) {
        if (isDigit(c)) {
            ++digits;
        } else if (c == '.' && !point) {
            point = true;
        } else {
            return false;
        }
    }
    return digits > 0;
}

std::optional<double> SdrReader::number(std::string_view field, std::string_view what,
                                        std::size_t n)
{
    // [SDR] 3.3 (ii): a field of blanks is null, "not measured".
    const std::string_view text = trimmed(field);
    if (text.empty()) {
        return std::nullopt;
    }
    const std::optional<double> value =
        isSdrReal(text) ? parseReal(text) : std::optional<double>{};
    if (!value) {
        builder_.warn(n, std::string(what) + " '" + cleaned(field).substr(0, 40) +
                             "' is not a number; it is read as not measured");
    }
    return value;
}

std::optional<double> SdrReader::angle(std::string_view field, std::string_view what,
                                       std::size_t n)
{
    const std::optional<double> value = number(field, what, n);
    if (!value) {
        return std::nullopt;
    }
    switch (angleUnit_) {
    case AngleUnit::Degrees:
        return survey::degreesToRadians(*value);
    case AngleUnit::Gons:
        return survey::gonToRadians(*value);
    case AngleUnit::Mils:
        return *value * kRadiansPerMil;
    }
    return std::nullopt;
}

std::optional<double> SdrReader::length(std::string_view field, std::string_view what,
                                        std::size_t n)
{
    const std::optional<double> value = number(field, what, n);
    if (!value) {
        return std::nullopt;
    }
    return katana::math::toMetres(*value, metres_);
}

std::string SdrReader::pointIdOf(std::string_view field)
{
    return cleaned(field);
}

void SdrReader::coordinates(std::string_view id, std::string_view first, std::string_view second,
                            std::string_view elevation, survey::CoordinateSource how,
                            std::size_t n)
{
    const bool firstStated = !trimmed(first).empty();
    const bool secondStated = !trimmed(second).empty();
    if (!firstStated && !secondStated) {
        return;
    }
    if (order_ == CoordinateOrder::Unknown) {
        refuse(n, "the record states coordinates, and the header's coordinate order option is '" +
                      std::string(1, orderOption_) +
                      "'; the format defines 1 (north first) and 2 (east first), and Katana "
                      "will not guess which ordinate is the northing");
        return;
    }
    const bool northFirst = order_ == CoordinateOrder::NorthFirst;
    const std::optional<double> a = length(first, northFirst ? "northing" : "easting", n);
    const std::optional<double> b = length(second, northFirst ? "easting" : "northing", n);
    // [SETX] 4: with the job's "Record elev" No every point has "the same
    // (indeterminate) elevation" - a placeholder, not a height.
    const std::optional<double> z =
        recordElevations_ ? length(elevation, "elevation", n) : std::nullopt;
    if (!a || !b) {
        if (a || b) {
            builder_.warn(n, "point '" + std::string(id) +
                                 "' is given only one of its northing and easting; it is "
                                 "imported without coordinates");
        }
        return;
    }
    builder_.positionPoint(id, northFirst ? *a : *b, northFirst ? *b : *a, z, how, n);
}

void SdrReader::settingsChanged(std::size_t n, std::string_view what)
{
    survey::SurveyStation* station = builder_.currentStation();
    if (station != nullptr && setupObservations_ == 0) {
        const survey::SurveyTimestamp time = station->instrument.time;
        station->instrument = settings_;
        station->instrument.time = time;
        return;
    }
    // Between one setup's shots and the next 02 is where the field book
    // writes the next setup's instrument and weather; only a shot of the
    // SAME setup after it says the values changed in the middle of one.
    if (station != nullptr) {
        pendingChange_ = std::string(what);
        pendingChangeRecord_ = n;
    }
}

void SdrReader::finishSetup()
{
    survey::SurveyStation* station = builder_.currentStation();
    if (station == nullptr || !orientation_.set) {
        return;
    }
    bool observed = false;
    for (const survey::Observation& o : station->observations) {
        if (const auto* direction = std::get_if<survey::HorizontalDirectionObservation>(&o)) {
            observed = observed || direction->to == orientation_.backsight;
        }
    }
    if (!observed) {
        builder_.warn(orientation_.record,
                      "setup '" + station->setup.id + "' has no observation of its backsight '" +
                          orientation_.backsight +
                          "'; the reduction orients it on the backsight record's azimuth, as "
                          "though the circle read that on the backsight");
    }
}

void SdrReader::readHeader(std::string_view line, std::size_t n)
{
    std::string_view text = line;
    while (!text.empty() && text.back() == ' ') {
        text.remove_suffix(1);
    }
    const std::string version = cleaned(text.substr(4, 16));
    Variant variant = Variant::Sdr33;
    if (version.starts_with("SDR33")) {
        variant = Variant::Sdr33;
    } else if (version.starts_with("SDR2")) {
        variant = Variant::Sdr2x;
    } else {
        refuse(n, "the header names the version '" + version.substr(0, 20) +
                      "'; Katana reads the two layouts the format's publisher documents, SDR33 "
                      "and SDR2x ([SDR] 3.2.5: 'SDR33 V04-..' and 'SDR20 V03-05')");
        return;
    }
    // [SDR] 3.6: 46 characters, the serial number in 21-24, the date and
    // time in 25-40 and the six options in 41-46. [TA] writes no serial
    // number, so its header is 42 characters with the options in 37-42.
    // Both end with the options, so they are the last six characters; a
    // header of another length is read that way too, and said to be odd.
    const bool optionsLast =
        text.size() >= 42 && std::all_of(text.end() - 6, text.end(), isDigit);
    if (!optionsLast) {
        refuse(n, "the header record does not end in its six unit and order options ([SDR] "
                  "3.6: 46 characters, the options in 41-46), so the units cannot be told");
        return;
    }
    const std::string options(text.substr(text.size() - 6));
    std::string serial;
    std::string date;
    if (text.size() == 46) {
        serial = cleaned(text.substr(20, 4));
        date = cleaned(text.substr(24, 16));
    } else {
        date = cleaned(text.substr(20, text.size() - 26));
        if (text.size() != 42) {
            builder_.warn(n, "the header record is " + std::to_string(text.size()) +
                                 " characters where the format's is 46; its last six are read "
                                 "as the options");
        }
    }
    if (headerRead_) {
        // A second transmission appended: the same layout and units read on;
        // any other would change what every number after it means.
        if (options != headerOptions_ || variant != variant_) {
            refuse(n, "a second header record states other units or another layout than the "
                      "first (record " +
                          std::to_string(headerRecord_) + ")");
            return;
        }
        builder_.countRead();
        return;
    }
    headerRead_ = true;
    headerRecord_ = n;
    headerOptions_ = options;
    variant_ = variant;
    pointIdWidth_ = variant == Variant::Sdr33 ? 16 : 4;
    realWidth_ = variant == Variant::Sdr33 ? 16 : 10;

    survey::AngularUnit angular = survey::AngularUnit::DecimalDegrees;
    switch (options[0]) {
    case '1':
        angleUnit_ = AngleUnit::Degrees;
        angular = survey::AngularUnit::DecimalDegrees;
        break;
    case '2':
        angleUnit_ = AngleUnit::Gons;
        angular = survey::AngularUnit::Gons;
        break;
    case '3': // [SDR] 3.5
    case '4': // [NIKON]
        angleUnit_ = AngleUnit::Mils;
        angular = survey::AngularUnit::Mils;
        break;
    default:
        refuse(n, "the header's angle unit option is '" + std::string(1, options[0]) +
                      "'; the format defines 1 degrees, 2 gons and 3 mils, and Katana will not "
                      "guess what another means");
        return;
    }
    switch (options[1]) {
    case '1':
        linearUnit_ = survey::LinearUnit::Metres;
        break;
    case '2': // [SDR] 3.3.2: "meters and international feet"
        linearUnit_ = survey::LinearUnit::Feet;
        break;
    case '3': // [TA]: US survey feet
        linearUnit_ = survey::LinearUnit::UsSurveyFeet;
        break;
    default:
        refuse(n, "the header's distance unit option is '" + std::string(1, options[1]) +
                      "'; the format defines 1 metres and 2 feet, and Katana will not guess "
                      "what another means");
        return;
    }
    metres_ = survey::metresPer(linearUnit_).value();
    switch (options[2]) {
    case '1':
        pressureUnit_ = PressureUnit::MillimetresOfMercury;
        break;
    case '2':
        pressureUnit_ = PressureUnit::InchesOfMercury;
        break;
    case '3':
        pressureUnit_ = PressureUnit::Millibars;
        break;
    default:
        builder_.warn(n, "the header's pressure unit option is '" + std::string(1, options[2]) +
                             "', which the format does not define; no pressure is imported");
        break;
    }
    switch (options[3]) {
    case '1':
        temperatureUnit_ = TemperatureUnit::Celsius;
        break;
    case '2':
        temperatureUnit_ = TemperatureUnit::Fahrenheit;
        break;
    default:
        builder_.warn(n, "the header's temperature unit option is '" +
                             std::string(1, options[3]) +
                             "', which the format does not define; no temperature is imported");
        break;
    }
    orderOption_ = options[4];
    order_ = options[4] == '1'                         ? CoordinateOrder::NorthFirst
             : (options[4] == '2' || options[4] == '3') ? CoordinateOrder::EastFirst
                                                        : CoordinateOrder::Unknown;
    if (options[5] != '1') {
        // [SDR] 3.6.1 "Always '1'"; [TA] "Always Angles Right". Nothing
        // says what another value would do to a circle reading.
        refuse(n, "the header's angles left/right option is '" + std::string(1, options[5]) +
                      "'; only 1, angles right, is documented, and Katana will not guess which "
                      "way another turns");
        return;
    }
    survey::SurveyProject& project = builder_.project();
    project.units = survey::DeclaredUnits{linearUnit_, angular};
    project.metadata["header: version"] = version;
    if (!serial.empty()) {
        project.metadata["header: serial number"] = serial;
    }
    if (!date.empty()) {
        project.metadata["header: date and time"] = date;
    }
    builder_.countRead();
}

void SdrReader::readJob(Fields& fields, std::size_t n)
{
    const std::string name = cleaned(fields.take(16));
    survey::SurveyProject& project = builder_.project();
    if (jobRead_) {
        // [SDR] chapter 5: a job ends where another begins. One import is
        // one survey, so the next job's records are read into it.
        builder_.warn(n, "a second job ('" + name + "') begins here; its records are read into "
                                                    "the same survey");
        project.metadata["job at record " + std::to_string(n)] = name;
        builder_.countRead();
        return;
    }
    jobRead_ = true;
    if (!name.empty()) {
        project.name = name;
    }
    if (variant_ == Variant::Sdr33) {
        // [SDR] 3.6.2: point id type, include elevation, atmospheric
        // correction, C & R correction, refraction constant, sea level
        // correction; 3.5: yes/no fields are 1 No, 2 Yes.
        const std::string_view flags = fields.take(6);
        const auto flag = [&](std::size_t i) { return i < flags.size() ? flags[i] : ' '; };
        const auto yesNo = [](char c) -> std::string {
            return c == '1' ? "off" : (c == '2' ? "on" : "");
        };
        if (flag(1) == '1') {
            recordElevations_ = false;
            project.metadata["job: elevations"] = "not recorded";
        }
        atmosphericSwitch_ = yesNo(flag(2));
        const std::string curvature = yesNo(flag(3));
        const std::string seaLevel = yesNo(flag(5));
        if (!atmosphericSwitch_.empty()) {
            project.metadata["job: atmospheric correction"] = atmosphericSwitch_;
        }
        if (!curvature.empty()) {
            project.metadata["job: curvature and refraction correction"] = curvature;
        }
        if (!seaLevel.empty()) {
            project.metadata["job: sea level correction"] = seaLevel;
        }
        // [SDR] 3.5: the refraction constant, 1 = 0.14, 2 = 0.20; [SETX] 4
        // makes it available "only when C and R crn is set to Yes".
        if (curvature == "on" && (flag(4) == '1' || flag(4) == '2')) {
            settings_.refractionCoefficient = flag(4) == '1' ? 0.14 : 0.20;
        }
    }
    builder_.countRead();
}

void SdrReader::readCorrectionNote(std::string_view text)
{
    // SDR2x keeps the job's corrections in 13CP notes, "Atmos crn: N"
    // ([SDR] chapter 4's sample; [SETX] 28 "Note CP").
    const std::size_t colon = text.find(':');
    if (colon == std::string_view::npos) {
        return;
    }
    const std::string_view key = trimmed(text.substr(0, colon));
    const std::string_view value = trimmed(text.substr(colon + 1));
    const std::string state = value == "Y" ? "on" : (value == "N" ? "off" : "");
    if (state.empty()) {
        return;
    }
    survey::SurveyProject& project = builder_.project();
    if (key == "Atmos crn") {
        atmosphericSwitch_ = state;
        project.metadata["job: atmospheric correction"] = state;
    } else if (key == "C and R crn") {
        project.metadata["job: curvature and refraction correction"] = state;
    } else if (key == "Sea level crn") {
        project.metadata["job: sea level correction"] = state;
    }
}

void SdrReader::readInstrument(Fields& fields, std::size_t n)
{
    // [SDR] 3.6: EDM type, EDM description, EDM serial, theodolite
    // description, theodolite serial, mounting, vertical angle option, EDM
    // offset, reflector offset, prism constant (mm).
    const std::string edmType = cleaned(fields.take(1));
    const std::string edm = cleaned(fields.take(16));
    const std::string edmSerial = cleaned(fields.take(6));
    const std::string theodolite = cleaned(fields.take(16));
    const std::string theodoliteSerial = cleaned(fields.take(6));
    const std::string mounting = cleaned(fields.take(1));
    const std::string verticalOption = cleaned(fields.take(1));
    const std::optional<double> edmOffset = length(fields.real(), "EDM offset", n);
    const std::optional<double> reflectorOffset = length(fields.real(), "reflector offset", n);
    const std::optional<double> prism = number(fields.real(), "prism constant", n);

    instrumentSeen_ = true;
    if (verticalOption == "1") {
        vertical_ = VerticalReference::Zenith;
    } else if (verticalOption == "2") {
        vertical_ = VerticalReference::Horizon;
    } else {
        vertical_ = VerticalReference::Unknown;
        builder_.warn(n, "the instrument's vertical angle option is '" + verticalOption +
                             "'; the format defines 1 (zenith) and 2 (horizon), so the vertical "
                             "readings after this record are not imported");
    }
    // A serial number of zeros is the field book's "none" ([SDR] 2.4's
    // "EDM S/N 000000" for a manual instrument).
    const auto stated = [](const std::string& serialText) {
        return !serialText.empty() && serialText.find_first_not_of('0') != std::string::npos;
    };
    settings_.model = theodolite.empty() ? edm : theodolite;
    settings_.serialNumber = stated(theodoliteSerial) ? theodoliteSerial
                                                      : (stated(edmSerial) ? edmSerial : "");
    if (prism) {
        // [SDR] 3.3.4: millimetres whatever the distance unit.
        settings_.prismConstant = *prism / 1000.0;
        settings_.prismConstantState = survey::CorrectionState::Applied;
    } else {
        settings_.prismConstant.reset();
        settings_.prismConstantState = survey::CorrectionState::Unknown;
    }
    survey::SurveyProject& project = builder_.project();
    if (!edmType.empty()) {
        project.metadata["instrument: EDM type option"] = edmType;
    }
    if (!edm.empty() && edm != settings_.model) {
        project.metadata["instrument: EDM"] = edm;
    }
    if (!mounting.empty()) {
        project.metadata["instrument: mounting option"] = mounting;
    }
    // An EDM or reflector off the telescope's axis needs [SETX] 29.2.4's
    // eccentric reduction, which the model has no place for.
    for (const auto& [offset, name] : {std::pair{edmOffset, "EDM offset"},
                                       std::pair{reflectorOffset, "reflector offset"}}) {
        if (offset && *offset != 0.0) {
            builder_.warn(n, std::string("the instrument record's ") + name + " is " +
                                 formatExactReal(*offset) +
                                 " m, which is not applied to the distances");
            project.metadata[std::string("instrument: ") + name + " (m)"] =
                formatExactReal(*offset);
        }
    }
    settingsChanged(n, "the instrument");
    builder_.countRead();
}

void SdrReader::readAtmosphere(Fields& fields, std::size_t n)
{
    const std::optional<double> pressure = number(fields.real(), "pressure", n);
    const std::optional<double> temperature = number(fields.real(), "temperature", n);
    if (!pressure && !temperature) {
        builder_.skip(n, "atmosphere record with neither a pressure nor a temperature");
        return;
    }
    weatherSeen_ = true;
    if (pressure) {
        switch (pressureUnit_) {
        case PressureUnit::MillimetresOfMercury:
            settings_.pressureHectopascals = *pressure * kHectopascalsPerMillimetreOfMercury;
            break;
        case PressureUnit::InchesOfMercury:
            settings_.pressureHectopascals = *pressure * kHectopascalsPerInchOfMercury;
            break;
        case PressureUnit::Millibars: // a millibar is a hectopascal
            settings_.pressureHectopascals = *pressure;
            break;
        case PressureUnit::Unknown:
            break;
        }
    }
    if (temperature) {
        switch (temperatureUnit_) {
        case TemperatureUnit::Celsius:
            settings_.temperatureCelsius = *temperature;
            break;
        case TemperatureUnit::Fahrenheit:
            // NIST SP 811 (2008), appendix B: t/C = (t/F - 32) / 1.8.
            settings_.temperatureCelsius = (*temperature - 32.0) / 1.8;
            break;
        case TemperatureUnit::Unknown:
            break;
        }
    }
    settingsChanged(n, "the weather");
    builder_.countRead();
}

void SdrReader::readScale(Fields& fields, std::size_t n)
{
    const std::optional<double> factor = number(fields.real(), "scale factor", n);
    if (!factor || !(*factor > 0.0)) {
        builder_.skip(n, "scale factor record without a positive factor");
        return;
    }
    // [SETX] 28: "plane scale factor", applied between an observation's
    // corrected view and its reduced one (29) - never in a raw distance.
    settings_.scaleFactor = *factor;
    settings_.scaleFactorState = survey::CorrectionState::NotApplied;
    settingsChanged(n, "the scale factor");
    builder_.countRead();
}

void SdrReader::readCollimation(Fields& fields, std::size_t n)
{
    const std::optional<double> vertical = angle(fields.real(), "vertical collimation", n);
    const std::optional<double> horizontal = angle(fields.real(), "horizontal collimation", n);
    if (!vertical && !horizontal) {
        builder_.skip(n, "collimation record with no value");
        return;
    }
    // [SETX] 29.2.5 corrects face 1 and face 2 readings by opposite amounts,
    // which the mean of the two faces cancels: kept, not applied.
    survey::SurveyStation* station = builder_.currentStation();
    std::map<std::string, std::string>& metadata =
        station != nullptr ? station->metadata : builder_.project().metadata;
    if (vertical) {
        metadata["collimation, vertical (radians)"] = formatExactReal(*vertical);
    }
    if (horizontal) {
        metadata["collimation, horizontal (radians)"] = formatExactReal(*horizontal);
    }
    builder_.countRead();
}

void SdrReader::readStation(Fields& fields, std::string_view derivation, std::size_t n)
{
    const std::string id = pointIdOf(fields.pointId());
    const std::string_view first = fields.real();
    const std::string_view second = fields.real();
    const std::string_view elevation = fields.real();
    const std::optional<double> height = length(fields.real(), "instrument height", n);
    const std::string description = cleaned(fields.take(16));
    if (id.empty()) {
        builder_.skip(n, "station record names no point");
        return;
    }
    finishSetup();
    if (!height) {
        builder_.warn(n, "the setup states no instrument height; 0 is used");
    }
    builder_.beginStation(id, height.value_or(0.0), settings_, n);
    orientation_ = Orientation{};
    setupObservations_ = 0;
    pendingChange_.clear();
    badSet_ = false;
    coordinates(id, first, second, elevation,
                derivation == "KI" ? survey::CoordinateSource::Entered
                                   : survey::CoordinateSource::Unknown,
                n);
    if (fatal_) {
        return;
    }
    if (!description.empty()) {
        builder_.codePoint(id, firstWord(description), description, {}, n);
    }
    builder_.countRead();
}

void SdrReader::readTarget(Fields& fields, std::size_t n)
{
    const std::optional<double> height = length(fields.real(), "target height", n);
    if (!height) {
        builder_.skip(n, "target height record with no height");
        return;
    }
    targetHeight_ = *height;
    targetHeightSeen_ = true;
    builder_.countRead();
}

void SdrReader::readBacksight(Fields& fields, std::size_t n)
{
    const std::string from = pointIdOf(fields.pointId());
    const std::string to = pointIdOf(fields.pointId());
    const std::optional<double> azimuth = angle(fields.real(), "backsight azimuth", n);
    const std::optional<double> reading = angle(fields.real(), "backsight circle reading", n);
    survey::SurveyStation* station = builder_.currentStation();
    if (station == nullptr) {
        builder_.skip(n, "backsight record with no station record before it");
        return;
    }
    if (from != station->setup.pointId) {
        builder_.skip(n, "backsight record from '" + from + "' while the setup is on '" +
                             station->setup.pointId + "'");
        return;
    }
    if (to.empty() || to == from) {
        builder_.skip(n, to.empty() ? std::string("backsight record names no backsight point")
                                    : "backsight record from '" + from + "' to itself");
        return;
    }
    const std::optional<double> wrappedAzimuth =
        azimuth ? std::optional<double>(wrapToCircle(*azimuth)) : std::nullopt;
    const std::optional<double> wrappedReading =
        reading ? std::optional<double>(wrapToCircle(*reading)) : std::nullopt;
    if (orientation_.set) {
        const auto sameCircle = [&]() {
            if (!wrappedReading || !orientation_.reading) {
                return wrappedReading.has_value() == orientation_.reading.has_value();
            }
            double difference = std::fabs(*wrappedReading - *orientation_.reading);
            difference = std::min(difference, 2.0 * kPi - difference);
            return difference <= kSameCircle;
        };
        const bool anotherRound =
            to == orientation_.backsight && wrappedAzimuth == orientation_.azimuth && sameCircle();
        if (anotherRound) {
            ++orientation_.rounds;
            station->metadata["backsight rounds"] = std::to_string(orientation_.rounds);
            if (wrappedReading) {
                std::string& readings = station->metadata["backsight circle readings (radians)"];
                readings += (readings.empty() ? "" : ", ") + formatExactReal(*wrappedReading);
            }
            builder_.countRead();
            return;
        }
        if (setupObservations_ > 0) {
            // The circle was oriented anew: what follows is read against
            // another zero, so it is a setup of its own ([SETX] 8.2).
            // The instrument height is the setup's: no 02 has moved it. The
            // settings are those in force now, which an 01 or 05 between
            // the rounds may have changed.
            finishSetup();
            const std::string point = station->setup.pointId;
            const double height = station->setup.instrumentHeight;
            const std::string previous = station->setup.id;
            station = &builder_.beginStation(point, height, settings_, n);
            station->metadata["begun by"] = "a backsight record re-orienting setup '" + previous +
                                            "' after its observations";
            setupObservations_ = 0;
            pendingChange_.clear();
        } else {
            builder_.warn(n, "a second backsight record before any observation replaces the "
                             "first (record " +
                                 std::to_string(orientation_.record) + ")");
            station->metadata.erase("backsight circle readings (radians)");
        }
    }
    orientation_ = Orientation{true, to, wrappedAzimuth, wrappedReading, 1, n};
    builder_.mentionPoint(to, n);
    station->backsightPointId = to;
    station->backsightAzimuth = wrappedAzimuth;
    station->metadata["backsight rounds"] = "1";
    if (wrappedReading) {
        station->metadata["backsight circle readings (radians)"] = formatExactReal(*wrappedReading);
    }
    builder_.countRead();
}

void SdrReader::readPosition(Fields& fields, std::string_view derivation, std::size_t n)
{
    const std::string id = pointIdOf(fields.pointId());
    const std::string_view first = fields.real();
    const std::string_view second = fields.real();
    const std::string_view elevation = fields.real();
    const std::string description = cleaned(fields.take(16));
    if (id.empty()) {
        builder_.skip(n, "coordinate record names no point");
        return;
    }
    survey::CoordinateSource how = survey::CoordinateSource::Unknown;
    if (derivation == "KI") {
        how = survey::CoordinateSource::Entered;
    } else if (derivation == "TP") {
        how = survey::CoordinateSource::FieldObserved;
    } else if (derivation == "AJ" || derivation == "TV" || derivation == "RS") {
        how = survey::CoordinateSource::Calculated;
    }
    builder_.mentionPoint(id, n);
    coordinates(id, first, second, elevation, how, n);
    if (fatal_) {
        return;
    }
    if (!description.empty()) {
        builder_.codePoint(id, firstWord(description), description, {}, n);
    }
    builder_.countRead();
}

void SdrReader::readObservation(Fields& fields, std::string_view derivation, std::size_t n)
{
    const std::string from = pointIdOf(fields.pointId());
    const std::string to = pointIdOf(fields.pointId());
    const std::string_view slopeField = fields.real();
    const std::string_view verticalField = fields.real();
    const std::string_view horizontalField = fields.real();
    const std::string description = cleaned(fields.take(16));
    survey::SurveyStation* setup = builder_.currentStation();
    if (derivation == "MC") {
        builder_.skip(n, "a corrected observation (MC) is derived from the raw face 1 and face 2 "
                         "observations the reduction works from; it is not imported");
        return;
    }
    if (derivation != "F1" && derivation != "F2" && derivation != "MD") {
        builder_.skip(n, "an observation with derivation code '" + std::string(derivation) +
                             "', which the format does not define for one");
        return;
    }
    if (setup == nullptr) {
        builder_.skip(n, "observation with no station record before it");
        return;
    }
    if (from != setup->setup.pointId) {
        builder_.skip(n, "observation from '" + from + "' while the setup is on '" +
                             setup->setup.pointId + "'");
        return;
    }
    if (to.empty() || to == from) {
        builder_.skip(n, to.empty() ? std::string("observation names no target point")
                                    : "observation from '" + from + "' to itself");
        return;
    }
    if (badSet_) {
        builder_.skip(n, "observation in " + badSetName_ + " (record " +
                             std::to_string(badSetRecord_) +
                             "), which the field book marks bad and does not use");
        return;
    }
    const std::optional<double> slope = length(slopeField, "slope distance", n);
    const std::optional<double> vertical = angle(verticalField, "vertical reading", n);
    const std::optional<double> horizontal = angle(horizontalField, "horizontal reading", n);

    if (!pendingChange_.empty()) {
        // One setup holds one set of settings, and the shots before this
        // record were made under the old ones.
        builder_.warn(pendingChangeRecord_,
                      pendingChange_ + " changes in the middle of setup '" + setup->setup.id +
                          "'; the setup keeps the values it began with, and these apply from "
                          "the next setup");
        setup->metadata[pendingChange_ + " changed at record " +
                        std::to_string(pendingChangeRecord_)] = "not applied to this setup";
        pendingChange_.clear();
    }
    if (!instrumentSeen_ && !instrumentWarned_) {
        instrumentWarned_ = true;
        builder_.warn(n, "no instrument record comes before the first observation; vertical "
                         "readings are read as zenith angles, the format's first option");
    }
    // A plain double and a flag, not a reassigned optional: GCC 16 at -O3
    // reports the optional's payload "maybe uninitialized" (a false positive
    // that -Werror turns into a build failure).
    survey::Face face = derivation == "F1"   ? survey::Face::Left
                        : derivation == "F2" ? survey::Face::Right
                                             : survey::Face::Unknown;
    bool hasZenith = false;
    double zenith = 0.0;
    if (vertical && vertical_ != VerticalReference::Unknown) {
        double reading = *vertical;
        if (vertical_ == VerticalReference::Horizon) {
            reading = wrapToCircle(kPi / 2.0 - reading);
        }
        if (reading < 0.0 || reading >= 2.0 * kPi) {
            builder_.warn(n, "vertical reading outside a full circle; the zenith is not "
                             "imported");
        } else {
            const survey::Face readingFace = faceOfZenithReading(reading);
            if (face == survey::Face::Unknown) {
                face = readingFace;
            } else if (readingFace != survey::Face::Unknown && readingFace != face) {
                builder_.warn(n, std::string(derivation) + " says " + survey::toString(face) +
                                     ", but its vertical reading is on " +
                                     survey::toString(readingFace) +
                                     "; the record's face is kept");
            }
            zenith = zenithInModelRange(reading);
            hasZenith = true;
        }
    }
    if (!targetHeightSeen_ && !targetHeightWarned_) {
        targetHeightWarned_ = true;
        builder_.warn(n, "no target height record comes before this observation; 0 is used "
                         "until one does");
        builder_.notCarried("no target height for the observations before the first target "
                            "height record (0 was used)");
    }
    builder_.mentionPoint(to, n);
    if (!description.empty()) {
        builder_.codePoint(to, firstWord(description), description, {}, n);
    }

    const survey::ObservationPrecision& precision = builder_.options().precision;
    const double hi = setup->setup.instrumentHeight;
    const survey::Pointing pointing{builder_.nextPointing(), face};
    bool any = false;
    if (horizontal) {
        auto& observation = builder_.stationObservation<survey::HorizontalDirectionObservation>(n);
        observation.at = setup->setup.pointId;
        observation.to = to;
        observation.direction = wrapToCircle(*horizontal);
        observation.sigma = precision.direction;
        observation.pointing = pointing;
        any = true;
    }
    if (hasZenith) {
        auto& observation = builder_.stationObservation<survey::ZenithAngleObservation>(n);
        observation.from = setup->setup.pointId;
        observation.to = to;
        observation.angle = zenith;
        observation.sigma = precision.zenith;
        observation.instrumentHeight = hi;
        observation.targetHeight = targetHeight_;
        observation.pointing = pointing;
        any = true;
    }
    if (slope) {
        if (!(*slope > 0.0)) {
            builder_.warn(n, "slope distance of " + formatExactReal(*slope) +
                                 " read as no distance measured");
        } else {
            auto& observation = builder_.stationObservation<survey::DistanceObservation>(n);
            observation.from = setup->setup.pointId;
            observation.to = to;
            observation.distance = *slope;
            observation.sigma = survey::distanceSigma(precision, *slope);
            observation.kind = survey::DistanceKind::Slope;
            observation.instrumentHeight = hi;
            observation.targetHeight = targetHeight_;
            observation.pointing = pointing;
            any = true;
        }
    }
    if (!any) {
        builder_.skip(n, "observation with no readable value");
        return;
    }
    ++setupObservations_;
    builder_.countRead();
}

void SdrReader::readReduced(Fields& fields, std::size_t n)
{
    const std::string from = pointIdOf(fields.pointId());
    const std::string to = pointIdOf(fields.pointId());
    const std::optional<double> azimuth = angle(fields.real(), "azimuth", n);
    const std::string_view horizontalField = fields.real();
    const std::string_view verticalField = fields.real();
    const std::string description = cleaned(fields.take(16));
    if (from.empty() || to.empty()) {
        builder_.skip(n, "reduced observation record names no point");
        return;
    }
    if (!trimmed(horizontalField).empty() || !trimmed(verticalField).empty()) {
        builder_.skip(n, "a reduced observation (azimuth, horizontal and vertical distance) is "
                         "derived from the raw observations the reduction works from; it is not "
                         "imported");
        return;
    }
    if (!azimuth) {
        builder_.skip(n, "reduced observation record with no value");
        return;
    }
    // An azimuth and nothing else: an orientation keyed for the line.
    builder_.mentionPoint(from, n);
    builder_.mentionPoint(to, n);
    survey::SurveyStation* station = builder_.currentStation();
    const bool here = station != nullptr && station->setup.pointId == from;
    std::map<std::string, std::string>& metadata =
        here ? station->metadata : builder_.project().metadata;
    const std::string key = here ? "azimuth to " + to : "azimuth " + from + " to " + to;
    metadata[key + " (radians)"] = formatExactReal(wrapToCircle(*azimuth));
    if (!description.empty()) {
        metadata[key + ", description"] = description;
    }
    builder_.countRead();
}

void SdrReader::readSet(Fields& fields, std::size_t n)
{
    const std::string from = pointIdOf(fields.pointId());
    const std::string count = cleaned(fields.take(3));
    std::string setNumber;
    std::string bad;
    if (variant_ == Variant::Sdr33) {
        // [SDR] 3.6.2: number of the set, bad marker, return sight, order.
        setNumber = cleaned(fields.take(3));
        bad = cleaned(fields.take(1));
    }
    badSet_ = bad == "2";
    badSetRecord_ = n;
    badSetName_ =
        "set " + (setNumber.empty() ? std::string("?") : setNumber) + " at '" + from + "'";
    if (badSet_) {
        builder_.warn(n, badSetName_ + " is marked bad; its observations are not imported");
    }
    if (survey::SurveyStation* station = builder_.currentStation(); station != nullptr) {
        station->metadata["set at record " + std::to_string(n)] =
            (setNumber.empty() ? std::string{} : "number " + setNumber + ", ") + count +
            " observation(s)" + (badSet_ ? ", marked bad" : "");
    }
    builder_.countRead();
}

void SdrReader::readDistanceUnitNote(std::string_view text, std::size_t n)
{
    // [SDR] 3.3.2: "13DU1:Meters:", "13DU2:Feet:", "13DU3:US Feet:"; "The
    // numeric code directly after DU specifies which distance unit ...
    // The text is informational only." It is sent to say US feet where
    // the header can say only feet.
    const char code = text.empty() ? ' ' : text.front();
    survey::LinearUnit unit = survey::LinearUnit::Unknown;
    switch (code) {
    case '1':
        unit = survey::LinearUnit::Metres;
        break;
    case '2':
        unit = survey::LinearUnit::Feet;
        break;
    case '3':
        unit = survey::LinearUnit::UsSurveyFeet;
        break;
    default:
        refuse(n, "the distance unit note gives the code '" + std::string(1, code) +
                      "'; the format defines 1 metres, 2 feet and 3 US feet");
        return;
    }
    const bool agrees = unit == linearUnit_ || (unit == survey::LinearUnit::UsSurveyFeet &&
                                                linearUnit_ == survey::LinearUnit::Feet);
    if (!agrees) {
        refuse(n, "the distance unit note says " + std::string(survey::toString(unit)) +
                      " where the header (record " + std::to_string(headerRecord_) + ") says " +
                      survey::toString(linearUnit_) +
                      "; Katana will not choose between them");
        return;
    }
    linearUnit_ = unit;
    metres_ = survey::metresPer(unit).value();
    builder_.project().units.linear = unit;
    builder_.countRead();
}

void SdrReader::readNote(Fields& fields, std::string_view derivation, std::size_t n)
{
    const std::string text = cleaned(fields.take(60));
    if (derivation == "DU") {
        readDistanceUnitNote(text, n);
        return;
    }
    if (text.empty()) {
        builder_.countRead();
        return;
    }
    if (derivation == "CP") {
        readCorrectionNote(text);
    }
    if (derivation == "TS") {
        if (const std::optional<survey::SurveyTimestamp> time = timeStampOf(text)) {
            survey::SurveyStation* station = builder_.currentStation();
            if (station != nullptr && !station->instrument.time.known()) {
                station->instrument.time = *time;
            }
        } else {
            builder_.warn(n, "time stamp '" + text.substr(0, 60) +
                                 "' is not in a form the format's writers use; it is kept as a "
                                 "note");
        }
    }
    builder_.addStationNote(derivation == "NM" ? text : std::string(derivation) + ": " + text);
    builder_.countRead();
}

void SdrReader::dispatch(std::string_view line, std::size_t n)
{
    if (!headerRead_) {
        // [SDR] chapter 3: "The first data record of each transmission is
        // always of type 00."
        if (!line.starts_with("00") || !isRecord(line)) {
            refuse(n, "the file does not begin with an SDR header record (type 00)");
            return;
        }
        readHeader(line, n);
        return;
    }
    std::size_t marks = 0;
    const std::string_view live = withoutDeletionMarks(line, marks);
    if (marks > 0) {
        builder_.skip(n, "a deleted record (marked '" + std::string(2 * marks, 'D') + "')" +
                             (isRecord(live) ? ", " + recordLabel(live) + "," : std::string{}) +
                             " is not imported");
        return;
    }
    if (!isRecord(line)) {
        builder_.skip(n, "'" + cleaned(line.substr(0, 12)) +
                             "' is not a record: a record opens with a two-digit type and a "
                             "derivation code");
        return;
    }
    if (!nonAscii_) {
        for (const char c : line) {
            if (static_cast<unsigned char>(c) >= 0x80) {
                nonAscii_ = true;
                builder_.warn(n, "the record holds characters outside the ASCII the format "
                                 "allows; its fields are read by character position");
                break;
            }
        }
    }
    const std::string_view type = line.substr(0, 2);
    const std::string_view derivation = line.substr(2, 2);
    Fields fields(line, pointIdWidth_, realWidth_);
    if (type == "00") {
        readHeader(line, n);
    } else if (type == "01") {
        readInstrument(fields, n);
    } else if (type == "02") {
        readStation(fields, derivation, n);
    } else if (type == "03") {
        readTarget(fields, n);
    } else if (type == "04") {
        readCollimation(fields, n);
    } else if (type == "05") {
        readAtmosphere(fields, n);
    } else if (type == "06") {
        readScale(fields, n);
    } else if (type == "07") {
        readBacksight(fields, n);
    } else if (type == "08") {
        readPosition(fields, derivation, n);
    } else if (type == "09") {
        readObservation(fields, derivation, n);
    } else if (type == "10") {
        readJob(fields, n);
    } else if (type == "11") {
        readReduced(fields, n);
    } else if (type == "12") {
        readSet(fields, n);
    } else if (type == "13") {
        readNote(fields, derivation, n);
    } else {
        builder_.skip(n, "record " + recordLabel(line) + " is of a type this reader does not "
                                                         "import");
    }
}

Result<ReadResult> SdrReader::read(std::string_view text, bool guessedEncoding,
                                   katana::core::TextEncoding encoding)
{
    if (guessedEncoding) {
        builder_.warn(0, "the file is not UTF-8; it was read as " +
                             std::string(katana::core::toString(encoding)) +
                             ", so an accented name may be wrong");
    }
    std::size_t n = 0;
    std::size_t start = 0;
    while (start < text.size() && !fatal_) {
        // CR LF ([SDR] chapter 3), LF or a lone CR: files are copied between
        // systems, and the probe (probeLines) takes all three.
        std::size_t end = text.find_first_of("\r\n", start);
        if (end == std::string_view::npos) {
            end = text.size();
        }
        const std::string_view line = text.substr(start, end - start);
        start = end + 1;
        if (end < text.size() && text[end] == '\r' && start < text.size() && text[start] == '\n') {
            ++start;
        }
        ++n;
        if (trimmed(line).empty()) {
            continue;
        }
        // [SDR] chapter 3: a transmission may open with STX and close with
        // ETX and a checksum. Neither is a record; the checksum is kept as
        // written and not checked.
        if (line.front() == '\x02') {
            continue;
        }
        if (line.front() == '\x03') {
            builder_.project().metadata["transmission checksum"] = cleaned(line.substr(1));
            continue;
        }
        dispatch(line, n);
    }
    if (fatal_) {
        return *fatal_;
    }
    if (!headerRead_) {
        return makeError(ErrorCode::FileImportFailure,
                         builder_.fileName() + " holds no SDR header record, so its units and "
                                               "its layout are unknown",
                         "Sokkia SDR");
    }
    finishSetup();
    survey::SurveyProject& project = builder_.project();
    if (!project.stations.empty()) {
        std::string atmospheric =
            "whether the distances carry the atmospheric correction: the field book applies it "
            "as each distance is accepted when the job turns it on, and another writer of the "
            "format turns the job's switch on and leaves it out";
        if (!atmosphericSwitch_.empty()) {
            atmospheric += " (this job turns it " + atmosphericSwitch_ + ")";
        }
        builder_.notCarried(std::move(atmospheric) + ", so the reduction applies none unless "
                                                     "told to recompute it");
        if (!weatherSeen_) {
            builder_.notCarried("no pressure and temperature to compute an atmospheric "
                                "correction from");
        }
        if (!instrumentSeen_) {
            builder_.notCarried("no instrument record: the prism constant, and whether the "
                                "distances carry one, are unknown");
        }
    }
    ReadResult result = builder_.finish();
    if (result.project.stations.empty() && result.project.points.empty() &&
        result.project.unpositionedPoints.empty()) {
        return makeError(ErrorCode::FileImportFailure,
                         result.project.source.fileName +
                             " holds no setup, observation or point Katana could read",
                         std::to_string(result.warnings.size()) + " warning(s)" +
                             (result.warnings.empty()
                                  ? std::string{}
                                  : ", the first: " +
                                        katana::surveyio::describe(result.warnings.front())));
    }
    return result;
}

// ---- Registration -----------------------------------------------------------------

// "00" and a derivation code, then "SDR2" or "SDR33" where the version
// field begins ([SDR] 3.2.5; [NIKON] writes "SDR33V04-01" with no blank).
bool isHeader(std::string_view line)
{
    return line.size() >= 9 && line[0] == '0' && line[1] == '0' && isCodeChar(line[2]) &&
           isCodeChar(line[3]) && line.substr(4).starts_with("SDR") &&
           (line[7] == '2' || line.substr(7, 2) == "33");
}

FormatSignature probe(const ProbeInput& input)
{
    const std::string_view bytes = katana::surveyio::withoutByteOrderMark(input.bytes);
    if (!katana::surveyio::looksLikeText(bytes)) {
        return katana::surveyio::ruledOut();
    }
    std::size_t lines = 0;
    std::size_t shaped = 0;
    std::string_view first;
    for (const std::string_view line : katana::surveyio::probeLines(input, 200)) {
        if (trimmed(line).empty() || line.front() == '\x02' || line.front() == '\x03') {
            continue;
        }
        if (lines == 0) {
            first = line;
        }
        ++lines;
        std::size_t marks = 0;
        if (isRecord(withoutDeletionMarks(line, marks))) {
            ++shaped;
        }
    }
    if (lines == 0) {
        return katana::surveyio::ruledOut();
    }
    const bool sdr = input.extension == "sdr";
    const bool header = isHeader(first);
    const bool mostlyRecords = shaped * 10 >= lines * 9;
    const std::string counted = std::to_string(shaped) + " of " + std::to_string(lines) +
                                " lines open with a record type and derivation code";
    if (header) {
        const std::string version = collapsedBlanks(first.substr(4, 16));
        if (mostlyRecords) {
            return {sdr ? 0.98 : 0.95, std::string(sdr ? "extension .sdr, " : "") + "header " +
                                           version + ", " + counted};
        }
        return {0.6, "header " + version + " but only " + counted};
    }
    if (sdr && mostlyRecords) {
        return {0.4, "extension .sdr and " + counted + ", but no SDR header record"};
    }
    if (sdr) {
        return {0.1, "extension .sdr only"};
    }
    return katana::surveyio::ruledOut();
}

FormatDescriptor descriptor()
{
    FormatDescriptor format;
    format.id = std::string(kFormatId);
    format.humanName = "Sokkia SDR33 / SDR2x";
    format.manufacturer = katana::surveyio::Manufacturer::Sokkia;
    format.reads = {.points = true,
                    .observations = true,
                    .stations = true,
                    .features = true,
                    .coordinateSystem = false,
                    .instrumentSettings = true,
                    .gnss = false};
    format.canImport = true;
    format.parserVersion = "1.0";
    format.extensions = {"sdr"};
    return format;
}

Result<ReadResult> readSdr(std::string_view bytes, std::string_view fileName,
                           const ReadOptions& options)
{
    Result<katana::core::DecodedText> decoded = katana::core::decodeText(bytes);
    if (!decoded) {
        return decoded.error();
    }
    const std::string_view text = decoded->text;
    // The header names the variant, which every record's provenance carries.
    std::string variant = "SDR";
    std::string version;
    for (std::size_t start = 0; start < text.size();) {
        std::size_t end = text.find_first_of("\r\n", start);
        if (end == std::string_view::npos) {
            end = text.size();
        }
        const std::string_view line = trimmed(text.substr(start, end - start));
        start = end + 1;
        if (line.empty() || line.front() == '\x02') {
            continue;
        }
        if (isHeader(line)) {
            version = collapsedBlanks(cleaned(line.substr(4, 16)));
            variant = version.starts_with("SDR33") ? "SDR33" : "SDR2x";
        }
        break;
    }
    SdrReader reader(fileName,
                     survey::SourceRecord{"Sokkia", variant, version, std::string(fileName), 0},
                     options);
    return reader.read(text, decoded->guessed, decoded->encoding);
}

const katana::surveyio::FormatRegistration kRegistration{descriptor(), &probe, &readSdr};

} // namespace
