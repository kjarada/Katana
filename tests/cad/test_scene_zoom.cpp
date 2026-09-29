// Zooming a 3D view towards what it draws (cad/scene_zoom.hpp; docs/render.md,
// "Zooming towards the cursor").
//
// The pick's expected values are worked by hand from scenes chosen so that
// the arithmetic is short: a camera looking due north from (0, -300, 5) at a
// block and the ground, where every depth is a difference of northings. The
// zoom's are properties of the zoom's definition - a notch magnifies what is
// under the cursor by 1.15, so its depth divides by 1.15 - measured against
// the plane the cursor's ray is met with here, never against what the zoom
// says it picked.

#include <gtest/gtest.h>

#include <cmath>
#include <cstdint>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "katana/cad/scene.hpp"
#include "katana/cad/scene_zoom.hpp"
#include "katana/commands/entity_commands.hpp"

using katana::cad::Document;
using katana::cad::minimumApproach;
using katana::cad::pickDrawnPoint;
using katana::cad::PickOptions;
using katana::cad::SceneLayers;
using katana::cad::ScenePick;
using katana::cad::zoomAtPixel;
using katana::math::Vec3;
using katana::render::Camera;
using katana::render::DrawList;
using katana::render::Projection;
using katana::render::StandardView;

namespace {

constexpr katana::render::Rgba kInk = katana::render::rgba(200, 200, 200);

// The rectangle [x0, x1] x [y0, y1] at height z, two triangles.
void addRectangle(DrawList& list, double x0, double y0, double x1, double y1, double z)
{
    const auto a = list.addVertex(Vec3(x0, y0, z), kInk);
    const auto b = list.addVertex(Vec3(x1, y0, z), kInk);
    const auto c = list.addVertex(Vec3(x1, y1, z), kInk);
    const auto d = list.addVertex(Vec3(x0, y1, z), kInk);
    list.addTriangle(a, b, c);
    list.addTriangle(a, c, d);
}

// A closed box, twelve triangles.
void addBox(DrawList& list, const Vec3& lo, const Vec3& hi)
{
    katana::render::VertexIndex corner[8];
    for (int k = 0; k < 8; ++k) {
        corner[k] = list.addVertex(Vec3((k & 1) != 0 ? hi.x : lo.x, (k & 2) != 0 ? hi.y : lo.y,
                                        (k & 4) != 0 ? hi.z : lo.z),
                                   kInk);
    }
    const int faces[6][4] = {{0, 1, 3, 2}, {4, 6, 7, 5}, {0, 4, 5, 1},
                             {2, 3, 7, 6}, {0, 2, 6, 4}, {1, 5, 7, 3}};
    for (const auto& f : faces) {
        list.addTriangle(corner[f[0]], corner[f[1]], corner[f[2]]);
        list.addTriangle(corner[f[0]], corner[f[2]], corner[f[3]]);
    }
}

// The scene the pick is worked on: ground 200 m square at z = 0, a block
// 10 m square and 10 m high in its middle, a line in the air in front of
// the block, a line behind it at the eye's height, a point, and the grid's
// square 260 m across on the datum. Laid out as SceneBuilder lays out a
// scene: the ground and the block in the terrain, the rest in the drawing.
SceneLayers pickScene()
{
    SceneLayers layers;
    addRectangle(layers.terrain, -100.0, -100.0, 100.0, 100.0, 0.0);
    addBox(layers.terrain, Vec3(-5.0, -5.0, 0.0), Vec3(5.0, 5.0, 10.0));
    layers.entities.addSegment(Vec3(-30.0, -50.0, 1.0), Vec3(30.0, -50.0, 1.0), kInk, 1.0f, 1.5f);
    layers.entities.addSegment(Vec3(-30.0, 20.0, 5.0), Vec3(30.0, 20.0, 5.0), kInk, 1.0f, 1.5f);
    layers.entities.addPoint(layers.entities.addVertex(Vec3(10.0, -30.0, 7.0), kInk), 5.0f, 1.5f);
    const Vec3 corners[4] = {{-130.0, -130.0, 0.0}, {130.0, -130.0, 0.0}, {130.0, 130.0, 0.0},
                             {-130.0, 130.0, 0.0}};
    for (int k = 0; k < 4; ++k) {
        layers.grid.addSegment(corners[k], corners[(k + 1) % 4], kInk);
    }
    layers.datum = 0.0;
    layers.bounds = layers.terrain.bounds();
    layers.bounds.expand(layers.entities.bounds());
    return layers;
}

// Looking due north from (0, -300, 5): the depth of a point is its northing
// plus 300.
Camera northCamera()
{
    Camera camera;
    camera.setViewportSize(400, 300);
    camera.setStandardView(StandardView::Front);
    camera.setTarget(Vec3(0.0, 0.0, 5.0));
    camera.setDistance(300.0);
    return camera;
}

// The pixel whose ray passes through `world`: rayThroughPixel(x, y) goes
// through the pixel's centre, (x + 0.5, y + 0.5) on the screen.
std::pair<double, double> pixelOf(const Camera& camera, const Vec3& world)
{
    const auto screen = camera.project(world);
    EXPECT_TRUE(screen.has_value());
    return screen ? std::pair{screen->x - 0.5, screen->y - 0.5} : std::pair{0.0, 0.0};
}

void expectPick(const std::optional<ScenePick>& pick, ScenePick::Source source, double depth,
                const Vec3& point)
{
    ASSERT_TRUE(pick.has_value());
    EXPECT_EQ(pick->source, source);
    EXPECT_NEAR(pick->depth, depth, 1e-9 * depth);
    EXPECT_NEAR((pick->point - point).length(), 0.0, 1e-9 * depth);
}

} // namespace

// ---- the pick ------------------------------------------------------------------------------

TEST(ScenePick, TheNearestTriangleUnderThePixelIsFoundAndALineBehindItIsNot)
{
    // The middle pixel's ray runs due north at z = 5. It meets the block's
    // south face (y = -5) 295 m from the eye, at (0, -5, 5) - exactly on the
    // diagonal the face is cut along, which both of its triangles count -
    // before the line behind the block, which lies on the same ray at
    // y = 20, 320 m away.
    const SceneLayers layers = pickScene();
    const Camera camera = northCamera();
    expectPick(pickDrawnPoint(layers, camera, 199.5, 149.5), ScenePick::Source::Terrain, 295.0,
               Vec3(0.0, -5.0, 5.0));

    // The ground 60 m south of the middle: 240 m away, with nothing before it.
    const auto [gx, gy] = pixelOf(camera, Vec3(0.0, -60.0, 0.0));
    expectPick(pickDrawnPoint(layers, camera, gx, gy), ScenePick::Source::Terrain, 240.0,
               Vec3(0.0, -60.0, 0.0));
}

TEST(ScenePick, ALineOrAPointWithinTheApertureIsFoundInFrontOfWhatIsBehindIt)
{
    // The ray through (0, -50, 1), on the line in the air, goes on down to
    // the block's south face 295 m away, 4.72 m lower; the line, 250 m away,
    // is nearer. Four pixels above it the line is 4 x 250 x 2 tan(22.5 deg)
    // / 300 = 2.76 m below the ray, outside the 3 px aperture (2.07 m
    // there), and the ray goes on to the block's face.
    const SceneLayers layers = pickScene();
    const Camera camera = northCamera();
    const auto [lx, ly] = pixelOf(camera, Vec3(0.0, -50.0, 1.0));
    expectPick(pickDrawnPoint(layers, camera, lx, ly), ScenePick::Source::Drawing, 250.0,
               Vec3(0.0, -50.0, 1.0));
    const auto above = pickDrawnPoint(layers, camera, lx, ly - 4.0);
    ASSERT_TRUE(above.has_value());
    EXPECT_EQ(above->source, ScenePick::Source::Terrain);
    EXPECT_NEAR(above->depth, 295.0, 1e-9 * 295.0);
    EXPECT_NEAR(above->point.y, -5.0, 1e-9);
    // Two pixels above it, inside the aperture, the line is still what is
    // there: its point nearest the ray, straight below it. (Two pixels BELOW
    // it the ray meets the ground 232 m away, in front of the line, and the
    // ground is what the pixel shows.)
    expectPick(pickDrawnPoint(layers, camera, lx, ly - 2.0), ScenePick::Source::Drawing, 250.0,
               Vec3(0.0, -50.0, 1.0));
    const auto below = pickDrawnPoint(layers, camera, lx, ly + 2.0);
    ASSERT_TRUE(below.has_value());
    EXPECT_EQ(below->source, ScenePick::Source::Terrain);
    EXPECT_LT(below->depth, 250.0);
    // With no aperture, the line must be exactly under the pixel's centre.
    PickOptions exact;
    exact.aperture = 0.0;
    const auto missed = pickDrawnPoint(layers, camera, lx, ly - 2.0, exact);
    ASSERT_TRUE(missed.has_value());
    EXPECT_EQ(missed->source, ScenePick::Source::Terrain);
    EXPECT_NEAR(missed->depth, 295.0, 1e-9 * 295.0);

    // The point at (10, -30, 7): 270 m away, the block behind it.
    const auto [px, py] = pixelOf(camera, Vec3(10.0, -30.0, 7.0));
    expectPick(pickDrawnPoint(layers, camera, px, py), ScenePick::Source::Drawing, 270.0,
               Vec3(10.0, -30.0, 7.0));
}

TEST(ScenePick, WithNothingDrawnThereThePickIsTheDatumsPlaneInsideTheScenesBoxOnly)
{
    // (0, -115, 0) is off the ground (which ends at y = -100) and inside the
    // grid's square (which reaches y = -130): the datum's plane, 185 m away.
    // (0, -140, 0) is past the grid: nothing, rather than a pivot out where
    // nothing is drawn.
    const SceneLayers layers = pickScene();
    const Camera camera = northCamera();
    const auto [dx, dy] = pixelOf(camera, Vec3(0.0, -115.0, 0.0));
    expectPick(pickDrawnPoint(layers, camera, dx, dy), ScenePick::Source::Datum, 185.0,
               Vec3(0.0, -115.0, 0.0));
    const auto [ox, oy] = pixelOf(camera, Vec3(0.0, -140.0, 0.0));
    EXPECT_FALSE(pickDrawnPoint(layers, camera, ox, oy).has_value());
    // A ray rising into the sky meets neither the scene nor the datum.
    EXPECT_FALSE(pickDrawnPoint(layers, camera, 199.5, 20.0).has_value());
}

TEST(ScenePick, ARayParallelToTheDatumAnEmptySceneAndAnUnusablePixelPickNothing)
{
    const SceneLayers layers = pickScene();
    const Camera camera = northCamera();
    // The middle row runs level at z = 5; at the right edge it passes the
    // block by 160 m and never comes down to the datum.
    EXPECT_FALSE(pickDrawnPoint(layers, camera, 399.0, 149.5).has_value());
    // No scene at all: no box, so not even the datum.
    EXPECT_FALSE(pickDrawnPoint(SceneLayers{}, camera, 199.5, 149.5).has_value());
    // A pixel that is not a number, and a camera with no viewport.
    EXPECT_FALSE(pickDrawnPoint(layers, camera, std::nan(""), 149.5).has_value());
    EXPECT_FALSE(pickDrawnPoint(layers, camera, 199.5, HUGE_VAL).has_value());
    Camera unsized = camera;
    unsized.setViewportSize(0, 0);
    EXPECT_FALSE(pickDrawnPoint(layers, unsized, 199.5, 149.5).has_value());
}

namespace {

// Rolling ground 400 m square in 40 x 40 cells, 3D strings in the air over
// it, survey points at their own heights and plan linework draped on it,
// built by SceneBuilder as the 3D view builds it.
struct Survey {
    katana::terrain::TinSurface surface;
    Document document;
    SceneLayers layers;

    Survey()
    {
        const auto height = [](double x, double y) {
            return 20.0 + 6.0 * std::sin(x / 57.0) * std::cos(y / 83.0);
        };
        std::vector<katana::geometry::Point3> vertices;
        for (int j = 0; j <= 40; ++j) {
            for (int i = 0; i <= 40; ++i) {
                vertices.push_back({10.0 * i, 10.0 * j, height(10.0 * i, 10.0 * j)});
            }
        }
        std::vector<katana::terrain::TinTriangle> triangles;
        for (int j = 0; j < 40; ++j) {
            for (int i = 0; i < 40; ++i) {
                const auto v0 = static_cast<std::uint32_t>(j * 41 + i);
                triangles.push_back({v0, v0 + 1, v0 + 42});
                triangles.push_back({v0, v0 + 42, v0 + 41});
            }
        }
        auto built = katana::terrain::TinSurface::create(std::move(vertices), std::move(triangles));
        EXPECT_TRUE(built.ok());
        if (built.ok()) {
            surface = std::move(*built);
        }
        std::vector<katana::entity::Entity> entities;
        for (int k = 0; k < 8; ++k) {
            katana::entity::Entity string;
            katana::geometry::Polyline2 path;
            std::vector<std::optional<double>> heights;
            for (int v = 0; v <= 10; ++v) {
                path.vertices.emplace_back(20.0 + 36.0 * v, 30.0 + 45.0 * k);
                heights.emplace_back(40.0 + k);
            }
            string.geometry = path;
            katana::entity::setHeights(string.properties, heights);
            entities.push_back(std::move(string));

            katana::entity::Entity point;
            point.geometry = katana::entity::PointGeometry{
                katana::geometry::Point2(50.0 + 40.0 * k, 370.0 - 40.0 * k)};
            katana::entity::setHeights(point.properties, {35.0});
            entities.push_back(std::move(point));

            katana::entity::Entity plan;
            katana::geometry::Polyline2 draped;
            draped.vertices = {katana::geometry::Point2(15.0 + 45.0 * k, 10.0),
                               katana::geometry::Point2(15.0 + 45.0 * k, 390.0)};
            plan.geometry = draped;
            entities.push_back(std::move(plan));
        }
        EXPECT_TRUE(document.execute(katana::commands::createEntities(std::move(entities))).ok());

        katana::cad::SceneSurface item;
        item.name = "Ground";
        item.surface = &surface;
        const std::vector<katana::cad::SceneSurface> surfaces{item};
        const katana::cad::SceneOptions options;
        katana::cad::SceneBuilder builder;
        builder.buildTerrain(surfaces, {}, options, layers);
        builder.buildEntities(document, surfaces, options, layers);
        builder.buildSelection(document, surfaces, options, layers);
        builder.buildGrid(options, layers);
    }
};

} // namespace

TEST(ScenePick, TheCulledPickFindsExactlyWhatTheFullOneDoes)
{
    // The cull skips a primitive whose vertices all lie beyond one side of
    // the ray (a triangle) or of the aperture (a line or a point), from one
    // pass over the vertices. It must change no answer: every 6th pixel of
    // two views of the survey, each projection, picked both ways - and the
    // pixels must reach every kind of answer, or they prove nothing about it.
    const Survey survey;
    ASSERT_FALSE(survey.layers.terrain.triangles.empty());
    ASSERT_FALSE(survey.layers.entities.lines.empty());
    ASSERT_FALSE(survey.layers.entities.points.empty());
    PickOptions full;
    full.cull = false;
    int terrain = 0;
    int drawing = 0;
    int datum = 0;
    int nothing = 0;
    for (const Projection projection : {Projection::Perspective, Projection::Orthographic}) {
        for (const double elevation : {0.61, 0.2}) {
            Camera camera;
            camera.setViewportSize(480, 320);
            camera.setProjection(projection);
            camera.setStandardView(StandardView::IsoSouthWest);
            camera.setOrientation(camera.azimuth(), elevation);
            ASSERT_TRUE(camera.frame(survey.layers.bounds, 0.3));
            for (int y = 0; y < 320; y += 6) {
                for (int x = 0; x < 480; x += 6) {
                    const auto culled = pickDrawnPoint(survey.layers, camera, x, y);
                    const auto every = pickDrawnPoint(survey.layers, camera, x, y, full);
                    ASSERT_EQ(culled.has_value(), every.has_value()) << x << "," << y;
                    if (!culled) {
                        ++nothing;
                        continue;
                    }
                    EXPECT_EQ(culled->depth, every->depth) << x << "," << y;
                    EXPECT_EQ(culled->point.x, every->point.x) << x << "," << y;
                    EXPECT_EQ(culled->point.y, every->point.y) << x << "," << y;
                    EXPECT_EQ(culled->point.z, every->point.z) << x << "," << y;
                    EXPECT_EQ(culled->source, every->source) << x << "," << y;
                    terrain += culled->source == ScenePick::Source::Terrain ? 1 : 0;
                    drawing += culled->source == ScenePick::Source::Drawing ? 1 : 0;
                    datum += culled->source == ScenePick::Source::Datum ? 1 : 0;
                }
            }
        }
    }
    RecordProperty("terrain", terrain);
    RecordProperty("drawing", drawing);
    RecordProperty("datum", datum);
    RecordProperty("nothing", nothing);
    EXPECT_GT(terrain, 1000);
    EXPECT_GT(drawing, 100);
    EXPECT_GT(datum, 100);
    EXPECT_GT(nothing, 100);
}

// ---- the zoom ----------------------------------------------------------------------------

namespace {

constexpr double kPlane = -10.0;
constexpr double kPerNotch = 1.15;

// Ground 200 m square at z = -10 under a camera framed as the audit framed
// it: target (0, 0, 0) 100 m from the eye, from the south-west and 35 deg up,
// 1200 x 800. The target is 10 m above the ground, so the ground above the
// middle of the view lies beyond the target's plane and the ground below it
// nearer.
SceneLayers planeScene()
{
    SceneLayers layers;
    addRectangle(layers.terrain, -100.0, -100.0, 100.0, 100.0, kPlane);
    layers.datum = kPlane;
    layers.bounds = layers.terrain.bounds();
    return layers;
}

Camera planeCamera(Projection projection = Projection::Perspective)
{
    Camera camera;
    camera.setViewportSize(1200, 800);
    camera.setProjection(projection);
    camera.setStandardView(StandardView::IsoSouthWest);
    camera.setTarget(Vec3(0.0, 0.0, 0.0));
    camera.setDistance(100.0);
    camera.setOrthographicHeight(100.0);
    return camera;
}

// Where the ray through pixel (x, y) meets the plane z = -10, and how far that
// is in front of the eye: by hand, not by the pick.
std::optional<Vec3> planeUnder(const Camera& camera, double x, double y)
{
    const katana::math::Ray ray = camera.rayThroughPixel(x, y);
    if (ray.direction.z == 0.0) {
        return std::nullopt;
    }
    const double t = (kPlane - ray.origin.z) / ray.direction.z;
    return t > 0.0 ? std::optional<Vec3>(ray.at(t)) : std::nullopt;
}

double depthOf(const Camera& camera, const Vec3& point)
{
    return (point - camera.eye()).dot(camera.forward());
}

} // namespace

TEST(SceneZoom, EveryNotchMagnifiesWhatIsDrawnUnderTheCursorByTheNotchFactor)
{
    // Above the middle, the ground lies beyond the target's plane: anchored
    // on that plane, the sample terrain's ground was magnified 4.8x in 30
    // notches and 5% more in the next ten (docs/render.md). Anchored on the
    // ground, the depth of the ground under the cursor divides by exactly
    // 1.15 a notch, and the same point of the ground stays under the
    // cursor's pixel.
    const SceneLayers layers = planeScene();
    Camera camera = planeCamera();
    const double px = 700.0;
    const double py = 150.0;
    const auto start = planeUnder(camera, px, py);
    ASSERT_TRUE(start.has_value());
    const double first = depthOf(camera, *start);
    ASSERT_GT(first, 1.2 * camera.distance()) << "not beyond the target's plane: proves nothing";
    double depth = first;
    for (int notch = 1; notch <= 50; ++notch) {
        const auto result = zoomAtPixel(layers, camera, 1.0, px, py);
        ASSERT_TRUE(result.anchor.has_value()) << notch;
        EXPECT_EQ(result.anchor->source, ScenePick::Source::Terrain);
        EXPECT_FALSE(result.limited);
        const auto under = planeUnder(camera, px, py);
        ASSERT_TRUE(under.has_value()) << notch;
        const double now = depthOf(camera, *under);
        ASSERT_NEAR(depth / now, kPerNotch, 1e-9) << "notch " << notch;
        depth = now;
        const auto screen = camera.project(*start);
        ASSERT_TRUE(screen.has_value());
        ASSERT_NEAR(screen->x, px + 0.5, 1e-6) << "notch " << notch;
        ASSERT_NEAR(screen->y, py + 0.5, 1e-6) << "notch " << notch;
    }
    EXPECT_NEAR(first / depth, std::pow(kPerNotch, 50.0), 1e-6 * std::pow(kPerNotch, 50.0));
}

TEST(SceneZoom, ZoomingInNeverTakesTheEyeThroughWhatIsUnderTheCursor)
{
    // Below the middle the ground is nearer than the target's plane, and the
    // old anchor was under it: the eye went through the ground at notch
    // -ln(1 - depth / distance) / ln 1.15. Anchored on the ground, the eye
    // closes 13% of the way each notch and stays above it.
    const SceneLayers layers = planeScene();
    Camera camera = planeCamera();
    const double px = 500.0;
    const double py = 780.0;
    const auto start = planeUnder(camera, px, py);
    ASSERT_TRUE(start.has_value());
    const double first = depthOf(camera, *start);
    ASSERT_LT(first, 0.8 * camera.distance()) << "not nearer than the target's plane";
    const double crossing = -std::log(1.0 - first / camera.distance()) / std::log(kPerNotch);
    ASSERT_LT(crossing, 20.0) << "the old anchor would not have gone through: proves nothing";
    for (int notch = 1; notch <= 40; ++notch) {
        (void)zoomAtPixel(layers, camera, 1.0, px, py);
        ASSERT_GT(camera.eye().z, kPlane) << "the eye is under the ground at notch " << notch;
        const auto under = planeUnder(camera, px, py);
        ASSERT_TRUE(under.has_value()) << notch;
        EXPECT_NEAR(depthOf(camera, *under), first * std::pow(kPerNotch, -notch),
                    1e-9 * first * std::pow(kPerNotch, -notch))
            << "notch " << notch;
    }
}

TEST(SceneZoom, ZoomingInStopsAtTheMinimumApproachAndANotchOutStillShrinksByTheFactor)
{
    // The box is the ground's: 200 x 200 x 0, a diagonal of 282.84 m, so the
    // eye comes no nearer than 1e-4 of it, 2.828 cm, where the GPU's float
    // error is still a sixth of a pixel (scene_zoom.hpp). The zoom stops
    // there, the point still under the cursor. Part of the box is then
    // behind the eye, so fitDepthRange puts the near plane at a millionth of
    // the far one, and the far one is no further than the box's diagonal
    // plus a thousandth of it: the near plane is 99 times nearer than the
    // limit at least.
    const SceneLayers layers = planeScene();
    EXPECT_NEAR(minimumApproach(layers), 1e-4 * std::sqrt(2.0) * 200.0, 1e-15);
    Camera camera = planeCamera();
    const double px = 700.0;
    const double py = 150.0;
    const auto start = planeUnder(camera, px, py);
    ASSERT_TRUE(start.has_value());
    bool limited = false;
    for (int notch = 0; notch < 120; ++notch) {
        limited = zoomAtPixel(layers, camera, 1.0, px, py).limited || limited;
    }
    EXPECT_TRUE(limited);
    const auto under = planeUnder(camera, px, py);
    ASSERT_TRUE(under.has_value());
    const double depth = depthOf(camera, *under);
    EXPECT_NEAR(depth, minimumApproach(layers), 1e-9 * minimumApproach(layers));
    EXPECT_NEAR((*under - *start).length(), 0.0, 1e-6);
    // Held there: another notch in moves nothing. The notch picks the ground
    // again, and its depth is a difference of coordinates some 100 m from the
    // origin (a unit in the last place of 1.4e-14 m) over the 2.8 cm between
    // eye and ground: good to a few parts in 1e13, so the factor that holds
    // the eye at the limit is 1 to that, and the eye moves by less than a
    // femtometre.
    const Vec3 eye = camera.eye();
    const auto held = zoomAtPixel(layers, camera, 1.0, px, py);
    EXPECT_TRUE(held.limited);
    EXPECT_NEAR(held.factor, 1.0, 1e-11);
    EXPECT_NEAR((camera.eye() - eye).length(), 0.0, 1e-12);
    // What the view draws through it keeps the ground in front of the near plane.
    Camera drawn = camera;
    ASSERT_TRUE(drawn.fitDepthRange(katana::cad::sceneDepthBox(layers)));
    ASSERT_DOUBLE_EQ(drawn.nearPlane(), drawn.farPlane() * Camera::kNearFarFloor)
        << "the box is all in front of the eye, so this proves nothing";
    EXPECT_LT(99.0 * drawn.nearPlane(), depth);
    // And a notch out undoes a notch in.
    (void)zoomAtPixel(layers, camera, -1.0, px, py);
    const auto out = planeUnder(camera, px, py);
    ASSERT_TRUE(out.has_value());
    EXPECT_NEAR(depthOf(camera, *out) / depth, kPerNotch, 1e-9);
}

TEST(SceneZoom, AnEnormousNotchCountStopsAtTheMinimumApproachInOneStep)
{
    // 1.15^-1e6 underflows to zero, which the limit catches; 1.15^1e6 out
    // overflows, which is refused rather than sending the eye to infinity.
    const SceneLayers layers = planeScene();
    Camera camera = planeCamera();
    const auto in = zoomAtPixel(layers, camera, 1.0e6, 700.0, 150.0);
    EXPECT_TRUE(in.limited);
    const auto under = planeUnder(camera, 700.0, 150.0);
    ASSERT_TRUE(under.has_value());
    EXPECT_NEAR(depthOf(camera, *under), minimumApproach(layers), 1e-9);
    const Vec3 eye = camera.eye();
    const auto out = zoomAtPixel(layers, camera, -1.0e6, 700.0, 150.0);
    EXPECT_EQ(out.factor, 1.0);
    EXPECT_TRUE(std::isfinite(camera.distance()));
    EXPECT_NEAR((camera.eye() - eye).length(), 0.0, 1e-9);
}

TEST(SceneZoom, AnOrthographicZoomMagnifiesByTheFactorAndStopsWhereAPerspectiveOneWould)
{
    // Every point of an orthographic pixel's ray stays under it, so there is
    // no depth to pick and nothing ever stalled: a notch divides the height
    // by 1.15. It stops at the height a perspective view shows at the minimum
    // approach, 2 x 0.028284 x tan(22.5 deg) = 0.023431 m, so P keeps the
    // limit where it was.
    const SceneLayers layers = planeScene();
    Camera camera = planeCamera(Projection::Orthographic);
    double height = camera.orthographicHeight();
    for (int notch = 1; notch <= 20; ++notch) {
        const auto result = zoomAtPixel(layers, camera, 1.0, 700.0, 150.0);
        EXPECT_FALSE(result.anchor.has_value());
        ASSERT_NEAR(height / camera.orthographicHeight(), kPerNotch, 1e-12) << notch;
        height = camera.orthographicHeight();
    }
    for (int notch = 0; notch < 100; ++notch) {
        (void)zoomAtPixel(layers, camera, 1.0, 700.0, 150.0);
    }
    const double least = 2.0 * minimumApproach(layers) * std::tan(0.5 * camera.fieldOfView());
    EXPECT_NEAR(least, 2.0 * 0.028284271247461901 * 0.41421356237309503, 1e-12);
    EXPECT_NEAR(camera.orthographicHeight(), least, 1e-12);
}

TEST(SceneZoom, WithNothingUnderTheCursorANotchZoomsAboutTheTargetsPlaneAsItAlwaysDid)
{
    // Nearly level, the top of the view is sky: nothing to anchor on, so the
    // zoom is Camera::dollyAtPixel's, to the bit.
    const SceneLayers layers = planeScene();
    Camera camera = planeCamera();
    camera.setOrientation(camera.azimuth(), 0.05);
    ASSERT_FALSE(pickDrawnPoint(layers, camera, 600.0, 20.0).has_value())
        << "the pixel is not sky, so this proves nothing";
    Camera expected = camera;
    expected.dollyAtPixel(1.0 / kPerNotch, 600.0, 20.0);
    const auto result = zoomAtPixel(layers, camera, 1.0, 600.0, 20.0);
    EXPECT_FALSE(result.anchor.has_value());
    EXPECT_EQ(result.factor, 1.0 / kPerNotch);
    EXPECT_EQ(camera.target().x, expected.target().x);
    EXPECT_EQ(camera.target().y, expected.target().y);
    EXPECT_EQ(camera.target().z, expected.target().z);
    EXPECT_EQ(camera.distance(), expected.distance());
}

TEST(SceneZoom, NoNotchesNoNumberNoPixelOrNoViewportChangeNothing)
{
    const SceneLayers layers = planeScene();
    const Camera before = planeCamera();
    const auto unchanged = [&before](const Camera& camera) {
        return camera.target().x == before.target().x && camera.target().y == before.target().y &&
               camera.target().z == before.target().z && camera.distance() == before.distance() &&
               camera.orthographicHeight() == before.orthographicHeight();
    };
    for (const double notches : {0.0, std::nan(""), HUGE_VAL}) {
        Camera camera = before;
        const auto result = zoomAtPixel(layers, camera, notches, 700.0, 150.0);
        EXPECT_EQ(result.factor, 1.0);
        EXPECT_TRUE(unchanged(camera)) << notches;
    }
    Camera camera = before;
    (void)zoomAtPixel(layers, camera, 1.0, std::nan(""), 150.0);
    EXPECT_TRUE(unchanged(camera));
    Camera unsized = before;
    unsized.setViewportSize(0, 0);
    (void)zoomAtPixel(layers, unsized, 1.0, 700.0, 150.0);
    EXPECT_EQ(unsized.distance(), before.distance());
}

TEST(SceneZoom, AnEmptySceneZoomsAboutTheTargetsPlaneDownToAFloorOfTwoTenthsOfAMillimetre)
{
    // No box: the floor is 1e-4 of a 2 m patch, what Camera::frame makes of
    // one point.
    const SceneLayers empty;
    EXPECT_DOUBLE_EQ(minimumApproach(empty), 2.0e-4);
    Camera camera = planeCamera();
    for (int notch = 0; notch < 200; ++notch) {
        (void)zoomAtPixel(empty, camera, 1.0, 600.0, 400.0);
    }
    EXPECT_NEAR(camera.distance(), 2.0e-4, 1e-15);
}
