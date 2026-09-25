// The scope and filter words every verb on drawing data takes
// (scope_verbs.hpp, docs/cad.md): what the one parser reads, what it
// refuses, how the words are written back, and what they take from a
// drawing - through matchEntities, as Global Modify takes it. Every
// expectation is worked out by hand from the drawing the fixture lays out.

#include <gtest/gtest.h>

#include <optional>
#include <string>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/cad/view_set.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"

using namespace katana::cad;
using katana::core::ErrorCode;
using katana::entity::Color;
using katana::entity::EntityId;
using katana::entity::EntityType;
using katana::entity::Layer;
using katana::entity::TextGeometry;
using katana::geometry::Box2;
using katana::geometry::Point2;
namespace cmd = katana::commands;

namespace {

// Splits a line as the interpreter does, so a test types what a person types.
std::vector<std::string> words(const std::string& line)
{
    auto tokens = CommandInterpreter::tokenize(line);
    EXPECT_TRUE(tokens.ok()) << line;
    return tokens.ok() ? *tokens : std::vector<std::string>{};
}

// The words of `line` read from its first; `rest` is what is left for the verb.
ScopeWords parsed(const std::string& line, std::string* rest = nullptr)
{
    const std::vector<std::string> all = words(line);
    std::size_t at = 0;
    auto scope = parseScopeWords(all, at);
    EXPECT_TRUE(scope.ok()) << line << ": " << (scope.ok() ? "" : scope.error().describe());
    if (rest != nullptr) {
        rest->clear();
        for (std::size_t i = at; i < all.size(); ++i) {
            *rest += (rest->empty() ? "" : " ") + all[i];
        }
    }
    return scope.ok() ? *scope : ScopeWords{};
}

katana::core::Error refusal(const std::string& line)
{
    const std::vector<std::string> all = words(line);
    std::size_t at = 0;
    auto scope = parseScopeWords(all, at);
    EXPECT_FALSE(scope.ok()) << line << " was read";
    return scope.ok() ? katana::core::Error{} : scope.error();
}

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

// The drawing:
//
//   layer          entity    where           property
//   survey         pA point  (0,0)           code=TREE1
//   survey         pB point  (10,0)          code=POLE
//   survey/trees   pC point  (20,0)          code=TREE2
//   roads          ln line   (0,5)-(30,5)    -
//   roads          tx text   at (0,10)       -           "CH 100"
struct ScopeVerbsTest : ::testing::Test {
    Document document;
    EntityId pA = 0, pB = 0, pC = 0, ln = 0, tx = 0;

    void SetUp() override
    {
        for (const char* name : {"survey", "survey/trees", "roads"}) {
            Layer layer;
            layer.name = name;
            ASSERT_TRUE(document.execute(cmd::createLayer(layer)).ok());
        }
        pA = create(cmd::createPoint(Point2(0, 0), {"survey", "", {}}));
        pB = create(cmd::createPoint(Point2(10, 0), {"survey", "", {}}));
        pC = create(cmd::createPoint(Point2(20, 0), {"survey/trees", "", {}}));
        ln = create(cmd::createLine(Point2(0, 5), Point2(30, 5), {"roads", "", {}}));
        TextGeometry text;
        text.position = Point2(0, 10);
        text.text = "CH 100";
        tx = create(cmd::createText(text, {"roads", "", {}}));
        for (const auto& [id, code] :
             {std::pair{pA, "TREE1"}, std::pair{pB, "POLE"}, std::pair{pC, "TREE2"}}) {
            ASSERT_TRUE(
                document.execute(cmd::setEntityProperty({id}, "code", std::string(code))).ok());
        }
    }

    EntityId create(cmd::CommandPtr command)
    {
        EXPECT_TRUE(document.execute(std::move(command)).ok());
        const auto created = document.lastCreatedEntities();
        return created.empty() ? 0 : created.front();
    }

    std::vector<EntityId> match(const std::string& line, const ScopeViewProvider& views = {})
    {
        auto matched = matchScope(document, parsed(line), views);
        EXPECT_TRUE(matched.ok()) << line << ": "
                                  << (matched.ok() ? "" : matched.error().describe());
        return matched.ok() ? matched->matched : std::vector<EntityId>{};
    }
};

} // namespace

// ---- reading the words -----------------------------------------------------------------------

TEST(ScopeWordsParse, EachScopeWordIsReadAndTheVerbsOwnWordsAreLeftForIt)
{
    std::string rest;
    EXPECT_EQ(parsed("SELECTION MINCOVER 0.6", &rest).source, ScopeSource::Selection);
    EXPECT_EQ(rest, "MINCOVER 0.6");
    EXPECT_EQ(parsed("sel").source, ScopeSource::Selection);
    EXPECT_EQ(parsed("DRAWING SET COLOUR=#FF0000", &rest).source, ScopeSource::Drawing);
    EXPECT_EQ(rest, "SET COLOUR=#FF0000");
    EXPECT_EQ(parsed("all").source, ScopeSource::Drawing);

    const ScopeWords active = parsed("VIEW SPACING 20", &rest);
    EXPECT_EQ(active.source, ScopeSource::View);
    EXPECT_FALSE(active.view.has_value());
    EXPECT_EQ(rest, "SPACING 20");
    const ScopeWords third = parsed("view 3 SPACING 20", &rest);
    EXPECT_EQ(third.view, std::optional<std::uint32_t>(3));
    EXPECT_EQ(rest, "SPACING 20");

    // Corners in either order are one window, min first.
    const ScopeWords area = parsed("AREA 10,20,0,5 SCHEMA s.csv", &rest);
    EXPECT_EQ(area.source, ScopeSource::Area);
    EXPECT_EQ(area.area.min, Point2(0, 5));
    EXPECT_EQ(area.area.max, Point2(10, 20));
    EXPECT_EQ(rest, "SCHEMA s.csv");

    const ScopeWords layers = parsed("LAYERS survey,roads ONLY DESIGN d.csv", &rest);
    EXPECT_EQ(layers.source, ScopeSource::Layers);
    EXPECT_EQ(layers.layers, (std::vector<std::string>{"survey", "roads"}));
    EXPECT_FALSE(layers.sublayers);
    EXPECT_EQ(rest, "DESIGN d.csv");
    const ScopeWords one = parsed("layer \"Site Services\"");
    EXPECT_EQ(one.layers, (std::vector<std::string>{"Site Services"}));
    EXPECT_TRUE(one.sublayers);

    // No scope word is the selection, as MODIFY always read it.
    EXPECT_EQ(parsed("WHERE DRAWN SET COLOUR=#FF0000", &rest).source, ScopeSource::Selection);
    EXPECT_EQ(rest, "SET COLOUR=#FF0000");
    EXPECT_EQ(parsed("", &rest).source, ScopeSource::Selection);
    EXPECT_EQ(rest, "");
}

TEST(ScopeWordsParse, EveryWhereKeyIsReadAndTheFilterEndsAtTheFirstWordThatIsNoCondition)
{
    std::string rest;
    const ScopeWords scope =
        parsed("DRAWING WHERE TYPE=point,Line LAYER=survey/*,roads STYLE=Kerb COLOUR=#FF0000 "
               "PROP=code:TREE* \"TEXT=CH 1*\" DRAWN MINCOVER 0.6",
               &rest);
    const ModifyFilter& filter = scope.filter;
    EXPECT_EQ(filter.types, (std::set<EntityType>{EntityType::Point, EntityType::Line}));
    EXPECT_EQ(filter.layers, (std::vector<std::string>{"survey/*", "roads"}));
    EXPECT_EQ(filter.style, std::optional<std::string>("Kerb"));
    ASSERT_TRUE(filter.colour.has_value());
    EXPECT_EQ(*filter.colour, std::optional<Color>(Color{255, 0, 0, 255}));
    EXPECT_EQ(filter.property, std::optional<std::string>("code"));
    EXPECT_EQ(filter.propertyValue, std::optional<std::string>("TREE*"));
    EXPECT_EQ(filter.text, std::optional<std::string>("CH 1*"));
    EXPECT_TRUE(filter.drawnOnly);
    EXPECT_EQ(rest, "MINCOVER 0.6");

    // ByLayer is the inner nullopt; COLOR is COLOUR; a property with no value
    // is "carries it"; a second WHERE reads on into the same filter.
    const ScopeWords byLayer =
        parsed("WHERE color=bylayer PROP=code where TYPE=text", &rest);
    ASSERT_TRUE(byLayer.filter.colour.has_value());
    EXPECT_FALSE(byLayer.filter.colour->has_value());
    EXPECT_EQ(byLayer.filter.property, std::optional<std::string>("code"));
    EXPECT_FALSE(byLayer.filter.propertyValue.has_value());
    EXPECT_EQ(byLayer.filter.types, (std::set<EntityType>{EntityType::Text}));
    EXPECT_EQ(rest, "");
}

TEST(ScopeWordsParse, WhatCannotBeReadIsRefusedNamingTheWord)
{
    EXPECT_TRUE(contains(refusal("SELECTION DRAWING").describe(), "give one scope"));
    EXPECT_TRUE(contains(refusal("LAYERS").describe(), "LAYERS needs the layers"));
    EXPECT_TRUE(contains(refusal("LAYERS ,").describe(), "LAYERS needs the layers"));
    EXPECT_TRUE(contains(refusal("DRAWING ONLY").describe(), "ONLY follows a LAYERS list"));
    EXPECT_TRUE(contains(refusal("AREA").describe(), "four numbers"));
    EXPECT_TRUE(contains(refusal("AREA 1,2,3").describe(), "1,2,3"));
    EXPECT_TRUE(contains(refusal("AREA 1,2,3,4,5").describe(), "four numbers"));
    EXPECT_TRUE(contains(refusal("AREA a,b,c,d").describe(), "a,b,c,d"));
    EXPECT_TRUE(contains(refusal("AREA 0,0,inf,1").describe(), "four numbers"));
    EXPECT_TRUE(contains(refusal("VIEW 0").describe(), "a view id is a whole number from 1"));
    EXPECT_TRUE(contains(refusal("VIEW 99999999999").describe(), "99999999999"));
    EXPECT_TRUE(contains(refusal("WHERE SHADE=blue").describe(), "not a WHERE key"));
    // A type that is no entity type is entityTypeFromString's refusal: a
    // word that does not read, like the colour below.
    const katana::core::Error blob = refusal("WHERE TYPE=blob");
    EXPECT_EQ(blob.code, ErrorCode::ParseFailure);
    EXPECT_TRUE(contains(blob.describe(), "Blob")) << blob.describe();
    EXPECT_TRUE(contains(refusal("WHERE COLOUR=red").describe(), "red"));
}

TEST(ScopeWordsParse, OnlyTheScopeWordsAndWhereBeginAScope)
{
    for (const char* word : {"SELECTION", "sel", "Drawing", "ALL", "view", "AREA", "LAYERS",
                             "layer", "where"}) {
        EXPECT_TRUE(isScopeWord(word)) << word;
    }
    for (const char* word : {"schedule.csv", "./drawing", "MINCOVER", "SET", "ONLY", "DRAWN",
                             "TYPE=point", ""}) {
        EXPECT_FALSE(isScopeWord(word)) << word;
    }
}

TEST(ScopeWordsParse, TheWordsWrittenBackReadAsTheSameScope)
{
    ScopeWords layers;
    layers.source = ScopeSource::Layers;
    layers.layers = {"Site Services/Located", "roads"};
    layers.sublayers = false;
    layers.filter.types = {EntityType::Point, EntityType::Polyline};
    layers.filter.layers = {"survey/*"};
    layers.filter.style = "";
    layers.filter.colour = std::optional<Color>{};
    layers.filter.property = "utility.line";
    layers.filter.propertyValue = "W 1";
    layers.filter.text = "CH 100";
    layers.filter.drawnOnly = true;
    auto line = formatScopeWords(layers);
    ASSERT_TRUE(line.ok()) << line.error().describe();
    EXPECT_EQ(*line, "LAYERS \"Site Services/Located,roads\" ONLY WHERE TYPE=point,polyline "
                     "LAYER=survey/* STYLE= COLOUR=ByLayer \"PROP=utility.line:W 1\" "
                     "\"TEXT=CH 100\" DRAWN");
    const ScopeWords back = parsed(*line);
    EXPECT_EQ(back.source, layers.source);
    EXPECT_EQ(back.layers, layers.layers);
    EXPECT_EQ(back.sublayers, layers.sublayers);
    EXPECT_EQ(back.filter.types, layers.filter.types);
    EXPECT_EQ(back.filter.layers, layers.filter.layers);
    EXPECT_EQ(back.filter.style, layers.filter.style);
    EXPECT_EQ(back.filter.colour, layers.filter.colour);
    EXPECT_EQ(back.filter.property, layers.filter.property);
    EXPECT_EQ(back.filter.propertyValue, layers.filter.propertyValue);
    EXPECT_EQ(back.filter.text, layers.filter.text);
    EXPECT_EQ(back.filter.drawnOnly, layers.filter.drawnOnly);

    ScopeWords area;
    area.source = ScopeSource::Area;
    area.area = Box2(Point2(334000.125, 6250000), Point2(334040, 6250007.2));
    area.filter.colour = std::optional<Color>(Color{0x2F, 0x80, 0xED, 255});
    line = formatScopeWords(area);
    ASSERT_TRUE(line.ok());
    EXPECT_EQ(*line, "AREA 334000.125,6250000,334040,6250007.2 WHERE COLOUR=#2F80ED");
    const ScopeWords areaBack = parsed(*line);
    EXPECT_EQ(areaBack.area.min, area.area.min);
    EXPECT_EQ(areaBack.area.max, area.area.max);
    EXPECT_EQ(areaBack.filter.colour, area.filter.colour);

    ScopeWords view;
    view.source = ScopeSource::View;
    view.view = 4;
    EXPECT_EQ(*formatScopeWords(view), "VIEW 4");
    view.view.reset();
    EXPECT_EQ(*formatScopeWords(view), "VIEW");
    EXPECT_EQ(*formatScopeWords(ScopeWords{}), "SELECTION");

    // What a line cannot say is refused, not mangled.
    ScopeWords quoted;
    quoted.source = ScopeSource::Drawing;
    quoted.filter.text = "say \"hello\"";
    EXPECT_EQ(formatScopeWords(quoted).error().code, ErrorCode::InvalidArgument);
    ScopeWords comma;
    comma.source = ScopeSource::Layers;
    comma.layers = {"a,b"};
    EXPECT_EQ(formatScopeWords(comma).error().code, ErrorCode::InvalidArgument);
    comma.layers = {};
    EXPECT_EQ(formatScopeWords(comma).error().code, ErrorCode::InvalidArgument);
    ScopeWords valueAlone;
    valueAlone.filter.propertyValue = "x";
    EXPECT_EQ(formatScopeWords(valueAlone).error().code, ErrorCode::InvalidArgument);
}

// ---- what the words take ---------------------------------------------------------------------

TEST_F(ScopeVerbsTest, TheScopesTakeWhatGlobalModifysTakes)
{
    document.selection().set({pB, ln});
    EXPECT_EQ(match("SELECTION"), (std::vector<EntityId>{pB, ln}));
    EXPECT_EQ(match("DRAWING"), (std::vector<EntityId>{pA, pB, pC, ln, tx}));
    EXPECT_EQ(match("LAYERS survey"), (std::vector<EntityId>{pA, pB, pC}));
    EXPECT_EQ(match("LAYERS survey ONLY"), (std::vector<EntityId>{pA, pB}));
    EXPECT_EQ(match("LAYERS survey/trees,roads"), (std::vector<EntityId>{pC, ln, tx}));
    // The box x 0..8, y 4..6 holds part of the road line and nothing else.
    EXPECT_EQ(match("AREA 8,6,0,4"), (std::vector<EntityId>{ln}));
    // x -1..12, y -1..1: the two survey points.
    EXPECT_EQ(match("AREA -1,-1,12,1"), (std::vector<EntityId>{pA, pB}));
    EXPECT_EQ(match("DRAWING WHERE PROP=code:TREE*"), (std::vector<EntityId>{pA, pC}));
    EXPECT_EQ(match("LAYERS survey WHERE TYPE=point PROP=code:P*"), (std::vector<EntityId>{pB}));
    EXPECT_EQ(match("DRAWING WHERE \"TEXT=CH *\""), (std::vector<EntityId>{tx}));

    // AREA is what a view hiding nothing of its own shows: an entity switched
    // off is in no view's window.
    ASSERT_TRUE(document.execute(cmd::setEntityVisible({pB}, false)).ok());
    EXPECT_EQ(match("AREA -1,-1,12,1"), (std::vector<EntityId>{pA}));
    // Taking nothing is an answer, not a failure.
    EXPECT_EQ(match("AREA 100,100,200,200"), (std::vector<EntityId>{}));
    // A layer the drawing lacks is matchEntities' refusal, by name.
    auto missing = matchScope(document, parsed("LAYERS nowhere"), {});
    ASSERT_FALSE(missing.ok());
    EXPECT_EQ(missing.error().code, ErrorCode::NotFound);
    EXPECT_EQ(missing.error().context, "nowhere");
}

TEST_F(ScopeVerbsTest, ViewAsksTheWindowForItsLayersAndItsAreaAndHeadlessIsRefusedNamingArea)
{
    // A window of two plan views: 1 hides "survey", 2 hides nothing and
    // shows x 5..25, y -1..1. The active one is 2.
    std::vector<std::optional<std::uint32_t>> asked;
    const ScopeViewProvider views = [&asked](std::optional<std::uint32_t> id)
        -> katana::core::Result<ScopeView> {
        asked.push_back(id);
        ScopeView view;
        if (id == std::optional<std::uint32_t>(1)) {
            view.id = 1;
            view.layers.hide("survey");
            return view;
        }
        if (!id || *id == 2) {
            view.id = 2;
            view.area = Box2(Point2(5, -1), Point2(25, 1));
            return view;
        }
        return katana::core::makeError(ErrorCode::NotFound, "no plan view",
                                       std::to_string(*id));
    };
    // View 1 hides "survey" and with it "survey/trees": the roads are left.
    EXPECT_EQ(match("VIEW 1", views), (std::vector<EntityId>{ln, tx}));
    // The active view: what lies in its visible area.
    EXPECT_EQ(match("VIEW", views), (std::vector<EntityId>{pB, pC}));
    EXPECT_EQ(match("VIEW WHERE PROP=code:TREE*", views), (std::vector<EntityId>{pC}));
    EXPECT_EQ(asked, (std::vector<std::optional<std::uint32_t>>{1, std::nullopt, std::nullopt}));
    // The provider's refusal is the verb's.
    auto unknown = matchScope(document, parsed("VIEW 7"), views);
    ASSERT_FALSE(unknown.ok());
    EXPECT_EQ(unknown.error().code, ErrorCode::NotFound);

    // Headless there is no view: refused, pointing at the window typed in.
    auto headless = matchScope(document, parsed("VIEW"), {});
    ASSERT_FALSE(headless.ok());
    EXPECT_EQ(headless.error().code, ErrorCode::InvalidState);
    EXPECT_TRUE(contains(headless.error().message, "AREA x0,y0,x1,y1")) << headless.error().message;

    // The resolved scope owns the view's layers: it may outlive the provider's
    // answer, be copied and be moved.
    auto resolved = resolveScope(parsed("VIEW 1"), views);
    ASSERT_TRUE(resolved.ok());
    const ResolvedScope copy = *resolved;
    const ResolvedScope moved = std::move(resolved).value();
    ASSERT_NE(copy.scope.view, nullptr);
    EXPECT_TRUE(copy.scope.view->hides("survey/trees"));
    EXPECT_EQ(moved.scope.view, copy.scope.view);
    EXPECT_EQ(copy.view, std::optional<std::uint32_t>(1));
}

TEST_F(ScopeVerbsTest, ViewExtentsTakesWhatTheViewDrawsAnywhereNotOnlyWhatIsOnScreen)
{
    // Global Modify's "What a view shows" without "Only what is on screen":
    // the view's own hidden layers, and no area.
    std::string rest;
    const ScopeWords active = parsed("VIEW EXTENTS SPACING 20", &rest);
    EXPECT_EQ(active.source, ScopeSource::View);
    EXPECT_FALSE(active.view.has_value());
    EXPECT_TRUE(active.extents);
    EXPECT_EQ(rest, "SPACING 20");
    const ScopeWords third = parsed("view 3 extents WHERE DRAWN", &rest);
    EXPECT_EQ(third.view, std::optional<std::uint32_t>(3));
    EXPECT_TRUE(third.extents);
    EXPECT_TRUE(third.filter.drawnOnly);
    EXPECT_FALSE(parsed("VIEW 3").extents);
    // Anywhere else it is refused by the word, as ONLY is.
    EXPECT_TRUE(contains(refusal("DRAWING EXTENTS").describe(), "EXTENTS follows VIEW"));
    EXPECT_TRUE(contains(refusal("EXTENTS").describe(), "EXTENTS follows VIEW"));

    // Written back and read as the same.
    ScopeWords words;
    words.source = ScopeSource::View;
    words.view = 4;
    words.extents = true;
    auto line = formatScopeWords(words);
    ASSERT_TRUE(line.ok());
    EXPECT_EQ(*line, "VIEW 4 EXTENTS");
    const ScopeWords back = parsed(*line);
    EXPECT_EQ(back.view, words.view);
    EXPECT_TRUE(back.extents);

    // One plan view, hiding nothing, showing x 5..25, y -1..1: pB and pC on
    // screen; all five drawn somewhere.
    const ScopeViewProvider views = [](std::optional<std::uint32_t>)
        -> katana::core::Result<ScopeView> {
        ScopeView view;
        view.id = 2;
        view.area = Box2(Point2(5, -1), Point2(25, 1));
        return view;
    };
    EXPECT_EQ(match("VIEW", views), (std::vector<EntityId>{pB, pC}));
    EXPECT_EQ(match("VIEW EXTENTS", views), (std::vector<EntityId>{pA, pB, pC, ln, tx}));
    EXPECT_EQ(match("VIEW 2 EXTENTS WHERE TYPE=point", views),
              (std::vector<EntityId>{pA, pB, pC}));
    // The record says where "on screen" was, or that the area was dropped.
    auto onScreen = matchScope(document, parsed("VIEW"), views);
    ASSERT_TRUE(onScreen.ok());
    EXPECT_EQ(scopeRecord(*onScreen), "scope=view view=2 area=5,-1,25,1 matched=2");
    auto anywhere = matchScope(document, parsed("VIEW EXTENTS"), views);
    ASSERT_TRUE(anywhere.ok());
    EXPECT_EQ(scopeRecord(*anywhere), "scope=view view=2 extents=yes matched=5");
    // Headless it is still the window's word, refused naming AREA.
    auto headless = matchScope(document, parsed("VIEW EXTENTS"), {});
    ASSERT_FALSE(headless.ok());
    EXPECT_TRUE(contains(headless.error().message, "AREA")) << headless.error().message;
}

TEST_F(ScopeVerbsTest, AWorkspacesViewSetAnswersViewAsTheWindowDoes)
{
    ViewSet views;
    // No plan view open, and none named: refused, naming AREA.
    auto none = scopeViewOf(views, std::nullopt);
    ASSERT_FALSE(none.ok());
    EXPECT_EQ(none.error().code, ErrorCode::InvalidState);
    EXPECT_TRUE(contains(none.error().message, "AREA x0,y0,x1,y1")) << none.error().message;

    // A plan view centred on (15, 0) at 10 px a unit in 200 x 20 px shows
    // x 15 -+ 10 and y 0 -+ 1, by hand; it hides the roads. A 3D view hides
    // the survey layer (and so survey/trees).
    ViewState& plan = views.add(ViewKind::Plan);
    plan.plan.center = Point2(15, 0);
    plan.plan.scale = 10.0;
    plan.plan.widthPixels = 200.0;
    plan.plan.heightPixels = 20.0;
    plan.layers.hide("roads");
    ViewState& model = views.add(ViewKind::Model3D);
    model.layers.hide("survey");

    auto active = scopeViewOf(views, std::nullopt);
    ASSERT_TRUE(active.ok()) << active.error().describe();
    EXPECT_EQ(active->id, plan.id);
    ASSERT_TRUE(active->area.has_value());
    EXPECT_EQ(active->area->min, Point2(5, -1));
    EXPECT_EQ(active->area->max, Point2(25, 1));
    EXPECT_TRUE(active->layers.hides("roads"));

    auto byId = scopeViewOf(views, model.id);
    ASSERT_TRUE(byId.ok()) << byId.error().describe();
    EXPECT_EQ(byId->id, model.id);
    EXPECT_FALSE(byId->area.has_value());
    EXPECT_TRUE(byId->layers.hides("survey/trees"));

    auto unknown = scopeViewOf(views, 999u);
    ASSERT_FALSE(unknown.ok());
    EXPECT_EQ(unknown.error().code, ErrorCode::NotFound);
    EXPECT_EQ(unknown.error().context, "999");

    // As the interpreter's provider: on screen pB and pC; anywhere every
    // point, the roads being hidden; the 3D view the roads alone.
    const ScopeViewProvider provider = [&views](std::optional<std::uint32_t> id) {
        return scopeViewOf(views, id);
    };
    EXPECT_EQ(match("VIEW", provider), (std::vector<EntityId>{pB, pC}));
    EXPECT_EQ(match("VIEW EXTENTS", provider), (std::vector<EntityId>{pA, pB, pC}));
    EXPECT_EQ(match("VIEW " + std::to_string(model.id), provider),
              (std::vector<EntityId>{ln, tx}));
}

TEST_F(ScopeVerbsTest, TheRecordSaysWhatTheScopeTook)
{
    const auto record = [this](const std::string& line, const ScopeViewProvider& views = {}) {
        auto matched = matchScope(document, parsed(line), views);
        EXPECT_TRUE(matched.ok()) << line;
        return matched.ok() ? scopeRecord(*matched) : std::string{};
    };
    EXPECT_EQ(record("DRAWING"), "scope=drawing matched=5");
    EXPECT_EQ(record("SELECTION"), "scope=selection matched=0");
    EXPECT_EQ(record("LAYERS survey,roads ONLY WHERE TYPE=point"),
              "scope=layers layers=survey,roads sublayers=no where=\"TYPE=point\" matched=2");
    EXPECT_EQ(record("AREA 0,4,8,6"), "scope=area area=0,4,8,6 matched=1");
    EXPECT_EQ(record("DRAWING WHERE \"TEXT=CH *\" DRAWN"),
              "scope=drawing where=\"TEXT=CH * DRAWN\" matched=1");
    const ScopeViewProvider views = [](std::optional<std::uint32_t>)
        -> katana::core::Result<ScopeView> {
        ScopeView view;
        view.id = 3;
        return view;
    };
    EXPECT_EQ(record("VIEW", views), "scope=view view=3 matched=5");
    EXPECT_EQ(recordValue("W1"), "W1");
    EXPECT_EQ(recordValue("Site Services"), "\"Site Services\"");
    EXPECT_EQ(recordValue("a\"b\\c"), "\"a\\\"b\\\\c\"");
    EXPECT_EQ(recordValue(""), "\"\"");
}

// ---- MODIFY reads them ----------------------------------------------------------------------

TEST_F(ScopeVerbsTest, ModifyTakesTheAreaAndTheWindowsView)
{
    CommandInterpreter interpreter(document);
    auto reply = interpreter.run("MODIFY AREA 0,4,8,6 SET COLOUR=#0000FF");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_EQ(*reply, "1 entity matched; changing 1 entity. One UNDO restores it.");
    const auto colourOf = [this](EntityId id) { return document.model().entities.find(id)->color; };
    EXPECT_EQ(colourOf(ln), std::optional<Color>(Color{0, 0, 255, 255}));
    EXPECT_EQ(colourOf(tx), std::nullopt);

    // Headless, VIEW is refused naming AREA, and nothing changes.
    const std::size_t steps = document.history().undoCount();
    auto headless = interpreter.run("MODIFY VIEW SET COLOUR=#00FF00");
    ASSERT_FALSE(headless.ok());
    EXPECT_TRUE(contains(headless.error().message, "AREA")) << headless.error().message;
    EXPECT_EQ(document.history().undoCount(), steps);

    // The window's view: here one hiding the roads, so the points are taken.
    interpreter.setScopeContext([](std::optional<std::uint32_t>) -> katana::core::Result<ScopeView> {
        ScopeView view;
        view.id = 1;
        view.layers.hide("roads");
        return view;
    });
    reply = interpreter.run("MODIFY VIEW WHERE PROP=code:TREE* SET COLOUR=#00FF00 PREVIEW");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_EQ(*reply, "preview: 2 entities matched; changing 2 entities.");
    reply = interpreter.run("GM VIEW WHERE PROP=code:TREE* SET COLOUR=#00FF00");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_EQ(colourOf(pA), std::optional<Color>(Color{0, 255, 0, 255}));
    EXPECT_EQ(colourOf(pC), std::optional<Color>(Color{0, 255, 0, 255}));
    EXPECT_EQ(colourOf(pB), std::nullopt);

    // The scope words' refusals are MODIFY's.
    EXPECT_FALSE(interpreter.run("MODIFY SELECTION DRAWING SET COLOUR=#FF0000").ok());
    EXPECT_FALSE(interpreter.run("MODIFY AREA 1,2 SET COLOUR=#FF0000").ok());
    EXPECT_FALSE(interpreter.run("MODIFY DRAWING WHERE TYPE=point").ok());
}
