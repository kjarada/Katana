// This file is a translation into Rust of nlohmann/json 3.12.0 (detail/output/serializer.hpp),
// Copyright (c) 2013-2025 Niels Lohmann <https://nlohmann.me>, used under the MIT
// licence, whose notice is kept in THIRD_PARTY_NOTICES.md ("Code translated into the
// Rust port").
//! nlohmann's serializer (detail/output/serializer.hpp) for JSON text: `dump`, `dump_escaped`,
//! `dump_integer` and `dump_float`, ported from nlohmann 3.12.0.
//!
//! The error handler is not ported: it decides what is written for a byte of a string that is not
//! UTF-8 (strict throws type_error 316, replace writes U+FFFD, ignore drops it), and a `String`
//! holds UTF-8 only, so every handler writes the same text here. For the same reason the escaping
//! walks characters, where nlohmann runs Hoehrmann's UTF-8 decoder over bytes: on valid UTF-8 the
//! two find the same code points.

use super::to_chars::to_chars;
use super::{BasicJson, ObjectMap};

/// nlohmann's `dump(indent, indent_char, ensure_ascii)`: compact for an indent below zero, else
/// pretty-printed with `indent` characters a level (0 puts each member on a line of its own with
/// no indent).
pub(super) fn dump<M: ObjectMap>(
    value: &BasicJson<M>,
    indent: i32,
    indent_char: char,
    ensure_ascii: bool,
) -> String {
    let mut serializer = Serializer {
        out: String::new(),
        // the serializer's indent_string(512, indent_char)
        indent_string: vec![indent_char; 512],
        ensure_ascii,
    };
    if indent >= 0 {
        serializer.dump(value, true, indent as usize, 0);
    } else {
        serializer.dump(value, false, 0, 0);
    }
    serializer.out
}

struct Serializer {
    out: String,
    indent_string: Vec<char>,
    ensure_ascii: bool,
}

impl Serializer {
    /// The first `width` characters of the indent string.
    fn indent(&mut self, width: usize) {
        self.out.extend(&self.indent_string[..width]);
    }

    /// nlohmann doubles its indent string when a level needs more than it holds, and fills the
    /// new half with spaces whatever the indent character. It doubles once: a level past twice
    /// the string's length reads beyond its end, which is undefined. Here it doubles until the
    /// level fits, which gives the same text wherever the C++'s is defined.
    fn reserve_indent(&mut self, width: usize) {
        while self.indent_string.len() < width {
            let doubled = self.indent_string.len() * 2;
            self.indent_string.resize(doubled, ' ');
        }
    }

    fn key(&mut self, key: &str) {
        self.out.push('"');
        write_escaped(&mut self.out, key, self.ensure_ascii);
    }

    fn dump<M: ObjectMap>(
        &mut self,
        value: &BasicJson<M>,
        pretty_print: bool,
        indent_step: usize,
        current_indent: usize,
    ) {
        match value {
            BasicJson::Object(members) => {
                if members.is_empty() {
                    self.out.push_str("{}");
                    return;
                }
                let count = members.len();
                if pretty_print {
                    self.out.push_str("{\n");
                    // the indentation for the members
                    let new_indent = current_indent + indent_step;
                    self.reserve_indent(new_indent);
                    for (index, (key, member)) in members.iter().enumerate() {
                        self.indent(new_indent);
                        self.key(key);
                        self.out.push_str("\": ");
                        self.dump(member, true, indent_step, new_indent);
                        // the last member ends with a line feed, the others with a comma too
                        if index + 1 < count {
                            self.out.push_str(",\n");
                        } else {
                            self.out.push('\n');
                        }
                    }
                    self.indent(current_indent);
                    self.out.push('}');
                } else {
                    self.out.push('{');
                    for (index, (key, member)) in members.iter().enumerate() {
                        self.key(key);
                        self.out.push_str("\":");
                        self.dump(member, false, indent_step, current_indent);
                        if index + 1 < count {
                            self.out.push(',');
                        }
                    }
                    self.out.push('}');
                }
            }
            BasicJson::Array(items) => {
                if items.is_empty() {
                    self.out.push_str("[]");
                    return;
                }
                let count = items.len();
                if pretty_print {
                    self.out.push_str("[\n");
                    let new_indent = current_indent + indent_step;
                    self.reserve_indent(new_indent);
                    for (index, item) in items.iter().enumerate() {
                        self.indent(new_indent);
                        self.dump(item, true, indent_step, new_indent);
                        if index + 1 < count {
                            self.out.push_str(",\n");
                        } else {
                            self.out.push('\n');
                        }
                    }
                    self.indent(current_indent);
                    self.out.push(']');
                } else {
                    self.out.push('[');
                    for (index, item) in items.iter().enumerate() {
                        self.dump(item, false, indent_step, current_indent);
                        if index + 1 < count {
                            self.out.push(',');
                        }
                    }
                    self.out.push(']');
                }
            }
            BasicJson::String(text) => write_string(&mut self.out, text, self.ensure_ascii),
            BasicJson::Boolean(flag) => self.out.push_str(if *flag { "true" } else { "false" }),
            // dump_integer: the decimal digits, a minus sign before a negative number
            BasicJson::NumberInteger(number) => self.out.push_str(&format!("{number}")),
            BasicJson::NumberUnsigned(number) => self.out.push_str(&format!("{number}")),
            BasicJson::NumberFloat(number) => self.out.push_str(&dump_float(*number)),
            BasicJson::Null => self.out.push_str("null"),
        }
    }
}

/// A string as dump() writes it: in double quotes, escaped as [`write_escaped`] escapes it.
pub fn write_string(out: &mut String, text: &str, ensure_ascii: bool) {
    out.push('"');
    write_escaped(out, text, ensure_ascii);
    out.push('"');
}

/// nlohmann's dump_escaped: the two-character escapes for the backspace, the tab, the line feed,
/// the form feed, the carriage return, the quotation mark and the backslash; `\u00xx` (lower-case
/// hex) for every other control character; and with `ensure_ascii`, `\uxxxx` for every code point
/// from DEL (U+007F) up, a surrogate pair beyond U+FFFF. Everything else - '/' included, and DEL
/// and every non-ASCII character without `ensure_ascii` - is written as it is.
pub fn write_escaped(out: &mut String, text: &str, ensure_ascii: bool) {
    for character in text.chars() {
        let codepoint = u32::from(character);
        match codepoint {
            0x08 => out.push_str("\\b"),
            0x09 => out.push_str("\\t"),
            0x0A => out.push_str("\\n"),
            0x0C => out.push_str("\\f"),
            0x0D => out.push_str("\\r"),
            0x22 => out.push_str("\\\""),
            0x5C => out.push_str("\\\\"),
            _ if codepoint <= 0x1F || (ensure_ascii && codepoint >= 0x7F) => {
                if codepoint <= 0xFFFF {
                    out.push_str(&format!("\\u{codepoint:04x}"));
                } else {
                    // snprintf("\\u%04x\\u%04x", 0xD7C0 + (codepoint >> 10), 0xDC00 + (codepoint
                    // & 0x3FF)): the high surrogate is 0xD800 + ((codepoint - 0x10000) >> 10).
                    let high = 0xD7C0 + (codepoint >> 10);
                    let low = 0xDC00 + (codepoint & 0x3FF);
                    out.push_str(&format!("\\u{high:04x}\\u{low:04x}"));
                }
            }
            _ => out.push(character),
        }
    }
}

/// nlohmann's dump_float: `null` for a NaN or an infinity, else its to_chars (Grisu2, see
/// to_chars.rs): "1.0", "0.1", "1e+20", "1e-05", "-0.0".
pub fn dump_float(value: f64) -> String {
    if !value.is_finite() {
        return String::from("null");
    }
    to_chars(value)
}
