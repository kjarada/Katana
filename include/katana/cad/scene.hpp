#pragma once

// Turns the document into draw lists (Rule 3).
//
// This is the only place that knows about both an Entity and a renderer, and
// it is deliberately one-directional: it READS the model and WRITES primitives.
// Nothing here can modify the document, and the renderer below it cannot see
// the document at all, so the renderer can never become a second source of
// truth however it is later reimplemented.
//
// Curves are tessellated here rather than in the renderer, because how finely
// an arc is chorded is a modelling decision (it depends on the view scale and
// on the tolerance the drawing is held to), not a rasterisation one.
//
// The scene is built in LAYERS (SceneLayers), each rebuilt only when what it
// depends on changes: a selection click rebuilds the selection overlay and not
// the 229k-triangle surface under it. build() still makes the whole scene as
// one list for a caller that wants that.

#include <cstdint>
#include <string>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/layer_overrides.hpp"
#include "katana/cad/selection.hpp"
#include "katana/cad/selection_style.hpp"
#include "katana/geometry/mesh.hpp"
#include "katana/render/camera.hpp"
#include "katana/render/draw_list.hpp"
#include "katana/render/rasterizer.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::cad {

using katana::geometry::Point2;

// How a surface is drawn.
enum class SurfaceStyle {
    Hidden,
    Wireframe,       // triangle edges only
    Shaded,          // filled triangles, lit
    ShadedWithEdges, // both; the edges fade out as the triangles shrink on screen
    // The default: ShadedWithEdges up to kDenseSurfaceTriangles, Shaded above.
    // A dense TIN's edges are all the eye can see at any useful zoom - they
    // covered 73% of the pixels of a 229k-triangle survey, which drew as a
    // black slab - and costing a line per edge they doubled the frame.
    Automatic,
};

// Past this many triangles an Automatic surface is drawn Shaded.
inline constexpr std::size_t kDenseSurfaceTriangles = 50000;

// What decides a surface triangle's colour.
enum class SurfaceColoring {
    Flat,      // one colour for the whole surface
    // One ramp for EVERY visible surface coloured by elevation, between the
    // lowest and highest of them, so a height is the same colour on every
    // surface. It was per surface, and two overlapping surfaces then gave one
    // elevation two colours (green and orange speckle where they met).
    Elevation,
    Slope      // green to red by steepness
};

struct SceneSurface {
    std::string name;
    const katana::terrain::TinSurface* surface = nullptr;
    SurfaceStyle style = SurfaceStyle::Automatic;
    SurfaceColoring coloring = SurfaceColoring::Elevation;
    katana::render::Rgba flatColor = katana::render::rgba(170, 170, 160);
    bool visible = true;
};

// A mesh in the session: a .12da trimesh, drawn like a surface but NOT one - it
// may be closed or overhang, so it is never sampled for a height and carries
// no elevation ramp. Colouring is flat or per face, because that is what the
// file gives.
struct SceneMesh {
    std::string name;
    const katana::geometry::TriangleMesh* mesh = nullptr;
    // Shaded, NOT ShadedWithEdges. A mesh has no neighbour table, so every
    // edge of every face is drawn and each line costs two triangles. Measured
    // on `test trimishes complex.12da` (266 meshes, 126 536 faces) in Release
    // on this machine, at the framing the import leaves: with edges 886 504
    // triangles rasterised in 24.9 ms, without them 127 288 in 18.0 ms. On a
    // mesh of this density the edges also read as noise. The style is per
    // mesh, so a user who wants to see the triangulation of one asks for it.
    SurfaceStyle style = SurfaceStyle::Shaded;
    katana::render::Rgba flatColor = katana::render::rgba(190, 170, 140);
    // Empty, or one per face. A face whose colour the file did not give
    // takes flatColor, so a partly coloured mesh is still wholly drawn.
    std::vector<katana::render::Rgba> faceColors;
    bool visible = true;
};

// Where drawing geometry is put in z.
enum class LineworkHeights {
    // A vertex with a height of its own (entity::heightsOf: a 3D string's
    // `elevations`, a point's `elevation`) is drawn at it; one without is
    // draped on the surface under it; one with no surface under it goes on
    // the datum. The default.
    HeightsThenDrape,
    Heights, // own heights, else the datum
    Drape,   // on the surface, else the datum, whatever heights it carries
    Datum,   // everything flat on the datum
};

struct SceneOptions {
    bool drawEntities = true;
    bool drawGrid = true;

    LineworkHeights linework = LineworkHeights::HeightsThenDrape;
    // The datum is the lowest visible surface or mesh; with neither, the
    // lowest height any entity carries; with none of those, this. Linework
    // with no height and nothing under it goes on the datum, and so does the
    // grid. It used to be the z of ALL linework, and the app never set it, so
    // a drawing at 25-39 m floated through its surface at 0.
    double entityElevation = 0.0;
    // Pixel footprints towards the eye (DrawLine::depthBias): what lets
    // linework draped on a surface win against it, and what is too small to
    // show a line through a building.
    float entityDepthBias = 1.5f;
    // The selection overlay draws the selected entities again, over the
    // drawing, and must win the tie with their own first drawing; its casing
    // sits between the two (selection_style.hpp).
    float selectionDepthBias = kSelectionCoreBias;
    float edgeDepthBias = 1.0f;

    // Vertical exaggeration. Applied to Z about `exaggerationDatum` as the
    // scene is built, so the camera, the picking and the framing all agree
    // about where things are. 1 is no exaggeration.
    double verticalExaggeration = 1.0;
    double exaggerationDatum = 0.0;

    // Sagitta tolerance for chording arcs and circles, in model units. The
    // same rule the 2D viewport uses, so the two views agree.
    double chordTolerance = 0.01;

    katana::render::Rgba defaultColor = katana::render::rgba(220, 220, 220);
    // The one selection colour (selection_style.hpp). It was (255, 190, 60),
    // beside the BOUNDARY layer's (255, 213, 79) and the top of the elevation
    // ramp, and apart from the plan view's.
    katana::render::Rgba selectionColor = kSelectionColor;
    // Whether the selection overlay draws a selected entity on a layer the
    // view hides (`layers`) and the document shows, as a ghost: one faint line
    // (selection_style.hpp). Off unless the view asks - its own switch,
    // ViewState::selectionGhosts.
    bool selectionGhosts = false;
    katana::render::Rgba gridColor = katana::render::rgba(70, 72, 82);
    katana::render::Rgba gridMajorColor = katana::render::rgba(92, 95, 108);
    // What the grid fades into at its rim: the view's background.
    katana::render::Rgba gridFadeColor = katana::render::rgba(28, 30, 36);
    katana::render::Rgba axisColorX = katana::render::rgba(170, 75, 75);
    katana::render::Rgba axisColorY = katana::render::rgba(75, 150, 75);
    // Linetypes. Off draws every line solid, which is a fast preview and a
    // way to prove a pattern is not hiding something.
    bool drawLinetypes = true;
    double linetypeScale = 1.0; // DXF $LTSCALE's eventual home
    // A dashed entity becomes one DrawLine per dash. Past this many spans the
    // whole path is drawn solid instead - all or nothing, because a truncated
    // dashed line is a shorter line with nothing to say so.
    std::size_t maximumDashSpans = 20000;
    // In LOGICAL pixels; pixelScale turns them into the framebuffer's.
    float entityLineWidth = 1.0f;
    float selectedLineWidth = 2.0f;
    float pointSize = 5.0f;
    // Device pixels per logical pixel (QWidget::devicePixelRatioF). A 3D view
    // on a 125% display renders at its physical resolution, so a 1 px line
    // must be 1.25 px there or it thins as the display gets sharper.
    float pixelScale = 1.0f;

    // HILLSHADE, baked per vertex on a surface and per face on a mesh. The
    // direction TOWARDS the sun: from the north-west, 45 degrees up, the
    // cartographic convention every hillshade uses, so relief reads as
    // raised rather than sunken. Normalised internally; a zero vector turns
    // the lighting off.
    katana::render::Vec3 lightDirection{-0.5, 0.5, 0.70710678118654752};
    // Hemispheric ambient: a face pointing straight up gets skyAmbient, one
    // pointing straight down groundAmbient, blended by the normal's z. The
    // sun adds sunStrength * max(0, n . light) - never the absolute value,
    // which lit faces turned away from the sun as brightly as those facing it.
    double skyAmbient = 0.30;
    double groundAmbient = 0.10;
    double sunStrength = 0.80;

    // Grid spacing in model units; 0 (the default) chooses a 1-2-5 spacing
    // from the scene's size, about gridCellsAcross cells over its larger
    // side. The grid is a navigation aid, not data, so its cell count is
    // capped however large the scene.
    double gridSpacing = 0.0;
    int gridCellsAcross = 16;

    // Layers hidden in the view being built for; null for the document rule
    // alone. A POINTER into that view's state, which outlives the build, so a
    // copy of the options made for one view never carries another's hides.
    const LayerOverrides* layers = nullptr;
};

// The scene split by what it depends on (header comment). Each list is drawn
// in the order declared, into one depth buffer.
struct SceneLayers {
    katana::render::DrawList grid;      // the datum and the bounds
    katana::render::DrawList terrain;   // surfaces and meshes
    katana::render::DrawList edges;     // surface edges, faded per frame (fadeEdges)
    katana::render::DrawList entities;  // the drawing
    // The selection's dark casing (selection_style.hpp), a list of its own
    // drawn before `selection` and writing no depth, so the core drawn after
    // it always covers it. In the one list, the GPU - which pulls a mark
    // nearer by its width and reads no draw-list bias - drew the wider casing
    // over the core, and a selection was a dark line
    // (GpuLayers.ASelectionCoreDrawnAfterACasingThatWritesNoDepthKeepsItsColour).
    katana::render::DrawList selectionCasing;
    katana::render::DrawList selection; // the selected entities, over the drawing

    // Per surface whose edges fade: its vertices in `edges`, and the typical
    // length of its triangles' edges in model units.
    struct EdgeRun {
        katana::render::VertexIndex first = 0;
        katana::render::VertexIndex count = 0;
        double typicalEdge = 0.0;
        float applied = -1.0f; // the strength edges.colors now show
    };
    std::vector<EdgeRun> edgeRuns;
    // Parallel to edges.colors: the lit surface colour an edge fades into and
    // the colour it has at full strength.
    std::vector<katana::render::Rgba> edgeBase;
    std::vector<katana::render::Rgba> edgeInk;

    // The one elevation ramp (true, unexaggerated elevations); low > high
    // when no visible surface is coloured by elevation.
    double rampLow = 0.0;
    double rampHigh = -1.0;
    // Where linework with no height and nothing under it goes, exaggerated;
    // terrainDatum when a surface or mesh decided it (buildTerrain).
    double datum = 0.0;
    bool terrainDatum = false;
    // Every vertex of terrain, edges and entities (and so of the selection's
    // core, which draws entities again): what a view frames. Its depth range
    // is fitted to this, the grid and the selection, whose ghosts lie outside
    // the drawn entities. The grid is left out on purpose - it is sized from
    // this.
    katana::math::AABB bounds;
    // The part of `bounds` buildTerrain found (terrain and edges), kept so a
    // rebuild of the drawing alone need not walk the terrain again: on a
    // 229k-triangle TIN that walk was an eighth of the build.
    katana::math::AABB terrainBounds;

    [[nodiscard]] bool hasRamp() const { return rampLow <= rampHigh; }
};

// Builds draw lists. Held across frames so its buffers are reused rather than
// reallocated on every redraw.
class SceneBuilder {
  public:
    // Appends the whole scene to `out`, which is cleared first: grid, terrain,
    // edges at full strength, and the drawing with the selection in the
    // selection style. One list cannot carry the depth rules renderLayers
    // draws the grid and the edges with, so a surface flat at the datum shows
    // the grid through it here; a view draws SceneLayers.
    void build(const Document& document, const std::vector<SceneSurface>& surfaces,
               const SceneOptions& options, katana::render::DrawList& out,
               const std::vector<SceneMesh>& meshes = {});

    // ---- by layer --------------------------------------------------------------
    // Terrain and edges, the ramp and the part of the datum surfaces and meshes
    // decide. Everything else depends on it, so it comes first.
    void buildTerrain(const std::vector<SceneSurface>& surfaces,
                      const std::vector<SceneMesh>& meshes, const SceneOptions& options,
                      SceneLayers& layers);
    // The drawing, draped on the visible surfaces. May lower the datum (to the
    // lowest entity height) when there is no terrain.
    void buildEntities(const Document& document, const std::vector<SceneSurface>& surfaces,
                       const SceneOptions& options, SceneLayers& layers);
    void buildSelection(const Document& document, const std::vector<SceneSurface>& surfaces,
                        const SceneOptions& options, SceneLayers& layers);
    // From layers.bounds and layers.datum, so after the others.
    void buildGrid(const SceneOptions& options, SceneLayers& layers);

    // Recolours the fading edges for how big the triangles now are on screen
    // through `camera`: gone below about 4 px, full above about 12. Touches
    // edges.colors only when a surface's strength changes by a step. False
    // when no edge has any strength, and the edges list need not be drawn.
    static bool fadeEdges(SceneLayers& layers, const katana::render::Camera& camera);

    // ---- single pieces, for a view that shows one thing ----------------------------
    void appendSurfaces(const std::vector<SceneSurface>& surfaces, const SceneOptions& options,
                        katana::render::DrawList& out);
    void appendMeshes(const std::vector<SceneMesh>& meshes, const SceneOptions& options,
                      katana::render::DrawList& out);
    // Flat on the datum rule without surfaces to drape on.
    void appendEntities(const Document& document, const SceneOptions& options,
                        katana::render::DrawList& out);
    // A grid round `around` at options.entityElevation.
    void appendGrid(const SceneOptions& options, const katana::math::AABB& around,
                    katana::render::DrawList& out);

  private:
    // Which entities an emit draws, and how: every one as drawn; the selected
    // ones drawn in the view, as the overlay's core (selection_style.hpp);
    // every one, the selected in the selection colour (the one-list build);
    // the selected ones the view alone hides, as ghosts. Only and Ghosts walk
    // the selection's ids rather than the drawing.
    enum class Selected { AsDrawn, Only, Styled, Ghosts };
    void emitEntities(const Document& document, const std::vector<SceneSurface>& surfaces,
                      const SceneOptions& options, Selected which, double datumHint,
                      bool datumKnown, katana::render::DrawList& out, double* lowestHeight);
    void emitGrid(const SceneOptions& options, const katana::math::AABB& around, double z,
                  katana::render::DrawList& out);

    std::vector<Point2> chord_; // tessellation scratch, reused
    std::vector<katana::render::Vec3> normals_; // per-vertex normal scratch, reused
    // The AVX2 kernels' scratch (src/katana_cad/simd/scene_kernels.hpp): four
    // doubles a vertex of lifted positions and of summed normals.
    std::vector<double> lifted_;
    std::vector<double> summed_;
    std::vector<std::uint8_t> used_; // bounds scratch: which vertices a primitive uses
};

// One frame of `layers` as the 3D view draws it: the depth range fitted to
// the layers, the grid and the selection (whose ghosts may lie outside the
// rest), the edges faded for `camera` (fadeEdges), then one pass per layer in
// declaration order into one depth buffer, each with its own depth rule:
//
//   grid       a backdrop, writing no depth, so the model always covers it.
//              It stands on the datum, exactly in the plane of a surface
//              that is flat at its lowest (a pad, a pond floor), where the
//              surface's slope push lost to it: the whole grid showed
//              through a flat pad.
//   terrain    written.
//   edges      tested against the terrain but not written. An edge and a
//              line draped across it both lie in the surface, each at one
//              depth across its width; off their centres those differ by up
//              to a pixel of the surface's slope, more than the half a
//              footprint between their biases at a low angle, so the edge
//              drawn first broke every draped line it crossed.
//   entities   written.
//   selectionCasing
//              tested but not written, so the core drawn next covers it
//              wherever the two overlap, whatever either's pull.
//   selection  written.
//
// `options.clear` is honoured by the first pass only. Stops at, and returns,
// the first failure.
[[nodiscard]] katana::core::Result<katana::render::RenderStats>
renderLayers(SceneLayers& layers, katana::render::Camera& camera,
             katana::render::Rasterizer& rasterizer, katana::render::Framebuffer& target,
             katana::render::RenderOptions options = {});

// The elevation ramp's colour at t in [0, 1] (clamped): what the legend of a
// view draws, so it cannot disagree with the surfaces.
[[nodiscard]] katana::render::Rgba elevationRampColor(double t);

// The world-space box of everything the scene would draw, with any vertical
// exaggeration applied: linework spans from the datum to the highest height
// or surface it can be put at. What a viewport frames when asked to zoom to
// extents.
[[nodiscard]] katana::math::AABB sceneBounds(const Document& document,
                                             const std::vector<SceneSurface>& surfaces,
                                             const SceneOptions& options,
                                             const std::vector<SceneMesh>& meshes = {});

} // namespace katana::cad
