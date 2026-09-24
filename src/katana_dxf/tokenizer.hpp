#pragma once

// DXF text as a stream of (group code, value) pairs.
//
// SPEED IS THE DESIGN. A DXF is two lines per pair, and a survey drawing is
// millions of pairs, so nothing here allocates: a pair's value is a view into
// the file, found with memchr, and a number is parsed only when an entity
// asks for it, with std::from_chars - the way the archive12d reader does it
// (docs/performance.md). GDAL's reader built a feature per entity; this
// builds nothing.

#include <cstddef>
#include <cstdint>
#include <optional>
#include <string>
#include <string_view>

namespace katana::dxf::detail {

struct Pair {
    int code = -1;
    std::string_view value;
};

class Tokenizer {
  public:
    explicit Tokenizer(std::string_view text) : text_(text) {}

    // The next pair, skipping 999 comments. False at the end of the text,
    // and when a group code line is not a number (failed() then says where).
    bool next(Pair& out);

    // One pair of lookahead: the next call to next() returns it again.
    void pushBack(const Pair& pair)
    {
        pending_ = pair;
        hasPending_ = true;
    }

    [[nodiscard]] bool failed() const { return !failure_.empty(); }
    [[nodiscard]] const std::string& failure() const { return failure_; }
    // True when the text ended between a group code and its value.
    [[nodiscard]] bool truncated() const { return truncated_; }
    // One-based line of the last group code read.
    [[nodiscard]] std::size_t line() const { return codeLine_; }

  private:
    // The next line without its terminator, LF or CRLF. Lone CRs (classic
    // Mac OS) are not line ends here: such a file reads as one line and fails
    // as not a DXF, which is said, rather than being half understood.
    bool readLine(std::string_view& out);

    std::string_view text_;
    std::size_t position_ = 0;
    std::size_t lineNumber_ = 0;
    std::size_t codeLine_ = 0;
    Pair pending_;
    bool hasPending_ = false;
    bool truncated_ = false;
    std::string failure_;
};

// Blanks off both ends: numbers are written right-aligned in their field.
[[nodiscard]] std::string_view trimmedValue(std::string_view value);

// A value as a number; nullopt when it is not one or is not finite. A leading
// '+' is accepted, since some writers put one there.
[[nodiscard]] std::optional<double> toReal(std::string_view value);
[[nodiscard]] std::optional<std::int64_t> toInteger(std::string_view value);
// A handle: hexadecimal.
[[nodiscard]] std::optional<std::uint64_t> toHandle(std::string_view value);

} // namespace katana::dxf::detail
