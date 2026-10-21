#include "settings_dialog.hpp"

#include <cstddef>
#include <initializer_list>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include <QCheckBox>
#include <QComboBox>
#include <QDir>
#include <QFileDialog>
#include <QFormLayout>
#include <QGridLayout>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QRegularExpression>
#include <QStackedWidget>
#include <QStringList>
#include <QVBoxLayout>

#include "code_manager_support.hpp"
#include "command_word.hpp"
#include "document_watcher.hpp"
#include "katana/cad/customisation_host.hpp"
#include "katana/cad/customisation_state.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/style_catalogue.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/linework_codes.hpp"
#include "katana/entity/style_library.hpp"
#include "katana/update/credential.hpp"
#include "linework_labels.hpp"
#include "theme.hpp"

namespace katana::qt {

namespace {

using katana::cad::CustomisationOrigin;
using katana::cad::CustomisationState;

QString text(std::string_view value)
{
    return QString::fromUtf8(value.data(), static_cast<qsizetype>(value.size()));
}

QString lines(const std::vector<std::string>& each)
{
    QStringList out;
    for (const std::string& line : each) {
        out << text(line);
    }
    return out.join(QLatin1Char('\n'));
}

// Where the session's customisation came from, in plain words: a reply names
// an origin by one word (cad::toString), which is for a script to read.
QString originWords(CustomisationOrigin origin)
{
    switch (origin) {
    case CustomisationOrigin::None:
        return QStringLiteral("None");
    case CustomisationOrigin::BuiltIn:
        return QStringLiteral("Built in");
    case CustomisationOrigin::Kept:
        return QStringLiteral("Kept by you");
    case CustomisationOrigin::Loaded:
        return QStringLiteral("Loaded this session");
    case CustomisationOrigin::Edited:
        return QStringLiteral("Edited this session");
    }
    return {};
}

// What the START of this session found, for the row under "What is active":
// each thing that went wrong, a sentence as cad::startCustomisation wrote it -
// a kept file that did not read and what started in its place, a built-in
// that did not parse - and, last, that the kept customisation was made from
// another built-in than this program has. The Document keeps both
// (CustomisationState::startProblems, keptFromAnotherBuiltIn), because the
// window says them once, in a log that has scrolled away by the time a person
// opens Settings to ask why their own customisation is not the one loaded.
//
// They are of the start and nothing since changes them: a session reset, or
// loaded over, still says what its start found.
QStringList startProblemsOf(const CustomisationState& state)
{
    QStringList problems;
    for (const std::string& problem : state.startProblems) {
        problems << text(problem);
    }
    if (state.keptFromAnotherBuiltIn) {
        // The window's own start-up words (MainWindow::loadDefaultCustomisation),
        // with the button a person has here in the place of the line to type.
        problems << QStringLiteral("The kept customisation was made from another built-in "
                                   "customisation than this program has; Reset to Built-in gives "
                                   "this program's.");
    }
    return problems;
}

// The author's notice a person is shown: the customisation's own, then that of
// each customisation that went into the session and kept one of its own, under
// its name. A source whose notice IS the customisation's own - a file lists
// itself as its one source - is not said twice.
QString noticeOf(const CustomisationState& state)
{
    QStringList parts;
    if (!state.notice.empty()) {
        parts << lines(state.notice);
    }
    for (const katana::cad::CustomisationSource& source : state.sources) {
        if (source.notice.empty() || source.notice == state.notice) {
            continue;
        }
        parts << QStringLiteral("From %1:\n%2").arg(text(source.name), lines(source.notice));
    }
    return parts.join(QStringLiteral("\n\n"));
}

// The seven control codes as the session spells them, in the order of
// entity::lineworkCodeMembers, each under the word a person reads its control
// by - the ONE list of those words (linework_labels.hpp), which is the
// controls' one editor's too: the same control under another word here would
// be two names for one thing. An empty spelling is a control switched off.
QString lineworkWords(const katana::entity::LineworkCodes& codes)
{
    QStringList words;
    for (const katana::entity::LineworkCodeMember& member :
         katana::entity::lineworkCodeMembers()) {
        const std::string& spelling = codes.*member.spelling;
        words << lineworkControlLabel(member.name) + QStringLiteral(": ") +
                     (spelling.empty() ? QStringLiteral("(off)") : text(spelling));
    }
    return words.join(QStringLiteral(", "));
}

QString rulesWords(std::size_t rules, std::size_t codes)
{
    if (rules == 0) {
        return QStringLiteral("0");
    }
    return QStringLiteral("%1 over %2 %3")
        .arg(rules)
        .arg(codes)
        .arg(codes == 1 ? QStringLiteral("code") : QStringLiteral("codes"));
}

// "1 code", "2 codes": the count and the noun for it.
QString countWords(std::size_t count, const char* one, const char* many)
{
    return QStringLiteral("%1 %2").arg(count).arg(QLatin1String(count == 1 ? one : many));
}

// One built-in as the chooser lists it: its name, and that it is the default.
QString builtInItemText(const katana::cad::BuiltInChoice& choice)
{
    return text(choice.name) + (choice.isDefault ? QStringLiteral(" (default)") : QString());
}

// What a person is told of a built-in before choosing it: its own description,
// what it holds and where it was compiled in from - or why it cannot be used.
QString builtInTip(const katana::cad::BuiltInChoice& choice)
{
    if (!choice.problem.empty()) {
        return text(choice.problem);
    }
    QStringList parts;
    if (!choice.description.empty()) {
        parts << text(choice.description);
    }
    parts << QStringLiteral("%1, %2 over %3")
                 .arg(countWords(choice.definitions, "definition", "definitions"),
                      countWords(choice.rules, "survey code rule", "survey code rules"),
                      countWords(choice.codes, "code", "codes"));
    if (!choice.source.empty()) {
        parts << QStringLiteral("Compiled in from %1").arg(text(choice.source));
    }
    return parts.join(QLatin1Char('\n'));
}

// The "Built-in" row under "What is active": the built-in the session IS, or
// the one a kept or edited copy of it was made from.
QString activeBuiltInWords(const std::vector<katana::cad::BuiltInChoice>& choices,
                           const CustomisationState& state)
{
    for (const katana::cad::BuiltInChoice& choice : choices) {
        if (choice.isActive) {
            return text(choice.name) + (choice.isDefault ? QStringLiteral(" (default)") : QString());
        }
    }
    if (state.basedOn) {
        return QStringLiteral("Based on %1").arg(text(state.basedOn->name));
    }
    return QStringLiteral("None");
}

QString noKeptFile()
{
    return QStringLiteral("This session has no kept file; the environment variable %1 names one.")
        .arg(QLatin1String(katana::cad::kKeptCustomisationVariable));
}

// The Kept row. With a kept file it says whether the session is what that
// file gives the next start. With none there is nothing to be kept: the next
// start gives the built-in (or nothing, in a program without one), and the row
// says that instead of the contradiction "kept ... no kept file".
QString keptWords(bool kept, const QString& keptFile, bool hasBuiltIn)
{
    if (keptFile.isEmpty()) {
        return QStringLiteral("Nothing is kept: the next start gives %1. The environment variable "
                              "%2 names a file to keep one in.")
            .arg(hasBuiltIn ? QStringLiteral("the built-in customisation")
                            : QStringLiteral("no customisation"),
                 QLatin1String(katana::cad::kKeptCustomisationVariable));
    }
    return (kept ? QStringLiteral("Kept: the next start gives this customisation.")
                 : QStringLiteral("Not kept: the next start does not give this customisation.")) +
           QLatin1Char('\n') + QStringLiteral("Kept file: %1").arg(keptFile);
}

// Where CUSTOMISE KEEP leaves the kept file that was there: beside it, under
// this name, the one before it of that name gone (docs/customisation.md, "The
// host").
QString setAsideFile(const QString& keptFile) { return keptFile + QStringLiteral(".bak"); }

// What Reset to Built-in says of itself BEFORE it is pressed, since nothing is
// asked after (a question is a modal box). On a kept session the press costs
// the kept file: the reset is kept like every edit made here, and CUSTOMISE
// KEEP keeps a session that IS the built-in by setting the kept file aside,
// after which CUSTOMISE REVERT has none to read. A session that is not kept
// is not followed by KEEP, and its kept file stands.
QString resetTip(bool hasBuiltIn, bool kept, const QString& keptFile)
{
    if (!hasBuiltIn) {
        return QStringLiteral("This program has no built-in customisation to reset to.");
    }
    QString tip = QStringLiteral("Put the program's built-in customisation in the place of the "
                                 "session's (CUSTOMISE RESET)");
    if (kept && !keptFile.isEmpty()) {
        tip += QStringLiteral(".\nThis session is the kept one, so its reset is kept too: the "
                              "kept file, when there is one, is set aside as %1, and Revert to "
                              "Kept then has none to read.\nImport that file with Replace "
                              "instead of merging ticked to have it back.")
                   .arg(setAsideFile(keptFile));
    }
    return tip;
}

// What Use says of itself BEFORE it is pressed, for the same reason as Reset
// (no question is asked after). The press is followed by CUSTOMISE KEEP when
// the session is the kept one, and the file that was kept does not stay where
// it is: the chosen built-in is written in its place and what was there goes
// to `<kept file>.bak`, replacing the one before it; or, when the chosen one
// is the built-in of the run, which every start gives when nothing is kept,
// nothing is written and the kept file is set aside the same way. The press
// after that replaces the `.bak` again, so a kept customisation of the
// person's own is in neither file by the second press.
// A session that is not kept is followed by nothing, and its kept file stands.
QString useTip(bool kept, bool chosenIsTheRunsOwn, const QString& keptFile)
{
    QString tip = QStringLiteral("Put the chosen built-in customisation in the place of the "
                                 "session's (CUSTOMISE BUILTIN)");
    if (keptFile.isEmpty()) {
        return tip;
    }
    if (!kept) {
        return tip + QStringLiteral(".\nThis session is not the kept one, so the choice is not "
                                    "kept: the kept file stands.");
    }
    const QString aside = setAsideFile(keptFile);
    if (chosenIsTheRunsOwn) {
        return tip + QStringLiteral(".\nThis session is the kept one, so the choice is kept too: "
                                    "that is the built-in every start gives when nothing is kept, "
                                    "so the kept file is set aside as %1, replacing the file of "
                                    "that name, and Revert to Kept then has none to read.\nImport "
                                    "that file with Replace instead of merging ticked to have it "
                                    "back.")
                       .arg(aside);
    }
    return tip + QStringLiteral(".\nThis session is the kept one, so the choice is kept too: the "
                                "kept file is written with it, and what was there is set aside as "
                                "%1, replacing the file of that name.\nImport that file with "
                                "Replace instead of merging ticked to have it back.")
                   .arg(aside);
}

// What Keep says of itself: why it cannot be pressed, or what the press does.
// A session whose origin is still BuiltIn is the built-in and nothing more -
// any edit makes it Edited, any load Loaded - and CUSTOMISE KEEP writes no
// copy of that: it sets the kept file aside, so that the next start gives the
// built-in, and a later edition of it is not hidden by a copy of this one.
QString keepTip(const CustomisationState& state, const QString& keptFile)
{
    if (keptFile.isEmpty()) {
        return noKeptFile();
    }
    if (state.kept) {
        return QStringLiteral("The session is already kept: the next start gives it.");
    }
    if (state.origin == CustomisationOrigin::BuiltIn) {
        return QStringLiteral("The session is the built-in customisation, which every start "
                              "gives when nothing is kept: nothing is written, and the kept "
                              "file, when there is one, is set aside as %1 (CUSTOMISE KEEP)")
            .arg(setAsideFile(keptFile));
    }
    return QStringLiteral("Write the session's customisation to the kept file, which the next "
                          "start reads (CUSTOMISE KEEP)");
}

// A load's reply with the names the program's own built-in leaves undefined
// said once, as a count: a load reports every name the rules ask for that
// nothing defines, and the built-in's own few (`in_built_in=yes`) would be
// listed after every load of anything, as though that file had broken them.
QString withBuiltInUndefinedAsACount(const QString& reply)
{
    QStringList kept;
    for (const QString& line : reply.split(QLatin1Char('\n'))) {
        if (line.startsWith(QStringLiteral("undefined name=")) &&
            line.endsWith(QStringLiteral(" in_built_in=yes"))) {
            continue;
        }
        static const QRegularExpression header(
            QStringLiteral("^undefined names=(\\d+) in_built_in=(\\d+)$"));
        const QRegularExpressionMatch match = header.match(line);
        if (!match.hasMatch()) {
            kept << line;
            continue;
        }
        const qlonglong names = match.captured(1).toLongLong();
        const qlonglong own = match.captured(2).toLongLong();
        const auto counted = [](qlonglong count) {
            return QStringLiteral("%1 %2").arg(count).arg(count == 1 ? QStringLiteral("name")
                                                                     : QStringLiteral("names"));
        };
        kept << (own == names
                     ? QStringLiteral("%1 undefined (as in the built-in)").arg(counted(names))
                     : QStringLiteral("%1 undefined, %2 of them as in the built-in")
                           .arg(counted(names))
                           .arg(own));
    }
    return kept.join(QLatin1Char('\n'));
}

// A line and what it answered, as the status shows them: the line as the
// command log echoes it, then the verb's own words - its reply, or what it
// was refused for.
QString answered(const QString& line, const VerbOutcome& outcome)
{
    QString shown = QStringLiteral("> ") + line + QLatin1Char('\n');
    if (outcome.ok) {
        shown += outcome.reply.isEmpty() ? QStringLiteral("Done.")
                                         : withBuiltInUndefinedAsACount(outcome.reply);
    } else {
        shown += QStringLiteral("Refused: ") +
                 (outcome.error.isEmpty() ? QStringLiteral("the command was refused.")
                                          : outcome.error);
    }
    return shown;
}

QLabel* valueLabel(const char* objectName, QWidget* parent)
{
    auto* label = new QLabel(parent);
    label->setObjectName(QString::fromLatin1(objectName));
    // A name, a path and a notice are a file's own words, never markup.
    label->setTextFormat(Qt::PlainText);
    label->setWordWrap(true);
    label->setTextInteractionFlags(Qt::TextSelectableByMouse);
    return label;
}

QPushButton* button(const QString& label, const char* objectName, QWidget* parent)
{
    auto* made = new QPushButton(label, parent);
    made->setObjectName(QString::fromLatin1(objectName));
    return made;
}

// ---- the Updates page's words ------------------------------------------------------------------

// What a check sends, and what the server it asks sees whatever is sent: the
// one thing about a person's computer an update check gives away, said where
// the check is switched on and off.
QString updateSendsWords(const SettingsUpdates& updates)
{
    return QStringLiteral("A check asks %1 for the newest version. It sends only \"%2\" as its "
                          "user agent - nothing about you, your files or your drawings - and, "
                          "as with every website, %1 sees this computer's network address.")
        .arg(updates.server.isEmpty() ? QStringLiteral("the release server") : updates.server,
             updates.userAgent.isEmpty() ? QStringLiteral("Katana/<version> (<target>)")
                                         : updates.userAgent);
}

// Whether a token is stored and where - never the token. A store that cannot
// be read says so in its own words, which name the place and not the value
// (update/credential.hpp).
QString updateTokenWords(const katana::update::CredentialStore* store)
{
    if (store == nullptr) {
        return QStringLiteral("This session keeps no access token; the environment variable %1 "
                              "gives a run one.")
            .arg(text(katana::update::kTokenEnvironmentVariable));
    }
    const katana::core::Result<std::string> stored = store->read();
    if (stored.ok()) {
        return QStringLiteral("An access token is stored in the %1.").arg(text(store->where()));
    }
    if (stored.error().code == katana::core::ErrorCode::NotFound) {
        return QStringLiteral("No access token is stored.");
    }
    return text(stored.error().message);
}

QString updateSkippedWords(const QString& version)
{
    return version.isEmpty() ? QStringLiteral("No version is skipped.")
                             : QStringLiteral("Katana %1 is skipped: it is not offered.")
                                   .arg(version);
}

// Whether a version is set aside, without its number: the state keeps which
// BUILD was put back (a release's sequence), and no version to say.
QString updatePutBackWords(bool putBack)
{
    return putBack ? QStringLiteral("A version that was installed here did not start and was "
                                    "put back: it is not offered.")
                   : QStringLiteral("No version was put back.");
}

} // namespace

SettingsDialog::SettingsDialog(SettingsContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName(QStringLiteral("settingsDialog"));
    setWindowTitle(QStringLiteral("Settings"));
    setModal(false);

    auto* outer = new QVBoxLayout(this);
    auto* body = new QHBoxLayout;
    pages_ = new QListWidget(this);
    pages_->setObjectName(QStringLiteral("settingsPages"));
    pages_->setFixedWidth(150);
    stack_ = new QStackedWidget(this);
    stack_->setObjectName(QStringLiteral("settingsStack"));
    body->addWidget(pages_);
    body->addWidget(stack_, 1);
    outer->addLayout(body, 1);

    addPage(QStringLiteral("Customisation"), buildCustomisationPage());
    addPage(QStringLiteral("Updates"), buildUpdatesPage());
    // The list chooses the page; the stack has no signal back to this dialog.
    QObject::connect(pages_, &QListWidget::currentRowChanged, stack_,
                     &QStackedWidget::setCurrentIndex);
    // The Updates page shows per-user state that a typed line or the update
    // dialog may have changed since: read again each time it is turned to.
    QObject::connect(pages_, &QListWidget::currentRowChanged, this, [this](int) {
        if (stack_->currentWidget() != nullptr &&
            stack_->currentWidget()->objectName() == QLatin1String("settingsUpdates")) {
            showUpdates();
        }
    });
    pages_->setCurrentRow(0);

    // A box, not a label: a load answers with several records and a refusal
    // may list every reason, and a label that grew with them would push the
    // page about.
    status_ = new QPlainTextEdit(this);
    status_->setObjectName(QStringLiteral("settingsStatus"));
    status_->setReadOnly(true);
    status_->setTabChangesFocus(true);
    status_->setPlaceholderText(
        QStringLiteral("The line each button runs is shown here, with what it answered."));
    status_->setFixedHeight(status_->fontMetrics().lineSpacing() * 4 + 14);
    outer->addWidget(status_);

    auto* buttons = new QHBoxLayout;
    QPushButton* close = button(QStringLiteral("Close"), "settingsClose", this);
    buttons->addStretch(1);
    buttons->addWidget(close);
    outer->addLayout(buttons);
    QObject::connect(close, &QPushButton::clicked, this, [this] { this->close(); });

    // No button is a default: QDialog makes the first auto-default button its
    // default when shown, and Enter in a path field would press it - Browse,
    // or Import with half a path typed.
    for (QPushButton* each : findChildren<QPushButton*>()) {
        each->setAutoDefault(false);
        each->setDefault(false);
    }
    resize(820, 760);

    if (context_.document != nullptr) {
        // The Document also notifies for a selection, a save, a command: the
        // page is read again only when what it shows has moved.
        const auto changed = [this](const DocumentChanges&) {
            if (!alive()) {
                return;
            }
            const katana::cad::Document& document = *context_.document;
            if (document.customisationGeneration() != shownCustomisation_ ||
                document.libraryGeneration() != shownLibrary_ ||
                document.surveyMapGeneration() != shownSurveyMap_) {
                refresh();
            }
        };
        watcher_ = std::make_unique<DocumentWatcher>(*context_.document, changed);
    }
    refresh();
    // After refresh, which says in the status when there is no drawing: the
    // Updates page is of the program and is filled either way.
    showUpdates();
}

SettingsDialog::~SettingsDialog()
{
    // The children outlive this body, and one dying can still signal - a box
    // unticked, the list losing its row. Cut every connection into this
    // dialog first, so none reaches a lambda whose members no longer exist.
    watcher_.reset();
    for (QObject* child : findChildren<QObject*>()) {
        QObject::disconnect(child, nullptr, this, nullptr);
    }
}

bool SettingsDialog::alive() const
{
    return context_.document != nullptr && watcher_ != nullptr && watcher_->documentAlive();
}

void SettingsDialog::addPage(const QString& title, QWidget* page)
{
    pages_->addItem(title);
    stack_->addWidget(page);
}

// ---- layout ------------------------------------------------------------------------------------

QWidget* SettingsDialog::buildCustomisationPage()
{
    auto* page = new QWidget(this);
    page->setObjectName(QStringLiteral("settingsCustomisation"));
    auto* grid = new QGridLayout(page);
    grid->setContentsMargins(0, 0, 0, 0);

    // What is active.
    auto* active = new QGroupBox(QStringLiteral("What is active"), page);
    auto* activeLayout = new QVBoxLayout(active);
    auto* form = new QFormLayout;
    name_ = valueLabel("settingsActiveName", active);
    origin_ = valueLabel("settingsActiveOrigin", active);
    definitions_ = valueLabel("settingsActiveDefinitions", active);
    symbols_ = valueLabel("settingsActiveSymbols", active);
    rules_ = valueLabel("settingsActiveRules", active);
    activeBuiltIn_ = valueLabel("settingsActiveBuiltIn", active);
    kept_ = valueLabel("settingsActiveKept", active);
    form->addRow(QStringLiteral("Name:"), name_);
    form->addRow(QStringLiteral("Origin:"), origin_);
    form->addRow(QStringLiteral("Built-in:"), activeBuiltIn_);
    form->addRow(QStringLiteral("Definitions:"), definitions_);
    form->addRow(QStringLiteral("Symbols among them:"), symbols_);
    form->addRow(QStringLiteral("Survey code rules:"), rules_);
    form->addRow(QStringLiteral("Kept:"), kept_);
    activeLayout->addLayout(form);
    // The next three are hidden until refresh() finds something for them to
    // show: a page opened on no drawing has neither problems nor a notice.
    problems_ = valueLabel("settingsActiveProblems", active);
    problems_->setStyleSheet(QStringLiteral("color: %1;").arg(theme::error().name()));
    problems_->hide();
    activeLayout->addWidget(problems_);
    showNotice_ = new QCheckBox(QStringLiteral("Show Notice"), active);
    showNotice_->setObjectName(QStringLiteral("settingsShowNotice"));
    showNotice_->setToolTip(QStringLiteral(
        "What the author of the customisation says of it and of its use, as its file carries "
        "it"));
    showNotice_->hide();
    activeLayout->addWidget(showNotice_);
    notice_ = new QPlainTextEdit(active);
    notice_->setObjectName(QStringLiteral("settingsNotice"));
    notice_->setReadOnly(true);
    notice_->setTabChangesFocus(true);
    notice_->setMaximumHeight(140);
    notice_->hide();
    activeLayout->addWidget(notice_);
    grid->addWidget(active, 0, 0, 1, 2);

    // Import.
    auto* importBox = new QGroupBox(QStringLiteral("Import"), page);
    auto* importLayout = new QGridLayout(importBox);
    importPath_ = new QLineEdit(importBox);
    importPath_->setObjectName(QStringLiteral("settingsImportPath"));
    importPath_->setPlaceholderText(QStringLiteral("a Katana customisation file to load"));
    importBrowse_ = button(QStringLiteral("Browse..."), "settingsImportBrowse", importBox);
    importReplace_ = new QCheckBox(QStringLiteral("Replace instead of merging"), importBox);
    importReplace_->setObjectName(QStringLiteral("settingsImportReplace"));
    importReplace_->setToolTip(QStringLiteral(
        "Put the file's definitions in the place of the whole library and its rules in the "
        "place of all the survey codes, each kind it brings, where a merge replaces a "
        "definition by its name and a code's rules by their section (CUSTOMISE REPLACE)"));
    import_ = button(QStringLiteral("Import"), "settingsImport", importBox);
    import_->setToolTip(QStringLiteral(
        "Load the file into the session's customisation (CUSTOMISE and the file)"));
    importLayout->addWidget(importPath_, 0, 0);
    importLayout->addWidget(importBrowse_, 0, 1);
    importLayout->addWidget(importReplace_, 1, 0);
    importLayout->addWidget(import_, 1, 1);
    grid->addWidget(importBox, 1, 0, 1, 2);

    // Export.
    auto* exportBox = new QGroupBox(QStringLiteral("Export"), page);
    auto* exportLayout = new QHBoxLayout(exportBox);
    exportPath_ = new QLineEdit(exportBox);
    exportPath_->setObjectName(QStringLiteral("settingsExportPath"));
    exportPath_->setPlaceholderText(QStringLiteral("the file to write the customisation to"));
    exportBrowse_ = button(QStringLiteral("Browse..."), "settingsExportBrowse", exportBox);
    export_ = button(QStringLiteral("Export"), "settingsExport", exportBox);
    export_->setToolTip(QStringLiteral(
        "Write the session's whole customisation to the file; a file that is there is written "
        "over, and the answer says so (CUSTOMISE EXPORT and the file)"));
    exportLayout->addWidget(exportPath_, 1);
    exportLayout->addWidget(exportBrowse_);
    exportLayout->addWidget(export_);
    grid->addWidget(exportBox, 2, 0, 1, 2);

    // The built-in and the kept file.
    auto* startBox = new QGroupBox(QStringLiteral("Built-in and kept"), page);
    auto* startLayout = new QVBoxLayout(startBox);
    // The chooser of the built-ins: its row is hidden until refresh() finds
    // two to choose between.
    builtInRow_ = new QWidget(startBox);
    builtInRow_->setObjectName(QStringLiteral("settingsBuiltInRow"));
    auto* builtInLayout = new QHBoxLayout(builtInRow_);
    builtInLayout->setContentsMargins(0, 0, 0, 0);
    auto* builtInLabel = new QLabel(QStringLiteral("Built-in library:"), builtInRow_);
    builtInChoice_ = new QComboBox(builtInRow_);
    builtInChoice_->setObjectName(QStringLiteral("settingsBuiltIn"));
    builtInChoice_->setSizeAdjustPolicy(QComboBox::AdjustToContents);
    builtInLabel->setBuddy(builtInChoice_);
    builtInUse_ = button(QStringLiteral("Use"), "settingsBuiltInUse", builtInRow_);
    builtInLayout->addWidget(builtInLabel);
    builtInLayout->addWidget(builtInChoice_, 1);
    builtInLayout->addWidget(builtInUse_);
    builtInRow_->hide();
    startLayout->addWidget(builtInRow_);
    auto* startButtons = new QHBoxLayout;
    reset_ = button(QStringLiteral("Reset to Built-in"), "settingsReset", startBox);
    keep_ = button(QStringLiteral("Keep"), "settingsKeep", startBox);
    revert_ = button(QStringLiteral("Revert to Kept"), "settingsRevert", startBox);
    startButtons->addWidget(reset_);
    startButtons->addStretch(1);
    startButtons->addWidget(keep_);
    startButtons->addWidget(revert_);
    startLayout->addLayout(startButtons);
    grid->addWidget(startBox, 3, 0, 1, 2);

    // Automation.
    auto* autoBox = new QGroupBox(QStringLiteral("When survey data comes in"), page);
    auto* autoLayout = new QVBoxLayout(autoBox);
    autoCodes_ = new QCheckBox(QStringLiteral("Apply survey codes"), autoBox);
    autoCodes_->setObjectName(QStringLiteral("settingsAutoCodes"));
    autoCodes_->setToolTip(QStringLiteral(
        "Code the points a survey import creates, by the survey code rules (CUSTOMISE SET "
        "auto.codes)"));
    autoLinework_ = new QCheckBox(QStringLiteral("Join coded points into lines"), autoBox);
    autoLinework_->setObjectName(QStringLiteral("settingsAutoLinework"));
    autoLinework_->setToolTip(QStringLiteral(
        "Then join the coded points into lines, by the linework control codes (CUSTOMISE SET "
        "auto.linework)"));
    autoLayout->addWidget(autoCodes_);
    autoLayout->addWidget(autoLinework_);
    grid->addWidget(autoBox, 4, 0);

    // The editors.
    auto* editorBox = new QGroupBox(QStringLiteral("Editors"), page);
    auto* editorLayout = new QVBoxLayout(editorBox);
    openCodes_ = button(QStringLiteral("Survey Codes..."), "settingsOpenCodes", editorBox);
    openCodes_->setToolTip(QStringLiteral("Open the editor of the survey code rules"));
    openSymbols_ = button(QStringLiteral("Symbol Library..."), "settingsOpenSymbols", editorBox);
    openSymbols_->setToolTip(QStringLiteral("Open the browser and editor of the symbols"));
    editorLayout->addWidget(openCodes_);
    editorLayout->addWidget(openSymbols_);
    grid->addWidget(editorBox, 4, 1);

    // The linework control codes: shown here, edited in ONE place.
    auto* lineworkBox = new QGroupBox(QStringLiteral("Linework control codes"), page);
    auto* lineworkLayout = new QHBoxLayout(lineworkBox);
    linework_ = valueLabel("settingsLinework", lineworkBox);
    linework_->setToolTip(QStringLiteral(
        "What is typed after a point's code to start, end or close a line, begin and end a "
        "curve, join to another point or make a rectangle"));
    editLinework_ =
        button(QStringLiteral("Edit Linework Codes..."), "settingsEditLinework", lineworkBox);
    editLinework_->setToolTip(QStringLiteral(
        "Open the editor of the survey codes at its Linework tab, the one place the control "
        "codes are changed"));
    lineworkLayout->addWidget(linework_, 1);
    lineworkLayout->addWidget(editLinework_);
    grid->addWidget(lineworkBox, 5, 0, 1, 2);
    grid->setRowStretch(6, 1);

    // ---- wiring: lambdas, no moc.
    QObject::connect(showNotice_, &QCheckBox::toggled, this, [this](bool shown) {
        notice_->setVisible(shown && !notice_->toPlainText().isEmpty());
    });
    QObject::connect(importBrowse_, &QPushButton::clicked, this, [this] { browseImport(); });
    QObject::connect(import_, &QPushButton::clicked, this, [this] { importFile(); });
    QObject::connect(exportBrowse_, &QPushButton::clicked, this, [this] { browseExport(); });
    QObject::connect(export_, &QPushButton::clicked, this, [this] { exportFile(); });
    QObject::connect(reset_, &QPushButton::clicked, this, [this] {
        if (mayReplaceRules(QStringLiteral("Reset to Built-in"))) {
            run(QStringLiteral("CUSTOMISE RESET"), Then::KeepWhatWasKept);
        }
    });
    QObject::connect(builtInUse_, &QPushButton::clicked, this, [this] { useBuiltIn(); });
    // The button follows the chooser: it is for any built-in but the one the
    // session already is.
    QObject::connect(builtInChoice_, &QComboBox::currentIndexChanged, this, [this](int) {
        if (!loading_) {
            showBuiltInUse();
        }
    });
    QObject::connect(keep_, &QPushButton::clicked, this,
                     [this] { run(QStringLiteral("CUSTOMISE KEEP"), Then::Nothing); });
    // Never followed by KEEP: it leaves the session as the kept file has it.
    QObject::connect(revert_, &QPushButton::clicked, this, [this] {
        if (mayReplaceRules(QStringLiteral("Revert to Kept"))) {
            run(QStringLiteral("CUSTOMISE REVERT"), Then::Nothing);
        }
    });
    // toggled, not clicked: a box set by the headless driver's --fill has been
    // toggled as much as one a person clicked. What tells a person's toggle
    // from this dialog's own, reading the Document, is loading_.
    QObject::connect(autoCodes_, &QCheckBox::toggled, this,
                     [this](bool on) { automationToggled("auto.codes", on); });
    QObject::connect(autoLinework_, &QCheckBox::toggled, this,
                     [this](bool on) { automationToggled("auto.linework", on); });
    QObject::connect(editLinework_, &QPushButton::clicked, this, [this] {
        if (context_.openLinework) {
            context_.openLinework();
        }
    });
    QObject::connect(openCodes_, &QPushButton::clicked, this, [this] {
        if (context_.openCodeManager) {
            context_.openCodeManager();
        }
    });
    QObject::connect(openSymbols_, &QPushButton::clicked, this, [this] {
        if (context_.openSymbolLibrary) {
            context_.openSymbolLibrary();
        }
    });
    return page;
}

QWidget* SettingsDialog::buildUpdatesPage()
{
    auto* page = new QWidget(this);
    page->setObjectName(QStringLiteral("settingsUpdates"));
    auto* column = new QVBoxLayout(page);
    column->setContentsMargins(0, 0, 0, 0);

    // Hidden until showUpdates() finds something to say.
    updateNote_ = valueLabel("settingsUpdateNote", page);
    updateNote_->hide();
    column->addWidget(updateNote_);

    // Checking.
    auto* checkBox = new QGroupBox(QStringLiteral("Checking"), page);
    auto* checkLayout = new QVBoxLayout(checkBox);
    updateAuto_ = new QCheckBox(QStringLiteral("Check for updates automatically"), checkBox);
    updateAuto_->setObjectName(QStringLiteral("settingsUpdateAuto"));
    updateAuto_->setToolTip(QStringLiteral(
        "Ask once a day, when the window is open, whether a newer Katana exists, and show it in "
        "the status bar when one does; nothing is downloaded or installed unasked (UPDATE SET "
        "AUTO ON or OFF). Help > Check for Updates asks at any time"));
    updateSends_ = valueLabel("settingsUpdateSends", checkBox);
    checkLayout->addWidget(updateAuto_);
    checkLayout->addWidget(updateSends_);
    column->addWidget(checkBox);

    // The access token.
    auto* tokenBox = new QGroupBox(QStringLiteral("Access token"), page);
    auto* tokenLayout = new QGridLayout(tokenBox);
    auto* tokenWhy = new QLabel(
        QStringLiteral("Needed only when the releases are private: a read-only token, which is "
                       "sent to the release server and to nothing else. Leave it empty "
                       "otherwise."),
        tokenBox);
    tokenWhy->setWordWrap(true);
    updateToken_ = new QLineEdit(tokenBox);
    updateToken_->setObjectName(QStringLiteral("settingsUpdateToken"));
    // Dots: a token typed with someone looking on is still a secret.
    updateToken_->setEchoMode(QLineEdit::Password);
    updateToken_->setPlaceholderText(QStringLiteral("paste an access token to store it"));
    updateTokenSave_ = button(QStringLiteral("Save"), "settingsUpdateTokenSave", tokenBox);
    updateTokenClear_ = button(QStringLiteral("Clear"), "settingsUpdateTokenClear", tokenBox);
    updateTokenClear_->setToolTip(
        QStringLiteral("Remove the stored access token (UPDATE TOKEN CLEAR)"));
    updateTokenState_ = valueLabel("settingsUpdateTokenState", tokenBox);
    tokenLayout->addWidget(tokenWhy, 0, 0, 1, 3);
    tokenLayout->addWidget(updateToken_, 1, 0);
    tokenLayout->addWidget(updateTokenSave_, 1, 1);
    tokenLayout->addWidget(updateTokenClear_, 1, 2);
    tokenLayout->addWidget(updateTokenState_, 2, 0, 1, 3);
    tokenLayout->setColumnStretch(0, 1);
    column->addWidget(tokenBox);

    // The skipped version.
    auto* skipBox = new QGroupBox(QStringLiteral("Skipped version"), page);
    auto* skipLayout = new QHBoxLayout(skipBox);
    updateSkipped_ = valueLabel("settingsUpdateSkipped", skipBox);
    updateUnskip_ =
        button(QStringLiteral("Offer the skipped version again"), "settingsUpdateUnskip", skipBox);
    skipLayout->addWidget(updateSkipped_, 1);
    skipLayout->addWidget(updateUnskip_);
    column->addWidget(skipBox);

    // The version that was put back: under the skipped one, and built as it
    // is - what the state has, and the button that runs the line.
    auto* blockedBox = new QGroupBox(QStringLiteral("Version that was put back"), page);
    auto* blockedLayout = new QHBoxLayout(blockedBox);
    updateBlocked_ = valueLabel("settingsUpdateBlocked", blockedBox);
    updateUnblock_ = button(QStringLiteral("Offer the version that was put back again"),
                            "settingsUpdateUnblock", blockedBox);
    blockedLayout->addWidget(updateBlocked_, 1);
    blockedLayout->addWidget(updateUnblock_);
    column->addWidget(blockedBox);
    column->addStretch(1);

    // ---- wiring: lambdas, no moc. toggled, not clicked, for the reason the
    // automation boxes give.
    QObject::connect(updateAuto_, &QCheckBox::toggled, this, [this](bool on) {
        if (!loading_) {
            runUpdatesLine(on ? QStringLiteral("UPDATE SET AUTO ON")
                              : QStringLiteral("UPDATE SET AUTO OFF"));
        }
    });
    QObject::connect(updateTokenSave_, &QPushButton::clicked, this, [this] { saveUpdateToken(); });
    QObject::connect(updateTokenClear_, &QPushButton::clicked, this, [this] {
        // What was typed and not saved goes too: Clear leaves no token anywhere.
        updateToken_->clear();
        runUpdatesLine(QStringLiteral("UPDATE TOKEN CLEAR"));
    });
    QObject::connect(updateUnskip_, &QPushButton::clicked, this,
                     [this] { runUpdatesLine(QStringLiteral("UPDATE SKIP NONE")); });
    QObject::connect(updateUnblock_, &QPushButton::clicked, this,
                     [this] { runUpdatesLine(QStringLiteral("UPDATE BLOCKED CLEAR")); });
    return page;
}

// ---- what the Document says --------------------------------------------------------------------

void SettingsDialog::refresh()
{
    if (!alive()) {
        showClosed();
        return;
    }
    const katana::cad::Document& document = *context_.document;
    const CustomisationState& state = document.customisationState();
    shownCustomisation_ = document.customisationGeneration();
    shownLibrary_ = document.libraryGeneration();
    shownSurveyMap_ = document.surveyMapGeneration();

    // A session edited before anything was installed has rules or definitions
    // and no name; one nothing was ever put into has neither.
    QString name = text(state.name);
    if (name.isEmpty()) {
        name = state.origin == CustomisationOrigin::None
                   ? QStringLiteral("No customisation is loaded")
                   : QStringLiteral("Not named");
    }
    name_->setText(name);
    origin_->setText(originWords(state.origin));
    definitions_->setText(QString::number(document.styleLibrary().size()));
    symbols_->setText(QString::number(katana::cad::symbolDefinitionCount(document)));
    rules_->setText(rulesWords(document.surveyMap().size(), document.surveyMap().keys().size()));
    kept_->setText(keptWords(state.kept, context_.keptFile, context_.hasBuiltIn));

    const QStringList problems = startProblemsOf(state);
    problems_->setText(problems.join(QLatin1Char('\n')));
    problems_->setVisible(!problems.isEmpty());

    // The toggle is there only while there is a notice to show, and what it
    // shows follows the Document like the rest.
    const QString notice = noticeOf(state);
    if (notice_->toPlainText() != notice) {
        notice_->setPlainText(notice);
    }
    showNotice_->setVisible(!notice.isEmpty());
    if (notice.isEmpty()) {
        showNotice_->setChecked(false);
    }
    notice_->setVisible(showNotice_->isChecked() && !notice.isEmpty());

    linework_->setText(lineworkWords(state.linework));
    showAutomation();
    showBuiltIns();

    for (QWidget* each :
         std::initializer_list<QWidget*>{importPath_, importBrowse_, importReplace_, import_,
                                         exportPath_, exportBrowse_, export_, autoCodes_,
                                         autoLinework_}) {
        each->setEnabled(true);
    }
    // Disabled with the reason, where the line could only be refused for what
    // the session's host does not have - or, for Keep, has nothing to do. And
    // where a press would set the kept file aside, the tip says so first.
    const bool hasKeptFile = !context_.keptFile.isEmpty();
    reset_->setEnabled(context_.hasBuiltIn);
    reset_->setToolTip(resetTip(context_.hasBuiltIn, state.kept, context_.keptFile));
    keep_->setEnabled(hasKeptFile && !state.kept);
    keep_->setToolTip(keepTip(state, context_.keptFile));
    revert_->setEnabled(hasKeptFile);
    revert_->setToolTip(hasKeptFile
                            ? QStringLiteral("Read the kept file again, in the place of the "
                                             "session's customisation (CUSTOMISE REVERT)")
                            : noKeptFile());
    // A button whose editor nobody handed over has nothing to open.
    editLinework_->setEnabled(static_cast<bool>(context_.openLinework));
    openCodes_->setEnabled(static_cast<bool>(context_.openCodeManager));
    openSymbols_->setEnabled(static_cast<bool>(context_.openSymbolLibrary));
}

void SettingsDialog::showBuiltIns()
{
    if (!alive()) {
        return;
    }
    const katana::cad::Document& document = *context_.document;
    const std::vector<katana::cad::BuiltInEntry> entries =
        context_.builtIns ? context_.builtIns() : katana::cad::builtIns();
    const std::vector<katana::cad::BuiltInChoice> choices =
        katana::cad::builtInChoices(document, entries);
    activeBuiltIn_->setText(activeBuiltInWords(choices, document.customisationState()));

    // The chooser is rebuilt from the built-ins and keeps the person's choice
    // by name; with none made yet it stands on the built-in the session is,
    // else on the default, else on the first.
    const QString before = builtInChoice_->currentData().toString();
    const bool wasLoading = loading_;
    loading_ = true;
    builtInChoice_->clear();
    int standOn = -1;
    int active = -1;
    int fallback = -1;
    for (std::size_t at = 0; at < choices.size(); ++at) {
        const katana::cad::BuiltInChoice& choice = choices[at];
        builtInChoice_->addItem(builtInItemText(choice), text(choice.name));
        builtInChoice_->setItemData(static_cast<int>(at), builtInTip(choice), Qt::ToolTipRole);
        if (!before.isEmpty() && text(choice.name) == before) {
            standOn = static_cast<int>(at);
        }
        if (choice.isActive) {
            active = static_cast<int>(at);
        }
        if (choice.isDefault) {
            fallback = static_cast<int>(at);
        }
    }
    if (standOn < 0) {
        standOn = active >= 0 ? active : fallback;
    }
    builtInChoice_->setCurrentIndex(standOn >= 0 ? standOn : (choices.empty() ? -1 : 0));
    loading_ = wasLoading;

    builtInChoices_ = choices;

    // Worth a row only when there is something to choose between.
    builtInRow_->setVisible(choices.size() >= 2);
    showBuiltInUse();
}

void SettingsDialog::showBuiltInUse()
{
    const int chosen = builtInChoice_->currentIndex();
    if (chosen < 0 || static_cast<std::size_t>(chosen) >= builtInChoices_.size()) {
        builtInUse_->setEnabled(false);
        builtInUse_->setToolTip(QStringLiteral("This program has no built-in customisation to use."));
        return;
    }
    const katana::cad::BuiltInChoice& choice = builtInChoices_[static_cast<std::size_t>(chosen)];
    builtInUse_->setEnabled(!choice.isActive && choice.problem.empty() &&
                            static_cast<bool>(context_.run));
    if (choice.isActive) {
        builtInUse_->setToolTip(QStringLiteral("The session is this built-in customisation already."));
    } else if (!choice.problem.empty()) {
        builtInUse_->setToolTip(text(choice.problem));
    } else {
        // The tip follows the Document, as Reset's does: a dialog whose
        // Document has gone says nothing of what a press would keep.
        const bool kept = alive() && context_.document->customisationState().kept;
        builtInUse_->setToolTip(useTip(kept, choice.isDefault, context_.keptFile));
    }
}

void SettingsDialog::useBuiltIn()
{
    if (!mayReplaceRules(QStringLiteral("Use Built-in"))) {
        return;
    }
    const QString name = builtInChoice_->currentData().toString();
    if (name.isEmpty()) {
        refuse(QStringLiteral("Use Built-in: no built-in customisation is chosen."));
        return;
    }
    const auto word = commandWord(name, QStringLiteral("Use Built-in"));
    if (!word) {
        refuse(text(word.error().message));
        return;
    }
    run(QStringLiteral("CUSTOMISE BUILTIN ") + *word, Then::KeepWhatWasKept);
}

void SettingsDialog::showAutomation()
{
    if (!alive()) {
        return;
    }
    const katana::entity::CustomisationAutomation& automation =
        context_.document->customisationState().automation;
    const bool wasLoading = loading_;
    loading_ = true;
    autoCodes_->setChecked(automation.codesOnSurveyImport);
    autoLinework_->setChecked(automation.lineworkOnSurveyImport);
    loading_ = wasLoading;
}

void SettingsDialog::showClosed()
{
    // What the page last showed stays to be read - the notice too - and
    // nothing that would act can be pressed.
    for (QWidget* each :
         std::initializer_list<QWidget*>{importPath_, importBrowse_, importReplace_, import_,
                                         exportPath_, exportBrowse_, export_, builtInChoice_,
                                         builtInUse_, reset_, keep_, revert_, autoCodes_,
                                         autoLinework_, editLinework_, openCodes_,
                                         openSymbols_}) {
        each->setEnabled(false);
    }
    say(context_.document == nullptr
            ? QStringLiteral("Settings was opened on no drawing: nothing here acts on anything.")
            : QStringLiteral("The drawing this dialog worked on has closed: nothing here acts "
                             "on anything any more."),
        true);
}

// ---- saying and running ------------------------------------------------------------------------

void SettingsDialog::say(const QString& message, bool isError)
{
    status_->setPlainText(message);
    status_->setStyleSheet(isError ? QStringLiteral("color: %1;").arg(theme::error().name())
                                   : QString());
}

void SettingsDialog::refuse(const QString& message)
{
    say(message, true);
    if (context_.log) {
        context_.log(message, true);
    }
}

void SettingsDialog::run(const QString& line, Then then)
{
    if (!alive()) {
        showClosed();
        return;
    }
    if (!context_.run) {
        refuse(QStringLiteral("Nothing here can run %1: Settings was given no command line.")
                   .arg(line));
        return;
    }
    const bool wasKept = context_.document->customisationState().kept;
    const VerbOutcome outcome = context_.run(line);
    if (!alive()) {
        showClosed();
        return;
    }
    QStringList shown{answered(line, outcome)};
    bool refused = !outcome.ok;
    // What is edited in Settings is kept: a session that was the kept one
    // before this line, and is not after it, is written to the kept file by
    // the verb that does that. Asked of the Document, not of the line: one
    // that was refused, or that changed nothing, left the session kept.
    if (outcome.ok && then == Then::KeepWhatWasKept && wasKept && !context_.keptFile.isEmpty() &&
        !context_.document->customisationState().kept) {
        const QString keep = QStringLiteral("CUSTOMISE KEEP");
        const VerbOutcome kept = context_.run(keep);
        if (!alive()) {
            showClosed();
            return;
        }
        shown << answered(keep, kept);
        refused = !kept.ok;
    }
    say(shown.join(QLatin1Char('\n')), refused);
}

bool SettingsDialog::mayReplaceRules(const QString& what)
{
    if (!alive()) {
        showClosed();
        return false;
    }
    // In the words the window refuses to close over the same edits with
    // (CustomisationWorkbench::confirmClose). The manager keeps its buffer
    // when the drawing's rules change under it, and its Apply then installs
    // that buffer whole.
    if (context_.codeManagerDirty && context_.codeManagerDirty()) {
        refuse(QStringLiteral("%1 was not run: the Survey Code Manager has rule edits that are "
                              "not on the drawing, and its Apply would then put the rules it "
                              "holds back over what %1 brought. Apply or Revert them first.")
                   .arg(what));
        return false;
    }
    return true;
}

void SettingsDialog::importFile()
{
    if (!mayReplaceRules(QStringLiteral("Import"))) {
        return;
    }
    const auto file = typedCustomisationFileWord(importPath_->text());
    if (!file) {
        refuse(QStringLiteral("Import: ") + text(file.error().message));
        return;
    }
    run((importReplace_->isChecked() ? QStringLiteral("CUSTOMISE REPLACE ")
                                     : QStringLiteral("CUSTOMISE ")) +
            *file,
        Then::KeepWhatWasKept);
}

void SettingsDialog::exportFile()
{
    const auto file = typedCustomisationFileWord(exportPath_->text());
    if (!file) {
        refuse(QStringLiteral("Export: ") + text(file.error().message));
        return;
    }
    // Never followed by KEEP: an export changes nothing of the session.
    run(QStringLiteral("CUSTOMISE EXPORT ") + *file, Then::Nothing);
}

void SettingsDialog::browseImport()
{
    if (context_.headless && context_.headless()) {
        refuse(QStringLiteral("A headless session opens no file dialog: type CUSTOMISE "
                              "[REPLACE] <file> instead, or fill settingsImportPath."));
        return;
    }
    // Opened where the field already points, read as Import reads it.
    const QString chosen = QFileDialog::getOpenFileName(
        this, QStringLiteral("Import a Katana Customisation"), typedFilePath(importPath_->text()),
        customisationFileFilter() + QStringLiteral(";;All files (*)"));
    if (!chosen.isEmpty()) {
        importPath_->setText(QDir::fromNativeSeparators(chosen));
    }
}

void SettingsDialog::browseExport()
{
    if (context_.headless && context_.headless()) {
        refuse(QStringLiteral("A headless session opens no file dialog: type CUSTOMISE EXPORT "
                              "<file> instead, or fill settingsExportPath."));
        return;
    }
    QString chosen = QFileDialog::getSaveFileName(
        this, QStringLiteral("Export the Customisation"), typedFilePath(exportPath_->text()),
        customisationFileFilter());
    if (chosen.isEmpty()) {
        return;
    }
    // A name that does not end .json, in any letter case, is given a
    // customisation file's ending, so that the file is one Import's own filter
    // shows; one that does is left as the person wrote it.
    if (!chosen.endsWith(QStringLiteral(".json"), Qt::CaseInsensitive)) {
        chosen += QStringLiteral(".customisation.json");
    }
    exportPath_->setText(QDir::fromNativeSeparators(chosen));
}

void SettingsDialog::automationToggled(const char* key, bool on)
{
    if (loading_) {
        return;
    }
    run(QStringLiteral("CUSTOMISE SET %1=%2")
            .arg(QLatin1String(key), on ? QStringLiteral("on") : QStringLiteral("off")),
        Then::KeepWhatWasKept);
    // The box shows what the DOCUMENT has. A line that was refused, or that
    // nothing ran, left the Document as it was, and no notification comes to
    // say so: the box goes back.
    showAutomation();
}

// ---- the Updates page --------------------------------------------------------------------------

void SettingsDialog::setUpdates(SettingsUpdates updates)
{
    context_.updates = std::move(updates);
    showUpdates();
}

void SettingsDialog::showUpdates()
{
    const SettingsUpdates& updates = context_.updates;
    const bool lines = static_cast<bool>(context_.run);

    updateNote_->setText(updates.note);
    updateNote_->setVisible(!updates.note.isEmpty());
    updateSends_->setText(updateSendsWords(updates));

    // The box shows what the STATE has, as the automation boxes show the
    // Document: a line that was refused left it as it was, and the box goes
    // back. With nobody to ask it is off and disabled, never a guess.
    const bool wasLoading = loading_;
    loading_ = true;
    updateAuto_->setChecked(updates.automatic && updates.automatic());
    loading_ = wasLoading;
    updateAuto_->setEnabled(lines && static_cast<bool>(updates.automatic));

    updateTokenState_->setText(updateTokenWords(updates.credentials));
    updateToken_->setEnabled(updates.credentials != nullptr);
    updateTokenSave_->setEnabled(updates.credentials != nullptr);
    updateTokenSave_->setToolTip(
        updates.credentials != nullptr
            ? QStringLiteral("Store the token typed here for the update checks. It is written "
                             "to the credential store and to nothing else: no line is run, so "
                             "the command log never holds it")
            : QStringLiteral("This session has no credential store to save a token in; the "
                             "environment variable %1 gives a run one.")
                  .arg(text(katana::update::kTokenEnvironmentVariable)));
    updateTokenClear_->setEnabled(lines);

    // With nobody to say which version is skipped - nobody to ask, or a state
    // file that does not read - the page says nothing of it, the button stays
    // on and the verb answers; with none skipped it has nothing to do.
    const std::optional<QString> skipped =
        updates.skippedVersion ? updates.skippedVersion() : std::nullopt;
    updateSkipped_->setText(skipped ? updateSkippedWords(*skipped) : QString());
    updateUnskip_->setEnabled(lines && (!skipped || !skipped->isEmpty()));
    updateUnskip_->setToolTip(QStringLiteral(
        "Forget which version was skipped, so that the next check offers it (UPDATE SKIP NONE)"));

    // The same of a version that was put back: on while nobody says whether
    // one was, and off when none was.
    const std::optional<bool> putBack = updates.putBack ? updates.putBack() : std::nullopt;
    updateBlocked_->setText(putBack ? updatePutBackWords(*putBack) : QString());
    updateUnblock_->setEnabled(lines && putBack.value_or(true));
    updateUnblock_->setToolTip(QStringLiteral(
        "Forget which versions were put back because they did not start here, so that the next "
        "check offers the newest again and it can be installed (UPDATE BLOCKED CLEAR)"));
}

void SettingsDialog::runUpdatesLine(const QString& line)
{
    if (!context_.run) {
        refuse(QStringLiteral("Nothing here can run %1: Settings was given no command line.")
                   .arg(line));
        showUpdates();
        return;
    }
    // No Document is asked for: these lines are of the program, and run with
    // no drawing and after one has closed.
    const VerbOutcome outcome = context_.run(line);
    say(answered(line, outcome), !outcome.ok);
    showUpdates();
}

void SettingsDialog::saveUpdateToken()
{
    katana::update::CredentialStore* const store = context_.updates.credentials;
    if (store == nullptr) {
        refuse(QStringLiteral("Save: this session has no credential store to save a token in."));
        return;
    }
    // The blanks round a pasted token are no part of it; anything else that
    // makes it no token is the store's to refuse.
    const QString typed = updateToken_->text().trimmed();
    if (typed.isEmpty()) {
        refuse(QStringLiteral("Save: no access token is typed. Paste one, or press Clear to "
                              "remove the stored one."));
        return;
    }
    // NO LINE: a line is echoed in the command log, and a token must never be
    // there. Nor is it in anything said below - the store's refusal names the
    // place and the rule, never the value (update/credential.hpp), and only
    // its message is shown, never its context.
    const katana::core::Status written = store->write(typed.toStdString());
    if (!written) {
        refuse(QStringLiteral("Save: ") + text(written.error().message));
        return;
    }
    updateToken_->clear();
    say(QStringLiteral("The access token was saved to the %1. It is not shown again.")
            .arg(text(store->where())),
        false);
    if (context_.updates.tokenChanged) {
        context_.updates.tokenChanged();
    }
    showUpdates();
}

} // namespace katana::qt
