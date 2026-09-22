#pragma once

// Small text helpers shared by the reader, the writer and the domain mapping.
// Internal to the module.

#include <cstdint>
#include <optional>
#include <string>
#include <string_view>

namespace katana::archive12d::detail {

[[nodiscard]] std::string lowered(std::string_view text);
[[nodiscard]] bool equalsIgnoringCase(std::string_view a, std::string_view b);
[[nodiscard]] std::string_view trimmed(std::string_view text);

// Decimal integers only, with an optional sign. nullopt for anything else -
// including a real, which must not be silently truncated into an index.
[[nodiscard]] std::optional<std::int64_t> parseInteger(std::string_view token);

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
[[nodiscard]] std::string formatExactReal(double value);

} // namespace katana::archive12d::detail
