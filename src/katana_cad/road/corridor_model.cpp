#include "katana/cad/road/corridor_model.hpp"

#include <cmath>
#include <set>
#include <utility>

#include <nlohmann/json.hpp>

#include "katana/cad/road/road_template.hpp"
#include "katana/core/text.hpp"

namespace katana::cad::road {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using Json = nlohmann::json;

namespace {

constexpr std::string_view kFormat = "katana-corridors";

Status refuse(const Corridor& corridor, const std::string& what)
{
    return makeError(ErrorCode::InvalidArgument, "corridor " + corridor.name + ": " + what);
}

std::string number(double value)
{
    return katana::core::formatExactReal(value);
}

} // namespace

const Corridor* CorridorSet::find(std::string_view name) const
{
    for (const Corridor& each : corridors) {
        if (katana::core::equalsIgnoringCase(each.name, name)) {
            return &each;
        }
    }
    return nullptr;
}

Corridor* CorridorSet::find(std::string_view name)
{
    for (Corridor& each : corridors) {
        if (katana::core::equalsIgnoringCase(each.name, name)) {
            return &each;
        }
    }
    return nullptr;
}

Status checkCorridor(const Corridor& corridor)
{
    if (auto status = checkName("a corridor", corridor.name); !status) {
        return status;
    }
    if (corridor.alignment.empty()) {
        return refuse(corridor, "it needs an alignment");
    }
    if (corridor.templateName.empty()) {
        return refuse(corridor, "it needs a road template");
    }
    if (!std::isfinite(corridor.interval) || !(corridor.interval > 0.0)) {
        return refuse(corridor, "the interval must be positive");
    }
    if (!std::isfinite(corridor.curveTolerance) || corridor.curveTolerance < 0.0 ||
        corridor.curveTolerance > kLargestCurveTolerance) {
        return refuse(corridor, "the curve tolerance is 0 (none) or a relative error of at most " +
                                    number(kLargestCurveTolerance) + ", not " +
                                    number(corridor.curveTolerance));
    }
    if ((corridor.start && !std::isfinite(*corridor.start)) ||
        (corridor.end && !std::isfinite(*corridor.end))) {
        return refuse(corridor, "a station is not a finite number");
    }
    if (corridor.start && corridor.end && !(*corridor.start < *corridor.end)) {
        return refuse(corridor, "the start station must be before the end station");
    }
    const TemplateRegion* previous = nullptr;
    for (const TemplateRegion& region : corridor.regions) {
        if (!std::isfinite(region.start) || !std::isfinite(region.end) ||
            !(region.start < region.end)) {
            return refuse(corridor, "a region runs from a station to a later one: " +
                                        number(region.start) + " to " + number(region.end));
        }
        if (region.templateName.empty()) {
            return refuse(corridor, "a region names the template it uses");
        }
        if (previous != nullptr && region.start < previous->end) {
            return refuse(corridor, "regions may not overlap: " + number(previous->start) + " to " +
                                        number(previous->end) + " and " + number(region.start) +
                                        " to " + number(region.end));
        }
        previous = &region;
    }
    const SuperelevationPoint* before = nullptr;
    for (const SuperelevationPoint& point : corridor.superelevation) {
        if (!std::isfinite(point.station) || !std::isfinite(point.left) ||
            !std::isfinite(point.right)) {
            return refuse(corridor, "a superelevation value is not a finite number");
        }
        if (std::abs(point.left) > 100.0 || std::abs(point.right) > 100.0) {
            return refuse(corridor, "a superelevation crossfall is a percent within -100 to 100");
        }
        if (before != nullptr && !(point.station > before->station)) {
            return refuse(corridor, "the superelevation table has two entries at station " +
                                        number(point.station) + " or is out of order");
        }
        before = &point;
    }
    return {};
}

std::optional<Crossfalls> superelevationAt(const Corridor& corridor, double station)
{
    const auto& table = corridor.superelevation;
    if (table.empty() || station < table.front().station || station > table.back().station) {
        return std::nullopt;
    }
    for (std::size_t i = 0; i + 1 < table.size(); ++i) {
        const SuperelevationPoint& a = table[i];
        const SuperelevationPoint& b = table[i + 1];
        if (station >= a.station && station <= b.station) {
            const double t = (station - a.station) / (b.station - a.station);
            return Crossfalls{a.left + (b.left - a.left) * t, a.right + (b.right - a.right) * t};
        }
    }
    return Crossfalls{table.back().left, table.back().right};
}

const std::string& templateAt(const Corridor& corridor, double station)
{
    for (const TemplateRegion& region : corridor.regions) {
        if (station >= region.start && station <= region.end) {
            return region.templateName;
        }
    }
    return corridor.templateName;
}

Result<std::string> corridorsToJson(const CorridorSet& set)
{
    Json corridors = Json::array();
    for (const Corridor& corridor : set.corridors) {
        Json regions = Json::array();
        for (const TemplateRegion& region : corridor.regions) {
            regions.push_back(Json{
                {"start", region.start}, {"end", region.end}, {"template", region.templateName}});
        }
        Json table = Json::array();
        for (const SuperelevationPoint& point : corridor.superelevation) {
            table.push_back(
                Json{{"station", point.station}, {"left", point.left}, {"right", point.right}});
        }
        Json out{{"name", corridor.name},
                 {"alignment", corridor.alignment},
                 {"template", corridor.templateName},
                 {"surface", corridor.surface},
                 {"interval", corridor.interval},
                 {"curveTolerance", corridor.curveTolerance},
                 {"regions", std::move(regions)},
                 {"superelevation", std::move(table)}};
        // Absent stays absent: no key, not a zero.
        if (corridor.start) {
            out["start"] = *corridor.start;
        }
        if (corridor.end) {
            out["end"] = *corridor.end;
        }
        corridors.push_back(std::move(out));
    }
    const Json root{
        {"format", kFormat}, {"version", kCorridorsVersion}, {"corridors", std::move(corridors)}};
    try {
        return root.dump();
    } catch (const std::exception& error) {
        return makeError(ErrorCode::InvalidArgument, "the corridors cannot be written as JSON",
                         error.what());
    }
}

Result<CorridorSet> corridorsFromJson(std::string_view text)
{
    CorridorSet set;
    try {
        const Json root = Json::parse(text);
        if (!root.is_object() || root.value("format", std::string{}) != kFormat) {
            return makeError(ErrorCode::ParseFailure, "this is not a set of corridors");
        }
        const int version = root.at("version").get<int>();
        if (version > kCorridorsVersion) {
            return makeError(ErrorCode::Unsupported,
                             "the corridors were written by a newer version of Katana",
                             "version " + std::to_string(version));
        }
        for (const Json& value : root.at("corridors")) {
            Corridor corridor;
            corridor.name = value.at("name").get<std::string>();
            corridor.alignment = value.at("alignment").get<std::string>();
            corridor.templateName = value.at("template").get<std::string>();
            corridor.surface = value.value("surface", std::string{});
            corridor.interval = value.at("interval").get<double>();
            // A corridor saved before the curve tolerance existed has none
            // written, and reads as the default: its quantities on a curve come
            // out as the tolerance says now, not as the chords did.
            corridor.curveTolerance = value.value("curveTolerance", kDefaultCurveTolerance);
            if (value.contains("start")) {
                corridor.start = value.at("start").get<double>();
            }
            if (value.contains("end")) {
                corridor.end = value.at("end").get<double>();
            }
            for (const Json& region : value.value("regions", Json::array())) {
                corridor.regions.push_back(
                    TemplateRegion{region.at("start").get<double>(), region.at("end").get<double>(),
                                   region.at("template").get<std::string>()});
            }
            for (const Json& point : value.value("superelevation", Json::array())) {
                corridor.superelevation.push_back(SuperelevationPoint{
                    point.at("station").get<double>(), point.at("left").get<double>(),
                    point.at("right").get<double>()});
            }
            set.corridors.push_back(std::move(corridor));
        }
    } catch (const std::exception& error) {
        return makeError(ErrorCode::ParseFailure, "the corridors cannot be read", error.what());
    }
    std::set<std::string> names;
    for (const Corridor& corridor : set.corridors) {
        if (auto status = checkCorridor(corridor); !status) {
            return status.error();
        }
        if (!names.insert(katana::core::lowered(corridor.name)).second) {
            return makeError(ErrorCode::ParseFailure, "two corridors have one name", corridor.name);
        }
    }
    return set;
}

} // namespace katana::cad::road
