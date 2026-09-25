#include "katana/cad/plotting/viewport_edits.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <iterator>
#include <set>
#include <utility>

namespace katana::cad::plotting {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::geometry::Point2;

namespace {

Status noSheet(std::size_t index, std::size_t count)
{
    return makeError(ErrorCode::NotFound, "no sheet at that position",
                     std::to_string(index) + " of " + std::to_string(count));
}

// `ids` as a set, each checked to be on `sheet`: the ids an edit applies to.
Result<std::set<std::string, std::less<>>> idsOn(const Sheet& sheet,
                                                 std::span<const std::string> ids)
{
    if (ids.empty()) {
        return makeError(ErrorCode::InvalidArgument, "no viewports given");
    }
    std::set<std::string, std::less<>> wanted;
    for (const std::string& id : ids) {
        const bool present = std::ranges::any_of(
            sheet.viewports, [&id](const Viewport& viewport) { return viewport.id == id; });
        if (!present) {
            return makeError(ErrorCode::NotFound, "no viewport with that id on the sheet",
                             id + " on " + sheet.id);
        }
        wanted.insert(id);
    }
    return wanted;
}

// The smallest move of `own` (a rectangle's edges on one axis) that puts one
// of them on a line of the grid.
double gridShift(std::span<const double> own, double gridMm)
{
    double best = 0.0;
    bool found = false;
    for (const double edge : own) {
        const double shift = snapToGrid(edge, gridMm) - edge;
        if (!found || std::abs(shift) < std::abs(best)) {
            best = shift;
            found = true;
        }
    }
    return best;
}

Box2 moved(const Box2& box, const Point2& delta)
{
    return Box2(box.min + delta, box.max + delta);
}

} // namespace

// ---- the paper grid ----------------------------------------------------------------

double snapToGrid(double value, double spacingMm)
{
    if (!(spacingMm > 0.0) || !std::isfinite(value)) {
        return value;
    }
    return std::floor(value / spacingMm + 0.5) * spacingMm;
}

SnapResult snapMovingRect(const Box2& moving, const Box2& drawingArea,
                          std::span<const Box2> others, double toleranceMm, double gridMm)
{
    SnapResult result = snapRect(moving, drawingArea, others, toleranceMm);
    if (!(gridMm > 0.0) || moving.empty()) {
        return result;
    }
    // Only where no edge or centre line was in reach: a panel lined up with
    // its neighbour stays lined up, whether or not the neighbour is on the
    // grid.
    if (!result.guideX) {
        const std::array<double, 2> own{result.rect.min.x, result.rect.max.x};
        const double shift = gridShift(own, gridMm);
        result.rect.min.x += shift;
        result.rect.max.x += shift;
    }
    if (!result.guideY) {
        const std::array<double, 2> own{result.rect.min.y, result.rect.max.y};
        const double shift = gridShift(own, gridMm);
        result.rect.min.y += shift;
        result.rect.max.y += shift;
    }
    return result;
}

EdgeSnap snapEdge(double value, std::span<const double> targets, double toleranceMm, double gridMm)
{
    EdgeSnap result{value, std::nullopt};
    if (toleranceMm > 0.0) {
        double bestDistance = toleranceMm;
        for (const double target : targets) {
            const double distance = std::abs(target - value);
            // The nearest wins; of two as near, the first given.
            if (distance < bestDistance || (distance == bestDistance && !result.guide)) {
                bestDistance = distance;
                result.value = target;
                result.guide = target;
            }
        }
    }
    if (!result.guide) {
        result.value = snapToGrid(value, gridMm);
    }
    return result;
}

// ---- picking -----------------------------------------------------------------------

std::vector<std::string> viewportsInBand(const Sheet& sheet, const Box2& band, BandMode mode)
{
    std::vector<std::string> picked;
    if (band.empty()) {
        return picked;
    }
    for (const Viewport& viewport : sheet.viewports) {
        if (viewport.rect.empty()) {
            continue;
        }
        const bool takes = mode == BandMode::Window ? band.contains(viewport.rect)
                                                    : band.intersects(viewport.rect);
        if (takes) {
            picked.push_back(viewport.id);
        }
    }
    return picked;
}

Box2 viewportBounds(const Sheet& sheet, std::span<const std::string> ids)
{
    Box2 bounds;
    for (const Viewport& viewport : sheet.viewports) {
        if (std::ranges::find(ids, viewport.id) != ids.end()) {
            bounds.expand(viewport.rect);
        }
    }
    return bounds;
}

std::string cycleViewport(const Sheet& sheet, std::string_view current, bool forward)
{
    std::vector<const Viewport*> placed;
    for (const Viewport& viewport : sheet.viewports) {
        if (!viewport.rect.empty()) {
            placed.push_back(&viewport);
        }
    }
    if (placed.empty()) {
        return {};
    }
    const auto at = std::ranges::find_if(
        placed, [current](const Viewport* viewport) { return viewport->id == current; });
    if (at == placed.end()) {
        return forward ? placed.front()->id : placed.back()->id;
    }
    const auto index = static_cast<std::size_t>(std::distance(placed.begin(), at));
    const std::size_t count = placed.size();
    return placed[forward ? (index + 1) % count : (index + count - 1) % count]->id;
}

// ---- a plan's paper and its world --------------------------------------------------

Point2 planPaperToWorld(const Viewport& viewport, double scale, const Point2& centre,
                        const Point2& paper)
{
    // The painter draws world point w at middle + rotate(w - centre, -rotation)
    // * (1000 / scale) millimetres; this is that, solved for w.
    const Point2 offset = paper - viewport.rect.center();
    return centre + offset.rotated(viewport.rotation) * (scale / 1000.0);
}

Point2 planWorldToPaper(const Viewport& viewport, double scale, const Point2& centre,
                        const Point2& world)
{
    return viewport.rect.center() + (world - centre).rotated(-viewport.rotation) * (1000.0 / scale);
}

// ---- the edits ---------------------------------------------------------------------

Status moveViewports(Document& document, std::size_t sheetIndex, std::span<const std::string> ids,
                     Point2 deltaMm, std::string stepName)
{
    SheetSet set = document.sheetSet();
    if (sheetIndex >= set.sheets.size()) {
        return noSheet(sheetIndex, set.sheets.size());
    }
    if (!deltaMm.isFinite()) {
        return makeError(ErrorCode::InvalidArgument, "the move is not a finite distance");
    }
    Sheet& sheet = set.sheets[sheetIndex];
    auto wanted = idsOn(sheet, ids);
    if (!wanted) {
        return wanted.error();
    }
    for (Viewport& viewport : sheet.viewports) {
        if (wanted->contains(viewport.id) && !viewport.locked && !viewport.rect.empty()) {
            viewport.rect = moved(viewport.rect, deltaMm);
        }
    }
    return document.setSheetSet(set, std::move(stepName));
}

Status removeViewports(Document& document, std::size_t sheetIndex, std::span<const std::string> ids,
                       std::string stepName)
{
    SheetSet set = document.sheetSet();
    if (sheetIndex >= set.sheets.size()) {
        return noSheet(sheetIndex, set.sheets.size());
    }
    Sheet& sheet = set.sheets[sheetIndex];
    auto wanted = idsOn(sheet, ids);
    if (!wanted) {
        return wanted.error();
    }
    std::erase_if(sheet.viewports,
                  [&wanted](const Viewport& viewport) { return wanted->contains(viewport.id); });
    return document.setSheetSet(set, std::move(stepName));
}

Result<std::vector<Viewport>> copyViewports(const SheetSet& set, std::size_t sheetIndex,
                                            std::span<const std::string> ids)
{
    if (sheetIndex >= set.sheets.size()) {
        return noSheet(sheetIndex, set.sheets.size()).error();
    }
    const Sheet& sheet = set.sheets[sheetIndex];
    auto wanted = idsOn(sheet, ids);
    if (!wanted) {
        return wanted.error();
    }
    std::vector<Viewport> copies;
    for (const Viewport& viewport : sheet.viewports) {
        if (wanted->contains(viewport.id)) {
            copies.push_back(viewport);
        }
    }
    return copies;
}

Point2 pasteOffset(const Sheet& sheet, std::span<const Viewport> viewports, double stepMm)
{
    const auto taken = [&sheet](const Box2& rect) {
        return std::ranges::any_of(sheet.viewports,
                                   [&rect](const Viewport& on) { return on.rect == rect; });
    };
    // Bounded by the sheet: each step can only be blocked by a viewport that
    // is on it, so one more step than there are viewports is always free.
    for (std::size_t step = 0; step <= sheet.viewports.size(); ++step) {
        const double d = static_cast<double>(step) * stepMm;
        const Point2 offset(d, -d);
        const bool blocked = std::ranges::any_of(viewports, [&](const Viewport& pasted) {
            return !pasted.rect.empty() && taken(moved(pasted.rect, offset));
        });
        if (!blocked) {
            return offset;
        }
    }
    const double d = static_cast<double>(sheet.viewports.size() + 1) * stepMm;
    return Point2(d, -d);
}

Result<std::vector<std::string>> pasteViewports(Document& document, std::size_t sheetIndex,
                                                std::vector<Viewport> viewports,
                                                std::optional<Point2> offsetMm, std::string stepName)
{
    if (viewports.empty()) {
        return makeError(ErrorCode::InvalidArgument, "nothing to paste");
    }
    SheetSet set = document.sheetSet();
    if (sheetIndex >= set.sheets.size()) {
        return noSheet(sheetIndex, set.sheets.size()).error();
    }
    Sheet& sheet = set.sheets[sheetIndex];
    const Point2 offset = offsetMm.value_or(pasteOffset(sheet, viewports));
    if (!offset.isFinite()) {
        return makeError(ErrorCode::InvalidArgument, "the paste offset is not a finite distance");
    }
    std::vector<std::string> ids = newViewportIds(set, viewports.size());
    for (std::size_t i = 0; i < viewports.size(); ++i) {
        Viewport& pasted = viewports[i];
        pasted.id = ids[i];
        if (!pasted.rect.empty()) {
            pasted.rect = moved(pasted.rect, offset);
        }
        sheet.viewports.push_back(std::move(pasted));
    }
    if (Status status = document.setSheetSet(set, std::move(stepName)); !status) {
        return status.error();
    }
    return ids;
}

Result<std::vector<std::string>> duplicateViewports(Document& document, std::size_t sheetIndex,
                                                    std::span<const std::string> ids,
                                                    std::string stepName)
{
    auto copies = copyViewports(document.sheetSet(), sheetIndex, ids);
    if (!copies) {
        return copies.error();
    }
    return pasteViewports(document, sheetIndex, std::move(*copies), std::nullopt,
                          std::move(stepName));
}

} // namespace katana::cad::plotting
