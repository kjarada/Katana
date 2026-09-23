// entity::tableUsage: who uses every style, linetype name, symbol name, hatch
// pattern and layer, in one pass - the answer the delete guards, purge and
// the managers all read, so it has to follow resolveDisplay's chain exactly,
// ByLayer included.

#include <gtest/gtest.h>

#include "katana/entity/display.hpp"
#include "katana/entity/table_usage.hpp"

using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::HatchPattern;
using katana::entity::Layer;
using katana::entity::Linetype;
using katana::entity::Model;
using katana::entity::Style;
using katana::entity::TableUsage;
using katana::entity::tableUsage;
using katana::entity::UsageOptions;
using katana::entity::Users;

namespace {

void addLinetype(Model& model, const char* name)
{
    Linetype linetype;
    linetype.name = name;
    linetype.pattern = {{1.0}, {-0.5}};
    ASSERT_TRUE(model.linetypes.add(linetype).ok());
}

void addLayer(Model& model, const char* name, const char* linetype)
{
    Layer layer;
    layer.name = name;
    layer.linetype = linetype;
    ASSERT_TRUE(model.layers.add(layer).ok());
}

void addStyle(Model& model, const char* name, const char* linetype, const char* symbol = "",
              const char* hatch = "")
{
    Style style;
    style.name = name;
    style.linetype = linetype;
    style.symbol = symbol;
    style.hatchPattern = hatch;
    ASSERT_TRUE(model.styles.add(style).ok());
}

EntityId addEntity(Model& model, const char* layer, const char* style)
{
    Entity entity;
    entity.layer = layer;
    entity.style = style;
    entity.geometry = katana::entity::PointGeometry{katana::geometry::Point2(0.0, 0.0)};
    const auto id = model.entities.add(entity);
    EXPECT_TRUE(id.ok());
    return id.ok() ? *id : katana::entity::kInvalidEntityId;
}

// The hand-built drawing every test below reads. Entity ids are assigned
// 1, 2, 3 ... in the order added (EntityDatabase::add), so the comment on
// each line is its id.
//
//   linetypes  continuous (seeded), fence, dash, spare
//   hatches    none (seeded), grass
//   layers     0 (continuous), survey (fence), roads ("WATR Main", a
//              library name the model does not have), kerbs (dash)
//   styles     Kerb    linetype dash, hatch grass
//              Tree    linetype ByLayer, symbol tree
//              Bollard linetype and symbol "CULT Bollard"
//              Unused  linetype dash
Model drawing()
{
    Model model;
    addLinetype(model, "fence");
    addLinetype(model, "dash");
    addLinetype(model, "spare");
    HatchPattern grass;
    grass.name = "grass";
    grass.families = {{0.0, 1.0, 0.0}};
    EXPECT_TRUE(model.hatchPatterns.add(grass).ok());
    addLayer(model, "survey", "fence");
    addLayer(model, "roads", "WATR Main");
    addLayer(model, "kerbs", "dash");
    addStyle(model, "Kerb", "dash", "", "grass");
    addStyle(model, "Tree", "ByLayer", "tree");
    addStyle(model, "Bollard", "CULT Bollard", "CULT Bollard");
    addStyle(model, "Unused", "dash");

    addEntity(model, "survey", "");        // 1: no style        -> fence (its layer's)
    addEntity(model, "survey", "Tree");    // 2: ByLayer         -> fence
    addEntity(model, "roads", "Tree");     // 3: ByLayer         -> WATR Main
    addEntity(model, "survey", "Kerb");    // 4: its style's     -> dash, hatch grass
    addEntity(model, "0", "Bollard");      // 5: its style's     -> CULT Bollard
    addEntity(model, "gone", "");          // 6: missing layer   -> continuous
    addEntity(model, "survey", "Ghost");   // 7: missing style   -> fence (its layer's)
    return model;
}

} // namespace

TEST(TableUsage, EveryStyleIsCountedByTheEntitiesWearingItAndAnUnusedOneIsPresentWithNone)
{
    const Model model = drawing();
    const TableUsage usage = tableUsage(model);

    // Kerb: entity 4. Tree: 2 and 3. Bollard: 5. Unused: nobody.
    EXPECT_EQ(TableUsage::of(usage.styles, "Kerb").entities, 1u);
    EXPECT_EQ(TableUsage::of(usage.styles, "Kerb").firstEntity, 4u);
    EXPECT_EQ(TableUsage::of(usage.styles, "Tree").entities, 2u);
    EXPECT_EQ(TableUsage::of(usage.styles, "Tree").firstEntity, 2u);
    EXPECT_EQ(TableUsage::of(usage.styles, "Bollard").entities, 1u);
    ASSERT_TRUE(usage.styles.contains("Unused")) << "a defined style is listed even when unused";
    EXPECT_FALSE(TableUsage::of(usage.styles, "Unused").used());
    // Entity 7 wears a style the table does not have: listed under its name,
    // because a manager has to be able to show it.
    EXPECT_EQ(TableUsage::of(usage.styles, "Ghost").entities, 1u);
    EXPECT_FALSE(model.styles.contains("Ghost"));
}

TEST(TableUsage, AnEntityIsCountedAgainstTheLinetypeItIsDrawnWithByLayerIncluded)
{
    const Model model = drawing();
    const TableUsage usage = tableUsage(model);

    // fence: named by layer survey; reached by 1 (no style), 2 (Tree is
    // ByLayer, on survey) and 7 (a missing style falls back to the layer).
    const Users& fence = TableUsage::of(usage.linetypes, "fence");
    EXPECT_EQ(fence.layers, std::vector<std::string>{"survey"});
    EXPECT_TRUE(fence.styles.empty());
    EXPECT_EQ(fence.entities, 3u);
    EXPECT_EQ(fence.firstEntity, 1u);

    // WATR Main: not a model linetype - a library name - but named by layer
    // roads and reached by 3, which inherits it through ByLayer.
    const Users& watr = TableUsage::of(usage.linetypes, "WATR Main");
    EXPECT_EQ(watr.layers, std::vector<std::string>{"roads"});
    EXPECT_EQ(watr.entities, 1u);
    EXPECT_EQ(watr.firstEntity, 3u);

    // dash: named by layer kerbs and styles Kerb and Unused (ascending);
    // reached by 4 only (kerbs holds no entity, Unused is worn by none).
    const Users& dash = TableUsage::of(usage.linetypes, "dash");
    EXPECT_EQ(dash.layers, std::vector<std::string>{"kerbs"});
    EXPECT_EQ(dash.styles, (std::vector<std::string>{"Kerb", "Unused"}));
    EXPECT_EQ(dash.entities, 1u);

    // continuous: named by layer 0; reached by 6 alone - 5 is on layer 0 but
    // wears Bollard, whose own linetype wins.
    const Users& continuous = TableUsage::of(usage.linetypes, "continuous");
    EXPECT_EQ(continuous.layers, std::vector<std::string>{"0"});
    EXPECT_EQ(continuous.entities, 1u);
    EXPECT_EQ(continuous.firstEntity, 6u);

    EXPECT_FALSE(TableUsage::of(usage.linetypes, "spare").used());
    EXPECT_FALSE(usage.linetypes.contains("ByLayer"))
        << "ByLayer is not a linetype: its entities are counted under the layer's";

    // Every entity reaches exactly one linetype: 3 + 1 + 1 + 1 + 1 (CULT
    // Bollard, entity 5) = 7, the number of entities.
    std::size_t total = 0;
    for (const auto& [name, users] : usage.linetypes) {
        total += users.entities;
    }
    EXPECT_EQ(total, model.entities.size());
}

TEST(TableUsage, TheCountsAgreeWithResolveDisplayForEveryEntity)
{
    // The property the one-pass count exists to keep: an entity is counted
    // against the linetype and hatch resolveDisplay draws it with. Checked
    // entity by entity, with ids, against the resolver itself.
    const Model model = drawing();
    const TableUsage usage = tableUsage(model, UsageOptions{.entityIds = true});
    model.entities.forEach([&](const Entity& entity) {
        const auto display = katana::entity::resolveDisplay(model, entity);
        const auto& linetypeIds = TableUsage::of(usage.linetypes, display.linetype).entityIds;
        EXPECT_NE(std::ranges::find(linetypeIds, entity.id), linetypeIds.end())
            << "entity " << entity.id << " draws with " << display.linetype;
        const auto& hatchIds = TableUsage::of(usage.hatchPatterns, display.hatchPattern).entityIds;
        EXPECT_NE(std::ranges::find(hatchIds, entity.id), hatchIds.end())
            << "entity " << entity.id << " hatches with " << display.hatchPattern;
    });
}

TEST(TableUsage, SymbolsHatchesAndLayersAreCountedInTheSamePass)
{
    const Model model = drawing();
    const TableUsage usage = tableUsage(model);

    // tree: named by Tree, worn by 2 and 3.
    const Users& tree = TableUsage::of(usage.symbols, "tree");
    EXPECT_EQ(tree.styles, std::vector<std::string>{"Tree"});
    EXPECT_EQ(tree.entities, 2u);
    EXPECT_EQ(TableUsage::of(usage.symbols, "CULT Bollard").entities, 1u);
    EXPECT_FALSE(usage.symbols.contains("")) << "no symbol is not a symbol name";

    // grass: Kerb's, so entity 4. none: every layer's default, and the other
    // six entities.
    EXPECT_EQ(TableUsage::of(usage.hatchPatterns, "grass").styles,
              std::vector<std::string>{"Kerb"});
    EXPECT_EQ(TableUsage::of(usage.hatchPatterns, "grass").entities, 1u);
    const Users& none = TableUsage::of(usage.hatchPatterns, "none");
    EXPECT_EQ(none.layers, (std::vector<std::string>{"0", "kerbs", "roads", "survey"}));
    EXPECT_EQ(none.entities, 6u);

    // The layer manager's count: entities ON each layer, not its subtree.
    // survey holds 1, 2, 4 and 7; kerbs none; "gone" is not a layer but 6
    // is on it.
    EXPECT_EQ(TableUsage::of(usage.layers, "survey").entities, 4u);
    EXPECT_EQ(TableUsage::of(usage.layers, "roads").entities, 1u);
    EXPECT_EQ(TableUsage::of(usage.layers, "0").entities, 1u);
    ASSERT_TRUE(usage.layers.contains("kerbs"));
    EXPECT_EQ(TableUsage::of(usage.layers, "kerbs").entities, 0u);
    EXPECT_EQ(TableUsage::of(usage.layers, "gone").entities, 1u);
}

TEST(TableUsage, IdsAreCollectedOnlyWhenAskedForAndThenAscending)
{
    const Model model = drawing();
    EXPECT_TRUE(TableUsage::of(tableUsage(model).linetypes, "fence").entityIds.empty())
        << "a count is all a column needs";
    const TableUsage usage = tableUsage(model, UsageOptions{.entityIds = true});
    EXPECT_EQ(TableUsage::of(usage.linetypes, "fence").entityIds,
              (std::vector<EntityId>{1, 2, 7}));
    EXPECT_EQ(TableUsage::of(usage.styles, "Tree").entityIds, (std::vector<EntityId>{2, 3}));
}

TEST(TableUsage, ARefusalReadsAsACountAndTheFirstHolder)
{
    const Model model = drawing();
    const TableUsage usage = tableUsage(model);
    // A layer is named first because it is what a person fixes first.
    EXPECT_EQ(TableUsage::of(usage.linetypes, "fence").describe(),
              "used by 1 layer and 3 entities, e.g. layer=survey");
    EXPECT_EQ(TableUsage::of(usage.linetypes, "dash").describe(),
              "used by 1 layer, 2 styles and 1 entity, e.g. layer=kerbs");
    EXPECT_EQ(TableUsage::of(usage.styles, "Tree").describe(), "used by 2 entities, e.g. id=2");
    EXPECT_EQ(TableUsage::of(usage.styles, "Unused").describe(), "");
    EXPECT_EQ(TableUsage::of(usage.styles, "never heard of").describe(), "")
        << "an unknown name is used by nobody, not an error";
}
