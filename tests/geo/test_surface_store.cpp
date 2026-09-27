// The named surfaces a session keeps (include/katana/terrain/surface_store.hpp).

#include <memory>

#include <gtest/gtest.h>

#include "katana/terrain/surface_store.hpp"

namespace {

using katana::terrain::NamedSurface;
using katana::terrain::SurfaceStore;
using katana::terrain::TinSurface;

std::shared_ptr<const TinSurface> triangle()
{
    auto surface = TinSurface::create({{0, 0, 10}, {10, 0, 11}, {0, 10, 12}}, {{0, 1, 2}});
    EXPECT_TRUE(surface.ok());
    return std::make_shared<const TinSurface>(std::move(surface).value());
}

TEST(SurfaceStore, ASurfaceIsFoundByItsNameInAnyCase)
{
    SurfaceStore store;
    ASSERT_TRUE(store.add({"Ground", triangle(), "drawing"}).ok());
    ASSERT_NE(store.find("ground"), nullptr);
    EXPECT_EQ(store.find("GROUND")->name, "Ground");
    EXPECT_EQ(store.find("GROUND")->source, "drawing");
    EXPECT_EQ(store.find("design"), nullptr);
}

TEST(SurfaceStore, ANameInUseIsRefused)
{
    SurfaceStore store;
    ASSERT_TRUE(store.add({"ground", triangle(), {}}).ok());
    const auto again = store.add({"GROUND", triangle(), {}});
    ASSERT_FALSE(again.ok());
    EXPECT_EQ(again.error().code, katana::core::ErrorCode::AlreadyExists);
    EXPECT_EQ(store.all().size(), 1u);
}

TEST(SurfaceStore, AnEmptyNameOrNoSurfaceIsRefused)
{
    SurfaceStore store;
    EXPECT_EQ(store.add({"", triangle(), {}}).error().code, katana::core::ErrorCode::InvalidArgument);
    EXPECT_EQ(store.add({"ground", nullptr, {}}).error().code,
              katana::core::ErrorCode::InvalidArgument);
    EXPECT_TRUE(store.empty());
}

TEST(SurfaceStore, EveryChangeMovesTheRevision)
{
    SurfaceStore store;
    const auto start = store.revision();
    ASSERT_TRUE(store.add({"a", triangle(), {}}).ok());
    const auto added = store.revision();
    EXPECT_GT(added, start);
    EXPECT_FALSE(store.remove("b"));
    EXPECT_EQ(store.revision(), added); // nothing removed, nothing changed
    EXPECT_TRUE(store.remove("A"));
    EXPECT_GT(store.revision(), added);
    const auto removed = store.revision();
    store.clear();
    EXPECT_GT(store.revision(), removed);
}

TEST(SurfaceStore, AUniqueNameCountsFromTwo)
{
    SurfaceStore store;
    EXPECT_EQ(store.uniqueName("ground"), "ground");
    ASSERT_TRUE(store.add({"ground", triangle(), {}}).ok());
    EXPECT_EQ(store.uniqueName("ground"), "ground (2)");
    ASSERT_TRUE(store.add({"ground (2)", triangle(), {}}).ok());
    EXPECT_EQ(store.uniqueName("Ground"), "Ground (3)");
    EXPECT_EQ(store.uniqueName(""), "surface");
}

} // namespace
