// The survey code manager's other four tabs: the codes a drawing carries,
// what is wrong with the rules being edited, applying the codes, and turning
// coded points into lines. Each shows a cad report; Preview never executes,
// and Execute runs the one command a preview planned - one undo step.

#include <algorithm>
#include <utility>

#include <QButtonGroup>
#include <QCheckBox>
#include <QComboBox>
#include <QCompleter>
#include <QDoubleSpinBox>
#include <QFormLayout>
#include <QGridLayout>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QRadioButton>
#include <QStyle>
#include <QTabWidget>
#include <QTreeWidget>
#include <QVBoxLayout>

#include "customisation/code_manager.hpp"
#include "customisation/filter_bar.hpp"
#include "customisation/name_picker.hpp"
#include "katana/archive12d/domain.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/tables.hpp"

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
                const std::string code = text(item->data(0, kCodeRole).toString());
                tabs_->setCurrentIndex(0);
                testCode_->setText(text(code));
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
                const QVariant rule = item->data(0, kRuleRole);
                if (rule.isValid()) {
                    selectRule(rule.toULongLong());
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
                        buffer_, doc->styleLibrary(),
                        [](std::string_view name) {
                            return katana::archive12d::standardColour(name);
                        },
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

// ---- apply codes -------------------------------------------------------------------------

QWidget* SurveyCodeManagerDialog::buildApplyTab()
{
    auto* page = new QWidget(this);
    page->setObjectName(QStringLiteral("applyCodesTab"));
    auto* layout = new QVBoxLayout(page);

    auto* options = new QGroupBox(tr("Apply the drawing's survey map to"), page);
    auto* grid = new QGridLayout(options);
    applySelection_ = new QRadioButton(tr("Selection"), options);
    applySelection_->setObjectName(QStringLiteral("applyScopeSelection"));
    applyAll_ = new QRadioButton(tr("Every coded entity"), options);
    applyAll_->setObjectName(QStringLiteral("applyScopeAll"));
    applyAll_->setChecked(true);
    auto* scope = new QButtonGroup(options);
    scope->addButton(applySelection_);
    scope->addButton(applyAll_);
    applyProperty_ = propertyChooser(options, "applyProperty");
    applyCreateLayers_ = new QCheckBox(tr("Create layers"), options);
    applyCreateLayers_->setObjectName(QStringLiteral("applyCreateLayers"));
    applyCreateStyles_ = new QCheckBox(tr("Create styles"), options);
    applyCreateStyles_->setObjectName(QStringLiteral("applyCreateStyles"));
    applySetAttributes_ = new QCheckBox(tr("Set attributes"), options);
    applySetAttributes_->setObjectName(QStringLiteral("applySetAttributes"));
    for (QCheckBox* box : {applyCreateLayers_, applyCreateStyles_, applySetAttributes_}) {
        box->setChecked(true);
    }
    grid->addWidget(applyAll_, 0, 0);
    grid->addWidget(applySelection_, 0, 1);
    grid->addWidget(new QLabel(tr("Code property"), options), 1, 0);
    grid->addWidget(applyProperty_, 1, 1, 1, 2);
    grid->addWidget(applyCreateLayers_, 2, 0);
    grid->addWidget(applyCreateStyles_, 2, 1);
    grid->addWidget(applySetAttributes_, 2, 2);
    layout->addWidget(options);

    applyDirtyNote_ = new QLabel(tr("The rules being edited are not applied yet: codes are "
                                    "applied with the drawing's survey map. Apply the edits "
                                    "first to use them."),
                                 page);
    applyDirtyNote_->setObjectName(QStringLiteral("applyDirtyNote"));
    applyDirtyNote_->setWordWrap(true);
    applyDirtyNote_->setStyleSheet(QStringLiteral("color: %1;").arg(kUndefinedNameColour.name()));
    layout->addWidget(applyDirtyNote_);

    auto* buttons = new QHBoxLayout;
    auto* preview = new QPushButton(tr("Preview"), page);
    preview->setObjectName(QStringLiteral("applyPreview"));
    preview->setToolTip(tr("Work out what applying the codes would do, changing nothing"));
    auto* execute = new QPushButton(tr("Execute"), page);
    execute->setObjectName(QStringLiteral("applyExecute"));
    execute->setToolTip(tr("Apply the codes as one undoable step"));
    buttons->addWidget(preview);
    buttons->addWidget(execute);
    buttons->addStretch(1);
    layout->addLayout(buttons);

    applyRows_ = reportTree(page, "applyRows",
                            {tr("Code"), tr("Entities"), tr("Class"), tr("Layer"), tr("Style"),
                             tr("Changed")});
    layout->addWidget(applyRows_, 2);
    applyReport_ = new QPlainTextEdit(page);
    applyReport_->setObjectName(QStringLiteral("applyReport"));
    applyReport_->setReadOnly(true);
    applyReport_->setLineWrapMode(QPlainTextEdit::NoWrap);
    layout->addWidget(applyReport_, 1);

    connect(preview, &QPushButton::clicked, this, [this] { previewCodes(); });
    connect(execute, &QPushButton::clicked, this, [this] { executeCodes(); });
    // A changed option makes the preview a plan for something else.
    for (QAbstractButton* button : std::initializer_list<QAbstractButton*>{
             applySelection_, applyAll_, applyCreateLayers_, applyCreateStyles_,
             applySetAttributes_}) {
        connect(button, &QAbstractButton::toggled, this, [this] { invalidatePlans(); });
    }
    connect(applyProperty_, &QComboBox::currentTextChanged, this, [this] { invalidatePlans(); });
    return page;
}

katana::cad::SurveyCodingOptions SurveyCodeManagerDialog::codingOptions() const
{
    katana::cad::SurveyCodingOptions options;
    options.property = std::string(katana::core::trimmed(text(applyProperty_->currentText())));
    const katana::cad::Document* doc = document();
    if (applySelection_->isChecked() && doc != nullptr) {
        options.ids = doc->selection().ids();
    }
    options.colourOf = [](std::string_view name) {
        return katana::archive12d::standardColour(name);
    };
    options.createLayers = applyCreateLayers_->isChecked();
    options.createStyles = applyCreateStyles_->isChecked();
    options.setAttributes = applySetAttributes_->isChecked();
    return options;
}

SurveyCodeManagerDialog::PlanStamp SurveyCodeManagerDialog::stamp() const
{
    PlanStamp mark;
    if (const katana::cad::Document* doc = document()) {
        mark.undo = doc->history().undoCount();
        mark.redo = doc->history().redoCount();
        mark.entities = doc->model().entities.size();
        mark.library = doc->libraryGeneration();
        mark.surveyMap = doc->surveyMapGeneration();
        mark.selection = doc->selection().ids();
    }
    return mark;
}

void SurveyCodeManagerDialog::invalidatePlans()
{
    plannedCodes_.reset();
    codesStamp_.reset();
    plannedLinework_.reset();
    lineworkStamp_.reset();
}

void SurveyCodeManagerDialog::previewCodes()
{
    plannedCodes_.reset();
    codesStamp_.reset();
    applyRows_->clear();
    const katana::cad::Document* doc = document();
    if (doc == nullptr) {
        applyReport_->setPlainText(tr("The drawing is closed."));
        return;
    }
    const katana::cad::SurveyCodingOptions options = codingOptions();
    if (applySelection_->isChecked() && options.ids.empty()) {
        applyReport_->setPlainText(tr("Nothing is selected."));
        return;
    }
    katana::cad::SurveyCodingReport report;
    auto command = katana::cad::applySurveyCodes(*doc, options, &report);
    if (!command) {
        applyReport_->setPlainText(text(command.error().describe()));
        log(text(command.error().describe()), true);
        return;
    }
    plannedCodes_ = std::move(*command);
    codesStamp_ = stamp();
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
                         : row.layerKept == row.entities
                             ? tr("%1 (kept: layer not created)").arg(from.join(QStringLiteral(", ")))
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
    QString shown = text(katana::cad::formatCodingReport(report));
    shown += plannedCodes_ == nullptr ? tr("\nNothing to change.")
                                      : tr("\nPreview only: nothing has changed. Execute applies "
                                           "this as one undoable step.");
    applyReport_->setPlainText(shown);
}

void SurveyCodeManagerDialog::executeCodes()
{
    katana::cad::Document* doc = document();
    if (doc == nullptr) {
        return;
    }
    // A plan made against another state of the drawing is yesterday's
    // answer: plan again, and show what is being done.
    if (plannedCodes_ == nullptr || !codesStamp_ || !(*codesStamp_ == stamp())) {
        previewCodes();
    }
    if (plannedCodes_ == nullptr) {
        log(tr("Apply Codes: nothing to change."));
        return;
    }
    katana::commands::CommandPtr command = std::move(plannedCodes_);
    invalidatePlans();
    if (const auto status = doc->execute(std::move(command)); !status) {
        applyReport_->appendPlainText(text(status.error().describe()));
        log(text(status.error().describe()), true);
        return;
    }
    applyReport_->appendPlainText(tr("Executed as one undoable step."));
    log(tr("Survey codes applied. Undo puts it all back."));
}

// ---- linework ----------------------------------------------------------------------------

QWidget* SurveyCodeManagerDialog::buildLineworkTab()
{
    auto* page = new QWidget(this);
    page->setObjectName(QStringLiteral("lineworkTab"));
    auto* layout = new QVBoxLayout(page);
    auto* top = new QHBoxLayout;

    auto* codes = new QGroupBox(tr("Control codes"), page);
    auto* codesForm = new QFormLayout(codes);
    const std::array<std::pair<const char*, QString>, 7> fields{{
        {"lineworkStart", tr("Start")},
        {"lineworkEnd", tr("End")},
        {"lineworkClose", tr("Close")},
        {"lineworkArcStart", tr("Begin curve")},
        {"lineworkArcEnd", tr("End curve")},
        {"lineworkJoin", tr("Join to point")},
        {"lineworkRectangle", tr("Rectangle")},
    }};
    for (const auto& [name, label] : fields) {
        auto* field = new QLineEdit(codes);
        field->setObjectName(QString::fromLatin1(name));
        field->setToolTip(tr("Typed after the code, matched ignoring case; empty switches it off"));
        codesForm->addRow(label, field);
        lineworkCodeFields_.push_back(field);
    }
    lineworkCodesStatus_ = new QLabel(codes);
    lineworkCodesStatus_->setObjectName(QStringLiteral("lineworkCodesStatus"));
    lineworkCodesStatus_->setWordWrap(true);
    codesForm->addRow(lineworkCodesStatus_);
    auto* codeButtons = new QHBoxLayout;
    lineworkCodesUse_ = new QPushButton(tr("Use These Codes"), codes);
    lineworkCodesUse_->setObjectName(QStringLiteral("lineworkCodesUse"));
    auto* defaults = new QPushButton(tr("Defaults"), codes);
    defaults->setObjectName(QStringLiteral("lineworkCodesDefaults"));
    codeButtons->addWidget(lineworkCodesUse_);
    codeButtons->addWidget(defaults);
    codesForm->addRow(codeButtons);
    top->addWidget(codes);

    auto* options = new QGroupBox(tr("Options"), page);
    auto* optionsForm = new QFormLayout(options);
    auto* scopeRow = new QHBoxLayout;
    auto* all = new QRadioButton(tr("Every point"), options);
    all->setObjectName(QStringLiteral("lineworkScopeAll"));
    all->setChecked(true);
    lineworkSelection_ = new QRadioButton(tr("Selection"), options);
    lineworkSelection_->setObjectName(QStringLiteral("lineworkScopeSelection"));
    auto* scope = new QButtonGroup(options);
    scope->addButton(all);
    scope->addButton(lineworkSelection_);
    scopeRow->addWidget(all);
    scopeRow->addWidget(lineworkSelection_);
    optionsForm->addRow(tr("Points"), scopeRow);
    lineworkProperty_ = propertyChooser(options, "lineworkProperty");
    optionsForm->addRow(tr("Code property"), lineworkProperty_);
    lineworkOrder_ = new QComboBox(options);
    lineworkOrder_->setObjectName(QStringLiteral("lineworkOrder"));
    lineworkOrder_->addItems({tr("By point number"), tr("In the order observed")});
    optionsForm->addRow(tr("Join in order"), lineworkOrder_);
    lineworkKeepPoints_ = new QCheckBox(tr("Keep the points a line replaces"), options);
    lineworkKeepPoints_->setObjectName(QStringLiteral("lineworkKeepPoints"));
    lineworkKeepPoints_->setChecked(true);
    optionsForm->addRow(QString(), lineworkKeepPoints_);
    lineworkChord_ = new QDoubleSpinBox(options);
    lineworkChord_->setObjectName(QStringLiteral("lineworkChordTolerance"));
    lineworkChord_->setSuffix(tr(" mm"));
    lineworkChord_->setDecimals(1);
    lineworkChord_->setRange(0.1, 1000.0);
    lineworkChord_->setValue(5.0);
    lineworkChord_->setToolTip(tr("How far a curve's chords may stray from the arc"));
    optionsForm->addRow(tr("Chord tolerance"), lineworkChord_);
    top->addWidget(options, 1);
    layout->addLayout(top);

    lineworkDirtyNote_ = new QLabel(tr("The rules being edited are not applied yet: lines are "
                                       "made with the drawing's survey map."),
                                    page);
    lineworkDirtyNote_->setObjectName(QStringLiteral("lineworkDirtyNote"));
    lineworkDirtyNote_->setStyleSheet(
        QStringLiteral("color: %1;").arg(kUndefinedNameColour.name()));
    layout->addWidget(lineworkDirtyNote_);

    auto* buttons = new QHBoxLayout;
    auto* preview = new QPushButton(tr("Preview"), page);
    preview->setObjectName(QStringLiteral("lineworkPreview"));
    auto* execute = new QPushButton(tr("Execute"), page);
    execute->setObjectName(QStringLiteral("lineworkExecute"));
    buttons->addWidget(preview);
    buttons->addWidget(execute);
    lineworkSummary_ = new QLabel(page);
    lineworkSummary_->setObjectName(QStringLiteral("lineworkSummary"));
    lineworkSummary_->setWordWrap(true);
    buttons->addWidget(lineworkSummary_, 1);
    layout->addLayout(buttons);

    auto* report = new QTabWidget(page);
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
            katana::cad::LineworkCodes typed;
            std::array<std::string*, 7> targets{&typed.start,    &typed.end,  &typed.close,
                                                &typed.arcStart, &typed.arcEnd, &typed.join,
                                                &typed.rectangle};
            for (std::size_t i = 0; i < targets.size(); ++i) {
                *targets[i] = std::string(
                    katana::core::trimmed(text(lineworkCodeFields_[i]->text())));
            }
            const auto valid = katana::cad::validate(typed);
            lineworkCodesStatus_->setText(valid ? tr("Valid.") : text(valid.error().describe()));
            lineworkCodesStatus_->setStyleSheet(valid ? QString()
                                                      : QStringLiteral("color: #d9534f;"));
            lineworkCodesUse_->setEnabled(valid.ok());
        });
    }
    connect(lineworkCodesUse_, &QPushButton::clicked, this, [this] { useLineworkCodes(); });
    connect(defaults, &QPushButton::clicked, this, [this] {
        const katana::cad::LineworkCodes standard;
        const std::array<const std::string*, 7> values{&standard.start,    &standard.end,
                                                       &standard.close,    &standard.arcStart,
                                                       &standard.arcEnd,   &standard.join,
                                                       &standard.rectangle};
        for (std::size_t i = 0; i < values.size(); ++i) {
            lineworkCodeFields_[i]->setText(text(*values[i]));
        }
    });
    connect(preview, &QPushButton::clicked, this, [this] { previewLinework(); });
    connect(execute, &QPushButton::clicked, this, [this] { executeLinework(); });
    for (QAbstractButton* button :
         std::initializer_list<QAbstractButton*>{all, lineworkSelection_, lineworkKeepPoints_}) {
        connect(button, &QAbstractButton::toggled, this, [this] { invalidatePlans(); });
    }
    connect(lineworkOrder_, &QComboBox::currentIndexChanged, this, [this] { invalidatePlans(); });
    connect(lineworkProperty_, &QComboBox::currentTextChanged, this,
            [this] { invalidatePlans(); });
    connect(lineworkChord_, &QDoubleSpinBox::valueChanged, this, [this] { invalidatePlans(); });
    return page;
}

void SurveyCodeManagerDialog::loadLineworkCodes()
{
    const katana::cad::LineworkCodes& codes = lineworkCodes();
    const std::array<const std::string*, 7> values{&codes.start,  &codes.end,    &codes.close,
                                                   &codes.arcStart, &codes.arcEnd, &codes.join,
                                                   &codes.rectangle};
    for (std::size_t i = 0; i < values.size(); ++i) {
        lineworkCodeFields_[i]->setText(text(*values[i]));
    }
}

void SurveyCodeManagerDialog::useLineworkCodes()
{
    katana::cad::LineworkCodes typed;
    std::array<std::string*, 7> targets{&typed.start,  &typed.end,    &typed.close,
                                        &typed.arcStart, &typed.arcEnd, &typed.join,
                                        &typed.rectangle};
    for (std::size_t i = 0; i < targets.size(); ++i) {
        *targets[i] = std::string(katana::core::trimmed(text(lineworkCodeFields_[i]->text())));
    }
    if (const auto valid = katana::cad::validate(typed); !valid) {
        log(text(valid.error().describe()), true);
        return;
    }
    (context_.lineworkCodes != nullptr ? *context_.lineworkCodes : localCodes_) = typed;
    invalidatePlans();
    log(context_.lineworkCodes != nullptr
            ? tr("Linework control codes set for this session.")
            : tr("Linework control codes set for this manager (the session keeps none)."));
}

katana::cad::LineworkOptions SurveyCodeManagerDialog::lineworkOptions() const
{
    katana::cad::LineworkOptions options;
    options.property = std::string(katana::core::trimmed(text(lineworkProperty_->currentText())));
    const katana::cad::Document* doc = document();
    if (lineworkSelection_->isChecked() && doc != nullptr) {
        options.ids = doc->selection().ids();
    }
    options.order = lineworkOrder_->currentIndex() == 1 ? katana::cad::LineworkOrder::EntityOrder
                                                        : katana::cad::LineworkOrder::PointNumber;
    options.keepPoints = lineworkKeepPoints_->isChecked();
    options.codes = lineworkCodes();
    options.chordTolerance = lineworkChord_->value() / 1000.0; // mm on the form, metres here
    options.coding.colourOf = [](std::string_view name) {
        return katana::archive12d::standardColour(name);
    };
    return options;
}

void SurveyCodeManagerDialog::previewLinework()
{
    plannedLinework_.reset();
    lineworkStamp_.reset();
    lineworkStrings_->clear();
    lineworkUnplaced_->clear();
    lineworkNotes_->clear();
    const katana::cad::Document* doc = document();
    if (doc == nullptr) {
        lineworkSummary_->setText(tr("The drawing is closed."));
        return;
    }
    const katana::cad::LineworkOptions options = lineworkOptions();
    if (lineworkSelection_->isChecked() && options.ids.empty()) {
        lineworkSummary_->setText(tr("Nothing is selected."));
        return;
    }
    auto result = katana::cad::processLinework(*doc, options);
    if (!result) {
        lineworkSummary_->setText(text(result.error().describe()));
        log(text(result.error().describe()), true);
        return;
    }
    plannedLinework_ = std::move(result->command);
    lineworkStamp_ = stamp();
    const katana::cad::LineworkReport& report = result->report;
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
    lineworkSummary_->setText(
        plannedLinework_ == nullptr
            ? tr("Nothing to build: %1 points looked at, %2 not placed.")
                  .arg(report.considered)
                  .arg(report.unplaced.size())
            : tr("Preview only: %1 lines from %2 points; %3 points not placed; %4 notes. "
                 "Execute builds them as one undoable step.")
                  .arg(report.strings.size())
                  .arg(report.considered)
                  .arg(report.unplaced.size())
                  .arg(report.notes.size()));
}

void SurveyCodeManagerDialog::executeLinework()
{
    katana::cad::Document* doc = document();
    if (doc == nullptr) {
        return;
    }
    if (plannedLinework_ == nullptr || !lineworkStamp_ || !(*lineworkStamp_ == stamp())) {
        previewLinework();
    }
    if (plannedLinework_ == nullptr) {
        log(tr("Linework: nothing to build."));
        return;
    }
    katana::commands::CommandPtr command = std::move(plannedLinework_);
    invalidatePlans();
    if (const auto status = doc->execute(std::move(command)); !status) {
        lineworkSummary_->setText(text(status.error().describe()));
        log(text(status.error().describe()), true);
        return;
    }
    lineworkSummary_->setText(lineworkSummary_->text() + tr(" Executed as one undoable step."));
    log(tr("Linework built. Undo puts it all back."));
}

} // namespace katana::qt
