//! What editing ONE library definition needs below the window: its strokes as
//! the text an editor shows and takes back, the name a copy starts under, and
//! the command line that removes it (include/katana/cad/definition_edit.hpp;
//! src/katana_cad/customisation/definition_edit.cpp).
//!
//! The window's definition editor is a form over entity's LineStyle with the
//! strokes in a text box. Everything about that text that is not a widget's
//! business is here, where it is tested without one: which line a refusal is
//! about, what a person is told of it, and what a line may not hold.
//!
//! THE STROKES AS TEXT are the Katana customisation format's own lines
//! (docs/customisation.md, "The members"), one stroke a line:
//!
//! ```text
//!     ["move", 0, 0],
//!     ["draw", 8, 0],
//!     ["text", {"text": "W", "height": 1.5}]
//! ```
//!
//! They are read by the format's reader (entity's definition_from_json) and by
//! nothing else: this joins the lines under the definition's members as a file
//! would hold them, hands the result over, and - when it is refused - finds
//! the line. Two things are allowed of the text that a file does not allow,
//! and neither reaches the reader: the comma that ends a line may be left off,
//! and a blank line is passed over.

use katana_core::error::make_error;
use katana_core::text::{equals_ignoring_case, split_lines, trimmed};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::LineStyle;
use katana_entity::customisation::{definition_from_json, definition_to_json};

use crate::annotation::command_words::command_word;
use crate::document::Document;

/// What reading a definition from its members and that text gave.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StrokeTextRead {
    /// The definition, when it read: `members` with the strokes and texts the
    /// text holds.
    pub definition: Option<LineStyle>,
    /// Why not, as the format's writer or reader - or entity's validate
    /// through it - refused it. Its context may cite a line and column of the
    /// joined text, which nobody typed; `problem` is what a person is shown.
    pub error: Error,
    /// The same for a person: the refusal with the LINE of `strokes` it is
    /// about in front ("Line 3: definition "TEST Valve" strokes[2]: "drow" is
    /// not a kind of stroke ..."; "Line 4 is not a stroke: ..." for text that
    /// is not JSON), and with no place in the joined text.
    pub problem: String,
    /// That line, counted from 1 as a text box counts; 0 when it read, or when
    /// what was refused is no line's (a member: a length below 0).
    pub line: i32,
}

/// One line of the text that holds something: its number in the text, from
/// 1, and what it holds without the comma that ends a stroke's line in a
/// file.
#[derive(Clone, Debug)]
struct StrokeLine {
    number: i32,
    text: String,
}

/// The text of `bytes`, which are a str's or a slice of one cut at ASCII
/// bytes, and so UTF-8.
fn text_of(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap_or_default()
}

fn stroke_lines(text: &str) -> Vec<StrokeLine> {
    let mut lines = Vec::new();
    // Numbered from 1, blank lines counted, as the C++ counts them.
    for (number, raw) in (1..).zip(split_lines(text.as_bytes())) {
        let mut line = trimmed(raw);
        if line.is_empty() {
            continue;
        }
        if line.last() == Some(&b',') {
            // A line that is nothing but a comma is kept as it is: it is not
            // a stroke, and passed over it would not be said to be one.
            let stroke = trimmed(&line[..line.len() - 1]);
            if !stroke.is_empty() {
                line = stroke;
            }
        }
        lines.push(StrokeLine {
            number,
            text: String::from(text_of(line)),
        });
    }
    lines
}

/// `header` - a definition with no strokes, as the format writes one: a
/// single line, {"name": "A", "length": 12} - with `lines` as its strokes, one
/// a line as the format lays them out.
fn with_strokes(header: &str, lines: &[StrokeLine]) -> String {
    if lines.is_empty() || !header.ends_with('}') {
        return String::from(header);
    }
    let mut out = String::from(&header[..header.len() - 1]);
    out.push_str(", \"strokes\": [");
    for (index, line) in lines.iter().enumerate() {
        out.push_str(if index == 0 { "\n" } else { ",\n" });
        out.push_str(&line.text);
    }
    out.push_str("\n]}");
    out
}

fn all_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// `context` without the place the reader ends a refusal with - "1e999 at
/// line 2, column 14" - which counts through the text it was handed: the
/// lines joined here under a first line nobody typed. The line is named by
/// read_stroke_text instead.
fn without_place(context: &str) -> String {
    const AT: &str = " at line ";
    const COLUMN: &str = ", column ";
    let Some(at) = context.rfind(AT) else {
        return String::from(context);
    };
    let place = &context[at + AT.len()..];
    let Some(column) = place.find(COLUMN) else {
        return String::from(context);
    };
    if !all_digits(&place[..column]) || !all_digits(&place[column + COLUMN.len()..]) {
        return String::from(context);
    }
    String::from(&context[..at])
}

/// A refusal as a person is told it: what was refused, then what it was
/// refused of. Not Error::describe(), which leads with the code's name.
fn worded(error: &Error) -> String {
    let mut out = error.message.clone();
    let context = without_place(&error.context);
    if !context.is_empty() {
        out.push_str(" - ");
        out.push_str(&context);
    }
    out
}

/// Text that is not JSON is refused in the JSON library's words, as the
/// context: "parse error at line 2, column 9: syntax error while parsing
/// array - ...". What follows the place, when `error` is one of those.
fn syntax_reason(error: &Error) -> Option<String> {
    const PLACE: &str = "parse error at ";
    if !error.context.starts_with(PLACE) {
        return None;
    }
    Some(match error.context.find(": ") {
        Some(colon) => String::from(&error.context[colon + 2..]),
        None => error.context.clone(),
    })
}

fn line_label(line: i32) -> String {
    format!("Line {line}")
}

/// The strokes of `definition` as that text: the lines the format's writer
/// gives them, without the indent, a comma ending each but the last. Empty
/// for a definition with no strokes. Fails as entity's definition_to_json
/// does, for a definition the format cannot write (a stroke carrying a member
/// its kind does not use, texts that are not exactly its text strokes in
/// order).
pub fn stroke_text(definition: &LineStyle) -> Result<String> {
    let written = definition_to_json(definition, "")?;
    // The format writes a definition's members on its first line, then its
    // strokes one a line, then "]}" (docs/customisation.md, "Layout", rule
    // 5); one with no strokes is that first line alone. A text is escaped
    // into its line, so no stroke spans two.
    let lines = split_lines(written.as_bytes());
    let mut text = String::new();
    let mut index = 1;
    while index + 1 < lines.len() {
        if index > 1 {
            text.push('\n');
        }
        text.push_str(text_of(trimmed(lines[index])));
        index += 1;
    }
    Ok(text)
}

/// Reads the definition `members` describes - its strokes and texts are not
/// looked at - with `strokes` as its strokes. `members.symbol` is the list it
/// would sit in. Text that reads as MORE than strokes is refused: a line can
/// end the list and go on to give the definition members of its own
/// (`]], "atVertices": true, ...`), which is sound JSON and would make a
/// definition `members` does not describe.
pub fn read_stroke_text(members: &LineStyle, strokes: &str) -> StrokeTextRead {
    let mut result = StrokeTextRead::default();
    let mut header = members.clone();
    header.strokes.clear();
    header.texts.clear();

    // The members as the format writes them, then the strokes as typed, read
    // back by the format's reader - which is also where entity's validate
    // refuses a length below 0 or a factor of 0.
    let written = match definition_to_json(&header, "") {
        Ok(written) => written,
        Err(error) => {
            result.problem = worded(&error);
            result.error = error;
            return result;
        }
    };
    let typed = stroke_lines(strokes);
    let whole = definition_from_json(with_strokes(&written, &typed).as_bytes(), header.symbol, "");
    let whole = match whole {
        Ok(read) => {
            // Only the strokes are taken from what was read; the members are
            // the caller's. A line that ended the list of strokes and went on
            // to give members of its own would otherwise make a definition
            // `members` does not describe.
            let mut from_members = header;
            from_members.strokes = read.strokes.clone();
            from_members.texts = read.texts.clone();
            if from_members != read {
                result.problem = String::from(
                    "The strokes hold more than strokes: a line ends the list and gives the \
                     definition members of its own. Those are set in the fields above.",
                );
                result.error = Error::new(ErrorCode::InvalidArgument, result.problem.clone());
                return result;
            }
            result.definition = Some(from_members);
            return result;
        }
        Err(error) => error,
    };
    result.error = whole.clone();

    // Which line: each alone, under members of no consequence, until one
    // does not read. Asked only of text that already failed, so a definition
    // that reads costs one reading.
    const ANY_MEMBERS: &str = r#"{"name": "a"}"#;
    for index in 0..typed.len() {
        let alone = definition_from_json(
            with_strokes(ANY_MEMBERS, &typed[index..index + 1]).as_bytes(),
            false,
            "",
        );
        let Err(alone) = alone else {
            continue;
        };
        result.line = typed[index].number;
        if let Some(alone_reason) = syntax_reason(&alone) {
            // Not JSON. The reason is asked of the line by ITSELF: under the
            // members it would be told with the brackets and the brace of a
            // text nobody typed - a line missing its "]" as "unexpected '}'",
            // a word cut short as a string running on into them.
            let bare = definition_from_json(typed[index].text.as_bytes(), false, "");
            let mut reason = match &bare {
                Ok(_) => None,
                Err(error) => syntax_reason(error),
            };
            // By itself, a line holding MORE than one value reads only as
            // "something follows the first" - which is no fault here, where
            // the lines are joined with commas. Then it is asked as the list
            // of the values it holds, in one pair of brackets of this
            // function's making.
            const MORE_FOLLOWS: &str = "expected end of input";
            if reason
                .as_ref()
                .is_none_or(|said| said.ends_with(MORE_FOLLOWS))
            {
                let list = format!("[{}]", typed[index].text);
                if let Err(error) = definition_from_json(list.as_bytes(), false, "")
                    && let Some(listed) = syntax_reason(&error)
                {
                    reason = Some(listed);
                }
            }
            result.problem = format!(
                "{} is not a stroke: {}",
                line_label(result.line),
                reason.unwrap_or(alone_reason)
            );
            return result;
        }
        // A stroke the reader refuses: its own words, from the strokes down
        // to this one, which name the stroke by its place among them and the
        // definition by its name.
        let in_place = definition_from_json(
            with_strokes(&written, &typed[..=index]).as_bytes(),
            header.symbol,
            "",
        );
        let refusal = match &in_place {
            Ok(_) => &alone,
            Err(error) => error,
        };
        result.problem = format!("{}: {}", line_label(result.line), worded(refusal));
        return result;
    }
    result.problem = worded(&whole);
    result
}

/// `base`, else "base 2", "base 3" ...: the first name neither the library
/// nor the drawing's Linetype table holds, so a copy of a definition does not
/// start life over another definition or as a D2 collision (style_resolver).
/// Beside free_style_name and free_linetype_name, which do the same for the
/// two model tables.
pub fn free_definition_name(document: &Document, base: &str) -> String {
    let taken = |name: &str| {
        document.style_library().contains(name) || document.model().linetypes.contains(name)
    };
    let mut name = String::from(base);
    let mut suffix = 2;
    while taken(name.as_str()) {
        name = format!("{base} {suffix}");
        suffix += 1;
    }
    name
}

/// The line that removes the definition `name` from the session's
/// customisation: `CUSTOMISE REMOVE "<name>"`, and ` FORCE` after it when
/// `force` - the name always in quotes, as the line is documented.
///
/// InvalidArgument when no such line can NAME it. The interpreter's words
/// have no escape and its quotes do not survive the tokenizer
/// (annotation/command_words.rs), so a name holding a double quote or a line
/// break cannot be written at all; a name that is one of the line's own words
/// (CODE or FORCE, in any case) would be read as that word, as
/// `CUSTOMISE REMOVE "CODE" FORCE` is "remove the survey code FORCE", not
/// "remove the definition CODE anyway"; and no name is no definition. The same
/// refusal is why an editor does not MAKE a definition under such a name: it
/// could be saved and never removed.
pub fn remove_definition_line(name: &str, force: bool) -> Result<String> {
    if name.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a definition with no name cannot be named on a command line",
        ));
    }
    // The one rule every dialog writes a word by: refused for a double quote
    // or a line break.
    command_word(name)?;
    // The tokenizer removes the quotes, so the verb cannot tell a name from
    // the words of its own grammar: `CUSTOMISE REMOVE <definition>... [FORCE]`
    // and `CUSTOMISE REMOVE CODE <key>...`.
    for keyword in ["CODE", "FORCE"] {
        if equals_ignoring_case(name.as_bytes(), keyword.as_bytes()) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "the name is a word of the CUSTOMISE REMOVE line itself ({keyword}), which a command line cannot tell from the definition"
                ),
                name,
            ));
        }
    }
    let mut line = format!("CUSTOMISE REMOVE \"{name}\"");
    if force {
        line.push_str(" FORCE");
    }
    Ok(line)
}
