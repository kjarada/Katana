// Road objects: kinds, parameters, and how each is made
// (include/katana/cad/road/road_objects.hpp, docs/road_objects.md).

#include "katana/cad/road/road_objects.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <format>
#include <iterator>
#include <numbers>
#include <utility>

#include "katana/cad/document.hpp"
#include "katana/cad/road/road_object_mesh.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/colour_names.hpp"
#include "katana/entity/layer_path.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/profile.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::cad::road {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::geometry::Point2;
using katana::geometry::Point3;
using katana::geometry::Polyline2;
using katana::geometry::TriangleMesh;
using katana::geometry::Vec2;
using katana::geometry::Vec3;

namespace {

// ---- what the kinds are made of: published dimensions, each named ---------------------------

// The W-beam and thrie-beam rails of AASHTO M 180: a W-beam 311 mm (12 1/4 in)
// and a thrie-beam 508 mm (20 in) from edge to edge, both 83 mm (3 1/4 in)
// deep. Drawn as a plain box of that size, the corrugation left out.
constexpr double kWBeamRailHeight = 0.311;
constexpr double kThrieBeamRailHeight = 0.508;
constexpr double kBeamDepth = 0.083;
// The steel post a beam barrier stands on: a W6x9 (W150x13.5) section, 150 mm
// deep across the barrier and 100 mm wide along it (AISC shape tables, rounded).
constexpr double kPostDepth = 0.150;
constexpr double kPostFlange = 0.100;
// The F-shape concrete barrier (AASHTO Roadside Design Guide, FHWA): a 76 mm
// (3 in) vertical reveal at the foot, the face at 55 degrees from the
// horizontal up to 254 mm (10 in), then at 84 degrees to the top.
constexpr double kFShapeReveal = 0.076;
constexpr double kFShapeBreak = 0.254;
constexpr double kFShapeLowerDegrees = 55.0;
constexpr double kFShapeUpperDegrees = 84.0;
// Wire rope: 19 mm ropes, 100 mm apart below the top one, on 100 mm posts
// standing 50 mm above it - typical of the proprietary systems, whose own
// drawings govern.
constexpr double kRopeDiameter = 0.019;
constexpr double kRopeSpacing = 0.10;
constexpr double kRopePost = 0.10;
constexpr double kRopePostAbove = 0.05;
// A fence's wires are 2.5 to 4 mm; drawn 20 mm so they show at the distance
// a road is looked at from. Their volume is not a quantity.
constexpr double kFenceStrand = 0.02;
// A light pole's arm, 80 mm square, and its luminaire, 600 x 300 x 150 mm:
// typical of a road lantern.
constexpr double kArmSection = 0.08;
constexpr double kLuminaireLength = 0.6;
constexpr double kLuminaireWidth = 0.3;
constexpr double kLuminaireHeight = 0.15;
// A culvert's headwall: 250 mm thick, standing 300 mm beyond the barrels at
// each side and above them - typical of a precast headwall.
constexpr double kHeadwallThickness = 0.25;
constexpr double kHeadwallMargin = 0.3;
// A pipe drawn with 24 facets: a facet of a 900 mm pipe is at most
// 0.45 x (1 - cos 7.5 deg) = 3.9 mm inside the true circle.
constexpr int kPipeSides = 24;
// A pole or a round sign with 8 and 24 sides.
constexpr int kPoleSides = 8;
constexpr int kRoundSignSides = 24;

// An alignment's objects are sampled every metre between its key stations,
// so a section swept along a curve of radius 50 m is within
// 1^2 / (8 x 50) = 2.5 mm of the true offset curve; a line draped on a
// surface is sampled as often, so it follows the ground.
constexpr double kPathStep = 1.0;
// A station this close to an end of the alignment is that end: a station
// typed as the end the reply gave must not fall off it by a rounding.
constexpr double kStationTolerance = 1e-6;

// ---- the parameter tables -----------------------------------------------------------------

constexpr std::string_view kBarrierTypes[] = {"wbeam", "thriebeam", "fshape", "singleslope",
                                              "wirerope"};
constexpr std::string_view kSides[] = {"auto", "left", "right"};
constexpr std::string_view kSignShapes[] = {"rectangle", "diamond", "circle", "octagon",
                                            "triangle"};
constexpr std::string_view kCulvertShapes[] = {"circular", "box"};
constexpr std::string_view kOnOff[] = {"on", "off"};
constexpr std::string_view kWallTypes[] = {"retaining", "noise"};
constexpr std::string_view kMarkingTypes[] = {"edge", "centre", "lane", "continuity", "solid"};

// A facing, an invert: a level anywhere a project could be. Ten kilometres
// either way of the datum holds every road on Earth.
constexpr double kAnyLevel = 10000.0;

using P = ParameterSpec;
using T = ParameterType;

const P kBarrierParameters[] = {
    {"type", T::Choice, "", "wbeam", kBarrierTypes, 0, 0,
     "wbeam, thriebeam (steel beams on posts), fshape, singleslope (concrete), wirerope"},
    {"height", T::Number, "m", "by type", {}, 0.3, 2.0, "top of the barrier above the ground"},
    {"postspacing", T::Number, "m", "by type", {}, 0.5, 10.0,
     "between the posts of a beam or wire rope barrier"},
    {"width", T::Number, "m", "by type", {}, 0.2, 1.5, "base width of a concrete barrier"},
    {"topwidth", T::Number, "m", "by type", {}, 0.05, 1.0, "top width of a single slope barrier"},
    {"ropes", T::Number, "", "4", {}, 2, 6, "wire ropes", true},
    {"side", T::Choice, "", "auto", kSides, 0, 0,
     "the side of its line the traffic is on, looking along it; auto: towards the centreline"},
};
const P kGuidePostParameters[] = {
    {"height", T::Number, "m", "1.0", {}, 0.3, 3.0, "above the ground"},
    {"size", T::Number, "m", "0.1", {}, 0.02, 0.5, "the post's square section"},
};
const P kSignParameters[] = {
    {"shape", T::Choice, "", "rectangle", kSignShapes, 0, 0, "the plate's outline"},
    {"width", T::Number, "m", "0.9", {}, 0.1, 10.0,
     "the plate's width; a diamond's diagonal, a circle's or an octagon's across"},
    {"plateheight", T::Number, "m", "0.6", {}, 0.1, 10.0,
     "a rectangle's height; the other shapes are as tall as their outline"},
    {"clearance", T::Number, "m", "1.5", {}, 0.0, 10.0,
     "from the ground to the plate's lower edge"},
    {"posts", T::Number, "", "1", {}, 1, 2, "one post, or two", true},
    {"postsize", T::Number, "m", "0.075", {}, 0.02, 0.5, "a post's square section"},
    {"thickness", T::Number, "m", "0.003", {}, 0.001, 0.1, "the plate's"},
    {"facing", T::Number, "deg", "auto", {}, -360.0, 360.0,
     "the way the face looks, counter-clockwise from east; auto: towards the traffic"},
    {"code", T::Text, "", "", {}, 0, 0, "the sign's code in its standard"},
    {"text", T::Text, "", "", {}, 0, 0, "its legend"},
};
const P kLightParameters[] = {
    {"height", T::Number, "m", "10", {}, 2.0, 40.0, "mounting height above the ground"},
    {"outreach", T::Number, "m", "1.5", {}, 0.0, 10.0, "the arm, from the pole to the lantern"},
    {"polesize", T::Number, "m", "0.2", {}, 0.05, 1.0, "the pole across its flats"},
    {"facing", T::Number, "deg", "auto", {}, -360.0, 360.0,
     "the way the arm points, counter-clockwise from east; auto: towards the centreline"},
};
const P kCulvertParameters[] = {
    {"shape", T::Choice, "", "circular", kCulvertShapes, 0, 0, "a pipe, or a box"},
    {"cells", T::Number, "", "1", {}, 1, 20, "barrels side by side", true},
    {"diameter", T::Number, "m", "0.9", {}, 0.1, 5.0, "a pipe's inside diameter"},
    {"span", T::Number, "m", "1.2", {}, 0.3, 10.0, "a box's inside width"},
    {"rise", T::Number, "m", "0.9", {}, 0.3, 10.0, "a box's inside height"},
    {"wall", T::Number, "m", "by type", {}, 0.02, 1.0, "the barrel's wall"},
    {"cellgap", T::Number, "m", "by type", {}, 0.0, 5.0, "between the barrels"},
    {"length", T::Number, "m", "20", {}, 1.0, 500.0,
     "across an alignment, centred on the station; along a line or between points, the "
     "line's"},
    {"skew", T::Number, "deg", "0", {}, -80.0, 80.0,
     "from square to the alignment, counter-clockwise"},
    {"invert", T::Number, "m", "auto", {}, -kAnyLevel, kAnyLevel,
     "the inlet's invert level; auto: from the cover"},
    {"invert2", T::Number, "m", "auto", {}, -kAnyLevel, kAnyLevel,
     "the outlet's; auto: the inlet's when invert is given, else from the cover"},
    {"cover", T::Number, "m", "0.6", {}, 0.0, 50.0, "from the ground to the barrel's top"},
    {"headwalls", T::Choice, "", "on", kOnOff, 0, 0, "a headwall at each end"},
};
const P kWallParameters[] = {
    {"type", T::Choice, "", "retaining", kWallTypes, 0, 0, "retaining, or noise"},
    {"height", T::Number, "m", "by type", {}, 0.2, 20.0, "above the ground"},
    {"thickness", T::Number, "m", "by type", {}, 0.05, 3.0, "the stem's"},
    {"footing", T::Number, "m", "by type", {}, 0.0, 10.0, "the strip footing's width; 0: none"},
    {"footingdepth", T::Number, "m", "by type", {}, 0.1, 5.0, "below the ground"},
};
const P kFenceParameters[] = {
    {"height", T::Number, "m", "1.2", {}, 0.3, 5.0, "above the ground"},
    {"postspacing", T::Number, "m", "3", {}, 0.5, 20.0, "between the posts"},
    {"postsize", T::Number, "m", "0.1", {}, 0.02, 0.5, "a post's square section"},
    {"rails", T::Number, "", "3", {}, 1, 10, "wires or rails", true},
};
const P kMarkingParameters[] = {
    {"type", T::Choice, "", "edge", kMarkingTypes, 0, 0,
     "edge, centre (dividing), lane, continuity, solid"},
    {"width", T::Number, "m", "by type", {}, 0.05, 1.0, "the line's"},
    {"dash", T::Number, "m", "by type", {}, 0.0, 50.0, "a mark's length; 0: continuous"},
    {"gap", T::Number, "m", "by type", {}, 0.0, 50.0, "between marks"},
    {"thickness", T::Number, "m", "0.003", {}, 0.0005, 0.05, "the material's"},
};

// The defaults that follow a type= (ParameterSpec::defaultText "by type").
struct TypeDefault {
    RoadObjectKind kind;
    std::string_view type;
    std::string_view key;
    double value;
};
constexpr TypeDefault kTypeDefaults[] = {
    // Tops of rail: 787 mm (31 in) for the MASH-tested Midwest Guardrail
    // System's W-beam; 864 mm (34 in) for thrie-beam. Posts at 1.905 m
    // (6 ft 3 in), the standard beam barrier spacing.
    {RoadObjectKind::Barrier, "wbeam", "height", 0.787},
    {RoadObjectKind::Barrier, "wbeam", "postspacing", 1.905},
    {RoadObjectKind::Barrier, "thriebeam", "height", 0.864},
    {RoadObjectKind::Barrier, "thriebeam", "postspacing", 1.905},
    // The F-shape: 813 mm (32 in) on a 610 mm (24 in) base.
    {RoadObjectKind::Barrier, "fshape", "height", 0.813},
    {RoadObjectKind::Barrier, "fshape", "width", 0.610},
    // The single slope: 1067 mm (42 in), a 610 mm base and a 203 mm (8 in)
    // top - its faces 10.8 degrees from the vertical.
    {RoadObjectKind::Barrier, "singleslope", "height", 1.067},
    {RoadObjectKind::Barrier, "singleslope", "width", 0.610},
    {RoadObjectKind::Barrier, "singleslope", "topwidth", 0.203},
    {RoadObjectKind::Barrier, "wirerope", "height", 0.80},
    {RoadObjectKind::Barrier, "wirerope", "postspacing", 3.0},
    {RoadObjectKind::Barrier, "wirerope", "ropes", 4.0},
    // A pipe's wall: 100 mm, a typical reinforced concrete pipe of 900 mm;
    // a box's 150 mm. Pipes 300 mm apart; boxes side by side.
    {RoadObjectKind::Culvert, "circular", "wall", 0.10},
    {RoadObjectKind::Culvert, "circular", "cellgap", 0.3},
    {RoadObjectKind::Culvert, "box", "wall", 0.15},
    {RoadObjectKind::Culvert, "box", "cellgap", 0.0},
    {RoadObjectKind::Wall, "retaining", "height", 2.0},
    {RoadObjectKind::Wall, "retaining", "thickness", 0.25},
    {RoadObjectKind::Wall, "retaining", "footing", 1.2},
    {RoadObjectKind::Wall, "retaining", "footingdepth", 0.4},
    {RoadObjectKind::Wall, "noise", "height", 4.0},
    {RoadObjectKind::Wall, "noise", "thickness", 0.2},
    {RoadObjectKind::Wall, "noise", "footing", 0.0},
    {RoadObjectKind::Wall, "noise", "footingdepth", 0.4},
    // AS 1742.2's longitudinal lines: an edge line 150 mm, a dividing or
    // lane line 100 mm broken 3 m line and 9 m gap, a continuity line
    // 1 m and 3 m.
    {RoadObjectKind::Marking, "edge", "width", 0.15},
    {RoadObjectKind::Marking, "edge", "dash", 0.0},
    {RoadObjectKind::Marking, "edge", "gap", 0.0},
    {RoadObjectKind::Marking, "centre", "width", 0.10},
    {RoadObjectKind::Marking, "centre", "dash", 3.0},
    {RoadObjectKind::Marking, "centre", "gap", 9.0},
    {RoadObjectKind::Marking, "lane", "width", 0.10},
    {RoadObjectKind::Marking, "lane", "dash", 3.0},
    {RoadObjectKind::Marking, "lane", "gap", 9.0},
    {RoadObjectKind::Marking, "continuity", "width", 0.15},
    {RoadObjectKind::Marking, "continuity", "dash", 1.0},
    {RoadObjectKind::Marking, "continuity", "gap", 3.0},
    {RoadObjectKind::Marking, "solid", "width", 0.10},
    {RoadObjectKind::Marking, "solid", "dash", 0.0},
    {RoadObjectKind::Marking, "solid", "gap", 0.0},
};

// The parameter that is the type, for the kinds that have one.
std::string_view typeKey(RoadObjectKind kind)
{
    switch (kind) {
    case RoadObjectKind::Barrier:
    case RoadObjectKind::Wall:
    case RoadObjectKind::Marking:
        return "type";
    case RoadObjectKind::Culvert:
    case RoadObjectKind::Sign:
        return "shape";
    case RoadObjectKind::GuidePost:
    case RoadObjectKind::LightPole:
    case RoadObjectKind::Fence:
        break;
    }
    return {};
}

std::string_view textOf(const Parameters& parameters, std::string_view key)
{
    const auto found = parameters.find(key);
    if (found == parameters.end()) {
        return {};
    }
    const auto* text = std::get_if<std::string>(&found->second);
    return text != nullptr ? std::string_view(*text) : std::string_view{};
}

// The value a parameter not given takes, for the object's type; nullopt for
// an "auto" one, a text, or a "by type" one this type has no use for.
std::optional<ParameterValue> defaultOf(RoadObjectKind kind, std::string_view type,
                                        const ParameterSpec& spec)
{
    if (spec.type == ParameterType::Choice) {
        return ParameterValue(std::string(spec.defaultText));
    }
    if (spec.type == ParameterType::Text) {
        return std::nullopt;
    }
    if (spec.defaultText == "by type" || spec.defaultText == "auto" || spec.defaultText.empty()) {
        for (const TypeDefault& row : kTypeDefaults) {
            if (row.kind == kind && row.type == type && row.key == spec.key) {
                return ParameterValue(row.value);
            }
        }
        return std::nullopt;
    }
    if (const auto value = katana::core::parseFiniteDouble(spec.defaultText)) {
        return ParameterValue(*value);
    }
    return std::nullopt;
}

// ---- formatting -----------------------------------------------------------------------------

std::string real(double value)
{
    return katana::core::formatExactReal(value);
}

std::string pointText(const Point3& point)
{
    return real(point.x) + "," + real(point.y) + "," + real(point.z);
}

double radians(double degrees)
{
    return degrees * std::numbers::pi / 180.0;
}

Vec2 unitAt(double radiansValue)
{
    return Vec2(std::cos(radiansValue), std::sin(radiansValue));
}

// ---- making: what cannot be made is thrown here and listed at the edge ------------------------

struct Unplaceable {
    std::string message;
};

[[noreturn]] void fail(std::string message)
{
    throw Unplaceable{std::move(message)};
}

// A parameter's value as made: given, else the default (effectiveParameters).
class Values {
  public:
    explicit Values(const RoadObject& object)
        : object_(object), values_(effectiveParameters(object))
    {
    }

    [[nodiscard]] double number(std::string_view key) const
    {
        const auto found = values_.find(key);
        if (found != values_.end()) {
            if (const auto* value = std::get_if<double>(&found->second)) {
                return *value;
            }
        }
        fail(std::format("{} has no value for {}", object_.id, key));
    }
    [[nodiscard]] std::optional<double> optionalNumber(std::string_view key) const
    {
        const auto found = values_.find(key);
        if (found == values_.end()) {
            return std::nullopt;
        }
        if (const auto* value = std::get_if<double>(&found->second)) {
            return *value;
        }
        return std::nullopt;
    }
    [[nodiscard]] std::string text(std::string_view key) const
    {
        return std::string(textOf(values_, key));
    }
    [[nodiscard]] const Parameters& all() const { return values_; }

  private:
    const RoadObject& object_;
    Parameters values_;
};

// A path in plan with a level at each point, and how far along each point is.
class Path {
  public:
    explicit Path(std::vector<PathPoint> points)
    {
        for (PathPoint& point : points) {
            if (!points_.empty() && points_.back().position.distanceTo(point.position) < 1e-6) {
                continue;
            }
            const double step =
                points_.empty() ? 0.0 : points_.back().position.distanceTo(point.position);
            along_.push_back(along_.empty() ? 0.0 : along_.back() + step);
            points_.push_back(point);
        }
    }

    [[nodiscard]] const std::vector<PathPoint>& points() const { return points_; }
    [[nodiscard]] double length() const { return along_.empty() ? 0.0 : along_.back(); }
    [[nodiscard]] std::size_t size() const { return points_.size(); }

    struct Sample {
        Point2 position;
        double z = 0.0;
        Vec2 direction;
    };
    // The point `distance` along, its level interpolated by length and the
    // direction of the segment it is on.
    [[nodiscard]] Sample at(double distance) const
    {
        const std::size_t segment = segmentAt(distance);
        const PathPoint& a = points_[segment];
        const PathPoint& b = points_[segment + 1];
        const double span = along_[segment + 1] - along_[segment];
        const double t = span > 0.0 ? std::clamp((distance - along_[segment]) / span, 0.0, 1.0)
                                    : 0.0;
        return Sample{a.position + (b.position - a.position) * t, a.z + (b.z - a.z) * t,
                      (b.position - a.position).normalized()};
    }
    // The part between `from` and `to`, its ends interpolated.
    [[nodiscard]] std::vector<PathPoint> between(double from, double to) const
    {
        std::vector<PathPoint> out;
        const Sample start = at(from);
        out.push_back({start.position, start.z});
        for (std::size_t i = 0; i < points_.size(); ++i) {
            if (along_[i] > from && along_[i] < to) {
                out.push_back(points_[i]);
            }
        }
        const Sample end = at(to);
        out.push_back({end.position, end.z});
        return out;
    }
    [[nodiscard]] Polyline2 plan() const
    {
        Polyline2 line;
        for (const PathPoint& point : points_) {
            line.vertices.push_back(point.position);
        }
        return line;
    }

  private:
    [[nodiscard]] std::size_t segmentAt(double distance) const
    {
        const auto upper = std::upper_bound(along_.begin(), along_.end(), distance);
        std::size_t index = upper == along_.begin()
                                ? 0
                                : static_cast<std::size_t>(upper - along_.begin()) - 1;
        return std::min(index, points_.size() - 2);
    }

    std::vector<PathPoint> points_;
    std::vector<double> along_;
};

// Distances from 0 to `length` every `spacing`, the end always among them:
// the posts of a barrier or a fence, the end post where the spacing does not
// come out even.
std::vector<double> postDistances(double length, double spacing)
{
    std::vector<double> out;
    const auto whole = static_cast<std::size_t>(std::floor(length / spacing + 1e-9));
    for (std::size_t k = 0; k <= whole; ++k) {
        out.push_back(std::min(static_cast<double>(k) * spacing, length));
    }
    if (length - out.back() > 1e-6) {
        out.push_back(length);
    }
    return out;
}

// Distances from 0 every `spacing` up to `length`, and no more: a run of
// guide posts or light poles is not given an extra one at its end, as a
// barrier is given an end post.
std::vector<double> everySpacing(double length, double spacing)
{
    std::vector<double> out;
    const auto whole = static_cast<std::size_t>(std::floor(length / spacing + 1e-9));
    for (std::size_t k = 0; k <= whole; ++k) {
        out.push_back(static_cast<double>(k) * spacing);
    }
    return out;
}

// The posts of a barrier or a fence along `path`: postDistances, less the
// end post of a path that comes back to its start, which would stand where
// the first one does.
std::vector<double> postsAlong(const Path& path, double spacing)
{
    std::vector<double> out = postDistances(path.length(), spacing);
    const std::vector<PathPoint>& points = path.points();
    if (out.size() > 1 && points.front().position.distanceTo(points.back().position) < 1e-6) {
        out.pop_back();
    }
    return out;
}

// One object stood on its ground: what a placement resolves to before any
// solid is made.
struct Spot {
    Point2 position;
    double z = 0.0;
    // Along the placement: the alignment's direction, the line's.
    Vec2 forward{1.0, 0.0};
    // The offset from the centreline, along an alignment.
    double offset = 0.0;
};

class Placement {
  public:
    Placement(const PlacementContext& context, const RoadObject& object)
        : object_(object), source_(heightSourceOf(object.placement))
    {
        const RoadObjectPlacement& placement = object.placement;
        if (placement.kind == PlacementKind::Alignment) {
            const katana::entity::Alignment* found =
                context.alignment ? context.alignment(placement.alignment) : nullptr;
            if (found == nullptr) {
                fail(std::format("alignment {} does not exist", placement.alignment));
            }
            auto solved = katana::geometry::solveAlignment(found->horizontal);
            if (!solved) {
                fail(std::format("alignment {} does not solve: {}", placement.alignment,
                                 solved.error().describe()));
            }
            alignment_ = std::move(*solved);
            if (source_ == HeightSource::Line) {
                fail("heights=line takes the heights of a line or points; along an alignment an "
                     "object stands on its profile or a surface");
            }
            if (source_ == HeightSource::Profile) {
                if (!found->vertical) {
                    fail(std::format("alignment {} has no design profile: give it one, or give "
                                     "surface=<name> to stand the object on a surface",
                                     placement.alignment));
                }
                auto profile = katana::geometry::solveProfile(*found->vertical);
                if (!profile) {
                    fail(std::format("the profile of {} does not solve: {}", placement.alignment,
                                     profile.error().describe()));
                }
                profile_ = std::move(*profile);
            }
        } else {
            if (source_ == HeightSource::Profile) {
                fail("heights=profile needs an alignment: give ALIGNMENT <name>, or the heights "
                     "of the line or a surface");
            }
            if (placement.vertices.empty()) {
                fail("the placement has no points");
            }
        }
        if (source_ == HeightSource::Surface) {
            if (placement.surface.empty()) {
                fail("heights=surface needs surface=<name>");
            }
            surface_ = context.surface ? context.surface(placement.surface) : nullptr;
            if (surface_ == nullptr) {
                fail(std::format("surface {} does not exist", placement.surface));
            }
        }
    }

    [[nodiscard]] const RoadObjectPlacement& placement() const { return object_.placement; }
    [[nodiscard]] HeightSource source() const { return source_; }
    [[nodiscard]] const katana::geometry::SolvedAlignment* alignment() const
    {
        return alignment_ ? &*alignment_ : nullptr;
    }

    // A station on the alignment, an end typed as the reply gave it kept on.
    [[nodiscard]] double checkedStation(double station) const
    {
        const auto& solved = *alignment_;
        if (station < solved.startStation() - kStationTolerance ||
            station > solved.endStation() + kStationTolerance) {
            fail(std::format("station {} is off alignment {} ({} to {})", real(station),
                             placement().alignment, real(solved.startStation()),
                             real(solved.endStation())));
        }
        return std::clamp(station, solved.startStation(), solved.endStation());
    }

    // The level at `position`, `station` and `offset` along the alignment.
    [[nodiscard]] double levelOnAlignment(double station, double offset,
                                          const Point2& position) const
    {
        if (source_ == HeightSource::Profile) {
            const auto level = profile_->elevationAt(station);
            if (!level) {
                fail(std::format("station {} is outside the design profile of {}", real(station),
                                 placement().alignment));
            }
            return *level + std::abs(offset) * placement().crossfall / 100.0 + placement().dz;
        }
        return surfaceLevel(position) + placement().dz;
    }

    [[nodiscard]] double surfaceLevel(const Point2& position) const
    {
        const auto level = surface_->elevationAt(position);
        if (!level) {
            fail(std::format("the point {},{} is off surface {}", real(position.x),
                             real(position.y), placement().surface));
        }
        return *level;
    }

    // A vertex's level: its own height, or the surface's under it.
    [[nodiscard]] double vertexLevel(std::size_t index) const
    {
        const PlacedVertex& vertex = placement().vertices[index];
        if (source_ == HeightSource::Surface) {
            return surfaceLevel(vertex.position) + placement().dz;
        }
        if (!vertex.z) {
            fail(std::format("point {} of the placement has no height: give surface=<name> to "
                             "stand it on a surface",
                             index + 1));
        }
        return *vertex.z + placement().dz;
    }

    [[nodiscard]] Spot alignmentSpot(double station) const
    {
        const double at = checkedStation(station);
        const auto point = alignment_->pointAtStationOffset(at, placement().offset);
        const auto direction = alignment_->directionAtStation(at);
        if (!point || !direction) {
            fail(std::format("station {} is off alignment {}", real(station),
                             placement().alignment));
        }
        return Spot{*point, levelOnAlignment(at, placement().offset, *point),
                    unitAt(*direction), placement().offset};
    }

    // The path a linear object runs along, with its ground levels.
    [[nodiscard]] Path linearPath() const
    {
        const RoadObjectPlacement& placement = this->placement();
        std::vector<PathPoint> points;
        if (placement.kind == PlacementKind::Alignment) {
            if (!placement.to) {
                fail(std::format("a {} along an alignment runs from station= to to=",
                                 kindWord(object_.kind)));
            }
            const double from = checkedStation(placement.station);
            const double to = checkedStation(*placement.to);
            if (!(to > from)) {
                fail(std::format("to= ({}) must be past station= ({})", real(*placement.to),
                                 real(placement.station)));
            }
            std::vector<double> stations;
            const auto steps = std::max<std::size_t>(
                1, static_cast<std::size_t>(std::ceil((to - from) / kPathStep)));
            for (std::size_t i = 0; i <= steps; ++i) {
                stations.push_back(from + (to - from) * static_cast<double>(i) /
                                              static_cast<double>(steps));
            }
            for (const double key : alignment_->keyStations()) {
                if (key > from && key < to) {
                    stations.push_back(key);
                }
            }
            std::ranges::sort(stations);
            const auto [first, last] = std::ranges::unique(
                stations, [](double a, double b) { return std::abs(a - b) < 1e-6; });
            stations.erase(first, last);
            for (const double station : stations) {
                const Spot spot = alignmentSpot(station);
                points.push_back({spot.position, spot.z});
            }
        } else {
            if (placement.vertices.size() < 2) {
                fail(std::format("a {} runs along two points at least", kindWord(object_.kind)));
            }
            for (std::size_t i = 0; i < placement.vertices.size(); ++i) {
                const PlacedVertex& vertex = placement.vertices[i];
                if (source_ == HeightSource::Surface && i > 0) {
                    // Draped: the ground between two vertices sampled too.
                    const Point2 start = placement.vertices[i - 1].position;
                    const double span = start.distanceTo(vertex.position);
                    const auto steps = static_cast<std::size_t>(std::ceil(span / kPathStep));
                    for (std::size_t s = 1; s < steps; ++s) {
                        const Point2 between = start + (vertex.position - start) *
                                                           (static_cast<double>(s) /
                                                            static_cast<double>(steps));
                        points.push_back({between, surfaceLevel(between) + placement.dz});
                    }
                }
                points.push_back({vertex.position, vertexLevel(i)});
            }
        }
        Path path(std::move(points));
        if (path.size() < 2 || path.length() < 1e-3) {
            fail(std::format("the {} has no length", kindWord(object_.kind)));
        }
        return path;
    }

    // Where the objects of a point kind stand.
    [[nodiscard]] std::vector<Spot> spots() const
    {
        const RoadObjectPlacement& placement = this->placement();
        std::vector<Spot> out;
        if (placement.spacing && !(*placement.spacing > 0.0)) {
            fail("spacing= must be more than 0");
        }
        if (placement.kind == PlacementKind::Alignment) {
            if (placement.to && !placement.spacing) {
                fail(std::format("to= needs spacing=: a {} every so many metres from station= "
                                 "to to=",
                                 kindWord(object_.kind)));
            }
            if (placement.spacing && !placement.to) {
                fail("spacing= needs to=: where the run ends");
            }
            if (!placement.to) {
                out.push_back(alignmentSpot(placement.station));
                return out;
            }
            if (!(*placement.to >= placement.station)) {
                fail(std::format("to= ({}) must be past station= ({})", real(*placement.to),
                                 real(placement.station)));
            }
            for (const double along :
                 everySpacing(*placement.to - placement.station, *placement.spacing)) {
                out.push_back(alignmentSpot(placement.station + along));
            }
            return out;
        }
        if (placement.kind == PlacementKind::Points || !placement.spacing) {
            const std::size_t count = placement.vertices.size();
            for (std::size_t i = 0; i < count; ++i) {
                Spot spot;
                spot.position = placement.vertices[i].position;
                spot.z = vertexLevel(i);
                if (placement.kind == PlacementKind::Points || count < 2) {
                    // At points nothing says which way is along: north, so a
                    // sign faces south, down a drawing.
                    spot.forward = Vec2(0.0, 1.0);
                } else {
                    // The segment it starts, or the last one at the end.
                    const std::size_t from = i + 1 < count ? i : i - 1;
                    spot.forward = (placement.vertices[from + 1].position -
                                    placement.vertices[from].position)
                                       .normalized();
                }
                out.push_back(spot);
            }
            return out;
        }
        // Along a line, every `spacing` from its start.
        const Path path = linearPath();
        for (const double along : everySpacing(path.length(), *placement.spacing)) {
            const Path::Sample sample = path.at(along);
            out.push_back(Spot{sample.position, sample.z, sample.direction, 0.0});
        }
        return out;
    }

  private:
    const RoadObject& object_;
    HeightSource source_;
    std::optional<katana::geometry::SolvedAlignment> alignment_;
    std::optional<katana::geometry::SolvedProfile> profile_;
    const katana::terrain::TinSurface* surface_ = nullptr;
};

// ---- the 3D objects -----------------------------------------------------------------------

struct Classification {
    std::string_view ifcClass;
    std::string_view predefinedType;
    std::string_view part;
};

bim::BimObject skeleton(const RoadObject& object, const Values& values, std::string suffix,
                        std::string nameSuffix, const Classification& ifc)
{
    bim::BimObject made;
    made.id = "object/" + object.id + suffix;
    made.name = displayName(object) + nameSuffix;
    made.kind = std::string(kindWord(object.kind));
    made.facility = object.facility;
    made.part = std::string(ifc.part);
    made.ifcClass = std::string(ifc.ifcClass);
    made.predefinedType = std::string(ifc.predefinedType);
    made.material = materialOf(object);
    made.colour = colourOf(object);
    made.layer = layerOf(object);
    const RoadObjectPlacement& placement = object.placement;
    made.properties.emplace_back("Object", object.id);
    made.properties.emplace_back("Kind", std::string(kindWord(object.kind)));
    made.properties.emplace_back("Placement", std::string(placementWord(placement.kind)));
    if (placement.kind == PlacementKind::Alignment) {
        made.properties.emplace_back("Alignment", placement.alignment);
        made.properties.emplace_back("Station", real(placement.station));
        if (placement.to) {
            made.properties.emplace_back("To", real(*placement.to));
        }
        made.properties.emplace_back("Offset", real(placement.offset));
    } else if (placement.source != katana::entity::kInvalidEntityId) {
        made.properties.emplace_back("Source", "#" + std::to_string(placement.source));
    }
    made.properties.emplace_back("Heights", std::string(heightWord(heightSourceOf(placement))));
    if (heightSourceOf(placement) == HeightSource::Surface) {
        made.properties.emplace_back("Surface", placement.surface);
    }
    for (const auto& [key, value] : values.all()) {
        made.properties.emplace_back(key, parameterText(value));
    }
    return made;
}

void addQuantity(bim::BimObject& object, std::string name, double value, std::string unit)
{
    object.quantities.push_back({std::move(name), value, std::move(unit)});
}

// Which way the traffic is, as the sign of u: +1 left of the line, -1 right.
double trafficSide(const RoadObject& object, const Values& values)
{
    const std::string side = values.text("side");
    if (side == "left") {
        return 1.0;
    }
    if (side == "right") {
        return -1.0;
    }
    // Along an alignment, the traffic is towards the centreline: on the
    // right of a barrier left of it, on the left of one right of it.
    if (object.placement.kind == PlacementKind::Alignment && object.placement.offset < 0.0) {
        return 1.0;
    }
    return -1.0;
}

std::vector<Point2> rectangleProfile(double u0, double u1, double v0, double v1)
{
    return {Point2(std::min(u0, u1), v0), Point2(std::max(u0, u1), v0),
            Point2(std::max(u0, u1), v1), Point2(std::min(u0, u1), v1)};
}

std::vector<bim::BimObject> makeBarrier(const RoadObject& object, const Placement& placement)
{
    const Values values(object);
    const Path path = placement.linearPath();
    const std::string type = values.text("type");
    const double height = values.number("height");
    const bool concrete = type == "fshape" || type == "singleslope";
    bim::BimObject made =
        skeleton(object, values, "", "", {"IfcRailing", "GUARDRAIL", "ROADSIDE"});
    std::size_t posts = 0;
    if (type == "wbeam" || type == "thriebeam") {
        const double rail = type == "wbeam" ? kWBeamRailHeight : kThrieBeamRailHeight;
        if (height < rail) {
            fail(std::format("height= ({}) is less than the rail ({} m)", real(height),
                             real(rail)));
        }
        for (const double along : postsAlong(path, values.number("postspacing"))) {
            const Path::Sample sample = path.at(along);
            addBox(made.mesh, Point3(sample.position.x, sample.position.y, sample.z),
                   sample.direction, kPostFlange, kPostDepth, height);
            ++posts;
        }
        // The rail on the traffic face of the posts.
        const double side = trafficSide(object, values);
        const auto profile = rectangleProfile(side * kPostDepth / 2.0,
                                              side * (kPostDepth / 2.0 + kBeamDepth),
                                              height - rail, height);
        addSweep(made.mesh, path.points(), profile);
    } else if (type == "wirerope") {
        const auto ropes = static_cast<std::size_t>(values.number("ropes"));
        const double lowest = height - kRopeSpacing * static_cast<double>(ropes - 1);
        if (lowest < kRopeDiameter) {
            fail(std::format("{} ropes {} m apart do not fit under a top rope at {} m", ropes,
                             real(kRopeSpacing), real(height)));
        }
        for (const double along : postsAlong(path, values.number("postspacing"))) {
            const Path::Sample sample = path.at(along);
            addBox(made.mesh, Point3(sample.position.x, sample.position.y, sample.z),
                   sample.direction, kRopePost, kRopePost, height + kRopePostAbove);
            ++posts;
        }
        for (std::size_t k = 0; k < ropes; ++k) {
            const double centre = height - kRopeSpacing * static_cast<double>(k);
            addSweep(made.mesh, path.points(),
                     rectangleProfile(-kRopeDiameter / 2.0, kRopeDiameter / 2.0,
                                      centre - kRopeDiameter / 2.0, centre + kRopeDiameter / 2.0));
        }
    } else {
        const double width = values.number("width");
        std::vector<Point2> profile;
        if (type == "fshape") {
            if (height <= kFShapeBreak) {
                fail(std::format("an F-shape barrier is taller than its {} m slope break",
                                 real(kFShapeBreak)));
            }
            const double base = width / 2.0;
            const double knee = base - (kFShapeBreak - kFShapeReveal) /
                                           std::tan(radians(kFShapeLowerDegrees));
            const double top =
                knee - (height - kFShapeBreak) / std::tan(radians(kFShapeUpperDegrees));
            if (top < 0.025) {
                fail(std::format("an F-shape {} m wide is too narrow for {} m: its faces meet",
                                 real(width), real(height)));
            }
            profile = {Point2(-base, 0.0),          Point2(base, 0.0),
                       Point2(base, kFShapeReveal), Point2(knee, kFShapeBreak),
                       Point2(top, height),         Point2(-top, height),
                       Point2(-knee, kFShapeBreak), Point2(-base, kFShapeReveal)};
        } else {
            const double top = values.number("topwidth");
            if (top > width) {
                fail("a single slope barrier's top is no wider than its base");
            }
            profile = {Point2(-width / 2.0, 0.0), Point2(width / 2.0, 0.0),
                       Point2(top / 2.0, height), Point2(-top / 2.0, height)};
        }
        addSweep(made.mesh, path.points(), profile);
    }
    made.plan.push_back(path.plan());
    addQuantity(made, "Length", path.length(), "m");
    addQuantity(made, "Height", height, "m");
    if (posts > 0) {
        addQuantity(made, "Count", static_cast<double>(posts), "");
    }
    if (concrete) {
        addQuantity(made, "GrossVolume", meshVolume(made.mesh), "m3");
    }
    return {std::move(made)};
}

std::vector<bim::BimObject> makeWall(const RoadObject& object, const Placement& placement)
{
    const Values values(object);
    const Path path = placement.linearPath();
    const bool retaining = values.text("type") == "retaining";
    const double height = values.number("height");
    const double thickness = values.number("thickness");
    const double footing = values.number("footing");
    const double depth = values.number("footingdepth");
    bim::BimObject made =
        skeleton(object, values, "", "",
                 {"IfcWall", retaining ? "RETAININGWALL" : "SOLIDWALL", "ROADSIDE"});
    std::vector<Point2> profile;
    if (footing > thickness) {
        // An inverted T: the footing below the ground, the stem above it.
        profile = {Point2(-footing / 2.0, -depth),   Point2(footing / 2.0, -depth),
                   Point2(footing / 2.0, 0.0),       Point2(thickness / 2.0, 0.0),
                   Point2(thickness / 2.0, height),  Point2(-thickness / 2.0, height),
                   Point2(-thickness / 2.0, 0.0),    Point2(-footing / 2.0, 0.0)};
    } else {
        profile = rectangleProfile(-thickness / 2.0, thickness / 2.0, 0.0, height);
    }
    addSweep(made.mesh, path.points(), profile);
    made.plan.push_back(path.plan());
    made.properties.emplace_back("WallType", retaining ? "retaining" : "noise");
    addQuantity(made, "Length", path.length(), "m");
    addQuantity(made, "Height", height, "m");
    addQuantity(made, "GrossArea", path.length() * height, "m2");
    addQuantity(made, "GrossVolume", meshVolume(made.mesh), "m3");
    return {std::move(made)};
}

std::vector<bim::BimObject> makeFence(const RoadObject& object, const Placement& placement)
{
    const Values values(object);
    const Path path = placement.linearPath();
    const double height = values.number("height");
    const double post = values.number("postsize");
    const auto rails = static_cast<std::size_t>(values.number("rails"));
    bim::BimObject made = skeleton(object, values, "", "", {"IfcRailing", "FENCE", "ROADSIDE"});
    std::size_t posts = 0;
    for (const double along : postsAlong(path, values.number("postspacing"))) {
        const Path::Sample sample = path.at(along);
        addBox(made.mesh, Point3(sample.position.x, sample.position.y, sample.z),
               sample.direction, post, post, height);
        ++posts;
    }
    // The wires evenly up the posts, the top one level with their tops.
    for (std::size_t k = 1; k <= rails; ++k) {
        const double top = height * static_cast<double>(k) / static_cast<double>(rails);
        addSweep(made.mesh, path.points(),
                 rectangleProfile(-kFenceStrand / 2.0, kFenceStrand / 2.0, top - kFenceStrand,
                                  top));
    }
    made.plan.push_back(path.plan());
    addQuantity(made, "Length", path.length(), "m");
    addQuantity(made, "Height", height, "m");
    addQuantity(made, "Count", static_cast<double>(posts), "");
    return {std::move(made)};
}

std::vector<bim::BimObject> makeMarking(const RoadObject& object, const Placement& placement)
{
    const Values values(object);
    const Path path = placement.linearPath();
    const double width = values.number("width");
    const double dash = values.number("dash");
    const double gap = values.number("gap");
    const double thickness = values.number("thickness");
    if (dash > 0.0 && !(gap > 0.0)) {
        fail("a broken line needs gap= as well as dash=");
    }
    bim::BimObject made =
        skeleton(object, values, "", "", {"IfcSurfaceFeature", "LINEMARKING", "CARRIAGEWAY"});
    const auto profile = rectangleProfile(-width / 2.0, width / 2.0, 0.0, thickness);
    double marked = 0.0;
    std::size_t marks = 0;
    const auto mark = [&](double from, double to) {
        const std::vector<PathPoint> piece = path.between(from, to);
        addSweep(made.mesh, piece, profile);
        Polyline2 line;
        for (const PathPoint& point : piece) {
            line.vertices.push_back(point.position);
        }
        made.plan.push_back(std::move(line));
        marked += to - from;
        ++marks;
    };
    if (dash <= 0.0) {
        mark(0.0, path.length());
    } else {
        // The pattern starts with a mark at the start of the line.
        for (double from = 0.0; from < path.length() - 1e-6; from += dash + gap) {
            mark(from, std::min(from + dash, path.length()));
        }
    }
    made.properties.emplace_back("MarkingType", values.text("type"));
    addQuantity(made, "Length", marked, "m");
    addQuantity(made, "GrossArea", marked * width, "m2");
    addQuantity(made, "Count", static_cast<double>(marks), "");
    return {std::move(made)};
}

std::vector<bim::BimObject> makeGuidePosts(const RoadObject& object, const Placement& placement)
{
    const Values values(object);
    const double height = values.number("height");
    const double size = values.number("size");
    std::vector<bim::BimObject> out;
    std::size_t n = 0;
    for (const Spot& spot : placement.spots()) {
        ++n;
        bim::BimObject made = skeleton(object, values, "/" + std::to_string(n),
                                       " #" + std::to_string(n), {"IfcSign", "MARKER", "ROADSIDE"});
        addBox(made.mesh, Point3(spot.position.x, spot.position.y, spot.z), spot.forward, size,
               size, height);
        const Vec2 a = spot.forward * (size / 2.0);
        const Vec2 l = spot.forward.perpendicular() * (size / 2.0);
        made.plan.push_back(Polyline2{{spot.position - a - l, spot.position + a - l,
                                       spot.position + a + l, spot.position - a + l},
                                      true});
        addQuantity(made, "Count", 1.0, "");
        addQuantity(made, "Height", height, "m");
        out.push_back(std::move(made));
    }
    return out;
}

// A sign's plate in (u across its face, seen from the front, v up), its
// lower edge at `bottom`.
std::vector<Point2> signPlate(std::string_view shape, double width, double plateHeight,
                              double bottom)
{
    if (shape == "diamond") {
        const double half = width / 2.0;
        return {Point2(0.0, bottom), Point2(half, bottom + half), Point2(0.0, bottom + width),
                Point2(-half, bottom + half)};
    }
    if (shape == "circle") {
        return regularPolygon(Point2(0.0, bottom + width / 2.0), width / 2.0, kRoundSignSides,
                              -std::numbers::pi / 2.0);
    }
    if (shape == "octagon") {
        // `width` across the flats: the circumradius is half of it over
        // cos 22.5 degrees, a flat at the bottom.
        const double radius = width / 2.0 / std::cos(std::numbers::pi / 8.0);
        return regularPolygon(Point2(0.0, bottom + width / 2.0), radius, 8,
                              std::numbers::pi / 8.0);
    }
    if (shape == "triangle") {
        const double tall = width * std::sqrt(3.0) / 2.0;
        return {Point2(-width / 2.0, bottom), Point2(width / 2.0, bottom),
                Point2(0.0, bottom + tall)};
    }
    return {Point2(-width / 2.0, bottom), Point2(width / 2.0, bottom),
            Point2(width / 2.0, bottom + plateHeight), Point2(-width / 2.0, bottom + plateHeight)};
}

double polygonArea(const std::vector<Point2>& polygon)
{
    Polyline2 ring{polygon, true};
    return ring.area();
}

std::vector<bim::BimObject> makeSigns(const RoadObject& object, const Placement& placement)
{
    const Values values(object);
    const double width = values.number("width");
    const double clearance = values.number("clearance");
    const double post = values.number("postsize");
    const double thickness = values.number("thickness");
    const auto posts = static_cast<std::size_t>(values.number("posts"));
    const std::vector<Point2> plate =
        signPlate(values.text("shape"), width, values.number("plateheight"), clearance);
    double top = clearance;
    for (const Point2& corner : plate) {
        top = std::max(top, corner.y);
    }
    std::vector<bim::BimObject> out;
    std::size_t n = 0;
    for (const Spot& spot : placement.spots()) {
        ++n;
        bim::BimObject made =
            skeleton(object, values, "/" + std::to_string(n), " #" + std::to_string(n),
                     {"IfcSign", "PICTORAL", "ROADSIDE"});
        // The face looks back along the placement, at the traffic coming
        // up it, unless facing= says otherwise.
        const auto facing = values.optionalNumber("facing");
        const Vec2 face = facing ? unitAt(radians(*facing)) : -spot.forward;
        // Across the face, to the right of someone reading it: up x face.
        const Vec2 across(-face.y, face.x);
        const Point3 base(spot.position.x, spot.position.y, spot.z);
        // The posts on the spot, the plate on their front.
        std::vector<double> postAt{0.0};
        if (posts == 2) {
            postAt = {-width / 4.0, width / 4.0};
        }
        for (const double u : postAt) {
            const Point2 at = spot.position + across * u;
            addBox(made.mesh, Point3(at.x, at.y, spot.z), face, post, post, top);
        }
        const Vec3 u(across.x, across.y, 0.0);
        const Vec3 v(0.0, 0.0, 1.0);
        const Point3 back = base + Vec3(face.x, face.y, 0.0) * (post / 2.0);
        addExtrusion(made.mesh, plate, back, u, v, thickness);
        made.plan.push_back(Polyline2{{Point2(back.x, back.y) - across * (width / 2.0),
                                       Point2(back.x, back.y) + across * (width / 2.0)},
                                      false});
        if (!values.text("code").empty()) {
            made.properties.emplace_back("SignCode", values.text("code"));
        }
        if (!values.text("text").empty()) {
            made.properties.emplace_back("SignText", values.text("text"));
        }
        addQuantity(made, "Count", 1.0, "");
        addQuantity(made, "Height", top, "m");
        addQuantity(made, "GrossArea", polygonArea(plate), "m2");
        out.push_back(std::move(made));
    }
    return out;
}

std::vector<bim::BimObject> makeLights(const RoadObject& object, const Placement& placement)
{
    const Values values(object);
    const double height = values.number("height");
    const double outreach = values.number("outreach");
    const double size = values.number("polesize");
    const auto facing = values.optionalNumber("facing");
    std::vector<bim::BimObject> out;
    std::size_t n = 0;
    for (const Spot& spot : placement.spots()) {
        ++n;
        // The arm reaches over the road: towards the centreline along an
        // alignment, to the left of a line, south at a point.
        Vec2 arm = spot.forward.perpendicular();
        if (facing) {
            arm = unitAt(radians(*facing));
        } else if (object.placement.kind == PlacementKind::Alignment && spot.offset > 0.0) {
            arm = -arm;
        } else if (object.placement.kind == PlacementKind::Points) {
            arm = -spot.forward;
        }
        const std::string item = "/" + std::to_string(n);
        bim::BimObject pole = skeleton(object, values, item + "/pole",
                                       " #" + std::to_string(n) + " pole",
                                       {"IfcMember", "POST", "ROADSIDE"});
        const double radius = size / 2.0 / std::cos(std::numbers::pi / kPoleSides);
        const std::vector<Point2> section =
            regularPolygon(spot.position, radius, kPoleSides, std::numbers::pi / kPoleSides);
        addPrism(pole.mesh, section, spot.z, spot.z + height);
        pole.plan.push_back(Polyline2{section, true});
        double lanternBase = spot.z + height;
        Point2 lanternAt = spot.position;
        if (outreach > 0.0) {
            const Point2 middle = spot.position + arm * (outreach / 2.0);
            addBox(pole.mesh, Point3(middle.x, middle.y, spot.z + height - kArmSection), arm,
                   outreach, kArmSection, kArmSection);
            lanternAt = spot.position + arm * outreach;
            lanternBase = spot.z + height - kArmSection - kLuminaireHeight;
            pole.plan.push_back(Polyline2{{spot.position, lanternAt}, false});
        }
        addQuantity(pole, "Count", 1.0, "");
        addQuantity(pole, "Height", height, "m");
        bim::BimObject lantern = skeleton(object, values, item + "/luminaire",
                                          " #" + std::to_string(n) + " luminaire",
                                          {"IfcLightFixture", "POINTSOURCE", "ROADSIDE"});
        addBox(lantern.mesh, Point3(lanternAt.x, lanternAt.y, lanternBase), arm, kLuminaireLength,
               kLuminaireWidth, kLuminaireHeight);
        addQuantity(lantern, "Count", 1.0, "");
        out.push_back(std::move(pole));
        out.push_back(std::move(lantern));
    }
    return out;
}

// The barrel's inside and outside, in (u across, v up from the invert).
struct BarrelSection {
    double outerWidth = 0.0;
    double outerHeight = 0.0;
    // From the invert up to the barrel's outside top.
    double aboveInvert = 0.0;
};

BarrelSection barrelSection(const Values& values)
{
    const double wall = values.number("wall");
    if (values.text("shape") == "box") {
        const double span = values.number("span");
        const double rise = values.number("rise");
        return {span + 2.0 * wall, rise + 2.0 * wall, rise + wall};
    }
    const double diameter = values.number("diameter");
    return {diameter + 2.0 * wall, diameter + 2.0 * wall, diameter + wall};
}

CulvertEnds resolveCulvert(const RoadObject& object, const Placement& placement)
{
    const Values values(object);
    const RoadObjectPlacement& where = object.placement;
    const BarrelSection section = barrelSection(values);
    Point2 inlet;
    Point2 outlet;
    std::optional<double> groundIn;
    std::optional<double> groundOut;
    const auto invert = values.optionalNumber("invert");
    const auto invert2 = values.optionalNumber("invert2");
    const bool needGround = !invert || !invert2;
    if (where.kind == PlacementKind::Alignment) {
        if (where.to) {
            fail("a culvert crosses the alignment at one station: give length= and skew=, not "
                 "to=");
        }
        const Spot centre = placement.alignmentSpot(where.station);
        const Vec2 forward = centre.forward;
        const Vec2 left = forward.perpendicular();
        const double skew = radians(values.number("skew"));
        const double length = values.number("length");
        // Square across the alignment, turned `skew` counter-clockwise; the
        // inlet on the right.
        const Vec2 axis = left * std::cos(skew) - forward * std::sin(skew);
        inlet = centre.position - axis * (length / 2.0);
        outlet = centre.position + axis * (length / 2.0);
        if (needGround) {
            // Each end's own station and offset, for the profile: exact on
            // a straight, the end's frame at the culvert's station on a curve.
            const double along = std::sin(skew) * length / 2.0;
            const double across = std::cos(skew) * length / 2.0;
            const auto level = [&](double station, double offset, const Point2& at) {
                return placement.levelOnAlignment(placement.checkedStation(station), offset, at);
            };
            groundIn = level(where.station + along, where.offset - across, inlet);
            groundOut = level(where.station - along, where.offset + across, outlet);
        }
    } else {
        if (where.kind == PlacementKind::Points && where.vertices.size() != 2) {
            fail("a culvert AT points takes two: its inlet, then its outlet");
        }
        if (where.vertices.size() < 2) {
            fail("a culvert along a line needs its two ends");
        }
        inlet = where.vertices.front().position;
        outlet = where.vertices.back().position;
        if (placement.source() == HeightSource::Line) {
            // A culvert's line is drawn at its invert, as a drainage line is;
            // an invert given wins, and a vertex it makes unneeded may have
            // no height.
            const std::size_t last = where.vertices.size() - 1;
            const double in = invert ? *invert : placement.vertexLevel(0);
            const double out = invert2 ? *invert2 : invert ? *invert : placement.vertexLevel(last);
            if (inlet.distanceTo(outlet) < 0.1) {
                fail("the culvert's ends are in one place");
            }
            return {Point3(inlet.x, inlet.y, in), Point3(outlet.x, outlet.y, out)};
        }
        if (needGround) {
            groundIn = placement.surfaceLevel(inlet) + where.dz;
            groundOut = placement.surfaceLevel(outlet) + where.dz;
        }
    }
    if (inlet.distanceTo(outlet) < 0.1) {
        fail("the culvert's ends are in one place");
    }
    const double cover = values.number("cover");
    const double inletInvert =
        invert ? *invert : *groundIn - cover - section.aboveInvert;
    const double outletInvert =
        invert2 ? *invert2 : (invert ? *invert : *groundOut - cover - section.aboveInvert);
    return {Point3(inlet.x, inlet.y, inletInvert), Point3(outlet.x, outlet.y, outletInvert)};
}

std::vector<bim::BimObject> makeCulvert(const RoadObject& object, const Placement& placement)
{
    const Values values(object);
    const CulvertEnds ends = resolveCulvert(object, placement);
    const BarrelSection section = barrelSection(values);
    const auto cells = static_cast<std::size_t>(values.number("cells"));
    const double gap = values.number("cellgap");
    const double wall = values.number("wall");
    const bool box = values.text("shape") == "box";
    const double pitch = section.outerWidth + gap;
    const double total = section.outerWidth * static_cast<double>(cells) +
                         gap * static_cast<double>(cells - 1);
    const std::vector<PathPoint> path{{Point2(ends.inlet.x, ends.inlet.y), ends.inlet.z},
                                      {Point2(ends.outlet.x, ends.outlet.y), ends.outlet.z}};
    bim::BimObject barrel = skeleton(object, values, "/barrel", " barrel",
                                     {"IfcPipeSegment", "CULVERT", "ROADSEGMENT"});
    for (std::size_t k = 0; k < cells; ++k) {
        const double u = (static_cast<double>(k) - static_cast<double>(cells - 1) / 2.0) * pitch;
        if (box) {
            const double span = values.number("span");
            const double rise = values.number("rise");
            addHollowSweep(barrel.mesh, path,
                           rectangleProfile(u - span / 2.0 - wall, u + span / 2.0 + wall, -wall,
                                            rise + wall),
                           rectangleProfile(u - span / 2.0, u + span / 2.0, 0.0, rise));
        } else {
            const double diameter = values.number("diameter");
            const Point2 centre(u, diameter / 2.0);
            addHollowSweep(barrel.mesh, path,
                           regularPolygon(centre, diameter / 2.0 + wall, kPipeSides),
                           regularPolygon(centre, diameter / 2.0, kPipeSides));
        }
    }
    const Vec2 axis = (Point2(ends.outlet.x, ends.outlet.y) - Point2(ends.inlet.x, ends.inlet.y))
                          .normalized();
    const Vec2 left = axis.perpendicular();
    const Point2 in(ends.inlet.x, ends.inlet.y);
    const Point2 out(ends.outlet.x, ends.outlet.y);
    barrel.plan.push_back(Polyline2{{in - left * (total / 2.0), out - left * (total / 2.0),
                                     out + left * (total / 2.0), in + left * (total / 2.0)},
                                    true});
    barrel.properties.emplace_back("Inlet", pointText(ends.inlet));
    barrel.properties.emplace_back("Outlet", pointText(ends.outlet));
    addQuantity(barrel, "Length", (ends.outlet - ends.inlet).length(), "m");
    addQuantity(barrel, "Count", static_cast<double>(cells), "");
    addQuantity(barrel, "GrossVolume", meshVolume(barrel.mesh), "m3");
    std::vector<bim::BimObject> made{std::move(barrel)};
    if (values.text("headwalls") == "on") {
        const double width = total + 2.0 * kHeadwallMargin;
        const double tall = section.outerHeight + kHeadwallMargin;
        for (const bool atInlet : {true, false}) {
            const Point3& end = atInlet ? ends.inlet : ends.outlet;
            const Vec2 outward = atInlet ? -axis : axis;
            const Point2 centre = Point2(end.x, end.y) + outward * (kHeadwallThickness / 2.0);
            bim::BimObject headwall = skeleton(object, values,
                                               atInlet ? "/headwall/inlet" : "/headwall/outlet",
                                               atInlet ? " inlet headwall" : " outlet headwall",
                                               {"IfcWall", "RETAININGWALL", "ROADSEGMENT"});
            // Standing on the barrel's outside bottom, outside its end.
            addBox(headwall.mesh, Point3(centre.x, centre.y, end.z - wall), axis,
                   kHeadwallThickness, width, tall);
            const Vec2 a = axis * (kHeadwallThickness / 2.0);
            const Vec2 l = left * (width / 2.0);
            headwall.plan.push_back(
                Polyline2{{centre - a - l, centre + a - l, centre + a + l, centre - a + l}, true});
            addQuantity(headwall, "Width", width, "m");
            addQuantity(headwall, "GrossVolume", meshVolume(headwall.mesh), "m3");
            made.push_back(std::move(headwall));
        }
    }
    return made;
}

bool sameWord(std::string_view a, std::string_view b)
{
    return katana::core::equalsIgnoringCase(a, b);
}

} // namespace

// ---- kinds ----------------------------------------------------------------------------------

std::span<const RoadObjectKind> roadObjectKinds()
{
    static constexpr std::array kKinds{RoadObjectKind::Barrier,   RoadObjectKind::GuidePost,
                                       RoadObjectKind::Sign,      RoadObjectKind::LightPole,
                                       RoadObjectKind::Culvert,   RoadObjectKind::Wall,
                                       RoadObjectKind::Fence,     RoadObjectKind::Marking};
    static_assert(kKinds.size() == kRoadObjectKindCount);
    return kKinds;
}

std::string_view kindWord(RoadObjectKind kind)
{
    switch (kind) {
    case RoadObjectKind::Barrier:
        return "barrier";
    case RoadObjectKind::GuidePost:
        return "guidepost";
    case RoadObjectKind::Sign:
        return "sign";
    case RoadObjectKind::LightPole:
        return "light";
    case RoadObjectKind::Culvert:
        return "culvert";
    case RoadObjectKind::Wall:
        return "wall";
    case RoadObjectKind::Fence:
        return "fence";
    case RoadObjectKind::Marking:
        return "marking";
    }
    return "barrier";
}

Result<RoadObjectKind> kindFromWord(std::string_view word)
{
    const std::string folded = katana::core::lowered(katana::core::trimmed(word));
    for (const RoadObjectKind kind : roadObjectKinds()) {
        const std::string_view name = kindWord(kind);
        if (folded == name || folded == std::string(name) + "s") {
            return kind;
        }
    }
    // The words the menu says, run together.
    if (folded == "lightpole" || folded == "lightpoles") {
        return RoadObjectKind::LightPole;
    }
    if (folded == "linemarking" || folded == "linemarkings") {
        return RoadObjectKind::Marking;
    }
    return makeError(ErrorCode::NotFound,
                     "not a road object kind; the kinds are barrier, guidepost, sign, light, "
                     "culvert, wall, fence and marking",
                     std::string(word));
}

std::string_view kindTitle(RoadObjectKind kind)
{
    switch (kind) {
    case RoadObjectKind::Barrier:
        return "Safety barrier";
    case RoadObjectKind::GuidePost:
        return "Guide posts";
    case RoadObjectKind::Sign:
        return "Sign";
    case RoadObjectKind::LightPole:
        return "Light poles";
    case RoadObjectKind::Culvert:
        return "Culvert";
    case RoadObjectKind::Wall:
        return "Wall";
    case RoadObjectKind::Fence:
        return "Fence";
    case RoadObjectKind::Marking:
        return "Line marking";
    }
    return "Road object";
}

KindShape shapeOf(RoadObjectKind kind)
{
    switch (kind) {
    case RoadObjectKind::Barrier:
    case RoadObjectKind::Wall:
    case RoadObjectKind::Fence:
    case RoadObjectKind::Marking:
        return KindShape::Linear;
    case RoadObjectKind::GuidePost:
    case RoadObjectKind::Sign:
    case RoadObjectKind::LightPole:
        return KindShape::Point;
    case RoadObjectKind::Culvert:
        return KindShape::Crossing;
    }
    return KindShape::Linear;
}

// ---- parameters -----------------------------------------------------------------------------

std::span<const ParameterSpec> parametersOf(RoadObjectKind kind)
{
    switch (kind) {
    case RoadObjectKind::Barrier:
        return kBarrierParameters;
    case RoadObjectKind::GuidePost:
        return kGuidePostParameters;
    case RoadObjectKind::Sign:
        return kSignParameters;
    case RoadObjectKind::LightPole:
        return kLightParameters;
    case RoadObjectKind::Culvert:
        return kCulvertParameters;
    case RoadObjectKind::Wall:
        return kWallParameters;
    case RoadObjectKind::Fence:
        return kFenceParameters;
    case RoadObjectKind::Marking:
        return kMarkingParameters;
    }
    return {};
}

const ParameterSpec* findParameter(RoadObjectKind kind, std::string_view key)
{
    for (const ParameterSpec& spec : parametersOf(kind)) {
        if (sameWord(spec.key, key)) {
            return &spec;
        }
    }
    return nullptr;
}

std::string_view kindSource(RoadObjectKind kind)
{
    switch (kind) {
    case RoadObjectKind::Barrier:
        return "Beam rails of AASHTO M 180 (W-beam 311 mm, thrie-beam 508 mm, 83 mm deep) on "
               "150 x 100 mm steel posts at 1.905 m; tops of rail 787 mm (the MASH-tested "
               "Midwest Guardrail System) and 864 mm; the F-shape's 76 mm reveal, 55 degrees "
               "to 254 mm and 84 degrees above on a 610 mm base, 813 mm tall, and the 1067 mm "
               "single slope (AASHTO Roadside Design Guide); wire rope typical of the "
               "proprietary systems. The job's standard drawings govern.";
    case RoadObjectKind::GuidePost:
        return "Guide posts as AS 1742.2 sets them out; 1.0 m above the ground and 100 mm "
               "square is typical. The job's standard drawing governs.";
    case RoadObjectKind::Sign:
        return "The plate's lower edge 1.5 m above the ground: the rural minimum of the MUTCD "
               "(2009), section 2A.18 (5 ft; 7 ft, 2.1 m, where there are pedestrians). A "
               "900 x 600 mm plate on a 75 mm post is typical; the sign's standard governs.";
    case RoadObjectKind::LightPole:
        return "A 10 m mounting height and a 1.5 m outreach, typical of road lighting to "
               "AS/NZS 1158; the lighting design governs.";
    case RoadObjectKind::Culvert:
        return "A 900 mm reinforced concrete pipe with a 100 mm wall, typical of the AS/NZS "
               "4058 sizes; 600 mm of cover, a typical minimum under a pavement (AS/NZS 3725 "
               "governs the loads); headwalls typical of precast units.";
    case RoadObjectKind::Wall:
        return "A 2.0 m retaining wall with a 250 mm stem on a 1.2 x 0.4 m strip footing, and "
               "a 4.0 m noise wall of 200 mm panels: typical; the structural design governs.";
    case RoadObjectKind::Fence:
        return "1.2 m posts at 3 m with three wires: a typical rural boundary fence.";
    case RoadObjectKind::Marking:
        return "AS 1742.2's longitudinal lines: an edge line 150 mm; dividing and lane lines "
               "100 mm, broken 3 m line and 9 m gap; continuity lines 1 m and 3 m; 3 mm of "
               "thermoplastic. The job's line marking plan governs.";
    }
    return {};
}

Result<ParameterValue> parseParameter(RoadObjectKind kind, std::string_view key,
                                      std::string_view text)
{
    const ParameterSpec* spec = findParameter(kind, key);
    if (spec == nullptr) {
        std::string keys;
        for (const ParameterSpec& each : parametersOf(kind)) {
            keys += (keys.empty() ? "" : ", ") + std::string(each.key);
        }
        return makeError(ErrorCode::NotFound,
                         std::format("a {} takes no {}=; it takes {}, and the keys every object "
                                     "takes",
                                     kindWord(kind), key, keys),
                         std::string(key));
    }
    switch (spec->type) {
    case ParameterType::Number: {
        const auto value = katana::core::parseFiniteDouble(katana::core::trimmed(text));
        if (!value) {
            return makeError(ErrorCode::ParseFailure,
                             std::format("{}= must be a number", spec->key), std::string(text));
        }
        if (spec->whole && *value != std::floor(*value)) {
            return makeError(ErrorCode::InvalidArgument,
                             std::format("{}= is a whole number", spec->key), std::string(text));
        }
        if (*value < spec->minimum || *value > spec->maximum) {
            return makeError(ErrorCode::InvalidArgument,
                             std::format("{}= must be from {} to {}{}", spec->key,
                                         real(spec->minimum), real(spec->maximum),
                                         spec->unit.empty() ? "" : " " + std::string(spec->unit)),
                             std::string(text));
        }
        return ParameterValue(*value);
    }
    case ParameterType::Choice: {
        const std::string folded = katana::core::lowered(katana::core::trimmed(text));
        for (const std::string_view choice : spec->choices) {
            if (folded == choice) {
                return ParameterValue(folded);
            }
        }
        std::string choices;
        for (const std::string_view choice : spec->choices) {
            choices += (choices.empty() ? "" : ", ") + std::string(choice);
        }
        return makeError(ErrorCode::InvalidArgument,
                         std::format("{}= is one of {}", spec->key, choices), std::string(text));
    }
    case ParameterType::Text:
        return ParameterValue(std::string(text));
    }
    return makeError(ErrorCode::Internal, "a parameter of no type", std::string(key));
}

std::string parameterText(const ParameterValue& value)
{
    if (const auto* number = std::get_if<double>(&value)) {
        return real(*number);
    }
    return std::get<std::string>(value);
}

// ---- the definition -------------------------------------------------------------------------

std::string_view placementWord(PlacementKind kind)
{
    switch (kind) {
    case PlacementKind::Alignment:
        return "alignment";
    case PlacementKind::Line:
        return "line";
    case PlacementKind::Points:
        return "points";
    }
    return "alignment";
}

std::string_view heightWord(HeightSource source)
{
    switch (source) {
    case HeightSource::Profile:
        return "profile";
    case HeightSource::Surface:
        return "surface";
    case HeightSource::Line:
        return "line";
    }
    return "profile";
}

Result<HeightSource> heightFromWord(std::string_view word)
{
    const std::string folded = katana::core::lowered(katana::core::trimmed(word));
    for (const HeightSource source :
         {HeightSource::Profile, HeightSource::Surface, HeightSource::Line}) {
        if (folded == heightWord(source)) {
            return source;
        }
    }
    return makeError(ErrorCode::InvalidArgument, "heights= is profile, surface or line",
                     std::string(word));
}

const RoadObject* RoadObjectSet::find(std::string_view id) const
{
    for (const RoadObject& object : objects) {
        if (sameWord(object.id, id)) {
            return &object;
        }
    }
    return nullptr;
}

RoadObject* RoadObjectSet::find(std::string_view id)
{
    for (RoadObject& object : objects) {
        if (sameWord(object.id, id)) {
            return &object;
        }
    }
    return nullptr;
}

std::string RoadObjectSet::takeId(RoadObjectKind kind)
{
    // Past any id a file edited by hand already holds.
    for (;;) {
        std::string id = std::string(kindWord(kind)) + "-" + std::to_string(nextNumber++);
        if (find(id) == nullptr) {
            return id;
        }
    }
}

HeightSource heightSourceOf(const RoadObjectPlacement& placement)
{
    if (placement.heights) {
        return *placement.heights;
    }
    if (!placement.surface.empty()) {
        return HeightSource::Surface;
    }
    return placement.kind == PlacementKind::Alignment ? HeightSource::Profile : HeightSource::Line;
}

std::string layerOf(const RoadObject& object)
{
    if (!object.layer.empty()) {
        return object.layer;
    }
    return "road/objects/" + std::string(kindWord(object.kind));
}

namespace {

bool concreteBarrier(const RoadObject& object)
{
    if (object.kind != RoadObjectKind::Barrier) {
        return false;
    }
    const std::string_view type = textOf(object.parameters, "type");
    return type == "fshape" || type == "singleslope";
}

} // namespace

std::string materialOf(const RoadObject& object)
{
    if (!object.material.empty()) {
        return object.material;
    }
    switch (object.kind) {
    case RoadObjectKind::Barrier:
        return concreteBarrier(object) ? "concrete" : "steel";
    case RoadObjectKind::GuidePost:
        return "plastic";
    case RoadObjectKind::Sign:
        return "aluminium";
    case RoadObjectKind::LightPole:
    case RoadObjectKind::Fence:
        return "steel";
    case RoadObjectKind::Culvert:
    case RoadObjectKind::Wall:
        return "concrete";
    case RoadObjectKind::Marking:
        return "thermoplastic";
    }
    return {};
}

katana::entity::Color colourOf(const RoadObject& object)
{
    if (object.colour) {
        return *object.colour;
    }
    // Colours a person tells apart in the 3D view: galvanised steel, grey
    // concrete, a white post, a green guide sign, line-marking white.
    const auto rgb = [](std::uint8_t r, std::uint8_t g, std::uint8_t b) {
        return katana::entity::Color{r, g, b, 255};
    };
    switch (object.kind) {
    case RoadObjectKind::Barrier:
        return concreteBarrier(object) ? rgb(189, 184, 174) : rgb(167, 173, 179);
    case RoadObjectKind::GuidePost:
        return rgb(244, 244, 244);
    case RoadObjectKind::Sign:
        return rgb(46, 125, 50);
    case RoadObjectKind::LightPole:
        return rgb(141, 148, 155);
    case RoadObjectKind::Culvert:
        return rgb(158, 154, 146);
    case RoadObjectKind::Wall:
        return rgb(181, 176, 166);
    case RoadObjectKind::Fence:
        return rgb(139, 115, 85);
    case RoadObjectKind::Marking:
        return rgb(250, 250, 250);
    }
    return rgb(200, 200, 200);
}

std::string displayName(const RoadObject& object)
{
    if (!object.name.empty()) {
        return object.name;
    }
    return std::string(kindTitle(object.kind)) + " " + object.id;
}

Parameters effectiveParameters(const RoadObject& object)
{
    Parameters out;
    std::string type;
    if (const std::string_view key = typeKey(object.kind); !key.empty()) {
        type = std::string(textOf(object.parameters, key));
        if (type.empty()) {
            if (const ParameterSpec* spec = findParameter(object.kind, key)) {
                type = std::string(spec->defaultText);
            }
        }
    }
    for (const ParameterSpec& spec : parametersOf(object.kind)) {
        if (const auto given = object.parameters.find(spec.key);
            given != object.parameters.end()) {
            out.insert_or_assign(std::string(spec.key), given->second);
            continue;
        }
        if (auto value = defaultOf(object.kind, type, spec)) {
            out.insert_or_assign(std::string(spec.key), std::move(*value));
        }
    }
    return out;
}

Status applyKey(RoadObject& object, std::string_view key, std::string_view value)
{
    const std::string folded = katana::core::lowered(key);
    RoadObjectPlacement& placement = object.placement;
    const auto number = [&](double low, double high) -> Result<double> {
        const auto parsed = katana::core::parseFiniteDouble(katana::core::trimmed(value));
        if (!parsed) {
            return makeError(ErrorCode::ParseFailure, std::format("{}= must be a number", folded),
                             std::string(value));
        }
        if (*parsed < low || *parsed > high) {
            return makeError(ErrorCode::InvalidArgument,
                             std::format("{}= must be from {} to {}", folded, real(low),
                                         real(high)),
                             std::string(value));
        }
        return *parsed;
    };
    const auto alongAlignment = [&]() -> Status {
        if (placement.kind != PlacementKind::Alignment) {
            return makeError(ErrorCode::InvalidArgument,
                             std::format("{}= is for an object along an alignment; this one is "
                                         "placed {}",
                                         folded,
                                         placement.kind == PlacementKind::Line ? "along a line"
                                                                               : "at points"),
                             object.id);
        }
        return {};
    };
    // A station, an offset: anywhere on a project, as a level is.
    constexpr double kFar = 1e7;
    if (folded == "name") {
        object.name = std::string(value);
        return {};
    }
    if (folded == "layer") {
        if (!value.empty()) {
            if (auto status = katana::entity::validateLayerPath(value); !status) {
                return status;
            }
        }
        object.layer = std::string(value);
        return {};
    }
    if (folded == "colour" || folded == "color") {
        if (value.empty()) {
            object.colour.reset();
            return {};
        }
        if (value.front() == '#') {
            auto colour = katana::entity::Color::fromHex(value);
            if (!colour) {
                return colour.error();
            }
            object.colour = *colour;
            return {};
        }
        const auto named = katana::entity::standardColour(value);
        if (!named) {
            return makeError(ErrorCode::InvalidArgument,
                             "colour= is #RRGGBB or a standard colour name (red, dark green ...)",
                             std::string(value));
        }
        object.colour = *named;
        return {};
    }
    if (folded == "material") {
        object.material = std::string(value);
        return {};
    }
    if (folded == "facility") {
        object.facility = std::string(value);
        return {};
    }
    if (folded == "alignment") {
        if (auto status = alongAlignment(); !status) {
            return status;
        }
        if (value.empty()) {
            return makeError(ErrorCode::InvalidArgument, "alignment= needs a name");
        }
        placement.alignment = std::string(value);
        return {};
    }
    if (folded == "station" || folded == "offset") {
        if (auto status = alongAlignment(); !status) {
            return status;
        }
        auto parsed = number(-kFar, kFar);
        if (!parsed) {
            return parsed.error();
        }
        (folded == "station" ? placement.station : placement.offset) = *parsed;
        return {};
    }
    if (folded == "to") {
        if (auto status = alongAlignment(); !status) {
            return status;
        }
        if (value.empty()) {
            placement.to.reset();
            return {};
        }
        auto parsed = number(-kFar, kFar);
        if (!parsed) {
            return parsed.error();
        }
        placement.to = *parsed;
        return {};
    }
    if (folded == "spacing") {
        if (value.empty()) {
            placement.spacing.reset();
            return {};
        }
        auto parsed = number(0.0, kFar);
        if (!parsed) {
            return parsed.error();
        }
        if (!(*parsed > 0.0)) {
            return makeError(ErrorCode::InvalidArgument, "spacing= must be more than 0",
                             std::string(value));
        }
        placement.spacing = *parsed;
        return {};
    }
    if (folded == "heights") {
        if (value.empty()) {
            placement.heights.reset();
            return {};
        }
        auto source = heightFromWord(value);
        if (!source) {
            return source.error();
        }
        placement.heights = *source;
        return {};
    }
    if (folded == "surface") {
        placement.surface = std::string(value);
        return {};
    }
    if (folded == "crossfall") {
        // Percent: a road's crossfall is a few percent, a verge's up to a
        // batter's 50 (1 on 2).
        auto parsed = number(-50.0, 50.0);
        if (!parsed) {
            return parsed.error();
        }
        placement.crossfall = *parsed;
        return {};
    }
    if (folded == "dz") {
        auto parsed = number(-1000.0, 1000.0);
        if (!parsed) {
            return parsed.error();
        }
        placement.dz = *parsed;
        return {};
    }
    const ParameterSpec* spec = findParameter(object.kind, folded);
    if (spec != nullptr && value.empty()) {
        object.parameters.erase(std::string(spec->key));
        return {};
    }
    auto parsed = parseParameter(object.kind, folded, value);
    if (!parsed) {
        return parsed.error();
    }
    object.parameters.insert_or_assign(std::string(spec->key), std::move(*parsed));
    return {};
}

// ---- making the objects ---------------------------------------------------------------------

PlacementContext placementContext(const Document& document)
{
    PlacementContext context;
    context.alignment = [&document](std::string_view name) {
        return document.model().alignments.find(name);
    };
    context.surface = [&document](std::string_view name) -> const katana::terrain::TinSurface* {
        const katana::terrain::NamedSurface* found = document.surfaces().find(name);
        return found != nullptr ? found->surface.get() : nullptr;
    };
    return context;
}

bim::BimModel buildRoadObject(const PlacementContext& context, const RoadObject& object)
{
    bim::BimModel model;
    try {
        const Placement placement(context, object);
        switch (object.kind) {
        case RoadObjectKind::Barrier:
            model.objects = makeBarrier(object, placement);
            break;
        case RoadObjectKind::GuidePost:
            model.objects = makeGuidePosts(object, placement);
            break;
        case RoadObjectKind::Sign:
            model.objects = makeSigns(object, placement);
            break;
        case RoadObjectKind::LightPole:
            model.objects = makeLights(object, placement);
            break;
        case RoadObjectKind::Culvert:
            model.objects = makeCulvert(object, placement);
            break;
        case RoadObjectKind::Wall:
            model.objects = makeWall(object, placement);
            break;
        case RoadObjectKind::Fence:
            model.objects = makeFence(object, placement);
            break;
        case RoadObjectKind::Marking:
            model.objects = makeMarking(object, placement);
            break;
        }
        if (model.objects.empty()) {
            fail("its placement gives nowhere to stand it");
        }
    } catch (const Unplaceable& problem) {
        model.objects.clear();
        model.problems.push_back({"object " + object.id, problem.message});
    }
    return model;
}

std::vector<bim::Quantity> totalQuantities(std::span<const bim::BimObject> objects)
{
    std::vector<bim::Quantity> totals;
    for (const bim::BimObject& object : objects) {
        for (const bim::Quantity& quantity : object.quantities) {
            const auto found = std::ranges::find(totals, quantity.name, &bim::Quantity::name);
            if (found == totals.end()) {
                totals.push_back(quantity);
                continue;
            }
            // A height, a width or a depth is the largest; the rest add up.
            if (quantity.name == "Height" || quantity.name == "Width" ||
                quantity.name == "Depth") {
                found->value = std::max(found->value, quantity.value);
            } else {
                found->value += quantity.value;
            }
        }
    }
    return totals;
}

Result<CulvertEnds> culvertEnds(const PlacementContext& context, const RoadObject& object)
{
    if (object.kind != RoadObjectKind::Culvert) {
        return makeError(ErrorCode::InvalidArgument, "not a culvert", object.id);
    }
    try {
        const Placement placement(context, object);
        return resolveCulvert(object, placement);
    } catch (const Unplaceable& problem) {
        return makeError(ErrorCode::InvalidGeometry, problem.message, object.id);
    }
}

} // namespace katana::cad::road

namespace katana::cad::bim {

BimModel placedObjects(const Document& document)
{
    BimModel model;
    if (const auto status = document.roadObjectsStatus(); !status) {
        model.problems.push_back({"road objects", status.error().describe()});
    }
    const road::PlacementContext context = road::placementContext(document);
    for (const road::RoadObject& object : document.roadObjects().objects) {
        BimModel one = road::buildRoadObject(context, object);
        std::ranges::move(one.objects, std::back_inserter(model.objects));
        std::ranges::move(one.problems, std::back_inserter(model.problems));
    }
    return model;
}

} // namespace katana::cad::bim
