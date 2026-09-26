// UTILITY DRAW <scope>: the located services as a survey or an import left
// them in the drawing - strings and points, lines and polylines - drawn as
// services (readGeometryServices, utility_data.hpp;
// docs/subsurface_utilities.md, "Services from surveyed and imported
// geometry"). The owner asked on 2026-09-26 for the utility tools to act on
// "the data on the program": services are surveyed by GNSS or total station,
// or come as a .12da, IFC, shapefile or DXF import, and are post-processed in
// Katana, not written into a schedule first.
//
// The proof the drawing is held to: a survey drawn as services is exactly
// what a schedule that says the same thing - written by hand below, row for
// row from the survey - draws, entity for entity. The schedule reader, and
// the grading it feeds, are held to hand-worked figures elsewhere
// (test_utility_verbs.cpp, the survey library's tests), so the survey is too.

#include <gtest/gtest.h>

#include <algorithm>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <map>
#include <optional>
#include <string>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/utilities/utility_data.hpp"
#include "katana/cad/utilities/utility_drawing.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/survey/subsurface/utility_csv.hpp"

namespace fs = std::filesystem;
namespace sub = katana::survey::subsurface;
using namespace katana::cad::utilities;
using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::EntityType;
using katana::entity::PropertyMap;
using katana::geometry::Point2;

namespace {

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

std::string firstLine(const std::string& text)
{
    return text.substr(0, text.find('\n'));
}

std::vector<std::string> lines(const std::string& text)
{
    std::vector<std::string> out;
    std::size_t start = 0;
    for (;;) {
        const std::size_t end = text.find('\n', start);
        out.push_back(text.substr(start, end - start));
        if (end == std::string::npos) {
            return out;
        }
        start = end + 1;
    }
}

std::string line(const std::string& text, std::size_t index)
{
    const std::vector<std::string> all = lines(text);
    return index < all.size() ? all[index] : std::string();
}

std::string withoutLine(const std::string& text, std::size_t index)
{
    std::string out;
    const std::vector<std::string> all = lines(text);
    for (std::size_t i = 0; i < all.size(); ++i) {
        if (i != index) {
            out += (out.empty() ? "" : "\n") + all[i];
        }
    }
    return out;
}

struct Session {
    Document document;
    CommandInterpreter interpreter{document};

    std::string ok(const std::string& text)
    {
        auto reply = interpreter.run(text);
        EXPECT_TRUE(reply.ok()) << text << "\n  -> "
                                << (reply.ok() ? std::string{} : reply.error().describe());
        return reply.ok() ? *reply : std::string{};
    }
    katana::core::Error refused(const std::string& text)
    {
        auto reply = interpreter.run(text);
        EXPECT_FALSE(reply.ok()) << text << " was accepted:\n" << (reply.ok() ? *reply : "");
        return reply.ok() ? katana::core::Error{} : reply.error();
    }
    std::size_t steps() const { return document.history().undoCount(); }

    EntityId add(Entity entity)
    {
        EntityId before = 0;
        document.model().entities.forEach(
            [&before](const Entity& each) { before = std::max(before, each.id); });
        std::vector<Entity> one{std::move(entity)};
        EXPECT_TRUE(document.execute(katana::commands::createEntities(std::move(one))).ok());
        EntityId added = 0;
        document.model().entities.forEach(
            [&added](const Entity& each) { added = std::max(added, each.id); });
        EXPECT_GT(added, before);
        return added;
    }

    // A string as survey linework or an import leaves one: a polyline with
    // its heights, carrying `properties` (its code among them).
    EntityId string(const std::string& layer, std::vector<Point2> vertices,
                    std::vector<std::optional<double>> heights, PropertyMap properties = {},
                    bool closed = false)
    {
        Entity entity;
        entity.layer = layer;
        entity.geometry = katana::geometry::Polyline2{std::move(vertices), closed};
        entity.properties = std::move(properties);
        katana::entity::setHeights(entity.properties, heights);
        return add(std::move(entity));
    }

    // A point as the survey import leaves one: its number and its height.
    EntityId point(const std::string& layer, Point2 at, const std::string& number,
                   std::optional<double> height, PropertyMap properties = {})
    {
        Entity entity;
        entity.layer = layer;
        entity.geometry = katana::entity::PointGeometry{at};
        entity.properties = std::move(properties);
        if (!number.empty()) {
            entity.properties.insert_or_assign("point", number);
        }
        katana::entity::setHeights(entity.properties, {height});
        return add(std::move(entity));
    }

    std::map<EntityId, Entity> snapshot() const
    {
        std::map<EntityId, Entity> entities;
        document.model().entities.forEach(
            [&entities](const Entity& entity) { entities.emplace(entity.id, entity); });
        return entities;
    }

    std::vector<EntityId> all() const
    {
        std::vector<EntityId> ids;
        document.model().entities.forEach(
            [&ids](const Entity& entity) { ids.push_back(entity.id); });
        return ids;
    }
};

// A directory of the test's own, removed by name.
struct ScratchDirectory {
    fs::path path;
    explicit ScratchDirectory(const std::string& name)
        : path(fs::temp_directory_path() / ("katana-utility-geometry-" + name))
    {
        fs::remove_all(path);
        fs::create_directories(path);
    }
    ~ScratchDirectory() { fs::remove_all(path); }

    std::string file(const std::string& name, const std::string& bytes) const
    {
        const fs::path where = path / name;
        std::ofstream out(where, std::ios::binary);
        out.write(bytes.data(), static_cast<std::streamsize>(bytes.size()));
        return "\"" + where.generic_string() + "\"";
    }
    std::string at(const std::string& name) const { return (path / name).generic_string(); }
    std::string read(const std::string& name) const
    {
        std::ifstream in(path / name, std::ios::binary);
        return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
    }
};

// What UTILITY DRAW made, entity by entity, as values that do not depend on
// ids: the entities under "utilities", with the provenance a draw from the
// drawing adds (keys::kSource) left out, since a schedule has none.
std::vector<std::string> drawn(const Document& document)
{
    using katana::core::formatExactReal;
    std::vector<std::string> entities;
    document.model().entities.forEach([&entities](const Entity& entity) {
        if (!entity.layer.starts_with("utilities")) {
            return;
        }
        std::string text =
            entity.layer + "|" + std::string(katana::entity::toString(entity.type()));
        if (const auto* point = std::get_if<katana::entity::PointGeometry>(&entity.geometry)) {
            text +=
                " " + formatExactReal(point->position.x) + "," + formatExactReal(point->position.y);
        } else if (const auto* run = std::get_if<katana::geometry::Polyline2>(&entity.geometry)) {
            for (const Point2& vertex : run->vertices) {
                text += " " + formatExactReal(vertex.x) + "," + formatExactReal(vertex.y);
            }
        }
        for (const auto& [key, value] : entity.properties) {
            if (key != keys::kSource) {
                text += "|" + key + "=" + std::string(katana::entity::typeName(value)) + ":" +
                        katana::entity::toString(value);
            }
        }
        entities.push_back(std::move(text));
    });
    std::ranges::sort(entities);
    return entities;
}

std::vector<sub::UtilityLine> parsed(const std::string& text)
{
    auto lines = sub::parseUtilityCsv(text);
    EXPECT_TRUE(lines.ok()) << lines.error().describe();
    return lines.ok() ? std::move(lines).value() : std::vector<sub::UtilityLine>{};
}

// ---- the survey --------------------------------------------------------------------------
//
// A water main detected by EML and marked on the road, the marks shot by
// GNSS - so its heights are the surface - with the locator's depth read at
// two marks; two potholes on it shot on the pipe, the first an open trench to
// the second and checking the detection at 102. An electricity duct picked by
// radar, whose string was reduced to the duct's top levels. Written out
// below as the schedule a person would type from the same field book.

const std::string kSurveySchedule =
    "line,point,easting,northing,method,level,level_ref,surface,depth,h_unc,v_unc,ql,path,"
    "verifies,type,owner,material,diameter_mm,status\n"
    "W1,101,334000,6250000,EML,,,20.25,1.05,0.1,,QL-B,,,water,WaterCo,DICL,150,in service\n"
    "W1,102,334008,6250000.15,EML,,,20.22,1.02,0.1,0.35,QL-B,,,,,,,\n"
    "W1,103,334016.08,6250000.31,pothole,19.155,top,20.22,,0.02,0.02,QL-A,exposed,102,,,,,\n"
    "W1,104,334017.5,6250000.3,pothole,19.15,top,20.21,,0.02,0.02,QL-A,,,,,,,\n"
    "W1,105,334030,6250000.5,EML,,,20.2,,0.1,,,,,,,,,\n"
    "E1,201,334000,6250003,GPR,19.6,top,,,0.25,0.45,,,,electricity,PowerCo,,,\n"
    "E1,202,334009,6250003.1,GPR,19.55,top,,,0.25,0.45,,,,,,,,\n";

// The survey in the drawing, as a coded survey leaves it: the strings on
// survey/strings named by their code, the points on survey/points. What the
// geometry cannot say is on the entities as utility.* properties - how the
// potholes were found, the locator's depths - or is left to the verb's
// options (the W1 main's method, uncertainty, type and owner).
struct Survey {
    EntityId water = 0;
    EntityId duct = 0;
    std::vector<EntityId> points;
};

Survey drawSurvey(Session& session)
{
    session.ok("LAYER NEW survey/strings");
    session.ok("LAYER NEW survey/points");
    Survey survey;
    const std::vector<Point2> water{{334000, 6250000},
                                    {334008, 6250000.15},
                                    {334016.08, 6250000.31},
                                    {334017.5, 6250000.3},
                                    {334030, 6250000.5}};
    // The potholes' shots are on the pipe, so the string's heights there are
    // the pipe's.
    const std::vector<std::optional<double>> waterHeights{20.25, 20.22, 19.155, 19.15, 20.2};
    survey.water = session.string("survey/strings", water, waterHeights,
                                  {{"code", std::string("W1")},
                                   {"utility.material", std::string("DICL")},
                                   {"utility.diameter", 0.15},
                                   {"utility.status", std::string("in service")}});
    const std::vector<Point2> duct{{334000, 6250003}, {334009, 6250003.1}};
    survey.duct = session.string("survey/strings", duct, {19.6, 19.55},
                                 {{"code", std::string("E1")},
                                  {"utility.type", std::string("electricity")},
                                  {"utility.owner", std::string("PowerCo")},
                                  {"utility.method", std::string("GPR")},
                                  {"utility.h_unc", 0.25},
                                  {"utility.v_unc", 0.45},
                                  {"utility.heights", std::string("service")}});

    const PropertyMap pothole{{"utility.method", std::string("pothole")},
                              {"utility.heights", std::string("service")},
                              {"utility.h_unc", 0.02},
                              {"utility.v_unc", 0.02},
                              {"utility.claimed", std::string("QL-A")}};
    PropertyMap trench = pothole;
    trench.insert_or_assign("utility.surface_level", 20.22);
    trench.insert_or_assign("utility.path", std::string("exposed"));
    trench.insert_or_assign("utility.verifies", std::string("102"));
    PropertyMap second = pothole;
    second.insert_or_assign("utility.surface_level", 20.21);
    const std::vector<std::pair<std::string, PropertyMap>> waterPoints{
        {"101",
         {{"code", std::string("W1")},
          {"utility.depth", 1.05},
          {"utility.claimed", std::string("QL-B")}}},
        {"102",
         {{"code", std::string("W1")},
          {"utility.depth", 1.02},
          {"utility.v_unc", 0.35},
          {"utility.claimed", std::string("QL-B")}}},
        {"103", trench},
        {"104", second},
        {"105", {{"code", std::string("W1")}}}};
    for (std::size_t i = 0; i < water.size(); ++i) {
        survey.points.push_back(session.point("survey/points", water[i], waterPoints[i].first,
                                              waterHeights[i], waterPoints[i].second));
    }
    survey.points.push_back(session.point("survey/points", duct[0], "201", 19.6));
    survey.points.push_back(session.point("survey/points", duct[1], "202", 19.55));
    return survey;
}

const std::string kDrawSurvey =
    "UTILITY DRAW LAYERS survey TYPE water METHOD EML H_UNC 0.1 OWNER WaterCo";

} // namespace

// ---- the survey is the schedule -------------------------------------------------------------

TEST(UtilityGeometry, TheSurveyReadAsServicesIsTheScheduleOfTheSameSurvey)
{
    Session session;
    const Survey survey = drawSurvey(session);
    GeometryServiceOptions options;
    options.type = sub::UtilityType::Water;
    options.method = sub::LocationMethod::ElectromagneticLocation;
    options.horizontalUncertainty = 0.1;
    options.owner = "WaterCo";
    std::vector<EntityId> scope = session.all();
    const auto services = readGeometryServices(session.document.model(), scope, options);
    ASSERT_TRUE(services.ok()) << services.error().describe();
    EXPECT_EQ(services->lines, parsed(kSurveySchedule));
    EXPECT_EQ(services->sources, (std::vector<EntityId>{survey.water, survey.duct}));
    EXPECT_EQ(services->points, 7u);
    EXPECT_EQ(services->loose, 0u);
    EXPECT_EQ(services->drawn, 0u);
    EXPECT_EQ(services->ignored, 0u);
}

TEST(UtilityGeometry, ASurveyDrawnAsServicesDrawsWhatTheScheduleOfTheSameSurveyDraws)
{
    const ScratchDirectory scratch("same");
    const std::string schedule = scratch.file("survey.csv", kSurveySchedule);
    Session fromSchedule;
    const std::string scheduleReply = fromSchedule.ok("UTILITY DRAW " + schedule);

    Session fromSurvey;
    drawSurvey(fromSurvey);
    const std::string reply = fromSurvey.ok(kDrawSurvey);
    EXPECT_EQ(drawn(fromSurvey.document), drawn(fromSchedule.document));
    // The draw's records, with what the scope took after the first: two
    // strings and seven points, each point on a vertex.
    EXPECT_EQ(firstLine(reply), firstLine(scheduleReply));
    EXPECT_EQ(line(reply, 1), "scope=layers layers=survey sublayers=yes matched=9 lines=2 "
                              "points=7 loose=0 drawn=0 ignored=0");
    EXPECT_EQ(withoutLine(reply, 1), scheduleReply);
    EXPECT_TRUE(contains(scheduleReply, "line id=W1 type=water")) << scheduleReply;
    EXPECT_TRUE(contains(scheduleReply, "line id=E1 type=electricity")) << scheduleReply;
}

TEST(UtilityGeometry, EveryVerbReadsWhatWasDrawnFromTheSurveyAsTheSchedulesDrawing)
{
    const ScratchDirectory scratch("verbs");
    const std::string schedule = scratch.file("survey.csv", kSurveySchedule);
    Session session;
    drawSurvey(session);
    session.ok(kDrawSurvey);
    const std::string report = session.ok("UTILITY REPORT LAYERS utilities MINCOVER 0.6");
    EXPECT_EQ(report.substr(report.find('\n') + 1),
              session.ok("UTILITY REPORT " + schedule + " MINCOVER 0.6"));
    const std::string verify = session.ok("UTILITY VERIFY LAYERS utilities");
    EXPECT_EQ(verify.substr(verify.find('\n') + 1), session.ok("UTILITY VERIFY " + schedule));

    // Written back, it is the schedule of the survey.
    session.ok("UTILITY SCHEDULE \"" + scratch.at("written.csv") + "\" LAYERS utilities");
    EXPECT_EQ(parsed(scratch.read("written.csv")), parsed(kSurveySchedule));

    // Nothing was edited, so a regrade changes nothing - and the points keep
    // the strings they were drawn from, which is what says they are drawn.
    const std::size_t steps = session.steps();
    EXPECT_TRUE(contains(firstLine(session.ok("UTILITY REGRADE LAYERS utilities")),
                         "utilities regraded changed=0 "));
    EXPECT_EQ(session.steps(), steps);
    std::size_t marked = 0;
    session.document.model().entities.forEach([&marked](const Entity& entity) {
        if (entity.type() == EntityType::Point && entity.properties.contains(keys::kLine)) {
            EXPECT_TRUE(entity.properties.contains(keys::kSource)) << entity.id;
            ++marked;
        }
    });
    EXPECT_EQ(marked, 7u);
}

TEST(UtilityGeometry, ADrawOfGeometryIsOneUndoStepAndLeavesTheSurveyAsItWas)
{
    Session session;
    const Survey survey = drawSurvey(session);
    const auto before = session.snapshot();
    const std::size_t layers = session.document.model().layers.size();
    const std::size_t steps = session.steps();
    session.ok(kDrawSurvey);
    EXPECT_EQ(session.steps(), steps + 1);
    const auto after = session.snapshot();
    for (const auto& [id, entity] : before) {
        ASSERT_TRUE(after.contains(id));
        EXPECT_EQ(after.at(id), entity) << "#" << id << " was changed";
    }
    EXPECT_GT(after.size(), before.size());
    ASSERT_TRUE(session.document.undo().ok());
    EXPECT_EQ(session.snapshot(), before);
    EXPECT_EQ(session.document.model().layers.size(), layers);
    EXPECT_NE(survey.water, 0u);
}

// ---- what each value is taken from -----------------------------------------------------------

TEST(UtilityGeometry, HeightsAreTheSurfaceUnlessSomethingSaysOtherwise)
{
    Session session;
    session.ok("LAYER NEW survey");
    const EntityId run = session.string("survey", {{0, 0}, {10, 0}, {20, 0}},
                                        {20.0, std::nullopt, 22.0}, {{"code", std::string("S1")}});
    // The string has no height at its middle; the point there has one.
    session.point("survey", {10, 0}, "", 21.0);
    const std::vector<EntityId> scope = session.all();
    const auto read = [&](GeometryHeights heights) {
        GeometryServiceOptions options;
        options.method = sub::LocationMethod::ElectromagneticLocation;
        options.heights = heights;
        auto services = readGeometryServices(session.document.model(), scope, options);
        EXPECT_TRUE(services.ok()) << services.error().describe();
        return services.ok() ? services->lines.front() : sub::UtilityLine{};
    };

    const sub::UtilityLine surface = read(GeometryHeights::Surface);
    ASSERT_EQ(surface.vertices.size(), 3u);
    const std::vector<double> expected{20.0, 21.0, 22.0};
    for (std::size_t i = 0; i < 3; ++i) {
        EXPECT_EQ(surface.vertices[i].surfaceLevel, expected[i]) << i;
        EXPECT_FALSE(surface.vertices[i].level) << i;
    }
    const sub::UtilityLine service = read(GeometryHeights::Service);
    for (std::size_t i = 0; i < 3; ++i) {
        EXPECT_EQ(service.vertices[i].level, expected[i]) << i;
        EXPECT_FALSE(service.vertices[i].surfaceLevel) << i;
        EXPECT_TRUE(service.vertices[i].evidence.hasLevel) << i;
    }
    const sub::UtilityLine none = read(GeometryHeights::Unused);
    for (const sub::UtilityVertex& vertex : none.vertices) {
        EXPECT_FALSE(vertex.level);
        EXPECT_FALSE(vertex.surfaceLevel);
    }

    // The line says it for all its vertices over the option; a point for its own.
    ASSERT_TRUE(session.document
                    .execute(katana::commands::setEntityProperty({run}, "utility.heights",
                                                                 std::string("service")))
                    .ok());
    const sub::UtilityLine told = read(GeometryHeights::Surface);
    EXPECT_EQ(told.vertices[0].level, 20.0);
    EXPECT_EQ(told.vertices[2].level, 22.0);
    // A string with no heights is a plan string: no level, no surface.
    session.ok("LAYER NEW flat");
    const EntityId plan = session.string("flat", {{0, 50}, {10, 50}}, {std::nullopt, std::nullopt},
                                         {{"code", std::string("F1")}});
    GeometryServiceOptions options;
    options.method = sub::LocationMethod::Records;
    const std::vector<EntityId> flatScope{plan};
    const auto flat = readGeometryServices(session.document.model(), flatScope, options);
    ASSERT_TRUE(flat.ok()) << flat.error().describe();
    for (const sub::UtilityVertex& vertex : flat->lines.front().vertices) {
        EXPECT_FALSE(vertex.level);
        EXPECT_FALSE(vertex.surfaceLevel);
        EXPECT_FALSE(vertex.evidence.hasLevel);
    }
}

TEST(UtilityGeometry, ThePointsOwnValueWinsOverTheLinesAndTheLinesOverTheOptions)
{
    Session session;
    session.ok("LAYER NEW survey");
    session.string("survey", {{0, 0}, {5, 0}, {10, 0}}, {},
                   {{"code", std::string("G1")},
                    {"utility.type", std::string("gas")},
                    {"utility.method", std::string("EML")}});
    session.point("survey", {5, 0}, "7", std::nullopt,
                  {{"utility.method", std::string("GPR")},
                   {"utility.h_unc", 0.05},
                   // A point says nothing of its line: the line is the gas main.
                   {"utility.type", std::string("water")},
                   {"utility.owner", std::string("Someone")}});
    GeometryServiceOptions options;
    options.type = sub::UtilityType::Electricity;
    options.method = sub::LocationMethod::Records;
    options.horizontalUncertainty = 0.3;
    options.owner = "GasCo";
    const std::vector<EntityId> scope = session.all();
    const auto services = readGeometryServices(session.document.model(), scope, options);
    ASSERT_TRUE(services.ok()) << services.error().describe();
    const sub::UtilityLine& gas = services->lines.front();
    EXPECT_EQ(gas.id, "G1");
    EXPECT_EQ(gas.attributes.type, sub::UtilityType::Gas);
    EXPECT_EQ(gas.attributes.owner, "GasCo");
    ASSERT_EQ(gas.vertices.size(), 3u);
    EXPECT_EQ(gas.vertices[0].evidence.method, sub::LocationMethod::ElectromagneticLocation);
    EXPECT_EQ(gas.vertices[1].evidence.method, sub::LocationMethod::GroundPenetratingRadar);
    EXPECT_EQ(gas.vertices[2].evidence.method, sub::LocationMethod::ElectromagneticLocation);
    EXPECT_EQ(gas.vertices[0].evidence.horizontalUncertainty, 0.3);
    EXPECT_EQ(gas.vertices[1].evidence.horizontalUncertainty, 0.05);
    // The point gives its vertex its number; the others are numbered along
    // the line.
    EXPECT_EQ(gas.vertices[0].id, "G1-1");
    EXPECT_EQ(gas.vertices[1].id, "7");
    EXPECT_EQ(gas.vertices[2].id, "G1-3");
}

TEST(UtilityGeometry, AServiceIsNamedByItsCodeAndTwoOfOneNameAreToldApartByTheirEntities)
{
    Session session;
    session.ok("LAYER NEW imported");
    const EntityId coded =
        session.string("imported", {{0, 0}, {10, 0}}, {}, {{"code", std::string("WM01")}});
    const EntityId first =
        session.string("imported", {{0, 10}, {10, 10}}, {}, {{"CODE", std::string("WATER")}});
    const EntityId second =
        session.string("imported", {{0, 20}, {10, 20}}, {}, {{"CODE", std::string("WATER")}});
    Entity archived;
    archived.layer = "imported";
    archived.geometry = katana::geometry::Segment2{{0, 30}, {10, 30}};
    archived.metadata.insert_or_assign("12d.name", std::string("GAS 1"));
    const EntityId fromArchive = session.add(archived);
    const EntityId unnamed = session.string("imported", {{0, 40}, {10, 40}}, {});

    const std::string reply = session.ok("UTILITY DRAW LAYERS imported METHOD records");
    for (const std::string& id :
         {std::string("WM01"), "WATER#" + std::to_string(first), "WATER#" + std::to_string(second),
          std::string("\"GAS 1\""), "#" + std::to_string(unnamed)}) {
        EXPECT_TRUE(contains(reply, "line id=" + id + " ")) << id << "\n" << reply;
    }
    EXPECT_NE(coded, fromArchive);

    // A service drawn already keeps its name; a new one of that name is told
    // apart from it.
    const EntityId another =
        session.string("imported", {{0, 50}, {10, 50}}, {}, {{"code", std::string("WM01")}});
    const std::string again = session.ok("UTILITY DRAW LAYERS imported METHOD records");
    EXPECT_TRUE(contains(again, "lines=1 ")) << again;
    EXPECT_TRUE(contains(again, "line id=WM01#" + std::to_string(another) + " ")) << again;
}

// ---- what the scope took --------------------------------------------------------------------

TEST(UtilityGeometry, ADrawOfTheSameScopeAgainDrawsOnlyWhatIsNew)
{
    Session session;
    drawSurvey(session);
    session.ok(kDrawSurvey);
    const std::size_t steps = session.steps();
    const auto before = session.snapshot();

    // Everything in the survey's layers is drawn already: the two strings and
    // their seven points.
    const std::string again = session.ok(kDrawSurvey);
    EXPECT_EQ(again, "scope=layers layers=survey sublayers=yes matched=9 lines=0 points=0 "
                     "loose=0 drawn=9 ignored=0\n"
                     "no lines or open polylines in the scope that are not drawn already: "
                     "nothing drawn");
    EXPECT_EQ(session.steps(), steps);
    EXPECT_EQ(session.snapshot(), before);

    // The whole drawing: the drawn services' runs and points are drawn too.
    const std::string everything = session.ok("UTILITY DRAW DRAWING METHOD EML");
    EXPECT_TRUE(contains(everything, " lines=0 points=0 loose=0 drawn=" +
                                         std::to_string(before.size()) + " ignored=0"))
        << everything;

    // A string surveyed since is drawn, and only it.
    session.string("survey/strings", {{334000, 6250010}, {334010, 6250010}}, {},
                   {{"code", std::string("T1")}});
    const std::size_t surveyed = session.steps();
    const std::string added = session.ok(kDrawSurvey);
    EXPECT_TRUE(contains(added, "matched=10 lines=1 points=0 loose=0 drawn=9 ignored=0")) << added;
    EXPECT_TRUE(contains(added, "line id=T1 type=water")) << added;
    EXPECT_EQ(session.steps(), surveyed + 1);
}

TEST(UtilityGeometry, WhatCannotBeAServiceIsIgnoredAndAPointOnNoLineIsLoose)
{
    Session session;
    session.ok("LAYER NEW site");
    session.string("site", {{0, 0}, {10, 0}}, {}, {{"code", std::string("S1")}});
    // An outline, not a run.
    session.string("site", {{0, 5}, {5, 5}, {5, 10}}, {}, {{"code", std::string("PIT")}}, true);
    Entity circle;
    circle.layer = "site";
    circle.geometry = katana::geometry::Circle2{{20, 20}, 1.0};
    session.add(circle);
    session.point("site", {3, 3}, "900", std::nullopt);
    const std::string reply = session.ok("UTILITY DRAW LAYERS site METHOD records");
    EXPECT_EQ(line(reply, 1), "scope=layers layers=site sublayers=yes matched=4 lines=1 "
                              "points=0 loose=1 drawn=0 ignored=2");

    // A scope with nothing to draw says so, and is no failure.
    Session empty;
    EXPECT_EQ(empty.ok("UTILITY DRAW DRAWING METHOD EML"),
              "scope=drawing matched=0 lines=0 points=0 loose=0 drawn=0 ignored=0\n"
              "no lines or open polylines in the scope that are not drawn already: nothing "
              "drawn");
    EXPECT_EQ(empty.steps(), 0u);
}

TEST(UtilityGeometry, AReportOfSurveyGeometrySaysHowToDrawItAsServices)
{
    Session session;
    drawSurvey(session);
    const std::string reply = session.ok("UTILITY REPORT LAYERS survey");
    EXPECT_EQ(firstLine(reply),
              "scope=layers layers=survey sublayers=yes matched=9 lines=0 completed=0 ignored=9");
    EXPECT_TRUE(contains(reply, "no utility lines in the scope: nothing to report\n"
                                "what the scope took carries no utility data; UTILITY DRAW "
                                "with the same scope and METHOD <method> draws"))
        << reply;
}

// ---- refused, by name, with nothing changed ----------------------------------------------------

TEST(UtilityGeometry, WhatCannotBeReadIsRefusedNamingTheEntityAndNothingChanges)
{
    Session session;
    const Survey survey = drawSurvey(session);
    const auto before = session.snapshot();
    const std::size_t steps = session.steps();
    const auto refusal = [&session](const std::string& text) {
        return session.refused(text).describe();
    };
    const auto unchanged = [&]() {
        EXPECT_EQ(session.snapshot(), before);
        EXPECT_EQ(session.steps(), steps);
    };

    // No method anywhere for the main's marks.
    std::string why = refusal("UTILITY DRAW LAYERS survey TYPE water");
    EXPECT_TRUE(contains(why, "line W1: vertex 101 (#" + std::to_string(survey.points[0]) +
                                  ") says no method it was located by; give METHOD"))
        << why;
    unchanged();

    // A value on the string that does not read is refused by the string.
    const auto setOn = [&session](EntityId id, const std::string& key, const std::string& value) {
        ASSERT_TRUE(
            session.document.execute(katana::commands::setEntityProperty({id}, key, value)).ok());
    };
    setOn(survey.duct, "utility.method", "dowsing");
    why = refusal(kDrawSurvey);
    EXPECT_TRUE(contains(why, "line E1: polyline (#" + std::to_string(survey.duct) +
                                  ") has utility.method \"dowsing\", which is not a location "
                                  "method"))
        << why;
    ASSERT_TRUE(session.document.undo().ok());

    // And one on a point, by the point.
    setOn(survey.points[1], "utility.depth", "deep");
    why = refusal(kDrawSurvey);
    EXPECT_TRUE(contains(why, "line W1: point 102 (#" + std::to_string(survey.points[1]) +
                                  ") has utility.depth \"deep\", which is not a number"))
        << why;
    ASSERT_TRUE(session.document.undo().ok());
    setOn(survey.points[1], "utility.heights", "sky");
    why = refusal(kDrawSurvey);
    EXPECT_TRUE(contains(why, "has utility.heights \"sky\", which is not surface, service or none"))
        << why;
    ASSERT_TRUE(session.document.undo().ok());
    unchanged();

    // Two points on one vertex: which is the survey's is not guessed.
    const EntityId twin = session.point("survey/points", {334008, 6250000.15}, "102A", 20.22);
    why = refusal(kDrawSurvey);
    EXPECT_TRUE(contains(why, "line W1: vertex 2 of polyline (#" + std::to_string(survey.water) +
                                  ") has two points on it, #" + std::to_string(survey.points[1]) +
                                  " and #" + std::to_string(twin)))
        << why;
    ASSERT_TRUE(session.document.undo().ok());

    // A point that is a vertex of a schedule and lost its line.
    setOn(survey.points[4], "utility.vertex", "W1-5");
    why = refusal(kDrawSurvey);
    EXPECT_TRUE(contains(why, "is a vertex of a schedule and has no utility.line")) << why;
    ASSERT_TRUE(session.document.undo().ok());
    unchanged();

    // A layer the draw would use, locked: refused when it runs, and the
    // layers made before it taken back.
    session.ok("LAYER NEW utilities/water/QL-A");
    session.ok("LAYER LOCK utilities/water/QL-A");
    const auto locked = session.snapshot();
    const std::size_t layers = session.document.model().layers.size();
    why = refusal(kDrawSurvey);
    EXPECT_TRUE(contains(why, "locked")) << why;
    EXPECT_EQ(session.snapshot(), locked);
    EXPECT_EQ(session.document.model().layers.size(), layers);
}

TEST(UtilityGeometry, TheOptionsAreRefusedByNameWhenTheyDoNotRead)
{
    Session session;
    drawSurvey(session);
    const std::size_t steps = session.steps();
    const auto refusal = [&session](const std::string& text) {
        return session.refused("UTILITY DRAW LAYERS survey METHOD EML " + text).describe();
    };
    EXPECT_TRUE(contains(refusal("TYPE steam"), "TYPE steam is not a utility type"));
    EXPECT_TRUE(contains(session.refused("UTILITY DRAW LAYERS survey METHOD dowsing").describe(),
                         "METHOD dowsing is not a location method"));
    EXPECT_TRUE(contains(refusal("HEIGHTS sky"), "HEIGHTS sky is not surface, service or none"));
    EXPECT_TRUE(contains(refusal("LEVEL_REF side"), "LEVEL_REF side is not a level reference"));
    EXPECT_TRUE(contains(refusal("PATH flown"), "PATH flown is not detected, exposed or assumed"));
    EXPECT_TRUE(contains(refusal("STATUS gone"), "STATUS gone is not a status"));
    EXPECT_TRUE(contains(refusal("DIAMETER_MM 0"),
                         "DIAMETER_MM needs a diameter in millimetres, more than 0"));
    EXPECT_TRUE(contains(refusal("H_UNC -0.1"),
                         "H_UNC needs a horizontal uncertainty, in metres, not negative"));
    EXPECT_TRUE(contains(refusal("OWNER"), "OWNER needs the owner's name"));
    EXPECT_TRUE(contains(refusal("TYPE water TYPE gas"), "TYPE given twice"));
    EXPECT_TRUE(contains(refusal("COLOUR red"), "unknown option COLOUR"));
    // A schedule says these in its columns.
    EXPECT_TRUE(contains(
        session.refused("UTILITY DRAW schedule.csv METHOD EML").describe(),
        "METHOD is for a draw of geometry in the drawing (UTILITY DRAW <scope> ...); a schedule "
        "gives its own in its columns"));
    EXPECT_EQ(session.steps(), steps);
}

TEST(UtilityGeometry, EveryOptionReachesTheDrawing)
{
    Session session;
    session.ok("LAYER NEW dxf");
    session.string("dxf", {{0, 0}, {6, 0}, {12, 0}}, {1.5, 1.4, 1.3},
                   {{"code", std::string("S7")}});
    const std::string reply = session.ok(
        "UTILITY DRAW LAYERS dxf TYPE sewer METHOD pothole H_UNC 0.03 V_UNC 0.04 HEIGHTS service "
        "LEVEL_REF invert PATH exposed OWNER \"Water Board\" MATERIAL VC DIAMETER_MM 225 STATUS "
        "abandoned SPACING 5 MINCOVER 0.9 LAYER located");
    EXPECT_TRUE(contains(reply, "line id=S7 type=sewer")) << reply;
    std::vector<const Entity*> points;
    session.document.model().entities.forEach([&points](const Entity& entity) {
        if (entity.type() == EntityType::Point && entity.layer == "located/sewer/points") {
            points.push_back(&entity);
        }
    });
    ASSERT_EQ(points.size(), 3u);
    const PropertyMap& first = points.front()->properties;
    const auto text = [&first](std::string_view key) {
        const auto found = first.find(key);
        return found == first.end() ? std::string("(absent)")
                                    : katana::entity::toString(found->second);
    };
    EXPECT_EQ(text(keys::kType), "sewer");
    EXPECT_EQ(text(keys::kMethod), sub::toString(sub::LocationMethod::NonDestructiveExcavation));
    EXPECT_EQ(text(keys::kHorizontalUncertainty), "0.03");
    EXPECT_EQ(text(keys::kVerticalUncertainty), "0.04");
    EXPECT_EQ(text(keys::kLevel), "1.5");
    EXPECT_EQ(text(keys::kSurfaceLevel), "(absent)");
    EXPECT_EQ(text(keys::kLevelReference), "invert");
    EXPECT_EQ(text(keys::kPath), "exposed");
    EXPECT_EQ(text(keys::kOwner), "Water Board");
    EXPECT_EQ(text(keys::kMaterial), "VC");
    EXPECT_EQ(text(keys::kDiameter), "0.225");
    EXPECT_EQ(text(keys::kStatus), sub::toString(sub::UtilityStatus::Abandoned));
    EXPECT_EQ(text(keys::kSpacing), "5");
    EXPECT_EQ(text(keys::kMinimumCover), "0.9");
}

// ---- an import's own attributes ------------------------------------------------------------

TEST(UtilityGeometry, FieldsReadAnImportsOwnAttributesAsTheSchedulesColumns)
{
    // A water authority's shapefile of its mains as an import leaves it: its
    // attributes under its own names. Its asset id, type code, locate method
    // (as AS 5488.2 names one), a depth for the whole main, a diameter in
    // millimetres, one of the delivery schema's own attributes - and an
    // owner a person has corrected in Katana since, which wins.
    Session session;
    session.ok("LAYER NEW gis");
    const EntityId main = session.string("gis", {{0, 0}, {8, 0}, {16, 0}}, {},
                                         {{"ASSET_ID", std::string("W-2231")},
                                          {"ASSET_TYPE", std::string("W")},
                                          {"LOC_METHOD", std::string("Electronic Detection")},
                                          {"DEPTH", 0.9},
                                          {"DIA", std::int64_t{150}},
                                          {"OWNER", std::string("Old Water Board")},
                                          {"utility.owner", std::string("WaterCo")},
                                          {"FEAT", std::string("Main")}});
    session.point("gis", {8, 0}, "", std::nullopt, {{"PT_ID", std::string("V17")}});
    const std::string reply =
        session.ok("UTILITY DRAW LAYERS gis H_UNC 0.2 FIELDS \"line=ASSET_ID, type=ASSET_TYPE, "
                   "LocateMethod=LOC_METHOD,depth=DEPTH,diameter_mm=DIA,owner=OWNER,point=PT_ID,"
                   "AssetFeature=FEAT\"");
    EXPECT_TRUE(contains(reply, "line id=W-2231 type=water")) << reply;
    EXPECT_NE(main, 0u);

    std::map<std::string, PropertyMap> byVertex;
    session.document.model().entities.forEach([&byVertex](const Entity& entity) {
        if (entity.type() == EntityType::Point && entity.layer == "utilities/water/points") {
            byVertex.emplace(
                katana::entity::toString(entity.properties.at(std::string(keys::kVertex))),
                entity.properties);
        }
    });
    ASSERT_EQ(byVertex.size(), 3u);
    ASSERT_TRUE(byVertex.contains("V17"));
    ASSERT_TRUE(byVertex.contains("W-2231-1"));
    const PropertyMap& first = byVertex.at("W-2231-1");
    const auto text = [&first](std::string_view key) {
        const auto found = first.find(key);
        return found == first.end() ? std::string("(absent)")
                                    : katana::entity::toString(found->second);
    };
    EXPECT_EQ(text(keys::kMethod), sub::toString(sub::LocationMethod::ElectromagneticLocation));
    EXPECT_EQ(text(keys::kDepth), "0.9");
    EXPECT_EQ(text(keys::kDiameter), "0.15");
    EXPECT_EQ(text(keys::kOwner), "WaterCo");
    EXPECT_EQ(text(std::string(keys::kFieldPrefix) + "AssetFeature"), "Main");
    EXPECT_EQ(text(keys::kHorizontalUncertainty), "0.2");
}

TEST(UtilityGeometry, FieldsThatNameNoColumnAreRefusedByName)
{
    Session session;
    session.ok("LAYER NEW gis");
    session.string("gis", {{0, 0}, {8, 0}}, {}, {});
    const std::size_t steps = session.steps();
    const auto refusal = [&session](const std::string& fields) {
        return session.refused("UTILITY DRAW LAYERS gis METHOD records FIELDS " + fields)
            .describe();
    };
    EXPECT_TRUE(contains(refusal("colour=COL"), "FIELDS: colour is not a column of a utility "
                                                "schedule"));
    EXPECT_TRUE(contains(refusal("easting=X"), "FIELDS: easting is where the geometry is"));
    EXPECT_TRUE(contains(refusal("Size=SZ"), "FIELDS: size is read from a schedule only"));
    EXPECT_TRUE(contains(refusal("type"), "FIELDS needs column=property pairs"));
    EXPECT_TRUE(contains(refusal("type=A,utility_type=B"), "FIELDS: type given twice"));
    EXPECT_TRUE(contains(refusal("type="), "FIELDS: type names no property"));
    EXPECT_EQ(session.steps(), steps);
}
