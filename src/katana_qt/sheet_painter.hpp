#pragma once

// A sheet, painted: the frame and title block, and every viewport on it
// (docs/plotting.md, "The sheet painter").
//
// The model - cad/plotting/ - says what a sheet holds; this draws it, for the
// sheet editor on screen and for a PDF on paper, through one function, so the
// two cannot disagree about anything but the resolution.
//
//   the frame      its lines at their measured weights and dashes, round caps;
//                  its texts in Arial with the 0.82 x factor applied as a
//                  painter scale (never a font stretch), sized by cap height,
//                  fields filled from resolveFields and centred in their
//                  cells; the legend glyphs; the user's logo fitted to its
//                  slot. The construction guide only when asked (the editor).
//   Plan           the drawing through paintPlan, at the viewport's scale and
//                  rotation, clipped to its rectangle, with its match lines.
//   KeyPlan        the same, faded, with the other sheets' outlines numbered.
//   LongSection    ground and design along an alignment, a grid, level and
//   CrossSections  chainage labels and a data band; cut once and cached;
//                  cut and fill shaded, crossings noted with level and
//                  depth, labels placed clear of each other; autoScale
//                  fitted (resolveSectionViewport).
//   Model3D        the 3D view's renderer, as a raster capped in resolution.
//   Legend         the layers the sheet draws, a sample line each.
//   Notes, Image   text wrapped to the rectangle; a project image fitted.
//
// North arrows, scale bars and titles are sized in paper millimetres.
//
// PAPER COORDINATES are millimetres from the bottom-left of the paper, Y up
// (frame.hpp). The device is Y down: SheetPaintOptions says where the paper's
// top-left corner is on it and how many device pixels a millimetre is, and
// every paper point goes through that one mapping.
//
// Like paintPlan this reads no widget and keeps what it learns in a cache
// the caller owns, so a sheet set can be plotted off the GUI thread.

#include <cstddef>
#include <cstdint>
#include <filesystem>
#include <map>
#include <optional>
#include <span>
#include <string>
#include <vector>

#include <QImage>
#include <QPointF>
#include <QRectF>
#include <QString>

#include "katana/cad/plot.hpp"
#include "katana/cad/plotting/frame.hpp"
#include "katana/cad/plotting/section_fit.hpp"
#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/cad/scene.hpp"
#include "katana/cad/section.hpp"
#include "katana/core/error.hpp"
#include "plan_painter.hpp"

class QPainter;

namespace katana::cad {
class Document;
}

namespace katana::qt {

// What the sheets are drawn from. Only `plan.model` is required; each other
// member left empty leaves out only what it provides.
struct SheetSource {
    PlanSource plan;
    // For the 3D snapshot, whose scene builder reads the document.
    const katana::cad::Document* document = nullptr;
    // Surfaces the sections are cut from and the 3D snapshot shows; hidden
    // ones are left out, as the section view leaves them out.
    std::vector<katana::cad::SceneSurface> surfaces;
    // Counts every change to the drawing (Document::modelRevision): what the
    // cached sections and snapshots are keyed on.
    std::uint64_t revision = 0;
    katana::cad::plotting::FieldContext fields;
    // The set's logo, decoded once for every page. Null: the slot is empty.
    QImage logo;
    // Where an Image viewport's file name is looked up.
    std::filesystem::path assets;
};

struct SheetPaintOptions {
    // Paper everywhere but where the editor wants a screen's economies; the
    // editor paints PAPER too, so what it shows is what plots.
    PlanMedium medium = PlanMedium::Paper;
    double pixelsPerMillimetre = 300.0 / 25.4;
    // The device point the paper's top-left corner lands on.
    QPointF origin{0.0, 0.0};
    // The paper colour rule (white prints black) and the resolution a 3D
    // snapshot is capped at.
    katana::cad::PlotSettings plot{};
    // Editor furniture, never plotted: the construction guide at the paper's
    // edge, and a faint name in each empty user slot (logo, organisation).
    bool construction = false;
    bool slotHints = false;
    // The 3D snapshot is rendered at no more than this many dots per inch
    // and this many pixels, whatever the plot's resolution: an A1 panel at
    // 300 dpi would be 70 megapixels of shaded terrain in every PDF.
    double rasterDpiCap = 200.0;
    std::size_t rasterPixelCap = 8'000'000;
};

// What one paint drew, for the editor's status line and for tests.
struct SheetPaintStats {
    std::size_t viewportsDrawn = 0;
    std::size_t frameTextsDrawn = 0;
    std::size_t planEntitiesDrawn = 0;
    bool logoDrawn = false;
    // Viewports that could not be drawn as asked, one line each ("vp3: no
    // alignment named MC01"). The viewport is outlined and left empty.
    std::vector<std::string> problems;
    // Notes the sections left out for want of room clear of the others in
    // their plots - a crossing's, the centreline's levels - each dropped
    // rather than drawn over another label or past its plot.
    std::size_t sectionNotesDropped = 0;
};

// A viewport's scale and centre once "auto" is decided: what is drawn, and
// what the editor reports.
struct ResolvedViewport {
    double scale = 500.0;
    katana::geometry::Point2 centre{};
};

// What the painter keeps between paints: one per editor, one per plotting
// thread. Sections and snapshots are keyed on the source's revision, so a
// stale one is never drawn.
class SheetPaintCache {
  public:
    [[nodiscard]] PlanPaintCache& plan() { return plan_; }
    void clear();

  private:
    friend class SheetPainter;

    PlanPaintCache plan_;
    struct CachedSection {
        std::uint64_t revision = 0;
        std::optional<katana::cad::Section> section;
        std::string failure;
        double startChainage = 0.0;
    };
    std::map<std::string, CachedSection> sections_;
    struct CachedImage {
        std::uint64_t revision = 0;
        QImage image;
    };
    std::map<std::string, CachedImage> snapshots_;
    std::map<std::string, QImage> images_;
    std::map<std::string, katana::core::Result<katana::cad::plotting::Frame>> frames_;
};

// Paints sheet `index` of `set`. The painter's state is the same afterwards.
SheetPaintStats paintSheet(QPainter& painter, const katana::cad::plotting::SheetSet& set,
                           std::size_t index, const SheetSource& source,
                           const SheetPaintOptions& options, SheetPaintCache& cache);

// The scale and centre `viewport` is drawn at: its own, or for autoScale the
// largest of kSheetScales at which what it shows fits, and for autoCentre the
// middle of it. Plans only; a section's are resolveSectionViewport's.
[[nodiscard]] ResolvedViewport resolvePlanViewport(const katana::cad::plotting::Viewport& viewport,
                                                   const SheetSource& source);

// The horizontal scale and exaggeration section `viewport` is drawn at: its
// own, or for autoScale the largest standard scale at which its chainage
// range (the whole alignment without one) or its cross sections' width fits
// its plot, and the largest exaggeration of kSectionExaggerations at which
// its levels then fit (cad/plotting/section_fit.hpp). The sections are cut
// through `cache`, as a paint cuts them; a section that cannot be cut keeps
// the viewport's own scale and exaggeration.
[[nodiscard]] katana::cad::plotting::SectionFit
resolveSectionViewport(const katana::cad::plotting::Viewport& viewport, const SheetSource& source,
                       SheetPaintCache& cache);

// The one mapping from paper to device and back.
[[nodiscard]] QPointF paperToDevice(const katana::geometry::Point2& paper, double paperHeightMm,
                                    const SheetPaintOptions& options);
[[nodiscard]] QRectF paperToDevice(const katana::geometry::Box2& paper, double paperHeightMm,
                                   const SheetPaintOptions& options);
[[nodiscard]] katana::geometry::Point2 deviceToPaper(const QPointF& device, double paperHeightMm,
                                                     const SheetPaintOptions& options);

// The frame sheet `sheet` is drawn in, or nothing for a sheet without one
// (an empty frame id, or a portrait sheet).
[[nodiscard]] std::optional<katana::cad::plotting::Frame>
sheetFrame(const katana::cad::plotting::Sheet& sheet);

// Plots the sheets at `indices` of `set` (every sheet when empty), in that
// order, to one vector PDF at `path`, each page the size of its sheet's paper.
// `problems` receives what paintSheet reports, prefixed with the sheet's
// name. FileExportFailure when the file cannot be written; InvalidArgument
// for an index past the end or a set with nothing to plot.
[[nodiscard]] katana::core::Status
plotSheetsToPdf(const QString& path, const katana::cad::plotting::SheetSet& set,
                std::span<const std::size_t> indices, const SheetSource& source, double dpi,
                SheetPaintCache& cache, const QString& title = QString(),
                std::vector<std::string>* problems = nullptr);

} // namespace katana::qt
