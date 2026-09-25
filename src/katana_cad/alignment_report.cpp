#include "katana/cad/alignment_report.hpp"

#include <algorithm>
#include <charconv>
#include <cmath>
#include <iomanip>
#include <sstream>
#include <string>
#include <system_error>
#include <vector>

#include "katana/cad/code_edit.hpp"
#include "katana/math/numerics.hpp"
#include "katana/survey/angles.hpp"

namespace katana::cad {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::AlignmentElementKind;
using katana::geometry::SolvedAlignment;

namespace {

// More rows than a person can use: a mistyped interval on a long road, which
// would otherwise build a table of millions and hang the dialog showing it.
constexpr std::size_t kMaximumStations = 100000;

// Fixed decimals, locale-independent, and never "-0.000": a coordinate a
// hair below zero reads as zero, as it is to the precision shown.
std::string fixed(double value, int decimals)
{
    char buffer[64];
    auto [end, error] =
        std::to_chars(buffer, buffer + sizeof buffer, value, std::chars_format::fixed, decimals);
    if (error != std::errc{}) {
        return std::isfinite(value) ? std::string("overflow") : std::string("nan");
    }
    std::string text(buffer, end);
    if (text.front() == '-' && text.find_first_not_of("-0.") == std::string::npos) {
        text.erase(0, 1);
    }
    return text;
}

char letterOf(AlignmentElementKind kind)
{
    switch (kind) {
    case AlignmentElementKind::Tangent:
        return 'T';
    case AlignmentElementKind::Spiral:
        return 'S';
    case AlignmentElementKind::Arc:
        return 'C';
    }
    return '?';
}

// The name of the joint between two elements: the letters of the two kinds
// met, which is how TS, SC, CS, ST, TC and CT are made - except two tangents,
// which meet at a PI with no curve.
std::string jointName(AlignmentElementKind before, AlignmentElementKind after)
{
    if (before == AlignmentElementKind::Tangent && after == AlignmentElementKind::Tangent) {
        return "PI";
    }
    return {letterOf(before), letterOf(after)};
}

// Azimuth clockwise from grid north, in [0, 2 pi), from a direction
// counter-clockwise from +x.
double azimuthOf(double direction)
{
    double azimuth = std::fmod(math::kHalfPi - direction, math::kTwoPi);
    if (azimuth < 0.0) {
        azimuth += math::kTwoPi;
    }
    // Due north computed a hair west of north is north: a direction within
    // the angular tolerance of a whole turn is parallel to it. (fmod of a
    // value a hair below a turn, plus a turn, can also round up to 2 pi.)
    return azimuth >= math::kTwoPi - tol::kAngular ? 0.0 : azimuth;
}

} // namespace

Result<std::vector<SettingOutStation>> settingOutStations(const SolvedAlignment& alignment,
                                                          double interval)
{
    if (!std::isfinite(interval) || !(interval > 0.0)) {
        return makeError(ErrorCode::InvalidArgument, "the interval must be positive",
                         "interval=" + fixed(interval, 3));
    }
    const double start = alignment.startStation();
    const double end = alignment.endStation();
    // Counted before a row is made: a table that is refused for its size
    // must not be built first.
    const double intervalCount = std::floor((end - start) / interval) + 1.0;
    if (intervalCount > static_cast<double>(kMaximumStations)) {
        return makeError(ErrorCode::InvalidArgument, "that interval gives too many stations",
                         fixed(intervalCount, 0) + " stations, the limit is " +
                             std::to_string(kMaximumStations));
    }

    struct Candidate {
        double station = 0.0;
        std::string key;
    };
    std::vector<Candidate> candidates;
    const std::vector<double> keys = alignment.keyStations();
    const auto& elements = alignment.elements();
    // keyStations is the start and then every element's end, in order, so
    // key k is the joint between element k-1 and element k.
    for (std::size_t k = 0; k < keys.size(); ++k) {
        std::string name;
        if (k == 0) {
            name = "start";
        } else if (k == keys.size() - 1) {
            name = "end";
        } else {
            name = jointName(geometry::kindOf(elements[k - 1]), geometry::kindOf(elements[k]));
        }
        candidates.push_back({keys[k], std::move(name)});
    }
    // Multiplied rather than accumulated, so the hundredth station is not
    // off by a hundred roundings.
    for (std::size_t i = 0;; ++i) {
        const double station = start + static_cast<double>(i) * interval;
        if (!(station < end)) {
            break;
        }
        candidates.push_back({station, {}});
    }
    std::stable_sort(candidates.begin(), candidates.end(),
                     [](const Candidate& a, const Candidate& b) { return a.station < b.station; });
    // A station the interval lands on to within the geometric tolerance IS
    // the key station there: one row, which keeps the key and the key's own
    // station.
    std::vector<Candidate> merged;
    for (Candidate& candidate : candidates) {
        if (!merged.empty() && candidate.station - merged.back().station <= tol::kGeometric) {
            if (merged.back().key.empty() && !candidate.key.empty()) {
                merged.back() = std::move(candidate);
            }
            continue;
        }
        merged.push_back(std::move(candidate));
    }

    std::vector<SettingOutStation> rows;
    rows.reserve(merged.size());
    for (const Candidate& candidate : merged) {
        // The last key is the sum of the element lengths and the end station
        // the total length, which can differ in the last bit; a query just
        // past the end would answer nothing.
        const double station = std::clamp(candidate.station, start, end);
        const auto point = alignment.pointAtStation(station);
        const auto direction = alignment.directionAtStation(station);
        const auto curvature = alignment.curvatureAtStation(station);
        if (!point || !direction || !curvature) {
            // Every station here is inside the alignment, so this is a
            // defect to report, never a row to leave out quietly.
            return makeError(ErrorCode::Internal, "a station on the alignment has no position",
                             "station=" + fixed(station, 6));
        }
        SettingOutStation row;
        row.station = station;
        row.point = *point;
        row.direction = *direction;
        row.azimuth = azimuthOf(*direction);
        row.curvature = *curvature;
        row.key = candidate.key;
        rows.push_back(std::move(row));
    }
    return rows;
}

std::string formatAzimuth(double azimuth)
{
    auto text = survey::formatDms(azimuth, 0);
    if (!text) {
        // Finite by construction everywhere this is called; a number is
        // still better than a blank if that ever stops being true.
        return fixed(azimuth * math::kRadToDeg, 6);
    }
    // Within half a second of a whole turn rounds to 360 degrees, which is
    // north and is written 0.
    if (text->starts_with("360")) {
        text->replace(0, 3, "0");
    }
    return *text;
}

std::string formatSettingOut(const std::vector<SettingOutStation>& stations)
{
    std::string text;
    for (const SettingOutStation& row : stations) {
        if (!text.empty()) {
            text += "\n";
        }
        text += "station=" + fixed(row.station, 3) + " x=" + fixed(row.point.x, 3) +
                " y=" + fixed(row.point.y, 3) +
                " direction=" + fixed(row.direction * math::kRadToDeg, 3) +
                " azimuth=" + formatAzimuth(row.azimuth);
        if (row.curvature == 0.0) {
            text += " radius=straight";
        } else {
            text += " radius=" + fixed(1.0 / std::abs(row.curvature), 3) +
                    (row.curvature > 0.0 ? " turn=left" : " turn=right");
        }
        if (!row.key.empty()) {
            text += " key=" + row.key;
        }
    }
    return text;
}

std::string settingOutCsv(const std::vector<SettingOutStation>& stations)
{
    std::string text = "Chainage,Easting,Northing,Azimuth (deg),Azimuth (DMS),Radius,Key\r\n";
    for (const SettingOutStation& row : stations) {
        text += fixed(row.station, 3) + "," + fixed(row.point.x, 3) + "," +
                fixed(row.point.y, 3) + "," + fixed(row.azimuth * math::kRadToDeg, 6) + "," +
                csvField(formatAzimuth(row.azimuth)) + "," +
                (row.curvature == 0.0 ? std::string() : fixed(1.0 / row.curvature, 3)) + "," +
                csvField(row.key) + "\r\n";
    }
    return text;
}

std::optional<double> curveK(const geometry::ProfileElement& element)
{
    if (element.kind != geometry::ProfileElementKind::Curve) {
        return std::nullopt;
    }
    const double changePercent = std::abs(element.endGrade - element.startGrade) * 100.0;
    if (math::nearlyZero(changePercent)) {
        return std::nullopt;
    }
    return element.length / changePercent;
}

std::string formatProfileReport(const geometry::VerticalAlignment& definition,
                                const geometry::SolvedProfile& profile)
{
    std::ostringstream out;
    out << std::fixed;
    out.precision(3);
    out << "  PVIs:";
    for (const geometry::ProfilePVI& pvi : definition.pvis) {
        out << "  " << pvi.station << " @ " << pvi.elevation;
        if (pvi.curveLength > 0.0) {
            out << " L=" << pvi.curveLength;
        }
    }
    out << "\n";
    for (const geometry::ProfileElement& element : profile.elements()) {
        const bool curve = element.kind == geometry::ProfileElementKind::Curve;
        out << "  " << (curve ? "curve  " : "tangent") << "  " << std::setw(10)
            << element.startStation << " to " << std::setw(10)
            << element.startStation + element.length << "  grade " << std::setw(7)
            << element.startGrade * 100.0 << "%";
        if (curve) {
            out << " to " << std::setw(7) << element.endGrade * 100.0 << "%";
            if (const auto k = curveK(element)) {
                out << "  K=" << *k;
            }
        }
        out << "\n";
    }
    for (const geometry::ProfileExtremum& point : profile.highLowPoints()) {
        out << "  " << (point.high ? "high point" : "low point ") << " at " << point.station
            << " @ " << point.elevation << "\n";
    }
    std::string text = out.str();
    text.pop_back();
    return text;
}

} // namespace katana::cad
