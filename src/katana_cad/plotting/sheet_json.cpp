#include "katana/cad/plotting/sheet_json.hpp"

#include <bit>
#include <cmath>
#include <cstdint>
#include <utility>
#include <vector>

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

// A rectangle as [x0, y0, x1, y1]. The empty rectangle - a viewport not
// placed on the paper yet - is Box2's default, made of infinities JSON has
// no numbers for; it is left out like every default (viewportJson), and a
// null is read as it too. Written as they were, the infinities came out as
// nulls that no reader took back (caught by the round-trip test).
Json boxJson(const Box2& box)
{
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

// A member at its default is left out: most of a generated set's viewports
// differ from the defaults in a few members only, and writing every member
// made a 116-sheet set 300 KB of JSON whose writing and reading were most of
// what an edit cost (bench_sheets.cpp). The reader takes its defaults from a
// default-constructed value of the same type, so what is left out reads back
// as exactly what was left out. A default therefore never changes without a
// version increase - the reader would supply the old defaults to an old set.
//
// Doubles are compared bit for bit, so -0.0 is written and reads back as -0.0.
bool sameBits(double a, double b)
{
    return std::bit_cast<std::uint64_t>(a) == std::bit_cast<std::uint64_t>(b);
}

void putNumber(Json& json, const char* key, double value, double fallback)
{
    if (!sameBits(value, fallback)) {
        json[key] = value;
    }
}

void putFlag(Json& json, const char* key, bool value, bool fallback)
{
    if (value != fallback) {
        json[key] = value;
    }
}

void putText(Json& json, const char* key, const std::string& value, const std::string& fallback)
{
    if (value != fallback) {
        json[key] = value;
    }
}

// An array or object, left out when it is empty.
void putList(Json& json, const char* key, Json value)
{
    if (!value.empty()) {
        json[key] = std::move(value);
    }
}

void putPoint(Json& json, const char* key, const Point2& value, const Point2& fallback)
{
    if (!sameBits(value.x, fallback.x) || !sameBits(value.y, fallback.y)) {
        json[key] = pointJson(value);
    }
}

Json signOffJson(const SignOff& signOff)
{
    Json json = Json::object();
    putText(json, "name", signOff.name, {});
    putText(json, "date", signOff.date, {});
    return json;
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
    Json json{{"kind", mark.kind == WorldMark::Kind::MatchLine ? "match_line" : "sheet_outline"}};
    putList(json, "points", std::move(points));
    putText(json, "label", mark.label, {});
    putText(json, "sheet", mark.sheet, {});
    return json;
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
    static const Viewport d;
    Json json{{"id", viewport.id}, {"kind", toString(viewport.kind)}};
    if (!(viewport.rect == d.rect)) {
        json["rect"] = boxJson(viewport.rect);
    }
    putNumber(json, "scale", viewport.scale, d.scale);
    putFlag(json, "auto_scale", viewport.autoScale, d.autoScale);
    putPoint(json, "centre", viewport.centre, d.centre);
    putFlag(json, "auto_centre", viewport.autoCentre, d.autoCentre);
    putNumber(json, "rotation", viewport.rotation, d.rotation);
    putNumber(json, "vertical_exaggeration", viewport.verticalExaggeration,
              d.verticalExaggeration);
    putNumber(json, "tilt_degrees", viewport.tiltDegrees, d.tiltDegrees);
    const ViewportSource& source = viewport.source;
    Json sourceJson = Json::object();
    putText(sourceJson, "alignment", source.alignment, d.source.alignment);
    putNumber(sourceJson, "chainage_from", source.chainageFrom, d.source.chainageFrom);
    putNumber(sourceJson, "chainage_to", source.chainageTo, d.source.chainageTo);
    putNumber(sourceJson, "section_interval", source.sectionInterval, d.source.sectionInterval);
    putList(sourceJson, "stations", Json(source.stations));
    putNumber(sourceJson, "section_half_width", source.sectionHalfWidth,
              d.source.sectionHalfWidth);
    putList(json, "source", std::move(sourceJson));
    Json hidden = Json::array();
    for (const std::string& layer : viewport.hiddenLayers.hidden()) {
        hidden.push_back(layer);
    }
    putList(json, "hidden_layers", std::move(hidden));
    putText(json, "title", viewport.title, d.title);
    putFlag(json, "north_arrow", viewport.northArrow, d.northArrow);
    putFlag(json, "scale_bar", viewport.scaleBar, d.scaleBar);
    putFlag(json, "locked", viewport.locked, d.locked);
    putText(json, "text", viewport.text, d.text);
    Json marks = Json::array();
    for (const WorldMark& mark : viewport.marks) {
        marks.push_back(markJson(mark));
    }
    putList(json, "marks", std::move(marks));
    if (viewport.revisionLimit != d.revisionLimit) {
        json["revision_limit"] = viewport.revisionLimit;
    }
    return json;
}

Viewport viewportFrom(const Json& json)
{
    const Viewport d;
    Viewport viewport;
    viewport.id = json.value("id", d.id);
    const std::string kind = json.value("kind", std::string(toString(d.kind)));
    const auto parsedKind = viewportKindFrom(kind);
    if (!parsedKind) {
        throw BadValue{"unknown viewport kind \"" + kind + "\""};
    }
    viewport.kind = *parsedKind;
    if (json.contains("rect")) {
        viewport.rect = boxFrom(json.at("rect"));
    }
    viewport.scale = json.value("scale", d.scale);
    viewport.autoScale = json.value("auto_scale", d.autoScale);
    if (json.contains("centre")) {
        viewport.centre = pointFrom(json.at("centre"));
    }
    viewport.autoCentre = json.value("auto_centre", d.autoCentre);
    viewport.rotation = json.value("rotation", d.rotation);
    viewport.verticalExaggeration = json.value("vertical_exaggeration", d.verticalExaggeration);
    viewport.tiltDegrees = json.value("tilt_degrees", d.tiltDegrees);
    if (json.contains("source")) {
        const Json& source = json.at("source");
        const ViewportSource& s = d.source;
        viewport.source.alignment = source.value("alignment", s.alignment);
        viewport.source.chainageFrom = source.value("chainage_from", s.chainageFrom);
        viewport.source.chainageTo = source.value("chainage_to", s.chainageTo);
        viewport.source.sectionInterval = source.value("section_interval", s.sectionInterval);
        viewport.source.stations = source.value("stations", s.stations);
        viewport.source.sectionHalfWidth = source.value("section_half_width", s.sectionHalfWidth);
    }
    for (const Json& layer : json.value("hidden_layers", Json::array())) {
        viewport.hiddenLayers.hide(layer.get<std::string>());
    }
    viewport.title = json.value("title", d.title);
    viewport.northArrow = json.value("north_arrow", d.northArrow);
    viewport.scaleBar = json.value("scale_bar", d.scaleBar);
    viewport.locked = json.value("locked", d.locked);
    viewport.text = json.value("text", d.text);
    for (const Json& mark : json.value("marks", Json::array())) {
        viewport.marks.push_back(markFrom(mark));
    }
    if (json.contains("revision_limit")) {
        // A count: -1 or 2.5 would read as some other count, not as an error.
        const Json& limit = json.at("revision_limit");
        if (!limit.is_number_unsigned()) {
            throw BadValue{"the revision limit is not a whole number of at least 0"};
        }
        viewport.revisionLimit = limit.get<std::size_t>();
    }
    return viewport;
}

Json sheetJson(const Sheet& sheet)
{
    static const Sheet d;
    Json json{{"id", sheet.id}};
    putText(json, "name", sheet.name, d.name);
    if (sheet.paper != d.paper) {
        json["paper"] = paperText(sheet.paper);
    }
    putFlag(json, "landscape", sheet.landscape, d.landscape);
    putText(json, "frame", sheet.frame, d.frame);
    putFlag(json, "frame_legend", sheet.frameLegend, d.frameLegend);
    Json fields = Json::object();
    for (const auto& [name, value] : sheet.fields) {
        fields[name] = value;
    }
    putList(json, "fields", std::move(fields));
    Json viewports = Json::array();
    for (const Viewport& viewport : sheet.viewports) {
        viewports.push_back(viewportJson(viewport));
    }
    putList(json, "viewports", std::move(viewports));
    return json;
}

Sheet sheetFrom(const Json& json)
{
    const Sheet d;
    Sheet sheet;
    sheet.id = json.value("id", d.id);
    sheet.name = json.value("name", d.name);
    sheet.paper = paperFrom(json.value("paper", std::string(paperText(d.paper))));
    sheet.landscape = json.value("landscape", d.landscape);
    sheet.frame = json.value("frame", d.frame);
    sheet.frameLegend = json.value("frame_legend", d.frameLegend);
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
    Json json = Json::object();
    putText(json, "organisation", d.organisation, {});
    putList(json, "project_lines", Json(d.projectLines));
    putText(json, "client", d.client, {});
    putList(json, "locator", signOffJson(d.locator));
    putList(json, "surveyor", signOffJson(d.surveyor));
    putList(json, "compiler", signOffJson(d.compiler));
    putList(json, "reviewer", signOffJson(d.reviewer));
    putList(json, "approver", signOffJson(d.approver));
    putText(json, "notes", d.notes, {});
    putText(json, "height_datum", d.heightDatum, {});
    putText(json, "coordinate_system", d.coordinateSystem, {});
    putText(json, "model_name", d.modelName, {});
    putText(json, "set_number", d.setNumber, {});
    putText(json, "logo_asset", d.logoAsset, {});
    return json;
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
    Json root{{"format", kFormat}, {"version", kSheetSetVersion}};
    putList(root, "defaults", defaultsJson(set.defaults));
    putText(root, "numbering", set.numbering, SheetSet{}.numbering);
    putList(root, "revisions", std::move(revisions));
    putList(root, "sheets", std::move(sheets));
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
