//! The part of SQLite's C API (sqlite3.h) that the storage layer calls:
//! exactly those 28 functions, three handle types and eleven constants,
//! written by hand from the header of SQLite 3.53.4, the toolchain's, and
//! the safe types that own the handles.
//!
//! No binding crate: as for PROJ in katana_geodesy (docs/rust.md, "Third-party
//! libraries"), the declarations are of exactly what the C++ wrapper
//! (src/katana_storage/sqlite_database.cpp) uses, so its every pragma,
//! statement and error path is the C++'s and nothing else of SQLite is
//! reachable.
//!
//! This is the crate's one unsafe module (docs/rust.md, "Unsafe confined to
//! FFI modules"). A handle lives only inside the type that owns it -
//! `Connection`, `Statement`, `Backup` - which frees it once, on drop, and
//! whose lifetimes make a statement or a backup end before its connection
//! closes. Everything above (`sqlite_database`) is safe code that chooses
//! what to call and reports the codes; the soundness argument is all here.

#![allow(
    unsafe_code,
    reason = "Declares SQLite's C API and owns its handles; every unsafe block states why it is sound."
)]

use std::ffi::{CStr, c_char, c_int, c_uchar, c_void};
use std::marker::{PhantomData, PhantomPinned};
use std::ptr::{self, NonNull};

/// `sqlite3`, a database connection.
#[repr(C)]
struct Sqlite3 {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// `sqlite3_stmt`, a prepared statement.
#[repr(C)]
struct Sqlite3Stmt {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// `sqlite3_backup`, an online backup in progress.
#[repr(C)]
struct Sqlite3Backup {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// `sqlite3_int64`: `long long int` (or `__int64`), 64 bits on every platform.
type Sqlite3Int64 = i64;
/// `sqlite3_uint64`.
type Sqlite3Uint64 = u64;

/// `int (*callback)(void*, int, char**, char**)`, sqlite3_exec's row callback;
/// the C++ always passes NULL.
type Sqlite3Callback =
    Option<unsafe extern "C" fn(*mut c_void, c_int, *mut *mut c_char, *mut *mut c_char) -> c_int>;

/// The `void(*)(void*)` destructor argument of the bind functions
/// (`sqlite3_destructor_type`). Katana passes only SQLITE_TRANSIENT, which is
/// not a function but the value `((sqlite3_destructor_type)-1)` that SQLite
/// compares against, so the parameter is declared as the pointer-sized
/// integer that value is: Rust cannot make a function pointer of -1 in a
/// constant, and a function pointer and a pointer-sized integer are passed
/// alike on every ABI Katana is built for.
type Sqlite3DestructorType = isize;

const _: () = assert!(
    std::mem::size_of::<Sqlite3DestructorType>()
        == std::mem::size_of::<unsafe extern "C" fn(*mut c_void)>()
);
const _: () = assert!(std::mem::size_of::<Sqlite3Int64>() == 8);
const _: () = assert!(std::mem::size_of::<Sqlite3Uint64>() == 8);
const _: () = assert!(std::mem::size_of::<Sqlite3Callback>() == std::mem::size_of::<*mut c_void>());

/// Result codes.
pub(crate) const SQLITE_OK: c_int = 0;
pub(crate) const SQLITE_ROW: c_int = 100;
pub(crate) const SQLITE_DONE: c_int = 101;

/// A column's fundamental datatype (sqlite3_column_type).
pub(crate) const SQLITE_NULL: c_int = 5;

/// sqlite3_open_v2 flags.
pub(crate) const SQLITE_OPEN_READONLY: c_int = 0x0000_0001;
pub(crate) const SQLITE_OPEN_READWRITE: c_int = 0x0000_0002;
pub(crate) const SQLITE_OPEN_CREATE: c_int = 0x0000_0004;
pub(crate) const SQLITE_OPEN_FULLMUTEX: c_int = 0x0001_0000;

/// `((sqlite3_destructor_type)-1)`: SQLite copies the bytes before the bind
/// returns.
const SQLITE_TRANSIENT: Sqlite3DestructorType = -1;

/// The text encoding argument of sqlite3_bind_text64.
const SQLITE_UTF8: c_uchar = 1;

/// sqlite3_limit's category for the largest string or BLOB.
pub(crate) const SQLITE_LIMIT_LENGTH: c_int = 0;

#[link(name = "sqlite3")]
unsafe extern "C" {
    // ---- connections -----------------------------------------------------------
    fn sqlite3_open_v2(
        filename: *const c_char,
        pp_db: *mut *mut Sqlite3,
        flags: c_int,
        z_vfs: *const c_char,
    ) -> c_int;
    fn sqlite3_close(db: *mut Sqlite3) -> c_int;
    fn sqlite3_errmsg(db: *mut Sqlite3) -> *const c_char;
    fn sqlite3_extended_result_codes(db: *mut Sqlite3, onoff: c_int) -> c_int;
    fn sqlite3_busy_timeout(db: *mut Sqlite3, ms: c_int) -> c_int;
    fn sqlite3_exec(
        db: *mut Sqlite3,
        sql: *const c_char,
        callback: Sqlite3Callback,
        argument: *mut c_void,
        errmsg: *mut *mut c_char,
    ) -> c_int;
    fn sqlite3_free(pointer: *mut c_void);
    fn sqlite3_limit(db: *mut Sqlite3, id: c_int, new_val: c_int) -> c_int;

    // ---- statements ------------------------------------------------------------
    fn sqlite3_prepare_v2(
        db: *mut Sqlite3,
        z_sql: *const c_char,
        n_byte: c_int,
        pp_stmt: *mut *mut Sqlite3Stmt,
        pz_tail: *mut *const c_char,
    ) -> c_int;
    fn sqlite3_finalize(stmt: *mut Sqlite3Stmt) -> c_int;
    fn sqlite3_bind_int64(stmt: *mut Sqlite3Stmt, index: c_int, value: Sqlite3Int64) -> c_int;
    fn sqlite3_bind_double(stmt: *mut Sqlite3Stmt, index: c_int, value: f64) -> c_int;
    fn sqlite3_bind_text64(
        stmt: *mut Sqlite3Stmt,
        index: c_int,
        text: *const c_char,
        bytes: Sqlite3Uint64,
        destructor: Sqlite3DestructorType,
        encoding: c_uchar,
    ) -> c_int;
    fn sqlite3_bind_zeroblob(stmt: *mut Sqlite3Stmt, index: c_int, n: c_int) -> c_int;
    fn sqlite3_bind_blob64(
        stmt: *mut Sqlite3Stmt,
        index: c_int,
        value: *const c_void,
        bytes: Sqlite3Uint64,
        destructor: Sqlite3DestructorType,
    ) -> c_int;
    fn sqlite3_bind_null(stmt: *mut Sqlite3Stmt, index: c_int) -> c_int;
    fn sqlite3_step(stmt: *mut Sqlite3Stmt) -> c_int;
    fn sqlite3_reset(stmt: *mut Sqlite3Stmt) -> c_int;
    fn sqlite3_clear_bindings(stmt: *mut Sqlite3Stmt) -> c_int;
    fn sqlite3_column_type(stmt: *mut Sqlite3Stmt, column: c_int) -> c_int;
    fn sqlite3_column_int64(stmt: *mut Sqlite3Stmt, column: c_int) -> Sqlite3Int64;
    fn sqlite3_column_double(stmt: *mut Sqlite3Stmt, column: c_int) -> f64;
    fn sqlite3_column_text(stmt: *mut Sqlite3Stmt, column: c_int) -> *const c_uchar;
    fn sqlite3_column_bytes(stmt: *mut Sqlite3Stmt, column: c_int) -> c_int;
    fn sqlite3_column_blob(stmt: *mut Sqlite3Stmt, column: c_int) -> *const c_void;

    // ---- the online backup API -------------------------------------------------
    fn sqlite3_backup_init(
        destination: *mut Sqlite3,
        destination_name: *const c_char,
        source: *mut Sqlite3,
        source_name: *const c_char,
    ) -> *mut Sqlite3Backup;
    fn sqlite3_backup_step(backup: *mut Sqlite3Backup, pages: c_int) -> c_int;
    fn sqlite3_backup_finish(backup: *mut Sqlite3Backup) -> c_int;
}

/// Copies a NUL-terminated string SQLite owns into bytes Rust owns; empty for
/// a null pointer.
///
/// # Safety
/// `text` is null or a NUL-terminated string that stays valid for the call.
unsafe fn copied(text: *const c_char) -> Vec<u8> {
    if text.is_null() {
        return Vec::new();
    }
    // SAFETY: non-null and NUL-terminated by the caller's contract; the bytes
    // are copied before the function returns.
    unsafe { CStr::from_ptr(text) }.to_bytes().to_vec()
}

// ---- Connection ------------------------------------------------------------------

/// An open connection, closed once, when dropped.
#[derive(Debug)]
pub(crate) struct Connection {
    // Never NULL: open() hands out a Connection only for a handle SQLite
    // returned, and never one it failed to allocate.
    raw: NonNull<Sqlite3>,
}

// SAFETY: every connection is opened with SQLITE_OPEN_FULLMUTEX ("serialized"
// mode) on a library built thread-safe (THREADSAFE=1), in which SQLite allows
// a connection to be used from any thread; moving the owner to another thread
// is therefore sound. Statements and backups borrow the connection and are not
// Send. Not Sync: the error message is the connection's last one, and two
// threads sharing it would read each other's.
unsafe impl Send for Connection {}

impl Drop for Connection {
    fn drop(&mut self) {
        // SAFETY: the handle is this value's own and is closed exactly once,
        // here. Every statement and backup borrowed the connection, so the
        // borrow checker has ended (finalized, finished) them all first and
        // sqlite3_close does not refuse with SQLITE_BUSY and leak it.
        unsafe {
            sqlite3_close(self.raw.as_ptr());
        }
    }
}

impl Connection {
    /// sqlite3_open_v2 with the default VFS. A failure is SQLite's message,
    /// read from the handle it returned with the failure before that handle
    /// is closed, or None when it could not allocate one.
    pub(crate) fn open(file_name: &CStr, flags: c_int) -> Result<Connection, Option<Vec<u8>>> {
        let mut raw: *mut Sqlite3 = ptr::null_mut();
        // SAFETY: `file_name` is NUL-terminated and outlives the call, `raw` a
        // place for the handle, the VFS the default (NULL).
        let code = unsafe { sqlite3_open_v2(file_name.as_ptr(), &mut raw, flags, ptr::null()) };
        // Owned from here, success or not: SQLite returns a handle with most
        // failures, and it must be closed either way (by the drop below).
        let Some(raw) = NonNull::new(raw) else {
            return Err(None);
        };
        let connection = Connection { raw };
        if code != SQLITE_OK {
            return Err(Some(connection.error_message()));
        }
        Ok(connection)
    }

    /// SQLite's message for the last failure on this connection, copied:
    /// the string sqlite3_errmsg returns lives only until the next call.
    pub(crate) fn error_message(&self) -> Vec<u8> {
        // SAFETY: the handle is open (or is the handle sqlite3_open_v2
        // returned with its failure, which sqlite3_errmsg accepts); the
        // message is NUL-terminated and alive until the next call on the
        // connection, and `copied` copies it at once.
        unsafe { copied(sqlite3_errmsg(self.raw.as_ptr())) }
    }

    pub(crate) fn set_extended_result_codes(&self, on: bool) {
        // SAFETY: the handle is open; the call only sets a flag on it.
        unsafe {
            sqlite3_extended_result_codes(self.raw.as_ptr(), c_int::from(on));
        }
    }

    pub(crate) fn set_busy_timeout(&self, milliseconds: c_int) {
        // SAFETY: the handle is open; the call only sets its busy handler.
        unsafe {
            sqlite3_busy_timeout(self.raw.as_ptr(), milliseconds);
        }
    }

    /// sqlite3_exec with no callback. A failure is SQLite's message, or None
    /// when it gave none; the message SQLite allocated is freed here, once.
    pub(crate) fn execute(&self, sql: &CStr) -> Result<(), Option<Vec<u8>>> {
        let mut message: *mut c_char = ptr::null_mut();
        // SAFETY: the handle is open, `sql` NUL-terminated and alive for the
        // call, no callback is given, and `message` receives a string SQLite
        // allocates (or NULL), freed below with sqlite3_free.
        let code = unsafe {
            sqlite3_exec(
                self.raw.as_ptr(),
                sql.as_ptr(),
                None,
                ptr::null_mut(),
                &mut message,
            )
        };
        let detail = if message.is_null() {
            None
        } else {
            // SAFETY: a non-null message is SQLite's NUL-terminated string,
            // alive until the sqlite3_free below.
            Some(unsafe { copied(message) })
        };
        // SAFETY: the message was allocated by SQLite (or is NULL, which
        // sqlite3_free accepts) and is freed once, after it was copied.
        unsafe {
            sqlite3_free(message.cast::<c_void>());
        }
        if code != SQLITE_OK {
            return Err(detail);
        }
        Ok(())
    }

    /// sqlite3_limit: `new_value` -1 reads the limit and sets nothing.
    pub(crate) fn limit(&self, category: c_int, new_value: c_int) -> c_int {
        // SAFETY: the handle is open; the call reads (or sets) one limit.
        unsafe { sqlite3_limit(self.raw.as_ptr(), category, new_value) }
    }

    /// sqlite3_prepare_v2 of the whole of `sql`. Ok with a statement that
    /// holds no handle when the text held no statement (an empty or
    /// comment-only text, which SQLite prepares to NULL with SQLITE_OK);
    /// the failure is SQLite's result code, its message on the connection.
    pub(crate) fn prepare(&self, sql: &[u8]) -> Result<Statement<'_>, c_int> {
        // A real empty string for an empty text, as a std::string_view of a
        // literal is, rather than an empty slice's dangling pointer.
        let text: *const c_char = if sql.is_empty() {
            c"".as_ptr()
        } else {
            sql.as_ptr().cast()
        };
        // A length past c_int would wrap negative, which SQLite takes as "up
        // to the NUL", reading past a text that has none; capped, SQLite reads
        // inside the text and refuses it as too long (its length limit is
        // below c_int's), with its own message.
        let length = c_int::try_from(sql.len()).unwrap_or(c_int::MAX);
        let mut statement = Statement {
            raw: ptr::null_mut(),
            connection: self,
        };
        // SAFETY: the handle is open; `text` is at least `length` readable
        // bytes and SQLite reads no further when given the length; the handle
        // written is owned by `statement`, which finalizes it (also when the
        // call fails after writing one: SQLite writes NULL then).
        let code = unsafe {
            sqlite3_prepare_v2(
                self.raw.as_ptr(),
                text,
                length,
                &mut statement.raw,
                ptr::null_mut(),
            )
        };
        if code != SQLITE_OK {
            return Err(code);
        }
        Ok(statement)
    }

    /// The test's way to see the close's result code, which drop discards:
    /// SQLITE_BUSY there would mean a statement or backup outlived its turn.
    #[cfg(test)]
    fn close(self) -> c_int {
        let raw = self.raw;
        std::mem::forget(self);
        // SAFETY: the handle is this value's, which was forgotten above, so
        // its drop does not close it a second time.
        unsafe { sqlite3_close(raw.as_ptr()) }
    }
}

// ---- Statement -------------------------------------------------------------------

/// A prepared statement, finalized once, when dropped; it borrows its
/// connection, so it cannot outlive it.
///
/// The handle is NULL when the SQL held no statement. Every call below is
/// one SQLite accepts with NULL - the binds and the step refuse it with
/// SQLITE_MISUSE, the column reads answer as for a NULL column, the reset
/// and the finalize do nothing - but for sqlite3_clear_bindings, which
/// dereferences its argument and is not handed NULL.
#[derive(Debug)]
pub(crate) struct Statement<'c> {
    raw: *mut Sqlite3Stmt,
    connection: &'c Connection,
}

impl Drop for Statement<'_> {
    fn drop(&mut self) {
        // SAFETY: the statement was prepared on a connection that outlives
        // this value (the 'c borrow), and is finalized exactly once, here;
        // sqlite3_finalize(NULL) is a harmless no-op.
        unsafe {
            sqlite3_finalize(self.raw);
        }
    }
}

impl<'c> Statement<'c> {
    /// The connection the statement was prepared on, whose message describes
    /// the statement's last failure.
    pub(crate) fn connection(&self) -> &'c Connection {
        self.connection
    }

    pub(crate) fn bind_int64(&mut self, index: c_int, value: i64) -> c_int {
        // SAFETY: the statement is live or NULL (refused with SQLITE_MISUSE);
        // binding reads nothing but the value.
        unsafe { sqlite3_bind_int64(self.raw, index, value) }
    }

    pub(crate) fn bind_double(&mut self, index: c_int, value: f64) -> c_int {
        // SAFETY: as for bind_int64.
        unsafe { sqlite3_bind_double(self.raw, index, value) }
    }

    /// Binds `value` as TEXT, copied (SQLITE_TRANSIENT).
    pub(crate) fn bind_text(&mut self, index: c_int, value: &[u8]) -> c_int {
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
        // die immediately afterwards. The statement is live or NULL.
        unsafe {
            sqlite3_bind_text64(
                self.raw,
                index,
                data,
                value.len() as Sqlite3Uint64,
                SQLITE_TRANSIENT,
                SQLITE_UTF8,
            )
        }
    }

    /// Binds a zero-filled BLOB of `bytes` bytes.
    pub(crate) fn bind_zeroblob(&mut self, index: c_int, bytes: c_int) -> c_int {
        // SAFETY: as for bind_int64.
        unsafe { sqlite3_bind_zeroblob(self.raw, index, bytes) }
    }

    /// Binds `value` as a BLOB, copied (SQLITE_TRANSIENT).
    pub(crate) fn bind_blob(&mut self, index: c_int, value: &[u8]) -> c_int {
        // A real (static) address for an empty value, never an empty slice's
        // dangling one; the caller binds an empty BLOB through bind_zeroblob.
        let data: *const c_void = if value.is_empty() {
            c"".as_ptr().cast()
        } else {
            value.as_ptr().cast()
        };
        // SAFETY: `data` points at `value.len()` readable bytes (or at a
        // static empty string when there are none), copied by SQLite before
        // this returns (SQLITE_TRANSIENT). The statement is live or NULL.
        unsafe {
            sqlite3_bind_blob64(
                self.raw,
                index,
                data,
                value.len() as Sqlite3Uint64,
                SQLITE_TRANSIENT,
            )
        }
    }

    pub(crate) fn bind_null(&mut self, index: c_int) -> c_int {
        // SAFETY: as for bind_int64.
        unsafe { sqlite3_bind_null(self.raw, index) }
    }

    pub(crate) fn step(&mut self) -> c_int {
        // SAFETY: the statement is live or NULL; SQLite refuses NULL with
        // SQLITE_MISUSE, which the caller reports like any failure.
        unsafe { sqlite3_step(self.raw) }
    }

    /// sqlite3_reset, then sqlite3_clear_bindings; nothing for the NULL
    /// statement of an empty text (the C++ would crash in clear_bindings
    /// there; sqlite3_reset(NULL) is a no-op).
    pub(crate) fn reset_and_clear_bindings(&mut self) {
        if self.raw.is_null() {
            return;
        }
        // SAFETY: the statement is live; both calls only change its state.
        unsafe {
            sqlite3_reset(self.raw);
            sqlite3_clear_bindings(self.raw);
        }
    }

    pub(crate) fn column_type(&self, column: c_int) -> c_int {
        // SAFETY: the statement is live or NULL, and SQLite answers NULL or a
        // column out of range with SQLITE_NULL; reading the type converts
        // nothing.
        unsafe { sqlite3_column_type(self.raw, column) }
    }

    pub(crate) fn column_int64(&self, column: c_int) -> i64 {
        // SAFETY: as for column_type; reading a value as an integer computes
        // from it and leaves its text or blob in place, so a borrowed view
        // (which holds `&mut self` anyway) is not invalidated.
        unsafe { sqlite3_column_int64(self.raw, column) }
    }

    pub(crate) fn column_double(&self, column: c_int) -> f64 {
        // SAFETY: as for column_int64.
        unsafe { sqlite3_column_double(self.raw, column) }
    }

    /// The column's value as text, BORROWED from SQLite's row buffer; empty
    /// for NULL.
    ///
    /// LIFETIME. SQLite owns the bytes; they are invalidated by the next
    /// step, reset or run, by finalizing, and by reading the SAME column
    /// through a different accessor (SQLite converts the value in place).
    /// The slice borrows the statement mutably, so none of those can happen
    /// while it is alive.
    pub(crate) fn column_text(&mut self, column: c_int) -> &[u8] {
        // sqlite3_column_bytes must follow sqlite3_column_text: it reports the
        // size of the value AS CONVERTED by the preceding call, and a blob or
        // an integer read as text has a different length before and after.
        //
        // SAFETY: the statement is live or NULL (SQLite answers NULL with a
        // null pointer); the two calls are made in the order SQLite documents.
        let (text, size) = unsafe {
            let text = sqlite3_column_text(self.raw, column);
            let size = sqlite3_column_bytes(self.raw, column);
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

    /// The same as column_text, for a BLOB column.
    pub(crate) fn column_blob(&mut self, column: c_int) -> &[u8] {
        // SAFETY: as for column_text.
        let (bytes, size) = unsafe {
            let bytes = sqlite3_column_blob(self.raw, column);
            let size = sqlite3_column_bytes(self.raw, column);
            (bytes, size)
        };
        if bytes.is_null() || size <= 0 {
            return &[];
        }
        // SAFETY: as for column_text.
        unsafe { std::slice::from_raw_parts(bytes.cast::<u8>(), size as usize) }
    }
}

// ---- Backup ----------------------------------------------------------------------

/// An online backup of one connection's "main" into another's, finished once:
/// by `finish`, or on drop. It borrows both connections, so neither can close
/// while it is in progress.
#[derive(Debug)]
pub(crate) struct Backup<'c> {
    // NULL only after finish() took it.
    raw: *mut Sqlite3Backup,
    _connections: PhantomData<&'c Connection>,
}

impl Drop for Backup<'_> {
    fn drop(&mut self) {
        if self.raw.is_null() {
            return;
        }
        // SAFETY: the backup is live (finish() has not taken it) and both of
        // its connections outlive this value (the 'c borrow); it is finished
        // exactly once, here.
        unsafe {
            sqlite3_backup_finish(self.raw);
        }
    }
}

impl<'c> Backup<'c> {
    /// sqlite3_backup_init of `source`'s "main" into `destination`'s
    /// "main"; None when SQLite refuses, its message on `destination`.
    pub(crate) fn start(destination: &'c Connection, source: &'c Connection) -> Option<Backup<'c>> {
        // SAFETY: both connections are open and outlive the backup (the 'c
        // borrows); the names are NUL-terminated literals.
        let raw = unsafe {
            sqlite3_backup_init(
                destination.raw.as_ptr(),
                c"main".as_ptr(),
                source.raw.as_ptr(),
                c"main".as_ptr(),
            )
        };
        if raw.is_null() {
            return None;
        }
        Some(Backup {
            raw,
            _connections: PhantomData,
        })
    }

    /// sqlite3_backup_step: -1 copies everything in one pass.
    pub(crate) fn step(&mut self, pages: c_int) -> c_int {
        // SAFETY: the backup is live: only finish() and drop end it, and both
        // take the value.
        unsafe { sqlite3_backup_step(self.raw, pages) }
    }

    /// sqlite3_backup_finish, its result code: SQLITE_OK unless the backup
    /// failed (its message on the destination).
    pub(crate) fn finish(mut self) -> c_int {
        let raw = std::mem::replace(&mut self.raw, ptr::null_mut());
        // SAFETY: the backup is live and is finished exactly once: the handle
        // was taken out above, so the drop that follows finds NULL.
        unsafe { sqlite3_backup_finish(raw) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_write_create() -> c_int {
        SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_FULLMUTEX
    }

    fn memory() -> Connection {
        Connection::open(c":memory:", read_write_create()).expect("an in-memory database opens")
    }

    #[test]
    fn a_connection_whose_statements_were_dropped_closes_without_being_busy() {
        // sqlite3_close answers SQLITE_BUSY (and leaves the connection open)
        // while a statement is unfinalized, so OK shows each statement's drop
        // finalized it before the close.
        let connection = memory();
        {
            let mut statement = connection.prepare(b"SELECT 1").expect("prepares");
            assert_eq!(statement.step(), SQLITE_ROW);
            let _second = connection.prepare(b"SELECT 2").expect("prepares");
        }
        assert_eq!(connection.close(), SQLITE_OK);
    }

    #[test]
    fn a_backup_dropped_unfinished_is_finished_so_both_connections_close() {
        // An unfinished backup also makes sqlite3_close answer SQLITE_BUSY:
        // SQLite counts it on the source's b-tree (sqlite3BtreeIsInBackup),
        // so the source's close is the one that shows the drop finished it.
        let source = memory();
        source.execute(c"CREATE TABLE t (x)").expect("creates");
        let destination = memory();
        {
            let backup = Backup::start(&destination, &source);
            assert!(backup.is_some(), "the backup starts");
        }
        assert_eq!(destination.close(), SQLITE_OK);
        assert_eq!(source.close(), SQLITE_OK);
    }

    #[test]
    fn a_finished_backup_reports_done_then_ok_and_copies_the_rows() {
        let source = memory();
        source
            .execute(c"CREATE TABLE t (x); INSERT INTO t VALUES (42);")
            .expect("creates");
        let destination = memory();
        let mut backup = Backup::start(&destination, &source).expect("starts");
        assert_eq!(backup.step(-1), SQLITE_DONE);
        assert_eq!(backup.finish(), SQLITE_OK);
        let mut statement = destination.prepare(b"SELECT x FROM t").expect("prepares");
        assert_eq!(statement.step(), SQLITE_ROW);
        assert_eq!(statement.column_int64(0), 42);
    }

    #[test]
    fn a_failed_execute_returns_sqlites_message_for_the_missing_table() {
        // "no such table: <name>" is SQLite's documented wording for a
        // reference to a table the schema lacks.
        let connection = memory();
        let failure = connection
            .execute(c"SELECT * FROM missing")
            .expect_err("the table is missing");
        assert_eq!(failure.as_deref(), Some(&b"no such table: missing"[..]));
    }

    #[test]
    fn a_failed_prepare_leaves_its_message_on_the_connection() {
        let connection = memory();
        let code = connection
            .prepare(b"SELECT * FROM nowhere")
            .expect_err("the table is missing");
        // SQLITE_ERROR, the generic error SQLite gives a bad reference.
        assert_eq!(code, 1);
        assert_eq!(connection.error_message(), b"no such table: nowhere");
    }

    #[test]
    fn an_open_that_cannot_create_its_file_returns_sqlites_message() {
        // SQLITE_CANTOPEN's text in sqlite3ErrStr is "unable to open database
        // file"; the handle SQLite returned with it is closed by the drop.
        let failure = Connection::open(
            c"/katana-no-such-folder/sub/project.sqlite",
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_FULLMUTEX,
        )
        .expect_err("the folder does not exist");
        assert_eq!(failure.as_deref(), Some(&b"unable to open database file"[..]));
    }

    #[test]
    fn a_text_holding_no_statement_prepares_to_one_that_steps_as_misuse_and_resets_quietly() {
        // SQLITE_MISUSE (21) is what sqlite3_step answers a NULL statement.
        let connection = memory();
        let mut statement = connection.prepare(b"-- a comment only").expect("prepares");
        assert_eq!(statement.step(), 21);
        assert_eq!(statement.column_type(0), SQLITE_NULL);
        assert!(statement.column_text(0).is_empty());
        statement.reset_and_clear_bindings();
    }

    #[test]
    fn empty_text_binds_as_empty_text_and_an_empty_blob_as_an_empty_blob() {
        let connection = memory();
        let mut statement = connection
            .prepare(b"SELECT typeof(?1), length(?1), typeof(?2), length(?2)")
            .expect("prepares");
        assert_eq!(statement.bind_text(1, b""), SQLITE_OK);
        assert_eq!(statement.bind_zeroblob(2, 0), SQLITE_OK);
        assert_eq!(statement.step(), SQLITE_ROW);
        assert_eq!(statement.column_text(0), b"text");
        assert_eq!(statement.column_int64(1), 0);
        assert_eq!(statement.column_text(2), b"blob");
        assert_eq!(statement.column_int64(3), 0);
    }
}
