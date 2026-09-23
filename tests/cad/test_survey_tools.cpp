// The survey tools (include/katana/cad/survey_tools.hpp) and the command-line
// verbs built on them (INVERSE, FORWARD, AREA).
//
// Every expected value is worked out here from the geometry, not taken from a
// run:
//   * the inverse of a 3-4-5 triangle is 5 at azimuth atan2(3, 4) =
//     36.869897645844021 degrees; 0.869897645844021 * 60 = 52.19385875064126',
//     0.19385875064126 * 60 = 11.6315250384756" -> 36°52'11.63";
//   * a 100 x 50 rectangle is 5000 m^2 = 0.5 ha (1 ha = 10 000 m^2, SI
//     Brochure 9th ed., table 8) with a perimeter of 300;
//   * the traverses are built from stations with KNOWN coordinates, so the
//     observed angles and distances are the exact geometry of the figure and
//     the closure is exact; the perturbed one lengthens one leg by 0.100 and
//     its Bowditch and transit corrections are worked out below by hand from
//     the rules' definitions (Ghilani & Wolf, "Elementary Surveying", the
//     compass and transit rules: correction = -misclosure * L / sum L, and
//     -misclosure * |lat| / sum |lat|). No textbook example is reproduced
//     because none could be checked against its source here; a constructed
//     figure whose answer is known exactly is the stronger test anyway;
//   * the level book is reduced by hand in the height-of-collimation layout.
//
// Coordinates are chosen so that easting and northing differ in magnitude
// (E ~ 5 000, N ~ 2 000 or E ~ 1 000, N ~ 5 000): the one mistake this module
// could make that still draws a plausible picture is transposing them.

#include <gtest/gtest.h>

#include <cmath>
#include <optional>
#include <string>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/survey_tools.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/math/numerics.hpp"
#include "katana/survey/angles.hpp"

using namespace katana::cad;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::Point2;
using katana::math::kPi;
namespace cmd = katana::commands;
namespace survey = katana::survey;

namespace {

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

SurveyPosition at(double easting, double northing,
                  std::optional<double> elevation = std::nullopt)
{
    SurveyPosition position;
    position.point = Point2(easting, northing);
    position.elevation = elevation;
    return position;
}

// A point entity with a name and, when given, a height - as the survey import
// writes them.
EntityId addPoint(Document& document, Point2 position, const std::string& name,
                  std::optional<double> elevation = std::nullopt)
{
    Entity entity;
    entity.geometry = katana::entity::PointGeometry{position};
    entity.layer = document.currentLayer();
    if (!name.empty()) {
        entity.properties.insert_or_assign("point", katana::entity::PropertyValue(name));
    }
    katana::entity::setHeights(entity.properties, {elevation});
    EXPECT_TRUE(document.execute(cmd::createEntities({std::move(entity)})).ok());
    return document.lastCreatedEntities().front();
}

EntityId addEntity(Document& document, cmd::CommandPtr command)
{
    const auto status = document.execute(std::move(command));
    EXPECT_TRUE(status.ok()) << (status.ok() ? "" : status.error().describe());
    return document.lastCreatedEntities().front();
}

double azimuthOf(double dE, double dN)
{
    return survey::normalizeAzimuth(std::atan2(dE, dN));
}

// ---- the constructed traverse ------------------------------------------------------------
//
// A(5000, 2000) -> B(5120, 2160) -> C(5360, 2090) -> D(5280, 1898) -> A, as
// (easting, northing). Every leg is a Pythagorean triple, so the distances are
// exact integers:
//   AB (+120, +160) = 200     (3-4-5 x 40)
//   BC (+240,  -70) = 250     (24-7-25 x 10)
//   CD ( -80, -192) = 208     (5-12-13 x 16)
//   DA (-280, +102) = 298     sqrt(78400 + 10404) = sqrt(88804) = 298
// and the four sum to (0, 0). The circuit runs clockwise on the map.
struct Station {
    const char* id;
    double e;
    double n;
};
constexpr Station kA{"A", 5000.0, 2000.0};
constexpr Station kB{"B", 5120.0, 2160.0};
constexpr Station kC{"C", 5360.0, 2090.0};
constexpr Station kD{"D", 5280.0, 1898.0};

double azimuth(const Station& from, const Station& to)
{
    return azimuthOf(to.e - from.e, to.n - from.n);
}

// The clockwise angle turned at `at` from the back station to the forward one,
// by the definition survey::Traverse uses: az(at->forward) - az(at->back).
double turned(const Station& back, const Station& at, const Station& forward)
{
    return survey::normalizeAzimuth(azimuth(at, forward) - azimuth(at, back));
}

TraverseSpec loopSpec()
{
    TraverseSpec spec;
    spec.name = "Loop";
    spec.kind = survey::TraverseKind::ClosedLoop;
    spec.start = Point2(kA.e, kA.n);
    // The first leg's direction, given by sighting B's known position.
    spec.startOrientation.referenceMark = Point2(kB.e, kB.n);
    spec.legs = {
        {"A", turned(kD, kA, kB), 200.0},
        {"B", turned(kA, kB, kC), 250.0},
        {"C", turned(kB, kC, kD), 208.0},
        {"D", turned(kC, kD, kA), 298.0},
    };
    return spec;
}

} // namespace

// ---- angle text ----------------------------------------------------------------------------

TEST(SurveyTools, DirectionsAreReadAsAzimuthsOrQuadrantBearings)
{
    // S 45 W is 180 + 45 = 225 degrees; N 90 E is due east.
    const auto southWest = parseSurveyDirection("S 45d00m00s W");
    ASSERT_TRUE(southWest.ok()) << southWest.error().describe();
    EXPECT_NEAR(*southWest, 1.25 * kPi, 1e-15);
    const auto east = parseSurveyDirection("n90e");
    ASSERT_TRUE(east.ok());
    EXPECT_NEAR(*east, 0.5 * kPi, 1e-15);
    // 36:52:11.63 is 36 + 52/60 + 11.63/3600 degrees.
    const auto dms = parseSurveyDirection("36:52:11.63");
    ASSERT_TRUE(dms.ok());
    EXPECT_NEAR(*dms, (36.0 + 52.0 / 60.0 + 11.63 / 3600.0) * kPi / 180.0, 1e-15);
    // A negative azimuth is the same direction one turn on.
    const auto negative = parseSurveyDirection("-90");
    ASSERT_TRUE(negative.ok());
    EXPECT_NEAR(*negative, 1.5 * kPi, 1e-15);
}

TEST(SurveyTools, AnUnreadableAngleSaysWhichFormatsAreAccepted)
{
    const auto angle = parseSurveyAngle("36x52");
    ASSERT_FALSE(angle.ok());
    EXPECT_EQ(angle.error().code, ErrorCode::ParseFailure);
    EXPECT_TRUE(contains(angle.error().message, "36d52m11.63s")) << angle.error().message;
    EXPECT_FALSE(parseSurveyDirection("Q 45 E").ok());
    EXPECT_FALSE(parseSurveyNumber("1,5", "distance").ok()) << "a decimal comma is not a point";
}

// ---- positions -----------------------------------------------------------------------------

TEST(SurveyTools, TypedPositionsAreEastingFirstWithAnOptionalHeight)
{
    Document document;
    const auto flat = parseSurveyPosition(document, "1000.5,5000.25");
    ASSERT_TRUE(flat.ok()) << flat.error().describe();
    EXPECT_EQ(flat->point, Point2(1000.5, 5000.25));
    EXPECT_FALSE(flat->elevation.has_value()) << "no Z typed is no height, not a height of 0";

    const auto withHeight = parseSurveyPosition(document, " 1,2,-3.5 ");
    ASSERT_TRUE(withHeight.ok());
    ASSERT_TRUE(withHeight->elevation.has_value());
    EXPECT_EQ(*withHeight->elevation, -3.5);

    EXPECT_EQ(parseSurveyPosition(document, "1,2,3,4").error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(parseSurveyPosition(document, "1,north").error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(parseSurveyPosition(document, "42").error().code, ErrorCode::NotFound);
}

TEST(SurveyTools, PointEntitiesGiveTheirPositionNameAndHeight)
{
    Document document;
    const EntityId id = addPoint(document, Point2(1000, 5000), "CP-01", 31.25);
    const auto position = parseSurveyPosition(document, std::to_string(id));
    ASSERT_TRUE(position.ok()) << position.error().describe();
    EXPECT_EQ(position->point, Point2(1000, 5000));
    ASSERT_TRUE(position->elevation.has_value());
    EXPECT_EQ(*position->elevation, 31.25);
    EXPECT_EQ(position->name, "CP-01");
    EXPECT_EQ(position->label(), "point " + std::to_string(id) + " (CP-01)");

    const EntityId line = addEntity(document, cmd::createLine(Point2(0, 0), Point2(1, 1)));
    const auto notAPoint = positionOfPoint(document, line);
    ASSERT_FALSE(notAPoint.ok());
    EXPECT_EQ(notAPoint.error().code, ErrorCode::InvalidArgument);
}

TEST(SurveyTools, OnlyTheSelectedPointsAreSurveyPositions)
{
    Document document;
    const EntityId p1 = addPoint(document, Point2(1, 2), "P1");
    const EntityId line = addEntity(document, cmd::createLine(Point2(0, 0), Point2(1, 1)));
    const EntityId p2 = addPoint(document, Point2(3, 4), "P2");
    document.selection().set({p2, line, p1});
    const auto positions = selectedPointPositions(document);
    ASSERT_EQ(positions.size(), 2u) << "the line is not a point";
    EXPECT_EQ(positions[0].name, "P1") << "in id order";
    EXPECT_EQ(positions[1].name, "P2");
}

// ---- inverse -------------------------------------------------------------------------------

TEST(SurveyTools, TheInverseOfAThreeFourFiveTriangleIsFiveAtAzimuth36d52m11s63)
{
    const auto result = computeInverse(at(1000, 5000), at(1003, 5004));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_NEAR(result->horizontalDistance, 5.0, 1e-12);
    EXPECT_NEAR(result->deltaEasting, 3.0, 1e-12);
    EXPECT_NEAR(result->deltaNorthing, 4.0, 1e-12);
    // atan2(3, 4) in degrees is 36.869897645844021 (see the file comment).
    EXPECT_NEAR(result->azimuth * 180.0 / kPi, 36.869897645844021, 1e-12);
    EXPECT_FALSE(result->heightDifference.has_value());

    const std::string report = formatInverseReport(*result);
    EXPECT_TRUE(contains(report, "Horizontal distance 5.000")) << report;
    EXPECT_TRUE(contains(report, "Azimuth 36\xC2\xB0" "52'11.63\"")) << report;
    EXPECT_TRUE(contains(report, "Bearing N 36\xC2\xB0" "52'11.63\" E")) << report;
    // Back azimuth: 36°52'11.63" + 180°.
    EXPECT_TRUE(contains(report, "back azimuth 216\xC2\xB0" "52'11.63\"")) << report;
    EXPECT_TRUE(contains(report, "No height difference")) << report;
}

TEST(SurveyTools, DueNorthIsAzimuthZeroSoEastingAndNorthingAreNotTransposed)
{
    // 100 north of the start. Transposed, it would be 100 EAST - azimuth 90.
    const auto result = computeInverse(at(1000, 5000), at(1000, 5100));
    ASSERT_TRUE(result.ok());
    EXPECT_NEAR(result->azimuth, 0.0, 1e-15);
    EXPECT_NEAR(result->deltaNorthing, 100.0, 1e-12);
    EXPECT_EQ(result->deltaEasting, 0.0);
}

TEST(SurveyTools, TheInverseToTheSouthWestIsAzimuth225AndBearingSouth45West)
{
    const auto result = computeInverse(at(0, 0), at(-1, -1));
    ASSERT_TRUE(result.ok());
    EXPECT_NEAR(result->azimuth, 1.25 * kPi, 1e-15);
    EXPECT_NEAR(result->horizontalDistance, std::sqrt(2.0), 1e-15);
    const std::string report = formatInverseReport(*result);
    EXPECT_TRUE(contains(report, "Azimuth 225\xC2\xB0" "00'00.00\"")) << report;
    EXPECT_TRUE(contains(report, "Bearing S 45\xC2\xB0" "00'00.00\" W")) << report;
}

TEST(SurveyTools, HeightsAtBothEndsGiveHeightDifferenceSlopeDistanceAndGrade)
{
    // dz = 12 - 10 = 2; slope = sqrt(5^2 + 2^2) = sqrt(29); grade = 2/5 = 40 %.
    const auto result = computeInverse(at(1000, 5000, 10.0), at(1003, 5004, 12.0));
    ASSERT_TRUE(result.ok());
    ASSERT_TRUE(result->heightDifference.has_value());
    EXPECT_NEAR(*result->heightDifference, 2.0, 1e-12);
    EXPECT_NEAR(*result->slopeDistance, std::sqrt(29.0), 1e-12);
    EXPECT_NEAR(*result->grade, 0.4, 1e-12);
    const std::string report = formatInverseReport(*result);
    EXPECT_TRUE(contains(report, "Height difference +2.000")) << report;
    EXPECT_TRUE(contains(report, "slope distance 5.385")) << report;
    EXPECT_TRUE(contains(report, "grade +40.000 %")) << report;
}

TEST(SurveyTools, AnEndWithoutAHeightGivesNoHeightDifferenceRatherThanOneFromZero)
{
    const auto result = computeInverse(at(1000, 5000, 10.0), at(1003, 5004));
    ASSERT_TRUE(result.ok());
    EXPECT_FALSE(result->heightDifference.has_value());
    EXPECT_FALSE(result->slopeDistance.has_value());
    EXPECT_FALSE(result->grade.has_value());
    const std::string report = formatInverseReport(*result);
    // The END without a height is named, not the one with it.
    EXPECT_TRUE(contains(report, "No height difference: E 1003.000 N 5004.000 has no elevation"))
        << report;
}

TEST(SurveyTools, CoincidentPositionsAreRefusedRatherThanGivenAnAzimuth)
{
    const auto result = computeInverse(at(1000, 5000), at(1000, 5000));
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, ErrorCode::InvalidArgument);
}

TEST(SurveyTools, TheInverseOfALineRunsFromItsStartToItsEndWithItsHeights)
{
    Document document;
    Entity line;
    line.geometry = katana::geometry::Segment2{Point2(1000, 5000), Point2(1003, 5004)};
    katana::entity::setHeights(line.properties, {10.0, 12.0});
    ASSERT_TRUE(document.execute(cmd::createEntities({line})).ok());
    const EntityId id = document.lastCreatedEntities().front();

    const auto ends = endsOfLine(document, id);
    ASSERT_TRUE(ends.ok()) << ends.error().describe();
    const auto result = computeInverse(ends->first, ends->second);
    ASSERT_TRUE(result.ok());
    EXPECT_NEAR(result->horizontalDistance, 5.0, 1e-12);
    EXPECT_NEAR(*result->heightDifference, 2.0, 1e-12);
    EXPECT_TRUE(contains(formatInverseReport(*result), "line " + std::to_string(id) + " start"));

    const EntityId circle = addEntity(document, cmd::createCircle(Point2(0, 0), 1.0));
    EXPECT_EQ(endsOfLine(document, circle).error().code, ErrorCode::InvalidArgument);
}

// ---- forward -------------------------------------------------------------------------------

TEST(SurveyTools, ForwardAlongTheThreeFourFiveDirectionReachesItsFarCorner)
{
    ForwardInput input;
    input.from = at(1000, 5000, 20.0);
    input.azimuth = std::atan2(3.0, 4.0);
    input.distance = 5.0;
    input.heightDifference = -1.5;
    const auto result = computeForward(input);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_NEAR(result->point.x, 1003.0, 1e-9);
    EXPECT_NEAR(result->point.y, 5004.0, 1e-9);
    ASSERT_TRUE(result->elevation.has_value());
    EXPECT_NEAR(*result->elevation, 18.5, 1e-12);

    // Due east by bearing: N 90 E, 10 along.
    input.azimuth = *parseSurveyDirection("N 90 E");
    input.distance = 10.0;
    const auto east = computeForward(input);
    ASSERT_TRUE(east.ok());
    EXPECT_NEAR(east->point.x, 1010.0, 1e-9);
    EXPECT_NEAR(east->point.y, 5000.0, 1e-9);
}

TEST(SurveyTools, AHeightDifferenceFromAStartWithNoHeightIsRefused)
{
    ForwardInput input;
    input.from = at(1000, 5000);
    input.azimuth = 0.0;
    input.distance = 10.0;
    input.heightDifference = 2.0;
    const auto result = computeForward(input);
    ASSERT_FALSE(result.ok()) << "the new point would get a height of 2 above a datum nobody "
                                 "observed";
    EXPECT_EQ(result.error().code, ErrorCode::InvalidArgument);

    input.heightDifference.reset();
    const auto flat = computeForward(input);
    ASSERT_TRUE(flat.ok());
    EXPECT_FALSE(flat->elevation.has_value());

    input.distance = 0.0;
    EXPECT_FALSE(computeForward(input).ok()) << "a zero distance is a point on top of the start";
}

TEST(SurveyTools, AForwardPointIsOneUndoableCommandOnTheCurrentLayer)
{
    Document document;
    CommandInterpreter interpreter(document);
    ASSERT_TRUE(interpreter.run("LAYER NEW stakeout").ok());
    ASSERT_TRUE(interpreter.run("LAYER SET stakeout").ok());

    ForwardInput input;
    input.from = at(1000, 5000, 20.0);
    input.azimuth = 0.0;
    input.distance = 25.0;
    input.heightDifference = 1.0;
    input.name = "P7";
    const auto result = computeForward(input);
    ASSERT_TRUE(result.ok());
    auto command = forwardPointCommand(document, *result);
    ASSERT_TRUE(command.ok()) << command.error().describe();
    ASSERT_TRUE(document.execute(std::move(*command)).ok());

    ASSERT_EQ(document.model().entities.size(), 1u);
    const EntityId id = document.lastCreatedEntities().front();
    const Entity& point = *document.model().entities.find(id);
    EXPECT_EQ(point.layer, "stakeout");
    EXPECT_EQ(std::get<katana::entity::PointGeometry>(point.geometry).position,
              Point2(1000, 5025));
    EXPECT_EQ(katana::entity::toString(point.properties.at("point")), "P7");
    EXPECT_EQ(katana::entity::heightsOf(point.properties, 1).front(), std::optional<double>(21.0));

    ASSERT_TRUE(document.undo().ok());
    EXPECT_TRUE(document.model().entities.empty()) << "one undo removes the whole operation";
}

TEST(SurveyTools, AForwardPointWithoutAHeightCarriesNoElevationProperty)
{
    Document document;
    ForwardInput input;
    input.from = at(1000, 5000);
    input.azimuth = kPi;
    input.distance = 10.0;
    const auto result = computeForward(input);
    ASSERT_TRUE(result.ok());
    auto command = forwardPointCommand(document, *result);
    ASSERT_TRUE(command.ok());
    ASSERT_TRUE(document.execute(std::move(*command)).ok());
    const Entity& point = *document.model().entities.find(document.lastCreatedEntities().front());
    EXPECT_FALSE(point.properties.contains(std::string(katana::entity::kElevationProperty)));
    EXPECT_EQ(std::get<katana::entity::PointGeometry>(point.geometry).position,
              Point2(1000, 4990));
}

// ---- area ----------------------------------------------------------------------------------

TEST(SurveyTools, ARectangleIsFiveThousandSquareMetresOrHalfAHectare)
{
    Document document;
    // UTM-sized: the shoelace sum of these raw coordinates is ~1e13 and its
    // rounding would show in the third decimal; survey::polygonArea works
    // relative to the first vertex and must not.
    const EntityId rectangle = addEntity(
        document, cmd::createPolyline(katana::geometry::Polyline2{
                                          {Point2(500000, 7000000), Point2(500100, 7000000),
                                           Point2(500100, 7000050), Point2(500000, 7000050)},
                                          true}));
    const auto result = computeArea(document, {rectangle});
    ASSERT_TRUE(result.ok()) << result.error().describe();
    ASSERT_EQ(result->items.size(), 1u);
    EXPECT_NEAR(result->items[0].area, 5000.0, 1e-9);
    EXPECT_NEAR(result->items[0].perimeter, 300.0, 1e-9);
    EXPECT_TRUE(result->metres) << "a new project's unit is the metre";

    const std::string report = formatAreaReport(*result);
    EXPECT_TRUE(contains(report, "area 5000.000 m\xC2\xB2 (0.5000 ha)")) << report;
    EXPECT_TRUE(contains(report, "perimeter 300.000 m")) << report;
}

TEST(SurveyTools, ClosedOutlinesAndCirclesAreMeasuredEachAndInTotal)
{
    Document document;
    const EntityId rectangle = addEntity(
        document, cmd::createPolyline(katana::geometry::Polyline2{
                                          {Point2(0, 0), Point2(100, 0), Point2(100, 50),
                                           Point2(0, 50)},
                                          true}));
    const EntityId circle = addEntity(document, cmd::createCircle(Point2(500, 500), 10.0));
    const EntityId open = addEntity(document, cmd::createPolyline(katana::geometry::Polyline2{
                                                  {Point2(0, 0), Point2(1, 0), Point2(1, 1)},
                                                  false}));
    const EntityId line = addEntity(document, cmd::createLine(Point2(0, 0), Point2(5, 5)));

    const auto result = computeArea(document, {rectangle, circle, open, line});
    ASSERT_TRUE(result.ok()) << result.error().describe();
    ASSERT_EQ(result->items.size(), 2u);
    // A circle of radius 10: 100 pi of area, 20 pi round.
    EXPECT_NEAR(result->items[1].area, 100.0 * kPi, 1e-9);
    EXPECT_NEAR(result->items[1].perimeter, 20.0 * kPi, 1e-9);
    EXPECT_NEAR(result->totalArea, 5000.0 + 100.0 * kPi, 1e-9);
    EXPECT_NEAR(result->totalPerimeter, 300.0 + 20.0 * kPi, 1e-9);
    ASSERT_EQ(result->skipped.size(), 2u) << "the open polyline and the line";
    EXPECT_EQ(result->skipped[0].id, open);
    EXPECT_TRUE(contains(result->skipped[0].reason, "open polyline"));

    const std::string report = formatAreaReport(*result);
    // 5000 + 314.159265... = 5314.159 m^2 = 0.5314 ha.
    EXPECT_TRUE(contains(report, "Total  area 5314.159 m\xC2\xB2 (0.5314 ha)")) << report;
    EXPECT_TRUE(contains(report, "Skipped " + std::to_string(line))) << report;
}

TEST(SurveyTools, NothingWithAnAreaIsAnErrorThatSaysWhy)
{
    Document document;
    const EntityId line = addEntity(document, cmd::createLine(Point2(0, 0), Point2(5, 5)));
    const auto result = computeArea(document, {line});
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, ErrorCode::InvalidGeometry);
    EXPECT_TRUE(contains(result.error().context, "has no area")) << result.error().context;
    EXPECT_EQ(computeArea(document, {}).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(computeArea(document, {999}).error().code, ErrorCode::NotFound);
}

TEST(SurveyTools, ADrawingInFeetGetsSquareFeetAndNoHectares)
{
    Document document;
    auto metadata = document.metadata();
    metadata.linearUnit = "foot";
    document.setMetadata(metadata);
    const EntityId square = addEntity(
        document, cmd::createPolyline(katana::geometry::Polyline2{
                                          {Point2(0, 0), Point2(10, 0), Point2(10, 10),
                                           Point2(0, 10)},
                                          true}));
    const auto result = computeArea(document, {square});
    ASSERT_TRUE(result.ok());
    EXPECT_FALSE(result->metres);
    const std::string report = formatAreaReport(*result);
    EXPECT_TRUE(contains(report, "area 100.000 square foot")) << report;
    EXPECT_FALSE(contains(report, " ha)")) << report;
    EXPECT_TRUE(contains(report, "No hectares")) << report;

    EXPECT_TRUE(isMetreUnit(" Metres "));
    EXPECT_TRUE(isMetreUnit("m"));
    EXPECT_FALSE(isMetreUnit("mm"));
}

// ---- traverse ------------------------------------------------------------------------------

TEST(SurveyTools, AClosedLoopBuiltFromKnownCoordinatesClosesExactly)
{
    const auto result = computeTraverseTool(loopSpec());
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const survey::TraverseResult& classical = result->classical;
    ASSERT_TRUE(classical.angularMisclosure.has_value());
    EXPECT_NEAR(*classical.angularMisclosure, 0.0, 1e-12);
    ASSERT_TRUE(classical.linearMisclosure.has_value());
    EXPECT_NEAR(classical.linearMisclosure->length, 0.0, 1e-9);
    EXPECT_NEAR(classical.totalLength, 956.0, 1e-12);

    ASSERT_EQ(result->stations.size(), 4u);
    const Station expected[] = {kA, kB, kC, kD};
    for (std::size_t i = 0; i < 4; ++i) {
        EXPECT_EQ(result->stations[i].id, expected[i].id);
        EXPECT_NEAR(result->stations[i].adjusted.x, expected[i].e, 1e-9) << expected[i].id;
        EXPECT_NEAR(result->stations[i].adjusted.y, expected[i].n, 1e-9) << expected[i].id;
    }
    EXPECT_TRUE(result->stations[0].control);
    EXPECT_FALSE(result->stations[1].control);
}

TEST(SurveyTools, APerturbedLoopIsAdjustedByTheBowditchRuleAsWorkedByHand)
{
    // BC booked as 250.100 instead of 250.000, angles exact. The computed
    // figure overshoots by 0.100 along BC's direction, whose unit vector is
    // (dE, dN) = (240, -70) / 250 = (0.96, -0.28):
    //   misclosure dN = 0.100 * -0.28 = -0.028, dE = 0.100 * 0.96 = +0.096,
    //   length 0.100, total length 200 + 250.1 + 208 + 298 = 956.1,
    //   precision 1 : 956.1 / 0.1 = 1 : 9561.
    // Compass rule: each leg is corrected by -misclosure * L / 956.1, so a
    // station's correction is -misclosure * (length run so far) / 956.1.
    TraverseSpec spec = loopSpec();
    spec.legs[1].distance = 250.1;
    spec.method = TraverseMethod::Compass;
    const auto result = computeTraverseTool(spec);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const auto& linear = *result->classical.linearMisclosure;
    EXPECT_NEAR(linear.latitude, -0.028, 1e-12);
    EXPECT_NEAR(linear.departure, 0.096, 1e-12);
    EXPECT_NEAR(linear.length, 0.1, 1e-12);
    EXPECT_NEAR(*linear.precisionDenominator, 9561.0, 1e-6);

    const double total = 956.1;
    // B: the computed B is exact (AB untouched); corrected by 200/956.1.
    const double toB = 200.0 / total;
    // C and D: carry the 0.100 overshoot, corrected by the run so far.
    const double toC = 450.1 / total;
    const double toD = 658.1 / total;
    const auto& stations = result->stations;
    EXPECT_NEAR(stations[1].adjusted.x, kB.e - 0.096 * toB, 1e-9);
    EXPECT_NEAR(stations[1].adjusted.y, kB.n + 0.028 * toB, 1e-9);
    EXPECT_NEAR(stations[2].adjusted.x, kC.e + 0.096 - 0.096 * toC, 1e-9);
    EXPECT_NEAR(stations[2].adjusted.y, kC.n - 0.028 + 0.028 * toC, 1e-9);
    EXPECT_NEAR(stations[3].adjusted.x, kD.e + 0.096 - 0.096 * toD, 1e-9);
    EXPECT_NEAR(stations[3].adjusted.y, kD.n - 0.028 + 0.028 * toD, 1e-9);
    // Unadjusted C is the overshoot itself.
    EXPECT_NEAR(stations[2].unadjusted.x, kC.e + 0.096, 1e-9);
    EXPECT_NEAR(stations[2].unadjusted.y, kC.n - 0.028, 1e-9);

    const std::string report = formatTraverseReport(*result);
    EXPECT_TRUE(contains(report, "precision 1 : 9561")) << report;
    EXPECT_TRUE(contains(report, "Linear misclosure dN -0.028  dE +0.096  length 0.100")) << report;
    EXPECT_TRUE(contains(report, "Bowditch")) << report;
}

TEST(SurveyTools, TheTransitRuleCorrectsInProportionToLatitudeAndDeparture)
{
    // The same perturbed loop. Latitudes (dN) of the observed legs: 160,
    // 250.1 * -0.28 = -70.028, -192, 102; sum of magnitudes 524.028.
    // Departures: 120, 250.1 * 0.96 = 240.096, -80, -280; sum 720.096.
    // Transit rule on AB: dN correction +0.028 * 160 / 524.028, dE correction
    // -0.096 * 120 / 720.096.
    TraverseSpec spec = loopSpec();
    spec.legs[1].distance = 250.1;
    spec.method = TraverseMethod::Transit;
    const auto result = computeTraverseTool(spec);
    ASSERT_TRUE(result.ok());
    EXPECT_NEAR(result->stations[1].adjusted.y, kB.n + 0.028 * 160.0 / 524.028, 1e-9);
    EXPECT_NEAR(result->stations[1].adjusted.x, kB.e - 0.096 * 120.0 / 720.096, 1e-9);
    EXPECT_TRUE(contains(formatTraverseReport(*result), "transit rule"));
}

namespace {

// A -> B -> C -> D between control A and D. Oriented at A on a mark due north
// (azimuth 0) and closed at D on a mark due south (azimuth 180 degrees), with
// the angles the figure turns:
//   at A, from north to B:            az(AB) - 0
//   at B and C: back to forward, as in the loop
//   at D, from C to the south mark:   180 degrees - az(DC)
TraverseSpec linkSpec()
{
    TraverseSpec spec;
    spec.name = "Link";
    spec.kind = survey::TraverseKind::Link;
    spec.start = Point2(kA.e, kA.n);
    spec.startOrientation.referenceMark = Point2(kA.e, kA.n + 500.0);
    spec.legs = {
        {"A", survey::normalizeAzimuth(azimuth(kA, kB) - 0.0), 200.0},
        {"B", turned(kA, kB, kC), 250.0},
        {"C", turned(kB, kC, kD), 208.0},
    };
    spec.endStation = "D";
    spec.end = Point2(kD.e, kD.n);
    spec.closingAngle = survey::normalizeAzimuth(kPi - azimuth(kD, kC));
    spec.closingOrientation.referenceMark = Point2(kD.e, kD.n - 100.0);
    return spec;
}

} // namespace

TEST(SurveyTools, ALinkTraverseBetweenControlChecksBothItsAnglesAndItsPosition)
{
    const auto result = computeTraverseTool(linkSpec());
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_NEAR(result->startAzimuth, 0.0, 1e-15);
    ASSERT_TRUE(result->closingReferenceAzimuth.has_value());
    EXPECT_NEAR(*result->closingReferenceAzimuth, kPi, 1e-15);
    // 3 setup angles + the closing angle take part in the check.
    EXPECT_EQ(result->classical.angleCount, 4u);
    EXPECT_NEAR(*result->classical.angularMisclosure, 0.0, 1e-12);
    EXPECT_NEAR(result->classical.linearMisclosure->length, 0.0, 1e-9);

    ASSERT_EQ(result->stations.size(), 4u);
    EXPECT_TRUE(result->stations.front().control);
    EXPECT_TRUE(result->stations.back().control) << "D is closing control";
    EXPECT_FALSE(result->stations[1].control);
    EXPECT_NEAR(result->stations[2].adjusted.x, kC.e, 1e-9);
    EXPECT_NEAR(result->stations[2].adjusted.y, kC.n, 1e-9);
}

TEST(SurveyTools, LeastSquaresHoldsTheControlAndCountsItsRedundancy)
{
    TraverseSpec spec = linkSpec();
    spec.method = TraverseMethod::LeastSquares;

    // Exact observations: the adjustment has nothing to distribute and must
    // return the constructed coordinates.
    const auto exact = computeTraverseTool(spec);
    ASSERT_TRUE(exact.ok()) << exact.error().describe();
    ASSERT_TRUE(exact->leastSquares.has_value());
    EXPECT_NEAR(exact->stations[1].adjusted.x, kB.e, 1e-6);
    EXPECT_NEAR(exact->stations[1].adjusted.y, kB.n, 1e-6);
    EXPECT_NEAR(exact->stations[2].adjusted.x, kC.e, 1e-6);
    EXPECT_NEAR(exact->stations[2].adjusted.y, kC.n, 1e-6);
    // Equations: 3 distances, 2 angles at the interior stations B and C, and
    // the two angles turned from reference directions (at A and at D) as
    // azimuths of their legs - 7. Unknowns: B and C, 2 coordinates each - 4.
    // Degrees of freedom 7 - 4 = 3 (survey::buildTraverseNetwork's model).
    EXPECT_EQ(exact->leastSquares->statistics.degreesOfFreedom, 3u);

    // Perturbed: the control stays exactly where it is published.
    spec.legs[1].distance = 250.05;
    const auto perturbed = computeTraverseTool(spec);
    ASSERT_TRUE(perturbed.ok()) << perturbed.error().describe();
    const auto& stations = perturbed->stations;
    EXPECT_EQ(stations.front().adjusted, Point2(kA.e, kA.n));
    EXPECT_EQ(stations.back().adjusted, Point2(kD.e, kD.n));
    ASSERT_TRUE(stations[1].sigmaEasting.has_value());
    EXPECT_GT(*stations[1].sigmaEasting, 0.0);
    EXPECT_EQ(*stations.front().sigmaEasting, 0.0) << "fixed control has no uncertainty";
    // A 50 mm blunder shared among three legs cannot move a station further
    // than the blunder itself.
    EXPECT_NEAR(stations[2].adjusted.x, kC.e, 0.05);
    EXPECT_NEAR(stations[2].adjusted.y, kC.n, 0.05);
    const std::string report = formatTraverseReport(*perturbed);
    EXPECT_TRUE(contains(report, "angles 5.0\", distances 2.0 mm + 2.0 ppm")) << report;
    EXPECT_TRUE(contains(report, "3 degrees of freedom")) << report;
}

TEST(SurveyTools, AnOpenTraverseHasNoCheckAndSaysSo)
{
    TraverseSpec spec = linkSpec();
    spec.kind = survey::TraverseKind::Open;
    spec.closingAngle.reset();
    const auto result = computeTraverseTool(spec);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_FALSE(result->classical.linearMisclosure.has_value());
    EXPECT_FALSE(result->stations.back().control) << "an open traverse's end is computed";
    EXPECT_NEAR(result->stations.back().adjusted.x, kD.e, 1e-9);
    EXPECT_NEAR(result->stations.back().adjusted.y, kD.n, 1e-9);
    EXPECT_TRUE(contains(formatTraverseReport(*result), "nothing checks it"));
}

TEST(SurveyTools, AReferenceMarkOnTopOfItsStationGivesNoDirection)
{
    TraverseSpec spec = linkSpec();
    spec.startOrientation.referenceMark = spec.start;
    const auto result = computeTraverseTool(spec);
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(contains(result.error().message, "backsight")) << result.error().message;
}

TEST(SurveyTools, AddingATraverseIsOneUndoStepOfNewStationsAndOnePolyline)
{
    Document document;
    const auto result = computeTraverseTool(loopSpec());
    ASSERT_TRUE(result.ok());
    auto command = traverseCommand(document, *result);
    ASSERT_TRUE(command.ok()) << command.error().describe();
    ASSERT_TRUE(document.execute(std::move(*command)).ok());

    // B, C and D are new; A is the control it hung on.
    std::size_t points = 0;
    std::size_t polylines = 0;
    for (const EntityId id : document.lastCreatedEntities()) {
        const Entity& entity = *document.model().entities.find(id);
        if (const auto* line = std::get_if<katana::geometry::Polyline2>(&entity.geometry)) {
            ++polylines;
            EXPECT_TRUE(line->closed);
            ASSERT_EQ(line->vertices.size(), 4u);
            EXPECT_NEAR(line->vertices[2].x, kC.e, 1e-9);
            EXPECT_NEAR(line->vertices[2].y, kC.n, 1e-9);
        } else {
            ++points;
            EXPECT_NE(katana::entity::toString(entity.properties.at("point")), "A");
        }
    }
    EXPECT_EQ(points, 3u);
    EXPECT_EQ(polylines, 1u);
    ASSERT_TRUE(document.undo().ok());
    EXPECT_TRUE(document.model().entities.empty());
}

TEST(SurveyTools, TraverseLegsAreReadFromAFieldBookAndABadLineIsNamed)
{
    const auto legs = parseTraverseLegs("# station angle distance\n"
                                        "A 286d53m11.63s 200.000\n"
                                        "\n"
                                        "B, 249:23:26.4, 250\n");
    ASSERT_TRUE(legs.ok()) << legs.error().describe();
    ASSERT_EQ(legs->size(), 2u);
    EXPECT_EQ((*legs)[1].stationId, "B");
    EXPECT_NEAR((*legs)[1].angle, (249.0 + 23.0 / 60.0 + 26.4 / 3600.0) * kPi / 180.0, 1e-15);
    EXPECT_EQ((*legs)[1].distance, 250.0);

    const auto bad = parseTraverseLegs("A 10d 100\nB 12x 100\n");
    ASSERT_FALSE(bad.ok());
    EXPECT_EQ(bad.error().code, ErrorCode::ParseFailure);
    EXPECT_TRUE(contains(bad.error().context, "line 2")) << bad.error().context;
    EXPECT_FALSE(parseTraverseLegs("A 10 20 30 100").ok()) << "a blank inside the angle";
    EXPECT_EQ(parseTraverseLegs("# nothing\n").error().code, ErrorCode::InvalidArgument);
}

// ---- level book ----------------------------------------------------------------------------

namespace {

// Reduced by hand, height of collimation:
//   BM1   BS 1.500               HPC 100.000 + 1.500 = 101.500   RL 100.000
//   A     IS 2.000                                     101.500 - 2.000 =  99.500
//   CP1   FS 0.500  BS 2.250     RL 101.500 - 0.500 = 101.000, HPC 103.250
//   B     IS 1.250                                     103.250 - 1.250 = 102.000
//   BM2   FS 3.260                                     103.250 - 3.260 =  99.990
// Check: sum BS 3.750 - sum FS 3.760 = -0.010 = 99.990 - 100.000.
// BM2 is known at 100.000, so the misclosure is 99.990 - 100.000 = -0.010.
const char* const kBook = "# point  BS     IS     FS\n"
                          "BM1  1.500  -      -\n"
                          "A    -      2.000  -\n"
                          "CP1  2.250  -      0.500  100\n"
                          "B    -      1.250  -\n"
                          "BM2  -      -      3.260  150\n";

LevelBookSpec bookSpec(survey::LevelAdjustment adjustment)
{
    LevelBookSpec spec;
    spec.name = "Hand book";
    spec.startLevel = 100.0;
    spec.closingLevel = 100.0;
    spec.lines = *parseLevelBook(kBook);
    spec.adjustment = adjustment;
    return spec;
}

} // namespace

TEST(SurveyTools, AHandReducedLevelBookIsReproducedLineByLine)
{
    const auto result = computeLevelBook(bookSpec(survey::LevelAdjustment::None));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const auto& rows = result->rows;
    ASSERT_EQ(rows.size(), 5u);
    EXPECT_NEAR(*rows[0].heightOfCollimation, 101.5, 1e-12);
    EXPECT_NEAR(rows[1].reducedLevel, 99.5, 1e-12);
    EXPECT_NEAR(rows[2].reducedLevel, 101.0, 1e-12);
    EXPECT_NEAR(*rows[2].heightOfCollimation, 103.25, 1e-12);
    EXPECT_NEAR(rows[3].reducedLevel, 102.0, 1e-12);
    EXPECT_NEAR(rows[4].reducedLevel, 99.99, 1e-12);
    EXPECT_FALSE(rows[1].heightOfCollimation.has_value()) << "an intersight sets up nothing";

    EXPECT_NEAR(result->sumBacksights, 3.75, 1e-12);
    EXPECT_NEAR(result->sumForesights, 3.76, 1e-12);
    EXPECT_NEAR(result->riseMinusFall, -0.01, 1e-12);
    EXPECT_NEAR(result->lastMinusFirst, -0.01, 1e-12);
    ASSERT_TRUE(result->misclosure.has_value());
    EXPECT_NEAR(*result->misclosure, -0.01, 1e-12);

    const std::string report = formatLevelBookReport(*result);
    EXPECT_TRUE(contains(report, "Check: sum BS 3.750 - sum FS 3.760 = -0.010; last RL - first "
                                 "RL = -0.010"))
        << report;
    EXPECT_TRUE(contains(report, "Misclosure -0.0100 (computed - known)")) << report;
}

TEST(SurveyTools, AdjustingBySetupsGivesEveryReadingOfASetupItsShare)
{
    // Two setups share +0.010: setup 1 (A, CP1) +0.005, setup 2 (B, BM2) +0.010.
    const auto result = computeLevelBook(bookSpec(survey::LevelAdjustment::BySetups));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const auto& rows = result->rows;
    EXPECT_NEAR(rows[1].correction, 0.005, 1e-12);
    EXPECT_NEAR(rows[1].adjustedLevel, 99.505, 1e-12);
    EXPECT_NEAR(rows[2].adjustedLevel, 101.005, 1e-12);
    EXPECT_NEAR(rows[3].correction, 0.010, 1e-12);
    EXPECT_NEAR(rows[3].adjustedLevel, 102.010, 1e-12);
    EXPECT_NEAR(rows[4].adjustedLevel, 100.0, 1e-12) << "the run closes on the known level";
}

TEST(SurveyTools, AdjustingByDistanceWeighsEachSetupByTheLengthLevelled)
{
    // Setup lengths 100 and 150, total 250: setup 1 gets 0.010 * 100/250 =
    // +0.004 and setup 2 the whole +0.010.
    const auto result = computeLevelBook(bookSpec(survey::LevelAdjustment::ByDistance));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    const auto& rows = result->rows;
    EXPECT_NEAR(rows[1].correction, 0.004, 1e-12);
    EXPECT_NEAR(rows[2].adjustedLevel, 101.004, 1e-12);
    EXPECT_NEAR(rows[3].correction, 0.010, 1e-12);
    // Allowable 12 mm * sqrt(0.250 km) = 6 mm; the 10 mm misclosure exceeds it.
    ASSERT_TRUE(result->allowable.has_value());
    EXPECT_NEAR(*result->allowable, 0.006, 1e-12);
    EXPECT_TRUE(contains(formatLevelBookReport(*result), "EXCEEDS it"));
}

TEST(SurveyTools, AMisclosureInsideTheAllowanceIsReportedAsWithinIt)
{
    // Two kilometres levelled: 12 mm * sqrt(2) = 16.97 mm > 10 mm.
    LevelBookSpec spec = bookSpec(survey::LevelAdjustment::None);
    spec.lines[2].distance = 1000.0;
    spec.lines[4].distance = 1000.0;
    const auto result = computeLevelBook(spec);
    ASSERT_TRUE(result.ok());
    EXPECT_NEAR(*result->allowable, 0.012 * std::sqrt(2.0), 1e-12);
    EXPECT_TRUE(contains(formatLevelBookReport(*result), "within it"));
}

TEST(SurveyTools, ALengthMissingFromOneSetupIsNotTakenAsZero)
{
    LevelBookSpec spec = bookSpec(survey::LevelAdjustment::ByDistance);
    spec.lines[4].distance.reset();
    const auto refused = computeLevelBook(spec);
    ASSERT_FALSE(refused.ok()) << "weighting by a length of zero nobody measured";
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);

    spec.adjustment = survey::LevelAdjustment::None;
    const auto unadjusted = computeLevelBook(spec);
    ASSERT_TRUE(unadjusted.ok());
    EXPECT_FALSE(unadjusted->allowable.has_value());
    EXPECT_FALSE(unadjusted->totalDistance.has_value());
    EXPECT_TRUE(contains(formatLevelBookReport(*unadjusted), "no allowable misclosure"));
}

TEST(SurveyTools, AMisbookedLevelBookIsRefusedAtTheLineThatIsWrong)
{
    const auto refuse = [](const char* text) {
        LevelBookSpec spec;
        spec.lines = *parseLevelBook(text);
        const auto result = computeLevelBook(spec);
        EXPECT_FALSE(result.ok()) << text;
        return result.ok() ? std::string{} : result.error().context;
    };
    EXPECT_EQ(refuse("A - 1.0 -\nB - - 1.0\n"), "line 1 (A)") << "must open on a backsight";
    EXPECT_EQ(refuse("A 1.0 - -\nB 1.0 1.0 -\nC - - 1.0\n"), "line 2 (B)")
        << "an intersight line with a backsight";
    EXPECT_EQ(refuse("A 1.0 - -\nB 1.0 - -\nC - - 1.0\n"), "line 2 (B)")
        << "a backsight with no foresight is not a change point";
    EXPECT_EQ(refuse("A 1.0 - -\nB 1.0 - 1.0\n"), "line 2 (B)") << "ends on an open setup";
    EXPECT_EQ(refuse("A 1.0 - -\nB - 1.0 - 50\nC - - 1.0\n"), "line 2 (B)")
        << "a distance on an intersight";

    EXPECT_EQ(parseLevelBook("A - - -\n").error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(parseLevelBook("A 1.0 -\n").error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(parseLevelBook("A 1.0 - - -5\n").error().code, ErrorCode::ParseFailure);
}

// ---- angle calculator ----------------------------------------------------------------------

TEST(SurveyTools, AnglesConvertByTheDefinitionsOfTheirUnits)
{
    // A right angle is 90 degrees, 100 gon (400 to the circle), pi/2 rad.
    const auto right = convertAngle("90", AngleInputUnit::Degrees);
    ASSERT_TRUE(right.ok());
    EXPECT_NEAR(right->gons, 100.0, 1e-12);
    EXPECT_NEAR(right->radians, 0.5 * kPi, 1e-15);
    EXPECT_NEAR(right->backAzimuth, 1.5 * kPi, 1e-15);
    const std::string report = formatAngleConversion(*right);
    EXPECT_TRUE(contains(report, "100.00000000 gon")) << report;
    EXPECT_TRUE(contains(report, "bearing N 90\xC2\xB0" "00'00.00\" E")) << report;
    EXPECT_TRUE(contains(report, "reverse 270\xC2\xB0" "00'00.00\"")) << report;

    const auto gons = convertAngle("200", AngleInputUnit::Gons);
    ASSERT_TRUE(gons.ok());
    EXPECT_NEAR(gons->degrees, 180.0, 1e-12);

    const auto bearing = convertAngle("S 45 W", AngleInputUnit::Bearing);
    ASSERT_TRUE(bearing.ok()) << bearing.error().describe();
    EXPECT_NEAR(bearing->degrees, 225.0, 1e-12);

    // -30 degrees read as a direction is 330.
    const auto negative = convertAngle("-30", AngleInputUnit::Degrees);
    ASSERT_TRUE(negative.ok());
    EXPECT_NEAR(negative->azimuth, 11.0 * kPi / 6.0, 1e-15);
    EXPECT_FALSE(convertAngle("pi", AngleInputUnit::Radians).ok());
}

// ---- coordinate converter ------------------------------------------------------------------
//
// UTM by definition (NGA.SIG.0012, the UTM grid): zone n has its central
// meridian at 6n - 183 degrees, so zone 30's is 3 degrees WEST; the false
// easting is 500 000 m, the false northing 0 m in the northern hemisphere, and
// the scale factor on the central meridian is 0.9996. So latitude 0, longitude
// -3 is E 500 000, N 0 exactly, with k = 0.9996 and no convergence there.

TEST(SurveyTools, TheEquatorOnZone30sCentralMeridianIsTheUtmFalseOrigin)
{
    const auto conversion = convertCoordinates("4326", "EPSG:32630", "origin 0 -3\n");
    ASSERT_TRUE(conversion.ok()) << conversion.error().describe();
    ASSERT_EQ(conversion->rows.size(), 1u);
    const auto& row = conversion->rows[0];
    EXPECT_EQ(row.label, "origin");
    EXPECT_TRUE(conversion->sourceGeographic);
    EXPECT_FALSE(conversion->targetGeographic);
    EXPECT_NEAR(row.outFirst, 500000.0, 1e-6) << "easting";
    EXPECT_NEAR(row.outSecond, 0.0, 1e-6) << "northing";
    ASSERT_TRUE(row.targetFactors.has_value());
    EXPECT_NEAR(row.targetFactors->pointScaleFactor(), 0.9996, 1e-9);
    EXPECT_NEAR(row.targetFactors->convergenceDegrees, 0.0, 1e-9);
    EXPECT_FALSE(row.sourceFactors.has_value()) << "a geographic system has no grid factors";
    const std::string report = formatCoordinateConversion(*conversion);
    EXPECT_TRUE(contains(report, "500000.000 0.000")) << report;
    EXPECT_TRUE(contains(report, "k 0.99960000")) << report;

    // And back: grid to geographic, latitude first in the answer.
    const auto back = convertCoordinates("32630", "4326", "500000 0");
    ASSERT_TRUE(back.ok()) << back.error().describe();
    EXPECT_NEAR(back->rows[0].outFirst, 0.0, 1e-9) << "latitude";
    EXPECT_NEAR(back->rows[0].outSecond, -3.0, 1e-9) << "longitude";
    EXPECT_EQ(back->rows[0].label, "P1");
}

TEST(SurveyTools, AGeographicPointMayBeTypedInDegreesMinutesAndSeconds)
{
    // -3 degrees written as -3:00:00.
    const auto conversion = convertCoordinates("4326", "32630", "0:00:00 -3:00:00");
    ASSERT_TRUE(conversion.ok()) << conversion.error().describe();
    EXPECT_NEAR(conversion->rows[0].outFirst, 500000.0, 1e-6);
}

TEST(SurveyTools, TheConverterRefusesWhatItCannotDoProperly)
{
    EXPECT_FALSE(convertCoordinates("999999", "4326", "0 0").ok()) << "no such EPSG code";
    EXPECT_EQ(convertCoordinates("", "4326", "0 0").error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(convertCoordinates("4326", "32630", "\n# nothing\n").error().code,
              ErrorCode::InvalidArgument);
    const auto bad = convertCoordinates("32630", "4326", "P1 500000 north");
    ASSERT_FALSE(bad.ok());
    EXPECT_EQ(bad.error().code, ErrorCode::ParseFailure);
    EXPECT_TRUE(contains(bad.error().context, "line 1"));
}

TEST(SurveyTools, SelectedPointsBecomeConverterLinesOnlyForAProjectedSource)
{
    Document document;
    const EntityId p = addPoint(document, Point2(500000.25, 12.5), "CP 1");
    document.selection().set({p});
    const auto lines = conversionLinesForSelection(document, "32630");
    ASSERT_TRUE(lines.ok()) << lines.error().describe();
    EXPECT_EQ(*lines, "CP 1 500000.25 12.5\n");
    const auto conversion = convertCoordinates("32630", "4326", *lines);
    ASSERT_TRUE(conversion.ok()) << conversion.error().describe();
    EXPECT_EQ(conversion->rows[0].label, "CP 1") << "a name with a blank survives";

    EXPECT_EQ(conversionLinesForSelection(document, "4326").error().code,
              ErrorCode::InvalidArgument)
        << "the drawing's x is an easting, not a latitude";
    document.selection().clear();
    EXPECT_EQ(conversionLinesForSelection(document, "32630").error().code,
              ErrorCode::InvalidState);
}

// ---- the command line ----------------------------------------------------------------------

TEST(SurveyToolsInterpreter, InversePrintsTheSameReportAsTheDialog)
{
    Document document;
    CommandInterpreter interpreter(document);
    const auto reply = interpreter.run("INVERSE 1000,5000 1003,5004");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_EQ(*reply, formatInverseReport(*computeInverse(at(1000, 5000), at(1003, 5004))));
    EXPECT_TRUE(contains(*reply, "Horizontal distance 5.000"));
}

TEST(SurveyToolsInterpreter, InverseTakesPointIdsAndTheEndsOfALine)
{
    Document document;
    CommandInterpreter interpreter(document);
    const EntityId a = addPoint(document, Point2(1000, 5000), "A", 10.0);
    const EntityId b = addPoint(document, Point2(1003, 5004), "B", 12.0);
    const auto points = interpreter.run("INVERSE " + std::to_string(a) + " " + std::to_string(b));
    ASSERT_TRUE(points.ok()) << points.error().describe();
    EXPECT_TRUE(contains(*points, "Height difference +2.000")) << *points;
    EXPECT_TRUE(contains(*points, "(A)")) << *points;

    ASSERT_TRUE(interpreter.run("LINE 0,0 -1,-1").ok());
    const EntityId line = document.lastCreatedEntities().front();
    const auto ends = interpreter.run("INVERSE " + std::to_string(line));
    ASSERT_TRUE(ends.ok()) << ends.error().describe();
    EXPECT_TRUE(contains(*ends, "Bearing S 45\xC2\xB0" "00'00.00\" W")) << *ends;

    EXPECT_FALSE(interpreter.run("INVERSE").ok());
    EXPECT_FALSE(interpreter.run("INVERSE 1,1 1,1").ok()) << "coincident";
    EXPECT_FALSE(interpreter.run("INVERSE " + std::to_string(a)).ok()) << "a point has no two ends";
}

TEST(SurveyToolsInterpreter, ForwardCreatesANamedPointThatOneUndoRemoves)
{
    Document document;
    CommandInterpreter interpreter(document);
    // N 36°52'11.63" E is the 3-4-5 direction to 0.005" (0.1 micrometre at 5 m).
    const auto reply = interpreter.run("FORWARD 1000,5000,20 N36d52m11.63sE 5 -1 P7");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_TRUE(contains(*reply, "Created on layer 0")) << *reply;
    ASSERT_EQ(document.model().entities.size(), 1u);
    const Entity& point = *document.model().entities.find(document.lastCreatedEntities().front());
    const Point2 at = std::get<katana::entity::PointGeometry>(point.geometry).position;
    EXPECT_NEAR(at.x, 1003.0, 1e-6);
    EXPECT_NEAR(at.y, 5004.0, 1e-6);
    EXPECT_EQ(katana::entity::toString(point.properties.at("point")), "P7");
    EXPECT_EQ(katana::entity::heightsOf(point.properties, 1).front(), std::optional<double>(19.0));
    ASSERT_TRUE(interpreter.run("UNDO").ok());
    EXPECT_TRUE(document.model().entities.empty());

    // RADIATE, a quoted bearing, no height difference, a numeric name.
    const auto radiate = interpreter.run("RADIATE 0,0 \"S 45 00 00 W\" 10 - 102");
    ASSERT_TRUE(radiate.ok()) << radiate.error().describe();
    const Entity& second =
        *document.model().entities.find(document.lastCreatedEntities().front());
    const Point2 there = std::get<katana::entity::PointGeometry>(second.geometry).position;
    EXPECT_NEAR(there.x, -10.0 / std::sqrt(2.0), 1e-9);
    EXPECT_NEAR(there.y, -10.0 / std::sqrt(2.0), 1e-9);
    EXPECT_EQ(katana::entity::toString(second.properties.at("point")), "102")
        << "the fifth field is the name, never a height";
    EXPECT_FALSE(second.properties.contains(std::string(katana::entity::kElevationProperty)));

    EXPECT_FALSE(interpreter.run("FORWARD 0,0 45").ok());
    EXPECT_FALSE(interpreter.run("FORWARD 0,0 45 10 2").ok())
        << "a height difference from a start with no height";
    EXPECT_FALSE(interpreter.run("FORWARD 0,0 X45 10").ok());
}

TEST(SurveyToolsInterpreter, AreaMeasuresTheSelectionOrTheIdsGiven)
{
    Document document;
    CommandInterpreter interpreter(document);
    EXPECT_EQ(interpreter.run("AREA").error().code, ErrorCode::InvalidState) << "nothing selected";
    ASSERT_TRUE(interpreter.run("RECT 0,0 100,50").ok());
    const EntityId rectangle = document.lastCreatedEntities().front();
    ASSERT_TRUE(interpreter.run("SELECT ALL").ok());
    const auto selected = interpreter.run("AREA");
    ASSERT_TRUE(selected.ok()) << selected.error().describe();
    EXPECT_TRUE(contains(*selected, "area 5000.000 m\xC2\xB2 (0.5000 ha)")) << *selected;
    const auto byId = interpreter.run("AREA " + std::to_string(rectangle));
    ASSERT_TRUE(byId.ok());
    EXPECT_EQ(*byId, *selected);
    EXPECT_FALSE(interpreter.run("AREA x").ok());
}

TEST(SurveyToolsInterpreter, HelpDocumentsTheSurveyVerbs)
{
    const std::string help = CommandInterpreter::helpText();
    for (const char* verb : {"INVERSE", "FORWARD", "RADIATE", "AREA"}) {
        EXPECT_TRUE(contains(help, verb)) << verb;
    }
}
