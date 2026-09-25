// Smart leaders (docs/annotation.md, "Smart leaders"): leaders whose tip is
// on an entity and whose note is read off it, driven as an agent drives them
// - the LEADER verbs, one undo step per edit - and followed through the
// edits that move, copy and erase what they are on.
//
// Expected values are worked out by hand in the comments beside them.

#include <gtest/gtest.h>

#include <cmath>
#include <map>
#include <string>

#include "katana/cad/annotation/leader_draw.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/leader_values.hpp"
#include "katana/math/numerics.hpp"

using namespace katana::cad;
using namespace katana::entity;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Vec2;
namespace cmd = katana::commands;
namespace ann = katana::cad::annotation;

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
    EntityId add(Geometry geometry, PropertyMap properties = {})
    {
        Entity entity;
        entity.geometry = std::move(geometry);
        entity.properties = std::move(properties);
        EXPECT_TRUE(document.execute(cmd::createEntities({std::move(entity)})).ok());
        return document.lastCreatedEntities().front();
    }
    [[nodiscard]] EntityId last() const { return document.lastCreatedEntities().front(); }
    [[nodiscard]] const LeaderGeometry& leader(EntityId id) const
    {
        return std::get<LeaderGeometry>(document.model().entities.find(id)->geometry);
    }
    [[nodiscard]] std::string note(EntityId id) const
    {
        return leaderNote(document.model(), leader(id));
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

} // namespace

// ---- making one ----------------------------------------------------------------------------

TEST(SmartLeaders, ATemplateIsReadOffThePitAndFollowsItsAttributes)
{
    Session s;
    const EntityId pit = s.add(PointGeometry{Point2(10, 10)}, {{"elevation", 12.5}});
    const std::string made = s.run("LEADER #" + id(pit) + " 20,20 template=\"PIT {id}\\nRL " +
                                   "{rl:.2f}\\nIL {prop.invert:.2f}\"");
    const EntityId leader = s.last();
    // The IL line names a property the pit does not carry yet: dropped.
    EXPECT_EQ(s.note(leader), "PIT " + id(pit) + "\nRL 12.50");
    EXPECT_TRUE(contains(made, "associative=yes")) << made;
    EXPECT_TRUE(contains(made, "target=" + id(pit))) << made;
    EXPECT_TRUE(contains(made, "template=")) << made;
    EXPECT_TRUE(s.leader(leader).fields);

    // Setting the attribute through the leader is one step on the pit, and
    // the note says it at once.
    const std::size_t before = s.steps();
    const std::string set = s.run("LEADER PROP " + id(leader) + " SET invert 10.5");
    EXPECT_EQ(s.steps(), before + 1);
    EXPECT_TRUE(contains(set, "IL 10.50")) << set;
    const auto& properties = s.document.model().entities.find(pit)->properties;
    ASSERT_TRUE(properties.contains("invert"));
    EXPECT_EQ(std::get<double>(properties.at("invert")), 10.5);
    EXPECT_EQ(s.note(leader), "PIT " + id(pit) + "\nRL 12.50\nIL 10.50");
    s.run("LEADER PROP " + id(leader) + " SET invert 9 integer");
    EXPECT_EQ(s.note(leader), "PIT " + id(pit) + "\nRL 12.50\nIL 9.00");
    s.run("LEADER PROP " + id(leader) + " DELETE invert");
    EXPECT_EQ(s.note(leader), "PIT " + id(pit) + "\nRL 12.50");
    s.run("UNDO");
    EXPECT_EQ(s.note(leader), "PIT " + id(pit) + "\nRL 12.50\nIL 9.00");
}

TEST(SmartLeaders, WhatCannotBeReadIsRefusedWithWhatIsMissing)
{
    Session s;
    const EntityId pit = s.add(PointGeometry{Point2(0, 0)});
    const std::string at = "LEADER #" + id(pit) + " 5,5 ";
    EXPECT_TRUE(contains(s.refusal(at + "template=\"IL {prop.invrt}\""), "no prop.invrt"));
    EXPECT_TRUE(
        contains(s.refusal(at + "template=\"{invert}\""), "a leader has no value 'invert'"));
    EXPECT_TRUE(contains(s.refusal("LEADER 0,0 5,5 template=\"{type}\""),
                         "reads the entity its tip is on"));
    EXPECT_TRUE(contains(s.refusal(at + "text=A template=\"{type}\""), "one of text="));
    EXPECT_TRUE(contains(s.refusal(at + "labelstyle=Nope"), "label style does not exist"));
    EXPECT_EQ(s.document.model().entities.size(), 1u) << "nothing made";

    // A plain note is what was typed, braces and all.
    s.run("LEADER 0,0 5,5 text=\"{not a field}\"");
    EXPECT_EQ(s.note(s.last()), "{not a field}");
}

TEST(SmartLeaders, ATipOnALineStaysAtItsPlaceAlongIt)
{
    Session s;
    const EntityId line = s.add(Segment2{Point2(0, 0), Point2(40, 0)});
    // #line@10,3 is the point of the line nearest (10, 3): (10, 0), a
    // quarter of the way along.
    s.run("LEADER #" + id(line) + "@10,3 20,10 template=\"CH {chainage:.1f}\"");
    const EntityId leader = s.last();
    EXPECT_EQ(s.leader(leader).vertices.front(), Point2(10, 0));
    EXPECT_EQ(s.leader(leader).tipRef.point, AnchorPoint::Along);
    EXPECT_DOUBLE_EQ(s.leader(leader).tipRef.parameter, 0.25);
    EXPECT_EQ(s.note(leader), "CH 10.0");

    // Stretched to 80 m, the tip keeps its quarter - (20, 0), chainage 20 -
    // in the same step as the stretch.
    const std::size_t before = s.steps();
    ASSERT_TRUE(
        s.document.execute(cmd::setEntityGeometry(line, Segment2{Point2(0, 0), Point2(80, 0)}))
            .ok());
    EXPECT_EQ(s.steps(), before + 1);
    EXPECT_EQ(s.leader(leader).vertices.front(), Point2(20, 0));
    EXPECT_EQ(s.note(leader), "CH 20.0");
    // Moved, the tip goes with it.
    ASSERT_TRUE(s.document.execute(cmd::moveEntities({line}, Vec2(0, 5))).ok());
    EXPECT_EQ(s.leader(leader).vertices.front(), Point2(20, 5));
    ASSERT_TRUE(s.document.undo().ok());
    ASSERT_TRUE(s.document.undo().ok());
    EXPECT_EQ(s.leader(leader).vertices.front(), Point2(10, 0)) << "one undo each";
}

TEST(SmartLeaders, InsideALotTheArrowIsADotAndTheNoteItsArea)
{
    Session s;
    const EntityId lot =
        s.add(Polyline2{{Point2(0, 0), Point2(20, 0), Point2(20, 10), Point2(0, 10)}, true},
              {{"lot", std::int64_t{12}}});
    s.run("LEADER #" + id(lot) + ".inside 30,20 template=\"LOT {prop.lot}\\n{area:.0f} m2\"");
    const LeaderGeometry& leader = s.leader(s.last());
    EXPECT_EQ(leader.vertices.front(), Point2(10, 5)) << "the lot's inside point";
    EXPECT_EQ(leader.arrow, ArrowHead::Dot) << "a leader ending inside an outline ends in a dot";
    EXPECT_EQ(s.note(s.last()), "LOT 12\n200 m2");
    s.run("LEADER #" + id(lot) + ".inside 30,20 text=X arrow=open");
    EXPECT_EQ(s.leader(s.last()).arrow, ArrowHead::Open) << "unless the arrow is given";
}

TEST(SmartLeaders, ALabelStylesTemplateIsReadLiveAndTheStyleIsKept)
{
    Session s;
    const EntityId pit = s.add(PointGeometry{Point2(0, 0)}, {{"invert", 10.25}});
    s.run("TEXTSTYLE NEW Notes paper=3.5");
    s.run("LABELSTYLE NEW Invert kind=point text=\"INV {prop.invert:.3f}\" textstyle=Notes "
          "paper=3");
    const std::string made = s.run("LEADER #" + id(pit) + " 5,5 labelstyle=Invert");
    EXPECT_TRUE(contains(made, "labelstyle=Invert")) << made;
    const EntityId leader = s.last();
    EXPECT_EQ(s.leader(leader).labelStyle, "Invert");
    EXPECT_TRUE(s.leader(leader).text.empty());
    EXPECT_EQ(s.leader(leader).style, "Notes") << "the style's face is lent when it is made";
    EXPECT_EQ(s.leader(leader).paperHeight, 3.0);
    EXPECT_EQ(s.note(leader), "INV 10.250");

    s.run("LABELSTYLE SET Invert text=\"IL {prop.invert:.2f}\"");
    EXPECT_EQ(s.note(leader), "IL 10.25") << "one style edited, every leader in it follows";
    EXPECT_TRUE(contains(s.refusal("LABELSTYLE DELETE Invert"), "leaders"));
}

// ---- asking and changing -----------------------------------------------------------------

TEST(SmartLeaders, ValuesListsWhatANoteThereCouldSay)
{
    Session s;
    const EntityId line =
        s.add(Segment2{Point2(0, 0), Point2(40, 0)}, {{"size", std::string("150")}});
    const std::string place = s.run("LEADER VALUES #" + id(line) + "@10,3");
    EXPECT_TRUE(contains(place, "target=" + id(line) + " type=Line")) << place;
    EXPECT_TRUE(contains(place, "\nvalue=chainage text=10.000")) << place;
    EXPECT_TRUE(contains(place, "\nvalue=length text=40.000")) << place;
    EXPECT_TRUE(contains(place, "\nvalue=bearing text=")) << place;
    EXPECT_TRUE(contains(place, "\nvalue=prop.size text=150")) << place;
    EXPECT_FALSE(contains(place, "value=z ")) << "no level, so no z";

    s.run("LEADER #" + id(line) + "@30,0 35,5 template=\"{prop.size} PVC\"");
    const std::string ofLeader = s.run("LEADER VALUES " + id(s.last()));
    EXPECT_TRUE(contains(ofLeader, "leader=" + id(s.last()) + " target=" + id(line))) << ofLeader;
    EXPECT_TRUE(contains(ofLeader, "value=chainage text=30.000")) << ofLeader;
    s.run("LEADER 0,0 1,1 text=X");
    EXPECT_TRUE(contains(s.refusal("LEADER VALUES " + id(s.last())), "on no entity"));
}

TEST(SmartLeaders, ListSetAttachDetachAndFreeze)
{
    Session s;
    const EntityId pit = s.add(PointGeometry{Point2(0, 0)}, {{"invert", 10.25}});
    const EntityId other = s.add(PointGeometry{Point2(50, 0)}, {{"invert", 8.0}});
    s.run("LEADER #" + id(pit) + " 5,5 template=\"IL {prop.invert:.2f}\"");
    const EntityId smart = s.last();
    s.run("LEADER 100,100 105,105 text=PLAIN");
    const EntityId plain = s.last();

    const std::string list = s.run("LEADER LIST");
    EXPECT_TRUE(contains(list, "id=" + id(smart))) << list;
    EXPECT_TRUE(contains(list, "smart=yes")) << list;
    EXPECT_TRUE(contains(list, "text=\"IL 10.25\"")) << list;
    EXPECT_TRUE(contains(list, "id=" + id(plain))) << list;
    const std::string onPit = s.run("LEADER LIST target=" + id(pit));
    EXPECT_FALSE(contains(onPit, "id=" + id(plain))) << onPit;

    // SET: a new template; the tip moved to another pit reads that one.
    s.run("LEADER SET " + id(smart) + " template=\"INV {prop.invert:.1f}\"");
    EXPECT_EQ(s.note(smart), "INV 10.3") << "10.25 is exact in binary: half away from zero";
    s.run("LEADER SET " + id(smart) + " tip=#" + id(other));
    EXPECT_EQ(s.leader(smart).vertices.front(), Point2(50, 0));
    EXPECT_EQ(s.note(smart), "INV 8.0");
    s.run("LEADER SET " + id(smart) + " at=60,20");
    EXPECT_EQ(s.leader(smart).vertices.back(), Point2(60, 20));
    EXPECT_TRUE(contains(s.refusal("LEADER SET " + id(smart) + " " + id(plain) + " at=1,1"),
                         "name one leader"));
    EXPECT_TRUE(contains(s.refusal("LEADER SET " + id(smart) + " tip=70,70"),
                         "reads the entity its tip is on"));
    EXPECT_TRUE(contains(s.refusal("LEADER SET " + id(smart) + " tip=#" + id(smart)), "itself"));

    // ATTACH a plain leader: it follows the pit, its note is still its own.
    s.run("LEADER ATTACH " + id(plain) + " #" + id(pit));
    EXPECT_EQ(s.leader(plain).vertices.front(), Point2(0, 0));
    EXPECT_EQ(s.leader(plain).tipRef.entity, pit);
    EXPECT_EQ(s.note(plain), "PLAIN");

    // FREEZE keeps the words and the tip's place; DETACH lets the tip go.
    EXPECT_EQ(s.run("LEADER FREEZE " + id(smart) + " " + id(plain)), "frozen leaders=1");
    EXPECT_FALSE(isSmart(s.leader(smart)));
    EXPECT_EQ(s.leader(smart).text, "INV 8.0");
    EXPECT_EQ(s.leader(smart).tipRef.entity, other);
    EXPECT_EQ(s.run("LEADER DETACH " + id(smart) + " " + id(plain)), "detached leaders=2 frozen=0");
    EXPECT_FALSE(s.leader(plain).tipRef.associated());
    EXPECT_EQ(s.run("LEADER DETACH " + id(plain)), "detached leaders=0 frozen=0")
        << "nothing to let go is no step";
}

TEST(SmartLeaders, DetachingASmartLeaderFreezesItsNoteFirst)
{
    Session s;
    const EntityId pit = s.add(PointGeometry{Point2(0, 0)}, {{"invert", 10.25}});
    s.run("LEADER #" + id(pit) + " 5,5 template=\"IL {prop.invert:.2f}\"");
    const EntityId leader = s.last();
    EXPECT_EQ(s.run("LEADER DETACH " + id(leader)), "detached leaders=1 frozen=1");
    EXPECT_EQ(s.leader(leader).text, "IL 10.25");
    EXPECT_FALSE(s.leader(leader).fields);
    EXPECT_FALSE(s.leader(leader).tipRef.associated());
    s.run("UNDO");
    EXPECT_TRUE(isSmart(s.leader(leader))) << "one step back";
}

TEST(SmartLeaders, ForMakesOneLeaderPerEntityInOneStep)
{
    Session s;
    s.run("ANNOSCALE 500");
    const EntityId pit = s.add(PointGeometry{Point2(0, 0)});
    const EntityId line = s.add(Segment2{Point2(100, 0), Point2(140, 0)});
    const EntityId lot =
        s.add(Polyline2{{Point2(200, 0), Point2(220, 0), Point2(220, 10), Point2(200, 10)}, true});
    const EntityId circle = s.add(Circle2{Point2(300, 0), 5.0});
    const EntityId dimension = s.add(DimensionGeometry{Point2(0, 50), Point2(10, 50), 2.0, ""});
    const std::size_t before = s.steps();
    const std::string made = s.run("LEADER FOR " + id(pit) + " " + id(line) + " " + id(lot) + " " +
                                   id(circle) + " " + id(dimension) + " template=\"{type}\"");
    EXPECT_TRUE(contains(made, "created leaders=4")) << made;
    EXPECT_TRUE(contains(made, "skipped=1")) << "a dimension offers no place: " << made;
    EXPECT_EQ(s.steps(), before + 1);

    std::map<EntityId, LeaderGeometry> byTarget;
    s.document.model().entities.forEach([&](const Entity& entity) {
        if (const auto* leader = std::get_if<LeaderGeometry>(&entity.geometry)) {
            byTarget[leader->tipRef.entity] = *leader;
        }
    });
    ASSERT_EQ(byTarget.size(), 4u);
    // 10 mm at 1:500 is 5 m, at 45 degrees: (5 cos 45, 5 sin 45) past the tip.
    const double reach = 5.0 * std::cos(0.25 * katana::math::kPi);
    EXPECT_EQ(byTarget[pit].vertices.front(), Point2(0, 0));
    EXPECT_NEAR(byTarget[pit].vertices.back().x, reach, 1e-12);
    EXPECT_NEAR(byTarget[pit].vertices.back().y, reach, 1e-12);
    EXPECT_EQ(byTarget[line].vertices.front(), Point2(120, 0)) << "the middle of the line";
    EXPECT_EQ(byTarget[lot].vertices.front(), Point2(210, 5)) << "inside the lot";
    EXPECT_EQ(byTarget[lot].arrow, ArrowHead::Dot);
    // On the circle, on the side the note goes: 45 degrees round.
    EXPECT_NEAR(byTarget[circle].vertices.front().x, 300.0 + reach, 1e-12);
    EXPECT_NEAR(byTarget[circle].vertices.front().y, reach, 1e-12);
    s.run("UNDO");
    std::size_t leaders = 0;
    s.document.model().entities.forEach([&](const Entity& entity) {
        leaders += std::holds_alternative<LeaderGeometry>(entity.geometry) ? 1 : 0;
    });
    EXPECT_EQ(leaders, 0u);
    EXPECT_TRUE(contains(s.refusal("LEADER FOR " + id(dimension)), "no leader made"));
}

TEST(SmartLeaders, BalloonsForTheSelectionAreNumberedOn)
{
    Session s;
    s.run("BALLOON 0,0 5,5");
    const EntityId a = s.add(PointGeometry{Point2(10, 0)});
    const EntityId b = s.add(PointGeometry{Point2(20, 0)});
    s.run("SELECT " + id(a) + " " + id(b));
    const std::string made = s.run("BALLOON FOR SELECTION");
    EXPECT_TRUE(contains(made, "created balloons=2")) << made;
    const auto ids = s.document.lastCreatedEntities();
    ASSERT_EQ(ids.size(), 2u);
    EXPECT_EQ(s.leader(ids[0]).text, "2");
    EXPECT_EQ(s.leader(ids[1]).text, "3");
    EXPECT_EQ(s.leader(ids[0]).callout, CalloutShape::Circle);
    // A smart balloon says its entity's item and is no number in the run.
    s.run("BALLOON #" + id(a) + " 30,30 template=\"{id}\"");
    EXPECT_TRUE(contains(s.run("BALLOON 40,40 45,45"), "text=4"));
}

// ---- following the geometry --------------------------------------------------------------

TEST(SmartLeaders, ASmartLeaderGoesWithItsTargetAndComesBackOnUndo)
{
    Session s;
    const EntityId pit = s.add(PointGeometry{Point2(0, 0)});
    s.run("LEADER #" + id(pit) + " 5,5 template=\"PIT {id}\"");
    const EntityId smart = s.last();
    // A smart leader on the smart leader's end: a chain.
    s.run("LEADER #" + id(smart) + ".end 9,9 template=\"{type}\"");
    const EntityId chained = s.last();
    s.run("LEADER #" + id(pit) + " -5,5 text=PLAIN");
    const EntityId plain = s.last();

    s.run("SELECT " + id(pit));
    s.run("ERASE");
    EXPECT_FALSE(s.document.model().entities.contains(smart)) << "it read the pit";
    EXPECT_FALSE(s.document.model().entities.contains(chained)) << "it read the leader that went";
    ASSERT_TRUE(s.document.model().entities.contains(plain));
    EXPECT_FALSE(s.leader(plain).tipRef.associated()) << "a plain one lets the reference go";

    s.run("UNDO");
    ASSERT_TRUE(s.document.model().entities.contains(smart));
    ASSERT_TRUE(s.document.model().entities.contains(chained));
    EXPECT_EQ(s.note(smart), "PIT " + id(pit));
    EXPECT_EQ(s.leader(plain).tipRef.entity, pit);
}

TEST(SmartLeaders, ACopyOfAPitWithItsCalloutGetsACalloutOfItsOwn)
{
    Session s;
    const EntityId pit = s.add(PointGeometry{Point2(0, 0)});
    s.run("LEADER #" + id(pit) + " 5,5 template=\"PIT {id}\"");
    const EntityId leader = s.last();
    ASSERT_TRUE(s.document.execute(cmd::copyEntities({pit, leader}, Vec2(100, 0))).ok());
    const auto copies = s.document.lastCreatedEntities();
    ASSERT_EQ(copies.size(), 2u);
    const EntityId pitCopy = copies[0];
    const LeaderGeometry& copy = s.leader(copies[1]);
    EXPECT_EQ(copy.tipRef.entity, pitCopy);
    EXPECT_EQ(copy.vertices.front(), Point2(100, 0));
    EXPECT_EQ(s.note(copies[1]), "PIT " + id(pitCopy));
    EXPECT_EQ(s.note(leader), "PIT " + id(pit)) << "the original is untouched";

    // A callout copied without its pit is another callout of the same pit.
    ASSERT_TRUE(s.document.execute(cmd::copyEntities({leader}, Vec2(0, 50))).ok());
    const LeaderGeometry& alone = s.leader(s.document.lastCreatedEntities().front());
    EXPECT_EQ(alone.tipRef.entity, pit);
    EXPECT_EQ(alone.vertices.front(), Point2(0, 0)) << "its tip stays on the pit";
    // The copy is undone and redone with its reference intact.
    ASSERT_TRUE(s.document.undo().ok());
    ASSERT_TRUE(s.document.undo().ok());
    ASSERT_TRUE(s.document.redo().ok());
    EXPECT_EQ(s.leader(copies[1]).tipRef.entity, pitCopy);
}

TEST(SmartLeaders, TheDrawnNoteIsTheOneReadOff)
{
    Session s;
    const EntityId pit = s.add(PointGeometry{Point2(0, 0)}, {{"invert", 10.5}});
    s.run("LEADER #" + id(pit) + " 5,5 template=\"IL {prop.invert:.2f}\\nRL {rl}\"");
    const auto drawing =
        ann::buildLeader(s.document.model(), s.leader(s.last()), 1000.0, ann::estimatedMeasure());
    ASSERT_EQ(drawing.texts.size(), 1u) << "the RL line has nothing to say";
    EXPECT_EQ(drawing.texts.front().text, "IL 10.50");

    // INFO and LIST say it too.
    const std::string info = s.run("INFO " + id(s.last()));
    EXPECT_TRUE(contains(info, "\"IL 10.50\"")) << info;
    EXPECT_TRUE(contains(info, "template=")) << info;
    EXPECT_TRUE(contains(info, "on " + id(pit))) << info;
}

TEST(SmartLeaders, ADimensionCanBeOnAPlaceAlongALineToo)
{
    Session s;
    const EntityId line = s.add(Segment2{Point2(0, 0), Point2(40, 0)});
    s.run("DIM ALIGNED #" + id(line) + ".start #" + id(line) + "@10,1 at=5,5");
    const EntityId dimension = s.last();
    const auto measured = [&] {
        return std::get<DimensionGeometry>(s.document.model().entities.find(dimension)->geometry)
            .measurement();
    };
    EXPECT_DOUBLE_EQ(measured(), 10.0);
    ASSERT_TRUE(
        s.document.execute(cmd::setEntityGeometry(line, Segment2{Point2(0, 0), Point2(80, 0)}))
            .ok());
    EXPECT_DOUBLE_EQ(measured(), 20.0) << "a quarter of the stretched line";
}

// ---- arranging -------------------------------------------------------------------------------

TEST(SmartLeaders, AlignPutsTheNotesInAColumnTopDown)
{
    Session s;
    s.run("ANNOSCALE 500");
    s.run("LEADER 0,0 10,30 text=A");
    const EntityId a = s.last();
    s.run("LEADER 5,0 22,40 text=B");
    const EntityId b = s.last();
    s.run("LEADER 9,0 13,20 20,10 text=C");
    const EntityId c = s.last();
    // The top note (B, at y 40) sets the column's x; 8 mm at 1:500 is 4 m
    // between them, top down: B 40, A 36, C 32.
    const std::size_t before = s.steps();
    const std::string reply =
        s.run("LEADER ALIGN " + id(a) + " " + id(b) + " " + id(c) + " spacing=8");
    EXPECT_EQ(reply, "aligned leaders=3 x=22 spacing=4");
    EXPECT_EQ(s.steps(), before + 1);
    EXPECT_EQ(s.leader(b).vertices.back(), Point2(22, 40));
    EXPECT_EQ(s.leader(a).vertices.back(), Point2(22, 36));
    EXPECT_EQ(s.leader(c).vertices.back(), Point2(22, 32));
    EXPECT_EQ(s.leader(c).vertices[1], Point2(13, 20)) << "only where the note hangs moves";
    // x= alone keeps each one's height.
    s.run("LEADER ALIGN " + id(a) + " " + id(c) + " x=-5");
    EXPECT_EQ(s.leader(a).vertices.back(), Point2(-5, 36));
    EXPECT_EQ(s.leader(c).vertices.back(), Point2(-5, 32));
    EXPECT_FALSE(s.refusal("LEADER ALIGN " + id(a) + " spacing=0").empty());
}

TEST(SmartLeaders, RenumberingNumbersTheBalloonsInTheOrderAsked)
{
    Session s;
    s.run("BALLOON 30,0 35,5 n=7");
    const EntityId right = s.last();
    s.run("BALLOON 10,20 15,25 n=3");
    const EntityId top = s.last();
    s.run("BALLOON 20,-10 25,-5 n=9");
    const EntityId bottom = s.last();
    const EntityId pit = s.add(PointGeometry{Point2(0, 0)});
    s.run("BALLOON #" + id(pit) + " 5,5 template=\"{id}\"");
    const EntityId smart = s.last();

    EXPECT_EQ(s.run("BALLOON RENUMBER"), "balloons=3 renumbered=3");
    EXPECT_EQ(s.leader(right).text, "1");
    EXPECT_EQ(s.leader(top).text, "2");
    EXPECT_EQ(s.leader(bottom).text, "3");
    EXPECT_TRUE(s.leader(smart).fields) << "a smart balloon is no number in the run";
    // Left to right by the arrows' tips: 10, 20, 30.
    s.run("BALLOON RENUMBER order=x start=101");
    EXPECT_EQ(s.leader(top).text, "101");
    EXPECT_EQ(s.leader(bottom).text, "102");
    EXPECT_EQ(s.leader(right).text, "103");
    // Top down: 20, 0, -10.
    s.run("BALLOON RENUMBER order=y");
    EXPECT_EQ(s.leader(top).text, "1");
    EXPECT_EQ(s.leader(right).text, "2");
    EXPECT_EQ(s.leader(bottom).text, "3");
    EXPECT_EQ(s.run("BALLOON RENUMBER order=y"), "balloons=3 renumbered=0")
        << "numbered as asked already: no step";
    EXPECT_FALSE(s.refusal("BALLOON RENUMBER order=z").empty());
}
