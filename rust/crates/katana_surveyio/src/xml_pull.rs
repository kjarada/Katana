//! A streaming (pull) XML tokenizer for survey files too large to hold as a
//! tree.
//!
//! Why not katana_core's xml reader, the one XML reader the project otherwise
//! insists on: it builds the whole document as nested nodes, which is the
//! right shape for a survey code file and the wrong one for a Trimble JobXML
//! job of a few hundred megabytes - every element becomes a heap node with a
//! string name and a vector of attributes, and the tree has to exist in full
//! before the first record can be read. This reader hands out one event at a
//! time as views into the caller's bytes and allocates nothing of its own.
//!
//! It keeps every REFUSAL core's xml keeps, for the reasons given there (they
//! are security properties of reading untrusted files, not omissions): a
//! document type declaration is refused; an entity reference other than the
//! five predefined ones is refused (append_xml_text); nesting deeper than
//! XML_MAX_DEPTH elements is refused. And it refuses what a truncated or
//! damaged file looks like: an end tag that does not match its start tag, text
//! or a second element outside the root, a construct that runs off the end of
//! the file.
//!
//! Attribute values and text come back RAW (undecoded); append_xml_text
//! decodes them when a caller wants the characters. Most values in a survey
//! file are numbers with no '&' in them, so decoding only on demand is the
//! difference between allocating per value and not.
//!
//! The document is bytes, as the C++ reads it: names, values and text are byte
//! views, and a reader that puts them into the survey model validates them as
//! UTF-8 there.

#![allow(
    dead_code,
    reason = "Its reader (trimble_jobxml) is ported in the next wave; remove this when it lands."
)]

use katana_core::xml::XML_MAX_DEPTH;
use katana_core::{Error, ErrorCode, Status};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Event {
    /// name(), depth() (1 = the root), attribute().
    StartElement,
    /// name(), depth() of the element being closed.
    EndElement,
    /// text(), raw; text_is_cdata() when it came from a CDATA section.
    Text,
    /// The root element closed and nothing but white space followed.
    EndOfDocument,
    /// error(); every later call returns Error again.
    Error,
}

fn is_space(c: u8) -> bool {
    c == b' ' || c == b'\t' || c == b'\r' || c == b'\n'
}

// XML's name rules, restricted to what can be checked a byte at a time: any
// byte of a multi-byte UTF-8 sequence is accepted, so a name in another script
// is read, and every ASCII byte is checked against the rule exactly. A table,
// because every byte of every tag name passes through here: a large job has
// tens of millions of them.
const NAME_START: u8 = 1;
const NAME_CHAR: u8 = 2;

const NAME_CLASS: [u8; 256] = {
    let mut table = [0_u8; 256];
    let mut c = 0;
    while c < 256 {
        let byte = c as u8;
        let start = byte.is_ascii_alphabetic() || byte == b'_' || byte == b':' || byte >= 0x80;
        let inside = start || byte.is_ascii_digit() || byte == b'-' || byte == b'.';
        table[c] = (if start { NAME_START } else { 0 }) | (if inside { NAME_CHAR } else { 0 });
        c += 1;
    }
    table
};

fn is_name_start(c: u8) -> bool {
    (NAME_CLASS[usize::from(c)] & NAME_START) != 0
}

fn is_name_char(c: u8) -> bool {
    (NAME_CLASS[usize::from(c)] & NAME_CHAR) != 0
}

fn all_space(text: &[u8]) -> bool {
    text.iter().all(|&c| is_space(c))
}

fn line_of(doc: &[u8], offset: usize) -> usize {
    let offset = offset.min(doc.len());
    1 + doc[..offset].iter().filter(|&&c| c == b'\n').count()
}

// The first `needle` at or after `from`.
fn find_bytes(doc: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    doc.get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|at| at + from)
}

// A name or value quoted in a message: the bytes as text, any that are not
// UTF-8 replaced, since a message is text.
fn shown(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn append_utf8(code: u32, out: &mut Vec<u8>) {
    if code < 0x80 {
        out.push(code as u8);
    } else if code < 0x800 {
        out.push((0xC0 | (code >> 6)) as u8);
        out.push((0x80 | (code & 0x3F)) as u8);
    } else if code < 0x10000 {
        out.push((0xE0 | (code >> 12)) as u8);
        out.push((0x80 | ((code >> 6) & 0x3F)) as u8);
        out.push((0x80 | (code & 0x3F)) as u8);
    } else {
        out.push((0xF0 | (code >> 18)) as u8);
        out.push((0x80 | ((code >> 12) & 0x3F)) as u8);
        out.push((0x80 | ((code >> 6) & 0x3F)) as u8);
        out.push((0x80 | (code & 0x3F)) as u8);
    }
}

const NO_NAME: &[u8] = &[];

pub(crate) struct XmlPullReader<'a> {
    doc: &'a [u8],
    pos: usize,
    open: [&'a [u8]; XML_MAX_DEPTH],
    depth: usize,
    root_seen: bool,
    root_closed: bool,
    pending_self_close: bool,
    failed: bool,

    name: &'a [u8],
    text: &'a [u8],
    /// The raw attribute region of the current start tag.
    attributes: &'a [u8],
    cdata: bool,
    text_has_reference: bool,
    event_depth: usize,
    event_offset: usize,
    error: Error,
}

impl<'a> XmlPullReader<'a> {
    /// `document` is not copied: it must outlive the reader.
    pub(crate) fn new(document: &'a [u8]) -> Self {
        Self {
            doc: document,
            pos: 0,
            open: [NO_NAME; XML_MAX_DEPTH],
            depth: 0,
            root_seen: false,
            root_closed: false,
            pending_self_close: false,
            failed: false,
            name: NO_NAME,
            text: NO_NAME,
            attributes: NO_NAME,
            cdata: false,
            text_has_reference: false,
            event_depth: 0,
            event_offset: 0,
            error: Error::default(),
        }
    }

    pub(crate) fn name(&self) -> &'a [u8] {
        self.name
    }

    #[allow(
        clippy::misnamed_getters,
        reason = "The C++ depth() is the depth of the current event (eventDepth_), not the running nesting depth."
    )]
    pub(crate) fn depth(&self) -> usize {
        self.event_depth
    }

    pub(crate) fn text(&self) -> &'a [u8] {
        self.text
    }

    pub(crate) fn text_is_cdata(&self) -> bool {
        self.cdata
    }

    /// True when the current Text holds an '&' (always false for CDATA):
    /// noted while the text is scanned, so a caller need not scan it again to
    /// know whether append_xml_text has anything to decode.
    pub(crate) fn text_has_reference(&self) -> bool {
        self.text_has_reference
    }

    /// Byte offset in the document at which the current event began.
    pub(crate) fn offset(&self) -> usize {
        self.event_offset
    }

    /// False when the current start element has no attributes at all, which
    /// is most of them: a caller can skip looking for any.
    pub(crate) fn has_attributes(&self) -> bool {
        !self.attributes.is_empty()
    }

    pub(crate) fn error(&self) -> &Error {
        &self.error
    }

    fn fail(&mut self, at: usize, message: String) -> Event {
        self.failed = true;
        self.error = Error::new(
            ErrorCode::ParseFailure,
            format!(
                "the XML is damaged or incomplete at line {}: {message}",
                line_of(self.doc, at)
            ),
        );
        Event::Error
    }

    /// The RAW value of attribute `attribute_name` on the current start
    /// element; None when there is none.
    pub(crate) fn attribute(&self, attribute_name: &[u8]) -> Option<&'a [u8]> {
        // The region was checked when the tag was read, so this walk can trust
        // its shape: name, optional space, '=', optional space, quoted value.
        let region = self.attributes;
        let mut p = 0;
        while p < region.len() {
            while p < region.len() && is_space(region[p]) {
                p += 1;
            }
            let name_start = p;
            while p < region.len() && is_name_char(region[p]) {
                p += 1;
            }
            let found = &region[name_start..p];
            while p < region.len() && region[p] != b'"' && region[p] != b'\'' {
                p += 1;
            }
            if p >= region.len() {
                return None;
            }
            let quote = region[p];
            let value_start = p + 1;
            let close = value_start + region[value_start..].iter().position(|&c| c == quote)?;
            if found == attribute_name {
                return Some(&region[value_start..close]);
            }
            p = close + 1;
        }
        None
    }

    fn read_start_tag(&mut self, tag_start: usize) -> Event {
        let doc = self.doc;
        let size = doc.len();
        let mut p = tag_start + 1;
        if p >= size || !is_name_start(doc[p]) {
            return self.fail(tag_start, String::from("a '<' that does not begin a tag"));
        }
        let name_start = p;
        while p < size && is_name_char(doc[p]) {
            p += 1;
        }
        let tag_name = &doc[name_start..p];
        let attributes_start = p;
        let attributes_end;
        let self_closing;
        loop {
            let spaced = p < size && is_space(doc[p]);
            while p < size && is_space(doc[p]) {
                p += 1;
            }
            if p >= size {
                return self.fail(
                    tag_start,
                    format!("the tag <{}> is not closed", shown(tag_name)),
                );
            }
            let c = doc[p];
            if c == b'>' {
                attributes_end = p;
                self_closing = false;
                p += 1;
                break;
            }
            if c == b'/' {
                if p + 1 < size && doc[p + 1] == b'>' {
                    attributes_end = p;
                    self_closing = true;
                    p += 2;
                    break;
                }
                return self.fail(p, format!("a '/' inside the tag <{}>", shown(tag_name)));
            }
            if !spaced || !is_name_start(c) {
                return self.fail(
                    p,
                    format!("an unexpected character in the tag <{}>", shown(tag_name)),
                );
            }
            while p < size && is_name_char(doc[p]) {
                p += 1;
            }
            while p < size && is_space(doc[p]) {
                p += 1;
            }
            if p >= size || doc[p] != b'=' {
                return self.fail(
                    p,
                    format!("an attribute with no value in <{}>", shown(tag_name)),
                );
            }
            p += 1;
            while p < size && is_space(doc[p]) {
                p += 1;
            }
            if p >= size || (doc[p] != b'"' && doc[p] != b'\'') {
                return self.fail(
                    p,
                    format!(
                        "an attribute value that is not quoted in <{}>",
                        shown(tag_name)
                    ),
                );
            }
            let quote = doc[p];
            let Some(close) = doc[p + 1..]
                .iter()
                .position(|&c| c == quote)
                .map(|at| at + p + 1)
            else {
                return self.fail(
                    p,
                    format!(
                        "an attribute value that is never closed in <{}>",
                        shown(tag_name)
                    ),
                );
            };
            if doc[p + 1..close].contains(&b'<') {
                return self.fail(
                    p,
                    format!("a '<' inside an attribute value in <{}>", shown(tag_name)),
                );
            }
            p = close + 1;
        }

        if self.root_closed {
            return self.fail(
                tag_start,
                format!("a second root element <{}>", shown(tag_name)),
            );
        }
        if self.depth >= self.open.len() {
            return self.fail(
                tag_start,
                format!(
                    "elements nested deeper than {} levels, which is refused",
                    self.open.len()
                ),
            );
        }
        self.root_seen = true;
        self.pos = p;
        self.name = tag_name;
        self.attributes = &doc[attributes_start..attributes_end];
        self.event_offset = tag_start;
        self.event_depth = self.depth + 1;
        if self_closing {
            self.pending_self_close = true;
            if self.depth == 0 {
                self.root_closed = true;
            }
        } else {
            self.open[self.depth] = tag_name;
            self.depth += 1;
        }
        Event::StartElement
    }

    fn read_end_tag(&mut self, tag_start: usize) -> Event {
        let doc = self.doc;
        let size = doc.len();
        let mut p = tag_start + 2;
        let name_start = p;
        while p < size && is_name_char(doc[p]) {
            p += 1;
        }
        let tag_name = &doc[name_start..p];
        while p < size && is_space(doc[p]) {
            p += 1;
        }
        if p >= size || doc[p] != b'>' {
            return self.fail(
                tag_start,
                format!("the end tag </{}> is not closed", shown(tag_name)),
            );
        }
        if self.depth == 0 {
            return self.fail(
                tag_start,
                format!("the end tag </{}> closes nothing", shown(tag_name)),
            );
        }
        let expected = self.open[self.depth - 1];
        if !same_text(expected, tag_name) {
            return self.fail(
                tag_start,
                format!(
                    "</{}> where </{}> was expected",
                    shown(tag_name),
                    shown(expected)
                ),
            );
        }
        self.pos = p + 1;
        self.name = tag_name;
        self.event_offset = tag_start;
        self.event_depth = self.depth;
        self.depth -= 1;
        if self.depth == 0 {
            self.root_closed = true;
        }
        Event::EndElement
    }

    fn read_markup(&mut self) -> Event {
        // Called with doc[pos] == '<'. Returns Text for a CDATA section, the
        // tag's event for a tag, and EndOfDocument as "nothing to report, carry
        // on" for comments and processing instructions - next() loops on it.
        let doc = self.doc;
        let tag_start = self.pos;
        // Start and end tags are nearly every piece of markup in a large file,
        // so they are recognised by their second byte before anything rarer is
        // looked for.
        if let Some(&second) = doc.get(self.pos + 1) {
            if is_name_start(second) {
                return self.read_start_tag(tag_start);
            }
            if second == b'/' {
                return self.read_end_tag(tag_start);
            }
        }
        let rest = &doc[self.pos..];
        if rest.starts_with(b"<!--") {
            let Some(close) = find_bytes(doc, b"-->", self.pos + 4) else {
                return self.fail(tag_start, String::from("a comment that is never closed"));
            };
            self.pos = close + 3;
            return Event::EndOfDocument;
        }
        if rest.starts_with(b"<![CDATA[") {
            let Some(close) = find_bytes(doc, b"]]>", self.pos + 9) else {
                return self.fail(
                    tag_start,
                    String::from("a CDATA section that is never closed"),
                );
            };
            if self.depth == 0 {
                return self.fail(
                    tag_start,
                    String::from("a CDATA section outside the root element"),
                );
            }
            self.text = &doc[self.pos + 9..close];
            self.cdata = true;
            self.text_has_reference = false;
            self.event_offset = tag_start;
            self.event_depth = self.depth;
            self.pos = close + 3;
            return Event::Text;
        }
        if rest.starts_with(b"<!DOCTYPE") {
            return self.fail(
                tag_start,
                String::from(
                    "a document type declaration (DOCTYPE), which is refused because it can \
                     change what the rest of the file means",
                ),
            );
        }
        if rest.starts_with(b"<!") {
            return self.fail(
                tag_start,
                String::from("a '<!' declaration this reader does not accept"),
            );
        }
        if rest.starts_with(b"<?") {
            let Some(close) = find_bytes(doc, b"?>", self.pos + 2) else {
                return self.fail(
                    tag_start,
                    String::from("a processing instruction that is never closed"),
                );
            };
            self.pos = close + 2;
            return Event::EndOfDocument;
        }
        if rest.starts_with(b"</") {
            return self.read_end_tag(tag_start);
        }
        self.read_start_tag(tag_start)
    }

    pub(crate) fn next(&mut self) -> Event {
        if self.failed {
            return Event::Error;
        }
        if self.pending_self_close {
            // name and event_depth still describe the self-closed element.
            self.pending_self_close = false;
            return Event::EndElement;
        }
        let doc = self.doc;
        loop {
            if self.pos >= doc.len() {
                if self.depth > 0 {
                    let inside = shown(self.open[self.depth - 1]);
                    return self.fail(
                        self.pos,
                        format!("the file ends inside <{inside}>, so it is incomplete"),
                    );
                }
                if !self.root_seen {
                    return self.fail(self.pos, String::from("there is no root element"));
                }
                return Event::EndOfDocument;
            }
            if doc[self.pos] == b'<' {
                let event = self.read_markup();
                if event == Event::EndOfDocument {
                    continue; // a comment or processing instruction
                }
                return event;
            }
            // Text runs between tags are short - a value or the indentation
            // before the next tag - so they are walked a byte at a time, which
            // also notes an '&' on the way, rather than searched for twice.
            let start = self.pos;
            let mut reference = false;
            let mut p = self.pos;
            while p < doc.len() && doc[p] != b'<' {
                reference = reference || doc[p] == b'&';
                p += 1;
            }
            self.pos = p;
            let raw = &doc[start..p];
            if self.depth == 0 {
                if !all_space(raw) {
                    return self.fail(start, String::from("text outside the root element"));
                }
                continue;
            }
            self.text = raw;
            self.cdata = false;
            self.text_has_reference = reference;
            self.event_offset = start;
            self.event_depth = self.depth;
            return Event::Text;
        }
    }
}

/// Byte-wise equality for the short names and values of a survey file, kept
/// as the C++ keeps it: there string_view's == is a call to the C library's
/// memcmp, which for the few bytes of a tag name costs more than the
/// comparison, and a large job makes millions of them.
pub(crate) fn same_text(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x == y)
}

// A character reference's digits as from_chars reads them into a 32-bit
// unsigned: every byte a digit of `radix`, no sign, nothing left over, and no
// overflow.
fn character_code(digits: &[u8], radix: u32) -> Option<u32> {
    if digits.is_empty() {
        return None;
    }
    let mut code: u32 = 0;
    for &digit in digits {
        let value = char::from(digit).to_digit(radix)?;
        code = code.checked_mul(radix)?.checked_add(value)?;
    }
    Some(code)
}

/// Appends `raw` to `out` with the five predefined entities and character
/// references (&#65; &#x41;) replaced by the characters they name. Anything
/// else after an '&' is refused (ParseFailure), for the reason given above.
pub(crate) fn append_xml_text(raw: &[u8], out: &mut Vec<u8>) -> Status {
    let mut p = 0;
    while p < raw.len() {
        let Some(amp) = raw[p..].iter().position(|&c| c == b'&').map(|at| at + p) else {
            out.extend_from_slice(&raw[p..]);
            break;
        };
        out.extend_from_slice(&raw[p..amp]);
        let semi = raw[amp + 1..]
            .iter()
            .position(|&c| c == b';')
            .map(|at| at + amp + 1);
        // The longest reference that can be valid is "&#x10FFFF;".
        let Some(semi) = semi.filter(|&semi| semi - amp <= 10) else {
            return Err(Error::new(
                ErrorCode::ParseFailure,
                "an '&' that does not begin an entity reference",
            )
            .with_context(shown(&raw[amp..amp + (raw.len() - amp).min(12)])));
        };
        let entity = &raw[amp + 1..semi];
        match entity {
            b"lt" => out.push(b'<'),
            b"gt" => out.push(b'>'),
            b"amp" => out.push(b'&'),
            b"quot" => out.push(b'"'),
            b"apos" => out.push(b'\''),
            [b'#', second, ..] => {
                let hex = *second == b'x';
                let digits = &entity[if hex { 2 } else { 1 }..];
                let code = character_code(digits, if hex { 16 } else { 10 });
                let Some(code) = code.filter(|&code| {
                    code != 0 && code <= 0x10FFFF && !(0xD800..=0xDFFF).contains(&code)
                }) else {
                    return Err(Error::new(
                        ErrorCode::ParseFailure,
                        "a character reference that names no character",
                    )
                    .with_context(format!("&{};", shown(entity))));
                };
                append_utf8(code, out);
            }
            _ => {
                return Err(Error::new(
                    ErrorCode::ParseFailure,
                    "an entity reference this reader does not define, which is refused",
                )
                .with_context(format!("&{};", shown(entity))));
            }
        }
        p = semi + 1;
    }
    Ok(())
}

/// 1-based line number of byte offsets met in increasing order, counted
/// incrementally so a reader that asks once per record pays for each byte
/// once. Asking for an earlier offset than last time recounts from the start.
pub(crate) struct LineCounter<'a> {
    doc: &'a [u8],
    counted_to: usize,
    line: usize,
}

impl<'a> LineCounter<'a> {
    pub(crate) fn new(document: &'a [u8]) -> Self {
        Self {
            doc: document,
            counted_to: 0,
            line: 1,
        }
    }

    pub(crate) fn line_at(&mut self, offset: usize) -> usize {
        let offset = offset.min(self.doc.len());
        if offset < self.counted_to {
            self.counted_to = 0;
            self.line = 1;
        }
        // Eight bytes at a time: a byte of x is zero exactly where the text has
        // a line feed, and the classic zero-byte test marks each such byte's
        // top bit without carrying into its neighbours. A reader asks once per
        // record, so this runs over the whole file.
        const ONES: u64 = 0x0101010101010101;
        const LOW7: u64 = 0x7F7F7F7F7F7F7F7F;
        let (words, rest) = self.doc[self.counted_to..offset].as_chunks::<8>();
        let mut lines = 0usize;
        for word in words {
            let x = u64::from_ne_bytes(*word) ^ (ONES * u64::from(b'\n'));
            let zero_bytes = !(((x & LOW7) + LOW7) | x | LOW7);
            lines += zero_bytes.count_ones() as usize;
        }
        lines += rest.iter().filter(|&&c| c == b'\n').count();
        self.line += lines;
        self.counted_to = offset;
        self.line
    }
}
