// GIS > GDAL Toolbox (src/katana_qt/geo/gdal_toolbox_dialog.hpp): the
// catalogue searched, a form made from what GDAL declares, and the GDAL line
// it describes - driven by object names as a person fills it, with a stub
// runner in place of the window's executor.

#include <gtest/gtest.h>

#include <QCheckBox>
#include <QComboBox>
#include <QDoubleSpinBox>
#include <QGroupBox>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QRadioButton>
#include <QStringList>
#include <QTreeWidget>

#include <algorithm>
#include <string>
#include <vector>

#include "geo/argument_form.hpp"
#include "geo/gdal_toolbox_dialog.hpp"
#include "geo/geo_dialog_support.hpp"
#include "katana/gis/processing.hpp"
#include "katana/interop/reference_data.hpp"

namespace {

namespace gp = katana::gis::processing;
using katana::qt::GdalToolboxDialog;
using katana::qt::GdalToolboxForm;
using katana::qt::gdalToolboxLine;
using katana::qt::GeoDialogContext;
using katana::qt::VerbOutcome;

template <class T>
T* child(QWidget& dialog, const char* name)
{
    auto* found = dialog.findChild<T*>(name);
    EXPECT_NE(found, nullptr) << name;
    return found;
}

QStringList itemsOf(const QComboBox& choice)
{
    QStringList items;
    for (int i = 0; i < choice.count(); ++i) {
        items << choice.itemText(i);
    }
    return items;
}

struct Stub {
    QStringList lines;
    VerbOutcome answer{true, "ok", {}};
    katana::interop::ReferenceData reference;
    GeoDialogContext context()
    {
        GeoDialogContext made;
        made.run = [this](const QString& line) {
            lines << line;
            return answer;
        };
        made.headless = [] { return true; };
        made.reference = &reference;
        return made;
    }
};

TEST(GdalToolbox, SearchingContourFindsRasterContour)
{
    Stub stub;
    GdalToolboxDialog dialog(stub.context());
    child<QLineEdit>(dialog, "gdalToolboxSearch")->setText("contour");
    // The algorithm named exactly so is chosen, and shown under its group.
    EXPECT_EQ(dialog.chosen(), "raster contour");
    auto* tree = child<QTreeWidget>(dialog, "gdalToolboxTree");
    ASSERT_NE(tree->currentItem(), nullptr);
    EXPECT_EQ(tree->currentItem()->text(0), "contour");
    EXPECT_FALSE(tree->currentItem()->isHidden());
    EXPECT_EQ(tree->currentItem()->parent()->text(0), "raster");
    // Something the search does not match is hidden.
    const QList<QTreeWidgetItem*> hillshade =
        tree->findItems("hillshade", Qt::MatchExactly | Qt::MatchRecursive);
    ASSERT_FALSE(hillshade.isEmpty());
    EXPECT_TRUE(hillshade.front()->isHidden());
}

TEST(GdalToolbox, HillshadeBuildsItsFormWithBoundsAndChoices)
{
    Stub stub;
    GdalToolboxDialog dialog(stub.context());
    ASSERT_TRUE(dialog.choose("raster hillshade"));
    // GDAL's own bounds and choices, read at run time.
    const auto spec = gp::describe({"raster", "hillshade"});
    ASSERT_TRUE(spec.ok());
    auto* altitude = child<QDoubleSpinBox>(dialog, "gdalArg.altitude");
    EXPECT_EQ(altitude->maximum(), 90.0);
    auto* variant = child<QComboBox>(dialog, "gdalArg.variant");
    QStringList choices{QString()};
    for (const gp::ArgSpec& arg : spec->args) {
        if (arg.name == "variant") {
            for (const std::string& choice : arg.choices) {
                choices << QString::fromStdString(choice);
            }
        }
    }
    EXPECT_GT(choices.size(), 1);
    EXPECT_EQ(itemsOf(*variant), choices);
    // Its default is chosen, and a default is no word on the line.
    EXPECT_EQ(variant->currentText(), "regular");
    // --quiet would print instead of running: the executor refuses it, so the
    // form does not offer it; the output's format is the target's to say.
    EXPECT_EQ(dialog.findChild<QWidget*>("gdalArg.quiet"), nullptr);
    EXPECT_EQ(dialog.findChild<QWidget*>("gdalArg.output-format"), nullptr);
    EXPECT_NE(dialog.findChild<QWidget*>("gdalOutput.format"), nullptr);
}

TEST(GdalToolbox, SettingZfactorBuildsTheExactLine)
{
    Stub stub;
    katana::interop::RasterOverlay dem;
    dem.name = "ground";
    dem.source = "C:/data/ground.tif";
    ASSERT_EQ(stub.reference.add(std::move(dem)), 1u);
    GdalToolboxDialog dialog(stub.context());
    ASSERT_TRUE(dialog.choose("raster hillshade"));
    child<QDoubleSpinBox>(dialog, "gdalArg.zfactor")->setValue(2.0);
    EXPECT_EQ(child<QLineEdit>(dialog, "gdalToolboxCommand")->text(),
              "GDAL raster hillshade --zfactor=2 FROM RASTER 1 TO REFERENCE hillshade");
}

TEST(GdalToolbox, ARasterOnlyInputOffersNoDrawingSource)
{
    Stub stub;
    GdalToolboxDialog dialog(stub.context());
    ASSERT_TRUE(dialog.choose("raster hillshade"));
    const QStringList raster = itemsOf(*child<QComboBox>(dialog, "gdalInput.input.Kind"));
    EXPECT_FALSE(raster.contains("Drawing"));
    EXPECT_TRUE(raster.contains("Reference raster"));
    EXPECT_TRUE(raster.contains("Surface"));
    EXPECT_TRUE(raster.contains("File"));
    // And the other way round: features come from the drawing or a file.
    ASSERT_TRUE(dialog.choose("vector buffer"));
    const QStringList vector = itemsOf(*child<QComboBox>(dialog, "gdalInput.input.Kind"));
    EXPECT_EQ(vector, (QStringList{"Drawing", "File"}));
    // A raster result is not offered a layer, nor features a reference raster.
    EXPECT_FALSE(itemsOf(*child<QComboBox>(dialog, "gdalOutput.kind")).contains("Reference raster"));
    ASSERT_TRUE(dialog.choose("raster hillshade"));
    EXPECT_FALSE(itemsOf(*child<QComboBox>(dialog, "gdalOutput.kind")).contains("Layer"));
}

TEST(GdalToolbox, AnExclusiveMinimumIsNotEmitted)
{
    // contour --interval must be above 0 (GDAL's minimum is exclusive): 0 is
    // refused when the line is written, and nothing can run.
    Stub stub;
    GdalToolboxDialog dialog(stub.context());
    ASSERT_TRUE(dialog.choose("raster contour"));
    child<QComboBox>(dialog, "gdalInput.input.Kind")->setCurrentText("File");
    child<QLineEdit>(dialog, "gdalInput.input.File")->setText("C:/data/dem.tif");
    auto* interval = child<QDoubleSpinBox>(dialog, "gdalArg.interval");
    interval->setValue(0.0);
    auto* command = child<QLineEdit>(dialog, "gdalToolboxCommand");
    EXPECT_TRUE(command->text().isEmpty());
    EXPECT_TRUE(command->placeholderText().contains("interval must be above 0"))
        << command->placeholderText().toStdString();
    EXPECT_FALSE(child<QPushButton>(dialog, "gdalToolboxRun")->isEnabled());
    child<QPushButton>(dialog, "gdalToolboxRun")->click();
    EXPECT_TRUE(stub.lines.isEmpty());
    interval->setValue(0.5);
    EXPECT_EQ(command->text(),
              "GDAL raster contour --interval=0.5 FROM FILE C:/data/dem.tif TO LAYER gis/contour");
    // interval, levels and exp-base are one exclusion group: one given, the
    // others are not offered.
    EXPECT_FALSE(child<QLineEdit>(dialog, "gdalArg.levels")->isEnabled());
}

TEST(GdalToolbox, AConfirmAlgorithmShowsConfirmAndEmitsIt)
{
    Stub stub;
    GdalToolboxDialog dialog(stub.context());
    auto* confirm = child<QCheckBox>(dialog, "gdalToolboxConfirm");
    ASSERT_TRUE(dialog.choose("vector buffer"));
    EXPECT_FALSE(confirm->isVisibleTo(&dialog));
    ASSERT_TRUE(dialog.choose("vsi delete"));
    EXPECT_TRUE(confirm->isVisibleTo(&dialog));
    child<QLineEdit>(dialog, "gdalArg.filename")->setText("C:/tmp/old.tif");
    auto* command = child<QLineEdit>(dialog, "gdalToolboxCommand");
    // Until it is ticked there is no line to run.
    EXPECT_TRUE(command->text().isEmpty());
    EXPECT_TRUE(command->placeholderText().contains("Confirm"));
    confirm->setChecked(true);
    EXPECT_EQ(command->text(), "GDAL vsi delete --filename=C:/tmp/old.tif CONFIRM");
}

TEST(GdalToolbox, RunHandsTheRunnerExactlyTheShownLine)
{
    Stub stub;
    GdalToolboxDialog dialog(stub.context());
    ASSERT_TRUE(dialog.choose("vector buffer"));
    child<QDoubleSpinBox>(dialog, "gdalArg.distance")->setValue(1.0);
    child<QComboBox>(dialog, "gdalArg.endcap-style")->setCurrentText("flat");
    auto* command = child<QLineEdit>(dialog, "gdalToolboxCommand");
    EXPECT_EQ(command->text(),
              "GDAL vector buffer --distance=1 --endcap-style=flat FROM DRAWING TO LAYER gis/buffer");
    child<QPushButton>(dialog, "gdalToolboxRun")->click();
    ASSERT_EQ(stub.lines.size(), 1);
    EXPECT_EQ(stub.lines.front(), command->text());
    // Without distance, which buffer requires, there is nothing to run.
    child<QDoubleSpinBox>(dialog, "gdalArg.distance")
        ->setValue(child<QDoubleSpinBox>(dialog, "gdalArg.distance")->minimum());
    EXPECT_TRUE(command->placeholderText().contains("distance is required"));
}

TEST(GdalToolbox, APreviewReplyIsShown)
{
    Stub stub;
    stub.answer = VerbOutcome{true,
                              "gdal algorithm=\"vector buffer\" policy=safe preview=yes\n"
                              "preview valid=yes changed=no",
                              {}};
    GdalToolboxDialog dialog(stub.context());
    ASSERT_TRUE(dialog.choose("vector buffer"));
    child<QDoubleSpinBox>(dialog, "gdalArg.distance")->setValue(5.0);
    child<QPushButton>(dialog, "gdalToolboxPreview")->click();
    ASSERT_EQ(stub.lines.size(), 1);
    EXPECT_TRUE(stub.lines.front().endsWith(" PREVIEW"));
    EXPECT_TRUE(child<QPlainTextEdit>(dialog, "gdalToolboxReply")
                    ->toPlainText()
                    .contains("preview valid=yes changed=no"));
    EXPECT_TRUE(child<QLabel>(dialog, "gdalToolboxStatus")->text().startsWith("Previewed"));
}

TEST(GdalToolbox, AnOptionalInputIsLeftOutUntilItIsGivenAndThenNamed)
{
    Stub stub;
    GdalToolboxDialog dialog(stub.context());
    ASSERT_TRUE(dialog.choose("raster clip"));
    child<QComboBox>(dialog, "gdalInput.input.Kind")->setCurrentText("File");
    child<QLineEdit>(dialog, "gdalInput.input.File")->setText("C:/data/dem.tif");
    auto* like = child<QComboBox>(dialog, "gdalInput.like.Kind");
    EXPECT_EQ(like->currentText(), "Not given");
    auto* command = child<QLineEdit>(dialog, "gdalToolboxCommand");
    EXPECT_EQ(command->text(), "GDAL raster clip FROM FILE C:/data/dem.tif TO REFERENCE clip");
    like->setCurrentText("Drawing");
    child<QRadioButton>(dialog, "gdalInput.like.ScopeDrawing")->click();
    EXPECT_EQ(command->text(),
              "GDAL raster clip FROM FILE C:/data/dem.tif FROM like DRAWING TO REFERENCE clip");
}

TEST(GdalToolbox, SeveralDatasetsForOneArgumentEachNameIt)
{
    Stub stub;
    GdalToolboxDialog dialog(stub.context());
    ASSERT_TRUE(dialog.choose("raster mosaic"));
    child<QComboBox>(dialog, "gdalInput.input.Kind")->setCurrentText("File");
    auto* file = child<QLineEdit>(dialog, "gdalInput.input.File");
    file->setText("C:/tiles/a.tif");
    child<QPushButton>(dialog, "gdalInput.input.Add")->click();
    file->setText("C:/tiles/b.tif");
    child<QPushButton>(dialog, "gdalInput.input.Add")->click();
    EXPECT_EQ(child<QLineEdit>(dialog, "gdalToolboxCommand")->text(),
              "GDAL raster mosaic FROM input FILE C:/tiles/a.tif FROM input FILE C:/tiles/b.tif TO "
              "REFERENCE mosaic");
}

TEST(GdalToolbox, AFileOutputTakesItsFormatAndOverwrite)
{
    Stub stub;
    GdalToolboxDialog dialog(stub.context());
    ASSERT_TRUE(dialog.choose("vector buffer"));
    child<QDoubleSpinBox>(dialog, "gdalArg.distance")->setValue(1.0);
    auto* kind = child<QComboBox>(dialog, "gdalOutput.kind");
    kind->setCurrentText("File");
    auto* command = child<QLineEdit>(dialog, "gdalToolboxCommand");
    // A file must be named: the layer's default does not follow it there.
    EXPECT_TRUE(command->text().isEmpty());
    child<QLineEdit>(dialog, "gdalOutput.name")->setText("C:/out/buffer.gpkg");
    auto* format = child<QComboBox>(dialog, "gdalOutput.format");
    EXPECT_GE(format->findText("GPKG"), 0);
    format->setCurrentText("GPKG");
    child<QCheckBox>(dialog, "gdalOutput.overwrite")->setChecked(true);
    EXPECT_EQ(command->text(), "GDAL vector buffer --distance=1 FROM DRAWING TO FILE "
                               "C:/out/buffer.gpkg FORMAT GPKG OVERWRITE");
}

TEST(GdalToolboxLine, WhatTheFormsHoldIsTheLineInTheVerbsOrder)
{
    GdalToolboxForm form;
    form.path = {"vector", "layer-algebra", "intersection"};
    form.inputs = {{"input", true, true, {"LAYERS lots"}, {}},
                   {"method", false, true, {"LAYERS corridor"}, {}}};
    form.outputKind = "LAYER";
    form.outputName = "gis/overlay";
    EXPECT_EQ(*gdalToolboxLine(form), "GDAL vector layer-algebra intersection FROM LAYERS lots "
                                      "FROM method LAYERS corridor TO LAYER gis/overlay");
    form.inputs[1].sources.clear();
    auto missing = gdalToolboxLine(form);
    ASSERT_FALSE(missing.ok());
    EXPECT_NE(missing.error().message.find("method"), std::string::npos);
}

} // namespace
