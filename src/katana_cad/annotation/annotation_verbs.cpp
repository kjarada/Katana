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
#include "katana/cad/annotation/command_words.hpp"
#include "katana/cad/annotation/dimension_build.hpp"
#include "katana/cad/annotation/label_layout.hpp"
#include "katana/cad/annotation/text_layout.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/dimension_draw.hpp"
#include "katana/commands/change_set.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/anchor.hpp"
#include "katana/entity/label_text.hpp"
#include "katana/entity/label_values.hpp"
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
          key=value records.  A point may be #id[.start|.end|.mid|.centre|.vN|.sN]: the
          annotation made from it follows that entity (associative).
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
Leader    LEADER p p [p...] [text= arrow=closed|open|tick|dot|none callout=none|box|circle
          style= paper=mm arrowsize=mm landing=mm]
          BALLOON p p [p...] [n=number style= paper=]   a numbered circle callout)";
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
                                 "unknown point of an entity (start end mid centre position vN sN)",
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

CommandInterpreter::Reply CommandInterpreter::leader(const std::string& verb, const Tokens& args)
{
    const bool balloon = verb == "BALLOON";
    auto parsed = splitArguments(args, 0,
                                 {"text", "arrow", "callout", "style", "paper", "arrowsize",
                                  "landing", "n"});
    if (!parsed) {
        return parsed.error();
    }
    if (parsed->positional.size() < 2) {
        return usageOf(balloon ? "BALLOON p p [p...] [n= style= paper=]"
                               : "LEADER p p [p...] [text= arrow= callout= style= paper= "
                                 "arrowsize= landing=]");
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
    if (balloon) {
        leader.callout = katana::entity::CalloutShape::Circle;
        // The next number: one more than the highest balloon already numbered.
        long long highest = 0;
        document_.model().entities.forEach([&](const Entity& entity) {
            const auto* other = std::get_if<katana::entity::LeaderGeometry>(&entity.geometry);
            if (other == nullptr || other->callout != katana::entity::CalloutShape::Circle) {
                return;
            }
            long long value = 0;
            const auto [end, error] = std::from_chars(
                other->text.data(), other->text.data() + other->text.size(), value);
            if (error == std::errc{} && end == other->text.data() + other->text.size()) {
                highest = std::max(highest, value);
            }
        });
        leader.text = std::to_string(highest + 1);
    }
    for (const auto& [key, value] : parsed->options) {
        if (key == "text" || key == "n") {
            leader.text = unescape(value);
        } else if (key == "arrow") {
            auto head = katana::entity::arrowHeadFromString(value);
            if (!head) {
                // The short names a person types.
                const std::string v = lowerCase(value);
                if (v == "closed" || v == "filled") {
                    head = katana::entity::ArrowHead::ClosedFilled;
                } else if (v == "open") {
                    head = katana::entity::ArrowHead::Open;
                } else if (v == "tick") {
                    head = katana::entity::ArrowHead::Tick;
                } else if (v == "dot") {
                    head = katana::entity::ArrowHead::Dot;
                } else if (v == "none") {
                    head = katana::entity::ArrowHead::None;
                } else {
                    return head.error();
                }
            }
            leader.arrow = *head;
        } else if (key == "callout") {
            auto shape = katana::entity::calloutShapeFromString(value);
            if (!shape) {
                return shape.error();
            }
            leader.callout = *shape;
        } else if (key == "style") {
            if (!document_.model().textStyles.contains(value)) {
                return makeError(ErrorCode::NotFound, "text style does not exist", value);
            }
            leader.style = value;
        } else {
            auto number = numberOf(value, key.c_str());
            if (!number) {
                return number.error();
            }
            (key == "paper" ? leader.paperHeight
                            : key == "arrowsize" ? leader.arrowSize : leader.landing) = *number;
        }
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
    return std::string(balloon ? "created balloon id=" : "created leader id=") + idList(ids) +
           " text=" + field(leader.text) + " associative=" +
           (leader.tipRef.associated() ? "yes" : "no");
}

} // namespace katana::cad
