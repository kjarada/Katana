// Which bytes of the payload to ask for (include/katana/update/ranges.hpp), the text of the
// request that asks, and the check of the answer.

#include "katana/update/ranges.hpp"

#include <algorithm>
#include <cctype>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"

namespace katana::update {

// ---- declared here until ranges.hpp (task T1's file, not T6's) carries them ---------------
//
// The transport (task T8) builds the `Range` header and judges the answer to it, and the design
// puts both in the range merger's task, so they are written and tested here. Nothing outside
// this file and tests/update/test_ranges.cpp names them yet; the report of task T6 gives the
// lines to move into the header.

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

namespace {

// offset + length when it fits 64 bits.
bool endOf(std::uint64_t offset, std::uint64_t length, std::uint64_t& end)
{
    return !__builtin_add_overflow(offset, length, &end);
}

} // namespace

std::vector<MergedRange> mergeRanges(std::vector<RangeMember> members, std::uint64_t maxGap,
                                     std::uint64_t maxLength)
{
    // A member with no bytes has nothing to ask for; it must not bridge two others either.
    std::erase_if(members, [](const RangeMember& member) { return member.length == 0; });
    // Offset order; ties by length then index so the result does not depend on the order
    // the caller listed the members in.
    std::sort(members.begin(), members.end(), [](const RangeMember& a, const RangeMember& b) {
        if (a.offset != b.offset) {
            return a.offset < b.offset;
        }
        if (a.length != b.length) {
            return a.length < b.length;
        }
        return a.fileIndex < b.fileIndex;
    });

    std::vector<MergedRange> ranges;
    std::uint64_t currentEnd = 0; // end of ranges.back(), when ranges.back() can grow
    bool currentCanGrow = false;
    for (const RangeMember& member : members) {
        std::uint64_t memberEnd = 0;
        if (!endOf(member.offset, member.length, memberEnd)) {
            // Cannot be a part of any real payload (offset + length wraps): its own range,
            // unchanged, never merged. rangeHeaderValue refuses to ask for it.
            ranges.push_back({member.offset, member.length, {member}});
            currentCanGrow = false;
            continue;
        }
        if (currentCanGrow) {
            MergedRange& current = ranges.back();
            // Members are sorted by offset, so member.offset >= current.offset; a member
            // that starts inside the range (an overlap) has gap 0.
            const std::uint64_t gap = member.offset > currentEnd ? member.offset - currentEnd : 0;
            const std::uint64_t newEnd = std::max(currentEnd, memberEnd);
            // The cap wins over everything, overlap included: the range length is what a
            // server that ignores `Range` is cut off at.
            if (gap <= maxGap && newEnd - current.offset <= maxLength) {
                current.length = newEnd - current.offset;
                current.members.push_back(member);
                currentEnd = newEnd;
                continue;
            }
        }
        ranges.push_back({member.offset, member.length, {member}});
        currentEnd = memberEnd;
        currentCanGrow = true;
    }
    return ranges;
}

core::Result<std::string> rangeHeaderValue(std::uint64_t offset, std::uint64_t length)
{
    std::uint64_t end = 0;
    if (length == 0 || !endOf(offset, length, end)) {
        return core::makeError(
            core::ErrorCode::InvalidArgument, "there is no valid byte range to ask for",
            "offset " + std::to_string(offset) + ", length " + std::to_string(length));
    }
    return "bytes=" + std::to_string(offset) + "-" + std::to_string(end - 1);
}

namespace {

bool isSpace(char c)
{
    return c == ' ' || c == '\t';
}

std::string_view trimmed(std::string_view text)
{
    while (!text.empty() && isSpace(text.front())) {
        text.remove_prefix(1);
    }
    while (!text.empty() && isSpace(text.back())) {
        text.remove_suffix(1);
    }
    return text;
}

// 1 to 20 ASCII digits that fit 64 bits, consumed from the front of `text`.
bool takeNumber(std::string_view& text, std::uint64_t& value)
{
    std::size_t digits = 0;
    std::uint64_t result = 0;
    while (digits < text.size() && text[digits] >= '0' && text[digits] <= '9') {
        const auto digit = static_cast<std::uint64_t>(text[digits] - '0');
        if (__builtin_mul_overflow(result, std::uint64_t{10}, &result) ||
            __builtin_add_overflow(result, digit, &result)) {
            return false;
        }
        ++digits;
    }
    if (digits == 0) {
        return false;
    }
    text.remove_prefix(digits);
    value = result;
    return true;
}

bool takeLiteral(std::string_view& text, char c)
{
    if (text.empty() || text.front() != c) {
        return false;
    }
    text.remove_prefix(1);
    return true;
}

// "bytes <first>-<last>/<total>" or "bytes <first>-<last>/*" (RFC 9110, 14.4). The unit
// is case-insensitive. `totalKnown` is false for "*".
struct ContentRange {
    std::uint64_t first = 0;
    std::uint64_t last = 0;
    bool totalKnown = false;
    std::uint64_t total = 0;
};

bool parseContentRange(std::string_view header, ContentRange& out)
{
    std::string_view text = trimmed(header);
    constexpr std::string_view kUnit = "bytes";
    if (text.size() < kUnit.size()) {
        return false;
    }
    for (std::size_t i = 0; i < kUnit.size(); ++i) {
        if (std::tolower(static_cast<unsigned char>(text[i])) != kUnit[i]) {
            return false;
        }
    }
    text.remove_prefix(kUnit.size());
    if (!takeLiteral(text, ' ')) {
        return false;
    }
    ContentRange parsed;
    if (!takeNumber(text, parsed.first) || !takeLiteral(text, '-') ||
        !takeNumber(text, parsed.last) || !takeLiteral(text, '/')) {
        return false;
    }
    if (takeLiteral(text, '*')) {
        parsed.totalKnown = false;
    } else if (takeNumber(text, parsed.total)) {
        parsed.totalKnown = true;
    } else {
        return false;
    }
    if (!text.empty() || parsed.first > parsed.last) {
        return false;
    }
    out = parsed;
    return true;
}

} // namespace

core::Status checkRangeResponse(int status, std::string_view contentRange, std::uint64_t bodyBytes,
                                std::uint64_t offset, std::uint64_t length,
                                std::uint64_t payloadSize)
{
    const std::string asked = "bytes " + std::to_string(offset) + "+" + std::to_string(length) +
                              " of " + std::to_string(payloadSize);
    const std::string answered206 =
        "status=206; Content-Range '" + std::string(contentRange) + "'; " + asked;

    if (status == 200) {
        // The server ignored `Range` and answered with the whole file. That is what was
        // asked for only when the request was for the whole file.
        if (offset == 0 && length == payloadSize && bodyBytes == payloadSize) {
            return {};
        }
        return core::makeError(core::ErrorCode::Unsupported, std::string(kPartialDownloadsRefused),
                               "status=200; " + asked);
    }
    if (status != 206) {
        return core::makeError(core::ErrorCode::FileImportFailure,
                               "the download server answered with an unexpected status",
                               "status=" + std::to_string(status) + "; " + asked);
    }

    ContentRange answered;
    if (!parseContentRange(contentRange, answered)) {
        return core::makeError(core::ErrorCode::FileImportFailure,
                               "the download server's Content-Range header is missing or malformed",
                               answered206);
    }
    std::uint64_t end = 0;
    if (length == 0 || !endOf(offset, length, end)) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "there is no valid byte range to ask for", asked);
    }
    if (answered.first != offset || answered.last != end - 1) {
        return core::makeError(core::ErrorCode::FileImportFailure,
                               "the download server answered with another part of the file than "
                               "the one asked for",
                               answered206);
    }
    if (answered.totalKnown && answered.total != payloadSize) {
        return core::makeError(core::ErrorCode::FileImportFailure,
                               "the download server's file is not the length the release names; "
                               "it may have been replaced",
                               answered206);
    }
    if (bodyBytes != length) {
        return core::makeError(core::ErrorCode::FileImportFailure,
                               "the download server sent " + std::to_string(bodyBytes) +
                                   " bytes for a range of " + std::to_string(length),
                               "status=206; " + asked);
    }
    return {};
}

} // namespace katana::update
