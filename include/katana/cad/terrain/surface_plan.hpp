#pragma once

// A surface in the plan view, in its display style (docs/surface_display.md):
// what the plan painter (src/katana_qt/terrain/surface_plan_painter.*) lays
// down and strokes for ONE surface seen through ONE view. Culled to what the
// view shows and drawn to a level of detail, so that a 400 000-triangle
// surface pans as a drawing does. The plan view, a plot and every sheet
// viewport draw a surface through it, beneath the drawing's entities, in
// this order:
//
//   faces       an IMAGE of the part of the surface in view: each pixel the
//               colour of the surface under its centre - flat, by elevation
//               (smooth, or banded exactly to the pixel), by slope or by
//               aspect - at the style's opacity. An image rather than a
//               polygon a triangle: zoomed out, a dense TIN has many
//               triangles to a pixel, and a polygon each would be most of a
//               frame for colour nobody can see.
//   triangles   each edge once, where it is at least kPlanEdgePixels long in
//               the view. A triangle whose every edge is shorter is not
//               stroked at all but tinted into the image: too dense to show
//               its edges, as the 3D view fades them out below the same size.
//   contours    traced once per surface and contour settings and kept
//               (SurfacePlanCache), culled to the view, thinned to the pixel;
//               the minors left out where they would lie closer than
//               kPlanContourPixels apart, and the majors likewise.
//   breaklines  the constrained edges inside the surface, joined into lines.
//   boundary    the rim of the surface - its outside and its holes - as rings.
//   points      the vertices: marks where they lie kPlanPointPixels apart or
//               more, else single pixels in the image.
//   labels      the level along each major contour, spaced along it.
//
// Plain data in, plain data out, no Qt: what is drawn is decided here, where
// it is tested, and the painter only draws it.

#include <cstddef>
#include <cstdint>
#include <memory>
#include <optional>
#include <span>
#include <string>
#include <vector>

#include "katana/cad/terrain/surface_colouring.hpp"
#include "katana/cad/terrain/surface_style.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::core {
class TaskPool;
}

namespace katana::cad {

// ---- the level of detail, in pixels of the view ------------------------------------------
//
// A pixel of the view is a logical pixel on screen and, on paper,
// kPlanPaperPixelMillimetres of the page (the painter converts).

// An edge shorter than this is not stroked. The 3D view's edges are gone
// below the same 4 px (SceneBuilder::fadeEdges, docs/render.md "Surfaces"):
// a mesh of one-pixel lines four pixels apart already inks half its area and
// reads as a sheet, not as triangles.
inline constexpr double kPlanEdgePixels = 4.0;
// A triangle too dense to stroke is tinted in the triangle colour at this
// opacity: about what its edges would ink at the threshold (two one-pixel
// edges a triangle across four pixels: 2 x 4 / (4 x 4 / 2) is all of it;
// a third of that keeps the faces beneath readable).
inline constexpr double kPlanDenseTriangleAlpha = 0.35;
// Two contours of one kind closer than this on average are not drawn: a
// band of lines that close is ink, and the majors (or the faces) say more.
inline constexpr double kPlanContourPixels = 3.0;
// Vertices closer than this on average are pixels in the image, not marks.
inline constexpr double kPlanPointPixels = 4.0;
// A drawn line keeps no vertex nearer than this to the one kept before it.
inline constexpr double kPlanSimplifyPixels = 0.5;
// Contour labels: one every kPlanLabelSpacingPixels along a major contour,
// none on a contour shorter than kPlanLabelMinimumPixels, kept
// kPlanLabelMarginPixels inside the view, and at most kPlanMaximumLabels a
// surface a paint (more could not be read).
inline constexpr double kPlanLabelSpacingPixels = 360.0;
inline constexpr double kPlanLabelMinimumPixels = 90.0;
inline constexpr double kPlanLabelMarginPixels = 12.0;
inline constexpr std::size_t kPlanMaximumLabels = 400;
// Contours whose tracing would make more points than this are refused for
// the plan, saying why (SurfacePlan::problem): an interval far too fine for
// the relief would fill memory and the view with ink. Estimated, before
// tracing, from how many levels cross each triangle.
inline constexpr std::size_t kPlanMaximumContourPoints = 4'000'000;
// On paper a view pixel is this much of the page: a screen pixel's size at
// 96 dpi, so a plot keeps the level of detail the screen showed at the same
// scale.
inline constexpr double kPlanPaperPixelMillimetres = 25.4 / 96.0;

// ---- how it is looked at -----------------------------------------------------------------

struct SurfacePlanView {
    // The part of the model the view shows (the plan painter's visibleBox):
    // what is culled to.
    katana::geometry::Box2 visible{};
    // Model units per pixel of the view: what the level of detail measures.
    double unitsPerPixel = 1.0;
    // The faces' image (planImageGrid): the model box it covers and its size
    // in pixels. No pixels: no image, and what would go into one - faces,
    // dense triangles, dense points - is not drawn.
    katana::geometry::Box2 imageBox{};
    int imageWidth = 0;
    int imageHeight = 0;
    double labelSpacingPixels = kPlanLabelSpacingPixels;
    // The threads the image is painted on; null: TaskPool::shared(). The
    // image is the same at any count.
    katana::core::TaskPool* pool = nullptr;
};

// The pixel grid of a surface's image: `bounds` (the surface's plan box) as
// far as `visible` shows it, in pixels `unitsPerPixel` wide lined up with the
// visible box's top-left corner - the view's first pixel, so on an unturned
// screen each image pixel is a device pixel - and coarsened evenly when it
// would be more than `pixelCap` pixels. Empty when they do not meet.
struct SurfacePlanImageGrid {
    katana::geometry::Box2 box{};
    int width = 0;
    int height = 0;
};
[[nodiscard]] SurfacePlanImageGrid planImageGrid(const katana::geometry::Box2& visible,
                                                 const katana::geometry::Box2& bounds,
                                                 double unitsPerPixel, std::size_t pixelCap);

// ---- what is drawn -----------------------------------------------------------------------

// Premultiplied 0xAARRGGBB, row 0 at the top (the box's largest y): the
// layout of QImage::Format_ARGB32_Premultiplied, so the painter wraps it
// without a copy.
struct SurfacePlanImage {
    katana::geometry::Box2 box{};
    int width = 0;
    int height = 0;
    std::vector<std::uint32_t> pixels{};
    [[nodiscard]] bool empty() const { return width <= 0 || height <= 0 || pixels.empty(); }
};

struct SurfacePlanContour {
    double level = 0.0;
    bool major = false;
    // A run of the contour as far as the view shows it, thinned; a closed
    // contour seen whole repeats its first point at its end.
    std::vector<katana::geometry::Point2> points{};
};

struct SurfacePlanLabel {
    katana::geometry::Point2 at{};
    // Of the contour there, counter-clockwise from +x in the model. The
    // painter turns the text upright on the device.
    double angle = 0.0;
    std::string text{};
};

struct SurfacePlanStats {
    std::size_t trianglesInView = 0;
    std::size_t edgesStroked = 0;
    // Not stroked: tinted into the image (kPlanEdgePixels).
    std::size_t trianglesTinted = 0;
    // Pixels of the image the surface covers.
    std::size_t imagePixels = 0;
    std::size_t pixelsCovered = 0;
    std::size_t contourLevels = 0; // traced for the style, whether drawn or not
    std::size_t contourRuns = 0;
    std::size_t contourVertices = 0;
    bool minorContoursHidden = false;
    bool majorContoursHidden = false;
    std::size_t labels = 0;
    std::size_t pointsMarked = 0; // as marks
    std::size_t pointsInImage = 0; // as pixels
    std::size_t boundaryRuns = 0;
    std::size_t breaklineRuns = 0;
};

struct SurfacePlan {
    std::string name{};
    SurfacePlanImage image{};
    katana::entity::Color triangleColour{};
    std::vector<katana::geometry::Segment2> triangles{};
    katana::entity::Color minorColour{};
    katana::entity::Color majorColour{};
    std::vector<SurfacePlanContour> contours{}; // minors first, then majors
    katana::entity::Color breaklineColour{};
    std::vector<std::vector<katana::geometry::Point2>> breaklines{};
    katana::entity::Color boundaryColour{};
    std::vector<std::vector<katana::geometry::Point2>> boundary{};
    katana::entity::Color pointColour{};
    std::vector<katana::geometry::Point2> points{};
    std::vector<SurfacePlanLabel> labels{};
    // What the faces' colours mean, when the style colours them.
    std::optional<SurfaceLegend> legend{};
    SurfacePlanStats stats{};
    // Why part of the style was not drawn (contours refused); empty when
    // all of it was.
    std::string problem{};
};

// ---- what is kept between paints ---------------------------------------------------------

// Per surface: its cull grid, its rim and breaklines joined into lines, its
// mean slope, and its contours for the settings last drawn. Keyed on the
// surface itself - shared and immutable, so an edited surface is a new one -
// and kept alive while kept. One per view or plotting thread, never shared
// between threads; a copy starts empty and works out again what it needs.
class SurfacePlanCache {
  public:
    SurfacePlanCache();
    ~SurfacePlanCache();
    SurfacePlanCache(const SurfacePlanCache&);
    SurfacePlanCache& operator=(const SurfacePlanCache&);
    SurfacePlanCache(SurfacePlanCache&&) noexcept;
    SurfacePlanCache& operator=(SurfacePlanCache&&) noexcept;

    // Keeps only the surfaces in `alive`: what a paint draws, so a surface
    // removed from the drawing is let go.
    void retain(std::span<const katana::terrain::TinSurface* const> alive);
    void clear();
    [[nodiscard]] std::size_t surfaceCount() const;
    // How many times contours have been traced and cull grids built since
    // construction: what proves a second paint reuses them.
    [[nodiscard]] std::size_t contourTracings() const;
    [[nodiscard]] std::size_t gridsBuilt() const;

    struct Impl;

  private:
    std::unique_ptr<Impl> impl_;
    friend Impl& planCacheImpl(SurfacePlanCache& cache);
};

// The triangles of `surface` whose plan box meets `box`, ascending: what a
// paint culls the surface to, through the cache's grid.
[[nodiscard]] std::vector<std::uint32_t>
planTrianglesInBox(const std::shared_ptr<const katana::terrain::TinSurface>& surface,
                   const katana::geometry::Box2& box, SurfacePlanCache& cache);
// The same found by testing every triangle: what the grid is held to.
[[nodiscard]] std::vector<std::uint32_t>
planTrianglesInBoxExhaustive(const katana::terrain::TinSurface& surface,
                             const katana::geometry::Box2& box);

// `surface` in `style` through `view`. A surface with no triangles, or none
// in view, draws nothing; whether the surface is visible is the caller's
// question (NamedSurface::visible).
[[nodiscard]] SurfacePlan surfacePlan(const katana::terrain::NamedSurface& surface,
                                      const SurfaceDisplayStyle& style, const SurfacePlanView& view,
                                      SurfacePlanCache& cache);

// The colouring the plan view gives `surface`'s faces in `style` (its
// planFaces), with the range the surface spans where the style gives none.
// What surfacePlan colours the image by and what its legend says.
[[nodiscard]] SurfaceColouring planColouring(const katana::terrain::TinSurface& surface,
                                             const SurfaceDisplayStyle& style);

} // namespace katana::cad
