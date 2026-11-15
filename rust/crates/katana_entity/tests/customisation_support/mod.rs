//! What the tests of the Katana customisation format share
//! (tests/entity/customisation/customisation_fixture.hpp): the smallest file
//! that is one, and reading and writing with the failure shown.

use katana_core::error::Error;
use katana_entity::customisation::{customisation_from_json, customisation_to_json};
use katana_entity::{Customisation, CustomisationWriteOptions};

/// A customisation holding only what every one must - format, version, name -
/// and then `members`, written as they would be in the file.
pub fn customisation_with(members: &str) -> String {
    let mut text = String::from(r#"{"format": "katana-customisation", "version": 1, "name": "T""#);
    if !members.is_empty() {
        text.push_str(", ");
        text.push_str(members);
    }
    text.push('}');
    text
}

/// The customisation `text` holds; a failure to read fails the test, saying
/// why and showing the text.
pub fn read_customisation(text: impl AsRef<[u8]>) -> Customisation {
    let text = text.as_ref();
    match customisation_from_json(text) {
        Ok(customisation) => customisation,
        Err(error) => panic!("{}\n{}", error.describe(), String::from_utf8_lossy(text)),
    }
}

/// Why `text` is refused; reading it is the failure here.
pub fn refusal_of(text: impl AsRef<[u8]>) -> Error {
    let text = text.as_ref();
    match customisation_from_json(text) {
        Ok(_) => panic!("read without complaint:\n{}", String::from_utf8_lossy(text)),
        Err(error) => error,
    }
}

pub fn write_customisation(customisation: &Customisation) -> String {
    write_customisation_with(customisation, &CustomisationWriteOptions::default())
}

pub fn write_customisation_with(
    customisation: &Customisation,
    options: &CustomisationWriteOptions,
) -> String {
    match customisation_to_json(customisation, options) {
        Ok(text) => text,
        Err(error) => panic!("{}", error.describe()),
    }
}

/// A refusal names the entry first, then says what is wrong with which
/// member: `codes[1] "WM*": unknown member "linesytle"`.
pub fn expect_names(error: &Error, entry: &str, member: &str) {
    let start = format!("{entry}: ");
    assert!(
        error.message.starts_with(&start),
        "the refusal does not begin with the entry {entry}: {}",
        error.describe()
    );
    let quoted = format!("\"{member}\"");
    assert!(
        error.message[start.len()..].contains(&quoted),
        "the refusal does not name {quoted}: {}",
        error.describe()
    );
}
