//! Minimal RAII wrapper around SQLite (PLAN.MD Phase 07, Rule 4). The port of
//! include/katana/storage/sqlite_database.hpp and
//! src/katana_storage/sqlite_database.cpp.
//!
//! No SQLite type appears here: the C API is reached through `ffi`, and only
//! this module calls it. Every call reports failures as DatabaseFailure with
//! SQLite's own message.
//!
//! Ownership is the C++'s, held by the borrow checker: a statement and a
//! transaction borrow the connection they came from, so neither can outlive
//! it, and a statement is finalized before its connection is closed. A
//! borrowed column view borrows its statement mutably, so the statement
//! cannot step, reset or convert the column while the view is alive - the
//! lifetime rule the C++ states on columnTextView, enforced.

#![allow(
    unsafe_code,
    reason = "Owns SQLite's handles and calls its C API; every unsafe block states why it is sound."
)]

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::marker::PhantomData;
use std::path::Path;
use std::ptr;

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

/// SQLite's message for the last failure on `connection`.
fn error_message(connection: *mut ffi::Sqlite3) -> String {
    // SAFETY: `connection` is a live handle this module opened (or the
    // handle sqlite3_open_v2 returned with its failure, which sqlite3_errmsg
    // accepts); the message it returns is a NUL-terminated string SQLite owns
    // until the next call on the connection, and it is copied at once.
    let message = unsafe { ffi::sqlite3_errmsg(connection) };
    if message.is_null() {
        return String::new();
    }
    // SAFETY: as above: non-null, NUL-terminated, alive until the next call.
    utf8_or_windows1252(unsafe { CStr::from_ptr(message) }.to_bytes())
}

fn sqlite_error(connection: *mut ffi::Sqlite3, action: &str) -> Error {
    let context = if connection.is_null() {
        String::from("no connection")
    } else {
        error_message(connection)
    };
    make_error(
        ErrorCode::DatabaseFailure,
        format!("SQLite failed to {action}"),
        context,
    )
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
    connection: *mut ffi::Sqlite3,
    // NULL when the SQL held no statement (sqlite3_prepare_v2 returns
    // SQLITE_OK and no statement for an empty or comment-only text).
    statement: *mut ffi::Sqlite3Stmt,
    _database: PhantomData<&'db SqliteDatabase>,
}

impl Drop for SqliteStatement<'_> {
    fn drop(&mut self) {
        // SAFETY: the statement was prepared on a connection that outlives
        // this value (the 'db borrow), and is finalized exactly once, here;
        // sqlite3_finalize(NULL) is a harmless no-op.
        unsafe {
            ffi::sqlite3_finalize(self.statement);
        }
    }
}

impl SqliteStatement<'_> {
    fn check(&self, code: c_int, action: &str) -> Status {
        if code != ffi::SQLITE_OK {
            return Err(sqlite_error(self.connection, action));
        }
        Ok(())
    }

    /// The C++ `bind(index, value)` overloads: an integer, a real, text, a
    /// flag (as 0 or 1) or a BLOB, by the type of `value` (BindValue).
    pub fn bind<V: BindValue>(&mut self, index: i32, value: V) -> Status {
        value.bind_to(self, index)
    }

    fn bind_integer(&mut self, index: i32, value: i64) -> Status {
        // SAFETY: the statement is live (or NULL, which SQLite refuses with
        // SQLITE_MISUSE); binding reads nothing but the value.
        let code = unsafe { ffi::sqlite3_bind_int64(self.statement, index, value) };
        self.check(code, "bind an integer")
    }

    fn bind_real(&mut self, index: i32, value: f64) -> Status {
        // SAFETY: as for bind_integer.
        let code = unsafe { ffi::sqlite3_bind_double(self.statement, index, value) };
        self.check(code, "bind a real")
    }

    fn bind_text(&mut self, index: i32, value: &[u8]) -> Status {
        // An empty slice's pointer dangles, and SQLite binds a null pointer as
        // SQL NULL rather than as empty text. A caller meaning "empty" would
        // silently violate a NOT NULL constraint, or worse, store NULL where
        // the schema allows it and have the difference surface much later.
        // Empty text is bound as empty text, from a real empty string.
        let data: *const c_char = if value.is_empty() {
            c"".as_ptr()
        } else {
            value.as_ptr().cast()
        };
        // SAFETY: `data` points at `value.len()` readable bytes (or at a
        // static empty string when there are none), and SQLITE_TRANSIENT has
        // SQLite copy them before this returns, so the caller's buffer may
        // die immediately afterwards.
        let code = unsafe {
            ffi::sqlite3_bind_text64(
                self.statement,
                index,
                data,
                value.len() as ffi::Sqlite3Uint64,
                ffi::SQLITE_TRANSIENT,
                ffi::SQLITE_UTF8,
            )
        };
        self.check(code, "bind text")
    }

    fn bind_blob(&mut self, index: i32, value: &[u8]) -> Status {
        // SQLITE_TRANSIENT for the same reason as text: SQLite copies, so the
        // caller's buffer may die immediately afterwards.
        //
        // A zero-length blob is bound through sqlite3_bind_zeroblob rather
        // than bind_blob64 with a null pointer, which SQLite would store as
        // NULL - and NULL is how this schema says "no blob here, read the JSON
        // column".
        if value.is_empty() {
            // SAFETY: as for bind_integer.
            let code = unsafe { ffi::sqlite3_bind_zeroblob(self.statement, index, 0) };
            return self.check(code, "bind empty blob");
        }
        // SAFETY: `value` is `value.len()` readable bytes, copied by SQLite
        // before this returns (SQLITE_TRANSIENT).
        let code = unsafe {
            ffi::sqlite3_bind_blob64(
                self.statement,
                index,
                value.as_ptr().cast::<c_void>(),
                value.len() as ffi::Sqlite3Uint64,
                ffi::SQLITE_TRANSIENT,
            )
        };
        self.check(code, "bind blob")
    }

    pub fn bind_null(&mut self, index: i32) -> Status {
        // SAFETY: as for bind_integer.
        let code = unsafe { ffi::sqlite3_bind_null(self.statement, index) };
        self.check(code, "bind null")
    }

    /// true: a row is available. false: the statement finished.
    pub fn step(&mut self) -> Result<bool> {
        // SAFETY: the statement is live or NULL; SQLite refuses NULL with
        // SQLITE_MISUSE, which is reported below like any failure.
        let code = unsafe { ffi::sqlite3_step(self.statement) };
        if code == ffi::SQLITE_ROW {
            return Ok(true);
        }
        if code == ffi::SQLITE_DONE {
            return Ok(false);
        }
        Err(sqlite_error(self.connection, "execute a statement"))
    }

    /// Runs a statement that returns no rows, then resets it for reuse.
    pub fn run(&mut self) -> Status {
        let row = self.step();
        self.reset();
        row.map(|_| ())
    }

    pub fn reset(&mut self) {
        // sqlite3_clear_bindings dereferences its argument, so the NULL
        // statement of an empty text is not handed to it (the C++ would crash
        // there; sqlite3_reset(NULL) is a no-op).
        if self.statement.is_null() {
            return;
        }
        // SAFETY: the statement is live; both calls only change its state.
        unsafe {
            ffi::sqlite3_reset(self.statement);
            ffi::sqlite3_clear_bindings(self.statement);
        }
    }

    pub fn column_is_null(&self, column: i32) -> bool {
        // SAFETY: the statement is live or NULL, and SQLite answers NULL or a
        // column out of range with SQLITE_NULL; reading the type converts
        // nothing.
        unsafe { ffi::sqlite3_column_type(self.statement, column) == ffi::SQLITE_NULL }
    }

    pub fn column_int64(&self, column: i32) -> i64 {
        // SAFETY: as for column_is_null; reading a value as an integer
        // computes from it and leaves its text or blob in place.
        unsafe { ffi::sqlite3_column_int64(self.statement, column) }
    }

    pub fn column_double(&self, column: i32) -> f64 {
        // SAFETY: as for column_int64.
        unsafe { ffi::sqlite3_column_double(self.statement, column) }
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
        // sqlite3_column_bytes must follow sqlite3_column_text: it reports the
        // size of the value AS CONVERTED by the preceding call, and a blob or
        // an integer read as text has a different length before and after.
        //
        // SAFETY: the statement is live or NULL (SQLite answers NULL with a
        // null pointer); the two calls are made in the order SQLite documents.
        let (text, size) = unsafe {
            let text = ffi::sqlite3_column_text(self.statement, column);
            let size = ffi::sqlite3_column_bytes(self.statement, column);
            (text, size)
        };
        if text.is_null() || size <= 0 {
            return &[];
        }
        // SAFETY: SQLite holds `size` bytes at `text` until the statement is
        // stepped, reset, finalized or the column converted again - each of
        // which needs `&mut self`, which the returned borrow holds.
        unsafe { std::slice::from_raw_parts(text, size as usize) }
    }

    /// The same as column_text_view, for a BLOB column.
    pub fn column_blob_span(&mut self, column: i32) -> &[u8] {
        // SAFETY: as for column_text_view.
        let (bytes, size) = unsafe {
            let bytes = ffi::sqlite3_column_blob(self.statement, column);
            let size = ffi::sqlite3_column_bytes(self.statement, column);
            (bytes, size)
        };
        if bytes.is_null() || size <= 0 {
            return &[];
        }
        // SAFETY: as for column_text_view.
        unsafe { std::slice::from_raw_parts(bytes.cast::<u8>(), size as usize) }
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
/// C++'s do, and SQLite serialises the connection itself (FULLMUTEX).
#[derive(Debug)]
pub struct SqliteDatabase {
    // Never NULL once open() returns: sqlite3_open_v2 writes NULL only when
    // it cannot allocate the handle, and open() fails then.
    connection: *mut ffi::Sqlite3,
}

// SAFETY: every connection is opened with SQLITE_OPEN_FULLMUTEX ("serialized"
// mode) on a library built thread-safe (THREADSAFE=1), in which SQLite allows
// a connection to be used from any thread; moving the owner to another thread
// is therefore sound. Statements borrow the connection and are not Send.
unsafe impl Send for SqliteDatabase {}

impl Drop for SqliteDatabase {
    fn drop(&mut self) {
        // SAFETY: the handle is this value's own; every statement borrowed it
        // and has been finalized (the borrow checker ends them first), so it
        // is closed exactly once, here. sqlite3_close(NULL) is a no-op.
        unsafe {
            ffi::sqlite3_close(self.connection);
        }
    }
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
        let mut connection: *mut ffi::Sqlite3 = ptr::null_mut();
        // SAFETY: `file_name` is a NUL-terminated string that outlives the
        // call, `connection` a place for the handle, the VFS the default.
        let code = unsafe {
            ffi::sqlite3_open_v2(file_name.as_ptr(), &mut connection, flags, ptr::null())
        };
        // Owned from here, success or not: SQLite returns a handle with most
        // failures, and it must be closed either way.
        let database = SqliteDatabase { connection };
        if code != ffi::SQLITE_OK {
            let mut error = sqlite_error(database.connection, "open the database");
            error.context.push_str(" path=");
            error
                .context
                .push_str(&utf8_or_windows1252(utf8_path.as_slice()));
            return Err(error);
        }
        // SAFETY: the handle is open.
        unsafe {
            ffi::sqlite3_extended_result_codes(database.connection, 1);
            ffi::sqlite3_busy_timeout(database.connection, 5000);
        }

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
        let mut message: *mut c_char = ptr::null_mut();
        // SAFETY: the connection is open, `text` NUL-terminated and alive for
        // the call, no callback is given, and `message` receives a string
        // SQLite allocates (or NULL), freed below with sqlite3_free.
        let code = unsafe {
            ffi::sqlite3_exec(
                self.connection,
                text.as_ptr(),
                None,
                ptr::null_mut(),
                &mut message,
            )
        };
        if code != ffi::SQLITE_OK {
            let detail = if message.is_null() {
                String::from("unknown error")
            } else {
                // SAFETY: a non-null message is SQLite's NUL-terminated string.
                utf8_or_windows1252(unsafe { CStr::from_ptr(message) }.to_bytes())
            };
            // SAFETY: the message was allocated by SQLite (or is NULL, which
            // sqlite3_free accepts) and is freed once.
            unsafe {
                ffi::sqlite3_free(message.cast::<c_void>());
            }
            return Err(make_error(
                ErrorCode::DatabaseFailure,
                "SQLite failed to execute SQL",
                detail,
            ));
        }
        Ok(())
    }

    pub fn prepare(&self, sql: &str) -> Result<SqliteStatement<'_>> {
        // A real empty string for an empty text, as a std::string_view of a
        // literal is, rather than an empty slice's dangling pointer.
        let text: *const c_char = if sql.is_empty() {
            c"".as_ptr()
        } else {
            sql.as_ptr().cast()
        };
        let mut statement = SqliteStatement {
            connection: self.connection,
            statement: ptr::null_mut(),
            _database: PhantomData,
        };
        // SAFETY: the connection is open; `text` is `sql.len()` readable
        // bytes (SQLite reads no further when given the length); the handle
        // written is owned by `statement`, which finalizes it.
        let code = unsafe {
            ffi::sqlite3_prepare_v2(
                self.connection,
                text,
                sql.len() as c_int,
                &mut statement.statement,
                ptr::null_mut(),
            )
        };
        if code != ffi::SQLITE_OK {
            return Err(sqlite_error(self.connection, "prepare a statement"));
        }
        Ok(statement)
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
        // SAFETY: the connection is open; -1 reads the limit, it sets nothing.
        i64::from(unsafe { ffi::sqlite3_limit(self.connection, ffi::SQLITE_LIMIT_LENGTH, -1) })
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
        // SAFETY: both connections are open; the names are NUL-terminated
        // literals; the backup handle is finished below before either
        // connection can close.
        let backup = unsafe {
            ffi::sqlite3_backup_init(
                target.connection,
                c"main".as_ptr(),
                self.connection,
                c"main".as_ptr(),
            )
        };
        if backup.is_null() {
            return Err(sqlite_error(target.connection, "start a backup"));
        }
        // SAFETY: `backup` is live until sqlite3_backup_finish, which is
        // called exactly once; -1 copies everything in one pass.
        let (step_code, finish_code) = unsafe {
            let step_code = ffi::sqlite3_backup_step(backup, -1);
            let finish_code = ffi::sqlite3_backup_finish(backup);
            (step_code, finish_code)
        };
        if step_code != ffi::SQLITE_DONE || finish_code != ffi::SQLITE_OK {
            return Err(sqlite_error(target.connection, "complete a backup"));
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
