// GIS > Import Vector Data, Import Raster and Import Point Cloud
// (src/katana_qt/gis_import_dialogs.hpp): each dialog's fields make the IMPORT
// line a person would type, shown in <d>Command, and the vector dialog's
// Preview runs that line with PREVIEW through the window's executor. The verb
// itself is tested in tests/geo/test_import_options.cpp.

#include <gtest/gtest.h>

#include <QCheckBox>
#include <QComboBox>
#include <QCoreApplication>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QPushButton>
#include <QRadioButton>
#include <QSettings>
#include <QSpinBox>
#include <QStringList>

#include "gis_import_dialogs.hpp"

namespace {

using katana::geometry::Box2;
using katana::qt::GisDialogContext;
using katana::qt::VerbOutcome;

template <typename T>
T* child(QWidget& parent, const char* name)
{
    return parent.findChild<T*>(QString::fromLatin1(name));
}

// The placement group remembers the last choice in QSettings; each test
// starts from Keep, in settings of its own.
struct OwnSettings {
    OwnSettings()
    {
        QCoreApplication::setOrganizationName("KatanaWidgetTests");
        QCoreApplication::setApplicationName("ImportDialogs");
        QSettings().clear();
    }
    ~OwnSettings()
    {
        QSettings().clear();
        QCoreApplication::setOrganizationName(QString());
        QCoreApplication::setApplicationName(QString());
    }
};

katana::interop::SourceDescription vectorSource()
{
    katana::interop::SourceDescription source;
    source.path = "C:/data/site lots.gpkg";
    source.kind = katana::interop::SourceKind::Vector;
    source.driver = "GPKG";
    source.vectorLayers.push_back({"lots", 3, "Polygon", ""});
    source.vectorLayers.push_back({"roads", 2, "LineString", ""});
    return source;
}

QString commandOf(QWidget& dialog, const char* name)
{
    return child<QLineEdit>(dialog, name)->text();
}

TEST(VectorImportDialog, TheFieldsBuildExactlyTheTypedLine)
{
    const OwnSettings settings;
    katana::qt::VectorImportDialog dialog(vectorSource(), Box2{});
    // Nothing chosen: the whole file, the path quoted.
    EXPECT_EQ(commandOf(dialog, "vectorImportCommand"), "IMPORT \"C:/data/site lots.gpkg\"");

    child<QListWidget>(dialog, "vectorImportLayers")->item(1)->setCheckState(Qt::Unchecked);
    child<QLineEdit>(dialog, "vectorImportWhere")->setText("kind = 'lot'");
    child<QLineEdit>(dialog, "vectorImportFields")->setText("name, kind");
    child<QLineEdit>(dialog, "vectorImportTarget")->setText("site");
    child<QSpinBox>(dialog, "vectorImportMax")->setValue(5);
    child<QLineEdit>(dialog, "vectorImportOpenOptions")->setText("LIST_ALL_TABLES=NO");
    auto* crs = child<QComboBox>(dialog, "vectorImportCrs");
    crs->setCurrentIndex(crs->findData(QString("project")));
    child<QLineEdit>(dialog, "vectorImportAssumeCrs")->setText("EPSG:28356");
    child<QCheckBox>(dialog, "vectorImportAttributes")->setChecked(false);
    EXPECT_EQ(commandOf(dialog, "vectorImportCommand"),
              "IMPORT \"C:/data/site lots.gpkg\" layers=lots where=\"kind = 'lot'\" "
              "fields=name,kind attributes=no target=site max=5 oo=LIST_ALL_TABLES=NO "
              "crs=project srs=EPSG:28356");
}

TEST(VectorImportDialog, TheScopeAndClipFollowTheLineAndAPlacementComesFirst)
{
    const OwnSettings settings;
    katana::qt::VectorImportDialog dialog(vectorSource(), Box2{});
    child<QCheckBox>(dialog, "vectorImportUseScope")->setChecked(true);
    child<QRadioButton>(dialog, "vectorImportScopeDrawing")->click();
    child<QCheckBox>(dialog, "vectorImportClip")->setChecked(true);
    EXPECT_EQ(commandOf(dialog, "vectorImportCommand"),
              "IMPORT \"C:/data/site lots.gpkg\" DRAWING clip");

    // A scope is in the drawing's coordinates, which a placement moves the
    // data away from: no line, and Import says why by being off.
    child<QRadioButton>(dialog, "importPlacementLocal")->click();
    EXPECT_FALSE(dialog.command().ok());
    child<QCheckBox>(dialog, "vectorImportUseScope")->setChecked(false);
    EXPECT_EQ(commandOf(dialog, "vectorImportCommand"), "IMPORT \"C:/data/site lots.gpkg\" LOCAL");
}

TEST(VectorImportDialog, SqlWithALayerLeftOutHasNoLine)
{
    const OwnSettings settings;
    katana::qt::VectorImportDialog dialog(vectorSource(), Box2{});
    child<QLineEdit>(dialog, "vectorImportSql")->setText("SELECT * FROM lots");
    auto* dialect = child<QComboBox>(dialog, "vectorImportDialect");
    dialect->setCurrentIndex(dialect->findData(QString("sqlite")));
    EXPECT_EQ(commandOf(dialog, "vectorImportCommand"),
              "IMPORT \"C:/data/site lots.gpkg\" sql=\"SELECT * FROM lots\" dialect=sqlite");
    child<QListWidget>(dialog, "vectorImportLayers")->item(0)->setCheckState(Qt::Unchecked);
    EXPECT_FALSE(dialog.command().ok());
    EXPECT_FALSE(child<QPushButton>(dialog, "vectorImportPreview")->isEnabled());
}

TEST(VectorImportDialog, PreviewRunsTheLineWithPreviewAndShowsTheMatch)
{
    const OwnSettings settings;
    QStringList ran;
    GisDialogContext context;
    context.run = [&ran](const QString& line) {
        ran << line;
        return VerbOutcome{true,
                           "import file=\"C:/data/site lots.gpkg\" kind=vector preview=yes "
                           "features=2 of=5 entities=2 layers=1",
                           QString()};
    };
    katana::qt::VectorImportDialog dialog(vectorSource(), Box2{}, std::move(context));
    child<QLineEdit>(dialog, "vectorImportWhere")->setText("kind = 'lot'");
    child<QPushButton>(dialog, "vectorImportPreview")->click();
    ASSERT_EQ(ran.size(), 1);
    EXPECT_EQ(ran.front(),
              "IMPORT \"C:/data/site lots.gpkg\" where=\"kind = 'lot'\" PREVIEW");
    EXPECT_EQ(child<QLabel>(dialog, "vectorImportMatchCount")->text(),
              "2 of 5 features match; 2 entities would be imported.");
}

TEST(VectorImportDialog, AMessageIsShownAsTypedNotAsMarkup)
{
    // What a preview says is GDAL's text; read as rich text, a "<code>" in
    // it was taken for a tag and vanished.
    const OwnSettings settings;
    GisDialogContext context;
    context.run = [](const QString&) {
        return VerbOutcome{false, QString(), "ParseFailure: no such column <code> in lots"};
    };
    katana::qt::VectorImportDialog dialog(vectorSource(), Box2{}, std::move(context));
    child<QLineEdit>(dialog, "vectorImportWhere")->setText("code = 1");
    child<QPushButton>(dialog, "vectorImportPreview")->click();
    auto* shown = child<QLabel>(dialog, "vectorImportMatchCount");
    EXPECT_EQ(shown->text(), "ParseFailure: no such column <code> in lots");
    EXPECT_EQ(shown->textFormat(), Qt::PlainText);
}

TEST(ImportDialogs, ImportAndCancelHaveTheirDialogsNames)
{
    // A headless run presses a button by its object name; these had none.
    const OwnSettings settings;
    katana::qt::VectorImportDialog vector(vectorSource(), Box2{});
    auto* run = child<QPushButton>(vector, "vectorImportRun");
    ASSERT_NE(run, nullptr);
    EXPECT_EQ(run->text(), "Import");
    EXPECT_NE(child<QPushButton>(vector, "vectorImportCancel"), nullptr);
    katana::interop::SourceDescription raster;
    raster.path = "C:/data/ortho.tif";
    raster.kind = katana::interop::SourceKind::Raster;
    raster.raster = katana::interop::RasterDescription{};
    katana::qt::RasterImportDialog rasterDialog(raster);
    EXPECT_NE(child<QPushButton>(rasterDialog, "rasterImportRun"), nullptr);
    EXPECT_NE(child<QPushButton>(rasterDialog, "rasterImportCancel"), nullptr);
    katana::interop::SourceDescription cloud;
    cloud.path = "C:/data/scan.las";
    cloud.kind = katana::interop::SourceKind::PointCloud;
    cloud.pointCloud = katana::interop::PointCloudDescription{};
    katana::qt::PointCloudImportDialog cloudDialog(cloud);
    EXPECT_NE(child<QPushButton>(cloudDialog, "pointCloudImportRun"), nullptr);
    EXPECT_NE(child<QPushButton>(cloudDialog, "pointCloudImportCancel"), nullptr);
}

TEST(VectorImportDialog, ADoubleQuoteInAFieldHasNoLine)
{
    const OwnSettings settings;
    katana::qt::VectorImportDialog dialog(vectorSource(), Box2{});
    child<QLineEdit>(dialog, "vectorImportWhere")->setText("name = \"A\"");
    EXPECT_FALSE(dialog.command().ok());
    EXPECT_TRUE(commandOf(dialog, "vectorImportCommand").isEmpty());
}

TEST(RasterImportDialog, TheFieldsBuildExactlyTheTypedLine)
{
    katana::interop::SourceDescription source;
    source.path = "C:/data/ortho.tif";
    source.kind = katana::interop::SourceKind::Raster;
    source.raster = katana::interop::RasterDescription{};
    source.raster->bandCount = 4;
    source.subdatasets.push_back({"GTIFF_DIR:1:C:/data/ortho.tif", "Page 1"});
    source.subdatasets.push_back({"GTIFF_DIR:2:C:/data/ortho.tif", "Page 2"});
    katana::qt::RasterImportDialog dialog(source);
    EXPECT_EQ(commandOf(dialog, "rasterImportCommand"), "IMPORT \"C:/data/ortho.tif\"");

    child<QLineEdit>(dialog, "rasterImportName")->setText("near infrared");
    auto* pixels = child<QComboBox>(dialog, "rasterImportMaxPixels");
    pixels->setCurrentIndex(pixels->findData(2048));
    child<QSpinBox>(dialog, "rasterImportBand")->setValue(4);
    child<QComboBox>(dialog, "rasterImportSubdataset")->setCurrentIndex(2);
    auto* crs = child<QComboBox>(dialog, "rasterImportCrs");
    crs->setCurrentIndex(crs->findData(QString("adopt")));
    EXPECT_EQ(commandOf(dialog, "rasterImportCommand"),
              "IMPORT \"C:/data/ortho.tif\" name=\"near infrared\" maxpixels=2048 band=4 "
              "subdataset=2 crs=adopt");
}

TEST(PointCloudImportDialog, TheFieldsBuildExactlyTheTypedLine)
{
    katana::interop::SourceDescription source;
    source.path = "C:/data/scan.las";
    source.kind = katana::interop::SourceKind::PointCloud;
    source.pointCloud = katana::interop::PointCloudDescription{};
    source.pointCloud->pointCount = 1'000'000;
    katana::qt::PointCloudImportDialog dialog(source);
    EXPECT_EQ(commandOf(dialog, "pointCloudImportCommand"), "IMPORT \"C:/data/scan.las\"");
    child<QSpinBox>(dialog, "pointCloudImportBudget")->setValue(500'000);
    auto* classes = child<QComboBox>(dialog, "pointCloudImportClasses");
    classes->setCurrentIndex(classes->findData(2)); // ASPRS class 2, ground
    EXPECT_EQ(commandOf(dialog, "pointCloudImportCommand"),
              "IMPORT \"C:/data/scan.las\" budget=500000 class=2");
}

} // namespace
