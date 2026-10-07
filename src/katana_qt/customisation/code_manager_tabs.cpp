// The survey code manager's other four tabs: the codes a drawing carries,
// what is wrong with the rules being edited, applying the codes, and turning
// coded points into lines. Each shows a cad report. On the two that change
// the drawing, Preview plans and never executes, and Execute hands the line
// the tab shows - CODE ..., LINEWORK ... - to the window's executor: one undo
// step, logged and undone as a typed line (code_manager.hpp).

#include <algorithm>
#include <cmath>
#include <optional>

#include <QComboBox>
#include <QCompleter>
#include <QDoubleSpinBox>
#include <QFormLayout>
#include <QFrame>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QScrollArea>
#include <QStyle>
#include <QTabWidget>
#include <QTimer>
#include <QTreeWidget>
#include <QVBoxLayout>

#include "command_word.hpp"
#include "customisation/code_manager.hpp"
#include "customisation/filter_bar.hpp"
#include "customisation/linework_labels.hpp"
#include "customisation/name_picker.hpp"
#include "customisation/scope_filter_widget.hpp"
#include "katana/cad/colour_lookup.hpp"
#include "katana/cad/global_modify.hpp"
#include "katana/cad/linework_verbs.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/linework_codes.hpp"
#include "katana/entity/tables.hpp"
#include "theme.hpp"

namespace katana::qt {

using katana::entity::SurveyMatchKind;

namespace {

constexpr int kCodeRole = Qt::UserRole + 1;
constexpr int kRuleRole = Qt::UserRole + 2;

[[nodiscard]] QString text(const std::string& value)
{
    return QString::fromStdString(value);
}

[[nodiscard]] std::string text(const QString& value)
{
    return value.toStdString();
}

[[nodiscard]] QString matchClass(SurveyMatchKind kind, bool matched)
{
    if (matched) {
        return QObject::tr("matched");
    }
    return kind == SurveyMatchKind::FallbackOnly ? QObject::tr("fallback only")
                                                 : QObject::tr("unmatched");
}

// A property chooser: "find it" first (the empty name - applySurveyCodes
// then finds it), then the names codes are usually kept under; any other
// may be typed.
QComboBox* propertyChooser(QWidget* parent, const char* name)
{
    auto* box = new QComboBox(parent);
    box->setObjectName(QString::fromLatin1(name));
    box->setEditable(true);
    box->setInsertPolicy(QComboBox::NoInsert);
    box->addItem(QString());
    for (const std::string& candidate : katana::cad::codePropertyCandidates()) {
        box->addItem(text(candidate));
    }
    box->lineEdit()->setPlaceholderText(QObject::tr("found in the drawing"));
    // A property name is matched exactly: "Code" typed is not "code".
    box->completer()->setCaseSensitivity(Qt::CaseSensitive);
    return box;
}

QTreeWidget* reportTree(QWidget* parent, const char* name, const QStringList& headers)
{
    auto* tree = new QTreeWidget(parent);
    tree->setObjectName(QString::fromLatin1(name));
    tree->setColumnCount(static_cast<int>(headers.size()));
    tree->setHeaderLabels(headers);
    tree->setRootIsDecorated(false);
    tree->setUniformRowHeights(true);
    tree->setEditTriggers(QAbstractItemView::NoEditTriggers);
    return tree;
}

void resizeColumns(QTreeWidget* tree)
{
    for (int column = 0; column + 1 < tree->columnCount(); ++column) {
        tree->resizeColumnToContents(column);
    }
}

} // namespace

// ---- codes in drawing ------------------------------------------------------------------

QWidget* SurveyCodeManagerDialog::buildCensusTab()
{
    auto* page = new QWidget(this);
    page->setObjectName(QStringLiteral("codesInDrawingTab"));
    auto* layout = new QVBoxLayout(page);

    auto* top = new QHBoxLayout;
    top->addWidget(new QLabel(tr("Code property"), page));
    censusPropertyBox_ = propertyChooser(page, "censusProperty");
    top->addWidget(censusPropertyBox_, 1);
    censusFilter_ = new FilterBar({tr("All"), tr("Matched"), tr("Fallback only"), tr("Unmatched")},
                                  page);
    censusFilter_->setObjectName(QStringLiteral("censusFilter"));
    censusFilter_->setPlaceholderText(tr("Search codes and layers"));
    top->addWidget(censusFilter_, 2);
    layout->addLayout(top);

    censusTree_ = reportTree(page, "censusTable",
                             {tr("Code"), tr("Entities"), tr("Class"), tr("Layer")});
    layout->addWidget(censusTree_, 1);
    censusSummary_ = new QLabel(page);
    censusSummary_->setObjectName(QStringLiteral("censusSummary"));
    censusSummary_->setWordWrap(true);
    layout->addWidget(censusSummary_);

    auto* buttons = new QHBoxLayout;
    auto* select = new QPushButton(tr("Select Entities With Code"), page);
    select->setObjectName(QStringLiteral("censusSelect"));
    auto* newRule = new QPushButton(tr("New Rule From Code"), page);
    newRule->setObjectName(QStringLiteral("censusNewRule"));
    auto* refresh = new QPushButton(tr("Count Again"), page);
    refresh->setObjectName(QStringLiteral("censusRefresh"));
    buttons->addWidget(select);
    buttons->addWidget(newRule);
    buttons->addStretch(1);
    buttons->addWidget(refresh);
    layout->addLayout(buttons);

    connect(censusPropertyBox_, &QComboBox::currentTextChanged, this,
            [this](const QString&) { refreshCensus(); });
    censusFilter_->onTextChanged = [this](const QString&) { filterCensus(); };
    censusFilter_->onChipChanged = [this](int) { filterCensus(); };
    connect(select, &QPushButton::clicked, this, [this] { selectEntitiesWithCode(); });
    connect(newRule, &QPushButton::clicked, this, [this] {
        const std::string code = selectedCensusCode();
        if (code.empty()) {
            log(tr("Choose a code in the list first."), true);
            return;
        }
        newRuleFromCode(code);
    });
    connect(refresh, &QPushButton::clicked, this, [this] { refreshCensus(); });
    // A double-click shows what the code gets, as Test a Code would.
    connect(censusTree_, &QTreeWidget::itemDoubleClicked, this,
            [this](QTreeWidgetItem* item, int) {
                const QString code = item->data(0, kCodeRole).toString();
                QTimer::singleShot(0, this, [this, code] {
                    tabs_->setCurrentIndex(0);
                    testCode_->setText(code);
                });
            });
    return page;
}

std::string SurveyCodeManagerDialog::censusProperty() const
{
    return std::string(katana::core::trimmed(text(censusPropertyBox_->currentText())));
}

void SurveyCodeManagerDialog::refreshCensus()
{
    census_.clear();
    const katana::cad::Document* doc = document();
    if (doc != nullptr) {
        const katana::cad::CodeCensus census = katana::cad::codeCensus(*doc, censusProperty());
        censusFoundProperty_ = census.property;
        for (const katana::cad::CodeCensusRow& row : census.codes) {
            census_.push_back(CensusRow{row.code, row.entities, row.kind, row.matched, row.model});
        }
    }
    filterCensus();
}

void SurveyCodeManagerDialog::filterCensus()
{
    // Classed against the BUFFER, not the Document's map: the census is how
    // a person checks the rules they are writing against the codes the
    // drawing actually carries, before applying them.
    std::size_t entities = 0, matched = 0, fallback = 0, unmatched = 0;
    for (CensusRow& row : census_) {
        const katana::entity::SurveyMatch match = buffer_.lookup(row.code);
        row.kind = match.kind;
        row.matched = match.matched();
        row.model = match.resolved.model;
        entities += row.entities;
        if (row.matched) {
            ++matched;
        } else if (row.kind == SurveyMatchKind::FallbackOnly) {
            ++fallback;
        } else {
            ++unmatched;
        }
    }
    censusFilter_->setChipLabel(1, tr("Matched (%1)").arg(matched));
    censusFilter_->setChipLabel(2, tr("Fallback only (%1)").arg(fallback));
    censusFilter_->setChipLabel(3, tr("Unmatched (%1)").arg(unmatched));

    const std::string selected = selectedCensusCode();
    const std::string filter = katana::core::lowered(text(censusFilter_->text()));
    censusTree_->clear();
    QTreeWidgetItem* keep = nullptr;
    for (const CensusRow& row : census_) {
        const int chip = censusFilter_->chip();
        const bool isFallback = !row.matched && row.kind == SurveyMatchKind::FallbackOnly;
        if ((chip == 1 && !row.matched) || (chip == 2 && !isFallback) ||
            (chip == 3 && (row.matched || isFallback))) {
            continue;
        }
        if (!filter.empty() && katana::core::lowered(row.code).find(filter) == std::string::npos &&
            katana::core::lowered(row.model).find(filter) == std::string::npos) {
            continue;
        }
        auto* item = new QTreeWidgetItem(censusTree_);
        item->setText(0, text(row.code));
        item->setData(0, kCodeRole, text(row.code));
        item->setText(1, QString::number(row.entities));
        item->setTextAlignment(1, Qt::AlignRight | Qt::AlignVCenter);
        item->setText(2, matchClass(row.kind, row.matched));
        if (!row.matched) {
            item->setForeground(2, kUndefinedNameColour);
        }
        item->setText(3, text(row.model));
        if (row.code == selected) {
            keep = item;
        }
    }
    if (keep != nullptr) {
        censusTree_->setCurrentItem(keep);
    }
    resizeColumns(censusTree_);
    censusSummary_->setText(
        tr("%1 entities carry a code under \"%2\": %3 distinct codes - %4 matched, %5 fallback "
           "only, %6 unmatched by the rules being edited.")
            .arg(entities)
            .arg(text(censusFoundProperty_))
            .arg(census_.size())
            .arg(matched)
            .arg(fallback)
            .arg(unmatched));
}

std::string SurveyCodeManagerDialog::selectedCensusCode() const
{
    const QTreeWidgetItem* item = censusTree_->currentItem();
    return item == nullptr ? std::string() : text(item->data(0, kCodeRole).toString());
}

void SurveyCodeManagerDialog::selectEntitiesWithCode()
{
    katana::cad::Document* doc = document();
    const std::string code = selectedCensusCode();
    if (doc == nullptr || code.empty()) {
        log(tr("Choose a code in the list first."), true);
        return;
    }
    std::vector<katana::entity::EntityId> ids;
    doc->model().entities.forEach([&](const katana::entity::Entity& entity) {
        const std::string* carried = katana::cad::surveyCodeOf(entity, censusFoundProperty_);
        if (carried != nullptr && *carried == code) {
            ids.push_back(entity.id);
        }
    });
    if (context_.selectAndShow) {
        context_.selectAndShow(ids);
    } else {
        doc->selection().set(ids);
        doc->notifySelectionChanged();
    }
    log(tr("Selected the %1 entities coded %2.").arg(ids.size()).arg(text(code)));
}

// ---- issues ----------------------------------------------------------------------------

QWidget* SurveyCodeManagerDialog::buildIssuesTab()
{
    auto* page = new QWidget(this);
    page->setObjectName(QStringLiteral("codeIssuesTab"));
    auto* layout = new QVBoxLayout(page);
    issuesFilter_ = new FilterBar({tr("All"), tr("Errors"), tr("Warnings")}, page);
    issuesFilter_->setObjectName(QStringLiteral("issuesFilter"));
    issuesFilter_->setPlaceholderText(tr("Search keys and messages"));
    layout->addWidget(issuesFilter_);
    issuesTree_ = reportTree(page, "issuesTable",
                             {tr("Severity"), tr("Rule"), tr("Key"), tr("Section"), tr("Problem"),
                              tr("Message")});
    issuesTree_->setToolTip(tr("Double-click an issue to edit its rule"));
    layout->addWidget(issuesTree_, 1);
    issuesSummary_ = new QLabel(page);
    issuesSummary_->setObjectName(QStringLiteral("issuesSummary"));
    layout->addWidget(issuesSummary_);

    issuesFilter_->onTextChanged = [this](const QString&) { rebuildIssues(); };
    issuesFilter_->onChipChanged = [this](int) { rebuildIssues(); };
    connect(issuesTree_, &QTreeWidget::itemDoubleClicked, this,
            [this](QTreeWidgetItem* item, int) {
                // Deferred, as every jump from a list is: the jump may
                // rebuild lists, and none is rebuilt inside its own signal.
                const QVariant rule = item->data(0, kRuleRole);
                if (rule.isValid()) {
                    const std::size_t index = rule.toULongLong();
                    QTimer::singleShot(0, this, [this, index] { selectRule(index); });
                }
            });
    return page;
}

void SurveyCodeManagerDialog::rebuildIssues()
{
    const katana::cad::Document* doc = document();
    issues_ = doc == nullptr
                  ? std::vector<katana::cad::LintIssue>{}
                  : katana::cad::lintSurveyMap(
                        buffer_, doc->styleLibrary(), katana::cad::colourLookup(*doc),
                        [](std::string_view name) {
                            return katana::entity::isBuiltInSymbolName(name);
                        });
    std::size_t errors = 0, warnings = 0;
    for (const katana::cad::LintIssue& issue : issues_) {
        (issue.severity == katana::cad::LintSeverity::Error ? errors : warnings) += 1;
    }
    issuesFilter_->setChipLabel(1, tr("Errors (%1)").arg(errors));
    issuesFilter_->setChipLabel(2, tr("Warnings (%1)").arg(warnings));

    const QIcon errorIcon = style()->standardIcon(QStyle::SP_MessageBoxCritical);
    const QIcon warningIcon = style()->standardIcon(QStyle::SP_MessageBoxWarning);
    const int chip = issuesFilter_->chip();
    const std::string filter = katana::core::lowered(text(issuesFilter_->text()));
    issuesTree_->clear();
    for (const katana::cad::LintIssue& issue : issues_) {
        const bool isError = issue.severity == katana::cad::LintSeverity::Error;
        if ((chip == 1 && !isError) || (chip == 2 && isError)) {
            continue;
        }
        if (!filter.empty() && katana::core::lowered(issue.key).find(filter) == std::string::npos &&
            katana::core::lowered(issue.message).find(filter) == std::string::npos) {
            continue;
        }
        auto* item = new QTreeWidgetItem(issuesTree_);
        item->setIcon(0, isError ? errorIcon : warningIcon);
        item->setText(0, QString::fromLatin1(katana::cad::toString(issue.severity)));
        item->setData(0, kRuleRole, QVariant::fromValue<qulonglong>(issue.rule));
        item->setText(1, QStringLiteral("#%1").arg(issue.rule));
        item->setText(2, text(issue.key));
        item->setText(3, QString::fromLatin1(katana::entity::toString(issue.section)));
        item->setText(4, QString::fromLatin1(katana::cad::toString(issue.kind)));
        item->setText(5, text(issue.message));
    }
    resizeColumns(issuesTree_);
    issuesSummary_->setText(tr("%1 errors and %2 warnings in the %3 rules being edited.")
                                .arg(errors)
                                .arg(warnings)
                                .arg(buffer_.size()));
    const int issuesTab = tabs_ != nullptr ? tabs_->indexOf(issuesTree_->parentWidget()) : -1;
    if (issuesTab >= 0) {
        tabs_->setTabText(issuesTab, issues_.empty() ? tr("Issues")
                                                     : tr("Issues (%1)").arg(issues_.size()));
    }
}

// ---- the two action tabs: what they share ------------------------------------------------

namespace {

// A read-only field showing the line a tab's Execute runs, as the utilities
// dialog shows its own: what is run is never a surprise, and it can be copied
// to a script or handed to an agent.
QLineEdit* commandField(QWidget* parent, const char* name)
{
    auto* field = new QLineEdit(parent);
    field->setObjectName(QString::fromLatin1(name));
    field->setReadOnly(true);
    field->setFont(katana::qt::theme::monospaceFont());
    field->setToolTip(QObject::tr("The line Execute hands to the command line - typed there, or "
                                  "given to a script or an agent, it does the same"));
    return field;
}

// The scope and filter controls are taller than a tab on a small screen, so
// they scroll in a pane of their own beside what the tab reports.
QScrollArea* scrolled(ScopeFilterWidget* scope, QWidget* parent)
{
    auto* area = new QScrollArea(parent);
    area->setWidgetResizable(true);
    area->setFrameShape(QFrame::NoFrame);
    area->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    area->setWidget(scope);
    return area;
}

// The line in its field, or - where the controls cannot be said on a line -
// why not, as the field's placeholder.
void showLine(QLineEdit* field, const katana::core::Result<QString>& line)
{
    field->setText(line ? *line : QString());
    field->setPlaceholderText(line ? QString()
                                   : QObject::tr("nothing to run yet: %1")
                                         .arg(text(line.error().describe())));
}

// The chord length the Linework tab starts on, in millimetres: the one
// processLinework itself starts on (LineworkOptions, 0.005 of a drawing in
// metres), so the tab adds CHORD to its line only for a length a person set.
[[nodiscard]] double defaultChordMillimetres()
{
    return katana::cad::LineworkOptions{}.chordTolerance * 1000.0;
}

} // namespace

ScopeFilterWidget* SurveyCodeManagerDialog::makeScope(const char* prefix, QWidget* parent)
{
    auto* scope = new ScopeFilterWidget(QString::fromLatin1(prefix), parent);
    // The dialog's own views function, read afresh each time it is needed.
    scope->views = [this] { return views ? views() : ScopeFilterWidget::noWorkspaceViews(); };
    // The whole drawing to begin with, as both tabs began before they had
    // these controls: codes are applied, and a survey strung, to all of it
    // far more often than to what happens to be selected.
    scope->setChoice(ScopeChoice::Drawing);
    scope->onChanged = [this] { showLines(); };
    return scope;
}

void SurveyCodeManagerDialog::reloadScopes()
{
    const katana::cad::Document* doc = document();
    for (ScopeFilterWidget* scope : {applyScope_, lineworkScope_}) {
        if (scope == nullptr) {
            continue;
        }
        if (doc != nullptr) {
            scope->reload(doc->model());
        } else {
            scope->reloadViews();
        }
    }
}

katana::core::Result<std::vector<katana::entity::EntityId>>
SurveyCodeManagerDialog::scopeTakes(const ScopeFilterWidget& scope) const
{
    const katana::cad::Document* doc = document();
    if (doc == nullptr) {
        return katana::core::makeError(katana::core::ErrorCode::InvalidState,
                                       "the drawing this manager edits is closed");
    }
    auto where = scope.scope();
    if (!where) {
        return where.error();
    }
    auto which = scope.filter();
    if (!which) {
        return which.error();
    }
    return katana::cad::matchEntities(*doc, *where, *which);
}

void SurveyCodeManagerDialog::showLines()
{
    // A scope is set up - and signals - before the fields exist.
    if (applyCommand_ == nullptr || lineworkCommand_ == nullptr) {
        return;
    }
    showLine(applyCommand_, codesLine());
    showLine(lineworkCommand_, lineworkLine());
}

bool SurveyCodeManagerDialog::runLine(const QString& line, VerbOutcome& outcome,
                                      QString& said) const
{
    if (!context_.run) {
        // Said, and nothing done: the work is the line's, and a manager with
        // no command line to hand it to does not do it some other way.
        said = tr("This manager was opened without the window's command line, so it cannot run "
                  "\"%1\". Type it on a command line instead.")
                   .arg(line);
        return false;
    }
    outcome = context_.run(line);
    if (!outcome.ok) {
        said = outcome.error.isEmpty() ? tr("\"%1\" was refused.").arg(line) : outcome.error;
        return false;
    }
    return true;
}

// ---- apply codes -------------------------------------------------------------------------

QWidget* SurveyCodeManagerDialog::buildApplyTab()
{
    auto* page = new QWidget(this);
    page->setObjectName(QStringLiteral("applyCodesTab"));
    auto* columns = new QHBoxLayout(page);

    applyScope_ = makeScope("apply", page);
    columns->addWidget(scrolled(applyScope_, page), 2);

    auto* right = new QWidget(page);
    auto* layout = new QVBoxLayout(right);
    layout->setContentsMargins(0, 0, 0, 0);
    columns->addWidget(right, 3);

    auto* options = new QGroupBox(tr("Apply the drawing's survey map"), right);
    auto* form = new QFormLayout(options);
    applyProperty_ = propertyChooser(options, "applyProperty");
    form->addRow(tr("Code property"), applyProperty_);
    layout->addWidget(options);

    applyDirtyNote_ = new QLabel(tr("The rules being edited are not applied yet: codes are "
                                    "applied with the drawing's survey map. Apply the edits "
                                    "first to use them."),
                                 right);
    applyDirtyNote_->setObjectName(QStringLiteral("applyDirtyNote"));
    applyDirtyNote_->setWordWrap(true);
    applyDirtyNote_->setStyleSheet(QStringLiteral("color: %1;").arg(kUndefinedNameColour.name()));
    layout->addWidget(applyDirtyNote_);

    applyCommand_ = commandField(right, "applyCommand");
    layout->addWidget(applyCommand_);

    auto* buttons = new QHBoxLayout;
    auto* preview = new QPushButton(tr("Preview"), right);
    preview->setObjectName(QStringLiteral("applyPreview"));
    preview->setToolTip(tr("Work out what applying the codes would do, changing nothing"));
    auto* execute = new QPushButton(tr("Execute"), right);
    execute->setObjectName(QStringLiteral("applyExecute"));
    execute->setToolTip(tr("Run the line above: the codes applied as one undoable step"));
    buttons->addWidget(preview);
    buttons->addWidget(execute);
    buttons->addStretch(1);
    layout->addLayout(buttons);

    applyRows_ = reportTree(right, "applyRows",
                            {tr("Code"), tr("Entities"), tr("Class"), tr("Layer"), tr("Style"),
                             tr("Changed")});
    layout->addWidget(applyRows_, 2);
    applyReport_ = new QPlainTextEdit(right);
    applyReport_->setObjectName(QStringLiteral("applyReport"));
    applyReport_->setReadOnly(true);
    applyReport_->setLineWrapMode(QPlainTextEdit::NoWrap);
    layout->addWidget(applyReport_, 1);

    connect(preview, &QPushButton::clicked, this, [this] { previewCodes(); });
    connect(execute, &QPushButton::clicked, this, [this] { executeCodes(); });
    connect(applyProperty_, &QComboBox::currentTextChanged, this, [this] { showLines(); });
    return page;
}

katana::cad::SurveyCodingOptions SurveyCodeManagerDialog::codingOptions() const
{
    katana::cad::SurveyCodingOptions options;
    options.property = std::string(katana::core::trimmed(text(applyProperty_->currentText())));
    // The drawing's own resolver: its customisation's colours, then the
    // standard names - what the CODE line resolves a colour name by, so the
    // preview shows the style the line will make.
    if (const katana::cad::Document* doc = document()) {
        options.colourOf = katana::cad::colourLookup(*doc);
    }
    return options;
}

katana::core::Result<QString> SurveyCodeManagerDialog::codesLine() const
{
    auto scope = applyScope_->verbWords();
    if (!scope) {
        return scope.error();
    }
    QString line = QStringLiteral("CODE ") + *scope;
    const QString property = applyProperty_->currentText().trimmed();
    if (!property.isEmpty()) {
        auto word = commandWord(property, tr("Code property"));
        if (!word) {
            return word.error();
        }
        line += QStringLiteral(" PROPERTY ") + *word;
    }
    return line;
}

bool SurveyCodeManagerDialog::previewCodes()
{
    applyRows_->clear();
    const katana::cad::Document* doc = document();
    if (doc == nullptr) {
        applyReport_->setPlainText(tr("The drawing is closed."));
        return false;
    }
    const auto taken = scopeTakes(*applyScope_);
    if (!taken) {
        applyReport_->setPlainText(text(taken.error().describe()));
        log(text(taken.error().describe()), true);
        return false;
    }
    katana::cad::SurveyCodingOptions options = codingOptions();
    katana::cad::SurveyCodingReport report;
    bool something = false;
    if (taken->empty()) {
        // A scope that took nothing is said, and is NOT handed on: to
        // applySurveyCodes an empty list is every entity in the drawing.
        report.property = options.property.empty()
                              ? katana::cad::findCodeProperty(doc->model(), {})
                              : options.property;
    } else {
        options.ids = *taken;
        auto command = katana::cad::applySurveyCodes(*doc, options, &report);
        if (!command) {
            applyReport_->setPlainText(text(command.error().describe()));
            log(text(command.error().describe()), true);
            return false;
        }
        something = *command != nullptr;
    }
    for (const katana::cad::SurveyCodeRow& row : report.codes) {
        auto* item = new QTreeWidgetItem(applyRows_);
        item->setText(0, text(row.code));
        item->setText(1, QString::number(row.entities));
        item->setTextAlignment(1, Qt::AlignRight | Qt::AlignVCenter);
        item->setText(2, matchClass(row.kind, row.matched));
        QStringList from;
        for (const std::string& layer : row.layersFrom) {
            from << text(layer);
        }
        item->setText(3, row.layer.empty() ? tr("(stays)")
                         : QStringLiteral("%1 -> %2").arg(from.join(QStringLiteral(", ")),
                                                         text(row.layer)));
        item->setText(4, row.style.empty()
                             ? QString()
                             : QStringLiteral("%1 (%2)").arg(
                                   text(row.style),
                                   QString::fromLatin1(katana::cad::toString(row.styleOutcome))));
        item->setText(5, QString::number(row.changed));
        if (!row.matched) {
            item->setForeground(2, kUndefinedNameColour);
        }
    }
    resizeColumns(applyRows_);
    // What the scope took first, as the line's own reply begins.
    QString shown = tr("%1 matched.\n").arg(taken->size());
    shown += text(katana::cad::formatCodingReport(report));
    shown += something ? tr("\nPreview only: nothing has changed. Execute runs the line above "
                            "as one undoable step.")
                       : tr("\nNothing to change.");
    applyReport_->setPlainText(shown);
    return true;
}

void SurveyCodeManagerDialog::executeCodes()
{
    if (document() == nullptr) {
        return;
    }
    const auto line = codesLine();
    if (!line) {
        applyReport_->setPlainText(text(line.error().describe()));
        log(text(line.error().describe()), true);
        return;
    }
    // What is about to be done, row by row: the line's reply has the totals,
    // not which code went where. A preview that could not plan has said why,
    // in the pane and the log; the line would be refused for the same reason,
    // so it is not run to say it twice.
    if (!previewCodes()) {
        return;
    }
    VerbOutcome outcome;
    QString said;
    if (!runLine(*line, outcome, said)) {
        applyReport_->setPlainText(said);
        // A refused line is in the log already, where the executor ran it;
        // only a manager with no executor has yet to say so.
        if (!context_.run) {
            log(said, true);
        }
        return;
    }
    // In the verb's own words, which are in the command log as well: what
    // the scope took, the report, and whether anything was applied.
    applyReport_->setPlainText(outcome.reply);
}

// ---- linework ----------------------------------------------------------------------------

QWidget* SurveyCodeManagerDialog::buildLineworkTab()
{
    auto* page = new QWidget(this);
    page->setObjectName(QStringLiteral("lineworkTab"));
    auto* columns = new QHBoxLayout(page);

    lineworkScope_ = makeScope("linework", page);
    columns->addWidget(scrolled(lineworkScope_, page), 2);

    auto* right = new QWidget(page);
    auto* layout = new QVBoxLayout(right);
    layout->setContentsMargins(0, 0, 0, 0);
    columns->addWidget(right, 3);
    auto* top = new QHBoxLayout;

    auto* codes = new QGroupBox(tr("Control codes"), right);
    auto* codesForm = new QFormLayout(codes);
    // One field a control, in the order the controls are declared in: the
    // fields are read back by that order (typedLineworkCodes). The words a
    // person reads each by are linework_labels.hpp's, the one list File >
    // Settings shows them under as well; the object name is "linework" and
    // the control's name capitalised (arcStart: lineworkArcStart).
    for (const katana::entity::LineworkCodeMember& member : katana::entity::lineworkCodeMembers()) {
        QString name =
            QString::fromLatin1(member.name.data(), static_cast<qsizetype>(member.name.size()));
        name[0] = name[0].toUpper();
        auto* field = new QLineEdit(codes);
        field->setObjectName(QStringLiteral("linework") + name);
        field->setToolTip(tr("Typed after the code, matched ignoring case; empty switches it off"));
        codesForm->addRow(lineworkControlLabel(member.name), field);
        lineworkCodeFields_.push_back(field);
    }
    lineworkCodesStatus_ = new QLabel(codes);
    lineworkCodesStatus_->setObjectName(QStringLiteral("lineworkCodesStatus"));
    lineworkCodesStatus_->setWordWrap(true);
    codesForm->addRow(lineworkCodesStatus_);
    auto* codeButtons = new QHBoxLayout;
    lineworkCodesUse_ = new QPushButton(tr("Use These Codes"), codes);
    lineworkCodesUse_->setObjectName(QStringLiteral("lineworkCodesUse"));
    lineworkCodesUse_->setToolTip(tr("Make these the customisation's control codes (the line "
                                     "CUSTOMISE SET linework...): every import and every "
                                     "LINEWORK then reads them"));
    auto* defaults = new QPushButton(tr("Defaults"), codes);
    defaults->setObjectName(QStringLiteral("lineworkCodesDefaults"));
    defaults->setToolTip(tr("Fill in the usual spellings; Use These Codes then sets them"));
    codeButtons->addWidget(lineworkCodesUse_);
    codeButtons->addWidget(defaults);
    codesForm->addRow(codeButtons);
    top->addWidget(codes);

    auto* options = new QGroupBox(tr("Options"), right);
    auto* optionsForm = new QFormLayout(options);
    lineworkProperty_ = propertyChooser(options, "lineworkProperty");
    optionsForm->addRow(tr("Code property"), lineworkProperty_);
    lineworkOrder_ = new QComboBox(options);
    lineworkOrder_->setObjectName(QStringLiteral("lineworkOrder"));
    lineworkOrder_->addItems({tr("By point number"), tr("In the order observed")});
    optionsForm->addRow(tr("Join in order"), lineworkOrder_);
    lineworkChord_ = new QDoubleSpinBox(options);
    lineworkChord_->setObjectName(QStringLiteral("lineworkChordTolerance"));
    lineworkChord_->setSuffix(tr(" mm"));
    lineworkChord_->setDecimals(1);
    lineworkChord_->setRange(0.1, 1000.0);
    lineworkChord_->setValue(defaultChordMillimetres());
    lineworkChord_->setToolTip(tr("How far a curve's chords may stray from the arc, for a "
                                  "drawing in metres"));
    optionsForm->addRow(tr("Chord tolerance"), lineworkChord_);
    top->addWidget(options, 1);
    layout->addLayout(top);

    lineworkDirtyNote_ = new QLabel(tr("The rules being edited are not applied yet: lines are "
                                       "made with the drawing's survey map."),
                                    right);
    lineworkDirtyNote_->setObjectName(QStringLiteral("lineworkDirtyNote"));
    lineworkDirtyNote_->setStyleSheet(
        QStringLiteral("color: %1;").arg(kUndefinedNameColour.name()));
    layout->addWidget(lineworkDirtyNote_);

    lineworkCommand_ = commandField(right, "lineworkCommand");
    layout->addWidget(lineworkCommand_);

    auto* buttons = new QHBoxLayout;
    auto* preview = new QPushButton(tr("Preview"), right);
    preview->setObjectName(QStringLiteral("lineworkPreview"));
    preview->setToolTip(tr("Work out the lines, changing nothing"));
    auto* execute = new QPushButton(tr("Execute"), right);
    execute->setObjectName(QStringLiteral("lineworkExecute"));
    execute->setToolTip(tr("Run the line above: the lines drawn as one undoable step"));
    buttons->addWidget(preview);
    buttons->addWidget(execute);
    lineworkSummary_ = new QLabel(right);
    lineworkSummary_->setObjectName(QStringLiteral("lineworkSummary"));
    lineworkSummary_->setWordWrap(true);
    buttons->addWidget(lineworkSummary_, 1);
    layout->addLayout(buttons);

    auto* report = new QTabWidget(right);
    report->setObjectName(QStringLiteral("lineworkReport"));
    lineworkStrings_ = reportTree(report, "lineworkStrings",
                                  {tr("String"), tr("Key"), tr("Points"), tr("Closed"),
                                   tr("Curves"), tr("Layer")});
    lineworkUnplaced_ =
        reportTree(report, "lineworkUnplaced", {tr("Point"), tr("Code"), tr("Why not placed")});
    lineworkNotes_ = reportTree(report, "lineworkNotes", {tr("Point"), tr("Note"), tr("Detail")});
    report->addTab(lineworkStrings_, tr("Lines"));
    report->addTab(lineworkUnplaced_, tr("Points not placed"));
    report->addTab(lineworkNotes_, tr("Notes"));
    layout->addWidget(report, 1);

    for (QLineEdit* field : lineworkCodeFields_) {
        connect(field, &QLineEdit::textChanged, this, [this] {
            const auto valid = katana::cad::validate(typedLineworkCodes());
            lineworkCodesStatus_->setText(valid ? tr("Valid.") : text(valid.error().describe()));
            lineworkCodesStatus_->setStyleSheet(valid ? QString()
                                                      : QStringLiteral("color: #d9534f;"));
            lineworkCodesUse_->setEnabled(valid.ok());
        });
    }
    connect(lineworkCodesUse_, &QPushButton::clicked, this, [this] { useLineworkCodes(); });
    connect(defaults, &QPushButton::clicked, this, [this] {
        const katana::cad::LineworkCodes standard;
        const auto members = katana::entity::lineworkCodeMembers();
        for (std::size_t i = 0; i < members.size() && i < lineworkCodeFields_.size(); ++i) {
            lineworkCodeFields_[i]->setText(text(standard.*members[i].spelling));
        }
    });
    connect(preview, &QPushButton::clicked, this, [this] { previewLinework(); });
    connect(execute, &QPushButton::clicked, this, [this] { executeLinework(); });
    connect(lineworkOrder_, &QComboBox::currentIndexChanged, this, [this] { showLines(); });
    connect(lineworkProperty_, &QComboBox::currentTextChanged, this, [this] { showLines(); });
    connect(lineworkChord_, &QDoubleSpinBox::valueChanged, this, [this] { showLines(); });
    return page;
}

void SurveyCodeManagerDialog::loadLineworkCodes()
{
    // The customisation's, which are the Document's; the usual spellings for
    // a manager whose drawing has gone.
    const katana::cad::Document* doc = document();
    shownLinework_ = doc != nullptr ? doc->customisationState().linework
                                    : katana::cad::LineworkCodes{};
    const auto members = katana::entity::lineworkCodeMembers();
    for (std::size_t i = 0; i < members.size() && i < lineworkCodeFields_.size(); ++i) {
        lineworkCodeFields_[i]->setText(text(shownLinework_.*members[i].spelling));
    }
}

katana::cad::LineworkCodes SurveyCodeManagerDialog::typedLineworkCodes() const
{
    // The fields stand in the order the controls are declared in
    // (entity::lineworkCodeMembers), which is the order they were built in.
    katana::cad::LineworkCodes typed;
    const auto members = katana::entity::lineworkCodeMembers();
    for (std::size_t i = 0; i < members.size() && i < lineworkCodeFields_.size(); ++i) {
        typed.*members[i].spelling =
            std::string(katana::core::trimmed(text(lineworkCodeFields_[i]->text())));
    }
    return typed;
}

katana::core::Result<QString> SurveyCodeManagerDialog::lineworkCodesLine() const
{
    const katana::cad::Document* doc = document();
    if (doc == nullptr) {
        return katana::core::makeError(katana::core::ErrorCode::InvalidState,
                                       "the drawing this manager edits is closed");
    }
    const katana::cad::LineworkCodes typed = typedLineworkCodes();
    if (const auto valid = katana::cad::validate(typed); !valid) {
        return valid.error();
    }
    const katana::cad::LineworkCodes& held = doc->customisationState().linework;
    QString line;
    for (const katana::entity::LineworkCodeMember& member : katana::entity::lineworkCodeMembers()) {
        const std::string& spelling = typed.*member.spelling;
        if (spelling == held.*member.spelling) {
            continue; // only what changes: the line then says what was changed
        }
        // A blank is refused above; a double quote cannot be written on a
        // line at all, and SET would read the word cut short at it.
        if (spelling.find('"') != std::string::npos) {
            return katana::core::makeError(
                katana::core::ErrorCode::InvalidArgument,
                "a control code cannot hold a double quote: a command line has no way to say one",
                spelling);
        }
        // An empty spelling is "linework.start=": it switches the control off.
        line += QStringLiteral(" linework.%1=%2")
                    .arg(text(katana::core::lowered(member.name)), text(spelling));
    }
    return line.isEmpty() ? QString() : QStringLiteral("CUSTOMISE SET") + line;
}

void SurveyCodeManagerDialog::useLineworkCodes()
{
    const auto line = lineworkCodesLine();
    if (!line) {
        lineworkCodesStatus_->setText(text(line.error().describe()));
        log(text(line.error().describe()), true);
        return;
    }
    if (line->isEmpty()) {
        lineworkCodesStatus_->setText(tr("These are the customisation's control codes already."));
        return;
    }
    VerbOutcome outcome;
    QString said;
    if (!runLine(*line, outcome, said)) {
        lineworkCodesStatus_->setText(said);
        if (!context_.run) {
            log(said, true);
        }
        return;
    }
    // The Document holds them now, and the fields are its: taken at once,
    // rather than when the watcher next delivers, so the tab never shows
    // codes between the two.
    loadLineworkCodes();
    lineworkCodesStatus_->setText(tr("Set: these are the customisation's control codes now."));
}

katana::cad::LineworkWords SurveyCodeManagerDialog::lineworkWords() const
{
    katana::cad::LineworkWords words;
    words.property = std::string(katana::core::trimmed(text(lineworkProperty_->currentText())));
    words.order = lineworkOrder_->currentIndex() == 1 ? katana::cad::LineworkOrder::EntityOrder
                                                      : katana::cad::LineworkOrder::PointNumber;
    // Millimetres on the form, the drawing's units - metres - on the line.
    // Said only when it is not the length the verb starts on, to a tenth of a
    // millimetre, which is as fine as the box is set.
    if (std::abs(lineworkChord_->value() - defaultChordMillimetres()) >= 0.05) {
        words.chord = lineworkChord_->value() / 1000.0;
    }
    return words;
}

katana::core::Result<QString> SurveyCodeManagerDialog::lineworkLine() const
{
    auto scope = lineworkScope_->verbWords();
    if (!scope) {
        return scope.error();
    }
    const katana::cad::LineworkWords words = lineworkWords();
    QString line = QStringLiteral("LINEWORK ") + *scope;
    if (!words.property.empty()) {
        auto word = commandWord(text(words.property), tr("Code property"));
        if (!word) {
            return word.error();
        }
        line += QStringLiteral(" PROPERTY ") + *word;
    }
    if (words.order == katana::cad::LineworkOrder::EntityOrder) {
        line += QStringLiteral(" ORDER entity");
    }
    if (words.chord) {
        line += QStringLiteral(" CHORD ") + exactNumber(*words.chord);
    }
    return line;
}

bool SurveyCodeManagerDialog::previewLinework()
{
    lineworkStrings_->clear();
    lineworkUnplaced_->clear();
    lineworkNotes_->clear();
    const katana::cad::Document* doc = document();
    if (doc == nullptr) {
        lineworkSummary_->setText(tr("The drawing is closed."));
        return false;
    }
    const auto taken = scopeTakes(*lineworkScope_);
    if (!taken) {
        lineworkSummary_->setText(text(taken.error().describe()));
        log(text(taken.error().describe()), true);
        return false;
    }
    // The verb's own plan (cad::planLinework), so what is listed here is
    // what the line then draws: the customisation's control codes and
    // colours, no point removed, the points their survey job has strung left
    // out, and a line the drawing already holds not drawn again.
    const auto plan = katana::cad::planLinework(*doc, *taken, lineworkWords());
    if (!plan) {
        lineworkSummary_->setText(text(plan.error().describe()));
        log(text(plan.error().describe()), true);
        return false;
    }
    const katana::cad::LineworkReport& report = plan->planned.report;
    for (const katana::cad::LineworkString& line : report.strings) {
        auto* item = new QTreeWidgetItem(lineworkStrings_);
        item->setText(0, text(line.name) + (line.join ? tr(" (join)") : QString()));
        item->setText(1, text(line.key));
        item->setText(2, QString::number(line.pointNumbers.size()));
        item->setText(3, line.closed ? tr("closed") : QString());
        item->setText(4, line.curves == 0 ? QString() : QString::number(line.curves));
        item->setText(5, text(line.layer));
    }
    for (const katana::cad::UnplacedPoint& point : report.unplaced) {
        auto* item = new QTreeWidgetItem(lineworkUnplaced_);
        item->setText(0, text(point.pointNumber));
        item->setText(1, text(point.code));
        item->setText(2, text(std::string(katana::cad::toString(point.reason))));
    }
    for (const katana::cad::LineworkNote& note : report.notes) {
        auto* item = new QTreeWidgetItem(lineworkNotes_);
        item->setText(0, text(note.pointNumber));
        item->setText(1, text(std::string(katana::cad::toString(note.kind))));
        item->setText(2, text(note.detail));
    }
    for (QTreeWidget* tree : {lineworkStrings_, lineworkUnplaced_, lineworkNotes_}) {
        resizeColumns(tree);
    }
    // What the scope took first, as the line's own reply begins; then what
    // was left out, each only when there is something to say.
    QString summary = tr("%1 matched. ").arg(taken->size());
    summary += plan->planned.command == nullptr
                   ? tr("Nothing to build: %1 points looked at, %2 not placed.")
                         .arg(report.considered)
                         .arg(report.unplaced.size())
                   : tr("Preview only: %1 lines from %2 points; %3 points not placed; %4 notes. "
                        "Execute builds them as one undoable step.")
                         .arg(report.strings.size())
                         .arg(report.considered)
                         .arg(report.unplaced.size())
                         .arg(report.notes.size());
    if (!plan->strungByTheirJob.empty()) {
        summary += tr(" %1 points are left to the lines their survey job drew.")
                       .arg(plan->strungByTheirJob.size());
    }
    if (!report.alreadyDrawn.empty()) {
        summary += tr(" %1 lines are in the drawing already and are not drawn again.")
                       .arg(report.alreadyDrawn.size());
    }
    lineworkSummary_->setText(summary);
    return true;
}

void SurveyCodeManagerDialog::executeLinework()
{
    if (document() == nullptr) {
        return;
    }
    const auto line = lineworkLine();
    if (!line) {
        lineworkSummary_->setText(text(line.error().describe()));
        log(text(line.error().describe()), true);
        return;
    }
    // The lines about to be drawn, point by point: the reply counts them. A
    // preview that could not plan has said why; the line is not run to say it
    // twice.
    if (!previewLinework()) {
        return;
    }
    VerbOutcome outcome;
    QString said;
    if (!runLine(*line, outcome, said)) {
        lineworkSummary_->setText(said);
        if (!context_.run) {
            log(said, true);
        }
        return;
    }
    // From the verb's own first record - "linework scope=... lines=<n> ..." -
    // and never from the plan above alone: what is said to have been drawn is
    // what the line says it drew.
    const QString first = outcome.reply.section(QLatin1Char('\n'), 0, 0);
    const auto record = katana::core::readReplyRecord(text(first));
    const std::optional<std::string> lines = record ? record->value("lines") : std::nullopt;
    if (!lines) {
        lineworkSummary_->setText(tr("Executed. %1").arg(first));
    } else if (*lines == "0") {
        lineworkSummary_->setText(tr("Executed: no line was drawn, so nothing was added to the "
                                     "undo history. %1")
                                      .arg(first));
    } else {
        lineworkSummary_->setText(
            tr("Executed as one undoable step: %1 lines drawn. %2").arg(text(*lines), first));
    }
}

} // namespace katana::qt
