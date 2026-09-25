#include "katana/cad/plotting/sheet_verbs.hpp"

#include <algorithm>
#include <array>
#include <cctype>
#include <charconv>
#include <cmath>
#include <format>
#include <fstream>
#include <initializer_list>
#include <iterator>
#include <utility>

#include "katana/cad/plotting/arrange.hpp"
#include "katana/cad/plotting/arrange_commands.hpp"
#include "katana/cad/plotting/generators.hpp"
#include "katana/cad/plotting/layout.hpp"
#include "katana/cad/plotting/legend.hpp"
#include "katana/cad/plotting/page_setup.hpp"
#include "katana/cad/plotting/plan_grid.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/cad/plotting/sheet_json.hpp"
#include "katana/cad/plotting/tables.hpp"
#include "katana/cad/selection.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad::plotting {

using katana::core::Error;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using Words = std::vector<std::string>;

namespace {

// An Image view may show a photograph of the site, so it is allowed more than
// a logo; still not so much that every PDF of the set carries a raw camera
// file.
constexpr std::uintmax_t kMaximumImageBytes = 32u * 1024u * 1024u;

// ---- words ---------------------------------------------------------------------------

std::string upper(std::string_view text)
{
    std::string out(text);
    std::ranges::transform(out, out.begin(),
                           [](unsigned char c) { return static_cast<char>(std::toupper(c)); });
    return out;
}

// Letters and digits only, lower case: "Project_Line_1", "project line 1" and
// "PROJECTLINE1" are one key, so neither a person nor an agent has to
// remember which separator a name uses.
std::string folded(std::string_view text)
{
    std::string out;
    for (const unsigned char c : text) {
        if (std::isalnum(c) != 0) {
            out += static_cast<char>(std::tolower(c));
        }
    }
    return out;
}

std::string joined(const Words& words, std::size_t from, std::string_view separator = " ")
{
    std::string out;
    for (std::size_t i = from; i < words.size(); ++i) {
        if (i > from) {
            out += separator;
        }
        out += words[i];
    }
    return out;
}

// A typed "\n" is a line break: a command line has no other way to put one
// in a note. "\\" is a backslash, so a text that holds "\n" itself (a path,
// C:\\new) can be typed, as a reply writes it (inQuotes). Any other
// backslash is itself.
std::string unescaped(std::string_view text)
{
    std::string out;
    for (std::size_t i = 0; i < text.size(); ++i) {
        const char next = i + 1 < text.size() ? text[i + 1] : '\0';
        if (text[i] == '\\' && next == 'n') {
            out += '\n';
            ++i;
        } else if (text[i] == '\\' && next == '\\') {
            out += '\\';
            ++i;
        } else {
            out += text[i];
        }
    }
    return out;
}

// A value in a reply: in double quotes, a line break written back as "\n"
// and a backslash as "\\", so the reply stays one fact per line and a text
// typed back (unescaped) is the text stored. A double quote, which only a
// loaded set can hold (the command line cannot type one inside quotes), is
// written "\"" so the reply still reads unambiguously.
std::string inQuotes(std::string_view text)
{
    std::string out = "\"";
    for (const char c : text) {
        if (c == '\n') {
            out += "\\n";
        } else if (c == '\\') {
            out += "\\\\";
        } else if (c == '"') {
            out += "\\\"";
        } else {
            out += c;
        }
    }
    return out + "\"";
}

std::string countOf(std::size_t count, std::string_view one, std::string_view many)
{
    return std::format("{} {}", count, count == 1 ? one : many);
}

Error usage(std::string_view text)
{
    return makeError(ErrorCode::InvalidArgument, "usage: " + std::string(text));
}

// A path typed on the command line is UTF-8 text; on Windows a narrow string
// would be read in the ANSI code page instead.
std::filesystem::path pathFrom(std::string_view utf8)
{
    std::u8string text;
    text.reserve(utf8.size());
    for (const char c : utf8) {
        text += static_cast<char8_t>(c);
    }
    return std::filesystem::path(text);
}

std::string pathText(const std::filesystem::path& path)
{
    const std::u8string text = path.u8string();
    return std::string(text.begin(), text.end());
}

// ---- values --------------------------------------------------------------------------

// Locale independent ("1.5" is never "1,5"), finite, and an explicit '+' allowed.
Result<double> number(std::string_view text, std::string_view what)
{
    double value = 0.0;
    const char* begin = text.data();
    const char* const end = text.data() + text.size();
    if (begin != end && *begin == '+') {
        ++begin;
    }
    const auto [parsed, error] = std::from_chars(begin, end, value);
    if (text.empty() || error != std::errc{} || parsed != end || !std::isfinite(value)) {
        return makeError(ErrorCode::ParseFailure, std::format("{} must be a number", what),
                         std::string(text));
    }
    return value;
}

Result<double> positive(std::string_view text, std::string_view what)
{
    auto value = number(text, what);
    if (value && !(*value > 0.0)) {
        return makeError(ErrorCode::InvalidArgument, std::format("{} must be more than 0", what),
                         std::string(text));
    }
    return value;
}

Result<double> notNegative(std::string_view text, std::string_view what)
{
    auto value = number(text, what);
    if (value && *value < 0.0) {
        return makeError(ErrorCode::InvalidArgument, std::format("{} cannot be negative", what),
                         std::string(text));
    }
    return value;
}

// A whole number from 1.
Result<std::size_t> counting(std::string_view text, std::string_view what)
{
    std::size_t value = 0;
    const char* const end = text.data() + text.size();
    const auto [parsed, error] = std::from_chars(text.data(), end, value);
    if (text.empty() || error != std::errc{} || parsed != end || value == 0) {
        return makeError(ErrorCode::ParseFailure,
                         std::format("{} must be a whole number from 1", what), std::string(text));
    }
    return value;
}

Result<bool> onOff(std::string_view text, std::string_view key)
{
    const std::string word = folded(text);
    if (word == "on" || word == "yes" || word == "true" || word == "1") {
        return true;
    }
    if (word == "off" || word == "no" || word == "false" || word == "0") {
        return false;
    }
    return makeError(ErrorCode::ParseFailure, std::format("{}= is on or off", key),
                     std::string(text));
}

// "a,b,c": numbers separated by commas; an empty text is an empty list.
Result<std::vector<double>> numberList(std::string_view text, std::string_view what)
{
    std::vector<double> values;
    if (text.empty()) {
        return values;
    }
    std::size_t start = 0;
    while (true) {
        const std::size_t comma = text.find(',', start);
        const std::string_view part =
            text.substr(start, comma == std::string_view::npos ? std::string_view::npos
                                                               : comma - start);
        auto value = number(part, what);
        if (!value) {
            return makeError(ErrorCode::ParseFailure,
                             std::format("{} must be numbers separated by commas", what),
                             std::string(text));
        }
        values.push_back(*value);
        if (comma == std::string_view::npos) {
            return values;
        }
        start = comma + 1;
    }
}

Result<Point2> pointFrom(std::string_view text, std::string_view what)
{
    auto values = numberList(text, what);
    if (!values || values->size() != 2) {
        return makeError(ErrorCode::ParseFailure, std::format("{} is x,y", what),
                         std::string(text));
    }
    return Point2((*values)[0], (*values)[1]);
}

// "x0,y0,x1,y1", either corner first; a rectangle with no area is refused.
Result<Box2> boxFrom(std::string_view text, std::string_view what)
{
    auto values = numberList(text, what);
    if (!values || values->size() != 4) {
        return makeError(ErrorCode::ParseFailure, std::format("{} is x0,y0,x1,y1", what),
                         std::string(text));
    }
    const auto& v = *values;
    const Box2 box(Point2(std::min(v[0], v[2]), std::min(v[1], v[3])),
                   Point2(std::max(v[0], v[2]), std::max(v[1], v[3])));
    if (!(box.width() > 0.0) || !(box.height() > 0.0)) {
        return makeError(ErrorCode::InvalidArgument,
                         std::format("{} must have a width and a height", what),
                         std::string(text));
    }
    return box;
}

// 1 : scale, typed as "500" or "1:500"; nullopt for "auto".
Result<std::optional<double>> scaleFrom(std::string_view text)
{
    if (folded(text) == "auto") {
        return std::optional<double>{};
    }
    std::string_view denominator = text;
    if (denominator.starts_with("1:")) {
        denominator.remove_prefix(2);
    }
    auto value = positive(denominator, "scale=");
    if (!value) {
        return value.error();
    }
    return std::optional<double>(*value);
}

// A number as a reply writes it: to a millionth - a micrometre of paper, a
// micro-degree - in its shortest form, so 25.5 is "25.5" and not
// "25.499999999999996", and -0 is "0".
std::string decimal(double value)
{
    double rounded = value;
    if (std::isfinite(value * 1e6)) {
        rounded = std::round(value * 1e6) / 1e6;
    }
    if (rounded == 0.0) {
        rounded = 0.0;
    }
    return std::format("{}", rounded);
}

std::string pointText(const Point2& point)
{
    return decimal(point.x) + "," + decimal(point.y);
}

std::string boxText(const Box2& box)
{
    if (box.empty()) {
        return "none";
    }
    return pointText(box.min) + "," + pointText(box.max);
}

std::string_view onOffText(bool on)
{
    return on ? "on" : "off";
}

// ---- options -------------------------------------------------------------------------

// A word of the form key=value. The key is folded; the value is as typed.
struct Option {
    std::string key;
    std::string value;
    std::string word; // as typed, for an error's context
};

std::optional<Option> optionFrom(const std::string& word)
{
    const std::size_t equals = word.find('=');
    if (equals == std::string::npos || equals == 0) {
        return std::nullopt;
    }
    return Option{folded(std::string_view(word).substr(0, equals)), word.substr(equals + 1), word};
}

// Every word from `from` on must be key=value.
Result<std::vector<Option>> optionsFrom(const Words& words, std::size_t from,
                                        std::string_view usageText)
{
    std::vector<Option> options;
    for (std::size_t i = from; i < words.size(); ++i) {
        auto option = optionFrom(words[i]);
        if (!option) {
            return makeError(ErrorCode::InvalidArgument,
                             std::format("expected option=value; usage: {}", usageText), words[i]);
        }
        options.push_back(std::move(*option));
    }
    return options;
}

Error unknownOption(const Option& option, std::string_view where, std::string_view known)
{
    return makeError(ErrorCode::InvalidArgument,
                     std::format("{} takes no option {}=; it takes {}", where, option.key, known),
                     option.word);
}

Result<PaperSize> paperFrom(std::string_view text)
{
    static constexpr std::array<std::pair<std::string_view, PaperSize>, 5> kPapers{
        {{"a0", PaperSize::A0},
         {"a1", PaperSize::A1},
         {"a2", PaperSize::A2},
         {"a3", PaperSize::A3},
         {"a4", PaperSize::A4}}};
    const std::string word = folded(text);
    for (const auto& [name, paper] : kPapers) {
        if (name == word) {
            return paper;
        }
    }
    return makeError(ErrorCode::InvalidArgument, "paper= is A0, A1, A2, A3 or A4",
                     std::string(text));
}

std::string_view paperName(PaperSize paper)
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
    return "A3";
}

Result<bool> landscapeFrom(std::string_view text)
{
    const std::string word = folded(text);
    if (word == "landscape") {
        return true;
    }
    if (word == "portrait") {
        return false;
    }
    return makeError(ErrorCode::InvalidArgument, "orientation= is landscape or portrait",
                     std::string(text));
}

// on (the built-in frame), off or none (no frame), or a frame's id.
Result<std::string> frameFrom(std::string_view text)
{
    const std::string word = folded(text);
    if (word == "on" || word == "yes" || word == "true" || word == folded(kBuiltInFrameId)) {
        return std::string(kBuiltInFrameId);
    }
    if (word == "off" || word == "none" || word == "no" || word == "false") {
        return std::string{};
    }
    return makeError(ErrorCode::NotFound,
                     std::format("no plot frame of that name: frame= is on, off or {}",
                                 kBuiltInFrameId),
                     std::string(text));
}

// The kinds by their stored names, and the words a person reaches for.
Result<ViewportKind> kindFrom(std::string_view text)
{
    if (const auto stored = viewportKindFrom(text)) {
        return *stored;
    }
    static constexpr std::array<std::pair<std::string_view, ViewportKind>, 23> kWords{{
        {"plan", ViewportKind::Plan},
        {"longsection", ViewportKind::LongSection},
        {"profile", ViewportKind::LongSection},
        {"crosssections", ViewportKind::CrossSections},
        {"crosssection", ViewportKind::CrossSections},
        {"sections", ViewportKind::CrossSections},
        {"xs", ViewportKind::CrossSections},
        {"model3d", ViewportKind::Model3D},
        {"3d", ViewportKind::Model3D},
        {"snapshot", ViewportKind::Model3D},
        {"legend", ViewportKind::Legend},
        {"notes", ViewportKind::Notes},
        {"note", ViewportKind::Notes},
        {"image", ViewportKind::Image},
        {"keyplan", ViewportKind::KeyPlan},
        {"key", ViewportKind::KeyPlan},
        {"sheetindex", ViewportKind::SheetIndex},
        {"register", ViewportKind::SheetIndex},
        {"drawingregister", ViewportKind::SheetIndex},
        {"index", ViewportKind::SheetIndex},
        {"revisions", ViewportKind::Revisions},
        {"revisiontable", ViewportKind::Revisions},
        {"revision", ViewportKind::Revisions},
    }};
    const std::string word = folded(text);
    for (const auto& [name, kind] : kWords) {
        if (name == word) {
            return kind;
        }
    }
    return makeError(ErrorCode::InvalidArgument,
                     "a view is plan, key_plan, long_section, cross_sections, model_3d, legend, "
                     "notes, image, sheet_index (the drawing register) or revisions",
                     std::string(text));
}

// A preset by its stored id (cols2), its menu name ("Main and panel right")
// or the name of its value (MainRight), in any case and spacing.
Result<TilingPreset> presetFrom(std::string_view text)
{
    const std::string word = folded(text);
    for (std::size_t i = 0; i < kTilingPresetCount; ++i) {
        const auto preset = static_cast<TilingPreset>(i);
        if (word == folded(presetId(preset)) || word == folded(presetName(preset))) {
            return preset;
        }
    }
    static constexpr std::array<std::pair<std::string_view, TilingPreset>, 8> kWords{{
        {"columns2", TilingPreset::Columns2},
        {"columns", TilingPreset::Columns2},
        {"rows", TilingPreset::Rows2},
        {"quarters", TilingPreset::Quad},
        {"mainright", TilingPreset::MainRight},
        {"mainbelow", TilingPreset::MainBelow},
        {"maincorner", TilingPreset::MainCorner},
        {"maintworight", TilingPreset::MainTwoRight},
    }};
    for (const auto& [name, preset] : kWords) {
        if (name == word) {
            return preset;
        }
    }
    return makeError(ErrorCode::InvalidArgument,
                     "a tiling preset is full, cols2, rows2, quad, sectionR, sectionB, sectionBR "
                     "or sectionMap3d, or its menu name",
                     std::string(text));
}

// ---- viewports -----------------------------------------------------------------------

bool isOneOf(ViewportKind kind, std::initializer_list<ViewportKind> kinds)
{
    return std::ranges::find(kinds, kind) != kinds.end();
}

bool drawnToScale(ViewportKind kind)
{
    return isOneOf(kind, {ViewportKind::Plan, ViewportKind::KeyPlan, ViewportKind::LongSection,
                          ViewportKind::CrossSections});
}

bool isSection(ViewportKind kind)
{
    return isOneOf(kind, {ViewportKind::LongSection, ViewportKind::CrossSections});
}

bool isPlanLike(ViewportKind kind)
{
    return isOneOf(kind, {ViewportKind::Plan, ViewportKind::KeyPlan});
}

Status onlyFor(const Viewport& viewport, const Option& option, bool allowed,
               std::string_view kinds)
{
    if (allowed) {
        return {};
    }
    return makeError(ErrorCode::InvalidArgument,
                     std::format("{}= is for {} views, not {}", option.key, kinds,
                                 toString(viewport.kind)),
                     option.word);
}

// The editor's placing of a new view: its size for the kind, centred in the
// tiling area, or the whole area on an empty sheet.
Box2 freePlace(const Sheet& sheet, double width, double height)
{
    const Box2 area = tilingArea(sheet);
    if (sheet.viewports.empty()) {
        return area;
    }
    width = std::min(width, area.width());
    height = std::min(height, area.height());
    const Point2 c = area.center();
    return Box2(Point2(c.x - width / 2.0, c.y - height / 2.0),
                Point2(c.x + width / 2.0, c.y + height / 2.0));
}

// The chainage half way along alignment `name`, when it solves.
std::optional<double> middleStation(const entity::Model& model, const std::string& name)
{
    const entity::Alignment* alignment = model.alignments.find(name);
    if (alignment == nullptr) {
        return std::nullopt;
    }
    auto solved = geometry::solveAlignment(alignment->horizontal);
    if (!solved) {
        return std::nullopt;
    }
    return (solved->startStation() + solved->endStation()) / 2.0;
}

Result<std::string> alignmentNamed(const entity::Model& model, std::string_view name)
{
    if (model.alignments.find(std::string(name)) == nullptr) {
        return makeError(ErrorCode::NotFound, "the drawing has no alignment of that name",
                         std::string(name));
    }
    return std::string(name);
}

// The alignment a generator runs along: the one named, else the drawing's
// only one.
Result<std::string> alignmentFor(const entity::Model& model, const std::string& given)
{
    if (!given.empty()) {
        return alignmentNamed(model, given);
    }
    const std::vector<std::string> names = model.alignments.names();
    if (names.size() == 1) {
        return names.front();
    }
    if (names.empty()) {
        return makeError(ErrorCode::NotFound,
                         "the drawing has no alignment: define one with ALIGN NEW");
    }
    return makeError(ErrorCode::InvalidArgument,
                     "name the alignment with alignment=: the drawing has " + joined(names, 0, ", "));
}

// Where the viewport `id` is: its sheet's index and its own. An id is
// matched as typed and then in any case (VP3 is vp3).
std::optional<std::pair<std::size_t, std::size_t>> findViewport(const SheetSet& set,
                                                                std::string_view id)
{
    for (const bool exact : {true, false}) {
        for (std::size_t s = 0; s < set.sheets.size(); ++s) {
            const auto& viewports = set.sheets[s].viewports;
            for (std::size_t v = 0; v < viewports.size(); ++v) {
                if (exact ? viewports[v].id == id : upper(viewports[v].id) == upper(id)) {
                    return std::pair(s, v);
                }
            }
        }
    }
    return std::nullopt;
}

Error noViewport(std::string_view id)
{
    return makeError(ErrorCode::NotFound, "no view with that id; VIEW LIST shows them",
                     std::string(id));
}

// A view must land on its paper, or it prints nothing.
Status checkOnPaper(const Sheet& sheet, const Viewport& viewport)
{
    const PaperDimensions paper = paperDimensions(sheet.paper, sheet.landscape);
    const Box2 page(Point2(0.0, 0.0), Point2(paper.widthMm, paper.heightMm));
    if (viewport.rect.empty() || page.intersects(viewport.rect)) {
        return {};
    }
    return makeError(ErrorCode::InvalidArgument,
                     std::format("view {} would be off the paper: {} {} is {} x {} mm", viewport.id,
                                 paperName(sheet.paper),
                                 sheet.landscape ? "landscape" : "portrait",
                                 decimal(paper.widthMm), decimal(paper.heightMm)),
                     "rect=" + boxText(viewport.rect));
}

// The short line SHEETS LIST gives a view.
std::string viewportSummary(const Viewport& viewport)
{
    std::string line = std::format("view id={} kind={}", viewport.id, toString(viewport.kind));
    if (drawnToScale(viewport.kind)) {
        line += " scale=" + (viewport.autoScale ? std::string("auto") : decimal(viewport.scale));
    }
    return line + " rect=" + boxText(viewport.rect);
}

// The sheet's own line, without its views.
std::string sheetLine(const SheetSet& set, std::size_t index)
{
    const Sheet& sheet = set.sheets[index];
    return std::format("sheet {} id={} name={} paper={} orientation={} frame={} legendblock={} "
                       "views={}",
                       index + 1, sheet.id, inQuotes(sheet.name), paperName(sheet.paper),
                       sheet.landscape ? "landscape" : "portrait",
                       sheet.frame.empty() ? std::string("none") : sheet.frame,
                       onOffText(sheet.frameLegend), sheet.viewports.size());
}

Result<const SheetSet*> readable(const Document& document)
{
    if (const Status status = document.sheetSetStatus(); !status) {
        return makeError(status.error().code,
                         "the project's sheets cannot be read: " + status.error().message,
                         status.error().context);
    }
    return &document.sheetSet();
}

// ---- the title block -----------------------------------------------------------------

struct TitleField {
    std::string_view name;      // as TITLEBLOCK takes and lists it
    std::string_view frameName; // the frame's field of the same value, if any
};

constexpr std::array<TitleField, 22> kTitleFields{{
    {"organisation", "organisation"},
    {"project1", "project_line_1"},
    {"project2", "project_line_2"},
    {"project3", "project_line_3"},
    {"project4", "project_line_4"},
    {"client", "client"},
    {"setnumber", "set_number"},
    {"numbering", ""},
    {"coordsys", "coordinate_system"},
    {"datum", "height_datum"},
    {"model", "model_name"},
    {"notes", "notes"},
    {"locatorname", "locator_name"},
    {"locatordate", "locator_date"},
    {"surveyorname", "surveyor_name"},
    {"surveyordate", "surveyor_date"},
    {"compilername", "compiler_name"},
    {"compilerdate", "compiler_date"},
    {"reviewername", "reviewer_name"},
    {"reviewerdate", "reviewer_date"},
    {"approvername", "approver_name"},
    {"approverdate", "approver_date"},
}};

std::optional<std::string_view> titleFieldFrom(std::string_view field)
{
    const std::string word = folded(field);
    for (const TitleField& known : kTitleFields) {
        if (word == folded(known.name) || (!known.frameName.empty() && word == folded(known.frameName))) {
            return known.name;
        }
    }
    if (word == "organization" || word == "org") {
        return "organisation";
    }
    if (word == "set") {
        return "setnumber";
    }
    return std::nullopt;
}

// The string field `name` (a TitleField name, not a project line) is kept
// in; `Set` is SheetSet or const SheetSet.
template <typename Set> auto* titleSlot(Set& set, std::string_view name)
{
    auto& d = set.defaults;
    using Slot = decltype(&d.organisation);
    if (name == "organisation") {
        return &d.organisation;
    }
    if (name == "client") {
        return &d.client;
    }
    if (name == "setnumber") {
        return &d.setNumber;
    }
    if (name == "numbering") {
        return &set.numbering;
    }
    if (name == "coordsys") {
        return &d.coordinateSystem;
    }
    if (name == "datum") {
        return &d.heightDatum;
    }
    if (name == "model") {
        return &d.modelName;
    }
    if (name == "notes") {
        return &d.notes;
    }
    const std::pair<std::string_view, decltype(&d.locator)> roles[] = {
        {"locator", &d.locator},   {"surveyor", &d.surveyor}, {"compiler", &d.compiler},
        {"reviewer", &d.reviewer}, {"approver", &d.approver}};
    for (const auto& [role, signOff] : roles) {
        if (name.starts_with(role)) {
            const std::string_view part = name.substr(role.size());
            if (part == "name") {
                return static_cast<Slot>(&signOff->name);
            }
            if (part == "date") {
                return static_cast<Slot>(&signOff->date);
            }
        }
    }
    return static_cast<Slot>(nullptr);
}

// 1 to 4 for project1..project4, else 0.
std::size_t projectLine(std::string_view name)
{
    if (name.size() == 8 && name.starts_with("project") && name[7] >= '1' && name[7] <= '4') {
        return static_cast<std::size_t>(name[7] - '0');
    }
    return 0;
}

Error unknownTitleField(std::string_view field)
{
    return makeError(ErrorCode::NotFound,
                     "no title-block field of that name; TITLEBLOCK LIST shows them",
                     std::string(field));
}

std::string revisionLine(const Revision& revision)
{
    return std::format("revision code={} date={} description={} by={}", inQuotes(revision.code),
                       inQuotes(revision.date), inQuotes(revision.description), inQuotes(revision.by));
}

// ---- SHEETS --------------------------------------------------------------------------

// The page setup as SHEETS PAGESETUP prints it, in the words it takes.
std::string pageSetupLine(const PageSetup& setup)
{
    return std::format("pagesetup style={} lineweight={} dpi={} pattern={} filepersheet={}",
                       toString(setup.colourMode), decimal(setup.lineWeightScale),
                       decimal(setup.dpi), inQuotes(setup.fileNamePattern),
                       onOffText(setup.filePerSheet));
}

// The plot style by its stored name or a common other one (plotColourModeFrom).
Result<PlotColourMode> styleFrom(std::string_view text)
{
    if (const auto mode = plotColourModeFrom(text)) {
        return *mode;
    }
    return makeError(ErrorCode::InvalidArgument, "style= is colour, grey or mono", std::string(text));
}

// SHEETS PAGESETUP [style=] [lineweight=] [dpi=] [pattern=] [filepersheet=]:
// the page setup kept with the set, printed, or changed as one step.
Result<std::string> pageSetupVerb(Document& document, const Words& args)
{
    constexpr std::string_view kUsage =
        "SHEETS PAGESETUP [style=colour|grey|mono] [lineweight=f] [dpi=n] [pattern=text] "
        "[filepersheet=on|off]";
    auto options = optionsFrom(args, 1, kUsage);
    if (!options) {
        return options.error();
    }
    PageSetup setup = document.sheetSet().pageSetup;
    if (options->empty()) {
        return pageSetupLine(setup);
    }
    for (const Option& option : *options) {
        const std::string& key = option.key;
        if (key == "style" || key == "colour" || key == "color" || key == "mode") {
            auto mode = styleFrom(option.value);
            if (!mode) {
                return mode.error();
            }
            setup.colourMode = *mode;
        } else if (key == "lineweight" || key == "lineweightscale") {
            auto factor = number(option.value, "lineweight=");
            if (!factor) {
                return factor.error();
            }
            setup.lineWeightScale = *factor;
        } else if (key == "dpi") {
            auto dpi = number(option.value, "dpi=");
            if (!dpi) {
                return dpi.error();
            }
            setup.dpi = *dpi;
        } else if (key == "pattern" || key == "names") {
            setup.fileNamePattern = unescaped(option.value);
        } else if (key == "filepersheet") {
            auto on = onOff(option.value, key);
            if (!on) {
                return on.error();
            }
            setup.filePerSheet = *on;
        } else {
            return unknownOption(option, "a page setup",
                                 "style=, lineweight=, dpi=, pattern= and filepersheet=");
        }
    }
    // Checked before the step, so a refused value says which it was.
    if (const Status valid = validatePageSetup(setup); !valid) {
        return valid.error();
    }
    if (const Status status = setPageSetup(document, setup); !status) {
        return status.error();
    }
    return pageSetupLine(document.sheetSet().pageSetup);
}

// SHEETS CHECK [sheets=1,3-5] [json]: the preflight checks, a summary line
// and then a finding a line (checkReplyLine), or the findings as JSON.
Result<std::string> checkVerb(Document& document, const Words& args,
                              const SheetVerbContextProvider& context)
{
    constexpr std::string_view kUsage = "SHEETS CHECK [sheets=1,3-5] [json]";
    const SheetSet& set = document.sheetSet();
    std::vector<std::size_t> indices;
    bool json = false;
    for (std::size_t i = 1; i < args.size(); ++i) {
        if (folded(args[i]) == "json") {
            json = true;
            continue;
        }
        const auto option = optionFrom(args[i]);
        if (!option || (option->key != "sheets" && option->key != "sheet")) {
            return usage(kUsage);
        }
        auto chosen = parseSheetSelection(option->value, set);
        if (!chosen) {
            return chosen.error();
        }
        indices = std::move(*chosen);
    }
    const SheetVerbContext given = context ? context() : SheetVerbContext{};
    std::vector<Finding> findings;
    if (given.check) {
        findings = given.check(indices);
    } else {
        PreflightOptions options;
        options.sheets = indices;
        findings = checkDocumentSheets(document, std::move(options));
    }
    if (json) {
        return findingsToJson(findings);
    }
    const std::size_t checked = indices.empty() ? set.sheets.size() : indices.size();
    std::string reply = std::format("checked {}: {}", countOf(checked, "sheet", "sheets"),
                                    summaryText(summarize(findings)));
    for (const Finding& finding : findings) {
        reply += "\n" + checkReplyLine(finding);
    }
    return reply;
}

Result<std::string> sheetsVerb(Document& document, const Words& args,
                               const SheetVerbContextProvider& context)
{
    constexpr std::string_view kUsage =
        "SHEETS [LIST] | JSON [path] | SAVE path | LOAD path | CHECK [sheets=] [json] | "
        "PAGESETUP [option=value ...]";
    const std::string action = args.empty() ? "LIST" : upper(args[0]);
    if (action == "LIST") {
        if (args.size() > 1) {
            return usage(kUsage);
        }
        auto set = readable(document);
        if (!set) {
            return set.error();
        }
        std::string reply = countOf((*set)->sheets.size(), "sheet", "sheets");
        for (std::size_t i = 0; i < (*set)->sheets.size(); ++i) {
            reply += "\n" + describeSheet(**set, i);
        }
        return reply;
    }
    if (action == "JSON" || action == "SAVE") {
        if (action == "SAVE" ? args.size() != 2 : args.size() > 2) {
            return usage(kUsage);
        }
        auto set = readable(document);
        if (!set) {
            return set.error();
        }
        if (args.size() == 1) {
            return sheetSetToJson(**set);
        }
        if (const Status status = writeSheetSetFile(**set, pathFrom(args[1])); !status) {
            return status.error();
        }
        return std::format("wrote {} to {}", countOf((*set)->sheets.size(), "sheet", "sheets"),
                           "\"" + args[1] + "\"");
    }
    if (action == "CHECK" || action == "PREFLIGHT") {
        return checkVerb(document, args, context);
    }
    if (action == "PAGESETUP" || action == "SETUP") {
        return pageSetupVerb(document, args);
    }
    if (action == "LOAD") {
        if (args.size() != 2) {
            return usage(kUsage);
        }
        auto loaded = readSheetSetFile(pathFrom(args[1]));
        if (!loaded) {
            return loaded.error();
        }
        const std::size_t count = loaded->sheets.size();
        if (const Status status = document.setSheetSet(*loaded, "LOAD_SHEETS"); !status) {
            return status.error();
        }
        return std::format("loaded {} from {}", countOf(count, "sheet", "sheets"),
                           "\"" + args[1] + "\"");
    }
    return usage(kUsage);
}

// ---- SHEET ---------------------------------------------------------------------------

// A sheet option of SHEET NEW and SHEET SET, applied to `sheet`.
Status setSheetOption(Sheet& sheet, const Option& option)
{
    if (option.key == "paper") {
        auto paper = paperFrom(option.value);
        if (!paper) {
            return paper.error();
        }
        sheet.paper = *paper;
    } else if (option.key == "orientation") {
        auto landscape = landscapeFrom(option.value);
        if (!landscape) {
            return landscape.error();
        }
        sheet.landscape = *landscape;
    } else if (option.key == "frame") {
        auto frame = frameFrom(option.value);
        if (!frame) {
            return frame.error();
        }
        sheet.frame = std::move(*frame);
    } else if (option.key == "legendblock") {
        auto on = onOff(option.value, option.key);
        if (!on) {
            return on.error();
        }
        sheet.frameLegend = *on;
    } else if (option.key == "name") {
        if (option.value.empty()) {
            return makeError(ErrorCode::InvalidArgument, "a sheet needs a name", option.word);
        }
        sheet.name = unescaped(option.value);
    } else {
        return unknownOption(option, "a sheet",
                             "paper=, orientation=, frame=, legendblock= and name=");
    }
    return {};
}

// The bare words PORTRAIT and LANDSCAPE, as SHEET NEW and SHEET SET take them.
std::optional<bool> orientationWord(std::string_view word)
{
    const std::string key = folded(word);
    if (key == "portrait") {
        return false;
    }
    if (key == "landscape") {
        return true;
    }
    return std::nullopt;
}

Result<std::string> newSheet(Document& document, const Words& args)
{
    const SheetSet& set = document.sheetSet();
    Sheet sheet = blankSheet({}, {});
    Words nameWords;
    std::optional<std::size_t> at;
    bool named = false;
    for (std::size_t i = 1; i < args.size(); ++i) {
        const auto option = optionFrom(args[i]);
        if (!option) {
            if (const auto landscape = orientationWord(args[i])) {
                sheet.landscape = *landscape;
            } else {
                nameWords.push_back(args[i]);
            }
            continue;
        }
        if (option->key == "at") {
            auto position = counting(option->value, "at=");
            if (!position) {
                return position.error();
            }
            if (*position > set.sheets.size() + 1) {
                return makeError(ErrorCode::InvalidArgument,
                                 std::format("at= is 1 to {}: the set has {}", set.sheets.size() + 1,
                                             countOf(set.sheets.size(), "sheet", "sheets")),
                                 option->word);
            }
            at = *position - 1;
            continue;
        }
        if (Status status = setSheetOption(sheet, *option); !status) {
            return status.error();
        }
        named = named || option->key == "name";
    }
    if (named && !nameWords.empty()) {
        return makeError(ErrorCode::InvalidArgument, "the name is given twice", joined(nameWords, 0));
    }
    if (!named) {
        sheet.name = nameWords.empty() ? std::format("SHEET {}", set.sheets.size() + 1)
                                       : unescaped(joined(nameWords, 0));
    }
    const std::size_t position = at.value_or(set.sheets.size());
    if (Status status = addSheet(document, std::move(sheet), position); !status) {
        return status.error();
    }
    return "added " + sheetLine(document.sheetSet(), position);
}

Result<std::string> sheetField(Document& document, std::size_t index, const Words& args)
{
    if (args.size() < 3) {
        return usage("SHEET FIELD n field [value]   (\"\" clears the sheet's own value)");
    }
    const SheetSet& set = document.sheetSet();
    const auto automatic = resolveFields(set, index, fieldContextFor(document, {}));
    std::string field;
    for (const auto& [name, value] : automatic) {
        if (folded(name) == folded(args[2])) {
            field = name;
        }
    }
    if (field.empty()) {
        std::string names;
        for (const auto& [name, value] : automatic) {
            names += (names.empty() ? "" : " ") + name;
        }
        return makeError(ErrorCode::NotFound,
                         "the frame prints no field of that name; it prints " + names, args[2]);
    }
    const Sheet& sheet = set.sheets[index];
    if (args.size() == 3) {
        if (const auto own = sheet.fields.find(field); own != sheet.fields.end()) {
            return std::format("sheet {} field {}={}", index + 1, field, inQuotes(own->second));
        }
        return std::format("sheet {} field {}={} automatic", index + 1, field,
                           inQuotes(automatic.find(field)->second));
    }
    const std::string value = unescaped(joined(args, 3));
    const Status status = editSheet(
        document, index,
        [&](Sheet& edited) -> Status {
            if (value.empty()) {
                edited.fields.erase(field);
            } else {
                edited.fields.insert_or_assign(field, value);
            }
            return {};
        },
        "SET_SHEET_FIELD");
    if (!status) {
        return status.error();
    }
    if (value.empty()) {
        return std::format("sheet {} field {} cleared", index + 1, field);
    }
    return std::format("sheet {} field {}={}", index + 1, field, inQuotes(value));
}

// What `viewport` shows, as world points: the front end's (with its
// imagery, and a key plan's outlines where the painter puts each plan),
// else the drawing's (viewportContent with the set, so a key plan's are its
// live outlines).
std::vector<Point2> contentOf(const Document& document, const SheetVerbContext& given,
                              const Viewport& viewport)
{
    if (given.content) {
        return given.content(viewport);
    }
    return viewportContent(document.model(), document.sheetSet(), viewport);
}

std::string paperAdviceLine(const PaperAdvice& advice)
{
    return std::format("paper={} orientation={} frame={} fill={}", paperName(advice.paper),
                       advice.landscape ? "landscape" : "portrait",
                       advice.frame.empty() ? std::string("off") : advice.frame,
                       decimal(std::round(advice.fill * 1000.0) / 1000.0));
}

// SHEET SUGGESTPAPER n [scale=n] [apply=on]: the smallest paper that holds
// what the sheet's main plan shows at a scale (its own, or scale=), as the
// editor's Arrange > Choose Paper for This Scale works it out; apply=on puts
// the sheet on it as one step.
Result<std::string> suggestPaperVerb(Document& document, std::size_t index, const Words& args,
                                     const SheetVerbContextProvider& context)
{
    constexpr std::string_view kUsage = "SHEET SUGGESTPAPER n [scale=n] [apply=on|off]";
    auto options = optionsFrom(args, 2, kUsage);
    if (!options) {
        return options.error();
    }
    std::optional<double> scale;
    bool apply = false;
    for (const Option& option : *options) {
        if (option.key == "scale") {
            auto parsed = scaleFrom(option.value);
            if (!parsed) {
                return parsed.error();
            }
            scale = *parsed; // auto: the scale it is drawn at
        } else if (option.key == "apply") {
            auto on = onOff(option.value, option.key);
            if (!on) {
                return on.error();
            }
            apply = *on;
        } else {
            return unknownOption(option, "SHEET SUGGESTPAPER", "scale= and apply=");
        }
    }
    const SheetSet& set = document.sheetSet();
    const std::string id = mainPlanOf(set, index);
    if (id.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "the sheet has no plan or key plan to choose paper for",
                         std::to_string(index + 1));
    }
    const auto at = findViewport(set, id);
    const Viewport& plan = set.sheets[at->first].viewports[at->second];
    const SheetVerbContext given = context ? context() : SheetVerbContext{};
    const std::vector<Point2> content = contentOf(document, given, plan);
    if (content.empty()) {
        return makeError(ErrorCode::InvalidArgument, "the plan shows nothing to choose paper for", id);
    }
    const double scaleUsed = scale.value_or(drawnScale(plan, content));
    if (apply) {
        auto change = choosePaperForScale(document, id, content, scaleUsed);
        if (!change) {
            return change.error();
        }
        return std::format("sheet {} view={} scale={} {}\n{}", index + 1, id, decimal(scaleUsed),
                           paperAdviceLine(change->advice), describeSheet(document.sheetSet(), index));
    }
    // Worked out on a copy of the sheet: the same answer apply=on gives.
    Sheet trial = set.sheets[index];
    auto change = fitPaperToViewport(trial, id, content, scaleUsed);
    if (!change) {
        return change.error();
    }
    return std::format("sheet {} view={} scale={} {}", index + 1, id, decimal(scaleUsed),
                       paperAdviceLine(change->advice));
}

Result<std::string> sheetVerb(Document& document, const Words& args,
                              const SheetVerbContextProvider& context)
{
    constexpr std::string_view kUsage =
        "SHEET NEW [name] [option=value ...] | REMOVE n | MOVE n to | COPY n | RENAME n name | "
        "SET n option=value ... | FIELD n field [value] | SUGGESTPAPER n [scale=n] [apply=on]";
    if (args.empty()) {
        return usage(kUsage);
    }
    const std::string action = upper(args[0]);
    if (action == "NEW" || action == "ADD") {
        return newSheet(document, args);
    }
    if (args.size() < 2) {
        return usage(kUsage);
    }
    const SheetSet& set = document.sheetSet();
    const auto found = sheetIndexFrom(set, args[1]);
    if (!found) {
        return found.error();
    }
    const std::size_t index = *found;

    if (action == "REMOVE" || action == "DELETE") {
        if (args.size() != 2) {
            return usage("SHEET REMOVE n");
        }
        const std::string gone =
            std::format("removed sheet {} id={} name={}", index + 1, set.sheets[index].id,
                        inQuotes(set.sheets[index].name));
        if (Status status = removeSheet(document, index); !status) {
            return status.error();
        }
        return gone;
    }
    if (action == "MOVE") {
        if (args.size() != 3) {
            return usage("SHEET MOVE n to   (to: the number it becomes)");
        }
        auto to = counting(args[2], "the position to move to");
        if (!to) {
            return to.error();
        }
        if (*to > set.sheets.size()) {
            return makeError(ErrorCode::NotFound,
                             std::format("no position {}: the set has {}", *to,
                                         countOf(set.sheets.size(), "sheet", "sheets")),
                             args[2]);
        }
        const std::string id = set.sheets[index].id;
        if (Status status = moveSheet(document, index, *to - 1); !status) {
            return status.error();
        }
        return std::format("moved sheet id={} from {} to {}", id, index + 1, *to);
    }
    if (action == "COPY" || action == "DUPLICATE") {
        if (args.size() != 2) {
            return usage("SHEET COPY n");
        }
        if (Status status = duplicateSheet(document, index); !status) {
            return status.error();
        }
        return std::format("copied sheet {} to {}", index + 1,
                           sheetLine(document.sheetSet(), index + 1));
    }
    if (action == "RENAME") {
        const std::string name = unescaped(joined(args, 2));
        if (name.empty()) {
            return makeError(ErrorCode::InvalidArgument, "a sheet needs a name",
                             "SHEET RENAME n name");
        }
        const Status status = editSheet(
            document, index,
            [&name](Sheet& edited) -> Status {
                edited.name = name;
                return {};
            },
            "RENAME_SHEET");
        if (!status) {
            return status.error();
        }
        return "renamed " + sheetLine(document.sheetSet(), index);
    }
    if (action == "SET") {
        if (args.size() < 3) {
            return usage("SHEET SET n [paper=A0..A4] [orientation=landscape|portrait] "
                         "[frame=on|off] [legendblock=on|off] [name=text]");
        }
        const Status status = editSheet(
            document, index,
            [&args](Sheet& edited) -> Status {
                for (std::size_t i = 2; i < args.size(); ++i) {
                    if (const auto landscape = orientationWord(args[i])) {
                        edited.landscape = *landscape;
                        continue;
                    }
                    const auto option = optionFrom(args[i]);
                    if (!option) {
                        return makeError(ErrorCode::InvalidArgument,
                                         "expected option=value, or portrait or landscape",
                                         args[i]);
                    }
                    if (Status applied = setSheetOption(edited, *option); !applied) {
                        return applied;
                    }
                }
                return {};
            },
            "EDIT_SHEET");
        if (!status) {
            return status.error();
        }
        return "set " + sheetLine(document.sheetSet(), index);
    }
    if (action == "FIELD") {
        return sheetField(document, index, args);
    }
    if (action == "SUGGESTPAPER" || action == "PAPER") {
        return suggestPaperVerb(document, index, args, context);
    }
    return usage(kUsage);
}

// ---- VIEW ----------------------------------------------------------------------------

// file=path for an Image view: the image copied into the project's assets.
Result<std::string> imageOption(const Document& document, const Viewport& viewport,
                                const Option& option)
{
    if (viewport.kind != ViewportKind::Image) {
        return makeError(ErrorCode::InvalidArgument,
                         std::format("file= is for image views, not {}", toString(viewport.kind)),
                         option.word);
    }
    return importImageAsset(document, pathFrom(option.value), kMaximumImageBytes, "image");
}

// The options of VIEW ADD and VIEW SET applied to `viewport` on `sheet`:
// file= as `imported` (already copied), the rest by setViewportOption.
Status applyViewOptions(Viewport& viewport, const Sheet& sheet, const std::vector<Option>& options,
                        const std::optional<std::string>& imported, const entity::Model& model)
{
    for (const Option& option : options) {
        if (option.key == "file") {
            if (imported) {
                viewport.text = *imported;
            }
            continue;
        }
        if (Status status = setViewportOption(viewport, option.key, option.value, model); !status) {
            return status;
        }
    }
    return checkOnPaper(sheet, viewport);
}

bool hasOption(const std::vector<Option>& options, std::string_view key)
{
    return std::ranges::any_of(options, [key](const Option& option) { return option.key == key; });
}

Result<std::string> viewAdd(Document& document, const Words& args)
{
    constexpr std::string_view kUsage = "VIEW ADD n kind [option=value ...]";
    if (args.size() < 3) {
        return usage(kUsage);
    }
    const SheetSet& set = document.sheetSet();
    const auto found = sheetIndexFrom(set, args[1]);
    if (!found) {
        return found.error();
    }
    const std::size_t index = *found;
    auto kind = kindFrom(args[2]);
    if (!kind) {
        return kind.error();
    }
    auto options = optionsFrom(args, 3, kUsage);
    if (!options) {
        return options.error();
    }
    const entity::Model& model = document.model();
    Viewport viewport = defaultViewport(set, index, *kind, model);
    // Everything but the file first, so a refused option copies nothing.
    if (Status status = applyViewOptions(viewport, set.sheets[index], *options, {}, model);
        !status) {
        return status.error();
    }
    // Cross sections moved to another alignment are cut half way along it,
    // as they are cut half way along the first one by default.
    if (viewport.kind == ViewportKind::CrossSections && hasOption(*options, "alignment") &&
        !hasOption(*options, "stations") && !hasOption(*options, "interval")) {
        viewport.source.stations.clear();
        if (const auto middle = middleStation(model, viewport.source.alignment)) {
            viewport.source.stations = {*middle};
        }
    }
    std::optional<std::string> imported;
    for (const Option& option : *options) {
        if (option.key == "file") {
            auto name = imageOption(document, viewport, option);
            if (!name) {
                return name.error();
            }
            imported = std::move(*name);
            viewport.text = *imported;
        }
    }
    if (viewport.kind == ViewportKind::Image && viewport.text.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "an image view needs file=path (copied into the project's assets) or "
                         "text=name of a file already in them");
    }
    const Status status = editSheet(
        document, index,
        [&viewport](Sheet& edited) -> Status {
            edited.viewports.push_back(viewport);
            return {};
        },
        "ADD_VIEWPORT");
    if (!status) {
        return status.error();
    }
    return std::format("added view {} to sheet {}\n{}", viewport.id, index + 1,
                       describeViewport(viewport));
}

Result<std::string> viewSet(Document& document, const Words& args)
{
    constexpr std::string_view kUsage = "VIEW SET id option=value ...";
    if (args.size() < 3) {
        return usage(kUsage);
    }
    const SheetSet& set = document.sheetSet();
    const auto at = findViewport(set, args[1]);
    if (!at) {
        return noViewport(args[1]);
    }
    auto options = optionsFrom(args, 2, kUsage);
    if (!options) {
        return options.error();
    }
    const entity::Model& model = document.model();
    const Sheet& sheet = set.sheets[at->first];
    // Tried on a copy first, so a refused option copies no file.
    Viewport trial = sheet.viewports[at->second];
    if (Status status = applyViewOptions(trial, sheet, *options, {}, model); !status) {
        return status.error();
    }
    std::optional<std::string> imported;
    for (const Option& option : *options) {
        if (option.key == "file") {
            auto name = imageOption(document, trial, option);
            if (!name) {
                return name.error();
            }
            imported = std::move(*name);
        }
    }
    const std::size_t sheetAt = at->first;
    const std::size_t viewAt = at->second;
    const Status status = editSheet(
        document, sheetAt,
        [&](Sheet& edited) -> Status {
            return applyViewOptions(edited.viewports[viewAt], edited, *options, imported, model);
        },
        "EDIT_VIEWPORT");
    if (!status) {
        return status.error();
    }
    return describeViewport(document.sheetSet().sheets[sheetAt].viewports[viewAt]);
}

// VIEW FIT id and VIEW BESTROTATE id: a plan's rectangle fitted to what it
// shows, or the plan turned to the rotation it shows most of at, each one
// step (arrange_commands.hpp).
Result<std::string> viewFitVerb(Document& document, const Words& args,
                                const SheetVerbContextProvider& context, bool rotate)
{
    if (args.size() != 2) {
        return usage(rotate ? "VIEW BESTROTATE id" : "VIEW FIT id");
    }
    const auto at = findViewport(document.sheetSet(), args[1]);
    if (!at) {
        return noViewport(args[1]);
    }
    const Viewport viewport = document.sheetSet().sheets[at->first].viewports[at->second];
    if (!isPlanLike(viewport.kind)) {
        return makeError(ErrorCode::InvalidArgument,
                         std::format("{} is for plans and key plans, not {}",
                                     rotate ? "VIEW BESTROTATE" : "VIEW FIT", toString(viewport.kind)),
                         viewport.id);
    }
    const SheetVerbContext given = context ? context() : SheetVerbContext{};
    const std::vector<Point2> content = contentOf(document, given, viewport);
    if (content.empty()) {
        return makeError(ErrorCode::InvalidArgument, "the view shows nothing to fit to", viewport.id);
    }
    const auto described = [&] {
        return describeViewport(document.sheetSet().sheets[at->first].viewports[at->second]);
    };
    if (rotate) {
        auto fit = rotateToBestFit(document, viewport.id, content);
        if (!fit) {
            return fit.error();
        }
        return std::format("rotated {} rotation={} scale={}\n{}", viewport.id,
                           decimal(fit->rotation * math::kRadToDeg), decimal(fit->standardScale),
                           described());
    }
    if (Status status = fitViewportToContent(document, viewport.id, content); !status) {
        return status.error();
    }
    return "fitted " + viewport.id + "\n" + described();
}

Result<std::string> viewVerb(Document& document, const Words& args,
                             const SheetVerbContextProvider& context)
{
    constexpr std::string_view kUsage =
        "VIEW ADD n kind [option=value ...] | SET id option=value ... | REMOVE id | LIST [n] | "
        "FIT id | BESTROTATE id";
    if (args.empty()) {
        return usage(kUsage);
    }
    const std::string action = upper(args[0]);
    if (action == "ADD" || action == "NEW") {
        return viewAdd(document, args);
    }
    if (action == "SET") {
        return viewSet(document, args);
    }
    if (action == "FIT") {
        return viewFitVerb(document, args, context, false);
    }
    if (action == "BESTROTATE" || action == "ROTATE") {
        return viewFitVerb(document, args, context, true);
    }
    if (action == "REMOVE" || action == "DELETE") {
        if (args.size() != 2) {
            return usage("VIEW REMOVE id");
        }
        const auto at = findViewport(document.sheetSet(), args[1]);
        if (!at) {
            return noViewport(args[1]);
        }
        // The id as stored: the one typed may differ in case (VP2 is vp2).
        const std::string id = document.sheetSet().sheets[at->first].viewports[at->second].id;
        const auto viewAt = static_cast<std::ptrdiff_t>(at->second);
        const Status status = editSheet(
            document, at->first,
            [viewAt](Sheet& edited) -> Status {
                edited.viewports.erase(edited.viewports.begin() + viewAt);
                return {};
            },
            "REMOVE_VIEWPORT");
        if (!status) {
            return status.error();
        }
        return std::format("removed view {} from sheet {}", id, at->first + 1);
    }
    if (action == "LIST") {
        if (args.size() > 2) {
            return usage("VIEW LIST [n]");
        }
        auto set = readable(document);
        if (!set) {
            return set.error();
        }
        std::size_t first = 0;
        std::size_t last = (*set)->sheets.size();
        if (args.size() == 2) {
            const auto found = sheetIndexFrom(**set, args[1]);
            if (!found) {
                return found.error();
            }
            first = *found;
            last = *found + 1;
        }
        if (first == last) {
            return std::string("0 sheets");
        }
        std::string reply;
        for (std::size_t i = first; i < last; ++i) {
            const Sheet& sheet = (*set)->sheets[i];
            reply += std::format("{}sheet {} id={} views={}", reply.empty() ? "" : "\n", i + 1,
                                 sheet.id, sheet.viewports.size());
            for (const Viewport& viewport : sheet.viewports) {
                reply += "\n" + describeViewport(viewport);
            }
        }
        return reply;
    }
    return usage(kUsage);
}

// ---- TILE ----------------------------------------------------------------------------

Result<std::string> tileVerb(Document& document, const Words& args)
{
    if (args.size() < 2) {
        return usage("TILE n preset   (full cols2 rows2 quad sectionR sectionB sectionBR "
                     "sectionMap3d, or the menu's name)");
    }
    const auto found = sheetIndexFrom(document.sheetSet(), args[0]);
    if (!found) {
        return found.error();
    }
    const std::size_t index = *found;
    // The rest of the line, so a menu name needs no quotes.
    auto preset = presetFrom(joined(args, 1));
    if (!preset) {
        return preset.error();
    }
    std::size_t placed = 0;
    const Status status = editSheet(
        document, index,
        [&placed, &preset](Sheet& edited) -> Status {
            placed = tileViewports(edited, *preset);
            return {};
        },
        "TILE_VIEWPORTS");
    if (!status) {
        return status.error();
    }
    std::string reply = std::format("tiled {} on sheet {} preset={}",
                                    countOf(placed, "view", "views"), index + 1,
                                    presetId(*preset));
    for (const Viewport& viewport : document.sheetSet().sheets[index].viewports) {
        reply += "\n" + viewportSummary(viewport);
    }
    return reply;
}

// ---- ARRANGE -------------------------------------------------------------------------

// Views by their ids, all on one sheet: that sheet, and the ids as stored.
Result<std::pair<std::size_t, Words>> viewsOnOneSheet(const SheetSet& set, const Words& args,
                                                      std::size_t from)
{
    std::optional<std::size_t> sheet;
    Words ids;
    for (std::size_t i = from; i < args.size(); ++i) {
        const auto at = findViewport(set, args[i]);
        if (!at) {
            return noViewport(args[i]);
        }
        if (sheet && *sheet != at->first) {
            return makeError(ErrorCode::InvalidArgument,
                             std::format("the views are on sheets {} and {}: they are arranged a "
                                         "sheet at a time",
                                         *sheet + 1, at->first + 1),
                             args[i]);
        }
        sheet = at->first;
        ids.push_back(set.sheets[at->first].viewports[at->second].id);
    }
    if (!sheet) {
        return makeError(ErrorCode::InvalidArgument, "name the views by their ids (vp1 vp2 ...)");
    }
    return std::pair{*sheet, std::move(ids)};
}

std::string idList(const Words& ids)
{
    return joined(ids, 0, ",");
}

// ARRANGE n | ALIGN edge id ... | DISTRIBUTE across|up id ... | MATCHSCALE
// from id ...: the editor's Arrange menu, each one step
// (arrange_commands.hpp).
Result<std::string> arrangeVerb(Document& document, const Words& args,
                                const SheetVerbContextProvider& context)
{
    constexpr std::string_view kUsage =
        "ARRANGE n | ALIGN left|right|top|bottom|hcentre|vcentre id id ... | "
        "DISTRIBUTE across|up id id id ... | MATCHSCALE from id id ...";
    if (args.empty()) {
        return usage(kUsage);
    }
    const SheetSet& set = document.sheetSet();
    const std::string action = upper(args[0]);
    if (action == "ALIGN") {
        if (args.size() < 3) {
            return usage("ARRANGE ALIGN left|right|top|bottom|hcentre|vcentre id id ...");
        }
        std::optional<AlignEdge> edge = alignEdgeFrom(folded(args[1]));
        const std::string word = folded(args[1]);
        if (!edge && (word == "centre" || word == "center")) {
            edge = AlignEdge::HorizontalCentre;
        } else if (!edge && word == "middle") {
            edge = AlignEdge::VerticalCentre;
        }
        if (!edge) {
            return makeError(ErrorCode::InvalidArgument,
                             "an edge is left, right, top, bottom, hcentre or vcentre", args[1]);
        }
        auto views = viewsOnOneSheet(set, args, 2);
        if (!views) {
            return views.error();
        }
        auto moved = alignViewports(document, views->first, views->second, *edge);
        if (!moved) {
            return moved.error();
        }
        return std::format("aligned {} on sheet {} edge={} moved={}", idList(views->second),
                           views->first + 1, toString(*edge), idList(*moved));
    }
    if (action == "DISTRIBUTE") {
        if (args.size() < 3) {
            return usage("ARRANGE DISTRIBUTE across|up id id id ...");
        }
        const std::string word = folded(args[1]);
        std::optional<DistributeAxis> axis = distributeAxisFrom(word);
        if (!axis && (word == "across" || word == "x")) {
            axis = DistributeAxis::Horizontal;
        } else if (!axis && (word == "up" || word == "y")) {
            axis = DistributeAxis::Vertical;
        }
        if (!axis) {
            return makeError(ErrorCode::InvalidArgument, "distribute across or up", args[1]);
        }
        auto views = viewsOnOneSheet(set, args, 2);
        if (!views) {
            return views.error();
        }
        auto moved = distributeViewports(document, views->first, views->second, *axis);
        if (!moved) {
            return moved.error();
        }
        return std::format("distributed {} on sheet {} axis={} moved={}", idList(views->second),
                           views->first + 1, toString(*axis), idList(*moved));
    }
    if (action == "MATCHSCALE" || action == "MATCH") {
        if (args.size() < 3) {
            return usage("ARRANGE MATCHSCALE from id id ...   (from: the view whose scale the "
                         "others take)");
        }
        const auto from = findViewport(set, args[1]);
        if (!from) {
            return noViewport(args[1]);
        }
        const Viewport& source = set.sheets[from->first].viewports[from->second];
        Words ids;
        for (std::size_t i = 2; i < args.size(); ++i) {
            const auto at = findViewport(set, args[i]);
            if (!at) {
                return noViewport(args[i]);
            }
            ids.push_back(set.sheets[at->first].viewports[at->second].id);
        }
        // An automatic plan is matched at the scale it is drawn at, with what
        // the front end's plan shows.
        std::optional<double> scale;
        if (source.autoScale && isPlanLike(source.kind)) {
            const SheetVerbContext given = context ? context() : SheetVerbContext{};
            scale = drawnScale(source, contentOf(document, given, source));
        }
        const std::string fromId = source.id;
        auto changed = matchScale(document, ids, fromId, scale);
        if (!changed) {
            return changed.error();
        }
        return std::format("matched {} to {} scale={} changed={}", idList(ids), fromId,
                           decimal(scale.value_or(source.scale)), idList(*changed));
    }
    if (args.size() != 1) {
        return usage(kUsage);
    }
    const auto found = sheetIndexFrom(set, args[0]);
    if (!found) {
        return found.error();
    }
    auto arranged = autoArrangeSheet(document, *found);
    if (!arranged) {
        return arranged.error();
    }
    return std::format("arranged sheet {} moved={} overlapping={} unplaced={} mainshrunk={}\n{}",
                       *found + 1, idList(arranged->moved), idList(arranged->overlapping),
                       idList(arranged->unplaced), onOffText(arranged->mainShrunk),
                       describeSheet(document.sheetSet(), *found));
}

// ---- GENERATE ------------------------------------------------------------------------

// What GENERATE was asked, parsed; nullopt is "not given".
struct GenerateOptions {
    SheetTemplate paper;
    std::optional<std::optional<double>> scale; // given: a denominator, or auto (nullopt)
    std::string alignment;
    std::optional<Box2> area;
    std::optional<double> overlap;
    std::optional<double> interval;
    std::optional<double> halfWidth;
    std::optional<double> from;
    std::optional<double> to;
    std::optional<double> exaggeration; // 0: auto
    std::vector<double> stations;
    std::optional<std::size_t> rows;
    std::optional<std::size_t> columns;
    std::optional<bool> keyPlan;
    std::optional<bool> model3d;
    std::optional<bool> legend;
    bool replace = false;
};

// The options each kind of GENERATE takes, beyond replace=.
std::string_view generateKeys(std::string_view kind)
{
    if (kind == "fit") {
        return "paper orientation frame area alignment scale model3d legend interval halfwidth";
    }
    if (kind == "grid") {
        return "paper orientation frame area scale overlap keyplan";
    }
    if (kind == "strips") {
        return "paper orientation frame alignment scale overlap from to keyplan";
    }
    if (kind == "profile") {
        return "paper orientation frame alignment scale interval halfwidth model3d legend";
    }
    if (kind == "sections") {
        return "paper orientation frame alignment scale interval stations halfwidth rows columns ve";
    }
    if (kind == "frames") {
        return "frame";
    }
    if (kind == "register") {
        return "paper orientation frame";
    }
    return {};
}

bool takesKey(std::string_view keys, std::string_view key)
{
    std::size_t start = 0;
    while (start < keys.size()) {
        const std::size_t space = keys.find(' ', start);
        const std::string_view word =
            keys.substr(start, space == std::string_view::npos ? std::string_view::npos
                                                               : space - start);
        if (word == key) {
            return true;
        }
        if (space == std::string_view::npos) {
            break;
        }
        start = space + 1;
    }
    return false;
}

Result<GenerateOptions> generateOptions(std::string_view kind, const Words& args)
{
    const std::string_view keys = generateKeys(kind);
    GenerateOptions o;
    const auto store = [](auto& slot, auto parsed) -> Status {
        if (!parsed) {
            return parsed.error();
        }
        slot = *parsed;
        return {};
    };
    for (std::size_t i = 1; i < args.size(); ++i) {
        const auto landscape = orientationWord(args[i]);
        if (landscape && takesKey(keys, "orientation")) {
            o.paper.landscape = *landscape;
            continue;
        }
        const auto option = optionFrom(args[i]);
        if (!option) {
            return makeError(ErrorCode::InvalidArgument,
                             std::format("expected option=value; GENERATE {} takes {} replace",
                                         upper(kind), keys),
                             args[i]);
        }
        const std::string& key = option->key;
        const std::string& value = option->value;
        if (key == "replace") {
            if (Status s = store(o.replace, onOff(value, key)); !s) {
                return s.error();
            }
            continue;
        }
        if (!takesKey(keys, key)) {
            return makeError(ErrorCode::InvalidArgument,
                             std::format("GENERATE {} takes no option {}=; it takes {} replace",
                                         upper(kind), key, keys),
                             option->word);
        }
        Status status;
        if (key == "paper") {
            status = store(o.paper.paper, paperFrom(value));
        } else if (key == "orientation") {
            status = store(o.paper.landscape, landscapeFrom(value));
        } else if (key == "frame") {
            status = store(o.paper.frame, frameFrom(value));
        } else if (key == "scale") {
            auto scale = scaleFrom(value);
            if (!scale) {
                return scale.error();
            }
            o.scale = *scale;
        } else if (key == "alignment") {
            o.alignment = value;
        } else if (key == "area") {
            status = store(o.area, boxFrom(value, "area="));
        } else if (key == "overlap") {
            status = store(o.overlap, notNegative(value, "overlap="));
        } else if (key == "interval") {
            status = store(o.interval, positive(value, "interval="));
        } else if (key == "halfwidth") {
            status = store(o.halfWidth, positive(value, "halfwidth="));
        } else if (key == "from") {
            status = store(o.from, number(value, "from="));
        } else if (key == "to") {
            status = store(o.to, number(value, "to="));
        } else if (key == "ve") {
            if (folded(value) == "auto") {
                o.exaggeration = 0.0;
            } else {
                status = store(o.exaggeration, positive(value, "ve="));
            }
        } else if (key == "stations") {
            status = store(o.stations, numberList(value, "stations="));
        } else if (key == "rows") {
            status = store(o.rows, counting(value, "rows="));
        } else if (key == "columns") {
            status = store(o.columns, counting(value, "columns="));
        } else if (key == "keyplan") {
            status = store(o.keyPlan, onOff(value, key));
        } else if (key == "model3d") {
            status = store(o.model3d, onOff(value, key));
        } else if (key == "legend") {
            status = store(o.legend, onOff(value, key));
        }
        if (!status) {
            return status.error();
        }
    }
    return o;
}

Result<geometry::SolvedAlignment> solvedAlignment(const entity::Model& model,
                                                  const std::string& name)
{
    const entity::Alignment* alignment = model.alignments.find(name);
    if (alignment == nullptr) {
        return makeError(ErrorCode::NotFound, "the drawing has no alignment of that name", name);
    }
    return geometry::solveAlignment(alignment->horizontal);
}

// What FIT and GRID cover: area=, else the front end's drawing extent, else
// the drawing's entities.
Result<Box2> generateArea(const GenerateOptions& o, const entity::Model& model,
                          const SheetVerbContextProvider& context)
{
    if (o.area) {
        return *o.area;
    }
    Box2 extent;
    if (context) {
        if (const auto given = context().drawingExtent) {
            extent = *given;
        }
    }
    if (extent.empty()) {
        extent = drawnExtent(model, LayerOverrides{});
    }
    if (extent.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "the drawing is empty, so there is nothing to lay out; draw something or "
                         "give area=x0,y0,x1,y1");
    }
    return extent;
}

Result<std::vector<Sheet>> generateSheets(std::string_view kind, const GenerateOptions& o,
                                          const entity::Model& model,
                                          const SheetVerbContextProvider& context,
                                          std::vector<std::string>& skipped)
{
    const double automatic = 0.0;
    if (kind == "fit") {
        LayoutRequest request;
        request.paper = o.paper;
        request.scale = o.scale ? o.scale->value_or(automatic) : automatic;
        request.model3d = o.model3d.value_or(false);
        request.legend = o.legend.value_or(false);
        if (!o.alignment.empty()) {
            if (o.area) {
                return makeError(ErrorCode::InvalidArgument,
                                 "GENERATE FIT lays out area= or alignment=, not both");
            }
            auto name = alignmentNamed(model, o.alignment);
            if (!name) {
                return name.error();
            }
            request.alignment = *name;
            request.planAlongAlignment = true;
        } else {
            auto area = generateArea(o, model, context);
            if (!area) {
                return area.error();
            }
            request.planArea = *area;
        }
        if (o.interval) {
            auto name = alignmentFor(model, o.alignment);
            if (!name) {
                return name.error();
            }
            request.alignment = *name;
            request.crossSectionInterval = *o.interval;
            request.crossSectionHalfWidth = o.halfWidth.value_or(request.crossSectionHalfWidth);
        }
        return smartLayout(model, request);
    }
    if (kind == "grid") {
        if (o.scale && !o.scale->has_value()) {
            return makeError(ErrorCode::InvalidArgument,
                             "GENERATE GRID needs a scale: tiles are cut at a fixed one (500 when "
                             "none is given)");
        }
        auto area = generateArea(o, model, context);
        if (!area) {
            return area.error();
        }
        GridRequest request;
        request.area = *area;
        request.paper = o.paper;
        request.scale = o.scale ? **o.scale : request.scale;
        request.overlapM = o.overlap.value_or(request.overlapM);
        request.keyPlan = o.keyPlan.value_or(request.keyPlan);
        return gridSheets(request);
    }
    if (kind == "strips" || kind == "profile") {
        auto name = alignmentFor(model, o.alignment);
        if (!name) {
            return name.error();
        }
        const bool fixed = o.scale && o.scale->has_value();
        if (kind == "strips" && fixed) {
            auto solved = solvedAlignment(model, *name);
            if (!solved) {
                return solved.error();
            }
            StripRequest request;
            request.paper = o.paper;
            request.scale = **o.scale;
            request.overlapM = o.overlap.value_or(request.overlapM);
            request.fromChainage = o.from;
            request.toChainage = o.to;
            request.keyPlan = o.keyPlan.value_or(request.keyPlan);
            return stripSheets(*solved, *name, request);
        }
        if (kind == "strips" && (o.overlap || o.from || o.to || o.keyPlan)) {
            return makeError(ErrorCode::InvalidArgument,
                             "overlap=, from=, to= and keyplan= need a fixed scale: at scale=auto "
                             "the whole alignment is fitted on one sheet");
        }
        LayoutRequest request;
        request.paper = o.paper;
        request.alignment = *name;
        request.planAlongAlignment = true;
        request.longSection = kind == "profile";
        request.scale = fixed ? **o.scale : automatic;
        request.model3d = o.model3d.value_or(false);
        request.legend = o.legend.value_or(false);
        request.crossSectionInterval = o.interval.value_or(0.0);
        request.crossSectionHalfWidth = o.halfWidth.value_or(request.crossSectionHalfWidth);
        return smartLayout(model, request);
    }
    if (kind == "sections") {
        if (o.interval && !o.stations.empty()) {
            return makeError(ErrorCode::InvalidArgument,
                             "GENERATE SECTIONS cuts every interval= or at stations=, not both");
        }
        auto name = alignmentFor(model, o.alignment);
        if (!name) {
            return name.error();
        }
        auto solved = solvedAlignment(model, *name);
        if (!solved) {
            return solved.error();
        }
        CrossSectionRequest request;
        request.paper = o.paper;
        request.interval = o.interval.value_or(request.interval);
        request.stations = o.stations;
        request.halfWidth = o.halfWidth.value_or(request.halfWidth);
        request.rows = o.rows.value_or(request.rows);
        request.columns = o.columns.value_or(request.columns);
        request.scale = o.scale ? o.scale->value_or(automatic) : automatic;
        request.exaggeration = o.exaggeration.value_or(automatic);
        if (context) {
            request.surfaces = context().surfaces;
        }
        return crossSectionSheets(*solved, *name, request);
    }
    // frames
    auto sheets = sheetsFromPlotFrames(model, o.paper, &skipped);
    if (sheets && sheets->empty()) {
        std::string why = "the drawing has no plot frame that can be read";
        for (const std::string& reason : skipped) {
            why += "\n  " + reason;
        }
        return makeError(ErrorCode::NotFound, why);
    }
    return sheets;
}

Result<std::string> generateVerb(Document& document, const Words& args,
                                 const SheetVerbContextProvider& context)
{
    if (args.empty()) {
        return usage("GENERATE fit|grid|strips|profile|sections|frames [option=value ...]   "
                     "(HELP SHEETS lists the options)");
    }
    std::string kind = folded(args[0]);
    if (kind == "tiles") {
        kind = "grid";
    } else if (kind == "strip") {
        kind = "strips";
    } else if (kind == "section" || kind == "crosssections") {
        kind = "sections";
    } else if (kind == "planprofile") {
        kind = "profile";
    } else if (kind == "plotframes") {
        kind = "frames";
    } else if (kind == "drawingregister" || kind == "sheetindex" || kind == "index") {
        kind = "register";
    }
    if (generateKeys(kind).empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "GENERATE makes fit, grid, strips, profile, sections, frames or register",
                         args[0]);
    }
    auto options = generateOptions(kind, args);
    if (!options) {
        return options.error();
    }
    if (kind == "register") {
        // A cover sheet, first in the set: the drawing register and the
        // revision table (tables.hpp). It adds to the set; replacing every
        // sheet with a register of none would list nothing.
        if (options->replace) {
            return makeError(ErrorCode::InvalidArgument,
                             "GENERATE register puts a cover first; it replaces no sheets",
                             "replace=on");
        }
        auto id = addRegisterSheet(document, options->paper);
        if (!id) {
            return id.error();
        }
        const SheetSet& now = document.sheetSet();
        const auto at = sheetIndex(now, *id);
        return std::format("generated 1 sheet: {} (the drawing register)\n{}",
                           at.value_or(0) + 1, describeSheet(now, at.value_or(0)));
    }
    std::vector<std::string> skipped;
    auto sheets = generateSheets(kind, *options, document.model(), context, skipped);
    if (!sheets) {
        return sheets.error();
    }
    if (sheets->empty()) {
        return makeError(ErrorCode::InvalidArgument, "nothing to lay out", args[0]);
    }
    const std::size_t count = sheets->size();
    const std::size_t first = options->replace ? 0 : document.sheetSet().sheets.size();
    Status status;
    if (options->replace) {
        status = editSheetSet(
            document,
            [&sheets](SheetSet& set) -> Status {
                set.sheets.clear();
                prepareForAppend(set, *sheets);
                set.sheets = std::move(*sheets);
                return {};
            },
            "GENERATE_SHEETS");
    } else {
        status = addSheets(document, std::move(*sheets), "GENERATE_SHEETS");
    }
    if (!status) {
        return status.error();
    }
    const SheetSet& set = document.sheetSet();
    std::string reply =
        count == 1 ? std::format("generated 1 sheet: {}", first + 1)
                   : std::format("generated {} sheets: {} to {}", count, first + 1, first + count);
    if (options->replace) {
        reply += " (the set was replaced)";
    }
    for (std::size_t i = first; i < first + count && i < set.sheets.size(); ++i) {
        reply += "\n" + describeSheet(set, i);
    }
    for (const std::string& reason : skipped) {
        reply += "\nskipped " + reason;
    }
    return reply;
}

// ---- TITLEBLOCK ----------------------------------------------------------------------

Result<std::string> titleBlockList(const Document& document)
{
    auto set = readable(document);
    if (!set) {
        return set.error();
    }
    std::string reply;
    for (const std::string_view field : titleBlockFields()) {
        reply += std::format("{}{}={}", reply.empty() ? "" : "\n", field,
                             inQuotes(titleBlockValue(**set, field).value()));
    }
    reply += "\nlogo=" + inQuotes((*set)->defaults.logoAsset);
    reply += "\n" + countOf((*set)->revisions.size(), "revision", "revisions");
    for (const Revision& revision : (*set)->revisions) {
        reply += "\n" + revisionLine(revision);
    }
    return reply;
}

Result<std::string> revisionVerb(Document& document, const Words& args)
{
    constexpr std::string_view kUsage =
        "TITLEBLOCK REVISION [LIST] | REVISION ADD code date description [by] | "
        "REVISION REMOVE code";
    const std::string action = args.size() < 2 ? "LIST" : upper(args[1]);
    if (action == "LIST") {
        if (args.size() > 2) {
            return usage(kUsage);
        }
        auto set = readable(document);
        if (!set) {
            return set.error();
        }
        std::string reply = countOf((*set)->revisions.size(), "revision", "revisions");
        for (const Revision& revision : (*set)->revisions) {
            reply += "\n" + revisionLine(revision);
        }
        return reply;
    }
    if (action == "ADD") {
        if (args.size() < 5 || args.size() > 6) {
            return usage("TITLEBLOCK REVISION ADD code date \"description\" [by]");
        }
        Revision revision{args[2], args[3], unescaped(args[4]),
                          args.size() == 6 ? args[5] : std::string{}};
        if (revision.code.empty()) {
            return makeError(ErrorCode::InvalidArgument, "a revision needs a code");
        }
        const Status status = editSheetSet(
            document,
            [&revision](SheetSet& set) -> Status {
                for (const Revision& existing : set.revisions) {
                    if (existing.code == revision.code) {
                        return makeError(ErrorCode::AlreadyExists,
                                         "there is a revision with that code already; REVISION "
                                         "REMOVE it first",
                                         revision.code);
                    }
                }
                set.revisions.push_back(revision);
                return {};
            },
            "ADD_REVISION");
        if (!status) {
            return status.error();
        }
        return "added " + revisionLine(revision);
    }
    if (action == "REMOVE" || action == "DELETE") {
        if (args.size() != 3) {
            return usage("TITLEBLOCK REVISION REMOVE code");
        }
        const std::string& code = args[2];
        const Status status = editSheetSet(
            document,
            [&code](SheetSet& set) -> Status {
                if (std::erase_if(set.revisions,
                                  [&code](const Revision& r) { return r.code == code; }) == 0) {
                    return makeError(ErrorCode::NotFound, "no revision with that code", code);
                }
                return {};
            },
            "REMOVE_REVISION");
        if (!status) {
            return status.error();
        }
        return "removed revision code=" + inQuotes(code);
    }
    return usage(kUsage);
}

Result<std::string> titleBlockVerb(Document& document, const Words& args)
{
    if (args.empty() || (args.size() == 1 && upper(args[0]) == "LIST")) {
        return titleBlockList(document);
    }
    const std::string action = upper(args[0]);
    if (action == "REVISION" || action == "REVISIONS") {
        return revisionVerb(document, args);
    }
    if (action == "LOGO") {
        if (args.size() != 2) {
            return usage("TITLEBLOCK LOGO path   (\"\" removes the logo)");
        }
        if (args[1].empty()) {
            const Status status = editSheetSet(
                document,
                [](SheetSet& set) -> Status {
                    set.defaults.logoAsset.clear();
                    return {};
                },
                "SET_LOGO");
            if (!status) {
                return status.error();
            }
            return std::string("logo=\"\"");
        }
        auto name = importLogo(document, pathFrom(args[1]));
        if (!name) {
            return name.error();
        }
        return "logo=" + inQuotes(*name);
    }
    const auto field = titleFieldFrom(args[0]);
    if (!field) {
        return unknownTitleField(args[0]);
    }
    if (args.size() == 1) {
        return std::format("{}={}", *field,
                           inQuotes(titleBlockValue(document.sheetSet(), *field).value()));
    }
    const std::string value = unescaped(joined(args, 1));
    const Status status = editSheetSet(
        document,
        [&](SheetSet& set) -> Status { return setTitleBlockValue(set, *field, value); },
        "EDIT_TITLE_BLOCK");
    if (!status) {
        return status.error();
    }
    return std::format("{}={}", *field, inQuotes(titleBlockValue(document.sheetSet(), *field).value()));
}

} // namespace

// ---- the public pieces ---------------------------------------------------------------

bool isSheetVerb(std::string_view verb)
{
    const std::string word = upper(verb);
    return word == "SHEETS" || word == "SHEET" || word == "VIEW" || word == "VIEWPORT" ||
           word == "TILE" || word == "ARRANGE" || word == "GENERATE" || word == "TITLEBLOCK";
}

Result<std::string> runSheetVerb(Document& document, const std::vector<std::string>& tokens,
                                 const SheetVerbContextProvider& context)
{
    if (tokens.empty() || !isSheetVerb(tokens.front())) {
        return makeError(ErrorCode::ParseFailure, "not a sheet command; type HELP SHEETS",
                         tokens.empty() ? std::string{} : tokens.front());
    }
    const std::string verb = upper(tokens.front());
    const Words args(tokens.begin() + 1, tokens.end());
    // Stored sheets that cannot be read are not an empty set: an edit made
    // on the empty set the document falls back to would replace them all as
    // one step. So every verb is refused while they cannot be read, but
    // SHEETS LOAD, which replaces them on purpose (and can be undone).
    const bool loads = verb == "SHEETS" && !args.empty() && upper(args.front()) == "LOAD";
    if (!loads) {
        if (auto set = readable(document); !set) {
            return set.error();
        }
    }
    if (verb == "SHEETS") {
        return sheetsVerb(document, args, context);
    }
    if (verb == "SHEET") {
        return sheetVerb(document, args, context);
    }
    if (verb == "VIEW" || verb == "VIEWPORT") {
        return viewVerb(document, args, context);
    }
    if (verb == "TILE") {
        return tileVerb(document, args);
    }
    if (verb == "ARRANGE") {
        return arrangeVerb(document, args, context);
    }
    if (verb == "GENERATE") {
        return generateVerb(document, args, context);
    }
    return titleBlockVerb(document, args);
}

std::string sheetVerbHelp()
{
    return R"(Sheets on the command line (docs/plotting.md). A sheet is its number (1, 2 ...) or its
id (s1 ...); a view is its id (vp1 ...). Options are key=value, a switch on|off, and "\n" in a
text is a line break. Every edit is one UNDO step. A reply is one fact per line, in the form the
options take.

SHEETS [LIST]                   every sheet and its views
SHEETS JSON [path]              the set as JSON, printed or written to a file
SHEETS SAVE path | LOAD path    write the set to a JSON file | replace the set from one
SHEETS CHECK [sheets=1,3-5] [json]   the preflight checks: a summary, then a finding a line,
                                "severity code sheet view message=... fix=..." (- for none)
SHEETS PAGESETUP [style=colour|grey|mono] [lineweight=f] [dpi=n] [pattern=text]
                 [filepersheet=on|off]   how the set plots; no option prints it
SHEET NEW [name] [paper=A3] [portrait|landscape] [frame=on|off] [legendblock=on|off] [at=n]
SHEET REMOVE n | MOVE n to | COPY n | RENAME n name
SHEET SET n [paper=A0..A4] [orientation=landscape|portrait] [frame=on|off] [legendblock=on|off]
          [name=text]
SHEET FIELD n field [value]     the sheet's own value for a field its frame prints, such as
                                sheet_number or scale; "" clears it, no value prints it
SHEET SUGGESTPAPER n [scale=n|auto] [apply=on]   the smallest paper for the sheet's main plan
                                at a scale; apply=on puts the sheet on it
VIEW ADD n kind [option=value ...]   kinds: plan key_plan long_section cross_sections model_3d
                                     legend notes image sheet_index (register) revisions
VIEW SET id option=value ... | VIEW REMOVE id | VIEW LIST [n]
VIEW FIT id                     a plan's rectangle fitted to what it shows
VIEW BESTROTATE id              a plan turned to show most, and scaled to fit
  options  rect=x0,y0,x1,y1 (paper mm)  scale=500|1:500|auto (plans and sections)
           centre=x,y|auto  rotation=deg  alignment=name  from=ch  to=ch  stations=a,b,c
           interval=m  halfwidth=m  ve=n  tilt=deg  north=on|off  scalebar=on|off
           grid=none|ticks|crosses|lines  gridinterval=m|auto (plans and key plans)
           legend=this_sheet|whole_set|whole_drawing (legends)  revisions=n|all (revision
           tables)  locked=on|off  title=text  text=text  hide=layer  show=layer
           hidden=layer,layer  file=path (an image view's picture, copied into the project)
TILE n preset                   full cols2 rows2 quad sectionR sectionB sectionBR sectionMap3d,
                                or the menu's name (Main and panel right)
ARRANGE n                       no two views overlapping, the main view first and largest
ARRANGE ALIGN left|right|top|bottom|hcentre|vcentre id id ...
ARRANGE DISTRIBUTE across|up id id id ...   equal gaps, the first and last staying
ARRANGE MATCHSCALE from id id ...   the views take the scale view `from` is drawn at
GENERATE kind [option=value ...] [replace=on]   adds the sheets, or replaces every sheet
  fit       the drawing, area=x0,y0,x1,y1 or alignment=name on as few sheets as it takes:
            scale=auto|n model3d=on legend=on interval=m halfwidth=m
  grid      tiles over the drawing or area=: scale=500 overlap=m keyplan=on|off
  strips    plans along alignment=: scale=auto|n; at a fixed scale overlap=m from=ch to=ch
            keyplan=on|off
  profile   plan and profile along alignment=: scale=auto|n interval=m halfwidth=m
  sections  cross sections of alignment=: interval=20|stations=a,b,c halfwidth=20 rows=4
            columns=2 scale=auto|n ve=auto|n
  frames    a sheet per imported plot frame: frame=on|off
  register  a cover sheet put first: the drawing register and the revision table
  and, but for frames, paper=A0..A4 portrait|landscape frame=on|off; alignment= may be left
  out when the drawing has one alignment
TITLEBLOCK [LIST]               the shared title-block values, the logo and the revisions
TITLEBLOCK field [value]        organisation project1..project4 client setnumber numbering
                                coordsys datum model notes, and <role>name <role>date for the
                                roles locator surveyor compiler reviewer approver
TITLEBLOCK REVISION [LIST]      the revisions
TITLEBLOCK REVISION ADD code date "description" [by] | TITLEBLOCK REVISION REMOVE code
TITLEBLOCK LOGO path            the logo, copied into the project ("" removes it)
PLOTSHEETS [path] [format=pdf|pdfs|png|tiff] [style=colour|grey|mono] [sheets=1,3-5] [dpi=n]
           [lineweight=f] [folder=path] [pattern=text]   plots in the desktop application, the
                                page setup filling in what is not given; a file a line after.
                                Headless: katana project --plot-sheets out.pdf)";
}

std::string checkReplyLine(const Finding& finding)
{
    std::string line = std::format(
        "{} {} {} {} message={}", toString(finding.severity), finding.code,
        finding.sheetIndex ? std::to_string(*finding.sheetIndex + 1) : std::string("-"),
        finding.viewportId.empty() ? std::string("-") : finding.viewportId, inQuotes(finding.message));
    if (!finding.fix.empty()) {
        line += " fix=" + inQuotes(finding.fix);
    }
    if (!finding.subject.empty()) {
        line += " subject=" + inQuotes(finding.subject);
    }
    return line;
}

Result<std::size_t> sheetIndexFrom(const SheetSet& set, std::string_view text)
{
    const bool digits =
        !text.empty() && std::ranges::all_of(text, [](unsigned char c) { return std::isdigit(c) != 0; });
    if (digits) {
        std::size_t number = 0;
        const auto [parsed, error] = std::from_chars(text.data(), text.data() + text.size(), number);
        if (error != std::errc{} || number == 0 || number > set.sheets.size()) {
            return makeError(ErrorCode::NotFound,
                             std::format("no sheet {}: the set has {}", text,
                                         countOf(set.sheets.size(), "sheet", "sheets")),
                             std::string(text));
        }
        return number - 1;
    }
    if (text.empty()) {
        return makeError(ErrorCode::ParseFailure,
                         "a sheet is its number (1, 2 ...) or its id (s1, s2 ...)");
    }
    for (std::size_t i = 0; i < set.sheets.size(); ++i) {
        if (set.sheets[i].id == text) {
            return i;
        }
    }
    const std::string wanted = upper(text);
    for (std::size_t i = 0; i < set.sheets.size(); ++i) {
        if (upper(set.sheets[i].id) == wanted) {
            return i;
        }
    }
    if (text.front() == 's' || text.front() == 'S') {
        return makeError(ErrorCode::NotFound, "no sheet with that id; SHEETS LIST shows them",
                         std::string(text));
    }
    return makeError(ErrorCode::ParseFailure,
                     "a sheet is its number (1, 2 ...) or its id (s1, s2 ...)", std::string(text));
}

Result<PlotSheetsRequest> parsePlotSheets(const SheetSet& set, const std::vector<std::string>& args)
{
    constexpr std::string_view kUsage =
        "PLOTSHEETS [path] [format=pdf|pdfs|png|tiff] [style=colour|grey|mono] [sheets=1,3-5] "
        "[dpi=n] [lineweight=f] [folder=path] [pattern=text]";
    PlotSheetsRequest request;
    std::optional<std::filesystem::path> folder;
    for (std::size_t i = 0; i < args.size(); ++i) {
        const auto option = optionFrom(args[i]);
        if (!option) {
            if (i != 0 || args[i].empty()) {
                return makeError(ErrorCode::InvalidArgument,
                                 std::format("expected option=value; usage: {}", kUsage), args[i]);
            }
            request.path = pathFrom(args[i]);
            continue;
        }
        const std::string& key = option->key;
        if (key == "sheets" || key == "sheet") {
            // Read as every plot reads it, so a refusal says what it says there.
            if (auto chosen = parseSheetSelection(option->value, set); !chosen) {
                return chosen.error();
            }
            request.sheets = option->value;
        } else if (key == "format") {
            const std::string format = folded(option->value);
            if (format != "pdf" && format != "pdfs" && format != "png" && format != "tiff" &&
                format != "tif") {
                return makeError(ErrorCode::InvalidArgument, "format= is pdf, pdfs, png or tiff",
                                 option->word);
            }
            request.format = format == "tif" ? std::string("tiff") : format;
        } else if (key == "style" || key == "colour" || key == "color" || key == "mode") {
            auto mode = styleFrom(option->value);
            if (!mode) {
                return mode.error();
            }
            request.colourMode = *mode;
        } else if (key == "dpi") {
            auto dpi = number(option->value, "dpi=");
            if (!dpi) {
                return dpi.error();
            }
            if (*dpi < kMinimumPlotDpi || *dpi > kMaximumPlotDpi) {
                return makeError(ErrorCode::InvalidArgument,
                                 std::format("dpi= is {} to {}", kMinimumPlotDpi, kMaximumPlotDpi),
                                 option->word);
            }
            request.dpi = *dpi;
        } else if (key == "lineweight" || key == "lineweightscale") {
            auto factor = number(option->value, "lineweight=");
            if (!factor) {
                return factor.error();
            }
            if (*factor < kMinimumLineWeightScale || *factor > kMaximumLineWeightScale) {
                return makeError(ErrorCode::InvalidArgument,
                                 std::format("lineweight= is {} to {}", kMinimumLineWeightScale,
                                             kMaximumLineWeightScale),
                                 option->word);
            }
            request.lineWeightScale = *factor;
        } else if (key == "folder") {
            if (option->value.empty()) {
                return makeError(ErrorCode::InvalidArgument, "folder= names a folder", option->word);
            }
            folder = pathFrom(option->value);
        } else if (key == "pattern" || key == "names") {
            const std::string pattern = unescaped(option->value);
            if (Status valid = validateFileNamePattern(pattern); !valid) {
                return valid.error();
            }
            request.fileNamePattern = pattern;
        } else {
            return unknownOption(*option, "PLOTSHEETS",
                                 "format=, style=, sheets=, dpi=, lineweight=, folder= and pattern=");
        }
    }
    if (folder) {
        if (!request.path.empty()) {
            return makeError(ErrorCode::InvalidArgument,
                             "give the PDF's path or folder=, not both: folder= is where a file a "
                             "sheet goes",
                             pathText(request.path));
        }
        if (request.format == "pdf") {
            return makeError(ErrorCode::InvalidArgument,
                             "format=pdf is one file: give its path rather than folder=");
        }
        request.path = *folder;
        if (!request.format) {
            request.format = "pdfs";
        }
    }
    if (request.path.empty()) {
        return usage(kUsage);
    }
    return request;
}

Status setViewportOption(Viewport& viewport, std::string_view key, std::string_view value,
                         const entity::Model& model)
{
    const Option option{folded(key), std::string(value), std::string(key) + "=" + std::string(value)};
    const std::string& name = option.key;
    // Changed on a copy, so a refusal leaves the viewport as it was.
    Viewport edited = viewport;
    const auto fail = [](const Error& error) -> Status { return error; };
    if (name == "rect") {
        auto rect = boxFrom(value, "rect=");
        if (!rect) {
            return fail(rect.error());
        }
        edited.rect = *rect;
    } else if (name == "scale") {
        if (Status s = onlyFor(viewport, option, drawnToScale(viewport.kind),
                               "plan, key_plan, long_section and cross_sections");
            !s) {
            return s;
        }
        auto scale = scaleFrom(value);
        if (!scale) {
            return fail(scale.error());
        }
        edited.autoScale = !scale->has_value();
        if (scale->has_value()) {
            edited.scale = **scale;
        }
    } else if (name == "centre" || name == "center") {
        if (Status s = onlyFor(viewport, option, drawnToScale(viewport.kind),
                               "plan, key_plan, long_section and cross_sections");
            !s) {
            return s;
        }
        if (folded(value) == "auto") {
            edited.autoCentre = true;
        } else {
            auto centre = pointFrom(value, "centre=");
            if (!centre) {
                return fail(centre.error());
            }
            edited.centre = *centre;
            edited.autoCentre = false;
        }
    } else if (name == "rotation") {
        if (Status s = onlyFor(viewport, option,
                               isPlanLike(viewport.kind) || viewport.kind == ViewportKind::Model3D,
                               "plan, key_plan and model_3d");
            !s) {
            return s;
        }
        auto degrees = number(value, "rotation=");
        if (!degrees) {
            return fail(degrees.error());
        }
        edited.rotation = *degrees * math::kDegToRad;
    } else if (name == "alignment") {
        if (Status s = onlyFor(viewport, option,
                               isPlanLike(viewport.kind) || isSection(viewport.kind),
                               "plan, key_plan, long_section and cross_sections");
            !s) {
            return s;
        }
        if (value.empty()) {
            edited.source.alignment.clear();
        } else {
            auto alignment = alignmentNamed(model, value);
            if (!alignment) {
                return fail(alignment.error());
            }
            edited.source.alignment = std::move(*alignment);
        }
    } else if (name == "from" || name == "to") {
        if (Status s = onlyFor(viewport, option,
                               isPlanLike(viewport.kind) || isSection(viewport.kind),
                               "plan, key_plan, long_section and cross_sections");
            !s) {
            return s;
        }
        auto chainage = number(value, name + "=");
        if (!chainage) {
            return fail(chainage.error());
        }
        (name == "from" ? edited.source.chainageFrom : edited.source.chainageTo) = *chainage;
    } else if (name == "stations" || name == "interval" || name == "halfwidth") {
        if (Status s = onlyFor(viewport, option, viewport.kind == ViewportKind::CrossSections,
                               "cross_sections");
            !s) {
            return s;
        }
        if (name == "stations") {
            auto stations = numberList(value, "stations=");
            if (!stations) {
                return fail(stations.error());
            }
            edited.source.stations = std::move(*stations);
        } else if (name == "interval") {
            auto interval = notNegative(value, "interval=");
            if (!interval) {
                return fail(interval.error());
            }
            edited.source.sectionInterval = *interval;
        } else {
            auto halfWidth = positive(value, "halfwidth=");
            if (!halfWidth) {
                return fail(halfWidth.error());
            }
            edited.source.sectionHalfWidth = *halfWidth;
        }
    } else if (name == "ve" || name == "exaggeration") {
        if (Status s = onlyFor(viewport, option, isSection(viewport.kind),
                               "long_section and cross_sections");
            !s) {
            return s;
        }
        auto exaggeration = positive(value, "ve=");
        if (!exaggeration) {
            return fail(exaggeration.error());
        }
        edited.verticalExaggeration = *exaggeration;
    } else if (name == "tilt") {
        if (Status s = onlyFor(viewport, option, viewport.kind == ViewportKind::Model3D, "model_3d");
            !s) {
            return s;
        }
        auto tilt = number(value, "tilt=");
        if (!tilt) {
            return fail(tilt.error());
        }
        if (*tilt < 1.0 || *tilt > 89.0) {
            return makeError(ErrorCode::InvalidArgument,
                             "tilt= is the camera's height angle, 1 to 89 degrees", option.word);
        }
        edited.tiltDegrees = *tilt;
    } else if (name == "north" || name == "northarrow" || name == "scalebar") {
        if (Status s = onlyFor(viewport, option, isPlanLike(viewport.kind), "plan and key_plan");
            !s) {
            return s;
        }
        auto on = onOff(value, name);
        if (!on) {
            return fail(on.error());
        }
        (name == "scalebar" ? edited.scaleBar : edited.northArrow) = *on;
    } else if (name == "locked" || name == "lock") {
        auto on = onOff(value, name);
        if (!on) {
            return fail(on.error());
        }
        edited.locked = *on;
    } else if (name == "title") {
        edited.title = unescaped(value);
    } else if (name == "text") {
        if (Status s = onlyFor(viewport, option,
                               isOneOf(viewport.kind, {ViewportKind::Notes, ViewportKind::Image}),
                               "notes and image");
            !s) {
            return s;
        }
        edited.text = unescaped(value);
    } else if (name == "hide" || name == "show") {
        if (value.empty()) {
            return makeError(ErrorCode::InvalidArgument, std::format("{}= names a layer", name),
                             option.word);
        }
        if (name == "hide") {
            (void)edited.hiddenLayers.hide(value);
        } else {
            (void)edited.hiddenLayers.show(value);
        }
    } else if (name == "hidden") {
        edited.hiddenLayers.clear();
        std::size_t start = 0;
        while (start < value.size()) {
            const std::size_t comma = value.find(',', start);
            const std::string_view layer =
                value.substr(start, comma == std::string_view::npos ? std::string_view::npos
                                                                    : comma - start);
            if (!layer.empty()) {
                (void)edited.hiddenLayers.hide(layer);
            }
            if (comma == std::string_view::npos) {
                break;
            }
            start = comma + 1;
        }
    } else if (name == "legend" || name == "scope") {
        if (Status s = onlyFor(viewport, option, viewport.kind == ViewportKind::Legend, "legend");
            !s) {
            return s;
        }
        // The stored names, and the words a person reaches for.
        std::optional<LegendScope> scope = legendScopeFrom(value);
        const std::string word = folded(value);
        if (!scope && (word == "thissheet" || word == "sheet")) {
            scope = LegendScope::ThisSheet;
        } else if (!scope && (word == "wholeset" || word == "set")) {
            scope = LegendScope::WholeSet;
        } else if (!scope && (word == "wholedrawing" || word == "drawing" || word == "all")) {
            scope = LegendScope::WholeDrawing;
        }
        if (!scope) {
            return makeError(ErrorCode::InvalidArgument,
                             "legend= is this_sheet, whole_set or whole_drawing", option.word);
        }
        edited.legendScope = *scope;
    } else if (name == "grid") {
        if (Status s = onlyFor(viewport, option, isPlanLike(viewport.kind), "plan and key_plan");
            !s) {
            return s;
        }
        std::optional<GridStyle> style = gridStyleFrom(folded(value));
        if (!style && folded(value) == "off") {
            style = GridStyle::None;
        }
        if (!style) {
            return makeError(ErrorCode::InvalidArgument, "grid= is none, ticks, crosses or lines",
                             option.word);
        }
        edited.gridStyle = *style;
    } else if (name == "gridinterval") {
        if (Status s = onlyFor(viewport, option, isPlanLike(viewport.kind), "plan and key_plan");
            !s) {
            return s;
        }
        if (folded(value) == "auto") {
            edited.gridInterval = 0.0;
        } else {
            auto interval = positive(value, "gridinterval=");
            if (!interval) {
                return fail(interval.error());
            }
            edited.gridInterval = *interval;
        }
    } else if (name == "revisions" || name == "limit") {
        if (Status s = onlyFor(viewport, option, viewport.kind == ViewportKind::Revisions,
                               "revisions");
            !s) {
            return s;
        }
        if (folded(value) == "all") {
            edited.revisionLimit = 0;
        } else {
            auto limit = counting(value, "revisions=");
            if (!limit) {
                return fail(limit.error());
            }
            edited.revisionLimit = *limit;
        }
    } else if (name == "kind") {
        return makeError(ErrorCode::InvalidArgument,
                         "a view's kind is fixed: VIEW REMOVE it and VIEW ADD another",
                         option.word);
    } else {
        return makeError(ErrorCode::InvalidArgument,
                         "no view option of that name; it is rect, scale, centre, rotation, "
                         "alignment, from, to, stations, interval, halfwidth, ve, tilt, north, "
                         "scalebar, grid, gridinterval, legend, revisions, locked, title, text, "
                         "hide, show, hidden or file",
                         option.word);
    }
    viewport = std::move(edited);
    return {};
}

Viewport defaultViewport(const SheetSet& set, std::size_t sheetIndexIn, ViewportKind kind,
                         const entity::Model& model)
{
    Viewport viewport;
    viewport.id = nextViewportId(set);
    viewport.kind = kind;
    const Sheet* sheet = sheetIndexIn < set.sheets.size() ? &set.sheets[sheetIndexIn] : nullptr;
    const auto place = [sheet](double width, double height) {
        return sheet != nullptr ? freePlace(*sheet, width, height) : Box2{};
    };
    const std::vector<std::string> alignments = model.alignments.names();
    if (kind == ViewportKind::Plan) {
        viewport.rect = place(200.0, 140.0);
        viewport.autoScale = true;
        viewport.autoCentre = true;
        viewport.northArrow = true;
        viewport.scaleBar = true;
    } else if (kind == ViewportKind::KeyPlan) {
        viewport.rect = place(90.0, 70.0);
        viewport.autoScale = true;
        viewport.autoCentre = true;
        viewport.northArrow = true;
        // No outlines are stored: a key plan draws the sheets' plans where
        // they are when it is drawn (key_plan.hpp), numbered as the set
        // numbers them then, and fits itself to them.
    } else if (kind == ViewportKind::LongSection) {
        viewport.rect = place(380.0, 110.0);
        viewport.scale = 500.0;
        viewport.verticalExaggeration = 10.0;
        viewport.autoCentre = true;
        if (!alignments.empty()) {
            viewport.source.alignment = alignments.front();
        }
    } else if (kind == ViewportKind::CrossSections) {
        viewport.rect = place(180.0, 110.0);
        viewport.scale = 200.0;
        viewport.autoCentre = true;
        viewport.source.sectionHalfWidth = 20.0;
        if (!alignments.empty()) {
            viewport.source.alignment = alignments.front();
            if (const auto middle = middleStation(model, alignments.front())) {
                viewport.source.stations = {*middle};
            }
        }
    } else if (kind == ViewportKind::Model3D) {
        viewport.rect = place(160.0, 110.0);
    } else if (kind == ViewportKind::Legend) {
        viewport.rect = place(80.0, 100.0);
    } else if (kind == ViewportKind::Notes) {
        viewport.rect = place(90.0, 80.0);
        viewport.text = "1. ALL DIMENSIONS ARE IN METRES UNLESS NOTED OTHERWISE.";
    } else if (kind == ViewportKind::SheetIndex) {
        // Room for its five columns and a dozen rows at the table's largest
        // text; a longer set continues in a second block, then shrinks.
        viewport.rect = place(200.0, 120.0);
    } else if (kind == ViewportKind::Revisions) {
        viewport.rect = place(150.0, 50.0);
    } else {
        viewport.rect = place(100.0, 80.0);
    }
    return viewport;
}

std::vector<std::string_view> titleBlockFields()
{
    std::vector<std::string_view> names;
    for (const TitleField& field : kTitleFields) {
        names.push_back(field.name);
    }
    return names;
}

Result<std::string> titleBlockValue(const SheetSet& set, std::string_view field)
{
    const auto name = titleFieldFrom(field);
    if (!name) {
        return unknownTitleField(field);
    }
    if (const std::size_t line = projectLine(*name); line != 0) {
        return line <= set.defaults.projectLines.size() ? set.defaults.projectLines[line - 1]
                                                        : std::string{};
    }
    return *titleSlot(set, *name);
}

Status setTitleBlockValue(SheetSet& set, std::string_view field, std::string value)
{
    const auto name = titleFieldFrom(field);
    if (!name) {
        return unknownTitleField(field);
    }
    if (const std::size_t line = projectLine(*name); line != 0) {
        std::vector<std::string>& lines = set.defaults.projectLines;
        if (lines.size() < line) {
            lines.resize(line);
        }
        lines[line - 1] = std::move(value);
        // Trailing empty lines are no lines: a cleared set stays the default.
        while (!lines.empty() && lines.back().empty()) {
            lines.pop_back();
        }
        return {};
    }
    if (*name == "numbering") {
        if (value.empty()) {
            value = SheetSet{}.numbering;
        } else if (value.find("{n") == std::string::npos) {
            return makeError(ErrorCode::InvalidArgument,
                             "a numbering pattern needs {n} (or {n:02}), or every sheet gets the "
                             "same number",
                             value);
        }
    }
    *titleSlot(set, *name) = std::move(value);
    return {};
}

Status writeSheetSetFile(const SheetSet& set, const std::filesystem::path& path)
{
    auto json = sheetSetToJson(set);
    if (!json) {
        return json.error();
    }
    std::ofstream out(path, std::ios::binary);
    if (!out) {
        return makeError(ErrorCode::FileExportFailure, "the file cannot be written",
                         pathText(path));
    }
    out << *json << '\n';
    out.close();
    if (!out) {
        return makeError(ErrorCode::FileExportFailure, "the file cannot be written",
                         pathText(path));
    }
    return {};
}

Result<SheetSet> readSheetSetFile(const std::filesystem::path& path)
{
    std::ifstream in(path, std::ios::binary);
    if (!in) {
        return makeError(ErrorCode::NotFound, "the file cannot be opened", pathText(path));
    }
    const std::string text((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
    if (in.bad()) {
        return makeError(ErrorCode::FileImportFailure, "the file cannot be read", pathText(path));
    }
    auto set = sheetSetFromJson(text);
    if (!set) {
        return makeError(set.error().code, set.error().message,
                         pathText(path) + (set.error().context.empty()
                                               ? std::string{}
                                               : ": " + set.error().context));
    }
    return set;
}

std::string describeSheet(const SheetSet& set, std::size_t index)
{
    if (index >= set.sheets.size()) {
        return {};
    }
    std::string text = sheetLine(set, index);
    for (const auto& [field, value] : set.sheets[index].fields) {
        text += std::format("\n  field {}={}", field, inQuotes(value));
    }
    for (const Viewport& viewport : set.sheets[index].viewports) {
        text += "\n  " + viewportSummary(viewport);
    }
    return text;
}

std::string describeViewport(const Viewport& viewport)
{
    const ViewportKind kind = viewport.kind;
    std::string line = std::format("view id={} kind={} rect={}", viewport.id, toString(kind),
                                   boxText(viewport.rect));
    if (drawnToScale(kind)) {
        line += " scale=" + (viewport.autoScale ? std::string("auto") : decimal(viewport.scale));
        line += " centre=" + (viewport.autoCentre ? std::string("auto") : pointText(viewport.centre));
    }
    if (isPlanLike(kind) || kind == ViewportKind::Model3D) {
        line += " rotation=" + decimal(viewport.rotation * math::kRadToDeg);
    }
    const ViewportSource& source = viewport.source;
    if (isSection(kind) || !source.alignment.empty()) {
        line += " alignment=" + inQuotes(source.alignment);
    }
    if (isSection(kind) || source.chainageFrom != 0.0 || source.chainageTo != 0.0) {
        line += " from=" + decimal(source.chainageFrom) + " to=" + decimal(source.chainageTo);
    }
    if (kind == ViewportKind::CrossSections) {
        std::string stations;
        for (const double station : source.stations) {
            stations += (stations.empty() ? "" : ",") + decimal(station);
        }
        line += " interval=" + decimal(source.sectionInterval) + " stations=" + stations +
                " halfwidth=" + decimal(source.sectionHalfWidth);
    }
    if (isSection(kind)) {
        line += " ve=" + decimal(viewport.verticalExaggeration);
    }
    if (kind == ViewportKind::Model3D) {
        line += " tilt=" + decimal(viewport.tiltDegrees);
    }
    if (isPlanLike(kind)) {
        line += std::format(" north={} scalebar={} grid={} gridinterval={}",
                            onOffText(viewport.northArrow), onOffText(viewport.scaleBar),
                            toString(viewport.gridStyle),
                            viewport.gridInterval > 0.0 ? decimal(viewport.gridInterval)
                                                        : std::string("auto"));
    }
    if (kind == ViewportKind::Legend) {
        line += std::format(" legend={}", toString(viewport.legendScope));
    }
    if (kind == ViewportKind::Revisions) {
        line += " revisions=" + (viewport.revisionLimit == 0 ? std::string("all")
                                                             : std::to_string(viewport.revisionLimit));
    }
    line += std::format(" locked={}", onOffText(viewport.locked));
    if (!viewport.hiddenLayers.empty()) {
        std::string hidden;
        for (const std::string& layer : viewport.hiddenLayers.hidden()) {
            hidden += (hidden.empty() ? "" : ",") + layer;
        }
        line += " hidden=" + inQuotes(hidden);
    }
    if (!viewport.title.empty()) {
        line += " title=" + inQuotes(viewport.title);
    }
    if (!viewport.text.empty()) {
        line += " text=" + inQuotes(viewport.text);
    }
    if (!viewport.marks.empty()) {
        line += std::format(" marks={}", viewport.marks.size());
    }
    return line;
}

} // namespace katana::cad::plotting
