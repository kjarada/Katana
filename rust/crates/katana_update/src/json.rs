//! The JSON the update layer reads: nlohmann::json's value, as the C++ parses it, built from
//! serde_json's reader. Private to the crate, so no serde type is in its interface (CLAUDE.md,
//! section 11); nothing here writes JSON (the records the next wave writes have their own writer).
//!
//! serde_json reads the grammar - RFC 8259, strings validated as UTF-8, no comments, no trailing
//! commas, nothing after the value - which is the grammar nlohmann reads with comments off. Where
//! the two readers differ, this module gives nlohmann's answer, because what a signed manifest or
//! an install record means must not depend on which reader read it:
//!
//! * **Numbers keep nlohmann's three kinds.** A whole number without a sign that fits 64 bits is
//!   unsigned; one with a sign that fits is a signed integer; anything else (a fraction, an
//!   exponent, a whole number past 64 bits) is a float. serde_json makes the same split, except
//!   for `-0`, which nlohmann reads as the signed integer 0 and serde_json as the float -0.0:
//!   the text is scanned for number tokens that are exactly `-0` (minus_zero_ordinals), and the
//!   visitor, which sees the numbers in the same order, gives those back as integers. A float no
//!   double holds (`1e999`, 400 nines) is nlohmann's out_of_range 406 and serde_json's "number out
//!   of range": both refuse it, and [`Failure::OutOfRange`] says so.
//! * **A member written twice** keeps its last value, as nlohmann's `operator[]` does, unless the
//!   caller refuses duplicates (the manifest does; the C++ does it in a parser callback).
//! * **A UTF-8 byte-order mark** at the very start is skipped, as nlohmann's lexer skips it (the
//!   manifest refuses one before it gets here).
//! * **A NUL byte ends the input** outside a string, as it does for nlohmann's lexer (inside a
//!   string it is an error to both). So the text is read up to its first NUL: what follows is
//!   never seen, and what comes before must be a whole document. The manifest and a release's
//!   asset list refuse a NUL before they get here; an install record is read as nlohmann reads it.
//! * **Depth.** The caller may refuse nesting deeper than a limit, counted as the C++'s callbacks
//!   count it (the outermost object or list is level 1). Without a limit nlohmann has none, while
//!   serde_json stops at 128 levels; such a document is refused here and read by the C++ (a
//!   divergence on records that no program writes, docs/rust.md, "Not done").
//! * **The place of a syntax error** is the byte offset the reader had reached, from serde_json's
//!   line and column, which is within a byte or two of nlohmann's count of characters read.

use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;

/// nlohmann::json's value. Objects are sorted by key, as nlohmann's std::map holds them.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Json {
    Null,
    Boolean(bool),
    /// number_unsigned: a whole number without a sign that fits 64 bits.
    Unsigned(u64),
    /// number_integer: a whole number with a minus sign that fits 64 bits (`-0` included).
    Integer(i64),
    /// number_float.
    #[allow(
        dead_code,
        reason = "the value is kept as nlohmann keeps it; no reader of this layer asks a float for its value"
    )]
    Float(f64),
    String(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

impl Json {
    pub(crate) fn as_object(&self) -> Option<&BTreeMap<String, Json>> {
        match self {
            Json::Object(members) => Some(members),
            _ => None,
        }
    }

    pub(crate) fn as_array(&self) -> Option<&Vec<Json>> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(text) => Some(text),
            _ => None,
        }
    }

    pub(crate) fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Boolean(value) => Some(*value),
            _ => None,
        }
    }

    /// nlohmann's is_number_unsigned() and get<std::uint64_t>().
    pub(crate) fn as_unsigned(&self) -> Option<u64> {
        match self {
            Json::Unsigned(value) => Some(*value),
            _ => None,
        }
    }

    /// The member `key` of an object; None for a missing member and for a value that is not an
    /// object (nlohmann's find() answers end() for both).
    pub(crate) fn get(&self, key: &str) -> Option<&Json> {
        self.as_object()?.get(key)
    }
}

/// Why a text is not a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    /// Nesting deeper than the caller's limit.
    TooDeep,
    /// A member written twice in one object, where the caller refuses that; its name.
    Duplicate(String),
    /// Not well-formed: the byte offset the reader had reached.
    Syntax(usize),
    /// A number no double holds (nlohmann's out_of_range 406).
    OutOfRange,
}

/// How a text is read.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Options {
    /// The deepest nesting accepted; None for nlohmann's default, no limit of its own.
    pub(crate) max_depth: Option<usize>,
    /// Refuse a member written twice instead of keeping the last value.
    pub(crate) refuse_duplicates: bool,
}

const BYTE_ORDER_MARK: &[u8] = b"\xEF\xBB\xBF";

/// The document `text` holds, read as nlohmann reads it (see the module's comment).
pub(crate) fn parse(text: &[u8], options: Options) -> Result<Json, Failure> {
    let (skipped, text) = match text.strip_prefix(BYTE_ORDER_MARK) {
        Some(rest) => (BYTE_ORDER_MARK.len(), rest),
        None => (0, text),
    };
    let text = match text.iter().position(|&byte| byte == 0) {
        Some(nul) => &text[..nul],
        None => text,
    };

    let state = RefCell::new(State {
        options,
        depth: 0,
        numbers: 0,
        minus_zeros: minus_zero_ordinals(text),
        refusal: None,
    });
    let mut deserializer = serde_json::Deserializer::from_slice(text);
    let parsed = Seed { state: &state }
        .deserialize(&mut deserializer)
        .and_then(|value| deserializer.end().map(|()| value));
    match parsed {
        Ok(value) => Ok(value),
        Err(error) => {
            let refusal = state.borrow_mut().refusal.take();
            if let Some(refusal) = refusal {
                return Err(refusal);
            }
            // serde_json's words for its ErrorCode::NumberOutOfRange; the category alone does not
            // tell it from a syntax error.
            if format!("{error}").starts_with("number out of range") {
                return Err(Failure::OutOfRange);
            }
            Err(Failure::Syntax(
                skipped + byte_offset(text, error.line(), error.column()),
            ))
        }
    }
}

// serde_json counts lines from 1 at each '\n', and a column as the bytes since the start of its
// line; the offset is the start of that line and the column.
fn byte_offset(text: &[u8], line: usize, column: usize) -> usize {
    if line == 0 {
        return column;
    }
    let mut start = 0usize;
    let mut lines = 1usize;
    for (index, &byte) in text.iter().enumerate() {
        if lines == line {
            break;
        }
        if byte == b'\n' {
            lines += 1;
            start = index + 1;
        }
    }
    start + column
}

// The ordinals, in the order they are written, of the number tokens that are exactly `-0`. Strings
// are stepped over (a backslash takes the byte after it with it); a number token is a '-' or a
// digit and every number character after it. In a well-formed document these are exactly its
// numbers, in the order serde_json hands them to the visitor; in any other, the parse fails and
// the count does not matter.
fn minus_zero_ordinals(text: &[u8]) -> Vec<usize> {
    let mut found = Vec::new();
    let mut ordinal = 0usize;
    let mut at = 0usize;
    while at < text.len() {
        match text[at] {
            b'"' => {
                at += 1;
                while at < text.len() && text[at] != b'"' {
                    if text[at] == b'\\' {
                        at += 1;
                    }
                    at += 1;
                }
                at += 1;
            }
            b'-' | b'0'..=b'9' => {
                let start = at;
                at += 1;
                while at < text.len()
                    && matches!(text[at], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
                {
                    at += 1;
                }
                if &text[start..at] == b"-0" {
                    found.push(ordinal);
                }
                ordinal += 1;
            }
            _ => at += 1,
        }
    }
    found
}

struct State {
    options: Options,
    depth: usize,
    // How many numbers the visitor has been handed so far.
    numbers: usize,
    minus_zeros: Vec<usize>,
    // Set just before the visitor fails the parse, so that the refusal, and not serde_json's
    // rendering of it, is what the caller gets.
    refusal: Option<Failure>,
}

#[derive(Clone, Copy)]
struct Seed<'s> {
    state: &'s RefCell<State>,
}

impl Seed<'_> {
    fn refuse<E: de::Error>(self, failure: Failure) -> E {
        self.state.borrow_mut().refusal = Some(failure);
        E::custom("refused")
    }

    // An object or a list begins: one level deeper.
    fn enter<E: de::Error>(self) -> Result<(), E> {
        let too_deep = {
            let mut state = self.state.borrow_mut();
            state.depth += 1;
            let depth = state.depth;
            state.options.max_depth.is_some_and(|max| depth > max)
        };
        if too_deep {
            return Err(self.refuse(Failure::TooDeep));
        }
        Ok(())
    }

    fn leave(self) {
        self.state.borrow_mut().depth -= 1;
    }

    // The ordinal of the number being handed over.
    fn next_number(self) -> usize {
        let mut state = self.state.borrow_mut();
        let ordinal = state.numbers;
        state.numbers += 1;
        ordinal
    }
}

impl<'de> DeserializeSeed<'de> for Seed<'_> {
    type Value = Json;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Json, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Seed<'_> {
    type Value = Json;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Json, E> {
        Ok(Json::Boolean(value))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Json, E> {
        self.next_number();
        Ok(Json::Unsigned(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Json, E> {
        self.next_number();
        Ok(Json::Integer(value))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Json, E> {
        let ordinal = self.next_number();
        let written_minus_zero = value.to_bits() == (-0.0_f64).to_bits()
            && self
                .state
                .borrow()
                .minus_zeros
                .binary_search(&ordinal)
                .is_ok();
        if written_minus_zero {
            return Ok(Json::Integer(0));
        }
        Ok(Json::Float(value))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Json, E> {
        Ok(Json::String(value.to_owned()))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Json, E> {
        Ok(Json::String(value))
    }

    fn visit_unit<E: de::Error>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<Json, A::Error> {
        self.enter::<A::Error>()?;
        let mut list = Vec::new();
        while let Some(item) = items.next_element_seed(self)? {
            list.push(item);
        }
        self.leave();
        Ok(Json::Array(list))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut entries: A) -> Result<Json, A::Error> {
        self.enter::<A::Error>()?;
        let refuse_duplicates = self.state.borrow().options.refuse_duplicates;
        let mut members = BTreeMap::new();
        // The name is judged when it is read, before its value: nlohmann's callback sees the key
        // event first, so a member written twice is refused whatever its second value holds.
        while let Some(key) = entries.next_key::<String>()? {
            if refuse_duplicates && members.contains_key(&key) {
                return Err(self.refuse(Failure::Duplicate(key)));
            }
            let value = entries.next_value_seed(self)?;
            members.insert(key, value);
        }
        self.leave();
        Ok(Json::Object(members))
    }
}
