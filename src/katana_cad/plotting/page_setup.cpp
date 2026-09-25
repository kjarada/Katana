#include "katana/cad/plotting/page_setup.hpp"

#include <algorithm>
#include <array>
#include <charconv>
#include <cmath>
#include <format>
#include <optional>
#include <set>

#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/cad/plotting/sheet_set.hpp"

namespace katana::cad::plotting {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

// The longest file name made, in bytes: well inside every file system's 255,
// with room for a " (12)" and an extension, and short enough that a folder a
// few levels down stays under Windows' 260-character path limit.
constexpr std::size_t kMaximumFileNameBytes = 120;

constexpr std::array<std::string_view, 6> kPatternTokens{"n", "N", "set", "name", "number", "id"};

std::string lowerAscii(std::string_view text)
{
    std::string out(text);
    for (char& c : out) {
        if (c >= 'A' && c <= 'Z') {
            c = static_cast<char>(c - 'A' + 'a');
        }
    }
    return out;
}

bool allDigits(std::string_view text)
{
    return !text.empty() &&
           std::all_of(text.begin(), text.end(), [](char c) { return c >= '0' && c <= '9'; });
}

// A position from 1 as written, or nothing when it does not fit a size_t.
std::optional<std::size_t> positionOf(std::string_view digits)
{
    std::size_t value = 0;
    const auto [end, error] = std::from_chars(digits.data(), digits.data() + digits.size(), value);
    if (error != std::errc{} || end != digits.data() + digits.size()) {
        return std::nullopt;
    }
    return value;
}

// Each {token} of `pattern`, and what is between them, in order: `literal`
// for the text, `token` for the name and the format after its colon. An
// unclosed brace is literal text. The one tokeniser both the validation and
// the expansion use, so they cannot disagree about a pattern.
template <typename Literal, typename Token>
void scanPattern(std::string_view pattern, Literal literal, Token token)
{
    std::size_t at = 0;
    while (at < pattern.size()) {
        const auto open = pattern.find('{', at);
        if (open == std::string_view::npos) {
            literal(pattern.substr(at));
            return;
        }
        literal(pattern.substr(at, open - at));
        const auto close = pattern.find('}', open);
        if (close == std::string_view::npos) {
            literal(pattern.substr(open));
            return;
        }
        const std::string_view inside = pattern.substr(open + 1, close - open - 1);
        const auto colon = inside.find(':');
        token(pattern.substr(open, close - open + 1), inside.substr(0, colon),
              colon == std::string_view::npos ? std::string_view{} : inside.substr(colon + 1));
        at = close + 1;
    }
}

bool isDeviceName(std::string_view name)
{
    const std::string base = lowerAscii(name.substr(0, name.find('.')));
    static constexpr std::array<std::string_view, 4> kPlain{"con", "prn", "aux", "nul"};
    if (std::find(kPlain.begin(), kPlain.end(), base) != kPlain.end()) {
        return true;
    }
    return base.size() == 4 && (base.starts_with("com") || base.starts_with("lpt")) &&
           base[3] >= '1' && base[3] <= '9';
}

void trimEnds(std::string& text)
{
    const auto trimmed = [](char c) { return c == ' ' || c == '.'; };
    while (!text.empty() && trimmed(text.back())) {
        text.pop_back();
    }
    std::size_t first = 0;
    while (first < text.size() && trimmed(text[first])) {
        ++first;
    }
    text.erase(0, first);
}

} // namespace

Status validateFileNamePattern(std::string_view pattern)
{
    if (pattern.find_first_not_of(' ') == std::string_view::npos) {
        return makeError(ErrorCode::InvalidArgument, "the file-name pattern is empty");
    }
    std::optional<std::string> problem;
    scanPattern(
        pattern,
        [&](std::string_view text) {
            if (!problem && text.find('{') != std::string_view::npos) {
                problem = std::format("a brace is left open in the file-name pattern \"{}\"",
                                      pattern);
            }
        },
        [&](std::string_view whole, std::string_view name, std::string_view format) {
            const bool known =
                std::find(kPatternTokens.begin(), kPatternTokens.end(), name) != kPatternTokens.end();
            if (!problem && (!known || (!format.empty() && !allDigits(format)))) {
                problem = std::format("the file-name pattern has no token {}: use {{n}}, "
                                      "{{n:02}}, {{N}}, {{set}}, {{name}}, {{number}} or {{id}}",
                                      whole);
            }
        });
    if (problem) {
        return makeError(ErrorCode::InvalidArgument, *problem);
    }
    return {};
}

Status validatePageSetup(const PageSetup& setup)
{
    if (!std::isfinite(setup.dpi) || setup.dpi < kMinimumPlotDpi || setup.dpi > kMaximumPlotDpi) {
        return makeError(ErrorCode::InvalidArgument,
                         std::format("the resolution must be {:g} to {:g} dpi", kMinimumPlotDpi,
                                     kMaximumPlotDpi),
                         std::format("{} dpi", setup.dpi));
    }
    if (!std::isfinite(setup.lineWeightScale) || setup.lineWeightScale < kMinimumLineWeightScale ||
        setup.lineWeightScale > kMaximumLineWeightScale) {
        return makeError(ErrorCode::InvalidArgument,
                         std::format("the line weight scale must be {:g} to {:g}",
                                     kMinimumLineWeightScale, kMaximumLineWeightScale),
                         std::format("{}", setup.lineWeightScale));
    }
    return validateFileNamePattern(setup.fileNamePattern);
}

PlotSettings plotSettingsFor(const PageSetup& setup)
{
    PlotSettings settings;
    settings.colourMode = setup.colourMode;
    settings.lineWeightScale = setup.lineWeightScale;
    settings.dpi = setup.dpi;
    return settings;
}

Result<std::vector<std::size_t>> parseSheetSelection(std::string_view text, const SheetSet& set)
{
    const std::size_t count = set.sheets.size();
    if (count == 0) {
        return makeError(ErrorCode::InvalidArgument, "there are no sheets to plot");
    }
    std::string compact;
    for (const char c : text) {
        if (c != ' ' && c != '\t') {
            compact.push_back(c);
        }
    }
    std::vector<std::size_t> chosen;
    if (compact.empty() || lowerAscii(compact) == "all") {
        for (std::size_t i = 0; i < count; ++i) {
            chosen.push_back(i);
        }
        return chosen;
    }
    std::set<std::size_t> seen;
    const auto take = [&](std::size_t index) {
        if (seen.insert(index).second) {
            chosen.push_back(index);
        }
    };
    // A position from 1 checked against the set; the reason when it is not one.
    const auto position = [&](std::string_view digits, std::size_t& index) -> std::optional<std::string> {
        const auto value = positionOf(digits);
        if (value && *value == 0) {
            return std::string("there is no sheet 0: sheets are numbered from 1");
        }
        if (!value || *value > count) {
            return std::format("there is no sheet {}: the set has {} sheet{}", digits, count,
                               count == 1 ? "" : "s");
        }
        index = *value - 1;
        return std::nullopt;
    };
    std::size_t at = 0;
    while (at <= compact.size()) {
        const auto comma = std::min(compact.find(',', at), compact.size());
        const std::string_view part = std::string_view(compact).substr(at, comma - at);
        at = comma + 1;
        if (part.empty()) {
            continue;
        }
        const auto dash = part.find('-');
        if (allDigits(part)) {
            std::size_t index = 0;
            if (auto why = position(part, index)) {
                return makeError(ErrorCode::InvalidArgument, *why);
            }
            take(index);
        } else if (dash != std::string_view::npos && dash > 0 && allDigits(part.substr(0, dash)) &&
                   allDigits(part.substr(dash + 1))) {
            std::size_t first = 0;
            std::size_t last = 0;
            if (auto why = position(part.substr(0, dash), first)) {
                return makeError(ErrorCode::InvalidArgument, *why);
            }
            if (auto why = position(part.substr(dash + 1), last)) {
                return makeError(ErrorCode::InvalidArgument, *why);
            }
            if (last < first) {
                return makeError(ErrorCode::InvalidArgument,
                                 std::format("the range {} runs backwards: write {}-{}", part,
                                             last + 1, first + 1));
            }
            for (std::size_t i = first; i <= last; ++i) {
                take(i);
            }
        } else if (const auto found = sheetIndex(set, part)) {
            take(*found);
        } else {
            return makeError(ErrorCode::InvalidArgument,
                             std::format("\"{}\" is neither a sheet number nor a sheet's id", part));
        }
    }
    if (chosen.empty()) {
        return makeError(ErrorCode::InvalidArgument, "the selection names no sheet",
                         std::string(text));
    }
    return chosen;
}

std::string formatSheetSelection(std::span<const std::size_t> indices)
{
    std::string out;
    std::size_t i = 0;
    while (i < indices.size()) {
        std::size_t run = 1;
        while (i + run < indices.size() && indices[i + run] == indices[i] + run) {
            ++run;
        }
        if (!out.empty()) {
            out += ',';
        }
        if (run >= 3) {
            out += std::format("{}-{}", indices[i] + 1, indices[i] + run);
            i += run;
        } else {
            out += std::to_string(indices[i] + 1);
            ++i;
        }
    }
    return out;
}

std::string sanitiseFileName(std::string_view name)
{
    static constexpr std::string_view kForbidden = "<>:\"/\\|?*";
    std::string out;
    for (const char c : name) {
        const auto byte = static_cast<unsigned char>(c);
        const char kept = byte < 0x20 || byte == 0x7F || kForbidden.find(c) != std::string_view::npos
                              ? '-'
                              : c;
        if (kept == ' ' && (out.empty() || out.back() == ' ')) {
            continue;
        }
        out.push_back(kept);
    }
    trimEnds(out);
    if (out.size() > kMaximumFileNameBytes) {
        std::size_t cut = kMaximumFileNameBytes;
        // Back to the first byte of a character: never half of one.
        while (cut > 0 && (static_cast<unsigned char>(out[cut]) & 0xC0u) == 0x80u) {
            --cut;
        }
        out.resize(cut);
        trimEnds(out);
    }
    if (out.empty()) {
        return "sheet";
    }
    if (isDeviceName(out)) {
        out.insert(0, 1, '_');
    }
    return out;
}

std::string sheetFileName(const SheetSet& set, std::size_t index, std::string_view pattern)
{
    if (index >= set.sheets.size()) {
        return {};
    }
    const Sheet& sheet = set.sheets[index];
    const std::size_t count = set.sheets.size();
    std::string expanded;
    scanPattern(
        pattern, [&](std::string_view text) { expanded.append(text); },
        [&](std::string_view whole, std::string_view name, std::string_view) {
            if (name == "name") {
                expanded += sheet.name;
            } else if (name == "number") {
                expanded +=
                    formatSheetNumber(set.numbering, index + 1, count, set.defaults.setNumber);
            } else if (name == "id") {
                expanded += sheet.id;
            } else {
                // {n}, {n:02}, {N}, {set}; anything else comes back as written.
                expanded += formatSheetNumber(whole, index + 1, count, set.defaults.setNumber);
            }
        });
    return sanitiseFileName(expanded);
}

std::vector<std::string> sheetFileNames(const SheetSet& set, std::span<const std::size_t> indices,
                                        std::string_view pattern)
{
    std::vector<std::string> names;
    std::set<std::string> used;
    for (const std::size_t index : indices) {
        const std::string base = sheetFileName(set, index, pattern);
        std::string name = base;
        for (int n = 2; !used.insert(lowerAscii(name)).second; ++n) {
            name = std::format("{} ({})", base, n);
        }
        names.push_back(std::move(name));
    }
    return names;
}

Status setPageSetup(Document& document, const PageSetup& setup)
{
    if (Status status = validatePageSetup(setup); !status) {
        return status;
    }
    if (document.sheetSetStatus() && document.sheetSet().pageSetup == setup) {
        return {};
    }
    return editSheetSet(
        document,
        [&setup](SheetSet& set) -> Status {
            set.pageSetup = setup;
            return {};
        },
        "PAGE_SETUP");
}

} // namespace katana::cad::plotting
