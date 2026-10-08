// Raw deflate (RFC 1951) by zlib, with the output held to the size the signed manifest
// names (include/katana/update/inflate.hpp).
//
// Why a cap of "expected + 1" per call. zlib is asked for at most one byte more than the
// signed size still to come, so a member that would inflate longer is noticed at the
// first surplus byte and no more than that is ever produced; nothing is allocated from
// the signed size either, the working buffer is a fixed block. A hostile member (a
// "zip bomb": a megabyte that inflates to a gigabyte) therefore costs one block of memory
// and a few microseconds, whatever it claims.
//
// CRC-32 is not checked: a member is a bare deflate stream, so there is none to compare
// with. The caller's SHA-256 against the signed manifest is the integrity check.

#include "katana/update/inflate.hpp"

// With ZLIB_CONST the input pointer is const, so no cast is needed to give zlib the caller's bytes.
#define ZLIB_CONST
#include <zlib.h>

#include <algorithm>
#include <array>
#include <climits>
#include <cstddef>
#include <string>

namespace katana::update {

namespace {

// Output is handed to the sink in blocks of at most this many bytes. 64 KiB is the size
// at which a block copy dominates the call overhead; a larger block only holds more
// memory.
constexpr std::size_t kBlockBytes = 64 * 1024;

// inflateRawToString refuses a signed size above this before it inflates anything: the
// result is held in memory whole, and a member that needs more than this is written to
// disk through inflateRaw with a sink. 256 MiB is far above any member "small" enough
// for the string form (the largest file Katana ships is about 50 MiB) and far below the
// 2 GiB total an install may have (paths.hpp).
constexpr std::uint64_t kMaxCollectedBytes = 256ull * 1024 * 1024;

// inflateEnd on every way out.
class InflateStream {
  public:
    InflateStream()
    {
        stream_.zalloc = Z_NULL;
        stream_.zfree = Z_NULL;
        stream_.opaque = Z_NULL;
    }
    InflateStream(const InflateStream&) = delete;
    InflateStream& operator=(const InflateStream&) = delete;
    ~InflateStream()
    {
        if (open_) {
            inflateEnd(&stream_);
        }
    }

    // windowBits -15: raw deflate, no zlib or gzip wrapper, the largest window (RFC 1951
    // lets a stream use 32 KiB of history).
    [[nodiscard]] bool open()
    {
        open_ = inflateInit2(&stream_, -15) == Z_OK;
        return open_;
    }
    z_stream& get() { return stream_; }

  private:
    z_stream stream_{};
    bool open_ = false;
};

core::Error truncated(std::uint64_t producedSoFar)
{
    return core::makeError(core::ErrorCode::ParseFailure,
                           "the compressed data ends before the end of the stream",
                           "inflated " + std::to_string(producedSoFar) + " bytes before it ended");
}

core::Error notDeflate(const z_stream& stream, int code)
{
    std::string detail = "zlib error " + std::to_string(code);
    if (stream.msg != nullptr) {
        detail += ": ";
        detail += stream.msg;
    }
    return core::makeError(core::ErrorCode::ParseFailure,
                           "the compressed data is not a valid deflate stream", std::move(detail));
}

} // namespace

core::Status inflateRaw(std::string_view compressed, std::uint64_t expectedSize,
                        const InflateSink& sink)
{
    InflateStream inflater;
    if (!inflater.open()) {
        return core::makeError(core::ErrorCode::Internal, "zlib could not start an inflater");
    }
    z_stream& stream = inflater.get();

    std::array<unsigned char, kBlockBytes> block;
    const auto* input = reinterpret_cast<const unsigned char*>(compressed.data());
    std::size_t inputLeft = compressed.size();
    std::uint64_t produced = 0;

    for (;;) {
        // zlib counts input in 32 bits; a longer member is fed in slices.
        if (stream.avail_in == 0 && inputLeft > 0) {
            const std::size_t slice = std::min<std::size_t>(inputLeft, UINT_MAX);
            stream.next_in = input;
            stream.avail_in = static_cast<uInt>(slice);
            input += slice;
            inputLeft -= slice;
        }

        // At most one byte beyond the signed size still to come: see the top of the file.
        const std::uint64_t remaining = expectedSize - produced; // produced never exceeds it
        const std::size_t room =
            remaining >= kBlockBytes ? kBlockBytes : static_cast<std::size_t>(remaining) + 1;
        stream.next_out = block.data();
        stream.avail_out = static_cast<uInt>(room);

        const int code = inflate(&stream, Z_NO_FLUSH);
        const std::size_t made = room - stream.avail_out;

        if (code == Z_MEM_ERROR) {
            return core::makeError(core::ErrorCode::Internal, "zlib is out of memory");
        }
        if (code != Z_OK && code != Z_STREAM_END && code != Z_BUF_ERROR) {
            return notDeflate(stream, code);
        }

        if (produced + made > expectedSize) {
            return core::makeError(core::ErrorCode::InvalidArgument,
                                   "the member inflates to more than the " +
                                       std::to_string(expectedSize) +
                                       " bytes its manifest entry names",
                                   "stopped at the first byte past the cap");
        }
        if (made > 0) {
            const core::Status delivered =
                sink(std::string_view(reinterpret_cast<const char*>(block.data()), made));
            if (!delivered) {
                return delivered;
            }
            produced += made;
        }

        if (code == Z_STREAM_END) {
            if (produced != expectedSize) {
                return core::makeError(core::ErrorCode::InvalidArgument,
                                       "the member inflates to " + std::to_string(produced) +
                                           " bytes, but its manifest entry names " +
                                           std::to_string(expectedSize));
            }
            if (stream.avail_in != 0 || inputLeft != 0) {
                return core::makeError(core::ErrorCode::InvalidArgument,
                                       "there are bytes after the end of the compressed stream",
                                       std::to_string(stream.avail_in + inputLeft) + " bytes");
            }
            return {};
        }

        // Z_OK or Z_BUF_ERROR with nothing more to read and no stream end: the data ran
        // out. (Z_BUF_ERROR is zlib saying "no progress possible", which with room in the
        // output can only mean no input.)
        if (stream.avail_in == 0 && inputLeft == 0 && made == 0) {
            return truncated(produced);
        }
        if (code == Z_BUF_ERROR && made == 0 && stream.avail_in != 0) {
            // No progress although both input and room remain: a state zlib is not meant
            // to reach; stop rather than spin.
            return notDeflate(stream, code);
        }
    }
}

core::Result<std::string> inflateRawToString(std::string_view compressed,
                                             std::uint64_t expectedSize)
{
    if (expectedSize > kMaxCollectedBytes) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "the member is too large to hold in memory; inflate it to a sink",
                               std::to_string(expectedSize) + " bytes");
    }
    std::string out;
    // The signed size is believed only as far as the stream proves it: reserving it whole
    // would let a manifest entry that lies commit a quarter of a gigabyte before a byte
    // was read.
    out.reserve(static_cast<std::size_t>(std::min<std::uint64_t>(expectedSize, 1 << 20)));
    const core::Status status =
        inflateRaw(compressed, expectedSize, [&out](std::string_view block) {
            out.append(block);
            return core::Status{};
        });
    if (!status) {
        return status.error();
    }
    return out;
}

} // namespace katana::update
