// The annotation front end (src/katana_qt/annotation/, docs/annotation.md
// "In the window"): the painter's paper sizes and marks, the text and label
// style managers, and the annotation scale box - each a thin front end over
// the commands, so each edit is checked as one undo step.

#include <gtest/gtest.h>

#include <QComboBox>
#include <QDoubleSpinBox>
#include <QImage>
#include <QLineEdit>
#include <QMainWindow>
#include <QMenu>
#include <QPainter>
#include <QToolBar>

#include "annotation/annotation_managers.hpp"
#include "annotation/annotation_workbench.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/plot.hpp"
#include "katana/commands/entity_commands.hpp"
#include "plan_painter.hpp"

using katana::cad::Document;
using katana::cad::PlotSettings;
using katana::entity::Entity;
using katana::entity::Model;
using katana::geometry::Point2;
using katana::qt::PlanFrame;
using katana::qt::PlanMedium;
using katana::qt::PlanPaintCache;
using katana::qt::PlanPaintOptions;
using katana::qt::PlanPaintStats;
using katana::qt::PlanSource;
namespace cmd = katana::commands;

namespace {

const QRgb kWhite = qRgb(255, 255, 255);

// A frame on paper at `pixelsPerMillimetre`, drawing 1 : `scale` about
// `centre`: a millimetre of paper is scale / 1000 model units.
PlanFrame paperFrame(double pixelsPerMillimetre, double scale, Point2 centre, double size = 400.0)
{
    PlanFrame frame;
    frame.transform.resize(size, size);
    frame.transform.scale = pixelsPerMillimetre * 1000.0 / scale;
    frame.transform.center = centre;
    return frame;
}

QImage paintedOnPaper(const Model& model, double scale, Point2 centre, PlanPaintStats* stats = nullptr)
{
    const PlotSettings settings;
    PlanPaintOptions options;
    options.medium = PlanMedium::Paper;
    options.pixelsPerMillimetre = 4.0;
    options.plot = &settings;
    options.annotationScale = scale;
    const PlanFrame frame = paperFrame(4.0, scale, centre);
    QImage image(400, 400, QImage::Format_ARGB32_Premultiplied);
    image.fill(kWhite);
    PlanSource source;
    source.model = &model;
    PlanPaintCache cache;
    QPainter painter(&image);
    const PlanPaintStats result = katana::qt::paintPlan(painter, source, frame, options, cache);
    painter.end();
    if (stats != nullptr) {
        *stats = result;
    }
    return image;
}

// The rows from the first to the last with any ink: the height of what was
// drawn, in pixels.
int inkHeight(const QImage& image)
{
    int first = -1;
    int last = -1;
    for (int y = 0; y < image.height(); ++y) {
        for (int x = 0; x < image.width(); ++x) {
            if (image.pixel(x, y) != kWhite) {
                first = first < 0 ? y : first;
                last = y;
                break;
            }
        }
    }
    return first < 0 ? 0 : last - first + 1;
}

int darkPixels(const QImage& image)
{
    int count = 0;
    for (int y = 0; y < image.height(); ++y) {
        for (int x = 0; x < image.width(); ++x) {
            count += qGray(image.pixel(x, y)) < 80 ? 1 : 0;
        }
    }
    return count;
}

// Pixels with any visible ink, antialiased edges included: small text is
// mostly grey edge, and how many of its pixels come out nearly black depends
// on the platform's fonts (Arial on Windows gave 10 where Linux gave more).
int inkPixels(const QImage& image)
{
    int count = 0;
    for (int y = 0; y < image.height(); ++y) {
        for (int x = 0; x < image.width(); ++x) {
            count += qGray(image.pixel(x, y)) < 200 ? 1 : 0;
        }
    }
    return count;
}

Model modelWithStyledText(const char* text, double paperHeight)
{
    Model model;
    katana::entity::TextStyle style;
    style.name = "Notes";
    style.paperHeight = paperHeight;
    EXPECT_TRUE(model.textStyles.add(style).ok());
    katana::entity::TextGeometry geometry{Point2(0, 0), text, 1.0, 0.0};
    geometry.style = "Notes";
    geometry.justify = katana::entity::TextJustify::MiddleCentre;
    EXPECT_TRUE(model.entities.add(Entity{.geometry = geometry}).ok());
    return model;
}

} // namespace

TEST(AnnotationPaint, APaperSizedTextPlotsTheSameHeightAtEveryScale)
{
    // 5 mm at 4 px/mm is 20 px of em; the glyphs' ink is a fixed part of it
    // whatever the scale. A model-unit text of 1 m would be 20 px at 1:200
    // and 2 px at 1:2000.
    const Model model = modelWithStyledText("HHHH", 5.0);
    const int at200 = inkHeight(paintedOnPaper(model, 200.0, Point2(0, 0)));
    const int at2000 = inkHeight(paintedOnPaper(model, 2000.0, Point2(0, 0)));
    ASSERT_GT(at200, 5);
    EXPECT_NEAR(at200, at2000, 1) << "1:200 " << at200 << " px, 1:2000 " << at2000 << " px";

    Model legacy;
    ASSERT_TRUE(legacy.entities
                    .add(Entity{.geometry = katana::entity::TextGeometry{Point2(0, 0), "HHHH", 1.0,
                                                                         0.0}})
                    .ok());
    const int legacy200 = inkHeight(paintedOnPaper(legacy, 200.0, Point2(2, 0.5)));
    const int legacy2000 = inkHeight(paintedOnPaper(legacy, 2000.0, Point2(2, 0.5)));
    EXPECT_GT(legacy200, 3 * legacy2000) << "a model-unit text shrinks with the scale, as before";
}

TEST(AnnotationPaint, AWhiteTextStylePrintsBlack)
{
    Model model = modelWithStyledText("HHHH", 5.0);
    katana::entity::TextStyle white = *model.textStyles.find("Notes");
    white.color = katana::entity::Color{255, 255, 255, 255};
    ASSERT_TRUE(model.textStyles.update(white).ok());
    EXPECT_GT(darkPixels(paintedOnPaper(model, 500.0, Point2(0, 0))), 20)
        << "white on white paper would vanish: it prints black (D7)";
}

TEST(AnnotationPaint, AMaskHidesTheLineUnderTheText)
{
    Model model;
    ASSERT_TRUE(model.entities
                    .add(Entity{.geometry = katana::geometry::Segment2{Point2(-20, 0), Point2(20, 0)}})
                    .ok());
    katana::entity::TextStyle masked;
    masked.name = "Masked";
    masked.paperHeight = 5.0;
    masked.mask = true;
    ASSERT_TRUE(model.textStyles.add(masked).ok());
    // A text of spaces: nothing but its mask, which interrupts the line.
    katana::entity::TextGeometry blank{Point2(0, 0), "      ", 1.0, 0.0};
    blank.style = "Masked";
    blank.justify = katana::entity::TextJustify::MiddleCentre;
    ASSERT_TRUE(model.entities.add(Entity{.geometry = blank}).ok());
    const QImage image = paintedOnPaper(model, 1000.0, Point2(0, 0));
    // Along the line's row: ink, then the mask's gap, then ink again.
    const int row = 200;
    int runs = 0;
    bool inRun = false;
    for (int x = 0; x < image.width(); ++x) {
        const bool inked = qGray(image.pixel(x, row)) < 200;
        runs += inked && !inRun ? 1 : 0;
        inRun = inked;
    }
    EXPECT_EQ(runs, 2);
}

TEST(AnnotationPaint, LabelsArePlacedAndCountedByThePainter)
{
    Model model;
    katana::entity::LabelStyle style;
    style.name = "pt";
    style.text = "{point}";
    ASSERT_TRUE(model.labelStyles.add(style).ok());
    Entity point{.geometry = katana::entity::PointGeometry{Point2(0, 0)}};
    point.properties["point"] = std::string("101");
    ASSERT_TRUE(model.entities.add(point).ok());
    const katana::entity::EntityId target = model.entities.nextId() - 1;
    // The point on its own, for the ink the label adds to it.
    const int pointInk = inkPixels(paintedOnPaper(model, 500.0, Point2(0, 0)));
    ASSERT_TRUE(model.entities
                    .add(Entity{.geometry = katana::entity::LabelGeometry{
                                    .target = target, .style = "pt", .anchor = Point2(0, 0)}})
                    .ok());
    PlanPaintStats stats;
    const QImage image = paintedOnPaper(model, 500.0, Point2(0, 0), &stats);
    EXPECT_EQ(stats.labelsPlaced, 1u);
    EXPECT_EQ(stats.labelsSuppressed, 0u);
    // "101" at 2.5 mm, 4 px a millimetre: tens of pixels of ink over the
    // point's own mark, whatever the platform's font.
    EXPECT_GT(inkPixels(image), pointInk + 20) << "point alone " << pointInk << " px of ink";
}

TEST(AnnotationManagers, TheTextStyleManagerAppliesAsOneStep)
{
    Document document;
    katana::qt::TextStyleManagerDialog dialog(document);
    dialog.select(QStringLiteral("Standard"));
    auto* paper = dialog.findChild<QDoubleSpinBox*>(QStringLiteral("textStylePaperHeight"));
    ASSERT_NE(paper, nullptr);
    paper->setValue(3.5);
    const std::size_t steps = document.history().undoCount();
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    EXPECT_DOUBLE_EQ(document.model().textStyles.find("Standard")->paperHeight, 3.5);
    EXPECT_EQ(document.history().undoCount(), steps + 1);
    EXPECT_TRUE(dialog.apply()) << "applying an unchanged form";
    EXPECT_EQ(document.history().undoCount(), steps + 1) << "is no step";

    ASSERT_TRUE(dialog.addStyle());
    EXPECT_TRUE(document.model().textStyles.contains("Text Style"));
    dialog.select(QStringLiteral("Standard"));
    EXPECT_FALSE(dialog.deleteStyle());
    EXPECT_FALSE(dialog.problem().isEmpty()) << "the refusal is said in the dialog";

    // An undo typed elsewhere shows at once.
    ASSERT_TRUE(document.undo().ok());
    EXPECT_FALSE(document.model().textStyles.contains("Text Style"));
}

TEST(AnnotationManagers, TheLabelStyleManagerChecksTemplatesAndRunsRules)
{
    Document document;
    katana::qt::LabelStyleManagerDialog dialog(document);
    ASSERT_TRUE(dialog.addDefaults());
    EXPECT_EQ(document.model().labelStyles.size(), 7u);
    EXPECT_EQ(document.history().undoCount(), 1u);

    dialog.select(QStringLiteral("Bearing Distance"));
    auto* templateEdit = dialog.findChild<QLineEdit*>(QStringLiteral("labelStyleTemplate"));
    ASSERT_NE(templateEdit, nullptr);
    templateEdit->setText(QStringLiteral("{area}"));
    EXPECT_FALSE(dialog.apply()) << "a segment has no area";
    EXPECT_FALSE(dialog.problem().isEmpty());
    templateEdit->setText(QStringLiteral("{distance:.2f}\\n{bearing:qb}"));
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    EXPECT_EQ(document.model().labelStyles.find("Bearing Distance")->text,
              "{distance:.2f}\n{bearing:qb}");

    // A rule, run and cleared.
    ASSERT_TRUE(document.execute(cmd::createPolyline(katana::geometry::Polyline2{
                                     {Point2(0, 0), Point2(20, 0), Point2(20, 20)}, true}))
                    .ok());
    dialog.findChild<QLineEdit*>(QStringLiteral("labelRuleName"))->setText(QStringLiteral("areas"));
    auto* styleBox = dialog.findChild<QComboBox*>(QStringLiteral("labelRuleStyle"));
    styleBox->setCurrentIndex(styleBox->findText(QStringLiteral("Lot Area")));
    ASSERT_TRUE(dialog.addRule()) << dialog.problem().toStdString();
    ASSERT_TRUE(dialog.runRules()) << dialog.problem().toStdString();
    EXPECT_TRUE(dialog.runReport().startsWith(QStringLiteral("created=1")))
        << dialog.runReport().toStdString();
    ASSERT_TRUE(dialog.clearRules());
    EXPECT_EQ(dialog.runReport(), QStringLiteral("removed=1"));
}

TEST(AnnotationManagers, TheScaleBoxSetsAndFollowsTheAnnotationScale)
{
    Document document;
    QMainWindow window;
    QMenu menu;
    QToolBar toolBar;
    katana::qt::AnnotationWorkbench workbench(window, document, menu, toolBar);
    QComboBox* box = workbench.scaleBox();
    ASSERT_NE(box, nullptr);
    EXPECT_EQ(box->objectName(), QStringLiteral("annotationScaleCombo"));
    EXPECT_EQ(box->currentText(), QStringLiteral("1:1000"));
    box->setCurrentIndex(box->findText(QStringLiteral("1:500")));
    emit box->activated(box->currentIndex());
    EXPECT_DOUBLE_EQ(document.annotationScale(), 500.0);
    ASSERT_TRUE(document.setAnnotationScale(2000.0).ok());
    EXPECT_EQ(box->currentText(), QStringLiteral("1:2000")) << "a scale set elsewhere is shown";
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(box->currentText(), QStringLiteral("1:500"));

    // The menu offers both managers, found by their object names.
    bool text = false;
    bool labels = false;
    for (const QAction* action : menu.actions()) {
        text = text || action->objectName() == QStringLiteral("formatTextStyles");
        labels = labels || action->objectName() == QStringLiteral("formatLabelStyles");
    }
    EXPECT_TRUE(text);
    EXPECT_TRUE(labels);
    EXPECT_EQ(workbench.showTextStyles().objectName(), QStringLiteral("textStyleManagerDialog"));
    EXPECT_EQ(workbench.showLabelStyles().objectName(), QStringLiteral("labelStyleManagerDialog"));
}
