#pragma once

// A 12d linestyle or symbol library (`.4d`) -> entity::StyleLibrary.
//
// One grammar covers both files, because in 12d a symbol is a linestyle drawn
// at a vertex. A definition is a block:
//
//   worldstyle "TEST Bollard" {
//       xorigin 0   yorigin 0
//       mode   vertex                 // this one is a symbol
//       group  "Test/Bollards"
//       move 0 0
//       arc 1.75 0 180
//       draw 1.2 -0.4
//   }
//
// `paperstyle` sizes are plot millimetres, `worldstyle` sizes are model units,
// `twoptstyle` is stretched between two anchors. Everything else is a stroke
// or a setting; see entity::LineStyle for what each means.
//
// The text must already be UTF-8: run the bytes through decodeText first, as
// the Archive reader does. In the reference customisation the symbol library
// is UTF-16LE while the linestyle library beside it is plain ASCII, so a
// reader that assumed either one would read half the customisation.
//
// Like readArchive, this reports rather than guesses: every keyword it does
// not know is counted and named, so "did it take all of it?" has a list for
// an answer instead of a shrug.

#include <cstddef>
#include <string>
#include <string_view>
#include <vector>

#include "katana/archive12d/domain.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/style_library.hpp"

namespace katana::archive12d {

struct StyleLibraryRead {
    katana::entity::StyleLibrary library{};
    // By keyword: how many were read, and how many were kept. A `move` that
    // was kept counts in both; a keyword with no member counts only in `read`.
    std::vector<ElementTally> tally{};
    std::vector<std::string> warnings{};
    // Definitions that replaced one of the same name. Four of the reference
    // customisation's definitions are given twice - three in both library
    // files and one twice in one - so this is expected and is reported rather
    // than treated as an error.
    std::size_t replaced = 0;
    // Each definition that count stands for, AS IT WAS before the one of this
    // text took its place, in the order they were replaced. A count says that
    // a name was given twice and not which, nor whether the two differ - and
    // one that differs is a definition somebody drew and nobody will see
    // (the converter of these files names each: legacy/convert.hpp).
    std::vector<katana::entity::LineStyle> replacedDefinitions{};
    // The `//` header, which is where these files carry their licence.
    std::vector<std::string> comments{};
};

// Fails only when the text cannot be read at all - an unterminated quote, a
// block that never closes. A definition with something wrong in it is skipped
// with a warning, so one bad block does not cost the other 791.
//
// `sourceName` is the name of the file the text came from, and is stamped on
// every definition read as LineStyle::source - it is how a browser says where
// a definition came from. It also decides LineStyle::symbol, which the text
// itself cannot say: a definition is listed as a symbol when that name holds
// "symbol", whatever its case (most symbols the reference mapfiles use are
// not `mode vertex`; they are symbols because they live in the symbol file).
// Only the NAME is kept: if a path is passed, everything up to its last
// separator is dropped, so a library never carries where on someone's disk it
// was loaded from. Empty for text that came from no file, and the definitions
// then say so by carrying none - and none of them is listed as a symbol.
[[nodiscard]] katana::core::Result<StyleLibraryRead> readStyleLibrary(std::string_view text,
                                                                      std::string_view sourceName = {});

// Reads into an existing library, so a customisation made of several files
// loads as one and the later file wins. The tally and warnings describe THIS
// text only, and only the definitions of this text are stamped with
// `sourceName`: the ones already in `library` keep the file they came from.
[[nodiscard]] katana::core::Result<StyleLibraryRead>
readStyleLibraryInto(katana::entity::StyleLibrary library, std::string_view text,
                     std::string_view sourceName = {});

// The file name part of `source`: what follows its last `/` or `\`. What
// readStyleLibrary stamps, exposed so a caller comparing against a stamp
// computes it the same way.
[[nodiscard]] std::string sourceFileName(std::string_view source);

// ---- writing ----------------------------------------------------------------------------------

struct StyleLibraryWriteOptions {
    // The definitions to write, by name; empty means every one. A name the
    // library does not hold fails the write and is named, rather than being
    // left out of a file the person believes holds it.
    std::vector<std::string> names{};
    // `//` lines at the head of the file, which is where 12d's own libraries
    // carry their licence and where readStyleLibrary finds them again
    // (StyleLibraryRead::comments). A line may not hold a line break: the
    // rest of it would be read as definitions.
    std::vector<std::string> comments{};
};

// A library -> `.4d` text, UTF-8, in name order, one command per line as 12d
// writes it. Every field of a definition is written - all three kinds, `mode
// vertex`, group, length, factor, origins, anchors, stretch and cycle mode,
// pens and every stroke, a text with the three numbers kept but not
// understood - so that readStyleLibrary(writeStyleLibrary(l), name) gives
// back `l` when every definition of `l` came from a file of that name.
// Two things a file does not hold, because they are said by the name of the
// file itself and are set again by the reader from the name it is given:
// LineStyle::source, and LineStyle::symbol (see `sourceName` above). Read back
// under no name, or under another, a definition differs in those two and in
// nothing else.
//
// Numbers are written as the shortest plain decimal that reads back exactly:
// never an exponent, because 12d's own files never use one and nothing here
// says 12d reads one. Fails, naming the definition, for a definition
// validate() refuses - the reader would skip it, so writing it would lose it.
[[nodiscard]] katana::core::Result<std::string>
writeStyleLibrary(const katana::entity::StyleLibrary& library,
                  const StyleLibraryWriteOptions& options = {});

} // namespace katana::archive12d
