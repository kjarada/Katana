// A plan's coordinate grid and the live key plan in the sheet editor
// (src/katana_qt/sheet_editor, src/katana_qt/plotting/grid_properties): the
// grid chosen in a plan's properties, each change ONE undoable step through
// setPlanGrid, and a key plan added from the toolbar storing nothing that
// could go stale. Driven on the offscreen platform by the widgets' object
// names.

#include <gtest/gtest.h>

#include <QComboBox>
#include <QDoubleSpinBox>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/key_plan.hpp"
#include "katana/cad/plotting/plan_grid.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "sheet_editor.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::qt::SheetEditor;
using katana::qt::SheetSource;
namespace plotting = katana::cad::plotting;
using plotting::GridStyle;

namespace {

SheetEditor::SourceProvider sourceOf(const Document& document)
{
    return [&document] {
        SheetSource source;
        source.plan = katana::qt::planSourceOf(document);
        source.revision = document.modelRevision();
        return source;
    };
}

plotting::Viewport viewportOf(std::string id, plotting::ViewportKind kind, Box2 rect,
                              Point2 centre = Point2())
{
    plotting::Viewport viewport;
    viewport.id = std::move(id);
    viewport.kind = kind;
    viewport.rect = rect;
    viewport.scale = 1000.0;
    viewport.centre = centre;
    return viewport;
}

// One sheet with a plan (vp1) and a notes panel (vp2), as one step.
void addPlanSheet(Document& document)
{
    plotting::Sheet sheet;
    sheet.id = "s1";
    sheet.name = "PLAN";
    sheet.viewports.push_back(viewportOf("vp1", plotting::ViewportKind::Plan,
                                         Box2(Point2(30.0, 40.0), Point2(250.0, 280.0))));
    sheet.viewports.push_back(viewportOf("vp2", plotting::ViewportKind::Notes,
                                         Box2(Point2(300.0, 100.0), Point2(400.0, 200.0))));
    ASSERT_TRUE(plotting::addSheet(document, sheet).ok());
}

const plotting::Viewport& planOf(const Document& document)
{
    return document.sheetSet().sheets.at(0).viewports.at(0);
}

struct Shown {
    explicit Shown(Document& document) : editor(document, sourceOf(document))
    {
        editor.resize(1400, 900);
        editor.show();
        katana::qt::test::processEvents();
        editor.canvas()->fitPage();
        katana::qt::test::paint(*editor.canvas());
    }
    [[nodiscard]] QComboBox* style() const
    {
        return editor.findChild<QComboBox*>(QStringLiteral("sheetGridStyle"));
    }
    [[nodiscard]] QDoubleSpinBox* interval() const
    {
        return editor.findChild<QDoubleSpinBox*>(QStringLiteral("sheetGridInterval"));
    }
    SheetEditor editor;
};

} // namespace

TEST(SheetGridEditor, APlansGridIsChosenInItsPropertiesOneStepAtATime)
{
    Document document;
    addPlanSheet(document);
    Shown shown(document);
    shown.editor.canvas()->select("vp1");
    katana::qt::test::processEvents();
    QComboBox* style = shown.style();
    ASSERT_NE(style, nullptr);
    ASSERT_NE(shown.interval(), nullptr);
    // As stored: no grid, so no interval to set yet.
    EXPECT_EQ(style->currentData().toString(), QStringLiteral("none"));
    EXPECT_FALSE(shown.interval()->isEnabled());
    // "Auto" is what 0 reads, and its tooltip what that comes to at 1:1000.
    EXPECT_EQ(shown.interval()->text(), QStringLiteral("Auto"));
    EXPECT_TRUE(shown.interval()->toolTip().contains(QStringLiteral("50 m now")))
        << shown.interval()->toolTip().toStdString();
    const std::size_t before = document.history().undoCount();

    style->setCurrentIndex(style->findData(QStringLiteral("lines")));
    katana::qt::test::processEvents();
    EXPECT_EQ(planOf(document).gridStyle, GridStyle::Lines);
    EXPECT_EQ(planOf(document).gridInterval, 0.0);
    EXPECT_EQ(document.history().undoCount(), before + 1);

    // The panel was built again from the document: the new box is enabled.
    QDoubleSpinBox* interval = shown.interval();
    ASSERT_NE(interval, nullptr);
    EXPECT_TRUE(interval->isEnabled());
    interval->setValue(25.0);
    katana::qt::test::processEvents();
    EXPECT_EQ(planOf(document).gridInterval, 25.0);
    EXPECT_EQ(planOf(document).gridStyle, GridStyle::Lines);
    EXPECT_EQ(document.history().undoCount(), before + 2);
    ASSERT_NE(shown.style(), nullptr);
    EXPECT_EQ(shown.style()->currentData().toString(), QStringLiteral("lines"));
    EXPECT_DOUBLE_EQ(shown.interval()->value(), 25.0);

    // The canvas draws it, with nothing to report.
    katana::qt::test::paint(*shown.editor.canvas());
    EXPECT_TRUE(shown.editor.canvas()->lastStats().problems.empty());

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(planOf(document).gridInterval, 0.0);
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(planOf(document).gridStyle, GridStyle::None);
}

TEST(SheetGridEditor, OnlyAPlanOrAKeyPlanHasAGrid)
{
    Document document;
    addPlanSheet(document);
    Shown shown(document);
    shown.editor.canvas()->select("vp2");
    katana::qt::test::processEvents();
    EXPECT_EQ(shown.style(), nullptr);
    EXPECT_EQ(shown.interval(), nullptr);
    // What the list offers, an agent calls directly - and is told why not.
    const auto refused = plotting::setPlanGrid(document, "vp2", GridStyle::Ticks);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, katana::core::ErrorCode::InvalidArgument);
}

TEST(SheetGridEditor, AKeyPlanAddedFromTheToolbarStoresNoOutlinesAndShowsEverySheet)
{
    Document document;
    // Two sheets of plans, 220 x 240 m apiece at 1:1000, side by side.
    for (int i = 0; i < 2; ++i) {
        plotting::Sheet sheet;
        sheet.name = "TILE";
        sheet.viewports.push_back(viewportOf("vp", plotting::ViewportKind::Plan,
                                             Box2(Point2(30.0, 40.0), Point2(250.0, 280.0)),
                                             Point2(1000.0 + 220.0 * i, 2000.0)));
        ASSERT_TRUE(plotting::addSheet(document, sheet).ok());
    }
    Shown shown(document);
    // A new sheet goes after the current one: the last.
    shown.editor.setCurrentSheet(1);
    ASSERT_TRUE(shown.editor.addBlankSheet().ok());
    ASSERT_EQ(shown.editor.currentSheet(), 2u);
    ASSERT_TRUE(shown.editor.addViewport(plotting::ViewportKind::KeyPlan).ok());
    const plotting::SheetSet& set = document.sheetSet();
    ASSERT_EQ(set.sheets.size(), 3u);
    ASSERT_EQ(set.sheets[2].viewports.size(), 1u);
    const plotting::Viewport& key = set.sheets[2].viewports[0];
    EXPECT_EQ(key.kind, plotting::ViewportKind::KeyPlan);
    EXPECT_TRUE(key.marks.empty());
    EXPECT_TRUE(key.autoScale);
    EXPECT_TRUE(key.autoCentre);

    // Fitted, as drawn, to both sheets' plans.
    const auto at = katana::qt::resolvePlanViewport(key, shown.editor.source(), set, 2);
    const auto outlines = plotting::keyPlanOutlines(set, 2);
    ASSERT_EQ(outlines.size(), 2u);
    EXPECT_EQ(outlines[0].label, "1");
    EXPECT_EQ(outlines[1].label, "2");
    for (const plotting::KeyPlanOutline& outline : outlines) {
        EXPECT_FALSE(outline.current);
        for (const Point2& corner : outline.corners) {
            const Point2 paper = plotting::planWorldToPaper(key, {at.scale, at.centre}, corner);
            EXPECT_TRUE(key.rect.contains(paper));
        }
    }
    katana::qt::test::paint(*shown.editor.canvas());
    EXPECT_TRUE(shown.editor.canvas()->lastStats().problems.empty());
}
