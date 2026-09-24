#pragma once

// How raw field observations are reduced and adjusted: every choice a person
// can make about it, the defaults they start from, and a stable text form.
//
// A type of its own, apart from reduction.hpp, because three things hold one:
// a survey job stores the settings it was last adjusted with (so the person can
// go back and change them), the reduction report prints the settings it used,
// and the reduction reads them. The report cannot include reduction.hpp, which
// includes the report.
//
// Units follow katana::survey: metres and radians throughout, including the
// tolerances and the a-priori standard deviations. The text form stores the
// same numbers, so it round-trips bit for bit; a person never reads it - the
// wizard shows seconds of arc and millimetres.

#include <cstddef>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/survey/data_model.hpp"

namespace katana::survey {

// ---- Constants a person may override --------------------------------------------

// The coefficient of refraction. 0.13 is Gauss's value for daytime sights
// over land, the one every total station defaults to and the one the owner's
// own survey engine uses.
inline constexpr double kDefaultRefractionCoefficient = 0.13;

// Metres. The mean radius of the earth, for curvature and refraction and for
// the height reduction factor R / (R + h). Using a mean rather than the radius
// of curvature at the site changes a 1 km sight's curvature term by well under
// a millimetre, which is below what the other terms know.
inline constexpr double kMeanEarthRadius = 6'371'000.0;

// ---- Basic: which corrections ---------------------------------------------------

enum class AtmosphericCorrection {
    // Never add one.
    None,
    // Compute it from the setup's recorded temperature, pressure and humidity
    // and apply it ONLY where the instrument states it did not apply one
    // itself. Where the file does not say, nothing is applied and the report
    // says so: applying it twice is as wrong as leaving it out, and a guess
    // either way has to be visible.
    Auto,
    // Take out whatever the instrument applied and apply the value computed
    // from the recorded weather instead.
    Recompute,
    // Apply ReductionSettings::fixedPpm to every distance not already
    // corrected by the instrument.
    Fixed,
};

enum class PrismConstantPolicy {
    // Add the recorded constant only where the shot or setup says the
    // instrument did not add it.
    Auto,
    // Take out any constant the instrument applied and apply
    // ReductionSettings::prismConstant to every prism distance instead.
    Override,
    // Distances as recorded.
    None,
};

// What face left and face right mean for the reduction.
enum class FaceHandling {
    // The mean of each face-left / face-right pair of one target at one setup
    // is the observation; a pointing on one face only is used as it is. The
    // pair's spread is checked against FaceTolerances and reported.
    Average,
    // Face-left only; face-right pointings are reported as unused, not dropped
    // silently.
    FaceLeftOnly,
    // Every pointing is an observation of its own (an adjustment then sees the
    // two faces as two measurements).
    Separate,
};

// The largest spread between the two faces of one pair that passes the check.
// Horizontal: |FL - (FR - pi)|, wrapped. Zenith: |FL + FR - 2 pi|, which is
// twice the index error. Distance: |FL - FR|. Defaults suit a 3" instrument.
struct FaceTolerances {
    double horizontal = 4.84813681109536e-05; // radians: 10" = 10 * pi / 648000
    double zenith = 9.69627362219072e-05;     // radians: 20"
    double distance = 0.005;                  // metres

    friend bool operator==(const FaceTolerances&, const FaceTolerances&) = default;
};

enum class HeightReduction {
    None,
    // Distances to the ellipsoid: factor R / (R + h) with h = H + N, the
    // geoid separation N coming from ReductionContext::geoidSeparation.
    Ellipsoid,
    // Distances to the geoid ("sea level"): factor R / (R + H).
    Geoid,
};

enum class GridScale {
    None,           // ground distances
    Fixed,          // ReductionSettings::fixedGridScaleFactor, everywhere
    FromProjection, // ReductionContext::gridScaleFactor at each line's mid point
};

// ---- Adjustment ------------------------------------------------------------------

enum class AdjustmentMethod {
    // Radiation from the setups: every point computed from the setup it was
    // shot from, nothing adjusted. Misclosures are still reported where the
    // data closes.
    None,
    // The setups form a traverse; see TraverseRule.
    Traverse,
    // Least squares over every observation; see NetworkDimension.
    Network,
};

enum class TraverseRule {
    Bowditch,     // compass rule: corrections in proportion to leg length
    Transit,      // in proportion to |latitude| and |departure|
    LeastSquares, // the traverse as a network (survey::adjustTraverse)
};

// There is no 3D option: katana::survey adjusts horizontal networks and level
// networks (network_adjustment.hpp) and has no combined 3D model. Horizontal
// AND levels runs the two, one after the other, which is what most field
// software does anyway.
enum class NetworkDimension {
    Horizontal,
    Levels,
    HorizontalAndLevels,
};

enum class OutlierTest {
    None,
    Baarda, // data snooping: w = v / sigma_v against the normal quantile
    Tau,    // Pope's tau test: the a-posteriori version, for an unknown variance factor
};

// Where the coordinates a control point is held at come from.
enum class ControlOrigin {
    File,    // the point's coordinates in the imported file
    Drawing, // a point already on the drawing (ReductionContext::drawingPoints)
};

// One control point as the person chose it: which components are held and
// how (survey::ControlPoint), and whose coordinates.
struct ControlSelection {
    ControlPoint point;
    ControlOrigin origin = ControlOrigin::File;

    friend bool operator==(const ControlSelection&, const ControlSelection&) = default;
};

// ---- The settings ------------------------------------------------------------------

// Every member has the default a new import starts with. The defaults reduce a
// total-station file the way its instrument would have, and adjust nothing:
// adjusting is a decision that needs control, and control is the person's to
// choose.
struct ReductionSettings {
    // Basic
    AtmosphericCorrection atmospheric = AtmosphericCorrection::Auto;
    double fixedPpm = 0.0; // parts per million; used by AtmosphericCorrection::Fixed
    PrismConstantPolicy prismConstantPolicy = PrismConstantPolicy::Auto;
    double prismConstant = 0.0; // metres; used by PrismConstantPolicy::Override
    FaceHandling faces = FaceHandling::Average;
    FaceTolerances faceTolerances{};
    bool curvatureAndRefraction = true;
    double refractionCoefficient = kDefaultRefractionCoefficient;
    double earthRadius = kMeanEarthRadius; // metres
    bool slopeToHorizontal = true;
    HeightReduction heightReduction = HeightReduction::None;
    GridScale gridScale = GridScale::None;
    double fixedGridScaleFactor = 1.0;
    // When on, `combinedFactor` is applied to every horizontal distance IN
    // PLACE OF the height and grid factors - the surveyor's "combined scale
    // factor" off a control certificate.
    bool useCombinedFactor = false;
    double combinedFactor = 1.0;

    // Adjustment
    AdjustmentMethod method = AdjustmentMethod::None;
    TraverseRule traverseRule = TraverseRule::Bowditch;
    NetworkDimension networkDimension = NetworkDimension::Horizontal;
    // A-priori standard deviations. Total-station observations are always
    // weighted from these; a GNSS value keeps the covariance its file states
    // when `useFileCovariances` is on and falls back to these otherwise.
    ObservationPrecision apriori{};
    bool useFileCovariances = true;
    std::vector<ControlSelection> control{};
    // Of the error ellipses and the global test (whose significance level is
    // 1 - confidenceLevel).
    double confidenceLevel = 0.95;
    OutlierTest outlierTest = OutlierTest::Baarda;
    // Per observation. 0.001 is Baarda's alpha0, critical value 3.29.
    double outlierSignificance = 0.001;
    // Off by default: throwing an observation away is the person's decision.
    // On, the worst flagged observation is rejected and the adjustment rerun,
    // one at a time, and every rejection is listed in the report.
    bool autoRejectOutliers = false;
    std::size_t maxIterations = 25; // Gauss-Newton iterations

    friend bool operator==(const ReductionSettings&, const ReductionSettings&) = default;
};

// Checks the settings on their own: finite numbers, positive factors, radii,
// sigmas and tolerances; a refraction coefficient in [-1, 1]; a confidence
// level and outlier significance in (0, 1); at least one iteration; control
// point ids non-empty and unique, and every Weighted component with a
// sigma > 0. InvalidArgument naming the setting, in words a surveyor uses.
[[nodiscard]] katana::core::Status validateReductionSettings(const ReductionSettings& settings);

// The control a file declares, as a starting selection: every
// SurveyProject::controlPoints entry held as the file holds it, with its
// coordinates from the file.
[[nodiscard]] std::vector<ControlSelection> controlFromFile(const SurveyProject& project);

// ---- Stable text form -------------------------------------------------------------
//
// key=value lines, one setting per line, the first line naming the format and
// its version:
//
//     katana-reduction-settings=1
//     atmospheric=auto
//     refraction.k=0.13
//     control=CP1;file;fixed;0;fixed;0;free;0
//     ...
//
// Doubles are written as the shortest text that reads back as the same
// double, so a save and a load change nothing. `control` repeats, once per
// control point: the id (percent-encoded, since an id is user data and may
// hold ';'), the origin, then constraint;sigma for northing, easting and
// elevation. Lines starting '#' and blank lines are ignored.
//
// Versioning: kReductionSettingsVersion is bumped when a key changes meaning.
// A new key does NOT need a bump - a key missing from older text keeps its
// default. Text from a NEWER version is refused, never half read.
inline constexpr int kReductionSettingsVersion = 1;

[[nodiscard]] std::string serialiseReductionSettings(const ReductionSettings& settings);

// ParseFailure naming the line for a malformed line or value; Unsupported for
// text written by a newer version; InvalidArgument when the settings read fail
// validateReductionSettings. An unknown key is not an error (a later build may
// have added it): it is skipped and, when `warnings` is given, reported there.
[[nodiscard]] katana::core::Result<ReductionSettings>
parseReductionSettings(std::string_view text, std::vector<std::string>* warnings = nullptr);

// The names the text form uses, also what the report prints.
[[nodiscard]] const char* toString(AtmosphericCorrection value);
[[nodiscard]] const char* toString(PrismConstantPolicy value);
[[nodiscard]] const char* toString(FaceHandling value);
[[nodiscard]] const char* toString(HeightReduction value);
[[nodiscard]] const char* toString(GridScale value);
[[nodiscard]] const char* toString(AdjustmentMethod value);
[[nodiscard]] const char* toString(TraverseRule value);
[[nodiscard]] const char* toString(NetworkDimension value);
[[nodiscard]] const char* toString(OutlierTest value);
[[nodiscard]] const char* toString(ControlOrigin value);

} // namespace katana::survey
