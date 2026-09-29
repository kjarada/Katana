// The import wizard's instrument path, driven by its object names as a
// person clicks it: the Content step shows what each format's reader read
// (checked against the reader called directly, and against what the
// fixtures hold, counted by hand), a format with no published layout is
// refused on the Format step with what to export instead, an import with an
// adjustment is one undo step that leaves a survey job, and a large file is
// read off the GUI thread, which keeps answering.

#include <gtest/gtest.h>

#include <QApplication>
#include <QComboBox>
#include <QElapsedTimer>
#include <QFile>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QTemporaryDir>
#include <QTimer>
#include <QTreeWidget>

#include <algorithm>
#include <filesystem>
#include <format>
#include <map>
#include <string>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/surveyio/reader.hpp"
#include "survey/survey_import_wizard.hpp"
#include "survey/survey_task.hpp"

namespace survey = katana::survey;
namespace surveyio = katana::surveyio;
using katana::cad::Document;
using katana::qt::SurveyImportContext;
using katana::qt::SurveyImportWizard;

namespace {

template <typename T> T* child(QWidget& parent, const char* name)
{
    T* found = parent.findChild<T*>(QString::fromLatin1(name));
    EXPECT_NE(found, nullptr) << "no " << name;
    return found;
}

void click(QWidget& parent, const char* name)
{
    auto* button = child<QPushButton>(parent, name);
    ASSERT_NE(button, nullptr);
    ASSERT_TRUE(button->isEnabled()) << name << " is disabled";
    button->click();
    QApplication::processEvents();
}

void choose(QWidget& parent, const char* name, const QString& item)
{
    auto* box = child<QComboBox>(parent, name);
    ASSERT_NE(box, nullptr);
    const int index = box->findText(item);
    ASSERT_GE(index, 0) << name << " has no " << item.toStdString();
    box->setCurrentIndex(index);
}

std::string fixture(const char* relative)
{
    return std::string(KATANA_SURVEYIO_DATA) + "/" + relative;
}

std::string uiFixture(const char* name)
{
    return std::string(KATANA_SURVEY_UI_DATA) + "/" + name;
}

// The Content step's rows, by what they count.
std::map<std::string, std::string> contentRows(QWidget& wizard)
{
    std::map<std::string, std::string> rows;
    auto* tree = child<QTreeWidget>(wizard, "content");
    for (int i = 0; tree != nullptr && i < tree->topLevelItemCount(); ++i) {
        rows[tree->topLevelItem(i)->text(0).toStdString()] =
            tree->topLevelItem(i)->text(1).toStdString();
    }
    return rows;
}

struct Session {
    Document document;
    std::vector<std::pair<QString, bool>> log;
    std::unique_ptr<SurveyImportWizard> wizard;

    Session()
    {
        SurveyImportContext context;
        context.document = &document;
        context.log = [this](const QString& text, bool isError) { log.emplace_back(text, isError); };
        wizard = std::make_unique<SurveyImportWizard>(std::move(context), nullptr);
        wizard->show();
    }

    // File -> Format -> (Content, read at once for a small file).
    void openAndRead(const std::string& path)
    {
        child<QLineEdit>(*wizard, "file")->setText(QString::fromStdString(path));
        click(*wizard, "next");
        click(*wizard, "next");
    }

    [[nodiscard]] QString step() const { return child<QLabel>(*wizard, "step")->text(); }

    [[nodiscard]] bool logged(const QString& pattern) const
    {
        return std::ranges::any_of(log, [&](const auto& line) { return line.first.contains(pattern); });
    }
};

// What the reader returns for the file, called directly - the Content step
// must show exactly this.
surveyio::ReadResult readDirectly(const std::string& path)
{
    QFile file(QString::fromStdString(path));
    EXPECT_TRUE(file.open(QIODevice::ReadOnly));
    const QByteArray bytes = file.readAll();
    const std::filesystem::path location(path);
    const auto detection = surveyio::detectFormat(surveyio::probeOf(
        std::string_view(bytes.constData(), static_cast<std::size_t>(bytes.size())),
        location.filename().string(), false));
    const auto format = detection.format();
    EXPECT_TRUE(format.ok());
    surveyio::ReadOptions options;
    options.siblings = surveyio::siblingsInFolder(location.parent_path());
    auto read = surveyio::readSurvey(
        surveyio::formatRegistry(), format ? format->id : std::string{},
        std::string_view(bytes.constData(), static_cast<std::size_t>(bytes.size())),
        location.filename().string(), options);
    EXPECT_TRUE(read.ok()) << (read ? "" : read.error().describe());
    return read ? std::move(*read) : surveyio::ReadResult{};
}

std::size_t observationsOf(const survey::SurveyProject& project)
{
    std::size_t n = project.observations.size();
    for (const survey::SurveyStation& station : project.stations) {
        n += station.observations.size();
    }
    return n;
}

void expectContentAsRead(const std::map<std::string, std::string>& rows,
                         const surveyio::ReadResult& read)
{
    const survey::SurveyProject& p = read.project;
    EXPECT_EQ(rows.at("Setups"), std::to_string(p.stations.size()));
    EXPECT_EQ(rows.at("Observations"), std::to_string(observationsOf(p)));
    EXPECT_EQ(rows.at("Points with coordinates"), std::to_string(p.points.size()));
    EXPECT_EQ(rows.at("Points without coordinates"), std::to_string(p.unpositionedPoints.size()));
    EXPECT_EQ(rows.at("Coded features"), std::to_string(p.features.size()));
    EXPECT_EQ(rows.at("Control the file declares"), std::to_string(p.controlPoints.size()));
    EXPECT_EQ(rows.at("GNSS sessions"), std::to_string(p.gnssSessions.size()));
    EXPECT_EQ(rows.at("Files read beside it"), std::to_string(read.siblingsRead.size()));
    EXPECT_EQ(rows.at("Not in the file"), std::to_string(read.notCarried.size()));
    EXPECT_EQ(rows.at("Warnings"), std::to_string(read.warnings.size()));
}

} // namespace

TEST(SurveyImportWizard, TheFormatStepListsEveryRegisteredFormatWithItsRecord)
{
    Session session;
    child<QLineEdit>(*session.wizard, "file")
        ->setText(QString::fromStdString(fixture("leica/tps_gsi8.gsi")));
    click(*session.wizard, "next");
    auto* candidates = child<QTreeWidget>(*session.wizard, "candidates");
    ASSERT_NE(candidates, nullptr);
    const auto formats = surveyio::formatRegistry().formats();
    ASSERT_EQ(candidates->topLevelItemCount(), static_cast<int>(formats.size()));
    // The detected one first, with its confidence; every row with its record.
    EXPECT_EQ(candidates->topLevelItem(0)->text(0).toStdString(),
              surveyio::formatRegistry().find("leica-gsi")->humanName);
    for (int i = 0; i < candidates->topLevelItemCount(); ++i) {
        const std::string record = candidates->topLevelItem(i)->text(3).toStdString();
        EXPECT_NE(record.find(", parser: "), std::string::npos) << record;
    }
    EXPECT_EQ(child<QComboBox>(*session.wizard, "format")->currentData().toString(), "leica-gsi");
}

TEST(SurveyImportWizard, TheContentStepShowsWhatEachFormatsReaderRead)
{
    // Setups counted by hand in each fixture: tps_gsi8.gsi has two blocks
    // with word 84 (a station), one_setup.jxl one StationRecord,
    // setup_metres.rw5 one OC record, setup.gt7 one STN line, setup.fld one
    // 03 record; a RINEX observation file has no setups and one GNSS session,
    // and its navigation file of the same name (.24n) is read beside it.
    const struct {
        const char* file;
        const char* setups;
    } files[] = {{"leica/tps_gsi8.gsi", "2"},
                 {"trimble_jxl/one_setup.jxl", "1"},
                 {"rw5/setup_metres.rw5", "1"},
                 {"gts/setup.gt7", "1"},
                 {"fld/setup.fld", "1"},
                 {"rinex/test2560.24o", "0"}};
    for (const auto& entry : files) {
        SCOPED_TRACE(entry.file);
        Session session;
        session.openAndRead(fixture(entry.file));
        EXPECT_TRUE(session.step().contains("What the file holds")) << session.step().toStdString();
        const auto rows = contentRows(*session.wizard);
        ASSERT_FALSE(rows.empty());
        EXPECT_EQ(rows.at("Setups"), entry.setups);
        expectContentAsRead(rows, readDirectly(fixture(entry.file)));
    }
}

TEST(SurveyImportWizard, TheContentStepNamesTheFilesFoundBesideTheChosenOne)
{
    Session session;
    session.openAndRead(fixture("rinex/test2560.24o"));
    auto* tree = child<QTreeWidget>(*session.wizard, "content");
    const auto found = tree->findItems("Files read beside it", Qt::MatchExactly);
    ASSERT_EQ(found.size(), 1);
    ASSERT_EQ(found.front()->childCount(), 1);
    EXPECT_EQ(found.front()->child(0)->text(0), "test2560.24n");
    EXPECT_EQ(contentRows(*session.wizard).at("GNSS sessions"), "1");
}

TEST(SurveyImportWizard, AFormatWithNoPublishedLayoutIsRefusedWithWhatToExportInstead)
{
    const struct {
        const char* file;
        const char* instead;
    } files[] = {{"leica/dbx/JOB1.X01", "GSI-16 or LandXML"},
                 {"trimble_dc/synthetic_job.dc", "JobXML"}};
    for (const auto& entry : files) {
        SCOPED_TRACE(entry.file);
        Session session;
        session.openAndRead(fixture(entry.file));
        // Still on the Format step, saying what to export.
        EXPECT_TRUE(session.step().contains("The file's format")) << session.step().toStdString();
        const QString message = child<QLabel>(*session.wizard, "message")->text();
        EXPECT_TRUE(message.contains(entry.instead)) << message.toStdString();
        EXPECT_TRUE(session.document.surveyJobs().empty());
    }
}

TEST(SurveyImportWizard, AnImportWithANetworkAdjustmentIsOneUndoStepThatLeavesASurveyJob)
{
    Session session;
    session.openAndRead(uiFixture("network_gsi8.gsi"));
    click(*session.wizard, "next"); // System
    click(*session.wizard, "next"); // Reduction and adjustment
    QWidget& w = *session.wizard;
    choose(w, "method", "network least squares");
    choose(w, "controlPick", "A");
    click(w, "addControl");
    choose(w, "controlPick", "B");
    click(w, "addControl");
    click(w, "previewReduction");
    auto* sections = child<QWidget>(w, "previewReportSections");
    ASSERT_NE(sections, nullptr);
    const QString preview = child<QLabel>(w, "message")->text();
    EXPECT_TRUE(preview.contains("network least squares (horizontal): variance factor"))
        << preview.toStdString();
    click(w, "next"); // Options
    click(w, "next"); // Report
    EXPECT_TRUE(session.step().contains("Report"));
    click(w, "import");

    ASSERT_EQ(session.document.surveyJobs().size(), 1u);
    const auto& job = session.document.surveyJobs().front();
    EXPECT_EQ(job.formatId, "leica-gsi");
    EXPECT_EQ(job.sourceFileName, "network_gsi8.gsi");
    EXPECT_EQ(job.settings.method, survey::AdjustmentMethod::Network);
    ASSERT_EQ(job.settings.control.size(), 2u);
    // A and B from the file, P and Q computed: four points drawn.
    EXPECT_EQ(job.placedPoints.size(), 4u);
    EXPECT_FALSE(job.reportHtml.empty());
    EXPECT_TRUE(session.logged("Imported survey job " + QString::fromStdString(job.id) +
                               " from network_gsi8.gsi"));
    EXPECT_TRUE(session.logged("network least squares (horizontal): variance factor"));

    ASSERT_TRUE(session.document.undo().ok());
    EXPECT_TRUE(session.document.surveyJobs().empty());
    ASSERT_TRUE(session.document.redo().ok());
    EXPECT_EQ(session.document.surveyJobs().size(), 1u);
}

// The opcode field file through Survey > Import Survey Data with every step
// left at its defaults: the reduction's radiation from the entered CP1,
// oriented on CP2, draws CP1, CP2 and the shots 101 and 102 - four points -
// and keeps the job, as one undo step (SURVEY IMPORT does the same from a
// line: src/katana_app/CMakeLists.txt, cli.survey_import_field_file).
TEST(SurveyImportWizard, AnOpcodeFieldFileIsImportedAsAJobWithItsDefaults)
{
    Session session;
    session.openAndRead(fixture("fld/setup.fld"));
    QWidget& w = *session.wizard;
    click(w, "next"); // System
    click(w, "next"); // Reduction and adjustment
    click(w, "next"); // Options
    click(w, "next"); // Report
    EXPECT_TRUE(session.step().contains("Report")) << session.step().toStdString();
    click(w, "import");

    ASSERT_EQ(session.document.surveyJobs().size(), 1u);
    const auto& job = session.document.surveyJobs().front();
    EXPECT_EQ(job.formatId, "opcode-field-file");
    EXPECT_EQ(job.sourceFileName, "setup.fld");
    EXPECT_EQ(job.placedPoints.size(), 4u);
    ASSERT_TRUE(session.document.undo().ok());
    EXPECT_TRUE(session.document.surveyJobs().empty());
}

TEST(SurveyImportWizard, ADelimitedCoordinateFileStillTakesItsSixSteps)
{
    Session session;
    child<QLineEdit>(*session.wizard, "file")
        ->setText(QString::fromStdString(fixture("survey_points_pnezd.csv")));
    click(*session.wizard, "next");
    choose(*session.wizard, "format", "Delimited text points (CSV, TXT)");
    click(*session.wizard, "next");
    EXPECT_TRUE(session.step().contains("Step 3 of 6: Columns and delimiter"))
        << session.step().toStdString();
}

// A GSI-16 job of `setups` setups of `shots` shots each, written the way the
// reader's own throughput test writes one: a station block, then a line per
// shot with Hz, V and a slope distance.
std::string syntheticGsi16(std::size_t setups, std::size_t shots)
{
    std::string out;
    out.reserve(setups * (shots + 1) * 100);
    std::size_t block = 0;
    for (std::size_t s = 0; s < setups; ++s) {
        out += std::format("*11{:04}+{:0>16} 84..16+{:016} 85..16+{:016} 86..16+{:016} "
                           "88..16+{:016} \n",
                           ++block % 10000, std::format("S{}", s), 10000000 + s * 1000,
                           20000000 + s * 1000, 500000, 15000);
        for (std::size_t i = 0; i < shots; ++i) {
            out += std::format("*11{:04}+{:0>16} 21.103+{:016} 22.103+{:016} 31..16+{:016} "
                               "87..16+{:016} \n",
                               ++block % 10000, std::format("P{}N{}", s, i),
                               (i * 1234567) % 36000000, 9000000 + (i % 100) * 1000,
                               100000 + (i % 5000) * 37, 15000);
        }
    }
    return out;
}

TEST(SurveyImportWizard, ALargeFileIsReadOffTheGuiThreadWhichKeepsAnsweringAndCanBeCancelled)
{
    // 8 MB by default so the test stays quick in a Debug build; set
    // KATANA_SURVEY_UI_MEGABYTES=50 to measure the 50 MB case.
    const QByteArray asked = qgetenv("KATANA_SURVEY_UI_MEGABYTES");
    const std::size_t megabytes = asked.isEmpty() ? 8 : static_cast<std::size_t>(asked.toInt());
    // About 100 bytes a line: 1 MB is some 10 000 lines.
    const std::size_t setups = megabytes * 50;
    const std::string bytes = syntheticGsi16(setups, 199);
    QTemporaryDir folder;
    ASSERT_TRUE(folder.isValid());
    const QString path = folder.filePath("big.gsi");
    {
        QFile file(path);
        ASSERT_TRUE(file.open(QIODevice::WriteOnly));
        ASSERT_EQ(file.write(bytes.data(), static_cast<qint64>(bytes.size())),
                  static_cast<qint64>(bytes.size()));
    }

    for (const bool cancel : {true, false}) {
        SCOPED_TRACE(cancel ? "cancelled" : "read to the end");
        Session session;
        QWidget& w = *session.wizard;
        child<QLineEdit>(w, "file")->setText(path);
        click(w, "next");
        // The GUI thread's longest wait between two ticks of a 10 ms timer,
        // from Next on the Format step until the Content step shows.
        QElapsedTimer clock;
        qint64 last = 0;
        qint64 longest = 0;
        QTimer tick;
        tick.setInterval(10);
        QObject::connect(&tick, &QTimer::timeout, [&] {
            const qint64 now = clock.elapsed();
            longest = std::max(longest, now - last);
            last = now;
        });
        clock.start();
        tick.start();
        click(w, "next");
        // Found as a QWidget and cast: findChild<T> needs T to carry
        // Q_OBJECT, which the application's classes never do (no moc).
        auto* bar = dynamic_cast<katana::qt::SurveyTaskBar*>(child<QWidget>(w, "task"));
        ASSERT_NE(bar, nullptr);
        EXPECT_TRUE(bar->busy()) << "a " << megabytes << " MB file is read in the background";
        EXPECT_FALSE(child<QPushButton>(w, "next")->isEnabled());
        if (cancel) {
            click(w, "cancelTask");
            EXPECT_FALSE(bar->busy());
            EXPECT_TRUE(session.step().contains("The file's format"));
            EXPECT_TRUE(child<QPushButton>(w, "next")->isEnabled());
            continue;
        }
        while (bar->busy() && clock.elapsed() < 600'000) {
            QApplication::processEvents(QEventLoop::AllEvents, 5);
        }
        const qint64 total = clock.elapsed();
        ASSERT_FALSE(bar->busy());
        EXPECT_TRUE(session.step().contains("What the file holds")) << session.step().toStdString();
        EXPECT_EQ(contentRows(w).at("Setups"), std::to_string(setups));
        std::printf("[ measured ] %zu MB read in %.2f s; the GUI thread's longest wait %lld ms\n",
                    bytes.size() >> 20, static_cast<double>(total) / 1000.0, longest);
        // A tenth of a second is where a person notices a stall; the read
        // itself takes seconds, so this is the thread doing its job.
        EXPECT_LT(longest, 250) << "the GUI thread waited " << longest << " ms";
    }
}
