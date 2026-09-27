// IMPORT, EXPORT and COPC in the window (docs/interop.md, "IMPORT, EXPORT,
// INFO, REFS and COPC on every front end"): lines of the executor, so the
// geo workbench runs them as background jobs - an IMPORT's read on its own
// thread, a cancel that imports nothing, an EXPORT that writes nothing once
// cancelled. What each writes is tested in tests/geo; here, the job.

#include <gtest/gtest.h>

#include <QMainWindow>
#include <QString>
#include <QStringList>

#include <filesystem>
#include <optional>
#include <string>

#include "geo/geo_workbench.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"

namespace {

using katana::qt::GeoServices;
using katana::qt::GeoWorkbench;
using katana::qt::JobId;
using katana::qt::JobRunner;
using katana::qt::VerbOutcome;

// samples/gis, from the widget tests' own data folder in the source tree.
const std::string kSamples = std::string(KATANA_QT_WIDGET_DATA) + "/../../../samples/gis";

// The window's side, as MainWindow hands it over; declared before the window
// and the workbench, so what a job's Apply reaches outlives the runner.
struct Window {
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter{document};
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    QStringList logged;
    QStringList errors;
    bool headless = true;
    std::optional<VerbOutcome> finished;
    QMainWindow main;
    std::unique_ptr<GeoWorkbench> workbench;

    Window()
    {
        GeoServices services;
        services.document = &document;
        services.interpreter = &interpreter;
        services.reference = &reference;
        services.surfaces = &surfaces;
        services.scratch = std::filesystem::temp_directory_path() / "katana-gis-verbs-window-test";
        services.log = [this](const QString& text, bool isError) {
            (isError ? errors : logged) << text;
        };
        services.headless = [this] { return headless; };
        services.finished = [this](JobId, const VerbOutcome& outcome) { finished = outcome; };
        workbench = std::make_unique<GeoWorkbench>(main, std::move(services));
    }
};

TEST(GisVerbsWindow, AnImportIsAJobWhoseRecordsAreLogged)
{
    Window window;
    const QString line =
        QString::fromStdString("IMPORT \"" + kSamples + "/parcels.geojson\" LOCAL");
    ASSERT_TRUE(window.workbench->runLine(line));
    EXPECT_NE(window.workbench->lastJob(), katana::qt::kNoJob);
    ASSERT_TRUE(window.finished.has_value());
    EXPECT_TRUE(window.finished->ok) << window.finished->error.toStdString();
    EXPECT_TRUE(window.finished->reply.startsWith("imported file=")) << window.finished->reply.toStdString();
    EXPECT_TRUE(window.finished->reply.contains("placed placement=local east=-180 north=0"));
    // parcels.geojson: 8 features, the spoil heaps a MultiPolygon of two.
    EXPECT_EQ(window.document.model().entities.size(), 9u);
}

TEST(GisVerbsWindow, AnInteractiveImportSaysItStartedAndACancelImportsNothing)
{
    Window window;
    window.headless = false;
    const QString line = QString::fromStdString("IMPORT \"" + kSamples + "/parcels.geojson\"");
    ASSERT_TRUE(window.workbench->runLine(line));
    const JobId id = window.workbench->lastJob();
    ASSERT_NE(id, katana::qt::kNoJob);
    ASSERT_FALSE(window.logged.isEmpty());
    EXPECT_TRUE(window.logged.back().startsWith(
        QString("job id=%1 title=\"IMPORT parcels.geojson\" state=started").arg(id)))
        << window.logged.back().toStdString();
    // Cancelled before its completion is delivered: whenever the read ended,
    // a cancelled job never applies (jobs.hpp).
    JobRunner& runner = JobRunner::of(window.main);
    EXPECT_TRUE(runner.cancel(id));
    runner.waitFor(id);
    ASSERT_TRUE(window.finished.has_value());
    EXPECT_FALSE(window.finished->ok);
    EXPECT_EQ(window.finished->error, "error: InvalidState: cancelled");
    EXPECT_TRUE(window.document.model().entities.empty());
    EXPECT_FALSE(window.document.model().layers.contains("parcels"));
}

TEST(GisVerbsWindow, ACancelledExportLeavesNoFile)
{
    Window window;
    window.headless = false;
    ASSERT_TRUE(window.interpreter.run("RECT 0,0 10,5").ok());
    const std::filesystem::path folder =
        std::filesystem::temp_directory_path() / "katana gis verbs window export";
    std::error_code error;
    std::filesystem::remove_all(folder, error);
    std::filesystem::create_directories(folder, error);
    const std::filesystem::path file = folder / "cancelled.geojson";
    ASSERT_TRUE(window.workbench->runLine(
        QString::fromStdString("EXPORT \"" + file.generic_string() + "\"")));
    const JobId id = window.workbench->lastJob();
    ASSERT_NE(id, katana::qt::kNoJob);
    JobRunner& runner = JobRunner::of(window.main);
    EXPECT_TRUE(runner.cancel(id));
    runner.waitFor(id);
    ASSERT_TRUE(window.finished.has_value());
    EXPECT_FALSE(window.finished->ok);
    // Nothing in the folder at all: not the file, and not the folder it was
    // written in before its apply would have moved it into place.
    EXPECT_TRUE(std::filesystem::is_empty(folder));
    std::filesystem::remove_all(folder, error);
}

TEST(GisVerbsWindow, RefsAnswersAtOnceWithoutAJob)
{
    Window window;
    ASSERT_TRUE(window.workbench->runLine("REFS"));
    EXPECT_EQ(window.workbench->lastJob(), katana::qt::kNoJob);
    ASSERT_EQ(window.logged.size(), 1);
    EXPECT_EQ(window.logged.front(), "references rasters=0 clouds=0 missing=0");
}

TEST(GisVerbsWindow, InfoOfAnEntityIsLeftToTheWindow)
{
    Window window;
    EXPECT_FALSE(window.workbench->runLine("INFO #1"));
    EXPECT_TRUE(window.logged.isEmpty());
    EXPECT_TRUE(window.errors.isEmpty());
}

} // namespace
