// An IFC file -> entities, layers, alignments and surfaces (import.hpp).
//
// The parse (step_reader) knows no schema; this file gives the instances
// their meaning, reading each entity by the positions of its attributes -
// which, for everything read here, are the same in IFC2X3, IFC4 and IFC4X3.
// Where a schema adds an attribute (a PredefinedType), it is looked for by
// kind rather than assumed.

#include "katana/ifc/import.hpp"

#include <algorithm>
#include <cmath>
#include <fstream>
#include <iterator>
#include <set>
#include <unordered_map>
#include <unordered_set>

#include "import_alignment.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/entity/layer_path.hpp"
#include "katana/geometry/curves2d.hpp"
#include "katana/math/mat4.hpp"
#include "katana/math/numerics.hpp"
#include "step_reader.hpp"

namespace katana::ifc {

using detail::StepInstance;
using detail::StepValue;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::math::Mat4;
using katana::math::Vec2;
using katana::math::Vec3;

namespace {

enum class ProductKind { Drawn, NotDrawn, Spatial, Positioning };

struct ProductClass {
    std::string_view upper;
    std::string_view name;
    ProductKind kind;
};

constexpr ProductClass kProductClasses[] = {
#include "product_classes.inc"
};

const ProductClass* productClass(std::string_view upper)
{
    const auto* end = std::end(kProductClasses);
    const auto* found = std::lower_bound(
        std::begin(kProductClasses), end, upper,
        [](const ProductClass& c, std::string_view name) { return c.upper < name; });
    return found != end && found->upper == upper ? found : nullptr;
}

// Nested mapped items and composite curves are followed at most this deep;
// a file that nests further (or refers to itself) is refused the rest.
constexpr int kMaximumNesting = 32;

// A segment of text as a layer path segment: no separator, no control
// character, not "." or "..", not only blanks.
std::string layerSegment(std::string_view text)
{
    std::string out;
    for (const char c : text) {
        out.push_back(c == '/' || static_cast<unsigned char>(c) < 0x20 ? '-' : c);
    }
    const auto first = out.find_first_not_of(' ');
    if (first == std::string::npos) {
        return {};
    }
    out = out.substr(first, out.find_last_not_of(' ') - first + 1);
    if (out == "." || out == "..") {
        out = "_";
    }
    return out;
}

// A presentation layer's name as a Katana layer path: its '/'-separated parts
// kept as the tree they already are, empty ones dropped.
std::string layerPath(std::string_view name)
{
    std::string out;
    std::size_t depth = 0;
    std::size_t at = 0;
    while (at <= name.size() && depth < entity::kMaximumLayerDepth) {
        const std::size_t end = std::min(name.find('/', at), name.size());
        std::string part;
        for (const char c : name.substr(at, end - at)) {
            part.push_back(static_cast<unsigned char>(c) < 0x20 ? '-' : c);
        }
        part = layerSegment(part);
        if (!part.empty()) {
            out += out.empty() ? part : "/" + part;
            ++depth;
        }
        at = end + 1;
    }
    if (out.size() > entity::kMaximumLayerNameLength) {
        out.resize(entity::kMaximumLayerNameLength);
    }
    return out;
}

double siPrefix(std::string_view prefix)
{
    static const std::pair<std::string_view, double> kPrefixes[] = {
        {"EXA", 1e18},  {"PETA", 1e15},  {"TERA", 1e12},   {"GIGA", 1e9},
        {"MEGA", 1e6},  {"KILO", 1e3},   {"HECTO", 1e2},   {"DECA", 1e1},
        {"DECI", 1e-1}, {"CENTI", 1e-2}, {"MILLI", 1e-3},  {"MICRO", 1e-6},
        {"NANO", 1e-9}, {"PICO", 1e-12}, {"FEMTO", 1e-15}, {"ATTO", 1e-18}};
    for (const auto& [name, factor] : kPrefixes) {
        if (name == prefix) {
            return factor;
        }
    }
    return 1.0;
}

// IfcBoxAlignment's words, back to a justification.
entity::TextJustify justification(std::string_view box)
{
    using J = entity::TextJustify;
    static const std::pair<std::string_view, J> kBoxes[] = {
        {"bottom-left", J::BottomLeft},   {"bottom-middle", J::BottomCentre},
        {"bottom-right", J::BottomRight}, {"middle-left", J::MiddleLeft},
        {"center", J::MiddleCentre},      {"middle-right", J::MiddleRight},
        {"top-left", J::TopLeft},         {"top-middle", J::TopCentre},
        {"top-right", J::TopRight}};
    for (const auto& [name, justify] : kBoxes) {
        if (name == box) {
            return justify;
        }
    }
    return J::BottomLeft;
}

// A drawn shape in project coordinates, and the item it came from.
struct Shape {
    entity::Geometry geometry;
    std::vector<std::optional<double>> heights;
    std::uint32_t item = 0;
    std::optional<double> sweptRadius; // the body this is the centre line of
};

class Reader {
  public:
    Reader(const detail::StepModel& model, const ImportOptions& options, IfcImport& out)
        : m_(model), options_(options), out_(out)
    {
    }

    void run()
    {
        readUnits();
        readGeoreference();
        index();
        if (options_.importAlignments) {
            readAlignments();
        }
        readProducts();
        finish();
    }

  private:
    // ---- access -----------------------------------------------------------------------

    static const StepValue* arg(const StepInstance& instance, std::size_t index)
    {
        return index < instance.arguments.size() ? &instance.arguments[index] : nullptr;
    }

    const StepInstance* at(const StepInstance& instance, std::size_t index) const
    {
        const StepValue* value = arg(instance, index);
        return value ? m_.follow(*value) : nullptr;
    }

    static std::string text(const StepInstance& instance, std::size_t index)
    {
        const StepValue* value = arg(instance, index);
        const std::string* s = value ? value->string() : nullptr;
        return s ? *s : std::string();
    }

    static std::optional<double> number(const StepInstance& instance, std::size_t index)
    {
        const StepValue* value = arg(instance, index);
        return value ? value->number() : std::nullopt;
    }

    static std::vector<std::uint32_t> ids(const StepInstance& instance, std::size_t index)
    {
        std::vector<std::uint32_t> out;
        const StepValue* value = arg(instance, index);
        if (value == nullptr) {
            return out;
        }
        if (value->kind == StepValue::Kind::Reference) {
            out.push_back(value->reference);
        } else if (value->kind == StepValue::Kind::List) {
            for (const StepValue& item : value->items) {
                if (item.kind == StepValue::Kind::Reference) {
                    out.push_back(item.reference);
                }
            }
        }
        return out;
    }

    static bool is(const StepInstance* instance, std::string_view type)
    {
        return instance != nullptr && instance->type == type;
    }

    void warnOnce(const std::string& message)
    {
        if (warned_.insert(message).second) {
            out_.warnings.push_back(message);
        }
    }

    // ---- units and frame -------------------------------------------------------------

    double unitScale(const StepInstance* unit, int depth = 0)
    {
        if (unit == nullptr || depth > 4) {
            return 1.0;
        }
        if (unit->type == "IFCSIUNIT") {
            const double prefix = siPrefix(text(*unit, 2));
            const std::string name = text(*unit, 3);
            if (name == "SQUARE_METRE") {
                return prefix * prefix;
            }
            if (name == "CUBIC_METRE") {
                return prefix * prefix * prefix;
            }
            return prefix;
        }
        if (unit->type == "IFCCONVERSIONBASEDUNIT" ||
            unit->type == "IFCCONVERSIONBASEDUNITWITHOFFSET") {
            const StepInstance* factor = at(*unit, 3); // IfcMeasureWithUnit
            if (factor != nullptr) {
                const double value = number(*factor, 0).value_or(1.0);
                return value * unitScale(at(*factor, 1), depth + 1);
            }
        }
        return 1.0;
    }

    void readUnits()
    {
        const StepInstance* project = nullptr;
        for (const std::uint32_t id : m_.order) {
            if (m_.find(id)->type == "IFCPROJECT") {
                project = m_.find(id);
                break;
            }
        }
        if (project == nullptr) {
            warnOnce("the file has no IfcProject: its lengths are read as metres");
            return;
        }
        const StepInstance* assignment = at(*project, 8);
        if (assignment == nullptr) {
            return;
        }
        for (const std::uint32_t id : ids(*assignment, 0)) {
            const StepInstance* unit = m_.find(id);
            if (unit == nullptr) {
                continue;
            }
            const std::string type = text(*unit, 1);
            if (type == "LENGTHUNIT") {
                length_ = unitScale(unit);
            } else if (type == "PLANEANGLEUNIT") {
                angle_ = unitScale(unit);
            } else if (type == "AREAUNIT") {
                area_ = unitScale(unit);
            } else if (type == "VOLUMEUNIT") {
                volume_ = unitScale(unit);
            }
        }
    }

    // The map conversion, or the length unit alone: what takes a point in the
    // file's world frame to project coordinates in metres.
    void readGeoreference()
    {
        world_ = Mat4::scaling(Vec3(length_, length_, length_));
        for (const std::uint32_t id : m_.order) {
            const StepInstance& instance = *m_.find(id);
            if (instance.type != "IFCMAPCONVERSION" && instance.type != "IFCMAPCONVERSIONSCALED") {
                continue;
            }
            const StepInstance* crs = at(instance, 1);
            double mapUnit = 1.0;
            if (crs != nullptr) {
                const std::string name = text(*crs, 0);
                if (name.starts_with("EPSG:")) {
                    out_.coordinateSystem = name;
                }
                if (const StepInstance* unit = at(*crs, 6)) {
                    mapUnit = unitScale(unit);
                }
            }
            const double east = number(instance, 2).value_or(0.0);
            const double north = number(instance, 3).value_or(0.0);
            const double height = number(instance, 4).value_or(0.0);
            const double abscissa = number(instance, 5).value_or(1.0);
            const double ordinate = number(instance, 6).value_or(0.0);
            std::optional<double> scale = number(instance, 7);
            if (!scale && std::abs(length_ / mapUnit - 1.0) > 1e-12) {
                // GRF005: the scale is what takes the file's length unit to
                // the map's; a file that leaves it out while its units differ
                // is read as though it had said so.
                scale = length_ / mapUnit;
                warnOnce("the map conversion gives no scale although the file's length unit is "
                         "not the map's; the ratio of the two is used");
            }
            const double s = scale.value_or(1.0);
            const double sx =
                instance.type == "IFCMAPCONVERSIONSCALED" ? number(instance, 8).value_or(1.0) : 1.0;
            const double sy =
                instance.type == "IFCMAPCONVERSIONSCALED" ? number(instance, 9).value_or(1.0) : 1.0;
            const double sz = instance.type == "IFCMAPCONVERSIONSCALED"
                                  ? number(instance, 10).value_or(1.0)
                                  : 1.0;
            const double rotation = std::atan2(ordinate, abscissa);
            world_ = Mat4::scaling(Vec3(mapUnit, mapUnit, mapUnit)) *
                     Mat4::translation(Vec3(east, north, height)) *
                     Mat4::rotation(Vec3(0.0, 0.0, 1.0), rotation) *
                     Mat4::scaling(Vec3(s * sx, s * sy, s * sz));
            break;
        }
        for (const std::uint32_t id : m_.order) {
            const StepInstance& instance = *m_.find(id);
            if (instance.type != "IFCRIGIDOPERATION") {
                continue;
            }
            // IFC 4.3's translation to a target system. In lengths it is a
            // shift; in angles it places the file in a geographic system,
            // which only a projection can turn into grid coordinates - and
            // this layer has none (geodesy is above it), so the file is read
            // in its own engineering coordinates and that is said.
            const StepValue* first = arg(instance, 2);
            const StepValue* second = arg(instance, 3);
            const bool lengths = first && second && first->kind == StepValue::Kind::Typed &&
                                 second->kind == StepValue::Kind::Typed &&
                                 first->text == "IFCLENGTHMEASURE" &&
                                 second->text == "IFCLENGTHMEASURE";
            if (!lengths) {
                warnOnce(
                    "the file is placed in a geographic coordinate system (an IfcRigidOperation "
                    "in degrees); it is read in its own engineering coordinates");
                break;
            }
            double mapUnit = 1.0;
            if (const StepInstance* crs = at(instance, 1)) {
                const std::string name = text(*crs, 0);
                if (name.starts_with("EPSG:")) {
                    out_.coordinateSystem = name;
                }
                if (const StepInstance* unit = at(*crs, 6)) {
                    mapUnit = unitScale(unit);
                }
            }
            world_ = Mat4::scaling(Vec3(mapUnit, mapUnit, mapUnit)) *
                     Mat4::translation(Vec3(first->number().value_or(0.0),
                                            second->number().value_or(0.0),
                                            number(instance, 4).value_or(0.0))) *
                     Mat4::scaling(Vec3(length_ / mapUnit, length_ / mapUnit, length_ / mapUnit));
            break;
        }
        if (options_.originShift) {
            world_ =
                Mat4::translation(Vec3(-options_.originShift->x, -options_.originShift->y, 0.0)) *
                world_;
        }
    }

    static Vec3 cartesian(const StepInstance* point, int* dimensions = nullptr)
    {
        Vec3 out;
        int count = 0;
        if (point != nullptr && !point->arguments.empty() &&
            point->arguments.front().kind == StepValue::Kind::List) {
            const auto& items = point->arguments.front().items;
            count = static_cast<int>(items.size());
            if (count > 0) {
                out.x = items[0].number().value_or(0.0);
            }
            if (count > 1) {
                out.y = items[1].number().value_or(0.0);
            }
            if (count > 2) {
                out.z = items[2].number().value_or(0.0);
            }
        }
        if (dimensions != nullptr) {
            *dimensions = count;
        }
        return out;
    }

    static std::optional<Vec3> directionOf(const StepInstance* direction)
    {
        if (direction == nullptr) {
            return std::nullopt;
        }
        const Vec3 d = cartesian(direction);
        return d.length() > 0.0 ? std::optional(d.normalized()) : std::nullopt;
    }

    // An IfcAxis2Placement2D or 3D as the matrix taking its frame to its parent's.
    Mat4 axisPlacement(const StepInstance* placement) const
    {
        if (placement == nullptr) {
            return Mat4{};
        }
        const Vec3 location = cartesian(at(*placement, 0));
        Vec3 z(0.0, 0.0, 1.0);
        Vec3 x(1.0, 0.0, 0.0);
        if (placement->type == "IFCAXIS2PLACEMENT3D") {
            z = directionOf(at(*placement, 1)).value_or(z);
            x = directionOf(at(*placement, 2)).value_or(x);
        } else if (placement->type == "IFCAXIS2PLACEMENT2D") {
            x = directionOf(at(*placement, 1)).value_or(x);
            x.z = 0.0;
        }
        // The reference direction made perpendicular to the axis, as the
        // schema's function IfcBuildAxes does.
        Vec3 xAxis = x - z * x.dot(z);
        if (xAxis.length() < 1e-12) {
            xAxis = std::abs(z.x) < 0.9 ? Vec3(1.0, 0.0, 0.0) : Vec3(0.0, 1.0, 0.0);
            xAxis = (xAxis - z * xAxis.dot(z));
        }
        xAxis = xAxis.normalized();
        const Vec3 yAxis = z.cross(xAxis);
        return Mat4(xAxis.x, yAxis.x, z.x, location.x, xAxis.y, yAxis.y, z.y, location.y, xAxis.z,
                    yAxis.z, z.z, location.z, 0.0, 0.0, 0.0, 1.0);
    }

    // An object placement, all the way up, in the file's world frame.
    Mat4 placement(const StepInstance* object, int depth = 0)
    {
        if (object == nullptr || depth > kMaximumNesting) {
            return Mat4{};
        }
        if (object->type == "IFCLOCALPLACEMENT") {
            return placement(at(*object, 0), depth + 1) * axisPlacement(at(*object, 1));
        }
        if (object->type == "IFCLINEARPLACEMENT") {
            if (const StepInstance* position = at(*object, 2)) {
                return placement(at(*object, 0), depth + 1) * axisPlacement(position);
            }
            warnOnce("a linear placement without a CartesianPosition is read at its "
                     "parent's origin");
            return placement(at(*object, 0), depth + 1);
        }
        if (object->type == "IFCGRIDPLACEMENT") {
            return gridPlacement(*object, depth);
        }
        return Mat4{};
    }

    // Where two grid axes meet, each offset to its left by its offset
    // distance (the third distance is a height), in the grid's frame - the
    // definition of IfcVirtualGridIntersection. IFC 4.3 gave the placement a
    // PlacementRelTo of its own ahead of the location; IFC4 has two
    // attributes, so the location is found by position from the end.
    Mat4 gridPlacement(const StepInstance& object, int depth)
    {
        const bool relative = object.arguments.size() >= 3;
        const StepInstance* intersection = at(object, relative ? 1 : 0);
        if (!is(intersection, "IFCVIRTUALGRIDINTERSECTION")) {
            warnOnce("a grid placement without a grid intersection is read at the origin");
            return Mat4{};
        }
        const auto line = [&](std::uint32_t axisId) -> std::optional<std::pair<Vec3, Vec3>> {
            const StepInstance* axis = m_.find(axisId);
            if (axis == nullptr) {
                return std::nullopt;
            }
            const StepInstance* curve = at(*axis, 1);
            const auto points = curve != nullptr && curve->type == "IFCPOLYLINE"
                                    ? ids(*curve, 0)
                                    : std::vector<std::uint32_t>{};
            if (points.size() < 2) {
                return std::nullopt;
            }
            Vec3 a = cartesian(m_.find(points.front()));
            Vec3 b = cartesian(m_.find(points[1]));
            const StepValue* same = arg(*axis, 2);
            if (same && same->kind == StepValue::Kind::Enumeration && same->text == "F") {
                std::swap(a, b);
            }
            return std::pair(a, b);
        };
        const auto axes = ids(*intersection, 0);
        std::vector<double> offsets;
        if (const StepValue* list = arg(*intersection, 1);
            list && list->kind == StepValue::Kind::List) {
            for (const StepValue& value : list->items) {
                offsets.push_back(value.number().value_or(0.0));
            }
        }
        const auto first = axes.size() == 2 ? line(axes[0]) : std::nullopt;
        const auto second = axes.size() == 2 ? line(axes[1]) : std::nullopt;
        if (!first || !second) {
            warnOnce("a grid placement whose axes are not straight lines is read at the origin");
            return Mat4{};
        }
        const auto offset = [](std::pair<Vec3, Vec3> l, double d) {
            const Vec2 along = Vec2(l.second.x - l.first.x, l.second.y - l.first.y).normalized();
            const Vec3 left(-along.y * d, along.x * d, 0.0);
            return std::pair(l.first + left, l.second + left);
        };
        const auto a = offset(*first, offsets.size() > 0 ? offsets[0] : 0.0);
        const auto b = offset(*second, offsets.size() > 1 ? offsets[1] : 0.0);
        const Vec2 u(a.second.x - a.first.x, a.second.y - a.first.y);
        const Vec2 v(b.second.x - b.first.x, b.second.y - b.first.y);
        const double denominator = u.x * v.y - u.y * v.x;
        if (std::abs(denominator) < 1e-12) {
            warnOnce("a grid placement on two parallel axes is read at the origin");
            return Mat4{};
        }
        const Vec2 w(b.first.x - a.first.x, b.first.y - a.first.y);
        const double t = (w.x * v.y - w.y * v.x) / denominator;
        const Vec3 at(a.first.x + u.x * t, a.first.y + u.y * t,
                      offsets.size() > 2 ? offsets[2] : 0.0);
        // IFC 4.3 says what the intersection is relative to; before it, the
        // grid the first axis belongs to places it.
        Mat4 grid;
        const StepInstance* relativeTo = relative ? this->at(object, 0) : nullptr;
        if (relativeTo != nullptr) {
            grid = placement(relativeTo, depth + 1);
        }
        for (const std::uint32_t id :
             relativeTo != nullptr ? std::vector<std::uint32_t>{} : m_.order) {
            const StepInstance& candidate = *m_.find(id);
            if (candidate.type != "IFCGRID") {
                continue;
            }
            bool owns = false;
            for (const std::size_t index : {7u, 8u, 9u}) {
                const auto members = ids(candidate, index);
                owns = owns || std::find(members.begin(), members.end(), axes[0]) != members.end();
            }
            if (owns) {
                grid = placement(this->at(candidate, 5), depth + 1);
                break;
            }
        }
        // Oriented along the first axis, unless a direction is given.
        const Vec2 x = u.normalized();
        Vec3 xAxis(x.x, x.y, 0.0);
        if (const auto given = directionOf(this->at(object, relative ? 2 : 1))) {
            xAxis = *given;
        }
        const Vec3 yAxis = Vec3(0.0, 0.0, 1.0).cross(xAxis);
        return grid * Mat4(xAxis.x, yAxis.x, 0.0, at.x, xAxis.y, yAxis.y, 0.0, at.y, xAxis.z,
                           yAxis.z, 1.0, at.z, 0.0, 0.0, 0.0, 1.0);
    }

    Vec3 world(const Mat4& local, Vec3 point) const
    {
        return math::transformPoint(world_ * local, point);
    }

    // ---- relationships ------------------------------------------------------------------

    void index()
    {
        for (const std::uint32_t id : m_.order) {
            const StepInstance& r = *m_.find(id);
            const std::string& type = r.type;
            if (type == "IFCRELDEFINESBYPROPERTIES") {
                for (const std::uint32_t object : ids(r, 4)) {
                    for (const std::uint32_t set : ids(r, 5)) {
                        propertySets_[object].push_back(set);
                    }
                }
            } else if (type == "IFCRELDEFINESBYTYPE") {
                for (const std::uint32_t object : ids(r, 4)) {
                    for (const std::uint32_t typeId : ids(r, 5)) {
                        typeOf_[object] = typeId;
                    }
                }
            } else if (type == "IFCRELASSOCIATESCLASSIFICATION") {
                for (const std::uint32_t object : ids(r, 4)) {
                    for (const std::uint32_t reference : ids(r, 5)) {
                        classifications_[object].push_back(reference);
                    }
                }
            } else if (type == "IFCRELASSIGNSTOGROUP" || type == "IFCRELASSIGNSTOGROUPBYFACTOR") {
                for (const std::uint32_t object : ids(r, 4)) {
                    for (const std::uint32_t group : ids(r, 6)) {
                        groups_[object].push_back(group);
                    }
                }
            } else if (type == "IFCRELCONTAINEDINSPATIALSTRUCTURE") {
                for (const std::uint32_t object : ids(r, 4)) {
                    for (const std::uint32_t structure : ids(r, 5)) {
                        container_[object] = structure;
                    }
                }
            } else if (type == "IFCRELAGGREGATES") {
                for (const std::uint32_t part : ids(r, 5)) {
                    for (const std::uint32_t whole : ids(r, 4)) {
                        parent_[part] = whole;
                    }
                }
            } else if (type == "IFCRELNESTS") {
                for (const std::uint32_t whole : ids(r, 4)) {
                    auto& children = nested_[whole];
                    const auto parts = ids(r, 5);
                    children.insert(children.end(), parts.begin(), parts.end());
                }
            } else if (type == "IFCPRESENTATIONLAYERASSIGNMENT" ||
                       type == "IFCPRESENTATIONLAYERWITHSTYLE") {
                const std::string name = layerPath(text(r, 0));
                if (!name.empty()) {
                    for (const std::uint32_t item : ids(r, 2)) {
                        layerOf_.emplace(item, name);
                    }
                }
            } else if (type == "IFCSTYLEDITEM") {
                if (const auto colour = styleColour(r, 1, 0)) {
                    for (const std::uint32_t item : ids(r, 0)) {
                        colourOf_.emplace(item, *colour);
                    }
                }
            }
        }
    }

    std::optional<entity::Color> colourOf(const StepInstance* colour) const
    {
        if (colour == nullptr || colour->type != "IFCCOLOURRGB") {
            return std::nullopt;
        }
        const auto channel = [&](std::size_t index) {
            const double value = std::clamp(number(*colour, index).value_or(0.0), 0.0, 1.0);
            return static_cast<std::uint8_t>(std::lround(value * 255.0));
        };
        return entity::Color{channel(1), channel(2), channel(3), 255};
    }

    // The colour a style (or a list of them) gives.
    std::optional<entity::Color> styleColour(const StepInstance& owner, std::size_t index,
                                             int depth) const
    {
        if (depth > 4) {
            return std::nullopt;
        }
        for (const std::uint32_t id : ids(owner, index)) {
            const StepInstance* style = m_.find(id);
            if (style == nullptr) {
                continue;
            }
            if (style->type == "IFCCURVESTYLE") {
                if (auto colour = colourOf(at(*style, 3))) {
                    return colour;
                }
            } else if (style->type == "IFCSURFACESTYLE") {
                for (const std::uint32_t element : ids(*style, 2)) {
                    const StepInstance* shading = m_.find(element);
                    if (shading != nullptr && (shading->type == "IFCSURFACESTYLESHADING" ||
                                               shading->type == "IFCSURFACESTYLERENDERING")) {
                        if (auto colour = colourOf(at(*shading, 0))) {
                            return colour;
                        }
                    }
                }
            } else if (style->type == "IFCPRESENTATIONSTYLEASSIGNMENT") {
                if (auto colour = styleColour(*style, 0, depth + 1)) {
                    return colour;
                }
            }
        }
        return std::nullopt;
    }

    // ---- properties -----------------------------------------------------------------------

    std::optional<entity::PropertyValue> propertyValue(const StepValue& value) const
    {
        if (value.kind == StepValue::Kind::Typed && !value.items.empty()) {
            const StepValue& inner = value.items.front();
            const std::string& type = value.text;
            if (type == "IFCBOOLEAN" || type == "IFCLOGICAL") {
                if (inner.kind == StepValue::Kind::Enumeration) {
                    if (inner.text == "T" || inner.text == "TRUE") {
                        return entity::PropertyValue{true};
                    }
                    if (inner.text == "F" || inner.text == "FALSE") {
                        return entity::PropertyValue{false};
                    }
                    return entity::PropertyValue{std::string("UNKNOWN")};
                }
            }
            if (inner.kind == StepValue::Kind::Integer &&
                (type == "IFCINTEGER" || type == "IFCCOUNTMEASURE" ||
                 type == "IFCPOSITIVEINTEGER" || type == "IFCTIMESTAMP")) {
                return entity::PropertyValue{inner.integer};
            }
            if (const auto n = inner.number()) {
                double scale = 1.0;
                if (type == "IFCLENGTHMEASURE" || type == "IFCPOSITIVELENGTHMEASURE" ||
                    type == "IFCNONNEGATIVELENGTHMEASURE") {
                    scale = length_;
                } else if (type == "IFCAREAMEASURE") {
                    scale = area_;
                } else if (type == "IFCVOLUMEMEASURE") {
                    scale = volume_;
                } else if (type == "IFCPLANEANGLEMEASURE" ||
                           type == "IFCPOSITIVEPLANEANGLEMEASURE") {
                    scale = angle_;
                }
                return entity::PropertyValue{*n * scale};
            }
            return propertyValue(inner);
        }
        if (value.kind == StepValue::Kind::String || value.kind == StepValue::Kind::Enumeration) {
            return entity::PropertyValue{value.text};
        }
        if (value.kind == StepValue::Kind::Integer) {
            return entity::PropertyValue{value.integer};
        }
        if (value.kind == StepValue::Kind::Real) {
            return entity::PropertyValue{value.real};
        }
        return std::nullopt;
    }

    static std::string joined(const std::vector<std::string>& parts)
    {
        std::string out;
        for (const std::string& part : parts) {
            out += out.empty() ? part : ", " + part;
        }
        return out;
    }

    void addProperty(const StepInstance& property, const std::string& prefix,
                     entity::PropertyMap& into, int depth = 0) const
    {
        const std::string name = text(property, 0);
        if (name.empty() || depth > 4) {
            return;
        }
        const std::string key = prefix.empty() ? name : prefix + "/" + name;
        const std::string& type = property.type;
        if (type == "IFCPROPERTYSINGLEVALUE") {
            if (const StepValue* value = arg(property, 2)) {
                if (auto converted = propertyValue(*value)) {
                    into.insert_or_assign(key, std::move(*converted));
                }
            }
        } else if (type == "IFCPROPERTYENUMERATEDVALUE" || type == "IFCPROPERTYLISTVALUE") {
            std::vector<std::string> values;
            if (const StepValue* list = arg(property, 2);
                list && list->kind == StepValue::Kind::List) {
                for (const StepValue& item : list->items) {
                    if (auto converted = propertyValue(item)) {
                        values.push_back(entity::toString(*converted));
                    }
                }
            }
            if (values.size() == 1) {
                into.insert_or_assign(key, entity::PropertyValue{values.front()});
            } else if (!values.empty()) {
                into.insert_or_assign(key, entity::PropertyValue{joined(values)});
            }
        } else if (type == "IFCPROPERTYBOUNDEDVALUE") {
            std::string lower;
            std::string upper;
            if (const StepValue* value = arg(property, 3); value) {
                if (auto converted = propertyValue(*value)) {
                    lower = entity::toString(*converted);
                }
            }
            if (const StepValue* value = arg(property, 2); value) {
                if (auto converted = propertyValue(*value)) {
                    upper = entity::toString(*converted);
                }
            }
            if (!lower.empty() || !upper.empty()) {
                into.insert_or_assign(key, entity::PropertyValue{lower + ".." + upper});
            }
        } else if (type == "IFCCOMPLEXPROPERTY") {
            for (const std::uint32_t id : ids(property, 3)) {
                if (const StepInstance* part = m_.find(id)) {
                    addProperty(*part, key, into, depth + 1);
                }
            }
        }
    }

    // What the product's property sets and quantities say, and its
    // provenance where Katana wrote it.
    void addPropertySet(const StepInstance& set, entity::Entity& entity) const
    {
        const std::string name = text(set, 2);
        if (set.type == "IFCPROPERTYSET") {
            if (name == "Katana_Attributes") {
                for (const std::uint32_t id : ids(set, 4)) {
                    if (const StepInstance* property = m_.find(id)) {
                        addProperty(*property, {}, entity.properties);
                    }
                }
                return;
            }
            if (name == "Katana_Provenance") {
                entity::PropertyMap provenance;
                for (const std::uint32_t id : ids(set, 4)) {
                    if (const StepInstance* property = m_.find(id)) {
                        addProperty(*property, {}, provenance);
                    }
                }
                for (auto& [key, value] : provenance) {
                    if (key != "EntityId" && key != "Layer" && key != "Style" &&
                        key != "ClassifiedBy") {
                        entity.metadata.insert_or_assign(key, value);
                    }
                }
                return;
            }
            for (const std::uint32_t id : ids(set, 4)) {
                if (const StepInstance* property = m_.find(id)) {
                    addProperty(*property, name, entity.properties);
                }
            }
        } else if (set.type == "IFCELEMENTQUANTITY") {
            for (const std::uint32_t id : ids(set, 5)) {
                const StepInstance* quantity = m_.find(id);
                if (quantity == nullptr) {
                    continue;
                }
                const auto value = number(*quantity, 3);
                const std::string quantityName = text(*quantity, 0);
                if (!value || quantityName.empty()) {
                    continue;
                }
                double scale = 1.0;
                if (quantity->type == "IFCQUANTITYLENGTH") {
                    scale = length_;
                } else if (quantity->type == "IFCQUANTITYAREA") {
                    scale = area_;
                } else if (quantity->type == "IFCQUANTITYVOLUME") {
                    scale = volume_;
                }
                entity.properties.insert_or_assign(name + "/" + quantityName, *value * scale);
            }
        }
    }

    // The classification system a reference belongs to, up its chain.
    std::string classificationSystem(const StepInstance* reference, int depth = 0) const
    {
        if (reference == nullptr || depth > 8) {
            return "Classification";
        }
        const StepInstance* source = at(*reference, 3);
        if (source == nullptr) {
            return "Classification";
        }
        if (source->type == "IFCCLASSIFICATION") {
            const std::string name = text(*source, 3);
            return name.empty() ? "Classification" : name;
        }
        return classificationSystem(source, depth + 1);
    }

    void describe(std::uint32_t id, const StepInstance& product, const ProductClass& ifcClass,
                  entity::Entity& entity) const
    {
        const auto meta = [&](std::string_view key, const std::string& value) {
            if (!value.empty()) {
                entity.metadata.insert_or_assign(std::string(key), value);
            }
        };
        meta("ifc.class", std::string(ifcClass.name));
        meta("ifc.globalId", text(product, 0));
        meta("ifc.name", text(product, 2));
        meta("ifc.description", text(product, 3));
        meta("ifc.objectType", text(product, 4));
        if (const StepValue* tag = arg(product, 7); tag && tag->kind == StepValue::Kind::String) {
            meta("ifc.tag", tag->text);
        }
        for (const std::size_t index : {8u, 7u}) {
            if (const StepValue* value = arg(product, index);
                value && value->kind == StepValue::Kind::Enumeration) {
                meta("ifc.predefinedType", value->text);
                break;
            }
        }
        meta("source", options_.sourceName);
        if (const auto container = containerOf(id)) {
            meta("ifc.container", text(*m_.find(*container), 2));
        }

        // The type's sets first, the occurrence's over them.
        if (const auto type = typeOf_.find(id); type != typeOf_.end()) {
            if (const StepInstance* typeObject = m_.find(type->second)) {
                for (const std::uint32_t set : ids(*typeObject, 5)) {
                    if (const StepInstance* definition = m_.find(set)) {
                        addPropertySet(*definition, entity);
                    }
                }
            }
        }
        if (const auto sets = propertySets_.find(id); sets != propertySets_.end()) {
            for (const std::uint32_t set : sets->second) {
                if (const StepInstance* definition = m_.find(set)) {
                    addPropertySet(*definition, entity);
                }
            }
        }
        if (const auto references = classifications_.find(id);
            references != classifications_.end()) {
            std::map<std::string, std::vector<std::string>> bySystem;
            for (const std::uint32_t reference : references->second) {
                const StepInstance* instance = m_.find(reference);
                if (instance == nullptr) {
                    continue;
                }
                std::string identification = text(*instance, 1);
                if (identification.empty()) {
                    identification = text(*instance, 2);
                }
                if (!identification.empty()) {
                    bySystem[classificationSystem(instance)].push_back(identification);
                }
            }
            for (const auto& [system, values] : bySystem) {
                entity.properties.insert_or_assign("Classification/" + system,
                                                   entity::PropertyValue{joined(values)});
            }
        }
        if (const auto groups = groups_.find(id); groups != groups_.end()) {
            std::vector<std::string> names;
            for (const std::uint32_t group : groups->second) {
                if (const StepInstance* instance = m_.find(group)) {
                    const std::string name = text(*instance, 2);
                    if (!name.empty()) {
                        names.push_back(name);
                    }
                }
            }
            if (!names.empty()) {
                entity.properties.insert_or_assign("System", entity::PropertyValue{joined(names)});
            }
        }
    }

    // The spatial element holding `id`, directly or through what it is part of.
    std::optional<std::uint32_t> containerOf(std::uint32_t id) const
    {
        for (int depth = 0; depth < kMaximumNesting; ++depth) {
            if (const auto found = container_.find(id); found != container_.end()) {
                return found->second;
            }
            const auto part = parent_.find(id);
            if (part == parent_.end()) {
                return std::nullopt;
            }
            id = part->second;
        }
        return std::nullopt;
    }

    // ---- geometry ----------------------------------------------------------------------------

    // Points of a circle's arc from `from` to `to` (radians, in its own frame),
    // chorded to the tolerance.
    // Where a trimmed circle starts and ends, in radians about its own
    // placement, `to` beyond `from` in the direction of the sense, so that
    // `to - from` is the signed sweep. A trim is a parameter value - an
    // angle, in the file's angle unit - or a point on the circle, whichever
    // the file gives; with both, the one MasterRepresentation prefers, the
    // parameter when it says neither. nullopt when either end is neither.
    // `stretch` is an ellipse's SemiAxis1 / SemiAxis2: a trimming point in
    // its placement's frame is at eccentric anomaly atan2(y a / b, x), which
    // for a circle (stretch 1) is its polar angle.
    std::optional<std::pair<double, double>> circleTrim(const StepInstance& item,
                                                        const StepInstance& circle,
                                                        double stretch = 1.0) const
    {
        const auto inverse = axisPlacement(at(circle, 0)).inverse();
        const StepValue* master = arg(item, 4);
        const bool preferPoint =
            master && master->kind == StepValue::Kind::Enumeration && master->text == "CARTESIAN";
        const auto trim = [&](std::size_t index) -> std::optional<double> {
            const StepValue* list = arg(item, index);
            if (list == nullptr || list->kind != StepValue::Kind::List) {
                return std::nullopt;
            }
            std::optional<double> parameter;
            std::optional<double> point;
            for (const StepValue& value : list->items) {
                if (value.kind == StepValue::Kind::Typed && value.text == "IFCPARAMETERVALUE") {
                    if (const auto number = value.number()) {
                        parameter = *number * angle_;
                    }
                } else if (value.kind == StepValue::Kind::Reference && inverse) {
                    const StepInstance* on = m_.find(value.reference);
                    if (is(on, "IFCCARTESIANPOINT")) {
                        const Vec3 local = math::transformPoint(*inverse, cartesian(on));
                        point = std::atan2(local.y * stretch, local.x);
                    }
                }
            }
            if (preferPoint && point) {
                return point;
            }
            return parameter ? parameter : point;
        };
        const auto from = trim(1);
        auto to = trim(2);
        if (!from || !to) {
            return std::nullopt;
        }
        const StepValue* sense = arg(item, 3);
        const bool forward =
            !(sense && sense->kind == StepValue::Kind::Enumeration && sense->text == "F");
        if (forward && *to < *from) {
            *to += math::kTwoPi;
        } else if (!forward && *to > *from) {
            *to -= math::kTwoPi;
        }
        return std::pair(*from, *to);
    }

    std::vector<Vec3> circlePoints(const Mat4& frame, double radius, double from, double to) const
    {
        const double sweep = to - from;
        const double scale = math::transformVector(world_ * frame, Vec3(1.0, 0.0, 0.0)).length();
        const double r = radius * scale;
        std::size_t count = 8;
        if (r > options_.curveTolerance) {
            const double step = 2.0 * std::acos(std::max(-1.0, 1.0 - options_.curveTolerance / r));
            count = std::max<std::size_t>(
                2, static_cast<std::size_t>(std::ceil(std::abs(sweep) / step)));
        }
        count = std::min<std::size_t>(count, 20000);
        std::vector<Vec3> points;
        for (std::size_t i = 0; i <= count; ++i) {
            const double a = from + sweep * static_cast<double>(i) / static_cast<double>(count);
            points.push_back(world(frame, Vec3(radius * std::cos(a), radius * std::sin(a), 0.0)));
        }
        return points;
    }

    // A run of points as a Line or Polyline shape.
    static void addRun(std::vector<Vec3> points, bool in3d, std::uint32_t item,
                       std::vector<Shape>& out)
    {
        std::vector<Vec3> kept;
        for (const Vec3& p : points) {
            if (kept.empty() || (Vec2(p.x, p.y) - Vec2(kept.back().x, kept.back().y)).length() >
                                    math::tolerance::kGeometric) {
                kept.push_back(p);
            }
        }
        bool closed = false;
        if (kept.size() > 2 &&
            (Vec2(kept.front().x, kept.front().y) - Vec2(kept.back().x, kept.back().y)).length() <=
                math::tolerance::kGeometric) {
            kept.pop_back();
            closed = true;
        }
        if (kept.size() < 2) {
            return;
        }
        Shape shape;
        shape.item = item;
        if (kept.size() == 2 && !closed) {
            shape.geometry =
                geometry::Segment2{Vec2(kept[0].x, kept[0].y), Vec2(kept[1].x, kept[1].y)};
        } else {
            geometry::Polyline2 polyline;
            polyline.closed = closed;
            for (const Vec3& p : kept) {
                polyline.vertices.emplace_back(p.x, p.y);
            }
            shape.geometry = std::move(polyline);
        }
        if (in3d) {
            for (const Vec3& p : kept) {
                shape.heights.emplace_back(p.z);
            }
        }
        out.push_back(std::move(shape));
    }

    // The points of a curve item, for curves that are only runs of points.
    bool curvePoints(const StepInstance& item, const Mat4& frame, std::vector<Vec3>& points,
                     bool& in3d, int depth)
    {
        if (depth > kMaximumNesting) {
            return false;
        }
        const std::string& type = item.type;
        if (type == "IFCPOLYLINE") {
            for (const std::uint32_t id : ids(item, 0)) {
                int dims = 0;
                const Vec3 p = cartesian(m_.find(id), &dims);
                in3d = in3d && dims == 3;
                points.push_back(world(frame, p));
            }
            return true;
        }
        if (type == "IFCINDEXEDPOLYCURVE") {
            const StepInstance* list = at(item, 0);
            if (list == nullptr || list->arguments.empty() ||
                list->arguments.front().kind != StepValue::Kind::List) {
                return false;
            }
            in3d = in3d && list->type == "IFCCARTESIANPOINTLIST3D";
            std::vector<Vec3> coordinates;
            for (const StepValue& row : list->arguments.front().items) {
                Vec3 p;
                if (row.kind == StepValue::Kind::List) {
                    if (!row.items.empty())
                        p.x = row.items[0].number().value_or(0.0);
                    if (row.items.size() > 1)
                        p.y = row.items[1].number().value_or(0.0);
                    if (row.items.size() > 2)
                        p.z = row.items[2].number().value_or(0.0);
                }
                coordinates.push_back(p);
            }
            const auto point = [&](const StepValue& index) -> std::optional<Vec3> {
                const auto n = index.number();
                if (!n || *n < 1 || *n > static_cast<double>(coordinates.size())) {
                    return std::nullopt;
                }
                return coordinates[static_cast<std::size_t>(*n) - 1];
            };
            const StepValue* segments = arg(item, 1);
            if (segments == nullptr || segments->kind != StepValue::Kind::List) {
                for (const Vec3& p : coordinates) {
                    points.push_back(world(frame, p));
                }
                return true;
            }
            for (const StepValue& segment : segments->items) {
                if (segment.kind != StepValue::Kind::Typed || segment.items.empty() ||
                    segment.items.front().kind != StepValue::Kind::List) {
                    continue;
                }
                const auto& indices = segment.items.front().items;
                if (segment.text == "IFCARCINDEX" && indices.size() == 3) {
                    const auto a = point(indices[0]);
                    const auto b = point(indices[1]);
                    const auto c = point(indices[2]);
                    if (!a || !b || !c) {
                        continue;
                    }
                    const auto arc = geometry::Arc2::throughPoints(
                        Vec2(a->x, a->y), Vec2(b->x, b->y), Vec2(c->x, c->y));
                    if (!arc) {
                        points.push_back(world(frame, *a));
                        points.push_back(world(frame, *c));
                        continue;
                    }
                    // In the arc's own plane, with the height moving evenly
                    // from its start to its end.
                    const double scale =
                        math::transformVector(world_ * frame, Vec3(1.0, 0.0, 0.0)).length();
                    const double r = arc->radius * scale;
                    std::size_t count = 8;
                    if (r > options_.curveTolerance) {
                        const double step =
                            2.0 * std::acos(std::max(-1.0, 1.0 - options_.curveTolerance / r));
                        count = std::max<std::size_t>(
                            2, static_cast<std::size_t>(std::ceil(std::abs(arc->sweep) / step)));
                    }
                    count = std::min<std::size_t>(count, 20000);
                    for (std::size_t i = 0; i <= count; ++i) {
                        const double t = static_cast<double>(i) / static_cast<double>(count);
                        const Vec2 p = arc->pointAt(t);
                        points.push_back(world(frame, Vec3(p.x, p.y, a->z + (c->z - a->z) * t)));
                    }
                } else {
                    for (const StepValue& index : indices) {
                        if (const auto p = point(index)) {
                            points.push_back(world(frame, *p));
                        }
                    }
                }
            }
            return true;
        }
        if (type == "IFCCIRCLE" || type == "IFCELLIPSE") {
            const Mat4 local = frame * axisPlacement(at(item, 0));
            const double radius = number(item, 1).value_or(0.0);
            if (type == "IFCELLIPSE") {
                const double semi2 = number(item, 2).value_or(radius);
                const Mat4 squash =
                    local * Mat4::scaling(Vec3(1.0, radius > 0.0 ? semi2 / radius : 1.0, 1.0));
                const auto circle = circlePoints(squash, radius, 0.0, math::kTwoPi);
                points.insert(points.end(), circle.begin(), circle.end());
            } else {
                const auto circle = circlePoints(local, radius, 0.0, math::kTwoPi);
                points.insert(points.end(), circle.begin(), circle.end());
            }
            in3d = in3d && at(item, 0) != nullptr && at(item, 0)->type == "IFCAXIS2PLACEMENT3D";
            return true;
        }
        if (type == "IFCTRIMMEDCURVE") {
            const StepInstance* basis = at(item, 0);
            if (basis == nullptr) {
                return false;
            }
            const auto trim = [&](std::size_t index) -> std::optional<double> {
                const StepValue* list = arg(item, index);
                if (list == nullptr || list->kind != StepValue::Kind::List) {
                    return std::nullopt;
                }
                for (const StepValue& value : list->items) {
                    if (value.kind == StepValue::Kind::Typed && value.text == "IFCPARAMETERVALUE") {
                        return value.number();
                    }
                }
                return std::nullopt;
            };
            if (basis->type == "IFCCIRCLE") {
                const auto ends = circleTrim(item, *basis);
                if (!ends) {
                    return false;
                }
                const Mat4 local = frame * axisPlacement(at(*basis, 0));
                const auto arc =
                    circlePoints(local, number(*basis, 1).value_or(0.0), ends->first, ends->second);
                points.insert(points.end(), arc.begin(), arc.end());
                in3d = in3d && at(*basis, 0) != nullptr &&
                       at(*basis, 0)->type == "IFCAXIS2PLACEMENT3D";
                return true;
            }
            if (basis->type == "IFCELLIPSE") {
                // As a circle's arc, in the ellipse's frame squashed along its
                // second axis: IFC's conic parameter is the eccentric anomaly.
                // Read as the whole ellipse, an elliptical arc came back closed.
                const double semi1 = number(*basis, 1).value_or(0.0);
                const double semi2 = number(*basis, 2).value_or(semi1);
                if (!(semi1 > 0.0) || !(semi2 > 0.0)) {
                    return false;
                }
                const auto ends = circleTrim(item, *basis, semi1 / semi2);
                if (!ends) {
                    return false;
                }
                const Mat4 squash = frame * axisPlacement(at(*basis, 0)) *
                                    Mat4::scaling(Vec3(1.0, semi2 / semi1, 1.0));
                const auto arc = circlePoints(squash, semi1, ends->first, ends->second);
                points.insert(points.end(), arc.begin(), arc.end());
                in3d = in3d && at(*basis, 0) != nullptr &&
                       at(*basis, 0)->type == "IFCAXIS2PLACEMENT3D";
                return true;
            }
            const auto t1 = trim(1);
            const auto t2 = trim(2);
            if (basis->type == "IFCLINE" && t1 && t2) {
                int dims = 0;
                const Vec3 origin = cartesian(at(*basis, 0), &dims);
                const StepInstance* vector = at(*basis, 1);
                if (vector == nullptr) {
                    return false;
                }
                const Vec3 direction = directionOf(at(*vector, 0)).value_or(Vec3(1.0, 0.0, 0.0)) *
                                       number(*vector, 1).value_or(1.0);
                points.push_back(world(frame, origin + direction * *t1));
                points.push_back(world(frame, origin + direction * *t2));
                in3d = in3d && dims == 3;
                return true;
            }
            return curvePoints(*basis, frame, points, in3d, depth + 1);
        }
        if (type == "IFCBSPLINECURVEWITHKNOTS" || type == "IFCRATIONALBSPLINECURVEWITHKNOTS") {
            // Degree, control points, the knots as distinct values with their
            // multiplicities, and a rational one's weights: a Spline2, chorded
            // within the import's curve tolerance, in plan (Katana's spline is
            // two-dimensional). Unread, it once came in as a point at 0,0.
            geometry::Spline2 spline;
            spline.degree = static_cast<int>(number(item, 0).value_or(0.0));
            for (const std::uint32_t id : ids(item, 1)) {
                const Vec3 p = world(frame, cartesian(m_.find(id)));
                spline.controlPoints.push_back(geometry::Point2(p.x, p.y));
            }
            const StepValue* multiplicities = arg(item, 5);
            const StepValue* knots = arg(item, 6);
            if (multiplicities == nullptr || knots == nullptr ||
                multiplicities->kind != StepValue::Kind::List ||
                knots->kind != StepValue::Kind::List ||
                multiplicities->items.size() != knots->items.size()) {
                return false;
            }
            for (std::size_t i = 0; i < knots->items.size(); ++i) {
                const auto count = multiplicities->items[i].number();
                const auto knot = knots->items[i].number();
                if (!count || !knot || *count < 1.0 || *count > 64.0) {
                    return false;
                }
                spline.knots.insert(spline.knots.end(), static_cast<std::size_t>(*count), *knot);
            }
            if (type == "IFCRATIONALBSPLINECURVEWITHKNOTS") {
                if (const StepValue* weights = arg(item, 8);
                    weights != nullptr && weights->kind == StepValue::Kind::List) {
                    for (const StepValue& weight : weights->items) {
                        spline.weights.push_back(weight.number().value_or(0.0));
                    }
                }
            }
            if (!spline.checkStructure()) {
                return false;
            }
            // The control points are in the world already (a B-spline is
            // the same curve of its points moved by any affine map), so the
            // tolerance is the world's, as an arc's is above.
            for (const geometry::Point2& p : spline.toPolyline(options_.curveTolerance).vertices) {
                points.push_back(Vec3(p.x, p.y, 0.0));
            }
            in3d = false;
            return true;
        }
        if (type == "IFCCOMPOSITECURVE" || type == "IFCCOMPOSITECURVEONSURFACE") {
            for (const std::uint32_t id : ids(item, 0)) {
                const StepInstance* segment = m_.find(id);
                if (segment == nullptr) {
                    continue;
                }
                if (segment->type == "IFCCOMPOSITECURVESEGMENT" ||
                    segment->type == "IFCREPARAMETRISEDCOMPOSITECURVESEGMENT") {
                    std::vector<Vec3> part;
                    if (const StepInstance* parent = at(*segment, 2);
                        parent && curvePoints(*parent, frame, part, in3d, depth + 1)) {
                        const StepValue* same = arg(*segment, 1);
                        if (same && same->kind == StepValue::Kind::Enumeration &&
                            same->text == "F") {
                            std::reverse(part.begin(), part.end());
                        }
                        points.insert(points.end(), part.begin(), part.end());
                    }
                } else if (segment->type == "IFCCURVESEGMENT") {
                    curveSegmentPoints(*segment, frame, points);
                    in3d = false;
                }
            }
            return true;
        }
        if (type == "IFCGRADIENTCURVE" || type == "IFCSEGMENTEDREFERENCECURVE") {
            warnOnce("a gradient curve outside an alignment is read in plan, from its base curve");
            if (const StepInstance* base = at(item, 2)) {
                return curvePoints(*base, frame, points, in3d, depth + 1);
            }
            return false;
        }
        return false;
    }

    // An IFC 4.3 IfcCurveSegment in plan: its parent curve from SegmentStart
    // for SegmentLength, moved so that it starts at its placement in the
    // placement's direction (the reading IfcOpenShell's kernel makes).
    void curveSegmentPoints(const StepInstance& segment, const Mat4& frame,
                            std::vector<Vec3>& points)
    {
        const StepInstance* parent = at(segment, 4);
        const auto start = number(segment, 2).value_or(0.0);
        const auto length = number(segment, 3).value_or(0.0);
        if (parent == nullptr || length == 0.0) {
            return;
        }
        double k0 = 0.0;
        double k1 = 0.0;
        if (parent->type == "IFCCIRCLE") {
            const double radius = number(*parent, 1).value_or(0.0);
            k0 = k1 = radius > 0.0 ? (length > 0.0 ? 1.0 : -1.0) / radius : 0.0;
        } else if (parent->type == "IFCCLOTHOID") {
            const double a = number(*parent, 1).value_or(0.0);
            if (a != 0.0) {
                k0 = start / (a * std::abs(a));
                k1 = (start + length) / (a * std::abs(a));
            }
        } else if (parent->type != "IFCLINE") {
            warnOnce("a curve segment on an " + parent->type + " is read as a straight");
        }
        const geometry::Spiral2 shape{Vec2(0.0, 0.0), 0.0, k0, k1, std::abs(length)};
        const Mat4 local = frame * axisPlacement(at(segment, 1));
        const std::size_t count =
            (k0 == 0.0 && k1 == 0.0)
                ? 1
                : std::min<std::size_t>(shape.chordCountFor(options_.curveTolerance), 20000);
        for (std::size_t i = 0; i <= count; ++i) {
            const Vec2 p =
                shape.pointAt(shape.length * static_cast<double>(i) / static_cast<double>(count));
            points.push_back(world(local, Vec3(p.x, p.y, 0.0)));
        }
    }

    void shapes(const StepInstance& item, std::uint32_t id, const Mat4& frame,
                std::vector<Shape>& out, int depth)
    {
        if (depth > kMaximumNesting) {
            warnOnce("geometry nested more than " + std::to_string(kMaximumNesting) +
                     " deep is not read");
            return;
        }
        const std::string& type = item.type;
        if (type == "IFCCARTESIANPOINT") {
            int dims = 0;
            const Vec3 p = world(frame, cartesian(&item, &dims));
            Shape shape;
            shape.item = id;
            shape.geometry = entity::PointGeometry{Vec2(p.x, p.y)};
            if (dims == 3) {
                shape.heights.emplace_back(p.z);
            }
            out.push_back(std::move(shape));
            return;
        }
        if (type == "IFCCIRCLE" || (type == "IFCTRIMMEDCURVE" && is(at(item, 0), "IFCCIRCLE"))) {
            // A horizontal circle or arc keeps its exact form.
            const StepInstance& circle = type == "IFCCIRCLE" ? item : *at(item, 0);
            const Mat4 local = world_ * frame * axisPlacement(at(circle, 0));
            const Vec3 axis = math::transformVector(local, Vec3(0.0, 0.0, 1.0));
            const Vec3 xAxis = math::transformVector(local, Vec3(1.0, 0.0, 0.0));
            const bool flat = axis.length() > 0.0 && std::abs(axis.normalized().z) > 1.0 - 1e-9;
            const bool threeD = is(at(circle, 0), "IFCAXIS2PLACEMENT3D");
            if (flat && type == "IFCCIRCLE") {
                const Vec3 centre = math::transformPoint(local, Vec3());
                Shape shape;
                shape.item = id;
                shape.geometry = geometry::Circle2{
                    Vec2(centre.x, centre.y), number(circle, 1).value_or(0.0) * xAxis.length()};
                if (threeD) {
                    shape.heights.emplace_back(centre.z);
                }
                out.push_back(std::move(shape));
                return;
            }
            if (flat) {
                if (const auto ends = circleTrim(item, circle)) {
                    // Seen from above, a placement whose z axis points down
                    // turns the other way.
                    const double hand = axis.z > 0.0 ? 1.0 : -1.0;
                    const Vec3 centre = math::transformPoint(local, Vec3());
                    Shape shape;
                    shape.item = id;
                    shape.geometry = geometry::Arc2{
                        Vec2(centre.x, centre.y), number(circle, 1).value_or(0.0) * xAxis.length(),
                        std::atan2(xAxis.y, xAxis.x) + hand * ends->first,
                        hand * (ends->second - ends->first)};
                    if (threeD) {
                        shape.heights.emplace_back(centre.z);
                    }
                    out.push_back(std::move(shape));
                    return;
                }
            }
        }
        if (type == "IFCTEXTLITERAL" || type == "IFCTEXTLITERALWITHEXTENT") {
            const Mat4 local = world_ * frame * axisPlacement(at(item, 1));
            const Vec3 origin = math::transformPoint(local, Vec3());
            const Vec3 xAxis = math::transformVector(local, Vec3(1.0, 0.0, 0.0));
            entity::TextGeometry text;
            text.text = Reader::text(item, 0);
            text.position = Vec2(origin.x, origin.y);
            text.rotation = std::atan2(xAxis.y, xAxis.x);
            if (type == "IFCTEXTLITERALWITHEXTENT") {
                const std::size_t lines =
                    static_cast<std::size_t>(std::count(text.text.begin(), text.text.end(), '\n')) +
                    1;
                if (const StepInstance* extent = at(item, 3)) {
                    const double height = number(*extent, 1).value_or(0.0) * xAxis.length() /
                                          static_cast<double>(lines);
                    if (height > 0.0) {
                        text.height = height;
                    }
                }
                text.justify = justification(Reader::text(item, 4));
            }
            if (text.text.empty()) {
                return;
            }
            Shape shape;
            shape.item = id;
            shape.geometry = std::move(text);
            out.push_back(std::move(shape));
            return;
        }
        if (type == "IFCGEOMETRICCURVESET" || type == "IFCGEOMETRICSET") {
            for (const std::uint32_t element : ids(item, 0)) {
                if (const StepInstance* child = m_.find(element)) {
                    shapes(*child, element, frame, out, depth + 1);
                }
            }
            return;
        }
        if (type == "IFCANNOTATIONFILLAREA") {
            for (const std::size_t index : {0u, 1u}) {
                for (const std::uint32_t boundary : ids(item, index)) {
                    if (const StepInstance* child = m_.find(boundary)) {
                        shapes(*child, boundary, frame, out, depth + 1);
                    }
                }
            }
            return;
        }
        if (type == "IFCMAPPEDITEM") {
            const StepInstance* map = at(item, 0);
            const StepInstance* target = at(item, 1);
            if (map == nullptr) {
                return;
            }
            const auto origin = axisPlacement(at(*map, 0)).inverse();
            const Mat4 mapped = frame * transformationOperator(target) * origin.value_or(Mat4{});
            if (const StepInstance* representation = at(*map, 1)) {
                for (const std::uint32_t child : ids(*representation, 3)) {
                    if (const StepInstance* instance = m_.find(child)) {
                        shapes(*instance, child, mapped, out, depth + 1);
                    }
                }
            }
            return;
        }
        if (type == "IFCSWEPTDISKSOLID" || type == "IFCSWEPTDISKSOLIDPOLYGONAL") {
            const std::size_t before = out.size();
            if (const StepInstance* directrix = at(item, 0)) {
                shapes(*directrix, ids(item, 0).empty() ? id : ids(item, 0).front(), frame, out,
                       depth + 1);
            }
            const double scale =
                math::transformVector(world_ * frame, Vec3(1.0, 0.0, 0.0)).length();
            for (std::size_t i = before; i < out.size(); ++i) {
                out[i].sweptRadius = number(item, 1).value_or(0.0) * scale;
                out[i].item = id;
            }
            return;
        }
        std::vector<Vec3> points;
        bool in3d = true;
        if (curvePoints(item, frame, points, in3d, depth + 1)) {
            addRun(std::move(points), in3d, id, out);
        }
    }

    Mat4 transformationOperator(const StepInstance* op) const
    {
        if (op == nullptr) {
            return Mat4{};
        }
        const bool threeD = op->type.starts_with("IFCCARTESIANTRANSFORMATIONOPERATOR3D");
        Vec3 x = directionOf(at(*op, 0)).value_or(Vec3(1.0, 0.0, 0.0));
        Vec3 y = directionOf(at(*op, 1)).value_or(Vec3(0.0, 1.0, 0.0));
        const Vec3 origin = cartesian(at(*op, 2));
        const double scale = number(*op, 3).value_or(1.0);
        Vec3 z = threeD ? directionOf(at(*op, 4)).value_or(x.cross(y)) : Vec3(0.0, 0.0, 1.0);
        double sy = scale;
        double sz = scale;
        if (op->type.ends_with("NONUNIFORM")) {
            sy = number(*op, threeD ? 5 : 4).value_or(scale);
            sz = threeD ? number(*op, 6).value_or(scale) : scale;
        }
        x = x * scale;
        y = y * sy;
        z = z * sz;
        return Mat4(x.x, y.x, z.x, origin.x, x.y, y.y, z.y, origin.y, x.z, y.z, z.z, origin.z, 0.0,
                    0.0, 0.0, 1.0);
    }

    // ---- products ----------------------------------------------------------------------------

    std::string fallbackLayer(std::uint32_t id, const ProductClass& ifcClass) const
    {
        std::string layer = "IFC";
        if (const auto container = containerOf(id)) {
            const std::string name = layerSegment(text(*m_.find(*container), 2));
            if (!name.empty()) {
                layer += "/" + name;
            }
        }
        std::string_view name = ifcClass.name;
        if (name.starts_with("Ifc")) {
            name.remove_prefix(3);
        }
        return layer + "/" + std::string(name);
    }

    std::string layerFor(std::uint32_t representation, const std::vector<Shape>& drawn,
                         std::uint32_t id, const ProductClass& ifcClass) const
    {
        if (const auto found = layerOf_.find(representation); found != layerOf_.end()) {
            return found->second;
        }
        for (const Shape& shape : drawn) {
            if (const auto found = layerOf_.find(shape.item); found != layerOf_.end()) {
                return found->second;
            }
        }
        return fallbackLayer(id, ifcClass);
    }

    void readProducts()
    {
        for (const std::uint32_t id : m_.order) {
            const StepInstance& product = *m_.find(id);
            const ProductClass* ifcClass = productClass(product.type);
            if (ifcClass == nullptr) {
                continue;
            }
            if (ifcClass->kind == ProductKind::Spatial) {
                // A site or storey is what holds the elements (ifc.container);
                // only a shape of its own - a site boundary, a space's body -
                // is something not drawn.
                if (at(product, 6) != nullptr) {
                    ++spatial_;
                }
                continue;
            }
            if (ifcClass->kind != ProductKind::Drawn) {
                continue;
            }
            ++out_.products;
            ++out_.classes[std::string(ifcClass->name)];
            if (options_.importSurfaces && readSurface(id, product, *ifcClass)) {
                continue;
            }
            if (options_.importElements) {
                readProduct(id, product, *ifcClass);
            }
        }
        if (spatial_ > 0) {
            out_.warnings.push_back(std::to_string(spatial_) +
                                    " spatial elements (sites, buildings, storeys, spaces) have a "
                                    "shape of their own, which is not drawn: they are read as the "
                                    "containers of what they hold");
        }
    }

    // The representations of a product, most drawable first.
    std::vector<std::uint32_t> representations(const StepInstance& product) const
    {
        const StepInstance* shape = at(product, 6);
        if (shape == nullptr) {
            return {};
        }
        std::vector<std::pair<int, std::uint32_t>> ranked;
        for (const std::uint32_t id : ids(*shape, 2)) {
            const StepInstance* representation = m_.find(id);
            if (representation == nullptr) {
                continue;
            }
            const std::string identifier = text(*representation, 1);
            int rank = 4;
            if (identifier == "Axis") {
                rank = 0;
            } else if (identifier == "FootPrint") {
                rank = 1;
            } else if (identifier == "Annotation") {
                rank = 2;
            } else if (identifier == "Body" || identifier == "Body-Fallback") {
                rank = 3;
            }
            ranked.emplace_back(rank, id);
        }
        std::stable_sort(ranked.begin(), ranked.end(),
                         [](const auto& a, const auto& b) { return a.first < b.first; });
        std::vector<std::uint32_t> out;
        for (const auto& [rank, id] : ranked) {
            out.push_back(id);
        }
        return out;
    }

    void readProduct(std::uint32_t id, const StepInstance& product, const ProductClass& ifcClass)
    {
        const Mat4 frame = placement(at(product, 5));
        std::vector<Shape> drawn;
        std::uint32_t used = 0;
        for (const std::uint32_t representation : representations(product)) {
            const StepInstance* instance = m_.find(representation);
            for (const std::uint32_t item : ids(*instance, 3)) {
                if (const StepInstance* child = m_.find(item)) {
                    shapes(*child, item, frame, drawn, 0);
                }
            }
            if (!drawn.empty()) {
                used = representation;
                break;
            }
        }
        entity::Entity base;
        describe(id, product, ifcClass, base);
        if (drawn.empty()) {
            if (at(product, 5) == nullptr) {
                ++unplaced_;
                return;
            }
            // Nothing Katana can draw: the object at its placement, so that
            // it and what it says are not lost.
            const Vec3 at = world(frame, Vec3());
            Shape shape;
            shape.geometry = entity::PointGeometry{Vec2(at.x, at.y)};
            shape.heights.emplace_back(at.z);
            drawn.push_back(std::move(shape));
            base.metadata.insert_or_assign("ifc.geometry", std::string("placement only"));
            ++out_.productsAsPoints;
        } else {
            ++out_.productsImported;
        }
        base.layer = layerFor(used, drawn, id, ifcClass);
        for (Shape& shape : drawn) {
            entity::Entity entity = base;
            entity.geometry = std::move(shape.geometry);
            if (const auto colour = colourOf_.find(shape.item); colour != colourOf_.end()) {
                entity.color = colour->second;
            }
            if (shape.sweptRadius && *shape.sweptRadius > 0.0) {
                entity.properties.insert_or_assign("SweptDisk/Radius", *shape.sweptRadius);
            }
            std::size_t count = 1;
            if (const auto* polyline = std::get_if<geometry::Polyline2>(&entity.geometry)) {
                count = polyline->vertices.size();
            } else if (std::holds_alternative<geometry::Segment2>(entity.geometry)) {
                count = 2;
            }
            if (shape.heights.size() == count) {
                entity::setHeights(entity.properties, shape.heights);
            }
            add(std::move(entity));
        }
    }

    void add(entity::Entity entity)
    {
        if (auto status = entity::validate(entity.geometry); !status) {
            ++invalid_;
            return;
        }
        out_.bounds.expand(entity::boundingBox(entity.geometry));
        layers_.insert(entity.layer);
        out_.entities.push_back(std::move(entity));
    }

    // ---- surfaces --------------------------------------------------------------------------

    bool readSurface(std::uint32_t id, const StepInstance& product, const ProductClass& ifcClass)
    {
        const bool geographic = ifcClass.name == "IfcGeographicElement";
        const Mat4 frame = placement(at(product, 5));
        for (const std::uint32_t representation : representations(product)) {
            const StepInstance* instance = m_.find(representation);
            for (const std::uint32_t item : ids(*instance, 3)) {
                const StepInstance* faces = m_.find(item);
                if (faces == nullptr) {
                    continue;
                }
                const bool tin = faces->type == "IFCTRIANGULATEDIRREGULARNETWORK";
                if (!(tin || (geographic && faces->type == "IFCTRIANGULATEDFACESET"))) {
                    continue;
                }
                std::string name = text(product, 2);
                if (name.empty()) {
                    name = "Surface " + std::to_string(out_.surfaces.size() + 1);
                }
                if (auto surface = triangles(*faces, frame)) {
                    out_.surfaces.push_back({name, std::move(*surface)});
                    ++out_.productsImported;
                    return true;
                }
                (void)id;
                return false;
            }
        }
        return false;
    }

    std::optional<terrain::TinSurface> triangles(const StepInstance& faces, const Mat4& frame)
    {
        const StepInstance* list = at(faces, 0);
        if (list == nullptr || list->arguments.empty() ||
            list->arguments.front().kind != StepValue::Kind::List) {
            return std::nullopt;
        }
        std::vector<geometry::Point3> vertices;
        for (const StepValue& row : list->arguments.front().items) {
            Vec3 p;
            if (row.kind == StepValue::Kind::List && row.items.size() >= 3) {
                p = Vec3(row.items[0].number().value_or(0.0), row.items[1].number().value_or(0.0),
                         row.items[2].number().value_or(0.0));
            }
            vertices.push_back(world(frame, p));
        }
        // PnIndex, when given, says which point each index means.
        std::vector<std::uint32_t> remap;
        if (const StepValue* pn = arg(faces, 4); pn && pn->kind == StepValue::Kind::List) {
            for (const StepValue& value : pn->items) {
                remap.push_back(static_cast<std::uint32_t>(value.number().value_or(0.0)));
            }
        }
        std::vector<terrain::TinTriangle> triangles;
        const StepValue* indices = arg(faces, 3);
        if (indices == nullptr || indices->kind != StepValue::Kind::List) {
            return std::nullopt;
        }
        for (const StepValue& row : indices->items) {
            if (row.kind != StepValue::Kind::List || row.items.size() != 3) {
                continue;
            }
            std::array<std::uint32_t, 3> t{};
            bool valid = true;
            for (std::size_t k = 0; k < 3; ++k) {
                auto n = static_cast<std::uint32_t>(row.items[k].number().value_or(0.0));
                if (!remap.empty()) {
                    n = n >= 1 && n <= remap.size() ? remap[n - 1] : 0;
                }
                valid = valid && n >= 1 && n <= vertices.size();
                t[k] = n - 1;
            }
            if (!valid) {
                continue;
            }
            // Either way round, as the .12da import takes a triangle.
            const auto& a = vertices[t[0]];
            const auto& b = vertices[t[1]];
            const auto& c = vertices[t[2]];
            if ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x) < 0.0) {
                std::swap(t[1], t[2]);
            }
            triangles.push_back(t);
        }
        auto surface = terrain::TinSurface::create(std::move(vertices), std::move(triangles));
        if (!surface) {
            out_.warnings.push_back("a surface could not be built: " + surface.error().describe());
            return std::nullopt;
        }
        return std::move(*surface);
    }

    // ---- alignments ------------------------------------------------------------------------

    // A name for a warning: the object's own, quoted, or its instance
    // number when it has none.
    static std::string shownName(std::uint32_t id, const std::string& name)
    {
        return name.empty() ? "#" + std::to_string(id) + " (no name)" : "\"" + name + "\"";
    }

    std::string uniqueAlignmentName(std::string name)
    {
        if (name.empty()) {
            name = "Alignment " + std::to_string(alignmentNames_.size() + 1);
        }
        std::string candidate = name;
        for (int n = 2; alignmentNames_.contains(candidate); ++n) {
            candidate = name + " (" + std::to_string(n) + ")";
        }
        alignmentNames_.insert(candidate);
        return candidate;
    }

    std::optional<std::uint32_t> layout(std::uint32_t alignment, std::string_view type) const
    {
        if (const auto found = nested_.find(alignment); found != nested_.end()) {
            for (const std::uint32_t child : found->second) {
                if (is(m_.find(child), type)) {
                    return child;
                }
            }
        }
        return std::nullopt;
    }

    std::vector<const StepInstance*> designParameters(std::uint32_t layout) const
    {
        std::vector<const StepInstance*> out;
        if (const auto found = nested_.find(layout); found != nested_.end()) {
            for (const std::uint32_t child : found->second) {
                const StepInstance* segment = m_.find(child);
                if (is(segment, "IFCALIGNMENTSEGMENT")) {
                    if (const StepInstance* parameters = at(*segment, 7)) {
                        out.push_back(parameters);
                    }
                }
            }
        }
        return out;
    }

    double startStation(std::uint32_t alignment) const
    {
        std::optional<std::pair<double, double>> best; // distance, station
        const auto found = nested_.find(alignment);
        if (found == nested_.end()) {
            return 0.0;
        }
        for (const std::uint32_t child : found->second) {
            const StepInstance* referent = m_.find(child);
            if (!is(referent, "IFCREFERENT")) {
                continue;
            }
            std::optional<double> station;
            if (const auto sets = propertySets_.find(child); sets != propertySets_.end()) {
                for (const std::uint32_t set : sets->second) {
                    const StepInstance* pset = m_.find(set);
                    if (pset == nullptr || text(*pset, 2) != "Pset_Stationing") {
                        continue;
                    }
                    for (const std::uint32_t property : ids(*pset, 4)) {
                        const StepInstance* value = m_.find(property);
                        if (value != nullptr && text(*value, 0) == "Station") {
                            station = number(*value, 2);
                        }
                    }
                }
            }
            double distance = 0.0;
            if (const StepInstance* linear = at(*referent, 5); is(linear, "IFCLINEARPLACEMENT")) {
                if (const StepInstance* relative = at(*linear, 1)) {
                    if (const StepInstance* expression = at(*relative, 0)) {
                        distance = number(*expression, 0).value_or(0.0);
                    }
                }
            }
            if (station && (!best || distance < best->first)) {
                best = std::pair(distance, *station);
            }
        }
        return best ? (best->second - best->first) * length_ : 0.0;
    }

    void readAlignments()
    {
        for (const std::uint32_t id : m_.order) {
            const StepInstance& alignment = *m_.find(id);
            if (alignment.type != "IFCALIGNMENT") {
                continue;
            }
            std::optional<std::uint32_t> horizontal = layout(id, "IFCALIGNMENTHORIZONTAL");
            std::uint32_t horizontalOwner = id;
            // A child alignment reuses its parent's horizontal (the IFC 4.3
            // concept template for a horizontal shared by several verticals).
            if (!horizontal) {
                if (const auto parent = parent_.find(id);
                    parent != parent_.end() && is(m_.find(parent->second), "IFCALIGNMENT")) {
                    horizontal = layout(parent->second, "IFCALIGNMENTHORIZONTAL");
                    horizontalOwner = parent->second;
                }
            }
            const std::string name = text(alignment, 2);
            if (!horizontal) {
                // No business logic: its representation is read as linework,
                // as any other product's is.
                readProduct(id, alignment, *productClass("IFCALIGNMENT"));
                out_.warnings.push_back("alignment " + shownName(id, name) +
                                        " has no horizontal layout and came in as its shape");
                ++out_.alignmentsAsPolylines;
                continue;
            }
            const Mat4 frame = world_ * placement(at(alignment, 5));
            const double linear = math::transformVector(frame, Vec3(1.0, 0.0, 0.0)).length();
            std::vector<detail::HorizontalSegment> hSegments;
            for (const StepInstance* p : designParameters(*horizontal)) {
                detail::HorizontalSegment s;
                s.type = text(*p, 8);
                const Vec3 start = math::transformPoint(frame, cartesian(at(*p, 2)));
                s.start = Vec2(start.x, start.y);
                const double a = number(*p, 3).value_or(0.0) * angle_;
                const Vec3 d = math::transformVector(frame, Vec3(std::cos(a), std::sin(a), 0.0));
                s.direction = std::atan2(d.y, d.x);
                const auto curvature = [&](std::size_t index) {
                    const double r = number(*p, index).value_or(0.0) * linear;
                    return r == 0.0 ? 0.0 : 1.0 / r;
                };
                s.startCurvature = curvature(4);
                s.endCurvature = curvature(5);
                s.length = number(*p, 6).value_or(0.0) * linear;
                hSegments.push_back(std::move(s));
            }
            std::vector<detail::VerticalSegment> vSegments;
            if (const auto vertical = layout(id, "IFCALIGNMENTVERTICAL")) {
                for (const StepInstance* p : designParameters(*vertical)) {
                    detail::VerticalSegment s;
                    s.type = text(*p, 8);
                    s.startDistance = number(*p, 2).value_or(0.0) * linear;
                    s.length = number(*p, 3).value_or(0.0) * linear;
                    s.startHeight =
                        math::transformPoint(frame, Vec3(0.0, 0.0, number(*p, 4).value_or(0.0))).z;
                    s.startGrade = number(*p, 5).value_or(0.0);
                    s.endGrade = number(*p, 6).value_or(0.0);
                    vSegments.push_back(std::move(s));
                }
            }
            if (layout(id, "IFCALIGNMENTCANT")) {
                warnOnce("cant (superelevation) is not read: Katana's alignments have none");
            }
            const double station = startStation(horizontalOwner);
            readAlignment(id, alignment, name, station, hSegments, vSegments);
        }
    }

    void readAlignment(std::uint32_t id, const StepInstance& alignment, const std::string& name,
                       double station, const std::vector<detail::HorizontalSegment>& hSegments,
                       const std::vector<detail::VerticalSegment>& vSegments)
    {
        std::string why;
        std::optional<geometry::HorizontalAlignment> horizontal =
            detail::reconstructHorizontal(hSegments, station, why);
        if (horizontal) {
            auto solved = geometry::solveAlignment(*horizontal);
            if (!solved) {
                why = "its reconstruction does not solve: " + solved.error().describe();
                horizontal.reset();
            } else if (const double off = detail::checkHorizontal(*solved, hSegments);
                       off > kAlignmentTolerance) {
                why = "its reconstruction stands " + std::to_string(off) +
                      " m from the geometry the file states";
                horizontal.reset();
            }
        }
        std::optional<geometry::VerticalAlignment> vertical;
        std::string verticalWhy;
        if (!vSegments.empty()) {
            vertical = detail::reconstructVertical(vSegments, station, verticalWhy);
            if (vertical) {
                auto solved = geometry::solveProfile(*vertical);
                if (!solved) {
                    verticalWhy = "its reconstruction does not solve: " + solved.error().describe();
                    vertical.reset();
                } else if (const double off = detail::checkVertical(*solved, vSegments, station);
                           off > kAlignmentTolerance) {
                    verticalWhy = "its reconstruction stands " + std::to_string(off) +
                                  " m from the levels the file states";
                    vertical.reset();
                }
            }
        }
        if (horizontal) {
            entity::Alignment result;
            result.name = uniqueAlignmentName(name);
            result.description = text(alignment, 3);
            result.horizontal = std::move(*horizontal);
            result.vertical = std::move(vertical);
            if (!vSegments.empty() && !result.vertical) {
                out_.warnings.push_back("the profile of alignment \"" + result.name +
                                        "\" is not a Katana design profile (" + verticalWhy +
                                        "); its levels are on a 3D polyline beside it");
                alignmentLine(id, alignment, name, hSegments, vSegments);
            }
            if (auto status = entity::validate(result); !status) {
                out_.warnings.push_back("alignment \"" + result.name +
                                        "\" is not valid: " + status.error().describe());
                alignmentLine(id, alignment, name, hSegments, vSegments);
                ++out_.alignmentsAsPolylines;
                return;
            }
            out_.alignments.push_back(std::move(result));
            return;
        }
        out_.warnings.push_back("alignment " + shownName(id, name) +
                                " came in as a polyline of its geometry: " + why);
        alignmentLine(id, alignment, name, hSegments, vSegments);
        ++out_.alignmentsAsPolylines;
    }

    // The alignment's exact geometry as a 3D polyline, heights from its
    // profile where it has one.
    void alignmentLine(std::uint32_t id, const StepInstance& alignment, const std::string& name,
                       const std::vector<detail::HorizontalSegment>& hSegments,
                       const std::vector<detail::VerticalSegment>& vSegments)
    {
        const auto samples = detail::sampleHorizontal(hSegments, options_.curveTolerance);
        if (samples.size() < 2) {
            return;
        }
        geometry::Polyline2 line;
        std::vector<std::optional<double>> heights;
        for (const auto& [distance, at] : samples) {
            line.vertices.push_back(at);
            heights.push_back(vSegments.empty() ? std::nullopt
                                                : detail::heightAt(vSegments, distance));
        }
        entity::Entity entity;
        const ProductClass& ifcClass = *productClass("IFCALIGNMENT");
        describe(id, alignment, ifcClass, entity);
        entity.metadata.insert_or_assign("ifc.alignment", name);
        entity.geometry = std::move(line);
        entity::setHeights(entity.properties, heights);
        entity.layer = "IFC/Alignments";
        if (const StepInstance* shape = at(alignment, 6)) {
            for (const std::uint32_t representation : ids(*shape, 2)) {
                if (const auto found = layerOf_.find(representation); found != layerOf_.end()) {
                    entity.layer = found->second;
                }
            }
        }
        add(std::move(entity));
    }

    void finish()
    {
        // The extent of everything the import brings, not only its entities:
        // what LOCAL shifts by and what the far-apart question weighs, so a
        // file of an alignment or a terrain alone is placed like any other.
        for (const entity::Alignment& alignment : out_.alignments) {
            for (const geometry::AlignmentPI& pi : alignment.horizontal.pis) {
                out_.bounds.expand(pi.point);
            }
        }
        for (const ImportedSurface& surface : out_.surfaces) {
            if (surface.surface.vertexCount() > 0) {
                out_.bounds.expand(surface.surface.bounds());
            }
        }
        for (const std::string& name : layers_) {
            entity::Layer layer;
            layer.name = name;
            out_.layers.push_back(std::move(layer));
        }
        if (invalid_ > 0) {
            out_.warnings.push_back(std::to_string(invalid_) +
                                    " shapes had no extent Katana can hold (a zero-length line, "
                                    "an empty text) and were not taken");
        }
        if (unplaced_ > 0) {
            out_.warnings.push_back(std::to_string(unplaced_) +
                                    " objects had neither a shape Katana can draw nor a placement");
        }
        if (out_.productsAsPoints > 0) {
            out_.warnings.push_back(std::to_string(out_.productsAsPoints) +
                                    " objects are solids Katana cannot draw and came in as a "
                                    "point at their placement, with their properties");
        }
        if (m_.complexInstances > 0) {
            out_.warnings.push_back(std::to_string(m_.complexInstances) +
                                    " complex instances were read past");
        }
    }

    const detail::StepModel& m_;
    const ImportOptions& options_;
    IfcImport& out_;

    double length_ = 1.0;
    double angle_ = 1.0;
    double area_ = 1.0;
    double volume_ = 1.0;
    Mat4 world_{};

    std::unordered_map<std::uint32_t, std::vector<std::uint32_t>> propertySets_;
    std::unordered_map<std::uint32_t, std::uint32_t> typeOf_;
    std::unordered_map<std::uint32_t, std::vector<std::uint32_t>> classifications_;
    std::unordered_map<std::uint32_t, std::vector<std::uint32_t>> groups_;
    std::unordered_map<std::uint32_t, std::uint32_t> container_;
    std::unordered_map<std::uint32_t, std::uint32_t> parent_;
    std::unordered_map<std::uint32_t, std::vector<std::uint32_t>> nested_;
    std::unordered_map<std::uint32_t, std::string> layerOf_;
    std::unordered_map<std::uint32_t, entity::Color> colourOf_;

    std::set<std::string> layers_;
    std::set<std::string> alignmentNames_;
    std::set<std::string> warned_;
    std::size_t spatial_ = 0;
    std::size_t invalid_ = 0;
    std::size_t unplaced_ = 0;
};

} // namespace

Result<IfcImport> readIfc(std::string_view text, const ImportOptions& options)
{
    auto parsed = detail::parseStep(text);
    if (!parsed) {
        return parsed.error();
    }
    if (!parsed->schema.starts_with("IFC")) {
        return makeError(ErrorCode::Unsupported,
                         "the file's schema is \"" + parsed->schema + "\", not an IFC schema");
    }
    IfcImport out;
    out.schema = parsed->schema;
    Reader(*parsed, options, out).run();
    return out;
}

Result<IfcImport> readIfcFile(const std::filesystem::path& path, const ImportOptions& options)
{
    std::ifstream file(path, std::ios::binary);
    if (!file) {
        return makeError(ErrorCode::NotFound, "the file cannot be read", path.string());
    }
    const std::string text((std::istreambuf_iterator<char>(file)),
                           std::istreambuf_iterator<char>());
    ImportOptions named = options;
    if (named.sourceName.empty()) {
        named.sourceName = path.filename().string();
    }
    auto result = readIfc(text, named);
    if (!result) {
        auto error = result.error();
        error.context = path.string() + (error.context.empty() ? "" : ": " + error.context);
        return error;
    }
    return result;
}

commands::CommandPtr importCommand(IfcImport& imported, const entity::Model& model)
{
    namespace cmd = katana::commands;
    auto transaction = std::make_unique<cmd::Transaction>("IMPORT");
    // By name, so that "a" comes before "a/b".
    std::vector<entity::Layer> layers = imported.layers;
    std::sort(layers.begin(), layers.end(),
              [](const auto& a, const auto& b) { return a.name < b.name; });
    std::set<std::string> created;
    for (const entity::Layer& layer : layers) {
        // Parents first: a layer "a/b" needs "a", which add() makes for it,
        // so only the layers the model lacks are asked for, once.
        if (model.layers.contains(layer.name) || created.contains(layer.name)) {
            continue;
        }
        for (const std::string& ancestor : entity::layerAncestors(layer.name)) {
            created.insert(ancestor);
        }
        created.insert(layer.name);
        transaction->add(cmd::createLayer(layer));
    }
    for (entity::Alignment& alignment : imported.alignments) {
        std::string name = alignment.name;
        for (int n = 2; model.alignments.contains(name); ++n) {
            name = alignment.name + " (" + std::to_string(n) + ")";
        }
        if (name != alignment.name) {
            imported.warnings.push_back("alignment \"" + alignment.name +
                                        "\" is already in the drawing; the import's is \"" + name +
                                        "\"");
            alignment.name = name;
        }
        transaction->add(cmd::createAlignment(alignment));
    }
    if (!imported.entities.empty()) {
        transaction->add(cmd::createEntities(std::move(imported.entities)));
        imported.entities.clear();
    }
    if (transaction->size() == 0) {
        return nullptr;
    }
    return transaction;
}

} // namespace katana::ifc
