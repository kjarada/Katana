#pragma once

// Which bytes of the release's payload zip to ask for. The zip holds every
// install file as one deflate member at a known offset (the manifest's
// offset and csize); the producer orders the members program, lib, data,
// seed, so the changed programs - the bulk of a monthly update - are adjacent.
// The payload is never fetched whole.
//
// Nearby members are merged into one request, because each request costs
// about 150 ms of latency and a gap of 1 MiB costs about as much to fetch
// again at 6 MB/s (both measured against the release host on 2026-10-08). A
// merged range is at most 32 MiB, so a server that ignores `Range` and
// answers with the whole file is cut off after that much (maxBytes equals the
// range length).
//
// One range per request: multi-range requests were answered with 501 by the
// release storage host when measured.

#include <cstddef>
#include <cstdint>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::update {

// Two members merge when the hole between them is at most this; a merged range never
// exceeds kRangeMaxLength.
inline constexpr std::uint64_t kRangeMergeGap = 1024 * 1024;
inline constexpr std::uint64_t kRangeMaxLength = 32ull * 1024 * 1024;

// One manifest file's compressed bytes in the payload.
struct RangeMember {
    std::size_t fileIndex = 0;   // into Manifest::files
    std::uint64_t offset = 0;
    std::uint64_t length = 0;    // csize
};

// A request: bytes [offset, offset + length) of the payload, and the members
// whose data lies in it, in offset order.
struct MergedRange {
    std::uint64_t offset = 0;
    std::uint64_t length = 0;
    std::vector<RangeMember> members;
};

// Sorts by offset, then merges neighbours whose gap is at most `maxGap`, never
// letting a range exceed `maxLength` (a single member larger than `maxLength`
// is a range of its own, unsplit). Overlapping members (a malformed manifest)
// are merged, not duplicated. A member with length 0 is DROPPED - it has no bytes to
// ask for, and parseManifest (csize >= 2) never lets one through. Total over what
// remains: every member with length > 0 is in exactly one range, and a dropped member
// is in none.
[[nodiscard]] std::vector<MergedRange> mergeRanges(std::vector<RangeMember> members,
                                                   std::uint64_t maxGap = kRangeMergeGap,
                                                   std::uint64_t maxLength = kRangeMaxLength);

// ---- the request and the check of its answer (the transport, feed.hpp, uses these) ---------

// The reason a server that ignores `Range` is refused with: the message of the Unsupported
// error checkRangeResponse returns, and the sentence the dialog shows (design, section 2).
inline constexpr std::string_view kPartialDownloadsRefused =
    "this network does not allow partial downloads";

// The value of the `Range` header for bytes [offset, offset + length): "bytes=<first>-<last>",
// <last> inclusive (RFC 9110, 14.1.2). InvalidArgument for length 0 (no byte to ask for) and
// for a range whose end does not fit 64 bits.
[[nodiscard]] core::Result<std::string> rangeHeaderValue(std::uint64_t offset,
                                                         std::uint64_t length);

// Whether an answer is the range that was asked for: bytes [offset, offset + length) of a
// payload `payloadSize` bytes long (the signed size). `contentRange` is the response's
// Content-Range header text, empty when it had none; `bodyBytes` the length of the body.
//   206  Content-Range must be "bytes <offset>-<offset+length-1>/<payloadSize>" (the total may
//        be "*"), and the body exactly `length` bytes. An answer that is another part of the
//        file, or of a file of another length, is refused: the payload may have been replaced.
//   200  the server ignored `Range` and sent the whole file. Acceptable only when that IS what
//        was asked for (offset 0, the whole payload) and the body is all of it; any other 200
//        is Unsupported with kPartialDownloadsRefused.
//   other statuses: FileImportFailure whose context begins `status=<n>` (the convention of
//        feed.hpp's Fetch).
[[nodiscard]] core::Status checkRangeResponse(int status, std::string_view contentRange,
                                              std::uint64_t bodyBytes, std::uint64_t offset,
                                              std::uint64_t length, std::uint64_t payloadSize);

} // namespace katana::update
