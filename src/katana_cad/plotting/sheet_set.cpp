#include "katana/cad/plotting/sheet_set.hpp"

#include <algorithm>
#include <array>
#include <charconv>
#include <cmath>
#include <format>
#include <limits>
#include <utility>

#include "katana/cad/plotting/layout.hpp"

namespace katana::cad::plotting {

namespace {

constexpr std::array<std::pair<ViewportKind, std::string_view>, 10> kKindNames{{
    {ViewportKind::Plan, "plan"},
    {ViewportKind::LongSection, "long_section"},
    {ViewportKind::CrossSections, "cross_sections"},
    {ViewportKind::Model3D, "model_3d"},
    {ViewportKind::Legend, "legend"},
    {ViewportKind::Notes, "notes"},
    {ViewportKind::Image, "image"},
    {ViewportKind::KeyPlan, "key_plan"},
    {ViewportKind::SheetIndex, "sheet_index"},
    {ViewportKind::Revisions, "revisions"},
}};

// The margin all round a sheet with no frame: the single-page plot's default
// (PlotSettings::marginMm), so a frameless sheet and a plot agree.
constexpr double kFramelessMarginMm = 10.0;

bool isSection(ViewportKind kind)
{
    return kind == ViewportKind::LongSection || kind == ViewportKind::CrossSections;
}

// Whether a viewport of this kind is drawn to a scale a title block reports.
bool isScaled(ViewportKind kind)
{
    return kind == ViewportKind::Plan || isSection(kind);
}

// A denominator as a scale rule carries it: whole. 1 : 31.25 (a 1 : 250
// section at 8 times) prints as 1 : 31, as the owner's app rounds it.
std::string denominatorText(double denominator)
{
    return std::to_string(std::llround(denominator));
}

std::string paperName(PaperSize paper)
{
    switch (paper) {
    case PaperSize::A0:
        return "A0";
    case PaperSize::A1:
        return "A1";
    case PaperSize::A2:
        return "A2";
    case PaperSize::A3:
        return "A3";
    case PaperSize::A4:
        return "A4";
    }
    return {};
}

std::string orDefault(const std::string& value, const std::string& fallback)
{
    return value.empty() ? fallback : value;
}

// The number after `prefix` in `id`, or 0 for an id of another shape -
// anything but digits after the prefix, or digits too many for a count, which
// no id given out here can equal.
std::size_t numberAfter(std::string_view id, std::string_view prefix)
{
    if (!id.starts_with(prefix)) {
        return 0;
    }
    std::size_t number = 0;
    const char* first = id.data() + prefix.size();
    const char* last = id.data() + id.size();
    const auto parsed = std::from_chars(first, last, number);
    return parsed.ec == std::errc{} && parsed.ptr == last ? number : 0;
}

// `count` numbers, in order, that `used` does not hold: those after the
// highest; or, when the highest is too near the largest count to have
// `count` after it - only an id typed into the stored JSON can be - the
// lowest free ones. Ids are compared as numbers - "vp01" counts as 1, and 1
// is not given out beside it - and 0 is never given out.
std::vector<std::size_t> freshNumbers(std::vector<std::size_t> used, std::size_t count)
{
    std::sort(used.begin(), used.end());
    const std::size_t highest = used.empty() ? 0 : used.back();
    std::vector<std::size_t> fresh;
    fresh.reserve(count);
    if (count <= std::numeric_limits<std::size_t>::max() - highest) {
        for (std::size_t i = 1; i <= count; ++i) {
            fresh.push_back(highest + i);
        }
        return fresh;
    }
    auto taken = used.begin();
    for (std::size_t candidate = 1; fresh.size() < count; ++candidate) {
        while (taken != used.end() && *taken < candidate) {
            ++taken;
        }
        if (taken == used.end() || *taken != candidate) {
            fresh.push_back(candidate);
        }
    }
    return fresh;
}

std::vector<std::string> idsFrom(std::string_view prefix, const std::vector<std::size_t>& numbers)
{
    std::vector<std::string> ids;
    ids.reserve(numbers.size());
    for (const std::size_t number : numbers) {
        ids.push_back(std::string(prefix) + std::to_string(number));
    }
    return ids;
}

} // namespace

std::string_view toString(ViewportKind kind)
{
    for (const auto& [value, name] : kKindNames) {
        if (value == kind) {
            return name;
        }
    }
    return "plan";
}

std::optional<ViewportKind> viewportKindFrom(std::string_view name)
{
    for (const auto& [value, known] : kKindNames) {
        if (known == name) {
            return value;
        }
    }
    return std::nullopt;
}

std::string formatSheetNumber(std::string_view pattern, std::size_t number, std::size_t count,
                              std::string_view setNumber)
{
    std::string out;
    std::size_t at = 0;
    while (at < pattern.size()) {
        const auto open = pattern.find('{', at);
        if (open == std::string_view::npos) {
            out.append(pattern.substr(at));
            break;
        }
        out.append(pattern.substr(at, open - at));
        const auto close = pattern.find('}', open);
        if (close == std::string_view::npos) {
            out.append(pattern.substr(open));
            break;
        }
        const std::string_view token = pattern.substr(open + 1, close - open - 1);
        const auto colon = token.find(':');
        const std::string_view name = token.substr(0, colon);
        std::size_t width = 0;
        if (colon != std::string_view::npos) {
            const std::string_view spec = token.substr(colon + 1);
            std::from_chars(spec.data(), spec.data() + spec.size(), width);
        }
        std::string value;
        if (name == "n") {
            value = std::to_string(number);
        } else if (name == "N") {
            value = std::to_string(count);
        } else if (name == "set") {
            value = std::string(setNumber);
        } else {
            value = std::string(pattern.substr(open, close - open + 1));
        }
        if (value.size() < width) {
            value.insert(0, width - value.size(), '0');
        }
        out += value;
        at = close + 1;
    }
    return out;
}

std::string expandTemplate(std::string_view text,
                           const std::map<std::string, std::string, std::less<>>& fields)
{
    std::string out;
    out.reserve(text.size());
    std::size_t at = 0;
    while (at < text.size()) {
        const char c = text[at];
        if (c == '{' && at + 1 < text.size() && text[at + 1] == '{') {
            out += '{';
            at += 2;
            continue;
        }
        if (c == '{') {
            const auto close = text.find('}', at);
            if (close == std::string_view::npos) {
                out.append(text.substr(at));
                break;
            }
            const auto found = fields.find(text.substr(at + 1, close - at - 1));
            if (found != fields.end()) {
                out += found->second;
            }
            at = close + 1;
            continue;
        }
        out += c;
        ++at;
    }
    return out;
}

std::string scaleText(const Viewport& viewport)
{
    if (isSection(viewport.kind)) {
        const double vertical = viewport.verticalExaggeration > 0.0
                                    ? viewport.scale / viewport.verticalExaggeration
                                    : viewport.scale;
        return "H 1:" + denominatorText(viewport.scale) + " V 1:" + denominatorText(vertical);
    }
    return "1:" + denominatorText(viewport.scale);
}

std::string automaticTitle(const Viewport& viewport)
{
    switch (viewport.kind) {
    case ViewportKind::Plan:
        return "PLAN " + scaleText(viewport);
    case ViewportKind::LongSection:
        return "LONG SECTION " + scaleText(viewport);
    case ViewportKind::CrossSections:
        if (viewport.source.stations.size() == 1) {
            return std::format("CROSS SECTION CH {:.3f}", viewport.source.stations.front());
        }
        return "CROSS SECTIONS " + scaleText(viewport);
    case ViewportKind::Model3D:
        return "3D VIEW";
    case ViewportKind::Legend:
        return "LEGEND";
    case ViewportKind::Notes:
        return "NOTES";
    case ViewportKind::Image:
        return {};
    case ViewportKind::KeyPlan:
        return "KEY PLAN";
    case ViewportKind::SheetIndex:
        return "DRAWING REGISTER";
    case ViewportKind::Revisions:
        return "REVISIONS";
    }
    return {};
}

std::string sheetScaleText(const Sheet& sheet)
{
    // Plans and sections first; a key plan only when there is neither - an
    // inset key plan is at its own, smaller scale by design, and a key-plan
    // sheet is still drawn to one.
    const auto reported = [&sheet](const auto& counts) -> std::string {
        const Viewport* main = nullptr;
        for (const Viewport& viewport : sheet.viewports) {
            if (counts(viewport.kind) &&
                (main == nullptr || tilingRank(viewport.kind) < tilingRank(main->kind))) {
                main = &viewport;
            }
        }
        if (main == nullptr) {
            return {};
        }
        const std::string text = scaleText(*main);
        for (const Viewport& viewport : sheet.viewports) {
            if (counts(viewport.kind) && scaleText(viewport) != text) {
                return "AS SHOWN";
            }
        }
        return text;
    };
    if (std::string text = reported(isScaled); !text.empty()) {
        return text;
    }
    if (std::string text = reported([](ViewportKind kind) { return kind == ViewportKind::KeyPlan; });
        !text.empty()) {
        return text;
    }
    return "N.T.S.";
}

std::map<std::string, std::string, std::less<>>
resolveFields(const SheetSet& set, std::size_t sheetIndex, const FieldContext& context)
{
    std::map<std::string, std::string, std::less<>> fields;
    if (sheetIndex >= set.sheets.size()) {
        return fields;
    }
    const Sheet& sheet = set.sheets[sheetIndex];
    const SheetDefaults& d = set.defaults;
    const std::size_t count = set.sheets.size();
    // The count is the set's size - the owner's app left it as typed, so a
    // set that grew to twelve sheets still said "of 1".
    fields["sheet_number"] = formatSheetNumber(set.numbering, sheetIndex + 1, count, d.setNumber);
    fields["sheet_count"] = std::to_string(count);
    fields["sheet_name"] = sheet.name;
    fields["scale"] = sheetScaleText(sheet);
    fields["paper"] = paperName(sheet.paper);
    fields["plot_date"] = context.plotDate;
    const std::pair<std::string_view, const SignOff*> signOffs[] = {
        {"locator", &d.locator},   {"surveyor", &d.surveyor}, {"compiler", &d.compiler},
        {"reviewer", &d.reviewer}, {"approver", &d.approver},
    };
    // Each sign-off keeps its own date; one not given is the plot's.
    for (const auto& [role, signOff] : signOffs) {
        fields[std::string(role) + "_name"] = signOff->name;
        fields[std::string(role) + "_date"] = orDefault(signOff->date, context.plotDate);
    }
    fields["organisation"] = d.organisation;
    fields["client"] = d.client;
    fields["notes"] = d.notes;
    fields["height_datum"] = d.heightDatum;
    fields["coordinate_system"] = orDefault(d.coordinateSystem, context.coordinateSystem);
    fields["model_name"] = d.modelName;
    fields["set_number"] = d.setNumber;
    const auto line = [&d](std::size_t index) {
        return index < d.projectLines.size() ? d.projectLines[index] : std::string{};
    };
    fields["project_line_1"] = orDefault(line(0), context.projectName);
    fields["project_line_2"] = orDefault(line(1), context.projectDescription);
    fields["project_line_3"] = line(2);
    fields["project_line_4"] = line(3);
    fields["file_name"] = context.fileName;
    fields["revision"] = set.revisions.empty() ? std::string{} : set.revisions.back().code;
    // What the user typed on the sheet wins over everything above.
    for (const auto& [name, value] : sheet.fields) {
        fields.insert_or_assign(name, value);
    }
    return fields;
}

Box2 drawingArea(const Sheet& sheet)
{
    if (!sheet.frame.empty() && sheet.landscape && sheet.frame == kBuiltInFrameId) {
        if (const auto& frame = builtInFrame(); frame) {
            const double factor = frameScaleFor(sheet.paper, true);
            const Box2& area = frame->drawingArea;
            return Box2(Point2(area.min.x * factor, area.min.y * factor),
                        Point2(area.max.x * factor, area.max.y * factor));
        }
    }
    const PaperDimensions paper = paperDimensions(sheet.paper, sheet.landscape);
    return Box2(Point2(kFramelessMarginMm, kFramelessMarginMm),
                Point2(paper.widthMm - kFramelessMarginMm, paper.heightMm - kFramelessMarginMm));
}

std::vector<std::string> newViewportIds(const SheetSet& set, std::size_t count)
{
    std::vector<std::size_t> used;
    for (const Sheet& sheet : set.sheets) {
        for (const Viewport& viewport : sheet.viewports) {
            used.push_back(numberAfter(viewport.id, "vp"));
        }
    }
    return idsFrom("vp", freshNumbers(std::move(used), count));
}

std::vector<std::string> newSheetIds(const SheetSet& set, std::size_t count)
{
    // The ids marks name count as used as well as the sheets' own: when a
    // sheet is removed, the match lines and key-plan outlines that led to it
    // still name it, and a new sheet given its id would be led to by them.
    std::vector<std::size_t> used;
    for (const Sheet& sheet : set.sheets) {
        used.push_back(numberAfter(sheet.id, "s"));
        for (const Viewport& viewport : sheet.viewports) {
            for (const WorldMark& mark : viewport.marks) {
                used.push_back(numberAfter(mark.sheet, "s"));
            }
        }
    }
    return idsFrom("s", freshNumbers(std::move(used), count));
}

std::string nextViewportId(const SheetSet& set)
{
    return newViewportIds(set, 1).front();
}

std::string nextSheetId(const SheetSet& set)
{
    return newSheetIds(set, 1).front();
}

std::optional<std::size_t> sheetIndex(const SheetSet& set, std::string_view id)
{
    for (std::size_t i = 0; i < set.sheets.size(); ++i) {
        if (!id.empty() && set.sheets[i].id == id) {
            return i;
        }
    }
    return std::nullopt;
}

std::string markLabel(const SheetSet& set, const WorldMark& mark)
{
    const auto index = sheetIndex(set, mark.sheet);
    if (!index) {
        return mark.label;
    }
    const Sheet& target = set.sheets[*index];
    const auto overridden = target.fields.find("sheet_number");
    const std::string number =
        overridden != target.fields.end()
            ? overridden->second
            : formatSheetNumber(set.numbering, *index + 1, set.sheets.size(),
                                set.defaults.setNumber);
    if (mark.kind == WorldMark::Kind::SheetOutline) {
        return mark.label.empty() ? number : mark.label + " " + number;
    }
    return mark.label.empty() ? "SEE SHEET " + number : mark.label + " - SEE SHEET " + number;
}

} // namespace katana::cad::plotting
