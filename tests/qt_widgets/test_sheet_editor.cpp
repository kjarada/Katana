// The sheet editor (src/katana_qt/sheet_editor): what a click, a drag and the
// toolbar do to the project's sheets, and that each is ONE undoable step.
// Driven with mouse events on the offscreen platform.
//
// With KATANA_SHEET_PNG set to a directory, the editor is also grabbed there.

#include <gtest/gtest.h>

#include <cstdlib>

#include <QApplication>
#include <QMouseEvent>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/layout.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/commands/entity_commands.hpp"
#include "sheet_editor.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::qt::SheetCanvas;
using katana::qt::SheetEditor;
using katana::qt::SheetSource;
namespace plotting = katana::cad::plotting;

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

// A sheet with one viewport of `kind` at `rect`, as one step.
void addSheetWith(Document& document, plotting::ViewportKind kind, Box2 rect)
{
    plotting::Sheet sheet;
    sheet.id = "s1";
    sheet.name = "TEST";
    plotting::Viewport viewport;
    viewport.id = "vp1";
    viewport.kind = kind;
    viewport.rect = rect;
    viewport.scale = 500.0;
    sheet.viewports.push_back(viewport);
    ASSERT_TRUE(plotting::addSheet(document, sheet).ok());
}

const plotting::Viewport& onlyViewport(const Document& document)
{
    return document.sheetSet().sheets.at(0).viewports.at(0);
}

void mouse(QWidget& widget, QEvent::Type type, QPointF at, Qt::MouseButton button = Qt::LeftButton,
           Qt::KeyboardModifiers modifiers = Qt::NoModifier)
{
    const Qt::MouseButtons held = type == QEvent::MouseButtonRelease ? Qt::NoButton : Qt::MouseButtons(button);
    QMouseEvent event(type, at, widget.mapToGlobal(at), type == QEvent::MouseMove ? Qt::NoButton : button,
                      held, modifiers);
    QApplication::sendEvent(&widget, &event);
}

// Presses on `from`, moves in three steps to `to` and lets go.
void drag(QWidget& widget, QPointF from, QPointF to, Qt::KeyboardModifiers modifiers = Qt::NoModifier)
{
    mouse(widget, QEvent::MouseButtonPress, from, Qt::LeftButton, modifiers);
    for (int step = 1; step <= 3; ++step) {
        mouse(widget, QEvent::MouseMove, from + (to - from) * (step / 3.0), Qt::LeftButton, modifiers);
    }
    mouse(widget, QEvent::MouseButtonRelease, to, Qt::LeftButton, modifiers);
}

// An editor shown at a fixed size, its page fitted.
struct Shown {
    explicit Shown(Document& document) : editor(document, sourceOf(document))
    {
        editor.resize(1400, 900);
        editor.show();
        katana::qt::test::processEvents();
        editor.canvas()->fitPage();
        katana::qt::test::paint(*editor.canvas());
    }
    SheetEditor editor;
    [[nodiscard]] SheetCanvas& canvas() const { return *editor.canvas(); }
};

} // namespace

TEST(SheetEditor, AddingAViewIsOneStepAndUndoTakesItAway)
{
    Document document;
    Shown shown(document);
    ASSERT_TRUE(shown.editor.addViewport(plotting::ViewportKind::Plan).ok());
    // A sheet first, since there was none, then the view on it.
    ASSERT_EQ(document.sheetSet().sheets.size(), 1u);
    ASSERT_EQ(document.sheetSet().sheets[0].viewports.size(), 1u);
    const plotting::Viewport& plan = onlyViewport(document);
    EXPECT_TRUE(plan.autoScale);
    EXPECT_TRUE(plan.northArrow);
    EXPECT_EQ(shown.canvas().selected(), plan.id);
    // On a sheet with nothing on it, a view fills the tiling area.
    EXPECT_EQ(plan.rect, plotting::tilingArea(document.sheetSet().sheets[0]));

    ASSERT_TRUE(document.undo().ok());
    EXPECT_TRUE(document.sheetSet().sheets[0].viewports.empty());
    EXPECT_TRUE(shown.canvas().selected().empty());
}

TEST(SheetEditor, DraggingAViewMovesItInOneStep)
{
    Document document;
    addSheetWith(document, plotting::ViewportKind::Notes,
                 Box2(Point2(100.0, 100.0), Point2(200.0, 180.0)));
    Shown shown(document);
    SheetCanvas& canvas = shown.canvas();
    const std::size_t before = document.history().undoCount();

    drag(canvas, canvas.paperToWidget(Point2(150.0, 140.0)), canvas.paperToWidget(Point2(180.0, 150.0)));
    EXPECT_EQ(document.history().undoCount(), before + 1);
    const Box2 moved = onlyViewport(document).rect;
    EXPECT_NEAR(moved.min.x, 130.0, 1e-6);
    EXPECT_NEAR(moved.min.y, 110.0, 1e-6);
    EXPECT_NEAR(moved.width(), 100.0, 1e-9);

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(onlyViewport(document).rect, Box2(Point2(100.0, 100.0), Point2(200.0, 180.0)));
}

TEST(SheetEditor, ADraggedViewSnapsToTheDrawingAreaUnlessAltIsHeld)
{
    Document document;
    addSheetWith(document, plotting::ViewportKind::Notes,
                 Box2(Point2(100.0, 100.0), Point2(200.0, 180.0)));
    Shown shown(document);
    SheetCanvas& canvas = shown.canvas();
    const double drawingLeft = plotting::drawingArea(document.sheetSet().sheets[0]).min.x;
    // Within a snap's reach (8 px) of the drawing area's left edge.
    const double near = drawingLeft + 3.0 / canvas.zoom();

    drag(canvas, canvas.paperToWidget(Point2(150.0, 140.0)),
         canvas.paperToWidget(Point2(150.0 - (100.0 - near), 140.0)));
    EXPECT_DOUBLE_EQ(onlyViewport(document).rect.min.x, drawingLeft);

    ASSERT_TRUE(document.undo().ok());
    drag(canvas, canvas.paperToWidget(Point2(150.0, 140.0)),
         canvas.paperToWidget(Point2(150.0 - (100.0 - near), 140.0)), Qt::AltModifier);
    EXPECT_NEAR(onlyViewport(document).rect.min.x, near, 1e-6);
}

TEST(SheetEditor, AHandleResizesOneEdge)
{
    Document document;
    addSheetWith(document, plotting::ViewportKind::Notes,
                 Box2(Point2(100.0, 100.0), Point2(200.0, 180.0)));
    Shown shown(document);
    SheetCanvas& canvas = shown.canvas();
    canvas.select("vp1");
    // The right edge's handle, dragged 40 mm right.
    drag(canvas, canvas.paperToWidget(Point2(200.0, 140.0)), canvas.paperToWidget(Point2(240.0, 140.0)),
         Qt::AltModifier);
    const Box2 r = onlyViewport(document).rect;
    EXPECT_NEAR(r.max.x, 240.0, 1e-6);
    EXPECT_DOUBLE_EQ(r.min.x, 100.0);
    EXPECT_DOUBLE_EQ(r.min.y, 100.0);
    EXPECT_DOUBLE_EQ(r.max.y, 180.0);
}

TEST(SheetEditor, ShiftDragPansTheDrawingInsideAPlan)
{
    Document document;
    addSheetWith(document, plotting::ViewportKind::Plan,
                 Box2(Point2(100.0, 100.0), Point2(300.0, 250.0)));
    Shown shown(document);
    SheetCanvas& canvas = shown.canvas();
    const std::size_t before = document.history().undoCount();
    // 20 mm right on the paper at 1 : 500 is 10 m; the drawing follows the
    // hand, so the centre moves 10 m west.
    drag(canvas, canvas.paperToWidget(Point2(200.0, 175.0)), canvas.paperToWidget(Point2(220.0, 175.0)),
         Qt::ShiftModifier);
    EXPECT_EQ(document.history().undoCount(), before + 1);
    EXPECT_NEAR(onlyViewport(document).centre.x, -10.0, 1e-6);
    EXPECT_NEAR(onlyViewport(document).centre.y, 0.0, 1e-6);
    EXPECT_EQ(onlyViewport(document).rect, Box2(Point2(100.0, 100.0), Point2(300.0, 250.0)));
}

TEST(SheetEditor, TilingPutsTheMainViewInTheLargestCell)
{
    Document document;
    Shown shown(document);
    ASSERT_TRUE(shown.editor.addViewport(plotting::ViewportKind::Legend).ok());
    ASSERT_TRUE(shown.editor.addViewport(plotting::ViewportKind::Plan).ok());
    const std::size_t before = document.history().undoCount();
    ASSERT_TRUE(shown.editor.tile(plotting::TilingPreset::MainRight).ok());
    EXPECT_EQ(document.history().undoCount(), before + 1);
    const auto& views = document.sheetSet().sheets[0].viewports;
    const auto& legend = views[0].kind == plotting::ViewportKind::Legend ? views[0] : views[1];
    const auto& plan = views[0].kind == plotting::ViewportKind::Plan ? views[0] : views[1];
    EXPECT_GT(plan.rect.width(), legend.rect.width());
    const Box2 area = plotting::tilingArea(document.sheetSet().sheets[0]);
    EXPECT_TRUE(area.contains(plan.rect));
    EXPECT_TRUE(area.contains(legend.rect));
}

TEST(SheetEditor, DeleteRemovesTheSelectedView)
{
    Document document;
    addSheetWith(document, plotting::ViewportKind::Notes,
                 Box2(Point2(100.0, 100.0), Point2(200.0, 180.0)));
    Shown shown(document);
    shown.canvas().select("vp1");
    ASSERT_TRUE(shown.editor.removeSelectedViewport().ok());
    EXPECT_TRUE(document.sheetSet().sheets[0].viewports.empty());
    EXPECT_TRUE(shown.canvas().selected().empty());
}

TEST(SheetEditor, TheCanvasPaintsOnceAndNotAgainForASelection)
{
    Document document;
    addSheetWith(document, plotting::ViewportKind::Notes,
                 Box2(Point2(100.0, 100.0), Point2(200.0, 180.0)));
    Shown shown(document);
    SheetCanvas& canvas = shown.canvas();
    katana::qt::test::paint(canvas);
    const int renders = canvas.renders();
    // Picking a viewport draws its handles over the last paint.
    canvas.select("vp1");
    katana::qt::test::paint(canvas);
    EXPECT_EQ(canvas.renders(), renders);
    // A change to the sheet paints it afresh.
    ASSERT_TRUE(plotting::editViewport(document, "vp1", [](plotting::Viewport& v) {
                    v.text = "CHANGED";
                    return katana::core::Status{};
                }).ok());
    katana::qt::test::paint(canvas);
    EXPECT_EQ(canvas.renders(), renders + 1);

    if (const char* dir = std::getenv("KATANA_SHEET_PNG"); dir != nullptr) {
        (void)shown.editor.grab().save(QString("%1/SheetEditor.png").arg(dir));
    }
}
