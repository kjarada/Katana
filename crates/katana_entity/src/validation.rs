//! What every named record in the entity layer checks about its name.
//! Internal to the module.
//!
//! This was a file-local helper in tables.cpp serving six tables, which meant
//! the seventh - the style library - would have been a second copy of it.

use katana_core::error::{Error, ErrorCode, Status};

/// A name must be there. The C++ also refused one that is not UTF-8, because
/// every name is written into a project file; a `&str` cannot be anything
/// else, so that half of the rule is kept by the type.
pub(crate) fn validate_name(name: &str, what: &str) -> Status {
    if name.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("{what} name is empty"),
        ));
    }
    Ok(())
}
