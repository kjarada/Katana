#include "katana/cad/plotting/section_fit.hpp"

#include <algorithm>
#include <cmath>
#include <format>

#include "katana/cad/plot.hpp"

namespace katana::cad::plotting {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::Point2;

namespace {

// A choice exactly on a boundary - a span that needs 1 : 500 to the last bit
// - is not pushed to the next step by the rounding of the arithmetic that
// found it.
constexpr double kRelativeSlack = 1e-9;

// The room a level label needs beside the plot: 0.8 mm from the plot's edge
// to the text, and as much again from the text to the viewport's.
constexpr double kLevelLabelClearanceMm = 1.6;
// The room a section keeps above and right of its plot.
constexpr double kPlotMarginMm = 2.0;
// A plot smaller than this each way has nothing worth drawing in it.
constexpr double kMinimumPlotWidthMm = 10.0;
constexpr double kMinimumPlotHeightMm = 8.0;
// Never narrower than this left of the plot, however short the levels.
constexpr double kMinimumLeftMm = 6.0;

} // namespace

double sectionSpan(double low, double high, std::optional<double> centre)
{
    if (!(high >= low) || !std::isfinite(low) || !std::isfinite(high)) {
        return 0.0;
    }
    if (centre && std::isfinite(*centre)) {
        return 2.0 * std::max(std::abs(high - *centre), std::abs(*centre - low));
    }
    return high - low;
}

double fitExaggeration(double depthM, double scale, double heightMm, double flatExaggeration)
{
    if (!(depthM > 0.0) || !std::isfinite(depthM)) {
        return flatExaggeration;
    }
    // The largest that fits, among those that give a vertical scale a rule
    // reads - a whole denominator (1 : 750 at 2.5 is V 1:300, at 4 would be
    // V 1:187.5) - else the largest that fits at all.
    double best = kSectionExaggerations.front();
    std::optional<double> whole;
    for (const double candidate : kSectionExaggerations) {
        if (depthM * 1000.0 * candidate / scale <= heightMm) {
            best = candidate;
            const double vertical = scale / candidate;
            if (std::abs(vertical - std::round(vertical)) <= 1e-9 * vertical) {
                whole = candidate;
            }
        }
    }
    return whole ? *whole : best;
}

Result<SectionFit> fitSection(const SectionFitRequest& request)
{
    if (!(request.spanM > 0.0) || !std::isfinite(request.spanM)) {
        return makeError(ErrorCode::InvalidArgument, "a section needs a length to fit",
                         std::format("{} m", request.spanM));
    }
    if (!(request.plotWidthMm > 0.0) || !(request.plotHeightMm > 0.0) ||
        !std::isfinite(request.plotWidthMm) || !std::isfinite(request.plotHeightMm)) {
        return makeError(ErrorCode::InvalidArgument, "the section's plot has no room",
                         std::format("{} x {} mm", request.plotWidthMm, request.plotHeightMm));
    }
    if (!(request.fill > 0.0) || request.fill > 1.0) {
        return makeError(ErrorCode::InvalidArgument, "the fill must be above 0 and at most 1",
                         std::format("{}", request.fill));
    }
    if (!(request.depthM >= 0.0) || !std::isfinite(request.depthM)) {
        return makeError(ErrorCode::InvalidArgument, "the depth must not be negative",
                         std::format("{} m", request.depthM));
    }
    const double width = request.plotWidthMm * request.fill;
    const double height = request.plotHeightMm * request.fill;
    // Across, and - at true scale - up: a deep narrow section is drawn
    // smaller rather than over its frame.
    double needed = request.spanM * 1000.0 / width;
    if (request.depthM > 0.0) {
        needed = std::max(needed, request.depthM * 1000.0 / height);
    }
    auto scale = sheetScaleAtLeast(needed * (1.0 - kRelativeSlack));
    if (!scale) {
        return scale.error();
    }
    SectionFit fit;
    fit.scale = *scale;
    fit.exaggeration = fitExaggeration(request.depthM, fit.scale, height * (1.0 + kRelativeSlack),
                                       request.flatExaggeration);
    return fit;
}

// ---- label steps ---------------------------------------------------------------------

double roundStepAtLeast(double minimum)
{
    if (!(minimum > 0.0) || !std::isfinite(minimum)) {
        return 1.0;
    }
    const double magnitude = std::pow(10.0, std::floor(std::log10(minimum)));
    for (const double multiple : {1.0, 2.0, 5.0, 10.0}) {
        if (magnitude * multiple >= minimum * (1.0 - kRelativeSlack)) {
            return magnitude * multiple;
        }
    }
    return magnitude * 10.0;
}

double nextRoundStep(double step)
{
    if (!(step > 0.0) || !std::isfinite(step)) {
        return 1.0;
    }
    const double magnitude = std::pow(10.0, std::floor(std::log10(step) + kRelativeSlack));
    const double multiple = step / magnitude;
    if (multiple < 1.5) {
        return 2.0 * magnitude;
    }
    if (multiple < 3.5) {
        return 5.0 * magnitude;
    }
    return 10.0 * magnitude;
}

int stepDecimals(double step)
{
    if (!(step > 0.0) || !std::isfinite(step)) {
        return 0;
    }
    for (int decimals = 0; decimals < 3; ++decimals) {
        const double scaled = step * std::pow(10.0, decimals);
        if (std::abs(scaled - std::round(scaled)) <= 1e-6 * std::max(1.0, scaled)) {
            return decimals;
        }
    }
    return 3;
}

std::string stepText(double value, double step)
{
    const int decimals = stepDecimals(step);
    // A value that rounds to zero is written "0", never "-0".
    if (std::abs(value) < 0.5 * std::pow(10.0, -decimals)) {
        value = 0.0;
    }
    return std::format("{:.{}f}", value, decimals);
}

std::vector<double> gridValues(double first, double last, double step, std::size_t cap)
{
    std::vector<double> values;
    if (!(step > 0.0) || !(last >= first) || !std::isfinite(first) || !std::isfinite(last)) {
        return values;
    }
    // Counted from a whole number of steps, so the thousandth value is not
    // a thousand additions' rounding away from its multiple.
    const double firstIndex = std::ceil(first / step - kRelativeSlack);
    for (std::size_t i = 0; values.size() < cap; ++i) {
        const double value = (firstIndex + static_cast<double>(i)) * step;
        if (value > last + step * kRelativeSlack) {
            break;
        }
        values.push_back(value);
    }
    return values;
}

double labelStep(double mmPerUnit, double minimumSpacingMm, double gapMm,
                 const std::function<double(double)>& widestLabelMm)
{
    if (!(mmPerUnit > 0.0) || !std::isfinite(mmPerUnit)) {
        return 1.0;
    }
    double step = roundStepAtLeast(minimumSpacingMm / mmPerUnit);
    // Sixty steps is eighteen decades: far past any label a sheet can hold.
    for (int tried = 0; tried < 60; ++tried) {
        const double widest = widestLabelMm ? widestLabelMm(step) : 0.0;
        if (step * mmPerUnit >= (widest + gapMm) * (1.0 - kRelativeSlack)) {
            return step;
        }
        step = nextRoundStep(step);
    }
    return step;
}

// ---- the plot's layout ----------------------------------------------------------------

SectionLayout sectionPlotLayout(const SectionLayoutRequest& request)
{
    SectionLayout layout;
    const Box2& area = request.area;
    if (area.empty()) {
        return layout;
    }
    const double levels = std::max(request.levelLabelMm + kLevelLabelClearanceMm, kMinimumLeftMm);
    const double caption = request.caption ? kSectionCaptionMm : 0.0;
    const auto plotWith = [&](double left, double bottom) {
        return Box2(Point2(area.min.x + left, area.min.y + bottom + 1.0),
                    Point2(area.max.x - kPlotMarginMm, area.max.y - kPlotMarginMm));
    };
    if (request.bandRows > 0) {
        // The band's row names share the column left of the plot with the
        // level labels, so it is as wide as the wider.
        const double left = std::max(kSectionBandHeadMm, levels);
        const Box2 plot =
            plotWith(left, static_cast<double>(request.bandRows) * kSectionBandRowMm + caption);
        if (plot.height() >= kSectionMinimumBandPlotMm) {
            layout.plot = plot;
            layout.banded = true;
            layout.leftMm = left;
        }
    }
    if (!layout.banded) {
        layout.plot = plotWith(levels, kSectionAxisRowMm + caption);
        layout.leftMm = levels;
    }
    if (!(layout.plot.width() > kMinimumPlotWidthMm) ||
        !(layout.plot.height() > kMinimumPlotHeightMm)) {
        return {};
    }
    return layout;
}

// ---- a section viewport ----------------------------------------------------------------

Box2 sectionDrawingArea(const Box2& rect)
{
    if (rect.empty()) {
        return rect;
    }
    Box2 area = rect;
    area.min.y = std::min(area.min.y + kSectionTitleMm, area.max.y);
    return area;
}

std::vector<double> viewportStations(const ViewportSource& source)
{
    std::vector<double> stations = source.stations;
    if (stations.empty() && source.sectionInterval > 0.0 && source.chainageTo > source.chainageFrom) {
        // Counted in whole intervals, so the last is not an accumulation of
        // additions' rounding short of the range's end.
        for (std::size_t i = 0; stations.size() < kMaximumSectionRows; ++i) {
            const double at = source.chainageFrom + static_cast<double>(i) * source.sectionInterval;
            if (at > source.chainageTo + 1e-9) {
                break;
            }
            stations.push_back(at);
        }
    }
    return stations;
}

double viewportHalfWidth(const ViewportSource& source)
{
    return source.sectionHalfWidth > 0.0 && std::isfinite(source.sectionHalfWidth)
               ? source.sectionHalfWidth
               : kDefaultSectionHalfWidth;
}

} // namespace katana::cad::plotting
