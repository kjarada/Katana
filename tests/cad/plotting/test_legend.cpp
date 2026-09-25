// The smart legend (cad/plotting/legend.hpp): what a sheet's plans show,
// grouped by what prints, labelled and ordered for a reader; its scope stored
// with the viewport; and the panel's flow into columns.
//
// Every window below is worked out by hand from the viewport: a rectangle of
// W x H mm at 1 : S shows W x S / 1000 by H x S / 1000 metres about its centre,
// turned by its rotation.

#include <gtest/gtest.h>

#include <cmath>
#include <string>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/legend.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/cad/plotting/sheet_json.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/survey_map.hpp"
#include "katana/math/numerics.hpp"

using katana::cad::Document;
using katana::core::ErrorCode;
using katana::entity::Color;
using katana::entity::Entity;
using katana::entity::HatchPattern;
using katana::entity::Layer;
using katana::entity::Model;
using katana::entity::PointGeometry;
using katana::entity::Style;
using katana::entity::TextGeometry;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using namespace katana::cad::plotting;
namespace cmd = katana::commands;

namespace {

const Color kRed{220, 0, 0, 255};
const Color kBlue{0, 0, 200, 255};

void addLayer(Model& model, const std::string& name, Color colour = {}, bool visible = true)
{
    Layer layer;
    layer.name = name;
    layer.color = colour;
    layer.visible = visible;
    ASSERT_TRUE(model.layers.add(layer).ok());
}

void addStyle(Model& model, Style style) { ASSERT_TRUE(model.styles.add(std::move(style)).ok()); }

katana::entity::EntityId add(Model& model, katana::entity::Geometry geometry,
                             const std::string& layer, const std::string& style = {})
{
    Entity entity;
    entity.geometry = std::move(geometry);
    entity.layer = layer;
    entity.style = style;
    auto id = model.entities.add(std::move(entity));
    EXPECT_TRUE(id.ok());
    return id.ok() ? *id : 0;
}

Segment2 line(double x0, double y0, double x1, double y1)
{
    return Segment2{Point2(x0, y0), Point2(x1, y1)};
}

// A plan of W x H mm at 1 : scale about `centre`, turned by `degrees`.
Viewport plan(std::string id, double w, double h, double scale, Point2 centre,
              double degrees = 0.0)
{
    Viewport viewport;
    viewport.id = std::move(id);
    viewport.kind = ViewportKind::Plan;
    viewport.rect = Box2(Point2(30.0, 40.0), Point2(30.0 + w, 40.0 + h));
    viewport.scale = scale;
    viewport.centre = centre;
    viewport.rotation = degrees * katana::math::kDegToRad;
    return viewport;
}

Viewport legendPanel(std::string id = "vpL")
{
    Viewport viewport;
    viewport.id = std::move(id);
    viewport.kind = ViewportKind::Legend;
    viewport.rect = Box2(Point2(300.0, 40.0), Point2(400.0, 140.0));
    return viewport;
}

SheetSet oneSheet(std::vector<Viewport> viewports)
{
    SheetSet set;
    Sheet sheet;
    sheet.id = "s1";
    sheet.viewports = std::move(viewports);
    set.sheets.push_back(std::move(sheet));
    return set;
}

std::vector<std::string> labels(const Legend& legend)
{
    std::vector<std::string> out;
    for (const LegendEntry& entry : legend.entries) {
        out.push_back(entry.label);
    }
    return out;
}

Legend legendOf(const Model& model, const SheetSet& set, std::size_t sheet = 0,
                LegendScope scope = LegendScope::ThisSheet, const LegendOptions& options = {})
{
    auto legend = computeLegend(model, set, sheet, scope, options);
    EXPECT_TRUE(legend.ok()) << (legend.ok() ? "" : legend.error().describe());
    return legend.ok() ? *legend : Legend{};
}

} // namespace

// ---- what the plans show --------------------------------------------------------

TEST(SheetLegend, ListsOnlyWhatThePlansWindowShows)
{
    Model model;
    addLayer(model, "ROAD");
    addLayer(model, "FENCE");
    addLayer(model, "FAR");
    // 100 x 50 mm at 1:1000 about the origin: x -50..50, y -25..25.
    add(model, line(-10.0, 0.0, 10.0, 0.0), "ROAD");
    add(model, line(40.0, 20.0, 60.0, 22.0), "FENCE"); // crosses the right edge at (50, 21)
    add(model, line(200.0, 0.0, 300.0, 0.0), "FAR");
    add(model, line(-20.0, 25.5, 20.0, 25.5), "FAR"); // half a metre above the top edge
    const SheetSet set = oneSheet({plan("vp1", 100.0, 50.0, 1000.0, Point2(0.0, 0.0)),
                                   legendPanel()});
    const Legend legend = legendOf(model, set);
    EXPECT_EQ(labels(legend), (std::vector<std::string>{"FENCE", "ROAD"}));
    EXPECT_EQ(legend.scope, LegendScope::ThisSheet);
    EXPECT_EQ(legend.windows, 1u);
    EXPECT_EQ(legend.scale, 1000.0);
}

TEST(SheetLegend, ATurnedViewportShowsItsRotatedRectangleNotTheBoxAroundIt)
{
    Model model;
    for (const char* name : {"ON", "CORNER", "ACROSS", "RING", "CURVE"}) {
        addLayer(model, name);
    }
    // A strip 100 x 10 mm at 1:1000 turned 45 degrees about the origin: 100 m
    // along the diagonal y = x and 10 m across it. The box around it reaches
    // (+-38.9, +-38.9).
    const SheetSet set =
        oneSheet({plan("vp1", 100.0, 10.0, 1000.0, Point2(0.0, 0.0), 45.0), legendPanel()});
    // On the strip, 42 m along it.
    add(model, PointGeometry{Point2(30.0, 30.0)}, "ON");
    // In the box but 28 m off the strip's middle line: 40 cos 45 along, 40 sin 45 across.
    add(model, PointGeometry{Point2(40.0, 0.0)}, "CORNER");
    // A line whose own box sits inside the strip's box, and which never meets the strip.
    add(model, line(30.0, 0.0, 38.0, 0.0), "CORNER");
    // A line square across the strip's middle.
    add(model, line(-20.0, 20.0, 20.0, -20.0), "ACROSS");
    // A circle round the strip, and one in the empty corner of its box.
    add(model, Circle2{Point2(0.0, 0.0), 30.0}, "RING");
    add(model, Circle2{Point2(30.0, -30.0), 5.0}, "CORNER");
    // An arc through the strip, and one bowing away from it in the corner.
    add(model, Arc2{Point2(0.0, 0.0), 20.0, 0.0, katana::math::kHalfPi}, "CURVE");
    add(model, Arc2{Point2(35.0, -35.0), 6.0, 0.0, katana::math::kTwoPi * 0.9}, "CORNER");
    const Legend legend = legendOf(model, set);
    EXPECT_EQ(labels(legend), (std::vector<std::string>{"ON", "ACROSS", "CURVE", "RING"}));
}

TEST(SheetLegend, HiddenLayersAndHiddenEntitiesAreNotListed)
{
    Model model;
    addLayer(model, "SHOWN");
    addLayer(model, "OFF", {}, false);  // hidden in the document
    addLayer(model, "HIDDEN_HERE");     // hidden in the viewport
    addLayer(model, "SURVEY");          // a parent hidden in the viewport ...
    addLayer(model, "SURVEY/TREES");    // ... hides its child
    add(model, line(0.0, 0.0, 1.0, 0.0), "SHOWN");
    add(model, line(0.0, 1.0, 1.0, 1.0), "OFF");
    add(model, line(0.0, 2.0, 1.0, 2.0), "HIDDEN_HERE");
    add(model, PointGeometry{Point2(0.0, 3.0)}, "SURVEY/TREES");
    Entity invisible;
    invisible.geometry = line(0.0, 4.0, 1.0, 4.0);
    invisible.layer = "SHOWN";
    invisible.visible = false;
    ASSERT_TRUE(model.entities.add(invisible).ok());
    Entity invisibleOnly = invisible;
    invisibleOnly.layer = "SURVEY"; // its layer has nothing else to list
    ASSERT_TRUE(model.entities.add(invisibleOnly).ok());
    Viewport view = plan("vp1", 100.0, 100.0, 500.0, Point2(0.0, 0.0));
    view.hiddenLayers.hide("HIDDEN_HERE");
    view.hiddenLayers.hide("SURVEY");
    const Legend legend = legendOf(model, oneSheet({view, legendPanel()}));
    ASSERT_EQ(labels(legend), (std::vector<std::string>{"SHOWN"}));
    EXPECT_EQ(legend.entries[0].count, 1u); // the invisible one is not counted
}

TEST(SheetLegend, AHatchedAreaCoveringTheWholeWindowIsShown)
{
    Model model;
    addLayer(model, "PARK");
    addLayer(model, "OUTLINE");
    ASSERT_TRUE(model.hatchPatterns.add(HatchPattern{"SOLIDFILL", "", true, {}}).ok());
    Layer park = *model.layers.find("PARK");
    park.hatchPattern = "SOLIDFILL";
    ASSERT_TRUE(model.layers.update(park).ok());
    // Both boundaries lie wholly outside a 50 x 50 m window they surround.
    const Polyline2 around{{Point2(-100.0, -100.0), Point2(100.0, -100.0), Point2(100.0, 100.0),
                            Point2(-100.0, 100.0)},
                           true};
    add(model, around, "PARK");
    add(model, around, "OUTLINE"); // not hatched: no ink inside the window
    const Legend legend =
        legendOf(model, oneSheet({plan("vp1", 100.0, 100.0, 500.0, Point2(0.0, 0.0))}));
    ASSERT_EQ(legend.entries.size(), 1u);
    EXPECT_EQ(legend.entries[0].label, "PARK");
    EXPECT_EQ(legend.entries[0].kind, LegendKind::Area);
    EXPECT_EQ(legend.entries[0].hatchPattern, "SOLIDFILL");
}

TEST(SheetLegend, ASymbolReachesHalfItsSizePastItsPoint)
{
    Model model;
    addLayer(model, "TREES");
    addLayer(model, "PEGS");
    Style tree;
    tree.name = "Tree";
    tree.symbol = "circle";
    tree.symbolSize = 6.0; // 3 m either side of its point
    addStyle(model, tree);
    // A 100 x 100 m window: the edge is at x = 50.
    add(model, PointGeometry{Point2(52.0, 0.0)}, "TREES", "Tree"); // its circle reaches 49
    add(model, PointGeometry{Point2(52.0, 5.0)}, "PEGS");          // a plain mark does not
    const Legend legend =
        legendOf(model, oneSheet({plan("vp1", 100.0, 100.0, 1000.0, Point2(0.0, 0.0))}));
    ASSERT_EQ(labels(legend), (std::vector<std::string>{"Tree"}));
    EXPECT_EQ(legend.entries[0].kind, LegendKind::Symbol);
    EXPECT_EQ(legend.entries[0].symbol, "circle");
    EXPECT_EQ(legend.entries[0].symbolSize, 6.0);
}

// ---- grouped by what prints, in order ------------------------------------------------

TEST(SheetLegend, GroupsByStyleElseLayerAndByKindOfMark)
{
    Model model;
    addLayer(model, "BOUNDARY", kBlue);
    addLayer(model, "WEST");
    addLayer(model, "EAST");
    addLayer(model, "ANNOTATION");
    Style fence;
    fence.name = "Fence";
    fence.color = kRed;
    fence.lineWeight = 0.35;
    addStyle(model, fence);
    Style pit;
    pit.name = "Pit";
    pit.symbol = "square";
    addStyle(model, pit);
    // One style on two layers is one entry.
    add(model, line(0.0, 0.0, 10.0, 0.0), "WEST", "Fence");
    add(model, line(0.0, 5.0, 10.0, 5.0), "EAST", "Fence");
    // A style naming no style of the drawing prints as its layer.
    add(model, line(0.0, 8.0, 10.0, 8.0), "BOUNDARY", "Gone");
    // A point and a line of one layer print differently.
    add(model, PointGeometry{Point2(1.0, 1.0)}, "BOUNDARY");
    add(model, line(0.0, 9.0, 10.0, 9.0), "BOUNDARY");
    add(model, PointGeometry{Point2(2.0, 2.0)}, "WEST", "Pit");
    add(model, TextGeometry{Point2(3.0, 3.0), "LOT 7", 1.0, 0.0}, "ANNOTATION");
    // A dimension explains itself.
    add(model, katana::entity::DimensionGeometry{Point2(0.0, 0.0), Point2(5.0, 0.0), 1.0, {}},
        "ANNOTATION");
    const Legend legend =
        legendOf(model, oneSheet({plan("vp1", 100.0, 100.0, 500.0, Point2(5.0, 5.0))}));
    ASSERT_EQ(legend.entries.size(), 5u);

    const LegendEntry& symbol = legend.entries[0];
    EXPECT_EQ(symbol.kind, LegendKind::Symbol);
    EXPECT_EQ(symbol.label, "Pit");
    EXPECT_EQ(symbol.style, "Pit");
    EXPECT_TRUE(symbol.layer.empty());

    const LegendEntry& point = legend.entries[1];
    EXPECT_EQ(point.kind, LegendKind::Point);
    EXPECT_EQ(point.label, "BOUNDARY");
    EXPECT_EQ(point.layer, "BOUNDARY");
    EXPECT_TRUE(point.style.empty());
    EXPECT_EQ(point.colour, kBlue);

    const LegendEntry& boundary = legend.entries[2];
    EXPECT_EQ(boundary.kind, LegendKind::Line);
    EXPECT_EQ(boundary.label, "BOUNDARY");
    EXPECT_EQ(boundary.count, 2u); // its own line and the one naming a missing style

    const LegendEntry& fenceLine = legend.entries[3];
    EXPECT_EQ(fenceLine.kind, LegendKind::Line);
    EXPECT_EQ(fenceLine.label, "Fence");
    EXPECT_EQ(fenceLine.count, 2u);
    EXPECT_EQ(fenceLine.colour, kRed);
    EXPECT_EQ(fenceLine.lineWeight, 0.35);

    EXPECT_EQ(legend.entries[4].kind, LegendKind::Text);
    EXPECT_EQ(legend.entries[4].label, "ANNOTATION");
}

TEST(SheetLegend, EntriesAreSortedSymbolsPointsLinesAreasTextsThenByLabel)
{
    Model model;
    ASSERT_TRUE(model.hatchPatterns.add(HatchPattern{"SOLIDFILL", "", true, {}}).ok());
    for (const char* name : {"cherry", "Banana", "apple", "Zone"}) {
        addLayer(model, name);
    }
    Layer zone = *model.layers.find("Zone");
    zone.hatchPattern = "SOLIDFILL";
    ASSERT_TRUE(model.layers.update(zone).ok());
    const Polyline2 square{{Point2(0.0, 0.0), Point2(4.0, 0.0), Point2(4.0, 4.0), Point2(0.0, 4.0)},
                           true};
    add(model, TextGeometry{Point2(1.0, 1.0), "A", 1.0, 0.0}, "apple");
    add(model, square, "Zone");
    add(model, line(0.0, 0.0, 1.0, 1.0), "cherry");
    add(model, line(0.0, 0.0, 1.0, 1.0), "Banana");
    add(model, line(0.0, 0.0, 1.0, 1.0), "apple");
    add(model, PointGeometry{Point2(1.0, 1.0)}, "cherry");
    Style mark;
    mark.name = "mark";
    mark.symbol = "cross";
    addStyle(model, mark);
    add(model, PointGeometry{Point2(2.0, 1.0)}, "Zone", "mark");
    const Legend legend =
        legendOf(model, oneSheet({plan("vp1", 100.0, 100.0, 500.0, Point2(0.0, 0.0))}));
    std::vector<std::pair<LegendKind, std::string>> order;
    for (const LegendEntry& entry : legend.entries) {
        order.emplace_back(entry.kind, entry.label);
    }
    const std::vector<std::pair<LegendKind, std::string>> expected = {
        {LegendKind::Symbol, "mark"}, {LegendKind::Point, "cherry"},
        {LegendKind::Line, "apple"},  {LegendKind::Line, "Banana"},
        {LegendKind::Line, "cherry"}, {LegendKind::Area, "Zone"},
        {LegendKind::Text, "apple"}};
    EXPECT_EQ(order, expected);
}

TEST(SheetLegend, AnEntryLooksAsMostOfItsEntitiesDoATieToTheFirst)
{
    Model model;
    addLayer(model, "KERB", kBlue);
    Entity red;
    red.geometry = line(0.0, 0.0, 1.0, 0.0);
    red.layer = "KERB";
    red.color = kRed;
    Entity byLayer = red;
    byLayer.color.reset();
    ASSERT_TRUE(model.entities.add(byLayer).ok());
    ASSERT_TRUE(model.entities.add(red).ok());
    const SheetSet set = oneSheet({plan("vp1", 100.0, 100.0, 500.0, Point2(0.0, 0.0))});
    // One of each: the first, blue by its layer.
    Legend legend = legendOf(model, set);
    ASSERT_EQ(legend.entries.size(), 1u);
    EXPECT_EQ(legend.entries[0].colour, kBlue);
    // Two red to one blue.
    ASSERT_TRUE(model.entities.add(red).ok());
    legend = legendOf(model, set);
    ASSERT_EQ(legend.entries.size(), 1u);
    EXPECT_EQ(legend.entries[0].colour, kRed);
    EXPECT_EQ(legend.entries[0].count, 3u);
}

// ---- labels from the survey code library ------------------------------------------------

TEST(SheetLegend, ACodedStyleIsLabelledByItsCodesDescription)
{
    Model model;
    addLayer(model, "SERVICES");
    Style water;
    water.name = "WATR Main";
    addStyle(model, water);
    Style plain;
    plain.name = "Plain";
    addStyle(model, plain);
    const auto coded = [&model](const std::string& code, const std::string& style, double y) {
        Entity entity;
        entity.geometry = line(0.0, y, 10.0, y);
        entity.layer = "SERVICES";
        entity.style = style;
        entity.properties["code"] = code;
        ASSERT_TRUE(model.entities.add(entity).ok());
    };
    coded("WM01", "WATR Main", 0.0);
    coded("WM02", "WATR Main", 1.0);
    add(model, line(0.0, 2.0, 10.0, 2.0), "SERVICES", "WATR Main"); // uncoded: no vote
    // Two codes of different descriptions share one appearance, and so one style.
    coded("FE", "Plain", 3.0);
    coded("WA", "Plain", 4.0);

    katana::entity::SurveyMap map;
    katana::entity::SurveyRule main;
    main.key = "WM*";
    main.comment = "Water main";
    main.linestyle = "WATR Main";
    ASSERT_TRUE(map.add(main).ok());
    katana::entity::SurveyRule fence;
    fence.key = "FE";
    fence.comment = "Fence";
    ASSERT_TRUE(map.add(fence).ok());
    katana::entity::SurveyRule wall;
    wall.key = "WA";
    wall.comment = "Wall";
    ASSERT_TRUE(map.add(wall).ok());

    const SheetSet set = oneSheet({plan("vp1", 100.0, 100.0, 500.0, Point2(0.0, 0.0))});
    LegendOptions options;
    options.codes = &map;
    const Legend legend = legendOf(model, set, 0, LegendScope::ThisSheet, options);
    ASSERT_EQ(legend.entries.size(), 2u);
    EXPECT_EQ(legend.entries[0].label, "Plain"); // the descriptions disagree
    EXPECT_TRUE(legend.entries[0].code.empty());
    EXPECT_EQ(legend.entries[1].label, "Water main");
    EXPECT_EQ(legend.entries[1].code, "WM01");
    EXPECT_EQ(legend.entries[1].style, "WATR Main");
    EXPECT_EQ(legend.entries[1].count, 3u);

    // Without a library, every label is a name.
    const Legend names = legendOf(model, set);
    EXPECT_EQ(labels(names), (std::vector<std::string>{"Plain", "WATR Main"}));

    // A coded entity whose code the library does not describe breaks the vote.
    coded("ZZ9", "WATR Main", 5.0);
    const Legend broken = legendOf(model, set, 0, LegendScope::ThisSheet, options);
    EXPECT_EQ(labels(broken), (std::vector<std::string>{"Plain", "WATR Main"}));
}

TEST(SheetLegend, APointsCodeIsLookedUpByItsStringName)
{
    Model model;
    addLayer(model, "SURVEY");
    Style pit;
    pit.name = "Pit";
    pit.symbol = "square";
    addStyle(model, pit);
    Entity point;
    point.geometry = PointGeometry{Point2(1.0, 1.0)};
    point.layer = "SURVEY";
    point.style = "Pit";
    point.properties["code"] = std::string("SP1 ST"); // a string name and a linework control
    ASSERT_TRUE(model.entities.add(point).ok());
    katana::entity::SurveyMap map;
    katana::entity::SurveyRule rule;
    rule.key = "SP*";
    rule.comment = "Stormwater pit";
    ASSERT_TRUE(map.add(rule).ok());
    LegendOptions options;
    options.codes = &map;
    const Legend legend =
        legendOf(model, oneSheet({plan("vp1", 100.0, 100.0, 500.0, Point2(0.0, 0.0))}), 0,
                 LegendScope::ThisSheet, options);
    ASSERT_EQ(legend.entries.size(), 1u);
    EXPECT_EQ(legend.entries[0].label, "Stormwater pit");
    EXPECT_EQ(legend.entries[0].code, "SP1");
}

// ---- scopes -------------------------------------------------------------------------------

TEST(SheetLegend, ScopesThisSheetWholeSetAndWholeDrawing)
{
    Model model;
    for (const char* name : {"WEST", "EAST", "NOWHERE"}) {
        addLayer(model, name);
    }
    add(model, line(-100.0, 0.0, -90.0, 0.0), "WEST");
    add(model, line(90.0, 0.0, 100.0, 0.0), "EAST");
    add(model, line(0.0, 500.0, 1.0, 500.0), "NOWHERE");
    SheetSet set;
    Sheet west;
    west.id = "s1";
    west.viewports = {plan("vp1", 40.0, 40.0, 1000.0, Point2(-95.0, 0.0)), legendPanel("vp2")};
    Sheet east;
    east.id = "s2";
    east.viewports = {plan("vp3", 40.0, 40.0, 500.0, Point2(95.0, 0.0))};
    set.sheets = {west, east};

    const Legend own = legendOf(model, set, 0, LegendScope::ThisSheet);
    EXPECT_EQ(labels(own), (std::vector<std::string>{"WEST"}));
    EXPECT_EQ(own.windows, 1u);

    const Legend all = legendOf(model, set, 0, LegendScope::WholeSet);
    EXPECT_EQ(labels(all), (std::vector<std::string>{"EAST", "WEST"}));
    EXPECT_EQ(all.scope, LegendScope::WholeSet);
    EXPECT_EQ(all.windows, 2u);
    EXPECT_EQ(all.scale, 1000.0); // the plan beside the legend, not the first of the set's

    const Legend drawing = legendOf(model, set, 0, LegendScope::WholeDrawing);
    EXPECT_EQ(labels(drawing), (std::vector<std::string>{"EAST", "NOWHERE", "WEST"}));
    EXPECT_EQ(drawing.scope, LegendScope::WholeDrawing);
    EXPECT_EQ(drawing.windows, 0u);
}

TEST(SheetLegend, ASheetWithoutAPlanFallsBackToTheSetAndASetWithoutOneToTheDrawing)
{
    Model model;
    addLayer(model, "SEEN");
    addLayer(model, "UNSEEN");
    add(model, line(0.0, 0.0, 1.0, 0.0), "SEEN");
    add(model, line(0.0, 900.0, 1.0, 900.0), "UNSEEN");
    SheetSet set;
    Sheet legendSheet;
    legendSheet.id = "s1";
    Viewport unplaced = plan("vp9", 10.0, 10.0, 500.0, Point2(0.0, 900.0));
    unplaced.rect = Box2{}; // not on the paper: shows nothing
    legendSheet.viewports = {legendPanel("vp1"), unplaced};
    Sheet planSheet;
    planSheet.id = "s2";
    planSheet.viewports = {plan("vp2", 100.0, 100.0, 250.0, Point2(0.0, 0.0))};
    set.sheets = {legendSheet, planSheet};

    const Legend fromSet = legendOf(model, set, 0, LegendScope::ThisSheet);
    EXPECT_EQ(fromSet.scope, LegendScope::WholeSet);
    EXPECT_EQ(labels(fromSet), (std::vector<std::string>{"SEEN"}));
    EXPECT_EQ(fromSet.scale, 250.0);

    set.sheets.pop_back();
    const Legend fromDrawing = legendOf(model, set, 0, LegendScope::ThisSheet);
    EXPECT_EQ(fromDrawing.scope, LegendScope::WholeDrawing);
    EXPECT_EQ(labels(fromDrawing), (std::vector<std::string>{"SEEN", "UNSEEN"}));
    EXPECT_EQ(fromDrawing.scale, 0.0);
}

TEST(SheetLegend, AnEntityTwoPlansShowIsCountedOnce)
{
    Model model;
    addLayer(model, "ROAD");
    add(model, line(-5.0, 0.0, 5.0, 0.0), "ROAD");
    add(model, line(-5.0, 1.0, 5.0, 1.0), "ROAD");
    const SheetSet set = oneSheet({plan("vp1", 50.0, 50.0, 500.0, Point2(0.0, 0.0)),
                                   plan("vp2", 50.0, 50.0, 500.0, Point2(1.0, 0.0), 30.0)});
    const Legend legend = legendOf(model, set);
    ASSERT_EQ(legend.entries.size(), 1u);
    EXPECT_EQ(legend.entries[0].count, 2u);
    EXPECT_EQ(legend.windows, 2u);
}

TEST(SheetLegend, AnAutomaticPlanIsFittedOrResolvedByTheCaller)
{
    Model model;
    addLayer(model, "SITE");
    add(model, line(1000.0, 1000.0, 1100.0, 1050.0), "SITE");
    Viewport automatic = plan("vp1", 100.0, 100.0, 500.0, Point2(0.0, 0.0));
    automatic.autoScale = true;
    automatic.autoCentre = true;
    const SheetSet set = oneSheet({automatic});
    // Fitted: centred on (1050, 1025); 104 m needs 1:1040, so 1:1250.
    const PlanWindow fitted = fittedPlanWindow(model, automatic);
    EXPECT_NEAR(fitted.centre.x, 1050.0, 1e-9);
    EXPECT_NEAR(fitted.centre.y, 1025.0, 1e-9);
    EXPECT_EQ(fitted.scale, 1250.0);
    const Legend legend = legendOf(model, set);
    EXPECT_EQ(labels(legend), (std::vector<std::string>{"SITE"}));
    EXPECT_EQ(legend.scale, 1250.0);
    // A caller's window is taken as it is: this one looks at the origin.
    LegendOptions options;
    options.window = [](const Viewport&) { return PlanWindow{200.0, Point2(0.0, 0.0)}; };
    const Legend elsewhere = legendOf(model, set, 0, LegendScope::ThisSheet, options);
    EXPECT_TRUE(elsewhere.entries.empty());
    EXPECT_EQ(elsewhere.scale, 200.0);
}

TEST(SheetLegend, AnIndexPastTheEndIsRefused)
{
    Model model;
    const auto legend = computeLegend(model, oneSheet({}), 1, LegendScope::WholeDrawing);
    ASSERT_FALSE(legend.ok());
    EXPECT_EQ(legend.error().code, ErrorCode::InvalidArgument);
}

// ---- through a document ----------------------------------------------------------------------

TEST(SheetLegend, TheDocumentsLegendUsesItsIndexAndCodesAndGivesTheSameAnswer)
{
    Document document;
    ASSERT_TRUE(document.execute(cmd::createLayer(Layer{"PITS"})).ok());
    ASSERT_TRUE(document.execute(cmd::createLayer(Layer{"FAR"})).ok());
    std::vector<Entity> entities;
    for (int i = 0; i < 400; ++i) {
        Entity near;
        near.geometry = PointGeometry{Point2(i % 20, i / 20)};
        near.layer = "PITS";
        near.properties["code"] = std::string("SP1");
        entities.push_back(near);
        Entity far = near;
        far.geometry = PointGeometry{Point2(5000.0 + i, 5000.0)};
        far.layer = "FAR";
        entities.push_back(far);
    }
    ASSERT_TRUE(document.execute(cmd::createEntities(entities)).ok());
    katana::entity::SurveyMap map;
    katana::entity::SurveyRule rule;
    rule.key = "SP*";
    rule.comment = "Stormwater pit";
    ASSERT_TRUE(map.add(rule).ok());
    document.setSurveyMap(map);
    ASSERT_TRUE(addSheets(document, oneSheet({plan("vp1", 60.0, 60.0, 500.0, Point2(10.0, 10.0)),
                                              legendPanel("vp2")})
                                        .sheets)
                    .ok());
    const std::string legendId = document.sheetSet().sheets[0].viewports[1].id;

    const auto legend = legendFor(document, legendId);
    ASSERT_TRUE(legend.ok()) << legend.error().describe();
    ASSERT_EQ(legend->entries.size(), 1u);
    EXPECT_EQ(legend->entries[0].label, "Stormwater pit");
    EXPECT_EQ(legend->entries[0].count, 400u);
    // The index narrows the search and changes nothing.
    LegendOptions scan;
    scan.codes = &document.surveyMap();
    const Legend scanned =
        legendOf(document.model(), document.sheetSet(), 0, LegendScope::ThisSheet, scan);
    EXPECT_EQ(*legend, scanned);
    // And the same question gives the same answer.
    EXPECT_EQ(*legendFor(document, legendId), *legend);

    const auto missing = legendFor(document, "vp999");
    ASSERT_FALSE(missing.ok());
    EXPECT_EQ(missing.error().code, ErrorCode::NotFound);
    const std::string planId = document.sheetSet().sheets[0].viewports[0].id;
    const auto notLegend = legendFor(document, planId);
    ASSERT_FALSE(notLegend.ok());
    EXPECT_EQ(notLegend.error().code, ErrorCode::InvalidArgument);
}

TEST(SheetLegend, SettingTheScopeIsOneUndoableStep)
{
    Document document;
    ASSERT_TRUE(addSheets(document, oneSheet({plan("vp1", 60.0, 60.0, 500.0, Point2(0.0, 0.0)),
                                              legendPanel("vp2")})
                                        .sheets)
                    .ok());
    const std::string planId = document.sheetSet().sheets[0].viewports[0].id;
    const std::string legendId = document.sheetSet().sheets[0].viewports[1].id;
    const auto scopeNow = [&] { return document.sheetSet().sheets[0].viewports[1].legendScope; };

    ASSERT_TRUE(setLegendScope(document, legendId, LegendScope::WholeSet).ok());
    EXPECT_EQ(scopeNow(), LegendScope::WholeSet);
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(scopeNow(), LegendScope::ThisSheet);
    ASSERT_TRUE(document.redo().ok());
    EXPECT_EQ(scopeNow(), LegendScope::WholeSet);

    const auto notLegend = setLegendScope(document, planId, LegendScope::WholeSet);
    ASSERT_FALSE(notLegend.ok());
    EXPECT_EQ(notLegend.error().code, ErrorCode::InvalidArgument);
    const auto missing = setLegendScope(document, "vp404", LegendScope::WholeSet);
    ASSERT_FALSE(missing.ok());
    EXPECT_EQ(missing.error().code, ErrorCode::NotFound);
    EXPECT_EQ(scopeNow(), LegendScope::WholeSet);
}

// ---- stored with the viewport ----------------------------------------------------------------

TEST(SheetLegend, TheScopeIsStoredOnlyWhenNotTheDefaultAndReadBack)
{
    for (const LegendScope scope :
         {LegendScope::ThisSheet, LegendScope::WholeSet, LegendScope::WholeDrawing}) {
        EXPECT_EQ(legendScopeFrom(toString(scope)), scope);
    }
    EXPECT_FALSE(legendScopeFrom("everything").has_value());

    SheetSet set = oneSheet({legendPanel("vp1")});
    auto text = sheetSetToJson(set);
    ASSERT_TRUE(text.ok());
    EXPECT_EQ(text->find("legend_scope"), std::string::npos);

    for (const LegendScope scope : {LegendScope::WholeSet, LegendScope::WholeDrawing}) {
        set.sheets[0].viewports[0].legendScope = scope;
        text = sheetSetToJson(set);
        ASSERT_TRUE(text.ok());
        EXPECT_NE(text->find("\"legend_scope\":\"" + std::string(toString(scope)) + "\""),
                  std::string::npos);
        const auto back = sheetSetFromJson(*text);
        ASSERT_TRUE(back.ok()) << back.error().describe();
        EXPECT_EQ(*back, set);
    }
}

TEST(SheetLegend, AnUnknownScopeIsRefusedWhenRead)
{
    const auto set = sheetSetFromJson(
        R"({"format": "katana-sheets", "version": 1, "sheets": [{"id": "s1", "viewports": )"
        R"([{"id": "vp1", "kind": "legend", "legend_scope": "somewhere"}]}]})");
    ASSERT_FALSE(set.ok());
    EXPECT_EQ(set.error().code, ErrorCode::ParseFailure);
}

TEST(SheetLegend, TheLegendAsJsonNamesEveryMemberOfEveryEntry)
{
    Legend legend;
    legend.scope = LegendScope::WholeSet;
    legend.windows = 2;
    legend.scale = 500.0;
    LegendEntry entry;
    entry.kind = LegendKind::Symbol;
    entry.label = "Stormwater pit";
    entry.style = "Pit";
    entry.code = "SP1";
    entry.colour = kRed;
    entry.symbol = "square";
    entry.symbolSize = 1.5;
    entry.count = 4;
    legend.entries.push_back(entry);
    EXPECT_EQ(legendJson(legend),
              R"({"entries":[{"code":"SP1","colour":"#DC0000","count":4,"hatch":"none",)"
              R"("kind":"symbol","label":"Stormwater pit","layer":"","linetype":"continuous",)"
              R"("style":"Pit","symbol":"square","symbol_size":1.5,"weight":0.25}],)"
              R"("scale":500.0,"scope":"whole_set","windows":2})");
}

// ---- the panel ---------------------------------------------------------------------------------

TEST(SheetLegend, EntriesFlowDownColumnsAsWideAsTheWidestLabel)
{
    // 100 x 100 mm: rows from 8.25 mm under the top to 2.5 mm over the
    // bottom, 89.25 mm, hold 19 rows of 4.5 mm.
    const Box2 rect(Point2(300.0, 40.0), Point2(400.0, 140.0));
    const std::vector<double> short3(3, 20.0);
    LegendLayout layout = layoutLegend(rect, short3);
    ASSERT_EQ(layout.cells.size(), 3u);
    EXPECT_EQ(layout.columns, 1u);
    EXPECT_EQ(layout.rows, 3u);
    EXPECT_EQ(layout.columnWidth, 32.0); // 10 of sample, 2 of gap, 20 of label
    EXPECT_NEAR(layout.cells[0].sample.min.x, 302.5, 1e-9);
    EXPECT_NEAR(layout.cells[0].sample.min.y, 127.7, 1e-9);
    EXPECT_NEAR(layout.cells[0].sample.max.x, 312.5, 1e-9);
    EXPECT_NEAR(layout.cells[0].sample.max.y, 131.3, 1e-9);
    EXPECT_EQ(layout.cells[0].label, Point2(314.5, 129.5));
    EXPECT_EQ(layout.cells[2].label, Point2(314.5, 120.5));
    EXPECT_EQ(layout.cells[0].labelRoom, 20.0);
    EXPECT_EQ(layout.more, 0u);
    EXPECT_FALSE(layout.moreAt);

    // 25 entries need two columns; the rows are shared out, 13 and 12.
    const std::vector<double> short25(25, 20.0);
    layout = layoutLegend(rect, short25);
    ASSERT_EQ(layout.cells.size(), 25u);
    EXPECT_EQ(layout.columns, 2u);
    EXPECT_EQ(layout.rows, 13u);
    EXPECT_EQ(layout.cells[13].label, Point2(302.5 + 32.0 + 4.0 + 12.0, 129.5));
}

TEST(SheetLegend, WhatDoesNotFitIsCountedInTheLastPlace)
{
    const Box2 rect(Point2(300.0, 40.0), Point2(400.0, 140.0));
    // Columns of 27 mm (10 + 2 + 15) and gaps of 4: three fit in 95 mm, so
    // 3 x 19 = 57 places, the last saying "+24 more".
    const std::vector<double> many(80, 15.0);
    LegendLayout layout = layoutLegend(rect, many);
    EXPECT_EQ(layout.columns, 3u);
    EXPECT_EQ(layout.rows, 19u);
    ASSERT_EQ(layout.cells.size(), 56u);
    EXPECT_EQ(layout.more, 24u);
    ASSERT_TRUE(layout.moreAt);
    EXPECT_EQ(*layout.moreAt, Point2(302.5 + 2.0 * 31.0, 48.5));
    // With 32 mm columns only two fit: 38 places.
    layout = layoutLegend(rect, std::vector<double>(80, 20.0));
    EXPECT_EQ(layout.columns, 2u);
    EXPECT_EQ(layout.cells.size(), 37u);
    EXPECT_EQ(layout.more, 43u);

    // A label wider than the panel gets one column and is squeezed to it.
    const std::vector<double> wide(2, 500.0);
    layout = layoutLegend(rect, wide);
    EXPECT_EQ(layout.columns, 1u);
    EXPECT_EQ(layout.columnWidth, 95.0);
    EXPECT_EQ(layout.cells[0].labelRoom, 83.0);

    // Too low for a row: nothing placed, everything counted.
    layout = layoutLegend(Box2(Point2(0.0, 0.0), Point2(100.0, 12.0)), many);
    EXPECT_TRUE(layout.cells.empty());
    EXPECT_EQ(layout.more, 80u);
    EXPECT_FALSE(layout.moreAt);
}
