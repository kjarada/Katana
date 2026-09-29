#include "survey/survey_import_wizard.hpp"

#include <QAction>
#include <QCheckBox>
#include <QComboBox>
#include <QDateTime>
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
#include <QScrollArea>
#include <QSpinBox>
#include <QSplitter>
#include <QStackedWidget>
#include <QTableWidget>
#include <QTimer>
#include <QTreeWidget>
#include <QVBoxLayout>

#include <algorithm>
#include <filesystem>
#include <format>
#include <iterator>
#include <set>
#include <string>
#include <string_view>
#include <utility>
#include <variant>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/survey_job.hpp"
#include "katana/cad/survey_points.hpp"
#include "katana/core/text.hpp"
#include "katana/surveyio/format.hpp"
#include "katana/surveyio/reader.hpp"
#include "survey/reduction_options_widget.hpp"
#include "survey/reduction_report_view.hpp"
#include "survey/survey_job_support.hpp"
#include "survey/survey_task.hpp"
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

// The column list as typed: one trimmed entry per comma-separated part, none
// for a blank field. NOT validated, because the role boxes follow it while it
// is not yet a layout surveyio accepts - two easting columns half way through
// swapping northing and easting, say. Boxes built from the parsed layout
// vanished at the first such step, and with them the only way to swap.
QStringList typedColumns(const QLineEdit& field)
{
    const QString text = field.text().trimmed();
    if (text.isEmpty()) {
        return {};
    }
    QStringList entries = text.split(',');
    for (QString& entry : entries) {
        entry = entry.trimmed();
    }
    return entries;
}

// The index in roles() of the role an entry's letter spells, matched without
// regard to case as templates are; -1 for an entry that is not one letter
// surveyio knows (its box is left blank, and the parser names the entry).
// Found through surveyio::templateLetter, so the letters are spelt in one
// place.
int roleIndexOf(const QString& entry)
{
    if (entry.size() != 1) {
        return -1;
    }
    const char letter = katana::core::asciiLower(entry.front().toLatin1());
    for (std::size_t i = 0; i < roles().size(); ++i) {
        if (katana::core::asciiLower(surveyio::templateLetter(roles()[i])) == letter) {
            return static_cast<int>(i);
        }
    }
    return -1;
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

// "50.2 MB", "812 KB": a file's size as the busy row says it.
QString sizeText(std::int64_t bytes)
{
    if (bytes >= 1'000'000) {
        return QString("%1 MB").arg(static_cast<double>(bytes) / 1e6, 0, 'f', 1);
    }
    return QString("%1 KB").arg(std::max<std::int64_t>(1, (bytes + 999) / 1000));
}

QString unitsText(const survey::DeclaredUnits& units)
{
    const auto angular = [](survey::AngularUnit unit) -> QString {
        switch (unit) {
        case survey::AngularUnit::Unknown:
            return "angles not stated";
        default:
            return QString("angles in %1").arg(qs(survey::toString(unit)));
        }
    };
    const QString linear = units.linear == survey::LinearUnit::Unknown
                               ? QString("lengths not stated")
                               : QString("lengths in %1").arg(qs(survey::toString(units.linear)));
    return linear + ", " + angular(units.angular);
}

QTreeWidgetItem* contentRow(QTreeWidget* tree, const QString& what, const QString& detail)
{
    auto* item = new QTreeWidgetItem(tree);
    item->setText(0, what);
    item->setText(1, detail);
    item->setToolTip(0, what);
    item->setToolTip(1, detail);
    return item;
}

QTreeWidgetItem* contentChild(QTreeWidgetItem* parent, const QString& what, const QString& detail)
{
    auto* item = new QTreeWidgetItem(parent);
    item->setText(0, what);
    item->setText(1, detail);
    item->setToolTip(0, what);
    item->setToolTip(1, detail);
    return item;
}

// Warnings listed one by one on the Content step; the rest are counted. A
// damaged file can have hundreds of thousands, and the list is for reading.
constexpr std::size_t kListedWarnings = 1'000;

} // namespace

SurveyContent surveyContentOf(const survey::SurveyProject& project)
{
    SurveyContent content;
    content.setups = project.stations.size();
    const auto count = [&content](const survey::Observation& observation) {
        ++content.observations;
        ++content.observationsByKind[survey::observationKindName(observation)];
    };
    for (const survey::SurveyStation& station : project.stations) {
        for (const survey::Observation& observation : station.observations) {
            count(observation);
        }
    }
    for (const survey::Observation& observation : project.observations) {
        count(observation);
    }
    content.positionedPoints = project.points.size();
    content.unpositionedPoints = project.unpositionedPoints.size();
    content.features = project.features.size();
    content.controlPoints = project.controlPoints.size();
    content.gnssSessions = project.gnssSessions.size();
    content.traverses = project.traverses.size();
    return content;
}

QString surveyFileFilter()
{
    // What no descriptor states: point lists named .pnt and .xyz, which the
    // delimited reader takes by content; GTS-6's .gt6; the numbered files of
    // a DBX job (.x01); and RINEX 2's names, whose extension is the year's
    // last two digits and the file's type (.24o, .24d).
    static constexpr std::string_view kUnregistered[] = {"*.pnt", "*.xyz", "*.gt6",
                                                         "*.x01", "*.??o", "*.??d"};
    std::set<std::string> patterns(std::begin(kUnregistered), std::end(kUnregistered));
    for (const surveyio::FormatDescriptor& descriptor : surveyio::formatRegistry().formats()) {
        for (const std::string& extension : descriptor.extensions) {
            patterns.insert("*." + extension);
        }
    }
    QStringList listed;
    for (const std::string& pattern : patterns) {
        listed << qs(pattern);
    }
    return "Survey files (" + listed.join(' ') +
           ");;Point files (*.csv *.txt *.tsv *.pnt *.xyz);;All files (*)";
}

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
    // In the order of the Page enum; path() says which a file goes through.
    pages_->addWidget(buildFilePage());
    pages_->addWidget(buildFormatPage());
    pages_->addWidget(buildLayoutPage());
    pages_->addWidget(buildContentPage());
    pages_->addWidget(buildSystemPage());
    pages_->addWidget(buildReductionPage());
    pages_->addWidget(buildOptionsPage());
    pages_->addWidget(buildReportPage());
    layout->addWidget(pages_, 1);

    task_ = new SurveyTaskBar(this);
    layout->addWidget(task_);

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

    connect(back_, &QPushButton::clicked, this,
            [this] { goTo(previousPage(pages_->currentIndex())); });
    connect(next_, &QPushButton::clicked, this, [this] { advance(); });
    connect(import_, &QPushButton::clicked, this, [this] { importNow(); });
    connect(close, &QPushButton::clicked, this, [this] { hide(); });
    // While a file is read or reduced, nothing that would start another or
    // leave the page is pressed; Cancel is.
    task_->onBusyChanged = [this](bool busy) {
        const int page = pages_->currentIndex();
        back_->setEnabled(!busy && page > FilePage);
        next_->setEnabled(!busy && page < ReportPage);
        import_->setEnabled(!busy && page == ReportPage && !blocked_);
        previewButton_->setEnabled(!busy);
    };
    task_->onCancelled = [this] { showMessage("Cancelled: nothing was changed."); };

    goTo(FilePage);
    resize(960, 760);
}

// ---- the pages ---------------------------------------------------------------------------

QWidget* SurveyImportWizard::buildFilePage()
{
    auto* page = new QWidget(this);
    auto* layout = new QVBoxLayout(page);
    layout->addWidget(mutedLabel(
        "Choose a file of surveyed points, or a field file from an instrument or a GNSS "
        "receiver. The next step says what Katana makes of it and, when it cannot be sure, asks "
        "you: a file is never read with a format nobody chose.",
        page));
    auto* row = new QHBoxLayout();
    file_ = new QLineEdit(page);
    file_->setObjectName("file");
    file_->setPlaceholderText("The file to import");
    auto* browse = new QPushButton("Browse...", page);
    browse->setObjectName("browse");
    browse->setAutoDefault(false);
    connect(browse, &QPushButton::clicked, this, [this] {
        const QString path = QFileDialog::getOpenFileName(this, "Import Survey Points",
                                                          file_->text(), surveyFileFilter());
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
    connect(format_, &QComboBox::currentIndexChanged, this, [this] {
        showFormatRecord();
        // The steps ahead depend on the format: seven for a field file, six
        // for a coordinate file.
        if (pages_ != nullptr && pages_->currentIndex() == FormatPage) {
            showStepTitle();
        }
    });
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
    keepTemplateChoiceCurrent(*template_);
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
        // The list was refilled by the save (keepTemplateChoiceCurrent);
        // the new name is chosen without re-applying what the fields hold.
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

QWidget* SurveyImportWizard::buildContentPage()
{
    auto* page = new QWidget(this);
    auto* layout = new QVBoxLayout(page);
    contentSummary_ = new QLabel(page);
    contentSummary_->setObjectName("contentSummary");
    contentSummary_->setWordWrap(true);
    layout->addWidget(contentSummary_);
    content_ = new QTreeWidget(page);
    content_->setObjectName("content");
    content_->setHeaderLabels({"In the file", "What was read"});
    // The sentences - what the file does not carry, each warning - are in
    // the first column and wrap in it; the counts keep a column of their own
    // that a long sentence can never push out of sight.
    content_->header()->setStretchLastSection(false);
    content_->header()->setSectionResizeMode(0, QHeaderView::Stretch);
    content_->header()->setSectionResizeMode(1, QHeaderView::Interactive);
    content_->setColumnWidth(1, 280);
    content_->setWordWrap(true);
    content_->setTextElideMode(Qt::ElideNone);
    content_->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    // Rows are measured for the width they have: a column made wider or
    // narrower lays them out again, so a wrapped sentence keeps its lines.
    connect(content_->header(), &QHeaderView::sectionResized, content_,
            [this] { content_->doItemsLayout(); });
    layout->addWidget(content_, 1);
    layout->addWidget(mutedLabel(
        "What the reader made of the file, before anything is reduced or drawn. Every record "
        "it could not read is a warning naming the record; nothing is dropped without a word.",
        page));
    return page;
}

QWidget* SurveyImportWizard::buildReductionPage()
{
    auto* page = new QWidget(this);
    auto* layout = new QVBoxLayout(page);
    auto* splitter = new QSplitter(Qt::Vertical, page);
    splitter->setChildrenCollapsible(false);
    reductionSplitter_ = splitter;
    auto* scroll = new QScrollArea(splitter);
    scroll->setWidgetResizable(true);
    scroll->setFrameShape(QFrame::NoFrame);
    scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    options_ = new ReductionOptionsWidget(scroll);
    scroll->setWidget(options_);
    auto* previewPane = new QWidget(splitter);
    auto* previewLayout = new QVBoxLayout(previewPane);
    previewLayout->setContentsMargins(0, 0, 0, 0);
    auto* row = new QHBoxLayout();
    previewButton_ = new QPushButton("Preview", previewPane);
    previewButton_->setObjectName("previewReduction");
    previewButton_->setAutoDefault(false);
    previewButton_->setToolTip("Reduce and adjust with these options and show the report here; "
                               "nothing is drawn");
    row->addWidget(previewButton_);
    row->addWidget(mutedLabel("Runs the reduction as the import would and shows its report "
                              "below. Nothing is drawn until Import.",
                              previewPane),
                   1);
    previewLayout->addLayout(row);
    previewReport_ = new ReductionReportView("previewReport", previewPane);
    previewLayout->addWidget(previewReport_, 1);
    splitter->addWidget(scroll);
    splitter->addWidget(previewPane);
    splitter->setStretchFactor(0, 1);
    splitter->setStretchFactor(1, 1);
    // The options first get most of the page - the basic ones and the
    // control table in view, the Advanced fold just below; a preview then
    // gives the room to its report (runPreview).
    splitter->setSizes({500, 200});
    layout->addWidget(splitter, 1);
    connect(previewButton_, &QPushButton::clicked, this, [this] { runPreview({}); });
    options_->onChanged = [this] {
        if (outcome_ != nullptr && !previewIsCurrent()) {
            previewReport_->showNote("The options have changed since the preview: press Preview "
                                     "to run the reduction again.");
        }
    };
    return page;
}

QWidget* SurveyImportWizard::buildSystemPage()
{
    auto* page = new QWidget(this);
    auto* layout = new QVBoxLayout(page);
    systemSummary_ = new QLabel(page);
    systemSummary_->setObjectName("systemSummary");
    systemSummary_->setWordWrap(true);
    systemSummary_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    systemSummary_->setVisible(false);
    layout->addWidget(systemSummary_);
    // What a coordinate file needs; a field file's reader states its units
    // and nothing of it is transformed, so its path hides all of this.
    systemFields_ = new QWidget(page);
    auto* fieldsLayout = new QVBoxLayout(systemFields_);
    fieldsLayout->setContentsMargins(0, 0, 0, 0);
    fieldsLayout->addWidget(mutedLabel(
        "A point file does not say what its numbers are: choose the unit they are in (there is "
        "no default - a wrong unit scales the whole survey). The coordinate system is recorded "
        "as unknown unless you state it. Katana transforms the points only when you give BOTH "
        "the file's system and a target, by EPSG code, through its geodesy library; both must "
        "be projected (grid) systems, and heights are carried through unchanged - no vertical "
        "datum change is made.",
        systemFields_));
    layout->addWidget(systemFields_);
    layout->addStretch(1);
    QWidget* fields = systemFields_;
    auto* form = new QFormLayout();
    unit_ = new QComboBox(fields);
    unit_->setObjectName("unit");
    unit_->addItem("(choose the unit)");
    for (const survey::LinearUnit unit : units()) {
        unit_->addItem(survey::toString(unit));
    }
    form->addRow("The numbers are in:", unit_);
    declared_ = new QLineEdit(fields);
    declared_->setObjectName("declared");
    declared_->setPlaceholderText("unknown - or an EPSG code, e.g. 28356");
    form->addRow("The file's system:", declared_);
    target_ = new QLineEdit(fields);
    target_->setObjectName("target");
    target_->setPlaceholderText("none - or an EPSG code to transform into");
    form->addRow("Transform into:", target_);
    fieldsLayout->addLayout(form);
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
        "wrong file than a wanted update. Survey codes need a loaded survey code file (Format > "
        "Load Customisation) and are applied to the imported points only, as their own undoable "
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
    auto* splitter = new QSplitter(Qt::Vertical, page);
    report_ = new QPlainTextEdit(splitter);
    report_->setObjectName("report");
    report_->setReadOnly(true);
    report_->setFont(katana::qt::theme::monospaceFont());
    report_->setLineWrapMode(QPlainTextEdit::NoWrap);
    // The reduction report, for a field file: what will happen to the data.
    importReport_ = new ReductionReportView("importReport", splitter);
    importReport_->setVisible(false);
    splitter->addWidget(report_);
    splitter->addWidget(importReport_);
    splitter->setStretchFactor(0, 0);
    splitter->setStretchFactor(1, 1);
    layout->addWidget(splitter, 1);
    return page;
}

void SurveyImportWizard::showEvent(QShowEvent* event)
{
    // Saves and deletes in this Katana refill the list as they happen; this
    // catches a template another Katana saved into the settings meanwhile.
    fillTemplateChoice(*template_);
    QDialog::showEvent(event);
}

// ---- moving between pages --------------------------------------------------------------------

std::vector<int> SurveyImportWizard::path() const
{
    // Until the format is chosen, the path is the one the format on show
    // would take, so the step count does not change under the person's feet
    // between the Format step and the next.
    bool reader = readerPath_;
    if (pages_ != nullptr && pages_->currentIndex() <= FormatPage && format_ != nullptr) {
        const QString id = format_->currentData().toString();
        reader = !id.isEmpty() && surveyio::formatRegistry().reader(id.toStdString()) != nullptr;
    }
    if (reader) {
        return {FilePage,   FormatPage,  ContentPage, SystemPage,
                ReductionPage, OptionsPage, ReportPage};
    }
    return {FilePage, FormatPage, LayoutPage, SystemPage, OptionsPage, ReportPage};
}

int SurveyImportWizard::nextPage(int page) const
{
    const std::vector<int> pages = path();
    const auto at = std::ranges::find(pages, page);
    return at == pages.end() || at + 1 == pages.end() ? page : *(at + 1);
}

int SurveyImportWizard::previousPage(int page) const
{
    const std::vector<int> pages = path();
    const auto at = std::ranges::find(pages, page);
    return at == pages.end() || at == pages.begin() ? FilePage : *(at - 1);
}

void SurveyImportWizard::showStepTitle()
{
    const int page = pages_->currentIndex();
    static const char* const titles[] = {"Choose the file",
                                         "The file's format",
                                         "Columns and delimiter",
                                         "What the file holds",
                                         "Units and coordinate system",
                                         "Reduction and adjustment",
                                         "Options",
                                         "Report"};
    const std::vector<int> pages = path();
    const auto at = std::ranges::find(pages, page);
    const auto number = at == pages.end() ? 0 : (at - pages.begin()) + 1;
    step_->setText(QString("Step %1 of %2: %3")
                       .arg(number)
                       .arg(pages.size())
                       .arg(titles[page]));
}

void SurveyImportWizard::goTo(int page)
{
    page = std::clamp(page, 0, Pages - 1);
    pages_->setCurrentIndex(page);
    showStepTitle();
    const bool busy = task_->busy();
    back_->setEnabled(!busy && page > 0);
    next_->setEnabled(!busy && page < ReportPage);
    import_->setEnabled(!busy && page == ReportPage && !blocked_);
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

void SurveyImportWizard::advance()
{
    if (task_->busy()) {
        return;
    }
    const int page = pages_->currentIndex();
    // The steps whose work may run on a pool thread go on from the work's
    // continuation, not from here.
    if (page == FormatPage) {
        if (const auto status = chooseFormat(); !status) {
            showError(status.error());
            return;
        }
        if (readerPath_) {
            readWithReader();
            return;
        }
        goTo(LayoutPage);
        return;
    }
    if (page == OptionsPage && readerPath_) {
        if (previewIsCurrent()) {
            prepareReaderReport();
            goTo(ReportPage);
            return;
        }
        runPreview([this] {
            prepareReaderReport();
            goTo(ReportPage);
        });
        return;
    }
    if (const auto status = leave(page); !status) {
        showError(status.error());
        return;
    }
    goTo(nextPage(page));
}

Status SurveyImportWizard::leave(int page)
{
    switch (page) {
    case FilePage:
        return readFile();
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
    case ContentPage:
        prepareSystemForReader();
        return {};
    case SystemPage:
        if (readerPath_) {
            prepareReduction();
            return {};
        }
        return parseAndTransform();
    case ReductionPage: {
        // The options must make sense before the page is left; the reduction
        // itself runs on Preview or on the way to the Report step.
        const auto settings = options_->settings();
        if (!settings) {
            return settings.error();
        }
        return {};
    }
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
    // A large file is not read here, on the GUI thread: detection needs only
    // its head, and its reader reads it whole on a pool thread (Content).
    const qint64 size = file.size();
    bytesComplete_ = size <= kBackgroundReadBytes;
    const QByteArray bytes =
        bytesComplete_ ? file.readAll()
                       : file.read(static_cast<qint64>(surveyio::kProbeBytes));
    bytes_.assign(bytes.constData(), static_cast<std::size_t>(bytes.size()));
    fileName_ = QFileInfo(path).fileName().toStdString();
    parsed_.reset();
    project_.reset();
    read_.reset();
    raw_.reset();
    sourceBytes_.reset();
    outcome_.reset();
    ++readGeneration_;

    const std::string_view sample =
        std::string_view(bytes_).substr(0, std::min(bytes_.size(), surveyio::kProbeBytes));
    detection_ = surveyio::detectFormat(
        surveyio::probeOf(sample, fileName_, size > static_cast<qint64>(surveyio::kProbeBytes)));
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
    // Every registered format, the candidates first with their evidence:
    // what Katana can read at all is worth seeing when the file is not what
    // the person thought, and each line carries its record - what the format
    // carries and the parser's version.
    const auto addRow = [this](const surveyio::FormatDescriptor& descriptor,
                               const QString& confidence, const QString& evidence) {
        auto* item = new QTreeWidgetItem(candidates_);
        item->setText(0, qs(descriptor.humanName));
        item->setText(1, confidence);
        item->setText(2, evidence);
        item->setToolTip(2, evidence);
        item->setText(3, qs(surveyio::describeFormat(descriptor)));
        item->setToolTip(3, item->text(3));
        return item;
    };
    std::set<std::string> listed;
    for (const surveyio::FormatCandidate& candidate : detection_->candidates()) {
        const auto descriptor = surveyio::formatRegistry().find(candidate.formatId);
        const QString name = descriptor ? qs(descriptor->humanName) : qs(candidate.formatId);
        if (descriptor) {
            addRow(*descriptor, QString::number(candidate.confidence, 'f', 2),
                   qs(candidate.evidence));
        }
        listed.insert(candidate.formatId);
        format_->addItem(name, qs(candidate.formatId));
    }
    for (const surveyio::FormatDescriptor& descriptor : surveyio::formatRegistry().formats()) {
        if (listed.contains(descriptor.id)) {
            continue;
        }
        QTreeWidgetItem* item = addRow(descriptor, "-", "not detected in this file");
        item->setForeground(0, theme::textMuted());
        item->setForeground(2, theme::textMuted());
        // Still choosable: a detection can be wrong, and the choice is the
        // person's. A format that cannot import is listed, not offered.
        if (descriptor.canImport) {
            format_->addItem(qs(descriptor.humanName), qs(descriptor.id));
        }
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
    if (!descriptor->canImport) {
        return makeError(ErrorCode::Unsupported, "Katana cannot import this format",
                         descriptor->humanName);
    }
    // A format with a reader of its own is read whole by it (the instrument
    // path); delimited points are read through the layout the person
    // confirms. Anything else is refused by name, never handed to the
    // delimited reader.
    if (surveyio::formatRegistry().reader(descriptor->id) != nullptr) {
        readerPath_ = true;
        formatId_ = descriptor->id;
        return {};
    }
    if (descriptor->id != surveyio::kDelimitedPointsFormatId) {
        return makeError(ErrorCode::Unsupported,
                         "Katana has no reader for this format: export the points as a "
                         "delimited text file instead",
                         descriptor->humanName);
    }
    readerPath_ = false;
    formatId_ = descriptor->id;
    systemSummary_->setVisible(false);
    systemFields_->setVisible(true);
    for (QWidget* field : std::initializer_list<QWidget*>{unit_, declared_, target_}) {
        field->setEnabled(true);
    }
    importReport_->setVisible(false);
    if (!bytesComplete_) {
        // A large coordinate file: the layout page needs all of it, read here
        // as it always was.
        QFile file(file_->text().trimmed());
        if (!file.open(QIODevice::ReadOnly)) {
            return makeError(ErrorCode::FileImportFailure, "the file cannot be opened",
                             fileName_ + ": " + file.errorString().toStdString());
        }
        const QByteArray bytes = file.readAll();
        bytes_.assign(bytes.constData(), static_cast<std::size_t>(bytes.size()));
        bytesComplete_ = true;
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

    // A role box per entry of the column list AS TYPED (typedColumns), rebuilt
    // only when the number of entries changes - never because the layout does
    // not validate, since swapping two columns passes through a layout that
    // does not. The boxes' own signals lead here, which is why this runs on
    // the event loop: a box is never deleted inside its own signal.
    const QStringList entries = typedColumns(*columns_);
    if (roleRow_->count() != entries.size()) {
        while (QLayoutItem* item = roleRow_->takeAt(0)) {
            delete item->widget();
            delete item;
        }
        for (int i = 0; i < entries.size(); ++i) {
            auto* box = new QComboBox(columnRoles_);
            box->setObjectName(QString("role%1").arg(i + 1));
            box->setToolTip(QString("What column %1 holds").arg(i + 1));
            for (const surveyio::ColumnRole role : roles()) {
                box->addItem(surveyio::toString(role));
            }
            // Any change of the choice, not only a click (activated): the
            // headless --fill chooses with setCurrentIndex, and a box that
            // ignored it would be a box no test could drive. The guard keeps
            // out the changes made below, which only mirror the text.
            connect(box, &QComboBox::currentIndexChanged, this, [this, i] {
                if (!settingRoles_) {
                    rolesChanged(i);
                }
            });
            roleRow_->addWidget(box);
        }
    }
    settingRoles_ = true;
    for (int i = 0; i < entries.size(); ++i) {
        auto* box = qobject_cast<QComboBox*>(roleRow_->itemAt(i)->widget());
        box->setCurrentIndex(roleIndexOf(entries[i]));
    }
    settingRoles_ = false;

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

void SurveyImportWizard::rolesChanged(int column)
{
    // Only the entry of the box that changed is rewritten. The others stay as
    // typed, so an entry no box can show - a letter surveyio does not know,
    // shown as a blank box - is kept for the parser to name rather than
    // silently read as the first role in the list.
    QStringList entries = typedColumns(*columns_);
    if (column >= entries.size() || column >= roleRow_->count()) {
        return;
    }
    const auto* box = qobject_cast<QComboBox*>(roleRow_->itemAt(column)->widget());
    if (box == nullptr || box->currentIndex() < 0) {
        return;
    }
    entries[column] = QString(QChar(
        surveyio::templateLetter(roles()[static_cast<std::size_t>(box->currentIndex())])));
    columns_->setText(entries.join(','));
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
    if (readerPath_) {
        importJob();
        return;
    }
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

// ---- the instrument path -----------------------------------------------------------------------

void SurveyImportWizard::readWithReader()
{
    const QString path = file_->text().trimmed();
    const QFileInfo info(path);
    const std::string name = fileName_;
    const std::string id = formatId_;
    // The bytes the File step read, when it read them all; a large file is
    // read on the pool thread instead.
    std::shared_ptr<const std::string> have =
        bytesComplete_ ? std::make_shared<const std::string>(bytes_) : nullptr;
    const std::filesystem::path folder(info.absolutePath().toStdWString());
    const std::uint64_t generation = ++readGeneration_;
    const bool background = info.size() > kBackgroundReadBytes;
    const auto descriptor = surveyio::formatRegistry().find(id);
    const QString format = descriptor ? qs(descriptor->humanName) : qs(id);
    message_->clear();
    // Runs on a pool thread for a large file: it touches nothing of this
    // dialog or the drawing, only what it was given, until its continuation.
    task_->run(
        QString("Reading %1 (%2) as %3").arg(qs(name), sizeText(info.size()), format), background,
        [this, path, name, id, have, folder, generation]() -> SurveyTaskBar::Finish {
            std::shared_ptr<const std::string> bytes = have;
            if (bytes == nullptr) {
                QFile file(path);
                if (!file.open(QIODevice::ReadOnly)) {
                    return [this, error = makeError(ErrorCode::FileImportFailure,
                                                    "the file cannot be opened",
                                                    path.toStdString() + ": " +
                                                        file.errorString().toStdString())] {
                        showError(error);
                    };
                }
                auto whole = std::make_shared<std::string>();
                whole->resize(static_cast<std::size_t>(file.size()));
                const qint64 got = file.read(whole->data(), static_cast<qint64>(whole->size()));
                whole->resize(static_cast<std::size_t>(std::max<qint64>(got, 0)));
                bytes = std::move(whole);
            }
            surveyio::ReadOptions options;
            // The job's other files - a DBX folder, a RINEX navigation file -
            // by name, from the folder the file is in and nowhere else.
            options.siblings = surveyio::siblingsInFolder(folder);
            auto read =
                surveyio::readSurvey(surveyio::formatRegistry(), id, *bytes, name, options);
            if (!read) {
                return [this, error = read.error(), generation] {
                    if (generation == readGeneration_) {
                        showError(error);
                    }
                };
            }
            auto result = std::make_shared<surveyio::ReadResult>(std::move(*read));
            SurveyContent counts = surveyContentOf(result->project);
            return [this, bytes, result, counts, generation] {
                if (generation != readGeneration_) {
                    return;
                }
                sourceBytes_ = bytes;
                read_ = result;
                // The project inside the result, kept alive by it.
                raw_ = std::shared_ptr<const survey::SurveyProject>(result, &result->project);
                contentCounts_ = counts;
                outcome_.reset();
                showContent();
                goTo(ContentPage);
            };
        });
}

void SurveyImportWizard::showContent()
{
    const surveyio::ReadResult& read = *read_;
    const SurveyContent& counts = contentCounts_;
    const auto descriptor = surveyio::formatRegistry().find(read.formatId);
    contentSummary_->setText(
        QString("%1 (parser %2) read %3 record(s) of %4; %5")
            .arg(descriptor ? qs(descriptor->humanName) : qs(read.formatId),
                 qs(read.parserVersion))
            .arg(read.recordsRead)
            .arg(qs(fileName_))
            .arg(read.recordsSkipped == 0
                     ? QString("none skipped.")
                     : QString("%1 skipped, each with a warning below.").arg(read.recordsSkipped)));
    content_->clear();
    const auto number = [](std::size_t n) { return QString::number(n); };
    contentRow(content_, "Setups", number(counts.setups));
    QTreeWidgetItem* observations =
        contentRow(content_, "Observations", number(counts.observations));
    for (const auto& [kind, n] : counts.observationsByKind) {
        contentChild(observations, qs(kind), number(n));
    }
    contentRow(content_, "Points with coordinates", number(counts.positionedPoints));
    contentRow(content_, "Points without coordinates", number(counts.unpositionedPoints));
    contentRow(content_, "Coded features", number(counts.features));
    QTreeWidgetItem* control =
        contentRow(content_, "Control the file declares", number(counts.controlPoints));
    for (const survey::ControlPoint& point : raw_->controlPoints) {
        contentChild(control, qs(point.pointId), QString());
    }
    if (counts.traverses > 0) {
        contentRow(content_, "Traverses", number(counts.traverses));
    }
    QTreeWidgetItem* sessions =
        contentRow(content_, "GNSS sessions", number(counts.gnssSessions));
    for (const survey::GnssSession& session : raw_->gnssSessions) {
        QString detail = QString("receiver %1, antenna %2, %3 epoch(s)")
                             .arg(session.receiverType.empty() ? QString("not stated")
                                                               : qs(session.receiverType),
                                  session.antenna.type.empty() ? QString("not stated")
                                                               : qs(session.antenna.type))
                             .arg(session.epochCount);
        if (session.firstEpoch.year != 0) {
            detail += QString(", %1 to %2").arg(qs(survey::toString(session.firstEpoch)),
                                                qs(survey::toString(session.lastEpoch)));
        }
        contentChild(sessions,
                     (session.markerName.empty() ? QString("(no marker name)")
                                                 : qs(session.markerName)) +
                         ": " + detail,
                     QString("%1 epoch(s)").arg(session.epochCount));
    }
    QTreeWidgetItem* siblings =
        contentRow(content_, "Files read beside it", number(read.siblingsRead.size()));
    for (const surveyio::SiblingFile& sibling : read.siblingsRead) {
        contentChild(siblings, qs(sibling.name),
                     sizeText(static_cast<std::int64_t>(sibling.bytes.size())));
    }
    contentRow(content_, "Units the file states", unitsText(raw_->units));
    contentRow(content_, "Coordinate system the file declares",
               raw_->coordinateSystem.unknown ? QString("not stated")
                                              : qs(raw_->coordinateSystem.name));
    QTreeWidgetItem* missing =
        contentRow(content_, "Not in the file", number(read.notCarried.size()));
    for (const std::string& line : read.notCarried) {
        contentChild(missing, qs(line), QString());
    }
    QTreeWidgetItem* warnings = contentRow(content_, "Warnings", number(read.warnings.size()));
    const std::size_t listed = std::min(read.warnings.size(), kListedWarnings);
    for (std::size_t i = 0; i < listed; ++i) {
        const surveyio::ReadWarning& warning = read.warnings[i];
        contentChild(warnings, qs(warning.message),
                     QString("%1 record %2").arg(qs(warning.fileName)).arg(warning.record));
    }
    if (listed < read.warnings.size()) {
        contentChild(warnings,
                     QString("%1 more, in the reduction report")
                         .arg(read.warnings.size() - listed),
                     "...");
    }
    if (!read.warnings.empty()) {
        warnings->setForeground(0, theme::error());
        warnings->setForeground(1, theme::error());
    }
    // Open what there is to read; the counts alone say the rest.
    for (QTreeWidgetItem* item : {observations, missing, warnings, siblings}) {
        item->setExpanded(item->childCount() > 0 && item->childCount() <= 50);
    }
}

void SurveyImportWizard::prepareSystemForReader()
{
    // The reader states the file's units and has converted to metres and
    // radians; nothing is transformed - the observations are reduced into the
    // drawing's system, which is what the reduction's scale factor, geoid and
    // GNSS conversions come from.
    for (QWidget* field : std::initializer_list<QWidget*>{unit_, declared_, target_}) {
        field->setEnabled(false);
    }
    systemFields_->setVisible(false);
    const std::string& drawing = context_.document->metadata().coordinateSystem;
    systemSummary_->setText(
        QString("The file's own units: %1 - converted to metres and radians as it was read.\n"
                "The file declares: %2.\n"
                "The observations are reduced into the drawing's coordinate system: %3.\n"
                "Nothing is transformed.")
            .arg(unitsText(raw_->units),
                 raw_->coordinateSystem.unknown ? QString("no coordinate system")
                                                : qs(raw_->coordinateSystem.name),
                 drawing.empty() ? QString("none set - a local drawing, so no scale factor from "
                                           "a projection, geoid or GNSS conversion is available")
                                 : qs(drawing)));
    systemSummary_->setVisible(true);
}

Result<survey::ReductionContext> SurveyImportWizard::contextForReduction() const
{
    auto context = cad::reductionContextFor(*context_.document);
    if (!context) {
        return context.error();
    }
    context->input = reportInputFor(*read_, fileName_);
    context->createdUtc =
        QDateTime::currentDateTimeUtc().toString(Qt::ISODate).toStdString();
    return context;
}

void SurveyImportWizard::prepareReduction()
{
    // The options start from the defaults and the control the file declares,
    // once per file read: going back and forth keeps what the person set.
    if (optionsGeneration_ != readGeneration_) {
        survey::ReductionSettings settings;
        settings.control = survey::controlFromFile(*raw_);
        options_->setSettings(settings);
        options_->setProject(*raw_);
        optionsGeneration_ = readGeneration_;
        previewReport_->showNote("Press Preview to reduce and adjust with these options and "
                                 "read the report here before importing.");
    }
    // The drawing's points, as they are now.
    auto context = cad::reductionContextFor(*context_.document);
    options_->setDrawingPoints(context ? context->drawingPoints
                                       : std::vector<survey::SurveyPoint>{});
    if (!context) {
        showError(context.error());
    }
}

bool SurveyImportWizard::previewIsCurrent() const
{
    if (outcome_ == nullptr || outcomeRead_ != readGeneration_ ||
        outcomeRevision_ != context_.document->modelRevision() ||
        outcomeSystem_ != context_.document->metadata().coordinateSystem) {
        return false;
    }
    const auto settings = options_->settings();
    return settings && *settings == outcomeSettings_;
}

void SurveyImportWizard::runPreview(std::function<void()> then)
{
    if (raw_ == nullptr || read_ == nullptr) {
        return;
    }
    const auto settings = options_->settings();
    if (!settings) {
        showError(settings.error());
        return;
    }
    auto context = contextForReduction();
    if (!context) {
        showError(context.error());
        return;
    }
    const std::shared_ptr<const survey::SurveyProject> raw = raw_;
    const std::size_t size = surveySize(*raw);
    const std::uint64_t generation = readGeneration_;
    const std::uint64_t revision = context_.document->modelRevision();
    std::string system = context_.document->metadata().coordinateSystem;
    std::vector<survey::SurveyPoint> drawingPoints = context->drawingPoints;
    message_->clear();
    previewReport_->showNote("Reducing and adjusting...");
    task_->run(
        QString("Reducing and adjusting %1 observation(s) and point(s)").arg(size),
        size > kBackgroundObservations,
        [this, raw, settings = *settings, context = std::move(*context), generation, revision,
         system = std::move(system), drawingPoints = std::move(drawingPoints),
         then = std::move(then)]() mutable -> SurveyTaskBar::Finish {
            auto outcome = survey::reduceAndAdjust(*raw, settings, context);
            if (!outcome) {
                return [this, error = outcome.error()] {
                    previewReport_->showNote("The reduction could not run: " +
                                             qs(error.describe()));
                    showError(error);
                };
            }
            auto shared = std::make_shared<const survey::ReductionOutcome>(std::move(*outcome));
            // Rendered here, off the GUI thread; cut to what a view can lay out.
            std::string html =
                survey::renderHtml(displayReport(shared->report, kReportRowsOnScreen));
            return [this, shared, html = std::move(html), settings, generation, revision,
                    system = std::move(system), drawingPoints = std::move(drawingPoints),
                    then = std::move(then)]() mutable {
                if (generation != readGeneration_) {
                    return;
                }
                outcome_ = shared;
                outcomeSettings_ = settings;
                outcomeDrawingPoints_ = std::move(drawingPoints);
                outcomeRead_ = generation;
                outcomeRevision_ = revision;
                outcomeSystem_ = std::move(system);
                previewReport_->setReportHtml(html);
                importReport_->setReportHtml(html);
                // The report was asked for: it gets the larger part of the
                // page, the options the smaller (the person may drag back).
                if (const QList<int> sizes = reductionSplitter_->sizes();
                    sizes.size() == 2 && sizes[1] < sizes[0]) {
                    reductionSplitter_->setSizes({sizes[1], sizes[0]});
                }
                showMessage(QString("Preview: %1 point(s); %2; %3 observation(s) rejected.")
                                .arg(shared->points.size())
                                .arg(qs(adjustmentSummary(shared->report)))
                                .arg(rejectedObservations(shared->report)));
                if (then) {
                    then();
                }
            };
        });
}

void SurveyImportWizard::prepareReaderReport()
{
    const surveyio::ReadResult& read = *read_;
    const auto descriptor = surveyio::formatRegistry().find(read.formatId);
    const survey::ReductionReport& report = outcome_->report;
    QString text;
    text += QString("File: %1\n").arg(qs(fileName_));
    text += QString("Format: %1 (parser %2)\n")
                .arg(descriptor ? qs(descriptor->humanName) : qs(read.formatId),
                     qs(read.parserVersion));
    text += QString("Records read: %1, skipped: %2, warnings: %3\n")
                .arg(read.recordsRead)
                .arg(read.recordsSkipped)
                .arg(read.warnings.size());
    text += QString("Adjustment: %1\n").arg(qs(adjustmentSummary(report)));
    text += QString("Points computed: %1; observations rejected: %2; reduction warnings: %3\n")
                .arg(outcome_->points.size())
                .arg(rejectedObservations(report))
                .arg(report.warnings.size());
    text += QString("Layer: %1%2\n")
                .arg(layer_->text().trimmed(),
                     layerPerCode_->isChecked() ? ", a layer per code beneath it" : "");
    text += QString("Ids the drawing already has: %1\n")
                .arg(existing_->currentText());
    text += "Import keeps the file as a survey job: Survey > Survey Jobs adjusts it again.\n";
    report_->setPlainText(text);
    importReport_->setVisible(true);
    blocked_ = outcome_ == nullptr;
}

void SurveyImportWizard::importJob()
{
    if (read_ == nullptr || raw_ == nullptr || sourceBytes_ == nullptr) {
        return;
    }
    // The drawing may have changed since the preview (the dialog is not
    // modal): the reduction is run again then, as the preview ran it.
    if (!previewIsCurrent()) {
        runPreview([this] { importJob(); });
        return;
    }
    auto context = contextForReduction();
    if (!context) {
        showError(context.error());
        return;
    }
    cad::SurveyImportOptions options;
    options.layer = layer_->text().trimmed().toStdString();
    options.layerPerCode = layerPerCode_->isChecked();
    const auto policy =
        policies()[static_cast<std::size_t>(std::max(0, existing_->currentIndex()))];

    const surveyio::ReadResult& read = *read_;
    const std::string now = context->createdUtc;
    cad::SurveyJobImport request;
    request.job.name = fileName_;
    request.job.formatId = read.formatId;
    request.job.parserVersion = read.parserVersion;
    request.job.sourceFileName = fileName_;
    request.job.sourceBytes = *sourceBytes_;
    for (const surveyio::SiblingFile& sibling : read.siblingsRead) {
        request.job.siblingFiles.push_back({sibling.name, sibling.bytes});
    }
    request.job.settings = outcomeSettings_;
    request.job.layer = options.layer;
    request.job.importedUtc = now;
    request.raw = *raw_;
    request.context = std::move(*context);
    request.importOptions = options;
    request.existingPoints = policy;
    // The reduction the preview ran on a pool thread, not a second one here.
    auto reduce = precomputedReduction(
        {outcome_, outcomeSettings_, {}, outcomeDrawingPoints_});
    auto command =
        std::make_unique<cad::ImportSurveyJobCommand>(*context_.document, std::move(request),
                                                     std::move(reduce));
    cad::ImportSurveyJobCommand* job = command.get();
    cad::Document& document = *context_.document;
    if (const auto status = document.execute(std::move(command)); !status) {
        showError(status.error());
        return;
    }
    const std::vector<katana::entity::EntityId> created = document.lastCreatedEntities();
    const survey::ReductionReport* report = job->report();
    const auto descriptor = surveyio::formatRegistry().find(read.formatId);
    const QString summary =
        QString("Imported survey job %1 from %2 (%3, parser %4): %5 point(s) on %6; %7; %8 "
                "observation(s) rejected; %9 warning(s). One command - Undo removes it; the "
                "reduction report is in Survey > Survey Jobs.")
            .arg(qs(job->jobId()), qs(fileName_),
                 descriptor ? qs(descriptor->humanName) : qs(read.formatId),
                 qs(read.parserVersion))
            .arg(outcome_->points.size())
            .arg(qs(options.layer),
                 report != nullptr ? qs(adjustmentSummary(*report)) : QString("-"))
            .arg(report != nullptr ? rejectedObservations(*report) : 0)
            .arg(read.warnings.size() + (report != nullptr ? report->warnings.size() : 0));
    context_.log(summary, false);
    if (context_.views != nullptr) {
        context_.views->zoomExtentsAll();
    }
    if (applyCodes_->isChecked() && applyCodes_->isEnabled() &&
        context_.applySurveyCodes != nullptr && !created.empty()) {
        document.selection().set(created);
        document.notifySelectionChanged();
        context_.applySurveyCodes->trigger();
    }
    blocked_ = true;
    goTo(FilePage);
    hide();
}

} // namespace katana::qt
