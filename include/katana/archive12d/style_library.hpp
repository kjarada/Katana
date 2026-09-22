#pragma once

// A 12d linestyle or symbol library (`.4d`) -> entity::StyleLibrary.
//
// One grammar covers both files, because in 12d a symbol is a linestyle drawn
// at a vertex. A definition is a block:
//
//   worldstyle "CULT Bollard" {
//       xorigin 0   yorigin 0
//       mode   vertex                 // this one is a symbol
//       group  "TfNSW Survey/CULT"
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
// the Archive reader does. The Transport for NSW symbol library is UTF-16LE
// and its linestyle library is plain ASCII, so a reader that assumed either
// one would read half the customisation.
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
    // Definitions that replaced one of the same name. Four of the Transport
    // for NSW definitions appear in both library files, so this is expected
    // and is reported rather than treated as an error.
    std::size_t replaced = 0;
    // The `//` header, which is where these files carry their licence.
    std::vector<std::string> comments{};
};

// Fails only when the text cannot be read at all - an unterminated quote, a
// block that never closes. A definition with something wrong in it is skipped
// with a warning, so one bad block does not cost the other 791.
[[nodiscard]] katana::core::Result<StyleLibraryRead> readStyleLibrary(std::string_view text);

// Reads into an existing library, so a customisation made of several files
// loads as one and the later file wins. The tally and warnings describe THIS
// text only.
[[nodiscard]] katana::core::Result<StyleLibraryRead>
readStyleLibraryInto(katana::entity::StyleLibrary library, std::string_view text);

} // namespace katana::archive12d
