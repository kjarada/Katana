// GIS > Analysis - GDAL > Query with SQL... (sql_dialog.hpp).

#include "geo/sql_dialog.hpp"

#include <QComboBox>
#include <QFontDatabase>
#include <QFormLayout>
#include <QHeaderView>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QTableWidget>
#include <QTreeWidget>

#include <map>
#include <string>
#include <utility>
#include <vector>

#include "geo/replies.hpp"
#include "geo/vector_support.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/global_modify.hpp"
#include "katana/interop/geo/drawing_dataset.hpp"

namespace katana::qt {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

namespace gp = katana::gis::processing;

katana::core::Error invalid(const QString& message)
{
    return makeError(ErrorCode::InvalidArgument, message.toStdString());
}

QString typeName(gp::FieldType type)
{
    switch (type) {
    case gp::FieldType::Boolean:
        return "boolean";
    case gp::FieldType::Integer64:
        return "integer";
    case gp::FieldType::Real:
        return "real";
    case gp::FieldType::Date:
        return "date";
    case gp::FieldType::DateTime:
        return "datetime";
    case gp::FieldType::String:
        break;
    }
    return "string";
}

} // namespace

Result<QString> gisSqlLine(const GisSqlForm& form)
{
    if (form.sql.trimmed().isEmpty()) {
        return invalid("write the SELECT statement first");
    }
    // One word of the line, as katana_gis_query writes it.
    const auto made =
        katana::app::geo::vector::sqlForLine(form.sql.toStdString(), form.dialect == "sqlite");
    if (!made) {
        return made.error();
    }
    const QString sql = QString::fromStdString(*made);
    const auto scope = gisScope(form.scope);
    if (!scope) {
        return scope.error();
    }
    QString line = "GIS SQL \"" + sql + "\" " + *scope;
    if (form.dialect != "sqlite") {
        line += " dialect=" + form.dialect;
    }
    if (form.as == "select") {
        line += " AS SELECT";
    } else if (form.as == "layer") {
        const QString layer = form.layer.trimmed();
        if (layer.isEmpty()) {
            return invalid("AS LAYER draws the rows' geometry: name the layer");
        }
        auto word = gisWord(layer, "The layer");
        if (!word) {
            return word.error();
        }
        line += " AS LAYER " + *word;
    } else if (const QString csv = form.csv.trimmed(); !csv.isEmpty()) {
        auto word = gisWord("csv=" + csv, "The rows' file");
        if (!word) {
            return word.error();
        }
        line += " " + *word;
    }
    return line;
}

GisSqlDialog::GisSqlDialog(GisDialogContext context, QWidget* parent)
    : GisToolDialog("gisSql", "Query with SQL", std::move(context), parent)
{
    resize(1180, 900);
    tables_ = new QTreeWidget(this);
    tables_->setObjectName("gisSqlTables");
    tables_->setHeaderLabels({"Table / column", "Type"});
    tables_->setToolTip("The tables the scope becomes, and the columns each has: katana_id, "
                        "layer, style, colour and type, then the properties; the geometry "
                        "column is geometry");
    tables_->setMinimumHeight(150);
    fields().addRow("Tables:", tables_);
    refresh_ = new QPushButton("List the tables for this scope", this);
    refresh_->setObjectName("gisSqlTablesRefresh");
    refresh_->setAutoDefault(false);
    fields().addRow(QString(), refresh_);
    sql_ = new QPlainTextEdit(this);
    sql_->setObjectName("gisSqlText");
    sql_->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    sql_->setPlaceholderText(
        "SELECT owner, SUM(ST_Area(geometry)) AS area FROM polygons GROUP BY owner");
    sql_->setToolTip("One SELECT: a query reads the drawing and never changes it");
    sql_->setMinimumHeight(80);
    fields().addRow("Statement:", sql_);
    dialect_ = new QComboBox(this);
    dialect_->setObjectName("gisSqlDialect");
    dialect_->addItems({"sqlite", "ogrsql"});
    dialect_->setToolTip("sqlite: SQLite with Spatialite's ST_ functions; ogrsql: OGR's own SQL");
    fields().addRow("Dialect:", dialect_);
    as_ = new QComboBox(this);
    as_->setObjectName("gisSqlAs");
    as_->addItems({"report", "select", "layer"});
    as_->setToolTip("report: the rows; select: the entities their katana_id names; layer: their "
                    "geometry drawn, one undo step");
    fields().addRow("Answer as:", as_);
    layer_ = addField("gisSqlLayer", "Layer (AS LAYER):", "the layer to draw on",
                      "AS LAYER: where the rows' geometry is drawn");
    csv_ = addField("gisSqlCsv", "Rows to CSV:", "none",
                    "csv=: AS REPORT - the rows written to this file too");
    results_ = new QTableWidget(this);
    results_->setObjectName("gisSqlResults");
    results_->setEditTriggers(QAbstractItemView::NoEditTriggers);
    results_->setMinimumHeight(160);
    results_->horizontalHeader()->setStretchLastSection(true);
    fields().addRow("Rows:", results_);

    connect(sql_, &QPlainTextEdit::textChanged, this, [this] { refreshCommand(); });
    connect(dialect_, &QComboBox::currentTextChanged, this, [this] { refreshCommand(); });
    connect(as_, &QComboBox::currentTextChanged, this, [this] {
        layer_->setEnabled(as_->currentText() == "layer");
        csv_->setEnabled(as_->currentText() == "report");
        refreshCommand();
    });
    connect(refresh_, &QPushButton::clicked, this, [this] { refreshTables(); });
    layer_->setEnabled(false);
    refreshCommand();
}

void GisSqlDialog::reload()
{
    GisToolDialog::reload();
    refreshTables();
}

void GisSqlDialog::refreshTables()
{
    if (tables_ == nullptr) {
        return;
    }
    tables_->clear();
    if (!documentAlive() || scopeControls() == nullptr) {
        return;
    }
    // What the scope takes now, converted as the verb converts it: only to
    // list the columns, never to answer the query.
    const auto scope = scopeControls()->scope();
    const auto filter = scopeControls()->filter();
    if (!scope || !filter) {
        return;
    }
    const katana::cad::Document& document = *context().document;
    auto ids = katana::cad::matchEntities(document, *scope, *filter);
    if (!ids) {
        return;
    }
    auto dataset = katana::interop::geo::drawingDataset(document.model(), *ids);
    if (!dataset) {
        return;
    }
    for (const gp::FeatureTable& table : dataset->set.tables) {
        auto* item = new QTreeWidgetItem(
            tables_, {QString::fromStdString(table.name), QString::number(table.features.size()) +
                                                               " rows"});
        for (const gp::FieldDef& field : table.fields) {
            (void)new QTreeWidgetItem(item, {QString::fromStdString(field.name), typeName(field.type)});
        }
        (void)new QTreeWidgetItem(item, {"geometry", "geometry"});
    }
    tables_->expandAll();
    tables_->resizeColumnToContents(0);
}

GisSqlForm GisSqlDialog::form() const
{
    GisSqlForm form;
    form.scope = scopeWords();
    form.sql = sql_ != nullptr ? sql_->toPlainText() : QString();
    form.dialect = dialect_ != nullptr ? dialect_->currentText() : QString("sqlite");
    form.as = as_ != nullptr ? as_->currentText() : QString("report");
    form.layer = layer_ != nullptr ? layer_->text() : QString();
    form.csv = csv_ != nullptr ? csv_->text() : QString();
    return form;
}

Result<QString> GisSqlDialog::command() const
{
    return gisSqlLine(form());
}

void GisSqlDialog::replied(const QString& reply)
{
    showResults(reply);
}

void GisSqlDialog::showResults(const QString& reply)
{
    results_->clear();
    results_->setRowCount(0);
    std::vector<std::pair<std::string, std::string>> columns; // key, name
    std::vector<katana::app::geo::Record> rows;
    for (auto& record : katana::app::geo::parseRecords(reply.toStdString())) {
        if (record.kind == "column") {
            const std::string name = record.get("name").value_or("");
            columns.emplace_back(record.get("key").value_or(name), name);
        } else if (record.kind == "row") {
            rows.push_back(std::move(record));
        }
    }
    results_->setColumnCount(static_cast<int>(columns.size()));
    QStringList headers;
    for (const auto& [key, name] : columns) {
        headers << QString::fromStdString(name);
    }
    results_->setHorizontalHeaderLabels(headers);
    for (const auto& row : rows) {
        const int at = results_->rowCount();
        results_->insertRow(at);
        for (std::size_t c = 0; c < columns.size(); ++c) {
            results_->setItem(at, static_cast<int>(c),
                              new QTableWidgetItem(QString::fromStdString(
                                  row.get(columns[c].first).value_or(std::string()))));
        }
    }
}

QString GisSqlDialog::summary(const QString& reply) const
{
    if (reply.contains(" target=selection ")) {
        return GisToolDialog::summary(reply).startsWith("Done.")
                   ? "Selected: the entities the rows name are the selection."
                   : GisToolDialog::summary(reply);
    }
    if (reply.contains(" target=report ")) {
        int rows = 0;
        for (const QString& line : reply.split('\n')) {
            rows += line.startsWith("row ") || line == "row" ? 1 : 0;
        }
        return QString("%1 rows: in the grid, and as row records in the reply.").arg(rows);
    }
    return GisToolDialog::summary(reply);
}

} // namespace katana::qt
