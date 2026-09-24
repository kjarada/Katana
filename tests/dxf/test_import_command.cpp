#include <gtest/gtest.h>

#include <string>

#include "katana/commands/command_stack.hpp"
#include "katana/dxf/import_command.hpp"
#include "katana/dxf/reader.hpp"
#include "katana/entity/model.hpp"

namespace dxf = katana::dxf;

namespace {

dxf::DxfImport load(const char* name)
{
    auto result = dxf::readDxfFile(std::string(KATANA_DXF_TEST_DATA) + "/" + name);
    EXPECT_TRUE(result.ok()) << (result.ok() ? "" : result.error().describe());
    return result.ok() ? std::move(*result) : dxf::DxfImport{};
}

} // namespace

TEST(DxfImportCommand, TheImportIsOneUndoStepThatEntitiesOnALockedLayerDoNotBreak)
{
    katana::entity::Model model;
    katana::commands::CommandStack stack(model);
    auto imported = load("r2000_site.dxf");
    const std::size_t count = imported.entities.size();
    // The file locks layer Boundary, and its polyline is on it: a locked
    // layer refuses new entities, so the lock must come after them.
    auto command = dxf::importCommand(imported, model);
    ASSERT_NE(command, nullptr);
    const auto status = stack.execute(std::move(command));
    ASSERT_TRUE(status.ok()) << status.error().describe();
    EXPECT_EQ(model.entities.size(), count);
    ASSERT_NE(model.layers.find("Boundary"), nullptr);
    EXPECT_TRUE(model.layers.find("Boundary")->locked);
    EXPECT_EQ(model.linetypes.find("CENTER2")->pattern.size(), 4u);
    EXPECT_EQ(stack.undoCount(), 1u);
    ASSERT_TRUE(stack.undo().ok());
    EXPECT_EQ(model.entities.size(), 0u);
    EXPECT_EQ(model.layers.find("Boundary"), nullptr);
    EXPECT_EQ(model.linetypes.find("CENTER2"), nullptr);
}

TEST(DxfImportCommand, ImportingTheSameFileTwiceAddsItsEntitiesAndNoLayerTwice)
{
    katana::entity::Model model;
    katana::commands::CommandStack stack(model);
    for (int time = 0; time < 2; ++time) {
        auto imported = load("r12_survey.dxf");
        auto command = dxf::importCommand(imported, model);
        ASSERT_NE(command, nullptr);
        ASSERT_TRUE(stack.execute(std::move(command)).ok());
    }
    // 11 entities a time; layers 0, KERB, HIDDEN once each.
    EXPECT_EQ(model.entities.size(), 22u);
    EXPECT_EQ(model.layers.size(), 3u);
}

TEST(DxfImportCommand, AFileThatAddsNothingMakesNoCommand)
{
    katana::entity::Model model;
    dxf::DxfImport empty;
    empty.layers.push_back(katana::entity::Layer{}); // layer 0, which the model has
    EXPECT_EQ(dxf::importCommand(empty, model), nullptr);
}

TEST(DxfImportCommand, AFileImportedASecondTimeComesInOnTheLayerTheFirstTimeLocked)
{
    // The first import locks Boundary, as the file says, with the site's
    // polyline on it. The second lands on that layer again: it must come in
    // too, and leave Boundary locked as it found it.
    katana::entity::Model model;
    katana::commands::CommandStack stack(model);
    std::size_t count = 0;
    for (int time = 0; time < 2; ++time) {
        auto imported = load("r2000_site.dxf");
        count = imported.entities.size();
        auto command = dxf::importCommand(imported, model);
        ASSERT_NE(command, nullptr);
        const auto status = stack.execute(std::move(command));
        ASSERT_TRUE(status.ok()) << "import " << time + 1 << ": " << status.error().describe();
        ASSERT_NE(model.layers.find("Boundary"), nullptr);
        EXPECT_TRUE(model.layers.find("Boundary")->locked) << "import " << time + 1;
    }
    EXPECT_EQ(model.entities.size(), 2 * count);
    // The second import is one step too: undone, the first is what is left,
    // with its lock.
    ASSERT_TRUE(stack.undo().ok());
    EXPECT_EQ(model.entities.size(), count);
    EXPECT_TRUE(model.layers.find("Boundary")->locked);
}

TEST(DxfImportCommand, ALayerLockedByItsParentInTheModelTakesTheImportAndStaysLocked)
{
    // The model's "survey" is locked, which locks "survey/kerb" under it; the
    // file adds a point on "survey/kerb". The parent is opened for the point
    // and locked again after it; the new child is not locked of itself.
    katana::entity::Model model;
    katana::entity::Layer survey;
    survey.name = "survey";
    survey.locked = true;
    ASSERT_TRUE(model.layers.add(survey).ok());
    dxf::DxfImport imported;
    katana::entity::Layer kerb;
    kerb.name = "survey/kerb";
    imported.layers.push_back(kerb);
    katana::entity::Entity point;
    point.geometry = katana::entity::PointGeometry{katana::geometry::Point2(1.0, 2.0)};
    point.layer = "survey/kerb";
    imported.entities.push_back(point);
    katana::commands::CommandStack stack(model);
    auto command = dxf::importCommand(imported, model);
    ASSERT_NE(command, nullptr);
    const auto status = stack.execute(std::move(command));
    ASSERT_TRUE(status.ok()) << status.error().describe();
    EXPECT_EQ(model.entities.size(), 1u);
    EXPECT_TRUE(model.layers.find("survey")->locked);
    ASSERT_NE(model.layers.find("survey/kerb"), nullptr);
    EXPECT_FALSE(model.layers.find("survey/kerb")->locked);
}
