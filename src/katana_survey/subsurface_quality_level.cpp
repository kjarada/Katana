#include "katana/survey/subsurface/quality_level.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <utility>

#include "katana/core/text.hpp"

namespace katana::survey::subsurface {

namespace {

// A tolerance test that treats an unassessed uncertainty as a failure. A
// non-finite or negative uncertainty is a broken record, not a small one.
bool within(const std::optional<double>& uncertainty, const std::optional<double>& tolerance)
{
    return uncertainty && tolerance && std::isfinite(*uncertainty) && *uncertainty >= 0.0 &&
           *uncertainty <= *tolerance;
}

std::string millimetres(double metres)
{
    return std::to_string(static_cast<long long>(std::llround(metres * 1000.0))) + " mm";
}

// The part of `text` that is the level letter, with "QL", '-', '_' and blanks
// stripped. Empty when there is anything else.
std::string compacted(std::string_view text)
{
    std::string out;
    for (const char ch : text) {
        if (core::isAsciiSpace(ch) || ch == '-' || ch == '_') {
            continue;
        }
        out += core::asciiLower(ch);
    }
    return out;
}

} // namespace

const char* toString(QualityLevel level)
{
    switch (level) {
    case QualityLevel::A:
        return "QL-A";
    case QualityLevel::B:
        return "QL-B";
    case QualityLevel::C:
        return "QL-C";
    case QualityLevel::D:
        return "QL-D";
    }
    return "QL-?";
}

std::optional<QualityLevel> parseQualityLevel(std::string_view text)
{
    std::string letters = compacted(text);
    if (letters.size() == 3 && letters.starts_with("ql")) {
        letters.erase(0, 2);
    } else if (letters.size() == 13 && letters.starts_with("qualitylevel")) {
        letters.erase(0, 12);
    }
    if (letters.size() != 1) {
        return std::nullopt;
    }
    switch (letters.front()) {
    case 'a':
        return QualityLevel::A;
    case 'b':
        return QualityLevel::B;
    case 'c':
        return QualityLevel::C;
    case 'd':
        return QualityLevel::D;
    default:
        return std::nullopt;
    }
}

const char* toString(LocationMethod method)
{
    switch (method) {
    case LocationMethod::Unknown:
        return "unknown method";
    case LocationMethod::Records:
        return "records";
    case LocationMethod::Anecdotal:
        return "anecdotal";
    case LocationMethod::SurfaceFeature:
        return "surface feature";
    case LocationMethod::ElectromagneticLocation:
        return "electromagnetic location";
    case LocationMethod::GroundPenetratingRadar:
        return "ground penetrating radar";
    case LocationMethod::OtherGeophysical:
        return "other geophysical";
    case LocationMethod::NonDestructiveExcavation:
        return "non-destructive excavation";
    case LocationMethod::OpenExcavation:
        return "open excavation";
    }
    return "unknown";
}

std::optional<LocationMethod> parseLocationMethod(std::string_view text)
{
    static constexpr auto kNames = std::to_array<std::pair<std::string_view, LocationMethod>>({
        {"unknown", LocationMethod::Unknown},
        {"records", LocationMethod::Records},
        {"archivedrawingsandplans", LocationMethod::Records},
        {"geographicinformationsystem", LocationMethod::Records},
        {"gis", LocationMethod::Records},
        {"electronicdetection", LocationMethod::ElectromagneticLocation},
        {"potholing", LocationMethod::NonDestructiveExcavation},
        {"survey", LocationMethod::SurfaceFeature},
        {"record", LocationMethod::Records},
        {"plans", LocationMethod::Records},
        {"anecdotal", LocationMethod::Anecdotal},
        {"verbal", LocationMethod::Anecdotal},
        {"surface", LocationMethod::SurfaceFeature},
        {"surfacefeature", LocationMethod::SurfaceFeature},
        {"feature", LocationMethod::SurfaceFeature},
        {"eml", LocationMethod::ElectromagneticLocation},
        {"electromagnetic", LocationMethod::ElectromagneticLocation},
        {"electromagneticlocation", LocationMethod::ElectromagneticLocation},
        {"gpr", LocationMethod::GroundPenetratingRadar},
        {"groundpenetratingradar", LocationMethod::GroundPenetratingRadar},
        {"geophysical", LocationMethod::OtherGeophysical},
        {"othergeophysical", LocationMethod::OtherGeophysical},
        {"ndd", LocationMethod::NonDestructiveExcavation},
        {"nde", LocationMethod::NonDestructiveExcavation},
        {"pothole", LocationMethod::NonDestructiveExcavation},
        {"vac", LocationMethod::NonDestructiveExcavation},
        {"nondestructiveexcavation", LocationMethod::NonDestructiveExcavation},
        {"trench", LocationMethod::OpenExcavation},
        {"openexcavation", LocationMethod::OpenExcavation},
    });
    // Blanks, '-' and '_' are not significant: "non-destructive excavation",
    // "Non_Destructive_Excavation" and "NonDestructiveExcavation" are one name.
    const std::string key = compacted(text);
    for (const auto& [name, method] : kNames) {
        if (key == name) {
            return method;
        }
    }
    return std::nullopt;
}

QualityLevel maximumQualityLevel(LocationMethod method)
{
    switch (method) {
    case LocationMethod::Unknown:
    case LocationMethod::Records:
    case LocationMethod::Anecdotal:
        return QualityLevel::D;
    case LocationMethod::SurfaceFeature:
        return QualityLevel::C;
    case LocationMethod::ElectromagneticLocation:
    case LocationMethod::GroundPenetratingRadar:
    case LocationMethod::OtherGeophysical:
        return QualityLevel::B;
    case LocationMethod::NonDestructiveExcavation:
    case LocationMethod::OpenExcavation:
        return QualityLevel::A;
    }
    return QualityLevel::D;
}

const Tolerance& QualityLevelTolerances::of(QualityLevel level) const
{
    switch (level) {
    case QualityLevel::A:
        return a;
    case QualityLevel::B:
        return b;
    case QualityLevel::C:
        return c;
    case QualityLevel::D:
        return d;
    }
    return d;
}

Classification classify(const PositionEvidence& evidence, const QualityLevelTolerances& tolerances)
{
    Classification result;
    const QualityLevel ceiling = maximumQualityLevel(evidence.method);
    if (ceiling <= QualityLevel::C) {
        // Records and surface features carry no measured position of the
        // service, so there is no tolerance to fail: the method is the answer.
        result.level = ceiling;
        return result;
    }

    const auto describe = [](const char* what, const std::optional<double>& value,
                             const std::optional<double>& limit, QualityLevel level) {
        std::string text = std::string(what) + " uncertainty ";
        text += value ? millimetres(*value) : std::string("not assessed");
        if (limit) {
            text += " against the " + std::string(toString(level)) + " tolerance of " +
                    millimetres(*limit);
        }
        return text;
    };

    if (ceiling == QualityLevel::A) {
        const Tolerance& a = tolerances.a;
        const bool horizontal = within(evidence.horizontalUncertainty, a.horizontal);
        const bool vertical = evidence.hasLevel && within(evidence.verticalUncertainty, a.vertical);
        if (horizontal && vertical) {
            result.level = QualityLevel::A;
            result.levelQualified = true;
            return result;
        }
        if (!evidence.hasLevel) {
            result.reasons.push_back("QL-A needs a level of the exposed service; none recorded");
        } else if (!vertical) {
            result.reasons.push_back(
                describe("vertical", evidence.verticalUncertainty, a.vertical, QualityLevel::A));
        }
        if (!horizontal) {
            result.reasons.push_back(describe("horizontal", evidence.horizontalUncertainty,
                                              a.horizontal, QualityLevel::A));
        }
    }

    const Tolerance& b = tolerances.b;
    if (within(evidence.horizontalUncertainty, b.horizontal)) {
        result.level = QualityLevel::B;
        result.levelQualified =
            evidence.hasLevel && within(evidence.verticalUncertainty, b.vertical);
        if (evidence.hasLevel && !result.levelQualified) {
            result.reasons.push_back(
                describe("vertical", evidence.verticalUncertainty, b.vertical, QualityLevel::B) +
                ": the plan position stands, the level must not be relied on");
        }
        return result;
    }
    result.reasons.push_back(
        describe("horizontal", evidence.horizontalUncertainty, b.horizontal, QualityLevel::B));
    result.level = QualityLevel::C;
    return result;
}

} // namespace katana::survey::subsurface
