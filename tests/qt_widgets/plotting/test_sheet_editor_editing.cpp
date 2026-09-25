// The sheet editor's editing of several viewports at once and its furniture
// (src/katana_qt/plotting/sheet_editor_editing.cpp and the canvas in
// src/katana_qt/sheet_editor.cpp; docs/plotting.md, "Editing on the canvas"):
// Ctrl- and Shift-clicks, the rubber band, a group dragged, nudged and deleted
// as ONE undoable step each, copy and paste onto another sheet with new ids,
// duplicate, the paper grid, the rulers, the cursor's paper and world
// position, a dragged view's own paint going with it, Tab, double-click and
// Zoom to Selection, the sheet list's thumbnails, its dragging and
// PgUp / PgDn. Driven with mouse and key events on the offscreen platform.
//
// With KATANA_SHEET_PNG set to a directory, the canvas mid-drag is also
// grabbed there.

#include <gtest/gtest.h>

#include <cmath>
#include <cstdlib>
#include <numbers>
#include <stdexcept>
#include <string>
#include <vector>

#include <QAction>
#include <QApplication>
#include <QClipboard>
#include <QKeyEvent>
#include <QLabel>
#include <QListWidget>
#include <QMenu>
#include <QMimeData>
#include <QMouseEvent>
#include <QToolBar>
#include <QWidgetAction>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/layout.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/cad/plotting/viewport_edits.hpp"
#include "katana/commands/entity_commands.hpp"
#include "plotting/sheet_list_widget.hpp"
#include "plotting/sheet_readout.hpp"
#include "plotting/sheet_rulers.hpp"
#include "plotting/sheet_thumbnails.hpp"
#include "plotting/viewport_clipboard.hpp"
#include "sheet_editor.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::qt::SheetCanvas;
using katana::qt::SheetEditor;
using katana::qt::SheetSource;
namespace plotting = katana::cad::plotting;
using Ids = std::vector<std::string>;

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

Box2 box(double x0, double y0, double x1, double y1)
{
    return Box2(Point2(x0, y0), Point2(x1, y1));
}

plotting::Viewport panel(std::string id, Box2 rect,
                         plotting::ViewportKind kind = plotting::ViewportKind::Notes)
{
    plotting::Viewport viewport;
    viewport.id = std::move(id);
    viewport.kind = kind;
    viewport.rect = rect;
    viewport.text = "PANEL " + viewport.id;
    return viewport;
}

// Two sheets: FIRST with three notes panels in a row, the third locked;
// SECOND with one small panel.
plotting::SheetSet twoSheets()
{
    plotting::SheetSet set;
    plotting::Sheet first;
    first.id = "s1";
    first.name = "FIRST";
    first.viewports = {panel("vp1", box(100.0, 100.0, 200.0, 180.0)),
                       panel("vp2", box(220.0, 100.0, 300.0, 180.0)),
                       panel("vp3", box(320.0, 100.0, 380.0, 180.0))};
    first.viewports[2].locked = true;
    plotting::Sheet second;
    second.id = "s2";
    second.name = "SECOND";
    second.viewports = {panel("vp4", box(50.0, 50.0, 90.0, 90.0))};
    set.sheets = {first, second};
    return set;
}

const plotting::Viewport& byId(const Document& document, std::size_t sheet, std::string_view id)
{
    for (const plotting::Viewport& viewport : document.sheetSet().sheets.at(sheet).viewports) {
        if (viewport.id == id) {
            return viewport;
        }
    }
    throw std::runtime_error("no viewport " + std::string(id));
}

Ids idsOf(const plotting::Sheet& sheet)
{
    Ids ids;
    for (const plotting::Viewport& viewport : sheet.viewports) {
        ids.push_back(viewport.id);
    }
    return ids;
}

void mouse(QWidget& widget, QEvent::Type type, QPointF at, Qt::MouseButton button = Qt::LeftButton,
           Qt::KeyboardModifiers modifiers = Qt::NoModifier)
{
    const Qt::MouseButtons held = type == QEvent::MouseButtonRelease ? Qt::NoButton : Qt::MouseButtons(button);
    QMouseEvent event(type, at, widget.mapToGlobal(at), type == QEvent::MouseMove ? Qt::NoButton : button,
                      held, modifiers);
    QApplication::sendEvent(&widget, &event);
}

void click(QWidget& widget, QPointF at, Qt::KeyboardModifiers modifiers = Qt::NoModifier)
{
    mouse(widget, QEvent::MouseButtonPress, at, Qt::LeftButton, modifiers);
    mouse(widget, QEvent::MouseButtonRelease, at, Qt::LeftButton, modifiers);
}

// Presses on `from` with `button`, moves in three steps to `to` and, unless
// told not to, lets go.
void drag(QWidget& widget, QPointF from, QPointF to, Qt::KeyboardModifiers modifiers = Qt::NoModifier,
          Qt::MouseButton button = Qt::LeftButton, bool release = true)
{
    mouse(widget, QEvent::MouseButtonPress, from, button, modifiers);
    for (int step = 1; step <= 3; ++step) {
        mouse(widget, QEvent::MouseMove, from + (to - from) * (step / 3.0), button, modifiers);
    }
    if (release) {
        mouse(widget, QEvent::MouseButtonRelease, to, button, modifiers);
    }
}

void key(QWidget& widget, int code, Qt::KeyboardModifiers modifiers = Qt::NoModifier,
         QEvent::Type type = QEvent::KeyPress)
{
    QKeyEvent event(type, code, modifiers);
    QApplication::sendEvent(&widget, &event);
}

// An editor shown at a fixed size on `set`, its page fitted.
struct Shown {
    explicit Shown(Document& doc, const plotting::SheetSet& set = twoSheets())
        : document(doc), editor((setUp(doc, set), doc), sourceOf(doc))
    {
        editor.resize(1400, 900);
        editor.show();
        katana::qt::test::processEvents();
        editor.canvas()->fitPage();
        katana::qt::test::paint(*editor.canvas());
    }
    static void setUp(Document& doc, const plotting::SheetSet& set)
    {
        if (!set.sheets.empty()) {
            ASSERT_TRUE(doc.setSheetSet(set).ok());
        }
    }
    [[nodiscard]] SheetCanvas& canvas() const { return *editor.canvas(); }
    [[nodiscard]] QAction& action(const char* name) const
    {
        auto* found = editor.findChild<QAction*>(QString::fromLatin1(name));
        if (found == nullptr) {
            throw std::runtime_error(std::string("no action ") + name);
        }
        return *found;
    }
    // The widget point of paper point (x, y) on the current sheet.
    [[nodiscard]] QPointF at(double x, double y) const { return canvas().paperToWidget(Point2(x, y)); }

    Document& document;
    SheetEditor editor;
};

std::size_t steps(const Document& document) { return document.history().undoCount(); }

void expectNear(QPointF actual, QPointF expected)
{
    EXPECT_NEAR(actual.x(), expected.x(), 1e-9);
    EXPECT_NEAR(actual.y(), expected.y(), 1e-9);
}

bool onGrid(double value) { return std::abs(value / 5.0 - std::round(value / 5.0)) < 1e-9; }

// Pixels of `image` inside widget rectangle `rect` whose every channel is
// below `darkest`: ink.
int darkIn(const QImage& image, QRectF rect, int darkest = 110)
{
    int count = 0;
    const QRect r = rect.toAlignedRect().intersected(image.rect());
    for (int y = r.top(); y <= r.bottom(); ++y) {
        for (int x = r.left(); x <= r.right(); ++x) {
            const QColor c = image.pixelColor(x, y);
            count += c.red() < darkest && c.green() < darkest && c.blue() < darkest ? 1 : 0;
        }
    }
    return count;
}

// The widget rectangle of paper `paper`, shrunk by `inset` pixels all round so
// that an outline drawn on its edge is left out.
QRectF inside(const SheetCanvas& canvas, Box2 paper, double inset = 5.0)
{
    return QRectF(canvas.paperToWidget(Point2(paper.min.x, paper.max.y)),
                  canvas.paperToWidget(Point2(paper.max.x, paper.min.y)))
        .adjusted(inset, inset, -inset, -inset);
}

} // namespace

// ---- picking ------------------------------------------------------------------------

TEST(SheetEditorEditing, CtrlAndShiftClicksAddAndRemoveViewsFromTheSelection)
{
    Document document;
    Shown shown(document);
    SheetCanvas& canvas = shown.canvas();
    const std::size_t before = steps(document);

    click(canvas, shown.at(150.0, 140.0));
    EXPECT_EQ(canvas.selectedIds(), (Ids{"vp1"}));
    click(canvas, shown.at(260.0, 140.0), Qt::ControlModifier);
    EXPECT_EQ(canvas.selectedIds(), (Ids{"vp1", "vp2"}));
    // The last one picked is the primary: its handles, its properties.
    EXPECT_EQ(canvas.selected(), "vp2");
    click(canvas, shown.at(350.0, 140.0), Qt::ShiftModifier);
    EXPECT_EQ(canvas.selectedIds(), (Ids{"vp1", "vp2", "vp3"}));
    // Again: out of the selection, and the primary passes on.
    click(canvas, shown.at(350.0, 140.0), Qt::ControlModifier);
    EXPECT_EQ(canvas.selectedIds(), (Ids{"vp1", "vp2"}));
    EXPECT_FALSE(canvas.selected().empty());
    EXPECT_NE(canvas.selected(), "vp3");
    // A plain click on one of a group selects it alone.
    click(canvas, shown.at(150.0, 140.0));
    EXPECT_EQ(canvas.selectedIds(), (Ids{"vp1"}));
    EXPECT_EQ(canvas.selected(), "vp1");
    // Picking changes nothing in the project.
    EXPECT_EQ(steps(document), before);
}

TEST(SheetEditorEditing, TheSelectionListsOnlyWhatIsOnTheSheetInItsOrder)
{
    Document document;
    Shown shown(document);
    SheetCanvas& canvas = shown.canvas();
    // Ids not on the sheet are dropped; the order is the sheet's, back to front.
    canvas.setSelection({"vp3", "nothing", "vp1", "vp4"});
    EXPECT_EQ(canvas.selectedIds(), (Ids{"vp1", "vp3"}));
    EXPECT_EQ(canvas.selected(), "vp1");
    canvas.setSelection({"vp1", "vp3"}, "vp3");
    EXPECT_EQ(canvas.selected(), "vp3");
    EXPECT_TRUE(canvas.isSelected("vp1"));
    EXPECT_FALSE(canvas.isSelected("vp2"));
    shown.action("sheetSelectAll").trigger();
    EXPECT_EQ(canvas.selectedIds(), (Ids{"vp1", "vp2", "vp3"}));
    // Another sheet starts with nothing selected.
    shown.editor.setCurrentSheet(1);
    EXPECT_TRUE(canvas.selectedIds().empty());
    EXPECT_TRUE(canvas.selected().empty());
}

TEST(SheetEditorEditing, ARubberBandOnEmptyPaperSelectsWhatItEnclosesOrCrosses)
{
    Document document;
    Shown shown(document);
    SheetCanvas& canvas = shown.canvas();
    const QPointF pageAt = shown.at(0.0, 0.0);
    const std::size_t before = steps(document);

    // Left to right, a window: vp1 wholly inside, vp2 only half.
    drag(canvas, shown.at(90.0, 190.0), shown.at(260.0, 90.0));
    EXPECT_EQ(canvas.selectedIds(), (Ids{"vp1"}));
    // Right to left, a crossing: both it touches.
    drag(canvas, shown.at(260.0, 190.0), shown.at(90.0, 90.0));
    EXPECT_EQ(canvas.selectedIds(), (Ids{"vp1", "vp2"}));
    // With Ctrl, the band adds to the selection.
    drag(canvas, shown.at(310.0, 190.0), shown.at(390.0, 90.0), Qt::ControlModifier);
    EXPECT_EQ(canvas.selectedIds(), (Ids{"vp1", "vp2", "vp3"}));
    // A plain click on empty paper selects nothing.
    click(canvas, shown.at(250.0, 250.0));
    EXPECT_TRUE(canvas.selectedIds().empty());

    // The band replaced the empty-paper pan: the paper has not moved, and
    // nothing was recorded.
    expectNear(shown.at(0.0, 0.0), pageAt);
    EXPECT_EQ(steps(document), before);
}

TEST(SheetEditorEditing, TheMiddleButtonAndSpaceDragStillPanThePaper)
{
    Document document;
    Shown shown(document);
    SheetCanvas& canvas = shown.canvas();
    canvas.select("vp1");
    const QPointF pageAt = shown.at(0.0, 0.0);

    drag(canvas, shown.at(250.0, 250.0), shown.at(250.0, 250.0) + QPointF(30.0, 20.0), Qt::NoModifier,
         Qt::MiddleButton);
    expectNear(shown.at(0.0, 0.0), pageAt + QPointF(30.0, 20.0));

    // Space held: a left drag pans too, even from a viewport, and moves nothing.
    key(canvas, Qt::Key_Space);
    drag(canvas, shown.at(150.0, 140.0), shown.at(150.0, 140.0) + QPointF(-10.0, 5.0));
    key(canvas, Qt::Key_Space, Qt::NoModifier, QEvent::KeyRelease);
    expectNear(shown.at(0.0, 0.0), pageAt + QPointF(20.0, 25.0));
    EXPECT_EQ(byId(document, 0, "vp1").rect, box(100.0, 100.0, 200.0, 180.0));
    EXPECT_EQ(canvas.selectedIds(), (Ids{"vp1"}));
}

TEST(SheetEditorEditing, TabStepsTheSelectionThroughTheViews)
{
    Document document;
    Shown shown(document);
    SheetCanvas& canvas = shown.canvas();
    key(canvas, Qt::Key_Tab);
    EXPECT_EQ(canvas.selectedIds(), (Ids{"vp1"}));
    key(canvas, Qt::Key_Tab);
    EXPECT_EQ(canvas.selected(), "vp2");
    key(canvas, Qt::Key_Tab);
    key(canvas, Qt::Key_Tab); // round to the first again
    EXPECT_EQ(canvas.selected(), "vp1");
    key(canvas, Qt::Key_Backtab, Qt::ShiftModifier);
    EXPECT_EQ(canvas.selectedIds(), (Ids{"vp3"}));
    shown.action("sheetSelectPreviousView").trigger();
    EXPECT_EQ(canvas.selected(), "vp2");
}

// ---- the group's edits: each one step ------------------------------------------------

TEST(SheetEditorEditing, DraggingOneOfSeveralSelectedMovesThemAllInOneStep)
{
    Document document;
    Shown shown(document);
    SheetCanvas& canvas = shown.canvas();
    canvas.setSelection({"vp1", "vp2", "vp3"});
    const std::size_t before = steps(document);

    // By vp2, 30 mm right and 20 mm up, without snapping.
    drag(canvas, shown.at(260.0, 140.0), shown.at(290.0, 160.0), Qt::AltModifier);
    EXPECT_EQ(steps(document), before + 1);
    const Box2 first = byId(document, 0, "vp1").rect;
    const Box2 second = byId(document, 0, "vp2").rect;
    EXPECT_NEAR(first.min.x, 130.0, 1e-6);
    EXPECT_NEAR(first.min.y, 120.0, 1e-6);
    EXPECT_NEAR(second.min.x, 250.0, 1e-6);
    EXPECT_NEAR(second.min.y, 120.0, 1e-6);
    EXPECT_NEAR(first.width(), 100.0, 1e-9);
    // The locked one stays; the group stays selected.
    EXPECT_EQ(byId(document, 0, "vp3").rect, box(320.0, 100.0, 380.0, 180.0));
    EXPECT_EQ(canvas.selectedIds(), (Ids{"vp1", "vp2", "vp3"}));

    // One undo puts the whole group back.
    ASSERT_TRUE(document.undo().ok());
    EXPECT_TRUE(document.sheetSet() == twoSheets());
}

TEST(SheetEditorEditing, EscapeAbandonsADragAndLeavesEverythingWhereItWas)
{
    Document document;
    Shown shown(document);
    SheetCanvas& canvas = shown.canvas();
    const std::size_t before = steps(document);
    drag(canvas, shown.at(150.0, 140.0), shown.at(170.0, 150.0), Qt::NoModifier, Qt::LeftButton,
         /*release=*/false);
    key(canvas, Qt::Key_Escape);
    mouse(canvas, QEvent::MouseButtonRelease, shown.at(170.0, 150.0));
    EXPECT_EQ(steps(document), before);
    EXPECT_EQ(byId(document, 0, "vp1").rect, box(100.0, 100.0, 200.0, 180.0));
}

TEST(SheetEditorEditing, TheArrowsNudgeEverySelectedViewInOneStepEach)
{
    Document document;
    Shown shown(document);
    SheetCanvas& canvas = shown.canvas();
    canvas.setSelection({"vp1", "vp2"});
    const std::size_t before = steps(document);
    key(canvas, Qt::Key_Right);
    EXPECT_EQ(steps(document), before + 1);
    EXPECT_EQ(byId(document, 0, "vp1").rect, box(101.0, 100.0, 201.0, 180.0));
    EXPECT_EQ(byId(document, 0, "vp2").rect, box(221.0, 100.0, 301.0, 180.0));
    key(canvas, Qt::Key_Up, Qt::ShiftModifier);
    EXPECT_EQ(steps(document), before + 2);
    EXPECT_EQ(byId(document, 0, "vp1").rect, box(101.0, 110.0, 201.0, 190.0));
    // The API an agent calls does the same.
    ASSERT_TRUE(shown.editor.nudgeSelection(Point2(-1.0, -10.0)).ok());
    EXPECT_EQ(byId(document, 0, "vp2").rect, box(220.0, 100.0, 300.0, 180.0));
    canvas.select({});
    EXPECT_FALSE(shown.editor.nudgeSelection(Point2(1.0, 0.0)).ok());
}

TEST(SheetEditorEditing, DeleteRemovesEverySelectedViewInOneStep)
{
    Document document;
    Shown shown(document);
    SheetCanvas& canvas = shown.canvas();
    // Nothing selected: Delete has nothing to do and leaves its key alone.
    EXPECT_FALSE(shown.action("sheetDeleteViews").isEnabled());

    canvas.setSelection({"vp1", "vp3"});
    EXPECT_TRUE(shown.action("sheetDeleteViews").isEnabled());
    const std::size_t before = steps(document);
    shown.action("sheetDeleteViews").trigger();
    EXPECT_EQ(steps(document), before + 1);
    EXPECT_EQ(idsOf(document.sheetSet().sheets[0]), (Ids{"vp2"}));
    EXPECT_TRUE(canvas.selectedIds().empty());

    // The key on the canvas does the same, and undo brings them all back.
    ASSERT_TRUE(document.undo().ok());
    canvas.setSelection({"vp1", "vp2"});
    key(canvas, Qt::Key_Delete);
    EXPECT_EQ(idsOf(document.sheetSet().sheets[0]), (Ids{"vp3"}));
    ASSERT_TRUE(document.undo().ok());
    EXPECT_TRUE(document.sheetSet() == twoSheets());
}

// ---- copy, paste, duplicate ---------------------------------------------------------

TEST(SheetEditorEditing, ViewsCopiedOnOneSheetPasteOntoAnotherWithNewIdsInOneStep)
{
    Document document;
    Shown shown(document);
    SheetCanvas& canvas = shown.canvas();
    canvas.setSelection({"vp1", "vp2"});
    shown.action("sheetCopy").trigger();
    EXPECT_TRUE(shown.action("sheetPaste").isEnabled());
    // Copying records nothing.
    const std::size_t before = steps(document);

    shown.action("sheetNextSheet").trigger();
    ASSERT_EQ(shown.editor.currentSheet(), 1u);
    shown.action("sheetPaste").trigger();
    EXPECT_EQ(steps(document), before + 1);
    const plotting::Sheet& second = document.sheetSet().sheets[1];
    // New ids after the highest in the set; where they were on the first
    // sheet, since nothing is there on this one; selected.
    EXPECT_EQ(idsOf(second), (Ids{"vp4", "vp5", "vp6"}));
    EXPECT_EQ(second.viewports[1].rect, box(100.0, 100.0, 200.0, 180.0));
    EXPECT_EQ(second.viewports[2].text, "PANEL vp2");
    EXPECT_EQ(canvas.selectedIds(), (Ids{"vp5", "vp6"}));
    // The first sheet is as it was.
    EXPECT_TRUE(document.sheetSet().sheets[0] == twoSheets().sheets[0]);

    // Pasted again on the same sheet: 5 mm right and down from the first paste.
    ASSERT_TRUE(shown.editor.paste().ok());
    EXPECT_EQ(byId(document, 1, "vp7").rect, box(105.0, 95.0, 205.0, 175.0));

    // Undo takes a paste away, and the selection with it.
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(idsOf(document.sheetSet().sheets[1]), (Ids{"vp4", "vp5", "vp6"}));
    EXPECT_TRUE(canvas.selectedIds().empty());
}

TEST(SheetEditorEditing, CutTakesTheViewsAndPasteBringsThemBackWhereTheyWere)
{
    Document document;
    Shown shown(document);
    SheetCanvas& canvas = shown.canvas();
    canvas.setSelection({"vp2"});
    const std::size_t before = steps(document);
    shown.action("sheetCut").trigger();
    EXPECT_EQ(steps(document), before + 1);
    EXPECT_EQ(idsOf(document.sheetSet().sheets[0]), (Ids{"vp1", "vp3"}));
    ASSERT_TRUE(shown.editor.paste().ok());
    EXPECT_EQ(steps(document), before + 2);
    // Its place is free again, so it lands there, under a new id.
    EXPECT_EQ(byId(document, 0, "vp5").rect, box(220.0, 100.0, 300.0, 180.0));
}

TEST(SheetEditorEditing, DuplicateIsOneStepFiveMillimetresRightAndDown)
{
    Document document;
    Shown shown(document);
    SheetCanvas& canvas = shown.canvas();
    canvas.setSelection({"vp2", "vp1"});
    const std::size_t before = steps(document);
    shown.action("sheetDuplicateViews").trigger();
    EXPECT_EQ(steps(document), before + 1);
    EXPECT_EQ(idsOf(document.sheetSet().sheets[0]), (Ids{"vp1", "vp2", "vp3", "vp5", "vp6"}));
    EXPECT_EQ(byId(document, 0, "vp5").rect, box(105.0, 95.0, 205.0, 175.0));
    EXPECT_EQ(byId(document, 0, "vp6").rect, box(225.0, 95.0, 305.0, 175.0));
    // The copies are selected, for the next drag.
    EXPECT_EQ(canvas.selectedIds(), (Ids{"vp5", "vp6"}));
    canvas.select({});
    EXPECT_FALSE(shown.editor.duplicateSelection().ok());
    EXPECT_FALSE(shown.action("sheetDuplicateViews").isEnabled());
}

TEST(SheetEditorEditing, ViewsTravelOnTheClipboardAsTheSheetsOwnJson)
{
    const plotting::SheetSet set = twoSheets();
    auto bytes = katana::qt::viewportsToClipboardBytes(set.sheets[0].viewports);
    ASSERT_TRUE(bytes.ok());
    EXPECT_TRUE(bytes->contains("katana-sheets"));
    auto back = katana::qt::viewportsFromClipboardBytes(*bytes);
    ASSERT_TRUE(back.ok());
    EXPECT_EQ(*back, set.sheets[0].viewports);

    EXPECT_FALSE(katana::qt::viewportsToClipboardBytes({}).ok());
    EXPECT_FALSE(katana::qt::viewportsFromClipboardBytes("not json").ok());
    // Bytes that are a sheet set but hold no viewports are refused too.
    EXPECT_EQ(katana::qt::viewportsFromClipboardBytes(R"({"format": "katana-sheets", "version": 1})")
                  .error()
                  .code,
              katana::core::ErrorCode::ParseFailure);

    // Something else on the clipboard is nothing to paste.
    QMimeData text;
    text.setText(QStringLiteral("hello"));
    EXPECT_EQ(katana::qt::viewportsOnClipboard(&text).error().code, katana::core::ErrorCode::NotFound);
}

// ---- snapping to the paper grid ----------------------------------------------------

TEST(SheetEditorEditing, WithTheGridOnADraggedViewLandsWithAnEdgeOnIt)
{
    plotting::SheetSet set;
    plotting::Sheet sheet;
    sheet.id = "s1";
    sheet.viewports = {panel("vp1", box(100.0, 100.0, 203.0, 180.0))};
    set.sheets = {sheet};
    Document document;
    Shown shown(document, set);
    SheetCanvas& canvas = shown.canvas();
    EXPECT_FALSE(canvas.snapsToGrid());

    shown.action("sheetSnapGrid").trigger();
    EXPECT_TRUE(canvas.snapsToGrid());
    EXPECT_TRUE(shown.action("sheetSnapGrid").isChecked());
    EXPECT_DOUBLE_EQ(canvas.gridSpacing(), plotting::kPaperGridMm);
    drag(canvas, shown.at(150.0, 140.0), shown.at(157.3, 143.6));
    const Box2 snapped = byId(document, 0, "vp1").rect;
    EXPECT_TRUE(onGrid(snapped.min.x) || onGrid(snapped.max.x)) << snapped.min.x << " " << snapped.max.x;
    EXPECT_TRUE(onGrid(snapped.min.y) || onGrid(snapped.max.y)) << snapped.min.y << " " << snapped.max.y;
    EXPECT_NEAR(snapped.width(), 103.0, 1e-9);
    EXPECT_NEAR(snapped.min.x, 107.3, 2.5);
    EXPECT_NEAR(snapped.min.y, 103.6, 2.5);

    // A handle's edge snaps to the grid too.
    ASSERT_TRUE(document.undo().ok());
    canvas.select("vp1");
    drag(canvas, shown.at(203.0, 140.0), shown.at(231.2, 140.0));
    EXPECT_TRUE(onGrid(byId(document, 0, "vp1").rect.max.x));
    EXPECT_DOUBLE_EQ(byId(document, 0, "vp1").rect.min.x, 100.0);

    // Off again: where the hand put it.
    ASSERT_TRUE(document.undo().ok());
    shown.action("sheetSnapGrid").trigger();
    EXPECT_FALSE(canvas.snapsToGrid());
    drag(canvas, shown.at(150.0, 140.0), shown.at(157.3, 143.6));
    EXPECT_NEAR(byId(document, 0, "vp1").rect.min.x, 107.3, 1e-6);
}

TEST(SheetEditorEditing, TheGridIsDrawnFaintlyOnlyWhileItIsOn)
{
    Document document;
    Shown shown(document);
    SheetCanvas& canvas = shown.canvas();
    // Empty paper inside the drawing area.
    const QRectF empty = inside(canvas, box(240.0, 200.0, 300.0, 260.0), 1.0);
    const QImage off = canvas.grab().toImage();
    canvas.setSnapToGrid(true);
    const QImage on = canvas.grab().toImage();
    int changed = 0;
    int dark = 0;
    const QRect r = empty.toAlignedRect();
    for (int y = r.top(); y <= r.bottom(); ++y) {
        for (int x = r.left(); x <= r.right(); ++x) {
            const QColor a = off.pixelColor(x, y);
            const QColor b = on.pixelColor(x, y);
            changed += a != b ? 1 : 0;
            // Faint: a tint of the paper, where two of the stronger lines
            // cross no darker than light grey.
            dark += b.red() < 150 ? 1 : 0;
        }
    }
    EXPECT_GT(changed, 50);
    EXPECT_EQ(dark, 0);
    // The View menu shows what the canvas holds, however it was set.
    QMenu* view = shown.editor.findChild<QMenu*>(QStringLiteral("sheetViewMenu"));
    ASSERT_NE(view, nullptr);
    emit view->aboutToShow();
    EXPECT_TRUE(shown.action("sheetSnapGrid").isChecked());

    // The whole window as a hand sees it: the grid, a group selected, a
    // crossing band mid-drag, the rulers, the pictures in the list.
    if (const char* dir = std::getenv("KATANA_SHEET_PNG"); dir != nullptr) {
        shown.editor.renderThumbnailsNow();
        canvas.setSelection({"vp1", "vp2"});
        drag(canvas, shown.at(390.0, 60.0), shown.at(300.0, 150.0), Qt::ControlModifier,
             Qt::LeftButton, /*release=*/false);
        (void)shown.editor.grab().save(QString("%1/SheetEditorEditing.png").arg(dir));
        mouse(canvas, QEvent::MouseButtonRelease, shown.at(300.0, 150.0));
    }
}

// ---- the rulers and the cursor -----------------------------------------------------

TEST(SheetEditorEditing, TheRulersStepsSuitTheZoom)
{
    // At 2 px a millimetre, 50 mm is the first step 50 px long; ticks every 5.
    const katana::qt::RulerSteps coarse = katana::qt::rulerSteps(2.0);
    EXPECT_DOUBLE_EQ(coarse.numbered, 50.0);
    EXPECT_DOUBLE_EQ(coarse.tick, 5.0);
    // At 10 px, every 5 mm, ticks every half millimetre.
    const katana::qt::RulerSteps fine = katana::qt::rulerSteps(10.0);
    EXPECT_DOUBLE_EQ(fine.numbered, 5.0);
    EXPECT_DOUBLE_EQ(fine.tick, 0.5);
    // Far out, a metre of paper at a time.
    EXPECT_DOUBLE_EQ(katana::qt::rulerSteps(0.05).numbered, 1000.0);
}

TEST(SheetEditorEditing, TheRulersFrameTheCanvasAndThePageFitsBesideThem)
{
    Document document;
    Shown shown(document);
    SheetCanvas& canvas = shown.canvas();
    ASSERT_TRUE(canvas.rulersShown());
    const double r = katana::qt::kSheetRulerPixels;
    // The page is fitted to what the rulers leave.
    EXPECT_GT(shown.at(0.0, 297.0).x(), r);
    EXPECT_GT(shown.at(0.0, 297.0).y(), r);

    const QImage with = canvas.grab().toImage();
    const QColor corner = with.pixelColor(static_cast<int>(r / 2.0), 2);
    EXPECT_GT(corner.red(), 200); // the rulers' light ground, not the dark desk
    // The paper's span is white on the top ruler, above its ticks and numbers.
    const QColor paperOnRuler = with.pixelColor(static_cast<int>(shown.at(235.0, 0.0).x()), 1);
    EXPECT_EQ(paperOnRuler, QColor(Qt::white));
    // The desk's is not.
    EXPECT_NE(with.pixelColor(canvas.width() - 2, 1), QColor(Qt::white));

    shown.action("sheetShowRulers").trigger();
    EXPECT_FALSE(canvas.rulersShown());
    const QImage without = canvas.grab().toImage();
    EXPECT_LT(without.pixelColor(static_cast<int>(r / 2.0), 2).red(), 120); // the desk

    // The rulers are not paper: a press on one leaves the selection alone.
    canvas.setRulersShown(true);
    canvas.select("vp1");
    click(canvas, QPointF(shown.at(250.0, 0.0).x(), r / 2.0));
    click(canvas, QPointF(r / 2.0, shown.at(0.0, 250.0).y()));
    EXPECT_EQ(canvas.selectedIds(), (Ids{"vp1"}));
}

TEST(SheetEditorEditing, TheStatusLineReadsThePaperAndOverAPlanTheWorld)
{
    Document document;
    // A line due north through (1010, 2005).
    ASSERT_TRUE(document.execute(katana::commands::createLine(Point2(1010.0, 1960.0), Point2(1010.0, 2050.0))).ok());
    plotting::SheetSet set;
    plotting::Sheet sheet;
    sheet.id = "s1";
    plotting::Viewport plan = panel("vp1", box(100.0, 100.0, 300.0, 250.0), plotting::ViewportKind::Plan);
    plan.scale = 500.0;
    plan.centre = Point2(1000.0, 2000.0);
    plan.rotation = std::numbers::pi / 6.0;
    sheet.viewports = {plan, panel("vp2", box(320.0, 100.0, 380.0, 180.0))};
    set.sheets = {sheet};
    Shown shown(document, set);
    SheetCanvas& canvas = shown.canvas();
    canvas.zoomTo(plan.rect);

    // Where the painter drew (1010, 2005) on the paper.
    const Point2 world(1010.0, 2005.0);
    const Point2 paper = plotting::planWorldToPaper(plan, 500.0, plan.centre, world);
    const QPointF widget = canvas.paperToWidget(paper);
    mouse(canvas, QEvent::MouseMove, widget, Qt::NoButton);
    const katana::qt::SheetCursorReadout& readout = canvas.cursorReadout();
    ASSERT_TRUE(readout.onSheet);
    EXPECT_TRUE(readout.onPaper);
    EXPECT_NEAR(readout.paper.x, paper.x, 1e-9);
    EXPECT_NEAR(readout.paper.y, paper.y, 1e-9);
    EXPECT_EQ(readout.viewportId, "vp1");
    EXPECT_EQ(readout.kind, plotting::ViewportKind::Plan);
    EXPECT_EQ(readout.scale, "1:500");
    ASSERT_TRUE(readout.world);
    EXPECT_NEAR(readout.world->x, 1010.0, 1e-6);
    EXPECT_NEAR(readout.world->y, 2005.0, 1e-6);

    // And the line is there on the canvas: the readout names what is drawn.
    const QImage shot = canvas.grab().toImage();
    EXPECT_GT(darkIn(shot, QRectF(widget - QPointF(3.0, 3.0), QSizeF(6.0, 6.0)), 160), 0);
    // 10 m east of it, nothing.
    const QPointF east =
        canvas.paperToWidget(plotting::planWorldToPaper(plan, 500.0, plan.centre, Point2(1020.0, 2005.0)));
    EXPECT_EQ(darkIn(shot, QRectF(east - QPointF(3.0, 3.0), QSizeF(6.0, 6.0)), 160), 0);

    // The status bar says so.
    auto* status = shown.editor.findChild<QLabel*>(QStringLiteral("sheetCursorStatus"));
    ASSERT_NE(status, nullptr);
    EXPECT_TRUE(status->text().contains(QStringLiteral("vp1 Plan 1:500"))) << status->text().toStdString();
    EXPECT_TRUE(status->text().contains(QStringLiteral("E 1010.000  N 2005.000"))) << status->text().toStdString();

    // Over a notes panel: its kind, but no world and no scale.
    const katana::qt::SheetCursorReadout notes = canvas.readoutAt(shown.at(350.0, 140.0));
    EXPECT_EQ(notes.viewportId, "vp2");
    EXPECT_EQ(notes.kind, plotting::ViewportKind::Notes);
    EXPECT_FALSE(notes.world);
    EXPECT_TRUE(notes.scale.empty());
    // Off the paper, it says so.
    EXPECT_FALSE(canvas.readoutAt(shown.at(-20.0, 100.0)).onPaper);
    EXPECT_TRUE(canvas.readoutAt(shown.at(-20.0, 100.0)).viewportId.empty());

    // For a look: the page with the grid, both views selected, a band being
    // drawn and the cursor over the plan.
    if (const char* dir = std::getenv("KATANA_SHEET_PNG"); dir != nullptr) {
        canvas.fitPage();
        canvas.setSnapToGrid(true);
        canvas.setSelection({"vp1", "vp2"});
        drag(canvas, shown.at(330.0, 270.0), shown.at(250.0, 200.0), Qt::ControlModifier, Qt::LeftButton,
             /*release=*/false);
        mouse(canvas, QEvent::MouseMove, canvas.paperToWidget(paper), Qt::NoButton);
        (void)shown.editor.grab().save(QString("%1/SheetEditorRotatedPlan.png").arg(dir));
        key(canvas, Qt::Key_Escape);
    }
}

TEST(SheetEditorEditing, AnAutomaticPlanReadsTheWorldAtTheScaleItIsDrawnAt)
{
    Document document;
    ASSERT_TRUE(document.execute(katana::commands::createLine(Point2(0.0, 0.0), Point2(300.0, 200.0))).ok());
    plotting::SheetSet set;
    plotting::Sheet sheet;
    sheet.id = "s1";
    plotting::Viewport plan = panel("vp1", box(100.0, 100.0, 300.0, 250.0), plotting::ViewportKind::Plan);
    plan.autoScale = true;
    plan.autoCentre = true;
    sheet.viewports = {plan};
    set.sheets = {sheet};
    Shown shown(document, set);
    SheetCanvas& canvas = shown.canvas();
    const katana::qt::ResolvedViewport drawn = katana::qt::resolvePlanViewport(plan, shown.editor.source());
    const katana::qt::SheetCursorReadout middle = canvas.readoutAt(shown.at(200.0, 175.0));
    ASSERT_TRUE(middle.world);
    EXPECT_NEAR(middle.world->x, drawn.centre.x, 1e-6);
    EXPECT_NEAR(middle.world->y, drawn.centre.y, 1e-6);
    EXPECT_EQ(QString::fromStdString(middle.scale),
              QString("1:%1").arg(drawn.scale, 0, 'f', 0));
}

// ---- what a drag shows --------------------------------------------------------------

TEST(SheetEditorEditing, ADraggedViewCarriesItsOwnPaintWithIt)
{
    Document document;
    Shown shown(document);
    SheetCanvas& canvas = shown.canvas();
    canvas.zoomTo(box(90.0, 90.0, 210.0, 280.0));
    const Box2 from = box(100.0, 100.0, 200.0, 180.0);
    // 90 mm up: empty paper above it.
    const Box2 to = box(100.0, 190.0, 200.0, 270.0);
    const QImage before = canvas.grab().toImage();
    const int inkThere = darkIn(before, inside(canvas, to));
    const int inkHere = darkIn(before, inside(canvas, from));
    ASSERT_GT(inkHere, 20); // "NOTES" and its text

    drag(canvas, shown.at(150.0, 140.0), shown.at(150.0, 230.0), Qt::AltModifier, Qt::LeftButton,
         /*release=*/false);
    const QImage during = canvas.grab().toImage();
    if (const char* dir = std::getenv("KATANA_SHEET_PNG"); dir != nullptr) {
        (void)during.save(QString("%1/SheetEditorDragging.png").arg(dir));
    }
    // Its ink goes with it, and where it was is faded.
    EXPECT_GT(darkIn(during, inside(canvas, to)), inkThere + inkHere / 2);
    EXPECT_LT(darkIn(during, inside(canvas, from)), inkHere / 4);
    // Nothing is recorded until it is let go.
    const std::size_t before2 = steps(document);
    mouse(canvas, QEvent::MouseButtonRelease, shown.at(150.0, 230.0), Qt::LeftButton, Qt::AltModifier);
    EXPECT_EQ(steps(document), before2 + 1);
    EXPECT_NEAR(byId(document, 0, "vp1").rect.min.x, 100.0, 1e-6);
    EXPECT_NEAR(byId(document, 0, "vp1").rect.min.y, 190.0, 1e-6);
}

// ---- zooming to views ---------------------------------------------------------------

TEST(SheetEditorEditing, DoubleClickingAViewZoomsToItAndSoDoesZoomToSelection)
{
    Document document;
    Shown shown(document);
    SheetCanvas& canvas = shown.canvas();
    const double fitted = canvas.zoom();
    const double r = katana::qt::kSheetRulerPixels;
    const QPointF middle((canvas.width() + r) / 2.0, (canvas.height() + r) / 2.0);

    mouse(canvas, QEvent::MouseButtonPress, shown.at(350.0, 140.0));
    mouse(canvas, QEvent::MouseButtonRelease, shown.at(350.0, 140.0));
    mouse(canvas, QEvent::MouseButtonDblClick, shown.at(350.0, 140.0));
    mouse(canvas, QEvent::MouseButtonRelease, shown.at(350.0, 140.0));
    EXPECT_GT(canvas.zoom(), fitted * 2.0);
    EXPECT_EQ(canvas.selected(), "vp3");
    const QPointF centre = shown.at(350.0, 140.0);
    EXPECT_NEAR(centre.x(), middle.x(), 1.0);
    EXPECT_NEAR(centre.y(), middle.y(), 1.0);
    // Nothing was moved by the clicks.
    EXPECT_TRUE(document.sheetSet() == twoSheets());

    // Two views: both in the window, their middle in its middle.
    canvas.setSelection({"vp1", "vp2"});
    shown.action("sheetZoomSelection").trigger();
    const QPointF both = shown.at(200.0, 140.0);
    EXPECT_NEAR(both.x(), middle.x(), 1.0);
    EXPECT_NEAR(both.y(), middle.y(), 1.0);
    EXPECT_GE(shown.at(100.0, 180.0).x(), r);
    EXPECT_LE(shown.at(300.0, 100.0).x(), canvas.width());

    // Double-click the desk: the whole page again.
    mouse(canvas, QEvent::MouseButtonDblClick, QPointF(canvas.width() - 3.0, canvas.height() - 3.0));
    EXPECT_NEAR(canvas.zoom(), fitted, 1e-9);
}

// ---- the sheet list -----------------------------------------------------------------

TEST(SheetEditorEditing, EachSheetInTheListHasAPictureOfItPaintedOnce)
{
    plotting::SheetSet set = twoSheets();
    // The second sheet has no frame and one small panel: little ink.
    set.sheets[1].frame.clear();
    Document document;
    Shown shown(document, set);
    auto* list = shown.editor.findChild<QListWidget*>(QStringLiteral("sheetList"));
    ASSERT_NE(list, nullptr);
    shown.editor.renderThumbnailsNow();
    EXPECT_EQ(shown.editor.pendingThumbnails(), 0u);
    const int painted = shown.editor.thumbnails().renders();

    // A picture of each sheet, the paper's shape, beside its name.
    const QImage framed = shown.editor.thumbnails().cached("s1");
    const QImage bare = shown.editor.thumbnails().cached("s2");
    ASSERT_FALSE(framed.isNull());
    ASSERT_FALSE(bare.isNull());
    if (const char* dir = std::getenv("KATANA_SHEET_PNG"); dir != nullptr) {
        (void)framed.save(QString("%1/SheetThumbnailFramed.png").arg(dir));
        (void)bare.save(QString("%1/SheetThumbnailBare.png").arg(dir));
    }
    EXPECT_FALSE(list->item(0)->icon().isNull());
    int framedInk = 0;
    int bareInk = 0;
    int white = 0;
    for (int y = 0; y < framed.height(); ++y) {
        for (int x = 0; x < framed.width(); ++x) {
            // Anything drawn on the paper, however faint at this size.
            framedInk += qGray(framed.pixel(x, y)) < 245 && qAlpha(framed.pixel(x, y)) == 255 ? 1 : 0;
            bareInk += qGray(bare.pixel(x, y)) < 245 && qAlpha(bare.pixel(x, y)) == 255 ? 1 : 0;
            white += framed.pixel(x, y) == qRgb(255, 255, 255) ? 1 : 0;
        }
    }
    EXPECT_GT(white, framed.width() * framed.height() / 3); // paper
    EXPECT_GT(framedInk, bareInk * 2);                      // its frame and title block

    // Nothing changed: nothing to paint.
    shown.editor.renderThumbnailsNow();
    EXPECT_EQ(shown.editor.thumbnails().renders(), painted);
    // An edit of the second sheet: only its picture is painted again.
    ASSERT_TRUE(plotting::editSheet(document, 1, [](plotting::Sheet& sheet) {
                    sheet.name = "RENAMED";
                    return katana::core::Status{};
                }).ok());
    EXPECT_EQ(shown.editor.pendingThumbnails(), 1u);
    shown.editor.renderThumbnailsNow();
    EXPECT_EQ(shown.editor.thumbnails().renders(), painted + 1);
    // A change to the drawing leaves sheets of notes alone.
    ASSERT_TRUE(document.execute(katana::commands::createLine(Point2(0.0, 0.0), Point2(1.0, 1.0))).ok());
    EXPECT_EQ(shown.editor.pendingThumbnails(), 0u);
}

TEST(SheetEditorEditing, APlansPictureIsPaintedAgainWhenTheDrawingChanges)
{
    plotting::SheetSet set;
    plotting::Sheet sheet;
    sheet.id = "s1";
    sheet.viewports = {panel("vp1", box(100.0, 100.0, 300.0, 250.0), plotting::ViewportKind::Plan)};
    set.sheets = {sheet};
    Document document;
    Shown shown(document, set);
    shown.editor.renderThumbnailsNow();
    ASSERT_EQ(shown.editor.pendingThumbnails(), 0u);
    ASSERT_TRUE(document.execute(katana::commands::createLine(Point2(0.0, 0.0), Point2(1.0, 1.0))).ok());
    EXPECT_EQ(shown.editor.pendingThumbnails(), 1u);
}

TEST(SheetEditorEditing, DroppingASheetInTheListMovesItInOneStep)
{
    plotting::SheetSet set = twoSheets();
    plotting::Sheet third;
    third.id = "s3";
    third.name = "THIRD";
    set.sheets.push_back(third);
    Document document;
    Shown shown(document, set);
    auto* list = dynamic_cast<katana::qt::SheetListWidget*>(
        shown.editor.findChild<QListWidget*>(QStringLiteral("sheetList")));
    ASSERT_NE(list, nullptr);
    ASSERT_EQ(list->count(), 3);
    const QRect last = list->visualItemRect(list->item(2));
    // On the lower half of the last row: after it.
    EXPECT_EQ(list->insertionRowAt(QPoint(last.center().x(), last.bottom() - 2)), 3);
    EXPECT_EQ(list->insertionRowAt(QPoint(last.center().x(), last.top() + 2)), 2);

    // FIRST dropped on itself, or just below itself: it stays, and nothing is
    // asked for.
    shown.editor.setCurrentSheet(0);
    const std::size_t before = steps(document);
    const QRect top = list->visualItemRect(list->item(0));
    EXPECT_FALSE(list->dropAt(QPoint(top.center().x(), top.top() + 2)));
    EXPECT_FALSE(list->dropAt(QPoint(top.center().x(), top.bottom() - 2)));
    // FIRST dropped below THIRD. (Qt delivers a drop only to a drag it
    // started, so the drop's own step is taken here; dropEvent does no more
    // than this after Qt's bookkeeping.)
    EXPECT_TRUE(list->dropAt(QPoint(last.center().x(), last.bottom() - 2)));
    // The move waits for the drop to unwind.
    EXPECT_EQ(steps(document), before);
    katana::qt::test::processEvents();
    EXPECT_EQ(steps(document), before + 1);
    const plotting::SheetSet& moved = document.sheetSet();
    ASSERT_EQ(moved.sheets.size(), 3u);
    EXPECT_EQ(moved.sheets[0].id, "s2");
    EXPECT_EQ(moved.sheets[1].id, "s3");
    EXPECT_EQ(moved.sheets[2].id, "s1");
    // The list is the set's order, the moved sheet shown.
    EXPECT_EQ(list->count(), 3);
    EXPECT_TRUE(list->item(2)->text().contains(QStringLiteral("FIRST")));
    EXPECT_EQ(shown.editor.currentSheet(), 2u);
    EXPECT_EQ(list->currentRow(), 2);

    // Undo puts the order back.
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.sheetSet().sheets[0].id, "s1");
    EXPECT_TRUE(list->item(0)->text().contains(QStringLiteral("FIRST")));

    // The API an agent calls; a sheet dropped where it was records nothing.
    ASSERT_TRUE(shown.editor.moveSheetTo(2, 0).ok());
    EXPECT_EQ(document.sheetSet().sheets[0].id, "s3");
    const std::size_t after = steps(document);
    ASSERT_TRUE(shown.editor.moveSheetTo(1, 1).ok());
    EXPECT_EQ(steps(document), after);
    EXPECT_FALSE(shown.editor.moveSheetTo(5, 0).ok());
}

TEST(SheetEditorEditing, PageUpAndPageDownStepThroughTheSheets)
{
    Document document;
    Shown shown(document);
    EXPECT_EQ(shown.editor.currentSheet(), 0u);
    shown.action("sheetNextSheet").trigger();
    EXPECT_EQ(shown.editor.currentSheet(), 1u);
    // Past the last: stays.
    shown.action("sheetNextSheet").trigger();
    EXPECT_EQ(shown.editor.currentSheet(), 1u);
    shown.action("sheetPreviousSheet").trigger();
    EXPECT_EQ(shown.editor.currentSheet(), 0u);
    shown.action("sheetPreviousSheet").trigger();
    EXPECT_EQ(shown.editor.currentSheet(), 0u);
    EXPECT_EQ(shown.action("sheetNextSheet").shortcut(), QKeySequence(Qt::Key_PageDown));
    EXPECT_EQ(shown.action("sheetPreviousSheet").shortcut(), QKeySequence(Qt::Key_PageUp));
}

TEST(SheetEditorEditing, EveryEditingActionHasAStableName)
{
    Document document;
    Shown shown(document);
    for (const char* name :
         {"sheetCut", "sheetCopy", "sheetPaste", "sheetDuplicateViews", "sheetDeleteViews",
          "sheetSelectAll", "sheetSelectNextView", "sheetSelectPreviousView", "sheetZoomSelection",
          "sheetSnapGrid", "sheetShowRulers", "sheetPreviousSheet", "sheetNextSheet", "sheetFitPage"}) {
        EXPECT_NE(shown.editor.findChild<QAction*>(QString::fromLatin1(name)), nullptr) << name;
    }
    // And every action of the window and its toolbar has a name an agent can
    // trigger it by: the separators and the toolbar's buttons aside.
    auto* bar = shown.editor.findChild<QToolBar*>(QStringLiteral("sheetToolBar"));
    ASSERT_NE(bar, nullptr);
    QList<QAction*> actions = shown.editor.actions();
    actions += bar->actions();
    for (QAction* action : actions) {
        if (action->isSeparator() || qobject_cast<QWidgetAction*>(action) != nullptr) {
            continue;
        }
        EXPECT_FALSE(action->objectName().isEmpty()) << action->text().toStdString();
    }
    EXPECT_NE(shown.editor.findChild<QMenu*>(QStringLiteral("sheetEditMenu")), nullptr);
    EXPECT_NE(shown.editor.findChild<QLabel*>(QStringLiteral("sheetCursorStatus")), nullptr);
}


// ---- found in review ------------------------------------------------------------------

TEST(SheetEditorEditing, AClickOnALockedMemberOfAGroupSelectsItAloneAndADragFromItMovesTheRest)
{
    Document document;
    Shown shown(document);
    SheetCanvas& canvas = shown.canvas();
    // vp3 is locked.
    canvas.setSelection({"vp2", "vp3"}, "vp2");
    const std::size_t before = steps(document);
    click(canvas, shown.at(350.0, 140.0));
    EXPECT_EQ(canvas.selectedIds(), (Ids{"vp3"}));
    EXPECT_EQ(steps(document), before);

    // Dragged from the locked one, the group moves without it, in one step.
    canvas.setSelection({"vp2", "vp3"}, "vp2");
    drag(canvas, shown.at(350.0, 140.0), shown.at(350.0, 120.0), Qt::AltModifier);
    EXPECT_EQ(steps(document), before + 1);
    EXPECT_EQ(byId(document, 0, "vp3").rect, box(320.0, 100.0, 380.0, 180.0));
    EXPECT_NEAR(byId(document, 0, "vp2").rect.min.y, 80.0, 1e-6);
    EXPECT_EQ(canvas.selectedIds(), (Ids{"vp2", "vp3"}));

    // Only locked ones selected: a drag from one moves nothing and records
    // nothing.
    ASSERT_TRUE(plotting::editViewport(document, "vp1", [](plotting::Viewport& v) {
                    v.locked = true;
                    return katana::core::Status{};
                }).ok());
    canvas.setSelection({"vp1", "vp3"}, "vp1");
    const std::size_t locked = steps(document);
    drag(canvas, shown.at(150.0, 140.0), shown.at(170.0, 120.0));
    EXPECT_EQ(steps(document), locked);
    EXPECT_EQ(byId(document, 0, "vp1").rect, box(100.0, 100.0, 200.0, 180.0));
    EXPECT_EQ(canvas.selectedIds(), (Ids{"vp1", "vp3"}));
    // And a click on one of them selects it alone.
    click(canvas, shown.at(150.0, 140.0));
    EXPECT_EQ(canvas.selectedIds(), (Ids{"vp1"}));
}

TEST(SheetEditorEditing, EscapeInARubberBandLeavesTheSelectionAsItWas)
{
    Document document;
    Shown shown(document);
    SheetCanvas& canvas = shown.canvas();
    canvas.select("vp1");
    drag(canvas, shown.at(260.0, 190.0), shown.at(90.0, 90.0), Qt::NoModifier, Qt::LeftButton,
         /*release=*/false);
    key(canvas, Qt::Key_Escape);
    mouse(canvas, QEvent::MouseButtonRelease, shown.at(90.0, 90.0));
    EXPECT_EQ(canvas.selectedIds(), (Ids{"vp1"}));
}

TEST(SheetEditorEditing, ADropInTheSpacingBetweenTwoRowsGoesBetweenThem)
{
    plotting::SheetSet set = twoSheets();
    plotting::Sheet third;
    third.id = "s3";
    third.name = "THIRD";
    set.sheets.push_back(third);
    Document document;
    Shown shown(document, set);
    auto* list = dynamic_cast<katana::qt::SheetListWidget*>(
        shown.editor.findChild<QListWidget*>(QStringLiteral("sheetList")));
    ASSERT_NE(list, nullptr);
    const QRect first = list->visualItemRect(list->item(0));
    const QRect second = list->visualItemRect(list->item(1));
    ASSERT_LT(first.bottom() + 1, second.top()); // there is a gap
    const int gap = (first.bottom() + second.top()) / 2;
    EXPECT_EQ(list->insertionRowAt(QPoint(first.center().x(), gap)), 1);
    // FIRST dropped there stays; THIRD dropped there goes second.
    shown.editor.setCurrentSheet(0);
    EXPECT_FALSE(list->dropAt(QPoint(first.center().x(), gap)));
    shown.editor.setCurrentSheet(2);
    EXPECT_TRUE(list->dropAt(QPoint(first.center().x(), gap)));
    katana::qt::test::processEvents();
    EXPECT_EQ(document.sheetSet().sheets[1].id, "s3");
    EXPECT_EQ(document.sheetSet().sheets[2].id, "s2");
}

TEST(SheetEditorEditing, APictureGoesStaleWhenAnotherSheetsNumberItsMatchLinePrintsChanges)
{
    plotting::SheetSet set = twoSheets();
    plotting::WorldMark mark;
    mark.points = {Point2(0.0, 0.0), Point2(10.0, 0.0)};
    mark.label = "MATCH LINE";
    mark.sheet = "s2";
    set.sheets[0].viewports[0].marks.push_back(mark);
    katana::qt::SheetThumbnails thumbnails;
    const SheetSource source;
    (void)thumbnails.thumbnail(set, 0, source);
    (void)thumbnails.thumbnail(set, 1, source);
    EXPECT_FALSE(thumbnails.isStale(set, 0, source));
    // The sheet it leads to gets a number of its own: the label changes.
    set.sheets[1].fields["sheet_number"] = "C-101";
    EXPECT_TRUE(thumbnails.isStale(set, 0, source));
}

TEST(SheetEditorEditing, PicturesGoStaleOnUndoAndOnATitleBlockEdit)
{
    Document document;
    Shown shown(document);
    shown.editor.renderThumbnailsNow();
    ASSERT_EQ(shown.editor.pendingThumbnails(), 0u);
    ASSERT_TRUE(plotting::editSheet(document, 1, [](plotting::Sheet& sheet) {
                    sheet.name = "RENAMED";
                    return katana::core::Status{};
                }).ok());
    shown.editor.renderThumbnailsNow();
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(shown.editor.pendingThumbnails(), 1u);
    shown.editor.renderThumbnailsNow();
    // The title block every sheet shares: every picture.
    plotting::SheetSet set = document.sheetSet();
    set.defaults.organisation = "ACME SURVEYS";
    ASSERT_TRUE(document.setSheetSet(set, "EDIT_TITLE_BLOCK").ok());
    EXPECT_EQ(shown.editor.pendingThumbnails(), 2u);
    // And a sheet removed takes its picture with it.
    set.sheets.pop_back();
    ASSERT_TRUE(document.setSheetSet(set, "REMOVE_SHEET").ok());
    katana::qt::test::processEvents();
    EXPECT_TRUE(shown.editor.thumbnails().cached("s2").isNull());
}

TEST(SheetEditorEditing, PageUpAndPageDownAreLeftToTheBoxesOfTheProperties)
{
    Document document;
    Shown shown(document);
    for (const char* name : {"sheetPreviousSheet", "sheetNextSheet"}) {
        QAction& action = shown.action(name);
        EXPECT_EQ(action.shortcutContext(), Qt::WidgetWithChildrenShortcut) << name;
        const QList<QObject*> on = action.associatedObjects();
        EXPECT_TRUE(on.contains(shown.editor.canvas())) << name;
        EXPECT_TRUE(on.contains(shown.editor.findChild<QListWidget*>(QStringLiteral("sheetList")))) << name;
        // Not the window: there a spin box's PgUp would switch sheets.
        EXPECT_FALSE(on.contains(&shown.editor)) << name;
    }
}

TEST(SheetEditorEditing, TheNoteOfAGroupIsShortEnoughForANarrowPanel)
{
    Document document;
    Shown shown(document);
    shown.canvas().setSelection({"vp1", "vp2"}, "vp2");
    auto* note = shown.editor.findChild<QLabel*>(QStringLiteral("sheetSelectionNote"));
    ASSERT_NE(note, nullptr);
    EXPECT_TRUE(note->wordWrap());
    EXPECT_TRUE(note->text().startsWith(QStringLiteral("2 views selected")));
    EXPECT_TRUE(note->text().contains(QStringLiteral("vp2")));
    for (const QString& line : note->text().split('\n')) {
        EXPECT_LE(note->fontMetrics().horizontalAdvance(line), 260) << line.toStdString();
    }
}
