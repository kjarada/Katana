// GIS > Dataset Information (src/katana_qt/dataset_info_dialog.hpp), driven by
// its object names: the INFO lines it builds, run through an executor as the
// window runs them, and what it shows of their records - the summary, the
// fields, the bands, GDAL's JSON - and what its Statistics and Check buttons
// run. The records themselves are tested in tests/geo/test_info_verb.cpp.
//
// By hand: samples/gis/terrain.asc is 120 x 90 cells, one Float32 band with
// no-data -9999, values 24.892 to 38.819 (read from the text);
// tests/geo/data/lots.geojson has one layer "lots" with fields kind and name.

#include <gtest/gtest.h>

#include <QApplication>
#include <QClipboard>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QTableWidget>

#include <filesystem>
#include <functional>
#include <string>
#include <vector>

#include "dataset_info_dialog.hpp"
#include "geo/geo_verbs.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"

namespace {

using katana::qt::DatasetInfoDialog;
using katana::qt::DatasetInfoRunner;
using katana::qt::VerbOutcome;

// The source tree, from the widget tests' own data folder.
const std::string kRoot = std::string(KATANA_QT_WIDGET_DATA) + "/../../..";

// An executor as the window has one, run inline: what a headless window's
// runVerbLine gives back once a job has been waited for.
struct Executor {
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter{document};
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    katana::app::geo::Context context{document,
                                      interpreter,
                                      reference,
                                      surfaces,
                                      std::filesystem::temp_directory_path() / "katana-info-dialog",
                                      {},
                                      {}};
    QStringList lines;

    DatasetInfoRunner runner()
    {
        DatasetInfoRunner made;
        made.run = [this](const QString& line) {
            lines << line;
            auto reply = katana::app::geo::runNow(context, line.toStdString());
            VerbOutcome outcome;
            outcome.ok = reply.ok();
            if (reply) {
                outcome.reply = QString::fromStdString(*reply);
            } else {
                outcome.error = "error: " + QString::fromStdString(reply.error().describe());
            }
            return outcome;
        };
        return made;
    }
};

template <typename Widget>
Widget* child(const QWidget& parent, const char* name)
{
    auto* found = parent.findChild<Widget*>(QString::fromLatin1(name));
    EXPECT_NE(found, nullptr) << name;
    return found;
}

TEST(DatasetInfoDialog, ItRunsTheInfoLinesAndShowsTheRasterItsBandsAndGdalsJson)
{
    Executor executor;
    const QString path = QString::fromStdString(kRoot + "/samples/gis/terrain.asc");
    DatasetInfoDialog dialog(path, executor.runner());
    EXPECT_EQ(dialog.objectName(), "datasetInfoDialog");
    EXPECT_TRUE(dialog.described());
    ASSERT_EQ(executor.lines.size(), 2);
    EXPECT_EQ(executor.lines[0], "INFO \"" + path + "\"");
    EXPECT_EQ(executor.lines[1], "INFO \"" + path + "\" JSON");
    EXPECT_EQ(child<QLineEdit>(dialog, "datasetInfoCommand")->text(), executor.lines[1]);

    const QString summary = child<QPlainTextEdit>(dialog, "datasetInfoSummary")->toPlainText();
    EXPECT_TRUE(summary.contains("dataset file=")) << summary.toStdString();
    EXPECT_TRUE(summary.contains("driver=AAIGrid"));
    EXPECT_TRUE(summary.contains("raster width=120 height=90 bands=1"));

    auto* bands = child<QTableWidget>(dialog, "datasetInfoBands");
    ASSERT_EQ(bands->rowCount(), 1);
    EXPECT_EQ(bands->item(0, 0)->text(), "1");
    EXPECT_EQ(bands->item(0, 1)->text(), "Float32");
    EXPECT_EQ(bands->item(0, 2)->text(), "-9999");
    EXPECT_EQ(bands->item(0, 3)->text(), ""); // not computed: absent, not zero

    const QString json = child<QPlainTextEdit>(dialog, "datasetInfoJson")->toPlainText();
    EXPECT_TRUE(json.contains("\"driverShortName\": \"AAIGrid\"")) << json.left(400).toStdString();
    auto* copy = child<QPushButton>(dialog, "datasetInfoCopyJson");
    ASSERT_TRUE(copy->isEnabled());
    copy->click();
    EXPECT_EQ(QApplication::clipboard()->text(), json);
}

TEST(DatasetInfoDialog, StatisticsAndCheckAreTheirLines)
{
    Executor executor;
    const QString path = QString::fromStdString(kRoot + "/samples/gis/terrain.asc");
    DatasetInfoDialog dialog(path, executor.runner());
    child<QPushButton>(dialog, "datasetInfoStats")->click();
    EXPECT_EQ(executor.lines.back(), "INFO \"" + path + "\" STATS");
    auto* bands = child<QTableWidget>(dialog, "datasetInfoBands");
    ASSERT_EQ(bands->rowCount(), 1);
    // Float32 values: within half an ulp (1.9e-6 below 64) of the text's.
    EXPECT_NEAR(bands->item(0, 3)->text().toDouble(), 24.892, 2e-6);
    EXPECT_NEAR(bands->item(0, 4)->text().toDouble(), 38.819, 2e-6);

    child<QPushButton>(dialog, "datasetInfoCheck")->click();
    EXPECT_EQ(executor.lines.back(), "INFO \"" + path + "\" CHECK");
    EXPECT_EQ(child<QLabel>(dialog, "datasetInfoReply")->text(), "check code=0 problems=0");
    // The check's own band records carry no statistics; the ones computed
    // are of the same file and stay (they were wiped to blanks).
    ASSERT_EQ(bands->rowCount(), 1);
    EXPECT_NEAR(bands->item(0, 3)->text().toDouble(), 24.892, 2e-6);
    EXPECT_NEAR(bands->item(0, 4)->text().toDouble(), 38.819, 2e-6);
}

TEST(DatasetInfoDialog, AVectorFilesFieldsAreListed)
{
    Executor executor;
    const QString path = QString::fromStdString(kRoot + "/tests/geo/data/lots.geojson");
    DatasetInfoDialog dialog(path, executor.runner());
    auto* fields = child<QTableWidget>(dialog, "datasetInfoFields");
    ASSERT_EQ(fields->rowCount(), 2);
    EXPECT_EQ(fields->item(0, 0)->text(), "lots");
    EXPECT_EQ(fields->item(0, 1)->text(), "kind");
    EXPECT_EQ(fields->item(0, 2)->text(), "String");
    EXPECT_EQ(fields->item(1, 1)->text(), "name");
    EXPECT_EQ(child<QTableWidget>(dialog, "datasetInfoBands")->rowCount(), 0);
}

TEST(DatasetInfoDialog, AFileThatCannotBeDescribedSaysWhy)
{
    Executor executor;
    DatasetInfoDialog dialog(QStringLiteral("C:/no such folder/absent.tif"), executor.runner());
    EXPECT_FALSE(dialog.described());
    EXPECT_TRUE(child<QLabel>(dialog, "datasetInfoReply")->text().contains("NotFound"))
        << child<QLabel>(dialog, "datasetInfoReply")->text().toStdString();
    EXPECT_FALSE(child<QPushButton>(dialog, "datasetInfoCopyJson")->isEnabled());
}

TEST(DatasetInfoDialog, AJobsAnswerIsTakenWhenTheJobEnds)
{
    // The window's runner starts a job and answers later: the dialog shows
    // it is reading, then what the job said.
    Executor executor;
    std::vector<std::function<void(const VerbOutcome&)>> waiting;
    DatasetInfoRunner runner = executor.runner();
    const auto direct = runner.run;
    runner.run = [&executor](const QString& line) {
        executor.lines << line;
        return VerbOutcome{true, "job id=7 title=\"INFO terrain.asc\" state=started", {}};
    };
    runner.await = [&waiting](const VerbOutcome& started, std::function<void(const VerbOutcome&)> done) {
        EXPECT_TRUE(started.reply.startsWith("job id=7"));
        waiting.push_back(std::move(done));
        return true;
    };
    const QString path = QString::fromStdString(kRoot + "/samples/gis/terrain.asc");
    DatasetInfoDialog dialog(path, runner);
    EXPECT_FALSE(dialog.described());
    EXPECT_TRUE(child<QLabel>(dialog, "datasetInfoReply")->text().startsWith("Reading"));
    ASSERT_EQ(waiting.size(), 2u);
    waiting[0](direct("INFO \"" + path + "\""));
    EXPECT_TRUE(dialog.described());
    EXPECT_EQ(child<QTableWidget>(dialog, "datasetInfoBands")->rowCount(), 1);
}

} // namespace
