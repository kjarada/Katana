//! An ISO 10303-21 file read into instances: the parse, with no knowledge of
//! any schema. Internal to katana_ifc; import.rs gives it meaning.
//!
//! The file is untrusted input, so what it may ask for is bounded: lists
//! nest at most MAXIMUM_DEPTH deep, a reference or an instance number must
//! fit in 32 bits, and a structure the parser cannot follow is a
//! ParseFailure naming the line rather than a guess - a guess would shift
//! every attribute after it.

use std::collections::HashMap;

use katana_core::error::{Error, ErrorCode, Result, Status, make_error};
use katana_core::text::parse_finite_double;

use crate::step::decode_step_string;

pub(crate) const MAXIMUM_DEPTH: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum StepKind {
    /// $
    #[default]
    Null,
    /// *
    Derived,
    Integer,
    Real,
    /// decoded to UTF-8
    String,
    /// .VALUE., without the dots; .T. and .F. are T and F
    Enumeration,
    /// #123
    Reference,
    List,
    /// IFCLABEL('x'): `text` the type, `items` the one value
    Typed,
    /// "0FF": kept as written
    Binary,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct StepValue {
    pub(crate) kind: StepKind,
    pub(crate) integer: i64,
    pub(crate) real: f64,
    pub(crate) reference: u32,
    pub(crate) text: String,
    pub(crate) items: Vec<StepValue>,
}

impl StepValue {
    /// A number, from an Integer, a Real, or a Typed wrapping one.
    pub(crate) fn number(&self) -> Option<f64> {
        match self.kind {
            StepKind::Integer => Some(self.integer as f64),
            StepKind::Real => Some(self.real),
            StepKind::Typed => self.items.first().and_then(StepValue::number),
            _ => None,
        }
    }

    /// The text of a String, an Enumeration, or a Typed wrapping one.
    pub(crate) fn string(&self) -> Option<&String> {
        match self.kind {
            StepKind::String | StepKind::Enumeration => Some(&self.text),
            StepKind::Typed => self.items.first().and_then(StepValue::string),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct StepInstance {
    /// upper case, as written
    pub(crate) entity_type: String,
    pub(crate) arguments: Vec<StepValue>,
    pub(crate) line: usize,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct StepModel {
    /// FILE_SCHEMA's first name: "IFC4X3_ADD2"
    pub(crate) schema: String,
    /// What FILE_DESCRIPTION names; read, as the C++ reads it, and asked of
    /// by nothing yet.
    #[allow(
        dead_code,
        reason = "Read from the header as the C++ reads it; no caller asks for it yet."
    )]
    pub(crate) view_definitions: Vec<String>,
    /// By number: looked up, never walked (`order` is the file's order).
    pub(crate) instances: HashMap<u32, StepInstance>,
    /// instance numbers in file order
    pub(crate) order: Vec<u32>,
    /// #1=(A()B()); read past, not modelled
    pub(crate) complex_instances: usize,
}

impl StepModel {
    pub(crate) fn find(&self, id: u32) -> Option<&StepInstance> {
        self.instances.get(&id)
    }

    /// The instance `value` refers to, or None.
    pub(crate) fn follow(&self, value: &StepValue) -> Option<&StepInstance> {
        if value.kind == StepKind::Reference {
            self.find(value.reference)
        } else {
            None
        }
    }
}

fn is_identifier_start(c: u8) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_lowercase() || c == b'_'
}

fn is_identifier_part(c: u8) -> bool {
    is_identifier_start(c) || c.is_ascii_digit() || c == b'-'
}

struct Parser<'t> {
    text: &'t [u8],
    at: usize,
    line: usize,
    model: StepModel,
}

impl<'t> Parser<'t> {
    fn new(text: &'t [u8]) -> Self {
        Self {
            text,
            at: 0,
            line: 1,
            model: StepModel::default(),
        }
    }

    fn run(mut self) -> Result<StepModel> {
        if !self.skip_to(b"ISO-10303-21") {
            return Err(self.fail("not an ISO 10303-21 file: it does not begin ISO-10303-21;"));
        }
        self.header()?;
        loop {
            self.skip_space();
            if self.at >= self.text.len() {
                return Err(self.fail("the file ends before END-ISO-10303-21"));
            }
            if self.consume_word(b"END-ISO-10303-21") {
                break;
            }
            if !self.consume_word(b"DATA") {
                return Err(self.fail("expected DATA; or END-ISO-10303-21;"));
            }
            self.skip_space();
            // DATA may carry a section name and schema in parentheses (the
            // standard's second edition); they are read past.
            if self.peek() == b'(' {
                self.at += 1;
                self.list(0)?;
                self.skip_space();
            }
            if !self.consume(b';') {
                return Err(self.fail("expected ; after DATA"));
            }
            self.data()?;
        }
        Ok(self.model)
    }

    fn peek(&self) -> u8 {
        self.text.get(self.at).copied().unwrap_or(0)
    }

    fn fail(&self, message: &str) -> Error {
        make_error(
            ErrorCode::ParseFailure,
            message,
            format!("line {}", self.line),
        )
    }

    fn advance(&mut self) {
        if self.text[self.at] == b'\n' {
            self.line += 1;
        }
        self.at += 1;
    }

    fn skip_space(&mut self) {
        while self.at < self.text.len() {
            let c = self.text[self.at];
            if c == b' ' || c == b'\t' || c == b'\r' || c == b'\n' {
                self.advance();
            } else if c == b'/' && self.at + 1 < self.text.len() && self.text[self.at + 1] == b'*' {
                self.at += 2;
                while self.at < self.text.len()
                    && !(self.text[self.at] == b'*'
                        && self.at + 1 < self.text.len()
                        && self.text[self.at + 1] == b'/')
                {
                    self.advance();
                }
                self.at = (self.at + 2).min(self.text.len());
            } else {
                break;
            }
        }
    }

    fn consume(&mut self, c: u8) -> bool {
        self.skip_space();
        if self.peek() == c {
            self.at += 1;
            return true;
        }
        false
    }

    /// `word`, then a ';' - HEADER;, ENDSEC;, DATA is left without its ';'.
    fn consume_word(&mut self, word: &[u8]) -> bool {
        self.skip_space();
        if !self.text[self.at..].starts_with(word) {
            return false;
        }
        let after = self.at + word.len();
        let data = word == b"DATA";
        if after < self.text.len() && is_identifier_part(self.text[after]) && !data {
            return false;
        }
        self.at = after;
        if !data {
            self.skip_space();
            if self.peek() == b';' {
                self.at += 1;
            }
        }
        true
    }

    fn skip_to(&mut self, word: &[u8]) -> bool {
        self.skip_space();
        // A byte order mark some writers put first.
        if self.text[self.at..].starts_with(b"\xEF\xBB\xBF") {
            self.at += 3;
        }
        self.skip_space();
        if !self.text[self.at..].starts_with(word) {
            return false;
        }
        self.at += word.len();
        self.consume(b';')
    }

    fn identifier(&mut self) -> String {
        self.skip_space();
        let mut out = String::new();
        while self.at < self.text.len() && is_identifier_part(self.text[self.at]) {
            out.push(char::from(self.text[self.at].to_ascii_uppercase()));
            self.at += 1;
        }
        out
    }

    fn header(&mut self) -> Status {
        if !self.consume_word(b"HEADER") {
            return Err(self.fail("expected HEADER;"));
        }
        loop {
            self.skip_space();
            if self.consume_word(b"ENDSEC") {
                return Ok(());
            }
            let name = self.identifier();
            if name.is_empty() {
                return Err(self.fail("expected a header entity or ENDSEC;"));
            }
            if !self.consume(b'(') {
                return Err(self.fail(&format!("expected ( after {name}")));
            }
            let arguments = self.list(0)?;
            if !self.consume(b';') {
                return Err(self.fail(&format!("expected ; after {name}")));
            }
            let first = arguments
                .first()
                .filter(|first| first.kind == StepKind::List);
            if name == "FILE_SCHEMA"
                && let Some(schema) = first
                    .and_then(|first| first.items.first())
                    .and_then(StepValue::string)
            {
                self.model.schema = schema.clone();
            } else if name == "FILE_DESCRIPTION"
                && let Some(first) = first
            {
                for item in &first.items {
                    if let Some(text) = item.string() {
                        self.model.view_definitions.push(text.clone());
                    }
                }
            }
        }
    }

    /// The digits at the cursor, as an unsigned number; None when there are
    /// none or they overflow, as `std::from_chars` refuses them.
    fn digits(&mut self) -> Option<u64> {
        let start = self.at;
        while self.at < self.text.len() && self.text[self.at].is_ascii_digit() {
            self.at += 1;
        }
        std::str::from_utf8(&self.text[start..self.at])
            .ok()?
            .parse::<u64>()
            .ok()
    }

    fn data(&mut self) -> Status {
        loop {
            self.skip_space();
            if self.consume_word(b"ENDSEC") {
                return Ok(());
            }
            if !self.consume(b'#') {
                return Err(self.fail("expected an instance (#n=...) or ENDSEC;"));
            }
            let Some(id) = self
                .digits()
                .filter(|&id| id != 0 && id <= u64::from(u32::MAX))
            else {
                return Err(self.fail("an instance number is missing or out of range"));
            };
            if !self.consume(b'=') {
                return Err(self.fail(&format!("expected = after #{id}")));
            }
            self.skip_space();
            let mut instance = StepInstance {
                line: self.line,
                ..StepInstance::default()
            };
            if self.peek() == b'(' {
                // A complex instance: its partial types are read and dropped.
                self.at += 1;
                loop {
                    self.skip_space();
                    if self.consume(b')') {
                        break;
                    }
                    let part = self.identifier();
                    if part.is_empty() || !self.consume(b'(') {
                        return Err(self.fail(&format!(
                            "a complex instance #{id} is not a list of partial types"
                        )));
                    }
                    self.list(0)?;
                }
                self.model.complex_instances += 1;
            } else {
                instance.entity_type = self.identifier();
                if instance.entity_type.is_empty() || !self.consume(b'(') {
                    return Err(self.fail(&format!("expected an entity name and ( for #{id}")));
                }
                instance.arguments = self.list(0)?;
            }
            if !self.consume(b';') {
                return Err(self.fail(&format!("expected ; to end #{id}")));
            }
            let id32 = id as u32;
            if !instance.entity_type.is_empty() {
                if self.model.instances.contains_key(&id32) {
                    return Err(self.fail(&format!("#{id} is defined twice")));
                }
                self.model.instances.insert(id32, instance);
                self.model.order.push(id32);
            }
        }
    }

    /// The values up to and including the ')' closing a list whose '(' has
    /// been consumed.
    fn list(&mut self, depth: usize) -> Result<Vec<StepValue>> {
        if depth > MAXIMUM_DEPTH {
            return Err(self.fail(&format!("lists nest more than {MAXIMUM_DEPTH} deep")));
        }
        let mut items = Vec::new();
        self.skip_space();
        if self.consume(b')') {
            return Ok(items);
        }
        loop {
            let item = self.value(depth)?;
            items.push(item);
            self.skip_space();
            if self.consume(b',') {
                continue;
            }
            if self.consume(b')') {
                return Ok(items);
            }
            return Err(self.fail("expected , or ) in a list"));
        }
    }

    fn value(&mut self, depth: usize) -> Result<StepValue> {
        self.skip_space();
        let mut out = StepValue::default();
        let c = self.peek();
        if c == b'$' {
            self.at += 1;
            return Ok(out);
        }
        if c == b'*' {
            self.at += 1;
            out.kind = StepKind::Derived;
            return Ok(out);
        }
        if c == b'#' {
            self.at += 1;
            let Some(id) = self.digits().filter(|&id| id <= u64::from(u32::MAX)) else {
                return Err(self.fail("a reference is missing its number or is out of range"));
            };
            out.kind = StepKind::Reference;
            out.reference = id as u32;
            return Ok(out);
        }
        if c == b'\'' {
            self.at += 1;
            let start = self.at;
            loop {
                if self.at >= self.text.len() {
                    return Err(self.fail("a string is never closed"));
                }
                if self.text[self.at] == b'\'' {
                    if self.at + 1 < self.text.len() && self.text[self.at + 1] == b'\'' {
                        self.at += 2;
                        continue;
                    }
                    break;
                }
                self.advance();
            }
            out.kind = StepKind::String;
            out.text = decode_step_string(&self.text[start..self.at]);
            self.at += 1;
            return Ok(out);
        }
        if c == b'"' {
            self.at += 1;
            let start = self.at;
            while self.at < self.text.len() && self.text[self.at] != b'"' {
                self.advance();
            }
            if self.at >= self.text.len() {
                return Err(self.fail("a binary value is never closed"));
            }
            out.kind = StepKind::Binary;
            // Hexadecimal digits in a well-formed file; any other byte is
            // the replacement character (the C++ keeps the bytes).
            out.text = String::from_utf8_lossy(&self.text[start..self.at]).into_owned();
            self.at += 1;
            return Ok(out);
        }
        if c == b'(' {
            self.at += 1;
            out.kind = StepKind::List;
            out.items = self.list(depth + 1)?;
            return Ok(out);
        }
        if c == b'.' && !(self.at + 1 < self.text.len() && self.text[self.at + 1].is_ascii_digit())
        {
            self.at += 1;
            let start = self.at;
            while self.at < self.text.len() && self.text[self.at] != b'.' {
                if !is_identifier_part(self.text[self.at]) {
                    return Err(self.fail("an enumeration value is malformed"));
                }
                self.at += 1;
            }
            if self.at >= self.text.len() {
                return Err(self.fail("an enumeration value is never closed"));
            }
            out.kind = StepKind::Enumeration;
            // Identifier characters: ASCII.
            out.text = String::from_utf8_lossy(&self.text[start..self.at]).into_owned();
            self.at += 1;
            return Ok(out);
        }
        if c.is_ascii_digit() || c == b'-' || c == b'+' || c == b'.' {
            let start = self.at;
            let mut real = false;
            if c == b'-' || c == b'+' {
                self.at += 1;
            }
            while self.at < self.text.len() {
                let d = self.text[self.at];
                if d.is_ascii_digit() {
                    self.at += 1;
                } else if d == b'.' || d == b'E' || d == b'e' {
                    real = true;
                    self.at += 1;
                    if (d == b'E' || d == b'e')
                        && self.at < self.text.len()
                        && (self.text[self.at] == b'-' || self.text[self.at] == b'+')
                    {
                        self.at += 1;
                    }
                } else {
                    break;
                }
            }
            let mut token = &self.text[start..self.at];
            if let Some(rest) = token.strip_prefix(b"+") {
                token = rest;
            }
            // Digits, signs, points and E: ASCII.
            let token = String::from_utf8_lossy(token).into_owned();
            if !real
                && let Ok(integer) = token.parse::<i64>()
                && !token.starts_with('+')
            {
                out.kind = StepKind::Integer;
                out.integer = integer;
                return Ok(out);
            }
            // "1." and "1.E-05" are how STEP writes reals; a point with no
            // digits after it says nothing, and dropping it changes no value
            // while keeping the text in the form every parser reads.
            let mut number = token.clone();
            if let Some(dot) = number.find('.') {
                let next = number.as_bytes().get(dot + 1).copied();
                if next.is_none() || next == Some(b'E') || next == Some(b'e') {
                    number.remove(dot);
                }
            }
            // std::from_chars' refusals: no leading '+', nothing past the
            // number, no infinity from an overflow and no zero from an
            // underflow (parse_finite_double refuses both, as from_chars'
            // result_out_of_range does).
            match parse_finite_double(&number).filter(|_| !number.starts_with('+')) {
                Some(value) => {
                    out.kind = StepKind::Real;
                    out.real = value;
                    return Ok(out);
                }
                None => return Err(self.fail(&format!("\"{token}\" is not a number"))),
            }
        }
        if is_identifier_start(c) {
            out.kind = StepKind::Typed;
            out.text = self.identifier();
            if !self.consume(b'(') {
                return Err(self.fail(&format!("expected ( after the type {}", out.text)));
            }
            out.items = self.list(depth + 1)?;
            return Ok(out);
        }
        // The C++ writes the byte itself into the message; a byte past ASCII
        // is not text, and is the replacement character here.
        let shown = if c.is_ascii() {
            char::from(c)
        } else {
            '\u{FFFD}'
        };
        Err(self.fail(&format!("unexpected character '{shown}'")))
    }
}

pub(crate) fn parse_step(text: &[u8]) -> Result<StepModel> {
    Parser::new(text).run()
}
