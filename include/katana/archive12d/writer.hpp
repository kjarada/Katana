#pragma once

// Archive -> 12da text.
//
// The writer produces the forms the manual documents for the CURRENT element
// types: every list of vertices is written as a `string super`, every
// alignment as a `string super_alignment`. The superseded types (2d, 3d, 4d,
// pipe, polyline, alignment, pipeline) are read but never written, which is
// what 12d Model itself does.
//
// What is read is written back: `readArchive(writeArchive(a))` equals `a` for
// any archive made of the documented elements, with the superseded kinds
// arriving as the super strings they were written as. That property is tested,
// and it is what "every element accounted for" means on the way out.

#include <string>

#include "katana/archive12d/archive.hpp"

namespace katana::archive12d {

struct WriteOptions {
    // Digits after the decimal point for coordinates and other reals. 12d
    // Model's own default is 8 (`// decimal_places 8`). Trailing zeros are
    // trimmed, so 8 costs nothing for round numbers.
    int decimalPlaces = 8;
    // Write tin points as C99 hexadecimal floats, as 12d Model does by default
    // (`output_tin_hex_floats true`): a double written in hex reads back
    // bit for bit, which decimal at 8 places does not.
    //
    // ON by default, because a tin's coordinates are not numbers a user typed
    // - they are computed, and the SIGN OF A TRIANGLE'S AREA depends on their
    // last bits. Eight places moves a point by up to 5e-9, which is enough to
    // turn a sliver triangle inside out; a surface with one inside-out
    // triangle is refused whole. Measured on plot_PW_example_data.12da: four
    // of its eight surfaces would not read back.
    bool hexFloatTins = true;
    // Comment written at the head of the file, one line per line of text.
    std::string banner = "Written by Katana";
};

// The text is UTF-8. It never fails: an Archive cannot hold anything the
// format has no way to say.
[[nodiscard]] std::string writeArchive(const Archive& archive, const WriteOptions& options = {});

// A name, colour or text value as it must appear in the file: in double
// quotes, with `"` and `\` escaped. Exposed because the rule is easy to get
// subtly wrong and is tested on its own.
[[nodiscard]] std::string quoted(std::string_view text);

} // namespace katana::archive12d
