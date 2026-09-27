// File > Plot to PDF and the PLOT verb (plotting/plot_drawing_dialog.hpp): the
// line's grammar, the line the dialog writes from its fields, and that Plot
// hands that line to the executor it was given. The plot itself is
// plotDrawingToPdf's, which qt_plot_headless checks through the real window.

#include <gtest/gtest.h>

#include <QCheckBox>
#include <QComboBox>
#include <QDoubleSpinBox>
#include <QDir>
#include <QFile>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QTemporaryDir>

#include <vector>

#include "plotting/plot_drawing_dialog.hpp"

namespace {

using katana::cad::PaperSize;
using katana::cad::PlotColourMode;
using katana::qt::PlotDrawingDialog;
using katana::qt::PlotDrawingDialogContext;
using katana::qt::PlotDrawingRequest;
using katana::qt::VerbOutcome;

template <typename T> T* child(QWidget& parent, const char* name)
{
    T* found = parent.findChild<T*>(QString::fromLatin1(name));
    EXPECT_NE(found, nullptr) << "no " << name;
    return found;
}

TEST(PlotDrawing, WhatIsNotGivenTakesThePlotDefaults)
{
    const auto request = katana::qt::parsePlotDrawing("PLOT site.pdf");
    ASSERT_TRUE(request.ok()) << request.error().describe();
    const katana::cad::PlotSettings defaults;
    EXPECT_EQ(request->path, "site.pdf");
    EXPECT_TRUE(request->fit);
    EXPECT_EQ(request->settings.paper, defaults.paper);
    EXPECT_EQ(request->settings.landscape, defaults.landscape);
    EXPECT_EQ(request->settings.dpi, defaults.dpi);
    EXPECT_EQ(request->settings.marginMm, defaults.marginMm);
    EXPECT_EQ(request->settings.colourMode, PlotColourMode::Colour);
    EXPECT_EQ(request->settings.lineWeightScale, 1.0);
}

TEST(PlotDrawing, EveryOptionIsRead)
{
    const auto request = katana::qt::parsePlotDrawing(
        "plot \"C:/my plots/site.pdf\" paper=a1 PORTRAIT scale=1:250 dpi=150 style=mono "
        "lineweight=0.7 margin=5");
    ASSERT_TRUE(request.ok()) << request.error().describe();
    EXPECT_EQ(request->path, "C:/my plots/site.pdf");
    EXPECT_FALSE(request->fit);
    EXPECT_EQ(request->settings.paper, PaperSize::A1);
    EXPECT_FALSE(request->settings.landscape);
    EXPECT_EQ(request->settings.scaleDenominator, 250.0);
    EXPECT_EQ(request->settings.dpi, 150.0);
    EXPECT_EQ(request->settings.colourMode, PlotColourMode::Monochrome);
    EXPECT_EQ(request->settings.lineWeightScale, 0.7);
    EXPECT_EQ(request->settings.marginMm, 5.0);
}

TEST(PlotDrawing, FitAfterAScaleFitsAgain)
{
    const auto request = katana::qt::parsePlotDrawing("PLOT a.pdf scale=500 fit");
    ASSERT_TRUE(request.ok());
    EXPECT_TRUE(request->fit);
}

TEST(PlotDrawing, WhatTheGrammarDoesNotTakeIsRefusedByItsWord)
{
    for (const char* line :
         {"PLOT", "PLOT \"\"", "PLOT a.pdf paper=B2", "PLOT a.pdf scale=0", "PLOT a.pdf scale=-5",
          "PLOT a.pdf scale=big", "PLOT a.pdf dpi=10", "PLOT a.pdf dpi=5000", "PLOT a.pdf style=sepia",
          "PLOT a.pdf lineweight=0.05", "PLOT a.pdf lineweight=9", "PLOT a.pdf margin=60",
          "PLOT a.pdf colour", "PLOT a.pdf size=A3", "PLOT \"never closed"}) {
        const auto request = katana::qt::parsePlotDrawing(line);
        EXPECT_FALSE(request.ok()) << line;
    }
    const auto paper = katana::qt::parsePlotDrawing("PLOT a.pdf paper=B2");
    EXPECT_NE(paper.error().describe().find("paper=B2"), std::string::npos)
        << paper.error().describe();
    EXPECT_NE(paper.error().describe().find("usage: PLOT"), std::string::npos);
}

TEST(PlotDrawing, TheLineItWritesIsTheLineItReads)
{
    PlotDrawingRequest request;
    // The path as the platform's file dialog gives it: "C:\plots\site plan.pdf"
    // on Windows. On POSIX '\' is part of a file name, not a separator
    // (POSIX.1-2017, 3.170), so each platform is given its own native form.
    request.path = QDir::toNativeSeparators("C:/plots/site plan.pdf");
    request.fit = false;
    request.settings.paper = PaperSize::A2;
    request.settings.landscape = false;
    request.settings.scaleDenominator = 750.0;
    request.settings.dpi = 200.0;
    request.settings.colourMode = PlotColourMode::Greyscale;
    request.settings.lineWeightScale = 1.4;
    request.settings.marginMm = 12.5;
    const QString line = katana::qt::plotDrawingCommandLine(request);
    EXPECT_EQ(line, "PLOT \"C:/plots/site plan.pdf\" paper=A2 portrait scale=750 dpi=200 "
                    "style=greyscale lineweight=1.4 margin=12.5");
    const auto read = katana::qt::parsePlotDrawing(line);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_EQ(read->path, "C:/plots/site plan.pdf");
    EXPECT_FALSE(read->fit);
    EXPECT_EQ(read->settings.paper, PaperSize::A2);
    EXPECT_EQ(read->settings.scaleDenominator, 750.0);
    EXPECT_EQ(read->settings.colourMode, PlotColourMode::Greyscale);
    EXPECT_EQ(read->settings.lineWeightScale, 1.4);
    EXPECT_EQ(read->settings.marginMm, 12.5);
}

TEST(PlotDrawingDialog, TheFieldsWriteTheLineAndPlotRunsIt)
{
    std::vector<QString> ran;
    PlotDrawingDialogContext context;
    context.run = [&ran](const QString& line) {
        ran.push_back(line);
        return VerbOutcome{true, "Plotted to out.pdf\nfile=\"out.pdf\" paper=A4", {}};
    };
    PlotDrawingDialog dialog(std::move(context));
    EXPECT_EQ(dialog.objectName(), "plotDrawingDialog");
    auto* plot = child<QPushButton>(dialog, "plotDrawingPlot");
    EXPECT_FALSE(plot->isEnabled()) << "nothing to plot to yet";
    child<QLineEdit>(dialog, "plotDrawingPath")->setText("out.pdf");
    child<QComboBox>(dialog, "plotDrawingPaper")->setCurrentText("A4");
    child<QComboBox>(dialog, "plotDrawingOrientation")->setCurrentText("Portrait");
    child<QCheckBox>(dialog, "plotDrawingFit")->setChecked(false);
    EXPECT_TRUE(child<QDoubleSpinBox>(dialog, "plotDrawingScale")->isEnabled());
    child<QDoubleSpinBox>(dialog, "plotDrawingScale")->setValue(250.0);
    child<QDoubleSpinBox>(dialog, "plotDrawingDpi")->setValue(150.0);
    child<QDoubleSpinBox>(dialog, "plotDrawingMargin")->setValue(5.0);
    child<QComboBox>(dialog, "plotDrawingColourMode")->setCurrentText("Monochrome");
    child<QDoubleSpinBox>(dialog, "plotDrawingLineWeightScale")->setValue(0.7);
    const QString expected = "PLOT \"out.pdf\" paper=A4 portrait scale=250 dpi=150 "
                             "style=monochrome lineweight=0.7 margin=5";
    EXPECT_EQ(child<QLineEdit>(dialog, "plotDrawingCommand")->text(), expected);
    ASSERT_TRUE(plot->isEnabled());
    plot->click();
    EXPECT_EQ(ran, std::vector<QString>{expected});
    EXPECT_EQ(child<QLabel>(dialog, "plotDrawingStatus")->text(),
              "Plotted: file=\"out.pdf\" paper=A4");
}

TEST(PlotDrawingDialog, FittingLeavesTheScaleOutAndARefusalIsSaid)
{
    PlotDrawingDialogContext context;
    context.run = [](const QString&) {
        return VerbOutcome{false, {}, "InvalidState: no plan viewport to plot from"};
    };
    context.suggestedPath = "site.pdf";
    PlotDrawingDialog dialog(std::move(context));
    EXPECT_FALSE(child<QDoubleSpinBox>(dialog, "plotDrawingScale")->isEnabled());
    EXPECT_EQ(child<QLineEdit>(dialog, "plotDrawingCommand")->text(),
              "PLOT \"site.pdf\" paper=A3 landscape fit dpi=300 style=colour lineweight=1 "
              "margin=10");
    dialog.plot();
    EXPECT_EQ(child<QLabel>(dialog, "plotDrawingStatus")->text(),
              "Not plotted: InvalidState: no plan viewport to plot from");
}

// Kept while the window lives, the dialog's file follows the drawing
// (suggestPath) until one is typed - it once plotted every drawing into the
// first project's folder - and a file that is there is written over only
// when the person says so.
TEST(PlotDrawingDialog, TheSuggestedFileFollowsTheDrawingUntilOneIsTypedAndAReplaceIsAsked)
{
    QTemporaryDir folder;
    ASSERT_TRUE(folder.isValid());
    const QString there = folder.filePath("there.pdf");
    QFile file(there);
    ASSERT_TRUE(file.open(QIODevice::WriteOnly));
    file.close();

    std::vector<QString> ran;
    std::vector<QString> asked;
    bool replace = false;
    PlotDrawingDialogContext context;
    context.run = [&ran](const QString& line) {
        ran.push_back(line);
        return VerbOutcome{true, "file=\"there.pdf\"", {}};
    };
    context.suggestedPath = "C:/projA/projA.pdf";
    context.confirmReplace = [&](const QString& path) {
        asked.push_back(path);
        return replace;
    };
    PlotDrawingDialog dialog(std::move(context));
    auto* path = child<QLineEdit>(dialog, "plotDrawingPath");
    dialog.suggestPath("C:/projB/projB.pdf");
    EXPECT_EQ(path->text(), "C:/projB/projB.pdf");

    path->setText(there);
    dialog.suggestPath("C:/projC/projC.pdf");
    EXPECT_EQ(path->text(), there) << "a file typed is kept";

    dialog.plot();
    EXPECT_EQ(asked, std::vector<QString>{there});
    EXPECT_TRUE(ran.empty()) << "kept when the person says no";
    EXPECT_TRUE(child<QLabel>(dialog, "plotDrawingStatus")->text().startsWith("Not plotted:"));
    replace = true;
    dialog.plot();
    EXPECT_EQ(ran.size(), 1u);
    // A file that is not there is written without a question.
    path->setText(folder.filePath("new.pdf"));
    dialog.plot();
    EXPECT_EQ(asked.size(), 2u);
    EXPECT_EQ(ran.size(), 2u);
}

TEST(PlotDrawingDialog, AHeadlessSessionIsToldToFillThePath)
{
    PlotDrawingDialogContext context;
    context.headless = [] { return true; };
    PlotDrawingDialog dialog(std::move(context));
    child<QPushButton>(dialog, "plotDrawingBrowse")->click();
    EXPECT_EQ(child<QLabel>(dialog, "plotDrawingStatus")->text(),
              "A headless session opens no file dialog: fill plotDrawingPath with the path "
              "instead.");
}

} // namespace
