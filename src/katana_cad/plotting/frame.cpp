#include "katana/cad/plotting/frame.hpp"

#include <algorithm>
#include <charconv>
#include <cmath>
#include <map>
#include <string>
#include <utility>

#include <nlohmann/json.hpp>

namespace katana::cad::plotting {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using Json = nlohmann::json;

namespace {

// The frame's JSON, embedded by the compiler (#embed; the directory is given
// by --embed-dir in src/katana_cad/CMakeLists.txt, and the dependency file
// names the JSON, so editing it rebuilds this file). #embed is C++26; the
// pragma keeps a C++23 build (KATANA_CXX_STANDARD) from failing -Werror on it.
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wc++26-extensions"
constexpr unsigned char kEmbeddedFrame[] = {
#embed "a3_landscape.json"
};
#pragma GCC diagnostic pop

// The size the measured frame was drawn for. Only its ratio to the paper is
// used (frameScaleFor); the JSON's own sheet size is what a parse reads.
constexpr double kMeasuredWidthMm = 420.0;
constexpr double kMeasuredHeightMm = 297.0;

// The removed organisation header's cap height and the removed notes text's
// first-line anchor and cap height, as the frame's cell descriptions record
// them: the slots are laid out exactly where the branding was.
constexpr double kOrganisationCapMm = 3.738;
constexpr Point2 kNotesAnchor{220.918, 32.961};
constexpr double kNotesCapMm = 0.85;
// The measured frame's condensing factor: 0.82 of Arial is Arial Narrow's
// width, which is what the notes were set in.
constexpr double kCondensed = 0.82;
// Line pitch as a multiple of the cap height (the measured rule).
constexpr double kLinePitch = 1.5;

// The frame's numbered text fields, by the meaning each has in the measured
// frame, under the general names a sheet set uses (docs/plotting.md). 8 and 9
// (the coordinate system and zone) are replaced as a whole text below.
const std::map<int, std::string_view>& userTextFields()
{
    static const std::map<int, std::string_view> fields{
        {1, "sheet_count"},     {2, "approver_name"},   {3, "locator_name"},
        {4, "surveyor_name"},   {5, "compiler_name"},   {6, "reviewer_name"},
        {7, "model_name"},      {10, "project_line_1"}, {11, "project_line_2"},
        {12, "project_line_3"}, {13, "project_line_4"}, {14, "set_number"},
        {17, "scale"},          {19, "file_name"},
    };
    return fields;
}

// The measured frame feeds ONE date to all six date texts, so a sheet
// reviewed a week after it was surveyed said otherwise. Each sign-off row gets
// its own date field here, by the entity that draws it (the sign-off rows
// from the top: utilities/locator, surveyed, compiled, reviewed; then the
// approver's row, and the stamp under the title block).
std::string_view dateFieldFor(std::size_t entityIndex)
{
    switch (entityIndex) {
    case 122:
        return "approver_date";
    case 123:
        return "locator_date";
    case 124:
        return "surveyor_date";
    case 125:
        return "compiler_date";
    case 126:
        return "reviewer_date";
    default:
        return "plot_date";
    }
}

Result<entity::Color> colourOf(const Json& item)
{
    return entity::Color::fromHex(item.at("colour_hex").get<std::string>());
}

Point2 pointOf(const Json& pair)
{
    return Point2(pair.at(0).get<double>(), pair.at(1).get<double>());
}

Box2 boxOf(const Json& rect)
{
    if (rect.is_array()) {
        return Box2(Point2(rect.at(0).get<double>(), rect.at(1).get<double>()),
                    Point2(rect.at(2).get<double>(), rect.at(3).get<double>()));
    }
    return Box2(Point2(rect.at("x0").get<double>(), rect.at("y0").get<double>()),
                Point2(rect.at("x1").get<double>(), rect.at("y1").get<double>()));
}

FrameRole roleOf(std::string_view role)
{
    if (role == "field-underline") {
        return FrameRole::Underline;
    }
    if (role == "construction") {
        return FrameRole::Construction;
    }
    if (role == "legend") {
        return FrameRole::Legend;
    }
    if (role == "field") {
        return FrameRole::Field;
    }
    if (role == "stamp") {
        return FrameRole::Stamp;
    }
    if (role == "label") {
        return FrameRole::Label;
    }
    return FrameRole::Border;
}

// "vertical-horizontal", e.g. "bottom-left" or "middle-centre".
std::pair<VerticalJustify, HorizontalJustify> justifyOf(std::string_view justify)
{
    VerticalJustify vertical = VerticalJustify::Bottom;
    HorizontalJustify horizontal = HorizontalJustify::Left;
    if (justify.starts_with("middle")) {
        vertical = VerticalJustify::Middle;
    } else if (justify.starts_with("top")) {
        vertical = VerticalJustify::Top;
    }
    if (justify.ends_with("centre") || justify.ends_with("center")) {
        horizontal = HorizontalJustify::Centre;
    } else if (justify.ends_with("right")) {
        horizontal = HorizontalJustify::Right;
    }
    return {vertical, horizontal};
}

// The field name of a {name} placeholder starting at `at` - lower-case
// letters, digits and underscores, as resolveFields names them - or empty
// text when there is none there.
std::string_view placeholderAt(std::string_view text, std::size_t at)
{
    if (at >= text.size() || text[at] != '{') {
        return {};
    }
    const auto close = text.find('}', at + 1);
    if (close == std::string_view::npos || close == at + 1) {
        return {};
    }
    const std::string_view name = text.substr(at + 1, close - at - 1);
    const bool named = std::all_of(name.begin(), name.end(), [](char c) {
        return (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '_';
    });
    return named ? name : std::string_view{};
}

// The measured text with its tokens turned into {field} placeholders, and the
// fields it shows. Fails on a numbered field this build has no name for, so
// a changed frame is caught by the tests rather than printing a raw token. A
// placeholder already in that form, as in a frame written for Katana, is a
// field too.
Result<std::string> templateOf(std::string_view raw, std::size_t entityIndex,
                               std::vector<std::string>& fields)
{
    // Regional wording turned into fields: the coordinate system (with its
    // zone) and the height datum were hard-coded for one country's grid.
    if (raw.starts_with("CO-ORD SYSTEM:")) {
        fields = {"coordinate_system"};
        return std::string("CO-ORD SYSTEM: {coordinate_system}");
    }
    if (raw.starts_with("HEIGHT DATUM:")) {
        fields = {"height_datum"};
        return std::string("HEIGHT DATUM: {height_datum}");
    }
    std::string text(raw);
    // The project block's first line carried a region-specific label ahead
    // of its field; the lines are the user's, so the label goes with it.
    if (const auto at = text.find("LGA:"); at == 0) {
        const auto field = text.find("$user_text<10,");
        if (field != std::string::npos) {
            text.erase(0, field);
        }
    }
    // The stamp names the paper; on a scaled frame that is no longer A3.
    if (text.starts_with("A3 Border")) {
        text.replace(0, 2, "{paper}");
        fields.emplace_back("paper");
    }
    std::string out;
    std::size_t at = 0;
    while (at < text.size()) {
        if (text.compare(at, 11, "$user_text<") == 0) {
            const auto comma = text.find(',', at);
            const auto close = text.find('>', at);
            if (comma == std::string::npos || close == std::string::npos || comma > close) {
                return makeError(ErrorCode::ParseFailure, "a frame field token is not closed",
                                 "entity " + std::to_string(entityIndex));
            }
            int number = 0;
            const char* first = text.data() + at + 11;
            const char* last = text.data() + comma;
            const auto parsed = std::from_chars(first, last, number);
            const auto found = parsed.ec == std::errc{} && parsed.ptr == last
                                   ? userTextFields().find(number)
                                   : userTextFields().end();
            if (found == userTextFields().end()) {
                return makeError(ErrorCode::ParseFailure,
                                 "the frame uses a numbered field this build has no name for",
                                 "user_text " + std::to_string(number) + " in entity " +
                                     std::to_string(entityIndex));
            }
            out += '{';
            out += found->second;
            out += '}';
            fields.emplace_back(found->second);
            at = close + 1;
        } else if (text.compare(at, 15, "$drawing_number") == 0) {
            out += "{sheet_number}";
            fields.emplace_back("sheet_number");
            at += 15;
        } else if (text.compare(at, 5, "$time") == 0) {
            const std::string_view key = dateFieldFor(entityIndex);
            out += '{';
            out += key;
            out += '}';
            fields.emplace_back(key);
            at += 5;
            if (at < text.size() && text[at] == '<') {
                const auto close = text.find('>', at);
                at = close == std::string::npos ? text.size() : close + 1;
            }
        } else if (text.compare(at, 2, "{{") == 0) {
            // A literal brace, as expandTemplate reads one: not a field.
            out += "{{";
            at += 2;
        } else if (const std::string_view name = placeholderAt(text, at); !name.empty()) {
            // A field named as a sheet names it, "REV {revision}": a frame
            // written for Katana shows it as it is, and the painter fills it.
            out += '{';
            out += name;
            out += '}';
            fields.emplace_back(name);
            at += name.size() + 2;
        } else {
            out += text[at];
            ++at;
        }
    }
    return out;
}

// The cell a derived value is centred in (the measured app re-centres the
// same three).
std::string centredCellFor(const std::vector<std::string>& fields)
{
    if (fields.size() == 1) {
        if (fields[0] == "scale" || fields[0] == "sheet_number" || fields[0] == "sheet_count") {
            return fields[0];
        }
    }
    return {};
}

core::Status readPolyline(const Json& item, Frame& frame)
{
    FramePolyline line;
    for (const Json& point : item.at("points_tbf")) {
        line.points.push_back(pointOf(point));
    }
    line.closed = item.at("closed").get<bool>();
    // The data repeats a closed ring's first point at its end; a closed
    // polyline here does not, so a renderer does not draw that edge twice.
    if (line.closed && line.points.size() > 1 && line.points.front() == line.points.back()) {
        line.points.pop_back();
    }
    line.weightMm = item.at("weight_mm").get<double>();
    auto colour = colourOf(item);
    if (!colour) {
        return colour.error();
    }
    line.colour = *colour;
    if (const Json& dash = item.at("dash_mm"); dash.is_array()) {
        line.dashMm = dash.at(0).get<double>();
        line.gapMm = dash.at(1).get<double>();
    }
    line.role = roleOf(item.at("role").get<std::string>());
    line.plots = line.role != FrameRole::Construction;
    frame.polylines.push_back(std::move(line));
    return {};
}

core::Status readText(const Json& item, Frame& frame)
{
    FrameText text;
    text.anchor = pointOf(item.at("anchor_tbf"));
    text.capHeightMm = item.at("cap_height_mm").get<double>();
    text.xFactor = item.at("x_factor").get<double>();
    const auto [vertical, horizontal] = justifyOf(item.at("justify").get<std::string>());
    text.vertical = vertical;
    text.horizontal = horizontal;
    text.angleDegrees = item.at("angle_deg").get<double>();
    text.lineSpacingMm = kLinePitch * text.capHeightMm;
    text.fontFace = item.at("font_face").get<std::string>();
    text.bold = item.at("bold").get<bool>();
    auto colour = colourOf(item);
    if (!colour) {
        return colour.error();
    }
    text.colour = *colour;
    const auto index = item.at("index").get<std::size_t>();
    auto content = templateOf(item.at("raw_text").get<std::string>(), index, text.fields);
    if (!content) {
        return content.error();
    }
    text.content = std::move(*content);
    text.role = roleOf(item.at("role").get<std::string>());
    if (text.role == FrameRole::Label && !text.fields.empty()) {
        text.role = FrameRole::Field; // the height datum
    }
    text.centredIn = centredCellFor(text.fields);
    frame.texts.push_back(std::move(text));
    return {};
}

core::Status readSymbol(const Json& item, Frame& frame)
{
    FrameSymbol symbol;
    symbol.style = item.at("symbol_style").get<std::string>();
    symbol.at = pointOf(item.at("at_tbf"));
    symbol.sizeMm = item.at("size_mm").get<double>();
    symbol.rotationDegrees = item.at("rotation_deg").get<double>();
    auto colour = colourOf(item);
    if (!colour) {
        return colour.error();
    }
    symbol.colour = *colour;
    frame.symbols.push_back(std::move(symbol));
    return {};
}

// The two slots whose text was removed with the branding, laid out where it
// was: the organisation's name centred in its cell, and the notes from the
// removed text's first-line anchor.
void addSlots(Frame& frame)
{
    if (const FrameCell* cell = frame.cell("organisation"); cell != nullptr) {
        FrameText name;
        name.anchor = cell->rect.center();
        name.capHeightMm = kOrganisationCapMm;
        name.xFactor = kCondensed;
        name.vertical = VerticalJustify::Middle;
        name.horizontal = HorizontalJustify::Centre;
        name.lineSpacingMm = kLinePitch * kOrganisationCapMm;
        name.content = "{organisation}";
        name.fields = {"organisation"};
        name.role = FrameRole::Slot;
        frame.texts.push_back(std::move(name));
    }
    if (frame.cell("notes") != nullptr) {
        FrameText notes;
        notes.anchor = kNotesAnchor;
        notes.capHeightMm = kNotesCapMm;
        notes.xFactor = kCondensed;
        notes.lineSpacingMm = kLinePitch * kNotesCapMm;
        notes.content = "{notes}";
        notes.fields = {"notes"};
        notes.role = FrameRole::Slot;
        frame.texts.push_back(std::move(notes));
    }
}

Result<Frame> parse(const Json& root)
{
    Frame frame;
    frame.id = std::string(kBuiltInFrameId);
    const Json& sheet = root.at("sheet");
    frame.widthMm = sheet.at("width_mm").get<double>();
    frame.heightMm = sheet.at("height_mm").get<double>();
    const Json& margins = sheet.at("margins_mm");
    frame.margins = {margins.at("left").get<double>(), margins.at("right").get<double>(),
                     margins.at("top").get<double>(), margins.at("bottom").get<double>()};
    if (!(frame.widthMm > 0.0) || !(frame.heightMm > 0.0) ||
        !(frame.margins.left + frame.margins.right < frame.widthMm) ||
        !(frame.margins.top + frame.margins.bottom < frame.heightMm)) {
        return makeError(ErrorCode::InvalidArgument, "the frame's size and margins leave no sheet");
    }
    frame.drawingArea = boxOf(sheet.at("viewport_tbf"));
    frame.border = boxOf(sheet.at("drawing_border_tbf"));
    frame.titleBlock = boxOf(sheet.at("title_block_tbf"));
    frame.sheetEdge = boxOf(sheet.at("sheet_edge_tbf"));
    for (const Json& cell : root.at("cells")) {
        std::string id = cell.at("id").get<std::string>();
        // The measured frame's disclaimer cell is the user's notes slot now.
        if (id == "disclaimer") {
            id = "notes";
        }
        frame.cells.push_back({std::move(id), boxOf(cell.at("tbf"))});
    }
    for (const Json& item : root.at("entities")) {
        const std::string kind = item.at("kind").get<std::string>();
        core::Status status;
        if (kind == "poly") {
            status = readPolyline(item, frame);
        } else if (kind == "text") {
            status = readText(item, frame);
        } else if (kind == "symbol") {
            status = readSymbol(item, frame);
        } else {
            status = makeError(ErrorCode::ParseFailure, "a frame item of an unknown kind", kind);
        }
        if (!status) {
            return status.error();
        }
    }
    addSlots(frame);
    return frame;
}

Point2 scaled(const Point2& point, double factor)
{
    return Point2(point.x * factor, point.y * factor);
}

Box2 scaled(const Box2& box, double factor)
{
    return Box2(scaled(box.min, factor), scaled(box.max, factor));
}

} // namespace

const FrameCell* Frame::cell(std::string_view cellId) const
{
    const auto found = std::find_if(cells.begin(), cells.end(),
                                    [cellId](const FrameCell& c) { return c.id == cellId; });
    return found == cells.end() ? nullptr : &*found;
}

std::vector<std::size_t> Frame::textsShowing(std::string_view field) const
{
    std::vector<std::size_t> found;
    for (std::size_t i = 0; i < texts.size(); ++i) {
        if (std::find(texts[i].fields.begin(), texts[i].fields.end(), field) !=
            texts[i].fields.end()) {
            found.push_back(i);
        }
    }
    return found;
}

Result<Frame> parseFrame(std::string_view json)
{
    try {
        return parse(Json::parse(json));
    } catch (const Json::exception& error) {
        return makeError(ErrorCode::ParseFailure, "the plot frame is not in the expected format",
                         error.what());
    }
}

const Result<Frame>& builtInFrame()
{
    static const Result<Frame> frame = parseFrame(std::string_view(
        reinterpret_cast<const char*>(kEmbeddedFrame), sizeof(kEmbeddedFrame)));
    return frame;
}

double frameScaleFor(PaperSize paper, bool landscape)
{
    if (!landscape) {
        return 0.0;
    }
    const PaperDimensions size = paperDimensions(paper, true);
    return std::min(size.widthMm / kMeasuredWidthMm, size.heightMm / kMeasuredHeightMm);
}

Frame scaledFrame(const Frame& frame, double factor, double paperWidthMm, double paperHeightMm)
{
    Frame out = frame;
    out.scale = frame.scale * factor;
    out.widthMm = paperWidthMm;
    out.heightMm = paperHeightMm;
    out.drawingArea = scaled(frame.drawingArea, factor);
    out.border = scaled(frame.border, factor);
    out.titleBlock = scaled(frame.titleBlock, factor);
    out.sheetEdge = Box2(Point2(0.0, 0.0), Point2(paperWidthMm, paperHeightMm));
    // The slack the ISO roundings leave goes to the right and top.
    out.margins = {frame.margins.left * factor, paperWidthMm - out.drawingArea.max.x,
                   paperHeightMm - out.drawingArea.max.y, frame.margins.bottom * factor};
    // The construction guide marks the paper's edge, not the scaled frame's.
    const double edgeX = paperWidthMm / frame.widthMm;
    const double edgeY = paperHeightMm / frame.heightMm;
    for (FramePolyline& line : out.polylines) {
        for (Point2& point : line.points) {
            point = line.role == FrameRole::Construction ? Point2(point.x * edgeX, point.y * edgeY)
                                                         : scaled(point, factor);
        }
        line.weightMm *= factor;
        line.dashMm *= factor;
        line.gapMm *= factor;
    }
    for (FrameText& text : out.texts) {
        text.anchor = scaled(text.anchor, factor);
        text.capHeightMm *= factor;
        text.lineSpacingMm *= factor;
    }
    for (FrameSymbol& symbol : out.symbols) {
        symbol.at = scaled(symbol.at, factor);
        symbol.sizeMm *= factor;
    }
    for (FrameCell& cell : out.cells) {
        cell.rect = scaled(cell.rect, factor);
    }
    return out;
}

Result<Frame> frameFor(std::string_view frameId, PaperSize paper, bool landscape)
{
    if (frameId != kBuiltInFrameId) {
        return makeError(ErrorCode::NotFound, "no plot frame of that name", std::string(frameId));
    }
    if (!landscape) {
        return makeError(ErrorCode::InvalidArgument,
                         "a portrait sheet has no frame: the title block is laid out for "
                         "landscape paper");
    }
    const Result<Frame>& base = builtInFrame();
    if (!base) {
        return base.error();
    }
    const PaperDimensions size = paperDimensions(paper, true);
    if (size.widthMm == base->widthMm && size.heightMm == base->heightMm) {
        return *base;
    }
    return scaledFrame(*base, frameScaleFor(paper, true), size.widthMm, size.heightMm);
}

Box2 fitImage(const Box2& cell, double imageWidth, double imageHeight, double paddingMm)
{
    const Box2 inner = cell.inflated(-paddingMm);
    if (!(imageWidth > 0.0) || !(imageHeight > 0.0) || !std::isfinite(imageWidth) ||
        !std::isfinite(imageHeight) || inner.empty() || !(inner.width() > 0.0) ||
        !(inner.height() > 0.0)) {
        return {};
    }
    const double factor = std::min(inner.width() / imageWidth, inner.height() / imageHeight);
    const double width = imageWidth * factor;
    const double height = imageHeight * factor;
    const Point2 centre = inner.center();
    return Box2(Point2(centre.x - width / 2.0, centre.y - height / 2.0),
                Point2(centre.x + width / 2.0, centre.y + height / 2.0));
}

} // namespace katana::cad::plotting
