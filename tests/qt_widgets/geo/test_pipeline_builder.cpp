// The GDAL Toolbox's Pipeline tab (src/katana_qt/geo/pipeline_builder.hpp):
// steps chained from GDAL's own pipeline steps, the pipeline's text written
// from them and read back into them, and the GDAL line it all describes -
// driven by object names, with a stub runner in place of the window's
// executor.

#include <gtest/gtest.h>

#include <QComboBox>
#include <QDoubleSpinBox>
#include <QFile>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QPushButton>
#include <QRadioButton>
#include <QStringList>

#include <algorithm>
#include <filesystem>
#include <string>
#include <system_error>
#include <vector>

#include "geo/argument_form.hpp"
#include "geo/geo_dialog_support.hpp"
#include "geo/pipeline_builder.hpp"

namespace {

using katana::qt::GeoDialogContext;
using katana::qt::PipelineBuilder;
using katana::qt::VerbOutcome;

template <class T>
T* child(QWidget& widget, const char* name)
{
    auto* found = widget.findChild<T*>(name);
    EXPECT_NE(found, nullptr) << name;
    return found;
}

struct Stub {
    QStringList lines;
    GeoDialogContext context()
    {
        GeoDialogContext made;
        made.run = [this](const QString& line) {
            lines << line;
            return VerbOutcome{true, "ok", {}};
        };
        made.headless = [] { return true; };
        return made;
    }
};

QStringList rows(QListWidget& list)
{
    QStringList text;
    for (int row = 0; row < list.count(); ++row) {
        text << list.item(row)->text();
    }
    return text;
}

QStringList offered(QComboBox& add)
{
    QStringList names;
    for (int i = 1; i < add.count(); ++i) {
        names << add.itemText(i);
    }
    return names;
}

void readFrom(PipelineBuilder& builder, const QString& file)
{
    child<QComboBox>(builder, "gdalPipelineKind")->setCurrentText("File");
    child<QLineEdit>(builder, "gdalPipelineFile")->setText(file);
}

TEST(PipelineBuilder, HillshadeThenColorMapBuildsTheExactPipelineString)
{
    Stub stub;
    PipelineBuilder builder(stub.context());
    readFrom(builder, "C:/data/dem.tif");
    auto* add = child<QComboBox>(builder, "gdalPipelineAdd");
    add->setCurrentText("hillshade");
    child<QDoubleSpinBox>(builder, "gdalStep.zfactor")->setValue(2.0);
    add->setCurrentText("color-map");
    child<QLineEdit>(builder, "gdalStep.color-map")->setText("ramp.txt");
    EXPECT_EQ(rows(*child<QListWidget>(builder, "gdalPipelineSteps")),
              (QStringList{"read", "hillshade  --zfactor=2", "color-map  --color-map=ramp.txt",
                           "write"}));
    EXPECT_EQ(child<QLineEdit>(builder, "gdalPipelineText")->text(),
              "read ! hillshade --zfactor=2 ! color-map --color-map=ramp.txt ! write");
    // A raster made: kept as a reference raster unless said otherwise.
    const QString command = child<QLineEdit>(builder, "gdalPipelineCommand")->text();
    EXPECT_EQ(command, "GDAL pipeline \"read ! hillshade --zfactor=2 ! color-map "
                       "--color-map=ramp.txt ! write\" FROM input FILE C:/data/dem.tif TO "
                       "REFERENCE pipeline");
    child<QPushButton>(builder, "gdalPipelineRun")->click();
    ASSERT_EQ(stub.lines.size(), 1);
    EXPECT_EQ(stub.lines.front(), command);
}

TEST(PipelineBuilder, AStepOfferedIsAvailableInAPipeline)
{
    Stub stub;
    PipelineBuilder builder(stub.context());
    readFrom(builder, "C:/data/dem.tif");
    const std::vector<std::string> steps = katana::qt::pipelineStepNames();
    ASSERT_FALSE(steps.empty());
    const QStringList names = offered(*child<QComboBox>(builder, "gdalPipelineAdd"));
    ASSERT_FALSE(names.isEmpty());
    for (const QString& name : names) {
        EXPECT_NE(std::ranges::find(steps, name.toStdString()), steps.end()) << name.toStdString();
    }
    // A pipeline's own ends are not steps to add; a program is never run.
    for (const char* never : {"read", "write", "external", "tee"}) {
        EXPECT_FALSE(names.contains(never)) << never;
    }
    // What the drawing gives is features: a raster step does not read them.
    child<QComboBox>(builder, "gdalPipelineKind")->setCurrentText("Drawing");
    const QStringList vector = offered(*child<QComboBox>(builder, "gdalPipelineAdd"));
    EXPECT_TRUE(vector.contains("buffer"));
    EXPECT_FALSE(vector.contains("hillshade"));
}

TEST(PipelineBuilder, ARasterTurnedIntoFeaturesIsOfferedFeatureStepsAfter)
{
    Stub stub;
    PipelineBuilder builder(stub.context());
    child<QComboBox>(builder, "gdalPipelineKind")->setCurrentText("Surface");
    auto* add = child<QComboBox>(builder, "gdalPipelineAdd");
    EXPECT_FALSE(offered(*add).contains("buffer"));
    add->setCurrentText("contour");
    // After contour the pipeline carries features.
    EXPECT_TRUE(offered(*add).contains("buffer"));
    EXPECT_FALSE(offered(*add).contains("hillshade"));
    QStringList kinds;
    auto* output = child<QComboBox>(builder, "gdalPipelineOutputKind");
    for (int i = 0; i < output->count(); ++i) {
        kinds << output->itemText(i);
    }
    EXPECT_EQ(kinds, (QStringList{"Layer", "File"}));
    EXPECT_EQ(child<QLineEdit>(builder, "gdalPipelineOutputName")->text(), "gis/pipeline");
}

TEST(PipelineBuilder, EditingTheTextRebuildsTheSteps)
{
    Stub stub;
    PipelineBuilder builder(stub.context());
    readFrom(builder, "C:/data/dem.tif");
    // As GDAL's own command line writes it: a value after its option, and
    // one by position (buffer's distance).
    child<QLineEdit>(builder, "gdalPipelineText")
        ->setText("read ! slope --unit percent ! contour --interval=5 ! buffer 0.1 ! write");
    EXPECT_EQ(rows(*child<QListWidget>(builder, "gdalPipelineSteps")),
              (QStringList{"read", "slope  --unit=percent", "contour  --interval=5",
                           "buffer  --distance=0.1", "write"}));
    builder.select(1);
    EXPECT_EQ(child<QComboBox>(builder, "gdalStep.unit")->currentText(), "percent");
    // A text the steps cannot follow still runs as typed, and says why.
    child<QLineEdit>(builder, "gdalPipelineText")->setText("read ! slope --nosuch=1 ! write");
    EXPECT_TRUE(child<QLabel>(builder, "gdalPipelineStatus")->text().contains("nosuch"));
    EXPECT_TRUE(child<QLineEdit>(builder, "gdalPipelineCommand")->text().contains("--nosuch=1"));
}

TEST(PipelineBuilder, APipelineWithAnExternalStepCannotBeBuilt)
{
    Stub stub;
    PipelineBuilder builder(stub.context());
    readFrom(builder, "C:/data/dem.tif");
    EXPECT_FALSE(offered(*child<QComboBox>(builder, "gdalPipelineAdd")).contains("external"));
    child<QLineEdit>(builder, "gdalPipelineText")
        ->setText("read ! external --command=calc.exe ! write");
    auto* command = child<QLineEdit>(builder, "gdalPipelineCommand");
    EXPECT_TRUE(command->text().isEmpty());
    EXPECT_TRUE(command->placeholderText().contains("external"))
        << command->placeholderText().toStdString();
    EXPECT_FALSE(child<QPushButton>(builder, "gdalPipelineRun")->isEnabled());
    child<QPushButton>(builder, "gdalPipelineRun")->click();
    EXPECT_TRUE(stub.lines.isEmpty());
}

TEST(PipelineBuilder, StepsMoveAndGoAndTheTextFollows)
{
    Stub stub;
    PipelineBuilder builder(stub.context());
    readFrom(builder, "C:/data/dem.tif");
    auto* add = child<QComboBox>(builder, "gdalPipelineAdd");
    add->setCurrentText("fill-nodata");
    add->setCurrentText("hillshade");
    auto* text = child<QLineEdit>(builder, "gdalPipelineText");
    EXPECT_EQ(text->text(), "read ! fill-nodata ! hillshade ! write");
    // hillshade is selected; up puts it first.
    child<QPushButton>(builder, "gdalPipelineUp")->click();
    EXPECT_EQ(text->text(), "read ! hillshade ! fill-nodata ! write");
    child<QPushButton>(builder, "gdalPipelineRemove")->click();
    EXPECT_EQ(text->text(), "read ! fill-nodata ! write");
    // read and write are the pipeline's own: they do not move or go.
    builder.select(0);
    EXPECT_FALSE(child<QPushButton>(builder, "gdalPipelineRemove")->isEnabled());
}

TEST(PipelineBuilder, ARecipeIsTheLineSavedIntoAScript)
{
    Stub stub;
    PipelineBuilder builder(stub.context());
    readFrom(builder, "C:/data/dem.tif");
    child<QComboBox>(builder, "gdalPipelineAdd")->setCurrentText("hillshade");
    const std::filesystem::path script =
        std::filesystem::temp_directory_path() / "katana-pipeline-recipe.kcs";
    std::error_code error;
    std::filesystem::remove(script, error);
    child<QLineEdit>(builder, "gdalPipelineScript")->setText(QString::fromStdString(script.string()));
    child<QPushButton>(builder, "gdalPipelineSave")->click();
    QFile file(QString::fromStdString(script.string()));
    ASSERT_TRUE(file.open(QIODevice::ReadOnly | QIODevice::Text));
    EXPECT_EQ(QString::fromUtf8(file.readAll()),
              child<QLineEdit>(builder, "gdalPipelineCommand")->text() + "\n");
    file.close();
    std::filesystem::remove(script, error);
}

} // namespace
