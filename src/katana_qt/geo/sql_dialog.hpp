#pragma once

// GIS > Analysis - GDAL > Query with SQL...: the window onto GIS SQL
// (docs/geoprocessing.md, "V5"), built on the GIS dialog frame
// (gis_tool_dialog.hpp): the statement and its options make the line, shown
// as it will run, and Run hands it to the window's one executor; the rows
// come back into a grid.
//
// Its fields, by object name (the frame's are in gis_tool_dialog.hpp; the
// scope's are gisSqlScope, gisSqlScopeDrawing, ...):
//   gisSqlTables         the tables the scope becomes - points, lines,
//                        polygons - and each one's columns with their types
//   gisSqlTablesRefresh  list them again for the scope as it is now
//   gisSqlText           the SELECT statement
//   gisSqlDialect        sqlite (Spatialite's functions) | ogrsql
//   gisSqlAs             report | select | layer: the rows, the entities
//                        their katana_id names selected, or their geometry
//                        drawn
//   gisSqlLayer          AS LAYER: the layer
//   gisSqlCsv            AS REPORT: a .csv to write the rows to too
//   gisSqlResults        the rows of the last reply, a column each

#include <QString>

#include "geo/gis_tool_dialog.hpp"

class QComboBox;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;
class QTableWidget;
class QTreeWidget;

namespace katana::qt {

struct GisSqlForm {
    GisScopeWords scope;
    QString sql;
    QString dialect = "sqlite";
    QString as = "report"; // report | select | layer
    QString layer;
    QString csv;
};

// GIS SQL "<sql>" <scope> [dialect=ogrsql] [AS SELECT | AS LAYER <layer>]
// [csv=<file>], exactly as it would be typed; the defaults (sqlite, AS
// REPORT) left out. A statement's line breaks become blanks - a line has
// one - and SQLite's double-quoted identifiers become [bracketed] ones,
// which a line can carry. InvalidArgument naming the field for an empty
// statement, a layer missing for AS LAYER, a double quote the line cannot
// carry, and a scope the controls cannot say.
[[nodiscard]] katana::core::Result<QString> gisSqlLine(const GisSqlForm& form);

class GisSqlDialog final : public GisToolDialog {
  public:
    explicit GisSqlDialog(GisDialogContext context, QWidget* parent = nullptr);
    [[nodiscard]] GisSqlForm form() const;
    [[nodiscard]] katana::core::Result<QString> command() const override;
    void reload() override;
    // Lists the tables and their columns for the scope as it is now: what
    // the drawing's properties would be as columns.
    void refreshTables();
    [[nodiscard]] QTableWidget* results() const { return results_; }

  protected:
    [[nodiscard]] QString summary(const QString& reply) const override;
    void replied(const QString& reply) override;

  private:
    void showResults(const QString& reply);

    QTreeWidget* tables_ = nullptr;
    QPushButton* refresh_ = nullptr;
    QPlainTextEdit* sql_ = nullptr;
    QComboBox* dialect_ = nullptr;
    QComboBox* as_ = nullptr;
    QLineEdit* layer_ = nullptr;
    QLineEdit* csv_ = nullptr;
    QTableWidget* results_ = nullptr;
};

} // namespace katana::qt
