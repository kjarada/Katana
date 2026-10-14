// The drawing's surfaces in a project (schema 13; docs/terrain.md, "Surfaces
// in the drawing"): opaque records kept in the same transaction as the rest,
// in their order, every field as it was - and refused before a byte is
// written when a load could not give them back.

#include <gtest/gtest.h>

#include <filesystem>
#include <string>
#include <system_error>
#include <vector>

#include "katana/storage/project_store.hpp"
#include "katana/storage/sqlite_database.hpp"

using namespace katana::storage;
namespace fs = std::filesystem;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::entity::Model;
using katana::geometry::Point2;
using katana::geometry::Segment2;

namespace {

class SurfaceStorage : public ::testing::Test {
  protected:
    void SetUp() override
    {
        const auto* info = ::testing::UnitTest::GetInstance()->current_test_info();
        root_ = fs::temp_directory_path() / "katana-storage-tests" /
                (std::string(info->test_suite_name()) + "." + info->name());
        std::error_code ignored;
        fs::remove_all(root_, ignored);
        fs::create_directories(root_);
    }
    void TearDown() override
    {
        std::error_code ignored;
        fs::remove_all(root_, ignored);
    }

    [[nodiscard]] fs::path projectDir() const { return root_ / "site.katana"; }

    fs::path root_;
};

// Bytes a TEXT column would mangle - a NUL, a lone CR, bytes that are not
// UTF-8 - since the record's data is the terrain layer's and opaque here.
SurfaceRecord record(std::string name, std::string style, bool visible)
{
    SurfaceRecord surface;
    surface.name = std::move(name);
    surface.source = "point cloud site.laz";
    surface.style = std::move(style);
    surface.visible = visible;
    surface.data = std::string("KTIN\0\r\xff\xfe opaque", 15) + surface.name;
    return surface;
}

ProjectContents contentsWith(std::vector<SurfaceRecord> surfaces)
{
    Model model;
    Entity line;
    line.geometry = Segment2{Point2(0, 0), Point2(10, 0)};
    EXPECT_TRUE(model.entities.add(line).ok());
    ProjectContents contents = captureModel(model, {});
    contents.surfaces = std::move(surfaces);
    return contents;
}

} // namespace

TEST_F(SurfaceStorage, SurfacesSurviveSavingTwiceAndReopeningInTheirOrderWithEveryField)
{
    const std::vector<SurfaceRecord> surfaces{record("ground", "", true),
                                              record("design pad", "Contours", false),
                                              record("Alpha", "Cut Fill", true)};
    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store->save(contentsWith(surfaces)).ok());
        // Twice: a table missing from save's DELETE list fails the second on
        // its primary key, not the first.
        const auto again = store->save(contentsWith(surfaces));
        ASSERT_TRUE(again.ok()) << again.error().describe();
    }
    auto store = ProjectStore::open(projectDir());
    ASSERT_TRUE(store.ok()) << store.error().describe();
    EXPECT_EQ(*store->schemaVersion(), 13);
    const auto loaded = store->load();
    ASSERT_TRUE(loaded.ok()) << loaded.error().describe();
    // In the order they were saved - "Alpha" last, not sorted first.
    EXPECT_EQ(loaded->surfaces, surfaces);
    EXPECT_EQ(loaded->entities.size(), 1U) << "saved in the one transaction with the drawing";
}

TEST_F(SurfaceStorage, ASaveWithNoSurfacesLeavesNoneOfTheLastSavesBehind)
{
    auto store = ProjectStore::create(projectDir(), {});
    ASSERT_TRUE(store.ok());
    ASSERT_TRUE(store->save(contentsWith({record("ground", "", true)})).ok());
    ASSERT_TRUE(store->save(contentsWith({})).ok());
    const auto loaded = store->load();
    ASSERT_TRUE(loaded.ok()) << loaded.error().describe();
    EXPECT_TRUE(loaded->surfaces.empty());
}

TEST_F(SurfaceStorage, ASchemaTwelveProjectMigratesToThirteenWithNoSurfaces)
{
    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store->save(contentsWith({})).ok());
    }
    {
        // Schema 12 as the build before this one left it: no surfaces table.
        auto raw = SqliteDatabase::open(projectDir() / "project.db");
        ASSERT_TRUE(raw.ok());
        ASSERT_TRUE(raw->execute("DROP TABLE surfaces;").ok());
        ASSERT_TRUE(raw->setUserVersion(12).ok());
    }
    auto store = ProjectStore::open(projectDir());
    ASSERT_TRUE(store.ok()) << store.error().describe();
    EXPECT_EQ(*store->schemaVersion(), 13);
    // Migrated only with a way back, as every migration is.
    EXPECT_EQ(store->listBackups().size(), 1U);
    auto loaded = store->load();
    ASSERT_TRUE(loaded.ok()) << loaded.error().describe();
    EXPECT_TRUE(loaded->surfaces.empty());
    EXPECT_EQ(loaded->entities.size(), 1U) << "the drawing opens as it was";
    // And it keeps a surface like any project of this schema.
    loaded->surfaces.push_back(record("ground", "", true));
    ASSERT_TRUE(store->save(*loaded).ok());
    const auto again = store->load();
    ASSERT_TRUE(again.ok()) << again.error().describe();
    ASSERT_EQ(again->surfaces.size(), 1U);
    EXPECT_EQ(again->surfaces[0], record("ground", "", true));
}

TEST_F(SurfaceStorage, ASurfaceALoadCouldNotGiveBackIsRefusedAndNothingIsWritten)
{
    auto store = ProjectStore::create(projectDir(), {});
    ASSERT_TRUE(store.ok());
    const std::vector<SurfaceRecord> kept{record("ground", "", true)};
    ASSERT_TRUE(store->save(contentsWith(kept)).ok());

    SurfaceRecord unnamed = record("x", "", true);
    unnamed.name.clear();
    SurfaceRecord empty = record("empty", "", true);
    empty.data.clear();
    // The drawing's store compares names without case, so these two could
    // not both be loaded into it.
    const std::vector<std::vector<SurfaceRecord>> refused{
        {unnamed}, {empty}, {record("Ground", "", true), record("GROUND", "", true)}};
    for (const std::vector<SurfaceRecord>& surfaces : refused) {
        const auto status = store->save(contentsWith(surfaces));
        ASSERT_FALSE(status.ok());
        EXPECT_EQ(status.error().code, ErrorCode::InvalidArgument) << status.error().describe();
    }
    const auto loaded = store->load();
    ASSERT_TRUE(loaded.ok());
    EXPECT_EQ(loaded->surfaces, kept) << "the last good save stands";
}

TEST_F(SurfaceStorage, ASurfaceTooLargeForTheDatabaseIsRefusedSayingWhatToDo)
{
    const SurfaceRecord surface = record("ground", "Contours", true);
    // Its row: name 6 + source 20 ("point cloud site.laz") + style 8 + data
    // 15 + 6 + the 8 for the flag and the position = 63 bytes.
    EXPECT_EQ(ProjectStore::surfaceRowBytes(surface), 63U);
    EXPECT_TRUE(ProjectStore::checkSurfaceSize(surface, 63).ok());
    const auto over = ProjectStore::checkSurfaceSize(surface, 62);
    ASSERT_FALSE(over.ok());
    EXPECT_EQ(over.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(over.error().message.find("ground"), std::string::npos);
    EXPECT_NE(over.error().message.find("max="), std::string::npos) << "says what to do instead";
    // The limit is SQLite's default SQLITE_MAX_LENGTH, 1e9, less headroom.
    EXPECT_LT(ProjectStore::kMaxSurfaceBytes, 1'000'000'000U);
}
