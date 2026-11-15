//! The twins of tests/qt_widgets/geo/test_sql_dialog.cpp: GIS > Analysis - GDAL > Query with
//! SQL... (geo::sql_dialog): the line its statement makes, the tables it lists for the scope, and
//! the rows of a run through the window's executor in its grid. The verb itself is tested in
//! katana_app's geo twins. Expected values are the C++'s.

use katana_window::geo::gis_tool_dialog::GisTool;
use katana_window::geo::sql_dialog::{GisSqlDialog, GisSqlForm, gis_sql_line};
use katana_window::host::Host;

use super::support::{Bench, WindowHost, window_context};

fn line_of(made: katana_core::Result<String>) -> String {
    made.unwrap_or_else(|error| panic!("{}", error.describe()))
}

mod gis_sql_line {
    use super::*;

    #[test]
    fn the_statement_is_one_word_of_the_line() {
        let mut form = GisSqlForm::default();
        form.scope.words = String::from("DRAWING");
        form.sql = String::from(
            "SELECT owner,\n  SUM(ST_Area(geometry)) AS area\nFROM polygons GROUP BY owner",
        );
        assert_eq!(
            line_of(gis_sql_line(&form)),
            "GIS SQL \"SELECT owner,   SUM(ST_Area(geometry)) AS area FROM polygons GROUP BY \
             owner\" DRAWING"
        );
        // SQLite reads [gis.source] as "gis.source", and a line can carry it.
        form.sql = String::from("SELECT \"gis.source\" FROM polygons WHERE owner = 'Smith'");
        form.r#as = String::from("select");
        assert_eq!(
            line_of(gis_sql_line(&form)),
            "GIS SQL \"SELECT [gis.source] FROM polygons WHERE owner = 'Smith'\" DRAWING AS \
             SELECT"
        );
        form.r#as = String::from("layer");
        // No layer named.
        assert!(gis_sql_line(&form).is_err());
        form.layer = String::from("results");
        form.dialect = String::from("ogrsql");
        // OGR SQL cannot take the brackets.
        assert!(gis_sql_line(&form).is_err());
        form.sql = String::from("SELECT * FROM polygons");
        assert_eq!(
            line_of(gis_sql_line(&form)),
            "GIS SQL \"SELECT * FROM polygons\" DRAWING dialect=ogrsql AS LAYER results"
        );
        form.sql = String::from("  ");
        assert!(gis_sql_line(&form).is_err());
    }
}

mod gis_sql_dialog {
    use super::*;

    // Two 50 x 40 lots, one owned by Smith: 2000 m2 each.
    #[test]
    fn the_tables_are_listed_and_the_rows_fill_the_grid() {
        let mut host = WindowHost::new("katana-gis-sql-dialog-test");
        for line in [
            "RECT 0,0 50,40",
            "RECT 50,0 100,40",
            "SELECT 1",
            "MODIFY SET PROP=owner:Smith",
        ] {
            host.type_line(line);
        }
        let mut dialog = GisSqlDialog::new(window_context(), &host);
        dialog.show(&host);
        let mut bench = Bench::new("gisSqlDialog", dialog, host);
        for name in [
            "gisSqlScope",
            "gisSqlTables",
            "gisSqlTablesRefresh",
            "gisSqlText",
            "gisSqlDialect",
            "gisSqlAs",
            "gisSqlLayer",
            "gisSqlCsv",
            "gisSqlResults",
            "gisSqlCommand",
            "gisSqlRun",
            "gisSqlReply",
        ] {
            assert!(bench.has(name), "{name}");
        }
        bench.press("gisSqlScopeDrawing");
        bench.dialog.refresh_tables(&bench.host);
        let tables = bench.dialog.tables();
        assert_eq!(tables.len(), 1);
        let polygons = &tables[0];
        assert_eq!(polygons.name, "polygons");
        let columns: Vec<String> = polygons
            .columns
            .iter()
            .map(|(name, kind)| format!("{name}:{kind}"))
            .collect();
        assert!(
            columns.iter().any(|column| column == "katana_id:integer"),
            "{}",
            columns.join(" ")
        );
        assert!(
            columns.iter().any(|column| column == "owner:string"),
            "{}",
            columns.join(" ")
        );
        bench.fill(
            "gisSqlText",
            "SELECT katana_id, owner, ST_Area(geometry) AS area FROM polygons ORDER BY katana_id",
        );
        bench.dialog.run(&mut bench.host);
        let grid = bench.dialog.results();
        assert_eq!(grid.len(), 2, "{}", bench.dialog.reply_text());
        assert_eq!(bench.dialog.result_headers().len(), 3);
        assert_eq!(bench.dialog.result_headers()[2], "area");
        assert_eq!(grid[0][1], "Smith");
        assert_eq!(grid[0][2], "2000");
        // Null.
        assert_eq!(grid[1][1], "");
        assert!(
            bench.dialog.status_text().starts_with("2 rows"),
            "{}",
            bench.dialog.status_text()
        );
        // AS SELECT: the rows' katana_id is the selection.
        bench.fill(
            "gisSqlText",
            "SELECT katana_id FROM polygons WHERE owner = 'Smith'",
        );
        bench.fill("gisSqlAs", "select");
        bench.dialog.run(&mut bench.host);
        let selected = bench.host.document().selection().ids();
        assert_eq!(selected.len(), 1, "{}", bench.dialog.reply_text());
        assert_eq!(selected[0], 1);
    }
}
