// The Dimension Styles manager (Format > Dimension Styles...,
// src/katana_qt/annotation/dimension_style_manager.hpp), driven by object name
// as a person drives it and checked by the lines it runs: every edit is a
// DIMSTYLE line through the command runner, and each button is ONE undo step.
// The runner here is the interpreter itself, so a line is checked by what the
// verb makes of it, not by what the dialog meant.

#include <gtest/gtest.h>

#include <QCheckBox>
#include <QComboBox>
#include <QImage>
#include <QLabel>
#include <QLineEdit>
#include <QMainWindow>
#include <QMenu>
#include <QPixmap>
#include <QSpinBox>
#include <QTableWidget>
#include <QToolBar>

#include "annotation/annotation_workbench.hpp"
#include "annotation/dimension_style_manager.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::entity::ArrowHead;
using katana::entity::DimensionStyle;
using katana::qt::DimensionStyleManagerDialog;
using katana::qt::VerbOutcome;

namespace {

// Runs each line through an interpreter over the document, as the window's
// executor runs it through the same interpreter, and keeps the lines.
struct Runner {
    explicit Runner(Document& document) : interpreter(document) {}
    CommandInterpreter interpreter;
    QStringList lines;

    katana::qt::CommandRunner runner()
    {
        return [this](const QString& line) {
            lines << line;
            const auto reply = interpreter.run(line.toStdString());
            if (!reply) {
                return VerbOutcome{false, {}, QString::fromStdString(reply.error().describe())};
            }
            return VerbOutcome{true, QString::fromStdString(*reply), {}};
        };
    }
};

template <typename Widget> Widget* child(const QWidget& dialog, const char* name)
{
    auto* widget = dialog.findChild<Widget*>(QString::fromLatin1(name));
    EXPECT_NE(widget, nullptr) << name;
    return widget;
}

void type(const QWidget& dialog, const char* name, const QString& text)
{
    if (auto* line = child<QLineEdit>(dialog, name)) {
        line->setText(text);
    }
}

void tick(const QWidget& dialog, const char* name, bool on)
{
    if (auto* box = child<QCheckBox>(dialog, name)) {
        box->setChecked(on);
    }
}

const DimensionStyle& stored(const Document& document, const char* name)
{
    return *document.model().dimensionStyles.find(name);
}

} // namespace

TEST(DimensionStyleManager, EveryFieldIsDrivenByNameAndApplyIsOneLineAndOneStep)
{
    Document document;
    Runner run(document);
    DimensionStyleManagerDialog dialog(document, run.runner());
    EXPECT_EQ(dialog.objectName(), QStringLiteral("dimensionStyleManagerDialog"));
    EXPECT_EQ(dialog.current(), QStringLiteral("Standard"));

    type(dialog, "dimStyleText", QStringLiteral("3.5"));
    type(dialog, "dimStyleGap", QStringLiteral("0.8"));
    type(dialog, "dimStylePrefix", QStringLiteral("L= "));
    type(dialog, "dimStyleSuffix", QStringLiteral("m"));
    type(dialog, "dimStyleExtOff", QStringLiteral("1"));
    type(dialog, "dimStyleExtBeyond", QStringLiteral("2"));
    type(dialog, "dimStyleArrow", QStringLiteral("3"));
    auto* head = child<QComboBox>(dialog, "dimStyleHead");
    head->setCurrentIndex(head->findText(QStringLiteral("Tick")));
    type(dialog, "dimStyleScale", QStringLiteral("1000"));
    child<QSpinBox>(dialog, "dimStyleDecimals")->setValue(1);
    type(dialog, "dimStyleRound", QStringLiteral("0.5"));
    tick(dialog, "dimStyleTrim", true);
    tick(dialog, "dimStylePaper", true);

    const std::size_t before = document.history().undoCount();
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    ASSERT_EQ(run.lines.size(), 1);
    EXPECT_EQ(run.lines.front(),
              QStringLiteral("DIMSTYLE SET Standard TEXT 3.5 GAP 0.8 EXTOFF 1 EXTBEYOND 2 ARROW 3 "
                             "HEAD Tick SCALE 1000 DECIMALS 1 ROUND 0.5 PREFIX \"L= \" SUFFIX m "
                             "TRIM on PAPER on"));
    EXPECT_EQ(document.history().undoCount(), before + 1) << "thirteen fields, one step";
    const DimensionStyle& standard = stored(document, "Standard");
    EXPECT_DOUBLE_EQ(standard.textHeight, 3.5);
    EXPECT_DOUBLE_EQ(standard.textGap, 0.8);
    EXPECT_EQ(standard.prefix, "L= ");
    EXPECT_EQ(standard.suffix, "m");
    EXPECT_DOUBLE_EQ(standard.extensionOffset, 1.0);
    EXPECT_DOUBLE_EQ(standard.extensionBeyond, 2.0);
    EXPECT_DOUBLE_EQ(standard.arrowSize, 3.0);
    EXPECT_EQ(standard.arrowHead, ArrowHead::Tick);
    EXPECT_DOUBLE_EQ(standard.unitScale, 1000.0);
    EXPECT_EQ(standard.decimals, 1);
    EXPECT_DOUBLE_EQ(standard.roundTo, 0.5);
    EXPECT_TRUE(standard.suppressTrailingZeros);
    EXPECT_TRUE(standard.paperSized);

    EXPECT_TRUE(dialog.apply()) << "an unchanged form";
    EXPECT_EQ(run.lines.size(), 1) << "runs no line";
    EXPECT_EQ(document.history().undoCount(), before + 1) << "and makes no step";

    // One undo takes every field back, and the form follows.
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(stored(document, "Standard"), DimensionStyle{});
    EXPECT_EQ(child<QLineEdit>(dialog, "dimStyleText")->text(), QStringLiteral("2.5"));
}

TEST(DimensionStyleManager, NewAndDuplicateNameTheStyleAndEachIsOneStep)
{
    Document document;
    Runner run(document);
    DimensionStyleManagerDialog dialog(document, run.runner());
    auto* name = child<QLineEdit>(dialog, "dimStyleNewName");
    EXPECT_EQ(name->placeholderText(), QStringLiteral("Dimension Style"))
        << "a blank name gives the fresh one shown";

    name->setText(QStringLiteral("site"));
    ASSERT_TRUE(dialog.newStyle()) << dialog.problem().toStdString();
    EXPECT_EQ(run.lines.back(), QStringLiteral("DIMSTYLE NEW site"));
    EXPECT_EQ(dialog.current(), QStringLiteral("site"));
    EXPECT_TRUE(name->text().isEmpty());

    // Duplicate makes a style of what the form shows, its edits included,
    // in one step: NEW with the fields that differ from a new style's.
    type(dialog, "dimStyleText", QStringLiteral("5"));
    tick(dialog, "dimStylePaper", true);
    name->setText(QStringLiteral("site wide"));
    const std::size_t before = document.history().undoCount();
    ASSERT_TRUE(dialog.duplicateStyle()) << dialog.problem().toStdString();
    EXPECT_EQ(run.lines.back(), QStringLiteral("DIMSTYLE NEW \"site wide\" TEXT 5 PAPER on"));
    EXPECT_EQ(document.history().undoCount(), before + 1);
    EXPECT_DOUBLE_EQ(stored(document, "site wide").textHeight, 5.0);
    EXPECT_TRUE(stored(document, "site wide").paperSized);
    EXPECT_DOUBLE_EQ(stored(document, "site").textHeight, 2.5) << "the original is untouched";
    EXPECT_EQ(dialog.current(), QStringLiteral("site wide"));

    // Blank names count on.
    ASSERT_TRUE(dialog.newStyle());
    ASSERT_TRUE(dialog.newStyle());
    EXPECT_TRUE(document.model().dimensionStyles.contains("Dimension Style"));
    EXPECT_TRUE(document.model().dimensionStyles.contains("Dimension Style 2"));

    // A name a line cannot carry is refused before anything runs.
    const auto lines = run.lines.size();
    name->setText(QStringLiteral("5\" arrows"));
    EXPECT_FALSE(dialog.newStyle());
    EXPECT_EQ(run.lines.size(), lines);
    EXPECT_TRUE(dialog.problem().contains(QStringLiteral("double quote")))
        << dialog.problem().toStdString();
}

TEST(DimensionStyleManager, TheListSaysWhoUsesEachStyleAndDeleteIsRefusedWhileOneDoes)
{
    Document document;
    Runner run(document);
    ASSERT_TRUE(run.interpreter.run("DIMSTYLE NEW site").ok());
    ASSERT_TRUE(run.interpreter.run("DIMSTYLE NEW spare").ok());
    ASSERT_TRUE(run.interpreter.run("LAYER NEW dims").ok());
    ASSERT_TRUE(run.interpreter.run("LAYER DIMSTYLE dims site").ok());
    DimensionStyleManagerDialog dialog(document, run.runner());
    auto* list = child<QTableWidget>(dialog, "dimStyleList");
    ASSERT_EQ(list->rowCount(), 3);
    QStringList rows;
    for (int row = 0; row < list->rowCount(); ++row) {
        rows << list->item(row, 0)->text() + QStringLiteral(": ") + list->item(row, 1)->text();
    }
    EXPECT_EQ(rows, (QStringList{QStringLiteral("Standard: no layer (the default)"),
                                 QStringLiteral("site: 1 layer"),
                                 QStringLiteral("spare: no layer")}));

    dialog.select(QStringLiteral("site"));
    const std::size_t before = document.history().undoCount();
    EXPECT_FALSE(dialog.deleteStyle());
    EXPECT_TRUE(dialog.problem().contains(QStringLiteral("dims")))
        << dialog.problem().toStdString();
    EXPECT_TRUE(document.model().dimensionStyles.contains("site"));
    EXPECT_EQ(document.history().undoCount(), before);

    // A click on a row shows that style.
    list->setCurrentCell(2, 0);
    EXPECT_EQ(dialog.current(), QStringLiteral("spare"));
    ASSERT_TRUE(dialog.deleteStyle()) << dialog.problem().toStdString();
    EXPECT_EQ(run.lines.back(), QStringLiteral("DIMSTYLE DELETE spare"));
    EXPECT_FALSE(document.model().dimensionStyles.contains("spare"));
    EXPECT_EQ(dialog.current(), QStringLiteral("Standard"));
    EXPECT_TRUE(dialog.problem().isEmpty());
}

TEST(DimensionStyleManager, ThePreviewReadsTheFormAndTheSampleIsDrawn)
{
    Document document;
    Runner run(document);
    DimensionStyleManagerDialog dialog(document, run.runner());
    EXPECT_EQ(dialog.preview(), QStringLiteral("a dimension of 10 reads as 10.000"));
    const QPixmap standard = child<QLabel>(dialog, "dimStyleSample")->pixmap();
    ASSERT_FALSE(standard.isNull());
    // Some ink: the painter drew the dimension on the transparent sample.
    const QImage image = standard.toImage();
    int inked = 0;
    for (int y = 0; y < image.height(); ++y) {
        for (int x = 0; x < image.width(); ++x) {
            inked += qAlpha(image.pixel(x, y)) > 0 ? 1 : 0;
        }
    }
    EXPECT_GT(inked, 50);

    // A drawing in metres dimensioned in millimetres, as DIMSTYLE LIST reads it
    // (CadInterpreter.DimensionStylesCanBeDefinedTunedAndAttachedToALayer).
    type(dialog, "dimStyleScale", QStringLiteral("1000"));
    child<QSpinBox>(dialog, "dimStyleDecimals")->setValue(1);
    type(dialog, "dimStyleSuffix", QStringLiteral("mm"));
    EXPECT_EQ(dialog.preview(), QStringLiteral("a dimension of 10 reads as 10000.0mm"));

    // A field that does not read says which, and Apply runs nothing.
    type(dialog, "dimStyleText", QStringLiteral("tall"));
    EXPECT_TRUE(dialog.preview().contains(QStringLiteral("text height must be a number")))
        << dialog.preview().toStdString();
    EXPECT_FALSE(dialog.apply());
    EXPECT_TRUE(run.lines.isEmpty());
    EXPECT_TRUE(dialog.problem().contains(QStringLiteral("tall")));
    // One the verb refuses is refused in the verb's words.
    type(dialog, "dimStyleText", QStringLiteral("0"));
    EXPECT_FALSE(dialog.apply());
    EXPECT_EQ(run.lines.size(), 1);
    EXPECT_TRUE(dialog.problem().contains(QStringLiteral("text height must be finite")))
        << dialog.problem().toStdString();
}

TEST(DimensionStyleManager, RevertDropsTheEditsAndAChangeMadeElsewhereShowsAtOnce)
{
    Document document;
    Runner run(document);
    ASSERT_TRUE(run.interpreter.run("DIMSTYLE NEW site").ok());
    DimensionStyleManagerDialog dialog(document, run.runner());
    dialog.select(QStringLiteral("site"));
    auto* text = child<QLineEdit>(dialog, "dimStyleText");
    text->setText(QStringLiteral("9"));
    dialog.revert();
    EXPECT_EQ(text->text(), QStringLiteral("2.5"));

    // An edit is kept through a change that leaves its style alone (a layer
    // typed elsewhere), and replaced by one that changes it.
    text->setText(QStringLiteral("9"));
    ASSERT_TRUE(run.interpreter.run("LAYER NEW other").ok());
    EXPECT_EQ(text->text(), QStringLiteral("9"));
    ASSERT_TRUE(run.interpreter.run("DIMSTYLE SET site TEXT 4 HEAD Dot").ok());
    EXPECT_EQ(text->text(), QStringLiteral("4"));
    EXPECT_EQ(child<QComboBox>(dialog, "dimStyleHead")->currentText(), QStringLiteral("Dot"));
}

TEST(DimensionStyleManager, TheFormatMenuOpensItByName)
{
    Document document;
    QMainWindow window;
    QMenu menu;
    QToolBar toolBar;
    katana::qt::AnnotationWorkbench workbench(window, document, menu, toolBar);
    Runner run(document);
    workbench.setCommandRunner(run.runner());
    const QAction* action = nullptr;
    for (const QAction* item : menu.actions()) {
        if (item->objectName() == QStringLiteral("formatDimensionStyles")) {
            action = item;
        }
    }
    ASSERT_NE(action, nullptr);
    EXPECT_EQ(action->data().toString(), QStringLiteral("dimensionStyleManagerDialog"))
        << "what the headless --dialog opens it by";
    DimensionStyleManagerDialog& dialog = workbench.showDimensionStyles();
    EXPECT_EQ(dialog.objectName(), QStringLiteral("dimensionStyleManagerDialog"));
    // Its lines go through the runner the window set.
    child<QLineEdit>(dialog, "dimStyleNewName")->setText(QStringLiteral("site"));
    ASSERT_TRUE(dialog.newStyle()) << dialog.problem().toStdString();
    EXPECT_EQ(run.lines, QStringList{QStringLiteral("DIMSTYLE NEW site")});
}
