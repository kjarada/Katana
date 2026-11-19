//! Minimal RAII wrapper around SQLite (PLAN.MD Phase 07, Rule 4). The port of
//! include/katana/storage/sqlite_database.hpp and
//! src/katana_storage/sqlite_database.cpp.
//!
//! No SQLite type appears here, and no unsafe code: the C API is reached
//! through `ffi`'s owning types (Connection, Statement, Backup), the crate's
//! one unsafe module, and only this module uses them. Every call reports
//! failures as DatabaseFailure with SQLite's own message.
//!
//! Ownership is the C++'s, held by the borrow checker: a statement and a
//! transaction borrow the connection they came from, so neither can outlive
//! it, and a statement is finalized before its connection is closed. A
//! borrowed column view borrows its statement mutably, so the statement
//! cannot step, reset or convert the column while the view is alive - the
//! lifetime rule the C++ states on columnTextView, enforced.

use std::ffi::{CString, c_int};
use std::path::Path;

use katana_core::error::{Error, ErrorCode, Result, Status, make_error};
use katana_core::path_text::path_to_utf8;
use katana_core::text_encoding::utf8_or_windows1252;

use crate::ffi;

/// The bytes of `text` up to its first NUL, as a C string: what the C++'s
/// `std::string::c_str()` hands SQLite, which stops reading there.
fn c_string(text: &[u8]) -> CString {
    let end = text
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(text.len());
    CString::new(&text[..end]).unwrap_or_default()
}

fn database_failure(action: &str, context: String) -> Error {
    make_error(
        ErrorCode::DatabaseFailure,
        format!("SQLite failed to {action}"),
        context,
    )
}

/// The failure of `action`, with SQLite's message for the last failure on
/// `connection`.
fn sqlite_error(connection: &ffi::Connection, action: &str) -> Error {
    database_failure(action, utf8_or_windows1252(connection.error_message()))
}

// ---- SqliteStatement ---------------------------------------------------------------

/// A value a statement's parameter can be bound to: the C++ `bind` overload
/// set, chosen by the type as the C++ chooses by overload. `&str` is the
/// C++'s `std::string_view` (TEXT), `&[u8]` its `std::span<const std::byte>`
/// (BLOB), and `TextBytes` text that is bytes up to the decoder (a
/// `std::string_view` of bytes that need not be UTF-8).
pub trait BindValue {
    fn bind_to(self, statement: &mut SqliteStatement<'_>, index: i32) -> Status;
}

/// Text that is bytes: bound as TEXT, as SQLite is given them, whether or
/// not they are UTF-8 (SQLite does not check).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextBytes<'a>(pub &'a [u8]);

impl BindValue for i64 {
    fn bind_to(self, statement: &mut SqliteStatement<'_>, index: i32) -> Status {
        statement.bind_integer(index, self)
    }
}

impl BindValue for f64 {
    fn bind_to(self, statement: &mut SqliteStatement<'_>, index: i32) -> Status {
        statement.bind_real(index, self)
    }
}

impl BindValue for bool {
    fn bind_to(self, statement: &mut SqliteStatement<'_>, index: i32) -> Status {
        statement.bind_integer(index, i64::from(self))
    }
}

impl BindValue for &str {
    fn bind_to(self, statement: &mut SqliteStatement<'_>, index: i32) -> Status {
        statement.bind_text(index, self.as_bytes())
    }
}

impl BindValue for &String {
    fn bind_to(self, statement: &mut SqliteStatement<'_>, index: i32) -> Status {
        statement.bind_text(index, self.as_bytes())
    }
}

impl BindValue for TextBytes<'_> {
    fn bind_to(self, statement: &mut SqliteStatement<'_>, index: i32) -> Status {
        statement.bind_text(index, self.0)
    }
}

impl BindValue for &[u8] {
    fn bind_to(self, statement: &mut SqliteStatement<'_>, index: i32) -> Status {
        statement.bind_blob(index, self)
    }
}

impl BindValue for &Vec<u8> {
    fn bind_to(self, statement: &mut SqliteStatement<'_>, index: i32) -> Status {
        statement.bind_blob(index, self)
    }
}

/// A prepared statement. Parameter indices start at 1, column indices at 0.
#[derive(Debug)]
pub struct SqliteStatement<'db> {
    // Holds no handle when the SQL held no statement (sqlite3_prepare_v2
    // returns SQLITE_OK and no statement for an empty or comment-only text).
    statement: ffi::Statement<'db>,
}

impl SqliteStatement<'_> {
    fn check(&self, code: c_int, action: &str) -> Status {
        if code != ffi::SQLITE_OK {
            return Err(sqlite_error(self.statement.connection(), action));
        }
        Ok(())
    }

    /// The C++ `bind(index, value)` overloads: an integer, a real, text, a
    /// flag (as 0 or 1) or a BLOB, by the type of `value` (BindValue).
    pub fn bind<V: BindValue>(&mut self, index: i32, value: V) -> Status {
        value.bind_to(self, index)
    }

    fn bind_integer(&mut self, index: i32, value: i64) -> Status {
        let code = self.statement.bind_int64(index, value);
        self.check(code, "bind an integer")
    }

    fn bind_real(&mut self, index: i32, value: f64) -> Status {
        let code = self.statement.bind_double(index, value);
        self.check(code, "bind a real")
    }

    /// Empty text is bound as empty text, not as SQL NULL (ffi's bind_text).
    fn bind_text(&mut self, index: i32, value: &[u8]) -> Status {
        let code = self.statement.bind_text(index, value);
        self.check(code, "bind text")
    }

    fn bind_blob(&mut self, index: i32, value: &[u8]) -> Status {
        // SQLite copies the bytes, so the caller's buffer may die immediately
        // afterwards.
        //
        // A zero-length blob is bound through sqlite3_bind_zeroblob rather
        // than bind_blob64 with a null pointer, which SQLite would store as
        // NULL - and NULL is how this schema says "no blob here, read the JSON
        // column".
        if value.is_empty() {
            let code = self.statement.bind_zeroblob(index, 0);
            return self.check(code, "bind empty blob");
        }
        let code = self.statement.bind_blob(index, value);
        self.check(code, "bind blob")
    }

    pub fn bind_null(&mut self, index: i32) -> Status {
        let code = self.statement.bind_null(index);
        self.check(code, "bind null")
    }

    /// true: a row is available. false: the statement finished.
    pub fn step(&mut self) -> Result<bool> {
        let code = self.statement.step();
        if code == ffi::SQLITE_ROW {
            return Ok(true);
        }
        if code == ffi::SQLITE_DONE {
            return Ok(false);
        }
        Err(sqlite_error(
            self.statement.connection(),
            "execute a statement",
        ))
    }

    /// Runs a statement that returns no rows, then resets it for reuse.
    pub fn run(&mut self) -> Status {
        let row = self.step();
        self.reset();
        row.map(|_| ())
    }

    pub fn reset(&mut self) {
        self.statement.reset_and_clear_bindings();
    }

    pub fn column_is_null(&self, column: i32) -> bool {
        self.statement.column_type(column) == ffi::SQLITE_NULL
    }

    pub fn column_int64(&self, column: i32) -> i64 {
        self.statement.column_int64(column)
    }

    pub fn column_double(&self, column: i32) -> f64 {
        self.statement.column_double(column)
    }

    /// The column's text, copied: bytes, as SQLite holds them (the C++
    /// `std::string`); the caller decodes them.
    pub fn column_text(&mut self, column: i32) -> Vec<u8> {
        self.column_text_view(column).to_vec()
    }

    /// Empty for a NULL column or a zero-length blob; column_is_null separates
    /// the two, which matters because a missing geometry and an empty one are
    /// different failures.
    pub fn column_blob(&mut self, column: i32) -> Vec<u8> {
        self.column_blob_span(column).to_vec()
    }

    /// A BORROWED view of the column's text, for a value that is parsed and
    /// discarded within the row: it points straight at SQLite's own row buffer
    /// and copies nothing, which is what makes loading 50k entities not
    /// allocate per row.
    ///
    /// LIFETIME. SQLite owns the bytes; they are invalidated by the next
    /// step, reset or run, by finalizing, and by reading the SAME column
    /// through a different accessor (SQLite converts the value in place).
    /// The view borrows the statement mutably, so none of those can happen
    /// while it is alive.
    pub fn column_text_view(&mut self, column: i32) -> &[u8] {
        self.statement.column_text(column)
    }

    /// The same as column_text_view, for a BLOB column.
    pub fn column_blob_span(&mut self, column: i32) -> &[u8] {
        self.statement.column_blob(column)
    }
}

// ---- SqliteDatabase ----------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OpenMode {
    #[default]
    ReadWriteCreate,
    ReadWrite,
    ReadOnly,
}

/// A connection. Its methods take `&self`: the statements and the
/// transaction borrow it while the connection is used beside them, as the
/// C++'s do, and SQLite serialises the connection itself (FULLMUTEX). It is
/// Send, as ffi's Connection is, and closed when dropped, after every
/// statement that borrowed it.
#[derive(Debug)]
pub struct SqliteDatabase {
    connection: ffi::Connection,
}

impl SqliteDatabase {
    pub fn open(path: &Path, mode: OpenMode) -> Result<SqliteDatabase> {
        let mut flags = ffi::SQLITE_OPEN_FULLMUTEX;
        match mode {
            OpenMode::ReadWriteCreate => {
                flags |= ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE;
            }
            OpenMode::ReadWrite => flags |= ffi::SQLITE_OPEN_READWRITE,
            OpenMode::ReadOnly => flags |= ffi::SQLITE_OPEN_READONLY,
        }

        let utf8_path = path_to_utf8(path);
        let file_name = c_string(&utf8_path);
        let connection = match ffi::Connection::open(&file_name, flags) {
            Ok(connection) => connection,
            Err(message) => {
                // No handle at all only when SQLite could not allocate one.
                let mut context = match message {
                    Some(message) => utf8_or_windows1252(&message),
                    None => String::from("no connection"),
                };
                context.push_str(" path=");
                context.push_str(&utf8_or_windows1252(utf8_path.as_slice()));
                return Err(database_failure("open the database", context));
            }
        };
        connection.set_extended_result_codes(true);
        connection.set_busy_timeout(5000);
        let database = SqliteDatabase { connection };

        if mode != OpenMode::ReadOnly {
            // FULL: a committed save survives power loss. Rollback journal
            // rather than WAL so projects behave on network shares and stay a
            // single file at rest.
            database.execute(
                "PRAGMA foreign_keys = ON; PRAGMA synchronous = FULL; PRAGMA journal_mode = DELETE;",
            )?;
        }
        Ok(database)
    }

    pub fn open_in_memory() -> Result<SqliteDatabase> {
        Self::open(Path::new(":memory:"), OpenMode::ReadWriteCreate)
    }

    /// Runs one or more statements that return no rows.
    pub fn execute(&self, sql: &str) -> Status {
        let text = c_string(sql.as_bytes());
        self.connection.execute(&text).map_err(|message| {
            let detail = match message {
                Some(message) => utf8_or_windows1252(&message),
                None => String::from("unknown error"),
            };
            make_error(
                ErrorCode::DatabaseFailure,
                "SQLite failed to execute SQL",
                detail,
            )
        })
    }

    pub fn prepare(&self, sql: &str) -> Result<SqliteStatement<'_>> {
        match self.connection.prepare(sql.as_bytes()) {
            Ok(statement) => Ok(SqliteStatement { statement }),
            Err(_) => Err(sqlite_error(&self.connection, "prepare a statement")),
        }
    }

    /// BEGIN IMMEDIATE.
    pub fn begin(&self) -> Status {
        self.execute("BEGIN IMMEDIATE")
    }

    pub fn commit(&self) -> Status {
        self.execute("COMMIT")
    }

    pub fn rollback(&self) -> Status {
        self.execute("ROLLBACK")
    }

    fn query_integer(&self, sql: &str) -> Result<i64> {
        let mut statement = self.prepare(sql)?;
        if !statement.step()? {
            return Err(make_error(
                ErrorCode::DatabaseFailure,
                "query returned no row",
                sql,
            ));
        }
        Ok(statement.column_int64(0))
    }

    pub fn user_version(&self) -> Result<i32> {
        Ok(self.query_integer("PRAGMA user_version")? as i32)
    }

    pub fn set_user_version(&self, version: i32) -> Status {
        self.execute(&format!("PRAGMA user_version = {version}"))
    }

    pub fn application_id(&self) -> Result<i64> {
        self.query_integer("PRAGMA application_id")
    }

    pub fn set_application_id(&self, id: i32) -> Status {
        self.execute(&format!("PRAGMA application_id = {id}"))
    }

    pub fn table_exists(&self, name: &str) -> Result<bool> {
        let mut statement =
            self.prepare("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1")?;
        statement.bind(1, name)?;
        let row = statement.step()?;
        Ok(row && statement.column_int64(0) > 0)
    }

    /// The most bytes this connection holds in one string or BLOB - and in
    /// one row, which SQLite encodes as a single BLOB while it inserts or
    /// reads it (sqlite3_limit, SQLITE_LIMIT_LENGTH; 1,000,000,000 as SQLite
    /// is built by default). A value or row over it fails with "string or
    /// blob too big".
    pub fn length_limit(&self) -> i64 {
        // -1 reads the limit, it sets nothing.
        i64::from(self.connection.limit(ffi::SQLITE_LIMIT_LENGTH, -1))
    }

    /// None when the file is sound, otherwise SQLite's description of the
    /// damage (PRAGMA integrity_check).
    pub fn integrity_problems(&self) -> Result<Option<String>> {
        let mut statement = self.prepare("PRAGMA integrity_check")?;
        let mut problems = String::new();
        while statement.step()? {
            let line = utf8_or_windows1252(statement.column_text_view(0));
            if line != "ok" {
                if !problems.is_empty() {
                    problems.push_str("; ");
                }
                problems.push_str(&line);
            }
        }
        if problems.is_empty() {
            return Ok(None);
        }
        Ok(Some(problems))
    }

    /// Consistent snapshot through SQLite's online backup API; safe while the
    /// connection is in use.
    pub fn backup_to(&self, destination: &Path) -> Status {
        let target = Self::open(destination, OpenMode::ReadWriteCreate)?;
        let Some(mut backup) = ffi::Backup::start(&target.connection, &self.connection) else {
            return Err(sqlite_error(&target.connection, "start a backup"));
        };
        // -1 copies everything in one pass; the backup is finished whatever
        // the step answered, before either connection can close.
        let step_code = backup.step(-1);
        let finish_code = backup.finish();
        if step_code != ffi::SQLITE_DONE || finish_code != ffi::SQLITE_OK {
            return Err(sqlite_error(&target.connection, "complete a backup"));
        }
        Ok(())
    }
}

// ---- SqliteTransaction -------------------------------------------------------------

/// Rolls the transaction back unless commit() succeeded.
#[derive(Debug)]
pub struct SqliteTransaction<'db> {
    database: Option<&'db SqliteDatabase>,
}

impl<'db> SqliteTransaction<'db> {
    pub fn begin(database: &'db SqliteDatabase) -> Result<SqliteTransaction<'db>> {
        database.begin()?;
        Ok(SqliteTransaction {
            database: Some(database),
        })
    }

    pub fn commit(&mut self) -> Status {
        let Some(database) = self.database else {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "transaction is no longer active",
            ));
        };
        database.commit()?;
        self.database = None;
        Ok(())
    }
}

impl Drop for SqliteTransaction<'_> {
    fn drop(&mut self) {
        if let Some(database) = self.database {
            let _ = database.rollback();
        }
    }
}
