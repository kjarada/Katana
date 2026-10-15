#include "katana/cad/road/road_template.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <set>
#include <utility>

#include <nlohmann/json.hpp>

#include "katana/cad/corridor.hpp"
#include "katana/core/text.hpp"

namespace katana::cad::road {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::entity::Color;
using Json = nlohmann::json;

namespace {

constexpr std::string_view kFormat = "katana-road-templates";

constexpr std::array<std::pair<ComponentKind, std::string_view>, 8> kKinds{{
    {ComponentKind::Lane, "lane"},
    {ComponentKind::Shoulder, "shoulder"},
    {ComponentKind::Kerb, "kerb"},
    {ComponentKind::Footpath, "footpath"},
    {ComponentKind::Verge, "verge"},
    {ComponentKind::Median, "median"},
    {ComponentKind::Drain, "drain"},
    {ComponentKind::Batter, "batter"},
}};

constexpr std::array<std::pair<KerbShape, std::string_view>, 4> kShapes{{
    {KerbShape::Barrier, "barrier"},
    {KerbShape::Mountable, "mountable"},
    {KerbShape::KerbAndGutter, "kerbgutter"},
    {KerbShape::EdgeStrip, "edgestrip"},
}};

constexpr std::array<std::pair<KerbFacing, std::string_view>, 2> kFacings{{
    {KerbFacing::Inward, "inward"},
    {KerbFacing::Outward, "outward"},
}};

constexpr Color rgb(std::uint8_t r, std::uint8_t g, std::uint8_t b)
{
    return Color{r, g, b, 255};
}

// A barrier face battered 1 in 6 (horizontal over vertical): near enough
// vertical to read as one, and never vertical, which a TIN of the finished
// surface could not hold (road_template.hpp, kerbShape).
constexpr double kBarrierFaceBatter = 1.0 / 6.0;
// A mountable kerb's face runs 1.5 times its height out: a slope a wheel
// climbs, as the Green Book describes a sloping curb (AASHTO, A Policy on
// Geometric Design of Highways and Streets, 7th ed. 2018, section 4.7).
constexpr double kMountableFaceRun = 1.5;

std::string sideName(const Component& component)
{
    return std::string(toString(component.side)) + ":" + component.name;
}

bool finite(double value)
{
    return std::isfinite(value);
}

} // namespace

// ---- words ------------------------------------------------------------------------------

std::string_view toString(ComponentKind kind)
{
    for (const auto& [value, word] : kKinds) {
        if (value == kind) {
            return word;
        }
    }
    return "lane";
}

std::optional<ComponentKind> componentKindFrom(std::string_view text)
{
    for (const auto& [value, word] : kKinds) {
        if (katana::core::equalsIgnoringCase(word, text)) {
            return value;
        }
    }
    return std::nullopt;
}

std::string_view toString(KerbShape shape)
{
    for (const auto& [value, word] : kShapes) {
        if (value == shape) {
            return word;
        }
    }
    return "barrier";
}

std::optional<KerbShape> kerbShapeFrom(std::string_view text)
{
    for (const auto& [value, word] : kShapes) {
        if (katana::core::equalsIgnoringCase(word, text)) {
            return value;
        }
    }
    return std::nullopt;
}

std::string_view toString(KerbFacing facing)
{
    for (const auto& [value, word] : kFacings) {
        if (value == facing) {
            return word;
        }
    }
    return "inward";
}

std::optional<KerbFacing> kerbFacingFrom(std::string_view text)
{
    for (const auto& [value, word] : kFacings) {
        if (katana::core::equalsIgnoringCase(word, text)) {
            return value;
        }
    }
    return std::nullopt;
}

std::string_view toString(Side side)
{
    return side == Side::Left ? "left" : "right";
}

std::optional<Side> sideFrom(std::string_view text)
{
    if (katana::core::equalsIgnoringCase(text, "left")) {
        return Side::Left;
    }
    if (katana::core::equalsIgnoringCase(text, "right")) {
        return Side::Right;
    }
    return std::nullopt;
}

// ---- the template -------------------------------------------------------------------------

std::vector<const Component*> RoadTemplate::side(Side which) const
{
    std::vector<const Component*> out;
    for (const Component& component : components) {
        if (component.side == which) {
            out.push_back(&component);
        }
    }
    return out;
}

std::optional<std::size_t> RoadTemplate::find(Side which, std::string_view componentName) const
{
    for (std::size_t i = 0; i < components.size(); ++i) {
        if (components[i].side == which &&
            katana::core::equalsIgnoringCase(components[i].name, componentName)) {
            return i;
        }
    }
    return std::nullopt;
}

const RoadTemplate* RoadTemplateSet::find(std::string_view name) const
{
    for (const RoadTemplate& each : templates) {
        if (katana::core::equalsIgnoringCase(each.name, name)) {
            return &each;
        }
    }
    return nullptr;
}

RoadTemplate* RoadTemplateSet::find(std::string_view name)
{
    for (RoadTemplate& each : templates) {
        if (katana::core::equalsIgnoringCase(each.name, name)) {
            return &each;
        }
    }
    return nullptr;
}

// ---- defaults -----------------------------------------------------------------------------

PavementCourse makeCourse(std::string name, double thickness, std::string material,
                          std::string ifcType)
{
    PavementCourse course;
    course.name = std::move(name);
    course.thickness = thickness;
    course.ifcType = std::move(ifcType);
    const std::string word = katana::core::lowered(material);
    course.material = std::move(material);
    // Colours a person reads a section by: dark asphalt over tan granular
    // courses, pale concrete, brown topsoil.
    if (word.find("asphalt") != std::string::npos || word.find("seal") != std::string::npos) {
        course.colour = rgb(60, 60, 60);
    } else if (word.find("concrete") != std::string::npos) {
        course.colour = rgb(200, 200, 190);
    } else if (word.find("rock") != std::string::npos || word.find("gravel") != std::string::npos ||
               word.find("granular") != std::string::npos) {
        course.colour = rgb(170, 145, 105);
    } else if (word.find("sand") != std::string::npos) {
        course.colour = rgb(215, 195, 150);
    } else if (word.find("topsoil") != std::string::npos ||
               word.find("soil") != std::string::npos) {
        course.colour = rgb(115, 90, 60);
    } else {
        course.colour = rgb(150, 150, 150);
    }
    return course;
}

// The numbers are a starting point a designer edits, each of the order the
// guide named gives (docs/road_design.md, "Defaults, and where they come
// from"): a 3.5 m lane at 3 % crossfall (Austroads, Guide to Road Design
// Part 3, the common rural two-lane cross-section), a path no steeper across
// than 1 in 40 (AS 1428.1, a continuous accessible path of travel), kerbs as
// the Green Book's curbs (AASHTO 2018, section 4.7: a vertical curb 150 to
// 200 mm high, a sloping one 100 mm or less, a gutter 0.3 to 1.8 m wide at 5
// to 8 %), a fill batter of 1V:4H, the slope the Roadside Design Guide calls
// recoverable (AASHTO 2011, chapter 3), and a 1V:2H cut. The courses are a
// typical flexible pavement's, not a design.
Component defaultComponent(ComponentKind kind, Side side)
{
    Component component;
    component.kind = kind;
    component.side = side;
    component.name = std::string(toString(kind));
    switch (kind) {
    case ComponentKind::Lane:
        component.width = 3.5;
        component.crossfall = -3.0;
        component.superelevated = true;
        component.layer = "road/pavement";
        component.colour = rgb(80, 80, 80);
        component.courses = {makeCourse("AC14", 0.050, "asphalt"),
                             makeCourse("base", 0.200, "crushed rock"),
                             makeCourse("subbase", 0.250, "crushed rock")};
        break;
    case ComponentKind::Shoulder:
        component.width = 1.5;
        component.crossfall = -3.0;
        component.superelevated = true;
        component.layer = "road/shoulder";
        component.colour = rgb(110, 110, 110);
        component.courses = {makeCourse("seal", 0.020, "sprayed seal"),
                             makeCourse("base", 0.200, "crushed rock")};
        break;
    case ComponentKind::Kerb:
        component.width = 0.150;
        component.crossfall = 0.0;
        component.layer = "road/kerb";
        component.colour = rgb(200, 200, 190);
        component.material = "concrete";
        component.shape = KerbShape::Barrier;
        component.height = 0.150;
        component.depth = 0.150;
        break;
    case ComponentKind::Footpath:
        component.width = 1.5;
        component.crossfall = 2.5;
        component.layer = "road/footpath";
        component.colour = rgb(180, 180, 170);
        component.courses = {makeCourse("concrete", 0.100, "concrete"),
                             makeCourse("bedding", 0.050, "sand")};
        break;
    case ComponentKind::Verge:
        component.width = 2.0;
        component.crossfall = 2.5;
        component.layer = "road/verge";
        component.colour = rgb(90, 140, 70);
        component.courses = {makeCourse("topsoil", 0.100, "topsoil", "NOTDEFINED")};
        break;
    case ComponentKind::Median:
        component.width = 2.0;
        component.crossfall = 0.0;
        component.layer = "road/median";
        component.colour = rgb(90, 140, 70);
        component.courses = {makeCourse("topsoil", 0.100, "topsoil", "NOTDEFINED")};
        break;
    case ComponentKind::Drain:
        component.width = 2.0;
        component.crossfall = 0.0;
        component.depth = 0.5;
        component.layer = "road/drain";
        component.colour = rgb(140, 115, 80);
        break;
    case ComponentKind::Batter:
        component.width = 0.0;
        component.crossfall = 0.0;
        component.layer = "road/batter";
        component.colour = rgb(160, 140, 100);
        component.cutSlope = 2.0;
        component.fillSlope = 4.0;
        component.maximumWidth = 50.0;
        break;
    }
    return component;
}

namespace {

// Both sides, mirror images: `left` lists the left side's components.
RoadTemplate mirrored(std::string name, std::string description, std::vector<Component> left)
{
    RoadTemplate made;
    made.name = std::move(name);
    made.description = std::move(description);
    for (const Component& component : left) {
        made.components.push_back(component);
    }
    for (Component component : left) {
        component.side = Side::Right;
        made.components.push_back(std::move(component));
    }
    return made;
}

Component named(Component component, std::string name)
{
    component.name = std::move(name);
    return component;
}

} // namespace

std::vector<RoadTemplate> standardTemplates()
{
    std::vector<RoadTemplate> made;
    {
        Component shoulder = defaultComponent(ComponentKind::Shoulder, Side::Left);
        shoulder.width = 2.0;
        made.push_back(
            mirrored("rural two-lane",
                     "Two 3.5 m lanes at 3 %, 2 m sealed shoulders, table drains and batters",
                     {defaultComponent(ComponentKind::Lane, Side::Left), shoulder,
                      defaultComponent(ComponentKind::Drain, Side::Left),
                      defaultComponent(ComponentKind::Batter, Side::Left)}));
    }
    {
        Component kerb = defaultComponent(ComponentKind::Kerb, Side::Left);
        kerb.shape = KerbShape::KerbAndGutter;
        Component lane = defaultComponent(ComponentKind::Lane, Side::Left);
        lane.width = 3.3;
        Component parking = named(defaultComponent(ComponentKind::Lane, Side::Left), "parking");
        parking.width = 2.3;
        Component verge = defaultComponent(ComponentKind::Verge, Side::Left);
        verge.width = 1.0;
        made.push_back(mirrored(
            "urban street",
            "A 3.3 m lane and 2.3 m parking each way, kerb and gutter, 1.5 m footpaths and "
            "verges",
            {lane, parking, kerb, defaultComponent(ComponentKind::Footpath, Side::Left), verge,
             defaultComponent(ComponentKind::Batter, Side::Left)}));
    }
    {
        // A raised median: the kerb is highest against the median, whose top
        // is the profile, and falls to its lip at the carriageway, so the lanes
        // start 100 mm under the profile and the face looks out to the traffic.
        Component kerb = defaultComponent(ComponentKind::Kerb, Side::Left);
        kerb.shape = KerbShape::Mountable;
        kerb.height = 0.100;
        kerb.faces = KerbFacing::Outward;
        Component inner = named(defaultComponent(ComponentKind::Lane, Side::Left), "lane1");
        Component outer = named(defaultComponent(ComponentKind::Lane, Side::Left), "lane2");
        Component shoulder = defaultComponent(ComponentKind::Shoulder, Side::Left);
        shoulder.width = 2.5;
        made.push_back(mirrored(
            "divided road",
            "A 4 m median (the profile is its top) with mountable kerbs, two 3.5 m lanes each way, "
            "2.5 m shoulders and batters",
            {defaultComponent(ComponentKind::Median, Side::Left), kerb, inner, outer, shoulder,
             defaultComponent(ComponentKind::Batter, Side::Left)}));
    }
    return made;
}

RoadTemplate templateFromAssembly(const cad::Assembly& assembly)
{
    Component lane = defaultComponent(ComponentKind::Lane, Side::Left);
    lane.name = "carriageway";
    lane.width = assembly.halfWidth;
    // The assembly's crossfall is a ratio falling AWAY from the centreline;
    // a template's is outward percent, rising positive.
    lane.crossfall = -assembly.crossfall * 100.0;
    lane.superelevated = false;
    lane.courses.clear();
    Component batter = defaultComponent(ComponentKind::Batter, Side::Left);
    batter.cutSlope = assembly.cutBatter;
    batter.fillSlope = assembly.fillBatter;
    batter.maximumWidth = assembly.maximumBatterWidth;
    return mirrored("assembly", "The one-carriageway assembly of cad::Assembly", {lane, batter});
}

// ---- checks ---------------------------------------------------------------------------

Status checkName(std::string_view what, std::string_view name)
{
    if (katana::core::trimmed(name).empty()) {
        return makeError(ErrorCode::InvalidArgument, std::string(what) + " needs a name");
    }
    for (const char c : name) {
        if (c == '/' || c == '"' || static_cast<unsigned char>(c) < 0x20) {
            return makeError(ErrorCode::InvalidArgument,
                             std::string(what) +
                                 " names may not hold a '/', a '\"' or a line break",
                             std::string(name));
        }
    }
    return {};
}

Status checkWord(std::string_view what, std::string_view name)
{
    if (name.empty()) {
        return makeError(ErrorCode::InvalidArgument, std::string(what) + " needs a name");
    }
    for (const char c : name) {
        const bool ok = (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
                        (c >= '0' && c <= '9') || c == '-' || c == '_' || c == '.';
        if (!ok) {
            return makeError(ErrorCode::InvalidArgument,
                             std::string(what) +
                                 " names are one word of letters, digits, '-', '_' and '.'",
                             std::string(name));
        }
    }
    return {};
}

Status checkTemplate(const RoadTemplate& roadTemplate)
{
    if (auto status = checkName("a road template", roadTemplate.name); !status) {
        return status;
    }
    const auto refuse = [&roadTemplate](const Component& component, const std::string& what) {
        return makeError(ErrorCode::InvalidArgument, "template " + roadTemplate.name + ", " +
                                                         sideName(component) + ": " + what);
    };
    const auto tooLong = [](const std::string& what) {
        return what + " is at most " + katana::core::formatExactReal(kMaximumComponentLength) +
               " m";
    };
    for (const Side side : {Side::Left, Side::Right}) {
        std::set<std::string> names;
        bool batterSeen = false;
        for (const Component* each : roadTemplate.side(side)) {
            const Component& component = *each;
            if (auto status = checkWord("a component", component.name); !status) {
                return refuse(component, status.error().message);
            }
            // The corridor's earthworks objects are corridor/<c>/<side>/earthworks/
            // cut|fill/<run>: a component of that name with a course called cut or
            // fill would make the same id, and the IFC export keeps the first
            // element of an id and skips the later one.
            if (katana::core::equalsIgnoringCase(component.name, "earthworks")) {
                return refuse(component,
                              "earthworks is the name of the corridor's cut and fill solids; a "
                              "component cannot take it");
            }
            if (!names.insert(katana::core::lowered(component.name)).second) {
                return refuse(component, "a second component of that name on this side");
            }
            if (batterSeen) {
                return refuse(component,
                              "nothing can be outside the batter, which runs to the ground");
            }
            if (!finite(component.width) || !finite(component.crossfall) ||
                !finite(component.height) || !finite(component.depth) ||
                !finite(component.gutterWidth) || !finite(component.gutterFall) ||
                !finite(component.cutSlope) || !finite(component.fillSlope) ||
                !finite(component.maximumWidth) || !finite(component.benchHeight) ||
                !finite(component.benchWidth)) {
                return refuse(component, "a value is not a finite number");
            }
            if (std::abs(component.crossfall) > 100.0) {
                return refuse(component, "a crossfall is a percent within -100 to 100");
            }
            switch (component.kind) {
            case ComponentKind::Batter:
                batterSeen = true;
                if (!(component.cutSlope > 0.0) || !(component.fillSlope > 0.0)) {
                    return refuse(component,
                                  "batter slopes must be positive (horizontal run per unit "
                                  "rise)");
                }
                if (!(component.maximumWidth > 0.0)) {
                    return refuse(component, "the maximum batter width must be positive");
                }
                if (component.maximumWidth > kMaximumComponentLength) {
                    return refuse(component, tooLong("the maximum batter width"));
                }
                if (component.benchHeight < 0.0 || component.benchWidth < 0.0 ||
                    ((component.benchHeight > 0.0) != (component.benchWidth > 0.0))) {
                    return refuse(component,
                                  "a bench needs both its height and its width, or neither");
                }
                if (component.benchHeight > 0.0) {
                    // The most benches the batter can make: one every period,
                    // the run being the flatter slope's (the most benches).
                    // An overflow to infinity is over the limit as well.
                    const double period =
                        component.benchHeight * std::min(component.cutSlope, component.fillSlope) +
                        component.benchWidth;
                    if (!(component.maximumWidth / period <=
                          static_cast<double>(kMaximumBenches))) {
                        return refuse(component,
                                      "a bench of " + katana::core::formatExactReal(
                                                          component.benchWidth) +
                                          " m every " +
                                          katana::core::formatExactReal(component.benchHeight) +
                                          " m of rise would make more than " +
                                          std::to_string(kMaximumBenches) +
                                          " benches across the batter's maximum width of " +
                                          katana::core::formatExactReal(component.maximumWidth) +
                                          " m: give a bench height and width nearer a real "
                                          "batter's");
                    }
                }
                break;
            case ComponentKind::Kerb:
                if (!(component.width > 0.0) || !(component.depth > 0.0)) {
                    return refuse(component, "a kerb's width and depth must be positive");
                }
                if (component.width > kMaximumComponentLength ||
                    component.height > kMaximumComponentLength ||
                    component.depth > kMaximumComponentLength ||
                    component.gutterWidth > kMaximumComponentLength) {
                    return refuse(component, tooLong("a kerb's width, height, depth and gutter"));
                }
                if (component.shape != KerbShape::EdgeStrip && !(component.height > 0.0)) {
                    return refuse(component, "a kerb's face height must be positive");
                }
                if (component.shape == KerbShape::EdgeStrip &&
                    component.faces != KerbFacing::Inward) {
                    return refuse(component,
                                  "an edge strip is flush: it has no face to turn outward");
                }
                if (component.superelevated && component.shape != KerbShape::EdgeStrip) {
                    return refuse(component, "a " + std::string(toString(component.shape)) +
                                                 " kerb has its own shape and no crossfall to "
                                                 "superelevate; only an edge strip has");
                }
                if (component.shape == KerbShape::KerbAndGutter &&
                    (!(component.gutterWidth > 0.0) || std::abs(component.gutterFall) > 100.0)) {
                    return refuse(component,
                                  "a gutter's width must be positive and its fall a percent "
                                  "within -100 to 100");
                }
                break;
            case ComponentKind::Drain:
                if (!(component.width > 0.0) || component.depth < 0.0) {
                    return refuse(component,
                                  "a drain's width must be positive and its depth not negative");
                }
                if (component.width > kMaximumComponentLength ||
                    component.depth > kMaximumComponentLength) {
                    return refuse(component, tooLong("a drain's width and depth"));
                }
                break;
            default:
                if (!(component.width > 0.0)) {
                    return refuse(component, "the width must be positive");
                }
                if (component.width > kMaximumComponentLength) {
                    return refuse(component, tooLong("the width"));
                }
                break;
            }
            const bool coursed =
                component.kind != ComponentKind::Kerb && component.kind != ComponentKind::Batter;
            if (!coursed && !component.courses.empty()) {
                return refuse(component, "a kerb or a batter has no courses");
            }
            std::set<std::string> courseNames;
            for (const PavementCourse& course : component.courses) {
                if (auto status = checkWord("a course", course.name); !status) {
                    return refuse(component, status.error().message);
                }
                if (!courseNames.insert(katana::core::lowered(course.name)).second) {
                    return refuse(component, "a second course called " + course.name);
                }
                if (!finite(course.thickness) || !(course.thickness > 0.0)) {
                    return refuse(component,
                                  "course " + course.name + ": the thickness must be positive");
                }
                if (course.thickness > kMaximumComponentLength) {
                    return refuse(component,
                                  "course " + course.name + ": " + tooLong("the thickness"));
                }
            }
        }
    }
    return {};
}

// ---- the keys ------------------------------------------------------------------------------

std::vector<std::string_view> componentKeys(ComponentKind kind)
{
    std::vector<std::string_view> keys{"name", "layer", "colour"};
    if (kind != ComponentKind::Batter) {
        keys.insert(keys.end(), {"width", "crossfall", "superelevated"});
    }
    switch (kind) {
    case ComponentKind::Kerb:
        keys.insert(keys.end(),
                    {"shape", "faces", "height", "depth", "gutter", "gutterfall", "material"});
        break;
    case ComponentKind::Drain:
        keys.push_back("depth");
        break;
    case ComponentKind::Batter:
        keys.insert(keys.end(), {"cut", "fill", "maxwidth", "benchheight", "benchwidth"});
        break;
    default:
        break;
    }
    return keys;
}

// ---- the kerb's shape --------------------------------------------------------------------

namespace {

// The kerb as seen from its other end: the columns in reverse, `out` measured
// from the far end and every level from the far end's top, so a kerb that rose
// going outward falls going outward, from its back to its lip.
std::vector<ShapeColumn> reversedKerb(const std::vector<ShapeColumn>& columns)
{
    std::vector<ShapeColumn> out;
    out.reserve(columns.size());
    const double width = columns.back().out;
    const double top = columns.back().top;
    for (auto it = columns.rbegin(); it != columns.rend(); ++it) {
        out.push_back(ShapeColumn{width - it->out, it->top - top, it->bottom - top});
    }
    return out;
}

} // namespace

std::vector<ShapeColumn> kerbShape(const Component& kerb)
{
    const double base = -kerb.depth;
    std::vector<ShapeColumn> inward;
    switch (kerb.shape) {
    case KerbShape::Barrier: {
        const double face = kerb.height * kBarrierFaceBatter;
        inward = {ShapeColumn{0.0, 0.0, base}, ShapeColumn{face, kerb.height, base},
                  ShapeColumn{face + kerb.width, kerb.height, base}};
        break;
    }
    case KerbShape::Mountable: {
        const double face = kerb.height * kMountableFaceRun;
        inward = {ShapeColumn{0.0, 0.0, base}, ShapeColumn{face, kerb.height, base},
                  ShapeColumn{face + kerb.width, kerb.height, base}};
        break;
    }
    case KerbShape::KerbAndGutter: {
        // The channel falls outward to the face, where the water runs; its
        // base follows it down, so the gutter is `depth` thick throughout.
        const double invert = -kerb.gutterWidth * kerb.gutterFall / 100.0;
        const double face = kerb.height * kBarrierFaceBatter;
        const double bottom = invert - kerb.depth;
        inward = {ShapeColumn{0.0, 0.0, -kerb.depth}, ShapeColumn{kerb.gutterWidth, invert, bottom},
                  ShapeColumn{kerb.gutterWidth + face, invert + kerb.height, bottom},
                  ShapeColumn{kerb.gutterWidth + face + kerb.width, invert + kerb.height, bottom}};
        break;
    }
    case KerbShape::EdgeStrip: {
        // Flush, with no face to turn.
        const double rise = kerb.width * kerb.crossfall / 100.0;
        return {ShapeColumn{0.0, 0.0, -kerb.depth},
                ShapeColumn{kerb.width, rise, rise - kerb.depth}};
    }
    }
    if (inward.empty()) {
        return {};
    }
    return kerb.faces == KerbFacing::Outward ? reversedKerb(inward) : inward;
}

double kerbWidth(const Component& kerb)
{
    const auto shape = kerbShape(kerb);
    return shape.empty() ? 0.0 : shape.back().out;
}

// ---- JSON -----------------------------------------------------------------------------------

namespace {

struct BadValue {
    std::string what;
};

Json colourJson(const Color& colour)
{
    return colour.toHex();
}

Color colourFrom(const Json& value)
{
    auto parsed = Color::fromHex(value.get<std::string>());
    if (!parsed) {
        throw BadValue{"a colour is \"#RRGGBB\": " + value.get<std::string>()};
    }
    return *parsed;
}

Json courseJson(const PavementCourse& course)
{
    return Json{{"name", course.name},
                {"thickness", course.thickness},
                {"material", course.material},
                {"ifc", course.ifcType},
                {"colour", colourJson(course.colour)}};
}

PavementCourse courseFrom(const Json& value)
{
    PavementCourse course;
    course.name = value.at("name").get<std::string>();
    course.thickness = value.at("thickness").get<double>();
    course.material = value.value("material", std::string{});
    course.ifcType = value.value("ifc", std::string("PAVEMENT"));
    if (value.contains("colour")) {
        course.colour = colourFrom(value.at("colour"));
    }
    return course;
}

// Only what the kind uses is written: a lane says nothing of batters.
Json componentJson(const Component& component)
{
    Json out{{"name", component.name},
             {"kind", std::string(toString(component.kind))},
             {"side", std::string(toString(component.side))},
             {"layer", component.layer},
             {"colour", colourJson(component.colour)}};
    if (component.kind != ComponentKind::Batter) {
        out["width"] = component.width;
        out["crossfall"] = component.crossfall;
        out["superelevated"] = component.superelevated;
    }
    if (component.kind == ComponentKind::Kerb) {
        out["shape"] = std::string(toString(component.shape));
        out["faces"] = std::string(toString(component.faces));
        out["height"] = component.height;
        out["depth"] = component.depth;
        out["gutterWidth"] = component.gutterWidth;
        out["gutterFall"] = component.gutterFall;
        out["material"] = component.material;
    }
    if (component.kind == ComponentKind::Drain) {
        out["depth"] = component.depth;
    }
    if (component.kind == ComponentKind::Batter) {
        out["cutSlope"] = component.cutSlope;
        out["fillSlope"] = component.fillSlope;
        out["maximumWidth"] = component.maximumWidth;
        out["benchHeight"] = component.benchHeight;
        out["benchWidth"] = component.benchWidth;
    }
    if (!component.courses.empty()) {
        Json courses = Json::array();
        for (const PavementCourse& course : component.courses) {
            courses.push_back(courseJson(course));
        }
        out["courses"] = std::move(courses);
    }
    return out;
}

Component componentFrom(const Json& value)
{
    const std::string kindText = value.at("kind").get<std::string>();
    const auto kind = componentKindFrom(kindText);
    if (!kind) {
        throw BadValue{"unknown component kind \"" + kindText + "\""};
    }
    const std::string sideText = value.at("side").get<std::string>();
    const auto side = sideFrom(sideText);
    if (!side) {
        throw BadValue{"a side is left or right: \"" + sideText + "\""};
    }
    // The defaults first, so a field a kind does not write keeps its value.
    Component component = defaultComponent(*kind, *side);
    component.courses.clear();
    component.name = value.at("name").get<std::string>();
    component.layer = value.value("layer", component.layer);
    if (value.contains("colour")) {
        component.colour = colourFrom(value.at("colour"));
    }
    component.width = value.value("width", component.width);
    component.crossfall = value.value("crossfall", component.crossfall);
    component.superelevated = value.value("superelevated", component.superelevated);
    if (value.contains("shape")) {
        const std::string shapeText = value.at("shape").get<std::string>();
        const auto shape = kerbShapeFrom(shapeText);
        if (!shape) {
            throw BadValue{"unknown kerb shape \"" + shapeText + "\""};
        }
        component.shape = *shape;
    }
    if (value.contains("faces")) {
        const std::string facesText = value.at("faces").get<std::string>();
        const auto facing = kerbFacingFrom(facesText);
        if (!facing) {
            throw BadValue{"a kerb faces inward or outward: \"" + facesText + "\""};
        }
        component.faces = *facing;
    }
    component.height = value.value("height", component.height);
    component.depth = value.value("depth", component.depth);
    component.gutterWidth = value.value("gutterWidth", component.gutterWidth);
    component.gutterFall = value.value("gutterFall", component.gutterFall);
    component.material = value.value("material", component.material);
    component.cutSlope = value.value("cutSlope", component.cutSlope);
    component.fillSlope = value.value("fillSlope", component.fillSlope);
    component.maximumWidth = value.value("maximumWidth", component.maximumWidth);
    component.benchHeight = value.value("benchHeight", component.benchHeight);
    component.benchWidth = value.value("benchWidth", component.benchWidth);
    if (value.contains("courses")) {
        for (const Json& course : value.at("courses")) {
            component.courses.push_back(courseFrom(course));
        }
    }
    return component;
}

} // namespace

Result<std::string> roadTemplatesToJson(const RoadTemplateSet& set)
{
    Json templates = Json::array();
    for (const RoadTemplate& roadTemplate : set.templates) {
        Json components = Json::array();
        for (const Component& component : roadTemplate.components) {
            components.push_back(componentJson(component));
        }
        templates.push_back(Json{{"name", roadTemplate.name},
                                 {"description", roadTemplate.description},
                                 {"components", std::move(components)}});
    }
    const Json root{{"format", kFormat},
                    {"version", kRoadTemplatesVersion},
                    {"templates", std::move(templates)}};
    try {
        return root.dump();
    } catch (const std::exception& error) {
        // dump() throws on text that is not UTF-8, which a name could be.
        return makeError(ErrorCode::InvalidArgument, "the road templates cannot be written as JSON",
                         error.what());
    }
}

Result<RoadTemplateSet> roadTemplatesFromJson(std::string_view text)
{
    RoadTemplateSet set;
    try {
        const Json root = Json::parse(text);
        if (!root.is_object() || root.value("format", std::string{}) != kFormat) {
            return makeError(ErrorCode::ParseFailure, "this is not a set of road templates");
        }
        const int version = root.at("version").get<int>();
        if (version > kRoadTemplatesVersion) {
            return makeError(ErrorCode::Unsupported,
                             "the road templates were written by a newer version of Katana",
                             "version " + std::to_string(version));
        }
        for (const Json& value : root.at("templates")) {
            RoadTemplate roadTemplate;
            roadTemplate.name = value.at("name").get<std::string>();
            roadTemplate.description = value.value("description", std::string{});
            for (const Json& component : value.at("components")) {
                roadTemplate.components.push_back(componentFrom(component));
            }
            set.templates.push_back(std::move(roadTemplate));
        }
    } catch (const BadValue& bad) {
        return makeError(ErrorCode::ParseFailure, "the road templates cannot be read", bad.what);
    } catch (const std::exception& error) {
        return makeError(ErrorCode::ParseFailure, "the road templates cannot be read",
                         error.what());
    }
    std::set<std::string> names;
    for (const RoadTemplate& roadTemplate : set.templates) {
        if (auto status = checkTemplate(roadTemplate); !status) {
            return status.error();
        }
        if (!names.insert(katana::core::lowered(roadTemplate.name)).second) {
            return makeError(ErrorCode::ParseFailure, "two road templates have one name",
                             roadTemplate.name);
        }
    }
    return set;
}

} // namespace katana::cad::road
