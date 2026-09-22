// Text placed as 12d draws it (PLAN.MD 20.2, slice 7): justification, the
// text's own colour and style, an offset in model units, and a raise that
// is a level and not a plan displacement.
#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <numbers>
#include <string>

#include "katana/archive12d/domain.hpp"
#include "katana/archive12d/reader.hpp"

namespace a12 = katana::archive12d;
using katana::entity::TextGeometry;
using katana::geometry::Point2;

namespace {

constexpr double kPi = std::numbers::pi;

a12::DomainImport import(const std::string& text)
{
    auto archive = a12::readArchive(text);
    EXPECT_TRUE(archive.ok()) << (archive.ok() ? "" : archive.error().describe());
    auto domain = a12::toDomain(archive.ok() ? *archive : a12::Archive{}, {});
    EXPECT_TRUE(domain.ok()) << (domain.ok() ? "" : domain.error().describe());
    return domain.ok() ? std::move(*domain) : a12::DomainImport{};
}

const std::string* meta(const katana::entity::Entity& entity, const std::string& key)
{
    const auto found = entity.metadata.find(key);
    return found == entity.metadata.end() ? nullptr : std::get_if<std::string>(&found->second);
}

// "ABCD" at 2 m: the width estimate is 4 characters x 2 m x 0.6 = 4.8 m.
constexpr double kWidth = 4.0 * 2.0 * 0.6;

std::string textAt(const char* justify)
{
    return std::string(R"(string text { name t x 100 y 200 z 5 text "ABCD"
  angle 0 worldsize 2 justify ")") + justify + R"(" })";
}

} // namespace

TEST(TextAnnotation, JustificationMovesTheAnchorToWhereTheBaselineStarts)
{
    // A Katana text's position IS the left end of its baseline, so 12d's
    // anchor has to be moved by the justification it was placed with.
    // Every annotation in the sample archives is bottom-left, which is
    // already that, and the other eight must not be silently treated as it.
    const auto left = import(textAt("bottom-left"));
    ASSERT_EQ(left.entities.size(), 1u);
    EXPECT_EQ(std::get<TextGeometry>(left.entities[0].geometry).position, Point2(100.0, 200.0))
        << "bottom-left is already what a Katana text position means";

    const auto centre = import(textAt("bottom-centre"));
    const auto& middle = std::get<TextGeometry>(centre.entities.at(0).geometry);
    EXPECT_NEAR(middle.position.x, 100.0 - kWidth / 2.0, 1e-12);
    EXPECT_NEAR(middle.position.y, 200.0, 1e-12);

    const auto right = import(textAt("bottom-right"));
    EXPECT_NEAR(std::get<TextGeometry>(right.entities.at(0).geometry).position.x, 100.0 - kWidth,
                1e-12);

    // Vertically: a top-anchored text hangs below its anchor by its height.
    const auto top = import(textAt("top-left"));
    EXPECT_NEAR(std::get<TextGeometry>(top.entities.at(0).geometry).position.y, 200.0 - 2.0, 1e-12);
    const auto mid = import(textAt("middle-left"));
    EXPECT_NEAR(std::get<TextGeometry>(mid.entities.at(0).geometry).position.y, 200.0 - 1.0, 1e-12);
}

TEST(TextAnnotation, JustificationIsMeasuredAlongTheTextNotAlongTheAxes)
{
    // Rotated a quarter turn, "along the text" is north and "across" is west.
    const auto domain = import(R"(string text { name t x 100 y 200 z 5 text "ABCD"
  angle 90 worldsize 2 justify "top-right" })");
    const auto& text = std::get<TextGeometry>(domain.entities.at(0).geometry);
    EXPECT_NEAR(text.rotation, kPi / 2.0, 1e-12);
    // along = -width (right), across = -height (top), turned 90 degrees:
    // x gets +height, y gets -width.
    EXPECT_NEAR(text.position.x, 100.0 + 2.0, 1e-12);
    EXPECT_NEAR(text.position.y, 200.0 - kWidth, 1e-12);
}

TEST(TextAnnotation, TheTextsOwnColourAndStyleAreItsOwnAndNotTheStrings)
{
    const auto domain = import(R"(string text { name t colour red x 0 y 0 z 0 text "RL"
  angle 0 worldsize 1 justify "bottom-left" text_colour cyan textstyle "Arial" })");
    ASSERT_EQ(domain.entities.size(), 1u);
    const auto& entity = domain.entities[0];
    EXPECT_EQ(entity.color, a12::standardColour("cyan")) << "the annotation's colour, not red";
    EXPECT_EQ(entity.style, "Arial");
    ASSERT_NE(meta(entity, "12d.colour"), nullptr);
    EXPECT_EQ(*meta(entity, "12d.colour"), "red") << "the string's own colour is still kept";
    const auto style = std::find_if(domain.stylesNeeded.begin(), domain.stylesNeeded.end(),
                                    [](const katana::entity::Style& s) { return s.name == "Arial"; });
    EXPECT_NE(style, domain.stylesNeeded.end()) << "a textstyle is a style like any other";
}

TEST(TextAnnotation, NoColourMeansTheStringsColourRatherThanNone)
{
    // 12d writes "no_colour" on the annotation of text that takes the
    // string's colour; treating it as a colour name would leave the text
    // ByLayer and lose the string's.
    const auto domain = import(R"(string text { name t colour green x 0 y 0 z 0 text "RL"
  angle 0 worldsize 1 justify "bottom-left" text_colour "no_colour" })");
    ASSERT_EQ(domain.entities.size(), 1u);
    EXPECT_EQ(domain.entities[0].color, a12::standardColour("green"));
    EXPECT_EQ(meta(domain.entities[0], "12d.text.colour"), nullptr);
}

TEST(TextAnnotation, AnOffsetInModelUnitsMovesTheTextAndOneInPaperMillimetresIsKept)
{
    // An offset is in the same units as the size. worldsize is model units,
    // so it can be applied; papersize is millimetres on a plot, which has no
    // model-unit meaning without a plot scale - the rule textHeight follows.
    const auto world = import(R"(string text { name t x 10 y 20 z 0 text "A"
  angle 0 worldsize 1 offset 3 justify "bottom-left" })");
    const auto& moved = std::get<TextGeometry>(world.entities.at(0).geometry);
    EXPECT_NEAR(moved.position.x, 10.0, 1e-12);
    EXPECT_NEAR(moved.position.y, 23.0, 1e-12) << "perpendicular to the text, to its left";
    EXPECT_EQ(*meta(world.entities.at(0), "12d.text.offset"), "3") << "and kept, to write back";

    const auto paper = import(R"(string text { name t x 10 y 20 z 0 text "A"
  angle 0 papersize 10 offset 12 justify "bottom-left" })");
    const auto& still = std::get<TextGeometry>(paper.entities.at(0).geometry);
    EXPECT_NEAR(still.position.x, 10.0, 1e-12);
    EXPECT_NEAR(still.position.y, 20.0, 1e-12) << "12 mm on paper is not 12 m on the ground";
    EXPECT_EQ(*meta(paper.entities.at(0), "12d.text.offset"), "12");
    EXPECT_EQ(*meta(paper.entities.at(0), "12d.text.papersize"), "10");
}

TEST(TextAnnotation, ARaiseIsALevelAndNotAPlanDisplacement)
{
    const auto domain = import(R"(string text { name t x 10 y 20 z 5 text "A"
  angle 0 worldsize 1 raise 2.5 justify "bottom-left" })");
    ASSERT_EQ(domain.entities.size(), 1u);
    const auto& text = std::get<TextGeometry>(domain.entities[0].geometry);
    EXPECT_EQ(text.position, Point2(10.0, 20.0)) << "a raise moves nothing in plan";
    EXPECT_DOUBLE_EQ(std::get<double>(domain.entities[0].properties.at("elevation")), 7.5);
    EXPECT_EQ(*meta(domain.entities[0], "12d.text.raise"), "2.5");
}

TEST(TextAnnotation, WhatKatanasTextHasNoFieldForIsKeptRatherThanDropped)
{
    const auto domain = import(R"(string text { name t x 0 y 0 z 0 text "A"
  angle 0 worldsize 1 slant 15 x_factor 0.8 justify "middle-centre" })");
    ASSERT_EQ(domain.entities.size(), 1u);
    const auto& entity = domain.entities[0];
    EXPECT_EQ(*meta(entity, "12d.text.slant"), "15");
    EXPECT_EQ(*meta(entity, "12d.text.x_factor"), "0.8");
    EXPECT_EQ(*meta(entity, "12d.text.justify"), "middle-centre")
        << "kept, because the position it produced cannot be turned back into it";
}

TEST(TextAnnotation, VertexAndSegmentTextAreAnnotatedTheSameWayAsAStandaloneText)
{
    // One annotation applies to every vertex, which is how 12d writes a
    // string whose vertices are all labelled the same way.
    const auto domain = import(R"(string super { name s data_3d { 0 0 0  10 0 0 }
  vertex_text_data { "A" "B" }
  vertex_annotate_value { angle 0 worldsize 2 offset 1 justify "bottom-left" colour cyan } })");
    // The string, then its two vertex texts.
    ASSERT_EQ(domain.entities.size(), 3u);
    const auto& first = std::get<TextGeometry>(domain.entities.at(1).geometry);
    EXPECT_NEAR(first.position.y, 1.0, 1e-12) << "offset applied to vertex text too";
    EXPECT_EQ(domain.entities.at(1).color, a12::standardColour("cyan"));
    EXPECT_EQ(domain.entities.at(2).color, a12::standardColour("cyan"));
}
