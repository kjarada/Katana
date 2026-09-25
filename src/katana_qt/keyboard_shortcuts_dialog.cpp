#include "keyboard_shortcuts_dialog.hpp"

#include <QHBoxLayout>
#include <QHeaderView>
#include <QKeySequence>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QSet>
#include <QTableWidget>
#include <QVBoxLayout>

#include <algorithm>

#include "theme.hpp"

namespace katana::qt {

namespace {

enum Column { kKey = 0, kCommand, kMenu, kTip, kColumns };

} // namespace

KeyboardShortcutsDialog::KeyboardShortcutsDialog(std::vector<ShortcutRow> rows,
                                                 QStringList clashes, QWidget* parent)
    : QDialog(parent), rows_(std::move(rows))
{
    setObjectName("keyboardShortcutsDialog");
    setWindowTitle("Keyboard Shortcuts");
    setModal(false);
    resize(820, 560);

    search_ = new QLineEdit(this);
    search_->setObjectName("keyboardShortcutsSearch");
    search_->setPlaceholderText("Search: a key (Ctrl+L), a command or a menu");
    search_->setClearButtonEnabled(true);

    // The keys of the clashes: a clash line is "<key>: <what shares it>".
    QSet<QString> clashing;
    for (const QString& clash : clashes) {
        clashing.insert(clash.section(": ", 0, 0));
    }

    table_ = new QTableWidget(static_cast<int>(rows_.size()), kColumns, this);
    table_->setObjectName("keyboardShortcutsTable");
    table_->setHorizontalHeaderLabels({"Key", "Command", "Menu", "What it does"});
    table_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    table_->setSelectionBehavior(QAbstractItemView::SelectRows);
    table_->verticalHeader()->setVisible(false);
    table_->horizontalHeader()->setStretchLastSection(true);
    for (std::size_t r = 0; r < rows_.size(); ++r) {
        const ShortcutRow& row = rows_[r];
        const bool clash = clashing.contains(row.key);
        const QString key = QKeySequence(row.key, QKeySequence::PortableText)
                                .toString(QKeySequence::NativeText) +
                            (clash ? " (clash)" : "");
        const int at = static_cast<int>(r);
        for (const auto& [column, text] :
             {std::pair{kKey, key}, std::pair{kCommand, row.command}, std::pair{kMenu, row.menu},
              std::pair{kTip, row.tip}}) {
            auto* cell = new QTableWidgetItem(text);
            if (clash) {
                cell->setForeground(theme::error());
            }
            table_->setItem(at, column, cell);
        }
    }
    table_->resizeColumnsToContents();
    table_->horizontalHeader()->setStretchLastSection(true);
    // Sorted by a click on a header; in the menus' order until then, which
    // an indicator on no column keeps.
    table_->horizontalHeader()->setSortIndicator(-1, Qt::AscendingOrder);
    table_->setSortingEnabled(true);
    table_->horizontalHeader()->setSortIndicatorShown(true);

    auto* clashLabel = new QLabel(this);
    clashLabel->setObjectName("keyboardShortcutsClashes");
    clashLabel->setWordWrap(true);
    clashLabel->setTextInteractionFlags(Qt::TextSelectableByMouse);
    clashLabel->setText(clashes.isEmpty()
                            ? QString("Every key reaches one command.")
                            : QString("%1 %2 more than one command, and Qt gives such a key "
                                      "to none of them: %3")
                                  .arg(clashes.size())
                                  .arg(clashes.size() == 1 ? "key reaches" : "keys reach",
                                       clashes.join("; ")));

    count_ = new QLabel(this);
    count_->setObjectName("keyboardShortcutsCount");
    auto* close = new QPushButton("Close", this);
    close->setObjectName("keyboardShortcutsClose");
    auto* buttons = new QHBoxLayout;
    buttons->addWidget(count_, 1);
    buttons->addWidget(close);

    auto* layout = new QVBoxLayout(this);
    layout->addWidget(search_);
    layout->addWidget(table_, 1);
    layout->addWidget(clashLabel);
    layout->addLayout(buttons);

    connect(search_, &QLineEdit::textChanged, this, [this](const QString& text) { setFilter(text); });
    connect(close, &QPushButton::clicked, this, [this] { hide(); });
    setFilter({});
}

void KeyboardShortcutsDialog::setFilter(const QString& text)
{
    if (search_->text() != text) {
        search_->setText(text); // comes back here through textChanged
        return;
    }
    const QStringList words = text.split(' ', Qt::SkipEmptyParts);
    int shown = 0;
    for (int r = 0; r < table_->rowCount(); ++r) {
        QString haystack;
        for (int column = 0; column < kColumns; ++column) {
            if (const QTableWidgetItem* cell = table_->item(r, column)) {
                haystack += cell->text() + ' ';
            }
        }
        const bool match = std::ranges::all_of(words, [&haystack](const QString& word) {
            return haystack.contains(word, Qt::CaseInsensitive);
        });
        table_->setRowHidden(r, !match);
        shown += match ? 1 : 0;
    }
    count_->setText(words.isEmpty() ? QString("%1 keys").arg(table_->rowCount())
                                    : QString("%1 of %2 keys").arg(shown).arg(table_->rowCount()));
}

int KeyboardShortcutsDialog::shownRows() const
{
    int shown = 0;
    for (int r = 0; r < table_->rowCount(); ++r) {
        shown += table_->isRowHidden(r) ? 0 : 1;
    }
    return shown;
}

} // namespace katana::qt
