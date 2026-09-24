#include "plan_painter.hpp"

#include <algorithm>
#include <charconv>
#include <cmath>
#include <functional>
#include <limits>
#include <optional>
#include <string_view>
#include <unordered_map>

#include <QFileInfo>
#include <QFont>
#include <QLineF>
#include <QList>
#include <QMarginsF>
#include <QPageSize>
#include <QPaintDevice>
#include <QPainter>
#include <QPainterPath>
#include <QPdfWriter>
#include <QPolygonF>
#include <QRect>
#include <QTransform>

#include "customisation/style_painter.hpp"
#include "katana/cad/dashing.hpp"
#include "katana/cad/dimension_draw.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/hatching.hpp"
#include "katana/cad/spatial_query.hpp"
#include "katana/cad/style_drawing.hpp"
#include "katana/core/task_pool.hpp"
#include "katana/entity/display.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/chording.hpp"
#include "katana/geometry/polygon.hpp"

namespace katana::qt {

namespace cad = katana::cad;
using katana::entity::Entity;
using katana::geometry::Arc2;
using katana::geometry::Box2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;

namespace {

const QColor kGridMinor(0x2a, 0x31, 0x39);
const QColor kGridMajor(0x38, 0x42, 0x4d);
const QColor kAxis(0x5a, 0x68, 0x75);
const QColor kSelection(0xff, 0x9f, 0x1c);
const QColor kAlignment(0xff, 0xb7, 0x4d); // amber: an overlay, not drawing content

// The size a paper text's one font is made at, in pixels. A text is set by
// scaling this, so its height on the page is exact to the fraction of a
// device pixel; the size only has to be large enough that the font's own
// metrics are not rounded coarsely.
constexpr int kPaperFontReferencePixels = 100;

// Texts are drawn no taller than this: a font asked for at a size of
// hundreds of thousands of pixels, one zoom step from a survey's extent,
// makes the raster engine allocate glyphs larger than any screen.
constexpr double kMaximumTextPixels = 2000.0;

// A symbol stamp larger than this across, in device pixels, is stroked and
// not cached: a sprite of it would cost more memory than it saves time, and
// a symbol that large is few on screen.
constexpr int kMaximumSpritePixels = 256;
// More cached stamps than this and the cache is emptied: a view zoomed
// through many scales keeps only the current one's anyway (the cache is
// keyed on the scale), and this bounds a drawing of very many symbol and
// colour combinations.
constexpr std::size_t kMaximumSprites = 20000;

QColor toQColor(const katana::entity::Color& color)
{
    return QColor(color.r, color.g, color.b, color.a);
}

// Whether a mesh is drawn at all - in plan as its footprint, and so counted
// in what Zoom Extents frames. The 3D scene applies the same rule to it.
bool isShown(const katana::cad::SceneMesh& item)
{
    return item.visible && item.mesh != nullptr && !item.mesh->empty() &&
           item.style != katana::cad::SurfaceStyle::Hidden;
}

bool hiddenReference(const PlanFrame& frame, std::uint64_t id)
{
    return frame.hiddenReferences != nullptr && frame.hiddenReferences->contains(id);
}

// Chainage in the civil convention, kilometres + metres: 1234.5 reads as
// "1+234.50". Formatted with to_chars rather than snprintf, because snprintf
// obeys the C locale and would print "1+234,50" on a machine set to one that
// uses a decimal comma - the same reason the dimension formatter avoids it.
std::string formatStation(double station)
{
    const double magnitude = std::abs(station);
    const auto kilometres = static_cast<long long>(magnitude / 1000.0);
    const double metres = magnitude - static_cast<double>(kilometres) * 1000.0;
    char buffer[32];
    const auto result =
        std::to_chars(buffer, buffer + sizeof buffer, metres, std::chars_format::fixed, 2);
    std::string metresText(buffer, result.ptr);
    while (metresText.size() < 6) { // "000.00"
        metresText.insert(metresText.begin(), '0');
    }
    return (station < 0.0 ? "-" : "") + std::to_string(kilometres) + "+" + metresText;
}

const katana::cad::LayerOverrides& noOverrides()
{
    static const katana::cad::LayerOverrides none;
    return none;
}

// What a source with no library draws symbols and linestyles from: nothing,
// so the built-in shapes and plain lines.
const katana::entity::StyleLibrary& noLibrary()
{
    static const katana::entity::StyleLibrary none;
    return none;
}

// What an entity's pens depend on besides the frame: its layer, its style
// and its own colour. The names are views into the entity's own strings,
// which the model keeps still for the whole paint.
struct DisplayKey {
    std::string_view layer;
    std::string_view style;
    std::uint32_t colour = 0;
    bool hasColour = false;
    friend bool operator==(const DisplayKey&, const DisplayKey&) = default;
};

struct DisplayKeyHash {
    std::size_t operator()(const DisplayKey& key) const noexcept
    {
        std::size_t hash = std::hash<std::string_view>{}(key.layer);
        const auto mix = [&hash](std::size_t value) {
            hash ^= value + 0x9e3779b97f4a7c15ULL + (hash << 6) + (hash >> 2);
        };
        mix(std::hash<std::string_view>{}(key.style));
        mix((static_cast<std::size_t>(key.colour) << 1) | (key.hasColour ? 1u : 0u));
        return hash;
    }
};

DisplayKey displayKeyOf(const Entity& entity)
{
    DisplayKey key;
    key.layer = entity.layer;
    key.style = entity.style;
    if (entity.color.has_value()) {
        const katana::entity::Color& c = *entity.color;
        key.colour = (static_cast<std::uint32_t>(c.r) << 24) | (static_cast<std::uint32_t>(c.g) << 16) |
                     (static_cast<std::uint32_t>(c.b) << 8) | static_cast<std::uint32_t>(c.a);
        key.hasColour = true;
    }
    return key;
}

// The width a pen strokes, in the painter's units: a cosmetic pen's width
// is device pixels and never less than one.
double strokeWidth(const QPen& pen)
{
    return pen.isCosmetic() || pen.widthF() <= 0.0 ? std::max(1.0, pen.widthF()) : pen.widthF();
}

} // namespace

// ---- the cache ------------------------------------------------------------------------

void PlanPaintCache::invalidateReferences()
{
    rasters_.clear();
    clouds_.clear();
}

void PlanPaintCache::clear()
{
    definitions_.clear();
    fonts_.clear();
    paperFont_.reset();
    rasters_.clear();
    clouds_.clear();
    sprites_.clear();
    spriteScale_ = 0.0;
    spritePaperScale_ = 0.0;
    spriteDeviceRatio_ = 0.0;
    spriteGeneration_ = 0;
}

// ---- the painter ----------------------------------------------------------------------

// One paint's worth of state over the caller's arguments. Built on the stack
// for each paintPlan, so nothing here outlives a frame except what it puts in
// the cache - which is what makes paintPlan reentrant.
class PlanPainter {
  public:
    PlanPainter(QPainter& painter, const PlanSource& source, const PlanFrame& frame,
                const PlanPaintOptions& options, PlanPaintCache& cache)
        : painter_(painter), source_(source), frame_(frame), options_(options), cache_(cache),
          library_(source.library != nullptr ? *source.library : noLibrary()),
          view_(frame.transform), visible_(visibleBox(frame))
    {
        if (cache_.fontFamily_ != options_.fontFamily) {
            cache_.fonts_.clear();
            cache_.paperFont_.reset();
            cache_.fontFamily_ = options_.fontFamily;
        }
    }

    PlanPaintStats paint();
    void geometry(const katana::entity::Geometry& geometry,
                  const katana::entity::DimensionStyle& dimensionStyle)
    {
        hatch_ = nullptr;
        dimensionStyle_ = dimensionStyle;
        drawGeometry(geometry);
    }

  private:
    // Everything an entity's look depends on besides its geometry, worked
    // out once per distinct layer, style and colour in a paint rather than
    // once per entity: the layer walk, resolveDisplay's table lookups and
    // string copies, the linetype and hatch resolution, the pens and the
    // flattened library linestyle. Measured on a 28k-entity survey drawing
    // this was about a sixth of a frame; a drawing has tens to hundreds of
    // such combinations.
    struct Resolved {
        katana::entity::ResolvedLayer layer;
        bool layerDrawn = false; // shown, and not hidden in this view
        katana::entity::ResolvedDisplay display;
        cad::ResolvedLinetype pattern;
        const katana::entity::HatchPattern* hatch = nullptr;
        std::shared_ptr<const cad::FlatDefinition> linestyle; // a library linestyle, flattened
        QPen entityPen; // the entity's pen WITHOUT a model linetype's dashes
        QPen pen;       // the pen the plain line is drawn with
        StylePaintTarget target;
    };
    // What a point symbol stamp is, worked out once per symbol and size in
    // a paint: where it reaches from its insertion point and whether it is
    // too small to draw.
    struct Stamp {
        Box2 extent; // about the insertion point
        bool belowDetail = false;
        bool empty = true;
    };

    [[nodiscard]] bool paper() const { return options_.medium == PlanMedium::Paper; }
    [[nodiscard]] QPointF toScreen(const Point2& world) const
    {
        const Point2 p = view_.worldToScreen(world);
        return QPointF(p.x, p.y);
    }
    [[nodiscard]] int widthPixels() const { return static_cast<int>(view_.widthPixels); }
    [[nodiscard]] int heightPixels() const { return static_cast<int>(view_.heightPixels); }
    // Model units to one plot millimetre, which is what a paper linestyle is
    // measured in. Dividing by the view scale is what makes such a mark keep
    // its size on the PAGE as you zoom, which is the whole point of one. On
    // screen a millimetre is a millimetre OF SCREEN.
    [[nodiscard]] double paperScale() const
    {
        return options_.pixelsPerMillimetre / std::max(view_.scale, 1e-12);
    }
    // A mark `pixels` wide on screen and `millimetres` wide on paper, in
    // device pixels.
    [[nodiscard]] double markSize(double pixels, double millimetres) const
    {
        return paper() ? millimetres * options_.pixelsPerMillimetre : pixels;
    }
    // The plain point mark's half-width, in device pixels.
    [[nodiscard]] double markPixels() const
    {
        return markSize(kPointMarkerPixels, kPointMarkerPaperMillimetres);
    }
    // The plain point mark's size, in model units at the current scale, so a
    // built-in symbol with no size of its own stays a mark and not a blob.
    [[nodiscard]] double plainMarkHalfWidth() const
    {
        return markPixels() / std::max(view_.scale, 1e-12);
    }
    const QFont& fontFor(double pixels);
    const QFont& paperFont();
    const Resolved& resolve(const Entity& entity);
    const Stamp& stampOf(const std::string& symbol, double size);

    void drawRasters();
    void drawGrid();
    void drawPointClouds();
    void drawMeshFootprints();
    void drawEntities();
    void drawAlignments();
    void drawGeometry(const katana::entity::Geometry& geometry);
    // The polyline through `vertices` (model units), in the painter's pen,
    // clipped to the view when that cannot change a pixel.
    void strokePolyline(const std::vector<Point2>& vertices, bool closed);
    [[nodiscard]] bool drawLineStyle(const StylePaintTarget& target,
                                     const cad::FlatDefinition& definition,
                                     const katana::entity::Geometry& geometry);
    void drawSymbol(const StylePaintTarget& target, const std::string& symbol,
                    const Point2& centre, double size);
    [[nodiscard]] bool stampSprite(const StylePaintTarget& target, const std::string& symbol,
                                   const Point2& centre, double size, const Stamp& stamp);
    void drawHatch(const Polyline2& boundary, const QPolygonF& screen);
    void drawText(const Point2& position, const std::string& text, double height,
                  double rotation);

    QPainter& painter_;
    const PlanSource& source_;
    const PlanFrame& frame_;
    const PlanPaintOptions& options_;
    PlanPaintCache& cache_;
    const katana::entity::StyleLibrary& library_;
    const cad::ViewTransform& view_;
    const Box2 visible_;
    PlanPaintStats stats_{};

    // The dimension style and hatch pattern in force for the entity being
    // drawn: resolved once per entity, and held here because drawGeometry's
    // visitor is handed only the geometry. The hatch pattern is owned by the
    // model, which outlives the paint.
    katana::entity::DimensionStyle dimensionStyle_{};
    const katana::entity::HatchPattern* hatch_ = nullptr;

    // This paint's resolutions, and the last one asked for: consecutive
    // entities mostly share a layer and style, and comparing two short
    // strings is cheaper than hashing them.
    std::unordered_map<DisplayKey, Resolved, DisplayKeyHash> resolved_;
    DisplayKey lastKey_{};
    const Resolved* last_ = nullptr;
    // By symbol, then by size: a handful of sizes per symbol at most.
    std::map<std::string, std::vector<std::pair<double, Stamp>>, std::less<>> stamps_;
    // Dash patterns by linetype name and pen width, for this paint only. A
    // pattern is the linetype's CONTENTS at this scale, and the model's
    // linetypes can be edited between two paints (updateLinetype, its undo,
    // a delete and re-create under the name) with nothing here to say so;
    // kept across paints by name, a view drew the old dashes until it was
    // zoomed and a second plot printed them. Worked out once per distinct
    // display a paint anyway (resolve), so keeping them longer saved a few
    // dozen small vectors a frame at most.
    std::map<std::pair<std::string, double>, QList<qreal>> dashes_;
    // Whether stamps may come from the sprite cache this paint, and where
    // the painter's (translation-only) transform and the device ratio put a
    // logical pixel.
    bool sprites_ = false;
    QPointF translation_{0.0, 0.0};
    double deviceRatio_ = 1.0;
    // Scratch for clipping and for arcs, reused across entities.
    katana::geometry::PolylineRuns runs_;
    std::vector<Point2> arcPoints_;
};

const QFont& PlanPainter::fontFor(double pixels)
{
    // One QFont per size, kept: making a QFont by family name resolves the
    // family through the font database, which done for each of thousands of
    // labels a frame was a measurable part of the frame. On screen a text is
    // a whole number of pixels, the screen's own resolution, and the glyphs
    // are hinted at that size.
    const int size = static_cast<int>(pixels);
    auto found = cache_.fonts_.find(size);
    if (found == cache_.fonts_.end()) {
        QFont font(options_.fontFamily);
        font.setPixelSize(size);
        found = cache_.fonts_.emplace(size, font).first;
    }
    return found->second;
}

const QFont& PlanPainter::paperFont()
{
    // One font for every paper text, scaled to each: a font's pixel size is
    // an int, and a 2.5 mm label at 1 : 1000 and 300 dpi is 29.5 device
    // pixels, which a whole-pixel size set 0.04 mm short (and at 72 dpi,
    // 0.35 mm).
    if (!cache_.paperFont_.has_value()) {
        QFont font(options_.fontFamily);
        font.setPixelSize(kPaperFontReferencePixels);
        cache_.paperFont_ = font;
    }
    return *cache_.paperFont_;
}

PlanPaintStats PlanPainter::paint()
{
    painter_.save();
    if (!frame_.origin.isNull()) {
        painter_.translate(frame_.origin);
    }
    if (frame_.clip) {
        painter_.setClipRect(QRectF(0.0, 0.0, view_.widthPixels, view_.heightPixels),
                             Qt::IntersectClip);
    }
    if (frame_.rotation != 0.0) {
        // About the viewport's centre, the point the transform puts its
        // centre at. Screen y points down, so a counter-clockwise turn of
        // the drawing is a negative angle to QPainter.
        const QPointF centre(0.5 * view_.widthPixels, 0.5 * view_.heightPixels);
        painter_.translate(centre);
        painter_.rotate(-frame_.rotation * katana::math::kRadToDeg);
        painter_.translate(-centre);
    }
    // A stamp is blitted onto whole device pixels, which only a painter that
    // merely shifts the drawing keeps: under a turn or a scale it is stroked.
    const QTransform& transform = painter_.transform();
    sprites_ = options_.symbolSprites && !paper() && transform.type() <= QTransform::TxTranslate;
    translation_ = QPointF(transform.dx(), transform.dy());
    deviceRatio_ = painter_.device() != nullptr ? painter_.device()->devicePixelRatioF() : 1.0;
    if (sprites_ && (cache_.spriteScale_ != view_.scale || cache_.spritePaperScale_ != paperScale() ||
                     cache_.spriteDeviceRatio_ != deviceRatio_ ||
                     cache_.spriteGeneration_ != source_.libraryGeneration ||
                     cache_.sprites_.size() > kMaximumSprites)) {
        cache_.sprites_.clear();
        cache_.spriteScale_ = view_.scale;
        cache_.spritePaperScale_ = paperScale();
        cache_.spriteDeviceRatio_ = deviceRatio_;
        cache_.spriteGeneration_ = source_.libraryGeneration;
    }

    // Imagery sits beneath everything: it is a backdrop, and the grid has to
    // stay legible over it. Point clouds sit above the grid but below the
    // drawing, so drawn geometry is never obscured by survey returns.
    if (options_.rasters && !paper()) {
        drawRasters();
    }
    if (options_.grid && !paper()) {
        drawGrid();
    }
    if (options_.pointClouds && !paper()) {
        drawPointClouds();
    }
    painter_.setRenderHint(QPainter::Antialiasing, true);
    // Beneath the drawing, like a surface: a mesh is context for what is
    // drawn over it, not a thing to be picked in plan.
    if (options_.meshFootprints && !paper()) {
        drawMeshFootprints();
    }
    if (source_.model != nullptr) {
        drawEntities();
        if (options_.alignments) {
            drawAlignments();
        }
    }
    painter_.restore();
    return stats_;
}

void PlanPainter::drawRasters()
{
    if (source_.reference == nullptr) {
        return;
    }
    for (const katana::interop::RasterOverlay& raster : source_.reference->rasters()) {
        // Hidden in the Reference Data panel, or in this view only.
        if (!raster.visible || hiddenReference(frame_, raster.id) || raster.width <= 0 ||
            raster.height <= 0) {
            continue;
        }

        // Cache the QImage: rebuilding it from the RGBA bytes every frame would
        // copy tens of megabytes per repaint.
        auto cached = std::find_if(
            cache_.rasters_.begin(), cache_.rasters_.end(),
            [&raster](const PlanPaintCache::RasterImage& entry) { return entry.id == raster.id; });
        if (cached == cache_.rasters_.end()) {
            QImage image(reinterpret_cast<const uchar*>(raster.rgba.data()), raster.width,
                         raster.height, raster.width * 4, QImage::Format_RGBA8888);
            // copy(): the QImage above only borrows the vector's buffer, and the
            // cache must outlive this loop iteration.
            cache_.rasters_.push_back(PlanPaintCache::RasterImage{raster.id, image.copy()});
            // Drawn in this same paint: an imported image must not wait for
            // the next one, which nothing asks for.
            cached = std::prev(cache_.rasters_.end());
        }

        // Pixel -> world is the geotransform; world -> screen is the view. The
        // composition is itself affine, so it is handed to QPainter as one
        // transform rather than resampling the image here.
        //
        //   world.x = g0 + px*g1 + py*g2       screen.x = w/2 + (world.x - cx)*s
        //   world.y = g3 + px*g4 + py*g5       screen.y = h/2 - (world.y - cy)*s
        //
        // so screen.x = [w/2 + (g0-cx)s] + px*(g1 s) + py*(g2 s)
        //    screen.y = [h/2 - (g3-cy)s] + px*(-g4 s) + py*(-g5 s)
        const auto& g = raster.geotransform;
        const double s = view_.scale;
        const double dx = 0.5 * widthPixels() + (g[0] - view_.center.x) * s;
        const double dy = 0.5 * heightPixels() - (g[3] - view_.center.y) * s;
        const QTransform transform(g[1] * s, -g[4] * s, g[2] * s, -g[5] * s, dx, dy);

        painter_.save();
        painter_.setOpacity(std::clamp(raster.opacity, 0.0, 1.0));
        // Composed with the frame's own placement (origin, rotation), which
        // is identity for a plain screen view.
        painter_.setTransform(transform, true);
        // Smooth only when magnifying past 1:1; downsampling a huge image with
        // smoothing on is slow and makes little visible difference.
        painter_.setRenderHint(QPainter::SmoothPixmapTransform, std::abs(g[1] * s) > 1.0);
        painter_.drawImage(QPointF(0.0, 0.0), cached->image);
        painter_.restore();
    }
}

void PlanPainter::drawPointClouds()
{
    const int width = widthPixels();
    const int height = heightPixels();
    if (source_.reference == nullptr || width <= 0 || height <= 0) {
        return;
    }

    for (const katana::interop::PointCloudLayer& cloud : source_.reference->pointClouds()) {
        // Hidden in the Reference Data panel, or in this view only.
        if (!cloud.visible || hiddenReference(frame_, cloud.id) || cloud.points.empty()) {
            continue;
        }

        // The display copy depends only on the layer's points and colour mode,
        // so it is built once; only the projection is redone per frame. (A
        // layer whose points were replaced under the same id is caught by its
        // count; invalidateReferences() is the rule for anything else.)
        auto cached = std::find_if(cache_.clouds_.begin(), cache_.clouds_.end(),
                                   [&cloud](const PlanPaintCache::CloudDisplay& entry) {
                                       return entry.id == cloud.id && entry.mode == cloud.colorMode &&
                                              entry.splat.sourceCount == cloud.points.size();
                                   });
        if (cached == cache_.clouds_.end()) {
            // Drop any stale entry for this layer whose mode has changed.
            std::erase_if(cache_.clouds_, [&cloud](const PlanPaintCache::CloudDisplay& entry) {
                return entry.id == cloud.id;
            });

            double minimum = 0.0;
            double maximum = 1.0;
            if (cloud.colorMode == katana::interop::PointColorMode::Intensity) {
                minimum = std::numeric_limits<double>::max();
                maximum = std::numeric_limits<double>::lowest();
                for (const auto& point : cloud.points) {
                    minimum = std::min(minimum, point.intensity);
                    maximum = std::max(maximum, point.intensity);
                }
            } else {
                minimum = cloud.bounds.minZ;
                maximum = cloud.bounds.maxZ;
            }

            std::vector<std::uint32_t> colours;
            colours.reserve(cloud.points.size());
            for (const auto& point : cloud.points) {
                const katana::interop::Rgb rgb =
                    katana::interop::colorForPoint(point, cloud.colorMode, minimum, maximum);
                colours.push_back(qRgb(rgb.r, rgb.g, rgb.b));
            }
            PlanPaintCache::CloudDisplay entry;
            entry.id = cloud.id;
            entry.mode = cloud.colorMode;
            entry.splat = katana::geometry::buildSplatCloud(
                &cloud.points.front().x, &cloud.points.front().y,
                sizeof(katana::pointcloud::PointCloudPoint), cloud.points.size(), colours.data(),
                katana::core::TaskPool::shared());
            cache_.clouds_.push_back(std::move(entry));
            cached = std::prev(cache_.clouds_.end());
        }

        // Splat into an image rather than calling QPainter per point: a
        // QPainter::drawPoint costs microseconds, which at two million points is
        // seconds per frame. The image is the cache's, reused frame to frame;
        // the splat clears and fills only the rows the cloud can reach, in
        // bands across the task pool, and only those rows are composited.
        QImage& layer = cache_.cloudLayer_;
        if (layer.width() != width || layer.height() != height) {
            layer = QImage(width, height, QImage::Format_ARGB32_Premultiplied);
        }
        katana::geometry::SplatView splatView;
        splatView.centre = view_.center;
        splatView.scale = view_.scale;
        splatView.width = width;
        splatView.height = height;
        splatView.radius = std::max(0, static_cast<int>(cloud.pointSize) - 1);
        splatView.pointBudget = options_.cloudPointBudget;
        // Opaque colours: qRgb's alpha is already 0xff, so a written pixel is
        // valid premultiplied ARGB and an unwritten one is transparent.
        const katana::geometry::SplatStats splat = katana::geometry::splatCloud(
            cached->splat, splatView, reinterpret_cast<std::uint32_t*>(layer.bits()),
            static_cast<std::size_t>(layer.bytesPerLine()) / sizeof(std::uint32_t),
            katana::core::TaskPool::shared());
        stats_.cloudPointsInView += splat.pointsInView;
        stats_.cloudPointsDrawn += splat.pointsDrawn;
        if (splat.rowEnd > splat.rowBegin) {
            const QRect rows(0, splat.rowBegin, width, splat.rowEnd - splat.rowBegin);
            painter_.drawImage(rows.topLeft(), layer, rows);
        }
    }
}

void PlanPainter::drawGrid()
{
    const double spacing = cad::gridSpacing(view_.scale);
    const auto firstIndex = [&](double lo) {
        return static_cast<long long>(std::floor(lo / spacing));
    };
    const auto lastIndex = [&](double hi) { return static_cast<long long>(std::ceil(hi / spacing)); };
    const double width = widthPixels();
    const double height = heightPixels();

    // Every fifth line is a major line so that distances can be read off.
    for (long long i = firstIndex(visible_.min.x); i <= lastIndex(visible_.max.x); ++i) {
        painter_.setPen(QPen(i % 5 == 0 ? kGridMajor : kGridMinor, 1));
        const double x = toScreen(Point2(static_cast<double>(i) * spacing, 0.0)).x();
        painter_.drawLine(QPointF(x, 0.0), QPointF(x, height));
    }
    for (long long i = firstIndex(visible_.min.y); i <= lastIndex(visible_.max.y); ++i) {
        painter_.setPen(QPen(i % 5 == 0 ? kGridMajor : kGridMinor, 1));
        const double y = toScreen(Point2(0.0, static_cast<double>(i) * spacing)).y();
        painter_.drawLine(QPointF(0.0, y), QPointF(width, y));
    }
    const QPointF origin = toScreen(Point2(0.0, 0.0));
    painter_.setPen(QPen(kAxis, 1));
    painter_.drawLine(QPointF(origin.x(), 0.0), QPointF(origin.x(), height));
    painter_.drawLine(QPointF(0.0, origin.y()), QPointF(width, origin.y()));
}

// A mesh in plan is its FOOTPRINT - the hull of its vertices - and not its
// triangles: 1 453 meshes of 90 656 triangles arrive from one real archive,
// and drawing those in plan would bury the drawing they are context for. The
// 3D view is where a mesh is looked at.
void PlanPainter::drawMeshFootprints()
{
    if (source_.meshes == nullptr) {
        return;
    }
    for (const katana::cad::SceneMesh& item : *source_.meshes) {
        if (!isShown(item)) {
            continue;
        }
        const auto hull = item.mesh->planHull();
        if (hull.size() < 2) {
            continue;
        }
        const QColor colour(katana::render::redOf(item.flatColor),
                            katana::render::greenOf(item.flatColor),
                            katana::render::blueOf(item.flatColor));
        QColor outline = colour;
        outline.setAlpha(190);
        painter_.setPen(QPen(outline, 1, Qt::DashLine));
        QColor fill = colour;
        fill.setAlpha(40);
        QPolygonF polygon;
        polygon.reserve(static_cast<int>(hull.size()) + 1);
        for (const auto& vertex : hull) {
            polygon << toScreen(vertex);
        }
        // Two points are a wall seen from above: a line, with nothing to fill.
        if (hull.size() == 2) {
            painter_.drawPolyline(polygon);
            continue;
        }
        painter_.setBrush(fill);
        painter_.drawPolygon(polygon);
        painter_.setBrush(Qt::NoBrush);
    }
}

const PlanPainter::Resolved& PlanPainter::resolve(const Entity& entity)
{
    const DisplayKey key = displayKeyOf(entity);
    if (last_ != nullptr && key == lastKey_) {
        return *last_;
    }
    auto found = resolved_.find(key);
    if (found == resolved_.end()) {
        const auto& model = *source_.model;
        const katana::cad::LayerOverrides& overrides =
            frame_.layers != nullptr ? *frame_.layers : noOverrides();
        Resolved r;
        // The layer is resolved ONCE and the visibility rule asked about
        // that (cad::isDrawn without the entity's own flag, which the caller
        // adds), rather than looked up again inside isDrawn.
        r.layer = model.layers.resolve(entity.layer);
        r.layerDrawn = r.layer.shown && !overrides.hides(entity.layer);
        // Through the one resolution chain, so this agrees with the 3D view
        // and so that a named style can change how an entity looks.
        r.display = katana::entity::resolveDisplay(model, entity);
        // One answer to what the linetype NAME draws (decisions D2 and D8): a
        // library linestyle's own strokes, a model linetype's dashes, or a
        // plain line - never a symbol laid along the line as a pattern.
        r.pattern = cad::resolveLinePattern(model, library_, r.display.linetype, r.display.symbol);
        // On paper, white and near-white print black (D7): white is a new
        // layer's colour and a third of the reference mapfile's, and it would
        // vanish into the sheet.
        QColor color = toQColor(paper() && options_.plot != nullptr
                                    ? cad::paperColour(r.display.color, *options_.plot)
                                    : r.display.color);
        // The fading of a locked layer is screen furniture: it says what the
        // user is working on, and a plot printed it in half tone (audit
        // QT-26).
        if (!paper() && r.layer.locked) {
            color.setAlpha(110); // locked layers, and their children, read as background
        }
        // On screen every line is a hairline: a screen has no paper for a
        // line weight to be millimetres of. It is 1.5 px, or with the view's
        // thin-line option a cosmetic pixel, which strokes 5-8x faster
        // because Qt's fast path for antialiased lines needs a width of at
        // most one. On a plot the width is the line weight - "millimetres on
        // paper" - which means what it says.
        double penWidthPixels = 1.5;
        if (paper()) {
            penWidthPixels = r.display.lineWeight * options_.pixelsPerMillimetre;
        } else if (options_.thinLines) {
            penWidthPixels = 1.0;
        }
        r.entityPen = QPen(color, penWidthPixels);
        if (!paper() && options_.thinLines) {
            r.entityPen.setCosmetic(true);
        }
        r.pen = r.entityPen;
        // Dashes are MODEL lengths: a 0.5 m dash stays half a metre of ground
        // at every zoom, so the pixel pattern is recomputed from the view
        // scale. Qt's array is in units of PEN WIDTH, not pixels, which is why
        // the width is passed in rather than assumed. The pattern depends only
        // on the linetype, the view scale and the pen width, so it is built
        // once per paint (dashes_). Only a MODEL linetype dashes the pen: a
        // library linestyle of the same name wins, and its strokes are drawn
        // undashed (D2).
        if (r.pattern.kind == cad::LinetypeKind::ModelLinetype) {
            const auto dashKey = std::make_pair(r.display.linetype, penWidthPixels);
            auto cached = dashes_.find(dashKey);
            if (cached == dashes_.end()) {
                cad::DashOptions dash;
                dash.viewScale = view_.scale;
                const auto dashes = cad::qtDashPattern(*r.pattern.linetype, dash, penWidthPixels);
                cached =
                    dashes_.emplace(dashKey, QList<qreal>(dashes.begin(), dashes.end())).first;
            }
            if (!cached->second.isEmpty()) {
                r.pen.setDashPattern(cached->second);
            }
        }
        // A library definition is painted in the entity's colour and width
        // with a FLAT cap, so a 3 mm dash plots 3 mm rather than 3 mm and a pen
        // width (QPen's square cap); the painter draws its dots round.
        r.target.view = view_;
        r.target.entityPen = r.entityPen;
        r.target.entityPen.setCapStyle(Qt::FlatCap);
        r.target.paper = paper() ? options_.plot : nullptr;
        // resolveHatchPattern from the display already resolved, rather than
        // a second full resolveDisplay of its own.
        r.hatch = cad::resolveHatchPattern(model, r.display);
        if (r.pattern.kind == cad::LinetypeKind::LibraryDefinition) {
            r.linestyle = cache_.definitions_.find(library_, source_.libraryGeneration,
                                                   r.pattern.definition->name);
        }
        found = resolved_.emplace(key, std::move(r)).first;
        ++stats_.displaysResolved;
    }
    lastKey_ = key;
    last_ = &found->second;
    return found->second;
}

const PlanPainter::Stamp& PlanPainter::stampOf(const std::string& symbol, double size)
{
    auto bySymbol = stamps_.find(symbol);
    if (bySymbol == stamps_.end()) {
        bySymbol = stamps_.emplace(symbol, std::vector<std::pair<double, Stamp>>{}).first;
    }
    auto& sizes = bySymbol->second;
    auto found = std::find_if(sizes.begin(), sizes.end(),
                              [size](const auto& entry) { return entry.first == size; });
    if (found == sizes.end()) {
        // At the origin: a stamp's reach and size do not depend on where it
        // is put, and at the origin its strokes are not rounded to the
        // magnitude of a map grid coordinate.
        const cad::StyleDrawing drawing = cad::pointSymbolDrawing(
            cache_.definitions_, library_, source_.libraryGeneration, symbol, Point2(0.0, 0.0),
            size, 0.0, paperScale(), plainMarkHalfWidth());
        Stamp stamp;
        stamp.extent = cad::drawnExtent(drawing);
        stamp.empty = stamp.extent.empty();
        stamp.belowDetail = cad::belowSymbolDetail(drawing, view_.scale);
        sizes.emplace_back(size, stamp);
        found = std::prev(sizes.end());
    }
    return found->second;
}

void PlanPainter::drawEntities()
{
    const auto& model = *source_.model;
    const Box2 visible = visible_;
    const double millimetre = paperScale();

    // How far each style's symbol reaches from the point it is put at, at
    // this frame's scale. A symbol is culled by what it DRAWS, not by its
    // insertion point: one library symbol draws 434 m from its point and the
    // plot stamps up to 388 m, and culling on the point dropped every one
    // whose point was just off screen while its strokes were in view. The index is
    // asked for the view grown by the furthest reach, and each entity then by
    // its own style's.
    std::map<std::string, double, std::less<>> symbolReach;
    double furthestReach = 0.0;
    model.styles.forEach([&](const katana::entity::Style& style) {
        if (style.symbol.empty()) {
            return;
        }
        const Box2 box = cad::drawnExtent(cad::pointSymbolDrawing(
            cache_.definitions_, library_, source_.libraryGeneration, style.symbol,
            Point2(0.0, 0.0), style.symbolSize, 0.0, millimetre, plainMarkHalfWidth()));
        if (box.empty()) {
            return;
        }
        const double reach = std::max(
            {std::abs(box.min.x), std::abs(box.max.x), std::abs(box.min.y), std::abs(box.max.y)});
        symbolReach.emplace(style.name, reach);
        furthestReach = std::max(furthestReach, reach);
    });

    // Through the spatial index. This runs on EVERY repaint
    // - every pan, every zoom - not just on a click, so it is the scan that
    // mattered most. Measured in Release at 500 000 entities zoomed to 1% of
    // the extent: 22.3 ms scanning, 0.090 ms indexed. forEachCandidate falls
    // back to the ordered scan for a zoomed-out repaint, where asking the
    // index for everything would be slower than walking the model once.
    std::vector<katana::geometry::SpatialId> scratch;
    cad::detail::forEachCandidate(
        model, source_.index, visible.inflated(furthestReach), scratch, [&](const Entity& entity) {
            const Resolved& resolved = resolve(entity);
            // cad::isDrawn, with the layer's half answered once per layer.
            if (!entity.visible || !resolved.layerDrawn) {
                return;
            }
            // This box test is NOT the one forEachCandidate already did:
            // queryExtents is deliberately wider than the geometry (an arc offers
            // its centre for snapping), so this is the tighter, drawing-specific
            // filter and removing it would paint entities that are off screen.
            // Except for a dimension, whose DRAWING - label, arrows, extension
            // overshoot - reaches beyond its geometry's box, which holds only the
            // measured points and the dimension line: culling on that box dropped
            // a dimension whose label alone was on screen (audit QT-25). Its
            // query extent is exactly the drawing's box. A style's symbol widens
            // the box by its reach, for the same reason.
            const bool dimension =
                std::holds_alternative<katana::entity::DimensionGeometry>(entity.geometry);
            Box2 drawn = dimension ? cad::detail::queryExtents(model, entity)
                                   : katana::entity::boundingBox(entity.geometry);
            if (!resolved.display.symbol.empty()) {
                if (const auto reach = symbolReach.find(entity.style); reach != symbolReach.end()) {
                    drawn = drawn.inflated(reach->second);
                }
            }
            if (!drawn.intersects(visible)) {
                return;
            }
            ++stats_.entitiesDrawn;
            const auto& display = resolved.display;
            // The selection is screen furniture: it says what the user is
            // working on, and a plot of a drawing printed it in orange dashes
            // (audit QT-26).
            const bool selected = !paper() && source_.selection != nullptr &&
                                  source_.selection->contains(entity.id);
            std::optional<StylePaintTarget> selectedTarget;
            if (selected) {
                const QPen selection(kSelection, 2, Qt::DashLine);
                painter_.setPen(selection);
                selectedTarget = resolved.target;
                selectedTarget->entityPen = selection;
                selectedTarget->entityPen.setCapStyle(Qt::FlatCap);
                selectedTarget->entityPenOnly = true;
            } else {
                painter_.setPen(resolved.pen);
            }
            const StylePaintTarget& target = selected ? *selectedTarget : resolved.target;
            hatch_ = resolved.hatch;
            // Looked up only for a dimension, the one entity that can use it.
            if (dimension) {
                dimensionStyle_ = cad::resolveDimensionStyle(model, entity);
            }
            if (const auto* point = std::get_if<katana::entity::PointGeometry>(&entity.geometry);
                point != nullptr && !display.symbol.empty()) {
                drawSymbol(target, display.symbol, point->position, display.symbolSize);
                return;
            }
            // A library linestyle IS the line, gaps and all, so it REPLACES the
            // plain line rather than being drawn over it. A pattern too fine to
            // see or too long to lay is the plain line, and so is one that is
            // not laid for any other reason: never nothing.
            bool drawnByStyle = false;
            if (resolved.linestyle != nullptr) {
                drawnByStyle = drawLineStyle(target, *resolved.linestyle, entity.geometry);
            }
            // A hatch is painted inside drawGeometry, so an entity carrying one
            // is drawn anyway and puts up with a doubled outline.
            if (!drawnByStyle || hatch_ != nullptr) {
                drawGeometry(entity.geometry);
            }
            // A line whose style names a symbol carries it at EVERY vertex (D8),
            // as the archive import intends: a fence line's posts, a string of
            // drill holes. The line above is drawn as well.
            if (!display.symbol.empty()) {
                for (const Point2& vertex : cad::symbolVertices(entity.geometry)) {
                    drawSymbol(target, display.symbol, vertex, display.symbolSize);
                }
            }
        });
}

// A linestyle runs along whatever plan shape the entity has. An arc and a
// circle are chorded first, because a pattern is laid by distance along a
// path and a path is what a polyline is.
bool PlanPainter::drawLineStyle(const StylePaintTarget& target,
                                const cad::FlatDefinition& definition,
                                const katana::entity::Geometry& geometry)
{
    bool drew = false;
    // Only the repeats that can reach the view are laid, each exactly where
    // it falls on the whole line; a pattern finer than two pixels or longer
    // than the budget is not laid at all, and the caller draws the plain line
    // (audit CAD-04: the tail of a long line used to vanish silently).
    cad::LinestyleOptions options;
    options.paperScale = paperScale();
    options.viewScale = view_.scale;
    options.visible = visible_;
    // A quarter of a pixel, the same accuracy drawGeometry chords to, so a
    // pattern laid along a curve follows the curve that was drawn.
    const double chordTolerance = 0.25 / std::max(view_.scale, 1e-12);
    const auto run = [&](const Polyline2& shape) {
        if (shape.vertices.size() < 2) {
            return;
        }
        const cad::LinestyleLayout laid = cad::styleDrawing(definition, shape, options);
        if (laid.outcome != cad::LinestyleLayout::Outcome::Laid) {
            return; // too fine, too long or degenerate: the caller draws the plain line
        }
        paintStyleDrawing(painter_, laid.drawing, target);
        drew = true;
    };
    std::visit(
        [&](const auto& shape) {
            using T = std::decay_t<decltype(shape)>;
            if constexpr (std::is_same_v<T, Segment2>) {
                run(Polyline2{{shape.start, shape.end}, false});
            } else if constexpr (std::is_same_v<T, Polyline2>) {
                run(shape);
            } else if constexpr (std::is_same_v<T, Arc2>) {
                run(Polyline2{katana::geometry::chordArc(shape, chordTolerance), false});
            } else if constexpr (std::is_same_v<T, Circle2>) {
                run(Polyline2{katana::geometry::chordCircle(shape, chordTolerance), true});
            }
            // A point, a text and a mesh have no line to lay a pattern along.
        },
        geometry);
    return drew;
}

void PlanPainter::drawSymbol(const StylePaintTarget& target, const std::string& symbol,
                             const Point2& centre, double size)
{
    // Where the stamp reaches and whether it is too small to draw are the
    // same wherever it is put, so they are worked out once per symbol and
    // size, and a stamp off screen or blitted from a sprite is never built.
    const Stamp& stamp = stampOf(symbol, size);
    if (stamp.empty) {
        return; // kNoSymbol, or a definition that draws nothing
    }
    const Box2 extent(stamp.extent.min + centre, stamp.extent.max + centre);
    if (!extent.intersects(visible_)) {
        return; // culled by what it draws, not by where it stands
    }
    ++stats_.symbolsStamped;
    if (stamp.belowDetail) {
        // Under three pixels a symbol is a smudge: a dot in its pen says a
        // point is there, for one draw call instead of every stroke.
        QPen dot = target.entityPen;
        dot.setCapStyle(Qt::RoundCap);
        dot.setWidthF(std::max(dot.widthF(), 2.0));
        const QPen previous = painter_.pen();
        painter_.setPen(dot);
        painter_.drawPoint(toScreen(centre));
        painter_.setPen(previous);
        return;
    }
    if (sprites_ && !target.entityPenOnly && stampSprite(target, symbol, centre, size, stamp)) {
        ++stats_.spritesDrawn;
        return;
    }
    // Through the one resolver the previews use: a loaded library definition
    // first, the sixteen built-in shapes after. Rotation is 0 because nothing
    // in the model carries one yet.
    const cad::StyleDrawing drawing = cad::pointSymbolDrawing(
        cache_.definitions_, library_, source_.libraryGeneration, symbol, centre, size, 0.0,
        paperScale(), plainMarkHalfWidth());
    paintStyleDrawing(painter_, drawing, target);
}

// A survey drawing puts the same few symbols in the same few pens at
// thousands of points; stroking each from scratch was a third of a frame
// (14 059 stamps, 267 ms, on the 28k-entity archive). Rasterised once into
// an image at the stamp's quarter-pixel position and blitted onto whole
// device pixels, each stamp after the first is a copy of a few hundred
// pixels, and differs from the stroked one by at most an eighth of a pixel
// of position.
bool PlanPainter::stampSprite(const StylePaintTarget& target, const std::string& symbol,
                              const Point2& centre, double size, const Stamp& stamp)
{
    const QPointF logical = toScreen(centre) + translation_;
    const double deviceX = logical.x() * deviceRatio_;
    const double deviceY = logical.y() * deviceRatio_;
    if (!(std::abs(deviceX) < 1.0e6 && std::abs(deviceY) < 1.0e6)) {
        return false; // no whole-pixel position to speak of; stroke it
    }
    // The nearest quarter pixel: a stamp is off by at most an eighth.
    double wholeX = std::floor(deviceX);
    double wholeY = std::floor(deviceY);
    int phaseX = static_cast<int>(std::lround((deviceX - wholeX) * 4.0));
    int phaseY = static_cast<int>(std::lround((deviceY - wholeY) * 4.0));
    if (phaseX == 4) {
        phaseX = 0;
        wholeX += 1.0;
    }
    if (phaseY == 4) {
        phaseY = 0;
        wholeY += 1.0;
    }
    PlanPaintCache::SpriteKey key;
    key.symbol = symbol;
    key.colour = target.entityPen.color().rgba();
    key.penWidth = target.entityPen.widthF();
    key.cosmetic = target.entityPen.isCosmetic();
    key.size = size;
    key.phaseX = phaseX;
    key.phaseY = phaseY;
    auto found = cache_.sprites_.find(key);
    if (found == cache_.sprites_.end()) {
        PlanPaintCache::Sprite sprite;
        // The stamp's reach in device pixels about its insertion point, plus
        // what the pen and antialiasing add beyond the strokes' own points.
        const double s = view_.scale * deviceRatio_;
        const double pad = std::ceil(0.75 * strokeWidth(target.entityPen) * deviceRatio_ + 2.0);
        const double left = std::ceil(-stamp.extent.min.x * s + pad);
        const double right = std::ceil(stamp.extent.max.x * s + pad);
        const double top = std::ceil(stamp.extent.max.y * s + pad); // y down
        const double bottom = std::ceil(-stamp.extent.min.y * s + pad);
        const double width = left + right + 1.0;
        const double height = top + bottom + 1.0;
        if (width <= kMaximumSpritePixels && height <= kMaximumSpritePixels && width > 0.0 &&
            height > 0.0) {
            QImage image(static_cast<int>(width), static_cast<int>(height),
                         QImage::Format_ARGB32_Premultiplied);
            image.fill(Qt::transparent);
            image.setDevicePixelRatio(deviceRatio_);
            // A view onto the image that puts the origin - where the stamp is
            // built - at device pixel (left, top) plus the phase, so the
            // strokes land in the image exactly as they would have landed on
            // the device relative to the stamp's whole pixel.
            cad::ViewTransform view;
            view.scale = view_.scale;
            view.widthPixels = width / deviceRatio_;
            view.heightPixels = height / deviceRatio_;
            const double anchorX = (left + 0.25 * phaseX) / deviceRatio_;
            const double anchorY = (top + 0.25 * phaseY) / deviceRatio_;
            view.center = Point2((0.5 * view.widthPixels - anchorX) / view_.scale,
                                 (anchorY - 0.5 * view.heightPixels) / view_.scale);
            StylePaintTarget local = target;
            local.view = view;
            const cad::StyleDrawing drawing = cad::pointSymbolDrawing(
                cache_.definitions_, library_, source_.libraryGeneration, symbol,
                Point2(0.0, 0.0), size, 0.0, paperScale(), plainMarkHalfWidth());
            QPainter imagePainter(&image);
            imagePainter.setRenderHints(painter_.renderHints());
            paintStyleDrawing(imagePainter, drawing, local);
            imagePainter.end();
            sprite.image = std::move(image);
            sprite.anchor = QPoint(static_cast<int>(left), static_cast<int>(top));
        }
        // A stamp too large to cache is remembered as such (a null image), so
        // it is not measured again at every point.
        found = cache_.sprites_.emplace(std::move(key), std::move(sprite)).first;
    }
    const PlanPaintCache::Sprite& sprite = found->second;
    if (sprite.image.isNull()) {
        return false;
    }
    const QPointF at((wholeX - sprite.anchor.x()) / deviceRatio_ - translation_.x(),
                     (wholeY - sprite.anchor.y()) / deviceRatio_ - translation_.y());
    painter_.drawImage(at, sprite.image);
    return true;
}

void PlanPainter::strokePolyline(const std::vector<Point2>& vertices, bool closed)
{
    const QPen& pen = painter_.pen();
    // Only a solid pen is clipped: a dash pattern starts at the start of the
    // line, so a line cut at the view's edge would start its dashes there
    // and every dash would move.
    const bool clip = options_.clipLines && !paper() && pen.style() == Qt::SolidLine &&
                      vertices.size() >= 2 && view_.scale > 0.0;
    if (!clip) {
        QPolygonF polygon;
        polygon.reserve(static_cast<int>(vertices.size()) + 1);
        for (const auto& vertex : vertices) {
            polygon << toScreen(vertex);
        }
        if (closed && !vertices.empty()) {
            polygon << toScreen(vertices.front());
        }
        painter_.drawPolyline(polygon);
        return;
    }
    // The view grown by more than the pen can reach past a line's end - a
    // square cap is half the width out and half across, 0.71 of it on the
    // diagonal - and an antialiasing pixel, so a dropped segment, and the
    // cap that replaces a join where one was dropped, lie wholly outside the
    // view and no visible pixel changes.
    const double margin = (strokeWidth(pen) + 2.0) / view_.scale;
    // Liang-Barsky decides which segments reach the view; each that does is
    // kept WHOLE, so its pixels are exactly the unclipped line's (a segment
    // cut at the view's edge rasterises with antialiasing a level or two
    // different along its whole length). Only the segments that miss the
    // view are dropped - nearly all of a long string seen in part.
    katana::geometry::clipPolyline(vertices, closed, visible_.inflated(margin), runs_,
                                   katana::geometry::PolylineClip::WholeSegments);
    if (runs_.empty()) {
        return;
    }
    const std::size_t whole = vertices.size() + (closed ? 1u : 0u);
    if (runs_.size() > 1 || runs_.points.size() != whole) {
        ++stats_.linesClipped;
    }
    // Each run as a polyline of its own, through the same QPainter call the
    // whole line went through: a path of several runs is stroked by another
    // of Qt's code paths, and its antialiasing differs by a level here and
    // there along every edge.
    QPolygonF polygon;
    for (std::size_t run = 0; run < runs_.size(); ++run) {
        polygon.clear();
        polygon.reserve(static_cast<int>(runs_.ends[run] - runs_.begin(run)));
        for (std::size_t i = runs_.begin(run); i < runs_.ends[run]; ++i) {
            polygon << toScreen(runs_.points[i]);
        }
        painter_.drawPolyline(polygon);
    }
}

void PlanPainter::drawGeometry(const katana::entity::Geometry& geometry)
{
    // Arcs are tessellated in model space so that very large radii, where only a
    // sliver is on screen, never hand QPainter coordinates in the millions.
    const auto drawArcPath = [&](const Arc2& arc) {
        const double radiusPixels = arc.radius * view_.scale;
        // Chord count for a sagitta under a quarter pixel, within sane bounds.
        const double stepAngle = radiusPixels > 1.0
                                     ? 2.0 * std::acos(std::max(0.0, 1.0 - 0.25 / radiusPixels))
                                     : katana::math::kPi;
        const int segments = static_cast<int>(
            std::clamp(std::ceil(std::abs(arc.sweep) / std::max(stepAngle, 1e-4)), 8.0, 2048.0));
        arcPoints_.clear();
        arcPoints_.reserve(static_cast<std::size_t>(segments) + 1);
        for (int i = 0; i <= segments; ++i) {
            arcPoints_.push_back(arc.pointAt(static_cast<double>(i) / segments));
        }
        strokePolyline(arcPoints_, false);
    };

    struct Visitor {
        PlanPainter& self;
        const decltype(drawArcPath)& arcPath;

        void operator()(const katana::entity::PointGeometry& g) const
        {
            const QPointF p = self.toScreen(g.position);
            const double r = self.markPixels();
            self.painter_.drawLine(p + QPointF(-r, 0), p + QPointF(r, 0));
            self.painter_.drawLine(p + QPointF(0, -r), p + QPointF(0, r));
        }
        void operator()(const Segment2& g) const
        {
            self.painter_.drawLine(self.toScreen(g.start), self.toScreen(g.end));
        }
        void operator()(const Arc2& g) const { arcPath(g); }
        void operator()(const Circle2& g) const
        {
            arcPath(Arc2{g.center, g.radius, 0.0, katana::math::kTwoPi});
        }
        void operator()(const Polyline2& g) const
        {
            if (g.closed && !g.vertices.empty() && self.hatch_ != nullptr) {
                // The fill goes down before the boundary, so the outline stays
                // crisp over its own hatching instead of being half covered.
                // It needs the whole boundary, visible or not.
                QPolygonF polygon;
                polygon.reserve(static_cast<int>(g.vertices.size()));
                for (const auto& vertex : g.vertices) {
                    polygon << self.toScreen(vertex);
                }
                self.drawHatch(g, polygon);
            }
            self.strokePolyline(g.vertices, g.closed);
        }
        void operator()(const katana::entity::TextGeometry& g) const
        {
            self.drawText(g.position, g.text, g.height, g.rotation);
        }
        void operator()(const katana::entity::DimensionGeometry& g) const
        {
            // Through the shared builder, so this draws exactly what the 3D
            // view draws and exactly what the cull box covers. Everything is in
            // MODEL units: a dimension is part of the drawing, not an overlay
            // on it, so it plots at its size on the ground.
            const auto drawing = cad::buildDimension(g, self.dimensionStyle_);
            if (drawing.empty()) {
                return;
            }
            const auto line = [this](const Segment2& segment) {
                self.painter_.drawLine(self.toScreen(segment.start), self.toScreen(segment.end));
            };
            for (const Segment2& segment : drawing.extensionLines) {
                line(segment);
            }
            line(drawing.dimensionLine);
            for (const Segment2& stroke : drawing.arrowStrokes) {
                line(stroke);
            }
            for (const std::vector<Point2>& fill : drawing.arrowFills) {
                QPolygonF polygon;
                polygon.reserve(static_cast<int>(fill.size()));
                for (const Point2& point : fill) {
                    polygon << self.toScreen(point);
                }
                const QBrush previous = self.painter_.brush();
                self.painter_.setBrush(self.painter_.pen().color());
                self.painter_.drawPolygon(polygon);
                self.painter_.setBrush(previous);
            }
            self.drawText(drawing.textAnchor, drawing.text, drawing.textHeight,
                          drawing.textRotation);
        }
    };
    std::visit(Visitor{*this, drawArcPath}, geometry);
}

void PlanPainter::drawAlignments()
{
    const auto& model = *source_.model;
    if (model.alignments.empty() || !(view_.scale > 0.0)) {
        return;
    }
    // An overlay's sizes are the medium's: pixels on screen, millimetres of
    // paper on a plot (kAlignment*PaperMillimetres), so a sheet's chainages
    // read the same at every resolution.
    const double linePen = markSize(2.0, kAlignmentLinePaperMillimetres);
    const double tickPen = markSize(1.0, kAlignmentTickPenPaperMillimetres);
    const double tickPixels = markSize(6.0, kAlignmentTickPaperMillimetres);
    const double textPixels = markSize(11.0, kAlignmentTextPaperMillimetres);
    // About one "0+000.00" at the text size: 70 px at 11 px.
    const double minimumLabelGapPixels = 70.0 / 11.0 * textPixels;
    // Half a pixel: finer cannot be seen, coarser shows facets on tight curves.
    const double tolerance = 0.5 / view_.scale;
    const double tick = tickPixels / view_.scale; // screen-constant, like the snap marker
    const double height = textPixels / view_.scale;

    for (const katana::entity::Alignment& alignment : model.alignments.all()) {
        // Solved per repaint. A document has a handful of alignments and the
        // solve is a few spiral end-points; caching it would need invalidation
        // on every edit for no measurable gain. Measure before changing this.
        const auto solved = katana::geometry::solveAlignment(alignment.horizontal);
        if (!solved) {
            continue; // the model refused it on the way in; nothing to draw
        }
        const Polyline2 line = solved->toPolyline(tolerance);
        if (line.vertices.size() < 2) {
            continue;
        }
        Box2 box;
        for (const Point2& vertex : line.vertices) {
            box.expand(vertex);
        }
        if (!box.inflated(tick * 4.0).intersects(visible_)) {
            continue;
        }

        QPolygonF polygon;
        polygon.reserve(static_cast<int>(line.vertices.size()));
        for (const Point2& vertex : line.vertices) {
            polygon << toScreen(vertex);
        }
        painter_.setPen(QPen(kAlignment, linePen));
        painter_.drawPolyline(polygon);

        painter_.setPen(QPen(kAlignment, tickPen));
        // Key stations bunch up - a 10 m spiral puts TS and SC ten metres
        // apart - and their labels then print over one another into a smear
        // nobody can read. A label is skipped when it would land within its
        // own length of the last one drawn; the TICK is always drawn, because
        // the tick is the information and the label only names it.
        std::optional<QPointF> lastLabel;
        for (const double station : solved->keyStations()) {
            const auto left = solved->pointAtStationOffset(station, tick);
            const auto right = solved->pointAtStationOffset(station, -tick);
            const auto direction = solved->directionAtStation(station);
            const auto label = solved->pointAtStationOffset(station, tick * 1.6);
            if (!left || !right || !direction || !label) {
                continue;
            }
            painter_.drawLine(toScreen(*left), toScreen(*right));
            const QPointF at = toScreen(*label);
            if (lastLabel.has_value() && QLineF(*lastLabel, at).length() < minimumLabelGapPixels) {
                continue;
            }
            lastLabel = at;
            drawText(*label, formatStation(station), height, *direction);
        }
        if (const auto start = solved->pointAtStationOffset(solved->startStation(), -tick * 3.0)) {
            const auto direction = solved->directionAtStation(solved->startStation());
            drawText(*start, alignment.name, height * 1.3, direction.value_or(0.0));
        }
    }
}

void PlanPainter::drawHatch(const Polyline2& boundary, const QPolygonF& screen)
{
    if (hatch_ == nullptr) {
        return;
    }
    cad::HatchOptions options;
    options.viewScale = view_.scale;
    const QColor color = painter_.pen().color();

    switch (cad::hatchDrawing(*hatch_, options)) {
    case cad::HatchDrawing::None:
        return;
    case cad::HatchDrawing::Solid: {
        // On screen at partial opacity rather than flat: a solid fill in the
        // entity's own colour hides the drawing underneath it, and at this zoom
        // the user is looking at the layout, not at the fill. On paper a solid
        // fill is what it says - a plot printed it at a third of its colour.
        QColor fill = color;
        if (!paper()) {
            fill.setAlpha(90);
        }
        painter_.fillPath(
            [&] {
                QPainterPath path;
                path.addPolygon(screen);
                path.closeSubpath();
                return path;
            }(),
            fill);
        return;
    }
    case cad::HatchDrawing::Lines:
        break;
    }

    // Hatch lines are always solid and fine, whatever the boundary is drawn
    // with: a dashed hatch of a dashed boundary is unreadable, and no CAD
    // package draws one. On screen a hairline; on paper the finest standard
    // pen, since a PDF hairline is one device pixel and its width on the
    // page would follow the resolution.
    const QPen previous = painter_.pen();
    painter_.setPen(QPen(color, paper() ? kHatchLinePaperMillimetres * options_.pixelsPerMillimetre
                                        : 0.0));
    for (const Segment2& line : cad::hatchSegments(boundary, *hatch_)) {
        painter_.drawLine(toScreen(line.start), toScreen(line.end));
    }
    painter_.setPen(previous);
}

void PlanPainter::drawText(const Point2& position, const std::string& text, double height,
                           double rotation)
{
    const double pixels = height * view_.scale;
    const QPointF anchor = toScreen(position);
    if (pixels < 3.0) {
        // Too small to read: a stroke along the baseline keeps it discoverable.
        const double width = 0.6 * pixels * static_cast<double>(text.size());
        painter_.drawLine(anchor,
                          anchor + QPointF(std::cos(rotation), -std::sin(rotation)) * width);
        return;
    }
    const double size = std::min(pixels, kMaximumTextPixels);
    painter_.save();
    painter_.translate(anchor);
    painter_.rotate(-rotation * katana::math::kRadToDeg); // screen y points down
    if (paper()) {
        // Exactly `size` tall: the one paper font, scaled.
        painter_.setFont(paperFont());
        const double scale = size / kPaperFontReferencePixels;
        painter_.scale(scale, scale);
    } else {
        painter_.setFont(fontFor(size));
    }
    painter_.drawText(QPointF(0.0, 0.0), QString::fromStdString(text));
    painter_.restore();
}

// ---- entry points ---------------------------------------------------------------------

PlanSource planSourceOf(const katana::cad::Document& document)
{
    PlanSource source;
    source.model = &document.model();
    source.library = &document.styleLibrary();
    source.libraryGeneration = document.libraryGeneration();
    source.index = &document.spatialIndex();
    source.selection = &document.selection();
    return source;
}

PlanPaintStats paintPlan(QPainter& painter, const PlanSource& source, const PlanFrame& frame,
                         const PlanPaintOptions& options, PlanPaintCache& cache)
{
    PlanPainter planPainter(painter, source, frame, options, cache);
    return planPainter.paint();
}

void paintPlanGeometry(QPainter& painter, const PlanFrame& frame, const PlanPaintOptions& options,
                       PlanPaintCache& cache, const katana::entity::Geometry& geometry,
                       const katana::entity::DimensionStyle& dimensionStyle)
{
    const PlanSource nothing;
    PlanPainter planPainter(painter, nothing, frame, options, cache);
    planPainter.geometry(geometry, dimensionStyle);
}

Box2 visibleBox(const PlanFrame& frame)
{
    const cad::ViewTransform& view = frame.transform;
    if (frame.rotation == 0.0) {
        return view.visibleWorldBounds();
    }
    // The viewport's corners turned back by the rotation about its centre,
    // then into the model: the box of those four points is what the rotated
    // rectangle can show. The unrotated visibleWorldBounds would leave out
    // the corners of a twisted viewport - exactly where a sheet's drawing
    // runs into its frame.
    const double halfWidth = 0.5 * view.widthPixels;
    const double halfHeight = 0.5 * view.heightPixels;
    const double c = std::cos(frame.rotation);
    const double s = std::sin(frame.rotation);
    Box2 box;
    for (const auto& [dx, dy] : {std::pair{-halfWidth, -halfHeight}, std::pair{halfWidth, -halfHeight},
                                 std::pair{halfWidth, halfHeight}, std::pair{-halfWidth, halfHeight}}) {
        // A device offset (dx, dy) is where the painter's rotation by -angle
        // (y down) put the unrotated offset R(angle) (dx, dy) in the same y-down
        // pixels: undo it by turning the other way.
        const double ux = c * dx - s * dy;
        const double uy = s * dx + c * dy;
        box.expand(view.screenToWorld(Point2(halfWidth + ux, halfHeight + uy)));
    }
    return box;
}

Box2 planDrawnBounds(const PlanSource& source, const katana::cad::LayerOverrides& layers,
                     const std::set<std::uint64_t>& hiddenReferences)
{
    Box2 bounds;
    if (source.model == nullptr) {
        return bounds;
    }
    // What THIS view draws, through the one visibility rule with this view's
    // hidden layers: the entities' own bounds counted every entity, so a
    // layer hidden here - or a hidden stray far away - still pulled the frame
    // out to it.
    bounds = cad::drawnExtent(*source.model, layers);
    // Everything the user can see, so imported imagery and point clouds
    // count. A drawing that is empty except for an orthophoto would otherwise
    // fit an empty box and leave the photo off screen. Layer by layer rather
    // than ReferenceData::visibleBounds, because a layer hidden in this view
    // is not seen here.
    if (source.reference != nullptr) {
        for (const katana::interop::RasterOverlay& raster : source.reference->rasters()) {
            if (raster.visible && !hiddenReferences.contains(raster.id)) {
                bounds.expand(raster.worldBounds());
            }
        }
        for (const katana::interop::PointCloudLayer& cloud : source.reference->pointClouds()) {
            if (cloud.visible && !hiddenReferences.contains(cloud.id)) {
                bounds.expand(cloud.worldBounds());
            }
        }
    }
    // A mesh is drawn in plan as its footprint, so it is seen and counts. A
    // drawing that is nothing but meshes framed an empty box before.
    if (source.meshes != nullptr) {
        for (const katana::cad::SceneMesh& item : *source.meshes) {
            if (isShown(item)) {
                const katana::math::AABB space = item.mesh->bounds();
                if (!space.empty()) {
                    bounds.expand(Point2(space.min.x, space.min.y));
                    bounds.expand(Point2(space.max.x, space.max.y));
                }
            }
        }
    }
    // Alignments are drawn but are not entities, so they are in no entity
    // bound. Found by looking at a screenshot: the sample's access road ran off
    // the bottom of a view that claimed to show everything.
    for (const katana::entity::Alignment& alignment : source.model->alignments.all()) {
        if (const auto solved = katana::geometry::solveAlignment(alignment.horizontal)) {
            // A metre is fine enough for a bounding box; the curve cannot
            // stray further than that from its chords.
            for (const Point2& vertex : solved->toPolyline(1.0).vertices) {
                bounds.expand(vertex);
            }
        }
    }
    return bounds;
}

PdfResolution pdfResolutionFor(double dpi)
{
    PdfResolution result;
    // The nearest whole resolution, so the scale that corrects it is within
    // half a dot of 1 and the writer's own units are as fine as asked for.
    result.resolution = std::max(1, static_cast<int>(std::lround(dpi)));
    result.scale = static_cast<double>(result.resolution) / dpi;
    return result;
}

katana::core::Result<PlanFrame> sheetFrame(const katana::cad::PlotSettings& settings)
{
    auto sheet = cad::sheetFor(settings);
    if (!sheet) {
        return sheet.error();
    }
    // The printable area in the sheet's device pixels. The margins are equal
    // all round, so the area's centre is the paper's and the sheet's
    // transform, sized to the area and moved in by one margin, maps every
    // model point to the same device pixel it did over the whole paper.
    const double margin = settings.marginMm * sheet->pixelsPerMillimetre;
    PlanFrame frame;
    frame.transform = sheet->view;
    frame.transform.resize(sheet->widthPixels - 2.0 * margin, sheet->heightPixels - 2.0 * margin);
    frame.origin = QPointF(margin, margin);
    frame.clip = true;
    return frame;
}

katana::core::Status plotPlanToPdf(const QString& path, const katana::cad::PlotSettings& settings,
                                   const PlanSource& source,
                                   const katana::cad::LayerOverrides& layers,
                                   const std::set<std::uint64_t>& hiddenReferences,
                                   PlanPaintCache& cache, const QString& title)
{
    auto frame = sheetFrame(settings);
    if (!frame) {
        return frame.error();
    }
    frame->layers = &layers;
    frame->hiddenReferences = &hiddenReferences;
    const PdfResolution resolution = pdfResolutionFor(settings.dpi);
    QPdfWriter writer(path);
    writer.setResolution(resolution.resolution);
    writer.setTitle(title.isEmpty() ? QFileInfo(path).completeBaseName() : title);
    writer.setCreator(QStringLiteral("Katana"));
    const cad::PaperDimensions paper = cad::paperDimensions(settings.paper, settings.landscape);
    writer.setPageSize(QPageSize(QSizeF(paper.widthMm, paper.heightMm), QPageSize::Millimeter));
    // The sheet transform owns the margins; the writer's would shift the page.
    writer.setPageMargins(QMarginsF(0.0, 0.0, 0.0, 0.0));
    QPainter painter(&writer);
    if (!painter.isActive()) {
        return katana::core::makeError(katana::core::ErrorCode::FileExportFailure,
                                       "could not open the PDF for writing", path.toStdString());
    }
    // The sheet is laid out in pixels of exactly settings.dpi; the writer's
    // whole-number resolution is brought to it by one scale.
    if (resolution.scale != 1.0) {
        painter.scale(resolution.scale, resolution.scale);
    }
    PlanPaintOptions options;
    options.medium = PlanMedium::Paper;
    options.pixelsPerMillimetre = cad::millimetresToPixels(1.0, settings.dpi);
    options.plot = &settings;
    painter.setRenderHint(QPainter::Antialiasing, true);
    (void)paintPlan(painter, source, *frame, options, cache);
    if (!painter.end()) {
        return katana::core::makeError(katana::core::ErrorCode::FileExportFailure,
                                       "could not finish the PDF", path.toStdString());
    }
    return {};
}

} // namespace katana::qt
