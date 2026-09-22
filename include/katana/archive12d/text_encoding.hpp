#pragma once

// The bytes of a 12d Archive file -> UTF-8 text, and back.
//
// THE MANUAL DOES NOT SAY WHAT ENCODING A 12da IS, AND THE ANSWER IS NOT UTF-8.
// Every archive written by 12d Model 15 that this was developed against - nine
// files, 4 MB to 58 MB - is UTF-16 little-endian with a byte order mark. The
// hand-written ones are UTF-8 without one. A reader that assumes UTF-8 reads
// the second kind and fails on every file that came out of the program the
// format belongs to, which is the wrong half to support.
//
// Detection, in order:
//   1. A byte order mark: FF FE (UTF-16LE), FE FF (UTF-16BE), EF BB BF (UTF-8).
//   2. No mark: where the NUL bytes fall. ASCII text as UTF-16LE is
//      `c 00 c 00`, NULs at odd offsets; as UTF-16BE the even ones. A 12da is
//      overwhelmingly ASCII - keywords and numbers - so this is decisive.
//   3. No NULs: UTF-8 if the bytes are well-formed UTF-8, otherwise
//      Windows-1252, which is what 12d Model wrote before it wrote UTF-16 and
//      what a file saved from an ANSI editor is. The fallback is reported, not
//      silent, because a degree sign or a superscript read under the wrong
//      code page is a wrong character rather than an error.
//
// Everything downstream of this is UTF-8, which is what the entity model
// requires of every string (see entity::isValidUtf8).

#include <string>
#include <string_view>

#include "katana/core/error.hpp"

namespace katana::archive12d {

enum class TextEncoding { Utf8, Utf8WithBom, Utf16LittleEndian, Utf16BigEndian, Windows1252 };

[[nodiscard]] const char* toString(TextEncoding encoding);

struct DecodedText {
    std::string text; // UTF-8, no byte order mark
    TextEncoding encoding = TextEncoding::Utf8;
    // True when the encoding was INFERRED and could be wrong: UTF-16 with no
    // mark, or the Windows-1252 fallback. Unmarked UTF-8 is not flagged -
    // UTF-8 is self-checking, so bytes that validate as it are it.
    bool guessed = false;
    // True when `text` is KNOWN to be well-formed UTF-8, which decodeText
    // establishes on every path: it either builds the text from code points,
    // which cannot come out ill-formed, or checks the bytes and fails. A
    // caller holding one of these can skip its own check, which on a 58 MB
    // archive is a second pass over the whole file. Default false so that a
    // DecodedText assembled by hand is still checked.
    bool validatedUtf8 = false;
};

// Fails with ParseFailure for UTF-16 that is not well formed: an odd number of
// bytes, or a surrogate half with no partner. Those are truncated or corrupt
// files, and decoding them to something plausible would hide that.
[[nodiscard]] katana::core::Result<DecodedText> decodeText(std::string_view bytes);

// UTF-8 -> UTF-16LE with a byte order mark, which is what 12d Model writes and
// so the form most certain to be read back by it. `utf8` must be valid UTF-8;
// InvalidArgument otherwise.
[[nodiscard]] katana::core::Result<std::string> encodeUtf16LittleEndian(std::string_view utf8);

} // namespace katana::archive12d
