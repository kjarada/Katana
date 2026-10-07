// The import wizard's instrument path, driven by its object names as a
// person clicks it: the Content step shows what each format's reader read
// (checked against the reader called directly, and against what the
// fixtures hold, counted by hand), a format with no published layout is
// refused on the Format step with what to export instead, an import with an
// adjustment is one undo step that leaves a survey job, and a large file is
// read off the GUI thread, which keeps answering.
//
// And the FINISH: with survey codes loaded the import codes its points and
// strings them inside its one command. The codes are the hand-written fixture
// tests/data/field_codes/test_field_codes.customisation.json, read by hand:
//   KB*   a LINE on FIELD KERB, linestyle "FIELD Kerb", colour "field kerb"
//   EB*   a LINE on FIELD EDGE, a plain line, colour "blue"
//   CTRL  a POINT on FIELD CONTROL, a plain line, colour "red"
// and no rule for anything else (it has no "*" rule). A style is named after
// its linestyle (cad/survey_coding.hpp), so a kerb's style is "FIELD Kerb".

#include <gtest/gtest.h>

#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QElapsedTimer>
#include <QFile>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QStringList>
#include <QTemporaryDir>
#include <QTimer>
#include <QTreeWidget>

#include <algorithm>
#include <filesystem>
#include <format>
#include <map>
#include <ostream>
#include <string>
#include <variant>
#include <vector>

#include "customisation/fixture_customisation.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/survey_job.hpp"
#include "katana/cad/survey_points.hpp"
#include "katana/core/path_text.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/entity.hpp"
#include "katana/survey/reduction_settings.hpp"
#include "katana/surveyio/reader.hpp"
#include "survey/survey_import_wizard.hpp"
#include "survey/survey_task.hpp"
#include "survey_verbs.hpp"

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
    // 03 record, gnss.fld none (RTK coordinates only), resection.fld a 128
    // and a 03, rtk_setup.fld one 03 on an RTK mark, traverse.sdr two live
    // 02 records (a third is deleted); a RINEX observation file has no
    // setups and one GNSS session, and its navigation file of the same name
    // (.24n) is read beside it.
    const struct {
        const char* file;
        const char* setups;
    } files[] = {{"leica/tps_gsi8.gsi", "2"},
                 {"trimble_jxl/one_setup.jxl", "1"},
                 {"rw5/setup_metres.rw5", "1"},
                 {"gts/setup.gt7", "1"},
                 {"fld/setup.fld", "1"},
                 {"fld/gnss.fld", "0"},
                 {"fld/resection.fld", "2"},
                 {"fld/rtk_setup.fld", "1"},
                 {"sdr/traverse.sdr", "2"},
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

TEST(SurveyImportWizard, TheBrowseFilterOffersEveryRegisteredExtensionAndTheUnregisteredOnes)
{
    // The filter was a hand-written list that had fallen behind the
    // registry (.sdr added by hand, .fld, .gts, .gts7 and .dat missing). It
    // is now built from the registry, so every extension a format names is
    // in it, and the patterns no format names stay.
    const QString filter = katana::qt::surveyFileFilter();
    const QString surveyFiles = filter.section(";;", 0, 0);
    ASSERT_TRUE(surveyFiles.startsWith("Survey files (") && surveyFiles.endsWith(")"))
        << filter.toStdString();
    QStringList offered = surveyFiles.mid(14, surveyFiles.size() - 15).split(' ');
    for (const surveyio::FormatDescriptor& descriptor : surveyio::formatRegistry().formats()) {
        for (const std::string& extension : descriptor.extensions) {
            EXPECT_TRUE(offered.contains(QString::fromStdString("*." + extension)))
                << descriptor.id << " ." << extension;
        }
    }
    for (const char* pattern : {"*.sdr", "*.fld", "*.gts", "*.gts7", "*.dat", "*.pnt", "*.xyz",
                                "*.gt6", "*.x01", "*.??o", "*.??d"}) {
        EXPECT_TRUE(offered.contains(pattern)) << pattern;
    }
    EXPECT_EQ(offered.removeDuplicates(), 0);
    EXPECT_TRUE(filter.endsWith(";;All files (*)"));
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

namespace {

// A job with what the clock decides taken out: when it was imported and when
// its report was made, which the report states.
katana::cad::SurveyJob timeless(katana::cad::SurveyJob job)
{
    for (const std::string& when : {job.reportCreatedUtc, job.importedUtc}) {
        if (when.empty()) {
            continue;
        }
        for (std::string* text : {&job.reportText, &job.reportHtml}) {
            for (std::size_t at = text->find(when); at != std::string::npos;
                 at = text->find(when, at)) {
                text->replace(at, when.size(), "<when>");
            }
        }
    }
    job.reportCreatedUtc.clear();
    job.importedUtc.clear();
    return job;
}

// The drawing's survey points, a line each, every number exactly.
std::string pointsText(const Document& document)
{
    std::string text;
    for (const katana::cad::DrawingSurveyPoint& point : katana::cad::drawingSurveyPoints(document)) {
        text += std::to_string(point.entity) + " " + point.id + " " + point.layer + " " +
                katana::core::formatExactReal(point.easting) + " " +
                katana::core::formatExactReal(point.northing) + " " +
                (point.elevation ? katana::core::formatExactReal(*point.elevation) : "-") + "\n";
    }
    return text;
}

} // namespace

// The wizard's Import and the SURVEY IMPORT line, given the same settings,
// make the same import: the same job - settings, points, report but for its
// time - and the same points on the drawing, bit for bit. The wizard runs its
// command itself rather than handing the line to the window's executor
// (docs/survey.md, "Not done"), so this is what holds the two together: the
// file with no coordinates, CP1 held where the drawing has it - picked from
// the drawing's points in the wizard, control=... on the line - and two
// settings changed from the defaults, the faces and the curvature and
// refraction.
TEST(SurveyImportWizard, ItsImportIsTheSurveyImportLinesForTheSameSettings)
{
    const std::string file = fixture("sdr/traverse_without_coordinates.sdr");
    const auto placeCp1 = [](Document& document) {
        katana::cad::CommandInterpreter interpreter(document);
        const auto placed = interpreter.run("FORWARD 500000,4999999,100 0 1 0 CP1");
        ASSERT_TRUE(placed.ok()) << placed.error().describe();
    };

    Session session;
    placeCp1(session.document);
    session.openAndRead(file);
    QWidget& w = *session.wizard;
    click(w, "next"); // System
    click(w, "next"); // Reduction and adjustment
    choose(w, "faces", "face left only");
    child<QCheckBox>(w, "curvatureRefraction")->setChecked(false);
    choose(w, "controlFrom", "the drawing");
    choose(w, "controlPick", "CP1");
    choose(w, "controlHorizontal", "fixed");
    choose(w, "controlVertical", "fixed");
    click(w, "addControl");
    click(w, "next"); // Options
    click(w, "next"); // Report
    ASSERT_TRUE(session.step().contains("Report")) << session.step().toStdString();
    click(w, "import");
    ASSERT_EQ(session.document.surveyJobs().size(), 1U);

    Document byLine;
    placeCp1(byLine);
    const auto reply = katana::app::runSurveyLine(
        byLine, "SURVEY IMPORT \"" + file +
                    "\" SET faces=face-left-only curvature_refraction=false "
                    "control=CP1;drawing;fixed;0;fixed;0;fixed;0");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    ASSERT_EQ(byLine.surveyJobs().size(), 1U);

    const katana::cad::SurveyJob wizard = timeless(session.document.surveyJobs().front());
    const katana::cad::SurveyJob line = timeless(byLine.surveyJobs().front());
    EXPECT_EQ(survey::serialiseReductionSettings(wizard.settings),
              survey::serialiseReductionSettings(line.settings));
    // The settings are the ones asked for, not merely equal to each other.
    survey::ReductionSettings asked;
    asked.faces = survey::FaceHandling::FaceLeftOnly;
    asked.curvatureAndRefraction = false;
    survey::ControlSelection cp1;
    cp1.point = survey::ControlPoint::fixed3d("CP1");
    cp1.origin = survey::ControlOrigin::Drawing;
    asked.control = {cp1};
    EXPECT_TRUE(wizard.settings == asked) << survey::serialiseReductionSettings(wizard.settings);
    EXPECT_EQ(wizard.placedPoints.size(), 3U); // CP2, T1, T2; CP1 is the drawing's
    EXPECT_EQ(wizard.reportText, line.reportText);
    EXPECT_TRUE(wizard == line);
    EXPECT_EQ(pointsText(session.document), pointsText(byLine));
    EXPECT_EQ(session.document.model().entities.size(), byLine.model().entities.size());
}

// The wizard reduces as the line does (cad::reduceForDrawing): with CP1 drawn
// at two places 100 km apart, holding it would put the job on whichever was
// drawn first, so its preview refuses, naming both - and nothing is imported.
TEST(SurveyImportWizard, ItsPreviewRefusesToHoldAPointTheDrawingHasAtTwoPlaces)
{
    Session session;
    {
        katana::cad::CommandInterpreter interpreter(session.document);
        for (const char* line : {"FORWARD 500000,4999999,100 0 1 0 CP1",
                                 "FORWARD 600000,4999999,100 0 1 0 CP1"}) {
            const auto placed = interpreter.run(line);
            ASSERT_TRUE(placed.ok()) << placed.error().describe();
        }
    }
    session.openAndRead(fixture("sdr/traverse_without_coordinates.sdr"));
    QWidget& w = *session.wizard;
    click(w, "next"); // System
    click(w, "next"); // Reduction and adjustment
    choose(w, "controlFrom", "the drawing");
    choose(w, "controlPick", "CP1");
    choose(w, "controlHorizontal", "fixed");
    choose(w, "controlVertical", "fixed");
    click(w, "addControl");
    click(w, "previewReduction");
    const QString message = child<QLabel>(w, "message")->text();
    EXPECT_TRUE(message.contains("Control point CP1 is on the drawing 2 times, at E 500000.0000 "
                                 "N 5000000.0000 Z 100.0000 and E 600000.0000 N 5000000.0000 "
                                 "Z 100.0000, so which one to hold is not known"))
        << message.toStdString();
    EXPECT_TRUE(session.document.surveyJobs().empty());
}

// ---- the finish: the survey codes and the linework ---------------------------------------

namespace {

using katana::entity::Entity;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

// The fixture of field codes (at the top of this file), loaded into the
// session as a person's customisation is. It has a folder of its own, beside
// the three of tests/data/customisation that the window's checks load whole.
void installFieldCodes(Document& document)
{
    const std::filesystem::path file =
        katana::qt::test::customisationFixtureDirectory().parent_path() / "field_codes" /
        "test_field_codes.customisation.json";
    const auto bytes = katana::core::readFileBytes(file);
    ASSERT_TRUE(bytes.ok()) << (bytes.ok() ? std::string() : bytes.error().describe());
    katana::qt::test::installCustomisations(document,
                                            {katana::qt::test::customisationFromText(*bytes)});
    ASSERT_EQ(document.surveyMap().size(), 3U);
}

// Everything of a drawing a survey import may change.
struct Drawing {
    std::vector<Entity> entities;
    std::vector<katana::entity::Layer> layers;
    std::vector<katana::entity::Style> styles;

    friend bool operator==(const Drawing&, const Drawing&) = default;
};

Drawing drawingOf(const Document& document)
{
    Drawing drawing;
    document.model().entities.forEach(
        [&](const Entity& entity) { drawing.entities.push_back(entity); });
    drawing.layers = document.model().layers.all();
    drawing.styles = document.model().styles.all();
    return drawing;
}

// What a failed comparison prints: enough to see what is left over.
void PrintTo(const Drawing& drawing, std::ostream* out)
{
    *out << drawing.entities.size() << " entities; layers:";
    for (const auto& layer : drawing.layers) {
        *out << " [" << layer.name << ']';
    }
    *out << " styles:";
    for (const auto& style : drawing.styles) {
        *out << " [" << style.name << ']';
    }
}

std::string textOf(const Entity& entity, const std::string& key)
{
    const auto found = entity.properties.find(key);
    if (found == entity.properties.end()) {
        return "<absent>";
    }
    const auto* text = std::get_if<std::string>(&found->second);
    return text != nullptr ? *text : "<not text>";
}

const Entity* pointNumbered(const Document& document, const std::string& number)
{
    const Entity* found = nullptr;
    document.model().entities.forEach([&](const Entity& entity) {
        if (std::holds_alternative<katana::entity::PointGeometry>(entity.geometry) &&
            textOf(entity, "point") == number) {
            found = &entity;
        }
    });
    return found;
}

std::vector<const Entity*> linesDrawn(const Document& document)
{
    std::vector<const Entity*> lines;
    document.model().entities.forEach([&](const Entity& entity) {
        if (std::holds_alternative<Polyline2>(entity.geometry)) {
            lines.push_back(&entity);
        }
    });
    return lines;
}

// The RTK job tests/surveyio/data/fld/gnss.fld through the wizard, from its
// File step to the Options step: File, Format (identified), Content, System,
// Reduction and adjustment.
void driveToOptions(Session& session, const std::string& file)
{
    session.openAndRead(file);
    QWidget& w = *session.wizard;
    click(w, "next"); // System
    click(w, "next"); // Reduction and adjustment
    click(w, "next"); // Options
    ASSERT_TRUE(session.step().contains("Options")) << session.step().toStdString();
}

// A point list in a folder of the test's, read as delimited points with its
// header naming the columns, and driven to the Options step in metres.
void drivePointListToOptions(Session& session, const QString& path)
{
    QWidget& w = *session.wizard;
    child<QLineEdit>(w, "file")->setText(path);
    click(w, "next"); // Format
    choose(w, "format", "Delimited text points (CSV, TXT)");
    click(w, "next"); // Columns and delimiter
    click(w, "next"); // Units and coordinate system
    choose(w, "unit", "metres");
    click(w, "next"); // Options
    ASSERT_TRUE(session.step().contains("Options")) << session.step().toStdString();
}

} // namespace

// The equivalence above, WITH survey codes: the fixture of field codes loaded
// into both drawings, and the RTK job gnss.fld, whose positions the file
// states (worked beside cli.survey_import_gnss_field_file and
// tests/app/test_survey_verbs.cpp):
//   CM1   entered, no code            E 500000  N 6200000
//   R001  KB, string 1                E 500010  N 6200020
//   R002  KB, string 1                E 500020  N 6200020
//   R003  KB, string 1                E 500020  N 6200030
//   20    closes the string KB 1
//   R004  KB, string 1, begun again   E 500040  N 6200040  (alone: no line)
// Both boxes start ticked - the customisation's two switches are on until
// something turns one off - so the wizard asks what the line asks when it
// says nothing: the five points, the four KB ones on FIELD KERB in the style
// FIELD Kerb, and the file's one closed string through R001, R002 and R003.
// The same job, the same entities, layers and styles, bit for bit.
TEST(SurveyImportWizard, WithSurveyCodesLoadedItsImportIsStillTheSurveyImportLines)
{
    const std::string file = fixture("fld/gnss.fld");

    Session session;
    installFieldCodes(session.document);
    driveToOptions(session, file);
    QWidget& w = *session.wizard;
    EXPECT_TRUE(child<QCheckBox>(w, "applyCodes")->isChecked());
    EXPECT_TRUE(child<QCheckBox>(w, "drawLinework")->isChecked());
    // A layer at the top of the tree, as the line below names it.
    child<QLineEdit>(w, "layer")->setText("fieldwork");
    click(w, "next"); // Report
    ASSERT_TRUE(session.step().contains("Report")) << session.step().toStdString();
    click(w, "import");
    ASSERT_EQ(session.document.surveyJobs().size(), 1U);

    Document byLine;
    installFieldCodes(byLine);
    const auto reply =
        katana::app::runSurveyLine(byLine, "SURVEY IMPORT \"" + file + "\" LAYER fieldwork");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    ASSERT_EQ(byLine.surveyJobs().size(), 1U);

    // The import worked by hand above, and not merely two equal drawings.
    const Document& drawn = session.document;
    EXPECT_EQ(drawn.model().entities.size(), 6U) << "five points and the one line";
    for (const char* number : {"R001", "R002", "R003", "R004"}) {
        const Entity* point = pointNumbered(drawn, number);
        ASSERT_NE(point, nullptr) << number;
        EXPECT_EQ(point->layer, "FIELD KERB") << number;
        EXPECT_EQ(point->style, "FIELD Kerb") << number;
    }
    const Entity* uncoded = pointNumbered(drawn, "CM1");
    ASSERT_NE(uncoded, nullptr);
    EXPECT_EQ(uncoded->layer, "fieldwork");
    EXPECT_EQ(uncoded->style, "");
    const std::vector<const Entity*> lines = linesDrawn(drawn);
    ASSERT_EQ(lines.size(), 1U);
    const Polyline2& kerb = std::get<Polyline2>(lines.front()->geometry);
    EXPECT_TRUE(kerb.closed) << "the file closed the string";
    EXPECT_EQ(kerb.vertices, (std::vector<Point2>{Point2(500010, 6200020), Point2(500020, 6200020),
                                                  Point2(500020, 6200030)}));
    EXPECT_EQ(lines.front()->layer, "FIELD KERB");
    EXPECT_EQ(lines.front()->style, "FIELD Kerb");

    // ... and the line's, to the last property of the last entity.
    const katana::cad::SurveyJob wizard = timeless(session.document.surveyJobs().front());
    const katana::cad::SurveyJob line = timeless(byLine.surveyJobs().front());
    EXPECT_EQ(wizard.importOptions, line.importOptions);
    EXPECT_EQ(wizard.reportText, line.reportText);
    EXPECT_TRUE(wizard == line);
    EXPECT_EQ(drawingOf(session.document), drawingOf(byLine));
    // The job remembers what it was asked, for its next re-adjustment.
    const auto stored = katana::cad::readSurveyJobOptions(wizard.importOptions, wizard.id);
    ASSERT_TRUE(stored.ok()) << stored.error().describe();
    EXPECT_TRUE(stored->applyCodes);
    EXPECT_TRUE(stored->drawLinework);
}

// The whole of it is ONE undo step, and the selection is left alone: it was
// two steps - the import, then Apply Survey Codes on the points just drawn,
// selected for the purpose - and no lines at all.
TEST(SurveyImportWizard, AFieldFilesImportWithItsCodesAndLinesIsOneUndoStep)
{
    Session session;
    installFieldCodes(session.document);
    Document& document = session.document;
    const Drawing before = drawingOf(document);
    const std::size_t steps = document.history().undoCount();

    driveToOptions(session, fixture("fld/gnss.fld"));
    QWidget& w = *session.wizard;
    child<QLineEdit>(w, "layer")->setText("fieldwork");
    click(w, "next"); // Report
    // Before the import the report says what is asked of the survey codes.
    const QString asked = child<QPlainTextEdit>(w, "report")->toPlainText();
    EXPECT_TRUE(asked.contains("Survey codes: applied to the imported points.")) << asked.toStdString();
    EXPECT_TRUE(asked.contains("Linework: the imported points are joined into lines"))
        << asked.toStdString();
    click(w, "import");

    EXPECT_EQ(document.history().undoCount(), steps + 1);
    EXPECT_EQ(document.model().entities.size(), 6U);
    EXPECT_TRUE(document.selection().empty()) << "nothing is selected to code it";
    // What the finish did, in cad::describe's sentences, by hand: five points
    // drawn, the four KB ones carry a code and all four have a rule (KB*),
    // one layer (FIELD KERB) and one style (FIELD Kerb) made; one line drawn;
    // the second KB 1, of one point, in no line; and CM1, with no code.
    const QString did = child<QPlainTextEdit>(w, "report")->toPlainText();
    for (const char* sentence :
         {"Survey codes were applied to the 5 point(s) drawn: 4 carry a code and 4 of those have "
          "a rule; 1 layer(s) and 1 style(s) were created.",
          "Linework: 1 line(s) drawn.", "1 string(s) of the file are in no line",
          "1 point(s) are in no line (no code: 1)."}) {
        EXPECT_TRUE(did.contains(sentence)) << sentence << "\n" << did.toStdString();
        EXPECT_TRUE(session.logged(sentence)) << sentence;
    }

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(drawingOf(document), before);
    EXPECT_TRUE(document.surveyJobs().empty());
    EXPECT_EQ(document.history().undoCount(), steps);
}

// A point list is coded and strung by its codes in its one command too. Five
// points, written here:
//   1, 2, 3  KB1   (500010,6200020) (500020,6200020) (500020,6200030)
//   4        CTRL  (500000,6200000)
//   5        ZZ    (500050,6200050)
// By the fixture: KB1 is answered by KB* - FIELD KERB, the style FIELD Kerb,
// a line, so 1, 2 and 3 in point order are one open line; CTRL is a point on
// FIELD CONTROL and in no line because it is a point code; ZZ has no rule,
// so point 5 stays on the import's layer, unstyled, in no line, and is named.
// A layer per field code is ticked as well, and is not used while the codes
// are applied: no layer is made beneath the import's.
TEST(SurveyImportWizard, APointListsImportWithItsCodesAndLinesIsOneUndoStep)
{
    QTemporaryDir folder;
    ASSERT_TRUE(folder.isValid());
    const QString path = folder.filePath("kerb.csv");
    {
        QFile file(path);
        ASSERT_TRUE(file.open(QIODevice::WriteOnly));
        file.write("Point,Northing,Easting,Elevation,Code\n"
                   "1,6200020.000,500010.000,12.500,KB1\n"
                   "2,6200020.000,500020.000,12.600,KB1\n"
                   "3,6200030.000,500020.000,12.700,KB1\n"
                   "4,6200000.000,500000.000,10.000,CTRL\n"
                   "5,6200050.000,500050.000,11.000,ZZ\n");
    }

    Session session;
    installFieldCodes(session.document);
    Document& document = session.document;
    const Drawing before = drawingOf(document);
    const std::size_t steps = document.history().undoCount();

    drivePointListToOptions(session, path);
    QWidget& w = *session.wizard;
    child<QLineEdit>(w, "layer")->setText("fieldwork");
    auto* codes = child<QCheckBox>(w, "applyCodes");
    auto* perCode = child<QCheckBox>(w, "layerPerCode");
    ASSERT_TRUE(codes->isChecked());
    codes->setChecked(false);
    ASSERT_TRUE(perCode->isEnabled());
    perCode->setChecked(true);
    codes->setChecked(true);
    EXPECT_FALSE(perCode->isEnabled());
    EXPECT_TRUE(perCode->isChecked()) << "the tick is kept for when the codes are off again";
    click(w, "next"); // Report
    click(w, "import");

    EXPECT_EQ(document.history().undoCount(), steps + 1);
    EXPECT_TRUE(document.selection().empty());
    EXPECT_EQ(document.model().entities.size(), 6U) << "five points and the one line";
    for (const char* number : {"1", "2", "3"}) {
        const Entity* point = pointNumbered(document, number);
        ASSERT_NE(point, nullptr) << number;
        EXPECT_EQ(point->layer, "FIELD KERB") << number;
        EXPECT_EQ(point->style, "FIELD Kerb") << number;
    }
    const Entity* mark = pointNumbered(document, "4");
    ASSERT_NE(mark, nullptr);
    EXPECT_EQ(mark->layer, "FIELD CONTROL");
    const Entity* stray = pointNumbered(document, "5");
    ASSERT_NE(stray, nullptr);
    EXPECT_EQ(stray->layer, "fieldwork");
    EXPECT_EQ(stray->style, "");
    const std::vector<const Entity*> lines = linesDrawn(document);
    ASSERT_EQ(lines.size(), 1U);
    const Polyline2& kerb = std::get<Polyline2>(lines.front()->geometry);
    EXPECT_FALSE(kerb.closed);
    EXPECT_EQ(kerb.vertices, (std::vector<Point2>{Point2(500010, 6200020), Point2(500020, 6200020),
                                                  Point2(500020, 6200030)}));
    EXPECT_EQ(lines.front()->layer, "FIELD KERB");
    EXPECT_EQ(lines.front()->style, "FIELD Kerb");
    // No layer per code: only the import's own and the two the rules name.
    for (const char* layer : {"fieldwork/KB1", "fieldwork/CTRL", "fieldwork/ZZ"}) {
        EXPECT_FALSE(document.model().layers.contains(layer)) << layer;
    }
    for (const char* layer : {"fieldwork", "FIELD KERB", "FIELD CONTROL"}) {
        EXPECT_TRUE(document.model().layers.contains(layer)) << layer;
    }

    // Said in the log and at the end of the report pane.
    const QString did = child<QPlainTextEdit>(w, "report")->toPlainText();
    for (const char* sentence :
         {"5 carry a code and 4 of those have a rule", "No rule for the code(s): ZZ.",
          "Linework: 1 line(s) drawn.", "2 point(s) are in no line"}) {
        EXPECT_TRUE(did.contains(sentence)) << sentence << "\n" << did.toStdString();
        EXPECT_TRUE(session.logged(sentence)) << sentence;
    }
    EXPECT_TRUE(session.logged("Imported 5 point(s) from kerb.csv, drawn on fieldwork and then "
                               "finished by the survey codes as follows"));

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(drawingOf(document), before);
    EXPECT_EQ(document.history().undoCount(), steps);
}

TEST(SurveyImportWizard, TheOptionsStepSaysHowSurveyCodesAreLoadedByTheLineThatLoadsThem)
{
    // The note under the Options step's boxes says what Import will do, and
    // with no survey codes loaded what brings some. It sent people to Format >
    // Load Customisation and called what that loaded "a survey code file": the
    // menu item is gone and that kind of file no longer loads. What loads
    // survey codes is a Katana customisation file: File > Settings > Import,
    // which exists since 2026-10-07 and shows the same menu path the person
    // would take (docs/desktop.md, "Settings"), or the CUSTOMISE <file> line -
    // the words CODE itself refuses with when none are loaded
    // (cad/survey_code_verbs.hpp, requireMap). Worked by hand from that
    // sentence, not from a run.
    Session session;
    driveToOptions(session, fixture("fld/gnss.fld"));
    const auto* note = child<QLabel>(*session.wizard, "finishNote");
    ASSERT_NE(note, nullptr);
    const QString text = note->text();
    EXPECT_TRUE(text.contains("Katana customisation file")) << text.toStdString();
    EXPECT_TRUE(text.contains("load one in File > Settings > Import")) << text.toStdString();
    EXPECT_TRUE(text.contains("type CUSTOMISE <file> on the command line"))
        << text.toStdString();
    EXPECT_FALSE(text.contains("Load Customisation")) << text.toStdString();
    EXPECT_FALSE(text.contains("survey code file")) << text.toStdString();
    EXPECT_FALSE(text.contains("Format >")) << text.toStdString();
    // Shown as it is written: "<file>" is not markup for the label to read.
    EXPECT_EQ(note->textFormat(), Qt::PlainText);
}

// The two boxes are the customisation's two switches when the Options step is
// first reached for a file, and the person's from then on; and a layer per
// field code is offered only while the codes are not applied, with the reason.
TEST(SurveyImportWizard, TheFinishBoxesStartAsTheCustomisationHasThemAndALayerPerCodeGivesWayToTheCodes)
{
    Session session;
    installFieldCodes(session.document);
    // Codes off, linework on: as CUSTOMISE SET auto.codes=off leaves them.
    katana::entity::CustomisationAutomation automation;
    automation.codesOnSurveyImport = false;
    automation.lineworkOnSurveyImport = true;
    session.document.setAutomation(automation);

    driveToOptions(session, fixture("fld/gnss.fld"));
    QWidget& w = *session.wizard;
    auto* codes = child<QCheckBox>(w, "applyCodes");
    auto* linework = child<QCheckBox>(w, "drawLinework");
    auto* perCode = child<QCheckBox>(w, "layerPerCode");
    auto* note = child<QLabel>(w, "finishNote");
    ASSERT_FALSE(codes == nullptr || linework == nullptr || perCode == nullptr || note == nullptr);
    EXPECT_TRUE(codes->isEnabled());
    EXPECT_TRUE(linework->isEnabled());
    EXPECT_FALSE(codes->isChecked());
    EXPECT_TRUE(linework->isChecked());
    EXPECT_TRUE(perCode->isEnabled()) << "the codes are not applied";
    EXPECT_TRUE(note->text().contains("Survey codes: not applied.")) << note->text().toStdString();
    EXPECT_TRUE(note->text().contains("Linework: the imported points are joined into lines"))
        << note->text().toStdString();
    EXPECT_FALSE(note->text().contains("Load Customisation")) << "no such menu item exists";

    // Ticked: the codes choose the layers, so a layer per code is not offered,
    // and its tip says why.
    codes->setChecked(true);
    EXPECT_FALSE(perCode->isEnabled());
    EXPECT_TRUE(perCode->toolTip().contains("the codes choose each point's layer"))
        << perCode->toolTip().toStdString();
    EXPECT_TRUE(note->text().contains("Survey codes: applied to the imported points."))
        << note->text().toStdString();

    // Back and forth over the same file keeps what the person ticked: the
    // customisation's switch still says off.
    click(w, "back");
    click(w, "next");
    ASSERT_TRUE(session.step().contains("Options")) << session.step().toStdString();
    EXPECT_TRUE(codes->isChecked());

    // Unticked again, a layer per code is offered again.
    codes->setChecked(false);
    EXPECT_TRUE(perCode->isEnabled());
    EXPECT_FALSE(perCode->toolTip().contains("the codes choose each point's layer"));

    // Imported so - codes off, linework on - the job is kept as one that was
    // strung and not coded: its points stay on the import's layer, unstyled,
    // and the file's closed string is drawn on its rule's layer.
    child<QLineEdit>(w, "layer")->setText("fieldwork");
    click(w, "next"); // Report
    click(w, "import");
    ASSERT_EQ(session.document.surveyJobs().size(), 1U);
    const katana::cad::SurveyJob& job = session.document.surveyJobs().front();
    const auto stored = katana::cad::readSurveyJobOptions(job.importOptions, job.id);
    ASSERT_TRUE(stored.ok()) << stored.error().describe();
    EXPECT_FALSE(stored->applyCodes);
    EXPECT_TRUE(stored->drawLinework);
    const Entity* shot = pointNumbered(session.document, "R001");
    ASSERT_NE(shot, nullptr);
    EXPECT_EQ(shot->layer, "fieldwork");
    EXPECT_EQ(shot->style, "");
    const std::vector<const Entity*> lines = linesDrawn(session.document);
    ASSERT_EQ(lines.size(), 1U);
    EXPECT_EQ(lines.front()->layer, "FIELD KERB");
}

// With no survey codes there is nothing to apply or to string by: both boxes
// are disabled and unticked, the note says why in today's words, a layer per
// code is offered as it always was, and the import is the plain one - which
// every test above this section runs.
TEST(SurveyImportWizard, WithNoSurveyCodesTheFinishBoxesAreDisabledAndTheNoteSaysWhy)
{
    Session session;
    driveToOptions(session, fixture("fld/gnss.fld"));
    QWidget& w = *session.wizard;
    auto* codes = child<QCheckBox>(w, "applyCodes");
    auto* linework = child<QCheckBox>(w, "drawLinework");
    auto* note = child<QLabel>(w, "finishNote");
    ASSERT_FALSE(codes == nullptr || linework == nullptr || note == nullptr);
    EXPECT_FALSE(codes->isEnabled());
    EXPECT_FALSE(linework->isEnabled());
    EXPECT_FALSE(codes->isChecked());
    EXPECT_FALSE(linework->isChecked());
    EXPECT_TRUE(child<QCheckBox>(w, "layerPerCode")->isEnabled());
    EXPECT_TRUE(note->text().contains("Survey codes: the session has none")) << note->text().toStdString();
    EXPECT_FALSE(note->text().contains("Load Customisation"));
    EXPECT_FALSE(note->text().contains("survey code file"));

    child<QLineEdit>(w, "layer")->setText("fieldwork");
    click(w, "next"); // Report
    click(w, "import");
    // The five points on the import's layer and nothing else; a job that was
    // asked for neither step.
    EXPECT_EQ(session.document.model().entities.size(), 5U);
    EXPECT_TRUE(linesDrawn(session.document).empty());
    ASSERT_EQ(session.document.surveyJobs().size(), 1U);
    const katana::cad::SurveyJob& job = session.document.surveyJobs().front();
    const auto stored = katana::cad::readSurveyJobOptions(job.importOptions, job.id);
    ASSERT_TRUE(stored.ok()) << stored.error().describe();
    EXPECT_FALSE(stored->applyCodes);
    EXPECT_FALSE(stored->drawLinework);

    // Survey codes loaded since, and another file read: the boxes are offered
    // and start as the customisation has them switched - both on.
    installFieldCodes(session.document);
    session.wizard->show();
    driveToOptions(session, fixture("fld/gnss.fld"));
    EXPECT_TRUE(codes->isEnabled());
    EXPECT_TRUE(codes->isChecked());
    EXPECT_TRUE(linework->isEnabled());
    EXPECT_TRUE(linework->isChecked());
    EXPECT_FALSE(child<QCheckBox>(w, "layerPerCode")->isEnabled());
}
