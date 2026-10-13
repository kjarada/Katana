#pragma once

// SHA-256 (FIPS 180-4), streaming, in core so that every layer shares one.
//
// It was written first as gis::sha256Hex (src/katana_io/web_access.cpp), which
// lives in the library that links GDAL; the update engine and its helper
// program may link core only (docs/update.md), so the one implementation moves
// down here, and gis::sha256Hex forwards to it. A second copy would be a
// defect (CLAUDE.md, section 2).
//
// Streaming because the update engine hashes files of tens of megabytes and
// must not hold them: the installed tree is hashed fresh, file by file, every
// time a plan is made and again before a swap. Whole blocks are hashed where
// the caller left them and the message is never held: the object is a 64-byte
// block (it keeps the tail of fewer than 64 bytes between calls) and eight
// words, however long the message.
//
// The implementation, src/katana_core/sha256.cpp, is portable scalar code
// with no hardware path: SHA-NI or ARM crypto would be one more thing that
// could disagree with the standard on a machine the tests do not run on, for
// a speed the update does not need (a file is hashed far faster than it is
// downloaded).

#include <array>
#include <cstddef>
#include <cstdint>
#include <filesystem>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"

namespace katana::core {

using Sha256Digest = std::array<std::uint8_t, 32>;

class Sha256 {
  public:
    Sha256();

    // Appends bytes; any number of calls, any split of the message.
    void update(std::string_view bytes);

    // The digest of everything appended so far, and the object is reset, so
    // the same Sha256 hashes the next message.
    [[nodiscard]] Sha256Digest finish();

    // The same digest as 64 lower-case hexadecimal characters.
    [[nodiscard]] std::string finishHex();

    // Forgets what was appended.
    void reset();

  private:
    std::array<std::uint32_t, 8> state_{};
    std::array<std::uint8_t, 64> block_{};
    std::uint64_t length_ = 0; // bytes appended, for the padding
    std::size_t used_ = 0;     // bytes waiting in block_
};

// The lower-case hexadecimal SHA-256 of `bytes`.
[[nodiscard]] std::string sha256Hex(std::string_view bytes);

// The lower-case hexadecimal SHA-256 of the file's bytes, read in blocks.
// NotFound when the file cannot be opened (absent, not a regular file - a
// directory, a device, a pipe - or not readable); FileImportFailure when
// reading it fails part way. The context is the path in UTF-8.
[[nodiscard]] Result<std::string> sha256FileHex(const std::filesystem::path& path);

// 64 lower-case hexadecimal characters -> the digest. ParseFailure on any
// other length or a character outside 0-9a-f (upper case is refused: a
// manifest has one spelling, so a comparison is a byte comparison).
[[nodiscard]] Result<Sha256Digest> parseSha256Hex(std::string_view hex);

} // namespace katana::core
