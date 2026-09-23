#pragma once

// The survey tools of the Survey menu and the command line (PLAN.MD 45 slice 9,
// and the report half of slice 13): inverse, forward (radiate), area, traverse,
// level book, the angle calculator and the coordinate converter.
//
// NOTHING HERE IS NEW SURVEYING. Every number comes from katana::survey (cogo,
// traverse, leveling, network_adjustment, angles) or katana::geodesy (the PROJ
// transformer and grid factors); this file only moves drawing entities in and
// out of those libraries and puts the answers into words. A second inverse or
// a second compass rule written here would be a second answer to the same
// question, and the two would drift.
//
// Each tool is a structured result plus ONE formatter. The Survey menu's
// dialogs and the command line (INVERSE, FORWARD, AREA) both print what the
// formatter returns, so a dialog and a typed command cannot describe one
// computation two ways. The Qt layer gathers text and calls these; it computes
// nothing survey-specific itself.
//
// Conventions, all inherited rather than invented here:
//   * Drawing coordinates are x = easting, y = northing (as everywhere in the
//     drawing and on the command line: "E,N"). survey::Coordinate2 is
//     (northing, easting); the conversion happens in survey_tools.cpp only.
//     Transposing the two gives a drawing that looks right and is wrong
//     (PLAN.MD 45.5), so the tests use coordinates whose magnitudes differ.
//   * Angles are radians in every struct. Azimuths are clockwise from grid
//     north in [0, 2*pi), as survey/angles.hpp defines them.
//   * Heights are std::optional. ABSENT IS NOT ZERO: an inverse between two
//     points of which one has no height reports no height difference, never
//     one measured from a datum nobody observed.
//   * Lengths are in drawing units, which the project metadata names
//     (Document::metadata().linearUnit, "metre" unless a project says
//     otherwise). Hectares are reported only when that unit is the metre.
//   * No scale factor, convergence or curvature is applied: these are plane
//     (grid) computations, as survey/cogo.hpp states. The coordinate converter
//     REPORTS the grid scale factor and convergence; it does not apply them.
//
// Angle text (surveyAngleFormats() is the one sentence every caller shows):
// what survey::parseDms and survey::parseBearing accept. D M S with a mark
// after each field - 36°52'11.63", 36d52m11.63s, 36:52:11.63, 36-52-11.63, or
// blank separated 36 52 11.63 - or decimal degrees 36.8699; only the last
// field may carry a fraction. The blank separated form is for a box that holds
// one angle: in a line of several fields (a traverse leg, a converter point)
// blanks separate the fields, so an angle there has none inside it.
// A direction may also be a quadrant bearing,
// N 36d52m11.63s E. There is no DDD.MMSS "calculator" notation, because
// 36.5211 would then mean two different angles depending on who typed it.

#include <cstddef>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/geodesy/grid_factors.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/survey/data_model.hpp"
#include "katana/survey/leveling.hpp"
#include "katana/survey/network_adjustment.hpp"
#include "katana/survey/traverse.hpp"

namespace katana::cad {

// Seconds shown in every DMS angle and bearing these tools print: 0.01" is
// 0.05 mm at 1 km, finer than any distance they print (1 mm), so rounding the
// angle never hides a difference the distance shows.
inline constexpr int kSurveySecondsDecimals = 2;

// ---- text in -------------------------------------------------------------------------------

// The accepted angle formats, in one sentence, for error messages and dialogs.
[[nodiscard]] std::string_view surveyAngleFormats();

// An angle (survey::parseDms): radians, sign kept. ParseFailure naming the
// accepted formats otherwise.
[[nodiscard]] core::Result<double> parseSurveyAngle(std::string_view text);

// A direction: a quadrant bearing (N ... E, survey::parseBearing) or an azimuth
// in any parseSurveyAngle format. An azimuth in [0, 2*pi).
[[nodiscard]] core::Result<double> parseSurveyDirection(std::string_view text);

// A finite number, locale-independently (core::parseFiniteDouble): "1.5" is
// never "1,5". ParseFailure naming `what` otherwise.
[[nodiscard]] core::Result<double> parseSurveyNumber(std::string_view text, std::string_view what);

// ---- positions -----------------------------------------------------------------------------

// A place a survey tool works from: typed coordinates, a point entity, or one
// end of a line.
struct SurveyPosition {
    geometry::Point2 point;          // x easting, y northing, drawing units
    std::optional<double> elevation; // absent is not zero
    // The entity it was read from; empty for typed coordinates.
    std::optional<entity::EntityId> entity;
    // What it is on that entity: "point 7", "line 5 start". Empty when typed.
    std::string source;
    // The point's name or number where the entity carries one (the property
    // the survey import writes, SurveyImportOptions::pointNumberProperty).
    std::string name;

    // How a report names it: "point 7 (CP-01)", "point 7", "E 3.000 N 4.000".
    [[nodiscard]] std::string label() const;
};

// A point entity. NotFound for an unknown id; InvalidArgument for an entity
// that is not a point (the message says what it is instead).
[[nodiscard]] core::Result<SurveyPosition> positionOfPoint(const Document& document,
                                                           entity::EntityId id);

// The two ends of a line, or the first and last vertex of an open polyline,
// with the heights the entity carries for them (entity::heightsOf).
// InvalidArgument for anything else, including a closed polyline, whose ends
// are the same place.
[[nodiscard]] core::Result<std::pair<SurveyPosition, SurveyPosition>>
endsOfLine(const Document& document, entity::EntityId id);

// "E,N", "E,N,Z" - easting first, as every point typed into this program is -
// or the id of a point entity. ParseFailure / NotFound / InvalidArgument.
[[nodiscard]] core::Result<SurveyPosition> parseSurveyPosition(const Document& document,
                                                               std::string_view text);

// The selected point entities, in id order (the order SelectionSet keeps).
// Everything selected that is not a point is left out; the dialogs say how
// many were.
[[nodiscard]] std::vector<SurveyPosition> selectedPointPositions(const Document& document);

// ---- inverse -------------------------------------------------------------------------------

struct InverseResult {
    SurveyPosition from;
    SurveyPosition to;
    double deltaEasting = 0.0;  // to - from
    double deltaNorthing = 0.0; // to - from
    double horizontalDistance = 0.0;
    double azimuth = 0.0; // radians, [0, 2*pi), from survey::inverse
    // All three need a height at BOTH ends; nullopt otherwise.
    std::optional<double> heightDifference; // to - from
    std::optional<double> slopeDistance;
    std::optional<double> grade; // heightDifference / horizontalDistance
};

// survey::inverse between two positions. InvalidArgument when they are the
// same ground mark (closer than 0.1 mm, survey/cogo.hpp): the azimuth between
// them is undefined and a report of "0.000 at 0°" would be a made-up answer.
[[nodiscard]] core::Result<InverseResult> computeInverse(const SurveyPosition& from,
                                                         const SurveyPosition& to);
[[nodiscard]] std::string formatInverseReport(const InverseResult& result);

// ---- forward (radiate) ---------------------------------------------------------------------

struct ForwardInput {
    SurveyPosition from;
    double azimuth = 0.0;  // radians, any finite value
    double distance = 0.0; // horizontal, drawing units, > 0
    // The new point's height is from.elevation + this. Given with a start that
    // has no height, it is REFUSED rather than taken from zero.
    std::optional<double> heightDifference;
    std::string name; // written as the new point's name; may be empty
};

struct ForwardResult {
    ForwardInput input;
    geometry::Point2 point; // x easting, y northing
    std::optional<double> elevation;
};

// survey::forward. InvalidArgument for a non-positive or non-finite distance,
// a non-finite azimuth, or a height difference from a start with no height.
[[nodiscard]] core::Result<ForwardResult> computeForward(const ForwardInput& input);
[[nodiscard]] std::string formatForwardReport(const ForwardResult& result);

// The new point as ONE undoable command on the document's current layer, with
// its name and height as the survey import writes them.
[[nodiscard]] core::Result<commands::CommandPtr> forwardPointCommand(const Document& document,
                                                                     const ForwardResult& result);

// ---- area ----------------------------------------------------------------------------------

struct AreaItem {
    entity::EntityId id = entity::kInvalidEntityId;
    entity::EntityType type = entity::EntityType::Polyline;
    double area = 0.0;      // square drawing units, >= 0
    double perimeter = 0.0; // drawing units
};

struct AreaSkip {
    entity::EntityId id = entity::kInvalidEntityId;
    std::string reason; // "an open polyline has no area", ...
};

struct AreaResult {
    std::vector<AreaItem> items; // in the order asked
    std::vector<AreaSkip> skipped;
    double totalArea = 0.0;
    double totalPerimeter = 0.0;
    // The project's linear unit, as its metadata names it, and whether that is
    // the metre - which is the only case in which hectares mean anything.
    std::string linearUnit;
    bool metres = false;
};

// True for "metre", "meter", "metres", "meters" and "m", in any case: the
// spellings a project's linear_unit is written with.
[[nodiscard]] bool isMetreUnit(std::string_view linearUnit);

// Closed polylines (survey::polygonArea and polygonPerimeter, which work
// relative to the first vertex so UTM-sized coordinates keep their precision)
// and circles. Everything else, and a degenerate polygon, is skipped with the
// reason; an id listed more than once is measured once and each repeat is
// skipped as "listed more than once", so no outline is added to the total
// twice. InvalidArgument for an empty list; NotFound for an unknown id;
// InvalidGeometry when nothing in the list has an area - with the reasons,
// grouped and shortened as formatAreaReport lists them (a count per reason and
// at most a dozen ids), so a selection of thousands of points is one short line.
// Self-intersecting outlines are not detected: the signed areas of their lobes
// cancel (survey/cogo.hpp), which the report states.
[[nodiscard]] core::Result<AreaResult> computeArea(const Document& document,
                                                   const std::vector<entity::EntityId>& ids);
[[nodiscard]] std::string formatAreaReport(const AreaResult& result);

// ---- traverse ------------------------------------------------------------------------------

enum class TraverseMethod {
    None,         // misclosures only
    Compass,      // Bowditch: corrections proportional to leg length
    Transit,      // corrections proportional to |latitude| and |departure|
    LeastSquares, // survey::adjustTraverse, weighted by `precision`
};

[[nodiscard]] std::string_view toString(TraverseMethod method);

// A direction a traverse is hung on: a known azimuth, or a reference mark whose
// azimuth from the station is computed (survey::inverse). The mark wins when
// both are given, because a mark is what a surveyor actually sights.
struct TraverseOrientation {
    std::optional<geometry::Point2> referenceMark; // x easting, y northing
    double azimuth = 0.0;                          // radians
};

// The a-priori precision the least-squares method weights with. The defaults
// are a common construction total-station specification - 5" angles and
// 2 mm + 2 ppm distances (the class manufacturers sell as "5 second") - and
// are there to be REPLACED with the instrument's own; the report prints them
// so a result is never read without the weights it came from. The azimuth
// sigma of 0.01" holds a loop's first-leg azimuth practically fixed, the value
// survey/network_adjustment.hpp suggests for it.
[[nodiscard]] survey::TraverseStochasticModel defaultTraversePrecision();

struct TraverseSpec {
    std::string name = "Traverse";
    survey::TraverseKind kind = survey::TraverseKind::Open;
    // Known coordinates of legs.front().stationId.
    geometry::Point2 start;
    // Open and Link: the direction from the start station to its backsight;
    // legs.front().angle is turned from it. ClosedLoop: the azimuth of the
    // first leg (known or assumed), as survey::Traverse defines it.
    TraverseOrientation startOrientation;
    // One per occupied station, in order: its id, the angle turned clockwise
    // from the back station to the forward one, and the horizontal distance to
    // the forward station. A loop's last leg returns to its first station.
    std::vector<survey::TraverseSetup> legs;
    // Open and Link: the station the last leg ends on. Link: its known
    // coordinates, and optionally the angle turned there from the last
    // traverse station to a reference mark, with that mark's direction - the
    // angular check.
    std::string endStation;
    geometry::Point2 end;
    std::optional<double> closingAngle;
    TraverseOrientation closingOrientation;

    TraverseMethod method = TraverseMethod::Compass;
    // Distribute the angular misclosure equally over the angles first (the
    // classical methods; least squares weighs the angles itself).
    bool balanceAngles = true;
    survey::TraverseStochasticModel precision = defaultTraversePrecision();
};

struct TraverseStationRow {
    std::string id;
    geometry::Point2 unadjusted; // from the (balanced) angles and observed distances
    geometry::Point2 adjusted;   // after the chosen method
    // Least squares only: the adjusted standard deviations (zero for control).
    std::optional<double> sigmaEasting;
    std::optional<double> sigmaNorthing;
    // Start and closing control: known, not computed. Not drawn as new points.
    bool control = false;
};

struct TraverseToolResult {
    TraverseSpec spec;
    double startAzimuth = 0.0; // the orientation actually used, radians
    std::optional<double> closingReferenceAzimuth;
    // survey::computeTraverse with the classical method (None for least
    // squares): the angular and linear misclosures and the precision ratio.
    survey::TraverseResult classical;
    std::optional<survey::HorizontalAdjustmentResult> leastSquares;
    std::vector<TraverseStationRow> stations; // traverse order, the start first
};

// Builds the survey::Traverse and runs the chosen method on it. The errors are
// survey::validateTraverse's (a repeated station, a missing end, ...) and
// survey::adjustTraverse's, unchanged, plus InvalidArgument for a reference
// mark on top of its station (no direction).
[[nodiscard]] core::Result<TraverseToolResult> computeTraverseTool(const TraverseSpec& spec);
[[nodiscard]] std::string formatTraverseReport(const TraverseToolResult& result);

// ONE undoable command: a point entity for every computed (non-control)
// station, named by its id, and the traverse as one polyline through every
// station in order - closed for a loop. On the document's current layer.
[[nodiscard]] core::Result<commands::CommandPtr> traverseCommand(const Document& document,
                                                                 const TraverseToolResult& result);

// A traverse field book, one leg per line: "station angle distance", blank or
// comma separated; '#' starts a comment. Angles in any parseSurveyAngle format
// without blanks inside it (36d52m11.63s, 36:52:11.63). ParseFailure naming the
// line and what is wrong with it; InvalidArgument when there are no legs.
[[nodiscard]] core::Result<std::vector<survey::TraverseSetup>>
parseTraverseLegs(std::string_view text);

// ---- level book ----------------------------------------------------------------------------

// The common third-order allowance for a level run, 12 mm * sqrt(K km), in
// metres per sqrt(km) - the figure survey/leveling.hpp names for
// allowableLevelMisclosure. A default the user replaces with their
// specification's; the report prints the k it used.
inline constexpr double kThirdOrderLevelAllowance = 0.012;

// One line of a level book, in the height-of-collimation layout.
struct LevelBookLine {
    std::string point;
    std::optional<double> backsight;
    std::optional<double> intersight;
    std::optional<double> foresight;
    // On a foresight line only: the length levelled in the setup that ends
    // there (backsight + foresight sight lengths), metres. Absent when not
    // recorded - and then the allowable misclosure is not stated, rather than
    // computed over a length of zero.
    std::optional<double> distance;
    std::size_t lineNumber = 0; // 1-based line of the text it was read from
};

// "point BS IS FS [distance]" per line, '-' for an empty column, blank or comma
// separated, '#' starts a comment. A change point carries its foresight and its
// backsight on one line. ParseFailure naming the line otherwise.
[[nodiscard]] core::Result<std::vector<LevelBookLine>> parseLevelBook(std::string_view text);

struct LevelBookSpec {
    std::string name = "Level run";
    // Reduced level of the first line's point, the opening benchmark.
    double startLevel = 0.0;
    // Known reduced level of the last line's point, when the run closes on a
    // benchmark. Without it there is no misclosure and no adjustment.
    std::optional<double> closingLevel;
    std::vector<LevelBookLine> lines;
    survey::LevelAdjustment adjustment = survey::LevelAdjustment::None;
    double allowanceK = kThirdOrderLevelAllowance; // metres per sqrt(km)
};

struct LevelBookRow {
    LevelBookLine line;
    // The instrument setup (1-based) the row's intersight or foresight was
    // read from; for the opening backsight, 0.
    std::size_t setup = 0;
    // Height of collimation established on this row: the opening line and
    // every change point carry one.
    std::optional<double> heightOfCollimation;
    double reducedLevel = 0.0; // unadjusted
    double correction = 0.0;
    double adjustedLevel = 0.0;
};

struct LevelBookResult {
    LevelBookSpec spec;
    std::vector<LevelBookRow> rows; // one per line, in order
    survey::LevelRunResult run;     // the survey library's own answer
    double sumBacksights = 0.0;
    double sumForesights = 0.0;
    // The arithmetic check of every level book: sum BS - sum FS must equal the
    // last reduced level minus the first. Both sides are reported, not a
    // verdict, because they are computed from the same readings by different
    // routes and a difference in them is a booking error, not a misclosure.
    double riseMinusFall = 0.0;       // sumBacksights - sumForesights
    double lastMinusFirst = 0.0;      // last unadjusted RL - start level
    std::optional<double> misclosure; // computed - known closing level
    std::optional<double> totalDistance; // absent unless every setup has one
    std::optional<double> allowable;     // allowanceK * sqrt(km), with the distance
};

// survey::computeLevelRun over the book's setups, with the intersights reduced
// from the height of collimation the library computed for their setup and
// corrected by that setup's correction - the textbook rule that every reading
// from one instrument position shares its correction. The library does not
// model intersights (survey/leveling.hpp), so this is the ONE place they are
// reduced. InvalidArgument naming the line for a book that does not start with
// a lone backsight, has a backsight away from a change point, a reading row
// with no reading, or does not end on a foresight; the library's errors
// otherwise (an adjustment without a closing level, ...).
[[nodiscard]] core::Result<LevelBookResult> computeLevelBook(const LevelBookSpec& spec);
[[nodiscard]] std::string formatLevelBookReport(const LevelBookResult& result);

// ---- angle calculator ----------------------------------------------------------------------

enum class AngleInputUnit {
    Degrees, // DMS or decimal degrees, parseSurveyAngle
    Gons,    // 400 to the circle
    Radians,
    Bearing, // a quadrant bearing, N 45d30m E
};

struct AngleConversion {
    AngleInputUnit unit = AngleInputUnit::Degrees;
    double radians = 0.0; // as entered, sign and size kept
    double degrees = 0.0;
    double gons = 0.0;
    // The same value read as a direction: wrapped into [0, 2*pi), its
    // quadrant bearing and the reverse direction.
    double azimuth = 0.0;
    double backAzimuth = 0.0;
};

[[nodiscard]] core::Result<AngleConversion> convertAngle(std::string_view text,
                                                         AngleInputUnit unit);
[[nodiscard]] std::string formatAngleConversion(const AngleConversion& conversion);

// ---- coordinate converter ------------------------------------------------------------------

struct CoordinateConversionRow {
    std::string label;
    // As entered and as converted, each in its system's own terms: easting and
    // northing for a projected system, latitude and longitude (decimal
    // degrees) for a geographic one. Named so that a transposition shows.
    double inFirst = 0.0;   // easting or latitude
    double inSecond = 0.0;  // northing or longitude
    double outFirst = 0.0;  // easting or latitude
    double outSecond = 0.0; // northing or longitude
    // Grid scale factor and convergence at the point, for each side that is
    // projected (geodesy::GridFactorCalculator).
    std::optional<geodesy::GridFactors> sourceFactors;
    std::optional<geodesy::GridFactors> targetFactors;
};

struct CoordinateConversion {
    std::string sourceName;
    std::string targetName;
    bool sourceGeographic = false;
    bool targetGeographic = false;
    std::string operation;                 // what PROJ applies, as it names it
    std::optional<double> accuracyMetres;  // as the authority declares it
    std::vector<CoordinateConversionRow> rows;
};

// Converts typed points from one coordinate system to another through
// geodesy::CoordinateTransformer - which REFUSES a ballpark (no datum shift)
// operation, so a datum change it cannot do properly is an error and not a
// point hundreds of metres out. `source` and `target` are an EPSG code ("4326",
// "EPSG:4326") or anything PROJ accepts. `points` is one point per line,
// "[label] first second", where first/second are easting and northing for a
// projected system and LATITUDE and LONGITUDE for a geographic one (decimal
// degrees or any parseSurveyAngle format WITHOUT BLANKS INSIDE IT - -33:51:24.5,
// -33d51m24.5s - since blanks separate the label and the two coordinates;
// negative is south and west). The label is everything before the last two
// fields and may hold blanks and one number ("102", "CP 1"); a label holding
// two or more numbers (angles, for a geographic source) is refused as an angle
// typed with blanks or a third coordinate, rather than read as a point that was
// never meant. Heights are not converted. Errors: InvalidCRS / NotFound /
// Unsupported from geodesy, ParseFailure naming the line, InvalidArgument for
// no points.
[[nodiscard]] core::Result<CoordinateConversion>
convertCoordinates(std::string_view source, std::string_view target, std::string_view points);
[[nodiscard]] std::string formatCoordinateConversion(const CoordinateConversion& conversion);

// The selected point entities as convertCoordinates lines for `source`:
// "name easting northing", with '#', ',' - and the blanks of a name holding two
// numbers, "STN 3 4" - turned into '_' so the name reads back as one label.
// The drawing's own x and y are easting and northing,
// so a geographic source system is refused (InvalidArgument) rather than having
// its latitude filled with an easting. NotFound / InvalidCRS from geodesy.
[[nodiscard]] core::Result<std::string> conversionLinesForSelection(const Document& document,
                                                                    std::string_view source);

} // namespace katana::cad
