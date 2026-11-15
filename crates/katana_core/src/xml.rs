use crate::text::trimmed;
use crate::text_encoding::utf8_or_windows1252;
use crate::{Error, ErrorCode, Result};

pub const XML_MAX_DEPTH: usize = 64;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct XmlNode {
    pub name: String,
    pub attributes: Vec<(String, Vec<u8>)>,
    pub text: Vec<u8>,
    pub children: Vec<XmlNode>,
}

impl XmlNode {
    pub fn child(&self, child_name: &str) -> Option<&XmlNode> {
        self.children.iter().find(|child| child.name == child_name)
    }

    pub fn child_text(&self, child_name: &str) -> &[u8] {
        self.child(child_name)
            .map_or(b"", |child| child.text.as_slice())
    }

    pub fn has(&self, child_name: &str) -> bool {
        self.child(child_name).is_some()
    }

    pub fn children_named(&self, child_name: &str) -> Vec<&XmlNode> {
        self.children
            .iter()
            .filter(|child| child.name == child_name)
            .collect()
    }
}

fn is_name_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || matches!(byte, b'_' | b':')
}

fn is_name_char(byte: u8) -> bool {
    is_name_start(byte) || byte.is_ascii_digit() || matches!(byte, b'-' | b'.')
}

fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

struct Parser<'a> {
    text: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn fail(&self, message: impl Into<String>) -> Error {
        let line = 1 + self.text[..self.at.min(self.text.len())]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count();
        Error::new(ErrorCode::InvalidArgument, message).with_context(format!("line {line}"))
    }

    fn starts_with(&self, token: &[u8]) -> bool {
        self.text[self.at..].starts_with(token)
    }

    fn find(&self, start: usize, token: &[u8]) -> Option<usize> {
        self.text[start..]
            .windows(token.len())
            .position(|window| window == token)
            .map(|at| start + at)
    }

    fn skip_space(&mut self) {
        while self.at < self.text.len() && is_space(self.text[self.at]) {
            self.at += 1;
        }
    }

    fn skip_trivia(&mut self) -> Result<()> {
        loop {
            self.skip_space();
            if self.starts_with(b"<!--") {
                let end = self
                    .find(self.at + 4, b"-->")
                    .ok_or_else(|| self.fail("a comment is never closed"))?;
                self.at = end + 3;
            } else if self.starts_with(b"<?") {
                let end = self
                    .find(self.at + 2, b"?>")
                    .ok_or_else(|| self.fail("a processing instruction is never closed"))?;
                self.at = end + 2;
            } else {
                return Ok(());
            }
        }
    }

    fn run(mut self) -> Result<XmlNode> {
        self.skip_trivia()?;
        if self.starts_with(b"<!DOCTYPE") {
            return Err(self.fail("a document type declaration is not read"));
        }
        if self.text.get(self.at) != Some(&b'<') {
            return Err(self.fail("there is no element here"));
        }
        self.read_element(0)
    }

    fn read_name(&mut self) -> String {
        let start = self.at;
        if self
            .text
            .get(self.at)
            .is_some_and(|byte| is_name_start(*byte))
        {
            self.at += 1;
            while self
                .text
                .get(self.at)
                .is_some_and(|byte| is_name_char(*byte))
            {
                self.at += 1;
            }
        }
        String::from_utf8(self.text[start..self.at].to_vec())
            .expect("XML names contain ASCII bytes")
    }

    fn append_resolved(&self, raw: &[u8], out: &mut Vec<u8>) -> Result<()> {
        let mut index = 0;
        while index < raw.len() {
            if raw[index] != b'&' {
                out.push(raw[index]);
                index += 1;
                continue;
            }
            let end = raw[index..]
                .iter()
                .position(|byte| *byte == b';')
                .map(|offset| index + offset)
                .ok_or_else(|| self.fail("an ampersand is not the start of an entity reference"))?;
            let name = &raw[index + 1..end];
            match name {
                b"amp" => out.push(b'&'),
                b"lt" => out.push(b'<'),
                b"gt" => out.push(b'>'),
                b"quot" => out.push(b'"'),
                b"apos" => out.push(b'\''),
                _ if name.len() > 1 && name[0] == b'#' => {
                    let hex = matches!(name[1], b'x' | b'X');
                    let digits = &name[if hex { 2 } else { 1 }..];
                    let valid_digits = !digits.is_empty()
                        && digits.iter().all(|byte| {
                            if hex {
                                byte.is_ascii_hexdigit()
                            } else {
                                byte.is_ascii_digit()
                            }
                        });
                    let code = valid_digits
                        .then(|| std::str::from_utf8(digits).ok())
                        .flatten()
                        .and_then(|digits| {
                            u32::from_str_radix(digits, if hex { 16 } else { 10 }).ok()
                        })
                        .filter(|code| *code <= 0x10ffff)
                        .ok_or_else(|| {
                            self.fail(format!(
                                "\"&{};\" is not a character",
                                utf8_or_windows1252(name)
                            ))
                        })?;
                    append_utf8(code, out);
                }
                _ => {
                    return Err(self.fail(format!(
                        "\"&{};\" is an entity this does not know",
                        utf8_or_windows1252(name)
                    )));
                }
            }
            index = end + 1;
        }
        Ok(())
    }

    fn read_attributes(&mut self, node: &mut XmlNode) -> Result<bool> {
        loop {
            self.skip_space();
            if self.at >= self.text.len() {
                return Err(self.fail(format!("element \"{}\" is never closed", node.name)));
            }
            if self.text[self.at] == b'>' {
                self.at += 1;
                return Ok(false);
            }
            if self.starts_with(b"/>") {
                self.at += 2;
                return Ok(true);
            }
            let name = self.read_name();
            if name.is_empty() {
                return Err(self.fail(format!(
                    "element \"{}\" has something that is not an attribute",
                    node.name
                )));
            }
            self.skip_space();
            if self.text.get(self.at) != Some(&b'=') {
                return Err(self.fail(format!("attribute \"{name}\" has no value")));
            }
            self.at += 1;
            self.skip_space();
            if !matches!(self.text.get(self.at), Some(b'"' | b'\'')) {
                return Err(self.fail(format!("attribute \"{name}\" is not in quotes")));
            }
            let quote = self.text[self.at];
            self.at += 1;
            let start = self.at;
            while self.at < self.text.len() && self.text[self.at] != quote {
                self.at += 1;
            }
            if self.at >= self.text.len() {
                return Err(self.fail(format!("attribute \"{name}\" is never closed")));
            }
            let mut value = Vec::new();
            self.append_resolved(&self.text[start..self.at], &mut value)?;
            self.at += 1;
            node.attributes.push((name, value));
        }
    }

    fn read_element(&mut self, depth: usize) -> Result<XmlNode> {
        if depth > XML_MAX_DEPTH {
            return Err(self.fail(format!(
                "elements are nested more than {XML_MAX_DEPTH} deep"
            )));
        }
        self.at += 1;
        let name = self.read_name();
        if name.is_empty() {
            return Err(self.fail("a tag has no name"));
        }
        let mut node = XmlNode {
            name,
            ..XmlNode::default()
        };
        if self.read_attributes(&mut node)? {
            return Ok(node);
        }
        let mut text = Vec::new();
        loop {
            if self.at >= self.text.len() {
                return Err(self.fail(format!("element \"{}\" is never closed", node.name)));
            }
            if self.text[self.at] != b'<' {
                let start = self.at;
                while self.at < self.text.len() && self.text[self.at] != b'<' {
                    self.at += 1;
                }
                self.append_resolved(&self.text[start..self.at], &mut text)?;
                continue;
            }
            if self.starts_with(b"<![CDATA[") {
                let end = self
                    .find(self.at + 9, b"]]>")
                    .ok_or_else(|| self.fail("a CDATA section is never closed"))?;
                text.extend_from_slice(&self.text[self.at + 9..end]);
                self.at = end + 3;
                continue;
            }
            if self.starts_with(b"<!--") || self.starts_with(b"<?") {
                self.skip_trivia()?;
                continue;
            }
            if self.starts_with(b"</") {
                self.at += 2;
                let closing = self.read_name();
                if closing != node.name {
                    return Err(self.fail(format!("\"{}\" is closed by \"{closing}\"", node.name)));
                }
                self.skip_space();
                if self.text.get(self.at) != Some(&b'>') {
                    return Err(
                        self.fail(format!("the closing tag of \"{}\" has no `>`", node.name))
                    );
                }
                self.at += 1;
                node.text = trimmed(&text).to_vec();
                return Ok(node);
            }
            node.children.push(self.read_element(depth + 1)?);
        }
    }
}

fn append_utf8(code: u32, out: &mut Vec<u8>) {
    if code < 0x80 {
        out.push(code as u8);
    } else if code < 0x800 {
        out.extend_from_slice(&[(0xc0 | (code >> 6)) as u8, (0x80 | (code & 0x3f)) as u8]);
    } else if code < 0x10000 {
        out.extend_from_slice(&[
            (0xe0 | (code >> 12)) as u8,
            (0x80 | ((code >> 6) & 0x3f)) as u8,
            (0x80 | (code & 0x3f)) as u8,
        ]);
    } else {
        out.extend_from_slice(&[
            (0xf0 | (code >> 18)) as u8,
            (0x80 | ((code >> 12) & 0x3f)) as u8,
            (0x80 | ((code >> 6) & 0x3f)) as u8,
            (0x80 | (code & 0x3f)) as u8,
        ]);
    }
}

pub fn read_xml(text: impl AsRef<[u8]>) -> Result<XmlNode> {
    Parser {
        text: text.as_ref(),
        at: 0,
    }
    .run()
}
