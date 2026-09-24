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
// in millimetres, white prints black (cad::paperColour), a solid hatch is
// opaque, text is set at its exact fractional size, and every mark - point
// crosses, the alignment overlay, hatch lines - is sized in paper
// millimetres, so a plot looks the same at 150 dpi and at 600.
//
// On screen the painter also does what only a screen wants for speed, none
// of which changes what a line looks like (docs/plan_view.md has the
// measurements): plain lines are clipped to the view before QPainter sees
// them, each distinct layer/style/colour is resolved to its pens once a
// frame instead of once an entity, and library symbols are stamped from
// images rasterised once per symbol, pen and size. A plot stays vector.

#include <cstddef>
#include <cstdint>
#include <map>
#include <memory>
#include <optional>
#include <set>
#include <string>
#include <utility>
#include <vector>

#include <QFont>
#include <QImage>
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
#include "katana/geometry/point_splat.hpp"
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
    // Screen only. Library symbols stamped from images rasterised once per
    // symbol, pen, size and quarter-pixel position, instead of stroked from
    // scratch at every point. Plots stay vector. Ignored while the painter's
    // transform turns or scales (a rotated viewport), where a stamp would not
    // land on the pixel grid it was rasterised for.
    bool symbolSprites = false;
    // Screen only. Plain polylines, arcs and circles clipped to the view
    // (with a margin wider than the pen reaches) before they are handed to
    // QPainter, so a string crossing the whole site costs what its visible
    // part costs. Off draws every vertex, which tests compare against.
    bool clipLines = true;
    // Screen only. At most this many cloud points projected a frame; above
    // it every part of the view draws the same share of its points, coarsest
    // first (geometry/point_splat.hpp), so a twenty-million-point cloud pans
    // as a four-million-point one does.
    std::size_t cloudPointBudget = katana::geometry::kSplatPointBudget;
    // The face plain text (TextGeometry, dimension labels, the overlay) is
    // drawn in. A library text names its own.
    QString fontFamily = QStringLiteral("Segoe UI");
};

// What one paint did, for a view's statistics and for tests that cannot
// look at pixels.
struct PlanPaintStats {
    std::size_t entitiesDrawn = 0; // passed the visibility rule and lay in view
    std::size_t symbolsStamped = 0;
    std::size_t spritesDrawn = 0; // of those, from a cached image
    // Distinct layer/style/colour combinations resolved to pens this paint:
    // once each, however many entities share one.
    std::size_t displaysResolved = 0;
    // Polylines, arcs and circles that ran off the view and were clipped.
    std::size_t linesClipped = 0;
    // Point-cloud points in the parts of the clouds on the view, and of those
    // how many were drawn: fewer only when the point budget thinned them.
    std::size_t cloudPointsInView = 0;
    std::size_t cloudPointsDrawn = 0;
};

// Sizes of the marks the painter draws that are not the drawing's own: on
// screen in pixels, on paper in millimetres of paper, so that a plot's marks
// do not shrink as its resolution rises (a 4 px cross was 0.68 mm at 300 dpi
// and 0.34 mm at 600).
inline constexpr double kPointMarkerPixels = 4.0;           // the plain point cross's half-width
inline constexpr double kPointMarkerPaperMillimetres = 1.0; // the same on paper: a 2 mm cross
inline constexpr double kHatchLinePaperMillimetres = 0.13;  // the finest ISO 128 pen
// The alignment overlay: on screen a 2 px centreline, 1 px ticks 6 px either
// side, 11 px labels at least 70 px apart; on paper the nearest standard
// sizes, the label gap kept in proportion to the label.
inline constexpr double kAlignmentLinePaperMillimetres = 0.5;
inline constexpr double kAlignmentTickPenPaperMillimetres = 0.25;
inline constexpr double kAlignmentTickPaperMillimetres = 1.5;
inline constexpr double kAlignmentTextPaperMillimetres = 2.5;

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
    // clear, reused rather than a QFont made per text: on screen one per
    // whole pixel size, on paper one for every size (a paper text is scaled
    // to its exact height rather than rounded to a font size).
    [[nodiscard]] std::size_t fontCount() const
    {
        return fonts_.size() + (paperFont_.has_value() ? 1u : 0u);
    }
    [[nodiscard]] std::size_t spriteCount() const { return sprites_.size(); }

  private:
    friend class PlanPainter;

    katana::cad::DefinitionCache definitions_;
    // Plain text's fonts by pixel size, for the family they were made in;
    // on paper one font at a reference size, scaled to each text's height.
    QString fontFamily_;
    std::map<int, QFont> fonts_;
    std::optional<QFont> paperFont_;

    struct RasterImage {
        katana::interop::ReferenceId id = 0;
        QImage image;
    };
    // A point cloud's display copy (geometry/point_splat.hpp): float offsets,
    // colours, tiles and level-of-detail order, built on the first frame that
    // shows the layer in this colour mode.
    struct CloudDisplay {
        katana::interop::ReferenceId id = 0;
        katana::interop::PointColorMode mode = katana::interop::PointColorMode::Elevation;
        katana::geometry::SplatCloud splat;
    };
    std::vector<RasterImage> rasters_;
    std::vector<CloudDisplay> clouds_;
    // The image every cloud is splatted into, kept between frames and
    // reallocated only when the view's size changes: allocating and filling a
    // window-sized image per cloud per frame cost 2.4 ms a cloud at 1600 x 1000.
    QImage cloudLayer_;

    // A symbol stamp rasterised once and blitted wherever the same symbol is
    // put in the same pen at the same size and sub-pixel position. The view
    // scale, the paper scale, the device ratio and the library generation
    // are the cache's, not the key's: every sprite goes when one changes.
    struct SpriteKey {
        std::string symbol;
        QRgb colour = 0; // with alpha
        double penWidth = 0.0;
        bool cosmetic = false;
        double size = 0.0; // Style::symbolSize
        int phaseX = 0;    // the stamp's sub-pixel position, in quarters of a device pixel
        int phaseY = 0;
        friend auto operator<=>(const SpriteKey&, const SpriteKey&) = default;
    };
    struct Sprite {
        QImage image;  // premultiplied, device pixels, tagged with the device ratio
        QPoint anchor; // the image's device pixel the stamp's whole pixel lands on
    };
    std::map<SpriteKey, Sprite> sprites_;
    double spriteScale_ = 0.0;
    double spritePaperScale_ = 0.0;
    double spriteDeviceRatio_ = 0.0;
    std::uint64_t spriteGeneration_ = 0;
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

// The whole-number resolution a QPdfWriter is given for `dpi` (the nearest,
// at least 1), and the painter scale that makes a sheet laid out at exactly
// `dpi` land exactly on it: the writer's resolution is an int, and a sheet at
// 300.9 dpi drawn at 300 plotted at 1 : 1003 while reporting 1 : 1000
// (audit QT-27).
struct PdfResolution {
    int resolution = 300;
    double scale = 1.0; // resolution / dpi
};
[[nodiscard]] PdfResolution pdfResolutionFor(double dpi);

// The frame a plot draws through: the printable area of the sheet (the paper
// less its margins), at the sheet's scale about its centre, clipped to it,
// so that at a fixed scale the drawing stops at the margin instead of running
// to the paper's edge. In the sheet's device pixels (cad::Sheet). Fails with
// whatever cad::sheetFor refuses.
[[nodiscard]] katana::core::Result<PlanFrame> sheetFrame(const katana::cad::PlotSettings& settings);

// Plots `source` to a one-page PDF at `path` through sheetFrame, in paper
// mode, with the view's hidden layers and reference layers. The PDF's title
// is `title` (the file's own name when empty) and its creator Katana, so a
// viewer's tab and a document list say what the sheet is. Widget-free, so it
// may run off the GUI thread with its own cache. Fails with
// FileExportFailure when the file cannot be written or finished, and with
// whatever cad::sheetFor refuses.
[[nodiscard]] katana::core::Status plotPlanToPdf(const QString& path,
                                                 const katana::cad::PlotSettings& settings,
                                                 const PlanSource& source,
                                                 const katana::cad::LayerOverrides& layers,
                                                 const std::set<std::uint64_t>& hiddenReferences,
                                                 PlanPaintCache& cache,
                                                 const QString& title = QString());

} // namespace katana::qt
