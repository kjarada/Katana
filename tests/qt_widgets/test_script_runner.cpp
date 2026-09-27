// File > Run Script and the SCRIPT verb (src/katana_qt/script_runner.hpp):
// how a script is read, how its lines are run and when the run stops, and the
// dialog, driven by its object names as a person drives it.
//
// The runners here are the tests' own - they record the lines and refuse the
// ones a test names - so what is tested is the order, the stopping and the
// record, not the verbs. The headless qt_script_* checks run real scripts in
// the real window.

#include <gtest/gtest.h>

#include <QApplication>
#include <QCheckBox>
#include <QDir>
#include <QFile>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QTemporaryDir>

#include <vector>

#include "script_runner.hpp"

namespace {

using katana::core::ErrorCode;
using katana::qt::CommandRunner;
using katana::qt::ScriptDialogContext;
using katana::qt::ScriptLine;
using katana::qt::ScriptOptions;
using katana::qt::ScriptReport;
using katana::qt::ScriptRunDialog;
using katana::qt::VerbOutcome;

const QString kScripts = QStringLiteral(KATANA_QT_WIDGET_DATA "/scripts");

// Records each line it is given, and refuses those in `refused`.
struct RecordingRunner {
    std::vector<QString> lines;
    QStringList refused;

    CommandRunner runner()
    {
        return [this](const QString& line) {
            lines.push_back(line);
            if (refused.contains(line)) {
                return VerbOutcome{false, {}, "refused: " + line};
            }
            return VerbOutcome{true, "ran " + line, {}};
        };
    }
};

std::vector<ScriptLine> linesOf(std::initializer_list<const char*> texts)
{
    std::vector<ScriptLine> lines;
    int number = 0;
    for (const char* text : texts) {
        lines.push_back({++number, QString::fromLatin1(text)});
    }
    return lines;
}

TEST(ScriptRunner, BlankLinesAndCommentsAreSkippedAndTheFileLineNumbersKept)
{
    const auto lines = katana::qt::scriptLines("# heading\n\nRECT 0,0 10,5\n   # indented\n"
                                               "  CIRCLE 5,5 2  \n\t\nLIST");
    ASSERT_EQ(lines.size(), 3U);
    EXPECT_EQ(lines[0].number, 3);
    EXPECT_EQ(lines[0].text, "RECT 0,0 10,5");
    EXPECT_EQ(lines[1].number, 5);
    EXPECT_EQ(lines[1].text, "CIRCLE 5,5 2");
    EXPECT_EQ(lines[2].number, 7);
    EXPECT_EQ(lines[2].text, "LIST");
}

TEST(ScriptRunner, WindowsAndOldMacLineEndsAreLineEnds)
{
    const auto crlf = katana::qt::scriptLines("LINE 0,0 1,1\r\nLIST\r\n");
    ASSERT_EQ(crlf.size(), 2U);
    EXPECT_EQ(crlf[0].text, "LINE 0,0 1,1");
    EXPECT_EQ(crlf[1].number, 2);
    const auto cr = katana::qt::scriptLines("LINE 0,0 1,1\rLIST");
    ASSERT_EQ(cr.size(), 2U);
    EXPECT_EQ(cr[1].text, "LIST");
}

TEST(ScriptRunner, TextWithNoCommandsHasNoLines)
{
    EXPECT_TRUE(katana::qt::scriptLines("").empty());
    EXPECT_TRUE(katana::qt::scriptLines("\n\n# only a comment\n   \n").empty());
}

TEST(ScriptRunner, AnInfoOfAnAnchoredIdIsACommandNotAComment)
{
    EXPECT_TRUE(katana::qt::isScriptComment("  # a note"));
    EXPECT_FALSE(katana::qt::isScriptComment("INFO #12"));
}

TEST(ScriptRunner, TheFixtureScriptIsReadAsKatanaCliReadsIt)
{
    const auto lines = katana::qt::readScript(kScripts + "/good.kcs");
    ASSERT_TRUE(lines.ok()) << lines.error().describe();
    // good.kcs: three comment lines, then commands on lines 4, 6 and 8.
    ASSERT_EQ(lines->size(), 3U);
    EXPECT_EQ((*lines)[0].number, 4);
    EXPECT_EQ((*lines)[0].text, "RECT 0,0 10,5");
    EXPECT_EQ((*lines)[1].number, 6);
    EXPECT_EQ((*lines)[2].number, 8);
    EXPECT_EQ((*lines)[2].text, "LINE 0,0 10,0");
}

TEST(ScriptRunner, AFileWrittenWithWindowsLineEndsReadsTheSame)
{
    QTemporaryDir folder;
    ASSERT_TRUE(folder.isValid());
    const QString path = folder.filePath("crlf.kcs");
    QFile file(path);
    ASSERT_TRUE(file.open(QIODevice::WriteOnly));
    file.write("# written on Windows\r\nRECT 0,0 10,5\r\n\r\nLIST\r\n");
    file.close();
    const auto lines = katana::qt::readScript(path);
    ASSERT_TRUE(lines.ok());
    ASSERT_EQ(lines->size(), 2U);
    EXPECT_EQ((*lines)[0].text, "RECT 0,0 10,5");
    EXPECT_EQ((*lines)[1].number, 4);
    EXPECT_EQ((*lines)[1].text, "LIST");
}

TEST(ScriptRunner, AMissingScriptIsNotFoundByItsPath)
{
    const auto lines = katana::qt::readScript(kScripts + "/no_such_script.kcs");
    ASSERT_FALSE(lines.ok());
    EXPECT_EQ(lines.error().code, ErrorCode::NotFound);
    EXPECT_NE(lines.error().describe().find("no_such_script.kcs"), std::string::npos);
}

TEST(ScriptRunner, TheScriptVerbReadsAQuotedPathAndContinue)
{
    const auto quoted = katana::qt::parseScriptCommand("SCRIPT \"C:/my scripts/site plan.kcs\"");
    ASSERT_TRUE(quoted.ok());
    EXPECT_EQ(quoted->path, "C:/my scripts/site plan.kcs");
    EXPECT_FALSE(quoted->continueOnError);
    const auto going = katana::qt::parseScriptCommand("script setup.kcs continue");
    ASSERT_TRUE(going.ok());
    EXPECT_EQ(going->path, "setup.kcs");
    EXPECT_TRUE(going->continueOnError);
}

TEST(ScriptRunner, TheScriptVerbRefusesNoFileAndAWordItDoesNotKnow)
{
    for (const char* line : {"SCRIPT", "SCRIPT \"\"", "SCRIPT a.kcs AGAIN", "SCRIPT a b CONTINUE",
                             "SCRIPT \"never closed"}) {
        const auto command = katana::qt::parseScriptCommand(line);
        EXPECT_FALSE(command.ok()) << line;
    }
    const auto usage = katana::qt::parseScriptCommand("SCRIPT");
    EXPECT_NE(usage.error().describe().find("usage: SCRIPT <file.kcs> [CONTINUE]"),
              std::string::npos);
}

TEST(ScriptRunner, TheLineItWritesIsTheLineItReads)
{
    // The dialog hands over the path the platform's file dialog gave, in the
    // platform's own separators: "C:\my scripts\site.kcs" on Windows. The
    // line writes it with '/', which every platform's file API reads. On a
    // POSIX system '\' is an ordinary character of a file name, not a
    // separator (POSIX.1-2017, 3.170 "Filename"), so a Windows path is not a
    // native path there and the test gives each platform its own.
    const QString line = katana::qt::scriptCommandLine(
        QDir::toNativeSeparators("C:/my scripts/site.kcs"), true);
    EXPECT_EQ(line, "SCRIPT \"C:/my scripts/site.kcs\" CONTINUE");
    const auto command = katana::qt::parseScriptCommand(line);
    ASSERT_TRUE(command.ok());
    EXPECT_EQ(command->path, "C:/my scripts/site.kcs");
    EXPECT_TRUE(command->continueOnError);
    EXPECT_EQ(katana::qt::scriptCommandLine("a.kcs", false), "SCRIPT \"a.kcs\"");
}

TEST(ScriptRunner, EveryLineRunsInOrderWhenNoneIsRefused)
{
    RecordingRunner recording;
    const auto lines = linesOf({"RECT 0,0 10,5", "CIRCLE 5,5 2", "LIST"});
    const ScriptReport report = katana::qt::runScriptLines(lines, recording.runner(), {});
    EXPECT_EQ(recording.lines, (std::vector<QString>{"RECT 0,0 10,5", "CIRCLE 5,5 2", "LIST"}));
    EXPECT_EQ(report.lines, 3);
    EXPECT_EQ(report.ran, 3);
    EXPECT_EQ(report.failed, 0);
    EXPECT_EQ(report.stoppedAt, 0);
    EXPECT_TRUE(report.ok());
    EXPECT_EQ(katana::qt::formatScriptReport("a.kcs", report),
              "script=\"a.kcs\" lines=3 ran=3 failed=0");
}

TEST(ScriptRunner, TheRunStopsAtTheFirstRefusedLine)
{
    RecordingRunner recording;
    recording.refused = {"CIRCLE 0,0 -1"};
    const auto lines = linesOf({"RECT 0,0 10,5", "CIRCLE 0,0 -1", "LINE 0,0 10,0"});
    const ScriptReport report = katana::qt::runScriptLines(lines, recording.runner(), {});
    EXPECT_EQ(recording.lines.size(), 2U) << "the line after the refused one must not run";
    EXPECT_EQ(report.ran, 2);
    EXPECT_EQ(report.failed, 1);
    EXPECT_EQ(report.stoppedAt, 2);
    EXPECT_FALSE(report.ok());
    EXPECT_EQ(katana::qt::formatScriptReport("bad.kcs", report),
              "script=\"bad.kcs\" lines=3 ran=2 failed=1 stopped_at=2");
}

TEST(ScriptRunner, ContinuingRunsEveryLineAndCountsTheRefused)
{
    RecordingRunner recording;
    recording.refused = {"CIRCLE 0,0 -1", "BOGUS"};
    const auto lines = linesOf({"RECT 0,0 10,5", "CIRCLE 0,0 -1", "BOGUS", "LINE 0,0 10,0"});
    ScriptOptions options;
    options.continueOnError = true;
    const ScriptReport report = katana::qt::runScriptLines(lines, recording.runner(), options);
    EXPECT_EQ(recording.lines.size(), 4U);
    EXPECT_EQ(report.ran, 4);
    EXPECT_EQ(report.failed, 2);
    EXPECT_EQ(report.stoppedAt, 0);
    EXPECT_FALSE(report.ok());
}

TEST(ScriptRunner, QuitEndsTheScriptWithoutFailingItAsInKatanaCli)
{
    RecordingRunner recording;
    const auto lines = linesOf({"RECT 0,0 10,5", "quit now", "LIST"});
    const ScriptReport report = katana::qt::runScriptLines(lines, recording.runner(), {});
    EXPECT_EQ(recording.lines, (std::vector<QString>{"RECT 0,0 10,5"}))
        << "QUIT is never run: it would close the window under the person who ran the script";
    EXPECT_TRUE(report.quit);
    EXPECT_TRUE(report.ok());
    EXPECT_EQ(report.stoppedAt, 2);
    EXPECT_EQ(katana::qt::formatScriptReport("", report),
              "script=pasted lines=3 ran=1 failed=0 quit_at=2");
}

TEST(ScriptRunner, CancellingStopsBeforeTheNextLine)
{
    RecordingRunner recording;
    const auto lines = linesOf({"RECT 0,0 10,5", "CIRCLE 5,5 2", "LIST"});
    ScriptOptions options;
    std::vector<std::pair<int, int>> asked;
    options.progress = [&asked](int done, int total) {
        asked.emplace_back(done, total);
        return done < 2; // Cancel pressed while the second line ran
    };
    const ScriptReport report = katana::qt::runScriptLines(lines, recording.runner(), options);
    EXPECT_EQ(recording.lines.size(), 2U);
    EXPECT_EQ(asked, (std::vector<std::pair<int, int>>{{0, 3}, {1, 3}, {2, 3}}));
    EXPECT_TRUE(report.cancelled);
    EXPECT_FALSE(report.ok());
    EXPECT_EQ(report.stoppedAt, 3);
    EXPECT_EQ(katana::qt::formatScriptReport("a.kcs", report),
              "script=\"a.kcs\" lines=3 ran=2 failed=0 cancelled_at=3");
}

TEST(ScriptRunner, AnEmptyScriptRunsNothingAndIsOk)
{
    RecordingRunner recording;
    const ScriptReport report = katana::qt::runScriptLines({}, recording.runner(), {});
    EXPECT_TRUE(recording.lines.empty());
    EXPECT_EQ(report.lines, 0);
    EXPECT_TRUE(report.ok());
}

// ---- the dialog -----------------------------------------------------------------------------

template <typename T> T* child(QWidget& parent, const char* name)
{
    T* found = parent.findChild<T*>(QString::fromLatin1(name));
    EXPECT_NE(found, nullptr) << "no " << name;
    return found;
}

TEST(ScriptRunDialog, ItShowsTheScriptsCommandsAndRunsTheLineItShows)
{
    RecordingRunner recording;
    ScriptDialogContext context;
    context.run = recording.runner();
    ScriptRunDialog dialog(std::move(context));
    EXPECT_EQ(dialog.objectName(), "fileRunScriptDialog");
    auto* run = child<QPushButton>(dialog, "scriptRun");
    ASSERT_NE(run, nullptr);
    EXPECT_FALSE(run->isEnabled()) << "nothing to run before a script is named";

    child<QLineEdit>(dialog, "scriptPath")->setText(kScripts + "/good.kcs");
    const QString preview = child<QPlainTextEdit>(dialog, "scriptPreview")->toPlainText();
    EXPECT_EQ(preview, "   4  RECT 0,0 10,5\n   6  CIRCLE 5,5 2\n   8  LINE 0,0 10,0");
    child<QCheckBox>(dialog, "scriptContinueOnError")->setChecked(true);
    const QString expected = "SCRIPT \"" + kScripts + "/good.kcs\" CONTINUE";
    EXPECT_EQ(child<QLineEdit>(dialog, "scriptCommand")->text(), expected);

    ASSERT_TRUE(run->isEnabled());
    run->click();
    EXPECT_EQ(recording.lines, (std::vector<QString>{expected}));
    EXPECT_EQ(child<QLabel>(dialog, "scriptStatus")->text(), "Done: ran " + expected);
}

TEST(ScriptRunDialog, ARefusalIsSaidInTheStatus)
{
    RecordingRunner recording;
    const QString line = "SCRIPT \"" + kScripts + "/bad.kcs\"";
    recording.refused = {line};
    ScriptDialogContext context;
    context.run = recording.runner();
    ScriptRunDialog dialog(std::move(context));
    dialog.setScriptPath(kScripts + "/bad.kcs");
    dialog.run();
    EXPECT_EQ(child<QLabel>(dialog, "scriptStatus")->text(), "Stopped: refused: " + line);
}

TEST(ScriptRunDialog, AFileThatCannotBeReadSaysSoInThePreview)
{
    ScriptRunDialog dialog(ScriptDialogContext{});
    child<QLineEdit>(dialog, "scriptPath")->setText(kScripts + "/no_such_script.kcs");
    EXPECT_TRUE(child<QPlainTextEdit>(dialog, "scriptPreview")
                    ->toPlainText()
                    .startsWith("NotFound: no such script"));
    // The line is still the verb's to refuse, by its path, once it runs.
    EXPECT_TRUE(child<QPushButton>(dialog, "scriptRun")->isEnabled());
}

TEST(ScriptRunDialog, AHeadlessSessionIsToldToFillThePathRatherThanBrowse)
{
    ScriptDialogContext context;
    context.headless = [] { return true; };
    ScriptRunDialog dialog(std::move(context));
    child<QPushButton>(dialog, "scriptBrowse")->click();
    EXPECT_EQ(child<QLabel>(dialog, "scriptStatus")->text(),
              "A headless session opens no file dialog: fill scriptPath with the path instead.");
}

} // namespace
