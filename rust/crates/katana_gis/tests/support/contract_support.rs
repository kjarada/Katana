//! The contract tests' one helper (tests/geo/contract_support.hpp,
//! docs/geoprocessing.md "Contract tests"): GDAL's algorithm framework is
//! provisional - 3.13 already turned every raster input into a dataset list -
//! and a toolchain upgrades GDAL on a rolling basis, so each package pins the
//! algorithm paths and arguments it binds. When an upgrade renames or
//! reshapes one, the test that fails names what changed, instead of a verb
//! failing in front of a person.
//!
//! One owner (L1); a test target includes it with
//! `#[path = "../support/contract_support.rs"] mod contract_support;`.

use katana_gis::processing::{ArgType, describe, path_text, to_string};

/// The argument `name` of the algorithm at `path` exists, has `type`, and is
/// required or not.
#[track_caller]
pub fn expect_argument(path: &[&str], name: &str, r#type: ArgType, required: bool) {
    let path: Vec<String> = path.iter().map(|word| String::from(*word)).collect();
    let spec = describe(&path)
        .unwrap_or_else(|error| panic!("{}: {}", path_text(&path), error.describe()));
    let found = spec
        .args
        .iter()
        .rfind(|arg| arg.name == name)
        .unwrap_or_else(|| panic!("{} has no argument {name}", path_text(&path)));
    assert_eq!(
        found.r#type,
        r#type,
        "{} {name} is {}, not {}",
        path_text(&path),
        to_string(found.r#type),
        to_string(r#type)
    );
    assert_eq!(
        found.required,
        required,
        "{} {name}{} required",
        path_text(&path),
        if required { " is no longer" } else { " is now" }
    );
}
