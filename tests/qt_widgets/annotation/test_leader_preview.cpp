// A smart leader painted as a tool's preview or as a grip drags it
// (plan_painter.hpp, paintPlanGeometry; docs/annotation.md "In the window"):
// its note is read off the entity its tip is on every time it is drawn, so it
// has to be painted with the drawing it reads. Painted with none, the preview
// of the preset leader tools (LEADERLEVEL ...) was a bare arrow and landing
// while the person chose where the note goes.

#include <gtest/gtest.h>

#include <QImage>
#include <QPainter>
#include <QPen>

#include <memory>

#include "katana/cad/annotation/leader_presets.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/anchor.hpp"
#include "katana/entity/leader_values.hpp"
#include "plan_painter.hpp"

namespace {

using katana::entity::AnchorPoint;
using katana::entity::AnchorRef;
using katana::entity::Entity;
using katana::entity::LeaderGeometry;
using katana::geometry::Point2;
using katana::qt::PlanFrame;
using katana::qt::PlanPaintCache;
using katana::qt::PlanPaintOptions;
using katana::qt::PlanSource;

// 300 x 300 pixels at 40 a unit about (50.8, 50.5): the unit square from
// (50, 50) to (51, 51) fills a third of it, with room for a note to its right.
// A 2.5 mm note at 1 : 100 is 0.25 units, 10 pixels.
QImage preview(const LeaderGeometry& leader, const PlanSource* source)
{
    PlanFrame frame;
    frame.transform.resize(300.0, 300.0);
    frame.transform.scale = 40.0;
    frame.transform.center = Point2(50.8, 50.5);
    PlanPaintOptions options;
    options.annotationScale = 100.0;
    QImage image(300, 300, QImage::Format_ARGB32_Premultiplied);
    image.fill(Qt::black);
    QPainter painter(&image);
    painter.setPen(QPen(Qt::white, 1.0));
    PlanPaintCache cache;
    katana::qt::paintPlanGeometry(painter, frame, options, cache, leader, katana::entity::DimensionStyle{}, source);
    painter.end();
    return image;
}

std::size_t differing(const QImage& a, const QImage& b)
{
    std::size_t count = 0;
    for (int y = 0; y < a.height(); ++y) {
        for (int x = 0; x < a.width(); ++x) {
            count += a.pixel(x, y) != b.pixel(x, y) ? 1u : 0u;
        }
    }
    return count;
}

std::size_t inked(const QImage& image)
{
    std::size_t count = 0;
    for (int y = 0; y < image.height(); ++y) {
        for (int x = 0; x < image.width(); ++x) {
            count += image.pixel(x, y) != qRgb(0, 0, 0) ? 1u : 0u;
        }
    }
    return count;
}

} // namespace

TEST(LeaderPreview, ASmartLeaderIsPaintedWithItsNoteReadOffTheDrawingItsTipIsOn)
{
    katana::cad::Document document;
    Entity point;
    point.geometry = katana::entity::PointGeometry{Point2(50.0, 50.0)};
    point.properties["elevation"] = 30.25;
    ASSERT_TRUE(document.execute(katana::commands::createEntities({point})).ok());
    const katana::entity::EntityId target = document.lastCreatedEntities().front();

    LeaderGeometry leader;
    leader.vertices = {Point2(50.0, 50.0), Point2(51.0, 51.0)};
    leader.tipRef = AnchorRef{target, AnchorPoint::Position};
    leader.text = "RL {rl:.3f}";
    leader.fields = true;

    PlanSource source;
    source.model = &document.model();
    source.leaders = std::make_shared<const katana::entity::LeaderContext>(
        katana::cad::annotation::leaderContext(document));

    // With no drawing the note's one line names a value nothing has, and is
    // dropped: the arrow, the line and the landing. With it, "RL 30.250" as
    // well - nine glyphs of ten pixels, a hundred pixels of ink and more.
    const QImage bare = preview(leader, nullptr);
    const QImage read = preview(leader, &source);
    ASSERT_GT(inked(bare), 0u) << "the leader itself was not drawn";
    EXPECT_GT(inked(read), inked(bare) + 60u) << "bare " << inked(bare) << ", read " << inked(read);
    EXPECT_GT(differing(read, bare), 60u);
}

TEST(LeaderPreview, APlainLeaderOrOneWhoseTargetIsGonePaintsAsItDidWithOrWithoutTheDrawing)
{
    katana::cad::Document document;
    PlanSource source;
    source.model = &document.model();
    source.leaders = std::make_shared<const katana::entity::LeaderContext>(
        katana::cad::annotation::leaderContext(document));

    // A plain leader says its own text, drawing or no drawing.
    LeaderGeometry plain;
    plain.vertices = {Point2(50.0, 50.0), Point2(51.0, 51.0)};
    plain.text = "plain";
    EXPECT_EQ(differing(preview(plain, nullptr), preview(plain, &source)), 0u);
    EXPECT_GT(inked(preview(plain, &source)), 0u);

    // A smart one whose target is not in the drawing has nothing to read:
    // the arrow alone, as with no drawing at all.
    LeaderGeometry orphan = plain;
    orphan.tipRef = AnchorRef{9999, AnchorPoint::Position};
    orphan.text = "RL {rl:.3f}";
    orphan.fields = true;
    EXPECT_EQ(differing(preview(orphan, nullptr), preview(orphan, &source)), 0u);
}
