#pragma once

// Arranging a project's sheets (docs/plotting.md, "Arranging a sheet"): the
// functions of arrange.hpp, each made ONE undoable step on a document through
// editSheet, editViewport or editSheetSet (sheet_commands.hpp). What the
// sheet editor's Arrange menu calls, and what a command-line verb calls: the
// same step, the same words in the history, whoever asked.
//
// A function that needs to know what a view shows takes it as `content`,
// world points (arrange.hpp). Left empty, it is gathered from the document's
// drawing (viewportContent); the editor passes its own, which adds the
// window's reference layers and meshes. An edit that changes nothing records
// no step; a refused one changes nothing.

#include <cstddef>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/arrange.hpp"
#include "katana/core/error.hpp"

namespace katana::cad::plotting {

// The step names these record, for a history to show and a test to find.
inline constexpr std::string_view kAutoArrangeStep = "AUTO_ARRANGE";
inline constexpr std::string_view kAlignStep = "ALIGN_VIEWPORTS";
inline constexpr std::string_view kDistributeStep = "DISTRIBUTE_VIEWPORTS";
inline constexpr std::string_view kMatchScaleStep = "MATCH_SCALE";
inline constexpr std::string_view kFitToContentStep = "FIT_VIEWPORT_TO_CONTENT";
inline constexpr std::string_view kRotateToFitStep = "ROTATE_TO_BEST_FIT";
inline constexpr std::string_view kChoosePaperStep = "CHOOSE_PAPER";

// autoArrange on sheet `sheetIndex`. NotFound for an index past the end.
[[nodiscard]] core::Result<ArrangeResult> autoArrangeSheet(Document& document,
                                                           std::size_t sheetIndex);

// alignViewports and distributeViewports on sheet `sheetIndex`: the ids that
// moved. Errors as those, and NotFound for an index past the end.
[[nodiscard]] core::Result<std::vector<std::string>>
alignViewports(Document& document, std::size_t sheetIndex, std::span<const std::string> ids,
               AlignEdge edge);
[[nodiscard]] core::Result<std::vector<std::string>>
distributeViewports(Document& document, std::size_t sheetIndex, std::span<const std::string> ids,
                    DistributeAxis axis);

// matchScale across the set: the ids whose scale changed. `fromScale` (and
// for a section `fromExaggeration`) is what an automatic `fromId` is drawn
// at, when the caller knows it (the editor does); without it an automatic
// plan's scale is worked out from what it shows, as the painter works it
// out, and any other view's own is used.
[[nodiscard]] core::Result<std::vector<std::string>>
matchScale(Document& document, std::span<const std::string> ids, std::string_view fromId,
           std::optional<double> fromScale = std::nullopt,
           std::optional<double> fromExaggeration = std::nullopt);

// The scale viewport `viewport` is drawn at: its own, or for an automatic
// plan or key plan the first of kSheetScales at which `content` fits its
// rectangle with kAutoScaleSpare to spare, turned as it is turned and
// measured about the point it is drawn round (the content's middle with an
// automatic centre, else the view's centre) - the painter's rule: a view
// along an alignment by `content` itself, any other by the four corners of
// `content`'s box, as the painter measures the drawing's box.
[[nodiscard]] double drawnScale(const Viewport& viewport, std::span<const Point2> content);

// fitViewportToContent on viewport `viewportId`, wherever it is, inside its
// sheet's drawing area, at `scale` when given (the scale an automatic view is
// drawn at) and else at drawnScale. Errors as fitViewportToContent; NotFound
// for an unknown id.
[[nodiscard]] core::Status fitViewportToContent(Document& document, std::string_view viewportId,
                                                std::span<const Point2> content = {},
                                                std::optional<double> scale = std::nullopt);

// rotateToBestFit on viewport `viewportId`: the fit it chose. Errors as
// rotateToBestFit; NotFound for an unknown id.
[[nodiscard]] core::Result<RotationFit> rotateToBestFit(Document& document,
                                                        std::string_view viewportId,
                                                        std::span<const Point2> content = {});

// fitPaperToViewport on the sheet viewport `viewportId` is on, at `scale`
// when given and else at drawnScale: "Choose paper for this scale". Errors as
// fitPaperToViewport; NotFound for an unknown id.
[[nodiscard]] core::Result<PaperChange>
choosePaperForScale(Document& document, std::string_view viewportId,
                    std::span<const Point2> content = {},
                    std::optional<double> scale = std::nullopt);

// The main view of sheet `sheetIndex` that "Choose paper for this scale"
// acts on: its first plan, else its first key plan. Empty when it has neither
// or there is no such sheet.
[[nodiscard]] std::string mainPlanOf(const SheetSet& set, std::size_t sheetIndex);

} // namespace katana::cad::plotting
