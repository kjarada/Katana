// Depth: what the 3D view got wrong before the depth-range fit (docs/render.md,
// "Depth"). Each test is one of the defects measured on real survey data,
// rebuilt as the smallest scene that shows it:
//
//   z-fighting    near = far * 1e-5 over standard [0, 1] depth gave a framed
//                 scene a few hundred distinct depths; a surface 5 cm above
//                 another 1 km across lost three quarters of its pixels.
//   x-ray         a constant NDC bias bigger than the whole scene's depth
//                 span showed linework through a building.
//   zoom-out      the depth range was fixed at frame(), so eight wheel
//                 notches out pushed the model past the far plane.
//   ortho zoom-in an orthographic eye a zoom had moved into the model cut
//                 away everything in front of it.
//
// The expected values are properties, not captured numbers: a hidden line has
// no pixels, a surface above another shows every pixel it shows alone.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <initializer_list>
#include <string>
#include <vector>

#include "katana/core/task_pool.hpp"
#include "katana/render/camera.hpp"
#include "katana/render/rasterizer.hpp"

using katana::core::TaskPool;
using katana::math::Vec3;
using katana::render::Camera;
using katana::render::DrawList;
using katana::render::Framebuffer;
using katana::render::Projection;
using katana::render::Rasterizer;
using katana::render::Rgba;
using katana::render::rgba;
using katana::render::RenderOptions;
using katana::render::StandardView;

namespace {

constexpr Rgba kBackground = rgba(0, 0, 0);
constexpr Rgba kGround = rgba(200, 40, 40);
constexpr Rgba kDesign = rgba(40, 200, 40);
constexpr Rgba kBuilding = rgba(40, 40, 200);
constexpr Rgba kLine = rgba(250, 250, 250);

// The scene's own linework bias (SceneOptions::entityDepthBias): one and a
// half pixel footprints towards the eye.
constexpr float kEntityBias = 1.5f;

std::size_t countPixels(const Framebuffer& fb, Rgba color)
{
    return static_cast<std::size_t>(std::count(fb.color().begin(), fb.color().end(), color));
}

// A rectangle cut into n x n cells, as a surface is: one big triangle across
// a kilometre would test the barycentric rounding of a shape no TIN has. At
// height z + slopeX * x + slopeY * y: flat unless a slope is given.
void addPlane(DrawList& list, double x0, double y0, double x1, double y1, double z, int n,
              Rgba color, double slopeX = 0.0, double slopeY = 0.0)
{
    const auto base = static_cast<katana::render::VertexIndex>(list.positions.size());
    for (int j = 0; j <= n; ++j) {
        for (int i = 0; i <= n; ++i) {
            const double x = x0 + (x1 - x0) * i / n;
            const double y = y0 + (y1 - y0) * j / n;
            list.addVertex(Vec3(x, y, z + slopeX * x + slopeY * y), color);
        }
    }
    const auto at = [base, n](int i, int j) {
        return base + static_cast<katana::render::VertexIndex>(j * (n + 1) + i);
    };
    for (int j = 0; j < n; ++j) {
        for (int i = 0; i < n; ++i) {
            list.addTriangle(at(i, j), at(i + 1, j), at(i + 1, j + 1));
            list.addTriangle(at(i, j), at(i + 1, j + 1), at(i, j + 1));
        }
    }
}

// A closed box, twelve triangles.
void addBox(DrawList& list, const Vec3& lo, const Vec3& hi, Rgba color)
{
    const auto v = [&](int corner) {
        return list.addVertex(Vec3((corner & 1) ? hi.x : lo.x, (corner & 2) ? hi.y : lo.y,
                                   (corner & 4) ? hi.z : lo.z),
                              color);
    };
    katana::render::VertexIndex c[8];
    for (int k = 0; k < 8; ++k) {
        c[k] = v(k);
    }
    const int faces[6][4] = {{0, 1, 3, 2}, {4, 6, 7, 5}, {0, 4, 5, 1},
                             {2, 3, 7, 6}, {0, 2, 6, 4}, {1, 5, 7, 3}};
    for (const auto& f : faces) {
        list.addTriangle(c[f[0]], c[f[1]], c[f[2]]);
        list.addTriangle(c[f[0]], c[f[2]], c[f[3]]);
    }
}

void append(DrawList& to, const DrawList& from)
{
    const auto base = static_cast<katana::render::VertexIndex>(to.positions.size());
    to.positions.insert(to.positions.end(), from.positions.begin(), from.positions.end());
    to.colors.insert(to.colors.end(), from.colors.begin(), from.colors.end());
    for (const auto& t : from.triangles) {
        to.addTriangle(base + t.a, base + t.b, base + t.c);
    }
    for (const auto& l : from.lines) {
        to.addLine(base + l.a, base + l.b, l.width, l.depthBias);
    }
    for (const auto& p : from.points) {
        to.addPoint(base + p.a, p.size, p.depthBias);
    }
}

Framebuffer render(const DrawList& list, const Camera& camera)
{
    auto target = Framebuffer::create(camera.viewportWidth(), camera.viewportHeight());
    EXPECT_TRUE(target.ok());
    TaskPool pool(0);
    RenderOptions options;
    options.background = kBackground;
    options.pool = &pool;
    Rasterizer rasterizer;
    const auto stats = rasterizer.render(list, camera, *target, options);
    EXPECT_TRUE(stats.ok());
    return std::move(*target);
}

// The measured case: a 1 km ground and a 400 m design surface `gap` above
// its middle, framed from the south-west as the 3D view opens.
struct Overlap {
    DrawList ground;
    DrawList design;
    katana::math::AABB bounds;

    explicit Overlap(double gap)
    {
        addPlane(ground, -500.0, -500.0, 500.0, 500.0, 0.0, 20, kGround);
        addPlane(design, -200.0, -200.0, 200.0, 200.0, gap, 8, kDesign);
        bounds = ground.bounds();
        bounds.expand(design.bounds());
    }
};

Camera framedCamera(const katana::math::AABB& bounds, Projection projection)
{
    Camera camera;
    camera.setViewportSize(1200, 800);
    camera.setProjection(projection);
    camera.setStandardView(StandardView::IsoSouthWest);
    EXPECT_TRUE(camera.frame(bounds));
    return camera;
}

// What the 3D view does to a camera before every frame.
constexpr double kZoomPerNotch = 1.15;

} // namespace

TEST(RenderDepth, ASurfaceFiveCentimetresAboveAnotherAKilometreAcrossShowsEveryPixel)
{
    // near = far * 1e-5 on standard depth lost 74% of these pixels to the
    // ground. Fitted to the scene, the two planes 5 cm apart at 1-2 km are
    // hundreds of float steps apart in depth: d = n (f - z) / ((f - n) z)
    // moves by n f / ((f - n) z^2) * 0.05 = about 4e-5 for n = 1200, f =
    // 2700 and z = 1800, against a float step near 0.5 of 6e-8.
    for (Projection projection : {Projection::Perspective, Projection::Orthographic}) {
        SCOPED_TRACE(static_cast<int>(projection));
        const Overlap scene(0.05);
        const Camera camera = framedCamera(scene.bounds, projection);

        const std::size_t alone = countPixels(render(scene.design, camera), kDesign);
        ASSERT_GT(alone, 10000u) << "the design surface is not in view, so this proves nothing";

        // Either order: the ground drawn first or last must lose everywhere
        // the design covers it.
        DrawList groundFirst = scene.ground;
        append(groundFirst, scene.design);
        EXPECT_EQ(countPixels(render(groundFirst, camera), kDesign), alone);
        DrawList designFirst = scene.design;
        append(designFirst, scene.ground);
        EXPECT_EQ(countPixels(render(designFirst, camera), kDesign), alone);
    }
}

TEST(RenderDepth, ALineOnTheGroundBehindABuildingIsHidden)
{
    // A 40 x 40 x 25 m building at the origin and a line on the ground 30 m
    // behind it, seen from the south 200 m away and 0.25 rad above level.
    // The eye is at about (0, -194, 59); the ray to the line's middle
    // (0, 30, 0) meets the building's south face (y = -20) at
    // z = 59 * (1 - 174 / 224) = 13, inside its 0..25, and the line's ends
    // (x = +-10) cross that face at x = +-7.8, inside its +-20: the whole
    // line is behind the building. The line carries the scene's bias; the
    // constant NDC bias it replaced was larger than the building's whole
    // depth span and drew the line through it.
    DrawList building;
    addBox(building, Vec3(-20.0, -20.0, 0.0), Vec3(20.0, 20.0, 25.0), kBuilding);
    DrawList line;
    line.addSegment(Vec3(-10.0, 30.0, 0.0), Vec3(10.0, 30.0, 0.0), kLine, 2.0f, kEntityBias);

    for (Projection projection : {Projection::Perspective, Projection::Orthographic}) {
        SCOPED_TRACE(static_cast<int>(projection));
        Camera camera;
        camera.setViewportSize(640, 480);
        camera.setProjection(projection);
        camera.setStandardView(StandardView::Front); // looking north
        camera.setOrientation(camera.azimuth(), 0.25);
        camera.setTarget(Vec3(0.0, 0.0, 10.0));
        camera.setDistance(200.0);
        camera.setOrthographicHeight(120.0);
        katana::math::AABB bounds = building.bounds();
        bounds.expand(line.bounds());
        ASSERT_TRUE(camera.fitDepthRange(bounds));

        ASSERT_GT(countPixels(render(line, camera), kLine), 20u)
            << "the line is not in view, so its being hidden proves nothing";
        DrawList buildingFirst = building;
        append(buildingFirst, line);
        const Framebuffer seen = render(buildingFirst, camera);
        EXPECT_EQ(countPixels(seen, kLine), 0u);
        EXPECT_GT(countPixels(seen, kBuilding), 1000u);
        DrawList lineFirst = line;
        append(lineFirst, building);
        EXPECT_EQ(countPixels(render(lineFirst, camera), kLine), 0u);
    }
}

TEST(RenderDepth, ALineLyingOnTheGroundIsDrawnWholeOverIt)
{
    // The bias exists for this: linework draped on a surface, exactly in its
    // plane, must win against it everywhere it would draw on its own - even
    // seen at a slant, where the ground a pixel beside the line's centre is
    // nearer than the line by a pixel of its depth slope.
    DrawList ground;
    addPlane(ground, -500.0, -500.0, 500.0, 500.0, 0.0, 20, kGround);
    DrawList line;
    line.addSegment(Vec3(-300.0, -100.0, 0.0), Vec3(300.0, 150.0, 0.0), kLine, 2.0f, kEntityBias);
    line.addSegment(Vec3(-100.0, -300.0, 0.0), Vec3(50.0, 300.0, 0.0), kLine, 1.0f, kEntityBias);

    for (Projection projection : {Projection::Perspective, Projection::Orthographic}) {
        SCOPED_TRACE(static_cast<int>(projection));
        const Camera camera = framedCamera(ground.bounds(), projection);
        const std::size_t alone = countPixels(render(line, camera), kLine);
        ASSERT_GT(alone, 500u);
        DrawList both = ground;
        append(both, line);
        EXPECT_EQ(countPixels(render(both, camera), kLine), alone);
    }
}

TEST(RenderDepth, ZoomingOutEightNotchesNeverBlanksTheView)
{
    // Eight wheel notches out is the camera 1.15^8 = 3.06 times further
    // away. The planes frame() chose put the far one about 3.2 radii past
    // the target, so the whole model went past it and the view drew three
    // fragments. The view fits the depth range every frame.
    const Overlap scene(0.05);
    DrawList both = scene.ground;
    append(both, scene.design);
    for (Projection projection : {Projection::Perspective, Projection::Orthographic}) {
        SCOPED_TRACE(static_cast<int>(projection));
        Camera camera = framedCamera(scene.bounds, projection);
        for (int notch = 0; notch < 8; ++notch) {
            camera.dollyAtPixel(kZoomPerNotch, 600.0, 400.0);
        }
        ASSERT_TRUE(camera.fitDepthRange(scene.bounds));
        const Framebuffer seen = render(both, camera);
        EXPECT_GT(countPixels(seen, kGround), 1000u);
        // And still without z-fighting: all of the design it shows alone.
        EXPECT_EQ(countPixels(seen, kDesign), countPixels(render(scene.design, camera), kDesign));
    }
}

TEST(RenderDepth, AnOrthographicZoomIntoTheModelDoesNotCutItsFrontAway)
{
    // Sixteen notches in at the middle of an orthographic view moves the eye
    // to 1/1.15^16 = 0.107 of the framing distance, about 160 m from the
    // target: inside the ground's reach back towards the viewer (its south-
    // west corner is 707 m * cos 35 = 577 m nearer than the target), so a
    // near plane in front of the eye cut the ground away. The view is then
    // 0.107 of the framed 1500 x 1000 m: 160 x 107 m, and the 400 m design
    // seen from 35 degrees up is a diamond 283 m and 163 m from its middle
    // to its corners: 80 / 283 + 53 / 163 = 0.61 < 1, so every pixel is the
    // design.
    const Overlap scene(0.05);
    DrawList both = scene.ground;
    append(both, scene.design);
    Camera camera = framedCamera(scene.bounds, Projection::Orthographic);
    for (int notch = 0; notch < 16; ++notch) {
        camera.dollyAtPixel(1.0 / kZoomPerNotch, 600.0, 400.0);
    }
    ASSERT_TRUE(camera.fitDepthRange(scene.bounds));
    EXPECT_GT(camera.orthographicStandoff(), 0.0) << "the eye was not inside, so this proves nothing";
    const Framebuffer seen = render(both, camera);
    EXPECT_EQ(countPixels(seen, kDesign), 1200u * 800u);
}

TEST(RenderDepth, TheOrthographicStandoffMovesOnlyTheDepthsNotThePicture)
{
    // Where along the view direction an orthographic eye sits changes no
    // ray's screen position, so moving it back to clear the scene must not
    // move a single projected point. The probes are in the target's own
    // plane, in front of the eye before the move as after it.
    Camera camera;
    camera.setViewportSize(400, 300);
    camera.setProjection(Projection::Orthographic);
    camera.setStandardView(StandardView::IsoSouthWest);
    camera.setTarget(Vec3(10.0, 20.0, 5.0));
    camera.setDistance(1.0); // inside the box below
    camera.setOrthographicHeight(60.0);
    const Vec3 probes[] = {camera.target(), camera.target() + camera.right() * 12.0,
                           camera.target() + camera.up() * -7.0 + camera.right() * -20.0};
    std::vector<Vec3> before;
    for (const Vec3& probe : probes) {
        const auto screen = camera.project(probe);
        ASSERT_TRUE(screen.has_value());
        before.push_back(*screen);
    }
    ASSERT_TRUE(camera.fitDepthRange(
        katana::math::AABB(Vec3(-100.0, -100.0, -20.0), Vec3(100.0, 100.0, 40.0))));
    ASSERT_GT(camera.orthographicStandoff(), 0.0) << "the eye was not moved, so this proves nothing";
    for (std::size_t k = 0; k < before.size(); ++k) {
        const auto after = camera.project(probes[k]);
        ASSERT_TRUE(after.has_value());
        EXPECT_NEAR(after->x, before[k].x, 1e-9);
        EXPECT_NEAR(after->y, before[k].y, 1e-9);
    }
    // The target is still the middle of the view: (200, 150).
    EXPECT_NEAR(before[0].x, 200.0, 1e-9);
    EXPECT_NEAR(before[0].y, 150.0, 1e-9);
}

TEST(RenderDepth, WithTheEyeInsideTheSceneSurfacesAKilometreAwayStillSeparate)
{
    // Walking a model: the eye 30 m above a 4 km ground, inside the scene's
    // box (a 100 m mast behind it makes the box reach above the eye), so the
    // near plane falls back to far * 1e-6: the far corner is about 2000 m
    // away, so near is about 2 mm. A strip 5 cm above the ground 700-1300 m
    // ahead, seen 0.03 rad below level, has the ground 0.05 / 0.03 = 1.7 m
    // behind it along each ray: n / z^2 * 1.7 = 2e-3 / 1e6 * 1.7 = 3e-9 of
    // depth at z = 1000, where the depth is n / z = 2e-6 and a float's step
    // is 2e-13 - thousands of steps. On standard depth the same scene sits
    // at 1 - 2e-6, where a float's step is 6e-8, and the strip and the
    // ground there are the same number - which is why the depth is reversed
    // (camera.hpp, Clip).
    DrawList ground;
    addPlane(ground, -2000.0, -2000.0, 2000.0, 2000.0, 0.0, 80, kGround);
    addBox(ground, Vec3(-5.0, -1900.0, 0.0), Vec3(5.0, -1890.0, 100.0), kBuilding);
    DrawList design;
    addPlane(design, -150.0, 700.0, 150.0, 1300.0, 0.05, 12, kDesign);

    Camera camera;
    camera.setViewportSize(800, 500);
    camera.setProjection(Projection::Perspective);
    camera.setStandardView(StandardView::Front); // looking north
    camera.setOrientation(camera.azimuth(), 0.03);
    camera.setTarget(Vec3(0.0, 1000.0, 0.0));
    camera.setDistance(1000.0); // the eye is 1000 m south and 30 m up
    katana::math::AABB bounds = ground.bounds();
    bounds.expand(design.bounds());
    ASSERT_TRUE(camera.fitDepthRange(bounds));
    ASSERT_TRUE(bounds.contains(camera.eye())) << "the eye must be inside the scene";
    ASSERT_LT(camera.nearPlane(), 0.01);

    const std::size_t alone = countPixels(render(design, camera), kDesign);
    ASSERT_GT(alone, 2000u);
    DrawList both = ground;
    append(both, design);
    EXPECT_EQ(countPixels(render(both, camera), kDesign), alone);
}

namespace {

// The scene's own point size (SceneOptions::pointSize); the bias is the
// linework's, kEntityBias.
constexpr float kPointSize = 5.0f;
constexpr Rgba kPoint = rgba(250, 250, 0);

// A 400 m site rising 5% to the east and 2% to the north, the slope the
// review measured on, and 20 x 20 survey points lying exactly on it.
struct SlopedSite {
    DrawList ground;
    DrawList points;
    katana::math::AABB bounds;

    SlopedSite()
    {
        addPlane(ground, 0.0, 0.0, 400.0, 400.0, 0.0, 40, kGround, 0.05, 0.02);
        for (int j = 0; j < 20; ++j) {
            for (int i = 0; i < 20; ++i) {
                const double x = 10.0 + 20.0 * i;
                const double y = 10.0 + 20.0 * j;
                points.addPoint(points.addVertex(Vec3(x, y, 0.05 * x + 0.02 * y), kPoint),
                                kPointSize, kEntityBias);
            }
        }
        bounds = ground.bounds();
    }
};

// Renders `passes` one after another into one buffer, as the 3D view draws
// its layers.
Framebuffer renderPasses(std::initializer_list<const DrawList*> passes, const Camera& camera)
{
    auto target = Framebuffer::create(camera.viewportWidth(), camera.viewportHeight());
    EXPECT_TRUE(target.ok());
    TaskPool pool(0);
    RenderOptions options;
    options.background = kBackground;
    options.pool = &pool;
    Rasterizer rasterizer;
    for (const DrawList* list : passes) {
        EXPECT_TRUE(rasterizer.render(*list, camera, *target, options).ok());
        options.clear = false;
    }
    return std::move(*target);
}

} // namespace

TEST(RenderDepth, SurveyPointsLyingOnASlopedSurfaceAreDrawnWhole)
{
    // A point is a 5 x 5 square at ONE depth, its centre's. Tested pixel by
    // pixel against the surface it lies on, it lost its lower rows: seen at
    // elevation e, a row k pixels below the centre looks at ground about
    // k / tan(e) footprints nearer. The surface is pushed back by one pixel
    // of its own slope and the point pulled 1.5 footprints, so the row two
    // down is covered once 1 / tan(e) > 1.5 (e below 0.59 rad; the slope
    // facing the eye steepens it at the iso view's 0.61) and the row one
    // down too below 0.3 rad. Decided at its centre, whose pixel centre is
    // at most half a pixel of slope from the point's own, a point is drawn
    // whole: in the surface's list or in a later pass.
    const SlopedSite site;
    DrawList together = site.ground;
    append(together, site.points);
    for (Projection projection : {Projection::Perspective, Projection::Orthographic}) {
        for (double elevation : {0.61, 0.25, 0.12}) {
            SCOPED_TRACE(std::to_string(static_cast<int>(projection)) + " at " +
                         std::to_string(elevation));
            Camera camera;
            camera.setViewportSize(1200, 800);
            camera.setProjection(projection);
            camera.setStandardView(StandardView::IsoSouthWest);
            camera.setOrientation(camera.azimuth(), elevation);
            ASSERT_TRUE(camera.frame(site.bounds));

            const std::size_t alone = countPixels(render(site.points, camera), kPoint);
            ASSERT_GT(alone, 2000u) << "the points are not in view, so this proves nothing";
            EXPECT_EQ(countPixels(render(together, camera), kPoint), alone);
            EXPECT_EQ(countPixels(renderPasses({&site.ground, &site.points}, camera), kPoint),
                      alone);
        }
    }
}

TEST(RenderDepth, APointOnTheGroundBehindABuildingIsHidden)
{
    // The middle of the line in ALineOnTheGroundBehindABuildingIsHidden,
    // (0, 30, 0): the ray from the eye to it meets the building's south face
    // at z = 13 (worked there). Deciding a point at its centre must not let
    // it through, in the building's list or in a later pass.
    DrawList building;
    addBox(building, Vec3(-20.0, -20.0, 0.0), Vec3(20.0, 20.0, 25.0), kBuilding);
    DrawList point;
    point.addPoint(point.addVertex(Vec3(0.0, 30.0, 0.0), kPoint), kPointSize, kEntityBias);
    DrawList together = building;
    append(together, point);

    for (Projection projection : {Projection::Perspective, Projection::Orthographic}) {
        SCOPED_TRACE(static_cast<int>(projection));
        Camera camera;
        camera.setViewportSize(640, 480);
        camera.setProjection(projection);
        camera.setStandardView(StandardView::Front); // looking north
        camera.setOrientation(camera.azimuth(), 0.25);
        camera.setTarget(Vec3(0.0, 0.0, 10.0));
        camera.setDistance(200.0);
        camera.setOrthographicHeight(120.0);
        katana::math::AABB bounds = building.bounds();
        bounds.expand(point.bounds());
        ASSERT_TRUE(camera.fitDepthRange(bounds));

        ASSERT_EQ(countPixels(render(point, camera), kPoint), 25u)
            << "the point is not in view, so its being hidden proves nothing";
        EXPECT_EQ(countPixels(render(together, camera), kPoint), 0u);
        EXPECT_EQ(countPixels(renderPasses({&building, &point}, camera), kPoint), 0u);
    }
}

TEST(RenderDepth, OfTwoOverlappingPointsTheNearerIsOnTopWhicheverIsDrawnFirst)
{
    // Seen from above in an orthographic view 100 m high on 500 px, a metre
    // is 5 px and the target (0, 0) is the pixel corner (250, 250). A point
    // at x = 0.25 m is centred on 251.25 and covers the pixels whose centres
    // are in [248.75, 253.75): 249..253; one at x = -0.15 m covers 247..251.
    // Both at y = 0.05 m cover the same five rows, so they share 3 x 5 = 15
    // pixels (a quarter pixel off the grid, so no rounding moves a square).
    // Those go to the nearer, higher point in either order: it shows all 25,
    // the lower one the other 10.
    constexpr Rgba kNear = rgba(0, 250, 250);
    constexpr Rgba kFar = rgba(250, 0, 250);
    for (bool nearFirst : {true, false}) {
        SCOPED_TRACE(nearFirst);
        DrawList list;
        const auto nearPoint = [&] {
            list.addPoint(list.addVertex(Vec3(0.25, 0.05, 10.0), kNear), kPointSize, kEntityBias);
        };
        const auto farPoint = [&] {
            list.addPoint(list.addVertex(Vec3(-0.15, 0.05, 0.0), kFar), kPointSize, kEntityBias);
        };
        if (nearFirst) {
            nearPoint();
            farPoint();
        } else {
            farPoint();
            nearPoint();
        }
        Camera camera;
        camera.setViewportSize(500, 500);
        camera.setProjection(Projection::Orthographic);
        camera.setStandardView(StandardView::Top);
        camera.setTarget(Vec3(0.0, 0.0, 5.0));
        camera.setOrthographicHeight(100.0);
        ASSERT_TRUE(camera.fitDepthRange(list.bounds()));
        const Framebuffer seen = render(list, camera);
        EXPECT_EQ(countPixels(seen, kNear), 25u);
        EXPECT_EQ(countPixels(seen, kFar), 10u);
    }
}

TEST(RenderDepth, APassThatWritesNoDepthIsCoveredByWhateverIsDrawnAfterIt)
{
    // The grid and the surface edges are drawn so (cad::renderLayers): a
    // pass with depthWrite off draws its colour where the test passes and
    // leaves the depth buffer as it found it, so a surface 5 cm BEHIND it,
    // drawn next, is tested against the cleared buffer and covers every
    // pixel it draws alone.
    const Overlap scene(0.05); // the design 5 cm above the ground
    const Camera camera = framedCamera(scene.bounds, Projection::Perspective);
    const std::size_t alone = countPixels(render(scene.ground, camera), kGround);
    ASSERT_GT(alone, 10000u);

    auto target = Framebuffer::create(camera.viewportWidth(), camera.viewportHeight());
    ASSERT_TRUE(target.ok());
    TaskPool pool(0);
    RenderOptions options;
    options.background = kBackground;
    options.pool = &pool;
    options.depthWrite = false;
    Rasterizer rasterizer;
    ASSERT_TRUE(rasterizer.render(scene.design, camera, *target, options).ok());
    EXPECT_GT(countPixels(*target, kDesign), 10000u);
    EXPECT_TRUE(std::all_of(target->depth().begin(), target->depth().end(),
                            [](float depth) { return depth == 0.0f; }))
        << "the pass wrote depth";
    options.depthWrite = true;
    options.clear = false;
    ASSERT_TRUE(rasterizer.render(scene.ground, camera, *target, options).ok());
    EXPECT_EQ(countPixels(*target, kGround), alone);
    EXPECT_EQ(countPixels(*target, kDesign), 0u);
}

TEST(RenderDepth, PointsDrawTheSamePixelsOnTheCallingThreadAndAcrossThreads)
{
    // Up to 4096 points are decided and filled on the calling thread (a
    // dispatch costs more; rasterisePoints), more across the pool. The same
    // frame both ways: 64 x 64 = 4096 points on the sloped site, then the
    // same plus one point 50 m under the ground in the middle of the view -
    // 4097, so the pool's path, and hidden, so it adds no pixel. Colour and
    // depth must match byte for byte on 0, 3 and 7 workers, and the points
    // must still be whole.
    DrawList ground;
    addPlane(ground, 0.0, 0.0, 400.0, 400.0, 0.0, 40, kGround, 0.05, 0.02);
    DrawList points;
    for (int j = 0; j < 64; ++j) {
        for (int i = 0; i < 64; ++i) {
            const double x = 3.0 + 6.2 * i;
            const double y = 3.0 + 6.2 * j;
            points.addPoint(points.addVertex(Vec3(x, y, 0.05 * x + 0.02 * y), kPoint), kPointSize,
                            kEntityBias);
        }
    }
    DrawList inlineScene = ground;
    append(inlineScene, points);
    DrawList pooledScene = inlineScene;
    pooledScene.addPoint(pooledScene.addVertex(Vec3(200.0, 200.0, 14.0 - 50.0), kPoint),
                         kPointSize, kEntityBias);

    Camera camera;
    camera.setViewportSize(800, 600);
    camera.setStandardView(StandardView::IsoSouthWest);
    camera.setOrientation(camera.azimuth(), 0.25);
    ASSERT_TRUE(camera.frame(ground.bounds()));
    // Framed on the ground alone, so the depth range must reach the point
    // under it too, and the point must be on the image, or it is clipped
    // before it is counted and both scenes take the same path.
    katana::math::AABB bounds = pooledScene.bounds();
    ASSERT_TRUE(camera.fitDepthRange(bounds));
    const auto under = camera.project(Vec3(200.0, 200.0, 14.0 - 50.0));
    ASSERT_TRUE(under.has_value());
    ASSERT_TRUE(under->x > 0.0 && under->x < 800.0 && under->y > 0.0 && under->y < 600.0)
        << "the hidden point is off the image, so this proves nothing";
    const std::size_t alone = countPixels(render(points, camera), kPoint);
    ASSERT_GT(alone, 20000u);

    const auto draw = [&camera](const DrawList& list, std::size_t workers) {
        auto target = Framebuffer::create(camera.viewportWidth(), camera.viewportHeight());
        EXPECT_TRUE(target.ok());
        TaskPool pool(workers);
        RenderOptions options;
        options.background = kBackground;
        options.pool = &pool;
        Rasterizer rasterizer;
        EXPECT_TRUE(rasterizer.render(list, camera, *target, options).ok());
        return std::move(*target);
    };
    const Framebuffer reference = draw(inlineScene, 3);
    EXPECT_EQ(countPixels(reference, kPoint), alone);
    for (std::size_t workers : {std::size_t{0}, std::size_t{3}, std::size_t{7}}) {
        SCOPED_TRACE(workers);
        const Framebuffer pooled = draw(pooledScene, workers);
        EXPECT_EQ(pooled.color(), reference.color());
        EXPECT_EQ(pooled.depth(), reference.depth());
    }
}
