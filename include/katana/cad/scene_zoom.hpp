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
// So a notch now finds what the cursor points at (zoomAnchor: what is drawn
// there, else the datum's plane or the depth the scene reaches) and moves
// the pivot to its depth first (Camera::setPivotDepth), which moves nothing
// on screen. The dolly then magnifies exactly that point by the notch's
// factor, the eye comes the same fraction nearer to it every notch and never
// reaches it, and the pan, the orbit and the edges' fade, which all go by the
// pivot's distance, go by that point's.
//
// Headless and free of Qt: RenderViewWidget calls zoomAtPixel for the wheel
// over either renderer, and so can anything else that zooms a 3D view.

#include <optional>

#include "katana/cad/scene.hpp"
#include "katana/math/primitives.hpp"
#include "katana/render/camera.hpp"

namespace katana::cad {

// What is under a pixel.
struct ScenePick {
    enum class Source {
        Terrain, // a triangle, line or point of a surface or a mesh
        Drawing, // a triangle, line or point of the drawing
        Datum,   // nothing drawn: the datum's plane, where the grid stands
        // Nothing drawn, and the datum's plane (if the ray meets it at all)
        // further than the scene reaches: the ray's point as deep as the
        // furthest corner of sceneDepthBox (zoomAnchor only).
        Reach,
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

// What a zoom about pixel (x, y) goes towards: what is drawn there
// (pickDrawnPoint); with nothing drawn there - two thirds of a docked view
// as it opens, the ground beyond the grid and the background round the
// model - where the ray meets the datum's plane, however far out, or the
// ray's point as deep as the scene reaches (the furthest corner of
// sceneDepthBox along the view direction), whichever is nearer. The datum
// keeps the eye above the ground a zoom goes towards, where the target's
// plane put the anchor under it and the eye dived; the reach keeps a pixel
// near the horizon from anchoring kilometres off in one notch, and leaves
// nothing drawn beyond the anchor, so nothing drawn stalls: it all grows
// until it leaves the view. A level or rising ray takes the reach. Nothing
// when the whole scene is behind the eye, or for what pickDrawnPoint
// refuses.
[[nodiscard]] std::optional<ScenePick> zoomAnchor(const SceneLayers& layers,
                                                  const katana::render::Camera& camera, double x,
                                                  double y);

// How near a zoom may bring the eye to `point` (a pivot's distance): where
// what draws the view stops drawing it right, and no nearer than that
// whatever the scene around it - a limit tied to the scene's box stopped the
// zoom 930 m from a site at MGA coordinates whose drawing held one stray
// entity at the origin. The larger of:
//
//   * tol::kGeometric / Camera::kNearPivotFloor, 0.1 mm: below it the near
//     plane, which fitDepthRange keeps a thousandth of the pivot's distance
//     in front of the eye, would reach its own floor of tol::kGeometric and
//     stop keeping clear of what the zoom went towards;
//   * where the double arithmetic that projects a point - the view's
//     translation, as far from the world's origin as the point, taken from
//     it - is out by kZoomPrecisionPixels of a pixel. Each rounding is at
//     most u = 2^-53 of what it holds, and a dot product of n terms is out by
//     at most n u of the sum of their sizes (Higham, Accuracy and Stability
//     of Numerical Algorithms, 2nd ed., section 3.1); the view's translation
//     (3 terms), the projection times the view (4) and the matrix times the
//     point (4, of the point and the translation, each |point| in size) add
//     to at most 16 u |point|: 0.11 mm at an MGA northing of 6.2e6 m on an
//     800 px tall view, and the same length whatever the drawing's unit,
//     which is what binds a survey drawn in millimetres.
//
// The GPU draws in float relative to a scene origin, whose error closes in
// far sooner; the GPU view keeps that origin next to the pivot instead
// (gpu::originErrorPixels, docs/gpu.md), so its limit is this one.
inline constexpr double kZoomPrecisionPixels = 0.1;
[[nodiscard]] double minimumApproach(const katana::render::Camera& camera,
                                     const katana::math::Vec3& point);

struct ZoomResult {
    // What the zoom went towards; nothing when it went about the pivot's
    // plane, as it always had: under an orthographic projection, where every
    // point of a pixel's ray stays under the pixel, or with the whole scene
    // behind the eye.
    std::optional<ScenePick> anchor;
    // What the distance and the orthographic height were multiplied by:
    // below 1 is nearer, 1 is no move.
    double factor = 1.0;
    // The minimum approach held the zoom short of the factor asked for.
    bool limited = false;
};

// Zooms `camera` by `notches` wheel notches, positive in, each
// Camera::kZoomPerNotch, about pixel (x, y). Under a perspective projection
// the pivot first moves to the depth of what the pixel points at
// (zoomAnchor, Camera::setPivotDepth), so that point stays under the pixel
// and its footprint shrinks by exactly the factor. Zooming in stops at
// minimumApproach of that point - of the target without one - and under an
// orthographic projection at the view a perspective view shows there, 2 d
// tan(fov / 2) tall, so P shows the same limit. Zooming out is not limited.
// Zero, non-finite notches, a pixel that is not finite or a camera with no
// viewport change nothing.
ZoomResult zoomAtPixel(const SceneLayers& layers, katana::render::Camera& camera, double notches,
                       double x, double y);

} // namespace katana::cad
