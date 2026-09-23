// Proposing the layout of a delimited point file from its first lines.
//
// The rule this file exists to keep (PLAN.MD 45.5): the order of northing and
// easting is DECIDED only by a header that names them. Everything else - the
// delimiter, the header count, whether the first column is an id - is inferred,
// because a wrong inference there fails loudly in the parser (a column of text
// read as a number is an error with a line and a column). A wrong coordinate
// order fails silently, so it is never inferred.

#include <algorithm>
#include <array>
#include <optional>
#include <string>
#include <vector>

#include "delimited_internal.hpp"
#include "katana/core/text.hpp"
#include "katana/core/text_encoding.hpp"
#include "katana/surveyio/detect.hpp"

namespace katana::surveyio {

using katana::core::Error;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

// ---- LayoutProposal ----------------------------------------------------------------

const char* toString(LayoutProposalOutcome outcome)
{
    switch (outcome) {
    case LayoutProposalOutcome::Decided:
        return "decided";
    case LayoutProposalOutcome::Uncertain:
        return "uncertain";
    case LayoutProposalOutcome::Empty:
        return "empty";
    }
    return "uncertain";
}

LayoutProposal LayoutProposal::empty(std::string summary)
{
    LayoutProposal proposal;
    proposal.outcome_ = LayoutProposalOutcome::Empty;
    proposal.summary_ = std::move(summary);
    return proposal;
}

LayoutProposal LayoutProposal::uncertain(std::vector<LayoutCandidate> ranked, std::string summary)
{
    LayoutProposal proposal;
    proposal.outcome_ = LayoutProposalOutcome::Uncertain;
    proposal.candidates_ = std::move(ranked);
    proposal.summary_ = std::move(summary);
    return proposal;
}

LayoutProposal LayoutProposal::decided(LayoutCandidate chosen, std::string summary)
{
    LayoutProposal proposal;
    proposal.outcome_ = LayoutProposalOutcome::Decided;
    proposal.candidates_.push_back(std::move(chosen));
    proposal.summary_ = std::move(summary);
    return proposal;
}

Result<DelimitedLayout> LayoutProposal::layout() const
{
    if (outcome_ == LayoutProposalOutcome::Decided && !candidates_.empty()) {
        return candidates_.front().layout;
    }
    return makeError(ErrorCode::NotFound,
                     std::string("the layout of this file was not decided (") +
                         toString(outcome_) + "): " + summary_);
}

Result<LayoutProposal> proposeLayout(std::string_view bytes, bool truncated)
{
    // A caller may hand over a whole file; only its start is ever looked at, so
    // only its start is decoded - the same budget a detection reads.
    const bool cut = bytes.size() > kProbeBytes;
    Result<std::string> text =
        delimited::decodeSample(bytes.substr(0, kProbeBytes), truncated || cut);
    if (!text) {
        return text.error();
    }
    return delimited::analyse(text.value()).proposal;
}

namespace delimited {

using katana::core::DecodedText;
using katana::core::TextEncoding;

Result<std::string> decodeSample(std::string_view bytes, bool truncated)
{
    const std::size_t attempts = truncated ? 4 : 1;
    std::optional<DecodedText> chosen;
    std::optional<DecodedText> windowsReading;
    std::optional<Error> firstError;
    for (std::size_t cut = 0; cut < attempts && cut <= bytes.size(); ++cut) {
        Result<DecodedText> decoded = katana::core::decodeText(bytes.substr(0, bytes.size() - cut));
        if (!decoded) {
            // A UTF-16 sample cut at an odd byte, or inside a surrogate pair.
            if (!firstError) {
                firstError = decoded.error();
            }
            continue;
        }
        const bool fallback =
            decoded->encoding == TextEncoding::Windows1252 && decoded->guessed;
        if (fallback && cut + 1 < attempts) {
            // Perhaps a UTF-8 character cut in half at the end; a shorter cut
            // that validates as UTF-8 is the better reading of the same file.
            if (!windowsReading) {
                windowsReading = std::move(decoded).value();
            }
            continue;
        }
        chosen = std::move(decoded).value();
        break;
    }
    if (!chosen) {
        chosen = std::move(windowsReading);
    }
    if (!chosen) {
        return firstError ? *firstError
                          : makeError(ErrorCode::ParseFailure, "the sample could not be decoded");
    }
    std::string text = std::move(chosen->text);
    if (truncated) {
        const std::size_t lastBreak = text.find_last_of("\r\n");
        text.resize(lastBreak == std::string::npos ? 0 : lastBreak + 1);
    }
    return text;
}

namespace {

// ---- Header words ------------------------------------------------------------------

enum class NameKind { Role, AxisX, AxisY, Geographic, Unknown };

struct KnownName {
    std::string_view name; // already normalised: see normalisedName
    NameKind kind;
    ColumnRole role;
};

// The vocabulary is documented, with the reasons for X, Y and latitude, beside
// proposeLayout() in the public header; keep the two in step.
constexpr std::array kKnownNames = {
    KnownName{"p", NameKind::Role, ColumnRole::PointId},
    KnownName{"pt", NameKind::Role, ColumnRole::PointId},
    KnownName{"pnt", NameKind::Role, ColumnRole::PointId},
    KnownName{"point", NameKind::Role, ColumnRole::PointId},
    KnownName{"id", NameKind::Role, ColumnRole::PointId},
    KnownName{"name", NameKind::Role, ColumnRole::PointId},
    KnownName{"no", NameKind::Role, ColumnRole::PointId},
    KnownName{"num", NameKind::Role, ColumnRole::PointId},
    KnownName{"number", NameKind::Role, ColumnRole::PointId},
    KnownName{"pointid", NameKind::Role, ColumnRole::PointId},
    KnownName{"pointname", NameKind::Role, ColumnRole::PointId},
    KnownName{"pointno", NameKind::Role, ColumnRole::PointId},
    KnownName{"pointnumber", NameKind::Role, ColumnRole::PointId},
    KnownName{"ptid", NameKind::Role, ColumnRole::PointId},
    KnownName{"ptno", NameKind::Role, ColumnRole::PointId},
    KnownName{"ptnum", NameKind::Role, ColumnRole::PointId},
    KnownName{"n", NameKind::Role, ColumnRole::Northing},
    KnownName{"north", NameKind::Role, ColumnRole::Northing},
    KnownName{"northing", NameKind::Role, ColumnRole::Northing},
    KnownName{"e", NameKind::Role, ColumnRole::Easting},
    KnownName{"east", NameKind::Role, ColumnRole::Easting},
    KnownName{"easting", NameKind::Role, ColumnRole::Easting},
    KnownName{"z", NameKind::Role, ColumnRole::Elevation},
    KnownName{"elev", NameKind::Role, ColumnRole::Elevation},
    KnownName{"elevation", NameKind::Role, ColumnRole::Elevation},
    KnownName{"el", NameKind::Role, ColumnRole::Elevation},
    KnownName{"height", NameKind::Role, ColumnRole::Elevation},
    KnownName{"ht", NameKind::Role, ColumnRole::Elevation},
    KnownName{"h", NameKind::Role, ColumnRole::Elevation},
    KnownName{"rl", NameKind::Role, ColumnRole::Elevation},
    KnownName{"reducedlevel", NameKind::Role, ColumnRole::Elevation},
    KnownName{"level", NameKind::Role, ColumnRole::Elevation},
    KnownName{"c", NameKind::Role, ColumnRole::Code},
    KnownName{"code", NameKind::Role, ColumnRole::Code},
    KnownName{"featurecode", NameKind::Role, ColumnRole::Code},
    KnownName{"feature", NameKind::Role, ColumnRole::Code},
    KnownName{"fcode", NameKind::Role, ColumnRole::Code},
    KnownName{"pointcode", NameKind::Role, ColumnRole::Code},
    KnownName{"ptcode", NameKind::Role, ColumnRole::Code},
    KnownName{"d", NameKind::Role, ColumnRole::Description},
    KnownName{"desc", NameKind::Role, ColumnRole::Description},
    KnownName{"descr", NameKind::Role, ColumnRole::Description},
    KnownName{"description", NameKind::Role, ColumnRole::Description},
    KnownName{"remark", NameKind::Role, ColumnRole::Description},
    KnownName{"remarks", NameKind::Role, ColumnRole::Description},
    KnownName{"note", NameKind::Role, ColumnRole::Description},
    KnownName{"notes", NameKind::Role, ColumnRole::Description},
    KnownName{"comment", NameKind::Role, ColumnRole::Description},
    KnownName{"comments", NameKind::Role, ColumnRole::Description},
    KnownName{"x", NameKind::AxisX, ColumnRole::Ignore},
    KnownName{"y", NameKind::AxisY, ColumnRole::Ignore},
    KnownName{"lat", NameKind::Geographic, ColumnRole::Ignore},
    KnownName{"latitude", NameKind::Geographic, ColumnRole::Ignore},
    KnownName{"lon", NameKind::Geographic, ColumnRole::Ignore},
    KnownName{"long", NameKind::Geographic, ColumnRole::Ignore},
    KnownName{"longitude", NameKind::Geographic, ColumnRole::Ignore},
};

// "Point No.", "point_no", "POINT-NO" and "Pt#" are one word to a person, so
// they are one word here: ASCII letters folded, blanks and the punctuation
// people put in column names dropped, and a bracketed unit - "Easting (m)",
// "RL[ft]" - removed. The unit is not read: the caller states the file's unit.
std::string normalisedName(std::string_view raw)
{
    std::string out;
    int depth = 0;
    for (const char c : raw) {
        if (c == '(' || c == '[') {
            ++depth;
        } else if (c == ')' || c == ']') {
            depth = depth > 0 ? depth - 1 : 0;
        } else if (depth == 0 && !katana::core::isAsciiSpace(c) && c != '_' && c != '-' &&
                   c != '.' && c != '#') {
            out += katana::core::asciiLower(c);
        }
    }
    return out;
}

KnownName meaningOf(std::string_view raw)
{
    const std::string name = normalisedName(raw);
    for (const KnownName& known : kKnownNames) {
        if (known.name == name) {
            return known;
        }
    }
    return {raw, NameKind::Unknown, ColumnRole::Ignore};
}

// ---- Rows --------------------------------------------------------------------------

struct Line {
    std::string_view text;
    std::size_t number = 0; // physical, 1-based
};

bool isNumber(std::string_view value)
{
    return katana::core::parseFiniteDouble(value).has_value();
}

std::size_t numericFields(const std::vector<Field>& fields)
{
    return static_cast<std::size_t>(std::count_if(
        fields.begin(), fields.end(), [](const Field& f) { return isNumber(valueOf(f)); }));
}

// A row of data has at least a northing and an easting in it.
constexpr std::size_t kNumbersInARow = 2;

struct Split {
    Delimiter delimiter = Delimiter::Comma;
    std::vector<std::vector<Field>> fields; // one per content line
    std::optional<std::size_t> firstRow;    // index of the first row of numbers
    std::optional<std::size_t> firstMisfit; // a later line that is not one
};

Split splitWith(const std::vector<Line>& content, Delimiter delimiter)
{
    Split split;
    split.delimiter = delimiter;
    for (std::size_t i = 0; i < content.size(); ++i) {
        split.fields.push_back(splitLine(content[i].text, delimiter));
        const bool row = numericFields(split.fields.back()) >= kNumbersInARow;
        if (row && !split.firstRow) {
            split.firstRow = i;
        } else if (!row && split.firstRow && !split.firstMisfit) {
            split.firstMisfit = i;
        }
    }
    return split;
}

// True when splitting on ';' and reading ',' as a decimal point would have made
// rows of numbers: the file is very likely written with decimal commas, which
// this reader refuses (core/text.hpp) and a person should be told about rather
// than left with "no delimiter fits".
bool looksLikeDecimalCommas(const std::vector<Line>& content)
{
    std::size_t rows = 0;
    for (const Line& line : content) {
        std::size_t numbers = 0;
        for (const Field& field : splitLine(line.text, Delimiter::Semicolon)) {
            std::string value(valueOf(field));
            std::replace(value.begin(), value.end(), ',', '.');
            numbers += isNumber(value) ? 1 : 0;
        }
        rows += numbers >= kNumbersInARow ? 1 : 0;
    }
    return rows != 0;
}

struct ColumnProfile {
    bool numeric = true;  // every value present is a number
    bool integral = true; // every value present is an integer
    bool present = true;  // every row has a value
};

std::vector<ColumnProfile> profileOf(const std::vector<std::vector<Field>>& rows,
                                     std::size_t width)
{
    std::vector<ColumnProfile> profiles(width);
    for (const std::vector<Field>& row : rows) {
        for (std::size_t c = 0; c < width; ++c) {
            const std::string_view value = c < row.size() ? valueOf(row[c]) : std::string_view{};
            if (value.empty()) {
                profiles[c].present = false;
                continue;
            }
            profiles[c].numeric = profiles[c].numeric && isNumber(value);
            profiles[c].integral =
                profiles[c].integral && katana::core::parseInteger(value).has_value();
        }
    }
    return profiles;
}

std::string columnList(const std::vector<std::size_t>& columns)
{
    std::string text;
    for (std::size_t i = 0; i < columns.size(); ++i) {
        text += i == 0 ? "" : (i + 1 == columns.size() ? " and " : ", ");
        text += std::to_string(columns[i]);
    }
    return text;
}

std::optional<std::size_t> columnOf(const DelimitedLayout& layout, ColumnRole role)
{
    for (std::size_t c = 0; c < layout.columns.size(); ++c) {
        if (layout.columns[c] == role) {
            return c;
        }
    }
    return std::nullopt;
}

std::string orderPhrase(const DelimitedLayout& layout)
{
    return "northing in column " + std::to_string(*columnOf(layout, ColumnRole::Northing) + 1) +
           ", easting in column " + std::to_string(*columnOf(layout, ColumnRole::Easting) + 1);
}

// The first sampled row this layout would fail on, as a sentence; nullopt when
// every row fits. The parser's own checks, applied early, so that a header the
// rows contradict is reported as uncertain rather than decided.
std::optional<std::string> firstMisfit(const DelimitedLayout& layout,
                                       const std::vector<std::vector<Field>>& rows,
                                       const std::vector<std::size_t>& lineNumbers)
{
    for (std::size_t r = 0; r < rows.size(); ++r) {
        for (std::size_t c = 0; c < layout.columns.size(); ++c) {
            const ColumnRole role = layout.columns[c];
            const std::string_view value =
                c < rows[r].size() ? valueOf(rows[r][c]) : std::string_view{};
            const std::string at = "line " + std::to_string(lineNumbers[r]) + ", column " +
                                   std::to_string(c + 1) + " (" + toString(role) + ")";
            const bool required = role == ColumnRole::PointId ||
                                  role == ColumnRole::Northing || role == ColumnRole::Easting;
            if (required && value.empty()) {
                return at + " is empty";
            }
            const bool numeric = role == ColumnRole::Northing || role == ColumnRole::Easting ||
                                 role == ColumnRole::Elevation;
            if (numeric && !value.empty() && !isNumber(value)) {
                return at + " holds " + quotedForMessage(value) + ", which is not a number";
            }
        }
    }
    return std::nullopt;
}

// Every preset the rows are consistent with, best fit first. What decides the
// order among them is column count and whether a first column that would be
// the id holds ids; what never does is which coordinate comes first - a
// northing-first and an easting-first preset that both fit are listed in
// allColumnPresets() order, and `why` says the order is not known.
std::vector<LayoutCandidate> presetCandidates(const DelimitedLayout& base,
                                              const std::vector<ColumnProfile>& profiles,
                                              const std::string& why)
{
    struct Scored {
        LayoutCandidate candidate;
        bool exact = false;
        bool doubtfulId = false;
    };
    std::vector<Scored> scored;
    const std::size_t width = profiles.size();
    for (const ColumnPreset preset : allColumnPresets()) {
        const std::vector<ColumnRole> columns = presetColumns(preset);
        if (columns.size() > width) {
            continue;
        }
        bool fits = true;
        for (std::size_t c = 0; c < columns.size() && fits; ++c) {
            switch (columns[c]) {
            case ColumnRole::Northing:
            case ColumnRole::Easting:
                fits = profiles[c].present && profiles[c].numeric;
                break;
            case ColumnRole::Elevation:
                fits = profiles[c].numeric;
                break;
            case ColumnRole::PointId:
                fits = profiles[c].present;
                break;
            default:
                break;
            }
        }
        if (!fits) {
            continue;
        }
        Scored entry;
        entry.candidate.layout = base;
        entry.candidate.layout.columns = columns;
        std::vector<std::size_t> ignored;
        for (std::size_t c = columns.size(); c < width; ++c) {
            entry.candidate.layout.columns.push_back(ColumnRole::Ignore);
            ignored.push_back(c + 1);
        }
        entry.exact = ignored.empty();
        // Ids like "12" or "PT7" are ids; a column of values like 5000000.125
        // read as ids would shift every coordinate one column along.
        entry.doubtfulId = columns.front() == ColumnRole::PointId && profiles.front().numeric &&
                           !profiles.front().integral;
        entry.candidate.evidence = why + "; " + toString(preset) + " reads " +
                                   orderPhrase(entry.candidate.layout);
        if (!ignored.empty()) {
            entry.candidate.evidence += "; column" + std::string(ignored.size() > 1 ? "s " : " ") +
                                        columnList(ignored) + " ignored";
        }
        if (entry.doubtfulId) {
            entry.candidate.evidence += "; column 1 holds decimal numbers, which are unusual ids";
        }
        scored.push_back(std::move(entry));
    }
    std::stable_sort(scored.begin(), scored.end(), [](const Scored& a, const Scored& b) {
        if (a.exact != b.exact) {
            return a.exact;
        }
        return !a.doubtfulId && b.doubtfulId;
    });
    std::vector<LayoutCandidate> candidates;
    for (Scored& entry : scored) {
        candidates.push_back(std::move(entry.candidate));
    }
    return candidates;
}

std::string quotedHeader(std::string_view header)
{
    return quotedForMessage(katana::core::trimmed(header), 60);
}

} // namespace

Analysis analyse(std::string_view text)
{
    Analysis analysis;

    // Content lines: not blank, not a '#' comment.
    std::vector<Line> content;
    bool comments = false;
    const std::vector<std::string_view> physical = katana::core::splitLines(text);
    for (std::size_t i = 0; i < physical.size() && content.size() < kProposalSampleLines; ++i) {
        const std::string_view line = katana::core::trimmed(physical[i]);
        if (line.empty()) {
            continue;
        }
        if (line.front() == '#') {
            comments = true;
            continue;
        }
        content.push_back(Line{physical[i], i + 1});
    }
    if (content.empty()) {
        analysis.proposal = LayoutProposal::empty(
            comments ? "every line of the file is blank or a comment" : "the file holds no text");
        return analysis;
    }

    // The delimiter: the first, in the order the public header gives and
    // justifies, that makes every line from the first row of numbers onwards a
    // row of numbers.
    std::optional<Split> chosen;
    std::optional<Split> nearest; // for saying why nothing fitted
    for (const Delimiter delimiter :
         {Delimiter::Tab, Delimiter::Semicolon, Delimiter::Comma, Delimiter::Whitespace}) {
        Split split = splitWith(content, delimiter);
        if (split.firstRow && !split.firstMisfit) {
            chosen = std::move(split);
            break;
        }
        if (split.firstRow && (!nearest || *split.firstMisfit > *nearest->firstMisfit)) {
            nearest = std::move(split);
        }
    }
    if (!chosen) {
        std::string summary;
        if (nearest) {
            const Line& misfit = content[*nearest->firstMisfit];
            summary = std::string("split on ") + toString(nearest->delimiter) + ", line " +
                      std::to_string(misfit.number) + " (" + quotedHeader(misfit.text) +
                      ") is not a row of numbers although the lines before it are, and no other "
                      "delimiter fits every row";
        } else {
            summary = "no line splits into two or more numbers on a tab, semicolon, comma or "
                      "blanks";
        }
        if (looksLikeDecimalCommas(content)) {
            summary += "; the numbers look like they are written with decimal commas, which "
                       "this reader does not accept - convert them to decimal points";
        }
        analysis.proposal = LayoutProposal::uncertain({}, std::move(summary));
        return analysis;
    }
    const Split& split = *chosen;
    analysis.delimiter = split.delimiter;

    DelimitedLayout base;
    base.delimiter = split.delimiter;
    base.quoting = Quoting::DoubleQuote;
    // Every physical line above the first row, header and title lines alike.
    base.headerLines = content[*split.firstRow].number - 1;
    base.commentPrefix = comments ? "#" : "";

    std::vector<std::vector<Field>> rows(split.fields.begin() +
                                             static_cast<std::ptrdiff_t>(*split.firstRow),
                                         split.fields.end());
    std::vector<std::size_t> lineNumbers;
    std::size_t width = 0;
    for (std::size_t i = *split.firstRow; i < content.size(); ++i) {
        lineNumbers.push_back(content[i].number);
        width = std::max(width, split.fields[i].size());
    }
    const std::vector<ColumnProfile> profiles = profileOf(rows, width);
    const std::string delimiterWord = std::string(toString(split.delimiter)) + "-delimited";
    const std::string commentNote = comments ? "; lines starting '#' are comments" : "";

    std::string why = "no header line, so the file does not say whether northing or easting "
                      "comes first";
    if (*split.firstRow > 0) {
        const Line& headerLine = content[*split.firstRow - 1];
        const std::vector<Field> names = splitLine(headerLine.text, split.delimiter);

        std::vector<KnownName> meanings;
        std::size_t northings = 0;
        std::size_t eastings = 0;
        std::size_t xs = 0;
        std::size_t ys = 0;
        std::optional<std::size_t> geographic;
        for (std::size_t c = 0; c < names.size(); ++c) {
            meanings.push_back(meaningOf(valueOf(names[c])));
            const KnownName& meaning = meanings.back();
            northings += meaning.kind == NameKind::Role && meaning.role == ColumnRole::Northing;
            eastings += meaning.kind == NameKind::Role && meaning.role == ColumnRole::Easting;
            xs += meaning.kind == NameKind::AxisX;
            ys += meaning.kind == NameKind::AxisY;
            if (meaning.kind == NameKind::Geographic && !geographic) {
                geographic = c;
            }
        }

        if (geographic) {
            analysis.proposal = LayoutProposal::uncertain(
                {}, "column " + std::to_string(*geographic + 1) + " is headed " +
                        quotedForMessage(valueOf(names[*geographic])) +
                        ": this reader takes grid northings and eastings, and latitude and "
                        "longitude read as metres would place the survey wrongly without any "
                        "error; convert the file to grid coordinates first");
            return analysis;
        }

        // The header's roles, for the columns it names; X, Y, repeats and words
        // it does not know are Ignore here and filled in or reported below.
        const auto layoutFromHeader = [&](std::optional<ColumnRole> xRole,
                                          std::optional<ColumnRole> yRole,
                                          std::vector<std::string>& notes) {
            DelimitedLayout layout = base;
            std::vector<ColumnRole> used;
            std::vector<std::string> unknown; // "2 ('Date')"
            for (std::size_t c = 0; c < std::max(width, names.size()); ++c) {
                ColumnRole role = ColumnRole::Ignore;
                if (c < names.size()) {
                    const KnownName& meaning = meanings[c];
                    if (meaning.kind == NameKind::Role) {
                        role = meaning.role;
                    } else if (meaning.kind == NameKind::AxisX && xRole) {
                        role = *xRole;
                    } else if (meaning.kind == NameKind::AxisY && yRole) {
                        role = *yRole;
                    } else if (meaning.kind == NameKind::Unknown && !valueOf(names[c]).empty()) {
                        unknown.push_back(std::to_string(c + 1) + " (" +
                                          quotedForMessage(valueOf(names[c])) + ")");
                    }
                    if (role != ColumnRole::Ignore &&
                        std::find(used.begin(), used.end(), role) != used.end()) {
                        notes.push_back("column " + std::to_string(c + 1) + " is a second " +
                                        toString(role) + " column and is ignored");
                        role = ColumnRole::Ignore;
                    }
                    used.push_back(role);
                }
                layout.columns.push_back(role);
            }
            if (!unknown.empty()) {
                // Quoted, so a person can see a misspelt "Descripton" for what it
                // is rather than wonder where the descriptions went.
                std::string list;
                for (std::size_t i = 0; i < unknown.size(); ++i) {
                    list += i == 0 ? "" : (i + 1 == unknown.size() ? " and " : ", ");
                    list += unknown[i];
                }
                notes.push_back(unknown.size() > 1
                                    ? "columns " + list +
                                          " are not header words this reader knows, so they are "
                                          "ignored"
                                    : "column " + list +
                                          " is not a header word this reader knows, so it is "
                                          "ignored");
            }
            if (width > names.size()) {
                notes.push_back("rows have " + std::to_string(width) +
                                " fields and the header names " + std::to_string(names.size()) +
                                ", so the rest are ignored");
            }
            return layout;
        };
        const auto joined = [](const std::vector<std::string>& notes) {
            std::string all;
            for (const std::string& note : notes) {
                all += "; " + note;
            }
            return all;
        };

        if (northings == 1 && eastings == 1) {
            analysis.headerNamesCoordinates = true;
            std::vector<std::string> notes;
            const DelimitedLayout layout = layoutFromHeader(std::nullopt, std::nullopt, notes);
            if (xs + ys != 0) {
                notes.push_back("an X or Y column beside named northing and easting columns is "
                                "ignored");
            }
            const std::string header = "the header on line " +
                                       std::to_string(headerLine.number) + " names " +
                                       orderPhrase(layout);
            LayoutCandidate candidate{layout, header + " (" + delimiterWord + ", " +
                                                  layoutTemplate(layout) + ")" + joined(notes) +
                                                  commentNote};
            // Named repeats are a question, not a detail: which "Code" was meant?
            const bool repeated = std::any_of(notes.begin(), notes.end(), [](const std::string& n) {
                return n.find("is a second") != std::string::npos;
            });
            if (const std::optional<std::string> misfit = firstMisfit(layout, rows, lineNumbers)) {
                analysis.proposal = LayoutProposal::uncertain(
                    {std::move(candidate)},
                    header + ", but " + *misfit + "; check the header against the rows");
                return analysis;
            }
            if (repeated) {
                analysis.proposal = LayoutProposal::uncertain(
                    {std::move(candidate)}, header + ", but it names a column twice" +
                                                joined(notes));
                return analysis;
            }
            analysis.proposal =
                LayoutProposal::decided(std::move(candidate), header + joined(notes));
            return analysis;
        }

        if (xs == 1 && ys == 1 && northings == 0 && eastings == 0) {
            analysis.headerNamesCoordinates = true;
            std::vector<LayoutCandidate> candidates;
            std::vector<std::string> notes;
            const DelimitedLayout gisOrder =
                layoutFromHeader(ColumnRole::Easting, ColumnRole::Northing, notes);
            const std::string rest = joined(notes) + commentNote;
            notes.clear();
            const DelimitedLayout geodeticOrder =
                layoutFromHeader(ColumnRole::Northing, ColumnRole::Easting, notes);
            // The rows cannot tell the two readings apart - that is the point -
            // but they can contradict the header's other columns.
            const std::optional<std::string> misfit = firstMisfit(gisOrder, rows, lineNumbers);
            const std::string check = misfit ? "; but " + *misfit : std::string{};
            candidates.push_back(
                {gisOrder, "X read as easting and Y as northing, the CAD and GIS convention and "
                           "Katana's own (" +
                               orderPhrase(gisOrder) + ")" + rest + check});
            candidates.push_back(
                {geodeticOrder, "X read as northing and Y as easting, the geodetic convention of "
                                "Gauss-Kruger grids (" +
                                    orderPhrase(geodeticOrder) + ")" + rest + check});
            analysis.proposal = LayoutProposal::uncertain(
                std::move(candidates),
                "the header on line " + std::to_string(headerLine.number) +
                    " names X and Y, which are easting and northing in CAD and GIS but northing "
                    "and easting in Gauss-Kruger grids; say which this file uses");
            return analysis;
        }

        why = "the header on line " + std::to_string(headerLine.number) + " (" +
              quotedHeader(headerLine.text) +
              ") does not name one northing and one easting column, so the file does not say "
              "which comes first";
    }

    std::vector<LayoutCandidate> candidates = presetCandidates(base, profiles, why);
    for (LayoutCandidate& candidate : candidates) {
        candidate.evidence += commentNote;
    }
    std::string summary = why;
    summary += candidates.empty()
                   ? "; no preset layout fits these " + delimiterWord +
                         " rows, so the columns have to be set by hand"
                   : "; choose a layout";
    analysis.proposal = LayoutProposal::uncertain(std::move(candidates), std::move(summary));
    return analysis;
}

} // namespace delimited

} // namespace katana::surveyio
