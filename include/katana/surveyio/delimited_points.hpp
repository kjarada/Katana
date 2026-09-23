#pragma once

// Delimited point files: CSV, TXT, tab-, semicolon- or whitespace-separated
// coordinate lists, read and written through a column layout a person states
// (PLAN.MD section 45, slice 4).
//
// This is the format with no specification at all, which is why everything
// about how a file is read is a value the caller holds - a DelimitedLayout - and
// nothing is decided inside the parser. The layout is what the import wizard
// shows, what a person corrects, and what is saved as a template for the next
// file off the same instrument.
//
// THE ONE THING THIS FILE REFUSES TO GUESS is the order of the coordinates.
// "1,5000000.0,500000.0" is a point at northing 5 000 000 or at easting
// 5 000 000 and nothing in the numbers says which (PLAN.MD 45.5): a transposed
// survey plots, is self-consistent, and is in the wrong place. So the order
// comes from the layout, the layout comes from a person, and proposeLayout()
// only DECIDES when a header line names the columns; otherwise it says it is
// uncertain and a caller has to ask.
//
// What the reader does not do, by design (45.2): it transforms nothing. The
// coordinate system is recorded as unknown - a delimited file cannot declare
// one - and the linear unit is whatever the caller says the numbers are in,
// converted to metres on the way in as survey/data_model.hpp requires.

#include <cstddef>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/core/text_encoding.hpp"
#include "katana/survey/data_model.hpp"
#include "katana/surveyio/format.hpp"

namespace katana::surveyio {

// The id this format registers under (format.hpp's contract: a slug, never
// changed, never translated).
inline constexpr std::string_view kDelimitedPointsFormatId = "delimited-points";

// ---- The layout ------------------------------------------------------------------

// What one column of the file holds.
enum class ColumnRole {
    PointId,
    Northing,
    Easting,
    Elevation,
    Code,
    Description,
    Ignore, // read past, never stored
};

// "point id", "northing", ... - how an error message names a column.
[[nodiscard]] const char* toString(ColumnRole role);
// The letter a layout template spells the role with: P N E Z C D, and '-' for
// Ignore. Not 'X': in a survey file X is a coordinate, and a template in which X
// meant "skip this column" would be misread by the first person to open it.
[[nodiscard]] char templateLetter(ColumnRole role);
// The name export writes in a header line - "Point", "Northing", ... - chosen so
// that proposeLayout() reads an exported file back with the same layout. Empty
// for Ignore.
[[nodiscard]] const char* headerName(ColumnRole role);

enum class Delimiter {
    Comma,
    Tab,
    Semicolon,
    // A RUN of blanks (space, tab, vertical tab, form feed) separates fields, and
    // blanks at either end of a line separate nothing - which is what a
    // column-aligned TXT listing needs. It also means an empty field cannot be
    // written except as "" (quoted), so export refuses one when quoting is off.
    Whitespace,
};

[[nodiscard]] const char* toString(Delimiter delimiter); // "comma", "tab", ...

enum class Quoting {
    // RFC 4180: a field that BEGINS with '"' runs to the matching '"', a doubled
    // "" inside it is one quote, and it may hold the delimiter and line breaks.
    // A quote anywhere else in a field is an ordinary character, so a
    // description such as 6" PVC reads as written; RFC 4180 would call that
    // field malformed, but it has only one possible reading, and it is common
    // in survey descriptions.
    DoubleQuote,
    // '"' is an ordinary character everywhere.
    None,
};

[[nodiscard]] const char* toString(Quoting quoting); // "double", "none"

// How to read, or write, one delimited point file.
//
// The member defaults make a value that is easy to build in code; they are not
// what a file is assumed to be. A layout that reaches a parser comes from a
// person, a saved template (parseLayoutTemplate, which requires the delimiter
// and header count to be stated) or a proposal that says whether it is sure.
struct DelimitedLayout {
    // In file order. Northing and Easting are required; every other role at
    // most once, except Ignore, which may repeat.
    std::vector<ColumnRole> columns;
    Delimiter delimiter = Delimiter::Comma;
    // Physical lines skipped at the top of the file, whatever they hold. Lines,
    // not records: a header is not parsed, so a quote in one cannot run on.
    std::size_t headerLines = 0;
    // A line whose first non-blank characters are these is a comment. Empty for
    // none. May not start with a digit, a sign or '.', since then a line of data
    // could be a comment and a point would vanish; may not hold a blank, '"',
    // ';' (the template separator) or the delimiter.
    std::string commentPrefix;
    Quoting quoting = Quoting::DoubleQuote;

    friend bool operator==(const DelimitedLayout&, const DelimitedLayout&) = default;
};

// InvalidArgument, naming the problem, for a layout no file can be read with:
// no northing or easting column, a role twice, a comment prefix the rules above
// forbid. parse and write both call this first.
[[nodiscard]] katana::core::Status validateLayout(const DelimitedLayout& layout);

// ---- Presets ---------------------------------------------------------------------

// The column orders survey software names by their letters. PNEZD is the
// commonest export from civil design packages and PENZD from GIS and most
// instrument CSV exports; which one a file is in is the question proposeLayout()
// will not answer without a header.
enum class ColumnPreset { PNEZD, PENZD, PNEZ, PENZ, NEZ, ENZ, PNE, PEN };

// Every preset, in the order above - the order proposals list them in when
// nothing in the file separates them, which is NOT a statement that one is more
// likely.
[[nodiscard]] std::span<const ColumnPreset> allColumnPresets();
[[nodiscard]] const char* toString(ColumnPreset preset); // "PNEZD"
[[nodiscard]] std::vector<ColumnRole> presetColumns(ColumnPreset preset);
// Case-insensitive ("pnezd" is PNEZD); NotFound for any other name.
[[nodiscard]] katana::core::Result<ColumnPreset> columnPresetNamed(std::string_view name);
[[nodiscard]] DelimitedLayout presetLayout(ColumnPreset preset, Delimiter delimiter,
                                           std::size_t headerLines);

// ---- Templates -------------------------------------------------------------------
//
// A layout as one line of text, for saving and for showing in an import report:
//
//     P,N,E,Z,D;delimiter=comma;header=1;quote=double;comment=#
//
// The first part is the columns, one letter each (templateLetter), separated by
// commas. Then key=value parts separated by ';':
//     delimiter  comma | tab | semicolon | whitespace     REQUIRED
//     header     lines to skip, a decimal integer >= 0   REQUIRED
//     quote      double | none        absent means double (RFC 4180)
//     comment    the comment prefix   absent means no comment lines
// The two required keys are required because nothing in a file can make up for
// a wrong one: a comma file read on whitespace, or a data line taken for a
// header, is a wrong import rather than a failed one. The two optional ones have
// a stated meaning when absent, which is not a default guessed for them.
// Letters, keys and enumerated values are matched without regard to case and
// blanks around any part are ignored; everything else - an unknown letter, key
// or value, a key given twice, an empty part, a layout validateLayout() refuses
// - is InvalidArgument. A template that cannot be read is never read as
// something close to it.

// The canonical text: every key written, in the order above; `comment` only
// when there is a prefix. parseLayoutTemplate(layoutTemplate(x)) == x.
[[nodiscard]] std::string layoutTemplate(const DelimitedLayout& layout);
[[nodiscard]] katana::core::Result<DelimitedLayout> parseLayoutTemplate(std::string_view text);

// ---- Proposing a layout from the file itself ----------------------------------------

enum class LayoutProposalOutcome {
    // A header line names the northing and easting columns in words that mean
    // nothing else, names no column twice, and the sampled rows agree with it.
    Decided,
    // Something is not known - usually the coordinate order: no header, a header
    // that does not name the coordinates, X/Y, or rows that disagree with the
    // header. The candidates are what fits; a person chooses.
    Uncertain,
    // There was no text to look at.
    Empty,
};

[[nodiscard]] const char* toString(LayoutProposalOutcome outcome);

struct LayoutCandidate {
    DelimitedLayout layout;
    // Why this layout, and what it could not tell - "no header, so the order of
    // northing and easting is not stated". A person has to be able to check it.
    std::string evidence;

    friend bool operator==(const LayoutCandidate&, const LayoutCandidate&) = default;
};

// What proposeLayout() found. Built like Detection (detect.hpp) and for the same
// reason: the one accessor that hands back a usable layout, layout(), answers
// only when the outcome is Decided, so a caller cannot take the first candidate
// of an uncertain proposal by accident.
class LayoutProposal {
  public:
    [[nodiscard]] static LayoutProposal empty(std::string summary);
    [[nodiscard]] static LayoutProposal uncertain(std::vector<LayoutCandidate> ranked,
                                                  std::string summary);
    [[nodiscard]] static LayoutProposal decided(LayoutCandidate chosen, std::string summary);

    [[nodiscard]] LayoutProposalOutcome outcome() const { return outcome_; }
    // The layout to parse with when the outcome is Decided; NotFound otherwise,
    // with the summary as the message.
    [[nodiscard]] katana::core::Result<DelimitedLayout> layout() const;
    // Best first. For Decided, the one layout the header decided.
    [[nodiscard]] const std::vector<LayoutCandidate>& candidates() const { return candidates_; }
    // One sentence for a person: what was decided, or what was not and why.
    [[nodiscard]] const std::string& summary() const { return summary_; }

  private:
    LayoutProposalOutcome outcome_ = LayoutProposalOutcome::Empty;
    std::vector<LayoutCandidate> candidates_;
    std::string summary_;
};

// Content lines a proposal looks at: rows of data, a header and any title lines
// above it, not counting blank and comment lines. Policy, not a measurement: a
// few hundred rows is enough to see that a column is numeric throughout and few
// enough that proposing is instant on the 64 KiB a detection reads.
inline constexpr std::size_t kProposalSampleLines = 200;

// Reads the start of a file and proposes how to read it.
//
//   * The encoding is found as parseDelimitedPoints finds it (core::decodeText).
//   * The delimiter is the first of tab, semicolon, comma, whitespace that
//     splits every sampled row into at least two numbers. Tab first because it
//     is almost never content; semicolon before comma because a semicolon file
//     is often written with decimal commas, which a comma split would cut in
//     half; whitespace last because a description with a space in it would
//     otherwise split a CSV row.
//   * Lines above the first row of numbers are header lines, and the last of
//     them is read as the header when there is one.
//   * '#' at the start of a line is proposed as a comment prefix when a sampled
//     line starts with one. Nothing else is: a prefix that happened to match a
//     point id would drop the point.
//
// Header words, compared without case, blanks, '_', '-', '.', '#' or a
// bracketed unit such as "(m)":
//   point id     p pt pnt point id name no num number pointid pointname pointno
//                pointnumber ptid ptno ptnum
//   northing     n north northing          easting   e east easting
//   elevation    z elev elevation el height ht h rl reducedlevel level
//   code         c code featurecode feature fcode pointcode ptcode
//   description  d desc descr description remark remarks note notes comment comments
//   X, Y         read, but NEVER as a decision. In CAD and GIS, x runs east and y
//                north; in the geodetic convention of Gauss-Kruger grids -
//                Germany, Russia and much of central Europe - x is northing and y
//                easting. A file headed X,Y,Z is therefore Uncertain with both
//                readings listed, x-as-easting first because it is Katana's own
//                drawing convention, and a person says which.
//   latitude, longitude (lat, latitude, lon, long, longitude)  refused: this
//                reader takes grid coordinates, and degrees read as metres put
//                the survey a few hundred metres from the grid origin, where it
//                plots, rather than failing.
// Any other header word makes its column Ignore, and the evidence names it.
//
// `bytes` may be the whole file: only the first kProbeBytes (detect.hpp) are
// decoded, the budget a detection reads, so a proposal costs the same for a
// 50 MB file as for a 5 KB one. `truncated` says the bytes themselves stop part
// way through the file (a detection sample). Either way the last line, which
// may be cut, is not looked at. ParseFailure when the bytes cannot be decoded
// at all.
[[nodiscard]] katana::core::Result<LayoutProposal> proposeLayout(std::string_view bytes,
                                                                 bool truncated = false);

// ---- Import ----------------------------------------------------------------------

struct DelimitedImportOptions {
    // The unit the file's coordinates are in. REQUIRED: Unknown is refused,
    // because a delimited file never says and there is no default unit
    // (survey::metresPer says why).
    katana::survey::LinearUnit unit = katana::survey::LinearUnit::Unknown;
    // The encoding a person has stated, or nullopt to detect it. Detection
    // (core/text_encoding.hpp) reads UTF-8 with or without a byte order mark,
    // UTF-16 little- or big-endian with a mark or inferred from where the NULs
    // fall - Excel's "Unicode text" is UTF-16LE - and falls back to Windows-1252
    // for bytes that are not UTF-8, with a warning, since that one is a guess.
    std::optional<katana::core::TextEncoding> encoding;
};

// Reads a delimited point file with `layout`.
//
// All or nothing. Any of these is an error naming the line and the column, and
// no point is imported: a number that does not parse (core::parseFiniteDouble:
// no decimal comma, no "inf", no trailing text); an empty northing or easting;
// an empty point id where the layout has an id column; a line too short to reach
// a point id, northing or easting column; text in a field past the layout's
// last column; a quoted field that is never closed, or text after its closing
// quote; a point id used twice (AlreadyExists, naming both lines). A silently
// dropped row is a lost measurement.
//
// What is NOT an error:
//   * An empty elevation, or a line that ends before the elevation, code or
//     description column: the point has no elevation (absent, never 0) and an
//     empty code or description. Counted in the warnings.
//   * Blank lines, and lines starting with the comment prefix: skipped, and
//     comments counted in the warnings.
//   * A layout with NO point id column. Each point is then named by the number of
//     the line it is on ("12"), and a warning says so. Line numbers because they
//     are unique by construction, and because the id then says where in the file
//     the point came from - a sequence number would say neither.
//
// Unquoted fields are trimmed of blanks; a quoted field is kept exactly. Line
// numbers are 1-based physical lines of the decoded text, with CR, LF and CRLF
// each ending one; a record whose quoted field spans lines is numbered by the
// line it starts on. Only the file NAME of `fileName` is kept (45.2). Every
// point carries a SourceRecord whose formatVersion is the layout template it was
// read with, so an import can be repeated exactly. The project's coordinate
// system is unknown, its units are options.unit, and it passes
// survey::validateProject.
[[nodiscard]] katana::core::Result<ImportResult>
parseDelimitedPoints(std::string_view bytes, const DelimitedLayout& layout,
                     std::string_view fileName, const DelimitedImportOptions& options);

// ---- Export ----------------------------------------------------------------------

// The most decimals export writes. Policy: 12 places of a metre is a picometre,
// far below any survey measurement; a larger request is a caller's mistake.
inline constexpr int kMaximumExportDecimals = 12;

struct DelimitedExportOptions {
    // Decimal places for every coordinate, 0 to kMaximumExportDecimals.
    // REQUIRED: -1 is refused, because how many places a survey is published to
    // is the surveyor's statement, not this writer's.
    int decimals = -1;
    // The unit to write coordinates in. REQUIRED, as on import.
    katana::survey::LinearUnit unit = katana::survey::LinearUnit::Unknown;
};

// Writes `points` with `layout`: UTF-8 with no byte order mark (instrument
// upload tools refuse one more often than spreadsheets need one), CRLF line
// ends as RFC 4180 has them, one header line of headerName()s when
// layout.headerLines is 1, then one row per point. Coordinates in fixed notation
// with exactly `decimals` places, written without consulting the locale, and
// correctly rounded: a value exactly half way, such as 0.125 to two places, goes
// to the even digit (0.12), as IEEE 754 and printf round it. An
// absent elevation is an empty field, never 0 - quoted as "" when the delimiter
// is whitespace, where an unquoted empty field would vanish. A field is quoted
// when it holds the delimiter, a quote, a line break or blanks at either end,
// or when it would make the line read as a comment.
//
// Refused, InvalidArgument naming the point: a layout validateLayout() refuses;
// headerLines above 1 (this writer produces one header line and nothing to fill
// others with); decimals or unit unset; a coordinate that is not finite; with an
// id column, an empty or repeated id; and, with quoting off, a field that would
// need quotes. Everything refused here is something parseDelimitedPoints would
// refuse or misread, so what this writes, that reads back: with the same layout
// and unit, parse(write(x)) gives x's ids, codes and descriptions exactly, and
// its coordinates rounded to `decimals` places.
[[nodiscard]] katana::core::Result<std::string>
writeDelimitedPoints(std::span<const katana::survey::SurveyPoint> points,
                     const DelimitedLayout& layout, const DelimitedExportOptions& options);

// ---- The format, as the registry knows it -------------------------------------------

// Points only - a delimited file has no observations, setups or strings -
// import and export, parser version 1.0.
[[nodiscard]] FormatDescriptor delimitedPointsFormat();

// The detection probe. A WEAK candidate by design: rows of numbers separated by
// commas are consistent with a Leica, Trimble or Topcon CSV export and with any
// number of formats nobody has registered, so this probe never claims a file.
// Its highest answer is below kIdentifiedConfidence - kIdentificationMargin
// (detect.hpp), which means any specific format confident enough to be
// identified also beats this one by the margin, and a file only this probe
// recognises comes back Uncertain for a person to confirm.
[[nodiscard]] FormatSignature probeDelimitedPoints(const ProbeInput& input);

// The probe's two answers: a header naming the coordinates, and rows of numbers
// with no header. Policy numbers, ordered so that a header is better evidence.
inline constexpr double kDelimitedHeaderConfidence = 0.5;
inline constexpr double kDelimitedHeaderlessConfidence = 0.3;

} // namespace katana::surveyio
