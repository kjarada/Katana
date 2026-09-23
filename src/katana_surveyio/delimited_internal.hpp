#pragma once

// Private to the delimited point format: the record reader that the parser, the
// layout proposal and the detection probe all use, so that "what fields does
// this line hold" has one answer. If the proposal split lines one way and the
// parser another, a proposal could be confident about a layout the parser then
// reads differently.

#include <cstddef>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/surveyio/delimited_points.hpp"

namespace katana::surveyio::delimited {

struct Field {
    std::string text;    // unquoted fields NOT yet trimmed; see valueOf()
    bool quoted = false; // the field was written "..." and is kept exactly
};

struct Record {
    std::vector<Field> fields;
    std::size_t line = 0; // the physical line the record starts on, 1-based
};

// What a field means once quoting is taken into account: an unquoted field is
// trimmed of blanks (padding in a column-aligned file is not content), a quoted
// one is exactly what was between the quotes.
[[nodiscard]] std::string_view valueOf(const Field& field);

// Reads records one at a time from decoded UTF-8 text.
//
// Header lines, blank lines and comment lines are stepped over here, so that
// every consumer skips the same lines. The reader does not know what a column
// means; it only splits.
class RecordReader {
  public:
    // `text` must outlive the reader. `layout` is copied: only its delimiter,
    // quoting, header count and comment prefix are used.
    RecordReader(std::string_view text, const DelimitedLayout& layout);

    // The next record, nullopt at the end of the text, or ParseFailure for a
    // quoted field that never closes or text after a closing quote - naming the
    // line and the column. After an error the reader is not to be used again.
    [[nodiscard]] katana::core::Result<std::optional<Record>> next();

    [[nodiscard]] std::size_t commentLines() const { return commentLines_; }

  private:
    [[nodiscard]] bool atEnd() const { return position_ >= text_.size(); }
    [[nodiscard]] bool atLineEnd() const;
    // Steps over one line terminator (CR, LF or CRLF) and counts the line.
    void consumeLineEnd();
    // The rest of the current physical line, without its terminator.
    [[nodiscard]] std::string_view restOfLine() const;
    void skipLine();
    [[nodiscard]] bool isBlank(char c) const;
    [[nodiscard]] bool isDelimiter(char c) const;
    [[nodiscard]] katana::core::Result<Record> readRecord();

    std::string_view text_;
    DelimitedLayout layout_;
    std::size_t position_ = 0;
    std::size_t line_ = 1; // the physical line `position_` is on
    std::size_t commentLines_ = 0;
};

// Splits ONE physical line into fields with `delimiter` and double-quote
// quoting, for the proposal's delimiter search, which has to try each delimiter
// on the same lines. A line the quote-aware split cannot read - a quote left
// open, text after a closing one - is split again with quoting off, so that the
// search still has a field count to judge; whether the parser will accept the
// line is the parser's to say, with its line and column.
[[nodiscard]] std::vector<Field> splitLine(std::string_view line, Delimiter delimiter);

// The character a delimiter is written as. Whitespace is written as one space.
[[nodiscard]] char delimiterCharacter(Delimiter delimiter);

// ---- The proposal's working, shared with the detection probe ----------------------

// `bytes` decoded to UTF-8 as core::decodeText decodes them. When `truncated`
// (a detection sample), the bytes may stop inside a character or a line: up to
// three trailing bytes are dropped until the rest decodes as what it evidently
// is, rather than letting a character cut in half turn a UTF-8 sample into a
// Windows-1252 one, and then the last line, which may be cut, is dropped.
[[nodiscard]] katana::core::Result<std::string> decodeSample(std::string_view bytes,
                                                             bool truncated);

struct Analysis {
    LayoutProposal proposal;
    // The delimiter that split the rows into numbers; nullopt when none did.
    std::optional<Delimiter> delimiter;
    // A header line names the horizontal coordinate columns - as northing and
    // easting, or as X and Y. What the probe reports as the stronger evidence
    // that this is a point file, whether or not the order was decided.
    bool headerNamesCoordinates = false;
};

// Everything proposeLayout() does after decoding.
[[nodiscard]] Analysis analyse(std::string_view text);

// `text` shortened to at most `limit` bytes at a UTF-8 character boundary, with
// "..." when it was cut, and in single quotes - for quoting a field's value back
// in an error message without pasting a whole malformed line into it.
[[nodiscard]] std::string quotedForMessage(std::string_view text, std::size_t limit = 40);

} // namespace katana::surveyio::delimited
