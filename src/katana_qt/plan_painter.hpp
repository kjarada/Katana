#pragma once

// The plan drawing, painted: one painter for the screen, the plot and every
// sheet viewport to come.
//
// What the plan view shows - imagery, point clouds, mesh footprints, every
// entity through its layer and style, library linestyles and symbols,
// hatches, dimensions, text and the alignment overlay - used to be painted by
// ViewportWidget's own members, and a plot borrowed the live widget to get at
// them: it swapped the view's transform for the sheet's, painted, and put it
// back. That tied every plot to a widget on the GUI thread, gave it one view
// transform and no rotation, and left the caches (dash patterns, flattened
// definitions) as widget members nobody else could share or own.
//
// paintPlan takes everything it needs as arguments - what to draw
// (PlanSource), where and how it is looked at (PlanFrame: the transform, a
// rotation, the viewport on the device, the view's hidden layers) and the
// medium (PlanPaintOptions: screen or paper, pixels per millimetre) - and
// keeps what it learns between frames in a PlanPaintCache the CALLER owns. It
// reads no widget and writes nothing but the painter and the cache, so it is
// reentrant: two threads each with their own cache may paint the same
// drawing onto a QImage and a QPdfWriter at once, which is what plotting a
// sheet set in the background needs. (The model must not be edited while it
// is being painted; the caller keeps it still, as the GUI thread does by
// painting between commands.)
//
// Screen and paper differ only where they must, and the difference is named
// here once: on screen a line is a hairline, the selection and locked layers
// show, and marks are sized in pixels; on paper a line is its layer's weight
// in millimetres, white prints black (cad::paperColour), and every mark -
// point crosses, the alignment overlay, hatch lines - is sized in paper
// millimetres, so a plot looks the same at 150 dpi and at 600.

#include <cstddef>
#include <cstdint>
#include <map>
#include <memory>
#include <set>
#include <string>
#include <utility>
#include <vector>

#include <QFont>
#include <QImage>
#include <QList>
#include <QPixmap>
#include <QPointF>
#include <QRgb>
#include <QString>

#include "katana/cad/layer_overrides.hpp"
#include "katana/cad/plot.hpp"
#include "katana/cad/scene.hpp"
#include "katana/cad/selection.hpp"
#include "katana/cad/style_resolver.hpp"
#include "katana/cad/view_transform.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/model.hpp"
#include "katana/entity/style_library.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/geometry/spatial_index.hpp"
#include "katana/interop/reference_data.hpp"

class QPainter;

namespace katana::cad {
class Document;
}

namespace katana::qt {

// What is drawn. Every pointer but `model` may be null, and each null leaves
// out only what it would have provided: no library draws symbols and
// linestyles as the built-in shapes and plain lines, no index visits every
// entity, no selection draws nothing selected.
struct PlanSource {
    const katana::entity::Model* model = nullptr;
    const katana::entity::StyleLibrary* library = nullptr;
    // Document::libraryGeneration: what the cache's flattened definitions
    // are keyed on, since a library can be replaced under the same address.
    std::uint64_t libraryGeneration = 0;
    const katana::geometry::SpatialIndex* index = nullptr;
    const katana::cad::SelectionSet* selection = nullptr;
    const katana::interop::ReferenceData* reference = nullptr;
    const std::vector<katana::cad::SceneMesh>* meshes = nullptr;
};

// The document's drawing, library, index and selection. The reference data and
// meshes belong to the window and are the caller's to add.
[[nodiscard]] PlanSource planSourceOf(const katana::cad::Document& document);

// Where, and through what, the drawing is looked at.
struct PlanFrame {
    // Model units to viewport pixels (y down), SIZED TO THE VIEWPORT: its
    // centre is drawn at the viewport's centre. Unrotated.
    katana::cad::ViewTransform transform{};
    // Counter-clockwise, in radians: the drawing turned on the device about
    // the viewport's centre - a twisted viewport, as a sheet laid along an
    // alignment is. 0 is north up.
    double rotation = 0.0;
    // The viewport's top-left corner on the device, in the painter's units.
    QPointF origin{0.0, 0.0};
    // Nothing is painted outside the viewport's rectangle. Off for the screen,
    // whose widget clips already; on for a plot's printable area and for a
    // viewport on a sheet, where the drawing must not run under the frame.
    bool clip = false;
    // The view's own hidden layers and reference layers; null hides nothing
    // beyond what the document hides.
    const katana::cad::LayerOverrides* layers = nullptr;
    const std::set<std::uint64_t>* hiddenReferences = nullptr;
};

enum class PlanMedium { Screen, Paper };

struct PlanPaintOptions {
    PlanMedium medium = PlanMedium::Screen;
    // Device pixels per millimetre. On paper, of the paper: what a line weight
    // and every paper-sized mark is multiplied by. On screen, of the screen:
    // what a paper linestyle's millimetres are, a screen line being a hairline
    // whatever its weight.
    double pixelsPerMillimetre = 96.0 / 25.4;
    // On paper, the plot's settings, for the paper colour rule (D7). Required
    // on paper; ignored on screen.
    const katana::cad::PlotSettings* plot = nullptr;
    // What is drawn besides the entities. The grid is screen furniture.
    // Rasters, point clouds and mesh footprints are screen only for now:
    // imagery at plot resolution needs a resolution cap first (an A1 page at
    // 300 dpi is 70 megapixels), and a footprint's pens are screen pixels.
    bool grid = false;
    bool rasters = true;
    bool pointClouds = true;
    bool meshFootprints = true;
    bool alignments = true;
    // Screen only. Every line a cosmetic one-pixel pen instead of the 1.5 px
    // hairline: measured 5-8x cheaper to stroke, because Qt's fast path for
    // antialiased lines needs a width of at most one pixel. Plots keep their
    // paper-millimetre weights whatever this says.
    bool thinLines = false;
    // Screen only. Library symbols stamped from pixmaps rasterised once per
    // symbol, pen and size, instead of stroked from scratch at every point.
    // Plots stay vector.
    bool symbolSprites = false;
    // The face plain text (TextGeometry, dimension labels, the overlay) is
    // drawn in. A library text names its own.
    QString fontFamily = QStringLiteral("Segoe UI");
};

// What one paint did, for a view's statistics and for tests that cannot
// look at pixels.
struct PlanPaintStats {
    std::size_t entitiesDrawn = 0; // passed the visibility rule and lay in view
    std::size_t symbolsStamped = 0;
    std::size_t spritesDrawn = 0; // of those, from a cached pixmap
};

// What the painter keeps between frames. Owned by the caller - one per view,
// one per plotting thread - and never shared between threads. Everything in
// it is keyed so that a stale entry is not used: flattened definitions on the
// library generation, raster images and cloud colours on the reference
// layer's id and mode, symbol sprites on the symbol, pen and size.
class PlanPaintCache {
  public:
    // Drops the imagery and point-cloud colours, which are keyed on a layer's
    // id and not its contents: call after a reference layer is added,
    // removed, or has its display settings changed.
    void invalidateReferences();
    void clear();

    // How many distinct fonts plain text has been set in since the last
    // clear: one per size, reused, rather than a QFont made per text.
    [[nodiscard]] std::size_t fontCount() const { return fonts_.size(); }
    [[nodiscard]] std::size_t spriteCount() const { return sprites_.size(); }

  private:
    friend class PlanPainter;

    katana::cad::DefinitionCache definitions_;
    // Dash patterns by linetype name and pen width. A pattern also depends on
    // the view scale, so they are dropped whenever the scale changes.
    std::map<std::pair<std::string, double>, QList<qreal>> dashes_;
    double dashScale_ = 0.0;
    // Plain text's fonts by pixel size, for the family they were made in.
    QString fontFamily_;
    std::map<double, QFont> fonts_;

    struct RasterImage {
        katana::interop::ReferenceId id = 0;
        QImage image;
    };
    struct CloudColours {
        katana::interop::ReferenceId id = 0;
        katana::interop::PointColorMode mode = katana::interop::PointColorMode::Elevation;
        std::vector<QRgb> colours;
    };
    std::vector<RasterImage> rasters_;
    std::vector<CloudColours> clouds_;

    struct SpriteKey {
        std::string symbol;
        QRgb colour = 0;
        double penWidth = 0.0;
        double size = 0.0;      // Style::symbolSize
        double scale = 0.0;     // view pixels per model unit
        double paperScale = 0.0;
        int phaseX = 0;         // the stamp's sub-pixel position, in quarters
        int phaseY = 0;
        friend auto operator<=>(const SpriteKey&, const SpriteKey&) = default;
    };
    struct Sprite {
        QImage image;       // premultiplied, device pixels
        QPointF offset;     // from the stamp's whole pixel to the image's corner
        bool empty = true;
    };
    std::map<SpriteKey, Sprite> sprites_;
};

// Paints the drawing through `frame` onto `painter`. The painter's state is
// the same afterwards as before. Reentrant: see the file comment.
PlanPaintStats paintPlan(QPainter& painter, const PlanSource& source, const PlanFrame& frame,
                         const PlanPaintOptions& options, PlanPaintCache& cache);

// One geometry, in the painter's current pen, through `frame`: a tool's
// preview, which is drawn as the geometry it will become. Never hatched.
void paintPlanGeometry(QPainter& painter, const PlanFrame& frame, const PlanPaintOptions& options,
                       PlanPaintCache& cache, const katana::entity::Geometry& geometry,
                       const katana::entity::DimensionStyle& dimensionStyle);

// The model-space box the frame's viewport shows: for a rotated frame, the
// box that bounds the rotated rectangle, so that an entity in a corner of a
// twisted viewport is not culled.
[[nodiscard]] katana::geometry::Box2 visibleBox(const PlanFrame& frame);

// Everything a view with these hidden layers and reference layers draws: the
// entities its layers let through, the visible reference layers it does not
// hide, the shown meshes and the alignments. What Zoom Extents frames and
// what a plot's Fit fits.
[[nodiscard]] katana::geometry::Box2
planDrawnBounds(const PlanSource& source, const katana::cad::LayerOverrides& layers,
            const std::set<std::uint64_t>& hiddenReferences);

// The whole-number resolution a QPdfWriter is given for `dpi`, and the
// painter scale that makes a sheet laid out at exactly `dpi` land exactly on
// it: the writer's resolution is an int, and a sheet at 300.9 dpi drawn at
// 300 plotted at 1 : 1003 while reporting 1 : 1000 (audit QT-27).
struct PdfResolution {
    int resolution = 300;
    double scale = 1.0; // resolution / dpi
};
[[nodiscard]] PdfResolution pdfResolutionFor(double dpi);

// The frame a plot draws through: the printable area of the sheet (the paper
// less its margins), at the sheet's scale about its centre, clipped to it.
// Fails with whatever cad::sheetFor refuses.
[[nodiscard]] katana::core::Result<PlanFrame> sheetFrame(const katana::cad::PlotSettings& settings);

// Plots `source` to a one-page PDF at `path` through sheetFrame, in paper
// mode, with the view's hidden layers and reference layers. Widget-free, so
// it may run off the GUI thread with its own cache. Fails with
// FileExportFailure when the file cannot be written or finished, and with
// whatever cad::sheetFor refuses.
[[nodiscard]] katana::core::Status plotPlanToPdf(const QString& path,
                                                 const katana::cad::PlotSettings& settings,
                                                 const PlanSource& source,
                                                 const katana::cad::LayerOverrides& layers,
                                                 const std::set<std::uint64_t>& hiddenReferences,
                                                 PlanPaintCache& cache);

} // namespace katana::qt
