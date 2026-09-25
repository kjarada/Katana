// The annotation verbs of the command line (annotation/annotation_verbs.cpp,
// docs/annotation.md "The command line"), driven as an agent drives them:
// key=value options, key=value replies, one undo step per edit.

#include <gtest/gtest.h>

#include <string>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"

using namespace katana::cad;
using namespace katana::entity;
using katana::geometry::Point2;

namespace {

struct Session {
    Document document;
    CommandInterpreter interpreter{document};

    std::string run(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << "\n  -> " << reply.error().describe();
        return reply.ok() ? *reply : std::string();
    }
    bool refused(const std::string& line) { return !interpreter.run(line).ok(); }
    [[nodiscard]] std::size_t steps() const { return document.history().undoCount(); }
};

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

} // namespace

TEST(AnnotationVerbs, AnnoscaleReadsSetsAndUndoes)
{
    Session s;
    EXPECT_EQ(s.run("ANNOSCALE"), "annoscale=1000");
    EXPECT_EQ(s.run("ANNOSCALE 1:500"), "annoscale=500");
    EXPECT_DOUBLE_EQ(s.document.annotationScale(), 500.0);
    EXPECT_EQ(s.steps(), 1u);
    EXPECT_EQ(s.run("ANNOSCALE 500"), "annoscale=500");
    EXPECT_EQ(s.steps(), 1u) << "the same scale again is no step";
    s.run("UNDO");
    EXPECT_DOUBLE_EQ(s.document.annotationScale(), 1000.0);
    EXPECT_TRUE(s.document.metadata().unknownKeys.empty()) << "the default is no key at all";
    EXPECT_TRUE(s.refused("ANNOSCALE -5"));
    EXPECT_TRUE(s.refused("ANNOSCALE 2:500"));
}

TEST(AnnotationVerbs, TextStylesAreMadeSetListedAndDeleted)
{
    Session s;
    const std::string made =
        s.run("TEXTSTYLE NEW Road font=\"DejaVu Sans\" paper=3.5 width=0.8 oblique=15 bold=on "
              "mask=on margin=0.75 colour=#FF0000");
    EXPECT_TRUE(contains(made, "name=Road")) << made;
    const TextStyle* road = s.document.model().textStyles.find("Road");
    ASSERT_NE(road, nullptr);
    EXPECT_EQ(road->fontFamily, "DejaVu Sans");
    EXPECT_DOUBLE_EQ(road->paperHeight, 3.5);
    EXPECT_NEAR(road->oblique, 15.0 * katana::math::kDegToRad, 1e-15);
    EXPECT_TRUE(road->bold);
    ASSERT_TRUE(road->color.has_value());
    s.run("TEXTSTYLE SET Road colour=bylayer readable=off");
    EXPECT_FALSE(s.document.model().textStyles.find("Road")->color.has_value());
    const std::string list = s.run("TEXTSTYLE LIST");
    EXPECT_TRUE(contains(list, "name=Road font=\"DejaVu Sans\" paper=3.5")) << list;
    EXPECT_TRUE(contains(list, "name=Standard"));
    EXPECT_TRUE(s.refused("TEXTSTYLE NEW Bad widht=2")) << "an unknown option is an error";
    EXPECT_TRUE(s.refused("TEXTSTYLE DELETE Standard"));
    s.run("TEXTSTYLE DELETE Road");
    EXPECT_EQ(s.document.model().textStyles.find("Road"), nullptr);
}

TEST(AnnotationVerbs, StyledTextIsPaperSizedAtTheAnnotationScale)
{
    Session s;
    s.run("ANNOSCALE 200");
    s.run("TEXTSTYLE NEW Notes paper=5");
    const std::string reply = s.run("MTEXT 10,20 \"LOT 7\\nDP 1234\" style=Notes justify=MC");
    EXPECT_TRUE(contains(reply, "created text id=")) << reply;
    const auto ids = s.document.lastCreatedEntities();
    ASSERT_EQ(ids.size(), 1u);
    const auto& text = std::get<TextGeometry>(s.document.model().entities.find(ids[0])->geometry);
    EXPECT_EQ(text.text, "LOT 7\nDP 1234");
    EXPECT_EQ(text.justify, TextJustify::MiddleCentre);
    EXPECT_DOUBLE_EQ(text.height, 1.0) << "5 mm at 1:200 is 1 m: its box at this scale";

    // The plain TEXT verb is unchanged: a model-unit text, no style.
    s.run("TEXT 0,0 2.5 \"plain\"");
    const auto& plain = std::get<TextGeometry>(
        s.document.model().entities.find(s.document.lastCreatedEntities().front())->geometry);
    EXPECT_TRUE(plain.style.empty());
    EXPECT_DOUBLE_EQ(plain.paperHeight, 0.0);

    // With an option it is styled; TEXTEDIT changes it as one step.
    s.run("TEXT 0,0 \"styled\" paper=2 rotation=90");
    const EntityId styled = s.document.lastCreatedEntities().front();
    const std::size_t before = s.steps();
    s.run("TEXTEDIT " + std::to_string(styled) + " justify=TR text=\"re worded\"");
    EXPECT_EQ(s.steps(), before + 1);
    const auto& edited = std::get<TextGeometry>(s.document.model().entities.find(styled)->geometry);
    EXPECT_EQ(edited.text, "re worded");
    EXPECT_EQ(edited.justify, TextJustify::TopRight);
    EXPECT_TRUE(s.refused("TEXT 0,0 \"x\" style=Nowhere"));
}

TEST(AnnotationVerbs, LabelStylesLabelsAndTheirLayout)
{
    Session s;
    const std::string defaults = s.run("LABELSTYLE DEFAULTS");
    EXPECT_TRUE(contains(defaults, "added=7")) << defaults;
    EXPECT_EQ(s.steps(), 1u) << "the whole standard set is one step";
    EXPECT_EQ(s.run("LABELSTYLE DEFAULTS"), "added=0");
    EXPECT_TRUE(contains(s.run("LABELSTYLE VALUES segment"), "bearing"));
    EXPECT_EQ(s.run("LABELSTYLE CHECK area \"{area:ha:.4f} ha\""), "valid=yes");
    EXPECT_TRUE(s.refused("LABELSTYLE CHECK area \"{bearing}\""));
    s.run("LABELSTYLE NEW Short kind=segment text=\"{distance:.2f}\" placement=below priority=4");
    EXPECT_TRUE(s.refused("LABELSTYLE SET Short kind=point")) << "the kind is fixed";

    s.run("PLINE 0,0 30,0 30,40 CLOSE");
    const EntityId lot = s.document.lastCreatedEntities().front();
    const std::string created =
        s.run("LABEL " + std::to_string(lot) + " style=\"Bearing Distance\"");
    EXPECT_TRUE(contains(created, "created labels=1")) << created;
    const std::string list = s.run("LABEL LIST");
    // (0,0)->(30,0) east 30; (30,0)->(30,40) north 40; back: 50 at 216 52 12.
    EXPECT_TRUE(contains(list, "90°00'00\\\" 30.000")) << list;
    EXPECT_TRUE(contains(list, "216°52'12\\\" 50.000")) << list;

    const std::string layout = s.run("LABEL LAYOUT scale=500");
    EXPECT_TRUE(contains(layout, "placed=3")) << layout;
    EXPECT_TRUE(contains(layout, "suppressed=0")) << layout;

    // The label follows its polyline: a vertex moves, the text changes.
    s.run("SELECT " + std::to_string(lot));
    s.run("MOVE 100,0");
    EXPECT_TRUE(contains(s.run("LABEL LIST"), "30.000"));
    EXPECT_TRUE(s.refused("LABEL " + std::to_string(lot) + " style=\"Spot Level\""))
        << "a point style cannot label a polyline";
}

TEST(AnnotationVerbs, ManyLabelsAreOneStep)
{
    Session s;
    s.run("LABELSTYLE DEFAULTS");
    s.run("POINT 0,0");
    const EntityId a = s.document.lastCreatedEntities().front();
    s.run("POINT 10,0");
    const EntityId b = s.document.lastCreatedEntities().front();
    s.run("SELECT " + std::to_string(a) + " " + std::to_string(b));
    s.run("PROP SET point P");
    const std::size_t before = s.steps();
    const std::string reply = s.run("LABEL SELECTION style=\"Point Number\"");
    EXPECT_TRUE(contains(reply, "labels=2")) << reply;
    EXPECT_EQ(s.steps(), before + 1);
    s.run("UNDO");
    EXPECT_EQ(s.run("LABEL LIST"), "");
}

TEST(AnnotationVerbs, AutoLabelRulesRunPreviewAndClear)
{
    Session s;
    s.run("LABELSTYLE DEFAULTS");
    s.run("LAYER NEW cadastre");
    s.run("LAYER SET cadastre");
    s.run("PLINE 0,0 20,0 20,20 0,20 CLOSE");
    s.run("PLINE 40,0 60,0 60,20 CLOSE");
    s.run("LAYER SET 0");
    s.run("LINE 0,50 50,50");
    s.run("AUTOLABEL RULE ADD lots style=\"Lot Area\" layer=cadastre");
    s.run("AUTOLABEL RULE ADD bearings style=\"Bearing Distance\" layer=cadastre "
          "labellayer=labels");
    EXPECT_TRUE(contains(s.run("AUTOLABEL RULE LIST"), "name=bearings style=\"Bearing Distance\""));
    EXPECT_TRUE(s.refused("AUTOLABEL RULE ADD bad style=Missing"));

    const std::string preview = s.run("AUTOLABEL PREVIEW");
    EXPECT_TRUE(contains(preview, "preview created=4")) << preview;
    EXPECT_EQ(s.run("LABEL LIST"), "") << "a preview changes nothing";

    const std::size_t before = s.steps();
    const std::string run = s.run("AUTOLABEL RUN");
    EXPECT_TRUE(contains(run, "autolabel created=4")) << run;
    EXPECT_EQ(s.steps(), before + 1);
    EXPECT_TRUE(contains(s.run("AUTOLABEL RUN"), "created=0 kept=4"));
    EXPECT_EQ(s.steps(), before + 1) << "a re-run with nothing to do is no step";
    EXPECT_TRUE(contains(s.run("AUTOLABEL CLEAR bearings"), "removed=2"));
    EXPECT_TRUE(contains(s.run("AUTOLABEL CLEAR"), "removed=2"));
}

TEST(AnnotationVerbs, DimensionsOfEveryKindFromTypedAndNamedPoints)
{
    Session s;
    s.run("LINE 0,0 10,0");
    const std::string line = std::to_string(s.document.lastCreatedEntities().front());
    const std::string linear = s.run("DIM LINEAR #" + line + ".start #" + line + ".end at=5,4");
    EXPECT_TRUE(contains(linear, "kind=linear measures=10")) << linear;
    EXPECT_TRUE(contains(linear, "associative=yes"));
    const std::string dim = linear.substr(linear.find("id=") + 3, linear.find(' ', linear.find("id=")) - linear.find("id=") - 3);

    const std::string baseline = s.run("DIM BASELINE " + dim + " 20,0 30,0");
    EXPECT_TRUE(contains(baseline, "created dimensions=2")) << baseline;
    const std::string chain = s.run("DIM CONTINUE " + dim + " 25,0");
    EXPECT_TRUE(contains(chain, "created dimensions=1")) << chain;

    s.run("LINE 0,0 0,10");
    const std::string other = std::to_string(s.document.lastCreatedEntities().front());
    const std::string angular = s.run("DIM ANGULAR " + line + " " + other + " at=3,3");
    EXPECT_TRUE(contains(angular, "kind=angular")) << angular;
    EXPECT_TRUE(contains(angular, "text=\"90°00'00\\\"\"")) << angular;

    s.run("CIRCLE 50,50 5");
    const std::string circle = std::to_string(s.document.lastCreatedEntities().front());
    EXPECT_TRUE(contains(s.run("DIM RADIUS " + circle), "text=R5.000"));
    EXPECT_TRUE(contains(s.run("DIM DIAMETER " + circle + " at=60,50"), "measures=10"));
    EXPECT_TRUE(contains(s.run("DIM ORDINATE 30,40 at=30,60 datum=10,10"), "kind=ordinate-x measures=20"));
    EXPECT_TRUE(contains(s.run("DIM ALIGNED 0,0 3,4 at=0,5"), "measures=5"));
    EXPECT_TRUE(contains(s.run("DIM HORIZONTAL 0,0 3,4 at=0,9"), "measures=3"));

    // The legacy form still makes an aligned dimension.
    s.run("DIM 0,0 6,8 2");
    const auto& legacy = std::get<DimensionGeometry>(
        s.document.model().entities.find(s.document.lastCreatedEntities().front())->geometry);
    EXPECT_EQ(legacy.kind, DimensionKind::Aligned);

    // The measured line moves; the linear dimension follows it.
    s.run("SELECT " + line);
    s.run("SCALE 0,0 2");
    const auto& followed =
        std::get<DimensionGeometry>(s.document.model().entities.find(std::stoull(dim))->geometry);
    EXPECT_DOUBLE_EQ(followed.measurement(), 20.0);
}

TEST(AnnotationVerbs, LeadersAndNumberedBalloons)
{
    Session s;
    s.run("POINT 5,5");
    const std::string pit = std::to_string(s.document.lastCreatedEntities().front());
    const std::string leader =
        s.run("LEADER #" + pit + " 15,15 25,15 text=\"PIT 12\\nIL 10.50\" arrow=dot callout=box");
    EXPECT_TRUE(contains(leader, "associative=yes")) << leader;
    const auto& made = std::get<LeaderGeometry>(
        s.document.model().entities.find(s.document.lastCreatedEntities().front())->geometry);
    EXPECT_EQ(made.vertices.size(), 3u);
    EXPECT_EQ(made.text, "PIT 12\nIL 10.50");
    EXPECT_EQ(made.arrow, ArrowHead::Dot);
    EXPECT_EQ(made.callout, CalloutShape::Box);

    EXPECT_TRUE(contains(s.run("BALLOON 0,0 5,5"), "text=1"));
    EXPECT_TRUE(contains(s.run("BALLOON 10,0 15,5"), "text=2"));
    EXPECT_TRUE(contains(s.run("BALLOON 20,0 25,5 n=7"), "text=7"));
    EXPECT_TRUE(contains(s.run("BALLOON 30,0 35,5"), "text=8"));
    EXPECT_TRUE(s.refused("LEADER 0,0"));
    EXPECT_TRUE(s.refused("LEADER 0,0 1,1 arrow=zigzag"));
}

TEST(AnnotationVerbs, HelpNamesEveryVerb)
{
    const std::string help = CommandInterpreter::helpText();
    for (const char* verb : {"ANNOSCALE", "TEXTSTYLE", "MTEXT", "TEXTEDIT", "LABELSTYLE", "LABEL ",
                             "AUTOLABEL", "DIM LINEAR", "DIM BASELINE", "LEADER", "BALLOON"}) {
        EXPECT_TRUE(contains(help, verb)) << verb;
    }
}

TEST(AnnotationVerbs, DimstylePaperSizesAStyle)
{
    Session s;
    s.run("DIMSTYLE SET Standard PAPER on");
    EXPECT_TRUE(s.document.model().dimensionStyles.find("Standard")->paperSized);
    EXPECT_TRUE(contains(s.run("DIMSTYLE LIST"), "paper-sized"));
}
