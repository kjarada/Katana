#include "step_reader.hpp"

#include <charconv>
#include <limits>

#include "katana/ifc/step.hpp"

namespace katana::ifc::detail {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

std::optional<double> StepValue::number() const
{
    switch (kind) {
    case Kind::Integer:
        return static_cast<double>(integer);
    case Kind::Real:
        return real;
    case Kind::Typed:
        return items.empty() ? std::nullopt : items.front().number();
    default:
        return std::nullopt;
    }
}

const std::string* StepValue::string() const
{
    switch (kind) {
    case Kind::String:
    case Kind::Enumeration:
        return &text;
    case Kind::Typed:
        return items.empty() ? nullptr : items.front().string();
    default:
        return nullptr;
    }
}

const StepInstance* StepModel::find(std::uint32_t id) const
{
    const auto found = instances.find(id);
    return found == instances.end() ? nullptr : &found->second;
}

const StepInstance* StepModel::follow(const StepValue& value) const
{
    return value.kind == StepValue::Kind::Reference ? find(value.reference) : nullptr;
}

namespace {

bool isIdentifierStart(char c)
{
    return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || c == '_';
}

bool isIdentifierPart(char c)
{
    return isIdentifierStart(c) || (c >= '0' && c <= '9') || c == '-';
}

char upper(char c)
{
    return c >= 'a' && c <= 'z' ? static_cast<char>(c - 'a' + 'A') : c;
}

class Parser {
  public:
    explicit Parser(std::string_view text) : text_(text) {}

    Result<StepModel> run()
    {
        if (!skipTo("ISO-10303-21")) {
            return fail("not an ISO 10303-21 file: it does not begin ISO-10303-21;");
        }
        if (auto status = header(); !status) {
            return status.error();
        }
        while (true) {
            skipSpace();
            if (at_ >= text_.size()) {
                return fail("the file ends before END-ISO-10303-21");
            }
            if (consumeWord("END-ISO-10303-21")) {
                break;
            }
            if (!consumeWord("DATA")) {
                return fail("expected DATA; or END-ISO-10303-21;");
            }
            skipSpace();
            // DATA may carry a section name and schema in parentheses (the
            // standard's second edition); they are read past.
            if (peek() == '(') {
                ++at_;
                if (auto skipped = list(0); !skipped) {
                    return skipped.error();
                }
                skipSpace();
            }
            if (!consume(';')) {
                return fail("expected ; after DATA");
            }
            if (auto status = data(); !status) {
                return status.error();
            }
        }
        return std::move(model_);
    }

  private:
    [[nodiscard]] char peek() const { return at_ < text_.size() ? text_[at_] : '\0'; }

    katana::core::Error fail(const std::string& message) const
    {
        return makeError(ErrorCode::ParseFailure, message, "line " + std::to_string(line_));
    }

    void advance()
    {
        if (text_[at_] == '\n') {
            ++line_;
        }
        ++at_;
    }

    void skipSpace()
    {
        while (at_ < text_.size()) {
            const char c = text_[at_];
            if (c == ' ' || c == '\t' || c == '\r' || c == '\n') {
                advance();
            } else if (c == '/' && at_ + 1 < text_.size() && text_[at_ + 1] == '*') {
                at_ += 2;
                while (at_ < text_.size() &&
                       !(text_[at_] == '*' && at_ + 1 < text_.size() && text_[at_ + 1] == '/')) {
                    advance();
                }
                at_ = std::min(at_ + 2, text_.size());
            } else {
                break;
            }
        }
    }

    bool consume(char c)
    {
        skipSpace();
        if (peek() == c) {
            ++at_;
            return true;
        }
        return false;
    }

    // `word`, then a ';' - HEADER;, ENDSEC;, DATA is left without its ';'.
    bool consumeWord(std::string_view word)
    {
        skipSpace();
        if (text_.substr(at_, word.size()) != word) {
            return false;
        }
        const std::size_t after = at_ + word.size();
        if (after < text_.size() && isIdentifierPart(text_[after]) && word != "DATA") {
            return false;
        }
        at_ = after;
        if (word != "DATA") {
            skipSpace();
            if (peek() == ';') {
                ++at_;
            }
        }
        return true;
    }

    bool skipTo(std::string_view word)
    {
        skipSpace();
        // A byte order mark some writers put first.
        if (text_.substr(at_).starts_with("\xEF\xBB\xBF")) {
            at_ += 3;
        }
        skipSpace();
        if (!text_.substr(at_).starts_with(word)) {
            return false;
        }
        at_ += word.size();
        return consume(';');
    }

    std::string identifier()
    {
        skipSpace();
        std::string out;
        while (at_ < text_.size() && isIdentifierPart(text_[at_])) {
            out.push_back(upper(text_[at_]));
            ++at_;
        }
        return out;
    }

    katana::core::Status header()
    {
        if (!consumeWord("HEADER")) {
            return fail("expected HEADER;");
        }
        while (true) {
            skipSpace();
            if (consumeWord("ENDSEC")) {
                return {};
            }
            const std::string name = identifier();
            if (name.empty()) {
                return fail("expected a header entity or ENDSEC;");
            }
            if (!consume('(')) {
                return fail("expected ( after " + name);
            }
            auto arguments = list(0);
            if (!arguments) {
                return arguments.error();
            }
            if (!consume(';')) {
                return fail("expected ; after " + name);
            }
            if (name == "FILE_SCHEMA" && !arguments->empty() &&
                arguments->front().kind == StepValue::Kind::List &&
                !arguments->front().items.empty() && arguments->front().items.front().string()) {
                model_.schema = *arguments->front().items.front().string();
            } else if (name == "FILE_DESCRIPTION" && !arguments->empty() &&
                       arguments->front().kind == StepValue::Kind::List) {
                for (const StepValue& item : arguments->front().items) {
                    if (const std::string* text = item.string()) {
                        model_.viewDefinitions.push_back(*text);
                    }
                }
            }
        }
    }

    katana::core::Status data()
    {
        while (true) {
            skipSpace();
            if (consumeWord("ENDSEC")) {
                return {};
            }
            if (!consume('#')) {
                return fail("expected an instance (#n=...) or ENDSEC;");
            }
            const std::size_t start = at_;
            while (at_ < text_.size() && text_[at_] >= '0' && text_[at_] <= '9') {
                ++at_;
            }
            std::uint64_t id = 0;
            const auto [end, error] = std::from_chars(text_.data() + start, text_.data() + at_, id);
            if (error != std::errc{} || id == 0 || id > std::numeric_limits<std::uint32_t>::max()) {
                return fail("an instance number is missing or out of range");
            }
            (void)end;
            if (!consume('=')) {
                return fail("expected = after #" + std::to_string(id));
            }
            skipSpace();
            StepInstance instance;
            instance.line = line_;
            if (peek() == '(') {
                // A complex instance: its partial types are read and dropped.
                ++at_;
                while (true) {
                    skipSpace();
                    if (consume(')')) {
                        break;
                    }
                    const std::string part = identifier();
                    if (part.empty() || !consume('(')) {
                        return fail("a complex instance #" + std::to_string(id) +
                                    " is not a list of partial types");
                    }
                    if (auto skipped = list(0); !skipped) {
                        return skipped.error();
                    }
                }
                ++model_.complexInstances;
            } else {
                instance.type = identifier();
                if (instance.type.empty() || !consume('(')) {
                    return fail("expected an entity name and ( for #" + std::to_string(id));
                }
                auto arguments = list(0);
                if (!arguments) {
                    return arguments.error();
                }
                instance.arguments = std::move(*arguments);
            }
            if (!consume(';')) {
                return fail("expected ; to end #" + std::to_string(id));
            }
            const auto id32 = static_cast<std::uint32_t>(id);
            if (!instance.type.empty()) {
                if (!model_.instances.emplace(id32, std::move(instance)).second) {
                    return fail("#" + std::to_string(id) + " is defined twice");
                }
                model_.order.push_back(id32);
            }
        }
    }

    // The values up to and including the ')' closing a list whose '(' has
    // been consumed.
    Result<std::vector<StepValue>> list(std::size_t depth)
    {
        if (depth > kMaximumDepth) {
            return fail("lists nest more than " + std::to_string(kMaximumDepth) + " deep");
        }
        std::vector<StepValue> items;
        skipSpace();
        if (consume(')')) {
            return items;
        }
        while (true) {
            auto item = value(depth);
            if (!item) {
                return item.error();
            }
            items.push_back(std::move(*item));
            skipSpace();
            if (consume(',')) {
                continue;
            }
            if (consume(')')) {
                return items;
            }
            return fail("expected , or ) in a list");
        }
    }

    Result<StepValue> value(std::size_t depth)
    {
        skipSpace();
        StepValue out;
        const char c = peek();
        if (c == '$') {
            ++at_;
            return out;
        }
        if (c == '*') {
            ++at_;
            out.kind = StepValue::Kind::Derived;
            return out;
        }
        if (c == '#') {
            ++at_;
            const std::size_t start = at_;
            while (at_ < text_.size() && text_[at_] >= '0' && text_[at_] <= '9') {
                ++at_;
            }
            std::uint64_t id = 0;
            const auto [end, error] = std::from_chars(text_.data() + start, text_.data() + at_, id);
            (void)end;
            if (error != std::errc{} || id > std::numeric_limits<std::uint32_t>::max()) {
                return fail("a reference is missing its number or is out of range");
            }
            out.kind = StepValue::Kind::Reference;
            out.reference = static_cast<std::uint32_t>(id);
            return out;
        }
        if (c == '\'') {
            ++at_;
            const std::size_t start = at_;
            while (true) {
                if (at_ >= text_.size()) {
                    return fail("a string is never closed");
                }
                if (text_[at_] == '\'') {
                    if (at_ + 1 < text_.size() && text_[at_ + 1] == '\'') {
                        at_ += 2;
                        continue;
                    }
                    break;
                }
                advance();
            }
            out.kind = StepValue::Kind::String;
            out.text = decodeStepString(text_.substr(start, at_ - start));
            ++at_;
            return out;
        }
        if (c == '"') {
            ++at_;
            const std::size_t start = at_;
            while (at_ < text_.size() && text_[at_] != '"') {
                advance();
            }
            if (at_ >= text_.size()) {
                return fail("a binary value is never closed");
            }
            out.kind = StepValue::Kind::Binary;
            out.text = std::string(text_.substr(start, at_ - start));
            ++at_;
            return out;
        }
        if (c == '(') {
            ++at_;
            auto items = list(depth + 1);
            if (!items) {
                return items.error();
            }
            out.kind = StepValue::Kind::List;
            out.items = std::move(*items);
            return out;
        }
        if (c == '.' &&
            !(at_ + 1 < text_.size() && text_[at_ + 1] >= '0' && text_[at_ + 1] <= '9')) {
            ++at_;
            const std::size_t start = at_;
            while (at_ < text_.size() && text_[at_] != '.') {
                if (!isIdentifierPart(text_[at_])) {
                    return fail("an enumeration value is malformed");
                }
                ++at_;
            }
            if (at_ >= text_.size()) {
                return fail("an enumeration value is never closed");
            }
            out.kind = StepValue::Kind::Enumeration;
            out.text = std::string(text_.substr(start, at_ - start));
            ++at_;
            return out;
        }
        if ((c >= '0' && c <= '9') || c == '-' || c == '+' || c == '.') {
            const std::size_t start = at_;
            bool real = false;
            if (c == '-' || c == '+') {
                ++at_;
            }
            while (at_ < text_.size()) {
                const char d = text_[at_];
                if (d >= '0' && d <= '9') {
                    ++at_;
                } else if (d == '.' || d == 'E' || d == 'e') {
                    real = true;
                    ++at_;
                    if ((d == 'E' || d == 'e') && at_ < text_.size() &&
                        (text_[at_] == '-' || text_[at_] == '+')) {
                        ++at_;
                    }
                } else {
                    break;
                }
            }
            std::string_view token = text_.substr(start, at_ - start);
            if (token.starts_with('+')) {
                token.remove_prefix(1);
            }
            if (!real) {
                const auto [end, error] =
                    std::from_chars(token.data(), token.data() + token.size(), out.integer);
                if (error == std::errc{} && end == token.data() + token.size()) {
                    out.kind = StepValue::Kind::Integer;
                    return out;
                }
            }
            // "1." and "1.E-05" are how STEP writes reals; a point with no
            // digits after it says nothing, and dropping it changes no value
            // while keeping the text in the form every parser reads.
            std::string number(token);
            if (const std::size_t dot = number.find('.');
                dot != std::string::npos &&
                (dot + 1 == number.size() || number[dot + 1] == 'E' || number[dot + 1] == 'e')) {
                number.erase(dot, 1);
            }
            const auto [end, error] =
                std::from_chars(number.data(), number.data() + number.size(), out.real);
            if (error != std::errc{} || end != number.data() + number.size()) {
                return fail("\"" + std::string(token) + "\" is not a number");
            }
            out.kind = StepValue::Kind::Real;
            return out;
        }
        if (isIdentifierStart(c)) {
            out.kind = StepValue::Kind::Typed;
            out.text = identifier();
            if (!consume('(')) {
                return fail("expected ( after the type " + out.text);
            }
            auto items = list(depth + 1);
            if (!items) {
                return items.error();
            }
            out.items = std::move(*items);
            return out;
        }
        return fail(std::string("unexpected character '") + c + "'");
    }

    std::string_view text_;
    std::size_t at_ = 0;
    std::size_t line_ = 1;
    StepModel model_;
};

} // namespace

Result<StepModel> parseStep(std::string_view text)
{
    return Parser(text).run();
}

} // namespace katana::ifc::detail
