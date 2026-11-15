//! storage handing rusqlite's type to whoever uses it.
pub(crate) use rusqlite::Row; // inside the crate: allowed
pub use rusqlite::Connection;
