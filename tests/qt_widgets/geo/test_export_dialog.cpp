// Export Drawing's dialog (src/katana_qt/gis_export_dialog.hpp): its fields
// make the EXPORT line a person would type - the shared scope first, then
// EXPORT's options - and Run and Preview hand that line to the window's
// executor. The verb itself is tested in tests/geo/test_export_options.cpp.

#include <gtest/gtest.h>

#include <QCheckBox>
#include <QComboBox>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QStringList>

#include "gis_export_dialog.hpp"

namespace {

using katana::qt::GisDialogContext;
using katana::qt::VectorExportDialog;
using katana::qt::VectorExportForm;
using katana::qt::VerbOutcome;

template <typename T>
T* child(QWidget& parent, const char* name)
{
    return parent.findChild<T*>(QString::fromLatin1(name));
}

VectorExportForm drawingForm(const QString& file)
{
    VectorExportForm form;
    form.file = file;
    form.scope.words = "DRAWING";
    return form;
}

TEST(VectorExportLine, TheOptionsFollowTheScopeAndDefaultsAreLeftOut)
{
    VectorExportForm form = drawingForm("C:/out/site lots.gpkg");
    EXPECT_EQ(*katana::qt::vectorExportLine(form), "EXPORT \"C:/out/site lots.gpkg\" DRAWING");
    form.layerName = "parcels";
    form.append = true;
    form.crs = "EPSG:4326";
    form.creationOptions = "VERSION=1.4";
    form.layerOptions = "SPATIAL_INDEX=NO  FID=fid";
    form.textAsPoints = true;
    form.curve = "0.01";
    form.properties = false;
    EXPECT_EQ(*katana::qt::vectorExportLine(form),
              "EXPORT \"C:/out/site lots.gpkg\" DRAWING layername=parcels append crs=EPSG:4326 "
              "co=VERSION=1.4 lco=SPATIAL_INDEX=NO lco=FID=fid text=points curve=0.01 "
              "properties=no");
    // project is the verb's default, and not said.
    form = drawingForm("C:/out/a.gpkg");
    form.crs = "project";
    EXPECT_EQ(*katana::qt::vectorExportLine(form), "EXPORT \"C:/out/a.gpkg\" DRAWING");
}

TEST(VectorExportLine, WhatALineCannotSayHasNoLine)
{
    VectorExportForm form = drawingForm("C:/out/a.gpkg");
    form.split = true;
    form.layerName = "one";
    EXPECT_FALSE(katana::qt::vectorExportLine(form).ok()); // split names each layer itself
    form = drawingForm("C:/out/a.gpkg");
    form.creationOptions = "VERSION";
    EXPECT_FALSE(katana::qt::vectorExportLine(form).ok()); // not KEY=VALUE
    form = drawingForm("C:/out/a.gpkg");
    form.curve = "0";
    EXPECT_FALSE(katana::qt::vectorExportLine(form).ok()); // a chord's stray is above 0
    form = drawingForm("");
    EXPECT_FALSE(katana::qt::vectorExportLine(form).ok()); // no file
    form = drawingForm("C:/out/a.gpkg");
    form.scope.words.clear();
    EXPECT_FALSE(katana::qt::vectorExportLine(form).ok()); // no scope
}

TEST(VectorExportLine, ADxfTakesTheScopeAlone)
{
    VectorExportForm form = drawingForm("C:/out/site.dxf");
    form.layerName = "parcels";
    form.textAsPoints = true;
    EXPECT_EQ(*katana::qt::vectorExportLine(form), "EXPORT \"C:/out/site.dxf\" DRAWING");
    EXPECT_TRUE(katana::qt::exportIsNative("C:/out/site.12da"));
    EXPECT_FALSE(katana::qt::exportIsNative("C:/out/site.gpkg"));
}

TEST(VectorExportDialog, TheFieldsBuildExactlyTheTypedLineAndRunHandsItOver)
{
    QStringList ran;
    GisDialogContext context;
    context.run = [&ran](const QString& line) {
        ran << line;
        return VerbOutcome{true,
                           line.endsWith(" PREVIEW")
                               ? "export file=\"C:/out/site.gpkg\" kind=vector driver=GPKG "
                                 "preview=yes entities=4"
                               : "exported file=\"C:/out/site.gpkg\" kind=vector driver=GPKG "
                                 "features=4 skipped=0 layers=parcels crs=",
                           QString()};
    };
    VectorExportDialog dialog(std::move(context));
    EXPECT_EQ(dialog.objectName(), "vectorExportDialog");
    // File > Export calls its item "Export Drawing...", and the window it opens says the same.
    EXPECT_EQ(dialog.windowTitle(), "Export Drawing");
    dialog.setFile("C:/out/site.gpkg");
    // The whole drawing is where an export starts.
    EXPECT_EQ(child<QLineEdit>(dialog, "vectorExportCommand")->text(),
              "EXPORT \"C:/out/site.gpkg\" DRAWING");
    child<QLineEdit>(dialog, "vectorExportLayerName")->setText("parcels");
    child<QComboBox>(dialog, "vectorExportCrs")->setCurrentText("native");
    child<QLineEdit>(dialog, "vectorExportLayerOptions")->setText("SPATIAL_INDEX=NO");
    auto* text = child<QComboBox>(dialog, "vectorExportText");
    text->setCurrentIndex(text->findData(QString("points")));
    child<QCheckBox>(dialog, "vectorExportProperties")->setChecked(false);
    const QString expected = "EXPORT \"C:/out/site.gpkg\" DRAWING layername=parcels crs=native "
                             "lco=SPATIAL_INDEX=NO text=points properties=no";
    EXPECT_EQ(child<QLineEdit>(dialog, "vectorExportCommand")->text(), expected);

    child<QPushButton>(dialog, "vectorExportPreview")->click();
    child<QPushButton>(dialog, "vectorExportRun")->click();
    ASSERT_EQ(ran.size(), 2);
    EXPECT_EQ(ran[0], expected + " PREVIEW");
    EXPECT_EQ(ran[1], expected);
    EXPECT_EQ(dialog.statusText(), "Wrote 4 features to C:/out/site.gpkg.");
}

TEST(VectorExportDialog, ADxfTurnsTheGdalFieldsOff)
{
    VectorExportDialog dialog(GisDialogContext{});
    dialog.setFile("C:/out/site.dxf");
    EXPECT_FALSE(child<QWidget>(dialog, "vectorExportCrs")->isEnabled());
    EXPECT_FALSE(child<QWidget>(dialog, "vectorExportSplit")->isEnabled());
    EXPECT_EQ(child<QLineEdit>(dialog, "vectorExportCommand")->text(),
              "EXPORT \"C:/out/site.dxf\" DRAWING");
    dialog.setFile("C:/out/site.gpkg");
    EXPECT_TRUE(child<QWidget>(dialog, "vectorExportCrs")->isEnabled());
    // One file layer per drawing layer names each: the layer name goes off.
    child<QCheckBox>(dialog, "vectorExportSplit")->setChecked(true);
    EXPECT_FALSE(child<QWidget>(dialog, "vectorExportLayerName")->isEnabled());
    EXPECT_EQ(child<QLineEdit>(dialog, "vectorExportCommand")->text(),
              "EXPORT \"C:/out/site.gpkg\" DRAWING split=layer");
}

} // namespace
