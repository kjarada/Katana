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
#include <vector>

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
// are merged, not duplicated. A member with length 0 is dropped. Total: every
// member is in exactly one range.
[[nodiscard]] std::vector<MergedRange> mergeRanges(std::vector<RangeMember> members,
                                                   std::uint64_t maxGap = kRangeMergeGap,
                                                   std::uint64_t maxLength = kRangeMaxLength);

} // namespace katana::update
