#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <numbers>
#include <string>

#include "katana/archive12d/domain.hpp"
#include "katana/archive12d/reader.hpp"
#include "katana/archive12d/writer.hpp"
#include "katana/entity/model.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace a12 = katana::archive12d;
using katana::entity::Entity;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

namespace {

constexpr double kPi = std::numbers::pi;

Entity make(katana::entity::Geometry geometry, const std::string& layer = "0")
{
    Entity entity;
    entity.geometry = std::move(geometry);
    entity.layer = layer;
    return entity;
}

katana::entity::EntityId add(katana::entity::Model& model, Entity entity)
{
    auto id = model.entities.add(std::move(entity));
    EXPECT_TRUE(id.ok()) << (id.ok() ? "" : id.error().describe());
    return id.ok() ? *id : katana::entity::kInvalidEntityId;
}

katana::entity::EntityId add(katana::entity::Model& model, katana::entity::Geometry geometry,
                             const std::string& layer = "0")
{
    return add(model, make(std::move(geometry), layer));
}

// Model -> archive -> 12da text -> archive -> domain: everything a user would
// put a drawing through to get it into 12d and back.
a12::DomainImport roundTrip(const katana::entity::Model& model,
                            const std::vector<a12::ExportSurface>& surfaces = {},
                            a12::DomainExport* exported = nullptr)
{
    auto out = a12::fromDomain(model, surfaces);
    EXPECT_TRUE(out.ok()) << (out.ok() ? "" : out.error().describe());
    if (!out.ok()) {
        return {};
    }
    const std::string text = a12::writeArchive(out->archive);
    auto archive = a12::readArchive(text);
    EXPECT_TRUE(archive.ok()) << (archive.ok() ? "" : archive.error().describe()) << "\n" << text;
    if (exported != nullptr) {
        *exported = std::move(*out);
    }
    auto back = a12::toDomain(archive.ok() ? *archive : a12::Archive{});
    EXPECT_TRUE(back.ok());
    return back.ok() ? std::move(*back) : a12::DomainImport{};
}

} // namespace

TEST(DomainExport, EveryDrawableEntityComesBackAsTheGeometryItWas)
{
    katana::entity::Model model;
    ASSERT_TRUE(model.layers.add(katana::entity::Layer{.name = "survey/kerb"}).ok());
    add(model, katana::entity::PointGeometry{Point2(1.5, 2.5)}, "survey/kerb");
    add(model, katana::geometry::Segment2{Point2(0.0, 0.0), Point2(3.0, 4.0)});
    add(model, Polyline2{{Point2(0.0, 0.0), Point2(10.0, 0.0), Point2(10.0, 5.0)}, true});
    add(model, katana::geometry::Circle2{Point2(650.0, 200.0), 8.0});
    add(model, katana::entity::TextGeometry{Point2(5.0, 6.0), "RL \"32.45\"", 2.75, kPi / 2.0});

    a12::DomainExport exported;
    const auto back = roundTrip(model, {}, &exported);
    EXPECT_EQ(exported.entitiesWritten, 5u);
    EXPECT_EQ(exported.entitiesSkipped, 0u);
    ASSERT_EQ(back.entities.size(), 5u);
    EXPECT_EQ(back.entities[0].layer, "survey/kerb") << "a layer path is a 12d tree name";
    EXPECT_EQ(std::get<katana::entity::PointGeometry>(back.entities[0].geometry).position,
              Point2(1.5, 2.5));
    EXPECT_EQ(std::get<katana::geometry::Segment2>(back.entities[1].geometry).end,
              Point2(3.0, 4.0));
    const auto& polygon = std::get<Polyline2>(back.entities[2].geometry);
    EXPECT_TRUE(polygon.closed);
    EXPECT_EQ(polygon.vertices.size(), 3u);
    EXPECT_EQ(std::get<katana::geometry::Circle2>(back.entities[3].geometry).radius, 8.0);
    const auto& text = std::get<katana::entity::TextGeometry>(back.entities[4].geometry);
    EXPECT_EQ(text.text, "RL \"32.45\"");
    EXPECT_EQ(text.height, 2.75);
    EXPECT_NEAR(text.rotation, kPi / 2.0, 1e-12);
}

TEST(DomainExport, AnArcKeepsItsHandAndWhichOfTheTwoArcsItIs)
{
    // Quarter turns about the origin from (10,0): counter-clockwise to (0,10),
    // clockwise to (0,-10); and the three-quarter turn the long way to (0,10).
    for (const double sweep : {kPi / 2.0, -kPi / 2.0, -1.5 * kPi}) {
        katana::entity::Model model;
        add(model, katana::geometry::Arc2{Point2(0.0, 0.0), 10.0, 0.0, sweep});
        a12::DomainExport exported;
        const auto back = roundTrip(model, {}, &exported);

        const auto& written = std::get<a12::VertexString>(exported.archive.elements.at(0));
        ASSERT_EQ(written.segments.size(), 1u);
        // 12d's positive radius turns right: counter-clockwise is negative.
        EXPECT_EQ(written.segments[0].radius, sweep > 0.0 ? -10.0 : 10.0);
        EXPECT_EQ(written.segments[0].major, std::fabs(sweep) > kPi);

        // Back in Katana it is chords, every one of them on the original circle
        // and running the original way round.
        const auto& chords = std::get<Polyline2>(back.entities.at(0).geometry);
        ASSERT_GT(chords.vertices.size(), 4u);
        double turned = 0.0;
        for (std::size_t i = 0; i < chords.vertices.size(); ++i) {
            EXPECT_NEAR(chords.vertices[i].distanceTo(Point2(0.0, 0.0)), 10.0, 1e-7);
            if (i > 0) {
                const Point2& p = chords.vertices[i - 1];
                const Point2& q = chords.vertices[i];
                turned += std::atan2(p.x * q.y - p.y * q.x, p.x * q.x + p.y * q.y);
            }
        }
        EXPECT_NEAR(turned, sweep, 1e-6);
    }
}

TEST(DomainExport, HeightsPropertiesAndTwelveDsOwnFieldsSurviveTheTrip)
{
    katana::entity::Model model;
    Entity string = make(Polyline2{{Point2(0.0, 0.0), Point2(5.0, 0.0), Point2(9.0, 3.0)}, false});
    string.properties["elevations"] = std::string("1.5 null 2.25");
    string.properties["Owner"] = std::string("Telstra");
    string.properties["Ways"] = std::int64_t{4};
    string.properties["Asset/Dim/Cover"] = 0.9;
    string.properties["Live"] = true;
    string.metadata["12d.name"] = std::string("Conduit 4-way");
    string.style = "Comms-Conduit";
    string.metadata["12d.colour"] = std::string("pen 025");
    string.metadata["12d.chainage"] = 12.5;
    // A conduit is a LINE. Saying `point` here would be saying its three
    // vertices are three separate points, which is what that flag means and
    // what the import now does with it.
    string.metadata["12d.breakline"] = std::string("line");
    string.metadata["12d.point_ids"] = std::string("101,A 7,103");
    add(model, string);
    Entity point = make(katana::entity::PointGeometry{Point2(1.0, 1.0)});
    point.properties["elevation"] = 31.25;
    add(model, point);

    a12::DomainExport exported;
    const auto back = roundTrip(model, {}, &exported);

    const auto& written = std::get<a12::VertexString>(exported.archive.elements.at(0));
    EXPECT_EQ(written.header.name, "Conduit 4-way");
    EXPECT_EQ(written.vertices[1].z, std::nullopt);
    EXPECT_EQ(written.vertices[2].z, 2.25);
    EXPECT_EQ(written.pointIds, (std::vector<std::string>{"101", "A 7", "103"}));
    // The group the path names is rebuilt, not written as a name with slashes.
    const auto group = std::find_if(written.header.attributes.begin(), written.header.attributes.end(),
                                    [](const a12::Attribute& a) { return a.name == "Asset"; });
    ASSERT_NE(group, written.header.attributes.end());
    EXPECT_TRUE(std::holds_alternative<a12::AttributeList>(group->value));

    ASSERT_EQ(back.entities.size(), 2u);
    const Entity& again = back.entities[0];
    EXPECT_EQ(std::get<std::string>(again.properties.at("elevations")), "1.5 null 2.25");
    EXPECT_EQ(std::get<std::string>(again.properties.at("Owner")), "Telstra");
    EXPECT_EQ(std::get<std::int64_t>(again.properties.at("Ways")), 4);
    EXPECT_EQ(std::get<double>(again.properties.at("Asset/Dim/Cover")), 0.9);
    EXPECT_EQ(std::get<std::int64_t>(again.properties.at("Live")), 1) << "12d has no boolean attribute";
    for (const char* key : {"12d.name", "12d.colour", "12d.chainage", "12d.breakline",
                            "12d.point_ids"}) {
        EXPECT_EQ(again.metadata.at(key), string.metadata.at(key)) << key;
    }
    EXPECT_EQ(again.style, "Comms-Conduit") << "the style went out as the 12d linestyle and came back";
    EXPECT_EQ(written.header.style, "Comms-Conduit");
    EXPECT_EQ(std::get<double>(back.entities[1].properties.at("elevation")), 31.25);
}

TEST(DomainExport, AnElevationListThatNoLongerFitsTheGeometryIsNotWrittenAsIfItDid)
{
    katana::entity::Model model;
    Entity string = make(Polyline2{{Point2(0.0, 0.0), Point2(5.0, 0.0), Point2(9.0, 3.0)}, false});
    string.properties["elevations"] = std::string("1 2"); // a vertex was added since
    add(model, string);
    const auto out = a12::fromDomain(model, {});
    ASSERT_TRUE(out.ok());
    for (const a12::Vertex& vertex : std::get<a12::VertexString>(out->archive.elements.at(0)).vertices) {
        EXPECT_FALSE(vertex.z.has_value());
    }
}

TEST(DomainExport, TheColourWrittenIsTheNameItCameWithUnlessTheColourHasChanged)
{
    katana::entity::Model model;
    Entity kept = make(katana::entity::PointGeometry{Point2(0.0, 0.0)});
    kept.color = katana::entity::Color{255, 255, 0, 255};
    kept.metadata["12d.colour"] = std::string("Yellow");
    add(model, kept);
    Entity recoloured = make(katana::entity::PointGeometry{Point2(1.0, 0.0)});
    recoloured.color = katana::entity::Color{0, 0, 250, 255};
    recoloured.metadata["12d.colour"] = std::string("yellow");
    add(model, recoloured);
    add(model, katana::entity::PointGeometry{Point2(2.0, 0.0)}); // ByLayer; layer "0" is white

    const auto out = a12::fromDomain(model, {});
    ASSERT_TRUE(out.ok());
    const auto colourOf = [&](std::size_t index) {
        return std::get<a12::VertexString>(out->archive.elements.at(index)).header.colour;
    };
    EXPECT_EQ(colourOf(0), "Yellow") << "its own spelling, since it still means the same colour";
    EXPECT_EQ(colourOf(1), "blue") << "the old name would be a lie";
    EXPECT_EQ(colourOf(2), "white");
}

TEST(DomainExport, DimensionsAreCountedAsSkippedAndOnlyChosenEntitiesAreWritten)
{
    katana::entity::Model model;
    add(model, katana::entity::DimensionGeometry{Point2(0.0, 0.0), Point2(10.0, 0.0), 2.0, {}});
    const katana::entity::EntityId wanted =
        add(model, katana::entity::PointGeometry{Point2(1.0, 1.0)});
    add(model, katana::entity::PointGeometry{Point2(2.0, 2.0)});

    const auto all = a12::fromDomain(model, {});
    ASSERT_TRUE(all.ok());
    EXPECT_EQ(all->entitiesWritten, 2u);
    EXPECT_EQ(all->entitiesSkipped, 1u);
    ASSERT_EQ(all->warnings.size(), 1u);
    EXPECT_NE(all->warnings[0].find("1 dimensions"), std::string::npos);

    a12::ExportOptions options;
    options.entities = {wanted};
    const auto some = a12::fromDomain(model, {}, options);
    ASSERT_TRUE(some.ok());
    EXPECT_EQ(some->entitiesWritten, 1u);
    EXPECT_EQ(some->archive.elements.size(), 1u);
}

TEST(DomainExport, AnOriginShiftIsAddedBackSoCoordinatesReturnToWhereTheyCameFrom)
{
    katana::entity::Model model;
    add(model, katana::entity::PointGeometry{Point2(10.0, 20.0)});
    a12::ExportOptions options;
    options.originShift = katana::geometry::Vec2(502000.0, 6960000.0);
    const auto out = a12::fromDomain(model, {}, options);
    ASSERT_TRUE(out.ok());
    EXPECT_EQ(std::get<a12::VertexString>(out->archive.elements.at(0)).vertices.at(0),
              (a12::Vertex{502010.0, 6960020.0, std::nullopt}));
}

TEST(DomainExport, AnAlignmentWithSpiralsIsWrittenSolvedAndReadsBackAsTheSamePis)
{
    // This closes the loop on the trailing-transition convention: Katana
    // writes its outgoing spiral described backwards, as 12d does, and the
    // importer - which checks every recorded vertex against what it solves -
    // accepts it. With either half of the convention wrong the check fails.
    katana::entity::Model model;
    katana::entity::Alignment alignment;
    alignment.name = "MC01";
    alignment.horizontal.startStation = 1000.0;
    alignment.horizontal.pis = {{Point2(0.0, 0.0), 0.0, 0.0, 0.0},
                                {Point2(500.0, 0.0), 200.0, 60.0, 40.0},
                                {Point2(700.0, 500.0), 150.0, 0.0, 0.0},
                                {Point2(1300.0, 500.0), 0.0, 0.0, 0.0}};
    alignment.vertical.emplace();
    alignment.vertical->pvis = {{1000.0, 30.0, 0.0}, {1400.0, 42.0, 120.0}, {2000.0, 36.0, 0.0}};
    ASSERT_TRUE(model.alignments.add(alignment).ok());

    a12::DomainExport exported;
    auto out = a12::fromDomain(model, {});
    ASSERT_TRUE(out.ok());
    EXPECT_EQ(out->alignmentsWritten, 1u);
    auto& written = std::get<a12::SuperAlignment>(out->archive.elements.at(0));
    EXPECT_TRUE(written.horizontalIsIpOnly());
    ASSERT_TRUE(written.horizontalData.has_value());
    // tangent, spiral, arc, spiral, tangent, arc, tangent
    ASSERT_EQ(written.horizontalData->segments.size(), 7u);
    EXPECT_EQ(written.horizontalData->segments[1].parameters.integer("leading"), 1);
    EXPECT_EQ(written.horizontalData->segments[3].parameters.integer("leading"), 0);
    EXPECT_EQ(written.horizontalData->segments[3].parameters.real("r1"), 0.0)
        << "a trailing transition is described from its straight end";

    // Force the importer down the harder road: no IPs, only solved elements.
    written.horizontalParts.clear();
    written.verticalParts.clear();
    auto archive = a12::readArchive(a12::writeArchive(out->archive));
    ASSERT_TRUE(archive.ok()) << archive.error().describe();
    const auto back = a12::toDomain(*archive);
    ASSERT_TRUE(back.ok());
    ASSERT_EQ(back->alignments.size(), 1u) << (back->warnings.empty() ? "" : back->warnings.front());
    const auto& pis = back->alignments[0].horizontal.pis;
    ASSERT_EQ(pis.size(), 4u);
    for (std::size_t i = 0; i < 4; ++i) {
        // Coordinates cross the file at 8 decimal places.
        EXPECT_NEAR(pis[i].point.x, alignment.horizontal.pis[i].point.x, 1e-5) << i;
        EXPECT_NEAR(pis[i].point.y, alignment.horizontal.pis[i].point.y, 1e-5) << i;
        EXPECT_NEAR(pis[i].radius, alignment.horizontal.pis[i].radius, 1e-6) << i;
        EXPECT_NEAR(pis[i].spiralIn, alignment.horizontal.pis[i].spiralIn, 1e-6) << i;
        EXPECT_NEAR(pis[i].spiralOut, alignment.horizontal.pis[i].spiralOut, 1e-6) << i;
    }
    EXPECT_EQ(back->alignments[0].horizontal.startStation, 1000.0);
    ASSERT_TRUE(back->alignments[0].vertical.has_value());
    ASSERT_EQ(back->alignments[0].vertical->pvis.size(), 3u);
    EXPECT_NEAR(back->alignments[0].vertical->pvis[1].station, 1400.0, 1e-6);
    EXPECT_NEAR(back->alignments[0].vertical->pvis[1].elevation, 42.0, 1e-6);
    EXPECT_NEAR(back->alignments[0].vertical->pvis[1].curveLength, 120.0, 1e-6);
}

TEST(DomainExport, ASurfaceIsWrittenClockwiseAndReadsBackAsTheSameGround)
{
    // Two triangles over a 10 m square tilted in x: z = 5 + 0.1 x.
    auto surface = katana::terrain::TinSurface::create(
        {{0.0, 0.0, 5.0}, {10.0, 0.0, 6.0}, {10.0, 10.0, 6.0}, {0.0, 10.0, 5.0}},
        {{0, 1, 2}, {0, 2, 3}});
    ASSERT_TRUE(surface.ok());
    katana::entity::Model model;

    a12::DomainExport exported;
    const auto back = roundTrip(model, {a12::ExportSurface{"DESIGN", &*surface}}, &exported);
    EXPECT_EQ(exported.surfacesWritten, 1u);
    const auto& tin = std::get<a12::Tin>(exported.archive.elements.at(0));
    EXPECT_FALSE(tin.full);
    // (0,0) (10,10) (10,0): clockwise seen from above, as the manual requires.
    EXPECT_EQ(tin.triangles.at(0), (std::array<std::uint32_t, 3>{0, 2, 1}));

    ASSERT_EQ(back.surfaces.size(), 1u);
    EXPECT_EQ(back.surfaces[0].name, "DESIGN");
    EXPECT_EQ(back.surfaces[0].surface.triangleCount(), 2u);
    const auto z = back.surfaces[0].surface.elevationAt(2.5, 7.5);
    ASSERT_TRUE(z.has_value());
    EXPECT_NEAR(*z, 5.25, 1e-12);
    EXPECT_NEAR(back.surfaces[0].surface.planArea(), 100.0, 1e-9);
}

TEST(DomainExport, ASurfacesCoordinatesCrossTheFileWithoutLosingABit)
{
    // Thirds and sevenths at survey magnitude, which eight decimal places
    // cannot hold. Eight places moves a point by up to 5e-9 - twenty times
    // less than the tolerance at which a triangle counts as flat, so it
    // cannot turn a real triangle inside out, but it is still not ours to
    // throw away: a tin's coordinates are computed, not typed.
    const double x = 187008.0 + 1.0 / 3.0;
    const double y = 6184863.0 + 1.0 / 7.0;
    auto surface = katana::terrain::TinSurface::create(
        {{x, y, 8.0 + 2.0 / 3.0},
         {x + 10.0, y, 9.5},
         {x + 10.0, y + 10.0, 10.25},
         {x, y + 10.0, 11.0}},
        {{0, 1, 2}, {0, 2, 3}});
    ASSERT_TRUE(surface.ok()) << (surface.ok() ? "" : surface.error().describe());
    katana::entity::Model model;
    const auto back = roundTrip(model, {a12::ExportSurface{"DESIGN", &*surface}});
    ASSERT_EQ(back.surfaces.size(), 1u);

    // The importer numbers vertices in the order the triangles first use them,
    // so compare the sets rather than the lists.
    const auto sorted = [](std::vector<katana::geometry::Point3> points) {
        std::sort(points.begin(), points.end(),
                  [](const katana::geometry::Point3& a, const katana::geometry::Point3& b) {
                      if (a.x != b.x) {
                          return a.x < b.x;
                      }
                      if (a.y != b.y) {
                          return a.y < b.y;
                      }
                      return a.z < b.z;
                  });
        return points;
    };
    EXPECT_EQ(sorted(back.surfaces[0].surface.vertices()), sorted(surface->vertices()))
        << "every vertex must arrive as exactly the double it left as";
}

TEST(DomainExport, AnEmptyModelIsAnEmptyArchiveAndBadOptionsAreRefused)
{
    katana::entity::Model model;
    const auto out = a12::fromDomain(model, {});
    ASSERT_TRUE(out.ok());
    EXPECT_TRUE(out->archive.elements.empty());
    a12::ExportOptions options;
    options.curveTolerance = -1.0;
    EXPECT_FALSE(a12::fromDomain(model, {}, options).ok());
}
