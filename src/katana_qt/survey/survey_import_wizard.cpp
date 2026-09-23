#include "survey/survey_import_wizard.hpp"

#include <QAction>
#include <QCheckBox>
#include <QComboBox>
#include <QFile>
#include <QFileDialog>
#include <QFileInfo>
#include <QFontDatabase>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QSpinBox>
#include <QStackedWidget>
#include <QTableWidget>
#include <QTimer>
#include <QTreeWidget>
#include <QVBoxLayout>

#include <algorithm>
#include <format>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/survey_points.hpp"
#include "katana/core/text.hpp"
#include "katana/surveyio/format.hpp"
#include "survey/survey_templates.hpp"
#include "theme.hpp"
#include "view_workspace.hpp"

namespace katana::qt {

namespace cad = katana::cad;
namespace surveyio = katana::surveyio;
namespace survey = katana::survey;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

// Points the layout page previews. Enough rows to see that the columns are
// the right way round and the numbers are what the file holds; the whole file
// is still parsed, so an error on its last line shows here too.
constexpr int kPreviewRows = 20;

const std::vector<surveyio::ColumnRole>& roles()
{
    static const std::vector<surveyio::ColumnRole> all{
        surveyio::ColumnRole::PointId,   surveyio::ColumnRole::Northing,
        surveyio::ColumnRole::Easting,   surveyio::ColumnRole::Elevation,
        surveyio::ColumnRole::Code,      surveyio::ColumnRole::Description,
        surveyio::ColumnRole::Ignore};
    return all;
}

const std::vector<survey::LinearUnit>& units()
{
    static const std::vector<survey::LinearUnit> all{
        survey::LinearUnit::Metres, survey::LinearUnit::Feet, survey::LinearUnit::UsSurveyFeet,
        survey::LinearUnit::Links};
    return all;
}

const std::vector<cad::ExistingPointPolicy>& policies()
{
    static const std::vector<cad::ExistingPointPolicy> all{
        cad::ExistingPointPolicy::Refuse, cad::ExistingPointPolicy::Skip,
        cad::ExistingPointPolicy::Replace, cad::ExistingPointPolicy::KeepBoth};
    return all;
}

QString qs(std::string_view text)
{
    return QString::fromUtf8(text.data(), static_cast<qsizetype>(text.size()));
}

QLabel* mutedLabel(const QString& text, QWidget* parent)
{
    auto* label = new QLabel(text, parent);
    label->setWordWrap(true);
    label->setStyleSheet(QString("color: %1").arg(theme::textMuted().name()));
    return label;
}

// The columns of a layout as a template spells them: "P,N,E,Z,D".
QString columnLetters(const surveyio::DelimitedLayout& layout)
{
    QStringList letters;
    for (const surveyio::ColumnRole role : layout.columns) {
        letters << QString(QChar(surveyio::templateLetter(role)));
    }
    return letters.join(',');
}

// A whole number from a field, or nullopt when it is blank; ParseFailure for
// anything else - through core/text.hpp, never the locale.
Result<std::optional<int>> epsgField(const QLineEdit* field, const char* what)
{
    const std::string text = field->text().trimmed().toStdString();
    if (text.empty()) {
        return std::optional<int>{};
    }
    std::string_view body = text;
    if (body.size() > 5 && (body.substr(0, 5) == "EPSG:" || body.substr(0, 5) == "epsg:")) {
        body.remove_prefix(5);
    }
    const auto code = katana::core::parseInteger(body);
    if (!code || *code <= 0 || *code > 2'147'483'647) {
        return makeError(ErrorCode::ParseFailure,
                         std::string(what) + ": an EPSG code is a positive whole number", text);
    }
    return std::optional<int>{static_cast<int>(*code)};
}

} // namespace

SurveyImportWizard::SurveyImportWizard(SurveyImportContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName("surveyImportDialog");
    setWindowTitle("Import Survey Points");
    setModal(false);
    auto* layout = new QVBoxLayout(this);
    step_ = new QLabel(this);
    step_->setObjectName("step");
    QFont bold = step_->font();
    bold.setBold(true);
    step_->setFont(bold);
    layout->addWidget(step_);

    pages_ = new QStackedWidget(this);
    pages_->addWidget(buildFilePage());
    pages_->addWidget(buildFormatPage());
    pages_->addWidget(buildLayoutPage());
    pages_->addWidget(buildSystemPage());
    pages_->addWidget(buildOptionsPage());
    pages_->addWidget(buildReportPage());
    layout->addWidget(pages_, 1);

    message_ = new QLabel(this);
    message_->setObjectName("message");
    message_->setWordWrap(true);
    message_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    layout->addWidget(message_);

    auto* buttons = new QHBoxLayout();
    buttons->addStretch(1);
    back_ = new QPushButton("< Back", this);
    back_->setObjectName("back");
    back_->setAutoDefault(false);
    next_ = new QPushButton("Next >", this);
    next_->setObjectName("next");
    next_->setDefault(true);
    import_ = new QPushButton("Import", this);
    import_->setObjectName("import");
    import_->setAutoDefault(false);
    auto* close = new QPushButton("Close", this);
    close->setObjectName("close");
    close->setAutoDefault(false);
    buttons->addWidget(back_);
    buttons->addWidget(next_);
    buttons->addWidget(import_);
    buttons->addWidget(close);
    layout->addLayout(buttons);

    connect(back_, &QPushButton::clicked, this, [this] { goTo(pages_->currentIndex() - 1); });
    connect(next_, &QPushButton::clicked, this, [this] {
        const int page = pages_->currentIndex();
        if (const auto status = leave(page); !status) {
            showError(status.error());
            return;
        }
        goTo(page + 1);
    });
    connect(import_, &QPushButton::clicked, this, [this] { importNow(); });
    connect(close, &QPushButton::clicked, this, [this] { hide(); });

    goTo(FilePage);
    resize(820, 620);
}

// ---- the pages ---------------------------------------------------------------------------

QWidget* SurveyImportWizard::buildFilePage()
{
    auto* page = new QWidget(this);
    auto* layout = new QVBoxLayout(page);
    layout->addWidget(mutedLabel(
        "Choose a file of surveyed points. The next step says what Katana makes of it and, "
        "when it cannot be sure, asks you: a file is never read with a format nobody chose.",
        page));
    auto* row = new QHBoxLayout();
    file_ = new QLineEdit(page);
    file_->setObjectName("file");
    file_->setPlaceholderText("The file to import");
    auto* browse = new QPushButton("Browse...", page);
    browse->setObjectName("browse");
    browse->setAutoDefault(false);
    connect(browse, &QPushButton::clicked, this, [this] {
        const QString path = QFileDialog::getOpenFileName(
            this, "Import Survey Points", file_->text(),
            "Point files (*.csv *.txt *.tsv *.pnt *.xyz);;All files (*)");
        if (!path.isEmpty()) {
            file_->setText(path);
        }
    });
    row->addWidget(file_, 1);
    row->addWidget(browse);
    layout->addLayout(row);
    layout->addStretch(1);
    return page;
}

QWidget* SurveyImportWizard::buildFormatPage()
{
    auto* page = new QWidget(this);
    auto* layout = new QVBoxLayout(page);
    detectionSummary_ = new QLabel(page);
    detectionSummary_->setObjectName("detectionSummary");
    detectionSummary_->setWordWrap(true);
    layout->addWidget(detectionSummary_);
    candidates_ = new QTreeWidget(page);
    candidates_->setObjectName("candidates");
    candidates_->setHeaderLabels({"Format", "Confidence", "Evidence", "Record"});
    candidates_->setRootIsDecorated(false);
    candidates_->header()->setStretchLastSection(true);
    layout->addWidget(candidates_, 1);
    auto* form = new QFormLayout();
    format_ = new QComboBox(page);
    format_->setObjectName("format");
    format_->setToolTip("The format to read the file with");
    form->addRow("Read the file as:", format_);
    layout->addLayout(form);
    formatRecord_ = mutedLabel({}, page);
    formatRecord_->setObjectName("formatRecord");
    layout->addWidget(formatRecord_);
    connect(format_, &QComboBox::currentIndexChanged, this, [this] { showFormatRecord(); });
    return page;
}

QWidget* SurveyImportWizard::buildLayoutPage()
{
    auto* page = new QWidget(this);
    auto* layout = new QVBoxLayout(page);
    proposal_ = new QLabel(page);
    proposal_->setObjectName("proposal");
    proposal_->setWordWrap(true);
    layout->addWidget(proposal_);

    auto* form = new QFormLayout();
    candidate_ = new QComboBox(page);
    candidate_->setObjectName("candidate");
    candidate_->setToolTip("The layouts that fit the file, best first");
    form->addRow("Proposed:", candidate_);

    auto* choices = new QHBoxLayout();
    preset_ = new QComboBox(page);
    preset_->setObjectName("preset");
    preset_->addItem("(presets)");
    for (const surveyio::ColumnPreset preset : surveyio::allColumnPresets()) {
        preset_->addItem(surveyio::toString(preset));
    }
    preset_->setToolTip("A column order survey software names by its letters");
    template_ = new QComboBox(page);
    template_->setObjectName("template");
    template_->setToolTip("A layout saved earlier");
    fillTemplateChoice(*template_);
    templateName_ = new QLineEdit(page);
    templateName_->setObjectName("templateName");
    templateName_->setPlaceholderText("name to save as");
    auto* save = new QPushButton("Save", page);
    save->setObjectName("saveTemplate");
    save->setAutoDefault(false);
    auto* remove = new QPushButton("Delete", page);
    remove->setObjectName("deleteTemplate");
    remove->setAutoDefault(false);
    choices->addWidget(preset_);
    choices->addWidget(template_, 1);
    choices->addWidget(templateName_, 1);
    choices->addWidget(save);
    choices->addWidget(remove);
    form->addRow("Or use:", choices);

    columns_ = new QLineEdit(page);
    columns_->setObjectName("columns");
    columns_->setToolTip("One letter per column in file order: P point id, N northing, "
                         "E easting, Z elevation, C code, D description, - ignore");
    form->addRow("Columns:", columns_);
    columnRoles_ = new QWidget(page);
    columnRoles_->setObjectName("columnRoles");
    roleRow_ = new QHBoxLayout(columnRoles_);
    roleRow_->setContentsMargins(0, 0, 0, 0);
    form->addRow("", columnRoles_);

    auto* format = new QHBoxLayout();
    delimiter_ = new QComboBox(page);
    delimiter_->setObjectName("delimiter");
    for (const auto d : {surveyio::Delimiter::Comma, surveyio::Delimiter::Tab,
                         surveyio::Delimiter::Semicolon, surveyio::Delimiter::Whitespace}) {
        delimiter_->addItem(surveyio::toString(d));
    }
    headerLines_ = new QSpinBox(page);
    headerLines_->setObjectName("headerLines");
    headerLines_->setRange(0, 1000);
    headerLines_->setToolTip("Lines at the top of the file skipped whatever they hold");
    comment_ = new QLineEdit(page);
    comment_->setObjectName("comment");
    comment_->setPlaceholderText("none");
    comment_->setToolTip("A line starting with this is a comment");
    comment_->setMaximumWidth(80);
    quoting_ = new QComboBox(page);
    quoting_->setObjectName("quoting");
    quoting_->addItems({surveyio::toString(surveyio::Quoting::DoubleQuote),
                        surveyio::toString(surveyio::Quoting::None)});
    format->addWidget(new QLabel("Delimiter", page));
    format->addWidget(delimiter_);
    format->addWidget(new QLabel("Header lines", page));
    format->addWidget(headerLines_);
    format->addWidget(new QLabel("Comment", page));
    format->addWidget(comment_);
    format->addWidget(new QLabel("Quotes", page));
    format->addWidget(quoting_);
    format->addStretch(1);
    form->addRow("Reading:", format);
    layout->addLayout(form);

    confirmOrder_ = new QCheckBox(
        "I have checked the preview: the northing and easting columns are the right way round",
        page);
    confirmOrder_->setObjectName("confirmOrder");
    layout->addWidget(confirmOrder_);

    preview_ = new QTableWidget(0, 7, page);
    preview_->setObjectName("preview");
    preview_->setHorizontalHeaderLabels(
        {"Line", "Point", "Easting", "Northing", "Elevation", "Code", "Description"});
    preview_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    preview_->verticalHeader()->setVisible(false);
    preview_->horizontalHeader()->setStretchLastSection(true);
    layout->addWidget(preview_, 1);
    parseError_ = new QLabel(page);
    parseError_->setObjectName("parseError");
    parseError_->setWordWrap(true);
    parseError_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    layout->addWidget(parseError_);

    connect(candidate_, &QComboBox::activated, this, [this](int index) {
        if (!layoutProposal_ || index < 0 ||
            static_cast<std::size_t>(index) >= layoutProposal_->candidates().size()) {
            return;
        }
        applyLayout(layoutProposal_->candidates()[static_cast<std::size_t>(index)].layout);
    });
    connect(preset_, &QComboBox::currentIndexChanged, this, [this](int index) {
        if (index <= 0) {
            return;
        }
        const auto preset = surveyio::columnPresetNamed(preset_->currentText().toStdString());
        if (!preset) {
            showError(preset.error());
            return;
        }
        surveyio::DelimitedLayout presetLayout;
        presetLayout.columns = surveyio::presetColumns(*preset);
        columns_->setText(columnLetters(presetLayout));
    });
    connect(template_, &QComboBox::currentIndexChanged, this, [this](int index) {
        if (index <= 0) {
            return;
        }
        const auto saved =
            surveyio::parseLayoutTemplate(template_->currentData().toString().toStdString());
        if (!saved) {
            showError(saved.error());
            return;
        }
        applyLayout(*saved);
        templateName_->setText(template_->currentText());
    });
    connect(save, &QPushButton::clicked, this, [this] {
        const auto current = layoutFromFields();
        if (!current) {
            showError(current.error());
            return;
        }
        const QString name = templateName_->text().trimmed();
        if (const auto status =
                saveLayoutTemplate(name, qs(surveyio::layoutTemplate(*current)));
            !status) {
            showError(status.error());
            return;
        }
        fillTemplateChoice(*template_);
        template_->blockSignals(true);
        template_->setCurrentIndex(std::max(0, template_->findText(name)));
        template_->blockSignals(false);
        showMessage("Saved the layout as the template \"" + name + "\".");
        context_.log("Saved the survey layout template \"" + name + "\": " +
                         qs(surveyio::layoutTemplate(*current)),
                     false);
    });
    connect(remove, &QPushButton::clicked, this, [this] {
        const QString name =
            template_->currentIndex() > 0 ? template_->currentText() : templateName_->text();
        if (const auto status = deleteLayoutTemplate(name.trimmed()); !status) {
            showError(status.error());
            return;
        }
        fillTemplateChoice(*template_);
        showMessage("Deleted the template \"" + name.trimmed() + "\".");
        context_.log("Deleted the survey layout template \"" + name.trimmed() + "\"", false);
    });
    connect(columns_, &QLineEdit::textChanged, this, [this] { schedulePreview(); });
    connect(delimiter_, &QComboBox::currentIndexChanged, this, [this] { schedulePreview(); });
    connect(headerLines_, &QSpinBox::valueChanged, this, [this] { schedulePreview(); });
    connect(comment_, &QLineEdit::textChanged, this, [this] { schedulePreview(); });
    connect(quoting_, &QComboBox::currentIndexChanged, this, [this] { schedulePreview(); });
    return page;
}

QWidget* SurveyImportWizard::buildSystemPage()
{
    auto* page = new QWidget(this);
    auto* layout = new QVBoxLayout(page);
    layout->addWidget(mutedLabel(
        "A point file does not say what its numbers are: choose the unit they are in (there is "
        "no default - a wrong unit scales the whole survey). The coordinate system is recorded "
        "as unknown unless you state it. Katana transforms the points only when you give BOTH "
        "the file's system and a target, by EPSG code, through its geodesy library; both must "
        "be projected (grid) systems, and heights are carried through unchanged - no vertical "
        "datum change is made.",
        page));
    auto* form = new QFormLayout();
    unit_ = new QComboBox(page);
    unit_->setObjectName("unit");
    unit_->addItem("(choose the unit)");
    for (const survey::LinearUnit unit : units()) {
        unit_->addItem(survey::toString(unit));
    }
    form->addRow("The numbers are in:", unit_);
    declared_ = new QLineEdit(page);
    declared_->setObjectName("declared");
    declared_->setPlaceholderText("unknown - or an EPSG code, e.g. 28356");
    form->addRow("The file's system:", declared_);
    target_ = new QLineEdit(page);
    target_->setObjectName("target");
    target_->setPlaceholderText("none - or an EPSG code to transform into");
    form->addRow("Transform into:", target_);
    layout->addLayout(form);
    layout->addStretch(1);
    return page;
}

QWidget* SurveyImportWizard::buildOptionsPage()
{
    auto* page = new QWidget(this);
    auto* layout = new QVBoxLayout(page);
    auto* form = new QFormLayout();
    const cad::SurveyImportOptions defaults;
    layer_ = new QLineEdit(qs(defaults.layer), page);
    layer_->setObjectName("layer");
    layer_->setToolTip("The layer the points go on; '/' makes a child layer");
    form->addRow("Layer:", layer_);
    layerPerCode_ = new QCheckBox("A layer per field code beneath it", page);
    layerPerCode_->setObjectName("layerPerCode");
    form->addRow("", layerPerCode_);
    existing_ = new QComboBox(page);
    existing_->setObjectName("existing");
    for (const cad::ExistingPointPolicy policy : policies()) {
        existing_->addItem(cad::toString(policy));
    }
    existing_->setToolTip("What happens to a point whose id a survey point in the drawing "
                          "already has");
    form->addRow("Ids already in the drawing:", existing_);
    applyCodes_ = new QCheckBox("Apply survey codes to the imported points", page);
    applyCodes_->setObjectName("applyCodes");
    form->addRow("", applyCodes_);
    layout->addLayout(form);
    layout->addWidget(mutedLabel(
        "Refusing is the default: an import of ids the drawing already has is more often the "
        "wrong file than a wanted update. Survey codes need a loaded 12d mapfile (File > Load "
        "12d Customisation) and are applied to the imported points only, as their own undoable "
        "step.",
        page));
    layout->addStretch(1);
    return page;
}

QWidget* SurveyImportWizard::buildReportPage()
{
    auto* page = new QWidget(this);
    auto* layout = new QVBoxLayout(page);
    layout->addWidget(mutedLabel(
        "What the import will do. Import makes it one command: one Undo removes it all.", page));
    report_ = new QPlainTextEdit(page);
    report_->setObjectName("report");
    report_->setReadOnly(true);
    report_->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    report_->setLineWrapMode(QPlainTextEdit::NoWrap);
    layout->addWidget(report_, 1);
    return page;
}

// ---- moving between pages --------------------------------------------------------------------

void SurveyImportWizard::goTo(int page)
{
    page = std::clamp(page, 0, Pages - 1);
    static const char* const titles[] = {"Choose the file",       "The file's format",
                                         "Columns and delimiter", "Units and coordinate system",
                                         "Options",               "Report"};
    step_->setText(QString("Step %1 of %2: %3").arg(page + 1).arg(Pages).arg(titles[page]));
    pages_->setCurrentIndex(page);
    back_->setEnabled(page > 0);
    next_->setEnabled(page < ReportPage);
    import_->setEnabled(page == ReportPage && !blocked_);
    next_->setDefault(page < ReportPage);
    import_->setDefault(page == ReportPage);
    if (page == OptionsPage) {
        const bool mapfile = context_.applySurveyCodes != nullptr &&
                             !context_.document->surveyMap().empty();
        applyCodes_->setEnabled(mapfile);
        if (!mapfile) {
            applyCodes_->setChecked(false);
        }
    }
    message_->clear();
}

Status SurveyImportWizard::leave(int page)
{
    switch (page) {
    case FilePage:
        return readFile();
    case FormatPage:
        return chooseFormat();
    case LayoutPage: {
        const auto layout = layoutFromFields();
        if (!layout) {
            return layout.error();
        }
        // Now, not when the event loop gets to it: a field typed a moment ago
        // must be what Next judges. Safe here - Next is not a role box.
        preview();
        if (previewFailed_) {
            return makeError(ErrorCode::ParseFailure,
                             "the file does not read with this layout - see the error under "
                             "the preview");
        }
        if (layoutProposal_ &&
            layoutProposal_->outcome() != surveyio::LayoutProposalOutcome::Decided &&
            !confirmOrder_->isChecked()) {
            return makeError(ErrorCode::InvalidState,
                             "Katana could not tell the order of the columns from the file: "
                             "check the preview and tick that northing and easting are the "
                             "right way round");
        }
        return {};
    }
    case SystemPage:
        return parseAndTransform();
    case OptionsPage:
        prepareReport();
        return {};
    default:
        return {};
    }
}

Status SurveyImportWizard::readFile()
{
    const QString path = file_->text().trimmed();
    if (path.isEmpty()) {
        return makeError(ErrorCode::InvalidArgument, "choose a file to import");
    }
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly)) {
        return makeError(ErrorCode::FileImportFailure, "the file cannot be opened",
                         path.toStdString() + ": " + file.errorString().toStdString());
    }
    const QByteArray bytes = file.readAll();
    bytes_.assign(bytes.constData(), static_cast<std::size_t>(bytes.size()));
    fileName_ = QFileInfo(path).fileName().toStdString();
    parsed_.reset();
    project_.reset();

    const std::string_view sample =
        std::string_view(bytes_).substr(0, std::min(bytes_.size(), surveyio::kProbeBytes));
    detection_ = surveyio::detectFormat(
        surveyio::probeOf(sample, fileName_, bytes_.size() > surveyio::kProbeBytes));
    if (detection_->outcome() == surveyio::DetectionOutcome::Empty) {
        return makeError(ErrorCode::FileImportFailure, "the file is empty", fileName_);
    }

    const bool identified = detection_->outcome() == surveyio::DetectionOutcome::Identified;
    QString summary = QString("Detection: %1. %2")
                          .arg(surveyio::toString(detection_->outcome()),
                               qs(detection_->summary()));
    if (!identified) {
        summary += "\nKatana is not sure what this file is: choose the format to read it with.";
    }
    detectionSummary_->setText(summary);
    detectionSummary_->setStyleSheet(
        QString("color: %1").arg(identified ? theme::textMuted().name() : theme::error().name()));

    candidates_->clear();
    format_->blockSignals(true);
    format_->clear();
    if (!identified) {
        format_->addItem("(choose the format)");
    }
    for (const surveyio::FormatCandidate& candidate : detection_->candidates()) {
        const auto descriptor = surveyio::formatRegistry().find(candidate.formatId);
        const QString name =
            descriptor ? qs(descriptor->humanName) : qs(candidate.formatId);
        auto* item = new QTreeWidgetItem(candidates_);
        item->setText(0, name);
        item->setText(1, QString::number(candidate.confidence, 'f', 2));
        item->setText(2, qs(candidate.evidence));
        item->setToolTip(2, qs(candidate.evidence));
        // What the format carries and the parser's version, for every
        // candidate and not only the chosen one: two formats a file could be
        // may differ in exactly that.
        if (descriptor) {
            item->setText(3, qs(surveyio::describeFormat(*descriptor)));
            item->setToolTip(3, item->text(3));
        }
        format_->addItem(name, qs(candidate.formatId));
    }
    if (identified) {
        const auto chosen = detection_->format();
        const int index = chosen ? format_->findData(qs(chosen->id)) : -1;
        format_->setCurrentIndex(std::max(0, index));
    } else {
        format_->setCurrentIndex(0);
    }
    format_->blockSignals(false);
    candidates_->resizeColumnToContents(0);
    showFormatRecord();
    return {};
}

void SurveyImportWizard::showFormatRecord()
{
    const QString id = format_->currentData().toString();
    if (id.isEmpty()) {
        formatRecord_->setText("No format chosen.");
        return;
    }
    const auto descriptor = surveyio::formatRegistry().find(id.toStdString());
    formatRecord_->setText(descriptor ? qs(surveyio::describeFormat(*descriptor))
                                      : "Not a registered format: " + id);
}

Status SurveyImportWizard::chooseFormat()
{
    const QString id = format_->currentData().toString();
    if (id.isEmpty()) {
        return makeError(ErrorCode::InvalidState,
                         "the file's format is not certain: choose the format to read it with");
    }
    const auto descriptor = surveyio::formatRegistry().find(id.toStdString());
    if (!descriptor) {
        return descriptor.error();
    }
    // The one reader this wizard drives. Every other registered format has a
    // descriptor and a probe but no parser yet, so it is refused by name
    // rather than handed to the delimited reader.
    if (descriptor->id != surveyio::kDelimitedPointsFormatId || !descriptor->canImport) {
        return makeError(ErrorCode::Unsupported,
                         "Katana cannot import this format yet: only delimited text points "
                         "have a reader",
                         descriptor->humanName);
    }
    auto proposal = surveyio::proposeLayout(bytes_, false);
    if (!proposal) {
        return proposal.error();
    }
    layoutProposal_ = std::move(*proposal);
    const bool decided = layoutProposal_->outcome() == surveyio::LayoutProposalOutcome::Decided;
    QString text = qs(layoutProposal_->summary());
    if (!decided) {
        text = "Not decided: " + text +
               "\nThe order of the columns is yours to state - check the preview below and "
               "tick the box.";
    }
    proposal_->setText(text);
    proposal_->setStyleSheet(
        QString("color: %1").arg(decided ? theme::textMuted().name() : theme::error().name()));
    confirmOrder_->setChecked(false);
    confirmOrder_->setVisible(!decided);

    candidate_->clear();
    for (const surveyio::LayoutCandidate& candidate : layoutProposal_->candidates()) {
        candidate_->addItem(qs(surveyio::layoutTemplate(candidate.layout)));
        candidate_->setItemData(candidate_->count() - 1, qs(candidate.evidence), Qt::ToolTipRole);
    }
    if (!layoutProposal_->candidates().empty()) {
        applyLayout(layoutProposal_->candidates().front().layout);
    } else {
        schedulePreview();
    }
    return {};
}

// ---- the layout --------------------------------------------------------------------------------

void SurveyImportWizard::applyLayout(const surveyio::DelimitedLayout& layout)
{
    columns_->setText(columnLetters(layout));
    delimiter_->setCurrentIndex(std::max(0, delimiter_->findText(surveyio::toString(layout.delimiter))));
    headerLines_->setValue(static_cast<int>(layout.headerLines));
    comment_->setText(qs(layout.commentPrefix));
    quoting_->setCurrentIndex(std::max(0, quoting_->findText(surveyio::toString(layout.quoting))));
    schedulePreview();
}

Result<surveyio::DelimitedLayout> SurveyImportWizard::layoutFromFields() const
{
    // The fields are assembled into surveyio's own template text and read by
    // its parser, so what this page accepts is exactly what a saved template
    // may hold, and a malformed column list is refused by the same rules.
    QString text = columns_->text().trimmed() + ";delimiter=" + delimiter_->currentText() +
                   ";header=" + QString::number(headerLines_->value()) +
                   ";quote=" + quoting_->currentText();
    const QString comment = comment_->text().trimmed();
    if (!comment.isEmpty()) {
        text += ";comment=" + comment;
    }
    return surveyio::parseLayoutTemplate(text.toStdString());
}

void SurveyImportWizard::schedulePreview()
{
    if (previewPending_) {
        return;
    }
    previewPending_ = true;
    QTimer::singleShot(0, this, [this] {
        previewPending_ = false;
        preview();
    });
}

void SurveyImportWizard::preview()
{
    const auto layout = layoutFromFields();
    const auto fail = [this](const katana::core::Error& error) {
        previewFailed_ = true;
        parseError_->setStyleSheet(QString("color: %1").arg(theme::error().name()));
        parseError_->setText(qs(error.describe()));
        preview_->setRowCount(0);
    };

    // A role box per column, rebuilt only when the number of columns changes.
    // The boxes' own signals lead here, which is why this runs on the event
    // loop: a box is never deleted inside its own signal.
    std::vector<surveyio::ColumnRole> columns;
    if (layout) {
        columns = layout->columns;
    }
    if (static_cast<std::size_t>(roleRow_->count()) != columns.size()) {
        while (QLayoutItem* item = roleRow_->takeAt(0)) {
            delete item->widget();
            delete item;
        }
        for (std::size_t i = 0; i < columns.size(); ++i) {
            auto* box = new QComboBox(columnRoles_);
            box->setObjectName(QString("role%1").arg(i + 1));
            box->setToolTip(QString("What column %1 holds").arg(i + 1));
            for (const surveyio::ColumnRole role : roles()) {
                box->addItem(surveyio::toString(role));
            }
            connect(box, &QComboBox::activated, this, [this] { rolesChanged(); });
            roleRow_->addWidget(box);
        }
    }
    for (std::size_t i = 0; i < columns.size(); ++i) {
        auto* box = qobject_cast<QComboBox*>(roleRow_->itemAt(static_cast<int>(i))->widget());
        box->setCurrentIndex(box->findText(surveyio::toString(columns[i])));
    }

    if (!layout) {
        fail(layout.error());
        return;
    }
    if (bytes_.empty()) {
        previewFailed_ = true;
        parseError_->clear();
        return;
    }
    // The numbers as the file writes them: the unit is chosen on the next
    // page, and metres converts nothing.
    surveyio::DelimitedImportOptions options;
    options.unit = survey::LinearUnit::Metres;
    const auto parsed = surveyio::parseDelimitedPoints(bytes_, *layout, fileName_, options);
    if (!parsed) {
        fail(parsed.error());
        return;
    }
    const auto& points = parsed->project.points;
    const int rows = static_cast<int>(std::min<std::size_t>(points.size(), kPreviewRows));
    preview_->setRowCount(rows);
    for (int row = 0; row < rows; ++row) {
        const survey::SurveyPoint& point = points[static_cast<std::size_t>(row)];
        const QStringList cells{
            QString::number(point.source.recordNumber),
            qs(point.id),
            qs(katana::core::formatExactReal(point.easting)),
            qs(katana::core::formatExactReal(point.northing)),
            point.elevation ? qs(katana::core::formatExactReal(*point.elevation)) : QString(),
            qs(point.code),
            qs(point.description)};
        for (int column = 0; column < cells.size(); ++column) {
            preview_->setItem(row, column, new QTableWidgetItem(cells[column]));
        }
    }
    preview_->resizeColumnsToContents();
    parseError_->setStyleSheet(QString("color: %1").arg(theme::textMuted().name()));
    parseError_->setText(QString("%1 points read; the first %2 shown, as written in the file.")
                             .arg(points.size())
                             .arg(rows));
    previewFailed_ = false;
}

void SurveyImportWizard::rolesChanged()
{
    QStringList letters;
    for (int i = 0; i < roleRow_->count(); ++i) {
        auto* box = qobject_cast<QComboBox*>(roleRow_->itemAt(i)->widget());
        letters << QString(QChar(surveyio::templateLetter(roles()[static_cast<std::size_t>(
                       std::max(0, box->currentIndex()))])));
    }
    columns_->setText(letters.join(','));
}

// ---- reading, transforming, reporting, importing ---------------------------------------------

Status SurveyImportWizard::parseAndTransform()
{
    const int unitIndex = unit_->currentIndex();
    if (unitIndex <= 0) {
        return makeError(ErrorCode::InvalidArgument,
                         "choose the unit the file's numbers are in - there is no default");
    }
    const auto layout = layoutFromFields();
    if (!layout) {
        return layout.error();
    }
    surveyio::DelimitedImportOptions options;
    options.unit = units()[static_cast<std::size_t>(unitIndex - 1)];
    auto parsed = surveyio::parseDelimitedPoints(bytes_, *layout, fileName_, options);
    if (!parsed) {
        return parsed.error();
    }

    const auto declared = epsgField(declared_, "the file's system");
    if (!declared) {
        return declared.error();
    }
    const auto target = epsgField(target_, "transform into");
    if (!target) {
        return target.error();
    }
    if (*target && !*declared) {
        return makeError(ErrorCode::InvalidArgument,
                         "a transformation needs the file's system as well as the target: "
                         "Katana never assumes what a file is in");
    }
    survey::SurveyProject project = parsed->project;
    systemText_ = "unknown (none stated)";
    transformText_.clear();
    if (*declared) {
        auto system = cad::declaredSystemFromEpsg(**declared);
        if (!system) {
            return system.error();
        }
        project.coordinateSystem = *system;
        systemText_ = "EPSG:" + std::to_string(**declared) + " " + system->name + " (stated)";
    }
    if (*target) {
        auto moved = cad::transformSurveyProject(std::move(project), **declared, **target);
        if (!moved) {
            return moved.error();
        }
        project = std::move(*moved);
        transformText_ = "from EPSG:" + std::to_string(**declared) + " to EPSG:" +
                         std::to_string(**target) + " " + project.coordinateSystem.name +
                         " by " + project.metadata["transformation"] +
                         "; heights unchanged";
    }
    layoutText_ = surveyio::layoutTemplate(*layout);
    parsed_ = std::move(*parsed);
    project_ = std::move(project);
    return {};
}

void SurveyImportWizard::prepareReport()
{
    cad::SurveyImportOptions options;
    options.layer = layer_->text().trimmed().toStdString();
    options.layerPerCode = layerPerCode_->isChecked();
    const auto policy = policies()[static_cast<std::size_t>(std::max(0, existing_->currentIndex()))];

    cad::SurveyImportSummary summary;
    summary.fileName = fileName_;
    const auto descriptor = surveyio::formatRegistry().find(parsed_->formatId);
    summary.format = descriptor ? descriptor->humanName + " (parser " +
                                      descriptor->parserVersion + ")"
                                : parsed_->formatId;
    summary.layout = layoutText_;
    summary.recordsRead = parsed_->recordsRead;
    summary.readerWarnings = parsed_->warnings;
    summary.coordinateSystem = systemText_;
    summary.transformation = transformText_;
    summary.units = survey::toString(project_->units.linear);
    summary.policy = policy;

    cad::SurveyPointImportReport report;
    const auto command =
        cad::importSurveyPoints(*context_.document, *project_, options, policy, &report);
    blocked_ = !command || *command == nullptr;
    std::string text;
    if (!command) {
        text = cad::formatSurveyImportReport(summary, report, &command.error());
    } else {
        text = cad::formatSurveyImportReport(summary, report);
        if (*command == nullptr) {
            text += "Nothing to import.\n";
        }
    }
    report_->setPlainText(qs(text));
}

void SurveyImportWizard::importNow()
{
    if (!project_) {
        return;
    }
    cad::SurveyImportOptions options;
    options.layer = layer_->text().trimmed().toStdString();
    options.layerPerCode = layerPerCode_->isChecked();
    const auto policy = policies()[static_cast<std::size_t>(std::max(0, existing_->currentIndex()))];
    // Built again rather than kept from the report: the dialog is not modal,
    // and the drawing may have changed since the report was written.
    cad::SurveyPointImportReport report;
    auto command = cad::importSurveyPoints(*context_.document, *project_, options, policy, &report);
    if (!command) {
        showError(command.error());
        return;
    }
    if (*command == nullptr) {
        showError(katana::core::Error{ErrorCode::InvalidState, "nothing to import", {}});
        return;
    }
    cad::Document& document = *context_.document;
    if (const auto status = document.execute(std::move(*command)); !status) {
        showError(status.error());
        return;
    }
    const std::vector<katana::entity::EntityId> created = document.lastCreatedEntities();
    context_.log(report_->toPlainText().trimmed(), false);
    context_.log(QString("Imported %1 point(s) from %2 on %3: one command - Undo removes it.")
                     .arg(report.import.points)
                     .arg(qs(fileName_), qs(options.layer)),
                 false);
    if (context_.views != nullptr) {
        context_.views->zoomExtentsAll();
    }
    // The window's own action, on the imported points alone: it acts on the
    // selection when there is one.
    if (applyCodes_->isChecked() && applyCodes_->isEnabled() &&
        context_.applySurveyCodes != nullptr) {
        document.selection().set(created);
        document.notifySelectionChanged();
        context_.applySurveyCodes->trigger();
    }
    // Done, as a wizard's Finish is: the report is in the log, and the next
    // Import Survey Points starts at the first page with the fields kept.
    blocked_ = true;
    goTo(FilePage);
    hide();
}

void SurveyImportWizard::showError(const katana::core::Error& error)
{
    QString text = qs(error.message);
    if (!error.context.empty()) {
        text += " - " + qs(error.context);
    }
    message_->setStyleSheet(QString("color: %1").arg(theme::error().name()));
    message_->setText(text);
    context_.log("Import Survey Points: " + qs(error.describe()), true);
}

void SurveyImportWizard::showMessage(const QString& text)
{
    message_->setStyleSheet(QString("color: %1").arg(theme::textMuted().name()));
    message_->setText(text);
}

} // namespace katana::qt
