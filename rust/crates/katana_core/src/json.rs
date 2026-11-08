// This file is a translation into Rust of nlohmann/json 3.12.0 (json.hpp and detail/conversions/from_json.hpp),
// Copyright (c) 2013-2025 Niels Lohmann <https://nlohmann.me>, used under the MIT
// licence, whose notice is kept in THIRD_PARTY_NOTICES.md ("Code translated into the
// Rust port").
//! nlohmann::json 3.12, the JSON library of the C++ (entity, cad, app, update, interop): its value,
//! its reader, its writer and the rules of its accessors, with no third-party crate - the JSON twin
//! of the strict XML reader beside it. One home, so that every layer reads and writes JSON as the
//! C++ does, and no crate's interface carries another library's types (CLAUDE.md, section 11).
//!
//! * **Numbers keep nlohmann's three kinds**, as its lexer decides them: a whole number without a
//!   minus sign that fits 64 bits is NumberUnsigned, one with a minus sign that fits is
//!   NumberInteger (`-0` is the integer 0), and anything else (a fraction, an exponent, a whole
//!   number past 64 bits) is NumberFloat. A value made in code takes the kind of what it is made
//!   from, as nlohmann's constructors do: a signed integer type gives NumberInteger, an unsigned
//!   one NumberUnsigned, a double NumberFloat (a NaN or an infinity too, which dump() writes as
//!   `null`).
//! * **Objects come in nlohmann's two orders**: [`Json`] (`nlohmann::json`, whose object is a
//!   std::map: the members sorted by key, byte by byte) and [`OrderedJson`]
//!   (`nlohmann::ordered_json`, whose ordered_map keeps the order the members were put in). A
//!   member written twice in a text keeps its LAST value, and in an ordered object its FIRST
//!   place (the parser assigns through `operator[]`); `j[key] = value` replaces a value in place;
//!   an object made from a list of members ([`BasicJson::object_of`], the C++'s initializer list)
//!   keeps the FIRST of two (it emplaces).
//! * **Reading** ([`BasicJson::parse`], [`sax_parse`]) is nlohmann's lexer and parser ported line
//!   for line (json/sax.rs): its parse_error texts and positions, no limit of depth, a UTF-8 byte
//!   order mark skipped at the start, a NUL byte outside a string taken for the end of the text,
//!   and a number past the largest double refused as out_of_range 406.
//! * **Writing** ([`BasicJson::dump`], [`BasicJson::dump_with`]) is nlohmann's serializer
//!   (json/serializer.rs) with its Grisu2 number text (json/to_chars.rs).
//! * **The accessors fail as nlohmann throws**: a [`JsonError`] whose `what` is the exception's
//!   what() text with JSON_DIAGNOSTICS off (the C++ build's default: no JSON pointer in it).
//! * **A value is released without recursion**, as nlohmann's destructor does it, so a document
//!   nested a hundred thousand deep is read and dropped as any other (copying, comparing and
//!   writing one recurse, in the C++ as here).
//!
//! What is not ported: comments (`ignore_comments`, off in every call the C++ makes), the binary
//! formats, JSON pointers and patches, the conversion between the two orders, and the parser
//! callback's power to discard what it is shown. A reader that watches the events, as the update
//! layer's callbacks do, wraps a [`DomBuilder`] in a [`Sax`] of its own and hands each event on.

mod sax;
mod serializer;
mod to_chars;

pub use sax::{Sax, sax_parse};
pub use serializer::{dump_float, write_escaped, write_string};

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::btree_map;
use std::fmt;
use std::ops::{Index, IndexMut};

// ---- the exceptions -----------------------------------------------------------------------------

/// The class of a nlohmann exception (detail/exceptions.hpp).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JsonErrorKind {
    ParseError,
    InvalidIterator,
    TypeError,
    OutOfRange,
    OtherError,
}

impl JsonErrorKind {
    /// The class's name as what() spells it.
    pub const fn name(self) -> &'static str {
        match self {
            JsonErrorKind::ParseError => "parse_error",
            JsonErrorKind::InvalidIterator => "invalid_iterator",
            JsonErrorKind::TypeError => "type_error",
            JsonErrorKind::OutOfRange => "out_of_range",
            JsonErrorKind::OtherError => "other_error",
        }
    }
}

/// A nlohmann exception: its class, its id, the byte a parse_error stopped at (nlohmann's
/// `parse_error::byte`, the bytes read so far; 0 for the other classes, which have none) and its
/// what() text, "[json.exception.<class>.<id>] <message>".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonError {
    pub kind: JsonErrorKind,
    pub id: u32,
    pub byte: usize,
    pub what: String,
}

/// What a nlohmann call that throws gives.
pub type JsonResult<T> = std::result::Result<T, JsonError>;

impl JsonError {
    fn new(kind: JsonErrorKind, id: u32, byte: usize, message: &str) -> Self {
        // exception::name, with JSON_DIAGNOSTICS off: nothing between the name and the message.
        Self {
            kind,
            id,
            byte,
            what: format!("[json.exception.{}.{id}] {message}", kind.name()),
        }
    }

    /// parse_error::create(id, position, message): "parse error at line L, column C: <message>",
    /// the line counted from 1 and the column the characters read on it.
    pub fn parse_error(id: u32, byte: usize, line: usize, column: usize, message: &str) -> Self {
        Self::new(
            JsonErrorKind::ParseError,
            id,
            byte,
            &format!("parse error at line {line}, column {column}: {message}"),
        )
    }

    pub fn invalid_iterator(id: u32, message: &str) -> Self {
        Self::new(JsonErrorKind::InvalidIterator, id, 0, message)
    }

    pub fn type_error(id: u32, message: &str) -> Self {
        Self::new(JsonErrorKind::TypeError, id, 0, message)
    }

    pub fn out_of_range(id: u32, message: &str) -> Self {
        Self::new(JsonErrorKind::OutOfRange, id, 0, message)
    }

    pub fn other_error(id: u32, message: &str) -> Self {
        Self::new(JsonErrorKind::OtherError, id, 0, message)
    }
}

impl fmt::Display for JsonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.what)
    }
}

impl std::error::Error for JsonError {}

// ---- the objects --------------------------------------------------------------------------------

/// An object of a [`BasicJson`]: nlohmann's `object_t`, one of the two orders.
pub trait ObjectMap: Clone + Default + fmt::Debug + PartialEq {
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn get(&self, key: &str) -> Option<&BasicJson<Self>>;

    fn get_mut(&mut self, key: &str) -> Option<&mut BasicJson<Self>>;

    fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// emplace(key, value): `value` goes in only when there is no member `key`; whether it went
    /// in. An existing member keeps its value and its place.
    fn emplace(&mut self, key: String, value: BasicJson<Self>) -> bool;

    /// `operator[](key)`: the member `key`, a null put in for it first when there is none (at the
    /// end, in an ordered object).
    fn entry(&mut self, key: String) -> &mut BasicJson<Self>;

    /// erase(key): how many members went, 0 or 1. An ordered object keeps the order of the rest.
    fn erase(&mut self, key: &str) -> usize;

    /// The members in the object's order.
    fn iter(&self) -> Box<dyn Iterator<Item = (&str, &BasicJson<Self>)> + '_>;

    fn iter_mut(&mut self) -> Box<dyn Iterator<Item = (&str, &mut BasicJson<Self>)> + '_>;

    /// Every member's value moved onto `into`, the object left empty: how a value is released
    /// without recursion.
    fn drain_values(&mut self, into: &mut Vec<BasicJson<Self>>);
}

/// `nlohmann::json`'s object, a std::map: the members sorted by key.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SortedMap(BTreeMap<String, Json>);

impl ObjectMap for SortedMap {
    fn len(&self) -> usize {
        self.0.len()
    }

    fn get(&self, key: &str) -> Option<&Json> {
        self.0.get(key)
    }

    fn get_mut(&mut self, key: &str) -> Option<&mut Json> {
        self.0.get_mut(key)
    }

    fn emplace(&mut self, key: String, value: Json) -> bool {
        match self.0.entry(key) {
            btree_map::Entry::Occupied(_) => false,
            btree_map::Entry::Vacant(slot) => {
                slot.insert(value);
                true
            }
        }
    }

    fn entry(&mut self, key: String) -> &mut Json {
        self.0.entry(key).or_default()
    }

    fn erase(&mut self, key: &str) -> usize {
        usize::from(self.0.remove(key).is_some())
    }

    fn iter(&self) -> Box<dyn Iterator<Item = (&str, &Json)> + '_> {
        Box::new(self.0.iter().map(|(key, value)| (key.as_str(), value)))
    }

    fn iter_mut(&mut self) -> Box<dyn Iterator<Item = (&str, &mut Json)> + '_> {
        Box::new(self.0.iter_mut().map(|(key, value)| (key.as_str(), value)))
    }

    fn drain_values(&mut self, into: &mut Vec<Json>) {
        into.extend(std::mem::take(&mut self.0).into_values());
    }
}

/// `nlohmann::ordered_json`'s object, nlohmann's ordered_map: a list of members in the order they
/// were put in, looked up from the front, as nlohmann's is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OrderedMap(Vec<(String, OrderedJson)>);

impl OrderedMap {
    fn position(&self, key: &str) -> Option<usize> {
        self.0.iter().position(|(name, _)| name == key)
    }
}

impl ObjectMap for OrderedMap {
    fn len(&self) -> usize {
        self.0.len()
    }

    fn get(&self, key: &str) -> Option<&OrderedJson> {
        self.0
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }

    fn get_mut(&mut self, key: &str) -> Option<&mut OrderedJson> {
        self.0
            .iter_mut()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }

    fn emplace(&mut self, key: String, value: OrderedJson) -> bool {
        if self.position(&key).is_some() {
            return false;
        }
        self.0.push((key, value));
        true
    }

    fn entry(&mut self, key: String) -> &mut OrderedJson {
        let index = match self.position(&key) {
            Some(index) => index,
            None => {
                self.0.push((key, OrderedJson::Null));
                self.0.len() - 1
            }
        };
        &mut self.0[index].1
    }

    fn erase(&mut self, key: &str) -> usize {
        match self.position(key) {
            Some(index) => {
                self.0.remove(index);
                1
            }
            None => 0,
        }
    }

    fn iter(&self) -> Box<dyn Iterator<Item = (&str, &OrderedJson)> + '_> {
        Box::new(self.0.iter().map(|(key, value)| (key.as_str(), value)))
    }

    fn iter_mut(&mut self) -> Box<dyn Iterator<Item = (&str, &mut OrderedJson)> + '_> {
        Box::new(self.0.iter_mut().map(|(key, value)| (key.as_str(), value)))
    }

    fn drain_values(&mut self, into: &mut Vec<OrderedJson>) {
        into.extend(self.0.drain(..).map(|(_, value)| value));
    }
}

// ---- the value ----------------------------------------------------------------------------------

/// nlohmann's `basic_json`, with the object order `M`. Its variants are nlohmann's value_t but for
/// `binary` and `discarded`, which no JSON text holds.
///
/// It implements Drop (the release without recursion), so a variant's contents cannot be moved out
/// by a pattern: match by reference, or `std::mem::take` the contents.
#[derive(Clone, Debug, Default)]
pub enum BasicJson<M: ObjectMap> {
    #[default]
    Null,
    Boolean(bool),
    NumberInteger(i64),
    NumberUnsigned(u64),
    NumberFloat(f64),
    String(String),
    Array(Vec<BasicJson<M>>),
    Object(M),
}

/// `nlohmann::json`: objects sorted by key.
pub type Json = BasicJson<SortedMap>;

/// `nlohmann::ordered_json`: objects in the order their members were put in.
pub type OrderedJson = BasicJson<OrderedMap>;

impl<M: ObjectMap> Drop for BasicJson<M> {
    /// json_value::destroy: the children are moved onto a list and released from it one by one,
    /// each emptied of its own children first, so no level of the program's stack is spent on a
    /// level of the document.
    fn drop(&mut self) {
        let mut pending = match self {
            BasicJson::Array(items) if !items.is_empty() => std::mem::take(items),
            BasicJson::Object(members) if !members.is_empty() => {
                let mut values = Vec::with_capacity(members.len());
                members.drain_values(&mut values);
                values
            }
            _ => return,
        };
        while let Some(mut item) = pending.pop() {
            match &mut item {
                BasicJson::Array(items) => pending.append(items),
                BasicJson::Object(members) => members.drain_values(&mut pending),
                _ => {}
            }
            // `item` is released here, with nothing left below it.
        }
    }
}

impl<M: ObjectMap> PartialEq for BasicJson<M> {
    /// nlohmann's operator==: values of one kind compare as that kind (an object member by member
    /// in its order, a NaN unequal to everything); numbers of two kinds compare as numbers, through
    /// the double where one is a float and through the signed integer otherwise; anything else is
    /// unequal.
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Null, Self::Null) => true,
            (Self::Boolean(a), Self::Boolean(b)) => a == b,
            (Self::NumberInteger(a), Self::NumberInteger(b)) => a == b,
            (Self::NumberUnsigned(a), Self::NumberUnsigned(b)) => a == b,
            (Self::NumberFloat(a), Self::NumberFloat(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Array(a), Self::Array(b)) => a == b,
            (Self::Object(a), Self::Object(b)) => a == b,
            (Self::NumberInteger(a), Self::NumberFloat(b)) => (*a as f64) == *b,
            (Self::NumberFloat(a), Self::NumberInteger(b)) => *a == (*b as f64),
            (Self::NumberUnsigned(a), Self::NumberFloat(b)) => (*a as f64) == *b,
            (Self::NumberFloat(a), Self::NumberUnsigned(b)) => *a == (*b as f64),
            (Self::NumberUnsigned(a), Self::NumberInteger(b)) => (*a as i64) == *b,
            (Self::NumberInteger(a), Self::NumberUnsigned(b)) => *a == (*b as i64),
            _ => false,
        }
    }
}

fn key_not_found(key: &str) -> JsonError {
    JsonError::out_of_range(403, &format!("key '{key}' not found"))
}

fn index_out_of_range(index: usize) -> JsonError {
    JsonError::out_of_range(401, &format!("array index {index} is out of range"))
}

impl<M: ObjectMap> BasicJson<M> {
    // ---- making values ----

    /// `json::object()`: an empty object.
    pub fn object() -> Self {
        Self::Object(M::default())
    }

    /// `json::array()`: an empty array.
    pub fn array() -> Self {
        Self::Array(Vec::new())
    }

    /// An object from its members, as the C++'s initializer list `{{"a", 1}, {"b", 2}}` makes one:
    /// each is emplaced, so of a key given twice the first is kept.
    pub fn object_of<K: Into<String>>(members: impl IntoIterator<Item = (K, Self)>) -> Self {
        let mut object = M::default();
        for (key, value) in members {
            object.emplace(key.into(), value);
        }
        Self::Object(object)
    }

    /// `json::array({...})`: an array of the values, in their order.
    pub fn array_of(items: impl IntoIterator<Item = Self>) -> Self {
        Self::Array(items.into_iter().collect())
    }

    // ---- what it is ----

    /// nlohmann's type_name(), the word its exceptions use.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Object(_) => "object",
            Self::Array(_) => "array",
            Self::String(_) => "string",
            Self::Boolean(_) => "boolean",
            Self::NumberInteger(_) | Self::NumberUnsigned(_) | Self::NumberFloat(_) => "number",
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn is_boolean(&self) -> bool {
        matches!(self, Self::Boolean(_))
    }

    /// Any of the three kinds of number.
    pub fn is_number(&self) -> bool {
        matches!(
            self,
            Self::NumberInteger(_) | Self::NumberUnsigned(_) | Self::NumberFloat(_)
        )
    }

    /// A whole number, signed or unsigned.
    pub fn is_number_integer(&self) -> bool {
        matches!(self, Self::NumberInteger(_) | Self::NumberUnsigned(_))
    }

    pub fn is_number_unsigned(&self) -> bool {
        matches!(self, Self::NumberUnsigned(_))
    }

    pub fn is_number_float(&self) -> bool {
        matches!(self, Self::NumberFloat(_))
    }

    pub fn is_string(&self) -> bool {
        matches!(self, Self::String(_))
    }

    pub fn is_array(&self) -> bool {
        matches!(self, Self::Array(_))
    }

    pub fn is_object(&self) -> bool {
        matches!(self, Self::Object(_))
    }

    /// Null, a string, a boolean or a number.
    pub fn is_primitive(&self) -> bool {
        !self.is_structured()
    }

    /// An array or an object.
    pub fn is_structured(&self) -> bool {
        matches!(self, Self::Array(_) | Self::Object(_))
    }

    // ---- what it holds, when it is that (nlohmann's get_ptr) ----

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Boolean(flag) => Some(*flag),
            _ => None,
        }
    }

    /// The value of a NumberInteger, and of nothing else.
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Self::NumberInteger(number) => Some(*number),
            _ => None,
        }
    }

    /// The value of a NumberUnsigned, and of nothing else: what `is_number_unsigned()` and
    /// `get<std::uint64_t>()` read together.
    pub fn as_unsigned(&self) -> Option<u64> {
        match self {
            Self::NumberUnsigned(number) => Some(*number),
            _ => None,
        }
    }

    /// The value of a NumberFloat, and of nothing else.
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Self::NumberFloat(number) => Some(*number),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Self>> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Self>> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&M> {
        match self {
            Self::Object(members) => Some(members),
            _ => None,
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut M> {
        match self {
            Self::Object(members) => Some(members),
            _ => None,
        }
    }

    // ---- reading it as nlohmann reads it ----

    /// `get<T>()`, by nlohmann's from_json for T (see [`FromJson`]).
    pub fn get<T: FromJson>(&self) -> JsonResult<T> {
        T::from_json(self)
    }

    /// `at(key)`: out_of_range 403 for a missing member, type_error 304 for a value that is not
    /// an object.
    pub fn at(&self, key: &str) -> JsonResult<&Self> {
        match self {
            Self::Object(members) => members.get(key).ok_or_else(|| key_not_found(key)),
            _ => Err(JsonError::type_error(
                304,
                &format!("cannot use at() with {}", self.type_name()),
            )),
        }
    }

    pub fn at_mut(&mut self, key: &str) -> JsonResult<&mut Self> {
        let type_name = self.type_name();
        match self {
            Self::Object(members) => members.get_mut(key).ok_or_else(|| key_not_found(key)),
            _ => Err(JsonError::type_error(
                304,
                &format!("cannot use at() with {type_name}"),
            )),
        }
    }

    /// `at(index)`: out_of_range 401 past the end, type_error 304 for a value that is not an
    /// array.
    pub fn at_index(&self, index: usize) -> JsonResult<&Self> {
        match self {
            Self::Array(items) => items.get(index).ok_or_else(|| index_out_of_range(index)),
            _ => Err(JsonError::type_error(
                304,
                &format!("cannot use at() with {}", self.type_name()),
            )),
        }
    }

    pub fn at_index_mut(&mut self, index: usize) -> JsonResult<&mut Self> {
        let type_name = self.type_name();
        match self {
            Self::Array(items) => items
                .get_mut(index)
                .ok_or_else(|| index_out_of_range(index)),
            _ => Err(JsonError::type_error(
                304,
                &format!("cannot use at() with {type_name}"),
            )),
        }
    }

    /// `value(key, default)`: the member read as T when it is there (its type_error when it is
    /// not a T), `default` when it is not, and type_error 306 for a value that is not an object.
    /// The C++ chooses T by the default's type, as the type of `default` does here.
    pub fn value<T: FromJson>(&self, key: &str, default: T) -> JsonResult<T> {
        match self {
            Self::Object(members) => match members.get(key) {
                Some(member) => member.get::<T>(),
                None => Ok(default),
            },
            _ => Err(JsonError::type_error(
                306,
                &format!("cannot use value() with {}", self.type_name()),
            )),
        }
    }

    /// `contains(key)`: false for a value that is not an object.
    pub fn contains(&self, key: &str) -> bool {
        matches!(self, Self::Object(members) if members.contains_key(key))
    }

    /// `find(key)`: the member, or None (nlohmann's end()) when there is none or the value is not
    /// an object.
    pub fn find(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Object(members) => members.get(key),
            _ => None,
        }
    }

    pub fn find_mut(&mut self, key: &str) -> Option<&mut Self> {
        match self {
            Self::Object(members) => members.get_mut(key),
            _ => None,
        }
    }

    /// `size()`: an array's elements or an object's members, 0 for null, 1 for anything else.
    pub fn size(&self) -> usize {
        match self {
            Self::Null => 0,
            Self::Array(items) => items.len(),
            Self::Object(members) => members.len(),
            _ => 1,
        }
    }

    /// `empty()`: true for null and for an empty array or object, false for anything else.
    pub fn empty(&self) -> bool {
        match self {
            Self::Null => true,
            Self::Array(items) => items.is_empty(),
            Self::Object(members) => members.is_empty(),
            _ => false,
        }
    }

    /// What `for (const auto& item : j)` visits: an array's elements, an object's values in its
    /// order, nothing for null, and a primitive value itself.
    pub fn iter(&self) -> Box<dyn Iterator<Item = &Self> + '_> {
        match self {
            Self::Null => Box::new(std::iter::empty::<&Self>()),
            Self::Array(items) => Box::new(items.iter()),
            Self::Object(members) => Box::new(members.iter().map(|(_, value)| value)),
            _ => Box::new(std::iter::once(self)),
        }
    }

    /// `items()`: each element with its key - an object's member names, an array's indices as
    /// text ("0", "1", ...), "" for a primitive value, which is its own one item; nothing for null.
    pub fn items(&self) -> Vec<(Cow<'_, str>, &Self)> {
        match self {
            Self::Null => Vec::new(),
            Self::Array(items) => items
                .iter()
                .enumerate()
                .map(|(index, item)| (Cow::Owned(format!("{index}")), item))
                .collect(),
            Self::Object(members) => members
                .iter()
                .map(|(key, value)| (Cow::Borrowed(key), value))
                .collect(),
            _ => vec![(Cow::Borrowed(""), self)],
        }
    }

    // ---- changing it ----

    /// `push_back(value)`: null becomes an array first; type_error 308 for anything else that is
    /// not an array.
    pub fn push_back(&mut self, value: Self) -> JsonResult<()> {
        if self.is_null() {
            *self = Self::Array(Vec::new());
        }
        let type_name = self.type_name();
        match self {
            Self::Array(items) => {
                items.push(value);
                Ok(())
            }
            _ => Err(JsonError::type_error(
                308,
                &format!("cannot use push_back() with {type_name}"),
            )),
        }
    }

    /// `erase(key)`: how many members went (0 or 1); type_error 307 for a value that is not an
    /// object.
    pub fn erase(&mut self, key: &str) -> JsonResult<usize> {
        let type_name = self.type_name();
        match self {
            Self::Object(members) => Ok(members.erase(key)),
            _ => Err(JsonError::type_error(
                307,
                &format!("cannot use erase() with {type_name}"),
            )),
        }
    }

    // ---- text ----

    /// `json::parse(text)`: strict (the text is one value and nothing after it), without
    /// comments; the parse_error (or the out_of_range 406 of a number past the largest double)
    /// nlohmann would throw.
    pub fn parse(text: impl AsRef<[u8]>) -> JsonResult<Self> {
        let mut builder = DomBuilder::new();
        if sax_parse(text.as_ref(), &mut builder) {
            return Ok(builder.into_value());
        }
        Err(builder
            .into_error()
            .expect("the tree builder stops a parse only at a parse error, which it keeps"))
    }

    /// `dump()`: compact, no space anywhere.
    pub fn dump(&self) -> String {
        serializer::dump(self, -1, ' ', false)
    }

    /// `dump(indent, indent_char, ensure_ascii, error_handler)` as the C++ calls it: compact for
    /// an indent below zero, else a member or element to a line, `indent` characters a level and
    /// `": "` after a name; with `ensure_ascii`, every character from DEL up written as a
    /// `\uxxxx` escape. nlohmann's error_handler is not a parameter (json/serializer.rs): it
    /// decides only what a byte that is not UTF-8 becomes, and a String holds none, so `strict`,
    /// `replace` and `ignore` all write this text.
    pub fn dump_with(&self, indent: i32, indent_char: char, ensure_ascii: bool) -> String {
        serializer::dump(self, indent, indent_char, ensure_ascii)
    }
}

impl<M: ObjectMap> Index<&str> for BasicJson<M> {
    type Output = BasicJson<M>;

    /// The const `operator[](key)`. nlohmann's is undefined for a missing member (an assertion);
    /// here it panics, as it does for a value that is not an object (nlohmann's type_error 305).
    fn index(&self, key: &str) -> &Self::Output {
        match self {
            Self::Object(members) => match members.get(key) {
                Some(member) => member,
                None => panic!("{}", key_not_found(key).what),
            },
            _ => panic!(
                "{}",
                JsonError::type_error(
                    305,
                    &format!(
                        "cannot use operator[] with a string argument with {}",
                        self.type_name()
                    ),
                )
                .what
            ),
        }
    }
}

impl<M: ObjectMap> IndexMut<&str> for BasicJson<M> {
    /// `operator[](key)`: null becomes an object first; a missing member is put in as null (at the
    /// end of an ordered object) and an existing one is handed back in its place, so assigning
    /// through it replaces the value. Panics with nlohmann's type_error 305 for a value that is
    /// neither null nor an object.
    fn index_mut(&mut self, key: &str) -> &mut Self::Output {
        if self.is_null() {
            *self = Self::object();
        }
        let type_name = self.type_name();
        match self {
            Self::Object(members) => members.entry(String::from(key)),
            _ => panic!(
                "{}",
                JsonError::type_error(
                    305,
                    &format!("cannot use operator[] with a string argument with {type_name}"),
                )
                .what
            ),
        }
    }
}

impl<M: ObjectMap> Index<usize> for BasicJson<M> {
    type Output = BasicJson<M>;

    /// The const `operator[](index)`: undefined past the end in nlohmann (a panic here), type_error
    /// 305 for a value that is not an array.
    fn index(&self, index: usize) -> &Self::Output {
        match self {
            Self::Array(items) => &items[index],
            _ => panic!(
                "{}",
                JsonError::type_error(
                    305,
                    &format!(
                        "cannot use operator[] with a numeric argument with {}",
                        self.type_name()
                    ),
                )
                .what
            ),
        }
    }
}

impl<M: ObjectMap> IndexMut<usize> for BasicJson<M> {
    /// `operator[](index)`: null becomes an array first, and an array too short is filled with
    /// nulls up to the index. Panics with nlohmann's type_error 305 for anything else.
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        if self.is_null() {
            *self = Self::array();
        }
        let type_name = self.type_name();
        match self {
            Self::Array(items) => {
                if index >= items.len() {
                    items.resize(index + 1, Self::Null);
                }
                &mut items[index]
            }
            _ => panic!(
                "{}",
                JsonError::type_error(
                    305,
                    &format!("cannot use operator[] with a numeric argument with {type_name}"),
                )
                .what
            ),
        }
    }
}

// ---- values from Rust's (nlohmann's constructors, to_json) ---------------------------------------

impl<M: ObjectMap> From<bool> for BasicJson<M> {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}

impl<M: ObjectMap> From<i32> for BasicJson<M> {
    fn from(value: i32) -> Self {
        Self::NumberInteger(i64::from(value))
    }
}

impl<M: ObjectMap> From<i64> for BasicJson<M> {
    fn from(value: i64) -> Self {
        Self::NumberInteger(value)
    }
}

impl<M: ObjectMap> From<isize> for BasicJson<M> {
    fn from(value: isize) -> Self {
        Self::NumberInteger(value as i64)
    }
}

impl<M: ObjectMap> From<u32> for BasicJson<M> {
    fn from(value: u32) -> Self {
        Self::NumberUnsigned(u64::from(value))
    }
}

impl<M: ObjectMap> From<u64> for BasicJson<M> {
    fn from(value: u64) -> Self {
        Self::NumberUnsigned(value)
    }
}

impl<M: ObjectMap> From<usize> for BasicJson<M> {
    fn from(value: usize) -> Self {
        Self::NumberUnsigned(value as u64)
    }
}

impl<M: ObjectMap> From<f32> for BasicJson<M> {
    fn from(value: f32) -> Self {
        Self::NumberFloat(f64::from(value))
    }
}

impl<M: ObjectMap> From<f64> for BasicJson<M> {
    fn from(value: f64) -> Self {
        Self::NumberFloat(value)
    }
}

impl<M: ObjectMap> From<&str> for BasicJson<M> {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl<M: ObjectMap> From<&String> for BasicJson<M> {
    fn from(value: &String) -> Self {
        Self::String(value.clone())
    }
}

impl<M: ObjectMap> From<String> for BasicJson<M> {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl<M: ObjectMap> From<Vec<BasicJson<M>>> for BasicJson<M> {
    fn from(value: Vec<BasicJson<M>>) -> Self {
        Self::Array(value)
    }
}

// ---- values to Rust's (nlohmann's from_json) -----------------------------------------------------

/// What `get<T>()` reads a value as, by nlohmann's from_json for T.
///
/// A number is converted to an arithmetic type with a static_cast, here Rust's `as`: the same for
/// every integer conversion (modular) and every double in range; a double out of an integer's
/// range is undefined in C++ and saturates here. nlohmann's own number types (`double`,
/// `std::int64_t`, `std::uint64_t`, and `std::size_t`, which is one of them) are read by
/// get_arithmetic_value and refuse a boolean; every other arithmetic type (`int`,
/// `std::uint32_t`, `float`) takes the generic from_json, which reads a boolean as 0 or 1. What is
/// not a number is type_error 302, "type must be number, but is <type>".
pub trait FromJson: Sized {
    fn from_json<M: ObjectMap>(json: &BasicJson<M>) -> JsonResult<Self>;
}

/// A number as from_json's switch sees it.
enum Arithmetic {
    Unsigned(u64),
    Integer(i64),
    Float(f64),
}

fn arithmetic<M: ObjectMap>(json: &BasicJson<M>, booleans: bool) -> JsonResult<Arithmetic> {
    match json {
        BasicJson::NumberUnsigned(number) => Ok(Arithmetic::Unsigned(*number)),
        BasicJson::NumberInteger(number) => Ok(Arithmetic::Integer(*number)),
        BasicJson::NumberFloat(number) => Ok(Arithmetic::Float(*number)),
        BasicJson::Boolean(flag) if booleans => Ok(Arithmetic::Unsigned(u64::from(*flag))),
        _ => Err(JsonError::type_error(
            302,
            &format!("type must be number, but is {}", json.type_name()),
        )),
    }
}

impl FromJson for f64 {
    fn from_json<M: ObjectMap>(json: &BasicJson<M>) -> JsonResult<Self> {
        Ok(match arithmetic(json, false)? {
            Arithmetic::Unsigned(number) => number as f64,
            Arithmetic::Integer(number) => number as f64,
            Arithmetic::Float(number) => number,
        })
    }
}

impl FromJson for f32 {
    fn from_json<M: ObjectMap>(json: &BasicJson<M>) -> JsonResult<Self> {
        Ok(match arithmetic(json, true)? {
            Arithmetic::Unsigned(number) => number as f32,
            Arithmetic::Integer(number) => number as f32,
            Arithmetic::Float(number) => number as f32,
        })
    }
}

impl FromJson for i64 {
    fn from_json<M: ObjectMap>(json: &BasicJson<M>) -> JsonResult<Self> {
        Ok(match arithmetic(json, false)? {
            Arithmetic::Unsigned(number) => number as i64,
            Arithmetic::Integer(number) => number,
            Arithmetic::Float(number) => number as i64,
        })
    }
}

impl FromJson for u64 {
    fn from_json<M: ObjectMap>(json: &BasicJson<M>) -> JsonResult<Self> {
        Ok(match arithmetic(json, false)? {
            Arithmetic::Unsigned(number) => number,
            Arithmetic::Integer(number) => number as u64,
            Arithmetic::Float(number) => number as u64,
        })
    }
}

impl FromJson for usize {
    fn from_json<M: ObjectMap>(json: &BasicJson<M>) -> JsonResult<Self> {
        Ok(match arithmetic(json, false)? {
            Arithmetic::Unsigned(number) => number as usize,
            Arithmetic::Integer(number) => number as usize,
            Arithmetic::Float(number) => number as usize,
        })
    }
}

impl FromJson for i32 {
    fn from_json<M: ObjectMap>(json: &BasicJson<M>) -> JsonResult<Self> {
        Ok(match arithmetic(json, true)? {
            Arithmetic::Unsigned(number) => number as i32,
            Arithmetic::Integer(number) => number as i32,
            Arithmetic::Float(number) => number as i32,
        })
    }
}

impl FromJson for u32 {
    fn from_json<M: ObjectMap>(json: &BasicJson<M>) -> JsonResult<Self> {
        Ok(match arithmetic(json, true)? {
            Arithmetic::Unsigned(number) => number as u32,
            Arithmetic::Integer(number) => number as u32,
            Arithmetic::Float(number) => number as u32,
        })
    }
}

impl FromJson for bool {
    /// type_error 302 for anything but a boolean.
    fn from_json<M: ObjectMap>(json: &BasicJson<M>) -> JsonResult<Self> {
        match json {
            BasicJson::Boolean(flag) => Ok(*flag),
            _ => Err(JsonError::type_error(
                302,
                &format!("type must be boolean, but is {}", json.type_name()),
            )),
        }
    }
}

impl FromJson for String {
    /// type_error 302 for anything but a string.
    fn from_json<M: ObjectMap>(json: &BasicJson<M>) -> JsonResult<Self> {
        match json {
            BasicJson::String(text) => Ok(text.clone()),
            _ => Err(JsonError::type_error(
                302,
                &format!("type must be string, but is {}", json.type_name()),
            )),
        }
    }
}

impl<T: FromJson> FromJson for Vec<T> {
    /// `get<std::vector<T>>()`: type_error 302 for anything but an array, then each element read
    /// as T, the first that is not one failing the whole.
    fn from_json<M: ObjectMap>(json: &BasicJson<M>) -> JsonResult<Self> {
        match json {
            BasicJson::Array(items) => items.iter().map(T::from_json).collect(),
            _ => Err(JsonError::type_error(
                302,
                &format!("type must be array, but is {}", json.type_name()),
            )),
        }
    }
}

// ---- the tree the events describe ---------------------------------------------------------------

/// nlohmann's json_sax_dom_parser: the value the parse events describe, each value put where it
/// belongs as it is read - in the open array, under the open object's last key (replacing a value
/// that key already had, in its place), or as the root. Public so that a reader that watches the
/// events (nlohmann's parser callback in the C++) can wrap it in a [`Sax`] of its own and hand
/// each event on.
pub struct DomBuilder<M: ObjectMap> {
    root: BasicJson<M>,
    // What is open, innermost last.
    open: Vec<Open<M>>,
    error: Option<JsonError>,
}

struct Open<M: ObjectMap> {
    container: BasicJson<M>,
    // An object's key for the value being read.
    key: String,
}

impl<M: ObjectMap> Default for DomBuilder<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M: ObjectMap> DomBuilder<M> {
    pub fn new() -> Self {
        Self {
            root: BasicJson::Null,
            open: Vec::new(),
            error: None,
        }
    }

    /// The value read, once the parse has gone to its end.
    pub fn into_value(self) -> BasicJson<M> {
        self.root
    }

    /// The parse error that stopped the parse, if one did.
    pub fn into_error(self) -> Option<JsonError> {
        self.error
    }

    fn handle_value(&mut self, value: BasicJson<M>) {
        let Some(open) = self.open.last_mut() else {
            self.root = value;
            return;
        };
        match &mut open.container {
            BasicJson::Array(items) => items.push(value),
            BasicJson::Object(members) => {
                *members.entry(std::mem::take(&mut open.key)) = value;
            }
            // only an array or an object is ever open
            _ => {}
        }
    }

    fn open(&mut self, container: BasicJson<M>) -> bool {
        self.open.push(Open {
            container,
            key: String::new(),
        });
        true
    }

    fn close(&mut self) -> bool {
        if let Some(open) = self.open.pop() {
            self.handle_value(open.container);
        }
        true
    }
}

impl<M: ObjectMap> Sax for DomBuilder<M> {
    fn null(&mut self) -> bool {
        self.handle_value(BasicJson::Null);
        true
    }

    fn boolean(&mut self, value: bool) -> bool {
        self.handle_value(BasicJson::Boolean(value));
        true
    }

    fn number_integer(&mut self, value: i64) -> bool {
        self.handle_value(BasicJson::NumberInteger(value));
        true
    }

    fn number_unsigned(&mut self, value: u64) -> bool {
        self.handle_value(BasicJson::NumberUnsigned(value));
        true
    }

    fn number_float(&mut self, value: f64, _written: &str) -> bool {
        self.handle_value(BasicJson::NumberFloat(value));
        true
    }

    fn string(&mut self, value: &mut String) -> bool {
        self.handle_value(BasicJson::String(std::mem::take(value)));
        true
    }

    fn start_object(&mut self) -> bool {
        self.open(BasicJson::object())
    }

    fn key(&mut self, value: &mut String) -> bool {
        if let Some(open) = self.open.last_mut() {
            open.key = std::mem::take(value);
        }
        true
    }

    fn end_object(&mut self) -> bool {
        self.close()
    }

    fn start_array(&mut self) -> bool {
        self.open(BasicJson::array())
    }

    fn end_array(&mut self) -> bool {
        self.close()
    }

    fn parse_error(&mut self, _position: usize, _token: &str, error: &JsonError) -> bool {
        self.error = Some(error.clone());
        false
    }
}
