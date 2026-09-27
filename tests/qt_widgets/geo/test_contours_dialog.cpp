// Terrain > Analysis > Contours (src/katana_qt/geo/contours_dialog.hpp): the
// line the fields describe, and the dialog driven by its object names as a
// person clicks it, running exactly the line it shows through the runner it
// was given.

#include <gtest/gtest.h>

#include <QCheckBox>
#include <QComboBox>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QSpinBox>
#include <QStringList>

#include <memory>

#include "geo/contours_dialog.hpp"
#include "katana/cad/document.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_builder.hpp"

namespace {

using katana::qt::ContourForm;
using katana::qt::ContoursDialog;
using katana::qt::TerrainDialogContext;
using katana::qt::VerbOutcome;

// A runner that keeps the lines and answers each as a headless run does.
struct Recorder {
    QStringList lines;
    TerrainDialogContext context(katana::cad::Document* document,
                                 const katana::interop::ReferenceData* reference,
                                 const katana::terrain::SurfaceStore* surfaces)
    {
        TerrainDialogContext made;
        made.run = [this](const QString& line) {
            lines << line;
            return VerbOutcome{true, "ran " + line, {}};
        };
        made.document = document;
        made.reference = reference;
        made.surfaces = surfaces;
        return made;
    }
};

std::shared_ptr<const katana::terrain::TinSurface> square()
{
    katana::terrain::TinInput input;
    input.points = {{0, 0, 1}, {10, 0, 2}, {10, 10, 3}, {0, 10, 2}};
    auto built = katana::terrain::buildTin(input);
    EXPECT_TRUE(built.ok());
    return std::make_shared<const katana::terrain::TinSurface>(built->surface);
}

TEST(ContourLine, TheFieldsWriteTheLineAndABlankIsLeftOut)
{
    ContourForm form;
    form.source = "RASTER 3";
    form.interval = "0.5";
    form.majorEvery = 2;
    form.base = "100";
    form.layer = "site contours";
    form.smooth = 5;
    EXPECT_EQ(*katana::qt::contourLine(form),
              "CONTOUR RASTER 3 interval=0.5 major=2 base=100 layer=\"site contours\" smooth=5");
    form.base.clear();
    form.layer.clear();
    form.clip = true;
    form.scope = "LAYERS lots WHERE TYPE=polyline";
    EXPECT_EQ(*katana::qt::contourLine(form),
              "CONTOUR RASTER 3 interval=0.5 major=2 smooth=5 LAYERS lots WHERE TYPE=polyline");
    // A surface is traced exactly: no smoothing is written for one.
    form.source = "SURFACE ground";
    form.clip = false;
    EXPECT_EQ(*katana::qt::contourLine(form), "CONTOUR SURFACE ground interval=0.5 major=2");
}

TEST(ContourLine, WhatCannotBeWrittenIsRefusedNamingTheField)
{
    ContourForm form;
    form.interval = "1";
    EXPECT_NE(katana::qt::contourLine(form).error().message.find("Source"), std::string::npos);
    form.source = "SURFACE ground";
    for (const char* interval : {"", "0", "-1", "one"}) {
        form.interval = interval;
        EXPECT_NE(katana::qt::contourLine(form).error().message.find("Interval"),
                  std::string::npos)
            << interval;
    }
    form.interval = "1";
    form.base = "sea";
    EXPECT_NE(katana::qt::contourLine(form).error().message.find("Base"), std::string::npos);
    form.base.clear();
    form.clip = true;
    form.scopeError = "no layer is ticked";
    EXPECT_NE(katana::qt::contourLine(form).error().message.find("Keep inside"),
              std::string::npos);
}

TEST(ContoursDialog, ASurfaceIsChosenAndRunRunsTheLineShown)
{
    katana::cad::Document document;
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    ASSERT_TRUE(surfaces.add({"ground", square(), "test"}).ok());
    Recorder recorder;
    ContoursDialog dialog(recorder.context(&document, &reference, &surfaces));
    EXPECT_EQ(dialog.objectName(), "contoursDialog");
    for (const char* name :
         {"contourSource", "contourInterval", "contourMajorEvery", "contourBase", "contourLayer",
          "contourSmooth", "contourClip", "contourScope", "contourCommand", "contourPreview",
          "contourRun", "contourReply"}) {
        EXPECT_NE(dialog.findChild<QWidget*>(name), nullptr) << name;
    }
    auto* source = dialog.findChild<QComboBox*>("contourSource");
    ASSERT_EQ(source->count(), 1);
    EXPECT_EQ(source->currentData().toString(), "SURFACE ground");
    // A surface has no smoothing, and the drawing's controls wait for the box.
    EXPECT_FALSE(dialog.findChild<QComboBox*>("contourSmooth")->isEnabled());
    EXPECT_FALSE(dialog.findChild<QWidget*>("contourScope")->isEnabled());
    EXPECT_EQ(dialog.findChild<QLineEdit*>("contourCommand")->text(),
              "CONTOUR SURFACE ground interval=1 major=5 layer=terrain/contours");

    dialog.findChild<QLineEdit*>("contourInterval")->setText("0.25");
    dialog.findChild<QSpinBox*>("contourMajorEvery")->setValue(4);
    dialog.findChild<QPushButton*>("contourRun")->click();
    ASSERT_EQ(recorder.lines.size(), 1);
    EXPECT_EQ(recorder.lines.front(),
              "CONTOUR SURFACE ground interval=0.25 major=4 layer=terrain/contours");
    EXPECT_EQ(dialog.findChild<QPlainTextEdit*>("contourReply")->toPlainText(),
              "ran " + recorder.lines.front());
    dialog.findChild<QPushButton*>("contourPreview")->click();
    EXPECT_EQ(recorder.lines.back(),
              "CONTOUR SURFACE ground interval=0.25 major=4 layer=terrain/contours PREVIEW");
}

TEST(ContoursDialog, ARasterTakesSmoothingAndTheClipWritesTheScope)
{
    katana::cad::Document document;
    katana::interop::ReferenceData reference;
    katana::interop::RasterOverlay raster;
    raster.name = "terrain";
    raster.source = "terrain.asc";
    const auto id = reference.add(raster);
    katana::terrain::SurfaceStore surfaces;
    Recorder recorder;
    ContoursDialog dialog(recorder.context(&document, &reference, &surfaces));
    auto* source = dialog.findChild<QComboBox*>("contourSource");
    ASSERT_EQ(source->count(), 1);
    EXPECT_EQ(source->currentData().toString(), QString("RASTER %1").arg(id));
    auto* smooth = dialog.findChild<QComboBox*>("contourSmooth");
    EXPECT_TRUE(smooth->isEnabled());
    smooth->setCurrentIndex(smooth->findData(3));
    dialog.findChild<QCheckBox*>("contourClip")->setChecked(true);
    EXPECT_TRUE(dialog.findChild<QWidget*>("contourScope")->isEnabled());
    dialog.scopeControls().setChoice(katana::qt::ScopeChoice::Drawing);
    EXPECT_EQ(dialog.findChild<QLineEdit*>("contourCommand")->text(),
              QString("CONTOUR RASTER %1 interval=1 major=5 layer=terrain/contours smooth=3 "
                      "DRAWING")
                  .arg(id));
}

TEST(ContoursDialog, WithNothingToContourRunIsOffAndTheCommandSaysWhy)
{
    katana::cad::Document document;
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    Recorder recorder;
    ContoursDialog dialog(recorder.context(&document, &reference, &surfaces));
    EXPECT_FALSE(dialog.findChild<QPushButton*>("contourRun")->isEnabled());
    EXPECT_TRUE(
        dialog.findChild<QLineEdit*>("contourCommand")->placeholderText().contains("Source"));
    EXPECT_TRUE(recorder.lines.isEmpty());
}

} // namespace
