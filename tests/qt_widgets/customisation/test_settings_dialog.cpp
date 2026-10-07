// SettingsDialog: the session's customisation shown as the Document has it, and
// every button a CUSTOMISE line handed to the runner it was given. Driven by
// object name, as a person - or the headless driver - drives it.
//
// The dialog does none of the work, so most tests give it a RECORDING runner:
// it notes each line and, unless a test says what a line does, changes
// nothing and answers that it was carried out. Where what follows a line
// depends on what the verb did to the Document - a kept session that an edit
// leaves not kept - the test's runner does that one thing to the Document, by
// the Document's own API, as the verb is documented to (docs/customisation.md,
// "The verbs"). The last seven give it the verb itself, the interpreter over
// the same Document: that is where the lines built here and the family that
// reads them meet below the window, and where what a press does to the kept
// file is seen on the disk.
//
// Every expected line and text is written out by hand from the dialog's
// header and the task it was built to. The counts are of customisations
// written out here or counted in fixture_customisation.hpp:
//
//   kSite          1 linestyle (SITE Fence) and 2 symbols (SITE Peg, SITE
//                  Post): 3 definitions; 3 rules over 2 codes (FN* feature,
//                  PG* feature, PG* symbol); a notice of two lines
//   kOneCode       no definition; 2 rules over 1 code (PG* feature, PG*
//                  symbol)
//   kMine          1 linestyle (MINE Hedge) and no symbol: 1 definition; 1
//                  rule over 1 code (HG* feature); its automation said - survey
//                  codes NOT applied, coded points joined
//   the fixtures   test_linestyles 3 linestyles, test_symbols 4 symbols,
//                  test_survey 11 rules over 8 codes
//   the noticed    four customisations, each with a notice of its own
//   session        (installNoticedSession)
//
// What the kept file is after RESET, KEEP and REVERT - when it is written,
// when it is set aside as .bak, that ONE such file is kept - is taken from
// docs/customisation.md, "The host: RESET, KEEP and REVERT", and worked out
// by hand for each press in the two tests that follow it through.

#include <gtest/gtest.h>

#include <array>
#include <filesystem>
#include <functional>
#include <memory>
#include <string>
#include <utility>
#include <vector>

#include <QAbstractButton>
#include <QApplication>
#include <QCheckBox>
#include <QDir>
#include <QFile>
#include <QFileDialog>
#include <QFileInfo>
#include <QFormLayout>
#include <QGroupBox>
#include <QKeyEvent>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QPlainTextEdit>
#include <QPointer>
#include <QPushButton>
#include <QStackedWidget>
#include <QTemporaryDir>
#include <QTimer>

#include "command_runner.hpp"
#include "customisation/code_manager.hpp"
#include "customisation/customisation_context.hpp"
#include "customisation/fixture_customisation.hpp"
#include "customisation/linework_labels.hpp"
#include "customisation/settings_dialog.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/customisation_host.hpp"
#include "katana/cad/customisation_state.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/path_text.hpp"
#include "katana/entity/customisation.hpp"
#include "katana/entity/linework_codes.hpp"
#include "theme.hpp"
#include "widget_harness.hpp"

using katana::cad::CustomisationOrigin;
using katana::cad::Document;
using katana::entity::CustomisationAutomation;
using katana::qt::SettingsContext;
using katana::qt::SettingsDialog;
using katana::qt::VerbOutcome;
using katana::qt::test::customisationFromText;
using katana::qt::test::processEvents;

namespace {

constexpr const char* kSite = R"({
  "format": "katana-customisation",
  "version": 1,
  "name": "Site",
  "notice": [
    "Site set: for this job only.",
    "Ask before passing it on."
  ],
  "linestyles": [
    {"name": "SITE Fence", "strokes": [
      ["move", 0, 0],
      ["draw", 4, 0]
    ]}
  ],
  "symbols": [
    {"name": "SITE Peg", "atVertices": true, "strokes": [
      ["circle", 0.5]
    ]},
    {"name": "SITE Post", "atVertices": true, "strokes": [
      ["move", -0.25, 0],
      ["draw", 0.25, 0]
    ]}
  ],
  "codes": [
    {"key": "FN*", "sets": "feature", "layer": "SITE FENCES", "linestyle": "SITE Fence"},
    {"key": "PG*", "sets": "feature", "layer": "SITE PEGS", "draw": "point"},
    {"key": "PG*", "sets": "symbol", "symbol": {"name": "SITE Peg"}}
  ]
}
)";

// Rules alone, and all of ONE code: kSite's two for PG*.
constexpr const char* kOneCode = R"({
  "format": "katana-customisation",
  "version": 1,
  "name": "Pegs",
  "codes": [
    {"key": "PG*", "sets": "feature", "layer": "SITE PEGS", "draw": "point"},
    {"key": "PG*", "sets": "symbol", "symbol": {"name": "SITE Peg"}}
  ]
}
)";

// A person's own customisation, as a kept file holds one: another name than
// kSite's, one linestyle and no symbol, one rule over one code, and its
// automation said - survey codes not applied, coded points joined.
constexpr const char* kMine = R"({
  "format": "katana-customisation",
  "version": 1,
  "name": "Mine",
  "automation": {"codesOnSurveyImport": false, "lineworkOnSurveyImport": true},
  "linestyles": [
    {"name": "MINE Hedge", "strokes": [
      ["move", 0, 0],
      ["draw", 2, 0]
    ]}
  ],
  "codes": [
    {"key": "HG*", "sets": "feature", "layer": "MINE HEDGES", "linestyle": "MINE Hedge"}
  ]
}
)";

// The kept file every fixture's host has: a folder with a blank in its name.
const QString kKeptFile = QStringLiteral("C:/kept files/customisation.json");

const QString kKeep = QStringLiteral("CUSTOMISE KEEP");

VerbOutcome carriedOut() { return VerbOutcome{true, {}, {}}; }

struct SettingsFixture {
    Document document;
    SettingsContext context;
    // Every line handed to the runner, in order.
    QStringList ran;
    std::vector<std::pair<QString, bool>> logged;
    // What a line does and answers. Unset: nothing, and that it was carried
    // out - a runner that only records.
    std::function<VerbOutcome(const QString& line)> answer;
    bool dirty = false;
    bool headless = false;
    int openedCodes = 0;
    int openedLinework = 0;
    int openedSymbols = 0;

    SettingsFixture()
    {
        context.document = &document;
        context.run = [this](const QString& line) {
            ran << line;
            return answer ? answer(line) : carriedOut();
        };
        context.log = [this](const QString& message, bool isError) {
            logged.emplace_back(message, isError);
        };
        context.headless = [this] { return headless; };
        context.codeManagerDirty = [this] { return dirty; };
        context.openCodeManager = [this] { ++openedCodes; };
        context.openLinework = [this] { ++openedLinework; };
        context.openSymbolLibrary = [this] { ++openedSymbols; };
        context.hasBuiltIn = true;
        context.keptFile = kKeptFile;
    }

    // kSite installed whole, in the place of whatever the session held.
    void install(CustomisationOrigin origin, bool kept)
    {
        const auto installed =
            document.installCustomisation(customisationFromText(kSite), origin, kept);
        ASSERT_TRUE(installed.ok()) << (installed.ok() ? "" : installed.error().describe());
    }
};

template <typename T> T* child(const QWidget& parent, const char* name)
{
    T* found = parent.findChild<T*>(QString::fromLatin1(name));
    EXPECT_NE(found, nullptr) << name;
    return found;
}

void fill(const QWidget& dialog, const char* field, const QString& text)
{
    if (QLineEdit* line = child<QLineEdit>(dialog, field)) {
        line->setText(text);
    }
}

// A click, as a person's or the headless driver's --press: a button or a box.
void press(const QWidget& dialog, const char* button)
{
    if (QAbstractButton* found = child<QAbstractButton>(dialog, button)) {
        found->click();
    }
}

QString shown(const QWidget& dialog, const char* label)
{
    const QLabel* found = child<QLabel>(dialog, label);
    return found != nullptr ? found->text() : QString();
}

QString status(const QWidget& dialog)
{
    return child<QPlainTextEdit>(dialog, "settingsStatus")->toPlainText();
}

// Whether the status is in the colour of a refusal.
bool saysARefusal(const QWidget& dialog)
{
    return child<QPlainTextEdit>(dialog, "settingsStatus")
        ->styleSheet()
        .contains(katana::qt::theme::error().name());
}

bool enabled(const QWidget& dialog, const char* name)
{
    return child<QWidget>(dialog, name)->isEnabled();
}

QString tip(const QWidget& dialog, const char* name)
{
    return child<QWidget>(dialog, name)->toolTip();
}

bool checked(const QWidget& dialog, const char* box)
{
    return child<QCheckBox>(dialog, box)->isChecked();
}

// Every file dialog a click opens while this lives, caught as it opens: how
// many there were, the last one's title, filters and the folder it opened in,
// and `file` chosen in it - or, with no file, cancelled. A dialog still up at
// the next poll, its choice not taken, is cancelled too: a press that should
// have opened none, and a choice that failed, are then a failure of the test
// and never a wait for a box nobody closes.
struct FileDialogAnswer {
    QTimer timer;
    int seen = 0;
    QString title;
    QStringList filters;
    QString directory;
    QPointer<QFileDialog> last;

    explicit FileDialogAnswer(QString file = {})
    {
        timer.setInterval(10);
        QObject::connect(&timer, &QTimer::timeout, [this, chosen = std::move(file)] {
            auto* dialog = qobject_cast<QFileDialog*>(QApplication::activeModalWidget());
            if (dialog == nullptr) {
                return;
            }
            const bool again = dialog == last;
            last = dialog;
            if (!again) {
                ++seen;
                title = dialog->windowTitle();
                filters = dialog->nameFilters();
                directory = dialog->directory().absolutePath();
            }
            if (again || chosen.isEmpty()) {
                dialog->reject();
            } else {
                katana::qt::test::chooseFile(*dialog, chosen);
            }
        });
        timer.start();
    }
};

// Every text `root` and the widgets under it can show a person.
QStringList textsIn(QWidget& root)
{
    QStringList seen;
    QList<QWidget*> widgets = root.findChildren<QWidget*>();
    widgets.prepend(&root);
    for (const QWidget* widget : widgets) {
        seen << widget->windowTitle() << widget->toolTip() << widget->statusTip()
             << widget->whatsThis() << widget->accessibleName()
             << widget->accessibleDescription();
        if (const auto* button = qobject_cast<const QAbstractButton*>(widget)) {
            seen << button->text();
        }
        if (const auto* label = qobject_cast<const QLabel*>(widget)) {
            seen << label->text();
        }
        if (const auto* line = qobject_cast<const QLineEdit*>(widget)) {
            seen << line->text() << line->placeholderText();
        }
        if (const auto* box = qobject_cast<const QPlainTextEdit*>(widget)) {
            seen << box->toPlainText() << box->placeholderText();
        }
        if (const auto* group = qobject_cast<const QGroupBox*>(widget)) {
            seen << group->title();
        }
        if (const auto* list = qobject_cast<const QListWidget*>(widget)) {
            for (int row = 0; row < list->count(); ++row) {
                seen << list->item(row)->text() << list->item(row)->toolTip()
                     << list->item(row)->statusTip();
            }
        }
    }
    seen.removeAll(QString());
    return seen;
}

// The window's executor as far as a dialog can tell (MainWindow::runVerbLine):
// the line run by the interpreter, a reply as it is and a refusal as the error
// with its code (core::Error::describe).
VerbOutcome runAsTheWindowDoes(katana::cad::CommandInterpreter& interpreter, const QString& line)
{
    VerbOutcome outcome;
    const auto reply = interpreter.run(line.toStdString());
    if (reply) {
        outcome.ok = true;
        outcome.reply = QString::fromStdString(*reply);
    } else {
        outcome.error = QString::fromStdString(reply.error().describe());
    }
    return outcome;
}

QString pathText(const std::filesystem::path& path)
{
    return QString::fromStdString(katana::core::pathToUtf8(path)).replace('\\', '/');
}

// The customisation the file at `path` holds, read by the format's own reader;
// an empty one, and a failure of the test, when it does not read.
katana::entity::Customisation customisationInFile(const QString& path)
{
    const auto read =
        katana::cad::readCustomisationFile(katana::core::pathFromUtf8(path.toStdString()));
    EXPECT_TRUE(read.ok()) << path.toStdString() << ": "
                           << (read.ok() ? std::string() : read.error().describe());
    return read.ok() ? read->customisation : katana::entity::Customisation{};
}

// A session as a front end starts one whose user keeps a customisation of
// their own: kSite as the program's built-in, and kMine in the kept file - on
// disk BEFORE the host is handed over, which is when the family notes the file
// it will not write over unseen (cad::customisationVerbContext). The runner is
// the verb itself, the interpreter over the same Document.
struct KeptSession {
    QTemporaryDir scratch;
    QString keptFile;
    // Where CUSTOMISE KEEP leaves the kept file that was there.
    QString setAside;
    katana::cad::CustomisationHost host;
    SettingsFixture f;
    katana::cad::CommandInterpreter interpreter{f.document};

    // Apart from the constructor, which cannot fail a test and stop.
    void start()
    {
        ASSERT_TRUE(scratch.isValid());
        keptFile = scratch.filePath(QStringLiteral("kept here/customisation.json"));
        setAside = keptFile + QStringLiteral(".bak");
        ASSERT_TRUE(QDir().mkpath(QFileInfo(keptFile).absolutePath()));
        QFile file(keptFile);
        ASSERT_TRUE(file.open(QIODevice::WriteOnly));
        file.write(kMine);
        file.close();

        host.builtIn.customisation =
            std::make_shared<const katana::entity::Customisation>(customisationFromText(kSite));
        host.builtIn.digest = katana::entity::customisationDigest(kSite);
        host.keptFile = katana::core::pathFromUtf8(keptFile.toStdString());
        f.context.keptFile = keptFile;
        interpreter.setCustomisationHost(host);
        const katana::cad::CustomisationStart started =
            katana::cad::startCustomisation(f.document, host);
        ASSERT_EQ(started.installed, CustomisationOrigin::Kept);
        ASSERT_TRUE(started.problems.empty()) << started.problems.front();
        ASSERT_TRUE(f.document.customisationState().kept);
        f.answer = [this](const QString& line) { return runAsTheWindowDoes(interpreter, line); };
    }
};

} // namespace

// ---- what it is ---------------------------------------------------------------------------------

TEST(SettingsDialog, ItIsANonModalDialogWithOnePageAndANameForEveryControl)
{
    SettingsFixture f;
    SettingsDialog dialog(f.context);
    EXPECT_EQ(dialog.objectName(), QStringLiteral("settingsDialog"));
    EXPECT_EQ(dialog.windowTitle(), QStringLiteral("Settings"));
    EXPECT_FALSE(dialog.isModal());

    const auto* pages = child<QListWidget>(dialog, "settingsPages");
    ASSERT_EQ(pages->count(), 1);
    EXPECT_EQ(pages->item(0)->text(), QStringLiteral("Customisation"));
    EXPECT_EQ(pages->currentRow(), 0);
    const auto* stack = child<QStackedWidget>(dialog, "settingsStack");
    ASSERT_EQ(stack->count(), 1);
    ASSERT_NE(stack->currentWidget(), nullptr);
    EXPECT_EQ(stack->currentWidget()->objectName(), QStringLiteral("settingsCustomisation"));

    const QWidget* page = stack->currentWidget();
    for (const char* name :
         {"settingsActiveName",   "settingsActiveOrigin",  "settingsActiveDefinitions",
          "settingsActiveSymbols", "settingsActiveRules",   "settingsActiveKept",
          "settingsActiveProblems", "settingsShowNotice",   "settingsNotice",
          "settingsImportPath",   "settingsImportBrowse",  "settingsImportReplace",
          "settingsImport",       "settingsExportPath",    "settingsExportBrowse",
          "settingsExport",       "settingsReset",         "settingsKeep",
          "settingsRevert",       "settingsAutoCodes",     "settingsAutoLinework",
          "settingsLinework",     "settingsEditLinework",  "settingsOpenCodes",
          "settingsOpenSymbols"}) {
        EXPECT_NE(page->findChild<QWidget*>(QString::fromLatin1(name)), nullptr) << name;
    }
    EXPECT_NE(dialog.findChild<QPlainTextEdit*>(QStringLiteral("settingsStatus")), nullptr);
    EXPECT_NE(dialog.findChild<QPushButton*>(QStringLiteral("settingsClose")), nullptr);
    // The words the task gives the controls.
    EXPECT_EQ(child<QCheckBox>(dialog, "settingsImportReplace")->text(),
              QStringLiteral("Replace instead of merging"));
    EXPECT_EQ(child<QCheckBox>(dialog, "settingsAutoCodes")->text(),
              QStringLiteral("Apply survey codes"));
    EXPECT_EQ(child<QCheckBox>(dialog, "settingsAutoLinework")->text(),
              QStringLiteral("Join coded points into lines"));
    EXPECT_EQ(child<QPushButton>(dialog, "settingsOpenCodes")->text(),
              QStringLiteral("Survey Codes..."));
    EXPECT_EQ(child<QPushButton>(dialog, "settingsOpenSymbols")->text(),
              QStringLiteral("Symbol Library..."));
    // There is no Apply button: each control runs its own line.
    for (const QPushButton* button : dialog.findChildren<QPushButton*>()) {
        EXPECT_FALSE(button->text().contains(QStringLiteral("Apply"), Qt::CaseInsensitive))
            << button->objectName().toStdString();
    }
    katana::qt::test::paint(dialog);
}

// ---- the lines ----------------------------------------------------------------------------------

TEST(SettingsDialog, ImportBuildsTheMergeLineAndWithReplaceTickedTheReplaceLine)
{
    SettingsFixture f;
    SettingsDialog dialog(f.context);
    // A folder and a file with a blank in each: one word of the line.
    fill(dialog, "settingsImportPath",
         QStringLiteral("C:/survey data/site codes.customisation.json"));
    press(dialog, "settingsImport");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral(
                         "CUSTOMISE \"C:/survey data/site codes.customisation.json\"")}));

    f.ran.clear();
    child<QCheckBox>(dialog, "settingsImportReplace")->setChecked(true);
    EXPECT_TRUE(f.ran.isEmpty()) << "ticking Replace runs nothing";
    press(dialog, "settingsImport");
    EXPECT_EQ(f.ran,
              (QStringList{QStringLiteral(
                  "CUSTOMISE REPLACE \"C:/survey data/site codes.customisation.json\"")}));
}

TEST(SettingsDialog, ExportBuildsTheExportLine)
{
    SettingsFixture f;
    SettingsDialog dialog(f.context);
    fill(dialog, "settingsExportPath", QStringLiteral("D:/out put/site set.customisation.json"));
    press(dialog, "settingsExport");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral(
                         "CUSTOMISE EXPORT \"D:/out put/site set.customisation.json\"")}));
}

TEST(SettingsDialog, ResetKeepAndRevertEachBuildTheirLine)
{
    SettingsFixture f;
    SettingsDialog dialog(f.context);
    press(dialog, "settingsReset");
    press(dialog, "settingsKeep");
    press(dialog, "settingsRevert");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE RESET"), kKeep,
                                  QStringLiteral("CUSTOMISE REVERT")}));
}

TEST(SettingsDialog, APathIsTrimmedAndABareNameIsGivenItsDirectorySoItIsNeverReadAsAKeyword)
{
    SettingsFixture f;
    SettingsDialog dialog(f.context);
    fill(dialog, "settingsImportPath", QStringLiteral("  C:/data/site.json  "));
    press(dialog, "settingsImport");
    // `CUSTOMISE "keep"` is CUSTOMISE KEEP to the verb, which reads a whole
    // first word as a keyword once the tokenizer has taken the quotes off.
    fill(dialog, "settingsImportPath", QStringLiteral("keep"));
    press(dialog, "settingsImport");
    // EXPORT refuses a file called as one of its own words.
    fill(dialog, "settingsExportPath", QStringLiteral("codes"));
    press(dialog, "settingsExport");
    // A path that names a folder is the person's own, as it is.
    fill(dialog, "settingsExportPath", QStringLiteral("out/codes"));
    press(dialog, "settingsExport");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE \"C:/data/site.json\""),
                                  QStringLiteral("CUSTOMISE \"./keep\""),
                                  QStringLiteral("CUSTOMISE EXPORT \"./codes\""),
                                  QStringLiteral("CUSTOMISE EXPORT \"out/codes\"")}));
}

TEST(SettingsDialog, APathPastedInItsQuotesIsReadAsThePathItIs)
{
    SettingsFixture f;
    SettingsDialog dialog(f.context);
    // As a file manager copies a path: the whole of it in double quotes. They
    // are no part of the file's name, and the line is the one the path alone
    // builds.
    fill(dialog, "settingsImportPath", QStringLiteral("\"C:/survey data/site codes.json\""));
    press(dialog, "settingsImport");
    EXPECT_FALSE(saysARefusal(dialog)) << status(dialog).toStdString();
    // Blanks round the quotes, and inside them, are trimmed as round a path
    // with none.
    fill(dialog, "settingsExportPath", QStringLiteral("  \" D:/out put/site set.json \"  "));
    press(dialog, "settingsExport");
    EXPECT_FALSE(saysARefusal(dialog)) << status(dialog).toStdString();
    // A bare name is given its directory in quotes or out of them.
    fill(dialog, "settingsImportPath", QStringLiteral("\"keep\""));
    press(dialog, "settingsImport");
    EXPECT_EQ(f.ran,
              (QStringList{QStringLiteral("CUSTOMISE \"C:/survey data/site codes.json\""),
                           QStringLiteral("CUSTOMISE EXPORT \"D:/out put/site set.json\""),
                           QStringLiteral("CUSTOMISE \"./keep\"")}));
    // The field keeps what was put in it.
    EXPECT_EQ(child<QLineEdit>(dialog, "settingsImportPath")->text(), QStringLiteral("\"keep\""));

    // ONE pair, round the WHOLE path. Quotes round nothing name no file; a
    // second pair, two paths side by side and a quote alone each still hold a
    // quote no line can carry, and are refused for it as before.
    f.ran.clear();
    const QString noFile =
        QStringLiteral("Import: no file is named: type its path, or choose it with Browse");
    const QString aQuote = QStringLiteral("Import: the file's path: a double quote cannot be "
                                          "written on a command line");
    const std::array<std::pair<QString, QString>, 5> refused{{
        {QStringLiteral("\"\""), noFile},
        {QStringLiteral("\"   \""), noFile},
        {QStringLiteral("\"\"C:/data/site.json\"\""), aQuote},
        {QStringLiteral("\"C:/data/a.json\" \"C:/data/b.json\""), aQuote},
        {QStringLiteral("\""), aQuote},
    }};
    for (const auto& [typed, why] : refused) {
        fill(dialog, "settingsImportPath", typed);
        press(dialog, "settingsImport");
        EXPECT_EQ(status(dialog), why) << typed.toStdString();
        EXPECT_TRUE(saysARefusal(dialog)) << typed.toStdString();
    }
    EXPECT_TRUE(f.ran.isEmpty()) << f.ran.join(" | ").toStdString();
}

#ifdef Q_OS_WIN
TEST(SettingsDialog, AWindowsPathIsWrittenWithForwardSlashes)
{
    SettingsFixture f;
    SettingsDialog dialog(f.context);
    fill(dialog, "settingsImportPath", QStringLiteral("C:\\survey data\\site.json"));
    press(dialog, "settingsImport");
    // And as Explorer's "Copy as path" gives one: in quotes, with its own
    // separators.
    fill(dialog, "settingsImportPath", QStringLiteral("\"C:\\data\\site codes.json\""));
    press(dialog, "settingsImport");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE \"C:/survey data/site.json\""),
                                  QStringLiteral("CUSTOMISE \"C:/data/site codes.json\"")}));
}
#endif

TEST(SettingsDialog, AFileNoLineCanCarryAndNoFileAtAllRunNothingAndSayWhy)
{
    SettingsFixture f;
    SettingsDialog dialog(f.context);
    press(dialog, "settingsImport");
    EXPECT_EQ(status(dialog),
              QStringLiteral("Import: no file is named: type its path, or choose it with Browse"));
    EXPECT_TRUE(saysARefusal(dialog));
    fill(dialog, "settingsExportPath", QStringLiteral("   "));
    press(dialog, "settingsExport");
    EXPECT_EQ(status(dialog),
              QStringLiteral("Export: no file is named: type its path, or choose it with Browse"));

    // The tokenizer's quoted words have no escape: a quote would end the word.
    fill(dialog, "settingsImportPath", QStringLiteral("C:/data/the \"new\" set.json"));
    press(dialog, "settingsImport");
    EXPECT_EQ(status(dialog),
              QStringLiteral("Import: the file's path: a double quote cannot be written on a "
                             "command line"));
    EXPECT_TRUE(saysARefusal(dialog));
    fill(dialog, "settingsExportPath", QStringLiteral("C:/data/the \"new\" set.json"));
    press(dialog, "settingsExport");
    EXPECT_EQ(status(dialog),
              QStringLiteral("Export: the file's path: a double quote cannot be written on a "
                             "command line"));

    EXPECT_TRUE(f.ran.isEmpty()) << f.ran.join(" | ").toStdString();
    // Said in the log too, as refusals: a headless run reads them there.
    ASSERT_EQ(f.logged.size(), 4u);
    EXPECT_EQ(f.logged.back().first, status(dialog));
    EXPECT_TRUE(f.logged.back().second);
}

// ---- the automation boxes -----------------------------------------------------------------------

TEST(SettingsDialog, TogglingABoxRunsItsLineOnceAndTheBoxShowsWhatTheDocumentHas)
{
    SettingsFixture f;
    SettingsDialog dialog(f.context);
    // Both switches are on in a Document nothing has set them in.
    ASSERT_TRUE(checked(dialog, "settingsAutoCodes"));
    ASSERT_TRUE(checked(dialog, "settingsAutoLinework"));

    press(dialog, "settingsAutoCodes");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE SET auto.codes=off")}));
    // The recording runner changed nothing, so the Document still has it on,
    // and putting the box back ran no second line.
    EXPECT_TRUE(checked(dialog, "settingsAutoCodes"));
    processEvents();
    EXPECT_EQ(f.ran.size(), 1);

    f.ran.clear();
    press(dialog, "settingsAutoLinework");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE SET auto.linework=off")}));
    EXPECT_TRUE(checked(dialog, "settingsAutoLinework"));

    // With a runner that does what the line says, the box stays where it was
    // put, and the next toggle says the other word.
    f.ran.clear();
    f.answer = [&f](const QString& line) {
        CustomisationAutomation automation = f.document.customisationState().automation;
        if (line == QStringLiteral("CUSTOMISE SET auto.codes=off")) {
            automation.codesOnSurveyImport = false;
        } else if (line == QStringLiteral("CUSTOMISE SET auto.codes=on")) {
            automation.codesOnSurveyImport = true;
        }
        f.document.setAutomation(automation);
        return carriedOut();
    };
    press(dialog, "settingsAutoCodes");
    processEvents();
    EXPECT_FALSE(checked(dialog, "settingsAutoCodes"));
    EXPECT_FALSE(f.document.customisationState().automation.codesOnSurveyImport);
    press(dialog, "settingsAutoCodes");
    processEvents();
    EXPECT_TRUE(checked(dialog, "settingsAutoCodes"));
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE SET auto.codes=off"),
                                  QStringLiteral("CUSTOMISE SET auto.codes=on")}));
}

TEST(SettingsDialog, ABoxSetAsTheHeadlessDriverFillsOneRunsItsLineToo)
{
    // --fill settingsAutoLinework=off is QCheckBox::setChecked: no click.
    SettingsFixture f;
    SettingsDialog dialog(f.context);
    child<QCheckBox>(dialog, "settingsAutoLinework")->setChecked(false);
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE SET auto.linework=off")}));
}

TEST(SettingsDialog, ABoxTheDocumentHasOffRunsItsLineWithOnWhenItIsTicked)
{
    // The fourth line of the two boxes: each box off in the Document, so
    // that a tick is the word "on".
    SettingsFixture f;
    f.document.setAutomation({.codesOnSurveyImport = false, .lineworkOnSurveyImport = false});
    SettingsDialog dialog(f.context);
    ASSERT_FALSE(checked(dialog, "settingsAutoCodes"));
    ASSERT_FALSE(checked(dialog, "settingsAutoLinework"));
    EXPECT_TRUE(f.ran.isEmpty()) << "showing the boxes as the Document has them runs nothing";

    press(dialog, "settingsAutoLinework");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE SET auto.linework=on")}));
    // The recording runner set nothing, so the box is off again, as the
    // Document is.
    EXPECT_FALSE(checked(dialog, "settingsAutoLinework"));

    f.ran.clear();
    press(dialog, "settingsAutoCodes");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE SET auto.codes=on")}));
    EXPECT_FALSE(checked(dialog, "settingsAutoCodes"));
}

TEST(SettingsDialog, RefreshingFromADocumentChangeRunsNothing)
{
    SettingsFixture f;
    SettingsDialog dialog(f.context);
    f.document.setAutomation({.codesOnSurveyImport = false, .lineworkOnSurveyImport = false});
    processEvents();
    EXPECT_FALSE(checked(dialog, "settingsAutoCodes"));
    EXPECT_FALSE(checked(dialog, "settingsAutoLinework"));
    f.document.setAutomation({.codesOnSurveyImport = true, .lineworkOnSurveyImport = false});
    processEvents();
    EXPECT_TRUE(checked(dialog, "settingsAutoCodes"));
    EXPECT_FALSE(checked(dialog, "settingsAutoLinework"));
    // A whole customisation installed, and the page read again by hand.
    f.install(CustomisationOrigin::Loaded, false);
    processEvents();
    dialog.refresh();
    EXPECT_TRUE(checked(dialog, "settingsAutoLinework"));
    EXPECT_TRUE(f.ran.isEmpty()) << f.ran.join(" | ").toStdString();
    EXPECT_TRUE(status(dialog).isEmpty()) << status(dialog).toStdString();
}

// ---- refused while the code manager holds edits -------------------------------------------------

TEST(SettingsDialog, ImportReplaceResetAndRevertAreRefusedWhileTheCodeManagerHoldsUnappliedEdits)
{
    SettingsFixture f;
    f.dirty = true;
    SettingsDialog dialog(f.context);
    fill(dialog, "settingsImportPath", QStringLiteral("C:/data/site.json"));

    press(dialog, "settingsImport");
    EXPECT_EQ(status(dialog),
              QStringLiteral("Import was not run: the Survey Code Manager has rule edits that "
                             "are not on the drawing, and its Apply would then put the rules it "
                             "holds back over what Import brought. Apply or Revert them first."));
    EXPECT_TRUE(saysARefusal(dialog));
    child<QCheckBox>(dialog, "settingsImportReplace")->setChecked(true);
    press(dialog, "settingsImport");
    EXPECT_TRUE(status(dialog).startsWith(QStringLiteral("Import was not run: the Survey Code "
                                                         "Manager has rule edits")));

    press(dialog, "settingsReset");
    EXPECT_EQ(status(dialog),
              QStringLiteral("Reset to Built-in was not run: the Survey Code Manager has rule "
                             "edits that are not on the drawing, and its Apply would then put "
                             "the rules it holds back over what Reset to Built-in brought. Apply "
                             "or Revert them first."));
    // Revert to Kept loads the kept file's rules: the same loss by the same
    // Apply, so it is held to the same.
    press(dialog, "settingsRevert");
    EXPECT_TRUE(status(dialog).startsWith(QStringLiteral("Revert to Kept was not run: the "
                                                         "Survey Code Manager has rule edits")));

    EXPECT_TRUE(f.ran.isEmpty()) << f.ran.join(" | ").toStdString();
    ASSERT_EQ(f.logged.size(), 4u);
    EXPECT_TRUE(f.logged.front().second);
    EXPECT_TRUE(f.logged.front().first.startsWith(QStringLiteral("Import was not run")));

    // What loads no rules is not held up: an export, a keep, a switch.
    fill(dialog, "settingsExportPath", QStringLiteral("C:/data/out.json"));
    press(dialog, "settingsExport");
    press(dialog, "settingsKeep");
    press(dialog, "settingsAutoCodes");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE EXPORT \"C:/data/out.json\""), kKeep,
                                  QStringLiteral("CUSTOMISE SET auto.codes=off")}));

    // Once the edits are applied or reverted, the same press runs.
    f.ran.clear();
    f.dirty = false;
    press(dialog, "settingsReset");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE RESET")}));
}

// ---- what the host does not have ----------------------------------------------------------------

TEST(SettingsDialog, ResetIsDisabledAndSaysWhyWhenTheHostHasNoBuiltIn)
{
    SettingsFixture f;
    f.context.hasBuiltIn = false;
    SettingsDialog dialog(f.context);
    EXPECT_FALSE(enabled(dialog, "settingsReset"));
    EXPECT_EQ(tip(dialog, "settingsReset"),
              QStringLiteral("This program has no built-in customisation to reset to."));
    press(dialog, "settingsReset");
    EXPECT_TRUE(f.ran.isEmpty());
    // The kept file is another matter.
    EXPECT_TRUE(enabled(dialog, "settingsKeep"));
    EXPECT_TRUE(enabled(dialog, "settingsRevert"));
}

TEST(SettingsDialog, KeepAndRevertAreDisabledAndSayWhyWhenThereIsNoKeptFile)
{
    SettingsFixture f;
    f.context.keptFile.clear();
    SettingsDialog dialog(f.context);
    const QString why = QStringLiteral(
        "This session has no kept file; the environment variable KATANA_CUSTOMISATION names one.");
    for (const char* name : {"settingsKeep", "settingsRevert"}) {
        EXPECT_FALSE(enabled(dialog, name)) << name;
        EXPECT_EQ(tip(dialog, name), why) << name;
        press(dialog, name);
    }
    EXPECT_TRUE(f.ran.isEmpty());
    EXPECT_TRUE(enabled(dialog, "settingsReset"));
    // The row does not call the session kept and then say there is no kept
    // file: nothing is kept, and the start gives the built-in.
    EXPECT_EQ(shown(dialog, "settingsActiveKept"),
              QStringLiteral("Nothing is kept: the next start gives the built-in customisation. "
                             "The environment variable KATANA_CUSTOMISATION names a file to "
                             "keep one in."));
}

TEST(SettingsDialog, TheKeptRowOfASessionWithNoKeptFileAndNoBuiltInSaysNothingIsGiven)
{
    SettingsFixture f;
    f.context.keptFile.clear();
    f.context.hasBuiltIn = false;
    SettingsDialog dialog(f.context);
    EXPECT_EQ(shown(dialog, "settingsActiveKept"),
              QStringLiteral("Nothing is kept: the next start gives no customisation. "
                             "The environment variable KATANA_CUSTOMISATION names a file to "
                             "keep one in."));
}

TEST(SettingsDialog, KeepIsDisabledWhileTheSessionIsKeptAndComesBackWithTheFirstEdit)
{
    SettingsFixture f;
    f.install(CustomisationOrigin::Kept, true);
    SettingsDialog dialog(f.context);
    EXPECT_FALSE(enabled(dialog, "settingsKeep"));
    EXPECT_EQ(tip(dialog, "settingsKeep"),
              QStringLiteral("The session is already kept: the next start gives it."));
    EXPECT_TRUE(enabled(dialog, "settingsRevert"));
    press(dialog, "settingsKeep");
    EXPECT_TRUE(f.ran.isEmpty());

    // An edit made elsewhere - a line typed, an editor's commit - leaves the
    // session not kept, and the Document says so.
    f.document.setAutomation({.codesOnSurveyImport = false, .lineworkOnSurveyImport = true});
    processEvents();
    EXPECT_TRUE(enabled(dialog, "settingsKeep"));
    press(dialog, "settingsKeep");
    EXPECT_EQ(f.ran, (QStringList{kKeep}));
}

// ---- what a press costs the kept file, said before it is pressed --------------------------------

TEST(SettingsDialog, ResetSaysBeforeItIsPressedThatAKeptSessionsFileIsSetAside)
{
    SettingsFixture f;
    f.install(CustomisationOrigin::Kept, true);
    SettingsDialog dialog(f.context);
    const QString plain = QStringLiteral("Put the program's built-in customisation in the place "
                                         "of the session's (CUSTOMISE RESET)");
    // A kept session: its reset is followed by KEEP, and KEEP keeps a session
    // that is the built-in by setting the kept file aside. No question is
    // asked, so the button says it, with the file's name and the way back.
    EXPECT_EQ(tip(dialog, "settingsReset"),
              plain + QStringLiteral(".\nThis session is the kept one, so its reset is kept too: "
                                     "the kept file, when there is one, is set aside as C:/kept "
                                     "files/customisation.json.bak, and Revert to Kept then has "
                                     "none to read.\nImport that file with Replace instead of "
                                     "merging ticked to have it back."));

    // Not kept - an edit made elsewhere. A reset is then followed by nothing
    // and the kept file stands, so the tip says what the button does and no
    // more.
    f.document.setAutomation({.codesOnSurveyImport = false, .lineworkOnSurveyImport = true});
    processEvents();
    EXPECT_EQ(tip(dialog, "settingsReset"), plain);

    // A host that keeps no file has none to set aside, kept session or not.
    SettingsFixture none;
    none.context.keptFile.clear();
    none.install(CustomisationOrigin::BuiltIn, true);
    SettingsDialog without(none.context);
    EXPECT_EQ(tip(without, "settingsReset"), plain);
}

TEST(SettingsDialog, KeepSaysThatASessionThatIsTheBuiltInIsNotWrittenAndTheKeptFileIsSetAside)
{
    // The built-in, and not kept: what a reset typed on the command line
    // leaves where a file is kept.
    SettingsFixture f;
    f.install(CustomisationOrigin::BuiltIn, false);
    SettingsDialog dialog(f.context);
    EXPECT_TRUE(enabled(dialog, "settingsKeep"));
    EXPECT_EQ(tip(dialog, "settingsKeep"),
              QStringLiteral("The session is the built-in customisation, which every start gives "
                             "when nothing is kept: nothing is written, and the kept file, when "
                             "there is one, is set aside as C:/kept files/customisation.json.bak "
                             "(CUSTOMISE KEEP)"));

    // Edited, it is no longer the built-in, and Keep writes it.
    f.document.setAutomation({.codesOnSurveyImport = false, .lineworkOnSurveyImport = true});
    processEvents();
    EXPECT_EQ(tip(dialog, "settingsKeep"),
              QStringLiteral("Write the session's customisation to the kept file, which the next "
                             "start reads (CUSTOMISE KEEP)"));
}

// ---- what is edited in Settings is kept ---------------------------------------------------------

TEST(SettingsDialog, AnActionOnAKeptSessionWithAKeptFileIsFollowedByKeep)
{
    SettingsFixture f;
    f.install(CustomisationOrigin::BuiltIn, true);
    SettingsDialog dialog(f.context);
    // Each line does to the Document what its verb does: a switch set, a file
    // loaded and the built-in put back each leave the session not kept.
    f.answer = [&f](const QString& line) {
        if (line == kKeep) {
            f.document.setCustomisationKept(true);
        } else if (line == QStringLiteral("CUSTOMISE SET auto.linework=off")) {
            f.document.setAutomation(
                {.codesOnSurveyImport = true, .lineworkOnSurveyImport = false});
        } else if (line.startsWith(QStringLiteral("CUSTOMISE REPLACE "))) {
            f.install(CustomisationOrigin::Loaded, false);
        } else if (line.startsWith(QStringLiteral("CUSTOMISE \""))) {
            f.install(CustomisationOrigin::Loaded, false);
        } else if (line == QStringLiteral("CUSTOMISE RESET")) {
            f.install(CustomisationOrigin::BuiltIn, false);
        }
        return carriedOut();
    };

    press(dialog, "settingsAutoLinework");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE SET auto.linework=off"), kKeep}));
    EXPECT_TRUE(f.document.customisationState().kept);

    f.ran.clear();
    fill(dialog, "settingsImportPath", QStringLiteral("C:/data/site.json"));
    press(dialog, "settingsImport");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE \"C:/data/site.json\""), kKeep}));

    f.ran.clear();
    child<QCheckBox>(dialog, "settingsImportReplace")->setChecked(true);
    press(dialog, "settingsImport");
    EXPECT_EQ(f.ran,
              (QStringList{QStringLiteral("CUSTOMISE REPLACE \"C:/data/site.json\""), kKeep}));

    f.ran.clear();
    press(dialog, "settingsReset");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE RESET"), kKeep}));
    // Both lines and both answers are in the status, in the order they ran.
    EXPECT_EQ(status(dialog), QStringLiteral("> CUSTOMISE RESET\nDone.\n> CUSTOMISE KEEP\nDone."));
    EXPECT_FALSE(saysARefusal(dialog));
}

TEST(SettingsDialog, AnActionOnASessionThatWasNotKeptIsNotFollowedByKeep)
{
    SettingsFixture f;
    f.install(CustomisationOrigin::Loaded, false);
    SettingsDialog dialog(f.context);
    f.answer = [&f](const QString& line) {
        if (line == QStringLiteral("CUSTOMISE SET auto.codes=off")) {
            f.document.setAutomation(
                {.codesOnSurveyImport = false, .lineworkOnSurveyImport = true});
        } else if (line != kKeep) {
            f.install(CustomisationOrigin::Loaded, false);
        }
        return carriedOut();
    };
    press(dialog, "settingsAutoCodes");
    fill(dialog, "settingsImportPath", QStringLiteral("C:/data/site.json"));
    press(dialog, "settingsImport");
    press(dialog, "settingsReset");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE SET auto.codes=off"),
                                  QStringLiteral("CUSTOMISE \"C:/data/site.json\""),
                                  QStringLiteral("CUSTOMISE RESET")}));
}

TEST(SettingsDialog, AnActionOnAKeptSessionWithNoKeptFileIsNotFollowedByKeep)
{
    SettingsFixture f;
    f.context.keptFile.clear();
    f.install(CustomisationOrigin::BuiltIn, true);
    SettingsDialog dialog(f.context);
    f.answer = [&f](const QString& line) {
        if (line == QStringLiteral("CUSTOMISE SET auto.codes=off")) {
            f.document.setAutomation(
                {.codesOnSurveyImport = false, .lineworkOnSurveyImport = true});
        } else if (line != kKeep) {
            f.install(CustomisationOrigin::Loaded, false);
        }
        return carriedOut();
    };
    // Kept before each press, and not after it.
    press(dialog, "settingsAutoCodes");
    EXPECT_FALSE(f.document.customisationState().kept);
    f.install(CustomisationOrigin::BuiltIn, true);
    fill(dialog, "settingsImportPath", QStringLiteral("C:/data/site.json"));
    press(dialog, "settingsImport");
    EXPECT_FALSE(f.document.customisationState().kept);
    f.install(CustomisationOrigin::BuiltIn, true);
    press(dialog, "settingsReset");
    EXPECT_FALSE(f.document.customisationState().kept);
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE SET auto.codes=off"),
                                  QStringLiteral("CUSTOMISE \"C:/data/site.json\""),
                                  QStringLiteral("CUSTOMISE RESET")}));
}

TEST(SettingsDialog, KeepFollowsOnlyALineThatWasCarriedOutAndLeftTheSessionNotKept)
{
    SettingsFixture f;
    f.install(CustomisationOrigin::BuiltIn, true);
    SettingsDialog dialog(f.context);
    fill(dialog, "settingsImportPath", QStringLiteral("C:/data/site.json"));

    // Refused, and nothing is kept behind a refusal - not even one that left
    // the session not kept. No verb of the family does that today, each being
    // all or nothing, so the runner does it here: a switch set, and then the
    // refusal. That makes the refusal ALONE what stops the KEEP. (A runner
    // that changed nothing left the session kept, and "is it still kept"
    // stopped the KEEP by itself: this passed with "was the line carried out"
    // taken out of the dialog.)
    f.answer = [&f](const QString&) {
        f.document.setAutomation({.codesOnSurveyImport = false, .lineworkOnSurveyImport = true});
        return VerbOutcome{false, {}, QStringLiteral("NotFound: the file cannot be opened")};
    };
    press(dialog, "settingsImport");
    ASSERT_FALSE(f.document.customisationState().kept) << "the runner left the session not kept";
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE \"C:/data/site.json\"")}));
    EXPECT_EQ(status(dialog),
              QStringLiteral("> CUSTOMISE \"C:/data/site.json\"\nRefused: NotFound: the file "
                             "cannot be opened"));
    EXPECT_TRUE(saysARefusal(dialog));

    // Carried out, and the session is still the kept one: a reset to the
    // built-in when nothing is kept in its place.
    f.ran.clear();
    f.install(CustomisationOrigin::BuiltIn, true);
    ASSERT_TRUE(f.document.customisationState().kept);
    f.answer = [&f](const QString&) {
        f.install(CustomisationOrigin::BuiltIn, true);
        return carriedOut();
    };
    press(dialog, "settingsReset");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE RESET")}));

    // An export changes nothing, and Revert leaves the session as the kept
    // file has it: neither is ever followed - not even where, as here, the
    // runner leaves a session that was kept not kept.
    f.ran.clear();
    f.answer = [&f](const QString&) {
        f.install(CustomisationOrigin::Loaded, false);
        return carriedOut();
    };
    fill(dialog, "settingsExportPath", QStringLiteral("C:/data/out.json"));
    ASSERT_TRUE(f.document.customisationState().kept);
    press(dialog, "settingsExport");
    EXPECT_FALSE(f.document.customisationState().kept);
    f.install(CustomisationOrigin::BuiltIn, true);
    press(dialog, "settingsRevert");
    EXPECT_FALSE(f.document.customisationState().kept);
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE EXPORT \"C:/data/out.json\""),
                                  QStringLiteral("CUSTOMISE REVERT")}));
}

TEST(SettingsDialog, AKeepThatIsRefusedBehindALineIsShownAsTheRefusalItIs)
{
    SettingsFixture f;
    f.install(CustomisationOrigin::Kept, true);
    SettingsDialog dialog(f.context);
    f.answer = [&f](const QString& line) {
        if (line == kKeep) {
            return VerbOutcome{false, {},
                               QStringLiteral("InvalidState: CUSTOMISE KEEP: the kept "
                                              "customisation changed on disk")};
        }
        f.document.setAutomation({.codesOnSurveyImport = false, .lineworkOnSurveyImport = true});
        return VerbOutcome{true, QStringLiteral("set auto.codes=off"), {}};
    };
    press(dialog, "settingsAutoCodes");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE SET auto.codes=off"), kKeep}));
    EXPECT_EQ(status(dialog),
              QStringLiteral("> CUSTOMISE SET auto.codes=off\nset auto.codes=off\n> CUSTOMISE "
                             "KEEP\nRefused: InvalidState: CUSTOMISE KEEP: the kept "
                             "customisation changed on disk"));
    EXPECT_TRUE(saysARefusal(dialog));
    // The switch was set all the same, and the page says the session is not
    // kept, with Keep there to try again.
    processEvents();
    EXPECT_FALSE(checked(dialog, "settingsAutoCodes"));
    EXPECT_TRUE(shown(dialog, "settingsActiveKept").startsWith(QStringLiteral("Not kept:")));
    EXPECT_TRUE(enabled(dialog, "settingsKeep"));
}

// ---- the status ---------------------------------------------------------------------------------

TEST(SettingsDialog, TheStatusShowsTheLineRunAndTheVerbsOwnWords)
{
    SettingsFixture f;
    SettingsDialog dialog(f.context);
    f.answer = [](const QString&) {
        return VerbOutcome{true,
                           QStringLiteral("reset name=Site definitions=3 codes=2 rules=3 kept=yes"),
                           {}};
    };
    press(dialog, "settingsReset");
    EXPECT_EQ(status(dialog),
              QStringLiteral("> CUSTOMISE RESET\nreset name=Site definitions=3 codes=2 rules=3 "
                             "kept=yes"));
    EXPECT_FALSE(saysARefusal(dialog));

    f.answer = [](const QString&) {
        return VerbOutcome{false, {},
                           QStringLiteral("InvalidState: CUSTOMISE REVERT needs a kept "
                                          "customisation file, and this session has none")};
    };
    press(dialog, "settingsRevert");
    EXPECT_EQ(status(dialog),
              QStringLiteral("> CUSTOMISE REVERT\nRefused: InvalidState: CUSTOMISE REVERT needs "
                             "a kept customisation file, and this session has none"));
    EXPECT_TRUE(saysARefusal(dialog));
    // What a line answered is the executor's to log, not the dialog's.
    EXPECT_TRUE(f.logged.empty());
}

TEST(SettingsDialog, WithNoRunnerAPressSaysNothingCanRunItsLineAndABoxGoesBack)
{
    SettingsFixture f;
    f.context.run = {};
    SettingsDialog dialog(f.context);
    press(dialog, "settingsKeep");
    EXPECT_EQ(status(dialog), QStringLiteral("Nothing here can run CUSTOMISE KEEP: Settings was "
                                             "given no command line."));
    EXPECT_TRUE(saysARefusal(dialog));
    press(dialog, "settingsAutoCodes");
    EXPECT_EQ(status(dialog), QStringLiteral("Nothing here can run CUSTOMISE SET auto.codes=off: "
                                             "Settings was given no command line."));
    EXPECT_TRUE(checked(dialog, "settingsAutoCodes"));
    EXPECT_TRUE(f.document.customisationState().automation.codesOnSurveyImport);
}

// ---- what is active -----------------------------------------------------------------------------

TEST(SettingsDialog, TheLabelsFollowTheDocument)
{
    SettingsFixture f;
    SettingsDialog dialog(f.context);
    // A Document nothing was installed in.
    EXPECT_EQ(shown(dialog, "settingsActiveName"), QStringLiteral("No customisation is loaded"));
    EXPECT_EQ(shown(dialog, "settingsActiveOrigin"), QStringLiteral("None"));
    EXPECT_EQ(shown(dialog, "settingsActiveDefinitions"), QStringLiteral("0"));
    EXPECT_EQ(shown(dialog, "settingsActiveSymbols"), QStringLiteral("0"));
    EXPECT_EQ(shown(dialog, "settingsActiveRules"), QStringLiteral("0"));
    EXPECT_EQ(shown(dialog, "settingsActiveKept"),
              QStringLiteral("Not kept: the next start does not give this customisation.\nKept "
                             "file: C:/kept files/customisation.json"));
    EXPECT_TRUE(child<QLabel>(dialog, "settingsActiveProblems")->isHidden());

    // kSite as the program's own, kept: 3 definitions, 2 of them symbols, and
    // 3 rules over FN* and PG*.
    f.install(CustomisationOrigin::BuiltIn, true);
    EXPECT_EQ(shown(dialog, "settingsActiveName"), QStringLiteral("No customisation is loaded"))
        << "the page is read again from the event loop, not inside the change";
    processEvents();
    EXPECT_EQ(shown(dialog, "settingsActiveName"), QStringLiteral("Site"));
    EXPECT_EQ(shown(dialog, "settingsActiveOrigin"), QStringLiteral("Built in"));
    EXPECT_EQ(shown(dialog, "settingsActiveDefinitions"), QStringLiteral("3"));
    EXPECT_EQ(shown(dialog, "settingsActiveSymbols"), QStringLiteral("2"));
    EXPECT_EQ(shown(dialog, "settingsActiveRules"), QStringLiteral("3 over 2 codes"));
    EXPECT_EQ(shown(dialog, "settingsActiveKept"),
              QStringLiteral("Kept: the next start gives this customisation.\nKept file: "
                             "C:/kept files/customisation.json"));

    // A second, with other counts: the survey fixture alone, read from the
    // user's kept file - 11 rules over 8 codes and no definition.
    const auto installed = f.document.installCustomisation(
        katana::qt::test::readCustomisationFixture("test_survey"), CustomisationOrigin::Kept, true);
    ASSERT_TRUE(installed.ok());
    processEvents();
    EXPECT_EQ(shown(dialog, "settingsActiveName"), QStringLiteral("test_survey"));
    EXPECT_EQ(shown(dialog, "settingsActiveOrigin"), QStringLiteral("Kept by you"));
    EXPECT_EQ(shown(dialog, "settingsActiveDefinitions"), QStringLiteral("0"));
    EXPECT_EQ(shown(dialog, "settingsActiveSymbols"), QStringLiteral("0"));
    EXPECT_EQ(shown(dialog, "settingsActiveRules"), QStringLiteral("11 over 8 codes"));

    // The three fixtures loaded into it: the 3 linestyles and the 4 symbols.
    katana::qt::test::installCustomisationFixtures(f.document, {"test_linestyles", "test_symbols"});
    processEvents();
    EXPECT_EQ(shown(dialog, "settingsActiveOrigin"), QStringLiteral("Loaded this session"));
    EXPECT_EQ(shown(dialog, "settingsActiveDefinitions"), QStringLiteral("7"));
    EXPECT_EQ(shown(dialog, "settingsActiveSymbols"), QStringLiteral("4"));
    EXPECT_EQ(shown(dialog, "settingsActiveRules"), QStringLiteral("11 over 8 codes"));
    EXPECT_TRUE(shown(dialog, "settingsActiveKept").startsWith(QStringLiteral("Not kept:")));

    // An editor's commit: one definition fewer, by the raw setter, which
    // moves the library's generation and marks the session edited.
    katana::entity::StyleLibrary library = f.document.styleLibrary();
    ASSERT_TRUE(library.remove("TEST Tree").ok());
    f.document.setStyleLibrary(std::move(library));
    processEvents();
    EXPECT_EQ(shown(dialog, "settingsActiveOrigin"), QStringLiteral("Edited this session"));
    EXPECT_EQ(shown(dialog, "settingsActiveDefinitions"), QStringLiteral("6"));
    EXPECT_EQ(shown(dialog, "settingsActiveSymbols"), QStringLiteral("3"));
    // And a second, of a session already edited: only the library's own
    // generation moves, and the counts follow that too.
    library = f.document.styleLibrary();
    ASSERT_TRUE(library.remove("TEST Gate").ok());
    f.document.setStyleLibrary(std::move(library));
    processEvents();
    EXPECT_EQ(shown(dialog, "settingsActiveDefinitions"), QStringLiteral("5"));
    EXPECT_EQ(shown(dialog, "settingsActiveSymbols"), QStringLiteral("3"));

    EXPECT_TRUE(f.ran.isEmpty());
}

TEST(SettingsDialog, WhatTheStartOfTheSessionFoundIsShownAndTheRowIsHiddenWhenItFoundNothing)
{
    // The Document keeps what its start found (Document::setCustomisationStart,
    // which cad::startCustomisation calls): the problems, a sentence each, and
    // whether the kept customisation was made from another built-in. The page
    // shows the sentences as they are, one a line, and after them its own
    // sentence for the second - written out here from the dialog's source,
    // which words it after the window's start-up line.
    SettingsFixture f;
    SettingsDialog dialog(f.context);
    dialog.show();
    auto* row = child<QLabel>(dialog, "settingsActiveProblems");
    EXPECT_TRUE(row->isHidden()) << "a session whose start found nothing";
    const QString fromAnother =
        QStringLiteral("The kept customisation was made from another built-in customisation "
                       "than this program has; Reset to Built-in gives this program's.");

    f.document.setCustomisationStart({"the kept file was not read", "the built-in did not parse"},
                                     false);
    EXPECT_TRUE(row->isHidden()) << "read again from the event loop, not inside the change";
    processEvents();
    EXPECT_FALSE(row->isHidden());
    EXPECT_TRUE(row->isVisibleTo(&dialog));
    EXPECT_EQ(row->text(),
              QStringLiteral("the kept file was not read\nthe built-in did not parse"));
    EXPECT_TRUE(row->styleSheet().contains(katana::qt::theme::error().name()))
        << "in the colour of an error";

    // Nothing went wrong, and the kept customisation is from another built-in.
    f.document.setCustomisationStart({}, true);
    processEvents();
    EXPECT_FALSE(row->isHidden());
    EXPECT_EQ(row->text(), fromAnother);

    // Both: the problems first.
    f.document.setCustomisationStart({"the built-in did not parse"}, true);
    processEvents();
    EXPECT_EQ(row->text(), QStringLiteral("the built-in did not parse\n") + fromAnother);

    // It is of the START: an edit of the session since does not take it away.
    f.install(CustomisationOrigin::Loaded, false);
    processEvents();
    EXPECT_EQ(row->text(), QStringLiteral("the built-in did not parse\n") + fromAnother);

    f.document.setCustomisationStart({}, false);
    processEvents();
    EXPECT_TRUE(row->isHidden());
    EXPECT_TRUE(row->text().isEmpty());
    EXPECT_TRUE(f.ran.isEmpty()) << "showing it runs no line";
}

TEST(SettingsDialog, AKeptFileThatDidNotReadWhenTheSessionStartedIsSaidOnThePage)
{
    // The start itself, as a front end makes it: kSite is the program's
    // built-in, and the kept file holds JSON of another format, which the
    // reader refuses as not a Katana customisation file. The built-in then
    // stands in (cad::startCustomisation), and the sentence saying so is the
    // one the page shows - the very words the start reported.
    QTemporaryDir scratch;
    ASSERT_TRUE(scratch.isValid());
    const QString keptFile = scratch.filePath(QStringLiteral("customisation.json"));
    QFile file(keptFile);
    ASSERT_TRUE(file.open(QIODevice::WriteOnly));
    file.write("{\"format\": \"another-format\", \"version\": 1, \"name\": \"x\"}\n");
    file.close();

    SettingsFixture f;
    katana::cad::CustomisationHost host;
    host.builtIn.customisation =
        std::make_shared<const katana::entity::Customisation>(customisationFromText(kSite));
    host.builtIn.digest = katana::entity::customisationDigest(kSite);
    host.keptFile = katana::core::pathFromUtf8(keptFile.toStdString());
    const katana::cad::CustomisationStart started =
        katana::cad::startCustomisation(f.document, host);
    ASSERT_EQ(started.installed, CustomisationOrigin::BuiltIn);
    ASSERT_EQ(started.problems.size(), 1u);
    const QString problem = QString::fromStdString(started.problems.front());
    ASSERT_TRUE(problem.startsWith(QStringLiteral(
        "the kept customisation is not read, so the built-in customisation is used: ")))
        << problem.toStdString();
    ASSERT_TRUE(problem.contains(QStringLiteral("not a Katana customisation file")))
        << problem.toStdString();

    f.context.keptFile = keptFile;
    SettingsDialog dialog(f.context);
    dialog.show();
    const auto* row = child<QLabel>(dialog, "settingsActiveProblems");
    EXPECT_FALSE(row->isHidden());
    EXPECT_EQ(row->text(), problem);
    EXPECT_EQ(shown(dialog, "settingsActiveName"), QStringLiteral("Site"));
    EXPECT_EQ(shown(dialog, "settingsActiveOrigin"), QStringLiteral("Built in"));
}

TEST(SettingsDialog, ASessionEditedBeforeAnythingWasInstalledIsShownAsNotNamed)
{
    SettingsFixture f;
    SettingsDialog dialog(f.context);
    ASSERT_EQ(shown(dialog, "settingsActiveName"), QStringLiteral("No customisation is loaded"));

    // An editor's commit into a session nothing was ever installed in: the
    // two rules of kOneCode, by the raw setter. The session then has rules
    // and an origin, and no name - which is not "nothing is loaded".
    f.document.setSurveyMap(customisationFromText(kOneCode).map);
    processEvents();
    ASSERT_TRUE(f.document.customisationState().name.empty());
    EXPECT_EQ(shown(dialog, "settingsActiveName"), QStringLiteral("Not named"));
    EXPECT_EQ(shown(dialog, "settingsActiveOrigin"), QStringLiteral("Edited this session"));
    EXPECT_EQ(shown(dialog, "settingsActiveDefinitions"), QStringLiteral("0"));
    EXPECT_EQ(shown(dialog, "settingsActiveRules"), QStringLiteral("2 over 1 code"));
}

TEST(SettingsDialog, RulesOverOneCodeAreCountedInTheSingular)
{
    // kOneCode: two rules, both of PG*.
    SettingsFixture f;
    const auto installed = f.document.installCustomisation(customisationFromText(kOneCode),
                                                           CustomisationOrigin::Loaded);
    ASSERT_TRUE(installed.ok()) << (installed.ok() ? "" : installed.error().describe());
    SettingsDialog dialog(f.context);
    EXPECT_EQ(shown(dialog, "settingsActiveName"), QStringLiteral("Pegs"));
    EXPECT_EQ(shown(dialog, "settingsActiveRules"), QStringLiteral("2 over 1 code"));
    // And kSite in its place, over two: the plural.
    f.install(CustomisationOrigin::Loaded, false);
    processEvents();
    EXPECT_EQ(shown(dialog, "settingsActiveRules"), QStringLiteral("3 over 2 codes"));
}

TEST(SettingsDialog, TheNoticeIsHiddenUntilAskedAndItsToggleIsHiddenWhenThereIsNone)
{
    SettingsFixture f;
    SettingsDialog dialog(f.context);
    dialog.show();
    auto* toggle = child<QCheckBox>(dialog, "settingsShowNotice");
    auto* notice = child<QPlainTextEdit>(dialog, "settingsNotice");
    // Nothing is loaded, so there is no notice to ask for.
    EXPECT_TRUE(toggle->isHidden());
    EXPECT_TRUE(notice->isHidden());

    f.install(CustomisationOrigin::BuiltIn, true);
    processEvents();
    EXPECT_FALSE(toggle->isHidden());
    EXPECT_TRUE(toggle->isVisibleTo(&dialog));
    EXPECT_FALSE(toggle->isChecked());
    EXPECT_TRUE(notice->isHidden()) << "hidden until asked";
    EXPECT_TRUE(notice->isReadOnly());

    toggle->click();
    EXPECT_FALSE(notice->isHidden());
    EXPECT_TRUE(notice->isVisibleTo(&dialog));
    EXPECT_EQ(notice->toPlainText(),
              QStringLiteral("Site set: for this job only.\nAsk before passing it on."));
    toggle->click();
    EXPECT_TRUE(notice->isHidden());

    // A session made of four customisations: its own notice, then each
    // source's under its name (fixture_customisation.hpp, installNoticedSession).
    {
        SettingsFixture several;
        katana::qt::test::installNoticedSession(several.document);
        SettingsDialog merged(several.context);
        child<QCheckBox>(merged, "settingsShowNotice")->click();
        EXPECT_EQ(child<QPlainTextEdit>(merged, "settingsNotice")->toPlainText(),
                  QStringLiteral("Base: all rights reserved.\n\n"
                                 "From client:\nClient codes, for this job only.\n\n"
                                 "From marks:\nMarks drawn by hand.\n\n"
                                 "From tints:\nTints: free to use."));
    }

    // Shown, and then a customisation with no notice takes its place: the
    // notice goes, and the toggle with it.
    toggle->click();
    ASSERT_FALSE(notice->isHidden());
    const auto installed = f.document.installCustomisation(
        katana::qt::test::readCustomisationFixture("test_symbols"), CustomisationOrigin::Loaded);
    ASSERT_TRUE(installed.ok());
    processEvents();
    EXPECT_TRUE(toggle->isHidden());
    EXPECT_FALSE(toggle->isChecked());
    EXPECT_TRUE(notice->isHidden());
    EXPECT_TRUE(notice->toPlainText().isEmpty());
    EXPECT_TRUE(f.ran.isEmpty());
}

TEST(SettingsDialog, TheLineworkCodesAreShownAsTheDocumentSpellsThemAndEditedInOnePlace)
{
    SettingsFixture f;
    SettingsDialog dialog(f.context);
    // The seven defaults (entity/linework_codes.hpp), each under the words a
    // person reads its control by - those of the Linework tab the button
    // opens (src/katana_qt/customisation/code_manager_tabs.cpp:
    // Start, End, Close, Begin curve, End curve, Join to point, Rectangle).
    // These two expectations named the controls as a file and CUSTOMISE SET
    // do (`arcStart=BC`) until the page was made to say them in plain words;
    // they were rewritten from the tab's labels, not from what a run showed.
    EXPECT_EQ(shown(dialog, "settingsLinework"),
              QStringLiteral("Start: ST, End: END, Close: CL, Begin curve: BC, End curve: EC, "
                             "Join to point: JPN, Rectangle: RECT"));

    katana::entity::LineworkCodes codes;
    codes.start = "S";
    codes.rectangle.clear(); // that control switched off
    ASSERT_TRUE(f.document.setLineworkCodes(codes).ok());
    processEvents();
    EXPECT_EQ(shown(dialog, "settingsLinework"),
              QStringLiteral("Start: S, End: END, Close: CL, Begin curve: BC, End curve: EC, "
                             "Join to point: JPN, Rectangle: (off)"));

    // Nothing on the page edits them: the button opens their one editor.
    EXPECT_EQ(dialog.findChildren<QLineEdit*>().size(), 2) << "the two path fields, and no more";
    press(dialog, "settingsEditLinework");
    EXPECT_EQ(f.openedLinework, 1);
    EXPECT_EQ(f.openedCodes, 0);
    press(dialog, "settingsOpenCodes");
    press(dialog, "settingsOpenSymbols");
    EXPECT_EQ(f.openedLinework, 1);
    EXPECT_EQ(f.openedCodes, 1);
    EXPECT_EQ(f.openedSymbols, 1);
    EXPECT_TRUE(f.ran.isEmpty());
}

TEST(SettingsDialog, TheOneListOfLineworkLabelsNamesEveryControlInTheOrderTheFormatListsThem)
{
    // The list Settings reads (linework_labels.hpp) against the list of the
    // controls themselves (entity::lineworkCodeMembers): one label for each,
    // in that order. A control added to the format and not given a label
    // fails here, where the page would have shown it by its file name.
    const auto members = katana::entity::lineworkCodeMembers();
    ASSERT_EQ(members.size(), katana::qt::kLineworkControlLabels.size());
    for (std::size_t i = 0; i < members.size(); ++i) {
        EXPECT_EQ(members[i].name, katana::qt::kLineworkControlLabels[i].member) << i;
        EXPECT_FALSE(katana::qt::lineworkControlLabel(members[i].name).isEmpty()) << i;
        EXPECT_NE(katana::qt::lineworkControlLabel(members[i].name).toStdString(),
                  std::string(members[i].name))
            << "a word for a person, not the format's";
    }
    // By hand, the two a file's name says least about.
    EXPECT_EQ(katana::qt::lineworkControlLabel("arcStart"), QStringLiteral("Begin curve"));
    EXPECT_EQ(katana::qt::lineworkControlLabel("join"), QStringLiteral("Join to point"));
    // One the list does not know is shown by the name a file gives it.
    EXPECT_EQ(katana::qt::lineworkControlLabel("spiral"), QStringLiteral("spiral"));
}

TEST(SettingsDialog, TheLineworkControlsAreCalledWhatTheirOneEditorCallsThem)
{
    // The editor itself: the Survey Code Manager, whose Linework tab labels
    // its seven fields in a form from the one list Settings reads
    // (linework_labels.hpp). The words are read from the tab, not written
    // out again here: this is the test that finds a field the tab built
    // without that list, or a list that gained a control the tab lacks.
    Document managed;
    katana::qt::CustomisationContext managerContext;
    managerContext.document = &managed;
    managerContext.log = [](const QString&, bool) {};
    katana::qt::SurveyCodeManagerDialog manager(managerContext);
    const QWidget* tab = child<QWidget>(manager, "lineworkTab");
    ASSERT_NE(tab, nullptr);

    // Each field of the tab, in the order the page lists the controls, with
    // the spelling a Document nothing has set them in gives it
    // (entity/linework_codes.hpp).
    const std::array<std::pair<const char*, const char*>, 7> fields{{
        {"lineworkStart", "ST"},
        {"lineworkEnd", "END"},
        {"lineworkClose", "CL"},
        {"lineworkArcStart", "BC"},
        {"lineworkArcEnd", "EC"},
        {"lineworkJoin", "JPN"},
        {"lineworkRectangle", "RECT"},
    }};
    QStringList words;
    for (const auto& [name, spelling] : fields) {
        auto* field = tab->findChild<QLineEdit*>(QString::fromLatin1(name));
        ASSERT_NE(field, nullptr) << name;
        const auto* form = qobject_cast<QFormLayout*>(field->parentWidget()->layout());
        ASSERT_NE(form, nullptr) << name;
        const auto* label = qobject_cast<QLabel*>(form->labelForField(field));
        ASSERT_NE(label, nullptr) << name;
        ASSERT_FALSE(label->text().isEmpty()) << name;
        words << label->text() + QStringLiteral(": ") + QString::fromLatin1(spelling);
    }

    SettingsFixture f;
    SettingsDialog dialog(f.context);
    EXPECT_EQ(shown(dialog, "settingsLinework"), words.join(QStringLiteral(", ")))
        << "Settings and the Linework tab call a control by different words: make the tab "
           "label its fields from kLineworkControlLabels (linework_labels.hpp), the list "
           "Settings reads";
}

TEST(SettingsDialog, AContextThatFillsNothingGivesADialogThatRunsNothing)
{
    // Every member of the context at its default.
    SettingsDialog dialog(SettingsContext{});
    for (const char* name :
         {"settingsImport", "settingsExport", "settingsReset", "settingsKeep", "settingsRevert",
          "settingsAutoCodes", "settingsAutoLinework", "settingsEditLinework", "settingsOpenCodes",
          "settingsOpenSymbols", "settingsImportBrowse", "settingsExportBrowse"}) {
        EXPECT_FALSE(enabled(dialog, name)) << name;
    }
    EXPECT_TRUE(enabled(dialog, "settingsClose"));
    EXPECT_EQ(status(dialog),
              QStringLiteral("Settings was opened on no drawing: nothing here acts on anything."));
    katana::qt::test::paint(dialog);

    // With a Document and nothing else, the buttons that have no editor to
    // open are the ones that are off.
    Document document;
    SettingsContext context;
    context.document = &document;
    SettingsDialog bare(context);
    for (const char* name : {"settingsEditLinework", "settingsOpenCodes", "settingsOpenSymbols",
                             "settingsReset", "settingsKeep", "settingsRevert"}) {
        EXPECT_FALSE(enabled(bare, name)) << name;
    }
    EXPECT_TRUE(enabled(bare, "settingsImport"));
    EXPECT_TRUE(enabled(bare, "settingsExport"));
}

// ---- Browse -------------------------------------------------------------------------------------

TEST(SettingsDialog, HeadlessBrowseOpensNoFileDialogAndSaysWhatToType)
{
    SettingsFixture f;
    f.headless = true;
    SettingsDialog dialog(f.context);
    dialog.show();
    FileDialogAnswer answer;

    press(dialog, "settingsImportBrowse");
    processEvents();
    EXPECT_EQ(status(dialog),
              QStringLiteral("A headless session opens no file dialog: type CUSTOMISE [REPLACE] "
                             "<file> instead, or fill settingsImportPath."));
    EXPECT_TRUE(saysARefusal(dialog));
    press(dialog, "settingsExportBrowse");
    processEvents();
    EXPECT_EQ(status(dialog),
              QStringLiteral("A headless session opens no file dialog: type CUSTOMISE EXPORT "
                             "<file> instead, or fill settingsExportPath."));

    EXPECT_EQ(answer.seen, 0) << "a file dialog opened: " << answer.title.toStdString();
    EXPECT_TRUE(f.ran.isEmpty());
    EXPECT_TRUE(child<QLineEdit>(dialog, "settingsImportPath")->text().isEmpty());
    ASSERT_EQ(f.logged.size(), 2u);
    EXPECT_TRUE(f.logged.front().second);
}

TEST(SettingsDialog, BrowseOffersKatanaCustomisationFilesAndOnlyFillsTheField)
{
    SettingsFixture f;
    SettingsDialog dialog(f.context);
    dialog.show();
    const QString filter = QStringLiteral("Katana customisation (*.customisation.json *.json)");

    // Import: the committed symbols fixture chosen, as Open would.
    const QString file =
        pathText(katana::qt::test::customisationFixtureFile("test_symbols"));
    ASSERT_TRUE(QFileInfo::exists(file)) << file.toStdString();
    {
        FileDialogAnswer answer(file);
        press(dialog, "settingsImportBrowse");
        ASSERT_EQ(answer.seen, 1) << "Browse opens one file dialog";
        EXPECT_EQ(answer.title, QStringLiteral("Import a Katana Customisation"));
        EXPECT_TRUE(answer.filters.contains(filter)) << answer.filters.join(" ;; ").toStdString();
    }
    const QString chosen = child<QLineEdit>(dialog, "settingsImportPath")->text();
    EXPECT_TRUE(QFileInfo(chosen) == QFileInfo(file)) << chosen.toStdString();
    EXPECT_FALSE(chosen.contains('\\')) << chosen.toStdString();

    // Export: cancelled, which leaves the field as it was.
    fill(dialog, "settingsExportPath", QStringLiteral("C:/data/out.json"));
    {
        FileDialogAnswer answer;
        press(dialog, "settingsExportBrowse");
        ASSERT_EQ(answer.seen, 1) << "Browse opens one file dialog";
        EXPECT_EQ(answer.title, QStringLiteral("Export the Customisation"));
        EXPECT_TRUE(answer.filters.contains(filter)) << answer.filters.join(" ;; ").toStdString();
    }
    EXPECT_EQ(child<QLineEdit>(dialog, "settingsExportPath")->text(),
              QStringLiteral("C:/data/out.json"));

    // Choosing a file loads and writes nothing: Import and Export do.
    EXPECT_TRUE(f.ran.isEmpty());
    EXPECT_TRUE(status(dialog).isEmpty());
}

TEST(SettingsDialog, ExportsBrowseFillsTheFieldAndEndsANameWithNoJsonEndingAsACustomisationFile)
{
    SettingsFixture f;
    SettingsDialog dialog(f.context);
    dialog.show();
    // A folder of this test's own, with a blank in its name and nothing in
    // it: a save dialog asks before it takes a file that is there.
    QTemporaryDir scratch;
    ASSERT_TRUE(scratch.isValid());
    const QString folder = scratch.filePath(QStringLiteral("site sets"));
    ASSERT_TRUE(QDir().mkpath(folder));
    // The name typed in the dialog and accepted, and the file name the field
    // then holds.
    const std::array<std::pair<const char*, const char*>, 4> names{{
        // No ending: given the one Import's own filter shows.
        {"all of it", "all of it.customisation.json"},
        // Ending .json, in any letter case: the person's own, as it is.
        {"plain.json", "plain.json"},
        {"LOUD.JSON", "LOUD.JSON"},
        {"whole.customisation.json", "whole.customisation.json"},
    }};
    for (const auto& [typed, expected] : names) {
        {
            FileDialogAnswer answer(folder + QLatin1Char('/') + QString::fromLatin1(typed));
            press(dialog, "settingsExportBrowse");
            ASSERT_EQ(answer.seen, 1) << typed;
        }
        const QString chosen = child<QLineEdit>(dialog, "settingsExportPath")->text();
        EXPECT_EQ(QFileInfo(chosen).fileName(), QString::fromLatin1(expected)) << typed;
        EXPECT_TRUE(QDir(QFileInfo(chosen).absolutePath()) == QDir(folder))
            << chosen.toStdString();
        EXPECT_FALSE(chosen.contains('\\')) << chosen.toStdString();
    }
    // Choosing writes nothing and runs nothing: Export does.
    EXPECT_TRUE(f.ran.isEmpty()) << f.ran.join(" | ").toStdString();
    EXPECT_TRUE(QDir(folder).isEmpty());
    EXPECT_TRUE(status(dialog).isEmpty());
}

TEST(SettingsDialog, BrowseOpensWhereThePathInTheFieldPointsInItsQuotesOrOutOfThem)
{
    SettingsFixture f;
    SettingsDialog dialog(f.context);
    dialog.show();
    // A folder for each press that no file dialog has been in before: a
    // Browse that could not read the field opens where the last one was left.
    const std::array<std::pair<const char*, const char*>, 2> browses{{
        {"settingsImportBrowse", "settingsImportPath"},
        {"settingsExportBrowse", "settingsExportPath"},
    }};
    for (const auto& [button, field] : browses) {
        for (const bool quoted : {false, true}) {
            QTemporaryDir scratch;
            ASSERT_TRUE(scratch.isValid());
            const QString file =
                QDir::toNativeSeparators(scratch.filePath(QStringLiteral("here.json")));
            fill(dialog, field, quoted ? QLatin1Char('"') + file + QLatin1Char('"') : file);
            FileDialogAnswer answer; // cancelled: only where it opened is asked
            press(dialog, button);
            ASSERT_EQ(answer.seen, 1) << button;
            EXPECT_TRUE(QDir(answer.directory) == QDir(scratch.path()))
                << button << (quoted ? " in quotes" : "") << " opened in "
                << answer.directory.toStdString();
        }
    }
    EXPECT_TRUE(f.ran.isEmpty());
}

// ---- no default button, no modal box ------------------------------------------------------------

TEST(SettingsDialog, NoButtonIsADefaultAndEnterInAFieldPressesNothing)
{
    SettingsFixture f;
    SettingsDialog dialog(f.context);
    ASSERT_TRUE(katana::qt::test::showActive(dialog));
    fill(dialog, "settingsImportPath", QStringLiteral("C:/data/site.json"));
    fill(dialog, "settingsExportPath", QStringLiteral("C:/data/out.json"));
    ASSERT_FALSE(dialog.findChildren<QPushButton*>().isEmpty());
    for (const QPushButton* button : dialog.findChildren<QPushButton*>()) {
        EXPECT_FALSE(button->isDefault()) << button->objectName().toStdString();
        EXPECT_FALSE(button->autoDefault()) << button->objectName().toStdString();
    }
    for (const char* name : {"settingsImportPath", "settingsExportPath"}) {
        QLineEdit* field = child<QLineEdit>(dialog, name);
        field->setFocus();
        QKeyEvent enter(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier);
        QCoreApplication::sendEvent(field, &enter);
    }
    processEvents();
    EXPECT_TRUE(f.ran.isEmpty()) << f.ran.join(" | ").toStdString();
    EXPECT_TRUE(dialog.isVisible());
    EXPECT_EQ(QApplication::activeModalWidget(), nullptr);

    // Close hides it; it is kept by whoever opened it, with what was typed.
    press(dialog, "settingsClose");
    EXPECT_FALSE(dialog.isVisible());
    EXPECT_EQ(child<QLineEdit>(dialog, "settingsImportPath")->text(),
              QStringLiteral("C:/data/site.json"));
}

// ---- the words ----------------------------------------------------------------------------------

TEST(SettingsDialog, NoTextItShowsNamesAnotherProgramOrAFileOfAnOlderFormat)
{
    QStringList seen;
    const auto read = [&seen](QWidget& dialog) { seen << textsIn(dialog); };
    // Cancels whatever file dialog a press here opens, and counts them.
    FileDialogAnswer fileDialogs;

    // As it opens; with a customisation, its notice shown; and every refusal
    // it words itself.
    SettingsFixture f;
    SettingsDialog dialog(f.context);
    dialog.show();
    read(dialog);
    f.install(CustomisationOrigin::Kept, true);
    processEvents();
    child<QCheckBox>(dialog, "settingsShowNotice")->click();
    read(dialog);
    for (const char* name : {"settingsImport", "settingsExport"}) {
        press(dialog, name); // no file is named
        read(dialog);
    }
    fill(dialog, "settingsImportPath", QStringLiteral("C:/data/a \"b\".json"));
    press(dialog, "settingsImport"); // no line can carry it
    read(dialog);
    fill(dialog, "settingsImportPath", QStringLiteral("C:/data/site.json"));
    f.dirty = true;
    for (const char* name : {"settingsImport", "settingsReset", "settingsRevert"}) {
        press(dialog, name); // the code manager holds edits
        read(dialog);
    }
    f.dirty = false;
    f.headless = true;
    for (const char* name : {"settingsImportBrowse", "settingsExportBrowse"}) {
        press(dialog, name); // no file dialog
        read(dialog);
    }
    ASSERT_EQ(fileDialogs.seen, 0);
    // The file dialogs a person gets, caught as they open.
    f.headless = false;
    for (const char* name : {"settingsImportBrowse", "settingsExportBrowse"}) {
        press(dialog, name);
        seen << fileDialogs.title << fileDialogs.filters;
    }
    ASSERT_EQ(fileDialogs.seen, 2);
    f.answer = [](const QString&) { return VerbOutcome{false, {}, {}}; };
    press(dialog, "settingsReset"); // refused, with no words of the verb's
    read(dialog);
    // Up to here the session was the kept one, and Reset's tip said what a
    // press costs the kept file. The built-in and not kept: Keep's says it.
    f.install(CustomisationOrigin::BuiltIn, false);
    processEvents();
    read(dialog);

    // What the host does not have, and nothing to run a line.
    {
        SettingsFixture without;
        without.context.hasBuiltIn = false;
        without.context.keptFile.clear();
        without.context.run = {};
        SettingsDialog bare(without.context);
        press(bare, "settingsAutoCodes");
        read(bare);
    }
    // No drawing at all, and a drawing that has closed.
    {
        SettingsDialog none(SettingsContext{});
        read(none);
        auto document = std::make_unique<Document>();
        SettingsContext context;
        context.document = document.get();
        SettingsDialog closed(context);
        document.reset();
        closed.refresh();
        read(closed);
    }

    for (const QString& text : seen) {
        // "1" "2d": the word the page must not say, written so that this file does not.
        for (const char* word : {"1" "2d", ".4d", "mapfile"}) {
            EXPECT_FALSE(text.contains(QString::fromLatin1(word), Qt::CaseInsensitive))
                << "\"" << word << "\" in: " << text.toStdString();
        }
    }
    // The walk found the dialog's words, so it did not pass for finding none.
    EXPECT_GT(seen.size(), 100);
    for (const char* expected :
         {"Settings", "Customisation", "What is active", "Replace instead of merging",
          "Reset to Built-in", "Revert to Kept", "When survey data comes in",
          "Linework control codes", "Katana customisation (*.customisation.json *.json)",
          "This program has no built-in customisation to reset to.",
          "The drawing this dialog worked on has closed: nothing here acts on anything any more.",
          "> CUSTOMISE RESET\nRefused: the command was refused."}) {
        EXPECT_TRUE(seen.contains(QString::fromLatin1(expected))) << expected;
    }
    // And the two tips that say what a press costs the kept file, each in
    // the state that shows it.
    for (const char* part : {"and Revert to Kept then has none to read",
                             "is set aside as C:/kept files/customisation.json.bak (CUSTOMISE "
                             "KEEP)"}) {
        EXPECT_FALSE(seen.filter(QString::fromLatin1(part)).isEmpty()) << part;
    }
}

// ---- lifetime -----------------------------------------------------------------------------------

TEST(SettingsDialog, TheDialogOutlivesItsDocumentAndThenRunsNothing)
{
    // Each control is pressed first after the Document has gone, in a dialog
    // of its own: the first press is what finds out.
    for (const char* name : {"settingsImport", "settingsExport", "settingsReset", "settingsKeep",
                             "settingsRevert", "settingsAutoCodes", "settingsAutoLinework"}) {
        QStringList ran;
        auto document = std::make_unique<Document>();
        SettingsContext context;
        context.document = document.get();
        context.run = [&ran](const QString& line) {
            ran << line;
            return carriedOut();
        };
        context.hasBuiltIn = true;
        context.keptFile = kKeptFile;
        SettingsDialog dialog(context);
        fill(dialog, "settingsImportPath", QStringLiteral("C:/data/site.json"));
        fill(dialog, "settingsExportPath", QStringLiteral("C:/data/out.json"));
        ASSERT_TRUE(enabled(dialog, name)) << name;

        // A change is notified, and the Document goes before it is delivered.
        document->setAutomation({.codesOnSurveyImport = false, .lineworkOnSurveyImport = false});
        document.reset();
        processEvents();

        press(dialog, name);
        EXPECT_TRUE(ran.isEmpty()) << name << " ran " << ran.join(" | ").toStdString();
        EXPECT_EQ(status(dialog),
                  QStringLiteral("The drawing this dialog worked on has closed: nothing here "
                                 "acts on anything any more."))
            << name;
        EXPECT_TRUE(saysARefusal(dialog)) << name;
        for (const char* each :
             {"settingsImport", "settingsExport", "settingsReset", "settingsKeep",
              "settingsRevert", "settingsAutoCodes", "settingsAutoLinework",
              "settingsImportBrowse", "settingsExportBrowse", "settingsImportPath",
              "settingsExportPath"}) {
            EXPECT_FALSE(enabled(dialog, each)) << name << ": " << each;
        }
        EXPECT_TRUE(enabled(dialog, "settingsClose")) << name;
        dialog.refresh();
        katana::qt::test::paint(dialog);
    }
}

// ---- the verb itself ----------------------------------------------------------------------------

TEST(SettingsDialog, TheLinesItBuildsAreReadByTheVerbItself)
{
    // No stand-in: the interpreter over the same Document, as the window's
    // executor runs a line. Given no host, as a test's Document is.
    SettingsFixture f;
    katana::cad::CommandInterpreter interpreter(f.document);
    f.answer = [&interpreter](const QString& line) {
        return runAsTheWindowDoes(interpreter, line);
    };
    SettingsDialog dialog(f.context);

    // Import: the committed fixture of 4 symbols.
    fill(dialog, "settingsImportPath",
         pathText(katana::qt::test::customisationFixtureFile("test_symbols")));
    press(dialog, "settingsImport");
    EXPECT_FALSE(saysARefusal(dialog)) << status(dialog).toStdString();
    processEvents();
    EXPECT_EQ(shown(dialog, "settingsActiveName"), QStringLiteral("test_symbols"));
    EXPECT_EQ(shown(dialog, "settingsActiveOrigin"), QStringLiteral("Loaded this session"));
    EXPECT_EQ(shown(dialog, "settingsActiveDefinitions"), QStringLiteral("4"));
    EXPECT_EQ(shown(dialog, "settingsActiveSymbols"), QStringLiteral("4"));
    EXPECT_EQ(f.document.styleLibrary().size(), 4u);

    // Export, to a folder and a file with a blank in each: one word to the
    // tokenizer, and the file reads back as what the session holds.
    QTemporaryDir scratch;
    ASSERT_TRUE(scratch.isValid());
    const QString written = scratch.filePath(QStringLiteral("site sets/all of it.json"));
    fill(dialog, "settingsExportPath", written);
    press(dialog, "settingsExport");
    EXPECT_FALSE(saysARefusal(dialog)) << status(dialog).toStdString();
    const auto read =
        katana::cad::readCustomisationFile(katana::core::pathFromUtf8(written.toStdString()));
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_TRUE(read->customisation == f.document.customisation());
    EXPECT_EQ(read->customisation.library.size(), 4u);

    // A switch: set on the Document by the verb, and the box follows it.
    press(dialog, "settingsAutoLinework");
    EXPECT_EQ(f.ran.back(), QStringLiteral("CUSTOMISE SET auto.linework=off"));
    EXPECT_FALSE(saysARefusal(dialog)) << status(dialog).toStdString();
    EXPECT_FALSE(f.document.customisationState().automation.lineworkOnSurveyImport);
    EXPECT_FALSE(checked(dialog, "settingsAutoLinework"));
    processEvents();
    EXPECT_EQ(shown(dialog, "settingsActiveOrigin"), QStringLiteral("Edited this session"));
    // And ticked again: the verb reads "on" as it read "off".
    press(dialog, "settingsAutoLinework");
    EXPECT_EQ(f.ran.back(), QStringLiteral("CUSTOMISE SET auto.linework=on"));
    EXPECT_FALSE(saysARefusal(dialog)) << status(dialog).toStdString();
    EXPECT_TRUE(f.document.customisationState().automation.lineworkOnSurveyImport);
    EXPECT_TRUE(checked(dialog, "settingsAutoLinework"));

    // A path in its quotes, as a file manager copies one: the verb is given
    // the path alone and reads the file. The fixture of 3 linestyles, merged
    // into the 4 symbols.
    fill(dialog, "settingsImportPath",
         QLatin1Char('"') +
             QDir::toNativeSeparators(
                 pathText(katana::qt::test::customisationFixtureFile("test_linestyles"))) +
             QLatin1Char('"'));
    press(dialog, "settingsImport");
    EXPECT_FALSE(saysARefusal(dialog)) << status(dialog).toStdString();
    EXPECT_EQ(f.document.styleLibrary().size(), 7u);
    EXPECT_TRUE(f.document.styleLibrary().contains("TEST Gate"));

    // A file called as a word of the family is read as a FILE: the verb looks
    // for it and does not find it, where `CUSTOMISE keep` is KEEP, refused
    // here for the kept file this session was not given.
    const katana::entity::Customisation before = f.document.customisation();
    fill(dialog, "settingsImportPath", QStringLiteral("keep"));
    press(dialog, "settingsImport");
    EXPECT_EQ(f.ran.back(), QStringLiteral("CUSTOMISE \"./keep\""));
    EXPECT_TRUE(saysARefusal(dialog));
    EXPECT_TRUE(status(dialog).contains(QStringLiteral("./keep"))) << status(dialog).toStdString();
    EXPECT_FALSE(status(dialog).contains(QStringLiteral("CUSTOMISE KEEP")))
        << status(dialog).toStdString();
    EXPECT_TRUE(f.document.customisation() == before);
}

TEST(SettingsDialog, WithTheVerbItselfAnEditOfAKeptSessionIsWrittenToTheKeptFile)
{
    // A host as a front end hands one over: kSite as the built-in, and a kept
    // file in a folder of this test's own that does not exist yet.
    QTemporaryDir scratch;
    ASSERT_TRUE(scratch.isValid());
    const QString keptFile = scratch.filePath(QStringLiteral("kept here/customisation.json"));
    katana::cad::CustomisationHost host;
    host.builtIn.customisation =
        std::make_shared<const katana::entity::Customisation>(customisationFromText(kSite));
    host.builtIn.digest = katana::entity::customisationDigest(kSite);
    host.keptFile = katana::core::pathFromUtf8(keptFile.toStdString());

    SettingsFixture f;
    f.context.keptFile = keptFile;
    katana::cad::CommandInterpreter interpreter(f.document);
    interpreter.setCustomisationHost(host);
    const katana::cad::CustomisationStart started =
        katana::cad::startCustomisation(f.document, host);
    ASSERT_EQ(started.installed, CustomisationOrigin::BuiltIn);
    ASSERT_TRUE(f.document.customisationState().kept);
    f.answer = [&interpreter](const QString& line) {
        return runAsTheWindowDoes(interpreter, line);
    };
    SettingsDialog dialog(f.context);
    EXPECT_EQ(shown(dialog, "settingsActiveOrigin"), QStringLiteral("Built in"));
    EXPECT_FALSE(enabled(dialog, "settingsKeep"));
    ASSERT_FALSE(QFileInfo::exists(keptFile));

    // A switch turned off in Settings: set, and kept behind it.
    press(dialog, "settingsAutoCodes");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE SET auto.codes=off"), kKeep}));
    EXPECT_FALSE(saysARefusal(dialog)) << status(dialog).toStdString();
    EXPECT_TRUE(f.document.customisationState().kept);
    ASSERT_TRUE(QFileInfo::exists(keptFile));
    const auto kept = katana::cad::readCustomisationFile(host.keptFile);
    ASSERT_TRUE(kept.ok()) << kept.error().describe();
    ASSERT_TRUE(kept->customisation.automation.has_value());
    EXPECT_FALSE(kept->customisation.automation->codesOnSurveyImport);
    EXPECT_TRUE(kept->customisation.automation->lineworkOnSurveyImport);
    processEvents();
    EXPECT_TRUE(shown(dialog, "settingsActiveKept").startsWith(QStringLiteral("Kept:")));
    EXPECT_FALSE(enabled(dialog, "settingsKeep"));

    // Reset to Built-in: with a kept file there the reset session is not the
    // kept one, so it is kept - and a session that IS the built-in is kept by
    // setting the kept file aside (docs/customisation.md, "The host").
    f.ran.clear();
    press(dialog, "settingsReset");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE RESET"), kKeep}));
    EXPECT_FALSE(saysARefusal(dialog)) << status(dialog).toStdString();
    EXPECT_TRUE(f.document.customisationState().kept);
    EXPECT_TRUE(f.document.customisationState().automation.codesOnSurveyImport);
    EXPECT_FALSE(QFileInfo::exists(keptFile));
    EXPECT_TRUE(QFileInfo::exists(keptFile + QStringLiteral(".bak")));
    processEvents();
    EXPECT_TRUE(checked(dialog, "settingsAutoCodes"));
    EXPECT_EQ(shown(dialog, "settingsActiveOrigin"), QStringLiteral("Built in"));
}

// ---- a reset of a kept session, and the way back from it ----------------------------------------
//
// docs/desktop.md said that Revert to Kept was the way back from a reset of a
// kept session. It is not: the reset is kept, and a session that is the
// built-in is kept by setting the kept file aside, so nothing is kept for
// Revert to read. These follow the file through each press.

TEST(SettingsDialog, AfterAResetOfAKeptSessionRevertHasNoneToReadAndTheFileSetAsideIsTheWayBack)
{
    KeptSession s;
    s.start();
    if (::testing::Test::HasFatalFailure()) {
        return;
    }
    SettingsFixture& f = s.f;
    SettingsDialog dialog(f.context);
    // kMine, read from the kept file at the start: survey codes not applied.
    EXPECT_EQ(shown(dialog, "settingsActiveName"), QStringLiteral("Mine"));
    EXPECT_EQ(shown(dialog, "settingsActiveOrigin"), QStringLiteral("Kept by you"));
    EXPECT_FALSE(checked(dialog, "settingsAutoCodes"));
    // What the press will cost is on the button before it is pressed.
    EXPECT_TRUE(tip(dialog, "settingsReset")
                    .contains(QStringLiteral("is set aside as ") + s.setAside +
                              QStringLiteral(", and Revert to Kept then has none to read")))
        << tip(dialog, "settingsReset").toStdString();

    // Reset to Built-in. The reset is kept, as every edit made here is, and
    // CUSTOMISE KEEP writes no copy of the built-in: it sets the kept file
    // aside (docs/customisation.md, "The host: RESET, KEEP and REVERT").
    press(dialog, "settingsReset");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE RESET"), kKeep}));
    EXPECT_FALSE(saysARefusal(dialog)) << status(dialog).toStdString();
    EXPECT_TRUE(
        status(dialog).contains(QStringLiteral("written=no reason=the-session-is-the-built-in")))
        << status(dialog).toStdString();
    EXPECT_FALSE(QFileInfo::exists(s.keptFile));
    ASSERT_TRUE(QFileInfo::exists(s.setAside));
    EXPECT_EQ(customisationInFile(s.setAside).name, "Mine");
    processEvents();
    EXPECT_EQ(shown(dialog, "settingsActiveName"), QStringLiteral("Site"));
    EXPECT_EQ(shown(dialog, "settingsActiveOrigin"), QStringLiteral("Built in"));
    EXPECT_TRUE(shown(dialog, "settingsActiveKept").startsWith(QStringLiteral("Kept:")));
    EXPECT_TRUE(checked(dialog, "settingsAutoCodes"));

    // Revert to Kept is NOT the way back: nothing is kept for it to read,
    // and the verb says so. The session stays the built-in.
    f.ran.clear();
    ASSERT_TRUE(enabled(dialog, "settingsRevert"));
    press(dialog, "settingsRevert");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE REVERT")}));
    EXPECT_TRUE(saysARefusal(dialog));
    EXPECT_TRUE(status(dialog).startsWith(QStringLiteral(
        "> CUSTOMISE REVERT\nRefused: NotFound: CUSTOMISE REVERT: no customisation is kept")))
        << status(dialog).toStdString();
    processEvents();
    EXPECT_EQ(shown(dialog, "settingsActiveName"), QStringLiteral("Site"));

    // The way back is the file that was set aside, imported with Replace: its
    // one linestyle in the place of the built-in's three definitions, its one
    // rule in the place of the three, its switches with them - and, the
    // session being the kept one, kept again, in a kept file written anew.
    f.ran.clear();
    fill(dialog, "settingsImportPath", s.setAside);
    child<QCheckBox>(dialog, "settingsImportReplace")->setChecked(true);
    press(dialog, "settingsImport");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE REPLACE \"") + s.setAside +
                                      QLatin1Char('"'),
                                  kKeep}));
    EXPECT_FALSE(saysARefusal(dialog)) << status(dialog).toStdString();
    processEvents();
    EXPECT_EQ(shown(dialog, "settingsActiveName"), QStringLiteral("Mine"));
    EXPECT_EQ(shown(dialog, "settingsActiveDefinitions"), QStringLiteral("1"));
    EXPECT_EQ(shown(dialog, "settingsActiveSymbols"), QStringLiteral("0"));
    EXPECT_EQ(shown(dialog, "settingsActiveRules"), QStringLiteral("1 over 1 code"));
    EXPECT_TRUE(shown(dialog, "settingsActiveKept").startsWith(QStringLiteral("Kept:")));
    EXPECT_FALSE(checked(dialog, "settingsAutoCodes"));
    EXPECT_TRUE(f.document.styleLibrary().contains("MINE Hedge"));
    EXPECT_FALSE(f.document.styleLibrary().contains("SITE Fence"));
    EXPECT_EQ(f.document.surveyMap().keys(), (std::vector<std::string>{"HG*"}));
    ASSERT_TRUE(QFileInfo::exists(s.keptFile));
    const katana::entity::Customisation kept = customisationInFile(s.keptFile);
    EXPECT_EQ(kept.name, "Mine");
    EXPECT_TRUE(kept.library.contains("MINE Hedge"));
    EXPECT_EQ(kept.library.size(), 1u);
    EXPECT_EQ(kept.map.size(), 1u);
    ASSERT_TRUE(kept.automation.has_value());
    EXPECT_FALSE(kept.automation->codesOnSurveyImport);
}

TEST(SettingsDialog, TheFileSetAsideGivenItsOwnNameBackIsReadByRevertAsItWas)
{
    KeptSession s;
    s.start();
    if (::testing::Test::HasFatalFailure()) {
        return;
    }
    SettingsFixture& f = s.f;
    SettingsDialog dialog(f.context);
    const katana::entity::Customisation before = f.document.customisation();
    press(dialog, "settingsReset");
    ASSERT_FALSE(QFileInfo::exists(s.keptFile));
    ASSERT_TRUE(QFileInfo::exists(s.setAside));
    processEvents();
    ASSERT_EQ(shown(dialog, "settingsActiveName"), QStringLiteral("Site"));

    // The other way back, taken outside the program, in a file manager: the
    // file's own name given back. Revert to Kept then has a file to read.
    ASSERT_TRUE(QFile::rename(s.setAside, s.keptFile));
    f.ran.clear();
    press(dialog, "settingsRevert");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE REVERT")}));
    EXPECT_FALSE(saysARefusal(dialog)) << status(dialog).toStdString();
    // The session the reset put away, every part of it - where an import
    // with Replace leaves what the file does not hold as the built-in has it.
    EXPECT_TRUE(f.document.customisation() == before);
    processEvents();
    EXPECT_EQ(shown(dialog, "settingsActiveName"), QStringLiteral("Mine"));
    EXPECT_EQ(shown(dialog, "settingsActiveOrigin"), QStringLiteral("Kept by you"));
    EXPECT_TRUE(shown(dialog, "settingsActiveKept").startsWith(QStringLiteral("Kept:")));
}

TEST(SettingsDialog, TheFileAResetSetAsideLastsOnlyUntilTheSecondEditKeptAfterIt)
{
    KeptSession s;
    s.start();
    if (::testing::Test::HasFatalFailure()) {
        return;
    }
    SettingsFixture& f = s.f;
    SettingsDialog dialog(f.context);
    press(dialog, "settingsReset");
    ASSERT_FALSE(saysARefusal(dialog)) << status(dialog).toStdString();
    ASSERT_FALSE(QFileInfo::exists(s.keptFile));
    ASSERT_EQ(customisationInFile(s.setAside).name, "Mine");
    processEvents();

    // The first edit after it is kept in a kept file written ANEW: none was
    // there to be set aside, so the file the reset set aside still is.
    f.ran.clear();
    press(dialog, "settingsAutoCodes");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE SET auto.codes=off"), kKeep}));
    EXPECT_FALSE(saysARefusal(dialog)) << status(dialog).toStdString();
    EXPECT_EQ(customisationInFile(s.keptFile).name, "Site");
    EXPECT_EQ(customisationInFile(s.setAside).name, "Mine");

    // The second finds that kept file and sets IT aside, in the place of the
    // one before. ONE earlier file is kept: what is set aside now is the
    // built-in with the first edit - survey codes off, coded points still
    // joined - and the customisation the reset put away is in neither file.
    f.ran.clear();
    press(dialog, "settingsAutoLinework");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE SET auto.linework=off"), kKeep}));
    EXPECT_FALSE(saysARefusal(dialog)) << status(dialog).toStdString();
    EXPECT_EQ(customisationInFile(s.keptFile).name, "Site");
    const katana::entity::Customisation setAside = customisationInFile(s.setAside);
    EXPECT_EQ(setAside.name, "Site");
    ASSERT_TRUE(setAside.automation.has_value());
    EXPECT_FALSE(setAside.automation->codesOnSurveyImport);
    EXPECT_TRUE(setAside.automation->lineworkOnSurveyImport);
}

TEST(SettingsDialog, KeepOfASessionThatIsTheBuiltInWritesNothingAndSetsTheKeptFileAside)
{
    KeptSession s;
    s.start();
    if (::testing::Test::HasFatalFailure()) {
        return;
    }
    SettingsFixture& f = s.f;
    SettingsDialog dialog(f.context);
    // A reset TYPED, as on the command line: not Settings', so nothing is
    // kept behind it. The kept file stands and the page says "Not kept".
    ASSERT_TRUE(s.interpreter.run("CUSTOMISE RESET").ok());
    processEvents();
    EXPECT_EQ(shown(dialog, "settingsActiveOrigin"), QStringLiteral("Built in"));
    EXPECT_TRUE(shown(dialog, "settingsActiveKept").startsWith(QStringLiteral("Not kept:")));
    ASSERT_TRUE(QFileInfo::exists(s.keptFile));
    EXPECT_FALSE(QFileInfo::exists(s.setAside));
    // Keep says what a press does to a session that is the built-in...
    ASSERT_TRUE(enabled(dialog, "settingsKeep"));
    EXPECT_TRUE(tip(dialog, "settingsKeep")
                    .contains(QStringLiteral("nothing is written, and the kept file, when there "
                                             "is one, is set aside as ") +
                              s.setAside))
        << tip(dialog, "settingsKeep").toStdString();
    // ...and the press does it: the verb's answer says nothing was written,
    // and the kept file is the one set aside.
    press(dialog, "settingsKeep");
    EXPECT_EQ(f.ran, (QStringList{kKeep}));
    EXPECT_FALSE(saysARefusal(dialog)) << status(dialog).toStdString();
    EXPECT_TRUE(
        status(dialog).contains(QStringLiteral("written=no reason=the-session-is-the-built-in")))
        << status(dialog).toStdString();
    EXPECT_FALSE(QFileInfo::exists(s.keptFile));
    EXPECT_EQ(customisationInFile(s.setAside).name, "Mine");
    processEvents();
    EXPECT_TRUE(shown(dialog, "settingsActiveKept").startsWith(QStringLiteral("Kept:")));
}

TEST(SettingsDialog, ARevertOfASessionThatWasResetAndNotKeptReadsTheKeptFileAgain)
{
    // The other half of the rule: where the reset was NOT kept - typed, or
    // made in Settings on a session that was not the kept one - the kept
    // file stands, and Revert to Kept goes back to it.
    KeptSession s;
    s.start();
    if (::testing::Test::HasFatalFailure()) {
        return;
    }
    SettingsFixture& f = s.f;
    SettingsDialog dialog(f.context);
    // An edit typed leaves the session not kept; a reset in Settings is then
    // followed by nothing.
    ASSERT_TRUE(s.interpreter.run("CUSTOMISE SET auto.linework=off").ok());
    processEvents();
    ASSERT_FALSE(f.document.customisationState().kept);
    press(dialog, "settingsReset");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE RESET")}));
    EXPECT_TRUE(QFileInfo::exists(s.keptFile));
    EXPECT_FALSE(QFileInfo::exists(s.setAside));
    processEvents();
    EXPECT_EQ(shown(dialog, "settingsActiveName"), QStringLiteral("Site"));

    f.ran.clear();
    press(dialog, "settingsRevert");
    EXPECT_EQ(f.ran, (QStringList{QStringLiteral("CUSTOMISE REVERT")}));
    EXPECT_FALSE(saysARefusal(dialog)) << status(dialog).toStdString();
    processEvents();
    EXPECT_EQ(shown(dialog, "settingsActiveName"), QStringLiteral("Mine"));
    EXPECT_EQ(shown(dialog, "settingsActiveOrigin"), QStringLiteral("Kept by you"));
    EXPECT_TRUE(shown(dialog, "settingsActiveKept").startsWith(QStringLiteral("Kept:")));
}
