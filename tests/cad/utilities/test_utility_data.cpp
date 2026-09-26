// The UTILITY verbs on what is drawn (utility_data.hpp, scope_verbs.hpp):
// every verb but DRAW takes the services UTILITY DRAW drew, by the scope and
// filter words Global Modify takes, as well as a schedule file.
//
// The proofs the drawing must pass to stand for the schedule:
//   * a schedule drawn and then reported from the drawing gives the report
//     of the schedule, word for word;
//   * the schedule written from the drawing reads back as the lines drawn,
//     field for field and doubles to the bit;
//   * a point moved or a method edited, then REGRADE, draws exactly what a
//     DRAW of the schedule so edited draws - and one undo puts it back.
//
// The sample is samples/utilities, graded by hand in
// docs/subsurface_utilities.md.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <map>
#include <regex>
#include <string>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/cad/utilities/utility_data.hpp"
#include "katana/cad/utilities/utility_drawing.hpp"
#include "katana/cad/utilities/utility_verbs.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/profile.hpp"
#include "katana/survey/subsurface/utility_csv.hpp"
#include "katana/survey/subsurface/utility_report.hpp"

namespace fs = std::filesystem;
namespace sub = katana::survey::subsurface;
using namespace katana::cad::utilities;
using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::EntityType;
using katana::geometry::Point2;

namespace {

const std::string kSamples = KATANA_UTILITY_SAMPLES;

std::string sample(const std::string& name)
{
    return "\"" + kSamples + "/" + name + "\"";
}

std::string sampleText(const std::string& name)
{
    std::ifstream file(kSamples + "/" + name, std::ios::binary);
    EXPECT_TRUE(file.good()) << name;
    return std::string(std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>());
}

std::vector<sub::UtilityLine> parsed(const std::string& text)
{
    auto lines = sub::parseUtilityCsv(text);
    EXPECT_TRUE(lines.ok()) << lines.error().describe();
    return lines.ok() ? std::move(lines).value() : std::vector<sub::UtilityLine>{};
}

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

std::string firstLine(const std::string& text)
{
    return text.substr(0, text.find('\n'));
}

std::string afterFirstLine(const std::string& text)
{
    const std::size_t end = text.find('\n');
    return end == std::string::npos ? std::string() : text.substr(end + 1);
}

std::string withoutTrailingBreaks(std::string text)
{
    while (!text.empty() && text.back() == '\n') {
        text.pop_back();
    }
    return text;
}

struct Session {
    Document document;
    CommandInterpreter interpreter{document};

    std::string ok(const std::string& line)
    {
        auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << "\n  -> "
                                << (reply.ok() ? std::string{} : reply.error().describe());
        return reply.ok() ? *reply : std::string{};
    }
    katana::core::Error refused(const std::string& line)
    {
        auto reply = interpreter.run(line);
        EXPECT_FALSE(reply.ok()) << line << " was accepted:\n" << (reply.ok() ? *reply : "");
        return reply.ok() ? katana::core::Error{} : reply.error();
    }
    std::size_t steps() const { return document.history().undoCount(); }

    // The drawn point of vertex `vertex`.
    EntityId point(const std::string& vertex) const
    {
        EntityId found = 0;
        document.model().entities.forEach([&](const Entity& entity) {
            const auto id = entity.properties.find(keys::kVertex);
            if (entity.type() == EntityType::Point && id != entity.properties.end() &&
                katana::entity::toString(id->second) == vertex) {
                found = entity.id;
            }
        });
        EXPECT_NE(found, 0u) << "no point " << vertex;
        return found;
    }

    void select(std::vector<EntityId> ids)
    {
        document.selection().set(std::move(ids));
        document.notifySelectionChanged();
    }

    std::map<EntityId, Entity> snapshot() const
    {
        std::map<EntityId, Entity> entities;
        document.model().entities.forEach(
            [&entities](const Entity& entity) { entities.emplace(entity.id, entity); });
        return entities;
    }
};

// A directory of the test's own, removed by name.
struct ScratchDirectory {
    fs::path path;
    explicit ScratchDirectory(const std::string& name)
        : path(fs::temp_directory_path() / ("katana-utility-data-" + name))
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

// An entity as a value that does not depend on its id: what two drawings of
// one schedule must agree on, entity for entity.
std::string describe(const Entity& entity)
{
    using katana::core::formatExactReal;
    std::string text = entity.layer + "|" + std::string(katana::entity::toString(entity.type()));
    if (const auto* point = std::get_if<katana::entity::PointGeometry>(&entity.geometry)) {
        text += " " + formatExactReal(point->position.x) + "," + formatExactReal(point->position.y);
    } else if (const auto* line = std::get_if<katana::geometry::Polyline2>(&entity.geometry)) {
        for (const Point2& vertex : line->vertices) {
            text += " " + formatExactReal(vertex.x) + "," + formatExactReal(vertex.y);
        }
    }
    text += "|" + entity.style + "|" + (entity.color ? entity.color->toHex() : "ByLayer") +
            (entity.visible ? "" : "|hidden");
    for (const auto& [key, value] : entity.properties) {
        text += "|" + key + "=" + std::string(katana::entity::typeName(value)) + ":" +
                katana::entity::toString(value);
    }
    return text;
}

std::vector<std::string> drawing(const Document& document)
{
    std::vector<std::string> entities;
    document.model().entities.forEach(
        [&entities](const Entity& entity) { entities.push_back(describe(entity)); });
    std::ranges::sort(entities);
    return entities;
}

// A schedule of the test's own drawn into a fresh document.
std::vector<std::string> drawnFrom(const std::string& quotedPath, const std::string& options = {})
{
    Session fresh;
    fresh.ok("UTILITY DRAW " + quotedPath + options);
    return drawing(fresh.document);
}

std::string replaced(std::string text, const std::string& from, const std::string& to)
{
    const std::size_t at = text.find(from);
    EXPECT_NE(at, std::string::npos) << from;
    if (at != std::string::npos) {
        text.replace(at, from.size(), to);
    }
    return text;
}

// A schema of the tests' own in the TfNSW Utility Schema's attribute names,
// as tools/utility_schema_domains.py writes one: the real schema is the
// client's document and stays out of the repository.
std::string schema(bool withRadar)
{
    return std::string("kind,attribute,value,detail,label\n"
                       "schema,Example Utility Schema,0.2,,\n"
                       "identifier,AssetIdentifier,,,\n"
                       "field,AssetIdentifier,Alphanumerical,Yes,Asset Identifier\n"
                       "field,AssetStatus,Domain List: Asset Status,Yes,Asset Status\n"
                       "domain,AssetStatus,In Service,,\n"
                       "domain,AssetStatus,Abandoned,,\n"
                       "field,LocateMethod,Domain List: Locate Method,Yes,Locate Method\n"
                       "domain,LocateMethod,Electronic Detection,,\n"
                       "domain,LocateMethod,Potholing,,\n") +
           (withRadar ? "domain,LocateMethod,Ground Penetrating Radar,,\n" : "") +
           "domain,LocateMethod,Archive Drawings and Plans,,\n"
           "domain,LocateMethod,Geographic Information System,,\n"
           "field,QualityLevel,Domain List: Quality Level,Yes,Quality Level\n"
           "domain,QualityLevel,Quality Level A,,\n"
           "domain,QualityLevel,Quality Level B,,\n"
           "domain,QualityLevel,Quality Level C,,\n"
           "domain,QualityLevel,Quality Level D,,\n";
}

} // namespace

// ---- the drawing is the schedule ------------------------------------------------------------

TEST(UtilityData, TheReportOfTheDrawingIsTheReportOfTheScheduleItWasDrawnFrom)
{
    for (const char* name : {"schedule.csv", "schedule_tfnsw.csv"}) {
        for (const char* options : {" MINCOVER 0.6", " SPACING 20", ""}) {
            Session session;
            const std::string fromFile = session.ok("UTILITY REPORT " + sample(name) + options);
            session.ok("UTILITY DRAW " + sample(name));
            const std::string fromDrawing = session.ok("UTILITY REPORT DRAWING" +
                                                       std::string(options));
            EXPECT_EQ(afterFirstLine(fromDrawing), fromFile) << name << options;
        }
    }
    Session session;
    session.ok("UTILITY DRAW " + sample("schedule.csv"));
    // 7 runs and 14 points, 4 lines, all of each.
    EXPECT_EQ(firstLine(session.ok("UTILITY REPORT DRAWING")),
              "scope=drawing matched=21 lines=4 completed=0 ignored=0");
}

TEST(UtilityData, VerifyClearanceAndCheckOfTheDrawingAreThoseOfTheSchedule)
{
    Session session;
    session.ok("UTILITY DRAW " + sample("schedule.csv"));
    EXPECT_EQ(afterFirstLine(session.ok("UTILITY VERIFY DRAWING")),
              session.ok("UTILITY VERIFY " + sample("schedule.csv")));
    const std::string fromFile =
        session.ok("UTILITY CLEARANCE " + sample("schedule.csv") + " " + sample("design.csv") +
                   " WIDTH 0.375");
    EXPECT_EQ(afterFirstLine(session.ok("UTILITY CLEARANCE DRAWING DESIGN " +
                                        sample("design.csv") + " WIDTH 0.375")),
              fromFile);
    // DESIGN may be left out, as the file's second place always took it.
    EXPECT_EQ(afterFirstLine(session.ok("UTILITY CLEARANCE DRAWING " + sample("design.csv") +
                                        " WIDTH 0.375")),
              fromFile);
    EXPECT_EQ(session.ok("UTILITY CLEARANCE " + sample("schedule.csv") + " DESIGN " +
                         sample("design.csv") + " WIDTH 0.375"),
              fromFile);
    // None of them touched the drawing.
    EXPECT_EQ(session.steps(), 1u);
}

TEST(UtilityData, TheDrawingReadBackIsTheScheduleItWasDrawnFrom)
{
    for (const char* name : {"schedule.csv", "schedule_tfnsw.csv"}) {
        Session session;
        session.ok("UTILITY DRAW " + sample(name));
        std::vector<EntityId> everything;
        session.document.model().entities.forEach(
            [&everything](const Entity& entity) { everything.push_back(entity.id); });
        const auto data = readUtilityData(session.document.model(), everything);
        ASSERT_TRUE(data.ok()) << data.error().describe();
        EXPECT_EQ(data->lines(), parsed(sampleText(name))) << name;
        EXPECT_EQ(data->ignored, 0u);
        EXPECT_EQ(data->completed, 0u);
    }
}

TEST(UtilityData, AScheduleWrittenFromTheDrawingReadsBackAsTheLinesDrawn)
{
    const ScratchDirectory scratch("schedule");
    for (const char* name : {"schedule.csv", "schedule_tfnsw.csv"}) {
        Session session;
        session.ok("UTILITY DRAW " + sample(name));
        const std::string out = scratch.at(std::string("out ") + name);
        const std::string reply = session.ok("UTILITY SCHEDULE \"" + out + "\" DRAWING");
        const std::size_t vertices = std::string(name) == "schedule.csv" ? 14 : 7;
        const std::size_t lines = std::string(name) == "schedule.csv" ? 4 : 3;
        EXPECT_EQ(firstLine(reply), "utilities scheduled path=\"" + out + "\" lines=" +
                                        std::to_string(lines) + " vertices=" +
                                        std::to_string(vertices));
        EXPECT_TRUE(afterFirstLine(reply).starts_with("scope=drawing matched=")) << reply;
        EXPECT_EQ(parsed(scratch.read(std::string("out ") + name)), parsed(sampleText(name)))
            << name;
        // The report of the schedule written is the report of the one drawn.
        EXPECT_EQ(session.ok("UTILITY REPORT \"" + out + "\" MINCOVER 0.6"),
                  session.ok("UTILITY REPORT " + sample(name) + " MINCOVER 0.6"));
        EXPECT_EQ(session.steps(), 1u);
    }
}

// ---- what the scope takes -------------------------------------------------------------------

TEST(UtilityData, APartOfALineTakesTheWholeLineAndTheReplySaysSo)
{
    Session session;
    session.ok("UTILITY DRAW " + sample("schedule.csv"));
    const std::vector<sub::UtilityLine> lines = parsed(sampleText("schedule.csv"));
    const std::string waterOnly = withoutTrailingBreaks(
        *sub::renderInvestigationReport({lines.front()}, sub::GradingSettings{}, std::nullopt));

    // One point of W1 selected: all of W1 is reported.
    session.select({session.point("W1-3")});
    std::string reply = session.ok("UTILITY REPORT SELECTION");
    EXPECT_EQ(firstLine(reply), "scope=selection matched=1 lines=1 completed=1 ignored=0");
    EXPECT_EQ(afterFirstLine(reply), waterOnly);

    // Only W1's QL-B run: W1 whole again, from its points.
    reply = session.ok("UTILITY REPORT LAYERS utilities/water/QL-B");
    EXPECT_EQ(firstLine(reply), "scope=layers layers=utilities/water/QL-B sublayers=yes matched=1 "
                                "lines=1 completed=1 ignored=0");
    EXPECT_EQ(afterFirstLine(reply), waterOnly);

    // Every layer of water: its three runs and six points, nothing completed.
    reply = session.ok("UTILITY REPORT LAYERS utilities/water");
    EXPECT_EQ(firstLine(reply),
              "scope=layers layers=utilities/water sublayers=yes matched=9 lines=1 completed=0 "
              "ignored=0");

    // The filter narrows as MODIFY's does: the gas main's run and points.
    reply = session.ok("UTILITY REPORT DRAWING WHERE PROP=utility.type:gas");
    EXPECT_EQ(firstLine(reply), "scope=drawing where=\"PROP=utility.type:gas\" matched=3 lines=1 "
                                "completed=0 ignored=0");
    EXPECT_TRUE(contains(reply, "G1")) << reply;
    EXPECT_FALSE(contains(reply, "W1-3")) << reply;

    // A window over the gas main's points (y 6250007 to 6250007.2) takes its
    // points and its run, and not the telecommunications run at y 6250005.5.
    reply = session.ok("UTILITY REPORT AREA 333990,6250006.5,334050,6250008");
    EXPECT_EQ(firstLine(reply), "scope=area area=333990,6250006.5,334050,6250008 matched=3 "
                                "lines=1 completed=0 ignored=0");
}

TEST(UtilityData, WhatCarriesNoUtilityDataIsIgnoredAndCounted)
{
    Session session;
    session.ok("UTILITY DRAW " + sample("schedule.csv"));
    session.ok("LINE 0,0 10,10");
    session.ok("POINT 5,5");
    const std::string reply = session.ok("UTILITY REPORT DRAWING MINCOVER 0.6");
    EXPECT_EQ(firstLine(reply), "scope=drawing matched=23 lines=4 completed=0 ignored=2");
    EXPECT_EQ(afterFirstLine(reply),
              session.ok("UTILITY REPORT " + sample("schedule.csv") + " MINCOVER 0.6"));
}

TEST(UtilityData, AScopeThatTakesNothingSaysSoAndIsNoFailure)
{
    Session session;
    session.ok("UTILITY DRAW " + sample("schedule.csv"));
    const ScratchDirectory scratch("nothing");
    const std::string schemaFile = scratch.file("schema.csv", schema(true));
    const std::string record = "scope=selection matched=0 lines=0 completed=0 ignored=0\n"
                               "no utility lines in the scope: nothing ";
    EXPECT_EQ(session.ok("UTILITY REPORT SELECTION"), record + "to report");
    EXPECT_EQ(session.ok("UTILITY VERIFY SELECTION"), record + "to verify");
    EXPECT_EQ(session.ok("UTILITY CLEARANCE SELECTION DESIGN " + sample("design.csv")),
              record + "to clear");
    EXPECT_EQ(session.ok("UTILITY CHECK SELECTION SCHEMA " + schemaFile), record + "to check");
    EXPECT_EQ(session.ok("UTILITY REGRADE SELECTION"), record + "to regrade");
    EXPECT_EQ(session.ok("UTILITY SCHEDULE \"" + scratch.at("none.csv") + "\" SELECTION"),
              record + "written");
    EXPECT_FALSE(fs::exists(scratch.at("none.csv")));
    EXPECT_EQ(session.steps(), 1u);
}

TEST(UtilityData, TheWindowsViewIsAScopeAndHeadlessItIsRefusedNamingArea)
{
    Session session;
    session.ok("UTILITY DRAW " + sample("schedule.csv"));
    const katana::core::Error headless = session.refused("UTILITY REPORT VIEW");
    EXPECT_EQ(headless.code, ErrorCode::InvalidState);
    EXPECT_TRUE(contains(headless.message, "AREA x0,y0,x1,y1")) << headless.message;

    // A plan view hiding the gas services draws the other three.
    session.interpreter.setScopeContext(
        [](std::optional<std::uint32_t>) -> katana::core::Result<katana::cad::ScopeView> {
            katana::cad::ScopeView view;
            view.id = 2;
            view.layers.hide("utilities/gas");
            return view;
        });
    const std::string reply = session.ok("UTILITY REPORT VIEW");
    EXPECT_EQ(firstLine(reply), "scope=view view=2 matched=18 lines=3 completed=0 ignored=0");
}

TEST(UtilityData, APathTypedOnAPointReadsAsTheSchedulesPathColumnReadsIt)
{
    // The schedule's path column takes the field words "traced", "trench"
    // and "inferred" too (subsurface::parsePathEvidence); the drawing took
    // only the three it writes, and refused "trench" typed in the property
    // panel for the pothole's open trench, which the schedule would read.
    Session session;
    session.ok("UTILITY DRAW " + sample("schedule.csv"));
    const std::string report = session.ok("UTILITY REPORT DRAWING");
    session.select({session.point("W1-X1")});
    session.ok("PROP SET utility.path trench text");
    EXPECT_EQ(session.ok("UTILITY REPORT DRAWING"), report);
    session.ok("PROP SET utility.path Trench text");
    EXPECT_EQ(session.ok("UTILITY REPORT DRAWING"), report);
    session.ok("PROP SET utility.path dug text");
    EXPECT_TRUE(contains(session.refused("UTILITY REPORT DRAWING").message,
                         "has utility.path \"dug\", which is not detected, exposed or assumed"));
}

TEST(UtilityData, PointsThatDoNotAgreeAreRefusedByName)
{
    Session session;
    session.ok("UTILITY DRAW " + sample("schedule.csv"));
    const EntityId first = session.point("W1-1");
    const EntityId second = session.point("W1-2");
    const auto refusal = [&session](const std::string& line) {
        return session.refused(line).message;
    };

    // Two points at one place along the line.
    session.select({second});
    session.ok("PROP SET utility.order 1 integer");
    std::string why = refusal("UTILITY REPORT DRAWING");
    EXPECT_EQ(why, "line W1 has two points at utility.order 1: #" + std::to_string(first) +
                       " and #" + std::to_string(second));
    session.ok("UNDO");

    // A point with no method.
    session.ok("PROP DELETE utility.method");
    why = refusal("UTILITY REPORT DRAWING");
    EXPECT_EQ(why, "line W1: point W1-2 (#" + std::to_string(second) +
                       ") has no utility.method, the method it was located by");
    session.ok("UNDO");

    // A point that says another owner than its line's first.
    session.ok("PROP SET utility.owner \"Other Co\"");
    why = refusal("UTILITY REPORT DRAWING");
    EXPECT_EQ(why, "line W1: point W1-2 (#" + std::to_string(second) +
                       ") has utility.owner \"Other Co\" where point W1-1 (#" +
                       std::to_string(first) +
                       ") has \"WaterCo\"; the points of one line must agree");
    session.ok("UNDO");

    // A level that is not a number.
    session.ok("PROP SET utility.level deep text");
    EXPECT_TRUE(contains(refusal("UTILITY REPORT DRAWING"),
                         "has utility.level \"deep\", which is not a number"));
    session.ok("UNDO");

    // A method nobody knows.
    session.ok("PROP SET utility.method dowsing");
    EXPECT_TRUE(contains(refusal("UTILITY REPORT LAYERS utilities/water"),
                         "utility.method \"dowsing\", which is not a location method"));
    session.ok("UNDO");

    // A line whose points were all erased, its run left.
    session.select({session.point("G1-1"), session.point("G1-2")});
    session.ok("ERASE");
    EXPECT_TRUE(contains(refusal("UTILITY REPORT LAYERS utilities/gas"),
                         "line G1 has no points left to read it from"));
    session.ok("UNDO");

    // A schedule drawn twice is two points at every place of every line.
    session.ok("UTILITY DRAW " + sample("schedule.csv"));
    EXPECT_TRUE(contains(refusal("UTILITY REPORT DRAWING"), "has two points at utility.order 1"));

    // A person's own entities outside a scope are not read: a scope that
    // takes only what agrees is answered.
    session.ok("UNDO");
    EXPECT_TRUE(session.ok("UTILITY REPORT DRAWING").starts_with("scope=drawing matched=21"));
}

TEST(UtilityData, APointWhoseLineWasDeletedRefusesItsLineRatherThanLeavingItShort)
{
    Session session;
    session.ok("UTILITY DRAW " + sample("schedule.csv"));
    const EntityId lost = session.point("W1-2");
    session.select({lost});
    session.ok("PROP DELETE utility.line");
    const std::string why = "point W1-2 (#" + std::to_string(lost) +
                            ") is a vertex of a schedule and has no utility.line, the line it is "
                            "a point of; by its attributes it is a point of line W1; give it its "
                            "line again, or delete its utility.vertex and utility.order";
    // In the scope: not "ignored", as a point of no schedule is.
    EXPECT_EQ(session.refused("UTILITY REPORT DRAWING").message, why);
    // Outside it: W1, read whole from its other points, would be W1 short of
    // a vertex - graded, redrawn and written so.
    session.select({session.point("W1-3")});
    const std::size_t steps = session.steps();
    const std::map<EntityId, Entity> before = session.snapshot();
    EXPECT_EQ(session.refused("UTILITY REGRADE SELECTION").message, why);
    EXPECT_EQ(session.steps(), steps);
    EXPECT_EQ(session.snapshot(), before);
    const ScratchDirectory scratch("stray");
    EXPECT_EQ(session.refused("UTILITY SCHEDULE \"" + scratch.at("out.csv") +
                              "\" LAYERS utilities/water")
                  .message,
              why);
    EXPECT_FALSE(fs::exists(scratch.at("out.csv")));
    // A line it is not a point of reads as it did.
    EXPECT_EQ(firstLine(session.ok("UTILITY REPORT LAYERS utilities/gas")),
              "scope=layers layers=utilities/gas sublayers=yes matched=3 lines=1 completed=0 "
              "ignored=0");
    // Given its line again, W1 reads whole.
    session.ok("UNDO");
    EXPECT_EQ(firstLine(session.ok("UTILITY REPORT DRAWING")),
              "scope=drawing matched=21 lines=4 completed=0 ignored=0");
}

// ---- REGRADE -------------------------------------------------------------------------------

TEST(UtilityData, RegradeAfterAMovedPointDrawsWhatADrawOfTheScheduleSoEditedDraws)
{
    const ScratchDirectory scratch("moved");
    Session session;
    session.ok("UTILITY DRAW " + sample("schedule.csv"));
    // W1-5 3 m back towards the pothole: 9.502 m from W1-4, inside the 10 m
    // detected spacing, so W1-4 -> W1-5 is QL-B now, not QL-C.
    session.select({session.point("W1-5")});
    session.ok("MOVE -3,0");
    const std::map<EntityId, Entity> before = session.snapshot();
    const std::vector<std::string> beforeLayers = session.document.model().layers.names();
    const std::size_t steps = session.steps();

    const std::string reply = session.ok("UTILITY REGRADE DRAWING");
    EXPECT_EQ(firstLine(reply), "utilities regraded changed=1 lines=4 vertices=14 segments=10 "
                                "entities=21 layers=10 "
                                "bounds=334000.000,6250000.000,334040.000,6250007.200");
    EXPECT_TRUE(contains(reply, "\nscope=drawing matched=21 lines=4 completed=0 ignored=0\n"))
        << reply;
    EXPECT_TRUE(contains(reply, "\nline id=W1 type=water length=27.024 ql_a=1.420 ql_b=25.604 "
                                "ql_c=0.000 ql_d=0.000"))
        << reply;
    EXPECT_EQ(session.steps(), steps + 1);
    EXPECT_EQ(session.document.history().undoName(), "UTILITY REGRADE");

    // Exactly what a DRAW of the schedule with W1-5 there draws.
    const std::string edited = scratch.file(
        "moved.csv", replaced(sampleText("schedule.csv"), "W1,W1-5,334030.000,",
                              "W1,W1-5,334027.000,"));
    EXPECT_EQ(drawing(session.document), drawnFrom(edited));
    // The other services' entities are the same entities, untouched.
    const std::map<EntityId, Entity> after = session.snapshot();
    for (const auto& [id, entity] : before) {
        const auto line = entity.properties.find(keys::kLine);
        if (katana::entity::toString(line->second) != "W1") {
            ASSERT_TRUE(after.contains(id));
            EXPECT_EQ(after.at(id), entity);
        } else if (entity.type() == EntityType::Point) {
            EXPECT_TRUE(after.contains(id)) << "a point keeps its id";
        }
    }

    // One undo puts every entity back as it was, ids and all.
    session.ok("UNDO");
    EXPECT_EQ(session.snapshot(), before);
    EXPECT_EQ(session.document.model().layers.names(), beforeLayers);
    session.ok("REDO");
    EXPECT_EQ(drawing(session.document), drawnFrom(edited));
}

TEST(UtilityData, RegradeAfterAnEditedMethodDrawsWhatADrawOfTheScheduleSoEditedDraws)
{
    const ScratchDirectory scratch("method");
    Session session;
    session.ok("UTILITY DRAW " + sample("schedule.csv") + " MINCOVER 0.6");
    // W1-1 said now to be from the records: QL-D, and its segment with it.
    session.select({session.point("W1-1")});
    session.ok("PROP SET utility.method records");
    session.ok("UTILITY REGRADE LAYERS utilities/water MINCOVER 0.6");
    const std::string edited = scratch.file(
        "method.csv",
        replaced(sampleText("schedule.csv"), "W1,W1-1,334000.000,6250000.000,EML,",
                 "W1,W1-1,334000.000,6250000.000,records,"));
    EXPECT_EQ(drawing(session.document), drawnFrom(edited, " MINCOVER 0.6"));
    EXPECT_EQ(katana::entity::toString(session.document.model()
                                           .entities.find(session.point("W1-1"))
                                           ->properties.at(std::string(keys::kQualityLevel))),
              "QL-D");
}

TEST(UtilityData, RegradeFollowsAnEditedTypeToItsLayers)
{
    const ScratchDirectory scratch("type");
    Session session;
    session.ok("UTILITY DRAW " + sample("schedule.csv"));
    session.ok("MODIFY DRAWING WHERE TYPE=point PROP=utility.line:T1 SET "
               "PROP=utility.type:sewer");
    session.ok("UTILITY REGRADE DRAWING WHERE PROP=utility.line:T1");
    const std::string edited = scratch.file(
        "type.csv", replaced(sampleText("schedule.csv"), "assumed,,telecommunications,",
                             "assumed,,sewer,"));
    EXPECT_EQ(drawing(session.document), drawnFrom(edited));
    EXPECT_TRUE(session.document.model().layers.contains("utilities/sewer/QL-C"));
    EXPECT_TRUE(session.document.model().layers.contains("utilities/sewer/points"));
}

TEST(UtilityData, ARunGivenAnArcIsStillItsLinesRunAndARegradeDrawsItAgain)
{
    // A run with an arc is stored as a curve polyline (docs/drawing.md,
    // "Which kind a polyline is"). Still its line's run: were it not, a
    // regrade would draw the line's runs again beside it.
    Session session;
    session.ok("UTILITY DRAW " + sample("schedule.csv"));
    EntityId run = 0;
    session.document.model().entities.forEach([&run](const Entity& entity) {
        const auto line = entity.properties.find(std::string(keys::kLine));
        if (run == 0 && entity.type() == EntityType::Polyline && line != entity.properties.end() &&
            katana::entity::toString(line->second) == "W1") {
            run = entity.id;
        }
    });
    ASSERT_NE(run, 0u);
    session.ok("VERTEX SET " + std::to_string(run) + " 0 bulge=0.1");
    ASSERT_EQ(session.document.model().entities.find(run)->type(), EntityType::CurvePolyline);

    session.ok("UTILITY REGRADE DRAWING");
    EXPECT_EQ(drawing(session.document), drawnFrom(sample("schedule.csv")))
        << "the arced run was left beside the runs drawn again";
    EXPECT_EQ(session.document.model().entities.find(run), nullptr);
}

TEST(UtilityData, RegradeOfWhatNobodyEditedChangesNothingAndPushesNoStep)
{
    Session session;
    session.ok("UTILITY DRAW " + sample("schedule.csv"));
    const std::map<EntityId, Entity> before = session.snapshot();
    const std::string reply = session.ok("UTILITY REGRADE DRAWING");
    EXPECT_TRUE(reply.starts_with("utilities regraded changed=0 lines=4 vertices=14 segments=10 "
                                  "entities=21 layers=11 "))
        << reply;
    EXPECT_EQ(session.steps(), 1u);
    EXPECT_EQ(session.snapshot(), before);
    // The bounds a front end frames are read from a regrade as from a draw.
    const auto bounds = drawReplyBounds(reply);
    ASSERT_TRUE(bounds.has_value());
    EXPECT_DOUBLE_EQ(bounds->max.y, 6250007.2);
}

TEST(UtilityData, RegradeGradesEachLineWithTheSettingsItWasDrawnWithUnlessToldOtherwise)
{
    // Unedited, whatever it was drawn with: nothing to regrade - no cover
    // flag stripped, no run graded at the default spacing instead.
    for (const char* options : {" MINCOVER 0.6", " SPACING 20", " SPACING 20 MINCOVER 0.6"}) {
        Session session;
        session.ok("UTILITY DRAW " + sample("schedule.csv") + options);
        const std::map<EntityId, Entity> before = session.snapshot();
        const std::string reply = session.ok("UTILITY REGRADE DRAWING");
        EXPECT_TRUE(reply.starts_with("utilities regraded changed=0 ")) << options << "\n"
                                                                        << reply;
        EXPECT_EQ(session.snapshot(), before) << options;
        EXPECT_EQ(session.steps(), 1u) << options;
    }

    // What the points say they were graded with.
    const ScratchDirectory scratch("settings");
    Session session;
    session.ok("UTILITY DRAW " + sample("schedule.csv") + " SPACING 20");
    const Entity& drawnPoint = *session.document.model().entities.find(session.point("W1-5"));
    EXPECT_EQ(katana::entity::toString(drawnPoint.properties.at(std::string(keys::kSpacing))),
              katana::entity::toString(katana::entity::PropertyValue(20.0)));
    EXPECT_FALSE(drawnPoint.properties.contains(std::string(keys::kMinimumCover)));

    // One point moved: its line alone changes, graded at the 20 m it was
    // drawn with - what a DRAW of the schedule so edited at 20 m draws.
    session.select({session.point("W1-5")});
    session.ok("MOVE -3,0");
    EXPECT_TRUE(session.ok("UTILITY REGRADE DRAWING").starts_with("utilities regraded changed=1 "));
    const std::string edited = scratch.file(
        "moved.csv",
        replaced(sampleText("schedule.csv"), "W1,W1-5,334030.000,", "W1,W1-5,334027.000,"));
    EXPECT_EQ(drawing(session.document), drawnFrom(edited, " SPACING 20"));

    // Told otherwise, every line in scope takes what it is told - and keeps it.
    session.ok("UTILITY REGRADE DRAWING SPACING 10");
    EXPECT_EQ(drawing(session.document), drawnFrom(edited, " SPACING 10"));
    EXPECT_TRUE(session.ok("UTILITY REGRADE DRAWING").starts_with("utilities regraded changed=0 "));

    // Points of one line that disagree on them are refused, as on any
    // attribute of the line.
    session.select({session.point("W1-2")});
    session.ok("PROP SET utility.spacing 15 real");
    EXPECT_TRUE(contains(session.refused("UTILITY REGRADE DRAWING").message,
                         "has utility.spacing \"15"))
        << session.refused("UTILITY REGRADE DRAWING").message;
}

TEST(UtilityData, RegradeKeepsWhatIsThePersonsOnAPoint)
{
    Session session;
    session.ok("UTILITY DRAW " + sample("schedule.csv"));
    const EntityId moved = session.point("W1-5");
    session.select({moved});
    session.ok("PROP SET note \"check with the owner\"");
    session.ok("PROP SET utility.note mine");
    session.ok("COLOR #FF00FF");
    session.ok("MOVE -3,0");
    session.ok("UTILITY REGRADE SELECTION");
    const Entity& point = *session.document.model().entities.find(moved);
    EXPECT_EQ(katana::entity::toString(point.properties.at("note")), "check with the owner");
    EXPECT_EQ(katana::entity::toString(point.properties.at("utility.note")), "mine");
    EXPECT_EQ(point.color, std::optional(katana::entity::Color{255, 0, 255, 255}));
    EXPECT_EQ(point.layer, "utilities/water/points");
}

TEST(UtilityData, RegradeIsOneStepAndAllOrNothing)
{
    Session session;
    session.ok("UTILITY DRAW " + sample("schedule.csv"));
    session.select({session.point("W1-5")});
    session.ok("MOVE -3,0");
    // The QL-C run the regrade must take away is on a locked layer.
    session.ok("LAYER LOCK utilities/water/QL-C");
    const std::map<EntityId, Entity> before = session.snapshot();
    const std::vector<std::string> layers = session.document.model().layers.names();
    const std::size_t steps = session.steps();
    const katana::core::Error locked = session.refused("UTILITY REGRADE DRAWING");
    EXPECT_TRUE(contains(locked.describe(), "locked")) << locked.describe();
    EXPECT_EQ(session.snapshot(), before);
    EXPECT_EQ(session.document.model().layers.names(), layers);
    EXPECT_EQ(session.steps(), steps);

    // A line that does not read refuses the whole regrade by its name.
    session.ok("LAYER UNLOCK utilities/water/QL-C");
    session.select({session.point("E1-2")});
    session.ok("PROP DELETE utility.order");
    const std::size_t unlockedSteps = session.steps();
    const katana::core::Error unread = session.refused("UTILITY REGRADE DRAWING");
    EXPECT_TRUE(contains(unread.message, "line E1: point E1-2")) << unread.message;
    EXPECT_EQ(session.steps(), unlockedSteps);
}

TEST(UtilityData, TheVerbsRefuseASourceTheyDoNotTake)
{
    Session session;
    const auto refusal = [&session](const std::string& line) {
        return session.refused(line).describe();
    };
    EXPECT_TRUE(contains(refusal("UTILITY REGRADE " + sample("schedule.csv")),
                         "usage: UTILITY REGRADE <scope>"));
    EXPECT_TRUE(contains(refusal("UTILITY REGRADE"), "usage: UTILITY REGRADE <scope>"));
    EXPECT_TRUE(contains(refusal("UTILITY SCHEDULE out.csv " + sample("schedule.csv")),
                         "usage: UTILITY SCHEDULE <out.csv> <scope>"));
    EXPECT_TRUE(contains(refusal("UTILITY SCHEDULE out.csv DRAWING SCHEMA"),
                         "usage: UTILITY SCHEDULE"));
    EXPECT_TRUE(contains(refusal("UTILITY REPORT DRAWING WHERE SHADE=blue"), "not a WHERE key"));
    EXPECT_TRUE(contains(refusal("UTILITY REPORT LAYERS nowhere"), "layer does not exist"));
    EXPECT_TRUE(contains(refusal("UTILITY VERIFY DRAWING extra"), "usage: UTILITY VERIFY"));
    EXPECT_TRUE(contains(refusal("UTILITY CHECK DRAWING"), "usage: UTILITY CHECK"));
    EXPECT_EQ(session.steps(), 0u);
}

// ---- CHECK and SCHEDULE in a client's words --------------------------------------------------

TEST(UtilityData, TheDrawingIsCheckedAsTheScheduleItWouldWriteInTheSchemasWords)
{
    const ScratchDirectory scratch("check");
    const std::string withRadar = scratch.file("schema.csv", schema(true));
    const std::string withoutRadar = scratch.file("no radar.csv", schema(false));
    Session session;
    // The file says E-0001 is "In service" on both its rows, which the
    // schema lists as "In Service": a warning each.
    const std::string fromFile =
        session.ok("UTILITY CHECK " + sample("schedule_tfnsw.csv") + " SCHEMA " + withRadar);
    EXPECT_TRUE(contains(fromFile, "7 rows, 3 assets (AssetIdentifier): 0 errors, 2 warnings"))
        << fromFile;

    // The drawing holds the status, not a spelling of it, and writes it as
    // the schema lists it.
    session.ok("UTILITY DRAW " + sample("schedule_tfnsw.csv"));
    const std::string fromDrawing = session.ok("UTILITY CHECK DRAWING SCHEMA " + withRadar);
    // Seven points and a run each: W-0001's two detected stretches are both
    // QL-B, E-0001's 18 m is past the detected spacing, G-0001 is records.
    EXPECT_EQ(firstLine(fromDrawing), "scope=drawing matched=10 lines=3 completed=0 ignored=0");
    EXPECT_TRUE(contains(fromDrawing, "7 rows, 3 assets (AssetIdentifier): 0 errors, 0 warnings"))
        << fromDrawing;

    // A method the schema has no spelling for is written in Katana's own, for
    // the check to find, and the check refuses with all of itself.
    const katana::core::Error refused =
        session.refused("UTILITY CHECK LAYERS utilities/electricity SCHEMA " + withoutRadar);
    EXPECT_TRUE(refused.message.starts_with("the schedule does not meet the schema: 2 errors\n"
                                            "scope=layers layers=utilities/electricity"))
        << refused.message;
    EXPECT_TRUE(contains(refused.message, "\"ground penetrating radar\" is not in the domain"))
        << refused.message;

    // SCHEDULE with the schema writes that same deliverable, and it reads
    // back as the lines drawn.
    const std::string out = scratch.at("deliverable.csv");
    session.ok("UTILITY SCHEDULE \"" + out + "\" DRAWING SCHEMA " + withRadar);
    const std::string written = scratch.read("deliverable.csv");
    const std::string header = firstLine(written);
    for (const char* column : {"AssetIdentifier", "LocateMethod", "QualityLevel", "AssetStatus"}) {
        EXPECT_TRUE(contains(header, column)) << header;
    }
    EXPECT_TRUE(contains(written, "In Service")) << written;
    EXPECT_TRUE(contains(written, "Electronic Detection")) << written;
    EXPECT_TRUE(contains(written, "Quality Level B")) << written;
    EXPECT_EQ(parsed(written), parsed(sampleText("schedule_tfnsw.csv")));
}

TEST(UtilityData, WhatTheScheduleSaidItDidNotKnowIsKeptSoTheDrawingMeetsTheSameSchema)
{
    // Every one of these reads as "not recorded": the drawing must still say
    // it, or a schedule that met the schema writes a deliverable that does not.
    const ScratchDirectory scratch("unknowns");
    const std::string schemaFile = scratch.file(
        "schema.csv", "kind,attribute,value,detail,label\n"
                      "schema,Example Utility Schema,0.2,,\n"
                      "identifier,AssetIdentifier,,,\n"
                      "field,AssetIdentifier,Alphanumerical,Yes,Asset Identifier\n"
                      "field,LocateMethod,Domain List: Locate Method,Yes,Locate Method\n"
                      "domain,LocateMethod,Electronic Detection,,\n"
                      "field,QualityLevel,Domain List: Quality Level,Yes,Quality Level\n"
                      "domain,QualityLevel,Quality Level B,,\n"
                      "domain,QualityLevel,Unknown,,\n"
                      "field,Size,Alphanumerical,Yes,Size\n"
                      "field,AssetStatus,Domain List: Asset Status,Yes,Asset Status\n"
                      "domain,AssetStatus,In Service,,\n"
                      "domain,AssetStatus,Unknown,,\n"
                      "field,AssetTypeCode,Domain List: Asset Type Code,Yes,Asset Type Code\n"
                      "domain,AssetTypeCode,W,,\n"
                      "domain,AssetTypeCode,N,,\n"
                      "field,DepthLocation,Domain List: Depth Location,Yes,Depth Location\n"
                      "domain,DepthLocation,Top of Pipe,,\n"
                      "domain,DepthLocation,Unknown,,\n");
    const std::string scheduleText =
        "AssetIdentifier,point,easting,northing,LocateMethod,QualityLevel,Size,AssetStatus,"
        "AssetTypeCode,DepthLocation\n"
        "W-1,P1,334000,6250000,Electronic Detection,Unknown,Not Applicable,Unknown,N,Top of Pipe\n"
        "W-1,P2,334010,6250000,Electronic Detection,Unknown,Not Applicable,Unknown,N,Top of Pipe\n";
    const std::string scheduleFile = scratch.file("schedule.csv", scheduleText);
    Session session;
    const std::string passes = "2 rows, 1 assets (AssetIdentifier): 0 errors, 0 warnings";
    const std::string fromFile =
        session.ok("UTILITY CHECK " + scheduleFile + " SCHEMA " + schemaFile);
    EXPECT_TRUE(contains(fromFile, passes)) << fromFile;

    session.ok("UTILITY DRAW " + scheduleFile);
    const std::string fromDrawing = session.ok("UTILITY CHECK DRAWING SCHEMA " + schemaFile);
    EXPECT_TRUE(contains(fromDrawing, passes)) << fromDrawing;

    // The deliverable has every column the schema asks for, said as the
    // schedule said it, and reads back as the schedule read.
    session.ok("UTILITY SCHEDULE \"" + scratch.at("out.csv") + "\" DRAWING SCHEMA " + schemaFile);
    const std::string written = scratch.read("out.csv");
    EXPECT_EQ(firstLine(written), "AssetIdentifier,point,easting,northing,LocateMethod,"
                                  "DepthLocation,QualityLevel,AssetTypeCode,Size,AssetStatus");
    EXPECT_TRUE(contains(written, "\nW-1,P1,334000,6250000,Electronic Detection,Top of Pipe,"
                                  "Unknown,N,Not Applicable,Unknown\n"))
        << written;
    EXPECT_EQ(parsed(written), parsed(scheduleText));
}

// ---- the design CLEARANCE measures against ----------------------------------------------------

TEST(UtilityData, AnEntityDrawnAsTheDesignIsTheDesignFilesCentreLine)
{
    // design.csv's two vertices, drawn: a polyline with the heights a 3D
    // string carries.
    Entity drawn;
    drawn.id = 7;
    drawn.geometry = katana::geometry::Polyline2{
        {Point2(334015.0, 6249995.0), Point2(334015.5, 6250010.0)}, false};
    katana::entity::setHeights(drawn.properties, {18.60, 18.45});
    const auto design = designFromEntity(drawn, std::nullopt);
    ASSERT_TRUE(design.ok()) << design.error().describe();
    const auto file = sub::parseDesignCsv(sampleText("design.csv"), "#7");
    ASSERT_TRUE(file.ok());
    EXPECT_EQ(design->id, "#7");
    ASSERT_EQ(design->vertices.size(), 2u);
    for (std::size_t i = 0; i < 2; ++i) {
        EXPECT_EQ(design->vertices[i].position, file->vertices[i].position);
        EXPECT_EQ(design->vertices[i].level, file->vertices[i].level);
    }
    // LEVEL is every vertex's; with neither there are none.
    const auto level = designFromEntity(drawn, 17.5);
    EXPECT_EQ(level->vertices[1].level, std::optional(17.5));
    drawn.properties.clear();
    EXPECT_FALSE(designFromEntity(drawn, std::nullopt)->vertices[0].level.has_value());

    // A closed polyline returns to its start; a line is its two ends.
    drawn.geometry = katana::geometry::Polyline2{
        {Point2(0, 0), Point2(10, 0), Point2(10, 10)}, true};
    const auto ring = designFromEntity(drawn, std::nullopt);
    ASSERT_EQ(ring->vertices.size(), 4u);
    EXPECT_EQ(ring->vertices.back().position, ring->vertices.front().position);
    drawn.geometry = katana::geometry::Segment2{Point2(0, 0), Point2(3, 4)};
    EXPECT_EQ(designFromEntity(drawn, 1.0)->vertices.size(), 2u);
    // Nothing else is a centre line.
    drawn.geometry = katana::geometry::Circle2{Point2(0, 0), 5.0};
    const auto circle = designFromEntity(drawn, std::nullopt);
    ASSERT_FALSE(circle.ok());
    // What is accepted, named: a curve polyline too, since the drawing
    // system (docs/drawing.md, "The merge into main").
    EXPECT_EQ(circle.error().message, "entity #7 is circle; the design centre line is a line, a "
                                      "polyline or a curve polyline");
}

TEST(UtilityData, ClearanceTakesTheDesignFromAnEntityOrAnAlignment)
{
    Session session;
    const std::string fileRow = "W1 W1-2 -> W1-3 +QL-B +within tolerance +-0\\.263 +0\\.300";
    const std::string fromFile =
        session.ok("UTILITY CLEARANCE " + sample("schedule.csv") + " " + sample("design.csv") +
                   " WIDTH 0.375");
    ASSERT_TRUE(std::regex_search(fromFile, std::regex(fileRow))) << fromFile;

    // The centre line drawn, at one level.
    session.ok("PLINE 334015,6249995 334015.5,6250010");
    const EntityId pipe = session.document.lastCreatedEntities().front();
    const std::string drawnDesign = session.ok("UTILITY CLEARANCE " + sample("schedule.csv") +
                                               " DESIGN #" + std::to_string(pipe) +
                                               " LEVEL 18.6 WIDTH 0.375");
    EXPECT_TRUE(std::regex_search(drawnDesign, std::regex(fileRow))) << drawnDesign;

    // The same line as an alignment, its design profile falling 0.15 m over
    // it as design.csv's levels do.
    katana::entity::Alignment alignment;
    alignment.name = "Storm 1";
    alignment.horizontal.pis = {{Point2(334015.0, 6249995.0)}, {Point2(334015.5, 6250010.0)}};
    const auto solved = katana::geometry::solveAlignment(alignment.horizontal);
    ASSERT_TRUE(solved.ok());
    alignment.vertical = katana::geometry::VerticalAlignment{
        {{0.0, 18.60, 0.0}, {solved->length(), 18.45, 0.0}}};
    ASSERT_TRUE(session.document.execute(katana::commands::createAlignment(alignment)).ok());
    const std::string alongAlignment = session.ok(
        "UTILITY CLEARANCE " + sample("schedule.csv") + " DESIGN ALIGNMENT \"Storm 1\" WIDTH 0.375");
    EXPECT_TRUE(std::regex_search(alongAlignment, std::regex(fileRow + " +0\\.372")))
        << alongAlignment;
    const auto design = designFromAlignment(alignment);
    ASSERT_TRUE(design.ok());
    const auto file = sub::parseDesignCsv(sampleText("design.csv"), "Storm 1");
    ASSERT_EQ(design->vertices.size(), 2u);
    for (std::size_t i = 0; i < 2; ++i) {
        EXPECT_NEAR(design->vertices[i].position.easting, file->vertices[i].position.easting, 1e-9);
        EXPECT_NEAR(design->vertices[i].position.northing, file->vertices[i].position.northing,
                    1e-9);
        EXPECT_NEAR(*design->vertices[i].level, *file->vertices[i].level, 1e-9);
    }

    const auto refusal = [&session](const std::string& line) {
        return session.refused(line).describe();
    };
    EXPECT_TRUE(contains(refusal("UTILITY CLEARANCE " + sample("schedule.csv") + " " +
                                 sample("design.csv") + " LEVEL 18"),
                         "LEVEL is the level of a design drawn as an entity"));
    EXPECT_TRUE(contains(refusal("UTILITY CLEARANCE " + sample("schedule.csv") + " DESIGN #99999"),
                         "no entity #99999"));
    EXPECT_TRUE(contains(refusal("UTILITY CLEARANCE " + sample("schedule.csv") +
                                 " DESIGN ALIGNMENT nowhere"),
                         "no alignment named nowhere"));
    EXPECT_TRUE(contains(refusal("UTILITY CLEARANCE " + sample("schedule.csv") + " DESIGN"),
                         "usage: UTILITY CLEARANCE"));
    EXPECT_TRUE(contains(refusal("UTILITY CLEARANCE " + sample("schedule.csv") + " DESIGN #x"),
                         "a design entity is #<entity id>"));
    EXPECT_TRUE(contains(refusal("UTILITY CLEARANCE " + sample("schedule.csv") + " DESIGN #" +
                                 std::to_string(pipe) + " LEVEL"),
                         "LEVEL needs a level"));
}

TEST(UtilityData, AnAlignmentsDesignIsWithinAMillimetreOfItsCurvesInPlanAndInLevel)
{
    // A tangent, a 50 m curve and a tangent in plan; in level, a 60 m crest
    // curve between grades of +4 % and -3 %.
    katana::entity::Alignment alignment;
    alignment.name = "Road";
    alignment.horizontal.pis = {{Point2(0, 0)}, {Point2(100, 0), 50.0}, {Point2(100, 100)}};
    alignment.vertical = katana::geometry::VerticalAlignment{
        {{0.0, 10.0, 0.0}, {80.0, 13.2, 60.0}, {170.0, 10.5, 0.0}}};
    const auto solved = katana::geometry::solveAlignment(alignment.horizontal);
    const auto profile = katana::geometry::solveProfile(*alignment.vertical);
    ASSERT_TRUE(solved.ok() && profile.ok());
    const auto stations = designStations(alignment);
    const auto design = designFromAlignment(alignment);
    ASSERT_TRUE(stations.ok() && design.ok());
    ASSERT_EQ(stations->size(), design->vertices.size());
    EXPECT_DOUBLE_EQ(stations->front(), solved->startStation());
    EXPECT_DOUBLE_EQ(stations->back(), solved->endStation());

    // Each chord's middle against the curve at the middle station: the
    // sagitta in plan, and the parabola's departure in level - both largest
    // there.
    std::size_t onTheArc = 0;
    std::size_t inTheCurve = 0;
    for (std::size_t i = 0; i + 1 < stations->size(); ++i) {
        const double middle = 0.5 * ((*stations)[i] + (*stations)[i + 1]);
        const auto& a = design->vertices[i];
        const auto& b = design->vertices[i + 1];
        const Point2 chord(0.5 * (a.position.easting + b.position.easting),
                           0.5 * (a.position.northing + b.position.northing));
        const Point2 curve = *solved->pointAtStation(middle);
        EXPECT_LE((chord - curve).length(), kDesignChordTolerance * 1.000001) << middle;
        const auto level = profile->elevationAt(middle);
        if (level && a.level && b.level) {
            EXPECT_LE(std::abs(0.5 * (*a.level + *b.level) - *level),
                      kDesignChordTolerance * 1.000001)
                << middle;
        }
        const auto* element = solved->elementAt(middle);
        onTheArc += katana::geometry::kindOf(*element) == katana::geometry::AlignmentElementKind::Arc;
        const auto* vertical = profile->elementAt(middle);
        inTheCurve += vertical && vertical->kind == katana::geometry::ProfileElementKind::Curve;
    }
    // The checks reached the cases they are for.
    EXPECT_GT(onTheArc, 10u);
    EXPECT_GT(inTheCurve, 10u);
    // The profile ends at 170 m, short of the alignment's end: no level there.
    EXPECT_FALSE(design->vertices.back().level.has_value());
    EXPECT_TRUE(design->vertices.front().level.has_value());
}

TEST(UtilityData, AnAlignmentsDesignEndsAtItsEndWhenItsLengthsSumARoundingPastIt)
{
    // A tangent, a tight curve and a 140 m tangent from chainage 43.45: the
    // last element's end, summed element by element from the start, comes out
    // one rounding past endStation(), start plus length. The design must still
    // run to the alignment's last PI - dropping that end dropped the whole
    // last tangent, and a service crossing it cleared.
    katana::entity::Alignment alignment;
    alignment.name = "road";
    alignment.horizontal.pis = {{Point2(333950, 6250050)},
                                {Point2(333960, 6250000.3), 4.5958},
                                {Point2(334100, 6250000.3)}};
    alignment.horizontal.startStation = 43.45;
    const auto solved = katana::geometry::solveAlignment(alignment.horizontal);
    ASSERT_TRUE(solved.ok());
    const auto& last = solved->elements().back();
    // The test reaches the case it is for.
    ASSERT_GT(last.startStation + last.length, solved->endStation());

    const auto stations = designStations(alignment);
    const auto design = designFromAlignment(alignment);
    ASSERT_TRUE(stations.ok() && design.ok());
    ASSERT_EQ(stations->size(), design->vertices.size());
    EXPECT_EQ(stations->back(), solved->endStation());
    EXPECT_NEAR(design->vertices.back().position.easting, 334100.0, 1e-6);
    EXPECT_NEAR(design->vertices.back().position.northing, 6250000.3, 1e-6);
    EXPECT_NEAR(design->vertices.front().position.easting, 333950.0, 1e-6);

    // The drawn water main crosses that tangent at W1-3 -> W1-4: a hard
    // conflict, whatever the chainage the alignment starts from.
    Session session;
    session.ok("UTILITY DRAW " + sample("schedule.csv"));
    ASSERT_TRUE(session.document.execute(katana::commands::createAlignment(alignment)).ok());
    const std::string reply = session.ok("UTILITY CLEARANCE DRAWING DESIGN ALIGNMENT road");
    EXPECT_TRUE(contains(reply, "Suggested Clash attribute: W1 Hard")) << reply;
}

TEST(UtilityData, HelpUtilityListsTheDrawingSourcesAndTheNewVerbs)
{
    const std::string help = utilityVerbHelp();
    for (const char* word : {"UTILITY REGRADE <scope>", "UTILITY SCHEDULE <out.csv> <scope>",
                             "<schedule.csv> | <scope>", "DESIGN", "#<id>", "LEVEL <z>",
                             "ALIGNMENT <name>", "AREA x0,y0,x1,y1", "VIEW [id]", "WHERE",
                             "completed=", "ignored="}) {
        EXPECT_TRUE(contains(help, word)) << word;
    }
    const std::string general = CommandInterpreter::helpText();
    for (const char* word : {"VIEW [id]", "AREA x0,y0,x1,y1", "REGRADE", "SCHEDULE"}) {
        EXPECT_TRUE(contains(general, word)) << word;
    }
}
