// The Sheets editor's Sheet Set menu and Page Setup
// (src/katana_qt/plotting/sheet_set_menu.cpp, docs/plotting.md "The Sheet
// Set menu and Page Setup"): the set saved to a JSON file and loaded back,
// another set's sheets appended, the set copied as JSON, and the page setup
// changed without a plot - each item by its object name, each the SHEETS
// line a person would type run through the editor (SheetEditor::runLine), so
// each edit is ONE undoable step. Driven on the offscreen platform, the file
// dialogs and the question answered as a person answers them.

#include <gtest/gtest.h>

#include <filesystem>
#include <string>
#include <vector>

#include <QAction>
#include <QApplication>
#include <QCheckBox>
#include <QClipboard>
#include <QComboBox>
#include <QDialog>
#include <QDir>
#include <QDoubleSpinBox>
#include <QFileDialog>
#include <QGuiApplication>
#include <QLabel>
#include <QLineEdit>
#include <QMenu>
#include <QMenuBar>
#include <QMessageBox>
#include <QPushButton>
#include <QSpinBox>
#include <QTimer>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/plotting/sheet_json.hpp"
#include "katana/cad/plotting/sheet_verbs.hpp"
#include "plotting/plot_dialog.hpp"
#include "sheet_editor.hpp"
#include "widget_harness.hpp"

namespace fs = std::filesystem;
using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::qt::SheetEditor;
using katana::qt::SheetSource;
namespace plotting = katana::cad::plotting;

namespace {

SheetEditor::SourceProvider sourceOf(const Document& document)
{
    return [&document] {
        SheetSource source;
        source.plan = katana::qt::planSourceOf(document);
        source.document = &document;
        source.revision = document.modelRevision();
        return source;
    };
}

// Lines typed into `document`, as the test's own set-up.
void type(Document& document, std::initializer_list<const char*> lines)
{
    CommandInterpreter interpreter(document);
    for (const char* line : lines) {
        const auto reply = interpreter.run(line);
        ASSERT_TRUE(reply.ok()) << line << ": " << reply.error().describe();
    }
}

struct ScratchDirectory {
    fs::path path;
    explicit ScratchDirectory(const std::string& name)
        : path(fs::temp_directory_path() / ("katana-sheet-set-menu-" + name))
    {
        fs::remove_all(path);
        fs::create_directories(path);
    }
    ~ScratchDirectory() { fs::remove_all(path); }
    [[nodiscard]] QString file(const char* name) const
    {
        return QString::fromStdWString((path / name).wstring());
    }
};

struct Shown {
    explicit Shown(Document& document) : editor(document, sourceOf(document))
    {
        editor.onMessage = [this](const QString& text, bool error) {
            messages.push_back(text);
            errors += error ? 1 : 0;
        };
        editor.resize(1400, 900);
        editor.show();
        katana::qt::test::processEvents();
    }
    void trigger(const char* name)
    {
        auto* action = editor.findChild<QAction*>(QString::fromLatin1(name));
        ASSERT_NE(action, nullptr) << name;
        action->trigger();
        katana::qt::test::processEvents();
    }
    [[nodiscard]] bool said(const QString& start) const
    {
        for (const QString& message : messages) {
            if (message.startsWith(start)) {
                return true;
            }
        }
        return false;
    }
    SheetEditor editor;
    std::vector<QString> messages;
    int errors = 0;
};

// Page Setup, found by its object name; PlotDialog has no Q_OBJECT of its
// own, so it is found as the QDialog it is.
katana::qt::PlotDialog* pageSetupDialogOf(const SheetEditor& editor)
{
    return dynamic_cast<katana::qt::PlotDialog*>(
        editor.findChild<QDialog*>(QStringLiteral("pageSetupDialog")));
}

// The modal boxes that come up, answered in turn: each file dialog with the
// next of `files`, each question with the next of `buttons`.
struct Answers {
    QTimer timer;
    QStringList files;
    std::vector<QMessageBox::StandardButton> buttons;
    int filesSeen = 0;
    std::size_t questionsSeen = 0;
    QString question;
    QString questionName;

    Answers(QStringList chosen, std::vector<QMessageBox::StandardButton> answers = {})
        : files(std::move(chosen)), buttons(std::move(answers))
    {
        timer.setInterval(10);
        QObject::connect(&timer, &QTimer::timeout, [this] {
            QWidget* modal = QApplication::activeModalWidget();
            if (auto* dialog = qobject_cast<QFileDialog*>(modal)) {
                if (filesSeen < files.size()) {
                    katana::qt::test::chooseFile(*dialog, files[filesSeen++]);
                } else {
                    dialog->reject();
                }
            } else if (auto* box = qobject_cast<QMessageBox*>(modal)) {
                question = box->text();
                questionName = box->objectName();
                const auto answer = questionsSeen < buttons.size() ? buttons[questionsSeen]
                                                                    : QMessageBox::Cancel;
                ++questionsSeen;
                box->button(answer)->click();
            }
        });
        timer.start();
    }
};

} // namespace

// The answer the tests below give a file dialog (widget_harness.hpp's
// chooseFile) chooses the file whether or not the dialog has been activated
// yet. Once it has, its file-name box has the focus, and QFileDialog::
// selectFile leaves a focused box alone: the answer then chose nothing, and
// which came first - the activation or the poll - was down to the load on
// the machine. Here the dialog is made active first, the late poll's case,
// every time.
TEST(FileDialogAnswer, TheFileIsChosenWhenTheNameBoxAlreadyHasTheFocus)
{
    const ScratchDirectory scratch("focus");
    const QString file = scratch.file("chosen.json");
    ASSERT_TRUE(plotting::writeSheetSetFile(plotting::SheetSet{}, scratch.path / "chosen.json").ok());
    QFileDialog dialog(nullptr, QStringLiteral("Load Sheet Set"),
                       QString::fromStdWString(scratch.path.wstring()),
                       QStringLiteral("Sheet sets (*.json)"));
    dialog.setFileMode(QFileDialog::ExistingFile);
    ASSERT_TRUE(katana::qt::test::showActive(dialog));
    ASSERT_NE(qobject_cast<QLineEdit*>(QApplication::focusWidget()), nullptr)
        << "the file-name box has the focus, as it does once the dialog is active";
    katana::qt::test::chooseFile(dialog, file);
    EXPECT_EQ(dialog.result(), QDialog::Accepted);
    // QFileDialog gives its choice with '/', Qt's one separator on every
    // platform (QDir: "Qt uses '/' as a universal directory separator"),
    // while the scratch path is native, '\' on Windows.
    EXPECT_EQ(dialog.selectedFiles().join('|').toStdString(),
              QDir::fromNativeSeparators(file).toStdString());
}

TEST(SheetSetMenu, TheMenuComesFirstWithEveryItemByName)
{
    Document document;
    Shown shown(document);
    const QList<QAction*> menus = shown.editor.menuBar()->actions();
    ASSERT_FALSE(menus.isEmpty());
    QMenu* first = menus.front()->menu();
    ASSERT_NE(first, nullptr);
    EXPECT_EQ(first->objectName(), QStringLiteral("sheetSetMenu"));
    EXPECT_EQ(QString(first->title()).remove('&'), QStringLiteral("Sheet Set"));
    for (const char* name :
         {"sheetSaveSet", "sheetLoadSet", "sheetAppendSet", "sheetCopySetJson", "sheetPageSetup"}) {
        auto* action = shown.editor.findChild<QAction*>(QString::fromLatin1(name));
        ASSERT_NE(action, nullptr) << name;
        EXPECT_TRUE(first->actions().contains(action)) << name;
        EXPECT_FALSE(action->statusTip().isEmpty()) << name;
    }
}

TEST(SheetSetMenu, TheSetIsSavedAndLoadedBackAskingFirst)
{
    const ScratchDirectory scratch("round-trip");
    Document document;
    type(document, {"SHEET NEW PLAN", "VIEW ADD 1 notes", "SHEET NEW KEY",
                    "TITLEBLOCK organisation \"Example Surveys\""});
    const plotting::SheetSet saved = document.sheetSet();
    Shown shown(document);
    const std::size_t steps = document.history().undoCount();
    {
        Answers answers({scratch.file("set one.json")});
        shown.trigger("sheetSaveSet");
        EXPECT_EQ(answers.filesSeen, 1);
    }
    const auto read = plotting::readSheetSetFile(scratch.path / "set one.json");
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_EQ(*read, saved);
    EXPECT_TRUE(shown.said(QStringLiteral("> SHEETS SAVE \"")));
    EXPECT_TRUE(shown.said(QStringLiteral("wrote 2 sheets to ")));
    EXPECT_EQ(document.history().undoCount(), steps) << "saving is no edit";

    // Changed since, then loaded: asked first, and one step undo takes back.
    type(document, {"SHEET NEW SCRAP"});
    const plotting::SheetSet changed = document.sheetSet();
    {
        Answers answers({scratch.file("set one.json")}, {QMessageBox::Yes});
        shown.trigger("sheetLoadSet");
        EXPECT_EQ(answers.questionsSeen, 1u);
        EXPECT_EQ(answers.questionName, QStringLiteral("sheetLoadSetQuestion"));
        EXPECT_EQ(answers.question,
                  QStringLiteral("Replace the 3 sheets there are now? Undo brings them back."));
    }
    EXPECT_EQ(document.sheetSet(), saved);
    EXPECT_EQ(document.history().undoName(), "LOAD_SHEETS");
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.sheetSet(), changed);

    // Cancelled at the question: nothing is loaded.
    const std::size_t before = document.history().undoCount();
    {
        Answers answers({scratch.file("set one.json")}, {QMessageBox::Cancel});
        shown.trigger("sheetLoadSet");
        EXPECT_EQ(answers.questionsSeen, 1u);
    }
    EXPECT_EQ(document.sheetSet(), changed);
    EXPECT_EQ(document.history().undoCount(), before);
}

TEST(SheetSetMenu, AnotherSetsSheetsAreAppendedInOneStep)
{
    const ScratchDirectory scratch("append");
    {
        Document other;
        type(other, {"SHEET NEW NORTH", "VIEW ADD 1 notes", "SHEET NEW SOUTH"});
        ASSERT_TRUE(plotting::writeSheetSetFile(other.sheetSet(), scratch.path / "other.json").ok());
    }
    Document document;
    type(document, {"SHEET NEW COVER", "VIEW ADD 1 notes"});
    Shown shown(document);
    const std::size_t steps = document.history().undoCount();
    {
        Answers answers({scratch.file("other.json")});
        shown.trigger("sheetAppendSet");
        EXPECT_EQ(answers.filesSeen, 1);
        EXPECT_EQ(answers.questionsSeen, 0u) << "appending replaces nothing, so asks nothing";
    }
    const plotting::SheetSet& set = document.sheetSet();
    ASSERT_EQ(set.sheets.size(), 3u);
    EXPECT_EQ(set.sheets[0].name, "COVER");
    EXPECT_EQ(set.sheets[1].name, "NORTH");
    EXPECT_EQ(set.sheets[2].name, "SOUTH");
    EXPECT_NE(set.sheets[1].viewports.at(0).id, set.sheets[0].viewports.at(0).id);
    EXPECT_EQ(document.history().undoCount(), steps + 1);
    EXPECT_EQ(document.history().undoName(), "APPEND_SHEETS");
    EXPECT_TRUE(shown.said(QStringLiteral("appended 2 sheets from ")));
    // The first appended sheet is shown.
    EXPECT_EQ(shown.editor.currentSheet(), 1u);
}

TEST(SheetSetMenu, CopySheetSetAsJsonPutsTheSetsJsonOnTheClipboard)
{
    Document document;
    type(document, {"SHEET NEW PLAN", "VIEW ADD 1 notes"});
    Shown shown(document);
    QGuiApplication::clipboard()->clear();
    const std::size_t steps = document.history().undoCount();
    shown.trigger("sheetCopySetJson");
    EXPECT_EQ(QGuiApplication::clipboard()->text().toStdString(),
              plotting::sheetSetToJson(document.sheetSet()).value());
    EXPECT_EQ(document.history().undoCount(), steps);
    EXPECT_TRUE(shown.said(QStringLiteral("> SHEETS JSON")));
}

TEST(SheetSetMenu, PageSetupIsSavedWithoutAPlot)
{
    Document document;
    type(document, {"SHEET NEW PLAN", "SHEETS PAGESETUP style=mono dpi=200"});
    Shown shown(document);
    shown.trigger("sheetPageSetup");
    auto* dialog = pageSetupDialogOf(shown.editor);
    ASSERT_NE(dialog, nullptr);
    // The page setup's controls, starting from the set's; none of a plot's.
    auto* colour = dialog->findChild<QComboBox*>(QStringLiteral("plotColourMode"));
    auto* weights = dialog->findChild<QDoubleSpinBox*>(QStringLiteral("plotLineWeightScale"));
    auto* dpi = dialog->findChild<QSpinBox*>(QStringLiteral("plotDpi"));
    auto* pattern = dialog->findChild<QLineEdit*>(QStringLiteral("plotPattern"));
    auto* perSheet = dialog->findChild<QCheckBox*>(QStringLiteral("pageSetupFilePerSheet"));
    auto* save = dialog->findChild<QPushButton*>(QStringLiteral("pageSetupSave"));
    for (QWidget* shownControl : std::initializer_list<QWidget*>{colour, weights, dpi, pattern, perSheet, save}) {
        ASSERT_NE(shownControl, nullptr);
        EXPECT_TRUE(shownControl->isVisibleTo(dialog)) << shownControl->objectName().toStdString();
    }
    for (const char* hidden : {"plotSheetsAll", "plotFormat", "plotFile", "plotFolder", "plotPrinter",
                               "plotOpenAfter", "plotKeepSetup"}) {
        auto* widget = dialog->findChild<QWidget*>(QString::fromLatin1(hidden));
        EXPECT_TRUE(widget == nullptr || !widget->isVisibleTo(dialog)) << hidden;
    }
    EXPECT_EQ(dialog->findChild<QPushButton*>(QStringLiteral("plotRun")), nullptr);
    EXPECT_EQ(colour->currentData().toString(), QStringLiteral("monochrome"));
    EXPECT_EQ(dpi->value(), 200);

    colour->setCurrentIndex(colour->findData(QStringLiteral("greyscale")));
    weights->setValue(0.7);
    dpi->setValue(150);
    pattern->setText(QStringLiteral("{n:02} {name}"));
    perSheet->setChecked(true);
    EXPECT_EQ(dialog->pageSetupLine(),
              QStringLiteral("SHEETS PAGESETUP style=greyscale lineweight=0.7 dpi=150 "
                             "pattern=\"{n:02} {name}\" filepersheet=on"));
    const std::size_t steps = document.history().undoCount();
    save->click();
    katana::qt::test::processEvents();
    const plotting::PageSetup& setup = document.sheetSet().pageSetup;
    EXPECT_EQ(setup.colourMode, katana::cad::PlotColourMode::Greyscale);
    EXPECT_DOUBLE_EQ(setup.lineWeightScale, 0.7);
    EXPECT_EQ(setup.dpi, 150.0);
    EXPECT_EQ(setup.fileNamePattern, "{n:02} {name}");
    EXPECT_TRUE(setup.filePerSheet);
    EXPECT_EQ(document.history().undoCount(), steps + 1);
    EXPECT_EQ(document.history().undoName(), "PAGE_SETUP");
    // The line and its reply, and no plot: nothing else was said.
    ASSERT_EQ(shown.messages.size(), 2u);
    EXPECT_EQ(shown.messages[0], QStringLiteral("> ") + QStringLiteral(
                                     "SHEETS PAGESETUP style=greyscale lineweight=0.7 dpi=150 "
                                     "pattern=\"{n:02} {name}\" filepersheet=on"));
    EXPECT_TRUE(shown.messages[1].startsWith(QStringLiteral("pagesetup style=greyscale")));
}

TEST(SheetSetMenu, APageSetupThatCannotBeTypedCannotBeSaved)
{
    Document document;
    type(document, {"SHEET NEW PLAN"});
    Shown shown(document);
    shown.trigger("sheetPageSetup");
    auto* dialog = pageSetupDialogOf(shown.editor);
    ASSERT_NE(dialog, nullptr);
    auto* pattern = dialog->findChild<QLineEdit*>(QStringLiteral("plotPattern"));
    auto* save = dialog->findChild<QPushButton*>(QStringLiteral("pageSetupSave"));
    auto* problem = dialog->findChild<QLabel*>(QStringLiteral("plotProblem"));
    ASSERT_NE(pattern, nullptr);
    EXPECT_TRUE(save->isEnabled());
    pattern->setText(QStringLiteral("{nope}"));
    EXPECT_FALSE(save->isEnabled());
    EXPECT_FALSE(problem->text().isEmpty());
    // The command line has no way to type a double quote in a value.
    pattern->setText(QStringLiteral("say \"{n}\""));
    EXPECT_FALSE(save->isEnabled());
    EXPECT_TRUE(problem->text().contains(QStringLiteral("double quote")));
    // A backslash is written so the verb reads it back as itself.
    pattern->setText(QStringLiteral("plots\\{n}"));
    EXPECT_TRUE(dialog->pageSetupLine().contains(QStringLiteral("pattern=\"plots\\\\{n}\"")));
    dialog->reject();
}

TEST(SheetSetMenu, AHeadlessSessionNamesTheVerbsInsteadOfOpeningAFileDialog)
{
    Document document;
    type(document, {"SHEET NEW PLAN"});
    Shown shown(document);
    shown.editor.setHeadless(true);
    Answers answers({});
    for (const auto& [item, verb] : {std::pair{"sheetSaveSet", "SHEETS SAVE"},
                                     std::pair{"sheetLoadSet", "SHEETS LOAD"},
                                     std::pair{"sheetAppendSet", "SHEETS APPEND"}}) {
        const int errors = shown.errors;
        shown.trigger(item);
        EXPECT_EQ(shown.errors, errors + 1) << item;
        EXPECT_TRUE(shown.messages.back().contains(QString::fromLatin1(verb))) << item;
    }
    EXPECT_EQ(answers.filesSeen, 0);
    EXPECT_EQ(answers.questionsSeen, 0u);
}
