#include <gtest/gtest.h>

#include <algorithm>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <numbers>
#include <string>

#include "katana/archive12d/domain.hpp"
#include "katana/archive12d/reader.hpp"
#include "katana/archive12d/writer.hpp"
#include "katana/entity/geometry_blob.hpp"
#include "katana/entity/serialization.hpp"

namespace a12 = katana::archive12d;
using katana::entity::LeaderGeometry;
using katana::geometry::Point2;

namespace {

std::string fixture()
{
    std::ifstream input(std::filesystem::path(KATANA_ARCHIVE12D_TEST_DATA) / "drafting_leaders.12da");
    EXPECT_TRUE(input.good());
    return {std::istreambuf_iterator<char>(input), std::istreambuf_iterator<char>()};
}

bool warns(const a12::DomainImport& imported, std::string_view text)
{
    return std::ranges::any_of(imported.warnings, [text](const std::string& warning) {
        return warning.find(text) != std::string::npos;
    });
}

} // namespace

TEST(ArchiveDrafting, LeaderBlocksAreReadAndRewrittenWithoutLosingTheirFields)
{
    const auto archive = a12::readArchive(fixture());
    ASSERT_TRUE(archive.ok()) << archive.error().describe();
    ASSERT_EQ(archive->elements.size(), 4u);
    EXPECT_TRUE(archive->unrecognised.empty());
    const auto* leader = std::get_if<a12::DraftingLeader>(&archive->elements[2]);
    ASSERT_NE(leader, nullptr);
    EXPECT_EQ(leader->header.model, "Test/Notes");
    EXPECT_EQ(leader->arrow.x, 100);
    EXPECT_EQ(leader->hook.y, 208);
    EXPECT_EQ(leader->fields.text("display_text"), "Service note\\nFirst line");
    EXPECT_EQ(leader->overrides.real("text_size"), 1.25);
    const auto again = a12::readArchive(a12::writeArchive(*archive));
    ASSERT_TRUE(again.ok()) << again.error().describe();
    EXPECT_EQ(again->elements, archive->elements);
}

TEST(ArchiveDrafting, ImportedLeadersKeepTheirNotesPaperSizesSidesAndLocalPlacement)
{
    const auto archive = a12::readArchive(fixture());
    ASSERT_TRUE(archive.ok());
    a12::ImportOptions options;
    options.originShift = katana::geometry::Vec2(100, 200);
    const auto imported = a12::toDomain(*archive, options);
    ASSERT_TRUE(imported.ok()) << imported.error().describe();
    ASSERT_EQ(imported->entities.size(), 4u);
    const auto* left = std::get_if<LeaderGeometry>(&imported->entities[2].geometry);
    const auto* right = std::get_if<LeaderGeometry>(&imported->entities[3].geometry);
    ASSERT_NE(left, nullptr);
    ASSERT_NE(right, nullptr);
    EXPECT_EQ(left->vertices, (std::vector<Point2>{Point2(0, 0), Point2(0, 8)}));
    EXPECT_EQ(left->text, "Service note\nFirst line");
    EXPECT_DOUBLE_EQ(left->paperHeight, 1.25);
    EXPECT_DOUBLE_EQ(left->arrowSize, 1);
    EXPECT_DOUBLE_EQ(left->landing, 2.5);
    EXPECT_EQ(left->landingAngle, 0.0);
    EXPECT_EQ(right->text, "Other note") << "display text, not an unevaluated external template";
    EXPECT_EQ(right->landingAngle, std::numbers::pi);
    EXPECT_DOUBLE_EQ(right->paperHeight, 2);
    EXPECT_DOUBLE_EQ(right->arrowSize, 2);
    EXPECT_DOUBLE_EQ(right->landing, 5);
    EXPECT_EQ(imported->entities[2].layer, "Test/Notes");
    const auto tally = std::ranges::find_if(imported->tally, [](const a12::ElementTally& row) {
        return row.keyword == "drafting_element/leader";
    });
    ASSERT_NE(tally, imported->tally.end());
    EXPECT_EQ(tally->read, 2u);
    EXPECT_EQ(tally->imported, 2u);
    EXPECT_FALSE(warns(*imported, "DIFFERENT symbol"));
    const auto blob = katana::entity::geometryToBlob(*left);
    ASSERT_TRUE(blob.ok());
    const auto stored = katana::entity::geometryFromBlob(*blob);
    ASSERT_TRUE(stored.ok());
    EXPECT_EQ(std::get<LeaderGeometry>(*stored), *left);
    const auto json = katana::entity::geometryToJson(*right);
    ASSERT_TRUE(json.ok());
    const auto restored = katana::entity::geometryFromJson(*json);
    ASSERT_TRUE(restored.ok());
    EXPECT_EQ(std::get<LeaderGeometry>(*restored), *right);
}

TEST(ArchiveDrafting, MissingOrNonFiniteCoordinatesDoNotCreateAnInventedLeaderAtTheOrigin)
{
    for (const char* coordinates :
         {"arrow_y 2 hook_x 3 hook_y 4", "arrow_x nan arrow_y 2 hook_x 3 hook_y 4",
          "arrow_x 1 arrow_y 2 hook_x inf hook_y 4"}) {
        const auto archive = a12::readArchive(
            std::string("drafting_element { leader { ") + coordinates + " } } "
            "string super { data_2d { 7 9 } }");
        ASSERT_TRUE(archive.ok()) << archive.error().describe();
        const auto imported = a12::toDomain(*archive);
        ASSERT_TRUE(imported.ok());
        ASSERT_EQ(imported->entities.size(), 1u);
        EXPECT_TRUE(std::holds_alternative<katana::entity::PointGeometry>(
            imported->entities[0].geometry));
        EXPECT_FALSE(imported->warnings.empty());
    }
}

TEST(ArchiveDrafting, InvalidSizesAnglesJustificationAndCoincidentTipsAreReported)
{
    for (const char* fields :
         {"hook_angle nan", "justify_x Unknown", "overrides { text_size -1 }",
          "overrides { arrow_size inf }", "overrides { hook_length nope }"}) {
        const auto archive = a12::readArchive(
            std::string("drafting_element { leader { arrow_x 0 arrow_y 0 hook_x 3 hook_y 4 ") +
            fields + " } }");
        ASSERT_TRUE(archive.ok());
        const auto imported = a12::toDomain(*archive);
        ASSERT_TRUE(imported.ok());
        EXPECT_TRUE(imported->entities.empty()) << fields;
        EXPECT_FALSE(imported->warnings.empty()) << fields;
    }
    for (const char* coordinates : {"arrow_x 1 arrow_y 2 hook_x 1 hook_y 2",
                                   "arrow_x -1e308 arrow_y 0 hook_x 1e308 hook_y 0"}) {
        const auto archive = a12::readArchive(
            std::string("drafting_element { leader { ") + coordinates + " } }");
        ASSERT_TRUE(archive.ok());
        const auto imported = a12::toDomain(*archive);
        ASSERT_TRUE(imported.ok());
        EXPECT_TRUE(imported->entities.empty());
        EXPECT_FALSE(imported->warnings.empty());
    }
}

TEST(ArchiveDrafting, EmptyNotesAndZeroSizesAreValidAndUnknownNestedBlocksAreCounted)
{
    const auto archive = a12::readArchive(R"(
        drafting_element {
          leader {
            arrow_x 1 arrow_y 2 hook_x 3 hook_y 4 hook_angle 90
            display_text ""
            overrides { arrow_size 0 hook_length 0 text_size 0 }
            future_setting { value 1 }
          }
        })");
    ASSERT_TRUE(archive.ok()) << archive.error().describe();
    EXPECT_EQ(archive->unrecognised.at("drafting_element/leader/future_setting"), 1u);
    const auto imported = a12::toDomain(*archive);
    ASSERT_TRUE(imported.ok());
    ASSERT_EQ(imported->entities.size(), 1u);
    const auto& leader = std::get<LeaderGeometry>(imported->entities[0].geometry);
    EXPECT_TRUE(leader.text.empty());
    EXPECT_EQ(leader.landingAngle, std::numbers::pi / 2);
    EXPECT_DOUBLE_EQ(leader.arrowSize, 0);
    EXPECT_DOUBLE_EQ(leader.landing, 0);
    EXPECT_DOUBLE_EQ(leader.paperHeight, 0);
}

TEST(ArchiveDrafting, MissingAndDuplicateLeaderBlocksAreNotReportedAsSuccessfulImports)
{
    const auto empty = a12::readArchive("drafting_element { time_created \"today\" }");
    ASSERT_TRUE(empty.ok());
    EXPECT_TRUE(empty->elements.empty());
    EXPECT_FALSE(empty->warnings.empty());
    const auto duplicate = a12::readArchive(
        "drafting_element { leader { arrow_x 0 arrow_y 0 hook_x 1 hook_y 1 } "
        "leader { arrow_x 1 arrow_y 1 hook_x 2 hook_y 2 } }");
    EXPECT_FALSE(duplicate.ok());
    const auto truncated = a12::readArchive("drafting_element { leader { arrow_x 0");
    EXPECT_FALSE(truncated.ok());
}

TEST(ArchiveDrafting, ExternalArrowReferencesAreRetainedWithoutGuessingATargetEntity)
{
    const auto archive = a12::readArchive(R"(
        drafting_element {
          leader {
            arrow_x 1 arrow_y 2 hook_x 3 hook_y 4 display_text "Snapshot"
            arrow {
              computator_horz_point_reference {
                valid true
                computator_horz_reference {
                  valid true direction 1
                  reference { model_name "Original services" model_id 12 name "Service 8" id 81 }
                  part { part 3 }
                }
              }
            }
          }
        })");
    ASSERT_TRUE(archive.ok()) << archive.error().describe();
    EXPECT_TRUE(archive->unrecognised.empty());
    const auto again = a12::readArchive(a12::writeArchive(*archive));
    ASSERT_TRUE(again.ok()) << again.error().describe();
    EXPECT_EQ(again->elements, archive->elements);
    const auto imported = a12::toDomain(*archive);
    ASSERT_TRUE(imported.ok());
    ASSERT_EQ(imported->entities.size(), 1u);
    const auto& entity = imported->entities.front();
    const auto& leader = std::get<LeaderGeometry>(entity.geometry);
    EXPECT_FALSE(leader.tipRef.associated());
    EXPECT_EQ(leader.vertices.front(), Point2(1, 2));
    EXPECT_EQ(std::get<std::string>(entity.metadata.at(
                  "12d.leader.arrow.computator_horz_point_reference[0].computator_horz_reference[0].reference[0].name")),
              "Service 8");
    EXPECT_TRUE(warns(*imported, "external arrow references"));
}
