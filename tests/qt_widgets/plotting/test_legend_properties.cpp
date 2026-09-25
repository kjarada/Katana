// The Legend viewport's properties in the sheet editor
// (src/katana_qt/plotting/legend_properties): the scope choice is ONE
// undoable step, and the line under it says what the legend lists and where a
// fall-back led.

#include <gtest/gtest.h>

#include <string>
#include <vector>

#include <QComboBox>
#include <QLabel>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/legend.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/commands/entity_commands.hpp"
#include "plotting/legend_properties.hpp"
#include "sheet_editor.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::entity::Entity;
using katana::entity::Layer;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::Segment2;
using katana::qt::SheetEditor;
using katana::qt::SheetSource;
namespace plotting = katana::cad::plotting;
namespace cmd = katana::commands;

namespace {

plotting::Viewport planAt(Box2 rect, Point2 centre)
{
    plotting::Viewport viewport;
    viewport.id = "vp1";
    viewport.kind = plotting::ViewportKind::Plan;
    viewport.rect = rect;
    viewport.scale = 500.0;
    viewport.centre = centre;
    return viewport;
}

// Two sheets: a plan of the west end with a legend beside it, and a plan of
// the east end. A line on WEST, one on EAST, and one nowhere near either.
void twoSheets(Document& document)
{
    for (const char* name : {"WEST", "EAST", "NOWHERE"}) {
        ASSERT_TRUE(document.execute(cmd::createLayer(Layer{name})).ok());
    }
    std::vector<Entity> entities(3);
    entities[0].geometry = Segment2{Point2(-100.0, 0.0), Point2(-90.0, 0.0)};
    entities[0].layer = "WEST";
    entities[1].geometry = Segment2{Point2(90.0, 0.0), Point2(100.0, 0.0)};
    entities[1].layer = "EAST";
    entities[2].geometry = Segment2{Point2(0.0, 900.0), Point2(1.0, 900.0)};
    entities[2].layer = "NOWHERE";
    ASSERT_TRUE(document.execute(cmd::createEntities(entities)).ok());

    plotting::Sheet west;
    west.id = "s1";
    west.name = "WEST";
    plotting::Viewport legend;
    legend.id = "vp2";
    legend.kind = plotting::ViewportKind::Legend;
    legend.rect = Box2(Point2(300.0, 40.0), Point2(400.0, 140.0));
    west.viewports = {planAt(Box2(Point2(30.0, 40.0), Point2(230.0, 240.0)), Point2(-95.0, 0.0)),
                      legend};
    plotting::Sheet east;
    east.id = "s2";
    east.name = "EAST";
    east.viewports = {planAt(Box2(Point2(30.0, 40.0), Point2(230.0, 240.0)), Point2(95.0, 0.0))};
    ASSERT_TRUE(plotting::addSheets(document, {west, east}).ok());
}

std::string legendIdOf(const Document& document)
{
    for (const plotting::Viewport& viewport : document.sheetSet().sheets.at(0).viewports) {
        if (viewport.kind == plotting::ViewportKind::Legend) {
            return viewport.id;
        }
    }
    return {};
}

plotting::LegendScope scopeOf(const Document& document)
{
    for (const plotting::Viewport& viewport : document.sheetSet().sheets.at(0).viewports) {
        if (viewport.kind == plotting::ViewportKind::Legend) {
            return viewport.legendScope;
        }
    }
    return plotting::LegendScope::ThisSheet;
}

struct Shown {
    explicit Shown(Document& document)
        : editor(document, [&document] {
              SheetSource source;
              source.plan = katana::qt::planSourceOf(document);
              source.document = &document;
              source.revision = document.modelRevision();
              return source;
          })
    {
        editor.resize(1400, 900);
        editor.show();
        katana::qt::test::processEvents();
    }
    SheetEditor editor;
};

QString summaryText(const SheetEditor& editor)
{
    const auto* summary = editor.findChild<QLabel*>(QStringLiteral("sheetLegendSummary"));
    return summary != nullptr ? summary->text() : QString();
}

} // namespace

TEST(SheetLegendProperties, TheScopeChoiceIsOneStepAndTheSummarySaysWhatItLists)
{
    Document document;
    twoSheets(document);
    Shown shown(document);
    shown.editor.canvas()->select(legendIdOf(document));

    auto* scope = shown.editor.findChild<QComboBox*>(QStringLiteral("sheetLegendScope"));
    ASSERT_NE(scope, nullptr);
    EXPECT_EQ(scope->count(), 3);
    EXPECT_EQ(scope->currentData().toInt(), static_cast<int>(plotting::LegendScope::ThisSheet));
    EXPECT_EQ(summaryText(shown.editor), QStringLiteral("1 entry from this sheet's 1 plan"));

    const std::size_t before = document.history().undoCount();
    scope->setCurrentIndex(scope->findData(static_cast<int>(plotting::LegendScope::WholeSet)));
    katana::qt::test::processEvents();
    EXPECT_EQ(scopeOf(document), plotting::LegendScope::WholeSet);
    EXPECT_EQ(document.history().undoCount(), before + 1);
    // The properties were built again, from the edited viewport.
    scope = shown.editor.findChild<QComboBox*>(QStringLiteral("sheetLegendScope"));
    ASSERT_NE(scope, nullptr);
    EXPECT_EQ(scope->currentData().toInt(), static_cast<int>(plotting::LegendScope::WholeSet));
    EXPECT_EQ(summaryText(shown.editor), QStringLiteral("2 entries from 2 plans of the set"));

    scope->setCurrentIndex(scope->findData(static_cast<int>(plotting::LegendScope::WholeDrawing)));
    katana::qt::test::processEvents();
    EXPECT_EQ(scopeOf(document), plotting::LegendScope::WholeDrawing);
    EXPECT_EQ(summaryText(shown.editor), QStringLiteral("3 entries from the whole drawing"));

    ASSERT_TRUE(document.undo().ok());
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(scopeOf(document), plotting::LegendScope::ThisSheet);
}

TEST(SheetLegendProperties, TheSummarySaysWhereAFallBackLed)
{
    using plotting::LegendScope;
    plotting::Legend legend;
    legend.entries.resize(3);
    legend.windows = 2;
    legend.scope = LegendScope::WholeSet;
    EXPECT_EQ(katana::qt::legendSummary(legend, LegendScope::ThisSheet),
              QStringLiteral("3 entries from 2 plans of the set - this sheet has no plan"));
    EXPECT_EQ(katana::qt::legendSummary(legend, LegendScope::WholeSet),
              QStringLiteral("3 entries from 2 plans of the set"));
    legend.scope = LegendScope::WholeDrawing;
    legend.windows = 0;
    legend.entries.resize(1);
    EXPECT_EQ(katana::qt::legendSummary(legend, LegendScope::ThisSheet),
              QStringLiteral("1 entry from the whole drawing - no sheet has a plan"));
    EXPECT_EQ(katana::qt::legendSummary(legend, LegendScope::WholeDrawing),
              QStringLiteral("1 entry from the whole drawing"));
}
