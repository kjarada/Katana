#include "lexer.hpp"

namespace katana::archive12d::detail {

std::string Token::text() const
{
    if (kind != TokenKind::Quoted || !hasEscapes) {
        return std::string(raw);
    }
    std::string out;
    out.reserve(raw.size());
    for (std::size_t i = 0; i < raw.size(); ++i) {
        // Only `\"` and `\\` are escapes (manual 1.1). A backslash before
        // anything else is itself: hand-written files carry Windows paths
        // with single backslashes, and eating them would corrupt the path.
        if (raw[i] == '\\' && i + 1 < raw.size() && (raw[i + 1] == '"' || raw[i + 1] == '\\')) {
            ++i;
        }
        out += raw[i];
    }
    return out;
}

const Token& Lexer::peek()
{
    if (!hasLookahead_) {
        scan();
        hasLookahead_ = true;
    }
    return lookahead_;
}

Token Lexer::next()
{
    if (!hasLookahead_) {
        scan();
    }
    hasLookahead_ = false;
    if (lookahead_.kind == TokenKind::Open) {
        ++depth_;
    } else if (lookahead_.kind == TokenKind::Close && depth_ > 0) {
        --depth_;
    }
    return lookahead_;
}

void Lexer::scan()
{
    const std::size_t size = text_.size();
    for (;;) {
        while (position_ < size) {
            const char ch = text_[position_];
            if (ch == '\n') {
                ++line_;
            } else if (ch != ' ' && ch != '\t' && ch != '\r' && ch != '\f' && ch != '\v' &&
                       ch != '\0') {
                break;
            }
            ++position_;
        }
        if (position_ + 1 < size && text_[position_] == '/' && text_[position_ + 1] == '/') {
            const std::size_t start = position_ + 2;
            std::size_t end = text_.find('\n', start);
            if (end == std::string_view::npos) {
                end = size;
            }
            if (collectComments) {
                comments.push_back(text_.substr(start, end - start));
            }
            position_ = end;
            continue;
        }
        break;
    }

    lookahead_ = Token{};
    lookahead_.line = line_;
    if (position_ >= size) {
        lookahead_.kind = TokenKind::End;
        return;
    }

    const char ch = text_[position_];
    if (ch == '{' || ch == '}') {
        lookahead_.kind = ch == '{' ? TokenKind::Open : TokenKind::Close;
        lookahead_.raw = text_.substr(position_, 1);
        ++position_;
        return;
    }
    if (ch == '"') {
        const std::size_t start = position_ + 1;
        std::size_t i = start;
        bool escapes = false;
        while (i < size && text_[i] != '"') {
            if (text_[i] == '\\' && i + 1 < size) {
                escapes = true;
                ++i; // whatever follows a backslash cannot close the text
            }
            if (text_[i] == '\n') {
                ++line_; // a text may run over lines; keep the count honest
            }
            ++i;
        }
        if (i >= size) {
            unterminatedQuote_ = true;
            unterminatedQuoteLine_ = lookahead_.line;
            lookahead_.kind = TokenKind::End;
            position_ = size;
            return;
        }
        lookahead_.kind = TokenKind::Quoted;
        lookahead_.raw = text_.substr(start, i - start);
        lookahead_.hasEscapes = escapes;
        position_ = i + 1;
        return;
    }

    const std::size_t start = position_;
    while (position_ < size) {
        const char c = text_[position_];
        if (c == ' ' || c == '\t' || c == '\r' || c == '\n' || c == '\f' || c == '\v' ||
            c == '\0' || c == '{' || c == '}' || c == '"') {
            break;
        }
        if (c == '/' && position_ + 1 < size && text_[position_ + 1] == '/') {
            break;
        }
        ++position_;
    }
    lookahead_.kind = TokenKind::Word;
    lookahead_.raw = text_.substr(start, position_ - start);
}

} // namespace katana::archive12d::detail
