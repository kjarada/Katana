//! The part of SQLite's C API (sqlite3.h) that the storage layer calls:
//! exactly those 28 functions, three handle types and eleven constants,
//! written by hand from the header of SQLite 3.53.4, the toolchain's.
//!
//! No binding crate: as for PROJ in katana_geodesy (docs/rust.md, "Third-party
//! libraries"), the declarations are of exactly what the C++ wrapper
//! (src/katana_storage/sqlite_database.cpp) uses, so its every pragma,
//! statement and error path is the C++'s and nothing else of SQLite is
//! reachable. Only `sqlite_database` calls these; it states why each call is
//! sound.

#![allow(
    unsafe_code,
    reason = "The declarations of SQLite's C API; every call of them is in sqlite_database, each with its SAFETY argument."
)]

use std::ffi::{c_char, c_int, c_uchar, c_void};
use std::marker::{PhantomData, PhantomPinned};

/// `sqlite3`, a database connection.
#[repr(C)]
pub(crate) struct Sqlite3 {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// `sqlite3_stmt`, a prepared statement.
#[repr(C)]
pub(crate) struct Sqlite3Stmt {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// `sqlite3_backup`, an online backup in progress.
#[repr(C)]
pub(crate) struct Sqlite3Backup {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// `sqlite3_int64`: `long long int` (or `__int64`), 64 bits on every platform.
pub(crate) type Sqlite3Int64 = i64;
/// `sqlite3_uint64`.
pub(crate) type Sqlite3Uint64 = u64;

/// `int (*callback)(void*, int, char**, char**)`, sqlite3_exec's row callback;
/// the C++ always passes NULL.
pub(crate) type Sqlite3Callback =
    Option<unsafe extern "C" fn(*mut c_void, c_int, *mut *mut c_char, *mut *mut c_char) -> c_int>;

/// The `void(*)(void*)` destructor argument of the bind functions
/// (`sqlite3_destructor_type`). Katana passes only SQLITE_TRANSIENT, which is
/// not a function but the value `((sqlite3_destructor_type)-1)` that SQLite
/// compares against, so the parameter is declared as the pointer-sized
/// integer that value is: Rust cannot make a function pointer of -1 in a
/// constant, and a function pointer and a pointer-sized integer are passed
/// alike on every ABI Katana is built for.
pub(crate) type Sqlite3DestructorType = isize;

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
pub(crate) const SQLITE_TRANSIENT: Sqlite3DestructorType = -1;

/// The text encoding argument of sqlite3_bind_text64.
pub(crate) const SQLITE_UTF8: c_uchar = 1;

/// sqlite3_limit's category for the largest string or BLOB.
pub(crate) const SQLITE_LIMIT_LENGTH: c_int = 0;

#[link(name = "sqlite3")]
unsafe extern "C" {
    // ---- connections -----------------------------------------------------------
    pub(crate) fn sqlite3_open_v2(
        filename: *const c_char,
        pp_db: *mut *mut Sqlite3,
        flags: c_int,
        z_vfs: *const c_char,
    ) -> c_int;
    pub(crate) fn sqlite3_close(db: *mut Sqlite3) -> c_int;
    pub(crate) fn sqlite3_errmsg(db: *mut Sqlite3) -> *const c_char;
    pub(crate) fn sqlite3_extended_result_codes(db: *mut Sqlite3, onoff: c_int) -> c_int;
    pub(crate) fn sqlite3_busy_timeout(db: *mut Sqlite3, ms: c_int) -> c_int;
    pub(crate) fn sqlite3_exec(
        db: *mut Sqlite3,
        sql: *const c_char,
        callback: Sqlite3Callback,
        argument: *mut c_void,
        errmsg: *mut *mut c_char,
    ) -> c_int;
    pub(crate) fn sqlite3_free(pointer: *mut c_void);
    pub(crate) fn sqlite3_limit(db: *mut Sqlite3, id: c_int, new_val: c_int) -> c_int;

    // ---- statements ------------------------------------------------------------
    pub(crate) fn sqlite3_prepare_v2(
        db: *mut Sqlite3,
        z_sql: *const c_char,
        n_byte: c_int,
        pp_stmt: *mut *mut Sqlite3Stmt,
        pz_tail: *mut *const c_char,
    ) -> c_int;
    pub(crate) fn sqlite3_finalize(stmt: *mut Sqlite3Stmt) -> c_int;
    pub(crate) fn sqlite3_bind_int64(
        stmt: *mut Sqlite3Stmt,
        index: c_int,
        value: Sqlite3Int64,
    ) -> c_int;
    pub(crate) fn sqlite3_bind_double(stmt: *mut Sqlite3Stmt, index: c_int, value: f64) -> c_int;
    pub(crate) fn sqlite3_bind_text64(
        stmt: *mut Sqlite3Stmt,
        index: c_int,
        text: *const c_char,
        bytes: Sqlite3Uint64,
        destructor: Sqlite3DestructorType,
        encoding: c_uchar,
    ) -> c_int;
    pub(crate) fn sqlite3_bind_zeroblob(stmt: *mut Sqlite3Stmt, index: c_int, n: c_int) -> c_int;
    pub(crate) fn sqlite3_bind_blob64(
        stmt: *mut Sqlite3Stmt,
        index: c_int,
        value: *const c_void,
        bytes: Sqlite3Uint64,
        destructor: Sqlite3DestructorType,
    ) -> c_int;
    pub(crate) fn sqlite3_bind_null(stmt: *mut Sqlite3Stmt, index: c_int) -> c_int;
    pub(crate) fn sqlite3_step(stmt: *mut Sqlite3Stmt) -> c_int;
    pub(crate) fn sqlite3_reset(stmt: *mut Sqlite3Stmt) -> c_int;
    pub(crate) fn sqlite3_clear_bindings(stmt: *mut Sqlite3Stmt) -> c_int;
    pub(crate) fn sqlite3_column_type(stmt: *mut Sqlite3Stmt, column: c_int) -> c_int;
    pub(crate) fn sqlite3_column_int64(stmt: *mut Sqlite3Stmt, column: c_int) -> Sqlite3Int64;
    pub(crate) fn sqlite3_column_double(stmt: *mut Sqlite3Stmt, column: c_int) -> f64;
    pub(crate) fn sqlite3_column_text(stmt: *mut Sqlite3Stmt, column: c_int) -> *const c_uchar;
    pub(crate) fn sqlite3_column_bytes(stmt: *mut Sqlite3Stmt, column: c_int) -> c_int;
    pub(crate) fn sqlite3_column_blob(stmt: *mut Sqlite3Stmt, column: c_int) -> *const c_void;

    // ---- the online backup API -------------------------------------------------
    pub(crate) fn sqlite3_backup_init(
        destination: *mut Sqlite3,
        destination_name: *const c_char,
        source: *mut Sqlite3,
        source_name: *const c_char,
    ) -> *mut Sqlite3Backup;
    pub(crate) fn sqlite3_backup_step(backup: *mut Sqlite3Backup, pages: c_int) -> c_int;
    pub(crate) fn sqlite3_backup_finish(backup: *mut Sqlite3Backup) -> c_int;
}
