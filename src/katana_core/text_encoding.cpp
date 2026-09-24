#include "katana/core/text_encoding.hpp"

#include "simd/text_kernels.hpp"

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>

namespace katana::core {

namespace {


// Bytes examined when there is no byte order mark. Every format decoded here
// begins with a header of keywords, names or numbers - a 12da its comments and
// first model, a coordinate list its first rows - so 4 KB is already hundreds of
// ASCII characters; more would only slow a wrong guess down.
constexpr std::size_t kSniffBytes = 4096;

// isValidUtf8 hands a run of ASCII to the kernel only once it has seen
// kAsciiProbe bytes of it one by one, and only while a whole kernel block
// (32 bytes) remains after them. Text in another language has runs of one to
// three bytes - a space, a digit - between its characters, and a call into the
// kernel for each such run was measured 2-4x slower than this loop
// (BM_IsValidUtf8Mixed); a run that has lasted 16 bytes is most likely a line
// of keywords and numbers, where the kernel is ten times faster.
constexpr std::size_t kAsciiProbe = 16;
constexpr std::size_t kAsciiBlock = 32;

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

Result<std::string> fromUtf16(std::string_view bytes, bool littleEndian)
{
    if (bytes.size() % 2 != 0) {
        return makeError(ErrorCode::ParseFailure,
                         "the file is UTF-16 but has an odd number of bytes, so it is truncated");
    }
    const auto* data = reinterpret_cast<const unsigned char*>(bytes.data());
    const std::size_t units = bytes.size() / 2;
    const auto unit = [&](std::size_t index) -> std::uint32_t {
        const std::uint32_t a = data[2 * index];
        const std::uint32_t b = data[2 * index + 1];
        return littleEndian ? (a | (b << 8)) : ((a << 8) | b);
    };

    std::string out;
    // These formats are almost entirely ASCII, which halves in UTF-8.
    out.reserve(units + 16);
    std::size_t i = 0;
    while (i < units) {
        // The run of ASCII units, which is nearly all of any file read here, a
        // whole block at a time where the processor allows (the kernel is the
        // loop below restricted to ASCII, and equals it byte for byte). Room
        // for the whole remainder is offered because the run's length is not
        // known until it ends; resize_and_overwrite keeps only what was
        // written, and initialises nothing.
        const std::size_t written = out.size();
        std::size_t run = 0;
        out.resize_and_overwrite(written + (units - i), [&](char* buffer, std::size_t) {
            run = kernels::narrowAsciiUtf16(data + 2 * i, units - i, littleEndian, buffer + written);
            return written + run;
        });
        i += run;

        // Then everything up to the next ASCII unit, one code point at a time.
        for (; i < units; ++i) {
            std::uint32_t codePoint = unit(i);
            if (codePoint < 0x80) {
                break;
            }
            if (codePoint >= 0xD800 && codePoint <= 0xDBFF) {
                if (i + 1 >= units) {
                    return makeError(ErrorCode::ParseFailure,
                                     "the file ends in the middle of a UTF-16 surrogate pair");
                }
                const std::uint32_t low = unit(i + 1);
                if (low < 0xDC00 || low > 0xDFFF) {
                    return makeError(ErrorCode::ParseFailure,
                                     "malformed UTF-16: a high surrogate with no low surrogate",
                                     "byte " + std::to_string(2 * i));
                }
                codePoint = 0x10000 + ((codePoint - 0xD800) << 10) + (low - 0xDC00);
                ++i;
            } else if (codePoint >= 0xDC00 && codePoint <= 0xDFFF) {
                return makeError(ErrorCode::ParseFailure,
                                 "malformed UTF-16: a low surrogate with no high surrogate",
                                 "byte " + std::to_string(2 * i));
            }
            appendUtf8(out, codePoint);
        }
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

bool isValidUtf8(std::string_view text)
{
    const auto* bytes = reinterpret_cast<const unsigned char*>(text.data());
    const std::size_t size = text.size();
    std::size_t i = 0;
    while (i < size) {
        const unsigned char lead = bytes[i];
        std::size_t following = 0;
        unsigned char lowSecond = 0x80;
        unsigned char highSecond = 0xBF;

        if (lead <= 0x7F) {
            // A run of ASCII: byte by byte for the first kAsciiProbe, then a
            // block at a time where the processor allows. A short name, or a
            // space between two accented words, never reaches the call.
            const std::size_t start = i;
            const std::size_t probeEnd = std::min(size, start + kAsciiProbe);
            while (i < probeEnd && bytes[i] <= 0x7F) {
                ++i;
            }
            if (i - start == kAsciiProbe && size - i >= kAsciiBlock) {
                i += kernels::asciiPrefix(bytes + i, size - i);
            }
            continue;
        }
        if (lead >= 0xC2 && lead <= 0xDF) {
            following = 1;
        } else if (lead == 0xE0) {
            following = 2;
            lowSecond = 0xA0; // anything lower would be an overlong encoding
        } else if (lead >= 0xE1 && lead <= 0xEC) {
            following = 2;
        } else if (lead == 0xED) {
            following = 2;
            highSecond = 0x9F; // D800-DFFF are surrogate halves, not characters
        } else if (lead >= 0xEE && lead <= 0xEF) {
            following = 2;
        } else if (lead == 0xF0) {
            following = 3;
            lowSecond = 0x90; // overlong
        } else if (lead >= 0xF1 && lead <= 0xF3) {
            following = 3;
        } else if (lead == 0xF4) {
            following = 3;
            highSecond = 0x8F; // above U+10FFFF
        } else {
            return false; // 0x80-0xC1 and 0xF5-0xFF are never lead bytes
        }

        if (i + following >= size) {
            return false; // truncated sequence
        }
        const unsigned char second = bytes[i + 1];
        if (second < lowSecond || second > highSecond) {
            return false;
        }
        for (std::size_t k = 2; k <= following; ++k) {
            const unsigned char continuation = bytes[i + k];
            if (continuation < 0x80 || continuation > 0xBF) {
                return false;
            }
        }
        i += following + 1;
    }
    return true;
}

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

Result<DecodedText> decodeText(std::string_view bytes)
{
    const auto byteAt = [&](std::size_t index) {
        return index < bytes.size() ? static_cast<unsigned char>(bytes[index]) : 0u;
    };

    DecodedText decoded;
    // Set once, for every path below: each one either appends code points,
    // which cannot produce ill-formed UTF-8, or validates the bytes and
    // returns an error, so no DecodedText carrying the claim ever escapes
    // with text that would fail the check.
    decoded.validatedUtf8 = true;
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
        if (!isValidUtf8(decoded.text)) {
            return makeError(ErrorCode::ParseFailure,
                             "the file is marked as UTF-8 but contains bytes that are not");
        }
        return decoded;
    }

    // No mark. ASCII as UTF-16 has a NUL in every other byte; nothing else a
    // text file could be has any. A quarter of the sample is far above what stray
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

    if (isValidUtf8(bytes)) {
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

Result<DecodedText> decodeTextAs(std::string_view bytes, TextEncoding encoding)
{
    const auto byteAt = [&](std::size_t index) {
        return index < bytes.size() ? static_cast<unsigned char>(bytes[index]) : 0u;
    };
    const bool markLittle = bytes.size() >= 2 && byteAt(0) == 0xFF && byteAt(1) == 0xFE;
    const bool markBig = bytes.size() >= 2 && byteAt(0) == 0xFE && byteAt(1) == 0xFF;
    const bool markUtf8 =
        bytes.size() >= 3 && byteAt(0) == 0xEF && byteAt(1) == 0xBB && byteAt(2) == 0xBF;

    const auto contradicted = [&](const char* found) {
        return makeError(ErrorCode::ParseFailure,
                         std::string("the file begins with a ") + found +
                             " byte order mark but was declared to be " + toString(encoding),
                         "declared and marked encodings disagree");
    };

    DecodedText decoded;
    decoded.encoding = encoding;
    decoded.validatedUtf8 = true;
    switch (encoding) {
    case TextEncoding::Utf16LittleEndian:
    case TextEncoding::Utf16BigEndian: {
        const bool little = encoding == TextEncoding::Utf16LittleEndian;
        if ((little && markBig) || (!little && markLittle)) {
            return contradicted(little ? "UTF-16 big-endian" : "UTF-16 little-endian");
        }
        if (markUtf8) {
            return contradicted("UTF-8");
        }
        const bool marked = little ? markLittle : markBig;
        auto text = fromUtf16(marked ? bytes.substr(2) : bytes, little);
        if (!text) {
            return text.error();
        }
        decoded.text = std::move(*text);
        return decoded;
    }
    case TextEncoding::Utf8:
    case TextEncoding::Utf8WithBom:
        if (markLittle || markBig) {
            return contradicted(markLittle ? "UTF-16 little-endian" : "UTF-16 big-endian");
        }
        decoded.text = std::string(markUtf8 ? bytes.substr(3) : bytes);
        decoded.encoding = markUtf8 ? TextEncoding::Utf8WithBom : TextEncoding::Utf8;
        if (!isValidUtf8(decoded.text)) {
            return makeError(ErrorCode::ParseFailure,
                             "the file was declared to be UTF-8 but contains bytes that are not");
        }
        return decoded;
    case TextEncoding::Windows1252:
        // Every byte is a Windows-1252 character, so a mark cannot be told from
        // text: EF BB BF is "ï»¿" there. It is refused rather than decoded as
        // three letters nobody typed.
        if (markLittle || markBig || markUtf8) {
            return contradicted(markUtf8 ? "UTF-8" : markLittle ? "UTF-16 little-endian"
                                                                : "UTF-16 big-endian");
        }
        decoded.text = fromWindows1252(bytes);
        return decoded;
    }
    return makeError(ErrorCode::InvalidArgument, "TextEncoding value outside the enumeration");
}

Result<std::string> encodeUtf16LittleEndian(std::string_view utf8)
{
    if (!isValidUtf8(utf8)) {
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

} // namespace katana::core
