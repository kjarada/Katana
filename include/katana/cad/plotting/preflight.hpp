#pragma once

// Preflight: what would go wrong on paper, found before the paper is used
// (docs/plotting.md, "Preflight checks").
//
// checkSheets reads a sheet set, the drawing and the project's field values,
// and lists what a plot would get wrong: a view off the paper or under the
// title block, two views on top of each other, a plan over nothing, a section
// of an alignment that is not there or at a chainage off its end, text too
// small to read at the view's scale, a title block with blanks in it, a match
// line leading to a sheet that was removed. Each finding carries a stable
// code, a severity, where it is (sheet and viewport ids), what is wrong in
// words, and what to do about it.
//
// It changes nothing and plots nothing: the sheet editor lists the findings
// in its Checks dock and warns before a plot, and an agent reads them as
// JSON (findingsToJson). A plot is never refused because of them - an error
// here is paper wasted, not a file that cannot be written.
//
// Pure: the same set, drawing and options give the same findings in the same
// order - the set's own findings first, then sheet by sheet, each sheet's own
// before its viewports', back to front, each viewport's checks in the order
// preflightChecks() lists them.

#include <array>
#include <cstddef>
#include <filesystem>
#include <functional>
#include <optional>
#include <set>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/model.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/geometry/spatial_index.hpp"

namespace katana::cad {
class Document;
}

namespace katana::cad::plotting {

// How much a finding matters. An Error wastes the sheet - something is not
// drawn, is cut off or is covered; a Warning is probably not what was meant;
// Info is worth knowing. Declared worst first, so sorting by it puts errors
// at the top.
enum class Severity { Error, Warning, Info };

[[nodiscard]] std::string_view toString(Severity severity);
[[nodiscard]] std::optional<Severity> severityFrom(std::string_view name);

struct Finding {
    Severity severity = Severity::Warning;
    // Stable: "viewport.outside", "field.empty" ... (preflightChecks lists
    // them all). Never renamed - an agent and a saved report key on it.
    std::string code;
    // The sheet, by position and id; nothing for a finding about the whole
    // set (a blank title-block value every sheet prints, the logo).
    std::optional<std::size_t> sheetIndex;
    std::string sheetId;
    // The viewport, when the finding is about one.
    std::string viewportId;
    // What else the finding names: the field ("organisation"), the other
    // viewport of an overlap, the sheet a mark leads to, the alignment.
    std::string subject;
    std::string message; // what is wrong, in words
    std::string fix;     // what to do about it

    friend bool operator==(const Finding&, const Finding&) = default;
};

// One line of the catalogue: a check's code, the worst severity it reports,
// and what it looks for.
struct CheckDescription {
    std::string_view code;
    Severity severity;
    std::string_view looksFor;
};

// Every check, in the order a viewport's findings come in.
[[nodiscard]] std::span<const CheckDescription> preflightChecks();

// The scale and centre a plan viewport is drawn at once "auto" is decided.
struct PlanWindow {
    double scale = 500.0;
    geometry::Point2 centre{};

    friend bool operator==(const PlanWindow&, const PlanWindow&) = default;
};

struct PreflightOptions {
    // The sheets to check, by index; every sheet when empty. The set's own
    // checks (the title block's blanks, the logo) count only these sheets.
    std::vector<std::size_t> sheets;
    // Drawing text printing smaller than this, in paper millimetres, is too
    // small to read: 1.8 mm is the smallest lettering a drafting standard
    // allows on a sheet that may be copied or reduced.
    double minimumTextMm = 1.8;
    // Two viewports overlap when they share more than this much on both
    // axes: edges snapped together meet exactly and are not an overlap.
    double overlapToleranceMm = 0.5;
    // How far a viewport may stray past the drawing area before it is
    // outside it: the millimetre's rounding a hand-drawn rectangle has.
    double outsideToleranceMm = 0.05;
    // How an automatic plan's scale and centre are decided. The painter's own
    // rule when the caller has it (the Qt layer passes resolvePlanViewport,
    // which counts imagery and meshes too); planWindow otherwise.
    std::function<PlanWindow(const Viewport&)> resolvePlan;
    // World boxes of what a plan draws besides entities and alignments -
    // imagery, point clouds, meshes - so a plan over nothing but an
    // orthophoto is not called empty.
    std::vector<geometry::Box2> otherContent;
    // How many surfaces the sections are cut from; nothing when not known,
    // and then the sections' surfaces are not checked.
    std::optional<std::size_t> sectionSurfaces;
    // Whether the set's logo file was found and read; nothing when not known.
    std::optional<bool> logoReadable;
    // Where an Image viewport's file is looked for; empty: not looked for.
    std::filesystem::path assets;
    // A spatial index in step with the model, to find a plan's entities
    // without scanning the drawing; null scans it.
    const geometry::SpatialIndex* index = nullptr;
    // Codes not to report.
    std::set<std::string, std::less<>> skip;
};

// Everything that would go wrong plotting `set` (docs/plotting.md lists each
// check and why). `context` is the project's contribution to the title block
// (fieldContextFor builds it); `model` is what the views show.
[[nodiscard]] std::vector<Finding> checkSheets(const SheetSet& set, const entity::Model& model,
                                               const FieldContext& context,
                                               const PreflightOptions& options = {});

// The same for a document's own sheets: its drawing, its spatial index, its
// project fields, whether its logo file is there and where its images are.
// Options the caller set are kept.
[[nodiscard]] std::vector<Finding> checkDocumentSheets(const Document& document,
                                                       PreflightOptions options = {});

// A plan viewport's scale and centre, "auto" decided the painter's way from
// the model alone: the viewport's stretch of its alignment, else what the
// viewport draws (every entity its layers show, every alignment, and
// `otherContent`), fitted with 4% to spare at the first of kSheetScales.
[[nodiscard]] PlanWindow planWindow(const Viewport& viewport, const entity::Model& model,
                                    std::span<const geometry::Box2> otherContent = {});

// The world corners of what a plan viewport shows at `window`, counter-
// clockwise from its bottom-left on the paper: its rectangle at the scale,
// about the centre, turned by the viewport's rotation.
[[nodiscard]] std::array<geometry::Point2, 4> planWindowCorners(const Viewport& viewport,
                                                                const PlanWindow& window);

// The findings about the sheets at `indices` - their own, and the set's -
// in their order; every finding when `indices` is empty. What a plot of just
// those sheets is warned about.
[[nodiscard]] std::vector<Finding> findingsOnSheets(std::span<const Finding> findings,
                                                    std::span<const std::size_t> indices);

struct PreflightSummary {
    std::size_t errors = 0;
    std::size_t warnings = 0;
    std::size_t infos = 0;

    friend bool operator==(const PreflightSummary&, const PreflightSummary&) = default;
};

[[nodiscard]] PreflightSummary summarize(std::span<const Finding> findings);
// "2 errors, 1 warning, 3 notes"; "no problems found" for none.
[[nodiscard]] std::string summaryText(const PreflightSummary& summary);
// One line for a log: "ERROR viewport.outside [s2/vp3] PLAN 1:500 runs 12.0 mm
// under the title block. Fix: ..."
[[nodiscard]] std::string findingLine(const Finding& finding);

// The findings as JSON, for an agent: {"format": "katana-sheet-checks",
// "version": 1, "summary": {...}, "findings": [{...}]}. Empty members are
// left out; findingsFromJson reads it back to the same findings.
[[nodiscard]] std::string findingsToJson(std::span<const Finding> findings);
[[nodiscard]] core::Result<std::vector<Finding>> findingsFromJson(std::string_view json);

} // namespace katana::cad::plotting
