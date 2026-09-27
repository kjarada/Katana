// GIS > Processing - GDAL > Formats... (formats_dialog.hpp).

#include "geo/formats_dialog.hpp"

#include <QAction>
#include <QComboBox>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QKeySequence>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QSplitter>
#include <QStringList>
#include <QTableWidget>
#include <QVBoxLayout>

#include <string>
#include <vector>

#include "geo/geo_workbench.hpp"
#include "geo/replies.hpp"

namespace katana::qt {

namespace {

namespace geo = katana::app::geo;

enum FormatColumn { kDriver = 0, kDescription, kKinds, kReads, kWrites, kExtensions, kVsi, kColumns };
enum OptionColumn { kList = 0, kName, kType, kDefault, kChoices, kOptionDescription, kOptionColumns };

QString qs(const std::string& text)
{
    return QString::fromStdString(text);
}

// What the one executor prepares for a line that answers at once: its
// records, or the refusal as a record of its own kind.
std::vector<geo::Record> recordsOf(GeoWorkbench& workbench, const QString& line)
{
    auto prepared = geo::prepare(workbench.context(), line.toStdString());
    if (!prepared) {
        return {geo::Record{"error", {{"text", prepared.error().describe()}}, {}}};
    }
    if (!prepared->reply) {
        return {};
    }
    return geo::parseRecords(*prepared->reply);
}

// A word of the line: FORMATS reads RASTER, WRITE ... as its own words, so a
// filter word that is one of them is quoted to stay text. A quote cannot be
// said in a line at all, so it is dropped.
QString filterWord(QString word)
{
    word.remove('"');
    for (const char* keyword : {"RASTER", "VECTOR", "READ", "WRITE", "JSON", "OPTIONS"}) {
        if (word.compare(QLatin1String(keyword), Qt::CaseInsensitive) == 0) {
            return '"' + word + '"';
        }
    }
    return word;
}

QString field(const geo::Record& record, const char* key)
{
    const auto found = record.get(key);
    return found ? qs(*found) : QString();
}

} // namespace

void addFormatsItem(GeoMenus& menus, GeoWorkbench& workbench)
{
    if (!workbench.services().makeAction) {
        return; // a workbench with no window to hang an item on (a test's)
    }
    QAction* action = workbench.services().makeAction(
        Icon::Processing, "&Formats...",
        "Every format this build of GDAL reads and writes, and each driver's options (FORMATS)",
        QKeySequence(), "gisFormats");
    // The dialog it shows, by object name: how --dialog finds it.
    action->setData(QString("gisFormatsDialog"));
    QObject::connect(action, &QAction::triggered, action, [action, &workbench] {
        // One dialog, kept, as the window's other non-modal ones are.
        auto* window = qobject_cast<QWidget*>(action->parent());
        QDialog* dialog =
            window != nullptr ? window->findChild<QDialog*>("gisFormatsDialog") : nullptr;
        if (dialog == nullptr) {
            dialog = new FormatsDialog(workbench, window);
        }
        dialog->show();
        dialog->raise();
        dialog->activateWindow();
    });
    menus.addToGis("Processing - GDAL", action);
}

FormatsDialog::FormatsDialog(GeoWorkbench& workbench, QWidget* parent)
    : QDialog(parent), workbench_(workbench)
{
    setObjectName("gisFormatsDialog");
    setWindowTitle("GDAL Formats");
    setModal(false);
    resize(980, 640);

    kind_ = new QComboBox(this);
    kind_->setObjectName("gisFormatsKind");
    kind_->addItems({"Raster and vector", "Raster", "Vector"});
    capability_ = new QComboBox(this);
    capability_->setObjectName("gisFormatsCapability");
    capability_->addItems({"Reads or writes", "Reads", "Writes"});
    filter_ = new QLineEdit(this);
    filter_->setObjectName("gisFormatsFilter");
    filter_->setPlaceholderText("Words to look for: a name, a description, an extension (fgb)");
    filter_->setClearButtonEnabled(true);

    auto* choices = new QHBoxLayout;
    choices->addWidget(kind_);
    choices->addWidget(capability_);
    choices->addWidget(filter_, 1);

    table_ = new QTableWidget(0, kColumns, this);
    table_->setObjectName("gisFormatsTable");
    table_->setHorizontalHeaderLabels(
        {"Driver", "Description", "Kinds", "Reads", "Writes", "Extensions", "/vsi"});
    table_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    table_->setSelectionBehavior(QAbstractItemView::SelectRows);
    table_->setSelectionMode(QAbstractItemView::SingleSelection);
    table_->verticalHeader()->setVisible(false);
    table_->horizontalHeader()->setStretchLastSection(true);

    options_ = new QTableWidget(0, kOptionColumns, this);
    options_->setObjectName("gisFormatsOptions");
    options_->setHorizontalHeaderLabels(
        {"List", "Name", "Type", "Default", "Choices", "Description"});
    options_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    options_->setSelectionBehavior(QAbstractItemView::SelectRows);
    options_->verticalHeader()->setVisible(false);
    options_->horizontalHeader()->setStretchLastSection(true);
    options_->setToolTip("The selected driver's options: what IMPORT's open options and "
                         "EXPORT's creation and layer options are checked against");

    auto* split = new QSplitter(Qt::Vertical, this);
    split->addWidget(table_);
    split->addWidget(options_);
    split->setStretchFactor(0, 3);
    split->setStretchFactor(1, 2);

    count_ = new QLabel(this);
    count_->setObjectName("gisFormatsCount");
    command_ = new QLabel(this);
    command_->setObjectName("gisFormatsCommand");
    command_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    command_->setToolTip("The line this table shows; type it, or put it in a script, for the "
                         "same records");
    auto* run = new QPushButton("Run in Command Line", this);
    run->setObjectName("gisFormatsRun");
    run->setToolTip("Run the line through the command line, so the records are in its log");
    auto* close = new QPushButton("Close", this);
    close->setObjectName("gisFormatsClose");
    auto* buttons = new QHBoxLayout;
    buttons->addWidget(count_);
    buttons->addWidget(command_, 1);
    buttons->addWidget(run);
    buttons->addWidget(close);

    auto* layout = new QVBoxLayout(this);
    layout->addLayout(choices);
    layout->addWidget(split, 1);
    layout->addLayout(buttons);

    connect(kind_, &QComboBox::currentIndexChanged, this, [this] { refresh(); });
    connect(capability_, &QComboBox::currentIndexChanged, this, [this] { refresh(); });
    connect(filter_, &QLineEdit::textChanged, this, [this] { refresh(); });
    connect(table_, &QTableWidget::itemSelectionChanged, this, [this] { showOptions(); });
    connect(run, &QPushButton::clicked, this, [this] {
        if (workbench_.services().run) {
            (void)workbench_.services().run(line());
        }
    });
    connect(close, &QPushButton::clicked, this, [this] { hide(); });
    refresh();
}

QString FormatsDialog::line() const
{
    QString text = "FORMATS";
    if (kind_->currentIndex() == 1) {
        text += " RASTER";
    } else if (kind_->currentIndex() == 2) {
        text += " VECTOR";
    }
    if (capability_->currentIndex() == 1) {
        text += " READ";
    } else if (capability_->currentIndex() == 2) {
        text += " WRITE";
    }
    for (const QString& word : filter_->text().split(' ', Qt::SkipEmptyParts)) {
        text += ' ' + filterWord(word);
    }
    return text;
}

int FormatsDialog::formatCount() const
{
    return table_->rowCount();
}

void FormatsDialog::refresh()
{
    const QString text = line();
    command_->setText(text);
    const std::vector<geo::Record> records = recordsOf(workbench_, text);
    table_->setSortingEnabled(false);
    table_->setRowCount(0);
    count_->clear();
    for (const geo::Record& record : records) {
        if (record.kind == "error") {
            count_->setText(field(record, "text"));
            continue;
        }
        if (record.kind == "listed") {
            const QString formats = field(record, "formats");
            count_->setText(QString("%1 %2 (GDAL %3)")
                                .arg(formats, formats == "1" ? "format" : "formats",
                                     field(record, "gdal")));
            continue;
        }
        if (record.kind != "format") {
            continue;
        }
        const int row = table_->rowCount();
        table_->insertRow(row);
        const auto yesNo = [](const QString& kinds) { return kinds == "no" ? QString() : kinds; };
        const QString cells[kColumns] = {
            field(record, "driver"),
            field(record, "description"),
            field(record, "kind"),
            yesNo(field(record, "read")),
            yesNo(field(record, "write")),
            field(record, "extensions").replace(',', ' '),
            field(record, "vsi"),
        };
        for (int column = 0; column < kColumns; ++column) {
            table_->setItem(row, column, new QTableWidgetItem(cells[column]));
        }
    }
    table_->resizeColumnsToContents();
    table_->horizontalHeader()->setStretchLastSection(true);
    // In the registry's order (by driver) until a header is clicked.
    table_->horizontalHeader()->setSortIndicator(-1, Qt::AscendingOrder);
    table_->setSortingEnabled(true);
    showOptions();
}

void FormatsDialog::showOptions()
{
    options_->setRowCount(0);
    const QList<QTableWidgetItem*> selected = table_->selectedItems();
    if (selected.isEmpty()) {
        return;
    }
    const QTableWidgetItem* driver = table_->item(selected.front()->row(), kDriver);
    if (driver == nullptr) {
        return;
    }
    QString name = driver->text();
    name.remove('"');
    for (const geo::Record& record :
         recordsOf(workbench_, "FORMATS OPTIONS \"" + name + "\"")) {
        if (record.kind != "option") {
            continue;
        }
        const int row = options_->rowCount();
        options_->insertRow(row);
        const QString cells[kOptionColumns] = {
            field(record, "list"),    field(record, "name"),
            field(record, "type"),    field(record, "default"),
            field(record, "choices").replace(',', ' '), field(record, "description"),
        };
        for (int column = 0; column < kOptionColumns; ++column) {
            options_->setItem(row, column, new QTableWidgetItem(cells[column]));
        }
    }
    options_->resizeColumnsToContents();
    options_->horizontalHeader()->setStretchLastSection(true);
}

} // namespace katana::qt
