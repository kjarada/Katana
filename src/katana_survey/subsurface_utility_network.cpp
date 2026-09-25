#include "katana/survey/subsurface/utility_network.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <utility>

#include "katana/core/text.hpp"

namespace katana::survey::subsurface {

namespace {

using core::ErrorCode;
using core::makeError;

// Lower case with blanks, '-' and '_' removed, so that "Recycled Water",
// "recycled_water" and "RecycledWater" are one key.
std::string key(std::string_view text)
{
    std::string out;
    for (const char ch : text) {
        if (!core::isAsciiSpace(ch) && ch != '-' && ch != '_') {
            out += core::asciiLower(ch);
        }
    }
    return out;
}

template <typename Enum, std::size_t N>
std::optional<Enum> lookUp(const std::array<std::pair<std::string_view, Enum>, N>& names,
                           std::string_view text)
{
    const std::string wanted = key(text);
    for (const auto& [name, value] : names) {
        if (wanted == name) {
            return value;
        }
    }
    return std::nullopt;
}

double planDistance(const Coordinate2& a, const Coordinate2& b)
{
    return std::hypot(b.northing - a.northing, b.easting - a.easting);
}

core::Status checkLine(const UtilityLine& line)
{
    if (line.vertices.size() < 2) {
        return makeError(ErrorCode::InvalidArgument, "a utility line needs at least two vertices",
                         line.id);
    }
    for (const UtilityVertex& vertex : line.vertices) {
        if (!std::isfinite(vertex.position.northing) || !std::isfinite(vertex.position.easting)) {
            return makeError(ErrorCode::InvalidArgument, "vertex has a non-finite coordinate",
                             line.id + " / " + vertex.id);
        }
    }
    if (!line.pathEvidence.empty() && line.pathEvidence.size() != line.vertices.size() - 1) {
        return makeError(ErrorCode::InvalidArgument,
                         "path evidence must name one entry per segment (" +
                             std::to_string(line.vertices.size() - 1) + "), not " +
                             std::to_string(line.pathEvidence.size()),
                         line.id);
    }
    return {};
}

} // namespace

const char* toString(UtilityType type)
{
    switch (type) {
    case UtilityType::Unknown:
        return "unknown";
    case UtilityType::Electricity:
        return "electricity";
    case UtilityType::Telecommunications:
        return "telecommunications";
    case UtilityType::Gas:
        return "gas";
    case UtilityType::Water:
        return "water";
    case UtilityType::RecycledWater:
        return "recycled water";
    case UtilityType::FireService:
        return "fire service";
    case UtilityType::Sewer:
        return "sewer";
    case UtilityType::Stormwater:
        return "stormwater";
    case UtilityType::Fuel:
        return "fuel";
    case UtilityType::IntelligentTransport:
        return "ITS";
    case UtilityType::Other:
        return "other";
    }
    return "unknown";
}

std::optional<UtilityType> parseUtilityType(std::string_view text)
{
    static constexpr auto kNames = std::to_array<std::pair<std::string_view, UtilityType>>({
        {"unknown", UtilityType::Unknown},
        {"electricity", UtilityType::Electricity},
        {"electrical", UtilityType::Electricity},
        {"elec", UtilityType::Electricity},
        {"power", UtilityType::Electricity},
        {"hv", UtilityType::Electricity},
        {"lv", UtilityType::Electricity},
        {"telecommunications", UtilityType::Telecommunications},
        {"telecoms", UtilityType::Telecommunications},
        {"telco", UtilityType::Telecommunications},
        {"comms", UtilityType::Telecommunications},
        {"communications", UtilityType::Telecommunications},
        {"nbn", UtilityType::Telecommunications},
        {"gas", UtilityType::Gas},
        {"water", UtilityType::Water},
        {"potable", UtilityType::Water},
        {"recycledwater", UtilityType::RecycledWater},
        {"recycled", UtilityType::RecycledWater},
        {"reuse", UtilityType::RecycledWater},
        {"sewer", UtilityType::Sewer},
        {"sewerage", UtilityType::Sewer},
        {"ww", UtilityType::Sewer},
        {"stormwater", UtilityType::Stormwater},
        {"storm", UtilityType::Stormwater},
        {"sw", UtilityType::Stormwater},
        {"drainage", UtilityType::Stormwater},
        {"fuel", UtilityType::Fuel},
        {"oil", UtilityType::Fuel},
        {"other", UtilityType::Other},
        // The asset type names of AS 5488.2 Table A.4, and the one-letter
        // codes delivery schemas key them by.
        {"communication", UtilityType::Telecommunications},
        {"fireservice", UtilityType::FireService},
        {"its", UtilityType::IntelligentTransport},
        {"intelligenttransport", UtilityType::IntelligentTransport},
        {"petroleum", UtilityType::Fuel},
        {"notspecified", UtilityType::Unknown},
        {"c", UtilityType::Telecommunications},
        {"d", UtilityType::Stormwater},
        {"e", UtilityType::Electricity},
        {"f", UtilityType::FireService},
        {"g", UtilityType::Gas},
        {"i", UtilityType::IntelligentTransport},
        {"p", UtilityType::Fuel},
        {"s", UtilityType::Sewer},
        {"w", UtilityType::Water},
        {"n", UtilityType::Unknown},
    });
    return lookUp(kNames, text);
}

const char* toString(UtilityStatus status)
{
    switch (status) {
    case UtilityStatus::Unknown:
        return "unknown";
    case UtilityStatus::InService:
        return "in service";
    case UtilityStatus::Disused:
        return "disused";
    case UtilityStatus::Abandoned:
        return "abandoned";
    case UtilityStatus::Proposed:
        return "proposed";
    }
    return "unknown";
}

std::optional<UtilityStatus> parseUtilityStatus(std::string_view text)
{
    static constexpr auto kNames = std::to_array<std::pair<std::string_view, UtilityStatus>>({
        {"unknown", UtilityStatus::Unknown},
        {"inservice", UtilityStatus::InService},
        {"live", UtilityStatus::InService},
        {"active", UtilityStatus::InService},
        {"abandoned", UtilityStatus::Abandoned},
        {"disused", UtilityStatus::Disused},
        {"redundant", UtilityStatus::Abandoned},
        {"proposed", UtilityStatus::Proposed},
    });
    return lookUp(kNames, text);
}

const char* toString(LevelReference reference)
{
    switch (reference) {
    case LevelReference::Top:
        return "top";
    case LevelReference::Centre:
        return "centre";
    case LevelReference::Invert:
        return "invert";
    case LevelReference::Unknown:
        return "unknown";
    }
    return "unknown";
}

std::optional<LevelReference> parseLevelReference(std::string_view text)
{
    static constexpr auto kNames = std::to_array<std::pair<std::string_view, LevelReference>>({
        {"top", LevelReference::Top},
        {"crown", LevelReference::Top},
        {"obvert", LevelReference::Top},
        {"centre", LevelReference::Centre},
        {"center", LevelReference::Centre},
        {"cl", LevelReference::Centre},
        {"invert", LevelReference::Invert},
        {"il", LevelReference::Invert},
        {"unknown", LevelReference::Unknown},
        // AS 5488.2's Depth Location names, as delivery schemas spell them.
        // What is met first above the service counts as its top: cover is
        // measured to it.
        {"topofpipe", LevelReference::Top},
        {"topofconcreteencasement", LevelReference::Top},
        {"plasticcoverprotectionencountered", LevelReference::Top},
        {"groundlevel", LevelReference::Top},
        {"toprowinvert", LevelReference::Invert},
        {"other", LevelReference::Unknown},
    });
    return lookUp(kNames, text);
}

const char* toString(PathEvidence evidence)
{
    switch (evidence) {
    case PathEvidence::Detected:
        return "detected";
    case PathEvidence::Exposed:
        return "exposed";
    case PathEvidence::Assumed:
        return "assumed";
    }
    return "assumed";
}

double GradedLine::length() const
{
    return lengthAt[0] + lengthAt[1] + lengthAt[2] + lengthAt[3];
}

core::Result<GradedLine> gradeLine(const UtilityLine& line, const GradingSettings& settings)
{
    if (auto status = checkLine(line); !status) {
        return status.error();
    }

    GradedLine graded;
    graded.vertices.reserve(line.vertices.size());
    for (const UtilityVertex& vertex : line.vertices) {
        PositionEvidence evidence = vertex.evidence;
        evidence.hasLevel = hasVerticalMeasurement(vertex);
        GradedVertex out;
        out.classification = classify(evidence, settings.tolerances);
        if (vertex.claimed && *vertex.claimed > out.classification.level) {
            out.overClaim = std::string("claimed ") + toString(*vertex.claimed) + ", the " +
                            toString(evidence.method) + " evidence supports " +
                            toString(out.classification.level);
        }
        graded.vertices.push_back(std::move(out));
    }

    for (std::size_t i = 0; i + 1 < line.vertices.size(); ++i) {
        GradedSegment segment;
        segment.from = i;
        segment.length = planDistance(line.vertices[i].position, line.vertices[i + 1].position);
        const QualityLevel start = graded.vertices[i].classification.level;
        const QualityLevel end = graded.vertices[i + 1].classification.level;
        QualityLevel level = std::min(start, end);
        if (start != end) {
            segment.limitedBy = std::string("the ") + toString(level) + " vertex " +
                                line.vertices[start < end ? i : i + 1].id;
        }

        const PathEvidence path =
            line.pathEvidence.empty() ? PathEvidence::Detected : line.pathEvidence[i];
        // Each cap is applied only when it lowers the level, and then names
        // itself, so that the reason given is the rule that decided.
        const auto cap = [&](QualityLevel limit, std::string why) {
            if (level > limit) {
                level = limit;
                segment.limitedBy = std::move(why);
            }
        };
        switch (path) {
        case PathEvidence::Exposed:
            break;
        case PathEvidence::Detected:
            cap(QualityLevel::B, "the path between the vertices was detected, not exposed");
            if (segment.length > settings.maximumDetectedSpacing) {
                cap(QualityLevel::C, "the segment is longer than the maximum detected spacing "
                                     "and was interpolated, not traced");
            }
            break;
        case PathEvidence::Assumed:
            cap(QualityLevel::C, "the path between the vertices was assumed, not observed");
            break;
        }
        segment.level = level;
        graded.lengthAt[static_cast<std::size_t>(level)] += segment.length;
        graded.segments.push_back(std::move(segment));
    }
    return graded;
}

std::optional<double> serviceLevel(const UtilityVertex& vertex)
{
    if (vertex.level) {
        return vertex.level;
    }
    if (vertex.surfaceLevel && vertex.depth) {
        return *vertex.surfaceLevel - *vertex.depth;
    }
    return std::nullopt;
}

bool hasVerticalMeasurement(const UtilityVertex& vertex)
{
    return vertex.level.has_value() || vertex.depth.has_value();
}

std::optional<double> topOffset(LevelReference reference, double diameter)
{
    const bool haveDiameter = std::isfinite(diameter) && diameter > 0.0;
    switch (reference) {
    case LevelReference::Top:
        return 0.0;
    case LevelReference::Centre:
        return haveDiameter ? std::optional<double>(diameter / 2.0) : std::nullopt;
    case LevelReference::Invert:
        return haveDiameter ? std::optional<double>(diameter) : std::nullopt;
    case LevelReference::Unknown:
        return std::nullopt;
    }
    return std::nullopt;
}

std::optional<double> topLevel(const UtilityVertex& vertex, double diameter)
{
    const std::optional<double> level = serviceLevel(vertex);
    const std::optional<double> offset = topOffset(vertex.levelReference, diameter);
    if (!level || !offset) {
        return std::nullopt;
    }
    return *level + *offset;
}

core::Result<std::vector<CoverResult>> depthOfCover(const UtilityLine& line,
                                                    std::optional<double> minimumCover,
                                                    const GradingSettings& settings)
{
    if (minimumCover && !std::isfinite(*minimumCover)) {
        return makeError(ErrorCode::InvalidArgument, "the minimum cover is not a finite number",
                         line.id);
    }
    auto graded = gradeLine(line, settings);
    if (!graded) {
        return graded.error();
    }

    std::vector<CoverResult> results;
    results.reserve(line.vertices.size());
    for (std::size_t i = 0; i < line.vertices.size(); ++i) {
        const UtilityVertex& vertex = line.vertices[i];
        CoverResult result;
        result.vertexId = vertex.id;
        const std::optional<double> offset =
            topOffset(vertex.levelReference, line.attributes.diameter);
        const std::optional<double> level = serviceLevel(vertex);
        if (!hasVerticalMeasurement(vertex)) {
            result.note = "no level or depth of the service";
        } else if (vertex.levelReference == LevelReference::Unknown) {
            result.note = "the level's place on the service is unknown";
        } else if (!offset) {
            result.note = std::string("a level on the ") + toString(vertex.levelReference) +
                          " needs the service's diameter";
        } else if (level && vertex.surfaceLevel) {
            result.cover = *vertex.surfaceLevel - (*level + *offset);
        } else if (vertex.depth && !vertex.level) {
            result.cover = *vertex.depth - *offset;
        } else {
            result.note = "no surface level";
        }
        if (result.cover) {
            if (!graded->vertices[i].classification.levelQualified) {
                result.note = std::string("the level is not qualified at ") +
                              toString(graded->vertices[i].classification.level) +
                              "; do not rely on this cover";
            } else if (line.attributes.diameterIsInside && *offset > 0.0) {
                result.note = "the size is an inside dimension: this cover is to the inside "
                              "top, and the service's outside is higher by its wall";
            }
            result.belowMinimum = minimumCover && *result.cover < *minimumCover;
        }
        results.push_back(std::move(result));
    }
    return results;
}

} // namespace katana::survey::subsurface
