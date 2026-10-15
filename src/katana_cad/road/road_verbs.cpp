#include "katana/cad/road/road_verbs.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <filesystem>
#include <format>
#include <fstream>
#include <map>
#include <memory>
#include <optional>
#include <set>
#include <string>
#include <utility>
#include <vector>

#include <nlohmann/json.hpp>

#include "katana/cad/csv_table.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/road/corridor_model.hpp"
#include "katana/cad/road/road_section.hpp"
#include "katana/cad/road/road_template.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/path_text.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/colour_names.hpp"
#include "katana/entity/layer_path.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/profile.hpp"
#include "katana/math/numerics.hpp"
#include "katana/terrain/surface_store.hpp"

namespace katana::cad::road {

namespace cmd = katana::commands;
namespace tol = katana::math::tolerance;
using katana::core::Error;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using Json = nlohmann::json;

namespace {

using Tokens = std::vector<std::string>;

// The properties CORRIDOR STRINGS marks its strings with, so that the next
// run replaces them and nothing else, and the layers the road model's
// objects and strings are put on by default.
constexpr std::string_view kCorridorProperty = "road.corridor";
constexpr std::string_view kStringProperty = "road.string";
constexpr std::string_view kStringsLayer = "road/strings";
constexpr std::string_view kCutLayer = "road/earthworks/cut";
constexpr std::string_view kFillLayer = "road/earthworks/fill";

// The IFC 4.3 IfcCourseTypeEnum (road_template.hpp, PavementCourse::ifcType).
constexpr std::array<std::string_view, 8> kCourseTypes{"ARMOUR",      "BALLASTBED", "CORE",
                                                       "FILTER",      "PAVEMENT",   "PROTECTION",
                                                       "USERDEFINED", "NOTDEFINED"};

std::string exact(double value)
{
    return katana::core::formatExactReal(value);
}
// A computed length, level, area or volume: to three places - a millimetre,
// a thousandth of a square or cubic metre - which is what a schedule is
// read to. std::format is locale-independent, so a decimal point always.
std::string fixed(double value)
{
    return std::format("{:.3f}", value);
}
std::string text(std::string_view value)
{
    return recordValue(value);
}
bool is(std::string_view a, std::string_view b)
{
    return katana::core::equalsIgnoringCase(a, b);
}
std::string upper(std::string_view value)
{
    return katana::core::uppered(value);
}

Error usage(std::string message)
{
    return makeError(ErrorCode::ParseFailure, std::move(message));
}

struct Option {
    std::string key;
    std::string value;
};

std::optional<Option> optionOf(std::string_view token)
{
    const auto equals = token.find('=');
    if (equals == std::string_view::npos || equals == 0) {
        return std::nullopt;
    }
    return Option{katana::core::lowered(token.substr(0, equals)),
                  std::string(token.substr(equals + 1))};
}

// The words from `from` on, every one key=value.
Result<std::vector<Option>> optionsFrom(const Tokens& args, std::size_t from)
{
    std::vector<Option> options;
    for (std::size_t i = from; i < args.size(); ++i) {
        auto option = optionOf(args[i]);
        if (!option) {
            return usage("expected key=value, not \"" + args[i] + "\"");
        }
        options.push_back(std::move(*option));
    }
    return options;
}

Result<double> numberOf(const Option& option)
{
    const auto value = katana::core::parseFiniteDouble(option.value);
    if (!value) {
        return usage(option.key + "= takes a number, not \"" + option.value + "\"");
    }
    return *value;
}

Result<bool> switchOf(const Option& option)
{
    const std::string value = katana::core::lowered(option.value);
    if (value == "on" || value == "yes" || value == "true") {
        return true;
    }
    if (value == "off" || value == "no" || value == "false") {
        return false;
    }
    return usage(option.key + "= is on or off, not \"" + option.value + "\"");
}

Result<katana::entity::Color> colourOf(const Option& option)
{
    if (option.value.starts_with('#')) {
        auto colour = katana::entity::Color::fromHex(option.value);
        if (!colour) {
            return usage(option.key + "= is #RRGGBB or a colour's name, not \"" + option.value +
                         "\"");
        }
        return *colour;
    }
    if (const auto named = katana::entity::standardColour(option.value)) {
        return *named;
    }
    return usage(option.key + "= is #RRGGBB or a colour's name, not \"" + option.value + "\"");
}

std::string yes(bool value)
{
    return value ? "yes" : "no";
}

// ---- road templates -------------------------------------------------------------------

Status templatesReadable(const Document& document)
{
    if (const Status status = document.roadTemplatesStatus(); !status) {
        return makeError(ErrorCode::InvalidState,
                         "the drawing's road templates cannot be read: nothing is changed",
                         status.error().describe());
    }
    return {};
}

Status corridorsReadable(const Document& document)
{
    if (const Status status = document.corridorsStatus(); !status) {
        return makeError(ErrorCode::InvalidState,
                         "the drawing's corridors cannot be read: nothing is changed",
                         status.error().describe());
    }
    return {};
}

Error noTemplate(std::string_view name)
{
    return makeError(ErrorCode::NotFound,
                     "no road template " + std::string(name) + ": ROADTEMPLATE LIST names them");
}

Error noCorridor(std::string_view name)
{
    return makeError(ErrorCode::NotFound,
                     "no corridor " + std::string(name) + ": CORRIDOR LIST names them");
}

std::string templateRecord(const RoadTemplate& roadTemplate)
{
    std::size_t courses = 0;
    for (const Component& component : roadTemplate.components) {
        courses += component.courses.size();
    }
    return "template name=" + text(roadTemplate.name) +
           " left=" + std::to_string(roadTemplate.side(Side::Left).size()) +
           " right=" + std::to_string(roadTemplate.side(Side::Right).size()) +
           " courses=" + std::to_string(courses) + " description=" + text(roadTemplate.description);
}

std::string componentRecord(const RoadTemplate& roadTemplate, const Component& component)
{
    std::string out = "component template=" + text(roadTemplate.name) +
                      " side=" + std::string(toString(component.side)) + " name=" + component.name +
                      " kind=" + std::string(toString(component.kind));
    switch (component.kind) {
    case ComponentKind::Batter:
        out += " cut=" + exact(component.cutSlope) + " fill=" + exact(component.fillSlope) +
               " maxwidth=" + exact(component.maximumWidth) +
               " benchheight=" + exact(component.benchHeight) +
               " benchwidth=" + exact(component.benchWidth);
        break;
    case ComponentKind::Kerb:
        out += " shape=" + std::string(toString(component.shape));
        // An edge strip is flush: no face to turn or to give a height, but a
        // crossfall, which a superelevation table may set.
        if (component.shape != KerbShape::EdgeStrip) {
            out += " faces=" + std::string(toString(component.faces));
        }
        out += " width=" + exact(component.width);
        if (component.shape != KerbShape::EdgeStrip) {
            out += " height=" + exact(component.height);
        }
        out += " depth=" + exact(component.depth);
        if (component.shape == KerbShape::KerbAndGutter) {
            out += " gutter=" + exact(component.gutterWidth) +
                   " gutterfall=" + exact(component.gutterFall);
        }
        if (component.shape == KerbShape::EdgeStrip) {
            out += " crossfall=" + exact(component.crossfall) +
                   " superelevated=" + yes(component.superelevated);
        }
        out +=
            " kerbwidth=" + fixed(kerbWidth(component)) + " material=" + text(component.material);
        break;
    case ComponentKind::Drain:
        out += " width=" + exact(component.width) + " crossfall=" + exact(component.crossfall) +
               " superelevated=" + yes(component.superelevated) +
               " depth=" + exact(component.depth);
        break;
    default:
        out += " width=" + exact(component.width) + " crossfall=" + exact(component.crossfall) +
               " superelevated=" + yes(component.superelevated);
        break;
    }
    out += " layer=" + text(component.layer) + " colour=" + component.colour.toHex() +
           " courses=" + std::to_string(component.courses.size());
    return out;
}

std::string courseRecord(const RoadTemplate& roadTemplate, const Component& component,
                         const PavementCourse& course)
{
    return "course template=" + text(roadTemplate.name) +
           " component=" + std::string(toString(component.side)) + ":" + component.name +
           " name=" + course.name + " thickness=" + exact(course.thickness) +
           " material=" + text(course.material) + " ifc=" + course.ifcType +
           " colour=" + course.colour.toHex();
}

// The components `ref` names: side:name, or name on every side that has it.
Result<std::vector<std::size_t>> componentsNamed(const RoadTemplate& roadTemplate,
                                                 std::string_view ref)
{
    std::vector<std::size_t> found;
    const auto colon = ref.find(':');
    if (colon != std::string_view::npos) {
        const auto side = sideFrom(ref.substr(0, colon));
        if (!side) {
            return usage("a component is side:name, left or right: \"" + std::string(ref) + "\"");
        }
        if (const auto index = roadTemplate.find(*side, ref.substr(colon + 1))) {
            found.push_back(*index);
        }
    } else {
        for (const Side side : {Side::Left, Side::Right}) {
            if (const auto index = roadTemplate.find(side, ref)) {
                found.push_back(*index);
            }
        }
    }
    if (found.empty()) {
        return makeError(ErrorCode::NotFound,
                         "template " + roadTemplate.name + " has no component " + std::string(ref) +
                             ": ROADTEMPLATE INFO " + roadTemplate.name + " lists them");
    }
    // Ascending in `components`, not left side first: a side's first
    // component is appended at the end of the vector (insertionPoint), so a
    // template whose right side was made first stores its right components
    // BEFORE its left ones. REMOVE walks these backwards and erases each, which
    // only keeps every later index valid when the indices ascend.
    std::ranges::sort(found);
    return found;
}

Status applyComponent(Component& component, const Option& option)
{
    // `color` is `colour` and `thickness` is `depth`.
    const std::string key =
        option.key == "color" ? "colour" : (option.key == "thickness" ? "depth" : option.key);
    // A key the component's kind does not keep would change nothing - the JSON
    // writes only what the kind uses - and be answered "updated", with no step
    // made: refused instead, naming the keys it has.
    const std::vector<std::string_view> keys = componentKeys(component.kind);
    if (std::ranges::find(keys, std::string_view(key)) == keys.end()) {
        std::string list;
        for (const std::string_view each : keys) {
            list += (list.empty() ? "" : " ") + std::string(each);
        }
        return usage("a " + std::string(toString(component.kind)) + " has no " + key +
                     "=; its keys are " + list);
    }
    const auto number = [&](double& field) -> Status {
        auto value = numberOf(option);
        if (!value) {
            return value.error();
        }
        field = *value;
        return {};
    };
    if (key == "name") {
        if (auto status = checkWord("a component", option.value); !status) {
            return status;
        }
        component.name = option.value;
        return {};
    }
    if (key == "width") {
        return number(component.width);
    }
    if (key == "crossfall") {
        return number(component.crossfall);
    }
    if (key == "height") {
        return number(component.height);
    }
    if (key == "depth") {
        return number(component.depth);
    }
    if (key == "gutter") {
        return number(component.gutterWidth);
    }
    if (key == "gutterfall") {
        return number(component.gutterFall);
    }
    if (key == "cut") {
        return number(component.cutSlope);
    }
    if (key == "fill") {
        return number(component.fillSlope);
    }
    if (key == "maxwidth") {
        return number(component.maximumWidth);
    }
    if (key == "benchheight") {
        return number(component.benchHeight);
    }
    if (key == "benchwidth") {
        return number(component.benchWidth);
    }
    if (key == "superelevated") {
        auto value = switchOf(option);
        if (!value) {
            return value.error();
        }
        component.superelevated = *value;
        return {};
    }
    if (key == "layer") {
        if (auto status = katana::entity::validateLayerPath(option.value); !status) {
            return status;
        }
        component.layer = option.value;
        return {};
    }
    if (key == "colour") {
        auto colour = colourOf(option);
        if (!colour) {
            return colour.error();
        }
        component.colour = *colour;
        return {};
    }
    if (key == "shape") {
        const auto shape = kerbShapeFrom(option.value);
        if (!shape) {
            return usage("shape= is barrier, mountable, kerbgutter or edgestrip, not \"" +
                         option.value + "\"");
        }
        component.shape = *shape;
        return {};
    }
    if (key == "faces") {
        const auto facing = kerbFacingFrom(option.value);
        if (!facing) {
            return usage("faces= is inward (a kerb on the outer edge of a carriageway: it rises "
                         "going outward) or outward (a median's or island's: it falls going "
                         "outward), not \"" +
                         option.value + "\"");
        }
        component.faces = *facing;
        return {};
    }
    if (key == "material") {
        component.material = option.value;
        return {};
    }
    return usage("unknown component key " + key + "=");
}

Status applyCourse(PavementCourse& course, const Option& option)
{
    if (option.key == "name") {
        if (auto status = checkWord("a course", option.value); !status) {
            return status;
        }
        course.name = option.value;
        return {};
    }
    if (option.key == "thickness") {
        auto value = numberOf(option);
        if (!value) {
            return value.error();
        }
        course.thickness = *value;
        return {};
    }
    if (option.key == "material") {
        course.material = option.value;
        return {};
    }
    if (option.key == "ifc") {
        const std::string type = upper(option.value);
        if (std::ranges::find(kCourseTypes, type) == kCourseTypes.end()) {
            return usage("ifc= is an IfcCourse type - ARMOUR, BALLASTBED, CORE, FILTER, "
                         "PAVEMENT, PROTECTION, USERDEFINED or NOTDEFINED - not \"" +
                         option.value + "\"");
        }
        course.ifcType = type;
        return {};
    }
    if (option.key == "colour" || option.key == "color") {
        auto colour = colourOf(option);
        if (!colour) {
            return colour.error();
        }
        course.colour = *colour;
        return {};
    }
    return usage("unknown course key " + option.key +
                 "=; the keys are name thickness material ifc colour");
}

// Where a new component goes in `components`: the `at`-th of its side (from
// 1), or before the side's batter, or after the side's last.
std::size_t insertionPoint(const RoadTemplate& roadTemplate, Side side, ComponentKind kind,
                           std::optional<std::size_t> at)
{
    std::vector<std::size_t> ofSide;
    for (std::size_t i = 0; i < roadTemplate.components.size(); ++i) {
        if (roadTemplate.components[i].side == side) {
            ofSide.push_back(i);
        }
    }
    if (at && *at >= 1 && *at <= ofSide.size()) {
        return ofSide[*at - 1];
    }
    if (!at && kind != ComponentKind::Batter) {
        for (const std::size_t i : ofSide) {
            if (roadTemplate.components[i].kind == ComponentKind::Batter) {
                return i;
            }
        }
    }
    return ofSide.empty() ? roadTemplate.components.size() : ofSide.back() + 1;
}

bool templateUsed(const CorridorSet& corridors, std::string_view name, std::string* by)
{
    for (const Corridor& corridor : corridors.corridors) {
        bool uses = is(corridor.templateName, name);
        for (const TemplateRegion& region : corridor.regions) {
            uses = uses || is(region.templateName, name);
        }
        if (uses) {
            *by = corridor.name;
            return true;
        }
    }
    return false;
}

Result<std::string> commitTemplates(Document& document, const RoadTemplateSet& set,
                                    std::string stepName, std::string reply)
{
    if (auto status = document.setRoadTemplates(set, std::move(stepName)); !status) {
        return status.error();
    }
    return reply;
}

Result<std::string> roadTemplateVerb(Document& document, const Tokens& args)
{
    const std::string sub = args.empty() ? std::string("LIST") : upper(args[0]);
    const RoadTemplateSet& current = document.roadTemplates();
    if (sub == "LIST" || sub == "JSON") {
        const bool json = sub == "JSON" || (args.size() > 1 && is(args[1], "JSON"));
        if (const Status status = document.roadTemplatesStatus(); !status) {
            return status.error();
        }
        if (json) {
            return roadTemplatesToJson(current);
        }
        if (current.templates.empty()) {
            return std::string("templates=0 (ROADTEMPLATE DEFAULTS adds the standard ones)");
        }
        std::string reply = "templates=" + std::to_string(current.templates.size());
        for (const RoadTemplate& each : current.templates) {
            reply += "\n" + templateRecord(each);
        }
        return reply;
    }
    if (auto status = templatesReadable(document); !status) {
        return status.error();
    }
    if (sub == "DEFAULTS") {
        RoadTemplateSet set = current;
        std::string added;
        std::size_t count = 0;
        std::size_t skipped = 0;
        for (RoadTemplate& standard : standardTemplates()) {
            if (set.find(standard.name) != nullptr) {
                ++skipped;
                continue;
            }
            added += "\n" + templateRecord(standard);
            set.templates.push_back(std::move(standard));
            ++count;
        }
        return commitTemplates(document, set, "ROADTEMPLATE_DEFAULTS",
                               "added=" + std::to_string(count) +
                                   " skipped=" + std::to_string(skipped) + added);
    }
    if (args.size() < 2) {
        return usage("ROADTEMPLATE " + sub + " needs a template's name (HELP ROADTEMPLATE)");
    }
    const std::string& name = args[1];
    if (sub == "NEW") {
        if (auto status = checkName("a road template", name); !status) {
            return status.error();
        }
        if (current.find(name) != nullptr) {
            return makeError(ErrorCode::AlreadyExists, "there is a road template called " + name);
        }
        RoadTemplate made;
        made.name = name;
        std::size_t at = 2;
        if (args.size() > 3 && is(args[2], "FROM")) {
            const RoadTemplate* source = current.find(args[3]);
            if (source == nullptr) {
                return noTemplate(args[3]);
            }
            made.components = source->components;
            made.description = source->description;
            at = 4;
        }
        auto options = optionsFrom(args, at);
        if (!options) {
            return options.error();
        }
        for (const Option& option : *options) {
            if (option.key != "description") {
                return usage("ROADTEMPLATE NEW takes description=, not " + option.key + "=");
            }
            made.description = option.value;
        }
        RoadTemplateSet set = current;
        set.templates.push_back(made);
        return commitTemplates(document, set, "ROADTEMPLATE_NEW",
                               templateRecord(made) + " created");
    }
    const RoadTemplate* found = current.find(name);
    if (found == nullptr) {
        return noTemplate(name);
    }
    const RoadTemplate& roadTemplate = *found;
    const std::size_t index = static_cast<std::size_t>(found - current.templates.data());
    if (sub == "INFO") {
        std::string reply = templateRecord(roadTemplate);
        for (const Side side : {Side::Left, Side::Right}) {
            for (const Component* component : roadTemplate.side(side)) {
                reply += "\n" + componentRecord(roadTemplate, *component);
                for (const PavementCourse& course : component->courses) {
                    reply += "\n  " + courseRecord(roadTemplate, *component, course);
                }
            }
        }
        return reply;
    }
    if (sub == "SECTION") {
        auto section = templateSection(roadTemplate);
        if (!section) {
            return section.error();
        }
        std::string reply = "section template=" + text(roadTemplate.name) +
                            " points=" + std::to_string(section->top.size()) +
                            " formation=" + std::to_string(section->formation.size()) +
                            " solids=" + std::to_string(section->solids.size()) + " level=0";
        for (const SectionPoint& point : section->top) {
            reply += "\npoint code=" + point.code + " offset=" + fixed(point.offset) +
                     " level=" + fixed(point.level);
        }
        for (const SectionPoint& point : section->formation) {
            reply += "\nformation code=" + point.code + " offset=" + fixed(point.offset) +
                     " level=" + fixed(point.level);
        }
        for (const SectionSolid& solid : section->solids) {
            reply +=
                "\nsolid key=" + solid.key + " columns=" + std::to_string(solid.columns.size());
        }
        return reply;
    }
    if (sub == "DELETE") {
        // Corridors that cannot be read use the template for all we can tell:
        // an empty set would say none does.
        if (auto status = corridorsReadable(document); !status) {
            return status.error();
        }
        std::string by;
        if (templateUsed(document.corridors(), roadTemplate.name, &by)) {
            return makeError(ErrorCode::InvalidArgument,
                             "the road template " + roadTemplate.name + " is used by corridor " +
                                 by + ": change or delete the corridor first");
        }
        RoadTemplateSet set = current;
        set.templates.erase(set.templates.begin() + static_cast<std::ptrdiff_t>(index));
        return commitTemplates(document, set, "ROADTEMPLATE_DELETE",
                               "template name=" + text(roadTemplate.name) + " deleted");
    }
    if (sub == "RENAME") {
        if (args.size() < 3) {
            return usage("ROADTEMPLATE RENAME <template> <new name>");
        }
        const std::string& newName = args[2];
        if (auto status = checkName("a road template", newName); !status) {
            return status.error();
        }
        if (const RoadTemplate* other = current.find(newName); other != nullptr && other != found) {
            return makeError(ErrorCode::AlreadyExists,
                             "there is a road template called " + newName);
        }
        if (auto status = corridorsReadable(document); !status) {
            return status.error();
        }
        RoadTemplateSet set = current;
        set.templates[index].name = newName;
        // The corridors that name it follow it, in the same step.
        CorridorSet corridors = document.corridors();
        std::size_t following = 0;
        for (Corridor& corridor : corridors.corridors) {
            bool moved = false;
            if (is(corridor.templateName, roadTemplate.name)) {
                corridor.templateName = newName;
                moved = true;
            }
            for (TemplateRegion& region : corridor.regions) {
                if (is(region.templateName, roadTemplate.name)) {
                    region.templateName = newName;
                    moved = true;
                }
            }
            following += moved ? 1 : 0;
        }
        auto templatesStep = document.roadTemplatesCommand(set, "ROADTEMPLATE_RENAME");
        if (!templatesStep) {
            return templatesStep.error();
        }
        auto corridorsStep = document.corridorsCommand(corridors, "ROADTEMPLATE_RENAME");
        if (!corridorsStep) {
            return corridorsStep.error();
        }
        auto transaction = std::make_unique<cmd::Transaction>("ROADTEMPLATE_RENAME");
        if (*templatesStep) {
            transaction->add(std::move(*templatesStep));
        }
        if (*corridorsStep) {
            transaction->add(std::move(*corridorsStep));
        }
        if (transaction->size() > 0) {
            if (auto status = document.execute(std::move(transaction)); !status) {
                return status.error();
            }
        }
        return "template renamed from=" + text(roadTemplate.name) + " to=" + text(newName) +
               " corridors=" + std::to_string(following);
    }
    if (sub == "SET" && args.size() > 2 && optionOf(args[2])) {
        auto options = optionsFrom(args, 2);
        if (!options) {
            return options.error();
        }
        RoadTemplateSet set = current;
        for (const Option& option : *options) {
            if (option.key != "description") {
                return usage("ROADTEMPLATE SET <template> takes description=; a component's "
                             "keys follow its name: ROADTEMPLATE SET <template> <component> ...");
            }
            set.templates[index].description = option.value;
        }
        return commitTemplates(document, set, "ROADTEMPLATE_SET",
                               templateRecord(set.templates[index]) + " updated");
    }
    if (sub == "ADD") {
        if (args.size() < 4) {
            return usage("ROADTEMPLATE ADD <template> left|right|both <kind> [key=value ...]");
        }
        std::vector<Side> sides;
        if (is(args[2], "BOTH")) {
            sides = {Side::Left, Side::Right};
        } else if (const auto side = sideFrom(args[2])) {
            sides = {*side};
        } else {
            return usage("a side is left, right or both, not \"" + args[2] + "\"");
        }
        const auto kind = componentKindFrom(args[3]);
        if (!kind) {
            return usage("a component is a lane, shoulder, kerb, footpath, verge, median, drain "
                         "or batter, not \"" +
                         args[3] + "\"");
        }
        auto options = optionsFrom(args, 4);
        if (!options) {
            return options.error();
        }
        RoadTemplateSet set = current;
        RoadTemplate& edited = set.templates[index];
        std::string reply;
        for (const Side side : sides) {
            Component component = defaultComponent(*kind, side);
            std::optional<std::size_t> at;
            for (const Option& option : *options) {
                if (option.key == "at") {
                    const auto position = katana::core::parseInteger(option.value);
                    if (!position || *position < 1) {
                        return usage("at= is a position from 1, not \"" + option.value + "\"");
                    }
                    at = static_cast<std::size_t>(*position);
                    continue;
                }
                if (auto status = applyComponent(component, option); !status) {
                    return status.error();
                }
            }
            // A second of a kind is named after its place: lane2, lane3.
            if (edited.find(side, component.name)) {
                const std::string base = component.name;
                for (int n = 2;; ++n) {
                    const std::string candidate = base + std::to_string(n);
                    if (!edited.find(side, candidate)) {
                        component.name = candidate;
                        break;
                    }
                }
            }
            const std::size_t position = insertionPoint(edited, side, *kind, at);
            reply += (reply.empty() ? "" : "\n") + componentRecord(edited, component) + " added";
            edited.components.insert(edited.components.begin() +
                                         static_cast<std::ptrdiff_t>(position),
                                     std::move(component));
        }
        return commitTemplates(document, set, "ROADTEMPLATE_ADD", reply);
    }
    if (sub == "SET" || sub == "REMOVE") {
        if (args.size() < 3) {
            return usage("ROADTEMPLATE " + sub + " <template> <component>" +
                         (sub == "SET" ? " key=value ..." : ""));
        }
        auto indices = componentsNamed(roadTemplate, args[2]);
        if (!indices) {
            return indices.error();
        }
        RoadTemplateSet set = current;
        RoadTemplate& edited = set.templates[index];
        std::string reply;
        if (sub == "REMOVE") {
            for (auto it = indices->rbegin(); it != indices->rend(); ++it) {
                const Component& gone = edited.components[*it];
                reply = "component template=" + text(edited.name) +
                        " side=" + std::string(toString(gone.side)) + " name=" + gone.name +
                        " removed" + (reply.empty() ? "" : "\n" + reply);
                edited.components.erase(edited.components.begin() +
                                        static_cast<std::ptrdiff_t>(*it));
            }
            return commitTemplates(document, set, "ROADTEMPLATE_REMOVE", reply);
        }
        // SET takes any number of groups, <component> key=value ..., and
        // applies them all to the one copy before committing it: a grid of
        // edited rows (the Road Template Manager's Apply) is ONE undo step, and
        // a value that any group is refused for changes nothing. Each group
        // names its component in the copy as the groups before it left it, as
        // the same lines typed one after the other would.
        std::size_t at = 2;
        while (at < args.size()) {
            auto named = componentsNamed(edited, args[at]);
            if (!named) {
                return named.error();
            }
            std::size_t end = at + 1;
            std::vector<Option> options;
            while (end < args.size()) {
                auto option = optionOf(args[end]);
                if (!option) {
                    break; // the next group's component
                }
                options.push_back(std::move(*option));
                ++end;
            }
            if (options.empty()) {
                return usage("ROADTEMPLATE SET <template> <component> needs key=value: " +
                             args[at] + " has none");
            }
            for (const std::size_t i : *named) {
                for (const Option& option : options) {
                    if (auto status = applyComponent(edited.components[i], option); !status) {
                        return status.error();
                    }
                }
                reply += (reply.empty() ? "" : "\n") +
                         componentRecord(edited, edited.components[i]) + " updated";
            }
            at = end;
        }
        return commitTemplates(document, set, "ROADTEMPLATE_SET", reply);
    }
    if (sub == "COURSE") {
        if (args.size() < 5) {
            return usage("ROADTEMPLATE COURSE <template> <component> ADD|SET|REMOVE <course> "
                         "[key=value ...]");
        }
        auto indices = componentsNamed(roadTemplate, args[2]);
        if (!indices) {
            return indices.error();
        }
        const std::string action = upper(args[3]);
        if (action == "SET") {
            // Any number of groups, <course> key=value ..., on the one copy:
            // as ROADTEMPLATE SET <t> <component> ..., one undo step that a
            // value refused anywhere leaves undone.
            RoadTemplateSet set = current;
            RoadTemplate& edited = set.templates[index];
            std::string reply;
            std::size_t at = 4;
            while (at < args.size()) {
                const std::string& setName = args[at];
                std::size_t end = at + 1;
                std::vector<Option> options;
                while (end < args.size()) {
                    auto option = optionOf(args[end]);
                    if (!option) {
                        break; // the next group's course
                    }
                    options.push_back(std::move(*option));
                    ++end;
                }
                if (options.empty()) {
                    return usage("ROADTEMPLATE COURSE ... SET <course> needs key=value: " +
                                 setName + " has none");
                }
                for (const std::size_t i : *indices) {
                    Component& component = edited.components[i];
                    auto course = std::ranges::find_if(
                        component.courses,
                        [&](const PavementCourse& c) { return is(c.name, setName); });
                    if (course == component.courses.end()) {
                        return makeError(ErrorCode::NotFound,
                                         std::string(toString(component.side)) + ":" +
                                             component.name + " has no course " + setName);
                    }
                    for (const Option& option : options) {
                        if (auto status = applyCourse(*course, option); !status) {
                            return status.error();
                        }
                    }
                    reply += (reply.empty() ? "" : "\n") + courseRecord(edited, component, *course) +
                             " updated";
                }
                at = end;
            }
            return commitTemplates(document, set, "ROADTEMPLATE_COURSE", reply);
        }
        const std::string& courseName = args[4];
        auto options = optionsFrom(args, 5);
        if (!options) {
            return options.error();
        }
        RoadTemplateSet set = current;
        RoadTemplate& edited = set.templates[index];
        std::string reply;
        for (const std::size_t i : *indices) {
            Component& component = edited.components[i];
            auto course = std::ranges::find_if(
                component.courses, [&](const PavementCourse& c) { return is(c.name, courseName); });
            if (action == "ADD") {
                if (course != component.courses.end()) {
                    return makeError(ErrorCode::AlreadyExists,
                                     std::string(toString(component.side)) + ":" + component.name +
                                         " has a course called " + courseName);
                }
                if (auto status = checkWord("a course", courseName); !status) {
                    return status.error();
                }
                PavementCourse made = makeCourse(courseName, 0.0, "");
                std::optional<std::size_t> at;
                bool colourGiven = false;
                for (const Option& option : *options) {
                    if (option.key == "at") {
                        const auto position = katana::core::parseInteger(option.value);
                        if (!position || *position < 1) {
                            return usage("at= is a position from 1, not \"" + option.value + "\"");
                        }
                        at = static_cast<std::size_t>(*position);
                        continue;
                    }
                    colourGiven = colourGiven || option.key == "colour" || option.key == "color";
                    if (auto status = applyCourse(made, option); !status) {
                        return status.error();
                    }
                }
                if (!colourGiven) {
                    made.colour = makeCourse(made.name, 0.0, made.material).colour;
                }
                const std::size_t position =
                    at ? std::min(*at - 1, component.courses.size()) : component.courses.size();
                reply +=
                    (reply.empty() ? "" : "\n") + courseRecord(edited, component, made) + " added";
                component.courses.insert(component.courses.begin() +
                                             static_cast<std::ptrdiff_t>(position),
                                         std::move(made));
                continue;
            }
            if (course == component.courses.end()) {
                return makeError(ErrorCode::NotFound, std::string(toString(component.side)) + ":" +
                                                          component.name + " has no course " +
                                                          courseName);
            }
            if (action == "REMOVE") {
                reply += std::string(reply.empty() ? "" : "\n") +
                         "course template=" + text(edited.name) +
                         " component=" + std::string(toString(component.side)) + ":" +
                         component.name + " name=" + course->name + " removed";
                component.courses.erase(course);
                continue;
            }
            return usage("ROADTEMPLATE COURSE ... ADD, SET or REMOVE, not " + args[3]);
        }
        return commitTemplates(document, set, "ROADTEMPLATE_COURSE", reply);
    }
    return usage("unknown ROADTEMPLATE word " + args[0] + " (HELP ROADTEMPLATE)");
}

// ---- corridors --------------------------------------------------------------------------

std::string corridorRecord(const Corridor& corridor)
{
    return "corridor name=" + text(corridor.name) + " alignment=" + text(corridor.alignment) +
           " template=" + text(corridor.templateName) + " surface=" + text(corridor.surface) +
           " interval=" + exact(corridor.interval) +
           " from=" + (corridor.start ? exact(*corridor.start) : std::string()) +
           " to=" + (corridor.end ? exact(*corridor.end) : std::string()) +
           " regions=" + std::to_string(corridor.regions.size()) +
           " superelevation=" + std::to_string(corridor.superelevation.size()) +
           " curvetol=" + exact(corridor.curveTolerance);
}

// The layers a corridor's objects are shown on that the drawing does not
// have, parents first, the cut's switched off (bim_view.hpp).
std::vector<katana::entity::Layer> missingLayers(const Document& document,
                                                 const std::vector<std::string>& paths)
{
    std::vector<katana::entity::Layer> layers;
    std::set<std::string> planned;
    for (const std::string& path : paths) {
        if (path.empty() || !katana::entity::validateLayerPath(path)) {
            continue;
        }
        std::vector<std::string> chain = katana::entity::layerAncestors(path);
        chain.push_back(path);
        for (const std::string& name : chain) {
            if (document.model().layers.contains(name) || !planned.insert(name).second) {
                continue;
            }
            katana::entity::Layer layer;
            layer.name = name;
            layer.visible = name != kCutLayer;
            layers.push_back(std::move(layer));
        }
    }
    return layers;
}

std::vector<std::string> objectLayers(const Corridor& corridor, const RoadTemplateSet& templates)
{
    std::vector<std::string> paths;
    const auto add = [&](std::string_view name) {
        if (const RoadTemplate* roadTemplate = templates.find(name)) {
            for (const Component& component : roadTemplate->components) {
                paths.push_back(component.layer);
            }
        }
    };
    add(corridor.templateName);
    for (const TemplateRegion& region : corridor.regions) {
        add(region.templateName);
    }
    paths.emplace_back(kCutLayer);
    paths.emplace_back(kFillLayer);
    return paths;
}

// The corridors replaced by `next` as one step, with the layers its objects
// need made in it. Empty: nothing changed.
Result<std::size_t> commitCorridors(Document& document, const CorridorSet& next,
                                    const Corridor* layersFor, const std::string& stepName)
{
    auto command = document.corridorsCommand(next, stepName);
    if (!command) {
        return command.error();
    }
    const std::vector<katana::entity::Layer> layers =
        layersFor != nullptr
            ? missingLayers(document, objectLayers(*layersFor, document.roadTemplates()))
            : std::vector<katana::entity::Layer>{};
    if (*command == nullptr && layers.empty()) {
        return std::size_t{0};
    }
    auto transaction = std::make_unique<cmd::Transaction>(stepName);
    for (const katana::entity::Layer& layer : layers) {
        transaction->add(cmd::createLayer(layer));
    }
    if (*command) {
        transaction->add(std::move(*command));
    }
    if (auto status = document.execute(std::move(transaction)); !status) {
        return status.error();
    }
    return layers.size();
}

std::string builtRecord(const BuiltCorridor& built, std::size_t objects)
{
    // Absent is not zero: a corridor that could not be built has no range (station
    // 0 is a real station), and one with no target surface measured no cut or
    // fill - the problems say why. The key is there and empty, as a
    // corridor's own from= and to= are when it gives none.
    const auto measured = [](bool present, double value) {
        return present ? fixed(value) : std::string();
    };
    return "built corridor=" + text(built.name) + " from=" + measured(built.hasRange, built.start) +
           " to=" + measured(built.hasRange, built.end) +
           " runs=" + std::to_string(built.runs.size()) +
           " sections=" + std::to_string(built.sections) +
           " incomplete=" + std::to_string(built.incompleteSections) +
           " objects=" + std::to_string(objects) + " cut=" + measured(built.hasEarthworks, built.cut) +
           " fill=" + measured(built.hasEarthworks, built.fill) +
           " net=" + measured(built.hasEarthworks, built.fill - built.cut) +
           " problems=" + std::to_string(built.problems.size());
}

std::string problemLines(const std::vector<std::string>& problems)
{
    std::string out;
    for (const std::string& problem : problems) {
        out += "\nproblem message=" + katana::core::replyQuoted(problem);
    }
    return out;
}

Status checkNames(const Document& document, const Corridor& corridor)
{
    const katana::entity::Alignment* alignment =
        document.model().alignments.find(corridor.alignment);
    if (alignment == nullptr) {
        return makeError(ErrorCode::NotFound,
                         "no alignment " + corridor.alignment + ": ALIGN LIST names them");
    }
    if (!alignment->vertical) {
        return makeError(ErrorCode::InvalidArgument,
                         "the alignment " + corridor.alignment +
                             " has no design profile: ALIGN DESIGN gives it one");
    }
    if (document.roadTemplates().find(corridor.templateName) == nullptr) {
        return noTemplate(corridor.templateName);
    }
    if (!corridor.surface.empty() && document.surfaces().find(corridor.surface) == nullptr) {
        return makeError(ErrorCode::NotFound,
                         "no surface " + corridor.surface + ": SURFACE LIST names them");
    }
    return {};
}

Status applyCorridor(Corridor& corridor, const Option& option)
{
    const auto station = [&](std::optional<double>& field) -> Status {
        if (option.value.empty()) {
            field.reset();
            return {};
        }
        auto value = numberOf(option);
        if (!value) {
            return value.error();
        }
        field = *value;
        return {};
    };
    // A corridor needs an alignment and a template; the other keys can be
    // emptied (surface, from, to) or have a default (interval, 10 m).
    if ((option.key == "alignment" || option.key == "template") && option.value.empty()) {
        return usage(option.key + "= needs a name: a corridor has " +
                     (option.key == "alignment" ? "an alignment (ALIGN LIST names them)"
                                                : "a road template (ROADTEMPLATE LIST names them)"));
    }
    if (option.key == "alignment") {
        corridor.alignment = option.value;
        return {};
    }
    if (option.key == "template") {
        corridor.templateName = option.value;
        return {};
    }
    if (option.key == "surface") {
        corridor.surface = option.value;
        return {};
    }
    if (option.key == "interval") {
        auto value = numberOf(option);
        if (!value) {
            return value.error();
        }
        corridor.interval = *value;
        return {};
    }
    if (option.key == "curvetol") {
        // "off" is 0, as the record writes it: no section is added in a curve.
        if (is(option.value, "off")) {
            corridor.curveTolerance = 0.0;
            return {};
        }
        auto value = numberOf(option);
        if (!value) {
            return value.error();
        }
        corridor.curveTolerance = *value;
        return {};
    }
    if (option.key == "from") {
        return station(corridor.start);
    }
    if (option.key == "to") {
        return station(corridor.end);
    }
    return usage("unknown corridor key " + option.key +
                 "=; the keys are alignment template surface interval from to curvetol");
}

Result<double> stationWord(const std::string& word, const char* what)
{
    const auto value = katana::core::parseFiniteDouble(word);
    if (!value) {
        return usage(std::string(what) + " is a number, not \"" + word + "\"");
    }
    return *value;
}

// The strings in the drawing that CORRIDOR STRINGS made for `corridor`: the
// entities marked with both properties, the corridor's name read as corridors
// are found, ignoring case.
std::vector<katana::entity::EntityId> stringsMadeFor(const Document& document,
                                                     std::string_view corridor)
{
    std::vector<katana::entity::EntityId> ids;
    document.model().entities.forEach([&](const katana::entity::Entity& entity) {
        const auto found = entity.properties.find(kCorridorProperty);
        if (found != entity.properties.end() && entity.properties.contains(kStringProperty) &&
            std::holds_alternative<std::string>(found->second) &&
            is(std::get<std::string>(found->second), corridor)) {
            ids.push_back(entity.id);
        }
    });
    return ids;
}

std::string quantityValue(const bim::BimObject& object, std::string_view name)
{
    for (const bim::Quantity& quantity : object.quantities) {
        if (quantity.name == name) {
            return fixed(quantity.value);
        }
    }
    return {};
}

std::string objectRecord(const bim::BimObject& object)
{
    std::string out =
        "object id=" + text(object.id) + " kind=" + object.kind + " name=" + text(object.name);
    for (const bim::Quantity& quantity : object.quantities) {
        out += " " + quantity.name + "=" + fixed(quantity.value);
    }
    return out;
}

// The class and, where it has one, its predefined type: IfcKerb,
// IfcCourse.PAVEMENT.
std::string ifcName(const bim::BimObject& object)
{
    return object.predefinedType.empty() ? object.ifcClass
                                         : object.ifcClass + "." + object.predefinedType;
}

bool belongsTo(const bim::BimObject& object, const std::string* corridor)
{
    if (!object.id.starts_with("corridor/")) {
        return false;
    }
    return corridor == nullptr || is(object.facility, *corridor);
}

// A problem of the corridors, or of the definitions they are made from: not a
// placed road object's (source "object <id>" or "road objects"), which
// ROADMODEL PROBLEMS lists with them. With a corridor named, its own only.
bool corridorProblem(const bim::BimProblem& problem, const std::string* corridor)
{
    if (corridor != nullptr) {
        return is(problem.source, "corridor " + *corridor);
    }
    return is(problem.source, "road templates") || is(problem.source, "corridors") ||
           problem.source.starts_with("corridor ");
}

Result<std::string> quantitiesVerb(Document& document, const Tokens& args)
{
    if (args.size() < 2) {
        return usage("CORRIDOR QUANTITIES <corridor>|ALL [csv=<file> [OVERWRITE]]");
    }
    const bool all = is(args[1], "ALL");
    const Corridor* corridor = all ? nullptr : document.corridors().find(args[1]);
    if (!all && corridor == nullptr) {
        return noCorridor(args[1]);
    }
    // OVERWRITE is a word, as the other report verbs have it, not a key.
    Tokens words(args.begin(), args.begin() + 2);
    bool overwrite = false;
    for (std::size_t i = 2; i < args.size(); ++i) {
        if (is(args[i], "OVERWRITE")) {
            overwrite = true;
        } else {
            words.push_back(args[i]);
        }
    }
    auto options = optionsFrom(words, 2);
    if (!options) {
        return options.error();
    }
    std::optional<std::string> csv;
    for (const Option& option : *options) {
        if (option.key != "csv") {
            return usage("CORRIDOR QUANTITIES takes csv=<file>, not " + option.key + "=");
        }
        csv = option.value;
    }
    if (overwrite && !csv) {
        return usage("OVERWRITE replaces the file csv=<file> names: CORRIDOR QUANTITIES "
                     "<corridor>|ALL csv=<file> OVERWRITE");
    }
    const auto model = bim::bimModel(document);
    const std::string* name = corridor != nullptr ? &corridor->name : nullptr;
    std::vector<bim::BimObject> objects;
    double cut = 0.0;
    double fill = 0.0;
    // A total has a length, an area or a volume only if some object of its kind
    // carries one: a kerb has no area, and a total of nothing is not 0.000.
    struct Total {
        std::size_t count = 0;
        std::optional<double> length;
        std::optional<double> area;
        std::optional<double> volume;
    };
    std::map<std::pair<std::string, std::string>, Total> totals;
    for (const bim::BimObject& object : model->objects) {
        if (!belongsTo(object, name)) {
            continue;
        }
        objects.push_back(object);
        double volume = 0.0;
        Total& total = totals[{object.kind, object.material}];
        ++total.count;
        for (const bim::Quantity& quantity : object.quantities) {
            if (quantity.name == "GrossVolume") {
                volume = quantity.value;
                total.volume = total.volume.value_or(0.0) + quantity.value;
            } else if (quantity.name == "GrossArea") {
                total.area = total.area.value_or(0.0) + quantity.value;
            } else if (quantity.name == "Length") {
                total.length = total.length.value_or(0.0) + quantity.value;
            }
        }
        if (object.ifcClass == "IfcEarthworksCut") {
            cut += volume;
        } else if (object.ifcClass == "IfcEarthworksFill") {
            fill += volume;
        }
    }
    // Cut and fill are measured only against a target surface: a corridor with
    // none, or whose alignment, profile or surface the drawing lacks, reports
    // them absent (empty), not 0.000, which would read as a road with no cut.
    bool measured = false;
    for (const Corridor& each : document.corridors().corridors) {
        if (name == nullptr || is(each.name, *name)) {
            const CorridorInputs inputs = corridorInputs(document, each);
            measured = measured || (inputs.alignment && inputs.profile && inputs.ground != nullptr);
        }
    }
    const auto sum = [measured](double value) {
        return measured ? fixed(value) : std::string();
    };
    const auto shown = [](const std::optional<double>& value) {
        return value ? fixed(*value) : std::string();
    };
    std::string reply = "quantities corridor=" + (all ? std::string("all") : text(*name)) +
                        " objects=" + std::to_string(objects.size()) + " cut=" + sum(cut) +
                        " fill=" + sum(fill) + " net=" + sum(fill - cut);
    for (const bim::BimObject& object : objects) {
        reply += "\n" + objectRecord(object);
    }
    for (const auto& [key, total] : totals) {
        reply += "\ntotal kind=" + key.first + " material=" + text(key.second) +
                 " count=" + std::to_string(total.count) + " length=" + shown(total.length) +
                 " area=" + shown(total.area) + " volume=" + shown(total.volume);
    }
    for (const bim::BimProblem& problem : model->problems) {
        if (corridorProblem(problem, name)) {
            reply += "\nproblem source=" + text(problem.source) +
                     " message=" + katana::core::replyQuoted(problem.message);
        }
    }
    if (csv) {
        auto written = writeCsvTable(quantitiesTable(objects), *csv, overwrite);
        if (!written) {
            return written.error();
        }
        reply += "\n" + *written;
    }
    return reply;
}

Result<std::string> stringsVerb(Document& document, const Corridor& corridor, const Tokens& args)
{
    std::string layer(kStringsLayer);
    bool preview = false;
    for (std::size_t i = 2; i < args.size(); ++i) {
        if (is(args[i], "PREVIEW")) {
            preview = true;
            continue;
        }
        const auto option = optionOf(args[i]);
        if (!option || option->key != "layer") {
            return usage("CORRIDOR STRINGS <corridor> [layer=<layer>] [PREVIEW]");
        }
        if (auto status = katana::entity::validateLayerPath(option->value); !status) {
            return status.error();
        }
        layer = option->value;
    }
    const BuiltCorridor built = buildCorridor(document, corridor);
    const std::vector<CorridorString> strings = corridorStrings(built, false);
    if (strings.empty()) {
        return makeError(
            ErrorCode::InvalidState,
            "corridor " + corridor.name + " has no sections to string" +
                (built.problems.empty() ? std::string() : ": " + built.problems.front()));
    }
    const std::vector<katana::entity::EntityId> previous = stringsMadeFor(document, corridor.name);
    std::size_t points = 0;
    std::vector<katana::entity::Entity> entities;
    for (const CorridorString& string : strings) {
        katana::entity::Entity entity;
        katana::geometry::Polyline2 line;
        std::vector<std::optional<double>> heights;
        for (const katana::geometry::Point3& point : string.points) {
            line.vertices.emplace_back(point.x, point.y);
            heights.emplace_back(point.z);
        }
        points += string.points.size();
        entity.geometry = std::move(line);
        entity.layer = layer;
        entity.properties.insert_or_assign(std::string(kCorridorProperty), corridor.name);
        entity.properties.insert_or_assign(std::string(kStringProperty), string.code);
        katana::entity::setHeights(entity.properties, heights);
        entities.push_back(std::move(entity));
    }
    const std::string record = " corridor=" + text(corridor.name) + " layer=" + text(layer) +
                               " points=" + std::to_string(points);
    if (preview) {
        return "strings created=" + std::to_string(entities.size()) +
               " replaced=" + std::to_string(previous.size()) + record +
               " preview=yes (nothing changed)";
    }
    auto transaction = std::make_unique<cmd::Transaction>("CORRIDOR_STRINGS");
    if (!previous.empty()) {
        transaction->add(cmd::deleteEntities(previous));
    }
    for (const katana::entity::Layer& made : missingLayers(document, {layer})) {
        transaction->add(cmd::createLayer(made));
    }
    const std::size_t created = entities.size();
    transaction->add(cmd::createEntities(std::move(entities)));
    if (auto status = document.execute(std::move(transaction)); !status) {
        return status.error();
    }
    return "strings created=" + std::to_string(created) +
           " replaced=" + std::to_string(previous.size()) + record;
}

Result<std::string> surfaceVerb(Document& document, const Corridor& corridor, const Tokens& args)
{
    std::string finishedName = corridor.name + " finished";
    std::string subgradeName = corridor.name + " subgrade";
    bool preview = false;
    for (std::size_t i = 2; i < args.size(); ++i) {
        if (is(args[i], "PREVIEW")) {
            preview = true;
            continue;
        }
        const auto option = optionOf(args[i]);
        if (option && option->key == "finished" && !option->value.empty()) {
            finishedName = option->value;
        } else if (option && option->key == "subgrade" && !option->value.empty()) {
            subgradeName = option->value;
        } else {
            return usage(
                "CORRIDOR SURFACE <corridor> [finished=<name>] [subgrade=<name>] [PREVIEW]");
        }
    }
    if (is(finishedName, subgradeName)) {
        return usage("the finished surface and the subgrade need two names");
    }
    const BuiltCorridor built = buildCorridor(document, corridor);
    if (!built.alignment) {
        return makeError(
            ErrorCode::InvalidState,
            "corridor " + corridor.name + " cannot be built" +
                (built.problems.empty() ? std::string() : ": " + built.problems.front()));
    }
    const std::vector<const RoadSection*> sections = corridorSections(built);
    if (sections.size() < 2) {
        return makeError(
            ErrorCode::InvalidState,
            "corridor " + corridor.name + " has fewer than two sections to make a surface of" +
                (built.problems.empty() ? std::string() : ": " + built.problems.front()));
    }
    auto finished = sectionsSurface(sections, *built.alignment, false, false);
    if (!finished) {
        return finished.error();
    }
    auto subgrade = sectionsSurface(sections, *built.alignment, true, false);
    if (!subgrade) {
        return subgrade.error();
    }
    const std::string reply =
        "surface name=" + text(finishedName) +
        " kind=finished triangles=" + std::to_string(finished->surface.triangleCount()) +
        " sections=" + std::to_string(finished->sections) + "\nsurface name=" + text(subgradeName) +
        " kind=subgrade triangles=" + std::to_string(subgrade->surface.triangleCount()) +
        " sections=" + std::to_string(subgrade->sections);
    if (preview) {
        return reply + "\npreview=yes (nothing changed)";
    }
    katana::terrain::SurfaceStore store = document.surfaces();
    const auto place = [&](const std::string& surfaceName,
                           katana::terrain::TinSurface surface) -> Status {
        auto shared = std::make_shared<const katana::terrain::TinSurface>(std::move(surface));
        if (store.find(surfaceName) != nullptr) {
            return store.replace(surfaceName, std::move(shared));
        }
        return store.add(katana::terrain::NamedSurface{
            surfaceName, std::move(shared), "corridor " + corridor.name, {}, true});
    };
    if (auto status = place(finishedName, std::move(finished->surface)); !status) {
        return status.error();
    }
    if (auto status = place(subgradeName, std::move(subgrade->surface)); !status) {
        return status.error();
    }
    if (auto status = document.setSurfaces(std::move(store), "CORRIDOR_SURFACE"); !status) {
        return status.error();
    }
    return reply;
}

Result<std::string> corridorVerb(Document& document, const Tokens& args)
{
    const std::string sub = args.empty() ? std::string("LIST") : upper(args[0]);
    const CorridorSet& current = document.corridors();
    if (sub == "LIST" || sub == "JSON") {
        const bool json = sub == "JSON" || (args.size() > 1 && is(args[1], "JSON"));
        if (const Status status = document.corridorsStatus(); !status) {
            return status.error();
        }
        if (json) {
            return corridorsToJson(current);
        }
        if (current.corridors.empty()) {
            return std::string("corridors=0 (CORRIDOR NEW makes one)");
        }
        std::string reply = "corridors=" + std::to_string(current.corridors.size());
        for (const Corridor& corridor : current.corridors) {
            reply += "\n" + corridorRecord(corridor);
        }
        return reply;
    }
    if (sub == "PROBLEMS") {
        const auto model = bim::bimModel(document);
        std::optional<std::string> named;
        if (args.size() > 1) {
            const Corridor* corridor = current.find(args[1]);
            if (corridor == nullptr) {
                return noCorridor(args[1]);
            }
            named = corridor->name;
        }
        std::string reply;
        std::size_t count = 0;
        for (const bim::BimProblem& problem : model->problems) {
            if (!corridorProblem(problem, named ? &*named : nullptr)) {
                continue;
            }
            ++count;
            reply += "\nproblem source=" + text(problem.source) +
                     " message=" + katana::core::replyQuoted(problem.message);
        }
        return "problems=" + std::to_string(count) + reply;
    }
    if (sub == "QUANTITIES") {
        return quantitiesVerb(document, args);
    }
    if (sub == "BUILD" && args.size() > 1 && is(args[1], "ALL")) {
        // Corridors that cannot be read are not "none": the same refusal as
        // CORRIDOR LIST, not a reply that says to make one.
        if (auto status = corridorsReadable(document); !status) {
            return status.error();
        }
        if (current.corridors.empty()) {
            return std::string("corridors=0 (CORRIDOR NEW makes one)");
        }
        std::string reply;
        for (const Corridor& corridor : current.corridors) {
            const BuiltCorridor built = buildCorridor(document, corridor);
            const bim::BimModel objects = corridorSolids(built);
            reply += (reply.empty() ? "" : "\n") + builtRecord(built, objects.objects.size()) +
                     problemLines(built.problems);
        }
        return reply;
    }
    if (auto status = corridorsReadable(document); !status) {
        return status.error();
    }
    if (args.size() < 2) {
        return usage("CORRIDOR " + sub + " needs a corridor's name (HELP CORRIDOR)");
    }
    const std::string& name = args[1];
    if (sub == "NEW") {
        if (auto status = checkName("a corridor", name); !status) {
            return status.error();
        }
        if (current.find(name) != nullptr) {
            return makeError(ErrorCode::AlreadyExists, "there is a corridor called " + name);
        }
        Corridor made;
        made.name = name;
        std::size_t at = 2;
        while (at + 1 < args.size() && !optionOf(args[at])) {
            if (is(args[at], "ALIGNMENT")) {
                made.alignment = args[at + 1];
            } else if (is(args[at], "TEMPLATE")) {
                made.templateName = args[at + 1];
            } else {
                break;
            }
            at += 2;
        }
        if (made.alignment.empty() || made.templateName.empty()) {
            return usage("CORRIDOR NEW <corridor> ALIGNMENT <alignment> TEMPLATE <template> "
                         "[surface=<name>] [interval=<m>] [from=<s>] [to=<s>] [curvetol=<e>]");
        }
        auto options = optionsFrom(args, at);
        if (!options) {
            return options.error();
        }
        for (const Option& option : *options) {
            if (option.key == "alignment" || option.key == "template") {
                return usage("CORRIDOR NEW names its alignment and template by ALIGNMENT and "
                             "TEMPLATE");
            }
            if (auto status = applyCorridor(made, option); !status) {
                return status.error();
            }
        }
        if (auto status = checkNames(document, made); !status) {
            return status.error();
        }
        CorridorSet set = current;
        set.corridors.push_back(made);
        auto layers = commitCorridors(document, set, &made, "CORRIDOR_NEW");
        if (!layers) {
            return layers.error();
        }
        const BuiltCorridor built = buildCorridor(document, made);
        const bim::BimModel objects = corridorSolids(built);
        return corridorRecord(made) + " created layers=" + std::to_string(*layers) + "\n" +
               builtRecord(built, objects.objects.size()) + problemLines(built.problems);
    }
    const Corridor* found = current.find(name);
    if (found == nullptr) {
        return noCorridor(name);
    }
    const Corridor& corridor = *found;
    const std::size_t index = static_cast<std::size_t>(found - current.corridors.data());
    if (sub == "INFO") {
        std::string reply = corridorRecord(corridor);
        for (const TemplateRegion& region : corridor.regions) {
            reply += "\nregion from=" + exact(region.start) + " to=" + exact(region.end) +
                     " template=" + text(region.templateName);
        }
        for (const SuperelevationPoint& point : corridor.superelevation) {
            reply += "\nsuperelevation station=" + exact(point.station) +
                     " left=" + exact(point.left) + " right=" + exact(point.right);
        }
        return reply;
    }
    if (sub == "BUILD") {
        const BuiltCorridor built = buildCorridor(document, corridor);
        const bim::BimModel objects = corridorSolids(built);
        return builtRecord(built, objects.objects.size()) + problemLines(built.problems);
    }
    if (sub == "SECTION") {
        if (args.size() < 3) {
            return usage("CORRIDOR SECTION <corridor> <station>");
        }
        auto station = stationWord(args[2], "a station");
        if (!station) {
            return station.error();
        }
        const CorridorInputs inputs = corridorInputs(document, corridor);
        if (!inputs.alignment || !inputs.profile) {
            return makeError(
                ErrorCode::InvalidState,
                "corridor " + corridor.name + " cannot be built" +
                    (inputs.problems.empty() ? std::string() : ": " + inputs.problems.front()));
        }
        // The corridor has sections only over its range (from= to= held to the
        // alignment and the profile): a section outside it would look built and
        // be nothing the corridor has.
        const CorridorRange range = corridorRange(corridor, *inputs.alignment, *inputs.profile);
        if (!range.ok) {
            return makeError(ErrorCode::InvalidState,
                             "corridor " + corridor.name + " has no stations to section" +
                                 (range.problems.empty() ? std::string()
                                                         : ": " + range.problems.front()));
        }
        if (*station < range.start - tol::kGeometric || *station > range.end + tol::kGeometric) {
            return makeError(ErrorCode::InvalidArgument,
                             "station " + exact(*station) + " is outside corridor " +
                                 corridor.name + "'s stations " + exact(range.start) + " to " +
                                 exact(range.end));
        }
        const RoadTemplate* roadTemplate =
            document.roadTemplates().find(templateAt(corridor, *station));
        if (roadTemplate == nullptr) {
            return noTemplate(templateAt(corridor, *station));
        }
        auto section = roadSection(*roadTemplate, *inputs.alignment, *inputs.profile, inputs.ground,
                                   *station, superelevationAt(corridor, *station));
        if (!section) {
            return section.error();
        }
        std::string reply =
            "section corridor=" + text(corridor.name) + " station=" + fixed(section->station) +
            " template=" + text(roadTemplate->name) + " design=" + fixed(section->designLevel) +
            " complete=" + yes(section->complete) + " cut=" + fixed(section->cutArea) +
            " fill=" + fixed(section->fillArea) + " leftcut=" + fixed(section->leftCut) +
            " leftfill=" + fixed(section->leftFill) + " rightcut=" + fixed(section->rightCut) +
            " rightfill=" + fixed(section->rightFill);
        for (const SectionPoint& point : section->top) {
            reply += "\npoint code=" + point.code + " offset=" + fixed(point.offset) +
                     " level=" + fixed(point.level);
        }
        for (const SectionPoint& point : section->formation) {
            reply += "\nformation code=" + point.code + " offset=" + fixed(point.offset) +
                     " level=" + fixed(point.level);
        }
        return reply;
    }
    if (sub == "STRINGS") {
        return stringsVerb(document, corridor, args);
    }
    if (sub == "SURFACE") {
        return surfaceVerb(document, corridor, args);
    }
    CorridorSet set = current;
    Corridor& edited = set.corridors[index];
    if (sub == "DELETE") {
        set.corridors.erase(set.corridors.begin() + static_cast<std::ptrdiff_t>(index));
        auto done = commitCorridors(document, set, nullptr, "CORRIDOR_DELETE");
        if (!done) {
            return done.error();
        }
        return "corridor name=" + text(corridor.name) + " deleted";
    }
    if (sub == "RENAME") {
        if (args.size() < 3) {
            return usage("CORRIDOR RENAME <corridor> <new name>");
        }
        if (auto status = checkName("a corridor", args[2]); !status) {
            return status.error();
        }
        if (const Corridor* other = current.find(args[2]); other != nullptr && other != found) {
            return makeError(ErrorCode::AlreadyExists, "there is a corridor called " + args[2]);
        }
        // The strings it drew carry its name (road.corridor), which is how its
        // next STRINGS finds them to replace: they follow it, in the same step,
        // or the next STRINGS would leave them beside a second set, and a corridor
        // later given the old name would delete them.
        const std::vector<katana::entity::EntityId> strings =
            stringsMadeFor(document, corridor.name);
        edited.name = args[2];
        auto command = document.corridorsCommand(set, "CORRIDOR_RENAME");
        if (!command) {
            return command.error();
        }
        auto transaction = std::make_unique<cmd::Transaction>("CORRIDOR_RENAME");
        if (*command) {
            transaction->add(std::move(*command));
        }
        if (!strings.empty()) {
            transaction->add(cmd::setEntityProperty(strings, std::string(kCorridorProperty),
                                                    katana::entity::PropertyValue(args[2])));
        }
        if (transaction->size() > 0) {
            if (auto status = document.execute(std::move(transaction)); !status) {
                return status.error();
            }
        }
        return "corridor renamed from=" + text(corridor.name) + " to=" + text(args[2]) +
               " strings=" + std::to_string(strings.size());
    }
    if (sub == "SET") {
        auto options = optionsFrom(args, 2);
        if (!options) {
            return options.error();
        }
        if (options->empty()) {
            return usage("CORRIDOR SET <corridor> key=value ...");
        }
        for (const Option& option : *options) {
            if (auto status = applyCorridor(edited, option); !status) {
                return status.error();
            }
        }
        if (auto status = checkNames(document, edited); !status) {
            return status.error();
        }
        auto layers = commitCorridors(document, set, &edited, "CORRIDOR_SET");
        if (!layers) {
            return layers.error();
        }
        return corridorRecord(edited) + " updated layers=" + std::to_string(*layers);
    }
    if (sub == "REGION" || sub == "SUPERELEVATION") {
        const std::string action = args.size() > 2 ? upper(args[2]) : std::string();
        if (sub == "REGION") {
            if (action == "ADD") {
                if (args.size() != 6) {
                    return usage("CORRIDOR REGION <corridor> ADD <from> <to> <template>");
                }
                auto from = stationWord(args[3], "a region's start");
                auto to = stationWord(args[4], "a region's end");
                if (!from || !to) {
                    return (!from ? from : to).error();
                }
                if (document.roadTemplates().find(args[5]) == nullptr) {
                    return noTemplate(args[5]);
                }
                edited.regions.push_back(TemplateRegion{*from, *to, args[5]});
                std::ranges::sort(edited.regions, {}, &TemplateRegion::start);
            } else if (action == "REMOVE") {
                if (args.size() != 4) {
                    return usage("CORRIDOR REGION <corridor> REMOVE <from>");
                }
                auto from = stationWord(args[3], "a region's start");
                if (!from) {
                    return from.error();
                }
                const auto before = edited.regions.size();
                std::erase_if(edited.regions, [&](const TemplateRegion& region) {
                    return std::abs(region.start - *from) <= tol::kGeometric;
                });
                if (edited.regions.size() == before) {
                    return makeError(ErrorCode::NotFound, "corridor " + corridor.name +
                                                              " has no region from " + args[3]);
                }
            } else if (action == "CLEAR") {
                edited.regions.clear();
            } else {
                return usage("CORRIDOR REGION <corridor> ADD <from> <to> <template> | REMOVE "
                             "<from> | CLEAR");
            }
        } else {
            if (action == "ADD") {
                if (args.size() != 6) {
                    return usage(
                        "CORRIDOR SUPERELEVATION <corridor> ADD <station> <left%> <right%>");
                }
                auto station = stationWord(args[3], "a station");
                auto left = stationWord(args[4], "a crossfall");
                auto right = stationWord(args[5], "a crossfall");
                if (!station || !left || !right) {
                    return (!station ? station : !left ? left : right).error();
                }
                // An entry at a station already in the table replaces it.
                std::erase_if(edited.superelevation, [&](const SuperelevationPoint& point) {
                    return std::abs(point.station - *station) <= tol::kGeometric;
                });
                edited.superelevation.push_back(SuperelevationPoint{*station, *left, *right});
                std::ranges::sort(edited.superelevation, {}, &SuperelevationPoint::station);
            } else if (action == "REMOVE") {
                if (args.size() != 4) {
                    return usage("CORRIDOR SUPERELEVATION <corridor> REMOVE <station>");
                }
                auto station = stationWord(args[3], "a station");
                if (!station) {
                    return station.error();
                }
                const auto before = edited.superelevation.size();
                std::erase_if(edited.superelevation, [&](const SuperelevationPoint& point) {
                    return std::abs(point.station - *station) <= tol::kGeometric;
                });
                if (edited.superelevation.size() == before) {
                    return makeError(ErrorCode::NotFound, "corridor " + corridor.name +
                                                              " has no superelevation at " +
                                                              args[3]);
                }
            } else if (action == "CLEAR") {
                edited.superelevation.clear();
            } else {
                return usage("CORRIDOR SUPERELEVATION <corridor> ADD <station> <left%> <right%> "
                             "| REMOVE <station> | CLEAR");
            }
        }
        auto layers = commitCorridors(document, set, &edited, "CORRIDOR_" + sub);
        if (!layers) {
            return layers.error();
        }
        std::string reply = corridorRecord(edited) + " updated";
        if (sub == "REGION") {
            for (const TemplateRegion& region : edited.regions) {
                reply += "\nregion from=" + exact(region.start) + " to=" + exact(region.end) +
                         " template=" + text(region.templateName);
            }
        } else {
            for (const SuperelevationPoint& point : edited.superelevation) {
                reply += "\nsuperelevation station=" + exact(point.station) +
                         " left=" + exact(point.left) + " right=" + exact(point.right);
            }
        }
        return reply;
    }
    return usage("unknown CORRIDOR word " + args[0] + " (HELP CORRIDOR)");
}

// ---- the whole model ------------------------------------------------------------------------

Result<std::string> roadModelVerb(const Document& document, const Tokens& args)
{
    const std::string sub = args.empty() ? std::string("LIST") : upper(args[0]);
    const auto model = bim::bimModel(document);
    if (sub == "JSON") {
        return roadModelJson(*model);
    }
    if (sub == "PROBLEMS" || sub == "LIST") {
        std::string reply = "road model objects=" + std::to_string(model->objects.size()) +
                            " problems=" + std::to_string(model->problems.size());
        if (sub == "LIST") {
            for (const bim::BimObject& object : model->objects) {
                reply += "\nobject id=" + text(object.id) + " kind=" + object.kind +
                         " facility=" + text(object.facility) + " part=" + object.part +
                         " ifc=" + ifcName(object) +
                         " layer=" + text(object.layer) +
                         " triangles=" + std::to_string(object.mesh.triangleCount()) +
                         " volume=" + quantityValue(object, "GrossVolume");
            }
        }
        for (const bim::BimProblem& problem : model->problems) {
            reply += "\nproblem source=" + text(problem.source) +
                     " message=" + katana::core::replyQuoted(problem.message);
        }
        return reply;
    }
    if (sub == "INFO") {
        if (args.size() < 2) {
            return usage("ROADMODEL INFO <id>");
        }
        for (const bim::BimObject& object : model->objects) {
            if (object.id != args[1]) {
                continue;
            }
            std::string reply = objectRecord(object) + " facility=" + text(object.facility) +
                                " part=" + object.part + " ifc=" + ifcName(object) +
                                " material=" + text(object.material) +
                                " colour=" + object.colour.toHex() +
                                " layer=" + text(object.layer) +
                                " triangles=" + std::to_string(object.mesh.triangleCount());
            for (const auto& [key, value] : object.properties) {
                reply += "\nproperty name=" + text(key) + " value=" + text(value);
            }
            return reply;
        }
        return makeError(ErrorCode::NotFound,
                         "the road model has no object " + args[1] + ": ROADMODEL LIST names them");
    }
    return usage("unknown ROADMODEL word " + args[0] + " (HELP ROADMODEL)");
}

} // namespace

bool isRoadVerb(std::string_view verb)
{
    return is(verb, "ROADTEMPLATE") || is(verb, "CORRIDOR") || is(verb, "ROADMODEL");
}

Result<std::string> runRoadVerb(Document& document, const std::vector<std::string>& tokens)
{
    if (tokens.empty() || !isRoadVerb(tokens.front())) {
        return usage("not a road verb");
    }
    const Tokens args(tokens.begin() + 1, tokens.end());
    if (is(tokens.front(), "ROADTEMPLATE")) {
        return roadTemplateVerb(document, args);
    }
    if (is(tokens.front(), "CORRIDOR")) {
        return corridorVerb(document, args);
    }
    return roadModelVerb(document, args);
}

std::string roadVerbHelp()
{
    return R"(Road model (docs/road_design.md): road templates, corridors and their 3D objects.

ROADTEMPLATE [LIST] [JSON]             the drawing's road templates
ROADTEMPLATE INFO <t>                  its components and their courses
ROADTEMPLATE DEFAULTS                  adds the standard templates it lacks: rural two-lane,
                                       urban street, divided road
ROADTEMPLATE SECTION <t>               the template on a level straight: its points by code
ROADTEMPLATE NEW <t> [FROM <t>] [description=<text>] | DELETE <t> | RENAME <t> <new>
ROADTEMPLATE SET <t> description=<text>
ROADTEMPLATE ADD <t> left|right|both <kind> [name=<word>] [at=<n>] [key=value ...]
  kinds: lane shoulder kerb footpath verge median drain batter. A component goes before
  the side's batter, or at=<n> (from 1, outward); a second of a name is name2.
ROADTEMPLATE SET <t> <component> key=value ... [<component> key=value ...]  |  REMOVE <t> <component>
  <component> is side:name (left:lane) or name (both sides that have it). Several groups are
  one undo step, and one refused value changes none of them. Keys:
  name= width= crossfall= (percent, outward: -3 falls) superelevated=on|off layer= colour=
  kerb: shape=barrier|mountable|kerbgutter|edgestrip faces=inward|outward height= depth=
        gutter= gutterfall= material= (width= is its top)   faces=outward is a median's
        or island's kerb, which falls going outward   drain: depth= (its invert below its
        inner edge)
  batter: cut= fill= (run per rise: 2 is 1V:2H) maxwidth= benchheight= benchwidth=
ROADTEMPLATE COURSE <t> <component> ADD <course> thickness=<m> [material=] [ifc=] [colour=]
  [at=<n>]  |  SET <course> key=value ... [<course> key=value ...]  |  REMOVE <course>
  courses are top down; SET of several courses is one undo step
  ifc=: ARMOUR BALLASTBED CORE FILTER PAVEMENT PROTECTION USERDEFINED NOTDEFINED

CORRIDOR [LIST] [JSON] | INFO <c> | PROBLEMS [<c>]
CORRIDOR NEW <c> ALIGNMENT <a> TEMPLATE <t> [surface=<s>] [interval=<m>] [from=<s>] [to=<s>]
  [curvetol=<e>]   makes the layers its objects are on; road/earthworks/cut switched off
CORRIDOR SET <c> alignment= template= surface= interval= from= to= curvetol=   (surface= from=
  to= empty clear them; a corridor always has an alignment, a template and an interval)
  curvetol= is the relative error of a course's area and volume on a curve, at most (0.001
  by default, up to 0.1; 0 or off adds no sections inside curves): sections are added
  inside a curve until the chords between them are that close
CORRIDOR DELETE <c> | RENAME <c> <new>
CORRIDOR REGION <c> ADD <from> <to> <t> | REMOVE <from> | CLEAR    another template there
CORRIDOR SUPERELEVATION <c> ADD <station> <left%> <right%> | REMOVE <station> | CLEAR
  the superelevated components' crossfalls, outward, linear between entries
CORRIDOR BUILD <c>|ALL                 sections, objects, end-area cut and fill, problems
CORRIDOR SECTION <c> <station>         one section: its points, its formation, its areas
CORRIDOR STRINGS <c> [layer=<layer>] [PREVIEW]   3D strings as polylines, replacing the
                                       corridor's earlier ones, one undo step
CORRIDOR SURFACE <c> [finished=<name>] [subgrade=<name>] [PREVIEW]   both surfaces into
                                       the drawing's surfaces, one undo step
CORRIDOR QUANTITIES <c>|ALL [csv=<file> [OVERWRITE]]   each object's quantities, totals by
                                       kind and material, the earthworks cut and fill (empty
                                       with no target surface); a file already there is only
                                       replaced with OVERWRITE

ROADMODEL [LIST] | JSON | PROBLEMS | INFO <id>   every 3D object of the road model)";
}

std::string roadModelJson(const bim::BimModel& model)
{
    Json objects = Json::array();
    for (const bim::BimObject& object : model.objects) {
        Json quantities = Json::array();
        for (const bim::Quantity& quantity : object.quantities) {
            quantities.push_back(
                Json{{"name", quantity.name}, {"value", quantity.value}, {"unit", quantity.unit}});
        }
        Json properties = Json::array();
        for (const auto& [key, value] : object.properties) {
            properties.push_back(Json{{"name", key}, {"value", value}});
        }
        const katana::math::AABB box = object.mesh.bounds();
        Json bounds = Json::array();
        if (!box.empty()) {
            bounds =
                Json::array({box.min.x, box.min.y, box.min.z, box.max.x, box.max.y, box.max.z});
        }
        objects.push_back(Json{{"id", object.id},
                               {"name", object.name},
                               {"kind", object.kind},
                               {"facility", object.facility},
                               {"part", object.part},
                               {"ifcClass", object.ifcClass},
                               {"predefinedType", object.predefinedType},
                               {"material", object.material},
                               {"colour", object.colour.toHex()},
                               {"layer", object.layer},
                               {"triangles", object.mesh.triangleCount()},
                               {"bounds", std::move(bounds)},
                               {"quantities", std::move(quantities)},
                               {"properties", std::move(properties)}});
    }
    Json problems = Json::array();
    for (const bim::BimProblem& problem : model.problems) {
        problems.push_back(Json{{"source", problem.source}, {"message", problem.message}});
    }
    const Json root{{"objects", std::move(objects)}, {"problems", std::move(problems)}};
    try {
        return root.dump(2);
    } catch (const std::exception&) {
        // A name that is not UTF-8: replaced rather than refused, so the
        // model can always be read.
        return root.dump(2, ' ', false, Json::error_handler_t::replace);
    }
}

CsvTable quantitiesTable(const std::vector<bim::BimObject>& objects)
{
    static constexpr std::array<std::string_view, 6> kQuantities{
        "Length", "Width", "Depth", "Height", "GrossArea", "GrossVolume"};
    CsvTable table;
    table.header = {"id", "name", "kind", "facility", "part", "material"};
    for (const std::string_view name : kQuantities) {
        table.header.emplace_back(name);
    }
    for (const bim::BimObject& object : objects) {
        std::vector<std::string> row{object.id,       object.name, object.kind,
                                     object.facility, object.part, object.material};
        for (const std::string_view name : kQuantities) {
            row.push_back(quantityValue(object, name));
        }
        table.rows.push_back(std::move(row));
    }
    return table;
}

} // namespace katana::cad::road
