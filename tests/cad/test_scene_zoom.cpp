// Zooming a 3D view towards what it draws (cad/scene_zoom.hpp; docs/render.md,
// "Zooming towards the cursor").
//
// The pick's expected values are worked by hand from scenes chosen so that
// the arithmetic is short: a camera looking due north from (0, -300, 5) at a
// block and the ground, where every depth is a difference of northings. The
// zoom's are properties of the zoom's definition - a notch magnifies what is
// under the cursor by 1.15, so its depth divides by 1.15 - measured against
// the plane the cursor's ray is met with here, never against what the zoom
// says it picked. The limits are the two a view cannot draw past: the near
// plane's floor, tol::kGeometric / Camera::kNearPivotFloor, and the bound of
// the double arithmetic, 16 x 2^-53 of the point's distance from the origin
// (Higham's bound for dot products; scene_zoom.hpp works the count).

#include <gtest/gtest.h>

#include <algorithm>
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

// Where the ray through pixel (x, y) meets the plane z = height: by hand, not
// by the pick.
std::optional<Vec3> planeUnderAt(const Camera& camera, double x, double y, double height)
{
    const katana::math::Ray ray = camera.rayThroughPixel(x, y);
    if (ray.direction.z == 0.0) {
        return std::nullopt;
    }
    const double t = (height - ray.origin.z) / ray.direction.z;
    return t > 0.0 ? std::optional<Vec3>(ray.at(t)) : std::nullopt;
}

std::optional<Vec3> planeUnder(const Camera& camera, double x, double y)
{
    return planeUnderAt(camera, x, y, kPlane);
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

TEST(SceneZoom, ZoomingInStopsATenthOfAMillimetreShortAndANotchOutStillShrinksByTheFactor)
{
    // The near plane stays a thousandth of the pivot's distance in front of
    // the eye (Camera::kNearPivotFloor) and never nearer than
    // tol::kGeometric, 1e-7: so the pivot, what the zoom went towards, comes
    // no nearer than 1e-7 / 1e-3 = 1e-4. The double arithmetic's own bound
    // is far below that here: 16 x 2^-53 x 110 m (the point's distance from
    // the origin) over a pixel's angle, 2 tan(22.5 deg) / 800, and a tenth of
    // a pixel is 2e-9 m. The zoom stops there, the point still under the
    // cursor, the near plane a thousand times nearer than it.
    const SceneLayers layers = planeScene();
    Camera camera = planeCamera();
    const double px = 700.0;
    const double py = 150.0;
    const auto start = planeUnder(camera, px, py);
    ASSERT_TRUE(start.has_value());
    EXPECT_DOUBLE_EQ(minimumApproach(camera, *start), 1e-4);
    // From about 150 m, 1.15^n = 1.5e6 at n = 102.
    bool limited = false;
    for (int notch = 0; notch < 130; ++notch) {
        limited = zoomAtPixel(layers, camera, 1.0, px, py).limited || limited;
    }
    EXPECT_TRUE(limited);
    const auto under = planeUnder(camera, px, py);
    ASSERT_TRUE(under.has_value());
    const double depth = depthOf(camera, *under);
    EXPECT_NEAR(depth, 1e-4, 1e-9 * 1e-4);
    EXPECT_NEAR((*under - *start).length(), 0.0, 1e-6);
    // Held there: another notch in moves nothing. The notch picks the ground
    // again, and its depth is a difference of coordinates some 100 m from the
    // origin (a unit in the last place of 1.4e-14 m) over the 0.1 mm between
    // eye and ground: good to a few parts in 1e10, so the factor that holds
    // the eye at the limit is 1 to that, and the eye moves by less than a
    // unit in the last place of its coordinates.
    const Vec3 eye = camera.eye();
    const auto held = zoomAtPixel(layers, camera, 1.0, px, py);
    EXPECT_TRUE(held.limited);
    EXPECT_NEAR(held.factor, 1.0, 1e-8);
    EXPECT_NEAR((camera.eye() - eye).length(), 0.0, 1e-12);
    // What the view draws through it keeps the ground well in front of the
    // near plane: the box reaches behind the eye, so the floor decides, and
    // the pivot's share of it, 1e-4 x 1e-3, is far below the far plane's.
    Camera drawn = camera;
    ASSERT_TRUE(drawn.fitDepthRange(katana::cad::sceneDepthBox(layers)));
    EXPECT_NEAR(drawn.nearPlane(), 1e-7, 1e-15);
    EXPECT_GT(depth / drawn.nearPlane(), 999.0);
    // And a notch out undoes a notch in.
    (void)zoomAtPixel(layers, camera, -1.0, px, py);
    const auto out = planeUnder(camera, px, py);
    ASSERT_TRUE(out.has_value());
    EXPECT_NEAR(depthOf(camera, *out) / depth, kPerNotch, 1e-8);
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
    EXPECT_NEAR(depthOf(camera, *under), 1e-4, 1e-12);
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
    // approach, 2 x 1e-4 x tan(22.5 deg) = 8.2843e-5 m, so P keeps the limit
    // where it was.
    const SceneLayers layers = planeScene();
    Camera camera = planeCamera(Projection::Orthographic);
    double height = camera.orthographicHeight();
    for (int notch = 1; notch <= 20; ++notch) {
        const auto result = zoomAtPixel(layers, camera, 1.0, 700.0, 150.0);
        EXPECT_FALSE(result.anchor.has_value());
        ASSERT_NEAR(height / camera.orthographicHeight(), kPerNotch, 1e-12) << notch;
        height = camera.orthographicHeight();
    }
    // From 100 m / 1.15^20, 1.15^n = 7.4e4 at n = 80.
    for (int notch = 0; notch < 100; ++notch) {
        (void)zoomAtPixel(layers, camera, 1.0, 700.0, 150.0);
    }
    EXPECT_NEAR(camera.orthographicHeight(), 2.0 * 1e-4 * 0.41421356237309503, 1e-15);
}

TEST(SceneZoom, AnOrthographicViewOfAKilometresWideSceneZoomsOnPastWhereTheScenesSizeStoppedIt)
{
    // The elevation view of a 12 km ground framed 8480 m tall: 120 notches in
    // divide that by 1.15^120 = 1.9e7, to 0.44 mm, and nothing about the
    // scene's size stops it. A limit of a fraction of the scene's box - 1e-4
    // of its 17 km diagonal, 1.7 m of view at a 45 degree field - held it at
    // 1.87 m, and the elevation view had never stalled.
    SceneLayers layers;
    addRectangle(layers.terrain, 0.0, 0.0, 12000.0, 12000.0, 90.0);
    layers.datum = 90.0;
    layers.bounds = layers.terrain.bounds();
    Camera camera;
    camera.setViewportSize(1200, 800);
    camera.setProjection(Projection::Orthographic);
    camera.setStandardView(StandardView::Front);
    camera.setTarget(Vec3(6000.0, 6000.0, 90.0));
    camera.setDistance(16000.0);
    camera.setOrthographicHeight(8480.0);
    for (int notch = 0; notch < 120; ++notch) {
        ASSERT_FALSE(zoomAtPixel(layers, camera, 1.0, 600.0, 400.0).limited) << notch;
    }
    const double expected = 8480.0 / std::pow(kPerNotch, 120.0);
    EXPECT_NEAR(camera.orthographicHeight(), expected, 1e-9 * expected);
}

namespace {

// A model in the middle of an empty view, as a docked view opens: ground
// 40 m square at z = -10, the grid's square 52 m across on the datum round
// it, seen from the south-west 150 m from (0, 0, -10). The eye is at
// (-86.6, -86.6, 76.6); the box reaches 180 m deep, to the grid's far
// corner (26, 26, -10): 2 x 112.6 x 0.5774 + 86.6 x 0.5774. The bottom-left
// corner of the view meets the datum at (-89.0, -6.3, -10), off the grid and
// 95.0 m deep; the top of the view meets it at (178, 178), 355 m deep,
// beyond what the scene reaches.
SceneLayers islandScene()
{
    SceneLayers layers;
    addRectangle(layers.terrain, -20.0, -20.0, 20.0, 20.0, kPlane);
    const Vec3 corners[4] = {{-26.0, -26.0, kPlane}, {26.0, -26.0, kPlane}, {26.0, 26.0, kPlane},
                             {-26.0, 26.0, kPlane}};
    for (int k = 0; k < 4; ++k) {
        layers.grid.addSegment(corners[k], corners[(k + 1) % 4], kInk);
    }
    layers.datum = kPlane;
    layers.bounds = layers.terrain.bounds();
    return layers;
}

Camera islandCamera()
{
    Camera camera;
    camera.setViewportSize(1200, 800);
    camera.setStandardView(StandardView::IsoSouthWest);
    camera.setTarget(Vec3(0.0, 0.0, kPlane));
    camera.setDistance(150.0);
    return camera;
}

} // namespace

TEST(SceneZoom, OffTheModelANotchZoomsTowardsTheDatumAndTheEyeNeverGoesUnderIt)
{
    // Nothing is drawn under the bottom-left corner, and the datum there is
    // outside the grid. The target's plane, 150 m deep, lies beyond the
    // datum's point, 95.0 m deep, so the old anchor was under the ground and
    // the eye went through it at -ln(1 - 95.0 / 150) / ln 1.15 = 7.2 notches:
    // the whole view blank from then on. Anchored on the datum, its point
    // under the cursor divides its depth by 1.15 a notch and the eye stays
    // above it.
    const SceneLayers layers = islandScene();
    Camera camera = islandCamera();
    const double px = 5.0;
    const double py = 795.0;
    ASSERT_FALSE(pickDrawnPoint(layers, camera, px, py).has_value())
        << "something is drawn under the cursor, so this proves nothing";
    const auto start = planeUnder(camera, px, py);
    ASSERT_TRUE(start.has_value());
    const double first = depthOf(camera, *start);
    ASSERT_LT(first, 0.7 * camera.distance()) << "the datum is not nearer than the target's plane";
    double depth = first;
    for (int notch = 1; notch <= 60; ++notch) {
        const auto result = zoomAtPixel(layers, camera, 1.0, px, py);
        ASSERT_TRUE(result.anchor.has_value()) << notch;
        EXPECT_EQ(result.anchor->source, ScenePick::Source::Datum) << notch;
        ASSERT_GT(camera.eye().z, kPlane) << "the eye is under the datum at notch " << notch;
        const auto under = planeUnder(camera, px, py);
        ASSERT_TRUE(under.has_value()) << notch;
        const double now = depthOf(camera, *under);
        ASSERT_NEAR(depth / now, kPerNotch, 1e-9) << "notch " << notch;
        depth = now;
        EXPECT_NEAR((*under - *start).length(), 0.0, 1e-9 * first) << "notch " << notch;
    }
}

TEST(SceneZoom, BeyondTheScenesReachANotchZoomsAsDeepAsItReachesSoNothingDrawnStalls)
{
    // The top of the view meets the datum 355 m deep, twice as deep as the
    // scene reaches (180 m). The target's plane, 150 m deep, left the grid's
    // far corner beyond the anchor, so its magnification tended to
    // 180 / (180 - 150) = 6 and stopped; anchored on the datum's point, a
    // notch would cover 13% of 355 m and put the model behind the eye in
    // four. Anchored as deep as the scene reaches, the far corner divides
    // its depth by exactly 1.15 a notch, and whatever is drawn, all of it
    // nearer, divides its own by at least that until it leaves the view.
    const SceneLayers layers = islandScene();
    Camera camera = islandCamera();
    const double px = 600.0;
    const double py = 5.0;
    const auto datum = planeUnder(camera, px, py);
    ASSERT_TRUE(datum.has_value());
    const Vec3 far(26.0, 26.0, kPlane);
    const Vec3 ground(20.0, 20.0, kPlane); // the model's far corner
    ASSERT_GT(depthOf(camera, *datum), 1.5 * depthOf(camera, far))
        << "the datum is within the scene's reach, so this proves nothing";
    double reach = depthOf(camera, far);
    EXPECT_NEAR(reach, 2.0 * 112.6 * 0.5774 + 86.6 * 0.5774, 0.1);
    double model = depthOf(camera, ground);
    for (int notch = 1; notch <= 40; ++notch) {
        const auto result = zoomAtPixel(layers, camera, 1.0, px, py);
        ASSERT_TRUE(result.anchor.has_value()) << notch;
        EXPECT_EQ(result.anchor->source, ScenePick::Source::Reach) << notch;
        ASSERT_GT(camera.eye().z, kPlane) << notch;
        const double now = depthOf(camera, far);
        ASSERT_NEAR(reach / now, kPerNotch, 1e-9) << "notch " << notch;
        reach = now;
        const double modelNow = depthOf(camera, ground);
        if (modelNow > 0.0) {
            EXPECT_GE(model / modelNow, kPerNotch - 1e-9) << "notch " << notch;
        }
        model = modelNow;
    }
}

TEST(SceneZoom, ALevelRayOutOfTheSceneTakesTheDepthTheSceneReaches)
{
    // Nearly level, the top of the view is sky: the ray never comes down to
    // the datum, so the zoom goes as deep as the scene reaches, its box's
    // furthest corner along the view.
    const SceneLayers layers = planeScene();
    Camera camera = planeCamera();
    camera.setOrientation(camera.azimuth(), 0.05);
    ASSERT_FALSE(pickDrawnPoint(layers, camera, 600.0, 20.0).has_value())
        << "the pixel is not sky, so this proves nothing";
    ASSERT_GT(camera.rayThroughPixel(600.0, 20.0).direction.z, 0.0) << "not a rising ray";
    double reach = 0.0;
    for (const double x : {-100.0, 100.0}) {
        for (const double y : {-100.0, 100.0}) {
            reach = std::max(reach, depthOf(camera, Vec3(x, y, kPlane)));
        }
    }
    const auto anchor = katana::cad::zoomAnchor(layers, camera, 600.0, 20.0);
    ASSERT_TRUE(anchor.has_value());
    EXPECT_EQ(anchor->source, ScenePick::Source::Reach);
    EXPECT_NEAR(anchor->depth, reach, 1e-9 * reach);
    EXPECT_NEAR(depthOf(camera, anchor->point), reach, 1e-9 * reach);
}

TEST(SceneZoom, WithTheWholeSceneBehindTheEyeANotchZoomsAboutTheTargetsPlaneAsItAlwaysDid)
{
    // Turned round to face away from the scene there is nothing in front to
    // go towards: the zoom is Camera::dollyAtPixel's, to the bit.
    const SceneLayers layers = planeScene();
    Camera camera = planeCamera();
    camera.setTarget(Vec3(-300.0, -300.0, 50.0));
    camera.setOrientation(camera.azimuth() + 3.141592653589793, 0.2);
    ASSERT_FALSE(katana::cad::zoomAnchor(layers, camera, 600.0, 400.0).has_value())
        << "something is in front of the eye, so this proves nothing";
    Camera expected = camera;
    expected.dollyAtPixel(1.0 / kPerNotch, 600.0, 400.0);
    const auto result = zoomAtPixel(layers, camera, 1.0, 600.0, 400.0);
    EXPECT_FALSE(result.anchor.has_value());
    EXPECT_EQ(result.factor, 1.0 / kPerNotch);
    EXPECT_EQ(camera.target().x, expected.target().x);
    EXPECT_EQ(camera.target().y, expected.target().y);
    EXPECT_EQ(camera.target().z, expected.target().z);
    EXPECT_EQ(camera.distance(), expected.distance());
}

TEST(SceneZoom, OneStrayEntityAtTheOriginNoLongerStopsTheZoomHundredsOfMetresFromASurvey)
{
    // A survey at MGA coordinates and one point left at (0, 0), with the
    // grid SceneBuilder stands round them, reaching 15% of the 6200 km span
    // past them: the scene's box is 8345 km across. A limit of 1e-4 of that
    // held the eye 830 m from the ground, and the near plane - a millionth of
    // the far one, the grid's far corner - was a metre out. What draws the view is good
    // far closer: the point zoomed towards is 6.2e6 m from the origin, where
    // 16 x 2^-53 of that over a tenth of an 800 px view's pixel angle,
    // 2 tan(22.5 deg) / 800, is 0.106 mm.
    SceneLayers layers;
    addRectangle(layers.terrain, 300000.0, 6200000.0, 300200.0, 6200200.0, 30.0);
    layers.entities.addPoint(layers.entities.addVertex(Vec3(0.0, 0.0, 30.0), kInk), 5.0f, 1.5f);
    layers.datum = 30.0;
    layers.bounds = layers.terrain.bounds();
    layers.bounds.expand(layers.entities.bounds());
    katana::cad::SceneBuilder builder;
    builder.buildGrid(katana::cad::SceneOptions{}, layers);
    ASSERT_FALSE(layers.grid.empty());
    Camera camera;
    camera.setViewportSize(1200, 800);
    camera.setStandardView(StandardView::IsoSouthWest);
    camera.setTarget(Vec3(300100.0, 6200100.0, 30.0));
    camera.setDistance(400.0);
    const auto start = planeUnderAt(camera, 600.0, 300.0, 30.0);
    ASSERT_TRUE(start.has_value());
    ASSERT_TRUE(start->x > 300000.0 && start->x < 300200.0 && start->y > 6200000.0 &&
                start->y < 6200200.0)
        << "the cursor is not on the survey";
    const double expected = 16.0 * 0x1.0p-53 * start->length() /
                            (0.1 * 2.0 * 0.41421356237309503 / 800.0);
    EXPECT_NEAR(expected, 1.06e-4, 0.01e-4);
    EXPECT_NEAR(minimumApproach(camera, *start), expected, 1e-9 * expected);
    // From 468 m to 0.106 mm, 1.15^n = 4.4e6 at n = 110.
    for (int notch = 0; notch < 140; ++notch) {
        (void)zoomAtPixel(layers, camera, 1.0, 600.0, 300.0);
    }
    const auto under = planeUnderAt(camera, 600.0, 300.0, 30.0);
    ASSERT_TRUE(under.has_value());
    // A depth of 0.1 mm taken as a difference of coordinates 6.2e6 m from
    // the origin, whose unit in the last place is 9.3e-10 m: 1e-5 of it.
    const double depth = depthOf(camera, *under);
    EXPECT_NEAR(depth, expected, 1e-4 * expected);
    EXPECT_NEAR((*under - *start).length(), 0.0, 1e-6);
    // And the view draws it: the near plane stays a thousand times nearer,
    // where a millionth of the far plane alone would lie beyond the ground.
    Camera drawn = camera;
    ASSERT_TRUE(drawn.fitDepthRange(katana::cad::sceneDepthBox(layers)));
    EXPECT_GT(depth / drawn.nearPlane(), 990.0);
    EXPECT_GT(drawn.farPlane() * Camera::kNearFarFloor, 1000.0 * depth);
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

TEST(SceneZoom, AnEmptySceneZoomsAboutTheTargetsPlaneDownToATenthOfAMillimetre)
{
    // Nothing to go towards: the target's plane, as before, down to the
    // near plane's limit, 1e-7 / 1e-3 (the target at the origin makes the
    // double arithmetic's bound nothing).
    const SceneLayers empty;
    Camera camera = planeCamera();
    for (int notch = 0; notch < 200; ++notch) {
        const auto result = zoomAtPixel(empty, camera, 1.0, 600.0, 400.0);
        ASSERT_FALSE(result.anchor.has_value());
    }
    EXPECT_NEAR(camera.distance(), 1.0e-4, 1e-15);
}

TEST(SceneZoom, TheMinimumApproachIsTheLargerOfTheNearPlanesLimitAndTheArithmeticsBound)
{
    // tol::kGeometric / Camera::kNearPivotFloor for a point near the origin;
    // 16 x 2^-53 |p| over a tenth of the pixel's angle for one far from it,
    // in proportion to its distance and to the view's height in pixels.
    Camera camera = planeCamera();
    EXPECT_DOUBLE_EQ(minimumApproach(camera, Vec3(0.0, 0.0, 0.0)), 1e-4);
    EXPECT_DOUBLE_EQ(minimumApproach(camera, Vec3(3000.0, 4000.0, 0.0)), 1e-4);
    const double angle = 2.0 * 0.41421356237309503 / 800.0;
    const Vec3 far(0.0, 6.0e9, 0.0); // an MGA northing in millimetres
    EXPECT_NEAR(minimumApproach(camera, far), 16.0 * 0x1.0p-53 * 6.0e9 / (0.1 * angle),
                1e-9 * 0.1);
    camera.setViewportSize(1200, 1600);
    EXPECT_NEAR(minimumApproach(camera, far), 2.0 * 16.0 * 0x1.0p-53 * 6.0e9 / (0.1 * angle),
                1e-9 * 0.2);
    // A point that is not finite has no bound of its own.
    EXPECT_DOUBLE_EQ(minimumApproach(camera, Vec3(std::nan(""), 0.0, 0.0)), 1e-4);
}
