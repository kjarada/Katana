// The leader verbs take the one scope and filter grammar (CLAUDE.md 1.1;
// docs/annotation.md, "Scope and filter"): LEADER LIST, VALUES, SET, FREEZE,
// DETACH, PROP, ALIGN and FOR and BALLOON FOR and RENUMBER act on ids, or on
// what [SELECTION | VIEW | DRAWING | AREA | LAYERS] [WHERE ...] takes, and say
// how many it took. A surface or an alignment a note reads follows its
// rename and is counted when it goes (the leaders' reading by name, L3), and
// the JSON writer's guard against a byte that is not UTF-8 is reached with a
// plain-data report (L4).
//
// Every expected value is worked out by hand in a comment beside it.

#include <gtest/gtest.h>

#include <algorithm>
#include <memory>
#include <optional>
#include <sstream>
#include <string>
#include <vector>

#include "katana/cad/annotation/command_words.hpp"
#include "katana/cad/annotation/leader_edit.hpp"
#include "katana/cad/annotation/leader_presets.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/label_text.hpp"
#include "katana/entity/leader_values.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_surface.hpp"

using namespace katana::cad;
using namespace katana::entity;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::Point3;
namespace ann = katana::cad::annotation;
namespace cmd = katana::commands;
namespace terrain = katana::terrain;

namespace {

struct Session {
    Document document;
    CommandInterpreter interpreter{document};

    std::string run(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << "\n  -> " << reply.error().describe();
        return reply.ok() ? *reply : std::string();
    }
    // The refusal's words, or "" when it was not refused.
    std::string refusal(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        EXPECT_FALSE(reply.ok()) << line << " was not refused: " << *reply;
        return reply.ok() ? std::string() : reply.error().describe();
    }
    // A point on `layer` (which exists) with its properties.
    EntityId point(Point2 at, const std::string& layer, PropertyMap properties = {})
    {
        Entity entity;
        entity.geometry = PointGeometry{at};
        entity.layer = layer;
        entity.properties = std::move(properties);
        EXPECT_TRUE(document.execute(cmd::createEntities({std::move(entity)})).ok());
        return document.lastCreatedEntities().front();
    }
    // Runs a line that makes one entity and says its id.
    EntityId made(const std::string& line)
    {
        run(line);
        return document.lastCreatedEntities().front();
    }
    [[nodiscard]] const LeaderGeometry& leader(EntityId id) const
    {
        return std::get<LeaderGeometry>(document.model().entities.find(id)->geometry);
    }
    [[nodiscard]] std::string says(EntityId id) const
    {
        return ann::leaderSays(document.model(), leader(id), ann::leaderContext(document));
    }
    [[nodiscard]] std::size_t steps() const { return document.history().undoCount(); }
};

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

std::string id(EntityId value)
{
    return std::to_string(value);
}

std::string firstLine(const std::string& reply)
{
    return reply.substr(0, reply.find('\n'));
}

// The ids of the "id=N ..." lines of a LEADER LIST reply, in the order given.
std::vector<EntityId> listedIds(const std::string& reply)
{
    std::vector<EntityId> ids;
    std::istringstream lines(reply);
    for (std::string line; std::getline(lines, line);) {
        if (line.rfind("id=", 0) == 0) {
            ids.push_back(std::stoull(line.substr(3)));
        }
    }
    return ids;
}

std::size_t countLinesStarting(const std::string& reply, const std::string& prefix)
{
    std::size_t count = 0;
    std::istringstream lines(reply);
    for (std::string line; std::getline(lines, line);) {
        count += line.rfind(prefix, 0) == 0 ? 1 : 0;
    }
    return count;
}

// Two pits and five leaders on three layers:
//
//   layer 0      p1 (0, 0) invert 1.5       p2 (50, 50) invert 2.5
//   layer notes  a1 smart, tip on p1, note hangs at (5, 5)
//                a2 smart, tip on p2, note hangs at (55, 55)
//                a4 smart, tip on p1, note hangs at (6, 6)
//                b1 plain, tip on nothing, (100, 100) to (110, 110)
//   layer other  a3 smart, tip on p1, note hangs at (-5, -5)
//
// Every smart note is "IL {prop.invert:.2f}": 1.50 on p1, 2.50 on p2.
struct Pits : Session {
    EntityId p1 = 0, p2 = 0, a1 = 0, a2 = 0, a3 = 0, a4 = 0, b1 = 0;

    Pits()
    {
        run("LAYER NEW notes");
        run("LAYER NEW other");
        p1 = point(Point2(0, 0), "0", {{"invert", 1.5}});
        p2 = point(Point2(50, 50), "0", {{"invert", 2.5}});
        run("LAYER SET notes");
        const std::string note = " template=\"IL {prop.invert:.2f}\"";
        a1 = made("LEADER #" + id(p1) + " 5,5" + note);
        a2 = made("LEADER #" + id(p2) + " 55,55" + note);
        a4 = made("LEADER #" + id(p1) + " 6,6" + note);
        b1 = made("LEADER 100,100 110,110 text=plain");
        run("LAYER SET other");
        a3 = made("LEADER #" + id(p1) + " -5,-5" + note);
        run("LAYER SET 0");
    }
};

} // namespace

// ---- reading: LIST and VALUES ---------------------------------------------------------------

TEST(LeaderScope, ListTakesTheLeadersOfALayerAndSaysHowManyMatched)
{
    Pits s;
    const std::string reply = s.run("LEADER LIST LAYERS notes");
    // notes holds a1, a2, a4 and b1: four. The other layer's a3 is not.
    EXPECT_EQ(firstLine(reply), "scope=layers layers=notes sublayers=yes matched=4") << reply;
    EXPECT_EQ(listedIds(reply), (std::vector<EntityId>{s.a1, s.a2, s.a4, s.b1})) << reply;
    // The whole drawing: five leaders, and the points are not among them.
    const std::string all = s.run("LEADER LIST DRAWING");
    EXPECT_EQ(firstLine(all), "scope=drawing matched=5") << all;
    EXPECT_EQ(listedIds(all).size(), 5U);
    // Ids still name them, and carry no scope record.
    const std::string named = s.run("LEADER LIST " + id(s.a2) + " " + id(s.a3));
    EXPECT_EQ(listedIds(named), (std::vector<EntityId>{s.a2, s.a3})) << named;
    EXPECT_EQ(named.find("scope="), std::string::npos) << named;
    // And a bare LIST is every leader, as it always was.
    EXPECT_EQ(listedIds(s.run("LEADER LIST")).size(), 5U);
}

TEST(LeaderScope, ListTakesAWhereFilterAndAnAreaAndTheSelection)
{
    Pits s;
    // Only the smart notes on notes: WHERE LAYER= is the filter's, on the
    // leader's own layer.
    const std::string onNotes = s.run("LEADER LIST DRAWING WHERE LAYER=notes");
    EXPECT_EQ(listedIds(onNotes), (std::vector<EntityId>{s.a1, s.a2, s.a4, s.b1})) << onNotes;
    EXPECT_TRUE(contains(firstLine(onNotes), "matched=4")) << onNotes;
    // AREA -1,-1,30,30 overlaps the boxes of a1 (0,0)-(5,5), a4 (0,0)-(6,6)
    // and a3 (-5,-5)-(0,0), and not a2 (50,50)-(55,55) or b1 (100,100)-
    // (110,110). Listed in id order: a3 was made after a4.
    const std::string area = s.run("LEADER LIST AREA -1,-1,30,30");
    EXPECT_EQ(firstLine(area), "scope=area area=-1,-1,30,30 matched=3") << area;
    EXPECT_EQ(listedIds(area), (std::vector<EntityId>{s.a1, s.a4, s.a3})) << area;
    // The selection: only the leader selected.
    s.run("SELECT " + id(s.a2));
    const std::string selected = s.run("LEADER LIST SELECTION");
    EXPECT_EQ(firstLine(selected), "scope=selection matched=1") << selected;
    EXPECT_EQ(listedIds(selected), (std::vector<EntityId>{s.a2}));
    // The selection of a point, which is no leader, takes no leader: said,
    // not refused.
    s.run("SELECT " + id(s.p1));
    const std::string none = s.run("LEADER LIST SELECTION");
    EXPECT_EQ(none, "scope=selection matched=0");
}

TEST(LeaderScope, AScopeThatTakesNoLeaderListsNoneAndNeverAll)
{
    Pits s;
    // The trap: a list of ids that is empty means "all" to leadersJson, and a
    // scope that took nothing must not become that.
    const std::string text = s.run("LEADER LIST DRAWING WHERE TYPE=point");
    EXPECT_TRUE(contains(text, "matched=0")) << text;
    EXPECT_TRUE(listedIds(text).empty()) << text;
    const std::string json = s.run("LEADER LIST DRAWING WHERE TYPE=point JSON");
    EXPECT_TRUE(contains(json, "\"leaders\": []")) << json;
    EXPECT_TRUE(contains(json, "\"matched\": 0")) << json;
    // JSON before or after the scope reads the same, and says the scope.
    const std::string after = s.run("LEADER LIST LAYERS notes JSON");
    const std::string before = s.run("LEADER LIST JSON LAYERS notes");
    EXPECT_EQ(after, before);
    EXPECT_TRUE(contains(after, "\"matched\": 4")) << after;
    EXPECT_TRUE(contains(after, "\"record\": \"scope=layers layers=notes sublayers=yes matched=4\""))
        << after;
    // Four leaders, each with an "id" member; the presets have "name".
    std::size_t ids = 0;
    for (std::size_t at = after.find("\"id\":"); at != std::string::npos;
         at = after.find("\"id\":", at + 1)) {
        ++ids;
    }
    EXPECT_EQ(ids, 4U);
    // With no scope the JSON has none.
    EXPECT_EQ(s.run("LEADER LIST JSON").find("\"scope\""), std::string::npos);
}

TEST(LeaderScope, IdsAndAScopeTogetherAreRefusedAndNothingIsGuessed)
{
    Pits s;
    EXPECT_TRUE(contains(s.refusal("LEADER LIST " + id(s.a1) + " DRAWING"), "by id or by a scope"));
    EXPECT_TRUE(contains(s.refusal("LEADER FREEZE " + id(s.a1) + " LAYERS notes"),
                         "by id or by a scope"));
    EXPECT_TRUE(contains(s.refusal("LEADER FOR " + id(s.p1) + " DRAWING"), "by id or by a scope"));
    // Two scopes, a layer that is not there, a view with no window.
    EXPECT_TRUE(contains(s.refusal("LEADER LIST DRAWING SELECTION"), "one scope"));
    EXPECT_TRUE(contains(s.refusal("LEADER LIST LAYERS nowhere"), "layer does not exist"));
    EXPECT_TRUE(contains(s.refusal("LEADER LIST VIEW"), "AREA"));
    // A word after the scope that is not the verb's.
    EXPECT_TRUE(contains(s.refusal("LEADER FREEZE DRAWING WHERE LAYER=notes banana"),
                         "banana"));
}

TEST(LeaderScope, ViewIsTheWindowsPlanViewWithItsOwnHiddenLayers)
{
    Pits s;
    s.interpreter.setScopeContext(
        [](std::optional<std::uint32_t>) -> katana::core::Result<ScopeView> {
            ScopeView view;
            view.id = 7;
            view.layers.hide("other");
            view.area = Box2(Point2(-1, -1), Point2(30, 30));
            return view;
        });
    // In the box: a1, a3 and a4 as above; the view hides "other", so not a3.
    const std::string reply = s.run("LEADER LIST VIEW");
    EXPECT_EQ(firstLine(reply), "scope=view view=7 area=-1,-1,30,30 matched=2") << reply;
    EXPECT_EQ(listedIds(reply), (std::vector<EntityId>{s.a1, s.a4})) << reply;
    // Without its area: everything it draws - a1, a2, a4, b1.
    const std::string extents = s.run("LEADER LIST VIEW EXTENTS");
    EXPECT_EQ(listedIds(extents), (std::vector<EntityId>{s.a1, s.a2, s.a4, s.b1})) << extents;
}

TEST(LeaderScope, ValuesListsTheBlockOfEachLeaderOnAnEntityAndCountsTheRest)
{
    Pits s;
    const std::string reply = s.run("LEADER VALUES LAYERS notes");
    // a1, a2 and a4 are on a pit; b1 is on nothing, so it has no values.
    EXPECT_EQ(firstLine(reply),
              "scope=layers layers=notes sublayers=yes matched=4 skipped=1")
        << reply;
    EXPECT_EQ(countLinesStarting(reply, "leader="), 3U) << reply;
    EXPECT_TRUE(contains(reply, "leader=" + id(s.a1) + " target=" + id(s.p1) + " type=Point"))
        << reply;
    EXPECT_TRUE(contains(reply, "leader=" + id(s.a2) + " target=" + id(s.p2) + " type=Point"))
        << reply;
    EXPECT_TRUE(contains(reply, "value=prop.invert")) << reply;
    // One leader by id is what it was: no scope record.
    const std::string one = s.run("LEADER VALUES " + id(s.a1));
    EXPECT_EQ(one.find("scope="), std::string::npos) << one;
    EXPECT_TRUE(contains(one, "leader=" + id(s.a1))) << one;
}

// ---- editing: SET, FREEZE, DETACH, PROP, ALIGN ------------------------------------------------

TEST(LeaderScope, SetChangesTheLeadersAScopeTakesInOneStepAndNoOthers)
{
    Pits s;
    const ArrowHead before = s.leader(s.a3).arrow;
    const std::size_t steps = s.steps();
    const std::string reply = s.run("LEADER SET LAYERS notes arrow=dot");
    EXPECT_EQ(firstLine(reply), "scope=layers layers=notes sublayers=yes matched=4") << reply;
    EXPECT_TRUE(contains(reply, "updated leaders=4")) << reply;
    for (const EntityId leader : {s.a1, s.a2, s.a4, s.b1}) {
        EXPECT_EQ(s.leader(leader).arrow, ArrowHead::Dot) << leader;
    }
    EXPECT_EQ(s.leader(s.a3).arrow, before) << "a3 is on another layer";
    EXPECT_EQ(s.steps(), steps + 1) << "four leaders, one step";
    s.run("UNDO");
    EXPECT_EQ(s.leader(s.a1).arrow, before);
}

TEST(LeaderScope, AWhereKeyAfterWhereIsTheConditionAndNotTheVerbsOwnOption)
{
    Pits s;
    const std::size_t steps = s.steps();
    // text= before WHERE would be the new note; after it, TEXT= is the
    // filter's - which only a text entity can pass - so nothing matches and
    // no leader's note changes.
    const std::string reply = s.run("LEADER SET DRAWING arrow=tick WHERE TEXT=zzz");
    EXPECT_TRUE(contains(reply, "matched=0")) << reply;
    EXPECT_TRUE(contains(reply, "updated leaders=0")) << reply;
    EXPECT_EQ(s.steps(), steps);
    EXPECT_EQ(s.says(s.a1), "IL 1.50");
    // The option BEFORE WHERE is the note: every leader on notes, "NEW".
    const std::string noted = s.run("LEADER SET DRAWING text=NEW WHERE LAYER=notes");
    EXPECT_TRUE(contains(noted, "updated leaders=4")) << noted;
    EXPECT_EQ(s.says(s.a1), "NEW");
    EXPECT_EQ(s.says(s.b1), "NEW");
    EXPECT_EQ(s.says(s.a3), "IL 1.50") << "a3 is not on notes";
}

TEST(LeaderScope, FreezeAndDetachTakeAScopeAndAnEmptyOneIsSaidNotRefused)
{
    Pits s;
    const std::size_t steps = s.steps();
    const std::string frozen = s.run("LEADER FREEZE LAYERS notes");
    // The smart notes among the four: a1, a2 and a4. b1 is plain already.
    EXPECT_EQ(frozen, "scope=layers layers=notes sublayers=yes matched=4\nfrozen leaders=3");
    EXPECT_FALSE(s.leader(s.a1).fields);
    EXPECT_EQ(s.leader(s.a1).text, "IL 1.50");
    EXPECT_EQ(s.leader(s.a2).text, "IL 2.50");
    EXPECT_TRUE(s.leader(s.a3).fields) << "a3 is on another layer";
    EXPECT_EQ(s.steps(), steps + 1);
    s.run("UNDO");
    EXPECT_TRUE(s.leader(s.a1).fields);

    const std::string detached = s.run("LEADER DETACH LAYERS notes");
    // a1, a2 and a4 let go (three), each frozen first; b1 has no tip.
    EXPECT_EQ(detached,
              "scope=layers layers=notes sublayers=yes matched=4\ndetached leaders=3 frozen=3");
    EXPECT_FALSE(s.leader(s.a2).tipRef.associated());
    EXPECT_TRUE(s.leader(s.a3).tipRef.associated());

    const std::size_t before = s.steps();
    const std::string none = s.run("LEADER FREEZE DRAWING WHERE LAYER=nowhere*");
    EXPECT_TRUE(contains(none, "matched=0")) << none;
    EXPECT_TRUE(contains(none, "frozen leaders=0")) << none;
    EXPECT_EQ(s.steps(), before) << "nothing taken, no step";
}

TEST(LeaderScope, AnEditOfTheSelectionWithNoLeaderSelectedStillTellsYouToSelectFirst)
{
    Pits s;
    EXPECT_TRUE(contains(s.refusal("LEADER FREEZE SELECTION"), "no leader is selected"));
    s.run("SELECT " + id(s.p1));
    EXPECT_TRUE(contains(s.refusal("LEADER SET SELECTION arrow=dot"), "no leader is selected"));
    s.run("SELECT " + id(s.a2));
    const std::string reply = s.run("LEADER FREEZE SELECTION");
    EXPECT_EQ(reply, "scope=selection matched=1\nfrozen leaders=1");
}

TEST(LeaderScope, PropSetsTheAttributeOnTheEntityEachLeaderIsOnOnceAndCountsTheSkipped)
{
    Pits s;
    const std::size_t steps = s.steps();
    const std::string reply = s.run("LEADER PROP LAYERS notes SET invert 9.5 real");
    // a1 and a4 are on p1, a2 on p2: two entities. b1 is on nothing: skipped.
    EXPECT_EQ(firstLine(reply),
              "scope=layers layers=notes sublayers=yes matched=4 entities=2 skipped=1")
        << reply;
    EXPECT_EQ(countLinesStarting(reply, "leader="), 3U) << reply;
    const auto invert = [&](EntityId pit) {
        return std::get<double>(s.document.model().entities.find(pit)->properties.at("invert"));
    };
    EXPECT_EQ(invert(s.p1), 9.5);
    EXPECT_EQ(invert(s.p2), 9.5);
    EXPECT_EQ(s.says(s.a2), "IL 9.50");
    EXPECT_EQ(s.says(s.a3), "IL 9.50") << "a3 reads p1 too";
    EXPECT_EQ(s.steps(), steps + 1) << "two entities, one step";

    s.run("LEADER PROP LAYERS notes DELETE invert");
    EXPECT_FALSE(s.document.model().entities.find(s.p1)->properties.contains("invert"));
    EXPECT_FALSE(s.document.model().entities.find(s.p2)->properties.contains("invert"));

    // One leader by id: the reply it always was.
    Pits t;
    const std::string one = t.run("LEADER PROP " + id(t.a1) + " SET invert 7 real");
    EXPECT_EQ(one.find("scope="), std::string::npos) << one;
    EXPECT_TRUE(contains(one, "leader=" + id(t.a1) + " target=" + id(t.p1) + " property=invert"))
        << one;
}

TEST(LeaderScope, PropOverLeadersWithNoEntityToSetItOnIsRefusedWithTheSingleLeadersReason)
{
    Pits s;
    s.run("SELECT " + id(s.b1));
    const std::string single = s.refusal("LEADER PROP " + id(s.b1) + " SET x 1 real");
    EXPECT_TRUE(contains(single, "tip is on no entity")) << single;
    // The scope of that one plain leader says the same, and sets nothing.
    const std::size_t steps = s.steps();
    const std::string scoped = s.refusal("LEADER PROP SELECTION SET x 1 real");
    EXPECT_TRUE(contains(scoped, "tip is on no entity")) << scoped;
    EXPECT_EQ(s.steps(), steps);
}

TEST(LeaderScope, AlignPutsTheNotesOfAScopeInAColumnAndLeavesTheOthers)
{
    Pits s;
    const std::string reply = s.run("LEADER ALIGN LAYERS notes x=70");
    // Four notes hang at (5,5), (55,55), (6,6) and (110,110): each moves to
    // x = 70 and keeps its y.
    EXPECT_EQ(reply, "scope=layers layers=notes sublayers=yes matched=4\naligned leaders=4 x=70");
    EXPECT_EQ(s.leader(s.a1).vertices.back(), Point2(70, 5));
    EXPECT_EQ(s.leader(s.a2).vertices.back(), Point2(70, 55));
    EXPECT_EQ(s.leader(s.a4).vertices.back(), Point2(70, 6));
    EXPECT_EQ(s.leader(s.b1).vertices.back(), Point2(70, 110));
    EXPECT_EQ(s.leader(s.a3).vertices.back(), Point2(-5, -5)) << "another layer";
}

// ---- making: FOR ------------------------------------------------------------------------------

namespace {

// Points with a level on layer plan (q1, q2) and on layer other (q3).
struct Levels : Session {
    EntityId q1 = 0, q2 = 0, q3 = 0;

    Levels()
    {
        run("LAYER NEW plan");
        run("LAYER NEW other");
        q1 = point(Point2(0, 0), "plan", {{"elevation", 25.0}});
        q2 = point(Point2(10, 0), "plan", {{"elevation", 30.0}});
        q3 = point(Point2(20, 0), "other", {{"elevation", 40.0}});
    }
};

} // namespace

TEST(LeaderScope, ForMakesALeaderToEachEntityAScopeTakesInOneStep)
{
    Levels s;
    const std::size_t steps = s.steps();
    const std::string reply = s.run("LEADER FOR LAYERS plan preset=level");
    // q1 and q2 are on plan; q3 is not.
    EXPECT_EQ(firstLine(reply), "scope=layers layers=plan sublayers=yes matched=2") << reply;
    const std::vector<EntityId> ids = s.document.lastCreatedEntities();
    ASSERT_EQ(ids.size(), 2U);
    EXPECT_TRUE(contains(reply, "created leaders=2 ids=" + id(ids[0]) + "," + id(ids[1]) +
                                    " skipped=0 preset=level"))
        << reply;
    EXPECT_EQ(s.says(ids[0]), "RL 25.000");
    EXPECT_EQ(s.says(ids[1]), "RL 30.000");
    EXPECT_EQ(s.leader(ids[0]).tipRef.entity, s.q1);
    EXPECT_EQ(s.steps(), steps + 1);
    s.run("UNDO");
    EXPECT_EQ(s.steps(), steps);
}

TEST(LeaderScope, ForTakesAWhereFilterAfterItsOwnOptionsAndAnArea)
{
    Levels s;
    // The levels of every point in the drawing: the three. `preset` is no
    // WHERE key, so it stays the verb's after WHERE.
    const std::string all = s.run("LEADER FOR DRAWING WHERE TYPE=point preset=level");
    EXPECT_TRUE(contains(firstLine(all), "matched=3")) << all;
    EXPECT_EQ(s.document.lastCreatedEntities().size(), 3U);
    // text= before WHERE is the note, LAYER= after it the filter: "NOTE" on
    // the two points of plan - and none on other.
    Levels t;
    const std::string noted = t.run("LEADER FOR DRAWING text=NOTE WHERE TYPE=point LAYER=plan");
    EXPECT_TRUE(contains(firstLine(noted), "matched=2")) << noted;
    const std::vector<EntityId> made = t.document.lastCreatedEntities();
    ASSERT_EQ(made.size(), 2U);
    EXPECT_EQ(t.says(made[0]), "NOTE");
    // AREA -1,-1,11,1: q1 at (0,0) and q2 at (10,0) are inside, q3 at (20,0)
    // is not.
    Levels u;
    const std::string area = u.run("LEADER FOR AREA -1,-1,11,1 preset=level");
    EXPECT_TRUE(contains(firstLine(area), "scope=area area=-1,-1,11,1 matched=2")) << area;
}

TEST(LeaderScope, ForReportsAScopeThatTakesNothingAndRefusesAnEmptySelection)
{
    Levels s;
    const std::size_t steps = s.steps();
    const std::string none = s.run("LEADER FOR LAYERS plan WHERE TYPE=circle preset=level");
    EXPECT_TRUE(contains(firstLine(none), "matched=0")) << none;
    EXPECT_TRUE(contains(none, "created leaders=0 ids= skipped=0 preset=level")) << none;
    EXPECT_EQ(s.steps(), steps);
    EXPECT_TRUE(contains(s.refusal("LEADER FOR SELECTION preset=level"), "nothing is selected"));
    EXPECT_TRUE(contains(s.refusal("LEADER FOR VIEW preset=level"), "AREA"));
    // The selection still works as it did.
    s.run("SELECT " + id(s.q3));
    const std::string one = s.run("LEADER FOR SELECTION preset=level");
    EXPECT_EQ(firstLine(one), "scope=selection matched=1") << one;
    EXPECT_EQ(s.says(s.document.lastCreatedEntities().front()), "RL 40.000");
}

TEST(LeaderScope, BalloonForNumbersTheBalloonsOfAScopeAndRenumberTakesOneToo)
{
    Levels s;
    const std::string made = s.run("BALLOON FOR LAYERS plan");
    EXPECT_TRUE(contains(made, "created balloons=2")) << made;
    const std::vector<EntityId> balloons = s.document.lastCreatedEntities();
    ASSERT_EQ(balloons.size(), 2U);
    // On from the highest there is (none): 1 and 2.
    EXPECT_EQ(s.leader(balloons[0]).text, "1");
    EXPECT_EQ(s.leader(balloons[1]).text, "2");

    // A third on another layer, numbered 3; renumber only the layer plan's.
    s.run("LAYER SET other");
    const EntityId third = s.made("BALLOON FOR " + id(s.q3));
    EXPECT_EQ(s.leader(third).text, "3");
    s.run("LAYER SET 0");
    const std::size_t steps = s.steps();
    // The scope is the leaders' own layer: they were made on layer 0, so
    // LAYERS plan takes none and nothing is renumbered.
    const std::string none = s.run("BALLOON RENUMBER LAYERS plan start=10");
    EXPECT_EQ(none, "scope=layers layers=plan sublayers=yes matched=0\nballoons=0 renumbered=0");
    EXPECT_EQ(s.steps(), steps);
    // By id: the two on layer 0 become 10 and 11, the one on other keeps 3.
    const std::string two = s.run("BALLOON RENUMBER DRAWING WHERE LAYER=0 start=10");
    EXPECT_EQ(two, "scope=drawing where=\"LAYER=0\" matched=2\nballoons=2 renumbered=2");
    EXPECT_EQ(s.leader(balloons[0]).text, "10");
    EXPECT_EQ(s.leader(balloons[1]).text, "11");
    EXPECT_EQ(s.leader(third).text, "3");
    // With no scope: every numbered balloon, in the order made, from 1.
    const std::string every = s.run("BALLOON RENUMBER");
    // 10 -> 1 and 11 -> 2 change; the third already says 3.
    EXPECT_EQ(every, "balloons=3 renumbered=2");
    EXPECT_EQ(s.leader(balloons[0]).text, "1");
    EXPECT_EQ(s.leader(third).text, "3");
}

// ---- the names a note reads follow a rename (L3) -----------------------------------------------

namespace {

std::shared_ptr<const terrain::TinSurface> plane(double z0, double ax, double ay)
{
    const auto z = [&](double x, double y) { return z0 + ax * x + ay * y; };
    auto tin = terrain::TinSurface::create(
        {Point3(0, 0, z(0, 0)), Point3(100, 0, z(100, 0)), Point3(100, 100, z(100, 100)),
         Point3(0, 100, z(0, 100))},
        {terrain::TinTriangle{0, 1, 2}, terrain::TinTriangle{0, 2, 3}});
    EXPECT_TRUE(tin.ok()) << tin.error().describe();
    return std::make_shared<const terrain::TinSurface>(std::move(*tin));
}

// The plane z = 10 + 0.1 x + 0.2 y, named "ground": 21 at (30, 40), by hand
// 10 + 3 + 8. A point at RL 25 there is 4 above it.
struct Ground : Session {
    EntityId pit = 0;

    Ground()
    {
        terrain::NamedSurface entry;
        entry.name = "ground";
        entry.surface = plane(10.0, 0.1, 0.2);
        entry.source = "test plane";
        entry.visible = true;
        terrain::SurfaceStore store;
        EXPECT_TRUE(store.add(std::move(entry)).ok());
        EXPECT_TRUE(document.setSurfaces(std::move(store), "TEST_SURFACES").ok());
        run("LAYER NEW notes");
        pit = point(Point2(30, 40), "0", {{"elevation", 25.0}});
    }
};

} // namespace

TEST(LeaderScope, RenamingASurfaceRewritesTheNotesThatReadItInTheSameUndoStep)
{
    Ground s;
    const EntityId above = s.made("LEADER #" + id(s.pit) + " 40,50 preset=surface surface=ground");
    // A name in another case is the same surface (the store finds it so).
    const EntityId level =
        s.made("LEADER #" + id(s.pit) + " 41,51 template=\"{surface.GROUND:.2f} / {{x}}\"");
    EXPECT_EQ(s.says(above), "+4.000");
    EXPECT_EQ(s.says(level), "21.00 / {x}");
    const std::size_t steps = s.steps();

    const std::string reply = s.run("SURFACE RENAME ground eg");
    EXPECT_TRUE(contains(reply, "renamed surface=ground name=eg corridors=0 roadobjects=0 leaders=2"))
        << reply;
    EXPECT_EQ(s.leader(above).text, "{dsurface.eg:+.3f}");
    EXPECT_EQ(s.leader(level).text, "{surface.eg:.2f} / {{x}}")
        << "the steps, the literal text and the doubled braces are kept";
    EXPECT_EQ(s.says(above), "+4.000") << "the note reads the renamed surface";
    EXPECT_EQ(s.says(level), "21.00 / {x}");
    EXPECT_EQ(s.steps(), steps + 1) << "the surface and its readers, one step";

    s.run("UNDO");
    EXPECT_EQ(s.leader(above).text, "{dsurface.ground:+.3f}");
    EXPECT_NE(s.document.surfaces().find("ground"), nullptr);
    EXPECT_EQ(s.says(above), "+4.000");
}

TEST(LeaderScope, ANameANoteCannotWriteLeavesTheReadersAndSaysSoInAWarning)
{
    Ground s;
    const EntityId above = s.made("LEADER #" + id(s.pit) + " 40,50 preset=surface surface=ground");
    // ':' starts a step in a field, so "a:b" cannot be a surface a note names.
    const std::string reply = s.run("SURFACE RENAME ground a:b");
    EXPECT_TRUE(contains(reply, "leaders=0")) << reply;
    EXPECT_TRUE(contains(reply, "warning leaders=1 still read surface ground")) << reply;
    EXPECT_EQ(s.leader(above).text, "{dsurface.ground:+.3f}") << "left, not half-rewritten";
    EXPECT_NE(s.document.surfaces().find("a:b"), nullptr) << "the rename itself was done";
}

TEST(LeaderScope, ALeaderOnALockedLayerIsLeftReadingTheOldNameAndTheRenameStillRuns)
{
    Ground s;
    s.run("LAYER SET notes");
    const EntityId locked = s.made("LEADER #" + id(s.pit) + " 40,50 preset=surface surface=ground");
    s.run("LAYER SET 0");
    const EntityId free = s.made("LEADER #" + id(s.pit) + " 41,51 preset=surface surface=ground");
    s.run("LAYER LOCK notes");
    const std::string reply = s.run("SURFACE RENAME ground eg");
    EXPECT_TRUE(contains(reply, "leaders=1")) << reply;
    EXPECT_TRUE(contains(reply, "warning leaders=1 still read surface ground")) << reply;
    EXPECT_EQ(s.leader(locked).text, "{dsurface.ground:+.3f}");
    EXPECT_EQ(s.leader(free).text, "{dsurface.eg:+.3f}");
}

TEST(LeaderScope, AnAlignmentsNameIsExactAndCountedWhenItIsDeleted)
{
    Session s;
    s.run("ALIGN NEW MC01 0,0 100,0");
    const EntityId pit = s.point(Point2(30, 5), "0");
    s.run("LEADER #" + id(pit) + " 40,10 preset=station alignment=MC01");
    s.run("LEADER #" + id(pit) + " 41,11 template=\"{offset.MC01:+.3f}\"");
    // A note that names neither is not counted.
    s.run("LEADER #" + id(pit) + " 42,12 template=\"{id}\"");
    EXPECT_EQ(ann::leadersReading(s.document.model(), ann::ReadKind::Alignment, "MC01"), 2U);
    EXPECT_EQ(ann::leadersReading(s.document.model(), ann::ReadKind::Alignment, "mc01"), 0U)
        << "an alignment is found exactly";
    EXPECT_EQ(ann::leadersReading(s.document.model(), ann::ReadKind::Surface, "MC01"), 0U);

    const std::string reply = s.run("ALIGN DELETE MC01");
    EXPECT_TRUE(contains(reply, "alignment MC01 deleted")) << reply;
    EXPECT_TRUE(contains(reply, "warning leaders=2 read alignment MC01 and will read nothing now"))
        << reply;
    // None read it any more: no warning for an alignment nobody reads.
    s.run("ALIGN NEW MC02 0,0 100,0");
    EXPECT_EQ(s.run("ALIGN DELETE MC02").find("warning"), std::string::npos);
}

TEST(LeaderScope, RenameLeaderReadsFollowsAnAlignmentByTheFunctionTheRenameWillUse)
{
    Session s;
    s.run("ALIGN NEW MC01 0,0 100,0");
    const EntityId pit = s.point(Point2(30, 5), "0");
    const EntityId station =
        s.made("LEADER #" + id(pit) + " 40,10 preset=station alignment=MC01");
    EXPECT_EQ(s.leader(station).text, "CH {station.MC01:ch}\nOS {offset.MC01:+.3f}");
    ann::LeaderReadsRenamed renamed =
        ann::renameLeaderReads(s.document.model(), ann::ReadKind::Alignment, "MC01", "Main Rd");
    EXPECT_EQ(renamed.followed, 1U);
    EXPECT_EQ(renamed.unfollowed, 0U);
    ASSERT_TRUE(renamed.command);
    ASSERT_TRUE(s.document.execute(std::move(renamed.command)).ok());
    // Both fields, with a blank in the name as a note writes one; the line
    // break and the steps are kept.
    EXPECT_EQ(s.leader(station).text, "CH {station.Main Rd:ch}\nOS {offset.Main Rd:+.3f}");
    // A rename to its own spelling changes nothing and makes no command.
    EXPECT_FALSE(ann::renameLeaderReads(s.document.model(), ann::ReadKind::Alignment, "Main Rd",
                                        "Main Rd")
                     .command);
    // Nothing reads it, so nothing is made.
    EXPECT_FALSE(
        ann::renameLeaderReads(s.document.model(), ann::ReadKind::Alignment, "none", "x").command);
}

TEST(LeaderScope, TemplateFieldsAreRenamedAndEverythingElseOfTheTemplateIsKept)
{
    const auto rename = [](std::string_view name) -> std::optional<std::string> {
        if (name == "dsurface.Ground") {
            return std::string("dsurface.eg");
        }
        return std::nullopt;
    };
    // The braces doubled to be literal, the line break, the other fields and
    // the steps stay as they were.
    EXPECT_EQ(renameTemplateFields("{{x}} {dsurface.Ground:+.3f}\nCUT {cut.ground:.3f} {{", rename),
              "{{x}} {dsurface.eg:+.3f}\nCUT {cut.ground:.3f} {{");
    // A field with no step.
    EXPECT_EQ(renameTemplateFields("{dsurface.Ground}", rename), "{dsurface.eg}");
    // Braces that do not balance name nothing: the template comes back as it was.
    EXPECT_EQ(renameTemplateFields("{dsurface.Ground", rename), "{dsurface.Ground");
    EXPECT_EQ(renameTemplateFields("dsurface.Ground}", rename), "dsurface.Ground}");
    EXPECT_EQ(renameTemplateFields("", rename), "");
}

// ---- the JSON's guard against a byte that is not UTF-8 (L4) -----------------------------------

TEST(LeaderScope, AByteThatIsNotUtf8InAnyNameIsWrittenAsTheReplacementCharacterNotThrown)
{
    // 0xFF is never a byte of UTF-8. Nothing in a Document can hold one any
    // more (SurfaceStore::checkName and the repair on open), so no drawing
    // reaches the writer's guard; the report is plain data, handed one here.
    ann::LeadersReport report;
    report.surfaces = {"Gro\xFF" "und"};
    report.alignments = {"MC\xFF"};
    ann::LeaderReport leader;
    leader.id = 7;
    leader.layer = "no\xFF" "tes";
    leader.text = "bad\xFF";
    leader.lines = {"bad\xFF"};
    report.leaders.push_back(leader);
    std::string json;
    ASSERT_NO_THROW(json = ann::leadersJsonOf(report));
    // U+FFFD is EF BF BD in UTF-8, one for each bad byte.
    EXPECT_NE(json.find("Gro\xEF\xBF\xBD" "und"), std::string::npos) << json;
    EXPECT_NE(json.find("MC\xEF\xBF\xBD"), std::string::npos) << json;
    EXPECT_NE(json.find("no\xEF\xBF\xBD" "tes"), std::string::npos) << json;
    EXPECT_EQ(json.find('\xFF'), std::string::npos) << "no raw bad byte in the reply";
}

TEST(LeaderScope, ALeadersReportRoundTripsThroughTheDocumentsJsonAndAnEmptyOneIsAnEmptyList)
{
    Pits s;
    const std::string json = ann::leadersJson(s.document);
    EXPECT_TRUE(contains(json, "\"template\": \"IL {prop.invert:.2f}\"")) << json;
    EXPECT_EQ(json.find("\"scope\""), std::string::npos);
    // `only` empty is every leader, as it always was; an explicit empty list
    // in the report is none.
    EXPECT_TRUE(contains(ann::leadersJson(s.document, {}, std::nullopt), "\"id\": "));
    const ann::LeadersReport none =
        ann::leadersReport(s.document, std::vector<EntityId>{}, std::nullopt);
    EXPECT_TRUE(none.leaders.empty());
    EXPECT_EQ(ann::leadersReport(s.document).leaders.size(), 5U);
    const ann::LeadersReport onP1 = ann::leadersReport(s.document, std::nullopt, s.p1);
    EXPECT_EQ(onP1.leaders.size(), 3U) << "a1, a3 and a4 are on p1";
}

// ---- the words the Leaders manager hands to the verbs ----------------------------------------------

TEST(LeaderScope, AnAnchorWordReadsBackAsTheSamePlaceTheDialogShowed)
{
    Session s;
    const EntityId point = s.point(Point2(0, 0), "0");
    const EntityId line = s.made("LINE 0,0 40,0");
    const EntityId circle = s.made("CIRCLE 100,0 5");
    const EntityId open = s.made("PLINE 0,50 10,50 10,60 20,60");
    const EntityId lot = s.made("PLINE 0,100 10,100 10,110 0,110 C");
    const EntityId leader = s.made("LEADER 0,0 5,5 text=A");
    // Every kind of place the dialog or a tool can name, on an entity that
    // has it: the verb must read the word back as exactly this reference,
    // the fraction included.
    const std::vector<AnchorRef> places = {
        {point, AnchorPoint::Position, 0, 0.0},
        {line, AnchorPoint::Start, 0, 0.0},
        {line, AnchorPoint::End, 0, 0.0},
        {line, AnchorPoint::Mid, 0, 0.0},
        {line, AnchorPoint::Along, 0, 0.123456},
        {circle, AnchorPoint::Centre, 0, 0.0},
        {circle, AnchorPoint::Along, 0, 0.5},
        {open, AnchorPoint::Vertex, 2, 0.0},
        {open, AnchorPoint::SegmentMid, 1, 0.0},
        {open, AnchorPoint::Along, 1, 0.25},
        {lot, AnchorPoint::Inside, 0, 0.0},
    };
    for (const AnchorRef& place : places) {
        const std::string word = ann::anchorWord(place);
        s.run("LEADER SET " + id(leader) + " tip=" + word);
        EXPECT_EQ(s.leader(leader).tipRef, place) << word;
    }
    EXPECT_EQ(ann::anchorWord(AnchorRef{12, AnchorPoint::Along, 3, 0.25}), "#12.along3:0.25");
    EXPECT_EQ(ann::anchorWord(AnchorRef{12, AnchorPoint::Vertex, 4, 0.0}), "#12.v4");
}
