#pragma once

// Raw deflate (RFC 1951) -> bytes, by zlib, which stays behind src/ (a public
// header never names it: tools/check_layering.cmake).
//
// Every member of the payload zip is deflated alone, so a member's bytes -
// found by the manifest's offset and csize - are a bare deflate stream with
// no zip header around them. The output is CAPPED at the size the signed
// manifest says (`expectedSize`): a stream that would inflate to more is cut
// off and refused, so a hostile or corrupt member cannot fill the disk or
// memory. Fewer bytes than expected is refused too.

#include <cstdint>
#include <functional>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"

namespace katana::update {

// Receives the inflated bytes in order, in blocks. A failure stops the
// inflation and is returned.
using InflateSink = std::function<core::Status(std::string_view block)>;

// ParseFailure for a stream that is not valid deflate, or ends early;
// InvalidArgument when it would produce more than `expectedSize` bytes or
// fewer; trailing bytes after the end of the stream are InvalidArgument too
// (the member's csize is exact).
[[nodiscard]] core::Status inflateRaw(std::string_view compressed, std::uint64_t expectedSize,
                                      const InflateSink& sink);

// The same, collected: for tests and for small members.
[[nodiscard]] core::Result<std::string> inflateRawToString(std::string_view compressed,
                                                           std::uint64_t expectedSize);

} // namespace katana::update
