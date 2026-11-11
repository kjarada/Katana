//! A name as the command line reads it, for a dialog that builds the verb line a person would type
//! (host::command_runner): double-quoted when it holds a blank - or is empty - which the
//! interpreter's tokenizer would otherwise split or drop. A double quote or a line break cannot be
//! carried at all, since the tokenizer's quoted words have no escape, so a name holding one is
//! refused naming `what` rather than cut short at the quote.
//!
//! The rule itself is katana_cad's (`annotation::command_words::command_word`), the one the
//! annotation dialogs use: this only names the field in the refusal, so every dialog writes a word
//! the same way. `annotation_text_word` is the same for an annotation's text, whose line breaks the
//! line carries as a backslash and an n (`annotation::command_words::annotation_text_word`).

use katana_cad::annotation::command_words;
use katana_core::Result;
use katana_core::error::make_error;
use katana_core::text::format_exact_real;

/// `word` for `text`, or its refusal with `what` named first and the text as the context.
fn named_word(word: Result<String>, text: &str, what: &str) -> Result<String> {
    word.map_err(|error| make_error(error.code, format!("{what}: {}", error.message), text))
}

/// The word `text` is written as in a line, or its refusal naming the field `what`.
pub fn command_word(text: &str, what: &str) -> Result<String> {
    named_word(command_words::command_word(text), text, what)
}

/// A number as a dialog writes it into its line: exactly, the shortest text that reads back as the
/// same double (`format_exact_real`), so a value written back unedited changes nothing.
pub fn exact_number(value: f64) -> String {
    format_exact_real(value)
}

/// An annotation's text as a line carries it, or its refusal naming the field `what`.
pub fn annotation_text_word(text: &str, what: &str) -> Result<String> {
    named_word(command_words::annotation_text_word(text), text, what)
}
