// The range merger (include/katana/update/ranges.hpp) and, beside it, the text of the Range
// header and the check of the answer to it.
//
// Every expectation is worked out by hand from the rules the header states - a gap of at most
// 1 MiB merges, a merged range is at most 32 MiB - at the byte: the boundary, and one past it.
// The header text and the Content-Range grammar are RFC 9110 (14.1.2 and 14.4): the last byte
// position is inclusive, so a range of `length` bytes from `offset` ends at offset + length - 1.

#include <gtest/gtest.h>

#include <algorithm>
#include <array>
#include <cstdint>
#include <limits>
#include <numeric>
#include <set>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/update/ranges.hpp"

using namespace katana;
using update::MergedRange;
using update::RangeMember;

// ---- defined in src/katana_update/ranges.cpp, which declares them there until ranges.hpp
// (task T1's file) does: see the report of task T6.
namespace katana::update {
core::Result<std::string> rangeHeaderValue(std::uint64_t offset, std::uint64_t length);
core::Status checkRangeResponse(int status, std::string_view contentRange, std::uint64_t bodyBytes,
                                std::uint64_t offset, std::uint64_t length,
                                std::uint64_t payloadSize);
} // namespace katana::update

namespace {

constexpr std::uint64_t kMiB = 1024 * 1024;

RangeMember member(std::size_t index, std::uint64_t offset, std::uint64_t length)
{
    return {index, offset, length};
}

// The indexes of a range's members, in order.
std::vector<std::size_t> indexes(const MergedRange& range)
{
    std::vector<std::size_t> out;
    for (const RangeMember& m : range.members) {
        out.push_back(m.fileIndex);
    }
    return out;
}

using Indexes = std::vector<std::size_t>;

} // namespace

// ---- the constants the header fixes -------------------------------------------------------

TEST(Ranges, TheDefaultsAreAMebibyteOfGapAnd32MebibytesOfRange)
{
    // Measured against the release host on 2026-10-08 (ranges.hpp): ~150 ms a request at ~6 MB/s.
    EXPECT_EQ(update::kRangeMergeGap, 1048576u);
    EXPECT_EQ(update::kRangeMaxLength, 33554432u);
}

// ---- the shape of the answer --------------------------------------------------------------

TEST(Ranges, NoMembersAreNoRanges)
{
    EXPECT_TRUE(update::mergeRanges({}).empty());
}

TEST(Ranges, OneMemberIsOneRangeThatIsExactlyIt)
{
    const auto ranges = update::mergeRanges({member(7, 44, 85)});
    ASSERT_EQ(ranges.size(), 1u);
    EXPECT_EQ(ranges[0].offset, 44u);
    EXPECT_EQ(ranges[0].length, 85u);
    EXPECT_EQ(indexes(ranges[0]), (Indexes{7}));
    EXPECT_EQ(ranges[0].members[0].offset, 44u);
    EXPECT_EQ(ranges[0].members[0].length, 85u);
}

TEST(Ranges, MembersThatTouchMergeIntoTheRangeThatCoversBoth)
{
    // [0,10) and [10,25): no gap.
    const auto ranges = update::mergeRanges({member(0, 0, 10), member(1, 10, 15)});
    ASSERT_EQ(ranges.size(), 1u);
    EXPECT_EQ(ranges[0].offset, 0u);
    EXPECT_EQ(ranges[0].length, 25u);
    EXPECT_EQ(indexes(ranges[0]), (Indexes{0, 1}));
}

TEST(Ranges, TheRangeCoversTheHolesBetweenItsMembers)
{
    // [100,110), hole of 50, [160,170), hole of 30, [200,230): the range is [100,230).
    const auto ranges =
        update::mergeRanges({member(0, 100, 10), member(1, 160, 10), member(2, 200, 30)});
    ASSERT_EQ(ranges.size(), 1u);
    EXPECT_EQ(ranges[0].offset, 100u);
    EXPECT_EQ(ranges[0].length, 130u);
    EXPECT_EQ(indexes(ranges[0]), (Indexes{0, 1, 2}));
}

// ---- the gap threshold --------------------------------------------------------------------

TEST(Ranges, AGapOfExactlyAMebibyteMergesAndOneByteMoreDoesNot)
{
    // [0,100) then a hole of exactly 1 MiB, then 100 bytes. "At most" 1 MiB merges
    // (ranges.hpp, the contract): at 1 MiB the latency of a request and the bytes of the hole
    // cost the same, so the boundary is a tie, and the design's "under 1 MiB apart" is the
    // same rule spoken loosely.
    const std::uint64_t second = 100 + kMiB;
    const auto merged = update::mergeRanges({member(0, 0, 100), member(1, second, 100)});
    ASSERT_EQ(merged.size(), 1u);
    EXPECT_EQ(merged[0].length, second + 100);

    const auto split = update::mergeRanges({member(0, 0, 100), member(1, second + 1, 100)});
    ASSERT_EQ(split.size(), 2u);
    EXPECT_EQ(split[0].offset, 0u);
    EXPECT_EQ(split[0].length, 100u);
    EXPECT_EQ(split[1].offset, second + 1);
    EXPECT_EQ(split[1].length, 100u);
    EXPECT_EQ(indexes(split[0]), (Indexes{0}));
    EXPECT_EQ(indexes(split[1]), (Indexes{1}));
}

TEST(Ranges, TheGapThresholdIsTheCallersWhenTheyGiveOne)
{
    const std::vector<RangeMember> members{member(0, 0, 10), member(1, 15, 10)}; // a hole of 5
    EXPECT_EQ(update::mergeRanges(members, 5).size(), 1u);
    EXPECT_EQ(update::mergeRanges(members, 4).size(), 2u);
    // Zero: only members that touch (or overlap) merge.
    EXPECT_EQ(update::mergeRanges({member(0, 0, 10), member(1, 10, 10)}, 0).size(), 1u);
    EXPECT_EQ(update::mergeRanges({member(0, 0, 10), member(1, 11, 10)}, 0).size(), 2u);
}

// ---- the 32 MiB cap -----------------------------------------------------------------------

TEST(Ranges, TwoAdjacentMembersFillingExactlyThe32MebibyteCapMerge)
{
    const std::uint64_t half = 16 * kMiB;
    const auto ranges = update::mergeRanges({member(0, 0, half), member(1, half, half)});
    ASSERT_EQ(ranges.size(), 1u);
    EXPECT_EQ(ranges[0].length, 32 * kMiB);
}

TEST(Ranges, OneByteOverTheCapSplitsWhereTheMemberBoundaryIs)
{
    const std::uint64_t half = 16 * kMiB;
    const auto ranges = update::mergeRanges({member(0, 0, half), member(1, half, half + 1)});
    ASSERT_EQ(ranges.size(), 2u);
    EXPECT_EQ(ranges[0].length, half);
    EXPECT_EQ(ranges[1].offset, half);
    EXPECT_EQ(ranges[1].length, half + 1);
}

TEST(Ranges, TheHoleCountsTowardTheCap)
{
    // 30 MiB, a hole of 1 MiB (within the gap limit), 1 MiB: 32 MiB in all - merges.
    const auto fits =
        update::mergeRanges({member(0, 0, 30 * kMiB), member(1, 31 * kMiB, 1 * kMiB)});
    ASSERT_EQ(fits.size(), 1u);
    EXPECT_EQ(fits[0].length, 32 * kMiB);
    // One byte more of member and the hole is no longer worth it: two requests.
    const auto over =
        update::mergeRanges({member(0, 0, 30 * kMiB), member(1, 31 * kMiB, 1 * kMiB + 1)});
    EXPECT_EQ(over.size(), 2u);
}

TEST(Ranges, SeveralSmallMembersFillOneRangeThenStartTheNext)
{
    // Forty members of 1 MiB, touching: 40 MiB. The first 32 fill a range, the other 8 the next.
    std::vector<RangeMember> members;
    for (std::size_t i = 0; i < 40; ++i) {
        members.push_back(member(i, i * kMiB, kMiB));
    }
    const auto ranges = update::mergeRanges(members);
    ASSERT_EQ(ranges.size(), 2u);
    EXPECT_EQ(ranges[0].length, 32 * kMiB);
    EXPECT_EQ(ranges[0].members.size(), 32u);
    EXPECT_EQ(ranges[1].offset, 32 * kMiB);
    EXPECT_EQ(ranges[1].length, 8 * kMiB);
    EXPECT_EQ(ranges[1].members.size(), 8u);
}

TEST(Ranges, AMemberLargerThanTheCapIsARangeOfItsOwnAndIsNotSplit)
{
    // 1 MiB, then 40 MiB, then 1 MiB, all touching: the big one stands alone, its
    // neighbours do not join it, and they are not joined to each other across it.
    const auto ranges = update::mergeRanges(
        {member(0, 0, kMiB), member(1, kMiB, 40 * kMiB), member(2, 41 * kMiB, kMiB)});
    ASSERT_EQ(ranges.size(), 3u);
    EXPECT_EQ(ranges[1].offset, kMiB);
    EXPECT_EQ(ranges[1].length, 40 * kMiB);
    EXPECT_EQ(indexes(ranges[1]), (Indexes{1}));
    EXPECT_EQ(indexes(ranges[0]), (Indexes{0}));
    EXPECT_EQ(indexes(ranges[2]), (Indexes{2}));
}

TEST(Ranges, TheCapIsTheCallersWhenTheyGiveOne)
{
    const std::vector<RangeMember> members{member(0, 0, 10), member(1, 10, 10)};
    EXPECT_EQ(update::mergeRanges(members, update::kRangeMergeGap, 20).size(), 1u);
    EXPECT_EQ(update::mergeRanges(members, update::kRangeMergeGap, 19).size(), 2u);
    // A cap of zero: every member is its own range, none is dropped.
    EXPECT_EQ(update::mergeRanges(members, update::kRangeMergeGap, 0).size(), 2u);
}

// ---- sorting, overlap, zero length --------------------------------------------------------

TEST(Ranges, MembersInAnyOrderGiveTheSameRanges)
{
    // Four members, 24 orders: one answer. Offsets 0, 50, 2 MiB + 100 (a gap too big), 2 MiB + 150.
    std::vector<RangeMember> base{member(10, 0, 50), member(11, 50, 40),
                                  member(12, 2 * kMiB + 100, 30), member(13, 2 * kMiB + 150, 20)};
    std::vector<std::size_t> order(base.size());
    std::iota(order.begin(), order.end(), 0);
    std::size_t orders = 0;
    do {
        std::vector<RangeMember> shuffled;
        for (const std::size_t i : order) {
            shuffled.push_back(base[i]);
        }
        const auto ranges = update::mergeRanges(shuffled);
        ASSERT_EQ(ranges.size(), 2u);
        EXPECT_EQ(ranges[0].offset, 0u);
        EXPECT_EQ(ranges[0].length, 90u);
        EXPECT_EQ(indexes(ranges[0]), (Indexes{10, 11}));
        EXPECT_EQ(ranges[1].offset, 2 * kMiB + 100);
        EXPECT_EQ(ranges[1].length, 70u);
        EXPECT_EQ(indexes(ranges[1]), (Indexes{12, 13}));
        ++orders;
    } while (std::next_permutation(order.begin(), order.end()));
    EXPECT_EQ(orders, 24u);
}

TEST(Ranges, OverlappingMembersAreMergedOnceEach)
{
    // [0,100) and [50,150) overlap; [10,20) lies inside the first.
    const auto ranges =
        update::mergeRanges({member(0, 0, 100), member(1, 50, 100), member(2, 10, 10)});
    ASSERT_EQ(ranges.size(), 1u);
    EXPECT_EQ(ranges[0].offset, 0u);
    EXPECT_EQ(ranges[0].length, 150u);
    // In offset order; each member appears once.
    EXPECT_EQ(indexes(ranges[0]), (Indexes{0, 2, 1}));
}

TEST(Ranges, AMemberInsideAnotherNeverShortensTheRange)
{
    // [0,100) holds [10,20); the third member starts exactly the gap limit past 100, the END of the
    // first - and 1 MiB - 80 bytes past 20, the end of the second.
    const auto ranges =
        update::mergeRanges({member(0, 0, 100), member(1, 10, 10), member(2, 100 + 1 * kMiB, 10)});
    ASSERT_EQ(ranges.size(), 1u) << "the gap is measured from the furthest end so far, 100";
    EXPECT_EQ(ranges[0].length, 100 + kMiB + 10);
}

TEST(Ranges, TheCapStillWinsOverAnOverlap)
{
    // [0,20) and [10,30) MiB overlap and together span 30 MiB, within the cap. A third member,
    // [20,50) MiB, overlaps them too, but joining would make a 50 MiB request: it stands alone.
    const auto ranges =
        update::mergeRanges({member(0, 0, 20 * kMiB), member(1, 10 * kMiB, 20 * kMiB),
                             member(2, 20 * kMiB, 30 * kMiB)});
    ASSERT_EQ(ranges.size(), 2u);
    EXPECT_EQ(ranges[0].length, 30 * kMiB);
    EXPECT_EQ(indexes(ranges[0]), (Indexes{0, 1}));
    EXPECT_EQ(indexes(ranges[1]), (Indexes{2}));
}

TEST(Ranges, AMemberWithNoBytesIsDroppedAndBridgesNothing)
{
    // [0,10) and [10 + 2 MiB, ...) are too far apart; a zero-length member in between, at
    // the end of the first, must not make them neighbours, and is in no range.
    const auto ranges =
        update::mergeRanges({member(0, 0, 10), member(1, 10, 0), member(2, 12 * kMiB, 0),
                             member(3, 10 + 2 * kMiB, 10)});
    ASSERT_EQ(ranges.size(), 2u);
    for (const MergedRange& range : ranges) {
        for (const RangeMember& m : range.members) {
            EXPECT_NE(m.length, 0u);
            EXPECT_NE(m.fileIndex, 1u);
            EXPECT_NE(m.fileIndex, 2u);
        }
    }
    EXPECT_EQ(indexes(ranges[0]), (Indexes{0}));
    EXPECT_EQ(indexes(ranges[1]), (Indexes{3}));
    // Only zero-length members: nothing to ask for.
    EXPECT_TRUE(update::mergeRanges({member(0, 5, 0), member(1, 9, 0)}).empty());
}

TEST(Ranges, AMemberWhoseEndDoesNotFitSixtyFourBitsIsItsOwnRangeUnchanged)
{
    const std::uint64_t top = std::numeric_limits<std::uint64_t>::max();
    const auto ranges = update::mergeRanges({member(0, top - 4, 10), member(1, top - 100, 50)});
    ASSERT_EQ(ranges.size(), 2u);
    EXPECT_EQ(ranges[0].offset, top - 100);
    EXPECT_EQ(ranges[0].length, 50u);
    EXPECT_EQ(ranges[1].offset, top - 4);
    EXPECT_EQ(ranges[1].length, 10u); // not clamped to fit: nobody can ask for it
    // And no request can be made for it.
    EXPECT_FALSE(update::rangeHeaderValue(top - 4, 10).ok());
}

// ---- the properties, over many member sets ------------------------------------------------

TEST(Ranges, EveryMemberIsInExactlyOneRangeAndTheRangesObeyTheRules)
{
    // A deterministic pseudo-random walk (a 64-bit LCG, Knuth's MMIX constants) lays members end to
    // end with holes chosen from the sizes that matter: 0, exactly the gap limit, one over it,
    // small, large; lengths from tiny to well over the cap.
    std::uint64_t seed = 0x9E3779B97F4A7C15ull;
    auto next = [&seed](std::uint64_t bound) {
        seed = seed * 6364136223846793005ull + 1442695040888963407ull;
        return (seed >> 33) % bound;
    };
    const std::array<std::uint64_t, 6> holes{
        0, 1, update::kRangeMergeGap, update::kRangeMergeGap + 1, 4096, 3 * kMiB};
    const std::array<std::uint64_t, 7> lengths{1,        100,           4096,     kMiB,
                                               9 * kMiB, 31 * kMiB + 7, 40 * kMiB};

    std::size_t sets = 0;
    std::size_t splitByGap = 0;
    std::size_t splitByCap = 0;
    std::size_t merged = 0;
    for (int round = 0; round < 400; ++round) {
        const std::size_t count = 1 + next(12);
        std::vector<RangeMember> members;
        std::uint64_t at = next(1000);
        for (std::size_t i = 0; i < count; ++i) {
            at += holes[next(holes.size())];
            const std::uint64_t length = lengths[next(lengths.size())];
            members.push_back(member(i, at, length));
            at += length;
        }
        // Present them in a scrambled order; the answer must not care.
        std::vector<RangeMember> given = members;
        for (std::size_t i = given.size(); i > 1; --i) {
            std::swap(given[i - 1], given[next(i)]);
        }
        const auto ranges = update::mergeRanges(given);
        ++sets;

        // Every member in exactly one range.
        std::multiset<std::size_t> seen;
        for (const MergedRange& range : ranges) {
            ASSERT_FALSE(range.members.empty());
            for (const RangeMember& m : range.members) {
                seen.insert(m.fileIndex);
                // The range covers it.
                EXPECT_GE(m.offset, range.offset);
                EXPECT_LE(m.offset + m.length, range.offset + range.length);
            }
            // The range starts at its first member and ends at its furthest end: no slack.
            EXPECT_EQ(range.offset, range.members.front().offset);
            std::uint64_t end = 0;
            for (const RangeMember& m : range.members) {
                end = std::max(end, m.offset + m.length);
            }
            EXPECT_EQ(range.offset + range.length, end);
            // The cap, unless the range is a single member.
            if (range.members.size() > 1) {
                EXPECT_LE(range.length, update::kRangeMaxLength);
            }
        }
        std::multiset<std::size_t> wanted;
        for (const RangeMember& m : members) {
            wanted.insert(m.fileIndex);
        }
        ASSERT_EQ(seen, wanted) << "round " << round;

        // Ranges in offset order; neighbours are neighbours for a reason.
        for (std::size_t i = 1; i < ranges.size(); ++i) {
            const MergedRange& before = ranges[i - 1];
            const MergedRange& after = ranges[i];
            ASSERT_GE(after.offset, before.offset + before.length) << "round " << round;
            const std::uint64_t gap = after.offset - (before.offset + before.length);
            if (gap > update::kRangeMergeGap) {
                ++splitByGap;
            } else {
                // Close enough to merge, so it was the cap that kept them apart: the first
                // member of the next range would have made the request longer than the cap.
                const RangeMember& first = after.members.front();
                EXPECT_GT(first.offset + first.length - before.offset, update::kRangeMaxLength)
                    << "round " << round;
                ++splitByCap;
            }
        }
        merged += members.size() - ranges.size();
    }
    // The cases the property claims to cover were reached.
    EXPECT_EQ(sets, 400u);
    EXPECT_GT(splitByGap, 50u);
    EXPECT_GT(splitByCap, 50u);
    EXPECT_GT(merged, 200u);
}

// ---- the Range header ---------------------------------------------------------------------

TEST(Ranges, TheRangeHeaderNamesTheFirstAndTheLastByte)
{
    // "bytes=first-last", last inclusive: 85 bytes from 44 end at 128; one byte at 0 is 0-0.
    EXPECT_EQ(*update::rangeHeaderValue(44, 85), "bytes=44-128");
    EXPECT_EQ(*update::rangeHeaderValue(0, 1), "bytes=0-0");
    EXPECT_EQ(*update::rangeHeaderValue(0, 32 * kMiB), "bytes=0-33554431");
    // Beyond 32 bits: 2^40 + 9 bytes from 2^40 is 2^40 .. 2^40 + 8.
    EXPECT_EQ(*update::rangeHeaderValue(1099511627776ull, 9), "bytes=1099511627776-1099511627784");
    // Ten bytes ending at the last position 64 bits can name (2^64 - 2): 2^64 - 11 .. 2^64 - 2.
    const std::uint64_t top = std::numeric_limits<std::uint64_t>::max();
    EXPECT_EQ(*update::rangeHeaderValue(top - 10, 10),
              "bytes=18446744073709551605-18446744073709551614");
}

TEST(Ranges, ARangeHeaderIsNotMadeForNothingOrForAnEndPastSixtyFourBits)
{
    EXPECT_EQ(update::rangeHeaderValue(10, 0).error().code, core::ErrorCode::InvalidArgument);
    const std::uint64_t top = std::numeric_limits<std::uint64_t>::max();
    EXPECT_EQ(update::rangeHeaderValue(top, 2).error().code, core::ErrorCode::InvalidArgument);
    EXPECT_EQ(update::rangeHeaderValue(top - 4, 10).error().code, core::ErrorCode::InvalidArgument);
}

// ---- the answer to a ranged request -------------------------------------------------------

namespace {

// A payload of 3791 bytes (the v2 fixture zip), and a range of its bytes 44 to 128.
constexpr std::uint64_t kPayload = 3791;
constexpr std::uint64_t kOffset = 44;
constexpr std::uint64_t kLength = 85;

core::Status answer(int status, std::string_view contentRange, std::uint64_t body,
                    std::uint64_t offset = kOffset, std::uint64_t length = kLength,
                    std::uint64_t payload = kPayload)
{
    return update::checkRangeResponse(status, contentRange, body, offset, length, payload);
}

} // namespace

TEST(Ranges, APartialAnswerThatIsExactlyTheRangeAskedForIsAccepted)
{
    EXPECT_TRUE(answer(206, "bytes 44-128/3791", 85).ok());
    // The total may be unknown, the unit is not case-sensitive, blanks around the field are not
    // part of it.
    EXPECT_TRUE(answer(206, "bytes 44-128/*", 85).ok());
    EXPECT_TRUE(answer(206, "Bytes 44-128/3791", 85).ok());
    EXPECT_TRUE(answer(206, "BYTES 44-128/3791", 85).ok());
    EXPECT_TRUE(answer(206, "  bytes 44-128/3791 \t", 85).ok());
    // The first byte, and the last byte of the file.
    EXPECT_TRUE(answer(206, "bytes 0-0/3791", 1, 0, 1).ok());
    EXPECT_TRUE(answer(206, "bytes 3790-3790/3791", 1, 3790, 1).ok());
}

TEST(Ranges, APartialAnswerForAnotherPartOfTheFileIsRefused)
{
    for (const char* header : {"bytes 45-129/3791", "bytes 43-127/3791", "bytes 44-127/3791",
                               "bytes 44-129/3791", "bytes 0-84/3791"}) {
        const core::Status status = answer(206, header, 85);
        ASSERT_FALSE(status.ok()) << header;
        EXPECT_EQ(status.error().code, core::ErrorCode::FileImportFailure) << header;
        EXPECT_EQ(status.error().context.rfind("status=206", 0), 0u) << status.error().context;
    }
}

TEST(Ranges, APartialAnswerFromAFileOfAnotherLengthIsRefused)
{
    // The release names a payload of 3791 bytes; a server holding 3790 or 3792 has another file.
    EXPECT_FALSE(answer(206, "bytes 44-128/3790", 85).ok());
    EXPECT_FALSE(answer(206, "bytes 44-128/3792", 85).ok());
    EXPECT_FALSE(answer(206, "bytes 44-128/0", 85).ok());
}

TEST(Ranges, APartialAnswerWithABodyOfTheWrongLengthIsRefused)
{
    for (const std::uint64_t body : {0ull, 84ull, 86ull, 3791ull}) {
        const core::Status status = answer(206, "bytes 44-128/3791", body);
        ASSERT_FALSE(status.ok()) << body;
        EXPECT_EQ(status.error().code, core::ErrorCode::FileImportFailure) << body;
    }
}

TEST(Ranges, APartialAnswerWithoutAUsableContentRangeIsRefused)
{
    for (const char* header :
         {"", "bytes", "bytes 44-128", "bytes 44-128/", "bytes 44/3791", "bytes */3791",
          "bytes 128-44/3791", "bytes -44/3791", "bytes 44-/3791", "items 44-128/3791",
          "bytes  44-128/3791", "bytes 44 -128/3791", "bytes 44-128/3791 extra",
          "bytes 4x-128/3791", "bytes 44-128/37 91", "bytes +44-128/3791", "bytes 44-128/3791,",
          // 21 digits: more than fit 64 bits, so not a number this program can compare
          "bytes 440000000000000000000-440000000000000000084/3791",
          "bytes 44-128/18446744073709551616"}) {
        const core::Status status = answer(206, header, 85);
        ASSERT_FALSE(status.ok()) << '"' << header << '"';
        EXPECT_EQ(status.error().code, core::ErrorCode::FileImportFailure) << '"' << header << '"';
    }
}

TEST(Ranges, AServerThatIgnoredRangeIsRefusedWithTheReasonTheDialogShows)
{
    // 200, the whole file, for a request for 85 bytes at 44.
    const core::Status status = answer(200, "", kPayload);
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, core::ErrorCode::Unsupported);
    EXPECT_EQ(status.error().message, "this network does not allow partial downloads");
    // Whatever the body was: a 200 for part of a file is never the part.
    EXPECT_FALSE(answer(200, "", kLength).ok());
    EXPECT_EQ(answer(200, "", kLength).error().code, core::ErrorCode::Unsupported);
    // A first-bytes range of the file, too: the server did not honour it, however well the
    // body happens to begin.
    EXPECT_EQ(answer(200, "", kPayload, 0, kLength).error().code, core::ErrorCode::Unsupported);
}

TEST(Ranges, AWholeFileAnswerIsAcceptedOnlyForARequestForTheWholeFile)
{
    EXPECT_TRUE(answer(200, "", kPayload, 0, kPayload).ok());
    // ... with all of the body.
    EXPECT_FALSE(answer(200, "", kPayload - 1, 0, kPayload).ok());
    EXPECT_FALSE(answer(200, "", kPayload + 1, 0, kPayload).ok());
}

TEST(Ranges, OtherStatusesAreFailuresThatNameTheStatus)
{
    for (const int code : {204, 301, 400, 403, 404, 416, 429, 500, 503}) {
        const core::Status status = answer(code, "", 0);
        ASSERT_FALSE(status.ok()) << code;
        EXPECT_EQ(status.error().code, core::ErrorCode::FileImportFailure) << code;
        EXPECT_EQ(status.error().context.rfind("status=" + std::to_string(code), 0), 0u)
            << status.error().context;
    }
}
