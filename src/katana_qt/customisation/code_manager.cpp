#include "code_manager.hpp"

#include <algorithm>
#include <array>
#include <map>
#include <set>
#include <span>
#include <utility>

#include <QCheckBox>
#include <QComboBox>
#include <QFileDialog>
#include <QFormLayout>
#include <QGuiApplication>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QMessageBox>
#include <QPainter>
#include <QPixmap>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QRadioButton>
#include <QScrollArea>
#include <QSplitter>
#include <QStackedWidget>
#include <QTabWidget>
#include <QTimer>
#include <QTreeWidget>
#include <QVBoxLayout>

#include "customisation/code_manager_support.hpp"
#include "customisation/definition_thumbnails.hpp"
#include "customisation/document_watcher.hpp"
#include "customisation/filter_bar.hpp"
#include "customisation/name_picker.hpp"
#include "customisation/style_preview.hpp"
#include "katana/cad/code_edit.hpp"
#include "katana/cad/colour_lookup.hpp"
#include "katana/cad/customisation_host.hpp"
#include "katana/cad/customisation_part.hpp"
#include "katana/core/path_text.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/customisation.hpp"
#include "katana/entity/tables.hpp"

namespace katana::qt {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::entity::SurveyBreakline;
using katana::entity::SurveyMatchKind;
using katana::entity::SurveyPipe;
using katana::entity::SurveyRule;
using katana::entity::SurveySection;
using katana::entity::SurveySymbol;
using katana::entity::SurveyTextStyle;

namespace {

// The Code Table's section chips, and the sections each stands for: the
// three pipe sections are one subject to a person, and so are the two
// attribute sections. `named` is the label a chip's objectName was made from
// when it was first shown ("filterMap", "filterTinable"): tests and headless
// scripts find a chip by that name, so it outlives the label. What a chip
// SAYS is sectionChipLabel's, and is written nowhere here.
struct SectionChip {
    const char* named;
    std::vector<SurveySection> sections;
};

const std::vector<SectionChip>& sectionChips()
{
    static const std::vector<SectionChip> chips{
        {"All", {}},
        {"Map", {SurveySection::Map}},
        {"Symbol", {SurveySection::VertexSymbol}},
        {"Text", {SurveySection::VertexTextStyle}},
        {"Pipe", {SurveySection::Pipe, SurveySection::VertexPipe, SurveySection::SegmentPipe}},
        {"Attributes", {SurveySection::StringAttribute, SurveySection::VertexAttribute}},
        {"Tinable", {SurveySection::Tinable}},
    };
    return chips;
}

// What a chip says: the word of the first section it stands for, from
// entity::toString - the one table the Section list and a customisation
// file's "sets" take theirs from - with a capital, as a chip is written. A
// copy typed here would go on saying the old word when that table changed.
// Not translated either: it is the word a file holds, in every language.
// Empty for the chip that stands for no section, which the caller names.
[[nodiscard]] QString sectionChipLabel(const SectionChip& chip)
{
    if (chip.sections.empty()) {
        return {};
    }
    QString word = QString::fromLatin1(katana::entity::toString(chip.sections.front()));
    if (!word.isEmpty()) {
        word[0] = word[0].toUpper();
    }
    return word;
}

// Tree columns.
enum Column {
    KeyColumn,
    DescriptionColumn,
    LayerColumn,
    ColourColumn,
    BreaklineColumn,
    LinestyleColumn,
    SymbolColumn,
    TinableColumn,
    AttributesColumn,
    ColumnCount
};

constexpr int kKeyRole = Qt::UserRole + 1;  // QString: the key of a key row
constexpr int kRuleRole = Qt::UserRole + 2; // qulonglong: the index of a rule row

const QSize kThumbnailSize{48, 16};

[[nodiscard]] QString text(const std::string& value)
{
    return QString::fromStdString(value);
}

[[nodiscard]] std::string text(const QString& value)
{
    return value.toStdString();
}

[[nodiscard]] QString real(double value)
{
    return text(katana::core::formatExactReal(value));
}

[[nodiscard]] QString breaklineText(const std::optional<SurveyBreakline>& breakline)
{
    if (!breakline) {
        return {};
    }
    return *breakline == SurveyBreakline::Line ? QStringLiteral("Line") : QStringLiteral("Point");
}

[[nodiscard]] QString yesNo(const std::optional<bool>& value)
{
    if (!value) {
        return {};
    }
    return *value ? QStringLiteral("yes") : QStringLiteral("no");
}

[[nodiscard]] QString attributeNames(const SurveyRule& rule)
{
    QStringList names;
    for (const auto* list : {&rule.attributes, &rule.vertexAttributes, &rule.segmentAttributes}) {
        for (const auto& attribute : *list) {
            names << text(attribute.name);
        }
    }
    return names.join(QStringLiteral(", "));
}

[[nodiscard]] QString ruleLabel(std::size_t index, const SurveyRule& rule)
{
    return QStringLiteral("#%1 %2 (%3)")
        .arg(index)
        .arg(text(rule.key), QString::fromLatin1(katana::entity::toString(rule.section)));
}

[[nodiscard]] QString matchClass(SurveyMatchKind kind, bool matched)
{
    if (matched) {
        return QStringLiteral("matched");
    }
    return kind == SurveyMatchKind::FallbackOnly ? QStringLiteral("fallback only")
                                                 : QStringLiteral("unmatched");
}

// A file's name as a person reads it, for the log: never its folder.
[[nodiscard]] QString fileName(const std::filesystem::path& path)
{
    return text(katana::core::pathToUtf8(path.filename()));
}

} // namespace

// ---- construction ----------------------------------------------------------------

SurveyCodeManagerDialog::SurveyCodeManagerDialog(CustomisationContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName(QStringLiteral("surveyCodeManagerDialog"));
    setWindowTitle(tr("Survey Code Manager"));
    setModal(false);
    const QString platform = QGuiApplication::platformName();
    interactive_ = platform != QLatin1String("offscreen") && platform != QLatin1String("minimal");
    if (context_.document != nullptr) {
        buffer_ = context_.document->surveyMap();
        baseline_ = buffer_;
    }

    auto* layout = new QVBoxLayout(this);

    // Files: what comes into the buffer and what goes out of it.
    auto* files = new QHBoxLayout;
    auto* importButton = new QPushButton(tr("Import Codes..."), this);
    importButton->setObjectName(QStringLiteral("importCodes"));
    importButton->setToolTip(tr("Read a Katana customisation file and merge its survey code "
                                "rules into the rules being edited, for review before Apply"));
    importReplace_ = new QCheckBox(tr("Replace instead of merging"), this);
    importReplace_->setObjectName(QStringLiteral("importReplace"));
    auto* exportButton = new QPushButton(tr("Export Codes..."), this);
    exportButton->setObjectName(QStringLiteral("exportCodes"));
    exportButton->setToolTip(tr("Write the rules being edited to a Katana customisation file "
                                "that holds the survey codes alone"));
    auto* csvButton = new QPushButton(tr("Export Code List CSV..."), this);
    csvButton->setObjectName(QStringLiteral("exportCodeList"));
    files->addWidget(importButton);
    files->addWidget(importReplace_);
    files->addStretch(1);
    files->addWidget(exportButton);
    files->addWidget(csvButton);
    layout->addLayout(files);

    tabs_ = new QTabWidget(this);
    tabs_->setObjectName(QStringLiteral("codeManagerTabs"));
    tabs_->addTab(buildCodeTableTab(), tr("Code Table"));
    tabs_->addTab(buildCensusTab(), tr("Codes in Drawing"));
    tabs_->addTab(buildIssuesTab(), tr("Issues"));
    tabs_->addTab(buildApplyTab(), tr("Apply Codes"));
    tabs_->addTab(buildLineworkTab(), tr("Linework"));
    layout->addWidget(tabs_, 1);

    // The buffer's state, and the two ways out of it.
    auto* commit = new QHBoxLayout;
    dirtyLabel_ = new QLabel(this);
    dirtyLabel_->setObjectName(QStringLiteral("dirtyIndicator"));
    applyButton_ = new QPushButton(tr("Apply"), this);
    applyButton_->setObjectName(QStringLiteral("applyMap"));
    applyButton_->setToolTip(tr("Make the edited rules the drawing's survey map"));
    revertButton_ = new QPushButton(tr("Revert"), this);
    revertButton_->setObjectName(QStringLiteral("revertMap"));
    revertButton_->setToolTip(tr("Discard the edits and take the drawing's survey map again"));
    auto* closeButton = new QPushButton(tr("Close"), this);
    closeButton->setObjectName(QStringLiteral("closeManager"));
    commit->addWidget(dirtyLabel_, 1);
    commit->addWidget(applyButton_);
    commit->addWidget(revertButton_);
    commit->addWidget(closeButton);
    layout->addLayout(commit);

    connect(importButton, &QPushButton::clicked, this, [this] {
        if (!interactive_) {
            log(tr("Import Codes needs a file chosen in an interactive session"), true);
            return;
        }
        const QString path = QFileDialog::getOpenFileName(
            this, tr("Import Survey Codes"), {},
            customisationFileFilter() + tr(";;All files (*)"));
        if (path.isEmpty()) {
            return;
        }
        const auto mode = importReplace_->isChecked() ? katana::cad::LoadMode::Replace
                                                      : katana::cad::LoadMode::Merge;
        if (const auto status = importCodes(path.toStdWString(), mode); !status) {
            log(text(status.error().describe()), true);
        }
    });
    connect(exportButton, &QPushButton::clicked, this, [this] {
        if (!interactive_) {
            log(tr("Export Codes needs a file chosen in an interactive session"), true);
            return;
        }
        const QString path = QFileDialog::getSaveFileName(this, tr("Export Survey Codes"), {},
                                                          customisationFileFilter());
        if (path.isEmpty()) {
            return;
        }
        if (const auto status = exportCodes(path.toStdWString()); !status) {
            log(text(status.error().describe()), true);
        }
    });
    connect(csvButton, &QPushButton::clicked, this, [this] {
        if (!interactive_) {
            log(tr("Export Code List needs a file chosen in an interactive session"), true);
            return;
        }
        const QString path = QFileDialog::getSaveFileName(this, tr("Export Code List"), {},
                                                          tr("CSV files (*.csv)"));
        if (path.isEmpty()) {
            return;
        }
        if (const auto status = exportCodeList(path.toStdWString()); !status) {
            log(text(status.error().describe()), true);
        }
    });
    connect(applyButton_, &QPushButton::clicked, this, [this] {
        if (const auto status = apply(); !status) {
            log(text(status.error().describe()), true);
        }
    });
    connect(revertButton_, &QPushButton::clicked, this, [this] { revert(); });
    connect(closeButton, &QPushButton::clicked, this, [this] { reject(); });

    // No button is a default: QDialog makes the first auto-default button
    // its default when shown, and Enter in any field - a typed code, a layer
    // name - would press it (Import Codes..., or Delete). Buttons here are
    // pressed, never defaulted to.
    for (QPushButton* button : findChildren<QPushButton*>()) {
        button->setAutoDefault(false);
        button->setDefault(false);
    }

    resize(1280, 780);

    if (context_.document != nullptr) {
        watcher_ = std::make_unique<DocumentWatcher>(
            *context_.document,
            [this](const DocumentChanges& changes) { documentChanged(changes); });
    }
    rebuildCodeTable();
    rebuildIssues();
    refreshCensus();
    loadLineworkCodes();
    if (context_.document != nullptr) {
        const QString count = QString::number(context_.document->selection().size());
        applySelection_->setText(tr("Selection (%1)").arg(count));
        lineworkSelection_->setText(tr("Selection (%1)").arg(count));
    }
    loadForm(SurveyRule{}, std::nullopt);
    updateDirty();
}

SurveyCodeManagerDialog::~SurveyCodeManagerDialog()
{
    // The children outlive this body (QWidget deletes them after the members
    // are gone), and a child dying can still signal - a tree clearing its
    // selection. Cut every connection into this dialog first, so none reaches
    // a lambda whose members no longer exist.
    watcher_.reset();
    for (QObject* child : findChildren<QObject*>()) {
        QObject::disconnect(child, nullptr, this, nullptr);
    }
}

katana::cad::Document* SurveyCodeManagerDialog::document() const
{
    return watcher_ != nullptr && watcher_->documentAlive() ? context_.document : nullptr;
}

void SurveyCodeManagerDialog::log(const QString& message, bool isError) const
{
    if (context_.log) {
        context_.log(message, isError);
    }
}

const katana::cad::LineworkCodes& SurveyCodeManagerDialog::lineworkCodes() const
{
    return context_.lineworkCodes != nullptr ? *context_.lineworkCodes : localCodes_;
}

// ---- the buffer --------------------------------------------------------------------

bool SurveyCodeManagerDialog::dirty() const
{
    return !(buffer_ == baseline_);
}

katana::core::Status SurveyCodeManagerDialog::apply()
{
    katana::cad::Document* doc = document();
    if (doc == nullptr) {
        return makeError(ErrorCode::InvalidState, "the drawing this manager edits is closed");
    }
    // Round the commit, the hook every editor of the customisation calls
    // (CustomisationContext::beginCommit; the definition editor's Save is
    // the model): before it, since setSurveyMap leaves the session "not
    // kept" and only the maker of the context can have noted that it was;
    // and what it hands back after it, which is where a session that was
    // kept is kept again. This manager knows neither.
    const std::function<void()> committed =
        context_.beginCommit ? context_.beginCommit() : std::function<void()>();
    doc->setSurveyMap(buffer_);
    if (committed) {
        committed();
    }
    baseline_ = buffer_;
    invalidatePlans();
    updateDirty();
    log(tr("Survey map applied: %1 rules over %2 codes.")
            .arg(buffer_.size())
            .arg(buffer_.keys().size()));
    return {};
}

void SurveyCodeManagerDialog::revert()
{
    katana::cad::Document* doc = document();
    if (doc == nullptr) {
        return;
    }
    buffer_ = doc->surveyMap();
    baseline_ = buffer_;
    if (formIndex_ && *formIndex_ >= buffer_.size()) {
        formIndex_.reset();
    }
    bufferChanged();
    if (formIndex_) {
        selectRule(*formIndex_);
    } else {
        loadForm(SurveyRule{}, std::nullopt);
    }
    log(tr("Edits discarded: the drawing's survey map is back in the manager."));
}

void SurveyCodeManagerDialog::bufferChanged()
{
    invalidatePlans();
    rebuildCodeTable();
    rebuildIssues();
    filterCensus();
    updateDirty();
    const std::string typed = text(testCode_->text());
    if (!typed.empty()) {
        explain(typed);
    } else if (!explained_.empty()) {
        explain(explained_);
    }
    updateFormIssues();
}

void SurveyCodeManagerDialog::updateDirty()
{
    const bool edited = dirty();
    if (edited) {
        dirtyLabel_->setText(tr("Unapplied edits: Apply puts them on the drawing, Revert "
                                "discards them. %1 rules.")
                                 .arg(buffer_.size()));
        dirtyLabel_->setStyleSheet(QStringLiteral("color: %1;").arg(kUndefinedNameColour.name()));
    } else {
        dirtyLabel_->setText(tr("No unapplied edits: these are the drawing's %1 rules.")
                                 .arg(buffer_.size()));
        dirtyLabel_->setStyleSheet({});
    }
    applyButton_->setEnabled(edited);
    revertButton_->setEnabled(edited);
    applyDirtyNote_->setHidden(!edited);
    lineworkDirtyNote_->setHidden(!edited);
    setWindowTitle(edited ? tr("Survey Code Manager *") : tr("Survey Code Manager"));
}

void SurveyCodeManagerDialog::documentChanged(const DocumentChanges& changes)
{
    katana::cad::Document* doc = document();
    if (doc == nullptr || !changes.any()) {
        return;
    }
    if (changes.surveyMap && !(doc->surveyMap() == baseline_)) {
        if (!dirty()) {
            // Unedited: the manager shows what the drawing has.
            buffer_ = doc->surveyMap();
            baseline_ = buffer_;
            formIndex_.reset();
            bufferChanged();
            loadForm(SurveyRule{}, std::nullopt);
        } else {
            // The edits are kept, but what they are measured against moves
            // to the map the drawing now has: dirty() means "the buffer is
            // not the drawing's map". Left on the old map, undoing the edits
            // would read as clean - Apply and Revert off, the tabs' notes
            // hidden - while Apply Codes and Linework ran a different map.
            baseline_ = doc->surveyMap();
            updateDirty();
            if (dirty()) {
                log(tr("The drawing's survey map was changed elsewhere while the manager has "
                       "unapplied edits. They are kept: Apply replaces the new map with them, "
                       "Revert takes the new map."));
            }
        }
    }
    if (changes.library) {
        // A change of the customisation's colours arrives as one of the
        // library too (Document::setColourTable), so the fields that list
        // them are filled again here, with the swatches that follow.
        for (QComboBox* field : {ruleColour_, ruleSymbolColour_, ruleTextColour_}) {
            refreshColourField(field, doc);
        }
        ruleLinestyle_->refresh();
        ruleSymbol_->refresh();
        rebuildCodeTable();
        rebuildIssues();
        if (!explained_.empty()) {
            explain(explained_);
        }
        updateFormIssues();
    }
    if (changes.model) {
        refreshCensus();
        updateFormIssues();
    }
    if (changes.model || changes.selection || changes.surveyMap || changes.library) {
        invalidatePlans();
        const QString count = QString::number(doc->selection().size());
        applySelection_->setText(tr("Selection (%1)").arg(count));
        lineworkSelection_->setText(tr("Selection (%1)").arg(count));
    }
}

// ---- editing ------------------------------------------------------------------------

katana::core::Status SurveyCodeManagerDialog::addRule(SurveyRule rule)
{
    if (auto status = buffer_.add(std::move(rule)); !status) {
        return status;
    }
    formIndex_ = buffer_.size() - 1;
    bufferChanged();
    selectRule(*formIndex_);
    return {};
}

katana::core::Status SurveyCodeManagerDialog::replaceRule(std::size_t index, SurveyRule rule)
{
    if (auto status = buffer_.replace(index, std::move(rule)); !status) {
        return status;
    }
    formIndex_ = index;
    bufferChanged();
    selectRule(index);
    return {};
}

katana::core::Status SurveyCodeManagerDialog::duplicateRule(std::size_t index)
{
    auto rule = buffer_.at(index);
    if (!rule) {
        return rule.error();
    }
    // Right after the original: the copy then loses every tie to it, so the
    // map resolves every code as before until the copy is edited.
    if (auto status = buffer_.insert(index + 1, std::move(*rule)); !status) {
        return status;
    }
    formIndex_ = index + 1;
    bufferChanged();
    selectRule(index + 1);
    return {};
}

katana::core::Status SurveyCodeManagerDialog::removeRule(std::size_t index)
{
    if (auto status = buffer_.remove(index); !status) {
        return status;
    }
    formIndex_.reset();
    bufferChanged();
    loadForm(SurveyRule{}, std::nullopt);
    return {};
}

katana::core::Status SurveyCodeManagerDialog::moveRule(std::size_t index, int by)
{
    if (by == 0) {
        return {};
    }
    if (by < 0 && index < static_cast<std::size_t>(-by)) {
        return makeError(ErrorCode::InvalidArgument, "the first rule cannot move up");
    }
    const std::size_t to =
        by < 0 ? index - static_cast<std::size_t>(-by) : index + static_cast<std::size_t>(by);
    if (to >= buffer_.size()) {
        return makeError(ErrorCode::InvalidArgument, "the last rule cannot move down");
    }
    if (auto status = buffer_.move(index, to); !status) {
        return status;
    }
    formIndex_ = to;
    bufferChanged();
    selectRule(to);
    return {};
}

void SurveyCodeManagerDialog::newRuleFromCode(const std::string& code)
{
    SurveyRule rule;
    rule.key = katana::cad::suggestedKey(code);
    rule.section = SurveySection::Map;
    tabs_->setCurrentIndex(0);
    tree_->setCurrentItem(nullptr);
    loadForm(rule, std::nullopt);
    testCode_->setText(text(code));
    ruleKey_->setFocus();
}

// ---- files -------------------------------------------------------------------------

katana::core::Status SurveyCodeManagerDialog::importCodes(const std::filesystem::path& path,
                                                          katana::cad::LoadMode mode)
{
    auto file = katana::cad::readCustomisationFile(path);
    if (!file) {
        return file.error();
    }
    const katana::entity::Customisation& held = file->customisation;
    if (held.map.empty()) {
        return makeError(ErrorCode::NotFound, "the file holds no survey code rules",
                         katana::core::pathToUtf8(path.filename()));
    }
    // The merge every load goes through (cad/customisation_merge.hpp), over
    // the two things this manager has a say in: the buffer as "the session"
    // and the file's rules, under the file's name, as the load. So Merge and
    // Replace mean here exactly what they mean to CUSTOMISE, and what else
    // the file holds cannot reach the buffer or the drawing.
    katana::entity::Customisation session;
    session.map = buffer_;
    katana::entity::Customisation rules;
    rules.name = held.name;
    rules.map = held.map;
    katana::cad::CustomisationMerge merged = katana::cad::mergeCustomisation(
        session, std::span<const katana::entity::Customisation>(&rules, 1), mode);
    if (!merged.ok()) {
        // All or nothing, as the merge is: the buffer is as it was.
        return makeError(ErrorCode::InvalidArgument,
                         "the file's survey code rules could not be merged: " +
                             merged.problems.front(),
                         katana::core::pathToUtf8(path.filename()));
    }
    buffer_ = std::move(merged.merged.map);
    for (const katana::cad::CustomisationLoad& load : merged.loads) {
        // The merge counts a key once for each section it has rules in.
        log(tr("%1 (%2): %3 added, %4 replaced in the rules being edited - a key once for "
               "each section it has rules in.")
                .arg(text(load.name), QString::fromLatin1(katana::cad::toString(mode)))
                .arg(load.addedKeys.size())
                .arg(load.replacedKeys.size()));
    }
    if (!merged.removedKeys.empty()) {
        log(tr("%1 keys removed by Replace.").arg(merged.removedKeys.size()));
    }
    QStringList left;
    if (!held.library.empty()) {
        left << tr("definitions (%1)").arg(held.library.size());
    }
    if (!held.colours.empty()) {
        left << tr("colours (%1)").arg(held.colours.size());
    }
    if (held.linework) {
        left << tr("the linework codes");
    }
    if (held.automation) {
        left << tr("the automation switches");
    }
    if (!left.isEmpty()) {
        log(tr("Not imported from %1, as this manager edits the survey code rules: %2. Import "
               "Definitions in the Symbol Library takes definitions and colours; the line "
               "CUSTOMISE <file> loads a whole customisation.")
                .arg(fileName(path), left.join(QStringLiteral(", "))));
    }
    formIndex_.reset();
    bufferChanged();
    loadForm(SurveyRule{}, std::nullopt);
    log(tr("Imported; review the rules and Apply to use them."));
    return {};
}

katana::core::Status SurveyCodeManagerDialog::exportCodes(const std::filesystem::path& path) const
{
    // The session's customisation with the rules being edited in the place
    // of its own, cut down to its codes by the one rule a part is written by
    // (cad::customisationPart, which CUSTOMISE EXPORT ... CODES cuts by too):
    // the session's name and its author's notice, the sources that brought
    // it rules, and the colours these rules name - not its definitions, and
    // nothing of its control codes or switches, which a file of codes for a
    // colleague must not reset. With the drawing closed there is no session
    // to say any of that, and the rules go under the file's own name.
    const katana::cad::Document* doc = document();
    katana::entity::Customisation session =
        doc != nullptr ? doc->customisation() : katana::entity::Customisation{};
    session.name = exportedCustomisationName(doc, path);
    session.map = buffer_;
    katana::entity::CustomisationWriteOptions options;
    options.linestyles = false;
    options.symbols = false;
    const katana::entity::Customisation codes =
        katana::cad::customisationPart(std::move(session), options);
    const auto written = katana::entity::customisationToJson(codes, options);
    if (!written) {
        return written.error();
    }
    if (auto status = writeFileBytes(path, *written); !status) {
        return status;
    }
    log(tr("Exported %1 rules to %2.").arg(buffer_.size()).arg(fileName(path)));
    return {};
}

katana::core::Status SurveyCodeManagerDialog::exportCodeList(const std::filesystem::path& path) const
{
    if (auto status = writeFileBytes(path, katana::cad::codeListCsv(buffer_)); !status) {
        return status;
    }
    log(tr("Exported the code list (%1 codes) to %2.")
            .arg(buffer_.keys().size())
            .arg(fileName(path)));
    return {};
}

// ---- closing -------------------------------------------------------------------------

void SurveyCodeManagerDialog::reject()
{
    if (dirty()) {
        if (!interactive_) {
            log(tr("Survey Code Manager closed with unapplied edits; they are kept in the "
                   "manager until applied or reverted."));
        } else {
            const auto answer = QMessageBox::question(
                this, tr("Unapplied edits"),
                tr("The survey code rules have edits that are not on the drawing yet."),
                QMessageBox::Apply | QMessageBox::Discard | QMessageBox::Cancel,
                QMessageBox::Apply);
            if (answer == QMessageBox::Cancel) {
                return;
            }
            if (answer == QMessageBox::Apply) {
                if (const auto status = apply(); !status) {
                    log(text(status.error().describe()), true);
                    return;
                }
            } else {
                revert();
            }
        }
    }
    QDialog::reject();
}

// ---- the code table ------------------------------------------------------------------

QWidget* SurveyCodeManagerDialog::buildCodeTableTab()
{
    auto* page = new QWidget(this);
    page->setObjectName(QStringLiteral("codeTableTab"));
    auto* layout = new QHBoxLayout(page);
    auto* split = new QSplitter(Qt::Horizontal, page);
    layout->addWidget(split);

    auto* left = new QWidget(split);
    auto* leftLayout = new QVBoxLayout(left);
    leftLayout->setContentsMargins(0, 0, 0, 0);
    QStringList chips;
    for (const SectionChip& chip : sectionChips()) {
        chips << QString::fromLatin1(chip.named);
    }
    tableFilter_ = new FilterBar(chips, left);
    for (int index = 0; index < chips.size(); ++index) {
        const QString word = sectionChipLabel(sectionChips()[static_cast<std::size_t>(index)]);
        tableFilter_->setChipLabel(index, word.isEmpty() ? tr("All") : word);
    }
    tableFilter_->setObjectName(QStringLiteral("codeTableFilter"));
    tableFilter_->setPlaceholderText(tr("Search keys, descriptions, layers, linestyles, symbols"));
    tableFilter_->onTextChanged = [this](const QString&) { rebuildCodeTable(); };
    tableFilter_->onChipChanged = [this](int) { rebuildCodeTable(); };
    leftLayout->addWidget(tableFilter_);

    tree_ = new QTreeWidget(left);
    tree_->setObjectName(QStringLiteral("codeTree"));
    tree_->setColumnCount(ColumnCount);
    tree_->setHeaderLabels({tr("Key"), tr("Description"), tr("Layer"), tr("Colour"),
                            tr("Line/Point"), tr("Linestyle"), tr("Symbol"), tr("Surface"),
                            tr("Attributes")});
    tree_->setIconSize(kThumbnailSize);
    tree_->setUniformRowHeights(true);
    tree_->setSelectionMode(QAbstractItemView::SingleSelection);
    tree_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    tree_->header()->setSectionResizeMode(QHeaderView::Interactive);
    tree_->setColumnWidth(KeyColumn, 150);
    tree_->setColumnWidth(DescriptionColumn, 170);
    tree_->setColumnWidth(LayerColumn, 130);
    leftLayout->addWidget(tree_, 1);
    connect(tree_, &QTreeWidget::currentItemChanged, this,
            [this](QTreeWidgetItem*, QTreeWidgetItem*) { treeSelectionChanged(); });

    auto* right = new QSplitter(Qt::Vertical, split);
    right->addWidget(buildDetailPane());
    auto* formScroll = new QScrollArea(right);
    formScroll->setWidgetResizable(true);
    formScroll->setWidget(buildRuleForm());
    right->addWidget(formScroll);

    split->addWidget(left);
    split->addWidget(right);
    split->setStretchFactor(0, 3);
    split->setStretchFactor(1, 2);
    return page;
}

void SurveyCodeManagerDialog::rebuildCodeTable()
{
    const katana::cad::Document* doc = document();
    rebuildingTree_ = true;
    tree_->clear();
    const std::string filter = text(tableFilter_->text());
    const std::vector<SurveySection>& wanted =
        sectionChips()[static_cast<std::size_t>(std::max(0, tableFilter_->chip()))].sections;

    const QColor ground = palette().color(QPalette::Base);
    const auto fill = [&](QTreeWidgetItem* item, const SurveyRule& rule) {
        item->setText(DescriptionColumn, text(rule.comment));
        item->setText(LayerColumn, text(rule.model));
        if (!rule.colour.empty()) {
            const auto rgb = colourOf(doc, rule.colour);
            if (rgb) {
                item->setIcon(ColourColumn, colourSwatch(*rgb, kThumbnailSize.height()));
                item->setText(ColourColumn, text(rule.colour));
            } else {
                item->setText(ColourColumn, tr("%1 (no RGB)").arg(text(rule.colour)));
                item->setForeground(ColourColumn, kUndefinedNameColour);
                item->setToolTip(ColourColumn,
                                 tr("A colour name neither the customisation's colours nor "
                                    "the standard names know: applying the code leaves the "
                                    "entity's colour as it is"));
            }
        }
        item->setText(BreaklineColumn, breaklineText(rule.breakline));
        if (!rule.linestyle.empty()) {
            const DefinitionState state = linestyleState(doc, rule.linestyle);
            item->setText(LinestyleColumn, definitionLabel(rule.linestyle, state));
            if (state == DefinitionState::Undefined || state == DefinitionState::WrongKind) {
                item->setForeground(LinestyleColumn, kUndefinedNameColour);
            } else if (state == DefinitionState::Defined && doc != nullptr &&
                       context_.thumbnails != nullptr) {
                const auto picture = context_.thumbnails->thumbnail(
                    *doc, ThumbnailKind::Linestyle, rule.linestyle, kThumbnailSize, ground);
                item->setIcon(LinestyleColumn, QIcon(QPixmap::fromImage(picture.image)));
            }
        }
        if (rule.symbol && !rule.symbol->style.empty()) {
            const DefinitionState state = symbolState(doc, rule.symbol->style);
            QString label = definitionLabel(rule.symbol->style, state);
            if (rule.symbol->size != 0.0) {
                label += QStringLiteral(", %1").arg(real(rule.symbol->size));
            }
            item->setText(SymbolColumn, label);
            if (state == DefinitionState::Undefined || state == DefinitionState::WrongKind) {
                item->setForeground(SymbolColumn, kUndefinedNameColour);
            } else if (doc != nullptr && context_.thumbnails != nullptr) {
                const auto picture = context_.thumbnails->thumbnail(
                    *doc, ThumbnailKind::Symbol, rule.symbol->style,
                    QSize(kThumbnailSize.height(), kThumbnailSize.height()), ground);
                item->setIcon(SymbolColumn, QIcon(QPixmap::fromImage(picture.image)));
            }
        }
        item->setText(TinableColumn, yesNo(rule.tinable));
        item->setText(AttributesColumn, attributeNames(rule));
    };

    std::map<std::string, QTreeWidgetItem*> groups;
    QTreeWidgetItem* keep = nullptr;
    for (const katana::cad::CodeTableRow& row : katana::cad::codeTable(buffer_)) {
        if (!katana::cad::codeTableRowMatches(row, filter)) {
            continue;
        }
        if (!wanted.empty() &&
            std::none_of(row.sections.begin(), row.sections.end(), [&](SurveySection section) {
                return std::find(wanted.begin(), wanted.end(), section) != wanted.end();
            })) {
            continue;
        }
        QTreeWidgetItem*& group = groups[row.combined.group];
        if (group == nullptr) {
            group = new QTreeWidgetItem(tree_);
            group->setText(KeyColumn, row.combined.group.empty() ? tr("(no group)")
                                                                 : text(row.combined.group));
            group->setFirstColumnSpanned(true);
            QFont bold = group->font(KeyColumn);
            bold.setBold(true);
            group->setFont(KeyColumn, bold);
        }
        auto* keyItem = new QTreeWidgetItem(group);
        keyItem->setText(KeyColumn, text(row.key));
        keyItem->setData(KeyColumn, kKeyRole, text(row.key));
        keyItem->setToolTip(KeyColumn,
                            tr("%1 rules; what a code this key catches resolves to")
                                .arg(row.rules.size()));
        fill(keyItem, row.combined);
        for (const std::size_t index : row.rules) {
            const SurveyRule& rule = buffer_.rules()[index];
            auto* ruleItem = new QTreeWidgetItem(keyItem);
            ruleItem->setText(KeyColumn, ruleLabel(index, rule));
            ruleItem->setData(KeyColumn, kRuleRole, QVariant::fromValue<qulonglong>(index));
            fill(ruleItem, rule);
            if (formIndex_ && *formIndex_ == index) {
                keep = ruleItem;
            }
        }
        if (keep == nullptr && !formIndex_ && row.key == explained_) {
            keep = keyItem;
        }
    }
    tree_->expandToDepth(0);
    if (keep != nullptr) {
        tree_->setCurrentItem(keep);
        tree_->scrollToItem(keep);
    }
    rebuildingTree_ = false;
}

QTreeWidgetItem* SurveyCodeManagerDialog::findRuleItem(std::size_t index) const
{
    for (QTreeWidgetItemIterator it(tree_); *it != nullptr; ++it) {
        const QVariant value = (*it)->data(KeyColumn, kRuleRole);
        if (value.isValid() && value.toULongLong() == index) {
            return *it;
        }
    }
    return nullptr;
}

QTreeWidgetItem* SurveyCodeManagerDialog::findKeyItem(const std::string& key) const
{
    const QString wanted = text(key);
    for (QTreeWidgetItemIterator it(tree_); *it != nullptr; ++it) {
        const QVariant value = (*it)->data(KeyColumn, kKeyRole);
        if (value.isValid() && value.toString() == wanted) {
            return *it;
        }
    }
    return nullptr;
}

void SurveyCodeManagerDialog::selectRule(std::size_t index)
{
    auto rule = buffer_.at(index);
    if (!rule) {
        return;
    }
    tabs_->setCurrentIndex(0);
    QTreeWidgetItem* item = findRuleItem(index);
    if (item == nullptr) {
        // Filtered out: clear the filter rather than edit a rule no one can see.
        tableFilter_->setText({});
        tableFilter_->setChip(0);
        item = findRuleItem(index);
    }
    formIndex_ = index;
    if (item != nullptr) {
        tree_->setCurrentItem(item);
        tree_->scrollToItem(item);
    }
    loadForm(*rule, index);
    testCode_->clear();
    explain(rule->key);
}

void SurveyCodeManagerDialog::selectKey(const std::string& key)
{
    QTreeWidgetItem* item = findKeyItem(key);
    if (item == nullptr) {
        tableFilter_->setText({});
        tableFilter_->setChip(0);
        item = findKeyItem(key);
    }
    if (item != nullptr) {
        tree_->setCurrentItem(item);
        tree_->scrollToItem(item);
    }
}

void SurveyCodeManagerDialog::treeSelectionChanged()
{
    if (rebuildingTree_) {
        return;
    }
    const QTreeWidgetItem* item = tree_->currentItem();
    if (item == nullptr) {
        return;
    }
    const QVariant ruleValue = item->data(KeyColumn, kRuleRole);
    if (ruleValue.isValid()) {
        const std::size_t index = ruleValue.toULongLong();
        auto rule = buffer_.at(index);
        if (rule) {
            loadForm(*rule, index);
            testCode_->clear();
            explain(rule->key);
        }
        return;
    }
    const QVariant keyValue = item->data(KeyColumn, kKeyRole);
    if (!keyValue.isValid()) {
        return; // a group heading
    }
    const std::string key = text(keyValue.toString());
    const auto& rules = buffer_.rules();
    const auto first = std::find_if(rules.begin(), rules.end(),
                                    [&](const SurveyRule& rule) { return rule.key == key; });
    if (first != rules.end()) {
        loadForm(*first, static_cast<std::size_t>(first - rules.begin()));
    }
    testCode_->clear();
    explain(key);
}

// ---- the detail pane -------------------------------------------------------------------

QWidget* SurveyCodeManagerDialog::buildDetailPane()
{
    auto* pane = new QWidget(this);
    pane->setObjectName(QStringLiteral("codeDetail"));
    auto* layout = new QVBoxLayout(pane);
    layout->setContentsMargins(0, 0, 0, 0);

    testCode_ = new QLineEdit(pane);
    testCode_->setObjectName(QStringLiteral("testCode"));
    testCode_->setPlaceholderText(tr("Test a code: type a field code, e.g. WM01"));
    testCode_->setClearButtonEnabled(true);
    layout->addWidget(testCode_);
    connect(testCode_, &QLineEdit::textChanged, this, [this](const QString& typed) {
        if (!typed.isEmpty()) {
            explain(text(typed));
        }
    });

    explainHeading_ = new QLabel(pane);
    explainHeading_->setObjectName(QStringLiteral("explainHeading"));
    explainHeading_->setWordWrap(true);
    explainHeading_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    layout->addWidget(explainHeading_);

    explainFields_ = new QTreeWidget(pane);
    explainFields_->setObjectName(QStringLiteral("explainFields"));
    explainFields_->setColumnCount(4);
    explainFields_->setHeaderLabels(
        {tr("Field"), tr("Value"), tr("Set by"), tr("Decided against")});
    explainFields_->setRootIsDecorated(false);
    explainFields_->setUniformRowHeights(true);
    explainFields_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    explainFields_->setToolTip(tr("Double-click a field to edit the rule that set it"));
    layout->addWidget(explainFields_, 1);
    // From "set by #7" to rule #7 in the form - after the click has
    // returned: selecting the rule explains it again, which rebuilds this
    // very list, and a view is never rebuilt inside its own signal.
    connect(explainFields_, &QTreeWidget::itemDoubleClicked, this,
            [this](QTreeWidgetItem* item, int) {
                const QVariant rule = item->data(0, kRuleRole);
                if (rule.isValid()) {
                    const std::size_t index = rule.toULongLong();
                    QTimer::singleShot(0, this, [this, index] { selectRule(index); });
                }
            });

    explainDefinitions_ = new QLabel(pane);
    explainDefinitions_->setObjectName(QStringLiteral("explainDefinitions"));
    explainDefinitions_->setWordWrap(true);
    explainDefinitions_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    layout->addWidget(explainDefinitions_);

    explainNearMisses_ = new QLabel(pane);
    explainNearMisses_->setObjectName(QStringLiteral("explainNearMisses"));
    explainNearMisses_->setWordWrap(true);
    layout->addWidget(explainNearMisses_);

    auto* previews = new QHBoxLayout;
    if (context_.document != nullptr) {
        linestylePreview_ = new StylePreview(*context_.document, pane);
        linestylePreview_->setObjectName(QStringLiteral("linestylePreview"));
        linestylePreview_->setMinimumSize(200, 70);
        symbolPreview_ = new StylePreview(*context_.document, pane);
        symbolPreview_->setObjectName(QStringLiteral("symbolPreview"));
        symbolPreview_->setMinimumSize(90, 70);
        previews->addWidget(linestylePreview_, 2);
        previews->addWidget(symbolPreview_, 1);
    }
    layout->addLayout(previews);
    return pane;
}

void SurveyCodeManagerDialog::explain(const std::string& code)
{
    explained_ = code;
    explainFields_->clear();
    const katana::cad::Document* doc = document();
    if (doc == nullptr) {
        explainHeading_->setText(tr("The drawing is closed."));
        return;
    }
    const katana::cad::CodeExplanation explanation = katana::cad::explainCode(
        buffer_, code, [doc](std::string_view name) { return doc->definitionFor(name); },
        katana::cad::colourLookup(*doc));

    QString how;
    switch (explanation.kind) {
    case SurveyMatchKind::Exact:
        how = tr("the exact key %1").arg(text(explanation.resolved.key));
        break;
    case SurveyMatchKind::Prefix:
        how = tr("the prefix key %1").arg(text(explanation.resolved.key));
        break;
    case SurveyMatchKind::FallbackOnly:
        how = tr("nothing but the fallback key *");
        break;
    case SurveyMatchKind::None:
        how = tr("no rule at all");
        break;
    }
    explainHeading_->setText(tr("<b>%1</b>: %2 - met by %3, %4 rules in all.")
                                 .arg(text(code).toHtmlEscaped(),
                                      matchClass(explanation.kind, explanation.matched), how)
                                 .arg(explanation.rules.size()));

    const auto cite = [](std::size_t rule, const std::string& key, SurveySection section) {
        return QStringLiteral("#%1 %2 (%3)")
            .arg(rule)
            .arg(text(key), QString::fromLatin1(katana::entity::toString(section)));
    };
    for (const katana::cad::CodeFieldSource& field : explanation.fields) {
        QStringList against;
        for (const auto& lost : field.overruled) {
            against << QStringLiteral("%1: %2").arg(cite(lost.rule, lost.key, lost.section),
                                                    text(lost.value));
        }
        auto* item = new QTreeWidgetItem(explainFields_);
        item->setText(0, text(field.field));
        item->setText(1, text(field.value));
        item->setText(2, cite(field.rule, field.key, field.section));
        item->setText(3, against.join(QStringLiteral("; ")));
        item->setData(0, kRuleRole, QVariant::fromValue<qulonglong>(field.rule));
    }
    for (const katana::cad::CodeAttribute& attribute : explanation.attributes) {
        auto* item = new QTreeWidgetItem(explainFields_);
        item->setText(0, tr("attribute (%1)").arg(text(attribute.scope)));
        QString value = QStringLiteral("%1 %2 = %3")
                            .arg(text(attribute.attribute.type), text(attribute.attribute.name),
                                 text(attribute.attribute.value));
        if (attribute.deferred) {
            value += tr(" (names another attribute: left for the survey data)");
        }
        item->setText(1, value);
        item->setText(2, cite(attribute.rule, attribute.key, attribute.section));
        item->setData(0, kRuleRole, QVariant::fromValue<qulonglong>(attribute.rule));
    }
    for (int column = 0; column < 3; ++column) {
        explainFields_->resizeColumnToContents(column);
    }

    QStringList definitions;
    const auto& linestyle = explanation.linestyle;
    if (!linestyle.name.empty()) {
        QString state;
        if (linestyle.plain) {
            state = tr("the plain continuous line: needs no definition");
        } else if (linestyle.defined && linestyle.vertexMode) {
            state = tr("an `at vertices` symbol, not a linestyle: drawn as a plain line");
        } else if (linestyle.defined) {
            state = tr("defined in the loaded library");
        } else {
            state = tr("NOT DEFINED: drawn as a plain line until a library defines it");
        }
        definitions << tr("Linestyle %1: %2").arg(text(linestyle.name), state);
    }
    const auto& symbol = explanation.symbol;
    if (!symbol.name.empty()) {
        QString state;
        if (symbol.defined) {
            state = symbol.vertexMode ? tr("defined, at vertices") : tr("defined");
        } else if (symbol.builtIn) {
            state = tr("drawn by Katana itself: no library needed");
        } else {
            state = tr("NOT DEFINED: drawn as a mark until a library defines it");
        }
        definitions << tr("Symbol %1: %2").arg(text(symbol.name), state);
    }
    if (!explanation.colour.name.empty()) {
        definitions << (explanation.colour.rgb
                            ? tr("Colour %1: %2")
                                  .arg(text(explanation.colour.name),
                                       QColor(explanation.colour.rgb->r, explanation.colour.rgb->g,
                                              explanation.colour.rgb->b)
                                           .name()
                                           .toUpper())
                            : tr("Colour %1: no RGB known - applying the code leaves the "
                                 "entity's colour as it is")
                                  .arg(text(explanation.colour.name)));
    }
    explainDefinitions_->setText(definitions.join(QLatin1Char('\n')));

    QStringList misses;
    for (const katana::cad::CodeNearMiss& miss : explanation.nearMisses) {
        misses << tr("%1 (%2)").arg(text(miss.key),
                                    QString::fromLatin1(katana::cad::toString(miss.kind)));
    }
    explainNearMisses_->setText(
        misses.isEmpty()
            ? QString()
            : tr("Would have matched but for letter case or blanks - matching is exact: %1")
                  .arg(misses.join(QStringLiteral(", "))));
    explainNearMisses_->setHidden(misses.isEmpty());

    if (linestylePreview_ != nullptr) {
        if (!linestyle.name.empty() && !linestyle.plain) {
            linestylePreview_->setLinestyle(linestyle.name);
        } else {
            linestylePreview_->clear();
        }
    }
    if (symbolPreview_ != nullptr) {
        if (!symbol.name.empty() && explanation.resolved.symbol) {
            symbolPreview_->setSymbol(symbol.name, explanation.resolved.symbol->size);
        } else {
            symbolPreview_->clear();
        }
    }
}

} // namespace katana::qt
