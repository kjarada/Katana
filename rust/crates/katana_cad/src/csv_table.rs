//! The one CSV writer of the report verbs (VOLUME ... csv=, CORRIDOR
//! QUANTITIES csv=): RFC 4180 records ending in CRLF, their fields quoted by
//! csv_field (code_edit.rs); a file already there replaced only with
//! OVERWRITE, as every verb that writes a file does (docs/volumes.md); and a
//! reply record that says what was written (include/katana/cad/csv_table.hpp,
//! src/katana_cad/csv_table.cpp).

use std::fs::File;
use std::io::Write;

use katana_core::error::make_error;
use katana_core::path_text::path_from_utf8;
use katana_core::{ErrorCode, Result};

use crate::code_edit::csv_field;
use crate::scope_verbs::record_value;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CsvTable {
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

fn record(fields: &[String]) -> String {
    let mut line = String::new();
    for (i, field) in fields.iter().enumerate() {
        if i != 0 {
            line.push(',');
        }
        line.push_str(&csv_field(field));
    }
    line.push_str("\r\n");
    line
}

/// The table written to `path`: the header, then the rows, each ending in
/// CRLF. AlreadyExists when the file is there and `overwrite` is not given
/// (nothing is touched), FileExportFailure when it cannot be written. Replies
/// "csv file=<path> rows=<n>".
pub fn write_csv_table(table: &CsvTable, path: &str, overwrite: bool) -> Result<String> {
    let file = path_from_utf8(path);
    // std::filesystem::exists with an error code: a path that cannot be
    // asked about is taken not to be there, and the open then says why.
    if file.try_exists().unwrap_or(false) && !overwrite {
        return Err(make_error(
            ErrorCode::AlreadyExists,
            "the file is already there; OVERWRITE replaces it",
            path,
        ));
    }
    let Ok(mut out) = File::create(file) else {
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "the file cannot be written",
            path,
        ));
    };
    let mut text = record(&table.header);
    for row in &table.rows {
        text.push_str(&record(row));
    }
    if out
        .write_all(text.as_bytes())
        .and_then(|()| out.flush())
        .is_err()
    {
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "the file could not be finished",
            path,
        ));
    }
    Ok(format!(
        "csv file={} rows={}",
        record_value(path),
        table.rows.len()
    ))
}
