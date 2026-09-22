#pragma once

// Tokens of a 12da file (manual section 1.1). Internal to the module.
//
// Four kinds of thing: a word, a text in double quotes, `{` and `}`. White
// space - including the end of a line - separates them and is otherwise
// nothing; `//` to the end of the line is a comment. Tokens are VIEWS into the
// text being read, so tokenizing a 60 MB archive allocates nothing; only a
// quoted text with an escape in it has to be copied, and only when its value
// is asked for.

#include <cstddef>
#include <cstdint>
#include <string>
#include <string_view>
#include <vector>

namespace katana::archive12d::detail {

enum class TokenKind { Word, Quoted, Open, Close, End };

struct Token {
    TokenKind kind = TokenKind::End;
    // A word as written; a quoted text WITHOUT its quotes, escapes unresolved.
    std::string_view raw;
    std::uint32_t line = 0;
    bool hasEscapes = false;

    [[nodiscard]] bool isWord() const { return kind == TokenKind::Word; }
    [[nodiscard]] bool isValue() const
    {
        return kind == TokenKind::Word || kind == TokenKind::Quoted;
    }
    // The value of the token: `\"` and `\\` resolved in a quoted text.
    [[nodiscard]] std::string text() const;
};

class Lexer {
  public:
    explicit Lexer(std::string_view text) : text_(text) {}

    [[nodiscard]] const Token& peek();
    Token next();

    // Braces opened and not yet closed, after the last token returned by next().
    [[nodiscard]] std::size_t depth() const { return depth_; }
    [[nodiscard]] std::uint32_t line() const { return line_; }

    // Set when the text ends inside a quoted string; the token stream then
    // ends, and the reader reports this rather than "unexpected end of file".
    [[nodiscard]] bool unterminatedQuote() const { return unterminatedQuote_; }
    [[nodiscard]] std::uint32_t unterminatedQuoteLine() const { return unterminatedQuoteLine_; }

    // Comment bodies (after the `//`), collected while `collectComments` is
    // true. 12d Model writes its export settings as comments at the head of
    // the file; the reader stops collecting at the first element.
    bool collectComments = true;
    std::vector<std::string_view> comments;

  private:
    void scan();

    std::string_view text_;
    std::size_t position_ = 0;
    std::uint32_t line_ = 1;
    std::size_t depth_ = 0;
    bool hasLookahead_ = false;
    Token lookahead_;
    bool unterminatedQuote_ = false;
    std::uint32_t unterminatedQuoteLine_ = 0;
};

} // namespace katana::archive12d::detail
