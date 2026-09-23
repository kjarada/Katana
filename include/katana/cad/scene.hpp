#pragma once

// Turns the document into a DrawList (PLAN.MD Phases 15 and 16, Rule 3).
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

#include <string>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/layer_overrides.hpp"
#include "katana/cad/selection.hpp"
#include "katana/geometry/mesh.hpp"
#include "katana/render/camera.hpp"
#include "katana/render/draw_list.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::cad {

using katana::geometry::Point2;

// How a surface is drawn.
enum class SurfaceStyle {
    Hidden,
    Wireframe,      // triangle edges only
    Shaded,         // filled triangles, flat lit
    ShadedWithEdges // both, with the edges biased forward so they do not z-fight
};

// What decides a surface triangle's colour.
enum class SurfaceColoring {
    Flat,      // one colour for the whole surface
    Elevation, // a ramp between the surface's own min and max
    Slope      // green to red by steepness
};

struct SceneSurface {
    std::string name;
    const katana::terrain::TinSurface* surface = nullptr;
    SurfaceStyle style = SurfaceStyle::ShadedWithEdges;
    SurfaceColoring coloring = SurfaceColoring::Elevation;
    katana::render::Rgba flatColor = katana::render::rgba(170, 170, 160);
    bool visible = true;
};

// A mesh in the session (PLAN.MD 20.2, slice 4): a 12d trimesh, drawn like
// a surface but NOT one - it may be closed or overhang, so it is never
// sampled for a height and carries no elevation ramp. Colouring is flat or
// per face, because that is what the file gives.
struct SceneMesh {
    std::string name;
    const katana::geometry::TriangleMesh* mesh = nullptr;
    // Shaded, NOT ShadedWithEdges as a surface is. A mesh has no neighbour
    // table, so every edge of every face is drawn and each line costs two
    // triangles. Measured on `test trimishes complex.12da` (266 meshes,
    // 126 536 faces) in Release on this machine, at the framing the import
    // leaves: with edges 886 504 triangles rasterised in 24.9 ms, without
    // them 127 288 in 18.0 ms. On a mesh of this density the edges also read
    // as noise. The style is per mesh, so a user who wants to see the
    // triangulation of one asks for it.
    SurfaceStyle style = SurfaceStyle::Shaded;
    katana::render::Rgba flatColor = katana::render::rgba(190, 170, 140);
    // Empty, or one per face. A face whose colour the file did not give
    // takes flatColor, so a partly coloured mesh is still wholly drawn.
    std::vector<katana::render::Rgba> faceColors;
    bool visible = true;
};

struct SceneOptions {
    bool drawEntities = true;
    bool drawGrid = true;
    // Z given to 2D drawing geometry, which carries none of its own. Drawn just
    // above the ground plane so a plan drawing is not buried inside a surface
    // it is drawn over.
    double entityElevation = 0.0;
    double entityDepthBias = 2.0e-4;

    // Vertical exaggeration. Applied to Z about `exaggerationDatum` as the
    // scene is built, so the camera, the picking and the framing all agree
    // about where things are. 1 is no exaggeration.
    double verticalExaggeration = 1.0;
    double exaggerationDatum = 0.0;

    // Sagitta tolerance for chording arcs and circles, in model units. The
    // same rule the 2D viewport uses, so the two views agree.
    double chordTolerance = 0.01;

    katana::render::Rgba defaultColor = katana::render::rgba(220, 220, 220);
    katana::render::Rgba selectionColor = katana::render::rgba(255, 190, 60);
    katana::render::Rgba gridColor = katana::render::rgba(60, 60, 68);
    katana::render::Rgba axisColorX = katana::render::rgba(150, 70, 70);
    katana::render::Rgba axisColorY = katana::render::rgba(70, 130, 70);
    // Linetypes. Off draws every line solid, which is a fast preview and a
    // way to prove a pattern is not hiding something.
    bool drawLinetypes = true;
    double linetypeScale = 1.0; // DXF $LTSCALE's eventual home
    // A dashed entity becomes one DrawLine per dash. Past this many spans the
    // whole path is drawn solid instead - all or nothing, because a truncated
    // dashed line is a shorter line with nothing to say so.
    std::size_t maximumDashSpans = 20000;
    float entityLineWidth = 1.0f;
    float selectedLineWidth = 2.0f;
    float pointSize = 5.0f;

    // Direction the flat lighting comes from, in world space. Normalised
    // internally; a zero vector disables shading.
    katana::render::Vec3 lightDirection{0.35, -0.5, 0.79};
    double ambient = 0.35;

    // Grid spacing in model units, and how many lines to draw each way. The
    // grid is a navigation aid, not data: it is clamped so that zooming out
    // cannot ask for a million lines.
    double gridSpacing = 10.0;
    int gridLines = 40;

    // Layers hidden in the view being built for; null for the document rule
    // alone. A POINTER into that view's state, which outlives the build, so a
    // copy of the options made for one view never carries another's hides.
    const LayerOverrides* layers = nullptr;
};

// Builds draw lists. Held across frames so its buffers are reused rather than
// reallocated on every redraw (PLAN.MD section 33).
class SceneBuilder {
  public:
    // Appends the whole scene to `out`, which is cleared first.
    void build(const Document& document, const std::vector<SceneSurface>& surfaces,
               const SceneOptions& options, katana::render::DrawList& out,
               const std::vector<SceneMesh>& meshes = {});

    // Just the surfaces, for a view that shows terrain alone.
    void appendSurfaces(const std::vector<SceneSurface>& surfaces, const SceneOptions& options,
                        katana::render::DrawList& out);
    void appendMeshes(const std::vector<SceneMesh>& meshes, const SceneOptions& options,
                      katana::render::DrawList& out);
    void appendEntities(const Document& document, const SceneOptions& options,
                        katana::render::DrawList& out);
    void appendGrid(const SceneOptions& options, const katana::math::AABB& around,
                    katana::render::DrawList& out);

  private:
    std::vector<Point2> chord_; // tessellation scratch, reused
};

// The world-space box of everything the scene would draw, with any vertical
// exaggeration applied. What a viewport frames when asked to zoom to extents.
[[nodiscard]] katana::math::AABB sceneBounds(const Document& document,
                                             const std::vector<SceneSurface>& surfaces,
                                             const SceneOptions& options,
                                             const std::vector<SceneMesh>& meshes = {});

} // namespace katana::cad
