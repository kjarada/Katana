// Terrain > Analysis > Statistics by Area and Drape and Sample Heights
// (src/katana_qt/geo/zonal_stats_dialog.hpp, drape_dialog.hpp): the lines
// the fields describe, and the dialogs driven by their object names as a
// person clicks them, running exactly the line they show through the runner
// they were given. A picked point is only written into the line.

#include <gtest/gtest.h>

#include <QCheckBox>
#include <QComboBox>
#include <QLineEdit>
#include <QListWidget>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QStringList>
#include <QTabWidget>

#include <memory>
#include <optional>

#include "geo/drape_dialog.hpp"
#include "geo/zonal_stats_dialog.hpp"
#include "katana/cad/document.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace {

using katana::qt::DrapeDialog;
using katana::qt::DrapeForm;
using katana::qt::SampleForm;
using katana::qt::TerrainDialogContext;
using katana::qt::VerbOutcome;
using katana::qt::ZonalForm;
using katana::qt::ZonalStatsDialog;

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

// ---- Statistics by Area ------------------------------------------------------------------------

TEST(ZonalLine, TheScopeStatisticsPrefixAndFileAreWrittenInOrder)
{
    ZonalForm form;
    form.source = "RASTER 2";
    form.scope = "LAYERS lots";
    form.stats = {"mean", "count"};
    EXPECT_EQ(*katana::qt::zonalLine(form),
              "RASTER ZONAL RASTER 2 LAYERS lots stats=mean,count prefix=zone pixels=fractional");
    form.prefix = "ground";
    form.pixels = "centre";
    form.csv = "C:/out/lot stats.csv";
    form.overwrite = true;
    EXPECT_EQ(*katana::qt::zonalLine(form),
              "RASTER ZONAL RASTER 2 LAYERS lots stats=mean,count prefix=ground pixels=centre "
              "\"csv=C:/out/lot stats.csv\" OVERWRITE");
}

TEST(ZonalLine, WhatCannotBeWrittenIsRefusedNamingTheField)
{
    ZonalForm form;
    form.stats = {"mean"};
    EXPECT_NE(katana::qt::zonalLine(form).error().message.find("Source"), std::string::npos);
    form.source = "SURFACE ground";
    form.stats.clear();
    EXPECT_NE(katana::qt::zonalLine(form).error().message.find("Statistics"), std::string::npos);
    form.stats = {"mean"};
    form.prefix = "a b";
    EXPECT_NE(katana::qt::zonalLine(form).error().message.find("prefix"), std::string::npos);
    form.prefix = "zone";
    form.scopeError = "no layer is ticked";
    EXPECT_NE(katana::qt::zonalLine(form).error().message.find("Zones"), std::string::npos);
}

TEST(ZonalStatsDialog, EveryFieldHasItsNameAndRunRunsTheLineShown)
{
    katana::cad::Document document;
    katana::interop::ReferenceData reference;
    katana::interop::RasterOverlay raster;
    raster.name = "terrain";
    raster.source = "terrain.asc";
    const auto id = reference.add(raster);
    katana::terrain::SurfaceStore surfaces;
    Recorder recorder;
    ZonalStatsDialog dialog(recorder.context(&document, &reference, &surfaces));
    EXPECT_EQ(dialog.objectName(), "zonalStatsDialog");
    for (const char* name :
         {"zonalSource", "zonalScope", "zonalStats", "zonalPrefix", "zonalPixels", "zonalCsv",
          "zonalOverwrite", "zonalCommand", "zonalPreview", "zonalRun", "zonalReply"}) {
        EXPECT_NE(dialog.findChild<QWidget*>(name), nullptr) << name;
    }
    // The verb's own default statistics are ticked, the zones the selection.
    EXPECT_EQ(dialog.findChild<QLineEdit*>("zonalCommand")->text(),
              QString("RASTER ZONAL RASTER %1 SELECTION stats=mean,min,max,count,sum prefix=zone "
                      "pixels=fractional")
                  .arg(id));
    // Untick all but the count; zones from the whole drawing.
    auto* stats = dialog.findChild<QListWidget*>("zonalStats");
    for (int i = 0; i < stats->count(); ++i) {
        const bool count = stats->item(i)->data(Qt::UserRole).toString() == "count";
        stats->item(i)->setCheckState(count ? Qt::Checked : Qt::Unchecked);
    }
    dialog.scopeControls().setChoice(katana::qt::ScopeChoice::Drawing);
    dialog.findChild<QLineEdit*>("zonalPrefix")->setText("lot");
    EXPECT_FALSE(dialog.findChild<QCheckBox*>("zonalOverwrite")->isEnabled());
    dialog.findChild<QPushButton*>("zonalRun")->click();
    ASSERT_EQ(recorder.lines.size(), 1);
    EXPECT_EQ(recorder.lines.front(),
              QString("RASTER ZONAL RASTER %1 DRAWING stats=count prefix=lot pixels=fractional")
                  .arg(id));
    EXPECT_EQ(dialog.findChild<QPlainTextEdit*>("zonalReply")->toPlainText(),
              "ran " + recorder.lines.front());
    dialog.findChild<QPushButton*>("zonalPreview")->click();
    EXPECT_TRUE(recorder.lines.back().endsWith(" PREVIEW"));
}

TEST(ZonalStatsDialog, WithNothingToMeasureRunIsOffAndTheCommandSaysWhy)
{
    katana::cad::Document document;
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    Recorder recorder;
    ZonalStatsDialog dialog(recorder.context(&document, &reference, &surfaces));
    EXPECT_FALSE(dialog.findChild<QPushButton*>("zonalRun")->isEnabled());
    EXPECT_TRUE(dialog.findChild<QLineEdit*>("zonalCommand")->placeholderText().contains("Source"));
}

// ---- Drape and Sample Heights ------------------------------------------------------------------

TEST(DrapeLine, ARasterTakesAMethodAndASurfaceDoesNot)
{
    DrapeForm form;
    form.source = "RASTER 1";
    form.scope = "LAYERS design/strings";
    EXPECT_EQ(*katana::qt::drapeLine(form), "DRAPE RASTER 1 LAYERS design/strings method=bilinear");
    form.source = "SURFACE ground";
    EXPECT_EQ(*katana::qt::drapeLine(form), "DRAPE SURFACE ground LAYERS design/strings");
    form.scopeError = "no layer is ticked";
    EXPECT_FALSE(katana::qt::drapeLine(form).ok());
}

TEST(SampleLine, EachPointIsAnAtAndAPointMustBeTwoNumbers)
{
    SampleForm form;
    form.source = "RASTER 1";
    EXPECT_NE(katana::qt::sampleLine(form).error().message.find("Points"), std::string::npos);
    form.points = {"12.5,40", "3, 4"};
    form.method = "nearest";
    EXPECT_EQ(*katana::qt::sampleLine(form), "RASTER SAMPLE RASTER 1 AT 12.5,40 AT 3,4 method=nearest");
    form.points = {"12.5"};
    EXPECT_FALSE(katana::qt::sampleLine(form).ok());
    EXPECT_EQ(katana::qt::pointWords(1.23456, -7.0), "1.235,-7.000");
}

TEST(DrapeDialog, EveryFieldHasItsNameAndEachTabRunsItsLine)
{
    katana::cad::Document document;
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    auto tin = katana::terrain::TinSurface::create({{0, 0, 10}, {10, 0, 11}, {0, 10, 12}},
                                                   {{0, 1, 2}});
    ASSERT_TRUE(tin.ok());
    ASSERT_TRUE(surfaces
                    .add({"ground",
                          std::make_shared<const katana::terrain::TinSurface>(std::move(tin).value()),
                          "test"})
                    .ok());
    Recorder recorder;
    TerrainDialogContext context = recorder.context(&document, &reference, &surfaces);
    // A picker that answers at once, as a click in a plan view would.
    context.pickPoint = [](katana::qt::PickedPoint picked) {
        picked(katana::geometry::Point2(3.25, 4.5));
    };
    DrapeDialog dialog(std::move(context));
    EXPECT_EQ(dialog.objectName(), "drapeDialog");
    for (const char* name :
         {"drapeTabs", "drapeSource", "drapeMethod", "drapeScope", "drapeCommand", "drapePreview",
          "drapeRun", "drapeReply", "sampleSource", "sampleMethod", "samplePoint", "sampleAdd",
          "samplePick", "samplePoints", "sampleRemove", "sampleCommand", "sampleRun",
          "sampleReply"}) {
        EXPECT_NE(dialog.findChild<QWidget*>(name), nullptr) << name;
    }
    // A surface is read on its triangles: no method.
    EXPECT_FALSE(dialog.findChild<QComboBox*>("drapeMethod")->isEnabled());
    dialog.scopeControls().setChoice(katana::qt::ScopeChoice::Drawing);
    dialog.findChild<QPushButton*>("drapeRun")->click();
    ASSERT_EQ(recorder.lines.size(), 1);
    EXPECT_EQ(recorder.lines.back(), "DRAPE SURFACE ground DRAWING");
    dialog.findChild<QPushButton*>("drapePreview")->click();
    EXPECT_EQ(recorder.lines.back(), "DRAPE SURFACE ground DRAWING PREVIEW");

    // Sample: nothing to run until there is a point; one typed, one picked.
    EXPECT_FALSE(dialog.findChild<QPushButton*>("sampleRun")->isEnabled());
    dialog.findChild<QLineEdit*>("samplePoint")->setText("1,2");
    dialog.findChild<QPushButton*>("sampleAdd")->click();
    dialog.findChild<QPushButton*>("samplePick")->click();
    ASSERT_EQ(dialog.findChild<QListWidget*>("samplePoints")->count(), 2);
    dialog.findChild<QPushButton*>("sampleRun")->click();
    EXPECT_EQ(recorder.lines.back(), "RASTER SAMPLE SURFACE ground AT 1,2 AT 3.250,4.500");
    EXPECT_EQ(dialog.findChild<QPlainTextEdit*>("sampleReply")->toPlainText(),
              "ran " + recorder.lines.back());
    // A point that is not two numbers is not added.
    dialog.findChild<QLineEdit*>("samplePoint")->setText("north");
    dialog.findChild<QPushButton*>("sampleAdd")->click();
    EXPECT_EQ(dialog.findChild<QListWidget*>("samplePoints")->count(), 2);
}

TEST(DrapeDialog, WithNoPickerPickIsOff)
{
    katana::cad::Document document;
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    Recorder recorder;
    DrapeDialog dialog(recorder.context(&document, &reference, &surfaces));
    EXPECT_FALSE(dialog.findChild<QPushButton*>("samplePick")->isEnabled());
    EXPECT_FALSE(dialog.findChild<QPushButton*>("drapeRun")->isEnabled());
}

} // namespace
