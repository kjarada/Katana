#pragma once

// Every element of the 12d Archive format, and what Katana does with it.
//
// "Make sure all elements are accounted for" is only a checkable claim if the
// list of elements is written down somewhere a test can reach. This is that
// list: one row per element the 12d Model V15 manual defines, in the manual's
// order, with its section. `tests/archive12d/test_coverage.cpp` writes a
// minimal instance of each row, reads it, and fails if the reader reports the
// block as unrecognised - so a row cannot be added here without the reader
// learning the element, and the table in docs/interop.md is generated from
// the same rows rather than maintained beside them.

#include <string_view>
#include <vector>

namespace katana::archive12d {

enum class Handling {
    Full,      // read, imported into the domain model, and written on export
    ImportOnly, // read and imported; export has no source for it
    ReadOnly,  // read into the Archive; the domain model has nowhere to put it
};

struct ElementCoverage {
    std::string_view keyword; // as it appears in a file: "string super", "full_tin"
    std::string_view manualSection;
    Handling handling;
    std::string_view becomes; // what it is in Katana, or why it is not
};

[[nodiscard]] const std::vector<ElementCoverage>& elementCoverage();

[[nodiscard]] std::string_view toString(Handling handling);

} // namespace katana::archive12d
