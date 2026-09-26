// The geoprocessing verbs in the window (src/katana_qt/geo/geo_workbench.hpp):
// a line prepared on the GUI thread, run as a background job, its reply
// logged and handed to whoever listens, and a cancelled job leaving nothing.
// The executor itself is tested in tests/geo; here, only what the window adds.

#include <gtest/gtest.h>

#include <QApplication>
#include <QMainWindow>
#include <QString>
#include <QStringList>

#include <filesystem>
#include <optional>

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

// The window's side, as MainWindow hands it over. Declared before the window
// and the workbench, so what a job's Apply reaches outlives the runner that
// joins it.
struct Window {
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter{document};
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    QStringList logged;
    QStringList errors;
    bool headless = true;
    std::optional<VerbOutcome> finished;
    JobId finishedJob = katana::qt::kNoJob;
    QMainWindow main;
    std::unique_ptr<GeoWorkbench> workbench;

    Window()
    {
        GeoServices services;
        services.document = &document;
        services.interpreter = &interpreter;
        services.reference = &reference;
        services.surfaces = &surfaces;
        services.scratch = std::filesystem::temp_directory_path() / "katana-geo-workbench-test";
        services.log = [this](const QString& text, bool isError) {
            (isError ? errors : logged) << text;
        };
        services.headless = [this] { return headless; };
        services.finished = [this](JobId id, const VerbOutcome& outcome) {
            finishedJob = id;
            finished = outcome;
        };
        workbench = std::make_unique<GeoWorkbench>(main, std::move(services));
    }
};

TEST(GeoWorkbench, ALineOfAnotherVerbIsLeftToTheWindow)
{
    Window window;
    EXPECT_FALSE(window.workbench->runLine("LINE 0,0 10,0"));
    EXPECT_FALSE(window.workbench->runLine("ONLINE PROVIDERS"));
    EXPECT_TRUE(window.logged.isEmpty());
}

TEST(GeoWorkbench, WhatAnswersAtOnceIsLoggedWithoutAJob)
{
    Window window;
    ASSERT_TRUE(window.workbench->runLine("GDAL VERSION"));
    EXPECT_EQ(window.workbench->lastJob(), katana::qt::kNoJob);
    ASSERT_EQ(window.logged.size(), 1);
    EXPECT_TRUE(window.logged.front().startsWith("gdal version=3.")) << window.logged.front().toStdString();
}

TEST(GeoWorkbench, ARefusalIsLoggedAsAnError)
{
    Window window;
    ASSERT_TRUE(window.workbench->runLine("GDAL raster hillshade --config X=Y"));
    ASSERT_EQ(window.errors.size(), 1);
    EXPECT_TRUE(window.errors.front().startsWith("error: InvalidArgument: --config"));
}

TEST(GeoWorkbench, ARunIsAJobWhoseReplyIsLoggedAndHandedToWhoeverListens)
{
    Window window;
    ASSERT_TRUE(window.interpreter.run("LINE 0,0 100,0").ok());
    std::optional<VerbOutcome> heard;
    const int key = window.workbench->addFinishedListener(
        [&heard](JobId, const VerbOutcome& outcome) { heard = outcome; });
    // Headless, the line waits for its job, as --command and a script do.
    ASSERT_TRUE(window.workbench->runLine("GDAL vector buffer distance=1 FROM DRAWING"));
    EXPECT_NE(window.workbench->lastJob(), katana::qt::kNoJob);
    ASSERT_TRUE(window.finished.has_value());
    EXPECT_EQ(window.finishedJob, window.workbench->lastJob());
    EXPECT_TRUE(window.finished->ok) << window.finished->error.toStdString();
    EXPECT_TRUE(window.finished->reply.contains("created=1"));
    ASSERT_TRUE(heard.has_value());
    EXPECT_EQ(heard->reply, window.finished->reply);
    ASSERT_FALSE(window.logged.isEmpty());
    EXPECT_TRUE(window.logged.back().contains("layer=gis/buffer created=1"));
    EXPECT_EQ(window.document.model().entities.size(), 2u);
    window.workbench->removeFinishedListener(key);
}

TEST(GeoWorkbench, AnInteractiveRunSaysItStartedAndACancelAppliesNothing)
{
    Window window;
    window.headless = false;
    ASSERT_TRUE(window.interpreter.run("LINE 0,0 100,0").ok());
    ASSERT_TRUE(window.workbench->runLine("GDAL vector buffer distance=1 FROM DRAWING"));
    const JobId id = window.workbench->lastJob();
    ASSERT_NE(id, katana::qt::kNoJob);
    ASSERT_FALSE(window.logged.isEmpty());
    EXPECT_TRUE(window.logged.back().startsWith(QString("job id=%1 title=\"GDAL vector buffer\" "
                                                        "state=started")
                                                    .arg(id)))
        << window.logged.back().toStdString();
    // Cancelled before its completion is delivered: whenever the work ended,
    // a cancelled job never applies (jobs.hpp).
    JobRunner& runner = JobRunner::of(window.main);
    EXPECT_TRUE(runner.cancel(id));
    runner.waitFor(id);
    ASSERT_TRUE(window.finished.has_value());
    EXPECT_FALSE(window.finished->ok);
    EXPECT_EQ(window.finished->error, "error: InvalidState: cancelled");
    EXPECT_EQ(window.document.model().entities.size(), 1u);
    EXPECT_FALSE(window.document.model().layers.contains("gis/buffer"));
}

// The one reader of the record an interactive run logs reads the one
// writer's line back, on whichever line of a reply it stands - the dialogs
// and the window's awaitJob each had their own pattern - and a title with a
// quote in it, which the writer now escapes as any record's value.
TEST(GeoWorkbench, TheStartedRecordReadsBackAsItsJob)
{
    using katana::qt::startedJob;
    using katana::qt::startedRecord;
    EXPECT_EQ(startedRecord(7, "GDAL vector buffer"),
              "job id=7 title=\"GDAL vector buffer\" state=started");
    EXPECT_EQ(startedJob(startedRecord(7, "GDAL vector buffer")), std::optional<JobId>(7));
    EXPECT_EQ(startedJob(startedRecord(12, "EXPORT \"a b\".gpkg")), std::optional<JobId>(12));
    EXPECT_EQ(startedJob("scope scope=drawing matched=1\n" + startedRecord(3, "x")),
              std::optional<JobId>(3));
    // A reply that started none - a headless run's records - is no job.
    EXPECT_EQ(startedJob("gdal algorithm=\"vector buffer\" seconds=0.001"), std::nullopt);
    EXPECT_EQ(startedJob("job id=4 title=x state=finished"), std::nullopt);
    EXPECT_EQ(startedJob(""), std::nullopt);
}

} // namespace
