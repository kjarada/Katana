//! GIS > Analysis - GDAL > Query with SQL...: the window onto GIS SQL (geo/sql_dialog.hpp;
//! docs/geoprocessing.md, "V5"), built on the GIS dialog frame (gis_tool_dialog): the statement
//! and its options make the line, shown as it will run, and Run hands it to the window's one
//! executor; the rows come back into a grid.
//!
//! Its fields, by object name (the frame's are in gis_tool_dialog; the scope's are gisSqlScope,
//! gisSqlScopeDrawing, ...):
//!
//! ```text
//! gisSqlTables         the tables the scope becomes - points, lines, polygons - and each one's
//!                      columns with their types
//! gisSqlTablesRefresh  list them again for the scope as it is now
//! gisSqlText           the SELECT statement
//! gisSqlDialect        sqlite (Spatialite's functions) | ogrsql
//! gisSqlAs             report | select | layer: the rows, the entities their katana_id names
//!                      selected, or their geometry drawn
//! gisSqlLayer          AS LAYER: the layer
//! gisSqlCsv            AS REPORT: a .csv to write the rows to too
//! gisSqlResults        the rows of the last reply, a column each
//! ```

use egui::Ui;
use katana_app::geo::replies::parse_records;
use katana_app::geo::vector_support::sql_for_line;
use katana_cad::global_modify::match_entities;
use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result};
use katana_gis::processing::FieldType;
use katana_interop::geo::DrawingDatasetOptions;
use katana_interop::geo::drawing_dataset::drawing_dataset;

use super::buffer_dialog::Words;
use super::gis_tool_dialog::{
    GisDialogContext, GisScopeWords, GisTool, GisToolFrame, gis_field, gis_scope, gis_summary,
    gis_word,
};
use crate::annotation::dialog_support::table;
use crate::drive::{TextOptions, button, text_with};
use crate::host::Host;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GisSqlForm {
    pub scope: GisScopeWords,
    pub sql: String,
    pub dialect: String,
    /// report | select | layer
    pub r#as: String,
    pub layer: String,
    pub csv: String,
}

impl Default for GisSqlForm {
    fn default() -> Self {
        Self {
            scope: GisScopeWords::default(),
            sql: String::new(),
            dialect: String::from("sqlite"),
            r#as: String::from("report"),
            layer: String::new(),
            csv: String::new(),
        }
    }
}

fn invalid(message: &str) -> Error {
    make_error(ErrorCode::InvalidArgument, message, String::new())
}

fn type_name(kind: FieldType) -> &'static str {
    match kind {
        FieldType::Boolean => "boolean",
        FieldType::Integer64 => "integer",
        FieldType::Real => "real",
        FieldType::Date => "date",
        FieldType::DateTime => "datetime",
        FieldType::String => "string",
    }
}

/// GIS SQL "<sql>" <scope> [dialect=ogrsql] [AS SELECT | AS LAYER <layer>] [csv=<file>], exactly
/// as it would be typed; the defaults (sqlite, AS REPORT) left out. A statement's line breaks
/// become blanks - a line has one - and SQLite's double-quoted identifiers become [bracketed]
/// ones, which a line can carry. InvalidArgument naming the field for an empty statement, a layer
/// missing for AS LAYER, a double quote the line cannot carry, and a scope the controls cannot
/// say.
pub fn gis_sql_line(form: &GisSqlForm) -> Result<String> {
    if form.sql.trim().is_empty() {
        return Err(invalid("write the SELECT statement first"));
    }
    // One word of the line, as katana_gis_query writes it.
    let sql = sql_for_line(&form.sql, form.dialect == "sqlite")?;
    let scope = gis_scope(&form.scope)?;
    let mut line = format!("GIS SQL \"{sql}\" {scope}");
    if form.dialect != "sqlite" {
        line.push_str(" dialect=");
        line.push_str(&form.dialect);
    }
    if form.r#as == "select" {
        line.push_str(" AS SELECT");
    } else if form.r#as == "layer" {
        let layer = form.layer.trim();
        if layer.is_empty() {
            return Err(invalid("AS LAYER draws the rows' geometry: name the layer"));
        }
        line.push_str(" AS LAYER ");
        line.push_str(&gis_word(layer, "The layer")?);
    } else {
        let csv = form.csv.trim();
        if !csv.is_empty() {
            line.push(' ');
            line.push_str(&gis_word(&format!("csv={csv}"), "The rows' file")?);
        }
    }
    Ok(line)
}

/// One table the scope becomes: its name, how many rows it has, and its columns with their
/// types, the geometry column last.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GisSqlTable {
    pub name: String,
    /// "2 rows".
    pub rows: String,
    /// Each column's name and type.
    pub columns: Vec<(String, String)>,
}

/// GIS > Analysis - GDAL > Query with SQL... (see the module comment).
pub struct GisSqlDialog {
    frame: GisToolFrame,
    tables: Vec<GisSqlTable>,
    sql: String,
    dialect: Words,
    answer_as: Words,
    layer: String,
    csv: String,
    /// The grid's columns, by name, and its rows.
    headers: Vec<String>,
    results: Vec<Vec<String>>,
    result: Option<usize>,
}

impl GisSqlDialog {
    pub fn new(context: GisDialogContext, host: &dyn Host) -> Self {
        let mut frame = GisToolFrame::new("gisSql", "Query with SQL", context, host, true);
        frame.set_width(1150.0);
        Self {
            frame,
            tables: Vec::new(),
            sql: String::new(),
            dialect: Words::new(&["sqlite", "ogrsql"]),
            answer_as: Words::new(&["report", "select", "layer"]),
            layer: String::new(),
            csv: String::new(),
            headers: Vec::new(),
            results: Vec::new(),
            result: None,
        }
    }

    /// The window's Query with SQL dialog, as the menu makes it.
    pub fn make(context: GisDialogContext, host: &dyn Host) -> Box<dyn GisTool> {
        Box::new(Self::new(context, host))
    }

    pub fn form(&self) -> GisSqlForm {
        GisSqlForm {
            scope: self.frame.scope_words(),
            sql: self.sql.clone(),
            dialect: self.dialect.text_or("sqlite"),
            r#as: self.answer_as.text_or("report"),
            layer: self.layer.clone(),
            csv: self.csv.clone(),
        }
    }

    /// The tables listed, as gisSqlTables shows them.
    pub fn tables(&self) -> &[GisSqlTable] {
        &self.tables
    }

    /// The grid's column names.
    pub fn result_headers(&self) -> &[String] {
        &self.headers
    }

    /// The grid's rows.
    pub fn results(&self) -> &[Vec<String>] {
        &self.results
    }

    /// Lists the tables and their columns for the scope as it is now: what the drawing's
    /// properties would be as columns.
    pub fn refresh_tables(&mut self, host: &dyn Host) {
        self.tables.clear();
        if !self.frame.document_alive() {
            return;
        }
        let Some(controls) = self.frame.scope_controls() else {
            return;
        };
        // What the scope takes now, converted as the verb converts it: only to list the columns,
        // never to answer the query.
        let (Ok(scope), Ok(filter)) = (controls.scope(), controls.filter()) else {
            return;
        };
        let document = host.document();
        let Ok(ids) = match_entities(document, &scope, &filter) else {
            return;
        };
        let Ok(dataset) =
            drawing_dataset(document.model(), &ids, &DrawingDatasetOptions::default())
        else {
            return;
        };
        for table in &dataset.set.tables {
            let mut columns: Vec<(String, String)> = table
                .fields
                .iter()
                .map(|field| (field.name.clone(), String::from(type_name(field.r#type))))
                .collect();
            columns.push((String::from("geometry"), String::from("geometry")));
            self.tables.push(GisSqlTable {
                name: table.name.clone(),
                rows: format!("{} rows", table.features.len()),
                columns,
            });
        }
    }

    fn show_results(&mut self, reply: &str) {
        self.headers.clear();
        self.results.clear();
        self.result = None;
        // Each column's key and name.
        let mut columns: Vec<(String, String)> = Vec::new();
        let mut rows = Vec::new();
        for record in parse_records(reply) {
            if record.kind == "column" {
                let name = String::from(record.get("name").unwrap_or(""));
                let key = record.get("key").map_or_else(|| name.clone(), String::from);
                columns.push((key, name));
            } else if record.kind == "row" {
                rows.push(record);
            }
        }
        self.headers = columns.iter().map(|(_, name)| name.clone()).collect();
        for row in &rows {
            self.results.push(
                columns
                    .iter()
                    .map(|(key, _)| String::from(row.get(key).unwrap_or("")))
                    .collect(),
            );
        }
    }

    /// The tables as rows of the list: a table, then its columns under it.
    fn table_rows(&self) -> Vec<Vec<String>> {
        let mut rows = Vec::new();
        for table in &self.tables {
            rows.push(vec![table.name.clone(), table.rows.clone()]);
            for (name, kind) in &table.columns {
                rows.push(vec![format!("  {name}"), kind.clone()]);
            }
        }
        rows
    }
}

impl GisTool for GisSqlDialog {
    fn frame(&self) -> &GisToolFrame {
        &self.frame
    }

    fn frame_mut(&mut self) -> &mut GisToolFrame {
        &mut self.frame
    }

    fn command(&self) -> Result<String> {
        gis_sql_line(&self.form())
    }

    fn reload(&mut self, host: &dyn Host) {
        self.frame.reload(host);
        self.refresh_tables(host);
    }

    fn replied(&mut self, reply: &str) {
        self.show_results(reply);
    }

    fn summary(&self, reply: &str) -> String {
        if reply.contains(" target=selection ") {
            let said = gis_summary(reply);
            return if said.starts_with("Done.") {
                String::from("Selected: the entities the rows name are the selection.")
            } else {
                said
            };
        }
        if reply.contains(" target=report ") {
            let rows = reply
                .split('\n')
                .filter(|line| line.starts_with("row ") || *line == "row")
                .count();
            return format!("{rows} rows: in the grid, and as row records in the reply.");
        }
        gis_summary(reply)
    }

    fn fields_ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.label("Tables:");
        let listed = self.table_rows();
        egui::ScrollArea::vertical()
            .id_salt("gisSqlTables")
            .max_height(150.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                table(
                    ui,
                    "gisSqlTables",
                    &["Table / column", "Type"],
                    &listed,
                    None,
                    &[],
                    None,
                );
            });
        let refresh = button(ui, "gisSqlTablesRefresh", "List the tables for this scope").clicked();
        ui.label("Statement:");
        text_with(
            ui,
            "gisSqlText",
            &mut self.sql,
            TextOptions {
                read_only: false,
                rows: 4,
                monospace: true,
            },
        )
        .on_hover_text("One SELECT: a query reads the drawing and never changes it");
        self.dialect.ui(
            ui,
            "gisSqlDialect",
            "Dialect:",
            "sqlite: SQLite with Spatialite's ST_ functions; ogrsql: OGR's own SQL",
        );
        self.answer_as.ui(
            ui,
            "gisSqlAs",
            "Answer as:",
            "report: the rows; select: the entities their katana_id names; layer: their \
             geometry drawn, one undo step",
        );
        let answer = self.answer_as.text();
        ui.add_enabled_ui(answer == "layer", |ui| {
            gis_field(
                ui,
                "gisSqlLayer",
                "Layer (AS LAYER):",
                &mut self.layer,
                "the layer to draw on",
                "AS LAYER: where the rows' geometry is drawn",
            );
        });
        ui.add_enabled_ui(answer == "report", |ui| {
            gis_field(
                ui,
                "gisSqlCsv",
                "Rows to CSV:",
                &mut self.csv,
                "none",
                "csv=: AS REPORT - the rows written to this file too",
            );
        });
        ui.label("Rows:");
        let headers: Vec<&str> = self.headers.iter().map(String::as_str).collect();
        let results = &self.results;
        let current = self.result;
        let shown = egui::ScrollArea::both()
            .id_salt("gisSqlResults")
            .max_height(160.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                table(ui, "gisSqlResults", &headers, results, current, &[], None)
            })
            .inner;
        if shown.chosen.is_some() {
            self.result = shown.chosen;
        }
        if refresh {
            self.refresh_tables(&*host);
        }
    }
}
