//! nlohmann's reader of JSON text - its lexer (detail/input/lexer.hpp) and
//! its event-driven parser (detail/input/parser.hpp, sax_parse) - ported line
//! for line from nlohmann 3.12.0, for the one reader that needs the events
//! rather than a tree: the customisation format sees a member given twice,
//! reads a definition's strokes as they go by, and stops at a number too large
//! to hold where nlohmann stops (customisation.rs). serde_json parses only to a
//! tree, and refuses a text nested deeper than 128.
//!
//! The parser is iterative, as nlohmann's is - a stack of what is open, never
//! a level of the program's stack for a level of the text - so a text nested a
//! hundred thousand deep is read as any other.
//!
//! What is not ported: comments (nlohmann's `ignore_comments`, off in every
//! call the C++ makes) and the locale's decimal point (the "C" locale's '.' is
//! the only one the C++ runs with). The text is a `&str`, validated UTF-8
//! before it is read (docs/rust.md, "bytes up to the decoder"), so the
//! lexer's checks of UTF-8, ported as they are, never fail; the one place the
//! C++ can still hold a character cut short - the text a message says was
//! "last read" - shows it as U+FFFD.

/// The handler of the parse events: nlohmann's json_sax. Each returns whether
/// the parse goes on; `parse_error` is handed where the text stops being what
/// it must be, the last token read and the exception nlohmann would throw.
pub(crate) trait Sax {
    fn null(&mut self) -> bool;
    fn boolean(&mut self, value: bool) -> bool;
    fn number_integer(&mut self, value: i64) -> bool;
    fn number_unsigned(&mut self, value: u64) -> bool;
    /// The number and its text as written.
    fn number_float(&mut self, value: f64, written: &str) -> bool;
    fn string(&mut self, value: &mut String) -> bool;
    fn start_object(&mut self) -> bool;
    fn key(&mut self, value: &mut String) -> bool;
    fn end_object(&mut self) -> bool;
    fn start_array(&mut self) -> bool;
    fn end_array(&mut self) -> bool;
    /// `position` is the bytes read so far (nlohmann's position_t as a
    /// size_t: chars_read_total).
    fn parse_error(&mut self, position: usize, token: &str, error: &SaxError) -> bool;
}

/// The exception nlohmann hands parse_error: its id (101 for text that is not
/// JSON, 406 for a number past the largest double) and its what() text.
pub(crate) struct SaxError {
    pub(crate) id: u32,
    pub(crate) what: String,
}

/// The id of nlohmann's parse_error for text that is not JSON.
const SYNTAX_ERROR: u32 = 101;
/// The id of its out_of_range for a number past the largest double.
const NUMBER_OVERFLOW: u32 = 406;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Token {
    Uninitialized,
    LiteralTrue,
    LiteralFalse,
    LiteralNull,
    ValueString,
    ValueUnsigned,
    ValueInteger,
    ValueFloat,
    BeginArray,
    BeginObject,
    EndArray,
    EndObject,
    NameSeparator,
    ValueSeparator,
    ParseError,
    EndOfInput,
    LiteralOrValue,
}

/// lexer_base::token_type_name.
const fn token_type_name(token: Token) -> &'static str {
    match token {
        Token::Uninitialized => "<uninitialized>",
        Token::LiteralTrue => "true literal",
        Token::LiteralFalse => "false literal",
        Token::LiteralNull => "null literal",
        Token::ValueString => "string literal",
        Token::ValueUnsigned | Token::ValueInteger | Token::ValueFloat => "number literal",
        Token::BeginArray => "'['",
        Token::BeginObject => "'{'",
        Token::EndArray => "']'",
        Token::EndObject => "'}'",
        Token::NameSeparator => "':'",
        Token::ValueSeparator => "','",
        Token::ParseError => "<parse error>",
        Token::EndOfInput => "end of input",
        Token::LiteralOrValue => "'[', '{', or a literal",
    }
}

/// What scan_string says of each control character, U+0000 to U+001F.
const CONTROL_CHARACTERS: [&str; 32] = [
    "invalid string: control character U+0000 (NUL) must be escaped to \\u0000",
    "invalid string: control character U+0001 (SOH) must be escaped to \\u0001",
    "invalid string: control character U+0002 (STX) must be escaped to \\u0002",
    "invalid string: control character U+0003 (ETX) must be escaped to \\u0003",
    "invalid string: control character U+0004 (EOT) must be escaped to \\u0004",
    "invalid string: control character U+0005 (ENQ) must be escaped to \\u0005",
    "invalid string: control character U+0006 (ACK) must be escaped to \\u0006",
    "invalid string: control character U+0007 (BEL) must be escaped to \\u0007",
    "invalid string: control character U+0008 (BS) must be escaped to \\u0008 or \\b",
    "invalid string: control character U+0009 (HT) must be escaped to \\u0009 or \\t",
    "invalid string: control character U+000A (LF) must be escaped to \\u000A or \\n",
    "invalid string: control character U+000B (VT) must be escaped to \\u000B",
    "invalid string: control character U+000C (FF) must be escaped to \\u000C or \\f",
    "invalid string: control character U+000D (CR) must be escaped to \\u000D or \\r",
    "invalid string: control character U+000E (SO) must be escaped to \\u000E",
    "invalid string: control character U+000F (SI) must be escaped to \\u000F",
    "invalid string: control character U+0010 (DLE) must be escaped to \\u0010",
    "invalid string: control character U+0011 (DC1) must be escaped to \\u0011",
    "invalid string: control character U+0012 (DC2) must be escaped to \\u0012",
    "invalid string: control character U+0013 (DC3) must be escaped to \\u0013",
    "invalid string: control character U+0014 (DC4) must be escaped to \\u0014",
    "invalid string: control character U+0015 (NAK) must be escaped to \\u0015",
    "invalid string: control character U+0016 (SYN) must be escaped to \\u0016",
    "invalid string: control character U+0017 (ETB) must be escaped to \\u0017",
    "invalid string: control character U+0018 (CAN) must be escaped to \\u0018",
    "invalid string: control character U+0019 (EM) must be escaped to \\u0019",
    "invalid string: control character U+001A (SUB) must be escaped to \\u001A",
    "invalid string: control character U+001B (ESC) must be escaped to \\u001B",
    "invalid string: control character U+001C (FS) must be escaped to \\u001C",
    "invalid string: control character U+001D (GS) must be escaped to \\u001D",
    "invalid string: control character U+001E (RS) must be escaped to \\u001E",
    "invalid string: control character U+001F (US) must be escaped to \\u001F",
];

/// position_t: where the lexer stands.
#[derive(Clone, Copy, Default)]
struct Position {
    chars_read_total: usize,
    chars_read_current_line: usize,
    lines_read: usize,
}

struct Lexer<'a> {
    input: &'a [u8],
    /// The input adapter's place: the next byte it hands out.
    next: usize,
    /// The current character; None is char_traits::eof().
    current: Option<u8>,
    next_unget: bool,
    position: Position,
    /// The raw text of the token, for error messages.
    token_string: Vec<u8>,
    /// A string's or a number's bytes, escapes resolved.
    token_buffer: Vec<u8>,
    error_message: &'static str,
    value_integer: i64,
    value_unsigned: u64,
    value_float: f64,
}

impl<'a> Lexer<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            next: 0,
            current: None,
            next_unget: false,
            position: Position::default(),
            token_string: Vec::new(),
            token_buffer: Vec::new(),
            error_message: "",
            value_integer: 0,
            value_unsigned: 0,
            value_float: 0.0,
        }
    }

    /// The code point of the four hexadecimal digits after `\u`, or -1.
    fn get_codepoint(&mut self) -> i32 {
        let mut codepoint: i32 = 0;
        for factor in [12_u32, 8, 4, 0] {
            let digit = match self.get() {
                Some(character @ b'0'..=b'9') => u32::from(character) - 0x30,
                Some(character @ b'A'..=b'F') => u32::from(character) - 0x37,
                Some(character @ b'a'..=b'f') => u32::from(character) - 0x57,
                _ => return -1,
            };
            codepoint += (digit << factor) as i32;
        }
        codepoint
    }

    /// Adds the current byte and, for each range, reads a byte and checks
    /// that it is inside it.
    fn next_byte_in_range(&mut self, ranges: &[(u8, u8)]) -> bool {
        if let Some(lead) = self.current {
            self.add(lead);
        }
        for &(low, high) in ranges {
            match self.get() {
                Some(byte) if (low..=high).contains(&byte) => self.add(byte),
                _ => {
                    self.error_message = "invalid string: ill-formed UTF-8 byte";
                    return false;
                }
            }
        }
        true
    }

    fn scan_string(&mut self) -> Token {
        // reset token_buffer (the opening quote is not part of the string)
        self.reset();
        loop {
            let Some(byte) = self.get() else {
                self.error_message = "invalid string: missing closing quote";
                return Token::ParseError;
            };
            match byte {
                b'"' => return Token::ValueString,
                b'\\' => match self.get() {
                    Some(b'"') => self.add(b'"'),
                    Some(b'\\') => self.add(b'\\'),
                    Some(b'/') => self.add(b'/'),
                    Some(b'b') => self.add(0x08),
                    Some(b'f') => self.add(0x0C),
                    Some(b'n') => self.add(b'\n'),
                    Some(b'r') => self.add(b'\r'),
                    Some(b't') => self.add(b'\t'),
                    Some(b'u') => {
                        if !self.scan_unicode_escape() {
                            return Token::ParseError;
                        }
                    }
                    _ => {
                        self.error_message = "invalid string: forbidden character after backslash";
                        return Token::ParseError;
                    }
                },
                0x00..=0x1F => {
                    self.error_message = CONTROL_CHARACTERS[usize::from(byte)];
                    return Token::ParseError;
                }
                // U+0020..U+007F, but for the quote and the backslash
                0x20..=0x21 | 0x23..=0x5B | 0x5D..=0x7F => self.add(byte),
                // U+0080..U+07FF: bytes C2..DF 80..BF
                0xC2..=0xDF => {
                    if !self.next_byte_in_range(&[(0x80, 0xBF)]) {
                        return Token::ParseError;
                    }
                }
                // U+0800..U+0FFF: bytes E0 A0..BF 80..BF
                0xE0 => {
                    if !self.next_byte_in_range(&[(0xA0, 0xBF), (0x80, 0xBF)]) {
                        return Token::ParseError;
                    }
                }
                // U+1000..U+CFFF and U+E000..U+FFFF: E1..EC or EE..EF 80..BF 80..BF
                0xE1..=0xEC | 0xEE | 0xEF => {
                    if !self.next_byte_in_range(&[(0x80, 0xBF), (0x80, 0xBF)]) {
                        return Token::ParseError;
                    }
                }
                // U+D000..U+D7FF: bytes ED 80..9F 80..BF
                0xED => {
                    if !self.next_byte_in_range(&[(0x80, 0x9F), (0x80, 0xBF)]) {
                        return Token::ParseError;
                    }
                }
                // U+10000..U+3FFFF: F0 90..BF 80..BF 80..BF
                0xF0 => {
                    if !self.next_byte_in_range(&[(0x90, 0xBF), (0x80, 0xBF), (0x80, 0xBF)]) {
                        return Token::ParseError;
                    }
                }
                // U+40000..U+FFFFF: F1..F3 80..BF 80..BF 80..BF
                0xF1..=0xF3 => {
                    if !self.next_byte_in_range(&[(0x80, 0xBF), (0x80, 0xBF), (0x80, 0xBF)]) {
                        return Token::ParseError;
                    }
                }
                // U+100000..U+10FFFF: F4 80..8F 80..BF 80..BF
                0xF4 => {
                    if !self.next_byte_in_range(&[(0x80, 0x8F), (0x80, 0xBF), (0x80, 0xBF)]) {
                        return Token::ParseError;
                    }
                }
                // 80..C1 and F5..FF are ill-formed
                _ => {
                    self.error_message = "invalid string: ill-formed UTF-8 byte";
                    return Token::ParseError;
                }
            }
        }
    }

    /// The `\uXXXX` (or the surrogate pair) after a backslash, as UTF-8 in
    /// the token buffer; false with the error message set when it is not one.
    fn scan_unicode_escape(&mut self) -> bool {
        const NOT_FOUR_DIGITS: &str = "invalid string: '\\u' must be followed by 4 hex digits";
        const NO_LOW_SURROGATE: &str =
            "invalid string: surrogate U+D800..U+DBFF must be followed by U+DC00..U+DFFF";
        let codepoint1 = self.get_codepoint();
        let mut codepoint = codepoint1;
        if codepoint1 == -1 {
            self.error_message = NOT_FOUR_DIGITS;
            return false;
        }
        if (0xD800..=0xDBFF).contains(&codepoint1) {
            // the next \uXXXX must be the low surrogate
            if self.get() == Some(b'\\') && self.get() == Some(b'u') {
                let codepoint2 = self.get_codepoint();
                if codepoint2 == -1 {
                    self.error_message = NOT_FOUR_DIGITS;
                    return false;
                }
                if (0xDC00..=0xDFFF).contains(&codepoint2) {
                    // (0xD800 << 10) + 0xDC00 - 0x10000 = 0x35FDC00
                    codepoint = ((codepoint1 as u32) << 10)
                        .wrapping_add(codepoint2 as u32)
                        .wrapping_sub(0x035F_DC00) as i32;
                } else {
                    self.error_message = NO_LOW_SURROGATE;
                    return false;
                }
            } else {
                self.error_message = NO_LOW_SURROGATE;
                return false;
            }
        } else if (0xDC00..=0xDFFF).contains(&codepoint1) {
            self.error_message =
                "invalid string: surrogate U+DC00..U+DFFF must follow U+D800..U+DBFF";
            return false;
        }
        let value = codepoint as u32;
        if value < 0x80 {
            self.add(value as u8);
        } else if value <= 0x7FF {
            self.add((0xC0 | (value >> 6)) as u8);
            self.add((0x80 | (value & 0x3F)) as u8);
        } else if value <= 0xFFFF {
            self.add((0xE0 | (value >> 12)) as u8);
            self.add((0x80 | ((value >> 6) & 0x3F)) as u8);
            self.add((0x80 | (value & 0x3F)) as u8);
        } else {
            self.add((0xF0 | (value >> 18)) as u8);
            self.add((0x80 | ((value >> 12) & 0x3F)) as u8);
            self.add((0x80 | ((value >> 6) & 0x3F)) as u8);
            self.add((0x80 | (value & 0x3F)) as u8);
        }
        true
    }

    /// The number the current character begins: RFC 8259's grammar as a
    /// state machine, each state the label of the C++'s goto.
    fn scan_number(&mut self) -> Token {
        #[derive(Clone, Copy)]
        enum State {
            Minus,
            Zero,
            Any1,
            Decimal1,
            Decimal2,
            Exponent,
            Sign,
            Any2,
            Done,
        }
        // reset token_buffer to store the number's bytes
        self.reset();
        // unsigned until a minus sign, a decimal point or an exponent is read
        let mut number_type = Token::ValueUnsigned;
        let mut state = match self.current {
            Some(b'-') => {
                self.add(b'-');
                State::Minus
            }
            Some(b'0') => {
                self.add(b'0');
                State::Zero
            }
            // scan() hands over only '-' and the digits
            Some(digit) => {
                self.add(digit);
                State::Any1
            }
            None => State::Done,
        };
        loop {
            state = match state {
                State::Minus => {
                    number_type = Token::ValueInteger;
                    match self.get() {
                        Some(b'0') => {
                            self.add(b'0');
                            State::Zero
                        }
                        Some(digit @ b'1'..=b'9') => {
                            self.add(digit);
                            State::Any1
                        }
                        _ => {
                            self.error_message = "invalid number; expected digit after '-'";
                            return Token::ParseError;
                        }
                    }
                }
                State::Zero => match self.get() {
                    Some(b'.') => {
                        self.add(b'.');
                        State::Decimal1
                    }
                    Some(exponent @ (b'e' | b'E')) => {
                        self.add(exponent);
                        State::Exponent
                    }
                    _ => State::Done,
                },
                State::Any1 => match self.get() {
                    Some(digit @ b'0'..=b'9') => {
                        self.add(digit);
                        State::Any1
                    }
                    Some(b'.') => {
                        self.add(b'.');
                        State::Decimal1
                    }
                    Some(exponent @ (b'e' | b'E')) => {
                        self.add(exponent);
                        State::Exponent
                    }
                    _ => State::Done,
                },
                State::Decimal1 => {
                    number_type = Token::ValueFloat;
                    match self.get() {
                        Some(digit @ b'0'..=b'9') => {
                            self.add(digit);
                            State::Decimal2
                        }
                        _ => {
                            self.error_message = "invalid number; expected digit after '.'";
                            return Token::ParseError;
                        }
                    }
                }
                State::Decimal2 => match self.get() {
                    Some(digit @ b'0'..=b'9') => {
                        self.add(digit);
                        State::Decimal2
                    }
                    Some(exponent @ (b'e' | b'E')) => {
                        self.add(exponent);
                        State::Exponent
                    }
                    _ => State::Done,
                },
                State::Exponent => {
                    number_type = Token::ValueFloat;
                    match self.get() {
                        Some(sign @ (b'+' | b'-')) => {
                            self.add(sign);
                            State::Sign
                        }
                        Some(digit @ b'0'..=b'9') => {
                            self.add(digit);
                            State::Any2
                        }
                        _ => {
                            self.error_message =
                                "invalid number; expected '+', '-', or digit after exponent";
                            return Token::ParseError;
                        }
                    }
                }
                State::Sign => match self.get() {
                    Some(digit @ b'0'..=b'9') => {
                        self.add(digit);
                        State::Any2
                    }
                    _ => {
                        self.error_message = "invalid number; expected digit after exponent sign";
                        return Token::ParseError;
                    }
                },
                State::Any2 => match self.get() {
                    Some(digit @ b'0'..=b'9') => {
                        self.add(digit);
                        State::Any2
                    }
                    _ => State::Done,
                },
                State::Done => break,
            };
        }
        // unget the character after the number: it was read only to know
        // that the number had ended
        self.unget();

        // Integers first, falling back to a double when one does not fit, as
        // strtoull and strtoll fail with ERANGE. Rust's parse of these
        // already-checked tokens is strtoull's, strtoll's and (correctly
        // rounded, an overflow to infinity) glibc strtod's.
        let text = String::from_utf8_lossy(&self.token_buffer);
        if number_type == Token::ValueUnsigned
            && let Ok(value) = text.parse::<u64>()
        {
            self.value_unsigned = value;
            return Token::ValueUnsigned;
        }
        if number_type == Token::ValueInteger
            && let Ok(value) = text.parse::<i64>()
        {
            self.value_integer = value;
            return Token::ValueInteger;
        }
        self.value_float = text.parse::<f64>().unwrap_or(f64::NAN);
        Token::ValueFloat
    }

    fn scan_literal(&mut self, literal: &[u8], token: Token) -> Token {
        for &expected in &literal[1..] {
            if self.get() != Some(expected) {
                self.error_message = "invalid literal";
                return Token::ParseError;
            }
        }
        token
    }

    /// The token buffer is reset, and the current character begins the token.
    fn reset(&mut self) {
        self.token_buffer.clear();
        self.token_string.clear();
        if let Some(character) = self.current {
            self.token_string.push(character);
        }
    }

    fn get(&mut self) -> Option<u8> {
        self.position.chars_read_total += 1;
        self.position.chars_read_current_line += 1;
        if self.next_unget {
            // just reset next_unget and work with the current character
            self.next_unget = false;
        } else {
            self.current = self.input.get(self.next).copied();
            if self.current.is_some() {
                self.next += 1;
            }
        }
        if let Some(character) = self.current {
            self.token_string.push(character);
        }
        if self.current == Some(b'\n') {
            self.position.lines_read += 1;
            self.position.chars_read_current_line = 0;
        }
        self.current
    }

    /// The current character is read again by the next get().
    fn unget(&mut self) {
        self.next_unget = true;
        self.position.chars_read_total -= 1;
        // an unget newline takes its line back too
        if self.position.chars_read_current_line == 0 {
            if self.position.lines_read > 0 {
                self.position.lines_read -= 1;
            }
        } else {
            self.position.chars_read_current_line -= 1;
        }
        if self.current.is_some() {
            self.token_string.pop();
        }
    }

    fn add(&mut self, byte: u8) {
        self.token_buffer.push(byte);
    }

    /// The token buffer as text: a string's value or a number's text. Always
    /// UTF-8, being whole characters of a `&str` and the UTF-8 of escapes.
    fn token_text(&self) -> String {
        String::from_utf8_lossy(&self.token_buffer).into_owned()
    }

    /// The last token read, control characters shown as <U+XXXX>.
    fn token_string(&self) -> String {
        let mut result = Vec::with_capacity(self.token_string.len());
        for &byte in &self.token_string {
            if byte <= 0x1F {
                result.extend_from_slice(format!("<U+{byte:04X}>").as_bytes());
            } else {
                result.push(byte);
            }
        }
        String::from_utf8_lossy(&result).into_owned()
    }

    /// Skips a UTF-8 byte order mark; false for one that is not whole.
    fn skip_bom(&mut self) -> bool {
        if self.get() == Some(0xEF) {
            return self.get() == Some(0xBB) && self.get() == Some(0xBF);
        }
        // not the beginning of a byte order mark: read it again later
        self.unget();
        true
    }

    fn skip_whitespace(&mut self) {
        loop {
            self.get();
            if !matches!(self.current, Some(b' ' | b'\t' | b'\n' | b'\r')) {
                break;
            }
        }
    }

    fn scan(&mut self) -> Token {
        // initially, skip the byte order mark
        if self.position.chars_read_total == 0 && !self.skip_bom() {
            self.error_message = "invalid BOM; must be 0xEF 0xBB 0xBF if given";
            return Token::ParseError;
        }
        self.skip_whitespace();
        match self.current {
            Some(b'[') => Token::BeginArray,
            Some(b']') => Token::EndArray,
            Some(b'{') => Token::BeginObject,
            Some(b'}') => Token::EndObject,
            Some(b':') => Token::NameSeparator,
            Some(b',') => Token::ValueSeparator,
            Some(b't') => self.scan_literal(b"true", Token::LiteralTrue),
            Some(b'f') => self.scan_literal(b"false", Token::LiteralFalse),
            Some(b'n') => self.scan_literal(b"null", Token::LiteralNull),
            Some(b'"') => self.scan_string(),
            Some(b'-' | b'0'..=b'9') => self.scan_number(),
            // the end of the input, or a NUL, which ends a C string
            Some(0) | None => Token::EndOfInput,
            Some(_) => {
                self.error_message = "invalid literal";
                Token::ParseError
            }
        }
    }
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    last_token: Token,
}

impl Parser<'_> {
    fn get_token(&mut self) -> Token {
        self.last_token = self.lexer.scan();
        self.last_token
    }

    fn exception_message(&self, expected: Token, context: &str) -> String {
        let mut message = String::from("syntax error ");
        if !context.is_empty() {
            message.push_str("while parsing ");
            message.push_str(context);
            message.push(' ');
        }
        message.push_str("- ");
        if self.last_token == Token::ParseError {
            message.push_str(self.lexer.error_message);
            message.push_str("; last read: '");
            message.push_str(&self.lexer.token_string());
            message.push('\'');
        } else {
            message.push_str("unexpected ");
            message.push_str(token_type_name(self.last_token));
        }
        if expected != Token::Uninitialized {
            message.push_str("; expected ");
            message.push_str(token_type_name(expected));
        }
        message
    }

    /// Hands the handler the exception parse_error::create(101, ...) makes of
    /// `message`, and returns what it says.
    fn refuse<S: Sax>(&self, sax: &mut S, message: &str) -> bool {
        let position = self.lexer.position;
        let error = SaxError {
            id: SYNTAX_ERROR,
            what: format!(
                "[json.exception.parse_error.{SYNTAX_ERROR}] parse error at line {}, column {}: {message}",
                position.lines_read + 1,
                position.chars_read_current_line
            ),
        };
        sax.parse_error(
            position.chars_read_total,
            &self.lexer.token_string(),
            &error,
        )
    }

    fn syntax_error<S: Sax>(&self, sax: &mut S, expected: Token, context: &str) -> bool {
        let message = self.exception_message(expected, context);
        self.refuse(sax, &message)
    }

    fn sax_parse_internal<S: Sax>(&mut self, sax: &mut S) -> bool {
        // what is open: true for a list, false for an object
        let mut states: Vec<bool> = Vec::new();
        // set where the C++ would jump to the evaluation of the states
        let mut skip_to_state_evaluation = false;
        loop {
            if skip_to_state_evaluation {
                skip_to_state_evaluation = false;
            } else {
                // get_token() was called before each pass
                match self.last_token {
                    Token::BeginObject => {
                        if !sax.start_object() {
                            return false;
                        }
                        // a closing '}' at once: the object is done
                        if self.get_token() == Token::EndObject {
                            if !sax.end_object() {
                                return false;
                            }
                        } else {
                            if self.last_token != Token::ValueString {
                                return self.syntax_error(sax, Token::ValueString, "object key");
                            }
                            let mut key = self.lexer.token_text();
                            if !sax.key(&mut key) {
                                return false;
                            }
                            if self.get_token() != Token::NameSeparator {
                                return self.syntax_error(
                                    sax,
                                    Token::NameSeparator,
                                    "object separator",
                                );
                            }
                            states.push(false);
                            self.get_token();
                            continue;
                        }
                    }
                    Token::BeginArray => {
                        if !sax.start_array() {
                            return false;
                        }
                        // a closing ']' at once: the list is done
                        if self.get_token() == Token::EndArray {
                            if !sax.end_array() {
                                return false;
                            }
                        } else {
                            states.push(true);
                            // the value is already the last token read
                            continue;
                        }
                    }
                    Token::ValueFloat => {
                        let value = self.lexer.value_float;
                        if !value.is_finite() {
                            let token = self.lexer.token_string();
                            let error = SaxError {
                                id: NUMBER_OVERFLOW,
                                what: format!(
                                    "[json.exception.out_of_range.{NUMBER_OVERFLOW}] number overflow parsing '{token}'"
                                ),
                            };
                            return sax.parse_error(
                                self.lexer.position.chars_read_total,
                                &token,
                                &error,
                            );
                        }
                        let written = self.lexer.token_text();
                        if !sax.number_float(value, &written) {
                            return false;
                        }
                    }
                    Token::LiteralFalse => {
                        if !sax.boolean(false) {
                            return false;
                        }
                    }
                    Token::LiteralNull => {
                        if !sax.null() {
                            return false;
                        }
                    }
                    Token::LiteralTrue => {
                        if !sax.boolean(true) {
                            return false;
                        }
                    }
                    Token::ValueInteger => {
                        if !sax.number_integer(self.lexer.value_integer) {
                            return false;
                        }
                    }
                    Token::ValueString => {
                        let mut text = self.lexer.token_text();
                        if !sax.string(&mut text) {
                            return false;
                        }
                    }
                    Token::ValueUnsigned => {
                        if !sax.number_unsigned(self.lexer.value_unsigned) {
                            return false;
                        }
                    }
                    Token::ParseError => {
                        // "uninitialized": no "expected" in the message
                        return self.syntax_error(sax, Token::Uninitialized, "value");
                    }
                    Token::EndOfInput => {
                        if self.lexer.position.chars_read_total == 1 {
                            return self.refuse(
                                sax,
                                "attempting to parse an empty input; check that your input string or stream contains the expected JSON",
                            );
                        }
                        return self.syntax_error(sax, Token::LiteralOrValue, "value");
                    }
                    Token::Uninitialized
                    | Token::EndArray
                    | Token::EndObject
                    | Token::NameSeparator
                    | Token::ValueSeparator
                    | Token::LiteralOrValue => {
                        // the last token was unexpected
                        return self.syntax_error(sax, Token::LiteralOrValue, "value");
                    }
                }
            }

            // a value was read
            let Some(&in_list) = states.last() else {
                // nothing is open: the text's one value is done
                return true;
            };
            if in_list {
                // a comma: the next value
                if self.get_token() == Token::ValueSeparator {
                    self.get_token();
                    continue;
                }
                if self.last_token == Token::EndArray {
                    if !sax.end_array() {
                        return false;
                    }
                    // the list is done: the state of what holds it decides next
                    states.pop();
                    skip_to_state_evaluation = true;
                    continue;
                }
                return self.syntax_error(sax, Token::EndArray, "array");
            }

            // in an object: a comma, then the next member
            if self.get_token() == Token::ValueSeparator {
                if self.get_token() != Token::ValueString {
                    return self.syntax_error(sax, Token::ValueString, "object key");
                }
                let mut key = self.lexer.token_text();
                if !sax.key(&mut key) {
                    return false;
                }
                if self.get_token() != Token::NameSeparator {
                    return self.syntax_error(sax, Token::NameSeparator, "object separator");
                }
                self.get_token();
                continue;
            }
            if self.last_token == Token::EndObject {
                if !sax.end_object() {
                    return false;
                }
                states.pop();
                skip_to_state_evaluation = true;
                continue;
            }
            return self.syntax_error(sax, Token::EndObject, "object");
        }
    }
}

/// `json::sax_parse(text, &sax)`: strict - the text is one value and nothing
/// after it - and without comments. Returns whether it was read to its end.
pub(crate) fn sax_parse<S: Sax>(text: &str, sax: &mut S) -> bool {
    let mut parser = Parser {
        lexer: Lexer::new(text.as_bytes()),
        last_token: Token::Uninitialized,
    };
    // the parser reads its first token as it is made
    parser.get_token();
    let read = parser.sax_parse_internal(sax);
    // strict: the next token must be the end of the input
    if read && parser.get_token() != Token::EndOfInput {
        return parser.syntax_error(sax, Token::EndOfInput, "value");
    }
    read
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records the events as text, the doubles apart, and the first error.
    #[derive(Default)]
    struct Events {
        seen: Vec<String>,
        doubles: Vec<f64>,
        error: Option<(usize, String, u32, String)>,
    }

    impl Sax for Events {
        fn null(&mut self) -> bool {
            self.seen.push(String::from("null"));
            true
        }
        fn boolean(&mut self, value: bool) -> bool {
            self.seen.push(format!("bool {value}"));
            true
        }
        fn number_integer(&mut self, value: i64) -> bool {
            self.seen.push(format!("integer {value}"));
            true
        }
        fn number_unsigned(&mut self, value: u64) -> bool {
            self.seen.push(format!("unsigned {value}"));
            true
        }
        fn number_float(&mut self, value: f64, written: &str) -> bool {
            self.seen.push(format!("float {written}"));
            self.doubles.push(value);
            true
        }
        fn string(&mut self, value: &mut String) -> bool {
            self.seen.push(format!("string {value}"));
            true
        }
        fn start_object(&mut self) -> bool {
            self.seen.push(String::from("{"));
            true
        }
        fn key(&mut self, value: &mut String) -> bool {
            self.seen.push(format!("key {value}"));
            true
        }
        fn end_object(&mut self) -> bool {
            self.seen.push(String::from("}"));
            true
        }
        fn start_array(&mut self) -> bool {
            self.seen.push(String::from("["));
            true
        }
        fn end_array(&mut self) -> bool {
            self.seen.push(String::from("]"));
            true
        }
        fn parse_error(&mut self, position: usize, token: &str, error: &SaxError) -> bool {
            self.error = Some((position, String::from(token), error.id, error.what.clone()));
            false
        }
    }

    fn events(text: &str) -> Events {
        let mut events = Events::default();
        sax_parse(text, &mut events);
        events
    }

    // The events, the classification of numbers and the escapes, worked out
    // from lexer.hpp's scan_number and scan_string: "-0" is the integer 0, a
    // fraction or an exponent makes a double, an integer past 64 bits falls
    // back to a double.
    #[test]
    fn the_events_are_nlohmanns() {
        let read = events(
            "{\"a\": [1, -0, -2, 1.5, 1e2, 18446744073709551616, true, null], \"b\\u00e9\": \"x\\ny\"}",
        );
        assert_eq!(read.error, None);
        assert_eq!(
            read.seen,
            [
                "{",
                "key a",
                "[",
                "unsigned 1",
                "integer 0",
                "integer -2",
                "float 1.5",
                "float 1e2",
                "float 18446744073709551616",
                "bool true",
                "null",
                "]",
                "key b\u{e9}",
                "string x\ny",
                "}",
            ]
        );
        // 2^64, one past the largest unsigned, is a double exactly.
        assert_eq!(read.doubles, [1.5, 100.0, 2.0_f64.powi(64)]);
    }

    // By hand from position_t and unget(): "[0,\n" is four bytes, and the
    // number's last character is the ninth byte read.
    #[test]
    fn a_number_past_the_largest_double_stops_the_parse_where_it_ends() {
        let read = events("[0,\n1e400]");
        let (position, token, id, what) = read.error.expect("an overflow");
        assert_eq!(position, 9);
        assert_eq!(token, "1e400");
        assert_eq!(id, 406);
        assert_eq!(
            what,
            "[json.exception.out_of_range.406] number overflow parsing '1e400'"
        );
    }

    // By hand: after "1" the lexer reads the line break and the blanks of
    // line 4, and stops at the closing quote of "name", column 8.
    #[test]
    fn a_syntax_error_says_its_line_and_column_as_nlohmann_does() {
        let read = events("{\n  \"format\": \"x\",\n  \"version\": 1\n  \"name\": \"T\"\n}\n");
        let (_, _, id, what) = read.error.expect("a syntax error");
        assert_eq!(id, 101);
        assert_eq!(
            what,
            "[json.exception.parse_error.101] parse error at line 4, column 8: syntax error while parsing object - unexpected string literal; expected '}'"
        );
        let empty = events("");
        assert_eq!(
            empty.error.expect("an empty input").3,
            "[json.exception.parse_error.101] parse error at line 1, column 1: attempting to parse an empty input; check that your input string or stream contains the expected JSON"
        );
        let literal = events("not json");
        assert_eq!(
            literal.error.expect("an invalid literal").3,
            "[json.exception.parse_error.101] parse error at line 1, column 2: syntax error while parsing value - invalid literal; last read: 'no'"
        );
    }

    #[test]
    fn a_text_nested_a_hundred_thousand_deep_is_read() {
        let deep = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
        let read = events(&deep);
        assert_eq!(read.error, None);
        assert_eq!(read.seen.len(), 200_000);
    }
}
