// Text in its style at a scale (cad/annotation/text_layout.hpp,
// docs/annotation.md "Text"): the paper-size arithmetic at several scales,
// justification with measured widths, the readable turn, masks, and a text
// written before styles laid out exactly as it always was drawn.

#include <gtest/gtest.h>

#include <cmath>

#include "katana/cad/annotation/text_layout.hpp"
#include "katana/entity/text_block.hpp"
#include "katana/math/numerics.hpp"

using namespace katana::cad::annotation;
using katana::entity::Model;
using katana::entity::TextGeometry;
using katana::entity::TextJustify;
using katana::entity::TextStyle;
using katana::geometry::Point2;
using katana::math::kPi;

namespace {

// A measure of exactly 0.5 per character, so widths are round numbers.
TextMeasure halfPerCharacter()
{
    return [](std::string_view line, const TextFace&) {
        return 0.5 * static_cast<double>(katana::entity::characterCount(line));
    };
}

Model modelWithRoadStyle()
{
    Model model;
    TextStyle road;
    road.name = "Road";
    road.paperHeight = 2.5;
    road.widthFactor = 0.8;
    road.mask = true;
    road.maskMargin = 1.0;
    EXPECT_TRUE(model.textStyles.add(road).ok());
    return model;
}

} // namespace

TEST(TextLayout, APaperHeightIsTheSameOnPaperAtEveryScale)
{
    const Model model = modelWithRoadStyle();
    TextGeometry text{Point2(0, 0), "A", 99.0, 0.0};
    text.style = "Road";
    // 2.5 mm at 1:200, 1:500, 1:2000 and 1:10000: 0.5, 1.25, 5 and 25 m,
    // which is 2.5 mm every time once the sheet divides by its scale.
    for (const double scale : {200.0, 500.0, 2000.0, 10000.0}) {
        const TextAppearance a = resolveTextStyle(model, text, scale);
        EXPECT_DOUBLE_EQ(a.height, 2.5 * scale / 1000.0) << "1:" << scale;
        EXPECT_DOUBLE_EQ(a.height / scale * 1000.0, 2.5) << "back on the sheet, 1:" << scale;
    }
    // The text's own paper height beats its style's.
    text.paperHeight = 5.0;
    EXPECT_DOUBLE_EQ(resolveTextStyle(model, text, 200.0).height, 1.0);
}

TEST(TextLayout, ATextFromBeforeStylesKeepsItsModelHeightAtEveryScale)
{
    const Model model;
    const TextGeometry text{Point2(3, 4), "OLD", 1.8, 0.4};
    EXPECT_FALSE(isPaperSized(model, text));
    for (const double scale : {200.0, 5000.0}) {
        const TextAppearance a = resolveTextStyle(model, text, scale);
        EXPECT_DOUBLE_EQ(a.height, 1.8);
        EXPECT_FALSE(a.readable) << "a text in no style is never turned";
        EXPECT_FALSE(a.mask);
    }
    // Laid out, it is one run at its position: exactly the old drawing.
    const Drawing d = layoutTextEntity(model, text, 500.0, halfPerCharacter());
    ASSERT_EQ(d.texts.size(), 1u);
    EXPECT_EQ(d.texts[0].origin, Point2(3, 4));
    EXPECT_DOUBLE_EQ(d.texts[0].height, 1.8);
    EXPECT_DOUBLE_EQ(d.texts[0].rotation, 0.4);
}

TEST(TextLayout, JustificationUsesTheMeasuredWidths)
{
    const Model model;
    TextAppearance a;
    a.height = 2.0;
    // "ABCD" is 4 x 0.5 x 2 = 4 wide; "AB" is 2. Centred at (10, 10):
    // the block is 4 wide, so the first line starts at 8 and the second,
    // centred in it, at 9. Two lines at height 2: the block is 2 + 10/3
    // tall, its middle at 10.
    const Drawing d =
        layoutText("ABCD\nAB", Point2(10, 10), 0.0, TextJustify::MiddleCentre, a, halfPerCharacter());
    ASSERT_EQ(d.texts.size(), 2u);
    const double block = 2.0 + katana::entity::kLinePitch * 2.0;
    EXPECT_DOUBLE_EQ(d.texts[0].origin.x, 8.0);
    EXPECT_DOUBLE_EQ(d.texts[1].origin.x, 9.0);
    EXPECT_NEAR(d.texts[0].origin.y, 10.0 + 0.5 * block - 2.0, 1e-12);
    EXPECT_NEAR(d.texts[1].origin.y, 10.0 - 0.5 * block, 1e-12);
    // Right-justified: every line ends at the point.
    const Drawing r =
        layoutText("ABCD\nAB", Point2(10, 10), 0.0, TextJustify::TopRight, a, halfPerCharacter());
    EXPECT_DOUBLE_EQ(r.texts[0].origin.x, 6.0);
    EXPECT_DOUBLE_EQ(r.texts[1].origin.x, 8.0);
    EXPECT_DOUBLE_EQ(r.texts[0].origin.y, 8.0) << "the top of the first line at the point";
}

TEST(TextLayout, AReadableTextIsTurnedAndCoversTheSamePlace)
{
    TextAppearance a;
    a.height = 1.0;
    a.readable = true;
    // Pointing west (180 degrees) reads upside down: turned to 0, and its
    // bottom-left anchor becomes top-right, so the block sits where it did.
    const Drawing turned =
        layoutText("AB", Point2(0, 0), kPi, TextJustify::BottomLeft, a, halfPerCharacter());
    ASSERT_EQ(turned.texts.size(), 1u);
    EXPECT_NEAR(katana::math::normalizeAngle(turned.texts[0].rotation), 0.0, 1e-12);
    a.readable = false;
    const Drawing asIs =
        layoutText("AB", Point2(0, 0), kPi, TextJustify::BottomLeft, a, halfPerCharacter());
    // Same block, drawn the other way up: compare the boxes' extents.
    Box2 one;
    Box2 two;
    for (const Point2& p : turned.textBoxes.front()) {
        one.expand(p);
    }
    for (const Point2& p : asIs.textBoxes.front()) {
        two.expand(p);
    }
    EXPECT_NEAR(one.min.x, two.min.x, 1e-9);
    EXPECT_NEAR(one.max.x, two.max.x, 1e-9);
    // The descender allowance is on the other side once turned, so the
    // heights may differ by it; the text body covers the same ground.
    EXPECT_NEAR(one.max.y - one.min.y, two.max.y - two.min.y, 1e-9);

    // Straight up (90 degrees) reads from the right of the sheet: left alone.
    EXPECT_FALSE(readableRotation(0.5 * kPi, TextJustify::BottomLeft).turned);
    EXPECT_TRUE(readableRotation(1.5 * kPi, TextJustify::BottomLeft).turned);
    EXPECT_EQ(readableRotation(kPi, TextJustify::MiddleLeft).justify, TextJustify::MiddleRight);
}

TEST(TextLayout, AMaskReachesPastTheTextByItsPaperMargin)
{
    const Model model = modelWithRoadStyle();
    TextGeometry text{Point2(0, 0), "AAAA", 1.0, 0.0};
    text.style = "Road";
    // At 1:1000 the text is 2.5 tall and 4 x 0.5 x 2.5 x 0.8 = 4 wide; the
    // mask is 1 mm = 1 unit beyond it all round.
    const Drawing d = layoutTextEntity(model, text, 1000.0, halfPerCharacter());
    ASSERT_EQ(d.masks.size(), 1u);
    Box2 mask;
    for (const Point2& p : d.masks.front()) {
        mask.expand(p);
    }
    EXPECT_DOUBLE_EQ(mask.min.x, -1.0);
    EXPECT_DOUBLE_EQ(mask.max.x, 5.0);
    EXPECT_DOUBLE_EQ(mask.max.y, 3.5);
    EXPECT_DOUBLE_EQ(d.texts[0].widthFactor, 0.8);
}
