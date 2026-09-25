#include "katana/ifc/step.hpp"

#include <array>
#include <charconv>
#include <cmath>
#include <cstdio>
#include <system_error>

namespace katana::ifc {

namespace {

// IfcGloballyUniqueId's alphabet, from the IFC documentation of the type.
constexpr std::string_view kGuidAlphabet =
    "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";

// FNV-1a, 64 bit: offset basis 14695981039346656037 and prime 1099511628211
// (Fowler, Noll and Vo's published parameters).
constexpr std::uint64_t kFnvPrime = 1099511628211ULL;

std::uint64_t fnv1a(std::uint64_t hash, std::string_view bytes)
{
    for (const char c : bytes) {
        hash ^= static_cast<unsigned char>(c);
        hash *= kFnvPrime;
    }
    return hash;
}

// SplitMix64's finaliser (Steele, Lea and Flood, "Fast splittable
// pseudorandom number generators", 2014): FNV-1a's high bits change little
// for keys that differ in their last characters - "entity/41" and
// "entity/42" - and the GlobalId's first characters are its high bits.
std::uint64_t mix(std::uint64_t z)
{
    z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL;
    z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL;
    return z ^ (z >> 31);
}

// One code point from `text` at `at`, advancing it; U+FFFD for a byte that
// does not begin a well-formed sequence, consuming just that byte.
char32_t nextCodePoint(std::string_view text, std::size_t& at)
{
    const auto byte = [&](std::size_t i) { return static_cast<unsigned char>(text[i]); };
    const unsigned char lead = byte(at);
    std::size_t length = 0;
    char32_t value = 0;
    if (lead < 0x80) {
        ++at;
        return lead;
    }
    if ((lead & 0xE0) == 0xC0) {
        length = 2;
        value = lead & 0x1F;
    } else if ((lead & 0xF0) == 0xE0) {
        length = 3;
        value = lead & 0x0F;
    } else if ((lead & 0xF8) == 0xF0) {
        length = 4;
        value = lead & 0x07;
    } else {
        ++at;
        return 0xFFFD;
    }
    if (at + length > text.size()) {
        ++at;
        return 0xFFFD;
    }
    for (std::size_t i = 1; i < length; ++i) {
        if ((byte(at + i) & 0xC0) != 0x80) {
            ++at;
            return 0xFFFD;
        }
        value = (value << 6) | (byte(at + i) & 0x3F);
    }
    // Overlong forms, surrogates and values past U+10FFFF are not UTF-8.
    constexpr std::array<char32_t, 5> kMinimum{0, 0, 0x80, 0x800, 0x10000};
    if (value < kMinimum[length] || value > 0x10FFFF || (value >= 0xD800 && value <= 0xDFFF)) {
        ++at;
        return 0xFFFD;
    }
    at += length;
    return value;
}

void appendHex(std::string& out, std::uint32_t value, int digits)
{
    constexpr std::string_view kHex = "0123456789ABCDEF";
    for (int shift = (digits - 1) * 4; shift >= 0; shift -= 4) {
        out.push_back(kHex[(value >> shift) & 0xF]);
    }
}

} // namespace

std::string compressGuid(std::uint64_t high, std::uint64_t low)
{
    std::array<std::uint8_t, 16> bytes{};
    for (int i = 0; i < 8; ++i) {
        bytes[i] = static_cast<std::uint8_t>(high >> (56 - 8 * i));
        bytes[8 + i] = static_cast<std::uint8_t>(low >> (56 - 8 * i));
    }
    std::string out;
    out.reserve(22);
    const auto encode = [&](std::uint32_t value, int characters) {
        for (int i = characters - 1; i >= 0; --i) {
            out.push_back(kGuidAlphabet[(value >> (6 * i)) & 0x3F]);
        }
    };
    encode(bytes[0], 2);
    for (std::size_t i = 1; i < 16; i += 3) {
        encode((std::uint32_t{bytes[i]} << 16) | (std::uint32_t{bytes[i + 1]} << 8) | bytes[i + 2],
               4);
    }
    return out;
}

std::string guidFor(std::string_view space, std::string_view key)
{
    // Two FNV-1a streams from different starting points over the same
    // bytes; the zero byte keeps ("ab", "c") and ("a", "bc") apart.
    const auto hash = [&](std::uint64_t basis) {
        std::uint64_t h = fnv1a(basis, space);
        h = fnv1a(h, std::string_view("\0", 1));
        return mix(fnv1a(h, key));
    };
    return compressGuid(hash(14695981039346656037ULL), hash(0x6A09E667F3BCC908ULL));
}

std::string stepString(std::string_view utf8)
{
    std::string out = "'";
    out.reserve(utf8.size() + 2);
    std::size_t at = 0;
    bool inWide = false; // inside a \X2\ run
    while (at < utf8.size()) {
        const char32_t c = nextCodePoint(utf8, at);
        if (c >= 0x20 && c < 0x7F) {
            if (inWide) {
                out += "\\X0\\";
                inWide = false;
            }
            if (c == '\'') {
                out += "''";
            } else if (c == '\\') {
                out += "\\\\";
            } else {
                out.push_back(static_cast<char>(c));
            }
        } else if (c <= 0xFFFF) {
            if (!inWide) {
                out += "\\X2\\";
                inWide = true;
            }
            appendHex(out, static_cast<std::uint32_t>(c), 4);
        } else {
            if (inWide) {
                out += "\\X0\\";
                inWide = false;
            }
            out += "\\X4\\";
            appendHex(out, static_cast<std::uint32_t>(c), 8);
            out += "\\X0\\";
        }
    }
    if (inWide) {
        out += "\\X0\\";
    }
    out.push_back('\'');
    return out;
}

std::string stepReal(double value)
{
    if (!std::isfinite(value)) {
        return "$";
    }
    if (value == 0.0) {
        return "0.";
    }
    std::array<char, 32> buffer{};
    const auto [end, error] = std::to_chars(buffer.data(), buffer.data() + buffer.size(), value);
    if (error != std::errc{}) {
        return "$";
    }
    const std::string_view text(buffer.data(), static_cast<std::size_t>(end - buffer.data()));
    const std::size_t e = text.find('e');
    std::string mantissa(text.substr(0, e));
    if (mantissa.find('.') == std::string::npos) {
        mantissa += '.';
    }
    if (e == std::string_view::npos) {
        return mantissa;
    }
    return mantissa + "E" + std::string(text.substr(e + 1));
}

std::size_t characterCount(std::string_view utf8)
{
    std::size_t count = 0;
    std::size_t at = 0;
    while (at < utf8.size()) {
        nextCodePoint(utf8, at);
        ++count;
    }
    return count;
}

} // namespace katana::ifc
