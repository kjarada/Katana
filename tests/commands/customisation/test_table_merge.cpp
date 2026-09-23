// The table commands the style and linetype managers need beyond create,
// update, rename and delete: merge, duplicate, purge, and an update that is
// no edit at all - plus the delete guards, which now read entity::tableUsage
// and say how many hold an item rather than only the first.

#include <gtest/gtest.h>

#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/table_usage.hpp"

using namespace katana::commands;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::entity::Layer;
using katana::entity::Linetype;
using katana::entity::Model;
using katana::entity::Style;

namespace {

struct TableFixture : ::testing::Test {
    Model model;
    CommandStack stack{model};

    void must(CommandPtr command)
    {
        const auto status = stack.execute(std::move(command));
        ASSERT_TRUE(status.ok()) << status.error().describe();
    }
    ErrorCode refused(CommandPtr command)
    {
        const auto status = stack.execute(std::move(command));
        EXPECT_FALSE(status.ok()) << "unexpectedly succeeded";
        return status.ok() ? ErrorCode::Internal : status.error().code;
    }
    katana::core::Error refusal(CommandPtr command)
    {
        const auto status = stack.execute(std::move(command));
        EXPECT_FALSE(status.ok()) << "unexpectedly succeeded";
        return status.ok() ? katana::core::Error{} : status.error();
    }
    EntityId point(const char* style, const char* layer = "0")
    {
        EntityAttributes attributes;
        attributes.layer = layer;
        attributes.style = style;
        const auto status =
            stack.execute(createPoint(katana::geometry::Point2(0.0, 0.0), attributes));
        EXPECT_TRUE(status.ok());
        const auto created = stack.lastCreatedEntities();
        return created.empty() ? 0 : created.front();
    }
    static Linetype dashed(const char* name, double dash = 1.0)
    {
        Linetype linetype;
        linetype.name = name;
        linetype.pattern = {{dash}, {-0.5}};
        return linetype;
    }
    static Style style(const char* name, const char* linetype = "continuous")
    {
        Style made;
        made.name = name;
        made.linetype = linetype;
        return made;
    }
    const Entity& entity(EntityId id) const
    {
        static const Entity missing{};
        const Entity* found = model.entities.find(id);
        EXPECT_NE(found, nullptr) << "no entity " << id;
        return found != nullptr ? *found : missing;
    }
};

using TableMerge = TableFixture;
using TableRename = TableFixture;
using TableDuplicate = TableFixture;
using TableGuards = TableFixture;
using TablePurge = TableFixture;
using TableNoOpUpdate = TableFixture;

} // namespace

// ---- merge ---------------------------------------------------------------------------

TEST_F(TableMerge, MergingAStyleMovesEveryWearerOntoTheTargetAndDeletesItInOneUndoStep)
{
    must(createStyle(style("Kerb")));
    must(createStyle(style("Kerb 2")));
    const EntityId a = point("Kerb 2");
    const EntityId b = point("Kerb 2");
    const EntityId c = point("Kerb"); // already on the target
    const std::size_t steps = stack.undoCount();

    must(mergeStyle("Kerb 2", "Kerb"));
    EXPECT_EQ(entity(a).style, "Kerb");
    EXPECT_EQ(entity(b).style, "Kerb");
    EXPECT_EQ(entity(c).style, "Kerb");
    EXPECT_FALSE(model.styles.contains("Kerb 2"));
    EXPECT_EQ(stack.undoCount(), steps + 1) << "one step";
    EXPECT_EQ(stack.undoName(), "MergeStyle");

    // Undo puts back exactly the wearers it moved - not c, which named the
    // target before the merge ran.
    ASSERT_TRUE(stack.undo().ok());
    EXPECT_TRUE(model.styles.contains("Kerb 2"));
    EXPECT_EQ(entity(a).style, "Kerb 2");
    EXPECT_EQ(entity(b).style, "Kerb 2");
    EXPECT_EQ(entity(c).style, "Kerb");

    ASSERT_TRUE(stack.redo().ok());
    EXPECT_FALSE(model.styles.contains("Kerb 2"));
    EXPECT_EQ(entity(a).style, "Kerb");
}

TEST_F(TableMerge, MergingALinetypeRepointsEveryLayerAndStyleNamingIt)
{
    must(createLinetype(dashed("fence")));
    must(createLinetype(dashed("fence old", 2.0)));
    Layer survey;
    survey.name = "survey";
    survey.linetype = "fence old";
    must(createLayer(survey));
    must(createStyle(style("Post", "fence old")));
    must(createStyle(style("Rail", "fence"))); // already on the target

    must(mergeLinetype("fence old", "fence"));
    EXPECT_EQ(model.layers.find("survey")->linetype, "fence");
    EXPECT_EQ(model.styles.find("Post")->linetype, "fence");
    EXPECT_FALSE(model.linetypes.contains("fence old"));

    ASSERT_TRUE(stack.undo().ok());
    EXPECT_EQ(model.layers.find("survey")->linetype, "fence old");
    EXPECT_EQ(model.styles.find("Post")->linetype, "fence old");
    EXPECT_EQ(model.styles.find("Rail")->linetype, "fence") << "it was never moved";
    ASSERT_TRUE(model.linetypes.contains("fence old"));
    EXPECT_DOUBLE_EQ(model.linetypes.find("fence old")->pattern.front().length, 2.0)
        << "the definition comes back as it was, not as a copy of the target";
}

TEST_F(TableMerge, AMergeRefusesAProtectedSourceAMissingTargetAndItself)
{
    must(createLinetype(dashed("fence")));
    must(createStyle(style("Kerb")));
    EXPECT_EQ(refused(mergeLinetype("continuous", "fence")), ErrorCode::CommandRejected)
        << "everything that resolves to continuous would change what it draws";
    EXPECT_EQ(refused(mergeLinetype("fence", "nosuch")), ErrorCode::NotFound);
    EXPECT_EQ(refused(mergeLinetype("nosuch", "fence")), ErrorCode::NotFound);
    EXPECT_EQ(refused(mergeStyle("Kerb", "nosuch")), ErrorCode::NotFound);
    EXPECT_EQ(refused(mergeStyle("Kerb", "Kerb")), ErrorCode::InvalidArgument);
    // A refused merge changes nothing.
    EXPECT_TRUE(model.linetypes.contains("fence"));
    EXPECT_TRUE(model.styles.contains("Kerb"));
}

TEST_F(TableMerge, MergeDeleteAndPurgeAreDestructiveAndDuplicateIsNot)
{
    // The flag is the confirmation gate for commands an AI proposes
    // (command.hpp): discarding a definition must ask (audit MOD-08).
    EXPECT_TRUE(mergeStyle("a", "b")->isDestructive());
    EXPECT_TRUE(mergeLinetype("a", "b")->isDestructive());
    EXPECT_TRUE(deleteStyle("a")->isDestructive());
    EXPECT_TRUE(deleteLinetype("a")->isDestructive());
    EXPECT_TRUE(deleteHatchPattern("a")->isDestructive());
    EXPECT_TRUE(deleteDimensionStyle("a")->isDestructive());
    EXPECT_TRUE(deleteAlignment("a")->isDestructive());
    EXPECT_TRUE(purgeTableItems(TableItems{{"a"}, {}, {}})->isDestructive());
    EXPECT_FALSE(duplicateStyle("a", "b")->isDestructive());
    EXPECT_FALSE(updateStyle(style("a"))->isDestructive());
}

// ---- rename ------------------------------------------------------------------------------

TEST_F(TableRename, UndoingALinetypeRenameOntoANameAlreadyHeldPutsBackOnlyWhatTheRenameMoved)
{
    // The commands layer does not see the 12d library, so a style or layer
    // may name a linetype the model lacks - a library linestyle (D2), which
    // STYLE SET now accepts. The rename's validate asks only the model
    // table, so a model linetype can be renamed onto that name. Its undo
    // used to repoint EVERYTHING naming the new name back to the old one,
    // rewriting s and survey, which the rename never touched, to fence.
    must(createLinetype(dashed("fence")));
    must(createStyle(style("s", "COMM Telephone Pole")));
    must(createStyle(style("t", "fence")));
    Layer survey;
    survey.name = "survey";
    survey.linetype = "COMM Telephone Pole";
    must(createLayer(survey));
    Layer kerbs;
    kerbs.name = "kerbs";
    kerbs.linetype = "fence";
    must(createLayer(kerbs));

    must(renameLinetype("fence", "COMM Telephone Pole"));
    EXPECT_EQ(model.styles.find("t")->linetype, "COMM Telephone Pole");
    EXPECT_EQ(model.layers.find("kerbs")->linetype, "COMM Telephone Pole");

    ASSERT_TRUE(stack.undo().ok());
    EXPECT_TRUE(model.linetypes.contains("fence"));
    EXPECT_FALSE(model.linetypes.contains("COMM Telephone Pole"));
    // Moved by the rename, so moved back:
    EXPECT_EQ(model.styles.find("t")->linetype, "fence");
    EXPECT_EQ(model.layers.find("kerbs")->linetype, "fence");
    // Named the new name before the rename ran, so left alone:
    EXPECT_EQ(model.styles.find("s")->linetype, "COMM Telephone Pole");
    EXPECT_EQ(model.layers.find("survey")->linetype, "COMM Telephone Pole");

    // Redo moves the same holders again; a second undo is as exact.
    ASSERT_TRUE(stack.redo().ok());
    EXPECT_EQ(model.styles.find("t")->linetype, "COMM Telephone Pole");
    ASSERT_TRUE(stack.undo().ok());
    EXPECT_EQ(model.styles.find("t")->linetype, "fence");
    EXPECT_EQ(model.styles.find("s")->linetype, "COMM Telephone Pole");
}

TEST_F(TableRename, UndoingAStyleRenameOntoANameEntitiesAlreadyWorePutsBackOnlyItsOwnWearers)
{
    // An entity can wear a style the table lacks - a project saved before
    // its style was removed outside Katana; STYLE USAGE lists it as "not in
    // the style table". Added straight to the database, since every command
    // refuses to create one.
    must(createStyle(style("Kerb")));
    const EntityId moved = point("Kerb");
    Entity orphan;
    orphan.layer = "0";
    orphan.style = "Ghost";
    orphan.geometry = katana::entity::PointGeometry{katana::geometry::Point2(1.0, 1.0)};
    const auto added = model.entities.add(orphan);
    ASSERT_TRUE(added.ok());
    const EntityId stranded = *added;

    must(renameStyle("Kerb", "Ghost"));
    EXPECT_EQ(entity(moved).style, "Ghost");

    ASSERT_TRUE(stack.undo().ok());
    EXPECT_TRUE(model.styles.contains("Kerb"));
    EXPECT_FALSE(model.styles.contains("Ghost"));
    EXPECT_EQ(entity(moved).style, "Kerb");
    EXPECT_EQ(entity(stranded).style, "Ghost") << "it wore Ghost before the rename ran";
}

// ---- duplicate ---------------------------------------------------------------------------

TEST_F(TableDuplicate, ADuplicateIsACopyUnderTheNewNameAndUndoRemovesIt)
{
    Style kerb = style("Kerb", "continuous");
    kerb.lineWeight = 0.7;
    kerb.symbol = "cross";
    must(createStyle(kerb));
    must(duplicateStyle("Kerb", "Kerb red"));
    ASSERT_TRUE(model.styles.contains("Kerb red"));
    Style expected = kerb;
    expected.name = "Kerb red";
    EXPECT_EQ(*model.styles.find("Kerb red"), expected);
    EXPECT_EQ(refused(duplicateStyle("Kerb", "Kerb red")), ErrorCode::AlreadyExists);
    EXPECT_EQ(refused(duplicateStyle("nosuch", "x")), ErrorCode::NotFound);
    ASSERT_TRUE(stack.undo().ok());
    EXPECT_FALSE(model.styles.contains("Kerb red"));

    must(createLinetype(dashed("fence")));
    must(duplicateLinetype("fence", "fence 2"));
    EXPECT_EQ(model.linetypes.find("fence 2")->pattern, model.linetypes.find("fence")->pattern);
}

// ---- the guards ------------------------------------------------------------------------------

TEST_F(TableGuards, DeletingAStyleInUseGivesTheCountAndTheFirstWearer)
{
    must(createStyle(style("Kerb")));
    const EntityId first = point("Kerb");
    point("Kerb");
    point("Kerb");
    const auto error = refusal(deleteStyle("Kerb"));
    EXPECT_EQ(error.code, ErrorCode::CommandRejected);
    // Three wearers; the first is the lowest id.
    EXPECT_EQ(error.context, "used by 3 entities, e.g. id=" + std::to_string(first));
}

TEST_F(TableGuards, DeletingALinetypeInUseCountsTheLayersStylesAndEntitiesReachingIt)
{
    must(createLinetype(dashed("fence")));
    Layer survey;
    survey.name = "survey";
    survey.linetype = "fence";
    must(createLayer(survey));
    must(createStyle(style("Post", "fence")));
    // Two points on survey with no style reach fence through the layer; a
    // third wears Post and reaches it through the style.
    point("", "survey");
    point("", "survey");
    point("Post");
    const auto error = refusal(deleteLinetype("fence"));
    EXPECT_EQ(error.code, ErrorCode::CommandRejected);
    EXPECT_EQ(error.context, "used by 1 layer, 1 style and 3 entities, e.g. layer=survey");
}

TEST_F(TableGuards, UpdatingContinuousToAPatternIsRefusedByValidateNotOnlyByExecute)
{
    // audit MOD-09: validate() used to say yes, and execute() then said no -
    // a caller that pre-validates (the AI safety model) was misled.
    auto command = updateLinetype(dashed("continuous"));
    CommandContext context{model};
    const auto validated = command->validate(context);
    ASSERT_FALSE(validated.ok());
    EXPECT_EQ(validated.error().code, ErrorCode::InvalidArgument);
}

// ---- no-op updates ---------------------------------------------------------------------------

TEST_F(TableNoOpUpdate, SavingAnUneditedStylePushesNoUndoStepAndChangesNothing)
{
    // The natural QT-02 regression base: the style manager's Save on a form
    // nobody touched must leave the style byte-identical and the history as
    // it was - which is also what stops it marking the project modified.
    Style imported = style("WATR Main", "WATR Main");
    imported.symbol = "CULT Bollard";
    imported.description = "12d linestyle";
    must(createStyle(imported));
    const std::size_t steps = stack.undoCount();

    EXPECT_EQ(updateStyleIfChanged(model, imported), nullptr);
    EXPECT_EQ(updateLinetypeIfChanged(model, *model.linetypes.find("continuous")), nullptr);
    EXPECT_EQ(stack.undoCount(), steps);
    EXPECT_EQ(*model.styles.find("WATR Main"), imported);

    // A real change still gets a command, and it is undoable.
    Style heavier = imported;
    heavier.lineWeight = 0.5;
    auto command = updateStyleIfChanged(model, heavier);
    ASSERT_NE(command, nullptr);
    must(std::move(command));
    EXPECT_EQ(stack.undoCount(), steps + 1);
    // A missing name still gets a command, whose validate says NotFound.
    auto missing = updateStyleIfChanged(model, style("nosuch"));
    ASSERT_NE(missing, nullptr);
    EXPECT_EQ(refused(std::move(missing)), ErrorCode::NotFound);
}

// ---- purge ----------------------------------------------------------------------------------

TEST_F(TablePurge, PurgingASetDeletesItAllAsOneStepAndOneUndoRestoresIt)
{
    must(createLinetype(dashed("fence")));
    must(createStyle(style("Post", "fence")));
    must(createStyle(style("Spare")));
    const std::size_t steps = stack.undoCount();

    // fence is named only by Post, which goes in the same set: it is free.
    must(purgeTableItems(TableItems{{"Post", "Spare"}, {"fence"}, {}}));
    EXPECT_FALSE(model.styles.contains("Post"));
    EXPECT_FALSE(model.styles.contains("Spare"));
    EXPECT_FALSE(model.linetypes.contains("fence"));
    EXPECT_EQ(stack.undoCount(), steps + 1);
    EXPECT_EQ(stack.undoName(), "PurgeTables");

    ASSERT_TRUE(stack.undo().ok());
    EXPECT_TRUE(model.styles.contains("Post"));
    EXPECT_TRUE(model.styles.contains("Spare"));
    EXPECT_EQ(model.styles.find("Post")->linetype, "fence");
    EXPECT_TRUE(model.linetypes.contains("fence"));
}

TEST_F(TablePurge, APurgeRefusesAnythingStillUsedOnceTheSetIsGoneAndChangesNothing)
{
    must(createLinetype(dashed("fence")));
    must(createStyle(style("Post", "fence")));
    must(createStyle(style("Spare")));
    // Post stays, so fence is held.
    const auto held = refusal(purgeTableItems(TableItems{{"Spare"}, {"fence"}, {}}));
    EXPECT_EQ(held.code, ErrorCode::CommandRejected);
    EXPECT_EQ(held.context, "fence: used by 1 style, e.g. style=Post");
    EXPECT_TRUE(model.styles.contains("Spare")) << "a refused purge removes nothing";

    point("Spare");
    EXPECT_EQ(refused(purgeTableItems(TableItems{{"Spare"}, {}, {}})),
              ErrorCode::CommandRejected);
    EXPECT_EQ(refused(purgeTableItems(TableItems{{"nosuch"}, {}, {}})), ErrorCode::NotFound);
    EXPECT_EQ(refused(purgeTableItems(TableItems{})), ErrorCode::InvalidArgument)
        << "never an empty undo step";
}

TEST_F(TablePurge, ContinuousAndNoneAreRefusedByValidateEvenWhenNothingNamesThem)
{
    // Layer 0 names continuous and none by default, so a refusal of either
    // could come from "still used" alone and never reach the protection.
    // Layer 0 is pointed elsewhere first, so only the protection can refuse
    // them - and it has to in validate(), where a caller that pre-validates
    // looks; the table's own refusal comes only at execute().
    must(createLinetype(dashed("fence")));
    katana::entity::HatchPattern stone;
    stone.name = "stone";
    stone.families = {{0.0, 1.0, 0.0}};
    must(createHatchPattern(stone));
    Layer zero = *model.layers.find("0");
    zero.linetype = "fence";
    zero.hatchPattern = "stone";
    must(updateLayer(zero));
    const katana::entity::TableUsage usage = katana::entity::tableUsage(model);
    ASSERT_FALSE(katana::entity::TableUsage::of(usage.linetypes, "continuous").used());
    ASSERT_FALSE(katana::entity::TableUsage::of(usage.hatchPatterns, "none").used());

    CommandContext context{model};
    const auto continuous = purgeTableItems(TableItems{{}, {"continuous"}, {}})->validate(context);
    ASSERT_FALSE(continuous.ok());
    EXPECT_EQ(continuous.error().code, ErrorCode::CommandRejected);
    const auto none = purgeTableItems(TableItems{{}, {}, {"none"}})->validate(context);
    ASSERT_FALSE(none.ok());
    EXPECT_EQ(none.error().code, ErrorCode::CommandRejected);
}
