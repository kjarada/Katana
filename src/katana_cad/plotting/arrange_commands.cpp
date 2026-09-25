#include "katana/cad/plotting/arrange_commands.hpp"

#include <algorithm>
#include <cmath>
#include <utility>

#include "katana/cad/plot.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"

namespace katana::cad::plotting {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

bool isPlanKind(ViewportKind kind)
{
    return kind == ViewportKind::Plan || kind == ViewportKind::KeyPlan;
}

bool isSectionKind(ViewportKind kind)
{
    return kind == ViewportKind::LongSection || kind == ViewportKind::CrossSections;
}

// The viewport `id`, and the index of the sheet it is on.
struct Found {
    const Viewport* viewport = nullptr;
    std::size_t sheet = 0;
};

Result<Found> locate(const SheetSet& set, std::string_view id)
{
    for (std::size_t s = 0; s < set.sheets.size(); ++s) {
        for (const Viewport& viewport : set.sheets[s].viewports) {
            if (viewport.id == id) {
                return Found{&viewport, s};
            }
        }
    }
    return makeError(ErrorCode::NotFound, "no viewport with that id", std::string(id));
}

// `content`, or when it is empty what the viewport shows of the document's
// drawing. A section's content is the ground it cuts, which only a caller
// that has cut it can give.
Result<std::vector<Point2>> contentFor(const Document& document, const Viewport& viewport,
                                       std::span<const Point2> content)
{
    if (!content.empty()) {
        return std::vector<Point2>(content.begin(), content.end());
    }
    if (isSectionKind(viewport.kind)) {
        return makeError(ErrorCode::InvalidArgument,
                         "a section's content is the ground it cuts: give its (chainage or "
                         "offset, level) points",
                         viewport.id);
    }
    return viewportContent(document.model(), document.sheetSet(), viewport);
}

} // namespace

Result<ArrangeResult> autoArrangeSheet(Document& document, std::size_t sheetIndex)
{
    ArrangeResult result;
    const Status status = editSheet(
        document, sheetIndex,
        [&result](Sheet& sheet) {
            result = autoArrange(sheet);
            return Status{};
        },
        std::string(kAutoArrangeStep));
    if (!status) {
        return status.error();
    }
    return result;
}

Result<std::vector<std::string>> alignViewports(Document& document, std::size_t sheetIndex,
                                                std::span<const std::string> ids, AlignEdge edge)
{
    std::vector<std::string> moved;
    const Status status = editSheet(
        document, sheetIndex,
        [&](Sheet& sheet) -> Status {
            auto aligned = alignViewports(sheet, ids, edge);
            if (!aligned) {
                return aligned.error();
            }
            moved = std::move(*aligned);
            return {};
        },
        std::string(kAlignStep));
    if (!status) {
        return status.error();
    }
    return moved;
}

Result<std::vector<std::string>> distributeViewports(Document& document, std::size_t sheetIndex,
                                                     std::span<const std::string> ids,
                                                     DistributeAxis axis)
{
    std::vector<std::string> moved;
    const Status status = editSheet(
        document, sheetIndex,
        [&](Sheet& sheet) -> Status {
            auto spaced = distributeViewports(sheet, ids, axis);
            if (!spaced) {
                return spaced.error();
            }
            moved = std::move(*spaced);
            return {};
        },
        std::string(kDistributeStep));
    if (!status) {
        return status.error();
    }
    return moved;
}

Result<std::vector<std::string>> matchScale(Document& document, std::span<const std::string> ids,
                                            std::string_view fromId,
                                            std::optional<double> fromScale)
{
    if (!fromScale) {
        // An automatic plan is matched at the scale it is drawn at, not at
        // the last one stored.
        if (auto from = locate(document.sheetSet(), fromId);
            from && from->viewport->autoScale && isPlanKind(from->viewport->kind)) {
            fromScale = drawnScale(*from->viewport,
                                   viewportContent(document.model(), document.sheetSet(),
                                                   *from->viewport));
        }
    }
    std::vector<std::string> changed;
    const Status status = editSheetSet(
        document,
        [&](SheetSet& set) -> Status {
            auto matched = matchScale(set, ids, fromId, fromScale);
            if (!matched) {
                return matched.error();
            }
            changed = std::move(*matched);
            return {};
        },
        std::string(kMatchScaleStep));
    if (!status) {
        return status.error();
    }
    return changed;
}

double drawnScale(const Viewport& viewport, std::span<const Point2> content)
{
    if (!viewport.autoScale || !isPlanKind(viewport.kind) || viewport.rect.empty()) {
        return viewport.scale;
    }
    // The painter's rule (resolvePlanViewport): the content measured about
    // the point it is drawn round - its own middle with an automatic centre,
    // else the view's centre - turned as the view turns it, with
    // kAutoScaleSpare to spare. The painter measures a stretch of alignment
    // by its points but the drawing by the four corners of its box, which a
    // turned view needs more room for than the drawing's own outline.
    std::vector<Point2> corners;
    if (viewport.source.alignment.empty()) {
        Box2 box;
        for (const Point2& point : content) {
            box.expand(point);
        }
        if (!box.empty()) {
            corners = {box.min, Point2(box.max.x, box.min.y), box.max, Point2(box.min.x, box.max.y)};
            content = corners;
        }
    }
    double width = 0.0;
    double height = 0.0;
    if (viewport.autoCentre) {
        const auto fit = fitAtRotation(content, {1.0, 1.0}, viewport.rotation);
        if (!fit) {
            return viewport.scale;
        }
        width = fit->width;
        height = fit->height;
    } else {
        for (const Point2& point : content) {
            const geometry::Vec2 d = (point - viewport.centre).rotated(-viewport.rotation);
            width = std::max(width, 2.0 * std::abs(d.x));
            height = std::max(height, 2.0 * std::abs(d.y));
        }
    }
    const double needed = std::max(width * 1000.0 / viewport.rect.width(),
                                   height * 1000.0 / viewport.rect.height()) *
                          kAutoScaleSpare;
    if (!(needed > 0.0) || !std::isfinite(needed)) {
        return viewport.scale;
    }
    return sheetScaleAtLeast(needed).valueOr(viewport.scale);
}

Status fitViewportToContent(Document& document, std::string_view viewportId,
                            std::span<const Point2> content, std::optional<double> scale)
{
    const SheetSet& set = document.sheetSet();
    auto found = locate(set, viewportId);
    if (!found) {
        return found.error();
    }
    auto points = contentFor(document, *found->viewport, content);
    if (!points) {
        return points.error();
    }
    const double drawn = scale.value_or(drawnScale(*found->viewport, *points));
    const Box2 area = drawingArea(set.sheets[found->sheet]);
    return editViewport(
        document, viewportId,
        [&](Viewport& viewport) {
            viewport.scale = drawn;
            return fitViewportToContent(viewport, *points, area);
        },
        std::string(kFitToContentStep));
}

Result<RotationFit> rotateToBestFit(Document& document, std::string_view viewportId,
                                    std::span<const Point2> content)
{
    auto found = locate(document.sheetSet(), viewportId);
    if (!found) {
        return found.error();
    }
    auto points = contentFor(document, *found->viewport, content);
    if (!points) {
        return points.error();
    }
    RotationFit chosen;
    const Status status = editViewport(
        document, viewportId,
        [&](Viewport& viewport) -> Status {
            auto fit = rotateToBestFit(viewport, *points);
            if (!fit) {
                return fit.error();
            }
            chosen = *fit;
            return {};
        },
        std::string(kRotateToFitStep));
    if (!status) {
        return status.error();
    }
    return chosen;
}

Result<PaperChange> choosePaperForScale(Document& document, std::string_view viewportId,
                                        std::span<const Point2> content,
                                        std::optional<double> scale)
{
    auto found = locate(document.sheetSet(), viewportId);
    if (!found) {
        return found.error();
    }
    auto points = contentFor(document, *found->viewport, content);
    if (!points) {
        return points.error();
    }
    const double drawn = scale.value_or(drawnScale(*found->viewport, *points));
    PaperChange change;
    const Status status = editSheet(
        document, found->sheet,
        [&](Sheet& sheet) -> Status {
            auto fitted = fitPaperToViewport(sheet, viewportId, *points, drawn);
            if (!fitted) {
                return fitted.error();
            }
            change = std::move(*fitted);
            return {};
        },
        std::string(kChoosePaperStep));
    if (!status) {
        return status.error();
    }
    return change;
}

std::string mainPlanOf(const SheetSet& set, std::size_t sheetIndex)
{
    if (sheetIndex >= set.sheets.size()) {
        return {};
    }
    for (const ViewportKind kind : {ViewportKind::Plan, ViewportKind::KeyPlan}) {
        for (const Viewport& viewport : set.sheets[sheetIndex].viewports) {
            if (viewport.kind == kind && !viewport.rect.empty()) {
                return viewport.id;
            }
        }
    }
    return {};
}

} // namespace katana::cad::plotting
