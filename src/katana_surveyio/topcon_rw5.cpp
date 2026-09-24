// RW5 raw data: the comma-separated field journal of TDS Survey Pro, Carlson
// SurvCE and Topcon Magnet Field.
//
// Specifications this reader implements, and nothing beyond them:
//   [TDS]  Tripod Data Systems, Inc., "Raw Data Record Specification, Survey
//          Pro Version 3.6", October 2, 2002 - every record type and field
//          tag, the enumerations (AD, UN, AU, EC, GM, ME ...) and units ("See
//          MO for units").
//   [SCE]  Carlson Software, "SurvCE Raw Data File Format (*.rw5) - Version
//          3.03", updated February 25, 2014 - the direct/reverse set records
//          BD, BR, FD, FR, the GPS record, the BP base position with its
//          antenna fields, and the G0 to G4 GNSS vector records.
//   [SCE250] Carlson Software, "SurvCE Version 2.50 Raw File records",
//          updated 5/4/2010 - the units and antenna points of the GNSS
//          vector: "The DX, DY and DZ values are phase center to phase
//          center. ALL THE VALUES ARE ALWAYS IN METERS"; "You will get the rod
//          height of the rover from the LS record prior to the vector records.
//          The LS,HR value is from phase center to the ground ... THE UNITS CAN
//          BE FEET OR METERS" (the MO unit); and the BP record's AG and PA,
//          always metres, added "to get the Phase Center to Ground value".
//   [CSP]  Carlson Survey 2019 help, "Edit-Process Raw Data File": "The Set
//          Azimuth is the circle reading of the instrument when sighting the
//          backsight", and that the MO scale factor "is multiplied by the
//          horizontal distance" and earth curvature "adjusts the calculated
//          points" when the file is PROCESSED - so neither is in a recorded
//          distance or angle.
//
// Readings of the specification that are this reader's own, stated here so
// that they can be checked:
//   * AR (angle right) is read as the horizontal CIRCLE reading, and BC (back
//     circle) as the circle reading on the backsight; each AR becomes a
//     survey::HorizontalDirectionObservation and BC the setup's
//     backsightAzimuth (which the model defines as that reading). The
//     evidence is [SCE]'s own sample: the sideshot to the backsight point
//     (SS,OP1,FP2,AR0.0044 after BK,OP1,BP2,BS315.0000,BC0.0044) reads exactly
//     the back circle, and its reverse set records BR at 180 00' 37" - a
//     circle reading, not an angle from the backsight. Where BC is zero, the
//     usual case, both readings agree.
//   * BS, the backsight AZIMUTH, has no field in the model (backsightAzimuth
//     is the circle reading): it is kept in the setup's metadata as
//     "backsight azimuth (radians)". [TDS]'s field list gives BS the type
//     "Linear Distance", which its own sample (BS315.0000) contradicts.
//   * Zeniths above 180 degrees are face-right readings: the observation
//     holds 360 - z with Face::Right. BD/FD are face left and BR/FR face
//     right by their record type; a zenith that disagrees is a warning.
//   * CE (change in elevation) is a line-of-sight height difference, like
//     one computed from ZE and SD; it becomes a LevelDifferenceObservation of
//     CE + HI - HR, ground to ground ([CSP]: LS "sets the instrument and rod
//     heights used in elevation calculations").
//   * EO, the EDM offset in inches, is the setup's prism constant; the
//     specification does not say whether a recorded distance includes it, so
//     its state is Unknown.
//   * The description after "--" is the point's description; its first word
//     is the field code that strings the point into a feature.
//   * OF (off-centre shot) does not say which shot it corrects, so it is not
//     applied to one: every field it carries ([SCE]'s AR, ZE and SD, [TDS]'s
//     OL, HD, VD and LR) is kept as written in the setup's metadata, with a
//     warning. FE (foresight elevation), listed by [SCE] without a record, is
//     kept in the target point's metadata.
//   * A SurvCE vector (G1) is in metres whatever the MO unit, as [SCE250]
//     says; a Survey Pro baseline (BL) is in the MO unit, as [TDS] says ("See
//     MO for units"). The vector runs from phase centre to phase centre
//     ([SCE250]) unless a G4 record names another point for an end: its base
//     end is the last BP record's AG (to the reference point) plus its PA
//     (to the phase centre), its rover end the LS HR in force at the G1.
//   * Nothing in a record is dropped without a word: every field the
//     specifications define for a record is read into the model or into
//     metadata; a field they do not define is warned about at its first
//     record and counted at the end; a note on a record with no use for one
//     joins the setup's notes; and a shot giving two of a group [SCE] says to
//     give one of (AR/AL/AZ/BR/DR/DL, ZE/VA/CE, SD/HD) reads the first and
//     names the others.
//
// Records the specifications define that carry no survey observation or
// coordinate - stake-out, cut sheets, slope staking, calibration, projection
// and legacy levelling records - are warned about by number and counted as
// skipped, never dropped silently.

#include <array>
#include <charconv>
#include <cmath>
#include <cstdint>
#include <map>
#include <numbers>
#include <optional>
#include <string>
#include <string_view>
#include <system_error>

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

constexpr std::string_view kFormatId = "tds-rw5";
constexpr double kPi = std::numbers::pi;
constexpr double kMetresPerInch = 0.0254;

// ---- Records ----------------------------------------------------------------

struct Field {
    std::string_view tag;
    std::string_view value;
};

// One record: its type and its tagged fields. A fixed array, so reading a
// record allocates nothing; no record in the specifications has more than 16
// fields, and a record with more is read up to the cap with a warning.
struct Record {
    std::string_view type;
    std::array<Field, 16> fields;
    std::size_t count = 0;
    bool overflow = false;
    std::string_view note; // the text after "--", commas and all
    bool hasNote = false;
    // The fields a handler has looked at, one bit each. A field no handler
    // asked for is one this reader does not know, and Rw5Reader::record says
    // so rather than let it vanish.
    mutable std::uint32_t looked = 0;
    mutable bool noteLooked = false;

    [[nodiscard]] std::optional<std::string_view> find(std::string_view tag) const
    {
        for (std::size_t i = 0; i < count; ++i) {
            if (fields[i].tag == tag) {
                looked |= 1u << i;
                return fields[i].value;
            }
        }
        return std::nullopt;
    }
};

bool isUpper(char c)
{
    return c >= 'A' && c <= 'Z';
}

bool startsNumber(char c)
{
    return (c >= '0' && c <= '9') || c == '-' || c == '+' || c == '.' || c == ' ';
}

// [TDS] record 30, "BL,DC%s,PN%s,DX%s,DY%s,DZ%s,--%s,GM%s,CL%s,HP%s,VP%s", is
// the one record whose note is followed by fields. One of those is its tag
// written straight after the comma and a number (GM and CL are enumerations,
// HP and VP distances), so a note's own ", HP 2" is not taken for one.
bool isBaselineTailField(std::string_view piece)
{
    if (piece.size() < 2) {
        return false;
    }
    const std::string_view tag = piece.substr(0, 2);
    if (tag != "GM" && tag != "CL" && tag != "HP" && tag != "VP") {
        return false;
    }
    const std::string_view value = trimmed(piece.substr(2));
    return value.empty() || (piece[2] != ' ' && startsNumber(value.front()));
}

// Where a BL record's note, starting at `noteStart`, ends: before the run of
// tail fields that closes the line. Found from the end, so that a comma in
// the note stays in it ("--CP fence, north,GM4,CL1,..."), and in one pass
// over the line.
std::size_t baselineNoteEnd(std::string_view line, std::size_t noteStart)
{
    std::size_t end = line.size();
    while (end > noteStart) {
        const std::size_t comma = line.rfind(',', end - 1);
        if (comma == std::string_view::npos || comma < noteStart) {
            break;
        }
        const std::string_view piece = line.substr(comma + 1, end - comma - 1);
        if (!trimmed(piece).empty() && !isBaselineTailField(piece)) {
            break;
        }
        end = comma;
    }
    return end;
}

// Splits `line` into a Record. [TDS]: "Raw data records are comma delimited";
// each field starts with its two-letter header, except northing and easting,
// whose header is "N " and "E " ("the header is N space"); a note field "--"
// is free text and runs to the end of the line, except on a BL record.
Record splitRecord(std::string_view line)
{
    Record record;
    std::size_t cut = line.find(',');
    record.type = trimmed(line.substr(0, cut));
    // The G0 record ([SCE]) is free text after its type: date, time, base id.
    if (record.type == "G0") {
        record.hasNote = true;
        record.note = cut == std::string_view::npos ? std::string_view{} : line.substr(cut + 1);
        return record;
    }
    while (cut != std::string_view::npos) {
        const std::size_t start = cut + 1;
        const std::string_view rest = line.substr(start);
        const std::string_view leading = trimmed(rest);
        if (leading.starts_with("--") && !record.hasNote) {
            record.hasNote = true;
            const std::size_t noteStart =
                static_cast<std::size_t>(leading.data() - line.data()) + 2;
            if (record.type != "BL") {
                record.note = trimmed(line.substr(noteStart));
                return record;
            }
            const std::size_t noteEnd = baselineNoteEnd(line, noteStart);
            record.note = trimmed(line.substr(noteStart, noteEnd - noteStart));
            cut = noteEnd == line.size() ? std::string_view::npos : noteEnd;
            continue;
        }
        cut = line.find(',', start);
        const std::string_view text =
            trimmed(line.substr(start, cut == std::string_view::npos ? line.npos : cut - start));
        if (text.empty()) {
            continue;
        }
        Field field;
        if (text.size() >= 2 && (text[0] == 'N' || text[0] == 'E') && startsNumber(text[1])) {
            field.tag = text.substr(0, 1);
            field.value = trimmed(text.substr(1));
        } else {
            field.tag = text.substr(0, 2);
            field.value = text.size() > 2 ? trimmed(text.substr(2)) : std::string_view{};
        }
        if (record.count == record.fields.size()) {
            record.overflow = true;
            continue;
        }
        record.fields[record.count++] = field;
    }
    return record;
}

// ---- The record types the specifications define ------------------------------

// Records defined by [TDS] or [SCE] that hold no observation or coordinate
// Katana imports, with the reason given in the warning.
struct NotImported {
    std::string_view type;
    std::string_view why;
};

constexpr std::array kNotImported = {
    NotImported{"CF", "a cut sheet (stake-out result) is not survey data Katana imports"},
    NotImported{"DE", "a design point is a stake-out target, not a surveyed point"},
    NotImported{"DL", "a defined location is a computed design point"},
    NotImported{"MD", "a multiple-distance record names no target point"},
    NotImported{"OE", "an offset delta is a stake-out result"},
    NotImported{"RD", "a repeat-directional record names no target point"},
    NotImported{"RE", "a remote elevation names no target point"},
    NotImported{"RS", "a resection observation is not imported: its setup is not stated until "
                      "the resection is solved"},
    NotImported{"SD", "a deltas record is a stake-out result"},
    NotImported{"SL", "a slope-staking record is a stake-out result"},
    NotImported{"SR", "a slope-staking reference record is a stake-out result"},
    NotImported{"SU", "a sun observation is not imported"},
    NotImported{"CG", "COGO settings are not survey data"},
    NotImported{"CT", "a calibration residual is not survey data"},
    NotImported{"DG", "a datum grid file name is not survey data"},
    NotImported{"DT", "a datum definition is geodesy the import does not apply"},
    NotImported{"EE", "a GPS edit flag is not survey data"},
    NotImported{"ES", "an ellipsoid definition is geodesy the import does not apply"},
    NotImported{"GK", "a GPS stake-out record is a stake-out result"},
    NotImported{"GO", "a GPS offset shot needs a reduction the model cannot hold"},
    NotImported{"GP", "a GPS point type is not survey data"},
    NotImported{"HA", "a horizontal calibration is a transformation the import does not apply"},
    NotImported{"PE", "a projection definition is geodesy the import does not apply"},
    NotImported{"PJ", "a projection definition is geodesy the import does not apply"},
    NotImported{"RP", "a calibration point's local coordinates are not imported"},
    NotImported{"RX", "a receiver setup is not survey data"},
    NotImported{"ST", "local site settings are geodesy the import does not apply"},
    NotImported{"VA", "a vertical calibration is a transformation the import does not apply"},
    NotImported{"AA", "a legacy accumulating-angle record is not imported"},
    NotImported{"BB", "a legacy bench-level record is not imported"},
    NotImported{"BG", "a legacy geoid record is not imported"},
    NotImported{"BS", "a legacy bench-level record is not imported"},
    NotImported{"BT", "a legacy bench-level record is not imported"},
    NotImported{"HC", "a legacy control record is not imported"},
    NotImported{"LE", "a legacy vertical setup record is not imported"},
    NotImported{"LG", "a legacy geoid record is not imported"},
    NotImported{"LM", "a legacy mapping-plane record is not imported"},
    NotImported{"LH", "a legacy transformation record is not imported"},
    NotImported{"LV", "a legacy transformation record is not imported"},
    NotImported{"VC", "a legacy control record is not imported"},
};

// Every record type the reader or kNotImported knows, for the probe.
constexpr std::array<std::string_view, 30> kImported = {
    "JB", "MO", "OC", "BK", "LS", "SS", "TR", "OB", "BD", "BR", "FD", "FR", "SK", "RB", "RF",
    "SP", "AP", "GS", "GR", "FC", "DP", "AT", "CS", "GPS", "BP", "BL", "CV", "EP", "AH", "OF"};

bool isKnownType(std::string_view type)
{
    if (type == "EQ" || (type.size() == 2 && type[0] == 'G' && type[1] >= '0' && type[1] <= '4')) {
        return true;
    }
    for (const std::string_view known : kImported) {
        if (known == type) {
            return true;
        }
    }
    for (const NotImported& known : kNotImported) {
        if (known.type == type) {
            return true;
        }
    }
    return false;
}

// The field tags [TDS] and [SCE] define for each record type the reader
// imports, space separated. Every handler reads all of its record's; the list
// is what a warning about a field no handler read consults, so that it says
// truly whether the specifications define the field.
struct DefinedFields {
    std::string_view type;
    std::string_view tags;
};

constexpr std::string_view kShotFields = "OP FP AZ BR AR AL DR DL ZE VA CE SD HD HR FE";

constexpr std::array kDefinedFields = {
    DefinedFields{"JB", "NM DT TM"},
    DefinedFields{"MO", "AD UN SF EC EO AU"},
    DefinedFields{"OC", "OP N E EL"},
    DefinedFields{"BK", "OP BP BS BC"},
    DefinedFields{"LS", "HI HR"},
    DefinedFields{"SS", kShotFields},
    DefinedFields{"TR", kShotFields},
    DefinedFields{"OB", kShotFields},
    DefinedFields{"BD", kShotFields},
    DefinedFields{"BR", kShotFields},
    DefinedFields{"FD", kShotFields},
    DefinedFields{"FR", kShotFields},
    DefinedFields{"SK", "OP FP AR ZE SD"},
    DefinedFields{"RB", "OP BP AR ZE SD HR"},
    DefinedFields{"RF", "OP FP AR ZE SD HR"},
    DefinedFields{"OF", "AR ZE SD OL HD VD LR"},
    DefinedFields{"SP", "PN N E EL"},
    DefinedFields{"AP", "PN N E EL"},
    DefinedFields{"GS", "PN N E EL"},
    DefinedFields{"GR", "PN N E EL"},
    DefinedFields{"FC", "PN FN"},
    DefinedFields{"DP", "PN"},
    DefinedFields{"AT", "TN TV"},
    DefinedFields{"CS", "CO ZG ZN DN"},
    DefinedFields{"GPS", "PN LA LN EL"},
    DefinedFields{"BP", "PN LA LN HT SG EL AG PA AT SR"},
    DefinedFields{"BL", "DC PN DX DY DZ GM CL HP VP"},
    DefinedFields{"CV", "DC SV SC XX XY XZ YY YZ ZZ"},
    DefinedFields{"EP", "TM LA LN HT RH RV DH DV GM CL"},
    DefinedFields{"AH", "DC MA ME RA"},
    DefinedFields{"EQ", "DC RX RS AN AI AT TS TA HO VO"},
    DefinedFields{"G1", "BP PN DX DY DZ"},
    DefinedFields{"G2", "VX VY VZ"},
    DefinedFields{"G3", "XY XZ YZ"},
    DefinedFields{"G4", "BV RV"},
};

bool isDefinedField(std::string_view type, std::string_view tag)
{
    for (const DefinedFields& defined : kDefinedFields) {
        if (defined.type != type) {
            continue;
        }
        std::string_view tags = defined.tags;
        while (!tags.empty()) {
            const std::size_t space = tags.find(' ');
            if (tags.substr(0, space) == tag) {
                return true;
            }
            tags = space == std::string_view::npos ? std::string_view{} : tags.substr(space + 1);
        }
        return false;
    }
    return false;
}

// The name [TDS]'s "GPS Enumerated Fields List" gives a value, numbered from
// `first`; a value it does not list is kept as written, and said to be one.
template <std::size_t N>
std::string enumerated(std::string_view value, const std::array<std::string_view, N>& names,
                       int first)
{
    int number = 0;
    const auto [end, error] = std::from_chars(value.data(), value.data() + value.size(), number);
    if (error == std::errc{} && end == value.data() + value.size() && number >= first &&
        static_cast<std::size_t>(number - first) < N) {
        return std::string(names[static_cast<std::size_t>(number - first)]);
    }
    return "'" + std::string(value) + "', a value the specification does not list";
}

// CL, numbered from 0; DC and CO from 1.
constexpr std::array<std::string_view, 11> kClassifications = {
    "unknown", "normal",          "control",          "as-built",
    "check",   "backsight",       "deleted normal",   "deleted control",
    "deleted as-built", "deleted check", "deleted backsight"};
constexpr std::array<std::string_view, 4> kDerivations = {"base", "rover", "get base", "static"};
constexpr std::array<std::string_view, 4> kCoordinateSystemOptions = {
    "none", "scale only", "keyed in", "chosen from library"};

// Where a GNSS vector ends on an antenna: [SCE] G4's "APC Antenna Phase
// Center (L1)", "ARP Antenna Reference Point (Bottom of Antenna Mount)" and
// "UNK Unknown".
enum class AntennaPoint { PhaseCentre, ReferencePoint, NotStated };

// ---- Reading ---------------------------------------------------------------------

class Rw5Reader {
  public:
    Rw5Reader(std::string_view fileName, const ReadOptions& options)
        : builder_(fileName,
                   survey::SourceRecord{"Carlson", "RW5", "TDS3.6/SCE3.03", std::string(fileName),
                                        0},
                   options)
    {
    }

    Result<ReadResult> read(std::string_view bytes);

  private:
    void record(std::string_view line, std::size_t n);
    void dispatch(const Record& r, std::size_t n);
    void mode(const Record& r, std::size_t n);
    void job(const Record& r, std::size_t n);
    void occupy(const Record& r, std::size_t n);
    void backsight(const Record& r, std::size_t n);
    void lineOfSight(const Record& r, std::size_t n);
    void shot(const Record& r, std::size_t n);
    void storePoint(const Record& r, std::size_t n, survey::CoordinateSource how);
    void gnssPosition(const Record& r, std::size_t n);
    void basePosition(const Record& r, std::size_t n);
    void vectorRecord(const Record& r, std::size_t n);
    void baseline(const Record& r, std::size_t n);
    void covariance(const Record& r, std::size_t n);
    void geodeticPosition(const Record& r, std::size_t n);
    void antennaHeight(const Record& r, std::size_t n);
    void equipment(const Record& r, std::size_t n);
    void offCentre(const Record& r, std::size_t n);
    void finishVector();
    // The antennas at the two ends of the vector being assembled.
    survey::GnssAntenna baseEnd();
    survey::GnssAntenna roverEnd();
    void unreadFields(const Record& r, std::size_t n);
    void reportUnreadFields();

    // Values in the file's units, converted; a failure is a warning naming the
    // field, and nullopt. Linear values need a stated unit: before the first
    // MO record there is none, and that fails the file (fatal_).
    std::optional<double> linear(std::optional<std::string_view> value, std::string_view tag,
                                 std::size_t n);
    std::optional<double> angle(std::optional<std::string_view> value, std::string_view tag,
                                std::size_t n);
    std::optional<double> latitudeOrLongitude(std::optional<std::string_view> value,
                                              std::string_view tag, std::size_t n);
    std::optional<double> real(std::optional<std::string_view> value, std::string_view tag,
                               std::size_t n);
    // The current setup when it occupies `op`, or a new one there.
    survey::SurveyStation* stationAt(std::optional<std::string_view> op, std::size_t n,
                                     std::string_view type);
    void describePoint(std::string_view id, const Record& r, std::size_t n);

    RawProjectBuilder builder_;
    std::optional<katana::core::Error> fatal_;

    // What the last MO record said.
    bool modeSeen_ = false;
    double metresPerUnit_ = 0.0;
    bool gons_ = false;
    bool southAzimuths_ = false;
    survey::InstrumentSettings settings_;

    double instrumentHeight_ = 0.0;
    bool instrumentHeightStated_ = false;
    double rodHeight_ = 0.0;
    bool rodHeightStated_ = false;
    std::string lastPoint_;

    std::string basePoint_;
    survey::GnssAntenna baseAntenna_;
    survey::GnssAntenna roverAntenna_;
    bool roverAntennaStated_ = false; // an AH record gave the rover's height
    // The last BP record's AG (reference point to ground) and PA (reference
    // point to phase centre), metres, kept apart because a vector from that
    // base may start at either point (G4 BV), not only where the BP's own EL
    // was (its AT).
    std::optional<double> baseReferenceHeight_;
    std::optional<double> basePhaseOffset_;

    std::optional<survey::GnssGeocentricBaselineObservation> vector_;
    std::size_t vectorRecord_ = 0;
    std::string vectorNote_;
    // The vector's ends and what was in force at its G1: an LS or BP record
    // between the G1 and the next vector does not belong to this one.
    struct VectorEnds {
        AntennaPoint base = AntennaPoint::PhaseCentre; // [SCE250]: "phase center to
        AntennaPoint rover = AntennaPoint::PhaseCentre; // phase center" unless G4 says
        std::size_t record = 0; // the G4 that named them, else the G1
        survey::GnssAntenna baseAntenna;
        std::optional<double> baseReferenceHeight;
        std::optional<double> basePhaseOffset;
        survey::GnssAntenna roverAntenna;
        std::optional<double> rodHeight;
    };
    VectorEnds vectorEnds_;
    std::size_t lastGnss_ = std::string_view::npos; // index in project().observations

    // Fields no handler reads, by "<record type> <tag>": the record of the
    // first and how many records carried one. Warned once at the first and
    // summed at the end, so a collector that writes an unknown field on
    // every shot gives two warnings, not one per shot.
    struct Unread {
        std::size_t first = 0;
        std::size_t records = 0;
    };
    std::map<std::string, Unread, std::less<>> unread_;
};

std::optional<double> Rw5Reader::real(std::optional<std::string_view> value, std::string_view tag,
                                      std::size_t n)
{
    if (!value) {
        return std::nullopt;
    }
    const std::optional<double> parsed = parseReal(*value);
    if (!parsed) {
        builder_.warn(n, std::string(tag) + " '" + std::string(*value) + "' is not a number");
    }
    return parsed;
}

std::optional<double> Rw5Reader::linear(std::optional<std::string_view> value,
                                        std::string_view tag, std::size_t n)
{
    if (!value) {
        return std::nullopt;
    }
    if (!modeSeen_) {
        if (!fatal_) {
            fatal_ = makeError(ErrorCode::FileImportFailure,
                               "record " + std::to_string(n) + " of " + builder_.fileName() +
                                   " gives a distance or coordinate before any MO (mode) record "
                                   "has said whether the file is in feet, US survey feet or "
                                   "metres, and Katana will not guess",
                               "RW5 [TDS] section 3: the mode setup record states the units");
        }
        return std::nullopt;
    }
    const std::optional<double> parsed = real(value, tag, n);
    if (!parsed) {
        return std::nullopt;
    }
    return *parsed * metresPerUnit_;
}

std::optional<double> Rw5Reader::angle(std::optional<std::string_view> value, std::string_view tag,
                                       std::size_t n)
{
    if (!value) {
        return std::nullopt;
    }
    if (!modeSeen_) {
        if (!fatal_) {
            fatal_ = makeError(ErrorCode::FileImportFailure,
                               "record " + std::to_string(n) + " of " + builder_.fileName() +
                                   " gives an angle before any MO (mode) record has said "
                                   "whether angles are in degrees or grads, and Katana will not "
                                   "guess",
                               "RW5 [TDS] section 3: the mode setup record states the units");
        }
        return std::nullopt;
    }
    const std::optional<double> parsed =
        gons_ ? gonsToRadians(*value) : packedDegreesToRadians(*value);
    if (!parsed) {
        builder_.warn(n, std::string(tag) + " '" + std::string(*value) + "' is not an angle in " +
                             (gons_ ? "grads" : "degrees (DDD.MMSS)"));
    }
    return parsed;
}

std::optional<double> Rw5Reader::latitudeOrLongitude(std::optional<std::string_view> value,
                                                     std::string_view tag, std::size_t n)
{
    if (!value) {
        return std::nullopt;
    }
    // [TDS] field lists: "LA: Latitude ... Geodetic Angle (DMS)", whatever the
    // MO angle unit; [SCE]: "Lat(dd.mmss) Lon(dd.mmss - Negative for West)".
    const std::optional<double> parsed = packedDegreesToRadians(*value);
    if (!parsed) {
        builder_.warn(n, std::string(tag) + " '" + std::string(*value) +
                             "' is not a latitude or longitude in DD.MMSS");
    }
    return parsed;
}

Result<ReadResult> Rw5Reader::read(std::string_view bytes)
{
    std::size_t n = 0;
    std::size_t start = 0;
    // UTF-8 byte order mark: a file saved by a Windows editor carries one.
    if (bytes.starts_with("\xEF\xBB\xBF")) {
        start = 3;
    }
    while (start < bytes.size() && !fatal_) {
        std::size_t end = bytes.find('\n', start);
        if (end == std::string_view::npos) {
            end = bytes.size();
        }
        std::string_view line = bytes.substr(start, end - start);
        start = end + 1;
        ++n;
        if (!line.empty() && line.back() == '\r') {
            line.remove_suffix(1);
        }
        record(line, n);
    }
    if (fatal_) {
        return *fatal_;
    }
    finishVector();
    reportUnreadFields();
    if (!builder_.project().stations.empty()) {
        if (!instrumentHeightStated_) {
            builder_.notCarried("no instrument heights (no LS record states HI)");
        }
        if (!rodHeightStated_) {
            builder_.notCarried("no target heights (no LS record states HR)");
        }
        builder_.notCarried("no atmospheric settings: the RW5 format has no field for "
                            "temperature, pressure or ppm, so whether the distances are "
                            "corrected is unknown");
        builder_.notCarried("no instrument make, model or serial number");
    }
    ReadResult result = builder_.finish();
    const survey::SurveyProject& project = result.project;
    if (project.stations.empty() && project.observations.empty() && project.points.empty() &&
        project.unpositionedPoints.empty()) {
        // Nothing but warnings: say so rather than import an empty job quietly.
        return makeError(ErrorCode::FileImportFailure,
                         result.project.source.fileName +
                             " holds no RW5 setup, observation or point Katana could read",
                         std::to_string(result.warnings.size()) + " warning(s)" +
                             (result.warnings.empty()
                                  ? std::string{}
                                  : ", the first: " +
                                        katana::surveyio::describe(result.warnings.front())));
    }
    return result;
}

void Rw5Reader::record(std::string_view line, std::size_t n)
{
    const std::string_view text = trimmed(line);
    if (text.empty()) {
        return;
    }
    // [TDS] 1: the note record, a line that is only a comment.
    if (text.starts_with("--")) {
        builder_.addStationNote(trimmed(text.substr(2)));
        builder_.countRead();
        return;
    }
    const Record r = splitRecord(text);
    if (r.type.empty() || r.type.size() > 3 || !isUpper(r.type[0])) {
        builder_.skip(n, "'" + std::string(text.substr(0, 40)) + "' is not an RW5 record");
        return;
    }
    if (r.overflow) {
        builder_.warn(n, "record has more fields than any RW5 record defines; the extra fields "
                         "were not read");
    }
    const std::size_t skippedBefore = builder_.recordsSkipped();
    dispatch(r, n);
    // A record skipped as a whole has already been warned about; one that
    // was read must not lose a field quietly.
    if (!fatal_ && builder_.recordsSkipped() == skippedBefore) {
        unreadFields(r, n);
    }
}

void Rw5Reader::dispatch(const Record& r, std::size_t n)
{
    const std::string_view type = r.type;
    if (type == "MO") {
        mode(r, n);
    } else if (type == "JB") {
        job(r, n);
    } else if (type == "OC") {
        occupy(r, n);
    } else if (type == "BK") {
        backsight(r, n);
    } else if (type == "LS") {
        lineOfSight(r, n);
    } else if (type == "SS" || type == "TR" || type == "OB" || type == "BD" || type == "BR" ||
               type == "FD" || type == "FR" || type == "SK" || type == "RB" || type == "RF") {
        shot(r, n);
    } else if (type == "SP") {
        storePoint(r, n, survey::CoordinateSource::Unknown);
    } else if (type == "AP" || type == "GS" || type == "GR") {
        // Adjusted points and GPS-derived local coordinates: computed by the
        // data collector, not entered or observed.
        storePoint(r, n, survey::CoordinateSource::Calculated);
    } else if (type == "FC") {
        const std::optional<std::string_view> pn = r.find("PN");
        if (!pn || pn->empty()) {
            builder_.skip(n, "feature code record names no point");
            return;
        }
        const std::string_view code = r.find("FN").value_or(std::string_view{});
        builder_.codePoint(*pn, code, {}, {}, n);
        builder_.countRead();
    } else if (type == "DP") {
        const std::optional<std::string_view> pn = r.find("PN");
        if (!pn || pn->empty()) {
            builder_.skip(n, "deleted-point record names no point");
            return;
        }
        builder_.addPointMetadata(*pn, "deleted in the field", "record " + std::to_string(n));
        builder_.warn(n, "point '" + std::string(*pn) +
                             "' was deleted in the field; it is imported with a note in its "
                             "metadata, because its observations are still in the file");
        builder_.countRead();
    } else if (type == "AT") {
        if (lastPoint_.empty()) {
            builder_.skip(n, "attribute record before any point");
            return;
        }
        const std::string name(r.find("TN").value_or(std::string_view{}));
        builder_.addPointMetadata(lastPoint_, "attribute " + name,
                                  std::string(r.find("TV").value_or(std::string_view{})));
        builder_.countRead();
    } else if (type == "CS") {
        std::string name(r.find("ZG").value_or(std::string_view{}));
        const std::string zone(r.find("ZN").value_or(std::string_view{}));
        const std::string datum(r.find("DN").value_or(std::string_view{}));
        if (!zone.empty()) {
            name += name.empty() ? zone : " / " + zone;
        }
        if (!datum.empty()) {
            name += name.empty() ? datum : " (" + datum + ")";
        }
        if (!name.empty()) {
            builder_.project().coordinateSystem = survey::DeclaredCoordinateSystem::named(name);
        }
        // [TDS] CO: how the system was set up (1 none ... 4 from the library).
        if (const std::optional<std::string_view> option = r.find("CO")) {
            builder_.project().metadata["coordinate system option"] =
                enumerated(*option, kCoordinateSystemOptions, 1);
        }
        builder_.countRead();
    } else if (type == "GPS") {
        gnssPosition(r, n);
    } else if (type == "BP") {
        basePosition(r, n);
    } else if (type.size() == 2 && type[0] == 'G' && type[1] >= '0' && type[1] <= '4') {
        vectorRecord(r, n);
    } else if (type == "BL") {
        baseline(r, n);
    } else if (type == "CV") {
        covariance(r, n);
    } else if (type == "EP") {
        geodeticPosition(r, n);
    } else if (type == "AH") {
        antennaHeight(r, n);
    } else if (type == "EQ") {
        equipment(r, n);
    } else if (type == "OF") {
        offCentre(r, n);
    } else {
        for (const NotImported& known : kNotImported) {
            if (known.type == type) {
                builder_.skip(n, std::string(type) + " record not imported: " +
                                     std::string(known.why));
                return;
            }
        }
        builder_.skip(n, "'" + std::string(type) +
                             "' is not a record type the RW5 specification defines");
    }
}

void Rw5Reader::mode(const Record& r, std::size_t n)
{
    // [TDS] enumerations: UN 0 feet, 1 metre, 2 US survey feet; AU 0 degrees,
    // 1 grads; AD 0 north, 1 south; EC 0 off, 1 on.
    const std::optional<std::string_view> un = r.find("UN");
    survey::LinearUnit unit = survey::LinearUnit::Unknown;
    if (un == "0") {
        unit = survey::LinearUnit::Feet;
        metresPerUnit_ = kMetresPerFoot;
    } else if (un == "1") {
        unit = survey::LinearUnit::Metres;
        metresPerUnit_ = 1.0;
    } else if (un == "2") {
        unit = survey::LinearUnit::UsSurveyFeet;
        metresPerUnit_ = kMetresPerUsSurveyFoot;
    } else {
        fatal_ = makeError(ErrorCode::FileImportFailure,
                           "record " + std::to_string(n) + " of " + builder_.fileName() +
                               " sets the distance unit to '" +
                               std::string(un.value_or("(none)")) +
                               "', which is not one the RW5 specification defines (0 feet, 1 "
                               "metres, 2 US survey feet)",
                           "RW5 [TDS] enumerated fields: UN");
        return;
    }
    const std::optional<std::string_view> au = r.find("AU");
    survey::AngularUnit angular = survey::AngularUnit::DegreesMinutesSeconds;
    if (!au || au == "0") {
        gons_ = false;
        if (!au) {
            // The mode record of [SCE] 2006 lists no AU; the angle unit field
            // came later and degrees is what a file without it was written in.
            builder_.warn(n, "the mode record states no angle unit (AU); angles are read as "
                             "degrees (DDD.MMSS)");
        }
    } else if (au == "1") {
        gons_ = true;
        angular = survey::AngularUnit::Gons;
    } else {
        fatal_ = makeError(ErrorCode::FileImportFailure,
                           "record " + std::to_string(n) + " of " + builder_.fileName() +
                               " sets the angle unit to '" + std::string(*au) +
                               "', which is not one the RW5 specification defines (0 degrees, 1 "
                               "grads)",
                           "RW5 [TDS] enumerated fields: AU");
        return;
    }
    if (modeSeen_ && (builder_.project().units.linear != unit ||
                      builder_.project().units.angular != angular)) {
        builder_.warn(n, "the units change here; values after this record are converted with the "
                         "new units");
    }
    if (!modeSeen_) {
        builder_.project().units = survey::DeclaredUnits{unit, angular};
    }
    modeSeen_ = true;
    southAzimuths_ = r.find("AD") == "1";
    if (const std::optional<double> sf = real(r.find("SF"), "SF", n)) {
        settings_.scaleFactor = *sf;
        settings_.scaleFactorState = survey::CorrectionState::NotApplied; // [CSP]
    }
    if (const std::optional<std::string_view> ec = r.find("EC")) {
        // [CSP]: the collector's curvature setting adjusts the points it
        // CALCULATES; a raw zenith or slope distance never contains it, a
        // recorded HD or CE may when it was on.
        settings_.curvatureRefractionState =
            *ec == "0" ? survey::CorrectionState::NotApplied : survey::CorrectionState::Unknown;
        builder_.project().metadata["earth curvature (collector setting)"] =
            *ec == "0" ? "off" : "on";
    }
    if (const std::optional<double> eo = real(r.find("EO"), "EO", n)) {
        settings_.prismConstant = *eo * kMetresPerInch;
        settings_.prismConstantState = survey::CorrectionState::Unknown;
    }
    builder_.countRead();
}

void Rw5Reader::job(const Record& r, std::size_t)
{
    if (const std::optional<std::string_view> name = r.find("NM")) {
        builder_.project().name = std::string(*name);
    }
    if (const std::optional<std::string_view> date = r.find("DT")) {
        builder_.project().metadata["job date"] = std::string(*date);
    }
    if (const std::optional<std::string_view> time = r.find("TM")) {
        builder_.project().metadata["job time"] = std::string(*time);
    }
    builder_.countRead();
}

void Rw5Reader::describePoint(std::string_view id, const Record& r, std::size_t n)
{
    lastPoint_ = std::string(id);
    r.noteLooked = true;
    if (!r.hasNote || r.note.empty()) {
        return;
    }
    const std::size_t space = r.note.find_first_of(" \t");
    const std::string_view code = r.note.substr(0, space);
    builder_.codePoint(id, code, r.note, {}, n);
}

void Rw5Reader::occupy(const Record& r, std::size_t n)
{
    const std::optional<std::string_view> op = r.find("OP");
    if (!op || op->empty()) {
        builder_.skip(n, "occupy record names no point");
        return;
    }
    const std::optional<double> northing = linear(r.find("N"), "N", n);
    const std::optional<double> easting = linear(r.find("E"), "E", n);
    const std::optional<double> elevation = linear(r.find("EL"), "EL", n);
    if (fatal_) {
        return;
    }
    builder_.beginStation(*op, instrumentHeight_, settings_, n);
    if (northing && easting) {
        builder_.positionPoint(*op, *northing, *easting, elevation,
                               survey::CoordinateSource::Unknown, n);
    } else if (northing || easting) {
        builder_.warn(n, "occupied point '" + std::string(*op) +
                             "' has only one of northing and easting; it is imported without "
                             "coordinates");
    }
    describePoint(*op, r, n);
    builder_.countRead();
}

survey::SurveyStation* Rw5Reader::stationAt(std::optional<std::string_view> op, std::size_t n,
                                            std::string_view type)
{
    survey::SurveyStation* station = builder_.currentStation();
    if (station && (!op || op->empty() || *op == station->setup.pointId)) {
        return station;
    }
    if (!op || op->empty()) {
        return nullptr;
    }
    builder_.warn(n, std::string(type) + " record from '" + std::string(*op) +
                         "' with no occupy record for it; a setup there is started without "
                         "coordinates");
    return &builder_.beginStation(*op, instrumentHeight_, settings_, n);
}

void Rw5Reader::backsight(const Record& r, std::size_t n)
{
    const std::optional<std::string_view> op = r.find("OP");
    survey::SurveyStation* station = stationAt(op, n, "backsight");
    if (!station) {
        builder_.skip(n, "backsight record before any setup and naming no occupied point");
        return;
    }
    const std::optional<double> backCircle = angle(r.find("BC"), "BC", n);
    const std::optional<double> backAzimuth = angle(r.find("BS"), "BS", n);
    if (fatal_) {
        return;
    }
    // A backsight taken again after shots re-orients the circle: the shots
    // after it are read against a different zero, so they are a new setup.
    if (!station->observations.empty()) {
        const std::string point = station->setup.pointId;
        station = &builder_.beginStation(point, instrumentHeight_, settings_, n);
    }
    if (const std::optional<std::string_view> bp = r.find("BP"); bp && !bp->empty()) {
        if (*bp == station->setup.pointId) {
            builder_.warn(n, "the backsight point is the occupied point; it is not recorded as "
                             "the backsight");
        } else {
            builder_.mentionPoint(*bp, n);
            station->backsightPointId = std::string(*bp);
        }
    }
    if (backCircle) {
        station->backsightAzimuth = wrapToCircle(*backCircle);
    }
    if (backAzimuth) {
        const double azimuth = wrapToCircle(*backAzimuth + (southAzimuths_ ? kPi : 0.0));
        station->metadata["backsight azimuth (radians)"] = katana::core::formatExactReal(azimuth);
    }
    builder_.countRead();
}

void Rw5Reader::lineOfSight(const Record& r, std::size_t n)
{
    const std::optional<double> hi = linear(r.find("HI"), "HI", n);
    const std::optional<double> hr = linear(r.find("HR"), "HR", n);
    if (hi) {
        instrumentHeight_ = *hi;
        instrumentHeightStated_ = true;
        survey::SurveyStation* station = builder_.currentStation();
        if (station && station->observations.empty()) {
            station->setup.instrumentHeight = *hi;
        } else if (station && station->setup.instrumentHeight != *hi) {
            builder_.warn(n, "the instrument height changes in the middle of setup '" +
                                 station->setup.id +
                                 "'; each observation carries the height it was taken with");
        }
    }
    if (hr) {
        rodHeight_ = *hr;
        rodHeightStated_ = true;
    }
    if (!fatal_) {
        builder_.countRead();
    }
}

void Rw5Reader::shot(const Record& r, std::size_t n)
{
    const std::string_view type = r.type;
    const bool repeatBacksight = type == "RB";
    const std::optional<std::string_view> target = r.find(repeatBacksight ? "BP" : "FP");
    if (!target || target->empty()) {
        builder_.skip(n, std::string(type) + " record names no " +
                             (repeatBacksight ? "backsight" : "foresight") + " point");
        return;
    }
    survey::SurveyStation* station = stationAt(r.find("OP"), n, type);
    if (!station) {
        builder_.skip(n, std::string(type) + " record before any setup");
        return;
    }
    if (*target == station->setup.pointId) {
        builder_.skip(n, "a shot from '" + std::string(*target) + "' to itself");
        return;
    }
    // Every value first, so a unit failure leaves nothing half recorded.
    std::optional<double> targetHeight = linear(r.find("HR"), "HR", n);
    const std::optional<double> ar = angle(r.find("AR"), "AR", n);
    const std::optional<double> al = angle(r.find("AL"), "AL", n);
    const std::optional<double> az = angle(r.find("AZ"), "AZ", n);
    const std::optional<double> dr = angle(r.find("DR"), "DR", n);
    const std::optional<double> dl = angle(r.find("DL"), "DL", n);
    const std::optional<double> ze = angle(r.find("ZE"), "ZE", n);
    const std::optional<double> va = angle(r.find("VA"), "VA", n);
    const std::optional<double> ce = linear(r.find("CE"), "CE", n);
    const std::optional<double> sd = linear(r.find("SD"), "SD", n);
    const std::optional<double> hd = linear(r.find("HD"), "HD", n);
    const std::optional<std::string_view> bearing = r.find("BR");
    // [SCE] lists FE, foresight elevation, among the field headers without
    // saying which record carries it: the elevation the collector computed
    // for the target, kept with the point.
    const std::optional<double> fe = linear(r.find("FE"), "FE", n);
    if (fatal_) {
        return;
    }
    // [SCE]: a shot records ONE of each group. A record with two says two
    // things; the first in the specification's order is read and the rest
    // named, never dropped without a word.
    const auto oneOf = [&](std::initializer_list<std::pair<std::string_view, bool>> group) {
        std::string_view used;
        std::string extra;
        for (const auto& [tag, present] : group) {
            if (!present) {
                continue;
            }
            if (used.empty()) {
                used = tag;
            } else {
                extra += extra.empty() ? std::string(tag) : ", " + std::string(tag);
            }
        }
        if (!extra.empty()) {
            builder_.warn(n, "the record gives " + std::string(used) + " and also " + extra +
                                 ", where the specification allows one; " + std::string(used) +
                                 " is read and " + extra + " is not imported");
        }
    };
    oneOf({{"AR", ar.has_value()},
           {"AL", al.has_value()},
           {"AZ", az.has_value()},
           {"BR", bearing.has_value()},
           {"DR", dr.has_value()},
           {"DL", dl.has_value()}});
    oneOf({{"ZE", ze.has_value()}, {"VA", va.has_value()}, {"CE", ce.has_value()}});
    // CE is a height difference and HD its horizontal partner: the pair is
    // one choice of the vertical group, so only SD with HD is a conflict.
    oneOf({{"SD", sd.has_value()}, {"HD", hd.has_value() && !ce}});
    const double hi = instrumentHeight_;
    const double th = targetHeight.value_or(rodHeight_);
    const survey::ObservationPrecision& precision = builder_.options().precision;
    // Nothing below begins a setup, so the station and its id stay put.
    const std::string& at = station->setup.pointId;
    const std::string_view to = *target;
    builder_.mentionPoint(to, n);

    // The face of this pointing: the record type says so for a set, the
    // zenith reading does for a single shot.
    survey::Face face = survey::Face::Unknown;
    if (type == "BD" || type == "FD") {
        face = survey::Face::Left;
    } else if (type == "BR" || type == "FR") {
        face = survey::Face::Right;
    }
    std::optional<double> zenith;
    if (ze) {
        if (*ze < 0.0 || *ze >= 2.0 * kPi) {
            builder_.warn(n, "zenith reading outside a full circle; the zenith is not imported");
        } else {
            const survey::Face read = faceOfZenithReading(*ze);
            if (face == survey::Face::Unknown) {
                face = read;
            } else if (read != survey::Face::Unknown && read != face) {
                builder_.warn(n, std::string(type) + " is a " + survey::toString(face) +
                                     "-face record but its zenith reading is a " +
                                     survey::toString(read) + "-face one");
            }
            zenith = zenithInModelRange(*ze);
        }
    }
    const survey::Pointing pointing{builder_.nextPointing(), face};

    bool horizontal = false;
    if (ar || al) {
        // Angle right is the clockwise circle reading (see the file comment);
        // angle left the anticlockwise one.
        auto& observation = builder_.stationObservation<survey::HorizontalDirectionObservation>(n);
        observation.at = at;
        observation.to = to;
        observation.direction = ar ? wrapToCircle(*ar) : wrapToCircle(-*al);
        observation.sigma = precision.direction;
        observation.pointing = pointing;
        horizontal = true;
    } else if (az || bearing) {
        std::optional<double> azimuth;
        if (az) {
            azimuth = wrapToCircle(*az + (southAzimuths_ ? kPi : 0.0));
        } else {
            // [SCE]: "BR Bearing (this field will be recorded as N123.4500W)":
            // a quadrant letter, the angle, a quadrant letter.
            const std::string_view text = *bearing;
            if (text.size() >= 3 && (text.front() == 'N' || text.front() == 'S') &&
                (text.back() == 'E' || text.back() == 'W')) {
                const std::optional<double> b =
                    gons_ ? gonsToRadians(text.substr(1, text.size() - 2))
                          : packedDegreesToRadians(text.substr(1, text.size() - 2));
                if (b && *b <= kPi / 2.0) {
                    const bool north = text.front() == 'N';
                    const bool east = text.back() == 'E';
                    azimuth = wrapToCircle(north ? (east ? *b : -*b) : (east ? kPi - *b : kPi + *b));
                }
            }
            if (!azimuth) {
                builder_.warn(n, "bearing '" + std::string(text) +
                                     "' is not a quadrant bearing; the horizontal angle is not "
                                     "imported");
            }
        }
        if (azimuth) {
            auto& observation = builder_.stationObservation<survey::AzimuthObservation>(n);
            observation.from = at;
            observation.to = to;
            observation.azimuth = *azimuth;
            observation.sigma = precision.direction;
            horizontal = true;
        }
    } else if (dr || dl) {
        // A deflection is turned from the prolongation of the backsight line.
        if (station->backsightPointId.empty() || station->backsightPointId == to) {
            builder_.warn(n, "a deflection angle needs the setup's backsight point, which is not "
                             "stated; the horizontal angle is not imported");
        } else {
            auto& observation = builder_.stationObservation<survey::HorizontalAngleObservation>(n);
            observation.at = at;
            observation.from = station->backsightPointId;
            observation.to = to;
            observation.angle = dr ? wrapToCircle(kPi + *dr) : wrapToCircle(kPi - *dl);
            observation.sigma = precision.direction;
            observation.pointing = pointing;
            horizontal = true;
        }
    }
    if (!horizontal && !(ar || al || az || bearing || dr || dl)) {
        builder_.warn(n, "the shot states no horizontal angle");
    }

    if (zenith) {
        auto& observation = builder_.stationObservation<survey::ZenithAngleObservation>(n);
        observation.from = at;
        observation.to = to;
        observation.angle = *zenith;
        observation.sigma = precision.zenith;
        observation.instrumentHeight = hi;
        observation.targetHeight = th;
        observation.pointing = pointing;
    } else if (va) {
        if (std::abs(*va) > kPi / 2.0) {
            builder_.warn(n, "vertical angle beyond 90 degrees; it is not imported");
        } else {
            auto& observation = builder_.stationObservation<survey::VerticalAngleObservation>(n);
            observation.from = at;
            observation.to = to;
            observation.angle = *va;
            observation.sigma = precision.zenith;
            observation.instrumentHeight = hi;
            observation.targetHeight = th;
            observation.pointing = pointing;
        }
    } else if (ce) {
        // Line of sight to ground: the instrument above its mark, the target
        // above its own.
        const double heightDifference = *ce + hi - th;
        if (std::isfinite(heightDifference)) {
            auto& difference = builder_.stationObservation<survey::LevelDifferenceObservation>(n);
            difference.from = at;
            difference.to = to;
            difference.heightDifference = heightDifference;
            difference.sigma = std::hypot(precision.heightMeasurement, precision.heightMeasurement);
            difference.length = hd.value_or(0.0) > 0.0 ? *hd : 0.0;
        } else {
            builder_.warn(n, "the change in elevation and heights do not add up to a finite "
                             "height difference; it is not imported");
        }
    }

    const auto distance = [&](double value, survey::DistanceKind kind, std::string_view tag) {
        if (!(value > 0.0)) {
            builder_.warn(n, std::string(tag) + " of " + katana::core::formatExactReal(value) +
                                 " m read as no distance measured");
            return;
        }
        auto& observation = builder_.stationObservation<survey::DistanceObservation>(n);
        observation.from = at;
        observation.to = to;
        observation.distance = value;
        observation.sigma = survey::distanceSigma(precision, value);
        observation.kind = kind;
        observation.instrumentHeight = hi;
        observation.targetHeight = th;
        observation.pointing = pointing;
        // The EDM offset in force for THIS shot: an MO record in the middle
        // of a setup changes it for the shots after it, which the setup's
        // InstrumentSettings (taken when the setup began) cannot say.
        observation.target.prismConstant = settings_.prismConstant;
        observation.target.prismConstantState = settings_.prismConstantState;
    };
    if (sd) {
        distance(*sd, survey::DistanceKind::Slope, "SD");
    } else if (hd) {
        distance(*hd, survey::DistanceKind::Horizontal, "HD");
    }
    if (type == "TR") {
        builder_.addPointMetadata(to, "traverse foresight", "record " + std::to_string(n));
    }
    if (fe) {
        builder_.addPointMetadata(to, "foresight elevation (collector, m)",
                                  katana::core::formatExactReal(*fe));
    }
    describePoint(to, r, n);
    builder_.countRead();
}

void Rw5Reader::storePoint(const Record& r, std::size_t n, survey::CoordinateSource how)
{
    const std::optional<std::string_view> pn = r.find("PN");
    if (!pn || pn->empty()) {
        builder_.skip(n, std::string(r.type) + " record names no point");
        return;
    }
    const std::optional<double> northing = linear(r.find("N"), "N", n);
    const std::optional<double> easting = linear(r.find("E"), "E", n);
    const std::optional<double> elevation = linear(r.find("EL"), "EL", n);
    if (fatal_) {
        return;
    }
    if (northing && easting) {
        builder_.positionPoint(*pn, *northing, *easting, elevation, how, n);
    } else {
        builder_.mentionPoint(*pn, n);
        builder_.warn(n, "point '" + std::string(*pn) +
                             "' is stored without both northing and easting; it is imported "
                             "without coordinates");
    }
    describePoint(*pn, r, n);
    builder_.countRead();
}

survey::GnssSolution solutionOf(std::optional<std::string_view> gm)
{
    // [TDS] GM: 2 Autonomous, 3 RTKFloat, 4 RTKFixed, 6 RTCMCode, 7 WAAS.
    if (gm == "2") {
        return survey::GnssSolution::Autonomous;
    }
    if (gm == "3") {
        return survey::GnssSolution::Float;
    }
    if (gm == "4") {
        return survey::GnssSolution::Fixed;
    }
    if (gm == "6" || gm == "7") {
        return survey::GnssSolution::Differential;
    }
    return survey::GnssSolution::Unknown;
}

void Rw5Reader::gnssPosition(const Record& r, std::size_t n)
{
    const std::optional<std::string_view> pn = r.find("PN");
    if (!pn || pn->empty()) {
        builder_.skip(n, "GPS record names no point");
        return;
    }
    const std::optional<double> latitude = latitudeOrLongitude(r.find("LA"), "LA", n);
    const std::optional<double> longitude = latitudeOrLongitude(r.find("LN"), "LN", n);
    // [SCE]: "EL Ellipsoid Height (meters)" - metres whatever the MO unit.
    const std::optional<double> height = real(r.find("EL"), "EL", n);
    if (!latitude || !longitude || !height || std::abs(*latitude) > kPi / 2.0) {
        builder_.skip(n, "GPS record without a readable latitude, longitude and ellipsoid height");
        return;
    }
    builder_.mentionPoint(*pn, n);
    survey::GnssGlobalPositionObservation position;
    position.point = std::string(*pn);
    position.geodetic = survey::GeodeticCoordinate{*latitude, *longitude, *height};
    position.referenceFrame = "WGS 84";
    // [SCE] 3.03: "GPS heights always to be recorded to phase center"; the
    // rod height in force is that antenna height.
    position.antenna.height = rodHeight_;
    position.antenna.method = survey::AntennaHeightMethod::PhaseCentre;
    position.antenna.measuredTo = "phase centre (RW5 GPS record)";
    position.source = builder_.source(n);
    builder_.addLooseObservation(std::move(position));
    lastGnss_ = builder_.project().observations.size() - 1;
    builder_.notCarried("whether a GPS record's ellipsoid height is of the mark or of the "
                        "antenna is not stated; the rod height in force is recorded as the "
                        "antenna height to the phase centre");
    describePoint(*pn, r, n);
    builder_.countRead();
}

void Rw5Reader::basePosition(const Record& r, std::size_t n)
{
    const std::optional<std::string_view> pn = r.find("PN");
    if (!pn || pn->empty()) {
        builder_.skip(n, "base position record names no point");
        return;
    }
    const std::optional<double> latitude = latitudeOrLongitude(r.find("LA"), "LA", n);
    const std::optional<double> longitude = latitudeOrLongitude(r.find("LN"), "LN", n);
    // [SCE] writes EL in metres; [TDS] writes HT "(See MO for units)".
    std::optional<double> height;
    const std::optional<std::string_view> ht = r.find("HT");
    if (r.find("EL")) {
        height = real(r.find("EL"), "EL", n);
        if (ht) {
            builder_.warn(n, "the base record gives both EL and HT; EL is read and HT ('" +
                                 std::string(*ht) + "') is not imported");
        }
    } else {
        height = linear(ht, "HT", n);
    }
    if (fatal_) {
        return;
    }
    basePoint_ = std::string(*pn);
    builder_.mentionPoint(*pn, n);
    // [TDS] SG: the setup group, an identifier with no survey meaning here.
    if (const std::optional<std::string_view> group = r.find("SG")) {
        builder_.addPointMetadata(basePoint_, "setup group", std::string(*group));
    }
    // [SCE] BP: AG "Antenna distance from ARP (bottom of antenna) to Ground",
    // PA "Phase Center to ARP", both metres ([SCE250]). A new base replaces
    // both, stated or not.
    const std::optional<double> ag = real(r.find("AG"), "AG", n);
    const std::optional<double> pa = real(r.find("PA"), "PA", n);
    baseReferenceHeight_ = ag;
    basePhaseOffset_ = pa;
    // The antenna where the BP's own EL is: [SCE] BP AT, APC, ARP or UNK.
    survey::GnssAntenna antenna = baseAntenna_;
    const std::optional<std::string_view> at = r.find("AT");
    if (ag) {
        if (at == "APC") {
            antenna.height = *ag + pa.value_or(0.0);
            antenna.method = survey::AntennaHeightMethod::PhaseCentre;
            antenna.measuredTo = "L1 phase centre";
        } else if (at == "ARP") {
            antenna.height = *ag;
            antenna.method = survey::AntennaHeightMethod::Vertical;
            antenna.measuredTo = "antenna reference point";
        } else {
            antenna.height = *ag;
            antenna.method = survey::AntennaHeightMethod::Unknown;
            builder_.warn(n, "the base record does not say whether its height is at the phase "
                             "centre or the antenna reference point");
        }
    }
    if (const std::optional<std::string_view> sr = r.find("SR")) {
        builder_.addPointMetadata(basePoint_, "base position source", std::string(*sr));
    }
    if (pa) {
        builder_.addPointMetadata(basePoint_, "antenna phase centre offset (m)",
                                  katana::core::formatExactReal(*pa));
    }
    // A Survey Pro baseline (BL) from this base starts at the same antenna; a
    // SurvCE vector (G1) at the point its G4 names (baseEnd).
    baseAntenna_ = antenna;
    if (!latitude || !longitude || !height || std::abs(*latitude) > kPi / 2.0) {
        builder_.warn(n, "base position without a readable latitude, longitude and height; the "
                         "base point is imported without a position");
        describePoint(*pn, r, n);
        builder_.countRead();
        return;
    }
    survey::GnssGlobalPositionObservation position;
    position.point = basePoint_;
    position.geodetic = survey::GeodeticCoordinate{*latitude, *longitude, *height};
    position.referenceFrame = "WGS 84";
    position.antenna = antenna;
    position.source = builder_.source(n);
    builder_.addLooseObservation(std::move(position));
    lastGnss_ = builder_.project().observations.size() - 1;
    describePoint(*pn, r, n);
    builder_.countRead();
}

void Rw5Reader::finishVector()
{
    if (!vector_) {
        return;
    }
    if (!vectorNote_.empty()) {
        builder_.addPointMetadata(vector_->to, "GNSS vector note", vectorNote_);
    }
    const survey::GnssCovariance3& c = vector_->covariance;
    if (c.stated() && (!(c.xx > 0.0) || !(c.yy > 0.0) || !(c.zz > 0.0))) {
        builder_.warn(vectorRecord_, "the vector's variances are not all positive; its "
                                     "covariance is not imported");
        vector_->covariance = {};
    }
    // Only now: the G4 that names the ends comes after the G1.
    vector_->fromAntenna = baseEnd();
    vector_->toAntenna = roverEnd();
    builder_.addLooseObservation(std::move(*vector_));
    lastGnss_ = builder_.project().observations.size() - 1;
    vector_.reset();
    vectorNote_.clear();
}

survey::GnssAntenna Rw5Reader::baseEnd()
{
    const VectorEnds& ends = vectorEnds_;
    survey::GnssAntenna antenna = ends.baseAntenna;
    if (!ends.baseReferenceHeight) {
        // No BP record gave AG: the base antenna is whatever an AH record
        // said, as it said it.
        return antenna;
    }
    const double ag = *ends.baseReferenceHeight;
    antenna.height = ag;
    switch (ends.base) {
    case AntennaPoint::PhaseCentre:
        if (ends.basePhaseOffset) {
            // [SCE250]: AG and PA "add ... together to get the Phase Center
            // to Ground value".
            antenna.height = ag + *ends.basePhaseOffset;
            antenna.method = survey::AntennaHeightMethod::PhaseCentre;
            antenna.measuredTo = "L1: AG + PA of the base record";
        } else {
            antenna.method = survey::AntennaHeightMethod::Other;
            antenna.measuredTo = "antenna reference point (AG of the base record); the vector "
                                 "starts at the phase centre, and no PA gives the offset";
            builder_.warn(ends.record,
                          "the vector starts at the base's phase centre, but the base record "
                          "gives no PA (phase centre to reference point); its height is to the "
                          "reference point and the vector's height difference cannot be taken "
                          "to the marks");
        }
        break;
    case AntennaPoint::ReferencePoint:
        antenna.method = survey::AntennaHeightMethod::Vertical;
        antenna.measuredTo = "AG of the base record";
        break;
    case AntennaPoint::NotStated:
        antenna.method = survey::AntennaHeightMethod::Unknown;
        antenna.measuredTo = "antenna reference point (AG of the base record); the G4 record "
                             "does not say where the vector starts (BV UNK)";
        break;
    }
    return antenna;
}

survey::GnssAntenna Rw5Reader::roverEnd()
{
    const VectorEnds& ends = vectorEnds_;
    survey::GnssAntenna antenna = ends.roverAntenna;
    if (!ends.rodHeight) {
        return antenna; // an AH record's, or nothing (warned at the G1)
    }
    // [SCE250]: "The LS,HR value is from phase center to the ground".
    antenna.height = *ends.rodHeight;
    switch (ends.rover) {
    case AntennaPoint::PhaseCentre:
        antenna.method = survey::AntennaHeightMethod::PhaseCentre;
        antenna.measuredTo = "LS HR, phase centre to ground";
        break;
    case AntennaPoint::ReferencePoint:
        // The height is not the one this end needs, and no record gives the
        // rover's phase centre offset to make it so: not passed off as one.
        antenna.method = survey::AntennaHeightMethod::Other;
        antenna.measuredTo = "phase centre (LS HR); the vector ends at the antenna reference "
                             "point (G4 RV ARP), an offset from it no record gives";
        builder_.warn(ends.record,
                      "the vector ends at the rover's antenna reference point (RV ARP), but "
                      "SurvCE records the rover's rod height (LS HR) to the phase centre, and no "
                      "record gives the offset between them; the vector's height difference "
                      "cannot be taken to the marks");
        break;
    case AntennaPoint::NotStated:
        antenna.method = survey::AntennaHeightMethod::Unknown;
        antenna.measuredTo = "phase centre (LS HR); the G4 record does not say where the vector "
                             "ends (RV UNK)";
        break;
    }
    return antenna;
}

void Rw5Reader::vectorRecord(const Record& r, std::size_t n)
{
    // [SCE] G0 date, time and base id; G1 base, rover and the vector; G2 the
    // variances; G3 the covariances; G4 the antenna points.
    const char which = r.type[1];
    if (which == '0') {
        finishVector();
        r.noteLooked = true;
        vectorNote_ = std::string(trimmed(r.note));
        builder_.countRead();
        return;
    }
    if (which == '1') {
        const std::string note = vectorNote_;
        finishVector();
        vectorNote_ = note;
        const std::optional<std::string_view> from = r.find("BP");
        const std::optional<std::string_view> to = r.find("PN");
        // [SCE250]: "ALL THE VALUES ARE ALWAYS IN METERS", whatever MO says -
        // unlike a Survey Pro BL, whose components are in the MO unit.
        const std::optional<double> dx = real(r.find("DX"), "DX", n);
        const std::optional<double> dy = real(r.find("DY"), "DY", n);
        const std::optional<double> dz = real(r.find("DZ"), "DZ", n);
        if (!from || !to || from->empty() || to->empty() || *from == *to || !dx || !dy || !dz) {
            builder_.skip(n, "GNSS vector without two distinct points and all three components");
            vectorNote_.clear();
            return;
        }
        builder_.mentionPoint(*from, n);
        builder_.mentionPoint(*to, n);
        survey::GnssGeocentricBaselineObservation vector;
        vector.from = std::string(*from);
        vector.to = std::string(*to);
        vector.delta = survey::GeocentricCoordinate{*dx, *dy, *dz};
        vector.referenceFrame = "WGS 84";
        vector.source = builder_.source(n);
        vector_ = std::move(vector);
        vectorRecord_ = n;
        vectorEnds_ = VectorEnds{};
        vectorEnds_.record = n;
        vectorEnds_.baseAntenna = baseAntenna_;
        vectorEnds_.baseReferenceHeight = baseReferenceHeight_;
        vectorEnds_.basePhaseOffset = basePhaseOffset_;
        vectorEnds_.roverAntenna = roverAntenna_;
        if (rodHeightStated_) {
            vectorEnds_.rodHeight = rodHeight_;
        } else if (!roverAntennaStated_) {
            // [SCE250]: the rover's height is "the LS record prior to the
            // vector records"; with none, the rover end's height is unknown,
            // not zero.
            vectorEnds_.roverAntenna.method = survey::AntennaHeightMethod::Other;
            vectorEnds_.roverAntenna.measuredTo = "not stated: no LS record before the vector "
                                                  "gives the rover's rod height";
            builder_.warn(n, "no LS record before this vector gives the rover's rod height, so "
                             "the vector's height difference cannot be taken to the marks");
        }
        builder_.countRead();
        return;
    }
    if (!vector_) {
        builder_.skip(n, std::string(r.type) + " record with no G1 vector before it");
        return;
    }
    if (which == '2') {
        // Variances in square metres ([TDS] field list: "for GPS position (m2)").
        vector_->covariance.xx = real(r.find("VX"), "VX", n).value_or(0.0);
        vector_->covariance.yy = real(r.find("VY"), "VY", n).value_or(0.0);
        vector_->covariance.zz = real(r.find("VZ"), "VZ", n).value_or(0.0);
    } else if (which == '3') {
        vector_->covariance.xy = real(r.find("XY"), "XY", n).value_or(0.0);
        vector_->covariance.xz = real(r.find("XZ"), "XZ", n).value_or(0.0);
        vector_->covariance.yz = real(r.find("YZ"), "YZ", n).value_or(0.0);
    } else {
        // [SCE] G4: "Antenna point in Base, Antenna point in Rover"; the
        // heights follow in finishVector.
        const auto point = [&](std::optional<std::string_view> where, std::string_view tag,
                               AntennaPoint& into) {
            if (!where) {
                return;
            }
            if (*where == "APC") {
                into = AntennaPoint::PhaseCentre;
            } else if (*where == "ARP") {
                into = AntennaPoint::ReferencePoint;
            } else if (*where == "UNK") {
                into = AntennaPoint::NotStated;
            } else {
                builder_.warn(n, std::string(tag) + " '" + std::string(*where) +
                                     "' is not APC, ARP or UNK; that end of the vector is taken "
                                     "to be the phase centre");
            }
        };
        point(r.find("BV"), "BV", vectorEnds_.base);
        point(r.find("RV"), "RV", vectorEnds_.rover);
        vectorEnds_.record = n;
    }
    builder_.countRead();
}

void Rw5Reader::baseline(const Record& r, std::size_t n)
{
    const std::optional<std::string_view> to = r.find("PN");
    const std::optional<double> dx = linear(r.find("DX"), "DX", n);
    const std::optional<double> dy = linear(r.find("DY"), "DY", n);
    const std::optional<double> dz = linear(r.find("DZ"), "DZ", n);
    if (fatal_) {
        return;
    }
    if (basePoint_.empty() || !to || to->empty() || *to == basePoint_ || !dx || !dy || !dz) {
        builder_.skip(n, "GPS baseline without a base position before it, a rover point and "
                         "all three components");
        return;
    }
    builder_.mentionPoint(*to, n);
    survey::GnssGeocentricBaselineObservation vector;
    vector.from = basePoint_;
    vector.to = std::string(*to);
    vector.delta = survey::GeocentricCoordinate{*dx, *dy, *dz};
    vector.referenceFrame = "WGS 84";
    vector.fromAntenna = baseAntenna_;
    vector.toAntenna = roverAntenna_;
    vector.solution = solutionOf(r.find("GM"));
    vector.source = builder_.source(n);
    builder_.addLooseObservation(std::move(vector));
    lastGnss_ = builder_.project().observations.size() - 1;
    if (const std::optional<double> hp = linear(r.find("HP"), "HP", n)) {
        builder_.addPointMetadata(*to, "horizontal precision (m)",
                                  katana::core::formatExactReal(*hp));
    }
    if (const std::optional<double> vp = linear(r.find("VP"), "VP", n)) {
        builder_.addPointMetadata(*to, "vertical precision (m)", katana::core::formatExactReal(*vp));
    }
    // [TDS] DC: how the baseline was derived (1 base, 2 rover, 3 get base, 4
    // static); CL: the point's classification (1 normal, 2 control ...).
    if (const std::optional<std::string_view> dc = r.find("DC")) {
        builder_.addPointMetadata(*to, "GNSS derivation", enumerated(*dc, kDerivations, 1));
    }
    if (const std::optional<std::string_view> cl = r.find("CL")) {
        builder_.addPointMetadata(*to, "classification", enumerated(*cl, kClassifications, 0));
    }
    describePoint(*to, r, n);
    builder_.countRead();
}

void Rw5Reader::covariance(const Record& r, std::size_t n)
{
    if (lastGnss_ == std::string_view::npos) {
        builder_.skip(n, "covariance record with no GPS position or baseline before it");
        return;
    }
    survey::GnssCovariance3 c;
    c.xx = real(r.find("XX"), "XX", n).value_or(0.0);
    c.yy = real(r.find("YY"), "YY", n).value_or(0.0);
    c.zz = real(r.find("ZZ"), "ZZ", n).value_or(0.0);
    c.xy = real(r.find("XY"), "XY", n).value_or(0.0);
    c.xz = real(r.find("XZ"), "XZ", n).value_or(0.0);
    c.yz = real(r.find("YZ"), "YZ", n).value_or(0.0);
    if (!(c.xx > 0.0) || !(c.yy > 0.0) || !(c.zz > 0.0)) {
        builder_.skip(n, "covariance record whose variances are not all positive");
        return;
    }
    survey::Observation& target = builder_.project().observations[lastGnss_];
    std::string point;
    if (auto* vector = std::get_if<survey::GnssGeocentricBaselineObservation>(&target)) {
        vector->covariance = c;
        point = vector->to;
    } else if (auto* position = std::get_if<survey::GnssGlobalPositionObservation>(&target)) {
        // An X/Y/Z covariance cannot be the north/east/up one a geodetic
        // position carries (data_model.hpp); it is kept as text.
        point = position->point;
        builder_.addPointMetadata(point, "covariance XX YY ZZ XY XZ YZ (m2)",
                                  katana::core::formatExactReal(c.xx) + " " +
                                      katana::core::formatExactReal(c.yy) + " " +
                                      katana::core::formatExactReal(c.zz) + " " +
                                      katana::core::formatExactReal(c.xy) + " " +
                                      katana::core::formatExactReal(c.xz) + " " +
                                      katana::core::formatExactReal(c.yz));
    }
    // [TDS] CV: DC the derivation, SV "Minimum number of SV during
    // observation", SC "Error Scale" - kept with the point the covariance
    // belongs to (lastGnss_ is always a vector or a position, so one exists).
    if (const std::optional<std::string_view> dc = r.find("DC"); dc && !point.empty()) {
        builder_.addPointMetadata(point, "covariance derivation", enumerated(*dc, kDerivations, 1));
    }
    if (const std::optional<std::string_view> sv = r.find("SV"); sv && !point.empty()) {
        builder_.addPointMetadata(point, "minimum satellites", std::string(*sv));
    }
    if (const std::optional<std::string_view> sc = r.find("SC"); sc && !point.empty()) {
        builder_.addPointMetadata(point, "covariance error scale", std::string(*sc));
        // The specification does not say whether the variances already hold
        // the scale, so they are imported as written; a scale other than one
        // is worth a surveyor's look.
        if (const std::optional<double> scale = real(sc, "SC", n); scale && *scale != 1.0) {
            builder_.warn(n, "the covariance's error scale SC is " + std::string(*sc) +
                                 "; the variances are imported as written, not scaled, because "
                                 "the specification does not say whether they include it");
        }
    }
    builder_.countRead();
}

void Rw5Reader::geodeticPosition(const Record& r, std::size_t n)
{
    // [TDS] 39: "When a point is stored, its geodetic position is recorded" -
    // the record names no point; it belongs to the one stored before it.
    if (lastPoint_.empty()) {
        builder_.skip(n, "geodetic position with no stored point before it");
        return;
    }
    const std::optional<double> latitude = latitudeOrLongitude(r.find("LA"), "LA", n);
    const std::optional<double> longitude = latitudeOrLongitude(r.find("LN"), "LN", n);
    const std::optional<double> height = linear(r.find("HT"), "HT", n);
    if (fatal_) {
        return;
    }
    if (!latitude || !longitude || !height || std::abs(*latitude) > kPi / 2.0) {
        builder_.skip(n, "geodetic position without a readable latitude, longitude and height");
        return;
    }
    survey::GnssGlobalPositionObservation position;
    position.point = lastPoint_;
    position.geodetic = survey::GeodeticCoordinate{*latitude, *longitude, *height};
    position.antenna = roverAntenna_;
    position.solution = solutionOf(r.find("GM"));
    position.source = builder_.source(n);
    builder_.addLooseObservation(std::move(position));
    lastGnss_ = builder_.project().observations.size() - 1;
    for (const std::string_view tag : {"RH", "RV", "DH", "DV"}) {
        if (const std::optional<std::string_view> value = r.find(tag)) {
            builder_.addPointMetadata(lastPoint_, "receiver " + std::string(tag),
                                      std::string(*value));
        }
    }
    // [TDS] EP TM "System Time" (HH:MM:SS) and CL the classification.
    if (const std::optional<std::string_view> time = r.find("TM")) {
        builder_.addPointMetadata(lastPoint_, "GNSS time", std::string(*time));
    }
    if (const std::optional<std::string_view> cl = r.find("CL")) {
        builder_.addPointMetadata(lastPoint_, "classification",
                                  enumerated(*cl, kClassifications, 0));
    }
    builder_.countRead();
}

void Rw5Reader::antennaHeight(const Record& r, std::size_t n)
{
    // [TDS] AH: DC derivation (1 base, 2 rover ...), MA measured height, ME
    // method (0 unknown, 1 true, 2 uncorrected), RA reduced to phase centre.
    const std::optional<double> measured = linear(r.find("MA"), "MA", n);
    const std::optional<double> reduced = linear(r.find("RA"), "RA", n);
    if (fatal_) {
        return;
    }
    const bool base = r.find("DC") == "1";
    survey::GnssAntenna& antenna = base ? baseAntenna_ : roverAntenna_;
    if (!base && (measured || reduced)) {
        roverAntennaStated_ = true;
    }
    if (measured) {
        antenna.height = *measured;
        const std::optional<std::string_view> me = r.find("ME");
        antenna.method = me == "1"   ? survey::AntennaHeightMethod::Vertical
                         : me == "2" ? survey::AntennaHeightMethod::Slant
                                     : survey::AntennaHeightMethod::Unknown;
        antenna.measuredTo = me == "2" ? "uncorrected (slant) measurement" : "";
    } else if (reduced) {
        antenna.height = *reduced;
        antenna.method = survey::AntennaHeightMethod::PhaseCentre;
        antenna.measuredTo = "reduced to the phase centre by the data collector";
    }
    if (measured && reduced) {
        builder_.project().metadata["antenna height to phase centre, record " + std::to_string(n)] =
            katana::core::formatExactReal(*reduced);
    }
    builder_.countRead();
}

void Rw5Reader::equipment(const Record& r, std::size_t n)
{
    // [TDS] EQ: TA "Tape Adjustment", HO "Horizontal Offset" and VO "Vertical
    // Offset" are the antenna's measuring geometry, in the MO unit. An AH
    // record's RA already has them applied; the raw values are kept.
    const std::optional<double> tape = linear(r.find("TA"), "TA", n);
    const std::optional<double> horizontal = linear(r.find("HO"), "HO", n);
    const std::optional<double> vertical = linear(r.find("VO"), "VO", n);
    if (fatal_) {
        return;
    }
    survey::GnssAntenna& antenna = r.find("DC") == "1" ? baseAntenna_ : roverAntenna_;
    if (const std::optional<std::string_view> type = r.find("AT")) {
        antenna.type = std::string(*type);
    }
    if (const std::optional<std::string_view> serial = r.find("TS")) {
        antenna.serialNumber = std::string(*serial);
    }
    const std::string role = r.find("DC") == "1" ? "base" : "rover";
    auto& metadata = builder_.project().metadata;
    if (const std::optional<std::string_view> receiver = r.find("RX")) {
        metadata[role + " receiver"] = std::string(*receiver);
    }
    if (const std::optional<std::string_view> serial = r.find("RS")) {
        metadata[role + " receiver serial"] = std::string(*serial);
    }
    // AN and AI index the collector's own antenna table (Antenna.ini).
    if (const std::optional<std::string_view> number = r.find("AN")) {
        metadata[role + " antenna number"] = std::string(*number);
    }
    if (const std::optional<std::string_view> index = r.find("AI")) {
        metadata[role + " antenna index"] = std::string(*index);
    }
    if (tape) {
        metadata[role + " antenna tape adjustment (m)"] = katana::core::formatExactReal(*tape);
    }
    if (horizontal) {
        metadata[role + " antenna horizontal offset (m)"] =
            katana::core::formatExactReal(*horizontal);
    }
    if (vertical) {
        metadata[role + " antenna vertical offset (m)"] = katana::core::formatExactReal(*vertical);
    }
    builder_.countRead();
}

void Rw5Reader::offCentre(const Record& r, std::size_t n)
{
    // [SCE] OF: the angle right, the actual zenith and the slope distance of
    // an off-centre shot (a tree, a pole); [TDS] 16 adds the offset length
    // OL, the horizontal and vertical distances HD and VD and the left/right
    // offset LR. The specifications do not say which shot it corrects, so it
    // is not tied to one: every field is kept, as written and in the order
    // written, with the setup it was observed from.
    survey::SurveyStation* station = builder_.currentStation();
    if (station == nullptr) {
        builder_.skip(n, "off-centre shot before any setup");
        return;
    }
    std::string values;
    for (std::size_t i = 0; i < r.count; ++i) {
        values += (values.empty() ? "" : ", ") + std::string(r.fields[i].tag) + " " +
                  std::string(r.fields[i].value);
    }
    r.looked = UINT32_MAX;
    r.noteLooked = true;
    if (r.hasNote && !r.note.empty()) {
        values += (values.empty() ? "note " : ", note ") + std::string(r.note);
    }
    station->metadata["off-centre shot, record " + std::to_string(n) + " (file units)"] = values;
    builder_.warn(n, "the off-centre shot is kept in setup '" + station->setup.id +
                         "' metadata but not applied: the specification does not say which "
                         "shot it corrects");
    builder_.countRead();
}

void Rw5Reader::unreadFields(const Record& r, std::size_t n)
{
    if (r.hasNote && !r.noteLooked && !r.note.empty()) {
        // A note on a record whose handler has no use for one (a backsight,
        // a mode record): kept with the setup like a note line.
        builder_.addStationNote(std::string(r.type) + " note: " + std::string(r.note));
    }
    const std::uint32_t all = r.count >= 32 ? UINT32_MAX : (1u << r.count) - 1u;
    if ((r.looked & all) == all) {
        return;
    }
    for (std::size_t i = 0; i < r.count; ++i) {
        if ((r.looked & (1u << i)) != 0) {
            continue;
        }
        const std::string_view tag = r.fields[i].tag;
        const std::string_view value = r.fields[i].value.substr(0, 40);
        // Record::find reads the first of a tag given twice.
        bool repeated = false;
        for (std::size_t j = 0; j < i && !repeated; ++j) {
            repeated = r.fields[j].tag == tag;
        }
        if (repeated) {
            builder_.warn(n, "field '" + std::string(tag) +
                                 "' is given more than once; the first is read and this one "
                                 "(value '" + std::string(value) + "') is not imported");
            continue;
        }
        if (isDefinedField(r.type, tag)) {
            // Every handler reads its record's fields, so this is a field
            // it passed over here; said on every record, with its value.
            builder_.warn(n, "field '" + std::string(tag) + "' (value '" + std::string(value) +
                                 "') is one the RW5 specifications define for a " +
                                 std::string(r.type) + " record, but it is not imported here");
            continue;
        }
        std::string key(r.type);
        key += ' ';
        key += r.fields[i].tag;
        auto [entry, first] = unread_.try_emplace(std::move(key));
        ++entry->second.records;
        if (first) {
            entry->second.first = n;
            builder_.warn(n, "field '" + std::string(tag) + "' (value '" + std::string(value) +
                                 "') is not one the RW5 specifications define for a " +
                                 std::string(r.type) + " record; it is not imported, here or "
                                 "on any later " + std::string(r.type) + " record");
        }
    }
}

void Rw5Reader::reportUnreadFields()
{
    for (const auto& [key, unread] : unread_) {
        if (unread.records > 1) {
            builder_.warn(0, "the '" + key.substr(key.find(' ') + 1) + "' field of " +
                                 key.substr(0, key.find(' ')) + " records was not imported on " +
                                 std::to_string(unread.records) + " records, the first record " +
                                 std::to_string(unread.first));
        }
    }
}

// ---- Registration ---------------------------------------------------------------

FormatSignature probeRw5(const ProbeInput& input)
{
    const std::string_view bytes = katana::surveyio::withoutByteOrderMark(input.bytes);
    if (!katana::surveyio::looksLikeText(bytes)) {
        return katana::surveyio::ruledOut();
    }
    std::size_t lines = 0;
    std::size_t records = 0;
    std::size_t tps = 0;
    for (const std::string_view line : katana::surveyio::probeLines(input, 200)) {
        const std::string_view text = trimmed(line);
        if (text.empty()) {
            continue;
        }
        ++lines;
        if (text.starts_with("--")) {
            ++records;
            continue;
        }
        const std::size_t comma = text.find(',');
        if (comma == std::string_view::npos) {
            continue;
        }
        const std::string_view type = text.substr(0, comma);
        if (isKnownType(type)) {
            ++records;
            if (type == "MO" || type == "OC" || type == "BK" || type == "SS" || type == "LS" ||
                type == "TR" || type == "SP" || type == "GPS") {
                ++tps;
            }
        }
    }
    if (lines == 0 || records == 0) {
        return katana::surveyio::ruledOut();
    }
    const double share = static_cast<double>(records) / static_cast<double>(lines);
    const bool extension = input.extension == "rw5";
    std::string evidence = std::to_string(records) + " of " + std::to_string(lines) +
                           " lines are RW5 records";
    if (share < 0.8 || tps == 0) {
        return extension ? FormatSignature{0.4, "extension .rw5 but " + evidence}
                         : katana::surveyio::ruledOut();
    }
    if (extension) {
        return {0.97, "extension .rw5 and " + evidence};
    }
    return {0.85, evidence + " (MO, OC, BK, LS, SS ...)"};
}

FormatDescriptor descriptor()
{
    FormatDescriptor format;
    format.id = std::string(kFormatId);
    format.humanName = "RW5 raw data (TDS Survey Pro, Carlson SurvCE, Topcon Magnet)";
    format.manufacturer = katana::surveyio::Manufacturer::Carlson;
    format.reads = {.points = true,
                    .observations = true,
                    .stations = true,
                    .features = true,
                    .coordinateSystem = true,
                    .instrumentSettings = true,
                    .gnss = true};
    format.canImport = true;
    format.parserVersion = "1.0";
    format.extensions = {"rw5"};
    return format;
}

Result<ReadResult> readRw5(std::string_view bytes, std::string_view fileName,
                           const ReadOptions& options)
{
    Rw5Reader reader(fileName, options);
    return reader.read(bytes);
}

const katana::surveyio::FormatRegistration kRegistration{descriptor(), &probeRw5, &readRw5};

} // namespace
