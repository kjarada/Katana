#include "corridor_quantities_dialog.hpp"

#include <QApplication>
#include <QClipboard>
#include <QComboBox>
#include <QFileDialog>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QTableWidget>
#include <QVBoxLayout>

#include <array>
#include <optional>
#include <string>
#include <utility>

#include "customisation/document_watcher.hpp"
#include "katana/cad/bim/bim_object.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/road/corridor_model.hpp"
#include "katana/core/text.hpp"
#include "road_dialog_support.hpp"

namespace katana::qt {

using road_ui::qs;

namespace {

constexpr const char* kAll = "All corridors";

// The quantities a row shows, by the names the records carry them under.
constexpr std::array<const char*, 6> kQuantities{"Length", "Width",     "Depth",
                                                 "Height", "GrossArea", "GrossVolume"};

} // namespace

struct CorridorQuantitiesDialog::Impl {
    CorridorQuantitiesDialog* dialog = nullptr;
    CorridorQuantitiesContext context;
    std::unique_ptr<QObject> guard = std::make_unique<QObject>();
    std::unique_ptr<DocumentWatcher> watcher;

    QComboBox* corridor = nullptr;
    QTableWidget* table = nullptr;
    QLabel* totals = nullptr;
    QLineEdit* csvPath = nullptr;
    QLabel* status = nullptr;
    // The road model the table and the totals were listed from: the object
    // CORRIDOR QUANTITIES reads (bim::bimModel), which is the same object for
    // as long as nothing in the road model changes. A different one means the
    // listing is of a drawing that is no longer there.
    std::shared_ptr<const katana::cad::bim::BimModel> listedFrom;

    [[nodiscard]] bool alive() const { return watcher != nullptr && watcher->documentAlive(); }

    template <typename Sender, typename Signal, typename Slot>
    void connect(Sender* sender, Signal signal, Slot slot)
    {
        QObject::connect(sender, signal, guard.get(), std::move(slot));
    }

    std::optional<QString> which()
    {
        if (corridor->currentText().isEmpty() || corridor->currentText() == kAll) {
            return QString("ALL");
        }
        return road_ui::word(status, corridor->currentText(), "the corridor's name");
    }

    // Runs the line and shows its records: an object a row, the rest under it.
    void show(const QString& line)
    {
        if (!alive() || !context.run) {
            road_ui::setStatus(status, "Nothing here can run " + line + ".", true);
            return;
        }
        const VerbOutcome outcome = context.run(line);
        if (!outcome.ok) {
            road_ui::setStatus(
                status, outcome.error.isEmpty() ? line + " was refused." : outcome.error, true);
            return;
        }
        table->setRowCount(0);
        totals->clear();
        QStringList summary;
        for (const QString& text : outcome.reply.split('\n', Qt::SkipEmptyParts)) {
            const auto record = katana::core::readReplyRecord(text.toStdString());
            if (!record || record->words.empty()) {
                continue;
            }
            const std::string& kind = record->words.front();
            if (kind == "object") {
                const int row = table->rowCount();
                table->insertRow(row);
                table->setItem(row, 0, road_ui::cell(qs(record->value("id").value_or(""))));
                table->setItem(row, 1, road_ui::cell(qs(record->value("kind").value_or(""))));
                for (std::size_t i = 0; i < kQuantities.size(); ++i) {
                    table->setItem(row, static_cast<int>(i) + 2,
                                   road_ui::cell(qs(record->value(kQuantities[i]).value_or(""))));
                }
            } else if (kind == "quantities") {
                summary.prepend(QString("Cut %1 m3, fill %2 m3, net %3 m3 (fill less cut) "
                                        "over %4 objects")
                                    .arg(qs(record->value("cut").value_or("")),
                                         qs(record->value("fill").value_or("")),
                                         qs(record->value("net").value_or("")),
                                         qs(record->value("objects").value_or(""))));
            } else if (kind == "total") {
                summary << QString("%1 %2: %3 of them, %4 m long, %5 m2, %6 m3")
                               .arg(qs(record->value("kind").value_or("")),
                                    qs(record->value("material").value_or("")),
                                    qs(record->value("count").value_or("")),
                                    qs(record->value("length").value_or("")),
                                    qs(record->value("area").value_or("")),
                                    qs(record->value("volume").value_or("")));
            } else if (kind == "problem") {
                summary << "Problem: " + qs(record->value("message").value_or(""));
            }
        }
        totals->setText(summary.join('\n'));
        // The line has run and read the road model; the model it read is the
        // one the drawing holds now.
        listedFrom = katana::cad::bim::bimModel(*context.document);
        const QStringList lines = outcome.reply.split('\n', Qt::SkipEmptyParts);
        road_ui::setStatus(status, lines.isEmpty() ? line : lines.back(), false);
    }

    // After a change to the drawing - an edit, an undo, a redo, a new or opened
    // drawing - the listing is cleared and the status says to list again when
    // the road model it was listed from is no longer the drawing's. It is not
    // listed again by itself: that would be a CORRIDOR QUANTITIES line in the
    // command log after every edit, and a change that left the road model as
    // it was (a line drawn) leaves the listing alone. True when it cleared.
    bool dropIfStale()
    {
        if (!alive() || listedFrom == nullptr ||
            katana::cad::bim::bimModel(*context.document) == listedFrom) {
            return false;
        }
        listedFrom.reset();
        table->setRowCount(0);
        totals->clear();
        road_ui::setStatus(
            status, "The drawing changed since the list: press List Quantities again.", true);
        return true;
    }

    void reload()
    {
        if (!alive()) {
            return;
        }
        const QString keep = corridor->currentText();
        corridor->clear();
        corridor->addItem(kAll);
        for (const auto& each : context.document->corridors().corridors) {
            corridor->addItem(qs(each.name));
        }
        const int at = corridor->findText(keep);
        corridor->setCurrentIndex(at >= 0 ? at : 0);
    }

    void build();
};

void CorridorQuantitiesDialog::Impl::build()
{
    dialog->setObjectName("corridorQuantitiesDialog");
    dialog->setWindowTitle("Corridor Quantities");
    auto* layout = new QVBoxLayout(dialog);
    auto* top = new QHBoxLayout;
    corridor = new QComboBox(dialog);
    corridor->setObjectName("corridorQuantitiesCorridor");
    corridor->setToolTip("The corridor whose quantities are listed, or all of them");
    auto* runButton = road_ui::button("List Quantities", "corridorQuantitiesRun",
                                      "CORRIDOR QUANTITIES: every object's quantities and the "
                                      "totals",
                                      dialog);
    top->addWidget(corridor, 1);
    top->addWidget(runButton);
    layout->addLayout(top);
    table = road_ui::table("corridorQuantitiesTable",
                           {"Object", "Kind", "Length (m)", "Width (m)", "Depth (m)", "Height (m)",
                            "Area (m2)", "Volume (m3)"},
                           dialog);
    layout->addWidget(table, 1);
    totals = road_ui::muted(QString(), dialog);
    totals->setObjectName("corridorQuantitiesTotals");
    layout->addWidget(totals);
    auto* csvRow = new QHBoxLayout;
    csvPath = road_ui::field("corridorQuantitiesCsvPath", "quantities.csv",
                             "The file Save CSV writes", dialog);
    auto* browse = road_ui::button("Browse...", "corridorQuantitiesBrowse",
                                   "Choose the file (no file dialog in a headless run)", dialog);
    auto* save = road_ui::button("Save CSV", "corridorQuantitiesSaveCsv",
                                 "CORRIDOR QUANTITIES ... csv=: the table as a CSV file", dialog);
    auto* copy = road_ui::button("Copy", "corridorQuantitiesCopy",
                                 "The table to the clipboard, tab separated", dialog);
    csvRow->addWidget(csvPath, 1);
    csvRow->addWidget(browse);
    csvRow->addWidget(save);
    csvRow->addWidget(copy);
    layout->addLayout(csvRow);
    status = new QLabel(dialog);
    status->setObjectName("corridorQuantitiesStatus");
    status->setWordWrap(true);
    layout->addWidget(status);
    auto* closeRow = new QHBoxLayout;
    closeRow->addStretch(1);
    auto* closeButton =
        road_ui::button("Close", "corridorQuantitiesClose", "Close the dialog", dialog);
    closeRow->addWidget(closeButton);
    layout->addLayout(closeRow);

    connect(runButton, &QPushButton::clicked, [this] {
        if (const auto corridorWord = which()) {
            show("CORRIDOR QUANTITIES " + *corridorWord);
        }
    });
    connect(browse, &QPushButton::clicked, [this] {
        if (context.headless && context.headless()) {
            road_ui::setStatus(status,
                               "A headless session opens no file dialog: type the path in the "
                               "field",
                               true);
            return;
        }
        const QString chosen =
            QFileDialog::getSaveFileName(dialog, "Save Quantities", csvPath->text(), "CSV (*.csv)");
        if (!chosen.isEmpty()) {
            csvPath->setText(chosen);
        }
    });
    connect(save, &QPushButton::clicked, [this] {
        const auto corridorWord = which();
        if (!corridorWord) {
            return;
        }
        if (csvPath->text().trimmed().isEmpty()) {
            road_ui::setStatus(status, "Type the file to save, or Browse for it.", true);
            return;
        }
        const auto file = road_ui::option(status, "csv", csvPath->text().trimmed());
        if (file) {
            show("CORRIDOR QUANTITIES " + *corridorWord + " " + *file);
        }
    });
    connect(copy, &QPushButton::clicked, [this] {
        // The watcher's delivery may not have run yet: never hand out a
        // listing of a road model the drawing no longer has.
        if (dropIfStale()) {
            return;
        }
        QStringList rows;
        QStringList header;
        for (int column = 0; column < table->columnCount(); ++column) {
            header << table->horizontalHeaderItem(column)->text();
        }
        rows << header.join('\t');
        for (int row = 0; row < table->rowCount(); ++row) {
            QStringList cells;
            for (int column = 0; column < table->columnCount(); ++column) {
                const QTableWidgetItem* item = table->item(row, column);
                cells << (item != nullptr ? item->text() : QString());
            }
            rows << cells.join('\t');
        }
        QApplication::clipboard()->setText(rows.join('\n'));
        road_ui::setStatus(status, QString("%1 rows copied.").arg(table->rowCount()), false);
    });
    connect(closeButton, &QPushButton::clicked, [this] { dialog->close(); });
}

CorridorQuantitiesDialog::CorridorQuantitiesDialog(CorridorQuantitiesContext context,
                                                   QWidget* parent)
    : QDialog(parent), impl_(std::make_unique<Impl>())
{
    impl_->dialog = this;
    impl_->context = std::move(context);
    impl_->build();
    if (impl_->context.document != nullptr) {
        impl_->watcher = std::make_unique<DocumentWatcher>(*impl_->context.document,
                                                           [this](const DocumentChanges& changes) {
                                                               if (changes.model) {
                                                                   impl_->reload();
                                                                   impl_->dropIfStale();
                                                               }
                                                           });
    }
    impl_->reload();
    resize(860, 560);
}

CorridorQuantitiesDialog::~CorridorQuantitiesDialog()
{
    impl_->guard.reset();
}

} // namespace katana::qt
