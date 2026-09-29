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
#include "katana/math/unit_ratio.hpp"
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
    // ABSENT IS NOT ZERO. A coordinate list with no height column, a GSI block
    // with no word 83, a LandXML CgPoint of two ordinates: each is a point whose
    // height nobody measured, and 0.0 in its place is a real height at the
    // datum. A surface built from it is pulled down to zero under every such
    // point with nothing to say why. So the model carries "no height" as a
    // value of its own and every consumer has to decide what it means for them:
    // the drawing leaves the height property off, a level adjustment refuses a
    // benchmark that has none (PLAN.MD 45.3b is the record of how this was
    // found).
    std::optional<double> elevation;
    std::string code;        // feature code, e.g. "IP", "EOP"
    std::string description; // free text
    std::map<std::string, std::string> metadata;
    CoordinateSource coordinateSource = CoordinateSource::Unknown;
    SourceRecord source;

    [[nodiscard]] Coordinate2 position() const { return Coordinate2{northing, easting}; }

    friend bool operator==(const SurveyPoint&, const SurveyPoint&) = default;
};

// A point a source NAMES and gives no coordinates for: the target of a raw
// observation before anything has reduced it, a backsight known only by its
// number, a point a traverse record refers to and never lists.
//
// A type of its own, and deliberately NOT a SurveyPoint with placeholder
// ordinates. The first round of instrument parsers each needed this and each
// improvised: one created the point at the origin with a marker in its
// metadata, another at placeholder ordinates. Either reaches the drawing as a
// real mark at (0, 0) the moment one consumer forgets to look for the marker -
// and nothing about a point at the origin says it was never measured. Keeping
// such a point OUT of SurveyProject::points means "every SurveyPoint has a
// position" stays true by construction; the bridge cannot draw one because it
// is not in the list it draws. The rejected alternative, an optional northing
// and easting, would have let the two disagree and made every horizontal
// consumer - the network, the COGO, the bridge - check for a case most of them
// can never meet. Reducing observations to give these a position is the job of
// a reduction step, which produces SurveyPoints with CoordinateSource::Calculated.
struct UnpositionedPoint {
    std::string id; // shares SurveyPoint's id space: unique across both lists
    std::string code;
    std::string description;
    std::map<std::string, std::string> metadata;
    SourceRecord source;

    friend bool operator==(const UnpositionedPoint&, const UnpositionedPoint&) = default;
};

// ---- What the instrument did, and when ---------------------------------------------

// Whether a correction is already in a recorded value.
//
// Three states and not a bool, because "the file does not say" is the common
// case and it is not the same as "not applied": an atmospheric correction
// added a second time is as wrong as one left out, and the reduction has to
// be able to tell the person which of the two it guessed (reduction.hpp).
enum class CorrectionState {
    Unknown,    // the file does not say
    Applied,    // the instrument or controller already put it in the recorded value
    NotApplied, // the file states the value is raw in this respect
};

[[nodiscard]] const char* toString(CorrectionState state);

// A calendar date and time exactly as a field file states it, with the time
// system it names. Not a std::chrono time point: a field file's clock is
// often local and unlabelled, and converting it would invent a time zone the
// file never gave. `year == 0` means "not stated".
struct SurveyTimestamp {
    int year = 0;
    int month = 0; // 1..12
    int day = 0;   // 1..31
    int hour = 0;
    int minute = 0;
    double second = 0.0;
    std::string timeSystem{}; // "UTC", "GPS", "local"; empty when the file does not say

    [[nodiscard]] bool known() const { return year != 0; }

    friend bool operator==(const SurveyTimestamp&, const SurveyTimestamp&) = default;
};

// "2024-03-05T10:15:30.5 GPS", or "" for a timestamp that is not known. The
// seconds are written with as many decimals as they need and no more.
[[nodiscard]] std::string toString(const SurveyTimestamp& timestamp);

// The settings a total station recorded for one setup: which instrument, and
// what it had already done to the numbers it wrote down.
//
// Typed rather than left in SurveyStation::metadata, because the reduction
// DECIDES from these (reduction.hpp): an atmospheric correction is applied
// automatically only when the instrument did not apply one, and a prism
// constant only when the shot says it is missing. A key/value bag would make
// every parser spell those facts its own way and the reduction guess.
struct InstrumentSettings {
    std::string make{};  // "Leica"
    std::string model{}; // "TS16"
    std::string serialNumber{};
    // Metres, as set on the instrument (e.g. -0.0344 for a Leica round prism
    // against a zero-constant instrument, 0.0 for a Leica GPR1 on a Leica).
    std::optional<double> prismConstant{};
    CorrectionState prismConstantState = CorrectionState::Unknown;
    // Parts per million, as the instrument computed or was told it.
    std::optional<double> atmosphericPpm{};
    CorrectionState atmosphericPpmState = CorrectionState::Unknown;
    std::optional<double> temperatureCelsius{};
    std::optional<double> pressureHectopascals{};
    std::optional<double> relativeHumidityPercent{};
    // The coefficient of refraction the instrument used for its own reduced
    // values (a recorded horizontal distance or height difference).
    std::optional<double> refractionCoefficient{};
    CorrectionState curvatureRefractionState = CorrectionState::Unknown;
    // A grid or project scale factor the instrument or controller applied to
    // the distances it recorded.
    std::optional<double> scaleFactor{};
    CorrectionState scaleFactorState = CorrectionState::Unknown;
    SurveyTimestamp time{};

    friend bool operator==(const InstrumentSettings&, const InstrumentSettings&) = default;
};

// The a-priori precision of observations, used twice: a parser gives an
// observation whose file states no standard deviation these values (every
// observation must carry a sigma > 0, see validateObservation), and the
// reduction replaces them with the ones the person set (reduction_settings.hpp).
// Defaults are those of a 3" total station with a 2 mm + 2 ppm EDM, which is a
// middling instrument: a better one is under-weighted, which is the safe way
// round for an adjustment's outlier test.
struct ObservationPrecision {
    double direction = 1.4544410433286079e-05;  // radians: 3" = 3 * pi / 648000
    double zenith = 1.4544410433286079e-05;     // radians: 3"
    double distanceConstant = 0.002;            // metres (the "a" of a mm + b ppm)
    double distancePpm = 2.0;                   // parts per million (the "b")
    double instrumentCentring = 0.001;          // metres
    double targetCentring = 0.001;              // metres
    double heightMeasurement = 0.002;           // metres, instrument and target heights
    double levellingPerSqrtKilometre = 0.001;   // metres per sqrt(km) of level line
    double gnssHorizontal = 0.010;              // metres, where a file gives no covariance
    double gnssVertical = 0.020;                // metres, where a file gives no covariance

    friend bool operator==(const ObservationPrecision&, const ObservationPrecision&) = default;
};

// sqrt(a^2 + (b * 1e-6 * d)^2): the EDM part only; centring is the reduction's
// business because it depends on the geometry, not on the distance alone.
[[nodiscard]] double distanceSigma(const ObservationPrecision& precision, double distance);

// ---- Observations ------------------------------------------------------------

// Every observation carries the record it was read from, for the reasons given
// beside SourceRecord. It is part of the observation rather than a table beside
// it so that an observation moved between containers cannot lose its origin.
//
// Fields added after `source` (pointing, target and the like) carry default
// member initialisers, so that an aggregate written before they existed still
// compiles and means what it meant.

// Which face of the telescope an observation was made on. Face I / face II in
// the older books; Left is "vertical circle on the left of the observer".
enum class Face { Unknown, Left, Right };

[[nodiscard]] const char* toString(Face face);

// Which POINTING an observation belongs to: the horizontal direction, zenith
// angle and slope distance of one shot share an index, so that the reduction
// can pair a face-left shot with its face-right partner and reduce the three
// together (a slope distance needs the zenith angle of the same pointing).
// 0 means "the file does not group this observation with any other". Indices
// are unique within one SurveyStation and carry no meaning beyond equality.
struct Pointing {
    std::size_t index = 0;
    Face face = Face::Unknown;

    friend bool operator==(const Pointing&, const Pointing&) = default;
};

// What a distance was measured to.
struct TargetInfo {
    // Metres, as recorded with the shot (which may differ from the setup's
    // InstrumentSettings::prismConstant when the prism was changed mid setup).
    std::optional<double> prismConstant{};
    CorrectionState prismConstantState = CorrectionState::Unknown;
    std::string targetType{}; // the file's own words: "prism", "reflectorless", "tape"

    friend bool operator==(const TargetInfo&, const TargetInfo&) = default;
};

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
    Pointing pointing{};
    TargetInfo target{};

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
    Pointing pointing{};

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
    Pointing pointing{};

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
    Pointing pointing{};

    friend bool operator==(const ZenithAngleObservation&,
                           const ZenithAngleObservation&) = default;
};

// A horizontal circle READING at `at` towards `to`, radians clockwise in
// [0, 2*pi): what a total station actually records, before anything has
// oriented it.
//
// Its own kind, rather than a HorizontalAngleObservation from the backsight,
// because a parser does not know the backsight reading of the same face when
// it meets a shot (the backsight may be observed last, or on both faces), and
// differencing early would throw away the face pairing the reduction needs.
// The reduction turns directions into angles or azimuths; the network
// adjustment does not take them (it lists them as unused).
struct HorizontalDirectionObservation {
    std::string at;
    std::string to;
    double direction = 0.0;
    double sigma = 0.0; // radians
    SourceRecord source;
    Pointing pointing{};

    friend bool operator==(const HorizontalDirectionObservation&,
                           const HorizontalDirectionObservation&) = default;
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

// ---- GNSS as the receiver states it --------------------------------------------
//
// GnssBaselineObservation and GnssPositionObservation above are GRID values,
// already reduced into the network's system. What a receiver or a Trimble job
// actually records is earth-centred: ECEF coordinates, ECEF vectors, or
// latitude, longitude and ellipsoidal height. Turning those into grid needs a
// projection, which is geodesy, which katana::survey may not see; so a parser
// records them as they are and the reduction converts them through a function
// the caller supplies (ReductionContext in reduction.hpp).

// Earth-centred, earth-fixed, metres.
struct GeocentricCoordinate {
    double x = 0.0;
    double y = 0.0;
    double z = 0.0;

    friend bool operator==(const GeocentricCoordinate&, const GeocentricCoordinate&) = default;
};

// Latitude and longitude in radians (north and east positive), height above
// the ellipsoid in metres.
struct GeodeticCoordinate {
    double latitude = 0.0;
    double longitude = 0.0;
    double ellipsoidalHeight = 0.0;

    friend bool operator==(const GeodeticCoordinate&, const GeodeticCoordinate&) = default;
};

// A symmetric 3x3 covariance, metres squared, in the frame of the value it
// belongs to: X/Y/Z for a geocentric value; north/east/up (local, at the
// point) for a geodetic one. All zero means the file gave none; otherwise the
// diagonal must be positive.
struct GnssCovariance3 {
    double xx = 0.0;
    double yy = 0.0;
    double zz = 0.0;
    double xy = 0.0;
    double xz = 0.0;
    double yz = 0.0;

    [[nodiscard]] bool stated() const
    {
        return xx != 0.0 || yy != 0.0 || zz != 0.0 || xy != 0.0 || xz != 0.0 || yz != 0.0;
    }

    friend bool operator==(const GnssCovariance3&, const GnssCovariance3&) = default;
};

// How an antenna height was measured. The reduction to the antenna reference
// point depends on it, and a slant height reduced as a vertical one is a
// centimetre-level error nobody sees.
enum class AntennaHeightMethod {
    Unknown,
    Vertical,    // vertically to the antenna reference point (ARP)
    Slant,       // to the edge of the antenna; needs the antenna's radius
    PhaseCentre, // already reduced to the phase centre by the receiver
    Other,       // the file names a mark this list does not: see GnssAntenna::measuredTo
};

[[nodiscard]] const char* toString(AntennaHeightMethod method);

struct GnssAntenna {
    std::string type{}; // as the file gives it, e.g. the IGS name "LEIGS15     NONE"
    std::string serialNumber{};
    double height = 0.0; // metres, as measured (RINEX "ANTENNA: DELTA H/E/N" H)
    AntennaHeightMethod method = AntennaHeightMethod::Unknown;
    std::string measuredTo{}; // the file's own words: "bottom of quick release"
    double eastOffset = 0.0;  // metres (RINEX DELTA E)
    double northOffset = 0.0; // metres (RINEX DELTA N)

    friend bool operator==(const GnssAntenna&, const GnssAntenna&) = default;
};

// The kind of solution a receiver reports. Carried so that a report can say
// "3 of these positions are float solutions" instead of weighting them all
// alike without comment.
enum class GnssSolution { Unknown, Fixed, Float, Differential, Autonomous };

[[nodiscard]] const char* toString(GnssSolution solution);

// A GNSS position of `point` in a global frame, as the file states it.
// Exactly one of `geocentric` and `geodetic` is set (validateObservation).
struct GnssGlobalPositionObservation {
    std::string point;
    std::optional<GeocentricCoordinate> geocentric{};
    std::optional<GeodeticCoordinate> geodetic{};
    GnssCovariance3 covariance{}; // see GnssCovariance3 for its frame
    std::string referenceFrame{}; // as declared: "WGS 84", "ITRF2014"; empty when not
    GnssAntenna antenna{};
    GnssSolution solution = GnssSolution::Unknown;
    SourceRecord source;

    friend bool operator==(const GnssGlobalPositionObservation&,
                           const GnssGlobalPositionObservation&) = default;
};

// A GNSS vector `from` -> `to` in ECEF components, metres.
struct GnssGeocentricBaselineObservation {
    std::string from;
    std::string to;
    GeocentricCoordinate delta{};
    GnssCovariance3 covariance{}; // X/Y/Z
    std::string referenceFrame{};
    GnssAntenna fromAntenna{};
    GnssAntenna toAntenna{};
    GnssSolution solution = GnssSolution::Unknown;
    SourceRecord source;

    friend bool operator==(const GnssGeocentricBaselineObservation&,
                           const GnssGeocentricBaselineObservation&) = default;
};

// New kinds are APPENDED, so that the index of every older alternative stays
// what it was.
using Observation =
    std::variant<DistanceObservation, HorizontalAngleObservation, VerticalAngleObservation,
                 ZenithAngleObservation, AzimuthObservation, GnssBaselineObservation,
                 GnssPositionObservation, LevelDifferenceObservation,
                 HorizontalDirectionObservation, GnssGlobalPositionObservation,
                 GnssGeocentricBaselineObservation>;

// The pointing an observation belongs to, or nullptr for a kind that has none
// (azimuths, GNSS, level differences).
[[nodiscard]] const Pointing* observationPointing(const Observation& observation);

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
    // produce different bearings, so an importer must not invent one. With no
    // backsight named, the reduction takes a setup that has one as a circle
    // set to read azimuths - its readings are its bearings - whatever the
    // value. (The name is older than `statedBacksightAzimuth`: this is the
    // circle, never the azimuth.)
    std::optional<double> backsightAzimuth;
    // The azimuth of the line to the backsight as the FILE states it - keyed
    // in, or worked out by the field software - radians clockwise from north
    // in [0, 2*pi). Absent when the file states none. Where the backsight's
    // coordinates orient the setup this is not used; where nothing places the
    // backsight, the reduction orients the setup on it less the setup's own
    // reading on the backsight, or with no reading less the circle above -
    // the field software's own orientation correction. Not the circle: a
    // circle set to 0 00 00 on a backsight 123 degrees away is the usual
    // case, and taking either for the other turns every bearing by the
    // difference.
    std::optional<double> statedBacksightAzimuth;
    // In the order the file gives them. The backsight's own observation, where
    // the file records one, is one of these like any other.
    std::vector<Observation> observations;
    std::map<std::string, std::string> metadata;
    SourceRecord source;
    // What the instrument was and what it had already done to the numbers.
    InstrumentSettings instrument{};

    friend bool operator==(const SurveyStation&, const SurveyStation&) = default;
};

// One GNSS observing session, as a RINEX observation header (or a receiver's
// own job) describes it. Metadata, not observations: raw phase and code are not
// read into the model - a RINEX file is imported for what it says about the
// occupation (marker, antenna, span), and processing it is out of scope.
struct GnssSession {
    std::string markerName{};
    std::string markerNumber{};
    std::string receiverType{};
    std::string receiverSerial{};
    std::string receiverFirmware{};
    GnssAntenna antenna{};
    // The header's approximate position, ECEF metres. Approximate is the word:
    // it is typically a navigation solution, metres out.
    std::optional<GeocentricCoordinate> approximatePosition{};
    SurveyTimestamp firstEpoch{};
    SurveyTimestamp lastEpoch{};
    std::optional<double> intervalSeconds{};
    std::size_t epochCount = 0;
    // Distinct satellites seen, per system, keyed by the system's name
    // ("GPS", "GLONASS", "Galileo", "BeiDou", "QZSS", "NavIC", "SBAS").
    std::map<std::string, std::size_t> satellitesPerSystem{};
    std::string formatVersion{}; // "3.04"
    std::string observer{};
    std::string agency{};
    SourceRecord source;

    friend bool operator==(const GnssSession&, const GnssSession&) = default;
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

// Metres in one `unit`, as the exact ratio katana/math/unit_ratio.hpp defines:
// Feet is the international foot (EPSG:9002), UsSurveyFeet the US survey foot
// (EPSG:9003), Links the international link of 0.201168 m (EPSG:9098).
// InvalidArgument for Unknown - there is no default unit, because a factor of 1
// silently applied to a file in feet puts a survey 3.28 times too far from the
// origin and every mark on it looks fine.
[[nodiscard]] katana::core::Result<katana::math::UnitRatio> metresPer(LinearUnit unit);

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
    // Named by the source with no coordinates - see UnpositionedPoint. An
    // observation, station or feature may refer to one of these exactly as it
    // may to a point; nothing may draw one.
    std::vector<UnpositionedPoint> unpositionedPoints;
    std::vector<SurveyStation> stations;
    // Observations belonging to no one setup: levelling runs, GNSS vectors.
    // An observation taken FROM a setup lives on that SurveyStation instead, so
    // that it is not recorded twice.
    std::vector<Observation> observations;
    std::vector<Traverse> traverses;
    std::vector<SurveyFeature> features;
    std::map<std::string, std::string> metadata;
    SourceRecord source; // the file as a whole; its recordNumber is 0
    // Points the FILE marks as control (a Leica fixed point, a Trimble
    // "control" class). The published values are the points' own coordinates.
    // What the reduction actually holds is the person's choice in
    // ReductionSettings, which starts from these.
    std::vector<ControlPoint> controlPoints{};
    // GNSS sessions the file describes (RINEX headers, receiver jobs).
    std::vector<GnssSession> gnssSessions{};

    friend bool operator==(const SurveyProject&, const SurveyProject&) = default;
};

// Checks one project's internal consistency, so that importers do not each invent
// their own check and disagree about what a valid project is:
//   * point ids are non-empty and unique across `points` and
//     `unpositionedPoints` together, and coordinates are finite (an absent
//     elevation is valid; a present one must be finite);
//   * every reference below resolves to a point in EITHER list;
//   * station ids are non-empty and unique, and every occupied and backsighted
//     point is in the project;
//   * every feature names at least one point and every point it names is in the
//     project;
//   * every observation, on a station or loose, passes validateObservation() and
//     refers only to points that are in the project;
//   * every control point names a POSITIONED point (control with no published
//     value is a contradiction), and a Weighted component has a sigma > 0.
// Traverses are NOT checked against the points: a Traverse is self-contained by
// design (see above - it carries its own start and end coordinates), so its
// station ids are names of its own and need not be project points.
// InvalidArgument / AlreadyExists / NotFound / InvalidSurveyObservation, with the
// offender named in the error context.
[[nodiscard]] katana::core::Status validateProject(const SurveyProject& project);

} // namespace katana::survey
