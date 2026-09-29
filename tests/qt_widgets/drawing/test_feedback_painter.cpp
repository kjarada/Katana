// A tool's preview drawn by role (src/katana_qt/drawing/feedback_painter.*):
// what a click takes, adds and removes, and the vertices of the polyline in
// play, each drawn where it belongs and in its own colour. Fonts differ from
// one platform to the next, so no count of pixels is absolute: each role's
// ink near its mark is compared with a baseline painted with nothing.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <variant>

#include <QImage>
#include <QPainter>

#include "drawing/feedback_painter.hpp"
#include "theme.hpp"

using katana::cad::FeedbackMark;
using katana::cad::FeedbackRole;
using katana::cad::ToolFeedback;
using katana::entity::Geometry;
using katana::entity::PointGeometry;
using katana::geometry::Point2;
using katana::geometry::Segment2;
namespace drawing = katana::qt::drawing;
namespace overlay = katana::qt::drawing::overlay;

namespace {

// 400 x 300 px at 10 px a unit, model (0,0) at pixel (100,200), y up.
QPointF toScreen(const Point2& p) { return QPointF(100.0 + 10.0 * p.x, 200.0 - 10.0 * p.y); }

struct Painted {
    QImage image;
    drawing::FeedbackCounts counts;
};

Painted paintOver(const ToolFeedback& feedback, const std::vector<Point2>& focus)
{
    Painted out;
    out.image = QImage(400, 300, QImage::Format_ARGB32_Premultiplied);
    out.image.fill(katana::qt::theme::viewport());
    QPainter painter(&out.image);
    painter.setRenderHint(QPainter::Antialiasing, true);
    drawing::FeedbackFrame frame;
    frame.toScreen = toScreen;
    // The segments these tests mark, in the painter's pen, as the plan
    // painter draws a geometry.
    frame.drawShape = [&painter](const Geometry& geometry) {
        if (const auto* segment = std::get_if<Segment2>(&geometry)) {
            painter.drawLine(toScreen(segment->start), toScreen(segment->end));
        }
    };
    frame.visible = QRectF(0, 0, 400, 300);
    frame.cursor = QPointF(100, 60);
    out.counts = drawing::paintFeedback(painter, feedback, focus, frame);
    return out;
}

// Whether `pixel` is `colour` laid over `ground` at some coverage from 40 %
// to full, within 24 a channel: the ink of that colour, anti-aliased edges
// included, and never the ground or a colour of another role.
bool inkOf(QColor pixel, QColor colour, QColor ground)
{
    const double d[3] = {double(colour.red() - ground.red()), double(colour.green() - ground.green()),
                         double(colour.blue() - ground.blue())};
    const double p[3] = {double(pixel.red() - ground.red()), double(pixel.green() - ground.green()),
                         double(pixel.blue() - ground.blue())};
    const double length = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    const double t = (p[0] * d[0] + p[1] * d[1] + p[2] * d[2]) / length;
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
    const QColor ground = katana::qt::theme::viewport();
    for (int y = int(centre.y() - radius); y <= int(centre.y() + radius); ++y) {
        for (int x = int(centre.x() - radius); x <= int(centre.x() + radius); ++x) {
            if (x < 0 || y < 0 || x >= image.width() || y >= image.height() ||
                std::hypot(x - centre.x(), y - centre.y()) > radius) {
                continue;
            }
            count += inkOf(image.pixelColor(x, y), colour, ground) ? 1 : 0;
        }
    }
    return count;
}

ToolFeedback sample()
{
    ToolFeedback feedback;
    feedback.marks.push_back(
        FeedbackMark{FeedbackRole::Target, Geometry{Segment2{Point2(0, 0), Point2(10, 0)}}, {}});
    feedback.marks.push_back(FeedbackMark{FeedbackRole::Added, Geometry{PointGeometry{Point2(5, 0)}}, {}});
    feedback.marks.push_back(
        FeedbackMark{FeedbackRole::Removed, Geometry{PointGeometry{Point2(10, 0)}}, {}});
    return feedback;
}

const std::vector<Point2> kFocus{Point2(0, 0), Point2(10, 0), Point2(20, 0)};

} // namespace

TEST(FeedbackPainter, EachRoleIsCountedAsDrawn)
{
    const Painted painted = paintOver(sample(), kFocus);
    EXPECT_EQ(painted.counts.target, 1u);
    EXPECT_EQ(painted.counts.added, 1u);
    EXPECT_EQ(painted.counts.removed, 1u);
    EXPECT_EQ(painted.counts.focus, 3u);
    EXPECT_EQ(painted.counts.shapes, 0u);
    EXPECT_EQ(painted.counts.markers, 0u);
}

TEST(FeedbackPainter, EachRoleIsInkedInItsColourWhereItsMarkIs)
{
    const Painted marked = paintOver(sample(), kFocus);
    const Painted baseline = paintOver(ToolFeedback{}, {});
    // The target segment at (2,0), away from the other marks.
    const QPointF onTarget = toScreen(Point2(2, 0));
    EXPECT_GT(inkNear(marked.image, onTarget, 8, overlay::target()),
              inkNear(baseline.image, onTarget, 8, overlay::target()));
    const QPointF added = toScreen(Point2(5, 0));
    EXPECT_GT(inkNear(marked.image, added, 8, overlay::preview()),
              inkNear(baseline.image, added, 8, overlay::preview()));
    const QPointF removed = toScreen(Point2(10, 0));
    EXPECT_GT(inkNear(marked.image, removed, 8, overlay::removed()),
              inkNear(baseline.image, removed, 8, overlay::removed()));
    // The polyline's vertex with no mark of its own shows the cold square.
    const QPointF focus = toScreen(Point2(20, 0));
    EXPECT_GT(inkNear(marked.image, focus, 8, overlay::gripCold()),
              inkNear(baseline.image, focus, 8, overlay::gripCold()));
    // And a role's colour is not drawn where another role's mark is: no
    // removed red at the added vertex.
    EXPECT_EQ(inkNear(marked.image, added, 4, overlay::removed()), 0);
}

TEST(FeedbackPainter, ACrowdedNewVertexGivesWayToTheVertexThatGoes)
{
    // A fillet a few pixels across: its new vertex 3 px from the corner's X.
    // Alone the new vertex is drawn; over the X it is left out, so the X
    // still reads - and it is still counted, being what the tool will do.
    ToolFeedback alone;
    alone.marks.push_back(
        FeedbackMark{FeedbackRole::Added, Geometry{PointGeometry{Point2(10.3, 0)}}, {}});
    ToolFeedback crowded = alone;
    crowded.marks.push_back(
        FeedbackMark{FeedbackRole::Removed, Geometry{PointGeometry{Point2(10, 0)}}, {}});
    const Painted drawn = paintOver(alone, {});
    const Painted given = paintOver(crowded, {});
    const QPointF where = toScreen(Point2(10.3, 0));
    EXPECT_GT(inkNear(drawn.image, where, 6, overlay::preview()), 0);
    EXPECT_EQ(inkNear(given.image, where, 6, overlay::preview()), 0);
    EXPECT_GT(inkNear(given.image, where, 6, overlay::removed()), 0);
    EXPECT_EQ(given.counts.added, 1u);
}

TEST(FeedbackPainter, ARefusedCaptionIsInTheRefusalsColour)
{
    ToolFeedback refused;
    refused.caption = "too close to vertex 0";
    refused.refused = true;
    ToolFeedback taken;
    taken.caption = "too close to vertex 0";
    const Painted red = paintOver(refused, {});
    const Painted plain = paintOver(taken, {});
    // The caption's chip starts 16 px right of and below the cursor at
    // (100,60): its red stripe and the first words are round (140,84).
    const QPointF chip(140, 84);
    EXPECT_GT(inkNear(red.image, chip, 40, overlay::removed()),
              inkNear(plain.image, chip, 40, overlay::removed()));
}

TEST(FeedbackPainter, ARefusedPicksTargetIsDrawnInTheRefusalsRedNotTheTargetsGreen)
{
    // Fillet over an end vertex: the marks still say what the pick took, but
    // green there would promise the click the caption says is turned down.
    ToolFeedback taken;
    taken.marks.push_back(
        FeedbackMark{FeedbackRole::Target, Geometry{PointGeometry{Point2(5, 0)}}, "0"});
    taken.marks.push_back(
        FeedbackMark{FeedbackRole::Target, Geometry{Segment2{Point2(10, 5), Point2(20, 5)}}, {}});
    ToolFeedback refused = taken;
    refused.refused = true;
    const Painted green = paintOver(taken, {});
    const Painted red = paintOver(refused, {});
    const Painted baseline = paintOver(ToolFeedback{}, {});
    const QPointF vertex = toScreen(Point2(5, 0));
    const QPointF piece = toScreen(Point2(15, 5));
    for (const QPointF& at : {vertex, piece}) {
        EXPECT_GT(inkNear(green.image, at, 9, overlay::target()),
                  inkNear(baseline.image, at, 9, overlay::target()));
        EXPECT_EQ(inkNear(green.image, at, 9, overlay::removed()), 0);
        EXPECT_GT(inkNear(red.image, at, 9, overlay::removed()),
                  inkNear(baseline.image, at, 9, overlay::removed()));
        EXPECT_EQ(inkNear(red.image, at, 9, overlay::target()), 0);
    }
    // Still the target in the counts: the record says what the pick took.
    EXPECT_EQ(red.counts.target, 2u);
}
