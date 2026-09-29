#pragma once

// Zooming a 3D view towards what it draws (docs/render.md, "Zooming towards
// the cursor").
//
// A wheel notch used to dolly towards the point under the cursor of the plane
// through the orbit target, facing the eye - a point in the air in front of
// the ground, or under it, not the ground itself. The eye closed in on that
// point 13% of the remaining way each notch, so over ground beyond the plane
// the magnification of the ground tended to depth / (depth - distance) and
// stopped: after about 30 notches nothing on screen moved, and the pan and
// the orbit, which go by the same collapsed distance, stopped with it. Over
// ground nearer than the plane the eye went through the ground and the view
// went blank.
//
// So a notch now finds what IS drawn under the cursor (pickDrawnPoint) and
// moves the pivot to its depth first (Camera::setPivotDepth), which moves
// nothing on screen. The dolly then magnifies exactly that point by the
// notch's factor, the eye comes the same fraction nearer to it every notch
// and never reaches it, and the pan, the orbit and the edges' fade, which all
// go by the pivot's distance, go by that point's.
//
// Headless and free of Qt: RenderViewWidget calls zoomAtPixel for the wheel
// over either renderer, and so can anything else that zooms a 3D view.

#include <optional>

#include "katana/cad/scene.hpp"
#include "katana/math/primitives.hpp"
#include "katana/render/camera.hpp"

namespace katana::cad {

// What is drawn under a pixel.
struct ScenePick {
    enum class Source {
        Terrain, // a triangle, line or point of a surface or a mesh
        Drawing, // a triangle, line or point of the drawing
        Datum,   // nothing drawn: the datum's plane, where the grid stands
    };
    // Where the ray through the pixel meets a triangle or the datum's plane;
    // for a line, its point nearest the ray; for a point, the point.
    katana::math::Vec3 point;
    // In front of the eye along the view direction: always positive.
    double depth = 0.0;
    Source source = Source::Datum;
};

struct PickOptions {
    // How far from the pixel's centre, in pixels, the centre line of a line
    // or the centre of a point may lie and still count as under the pixel:
    // exactly on a 1 px line is hard to be. It is an angle about the ray -
    // that many pixels in the middle of a perspective view, and more towards
    // its edges, where a pixel spans less of the field of view: 1 / cos^2 of
    // the angle off the axis, 1.5 times in the corner of a 4:3 view at 45
    // degrees, 1.7 in a 16:9 one.
    double aperture = 3.0;
    // Tests only the primitives that can be under the pixel: a pass over the
    // vertices finds which side of the ray and of the aperture each lies, and
    // a primitive whose vertices all lie beyond one side is skipped. Off,
    // every primitive is tested - what the tests compare the culled pick with
    // (docs/render.md measures both).
    bool cull = true;
};

// The nearest thing the terrain and drawing layers draw under the centre of
// pixel (x, y) of `camera` (in its viewport's pixels, as rayThroughPixel):
// a triangle the pixel's ray passes through, or a line or a point within
// `options.aperture` pixels of it, whichever is nearest the eye. With none,
// where the ray meets the datum's plane (SceneLayers::datum), when that is in
// front of the eye and inside sceneDepthBox in plan: the grid stands there,
// and linework with no height lies there. Else nothing - a pixel of sky, or
// of ground beyond anything drawn. The grid, the faded edges and the
// selection overlay are not looked at: the grid is a backdrop, and the other
// two draw again what the terrain and the drawing already hold.
//
// Also nothing for a camera with no viewport or a pixel that is not finite.
[[nodiscard]] std::optional<ScenePick> pickDrawnPoint(const SceneLayers& layers,
                                                      const katana::render::Camera& camera,
                                                      double x, double y,
                                                      const PickOptions& options = {});

// How near a zoom may bring the eye to what it zooms towards: this fraction
// of the diagonal of sceneDepthBox, and of 2 m at least (a box that small is
// one point, which Camera::frame shows as a 2 m patch): 3.3 cm on the sample
// terrain, 20 cm on a 1.2 x 0.8 km site. What sets it is the GPU, which
// draws in float relative to its scene origin, so its error grows as the
// view closes on a point away from that origin. Measured with the GPU's own
// matrix code on an 800 px tall view (docs/render.md, "Zooming towards the
// cursor"), at this distance it is 0.14-0.16 px, on the sample 80 m from the
// origin and on the site 460 m; at half of it 0.3-0.4 px, at an eighth of it
// 1.1-1.7 px. The near plane, which fitDepthRange puts at a millionth of the
// box's diagonal at most once the eye is inside the box, measured 160 to 450
// times nearer than the point zoomed into there: it clips nothing of it.
inline constexpr double kMinimumApproachFraction = 1.0e-4;
[[nodiscard]] double minimumApproach(const SceneLayers& layers);

struct ZoomResult {
    // What the zoom went towards; nothing when it went about the pivot's
    // plane, as it always had: under an orthographic projection, where every
    // point of a pixel's ray stays under the pixel, or with nothing under the
    // cursor.
    std::optional<ScenePick> anchor;
    // What the distance and the orthographic height were multiplied by:
    // below 1 is nearer, 1 is no move.
    double factor = 1.0;
    // The minimum approach held the zoom short of the factor asked for.
    bool limited = false;
};

// Zooms `camera` by `notches` wheel notches, positive in, each
// Camera::kZoomPerNotch, about pixel (x, y). Under a perspective projection
// the pivot first moves to the depth of what is drawn under the pixel
// (pickDrawnPoint, Camera::setPivotDepth), so that point stays under the
// pixel and its footprint shrinks by exactly the factor. Zooming in stops at
// minimumApproach: the pivot no nearer than that under a perspective
// projection, and under an orthographic one no smaller a view than a
// perspective view shows at that distance, 2 d tan(fov / 2), so P shows the
// same limit. Zooming out is not limited. Zero, non-finite notches, a pixel
// that is not finite or a camera with no viewport change nothing.
ZoomResult zoomAtPixel(const SceneLayers& layers, katana::render::Camera& camera, double notches,
                       double x, double y);

} // namespace katana::cad
