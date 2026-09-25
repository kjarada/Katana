// Alignments -> IfcAlignment, business logic and geometry both.
//
// IFC 4.3 describes an alignment twice, and a reader may use either: the
// BUSINESS LOGIC - IfcAlignmentHorizontal and IfcAlignmentVertical nesting
// IfcAlignmentSegment, each carrying the design parameters a designer
// states (start point and direction, radii, length; start distance, height
// and grades) - and the GEOMETRY - an IfcCompositeCurve of IfcCurveSegment,
// and over it an IfcGradientCurve. Katana holds neither: it holds PIs and
// PVIs and solves them (geometry/alignment.hpp, geometry/profile.hpp). Both
// descriptions are written from the SOLVED elements, one segment per
// element, so the two agree segment for segment (ALB022, ALB023) and both
// agree with what Katana draws.
//
// The encodings of each segment type follow IfcOpenShell's alignment API
// (ifcopenshell.api.alignment, 0.8), the reference implementation the
// buildingSMART validation service evaluates geometry with:
//
//   LINE          IfcLine through the origin along +x, from 0 for L
//   CIRCULARARC   IfcCircle of |R| at the origin, from 0 for L signed by
//                 the turn (negative: clockwise)
//   CLOTHOID      IfcClothoid with A = sign(dk) sqrt(L / |dk|), from
//                 k0 / (dk / L) for L - the length at which the parent
//                 spiral's curvature k(s) = s dk / L is the segment's start
//                 curvature k0
//   CONSTANTGRADIENT  IfcLine, for the length along the grade
//   PARABOLICARC  IfcPolynomialCurve y = A + B x + C x^2 with B the start
//                 grade and C = (g2 - g1) / 2L, for its arc length
//
// each positioned by an IfcAxis2Placement2D at the segment's start point in
// the segment's start direction. Radii and curvatures are positive turning
// left, as in Katana (spiral2.hpp) and in IFC.
//
// Each layout, and each curve, ends with a zero-length segment (ALB015,
// ALS015) at the alignment's end, in its end direction.

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <variant>

#include "katana/entity/tables.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/profile.hpp"
#include "katana/math/numerics.hpp"
#include "parts.hpp"

namespace katana::ifc::detail {

namespace {

using geometry::AlignmentElementKind;

// Two directions or two curvatures closer than this are the same for the
// transition code. The solver closes an element to about 1e-12 rad; this is
// a thousand times that, and still far below anything a joint could be
// designed to.
constexpr double kSameDirection = 1e-9;
constexpr double kSameCurvature = 1e-9;

struct Piece {
    AlignmentElementKind kind = AlignmentElementKind::Tangent;
    Vec2 start{};
    double direction = 0.0;
    double length = 0.0;
    double startCurvature = 0.0; // signed, positive turning left
    double endCurvature = 0.0;
    Vec2 end{};
    double endDirection = 0.0;
    std::size_t pi = 0;
    double startStation = 0.0;
};

Piece pieceOf(const geometry::AlignmentElement& element)
{
    Piece piece;
    piece.kind = geometry::kindOf(element);
    piece.length = element.length;
    piece.pi = element.pi;
    piece.startStation = element.startStation;
    if (const auto* line = std::get_if<geometry::Segment2>(&element.shape)) {
        piece.start = line->start;
        piece.end = line->end;
        piece.direction = std::atan2(line->delta().y, line->delta().x);
        piece.endDirection = piece.direction;
    } else if (const auto* arc = std::get_if<geometry::Arc2>(&element.shape)) {
        const double turn = arc->sweep >= 0.0 ? 1.0 : -1.0;
        piece.start = arc->startPoint();
        piece.end = arc->endPoint();
        piece.direction = arc->startAngle + turn * math::kHalfPi;
        piece.endDirection = arc->endAngle() + turn * math::kHalfPi;
        piece.startCurvature = piece.endCurvature = turn / arc->radius;
    } else if (const auto* spiral = std::get_if<geometry::Spiral2>(&element.shape)) {
        piece.start = spiral->start;
        piece.end = spiral->endPoint();
        piece.direction = spiral->startDirection;
        piece.endDirection = spiral->endDirection();
        piece.startCurvature = spiral->startCurvature;
        piece.endCurvature = spiral->endCurvature;
    }
    return piece;
}

// The difference of two directions, folded into (-pi, pi].
double angleBetween(double a, double b)
{
    double d = std::remainder(a - b, math::kTwoPi);
    return std::abs(d);
}

// What one piece's end has in common with the next's start.
std::string transition(double endDirection, double endCurvature, double nextDirection,
                       double nextCurvature)
{
    if (angleBetween(endDirection, nextDirection) > kSameDirection) {
        return "CONTINUOUS"; // a kink: a PI with no curve
    }
    if (std::abs(endCurvature - nextCurvature) >
        kSameCurvature * std::max(1.0, std::abs(endCurvature))) {
        return "CONTSAMEGRADIENT";
    }
    return "CONTSAMEGRADIENTSAMECURVATURE";
}

std::string_view horizontalType(AlignmentElementKind kind)
{
    switch (kind) {
    case AlignmentElementKind::Tangent:
        return "LINE";
    case AlignmentElementKind::Spiral:
        return "CLOTHOID";
    case AlignmentElementKind::Arc:
        return "CIRCULARARC";
    }
    return "LINE";
}

// The name surveyors give the point where one kind of element meets
// another: TS, SC, CS, ST at a spiral; PC and PT at a simple curve; PCC
// between two arcs; PI at a kink.
std::string horizontalTag(std::optional<AlignmentElementKind> before,
                          std::optional<AlignmentElementKind> after)
{
    using K = AlignmentElementKind;
    if (!before) {
        return "Start";
    }
    if (!after) {
        return "End";
    }
    const auto code = [](K k) { return k == K::Tangent ? 'T' : (k == K::Spiral ? 'S' : 'C'); };
    const std::string pair{code(*before), code(*after)};
    if (pair == "TS" || pair == "SC" || pair == "CS" || pair == "ST" || pair == "SS") {
        return pair;
    }
    if (pair == "TC") {
        return "PC";
    }
    if (pair == "CT") {
        return "PT";
    }
    if (pair == "CC") {
        return "PCC";
    }
    return "PI";
}

std::string pieceName(const Piece& piece)
{
    const std::string pi = std::to_string(piece.pi);
    switch (piece.kind) {
    case AlignmentElementKind::Tangent:
        return "Tangent to PI " + pi;
    case AlignmentElementKind::Spiral:
        return "Spiral at PI " + pi;
    case AlignmentElementKind::Arc:
        return "Arc at PI " + pi;
    }
    return pi;
}

std::string stationText(double station)
{
    char buffer[48];
    std::snprintf(buffer, sizeof buffer, "%.3f", station);
    return buffer;
}

// The length of y = A + B x + C x^2 over 0..L, in closed form: with
// u = B + 2 C x, the integral of sqrt(1 + u^2) du / 2C is
// (u sqrt(1 + u^2) + asinh u) / 4C. A grade change too small to divide by
// is a straight line.
double parabolaLength(double length, double g1, double g2)
{
    if (std::abs(g2 - g1) < 1e-9) {
        return length * std::sqrt(1.0 + g1 * g1);
    }
    const auto f = [](double u) { return u * std::sqrt(1.0 + u * u) + std::asinh(u); };
    return length * (f(g2) - f(g1)) / (2.0 * (g2 - g1));
}

// The curvature of the profile curve in the (distance, height) plane, which
// is what an IfcGradientCurve's segments join in.
double profileCurvature(double grade, double gradeRate)
{
    return gradeRate / std::pow(1.0 + grade * grade, 1.5);
}

class AlignmentWriter {
  public:
    AlignmentWriter(Builder& builder, const entity::Alignment& alignment)
        : b_(builder), alignment_(alignment), key_("alignment/" + alignment.name)
    {
    }

    void write(const geometry::SolvedAlignment& horizontal,
               const std::optional<geometry::SolvedProfile>& profile)
    {
        StepFile& file = b_.file();
        placement_ = file.add("IfcLocalPlacement", Args().null().ref(b_.identity3()));

        // ---- horizontal ------------------------------------------------------
        std::vector<Piece> pieces;
        for (const auto& element : horizontal.elements()) {
            pieces.push_back(pieceOf(element));
        }
        std::vector<Id> curveSegments;
        std::vector<Id> segments;
        for (std::size_t i = 0; i < pieces.size(); ++i) {
            const Piece& piece = pieces[i];
            const std::optional<AlignmentElementKind> before =
                i == 0 ? std::nullopt : std::optional(pieces[i - 1].kind);
            const std::optional<AlignmentElementKind> after =
                i + 1 == pieces.size() ? std::nullopt : std::optional(pieces[i + 1].kind);
            const std::string code =
                i + 1 < pieces.size()
                    ? transition(piece.endDirection, piece.endCurvature, pieces[i + 1].direction,
                                 pieces[i + 1].startCurvature)
                    : transition(piece.endDirection, piece.endCurvature, piece.endDirection, 0.0);
            const Id curveSegment = horizontalCurveSegment(piece, code);
            curveSegments.push_back(curveSegment);
            segments.push_back(horizontalSegment(piece, horizontalTag(before, piece.kind),
                                                 horizontalTag(piece.kind, after), curveSegment,
                                                 pieceName(piece), i));
        }
        // The zero-length segment that closes the layout, at the end in the
        // end direction.
        {
            Piece last;
            last.start = pieces.back().end;
            last.direction = pieces.back().endDirection;
            last.end = last.start;
            last.endDirection = last.direction;
            const Id curveSegment = horizontalCurveSegment(last, "DISCONTINUOUS");
            curveSegments.push_back(curveSegment);
            segments.push_back(
                horizontalSegment(last, "End", "End", curveSegment, "End", pieces.size()));
        }
        composite_ = file.add("IfcCompositeCurve", Args().refs(curveSegments).boolean(false));

        // ---- vertical ----------------------------------------------------------
        std::vector<Id> verticalSegments;
        Id gradient = 0;
        if (profile) {
            std::vector<Id> gradientSegments;
            writeProfile(horizontal, *profile, gradientSegments, verticalSegments);
            gradient = file.add("IfcGradientCurve",
                                Args().refs(gradientSegments).boolean(false).ref(composite_).null());
        }

        // ---- the alignment ---------------------------------------------------------
        std::vector<Id> representations;
        if (gradient == 0) {
            representations.push_back(
                b_.shape(b_.axisContext(), "Axis", "Curve2D", {composite_}));
        } else {
            representations.push_back(
                b_.shape(b_.axisContext(), "FootPrint", "Curve2D", {composite_}));
            representations.push_back(b_.shape(b_.axisContext(), "Axis", "Curve3D", {gradient}));
        }
        const Id shape = b_.productShape(representations);
        const Id ifcAlignment = file.add("IfcAlignment", Args()
                                                             .string(b_.guid(key_))
                                                             .null()
                                                             .string(alignment_.name)
                                                             .stringOrNull(alignment_.description)
                                                             .null()
                                                             .ref(placement_)
                                                             .ref(shape)
                                                             .null());
        ++b_.report().classes["IfcAlignment"];
        const Id layoutH = file.add("IfcAlignmentHorizontal", Args()
                                                                  .string(b_.guid(key_ + "/horizontal"))
                                                                  .null()
                                                                  .null()
                                                                  .null()
                                                                  .null()
                                                                  .null()
                                                                  .null());
        std::vector<Id> layouts{layoutH};
        Id layoutV = 0;
        if (gradient != 0) {
            layoutV = file.add("IfcAlignmentVertical", Args()
                                                           .string(b_.guid(key_ + "/vertical"))
                                                           .null()
                                                           .null()
                                                           .null()
                                                           .null()
                                                           .null()
                                                           .null());
            layouts.push_back(layoutV);
        }
        nest(key_ + "/layouts", ifcAlignment, layouts);
        nest(key_ + "/horizontal/segments", layoutH, segments);
        if (layoutV != 0) {
            nest(key_ + "/vertical/segments", layoutV, verticalSegments);
        }
        writeReferents(ifcAlignment, horizontal, profile);
        writeProperties(ifcAlignment, horizontal, profile);
        b_.aggregateInProject(ifcAlignment);
        ++b_.report().alignments;
    }

  private:
    Id horizontalCurveSegment(const Piece& piece, std::string_view code)
    {
        StepFile& file = b_.file();
        Id parent = 0;
        double start = 0.0;
        double length = piece.length;
        switch (piece.kind) {
        case AlignmentElementKind::Tangent:
            parent = b_.unitLine();
            break;
        case AlignmentElementKind::Arc: {
            const double radius = 1.0 / std::abs(piece.startCurvature);
            parent = file.add("IfcCircle", Args()
                                               .ref(b_.placement2(b_.origin2(), 0.0))
                                               .real(radius));
            length = piece.startCurvature >= 0.0 ? piece.length : -piece.length;
            break;
        }
        case AlignmentElementKind::Spiral: {
            const double change = piece.endCurvature - piece.startCurvature;
            const double constant =
                std::copysign(std::sqrt(piece.length / std::abs(change)), change);
            parent = file.add("IfcClothoid", Args()
                                                 .ref(b_.placement2(b_.origin2(), 0.0))
                                                 .real(constant));
            start = piece.startCurvature * piece.length / change;
            break;
        }
        }
        const Id placement = b_.placement2(b_.localPoint(b_.local(piece.start)), piece.direction);
        return file.add("IfcCurveSegment", Args()
                                               .enumeration(code)
                                               .ref(placement)
                                               .typed("IfcLengthMeasure", stepReal(start))
                                               .typed("IfcLengthMeasure", stepReal(length))
                                               .ref(parent));
    }

    Id horizontalSegment(const Piece& piece, std::string_view startTag, std::string_view endTag,
                         Id curveSegment, std::string_view name, std::size_t index)
    {
        StepFile& file = b_.file();
        const auto radius = [](double curvature) { return curvature == 0.0 ? 0.0 : 1.0 / curvature; };
        const Id design = file.add("IfcAlignmentHorizontalSegment",
                                   Args()
                                       .string(startTag)
                                       .string(endTag)
                                       .ref(b_.localPoint(b_.local(piece.start)))
                                       .real(piece.direction)
                                       .real(radius(piece.startCurvature))
                                       .real(radius(piece.endCurvature))
                                       .real(piece.length)
                                       .null()
                                       .enumeration(horizontalType(piece.kind)));
        return alignmentSegment(key_ + "/horizontal/" + std::to_string(index), name, curveSegment,
                                design);
    }

    Id alignmentSegment(const std::string& key, std::string_view name, Id curveSegment, Id design)
    {
        // Its own representation, the one curve segment it is (ALS004),
        // placed as the alignment is.
        const Id shape = b_.productShape(
            {b_.shape(b_.axisContext(), "Axis", "Segment", {curveSegment})});
        return b_.file().add("IfcAlignmentSegment", Args()
                                                        .string(b_.guid(key))
                                                        .null()
                                                        .string(name)
                                                        .null()
                                                        .null()
                                                        .ref(placement_)
                                                        .ref(shape)
                                                        .ref(design));
    }

    void writeProfile(const geometry::SolvedAlignment& horizontal,
                      const geometry::SolvedProfile& profile, std::vector<Id>& gradientSegments,
                      std::vector<Id>& segments)
    {
        const auto& elements = profile.elements();
        const double originHeight = b_.local(Vec3(0.0, 0.0, 0.0)).z;
        for (std::size_t i = 0; i < elements.size(); ++i) {
            const geometry::ProfileElement& element = elements[i];
            const bool curve = element.kind == geometry::ProfileElementKind::Curve;
            const double along = element.startStation - horizontal.startStation();
            const double height = element.startElevation + originHeight;
            const double rate = curve ? (element.endGrade - element.startGrade) / element.length : 0.0;

            std::string code = "DISCONTINUOUS";
            if (i + 1 < elements.size()) {
                const auto& next = elements[i + 1];
                const double nextRate =
                    next.kind == geometry::ProfileElementKind::Curve
                        ? (next.endGrade - next.startGrade) / next.length
                        : 0.0;
                code = transition(std::atan(element.endGrade),
                                  profileCurvature(element.endGrade, 2.0 * rate / 2.0),
                                  std::atan(next.startGrade),
                                  profileCurvature(next.startGrade, nextRate));
            } else {
                code = transition(std::atan(element.endGrade),
                                  profileCurvature(element.endGrade, rate),
                                  std::atan(element.endGrade), 0.0);
            }
            const Id gradientSegment =
                verticalCurveSegment(along, height, element.startGrade, element.endGrade,
                                     element.length, curve, code);
            gradientSegments.push_back(gradientSegment);

            const std::string startTag =
                i == 0 ? "Start" : (curve ? "PVC" : (elements[i - 1].kind == geometry::ProfileElementKind::Curve ? "PVT" : "PVI"));
            const std::string endTag =
                i + 1 == elements.size()
                    ? "End"
                    : (curve ? "PVT"
                             : (elements[i + 1].kind == geometry::ProfileElementKind::Curve ? "PVC"
                                                                                            : "PVI"));
            Args design;
            design.string(startTag)
                .string(endTag)
                .real(along)
                .real(element.length)
                .real(height)
                .real(element.startGrade)
                .real(element.endGrade);
            if (curve) {
                // Signed: negative on a crest, where the grade falls (ALB012).
                design.real(element.length / (element.endGrade - element.startGrade))
                    .enumeration("PARABOLICARC");
            } else {
                design.null().enumeration("CONSTANTGRADIENT");
            }
            const Id parameters = b_.file().add("IfcAlignmentVerticalSegment", design);
            segments.push_back(alignmentSegment(key_ + "/vertical/" + std::to_string(i),
                                                curve ? "Vertical curve at PVI " + std::to_string(element.pvi)
                                                      : "Grade to PVI " + std::to_string(element.pvi),
                                                gradientSegment, parameters));
        }
        // The closing zero-length segment, at the profile's end on its last grade.
        const geometry::ProfileElement& last = elements.back();
        const double along = last.startStation + last.length - horizontal.startStation();
        const double height = last.endElevation() + originHeight;
        const Id gradientSegment =
            verticalCurveSegment(along, height, last.endGrade, last.endGrade, 0.0, false, "DISCONTINUOUS");
        gradientSegments.push_back(gradientSegment);
        const Id parameters = b_.file().add("IfcAlignmentVerticalSegment", Args()
                                                                               .string("End")
                                                                               .string("End")
                                                                               .real(along)
                                                                               .real(0.0)
                                                                               .real(height)
                                                                               .real(last.endGrade)
                                                                               .real(last.endGrade)
                                                                               .null()
                                                                               .enumeration("CONSTANTGRADIENT"));
        segments.push_back(alignmentSegment(key_ + "/vertical/" + std::to_string(elements.size()),
                                            "End", gradientSegment, parameters));
    }

    Id verticalCurveSegment(double along, double height, double g1, double g2, double length,
                            bool curve, std::string_view code)
    {
        StepFile& file = b_.file();
        Id parent = 0;
        double curveLength = 0.0;
        if (curve) {
            // y = A + B x + C x^2, A the start height as IfcOpenShell writes
            // it (the placement moves the curve's start there in any case).
            const double c = (g2 - g1) / (2.0 * length);
            parent = file.add("IfcPolynomialCurve", Args()
                                                        .ref(b_.placement2(b_.origin2(), 0.0))
                                                        .reals({0.0, 1.0})
                                                        .reals({height, g1, c})
                                                        .null());
            curveLength = parabolaLength(length, g1, g2);
        } else {
            parent = b_.unitLine();
            curveLength = length * std::sqrt(1.0 + g1 * g1);
        }
        const Id location = b_.localPoint(Vec2(along, height));
        const Id placement = b_.placement2(location, std::atan(g1));
        return file.add("IfcCurveSegment", Args()
                                               .enumeration(code)
                                               .ref(placement)
                                               .typed("IfcLengthMeasure", stepReal(0.0))
                                               .typed("IfcLengthMeasure", stepReal(curveLength))
                                               .ref(parent));
    }

    void nest(const std::string& key, Id parent, const std::vector<Id>& children)
    {
        b_.file().add("IfcRelNests", Args()
                                         .string(b_.guid("nests/" + key))
                                         .null()
                                         .null()
                                         .null()
                                         .ref(parent)
                                         .refs(children));
    }

    // A station referent at the start, at every key station of the
    // horizontal and the profile, and at the end: where an alignment is
    // labelled and set out from. Each is placed by its distance along the
    // horizontal curve, with the profile's level as a vertical offset where
    // there is one, and carries its station in Pset_Stationing.
    void writeReferents(Id ifcAlignment, const geometry::SolvedAlignment& horizontal,
                        const std::optional<geometry::SolvedProfile>& profile)
    {
        std::vector<std::pair<double, std::string>> stations;
        const auto& elements = horizontal.elements();
        for (std::size_t i = 0; i < elements.size(); ++i) {
            if (i == 0) {
                stations.emplace_back(elements[i].startStation, "Start");
            } else {
                stations.emplace_back(elements[i].startStation,
                                      horizontalTag(geometry::kindOf(elements[i - 1]),
                                                    geometry::kindOf(elements[i])));
            }
        }
        stations.emplace_back(horizontal.endStation(), "End");
        if (profile) {
            const auto& profileElements = profile->elements();
            for (std::size_t i = 1; i < profileElements.size(); ++i) {
                const auto& element = profileElements[i];
                const bool curve = element.kind == geometry::ProfileElementKind::Curve;
                const bool afterCurve =
                    profileElements[i - 1].kind == geometry::ProfileElementKind::Curve;
                stations.emplace_back(element.startStation,
                                      curve ? "PVC" : (afterCurve ? "PVT" : "PVI"));
            }
        }
        std::stable_sort(stations.begin(), stations.end(),
                         [](const auto& a, const auto& b) { return a.first < b.first; });

        std::vector<Id> referents;
        double previous = -std::numeric_limits<double>::infinity();
        const double originHeight = b_.local(Vec3(0.0, 0.0, 0.0)).z;
        for (const auto& [station, tag] : stations) {
            if (station - previous < math::tolerance::kCoordinate ||
                !horizontal.containsStation(station)) {
                continue;
            }
            previous = station;
            const double along = station - horizontal.startStation();
            const Vec2 at = *horizontal.pointAtStation(station);
            const double direction = *horizontal.directionAtStation(station);
            std::optional<double> level;
            if (profile && profile->containsStation(station)) {
                level = profile->elevationAt(station).value() + originHeight;
            }
            StepFile& file = b_.file();
            Args expression;
            expression.typed("IfcLengthMeasure", stepReal(along)).null();
            if (level) {
                expression.real(*level);
            } else {
                expression.null();
            }
            expression.null().ref(composite_);
            const Id distance = file.add("IfcPointByDistanceExpression", expression);
            const Id relative =
                file.add("IfcAxis2PlacementLinear", Args().ref(distance).null().null());
            const Vec2 local = b_.local(at);
            const Id cartesianPoint = file.add(
                "IfcCartesianPoint", Args().reals({local.x, local.y, level.value_or(0.0)}));
            const Id cartesian = file.add("IfcAxis2Placement3D",
                                          Args()
                                              .ref(cartesianPoint)
                                              .ref(b_.direction(0.0, 0.0, 1.0))
                                              .ref(b_.direction(std::cos(direction),
                                                                std::sin(direction), 0.0)));
            const Id placement =
                file.add("IfcLinearPlacement", Args().null().ref(relative).ref(cartesian));
            const std::string name = tag + " " + stationText(station);
            const Id referent = file.add("IfcReferent",
                                         Args()
                                             .string(b_.guid(key_ + "/station/" + stationText(station)))
                                             .null()
                                             .string(name)
                                             .null()
                                             .null()
                                             .ref(placement)
                                             .null()
                                             .enumeration("STATION"));
            ++b_.report().classes["IfcReferent"];
            PropertyList stationing;
            stationing.length("Station", station);
            const std::string psetKey = key_ + "/station/" + stationText(station);
            b_.defines(psetKey, b_.propertySet(psetKey, "Pset_Stationing", stationing), {referent});
            referents.push_back(referent);
        }
        nest(key_ + "/referents", ifcAlignment, referents);
    }

    void writeProperties(Id ifcAlignment, const geometry::SolvedAlignment& horizontal,
                         const std::optional<geometry::SolvedProfile>& profile)
    {
        PropertyList list;
        list.length("StartStation", horizontal.startStation())
            .length("EndStation", horizontal.endStation())
            .positiveLength("HorizontalLength", horizontal.length())
            .integer("PointsOfIntersection",
                     static_cast<long long>(alignment_.horizontal.pis.size()));
        if (profile) {
            list.length("ProfileStartStation", profile->startStation())
                .length("ProfileEndStation", profile->endStation())
                .integer("PointsOfVerticalIntersection",
                         static_cast<long long>(alignment_.vertical->pvis.size()));
        }
        b_.defines(key_, b_.propertySet(key_, "Katana_Alignment", list), {ifcAlignment});
    }

    Builder& b_;
    const entity::Alignment& alignment_;
    std::string key_;
    Id placement_ = 0;
    Id composite_ = 0;
};

} // namespace

void exportAlignments(Builder& builder, const entity::Model& model)
{
    model.alignments.forEach([&](const entity::Alignment& alignment) {
        auto horizontal = geometry::solveAlignment(alignment.horizontal);
        if (!horizontal) {
            builder.warn("alignment \"" + alignment.name + "\" not written: " +
                         horizontal.error().describe());
            return;
        }
        std::optional<geometry::SolvedProfile> profile;
        if (alignment.vertical) {
            auto solved = geometry::solveProfile(*alignment.vertical);
            if (!solved) {
                builder.warn("the profile of alignment \"" + alignment.name +
                             "\" is not written: " + solved.error().describe());
            } else if (solved->startStation() < horizontal->startStation() - math::tolerance::kCoordinate ||
                       solved->endStation() > horizontal->endStation() + math::tolerance::kCoordinate) {
                // IFC measures a profile along its alignment; a part beyond
                // the alignment has nowhere to be.
                builder.warn("the profile of alignment \"" + alignment.name + "\" (" +
                             stationText(solved->startStation()) + " to " +
                             stationText(solved->endStation()) +
                             ") runs beyond the alignment (" +
                             stationText(horizontal->startStation()) + " to " +
                             stationText(horizontal->endStation()) + ") and is not written");
            } else {
                profile = std::move(*solved);
            }
        }
        AlignmentWriter(builder, alignment).write(*horizontal, profile);
    });
}

} // namespace katana::ifc::detail
