#pragma once

// Subsurface utility quality levels, after AS 5488.1-2019 "Classification of
// subsurface utility information (SUI)". docs/subsurface_utilities.md is the
// record of how the standard was read and where Katana is deliberately
// stricter than it.
//
// The standard grades every piece of information about a buried service by
// HOW it was obtained, not by how good it looks:
//
//   QL-D  from existing records or anecdote; no position is measured.
//   QL-C  a surveyed surface feature (a pit, a valve, a marker post) correlated
//         with the records; the service itself is still inferred.
//   QL-B  the service is detected from the surface - electromagnetic location,
//         ground penetrating radar and the like - and its position surveyed,
//         within a stated horizontal and (where a depth is given) vertical
//         tolerance.
//   QL-A  the service is exposed and seen - potholing, non-destructive
//         excavation, an open trench - and surveyed in three dimensions
//         within a tight absolute tolerance.
//
// The ordering is the point of the enumeration: a worse level compares LESS
// than a better one, so "the weakest of two" is std::min and a claim is
// supported when claimed <= attainable.
//
// Tolerances are carried as data rather than constants. The defaults are the
// published figures (QL-A +/-50 mm horizontal and vertical, QL-B +/-300 mm
// horizontal and +/-500 mm vertical), but a client specification may be
// tighter, and a future edition may differ; a program that hard-codes them
// would silently certify against the wrong numbers.

#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace katana::survey::subsurface {

enum class QualityLevel {
    D = 0, // records / anecdotal
    C = 1, // surface feature correlation
    B = 2, // detected and surveyed
    A = 3, // exposed and surveyed in 3D
};

// "QL-A" .. "QL-D": the spelling the standard and every deliverable uses.
[[nodiscard]] const char* toString(QualityLevel level);

// Accepts "QL-A", "QLA", "QL A", "A" and "Quality Level A" in any letter
// case, with blanks around. nullopt for anything else, including "E",
// "Unknown" and an empty string: an unknown quality level is not QL-D, it is
// a record nobody graded.
[[nodiscard]] std::optional<QualityLevel> parseQualityLevel(std::string_view text);

// How a position was obtained. The quality level a record may claim is capped
// by its method before any tolerance is looked at: no accuracy makes a
// radar pick into a QL-A observation, because QL-A means the service was SEEN.
enum class LocationMethod {
    Unknown,                  // the method was not recorded: nothing supports better than QL-D
    Records,                  // plans, GIS, as-constructed drawings
    Anecdotal,                // verbal, site knowledge
    SurfaceFeature,           // a pit, valve, marker or pole surveyed and correlated
    ElectromagneticLocation,  // active or passive EML, sonde in a duct
    GroundPenetratingRadar,   // GPR
    OtherGeophysical,         // acoustic, magnetometry, seismic ...
    NonDestructiveExcavation, // vacuum or hydro excavation: a pothole
    OpenExcavation,           // an open trench, the service in view
};

[[nodiscard]] const char* toString(LocationMethod method);

// Accepts the enumerator names, the field abbreviations surveyors use -
// "records", "anecdotal", "surface", "EML", "GPR", "geophysical", "NDD",
// "pothole", "vac", "trench" - and the Locate Method names of AS 5488.2 as
// delivery schemas spell them: "Archive Drawings and Plans" and "Geographic
// Information System" are Records, "Electronic Detection" is EML,
// "Potholing" is non-destructive excavation, "Survey" (a surveyed feature)
// is SurfaceFeature. Any letter case; nullopt otherwise.
[[nodiscard]] std::optional<LocationMethod> parseLocationMethod(std::string_view text);

// The best level the method can ever support.
[[nodiscard]] QualityLevel maximumQualityLevel(LocationMethod method);

// Positional tolerance of one quality level, in metres, as a +/- figure.
// nullopt means the level states none: QL-C and QL-D positions are not
// measurements of the service, so there is nothing to test them against.
struct Tolerance {
    std::optional<double> horizontal;
    std::optional<double> vertical;
};

struct QualityLevelTolerances {
    Tolerance a{0.050, 0.050};
    Tolerance b{0.300, 0.500};
    Tolerance c{};
    Tolerance d{};

    [[nodiscard]] const Tolerance& of(QualityLevel level) const;
};

// What a position's own uncertainty supports. Uncertainties are +/- figures
// in metres at the confidence the tolerances are stated at; the caller is
// responsible for bringing an instrument's 1-sigma figure to that confidence.
struct PositionEvidence {
    LocationMethod method = LocationMethod::Records;
    std::optional<double> horizontalUncertainty; // nullopt: not assessed
    std::optional<double> verticalUncertainty;   // nullopt: no level, or not assessed
    bool hasLevel = false;                       // a level of the service was recorded

    friend bool operator==(const PositionEvidence&, const PositionEvidence&) = default;
};

struct Classification {
    QualityLevel level = QualityLevel::D;
    // False when a level was recorded but is not good to the tolerance of
    // `level` - a QL-B detection whose depth estimate is outside +/-500 mm
    // keeps its QL-B plan position, and its depth must not be relied on.
    bool levelQualified = false;
    // Why the result is below the method's ceiling, or why its level is not
    // qualified: one sentence each, in the order the rules were applied.
    // Empty when the ceiling was reached in full.
    std::vector<std::string> reasons;
};

// The highest level `evidence` supports under `tolerances`:
//   * never above maximumQualityLevel(method);
//   * QL-A needs a recorded level and BOTH uncertainties assessed and within
//     the QL-A tolerance - the standard's QL-A is a three-dimensional
//     position, so a plan position from a pothole with no level is QL-B;
//   * QL-B needs the horizontal uncertainty assessed and within tolerance;
//     an unassessed uncertainty is not assumed to be good;
//   * a detection or exposure that fails QL-B falls to QL-C: something was
//     found on site at that place, which is at least what a correlated
//     surface feature says, and nothing about its position is certified.
[[nodiscard]] Classification classify(const PositionEvidence& evidence,
                                      const QualityLevelTolerances& tolerances = {});

} // namespace katana::survey::subsurface
