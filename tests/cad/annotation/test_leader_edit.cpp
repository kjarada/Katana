// The leader edits the LEADER verbs and the window's Leaders manager share
// (cad/annotation/leader_edit.hpp, docs/annotation.md "Smart leaders"): each
// checked against the model and made ONE command, or refused in the words
// the command line replies with.

#include <gtest/gtest.h>

#include <string>

#include "katana/cad/annotation/leader_edit.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"

using namespace katana::cad;
using namespace katana::entity;
using katana::geometry::Point2;
using katana::geometry::Segment2;
namespace ann = katana::cad::annotation;
namespace cmd = katana::commands;

namespace {

EntityId add(Document& document, Geometry geometry, PropertyMap properties = {})
{
    Entity entity;
    entity.geometry = std::move(geometry);
    entity.properties = std::move(properties);
    EXPECT_TRUE(document.execute(cmd::createEntities({std::move(entity)})).ok());
    return document.lastCreatedEntities().front();
}

const LeaderGeometry& leaderOf(const Document& document, EntityId id)
{
    return std::get<LeaderGeometry>(document.model().entities.find(id)->geometry);
}

ann::LeaderChange templateChange(std::string text)
{
    ann::LeaderChange change;
    change.note = ann::LeaderNote{ann::LeaderNote::Kind::Template, std::move(text)};
    return change;
}

} // namespace

TEST(LeaderEdit, AnUnchangedEditIsNoCommandAndAChangeIsOne)
{
    Document document;
    const EntityId pit = add(document, PointGeometry{Point2(0, 0)}, {{"invert", 10.5}});
    auto made = ann::newLeader(
        document.model(),
        {ann::AnchoredPoint{Point2(0, 0), AnchorRef{pit}}, ann::AnchoredPoint{Point2(5, 5), {}}},
        templateChange("IL {prop.invert:.2f}"), false);
    ASSERT_TRUE(made.ok()) << made.error().describe();
    const EntityId leader = add(document, *made);
    EXPECT_EQ(ann::leaderSays(document.model(), leaderOf(document, leader)), "IL 10.50");
    EXPECT_EQ(ann::noteOf(leaderOf(document, leader)),
              (ann::LeaderNote{ann::LeaderNote::Kind::Template, "IL {prop.invert:.2f}"}));

    auto same =
        ann::changeLeaders(document.model(), {leader}, templateChange("IL {prop.invert:.2f}"));
    ASSERT_TRUE(same.ok());
    EXPECT_EQ(*same, nullptr) << "the same template again changes nothing";

    ann::LeaderChange look;
    look.arrow = ArrowHead::Open;
    look.paperHeight = 3.5;
    auto changed = ann::changeLeaders(document.model(), {leader, leader}, look);
    ASSERT_TRUE(changed.ok());
    ASSERT_NE(*changed, nullptr);
    const std::size_t steps = document.history().undoCount();
    ASSERT_TRUE(document.execute(std::move(*changed)).ok());
    EXPECT_EQ(document.history().undoCount(), steps + 1);
    EXPECT_EQ(leaderOf(document, leader).arrow, ArrowHead::Open);
    EXPECT_EQ(leaderOf(document, leader).paperHeight, 3.5);
    EXPECT_TRUE(leaderOf(document, leader).fields) << "the note is left as it was";

    EXPECT_FALSE(ann::changeLeaders(document.model(), {pit}, look).ok()) << "a point is no leader";
    EXPECT_FALSE(ann::changeLeaders(document.model(), {}, look).ok());
}

TEST(LeaderEdit, WhatCannotBeMadeIsRefusedNotSkipped)
{
    Document document;
    const EntityId a = add(document, PointGeometry{Point2(0, 0)});
    const EntityId b = add(document, PointGeometry{Point2(10, 0)}, {{"invert", 9.0}});
    ann::LeadersForOptions options;
    options.change = templateChange("{invert}"); // no such value: every leader would be wrong
    auto refused = ann::leadersFor(document.model(), {a, b}, options, 1000.0, {});
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().context.find("no value 'invert'"), std::string::npos)
        << refused.error().describe();

    // A note one target cannot say is that target skipped.
    options.change = templateChange("IL {prop.invert}");
    auto made = ann::leadersFor(document.model(), {a, b}, options, 1000.0, {});
    ASSERT_TRUE(made.ok()) << made.error().describe();
    EXPECT_EQ(made->made, 1u);
    EXPECT_EQ(made->skipped, 1u);
    EXPECT_NE(made->firstSkip.find("no prop.invert"), std::string::npos) << made->firstSkip;

    ann::LeaderChange missingStyle;
    missingStyle.textStyle = "Nowhere";
    EXPECT_EQ(ann::checkLeaderChange(document.model(), missingStyle).error().code,
              katana::core::ErrorCode::NotFound);
}

TEST(LeaderEdit, ALabelStyleLendsItsLookUnlessTheChangeGivesOne)
{
    Document document;
    TextStyle notes;
    notes.name = "Notes";
    ASSERT_TRUE(document.execute(cmd::createTextStyle(notes)).ok());
    LabelStyle invert;
    invert.name = "Invert";
    invert.kind = LabelKind::Point;
    invert.text = "{prop.invert}";
    invert.textStyle = "Notes";
    invert.paperHeight = 3.0;
    ASSERT_TRUE(document.execute(cmd::createLabelStyle(invert)).ok());
    const EntityId pit = add(document, PointGeometry{Point2(0, 0)}, {{"invert", 9.5}});
    LeaderGeometry leader{.vertices = {Point2(0, 0), Point2(5, 5)}, .tipRef = AnchorRef{pit}};
    ann::LeaderChange change;
    change.note = ann::LeaderNote{ann::LeaderNote::Kind::LabelStyle, "Invert"};
    ASSERT_TRUE(ann::applyLeaderChange(document.model(), change, leader).ok());
    EXPECT_EQ(leader.style, "Notes");
    EXPECT_EQ(leader.paperHeight, 3.0);

    LeaderGeometry own{.vertices = {Point2(0, 0), Point2(5, 5)}, .tipRef = AnchorRef{pit}};
    change.textStyle = "";
    change.paperHeight = 2.0;
    ASSERT_TRUE(ann::applyLeaderChange(document.model(), change, own).ok());
    EXPECT_EQ(own.style, "") << "given: the default face";
    EXPECT_EQ(own.paperHeight, 2.0);
}

TEST(LeaderEdit, AttachingNeedsAnEntityAndTheValuesAreRowsInOrder)
{
    Document document;
    const EntityId line = add(document, Segment2{Point2(0, 0), Point2(40, 0)}, {{"size", 150.0}});
    const EntityId leader =
        add(document, LeaderGeometry{.vertices = {Point2(9, 9), Point2(20, 20)}, .text = "X"});
    EXPECT_FALSE(
        ann::attachLeader(document.model(), leader, ann::AnchoredPoint{Point2(1, 1), {}}).ok())
        << "a plain point is nothing to attach to";
    auto attach = ann::attachLeader(
        document.model(), leader,
        ann::AnchoredPoint{Point2(10, 0), AnchorRef{line, AnchorPoint::Along, 0, 0.25}});
    ASSERT_TRUE(attach.ok());
    ASSERT_TRUE(document.execute(std::move(*attach)).ok());
    EXPECT_EQ(leaderOf(document, leader).vertices.front(), Point2(10, 0));

    const auto values = ann::leaderTargetValues(document.model(), leaderOf(document, leader));
    ASSERT_TRUE(values);
    const auto rows = ann::leaderValueRows(*values);
    ASSERT_FALSE(rows.empty());
    EXPECT_EQ(rows.front().name, "id") << "leaderValueNames order";
    EXPECT_EQ(rows.back().name, "prop.size") << "then the properties";
    EXPECT_EQ(rows.back().text, "150.000");

    auto property =
        ann::setLeaderTargetProperty(document.model(), leader, "size", PropertyValue{200.0});
    ASSERT_TRUE(property.ok());
    ASSERT_TRUE(document.execute(std::move(*property)).ok());
    EXPECT_EQ(std::get<double>(document.model().entities.find(line)->properties.at("size")), 200.0);
    EXPECT_FALSE(
        ann::setLeaderTargetProperty(document.model(), leader, "", PropertyValue{1.0}).ok());
}
