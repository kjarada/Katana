// A tool's preview drawn by role (src/katana_qt/drawing/feedback_painter.*):
// what a click takes, adds and removes, and the vertices of the polyline in
// play, each drawn where it belongs and in its own colour. Fonts differ from
// one platform to the next, so no count of pixels is absolute: each role's
// ink near its mark is compared with a baseline painted with nothing.

#include <gtest/gtest.h>

#include <algorithm>
#include <array>
#include <cmath>
#include <variant>

#include <QImage>
#include <QPainter>
#include <QPen>

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
    // still reads - and, left out, it is not counted: the counts are the
    // headless record of what is on screen, and a record of added=1 over a
    // picture with no disc told a test something that was not there.
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
    EXPECT_EQ(drawn.counts.added, 1u);
    EXPECT_EQ(given.counts.added, 0u);
}

TEST(FeedbackPainter, EntersPlaceIsAnOutlinedRingNotTheNewVertexsFilledDisc)
{
    // Where Enter would add: the Added disc's circle in the same cyan round
    // a small dot, and between the two the dark ground, where the disc is
    // cyan all through (EntersPlaceIsARingRoundADotWithNoLineThroughIt has
    // the dot).
    ToolFeedback enter;
    enter.marks.push_back(
        FeedbackMark{FeedbackRole::Enter, Geometry{PointGeometry{Point2(5, 0)}}, "Enter"});
    ToolFeedback added;
    added.marks.push_back(FeedbackMark{FeedbackRole::Added, Geometry{PointGeometry{Point2(5, 0)}}, {}});
    const Painted ring = paintOver(enter, {});
    const Painted disc = paintOver(added, {});
    const Painted baseline = paintOver(ToolFeedback{}, {});
    // The ring, 6 px out: inked. A point 2.5 px out on each axis, 3.5 px from
    // the middle - past the 1.75 px dot, off the disc's "+": the disc's cyan,
    // the ring's ground.
    const QPointF centre = toScreen(Point2(5, 0));
    EXPECT_GT(inkNear(ring.image, centre, 7.5, overlay::preview()),
              inkNear(baseline.image, centre, 7.5, overlay::preview()));
    const QPointF inside = centre + QPointF(2.5, 2.5);
    EXPECT_GT(inkNear(disc.image, inside, 1, overlay::preview()), 0);
    EXPECT_EQ(inkNear(ring.image, inside, 1, overlay::preview()), 0);
    EXPECT_EQ(ring.counts.enter, 1u);
    EXPECT_EQ(ring.counts.added, 0u) << "Enter's place is not what a click adds";
}

TEST(FeedbackPainter, ADenseStringsVerticesAreThinnedAndLieUnderTheTarget)
{
    // 201 vertices 0.1 apart - 1 px at 10 px a unit - under a target
    // segment along them. One square a vertex was a solid blue band drawn
    // over the target; now no two squares are nearer than 10 px, the ends
    // kept, and they are drawn first, so the target's green lies on top.
    std::vector<Point2> dense;
    for (int i = 0; i <= 200; ++i) {
        dense.emplace_back(0.1 * i, 0.0);
    }
    ToolFeedback feedback;
    feedback.marks.push_back(
        FeedbackMark{FeedbackRole::Target, Geometry{Segment2{Point2(0, 0), Point2(20, 0)}}, {}});
    const Painted painted = paintOver(feedback, dense);
    // 200 px of string at one square per 10 px: 21 squares, give or take
    // one where 0.1 x 10 px rounds under 10.
    EXPECT_GE(painted.counts.focus, 20u);
    EXPECT_LE(painted.counts.focus, 22u);
    // Midway between squares and on them alike, the segment is green.
    for (const double x : {3.05, 7.0, 11.05, 15.0}) {
        EXPECT_GT(inkNear(painted.image, toScreen(Point2(x, 0)), 1.5, overlay::target()), 0)
            << "at x = " << x;
    }
}

TEST(FeedbackPainter, TheSnapMarkerLiesBeneathTheNewVertex)
{
    // The view's snap marker is drawn by beneathGlyphs: before the vertex
    // glyphs, so a yellow X at the new vertex no longer crosses its disc.
    ToolFeedback feedback;
    feedback.marks.push_back(FeedbackMark{FeedbackRole::Added, Geometry{PointGeometry{Point2(5, 0)}}, {}});
    QImage image(400, 300, QImage::Format_ARGB32_Premultiplied);
    image.fill(katana::qt::theme::viewport());
    QPainter painter(&image);
    painter.setRenderHint(QPainter::Antialiasing, true);
    drawing::FeedbackFrame frame;
    frame.toScreen = toScreen;
    frame.drawShape = [](const Geometry&) {};
    frame.visible = QRectF(0, 0, 400, 300);
    frame.cursor = QPointF(100, 60);
    const QPointF centre = toScreen(Point2(5, 0));
    frame.beneathGlyphs = [&painter, centre] {
        // The Intersection marker: an X 6 px each way, 2 px wide.
        painter.setPen(QPen(overlay::snap(), 2));
        painter.drawLine(centre + QPointF(-6, -6), centre + QPointF(6, 6));
        painter.drawLine(centre + QPointF(-6, 6), centre + QPointF(6, -6));
    };
    (void)drawing::paintFeedback(painter, feedback, {}, frame);
    painter.end();
    // On the disc, off its "+": the disc's cyan, and no yellow over it.
    EXPECT_EQ(inkNear(image, centre + QPointF(2.5, 2.5), 1, overlay::snap()), 0);
    EXPECT_GT(inkNear(image, centre + QPointF(2.5, 2.5), 1, overlay::preview()), 0);
    // The marker's tips, beyond the disc, still show.
    EXPECT_GT(inkNear(image, centre + QPointF(5.5, 5.5), 1, overlay::snap()), 0);
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

// ---- the review of 2026-09-30, second round ------------------------------------------------

namespace {

// A colour as a red-green colour-blind eye sees it: deuteranopia at severity
// 1.0 in Machado, Oliveira and Fernandes (2009), "A Physiologically-based
// Model for Simulation of Color Vision Deficiency", IEEE TVCG 15(6), whose
// matrix applies to linear RGB (IEC 61966-2-1's sRGB decoding).
std::array<double, 3> deuteranope(const QColor& colour)
{
    const auto linear = [](int channel) {
        const double v = channel / 255.0;
        return v <= 0.04045 ? v / 12.92 : std::pow((v + 0.055) / 1.055, 2.4);
    };
    const double r = linear(colour.red());
    const double g = linear(colour.green());
    const double b = linear(colour.blue());
    return {0.367322 * r + 0.860646 * g - 0.227968 * b,
            0.280085 * r + 0.672501 * g + 0.047413 * b,
            -0.011820 * r + 0.042940 * g + 0.968881 * b};
}

// The pixels within `radius` of `centre` that such an eye sees differently in
// `a` and `b`: some channel more than 0.08 apart in linear light. The
// target's green (0x3cd070) and the refusal's red (0xff6b6b) come out at
// (0.523, 0.445, 0.184) and (0.460, 0.386, 0.137) - 0.063 apart at most,
// worked from the matrix - so two marks that differ only in those two hues
// count none (the control below).
int seenApart(const QImage& a, const QImage& b, QPointF centre, double radius)
{
    int count = 0;
    for (int y = int(centre.y() - radius); y <= int(centre.y() + radius); ++y) {
        for (int x = int(centre.x() - radius); x <= int(centre.x() + radius); ++x) {
            if (std::hypot(x - centre.x(), y - centre.y()) > radius) {
                continue;
            }
            const auto p = deuteranope(a.pixelColor(x, y));
            const auto q = deuteranope(b.pixelColor(x, y));
            if (std::abs(p[0] - q[0]) > 0.08 || std::abs(p[1] - q[1]) > 0.08 ||
                std::abs(p[2] - q[2]) > 0.08) {
                ++count;
            }
        }
    }
    return count;
}

} // namespace

TEST(FeedbackPainter, ARefusedMarkDiffersInShapeNotOnlyInHueToARedGreenColourBlindEye)
{
    // Fillet on an end vertex, Segment to Line on a straight segment: the
    // refused pick was the accepted one's ring and line in red, the same
    // khaki to a deuteranope as the green. Now the refusal's own shape - the
    // ring struck through, on a vertex and at a piece's middle - tells them
    // apart without the colour.
    ToolFeedback taken;
    taken.marks.push_back(FeedbackMark{FeedbackRole::Target, Geometry{PointGeometry{Point2(5, 0)}}, {}});
    taken.marks.push_back(
        FeedbackMark{FeedbackRole::Target, Geometry{Segment2{Point2(10, 5), Point2(20, 5)}}, {}});
    ToolFeedback refused = taken;
    refused.refused = true;
    const Painted accepted = paintOver(taken, {});
    const Painted turnedDown = paintOver(refused, {});
    EXPECT_GT(seenApart(accepted.image, turnedDown.image, toScreen(Point2(5, 0)), 9), 10);
    EXPECT_GT(seenApart(accepted.image, turnedDown.image, toScreen(Point2(15, 5)), 9), 10);

    // The control: one ring and line drawn in each colour, the same shape,
    // are not told apart by this measure.
    const auto drawn = [](const QColor& colour) {
        QImage image(400, 300, QImage::Format_ARGB32_Premultiplied);
        image.fill(katana::qt::theme::viewport());
        QPainter painter(&image);
        painter.setRenderHint(QPainter::Antialiasing, true);
        painter.setPen(QPen(colour, 2.0));
        painter.drawEllipse(toScreen(Point2(5, 0)), 7.0, 7.0);
        painter.setPen(QPen(colour, 3.0));
        painter.drawLine(toScreen(Point2(10, 5)), toScreen(Point2(20, 5)));
        return image;
    };
    const QImage green = drawn(overlay::target());
    const QImage red = drawn(overlay::removed());
    EXPECT_EQ(seenApart(green, red, toScreen(Point2(5, 0)), 9), 0);
    EXPECT_EQ(seenApart(green, red, toScreen(Point2(15, 5)), 9), 0);
}

TEST(FeedbackPainter, OnlyTheMarkFlaggedAsTheReasonIsDrawnRefused)
{
    // Insert beside a chosen vertex, too near another: the vertex it is too
    // near is the refusal's, struck in red; the chosen vertex stays the
    // target green it was - the whole preview in red said the choice itself
    // was refused.
    ToolFeedback feedback;
    feedback.refused = true;
    feedback.marks.push_back(
        FeedbackMark{FeedbackRole::Target, Geometry{PointGeometry{Point2(5, 0)}}, "2"});
    feedback.marks.push_back(
        FeedbackMark{FeedbackRole::Target, Geometry{PointGeometry{Point2(15, 0)}}, "0", true});
    const Painted painted = paintOver(feedback, {});
    const Painted baseline = paintOver(ToolFeedback{}, {});
    const QPointF chosen = toScreen(Point2(5, 0));
    const QPointF near = toScreen(Point2(15, 0));
    EXPECT_GT(inkNear(painted.image, chosen, 8, overlay::target()),
              inkNear(baseline.image, chosen, 8, overlay::target()));
    EXPECT_EQ(inkNear(painted.image, chosen, 8, overlay::removed()), 0);
    EXPECT_GT(inkNear(painted.image, near, 8, overlay::removed()),
              inkNear(baseline.image, near, 8, overlay::removed()));
    EXPECT_EQ(inkNear(painted.image, near, 8, overlay::target()), 0);
}

TEST(FeedbackPainter, EntersPlaceIsARingRoundADotWithNoLineThroughIt)
{
    // Insert beside a chosen vertex draws Enter's place on the segment it
    // splits. Hollow, the ring had the green line across its middle - a
    // circled minus beside the new vertex's circled plus, "remove" and "add"
    // for two places that both add. Its middle is now the dark ground with a
    // dot, and the line stops at the ring.
    ToolFeedback feedback;
    feedback.marks.push_back(
        FeedbackMark{FeedbackRole::Target, Geometry{Segment2{Point2(0, 0), Point2(10, 0)}}, {}});
    feedback.marks.push_back(
        FeedbackMark{FeedbackRole::Enter, Geometry{PointGeometry{Point2(5, 0)}}, {}});
    const Painted painted = paintOver(feedback, {});
    const QPointF centre = toScreen(Point2(5, 0));
    // On the line, inside the ring's 6 px: no green.
    EXPECT_EQ(inkNear(painted.image, centre + QPointF(3.5, 0), 1, overlay::target()), 0);
    EXPECT_EQ(inkNear(painted.image, centre - QPointF(3.5, 0), 1, overlay::target()), 0);
    // The dot at its middle, in the preview cyan.
    EXPECT_GT(inkNear(painted.image, centre, 1, overlay::preview()), 0);
    // Beyond the ring the line goes on.
    EXPECT_GT(inkNear(painted.image, centre + QPointF(12, 0), 1, overlay::target()), 0);
}

TEST(FeedbackPainter, TheCaptionKeepsOffTheMarks)
{
    // The caption's chip went 16 px right of and below the cursor whatever
    // was there; beside a chosen vertex it hid Enter's place. With a mark
    // where it would go - the cursor at (100,60), Enter's place at pixel
    // (150,86), model (5,11.4) - it goes to another corner of the cursor,
    // and every bit of the mark's ink is still there.
    ToolFeedback feedback;
    feedback.marks.push_back(
        FeedbackMark{FeedbackRole::Enter, Geometry{PointGeometry{Point2(5, 11.4)}}, {}});
    feedback.caption = "vertex 2 · 5.000";
    ToolFeedback bare = feedback;
    bare.caption.clear();
    const Painted with = paintOver(feedback, {});
    const Painted without = paintOver(bare, {});
    const QPointF mark = toScreen(Point2(5, 11.4));
    ASSERT_GT(inkNear(without.image, mark, 7.5, overlay::preview()), 0);
    EXPECT_EQ(inkNear(with.image, mark, 7.5, overlay::preview()),
              inkNear(without.image, mark, 7.5, overlay::preview()));
    // The caption is drawn all the same: its text's ink, above the cursor.
    int text = 0;
    for (int y = 0; y < 60; ++y) {
        for (int x = 100; x < 400; ++x) {
            text += inkOf(with.image.pixelColor(x, y), katana::qt::theme::text(),
                          katana::qt::theme::viewport())
                        ? 1
                        : 0;
        }
    }
    EXPECT_GT(text, 0);
}

TEST(FeedbackPainter, TheBandCutsWhereItIsToldAndSaysWhatItDrew)
{
    // A prompt too long for the view: cut in the middle it keeps the tool's
    // name and its end; at the left, as for what is being typed, it keeps
    // the end alone.
    QImage image(300, 100, QImage::Format_ARGB32_Premultiplied);
    image.fill(katana::qt::theme::viewport());
    QPainter painter(&image);
    const QString prompt =
        "Insert Vertex: Press Enter for a vertex at the marked middle of segment 2, or click "
        "beside vertex 2 of polyline 2 where the new vertex goes, or anywhere else nearby";
    const QString middle = drawing::paintBand(painter, QRect(0, 0, 300, 100), prompt, Qt::ElideMiddle);
    EXPECT_TRUE(middle.startsWith("Insert Vertex:")) << middle.toStdString();
    EXPECT_TRUE(middle.endsWith("nearby")) << middle.toStdString();
    EXPECT_LT(middle.size(), prompt.size());
    const QString left = drawing::paintBand(painter, QRect(0, 0, 300, 100), prompt, Qt::ElideLeft);
    EXPECT_FALSE(left.startsWith("Insert Vertex:")) << left.toStdString();
    EXPECT_TRUE(left.endsWith("nearby"));
}

// ---- the review of 2026-09-30, third round -------------------------------------------------

TEST(FeedbackPainter, ARunOfVerticesThatGoIsThinnedAndNeverCrossesOutAKeptVertex)
{
    // Straighten on a dense string: "keep" rings at (0,0) and (6,0), pixels
    // (100,200) and (160,200), and the 29 vertices between, 0.2 apart - 2 px
    // - going. An X a vertex was one red rope, and the X beside each end lay
    // across its "keep" ring: a circled X, crossing out a vertex that stays.
    ToolFeedback feedback;
    feedback.marks.push_back(
        FeedbackMark{FeedbackRole::Target, Geometry{PointGeometry{Point2(0, 0)}}, "keep"});
    feedback.marks.push_back(
        FeedbackMark{FeedbackRole::Target, Geometry{PointGeometry{Point2(6, 0)}}, "keep"});
    for (int i = 1; i < 30; ++i) {
        feedback.marks.push_back(
            FeedbackMark{FeedbackRole::Removed, Geometry{PointGeometry{Point2(0.2 * i, 0)}}, {}});
    }
    const Painted painted = paintOver(feedback, {});
    // No red within either ring's outer edge, 8 px.
    for (const Point2& kept : {Point2(0, 0), Point2(6, 0)}) {
        EXPECT_EQ(inkNear(painted.image, toScreen(kept), 8, overlay::removed()), 0)
            << "at " << kept.x;
    }
    // The X's clear of the rings by an X's reach (8 + 7.6 px) lie from pixel
    // 116 to 144, 29 px; at one per 10 px, three - four at most, where the
    // floating point puts two a hair under 10 px apart and one is skipped.
    EXPECT_GE(painted.counts.removed, 2u);
    EXPECT_LE(painted.counts.removed, 4u);
    EXPECT_GT(inkNear(painted.image, toScreen(Point2(3, 0)), 16, overlay::removed()), 0)
        << "some of the run is still marked";
}

TEST(FeedbackPainter, AChosenVertexIsDrawnOverEntersPlaceBesideIt)
{
    // Insert beside a chosen vertex on a short segment: Enter's place, the
    // segment's middle, 5 px from the chosen vertex. Enter's opaque ring was
    // drawn after the vertex and covered its ring and all of its filled
    // square - the vertex the owner asked to see.
    ToolFeedback chosen;
    chosen.marks.push_back(
        FeedbackMark{FeedbackRole::Target, Geometry{PointGeometry{Point2(0, 0)}}, "5"});
    ToolFeedback both = chosen;
    both.marks.push_back(
        FeedbackMark{FeedbackRole::Enter, Geometry{PointGeometry{Point2(0.5, 0)}}, "Enter"});
    const Painted alone = paintOver(chosen, {});
    const Painted beside = paintOver(both, {});
    const QPointF square = toScreen(Point2(0, 0));
    ASSERT_GT(inkNear(alone.image, square, 2.5, overlay::target()), 0);
    EXPECT_EQ(inkNear(beside.image, square, 2.5, overlay::target()),
              inkNear(alone.image, square, 2.5, overlay::target()));
    // Enter's place is still there: its ring's far side, 11 px from the
    // vertex, past the vertex's ring.
    EXPECT_GT(inkNear(beside.image, toScreen(Point2(0.5, 0)) + QPointF(6, 0), 1.5,
                      overlay::preview()),
              0);
}

namespace {

// WCAG 2.1's relative luminance of an sRGB colour, and the contrast ratio
// of two (https://www.w3.org/TR/WCAG21/#dfn-relative-luminance).
double luminance(const QColor& colour)
{
    const auto linear = [](int channel) {
        const double v = channel / 255.0;
        return v <= 0.04045 ? v / 12.92 : std::pow((v + 0.055) / 1.055, 2.4);
    };
    return 0.2126 * linear(colour.red()) + 0.7152 * linear(colour.green()) +
           0.0722 * linear(colour.blue());
}

double contrast(const QColor& a, const QColor& b)
{
    const double la = luminance(a);
    const double lb = luminance(b);
    return (std::max(la, lb) + 0.05) / (std::min(la, lb) + 0.05);
}

} // namespace

TEST(FeedbackPainter, ARefusedVertexsNumberReadsAtThreeToOneAgainstItsChip)
{
    // A refused pick names its vertex in red beside the struck ring. At the
    // regular weight the "1"'s one-pixel stem fell across two columns at
    // about half ink each - wherever the label began - and its most inked
    // pixel read at 2.9:1 against the chip (#9b4849 on #14191d), under the
    // 3:1 of WCAG 2.1's 1.4.11 that the overlay's colours are chosen to
    // (feedback_painter.hpp), where the red itself is 6.4:1. Labels are
    // DemiBold now: 4.9:1 on this machine's font. The vertex at (5,0), pixel
    // (150,200): the label's baseline starts at (159,191), its chip one pixel
    // left of that, the text one right.
    ToolFeedback feedback;
    FeedbackMark mark{FeedbackRole::Target, Geometry{PointGeometry{Point2(5, 0)}}, "1"};
    mark.refused = true;
    feedback.marks.push_back(mark);
    feedback.refused = true;
    const Painted painted = paintOver(feedback, {});
    // The chip's own ground: its column left of the text.
    const QColor chip = painted.image.pixelColor(159, 186);
    // The label's most inked pixel: the most red over green (the red
    // 0xff6b6b is 148 more red than green, the chip next to none), right
    // of the struck ring's outer edge at 158.5.
    QColor inked = chip;
    for (int y = 176; y <= 196; ++y) {
        for (int x = 159; x <= 180; ++x) {
            const QColor pixel = painted.image.pixelColor(x, y);
            if (pixel.red() - pixel.green() > inked.red() - inked.green()) {
                inked = pixel;
            }
        }
    }
    ASSERT_GT(inked.red() - inked.green(), 40) << "the label is there to measure";
    EXPECT_GE(contrast(inked, chip), 3.0)
        << "the stem at " << inked.name().toStdString() << " on " << chip.name().toStdString();
}

TEST(FeedbackPainter, TheCaptionKeepsOffAPieceAlongItsLengthNotOnlyItsMiddle)
{
    // A target segment passing below and right of the cursor, its middle far
    // off: pixel x 130 from the top of the view to the bottom, the middle at
    // (130,150). Only a piece's middle was kept off, so the caption went
    // below and right of the cursor at (100,60) - from pixel 116 rightwards,
    // 76 to 96 down - over the very line the preview marks. A short caption,
    // so that one corner, below and left, is clear of the line. And the same
    // line running a million units past the view each way, at 10 px a unit:
    // it is kept off where it is seen, sampled there alone.
    for (const double reach : {20.0, 1.0e6}) {
        ToolFeedback feedback;
        feedback.marks.push_back(FeedbackMark{
            FeedbackRole::Target, Geometry{Segment2{Point2(3, reach), Point2(3, -reach)}}, {}});
        feedback.caption = "vertex 2";
        ToolFeedback bare = feedback;
        bare.caption.clear();
        const Painted with = paintOver(feedback, {});
        const Painted without = paintOver(bare, {});
        int lineWith = 0;
        int lineWithout = 0;
        for (int y = 70; y <= 110; ++y) {
            lineWith += inkNear(with.image, QPointF(130, y), 1.5, overlay::target()) > 0 ? 1 : 0;
            lineWithout +=
                inkNear(without.image, QPointF(130, y), 1.5, overlay::target()) > 0 ? 1 : 0;
        }
        ASSERT_GT(lineWithout, 30) << reach;
        EXPECT_EQ(lineWith, lineWithout) << reach << ": no part of the line under the caption";
        // The caption is drawn all the same.
        int text = 0;
        for (int y = 0; y < 300; ++y) {
            for (int x = 0; x < 400; ++x) {
                text += inkOf(with.image.pixelColor(x, y), katana::qt::theme::text(),
                              katana::qt::theme::viewport())
                            ? 1
                            : 0;
            }
        }
        EXPECT_GT(text, 0) << reach;
    }
}
