#pragma once

// Survey data model (PLAN.MD Phase 10).
//
// Surveying entities are plain values, kept apart from graphical CAD entities.
//
// Conventions
//   * Lengths are metres, angles radians, standard deviations in the unit of the
//     value they describe. Every observation carries an a-priori standard
//     deviation `sigma` > 0.
//   * Azimuths and horizontal angles are clockwise, in [0, 2*pi).
//   * Points are referenced by their string id. Containers that are iterated for
//     output are ordered (vector / std::map), never hash based, so results do not
//     depend on hashing.

#include <cstddef>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <variant>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/survey/coordinate.hpp"

namespace katana::survey {

// ---- Provenance ----------------------------------------------------------------

// The file name of `supplied`, with any directory the source itself gave stripped
// off. "." and ".." are not names and come back empty.
//
// SECURITY, and this is the reason the function exists. A field file is untrusted
// text and it names other files - a coordinate list beside it, a raw data file, a
// job. A name like "..\..\..\Windows\win.ini" must never become a path this
// program opens and must never be recorded as though it were one: a file must not
// be able to choose what gets read. Only the NAME survives; a caller that wants
// the referenced file looks for it beside the file that named it and nowhere
// else. That is the rule the 12d reader already follows for its ref_data
// references (referencedCloud in src/katana_interop/archive12d.cpp).
//
// Separators are cut here rather than by std::filesystem::path::filename()
// because what counts as a separator THERE depends on the host: a POSIX build
// does not treat '\' as one, so a Windows path inside a file read on Linux would
// pass through whole. An untrusted name has to be cut the same way everywhere.
// ':' is cut as well, so that a drive-relative "C:job.gsi" and an NTFS alternate
// data stream "job.gsi:hidden" both come back as one plain name.
[[nodiscard]] std::string sourceFileName(std::string_view supplied);

// Where an imported value came from: enough to point at the exact record of the
// exact file, and no more.
//
// Every imported point, observation, station and feature carries one. Two things
// need it. An audit has to be able to answer "which file says this mark is
// here?", and reprocessing has to be able to find the values a parser produced
// when that parser is fixed - a value with no provenance cannot be found, so it
// silently keeps whatever the old parser made of it.
//
// `fileName` is a NAME, never a path: see sourceFileName().
struct SourceRecord {
    std::string manufacturer;     // "Leica"; empty for a value this program computed
    std::string format;           // "GSI-16", as the format descriptor names it
    std::string formatVersion;    // the variant the parser ACTUALLY used, e.g. "16 byte words"
    std::string fileName;         // file name only, no directory
    std::size_t recordNumber = 0; // 1-based record or line; 0 when the source has none

    // True when this says anything at all. A default SourceRecord means "not
    // imported", which is not the same as "imported from somewhere unnamed".
    [[nodiscard]] bool known() const { return !format.empty() || !fileName.empty(); }

    friend bool operator==(const SourceRecord&, const SourceRecord&) = default;
};

// "site.gsi record 412 (Leica GSI-16)", or "unknown source" for a default record.
// One phrasing, so five importers do not each invent their own for their errors.
[[nodiscard]] std::string describeSource(const SourceRecord& source);

// How a coordinate came to be.
//
// Not a metadata string, because the distinction between a coordinate that was
// MEASURED and one that was COMPUTED is load bearing twice over:
//   * an audit has to say which numbers in a report were observed and which were
//     derived from them - a derived number is evidence of nothing on its own;
//   * reprocessing has to recompute the derived ones and leave the measured ones
//     exactly as they are. A traverse readjusted after an instrument height is
//     corrected must move the calculated marks and must not touch the control it
//     was hung on.
// Neither is possible from a free-text field that some importers fill in and
// others do not, so this is a field with a closed set of values.
//
// One value per point rather than one per component (northing / easting /
// elevation). The rejected alternative: no field format met so far states the
// horizontal and the vertical provenance separately, and three fields that every
// importer would have to fill identically are three chances to fill them
// inconsistently. A point whose height was levelled onto observed horizontal
// coordinates is Calculated, which is the safer of the two answers.
enum class CoordinateSource {
    Unknown,       // the source did not say
    FieldObserved, // reduced from observations made at the instrument
    Calculated,    // computed here or by the instrument: traverse, resection, adjustment
    Entered,       // keyed in or published: control off a certificate, a design value
};

[[nodiscard]] const char* toString(CoordinateSource source);

struct SurveyPoint {
    std::string id; // unique within a network; doubles as the point name
    double northing = 0.0;
    double easting = 0.0;
    double elevation = 0.0;
    std::string code;        // feature code, e.g. "IP", "EOP"
    std::string description; // free text
    std::map<std::string, std::string> metadata;
    CoordinateSource coordinateSource = CoordinateSource::Unknown;
    SourceRecord source;

    [[nodiscard]] Coordinate2 position() const { return Coordinate2{northing, easting}; }

    friend bool operator==(const SurveyPoint&, const SurveyPoint&) = default;
};

// ---- Observations ------------------------------------------------------------

// Every observation carries the record it was read from, for the reasons given
// beside SourceRecord. It is part of the observation rather than a table beside
// it so that an observation moved between containers cannot lose its origin.
enum class DistanceKind { Horizontal, Slope };

// Distance from `from` to `to`. Slope distances carry the instrument and target
// heights needed to reduce them; both are ignored for horizontal distances.
struct DistanceObservation {
    std::string from;
    std::string to;
    double distance = 0.0; // metres, > 0
    double sigma = 0.0;    // metres
    DistanceKind kind = DistanceKind::Horizontal;
    double instrumentHeight = 0.0;
    double targetHeight = 0.0;
    SourceRecord source;

    friend bool operator==(const DistanceObservation&, const DistanceObservation&) = default;
};

// Clockwise angle measured at `at`, turning from the backsight `from` to the
// foresight `to`: azimuth(at->to) - azimuth(at->from), wrapped into [0, 2*pi).
struct HorizontalAngleObservation {
    std::string at;
    std::string from;
    std::string to;
    double angle = 0.0;
    double sigma = 0.0; // radians
    SourceRecord source;

    friend bool operator==(const HorizontalAngleObservation&,
                           const HorizontalAngleObservation&) = default;
};

// Elevation angle above (+) or below (-) the horizon, in [-pi/2, pi/2].
struct VerticalAngleObservation {
    std::string from;
    std::string to;
    double angle = 0.0;
    double sigma = 0.0; // radians
    double instrumentHeight = 0.0;
    double targetHeight = 0.0;
    SourceRecord source;

    friend bool operator==(const VerticalAngleObservation&,
                           const VerticalAngleObservation&) = default;
};

// Angle from the zenith, in [0, pi]; pi/2 is horizontal.
struct ZenithAngleObservation {
    std::string from;
    std::string to;
    double angle = 0.0;
    double sigma = 0.0; // radians
    double instrumentHeight = 0.0;
    double targetHeight = 0.0;
    SourceRecord source;

    friend bool operator==(const ZenithAngleObservation&,
                           const ZenithAngleObservation&) = default;
};

// Grid azimuth of the line `from` -> `to` (gyro, astronomic or a known bearing).
struct AzimuthObservation {
    std::string from;
    std::string to;
    double azimuth = 0.0;
    double sigma = 0.0; // radians
    SourceRecord source;

    friend bool operator==(const AzimuthObservation&, const AzimuthObservation&) = default;
};

// GNSS baseline `from` -> `to`, already reduced to grid components.
struct GnssBaselineObservation {
    std::string from;
    std::string to;
    double deltaNorthing = 0.0;
    double deltaEasting = 0.0;
    double deltaUp = 0.0;
    double sigmaNorthing = 0.0;
    double sigmaEasting = 0.0;
    double sigmaUp = 0.0;
    SourceRecord source;

    friend bool operator==(const GnssBaselineObservation&,
                           const GnssBaselineObservation&) = default;
};

// GNSS position of `point` in the grid system of the network.
struct GnssPositionObservation {
    std::string point;
    double northing = 0.0;
    double easting = 0.0;
    double elevation = 0.0;
    double sigmaNorthing = 0.0;
    double sigmaEasting = 0.0;
    double sigmaElevation = 0.0;
    SourceRecord source;

    friend bool operator==(const GnssPositionObservation&,
                           const GnssPositionObservation&) = default;
};

// Levelled height difference H(to) - H(from). `length` is the length of the
// level line in metres (0 when unknown); it is informational, the weight always
// comes from `sigma`.
struct LevelDifferenceObservation {
    std::string from;
    std::string to;
    double heightDifference = 0.0;
    double sigma = 0.0; // metres
    double length = 0.0;
    SourceRecord source;

    friend bool operator==(const LevelDifferenceObservation&,
                           const LevelDifferenceObservation&) = default;
};

using Observation =
    std::variant<DistanceObservation, HorizontalAngleObservation, VerticalAngleObservation,
                 ZenithAngleObservation, AzimuthObservation, GnssBaselineObservation,
                 GnssPositionObservation, LevelDifferenceObservation>;

// Short name of the alternative held by `observation`, for diagnostics.
[[nodiscard]] std::string observationKindName(const Observation& observation);

// Ids of the points the observation refers to, in declaration order.
[[nodiscard]] std::vector<std::string> referencedPoints(const Observation& observation);

// Checks the values of an observation on their own: finite numbers, sigma > 0,
// distance > 0, angles in range, distinct non-empty station ids. Does not know
// whether the stations exist; SurveyNetwork::addObservation checks that.
// InvalidSurveyObservation on failure.
[[nodiscard]] katana::core::Status validateObservation(const Observation& observation);

// Copy with azimuths and horizontal angles wrapped into [0, 2*pi). Other
// observations are returned unchanged.
[[nodiscard]] Observation normalizedObservation(Observation observation);

// The record an observation was read from, whichever alternative it holds, so
// that a caller reporting on a mixed list of observations does not have to visit
// the variant itself.
[[nodiscard]] const SourceRecord& observationSource(const Observation& observation);
void setObservationSource(Observation& observation, SourceRecord source);

// ---- Setups and control --------------------------------------------------------

// One instrument setup. Target heights belong to the individual observations.
struct Station {
    std::string id;      // setup identifier, unique within a network
    std::string pointId; // occupied point
    double instrumentHeight = 0.0;

    friend bool operator==(const Station&, const Station&) = default;
};

enum class ControlConstraint {
    Free,     // adjusted like any other unknown
    Fixed,    // held at its published value, removed from the unknowns
    Weighted, // published value enters the adjustment as an observation
};

struct ControlComponent {
    ControlConstraint constraint = ControlConstraint::Free;
    double sigma = 0.0; // metres; required > 0 when Weighted, ignored otherwise

    friend bool operator==(const ControlComponent&, const ControlComponent&) = default;
};

// Declares which coordinate components of a point are control. The published
// values are the coordinates of the SurveyPoint itself.
struct ControlPoint {
    std::string pointId;
    ControlComponent northing;
    ControlComponent easting;
    ControlComponent elevation;

    [[nodiscard]] static ControlPoint fixedHorizontal(std::string pointId);
    [[nodiscard]] static ControlPoint fixedVertical(std::string pointId);
    [[nodiscard]] static ControlPoint fixed3d(std::string pointId);
    [[nodiscard]] static ControlPoint weightedHorizontal(std::string pointId, double sigmaNorthing,
                                                         double sigmaEasting);
    [[nodiscard]] static ControlPoint weightedVertical(std::string pointId, double sigma);

    friend bool operator==(const ControlPoint&, const ControlPoint&) = default;
};

// ---- Traverse ------------------------------------------------------------------

enum class TraverseKind {
    Open,       // starts on control, ends on a new point: no closure check
    ClosedLoop, // returns to its first station; interior-angle procedure
    Link,       // runs from one control point to another
};

// One occupied traverse station.
struct TraverseSetup {
    std::string stationId;
    double angle = 0.0;    // clockwise, from the back station to the forward station
    double distance = 0.0; // horizontal, to the forward station; metres, > 0

    friend bool operator==(const TraverseSetup&, const TraverseSetup&) = default;
};

// Angle turned at the end station of a link traverse, from the last traverse
// station to a reference mark whose azimuth is known.
struct TraverseClosingAngle {
    double angle = 0.0;            // clockwise, last traverse station -> reference mark
    double referenceAzimuth = 0.0; // known azimuth end station -> reference mark

    friend bool operator==(const TraverseClosingAngle&, const TraverseClosingAngle&) = default;
};

// A traverse observed with angles and horizontal distances; self-contained.
//
//   Open / Link   `startAzimuth` is the known azimuth from the first station to
//                 its backsight reference; setups[0].angle is turned from that
//                 reference to the second station. The forward station of the
//                 last setup is `endStationId`.
//   ClosedLoop    `startAzimuth` is the known (or assumed) azimuth of the first
//                 leg setups[0] -> setups[1]; setups[0].angle is turned from the
//                 last station to the second one; the last setup sights forward
//                 to setups[0]. `endStationId`, `end` and `closingAngle` are
//                 unused. A loop oriented on an external reference mark is
//                 modelled as a Link traverse whose end equals its start.
//   Link          `end` holds the known coordinates of `endStationId`;
//                 `closingAngle` is optional (without it there is no angular
//                 check, only a linear one).
struct Traverse {
    std::string name;
    TraverseKind kind = TraverseKind::Open;
    Coordinate2 start; // known coordinates of setups[0]
    double startAzimuth = 0.0;
    std::vector<TraverseSetup> setups;
    std::string endStationId;
    Coordinate2 end;
    std::optional<TraverseClosingAngle> closingAngle;

    friend bool operator==(const Traverse&, const Traverse&) = default;
};

// ---- Field setups, features and the project one import produces -----------------

// One instrument setup as a field file records it: the setup, what the instrument
// was oriented on, and everything shot from there.
//
// The setup is a `Station`, not a repetition of its fields. A second struct
// carrying an id, an occupied point and an instrument height would be a second
// way of saying the same thing, and the two would drift apart the first time one
// of them gained a field.
struct SurveyStation {
    Station setup;
    // The point the instrument was oriented on. Empty when the file records no
    // backsight - a free setup, or a resection stated only by its result.
    std::string backsightPointId;
    // The horizontal circle reading set on the backsight, radians clockwise in
    // [0, 2*pi). Absent when the file does not say, and absent is not zero: a
    // setup oriented on 0 00 00 and a setup whose orientation was not recorded
    // produce different bearings, so an importer must not invent one.
    std::optional<double> backsightAzimuth;
    // In the order the file gives them. The backsight's own observation, where
    // the file records one, is one of these like any other.
    std::vector<Observation> observations;
    std::map<std::string, std::string> metadata;
    SourceRecord source;

    friend bool operator==(const SurveyStation&, const SurveyStation&) = default;
};

// A coded string: the points one field code strung together, in the order they
// were observed. That is what a kerb line, a fence or a pipe run IS in raw field
// data. The geometry is made from it later by the survey coding rules, not here.
struct SurveyFeature {
    std::string name; // string name where the file gives one; may be empty
    std::string code; // the field code, e.g. "KB", "FL"
    std::vector<std::string> pointIds; // in observation order
    bool closed = false;
    std::string description;
    std::map<std::string, std::string> metadata;
    SourceRecord source;

    friend bool operator==(const SurveyFeature&, const SurveyFeature&) = default;
};

// The coordinate system a source file DECLARES, and nothing else.
//
// Deliberately not a geodesy type and deliberately not a transformation. What a
// field file states is a name, sometimes an EPSG code, and very often nothing at
// all. Turning that into a transformation needs BOTH ends of it, and the far end
// is the drawing the data is being imported into, which an importer does not
// know. Transforming with one end guessed silently moves a survey - the failure
// this type exists to make impossible. The declaration is recorded, carried, and
// put in front of a person who can decide.
//
// Built through the factories so that `unknown` cannot drift away from the fields
// beside it.
struct DeclaredCoordinateSystem {
    std::string name; // exactly as the file spells it
    int epsgCode = 0; // 0 when the file gave none
    bool unknown = true;

    [[nodiscard]] static DeclaredCoordinateSystem named(std::string name);
    [[nodiscard]] static DeclaredCoordinateSystem epsg(int code, std::string name = {});

    friend bool operator==(const DeclaredCoordinateSystem&,
                           const DeclaredCoordinateSystem&) = default;
};

// The units a file declares ITS OWN numbers in.
//
// The model is always metres and radians (see the conventions at the top of this
// file): an importer converts on the way in, and this records what it converted
// FROM, so a stored value can be checked against the figure printed in the file.
// An importer that meets a unit it cannot convert must fail. It must never store
// the raw number and note the unit here, because everything downstream reads
// metres and would not look.
enum class LinearUnit { Unknown, Metres, Feet, UsSurveyFeet, Links };

// DegreesMinutesSeconds is a NOTATION whose unit is degrees, but instruments and
// LandXML declare it as though it were a unit, so it is recorded as they state it.
enum class AngularUnit { Unknown, Radians, DecimalDegrees, DegreesMinutesSeconds, Gons, Mils };

[[nodiscard]] const char* toString(LinearUnit unit);
[[nodiscard]] const char* toString(AngularUnit unit);

struct DeclaredUnits {
    LinearUnit linear = LinearUnit::Unknown;
    AngularUnit angular = AngularUnit::Unknown;

    friend bool operator==(const DeclaredUnits&, const DeclaredUnits&) = default;
};

// Everything one import produced, as the source stated it.
//
// A plain aggregate rather than a class with invariants: an importer fills it
// record by record, and a file that fails half way through is still worth
// reporting on. What SurveyNetwork enforces at every insert, this checks once at
// the end, in validateProject().
struct SurveyProject {
    std::string name;
    DeclaredCoordinateSystem coordinateSystem;
    DeclaredUnits units;
    std::vector<SurveyPoint> points;
    std::vector<SurveyStation> stations;
    // Observations belonging to no one setup: levelling runs, GNSS vectors.
    // An observation taken FROM a setup lives on that SurveyStation instead, so
    // that it is not recorded twice.
    std::vector<Observation> observations;
    std::vector<Traverse> traverses;
    std::vector<SurveyFeature> features;
    std::map<std::string, std::string> metadata;
    SourceRecord source; // the file as a whole; its recordNumber is 0

    friend bool operator==(const SurveyProject&, const SurveyProject&) = default;
};

// Checks one project's internal consistency, so that importers do not each invent
// their own check and disagree about what a valid project is:
//   * point ids are non-empty and unique, and coordinates are finite;
//   * station ids are non-empty and unique, and every occupied and backsighted
//     point is in the project;
//   * every feature names at least one point and every point it names is in the
//     project;
//   * every observation, on a station or loose, passes validateObservation() and
//     refers only to points that are in the project.
// Traverses are NOT checked against the points: a Traverse is self-contained by
// design (see above - it carries its own start and end coordinates), so its
// station ids are names of its own and need not be project points.
// InvalidArgument / AlreadyExists / NotFound / InvalidSurveyObservation, with the
// offender named in the error context.
[[nodiscard]] katana::core::Status validateProject(const SurveyProject& project);

} // namespace katana::survey
