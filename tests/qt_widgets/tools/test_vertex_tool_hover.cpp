// The Draw > Vertices tools in a real plan view, driven with mouse and key
// events: what the view draws while the cursor hovers before a click (the
// owner's "no visual clue where the vertex is going"), that drawing it never
// repaints the drawing, and that a vertex clicked BEFORE the tool is the one
// the tool works beside ("what about the already selected vertex").
//
// The view is 400 x 300 px at 10 px a unit centred on (10, 5), so model
// (0, 0) is pixel (100, 200); snapping is off, so the cursor is where the
// mouse is. Fonts differ by platform, so ink is compared with a baseline,
// never counted absolutely.

#include <gtest/gtest.h>

#include <cmath>
#include <string>
#include <vector>

#include <QImage>
#include <QKeyEvent>
#include <QMouseEvent>

#include "drawing/feedback_painter.hpp"
#include "katana/cad/drawing/vertex_editing.hpp"
#include "katana/cad/view_set.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "theme.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::cad::ViewKind;
using katana::cad::ViewSet;
using katana::cad::ViewState;
using katana::entity::EntityId;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::qt::ViewportWidget;
using katana::qt::test::paint;

namespace {

struct Fixture {
    Document document;
    ViewSet views;
    ViewState* state = nullptr;
    std::unique_ptr<ViewportWidget> view;
    EntityId polyline = 0;
    QString prompt;

    Fixture()
    {
        EXPECT_TRUE(document
                        .execute(katana::commands::createPolyline(Polyline2{
                            {Point2(0, 0), Point2(10, 0), Point2(20, 0)}, false}))
                        .ok());
        polyline = document.lastCreatedEntities().front();
        state = &views.add(ViewKind::Plan);
        state->planFramed = true;
        state->plan.center = Point2(10.0, 5.0);
        state->plan.scale = 10.0;
        view = std::make_unique<ViewportWidget>(document, *state);
        view->resize(400, 300);
        paint(*view);
        view->state().plan.center = Point2(10.0, 5.0);
        view->state().plan.scale = 10.0;
        view->setSnapEnabled(false);
        view->onPrompt = [this](const QString& text) { prompt = text; };
    }

    [[nodiscard]] QPointF pixel(const Point2& world) const
    {
        const Point2 p = state->plan.worldToScreen(world);
        return QPointF(p.x, p.y);
    }

    void mouse(QEvent::Type type, const Point2& world, Qt::MouseButton button)
    {
        const QPointF at = pixel(world);
        const Qt::MouseButtons held = type == QEvent::MouseButtonRelease ? Qt::NoButton : button;
        QMouseEvent event(type, at, view->mapToGlobal(at), button, held, Qt::NoModifier);
        QCoreApplication::sendEvent(view.get(), &event);
    }
    void hover(const Point2& world) { mouse(QEvent::MouseMove, world, Qt::NoButton); }
    void click(const Point2& world)
    {
        mouse(QEvent::MouseButtonPress, world, Qt::LeftButton);
        mouse(QEvent::MouseButtonRelease, world, Qt::LeftButton);
    }
    void key(int code)
    {
        QKeyEvent press(QEvent::KeyPress, code, Qt::NoModifier);
        QCoreApplication::sendEvent(view.get(), &press);
    }
    void select()
    {
        document.selection().add(polyline);
        document.notifySelectionChanged();
        paint(*view);
    }
    [[nodiscard]] std::vector<Point2> vertices() const
    {
        return katana::cad::readPolyline(*document.model().entities.find(polyline))->positions();
    }
};

// Whether `pixel` is `colour` laid over the view's ground at a coverage of
// 40 % or more, within 24 a channel (as test_feedback_painter.cpp judges ink).
bool inkOf(QColor pixel, QColor colour)
{
    const QColor ground = katana::qt::theme::viewport();
    const double d[3] = {double(colour.red() - ground.red()), double(colour.green() - ground.green()),
                         double(colour.blue() - ground.blue())};
    const double p[3] = {double(pixel.red() - ground.red()), double(pixel.green() - ground.green()),
                         double(pixel.blue() - ground.blue())};
    const double t = (p[0] * d[0] + p[1] * d[1] + p[2] * d[2]) / (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
    if (t < 0.4 || t > 1.1) {
        return false;
    }
    for (int i = 0; i < 3; ++i) {
        if (std::abs(p[i] - t * d[i]) > 24.0) {
            return false;
        }
    }
    return true;
}

int inkNear(const QImage& image, QPointF centre, double radius, QColor colour)
{
    int count = 0;
    for (int y = int(centre.y() - radius); y <= int(centre.y() + radius); ++y) {
        for (int x = int(centre.x() - radius); x <= int(centre.x() + radius); ++x) {
            if (x >= 0 && y >= 0 && x < image.width() && y < image.height() &&
                std::hypot(x - centre.x(), y - centre.y()) <= radius &&
                inkOf(image.pixelColor(x, y), colour)) {
                ++count;
            }
        }
    }
    return count;
}

} // namespace

TEST(VertexToolHover, HoveringInsertVertexDrawsItsMarksAndNeverRepaintsTheDrawing)
{
    Fixture f;
    ASSERT_TRUE(f.view->startTool("draw.vertex.insert").ok());
    f.hover(Point2(15, 0.3));
    paint(*f.view);
    const auto& counts = f.view->lastPreviewCounts();
    EXPECT_EQ(counts.target, 1u) << "the segment that splits";
    EXPECT_EQ(counts.added, 1u) << "the vertex, where it will go";
    EXPECT_EQ(counts.focus, 3u) << "the polyline's vertices, while the grips are hidden";
    EXPECT_FALSE(counts.refused);
    EXPECT_EQ(counts.caption, "new vertex between 1 and 2 · 5.000 from 1");
    // The marks are the view's furniture: moving over the drawing never
    // paints the drawing again.
    const std::size_t drawn = f.view->drawingPaintCount();
    for (const double x : {12.0, 14.0, 16.0}) {
        f.hover(Point2(x, 0.2));
        paint(*f.view);
    }
    EXPECT_EQ(f.view->drawingPaintCount(), drawn);
    EXPECT_EQ(f.view->lastPreviewCounts().added, 1u);
    EXPECT_EQ(f.document.history().undoCount(), 1u) << "hovering changes nothing";
}

TEST(VertexToolHover, TheNewVertexIsInkedWhereItWillGo)
{
    Fixture f;
    ASSERT_TRUE(f.view->startTool("draw.vertex.insert").ok());
    // Out of reach: 5 units, 50 px, from the polyline.
    f.hover(Point2(15, 5));
    const QImage away = f.view->grab().toImage();
    EXPECT_EQ(f.view->lastPreviewCounts().added, 0u);
    f.hover(Point2(15, 0.3));
    const QImage hovered = f.view->grab().toImage();
    ASSERT_EQ(f.view->lastPreviewCounts().added, 1u);
    // On the line at (15, 0), not under the cursor 3 px above it.
    const QPointF where = f.pixel(Point2(15, 0));
    const QColor cyan = katana::qt::drawing::overlay::preview();
    EXPECT_GT(inkNear(hovered, where, 6, cyan), inkNear(away, where, 6, cyan));
    const QColor green = katana::qt::drawing::overlay::target();
    const QPointF onSegment = f.pixel(Point2(12, 0));
    EXPECT_GT(inkNear(hovered, onSegment, 4, green), inkNear(away, onSegment, 4, green))
        << "the segment that splits, in the target green";
}

TEST(VertexToolHover, AVertexClickedBeforeTheToolIsTheOneInsertWorksBeside)
{
    Fixture f;
    f.select();
    // A plain click on the middle vertex's grip: it is hot, and picked up.
    f.click(Point2(10, 0));
    ASSERT_EQ(f.view->gripController().hot().size(), 1u);
    const std::size_t before = f.document.history().undoCount();
    ASSERT_TRUE(f.view->startTool("draw.vertex.insert").ok());
    EXPECT_TRUE(f.prompt.contains("beside vertex 1 of polyline")) << f.prompt.toStdString();
    EXPECT_TRUE(f.prompt.startsWith("Insert Vertex: ")) << "the tool, not its family";
    // Enter: the middle of the segment after it, (10,0)-(20,0).
    f.key(Qt::Key_Return);
    EXPECT_EQ(f.vertices(),
              (std::vector<Point2>{Point2(0, 0), Point2(10, 0), Point2(15, 0), Point2(20, 0)}));
    // The grip went with the tool: a later click puts nothing down.
    f.key(Qt::Key_Escape);
    f.hover(Point2(25, 5));
    f.click(Point2(25, 5));
    EXPECT_EQ(f.document.history().undoCount(), before + 1) << "the insert, and no GRIP_EDIT";
    EXPECT_EQ(f.document.history().undoName(), "VERTEX_INSERT");
}

TEST(VertexToolHover, AnInsertIsAnsweredWithWhatItDidNotARefusalUntilThePointerMoves)
{
    // The restarted tool's preview where the vertex now is would refuse a
    // second click there, and the view drew that in red at once.
    Fixture f;
    ASSERT_TRUE(f.view->startTool("draw.vertex.insert").ok());
    f.hover(Point2(15, 0.3));
    f.click(Point2(15, 0.3));
    paint(*f.view);
    ASSERT_EQ(f.vertices().size(), 4u);
    EXPECT_FALSE(f.view->lastPreviewCounts().refused);
    EXPECT_EQ(f.view->lastPreviewCounts().caption,
              "vertex 2 added to polyline " + std::to_string(f.polyline) +
                  " on segment 1 (4 vertices)");
    // Away (2 units, 20 px) and back: now a click there would be refused,
    // and the preview says so.
    f.hover(Point2(17, 0.3));
    paint(*f.view);
    EXPECT_FALSE(f.view->lastPreviewCounts().refused);
    f.hover(Point2(15, 0.3));
    paint(*f.view);
    EXPECT_TRUE(f.view->lastPreviewCounts().refused);
}

TEST(VertexToolHover, TheSnapMarkerLiesUnderTheNewVertexNotOverIt)
{
    // With the snaps on, the cursor by the middle of segment 0 snaps to its
    // Midpoint, (5,0), where the new vertex goes: the triangle was drawn over
    // the disc and hid its "+".
    Fixture f;
    f.view->setSnapEnabled(true);
    ASSERT_TRUE(f.view->startTool("draw.vertex.insert").ok());
    f.hover(Point2(5.2, 0.3));
    const QImage image = f.view->grab().toImage();
    ASSERT_EQ(f.view->lastPreviewCounts().added, 1u);
    const QPointF disc = f.pixel(Point2(5, 0));
    const QColor snap = katana::qt::drawing::overlay::snap();
    // The Midpoint marker is a triangle 6 px each way (viewport_widget.cpp,
    // drawSnapMarker): its right side runs from (0,-6) to (6,6) about the
    // point, through (3.6,1.2) - inside the 6 px disc and off its "+".
    // There: the disc's cyan, and no yellow over it.
    const QPointF onTheSide = disc + QPointF(3.6, 1.2);
    EXPECT_EQ(inkNear(image, onTheSide, 0.8, snap), 0);
    EXPECT_GT(inkNear(image, onTheSide, 0.8, katana::qt::drawing::overlay::preview()), 0);
    // The marker is there, beyond the disc: the triangle's lower corners.
    EXPECT_GT(inkNear(image, disc, 10, snap), 0);
}

TEST(VertexToolHover, APolylineOnALayerThisViewHidesIsNotEditedThroughTheSelection)
{
    // Selected, then its layer hidden in this view alone: the view's pick
    // passes it over, and the tool's own look through the selection must too.
    Fixture f;
    f.select();
    const std::string layer = f.document.model().entities.find(f.polyline)->layer;
    f.view->state().layers.hide(layer);
    ASSERT_TRUE(f.view->startTool("draw.vertex.delete").ok());
    f.hover(Point2(10, 0));
    paint(*f.view);
    EXPECT_EQ(f.view->lastPreviewCounts().removed, 0u);
    f.click(Point2(10, 0));
    EXPECT_EQ(f.vertices().size(), 3u);
}

TEST(VertexToolHover, BesideAChosenVertexEntersPlaceIsDrawn)
{
    Fixture f;
    f.select();
    f.click(Point2(10, 0)); // the middle vertex's grip, now hot
    ASSERT_TRUE(f.view->startTool("draw.vertex.insert").ok());
    // By segment 0: the click's place there, and Enter's at (15,0), the
    // middle of segment 1.
    f.hover(Point2(6, 0.3));
    const QImage image = f.view->grab().toImage();
    EXPECT_EQ(f.view->lastPreviewCounts().enter, 1u);
    EXPECT_EQ(f.view->lastPreviewCounts().added, 1u);
    EXPECT_GT(inkNear(image, f.pixel(Point2(15, 0)), 7.5, katana::qt::drawing::overlay::preview()),
              0);
}

TEST(VertexToolHover, ThePointerRecordReadsBackAsOneRecordWithABearingInItsCaption)
{
    // Move Vertex's caption ends in a bearing, whose seconds mark is a
    // double quote. The record quoted the caption by hand, the quote closed
    // it early, and the line read back as no record at all. It is written
    // as every reply is (core::replyQuoted), and read back whole.
    Fixture f;
    // Shown, as the window is in a headless run: pointerAt paints with the
    // view's own repaint, which paints nothing for a view not yet exposed.
    f.view->show();
    katana::qt::test::processEvents();
    f.select();
    ASSERT_TRUE(f.view->startTool("draw.vertex.move").ok());
    ASSERT_TRUE(f.view->pointerAt(Point2(10, 0), true).ok()); // vertex 1
    const auto hovered = f.view->pointerAt(Point2(10, 3), false);
    ASSERT_TRUE(hovered.ok()) << hovered.error().describe();
    const std::string caption = f.view->lastPreviewCounts().caption;
    ASSERT_NE(caption.find('"'), std::string::npos) << "a bearing's seconds mark: " << caption;
    const auto record = katana::core::readReplyRecord(*hovered);
    ASSERT_TRUE(record.has_value()) << *hovered;
    EXPECT_EQ(record->words, std::vector<std::string>{"pointer:"});
    EXPECT_EQ(record->value("caption"), caption);
    EXPECT_EQ(record->value("prompt"), "Move Vertex: Specify its new position");
    // Its keys for the roles are the roles' own names.
    for (const auto role : {katana::cad::FeedbackRole::Target, katana::cad::FeedbackRole::Added,
                            katana::cad::FeedbackRole::Removed, katana::cad::FeedbackRole::Enter}) {
        EXPECT_TRUE(record->value(katana::cad::toString(role)).has_value())
            << katana::cad::toString(role);
    }
    EXPECT_EQ(record->value("added"), "1") << "the vertex where it goes";
}

TEST(VertexToolHover, ALongPromptIsCutInTheMiddleSoTheToolsNameStays)
{
    // Cut at the left, a prompt longer than the view lost the tool's name -
    // at the larger text sizes, or in a narrow view - and the band read
    // "...Vertex: Click beside". With nothing typed it is cut in the middle;
    // with something typed, at the left, where the typing is.
    Fixture f;
    f.view->resize(260, 300);
    f.select();
    f.click(Point2(10, 0)); // the middle vertex's grip, now chosen
    ASSERT_TRUE(f.view->startTool("draw.vertex.insert").ok());
    paint(*f.view);
    const QString band = f.view->promptBandText();
    EXPECT_TRUE(band.startsWith("Insert Vertex: ")) << band.toStdString();
    EXPECT_TRUE(band.contains(QChar(0x2026))) << "cut: " << band.toStdString();
    EXPECT_TRUE(band.endsWith("_")) << band.toStdString();
    QKeyEvent digit(QEvent::KeyPress, Qt::Key_4, Qt::NoModifier, "4");
    QCoreApplication::sendEvent(f.view.get(), &digit);
    paint(*f.view);
    EXPECT_TRUE(f.view->promptBandText().endsWith("4_")) << f.view->promptBandText().toStdString();
    EXPECT_FALSE(f.view->promptBandText().startsWith("Insert Vertex: "));
}
