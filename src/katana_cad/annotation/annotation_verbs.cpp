// The annotation verbs of the command line (docs/annotation.md, "The command
// line"): CommandInterpreter's members for ANNOSCALE, TEXTSTYLE, TEXT and
// MTEXT with options, TEXTEDIT, LABELSTYLE, LABEL, AUTOLABEL, the DIM kinds,
// LEADER and BALLOON. Kept here, with the rest of the annotation code, rather
// than in command_interpreter.cpp.
//
// Written for an agent as much as for a person, which decides three things:
//
//   * OPTIONS ARE key=value, in any order after the positional arguments,
//     keys case-insensitive; an unknown key is refused naming the keys that
//     are known, so a typo is an error and not a silently ignored setting.
//   * REPLIES ARE RECORDS: `key=value` pairs separated by spaces, one record
//     per line, values quoted when they hold a space or a quote. What was
//     made says its id; a LIST says every field a SET takes.
//   * EVERY EDIT IS ONE STEP through Document::execute: one UNDO takes back a
//     LABEL of forty entities or an AUTOLABEL RUN of four thousand labels.
//
// "\n" (a backslash and an n) in a text, template or note value is a line
// break, since a command is one line.

#include <algorithm>
#include <charconv>
#include <cmath>
#include <map>
#include <set>
#include <sstream>
#include <string>
#include <vector>

#include "katana/cad/annotation/auto_label.hpp"
#include "katana/cad/annotation/dimension_build.hpp"
#include "katana/cad/annotation/label_layout.hpp"
#include "katana/cad/annotation/text_layout.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/dimension_draw.hpp"
#include "katana/cad/survey_coding.hpp"
#include "katana/commands/change_set.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/anchor.hpp"
#include "katana/entity/label_text.hpp"
#include "katana/entity/label_values.hpp"
#include "katana/entity/leader_values.hpp"
#include "katana/entity/text_block.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::Point2;
using katana::geometry::Vec2;
namespace cmd = katana::commands;
namespace ann = katana::cad::annotation;

namespace {

// LABEL LAYOUT's keep-out at most: the plan painter's own bound, so a
// drawing too large for it is judged the same way on both.
constexpr std::size_t kLayoutLineworkLimit = 200000;

std::string upperCase(std::string text)
{
    for (char& c : text) {
        if (c >= 'a' && c <= 'z') {
            c = static_cast<char>(c - 'a' + 'A');
        }
    }
    return text;
}

std::string lowerCase(std::string text)
{
    for (char& c : text) {
        if (c >= 'A' && c <= 'Z') {
            c = static_cast<char>(c - 'A' + 'a');
        }
    }
    return text;
}

katana::core::Error usageOf(std::string_view text)
{
    return makeError(ErrorCode::InvalidArgument, "usage: " + std::string(text));
}

// ---- options ------------------------------------------------------------------------

// The positional arguments and the key=value options of a verb. A token is an
// option when what precedes its first '=' is a word (letters, digits, '-'),
// so a template or a note that holds an '=' is still an option's value.
struct Arguments {
    std::vector<std::string> positional;
    std::map<std::string, std::string> options;

    [[nodiscard]] bool has(std::string_view key) const { return options.contains(std::string(key)); }
    [[nodiscard]] const std::string* find(std::string_view key) const
    {
        const auto found = options.find(std::string(key));
        return found == options.end() ? nullptr : &found->second;
    }
};

bool isKey(std::string_view key)
{
    return !key.empty() && std::all_of(key.begin(), key.end(), [](char c) {
        return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') ||
               c == '-' || c == '_';
    });
}

// "\n" typed on the command line is a line break in the value.
std::string unescape(std::string_view text)
{
    std::string out;
    out.reserve(text.size());
    for (std::size_t i = 0; i < text.size(); ++i) {
        if (text[i] == '\\' && i + 1 < text.size() && text[i + 1] == 'n') {
            out += '\n';
            ++i;
        } else {
            out += text[i];
        }
    }
    return out;
}

Result<Arguments> splitArguments(const std::vector<std::string>& args, std::size_t from,
                                 std::initializer_list<std::string_view> known)
{
    Arguments result;
    for (std::size_t i = from; i < args.size(); ++i) {
        const std::string& token = args[i];
        const std::size_t equals = token.find('=');
        if (equals != std::string::npos && isKey(std::string_view(token).substr(0, equals))) {
            std::string key = lowerCase(token.substr(0, equals));
            if (std::find(known.begin(), known.end(), key) == known.end()) {
                std::string list;
                for (const std::string_view k : known) {
                    list += list.empty() ? "" : " ";
                    list += k;
                }
                return makeError(ErrorCode::InvalidArgument, "unknown option '" + key + "'",
                                 "known: " + list);
            }
            result.options[key] = token.substr(equals + 1);
        } else {
            result.positional.push_back(token);
        }
    }
    return result;
}

Result<double> numberOf(const std::string& text, const char* what)
{
    if (const auto value = katana::core::parseFiniteDouble(text)) {
        return *value;
    }
    return makeError(ErrorCode::ParseFailure, std::string("expected a number for ") + what, text);
}

Result<bool> switchOf(const std::string& text, const char* what)
{
    const std::string value = lowerCase(text);
    if (value == "on" || value == "yes" || value == "true" || value == "1") {
        return true;
    }
    if (value == "off" || value == "no" || value == "false" || value == "0") {
        return false;
    }
    return makeError(ErrorCode::ParseFailure, std::string("expected on or off for ") + what, text);
}

Result<int> integerOf(const std::string& text, const char* what)
{
    int value = 0;
    const char* end = text.data() + text.size();
    const auto [parsed, error] = std::from_chars(text.data(), end, value);
    if (error != std::errc{} || parsed != end) {
        return makeError(ErrorCode::ParseFailure, std::string("expected a whole number for ") + what,
                         text);
    }
    return value;
}

// A colour, or ByLayer (none).
Result<std::optional<katana::entity::Color>> colourOf(const std::string& text)
{
    const std::string value = lowerCase(text);
    if (value == "bylayer" || value == "-" || value.empty()) {
        return std::optional<katana::entity::Color>{};
    }
    auto colour = katana::entity::Color::fromHex(text);
    if (!colour) {
        return colour.error();
    }
    return std::optional<katana::entity::Color>(*colour);
}

Result<EntityId> idOf(std::string_view text)
{
    if (!text.empty() && text.front() == '#') {
        text.remove_prefix(1);
    }
    EntityId value = 0;
    const char* end = text.data() + text.size();
    const auto [parsed, error] = std::from_chars(text.data(), end, value);
    if (error != std::errc{} || parsed != end || value == 0) {
        return makeError(ErrorCode::ParseFailure, "expected an entity id", std::string(text));
    }
    return value;
}

// ---- replies --------------------------------------------------------------------------

// A value as a record field: bare when it can be, quoted with \" and \\ and
// \n escaped when it holds a space, a quote, a backslash or a line break.
std::string field(std::string_view value)
{
    const bool needs = value.empty() || value.find_first_of(" \"\\\n=") != std::string_view::npos;
    if (!needs) {
        return std::string(value);
    }
    std::string out = "\"";
    for (const char c : value) {
        if (c == '"' || c == '\\') {
            out += '\\';
            out += c;
        } else if (c == '\n') {
            out += "\\n";
        } else {
            out += c;
        }
    }
    out += '"';
    return out;
}

// Shortest text that reads back as the same double, as every number in a
// reply is: an agent that reads 2.5 back must get 2.5.
std::string real(double value)
{
    return katana::core::formatExactReal(value);
}

std::string onOff(bool value)
{
    return value ? "on" : "off";
}

std::string colourText(const std::optional<katana::entity::Color>& colour)
{
    return colour ? colour->toHex() : std::string("bylayer");
}

std::string degrees(double radians)
{
    return real(radians * katana::math::kRadToDeg);
}

// ---- text styles ----------------------------------------------------------------------

constexpr std::initializer_list<std::string_view> kTextStyleKeys = {
    "font", "paper", "width", "oblique", "bold", "italic", "colour", "color",
    "mask", "margin", "readable", "spacing"};

Status applyTextStyleOptions(const Arguments& args, katana::entity::TextStyle& style)
{
    for (const auto& [key, value] : args.options) {
        if (key == "font") {
            style.fontFamily = value;
        } else if (key == "paper") {
            auto v = numberOf(value, "paper");
            if (!v) {
                return v.error();
            }
            style.paperHeight = *v;
        } else if (key == "width") {
            auto v = numberOf(value, "width");
            if (!v) {
                return v.error();
            }
            style.widthFactor = *v;
        } else if (key == "oblique") {
            auto v = numberOf(value, "oblique");
            if (!v) {
                return v.error();
            }
            style.oblique = *v * katana::math::kDegToRad;
        } else if (key == "bold" || key == "italic" || key == "mask" || key == "readable") {
            auto v = switchOf(value, key.c_str());
            if (!v) {
                return v.error();
            }
            (key == "bold" ? style.bold
                           : key == "italic" ? style.italic
                                             : key == "mask" ? style.mask : style.readable) = *v;
        } else if (key == "colour" || key == "color") {
            auto v = colourOf(value);
            if (!v) {
                return v.error();
            }
            style.color = *v;
        } else if (key == "margin") {
            auto v = numberOf(value, "margin");
            if (!v) {
                return v.error();
            }
            style.maskMargin = *v;
        } else if (key == "spacing") {
            auto v = numberOf(value, "spacing");
            if (!v) {
                return v.error();
            }
            style.lineSpacing = *v;
        }
    }
    return {};
}

std::string describeTextStyle(const katana::entity::TextStyle& style)
{
    std::ostringstream out;
    out << "name=" << field(style.name) << " font=" << field(style.fontFamily)
        << " paper=" << real(style.paperHeight) << " width=" << real(style.widthFactor)
        << " oblique=" << degrees(style.oblique) << " bold=" << onOff(style.bold)
        << " italic=" << onOff(style.italic) << " colour=" << colourText(style.color)
        << " mask=" << onOff(style.mask) << " margin=" << real(style.maskMargin)
        << " readable=" << onOff(style.readable) << " spacing=" << real(style.lineSpacing);
    return out.str();
}

// ---- label styles ---------------------------------------------------------------------

constexpr std::initializer_list<std::string_view> kLabelStyleKeys = {
    "kind",      "text",     "textstyle",  "paper",     "placement", "orientation",
    "offset",    "leader",   "displace",   "priority",  "colour",    "color",
    "marker",    "markersize", "minlength", "interval", "tick",      "ticklength"};

// The template a new style of each kind starts with.
std::string defaultTemplate(katana::entity::LabelKind kind)
{
    switch (kind) {
    case katana::entity::LabelKind::Point:
        return "{point}";
    case katana::entity::LabelKind::Segment:
        return "{bearing:dms} {distance:.3f}";
    case katana::entity::LabelKind::Arc:
        return "R {radius:.3f}";
    case katana::entity::LabelKind::Area:
        return "{area:m2:.1f} m²";
    case katana::entity::LabelKind::Chainage:
        return "{chainage:ch}";
    }
    return "{id}";
}

Status applyLabelStyleOptions(const Arguments& args, katana::entity::LabelStyle& style)
{
    for (const auto& [key, value] : args.options) {
        const auto number = [&](double& field) -> Status {
            auto v = numberOf(value, key.c_str());
            if (!v) {
                return v.error();
            }
            field = *v;
            return {};
        };
        Status status;
        if (key == "kind") {
            continue; // fixed at NEW
        } else if (key == "text") {
            style.text = unescape(value);
        } else if (key == "textstyle") {
            style.textStyle = value;
        } else if (key == "paper") {
            status = number(style.paperHeight);
        } else if (key == "offset") {
            status = number(style.offset);
        } else if (key == "markersize") {
            status = number(style.markerSize);
        } else if (key == "minlength") {
            status = number(style.minimumLength);
        } else if (key == "interval") {
            status = number(style.interval);
        } else if (key == "tick") {
            status = number(style.tickInterval);
        } else if (key == "ticklength") {
            status = number(style.tickLength);
        } else if (key == "placement") {
            auto v = katana::entity::labelPlacementFromString(value);
            if (!v) {
                return v.error();
            }
            style.placement = *v;
        } else if (key == "orientation") {
            auto v = katana::entity::labelOrientationFromString(value);
            if (!v) {
                return v.error();
            }
            style.orientation = *v;
        } else if (key == "marker") {
            auto v = katana::entity::labelMarkerFromString(value);
            if (!v) {
                return v.error();
            }
            style.marker = *v;
        } else if (key == "leader" || key == "displace") {
            auto v = switchOf(value, key.c_str());
            if (!v) {
                return v.error();
            }
            (key == "leader" ? style.leader : style.displace) = *v;
        } else if (key == "priority") {
            auto v = integerOf(value, "priority");
            if (!v) {
                return v.error();
            }
            style.priority = *v;
        } else if (key == "colour" || key == "color") {
            auto v = colourOf(value);
            if (!v) {
                return v.error();
            }
            style.color = *v;
        }
        if (!status) {
            return status;
        }
    }
    return {};
}

std::string describeLabelStyle(const katana::entity::LabelStyle& style)
{
    std::ostringstream out;
    out << "name=" << field(style.name) << " kind=" << katana::entity::toString(style.kind)
        << " text=" << field(style.text)
        << " textstyle=" << field(style.textStyle.empty()
                                       ? std::string(katana::entity::kDefaultTextStyleName)
                                       : style.textStyle)
        << " paper=" << real(style.paperHeight)
        << " placement=" << katana::entity::toString(style.placement)
        << " orientation=" << katana::entity::toString(style.orientation)
        << " offset=" << real(style.offset) << " leader=" << onOff(style.leader)
        << " displace=" << onOff(style.displace) << " priority=" << style.priority
        << " colour=" << colourText(style.color)
        << " marker=" << katana::entity::toString(style.marker)
        << " markersize=" << real(style.markerSize) << " minlength=" << real(style.minimumLength);
    if (style.kind == katana::entity::LabelKind::Chainage) {
        out << " interval=" << real(style.interval) << " tick=" << real(style.tickInterval)
            << " ticklength=" << real(style.tickLength);
    }
    return out.str();
}

// ---- rules ------------------------------------------------------------------------

constexpr std::initializer_list<std::string_view> kRuleKeys = {
    "style", "layer", "code", "type", "labellayer", "enabled"};

Status applyRuleOptions(const Arguments& args, katana::entity::LabelRule& rule)
{
    for (const auto& [key, value] : args.options) {
        if (key == "style") {
            rule.labelStyle = value;
        } else if (key == "layer") {
            rule.layer = value;
        } else if (key == "code") {
            rule.code = value;
        } else if (key == "type") {
            rule.entityType = value;
        } else if (key == "labellayer") {
            rule.labelLayer = value;
        } else if (key == "enabled") {
            auto v = switchOf(value, "enabled");
            if (!v) {
                return v.error();
            }
            rule.enabled = *v;
        }
    }
    return {};
}

std::string describeRule(const katana::entity::LabelRule& rule)
{
    std::ostringstream out;
    out << "name=" << field(rule.name) << " style=" << field(rule.labelStyle)
        << " layer=" << field(rule.layer) << " code=" << field(rule.code)
        << " type=" << field(rule.entityType) << " labellayer=" << field(rule.labelLayer)
        << " enabled=" << onOff(rule.enabled);
    return out.str();
}

// The text a label shows now, its pieces' texts joined by " | ".
std::string labelTexts(const katana::entity::Model& model, const katana::entity::LabelGeometry& label)
{
    const auto* style = model.labelStyles.find(label.style);
    if (style == nullptr) {
        return {};
    }
    std::string joined;
    for (const auto& piece : ann::labelPiecesOf(model, label, *style)) {
        if (piece.tickOnly) {
            continue;
        }
        const std::string text = ann::labelText(label, *style, piece);
        if (text.empty()) {
            continue;
        }
        joined += joined.empty() ? "" : " | ";
        joined += text;
    }
    return joined;
}

// Runs a command built elsewhere; nullptr is "nothing to do", said.
Result<std::string> runBuilt(Document& document, Result<cmd::CommandPtr> built,
                             const std::string& done, const std::string& nothing)
{
    if (!built) {
        return built.error();
    }
    if (!*built) {
        return nothing;
    }
    if (auto status = document.execute(std::move(*built)); !status) {
        return status.error();
    }
    return done;
}

std::string idList(const std::vector<EntityId>& ids)
{
    std::string text;
    for (const EntityId id : ids) {
        text += text.empty() ? "" : ",";
        text += std::to_string(id);
    }
    return text;
}

} // namespace

// ---- dispatch ---------------------------------------------------------------------------

bool CommandInterpreter::isAnnotationVerb(const std::string& verb, const Tokens& args)
{
    static const std::set<std::string, std::less<>> kVerbs = {
        "ANNOSCALE", "TEXTSTYLE", "MTEXT", "TEXTEDIT", "LABELSTYLE",
        "LABEL",     "AUTOLABEL", "LEADER", "BALLOON"};
    if (kVerbs.contains(verb)) {
        return true;
    }
    if (verb == "TEXT") {
        // The plain TEXT p height "text" is the drawing verb's; with an
        // option it is a styled text.
        return std::any_of(args.begin(), args.end(), [](const std::string& token) {
            const std::size_t equals = token.find('=');
            return equals != std::string::npos &&
                   isKey(std::string_view(token).substr(0, equals));
        });
    }
    if (verb == "DIM" && !args.empty()) {
        static const std::set<std::string, std::less<>> kKinds = {
            "LINEAR", "HORIZONTAL", "VERTICAL", "ALIGNED",  "ANGULAR", "RADIUS",
            "DIAMETER", "ORDINATE", "BASELINE", "CONTINUE", "LIST"};
        return kKinds.contains(upperCase(args.front()));
    }
    return false;
}

std::string CommandInterpreter::annotationHelpText()
{
    return R"(Annotation (docs/annotation.md): sizes in PAPER mm, drawn at mm x scale / 1000
          options are key=value; \n in a text or template is a line break; replies are
          key=value records.  A point may be #id[.start|.end|.mid|.centre|.inside|.vN|.sN],
          or #id@x,y (the point of #id nearest x,y): the annotation made from it follows
          that entity (associative).
AnnoScale ANNOSCALE [N | 1:N]   the plan view's annotation scale (one undo step)
TextStyle TEXTSTYLE LIST | NEW name [opts] | SET name opts | DELETE name
          opts: font= paper=mm width= oblique=deg bold= italic= colour=#RRGGBB|bylayer
          mask=on|off margin=mm readable=on|off spacing=
Text      TEXT p [height] "text" style= paper=mm justify=TL..BR rotation=deg
          MTEXT p "text" [style= paper= justify= rotation=]   paper-sized, multi-line
          TEXTEDIT id [text= style= paper= justify= rotation= height= at=x,y]
LabelStyle LABELSTYLE LIST | NEW name kind=point|segment|arc|area|chainage [text=template]
          [opts] | SET name opts | DELETE name | DEFAULTS | VALUES kind | CHECK kind template
          opts: text= textstyle= paper= placement=auto|above|below|along|centroid|right|left
          orientation=aligned|horizontal offset=mm leader= displace= priority= colour=
          marker=none|cross|dot|circle markersize=mm minlength=mm interval= tick= ticklength=mm
          template: {value:step:step} e.g. {bearing:dms} {distance:.3f}  {area:ha:.4f}
          RL {z:.3f}  {chainage:ch}; steps m mm km ft m2 ha km2 ac deg rad gon dms dms.N dm qb
          ch .Nf upper lower; a line with a value the target lacks is dropped
Label     LABEL id [id...] style= [part=N] [layer=] [at=x,y] [text=]  (SELECTION for the
          selection) | LABEL ALIGN name style= [layer=] | LIST [id...] | SET id [style=
          at=x,y|none text=|none] | DELETE id... | LAYOUT [scale=N] [collisions=on|off]
AutoLabel AUTOLABEL RULE ADD name style= [layer=glob code=glob type= labellayer= enabled=]
          AUTOLABEL RULE SET name opts | RULE DELETE name | RULE LIST
          AUTOLABEL RUN [rule...] | PREVIEW [rule...] | CLEAR [rule...]
Dim       DIM LINEAR|HORIZONTAL|VERTICAL p p at=p [angle=deg] | DIM ALIGNED p p at=p
          DIM ANGULAR vertex p p [at=p] | DIM ANGULAR line-id line-id at=p
          DIM RADIUS|DIAMETER circle-id [at=p] | DIM ORDINATE p at=p [datum=p axis=x|y]
          DIM BASELINE dim-id p [p...] [spacing=] | DIM CONTINUE dim-id p [p...]
Leader    LEADER p p [p...] [text= | template= | labelstyle=] [arrow=closed|open|tick|dot|none
          callout=none|box|circle style= paper=mm arrowsize=mm landing=mm]
          a SMART leader's tip is on an entity (#id...) and its note is read off it every
          time it is drawn: template="IL {prop.invert:.3f}\n{length:.2f} m" (the label
          template language), or labelstyle=name (that style's template); a line naming a
          value the entity lacks is dropped. A tip inside an outline (#id.inside) ends in a dot
          LEADER VALUES id | #id[.point|@x,y]   every value a note there can name
          LEADER LIST [id...] [target=id] | SET id...|SELECTION [text= template= labelstyle=
          arrow= callout= style= paper= arrowsize= landing= tip=p at=p]
          LEADER ATTACH id #id[.point|@x,y] | DETACH id...|SELECTION | FREEZE id...|SELECTION
          LEADER PROP id SET key value [type] | PROP id DELETE key   the tip entity's attribute
          LEADER FOR id...|SELECTION [text=|template=|labelstyle=] [angle=deg length=mm ...]
          a leader to each entity, one undo step (a point, mid-line, mid-arc, inside a lot)
          LEADER ALIGN id id...|SELECTION [x=] [spacing=mm]   the notes in a column
          BALLOON p p [p...] [n=number | template= | labelstyle=] [style= paper=]   a numbered
          circle callout; BALLOON FOR id...|SELECTION numbers one to each entity;
          BALLOON RENUMBER [start=1] [order=id|x|y]   the numbered balloons, in order)";
}

CommandInterpreter::Reply CommandInterpreter::annotation(const std::string& verb,
                                                         const Tokens& args)
{
    if (verb == "ANNOSCALE") {
        return annotationScale(args);
    }
    if (verb == "TEXTSTYLE") {
        return textStyle(args);
    }
    if (verb == "TEXT" || verb == "MTEXT") {
        return styledText(verb, args);
    }
    if (verb == "TEXTEDIT") {
        return textEdit(args);
    }
    if (verb == "LABELSTYLE") {
        return labelStyle(args);
    }
    if (verb == "LABEL") {
        return label(args);
    }
    if (verb == "AUTOLABEL") {
        return autoLabel(args);
    }
    if (verb == "DIM") {
        return dimension(args);
    }
    return leader(verb, args);
}

// ---- points -------------------------------------------------------------------------------

Result<ann::AnchoredPoint> CommandInterpreter::parseAnchoredPoint(const std::string& text)
{
    if (text.empty() || text.front() != '#') {
        auto point = parsePoint(text);
        if (!point) {
            return point.error();
        }
        return ann::AnchoredPoint{*point, {}};
    }
    // "#12@x,y": the place on #12 nearest x,y, named so that it stays there
    // as #12 moves and stretches (entity::nearestAnchor). Split before the
    // '.' of "#12.end", since the point may have decimals.
    if (const std::size_t at = text.find('@'); at != std::string::npos) {
        auto id = idOf(std::string_view(text).substr(0, at));
        if (!id) {
            return id.error();
        }
        const Entity* entity = document_.model().entities.find(*id);
        if (entity == nullptr) {
            return makeError(ErrorCode::NotFound, "entity does not exist", text);
        }
        auto near = parsePoint(text.substr(at + 1));
        if (!near) {
            return near.error();
        }
        const auto ref = katana::entity::nearestAnchor(*entity, *near);
        if (!ref) {
            return makeError(ErrorCode::InvalidArgument,
                             "that entity has no place on it to attach to", text);
        }
        auto anchored = ann::anchoredPoint(document_.model(), *ref);
        if (anchored) {
            lastPoint_ = anchored->point;
        }
        return anchored;
    }
    const std::size_t dot = text.find('.');
    auto id = idOf(std::string_view(text).substr(0, dot));
    if (!id) {
        return id.error();
    }
    const Entity* entity = document_.model().entities.find(*id);
    if (entity == nullptr) {
        return makeError(ErrorCode::NotFound, "entity does not exist", text);
    }
    katana::entity::AnchorRef ref{*id, ann::defaultAnchor(entity->geometry), 0};
    if (dot != std::string::npos) {
        const std::string part = lowerCase(text.substr(dot + 1));
        const auto indexed = [&](std::string_view prefix, katana::entity::AnchorPoint point) {
            if (part.size() > prefix.size() && part.substr(0, prefix.size()) == prefix) {
                std::uint32_t index = 0;
                const std::string_view digits = std::string_view(part).substr(prefix.size());
                const auto [end, error] =
                    std::from_chars(digits.data(), digits.data() + digits.size(), index);
                if (error == std::errc{} && end == digits.data() + digits.size()) {
                    ref.point = point;
                    ref.index = index;
                    return true;
                }
            }
            return false;
        };
        if (!indexed("vertex", katana::entity::AnchorPoint::Vertex) &&
            !indexed("v", katana::entity::AnchorPoint::Vertex) &&
            !indexed("segment", katana::entity::AnchorPoint::SegmentMid) &&
            !indexed("s", katana::entity::AnchorPoint::SegmentMid)) {
            auto point = katana::entity::anchorPointFromString(part);
            if (!point) {
                return makeError(ErrorCode::ParseFailure,
                                 "unknown point of an entity (start end mid centre position "
                                 "inside vN sN, or #id@x,y for the nearest point on it)",
                                 text);
            }
            ref.point = *point;
        }
    }
    auto anchored = ann::anchoredPoint(document_.model(), ref);
    if (anchored) {
        lastPoint_ = anchored->point;
    }
    return anchored;
}

// ---- ANNOSCALE ------------------------------------------------------------------------

CommandInterpreter::Reply CommandInterpreter::annotationScale(const Tokens& args)
{
    if (args.empty()) {
        return "annoscale=" + real(document_.annotationScale());
    }
    std::string text = args.front();
    if (const std::size_t colon = text.find(':'); colon != std::string::npos) {
        if (text.substr(0, colon) != "1") {
            return usageOf("ANNOSCALE N | 1:N");
        }
        text = text.substr(colon + 1);
    }
    auto scale = numberOf(text, "the scale");
    if (!scale) {
        return scale.error();
    }
    if (auto status = document_.setAnnotationScale(*scale); !status) {
        return status.error();
    }
    return "annoscale=" + real(document_.annotationScale());
}

// ---- TEXTSTYLE ------------------------------------------------------------------------------

CommandInterpreter::Reply CommandInterpreter::textStyle(const Tokens& args)
{
    const auto& model = document_.model();
    const std::string action = args.empty() ? "LIST" : upperCase(args.front());
    if (action == "LIST") {
        std::string out;
        model.textStyles.forEach([&](const katana::entity::TextStyle& style) {
            out += out.empty() ? "" : "\n";
            out += describeTextStyle(style);
        });
        return out;
    }
    if (args.size() < 2) {
        return usageOf("TEXTSTYLE LIST | NEW name [opts] | SET name opts | DELETE name");
    }
    const std::string& name = args[1];
    if (action == "DELETE") {
        return finish(document_.execute(cmd::deleteTextStyle(name)),
                      "deleted text style name=" + field(name));
    }
    auto parsed = splitArguments(args, 2, kTextStyleKeys);
    if (!parsed) {
        return parsed.error();
    }
    if (action == "NEW") {
        katana::entity::TextStyle style;
        style.name = name;
        if (auto status = applyTextStyleOptions(*parsed, style); !status) {
            return status.error();
        }
        return finish(document_.execute(cmd::createTextStyle(style)),
                      "created text style " + describeTextStyle(style));
    }
    if (action == "SET") {
        const auto* current = model.textStyles.find(name);
        if (current == nullptr) {
            return makeError(ErrorCode::NotFound, "text style does not exist", name);
        }
        katana::entity::TextStyle style = *current;
        if (auto status = applyTextStyleOptions(*parsed, style); !status) {
            return status.error();
        }
        return finish(document_.execute(cmd::updateTextStyle(style)),
                      "updated text style " + describeTextStyle(style));
    }
    if (action == "INFO") {
        const auto* current = model.textStyles.find(name);
        if (current == nullptr) {
            return makeError(ErrorCode::NotFound, "text style does not exist", name);
        }
        return describeTextStyle(*current);
    }
    return usageOf("TEXTSTYLE LIST | NEW name [opts] | SET name opts | DELETE name | INFO name");
}

// ---- TEXT, MTEXT, TEXTEDIT ------------------------------------------------------------------

namespace {

constexpr std::initializer_list<std::string_view> kTextKeys = {"style", "paper", "justify",
                                                               "rotation", "height", "text", "at"};

// The options a styled text takes, onto `text`. The model height of a
// paper-sized text is set for the document's annotation scale (what its box
// and picking go by; TextGeometry::height).
Status applyTextOptions(const Arguments& args, const katana::entity::Model& model, double scale,
                        katana::entity::TextGeometry& text)
{
    for (const auto& [key, value] : args.options) {
        if (key == "style") {
            if (!value.empty() && !model.textStyles.contains(value)) {
                return makeError(ErrorCode::NotFound, "text style does not exist", value);
            }
            text.style = value;
        } else if (key == "paper") {
            auto v = numberOf(value, "paper");
            if (!v) {
                return v.error();
            }
            text.paperHeight = *v;
        } else if (key == "justify") {
            auto v = katana::entity::textJustifyFromString(value);
            if (!v) {
                return v.error();
            }
            text.justify = *v;
        } else if (key == "rotation") {
            auto v = numberOf(value, "rotation");
            if (!v) {
                return v.error();
            }
            text.rotation = *v * katana::math::kDegToRad;
        } else if (key == "height") {
            auto v = numberOf(value, "height");
            if (!v) {
                return v.error();
            }
            text.height = *v;
        } else if (key == "text") {
            text.text = unescape(value);
        }
    }
    if (ann::isPaperSized(model, text)) {
        text.height = ann::resolveTextStyle(model, text, scale).height;
    }
    return {};
}

} // namespace

CommandInterpreter::Reply CommandInterpreter::styledText(const std::string& verb, const Tokens& args)
{
    auto parsed = splitArguments(args, 0, kTextKeys);
    if (!parsed) {
        return parsed.error();
    }
    const bool mtext = verb == "MTEXT";
    const auto& positional = parsed->positional;
    if (positional.size() < 2 || positional.size() > 3 || (mtext && positional.size() != 2)) {
        return usageOf(mtext ? "MTEXT p \"text\" [style= paper= justify= rotation=]"
                             : "TEXT p [height] \"text\" [style= paper= justify= rotation=]");
    }
    auto at = parsePoint(positional[0]);
    if (!at) {
        return at.error();
    }
    katana::entity::TextGeometry text;
    text.position = *at;
    text.text = unescape(positional.back());
    if (positional.size() == 3) {
        auto height = numberOf(positional[1], "the height");
        if (!height) {
            return height.error();
        }
        text.height = *height;
    } else {
        // No height given: a paper-sized text in the Standard style, unless
        // the options say otherwise.
        text.style = std::string(katana::entity::kDefaultTextStyleName);
    }
    const auto& model = document_.model();
    if (auto status = applyTextOptions(*parsed, model, document_.annotationScale(), text); !status) {
        return status.error();
    }
    auto attributes = document_.currentAttributes();
    if (auto status = document_.execute(cmd::createText(text, attributes)); !status) {
        return status.error();
    }
    const auto created = document_.lastCreatedEntities();
    return "created text id=" + (created.empty() ? std::string("?") : std::to_string(created.front())) +
           " style=" + field(text.style) + " paper=" + real(text.paperHeight) +
           " justify=" + std::string(katana::entity::toString(text.justify)) +
           " height=" + real(text.height);
}

CommandInterpreter::Reply CommandInterpreter::textEdit(const Tokens& args)
{
    auto parsed = splitArguments(args, 0, kTextKeys);
    if (!parsed) {
        return parsed.error();
    }
    if (parsed->positional.size() != 1 || parsed->options.empty()) {
        return usageOf("TEXTEDIT id [text= style= paper= justify= rotation= height= at=x,y]");
    }
    auto id = idOf(parsed->positional.front());
    if (!id) {
        return id.error();
    }
    const auto& model = document_.model();
    const Entity* entity = model.entities.find(*id);
    const auto* current =
        entity != nullptr ? std::get_if<katana::entity::TextGeometry>(&entity->geometry) : nullptr;
    if (current == nullptr) {
        return makeError(ErrorCode::InvalidArgument, "that entity is not a text",
                         "id=" + std::to_string(*id));
    }
    katana::entity::TextGeometry text = *current;
    if (const std::string* at = parsed->find("at")) {
        auto point = parsePoint(*at);
        if (!point) {
            return point.error();
        }
        text.position = *point;
    }
    if (auto status = applyTextOptions(*parsed, model, document_.annotationScale(), text); !status) {
        return status.error();
    }
    return finish(document_.execute(cmd::setEntityGeometry(*id, text)),
                  "updated text id=" + std::to_string(*id));
}

// ---- LABELSTYLE -------------------------------------------------------------------------------

CommandInterpreter::Reply CommandInterpreter::labelStyle(const Tokens& args)
{
    const auto& model = document_.model();
    const std::string action = args.empty() ? "LIST" : upperCase(args.front());
    if (action == "LIST") {
        std::string out;
        model.labelStyles.forEach([&](const katana::entity::LabelStyle& style) {
            out += out.empty() ? "" : "\n";
            out += describeLabelStyle(style);
        });
        return out;
    }
    if (action == "DEFAULTS") {
        // The standard set, those not there yet, as one step.
        auto transaction = std::make_unique<cmd::Transaction>("LABELSTYLE_DEFAULTS");
        std::string added;
        for (const katana::entity::LabelStyle& style : ann::defaultLabelStyles()) {
            if (model.labelStyles.contains(style.name)) {
                continue;
            }
            added += added.empty() ? "" : ",";
            added += field(style.name);
            transaction->add(cmd::createLabelStyle(style));
        }
        if (transaction->size() == 0) {
            return std::string("added=0");
        }
        const std::size_t count = transaction->size();
        return finish(document_.execute(std::move(transaction)),
                      "added=" + std::to_string(count) + " styles=" + added);
    }
    if (args.size() < 2) {
        return usageOf("LABELSTYLE LIST | NEW name kind= [opts] | SET name opts | DELETE name | "
                       "DEFAULTS | VALUES kind | CHECK kind template");
    }
    const std::string& name = args[1];
    if (action == "VALUES") {
        auto kind = katana::entity::labelKindFromString(name);
        if (!kind) {
            return kind.error();
        }
        std::string out = "kind=" + std::string(katana::entity::toString(*kind)) +
                          " values=id,layer,code,point,description";
        for (const std::string_view value : katana::entity::labelValueNames(*kind)) {
            out += ",";
            out += value;
        }
        return out + ",prop.NAME";
    }
    if (action == "CHECK") {
        auto kind = katana::entity::labelKindFromString(name);
        if (!kind) {
            return kind.error();
        }
        if (args.size() < 3) {
            return usageOf("LABELSTYLE CHECK kind \"template\"");
        }
        if (auto status = katana::entity::checkLabelTemplate(unescape(args[2]), *kind); !status) {
            return status.error();
        }
        return std::string("valid=yes");
    }
    if (action == "DELETE") {
        return finish(document_.execute(cmd::deleteLabelStyle(name)),
                      "deleted label style name=" + field(name));
    }
    auto parsed = splitArguments(args, 2, kLabelStyleKeys);
    if (!parsed) {
        return parsed.error();
    }
    if (action == "NEW") {
        katana::entity::LabelStyle style;
        style.name = name;
        if (const std::string* kind = parsed->find("kind")) {
            auto value = katana::entity::labelKindFromString(*kind);
            if (!value) {
                return value.error();
            }
            style.kind = *value;
        }
        style.text = defaultTemplate(style.kind);
        if (style.kind == katana::entity::LabelKind::Area) {
            style.placement = katana::entity::LabelPlacement::Centroid;
        }
        if (auto status = applyLabelStyleOptions(*parsed, style); !status) {
            return status.error();
        }
        return finish(document_.execute(cmd::createLabelStyle(style)),
                      "created label style " + describeLabelStyle(style));
    }
    if (action == "SET") {
        const auto* current = model.labelStyles.find(name);
        if (current == nullptr) {
            return makeError(ErrorCode::NotFound, "label style does not exist", name);
        }
        if (parsed->has("kind")) {
            return makeError(ErrorCode::InvalidArgument,
                             "a label style's kind is fixed; make a new style for another kind",
                             name);
        }
        katana::entity::LabelStyle style = *current;
        if (auto status = applyLabelStyleOptions(*parsed, style); !status) {
            return status.error();
        }
        return finish(document_.execute(cmd::updateLabelStyle(style)),
                      "updated label style " + describeLabelStyle(style));
    }
    if (action == "INFO") {
        const auto* current = model.labelStyles.find(name);
        if (current == nullptr) {
            return makeError(ErrorCode::NotFound, "label style does not exist", name);
        }
        return describeLabelStyle(*current);
    }
    return usageOf("LABELSTYLE LIST | NEW | SET | DELETE | INFO | DEFAULTS | VALUES | CHECK");
}

// ---- LABEL -----------------------------------------------------------------------------------

CommandInterpreter::Reply CommandInterpreter::label(const Tokens& args)
{
    const auto& model = document_.model();
    const std::string action = args.empty() ? "" : upperCase(args.front());
    if (action == "LIST") {
        std::set<EntityId> only;
        for (std::size_t i = 1; i < args.size(); ++i) {
            auto id = idOf(args[i]);
            if (!id) {
                return id.error();
            }
            only.insert(*id);
        }
        std::string out;
        model.entities.forEach([&](const Entity& entity) {
            const auto* label = std::get_if<katana::entity::LabelGeometry>(&entity.geometry);
            if (label == nullptr || (!only.empty() && !only.contains(entity.id))) {
                return;
            }
            std::ostringstream line;
            line << "id=" << entity.id << " style=" << field(label->style);
            if (label->target != 0) {
                line << " target=" << label->target;
            } else {
                line << " alignment=" << field(label->alignment);
            }
            line << " part=" << label->part << " layer=" << field(entity.layer)
                 << " rule=" << field(label->rule);
            if (label->position) {
                line << " at=" << real(label->position->x) << "," << real(label->position->y);
            }
            line << " text=" << field(labelTexts(model, *label));
            out += out.empty() ? "" : "\n";
            out += line.str();
        });
        return out;
    }
    if (action == "DELETE") {
        std::vector<EntityId> ids;
        for (std::size_t i = 1; i < args.size(); ++i) {
            auto id = idOf(args[i]);
            if (!id) {
                return id.error();
            }
            const Entity* entity = model.entities.find(*id);
            if (entity == nullptr ||
                !std::holds_alternative<katana::entity::LabelGeometry>(entity->geometry)) {
                return makeError(ErrorCode::InvalidArgument, "that entity is not a label",
                                 "id=" + std::to_string(*id));
            }
            ids.push_back(*id);
        }
        if (ids.empty()) {
            return usageOf("LABEL DELETE id [id...]");
        }
        return finish(document_.execute(cmd::deleteEntities(ids)),
                      "deleted labels=" + std::to_string(ids.size()));
    }
    if (action == "LAYOUT") {
        auto parsed = splitArguments(args, 1, {"scale", "collisions"});
        if (!parsed) {
            return parsed.error();
        }
        ann::LabelLayoutOptions options;
        options.scale = document_.annotationScale();
        if (const std::string* scale = parsed->find("scale")) {
            auto value = numberOf(*scale, "scale");
            if (!value) {
                return value.error();
            }
            options.scale = *value;
        }
        if (const std::string* collisions = parsed->find("collisions")) {
            auto value = switchOf(*collisions, "collisions");
            if (!value) {
                return value.error();
            }
            options.avoidCollisions = *value;
        }
        // What the plan view keeps labels out of, over the whole drawing, so
        // this reply is where a view at this scale puts them.
        if (options.avoidCollisions) {
            options.linework = ann::labelKeepOut(model, options.scale, options.measure,
                                                 kLayoutLineworkLimit);
        }
        const ann::LabelLayout layout = ann::layoutLabels(model, ann::labelEntities(model), options);
        std::ostringstream out;
        out << "scale=" << real(options.scale) << " considered=" << layout.considered
            << " placed=" << layout.placed.size() << " displaced=" << layout.displaced
            << " suppressed=" << layout.suppressed << " orphaned=" << layout.orphaned;
        for (const ann::PlacedLabel& placed : layout.placed) {
            Box2 box;
            for (const auto& corners : placed.drawing.textBoxes) {
                for (const Point2& p : corners) {
                    box.expand(p);
                }
            }
            std::string text;
            for (const auto& run : placed.drawing.texts) {
                text += text.empty() ? "" : "\n";
                text += run.text;
            }
            const Point2 centre = box.empty() ? Point2{} : box.center();
            out << "\nlabel=" << placed.label << " piece=" << placed.piece << " x=" << real(centre.x)
                << " y=" << real(centre.y) << " candidate=" << placed.candidate
                << " displaced=" << (placed.displaced ? "yes" : "no") << " text=" << field(text);
        }
        return out.str();
    }
    if (action == "SET") {
        auto parsed = splitArguments(args, 2, {"style", "at", "text", "layer"});
        if (!parsed || args.size() < 3) {
            return parsed ? Reply(usageOf("LABEL SET id [style= at=x,y|none text=|none layer=]"))
                          : Reply(parsed.error());
        }
        auto id = idOf(args[1]);
        if (!id) {
            return id.error();
        }
        const Entity* entity = model.entities.find(*id);
        const auto* current = entity != nullptr
                                  ? std::get_if<katana::entity::LabelGeometry>(&entity->geometry)
                                  : nullptr;
        if (current == nullptr) {
            return makeError(ErrorCode::InvalidArgument, "that entity is not a label",
                             "id=" + std::to_string(*id));
        }
        Entity changed = *entity;
        auto& label = std::get<katana::entity::LabelGeometry>(changed.geometry);
        if (const std::string* style = parsed->find("style")) {
            label.style = *style;
        }
        if (const std::string* text = parsed->find("text")) {
            label.textOverride = lowerCase(*text) == "none" ? std::string() : unescape(*text);
        }
        if (const std::string* at = parsed->find("at")) {
            if (lowerCase(*at) == "none") {
                label.position.reset();
            } else {
                auto point = parsePoint(*at);
                if (!point) {
                    return point.error();
                }
                label.position = *point;
            }
        }
        if (const std::string* layer = parsed->find("layer")) {
            changed.layer = *layer;
        }
        cmd::ChangeSet changes;
        changes.modify.push_back(std::move(changed));
        return finish(document_.execute(std::make_unique<cmd::ChangeSetCommand>(
                          "SET_LABEL",
                          [changes](const cmd::CommandContext&) -> Result<cmd::ChangeSet> {
                              return changes;
                          })),
                      "updated label id=" + std::to_string(*id));
    }

    // Making labels: LABEL id... | LABEL SELECTION | LABEL ALIGN name.
    auto parsed = splitArguments(args, 0, {"style", "part", "layer", "at", "text"});
    if (!parsed) {
        return parsed.error();
    }
    const std::string* style = parsed->find("style");
    if (style == nullptr || parsed->positional.empty()) {
        return usageOf("LABEL id [id...] style= [part=N layer= at=x,y text=] | LABEL SELECTION "
                       "style= | LABEL ALIGN name style=");
    }
    ann::LabelRequest request;
    request.style = *style;
    if (const std::string* layer = parsed->find("layer")) {
        request.layer = *layer;
    }
    if (const std::string* text = parsed->find("text")) {
        request.textOverride = unescape(*text);
    }
    if (const std::string* part = parsed->find("part")) {
        auto value = integerOf(*part, "part");
        if (!value) {
            return value.error();
        }
        request.part = *value;
    }
    if (const std::string* at = parsed->find("at")) {
        auto point = parsePoint(*at);
        if (!point) {
            return point.error();
        }
        request.position = *point;
    }
    std::vector<ann::LabelRequest> requests;
    const std::string first = upperCase(parsed->positional.front());
    if (first == "ALIGN") {
        if (parsed->positional.size() != 2) {
            return usageOf("LABEL ALIGN name style=");
        }
        request.alignment = parsed->positional[1];
        requests.push_back(request);
    } else if (first == "SELECTION") {
        for (const EntityId id : document_.selection().ids()) {
            request.target = id;
            requests.push_back(request);
        }
        if (requests.empty()) {
            return makeError(ErrorCode::InvalidState, "nothing is selected; use SELECT first");
        }
    } else {
        for (const std::string& token : parsed->positional) {
            auto id = idOf(token);
            if (!id) {
                return id.error();
            }
            request.target = *id;
            requests.push_back(request);
        }
    }
    // Every label as one step: one UNDO takes back the lot.
    auto transaction = std::make_unique<cmd::Transaction>("CREATE_LABEL");
    for (const ann::LabelRequest& one : requests) {
        auto built = ann::createLabel(model, one);
        if (!built) {
            return makeError(built.error().code, built.error().message,
                             (one.target != 0 ? "id=" + std::to_string(one.target)
                                              : "alignment=" + one.alignment) +
                                 " " + built.error().context);
        }
        transaction->add(std::move(*built));
    }
    if (auto status = document_.execute(std::move(transaction)); !status) {
        return status.error();
    }
    const auto created = document_.lastCreatedEntities();
    return "created labels=" + std::to_string(created.size()) + " ids=" + idList(created);
}

// ---- AUTOLABEL -------------------------------------------------------------------------------

CommandInterpreter::Reply CommandInterpreter::autoLabel(const Tokens& args)
{
    const auto& model = document_.model();
    const std::string action = args.empty() ? "" : upperCase(args.front());
    if (action == "RULE") {
        const std::string sub = args.size() > 1 ? upperCase(args[1]) : "LIST";
        if (sub == "LIST") {
            std::string out;
            model.labelRules.forEach([&](const katana::entity::LabelRule& rule) {
                out += out.empty() ? "" : "\n";
                out += describeRule(rule);
            });
            return out;
        }
        if (args.size() < 3) {
            return usageOf("AUTOLABEL RULE ADD name style= [opts] | SET name opts | DELETE name | "
                           "LIST");
        }
        const std::string& name = args[2];
        if (sub == "DELETE") {
            return finish(document_.execute(cmd::deleteLabelRule(name)),
                          "deleted rule name=" + field(name));
        }
        auto parsed = splitArguments(args, 3, kRuleKeys);
        if (!parsed) {
            return parsed.error();
        }
        if (sub == "ADD" || sub == "NEW") {
            katana::entity::LabelRule rule;
            rule.name = name;
            if (auto status = applyRuleOptions(*parsed, rule); !status) {
                return status.error();
            }
            if (!model.labelStyles.contains(rule.labelStyle)) {
                return makeError(ErrorCode::NotFound, "label style does not exist",
                                 rule.labelStyle);
            }
            return finish(document_.execute(cmd::createLabelRule(rule)),
                          "created rule " + describeRule(rule));
        }
        if (sub == "SET") {
            const auto* current = model.labelRules.find(name);
            if (current == nullptr) {
                return makeError(ErrorCode::NotFound, "label rule does not exist", name);
            }
            katana::entity::LabelRule rule = *current;
            if (auto status = applyRuleOptions(*parsed, rule); !status) {
                return status.error();
            }
            if (!model.labelStyles.contains(rule.labelStyle)) {
                return makeError(ErrorCode::NotFound, "label style does not exist",
                                 rule.labelStyle);
            }
            return finish(document_.execute(cmd::updateLabelRule(rule)),
                          "updated rule " + describeRule(rule));
        }
        return usageOf("AUTOLABEL RULE ADD | SET | DELETE | LIST");
    }
    const std::vector<std::string> rules(args.size() > 1 ? args.begin() + 1 : args.end(), args.end());
    if (action == "RUN" || action == "PREVIEW") {
        ann::AutoLabelReport report;
        auto built = ann::autoLabel(model, rules, &report);
        if (!built) {
            return built.error();
        }
        std::ostringstream out;
        out << (action == "RUN" ? "autolabel" : "preview") << " created=" << report.created
            << " kept=" << report.kept << " removed=" << report.removed
            << " skipped=" << report.skipped;
        for (const auto& [rule, count] : report.perRule) {
            out << "\nrule=" << field(rule) << " labels=" << count;
        }
        if (action == "RUN" && *built) {
            if (auto status = document_.execute(std::move(*built)); !status) {
                return status.error();
            }
        }
        return out.str();
    }
    if (action == "CLEAR") {
        std::size_t removed = 0;
        auto built = ann::clearAutoLabels(model, rules, &removed);
        return runBuilt(document_, std::move(built), "removed=" + std::to_string(removed),
                        "removed=0");
    }
    return usageOf("AUTOLABEL RULE ... | RUN [rule...] | PREVIEW [rule...] | CLEAR [rule...]");
}

// ---- DIM --------------------------------------------------------------------------------------

CommandInterpreter::Reply CommandInterpreter::dimension(const Tokens& args)
{
    const auto& model = document_.model();
    const std::string kind = upperCase(args.front());
    auto parsed = splitArguments(args, 1, {"at", "angle", "datum", "axis", "spacing"});
    if (!parsed) {
        return parsed.error();
    }
    const auto& positional = parsed->positional;
    const auto pointOption = [&](std::string_view key) -> Result<std::optional<Point2>> {
        const std::string* text = parsed->find(key);
        if (text == nullptr) {
            return std::optional<Point2>{};
        }
        auto point = parseAnchoredPoint(*text);
        if (!point) {
            return point.error();
        }
        return std::optional<Point2>(point->point);
    };
    const auto points = [&](std::size_t from) -> Result<std::vector<ann::AnchoredPoint>> {
        std::vector<ann::AnchoredPoint> list;
        for (std::size_t i = from; i < positional.size(); ++i) {
            auto point = parseAnchoredPoint(positional[i]);
            if (!point) {
                return point.error();
            }
            list.push_back(*point);
        }
        return list;
    };
    const auto created = [&](const katana::entity::DimensionGeometry& dimension) -> Reply {
        if (auto status = document_.execute(
                cmd::createDimension(dimension, document_.currentAttributes()));
            !status) {
            return status.error();
        }
        const auto ids = document_.lastCreatedEntities();
        const Entity* entity = ids.empty() ? nullptr : document_.model().entities.find(ids.front());
        const std::string text =
            entity != nullptr
                ? dimensionText(dimension, resolveDimensionStyle(document_.model(), *entity))
                : std::string();
        return "created dimension id=" + idList(ids) + " kind=" +
               std::string(katana::entity::toString(dimension.kind)) +
               " measures=" + real(dimension.measurement()) + " text=" + field(text) +
               " associative=" +
               (dimension.startRef.associated() || dimension.endRef.associated() ||
                        dimension.vertexRef.associated()
                    ? "yes"
                    : "no");
    };

    auto at = pointOption("at");
    if (!at) {
        return at.error();
    }
    if (kind == "LINEAR" || kind == "HORIZONTAL" || kind == "VERTICAL" || kind == "ALIGNED") {
        auto list = points(0);
        if (!list) {
            return list.error();
        }
        if (list->size() != 2 || !*at) {
            return usageOf("DIM " + kind + " p p at=p" + (kind == "LINEAR" ? " [angle=deg]" : ""));
        }
        std::optional<double> angle;
        if (kind == "HORIZONTAL") {
            angle = 0.0;
        } else if (kind == "VERTICAL") {
            angle = 0.5 * katana::math::kPi;
        } else if (const std::string* text = parsed->find("angle")) {
            auto value = numberOf(*text, "angle");
            if (!value) {
                return value.error();
            }
            angle = *value * katana::math::kDegToRad;
        }
        auto made = kind == "ALIGNED" ? ann::alignedDimension((*list)[0], (*list)[1], **at)
                                      : ann::linearDimension((*list)[0], (*list)[1], **at, angle);
        if (!made) {
            return made.error();
        }
        return created(*made);
    }
    if (kind == "ANGULAR") {
        // Two ids: the angle between two lines.
        if (positional.size() == 2) {
            auto first = idOf(positional[0]);
            auto second = idOf(positional[1]);
            if (!first || !second || !*at) {
                return usageOf("DIM ANGULAR line-id line-id at=p");
            }
            auto made = ann::angularBetweenLines(model, *first, *second, **at);
            if (!made) {
                return made.error();
            }
            return created(*made);
        }
        auto list = points(0);
        if (!list) {
            return list.error();
        }
        if (list->size() != 3) {
            return usageOf("DIM ANGULAR vertex p p [at=p]");
        }
        auto made = ann::angularDimension((*list)[0], (*list)[1], (*list)[2], *at);
        if (!made) {
            return made.error();
        }
        return created(*made);
    }
    if (kind == "RADIUS" || kind == "DIAMETER") {
        if (positional.size() != 1) {
            return usageOf("DIM " + kind + " circle-id [at=p]");
        }
        auto id = idOf(positional[0]);
        if (!id) {
            return id.error();
        }
        Point2 through;
        if (*at) {
            through = **at;
        } else {
            // Up and to the right of the curve, a third of its radius out.
            const Entity* entity = model.entities.find(*id);
            if (entity == nullptr) {
                return makeError(ErrorCode::NotFound, "entity does not exist",
                                 "id=" + std::to_string(*id));
            }
            const auto* circle = std::get_if<katana::geometry::Circle2>(&entity->geometry);
            const auto* arc = std::get_if<katana::geometry::Arc2>(&entity->geometry);
            if (circle != nullptr) {
                through = circle->center + Vec2(1.0, 1.0).normalized() * (circle->radius * 1.33);
            } else if (arc != nullptr) {
                through = arc->center + (arc->pointAt(0.5) - arc->center) * 1.33;
            }
        }
        auto made = ann::radialDimension(model, *id, through, kind == "DIAMETER");
        if (!made) {
            return made.error();
        }
        return created(*made);
    }
    if (kind == "ORDINATE") {
        auto list = points(0);
        if (!list) {
            return list.error();
        }
        if (list->size() != 1 || !*at) {
            return usageOf("DIM ORDINATE p at=p [datum=p axis=x|y]");
        }
        ann::AnchoredPoint datum{Point2(0.0, 0.0), {}};
        if (const std::string* text = parsed->find("datum")) {
            auto point = parseAnchoredPoint(*text);
            if (!point) {
                return point.error();
            }
            datum = *point;
        }
        std::optional<bool> xAxis;
        if (const std::string* axis = parsed->find("axis")) {
            const std::string a = lowerCase(*axis);
            if (a != "x" && a != "y") {
                return usageOf("DIM ORDINATE ... axis=x|y");
            }
            xAxis = a == "x";
        }
        auto made = ann::ordinateDimension(datum, list->front(), **at, xAxis);
        if (!made) {
            return made.error();
        }
        return created(*made);
    }
    if (kind == "BASELINE" || kind == "CONTINUE") {
        if (positional.size() < 2) {
            return usageOf("DIM " + kind + " dim-id p [p...]" +
                           (kind == "BASELINE" ? " [spacing=]" : ""));
        }
        auto id = idOf(positional[0]);
        if (!id) {
            return id.error();
        }
        const Entity* base = model.entities.find(*id);
        const auto* dimension = base != nullptr
                                    ? std::get_if<katana::entity::DimensionGeometry>(&base->geometry)
                                    : nullptr;
        if (dimension == nullptr) {
            return makeError(ErrorCode::InvalidArgument, "that entity is not a dimension",
                             "id=" + std::to_string(*id));
        }
        auto list = points(1);
        if (!list) {
            return list.error();
        }
        Result<std::vector<katana::entity::DimensionGeometry>> chain =
            std::vector<katana::entity::DimensionGeometry>{};
        if (kind == "BASELINE") {
            // A text height and a half apart unless told: AutoCAD's metric
            // DIMDLI of 3.75 against a DIMTXT of 2.5.
            const auto style = dimensionStyleAtScale(resolveDimensionStyle(model, *base),
                                                     document_.annotationScale());
            double spacing = 1.5 * style.textHeight;
            if (const std::string* text = parsed->find("spacing")) {
                auto value = numberOf(*text, "spacing");
                if (!value) {
                    return value.error();
                }
                spacing = *value;
            }
            chain = ann::baselineDimensions(*dimension, *list, spacing);
        } else {
            chain = ann::continuedDimensions(*dimension, *list);
        }
        if (!chain) {
            return chain.error();
        }
        // The whole chain as one step, on the base's layer and style.
        cmd::ChangeSet changes;
        for (auto& geometry : *chain) {
            Entity entity;
            entity.geometry = std::move(geometry);
            entity.layer = base->layer;
            entity.style = base->style;
            entity.color = base->color;
            changes.add.push_back(std::move(entity));
        }
        if (auto status = document_.execute(std::make_unique<cmd::ChangeSetCommand>(
                kind == "BASELINE" ? "DIM_BASELINE" : "DIM_CONTINUE",
                [changes](const cmd::CommandContext&) -> Result<cmd::ChangeSet> {
                    return changes;
                }));
            !status) {
            return status.error();
        }
        const auto ids = document_.lastCreatedEntities();
        return "created dimensions=" + std::to_string(ids.size()) + " ids=" + idList(ids);
    }
    return usageOf("DIM LINEAR|HORIZONTAL|VERTICAL|ALIGNED|ANGULAR|RADIUS|DIAMETER|ORDINATE|"
                   "BASELINE|CONTINUE ...");
}

// ---- LEADER, BALLOON --------------------------------------------------------------------------

namespace {

constexpr const char* kLeaderUsage =
    "LEADER p p [p...] [text= | template= | labelstyle=] [arrow= callout= style= paper= "
    "arrowsize= landing=] | LEADER LIST | VALUES | SET | ATTACH | DETACH | FREEZE | PROP | FOR "
    "| ALIGN (HELP says each)";

// The long names and the short ones a person types.
Result<katana::entity::ArrowHead> arrowOf(const std::string& value)
{
    auto head = katana::entity::arrowHeadFromString(value);
    if (head) {
        return *head;
    }
    const std::string v = lowerCase(value);
    if (v == "closed" || v == "filled") {
        return katana::entity::ArrowHead::ClosedFilled;
    }
    if (v == "open") {
        return katana::entity::ArrowHead::Open;
    }
    if (v == "tick") {
        return katana::entity::ArrowHead::Tick;
    }
    if (v == "dot") {
        return katana::entity::ArrowHead::Dot;
    }
    if (v == "none") {
        return katana::entity::ArrowHead::None;
    }
    return head.error();
}

// One more than the highest balloon numbered: a circled plain leader whose
// note is a whole number. A smart balloon's note is read off its target and
// is no number in the sequence.
long long nextBalloonNumber(const katana::entity::Model& model)
{
    long long highest = 0;
    model.entities.forEach([&](const Entity& entity) {
        const auto* other = std::get_if<katana::entity::LeaderGeometry>(&entity.geometry);
        if (other == nullptr || other->callout != katana::entity::CalloutShape::Circle ||
            katana::entity::isSmart(*other)) {
            return;
        }
        long long value = 0;
        const auto [end, error] =
            std::from_chars(other->text.data(), other->text.data() + other->text.size(), value);
        if (error == std::errc{} && end == other->text.data() + other->text.size()) {
            highest = std::max(highest, value);
        }
    });
    return highest + 1;
}

// What a leader's note is: at most one of text= (literal), template= (read
// off the tip's entity), labelstyle= (that style's template) and, for a
// balloon, n=. A label style given with no style= or paper= lends the leader
// its text style and paper height, so the leader looks as the style's labels
// do when it is made; after that the look is the leader's own.
Status applyLeaderNote(const Arguments& args, const katana::entity::Model& model,
                       katana::entity::LeaderGeometry& leader)
{
    int given = 0;
    for (const char* key : {"text", "template", "labelstyle", "n"}) {
        given += args.has(key) ? 1 : 0;
    }
    if (given > 1) {
        return makeError(ErrorCode::InvalidArgument,
                         "a leader's note is one of text=, template= or labelstyle=");
    }
    for (const char* key : {"text", "n"}) {
        if (const std::string* text = args.find(key)) {
            leader.text = unescape(*text);
            leader.fields = false;
            leader.labelStyle.clear();
        }
    }
    if (const std::string* templateText = args.find("template")) {
        std::string text = unescape(*templateText);
        if (auto status = katana::entity::checkLeaderTemplate(text); !status) {
            return status;
        }
        leader.text = std::move(text);
        leader.fields = true;
        leader.labelStyle.clear();
    }
    if (const std::string* name = args.find("labelstyle")) {
        const katana::entity::LabelStyle* style = model.labelStyles.find(*name);
        if (style == nullptr) {
            return makeError(ErrorCode::NotFound, "label style does not exist", *name);
        }
        leader.labelStyle = *name;
        leader.text.clear();
        leader.fields = false;
        if (!args.has("style") && !style->textStyle.empty()) {
            leader.style = style->textStyle;
        }
        if (!args.has("paper") && style->paperHeight > 0.0) {
            leader.paperHeight = style->paperHeight;
        }
    }
    return {};
}

// What a leader looks like: arrow=, callout=, style=, paper=, arrowsize=,
// landing=.
Status applyLeaderLook(const Arguments& args, const katana::entity::Model& model,
                       katana::entity::LeaderGeometry& leader)
{
    if (const std::string* value = args.find("arrow")) {
        auto head = arrowOf(*value);
        if (!head) {
            return head.error();
        }
        leader.arrow = *head;
    }
    if (const std::string* value = args.find("callout")) {
        auto shape = katana::entity::calloutShapeFromString(*value);
        if (!shape) {
            return shape.error();
        }
        leader.callout = *shape;
    }
    if (const std::string* value = args.find("style")) {
        if (!model.textStyles.contains(*value)) {
            return makeError(ErrorCode::NotFound, "text style does not exist", *value);
        }
        leader.style = *value;
    }
    for (const char* key : {"paper", "arrowsize", "landing"}) {
        if (const std::string* value = args.find(key)) {
            auto number = numberOf(*value, key);
            if (!number) {
                return number.error();
            }
            (std::string_view(key) == "paper"       ? leader.paperHeight
             : std::string_view(key) == "arrowsize" ? leader.arrowSize
                                                    : leader.landing) = *number;
        }
    }
    return {};
}

// A smart leader that would say nothing is refused, naming what its target
// lacks (entity::checkLeaderSaysSomething), the survey code looked for where
// survey coding looks.
Status requireNote(const katana::entity::Model& model, const katana::entity::LeaderGeometry& leader)
{
    return katana::entity::checkLeaderSaysSomething(model, leader, codePropertyCandidates());
}

// One leader as a record: where it is, what it is on, what it says.
std::string describeLeader(const katana::entity::Model& model, const Entity& entity)
{
    const auto& leader = std::get<katana::entity::LeaderGeometry>(entity.geometry);
    const Point2& tip = leader.vertices.front();
    std::ostringstream line;
    line << "id=" << entity.id << " layer=" << field(entity.layer)
         << " vertices=" << leader.vertices.size() << " tip=" << real(tip.x) << "," << real(tip.y);
    if (leader.tipRef.associated()) {
        line << " target=" << leader.tipRef.entity
             << " anchor=" << field(katana::entity::describe(leader.tipRef));
    }
    line << " smart=" << (katana::entity::isSmart(leader) ? "yes" : "no");
    if (leader.fields) {
        line << " template=" << field(leader.text);
    }
    if (!leader.labelStyle.empty()) {
        line << " labelstyle=" << field(leader.labelStyle);
    }
    line << " arrow=" << katana::entity::toString(leader.arrow)
         << " callout=" << katana::entity::toString(leader.callout)
         << " text=" << field(katana::entity::leaderNote(model, leader, codePropertyCandidates()));
    return line.str();
}

// Where LEADER FOR puts a tip on `entity`: a point's or a text's position,
// the middle of a line or an arc, halfway along an open polyline, inside a
// closed one (a lot), and on a circle on the side the note goes - `angle`,
// the direction from the tip to the note, radians. nullopt for what offers
// no place (a dimension, a label, a leader).
std::optional<katana::entity::AnchorRef> naturalAnchor(const Entity& entity, double angle)
{
    using katana::entity::AnchorPoint;
    katana::entity::AnchorRef ref;
    ref.entity = entity.id;
    ref.point = AnchorPoint::Along;
    const auto& geometry = entity.geometry;
    if (std::holds_alternative<katana::entity::PointGeometry>(geometry) ||
        std::holds_alternative<katana::entity::TextGeometry>(geometry)) {
        ref.point = AnchorPoint::Position;
        return ref;
    }
    if (std::holds_alternative<katana::geometry::Segment2>(geometry) ||
        std::holds_alternative<katana::geometry::Arc2>(geometry)) {
        ref.parameter = 0.5;
        return ref;
    }
    if (std::holds_alternative<katana::geometry::Circle2>(geometry)) {
        const double turn = katana::math::normalizeAngle(angle) / katana::math::kTwoPi;
        ref.parameter = turn < 1.0 ? turn : 0.0;
        return ref;
    }
    if (const auto* polyline = std::get_if<katana::geometry::Polyline2>(&geometry)) {
        const auto& v = polyline->vertices;
        if (polyline->closed && v.size() >= 3) {
            ref.point = AnchorPoint::Inside;
            return ref;
        }
        const double half = 0.5 * polyline->length();
        if (!(half > 0.0) || v.size() < 2) {
            return std::nullopt;
        }
        double before = 0.0;
        for (std::size_t i = 0; i + 1 < v.size(); ++i) {
            const double length = v[i].distanceTo(v[i + 1]);
            if (length > 0.0 && (before + length >= half || i + 2 == v.size())) {
                ref.index = static_cast<std::uint32_t>(i);
                ref.parameter = std::clamp((half - before) / length, 0.0, 1.0);
                return ref;
            }
            before += length;
        }
        return std::nullopt;
    }
    return std::nullopt;
}

} // namespace

CommandInterpreter::Reply CommandInterpreter::leader(const std::string& verb, const Tokens& args)
{
    const bool balloon = verb == "BALLOON";
    const auto& model = document_.model();
    const std::string action = args.empty() ? std::string() : upperCase(args.front());

    // The leaders `tokens` name: ids, or SELECTION for the selected ones.
    // In id order, each once.
    const auto leadersOf =
        [&](const std::vector<std::string>& tokens) -> Result<std::vector<EntityId>> {
        std::vector<EntityId> ids;
        if (tokens.size() == 1 && upperCase(tokens.front()) == "SELECTION") {
            for (const EntityId id : document_.selection().ids()) {
                const Entity* entity = model.entities.find(id);
                if (entity != nullptr &&
                    std::holds_alternative<katana::entity::LeaderGeometry>(entity->geometry)) {
                    ids.push_back(id);
                }
            }
            if (ids.empty()) {
                return makeError(ErrorCode::InvalidState,
                                 "no leader is selected; use SELECT first");
            }
            return ids;
        }
        for (const std::string& token : tokens) {
            auto id = idOf(token);
            if (!id) {
                return id.error();
            }
            const Entity* entity = model.entities.find(*id);
            if (entity == nullptr ||
                !std::holds_alternative<katana::entity::LeaderGeometry>(entity->geometry)) {
                return makeError(ErrorCode::InvalidArgument, "that entity is not a leader",
                                 "id=" + std::to_string(*id));
            }
            ids.push_back(*id);
        }
        std::sort(ids.begin(), ids.end());
        ids.erase(std::unique(ids.begin(), ids.end()), ids.end());
        return ids;
    };
    // `changes` as one step named `name`.
    const auto apply = [&](const char* name, cmd::ChangeSet changes) {
        return document_.execute(std::make_unique<cmd::ChangeSetCommand>(
            name,
            [changes = std::move(changes)](const cmd::CommandContext&) -> Result<cmd::ChangeSet> {
                return changes;
            }));
    };
    const auto leaderOf = [&](EntityId id) -> const katana::entity::LeaderGeometry& {
        return std::get<katana::entity::LeaderGeometry>(model.entities.find(id)->geometry);
    };

    if (!balloon && action == "LIST") {
        auto parsed = splitArguments(args, 1, {"target"});
        if (!parsed) {
            return parsed.error();
        }
        std::set<EntityId> only;
        for (const std::string& token : parsed->positional) {
            auto id = idOf(token);
            if (!id) {
                return id.error();
            }
            only.insert(*id);
        }
        std::optional<EntityId> target;
        if (const std::string* value = parsed->find("target")) {
            auto id = idOf(*value);
            if (!id) {
                return id.error();
            }
            target = *id;
        }
        std::string out;
        model.entities.forEach([&](const Entity& entity) {
            const auto* leader = std::get_if<katana::entity::LeaderGeometry>(&entity.geometry);
            if (leader == nullptr || (!only.empty() && !only.contains(entity.id)) ||
                (target && leader->tipRef.entity != *target)) {
                return;
            }
            out += out.empty() ? "" : "\n";
            out += describeLeader(model, entity);
        });
        return out;
    }

    if (!balloon && action == "VALUES") {
        if (args.size() != 2 || args[1].empty()) {
            return usageOf("LEADER VALUES id | #id[.point] | #id@x,y");
        }
        katana::entity::LabelValues values;
        std::ostringstream out;
        const auto asLeader = idOf(args[1]);
        const Entity* named = asLeader ? model.entities.find(*asLeader) : nullptr;
        if (named != nullptr &&
            std::holds_alternative<katana::entity::LeaderGeometry>(named->geometry)) {
            const auto& leader = std::get<katana::entity::LeaderGeometry>(named->geometry);
            if (!leader.tipRef.associated()) {
                return makeError(ErrorCode::InvalidArgument,
                                 "that leader's tip is on no entity (LEADER ATTACH puts it on one)",
                                 "id=" + std::to_string(named->id));
            }
            auto read = katana::entity::leaderValues(model, leader, codePropertyCandidates());
            if (!read) {
                return makeError(ErrorCode::NotFound, "the entity the leader's tip is on is gone",
                                 "id=" + std::to_string(leader.tipRef.entity));
            }
            values = std::move(*read);
            out << "leader=" << named->id << " target=" << leader.tipRef.entity;
        } else {
            // A place on an entity: what a leader there could say.
            auto place = parseAnchoredPoint(args[1].front() == '#' ? args[1] : "#" + args[1]);
            if (!place) {
                return place.error();
            }
            const Entity* target = model.entities.find(place->ref.entity);
            values = katana::entity::anchorValues(*target, place->ref, place->point,
                                                  codePropertyCandidates());
            out << "target=" << target->id;
        }
        const auto& type = values.at("type").text;
        out << " type=" << type;
        const auto line = [&](const std::string& name, const katana::entity::LabelValue& value) {
            out << "\nvalue=" << field(name)
                << " text=" << field(katana::entity::formatValue(value));
        };
        for (const std::string_view name : katana::entity::leaderValueNames()) {
            if (const auto found = values.find(name); found != values.end()) {
                line(std::string(name), found->second);
            }
        }
        for (const auto& [name, value] : values) {
            if (name.starts_with("prop.")) {
                line(name, value);
            }
        }
        return out.str();
    }

    if (!balloon && action == "SET") {
        auto parsed = splitArguments(args, 1,
                                     {"text", "template", "labelstyle", "arrow", "callout", "style",
                                      "paper", "arrowsize", "landing", "tip", "at"});
        if (!parsed) {
            return parsed.error();
        }
        if (parsed->positional.empty() || parsed->options.empty()) {
            return usageOf("LEADER SET id [id...] | SELECTION [text= template= labelstyle= arrow= "
                           "callout= style= paper= arrowsize= landing= tip=p at=p]");
        }
        auto ids = leadersOf(parsed->positional);
        if (!ids) {
            return ids.error();
        }
        if (parsed->has("at") && ids->size() != 1) {
            return makeError(ErrorCode::InvalidArgument,
                             "at= moves one leader's note; name one leader");
        }
        std::optional<ann::AnchoredPoint> tip;
        if (const std::string* value = parsed->find("tip")) {
            auto point = parseAnchoredPoint(*value);
            if (!point) {
                return point.error();
            }
            tip = *point;
        }
        std::optional<Point2> at;
        if (const std::string* value = parsed->find("at")) {
            auto point = parsePoint(*value);
            if (!point) {
                return point.error();
            }
            at = *point;
        }
        cmd::ChangeSet changes;
        for (const EntityId id : *ids) {
            Entity changed = *model.entities.find(id);
            auto& leader = std::get<katana::entity::LeaderGeometry>(changed.geometry);
            if (auto status = applyLeaderNote(*parsed, model, leader); !status) {
                return status.error();
            }
            if (auto status = applyLeaderLook(*parsed, model, leader); !status) {
                return status.error();
            }
            if (tip) {
                if (tip->ref.entity == id) {
                    return makeError(ErrorCode::InvalidArgument,
                                     "a leader's tip cannot be on the leader itself",
                                     "id=" + std::to_string(id));
                }
                leader.vertices.front() = tip->point;
                leader.tipRef = tip->ref;
            }
            if (at) {
                leader.vertices.back() = *at;
            }
            if (auto status = requireNote(model, leader); !status) {
                return makeError(status.error().code, status.error().message,
                                 "leader=" + std::to_string(id) + " " + status.error().context);
            }
            changes.modify.push_back(std::move(changed));
        }
        if (auto status = apply("SET_LEADER", std::move(changes)); !status) {
            return status.error();
        }
        std::string out = "updated leaders=" + std::to_string(ids->size());
        for (const EntityId id : *ids) {
            if (const Entity* entity = model.entities.find(id)) {
                out += "\n" + describeLeader(model, *entity);
            }
        }
        return out;
    }

    if (!balloon && action == "ATTACH") {
        if (args.size() != 3) {
            return usageOf("LEADER ATTACH id #id[.point] | #id@x,y");
        }
        auto ids = leadersOf({args[1]});
        if (!ids) {
            return ids.error();
        }
        const EntityId id = ids->front();
        auto place = parseAnchoredPoint(args[2]);
        if (!place) {
            return place.error();
        }
        if (!place->ref.associated()) {
            return makeError(ErrorCode::InvalidArgument,
                             "ATTACH puts the tip on an entity: give #id, #id.end, #id.inside or "
                             "#id@x,y",
                             args[2]);
        }
        if (place->ref.entity == id) {
            return makeError(ErrorCode::InvalidArgument,
                             "a leader's tip cannot be on the leader itself",
                             "id=" + std::to_string(id));
        }
        Entity changed = *model.entities.find(id);
        auto& leader = std::get<katana::entity::LeaderGeometry>(changed.geometry);
        leader.vertices.front() = place->point;
        leader.tipRef = place->ref;
        if (auto status = requireNote(model, leader); !status) {
            return status.error();
        }
        cmd::ChangeSet changes;
        changes.modify.push_back(std::move(changed));
        if (auto status = apply("ATTACH_LEADER", std::move(changes)); !status) {
            return status.error();
        }
        return "attached leader " + describeLeader(model, *model.entities.find(id));
    }

    if (!balloon && (action == "DETACH" || action == "FREEZE")) {
        const bool detach = action == "DETACH";
        if (args.size() < 2) {
            return usageOf(detach ? "LEADER DETACH id [id...] | SELECTION"
                                  : "LEADER FREEZE id [id...] | SELECTION");
        }
        auto ids = leadersOf(Tokens(args.begin() + 1, args.end()));
        if (!ids) {
            return ids.error();
        }
        // FREEZE turns a smart note into the words it says now; DETACH lets
        // the tip go, freezing a smart note first, since a note read off
        // nothing would say nothing.
        cmd::ChangeSet changes;
        std::size_t frozen = 0;
        for (const EntityId id : *ids) {
            Entity changed = *model.entities.find(id);
            auto& leader = std::get<katana::entity::LeaderGeometry>(changed.geometry);
            if (detach && !leader.tipRef.associated()) {
                continue;
            }
            if (katana::entity::isSmart(leader)) {
                leader.text = katana::entity::leaderNote(model, leader, codePropertyCandidates());
                leader.fields = false;
                leader.labelStyle.clear();
                ++frozen;
            } else if (!detach) {
                continue;
            }
            if (detach) {
                leader.tipRef = katana::entity::AnchorRef{};
            }
            changes.modify.push_back(std::move(changed));
        }
        const std::size_t touched = changes.modify.size();
        if (!changes.empty()) {
            if (auto status = apply(detach ? "DETACH_LEADER" : "FREEZE_LEADER", std::move(changes));
                !status) {
                return status.error();
            }
        }
        return detach ? "detached leaders=" + std::to_string(touched) +
                            " frozen=" + std::to_string(frozen)
                      : "frozen leaders=" + std::to_string(frozen);
    }

    if (!balloon && action == "PROP") {
        static constexpr const char* kUsage =
            "LEADER PROP id SET key value [text|integer|real|boolean] | LEADER PROP id DELETE key";
        if (args.size() < 4) {
            return usageOf(kUsage);
        }
        auto ids = leadersOf({args[1]});
        if (!ids) {
            return ids.error();
        }
        const EntityId id = ids->front();
        const katana::entity::AnchorRef tipRef = leaderOf(id).tipRef;
        if (!tipRef.associated()) {
            return makeError(ErrorCode::InvalidArgument,
                             "that leader's tip is on no entity, so it has no attributes to set",
                             "id=" + std::to_string(id));
        }
        if (!model.entities.contains(tipRef.entity)) {
            return makeError(ErrorCode::NotFound, "the entity the leader's tip is on is gone",
                             "id=" + std::to_string(tipRef.entity));
        }
        const std::string sub = upperCase(args[2]);
        const std::string& key = args[3];
        Status status;
        if (sub == "SET") {
            if (args.size() < 5 || args.size() > 6) {
                return usageOf(kUsage);
            }
            auto value = propertyValue(args[4], args.size() == 6 ? &args[5] : nullptr);
            if (!value) {
                return value.error();
            }
            status =
                document_.execute(cmd::setEntityProperty({tipRef.entity}, key, std::move(*value)));
        } else if (sub == "DELETE") {
            if (args.size() != 4) {
                return usageOf(kUsage);
            }
            status = document_.execute(cmd::removeEntityProperty({tipRef.entity}, key));
        } else {
            return usageOf(kUsage);
        }
        if (!status) {
            return status.error();
        }
        return "leader=" + std::to_string(id) + " target=" + std::to_string(tipRef.entity) +
               " property=" + field(key) + " text=" +
               field(katana::entity::leaderNote(model, leaderOf(id), codePropertyCandidates()));
    }

    if (!balloon && action == "ALIGN") {
        auto parsed = splitArguments(args, 1, {"x", "spacing"});
        if (!parsed) {
            return parsed.error();
        }
        if (parsed->positional.empty()) {
            return usageOf("LEADER ALIGN id id [id...] | SELECTION [x=] [spacing=mm]");
        }
        auto ids = leadersOf(parsed->positional);
        if (!ids) {
            return ids.error();
        }
        // Top to bottom by where each note hangs (the last vertex), then by id.
        std::vector<EntityId> order = *ids;
        std::stable_sort(order.begin(), order.end(), [&](EntityId a, EntityId b) {
            return leaderOf(a).vertices.back().y > leaderOf(b).vertices.back().y;
        });
        const Point2 top = leaderOf(order.front()).vertices.back();
        double x = top.x;
        if (const std::string* value = parsed->find("x")) {
            auto number = numberOf(*value, "x");
            if (!number) {
                return number.error();
            }
            x = *number;
        }
        std::optional<double> step;
        if (const std::string* value = parsed->find("spacing")) {
            auto number = numberOf(*value, "spacing");
            if (!number) {
                return number.error();
            }
            if (!(*number > 0.0)) {
                return makeError(ErrorCode::InvalidArgument, "spacing= must be more than zero",
                                 *value);
            }
            step = katana::entity::annotationModelSize(*number, document_.annotationScale());
        }
        cmd::ChangeSet changes;
        for (std::size_t k = 0; k < order.size(); ++k) {
            Entity changed = *model.entities.find(order[k]);
            auto& leader = std::get<katana::entity::LeaderGeometry>(changed.geometry);
            Point2& hang = leader.vertices.back();
            const Point2 moved(x, step ? top.y - static_cast<double>(k) * *step : hang.y);
            if (moved == hang) {
                continue;
            }
            hang = moved;
            changes.modify.push_back(std::move(changed));
        }
        if (!changes.empty()) {
            if (auto status = apply("ALIGN_LEADERS", std::move(changes)); !status) {
                return status.error();
            }
        }
        return "aligned leaders=" + std::to_string(order.size()) + " x=" + real(x) +
               (step ? " spacing=" + real(*step) : std::string());
    }

    if (balloon && action == "RENUMBER") {
        auto parsed = splitArguments(args, 1, {"start", "order"});
        if (!parsed) {
            return parsed.error();
        }
        if (!parsed->positional.empty()) {
            return usageOf("BALLOON RENUMBER [start=1] [order=id|x|y]");
        }
        int start = 1;
        if (const std::string* value = parsed->find("start")) {
            auto number = integerOf(*value, "start");
            if (!number) {
                return number.error();
            }
            start = *number;
        }
        const std::string order =
            parsed->has("order") ? lowerCase(*parsed->find("order")) : std::string("id");
        if (order != "id" && order != "x" && order != "y") {
            return makeError(ErrorCode::InvalidArgument, "order= is id, x or y", order);
        }
        // The numbered balloons - a circled plain leader saying a whole
        // number, which is what BALLOON makes - in id order first.
        std::vector<EntityId> balloons;
        model.entities.forEach([&](const Entity& entity) {
            const auto* other = std::get_if<katana::entity::LeaderGeometry>(&entity.geometry);
            if (other == nullptr || other->callout != katana::entity::CalloutShape::Circle ||
                katana::entity::isSmart(*other) || other->text.empty()) {
                return;
            }
            long long value = 0;
            const auto [end, error] = std::from_chars(
                other->text.data(), other->text.data() + other->text.size(), value);
            if (error == std::errc{} && end == other->text.data() + other->text.size()) {
                balloons.push_back(entity.id);
            }
        });
        // Across the sheet (x, left to right) or down it (y, top to bottom),
        // by the point each balloon's arrow touches; ties keep id order.
        if (order != "id") {
            std::stable_sort(balloons.begin(), balloons.end(), [&](EntityId a, EntityId b) {
                const Point2& p = leaderOf(a).vertices.front();
                const Point2& q = leaderOf(b).vertices.front();
                return order == "x" ? p.x < q.x : p.y > q.y;
            });
        }
        cmd::ChangeSet changes;
        for (std::size_t k = 0; k < balloons.size(); ++k) {
            Entity changed = *model.entities.find(balloons[k]);
            auto& leader = std::get<katana::entity::LeaderGeometry>(changed.geometry);
            const std::string number =
                std::to_string(static_cast<long long>(start) + static_cast<long long>(k));
            if (leader.text == number) {
                continue;
            }
            leader.text = number;
            changes.modify.push_back(std::move(changed));
        }
        const std::size_t renumbered = changes.modify.size();
        if (!changes.empty()) {
            if (auto status = apply("RENUMBER_BALLOONS", std::move(changes)); !status) {
                return status.error();
            }
        }
        return "balloons=" + std::to_string(balloons.size()) +
               " renumbered=" + std::to_string(renumbered);
    }

    if (action == "FOR") {
        auto parsed = splitArguments(args, 1,
                                     {"text", "template", "labelstyle", "n", "arrow", "callout",
                                      "style", "paper", "arrowsize", "landing", "angle", "length"});
        if (!parsed) {
            return parsed.error();
        }
        if (parsed->positional.empty()) {
            return usageOf(balloon ? "BALLOON FOR id [id...] | SELECTION [n= template= "
                                     "labelstyle= angle=deg length=mm ...]"
                                   : "LEADER FOR id [id...] | SELECTION [text= template= "
                                     "labelstyle= angle=deg length=mm ...]");
        }
        std::vector<EntityId> targets;
        if (parsed->positional.size() == 1 &&
            upperCase(parsed->positional.front()) == "SELECTION") {
            targets = document_.selection().ids();
            if (targets.empty()) {
                return makeError(ErrorCode::InvalidState, "nothing is selected; use SELECT first");
            }
        } else {
            for (const std::string& token : parsed->positional) {
                auto id = idOf(token);
                if (!id) {
                    return id.error();
                }
                if (!model.entities.contains(*id)) {
                    return makeError(ErrorCode::NotFound, "entity does not exist",
                                     "id=" + std::to_string(*id));
                }
                targets.push_back(*id);
            }
            std::sort(targets.begin(), targets.end());
            targets.erase(std::unique(targets.begin(), targets.end()), targets.end());
        }
        // The note goes `length` paper millimetres from the tip at `angle`:
        // up and to the right by default, the way a callout is usually drawn.
        double angle = 45.0;
        double length = 10.0;
        for (const auto& [key, target] :
             {std::pair{"angle", &angle}, std::pair{"length", &length}}) {
            if (const std::string* value = parsed->find(key)) {
                auto number = numberOf(*value, key);
                if (!number) {
                    return number.error();
                }
                *target = *number;
            }
        }
        if (!(length > 0.0)) {
            return makeError(ErrorCode::InvalidArgument, "length= must be more than zero",
                             real(length));
        }
        const double radians = angle * katana::math::kDegToRad;
        const Vec2 toNote =
            Vec2(std::cos(radians), std::sin(radians)) *
            katana::entity::annotationModelSize(length, document_.annotationScale());
        const bool numbered = balloon && !parsed->has("text") && !parsed->has("template") &&
                              !parsed->has("labelstyle") && !parsed->has("n");
        long long number = balloon ? nextBalloonNumber(model) : 0;
        const auto attributes = document_.currentAttributes();
        cmd::ChangeSet changes;
        std::size_t skipped = 0;
        std::string firstSkip;
        for (const EntityId id : targets) {
            const Entity* target = model.entities.find(id);
            if (target == nullptr) {
                ++skipped;
                continue;
            }
            const auto ref = naturalAnchor(*target, radians);
            const auto tip = ref ? katana::entity::resolveAnchor(*target, *ref) : std::nullopt;
            if (!tip) {
                ++skipped;
                if (firstSkip.empty()) {
                    firstSkip = "id=" + std::to_string(id) + " (a " +
                                std::string(katana::entity::toString(target->type())) +
                                ") has no place to attach a leader to";
                }
                continue;
            }
            katana::entity::LeaderGeometry leader;
            leader.vertices = {*tip, *tip + toNote};
            leader.tipRef = *ref;
            if (ref->point == katana::entity::AnchorPoint::Inside) {
                // A leader that ends inside an outline ends in a dot, one
                // that ends on it in an arrowhead (ISO 128-22, leader lines).
                leader.arrow = katana::entity::ArrowHead::Dot;
            }
            if (balloon) {
                leader.callout = katana::entity::CalloutShape::Circle;
            }
            if (auto status = applyLeaderNote(*parsed, model, leader); !status) {
                return status.error();
            }
            if (auto status = applyLeaderLook(*parsed, model, leader); !status) {
                return status.error();
            }
            if (numbered) {
                leader.text = std::to_string(number++);
            }
            if (auto status = requireNote(model, leader); !status) {
                ++skipped;
                if (firstSkip.empty()) {
                    firstSkip = status.error().message + " [" + status.error().context + "]";
                }
                continue;
            }
            Entity entity;
            entity.geometry = std::move(leader);
            entity.layer = attributes.layer;
            entity.style = attributes.style;
            entity.color = attributes.color;
            changes.add.push_back(std::move(entity));
        }
        if (changes.add.empty()) {
            return makeError(ErrorCode::InvalidArgument, "no leader made", firstSkip);
        }
        if (auto status = apply(balloon ? "CREATE_BALLOONS" : "CREATE_LEADERS", std::move(changes));
            !status) {
            return status.error();
        }
        const auto ids = document_.lastCreatedEntities();
        return std::string("created ") + (balloon ? "balloons=" : "leaders=") +
               std::to_string(ids.size()) + " ids=" + idList(ids) +
               " skipped=" + std::to_string(skipped);
    }

    // Making one: LEADER p p [p...] / BALLOON p p [p...].
    auto parsed = splitArguments(args, 0,
                                 {"text", "template", "labelstyle", "arrow", "callout", "style",
                                  "paper", "arrowsize", "landing", "n"});
    if (!parsed) {
        return parsed.error();
    }
    if (parsed->positional.size() < 2) {
        return usageOf(balloon ? "BALLOON p p [p...] [n= | template= | labelstyle=] [style= "
                                 "paper=] | BALLOON FOR id... | SELECTION | BALLOON RENUMBER"
                               : kLeaderUsage);
    }
    katana::entity::LeaderGeometry leader;
    for (std::size_t i = 0; i < parsed->positional.size(); ++i) {
        auto point = parseAnchoredPoint(parsed->positional[i]);
        if (!point) {
            return point.error();
        }
        leader.vertices.push_back(point->point);
        if (i == 0) {
            leader.tipRef = point->ref; // the tip follows what it points at
        }
    }
    if (leader.tipRef.point == katana::entity::AnchorPoint::Inside) {
        // Inside an outline a leader ends in a dot (ISO 128-22, leader lines).
        leader.arrow = katana::entity::ArrowHead::Dot;
    }
    if (balloon) {
        leader.callout = katana::entity::CalloutShape::Circle;
        leader.text = std::to_string(nextBalloonNumber(model));
    }
    if (auto status = applyLeaderNote(*parsed, model, leader); !status) {
        return status.error();
    }
    if (auto status = applyLeaderLook(*parsed, model, leader); !status) {
        return status.error();
    }
    if (auto status = requireNote(model, leader); !status) {
        return status.error();
    }
    if (auto status = document_.execute(std::make_unique<cmd::ChangeSetCommand>(
            balloon ? "CREATE_BALLOON" : "CREATE_LEADER",
            [leader, attributes = document_.currentAttributes()](const cmd::CommandContext&)
                -> Result<cmd::ChangeSet> {
                Entity entity;
                entity.geometry = leader;
                entity.layer = attributes.layer;
                entity.style = attributes.style;
                entity.color = attributes.color;
                cmd::ChangeSet changes;
                changes.add.push_back(std::move(entity));
                return changes;
            }));
        !status) {
        return status.error();
    }
    const auto ids = document_.lastCreatedEntities();
    std::string reply =
        std::string(balloon ? "created balloon id=" : "created leader id=") + idList(ids) +
        " text=" + field(katana::entity::leaderNote(model, leader, codePropertyCandidates())) +
        " associative=" + (leader.tipRef.associated() ? "yes" : "no");
    if (leader.tipRef.associated()) {
        reply += " target=" + std::to_string(leader.tipRef.entity) +
                 " anchor=" + field(katana::entity::describe(leader.tipRef));
    }
    if (leader.fields) {
        reply += " template=" + field(leader.text);
    }
    if (!leader.labelStyle.empty()) {
        reply += " labelstyle=" + field(leader.labelStyle);
    }
    return reply;
}

} // namespace katana::cad
