// RowTableModel: the read-only table model over plain rows that the manager
// tables share.

#include <gtest/gtest.h>

#include <string>
#include <vector>

#include <QSortFilterProxyModel>

#include "customisation/row_table_model.hpp"

using katana::qt::RowTableModel;

namespace {

struct NameCount {
    std::string name;
    int count = 0;
};

using Model = RowTableModel<NameCount>;

Model makeModel()
{
    std::vector<Model::Column> columns;
    columns.push_back(Model::textColumn(QStringLiteral("Name"), [](const NameCount& row) {
        return QString::fromStdString(row.name);
    }));
    columns.push_back(Model::Column{QStringLiteral("Used by"), [](const NameCount& row, int role) {
                                        if (role == Qt::DisplayRole) {
                                            return QVariant(QString::number(row.count));
                                        }
                                        if (role == katana::qt::kSortRole) {
                                            return QVariant(row.count);
                                        }
                                        return QVariant();
                                    }});
    return Model(std::move(columns));
}

} // namespace

TEST(RowTableModel, EachColumnShowsItsLambdaAndTheTableCannotBeEdited)
{
    Model model = makeModel();
    model.setRows({{"Kerb", 9}, {"Fence", 10}});
    ASSERT_EQ(model.rowCount(), 2);
    ASSERT_EQ(model.columnCount(), 2);
    EXPECT_EQ(model.headerData(1, Qt::Horizontal, Qt::DisplayRole).toString(),
              QStringLiteral("Used by"));
    EXPECT_EQ(model.data(model.index(1, 0), Qt::DisplayRole).toString(), QStringLiteral("Fence"));
    EXPECT_EQ(model.data(model.index(0, 1), Qt::DisplayRole).toString(), QStringLiteral("9"));
    EXPECT_FALSE(model.flags(model.index(0, 0)).testFlag(Qt::ItemIsEditable));
    EXPECT_FALSE(model.setData(model.index(0, 0), QStringLiteral("x"), Qt::EditRole));
    EXPECT_EQ(model.rowAt(1)->name, "Fence");
    EXPECT_EQ(model.rowAt(2), nullptr);
}

TEST(RowTableModel, ACountColumnSortsAsNumbersAndATextColumnFallsBackToItsText)
{
    Model model = makeModel();
    model.setRows({{"b", 10}, {"a", 9}, {"c", 100}});
    QSortFilterProxyModel proxy;
    proxy.setSourceModel(&model);
    proxy.setSortRole(katana::qt::kSortRole);

    // As text "10" < "100" < "9"; as numbers 9 < 10 < 100.
    proxy.sort(1, Qt::AscendingOrder);
    EXPECT_EQ(proxy.index(0, 0).data().toString(), QStringLiteral("a"));
    EXPECT_EQ(proxy.index(1, 0).data().toString(), QStringLiteral("b"));
    EXPECT_EQ(proxy.index(2, 0).data().toString(), QStringLiteral("c"));

    proxy.sort(0, Qt::DescendingOrder);
    EXPECT_EQ(proxy.index(0, 0).data().toString(), QStringLiteral("c"));
}

TEST(RowTableModel, ReloadingResetsTheRowsAndIndexOfFindsARowByItsKey)
{
    Model model = makeModel();
    model.setRows({{"Kerb", 1}});
    int resets = 0;
    QObject::connect(&model, &QAbstractItemModel::modelReset, [&resets] { ++resets; });
    model.setRows({{"Fence", 2}, {"Kerb", 3}});
    EXPECT_EQ(resets, 1);
    EXPECT_EQ(model.indexOf([](const NameCount& row) { return row.name == "Kerb"; }), 1);
    EXPECT_EQ(model.indexOf([](const NameCount& row) { return row.name == "Wall"; }), -1);
}
