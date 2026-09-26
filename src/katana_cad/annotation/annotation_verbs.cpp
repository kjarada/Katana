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
#include <optional>
#include <set>
#include <sstream>
#include <string>
#include <vector>

#include "katana/cad/annotation/auto_label.hpp"
#include "katana/cad/annotation/command_words.hpp"
#include "katana/cad/annotation/dimension_build.hpp"
#include "katana/cad/annotation/label_layout.hpp"
#include "katana/cad/annotation/leader_edit.hpp"
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

// A typed text read back, as field() writes it: "\n" a line break, "\\" a
// backslash (core's reader, shared with the sheet verbs).
std::string unescape(std::string_view text)
{
    return katana::core::unescapeTyped(text);
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

// A value as a record field (command_words.hpp, recordValue: the one rule,
// shared with DIMSTYLE INFO's record).
std::string field(std::string_view value)
{
    return ann::recordValue(value);
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
          at=x,y|none text=|none layer=] | DELETE id... | LAYOUT [scale=N] [collisions=on|off]
          (LAYOUT adds a label= piece= suppressed=yes line for each piece with no room)
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
        if (part.starts_with("along")) {
            // "#12.along1:0.25": a quarter of the way along segment 1 of #12
            // (0 on a line, an arc or a circle), exactly - what LIST prints as
            // "along 1 0.25", and what the window's Along sets; "#12@x,y"
            // names only the place nearest a point.
            const std::string_view spec = std::string_view(part).substr(5);
            const std::size_t colon = spec.find(':');
            std::uint32_t index = 0;
            std::optional<double> fraction;
            if (colon != std::string_view::npos) {
                const auto [end, error] = std::from_chars(spec.data(), spec.data() + colon, index);
                if (error == std::errc{} && end == spec.data() + colon) {
                    fraction = katana::core::parseFiniteDouble(spec.substr(colon + 1));
                }
            }
            if (!fraction || *fraction < 0.0 || *fraction > 1.0) {
                return makeError(ErrorCode::ParseFailure,
                                 "along is #id.along<segment>:<fraction>, the fraction from 0 "
                                 "to 1 (#12.along0:0.5 is the middle of a line)",
                                 text);
            }
            if (index != 0 &&
                !std::holds_alternative<katana::geometry::Polyline2>(entity->geometry)) {
                return makeError(ErrorCode::InvalidArgument,
                                 "only a polyline has segments to number: along0:t on a line, "
                                 "an arc or a circle",
                                 text);
            }
            ref.point = katana::entity::AnchorPoint::Along;
            ref.index = index;
            // A circle's turn is [0, 1), as nearestAnchor names it: a whole
            // turn is where it starts.
            ref.parameter = std::holds_alternative<katana::geometry::Circle2>(entity->geometry) &&
                                    *fraction == 1.0
                                ? 0.0
                                : *fraction;
        } else if (!indexed("vertex", katana::entity::AnchorPoint::Vertex) &&
                   !indexed("v", katana::entity::AnchorPoint::Vertex) &&
                   !indexed("segment", katana::entity::AnchorPoint::SegmentMid) &&
                   !indexed("s", katana::entity::AnchorPoint::SegmentMid)) {
            auto point = katana::entity::anchorPointFromString(part);
            if (!point) {
                return makeError(ErrorCode::ParseFailure,
                                 "unknown point of an entity (start end mid centre position "
                                 "inside vN sN alongN:T, or #id@x,y for the nearest point on it)",
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
    // The Text and Multiline Text tools make theirs by the same rule.
    ann::fitModelHeight(model, scale, text);
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
            line << " text=" << field(ann::shownLabelText(model, *label));
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
        // Then each piece that found no room, so it can be selected and
        // given some.
        for (const ann::LabelPieceRef& piece : layout.suppressedPieces) {
            out << "\nlabel=" << piece.label << " piece=" << piece.piece << " suppressed=yes";
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
    // Every label as one step: one UNDO takes back the lot. The Label
    // Objects tool makes its labels through the same door.
    auto built = ann::createLabels(model, requests);
    if (!built) {
        return built.error();
    }
    if (auto status = document_.execute(std::move(*built)); !status) {
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
        auto list = points(1);
        if (!list) {
            return list.error();
        }
        // A text height and a half apart unless told (ann::baselineSpacing).
        std::optional<double> spacing;
        if (const std::string* text = parsed->find("spacing");
            text != nullptr && kind == "BASELINE") {
            auto value = numberOf(*text, "spacing");
            if (!value) {
                return value.error();
            }
            spacing = *value;
        }
        // The whole chain as one step, on the base's layer and style - the
        // Baseline and Continue Dimension tools' door too.
        auto built = ann::dimensionChain(
            model, *id, *list,
            kind == "BASELINE" ? ann::DimensionChain::Baseline : ann::DimensionChain::Continued,
            spacing, document_.annotationScale());
        if (!built) {
            return built.error();
        }
        if (auto status = document_.execute(std::move(*built)); !status) {
            return status.error();
        }
        const auto ids = document_.lastCreatedEntities();
        return "created dimensions=" + std::to_string(ids.size()) + " ids=" + idList(ids);
    }
    return usageOf("DIM LINEAR|HORIZONTAL|VERTICAL|ALIGNED|ANGULAR|RADIUS|DIAMETER|ORDINATE|"
                   "BASELINE|CONTINUE ...");
}

// ---- LEADER, BALLOON --------------------------------------------------------------------------
//
// Thin: each subcommand reads its words into the edit annotation/leader_edit.hpp
// makes, which the window's Leaders manager makes too, and replies with what
// happened.

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

// The note and look options as a change: at most one of text= (literal),
// template= (read off the tip's entity), labelstyle= (that style's
// template) and, for a balloon, n=; arrow=, callout=, style=, paper=,
// arrowsize=, landing=.
Result<ann::LeaderChange> leaderChangeOf(const Arguments& args)
{
    ann::LeaderChange change;
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
            change.note = ann::LeaderNote{ann::LeaderNote::Kind::Text, unescape(*text)};
        }
    }
    if (const std::string* text = args.find("template")) {
        change.note = ann::LeaderNote{ann::LeaderNote::Kind::Template, unescape(*text)};
    }
    if (const std::string* name = args.find("labelstyle")) {
        change.note = ann::LeaderNote{ann::LeaderNote::Kind::LabelStyle, *name};
    }
    if (const std::string* value = args.find("arrow")) {
        auto head = arrowOf(*value);
        if (!head) {
            return head.error();
        }
        change.arrow = *head;
    }
    if (const std::string* value = args.find("callout")) {
        auto shape = katana::entity::calloutShapeFromString(*value);
        if (!shape) {
            return shape.error();
        }
        change.callout = *shape;
    }
    if (const std::string* value = args.find("style")) {
        change.textStyle = *value;
    }
    for (const auto& [key, target] :
         {std::pair{"paper", &change.paperHeight}, std::pair{"arrowsize", &change.arrowSize},
          std::pair{"landing", &change.landing}}) {
        if (const std::string* value = args.find(key)) {
            auto number = numberOf(*value, key);
            if (!number) {
                return number.error();
            }
            *target = *number;
        }
    }
    return change;
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
         << " text=" << field(ann::leaderSays(model, leader));
    return line.str();
}

// The ids `tokens` name, in id order and each once: the selection's for
// SELECTION, restricted to leaders when `leadersOnly`.
Result<std::vector<EntityId>> idsOf(const std::vector<std::string>& tokens,
                                    const katana::cad::Document& document, bool leadersOnly)
{
    std::vector<EntityId> ids;
    if (tokens.size() == 1 && upperCase(tokens.front()) == "SELECTION") {
        for (const EntityId id : document.selection().ids()) {
            const Entity* entity = document.model().entities.find(id);
            if (entity != nullptr &&
                (!leadersOnly ||
                 std::holds_alternative<katana::entity::LeaderGeometry>(entity->geometry))) {
                ids.push_back(id);
            }
        }
        if (ids.empty()) {
            return makeError(ErrorCode::InvalidState,
                             leadersOnly ? "no leader is selected; use SELECT first"
                                         : "nothing is selected; use SELECT first");
        }
        return ids;
    }
    for (const std::string& token : tokens) {
        auto id = idOf(token);
        if (!id) {
            return id.error();
        }
        if (!document.model().entities.contains(*id)) {
            return makeError(ErrorCode::NotFound, "entity does not exist",
                             "id=" + std::to_string(*id));
        }
        ids.push_back(*id);
    }
    if (leadersOnly) {
        if (auto status = ann::requireLeaders(document.model(), ids); !status) {
            return status.error();
        }
    }
    std::sort(ids.begin(), ids.end());
    ids.erase(std::unique(ids.begin(), ids.end()), ids.end());
    return ids;
}

} // namespace

CommandInterpreter::Reply CommandInterpreter::leader(const std::string& verb, const Tokens& args)
{
    const bool balloon = verb == "BALLOON";
    const auto& model = document_.model();
    const std::string action = args.empty() ? std::string() : upperCase(args.front());
    // Runs `command` when there is one: an edit that changes nothing is no step.
    const auto run = [&](katana::commands::CommandPtr command) -> Status {
        return command ? document_.execute(std::move(command)) : Status{};
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
            auto read = ann::leaderTargetValues(model, leader);
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
        out << " type=" << values.at("type").text;
        for (const ann::LeaderValueRow& row : ann::leaderValueRows(values)) {
            out << "\nvalue=" << field(row.name) << " text=" << field(row.text);
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
        auto ids = idsOf(parsed->positional, document_, true);
        if (!ids) {
            return ids.error();
        }
        auto change = leaderChangeOf(*parsed);
        if (!change) {
            return change.error();
        }
        if (const std::string* value = parsed->find("tip")) {
            auto point = parseAnchoredPoint(*value);
            if (!point) {
                return point.error();
            }
            change->tip = *point;
        }
        if (const std::string* value = parsed->find("at")) {
            auto point = parsePoint(*value);
            if (!point) {
                return point.error();
            }
            change->hang = *point;
        }
        auto command = ann::changeLeaders(model, *ids, *change);
        if (!command) {
            return command.error();
        }
        if (auto status = run(std::move(*command)); !status) {
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
        auto ids = idsOf({args[1]}, document_, true);
        if (!ids) {
            return ids.error();
        }
        auto place = parseAnchoredPoint(args[2]);
        if (!place) {
            return place.error();
        }
        auto command = ann::attachLeader(model, ids->front(), *place);
        if (!command) {
            return makeError(command.error().code, command.error().message,
                             command.error().context.empty() ? args[2] : command.error().context);
        }
        if (auto status = run(std::move(*command)); !status) {
            return status.error();
        }
        return "attached leader " + describeLeader(model, *model.entities.find(ids->front()));
    }

    if (!balloon && (action == "DETACH" || action == "FREEZE")) {
        const bool detach = action == "DETACH";
        if (args.size() < 2) {
            return usageOf(detach ? "LEADER DETACH id [id...] | SELECTION"
                                  : "LEADER FREEZE id [id...] | SELECTION");
        }
        auto ids = idsOf(Tokens(args.begin() + 1, args.end()), document_, true);
        if (!ids) {
            return ids.error();
        }
        auto release = ann::releaseLeaders(model, *ids, detach);
        if (!release) {
            return release.error();
        }
        if (auto status = run(std::move(release->command)); !status) {
            return status.error();
        }
        return detach ? "detached leaders=" + std::to_string(release->changed) +
                            " frozen=" + std::to_string(release->frozen)
                      : "frozen leaders=" + std::to_string(release->frozen);
    }

    if (!balloon && action == "PROP") {
        static constexpr const char* kUsage =
            "LEADER PROP id SET key value [text|integer|real|boolean] | LEADER PROP id DELETE key";
        if (args.size() < 4) {
            return usageOf(kUsage);
        }
        auto ids = idsOf({args[1]}, document_, true);
        if (!ids) {
            return ids.error();
        }
        const EntityId id = ids->front();
        const std::string sub = upperCase(args[2]);
        const std::string& key = args[3];
        std::optional<katana::entity::PropertyValue> value;
        if (sub == "SET") {
            if (args.size() < 5 || args.size() > 6) {
                return usageOf(kUsage);
            }
            auto read = propertyValue(args[4], args.size() == 6 ? &args[5] : nullptr);
            if (!read) {
                return read.error();
            }
            value = std::move(*read);
        } else if (sub != "DELETE" || args.size() != 4) {
            return usageOf(kUsage);
        }
        auto command = ann::setLeaderTargetProperty(model, id, key, value);
        if (!command) {
            return command.error();
        }
        if (auto status = run(std::move(*command)); !status) {
            return status.error();
        }
        const auto& leader =
            std::get<katana::entity::LeaderGeometry>(model.entities.find(id)->geometry);
        return "leader=" + std::to_string(id) + " target=" + std::to_string(leader.tipRef.entity) +
               " property=" + field(key) + " text=" + field(ann::leaderSays(model, leader));
    }

    if (!balloon && action == "ALIGN") {
        auto parsed = splitArguments(args, 1, {"x", "spacing"});
        if (!parsed) {
            return parsed.error();
        }
        if (parsed->positional.empty()) {
            return usageOf("LEADER ALIGN id id [id...] | SELECTION [x=] [spacing=mm]");
        }
        auto ids = idsOf(parsed->positional, document_, true);
        if (!ids) {
            return ids.error();
        }
        std::optional<double> x;
        if (const std::string* value = parsed->find("x")) {
            auto number = numberOf(*value, "x");
            if (!number) {
                return number.error();
            }
            x = *number;
        }
        std::optional<double> spacing;
        if (const std::string* value = parsed->find("spacing")) {
            auto number = numberOf(*value, "spacing");
            if (!number) {
                return number.error();
            }
            if (!(*number > 0.0)) {
                return makeError(ErrorCode::InvalidArgument, "spacing= must be more than zero",
                                 *value);
            }
            spacing = katana::entity::annotationModelSize(*number, document_.annotationScale());
        }
        auto alignment = ann::alignLeaders(model, *ids, x, spacing);
        if (!alignment) {
            return alignment.error();
        }
        if (auto status = run(std::move(alignment->command)); !status) {
            return status.error();
        }
        return "aligned leaders=" + std::to_string(alignment->count) + " x=" + real(alignment->x) +
               (spacing ? " spacing=" + real(*spacing) : std::string());
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
        auto renumbering = ann::renumberBalloons(
            model, start,
            order == "x" ? ann::BalloonOrder::X
                         : (order == "y" ? ann::BalloonOrder::Y : ann::BalloonOrder::Id));
        if (auto status = run(std::move(renumbering.command)); !status) {
            return status.error();
        }
        return "balloons=" + std::to_string(renumbering.balloons) +
               " renumbered=" + std::to_string(renumbering.renumbered);
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
        auto targets = idsOf(parsed->positional, document_, false);
        if (!targets) {
            return targets.error();
        }
        auto change = leaderChangeOf(*parsed);
        if (!change) {
            return change.error();
        }
        ann::LeadersForOptions options;
        options.change = std::move(*change);
        options.balloon = balloon;
        // Degrees typed; left out, LeadersForOptions' own (its one source).
        if (const std::string* value = parsed->find("angle")) {
            auto number = numberOf(*value, "angle");
            if (!number) {
                return number.error();
            }
            options.angle = *number * katana::math::kDegToRad;
        }
        if (const std::string* value = parsed->find("length")) {
            auto number = numberOf(*value, "length");
            if (!number) {
                return number.error();
            }
            options.length = *number;
        }
        auto made = ann::leadersFor(model, *targets, options, document_.annotationScale(),
                                    document_.currentAttributes());
        if (!made) {
            return made.error();
        }
        if (auto status = run(std::move(made->command)); !status) {
            return status.error();
        }
        const auto ids = document_.lastCreatedEntities();
        return std::string("created ") + (balloon ? "balloons=" : "leaders=") +
               std::to_string(ids.size()) + " ids=" + idList(ids) +
               " skipped=" + std::to_string(made->skipped);
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
    std::vector<ann::AnchoredPoint> points;
    for (const std::string& token : parsed->positional) {
        auto point = parseAnchoredPoint(token);
        if (!point) {
            return point.error();
        }
        points.push_back(*point);
    }
    auto change = leaderChangeOf(*parsed);
    if (!change) {
        return change.error();
    }
    auto made = ann::newLeader(model, points, *change, balloon);
    if (!made) {
        return made.error();
    }
    const katana::entity::LeaderGeometry leader = std::move(*made);
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
    std::string reply = std::string(balloon ? "created balloon id=" : "created leader id=") +
                        idList(ids) + " text=" + field(ann::leaderSays(model, leader)) +
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
