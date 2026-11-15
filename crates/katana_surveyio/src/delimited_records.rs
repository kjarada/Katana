//! Private to the delimited point format: the record reader that the parser,
//! the layout proposal and the detection probe all use, so that "what fields
//! does this line hold" has one answer. If the proposal split lines one way and
//! the parser another, a proposal could be confident about a layout the parser
//! then reads differently.
//!
//! The text here is decoded, validated UTF-8. Every cut is made at an ASCII
//! byte - a delimiter, a quote, a blank, a line break - so every piece of it is
//! whole UTF-8 too.

use katana_core::text::is_ascii_space;
use katana_core::{Error, ErrorCode, Result};

use crate::delimited_points::{DelimitedLayout, Delimiter, Quoting};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Field {
    /// Unquoted fields NOT yet trimmed; see value_of().
    pub(crate) text: String,
    /// The field was written "..." and is kept exactly.
    pub(crate) quoted: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Record {
    pub(crate) fields: Vec<Field>,
    /// The physical line the record starts on, 1-based.
    pub(crate) line: usize,
}

const fn is_line_end(c: u8) -> bool {
    c == b'\n' || c == b'\r'
}

/// The blanks trimmed off an unquoted field and allowed round a quoted one. Not
/// is_ascii_space, which also answers true for CR and LF: a line break ends a
/// record, it is never padding.
pub(crate) const fn is_padding(c: u8) -> bool {
    c == b' ' || c == b'\t' || c == b'\x0b' || c == b'\x0c'
}

/// core's is_ascii_space for a character of decoded text.
pub(crate) fn is_ascii_space_char(c: char) -> bool {
    u8::try_from(c).is_ok_and(is_ascii_space)
}

/// core's trimmed() for decoded text: the blanks it trims are ASCII, so what is
/// left is whole UTF-8.
pub(crate) fn trimmed_text(text: &str) -> &str {
    text.trim_matches(is_ascii_space_char)
}

/// core's split_lines() for decoded text. It cuts only at CR and LF, which are
/// ASCII, so every line of UTF-8 is UTF-8.
pub(crate) fn split_text_lines(text: &str) -> Vec<&str> {
    katana_core::text::split_lines(text.as_bytes())
        .into_iter()
        .map(|line| std::str::from_utf8(line).expect("a line of UTF-8 cut at CR or LF is UTF-8"))
        .collect()
}

/// What a field means once quoting is taken into account: an unquoted field is
/// trimmed of blanks (padding in a column-aligned file is not content), a
/// quoted one is exactly what was between the quotes.
pub(crate) fn value_of(field: &Field) -> &str {
    if field.quoted {
        &field.text
    } else {
        trimmed_text(&field.text)
    }
}

/// The character a delimiter is written as. Whitespace is written as one space.
pub(crate) const fn delimiter_character(delimiter: Delimiter) -> u8 {
    match delimiter {
        Delimiter::Comma => b',',
        Delimiter::Tab => b'\t',
        Delimiter::Semicolon => b';',
        Delimiter::Whitespace => b' ',
    }
}

/// `text` shortened to at most `limit` bytes at a UTF-8 character boundary,
/// with "..." when it was cut, and in single quotes - for quoting a field's
/// value back in an error message without pasting a whole malformed line into
/// it. 40 is the usual limit.
pub(crate) fn quoted_for_message(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return format!("'{text}'");
    }
    let bytes = text.as_bytes();
    let mut cut = limit;
    // Back up over UTF-8 continuation bytes (10xxxxxx) so the cut falls between
    // characters; half a character in a message is invalid UTF-8 in the log.
    while cut > 0 && (bytes[cut] & 0xC0) == 0x80 {
        cut -= 1;
    }
    format!("'{}...'", &text[..cut])
}

// ---- RecordReader ------------------------------------------------------------------

/// Reads records one at a time from decoded UTF-8 text.
///
/// Header lines, blank lines and comment lines are stepped over here, so that
/// every consumer skips the same lines. The reader does not know what a column
/// means; it only splits.
pub(crate) struct RecordReader<'t> {
    text: &'t str,
    /// Copied: only its delimiter, quoting, header count and comment prefix are
    /// used.
    layout: DelimitedLayout,
    position: usize,
    /// The physical line `position` is on.
    line: usize,
    comment_lines: usize,
}

impl<'t> RecordReader<'t> {
    pub(crate) fn new(text: &'t str, layout: &DelimitedLayout) -> Self {
        Self {
            text,
            layout: layout.clone(),
            position: 0,
            line: 1,
            comment_lines: 0,
        }
    }

    pub(crate) fn comment_lines(&self) -> usize {
        self.comment_lines
    }

    fn at_end(&self) -> bool {
        self.position >= self.text.len()
    }

    fn at_line_end(&self) -> bool {
        self.at_end() || is_line_end(self.text.as_bytes()[self.position])
    }

    // Steps over one line terminator (CR, LF or CRLF) and counts the line.
    fn consume_line_end(&mut self) {
        if self.at_end() {
            return;
        }
        let bytes = self.text.as_bytes();
        let c = bytes[self.position];
        self.position += 1;
        if c == b'\r' && !self.at_end() && bytes[self.position] == b'\n' {
            self.position += 1;
        }
        self.line += 1;
    }

    // The rest of the current physical line, without its terminator.
    fn rest_of_line(&self) -> &'t str {
        let text: &'t str = self.text;
        let bytes = text.as_bytes();
        let mut end = self.position;
        while end < bytes.len() && !is_line_end(bytes[end]) {
            end += 1;
        }
        &text[self.position..end]
    }

    fn skip_line(&mut self) {
        self.position += self.rest_of_line().len();
        self.consume_line_end();
    }

    fn is_blank(&self, c: u8) -> bool {
        // A tab is the field separator in a tab-delimited file, so it cannot
        // also be padding there.
        if self.layout.delimiter == Delimiter::Tab && c == b'\t' {
            return false;
        }
        is_padding(c)
    }

    fn is_delimiter(&self, c: u8) -> bool {
        if self.layout.delimiter == Delimiter::Whitespace {
            return is_padding(c);
        }
        c == delimiter_character(self.layout.delimiter)
    }

    /// The next record, None at the end of the text, or ParseFailure for a
    /// quoted field that never closes or text after a closing quote - naming
    /// the line and the column. After an error the reader is not to be used
    /// again.
    pub(crate) fn next(&mut self) -> Result<Option<Record>> {
        while !self.at_end() {
            if self.line <= self.layout.header_lines {
                self.skip_line();
                continue;
            }
            let content = trimmed_text(self.rest_of_line());
            if content.is_empty() {
                self.skip_line();
                continue;
            }
            if !self.layout.comment_prefix.is_empty()
                && content.starts_with(self.layout.comment_prefix.as_str())
            {
                self.comment_lines += 1;
                self.skip_line();
                continue;
            }
            return self.read_record().map(Some);
        }
        Ok(None)
    }

    fn read_record(&mut self) -> Result<Record> {
        let text: &'t str = self.text;
        let bytes = text.as_bytes();
        let mut record = Record {
            fields: Vec::new(),
            line: self.line,
        };
        let runs = self.layout.delimiter == Delimiter::Whitespace;
        let quoting = self.layout.quoting == Quoting::DoubleQuote;

        loop {
            if runs {
                // Blanks between fields, and at either end of the line,
                // separate nothing more than one blank does.
                while !self.at_line_end() && is_padding(bytes[self.position]) {
                    self.position += 1;
                }
                if self.at_line_end() {
                    break;
                }
            }
            let column = record.fields.len() + 1;
            let field_start = self.position;
            // Blanks before an opening quote are allowed - `1, "Kerb, north"`
            // is how many writers pad after a comma - so look past them for
            // one.
            let mut peek = self.position;
            while peek < bytes.len()
                && self.is_blank(bytes[peek])
                && !self.is_delimiter(bytes[peek])
            {
                peek += 1;
            }
            let mut field = Field::default();
            if quoting && peek < bytes.len() && bytes[peek] == b'"' {
                let opened_on = self.line;
                self.position = peek + 1;
                field.quoted = true;
                let mut closed = false;
                // The run of ordinary characters since the last quote, appended
                // whole; a line break inside it is kept as written, since RFC
                // 4180 lets a quoted field hold one and a description is the
                // one place it would.
                let mut run = self.position;
                while !self.at_end() {
                    let c = bytes[self.position];
                    if c == b'"' {
                        field.text.push_str(&text[run..self.position]);
                        if self.position + 1 < bytes.len() && bytes[self.position + 1] == b'"' {
                            field.text.push('"');
                            self.position += 2;
                            run = self.position;
                            continue;
                        }
                        self.position += 1;
                        closed = true;
                        break;
                    }
                    if is_line_end(c) {
                        self.consume_line_end();
                        continue;
                    }
                    self.position += 1;
                }
                if !closed {
                    return Err(Error::new(
                        ErrorCode::ParseFailure,
                        format!(
                            "line {opened_on}, column {column}: a quoted field opens here and is never closed"
                        ),
                    ));
                }
                while !self.at_line_end()
                    && self.is_blank(bytes[self.position])
                    && !self.is_delimiter(bytes[self.position])
                {
                    self.position += 1;
                }
                if !self.at_line_end() && !self.is_delimiter(bytes[self.position]) {
                    return Err(Error::new(
                        ErrorCode::ParseFailure,
                        format!(
                            "line {}, column {column}: text follows the closing quote of a quoted field ({})",
                            self.line,
                            quoted_for_message(self.rest_of_line(), 40)
                        ),
                    ));
                }
            } else {
                self.position = field_start;
                while !self.at_line_end() && !self.is_delimiter(bytes[self.position]) {
                    self.position += 1;
                }
                field.text = String::from(&text[field_start..self.position]);
            }
            record.fields.push(field);

            if self.at_line_end() {
                break;
            }
            // At a delimiter. With runs, the loop head steps over the whole
            // run; with a single-character delimiter exactly one is consumed,
            // so "a,,b" has an empty middle field and "a,b," an empty last one.
            if !runs {
                self.position += 1;
            }
        }
        self.consume_line_end();
        Ok(record)
    }
}

// ---- split_line --------------------------------------------------------------------

/// Splits ONE physical line into fields with `delimiter` and double-quote
/// quoting, for the proposal's delimiter search, which has to try each
/// delimiter on the same lines. A line the quote-aware split cannot read - a
/// quote left open, text after a closing one - is split again with quoting
/// off, so that the search still has a field count to judge; whether the
/// parser will accept the line is the parser's to say, with its line and
/// column.
pub(crate) fn split_line(line: &str, delimiter: Delimiter) -> Vec<Field> {
    // The line is handed over without its terminator, so the reader sees one
    // record. A quote left open would otherwise be an error; for the search it
    // is only a line that does not fit.
    let mut layout = DelimitedLayout {
        delimiter,
        quoting: Quoting::DoubleQuote,
        ..Default::default()
    };
    if let Ok(Some(record)) = RecordReader::new(line, &layout).next() {
        return record.fields;
    }
    // Unclosed quote, or text after one: split without quoting instead, which
    // gives the search a field count to judge rather than nothing.
    layout.quoting = Quoting::None;
    if let Ok(Some(record)) = RecordReader::new(line, &layout).next() {
        return record.fields;
    }
    Vec::new()
}
