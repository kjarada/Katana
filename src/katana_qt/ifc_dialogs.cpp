#include "ifc_dialogs.hpp"

#include <QCheckBox>
#include <QDir>
#include <QFileDialog>
#include <QFontDatabase>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QTableWidget>
#include <QVBoxLayout>

#include "format.hpp"
#include "katana/core/text.hpp"
#include "theme.hpp"

namespace katana::qt {

namespace {

QString qs(const std::string& text)
{
    return QString::fromStdString(text);
}

// The text of a path field as the filesystem takes it.
QString pathOf(const QLineEdit* field)
{
    return QDir::fromNativeSeparators(field->text().trimmed());
}

// A path field and its Browse button, in one row.
QWidget* browseRow(QLineEdit*& edit, const QString& editName, const QString& placeholder,
                   const QString& buttonName, const std::function<void()>& browse, QWidget* parent)
{
    auto* row = new QWidget(parent);
    auto* layout = new QHBoxLayout(row);
    layout->setContentsMargins(0, 0, 0, 0);
    edit = new QLineEdit(row);
    edit->setObjectName(editName);
    edit->setPlaceholderText(placeholder);
    auto* button = new QPushButton(QStringLiteral("Browse..."), row);
    button->setObjectName(buttonName);
    button->setAutoDefault(false);
    QObject::connect(button, &QPushButton::clicked, row, browse);
    layout->addWidget(edit, 1);
    layout->addWidget(button);
    return row;
}

QCheckBox* checkBox(const QString& name, const QString& tip, bool checked, QWidget* parent)
{
    auto* box = new QCheckBox(parent);
    box->setObjectName(name);
    box->setToolTip(tip);
    box->setChecked(checked);
    return box;
}

QPushButton* button(const QString& text, const QString& name, QWidget* parent)
{
    auto* made = new QPushButton(text, parent);
    made->setObjectName(name);
    made->setAutoDefault(false);
    return made;
}

bool isIfcName(const QString& path)
{
    return path.endsWith(QStringLiteral(".ifc"), Qt::CaseInsensitive);
}

// "IfcPipeSegment RIGIDSEGMENT", "IfcDistributionChamberElement USERDEFINED
// (HEADWALL)", or that nothing was written.
QString classText(const katana::ifc::ClassTally& row)
{
    if (row.entity.empty()) {
        return QStringLiteral("not written");
    }
    QString text = qs(row.entity);
    if (!row.predefinedType.empty()) {
        text += ' ' + qs(row.predefinedType);
    }
    if (!row.objectType.empty()) {
        text += " (" + qs(row.objectType) + ')';
    }
    return text;
}

} // namespace

// ---- export -------------------------------------------------------------------------

IfcExportDialog::IfcExportDialog(IfcExportContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName(QStringLiteral("fileExportIfcDialog"));
    setWindowTitle(QStringLiteral("Export IFC"));
    setModal(false);
    resize(820, 720);
    auto* layout = new QVBoxLayout(this);

    auto* intro = new QLabel(
        QStringLiteral("IFC 4.3 (IFC4X3_ADD2): alignments as their design, the drawing by class - "
                       "kerbs, fences, pits, pipes - and an AS 5488 investigation graded and "
                       "typed by its services. Nothing is written as a proxy."),
        this);
    intro->setWordWrap(true);
    intro->setStyleSheet(QString("color: %1").arg(theme::textMuted().name()));
    layout->addWidget(intro);

    auto* form = new QFormLayout();
    form->addRow(QStringLiteral("File:"),
                 browseRow(
                     file_, QStringLiteral("ifcExportFile"),
                     QStringLiteral("the .ifc file to write; it is replaced whole"),
                     QStringLiteral("ifcExportBrowse"),
                     [this] {
                         browse(file_, QStringLiteral("Export IFC"),
                                QStringLiteral("IFC 4.3 (*.ifc)"), true);
                     },
                     this));

    entities_ = checkBox(QStringLiteral("ifcExportEntities"),
                         QStringLiteral("The drawing's entities, each as the class its layer, "
                                        "survey code or 12d name makes it"),
                         true, this);
    selectedOnly_ =
        checkBox(QStringLiteral("ifcExportSelectedOnly"),
                 QStringLiteral("Only the entities selected in the drawing"), false, this);
    alignments_ = checkBox(QStringLiteral("ifcExportAlignments"),
                           QStringLiteral("Every alignment, as IfcAlignment: its PIs' design "
                                          "and its profile's, with stations"),
                           true, this);
    surfaces_ = checkBox(QStringLiteral("ifcExportSurfaces"),
                         QStringLiteral("The surfaces of this session, as IfcGeographicElement "
                                        "TERRAIN"),
                         true, this);
    form->addRow(QStringLiteral("Drawing:"), entities_);
    form->addRow(QString(), selectedOnly_);
    form->addRow(QString(), alignments_);
    form->addRow(QString(), surfaces_);

    form->addRow(QStringLiteral("Utility schedule:"),
                 browseRow(
                     schedule_, QStringLiteral("ifcExportSchedule"),
                     QStringLiteral("optional: an AS 5488 schedule (.csv)"),
                     QStringLiteral("ifcExportScheduleBrowse"),
                     [this] {
                         browse(schedule_, QStringLiteral("Utility Schedule"),
                                QStringLiteral("Utility schedule (*.csv);;All files (*)"), false);
                     },
                     this));
    form->addRow(QStringLiteral("Delivery schema:"),
                 browseRow(
                     schema_, QStringLiteral("ifcExportSchema"),
                     QStringLiteral("optional: the schema it was written to (.csv)"),
                     QStringLiteral("ifcExportSchemaBrowse"),
                     [this] {
                         browse(schema_, QStringLiteral("Delivery Schema"),
                                QStringLiteral("Delivery schema (*.csv);;All files (*)"), false);
                     },
                     this));
    spacing_ = new QLineEdit(this);
    spacing_->setObjectName(QStringLiteral("ifcExportSpacing"));
    spacing_->setText(
        QString::number(katana::survey::subsurface::GradingSettings{}.maximumDetectedSpacing));
    spacing_->setToolTip(QStringLiteral(
        "The longest detected spacing that keeps QL-B, in metres: a path interpolated across a "
        "longer gap was not traced (the project specification sets it)"));
    form->addRow(QStringLiteral("Detected spacing (m):"), spacing_);

    auto* rulesRow = new QWidget(this);
    auto* rulesLayout = new QHBoxLayout(rulesRow);
    rulesLayout->setContentsMargins(0, 0, 0, 0);
    QWidget* rulesField = browseRow(
        rules_, QStringLiteral("ifcExportRules"),
        QStringLiteral("optional: a project's classification rules (.csv)"),
        QStringLiteral("ifcExportRulesBrowse"),
        [this] {
            browse(rules_, QStringLiteral("Classification Rules"),
                   QStringLiteral("Rules (*.csv);;All files (*)"), false);
        },
        rulesRow);
    rules_->setToolTip(QStringLiteral(
        "Rules tried before the defaults: a layer, survey code or 12d name word and the IFC "
        "class it makes (docs/ifc.md, \"Classification rules\")"));
    QPushButton* saveRulesButton = button(QStringLiteral("Save Default Rules..."),
                                          QStringLiteral("ifcExportSaveRules"), rulesRow);
    saveRulesButton->setToolTip(
        QStringLiteral("Write the default rules to a file, to edit into the project's own"));
    rulesLayout->addWidget(rulesField, 1);
    rulesLayout->addWidget(saveRulesButton);
    form->addRow(QStringLiteral("Classification rules:"), rulesRow);
    layout->addLayout(form);

    crs_ = new QLabel(this);
    crs_->setObjectName(QStringLiteral("ifcExportCrs"));
    crs_->setWordWrap(true);
    layout->addWidget(crs_);

    classes_ = new QTableWidget(0, 5, this);
    classes_->setObjectName(QStringLiteral("ifcExportClasses"));
    classes_->setHorizontalHeaderLabels({QStringLiteral("From"), QStringLiteral("Objects"),
                                         QStringLiteral("IFC class"), QStringLiteral("System"),
                                         QStringLiteral("Why")});
    classes_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    classes_->setSelectionBehavior(QAbstractItemView::SelectRows);
    classes_->verticalHeader()->setVisible(false);
    classes_->horizontalHeader()->setSectionResizeMode(QHeaderView::ResizeToContents);
    classes_->horizontalHeader()->setStretchLastSection(true);
    classes_->setToolTip(QStringLiteral(
        "What the file holds, by where it came from: the writer's own account, from Preview or "
        "the export just written"));
    layout->addWidget(classes_, 1);

    check_ = new QLabel(this);
    check_->setObjectName(QStringLiteral("ifcExportCheck"));
    check_->setWordWrap(true);
    check_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    layout->addWidget(check_);

    auto* buttons = new QHBoxLayout();
    preview_ = button(QStringLiteral("Preview"), QStringLiteral("ifcExportPreview"), this);
    preview_->setToolTip(
        QStringLiteral("What would be written, class by class; no file is written"));
    export_ = button(QStringLiteral("Export"), QStringLiteral("ifcExportExport"), this);
    export_->setAutoDefault(true);
    export_->setDefault(true);
    QPushButton* close = button(QStringLiteral("Close"), QStringLiteral("ifcExportClose"), this);
    buttons->addWidget(preview_);
    buttons->addStretch(1);
    buttons->addWidget(export_);
    buttons->addWidget(close);
    layout->addLayout(buttons);

    for (QLineEdit* field : {file_, schedule_, schema_, spacing_, rules_}) {
        connect(field, &QLineEdit::textChanged, this, [this] {
            showingResult_ = false;
            recheck();
        });
    }
    for (QCheckBox* box : {entities_, selectedOnly_, alignments_, surfaces_}) {
        connect(box, &QCheckBox::toggled, this, [this] {
            showingResult_ = false;
            recheck();
        });
    }
    connect(saveRulesButton, &QPushButton::clicked, this, [this] { saveRules(); });
    connect(preview_, &QPushButton::clicked, this, [this] { preview(); });
    connect(export_, &QPushButton::clicked, this, [this] { exportFile(); });
    connect(close, &QPushButton::clicked, this, &QWidget::hide);

    refresh();
}

void IfcExportDialog::showEvent(QShowEvent* event)
{
    refresh();
    QDialog::showEvent(event);
}

void IfcExportDialog::refresh()
{
    if (context_.state) {
        state_ = context_.state();
    }
    entities_->setText(QStringLiteral("Drawing entities (") + grouped(state_.entities) + ')');
    if (state_.selected == 0) {
        selectedOnly_->setText(QStringLiteral("Selected entities only (nothing is selected)"));
        selectedOnly_->setChecked(false);
    } else {
        selectedOnly_->setText(QStringLiteral("Selected entities only (") +
                               grouped(state_.selected) + QStringLiteral(" selected)"));
    }
    selectedOnly_->setEnabled(state_.selected > 0 && entities_->isChecked());
    alignments_->setText(QStringLiteral("Alignments (") + grouped(state_.alignments) + ')');
    surfaces_->setText(QStringLiteral("Surfaces of this session (") + grouped(state_.surfaces) +
                       ')');
    crs_->setText(state_.georeferenced
                      ? QStringLiteral("Georeferenced in ") + state_.coordinateSystem +
                            QStringLiteral(": coordinates are written from a local origin near "
                                           "the work, which an IfcMapConversion records.")
                      : QStringLiteral("Not georeferenced: ") +
                            (state_.coordinateSystem.isEmpty()
                                 ? QStringLiteral("the project has no coordinate system")
                                 : state_.coordinateSystem + QStringLiteral(" has no EPSG code")) +
                            QStringLiteral(", so coordinates are written as they are. File > "
                                           "Project Coordinate System sets one."));
    crs_->setStyleSheet(QString("color: %1").arg(theme::textMuted().name()));
    recheck();
}

IfcExportRequest IfcExportDialog::request() const
{
    IfcExportRequest out;
    out.arguments.path = pathOf(file_).toStdString();
    if (const QString schedule = pathOf(schedule_); !schedule.isEmpty()) {
        out.arguments.schedule = schedule.toStdString();
    }
    if (const QString schema = pathOf(schema_); !schema.isEmpty()) {
        out.arguments.schema = schema.toStdString();
    }
    if (const QString rules = pathOf(rules_); !rules.isEmpty()) {
        out.arguments.rules = rules.toStdString();
    }
    out.arguments.spacing = core::parseFiniteDouble(spacing_->text().trimmed().toStdString());
    out.entities = entities_->isChecked();
    out.selectedOnly = out.entities && selectedOnly_->isEnabled() && selectedOnly_->isChecked();
    out.alignments = alignments_->isChecked();
    out.surfaces = surfaces_->isChecked();
    out.arguments.drawing = out.entities || out.alignments || out.surfaces;
    return out;
}

QString IfcExportDialog::check() const
{
    const QString file = pathOf(file_);
    if (file.isEmpty()) {
        return QStringLiteral("Name the .ifc file to write.");
    }
    if (!isIfcName(file)) {
        return QStringLiteral("The file must end in .ifc.");
    }
    if (!pathOf(schema_).isEmpty() && pathOf(schedule_).isEmpty()) {
        return QStringLiteral(
            "A delivery schema describes a schedule: choose the schedule it describes too.");
    }
    const auto spacing = core::parseFiniteDouble(spacing_->text().trimmed().toStdString());
    if (!spacing || *spacing <= 0.0) {
        return QStringLiteral("The detected spacing must be a positive number of metres.");
    }
    const IfcExportRequest asked = request();
    const bool drawing =
        (asked.entities && (asked.selectedOnly ? state_.selected : state_.entities) > 0) ||
        (asked.alignments && state_.alignments > 0) || (asked.surfaces && state_.surfaces > 0);
    if (!drawing && pathOf(schedule_).isEmpty()) {
        return QStringLiteral("Nothing to write: the chosen parts of the drawing are empty, and no "
                              "utility schedule is chosen.");
    }
    return {};
}

void IfcExportDialog::recheck()
{
    selectedOnly_->setEnabled(state_.selected > 0 && entities_->isChecked());
    schema_->setEnabled(!pathOf(schedule_).isEmpty() || !pathOf(schema_).isEmpty());
    const QString problem = check();
    export_->setEnabled(problem.isEmpty());
    // A preview needs everything but the file.
    const bool onlyTheFile = problem == QStringLiteral("Name the .ifc file to write.") ||
                             problem == QStringLiteral("The file must end in .ifc.");
    preview_->setEnabled(problem.isEmpty() || onlyTheFile);
    if (showingResult_) {
        return;
    }
    if (!problem.isEmpty()) {
        say(problem, true);
        return;
    }
    const IfcExportRequest asked = request();
    QStringList parts;
    if (asked.entities) {
        parts << (asked.selectedOnly
                      ? grouped(state_.selected) + QStringLiteral(" selected entities")
                      : grouped(state_.entities) + QStringLiteral(" entities"));
    }
    if (asked.alignments && state_.alignments > 0) {
        parts << grouped(state_.alignments) + QStringLiteral(" alignments");
    }
    if (asked.surfaces && state_.surfaces > 0) {
        parts << grouped(state_.surfaces) + QStringLiteral(" surfaces");
    }
    if (asked.arguments.schedule) {
        parts << QStringLiteral("the schedule's services");
    }
    say(QStringLiteral("Ready to write ") + parts.join(QStringLiteral(", ")) +
            QStringLiteral(". Preview shows the class each becomes."),
        false);
}

void IfcExportDialog::setFile(const QString& path)
{
    file_->setText(QDir::toNativeSeparators(path));
}

void IfcExportDialog::browse(QLineEdit* field, const QString& title, const QString& filter,
                             bool save)
{
    if (context_.headless && context_.headless()) {
        say(QStringLiteral("Browse asks a person to choose a file; type its path instead."), true);
        showingResult_ = true;
        return;
    }
    const QString start = pathOf(field);
    QString chosen = save ? QFileDialog::getSaveFileName(this, title, start, filter)
                          : QFileDialog::getOpenFileName(this, title, start, filter);
    if (chosen.isEmpty()) {
        return;
    }
    if (save && !isIfcName(chosen)) {
        chosen += QStringLiteral(".ifc");
    }
    field->setText(QDir::toNativeSeparators(chosen));
}

void IfcExportDialog::say(const QString& text, bool isError)
{
    check_->setText(text);
    check_->setStyleSheet(
        QString("color: %1").arg((isError ? theme::error() : theme::textMuted()).name()));
}

void IfcExportDialog::showTally(const std::vector<katana::ifc::ClassTally>& tally)
{
    classes_->setRowCount(static_cast<int>(tally.size()));
    for (int row = 0; row < static_cast<int>(tally.size()); ++row) {
        const katana::ifc::ClassTally& item = tally[static_cast<std::size_t>(row)];
        const QStringList cells{qs(item.source), grouped(item.count), classText(item),
                                qs(item.system), qs(item.why)};
        for (int column = 0; column < cells.size(); ++column) {
            auto* cell = new QTableWidgetItem(cells[column]);
            cell->setToolTip(cells[column]); // a long reason is cut short in its column
            if (item.entity.empty()) {
                cell->setForeground(theme::textMuted());
            }
            if (column == 1) {
                cell->setTextAlignment(Qt::AlignRight | Qt::AlignVCenter);
            }
            classes_->setItem(row, column, cell);
        }
    }
}

void IfcExportDialog::preview()
{
    if (!context_.preview) {
        return;
    }
    IfcExportRequest asked = request();
    if (asked.arguments.path.empty() || !isIfcName(pathOf(file_))) {
        asked.arguments.path = "preview.ifc";
    }
    const auto result = context_.preview(asked);
    showingResult_ = true;
    if (!result) {
        showTally({});
        say(QString::fromStdString(result.error().describe()), true);
        return;
    }
    showTally(result->tally);
    std::size_t objects = 0;
    for (const auto& row : result->tally) {
        if (!row.entity.empty()) {
            objects += row.count;
        }
    }
    say(QStringLiteral("Preview: ") + grouped(objects) +
            QStringLiteral(" objects would be written, none as a proxy unless a rule asks for "
                           "one; no file has been written."),
        false);
}

void IfcExportDialog::exportFile()
{
    const QString problem = check();
    if (!problem.isEmpty()) {
        say(problem, true);
        return;
    }
    if (!context_.run) {
        return;
    }
    const auto result = context_.run(request());
    showingResult_ = true;
    if (!result) {
        say(QString::fromStdString(result.error().describe()), true);
        return;
    }
    showTally(result->tally);
    say(QStringLiteral("Wrote ") + pathOf(file_) + QStringLiteral(": ") +
            grouped(result->instances) + QStringLiteral(" instances, ") +
            grouped(result->entitiesWritten) + QStringLiteral(" entities, ") +
            grouped(result->alignments) + QStringLiteral(" alignments, ") +
            grouped(result->services) + QStringLiteral(" services and ") +
            grouped(result->surfaces) + QStringLiteral(" surfaces (the log has the rest)."),
        false);
}

void IfcExportDialog::saveRules()
{
    if (context_.headless && context_.headless()) {
        say(QStringLiteral("Save Default Rules asks a person where; it is not available here."),
            true);
        showingResult_ = true;
        return;
    }
    QString chosen = QFileDialog::getSaveFileName(this, QStringLiteral("Save Default Rules"),
                                                  QStringLiteral("classification_rules.csv"),
                                                  QStringLiteral("Rules (*.csv)"));
    if (chosen.isEmpty() || !context_.saveDefaultRules) {
        return;
    }
    if (!chosen.endsWith(QStringLiteral(".csv"), Qt::CaseInsensitive)) {
        chosen += QStringLiteral(".csv");
    }
    const auto status = context_.saveDefaultRules(chosen);
    showingResult_ = true;
    if (!status) {
        say(QString::fromStdString(status.error().describe()), true);
        return;
    }
    rules_->setText(QDir::toNativeSeparators(chosen));
    showingResult_ = true; // after the field's own change cleared it
    say(QStringLiteral("Wrote the default rules to ") + chosen +
            QStringLiteral(": edit them for the project's layers, and they are used from there."),
        false);
}

// ---- import -------------------------------------------------------------------------

IfcImportDialog::IfcImportDialog(IfcImportContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName(QStringLiteral("fileImportIfcDialog"));
    setWindowTitle(QStringLiteral("Import IFC"));
    setModal(false);
    resize(640, 560);
    auto* layout = new QVBoxLayout(this);

    auto* form = new QFormLayout();
    form->addRow(QStringLiteral("File:"),
                 browseRow(
                     file_, QStringLiteral("ifcImportFile"),
                     QStringLiteral("an IFC file: IFC2X3, IFC4 or IFC4X3"),
                     QStringLiteral("ifcImportBrowse"),
                     [this] {
                         if (context_.headless && context_.headless()) {
                             say(QStringLiteral("Browse asks a person to choose a file; "
                                                "type its path instead."),
                                 true);
                             showingResult_ = true;
                             return;
                         }
                         const QString chosen = QFileDialog::getOpenFileName(
                             this, QStringLiteral("Import IFC"), pathOf(file_),
                             QStringLiteral("IFC (*.ifc);;All files (*)"));
                         if (!chosen.isEmpty()) {
                             file_->setText(QDir::toNativeSeparators(chosen));
                             describe();
                         }
                     },
                     this));
    layout->addLayout(form);

    summary_ = new QPlainTextEdit(this);
    summary_->setObjectName(QStringLiteral("ifcImportSummary"));
    summary_->setReadOnly(true);
    summary_->setPlaceholderText(
        QStringLiteral("What the file holds - its schema, coordinate system, classes, alignments "
                       "and surfaces - shown by Describe."));
    summary_->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    layout->addWidget(summary_, 1);

    auto* options = new QFormLayout();
    alignments_ = checkBox(QStringLiteral("ifcImportAlignments"),
                           QStringLiteral("Alignments as PIs and PVIs, checked against the file "
                                          "to 10 mm; else their exact geometry as a polyline"),
                           true, this);
    alignments_->setText(QStringLiteral("Alignments"));
    elements_ =
        checkBox(QStringLiteral("ifcImportElements"),
                 QStringLiteral("Elements and annotations, with their property sets"), true, this);
    elements_->setText(QStringLiteral("Elements and annotations"));
    surfaces_ = checkBox(QStringLiteral("ifcImportSurfaces"),
                         QStringLiteral("Terrain as surfaces of this session (not saved with the "
                                        "project, and not undone)"),
                         true, this);
    surfaces_->setText(QStringLiteral("Surfaces"));
    local_ =
        checkBox(QStringLiteral("ifcImportLocal"),
                 QStringLiteral("Move everything to sit at the origin, as one piece"), false, this);
    local_->setText(QStringLiteral("Move to the origin (LOCAL)"));
    takeCrs_ = checkBox(QStringLiteral("ifcImportTakeCrs"),
                        QStringLiteral("When the file names an EPSG code and the project has no "
                                       "coordinate system, the project takes the file's"),
                        true, this);
    takeCrs_->setText(
        QStringLiteral("Take the file's coordinate system when the project has none"));
    tolerance_ = new QLineEdit(this);
    tolerance_->setObjectName(QStringLiteral("ifcImportTolerance"));
    tolerance_->setText(QString::number(katana::ifc::ImportOptions{}.curveTolerance));
    tolerance_->setToolTip(QStringLiteral(
        "How far a chorded curve - an arc in a polycurve, a transition other than the clothoid "
        "- may stand from the true one, in metres"));
    options->addRow(QStringLiteral("Bring in:"), alignments_);
    options->addRow(QString(), elements_);
    options->addRow(QString(), surfaces_);
    options->addRow(QStringLiteral("Place:"), local_);
    options->addRow(QString(), takeCrs_);
    options->addRow(QStringLiteral("Curves within (m):"), tolerance_);
    layout->addLayout(options);

    check_ = new QLabel(this);
    check_->setObjectName(QStringLiteral("ifcImportCheck"));
    check_->setWordWrap(true);
    check_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    layout->addWidget(check_);

    auto* buttons = new QHBoxLayout();
    QPushButton* describeButton =
        button(QStringLiteral("Describe"), QStringLiteral("ifcImportDescribe"), this);
    describeButton->setToolTip(QStringLiteral("Read the file and say what it holds; nothing is "
                                              "imported"));
    import_ = button(QStringLiteral("Import"), QStringLiteral("ifcImportImport"), this);
    import_->setAutoDefault(true);
    import_->setDefault(true);
    QPushButton* close = button(QStringLiteral("Close"), QStringLiteral("ifcImportClose"), this);
    buttons->addWidget(describeButton);
    buttons->addStretch(1);
    buttons->addWidget(import_);
    buttons->addWidget(close);
    layout->addLayout(buttons);

    connect(file_, &QLineEdit::textChanged, this, [this] {
        showingResult_ = false;
        summary_->clear();
        recheck();
    });
    connect(tolerance_, &QLineEdit::textChanged, this, [this] {
        showingResult_ = false;
        recheck();
    });
    for (QCheckBox* box : {alignments_, elements_, surfaces_, local_, takeCrs_}) {
        connect(box, &QCheckBox::toggled, this, [this] {
            showingResult_ = false;
            recheck();
        });
    }
    connect(local_, &QCheckBox::toggled, this, [this](bool on) {
        // Moved to the origin, the data is no longer in the file's system.
        takeCrs_->setEnabled(!on);
    });
    connect(describeButton, &QPushButton::clicked, this, [this] { describe(); });
    connect(import_, &QPushButton::clicked, this, [this] { importFile(); });
    connect(close, &QPushButton::clicked, this, &QWidget::hide);
    recheck();
}

IfcImportRequest IfcImportDialog::request() const
{
    IfcImportRequest out;
    out.arguments.path = pathOf(file_).toStdString();
    out.arguments.local = local_->isChecked();
    out.alignments = alignments_->isChecked();
    out.elements = elements_->isChecked();
    out.surfaces = surfaces_->isChecked();
    if (const auto tolerance =
            core::parseFiniteDouble(tolerance_->text().trimmed().toStdString())) {
        out.curveTolerance = *tolerance;
    }
    out.takeCoordinateSystem = !out.arguments.local && takeCrs_->isChecked();
    return out;
}

QString IfcImportDialog::check() const
{
    if (pathOf(file_).isEmpty()) {
        return QStringLiteral("Name the IFC file to import.");
    }
    if (!isIfcName(pathOf(file_))) {
        return QStringLiteral("The file must end in .ifc.");
    }
    const auto tolerance = core::parseFiniteDouble(tolerance_->text().trimmed().toStdString());
    if (!tolerance || *tolerance <= 0.0) {
        return QStringLiteral("The curve tolerance must be a positive number of metres.");
    }
    if (!alignments_->isChecked() && !elements_->isChecked() && !surfaces_->isChecked()) {
        return QStringLiteral("Nothing to import: tick alignments, elements or surfaces.");
    }
    return {};
}

void IfcImportDialog::recheck()
{
    const QString problem = check();
    import_->setEnabled(problem.isEmpty());
    if (showingResult_) {
        return;
    }
    say(problem.isEmpty() ? QStringLiteral("Ready to import, as one step Undo takes back "
                                           "(surfaces are session data and stay).")
                          : problem,
        !problem.isEmpty());
}

void IfcImportDialog::setFile(const QString& path)
{
    file_->setText(QDir::toNativeSeparators(path));
}

void IfcImportDialog::say(const QString& text, bool isError)
{
    check_->setText(text);
    check_->setStyleSheet(
        QString("color: %1").arg((isError ? theme::error() : theme::textMuted()).name()));
}

void IfcImportDialog::describe()
{
    if (pathOf(file_).isEmpty() || !context_.describe) {
        return;
    }
    const auto described = context_.describe(pathOf(file_));
    if (!described) {
        summary_->clear();
        showingResult_ = true;
        say(QString::fromStdString(described.error().describe()), true);
        return;
    }
    summary_->setPlainText(*described);
}

void IfcImportDialog::importFile()
{
    const QString problem = check();
    if (!problem.isEmpty()) {
        say(problem, true);
        return;
    }
    if (!context_.run) {
        return;
    }
    const bool imported = context_.run(request());
    showingResult_ = true;
    say(imported ? QStringLiteral("Imported ") + pathOf(file_) +
                       QStringLiteral(": the log says what came in.")
                 : QStringLiteral("The import failed: the log says why."),
        !imported);
}

} // namespace katana::qt
