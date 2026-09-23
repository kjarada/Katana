#include "delimited_internal.hpp"

#include "katana/core/text.hpp"

namespace katana::surveyio::delimited {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

constexpr bool isLineEnd(char c)
{
    return c == '\n' || c == '\r';
}

// The blanks trimmed off an unquoted field and allowed round a quoted one. Not
// isAsciiSpace, which also answers true for CR and LF: a line break ends a
// record, it is never padding.
constexpr bool isPadding(char c)
{
    return c == ' ' || c == '\t' || c == '\v' || c == '\f';
}

} // namespace

std::string_view valueOf(const Field& field)
{
    return field.quoted ? std::string_view(field.text) : katana::core::trimmed(field.text);
}

char delimiterCharacter(Delimiter delimiter)
{
    switch (delimiter) {
    case Delimiter::Comma:
        return ',';
    case Delimiter::Tab:
        return '\t';
    case Delimiter::Semicolon:
        return ';';
    case Delimiter::Whitespace:
        return ' ';
    }
    return ',';
}

std::string quotedForMessage(std::string_view text, std::size_t limit)
{
    if (text.size() <= limit) {
        return "'" + std::string(text) + "'";
    }
    std::size_t cut = limit;
    // Back up over UTF-8 continuation bytes (10xxxxxx) so the cut falls between
    // characters; half a character in a message is invalid UTF-8 in the log.
    while (cut > 0 && (static_cast<unsigned char>(text[cut]) & 0xC0) == 0x80) {
        --cut;
    }
    return "'" + std::string(text.substr(0, cut)) + "...'";
}

// ---- RecordReader ------------------------------------------------------------------

RecordReader::RecordReader(std::string_view text, const DelimitedLayout& layout)
    : text_(text), layout_(layout)
{
}

bool RecordReader::atLineEnd() const
{
    return atEnd() || isLineEnd(text_[position_]);
}

void RecordReader::consumeLineEnd()
{
    if (atEnd()) {
        return;
    }
    const char c = text_[position_];
    ++position_;
    if (c == '\r' && !atEnd() && text_[position_] == '\n') {
        ++position_;
    }
    ++line_;
}

std::string_view RecordReader::restOfLine() const
{
    std::size_t end = position_;
    while (end < text_.size() && !isLineEnd(text_[end])) {
        ++end;
    }
    return text_.substr(position_, end - position_);
}

void RecordReader::skipLine()
{
    position_ += restOfLine().size();
    consumeLineEnd();
}

bool RecordReader::isBlank(char c) const
{
    // A tab is the field separator in a tab-delimited file, so it cannot also be
    // padding there.
    if (layout_.delimiter == Delimiter::Tab && c == '\t') {
        return false;
    }
    return isPadding(c);
}

bool RecordReader::isDelimiter(char c) const
{
    if (layout_.delimiter == Delimiter::Whitespace) {
        return isPadding(c);
    }
    return c == delimiterCharacter(layout_.delimiter);
}

Result<std::optional<Record>> RecordReader::next()
{
    while (!atEnd()) {
        if (line_ <= layout_.headerLines) {
            skipLine();
            continue;
        }
        const std::string_view content = katana::core::trimmed(restOfLine());
        if (content.empty()) {
            skipLine();
            continue;
        }
        if (!layout_.commentPrefix.empty() && content.starts_with(layout_.commentPrefix)) {
            ++commentLines_;
            skipLine();
            continue;
        }
        Result<Record> record = readRecord();
        if (!record) {
            return record.error();
        }
        return std::optional<Record>(std::move(record).value());
    }
    return std::optional<Record>{};
}

Result<Record> RecordReader::readRecord()
{
    Record record;
    record.line = line_;
    const bool runs = layout_.delimiter == Delimiter::Whitespace;
    const bool quoting = layout_.quoting == Quoting::DoubleQuote;

    while (true) {
        if (runs) {
            // Blanks between fields, and at either end of the line, separate
            // nothing more than one blank does.
            while (!atLineEnd() && isPadding(text_[position_])) {
                ++position_;
            }
            if (atLineEnd()) {
                break;
            }
        }
        const std::size_t column = record.fields.size() + 1;
        const std::size_t fieldStart = position_;
        // Blanks before an opening quote are allowed - `1, "Kerb, north"` is how
        // many writers pad after a comma - so look past them for one.
        std::size_t peek = position_;
        while (peek < text_.size() && isBlank(text_[peek]) && !isDelimiter(text_[peek])) {
            ++peek;
        }
        Field field;
        if (quoting && peek < text_.size() && text_[peek] == '"') {
            const std::size_t openedOn = line_;
            position_ = peek + 1;
            field.quoted = true;
            bool closed = false;
            while (!atEnd()) {
                const char c = text_[position_];
                if (c == '"') {
                    if (position_ + 1 < text_.size() && text_[position_ + 1] == '"') {
                        field.text += '"';
                        position_ += 2;
                        continue;
                    }
                    ++position_;
                    closed = true;
                    break;
                }
                if (isLineEnd(c)) {
                    // Kept as written: RFC 4180 lets a quoted field hold a line
                    // break, and a description is the one place it would.
                    const std::size_t before = position_;
                    consumeLineEnd();
                    field.text.append(text_.substr(before, position_ - before));
                    continue;
                }
                field.text += c;
                ++position_;
            }
            if (!closed) {
                return makeError(ErrorCode::ParseFailure,
                                 "line " + std::to_string(openedOn) + ", column " +
                                     std::to_string(column) +
                                     ": a quoted field opens here and is never closed");
            }
            while (!atLineEnd() && isBlank(text_[position_]) && !isDelimiter(text_[position_])) {
                ++position_;
            }
            if (!atLineEnd() && !isDelimiter(text_[position_])) {
                return makeError(ErrorCode::ParseFailure,
                                 "line " + std::to_string(line_) + ", column " +
                                     std::to_string(column) +
                                     ": text follows the closing quote of a quoted field (" +
                                     quotedForMessage(restOfLine()) + ")");
            }
        } else {
            position_ = fieldStart;
            while (!atLineEnd() && !isDelimiter(text_[position_])) {
                ++position_;
            }
            field.text.assign(text_.substr(fieldStart, position_ - fieldStart));
        }
        record.fields.push_back(std::move(field));

        if (atLineEnd()) {
            break;
        }
        // At a delimiter. With runs, the loop head steps over the whole run; with
        // a single-character delimiter exactly one is consumed, so "a,,b" has an
        // empty middle field and "a,b," an empty last one.
        if (!runs) {
            ++position_;
        }
    }
    consumeLineEnd();
    return record;
}

// ---- splitLine ---------------------------------------------------------------------

std::vector<Field> splitLine(std::string_view line, Delimiter delimiter)
{
    DelimitedLayout layout;
    layout.delimiter = delimiter;
    layout.quoting = Quoting::DoubleQuote;
    // The line is handed over without its terminator, so the reader sees one
    // record. A quote left open would otherwise be an error; for the search it
    // is only a line that does not fit, so it is closed at the line's end.
    RecordReader reader(line, layout);
    Result<std::optional<Record>> record = reader.next();
    if (record && record.value()) {
        return std::move(*record.value()).fields;
    }
    // Unclosed quote, or text after one: split without quoting instead, which
    // gives the search a field count to judge rather than nothing.
    layout.quoting = Quoting::None;
    RecordReader plain(line, layout);
    Result<std::optional<Record>> unquoted = plain.next();
    if (unquoted && unquoted.value()) {
        return std::move(*unquoted.value()).fields;
    }
    return {};
}

} // namespace katana::surveyio::delimited
