#include "katana/archive12d/text_encoding.hpp"

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>

#include "katana/entity/entity.hpp"

namespace katana::archive12d {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;

// Bytes examined when there is no byte order mark. A 12da begins with its
// header comments and the first model, so 4 KB is already hundreds of ASCII
// characters; more would only slow a wrong guess down.
constexpr std::size_t kSniffBytes = 4096;

// Windows-1252 differs from Latin-1 only in 0x80-0x9F. Source: the Unicode
// Consortium's mapping table for CP1252 (MAPPINGS/VENDORS/MICSFT/WINDOWS/
// CP1252.TXT). The five undefined bytes map to the C1 control of the same
// value, as Windows itself does.
constexpr std::array<std::uint16_t, 32> kWindows1252High = {
    0x20AC, 0x0081, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160,
    0x2039, 0x0152, 0x008D, 0x017D, 0x008F, 0x0090, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022,
    0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x009D, 0x017E, 0x0178};

void appendUtf8(std::string& out, std::uint32_t codePoint)
{
    if (codePoint < 0x80) {
        out += static_cast<char>(codePoint);
    } else if (codePoint < 0x800) {
        out += static_cast<char>(0xC0 | (codePoint >> 6));
        out += static_cast<char>(0x80 | (codePoint & 0x3F));
    } else if (codePoint < 0x10000) {
        out += static_cast<char>(0xE0 | (codePoint >> 12));
        out += static_cast<char>(0x80 | ((codePoint >> 6) & 0x3F));
        out += static_cast<char>(0x80 | (codePoint & 0x3F));
    } else {
        out += static_cast<char>(0xF0 | (codePoint >> 18));
        out += static_cast<char>(0x80 | ((codePoint >> 12) & 0x3F));
        out += static_cast<char>(0x80 | ((codePoint >> 6) & 0x3F));
        out += static_cast<char>(0x80 | (codePoint & 0x3F));
    }
}

katana::core::Result<std::string> fromUtf16(std::string_view bytes, bool littleEndian)
{
    if (bytes.size() % 2 != 0) {
        return makeError(ErrorCode::ParseFailure,
                         "the file is UTF-16 but has an odd number of bytes, so it is truncated");
    }
    const auto unit = [&](std::size_t index) -> std::uint32_t {
        const auto a = static_cast<unsigned char>(bytes[index]);
        const auto b = static_cast<unsigned char>(bytes[index + 1]);
        return littleEndian ? static_cast<std::uint32_t>(a | (b << 8))
                            : static_cast<std::uint32_t>((a << 8) | b);
    };

    std::string out;
    // A 12da is almost entirely ASCII, which halves in UTF-8.
    out.reserve(bytes.size() / 2 + 16);
    for (std::size_t i = 0; i < bytes.size(); i += 2) {
        std::uint32_t codePoint = unit(i);
        if (codePoint >= 0xD800 && codePoint <= 0xDBFF) {
            if (i + 3 >= bytes.size()) {
                return makeError(ErrorCode::ParseFailure,
                                 "the file ends in the middle of a UTF-16 surrogate pair");
            }
            const std::uint32_t low = unit(i + 2);
            if (low < 0xDC00 || low > 0xDFFF) {
                return makeError(ErrorCode::ParseFailure,
                                 "malformed UTF-16: a high surrogate with no low surrogate",
                                 "byte " + std::to_string(i));
            }
            codePoint = 0x10000 + ((codePoint - 0xD800) << 10) + (low - 0xDC00);
            i += 2;
        } else if (codePoint >= 0xDC00 && codePoint <= 0xDFFF) {
            return makeError(ErrorCode::ParseFailure,
                             "malformed UTF-16: a low surrogate with no high surrogate",
                             "byte " + std::to_string(i));
        }
        appendUtf8(out, codePoint);
    }
    return out;
}

std::string fromWindows1252(std::string_view bytes)
{
    std::string out;
    out.reserve(bytes.size() + bytes.size() / 8);
    for (const char ch : bytes) {
        const auto byte = static_cast<unsigned char>(ch);
        if (byte < 0x80) {
            out += ch;
        } else if (byte < 0xA0) {
            appendUtf8(out, kWindows1252High[byte - 0x80]);
        } else {
            appendUtf8(out, byte); // 0xA0-0xFF are Latin-1, which is Unicode's first page
        }
    }
    return out;
}

} // namespace

const char* toString(TextEncoding encoding)
{
    switch (encoding) {
    case TextEncoding::Utf8:
        return "UTF-8";
    case TextEncoding::Utf8WithBom:
        return "UTF-8 with a byte order mark";
    case TextEncoding::Utf16LittleEndian:
        return "UTF-16 little-endian";
    case TextEncoding::Utf16BigEndian:
        return "UTF-16 big-endian";
    case TextEncoding::Windows1252:
        return "Windows-1252";
    }
    return "unknown";
}

katana::core::Result<DecodedText> decodeText(std::string_view bytes)
{
    const auto byteAt = [&](std::size_t index) {
        return index < bytes.size() ? static_cast<unsigned char>(bytes[index]) : 0u;
    };

    DecodedText decoded;
    if (bytes.size() >= 2 && byteAt(0) == 0xFF && byteAt(1) == 0xFE) {
        auto text = fromUtf16(bytes.substr(2), true);
        if (!text) {
            return text.error();
        }
        decoded.text = std::move(*text);
        decoded.encoding = TextEncoding::Utf16LittleEndian;
        return decoded;
    }
    if (bytes.size() >= 2 && byteAt(0) == 0xFE && byteAt(1) == 0xFF) {
        auto text = fromUtf16(bytes.substr(2), false);
        if (!text) {
            return text.error();
        }
        decoded.text = std::move(*text);
        decoded.encoding = TextEncoding::Utf16BigEndian;
        return decoded;
    }
    if (bytes.size() >= 3 && byteAt(0) == 0xEF && byteAt(1) == 0xBB && byteAt(2) == 0xBF) {
        decoded.text = std::string(bytes.substr(3));
        decoded.encoding = TextEncoding::Utf8WithBom;
        if (!katana::entity::isValidUtf8(decoded.text)) {
            return makeError(ErrorCode::ParseFailure,
                             "the file is marked as UTF-8 but contains bytes that are not");
        }
        return decoded;
    }

    // No mark. ASCII as UTF-16 has a NUL in every other byte; nothing else a
    // 12da could be has any. A quarter of the sample is far above what stray
    // NULs in a damaged UTF-8 file would reach and far below the half that
    // real UTF-16 text shows, so the threshold is not delicate.
    const std::size_t sample = std::min(bytes.size(), kSniffBytes);
    std::size_t nulsEven = 0;
    std::size_t nulsOdd = 0;
    for (std::size_t i = 0; i < sample; ++i) {
        if (bytes[i] == '\0') {
            (i % 2 == 0 ? nulsEven : nulsOdd) += 1;
        }
    }
    decoded.guessed = true;
    if (sample >= 8 && nulsOdd > sample / 4 && nulsOdd > nulsEven) {
        auto text = fromUtf16(bytes, true);
        if (!text) {
            return text.error();
        }
        decoded.text = std::move(*text);
        decoded.encoding = TextEncoding::Utf16LittleEndian;
        return decoded;
    }
    if (sample >= 8 && nulsEven > sample / 4) {
        auto text = fromUtf16(bytes, false);
        if (!text) {
            return text.error();
        }
        decoded.text = std::move(*text);
        decoded.encoding = TextEncoding::Utf16BigEndian;
        return decoded;
    }

    if (katana::entity::isValidUtf8(bytes)) {
        decoded.text = std::string(bytes);
        decoded.encoding = TextEncoding::Utf8;
        // Not a guess worth reporting. Plain ASCII is every encoding at once,
        // and text with multi-byte sequences that all validate is UTF-8 to
        // within a vanishing chance: UTF-8 is self-checking, and CP1252 prose
        // essentially never happens to be well-formed UTF-8.
        decoded.guessed = false;
        return decoded;
    }
    decoded.text = fromWindows1252(bytes);
    decoded.encoding = TextEncoding::Windows1252;
    return decoded;
}

katana::core::Result<std::string> encodeUtf16LittleEndian(std::string_view utf8)
{
    if (!katana::entity::isValidUtf8(utf8)) {
        return makeError(ErrorCode::InvalidArgument, "the text is not valid UTF-8");
    }
    std::string out;
    out.reserve(utf8.size() * 2 + 2);
    out += static_cast<char>(0xFF);
    out += static_cast<char>(0xFE);
    const auto put = [&out](std::uint32_t unit) {
        out += static_cast<char>(unit & 0xFF);
        out += static_cast<char>((unit >> 8) & 0xFF);
    };
    for (std::size_t i = 0; i < utf8.size();) {
        const auto lead = static_cast<unsigned char>(utf8[i]);
        std::uint32_t codePoint = 0;
        std::size_t length = 1;
        if (lead < 0x80) {
            codePoint = lead;
        } else if (lead < 0xE0) {
            codePoint = lead & 0x1Fu;
            length = 2;
        } else if (lead < 0xF0) {
            codePoint = lead & 0x0Fu;
            length = 3;
        } else {
            codePoint = lead & 0x07u;
            length = 4;
        }
        // Validity was established above, so the continuation bytes exist.
        for (std::size_t k = 1; k < length; ++k) {
            codePoint = (codePoint << 6) | (static_cast<unsigned char>(utf8[i + k]) & 0x3Fu);
        }
        i += length;
        if (codePoint >= 0x10000) {
            const std::uint32_t offset = codePoint - 0x10000;
            put(0xD800 + (offset >> 10));
            put(0xDC00 + (offset & 0x3FF));
        } else {
            put(codePoint);
        }
    }
    return out;
}

} // namespace katana::archive12d
