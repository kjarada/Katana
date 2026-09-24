#include "katana/cad/plot.hpp"

#include <algorithm>
#include <cmath>
#include <string>

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

PaperDimensions paperDimensions(PaperSize size, bool landscape)
{
    // ISO 216:2007 Table 1. Each size is the previous halved along its long
    // side; A0 is one square metre, rounded to the millimetre.
    PaperDimensions portrait;
    switch (size) {
    case PaperSize::A0:
        portrait = {841.0, 1189.0};
        break;
    case PaperSize::A1:
        portrait = {594.0, 841.0};
        break;
    case PaperSize::A2:
        portrait = {420.0, 594.0};
        break;
    case PaperSize::A3:
        portrait = {297.0, 420.0};
        break;
    case PaperSize::A4:
        portrait = {210.0, 297.0};
        break;
    }
    return landscape ? PaperDimensions{portrait.heightMm, portrait.widthMm} : portrait;
}

namespace {

// The area the drawing may occupy, in millimetres; nullopt when the margins
// eat the whole sheet.
struct Printable {
    double widthMm = 0.0;
    double heightMm = 0.0;
};

core::Result<Printable> printableArea(const PlotSettings& settings)
{
    if (!std::isfinite(settings.marginMm) || settings.marginMm < 0.0) {
        return makeError(ErrorCode::InvalidArgument, "the margin must be zero or positive");
    }
    const PaperDimensions paper = paperDimensions(settings.paper, settings.landscape);
    const Printable area{paper.widthMm - 2.0 * settings.marginMm,
                         paper.heightMm - 2.0 * settings.marginMm};
    if (!(area.widthMm > 0.0) || !(area.heightMm > 0.0)) {
        return makeError(ErrorCode::InvalidArgument,
                         "the margins leave no printable area on the sheet",
                         std::to_string(settings.marginMm) + " mm on " +
                             std::to_string(paper.widthMm) + " x " +
                             std::to_string(paper.heightMm));
    }
    return area;
}

} // namespace

Result<Sheet> sheetFor(const PlotSettings& settings)
{
    if (!(settings.dpi > 0.0) || !std::isfinite(settings.dpi)) {
        return makeError(ErrorCode::InvalidArgument, "the resolution must be positive");
    }
    if (!(settings.scaleDenominator > 0.0) || !std::isfinite(settings.scaleDenominator)) {
        return makeError(ErrorCode::InvalidArgument, "the scale denominator must be positive");
    }
    if (!std::isfinite(settings.center.x) || !std::isfinite(settings.center.y)) {
        return makeError(ErrorCode::InvalidArgument, "the sheet centre must be finite");
    }
    if (auto area = printableArea(settings); !area) {
        return area.error();
    }
    const PaperDimensions paper = paperDimensions(settings.paper, settings.landscape);
    Sheet sheet;
    sheet.pixelsPerMillimetre = millimetresToPixels(1.0, settings.dpi);
    sheet.widthPixels = paper.widthMm * sheet.pixelsPerMillimetre;
    sheet.heightPixels = paper.heightMm * sheet.pixelsPerMillimetre;
    sheet.view.center = settings.center;
    // One metre is 1000 / N millimetres on the paper, and each of those is
    // pixelsPerMillimetre device pixels.
    sheet.view.scale = (1000.0 / settings.scaleDenominator) * sheet.pixelsPerMillimetre;
    sheet.view.resize(sheet.widthPixels, sheet.heightPixels);
    return sheet;
}

entity::Color paperColour(entity::Color colour, const PlotSettings& settings)
{
    const bool nearWhite = colour.r >= kNearWhiteChannel && colour.g >= kNearWhiteChannel &&
                           colour.b >= kNearWhiteChannel;
    if (settings.whiteToBlack && nearWhite) {
        colour.r = 0;
        colour.g = 0;
        colour.b = 0;
    }
    return colour;
}

Result<double> fitScale(const geometry::Box2& extent, const PlotSettings& settings)
{
    const double widthM = extent.max.x - extent.min.x;
    const double heightM = extent.max.y - extent.min.y;
    // A line along one axis - a centreline, a row of points on one northing -
    // has no height, and its zero dimension simply asks nothing of the sheet
    // (audit CAD-15), as ViewTransform::fit treats it. A box with no size in
    // either direction - nothing, or one point - has no scale to find.
    if (!(widthM >= 0.0) || !(heightM >= 0.0) || !(std::max(widthM, heightM) > 0.0) ||
        !std::isfinite(widthM) || !std::isfinite(heightM)) {
        return makeError(ErrorCode::InvalidArgument, "the drawing has no extent to fit");
    }
    auto area = printableArea(settings);
    if (!area) {
        return area.error();
    }
    // The denominator at which each dimension just fits: metres to
    // millimetres, over the millimetres available.
    const double needed =
        std::max(widthM * 1000.0 / area->widthMm, heightM * 1000.0 / area->heightMm);
    for (const double standard : kStandardScales) {
        if (standard >= needed) {
            return standard;
        }
    }
    return needed;
}

Result<double> sheetScaleAtLeast(double needed)
{
    if (!(needed > 0.0) || !std::isfinite(needed)) {
        return makeError(ErrorCode::InvalidArgument, "the scale denominator must be positive",
                         std::to_string(needed));
    }
    for (const double standard : kSheetScales) {
        if (standard >= needed) {
            return standard;
        }
    }
    return needed;
}

} // namespace katana::cad
