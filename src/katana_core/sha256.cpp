// SHA-256 (FIPS 180-4), portable, streaming. The header says why it is in core.
//
// Plain scalar code: the update engine hashes tens of megabytes a time, which
// this does in well under a second, and a hardware path (SHA-NI, ARM crypto)
// is one more thing that could disagree with the standard on a machine the
// tests do not run on.

#include "katana/core/sha256.hpp"

#include <algorithm>
#include <bit>
#include <cstdio>
#include <cstring>
#include <memory>
#include <system_error>
#include <vector>

#include "katana/core/path_text.hpp"

#if defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <io.h>
#include <windows.h>
#else
#include <sys/stat.h>
#endif

namespace katana::core {

namespace {

// 5.3.3: the initial hash value, the first 32 bits of the fractional parts of
// the square roots of the first eight primes.
constexpr std::array<std::uint32_t, 8> kInitial = {0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
                                                   0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19};

// 4.2.2: the round constants, the first 32 bits of the fractional parts of the
// cube roots of the first 64 primes.
constexpr std::array<std::uint32_t, 64> kRound = {
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2};

constexpr std::size_t kBlockBytes = 64;
constexpr std::size_t kLengthOffset = 56; // the last 8 bytes of the final block hold the bit length

// How much of a file is read at once. Big enough that the stream's own
// per-call cost is lost in the hashing, small enough to sit in cache.
constexpr std::size_t kFileChunk = 256 * 1024;

// A file opened by the C library, closed when it goes out of scope.
struct FileCloser {
    void operator()(std::FILE* file) const { std::fclose(file); }
};
using CFile = std::unique_ptr<std::FILE, FileCloser>;

CFile openForReading(const std::filesystem::path& path)
{
#if defined(_WIN32)
    return CFile(_wfopen(path.c_str(), L"rb"));
#else
    return CFile(std::fopen(path.c_str(), "rb"));
#endif
}

// What the system says the OPEN file is: a file on a disk, and not a device
// or a pipe that the name happened to open.
bool isRegularFile(std::FILE* file)
{
#if defined(_WIN32)
    const auto handle = reinterpret_cast<HANDLE>(_get_osfhandle(_fileno(file)));
    return handle != INVALID_HANDLE_VALUE && GetFileType(handle) == FILE_TYPE_DISK;
#else
    struct stat info {};
    return fstat(fileno(file), &info) == 0 && S_ISREG(info.st_mode);
#endif
}

// 6.2.2: one block through the compression function. The message is read as
// big-endian words byte by byte, so the host's byte order does not matter.
void compress(std::array<std::uint32_t, 8>& state, const std::uint8_t* block)
{
    std::array<std::uint32_t, 64> w{};
    for (std::size_t i = 0; i < 16; ++i) {
        const std::uint8_t* at = block + 4 * i;
        w[i] = (static_cast<std::uint32_t>(at[0]) << 24) |
               (static_cast<std::uint32_t>(at[1]) << 16) |
               (static_cast<std::uint32_t>(at[2]) << 8) | static_cast<std::uint32_t>(at[3]);
    }
    for (std::size_t i = 16; i < 64; ++i) {
        const std::uint32_t s0 =
            std::rotr(w[i - 15], 7) ^ std::rotr(w[i - 15], 18) ^ (w[i - 15] >> 3);
        const std::uint32_t s1 =
            std::rotr(w[i - 2], 17) ^ std::rotr(w[i - 2], 19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16] + s0 + w[i - 7] + s1;
    }
    std::uint32_t a = state[0], b = state[1], c = state[2], d = state[3];
    std::uint32_t e = state[4], f = state[5], g = state[6], h = state[7];
    for (std::size_t i = 0; i < 64; ++i) {
        const std::uint32_t s1 = std::rotr(e, 6) ^ std::rotr(e, 11) ^ std::rotr(e, 25);
        const std::uint32_t choice = (e & f) ^ (~e & g);
        const std::uint32_t t1 = h + s1 + choice + kRound[i] + w[i];
        const std::uint32_t s0 = std::rotr(a, 2) ^ std::rotr(a, 13) ^ std::rotr(a, 22);
        const std::uint32_t majority = (a & b) ^ (a & c) ^ (b & c);
        const std::uint32_t t2 = s0 + majority;
        h = g;
        g = f;
        f = e;
        e = d + t1;
        d = c;
        c = b;
        b = a;
        a = t1 + t2;
    }
    state[0] += a;
    state[1] += b;
    state[2] += c;
    state[3] += d;
    state[4] += e;
    state[5] += f;
    state[6] += g;
    state[7] += h;
}

constexpr char kHexDigits[] = "0123456789abcdef";

std::string toHex(const Sha256Digest& digest)
{
    std::string out;
    out.reserve(digest.size() * 2);
    for (const std::uint8_t byte : digest) {
        out.push_back(kHexDigits[byte >> 4]);
        out.push_back(kHexDigits[byte & 0x0F]);
    }
    return out;
}

} // namespace

Sha256::Sha256()
{
    reset();
}

void Sha256::reset()
{
    state_ = kInitial;
    block_.fill(0);
    length_ = 0;
    used_ = 0;
}

void Sha256::update(std::string_view bytes)
{
    if (bytes.empty()) {
        return; // data() may be null, which memcpy must not be given
    }
    const auto* data = reinterpret_cast<const std::uint8_t*>(bytes.data());
    std::size_t left = bytes.size();
    length_ += left;

    if (used_ > 0) {
        const std::size_t take = std::min(left, kBlockBytes - used_);
        std::memcpy(block_.data() + used_, data, take);
        used_ += take;
        data += take;
        left -= take;
        if (used_ < kBlockBytes) {
            return;
        }
        compress(state_, block_.data());
        used_ = 0;
    }
    // Whole blocks are read where they lie: the caller's buffer is never copied.
    while (left >= kBlockBytes) {
        compress(state_, data);
        data += kBlockBytes;
        left -= kBlockBytes;
    }
    if (left > 0) {
        std::memcpy(block_.data(), data, left);
        used_ = left;
    }
}

Sha256Digest Sha256::finish()
{
    // 5.1.1: a 1 bit, zeros up to 56 mod 64 bytes, then the message length in
    // bits as a 64-bit big-endian number. One more block when the 1 bit leaves
    // no room for the length.
    const std::uint64_t bitLength = length_ * 8;
    block_[used_++] = 0x80;
    if (used_ > kLengthOffset) {
        std::fill(block_.begin() + static_cast<std::ptrdiff_t>(used_), block_.end(),
                  std::uint8_t{0});
        compress(state_, block_.data());
        used_ = 0;
    }
    std::fill(block_.begin() + static_cast<std::ptrdiff_t>(used_),
              block_.begin() + static_cast<std::ptrdiff_t>(kLengthOffset), std::uint8_t{0});
    for (std::size_t i = 0; i < 8; ++i) {
        block_[kLengthOffset + i] = static_cast<std::uint8_t>(bitLength >> (56 - 8 * i));
    }
    compress(state_, block_.data());

    Sha256Digest digest{};
    for (std::size_t i = 0; i < state_.size(); ++i) {
        digest[4 * i] = static_cast<std::uint8_t>(state_[i] >> 24);
        digest[4 * i + 1] = static_cast<std::uint8_t>(state_[i] >> 16);
        digest[4 * i + 2] = static_cast<std::uint8_t>(state_[i] >> 8);
        digest[4 * i + 3] = static_cast<std::uint8_t>(state_[i]);
    }
    reset();
    return digest;
}

std::string Sha256::finishHex()
{
    return toHex(finish());
}

std::string sha256Hex(std::string_view bytes)
{
    Sha256 hash;
    hash.update(bytes);
    return hash.finishHex();
}

Result<std::string> sha256FileHex(const std::filesystem::path& path)
{
    // Only a regular file is hashed. A directory opens as a stream on some
    // platforms and then reads as nothing, and so does the null device (NUL,
    // /dev/null): either would pass for the digest of an empty file. A pipe or
    // the console would block the open or the read for good. A path that does
    // not exist is left to the open below, which says so.
    //
    // Asked twice. Before the open, so that a FIFO is never opened: opening one
    // to read waits for a writer. And of the open file, which is the answer
    // that holds: libc++'s std::filesystem::status fails on NUL with
    // ERROR_INVALID_PARAMETER and so calls it absent, and the device then
    // opens (measured with clang 22 and libc++ on Windows, the library of the
    // Windows ARM64 build, 2026-10-14).
    const auto notRegular = [&path] {
        return makeError(ErrorCode::NotFound,
                         "the file cannot be opened: it is not a regular file (a directory, "
                         "a device or a pipe)",
                         pathToUtf8(path));
    };
    std::error_code unknown;
    const std::filesystem::file_status status = std::filesystem::status(path, unknown);
    if (std::filesystem::exists(status) && !std::filesystem::is_regular_file(status)) {
        return notRegular();
    }
    // Read through the C library, not a stream: a read that fails part way
    // (another handle's lock, a disk that goes away) leaves libc++'s ifstream
    // exactly as the end of the file does, eofbit and failbit and not badbit,
    // while fread sets the file's error indicator and not its end-of-file one
    // (measured as above). libstdc++'s ifstream told the two apart; the C
    // library does on every platform.
    const CFile in = openForReading(path);
    if (!in) {
        return makeError(ErrorCode::NotFound, "the file cannot be opened", pathToUtf8(path));
    }
    if (!isRegularFile(in.get())) {
        return notRegular();
    }
    Sha256 hash;
    std::vector<char> chunk(kFileChunk);
    for (;;) {
        const std::size_t got = std::fread(chunk.data(), 1, chunk.size(), in.get());
        hash.update(std::string_view(chunk.data(), got));
        if (got < chunk.size()) {
            break;
        }
    }
    // A short read is the end of the file or a failure, and only the error
    // indicator says which: a digest of the part read must not pass for the
    // digest of the file.
    if (std::ferror(in.get()) != 0) {
        return makeError(ErrorCode::FileImportFailure, "the file cannot be read", pathToUtf8(path));
    }
    return hash.finishHex();
}

Result<Sha256Digest> parseSha256Hex(std::string_view hex)
{
    constexpr std::size_t kHexLength = 64;
    if (hex.size() != kHexLength) {
        return makeError(ErrorCode::ParseFailure,
                         "a SHA-256 digest is 64 hexadecimal characters, not " +
                             std::to_string(hex.size()));
    }
    const auto nibble = [](char c) -> int {
        if (c >= '0' && c <= '9') {
            return c - '0';
        }
        if (c >= 'a' && c <= 'f') {
            return c - 'a' + 10;
        }
        return -1; // upper case too: a digest has one spelling
    };
    Sha256Digest digest{};
    for (std::size_t i = 0; i < digest.size(); ++i) {
        const int high = nibble(hex[2 * i]);
        const int low = nibble(hex[2 * i + 1]);
        if (high < 0 || low < 0) {
            return makeError(ErrorCode::ParseFailure,
                             "a SHA-256 digest is written with the characters 0-9 and a-f only",
                             "character " + std::to_string(high < 0 ? 2 * i : 2 * i + 1));
        }
        digest[i] = static_cast<std::uint8_t>((high << 4) | low);
    }
    return digest;
}

} // namespace katana::core
