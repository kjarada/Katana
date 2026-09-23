#pragma once

// 12da text -> Archive.
//
// The manual describes the format as free-form: tokens separated by white
// space, `//` to the end of the line a comment, text in double quotes with
// `\"` and `\\` as escapes, keywords in any case. So this is a tokenizer and a
// recursive-descent reader, NOT a line-oriented one - `chainage 0 interval 1
// radius 15` on one line and the same three pairs on three lines are the same
// file, and the manual's own examples use both.
//
// WHAT IS AN ERROR AND WHAT IS NOT. A file this reader cannot make structural
// sense of is a ParseFailure naming the line: a brace that never closes, a
// data block whose values do not divide into rows, a number that is not one,
// a triangle naming a point that does not exist. Vocabulary it does not know
// is NOT an error, because 12d Model writes a great deal the manual does not
// describe: an unknown block is skipped whole and counted in
// `Archive::unrecognised`, an unknown scalar is kept in the element's
// `extras`. The line between the two is whether carrying on could produce
// WRONG geometry (stop) or merely less of it (continue, and say so).

#include <cstddef>
#include <string_view>

#include "katana/archive12d/archive.hpp"
#include "katana/core/error.hpp"

namespace katana::archive12d {

struct ReadOptions {
    // A file claiming more elements, or one data block claiming more values,
    // than this is refused rather than allowed to exhaust memory. The defaults
    // are an order of magnitude above the largest real archive seen (a 58 MB
    // export with 2 400 strings and a 233 000-triangle tin).
    std::size_t maxElements = 5'000'000;
    std::size_t maxValuesPerBlock = 200'000'000;
    // Nesting beyond this is refused. Real exports reach 8 (attribute groups
    // inside drawables inside a super alignment); the limit exists so that a
    // file of a million `{` cannot overflow the stack.
    std::size_t maxDepth = 64;
};

// `text` is UTF-8 - see katana/core/text_encoding.hpp for getting there from a file.
[[nodiscard]] katana::core::Result<Archive> readArchive(std::string_view text,
                                                        const ReadOptions& options = {});

// Decodes, then reads. The encoding that was detected is added to the
// archive's warnings when it had to be guessed.
[[nodiscard]] katana::core::Result<Archive> readArchiveBytes(std::string_view bytes,
                                                             const ReadOptions& options = {});

// A number as 12d writes them: decimal, exponent, or a C99 hexadecimal float
// (`0x1.6d4014ac08318p+17`) - which is how 12d Model writes every tin by
// default, because it round-trips a double exactly. nullopt for anything else,
// including infinities and NaN, which no coordinate may be.
[[nodiscard]] std::optional<double> parseReal(std::string_view token);

} // namespace katana::archive12d
