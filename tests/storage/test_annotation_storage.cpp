// The annotation tables and kinds in a project file (schema 11,
// docs/annotation.md): saved, saved again (the DELETE list), loaded back
// exactly, and a project from before schema 11 opening with none.

#include <gtest/gtest.h>

#include <filesystem>
#include <string>
#include <system_error>

#include "katana/entity/model.hpp"
#include "katana/storage/project_store.hpp"
#include "katana/storage/sqlite_database.hpp"

namespace fs = std::filesystem;
using namespace katana::entity;
using katana::geometry::Point2;
using katana::storage::applyToModel;
using katana::storage::captureModel;
using katana::storage::ProjectStore;
using katana::storage::SqliteDatabase;

namespace {

class AnnotationStorage : public ::testing::Test {
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

Model annotatedModel()
{
    Model model;
    TextStyle road;
    road.name = "Road";
    road.fontFamily = "DejaVu Sans";
    road.paperHeight = 3.5;
    road.widthFactor = 0.85;
    road.oblique = 0.25;
    road.bold = true;
    road.color = Color{200, 10, 20, 255};
    road.mask = true;
    road.maskMargin = 0.75;
    road.readable = false;
    road.lineSpacing = 1.25;
    EXPECT_TRUE(model.textStyles.add(road).ok());
    // The built-in one, changed: stored, and read back over the seed.
    TextStyle standard = *model.textStyles.find(kDefaultTextStyleName);
    standard.paperHeight = 1.8;
    EXPECT_TRUE(model.textStyles.update(standard).ok());

    LabelStyle bearing;
    bearing.name = "Bearing Distance";
    bearing.kind = LabelKind::Segment;
    bearing.text = "{bearing:dms} {distance:.3f}";
    bearing.textStyle = "Road";
    bearing.placement = LabelPlacement::Below;
    bearing.offset = 0.8;
    bearing.priority = 3;
    bearing.color = Color{0, 128, 0, 255};
    bearing.minimumLength = 4.0;
    EXPECT_TRUE(model.labelStyles.add(bearing).ok());
    LabelStyle chainage;
    chainage.name = "Chainage";
    chainage.kind = LabelKind::Chainage;
    chainage.text = "{chainage:ch}";
    chainage.interval = 50.0;
    chainage.tickInterval = 0.0;
    EXPECT_TRUE(model.labelStyles.add(chainage).ok());

    LabelRule rule;
    rule.name = "boundaries";
    rule.labelStyle = "Bearing Distance";
    rule.layer = "cadastre*";
    rule.code = "BDY*";
    rule.entityType = "Polyline";
    rule.labelLayer = "labels/bearings";
    rule.enabled = false;
    EXPECT_TRUE(model.labelRules.add(rule).ok());

    DimensionStyle paper = *model.dimensionStyles.find(kDefaultDimensionStyleName);
    paper.paperSized = true;
    EXPECT_TRUE(model.dimensionStyles.update(paper).ok());

    const auto add = [&](Geometry geometry) {
        Entity entity;
        entity.geometry = std::move(geometry);
        EXPECT_TRUE(model.entities.add(std::move(entity)).ok());
    };
    add(katana::geometry::Polyline2{{Point2(0, 0), Point2(10, 0), Point2(10, 10)}, true});
    TextGeometry text{Point2(1, 1), "LOT 1\nDP 1234", 3.5, 0.0};
    text.style = "Road";
    text.paperHeight = 5.0;
    text.justify = TextJustify::MiddleCentre;
    add(text);
    add(LabelGeometry{.target = 1, .part = 1, .style = "Bearing Distance",
                      .anchor = Point2(10, 5), .rule = "boundaries"});
    add(LeaderGeometry{.vertices = {Point2(0, 0), Point2(5, 5)}, .text = "7",
                       .callout = CalloutShape::Circle,
                       .tipRef = AnchorRef{1, AnchorPoint::Vertex, 0}});
    DimensionGeometry dimension;
    dimension.kind = DimensionKind::Linear;
    dimension.start = Point2(0, 0);
    dimension.end = Point2(10, 10);
    dimension.offset = -3.0;
    dimension.startRef = AnchorRef{1, AnchorPoint::Vertex, 0};
    dimension.endRef = AnchorRef{1, AnchorPoint::Vertex, 2};
    add(dimension);
    return model;
}

} // namespace

TEST_F(AnnotationStorage, EveryTableAndKindSurvivesSavingTwiceAndReopening)
{
    const Model model = annotatedModel();
    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store->save(captureModel(model, {})).ok());
        // Twice: a table missing from save's DELETE list fails here, on a
        // primary-key conflict, and not on the first save.
        ASSERT_TRUE(store->save(captureModel(model, {})).ok());
    }
    auto store = ProjectStore::open(projectDir());
    ASSERT_TRUE(store.ok()) << store.error().describe();
    EXPECT_EQ(*store->schemaVersion(), 11);
    const auto contents = store->load();
    ASSERT_TRUE(contents.ok()) << contents.error().describe();
    Model loaded;
    ASSERT_TRUE(applyToModel(*contents, loaded).ok());

    EXPECT_EQ(loaded.textStyles.all(), model.textStyles.all());
    EXPECT_EQ(loaded.labelStyles.all(), model.labelStyles.all());
    EXPECT_EQ(loaded.labelRules.all(), model.labelRules.all());
    EXPECT_EQ(loaded.dimensionStyles.all(), model.dimensionStyles.all());
    std::vector<Entity> saved;
    model.entities.forEach([&](const Entity& e) { saved.push_back(e); });
    std::vector<Entity> reopened;
    loaded.entities.forEach([&](const Entity& e) { reopened.push_back(e); });
    EXPECT_EQ(reopened, saved);
}

TEST_F(AnnotationStorage, AProjectFromBeforeSchema11OpensWithNoneAndModelDimensions)
{
    Model model;
    Entity line;
    line.geometry = katana::geometry::Segment2{Point2(0, 0), Point2(1, 1)};
    ASSERT_TRUE(model.entities.add(line).ok());
    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store->save(captureModel(model, {})).ok());
    }
    {
        // Back to schema 10 as a build before this one left it.
        auto database = SqliteDatabase::open(projectDir() / "project.db");
        ASSERT_TRUE(database.ok());
        ASSERT_TRUE(database
                        ->execute("DROP TABLE label_rules; DROP TABLE label_styles;"
                                  "DROP TABLE text_styles;"
                                  "ALTER TABLE dimension_styles DROP COLUMN paper_sized;")
                        .ok());
        ASSERT_TRUE(database->setUserVersion(10).ok());
    }
    auto store = ProjectStore::open(projectDir());
    ASSERT_TRUE(store.ok()) << store.error().describe();
    EXPECT_EQ(*store->schemaVersion(), 11);
    const auto contents = store->load();
    ASSERT_TRUE(contents.ok()) << contents.error().describe();
    Model loaded;
    ASSERT_TRUE(applyToModel(*contents, loaded).ok());
    EXPECT_EQ(loaded.textStyles.size(), 1u) << "only the built-in Standard";
    EXPECT_EQ(loaded.labelStyles.size(), 0u);
    EXPECT_EQ(loaded.labelRules.size(), 0u);
    EXPECT_FALSE(loaded.dimensionStyles.find(kDefaultDimensionStyleName)->paperSized)
        << "the column's default is what every style was: model units";
}
