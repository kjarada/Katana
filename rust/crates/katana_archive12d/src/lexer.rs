//! Tokens of a .12da file (manual section 1.1). Internal to the crate.
//!
//! Four kinds of thing: a word, a text in double quotes, `{` and `}`. White
//! space, including the end of a line, separates them and is otherwise
//! nothing; `//` to the end of the line is a comment. Tokens are VIEWS into the
//! text being read, so tokenizing a 60 MB archive allocates nothing; only a
//! quoted text with an escape in it has to be copied, and only when its value
//! is asked for.
//!
//! Every place a token or a comment begins or ends is an ASCII byte (a blank,
//! a brace, a quote, a slash, a line end) or an end of the text, so every view
//! is a whole piece of the UTF-8 it was cut from.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TokenKind {
    Word,
    Quoted,
    Open,
    Close,
    #[default]
    End,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Token<'a> {
    pub kind: TokenKind,
    /// A word as written; a quoted text WITHOUT its quotes, escapes
    /// unresolved.
    pub raw: &'a str,
    pub line: u32,
    pub has_escapes: bool,
}

impl Token<'_> {
    pub fn is_word(&self) -> bool {
        self.kind == TokenKind::Word
    }

    pub fn is_value(&self) -> bool {
        self.kind == TokenKind::Word || self.kind == TokenKind::Quoted
    }

    /// The value of the token: `\"` and `\\` resolved in a quoted text.
    pub fn text(&self) -> String {
        if self.kind != TokenKind::Quoted || !self.has_escapes {
            return String::from(self.raw);
        }
        let mut out = String::with_capacity(self.raw.len());
        let mut characters = self.raw.chars().peekable();
        while let Some(character) = characters.next() {
            // Only `\"` and `\\` are escapes (manual 1.1). A backslash before
            // anything else is itself: hand-written files carry Windows paths
            // with single backslashes, and eating them would corrupt the path.
            if character == '\\' && matches!(characters.peek(), Some(&'"' | &'\\')) {
                if let Some(escaped) = characters.next() {
                    out.push(escaped);
                }
                continue;
            }
            out.push(character);
        }
        out
    }
}

/// White space between tokens: the C++'s blank, tab, carriage return, form
/// feed, vertical tab and NUL (the line feed is counted on its own).
fn is_separator(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\x0c' | b'\x0b' | b'\0')
}

pub(crate) struct Lexer<'a> {
    text: &'a str,
    position: usize,
    line: u32,
    depth: usize,
    has_lookahead: bool,
    lookahead: Token<'a>,
    unterminated_quote: bool,
    unterminated_quote_line: u32,
    /// Comment bodies (after the `//`), collected while `collect_comments` is
    /// true. The writing program puts its export settings as comments at the
    /// head of the file; the reader stops collecting at the first element.
    pub collect_comments: bool,
    pub comments: Vec<&'a str>,
}

impl<'a> Lexer<'a> {
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            position: 0,
            line: 1,
            depth: 0,
            has_lookahead: false,
            lookahead: Token::default(),
            unterminated_quote: false,
            unterminated_quote_line: 0,
            collect_comments: true,
            comments: Vec::new(),
        }
    }

    pub fn peek(&mut self) -> Token<'a> {
        if !self.has_lookahead {
            self.scan();
            self.has_lookahead = true;
        }
        self.lookahead
    }

    #[allow(
        clippy::should_implement_trait,
        reason = "The C++ Lexer::next: a token stream whose end is a token, not an iterator."
    )]
    pub fn next(&mut self) -> Token<'a> {
        if !self.has_lookahead {
            self.scan();
        }
        self.has_lookahead = false;
        if self.lookahead.kind == TokenKind::Open {
            self.depth += 1;
        } else if self.lookahead.kind == TokenKind::Close && self.depth > 0 {
            self.depth -= 1;
        }
        self.lookahead
    }

    /// Braces opened and not yet closed, after the last token returned by
    /// next().
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Set when the text ends inside a quoted string; the token stream then
    /// ends, and the reader reports this rather than "unexpected end of file".
    pub fn unterminated_quote(&self) -> bool {
        self.unterminated_quote
    }

    pub fn unterminated_quote_line(&self) -> u32 {
        self.unterminated_quote_line
    }

    fn scan(&mut self) {
        let bytes = self.text.as_bytes();
        let size = bytes.len();
        loop {
            while self.position < size {
                let byte = bytes[self.position];
                if byte == b'\n' {
                    self.line += 1;
                } else if !is_separator(byte) {
                    break;
                }
                self.position += 1;
            }
            if self.position + 1 < size
                && bytes[self.position] == b'/'
                && bytes[self.position + 1] == b'/'
            {
                let start = self.position + 2;
                let end = bytes[start..]
                    .iter()
                    .position(|&byte| byte == b'\n')
                    .map_or(size, |offset| start + offset);
                if self.collect_comments {
                    self.comments.push(&self.text[start..end]);
                }
                self.position = end;
                continue;
            }
            break;
        }

        self.lookahead = Token {
            line: self.line,
            ..Token::default()
        };
        if self.position >= size {
            self.lookahead.kind = TokenKind::End;
            return;
        }

        let byte = bytes[self.position];
        if byte == b'{' || byte == b'}' {
            self.lookahead.kind = if byte == b'{' {
                TokenKind::Open
            } else {
                TokenKind::Close
            };
            self.lookahead.raw = &self.text[self.position..self.position + 1];
            self.position += 1;
            return;
        }
        if byte == b'"' {
            let start = self.position + 1;
            let mut at = start;
            let mut escapes = false;
            while at < size && bytes[at] != b'"' {
                if bytes[at] == b'\\' && at + 1 < size {
                    escapes = true;
                    at += 1; // whatever follows a backslash cannot close the text
                }
                if bytes[at] == b'\n' {
                    self.line += 1; // a text may run over lines; keep the count honest
                }
                at += 1;
            }
            if at >= size {
                self.unterminated_quote = true;
                self.unterminated_quote_line = self.lookahead.line;
                self.lookahead.kind = TokenKind::End;
                self.position = size;
                return;
            }
            self.lookahead.kind = TokenKind::Quoted;
            self.lookahead.raw = &self.text[start..at];
            self.lookahead.has_escapes = escapes;
            self.position = at + 1;
            return;
        }

        let start = self.position;
        while self.position < size {
            let byte = bytes[self.position];
            if is_separator(byte) || byte == b'\n' || byte == b'{' || byte == b'}' || byte == b'"' {
                break;
            }
            if byte == b'/' && self.position + 1 < size && bytes[self.position + 1] == b'/' {
                break;
            }
            self.position += 1;
        }
        self.lookahead.kind = TokenKind::Word;
        self.lookahead.raw = &self.text[start..self.position];
    }
}
