#pragma once

// A read-only table over a vector of plain rows, each column a lambda - the
// model behind every manager table (styles and their use counts, library
// definitions, survey-map rules), so none of them keeps a QStandardItemModel
// in step with its data by hand.
//
// Read-only on purpose: a manager's table shows, and a form beside it edits
// (docs/cad.md). flags() gives enabled and selectable, never editable, and
// nothing here writes a row back.
//
// Reloading: a dialog rebuilds its rows (from DocumentWatcher's delivery,
// never inside a Document notification) and calls setRows, which is one
// model reset; indexOf then finds the row it had selected by key, since a
// row index means nothing across a reload.
//
// Sorting: a QSortFilterProxyModel sorts by kSortRole. A column that gives
// nothing for kSortRole sorts by its DisplayRole, so a count column should
// answer kSortRole with a number - "10" sorts before "9" as text.
//
// A template, and so without Q_OBJECT; a QAbstractTableModel needs none to be
// a model.

#include <cstddef>
#include <functional>
#include <utility>
#include <vector>

#include <QAbstractTableModel>
#include <QString>
#include <QVariant>

namespace katana::qt {

// The role a column answers with a value to sort by.
inline constexpr int kSortRole = Qt::UserRole + 100;

template <typename Row>
class RowTableModel : public QAbstractTableModel {
  public:
    struct Column {
        QString title{};
        // The value for any role: DisplayRole, ToolTipRole, ForegroundRole,
        // DecorationRole, TextAlignmentRole, kSortRole... An invalid QVariant
        // for a role the column does not answer.
        std::function<QVariant(const Row& row, int role)> data{};
    };

    // A column that shows `text(row)` and answers no other role.
    template <typename Text>
    [[nodiscard]] static Column textColumn(QString title, Text text)
    {
        return Column{std::move(title), [text = std::move(text)](const Row& row, int role) {
                          return role == Qt::DisplayRole ? QVariant(text(row)) : QVariant();
                      }};
    }

    explicit RowTableModel(std::vector<Column> columns, QObject* parent = nullptr)
        : QAbstractTableModel(parent), columns_(std::move(columns))
    {
    }

    // Replaces every row: one model reset, so views drop their selection and
    // the dialog restores it through indexOf.
    void setRows(std::vector<Row> rows)
    {
        beginResetModel();
        rows_ = std::move(rows);
        endResetModel();
    }
    [[nodiscard]] const std::vector<Row>& rows() const { return rows_; }
    // The row at a SOURCE model row, or nullptr (map a proxy's index first).
    [[nodiscard]] const Row* rowAt(int row) const
    {
        return row >= 0 && static_cast<std::size_t>(row) < rows_.size()
                   ? &rows_[static_cast<std::size_t>(row)]
                   : nullptr;
    }
    // The first row `matches` accepts, or -1.
    [[nodiscard]] int indexOf(const std::function<bool(const Row&)>& matches) const
    {
        for (std::size_t i = 0; i < rows_.size(); ++i) {
            if (matches(rows_[i])) {
                return static_cast<int>(i);
            }
        }
        return -1;
    }

    [[nodiscard]] int rowCount(const QModelIndex& parent = QModelIndex()) const override
    {
        return parent.isValid() ? 0 : static_cast<int>(rows_.size());
    }
    [[nodiscard]] int columnCount(const QModelIndex& parent = QModelIndex()) const override
    {
        return parent.isValid() ? 0 : static_cast<int>(columns_.size());
    }
    [[nodiscard]] QVariant data(const QModelIndex& index, int role) const override
    {
        const Row* row = rowAt(index.row());
        if (row == nullptr || index.column() < 0 ||
            static_cast<std::size_t>(index.column()) >= columns_.size()) {
            return {};
        }
        const Column& column = columns_[static_cast<std::size_t>(index.column())];
        if (!column.data) {
            return {};
        }
        QVariant value = column.data(*row, role);
        if (role == kSortRole && !value.isValid()) {
            return column.data(*row, Qt::DisplayRole);
        }
        return value;
    }
    [[nodiscard]] QVariant headerData(int section, Qt::Orientation orientation,
                                      int role) const override
    {
        if (orientation == Qt::Horizontal && role == Qt::DisplayRole && section >= 0 &&
            static_cast<std::size_t>(section) < columns_.size()) {
            return columns_[static_cast<std::size_t>(section)].title;
        }
        return QAbstractTableModel::headerData(section, orientation, role);
    }
    [[nodiscard]] Qt::ItemFlags flags(const QModelIndex& index) const override
    {
        return index.isValid() ? Qt::ItemIsEnabled | Qt::ItemIsSelectable : Qt::NoItemFlags;
    }

  private:
    std::vector<Column> columns_;
    std::vector<Row> rows_{};
};

} // namespace katana::qt
