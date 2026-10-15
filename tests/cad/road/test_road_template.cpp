// Road templates as data (include/katana/cad/road/road_template.hpp): the
// standard ones, the checks, a kerb's shape worked by hand, and the JSON the
// project keeps them in.

#include <gtest/gtest.h>

#include <cmath>
#include <functional>
#include <string>

#include "katana/cad/corridor.hpp"
#include "katana/cad/road/road_template.hpp"

using namespace katana::cad::road;
using katana::core::ErrorCode;

namespace {

Component component(ComponentKind kind, Side side, std::string name)
{
    Component made = defaultComponent(kind, side);
    made.name = std::move(name);
    return made;
}

RoadTemplate twoLanes()
{
    RoadTemplate made;
    made.name = "two lanes";
    made.components = {component(ComponentKind::Lane, Side::Left, "lane"),
                       component(ComponentKind::Batter, Side::Left, "batter"),
                       component(ComponentKind::Lane, Side::Right, "lane"),
                       component(ComponentKind::Batter, Side::Right, "batter")};
    return made;
}

} // namespace

TEST(RoadTemplate, TheStandardTemplatesAreValidAndEachIsTheMirrorOfItself)
{
    const auto templates = standardTemplates();
    ASSERT_EQ(templates.size(), 3u);
    EXPECT_EQ(templates[0].name, "rural two-lane");
    EXPECT_EQ(templates[1].name, "urban street");
    EXPECT_EQ(templates[2].name, "divided road");
    for (const RoadTemplate& each : templates) {
        EXPECT_TRUE(checkTemplate(each).ok()) << checkTemplate(each).error().describe();
        const auto left = each.side(Side::Left);
        const auto right = each.side(Side::Right);
        ASSERT_EQ(left.size(), right.size()) << each.name;
        for (std::size_t i = 0; i < left.size(); ++i) {
            Component mirrored = *right[i];
            mirrored.side = Side::Left;
            EXPECT_EQ(*left[i], mirrored) << each.name << " component " << i;
        }
        // Every standard template ends each side at the ground.
        EXPECT_EQ(left.back()->kind, ComponentKind::Batter) << each.name;
    }
}

TEST(RoadTemplate, ACourseTakesItsColourFromItsMaterial)
{
    EXPECT_EQ(makeCourse("AC14", 0.05, "asphalt").colour, (katana::entity::Color{60, 60, 60, 255}));
    EXPECT_EQ(makeCourse("slab", 0.1, "Concrete").colour,
              (katana::entity::Color{200, 200, 190, 255}));
    EXPECT_EQ(makeCourse("base", 0.2, "crushed rock").colour,
              (katana::entity::Color{170, 145, 105, 255}));
    EXPECT_EQ(makeCourse("x", 0.2, "").colour, (katana::entity::Color{150, 150, 150, 255}));
    EXPECT_EQ(makeCourse("x", 0.2, "").ifcType, "PAVEMENT");
}

TEST(RoadTemplate, ABarrierKerbIsItsFaceBatteredOneInSixThenItsTop)
{
    // 150 mm face battered 1 in 6 is 25 mm out; a 150 mm top; its base 150
    // mm under the lip, level.
    Component kerb = defaultComponent(ComponentKind::Kerb, Side::Left);
    const auto shape = kerbShape(kerb);
    ASSERT_EQ(shape.size(), 3u);
    EXPECT_EQ(shape[0], (ShapeColumn{0.0, 0.0, -0.15}));
    EXPECT_NEAR(shape[1].out, 0.025, 1e-15);
    EXPECT_NEAR(shape[1].top, 0.15, 1e-15);
    EXPECT_NEAR(shape[2].out, 0.175, 1e-15);
    EXPECT_NEAR(shape[2].top, 0.15, 1e-15);
    for (const ShapeColumn& column : shape) {
        EXPECT_NEAR(column.bottom, -0.15, 1e-15);
    }
    EXPECT_NEAR(kerbWidth(kerb), 0.175, 1e-15);
}

TEST(RoadTemplate, AKerbAndGutterFallsToItsFaceAndKeepsItsDepthUnderTheChannel)
{
    // A 300 mm gutter falling 6 % reaches its invert 18 mm down; the 150 mm
    // face then rises from there, battered 25 mm, to a 150 mm top. The base
    // is 150 mm under the lip, and under the invert from the invert on.
    Component kerb = defaultComponent(ComponentKind::Kerb, Side::Left);
    kerb.shape = KerbShape::KerbAndGutter;
    const auto shape = kerbShape(kerb);
    ASSERT_EQ(shape.size(), 4u);
    EXPECT_NEAR(shape[1].out, 0.300, 1e-15);
    EXPECT_NEAR(shape[1].top, -0.018, 1e-15);
    EXPECT_NEAR(shape[1].bottom, -0.168, 1e-15);
    EXPECT_NEAR(shape[2].out, 0.325, 1e-15);
    EXPECT_NEAR(shape[2].top, 0.132, 1e-15);
    EXPECT_NEAR(shape[3].out, 0.475, 1e-15);
    EXPECT_NEAR(shape[3].top, 0.132, 1e-15);
    EXPECT_NEAR(shape[0].bottom, -0.15, 1e-15);
    EXPECT_NEAR(kerbWidth(kerb), 0.475, 1e-15);
}

TEST(RoadTemplate, AMountableKerbsFaceRunsOneAndAHalfTimesItsHeight)
{
    Component kerb = defaultComponent(ComponentKind::Kerb, Side::Left);
    kerb.shape = KerbShape::Mountable;
    kerb.height = 0.1;
    kerb.width = 0.1;
    const auto shape = kerbShape(kerb);
    ASSERT_EQ(shape.size(), 3u);
    EXPECT_NEAR(shape[1].out, 0.15, 1e-15);
    EXPECT_NEAR(shape[1].top, 0.1, 1e-15);
    EXPECT_NEAR(kerbWidth(kerb), 0.25, 1e-15);
}

TEST(RoadTemplate, AnEdgeStripIsFlushAtItsCrossfall)
{
    Component strip = defaultComponent(ComponentKind::Kerb, Side::Left);
    strip.shape = KerbShape::EdgeStrip;
    strip.width = 0.3;
    strip.crossfall = -2.0;
    const auto shape = kerbShape(strip);
    ASSERT_EQ(shape.size(), 2u);
    EXPECT_NEAR(shape[1].top, -0.006, 1e-15); // 300 mm at -2 %
    EXPECT_NEAR(shape[1].bottom, -0.156, 1e-15);
}

TEST(RoadTemplate, CheckRefusesWhatCannotBeBuiltNamingTheComponent)
{
    EXPECT_TRUE(checkTemplate(twoLanes()).ok());

    RoadTemplate twice = twoLanes();
    twice.components.insert(twice.components.begin(),
                            component(ComponentKind::Lane, Side::Left, "LANE"));
    const auto duplicate = checkTemplate(twice);
    ASSERT_FALSE(duplicate.ok());
    EXPECT_EQ(duplicate.error().code, ErrorCode::InvalidArgument);
    // The second of the two is the one refused, named as it is written.
    EXPECT_NE(duplicate.error().message.find("left:lane"), std::string::npos);

    RoadTemplate outside = twoLanes();
    outside.components.insert(outside.components.begin() + 2,
                              component(ComponentKind::Verge, Side::Left, "verge"));
    EXPECT_NE(checkTemplate(outside).error().message.find("outside the batter"), std::string::npos);

    RoadTemplate narrow = twoLanes();
    narrow.components[0].width = 0.0;
    EXPECT_EQ(checkTemplate(narrow).error().code, ErrorCode::InvalidArgument);

    RoadTemplate notANumber = twoLanes();
    notANumber.components[0].crossfall = std::nan("");
    EXPECT_NE(checkTemplate(notANumber).error().message.find("finite"), std::string::npos);

    RoadTemplate steep = twoLanes();
    steep.components[0].crossfall = 101.0;
    EXPECT_FALSE(checkTemplate(steep).ok());

    RoadTemplate coursedKerb = twoLanes();
    coursedKerb.components.insert(coursedKerb.components.begin() + 1,
                                  component(ComponentKind::Kerb, Side::Left, "kerb"));
    coursedKerb.components[1].courses = {makeCourse("x", 0.1, "concrete")};
    EXPECT_NE(checkTemplate(coursedKerb).error().message.find("no courses"), std::string::npos);

    RoadTemplate badWord = twoLanes();
    badWord.components[0].name = "lane one";
    EXPECT_FALSE(checkTemplate(badWord).ok());

    RoadTemplate halfBench = twoLanes();
    halfBench.components[1].benchHeight = 1.0;
    EXPECT_NE(checkTemplate(halfBench).error().message.find("bench"), std::string::npos);

    RoadTemplate thinCourse = twoLanes();
    thinCourse.components[0].courses = {makeCourse("AC", 0.0, "asphalt")};
    EXPECT_NE(checkTemplate(thinCourse).error().message.find("thickness"), std::string::npos);

    RoadTemplate slash = twoLanes();
    slash.name = "a/b";
    EXPECT_FALSE(checkTemplate(slash).ok());
}

TEST(RoadTemplate, ABenchTooSmallToBuildIsRefusedNamingTheBatterAndTheNumbers)
{
    const auto benched = [](double height, double width) {
        RoadTemplate made = twoLanes(); // batters at 2 and 4 run per rise, 50 m reach
        for (Component& each : made.components) {
            if (each.kind == ComponentKind::Batter) {
                each.benchHeight = height;
                each.benchWidth = width;
            }
        }
        return made;
    };
    // A bench every 0.125 m of rise at the flatter 1 in 2 is 0.25 m of
    // slope, and a 0.25 m flat: a bench every 0.5 m, 50 / 0.5 = 100 across
    // the 50 m - exactly the most. All binary fractions, so exact.
    EXPECT_TRUE(checkTemplate(benched(0.125, 0.25)).ok());
    // A hair less flat is 50 / 0.4999 = 100.02 benches: over.
    const auto over = checkTemplate(benched(0.125, 0.2499));
    ASSERT_FALSE(over.ok());
    EXPECT_EQ(over.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(over.error().message.find("left:batter"), std::string::npos) << over.error().message;
    EXPECT_NE(over.error().message.find("100 benches"), std::string::npos) << over.error().message;
    EXPECT_NE(over.error().message.find("0.2499"), std::string::npos) << over.error().message;
    // A real batter's bench - 5 m of rise, a 3 m flat - and none at all.
    EXPECT_TRUE(checkTemplate(benched(5.0, 3.0)).ok());
    EXPECT_TRUE(checkTemplate(benched(0.0, 0.0)).ok());
    // The millimetre typo of the report, and a value so small that adding it
    // to a running distance changed nothing and the bench loop never ended.
    EXPECT_FALSE(checkTemplate(benched(0.001, 0.001)).ok());
    EXPECT_FALSE(checkTemplate(benched(1e-300, 1e-300)).ok());
    EXPECT_FALSE(checkTemplate(benched(1e-320, 1e-320)).ok()); // a denormal: the period underflows
    // A short reach has few benches: a 0.05 m bench (period 0.05 x 2 + 0.05 =
    // 0.15 m) over a 50 m reach is 333, over a 5 m reach 33.
    EXPECT_FALSE(checkTemplate(benched(0.05, 0.05)).ok());
    RoadTemplate shortReach = benched(0.05, 0.05);
    for (Component& each : shortReach.components) {
        if (each.kind == ComponentKind::Batter) {
            each.maximumWidth = 5.0;
        }
    }
    EXPECT_TRUE(checkTemplate(shortReach).ok());
}

TEST(RoadTemplate, AnEnormousLengthIsRefusedAndTheLimitItselfIsAccepted)
{
    // 1000 m: the most; the half-metre sampling across a section is linear in
    // a width, and past 4.5e15 m it never moved.
    const auto with = [](const std::function<void(RoadTemplate&)>& change) {
        RoadTemplate made = twoLanes();
        change(made);
        return checkTemplate(made);
    };
    EXPECT_TRUE(with([](RoadTemplate& t) { t.components[0].width = 1000.0; }).ok());
    for (const double huge : {1000.001, 1e6, 1e16, 4.6e15}) {
        const auto lane = with([&](RoadTemplate& t) { t.components[0].width = huge; });
        ASSERT_FALSE(lane.ok()) << huge;
        EXPECT_EQ(lane.error().code, ErrorCode::InvalidArgument);
        EXPECT_NE(lane.error().message.find("at most 1000 m"), std::string::npos)
            << lane.error().message;
        EXPECT_NE(lane.error().message.find("left:lane"), std::string::npos);
    }
    EXPECT_FALSE(with([](RoadTemplate& t) { t.components[1].maximumWidth = 5000.0; }).ok());
    EXPECT_TRUE(with([](RoadTemplate& t) { t.components[1].maximumWidth = 1000.0; }).ok());
    EXPECT_FALSE(with([](RoadTemplate& t) {
                     t.components[0].courses = {makeCourse("slab", 2000.0, "concrete")};
                 }).ok());
    // A kerb's dimensions and a drain's.
    EXPECT_FALSE(with([](RoadTemplate& t) {
                     Component kerb = defaultComponent(ComponentKind::Kerb, Side::Left);
                     kerb.width = 1e9;
                     t.components.insert(t.components.begin() + 1, kerb);
                 }).ok());
    EXPECT_FALSE(with([](RoadTemplate& t) {
                     Component drain = defaultComponent(ComponentKind::Drain, Side::Left);
                     drain.depth = 1e9;
                     t.components.insert(t.components.begin() + 1, drain);
                 }).ok());
}

TEST(RoadTemplate, AnEmptyTemplateIsAcceptedToBeFilledIn)
{
    RoadTemplate empty;
    empty.name = "new";
    EXPECT_TRUE(checkTemplate(empty).ok());
}

TEST(RoadTemplate, TheAssemblyIsALaneAndABatterEachSideWithItsCrossfallMadeOutwardPercent)
{
    katana::cad::Assembly assembly;
    assembly.halfWidth = 4.0;
    assembly.crossfall = 0.025; // falls away: -2.5 % outward
    assembly.cutBatter = 1.5;
    assembly.fillBatter = 3.0;
    assembly.maximumBatterWidth = 30.0;
    const RoadTemplate made = templateFromAssembly(assembly);
    ASSERT_TRUE(checkTemplate(made).ok());
    for (const Side side : {Side::Left, Side::Right}) {
        const auto components = made.side(side);
        ASSERT_EQ(components.size(), 2u);
        EXPECT_EQ(components[0]->kind, ComponentKind::Lane);
        EXPECT_EQ(components[0]->width, 4.0);
        EXPECT_NEAR(components[0]->crossfall, -2.5, 1e-12);
        EXPECT_TRUE(components[0]->courses.empty());
        EXPECT_FALSE(components[0]->superelevated);
        EXPECT_EQ(components[1]->kind, ComponentKind::Batter);
        EXPECT_EQ(components[1]->cutSlope, 1.5);
        EXPECT_EQ(components[1]->fillSlope, 3.0);
        EXPECT_EQ(components[1]->maximumWidth, 30.0);
    }
}

TEST(RoadTemplate, TheJsonRoundTripsEveryValueExactly)
{
    RoadTemplateSet set;
    set.templates = standardTemplates();
    // Values with no short decimal form, to prove the doubles are exact.
    set.templates[0].components[0].width = 0.1 + 0.2;
    set.templates[0].components[0].courses[0].thickness = 1.0 / 3.0;
    set.templates[0].description = "line one, \"quoted\"";
    set.templates[1].components.back().benchHeight = 1.25;
    set.templates[1].components.back().benchWidth = 2.5;
    const auto json = roadTemplatesToJson(set);
    ASSERT_TRUE(json.ok()) << json.error().describe();
    const auto read = roadTemplatesFromJson(*json);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_EQ(*read, set);
    EXPECT_NE(json->find("\"format\":\"katana-road-templates\""), std::string::npos);
}

TEST(RoadTemplate, TheJsonOfANewerVersionIsRefusedAsUnsupported)
{
    const auto read =
        roadTemplatesFromJson(R"({"format":"katana-road-templates","version":99,"templates":[]})");
    ASSERT_FALSE(read.ok());
    EXPECT_EQ(read.error().code, ErrorCode::Unsupported);
}

TEST(RoadTemplate, TextThatIsNotATemplateSetIsAParseFailure)
{
    EXPECT_EQ(roadTemplatesFromJson("not json").error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(roadTemplatesFromJson(R"({"format":"katana-sheets","version":1})").error().code,
              ErrorCode::ParseFailure);
    EXPECT_EQ(
        roadTemplatesFromJson(
            R"({"format":"katana-road-templates","version":1,"templates":[{"name":"t","components":[{"name":"x","kind":"tunnel","side":"left"}]}]})")
            .error()
            .code,
        ErrorCode::ParseFailure);
    // Two templates of one name, ignoring case.
    EXPECT_EQ(
        roadTemplatesFromJson(
            R"({"format":"katana-road-templates","version":1,"templates":[{"name":"T","components":[]},{"name":"t","components":[]}]})")
            .error()
            .code,
        ErrorCode::ParseFailure);
}

TEST(RoadTemplate, AnEmptySetIsWrittenAndReadBack)
{
    const auto json = roadTemplatesToJson(RoadTemplateSet{});
    ASSERT_TRUE(json.ok());
    const auto read = roadTemplatesFromJson(*json);
    ASSERT_TRUE(read.ok());
    EXPECT_TRUE(read->templates.empty());
}
