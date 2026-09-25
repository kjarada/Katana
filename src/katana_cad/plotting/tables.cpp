#include "katana/cad/plotting/tables.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <format>
#include <utility>

#include "katana/cad/plotting/layout.hpp"

namespace katana::cad::plotting {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

// ---- measuring ------------------------------------------------------------------

// Advance widths of ASCII 32 to 126 in thousandths of an em: Helvetica's
// published metrics, which Arial was drawn to match character for character.
constexpr std::array<std::uint16_t, 95> kRegularWidths{
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, // space to /
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556,                               // 0 to 9
    278, 278, 584, 584, 584, 556, 1015,                                             // : to @
    667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833,                // A to M
    722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611,                // N to Z
    278, 278, 278, 469, 556, 333,                                                   // [ to `
    556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833,                // a to m
    556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500,                // n to z
    334, 260, 334, 584,                                                             // { to ~
};
constexpr std::array<std::uint16_t, 95> kBoldWidths{
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556,
    333, 333, 584, 584, 584, 611, 975,
    722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833,
    722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611,
    333, 278, 333, 584, 556, 333,
    556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889,
    611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500,
    389, 280, 389, 584,
};
// Arial's cap height in ems (1467 of 2048 units), what a text's size is
// given by here.
constexpr double kCapHeightEm = 1467.0 / 2048.0;
constexpr std::uint16_t kOtherWidth = 556; // a digit's
constexpr std::uint16_t kWideWidth = 1000; // a full em

// Whether a code point is drawn a full em wide: the East Asian wide blocks.
bool isWide(std::uint32_t c)
{
    return (c >= 0x1100 && c <= 0x115F) || (c >= 0x2E80 && c <= 0xA4CF) ||
           (c >= 0xAC00 && c <= 0xD7A3) || (c >= 0xF900 && c <= 0xFAFF) ||
           (c >= 0xFE30 && c <= 0xFE4F) || (c >= 0xFF00 && c <= 0xFF60) ||
           (c >= 0xFFE0 && c <= 0xFFE6) || c >= 0x20000;
}

// ---- laying out -----------------------------------------------------------------

// `text` on one line: each line break a space.
std::string oneLine(std::string_view text)
{
    std::string out(text);
    std::replace_if(out.begin(), out.end(), [](char c) { return c == '\n' || c == '\r'; }, ' ');
    return out;
}

// `text` without the spaces, tabs and line breaks at either end: a
// description typed with a trailing line break would otherwise make its row
// a line taller, and a centred code with a trailing space would sit off
// centre.
std::string_view trimmed(std::string_view text)
{
    constexpr std::string_view kBlank = " \t\r\n";
    const std::size_t first = text.find_first_not_of(kBlank);
    if (first == std::string_view::npos) {
        return {};
    }
    return text.substr(first, text.find_last_not_of(kBlank) - first + 1);
}

// A word of a wrapping cell and its width at a cap height of 1 mm.
struct Word {
    std::string_view text;
    double width = 0.0;
};
using Paragraph = std::vector<Word>;

// A cell measured once at a cap height of 1 mm, condensed: at a cap height of
// c every width is c times these (TextWidth's promise).
struct Cell {
    double width = 0.0;                // the widest line
    double widestWord = 0.0;           // a wrapping column's only
    std::vector<Paragraph> paragraphs; // a wrapping column's only
};

struct Measured {
    double heading = 0.0;
    std::vector<double> headers;
    std::vector<std::vector<Cell>> cells; // [row][column]
    double space = 0.0;
};

// The lines a wrapping cell breaks into, `room` wide at a cap height of 1 mm,
// with their widths. Words are never split: one wider than the room has a
// line of its own and is squeezed. An empty paragraph is an empty line.
std::vector<std::pair<std::string, double>> wrapLines(const Cell& cell, double room, double space)
{
    std::vector<std::pair<std::string, double>> lines;
    for (const Paragraph& paragraph : cell.paragraphs) {
        std::string line;
        double width = 0.0;
        bool started = false;
        for (const Word& word : paragraph) {
            if (started && width + space + word.width > room) {
                lines.emplace_back(std::move(line), width);
                line.clear();
                width = 0.0;
                started = false;
            }
            if (started) {
                line += ' ';
                width += space;
            }
            line += word.text;
            width += word.width;
            started = true;
        }
        lines.emplace_back(std::move(line), width);
    }
    return lines;
}

// One way of fitting the table: a text size, a number of blocks, the column
// widths that gives, and each row's number of lines.
struct Candidate {
    double cap = 0.0;
    std::size_t blocks = 1;
    double blockWidth = 0.0;
    std::vector<double> widths;
    bool widthFits = false;
    // No line is squeezed below TableStyle::leastSqueeze to fit its cell.
    bool comfortable = false;
    std::vector<double> heights; // each row's
    double headingHeight = 0.0;
    double headerHeight = 0.0;
    double available = 0.0; // for rows in each block, under the heading and header
};

// Where the placed rows go: each row's block and its distance below the
// block's header.
struct Placement {
    std::size_t block = 0;
    double offset = 0.0;
};

Candidate evaluate(const TableSpec& spec, const Measured& measured, const Box2& rect, double cap,
                   std::size_t blocks, const TableStyle& style)
{
    Candidate c;
    c.cap = cap;
    c.blocks = blocks;
    c.blockWidth = rect.width() / static_cast<double>(blocks);
    const double pad = style.paddingRatio * cap;
    const std::size_t columns = spec.columns.size();

    // Every column but the stretching one as wide as its widest text; the
    // stretching one what is left, at least as wide as its header.
    std::optional<std::size_t> stretch;
    std::vector<double> natural(columns, 0.0);
    for (std::size_t j = 0; j < columns; ++j) {
        double widest = measured.headers[j];
        if (spec.columns[j].stretch && !stretch) {
            stretch = j;
        } else {
            for (const auto& row : measured.cells) {
                widest = std::max(widest, row[j].width);
            }
        }
        natural[j] = cap * widest + 2.0 * pad;
    }
    double fixed = 0.0;
    for (std::size_t j = 0; j < columns; ++j) {
        if (j != stretch) {
            fixed += natural[j];
        }
    }
    constexpr double kSlack = 1e-9;
    c.widths = natural;
    if (stretch) {
        c.widths[*stretch] = c.blockWidth - fixed;
        c.widthFits = c.widths[*stretch] + kSlack >= natural[*stretch];
    } else {
        // No column stretches: the room left over is shared in proportion.
        c.widthFits = fixed <= c.blockWidth + kSlack;
        for (double& width : c.widths) {
            width *= fixed > 0.0 ? c.blockWidth / fixed : 0.0;
        }
    }
    if (!c.widthFits) {
        // Too narrow: every column in proportion to what it needs, so the
        // table still stays inside its block and a last resort still draws.
        const double needed = fixed + (stretch ? natural[*stretch] : 0.0);
        for (std::size_t j = 0; j < columns; ++j) {
            c.widths[j] = needed > 0.0 ? natural[j] * c.blockWidth / needed : 0.0;
        }
    }

    // Row heights: one line, or as many as the longest wrapped cell. And
    // whether any line - a whole cell, or a wrapped cell's longest word -
    // would be squeezed hard to fit.
    c.comfortable = c.widthFits;
    c.heights.reserve(measured.cells.size());
    for (const auto& row : measured.cells) {
        std::size_t lines = 1;
        for (std::size_t j = 0; j < columns; ++j) {
            const double room = std::max(c.widths[j] - 2.0 * pad, 0.0) / cap;
            const double longest = spec.columns[j].wrap ? row[j].widestWord : row[j].width;
            if (longest * style.leastSqueeze > room) {
                c.comfortable = false;
            }
            if (spec.columns[j].wrap) {
                lines = std::max(lines, wrapLines(row[j], room, measured.space).size());
            }
        }
        c.heights.push_back(cap * (style.rowPitch +
                                   style.linePitch * static_cast<double>(lines - 1)));
    }
    c.headingHeight = spec.heading.empty() ? 0.0 : style.headingRatio * cap * style.rowPitch;
    c.headerHeight = cap * style.rowPitch;
    c.available = rect.height() - c.headingHeight - c.headerHeight;
    return c;
}

// The rows of `heights`, in order, into `blocks` blocks each `available`
// high: a row that does not fit what is left of a block starts the next. It
// stops at the first row that fits nowhere.
std::vector<Placement> place(const std::vector<double>& heights, std::size_t blocks,
                             double available)
{
    constexpr double kSlack = 1e-9;
    std::vector<Placement> placed;
    std::size_t block = 0;
    double used = 0.0;
    for (const double height : heights) {
        if (used + height > available + kSlack) {
            ++block;
            used = 0.0;
        }
        if (block >= blocks || height > available + kSlack) {
            break;
        }
        placed.push_back({block, used});
        used += height;
    }
    return placed;
}

// The cap heights tried, largest first: every step from the maximum down,
// and the minimum itself.
std::vector<double> capsToTry(const TableStyle& style)
{
    std::vector<double> caps;
    const double step = style.stepMm > 0.0 ? style.stepMm : style.maxCapMm - style.minCapMm;
    for (int k = 0; step > 0.0 && k < 1000; ++k) {
        const double cap = style.maxCapMm - static_cast<double>(k) * step;
        if (cap <= style.minCapMm + 1e-9) {
            break;
        }
        caps.push_back(cap);
    }
    caps.push_back(style.minCapMm);
    return caps;
}

// A line of text in a cell `width` wide starting at `left`, squeezed when
// it is wider than the cell less its padding.
TableText cellText(std::string text, double widthAtOne, double left, double width, double baseline,
                   double cap, double pad, HorizontalJustify justify, bool bold,
                   const TableStyle& style)
{
    TableText out;
    out.text = std::move(text);
    out.capMm = cap;
    out.xFactor = style.xFactor;
    out.bold = bold;
    out.horizontal = justify;
    const double room = width - 2.0 * pad;
    const double natural = widthAtOne * cap;
    if (natural > room && natural > 0.0) {
        out.squeeze = std::max(room, 0.0) / natural;
    }
    double x = left + pad;
    if (justify == HorizontalJustify::Centre) {
        x = left + width / 2.0;
    } else if (justify == HorizontalJustify::Right) {
        x = left + width - pad;
    }
    out.anchor = Point2(x, baseline);
    return out;
}

// The number a sheet prints: typed on it, else the set's for its position.
std::string numberOf(const SheetSet& set, std::size_t index)
{
    const Sheet& sheet = set.sheets[index];
    if (const auto typed = sheet.fields.find("sheet_number"); typed != sheet.fields.end()) {
        return typed->second;
    }
    return formatSheetNumber(set.numbering, index + 1, set.sheets.size(), set.defaults.setNumber);
}

} // namespace

double estimateTextWidth(std::string_view text, double capMm, bool bold)
{
    const auto& table = bold ? kBoldWidths : kRegularWidths;
    double units = 0.0;
    for (std::size_t i = 0; i < text.size();) {
        const auto lead = static_cast<unsigned char>(text[i]);
        if (lead < 0x80) {
            if (lead >= 32 && lead <= 126) {
                units += table[lead - 32];
            } else if (lead == '\t') {
                units += table[0];
            }
            ++i;
            continue;
        }
        // A UTF-8 sequence: its length from the lead byte, its code point
        // from the bytes after it. A stray continuation byte counts as one.
        std::size_t length = 1;
        std::uint32_t code = 0;
        if ((lead & 0xE0) == 0xC0) {
            length = 2;
            code = lead & 0x1Fu;
        } else if ((lead & 0xF0) == 0xE0) {
            length = 3;
            code = lead & 0x0Fu;
        } else if ((lead & 0xF8) == 0xF0) {
            length = 4;
            code = lead & 0x07u;
        }
        for (std::size_t k = 1; k < length && i + k < text.size(); ++k) {
            code = (code << 6) | (static_cast<unsigned char>(text[i + k]) & 0x3Fu);
        }
        units += length > 1 && isWide(code) ? kWideWidth : kOtherWidth;
        i += length;
    }
    return units / 1000.0 * capMm / kCapHeightEm;
}

std::vector<RegisterRow> drawingRegister(const SheetSet& set)
{
    std::vector<RegisterRow> rows;
    rows.reserve(set.sheets.size());
    // The five fields read here owe nothing to the project, so no context.
    const FieldContext none;
    for (std::size_t i = 0; i < set.sheets.size(); ++i) {
        const auto fields = resolveFields(set, i, none);
        const auto field = [&fields](std::string_view name) {
            const auto found = fields.find(name);
            return found == fields.end() ? std::string{} : found->second;
        };
        rows.push_back({set.sheets[i].id, field("sheet_number"), field("sheet_name"),
                        field("scale"), field("paper"), field("revision")});
    }
    return rows;
}

std::vector<Revision> revisionRows(const SheetSet& set, std::size_t limit)
{
    std::vector<Revision> rows(set.revisions.rbegin(), set.revisions.rend());
    if (limit > 0 && rows.size() > limit) {
        rows.resize(limit);
    }
    return rows;
}

TableLayout layoutTable(const TableSpec& spec, const Box2& rect, const TextWidth& measure,
                        const TableStyle& style)
{
    TableLayout out;
    const std::size_t rowCount = spec.rows.size();
    const std::size_t columns = spec.columns.size();
    if (columns == 0 || rect.empty() || !(rect.width() > 0.0) || !(rect.height() > 0.0) ||
        !(style.minCapMm > 0.0) || !(style.maxCapMm >= style.minCapMm)) {
        out.rowsHidden = rowCount;
        return out;
    }
    const TextWidth width = measure ? measure : TextWidth(estimateTextWidth);
    const auto atOne = [&](std::string_view text, bool bold) {
        return text.empty() ? 0.0 : width(text, 1.0, bold) * style.xFactor;
    };

    // What each cell prints: a line break in a cell that does not wrap, or
    // in the heading, is a space - the row is one line high, and a painter
    // would step a second line down over the row below. Blanks at either
    // end of a text are dropped (trimmed).
    const std::string heading = oneLine(trimmed(spec.heading));
    std::vector<std::vector<std::string>> texts(rowCount, std::vector<std::string>(columns));
    for (std::size_t i = 0; i < rowCount; ++i) {
        for (std::size_t j = 0; j < columns && j < spec.rows[i].size(); ++j) {
            const std::string_view cell = trimmed(spec.rows[i][j]);
            texts[i][j] = spec.columns[j].wrap ? std::string(cell) : oneLine(cell);
        }
    }

    // Everything measured once, at a cap height of 1 mm.
    Measured measured;
    measured.heading = atOne(heading, true);
    measured.space = atOne(" ", false);
    for (const TableColumn& column : spec.columns) {
        measured.headers.push_back(atOne(column.header, true));
    }
    measured.cells.resize(rowCount);
    for (std::size_t i = 0; i < rowCount; ++i) {
        measured.cells[i].resize(columns);
        for (std::size_t j = 0; j < columns; ++j) {
            const std::string_view text = texts[i][j];
            Cell& cell = measured.cells[i][j];
            if (!spec.columns[j].wrap) {
                cell.width = atOne(text, false);
                continue;
            }
            // Paragraphs on '\n', words on spaces (a tab, or the '\r' of a
            // "\r\n" typed on another system, is a space too).
            std::size_t start = 0;
            while (true) {
                const std::size_t end = std::min(text.find('\n', start), text.size());
                Paragraph paragraph;
                const std::string_view line = text.substr(start, end - start);
                for (std::size_t at = 0; at < line.size();) {
                    const std::size_t space = std::min(line.find_first_of(" \t\r", at), line.size());
                    if (space > at) {
                        const std::string_view word = line.substr(at, space - at);
                        paragraph.push_back({word, atOne(word, false)});
                    }
                    at = space + 1;
                }
                cell.paragraphs.push_back(std::move(paragraph));
                if (end >= text.size()) {
                    break;
                }
                start = end + 1;
            }
            for (const Paragraph& paragraph : cell.paragraphs) {
                double lineWidth = 0.0;
                for (const Word& word : paragraph) {
                    lineWidth += (lineWidth > 0.0 ? measured.space : 0.0) + word.width;
                    cell.widestWord = std::max(cell.widestWord, word.width);
                }
                cell.width = std::max(cell.width, lineWidth);
            }
        }
    }

    // The largest text at which every row fits one block, else two (up to
    // maxBlocks) - first among the layouts that squeeze no line hard, then
    // among any - else the smallest text with as many rows as fit.
    const std::size_t maxBlocks = std::max<std::size_t>(spec.maxBlocks, 1);
    const std::vector<double> caps = capsToTry(style);
    std::optional<Candidate> chosen;
    std::vector<Placement> placement;
    for (const bool comfort : {true, false}) {
        for (std::size_t blocks = 1; blocks <= maxBlocks && !chosen; ++blocks) {
            for (const double cap : caps) {
                Candidate candidate = evaluate(spec, measured, rect, cap, blocks, style);
                if (!candidate.widthFits || (comfort && !candidate.comfortable) ||
                    candidate.available < 0.0) {
                    continue;
                }
                std::vector<Placement> rows =
                    place(candidate.heights, blocks, candidate.available);
                if (rows.size() == rowCount) {
                    chosen = std::move(candidate);
                    placement = std::move(rows);
                    break;
                }
            }
        }
    }
    std::size_t hidden = 0;
    std::optional<Placement> overflow;
    if (!chosen) {
        // Nothing fits: the smallest text, in as many blocks as are wide
        // enough for it, and a last line counting the rows left out.
        std::size_t blocks = 1;
        for (std::size_t b = maxBlocks; b > 1; --b) {
            if (evaluate(spec, measured, rect, style.minCapMm, b, style).widthFits) {
                blocks = b;
                break;
            }
        }
        chosen = evaluate(spec, measured, rect, style.minCapMm, blocks, style);
        if (chosen->available < 0.0) {
            // Not even the heading and the header fit: no table, every row
            // counted as hidden.
            out.rowsHidden = rowCount;
            out.capMm = style.minCapMm;
            out.rowHeightMm = style.minCapMm * style.rowPitch;
            return out;
        }
        placement = place(chosen->heights, blocks, chosen->available);
        if (placement.size() < rowCount) {
            // The count's line goes after the last row shown; rows give way
            // to it until it fits.
            const double line = chosen->cap * style.rowPitch;
            for (std::size_t shown = placement.size();; --shown) {
                std::vector<double> heights(chosen->heights.begin(),
                                            chosen->heights.begin() +
                                                static_cast<std::ptrdiff_t>(shown));
                heights.push_back(line);
                std::vector<Placement> trial = place(heights, blocks, chosen->available);
                if (trial.size() == heights.size()) {
                    overflow = trial.back();
                    trial.pop_back();
                    placement = std::move(trial);
                    break;
                }
                if (shown == 0) {
                    placement.clear();
                    break;
                }
            }
        }
        hidden = rowCount - placement.size();
    }

    const Candidate& c = *chosen;
    const double cap = c.cap;
    const double pad = style.paddingRatio * cap;
    const double top = rect.max.y;
    const double headerTop = top - c.headingHeight;
    const double bodyTop = headerTop - c.headerHeight;
    out.capMm = cap;
    out.rowHeightMm = cap * style.rowPitch;
    out.blocks = c.blocks;
    out.rowsShown = placement.size();
    out.rowsHidden = hidden;
    const auto rule = [&out](Point2 a, Point2 b, double weight) {
        out.rules.push_back({a, b, weight});
    };
    // The first line's baseline in a row whose top is `rowTop`: the cap
    // height centred in a one-line row.
    const auto baseline = [&](double rowTop, double capMm, double rowHeight) {
        return rowTop - (rowHeight + capMm) / 2.0;
    };

    // The box.
    rule(rect.min, Point2(rect.max.x, rect.min.y), style.outerWeightMm);
    rule(Point2(rect.max.x, rect.min.y), rect.max, style.outerWeightMm);
    rule(rect.max, Point2(rect.min.x, rect.max.y), style.outerWeightMm);
    rule(Point2(rect.min.x, rect.max.y), rect.min, style.outerWeightMm);

    // The heading, across the top.
    if (!heading.empty()) {
        const double headingCap = style.headingRatio * cap;
        out.texts.push_back(cellText(heading, measured.heading, rect.min.x, rect.width(),
                                     baseline(top, headingCap, c.headingHeight), headingCap,
                                     style.paddingRatio * headingCap, HorizontalJustify::Left,
                                     true, style));
        rule(Point2(rect.min.x, headerTop), Point2(rect.max.x, headerTop), style.outerWeightMm);
    }

    // Each block: its header, its rules down to the foot of the box - or, in
    // the block that ends with the count of the rows left out, down to that
    // line, which is one cell across the block.
    const auto blockLeft = [&](std::size_t block) {
        return rect.min.x + static_cast<double>(block) * c.blockWidth;
    };
    for (std::size_t b = 0; b < c.blocks; ++b) {
        const double left = blockLeft(b);
        if (b > 0) {
            rule(Point2(left, headerTop), Point2(left, rect.min.y), style.outerWeightMm);
        }
        const double foot =
            overflow && overflow->block == b ? bodyTop - overflow->offset : rect.min.y;
        double x = left;
        for (std::size_t j = 0; j < columns; ++j) {
            if (j > 0) {
                rule(Point2(x, headerTop), Point2(x, foot), style.innerWeightMm);
            }
            if (!spec.columns[j].header.empty()) {
                out.texts.push_back(cellText(spec.columns[j].header, measured.headers[j], x,
                                             c.widths[j], baseline(headerTop, cap, c.headerHeight),
                                             cap, pad, spec.columns[j].justify, true, style));
            }
            x += c.widths[j];
        }
        rule(Point2(left, bodyTop), Point2(left + c.blockWidth, bodyTop), style.outerWeightMm);
    }

    // The rows.
    for (std::size_t i = 0; i < placement.size(); ++i) {
        const double left = blockLeft(placement[i].block);
        const double rowTop = bodyTop - placement[i].offset;
        const double rowBottom = rowTop - c.heights[i];
        const Box2 box(Point2(left, rowBottom), Point2(left + c.blockWidth, rowTop));
        out.rowBoxes.push_back(box);
        if (spec.highlight == i) {
            out.shaded.push_back(box);
        }
        const double first = baseline(rowTop, cap, out.rowHeightMm);
        double x = left;
        for (std::size_t j = 0; j < columns; ++j) {
            const TableColumn& column = spec.columns[j];
            const Cell& cell = measured.cells[i][j];
            if (column.wrap) {
                const double room = std::max(c.widths[j] - 2.0 * pad, 0.0) / cap;
                double y = first;
                for (auto& [line, lineWidth] : wrapLines(cell, room, measured.space)) {
                    if (!line.empty()) {
                        out.texts.push_back(cellText(std::move(line), lineWidth, x, c.widths[j], y,
                                                     cap, pad, column.justify, false, style));
                    }
                    y -= style.linePitch * cap;
                }
            } else if (!texts[i][j].empty()) {
                out.texts.push_back(cellText(texts[i][j], cell.width, x, c.widths[j], first, cap,
                                             pad, column.justify, false, style));
            }
            x += c.widths[j];
        }
        rule(Point2(left, rowBottom), Point2(left + c.blockWidth, rowBottom), style.innerWeightMm);
    }

    // What did not fit, counted where the next row would have gone.
    if (overflow && hidden > 0) {
        const double left = blockLeft(overflow->block);
        const double rowTop = bodyTop - overflow->offset;
        std::string text = std::format("+{} {}", hidden, spec.overflowWord);
        const double widthAtOne = atOne(text, false);
        TableText count = cellText(std::move(text), widthAtOne, left, c.blockWidth,
                                   baseline(rowTop, cap, out.rowHeightMm), cap, pad,
                                   HorizontalJustify::Right, false, style);
        count.muted = true;
        out.texts.push_back(std::move(count));
    }
    return out;
}

TableSpec tableSpecFor(const SheetSet& set, std::size_t sheetIndex, const Viewport& viewport)
{
    TableSpec spec;
    const std::string heading = viewport.title.empty() ? automaticTitle(viewport) : viewport.title;
    if (viewport.kind == ViewportKind::SheetIndex) {
        spec.heading = heading;
        spec.columns = {{"SHEET No.", HorizontalJustify::Centre},
                        {"TITLE", HorizontalJustify::Left, true},
                        {"SCALE", HorizontalJustify::Centre},
                        {"PAPER", HorizontalJustify::Centre},
                        {"REV", HorizontalJustify::Centre}};
        for (RegisterRow& row : drawingRegister(set)) {
            spec.rows.push_back({std::move(row.number), std::move(row.title), std::move(row.scale),
                                 std::move(row.paper), std::move(row.revision)});
        }
        if (sheetIndex < set.sheets.size()) {
            spec.highlight = sheetIndex;
        }
        spec.maxBlocks = 2;
        spec.overflowWord = "more";
    } else if (viewport.kind == ViewportKind::Revisions) {
        spec.heading = heading;
        spec.columns = {{"REV", HorizontalJustify::Centre},
                        {"DATE", HorizontalJustify::Centre},
                        {"DESCRIPTION", HorizontalJustify::Left, true, true},
                        {"BY", HorizontalJustify::Centre}};
        for (Revision& revision : revisionRows(set, viewport.revisionLimit)) {
            spec.rows.push_back({std::move(revision.code), std::move(revision.date),
                                 std::move(revision.description), std::move(revision.by)});
        }
        // A revision table reads down from the newest; a second block would
        // put older revisions level with newer ones.
        spec.maxBlocks = 1;
        spec.overflowWord = "earlier";
    }
    return spec;
}

TableLayout layoutViewportTable(const SheetSet& set, std::size_t sheetIndex,
                                const Viewport& viewport, const TextWidth& measure)
{
    return layoutTable(tableSpecFor(set, sheetIndex, viewport), viewport.rect, measure);
}

Result<Sheet> registerSheet(const SheetSet& set, const SheetTemplate& paper)
{
    for (std::size_t i = 0; i < set.sheets.size(); ++i) {
        for (const Viewport& viewport : set.sheets[i].viewports) {
            if (viewport.kind == ViewportKind::SheetIndex) {
                return makeError(ErrorCode::AlreadyExists, "the set already has a drawing register",
                                 std::format("sheet {} ({})", numberOf(set, i),
                                             set.sheets[i].name));
            }
        }
    }
    Sheet sheet = blankSheet(paper, std::string(kRegisterSheetName));
    sheet.id = "s1";
    // The frame's utility legend explains symbols a cover does not draw.
    sheet.frameLegend = false;
    Viewport index;
    index.id = "vp1";
    index.kind = ViewportKind::SheetIndex;
    Viewport revisions;
    revisions.id = "vp2";
    revisions.kind = ViewportKind::Revisions;
    sheet.viewports = {std::move(index), std::move(revisions)};
    // The register ranks first, so it takes the big cell (tilingRank).
    tileViewports(sheet, TilingPreset::MainRight);
    return sheet;
}

Result<std::string> addRegisterSheet(Document& document, const SheetTemplate& paper)
{
    SheetSet set = document.sheetSet();
    auto sheet = registerSheet(set, paper);
    if (!sheet) {
        return sheet.error();
    }
    std::vector<Sheet> one{std::move(*sheet)};
    prepareForAppend(set, one);
    std::string id = one.front().id;
    set.sheets.insert(set.sheets.begin(), std::move(one.front()));
    if (auto status = document.setSheetSet(set, "ADD_REGISTER_SHEET"); !status) {
        return status.error();
    }
    return id;
}

} // namespace katana::cad::plotting
