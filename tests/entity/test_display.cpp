// Resolved appearance (PLAN.MD Phase 09).
//
// Before resolveDisplay there were three answers to "what colour is this
// entity": the 3D scene builder used the entity's own colour or a fixed
// default and ignored the layer entirely, the 2D viewport used the entity's or
// the layer's and ignored the style, and nothing whatsoever read Style::color.
// The tests here pin the one chain, including the two cases that were actually
// wrong on screen.

#include <gtest/gtest.h>

#include "katana/entity/display.hpp"

using katana::entity::Color;
using katana::entity::Entity;
using katana::entity::kContinuousLinetype;
using katana::entity::Layer;
using katana::entity::Model;
using katana::entity::resolveDisplay;
using katana::entity::Style;

namespace {

constexpr Color kRed{255, 0, 0, 255};
constexpr Color kGreen{0, 255, 0, 255};
constexpr Color kBlue{0, 0, 255, 255};

Model modelWithLayerAndStyle()
{
    Model model;

    Layer layer;
    layer.name = "kerb";
    layer.color = kRed;
    layer.lineWeight = 0.5;
    layer.linetype = "dashed";
    EXPECT_TRUE(model.layers.add(layer).ok());

    Style style;
    style.name = "heavy";
    style.color = kGreen;
    style.lineWeight = 1.2;
    style.linetype = "centre";
    EXPECT_TRUE(model.styles.add(style).ok());

    Style colourless;
    colourless.name = "thick-only";
    colourless.color = std::nullopt; // ByLayer
    colourless.lineWeight = 2.0;
    colourless.linetype = "phantom";
    EXPECT_TRUE(model.styles.add(colourless).ok());

    return model;
}

Entity onLayer(const char* layer)
{
    Entity entity;
    entity.layer = layer;
    entity.geometry = katana::geometry::Segment2{katana::geometry::Point2(0, 0),
                                                 katana::geometry::Point2(1, 1)};
    return entity;
}

} // namespace

TEST(ResolveDisplay, AnEntityWithNoOverridesTakesEverythingFromItsLayer)
{
    // THE case that was wrong in the 3D view: ByLayer is the default, so this
    // is the common path, not a corner one. It drew the fixed default colour.
    const Model model = modelWithLayerAndStyle();
    const auto display = resolveDisplay(model, onLayer("kerb"));

    EXPECT_EQ(display.color, kRed);
    EXPECT_DOUBLE_EQ(display.lineWeight, 0.5);
    EXPECT_EQ(display.linetype, "dashed");
}

TEST(ResolveDisplay, ANamedStyleOverridesTheLayer)
{
    // THE case that was wrong in both views: Style::color, lineWeight and
    // linetype were stored, validated and persisted, and read by nothing.
    const Model model = modelWithLayerAndStyle();
    Entity entity = onLayer("kerb");
    entity.style = "heavy";

    const auto display = resolveDisplay(model, entity);
    EXPECT_EQ(display.color, kGreen);
    EXPECT_DOUBLE_EQ(display.lineWeight, 1.2);
    EXPECT_EQ(display.linetype, "centre");
}

TEST(ResolveDisplay, AStyleWithoutAColourKeepsTheLayersColourButItsOwnWidth)
{
    // The asymmetry is in the Style type: `std::optional<Color> color` is
    // documented "empty: ByLayer", while lineWeight and linetype are plain
    // values that a named style always supplies.
    const Model model = modelWithLayerAndStyle();
    Entity entity = onLayer("kerb");
    entity.style = "thick-only";

    const auto display = resolveDisplay(model, entity);
    EXPECT_EQ(display.color, kRed) << "a colourless style must fall through to the layer";
    EXPECT_DOUBLE_EQ(display.lineWeight, 2.0);
    EXPECT_EQ(display.linetype, "phantom");
}

TEST(ResolveDisplay, TheEntitysOwnColourOverridesEverything)
{
    const Model model = modelWithLayerAndStyle();
    Entity entity = onLayer("kerb");
    entity.style = "heavy";
    entity.color = kBlue;

    const auto display = resolveDisplay(model, entity);
    EXPECT_EQ(display.color, kBlue);
    // ...but only the colour. There is deliberately no per-entity lineWeight or
    // linetype, so those still come from the style.
    EXPECT_DOUBLE_EQ(display.lineWeight, 1.2);
    EXPECT_EQ(display.linetype, "centre");
}

TEST(ResolveDisplay, AMissingLayerOrStyleFallsBackInsteadOfDereferencingNull)
{
    // An entity can reference a layer that has been removed; the model permits
    // it and a renderer must not crash on it.
    const Model model = modelWithLayerAndStyle();

    const auto orphan = resolveDisplay(model, onLayer("deleted"));
    EXPECT_EQ(orphan.color, Color{}) << "the built-in default, not a dereference";
    EXPECT_DOUBLE_EQ(orphan.lineWeight, 0.25);
    EXPECT_EQ(orphan.linetype, std::string(kContinuousLinetype));

    Entity withMissingStyle = onLayer("kerb");
    withMissingStyle.style = "no-such-style";
    const auto display = resolveDisplay(model, withMissingStyle);
    EXPECT_EQ(display.color, kRed) << "a missing style must not lose the layer's values";
    EXPECT_DOUBLE_EQ(display.lineWeight, 0.5);

    // An entity's own colour still wins even with nothing else resolvable.
    Entity orphanWithColour = onLayer("deleted");
    orphanWithColour.color = kBlue;
    EXPECT_EQ(resolveDisplay(model, orphanWithColour).color, kBlue);
}

TEST(ResolveDisplay, TheDefaultLayerResolvesWithoutAnythingBeingConfigured)
{
    // A fresh model with an entity on layer "0" is the very first thing the
    // application ever draws.
    const Model model;
    const auto display = resolveDisplay(model, onLayer("0"));
    EXPECT_EQ(display.color, Color{});
    EXPECT_DOUBLE_EQ(display.lineWeight, 0.25);
    EXPECT_EQ(display.linetype, std::string(kContinuousLinetype));
}

TEST(ResolveDisplay, ResolutionIsPureAndRepeatable)
{
    // Called once per entity per frame, so it must not depend on anything but
    // its arguments.
    const Model model = modelWithLayerAndStyle();
    Entity entity = onLayer("kerb");
    entity.style = "heavy";

    const auto first = resolveDisplay(model, entity);
    for (int i = 0; i < 5; ++i) {
        EXPECT_EQ(resolveDisplay(model, entity), first);
    }
}

TEST(ResolveDisplay, ThePointSymbolComesFromTheStyleAndNowhereElse)
{
    // PLAN.MD 20.2 slice 2. A layer has no symbol - a layer holds lines and
    // points alike - so a point on a layer with no style draws the plain mark,
    // and a point in a style with a symbol draws that at the style's size.
    Model model = modelWithLayerAndStyle();
    Style manhole;
    manhole.name = "manhole";
    manhole.symbol = "manhole";
    manhole.symbolSize = 1.2;
    ASSERT_TRUE(model.styles.add(manhole).ok());

    Entity plain = onLayer("kerb");
    EXPECT_EQ(resolveDisplay(model, plain).symbol, katana::entity::kNoSymbol);
    EXPECT_DOUBLE_EQ(resolveDisplay(model, plain).symbolSize, 0.0);

    Entity marked = onLayer("kerb");
    marked.style = "manhole";
    const auto display = resolveDisplay(model, marked);
    EXPECT_EQ(display.symbol, "manhole");
    EXPECT_DOUBLE_EQ(display.symbolSize, 1.2);
    EXPECT_EQ(display.color, kRed) << "a style that says nothing about colour leaves the layer's";

    marked.style = "heavy";
    EXPECT_EQ(resolveDisplay(model, marked).symbol, katana::entity::kNoSymbol)
        << "a line style draws a point with the plain mark";
}
