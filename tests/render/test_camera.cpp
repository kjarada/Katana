// Camera conventions (PLAN.MD Phase 15).
//
// Every assertion here pins a convention the rasteriser and the pick ray both
// assume: Z up, screen +y down, NDC depth 1 at the near plane (reversed Z).
// Getting one of them backwards produces a picture that looks almost right,
// which is the worst kind of wrong, so they are asserted rather than commented.

#include <gtest/gtest.h>

#include <cmath>
#include <limits>
#include <utility>
#include <vector>

#include "katana/render/camera.hpp"

using katana::math::Vec3;
using katana::render::Camera;
using katana::render::Projection;
using katana::render::StandardView;

namespace {

constexpr double kPi = 3.14159265358979323846;

Camera defaultCamera(int width = 800, int height = 600)
{
    Camera camera;
    camera.setViewportSize(width, height);
    camera.setTarget(Vec3(0.0, 0.0, 0.0));
    camera.setDistance(100.0);
    return camera;
}

} // namespace

TEST(RenderCamera, TopViewLooksStraightDownWithNorthUp)
{
    Camera camera = defaultCamera();
    camera.setStandardView(StandardView::Top);

    // Looking down: forward is -Z (within the pole guard).
    EXPECT_NEAR(camera.forward().z, -1.0, 1e-3);
    EXPECT_GT(camera.eye().z, 0.0) << "the eye must be above the ground in plan view";

    // North (+Y) must appear at the top of the screen, i.e. along screen up.
    EXPECT_NEAR(camera.up().y, 1.0, 1e-3);
    // East (+X) must appear to the right.
    EXPECT_NEAR(camera.right().x, 1.0, 1e-3);
}

TEST(RenderCamera, FrontViewLooksNorthWithElevationUpTheScreen)
{
    Camera camera = defaultCamera();
    camera.setStandardView(StandardView::Front);

    EXPECT_NEAR(camera.forward().y, 1.0, 1e-9) << "front view looks north";
    EXPECT_NEAR(camera.forward().z, 0.0, 1e-9);
    // Elevation (world +Z) is up the screen: that is what makes a section view
    // read as a section rather than as a plan.
    EXPECT_NEAR(camera.up().z, 1.0, 1e-9);
    EXPECT_NEAR(camera.right().x, 1.0, 1e-9);
}

TEST(RenderCamera, ScreenYIncreasesDownwards)
{
    Camera camera = defaultCamera();
    camera.setStandardView(StandardView::Top);
    camera.setProjection(Projection::Orthographic);
    camera.setOrthographicHeight(100.0);
    camera.setDepthRange(1.0, 1000.0);

    const auto north = camera.project(Vec3(0.0, 10.0, 0.0));
    const auto south = camera.project(Vec3(0.0, -10.0, 0.0));
    ASSERT_TRUE(north.has_value());
    ASSERT_TRUE(south.has_value());
    EXPECT_LT(north->y, south->y) << "north must be nearer the top of the image";

    const auto centre = camera.project(Vec3(0.0, 0.0, 0.0));
    ASSERT_TRUE(centre.has_value());
    EXPECT_NEAR(centre->x, 400.0, 1e-6);
    EXPECT_NEAR(centre->y, 300.0, 1e-6);
}

TEST(RenderCamera, DepthIsOneAtTheNearPlaneAndZeroAtTheFarBecauseItIsReversed)
{
    // Reversed Z (camera.hpp, Clip): nearer is LARGER. The rasteriser's
    // strictly-greater test and its clear to 0 both rest on this.
    for (Projection projection : {Projection::Perspective, Projection::Orthographic}) {
        Camera camera = defaultCamera();
        camera.setProjection(projection);
        camera.setStandardView(StandardView::Front); // forward = +Y
        camera.setTarget(Vec3(0.0, 0.0, 0.0));
        camera.setDistance(100.0);
        camera.setDepthRange(10.0, 210.0);
        camera.setOrthographicHeight(50.0);

        // The eye is 100 south of the origin looking north, so the near plane
        // sits at y = -90 and the far plane at y = 110.
        const auto atNear = camera.project(Vec3(0.0, -90.0, 0.0));
        const auto atFar = camera.project(Vec3(0.0, 110.0, 0.0));
        ASSERT_TRUE(atNear.has_value());
        ASSERT_TRUE(atFar.has_value());
        EXPECT_NEAR(atNear->z, 1.0, 1e-9) << static_cast<int>(projection);
        EXPECT_NEAR(atFar->z, 0.0, 1e-9) << static_cast<int>(projection);
    }
}

TEST(RenderCamera, PointsBehindTheEyeDoNotProject)
{
    Camera camera = defaultCamera();
    camera.setStandardView(StandardView::Front); // eye at y = -100, looking north
    camera.setDepthRange(1.0, 1000.0);

    EXPECT_FALSE(camera.project(Vec3(0.0, -200.0, 0.0)).has_value())
        << "a point behind the eye would otherwise project to a mirrored pixel";
    EXPECT_TRUE(camera.project(Vec3(0.0, 0.0, 0.0)).has_value());
}

TEST(RenderCamera, PickRayThroughTheCentrePixelIsTheViewDirection)
{
    Camera camera = defaultCamera(801, 601); // odd, so there IS a centre pixel
    camera.setStandardView(StandardView::IsoSouthWest);

    const katana::math::Ray ray = camera.rayThroughPixel(400.0, 300.0);
    const Vec3 forward = camera.forward();
    EXPECT_NEAR(ray.direction.x, forward.x, 1e-9);
    EXPECT_NEAR(ray.direction.y, forward.y, 1e-9);
    EXPECT_NEAR(ray.direction.z, forward.z, 1e-9);
}

TEST(RenderCamera, PickRayAndProjectionAreInverses)
{
    Camera camera = defaultCamera();
    camera.setStandardView(StandardView::IsoNorthEast);
    camera.setTarget(Vec3(5.0, -3.0, 2.0));
    camera.setDistance(80.0);
    camera.setDepthRange(1.0, 1000.0);

    for (Projection projection : {Projection::Perspective, Projection::Orthographic}) {
        camera.setProjection(projection);
        for (const auto& pixel : {std::pair{123.0, 456.0}, std::pair{7.0, 11.0},
                                  std::pair{799.0, 599.0}}) {
            const katana::math::Ray ray = camera.rayThroughPixel(pixel.first, pixel.second);
            const Vec3 world = ray.at(50.0);
            const auto back = camera.project(world);
            ASSERT_TRUE(back.has_value());
            // The ray targets the CENTRE of the pixel, so projecting a point on
            // it lands half a pixel on.
            EXPECT_NEAR(back->x, pixel.first + 0.5, 1e-6);
            EXPECT_NEAR(back->y, pixel.second + 0.5, 1e-6);
        }
    }
}

TEST(RenderCamera, FramingABoxPutsEveryCornerOnScreenFromAnyDirection)
{
    const katana::math::AABB bounds(Vec3(100.0, 200.0, -5.0), Vec3(180.0, 260.0, 35.0));
    const auto corners = [&bounds](int corner) {
        return Vec3((corner & 1) ? bounds.max.x : bounds.min.x,
                    (corner & 2) ? bounds.max.y : bounds.min.y,
                    (corner & 4) ? bounds.max.z : bounds.min.z);
    };

    for (Projection projection : {Projection::Perspective, Projection::Orthographic}) {
        Camera camera = defaultCamera(640, 480);
        camera.setProjection(projection);
        // frame() fits the corners as seen from where the camera looks, so
        // it is asked from each direction of a full turn.
        for (int step = 0; step < 16; ++step) {
            camera.setOrientation(static_cast<double>(step) * kPi / 8.0, 0.4);
            ASSERT_TRUE(camera.frame(bounds));
            for (int corner = 0; corner < 8; ++corner) {
                const auto screen = camera.project(corners(corner));
                ASSERT_TRUE(screen.has_value()) << "corner " << corner << " step " << step;
                EXPECT_GE(screen->x, 0.0);
                EXPECT_LE(screen->x, 640.0);
                EXPECT_GE(screen->y, 0.0);
                EXPECT_LE(screen->y, 480.0);
                EXPECT_GE(screen->z, 0.0);
                EXPECT_LE(screen->z, 1.0);
            }
        }
    }
}

TEST(RenderCamera, AnOrbitAfterFramingKeepsTheBoxInsideTheFittedDepthRange)
{
    // The view fits the depth range to the scene every frame. Orbiting a full
    // turn after one frame() must never put a corner past either plane - the
    // planes frame() chose alone did, by up to 3% of the range.
    const katana::math::AABB bounds(Vec3(100.0, 200.0, -5.0), Vec3(180.0, 260.0, 35.0));
    for (Projection projection : {Projection::Perspective, Projection::Orthographic}) {
        Camera camera = defaultCamera(640, 480);
        camera.setProjection(projection);
        ASSERT_TRUE(camera.frame(bounds));
        for (int step = 0; step < 16; ++step) {
            camera.setOrientation(static_cast<double>(step) * kPi / 8.0, 0.4);
            ASSERT_TRUE(camera.fitDepthRange(bounds));
            for (int corner = 0; corner < 8; ++corner) {
                const Vec3 p((corner & 1) ? bounds.max.x : bounds.min.x,
                             (corner & 2) ? bounds.max.y : bounds.min.y,
                             (corner & 4) ? bounds.max.z : bounds.min.z);
                const auto screen = camera.project(p);
                ASSERT_TRUE(screen.has_value()) << "corner " << corner << " step " << step;
                EXPECT_GE(screen->z, 0.0);
                EXPECT_LE(screen->z, 1.0);
            }
        }
    }
}

TEST(RenderCamera, FramingALongCorridorFillsTheWidthNotASliverOfIt)
{
    // A 12 km x 200 m corridor seen from above in a 1600 x 1000 view. The
    // corners fit: half-width 6000 over an aspect of 1.6 needs a half-height
    // of 3750, against the corridor's own 100, so the width decides and the
    // corridor spans 1 / 1.06 = 94% of it. The bounding sphere (radius 6002)
    // made the view 2 x 6002 x 1.06 tall, 20 360 m wide: 59%.
    const katana::math::AABB corridor(Vec3(0.0, 0.0, 10.0), Vec3(12000.0, 200.0, 40.0));
    for (Projection projection : {Projection::Orthographic, Projection::Perspective}) {
        Camera camera = defaultCamera(1600, 1000);
        camera.setProjection(projection);
        camera.setStandardView(StandardView::Top);
        ASSERT_TRUE(camera.frame(corridor));
        const auto west = camera.project(Vec3(0.0, 100.0, 40.0));
        const auto east = camera.project(Vec3(12000.0, 100.0, 40.0));
        ASSERT_TRUE(west.has_value());
        ASSERT_TRUE(east.has_value());
        EXPECT_GT((east->x - west->x) / 1600.0, 0.9) << static_cast<int>(projection);
        EXPECT_GE(west->x, 0.0);
        EXPECT_LE(east->x, 1600.0);
    }
}

TEST(RenderCamera, AnOrthographicFrameInATallViewKeepsTheSidesIn)
{
    // Audit REN-08: the orthographic height ignored the aspect, so a portrait
    // view (345 x 545, a docked 3D view beside the plan) cut both sides off.
    const katana::math::AABB bounds(Vec3(0.0, 0.0, 0.0), Vec3(100.0, 50.0, 0.0));
    Camera camera = defaultCamera(345, 545);
    camera.setProjection(Projection::Orthographic);
    camera.setStandardView(StandardView::Top);
    ASSERT_TRUE(camera.frame(bounds));
    for (int corner = 0; corner < 4; ++corner) {
        const auto screen = camera.project(
            Vec3((corner & 1) ? 100.0 : 0.0, (corner & 2) ? 50.0 : 0.0, 0.0));
        ASSERT_TRUE(screen.has_value());
        EXPECT_GE(screen->x, 0.0);
        EXPECT_LE(screen->x, 345.0);
    }
}

TEST(RenderCamera, FramingOnePointShowsAPatchAroundItNotAMicrometre)
{
    // Audit REN-07: a box of one point framed at a radius of 1e-7 m, and no
    // zooming out recovered a useful view. It is grown to a half-diagonal of
    // kMinimumFrameRadius (1 m): half 1 / sqrt 3 each way, so points 0.5 m
    // either side of it are in view and not at the view's edge.
    const Vec3 point(300000.0, 6250000.0, 42.0);
    for (Projection projection : {Projection::Orthographic, Projection::Perspective}) {
        Camera camera = defaultCamera(400, 300);
        camera.setProjection(projection);
        camera.setStandardView(StandardView::Top);
        ASSERT_TRUE(camera.frame(katana::math::AABB(point, point)));
        EXPECT_GE(camera.worldPerPixel() * 300.0, 2.0 / std::sqrt(3.0));
        for (const double dx : {-0.5, 0.5}) {
            const auto screen = camera.project(point + Vec3(dx, 0.0, 0.0));
            ASSERT_TRUE(screen.has_value());
            EXPECT_GT(screen->x, 0.0);
            EXPECT_LT(screen->x, 400.0);
        }
    }
}

TEST(RenderCamera, FramingRejectsAnEmptyOrNonFiniteBox)
{
    Camera camera = defaultCamera();
    const double before = camera.distance();

    EXPECT_FALSE(camera.frame(katana::math::AABB{})); // default constructed is empty
    EXPECT_FALSE(camera.frame(katana::math::AABB(
        Vec3(0.0, 0.0, 0.0), Vec3(std::numeric_limits<double>::quiet_NaN(), 1.0, 1.0))));
    EXPECT_EQ(camera.distance(), before) << "a rejected frame must leave the view alone";
}

TEST(RenderCamera, ElevationIsClampedOffThePolesSoTheBasisNeverCollapses)
{
    Camera camera = defaultCamera();
    camera.setOrientation(0.0, 10.0); // far past straight down

    EXPECT_LT(camera.elevation(), kPi / 2.0);
    // The whole point: right() is a cross product with world +Z and must not
    // degenerate.
    EXPECT_NEAR(camera.right().length(), 1.0, 1e-12);
    EXPECT_NEAR(camera.up().length(), 1.0, 1e-12);
    EXPECT_NEAR(camera.forward().length(), 1.0, 1e-12);
    EXPECT_NEAR(camera.right().dot(camera.up()), 0.0, 1e-12);
    EXPECT_NEAR(camera.right().dot(camera.forward()), 0.0, 1e-12);
}

TEST(RenderCamera, ZoomAtAPixelKeepsThatWorldPointUnderTheCursor)
{
    // dollyAtPixel's own anchor: the point of the TARGET'S plane under the
    // pixel. That is all this pins - it passed while the 3D view's zoom
    // stalled, because the ground under the cursor is not on that plane. The
    // view moves the pivot to the ground first (setPivotDepth), pinned by
    // AfterMovingThePivotToAPointEachDollyMagnifiesItByExactlyTheFactor.
    for (Projection projection : {Projection::Perspective, Projection::Orthographic}) {
        Camera camera = defaultCamera();
        camera.setProjection(projection);
        camera.setStandardView(StandardView::IsoSouthWest);
        camera.setDepthRange(1.0, 10000.0);
        camera.setOrthographicHeight(100.0);

        const double px = 620.0;
        const double py = 140.0;
        // The world point on the target plane that the cursor is over.
        const katana::math::Ray ray = camera.rayThroughPixel(px, py);
        const double t = (camera.target() - ray.origin).dot(camera.forward()) /
                         ray.direction.dot(camera.forward());
        const Vec3 anchor = ray.at(t);

        camera.dollyAtPixel(0.5, px, py);

        const auto after = camera.project(anchor);
        ASSERT_TRUE(after.has_value());
        EXPECT_NEAR(after->x, px + 0.5, 1e-6);
        EXPECT_NEAR(after->y, py + 0.5, 1e-6);
    }
}

TEST(RenderCamera, MovingThePivotAlongTheViewAxisMovesNothingOnScreen)
{
    // setPivotDepth puts the target `depth` in front of the eye on the view
    // axis and the distance to `depth`: by its definition the eye does not
    // move, so no point projects anywhere else. Under a perspective
    // projection the orthographic height becomes what the view shows at that
    // depth, 2 x 30 x tan(22.5 deg) = 24.852813742385702 (tan 22.5 deg =
    // sqrt 2 - 1); under an orthographic one it is the picture, and stays.
    for (Projection projection : {Projection::Perspective, Projection::Orthographic}) {
        SCOPED_TRACE(static_cast<int>(projection));
        Camera camera = defaultCamera();
        camera.setProjection(projection);
        camera.setStandardView(StandardView::IsoNorthEast);
        camera.setTarget(Vec3(5.0, -3.0, 2.0));
        camera.setDistance(80.0);
        camera.setOrthographicHeight(60.0);
        // A box reaching behind the orthographic eye gives it a standoff,
        // which the move must fold in rather than lose.
        ASSERT_TRUE(camera.fitDepthRange(
            katana::math::AABB(Vec3(-200.0, -200.0, -50.0), Vec3(200.0, 200.0, 50.0))));
        const Vec3 probes[] = {Vec3(0.0, 0.0, 0.0), Vec3(12.0, 7.0, -4.0), Vec3(-9.0, 15.0, 6.0)};
        std::vector<Vec3> before;
        for (const Vec3& probe : probes) {
            const auto screen = camera.project(probe);
            ASSERT_TRUE(screen.has_value());
            before.push_back(*screen);
        }
        const Vec3 eye = camera.eye();
        const Vec3 forward = camera.forward();

        camera.setPivotDepth(30.0);

        EXPECT_NEAR((camera.eye() - eye).length(), 0.0, 1e-12);
        EXPECT_NEAR((camera.target() - (eye + forward * 30.0)).length(), 0.0, 1e-12);
        EXPECT_DOUBLE_EQ(camera.distance(), 30.0);
        EXPECT_NEAR(camera.orthographicHeight(),
                    projection == Projection::Perspective ? 24.852813742385702 : 60.0, 1e-12);
        for (std::size_t k = 0; k < before.size(); ++k) {
            const auto after = camera.project(probes[k]);
            ASSERT_TRUE(after.has_value());
            EXPECT_NEAR(after->x, before[k].x, 1e-9);
            EXPECT_NEAR(after->y, before[k].y, 1e-9);
        }
    }
}

TEST(RenderCamera, APivotDepthThatIsNotPositiveAndFiniteIsIgnored)
{
    for (const double depth : {0.0, -5.0, std::numeric_limits<double>::quiet_NaN(),
                               std::numeric_limits<double>::infinity()}) {
        Camera camera = defaultCamera();
        camera.setStandardView(StandardView::IsoSouthWest);
        const Vec3 target = camera.target();
        camera.setPivotDepth(depth);
        EXPECT_EQ(camera.target().x, target.x) << depth;
        EXPECT_EQ(camera.target().y, target.y) << depth;
        EXPECT_EQ(camera.target().z, target.z) << depth;
        EXPECT_EQ(camera.distance(), 100.0) << depth;
    }
}

TEST(RenderCamera, AfterMovingThePivotToAPointEachDollyMagnifiesItByExactlyTheFactor)
{
    // Ground at z = -10 under a target at the origin 100 from the eye, seen
    // from the south-west at 1200 x 800: the ground under pixel (700, 150)
    // lies beyond the target's plane. Anchored on that plane, a notch
    // magnified the ground there by less each time, tending to depth /
    // (depth - distance), and it stopped: the stall. With the pivot moved to
    // the ground's depth first, each dolly by 1 / 1.15 divides the ground's
    // depth, and so its footprint, by exactly 1.15, and the same point of
    // the ground stays at the pixel's centre.
    Camera camera = defaultCamera(1200, 800);
    camera.setStandardView(StandardView::IsoSouthWest);
    const double px = 700.0;
    const double py = 150.0;
    const auto groundUnder = [&camera, px, py] {
        const katana::math::Ray ray = camera.rayThroughPixel(px, py);
        return ray.at((-10.0 - ray.origin.z) / ray.direction.z);
    };
    const auto depthOf = [&camera](const Vec3& p) {
        return (p - camera.eye()).dot(camera.forward());
    };
    const Vec3 ground = groundUnder();
    const double first = depthOf(ground);
    ASSERT_GT(first, 1.2 * camera.distance()) << "not beyond the target's plane: proves nothing";
    double depth = first;
    for (int notch = 1; notch <= 50; ++notch) {
        camera.setPivotDepth(depthOf(groundUnder()));
        camera.dollyAtPixel(1.0 / 1.15, px, py);
        const double now = depthOf(groundUnder());
        ASSERT_NEAR(depth / now, 1.15, 1e-9) << "notch " << notch;
        depth = now;
        const auto screen = camera.project(ground);
        ASSERT_TRUE(screen.has_value());
        ASSERT_NEAR(screen->x, px + 0.5, 1e-6) << "notch " << notch;
        ASSERT_NEAR(screen->y, py + 0.5, 1e-6) << "notch " << notch;
    }
    EXPECT_NEAR(first / depth, std::pow(1.15, 50.0), 1e-6 * std::pow(1.15, 50.0));
}

TEST(RenderCamera, PanningMovesTheSceneWithTheDrag)
{
    Camera camera = defaultCamera();
    camera.setStandardView(StandardView::Top);
    camera.setProjection(Projection::Orthographic);
    camera.setOrthographicHeight(600.0); // exactly 1 world unit per pixel
    camera.setDepthRange(1.0, 1000.0);

    const auto before = camera.project(Vec3(0.0, 0.0, 0.0));
    ASSERT_TRUE(before.has_value());

    camera.panPixels(30.0, 20.0); // drag right and down

    const auto after = camera.project(Vec3(0.0, 0.0, 0.0));
    ASSERT_TRUE(after.has_value());
    EXPECT_NEAR(after->x - before->x, 30.0, 1e-6) << "the scene follows the drag";
    EXPECT_NEAR(after->y - before->y, 20.0, 1e-6);
}

TEST(RenderCamera, WorldPerPixelMatchesWhatProjectionActuallyDoes)
{
    Camera camera = defaultCamera(800, 600);
    // Front, not Top. Top is clamped a milliradian off vertical by the pole
    // guard, so a displacement in the ground plane is foreshortened by
    // cos(1e-3) = 0.9999995 and the identity below only holds to 5e-7. Front
    // sits at elevation 0, where nothing is clamped and the relation is exact -
    // which is what makes this a test of worldPerPixel rather than of the guard.
    camera.setStandardView(StandardView::Front);
    camera.setProjection(Projection::Orthographic);
    camera.setOrthographicHeight(120.0);
    camera.setDepthRange(1.0, 1000.0);

    EXPECT_NEAR(camera.worldPerPixel(), 0.2, 1e-12);

    // Screen up is world +Z in the front view.
    const auto a = camera.project(Vec3(0.0, 0.0, 0.0));
    const auto b = camera.project(Vec3(0.0, 0.0, 1.0));
    ASSERT_TRUE(a.has_value());
    ASSERT_TRUE(b.has_value());
    EXPECT_NEAR(std::abs(b->y - a->y) * camera.worldPerPixel(), 1.0, 1e-12);

    // And horizontally, which shares no code path with the vertical extent.
    const auto c = camera.project(Vec3(1.0, 0.0, 0.0));
    ASSERT_TRUE(c.has_value());
    EXPECT_NEAR(std::abs(c->x - a->x) * camera.worldPerPixel(), 1.0, 1e-12);
}

TEST(RenderCamera, ThePoleGuardCostsExactlyTheCosineOfItsAngle)
{
    // Plan view is a milliradian off vertical on purpose (see camera.hpp). That
    // is invisible on screen but it is not nothing, and pinning the size of it
    // here means a later change to the guard shows up as a failure rather than
    // as a drawing that is quietly 0.00005% out of scale.
    Camera camera = defaultCamera(800, 600);
    camera.setStandardView(StandardView::Top);
    camera.setProjection(Projection::Orthographic);
    camera.setOrthographicHeight(120.0);
    camera.setDepthRange(1.0, 1000.0);

    const auto a = camera.project(Vec3(0.0, 0.0, 0.0));
    const auto b = camera.project(Vec3(0.0, 1.0, 0.0));
    ASSERT_TRUE(a.has_value());
    ASSERT_TRUE(b.has_value());
    const double measured = std::abs(b->y - a->y) * camera.worldPerPixel();
    EXPECT_NEAR(measured, std::cos(1.0e-3), 1e-12);
    EXPECT_NEAR(measured, 1.0, 1e-6) << "still far below one pixel at any sane scale";
}
