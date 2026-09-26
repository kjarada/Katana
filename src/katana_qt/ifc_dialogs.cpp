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

// The first record of a reply - "ifc exported ...", "ifc imported ..." - as
// the verb wrote it (core::readReplyRecord).
std::optional<core::ReplyRecord> headRecord(const std::string& reply)
{
    const auto lines = core::splitLines(reply);
    return lines.empty() ? std::nullopt : core::readReplyRecord(lines.front());
}

// A reply's field, for a sentence; "?" when it is not there.
QString field(const std::optional<core::ReplyRecord>& record, std::string_view key)
{
    const auto value = record ? record->value(key) : std::nullopt;
    return value ? QString::fromStdString(*value) : QStringLiteral("?");
}

// A read-only field showing the line the dialog runs, as it is edited.
QLineEdit* commandField(const QString& name, QWidget* parent)
{
    auto* field = new QLineEdit(parent);
    field->setObjectName(name);
    field->setReadOnly(true);
    field->setToolTip(QStringLiteral(
        "The command this runs, exactly as it could be typed on the command line, given to "
        "katana_cli or sent by an agent"));
    field->setFont(katana::qt::theme::monospaceFont());
    return field;
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
                                        "survey code or string name makes it"),
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
        "Rules tried before the defaults: a layer, survey code or string name word and the IFC "
        "class it makes (docs/ifc.md, \"Classification rules\")"));
    QPushButton* saveRulesButton = button(QStringLiteral("Save Default Rules..."),
                                          QStringLiteral("ifcExportSaveRules"), rulesRow);
    saveRulesButton->setToolTip(QStringLiteral(
        "Write the default rules to a file, to edit into the project's own (IFC RULES)"));
    rulesLayout->addWidget(rulesField, 1);
    rulesLayout->addWidget(saveRulesButton);
    form->addRow(QStringLiteral("Classification rules:"), rulesRow);
    layout->addLayout(form);

    crs_ = new QLabel(this);
    crs_->setObjectName(QStringLiteral("ifcExportCrs"));
    crs_->setWordWrap(true);
    layout->addWidget(crs_);

    command_ = commandField(QStringLiteral("ifcExportCommand"), this);
    auto* commandRow = new QFormLayout();
    commandRow->addRow(QStringLiteral("Command:"), command_);
    layout->addLayout(commandRow);

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

    // A changed choice makes the last account out of date: it is cleared,
    // so that a table never stands beside "Ready" showing what would no
    // longer be written. The file alone changes no class, and keeps it.
    for (QLineEdit* field : {schedule_, schema_, spacing_, rules_}) {
        connect(field, &QLineEdit::textChanged, this, [this] {
            showingResult_ = false;
            showTally({});
            recheck();
        });
    }
    connect(file_, &QLineEdit::textChanged, this, [this] {
        showingResult_ = false;
        recheck();
    });
    for (QCheckBox* box : {entities_, selectedOnly_, alignments_, surfaces_}) {
        connect(box, &QCheckBox::toggled, this, [this] {
            showingResult_ = false;
            showTally({});
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
    // Opened again, the last export's words and table are about a drawing
    // that may have changed since.
    showingResult_ = false;
    showTally({});
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
        // Kept as chosen: a selection cleared while the dialog is open must
        // refuse the export (check()), not quietly widen it to everything.
        selectedOnly_->setText(QStringLiteral("Selected entities only (nothing is selected)"));
    } else {
        selectedOnly_->setText(QStringLiteral("Selected entities only (") +
                               grouped(state_.selected) + QStringLiteral(" selected)"));
    }
    selectedOnly_->setEnabled(entities_->isChecked());
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
                                 : state_.coordinateSystem +
                                       QStringLiteral(" is not one EPSG code, which is how "
                                                      "IFC4X3_ADD2 names a system")) +
                            QStringLiteral(", so coordinates are written as they are. File > "
                                           "Project Coordinate System sets one."));
    crs_->setStyleSheet(QString("color: %1").arg(theme::textMuted().name()));
    recheck();
}

katana::ifc::ExportArguments IfcExportDialog::arguments() const
{
    katana::ifc::ExportArguments out;
    out.path = pathOf(file_).toStdString();
    if (const QString schedule = pathOf(schedule_); !schedule.isEmpty()) {
        out.schedule = schedule.toStdString();
    }
    if (const QString schema = pathOf(schema_); !schema.isEmpty()) {
        out.schema = schema.toStdString();
    }
    if (const QString rules = pathOf(rules_); !rules.isEmpty()) {
        out.rules = rules.toStdString();
    }
    // SPACING is said only when it matters and is not the verb's own
    // default, so that the line is what a person would type.
    const auto spacing = core::parseFiniteDouble(spacing_->text().trimmed().toStdString());
    if (out.schedule && spacing &&
        *spacing != katana::survey::subsurface::GradingSettings{}.maximumDetectedSpacing) {
        out.spacing = *spacing;
    }
    out.entities = entities_->isChecked();
    out.selected = out.entities && selectedOnly_->isChecked();
    out.alignments = alignments_->isChecked();
    out.surfaces = surfaces_->isChecked();
    return out;
}

QString IfcExportDialog::line() const
{
    if (!check().isEmpty()) {
        return {};
    }
    const auto written = katana::ifc::formatExportLine(arguments());
    return written ? QString::fromStdString(*written) : QString();
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
    return checkWithoutFile();
}

QString IfcExportDialog::checkWithoutFile() const
{
    if (!pathOf(schema_).isEmpty() && pathOf(schedule_).isEmpty()) {
        return QStringLiteral(
            "A delivery schema describes a schedule: choose the schedule it describes too.");
    }
    const auto spacing = core::parseFiniteDouble(spacing_->text().trimmed().toStdString());
    if (!spacing || *spacing <= 0.0) {
        return QStringLiteral("The detected spacing must be a positive number of metres.");
    }
    const katana::ifc::ExportArguments asked = arguments();
    if (asked.selected && state_.selected == 0) {
        return QStringLiteral("Selected entities only is ticked, and nothing is selected: select "
                              "what to export, or untick it.");
    }
    const bool drawing =
        (asked.entities && (asked.selected ? state_.selected : state_.entities) > 0) ||
        (asked.alignments && state_.alignments > 0) || (asked.surfaces && state_.surfaces > 0);
    if (!drawing && pathOf(schedule_).isEmpty()) {
        return QStringLiteral("Nothing to write: the chosen parts of the drawing are empty, and no "
                              "utility schedule is chosen.");
    }
    for (QLineEdit* path : {file_, schedule_, schema_, rules_}) {
        if (path->text().contains('"')) {
            return QStringLiteral("A path holding a double quote cannot be written in a "
                                  "command; rename the file.");
        }
    }
    return {};
}

void IfcExportDialog::recheck()
{
    selectedOnly_->setEnabled(entities_->isChecked());
    schema_->setEnabled(!pathOf(schedule_).isEmpty() || !pathOf(schema_).isEmpty());
    const QString problem = check();
    export_->setEnabled(problem.isEmpty());
    // A preview needs everything but the file.
    preview_->setEnabled(checkWithoutFile().isEmpty());
    command_->setText(line());
    if (!problem.isEmpty()) {
        // What stops the export is said, even over the last result: a
        // disabled button with a stale "Wrote ..." beside it says nothing.
        showingResult_ = false;
        say(problem, true);
        return;
    }
    if (showingResult_) {
        return;
    }
    const katana::ifc::ExportArguments asked = arguments();
    QStringList parts;
    if (asked.entities) {
        parts << (asked.selected ? grouped(state_.selected) + QStringLiteral(" selected entities")
                                 : grouped(state_.entities) + QStringLiteral(" entities"));
    }
    if (asked.alignments && state_.alignments > 0) {
        parts << grouped(state_.alignments) + QStringLiteral(" alignments");
    }
    if (asked.surfaces && state_.surfaces > 0) {
        parts << grouped(state_.surfaces) + QStringLiteral(" surfaces");
    }
    if (asked.schedule) {
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

std::optional<std::vector<katana::ifc::ClassTally>>
IfcExportDialog::showReply(const std::string& reply)
{
    auto objects = katana::ifc::readExportObjects(reply);
    if (!objects) {
        showTally({});
        say(QString::fromStdString(objects.error().describe()), true);
        return std::nullopt;
    }
    showTally(*objects);
    return std::move(*objects);
}

void IfcExportDialog::preview()
{
    if (!context_.runLine) {
        return;
    }
    refresh(); // the drawing, and its selection, as they are now
    if (const QString problem = checkWithoutFile(); !problem.isEmpty()) {
        say(problem, true);
        return;
    }
    // The export's own line, PREVIEW added: a file not yet named is given
    // one, which PREVIEW never writes.
    katana::ifc::ExportArguments asked = arguments();
    if (asked.path.empty() || !isIfcName(pathOf(file_))) {
        asked.path = "preview.ifc";
    }
    asked.preview = true;
    const auto written = katana::ifc::formatExportLine(asked);
    if (!written) {
        say(QString::fromStdString(written.error().describe()), true);
        return;
    }
    const auto reply = context_.runLine(QString::fromStdString(*written));
    showingResult_ = true;
    if (!reply) {
        showTally({});
        say(QString::fromStdString(reply.error().describe()), true);
        return;
    }
    const auto tally = showReply(*reply);
    if (!tally) {
        return;
    }
    std::size_t objects = 0;
    for (const katana::ifc::ClassTally& row : *tally) {
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
    refresh(); // the drawing, and its selection, as they are now
    const QString problem = check();
    if (!problem.isEmpty()) {
        say(problem, true);
        return;
    }
    if (!context_.runLine) {
        return;
    }
    const auto reply = context_.runLine(line());
    showingResult_ = true;
    if (!reply) {
        say(QString::fromStdString(reply.error().describe()), true);
        return;
    }
    if (!showReply(*reply)) {
        return;
    }
    const auto head = headRecord(*reply);
    say(QStringLiteral("Wrote ") + pathOf(file_) + QStringLiteral(": ") + field(head, "instances") +
            QStringLiteral(" instances (the log has the reply)."),
        false);
}

void IfcExportDialog::saveRules()
{
    if (context_.headless && context_.headless()) {
        say(QStringLiteral("Save Default Rules asks a person where; type IFC RULES <file.csv> "
                           "on the command line instead."),
            true);
        showingResult_ = true;
        return;
    }
    QString chosen = QFileDialog::getSaveFileName(this, QStringLiteral("Save Default Rules"),
                                                  QStringLiteral("classification_rules.csv"),
                                                  QStringLiteral("Rules (*.csv)"));
    if (chosen.isEmpty() || !context_.runLine) {
        return;
    }
    if (!chosen.endsWith(QStringLiteral(".csv"), Qt::CaseInsensitive)) {
        chosen += QStringLiteral(".csv");
    }
    const auto written = katana::ifc::formatRulesLine(chosen.toStdString());
    if (!written) {
        say(QString::fromStdString(written.error().describe()), true);
        return;
    }
    const auto reply = context_.runLine(QString::fromStdString(*written));
    showingResult_ = true;
    if (!reply) {
        say(QString::fromStdString(reply.error().describe()), true);
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
    summary_->setFont(katana::qt::theme::monospaceFont());
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
    command_ = commandField(QStringLiteral("ifcImportCommand"), this);
    options->addRow(QStringLiteral("Command:"), command_);
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

katana::ifc::ImportArguments IfcImportDialog::arguments() const
{
    katana::ifc::ImportArguments out;
    out.path = pathOf(file_).toStdString();
    out.local = local_->isChecked();
    out.alignments = alignments_->isChecked();
    out.elements = elements_->isChecked();
    out.surfaces = surfaces_->isChecked();
    // TOLERANCE only when it is not the reader's own default: the line is
    // what a person would type.
    const auto tolerance = core::parseFiniteDouble(tolerance_->text().trimmed().toStdString());
    if (tolerance && *tolerance != katana::ifc::ImportOptions{}.curveTolerance) {
        out.tolerance = *tolerance;
    }
    // The dialog always answers, so its line never asks: moved to the
    // origin, the data is in no system and there is nothing to take.
    if (!out.local) {
        out.takeCoordinateSystem = takeCrs_->isChecked();
    }
    return out;
}

QString IfcImportDialog::line() const
{
    if (!check().isEmpty()) {
        return {};
    }
    const auto written = katana::ifc::formatImportLine(arguments());
    return written ? QString::fromStdString(*written) : QString();
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
    if (file_->text().contains('"')) {
        return QStringLiteral("A path holding a double quote cannot be written in a command; "
                              "rename the file.");
    }
    return {};
}

void IfcImportDialog::recheck()
{
    const QString problem = check();
    import_->setEnabled(problem.isEmpty());
    command_->setText(line());
    if (!problem.isEmpty()) {
        showingResult_ = false;
        say(problem, true);
        return;
    }
    if (showingResult_) {
        return;
    }
    say(QStringLiteral("Ready to import, as one step Undo takes back (surfaces are session data "
                       "and stay)."),
        false);
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
    if (pathOf(file_).isEmpty() || !context_.runLine) {
        return;
    }
    const auto written = katana::ifc::formatInfoLine(pathOf(file_).toStdString());
    const auto described = written ? context_.runLine(QString::fromStdString(*written))
                                   : katana::core::Result<std::string>(written.error());
    if (!described) {
        summary_->clear();
        showingResult_ = true;
        say(QString::fromStdString(described.error().describe()), true);
        return;
    }
    setSummary(QString::fromStdString(*described));
}

void IfcImportDialog::setSummary(const QString& text)
{
    summary_->setPlainText(text);
    // Read, the file's earlier failure no longer stands.
    showingResult_ = false;
    recheck();
}

void IfcImportDialog::importFile()
{
    const QString problem = check();
    if (!problem.isEmpty()) {
        say(problem, true);
        return;
    }
    if (!context_.runLine) {
        return;
    }
    const auto reply = context_.runLine(line());
    showingResult_ = true;
    if (!reply) {
        // Declining the far-apart question is a cancel, not a failure.
        const bool cancelled = reply.error().code == katana::core::ErrorCode::CommandRejected;
        say(cancelled ? QStringLiteral("Import cancelled.")
                      : QString::fromStdString(reply.error().describe()),
            !cancelled);
        return;
    }
    const auto head = headRecord(*reply);
    say(QStringLiteral("Imported ") + field(head, "entities") + QStringLiteral(" entities, ") +
            field(head, "alignments") + QStringLiteral(" alignments and ") +
            field(head, "surfaces") + QStringLiteral(" surfaces from ") + pathOf(file_) +
            QStringLiteral(": the log has the reply."),
        false);
}

} // namespace katana::qt
