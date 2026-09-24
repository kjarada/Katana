#include "katana/cad/plotting/sheet_json.hpp"

#include <cmath>
#include <utility>

#include <nlohmann/json.hpp>

namespace katana::cad::plotting {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using Json = nlohmann::json;

namespace {

constexpr std::string_view kFormat = "katana-sheets";

std::string_view paperText(PaperSize paper)
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

// Thrown inside the reader and caught at its edge, like the JSON library's
// own exceptions, so the helpers below can stay plain value functions.
struct BadValue {
    std::string what;
};

PaperSize paperFrom(const std::string& text)
{
    static constexpr std::pair<std::string_view, PaperSize> kPapers[] = {
        {"A0", PaperSize::A0}, {"A1", PaperSize::A1}, {"A2", PaperSize::A2},
        {"A3", PaperSize::A3}, {"A4", PaperSize::A4}};
    for (const auto& [name, paper] : kPapers) {
        if (name == text) {
            return paper;
        }
    }
    throw BadValue{"unknown paper size \"" + text + "\""};
}

Json pointJson(const Point2& point)
{
    return Json::array({point.x, point.y});
}

Point2 pointFrom(const Json& json)
{
    return Point2(json.at(0).get<double>(), json.at(1).get<double>());
}

// A rectangle as [x0, y0, x1, y1]; the empty rectangle - a viewport not
// placed on the paper yet - as null. Box2's empty value is made of
// infinities, which JSON has no numbers for: written as they are they came
// out as nulls that no reader took back (caught by the round-trip test).
Json boxJson(const Box2& box)
{
    if (box == Box2{}) {
        return nullptr;
    }
    return Json::array({box.min.x, box.min.y, box.max.x, box.max.y});
}

Box2 boxFrom(const Json& json)
{
    if (json.is_null()) {
        return Box2{};
    }
    return Box2(Point2(json.at(0).get<double>(), json.at(1).get<double>()),
                Point2(json.at(2).get<double>(), json.at(3).get<double>()));
}

// Whether every number in `json` is finite. JSON cannot hold an infinity or
// a NaN, and the writer would put a null in its place that reads back as an
// error - so such a set is refused when it is written, where the mistake is.
bool allFinite(const Json& json)
{
    if (json.is_number_float()) {
        return std::isfinite(json.get<double>());
    }
    if (json.is_structured()) {
        for (const Json& item : json) {
            if (!allFinite(item)) {
                return false;
            }
        }
    }
    return true;
}

Json signOffJson(const SignOff& signOff)
{
    return Json{{"name", signOff.name}, {"date", signOff.date}};
}

SignOff signOffFrom(const Json& json)
{
    return {json.value("name", std::string{}), json.value("date", std::string{})};
}

Json markJson(const WorldMark& mark)
{
    Json points = Json::array();
    for (const Point2& point : mark.points) {
        points.push_back(pointJson(point));
    }
    return Json{{"kind", mark.kind == WorldMark::Kind::MatchLine ? "match_line" : "sheet_outline"},
                {"points", std::move(points)},
                {"label", mark.label},
                {"sheet", mark.sheet}};
}

WorldMark markFrom(const Json& json)
{
    WorldMark mark;
    const std::string kind = json.value("kind", std::string("match_line"));
    if (kind == "sheet_outline") {
        mark.kind = WorldMark::Kind::SheetOutline;
    } else if (kind != "match_line") {
        throw BadValue{"unknown mark kind \"" + kind + "\""};
    }
    for (const Json& point : json.value("points", Json::array())) {
        mark.points.push_back(pointFrom(point));
    }
    mark.label = json.value("label", std::string{});
    mark.sheet = json.value("sheet", std::string{});
    return mark;
}

Json viewportJson(const Viewport& viewport)
{
    Json hidden = Json::array();
    for (const std::string& layer : viewport.hiddenLayers.hidden()) {
        hidden.push_back(layer);
    }
    Json marks = Json::array();
    for (const WorldMark& mark : viewport.marks) {
        marks.push_back(markJson(mark));
    }
    const ViewportSource& source = viewport.source;
    return Json{
        {"id", viewport.id},
        {"kind", toString(viewport.kind)},
        {"rect", boxJson(viewport.rect)},
        {"scale", viewport.scale},
        {"auto_scale", viewport.autoScale},
        {"centre", pointJson(viewport.centre)},
        {"auto_centre", viewport.autoCentre},
        {"rotation", viewport.rotation},
        {"vertical_exaggeration", viewport.verticalExaggeration},
        {"tilt_degrees", viewport.tiltDegrees},
        {"source",
         {{"alignment", source.alignment},
          {"chainage_from", source.chainageFrom},
          {"chainage_to", source.chainageTo},
          {"section_interval", source.sectionInterval},
          {"stations", source.stations},
          {"section_half_width", source.sectionHalfWidth}}},
        {"hidden_layers", std::move(hidden)},
        {"title", viewport.title},
        {"north_arrow", viewport.northArrow},
        {"scale_bar", viewport.scaleBar},
        {"locked", viewport.locked},
        {"text", viewport.text},
        {"marks", std::move(marks)},
    };
}

Viewport viewportFrom(const Json& json)
{
    Viewport viewport;
    viewport.id = json.value("id", std::string{});
    const std::string kind = json.value("kind", std::string("plan"));
    const auto parsedKind = viewportKindFrom(kind);
    if (!parsedKind) {
        throw BadValue{"unknown viewport kind \"" + kind + "\""};
    }
    viewport.kind = *parsedKind;
    if (json.contains("rect")) {
        viewport.rect = boxFrom(json.at("rect"));
    }
    viewport.scale = json.value("scale", viewport.scale);
    viewport.autoScale = json.value("auto_scale", false);
    if (json.contains("centre")) {
        viewport.centre = pointFrom(json.at("centre"));
    }
    viewport.autoCentre = json.value("auto_centre", false);
    viewport.rotation = json.value("rotation", 0.0);
    viewport.verticalExaggeration = json.value("vertical_exaggeration", 1.0);
    viewport.tiltDegrees = json.value("tilt_degrees", viewport.tiltDegrees);
    if (json.contains("source")) {
        const Json& source = json.at("source");
        viewport.source.alignment = source.value("alignment", std::string{});
        viewport.source.chainageFrom = source.value("chainage_from", 0.0);
        viewport.source.chainageTo = source.value("chainage_to", 0.0);
        viewport.source.sectionInterval = source.value("section_interval", 0.0);
        viewport.source.stations = source.value("stations", std::vector<double>{});
        viewport.source.sectionHalfWidth = source.value("section_half_width", 0.0);
    }
    for (const Json& layer : json.value("hidden_layers", Json::array())) {
        viewport.hiddenLayers.hide(layer.get<std::string>());
    }
    viewport.title = json.value("title", std::string{});
    viewport.northArrow = json.value("north_arrow", false);
    viewport.scaleBar = json.value("scale_bar", false);
    viewport.locked = json.value("locked", false);
    viewport.text = json.value("text", std::string{});
    for (const Json& mark : json.value("marks", Json::array())) {
        viewport.marks.push_back(markFrom(mark));
    }
    return viewport;
}

Json sheetJson(const Sheet& sheet)
{
    Json viewports = Json::array();
    for (const Viewport& viewport : sheet.viewports) {
        viewports.push_back(viewportJson(viewport));
    }
    Json fields = Json::object();
    for (const auto& [name, value] : sheet.fields) {
        fields[name] = value;
    }
    return Json{{"id", sheet.id},
                {"name", sheet.name},
                {"paper", paperText(sheet.paper)},
                {"landscape", sheet.landscape},
                {"frame", sheet.frame},
                {"frame_legend", sheet.frameLegend},
                {"fields", std::move(fields)},
                {"viewports", std::move(viewports)}};
}

Sheet sheetFrom(const Json& json)
{
    Sheet sheet;
    sheet.id = json.value("id", std::string{});
    sheet.name = json.value("name", std::string{});
    sheet.paper = paperFrom(json.value("paper", std::string("A3")));
    sheet.landscape = json.value("landscape", true);
    sheet.frame = json.value("frame", std::string(kBuiltInFrameId));
    sheet.frameLegend = json.value("frame_legend", true);
    for (const auto& [name, value] : json.value("fields", Json::object()).items()) {
        sheet.fields.insert_or_assign(name, value.get<std::string>());
    }
    for (const Json& viewport : json.value("viewports", Json::array())) {
        sheet.viewports.push_back(viewportFrom(viewport));
    }
    return sheet;
}

Json defaultsJson(const SheetDefaults& d)
{
    return Json{{"organisation", d.organisation},
                {"project_lines", d.projectLines},
                {"client", d.client},
                {"locator", signOffJson(d.locator)},
                {"surveyor", signOffJson(d.surveyor)},
                {"compiler", signOffJson(d.compiler)},
                {"reviewer", signOffJson(d.reviewer)},
                {"approver", signOffJson(d.approver)},
                {"notes", d.notes},
                {"height_datum", d.heightDatum},
                {"coordinate_system", d.coordinateSystem},
                {"model_name", d.modelName},
                {"set_number", d.setNumber},
                {"logo_asset", d.logoAsset}};
}

SheetDefaults defaultsFrom(const Json& json)
{
    SheetDefaults d;
    d.organisation = json.value("organisation", std::string{});
    d.projectLines = json.value("project_lines", std::vector<std::string>{});
    d.client = json.value("client", std::string{});
    const auto signOff = [&json](const char* key) {
        return json.contains(key) ? signOffFrom(json.at(key)) : SignOff{};
    };
    d.locator = signOff("locator");
    d.surveyor = signOff("surveyor");
    d.compiler = signOff("compiler");
    d.reviewer = signOff("reviewer");
    d.approver = signOff("approver");
    d.notes = json.value("notes", std::string{});
    d.heightDatum = json.value("height_datum", std::string{});
    d.coordinateSystem = json.value("coordinate_system", std::string{});
    d.modelName = json.value("model_name", std::string{});
    d.setNumber = json.value("set_number", std::string{});
    d.logoAsset = json.value("logo_asset", std::string{});
    return d;
}

} // namespace

Result<std::string> sheetSetToJson(const SheetSet& set)
{
    Json revisions = Json::array();
    for (const Revision& revision : set.revisions) {
        revisions.push_back(Json{{"code", revision.code},
                                 {"date", revision.date},
                                 {"description", revision.description},
                                 {"by", revision.by}});
    }
    Json sheets = Json::array();
    for (const Sheet& sheet : set.sheets) {
        sheets.push_back(sheetJson(sheet));
    }
    const Json root{{"format", kFormat},
                    {"version", kSheetSetVersion},
                    {"defaults", defaultsJson(set.defaults)},
                    {"numbering", set.numbering},
                    {"revisions", std::move(revisions)},
                    {"sheets", std::move(sheets)}};
    if (!allFinite(root)) {
        return makeError(ErrorCode::InvalidArgument,
                         "the sheet set holds a number that is not finite (an infinity or NaN)");
    }
    try {
        return root.dump();
    } catch (const Json::exception& error) {
        return makeError(ErrorCode::InvalidArgument, "the sheet set holds text that is not UTF-8",
                         error.what());
    }
}

Result<SheetSet> sheetSetFromJson(std::string_view text)
{
    try {
        const Json root = Json::parse(text);
        if (root.value("format", std::string{}) != kFormat) {
            return makeError(ErrorCode::ParseFailure, "not a sheet set");
        }
        const int version = root.at("version").get<int>();
        if (version > kSheetSetVersion) {
            return makeError(ErrorCode::Unsupported,
                             "the sheets were written by a newer version of Katana",
                             "version " + std::to_string(version));
        }
        SheetSet set;
        if (root.contains("defaults")) {
            set.defaults = defaultsFrom(root.at("defaults"));
        }
        set.numbering = root.value("numbering", set.numbering);
        for (const Json& revision : root.value("revisions", Json::array())) {
            set.revisions.push_back({revision.value("code", std::string{}),
                                     revision.value("date", std::string{}),
                                     revision.value("description", std::string{}),
                                     revision.value("by", std::string{})});
        }
        for (const Json& sheet : root.value("sheets", Json::array())) {
            set.sheets.push_back(sheetFrom(sheet));
        }
        return set;
    } catch (const Json::exception& error) {
        return makeError(ErrorCode::ParseFailure, "the sheet set is not in the expected format",
                         error.what());
    } catch (const BadValue& error) {
        return makeError(ErrorCode::ParseFailure, "the sheet set is not in the expected format",
                         error.what);
    }
}

} // namespace katana::cad::plotting
