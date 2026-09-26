// Drawing entities -> IFC, by classification (classification.hpp).
//
// An entity becomes the class its rule names, or an IfcAnnotation. Its
// geometry is written as the class expects it:
//
//   an element's run (kerb, fence, pipe)   Axis Curve3D where every vertex
//                                          has a height, else FootPrint
//                                          Curve2D
//   an element at a point (pit, valve)     FootPrint GeometricCurveSet: the
//                                          point, and the outline when a
//                                          circle drew it
//   an annotation                          Annotation: Point, Curve2D or
//                                          Curve3D, or Annotation2D for
//                                          text, dimensions and leaders
//
// Arcs are IfcIndexedPolyCurve arcs through three points of the true arc and
// circles IfcCircle: never chords. A height is written where Katana has one
// (entity::heightsOf) and nowhere else.
//
// DRAINAGE from a .12da archive (archive12d/domain.hpp: the line carries its pipes as
// pipe.<i>.* properties, each pit is a point carrying pit.*) is taken as the
// network it is: pipe i an IfcPipeSegment from vertex i to vertex i + 1 of
// the line, its centre at its inverts plus half its diameter, and each pit
// an IfcDistributionChamberElement from its lowest connected invert to its
// top, all in one IfcDistributionSystem per string.
//
// A SERVICES PLAN drawn from an AS 5488 schedule (UTILITY DRAW,
// docs/subsurface_utilities.md, "Drawing the services": each run and point
// carries what the grading found as utility.* properties) is laid out for a
// plan, a layer per type and quality level. It is written as the service it
// is: each run the class the schedule's own export gives the service
// (classifyUtilityRun, from the same attributes), each point an IfcAnnotation
// SURVEY, all of a service in one IfcDistributionSystem named by its line,
// and each classified by its quality level in AS 5488.1-2019.
//
// Each entity keeps its layer (IfcPresentationLayerAssignment), its colour
// (a curve style), its properties (Katana_Attributes) and where it came from
// (Katana_Provenance). Labels are not written: what a label says and where
// it stands are worked out for a view by the placer (cad/annotation), which
// this layer cannot see; they are counted and reported.

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <map>
#include <set>

#include "katana/core/text.hpp"
#include "katana/math/numerics.hpp"
#include "katana/survey/subsurface/utility_network.hpp"
#include "parts.hpp"

namespace katana::ifc::detail {

namespace {

using entity::Entity;
using entity::EntityType;
using geometry::Point2;

// Two consecutive points closer than the model context's precision are one
// point to every reader (GEM111), and are written once.
constexpr double kSamePoint = 1e-5;

std::string text(const entity::PropertyMap& map, std::string_view key)
{
    const auto found = map.find(key);
    return found == map.end() ? std::string() : entity::toString(found->second);
}

std::optional<double> number(const entity::PropertyMap& map, std::string_view key)
{
    const auto found = map.find(key);
    if (found == map.end()) {
        return std::nullopt;
    }
    if (const auto* real = std::get_if<double>(&found->second)) {
        return *real;
    }
    if (const auto* whole = std::get_if<std::int64_t>(&found->second)) {
        return static_cast<double>(*whole);
    }
    if (const auto* words = std::get_if<std::string>(&found->second)) {
        return core::parseFiniteDouble(*words);
    }
    return std::nullopt;
}

std::string formatNumber(double value)
{
    char buffer[48];
    std::snprintf(buffer, sizeof buffer, "%.3f", value);
    return buffer;
}

// The one height of an entity whose vertices all share it.
std::optional<double> uniformHeight(const std::vector<std::optional<double>>& heights)
{
    if (heights.empty() || !heights.front()) {
        return std::nullopt;
    }
    for (const auto& height : heights) {
        if (!height || *height != *heights.front()) {
            return std::nullopt;
        }
    }
    return heights.front();
}

bool allHeights(const std::vector<std::optional<double>>& heights)
{
    return !heights.empty() &&
           std::all_of(heights.begin(), heights.end(), [](const auto& h) { return h.has_value(); });
}

// The IfcDistributionSystemEnum values; anything else a system is called is
// written USERDEFINED with that name as its object type.
bool isSystemEnumeration(std::string_view value)
{
    static const std::set<std::string_view> kValues{"AIRCONDITIONING",
                                                    "AUDIOVISUAL",
                                                    "CATENARY_SYSTEM",
                                                    "CHEMICAL",
                                                    "CHILLEDWATER",
                                                    "COMMUNICATION",
                                                    "COMPRESSEDAIR",
                                                    "CONDENSERWATER",
                                                    "CONTROL",
                                                    "CONVEYING",
                                                    "DATA",
                                                    "DISPOSAL",
                                                    "DOMESTICCOLDWATER",
                                                    "DOMESTICHOTWATER",
                                                    "DRAINAGE",
                                                    "EARTHING",
                                                    "ELECTRICAL",
                                                    "ELECTROACOUSTIC",
                                                    "EXHAUST",
                                                    "FIREPROTECTION",
                                                    "FIXEDTRANSMISSIONNETWORK",
                                                    "FUEL",
                                                    "GAS",
                                                    "HAZARDOUS",
                                                    "HEATING",
                                                    "LIGHTING",
                                                    "LIGHTNINGPROTECTION",
                                                    "MOBILENETWORK",
                                                    "MONITORINGSYSTEM",
                                                    "MUNICIPALSOLIDWASTE",
                                                    "OIL",
                                                    "OPERATIONAL",
                                                    "OPERATIONALTELEPHONYSYSTEM",
                                                    "OVERHEAD_CONTACTLINE_SYSTEM",
                                                    "POWERGENERATION",
                                                    "RAINWATER",
                                                    "REFRIGERATION",
                                                    "RETURN_CIRCUIT",
                                                    "SECURITY",
                                                    "SEWAGE",
                                                    "SIGNAL",
                                                    "STORMWATER",
                                                    "TELEPHONE",
                                                    "TV",
                                                    "VACUUM",
                                                    "VENT",
                                                    "VENTILATION",
                                                    "WASTEWATER",
                                                    "WATERSUPPLY",
                                                    "USERDEFINED",
                                                    "NOTDEFINED"};
    return kValues.contains(value);
}

// A .12da pit type ("1050 dia Access Chamber", "Headwall") as a chamber type.
IfcClass pitClass(std::string_view type)
{
    const std::string words = [&] {
        std::string out;
        for (const char c : type) {
            out.push_back(std::isalnum(static_cast<unsigned char>(c)) != 0
                              ? static_cast<char>(std::toupper(static_cast<unsigned char>(c)))
                              : ' ');
        }
        return " " + out + " ";
    }();
    const auto has = [&](std::string_view word) {
        return words.find(" " + std::string(word) + " ") != std::string::npos;
    };
    if (has("MANHOLE") || has("MH")) {
        return {"IfcDistributionChamberElement", "MANHOLE", {}};
    }
    if (has("SUMP")) {
        return {"IfcDistributionChamberElement", "SUMP", {}};
    }
    if (has("CHAMBER")) {
        return {"IfcDistributionChamberElement", "INSPECTIONCHAMBER", {}};
    }
    if (has("PIT") || has("GULLY") || has("INLET") || has("SIP") || has("KIP") || has("GRATE")) {
        return {"IfcDistributionChamberElement", "INSPECTIONPIT", {}};
    }
    if (has("HEADWALL")) {
        // The structure a pipe discharges through at its outfall: a
        // drainage structure IFC has no type of chamber for.
        return {"IfcDistributionChamberElement", "USERDEFINED", "HEADWALL"};
    }
    return {"IfcDistributionChamberElement", "NOTDEFINED", {}};
}

class DrawingWriter {
  public:
    DrawingWriter(Builder& builder, const entity::Model& model,
                  const std::vector<ClassificationRule>& rules)
        : b_(builder), model_(model), rules_(rules)
    {
        // A project's own rules: what the caller gave that is not one of the
        // defaults (a RULES file's, which the front ends put before them).
        const auto& defaults = defaultClassificationRules();
        for (const ClassificationRule& rule : rules_) {
            if (std::find(defaults.begin(), defaults.end(), rule) == defaults.end()) {
                projectRules_.push_back(rule);
            }
        }
    }

    void write()
    {
        std::vector<const Entity*> chosen;
        model_.entities.forEach([&](const Entity& entity) {
            if (selected(entity)) {
                chosen.push_back(&entity);
            }
        });
        writeDrawnServices(chosen);
        writeDrainage(chosen);
        for (const Entity* entity : chosen) {
            if (handled_.contains(entity->id)) {
                continue;
            }
            writeEntity(*entity);
        }
        writeSystems();
        if (labelsSkipped_ > 0) {
            b_.warn(std::to_string(labelsSkipped_) +
                    (labelsSkipped_ == 1 ? " label was" : " labels were") +
                    " not written: a label's words and place are worked out for a view");
        }
    }

  private:
    [[nodiscard]] bool selected(const Entity& entity) const
    {
        const ExportOptions& options = b_.options();
        if (!options.entities.empty() && std::find(options.entities.begin(), options.entities.end(),
                                                   entity.id) == options.entities.end()) {
            return false;
        }
        if (!options.layers.empty() && std::find(options.layers.begin(), options.layers.end(),
                                                 entity.layer) == options.layers.end()) {
            return false;
        }
        return true;
    }

    [[nodiscard]] entity::Color colourOf(const Entity& entity) const
    {
        if (entity.color) {
            return *entity.color;
        }
        const entity::Layer* layer = model_.layers.find(entity.layer);
        return layer ? layer->color : entity::Color{};
    }

    [[nodiscard]] static std::string nameOf(const Entity& entity)
    {
        for (const std::string& candidate :
             {text(entity.metadata, "12d.name"), text(entity.properties, "point"),
              text(entity.properties, "code")}) {
            if (!candidate.empty()) {
                return candidate;
            }
        }
        return std::string(entity::toString(entity.type())) + " " + std::to_string(entity.id);
    }

    // ---- geometry ---------------------------------------------------------------

    // A polyline through `plan` at `heights` (all present, or ignored),
    // consecutive repeats dropped; closed by referring back to its first
    // point, as GEM111 asks. 0 when fewer than two points remain.
    Id curve(const std::vector<Point2>& plan, const std::vector<std::optional<double>>& heights,
             bool closed, bool in3d)
    {
        std::vector<Id> points;
        std::optional<Vec3> previous;
        const std::size_t count = plan.size();
        std::optional<Vec3> first;
        for (std::size_t i = 0; i < count; ++i) {
            const Vec3 at(plan[i].x, plan[i].y, in3d ? *heights[i] : 0.0);
            if (previous && (at - *previous).length() < kSamePoint) {
                continue;
            }
            previous = at;
            if (!first) {
                first = at;
            }
            points.push_back(in3d ? b_.point(at) : b_.point(Vec2(at.x, at.y)));
        }
        // A last point on the first is the closing point, whether or not the
        // polyline says it is closed.
        if (points.size() > 2 && first && previous && (*previous - *first).length() < kSamePoint) {
            points.pop_back();
            closed = true;
        }
        if (points.size() < 2) {
            return 0;
        }
        if (closed && points.size() > 2) {
            points.push_back(points.front());
        }
        return b_.file().add("IfcPolyline", Args().refs(points));
    }

    // An arc as an IfcIndexedPolyCurve through its start, middle and end.
    Id arc(const geometry::Arc2& shape, std::optional<double> height)
    {
        const Point2 points[3] = {shape.startPoint(), shape.midpoint(), shape.endPoint()};
        std::vector<std::string> coordinates;
        for (const Point2& p : points) {
            if (height) {
                const Vec3 at = b_.local(Vec3(p.x, p.y, *height));
                coordinates.push_back(listOf({stepReal(at.x), stepReal(at.y), stepReal(at.z)}));
            } else {
                const Vec2 at = b_.local(p);
                coordinates.push_back(listOf({stepReal(at.x), stepReal(at.y)}));
            }
        }
        const Id list =
            b_.file().add(height ? "IfcCartesianPointList3D" : "IfcCartesianPointList2D",
                          Args().raw(listOf(coordinates)).null());
        return b_.file().add("IfcIndexedPolyCurve",
                             Args().ref(list).raw("(IFCARCINDEX((1,2,3)))").boolean(false));
    }

    // A curve polyline as ONE IfcIndexedPolyCurve: an IfcLineIndex per
    // straight segment and an IfcArcIndex through the true midpoint of each
    // arc, so its arcs stay arcs. The midpoints follow the vertices in the
    // point list; a midpoint's height is its ends' mean, the height halfway
    // along the segment by the rule the 3D view uses (heightAtStation).
    // 0 with fewer than two vertices.
    Id curvePolyline(const geometry::CurvePolyline2& shape, bool in3d)
    {
        if (shape.vertices.size() < 2) {
            return 0;
        }
        std::vector<std::string> coordinates;
        const auto add = [&](const Point2& p, double z) {
            if (in3d) {
                const Vec3 at = b_.local(Vec3(p.x, p.y, z));
                coordinates.push_back(listOf({stepReal(at.x), stepReal(at.y), stepReal(at.z)}));
            } else {
                const Vec2 at = b_.local(p);
                coordinates.push_back(listOf({stepReal(at.x), stepReal(at.y)}));
            }
            return std::to_string(coordinates.size()); // IFC indices count from 1
        };
        const auto heightOf = [&](std::size_t vertex) {
            return in3d ? *shape.vertices[vertex].height : 0.0;
        };
        std::vector<std::string> vertexIndex;
        for (std::size_t i = 0; i < shape.vertices.size(); ++i) {
            vertexIndex.push_back(add(shape.vertices[i].position, heightOf(i)));
        }
        std::vector<std::string> segments;
        for (std::size_t i = 0; i < shape.segmentCount(); ++i) {
            const std::size_t end = shape.segmentEnd(i);
            const auto piece = shape.segment(i);
            if (const auto* arcPiece = std::get_if<geometry::Arc2>(&piece)) {
                const std::string middle =
                    add(arcPiece->midpoint(), (heightOf(i) + heightOf(end)) * 0.5);
                segments.push_back("IFCARCINDEX((" + vertexIndex[i] + "," + middle + "," +
                                   vertexIndex[end] + "))");
            } else {
                segments.push_back("IFCLINEINDEX((" + vertexIndex[i] + "," + vertexIndex[end] +
                                   "))");
            }
        }
        const Id list =
            b_.file().add(in3d ? "IfcCartesianPointList3D" : "IfcCartesianPointList2D",
                          Args().raw(listOf(coordinates)).null());
        return b_.file().add("IfcIndexedPolyCurve",
                             Args().ref(list).raw(listOf(segments)).boolean(false));
    }

    // An ellipse as IfcEllipse, its placement's x axis along the major axis;
    // an elliptical arc as that ellipse trimmed by parameter. IFC's conic
    // parameter is the eccentric anomaly in the file's plane angle unit
    // (radians, builder.cpp), which is exactly Ellipse2's startParameter
    // and sweep, so the trim needs no conversion.
    Id ellipse(const geometry::Ellipse2& shape, std::optional<double> height)
    {
        const double length = shape.majorAxis.length();
        const double ux = shape.majorAxis.x / length;
        const double uy = shape.majorAxis.y / length;
        Id placement = 0;
        if (height) {
            const Id centre = b_.point(Vec3(shape.center.x, shape.center.y, *height));
            placement = b_.file().add("IfcAxis2Placement3D",
                                      Args().ref(centre).null().ref(b_.direction(ux, uy, 0.0)));
        } else {
            placement = b_.file().add(
                "IfcAxis2Placement2D",
                Args().ref(b_.point(Vec2(shape.center.x, shape.center.y))).ref(b_.direction(ux, uy)));
        }
        const Id basis = b_.file().add(
            "IfcEllipse", Args().ref(placement).real(shape.majorRadius()).real(shape.minorRadius()));
        if (shape.isFull()) {
            return basis;
        }
        return b_.file().add(
            "IfcTrimmedCurve",
            Args()
                .ref(basis)
                .raw("(IFCPARAMETERVALUE(" + stepReal(shape.startParameter) + "))")
                .raw("(IFCPARAMETERVALUE(" + stepReal(shape.endParameter()) + "))")
                .boolean(true)
                .enumeration("PARAMETER"));
    }

    // A spline as IfcBSplineCurveWithKnots - IfcRationalBSplineCurveWithKnots
    // when it has weights - from its control points and knots, the knots
    // written as IFC asks: each distinct value once, with its multiplicity.
    // Its fit points are how it was drawn, not what it is, and IFC has no
    // place for them.
    Id spline(const geometry::Spline2& shape)
    {
        std::vector<Id> points;
        for (const Point2& p : shape.controlPoints) {
            points.push_back(b_.point(Vec2(p.x, p.y)));
        }
        std::vector<std::string> multiplicities;
        std::vector<double> knots;
        for (std::size_t i = 0; i < shape.knots.size();) {
            std::size_t j = i;
            while (j < shape.knots.size() && shape.knots[j] == shape.knots[i]) {
                ++j;
            }
            multiplicities.push_back(std::to_string(j - i));
            knots.push_back(shape.knots[i]);
            i = j;
        }
        Args args;
        args.integer(shape.degree)
            .refs(points)
            .enumeration("UNSPECIFIED")
            .raw(".F.")
            .raw(".U.")
            .raw(listOf(multiplicities))
            .reals(knots)
            .enumeration("UNSPECIFIED");
        if (shape.weights.empty()) {
            return b_.file().add("IfcBSplineCurveWithKnots", args);
        }
        args.reals(shape.weights);
        return b_.file().add("IfcRationalBSplineCurveWithKnots", args);
    }

    Id circle(const geometry::Circle2& shape, std::optional<double> height)
    {
        Id placement = 0;
        if (height) {
            const Id centre = b_.point(Vec3(shape.center.x, shape.center.y, *height));
            placement = b_.file().add("IfcAxis2Placement3D", Args().ref(centre).null().null());
        } else {
            placement =
                b_.file().add("IfcAxis2Placement2D",
                              Args().ref(b_.point(Vec2(shape.center.x, shape.center.y))).null());
        }
        return b_.file().add("IfcCircle", Args().ref(placement).real(shape.radius));
    }

    // Text, as IfcTextLiteralWithExtent: IFC 4.3 deprecates IfcTextLiteral
    // (IFC102), and the extent is what the current one asks for. There is no
    // font here, so the box is ESTIMATED as the .12da import estimates a
    // text's width - 0.6 of the height per character of its longest line -
    // and a line's height per line; the corner the box alignment names sits
    // on the text's justification point, as Katana places it. Written in
    // plan: an Annotation2D's text is two-dimensional.
    Id textLiteral(const entity::TextGeometry& shape)
    {
        std::size_t lines = 1;
        std::size_t widest = 0;
        std::size_t current = 0;
        for (std::size_t at = 0; at <= shape.text.size(); ++at) {
            if (at == shape.text.size() || shape.text[at] == '\n') {
                widest = std::max(widest, current);
                current = 0;
                if (at < shape.text.size()) {
                    ++lines;
                }
            } else if ((static_cast<unsigned char>(shape.text[at]) & 0xC0) != 0x80) {
                ++current; // a character, not a UTF-8 continuation byte
            }
        }
        const Id placement = b_.file().add(
            "IfcAxis2Placement2D",
            Args()
                .ref(b_.point(Vec2(shape.position.x, shape.position.y)))
                .ref(b_.direction(std::cos(shape.rotation), std::sin(shape.rotation))));
        const Id extent = b_.file().add(
            "IfcPlanarExtent",
            Args()
                .real(0.6 * shape.height * static_cast<double>(std::max<std::size_t>(widest, 1)))
                .real(shape.height * static_cast<double>(lines)));
        return b_.file().add("IfcTextLiteralWithExtent", Args()
                                                             .string(shape.text)
                                                             .ref(placement)
                                                             .enumeration("RIGHT")
                                                             .ref(extent)
                                                             .string(boxAlignment(shape.justify)));
    }

    // IfcBoxAlignment's words for a Katana justification.
    static std::string_view boxAlignment(entity::TextJustify justify)
    {
        using J = entity::TextJustify;
        switch (justify) {
        case J::BottomLeft:
            return "bottom-left";
        case J::BottomCentre:
            return "bottom-middle";
        case J::BottomRight:
            return "bottom-right";
        case J::MiddleLeft:
            return "middle-left";
        case J::MiddleCentre:
            return "center";
        case J::MiddleRight:
            return "middle-right";
        case J::TopLeft:
            return "top-left";
        case J::TopCentre:
            return "top-middle";
        case J::TopRight:
            return "top-right";
        }
        return "bottom-left";
    }

    // The value of a dimension, or a leader's note, at the entity model's
    // default text height (TextGeometry::height): a dimension's own text
    // size is on paper, and this layer has no plot scale to turn it into
    // model units.
    Id textAt(Point2 at, double rotation, const std::string& words)
    {
        entity::TextGeometry shape;
        shape.position = at;
        shape.rotation = rotation;
        shape.text = words;
        return textLiteral(shape);
    }

    struct Drawn {
        std::vector<Id> items;
        std::string type; // the RepresentationType
        bool in3d = false;
    };

    // The geometry of `entity` as IFC representation items.
    Drawn draw(const Entity& entity)
    {
        Drawn drawn;
        std::visit(
            [&](const auto& shape) {
                using T = std::decay_t<decltype(shape)>;
                if constexpr (std::is_same_v<T, entity::PointGeometry>) {
                    const auto heights = entity::heightsOf(entity.properties, 1);
                    drawn.in3d = heights.front().has_value();
                    drawn.items.push_back(
                        drawn.in3d
                            ? b_.point(Vec3(shape.position.x, shape.position.y, *heights.front()))
                            : b_.point(Vec2(shape.position.x, shape.position.y)));
                    drawn.type = "Point";
                } else if constexpr (std::is_same_v<T, geometry::Segment2>) {
                    const auto heights = entity::heightsOf(entity.properties, 2);
                    drawn.in3d = allHeights(heights);
                    if (const Id id = curve({shape.start, shape.end}, heights, false, drawn.in3d)) {
                        drawn.items.push_back(id);
                    }
                } else if constexpr (std::is_same_v<T, geometry::Polyline2>) {
                    const auto heights =
                        entity::heightsOf(entity.properties, shape.vertices.size());
                    drawn.in3d = allHeights(heights);
                    if (const Id id = curve(shape.vertices, heights, shape.closed, drawn.in3d)) {
                        drawn.items.push_back(id);
                    }
                } else if constexpr (std::is_same_v<T, geometry::Arc2>) {
                    const auto height = uniformHeight(entity::heightsOf(entity.properties, 2));
                    drawn.in3d = height.has_value();
                    drawn.items.push_back(arc(shape, height));
                } else if constexpr (std::is_same_v<T, geometry::Circle2>) {
                    const auto height = uniformHeight(entity::heightsOf(entity.properties, 1));
                    drawn.in3d = height.has_value();
                    drawn.items.push_back(circle(shape, height));
                } else if constexpr (std::is_same_v<T, entity::TextGeometry>) {
                    drawn.items.push_back(textLiteral(shape));
                    drawn.type = "Annotation2D";
                } else if constexpr (std::is_same_v<T, entity::DimensionGeometry>) {
                    drawDimension(shape, drawn);
                    drawn.type = "Annotation2D";
                } else if constexpr (std::is_same_v<T, entity::LeaderGeometry>) {
                    if (const Id id = curve(shape.vertices, {}, false, false)) {
                        drawn.items.push_back(id);
                    }
                    if (!shape.text.empty()) {
                        drawn.items.push_back(textAt(shape.vertices.back(), 0.0, shape.text));
                    }
                    drawn.type = "Annotation2D";
                } else if constexpr (std::is_same_v<T, geometry::CurvePolyline2>) {
                    // Its heights are its vertices' own, not the elevation
                    // properties a Polyline2 keeps them in.
                    drawn.in3d = !shape.vertices.empty() &&
                                 std::all_of(shape.vertices.begin(), shape.vertices.end(),
                                             [](const geometry::CurveVertex& vertex) {
                                                 return vertex.height.has_value();
                                             });
                    if (const Id id = curvePolyline(shape, drawn.in3d)) {
                        drawn.items.push_back(id);
                    }
                } else if constexpr (std::is_same_v<T, geometry::Ellipse2>) {
                    const auto height = uniformHeight(entity::heightsOf(entity.properties, 1));
                    drawn.in3d = height.has_value();
                    drawn.items.push_back(ellipse(shape, height));
                } else if constexpr (std::is_same_v<T, geometry::Spline2>) {
                    drawn.items.push_back(spline(shape));
                } else {
                    static_assert(std::is_same_v<T, entity::LabelGeometry>);
                }
            },
            entity.geometry);
        if (drawn.type.empty() && !drawn.items.empty()) {
            drawn.type = drawn.in3d ? "Curve3D" : "Curve2D";
        }
        return drawn;
    }

    void drawDimension(const entity::DimensionGeometry& shape, Drawn& drawn)
    {
        const double measured = shape.measurement();
        const std::string words = shape.textOverride.empty()
                                      ? (shape.kind == entity::DimensionKind::Angular
                                             ? formatNumber(measured * math::kRadToDeg) + "\xC2\xB0"
                                             : formatNumber(measured))
                                      : shape.textOverride;
        if (shape.kind == entity::DimensionKind::Aligned) {
            const Vec2 along = shape.end - shape.start;
            const double length = along.length();
            if (length > 0.0) {
                const Vec2 left = Vec2(-along.y, along.x) * (shape.offset / length);
                const Point2 a = shape.start + left;
                const Point2 b = shape.end + left;
                for (const auto& line :
                     {std::vector<Point2>{shape.start, a}, std::vector<Point2>{shape.end, b},
                      std::vector<Point2>{a, b}}) {
                    if (const Id id = curve(line, {}, false, false)) {
                        drawn.items.push_back(id);
                    }
                }
                drawn.items.push_back(textAt((a + b) * 0.5, std::atan2(along.y, along.x), words));
                return;
            }
        }
        // The other kinds as the points they measure between, and the value.
        std::vector<Point2> points{shape.start};
        if (shape.usesVertex()) {
            points.push_back(shape.vertex);
        }
        points.push_back(shape.end);
        if (const Id id = curve(points, {}, false, false)) {
            drawn.items.push_back(id);
        }
        drawn.items.push_back(textAt(shape.end, 0.0, words));
    }

    // ---- entities -----------------------------------------------------------------

    void writeEntity(const Entity& entity)
    {
        if (entity.type() == EntityType::Label) {
            ++labelsSkipped_;
            ++b_.report().entitiesSkipped;
            b_.tally("layer " + entity.layer, {}, {}, "a label: labels are not exported");
            return;
        }
        const EntityClass classified = classifyEntity(entity, rules_);
        const bool annotation = classified.ifcClass.entity == "IfcAnnotation";
        Drawn drawn = draw(entity);
        if (drawn.items.empty()) {
            ++b_.report().entitiesSkipped;
            b_.warn(std::string(entity::toString(entity.type())) + " " + std::to_string(entity.id) +
                    " has no extent to write and is not written");
            b_.tally("layer " + entity.layer, {}, {}, "nothing to draw");
            return;
        }

        const Id representation = representationOf(entity, drawn, annotation);

        const std::string key = "entity/" + std::to_string(entity.id);
        const Id product = b_.product(classified.ifcClass, key, nameOf(entity), classified.rule,
                                      b_.productShape({representation}), std::to_string(entity.id));
        ++b_.report().entitiesWritten;
        b_.tally("layer " + entity.layer, classified.ifcClass, classified.system,
                 classified.rule.empty()
                     ? "no rule: a " + core::lowered(entity::toString(entity.type()))
                     : "rule " + classified.rule);

        if (!classified.system.empty()) {
            systems_[{entity.layer, classified.system}].push_back(product);
        }
        writeEntityProperties(entity, key, product,
                              classified.rule.empty() ? "kind" : "rule " + classified.rule,
                              classified.ifcClass);
    }

    // The entity's shape as its class expects it, styled in its colour and
    // on its layer.
    Id representationOf(const Entity& entity, const Drawn& drawn, bool annotation)
    {
        Id representation = 0;
        if (annotation) {
            representation =
                b_.shape(b_.annotationContext(), "Annotation", drawn.type, drawn.items);
        } else if (drawn.type == "Point" || entity.type() == EntityType::Circle) {
            // An element at a point, or outlined by a circle: its footprint.
            std::vector<Id> items = drawn.items;
            if (drawn.type == "Point" && drawn.in3d) {
                const auto& position = std::get<entity::PointGeometry>(entity.geometry).position;
                items = {b_.point(Vec2(position.x, position.y))};
            } else if (entity.type() == EntityType::Circle && drawn.in3d) {
                items = {circle(std::get<geometry::Circle2>(entity.geometry), std::nullopt)};
            }
            const Id set = b_.file().add("IfcGeometricCurveSet", Args().refs(items));
            representation =
                b_.shape(b_.footPrintContext(), "FootPrint", "GeometricCurveSet", {set});
        } else if (drawn.in3d) {
            representation = b_.shape(b_.axisContext(), "Axis", "Curve3D", drawn.items);
        } else {
            representation = b_.shape(b_.footPrintContext(), "FootPrint", "Curve2D", drawn.items);
        }
        if (drawn.type != "Point") {
            const entity::Color colour = colourOf(entity);
            for (const Id item : drawn.items) {
                b_.colourCurve(item, colour);
            }
        }
        b_.layer(entity.layer, representation);
        return representation;
    }

    // Its properties, and where it came from: `classifiedBy` says what chose
    // `ifcClass`.
    void writeEntityProperties(const Entity& entity, const std::string& key, Id product,
                               const std::string& classifiedBy, const IfcClass& ifcClass)
    {
        PropertyList attributes;
        addEntityProperties(attributes, entity.properties);
        b_.defines(key + "/attributes",
                   b_.propertySet(key + "/attributes", "Katana_Attributes", attributes), {product});

        PropertyList provenance;
        provenance.integer("EntityId", static_cast<long long>(entity.id));
        provenance.label("Layer", entity.layer);
        provenance.label("Style", entity.style);
        provenance.label("ClassifiedBy", classifiedBy);
        for (const auto& [name, value] : entity.metadata) {
            // What the archive knew and Katana keeps only to write back (12d.x.*,
            // symbols, text formatting) is not the entity's provenance.
            if (name.starts_with("12d.x.") || name.starts_with("12d.symbol.") ||
                name.starts_with("12d.text.")) {
                continue;
            }
            provenance.label(name, entity::toString(value));
        }
        b_.defines(key + "/provenance",
                   b_.propertySet(key + "/provenance", "Katana_Provenance", provenance), {product});

        if (ifcClass.predefinedType == "CONTOURLINE") {
            PropertyList contour;
            // A curve polyline's heights are its vertices' own.
            const auto* curve = std::get_if<geometry::CurvePolyline2>(&entity.geometry);
            contour.length("ContourValue",
                           uniformHeight(
                               curve != nullptr
                                   ? curve->heights()
                                   : entity::heightsOf(
                                         entity.properties,
                                         std::holds_alternative<geometry::Polyline2>(entity.geometry)
                                             ? std::get<geometry::Polyline2>(entity.geometry)
                                                   .vertices.size()
                                             : 2)));
            b_.defines(key + "/contour",
                       b_.propertySet(key + "/contour", "Pset_AnnotationContourLine", contour),
                       {product});
        }
    }

    // One IfcDistributionSystem per layer and system, for the service
    // elements the drawing's rules found.
    void writeSystems()
    {
        for (const auto& [where, members] : systems_) {
            const auto& [layer, system] = where;
            const bool listed = isSystemEnumeration(system);
            const std::string key = "system/" + layer + "/" + system;
            const Id id = b_.file().add("IfcDistributionSystem",
                                        Args()
                                            .string(b_.guid(key))
                                            .null()
                                            .string(layer)
                                            .null()
                                            .stringOrNull(listed ? "" : system)
                                            .null()
                                            .enumeration(listed ? system : "USERDEFINED"));
            ++b_.report().classes["IfcDistributionSystem"];
            b_.referenceInSite(id);
            b_.group(id, key, members);
        }
    }

    // ---- services drawn from an AS 5488 schedule -------------------------------------

    // Whether `entity` is part of a plan UTILITY DRAW made: the names of the
    // utility.* properties are the drawing's documented record
    // (docs/subsurface_utilities.md, "What the grading found is on the
    // entities"; cad/utilities/utility_drawing.hpp, which this layer cannot
    // see). A run is a polyline (a curve polyline when it has arcs), or a line an EXPLODE made of one, which
    // keeps its properties; a located point is a point.
    [[nodiscard]] static bool isDrawnRun(const Entity& entity)
    {
        return (std::holds_alternative<geometry::Polyline2>(entity.geometry) ||
                std::holds_alternative<geometry::CurvePolyline2>(entity.geometry) ||
                std::holds_alternative<geometry::Segment2>(entity.geometry)) &&
               !text(entity.properties, "utility.line").empty() &&
               !text(entity.properties, "utility.type").empty();
    }
    [[nodiscard]] static bool isDrawnPoint(const Entity& entity)
    {
        return std::holds_alternative<entity::PointGeometry>(entity.geometry) &&
               !text(entity.properties, "utility.line").empty() &&
               !text(entity.properties, "utility.type").empty();
    }

    // The service's attributes, as UTILITY DRAW put them on a run.
    [[nodiscard]] static survey::subsurface::UtilityAttributes attributesOf(const Entity& run)
    {
        namespace sub = survey::subsurface;
        const entity::PropertyMap& properties = run.properties;
        sub::UtilityAttributes service;
        service.type = sub::parseUtilityType(text(properties, "utility.type"))
                           .value_or(sub::UtilityType::Unknown);
        service.owner = text(properties, "utility.owner");
        service.material = text(properties, "utility.material");
        service.configuration = text(properties, "utility.configuration");
        service.description = text(properties, "utility.description");
        for (const auto& [name, value] : properties) {
            if (name.starts_with("utility.field.")) {
                service.fields[name.substr(std::string_view("utility.field.").size())] =
                    entity::toString(value);
            }
        }
        return service;
    }

    // What says one service from another among runs of one line id: two
    // schedules drawn under one prefix may both have an "E1", and E1 from
    // each is its own service, classed by its own attributes.
    [[nodiscard]] static std::string fingerprintOf(const Entity& run)
    {
        std::string out;
        for (const auto& [name, value] : run.properties) {
            if (name == "utility.type" || name == "utility.owner" || name == "utility.material" ||
                name == "utility.configuration" || name == "utility.description" ||
                name.starts_with("utility.field.")) {
                out += name + '\x1f' + entity::toString(value) + '\x1e';
            }
        }
        return out;
    }

    struct DrawnService {
        std::string drawnOn; // the <prefix>/<type> its runs were drawn under
        std::string line;    // utility.line
        std::string fingerprint;
        std::vector<const Entity*> runs;
        std::vector<const Entity*> points;
    };

    // The services of a drawn plan, then each written. A service is the runs
    // of one line id drawn under one <prefix>/<type> (the layer above their
    // QL layer) with the same attributes; each point goes to the service it
    // stands on - a vertex of one of its runs, as a pit goes to its drainage
    // string - whatever layer it has since been moved to, and only when it
    // stands on none to the one service of its line and type, or to its own
    // layer's.
    void writeDrawnServices(const std::vector<const Entity*>& chosen)
    {
        std::vector<DrawnService> services;
        for (const Entity* entity : chosen) {
            if (!isDrawnRun(*entity)) {
                continue;
            }
            const std::size_t slash = entity->layer.rfind('/');
            const std::string drawnOn =
                slash == std::string::npos ? std::string() : entity->layer.substr(0, slash);
            const std::string line = text(entity->properties, "utility.line");
            const std::string fingerprint = fingerprintOf(*entity);
            auto found =
                std::find_if(services.begin(), services.end(), [&](const DrawnService& service) {
                    return service.drawnOn == drawnOn && service.line == line &&
                           service.fingerprint == fingerprint;
                });
            if (found == services.end()) {
                services.push_back({drawnOn, line, fingerprint, {}, {}});
                found = std::prev(services.end());
            }
            found->runs.push_back(entity);
        }
        for (const Entity* entity : chosen) {
            if (!isDrawnPoint(*entity)) {
                continue;
            }
            const std::string line = text(entity->properties, "utility.line");
            const std::string type = text(entity->properties, "utility.type");
            const Point2 at = std::get<entity::PointGeometry>(entity->geometry).position;
            const auto sameLine = [&](const DrawnService& service) {
                return service.line == line &&
                       text(service.runs.front()->properties, "utility.type") == type;
            };
            const auto standsOn = [&](const DrawnService& service) {
                return sameLine(service) &&
                       std::any_of(service.runs.begin(), service.runs.end(),
                                   [&](const Entity* run) { return passesThrough(*run, at); });
            };
            auto owner = std::find_if(services.begin(), services.end(), standsOn);
            if (owner == services.end() &&
                std::count_if(services.begin(), services.end(), sameLine) == 1) {
                owner = std::find_if(services.begin(), services.end(), sameLine);
            }
            if (owner == services.end()) {
                const std::size_t slash = entity->layer.rfind('/');
                const std::string drawnOn =
                    slash == std::string::npos ? std::string() : entity->layer.substr(0, slash);
                owner = std::find_if(services.begin(), services.end(),
                                     [&](const DrawnService& service) {
                                         return service.runs.empty() &&
                                                service.drawnOn == drawnOn && service.line == line;
                                     });
                if (owner == services.end()) {
                    services.push_back({drawnOn, line, {}, {}, {}});
                    owner = std::prev(services.end());
                }
            }
            owner->points.push_back(entity);
        }

        // A line id drawn from services that differ is said, and each
        // written as its own system: "E1" and "E1 (2)" in the report.
        std::map<std::pair<std::string, std::string>, std::size_t> seen;
        for (const DrawnService& service : services) {
            const std::size_t nth = ++seen[{service.drawnOn, service.line}];
            if (nth == 2) {
                b_.warn("line " + service.line + " under " + service.drawnOn +
                        " was drawn from services with different attributes (two schedules "
                        "with one line id?): each is written as its own system");
            }
            writeDrawnService(service, nth);
        }
    }

    // Whether a vertex of `run` is at `at`, to the centimetre writeDrainage
    // gives a pit to its string with.
    [[nodiscard]] static bool passesThrough(const Entity& run, Point2 at)
    {
        const auto near = [&](Point2 vertex) { return (vertex - at).length() < 0.01; };
        if (const auto* polyline = std::get_if<geometry::Polyline2>(&run.geometry)) {
            return std::any_of(polyline->vertices.begin(), polyline->vertices.end(), near);
        }
        if (const auto* curve = std::get_if<geometry::CurvePolyline2>(&run.geometry)) {
            return std::any_of(curve->vertices.begin(), curve->vertices.end(),
                               [&](const geometry::CurveVertex& vertex) { return near(vertex.position); });
        }
        const auto& segment = std::get<geometry::Segment2>(run.geometry);
        return near(segment.start) || near(segment.end);
    }

    void writeDrawnService(const DrawnService& drawn, std::size_t nth)
    {
        namespace sub = survey::subsurface;
        // Every run of the service carries its attributes alike (they are its
        // fingerprint); a service drawn as points alone has only its type.
        const Entity& first = drawn.runs.empty() ? *drawn.points.front() : *drawn.runs.front();
        const sub::UtilityAttributes service = attributesOf(first);
        const UtilityClass runClass = classifyUtilityRun(service);
        const std::string systemName =
            runClass.system == "USERDEFINED" ? runClass.systemObjectType : runClass.system;
        const std::string numbered =
            nth == 1 ? drawn.line : drawn.line + " (" + std::to_string(nth) + ")";
        const std::string key = "drawn service/" + drawn.drawnOn + "/" + numbered;
        // Named apart from the schedule's "service W1" rows, and with where it
        // was drawn, so the report names each system the file holds once.
        const std::string source =
            "drawn service " + numbered + (drawn.drawnOn.empty() ? "" : " in " + drawn.drawnOn);
        if (b_.isScheduleService(drawn.line + "/" + sub::toString(service.type))) {
            b_.warn(source + " is also in the schedule given with UTILITIES: the file holds "
                             "it twice, drawn and graded; leave out one (NOENTITIES, or no "
                             "UTILITIES)");
        }

        std::vector<Id> written;
        for (const Entity* entity : drawn.runs) {
            written.push_back(
                writeDrawnMember(*entity, false, drawn, runClass, systemName, source));
        }
        for (const Entity* entity : drawn.points) {
            written.push_back(writeDrawnMember(*entity, true, drawn, runClass, systemName, source));
        }
        std::erase(written, Id{0});
        if (written.empty()) {
            return; // nothing of the service was written: no system of nothing
        }
        // As the schedule's export writes a service's system (utilities.cpp).
        const Id id =
            b_.file().add("IfcDistributionSystem", Args()
                                                       .string(b_.guid(key))
                                                       .null()
                                                       .string(b_.label(numbered, source))
                                                       .stringOrNull(service.description)
                                                       .stringOrNull(runClass.systemObjectType)
                                                       .stringOrNull(serviceLongName(service))
                                                       .enumeration(runClass.system));
        ++b_.report().classes["IfcDistributionSystem"];
        b_.referenceInSite(id);
        b_.group(id, key, written);
    }

    // One run or located point of a drawn service; 0 when it has nothing to
    // draw, which is said.
    Id writeDrawnMember(const Entity& entity, bool point, const DrawnService& drawn,
                        const UtilityClass& runClass, const std::string& systemName,
                        const std::string& source)
    {
        namespace sub = survey::subsurface;
        handled_.insert(entity.id);
        // A project's own rules still name the class of what it drew (a
        // "gas flexible" rule makes a gas run a FLEXIBLESEGMENT); only the
        // defaults give way to the service's own class.
        const EntityClass ruled = classifyEntity(entity, projectRules_);
        const bool byRule = !ruled.rule.empty();
        const IfcClass ifcClass = byRule  ? ruled.ifcClass
                                  : point ? IfcClass{"IfcAnnotation", "SURVEY", {}}
                                          : runClass.element;
        const std::string system = byRule && !ruled.system.empty() ? ruled.system : systemName;

        // A located point at its level where the drawing kept one (the
        // level the grading took it at, as the schedule's export places its
        // points); in plan where it did not.
        Drawn shape;
        const auto level =
            point ? number(entity.properties, "utility.service_level") : std::nullopt;
        if (level) {
            const Point2 at = std::get<entity::PointGeometry>(entity.geometry).position;
            shape.items = {b_.point(Vec3(at.x, at.y, *level))};
            shape.type = "Point";
            shape.in3d = true;
        } else {
            shape = draw(entity);
        }
        if (shape.items.empty()) {
            ++b_.report().entitiesSkipped;
            b_.warn(source + ": " + std::string(entity::toString(entity.type())) + " " +
                    std::to_string(entity.id) + " has no extent to write and is not written");
            b_.tally(source, {}, {}, "nothing to draw");
            return 0;
        }
        const bool annotation = ifcClass.entity == "IfcAnnotation";
        const Id representation = representationOf(entity, shape, annotation);

        // The level the drawing recorded, read as AS 5488's or not at all: a
        // level the standard does not have is said, and written unclassified.
        const std::string recorded = text(entity.properties, "utility.quality_level");
        const auto graded = sub::parseQualityLevel(recorded);
        if (!graded) {
            b_.warn(source + ": " + std::string(entity::toString(entity.type())) + " " +
                    std::to_string(entity.id) +
                    (recorded.empty() ? std::string(" has no quality level")
                                      : " has quality level \"" + recorded +
                                            "\", which is not one of AS 5488.1-2019's QL-A to "
                                            "QL-D") +
                    ": it is written unclassified");
        }
        const std::string entityKey = "entity/" + std::to_string(entity.id);
        const std::string name = point
                                     ? text(entity.properties, "utility.vertex")
                                     : drawn.line + " " + text(entity.properties, "utility.from") +
                                           " to " + text(entity.properties, "utility.to");
        const std::string description =
            point ? "Located point of " + drawn.line
            : graded
                ? std::string("AS 5488.1-2019 ") + sub::toString(*graded) + " run of " + drawn.line
                : "Run of " + drawn.line + ", its quality level not one of AS 5488's";
        const Id product =
            b_.product(ifcClass, entityKey, name.empty() ? drawn.line : name, description,
                       b_.productShape({representation}), std::to_string(entity.id));
        ++b_.report().entitiesWritten;
        b_.tally(source, ifcClass, system,
                 byRule  ? "rule " + ruled.rule
                 : point ? std::string("a drawn located point")
                         : "a drawn run: " + runClass.reason);
        if (graded) {
            b_.associate(qualityLevelReference(b_, *graded), product);
        }
        writeDrawnGrade(entity, entityKey, product, point, graded);
        writeEntityProperties(entity, entityKey, product,
                              byRule ? "rule " + ruled.rule : "a drawn AS 5488 service", ifcClass);
        return product;
    }

    // What the grading found, in the property sets the schedule's own
    // export writes (utilities.cpp), from what the drawing kept of it: a
    // run's AS5488_QualityLevel, a point's AS5488_LocatedPoint. What the
    // drawing does not keep - a run's claimed level, its path evidence, a
    // point's recorded level and depth - is absent, not guessed; the level
    // the grading took a point at, which the drawing keeps, is ServiceLevel,
    // since the schedule's Level is only a recorded one.
    void writeDrawnGrade(const Entity& entity, const std::string& key, Id product, bool point,
                         std::optional<survey::subsurface::QualityLevel> graded)
    {
        const entity::PropertyMap& p = entity.properties;
        const auto flag = [&](std::string_view name) -> std::optional<bool> {
            const auto found = p.find(name);
            if (found == p.end()) {
                return std::nullopt;
            }
            if (const auto* value = std::get_if<bool>(&found->second)) {
                return *value;
            }
            return std::nullopt;
        };
        PropertyList grade;
        if (graded) {
            grade.label("QualityLevel", survey::subsurface::toString(*graded),
                        point ? "The AS 5488.1-2019 quality level the point's evidence supports"
                              : "The AS 5488.1-2019 quality level the run's evidence supports");
        }
        if (point) {
            grade.label("LocateMethod", text(p, "utility.method"));
            grade.label("QualityLevelClaimed", text(p, "utility.claimed"));
            grade.text("OverClaim", text(p, "utility.over_claim"));
            grade.label("LevelReference", text(p, "utility.level_ref"));
            grade.length("ServiceLevel", number(p, "utility.service_level"));
            grade.boolean("LevelQualified", flag("utility.level_qualified"));
            grade.length("SurfaceLevel", number(p, "utility.surface_level"));
            grade.length("DepthOfCover", number(p, "utility.cover"));
            grade.text("CoverNote", text(p, "utility.cover_note"));
            grade.boolean("CoverBelowMinimum", flag("utility.cover_below_minimum"));
            grade.identifier("Verifies", text(p, "utility.verifies"));
        } else {
            grade.text("LimitedBy", text(p, "utility.limited_by"));
            grade.identifier("StartPoint", text(p, "utility.from"));
            grade.identifier("EndPoint", text(p, "utility.to"));
            grade.length("PlanLength", number(p, "utility.length"));
        }
        grade.label("Standard", "AS 5488.1-2019");
        const std::string name = point ? "AS5488_LocatedPoint" : "AS5488_QualityLevel";
        b_.defines(key + "/grade", b_.propertySet(key + "/grade", name, grade), {product});
    }

    // ---- drainage from a .12da archive -------------------------------------------

    void writeDrainage(const std::vector<const Entity*>& chosen)
    {
        // The strings, and their pits and house connections, by the string
        // they came from: the archive gives them one header (layer and name).
        std::map<std::pair<std::string, std::string>, std::vector<const Entity*>> parts;
        std::vector<const Entity*> lines;
        for (const Entity* entity : chosen) {
            if (handled_.contains(entity->id)) {
                continue; // already written, as a drawn service
            }
            const std::string element = text(entity->metadata, "12d.element");
            if (element == "string drainage" &&
                std::holds_alternative<geometry::Polyline2>(entity->geometry)) {
                lines.push_back(entity);
            } else if (element == "drainage pit" || element == "drainage house_connection") {
                parts[{entity->layer, text(entity->metadata, "12d.name")}].push_back(entity);
            }
        }
        // Each pit and connection goes to ONE string. The archive names a string's
        // parts by its header, and two strings may share a name - or have
        // none - so a part goes to the string of its name it stands on (a
        // vertex of it), and to the first of its name only when it stands on
        // none; otherwise every pit of the name would be written once per
        // string.
        std::map<const Entity*, std::vector<const Entity*>> partsOf;
        for (const auto& [key, members] : parts) {
            std::vector<const Entity*> named;
            for (const Entity* line : lines) {
                if (line->layer == key.first && text(line->metadata, "12d.name") == key.second) {
                    named.push_back(line);
                }
            }
            if (named.empty()) {
                continue; // no string of its name: written as the entity it is
            }
            for (const Entity* part : members) {
                const Entity* owner = named.front();
                if (const auto* point = std::get_if<entity::PointGeometry>(&part->geometry)) {
                    const auto standsOn = [&](const Entity* line) {
                        const auto& vertices =
                            std::get<geometry::Polyline2>(line->geometry).vertices;
                        return std::any_of(vertices.begin(), vertices.end(), [&](const Point2& v) {
                            return (v - point->position).length() < 0.01;
                        });
                    };
                    if (const auto found = std::find_if(named.begin(), named.end(), standsOn);
                        found != named.end()) {
                        owner = *found;
                    }
                }
                partsOf[owner].push_back(part);
            }
        }
        for (const Entity* line : lines) {
            writeDrainageString(*line, partsOf[line]);
        }
    }

    void writeDrainageString(const Entity& line, const std::vector<const Entity*>& parts)
    {
        const auto& shape = std::get<geometry::Polyline2>(line.geometry);
        const std::string name = nameOf(line);
        const std::string key = "drainage/" + std::to_string(line.id);
        std::string system = serviceSystemFor(classificationText(line));
        if (system.empty()) {
            system = "STORMWATER"; // an archive's drainage string with nothing else said drains
                                   // stormwater
        }
        const auto heights = entity::heightsOf(line.properties, shape.vertices.size());
        const auto flow = number(line.properties, "flow_direction");

        // The pipes, pipe i from vertex i to vertex i + 1.
        std::size_t pipes = 0;
        while (line.properties.contains("pipe." + std::to_string(pipes + 1) + ".diameter") ||
               line.properties.contains("pipe." + std::to_string(pipes + 1) + ".name")) {
            ++pipes;
        }
        std::vector<Id> members;
        // The lowest invert at each vertex, for the pits' depths.
        std::vector<std::optional<double>> invertAt(shape.vertices.size());
        if (pipes == 0 || pipes + 1 != shape.vertices.size()) {
            const Drawn drawn = draw(line);
            if (drawn.items.empty()) {
                // Nothing to draw: not written, and accounted for as not.
                b_.warn("drainage string \"" + name + "\" has " + std::to_string(pipes) +
                        " pipes for " + std::to_string(shape.vertices.size()) +
                        " vertices and no extent to write; it is not written");
                ++b_.report().entitiesSkipped;
                b_.tally("layer " + line.layer, {}, {}, "nothing to draw");
            } else {
                b_.warn("drainage string \"" + name + "\" has " + std::to_string(pipes) +
                        " pipes for " + std::to_string(shape.vertices.size()) +
                        " vertices; it is written as one pipe along its line");
                const Id representation =
                    drawn.in3d
                        ? b_.shape(b_.axisContext(), "Axis", "Curve3D", drawn.items)
                        : b_.shape(b_.footPrintContext(), "FootPrint", "Curve2D", drawn.items);
                b_.layer(line.layer, representation);
                members.push_back(b_.product(
                    {"IfcPipeSegment", "RIGIDSEGMENT", {}}, key + "/line", name, "drainage string",
                    b_.productShape({representation}), std::to_string(line.id)));
                ++b_.report().entitiesWritten;
                b_.tally("layer " + line.layer, {"IfcPipeSegment", "RIGIDSEGMENT", {}}, system,
                         "drainage string, its pipes not given: one pipe along it");
            }
        } else {
            for (std::size_t i = 0; i < pipes; ++i) {
                members.push_back(writePipe(line, i, shape, heights, flow, invertAt, key, system));
            }
            ++b_.report().entitiesWritten;
        }
        handled_.insert(line.id);

        for (const Entity* part : parts) {
            if (!std::holds_alternative<entity::PointGeometry>(part->geometry)) {
                continue;
            }
            members.push_back(writePit(*part, shape, invertAt, system));
            handled_.insert(part->id);
            ++b_.report().entitiesWritten;
        }
        if (members.empty()) {
            return; // nothing of the string was written: no system of nothing
        }
        const bool listed = isSystemEnumeration(system);
        const Id id = b_.file().add("IfcDistributionSystem",
                                    Args()
                                        .string(b_.guid(key))
                                        .null()
                                        .string(name)
                                        .string("drainage string")
                                        .stringOrNull(listed ? "" : system)
                                        .null()
                                        .enumeration(listed ? system : "USERDEFINED"));
        ++b_.report().classes["IfcDistributionSystem"];
        b_.referenceInSite(id);
        b_.group(id, key, members);
    }

    Id writePipe(const Entity& line, std::size_t index, const geometry::Polyline2& shape,
                 const std::vector<std::optional<double>>& heights, std::optional<double> flow,
                 std::vector<std::optional<double>>& invertAt, const std::string& key,
                 const std::string& system)
    {
        const std::string prefix = "pipe." + std::to_string(index + 1) + ".";
        const auto diameter = number(line.properties, prefix + "diameter");
        const auto upstream = number(line.properties, prefix + "us_level");
        const auto downstream = number(line.properties, prefix + "ds_level");
        const Point2 a = shape.vertices[index];
        const Point2 b = shape.vertices[index + 1];

        // Which end is upstream. The string's own heights at its vertices
        // say it where there are any; otherwise a flow_direction of 1 puts
        // it at the string's start, as every .12da file seen writes it. Any
        // other case is not guessed at.
        std::optional<std::pair<double, double>> inverts; // at a, at b
        std::string how;
        if (upstream && downstream) {
            if (heights[index] && heights[index + 1]) {
                const double forward = std::abs(*upstream - *heights[index]) +
                                       std::abs(*downstream - *heights[index + 1]);
                const double backward = std::abs(*upstream - *heights[index + 1]) +
                                        std::abs(*downstream - *heights[index]);
                inverts = forward <= backward ? std::pair(*upstream, *downstream)
                                              : std::pair(*downstream, *upstream);
                how = "upstream end by the string's vertex levels";
            } else if (flow && *flow == 1.0) {
                inverts = std::pair(*upstream, *downstream);
                how = "upstream end at the string's start (flow_direction 1)";
            } else {
                how = "the upstream end is not known: drawn in plan";
            }
        }
        if (inverts) {
            invertAt[index] =
                invertAt[index] ? std::min(*invertAt[index], inverts->first) : inverts->first;
            invertAt[index + 1] = invertAt[index + 1]
                                      ? std::min(*invertAt[index + 1], inverts->second)
                                      : inverts->second;
        }

        std::vector<Id> representations;
        if (inverts && diameter && *diameter > 0.0) {
            const double r = *diameter / 2.0;
            const Id axis = b_.polyline(std::vector<Vec3>{Vec3(a.x, a.y, inverts->first + r),
                                                          Vec3(b.x, b.y, inverts->second + r)},
                                        false);
            representations.push_back(b_.shape(b_.axisContext(), "Axis", "Curve3D", {axis}));
            const Id body =
                b_.file().add("IfcSweptDiskSolid", Args().ref(axis).real(r).null().null().null());
            representations.push_back(
                b_.shape(b_.bodyContext(), "Body", "AdvancedSweptSolid", {body}));
        } else {
            const Id footprint = b_.polyline(std::vector<Vec2>{a, b}, false);
            representations.push_back(
                b_.shape(b_.footPrintContext(), "FootPrint", "Curve2D", {footprint}));
        }
        for (const Id representation : representations) {
            b_.layer(line.layer, representation);
        }
        const std::string pipeName = text(line.properties, prefix + "name");
        const std::string type = text(line.properties, prefix + "type");
        IfcClass pipeClass{"IfcPipeSegment", "RIGIDSEGMENT", {}};
        if (containsWord(classificationText(line) + " " + core::lowered(type), "CULVERT*")) {
            pipeClass.predefinedType = "CULVERT";
        }
        const std::string pipeKey = key + "/pipe/" + std::to_string(index + 1);
        const Id pipe = b_.product(
            pipeClass, pipeKey,
            pipeName.empty() ? nameOf(line) + " pipe " + std::to_string(index + 1) : pipeName, type,
            b_.productShape(representations), pipeName);
        b_.tally("layer " + line.layer, pipeClass, system, "drainage pipe");

        PropertyList common;
        common.identifier("Reference", pipeName);
        common.positiveLength("NominalDiameter", diameter);
        common.positiveLength("Length", (b - a).length());
        b_.defines(pipeKey + "/common",
                   b_.propertySet(pipeKey + "/common", "Pset_PipeSegmentTypeCommon", common),
                   {pipe});
        if (inverts) {
            PropertyList occurrence;
            occurrence.length("InvertElevation", std::max(inverts->first, inverts->second));
            const double run = (b - a).length();
            if (run > 0.0) {
                occurrence.positiveRatio("Gradient",
                                         std::abs(inverts->first - inverts->second) / run);
            }
            b_.defines(
                pipeKey + "/occurrence",
                b_.propertySet(pipeKey + "/occurrence", "Pset_PipeSegmentOccurrence", occurrence),
                {pipe});
        }
        PropertyList drainage;
        entity::PropertyMap own;
        for (const auto& [name, value] : line.properties) {
            if (name.starts_with(prefix)) {
                own.emplace(name.substr(prefix.size()), value);
            }
        }
        addEntityProperties(drainage, own);
        drainage.label("LevelsFrom", how);
        b_.defines(pipeKey + "/drainage",
                   b_.propertySet(pipeKey + "/drainage", "Katana_DrainagePipe", drainage), {pipe});
        return pipe;
    }

    Id writePit(const Entity& part, const geometry::Polyline2& shape,
                const std::vector<std::optional<double>>& invertAt, const std::string& system)
    {
        const bool pit = text(part.metadata, "12d.element") == "drainage pit";
        const std::string prefix = pit ? "pit." : "house_connection.";
        const Point2 at = std::get<entity::PointGeometry>(part.geometry).position;
        const auto top = number(part.properties, entity::kElevationProperty);
        const auto diameter = number(part.properties, prefix + "diameter");

        // The pit's invert: the lowest pipe invert at the vertex it stands on.
        std::optional<double> invert;
        for (std::size_t i = 0; i < shape.vertices.size(); ++i) {
            if ((shape.vertices[i] - at).length() < 0.01 && invertAt[i]) {
                invert = invertAt[i];
            }
        }
        std::vector<Id> representations;
        if (pit && top && invert && diameter && *diameter > 0.0 && *top > *invert) {
            const Id axis = b_.polyline(
                std::vector<Vec3>{Vec3(at.x, at.y, *invert), Vec3(at.x, at.y, *top)}, false);
            const Id body = b_.file().add(
                "IfcSweptDiskSolid", Args().ref(axis).real(*diameter / 2.0).null().null().null());
            representations.push_back(
                b_.shape(b_.bodyContext(), "Body", "AdvancedSweptSolid", {body}));
        }
        std::vector<Id> items{b_.point(at)};
        if (diameter && *diameter > 0.0) {
            const Id placement =
                b_.file().add("IfcAxis2Placement2D", Args().ref(b_.point(at)).null());
            items.push_back(
                b_.file().add("IfcCircle", Args().ref(placement).real(*diameter / 2.0)));
        }
        const Id set = b_.file().add("IfcGeometricCurveSet", Args().refs(items));
        representations.push_back(
            b_.shape(b_.footPrintContext(), "FootPrint", "GeometricCurveSet", {set}));
        for (const Id representation : representations) {
            b_.layer(part.layer, representation);
        }
        const std::string type = text(part.properties, prefix + "type");
        const std::string name = text(part.properties, prefix + "name");
        const IfcClass ifcClass = pit ? pitClass(type) : IfcClass{"IfcPipeFitting", "JUNCTION", {}};
        const std::string key = "entity/" + std::to_string(part.id);
        const Id product = b_.product(ifcClass, key, name.empty() ? nameOf(part) : name, type,
                                      b_.productShape(representations), name);
        b_.tally("layer " + part.layer, ifcClass, system,
                 pit ? "drainage pit" : "drainage house connection");
        if (pit) {
            PropertyList common;
            common.identifier("Reference", name);
            b_.defines(
                key + "/common",
                b_.propertySet(key + "/common", "Pset_DistributionChamberElementCommon", common),
                {product});
            PropertyList size;
            size.positiveLength("NominalLength", diameter).positiveLength("NominalWidth", diameter);
            if (top && invert && *top > *invert) {
                size.positiveLength("NominalHeight", *top - *invert);
            }
            b_.defines(key + "/size", b_.propertySet(key + "/size", "Pset_ElementSize", size),
                       {product});
        }
        PropertyList drainage;
        entity::PropertyMap own;
        for (const auto& [field, value] : part.properties) {
            own.emplace(field.starts_with(prefix) ? field.substr(prefix.size()) : field, value);
        }
        addEntityProperties(drainage, own);
        drainage.length("InvertLevel", invert);
        b_.defines(key + "/drainage",
                   b_.propertySet(key + "/drainage",
                                  pit ? "Katana_DrainagePit" : "Katana_DrainageConnection",
                                  drainage),
                   {product});
        return product;
    }

    Builder& b_;
    const entity::Model& model_;
    const std::vector<ClassificationRule>& rules_;
    std::vector<ClassificationRule> projectRules_;
    std::set<entity::EntityId> handled_;
    std::map<std::pair<std::string, std::string>, std::vector<Id>> systems_;
    std::size_t labelsSkipped_ = 0;
};

} // namespace

void exportEntities(Builder& builder, const entity::Model& model,
                    const std::vector<ClassificationRule>& rules)
{
    DrawingWriter(builder, model, rules).write();
}

} // namespace katana::ifc::detail
