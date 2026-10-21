#include "start_dialog.hpp"

#include <algorithm>

#include <QCheckBox>
#include <QComboBox>
#include <QCompleter>
#include <QCoreApplication>
#include <QDialogButtonBox>
#include <QDir>
#include <QFileDialog>
#include <QFormLayout>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPixmap>
#include <QPushButton>
#include <QStandardItemModel>
#include <QVBoxLayout>

#include "command_word.hpp"
#include "customisation/code_manager_support.hpp"
#include "icons.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/customisation_state.hpp"
#include "katana/cad/project_crs.hpp"
#include "project_crs_dialog.hpp"
#include "theme.hpp"

namespace katana::qt {

namespace {

QString qs(const std::string& text)
{
    return QString::fromStdString(text);
}

// What a customisation item is: the session as it stands, a built-in by name,
// or a file to load.
constexpr int kKindRole = Qt::UserRole;
constexpr int kNameRole = Qt::UserRole + 1;
constexpr QLatin1StringView kNow("now");
constexpr QLatin1StringView kBuiltIn("builtin");
constexpr QLatin1StringView kFile("file");

// The coordinate system item that is local coordinates.
constexpr QLatin1StringView kLocal("Local coordinates (no coordinate system)");

// How a session that is no built-in as installed came to be, in words.
QString originWords(katana::cad::CustomisationOrigin origin)
{
    switch (origin) {
    case katana::cad::CustomisationOrigin::Kept:
        return QStringLiteral("kept from the last session");
    case katana::cad::CustomisationOrigin::Loaded:
        return QStringLiteral("loaded in this session");
    case katana::cad::CustomisationOrigin::Edited:
        return QStringLiteral("edited in this session");
    case katana::cad::CustomisationOrigin::BuiltIn:
        return QStringLiteral("built in");
    case katana::cad::CustomisationOrigin::None:
        break;
    }
    return QString();
}

// A common system as the list shows it: its code, then its name.
QString crsItemText(const katana::cad::CrsChoice& choice)
{
    return qs(choice.id) + QStringLiteral("  ") + qs(choice.name);
}

} // namespace

StartDialog::StartDialog(katana::cad::Document& document, StartChoices last, CommandRunner runner,
                         QWidget* parent, const std::vector<katana::cad::BuiltInEntry>& builtIns)
    : QDialog(parent), document_(document), runner_(std::move(runner)),
      keptFile_(std::move(last.keptFile))
{
    setObjectName(QStringLiteral("startDialog"));
    setWindowTitle(QStringLiteral("Start"));
    setMinimumWidth(560);
    auto* layout = new QVBoxLayout(this);

    // Which program, and what is asked.
    auto* header = new QHBoxLayout();
    auto* logo = new QLabel(this);
    logo->setObjectName(QStringLiteral("startLogo"));
    logo->setPixmap(QPixmap::fromImage(applicationIconImage(64)));
    header->addWidget(logo, 0, Qt::AlignTop);
    const QString version = QCoreApplication::applicationVersion();
    auto* title = new QLabel(
        QStringLiteral("<h2 style='margin:0'>Katana%1</h2>"
                       "<p style='color:%2'>Choose what this session starts with: the "
                       "customisation the survey codes are drawn by, and the coordinate system "
                       "the drawing is in. Both can be changed later, in File &gt; Settings and "
                       "File &gt; Project Coordinate System.</p>")
            .arg(version.isEmpty() ? QString() : QStringLiteral(" ") + version,
                 theme::textMuted().name()),
        this);
    title->setObjectName(QStringLiteral("startTitle"));
    title->setWordWrap(true);
    header->addWidget(title, 1);
    layout->addLayout(header);

    // ---- the customisation ----
    auto* customisationBox = new QGroupBox(QStringLiteral("Customisation"), this);
    auto* customisationForm = new QFormLayout(customisationBox);
    customisation_ = new QComboBox(customisationBox);
    customisation_->setObjectName(QStringLiteral("startCustomisation"));
    const katana::cad::CustomisationState& state = document_.customisationState();
    const std::vector<katana::cad::BuiltInChoice> choices =
        katana::cad::builtInChoices(document_, builtIns);
    for (const katana::cad::BuiltInChoice& choice : choices) {
        if (choice.isActive) {
            activeBuiltIn_ = qs(choice.name);
        }
    }
    // A session that is no built-in as installed - a kept copy, a file
    // loaded, or none at all (a run whose seam says none) - is offered first
    // as it stands, which makes no line: Start on the screen as it opens
    // changes nothing.
    if (activeBuiltIn_.isEmpty()) {
        customisation_->addItem(
            state.origin == katana::cad::CustomisationOrigin::None
                ? QStringLiteral("None - as it is, no customisation installed")
                : QStringLiteral("%1 - as it is, %2")
                      .arg(qs(state.name), originWords(state.origin)));
        customisation_->setItemData(0, QString(kNow), kKindRole);
    }
    int selected = customisation_->count() > 0 ? 0 : -1;
    for (const katana::cad::BuiltInChoice& choice : choices) {
        const int at = customisation_->count();
        customisation_->addItem(qs(choice.name) +
                                (choice.isDefault ? QStringLiteral(" (built in, the default)")
                                                  : QStringLiteral(" (built in)")));
        customisation_->setItemData(at, QString(kBuiltIn), kKindRole);
        customisation_->setItemData(at, qs(choice.name), kNameRole);
        customisation_->setItemData(
            at, choice.problem.empty() ? qs(choice.description) : qs(choice.problem),
            Qt::ToolTipRole);
        if (!choice.problem.empty()) {
            // Shown, and why it cannot be used, but not to be chosen.
            if (auto* model = qobject_cast<QStandardItemModel*>(customisation_->model())) {
                model->item(at)->setEnabled(false);
            }
        }
        if (choice.isActive || (selected < 0 && choice.isDefault && choice.problem.empty())) {
            selected = at;
        }
    }
    customisation_->addItem(QStringLiteral("From a customisation file..."));
    customisation_->setItemData(customisation_->count() - 1, QString(kFile), kKindRole);
    customisation_->setCurrentIndex(std::max(selected, 0));
    customisationForm->addRow(QStringLiteral("Start with"), customisation_);

    auto* fileRow = new QWidget(customisationBox);
    auto* fileLayout = new QHBoxLayout(fileRow);
    fileLayout->setContentsMargins(0, 0, 0, 0);
    file_ = new QLineEdit(fileRow);
    file_->setObjectName(QStringLiteral("startCustomisationFile"));
    file_->setPlaceholderText(QStringLiteral("a Katana customisation file (.json)"));
    browse_ = new QPushButton(QStringLiteral("Browse..."), fileRow);
    browse_->setObjectName(QStringLiteral("startCustomisationBrowse"));
    fileLayout->addWidget(file_, 1);
    fileLayout->addWidget(browse_);
    customisationForm->addRow(QStringLiteral("File"), fileRow);
    replace_ = new QCheckBox(QStringLiteral("Replace instead of merging"), customisationBox);
    replace_->setObjectName(QStringLiteral("startCustomisationReplace"));
    replace_->setToolTip(QStringLiteral("Without it the file's definitions and rules are merged "
                                        "onto the session's (CUSTOMISE REPLACE)"));
    customisationForm->addRow(QString(), replace_);
    keep_ = new QCheckBox(QStringLiteral("Use it at every start"), customisationBox);
    keep_->setObjectName(QStringLiteral("startCustomisationKeep"));
    keep_->setToolTip(keptFile_.isEmpty()
                          ? QStringLiteral("This window keeps no customisation between runs")
                          : QStringLiteral("Write it to the kept file, which the next start "
                                           "reads (CUSTOMISE KEEP): %1")
                                .arg(keptFile_));
    // As Settings' Use keeps what was kept: a session that the next start
    // would give again stays so.
    keep_->setChecked(!keptFile_.isEmpty() && state.kept);
    customisationForm->addRow(QString(), keep_);
    layout->addWidget(customisationBox);

    // ---- the coordinate system ----
    auto* crsBox = new QGroupBox(QStringLiteral("Coordinate system"), this);
    auto* crsLayout = new QVBoxLayout(crsBox);
    crs_ = new QComboBox(crsBox);
    crs_->setObjectName(QStringLiteral("startCrs"));
    crs_->setEditable(true);
    crs_->setInsertPolicy(QComboBox::NoInsert);
    crs_->setToolTip(QStringLiteral("Pick a common system, or type an EPSG code, WKT or a PROJ "
                                    "string (CRS SET)"));
    crs_->addItem(QString(kLocal), QString());
    for (const katana::cad::CrsChoice& choice : katana::cad::commonCoordinateSystems()) {
        crs_->addItem(crsItemText(choice), qs(choice.id));
    }
    // Typing finds a system by any part of its code or name.
    if (QCompleter* completer = crs_->completer()) {
        completer->setFilterMode(Qt::MatchContains);
        completer->setCompletionMode(QCompleter::PopupCompletion);
    }
    // The drawing's own when it has one, else the one chosen last.
    const QString now = qs(document_.metadata().coordinateSystem);
    const QString wanted = now.isEmpty() ? last.coordinateSystem.trimmed() : now;
    if (wanted.isEmpty()) {
        crs_->setCurrentIndex(0);
    } else if (const int at = crs_->findData(wanted); at >= 0) {
        crs_->setCurrentIndex(at);
    } else {
        crs_->setEditText(wanted);
    }
    crsLayout->addWidget(crs_);
    crsCheck_ = new QLabel(crsBox);
    crsCheck_->setObjectName(QStringLiteral("startCrsCheck"));
    crsCheck_->setWordWrap(true);
    crsLayout->addWidget(crsCheck_);
    layout->addWidget(crsBox);

    // The lines Start runs, as they will be logged: what a person learns
    // the verbs from, and what an agent types instead.
    line_ = new QLabel(this);
    line_->setObjectName(QStringLiteral("startLine"));
    line_->setWordWrap(true);
    line_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    layout->addWidget(line_);
    status_ = new QLabel(this);
    status_->setObjectName(QStringLiteral("startStatus"));
    status_->setWordWrap(true);
    status_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    layout->addWidget(status_);

    auto* bottom = new QHBoxLayout();
    showAtStart_ = new QCheckBox(QStringLiteral("Show this at start"), this);
    showAtStart_->setObjectName(QStringLiteral("startShowAtStart"));
    showAtStart_->setToolTip(QStringLiteral("File > Start Screen opens it at any time"));
    showAtStart_->setChecked(last.showAtStart);
    bottom->addWidget(showAtStart_);
    auto* buttons = new QDialogButtonBox(this);
    start_ = buttons->addButton(QStringLiteral("Start"), QDialogButtonBox::AcceptRole);
    start_->setObjectName(QStringLiteral("startOk"));
    start_->setDefault(true);
    QPushButton* skip = buttons->addButton(QStringLiteral("Skip"), QDialogButtonBox::RejectRole);
    skip->setObjectName(QStringLiteral("startSkip"));
    skip->setToolTip(QStringLiteral("Start with the session as it is"));
    bottom->addWidget(buttons, 1);
    layout->addLayout(bottom);

    connect(start_, &QPushButton::clicked, this, [this] { (void)start(); });
    connect(skip, &QPushButton::clicked, this, &QDialog::reject);
    connect(browse_, &QPushButton::clicked, this, [this] {
        const QString chosen = QFileDialog::getOpenFileName(
            this, QStringLiteral("Start with a Katana Customisation"),
            typedFilePath(file_->text()),
            QStringLiteral("Katana customisation (*.json);;All files (*)"));
        if (!chosen.isEmpty()) {
            file_->setText(QDir::toNativeSeparators(chosen));
        }
    });
    connect(customisation_, &QComboBox::currentIndexChanged, this, [this] { refresh(); });
    connect(crs_, &QComboBox::currentTextChanged, this, [this] { refresh(); });
    for (QCheckBox* box : {replace_, keep_}) {
        connect(box, &QCheckBox::toggled, this, [this] { refresh(); });
    }
    connect(file_, &QLineEdit::textChanged, this, [this] { refresh(); });
    refresh();
}

QString StartDialog::coordinateSystem() const
{
    const QString typed = crs_->currentText().trimmed();
    // An item of the list chosen and left as the list wrote it is its code;
    // anything else is what was typed.
    const int at = crs_->findText(typed);
    if (at >= 0) {
        return crs_->itemData(at).toString();
    }
    return typed == kLocal ? QString() : typed;
}

bool StartDialog::showAtStart() const
{
    return showAtStart_->isChecked();
}

katana::core::Result<QStringList> StartDialog::lines() const
{
    using katana::core::ErrorCode;
    using katana::core::makeError;
    QStringList out;
    const QString kind = customisation_->currentData(kKindRole).toString();
    bool changed = false;
    if (kind == kBuiltIn) {
        const QString name = customisation_->currentData(kNameRole).toString();
        if (name != activeBuiltIn_) {
            auto word = commandWord(name, QStringLiteral("the built-in customisation"));
            if (!word) {
                return word.error();
            }
            out << QStringLiteral("CUSTOMISE BUILTIN ") + *word;
            changed = true;
        }
    } else if (kind == kFile) {
        auto word = typedCustomisationFileWord(file_->text());
        if (!word) {
            return word.error();
        }
        out << (replace_->isChecked() ? QStringLiteral("CUSTOMISE REPLACE ")
                                      : QStringLiteral("CUSTOMISE ")) +
                   *word;
        changed = true;
    }
    if (changed && keep_->isEnabled() && keep_->isChecked()) {
        out << QStringLiteral("CUSTOMISE KEEP");
    }

    const QString chosen = coordinateSystem();
    std::string stored;
    if (!chosen.isEmpty()) {
        auto normalised = katana::cad::normaliseCoordinateSystem(chosen.toStdString());
        if (!normalised) {
            return makeError(ErrorCode::InvalidCRS,
                             "the coordinate system: " + normalised.error().message,
                             chosen.toStdString());
        }
        stored = *normalised;
    }
    if (stored != document_.metadata().coordinateSystem) {
        // The text goes on the line as typed: CRS SET reads everything after
        // SET verbatim, so a WKT keeps its quotes (as ProjectCrsDialog writes it).
        out << (chosen.isEmpty() ? QStringLiteral("CRS CLEAR") : QStringLiteral("CRS SET ") + chosen);
    }
    return out;
}

void StartDialog::refresh()
{
    const QString kind = customisation_->currentData(kKindRole).toString();
    const bool fromFile = kind == kFile;
    file_->setEnabled(fromFile);
    browse_->setEnabled(fromFile);
    replace_->setEnabled(fromFile);
    // Keeping is for a choice that changes the session, in a window that
    // keeps one.
    const bool changes =
        fromFile || (kind == kBuiltIn &&
                     customisation_->currentData(kNameRole).toString() != activeBuiltIn_);
    keep_->setEnabled(!keptFile_.isEmpty() && changes);

    crsCheck_->setText(crsCheckText(coordinateSystem()));
    const auto made = lines();
    if (!made) {
        line_->setText(qs(made.error().message));
        line_->setStyleSheet(QStringLiteral("color: #d05050;"));
        start_->setEnabled(false);
        return;
    }
    line_->setStyleSheet(QString());
    line_->setText(made->isEmpty() ? QStringLiteral("Nothing to change: the session starts as it is.")
                                   : made->join(QLatin1Char('\n')));
    start_->setEnabled(true);
}

VerbOutcome StartDialog::run(const QString& line)
{
    if (runner_) {
        return runner_(line);
    }
    VerbOutcome outcome;
    katana::cad::CommandInterpreter interpreter(document_);
    const auto reply = interpreter.run(line.toStdString());
    outcome.ok = reply.ok();
    if (reply) {
        outcome.reply = qs(*reply);
    } else {
        outcome.error = qs(reply.error().describe());
    }
    return outcome;
}

bool StartDialog::start()
{
    const auto made = lines();
    if (!made) {
        status_->setText(QStringLiteral("Not started: ") + qs(made.error().message));
        return false;
    }
    QStringList said;
    for (const QString& line : *made) {
        const VerbOutcome outcome = run(line);
        said << QStringLiteral("> ") + line;
        if (!outcome.ok) {
            said << outcome.error;
            status_->setText(said.join(QLatin1Char('\n')));
            refresh();
            return false;
        }
        if (!outcome.reply.isEmpty()) {
            said << outcome.reply.section(QLatin1Char('\n'), 0, 0);
        }
    }
    status_->setText(said.join(QLatin1Char('\n')));
    accept();
    return true;
}

} // namespace katana::qt
