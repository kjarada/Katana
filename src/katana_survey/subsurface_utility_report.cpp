#include "katana/survey/subsurface/utility_report.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <format>
#include <map>

namespace katana::survey::subsurface {

namespace {

constexpr std::array<QualityLevel, 4> kBestFirst{QualityLevel::A, QualityLevel::B, QualityLevel::C,
                                                 QualityLevel::D};

std::string metres(double value)
{
    return std::format("{:.3f}", value);
}

std::string millimetres(double value)
{
    return std::format("{:.0f} mm", value * 1000.0);
}

std::string toleranceText(const Tolerance& tolerance)
{
    if (!tolerance.horizontal && !tolerance.vertical) {
        return "none stated";
    }
    std::string text;
    if (tolerance.horizontal) {
        text += "+/-" + millimetres(*tolerance.horizontal) + " H";
    }
    if (tolerance.vertical) {
        text += (text.empty() ? "" : ", ") + std::string("+/-") + millimetres(*tolerance.vertical) +
                " V";
    }
    return text;
}

std::string settingsText(const GradingSettings& settings)
{
    std::string text = "Tolerances: ";
    text += "QL-A " + toleranceText(settings.tolerances.a);
    text += "; QL-B " + toleranceText(settings.tolerances.b);
    if (std::isfinite(settings.maximumDetectedSpacing)) {
        text += "; detected spacing <= " + metres(settings.maximumDetectedSpacing) + " m";
    }
    return text + "\n";
}

std::string attributesText(const UtilityAttributes& attributes)
{
    std::string text = toString(attributes.type);
    if (!attributes.owner.empty()) {
        text += ", " + attributes.owner;
    }
    if (!attributes.material.empty()) {
        text += ", " + attributes.material;
    }
    if (attributes.diameter > 0.0) {
        text += ", " + millimetres(attributes.diameter);
    }
    if (!attributes.configuration.empty()) {
        text += ", " + attributes.configuration;
    }
    text += std::string(", ") + toString(attributes.status);
    return text;
}

std::vector<std::string> missingAttributes(const UtilityAttributes& attributes)
{
    std::vector<std::string> missing;
    if (attributes.type == UtilityType::Unknown) {
        missing.emplace_back("type");
    }
    if (attributes.owner.empty()) {
        missing.emplace_back("owner");
    }
    if (attributes.status == UtilityStatus::Unknown) {
        missing.emplace_back("status");
    }
    if (attributes.material.empty()) {
        missing.emplace_back("material");
    }
    if (!(attributes.diameter > 0.0)) {
        missing.emplace_back("size");
    }
    return missing;
}

std::string lengthsText(const std::array<double, 4>& lengthAt)
{
    std::string text;
    for (const QualityLevel level : kBestFirst) {
        text += std::format("{:>12}", metres(lengthAt[static_cast<std::size_t>(level)]));
    }
    return text;
}

} // namespace

core::Result<std::string> renderInvestigationReport(const std::vector<UtilityLine>& lines,
                                                    const GradingSettings& settings,
                                                    std::optional<double> minimumCover)
{
    std::string body;
    std::vector<std::string> findings;
    std::map<std::string, std::array<double, 4>> byType;
    std::array<double, 4> total{};
    std::size_t vertexCount = 0;

    for (const UtilityLine& line : lines) {
        auto graded = gradeLine(line, settings);
        if (!graded) {
            return graded.error();
        }
        auto cover = depthOfCover(line, minimumCover, settings);
        if (!cover) {
            return cover.error();
        }
        vertexCount += line.vertices.size();

        body += "\nLine " + line.id + ": " + attributesText(line.attributes) + "\n";
        body += "  length " + metres(graded->length()) + " m:";
        for (const QualityLevel level : kBestFirst) {
            body += std::string(" ") + toString(level) + " " +
                    metres(graded->lengthAt[static_cast<std::size_t>(level)]);
        }
        body += "\n";

        for (std::size_t i = 0; i < line.vertices.size(); ++i) {
            const UtilityVertex& vertex = line.vertices[i];
            const GradedVertex& grade = graded->vertices[i];
            const CoverResult& depth = (*cover)[i];
            body += std::format("  {:<10} {:<28} {}", vertex.id, toString(vertex.evidence.method),
                                toString(grade.classification.level));
            if (depth.cover) {
                body += "  cover " + metres(*depth.cover);
            }
            body += "\n";
            for (const std::string& reason : grade.classification.reasons) {
                body += "             " + reason + "\n";
            }

            const std::string where = line.id + " / " + vertex.id;
            if (!grade.overClaim.empty()) {
                findings.push_back(where + ": " + grade.overClaim);
            }
            if (depth.belowMinimum) {
                findings.push_back(where + ": cover " + metres(*depth.cover) +
                                   " m is below the minimum " + metres(*minimumCover) + " m");
            }
            if (depth.cover && !depth.note.empty()) {
                findings.push_back(where + ": " + depth.note);
            }
        }
        for (const GradedSegment& segment : graded->segments) {
            body += std::format("  {} -> {}  {} m  {}", line.vertices[segment.from].id,
                                line.vertices[segment.from + 1].id, metres(segment.length),
                                toString(segment.level));
            if (!segment.limitedBy.empty()) {
                body += "  (" + segment.limitedBy + ")";
            }
            body += "\n";
        }

        // A delivery schema claims one level for a whole asset, so the claim
        // is tested along it too: a segment between two vertices that both
        // claim a level is claimed at the weaker of the two.
        double overClaimedLength = 0.0;
        std::vector<std::string> overClaimed;
        for (const GradedSegment& segment : graded->segments) {
            const auto& from = line.vertices[segment.from].claimed;
            const auto& to = line.vertices[segment.from + 1].claimed;
            if (!from || !to || std::min(*from, *to) <= segment.level) {
                continue;
            }
            overClaimedLength += segment.length;
            if (overClaimed.size() < 3) {
                overClaimed.push_back(line.vertices[segment.from].id + " -> " +
                                      line.vertices[segment.from + 1].id + " claimed " +
                                      toString(std::min(*from, *to)) + ", grades " +
                                      toString(segment.level));
            }
        }
        if (!overClaimed.empty()) {
            std::string list;
            for (const std::string& entry : overClaimed) {
                list += (list.empty() ? "" : "; ") + entry;
            }
            findings.push_back(line.id + ": " + metres(overClaimedLength) +
                               " m is claimed better than it grades: " + list);
        }

        const std::vector<std::string> missing = missingAttributes(line.attributes);
        if (!missing.empty()) {
            std::string list;
            for (const std::string& name : missing) {
                list += (list.empty() ? "" : ", ") + name;
            }
            findings.push_back(line.id + ": not recorded: " + list);
        }

        auto& lengths = byType[toString(line.attributes.type)];
        for (std::size_t level = 0; level < 4; ++level) {
            lengths[level] += graded->lengthAt[level];
            total[level] += graded->lengthAt[level];
        }
    }

    std::string report =
        "AS 5488 subsurface utility investigation: " + std::to_string(lines.size()) + " lines, " +
        std::to_string(vertexCount) + " vertices\n";
    report += settingsText(settings);
    if (minimumCover) {
        report += "Minimum cover: " + metres(*minimumCover) + " m\n";
    }
    report += body;

    report += "\nLength by type and quality level (m)\n";
    report += std::format("  {:<20}{:>12}{:>12}{:>12}{:>12}{:>12}\n", "type", "QL-A", "QL-B",
                          "QL-C", "QL-D", "total");
    for (const auto& [type, lengths] : byType) {
        report += std::format("  {:<20}{}{:>12}\n", type, lengthsText(lengths),
                              metres(lengths[0] + lengths[1] + lengths[2] + lengths[3]));
    }
    report += std::format("  {:<20}{}{:>12}\n", "all", lengthsText(total),
                          metres(total[0] + total[1] + total[2] + total[3]));

    report += "\nFindings: " + std::to_string(findings.size()) + "\n";
    for (const std::string& finding : findings) {
        report += "  " + finding + "\n";
    }
    if (findings.empty()) {
        report += "  none\n";
    }
    return report;
}

std::string renderClearanceReport(const DesignAlignment& design,
                                  const std::vector<ClearanceResult>& results,
                                  const ClearanceRequirement& requirement)
{
    std::vector<const ClearanceResult*> order;
    std::vector<std::string> services;
    std::array<std::size_t, 4> counts{};
    for (const ClearanceResult& result : results) {
        ++counts[static_cast<std::size_t>(result.status)];
        if (std::find(services.begin(), services.end(), result.utilityId) == services.end()) {
            services.push_back(result.utilityId);
        }
        if (result.status != ClearanceStatus::Clear) {
            order.push_back(&result);
        }
    }
    std::stable_sort(order.begin(), order.end(), [](const auto* a, const auto* b) {
        return a->status != b->status ? a->status < b->status : a->horizontalGap < b->horizontalGap;
    });

    std::string report =
        "Clearance of " + (design.id.empty() ? std::string("the design") : design.id) +
        " (half-width " + metres(design.halfWidth) + " m) from " + std::to_string(services.size()) +
        " services, " + std::to_string(results.size()) + " segments\n";
    report += "Required: " + metres(requirement.horizontal) + " m horizontal, " +
              metres(requirement.vertical) + " m vertical; QL-C/QL-D margin " +
              metres(requirement.unverifiedMargin) + " m\n";
    report += std::format("Segments: {} conflict, {} unconfirmed, {} within tolerance, {} clear\n",
                          counts[0], counts[1], counts[2], counts[3]);
    // The worst clash of each service, for a delivery schema's Clash attribute.
    std::vector<std::pair<std::string, Clash>> clashes;
    for (const ClearanceResult& result : results) {
        auto found = std::find_if(clashes.begin(), clashes.end(), [&result](const auto& entry) {
            return entry.first == result.utilityId;
        });
        if (found == clashes.end()) {
            clashes.emplace_back(result.utilityId, clashOf(result));
        } else {
            found->second = std::min(found->second, clashOf(result));
        }
    }
    report += "Suggested Clash attribute:";
    for (const auto& [service, clash] : clashes) {
        report +=
            " " + service + " " + toString(clash) + (&clashes.back().first == &service ? "" : ",");
    }
    report += "\n";
    if (order.empty()) {
        return report + "Every segment is clear.\n";
    }

    report += "\n";
    report += std::format("  {:<26}{:<6}{:<18}{:>10}{:>8}{:>10}\n", "service segment", "QL",
                          "status", "plan gap", "tol H", "vert gap");
    std::vector<std::string> toLocate;
    for (const ClearanceResult* result : order) {
        report += std::format(
            "  {:<26}{:<6}{:<18}{:>10}{:>8}{:>10}",
            result->utilityId + " " + result->fromId + " -> " + result->toId,
            toString(result->level), toString(result->status), metres(result->horizontalGap),
            result->horizontalTolerance ? metres(*result->horizontalTolerance) : std::string("-"),
            result->verticalGap ? metres(*result->verticalGap) : std::string("-"));
        if (!result->note.empty()) {
            report += "  " + result->note;
        }
        report += "\n";
        if (result->status != ClearanceStatus::Conflict) {
            toLocate.push_back(result->utilityId + std::format(" near N {:.3f} E {:.3f}",
                                                               result->nearest.northing,
                                                               result->nearest.easting));
        }
    }
    if (!toLocate.empty()) {
        report += "\nPothole (QL-A) before relying on the design:\n";
        for (const std::string& entry : toLocate) {
            report += "  " + entry + "\n";
        }
    }
    return report;
}

std::string renderVerificationReport(const VerificationReport& report,
                                     const GradingSettings& settings)
{
    std::string text =
        "Verification of detections by exposure: " + std::to_string(report.results.size()) +
        " comparisons\n";
    text += settingsText(settings);
    text += "\n";
    text += std::format("  {:<10}{:<12}{:<12}{:>10}{:>6}{:>10}{:>6}\n", "line", "detected",
                        "exposed", "dH (m)", "", "dV (m)", "");
    for (const VerificationResult& result : report.results) {
        text += std::format("  {:<10}{:<12}{:<12}{:>10}{:>6}{:>10}{:>6}\n", result.lineId,
                            result.detectedId, result.exposedId, metres(result.horizontalDeviation),
                            result.horizontalWithin ? "ok" : "FAIL",
                            result.verticalDeviation ? metres(*result.verticalDeviation)
                                                     : std::string("-"),
                            result.verticalWithin ? (*result.verticalWithin ? "ok" : "FAIL") : "");
    }
    const std::size_t n = report.results.size();
    text += "\nHorizontal: " + std::to_string(report.horizontalPassed) + " of " +
            std::to_string(n) + " within QL-B";
    if (n > 0) {
        text += ", max " + metres(report.maximumHorizontal) + " m, RMS " +
                metres(report.rmsHorizontal) + " m";
    }
    text += "\nVertical: " + std::to_string(report.verticalPassed) + " of " +
            std::to_string(report.verticalChecked) + " within QL-B";
    if (report.maximumVertical && report.rmsVertical) {
        text += ", max " + metres(*report.maximumVertical) + " m, RMS " +
                metres(*report.rmsVertical) + " m";
    }
    text += "\n";

    if (!report.problems.empty()) {
        text += "\nNot used: " + std::to_string(report.problems.size()) + "\n";
        for (const std::string& problem : report.problems) {
            text += "  " + problem + "\n";
        }
    }

    const bool allPassed =
        report.horizontalPassed == n && report.verticalPassed == report.verticalChecked;
    if (n == 0) {
        text += "\nConclusion: nothing was verified; the QL-B grades rest on the locator's "
                "stated uncertainty alone.\n";
    } else if (allPassed) {
        text += "\nConclusion: every checked detection is within the QL-B tolerance.\n";
    } else {
        text += "\nConclusion: some detections are outside the QL-B tolerance; review the QL-B "
                "grades of the lines they belong to.\n";
    }
    return text;
}

} // namespace katana::survey::subsurface
