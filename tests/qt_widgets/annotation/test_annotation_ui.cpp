// The annotation front end (src/katana_qt/annotation/, docs/annotation.md
// "In the window"): the painter's paper sizes and marks, the text and label
// style managers, and the annotation scale box - each a thin front end over
// the verbs, so each edit is checked as the line it runs and one undo step.
// The managers' runner is the interpreter itself, so a line is checked by what
// the verb makes of it.

#include <gtest/gtest.h>

#include <QCheckBox>
#include <QComboBox>
#include <QCoreApplication>
#include <QDoubleSpinBox>
#include <QImage>
#include <QLineEdit>
#include <QMainWindow>
#include <QMenu>
#include <QPainter>
#include <QTableWidget>
#include <QToolBar>

#include "annotation/annotation_managers.hpp"
#include "annotation/annotation_workbench.hpp"
#include "katana/cad/command_interpreter.hpp"
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

// Runs each line through an interpreter over the document, as the window's
// executor runs it through the same interpreter, and keeps the lines.
struct Runner {
    explicit Runner(Document& document) : interpreter(document) {}
    katana::cad::CommandInterpreter interpreter;
    QStringList lines;

    katana::qt::CommandRunner runner()
    {
        return [this](const QString& line) {
            lines << line;
            const auto reply = interpreter.run(line.toStdString());
            if (!reply) {
                return katana::qt::VerbOutcome{false, {},
                                               QString::fromStdString(reply.error().describe())};
            }
            return katana::qt::VerbOutcome{true, QString::fromStdString(*reply), {}};
        };
    }
    void ok(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        ASSERT_TRUE(reply.ok()) << line << ": " << reply.error().describe();
    }
};

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
    Runner run(document);
    katana::qt::TextStyleManagerDialog dialog(document, run.runner());
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
    Runner run(document);
    katana::qt::LabelStyleManagerDialog dialog(document, run.runner());
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
    EXPECT_TRUE(dialog.runReport().startsWith(QStringLiteral("autolabel created=1")))
        << dialog.runReport().toStdString();
    ASSERT_TRUE(dialog.clearRules());
    EXPECT_EQ(dialog.runReport(), QStringLiteral("removed=1"));
}

TEST(AnnotationManagers, NewTakesTheNameTypedAndApplySendsOnlyWhatChanged)
{
    Document document;
    Runner run(document);
    katana::qt::TextStyleManagerDialog dialog(document, run.runner());
    auto* name = dialog.findChild<QLineEdit*>(QStringLiteral("textStyleNewName"));
    ASSERT_NE(name, nullptr);
    EXPECT_EQ(name->placeholderText(), QStringLiteral("Text Style"));
    name->setText(QStringLiteral("Road Names"));
    ASSERT_TRUE(dialog.addStyle()) << dialog.problem().toStdString();
    EXPECT_EQ(run.lines.back(), QStringLiteral("TEXTSTYLE NEW \"Road Names\""));
    EXPECT_TRUE(document.model().textStyles.contains("Road Names"));
    EXPECT_TRUE(name->text().isEmpty());

    dialog.findChild<QDoubleSpinBox*>(QStringLiteral("textStylePaperHeight"))->setValue(3.5);
    dialog.findChild<QCheckBox*>(QStringLiteral("textStyleBold"))->setChecked(true);
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    EXPECT_EQ(run.lines.back(), QStringLiteral("TEXTSTYLE SET \"Road Names\" paper=3.5 bold=on"));
    EXPECT_DOUBLE_EQ(document.model().textStyles.find("Road Names")->paperHeight, 3.5);

    // A name a line cannot carry is refused before anything runs.
    const auto lines = run.lines.size();
    name->setText(QStringLiteral("say \"hi\""));
    EXPECT_FALSE(dialog.addStyle());
    EXPECT_EQ(run.lines.size(), lines);
    EXPECT_TRUE(dialog.problem().contains(QStringLiteral("double quote")));
}

TEST(AnnotationManagers, ANewLabelStyleHasTheNameAndKindChosen)
{
    Document document;
    Runner run(document);
    katana::qt::LabelStyleManagerDialog dialog(document, run.runner());
    dialog.findChild<QLineEdit*>(QStringLiteral("labelStyleNewName"))
        ->setText(QStringLiteral("Lot areas"));
    auto* kind = dialog.findChild<QComboBox*>(QStringLiteral("labelStyleNewKind"));
    kind->setCurrentIndex(kind->findText(QStringLiteral("area")));
    ASSERT_TRUE(dialog.addStyle()) << dialog.problem().toStdString();
    EXPECT_EQ(run.lines.back(), QStringLiteral("LABELSTYLE NEW \"Lot areas\" kind=area"));
    const auto* made = document.model().labelStyles.find("Lot areas");
    ASSERT_NE(made, nullptr);
    EXPECT_EQ(made->kind, katana::entity::LabelKind::Area);
    // LABELSTYLE NEW's own start for an area: at the centroid.
    EXPECT_EQ(made->placement, katana::entity::LabelPlacement::Centroid);
    EXPECT_EQ(dialog.findChild<QComboBox*>(QStringLiteral("labelStyleKind"))->currentText(),
              QStringLiteral("area"))
        << "the new style is the one shown";
}

TEST(AnnotationManagers, TheInsertValueMenuOffersWhatTheKindTakesWithItsSteps)
{
    Document document;
    Runner run(document);
    katana::qt::LabelStyleManagerDialog dialog(document, run.runner());
    ASSERT_TRUE(dialog.addDefaults());
    dialog.select(QStringLiteral("Bearing Distance"));
    const auto action = [&](const char* name) {
        return dialog.findChild<QAction*>(QString::fromLatin1(name));
    };
    // A segment's bearing in DMS and its distance to three places; a
    // segment has no area, and a bearing no square metres.
    EXPECT_NE(action("labelValue:bearing:dms"), nullptr);
    EXPECT_NE(action("labelValue:distance:.3f"), nullptr);
    EXPECT_NE(action("labelValue:layer:upper"), nullptr);
    EXPECT_EQ(action("labelValue:area"), nullptr);
    EXPECT_EQ(action("labelValue:bearing:m2"), nullptr);

    auto* templateEdit = dialog.findChild<QLineEdit*>(QStringLiteral("labelStyleTemplate"));
    templateEdit->clear();
    action("labelValue:bearing:dms")->trigger();
    templateEdit->insert(QStringLiteral(" "));
    dialog.insertValue(QStringLiteral("distance:.2f"));
    EXPECT_EQ(templateEdit->text(), QStringLiteral("{bearing:dms} {distance:.2f}"));
    dialog.insertValue(QStringLiteral("prop.NAME"));
    EXPECT_EQ(templateEdit->selectedText(), QStringLiteral("NAME")) << "left to type over";
    templateEdit->insert(QStringLiteral("owner"));
    EXPECT_EQ(templateEdit->text(), QStringLiteral("{bearing:dms} {distance:.2f}{prop.owner}"));

    // Another kind, other values: a point's level.
    dialog.select(QStringLiteral("Spot Level"));
    EXPECT_NE(action("labelValue:z:.3f"), nullptr);
    EXPECT_EQ(action("labelValue:bearing"), nullptr);
}

TEST(AnnotationManagers, RulesAreAddedWithTheirTypeEditedAndSwitchedByLines)
{
    Document document;
    Runner run(document);
    katana::qt::LabelStyleManagerDialog dialog(document, run.runner());
    ASSERT_TRUE(dialog.addDefaults());
    dialog.findChild<QLineEdit*>(QStringLiteral("labelRuleName"))->setText(QStringLiteral("lots"));
    auto* style = dialog.findChild<QComboBox*>(QStringLiteral("labelRuleStyle"));
    style->setCurrentIndex(style->findText(QStringLiteral("Lot Area")));
    auto* type = dialog.findChild<QComboBox*>(QStringLiteral("labelRuleType"));
    ASSERT_NE(type, nullptr);
    type->setCurrentIndex(type->findText(QStringLiteral("Polyline")));
    ASSERT_TRUE(dialog.addRule()) << dialog.problem().toStdString();
    EXPECT_EQ(run.lines.back(),
              QStringLiteral("AUTOLABEL RULE ADD lots style=\"Lot Area\" type=Polyline enabled=on"));
    EXPECT_EQ(document.model().labelRules.find("lots")->entityType, "Polyline");

    // Chosen in the table, the rule fills the form; Update stores it whole.
    auto* table = dialog.findChild<QTableWidget*>(QStringLiteral("labelRuleTable"));
    ASSERT_EQ(table->rowCount(), 1);
    dialog.findChild<QLineEdit*>(QStringLiteral("labelRuleName"))->clear();
    table->setCurrentCell(0, 1);
    EXPECT_EQ(dialog.findChild<QLineEdit*>(QStringLiteral("labelRuleName"))->text(),
              QStringLiteral("lots"));
    dialog.findChild<QLineEdit*>(QStringLiteral("labelRuleLayer"))->setText(QStringLiteral("BDY*"));
    ASSERT_TRUE(dialog.updateRule()) << dialog.problem().toStdString();
    EXPECT_EQ(run.lines.back(),
              QStringLiteral("AUTOLABEL RULE SET lots style=\"Lot Area\" layer=BDY* code=\"\" "
                             "type=Polyline labellayer=\"\" enabled=on"));
    EXPECT_EQ(document.model().labelRules.find("lots")->layer, "BDY*");

    // The table's Enabled box switches the rule with a line of its own.
    const std::size_t before = document.history().undoCount();
    table->item(0, 6)->setCheckState(Qt::Unchecked);
    QCoreApplication::processEvents(); // run once the box's signal has returned
    EXPECT_EQ(run.lines.back(), QStringLiteral("AUTOLABEL RULE SET lots enabled=off"));
    EXPECT_FALSE(document.model().labelRules.find("lots")->enabled);
    EXPECT_EQ(document.history().undoCount(), before + 1);
    EXPECT_EQ(table->item(0, 6)->checkState(), Qt::Unchecked) << "the table shows it";
}

TEST(AnnotationManagers, RunPreviewAndClearActOnTheChosenRulesAndCountTheirLabels)
{
    Document document;
    Runner run(document);
    katana::qt::LabelStyleManagerDialog dialog(document, run.runner());
    ASSERT_TRUE(dialog.addDefaults());
    run.ok("RECT 0,0 20,20");
    run.ok("AUTOLABEL RULE ADD lots style=\"Lot Area\"");
    run.ok("AUTOLABEL RULE ADD sides style=\"Bearing Distance\"");
    auto* table = dialog.findChild<QTableWidget*>(QStringLiteral("labelRuleTable"));
    ASSERT_EQ(table->rowCount(), 2);
    // Rules are listed by name: lots, then sides.
    table->selectRow(0);
    EXPECT_EQ(dialog.chosenRules(), QStringList{QStringLiteral("lots")});

    // One label entity a target a rule matches (auto_label.hpp): the
    // rectangle once for its area.
    const std::size_t entities = document.model().entities.size();
    ASSERT_TRUE(dialog.previewRules()) << dialog.problem().toStdString();
    EXPECT_EQ(run.lines.back(), QStringLiteral("AUTOLABEL PREVIEW lots"));
    EXPECT_EQ(dialog.runReport(), QStringLiteral("preview created=1 kept=0 removed=0 skipped=0"));
    EXPECT_EQ(document.model().entities.size(), entities) << "a preview makes nothing";
    EXPECT_EQ(table->item(0, 7)->text(), QStringLiteral("1")) << "the Labels column";

    ASSERT_TRUE(dialog.runRules());
    EXPECT_EQ(run.lines.back(), QStringLiteral("AUTOLABEL RUN lots"));
    EXPECT_EQ(document.model().entities.size(), entities + 1) << "only the chosen rule ran";

    // None chosen: every enabled rule. The area label is kept, the sides'
    // made - one label for the rectangle's four segments.
    table->clearSelection();
    ASSERT_TRUE(dialog.runRules());
    EXPECT_EQ(run.lines.back(), QStringLiteral("AUTOLABEL RUN"));
    EXPECT_EQ(dialog.runReport(), QStringLiteral("autolabel created=1 kept=1 removed=0 skipped=0"));
    EXPECT_EQ(table->item(1, 7)->text(), QStringLiteral("1"));

    table->selectRow(1);
    ASSERT_TRUE(dialog.clearRules());
    EXPECT_EQ(run.lines.back(), QStringLiteral("AUTOLABEL CLEAR sides"));
    EXPECT_EQ(dialog.runReport(), QStringLiteral("removed=1"));
    EXPECT_EQ(document.model().entities.size(), entities + 1);
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
