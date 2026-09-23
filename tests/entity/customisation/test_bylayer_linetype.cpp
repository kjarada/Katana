// A Style whose linetype is "ByLayer" inherits the layer's (the lead's D2).
//
// Before, a Style always took Style::linetype, so a symbol-only or
// colour-only style silently overrode its layer's linetype with
// "continuous". "ByLayer" is already a reserved Linetype NAME (the DXF
// keyword), so it can mean "inherit" without a storage change.

#include <gtest/gtest.h>

#include "katana/entity/display.hpp"

using katana::entity::Entity;
using katana::entity::Layer;
using katana::entity::Linetype;
using katana::entity::Model;
using katana::entity::resolveDisplay;
using katana::entity::Style;

namespace {

Model layerWithAFenceLinetype()
{
    Model model;
    Linetype fence;
    fence.name = "fence";
    fence.pattern = {{1.0}, {-0.5}};
    EXPECT_TRUE(model.linetypes.add(fence).ok());
    Layer layer;
    layer.name = "survey";
    layer.linetype = "fence";
    EXPECT_TRUE(model.layers.add(layer).ok());
    return model;
}

Entity pointOn(const char* layer, const char* style)
{
    Entity entity;
    entity.layer = layer;
    entity.style = style;
    entity.geometry = katana::entity::PointGeometry{katana::geometry::Point2(0.0, 0.0)};
    return entity;
}

} // namespace

TEST(ByLayerLinetype, AStyleWhoseLinetypeIsByLayerDrawsWithItsLayersLinetype)
{
    Model model = layerWithAFenceLinetype();
    Style tree;
    tree.name = "Tree";
    tree.linetype = "ByLayer";
    tree.symbol = "tree";
    tree.lineWeight = 0.7;
    ASSERT_TRUE(model.styles.add(tree).ok());

    const auto display = resolveDisplay(model, pointOn("survey", "Tree"));
    EXPECT_EQ(display.linetype, "fence") << "the layer's, not the word ByLayer";
    // Everything else the style says still holds.
    EXPECT_EQ(display.symbol, "tree");
    EXPECT_DOUBLE_EQ(display.lineWeight, 0.7);
}

TEST(ByLayerLinetype, TheSameStyleOnAnotherLayerFollowsThatLayer)
{
    Model model = layerWithAFenceLinetype();
    Layer other;
    other.name = "roads"; // continuous by default
    ASSERT_TRUE(model.layers.add(other).ok());
    Style tree;
    tree.name = "Tree";
    tree.linetype = "ByLayer";
    ASSERT_TRUE(model.styles.add(tree).ok());

    EXPECT_EQ(resolveDisplay(model, pointOn("survey", "Tree")).linetype, "fence");
    EXPECT_EQ(resolveDisplay(model, pointOn("roads", "Tree")).linetype, "continuous");
    // A missing layer resolves to the defaults, as it always has.
    EXPECT_EQ(resolveDisplay(model, pointOn("gone", "Tree")).linetype, "continuous");
}

TEST(ByLayerLinetype, AStyleNamingItsOwnLinetypeStillOverridesTheLayer)
{
    // The regression the change must not cause: a named linetype on a style
    // wins over the layer exactly as before.
    Model model = layerWithAFenceLinetype();
    Style kerb;
    kerb.name = "Kerb";
    kerb.linetype = "continuous";
    ASSERT_TRUE(model.styles.add(kerb).ok());
    EXPECT_EQ(resolveDisplay(model, pointOn("survey", "Kerb")).linetype, "continuous");
}

TEST(ByLayerLinetype, TheWordIsReadInAnyCaseBecauseNoLinetypeCanBeCalledThatInAnyCase)
{
    EXPECT_TRUE(katana::entity::isByLayer("ByLayer"));
    EXPECT_TRUE(katana::entity::isByLayer("BYLAYER"));
    EXPECT_TRUE(katana::entity::isByLayer("bylayer"));
    EXPECT_FALSE(katana::entity::isByLayer("ByLayerish"));
    EXPECT_FALSE(katana::entity::isByLayer(""));
    // The reservation it relies on.
    Linetype named;
    named.name = "BYLAYER";
    EXPECT_FALSE(katana::entity::validate(named).ok());
}

TEST(ByLayerLinetype, ValidateAcceptsAStyleThatSaysByLayer)
{
    Style style;
    style.name = "symbol only";
    style.linetype = "ByLayer";
    EXPECT_TRUE(katana::entity::validate(style).ok());
}

TEST(TableValidation, ALinetypeDescriptionThatIsNotUtf8IsRefused)
{
    // 0xE9 alone is Latin-1 "e acute", not UTF-8: what a CP1252 console
    // types. Accepted, it would make every later save fail far from here.
    Linetype linetype;
    linetype.name = "fence";
    linetype.description = "cl\xE9ture";
    const auto status = katana::entity::validate(linetype);
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, katana::core::ErrorCode::InvalidArgument);
    linetype.description = "cl\xC3\xA9ture"; // the same word in UTF-8
    EXPECT_TRUE(katana::entity::validate(linetype).ok());
}

TEST(TableValidation, AStyleWhoseLinetypeNameIsNotUtf8IsRefused)
{
    Style style;
    style.name = "s";
    style.linetype = "\xFF";
    EXPECT_FALSE(katana::entity::validate(style).ok());
}
