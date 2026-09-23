#pragma once

// Small text helpers shared by the reader, the writer and the domain mapping.
// Internal to the module.

#include <array>
#include <cstddef>
#include <cstdint>
#include <optional>
#include <string>
#include <string_view>

#include "katana/core/text.hpp"

namespace katana::archive12d::detail {

// The locale-independent helpers every text reader shares; see
// katana/core/text.hpp for why they must not consult the C locale.
using katana::core::equalsIgnoringCase;
using katana::core::lowered;
using katana::core::trimmed;

// Folds a token to lower case WITHOUT allocating, for the common case where
// the folded text is compared and then thrown away. The reader asks this of
// every keyword it meets: 2.4 million times in a 63 MB archive, and half a
// million of those tokens are longer than a std::string holds internally, so
// `lowered` pays a heap allocation for each.
//
// The view returned is valid until the NEXT call on the same buffer. A
// function needing two folded tokens at once therefore needs two buffers, and
// the buffer belongs on the stack of the function that reads it rather than
// to the reader, so that a nested block cannot overwrite an enclosing one's.
class CaseBuffer {
  public:
    [[nodiscard]] std::string_view lower(std::string_view text);

  private:
    // The longest keyword in the format is `segment_attribute_data`, 22
    // characters. 64 covers that with room for whatever a file names that
    // this does not know; anything longer still folds, through `spill_`.
    std::array<char, 64> inline_{};
    std::string spill_;
};

// Hash and compare text the way the 12da format does (manual 1.1: case is
// stored but not compared), so that an unordered container can be asked with
// the token as it was written and no folded copy of it.
struct CaseFoldedHash {
    using is_transparent = void;
    [[nodiscard]] std::size_t operator()(std::string_view text) const noexcept;
};
struct CaseFoldedEqual {
    using is_transparent = void;
    [[nodiscard]] bool operator()(std::string_view a, std::string_view b) const
    {
        return equalsIgnoringCase(a, b);
    }
};

// Decimal integers only, with one optional sign. nullopt for anything else -
// including a real, which must not be silently truncated into an index.
using katana::core::parseInteger;

// The manual's rule (1.5.8): true is 1, or a word starting with T, t, Y or y;
// false is 0, or a word starting with F, f, N or n. 12d Model also writes 2
// for "visible" in some per-vertex flags, and any other non-zero integer is
// read as true for the same reason.
[[nodiscard]] std::optional<bool> parseBoolean(std::string_view token);

// A real with at most `decimalPlaces` digits after the point and trailing
// zeros removed: 502000, 30.45, -0.001. Never an exponent - 12d Model reads
// them, but a coordinate written as 5.02e+05 is unreadable to a person, and a
// 12da is a file people open.
[[nodiscard]] std::string formatReal(double value, int decimalPlaces);

// C99 hexadecimal float, `0x1.6d4014ac08318p+17`: exact for every double.
[[nodiscard]] std::string formatHexReal(double value);

// Shortest decimal text that reads back as exactly `value`.
using katana::core::formatExactReal;

} // namespace katana::archive12d::detail
