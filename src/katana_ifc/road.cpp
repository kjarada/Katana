// The road model -> IFC 4.3 (docs/ifc.md, "Road model").
//
// Horizontal BIM: the objects a corridor sweeps and the objects placed along
// a road (RoadModelInput, made by katana_app from cad::bim::bimModel). The
// sections cited are those of the IFC 4.3.2.0 documentation (IFC4X3_ADD2).
//
// THE SPATIAL STRUCTURE. An IfcRoad (7.9.3.3) per facility - a corridor -
// aggregated into the site, and an IfcRoadPart (7.9.3.4) per part of it,
// aggregated into its road; each element contained in its part, in its road
// when it names no part, in the site when it names no facility. The road is
// an ELEMENT and its parts PARTIAL, the steps IfcRoad's page asks of one
// facility over another (an ELEMENT parent over PARTIAL children). A part's
// UsageType (IfcFacilityPart, mandatory; IfcFacilityUsageEnum 5.4.2.11) is
// LATERAL for the parts across the road's section - most a "lateral part" in
// IfcRoadPartTypeEnum's own definitions (7.9.2.2: CARRIAGEWAY, TRAFFICLANE,
// SHOULDER, ROADSIDE, CENTRALRESERVE ...), the rest beside one (SIDEWALK, a
// footpath along the side of the road; ROADSIDEPART) - LONGITUDINAL for a
// ROADSEGMENT ("Longitudinal, linear segment of a road"), REGION for a
// bounded area (an INTERSECTION, a ROUNDABOUT, a LAYBY ...). Every placement is relative to its container's,
// as IfcLocalPlacement (8.7.3.14) has it: the road to the site, a part to
// its road, an element to its part, a feature to its host.
//
// THE BODY is an IfcTriangulatedFaceSet (8.8.3.47), Closed when it is: every
// edge used by exactly two faces, each once in each direction, which is what
// the schema asks of a closed face set ("each edge shall be used by exactly
// two boundaries of faces", normals pointing away from the material). A
// closed mesh wound inwards is turned outwards; vertices at exactly the same
// place are written once and a face naming one vertex twice is dropped,
// neither of which moves anything. Rejected: IfcPolygonalFaceSet. The road
// model's meshes are triangles already, so merging coplanar ones into
// polygons would only save bytes, and the triangulated set is the one the
// surfaces already write, which is in scope of the Alignment-based view this
// file declares (surfaces.cpp). Rejected too: an extruded profile (a
// course's section swept along the alignment) - truer to the design, but a
// sweep along an IfcGradientCurve is beyond what every viewer draws, and the
// mesh is already what the 3D view shows and the quantities were measured on.
//
// FEATURES. An IfcEarthworksCut (6.6.3.4) is "the resulting void from
// modification of existing terrain", an IfcFeatureElementSubtraction whose
// IfcRelVoidsElement is a mandatory inverse and which "shall have no
// independent containment relationship to the spatial structure" (5.4.3.30);
// an IfcSurfaceFeature (7.11.3.16) - a line marking - likewise adheres to an
// element through IfcRelAdheresToElement (5.4.3.45). So each is written on its
// host and contained nowhere: a cut voids the element it names, else the
// surface it was taken from when the file holds it, else an
// IfcGeographicElement TERRAIN that stands for that ground with no shape of
// its own; a marking adheres to the element it names, else to the course,
// pavement or slab of its road that lies under it - the highest there - and
// with none, is not written, said, since a marking on nothing would be an
// invalid file.
//
// MATERIAL is an IfcMaterial of the element's material's name, written once
// and associated (IfcRelAssociatesMaterial) with every element of it - but
// not with a cut, which is a void: that relationship's NoVoidElement rule
// refuses one. Every element names its material in Katana_RoadElement too.
//
// QUANTITIES are an IfcElementQuantity (5.4.3.21) of IfcQuantityLength, Area,
// Volume, Weight and Count. Those a class's base quantity set names - with
// the quantity type it gives them - go in that set (Qto_CourseBaseQuantities
// 6.6.5.2, Qto_EarthworksCutBaseQuantities 6.6.5.3,
// Qto_EarthworksFillBaseQuantities 6.6.5.4, Qto_KerbBaseQuantities 6.6.5.6,
// Qto_PavementBaseQuantities 6.6.5.8, Qto_SignBaseQuantities 6.6.5.11,
// Qto_RailingBaseQuantities 6.1.5.10), whose MethodOfMeasurement is
// 'BaseQuantities'; the rest in Katana_Quantities, so a quantity-take-off
// tool finds a course's volume where it looks and nothing is claimed to be a
// base quantity that the template does not define.

#include <algorithm>
#include <array>
#include <cctype>
#include <cmath>
#include <map>
#include <optional>
#include <set>
#include <string>
#include <string_view>
#include <tuple>
#include <utility>
#include <vector>

#include "katana/core/text.hpp"
#include "parts.hpp"
#include "product_layout.hpp"

namespace katana::ifc::detail {

namespace {

enum class QuantityKind { Length, Area, Volume, Weight, Count };

std::string_view entityOf(QuantityKind kind)
{
    switch (kind) {
    case QuantityKind::Length:
        return "IfcQuantityLength";
    case QuantityKind::Area:
        return "IfcQuantityArea";
    case QuantityKind::Volume:
        return "IfcQuantityVolume";
    case QuantityKind::Weight:
        return "IfcQuantityWeight";
    case QuantityKind::Count:
        break;
    }
    return "IfcQuantityCount";
}

// A quantity's kind and its value in the units the file is written in
// (metres, square and cubic metres, kilograms); nullopt for a unit that is
// none of them.
struct Measured {
    QuantityKind kind = QuantityKind::Count;
    double value = 0.0;
};

std::optional<Measured> measured(const RoadQuantity& quantity)
{
    const std::string unit = core::lowered(core::trimmed(quantity.unit));
    if (unit == "m") {
        return Measured{QuantityKind::Length, quantity.value};
    }
    if (unit == "m2" || unit == "m\xC2\xB2") {
        return Measured{QuantityKind::Area, quantity.value};
    }
    if (unit == "m3" || unit == "m\xC2\xB3") {
        return Measured{QuantityKind::Volume, quantity.value};
    }
    if (unit == "kg") {
        return Measured{QuantityKind::Weight, quantity.value};
    }
    if (unit == "t") {
        return Measured{QuantityKind::Weight, quantity.value * 1000.0}; // a tonne is 1000 kg
    }
    if (unit.empty()) {
        return Measured{QuantityKind::Count, quantity.value};
    }
    return std::nullopt;
}

// A class's base quantity set: its name, and each quantity it defines with
// its type, "Name=K" separated by blanks (L length, A area, V volume, W
// weight). Each row is the IFC 4.3.2.0 template cited in the header.
struct BaseQuantities {
    std::string_view entity;
    std::string_view set;
    std::string_view quantities;
};

constexpr BaseQuantities kBaseQuantities[] = {
    {"IfcCourse", "Qto_CourseBaseQuantities",
     "Length=L Width=L Thickness=L Volume=V GrossVolume=V Weight=W"},
    {"IfcEarthworksCut", "Qto_EarthworksCutBaseQuantities",
     "Length=L Width=L Depth=L UndisturbedVolume=V LooseVolume=V Weight=W"},
    {"IfcEarthworksFill", "Qto_EarthworksFillBaseQuantities",
     "Length=L Width=L Depth=L CompactedVolume=V LooseVolume=V"},
    {"IfcKerb", "Qto_KerbBaseQuantities", "Length=L Width=L Height=L Depth=L Volume=V Weight=W"},
    {"IfcPavement", "Qto_PavementBaseQuantities",
     "Length=L Width=L Depth=L GrossArea=A NetArea=A GrossVolume=V NetVolume=V"},
    {"IfcRailing", "Qto_RailingBaseQuantities", "Length=L"},
    {"IfcSign", "Qto_SignBaseQuantities", "Height=L Width=L Thickness=L Weight=W"},
};

const BaseQuantities* baseQuantitiesOf(std::string_view entity)
{
    for (const BaseQuantities& row : kBaseQuantities) {
        if (row.entity == entity) {
            return &row;
        }
    }
    return nullptr;
}

// True when `row` defines `name` as a quantity of `kind`.
bool templateDefines(const BaseQuantities& row, std::string_view name, QuantityKind kind)
{
    char letter = 'L';
    switch (kind) {
    case QuantityKind::Length:
        letter = 'L';
        break;
    case QuantityKind::Area:
        letter = 'A';
        break;
    case QuantityKind::Volume:
        letter = 'V';
        break;
    case QuantityKind::Weight:
        letter = 'W';
        break;
    case QuantityKind::Count:
        return false; // no template here defines a count
    }
    const std::string wanted = std::string(name) + "=" + letter;
    return listsWord(row.quantities, wanted);
}

// The lateral parts of a road, "lateral" in IfcRoadPartTypeEnum's own
// definitions (7.9.2.2), and the one longitudinal part.
constexpr std::string_view kLateralParts[] = {
    "CARRIAGEWAY", "TRAFFICLANE", "SHOULDER",       "HARDSHOULDER",  "SOFTSHOULDER",
    "SIDEWALK",    "ROADSIDE",    "ROADSIDEPART",   "CENTRALRESERVE", "ROADWAYPLATEAU"};

// IfcRoadPart's PredefinedType, ObjectType and UsageType for a part named
// `part`: the enumeration's own value where it lists it, else USERDEFINED of
// that name.
struct PartType {
    std::string predefinedType;
    std::string objectType;
    std::string_view usage;
};

PartType partTypeOf(const std::string& part)
{
    const std::string upper = [&] {
        std::string out(part);
        for (char& c : out) {
            c = static_cast<char>(std::toupper(static_cast<unsigned char>(c)));
        }
        return out;
    }();
    const auto values = predefinedTypesOf("IfcRoadPart");
    if (!values || !listsWord(*values, upper) || upper == "USERDEFINED") {
        return {"USERDEFINED", part, "NOTDEFINED"};
    }
    if (upper == "NOTDEFINED") {
        return {upper, {}, "NOTDEFINED"};
    }
    if (upper == "ROADSEGMENT") {
        return {upper, {}, "LONGITUDINAL"};
    }
    if (std::find(std::begin(kLateralParts), std::end(kLateralParts), upper) !=
        std::end(kLateralParts)) {
        return {upper, {}, "LATERAL"};
    }
    return {upper, {}, "REGION"};
}

// A mesh as a face set writes it: each place once, in the order the faces
// first name it; no face naming a place twice; closed, and then wound
// outwards, when every edge is used once in each direction.
struct Tessellation {
    std::vector<Vec3> points; // world
    std::vector<std::array<std::uint32_t, 3>> faces;
    bool closed = false;
};

std::optional<Tessellation> tessellate(const geometry::TriangleMesh& mesh)
{
    Tessellation out;
    std::map<std::array<double, 3>, std::uint32_t> placed;
    const auto indexOf = [&](const geometry::Point3& p) {
        const std::array<double, 3> key{p.x, p.y, p.z};
        const auto found = placed.find(key);
        if (found != placed.end()) {
            return found->second;
        }
        const auto index = static_cast<std::uint32_t>(out.points.size());
        placed.emplace(key, index);
        out.points.push_back(p);
        return index;
    };
    for (const auto& face : mesh.faces) {
        const std::uint32_t a = indexOf(mesh.vertices[face[0]]);
        const std::uint32_t b = indexOf(mesh.vertices[face[1]]);
        const std::uint32_t c = indexOf(mesh.vertices[face[2]]);
        if (a == b || b == c || c == a) {
            continue;
        }
        out.faces.push_back({a, b, c});
    }
    if (out.faces.empty()) {
        return std::nullopt;
    }
    // A point only a dropped face named is not written.
    std::vector<std::uint32_t> renumbered(out.points.size(), 0);
    std::vector<Vec3> used;
    std::vector<bool> seen(out.points.size(), false);
    for (auto& face : out.faces) {
        for (std::uint32_t& index : face) {
            if (!seen[index]) {
                seen[index] = true;
                renumbered[index] = static_cast<std::uint32_t>(used.size());
                used.push_back(out.points[index]);
            }
            index = renumbered[index];
        }
    }
    out.points = std::move(used);

    std::map<std::pair<std::uint32_t, std::uint32_t>, int> edges;
    for (const auto& face : out.faces) {
        for (int i = 0; i < 3; ++i) {
            ++edges[{face[static_cast<std::size_t>(i)], face[static_cast<std::size_t>((i + 1) % 3)]}];
        }
    }
    out.closed = std::all_of(edges.begin(), edges.end(), [&](const auto& edge) {
        const auto reverse = edges.find({edge.first.second, edge.first.first});
        return edge.second == 1 && reverse != edges.end() && reverse->second == 1;
    });
    if (out.closed) {
        // The signed volume (the divergence theorem over the faces, about the
        // first point to keep the products small): negative when the faces
        // are wound inwards.
        const Vec3 origin = out.points.front();
        double volume = 0.0;
        for (const auto& face : out.faces) {
            const Vec3 p = out.points[face[0]] - origin;
            const Vec3 q = out.points[face[1]] - origin;
            const Vec3 r = out.points[face[2]] - origin;
            volume += p.dot(q.cross(r));
        }
        if (volume < 0.0) {
            for (auto& face : out.faces) {
                std::swap(face[1], face[2]);
            }
        }
    }
    return out;
}

// The level of `mesh` at plan point `at`: the highest of its faces over it,
// nullopt where none is.
std::optional<double> levelAt(const Tessellation& mesh, Vec2 at)
{
    // Inside to within a nanometre of an edge, so a point on the edge
    // between two faces is on both rather than on neither.
    constexpr double kOnEdge = 1e-9;
    std::optional<double> best;
    for (const auto& face : mesh.faces) {
        const Vec3& a = mesh.points[face[0]];
        const Vec3& b = mesh.points[face[1]];
        const Vec3& c = mesh.points[face[2]];
        const double det = (b.y - c.y) * (a.x - c.x) + (c.x - b.x) * (a.y - c.y);
        if (std::abs(det) < 1e-12) {
            continue; // vertical in plan: nothing lies on it from above
        }
        const double u = ((b.y - c.y) * (at.x - c.x) + (c.x - b.x) * (at.y - c.y)) / det;
        const double v = ((c.y - a.y) * (at.x - c.x) + (a.x - c.x) * (at.y - c.y)) / det;
        const double w = 1.0 - u - v;
        if (u < -kOnEdge || v < -kOnEdge || w < -kOnEdge) {
            continue;
        }
        const double z = u * a.z + v * b.z + w * c.z;
        best = best ? std::max(*best, z) : z;
    }
    return best;
}

bool isSurfaceClass(std::string_view entity)
{
    return entity == "IfcCourse" || entity == "IfcPavement" || entity == "IfcSlab";
}

class RoadWriter {
  public:
    RoadWriter(Builder& builder, const RoadModelInput& road,
               const std::map<std::string, Id>& terrain)
        : b_(builder), road_(road), terrain_(terrain)
    {
    }

    void write()
    {
        for (const std::string& problem : road_.problems) {
            b_.warn("the road model: " + problem);
        }
        std::vector<Accepted> accepted;
        std::set<std::string> ids;
        for (const RoadElement& element : road_.elements) {
            if (auto ready = accept(element, ids)) {
                accepted.push_back(std::move(*ready));
            }
        }
        writeFacilities(accepted);
        for (Accepted& item : accepted) {
            if (!item.feature) {
                writeElement(item);
            }
        }
        for (Accepted& item : accepted) {
            if (item.feature) {
                writeFeature(item);
            }
        }
        writeAdherences();
        writeMaterials();
        sayOnce();
    }

  private:
    // An element that will be written: its tessellation, and once written
    // its product and the placement it hangs off.
    struct Accepted {
        const RoadElement* element = nullptr;
        Tessellation body;
        bool feature = false;
        bool voids = false; // a feature that voids its host (ProductLayout::voids)
        Id product = 0;
        Id placement = 0;
    };

    struct Facility {
        Id road = 0;
        Container container; // the road itself, for elements of no part
        std::map<std::string, Container> parts;
    };

    static std::string sourceOf(const RoadElement& element)
    {
        return element.facility.empty() ? std::string("road model") : "road " + element.facility;
    }

    static std::string whyOf(const RoadElement& element)
    {
        return "the road model's " + (element.kind.empty() ? std::string("object") : element.kind);
    }

    void skip(const RoadElement& element, const std::string& reason)
    {
        ++b_.report().roadElementsSkipped;
        b_.warn("road element \"" + element.id + "\" is not written: " + reason);
        b_.tally(sourceOf(element), {}, {}, "not written: " + reason);
    }

    // Checks what would make an invalid file, and tessellates; nullopt, said,
    // for an element that is not written.
    std::optional<Accepted> accept(const RoadElement& element, std::set<std::string>& ids)
    {
        if (element.id.empty()) {
            skip(element, "it has no id, from which its GlobalId would be made");
            return std::nullopt;
        }
        if (!ids.insert(element.id).second) {
            skip(element, "an earlier element has the same id");
            return std::nullopt;
        }
        const IfcClass& ifcClass = element.ifcClass;
        if (ifcClass.entity.empty()) {
            skip(element, "it names no IFC class");
            return std::nullopt;
        }
        const auto layout = findProductLayout(ifcClass.entity);
        if (!layout || ifcClass.entity == "IfcAnnotation") {
            skip(element, ifcClass.entity + " is not a class this export writes for an element");
            return std::nullopt;
        }
        if (!ifcClass.predefinedType.empty()) {
            const auto values = predefinedTypesOf(ifcClass.entity);
            if (!layout->hasPredefinedType || !values ||
                !listsWord(*values, ifcClass.predefinedType)) {
                skip(element, "\"" + ifcClass.predefinedType + "\" is not a predefined type of " +
                                  ifcClass.entity);
                return std::nullopt;
            }
        }
        if (ifcClass.predefinedType == "USERDEFINED" && ifcClass.objectType.empty()) {
            skip(element, "it is USERDEFINED and says nothing of what it is (its object type)");
            return std::nullopt;
        }
        if (const auto valid = geometry::validate(element.mesh); !valid) {
            skip(element, "its mesh is not valid: " + valid.error().describe());
            return std::nullopt;
        }
        for (const geometry::Polyline2& line : element.plan) {
            for (const geometry::Point2& vertex : line.vertices) {
                if (!std::isfinite(vertex.x) || !std::isfinite(vertex.y)) {
                    skip(element, "its plan outline has a vertex that is not finite");
                    return std::nullopt;
                }
            }
        }
        auto body = tessellate(element.mesh);
        if (!body) {
            skip(element, "its mesh has no face");
            return std::nullopt;
        }
        Accepted out;
        out.element = &element;
        out.body = std::move(*body);
        out.feature = layout->feature;
        out.voids = layout->voids;
        return out;
    }

    void writeFacilities(const std::vector<Accepted>& accepted)
    {
        std::vector<Id> roads;
        for (const Accepted& item : accepted) {
            const RoadElement& element = *item.element;
            if (element.facility.empty() || item.feature) {
                continue; // a feature is contained nowhere
            }
            auto found = facilities_.find(element.facility);
            if (found == facilities_.end()) {
                found = facilities_.emplace(element.facility, writeRoad(element.facility)).first;
                roads.push_back(found->second.road);
            }
            if (!element.part.empty() && !found->second.parts.contains(element.part)) {
                found->second.parts.emplace(
                    element.part, writePart(element.facility, found->second, element.part));
                partOrder_[element.facility].push_back(element.part);
            }
        }
        b_.aggregate(b_.site(), "site", roads);
        for (const auto& [name, facility] : facilities_) {
            std::vector<Id> parts;
            for (const std::string& part : partOrder_[name]) {
                parts.push_back(facility.parts.at(part).element);
            }
            b_.aggregate(facility.road, "road/" + name, parts);
        }
    }

    Facility writeRoad(const std::string& name)
    {
        const std::string key = "road/facility/" + name;
        const Id placement = b_.placementIn(b_.sitePlacement());
        const Id road = b_.file().add("IfcRoad", Args()
                                                     .string(b_.guid(key))
                                                     .null()
                                                     .stringOrNull(b_.label(name, "IfcRoad " + name))
                                                     .null()
                                                     .null()
                                                     .ref(placement)
                                                     .null()
                                                     .null()
                                                     .enumeration("ELEMENT")
                                                     .enumeration("NOTDEFINED"));
        ++b_.report().classes["IfcRoad"];
        ++b_.report().roads;
        Facility out;
        out.road = road;
        out.container = Container{road, placement, 0, "road/" + name};
        return out;
    }

    Container writePart(const std::string& facility, const Facility& road, const std::string& part)
    {
        const PartType type = partTypeOf(part);
        const std::string key = "road/facility/" + facility + "/part/" + part;
        const Id placement = b_.placementIn(road.container.placement);
        const std::string name = facility + " " + part;
        const Id id =
            b_.file().add("IfcRoadPart",
                          Args()
                              .string(b_.guid(key))
                              .null()
                              .stringOrNull(b_.label(name, "IfcRoadPart " + name))
                              .null()
                              .stringOrNull(b_.label(type.objectType, "IfcRoadPart " + name))
                              .ref(placement)
                              .null()
                              .null()
                              .enumeration("PARTIAL")
                              .enumeration(type.usage)
                              .enumeration(type.predefinedType));
        ++b_.report().classes["IfcRoadPart"];
        ++b_.report().roadParts;
        return Container{id, placement, 0, "road/" + facility + "/" + part};
    }

    // The container an element is contained in, its element placement made
    // the first time one is placed in it.
    Container& containerOf(const RoadElement& element)
    {
        if (element.facility.empty()) {
            site_ = b_.siteContainer();
            return site_;
        }
        Facility& facility = facilities_.at(element.facility);
        Container& container =
            element.part.empty() ? facility.container : facility.parts.at(element.part);
        if (container.elementPlacement == 0) {
            container.elementPlacement = b_.placementIn(container.placement);
        }
        return container;
    }

    // The body and the footprint, styled and on the element's layer.
    Id representationOf(const Accepted& item)
    {
        const RoadElement& element = *item.element;
        std::vector<std::string> coordinates;
        coordinates.reserve(item.body.points.size());
        for (const Vec3& point : item.body.points) {
            const Vec3 at = b_.local(point);
            coordinates.push_back(listOf({stepReal(at.x), stepReal(at.y), stepReal(at.z)}));
        }
        std::vector<std::string> triangles;
        triangles.reserve(item.body.faces.size());
        for (const auto& face : item.body.faces) {
            triangles.push_back(listOf({std::to_string(face[0] + 1), std::to_string(face[1] + 1),
                                        std::to_string(face[2] + 1)}));
        }
        StepFile& file = b_.file();
        const Id points = file.add("IfcCartesianPointList3D", Args().raw(listOf(coordinates)).null());
        const Id faces = file.add(
            "IfcTriangulatedFaceSet",
            Args().ref(points).null().boolean(item.body.closed).raw(listOf(triangles)).null());
        if (!item.body.closed) {
            ++open_;
        }
        b_.colourSurface(faces, element.colour);
        const Id body = b_.shape(b_.bodyContext(), "Body", "Tessellation", {faces});
        b_.layer(element.layer, body);
        std::vector<Id> representations{body};

        std::vector<Id> outline;
        const auto addOutline = [&](const std::vector<Vec2>& vertices, bool closed) {
            if (vertices.size() >= 2) {
                const Id curve = b_.polyline(vertices, closed);
                b_.colourCurve(curve, element.colour);
                outline.push_back(curve);
            }
        };
        if (!element.plan.empty()) {
            for (const geometry::Polyline2& line : element.plan) {
                addOutline(line.vertices, line.closed);
            }
        } else {
            const std::vector<geometry::Point2> hull = element.mesh.planHull();
            addOutline(hull, hull.size() >= 3);
        }
        if (!outline.empty()) {
            const Id footPrint = b_.shape(b_.footPrintContext(), "FootPrint", "Curve2D", outline);
            b_.layer(element.layer, footPrint);
            representations.push_back(footPrint);
        }
        return b_.productShape(representations);
    }

    void writeElement(Accepted& item)
    {
        const RoadElement& element = *item.element;
        const Id shape = representationOf(item);
        Container& container = containerOf(element);
        item.product = b_.product(element.ifcClass, keyOf(element), element.name, element.kind,
                                  shape, element.id, container);
        item.placement = container.elementPlacement;
        written_.emplace(element.id, &item);
        finishElement(item);
    }

    void writeFeature(Accepted& item)
    {
        const RoadElement& element = *item.element;
        Id host = 0;
        Id hostPlacement = 0;
        std::string hostKey;
        if (!element.host.empty()) {
            const auto found = written_.find(element.host);
            if (found == written_.end()) {
                skip(element, "the element it is on, \"" + element.host + "\", is not written");
                return;
            }
            host = found->second->product;
            hostPlacement = found->second->placement;
            hostKey = "road/element/" + element.host;
        } else if (item.voids) {
            std::tie(host, hostKey) = groundOf(element);
            hostPlacement = b_.elementPlacement();
        } else {
            const Accepted* under = surfaceUnder(item);
            if (under == nullptr) {
                skip(element, "a surface feature adheres to an element, and no course, pavement "
                              "or slab of its road lies under it");
                return;
            }
            host = under->product;
            hostPlacement = under->placement;
            hostKey = "road/element/" + under->element->id;
        }
        auto placement = featurePlacements_.find(hostPlacement);
        if (placement == featurePlacements_.end()) {
            placement = featurePlacements_.emplace(hostPlacement, b_.placementIn(hostPlacement)).first;
        }
        const Id shape = representationOf(item);
        item.product = b_.feature(element.ifcClass, keyOf(element), element.name, element.kind,
                                  shape, element.id, placement->second);
        item.placement = placement->second;
        if (item.voids) {
            b_.file().add("IfcRelVoidsElement", Args()
                                                    .string(b_.guid("voids/" + keyOf(element)))
                                                    .null()
                                                    .null()
                                                    .null()
                                                    .ref(host)
                                                    .ref(item.product));
        } else {
            auto& features = adherences_[host];
            features.key = hostKey;
            features.features.push_back(item.product);
        }
        finishElement(item);
    }

    // The element a cut with no host voids: the surface it was taken from,
    // where the file holds it, else a TERRAIN standing for it - one a ground.
    std::pair<Id, std::string> groundOf(const RoadElement& element)
    {
        if (const auto surface = terrain_.find(element.ground); surface != terrain_.end()) {
            return {surface->second, "surface/" + element.ground};
        }
        const std::string name = element.ground.empty() ? std::string("Existing ground")
                                                        : element.ground;
        const std::string key = "road/ground/" + name;
        auto found = grounds_.find(name);
        if (found == grounds_.end()) {
            const IfcClass terrain{"IfcGeographicElement", "TERRAIN", {}};
            const Id id = b_.product(terrain, key, name,
                                     "the ground the road's earthworks are cut from; its surface "
                                     "is not in this file",
                                     0);
            b_.tally("road model", terrain, {}, "the ground a cut is taken from, not in the file");
            found = grounds_.emplace(name, id).first;
        }
        return {found->second, key};
    }

    // The course, pavement or slab of the marking's road under its first
    // face's centre, the highest there; null when none is.
    const Accepted* surfaceUnder(const Accepted& item) const
    {
        const auto& face = item.body.faces.front();
        const Vec3 centre = (item.body.points[face[0]] + item.body.points[face[1]] +
                             item.body.points[face[2]]) *
                            (1.0 / 3.0);
        const Vec2 at(centre.x, centre.y);
        const Accepted* best = nullptr;
        double bestLevel = 0.0;
        for (const auto& entry : writtenInOrder()) {
            const Accepted* candidate = entry.second;
            const RoadElement& element = *candidate->element;
            if (element.facility != item.element->facility ||
                !isSurfaceClass(element.ifcClass.entity)) {
                continue;
            }
            const auto level = levelAt(candidate->body, at);
            if (level && (best == nullptr || *level > bestLevel)) {
                best = candidate;
                bestLevel = *level;
            }
        }
        return best;
    }

    // The written elements in the order they were written, so that of two at
    // one level the first is chosen whatever their ids.
    std::vector<std::pair<std::string, const Accepted*>> writtenInOrder() const
    {
        std::vector<std::pair<std::string, const Accepted*>> out(written_.begin(), written_.end());
        std::sort(out.begin(), out.end(),
                  [](const auto& a, const auto& b) { return a.second->product < b.second->product; });
        return out;
    }

    static std::string keyOf(const RoadElement& element) { return "road/element/" + element.id; }

    // What every written element carries: its account, its properties, its
    // quantities and its material.
    void finishElement(const Accepted& item)
    {
        const RoadElement& element = *item.element;
        const std::string key = keyOf(element);
        ++b_.report().roadElements;
        b_.tally(sourceOf(element), element.ifcClass, {}, whyOf(element));

        PropertyList own;
        own.identifier("Id", element.id);
        own.label("Kind", element.kind);
        own.label("Facility", element.facility);
        own.label("Part", element.part);
        own.label("Layer", element.layer);
        own.label("Material", element.material);
        own.boolean("ClosedSolid", item.body.closed);
        b_.defines(key + "/element", b_.propertySet(key + "/element", "Katana_RoadElement", own),
                   {item.product});

        PropertyList properties;
        std::set<std::string> named;
        for (const auto& [name, value] : element.properties) {
            if (name.empty() || characterCount(name) > 255) {
                ++unnamed_;
                continue;
            }
            if (!named.insert(name).second) {
                ++repeated_;
                continue;
            }
            properties.label(name, value);
        }
        b_.defines(key + "/properties",
                   b_.propertySet(key + "/properties", "Katana_RoadProperties", properties),
                   {item.product});

        writeQuantities(item, key);
        // A void takes no material (IfcRelAssociatesMaterial's NoVoidElement):
        // a cut's - the ground it removes - is its Katana_RoadElement Material.
        if (!element.material.empty() && !item.voids) {
            auto& objects = materials_[element.material];
            if (objects.empty()) {
                materialOrder_.push_back(element.material);
            }
            objects.push_back(item.product);
        }
    }

    void writeQuantities(const Accepted& item, const std::string& key)
    {
        const RoadElement& element = *item.element;
        const BaseQuantities* base = baseQuantitiesOf(element.ifcClass.entity);
        std::vector<Id> standard;
        std::vector<Id> own;
        std::set<std::string> named;
        for (const RoadQuantity& quantity : element.quantities) {
            const auto value = measured(quantity);
            if (!value) {
                units_.insert(quantity.unit);
                continue;
            }
            if (quantity.name.empty() || !std::isfinite(value->value) || value->value < 0.0) {
                ++invalidQuantities_; // the schema's WR: a quantity's value is not negative
                continue;
            }
            if (!named.insert(quantity.name).second) {
                ++repeatedQuantities_; // UniqueQuantityNames, within the element
                continue;
            }
            Args args;
            args.string(b_.label(quantity.name, "a quantity of " + element.id)).null();
            if (value->kind == QuantityKind::Weight) {
                args.ref(b_.kilogram());
            } else {
                args.null(); // the project's unit (writeProjectFrame)
            }
            args.real(value->value).null();
            const Id id = b_.file().add(entityOf(value->kind), args);
            (base != nullptr && templateDefines(*base, quantity.name, value->kind) ? standard : own)
                .push_back(id);
        }
        const auto set = [&](std::string_view name, std::string_view method,
                             const std::vector<Id>& quantities) {
            if (quantities.empty()) {
                return;
            }
            const std::string setKey = key + "/" + std::string(name);
            const Id id = b_.file().add("IfcElementQuantity", Args()
                                                                  .string(b_.guid("qto/" + setKey))
                                                                  .null()
                                                                  .string(name)
                                                                  .null()
                                                                  .stringOrNull(method)
                                                                  .refs(quantities));
            b_.defines(setKey, id, {item.product});
        };
        if (base != nullptr) {
            set(base->set, "BaseQuantities", standard);
        }
        set("Katana_Quantities", {}, own);
    }

    void writeAdherences()
    {
        for (const auto& [host, adhering] : adherences_) {
            b_.file().add("IfcRelAdheresToElement", Args()
                                                        .string(b_.guid("adheres/" + adhering.key))
                                                        .null()
                                                        .null()
                                                        .null()
                                                        .ref(host)
                                                        .refs(adhering.features));
        }
    }

    void writeMaterials()
    {
        for (const std::string& name : materialOrder_) {
            const Id material = b_.file().add(
                "IfcMaterial", Args().string(b_.label(name, "the material " + name)).null().null());
            b_.file().add("IfcRelAssociatesMaterial", Args()
                                                          .string(b_.guid("material/" + name))
                                                          .null()
                                                          .null()
                                                          .null()
                                                          .refs(materials_[name])
                                                          .ref(material));
        }
    }

    // What was passed over in many elements, said once rather than once an
    // element.
    void sayOnce()
    {
        if (open_ > 0) {
            b_.warn(std::to_string(open_) +
                    " road elements are not closed solids - an edge is not shared by exactly two "
                    "faces - and are written as open face sets");
        }
        if (!units_.empty()) {
            std::string listed;
            for (const std::string& unit : units_) {
                listed += (listed.empty() ? "\"" : ", \"") + unit + "\"";
            }
            b_.warn("road element quantities in " + listed +
                    " are left out: a quantity is in m, m2, m3, kg or t, or is a count");
        }
        if (invalidQuantities_ > 0) {
            b_.warn(std::to_string(invalidQuantities_) +
                    " road element quantities are negative, not finite or unnamed and are left out");
        }
        if (repeatedQuantities_ > 0 || repeated_ > 0) {
            b_.warn(std::to_string(repeatedQuantities_ + repeated_) +
                    " road element quantities or properties repeat a name on their element; the "
                    "first of each is written");
        }
        if (unnamed_ > 0) {
            b_.warn(std::to_string(unnamed_) +
                    " road element properties have no name, or one longer than IFC's 255 "
                    "characters, and are left out");
        }
    }

    struct Adhering {
        std::string key;
        std::vector<Id> features;
    };

    Builder& b_;
    const RoadModelInput& road_;
    const std::map<std::string, Id>& terrain_;
    std::map<std::string, Facility> facilities_;
    std::map<std::string, std::vector<std::string>> partOrder_;
    Container site_;
    std::map<std::string, const Accepted*> written_;
    std::map<Id, Id> featurePlacements_; // a host's placement -> its features'
    std::map<Id, Adhering> adherences_;
    std::map<std::string, Id> grounds_;
    std::map<std::string, std::vector<Id>> materials_;
    std::vector<std::string> materialOrder_;
    std::set<std::string> units_;
    std::size_t open_ = 0;
    std::size_t invalidQuantities_ = 0;
    std::size_t repeatedQuantities_ = 0;
    std::size_t repeated_ = 0;
    std::size_t unnamed_ = 0;
};

} // namespace

void exportRoad(Builder& builder, const RoadModelInput& road,
                const std::map<std::string, Id>& terrain)
{
    RoadWriter(builder, road, terrain).write();
}

} // namespace katana::ifc::detail
