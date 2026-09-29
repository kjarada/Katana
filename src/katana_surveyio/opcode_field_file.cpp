// The opcode field file (.fld): a survey job as tab-separated records, each
// opening with a numeric operation code - total-station setups, resections
// and shots, entered and GNSS coordinates, coded strings and their attributes.
//
// Specification this reader implements:
//   [FLD] "Field File Format", the format publisher's reference manual
//         chapter, in the edition the owner supplied: sections 44.2
//         "Structure of the .fld File", 44.3 "Point Description", 44.4
//         "Measurements and Named Measurements", 44.5 "Searching for Special
//         Coordinates", 44.6 (time_text), 44.7 and 44.8, one entry per opcode.
//         The June 2025 edition numbers the same sections 1.2 to 1.8 and
//         lacks 44.7's resection block and 44.8's descriptions of 128 and 129.
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
//   * 44.8, per opcode:
//       100 units, "only one choice for each": decimal degrees and metres;
//           any other unit is refused rather than guessed at.
//       02  a "directly entered coordinate": description, X, Y, Z, needing
//           "no reduction".
//       03  a setup on the point the description names, with its instrument
//           height. 128 a setup on an unknown point, whose coordinates "a
//           Least Squares Resection" computes from measurements to known
//           points; 129 ends its block, which "In some cases ... is not
//           necessary". 138 and 139 are the same for a Helmert resection.
//       04  the backsight: HA, VA, SD[, azimuth]. 06 a check measurement,
//           07 a measurement: HA, VA, SD, in decimal degrees and metres.
//       05  the target height for 04, 06 and 07 after it; 09 a scale factor
//           "to apply to subsequent slope distances".
//       16  "Additional coding for the current measurement point": a point at
//           the same position, strung by this record's code and number.
//       20  close string: with no description "the current string is
//           closed"; with one, the string its code and number (else its
//           point) names.
//       42, 43, 44  a radial, tangential or height offset of the current
//           measured point, or of the point a description names.
//       29  a memo; 41 text for the current measurement point, "any spaces
//           from column four onwards" being part of it; 71, 72, 73 an
//           integer, a real and a text attribute (name, value) of it, an
//           attribute whose name is blank being "unnamed"; -2 a comment.
//       99  "Stop processing ... at this line."
//
// Readings that are this reader's own, stated so they can be checked:
//   * THE COLUMN AFTER THE OPCODE. Every record with values in the files this
//     reader was written against has one field more than [FLD]'s syntax, at
//     the front ("07<tab><tab>KJ<tab>01..."). Where it is filled it holds the
//     date and time the record was made ("05/05/26 00:49:06.52" on each GNSS
//     coordinate of one file) - the counterpart of the time_surveyed 44.7
//     gives the XML form. The records whose opcode has a fixed number of
//     values (02, 03, 04, 06, 07) decide the file's layout before any is
//     read: one field more than [FLD] gives, the first of them empty or a
//     date and time, votes for the column; exactly [FLD]'s count with a
//     filled first field votes against. A record is not trusted to decide
//     alone because one of each can look the same: a record in the column's
//     layout that has lost a value has [FLD]'s count, and a feature code left
//     empty makes its first field empty. A fixed record that does not fit the
//     file's layout is a warning and is skipped - a guess at which value is
//     the slope distance reads a plausible survey that is wrong. A record
//     with free text takes the file's layout, and in a file with the column,
//     a filled first field that is not a date and time as a record written
//     without it. The date and time is kept as text, "time stamp", on the
//     point or setup its record makes: which of "05/05/26" is the day the
//     file does not say.
//   * An opcode may have a blank before it (" 2") or lack its leading zero
//     ("7"): [FLD] calls it numeric and writes 1 to 9 as 01 to 09, and
//     nothing makes 07 differ from 7.
//   * X is the easting and Y the northing. [FLD] says only "(x, y, z)"; the
//     files put six-figure eastings in X and seven-figure MGA northings in Y,
//     and one labels a reference station's Easting and Northing so.
//   * A point is identified by its point name where it has one and by its
//     point ID otherwise ([FLD] 44.5 finds a known point by either). A fixed
//     record with neither is skipped.
//   * The vertical circle is a zenith reading: [FLD] never states its zero,
//     and the files' readings near 90 and 270 degrees are zeniths on the two
//     faces. One past 180 degrees is face right, held as 360 - z.
//   * A GNSS POSITION WRITTEN AS 02. [FLD] gives a GNSS coordinate opcode 140
//     and says 140 "does not exist in the fld file", so a .fld carries an RTK
//     position as an 02. An 02 whose attributes say it is one - an attribute
//     group whose name holds "GPS" or "GNSS", or a "GNSS Solution" attribute
//     - is imported as CoordinateSource::Calculated (the receiver computed
//     it), not Entered ("keyed in or published"), which the reduction holds
//     as control. Its Z is the mark's, as [FLD]'s 02 needs "no reduction";
//     an antenna height among its attributes is kept, not subtracted again.
//   * ATTRIBUTE GROUPS (124 and 125). [FLD] defines them for the XML form
//     only (a name and a level, the group running "until" 125); the files
//     write 124 with an empty value, the name and the level, and 125 with an
//     empty name and the level. An attribute inside a group is kept as
//     "Group/Name" - a reference station's "Easting" is not the point's - and
//     nested groups as "Outer/Inner/Name", "/" being how Katana's properties
//     show a tree.
//   * An 02 that restates a point with other coordinates (a mark measured
//     twice) keeps the first coordinates, as the builder does; the attributes
//     and time stamp after the restatement are kept as "record N/...", so the
//     first measurement's are not overwritten by the second's.
//   * 04 and 06 targets take the attributes after them as a shot's does: the
//     files record the date, time and target height of a backsight and of a
//     check that way. A check (06) does not string its target into a feature
//     ([FLD] makes a check a one-vertex string of its own); its code is kept
//     in the point's "check measurement" metadata, and a backsight's (04) in
//     its setup's metadata, since [FLD] strings neither by it.
//   * A record that makes a point but is not read leaves no current point,
//     and a setup that is not read leaves no setup: what follows is skipped,
//     naming that record, rather than given to the point or setup before.
//   * OFFSETS (42, 43, 44) are read and kept in the point's metadata, with the
//     setup a radial or tangential one is from, but not applied: the model
//     holds what was measured, the reduction has no offset, and each is a
//     warning since the point is imported as measured.
//   * 128 (and 138) starts a setup on the point its description names:
//     [FLD]'s .fld syntax has the height only, and the files write the
//     description first, as the XML form carries one, and an empty value
//     after the height. With no point named the setup is on "resection at
//     record N". The file states no coordinates for it and the reduction
//     computes no resection, which notCarried says.
//   * 29 is a note on the setup (the project's before the first); 41 is kept
//     whole as "additional text N" on the point rather than joined to its
//     comment.
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

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <cstdint>
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
// Where a refusal points a person: [FLD]'s entries, one per opcode.
constexpr std::string_view kSpecification = "Field File Format 44.8";
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
        if (c < '0' || c > '9') {
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

bool isDigit(char c)
{
    return c >= '0' && c <= '9';
}

// Whether `text` is a date and a time: three numbers of one to four digits
// joined by one separator ('/', '-' or '.'), a blank or 'T', hours and
// minutes with optional seconds and decimals of a second, and an optional
// zone ('Z' or +hh:mm). That covers "05/05/26 00:49:06.52", as the files
// write it, and [FLD] 44.6's time_text, "2015-09-28T06:42:45Z". Nothing is
// read out of it (see the top of this file).
bool isDateTime(std::string_view text)
{
    std::size_t i = 0;
    const auto digits = [&](std::size_t most) {
        const std::size_t start = i;
        while (i < text.size() && i - start < most && isDigit(text[i])) {
            ++i;
        }
        return i > start;
    };
    const auto next = [&](char wanted) {
        if (i < text.size() && text[i] == wanted) {
            ++i;
            return true;
        }
        return false;
    };
    if (!digits(4) || i >= text.size()) {
        return false;
    }
    const char separator = text[i];
    if (separator != '/' && separator != '-' && separator != '.') {
        return false;
    }
    ++i;
    if (!digits(4) || !next(separator) || !digits(4)) {
        return false;
    }
    if (!next(' ') && !next('T')) {
        return false;
    }
    if (!digits(2) || !next(':') || !digits(2)) {
        return false;
    }
    if (next(':')) {
        if (!digits(2)) {
            return false;
        }
        if (next('.') && !digits(9)) {
            return false;
        }
    }
    if (!next('Z') && (next('+') || next('-'))) {
        if (!digits(2) || !next(':') || !digits(2)) {
            return false;
        }
    }
    return i == text.size();
}

// A comment that is only a rule ("//------") or nothing.
bool isDecoration(std::string_view text)
{
    return text.find_first_not_of("-=_*~ \t") == std::string_view::npos;
}

// Whether an attribute group's name says it describes a GNSS solution.
bool namesGnss(std::string_view name)
{
    const std::string lower = katana::core::lowered(name);
    return lower.find("gps") != std::string::npos || lower.find("gnss") != std::string::npos;
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

// Opcodes that make a point or a setup, read or not: the header comments end
// at the first of them.
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
    {.opcode = 50, .name = "backsight bearing"},
    {.opcode = 51, .name = "template start"},
    {.opcode = 52, .name = "template end"},
    {.opcode = 53, .name = "template pause"},
    {.opcode = 54, .name = "template continue"},
    {.opcode = 55, .name = "template record"},
    {.opcode = 56, .name = "template skip"},
    {.opcode = 57, .name = "template delete"},
    {.opcode = 58, .name = "template insert"},
    {.opcode = 59, .name = "template change"},
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
    // of a point that it does not string (a backsight, a check); empty when
    // it says none of that.
    [[nodiscard]] std::string coding() const
    {
        std::string text;
        const auto add = [&text](std::string_view label, std::string_view value) {
            if (!trimmed(value).empty()) {
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
        if (!trimmed(values[i]).empty()) {
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
    // column after the opcode; `stamp` is that column (empty, or a date and
    // time).
    struct Values {
        std::vector<std::string_view> values;
        std::string_view stamp;
    };
    [[nodiscard]] Values valuesOf(const std::vector<std::string_view>& fields) const;

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

    // The bookkeeping of the current point (see the members below).
    void pointRecordStarts(std::size_t n, bool isMeasurement);
    void pointRecordRead(const std::string& id, const Description& d, std::size_t n,
                         bool isMeasurement);
    // Whether a record about the current point has one; skips it otherwise.
    bool hasCurrentPoint(std::string_view what, std::size_t n);
    // Whether a record about the current measurement point has one.
    bool hasCurrentMeasurement(std::string_view what, std::size_t n);
    survey::SurveyStation& beginSetup(std::string_view pointId, std::optional<double> height,
                                      std::string_view stamp, std::size_t n);
    void setupNotRead(std::size_t n, std::string message);
    // An 02 that is the current point, shown by its attributes to be a GNSS
    // solution.
    void gnssEvidence();

    std::optional<double> number(std::string_view text, std::string_view what, std::size_t n);

    RawProjectBuilder builder_;
    std::optional<katana::core::Error> fatal_;
    Layout layout_ = Layout::Undecided;
    double targetHeight_ = 0.0;
    bool targetHeightSeen_ = false;
    double distanceScale_ = 1.0;
    bool scaleWarned_ = false;
    // Comments before the first record that makes a point or a setup are the
    // file's header.
    bool header_ = true;
    std::string headerText_;

    // The point the last 02, 04, 06 or 07 was about: 41 and 71 to 73 describe
    // it. Empty after such a record that was not read, which is then
    // `unreadPointRecord_`.
    std::string currentPoint_;
    std::size_t unreadPointRecord_ = 0;
    // [FLD] 44.4's current measurement point - the point of the last 02 or 07
    // - with the record, the feature code and the string number that made
    // it: 16, 20 and the offsets act on it and its string.
    std::string currentMeasurement_;
    std::size_t measurementRecord_ = 0;
    std::string currentCode_;
    std::string currentString_;
    std::size_t unreadMeasurementRecord_ = 0;
    // The record of the 02 that is the current point, until its GNSS evidence
    // is taken or another point record comes.
    std::size_t coordinateRecord_ = 0;
    // "record N/" on the attributes after an 02 that restated its point.
    std::string attributePrefix_;
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

FieldFileReader::Values FieldFileReader::valuesOf(const std::vector<std::string_view>& fields) const
{
    Values result;
    if (fields.size() < 2) {
        return result; // the opcode alone
    }
    const std::string_view first = fields[1];
    const bool columnLike = first.empty() || isDateTime(first);
    bool column = false;
    switch (layout_) {
    case Layout::Column: column = columnLike; break;
    case Layout::NoColumn: column = false; break;
    // No fixed record decided the file: a record with more than one value and
    // an empty (or dated) first is taken to have the column.
    case Layout::Undecided: column = columnLike && fields.size() > 2; break;
    }
    if (column) {
        result.stamp = first;
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
    coordinateRecord_ = 0;
    if (isMeasurement) {
        currentMeasurement_.clear();
        measurementRecord_ = 0;
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
    const RawProjectBuilder::Positioned positioned =
        builder_.positionPoint(id, *y, *x, z, survey::CoordinateSource::Entered, n);
    builder_.codePoint(id, d.featureCode, d.comment, d.stringNumber, n);
    pointRecordRead(id, d, n, true);
    coordinateRecord_ = n;
    if (positioned == RawProjectBuilder::Positioned::Restated) {
        attributePrefix_ = "record " + std::to_string(n) + "/";
    }
    if (!stamp.empty()) {
        builder_.addPointMetadata(id, attributePrefix_ + "time stamp", std::string(stamp));
    }
    builder_.countRead();
}

void FieldFileReader::station(const Description& d, const std::vector<std::string_view>& v,
                              std::string_view stamp, std::size_t n)
{
    beginSetup(d.id(), number(v[0], "instrument height", n), stamp, n);
    if (!d.featureCode.empty() || !d.comment.empty()) {
        builder_.codePoint(d.id(), d.featureCode, d.comment, d.stringNumber, n);
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
    } else if (opcode == 6) {
        const std::string coding = d.coding();
        builder_.addPointMetadata(target, "check measurement",
                                  "record " + std::to_string(n) +
                                      (coding.empty() ? "" : ", " + coding));
    } else {
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
    if (horizontal) {
        auto& observation = builder_.stationObservation<survey::HorizontalDirectionObservation>(n);
        observation.at = setup->setup.pointId;
        observation.to = target;
        observation.direction = wrapToCircle(*horizontal * kRadiansPerDegree);
        observation.sigma = precision.direction;
        observation.pointing = pointing;
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
    }
    pointRecordRead(target, d, n, opcode == 7);
    if (!stamp.empty()) {
        builder_.addPointMetadata(target, "time stamp", std::string(stamp));
    }
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
    } else if (values.size() == 6 || (values.size() == 7 && trimmed(values[6]).empty())) {
        d = Description::of(values, 0);
        height = values[5];
    } else {
        setupNotRead(n, "resection (opcode " + opcodeText(opcode) + ") with " +
                            std::to_string(values.size()) +
                            " value(s) where the format gives the instrument height, or a point "
                            "description and the height; which value is which cannot be told");
        return;
    }
    const std::string pointId = trimmed(d.id()).empty()
                                    ? "resection at record " + std::to_string(n)
                                    : std::string(d.id());
    survey::SurveyStation& setup =
        beginSetup(pointId, number(height, "instrument height", n), v.stamp, n);
    setup.metadata["resection"] = std::string(opcode == 128 ? "least squares" : "Helmert") +
                                    ", record " + std::to_string(n);
    if (!d.featureCode.empty() || !d.comment.empty()) {
        builder_.codePoint(pointId, d.featureCode, d.comment, d.stringNumber, n);
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
    if (trimmed(d.featureCode).empty()) {
        builder_.skip(n, what + " with no feature code: there is no string to add the point to");
        return;
    }
    // [FLD] makes a second point at the current point's position, strung by
    // this record's code and number. Katana strings the current point itself
    // into that string - one position, as [FLD]'s is - and keeps what this
    // record says of the second point with it.
    builder_.codePoint(currentMeasurement_, d.featureCode, "", d.stringNumber, n);
    if (!trimmed(d.id()).empty() || !trimmed(d.comment).empty()) {
        std::string said = featureName(d.featureCode, d.stringNumber);
        if (!trimmed(d.pointId).empty()) {
            said += ", point ID " + std::string(trimmed(d.pointId));
        }
        if (!trimmed(d.pointName).empty()) {
            said += ", point name " + std::string(trimmed(d.pointName));
        }
        if (!trimmed(d.comment).empty()) {
            said += ", comment " + std::string(trimmed(d.comment));
        }
        builder_.addPointMetadata(currentMeasurement_, "multiple coding record " + std::to_string(n),
                                  said);
        if (!trimmed(d.id()).empty()) {
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
        if (!trimmed(d.featureCode).empty()) {
            which = featureName(d.featureCode, d.stringNumber);
            points = builder_.closeFeature(d.featureCode, d.stringNumber);
        } else if (!trimmed(d.id()).empty()) {
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
    if (values.size() == 1) {
        // "If no point description is given, the offset is used to adjust
        // the position of the current measured point."
        if (!hasCurrentMeasurement(what, n)) {
            return;
        }
        target = currentMeasurement_;
        amountText = values[0];
    } else if (values.size() == 6) {
        const Description d = Description::of(values, 0);
        amountText = values[5];
        if (!trimmed(d.featureCode).empty()) {
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
        } else if (!trimmed(d.id()).empty() && builder_.hasPoint(d.id())) {
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
    std::string value = std::string(kind) + " " + katana::core::formatExactReal(*amount) + " m";
    if (const survey::SurveyStation* setup = builder_.currentStation();
        setup != nullptr && opcode != 44) {
        value += " from setup " + setup->setup.id;
    }
    builder_.addPointMetadata(target, "offset record " + std::to_string(n), value);
    builder_.warn(n, "the " + what + " of " + katana::core::formatExactReal(*amount) +
                         " m to point '" + target +
                         "' is kept in its metadata and not applied: the point is imported as "
                         "measured");
    builder_.countRead();
}

void FieldFileReader::attribute(int opcode, const std::vector<std::string_view>& values,
                                std::size_t n)
{
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
    std::string key = attributePrefix_;
    for (const std::string& group : groups_) {
        if (!group.empty()) {
            key += group + "/";
        }
    }
    // [FLD] 44.8, opcodes 68 to 79: a blank name makes the attribute unnamed.
    key += name.empty() ? "unnamed attribute (record " + std::to_string(n) + ")" : std::string(name);
    builder_.addPointMetadata(currentPoint_, std::move(key), std::move(value));
    if (opcode == 73 && name == "GNSS Solution") {
        gnssEvidence();
    }
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
        if (!depth || *depth != static_cast<std::int64_t>(groups_.size())) {
            builder_.warn(n, "attribute group '" + std::string(name) + "' gives the level '" +
                                 std::string(level.substr(0, 20)) + "' and opens inside " +
                                 std::to_string(groups_.size()) +
                                 " group(s); it is nested as the records open and end it");
        }
    }
    if (namesGnss(name)) {
        gnssEvidence();
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

void FieldFileReader::gnssEvidence()
{
    if (coordinateRecord_ == 0) {
        return;
    }
    (void)builder_.setCoordinateSource(currentPoint_, coordinateRecord_,
                                       survey::CoordinateSource::Calculated);
    coordinateRecord_ = 0;
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
    }
    builder_.skip(n, std::move(message));
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
        if (countFits(*opcode, fields.size(), 1) && (fields[1].empty() || isDateTime(fields[1]))) {
            ++column;
        } else if (countFits(*opcode, fields.size(), 0) && !fields[1].empty()) {
            ++noColumn;
        }
    }
    if (column > 0 || noColumn > 0) {
        layout_ = column >= noColumn ? Layout::Column : Layout::NoColumn;
    }
    if (column > 0 && noColumn > 0) {
        builder_.warn(0, std::to_string(column) + " fixed record(s) have the column after the "
                             "opcode (empty or a date and time) and " +
                             std::to_string(noColumn) + " do not; the file is read as " +
                             (layout_ == Layout::Column ? "having it" : "not having it") +
                             ", and a record that does not fit is skipped");
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
    // A file no fixed record decided (none has an unambiguous count) is read
    // as [FLD] writes it, without the column.
    const std::size_t columns = layout_ == Layout::Column ? 1 : 0;
    if (!countFits(opcode, fields.size(), columns)) {
        fail("opcode " + opcodeText(opcode) + " has " + std::to_string(fields.size() - 1) +
             " values where the format gives " + std::to_string(5 + arity + columns) +
             (columns == 1 ? " in this file (with the column after the opcode)" : "") +
             "; which value is which cannot be told");
        return;
    }
    if (columns == 1 && !fields[1].empty() && !isDateTime(fields[1])) {
        fail("opcode " + opcodeText(opcode) + " holds '" +
             std::string(trimmed(fields[1]).substr(0, 40)) +
             "' in the column after the opcode, which is neither empty nor a date and time; "
             "which value is which cannot be told");
        return;
    }
    const std::size_t offset = 1 + columns;
    const Description d = Description::of(fields, offset);
    const std::vector<std::string_view> values(fields.begin() + static_cast<std::ptrdiff_t>(offset + 5),
                                               fields.end());
    const std::string_view stamp = columns == 1 ? fields[1] : std::string_view{};
    if (trimmed(d.id()).empty()) {
        fail("opcode " + opcodeText(opcode) +
             " names its point by neither a point name nor a point ID");
        return;
    }
    switch (opcode) {
    case 2: coordinate(d, values, stamp, n); break;
    case 3: station(d, values, stamp, n); break;
    default: measurement(opcode, d, values, stamp, n); break;
    }
    // Named by its name, the point keeps its ID too - once the record
    // has made it, so its provenance is this record.
    if (!d.pointName.empty() && !d.pointId.empty() && builder_.hasPoint(d.pointName)) {
        builder_.addPointMetadata(d.pointName, "point ID", std::string(d.pointId));
    }
}

void FieldFileReader::record(int opcode, const std::vector<std::string_view>& fields,
                             std::size_t n)
{
    if (makesPointOrSetup(opcode)) {
        header_ = false;
    }
    if (valuesAfterDescription(opcode)) {
        fixedRecord(opcode, fields, n);
        return;
    }
    const Values v = valuesOf(fields);
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
        if (*opcode == 99) {
            // "Stop processing ... at this line": what follows is not read,
            // and is counted.
            builder_.countRead();
            std::size_t after = 0;
            for (std::size_t j = i + 1; j < lines.size(); ++j) {
                after += trimmed(lines[j]).empty() || lines[j].starts_with("//") ? 0 : 1;
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
        // The opcode as the reader takes it: trimmed (" 2" is opcode 2), and
        // a record even alone on its line ("20", "129").
        const std::size_t tab = line.find('\t');
        const std::optional<int> opcode = opcodeOf(trimmed(line.substr(0, tab)));
        if (!opcode) {
            continue;
        }
        ++records;
        if (tab != std::string_view::npos && (*opcode == 2 || *opcode == 3 || *opcode == 7)) {
            ++measurements;
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
    // gnss stays false: a GNSS position written as an 02 arrives as a grid
    // coordinate (CoordinateSource::Calculated), not as a GNSS observation.
    format.reads = {.points = true,
                    .observations = true,
                    .stations = true,
                    .features = true,
                    .coordinateSystem = true,
                    .instrumentSettings = false,
                    .gnss = false};
    format.canImport = true;
    // 1.1: the column's date and time, blank-padded opcodes, resections,
    // multiple coding, strings closed, offsets and attribute groups.
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
