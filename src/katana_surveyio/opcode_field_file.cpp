// The opcode field file (.fld): a survey job as tab-separated records, each
// opening with a numeric operation code - total-station setups, resections
// and shots, entered coordinates (RTK positions among them), coded strings and
// their attributes.
//
// Specification this reader implements:
//   [FLD] "Field File Format", the format publisher's reference manual
//         chapter, in the edition the owner supplied: sections 44.2
//         "Structure of the .fld File", 44.3 "Point Description", 44.4
//         "Measurements and Named Measurements", 44.5 "Searching for Special
//         Coordinates", 44.6 (time_text), 44.7 and 44.8, one entry per opcode.
//         The June 2025 edition numbers the same sections 1.2 to 1.8; it has
//         the entries and .fld syntax of 128, 129, 138 and 139, but not their
//         descriptions, nor 44.7's resection_measurement block.
//
// What [FLD] says, and this reader relies on:
//   * 44.2: a record is "a numeric operation code ... followed by zero or
//     more tabs and pieces of information", so an opcode alone on its line
//     (20, 99, 129) is a record.
//   * 44.3: a POINT DESCRIPTION is five tab-separated values - feature code,
//     string number, point ID, point name, point comment - in that order, a
//     missing one still taking its tab.
//   * 44.4: a measurement (02, 07 among the six kinds) makes a point that "is
//     appended to previous points with the same feature code and string
//     number": a feature is a code and a string number, strung in the order
//     measured. The CURRENT MEASUREMENT POINT is the last one made, and the
//     CURRENT STRING the string it was appended to.
//   * 44.8, per opcode ("Optional information is enclosed in square
//     brackets"):
//       100 units, "only one choice for each": decimal degrees, metres, and
//           millimetres and celsius for pressure and temperature.
//       02  a "directly entered coordinate": description, X, Y, Z, of which
//           "No reduction is needed".
//       03  a setup on the point the description names, with its instrument
//           height. 128 a setup on an unknown point, whose coordinates "a
//           Least Squares Resection" computes from measurements to known
//           points; 129 ends its block, which "In some cases ... is not
//           necessary". 138 and 139 are the same for a Helmert resection.
//       04  the backsight: description, horizontal circle, vertical circle,
//           slope distance and an azimuth_value, which "may be specified when
//           no coordinate for the backsight point exists". 06 a check
//           measurement and 07 a measurement: description, horizontal and
//           vertical circle, slope distance; decimal degrees and metres.
//       05  the target height for 04, 06 and 07 after it; 09 a scale factor
//           "to apply to subsequent slope distances".
//       16  "Additional coding for the current measurement point": "A new
//           measurement point is created at the same position", strung by
//           this record's code and string number.
//       20  close string: with no description "the current string is
//           closed"; with one, the string its code and number (else its
//           point) names.
//       42, 43, 44  a radial, tangential or height offset of "the current
//           measured point", or of the point a description names. Radial:
//           "along the plan line joining the current station to the specified
//           point", positive away from the station; tangential: "at rights
//           angles" to that line, "negative ... to the left (looking from the
//           station)"; height: added to a height that "is not null". Each is
//           "from the specified points original position".
//       29  a memo; 41 text for the current measurement point, "any spaces
//           from column four onwards" being part of it; 71, 72, 73 an
//           integer, a real and a text attribute (name, value) of it, an
//           attribute whose name is blank being "unnamed"; -2 a comment.
//       99  "Stop processing ... at this line."
//       124 and 125 (an attribute group and its end), 140 (a GNSS coordinate)
//           and 145: "This opcode does not exist in the fld file."
//
// Readings that are this reader's own, stated so they can be checked:
//   * THE COLUMN AFTER THE OPCODE. The files this reader was written against
//     put one field more than [FLD]'s syntax at the front of every record
//     with values ("07<tab><tab>KJ<tab>01..."): their 04 has as many fields as
//     [FLD]'s 04 with its azimuth, and their 128 a description and a value
//     [FLD]'s .fld syntax does not give it. Where the column is filled it
//     holds the date and time the record was made ("05/05/26 00:49:06.52" on
//     each coordinate of the RTK job) - the counterpart of the XML form's
//     time_surveyed, though [FLD] gives the op_code_properties that carry it
//     to the XML form only. The records whose opcode has a fixed
//     number of values (02, 03, 04, 06, 07) decide the file's layout before
//     any is read: one whose field count fits only the column's layout, its
//     first field blank or a date or time, votes for the column; one whose
//     count fits only [FLD]'s, its first field filled, votes against. A 04 of
//     ten fields fits both (the column and no azimuth, or the azimuth and no
//     column), as does a record one field longer than [FLD]'s whose last is
//     blank (a trailing tab, or the column and a blank last value): neither
//     votes. A record is not trusted to decide alone because one of each can
//     look the same: a record in the column's layout that has lost a value
//     has [FLD]'s count, and a feature code left empty makes its first field
//     empty. A fixed record that does not fit the file's layout is a warning
//     and is skipped - a guess at which value is the slope distance reads a
//     plausible survey that is wrong - and so is one that fits both layouts
//     in a file whose records show both, unless its first field is a date or
//     time. One blank field past a layout's count is a trailing tab. A record
//     of free text takes the file's layout: in a file with the column, a
//     filled first field that is not a date or time is read as a record
//     written without it; in a file without it, a record of fixed form (05,
//     09, 100, the offsets, 16, 20, 128) whose first value is blank and that
//     fits only without that value carries the column. A date or time is
//     known by its shape (isDateOrTime) and kept as text, "time stamp", on the
//     point or setup its record makes: which of "05/05/26" is the day the
//     file does not say.
//   * An opcode may have a blank before it (" 2") or lack its leading zero
//     ("7"): [FLD] calls it numeric and writes 1 to 9 as 01 to 09, and
//     nothing makes 07 differ from 7.
//   * The first line of the files is "{Version 6.0}", which [FLD] does not
//     mention: kept as "version". A line like it after the first record is no
//     record.
//   * X is the easting and Y the northing. [FLD] says only "(x, y, z)"; the
//     files put six-figure eastings in X and seven-figure MGA northings in Y,
//     and one labels a reference station's Easting and Northing so.
//   * A point is identified by its point name where it has one and by its
//     point ID otherwise ([FLD] 44.5 finds a known point by either). A fixed
//     record with neither is skipped.
//   * The vertical circle is a zenith reading: [FLD] never states its zero,
//     and the files' readings near 90 and 270 degrees are zeniths on the two
//     faces. One past 180 degrees is face right, held as 360 - z.
//   * AN RTK POSITION WRITTEN AS 02 IS AN ENTERED COORDINATE. [FLD] gives a
//     GNSS coordinate opcode 140 and says 140 "does not exist in the fld
//     file", so a .fld carries an RTK position as an 02, its "directly entered
//     coordinate", with attributes after it that say what the receiver solved
//     ("GPS Information/GNSS Solution"). It is CoordinateSource::Entered like
//     every 02: the reduction takes an entered coordinate as a known point, as
//     it takes a GNSS position, so a total-station setup on an RTK mark
//     orients on another. Its Z is the mark's, as [FLD]'s 02 needs "No
//     reduction"; an antenna height among its attributes is kept, not
//     subtracted again.
//   * THE ATTRIBUTES OF A POINT MEASURED MORE THAN ONCE. [FLD] makes every
//     measurement a point of its own; Katana makes one point of a name and
//     keeps every measurement's attributes on it. The first record that makes
//     a point current (02, 04, 06, 07) owns the plain names; the time stamp
//     and attributes after each later record about the same point - a face
//     pair, a mark measured again, a check, a backsight - are kept under
//     "record N/", N that record. 16 makes no second point here (see below),
//     so what follows it is still the current record's; a name given twice
//     there keeps its second value, where it differs, as "Name (record M)",
//     M the attribute's record, with a warning.
//   * ATTRIBUTE GROUPS (124 and 125). [FLD] defines them for the XML form
//     only (a name and a level, the group running "until" 125); the files
//     write 124 with an empty value, the name and the level, and 125 with an
//     empty name and the level. An attribute inside a group is kept as
//     "Group/Name" - a reference station's "Easting" is not the point's - and
//     nested groups as "Outer/Inner/Name", "/" being how Katana's properties
//     show a tree. A group still open when a record makes another point or a
//     setup, or when a 124 gives the level of a group still open, is ended
//     there, with a warning.
//   * 04 and 06 targets take the attributes after them as a shot's does: the
//     files record the date, time and target height of a backsight and of a
//     check that way. A check (06) does not string its target ([FLD] makes a
//     check a one-vertex string of its own): "check measurement" on the point
//     names the setup and the check's coding. A backsight's (04) coding is
//     kept in its setup's metadata, and a setup's (03, 128): [FLD] strings a
//     measurement point, which neither makes.
//   * THE BACKSIGHT'S AZIMUTH. SurveyStation::backsightAzimuth is what the
//     reduction orients a setup by when its backsight has no coordinates:
//     the 04's azimuth_value where the file gives one (the orientation is then
//     that azimuth less the circle reading on the backsight), and the circle
//     reading otherwise (the circle taken as set to a grid azimuth).
//   * A record that makes a point but is not read leaves no current point,
//     and a setup that is not read leaves no setup: what follows is skipped,
//     naming that record, rather than given to the point or setup before.
//   * OFFSETS (42, 43, 44) are applied to the one shot (07) of their point
//     from the current setup: its direction, zenith and slope distance become
//     those of the offset position, worked from what was measured with every
//     offset of that shot so far, and what was measured is kept in the
//     point's metadata ("shot record N as measured"). They are applied when
//     the setup ends, because a later shot or check of the same point from
//     the setup would make it one of several, and which of those an offset
//     corrects [FLD] does not say: an offset of such a point, or of a point
//     not shot from the current setup (an entered coordinate, a shot from an
//     earlier setup), or one that puts its point on the station, is kept in
//     the point's metadata and not applied, with a warning. The arithmetic
//     and its error are at applyOffsets.
//   * 128 (and 138) starts a setup on the point its description names:
//     [FLD]'s .fld syntax has the height only, and the files write the
//     description first, as the XML form carries one, and an empty value
//     after the height. With no point named the setup is on "resection at
//     record N". The file states no coordinates for it and the reduction
//     computes no resection, which notCarried says.
//   * 16 strings the current measurement point into the second string as
//     well, rather than making a second point at its position: one position,
//     as [FLD]'s two are. A point ID or name the 16 gives is kept with the
//     point, with a warning.
//   * 29 is a note on the setup (the project's before the first): [FLD] puts
//     a memo in its check-measurement model, which Katana does not have, and
//     a setup's notes are where a person reads a job. 41 is kept whole as
//     "additional text N" on the point: [FLD] appends it to the vertex's text,
//     and the point's description is already its comment.
//   * COMMENTS ("//", which [FLD] does not mention). Those before the first
//     record that makes a point or a setup are the header: a "Key : value"
//     one is kept in the project's metadata as "header: Key", any other in
//     "header", and "Coordinate System" (with its "Zone") is the system the
//     file declares - by name, exactly as written, never turned into an EPSG
//     code here. A later comment (a resection's residuals, an antenna height
//     change) is a note on the setup it follows, verbatim. A rule of dashes
//     is decoration either way.
//   * 72 "Target height" and "Prism constant" are attributes of the point like
//     any other: the target height that applies is 05's, which the format
//     defines as such, and the prism constant has no unit in the file.
//   * An opcode [FLD] defines only for its XML form (140, 145 ...) has no .fld
//     layout to read and is skipped, saying so. 124 and 125 are read because
//     the files show their layout, and one misread can only misname an
//     attribute, never move a point.
//   * 100: angle and distance units other than decimal degrees and metres are
//     refused rather than guessed at; the pressure and temperature units are
//     not read, since nothing here uses them (the files write "millibars").

#include <algorithm>
#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <numbers>
#include <optional>
#include <string>
#include <string_view>
#include <unordered_map>
#include <unordered_set>
#include <vector>

#include "katana/core/text.hpp"
#include "katana/core/text_encoding.hpp"
#include "katana/math/numerics.hpp"
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
// Where a refusal points a person: [FLD]'s entries, one per opcode.
constexpr std::string_view kSpecification = "Field File Format 44.8";
constexpr double kPi = std::numbers::pi;
constexpr double kRadiansPerDegree = kPi / 180.0;
// Records the probe reads before it decides: enough to pass a header of
// comments and reach the survey (kProbeBytes caps the lines it is given).
constexpr std::size_t kProbeRecords = 200;

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

bool isDigit(char c)
{
    return c >= '0' && c <= '9';
}

// An opcode: an optional minus and one to three digits ("02", "100", "-2").
// The caller trims: " 2" is opcode 2.
std::optional<int> opcodeOf(std::string_view text)
{
    const bool negative = text.starts_with('-');
    const std::string_view digits = negative ? text.substr(1) : text;
    if (digits.empty() || digits.size() > 3) {
        return std::nullopt;
    }
    int value = 0;
    for (const char c : digits) {
        if (!isDigit(c)) {
            return std::nullopt;
        }
        value = value * 10 + (c - '0');
    }
    return negative ? -value : value;
}

// "07", "42", "-2": an opcode as [FLD]'s syntax lines write it, whatever
// padding the file gave it.
std::string opcodeText(int opcode)
{
    return (opcode >= 0 && opcode < 10 ? "0" : "") + std::to_string(opcode);
}

bool blank(std::string_view text)
{
    return trimmed(text).empty();
}

bool isLetter(char c)
{
    return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z');
}

// Whether `text` reads as a date, a time or both, the way a controller writes
// one: "05/05/26 00:49:06.52" (the RTK job), [FLD] 44.6's time_text
// "2015-09-28T06:42:45Z", "00:49:06", "05/05/2026", "5 May 2026 12:49 PM",
// "2026-05-05T00:49:06+1000". Digits; the separators / - : . , + and blanks;
// and only the words of a date or time - T, Z, AM, PM, UTC, GMT, and a month
// standing apart from any digit. At least one digit, and a / - or : between
// two digits or a month, so that a number alone ("12", "1.5") and a feature
// code ("KB", "MAR1") are not one. Nothing is read out of it (see the top of
// this file).
bool isDateOrTime(std::string_view text)
{
    static constexpr std::array<std::string_view, 6> kWords = {"t", "z", "am", "pm", "utc", "gmt"};
    static constexpr std::array<std::string_view, 24> kMonths = {
        "jan",   "feb",     "mar",      "apr",     "may",     "jun",
        "jul",   "aug",     "sep",      "sept",    "oct",     "nov",
        "dec",   "january", "february", "march",   "april",   "june",
        "july",  "august",  "september", "october", "november", "december"};
    text = trimmed(text);
    bool digit = false;
    bool separated = false;
    bool month = false;
    for (std::size_t i = 0; i < text.size();) {
        const char c = text[i];
        if (isDigit(c)) {
            digit = true;
            ++i;
        } else if (c == '/' || c == '-' || c == ':') {
            if (i > 0 && i + 1 < text.size() && isDigit(text[i - 1]) && isDigit(text[i + 1])) {
                separated = true;
            }
            ++i;
        } else if (c == '.' || c == ',' || c == '+' || c == ' ') {
            ++i;
        } else if (isLetter(c)) {
            std::size_t end = i;
            while (end < text.size() && isLetter(text[end])) {
                ++end;
            }
            const std::string word = katana::core::lowered(text.substr(i, end - i));
            const bool apart = (i == 0 || !isDigit(text[i - 1])) &&
                               (end == text.size() || !isDigit(text[end]));
            if (apart && std::find(kMonths.begin(), kMonths.end(), word) != kMonths.end()) {
                month = true;
            } else if (std::find(kWords.begin(), kWords.end(), word) == kWords.end()) {
                return false;
            }
            i = end;
        } else {
            return false;
        }
    }
    return digit && (separated || month);
}

// A comment that is only a rule ("//------") or nothing.
bool isDecoration(std::string_view text)
{
    return text.find_first_not_of("-=_*~ \t") == std::string_view::npos;
}

// The values after a point description that [FLD] gives each opcode with one;
// nullopt for an opcode without a description.
std::optional<std::size_t> valuesAfterDescription(int opcode)
{
    switch (opcode) {
    case 2: return 3;  // X Y Z
    case 3: return 1;  // instrument height
    case 4: return 3;  // HA VA SD, and the azimuth countFits allows
    case 6: return 3;  // HA VA SD
    case 7: return 3;  // HA VA SD
    default: return std::nullopt;
    }
}

// 1 (opcode) + `columns` + 5 (description) + the values: whether `count`
// fields are the fixed record [FLD] gives, with the column after the opcode
// (`columns` 1) or without it. 04's azimuth may be left off.
bool countFits(int opcode, std::size_t count, std::size_t columns)
{
    const std::optional<std::size_t> arity = valuesAfterDescription(opcode);
    if (!arity) {
        return false;
    }
    const std::size_t plain = 1 + columns + 5 + *arity;
    return count == plain || (opcode == 4 && count == plain + 1);
}

// The layouts a fixed record's fields fit: with the column after the opcode,
// and without it. One blank field past a layout's count is a trailing tab.
struct LayoutFit {
    bool column = false;
    bool plain = false;
};

LayoutFit layoutFit(int opcode, const std::vector<std::string_view>& fields)
{
    const std::size_t count = fields.size();
    const bool trailing = count > 1 && blank(fields.back());
    const auto fits = [&](std::size_t columns) {
        return countFits(opcode, count, columns) ||
               (trailing && countFits(opcode, count - 1, columns));
    };
    return {fits(1), fits(0)};
}

// For a record of free fields whose values nonetheless have a fixed form - a
// number, a list of units, the offsets' and 128's optional description, 16's
// and 20's - whether `values` fit that form; nullopt for text, whose first
// value may be anything, blank included.
std::optional<bool> fixedFormFits(int opcode, const std::vector<std::string_view>& values)
{
    const std::size_t count = values.size();
    const bool firstFilled = count > 0 && !blank(values[0]);
    switch (opcode) {
    case 5:
    case 9: return count == 1 && firstFilled;
    case 100: return count >= 2 && firstFilled;
    case 42:
    case 43:
    case 44: return count == 1 || count == 6;
    case 128:
    case 138: return count == 1 || count == 6 || (count == 7 && blank(values[6]));
    case 16: return count == 5;
    case 20:
        return count == 5 || std::all_of(values.begin(), values.end(),
                                          [](std::string_view value) { return blank(value); });
    default: return std::nullopt;
    }
}

// Opcodes that make a point or a setup, read or not: the header comments end
// at the first of them, and so does an attribute group left open.
bool makesPointOrSetup(int opcode)
{
    switch (opcode) {
    case 2: case 3: case 4: case 6: case 7: case 10: case 11: case 12: case 128: case 138:
    case 140:
        return true;
    default:
        return false;
    }
}

bool startsSetup(int opcode)
{
    return opcode == 3 || opcode == 128 || opcode == 138;
}

// [FLD] 44.8's name for each opcode this reader does not read, so a skipped
// record says what it was.
struct UnreadOpcode {
    int opcode = 0;
    std::string_view name;
    // "This opcode does not exist in the fld file": defined for the XML form.
    bool xmlOnly = false;
    // A measurement point [FLD] 44.4 counts as current: nothing after it may
    // be given to the point before.
    bool makesPoint = false;
    // A correction to the measurements after it.
    bool correctsLater = false;
    // A field template, from which the measurements after it may take their
    // feature codes and string numbers.
    bool codesLater = false;
};

constexpr UnreadOpcode kUnreadOpcodes[] = {
    {.opcode = -3, .name = "group"},
    {.opcode = -1, .name = "error text"},
    {.opcode = 1, .name = "job data"},
    {.opcode = 10, .name = "stadia measurement", .makesPoint = true},
    {.opcode = 11, .name = "measurement by horizontal distance and height", .makesPoint = true},
    {.opcode = 12, .name = "measurement by horizontal distance and height difference",
     .makesPoint = true},
    {.opcode = 14, .name = "check coordinate"},
    {.opcode = 15, .name = "vertical circle correction", .correctsLater = true},
    {.opcode = 17, .name = "arc through the last three points"},
    {.opcode = 18, .name = "circle feature"},
    {.opcode = 19, .name = "reverse string"},
    {.opcode = 21, .name = "join strings, last to last"},
    {.opcode = 22, .name = "join strings, first to last"},
    {.opcode = 23, .name = "join strings, first to first"},
    {.opcode = 24, .name = "join strings, last to first"},
    {.opcode = 28, .name = "height or depth text"},
    {.opcode = 30, .name = "remove height"},
    {.opcode = 31, .name = "remove point"},
    {.opcode = 37, .name = "rectangle"},
    {.opcode = 38, .name = "non-tinable next segment"},
    {.opcode = 39, .name = "non-tinable previous segment"},
    {.opcode = 40, .name = "non-tinable vertex"},
    {.opcode = 45, .name = "parallelogram"},
    {.opcode = 46, .name = "breakline"},
    {.opcode = 47, .name = "new string"},
    {.opcode = 48, .name = "end string"},
    {.opcode = 49, .name = "distances"},
    // "used as the bearing datum difference for the current instrument set up
    // for all measurements ... that follow"
    {.opcode = 50, .name = "backsight bearing", .correctsLater = true},
    // 51, 53, 54 and 56 to 59: "The next measurement takes the feature code and
    // string number from the next point of the field template". 52 ends a
    // template and 55 records one from the measurements' own codes.
    {.opcode = 51, .name = "template start", .codesLater = true},
    {.opcode = 52, .name = "template end"},
    {.opcode = 53, .name = "template pause", .codesLater = true},
    {.opcode = 54, .name = "template continue", .codesLater = true},
    {.opcode = 55, .name = "template record"},
    {.opcode = 56, .name = "template skip", .codesLater = true},
    {.opcode = 57, .name = "template delete", .codesLater = true},
    {.opcode = 58, .name = "template insert", .codesLater = true},
    {.opcode = 59, .name = "template change", .codesLater = true},
    {.opcode = 60, .name = "arc through the next three points"},
    {.opcode = 61, .name = "arc start"},
    {.opcode = 62, .name = "arc end"},
    {.opcode = 68, .name = "integer attribute of the string"},
    {.opcode = 69, .name = "real attribute of the string"},
    {.opcode = 70, .name = "text attribute of the string"},
    {.opcode = 74, .name = "integer attribute of the next segment"},
    {.opcode = 75, .name = "real attribute of the next segment"},
    {.opcode = 76, .name = "text attribute of the next segment"},
    {.opcode = 77, .name = "integer attribute of the previous segment"},
    {.opcode = 78, .name = "real attribute of the previous segment"},
    {.opcode = 79, .name = "text attribute of the previous segment"},
    {.opcode = 80, .name = "pipe justification, invert"},
    {.opcode = 81, .name = "pipe justification, axial"},
    {.opcode = 82, .name = "pipe justification, obvert"},
    {.opcode = 83, .name = "shape record"},
    {.opcode = 84, .name = "shape end"},
    {.opcode = 85, .name = "shape parallel"},
    {.opcode = 86, .name = "shape extrude"},
    {.opcode = 87, .name = "shape parallel start"},
    {.opcode = 88, .name = "shape extrude start"},
    {.opcode = 92, .name = "2d string"},
    {.opcode = 93, .name = "3d string"},
    {.opcode = 94, .name = "4d string"},
    {.opcode = 95, .name = "pipe diameter"},
    {.opcode = 96, .name = "culvert"},
    {.opcode = 101, .name = "automatic string order"},
    {.opcode = 107, .name = "invisible previous segment"},
    {.opcode = 108, .name = "invisible next segment"},
    {.opcode = 109, .name = "invisible vertex"},
    {.opcode = 110, .name = "building face"},
    {.opcode = 111, .name = "building face end"},
    {.opcode = 112, .name = "set collection"},
    {.opcode = 113, .name = "set collection end"},
    {.opcode = 119, .name = "code file"},
    {.opcode = 120, .name = "measured attribute of the string", .xmlOnly = true},
    {.opcode = 121, .name = "measured attribute of the vertex", .xmlOnly = true},
    {.opcode = 122, .name = "measured attribute of the next segment", .xmlOnly = true},
    {.opcode = 123, .name = "measured attribute of the previous segment", .xmlOnly = true},
    {.opcode = 126, .name = "attachment"},
    {.opcode = 127, .name = "distance correction", .correctsLater = true},
    {.opcode = 130, .name = "field file", .xmlOnly = true},
    {.opcode = 131, .name = "PPM correction", .correctsLater = true},
    {.opcode = 140, .name = "GNSS coordinate", .xmlOnly = true, .makesPoint = true},
    {.opcode = 141, .name = "non-tinable string", .xmlOnly = true},
    {.opcode = 142, .name = "extend the next segment", .xmlOnly = true},
    {.opcode = 143, .name = "extend the previous segment", .xmlOnly = true},
    {.opcode = 144, .name = "remove string", .xmlOnly = true},
    {.opcode = 145, .name = "GNSS offset", .xmlOnly = true},
    {.opcode = 146, .name = "midpoint of two points", .xmlOnly = true},
    {.opcode = 147, .name = "midpoint of three points", .xmlOnly = true},
    {.opcode = 172, .name = "extend the next segment in 3d", .xmlOnly = true},
    {.opcode = 173, .name = "extend the previous segment in 3d", .xmlOnly = true},
};

const UnreadOpcode* unreadOpcode(int opcode)
{
    for (const UnreadOpcode& known : kUnreadOpcodes) {
        if (known.opcode == opcode) {
            return &known;
        }
    }
    return nullptr;
}

struct Description {
    std::string_view featureCode;
    std::string_view stringNumber;
    std::string_view pointId;
    std::string_view pointName;
    std::string_view comment;

    // The five values from `values[at]` on.
    static Description of(const std::vector<std::string_view>& values, std::size_t at)
    {
        return {values[at], values[at + 1], values[at + 2], values[at + 3], values[at + 4]};
    }

    // [FLD] 44.5: a known point is found by its name or its ID.
    [[nodiscard]] std::string_view id() const
    {
        return pointName.empty() ? pointId : pointName;
    }

    // "feature code KB, string number 01, comment kerb": what a record says
    // of a point that it does not string (a backsight, a check, a setup);
    // empty when it says none of that.
    [[nodiscard]] std::string coding() const
    {
        std::string text;
        const auto add = [&text](std::string_view label, std::string_view value) {
            if (!blank(value)) {
                text += (text.empty() ? "" : ", ") + std::string(label) + " " +
                        std::string(trimmed(value));
            }
        };
        add("feature code", featureCode);
        add("string number", stringNumber);
        add("comment", comment);
        return text;
    }
};

// "KB 01", "PT04": a feature as a person names it.
std::string featureName(std::string_view code, std::string_view stringNumber)
{
    return stringNumber.empty() ? std::string(code)
                                : std::string(code) + " " + std::string(stringNumber);
}

bool anyFilled(const std::vector<std::string_view>& values, std::size_t from = 0)
{
    for (std::size_t i = from; i < values.size(); ++i) {
        if (!blank(values[i])) {
            return true;
        }
    }
    return false;
}

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
    // Whether the file's records carry the column after the opcode (see the
    // top of this file). Decided by a first pass over every line.
    enum class Layout { Undecided, Column, NoColumn };
    void decideLayout(const std::vector<std::string_view>& lines);

    // A record's values after its opcode and, where the record has it, the
    // column after the opcode; `stamp` is that column, trimmed (empty, or a
    // date or time).
    struct Values {
        std::vector<std::string_view> values;
        std::string_view stamp;
    };
    [[nodiscard]] Values valuesOf(int opcode, const std::vector<std::string_view>& fields) const;

    void record(int opcode, const std::vector<std::string_view>& fields, std::size_t n);
    void fixedRecord(int opcode, const std::vector<std::string_view>& fields, std::size_t n);
    void unread(int opcode, std::size_t n);
    void comment(std::string_view line);
    void units(const std::vector<std::string_view>& values, std::size_t n);
    void coordinate(const Description& d, const std::vector<std::string_view>& v,
                    std::string_view stamp, std::size_t n);
    void station(const Description& d, const std::vector<std::string_view>& v,
                 std::string_view stamp, std::size_t n);
    void measurement(int opcode, const Description& d, const std::vector<std::string_view>& v,
                     std::string_view stamp, std::size_t n);
    void resection(int opcode, const Values& v, std::size_t n);
    void resectionEnd(int opcode, const Values& v, std::size_t n);
    void multipleCoding(const Values& v, std::size_t n);
    void closeString(const Values& v, std::size_t n);
    void pointOffset(int opcode, const Values& v, std::size_t n);
    void attribute(int opcode, const std::vector<std::string_view>& values, std::size_t n);
    void additionalText(const std::vector<std::string_view>& values, std::size_t n);
    void groupStart(const std::vector<std::string_view>& values, std::size_t n);
    void groupEnd(const std::vector<std::string_view>& values, std::size_t n);
    // Ends the open groups from depth `from` inward, each a warning that
    // says `why` (record n).
    void endGroups(std::size_t from, std::size_t n, std::string_view why);

    // The bookkeeping of the current point (see the members below).
    void pointRecordStarts(std::size_t n, bool isMeasurement);
    void pointRecordRead(const std::string& id, const Description& d, std::size_t n,
                         bool isMeasurement);
    // Whether a record about the current point has one; skips it otherwise.
    bool hasCurrentPoint(std::string_view what, std::size_t n);
    // Whether a record about the current measurement point has one.
    bool hasCurrentMeasurement(std::string_view what, std::size_t n);
    // `key` = `value` on the current point, under the current record's
    // prefix; a key the record has given already keeps a different value as
    // "key (record n)", with a warning.
    void putPointValue(const std::string& key, std::string value, std::size_t n);
    // The point ID a record gives a point it names by its name.
    void keepPointId(const Description& d, std::size_t n);
    survey::SurveyStation& beginSetup(std::string_view pointId, std::optional<double> height,
                                      std::string_view stamp, std::size_t n);
    void setupNotRead(std::size_t n, std::string message);
    // The current setup's offsets, applied or said not to be (applyOffsets).
    void applyOffsets();

    std::optional<double> number(std::string_view text, std::string_view what, std::size_t n);

    RawProjectBuilder builder_;
    std::optional<katana::core::Error> fatal_;
    Layout layout_ = Layout::Undecided;
    // Fixed records voted both ways: one that fits both layouts is skipped.
    bool mixedLayout_ = false;
    double targetHeight_ = 0.0;
    bool targetHeightSeen_ = false;
    double distanceScale_ = 1.0;
    bool scaleWarned_ = false;
    // Comments before the first record that makes a point or a setup are the
    // file's header.
    bool header_ = true;
    std::string headerText_;
    // No record has been met yet: a "{Version ...}" line is the file's own.
    bool firstRecord_ = true;

    // The point the last 02, 04, 06 or 07 was about: 41 and 71 to 73 describe
    // it. Empty after such a record that was not read, which is then
    // `unreadPointRecord_`.
    std::string currentPoint_;
    std::size_t unreadPointRecord_ = 0;
    // "record N/" on what the records after a point record give its point,
    // unless that record was the first about it; and what they gave, by key.
    std::string attributePrefix_;
    std::unordered_map<std::string, std::string> recordValues_;
    // Every point a record has made current, and the point ID first given
    // with each point name.
    std::unordered_set<std::string> describedPoints_;
    std::unordered_map<std::string, std::string> pointIds_;
    // [FLD] 44.4's current measurement point - the point of the last 02 or 07
    // - with the record, the feature code and the string number that made
    // it: 16, 20 and the offsets act on it and its string.
    std::string currentMeasurement_;
    std::size_t measurementRecord_ = 0;
    bool measurementIsShot_ = false; // made by a 07, not an 02
    std::string currentCode_;
    std::string currentString_;
    std::size_t unreadMeasurementRecord_ = 0;
    // Open attribute groups, outermost first, with the records that opened them.
    std::vector<std::string> groups_;
    std::vector<std::size_t> groupRecords_;
    // A setup record (03, 128, 138) that was not read: the measurements after
    // it are not filed under the setup before it.
    std::size_t unreadSetupRecord_ = 0;
    // The resection (128 or 138) whose block is open, 0 when none.
    std::size_t openResection_ = 0;
    int openResectionOpcode_ = 0;
    std::size_t resections_ = 0;
    // A date and time on a record that makes no point or setup: said once.
    bool unkeptStampWarned_ = false;
    std::string coordinateSystem_;
    std::string zone_;

    // What the current setup measured to each point, for the offsets.
    struct Shot {
        std::size_t pointings = 0; // 04, 06 and 07 to the point from the setup
        std::size_t record = 0;    // of the first, when it is a 07
        std::size_t station = 0;   // index in SurveyProject::stations
        std::optional<std::size_t> direction; // indices in the setup's observations
        std::optional<std::size_t> zenith;
        std::optional<std::size_t> distance;
        double circle = 0.0; // radians, as read
        double zenithAngle = 0.0; // radians, FL-equivalent
        double slope = 0.0; // metres, scaled
        std::string measured; // the values as the file writes them
        double radial = 0.0; // the offsets applied, metres
        double tangential = 0.0;
        double height = 0.0;
    };
    std::unordered_map<std::string, Shot> shots_;
    struct PendingOffset {
        int opcode = 0;
        std::string target;
        double amount = 0.0;
        std::size_t record = 0;
    };
    std::vector<PendingOffset> pendingOffsets_;
};

std::optional<double> FieldFileReader::number(std::string_view text, std::string_view what,
                                              std::size_t n)
{
    if (blank(text)) {
        return std::nullopt;
    }
    const std::optional<double> value = parseReal(text);
    if (!value) {
        builder_.warn(n, std::string(what) + " '" + std::string(trimmed(text).substr(0, 40)) +
                             "' is not a number");
    }
    return value;
}

FieldFileReader::Values FieldFileReader::valuesOf(int opcode,
                                                  const std::vector<std::string_view>& fields) const
{
    Values result;
    if (fields.size() < 2) {
        return result; // the opcode alone
    }
    const std::string_view first = fields[1];
    const bool columnLike = blank(first) || isDateOrTime(first);
    bool column = false;
    switch (layout_) {
    case Layout::Column: column = columnLike; break;
    case Layout::NoColumn:
        // A record of fixed form that fits only with its blank first value
        // taken as the column carries it: an empty target height, factor or
        // unit followed by one is no value.
        if (blank(first) && fields.size() > 2) {
            const std::vector<std::string_view> all(fields.begin() + 1, fields.end());
            const std::vector<std::string_view> rest(fields.begin() + 2, fields.end());
            const std::optional<bool> whole = fixedFormFits(opcode, all);
            column = whole.has_value() && !*whole && fixedFormFits(opcode, rest).value_or(false);
        }
        break;
    // No fixed record decided the file: a record with more than one value and
    // a blank (or dated) first is taken to have the column.
    case Layout::Undecided: column = columnLike && fields.size() > 2; break;
    }
    if (column) {
        result.stamp = trimmed(first);
        result.values.assign(fields.begin() + 2, fields.end());
    } else {
        result.values.assign(fields.begin() + 1, fields.end());
    }
    return result;
}

void FieldFileReader::comment(std::string_view line)
{
    const std::string_view text = trimmed(line.substr(2));
    if (isDecoration(text)) {
        return;
    }
    if (!header_) {
        // A resection's residuals, an antenna height changed: kept, as
        // written, with the setup they follow.
        builder_.addStationNote(text);
        return;
    }
    const std::size_t colon = text.find(':');
    if (colon != std::string_view::npos) {
        const std::string_view key = trimmed(text.substr(0, colon));
        const std::string_view value = trimmed(text.substr(colon + 1));
        if (!key.empty() && key.size() <= 40) {
            if (value.empty()) {
                return; // a heading the writer left blank ("Surveyor : ")
            }
            builder_.project().metadata["header: " + std::string(key)] = std::string(value);
            if (key == "Coordinate System") {
                coordinateSystem_ = std::string(value);
            } else if (key == "Zone") {
                zone_ = std::string(value);
            }
            return;
        }
    }
    headerText_ += (headerText_.empty() ? "" : "\n") + std::string(text);
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

void FieldFileReader::pointRecordStarts(std::size_t n, bool isMeasurement)
{
    currentPoint_.clear();
    unreadPointRecord_ = n;
    attributePrefix_.clear();
    recordValues_.clear();
    if (isMeasurement) {
        currentMeasurement_.clear();
        measurementRecord_ = 0;
        measurementIsShot_ = false;
        currentCode_.clear();
        currentString_.clear();
        unreadMeasurementRecord_ = n;
    }
}

void FieldFileReader::pointRecordRead(const std::string& id, const Description& d, std::size_t n,
                                      bool isMeasurement)
{
    currentPoint_ = id;
    unreadPointRecord_ = 0;
    // The first record about a point owns its attributes' plain names; a
    // later one's go under its record (see the top of this file).
    attributePrefix_ =
        describedPoints_.insert(id).second ? std::string{} : "record " + std::to_string(n) + "/";
    recordValues_.clear();
    if (isMeasurement) {
        currentMeasurement_ = id;
        measurementRecord_ = n;
        currentCode_ = std::string(d.featureCode);
        currentString_ = std::string(d.stringNumber);
        unreadMeasurementRecord_ = 0;
    }
}

bool FieldFileReader::hasCurrentPoint(std::string_view what, std::size_t n)
{
    if (!currentPoint_.empty()) {
        return true;
    }
    builder_.skip(n, unreadPointRecord_ != 0
                         ? std::string(what) + " of the point of record " +
                               std::to_string(unreadPointRecord_) + ", which was not read"
                         : std::string(what) + " with no point before it to describe");
    return false;
}

bool FieldFileReader::hasCurrentMeasurement(std::string_view what, std::size_t n)
{
    if (!currentMeasurement_.empty()) {
        return true;
    }
    builder_.skip(n, unreadMeasurementRecord_ != 0
                         ? std::string(what) + " of the measurement of record " +
                               std::to_string(unreadMeasurementRecord_) + ", which was not read"
                         : std::string(what) + " with no measurement (opcode 02 or 07) before it");
    return false;
}

void FieldFileReader::putPointValue(const std::string& key, std::string value, std::size_t n)
{
    std::string full = attributePrefix_ + key;
    const auto [given, first] = recordValues_.try_emplace(full, value);
    if (first) {
        builder_.addPointMetadata(currentPoint_, std::move(full), std::move(value));
        return;
    }
    if (given->second == value) {
        return; // given again, the same: nothing to add
    }
    std::string again = full + " (record " + std::to_string(n) + ")";
    builder_.warn(n, "point '" + currentPoint_ + "' is given '" + key + "' again, as '" +
                         value.substr(0, 40) + "' after '" + given->second.substr(0, 40) +
                         "'; both are kept, this one as '" + again + "'");
    builder_.addPointMetadata(currentPoint_, std::move(again), std::move(value));
}

void FieldFileReader::keepPointId(const Description& d, std::size_t n)
{
    if (blank(d.pointName) || blank(d.pointId)) {
        return;
    }
    // Named by its name, the point keeps its ID too: the first one given it,
    // and a different one a later record gives under that record.
    const std::string name(d.pointName);
    const auto [known, first] = pointIds_.try_emplace(name, std::string(d.pointId));
    if (first) {
        builder_.addPointMetadata(name, "point ID", known->second);
    } else if (known->second != d.pointId) {
        builder_.addPointMetadata(name, "record " + std::to_string(n) + "/point ID",
                                  std::string(d.pointId));
    }
}

survey::SurveyStation& FieldFileReader::beginSetup(std::string_view pointId,
                                                   std::optional<double> height,
                                                   std::string_view stamp, std::size_t n)
{
    if (!height) {
        builder_.warn(n, "the setup states no instrument height; 0 is used");
    }
    survey::InstrumentSettings settings;
    if (distanceScale_ != 1.0) {
        // Applied, not NotApplied: measurement() has already multiplied every
        // slope distance of this setup by it, and a reduction that read it as
        // missing would apply it twice.
        settings.scaleFactor = distanceScale_;
        settings.scaleFactorState = survey::CorrectionState::Applied;
    }
    survey::SurveyStation& setup =
        builder_.beginStation(pointId, height.value_or(0.0), settings, n);
    if (!stamp.empty()) {
        setup.metadata["time stamp"] = std::string(stamp);
    }
    unreadSetupRecord_ = 0;
    openResection_ = 0;
    openResectionOpcode_ = 0;
    return setup;
}

void FieldFileReader::setupNotRead(std::size_t n, std::string message)
{
    unreadSetupRecord_ = n;
    openResection_ = 0;
    openResectionOpcode_ = 0;
    builder_.skip(n, std::move(message) +
                         "; the measurements after it are not read until the next setup");
}

void FieldFileReader::coordinate(const Description& d, const std::vector<std::string_view>& v,
                                 std::string_view stamp, std::size_t n)
{
    const std::optional<double> x = number(v[0], "X", n);
    const std::optional<double> y = number(v[1], "Y", n);
    const std::optional<double> z = number(v[2], "Z", n);
    if (!x || !y) {
        builder_.skip(n, "coordinate record without both X and Y");
        return;
    }
    const std::string id(d.id());
    builder_.positionPoint(id, *y, *x, z, survey::CoordinateSource::Entered, n);
    builder_.codePoint(id, d.featureCode, d.comment, d.stringNumber, n);
    pointRecordRead(id, d, n, true);
    if (!stamp.empty()) {
        putPointValue("time stamp", std::string(stamp), n);
    }
    keepPointId(d, n);
    builder_.countRead();
}

void FieldFileReader::station(const Description& d, const std::vector<std::string_view>& v,
                              std::string_view stamp, std::size_t n)
{
    survey::SurveyStation& setup =
        beginSetup(d.id(), number(v[0], "instrument height", n), stamp, n);
    // A setup makes no measurement point, so its code strings nothing
    // ([FLD] 44.4); its comment still describes the point it stands on.
    if (const std::string coding = d.coding(); !coding.empty()) {
        setup.metadata["setup coding"] = coding;
    }
    if (!blank(d.comment)) {
        builder_.codePoint(d.id(), {}, d.comment, {}, n);
    }
    if (!blank(d.pointName) && !blank(d.pointId)) {
        setup.metadata["point ID"] = std::string(d.pointId);
    }
    builder_.countRead();
}

void FieldFileReader::measurement(int opcode, const Description& d,
                                  const std::vector<std::string_view>& v, std::string_view stamp,
                                  std::size_t n)
{
    const char* const kind =
        opcode == 4 ? "backsight" : (opcode == 6 ? "check measurement" : "measurement");
    if (unreadSetupRecord_ != 0) {
        builder_.skip(n, std::string(kind) + " from the setup of record " +
                             std::to_string(unreadSetupRecord_) + ", which was not read");
        return;
    }
    survey::SurveyStation* setup = builder_.currentStation();
    if (setup == nullptr) {
        builder_.skip(n, std::string(kind) + " with no setup (opcode 03 or 128) before it");
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
    bool hasDistance = false;
    double distance = 0.0;
    if (slope) {
        distance = *slope * distanceScale_;
        hasDistance = distance > 0.0;
        if (!hasDistance) {
            builder_.warn(n, "slope distance of " + katana::core::formatExactReal(*slope) +
                                 " m read as no distance measured");
        }
    }
    if (!horizontal && !hasZenith && !hasDistance) {
        // Nothing of the record reaches the model: no point, no code.
        builder_.skip(n, std::string(kind) + " with no readable value");
        return;
    }

    builder_.mentionPoint(target, n);
    if (opcode == 7) {
        builder_.codePoint(target, d.featureCode, d.comment, d.stringNumber, n);
    } else if (opcode == 4) {
        setup->backsightPointId = target;
        // What the reduction orients the setup by when the backsight has no
        // coordinates: the file's azimuth to it where the 04 gives one ("may
        // be specified when no coordinate for the backsight point exists"),
        // else the circle reading, taken as set to a grid azimuth.
        std::optional<double> azimuth;
        if (v.size() > 3) {
            azimuth = number(v[3], "backsight azimuth", n);
        }
        if (azimuth) {
            setup->backsightAzimuth = wrapToCircle(*azimuth * kRadiansPerDegree);
            setup->metadata["backsight azimuth"] =
                std::string(trimmed(v[3])) + " degrees, record " + std::to_string(n) +
                ": the setup is oriented by it if its backsight has no coordinates";
        } else if (horizontal) {
            setup->backsightAzimuth = wrapToCircle(*horizontal * kRadiansPerDegree);
        }
        // A backsight's own code strings nothing ([FLD] finds the backsight
        // by its name): kept with the setup it orients rather than on the
        // point, whose drawn properties are its shots'.
        if (const std::string coding = d.coding(); !coding.empty()) {
            setup->metadata["backsight record " + std::to_string(n)] = coding;
        }
    }

    const survey::ObservationPrecision& precision = builder_.options().precision;
    const double hi = setup->setup.instrumentHeight;
    const survey::Pointing pointing{builder_.nextPointing(), face};
    Shot& shot = shots_[target];
    if (shot.pointings++ == 0) {
        shot.station = builder_.project().stations.size() - 1;
    }
    // The first 07 to the point from this setup is what an offset moves.
    const bool firstShot = opcode == 7 && shot.record == 0;
    if (firstShot) {
        shot.record = n;
        shot.measured = "horizontal circle " + std::string(trimmed(v[0])) + ", vertical circle " +
                        std::string(trimmed(v[1])) + ", slope distance " +
                        std::string(trimmed(v[2]));
    }
    if (horizontal) {
        auto& observation = builder_.stationObservation<survey::HorizontalDirectionObservation>(n);
        observation.at = setup->setup.pointId;
        observation.to = target;
        observation.direction = wrapToCircle(*horizontal * kRadiansPerDegree);
        observation.sigma = precision.direction;
        observation.pointing = pointing;
        if (firstShot) {
            shot.direction = setup->observations.size() - 1;
            shot.circle = observation.direction;
        }
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
        if (firstShot) {
            shot.zenith = setup->observations.size() - 1;
            shot.zenithAngle = zenith;
        }
    }
    if (hasDistance) {
        auto& observation = builder_.stationObservation<survey::DistanceObservation>(n);
        observation.from = setup->setup.pointId;
        observation.to = target;
        observation.distance = distance;
        observation.sigma = survey::distanceSigma(precision, distance);
        observation.kind = survey::DistanceKind::Slope;
        observation.instrumentHeight = hi;
        observation.targetHeight = targetHeight_;
        observation.pointing = pointing;
        if (firstShot) {
            shot.distance = setup->observations.size() - 1;
            shot.slope = distance;
        }
    }
    pointRecordRead(target, d, n, opcode == 7);
    if (opcode == 7) {
        measurementIsShot_ = true;
    }
    if (!stamp.empty()) {
        putPointValue("time stamp", std::string(stamp), n);
    }
    if (opcode == 6) {
        const std::string coding = d.coding();
        putPointValue("check measurement",
                      "from setup " + setup->setup.id + (coding.empty() ? "" : ", " + coding), n);
    }
    keepPointId(d, n);
    builder_.countRead();
}

void FieldFileReader::resection(int opcode, const Values& v, std::size_t n)
{
    // [FLD]: "128 instrument_height_value"; the files write a point
    // description first, as the XML form has one, and an empty value after.
    const std::vector<std::string_view>& values = v.values;
    std::string_view height;
    Description d{};
    if (values.size() == 1) {
        height = values[0];
    } else if (values.size() == 6 || (values.size() == 7 && blank(values[6]))) {
        d = Description::of(values, 0);
        height = values[5];
    } else {
        setupNotRead(n, "resection (opcode " + opcodeText(opcode) + ") with " +
                            std::to_string(values.size()) +
                            " value(s) where the format gives the instrument height, or a point "
                            "description and the height; which value is which cannot be told");
        return;
    }
    const std::string pointId = blank(d.id()) ? "resection at record " + std::to_string(n)
                                              : std::string(d.id());
    survey::SurveyStation& setup =
        beginSetup(pointId, number(height, "instrument height", n), v.stamp, n);
    setup.metadata["resection"] = std::string(opcode == 128 ? "least squares" : "Helmert") +
                                    ", record " + std::to_string(n);
    // The XML form calls this description the "created resection point"'s;
    // [FLD] says nothing strings it, so its code is the setup's, as a 03's.
    if (const std::string coding = d.coding(); !coding.empty()) {
        setup.metadata["setup coding"] = coding;
    }
    if (!blank(d.comment)) {
        builder_.codePoint(pointId, {}, d.comment, {}, n);
    }
    openResection_ = n;
    openResectionOpcode_ = opcode;
    ++resections_;
    builder_.countRead();
}

void FieldFileReader::resectionEnd(int opcode, const Values& v, std::size_t n)
{
    const std::string what = "end of a resection (opcode " + opcodeText(opcode) + ")";
    if (openResection_ == 0) {
        builder_.skip(n, what + " with no resection open");
        return;
    }
    if (anyFilled(v.values)) {
        builder_.warn(n, what + " carries values the format does not give it; they are not kept");
    }
    if (openResectionOpcode_ + 1 != opcode) {
        builder_.warn(n, what + " ends the resection of record " + std::to_string(openResection_) +
                             ", which is opcode " + opcodeText(openResectionOpcode_));
    }
    if (survey::SurveyStation* setup = builder_.currentStation()) {
        setup->metadata["resection end"] = "record " + std::to_string(n);
    }
    openResection_ = 0;
    openResectionOpcode_ = 0;
    builder_.countRead();
}

void FieldFileReader::multipleCoding(const Values& v, std::size_t n)
{
    const std::string what = "multiple coding (opcode 16)";
    if (v.values.size() != 5) {
        builder_.skip(n, what + " with " + std::to_string(v.values.size()) +
                             " value(s) where the format gives a point description of 5");
        return;
    }
    if (!hasCurrentMeasurement(what, n)) {
        return;
    }
    const Description d = Description::of(v.values, 0);
    if (blank(d.featureCode)) {
        builder_.skip(n, what + " with no feature code: there is no string to add the point to");
        return;
    }
    // [FLD] makes a second point at the current point's position, strung by
    // this record's code and number. Katana strings the current point itself
    // into that string - one position, as [FLD]'s is - and keeps what this
    // record says of the second point with it.
    builder_.codePoint(currentMeasurement_, d.featureCode, "", d.stringNumber, n);
    if (!blank(d.id()) || !blank(d.comment)) {
        std::string said = featureName(d.featureCode, d.stringNumber);
        if (!blank(d.pointId)) {
            said += ", point ID " + std::string(trimmed(d.pointId));
        }
        if (!blank(d.pointName)) {
            said += ", point name " + std::string(trimmed(d.pointName));
        }
        if (!blank(d.comment)) {
            said += ", comment " + std::string(trimmed(d.comment));
        }
        builder_.addPointMetadata(currentMeasurement_, "multiple coding record " + std::to_string(n),
                                  said);
        if (!blank(d.id())) {
            builder_.warn(n, what + " names point '" + std::string(d.id()) +
                                 "': Katana strings point '" + currentMeasurement_ + "' into " +
                                 featureName(d.featureCode, d.stringNumber) +
                                 " rather than making a second point at its position, and "
                                 "keeps the name in its metadata");
        }
    }
    builder_.countRead();
}

void FieldFileReader::closeString(const Values& v, std::size_t n)
{
    const std::string what = "close string (opcode 20)";
    const std::vector<std::string_view>& values = v.values;
    std::optional<std::size_t> points;
    std::string which;
    if (!anyFilled(values)) {
        // "If no point description is given, the current string is closed."
        if (!hasCurrentMeasurement(what, n)) {
            return;
        }
        if (currentCode_.empty()) {
            builder_.skip(n, what + ": the point of record " + std::to_string(measurementRecord_) +
                                 " has no feature code, so there is no current string");
            return;
        }
        which = featureName(currentCode_, currentString_);
        points = builder_.closeFeature(currentCode_, currentString_);
    } else if (values.size() == 5) {
        const Description d = Description::of(values, 0);
        if (!blank(d.featureCode)) {
            which = featureName(d.featureCode, d.stringNumber);
            points = builder_.closeFeature(d.featureCode, d.stringNumber);
        } else if (!blank(d.id())) {
            which = "of point '" + std::string(d.id()) + "'";
            points = builder_.closeFeatureOf(d.id());
        } else {
            builder_.skip(n, what + " whose point description names neither a string nor a point");
            return;
        }
    } else {
        builder_.skip(n, what + " with " + std::to_string(values.size()) +
                             " value(s) where the format gives none, or a point description of 5");
        return;
    }
    if (!points) {
        builder_.skip(n, what + ": there is no open string " + which);
        return;
    }
    if (*points < 3) {
        builder_.warn(n, "the string " + which + " is closed with " + std::to_string(*points) +
                             " point(s)");
    }
    builder_.countRead();
}

void FieldFileReader::pointOffset(int opcode, const Values& v, std::size_t n)
{
    const std::string_view kind =
        opcode == 42 ? "radial" : (opcode == 43 ? "tangential" : "height");
    const std::string what = std::string(kind) + " offset (opcode " + opcodeText(opcode) + ")";
    const std::vector<std::string_view>& values = v.values;
    std::string target;
    std::string_view amountText;
    // Why the offset cannot move a shot, when that is already known.
    std::string notApplied;
    if (values.size() == 1) {
        // "If no point description is given, the offset is used to adjust
        // the position of the current measured point."
        if (!hasCurrentMeasurement(what, n)) {
            return;
        }
        target = currentMeasurement_;
        amountText = values[0];
        if (!measurementIsShot_) {
            notApplied = "point '" + target + "' is the entered coordinate of record " +
                         std::to_string(measurementRecord_) + ", which is kept as the file states it";
        }
    } else if (values.size() == 6) {
        const Description d = Description::of(values, 0);
        amountText = values[5];
        if (!blank(d.featureCode)) {
            // "the last point of the previous string with that feature code
            // and string number is adjusted"
            const std::optional<std::string> last =
                builder_.lastPointOf(d.featureCode, d.stringNumber);
            if (!last) {
                builder_.skip(n, what + ": there is no open string " +
                                     featureName(d.featureCode, d.stringNumber));
                return;
            }
            target = *last;
        } else if (!blank(d.id()) && builder_.hasPoint(d.id())) {
            target = std::string(d.id());
        } else {
            builder_.skip(n, what + " names no string and no point a record before it made");
            return;
        }
    } else {
        builder_.skip(n, what + " with " + std::to_string(values.size()) +
                             " value(s) where the format gives the offset, or a point description "
                             "and the offset; which value is which cannot be told");
        return;
    }
    const std::optional<double> amount = number(amountText, what, n);
    if (!amount) {
        builder_.skip(n, what + " with no offset");
        return;
    }
    // shots_ holds the current setup's, and none after a setup not read.
    if (notApplied.empty() && (!shots_.contains(target) || shots_.at(target).record == 0)) {
        notApplied = "point '" + target + "' was not measured (opcode 07) from the current setup";
    }
    if (!notApplied.empty()) {
        builder_.addPointMetadata(target, "offset record " + std::to_string(n),
                                  std::string(kind) + " " + katana::core::formatExactReal(*amount) +
                                      " m, not applied");
        builder_.warn(n, "the " + what + " of " + katana::core::formatExactReal(*amount) +
                             " m to point '" + target + "' is kept in its metadata and not applied: " +
                             notApplied);
    } else {
        pendingOffsets_.push_back({opcode, target, *amount, n});
    }
    builder_.countRead();
}

// The current setup's offsets. An offset moves the one shot of its point
// from the setup; one of a point measured more than once from it (a face
// pair, a check, a backsight), of a shot with no slope distance or zenith,
// or one that with the shot's other offsets puts the point on the station,
// is kept and not applied, with a warning.
//
// The arithmetic ([FLD] 44.8, 42 to 44): what was measured gives the plan
// distance h = s sin z and the rise v = s cos z along the line of the circle
// reading a. With every radial offset r, tangential t and height offset u of
// the shot so far - each "from the specified points original position", so
// their sum - the offset position is at plan distance h' = hypot(h + r, t)
// on the circle reading a + atan2(t, h + r) (t to the right looking from the
// station, which a clockwise circle reads as more), and rises v' = v + u. The
// shot becomes that reading, the zenith atan2(h', v') and the slope distance
// hypot(h', v'); the target height is the prism's, as measured. The
// reduction then applies its grid scale factor k and its curvature to h'
// rather than to h, so the offset is scaled by k with the rest: (k - 1) of
// it, under a millimetre for a metre's offset anywhere in a six-degree
// transverse Mercator zone (k from 0.9996 on the central meridian to about
// 1.001 at a zone's edge, k0 (1 + (dl cos phi)^2 / 2) for dl up to 3
// degrees); the change in curvature is h (h' - h) (1 - k_r) / R, a few
// micrometres.
void FieldFileReader::applyOffsets()
{
    // Why each offset is not applied; empty for one that is.
    std::vector<std::string> notApplied(pendingOffsets_.size());
    for (std::size_t i = 0; i < pendingOffsets_.size(); ++i) {
        const PendingOffset& offset = pendingOffsets_[i];
        Shot& shot = shots_.at(offset.target);
        if (shot.pointings > 1) {
            notApplied[i] = "point '" + offset.target + "' was measured " +
                            std::to_string(shot.pointings) + " times from setup " +
                            builder_.project().stations[shot.station].setup.id +
                            ", and which of the measurements the offset corrects the format does "
                            "not say";
        } else if (!shot.distance || !shot.zenith) {
            notApplied[i] = "the shot of record " + std::to_string(shot.record) +
                            " has no slope distance or no zenith, so the point has no plan " +
                            "distance or height to offset";
        } else if (offset.opcode == 42) {
            shot.radial += offset.amount;
        } else if (offset.opcode == 43) {
            shot.tangential += offset.amount;
        } else {
            shot.height += offset.amount;
        }
    }
    // Each shot an offset applies to, moved once for all of its offsets.
    const survey::ObservationPrecision& precision = builder_.options().precision;
    std::unordered_set<std::string> moved;
    std::unordered_set<std::string> onStation;
    for (std::size_t i = 0; i < pendingOffsets_.size(); ++i) {
        const std::string& target = pendingOffsets_[i].target;
        if (!notApplied[i].empty() || !moved.insert(target).second) {
            continue;
        }
        const Shot& shot = shots_.at(target);
        const double plan = shot.slope * std::sin(shot.zenithAngle);
        const double rise = shot.slope * std::cos(shot.zenithAngle);
        const double along = plan + shot.radial;
        const double planAfter = std::hypot(along, shot.tangential);
        const double riseAfter = rise + shot.height;
        const double slopeAfter = std::hypot(planAfter, riseAfter);
        // Within kCoordinate of the instrument it is the station's own mark,
        // to which no shot has a direction or zenith: kept as measured.
        if (!(slopeAfter >= katana::math::tolerance::kCoordinate)) {
            onStation.insert(target);
            continue;
        }
        std::vector<survey::Observation>& observations =
            builder_.project().stations[shot.station].observations;
        if (shot.direction) {
            auto& direction =
                std::get<survey::HorizontalDirectionObservation>(observations[*shot.direction]);
            direction.direction = wrapToCircle(shot.circle + std::atan2(shot.tangential, along));
        }
        auto& zenith = std::get<survey::ZenithAngleObservation>(observations[*shot.zenith]);
        zenith.angle = std::atan2(planAfter, riseAfter);
        auto& distance = std::get<survey::DistanceObservation>(observations[*shot.distance]);
        distance.distance = slopeAfter;
        distance.sigma = survey::distanceSigma(precision, slopeAfter);
        builder_.addPointMetadata(target, "shot record " + std::to_string(shot.record) +
                                              " as measured",
                                  shot.measured);
    }
    for (std::size_t i = 0; i < pendingOffsets_.size(); ++i) {
        const PendingOffset& offset = pendingOffsets_[i];
        const Shot& shot = shots_.at(offset.target);
        if (notApplied[i].empty() && onStation.contains(offset.target)) {
            notApplied[i] = "with the other offsets of its shot it puts point '" + offset.target +
                            "' on the station";
        }
        const std::string_view kind =
            offset.opcode == 42 ? "radial" : (offset.opcode == 43 ? "tangential" : "height");
        const std::string amount = katana::core::formatExactReal(offset.amount);
        std::string value = std::string(kind) + " " + amount + " m from setup " +
                            builder_.project().stations[shot.station].setup.id;
        if (notApplied[i].empty()) {
            value += ", applied to the shot of record " + std::to_string(shot.record);
        } else {
            value += ", not applied";
            builder_.warn(offset.record, "the " + std::string(kind) + " offset (opcode " +
                                             opcodeText(offset.opcode) + ") of " + amount +
                                             " m to point '" + offset.target +
                                             "' is kept in its metadata and not applied: " +
                                             notApplied[i]);
        }
        builder_.addPointMetadata(offset.target, "offset record " + std::to_string(offset.record),
                                  std::move(value));
    }
    pendingOffsets_.clear();
    shots_.clear();
}

void FieldFileReader::attribute(int opcode, const std::vector<std::string_view>& values,
                                std::size_t n)
{
    if (!anyFilled(values)) {
        builder_.skip(n, "attribute record (opcode " + opcodeText(opcode) +
                             ") with neither a name nor a value");
        return;
    }
    if (!hasCurrentPoint("an attribute", n)) {
        return;
    }
    const std::string_view name = values.empty() ? std::string_view{} : trimmed(values[0]);
    std::string value;
    if (opcode == 73) {
        // Text: a tab in it split it into more values.
        for (std::size_t i = 1; i < values.size(); ++i) {
            value += (i > 1 ? "\t" : "") + std::string(values[i]);
        }
        value = std::string(trimmed(value));
    } else {
        value = values.size() < 2 ? std::string{} : std::string(trimmed(values[1]));
        if (anyFilled(values, 2)) {
            builder_.warn(n, "attribute '" + std::string(name) +
                                 "' has values after its value; they are not kept");
        }
    }
    const std::string label = name.empty() ? std::string("unnamed") : std::string(name);
    if (opcode == 72 && !value.empty() && !parseReal(value)) {
        builder_.warn(n, "real attribute '" + label + "' holds '" + value.substr(0, 40) +
                             "', which is not a number; it is kept as text");
    } else if (opcode == 71 && !value.empty() && !katana::core::parseInteger(value)) {
        builder_.warn(n, "integer attribute '" + label + "' holds '" + value.substr(0, 40) +
                             "', which is not a whole number; it is kept as text");
    }
    std::string key;
    for (const std::string& group : groups_) {
        if (!group.empty()) {
            key += group + "/";
        }
    }
    // [FLD] 44.8, opcodes 68 to 79: a blank name makes the attribute unnamed.
    key += name.empty() ? "unnamed attribute (record " + std::to_string(n) + ")" : std::string(name);
    putPointValue(key, std::move(value), n);
    builder_.countRead();
}

void FieldFileReader::additionalText(const std::vector<std::string_view>& values, std::size_t n)
{
    if (!hasCurrentPoint("additional text", n)) {
        return;
    }
    // Not trimmed: "any spaces from column four onwards will be part of the
    // text". A tab in it split it into more values.
    std::string text;
    for (std::size_t i = 0; i < values.size(); ++i) {
        text += (i > 0 ? "\t" : "") + std::string(values[i]);
    }
    builder_.addPointMetadata(currentPoint_, "additional text " + std::to_string(n),
                              std::move(text));
    builder_.countRead();
}

void FieldFileReader::endGroups(std::size_t from, std::size_t n, std::string_view why)
{
    while (groups_.size() > from) {
        builder_.warn(groupRecords_.back(), "attribute group '" + groups_.back() +
                                                "' is not ended (opcode 125) before record " +
                                                std::to_string(n) + ", which " + std::string(why) +
                                                "; it is ended there");
        groups_.pop_back();
        groupRecords_.pop_back();
    }
}

void FieldFileReader::groupStart(const std::vector<std::string_view>& values, std::size_t n)
{
    // XML: a name and a level. The files: an empty value, the name, the level.
    std::string_view before;
    std::string_view name;
    std::string_view level;
    if (values.size() == 3) {
        before = trimmed(values[0]);
        name = trimmed(values[1]);
        level = trimmed(values[2]);
    } else if (values.size() == 2) {
        name = trimmed(values[0]);
        level = trimmed(values[1]);
    } else if (values.size() == 1) {
        name = trimmed(values[0]);
    } else {
        builder_.skip(n, "attribute group (opcode 124) with " + std::to_string(values.size()) +
                             " value(s); which is its name cannot be told");
        return;
    }
    if (!before.empty()) {
        builder_.warn(n, "attribute group '" + std::string(name) + "' has '" +
                             std::string(before.substr(0, 40)) +
                             "' before its name, which the files this reader knows leave empty; "
                             "it is not kept");
    }
    if (name.empty()) {
        builder_.warn(n, "attribute group with no name: its attributes are kept without one");
    }
    if (!level.empty()) {
        const std::optional<std::int64_t> depth = katana::core::parseInteger(level);
        if (depth && *depth >= 0 && static_cast<std::size_t>(*depth) < groups_.size()) {
            // A level a group still open has: that group, and any inside it,
            // were not ended.
            endGroups(static_cast<std::size_t>(*depth), n,
                      "opens attribute group '" + std::string(name) + "' at its level");
        } else if (!depth || static_cast<std::size_t>(*depth) != groups_.size()) {
            builder_.warn(n, "attribute group '" + std::string(name) + "' gives the level '" +
                                 std::string(level.substr(0, 20)) + "' and opens inside " +
                                 std::to_string(groups_.size()) +
                                 " group(s); it is nested as the records open and end it");
        }
    }
    groups_.emplace_back(name);
    groupRecords_.push_back(n);
    builder_.countRead();
}

void FieldFileReader::groupEnd(const std::vector<std::string_view>& values, std::size_t n)
{
    if (groups_.empty()) {
        builder_.skip(n, "end of an attribute group (opcode 125) with no group open");
        return;
    }
    // The files give the level of the group ended, last; a name, where one is
    // given, before it.
    const std::string_view level = values.empty() ? std::string_view{} : trimmed(values.back());
    const std::string_view name = values.size() < 2 ? std::string_view{}
                                                    : trimmed(values[values.size() - 2]);
    if (!level.empty()) {
        const std::optional<std::int64_t> depth = katana::core::parseInteger(level);
        if (!depth || *depth + 1 != static_cast<std::int64_t>(groups_.size())) {
            builder_.warn(n, "the end of attribute group '" + groups_.back() + "' gives the level '" +
                                 std::string(level.substr(0, 20)) + "', which is not the group's");
        }
    }
    if (!name.empty() && name != groups_.back()) {
        builder_.warn(n, "the end of attribute group '" + groups_.back() + "' names '" +
                             std::string(name.substr(0, 40)) + "'; the open group is ended");
    }
    groups_.pop_back();
    groupRecords_.pop_back();
    builder_.countRead();
}

void FieldFileReader::unread(int opcode, std::size_t n)
{
    const UnreadOpcode* known = unreadOpcode(opcode);
    if (known != nullptr && known->makesPoint) {
        pointRecordStarts(n, true);
    }
    std::string message = "opcode " + opcodeText(opcode);
    if (known == nullptr) {
        message += " is not one the format defines";
    } else if (known->xmlOnly) {
        message += " (" + std::string(known->name) +
                   ") is defined only for the format's XML form, which says it does not exist "
                   "in a .fld; its values are not guessed at";
    } else {
        message += " (" + std::string(known->name) + ") is not one this reader imports";
    }
    if (known != nullptr && known->correctsLater) {
        message += "; the measurements after it are read without it";
    } else if (known != nullptr && known->codesLater) {
        message += "; the measurements after it keep the codes they are written with, which a "
                   "field template may have changed";
    }
    builder_.skip(n, std::move(message));
}

void FieldFileReader::decideLayout(const std::vector<std::string_view>& lines)
{
    std::size_t column = 0;
    std::size_t noColumn = 0;
    for (const std::string_view line : lines) {
        const std::size_t tab = line.find('\t');
        if (tab == std::string_view::npos) {
            continue;
        }
        const std::optional<int> opcode = opcodeOf(trimmed(line.substr(0, tab)));
        if (!opcode || !valuesAfterDescription(*opcode)) {
            continue;
        }
        const std::vector<std::string_view> fields = splitTabs(line);
        const LayoutFit fit = layoutFit(*opcode, fields);
        if (fit.column && fit.plain) {
            continue; // either layout's
        }
        if (fit.column && (blank(fields[1]) || isDateOrTime(fields[1]))) {
            ++column;
        } else if (fit.plain && !blank(fields[1])) {
            ++noColumn;
        }
    }
    if (column > 0 || noColumn > 0) {
        layout_ = column >= noColumn ? Layout::Column : Layout::NoColumn;
    }
    mixedLayout_ = column > 0 && noColumn > 0;
    if (mixedLayout_) {
        builder_.warn(0, std::to_string(column) + " fixed record(s) have the column after the "
                             "opcode (blank or a date or time) and " +
                             std::to_string(noColumn) + " do not; the file is read as " +
                             (layout_ == Layout::Column ? "having it" : "not having it") +
                             ", and a record that does not fit, or fits either way, is skipped");
    }
}

void FieldFileReader::fixedRecord(int opcode, const std::vector<std::string_view>& fields,
                                  std::size_t n)
{
    const std::size_t arity = *valuesAfterDescription(opcode);
    if (opcode != 3) {
        pointRecordStarts(n, opcode == 2 || opcode == 7);
    }
    const auto fail = [&](std::string message) {
        if (opcode == 3) {
            setupNotRead(n, std::move(message));
        } else {
            builder_.skip(n, std::move(message));
        }
    };
    const LayoutFit fit = layoutFit(opcode, fields);
    // A file no fixed record decided (none has an unambiguous count) is read
    // as [FLD] writes it, without the column.
    std::size_t columns = layout_ == Layout::Column ? 1 : 0;
    if (fit.column && fit.plain) {
        // A 04 of ten fields, or a record with a blank last field: a date or
        // time first says which; else the file's layout does, unless the
        // file has both.
        if (isDateOrTime(fields[1])) {
            columns = 1;
        } else if (mixedLayout_) {
            fail("opcode " + opcodeText(opcode) + " has " + std::to_string(fields.size() - 1) +
                 " values, which fit the format's layout both with the column after the opcode "
                 "and without it, in a file whose records show both; which value is which "
                 "cannot be told");
            return;
        }
    }
    if (!(columns == 1 ? fit.column : fit.plain)) {
        fail("opcode " + opcodeText(opcode) + " has " + std::to_string(fields.size() - 1) +
             " values where the format gives " + std::to_string(5 + arity + columns) +
             (columns == 1 ? " in this file (with the column after the opcode)"
                           : (layout_ == Layout::Undecided
                                  ? " (no record shows the column after the opcode as blank or "
                                    "a date or time, so the file is read without it)"
                                  : "")) +
             "; which value is which cannot be told");
        return;
    }
    if (columns == 1 && !blank(fields[1]) && !isDateOrTime(fields[1])) {
        fail("opcode " + opcodeText(opcode) + " holds '" +
             std::string(trimmed(fields[1]).substr(0, 40)) +
             "' in the column after the opcode, which is neither blank nor a date or time; "
             "which value is which cannot be told");
        return;
    }
    // One blank field past the count is a trailing tab.
    const std::size_t count =
        countFits(opcode, fields.size(), columns) ? fields.size() : fields.size() - 1;
    const std::size_t offset = 1 + columns;
    const Description d = Description::of(fields, offset);
    const std::vector<std::string_view> values(
        fields.begin() + static_cast<std::ptrdiff_t>(offset + 5),
        fields.begin() + static_cast<std::ptrdiff_t>(count));
    const std::string_view stamp = columns == 1 ? trimmed(fields[1]) : std::string_view{};
    if (blank(d.id())) {
        fail("opcode " + opcodeText(opcode) +
             " names its point by neither a point name nor a point ID");
        return;
    }
    switch (opcode) {
    case 2: coordinate(d, values, stamp, n); break;
    case 3: station(d, values, stamp, n); break;
    default: measurement(opcode, d, values, stamp, n); break;
    }
}

void FieldFileReader::record(int opcode, const std::vector<std::string_view>& fields,
                             std::size_t n)
{
    if (makesPointOrSetup(opcode)) {
        header_ = false;
        endGroups(0, n, "makes another point or a setup");
    }
    if (startsSetup(opcode)) {
        applyOffsets();
    }
    if (valuesAfterDescription(opcode)) {
        fixedRecord(opcode, fields, n);
        return;
    }
    const Values v = valuesOf(opcode, fields);
    if (!v.stamp.empty() && opcode != 128 && opcode != 138 && !unkeptStampWarned_) {
        unkeptStampWarned_ = true;
        builder_.warn(n, "the date and time in the column after the opcode is kept on the points "
                         "and setups records make; on this record, and any like it after, it "
                         "is not kept");
    }
    const std::string_view first =
        v.values.empty() ? std::string_view{} : trimmed(v.values[0]);
    switch (opcode) {
    case 100:
        units(v.values, n);
        return;
    case 5:
        if (const std::optional<double> height = number(first, "target height", n)) {
            targetHeight_ = *height;
            targetHeightSeen_ = true;
            builder_.countRead();
        } else {
            builder_.skip(n, "target height record without a height the reader can read");
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
            builder_.skip(n, "scale factor record without a positive factor the reader can read");
        }
        return;
    case 29:
    case -2: {
        std::string text;
        for (std::size_t i = 0; i < v.values.size(); ++i) {
            text += (i > 0 ? " " : "") + std::string(trimmed(v.values[i]));
        }
        builder_.addStationNote(trimmed(text));
        builder_.countRead();
        return;
    }
    case 41:
        additionalText(v.values, n);
        return;
    case 71:
    case 72:
    case 73:
        attribute(opcode, v.values, n);
        return;
    case 124:
        groupStart(v.values, n);
        return;
    case 125:
        groupEnd(v.values, n);
        return;
    case 16:
        multipleCoding(v, n);
        return;
    case 20:
        closeString(v, n);
        return;
    case 42:
    case 43:
    case 44:
        pointOffset(opcode, v, n);
        return;
    case 128:
    case 138:
        resection(opcode, v, n);
        return;
    case 129:
    case 139:
        resectionEnd(opcode, v, n);
        return;
    default:
        unread(opcode, n);
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
    // CR, LF and CRLF each end a line, so a record number is a line number
    // whichever system wrote the file.
    const std::vector<std::string_view> lines = katana::core::splitLines(decoded->text);
    decideLayout(lines);
    for (std::size_t i = 0; i < lines.size() && !fatal_; ++i) {
        const std::size_t n = i + 1;
        const std::string_view line = lines[i];
        // A line is read from its first character that is not a blank: the
        // opcode may have one before it, and so may a comment.
        const std::string_view lead =
            line.substr(std::min(line.size(), line.find_first_not_of(" \t")));
        if (blank(lead) || lead == "\x1A") {
            continue; // nothing, or a DOS end-of-file byte
        }
        if (lead.starts_with("//")) {
            comment(lead);
            continue;
        }
        if (firstRecord_ && lead.starts_with("{Version")) {
            // "{Version 6.0}": the file's own version line.
            firstRecord_ = false;
            builder_.project().metadata["version"] = std::string(trimmed(lead).substr(0, 60));
            builder_.countRead();
            continue;
        }
        firstRecord_ = false;
        const std::vector<std::string_view> fields = splitTabs(line);
        const std::string_view first = trimmed(fields[0]);
        const std::optional<int> opcode = opcodeOf(first);
        if (!opcode) {
            std::string why;
            if (first.empty()) {
                why = "the record's first field is empty, where its opcode belongs";
            } else if (first.find_first_not_of("+-0123456789") == std::string_view::npos) {
                why = "'" + std::string(first.substr(0, 20)) +
                      "' is not an opcode: an opcode is a number of one to three digits, a "
                      "minus before the negative ones";
            } else {
                why = "'" + std::string(first.substr(0, 20)) +
                      "' is not an opcode: a record opens with a number";
            }
            builder_.skip(n, std::move(why));
            continue;
        }
        if (*opcode == 99) {
            // "Stop processing ... at this line": what follows is not read,
            // and is counted.
            builder_.countRead();
            std::size_t after = 0;
            for (std::size_t j = i + 1; j < lines.size(); ++j) {
                const std::string_view rest = trimmed(lines[j]);
                after += rest.empty() || rest.starts_with("//") || rest == "\x1A" ? 0 : 1;
            }
            if (after > 0) {
                builder_.countSkipped(after);
                builder_.warn(n, std::to_string(after) + " record(s) after the end of the file "
                                 "(opcode 99) are not read, as the format says");
            }
            break;
        }
        record(*opcode, fields, n);
    }
    if (fatal_) {
        return *fatal_;
    }
    applyOffsets();
    for (std::size_t g = 0; g < groups_.size(); ++g) {
        builder_.warn(groupRecords_[g], "attribute group '" + groups_[g] +
                                            "' is not ended (opcode 125) before the file ends");
    }
    survey::SurveyProject& project = builder_.project();
    project.units.linear = survey::LinearUnit::Metres;
    project.units.angular = survey::AngularUnit::DecimalDegrees;
    if (!coordinateSystem_.empty()) {
        project.coordinateSystem = survey::DeclaredCoordinateSystem::named(
            zone_.empty() ? coordinateSystem_ : coordinateSystem_ + ", " + zone_);
    }
    if (!headerText_.empty()) {
        project.metadata["header"] = headerText_;
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
    if (resections_ > 0) {
        builder_.notCarried("no coordinates for the " + std::to_string(resections_) +
                            " setup(s) made by resection (opcode 128 or 138): the file does not "
                            "state them and the reduction does not compute a resection, so "
                            "those setups and what was measured from them are not positioned");
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
    // Every line kProbeBytes holds, so that a long header of comments does not
    // hide the records after it; kProbeRecords of them decide.
    for (std::string_view line : katana::surveyio::probeLines(input, katana::surveyio::kProbeBytes)) {
        if (lines == kProbeRecords) {
            break;
        }
        line = trimmed(line);
        if (line.empty() || line.starts_with("//")) {
            continue;
        }
        if (first && line.starts_with("{Version ")) {
            versionLine = true;
            first = false;
            continue;
        }
        first = false;
        ++lines;
        // The opcode as the reader takes it: trimmed (" 2" is opcode 2), and
        // a record even alone on its line ("20", "129").
        const std::size_t tab = line.find('\t');
        const std::optional<int> opcode = opcodeOf(trimmed(line.substr(0, tab)));
        if (!opcode) {
            continue;
        }
        ++records;
        // A coordinate, setup or shot of the field count the format gives it,
        // in either layout: a list of numbered rows ("   2<tab>E<tab>N<tab>Z")
        // opens with numbers too, but not with such records.
        if (tab != std::string_view::npos && (*opcode == 2 || *opcode == 3 || *opcode == 7)) {
            const LayoutFit fit = layoutFit(*opcode, splitTabs(line));
            measurements += fit.column || fit.plain ? 1 : 0;
        }
    }
    if (records == 0 || measurements == 0) {
        return katana::surveyio::ruledOut();
    }
    const double share = static_cast<double>(records) / static_cast<double>(lines);
    const std::string evidence = std::to_string(records) + " of " + std::to_string(lines) +
                                 " records open with a numeric opcode";
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
    // gnss stays false: a GNSS position written as an 02 arrives as an entered
    // grid coordinate, not as a GNSS observation.
    format.reads = {.points = true,
                    .observations = true,
                    .stations = true,
                    .features = true,
                    .coordinateSystem = true,
                    .instrumentSettings = false,
                    .gnss = false};
    format.canImport = true;
    // 1.1: the column's date and time, blank-padded opcodes, resections,
    // multiple coding, strings closed, offsets applied, attribute groups, and
    // every measurement's attributes kept.
    format.parserVersion = "1.1";
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
