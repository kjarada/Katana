//! Values written as words of a command line, the inverse of the
//! interpreter's tokenize (include/katana/cad/annotation/command_words.hpp):
//! what a dialog that builds a verb line (CLAUDE.md section 1;
//! docs/desktop.md, "One executor") puts on it, so the verb reads back exactly
//! the value the dialog showed.
//!
//! The tokenizer splits on blanks, groups quoted words and removes the
//! quotes, and has no escape: a value holding a double quote cannot be said
//! on a line at all. It is REFUSED here, naming it, rather than written and
//! cut short at the quote - the silent failure of a line that runs but sets
//! something other than what was asked.

use katana_core::error::make_error;
use katana_core::text::{format_exact_real, is_ascii_space};
use katana_core::{ErrorCode, Result};
use katana_entity::{AnchorPoint, AnchorRef};

/// `text` as one word: bare when it can be, double-quoted when it is empty or
/// holds a blank. InvalidArgument for a double quote or a line break.
pub fn command_word(text: &str) -> Result<String> {
    if text.contains('"') {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a double quote cannot be written on a command line",
            text,
        ));
    }
    if text.contains(['\n', '\r']) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a line break cannot be written on a command line",
            text,
        ));
    }
    let blank = text.is_empty() || text.bytes().any(is_ascii_space);
    Ok(if blank {
        format!("\"{text}\"")
    } else {
        String::from(text)
    })
}

/// An annotation text or template as one word of an annotation verb (TEXT,
/// TEXTEDIT, LEADER text=, LABELSTYLE text=), whose values read "\n" - a
/// backslash and an n - as a line break: each line break written so. Refused
/// as command_word refuses, and for a backslash already followed by an n,
/// which the verb would read as a break the text does not have.
pub fn annotation_text_word(text: &str) -> Result<String> {
    if text.contains("\\n") {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a backslash before an n would be read as a line break",
            text,
        ));
    }
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if c == '\n' {
            escaped.push_str("\\n");
        } else if c != '\r' {
            // A Windows line end is one break, not two.
            escaped.push(c);
        }
    }
    command_word(&escaped)
}

/// A place on an entity as the word the annotation verbs read it from
/// (LEADER ATTACH id `word`, LEADER SET id tip=`word`): `#12.position`,
/// `#12.start`, `#12.end`, `#12.mid`, `#12.centre`, `#12.inside`, `#12.v3`,
/// `#12.s1`, and `#12.along2:0.25` with the fraction written exactly (core's
/// format_exact_real), so the verb reads back the very place the dialog
/// showed. The place of an entity that is not associated (entity 0) has no
/// word: "#0".
pub fn anchor_word(anchor: &AnchorRef) -> String {
    let entity = format!("#{}", anchor.entity);
    match anchor.point {
        AnchorPoint::Position => format!("{entity}.position"),
        AnchorPoint::Start => format!("{entity}.start"),
        AnchorPoint::End => format!("{entity}.end"),
        AnchorPoint::Mid => format!("{entity}.mid"),
        AnchorPoint::Centre => format!("{entity}.centre"),
        AnchorPoint::Vertex => format!("{entity}.v{}", anchor.index),
        AnchorPoint::SegmentMid => format!("{entity}.s{}", anchor.index),
        AnchorPoint::Along => format!(
            "{entity}.along{}:{}",
            anchor.index,
            format_exact_real(anchor.parameter)
        ),
        AnchorPoint::Inside => format!("{entity}.inside"),
    }
}

/// The other direction: a value as a field of a key=value REPLY record, which
/// an agent reads rather than types - bare when it can be, else quoted with
/// \", \\ and \n escaped (an empty value, or one holding a blank, a quote, a
/// backslash, a line break or an '='). Every annotation verb's reply writes
/// its values so.
pub fn record_value(value: &str) -> String {
    let needs = value.is_empty() || value.contains([' ', '"', '\\', '\n', '=']);
    if !needs {
        return String::from(value);
    }
    let mut out = String::from("\"");
    for c in value.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
            out.push(c);
        } else if c == '\n' {
            out.push_str("\\n");
        } else {
            out.push(c);
        }
    }
    out.push('"');
    out
}
