#include "katana/survey/subsurface/verification.hpp"

#include <algorithm>
#include <cmath>

namespace katana::survey::subsurface {

namespace {

struct Found {
    const UtilityLine* line = nullptr;
    const UtilityVertex* vertex = nullptr;
};

Found find(const std::vector<UtilityLine>& lines, const UtilityLine& own, const std::string& id)
{
    for (const UtilityVertex& vertex : own.vertices) {
        if (vertex.id == id) {
            return {&own, &vertex};
        }
    }
    for (const UtilityLine& line : lines) {
        if (&line == &own) {
            continue;
        }
        for (const UtilityVertex& vertex : line.vertices) {
            if (vertex.id == id) {
                return {&line, &vertex};
            }
        }
    }
    return {};
}

} // namespace

VerificationReport verifyDetections(const std::vector<UtilityLine>& lines,
                                    const GradingSettings& settings)
{
    VerificationReport report;
    const Tolerance& b = settings.tolerances.b;
    double sumHorizontal = 0.0;
    double sumVertical = 0.0;

    for (const UtilityLine& line : lines) {
        for (const UtilityVertex& exposure : line.vertices) {
            if (exposure.verifies.empty()) {
                continue;
            }
            const std::string where = line.id + " / " + exposure.id;
            PositionEvidence evidence = exposure.evidence;
            evidence.hasLevel = exposure.level.has_value();
            const Classification grade = classify(evidence, settings.tolerances);
            if (grade.level != QualityLevel::A) {
                report.problems.push_back(where + " checks " + exposure.verifies + " but is " +
                                          toString(grade.level) +
                                          ", not QL-A: only an exposure verifies a detection");
                continue;
            }
            const Found detected = find(lines, line, exposure.verifies);
            if (!detected.vertex) {
                report.problems.push_back(where + " checks " + exposure.verifies +
                                          ", which no line has");
                continue;
            }

            VerificationResult result;
            result.lineId = detected.line->id;
            result.detectedId = detected.vertex->id;
            result.exposedId = exposure.id;
            result.horizontalDeviation =
                std::hypot(exposure.position.northing - detected.vertex->position.northing,
                           exposure.position.easting - detected.vertex->position.easting);
            result.horizontalWithin = b.horizontal && result.horizontalDeviation <= *b.horizontal;

            const auto exposedTop = topLevel(exposure, line.attributes.diameter);
            const auto detectedTop = topLevel(*detected.vertex, detected.line->attributes.diameter);
            if (exposedTop && detectedTop) {
                result.verticalDeviation = *exposedTop - *detectedTop;
                if (b.vertical) {
                    result.verticalWithin = std::abs(*result.verticalDeviation) <= *b.vertical;
                }
            }

            report.horizontalPassed += result.horizontalWithin ? 1 : 0;
            report.maximumHorizontal =
                std::max(report.maximumHorizontal, result.horizontalDeviation);
            sumHorizontal += result.horizontalDeviation * result.horizontalDeviation;
            if (result.verticalDeviation) {
                const double magnitude = std::abs(*result.verticalDeviation);
                ++report.verticalChecked;
                report.verticalPassed += result.verticalWithin.value_or(false) ? 1 : 0;
                report.maximumVertical = std::max(report.maximumVertical.value_or(0.0), magnitude);
                sumVertical += magnitude * magnitude;
            }
            report.results.push_back(std::move(result));
        }
    }

    if (!report.results.empty()) {
        report.rmsHorizontal =
            std::sqrt(sumHorizontal / static_cast<double>(report.results.size()));
    }
    if (report.verticalChecked > 0) {
        report.rmsVertical = std::sqrt(sumVertical / static_cast<double>(report.verticalChecked));
    }
    return report;
}

} // namespace katana::survey::subsurface
