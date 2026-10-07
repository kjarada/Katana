// SHA-256 (core/sha256.hpp): the digest of a message however it is cut into
// update() calls, of a file, and of the text a manifest writes it as.
//
// Every expected digest comes from outside the program under test, as
// CLAUDE.md section 4 requires:
//   - the messages FIPS 180-4 / NIST give with their digests (the "SHA-256
//     examples" document: "abc", the 448-bit and 896-bit messages, one million
//     'a'; the empty message's digest is the well-known e3b0c442...);
//   - every other digest below was printed by Python's hashlib (OpenSSL
//     underneath) for the input the comment above it spells out. Where the
//     input is the "pattern", it is the byte at position p
//         (p * 7 + (p >> 8)) & 0xFF
//     starting at p = 0, which in Python is
//         bytes(((p * 7 + (p >> 8)) & 0xFF) for p in range(n))
//     and the 64 MiB case is hashlib.sha256() fed the same bytes for
//     p in range(64 * 1024 * 1024), a megabyte at a time. The formula has a
//     carry from the high byte, so no slice repeats and a hash that skipped or
//     repeated a block would not match.
// `openssl dgst -sha256` agrees with the FIPS values (checked by hand).

#include <gtest/gtest.h>

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <string>
#include <string_view>
#include <system_error>
#include <vector>

#if defined(_WIN32)
#include <process.h>
#else
#include <unistd.h>
#endif

#include "katana/core/sha256.hpp"

namespace fs = std::filesystem;
using katana::core::ErrorCode;
using katana::core::parseSha256Hex;
using katana::core::Sha256;
using katana::core::Sha256Digest;
using katana::core::sha256FileHex;
using katana::core::sha256Hex;

namespace {

// ---- inputs --------------------------------------------------------------------------

// The byte at position `p` of the pattern the comment at the top defines.
char patternByte(std::uint64_t p)
{
    return static_cast<char>((p * 7 + (p >> 8)) & 0xFF);
}

// `count` bytes of the pattern starting at position `start`.
std::string patternBytes(std::uint64_t start, std::size_t count)
{
    std::string bytes(count, '\0');
    for (std::size_t i = 0; i < count; ++i) {
        bytes[i] = patternByte(start + i);
    }
    return bytes;
}

// The digest of `message` fed in pieces of `piece` bytes (the last may be shorter).
std::string hashInPieces(std::string_view message, std::size_t piece)
{
    Sha256 hash;
    for (std::size_t at = 0; at < message.size(); at += piece) {
        hash.update(message.substr(at, piece));
    }
    return hash.finishHex();
}

// A fresh directory of one test's own, in one process's own, removed when the test
// ends. The process id is in the name because ctest runs test programs side by side
// and a shared name let one remove the folder under another (test_path_text.cpp).
struct Scratch {
    fs::path root;

    explicit Scratch(const char* name)
        : root(fs::temp_directory_path() / ("katana-core-tests-sha256-" + processTag()) / name)
    {
        fs::remove_all(root);
        fs::create_directories(root);
    }
    ~Scratch()
    {
        std::error_code ignored;
        fs::remove_all(root.parent_path(), ignored);
    }

    static std::string processTag()
    {
#if defined(_WIN32)
        return std::to_string(_getpid());
#else
        return std::to_string(getpid());
#endif
    }
};

void writeFile(const fs::path& path, std::string_view bytes)
{
    std::ofstream out(path, std::ios::binary);
    out.write(bytes.data(), static_cast<std::streamsize>(bytes.size()));
    ASSERT_TRUE(out.good()) << path.string();
}

// ---- expected digests ----------------------------------------------------------------

constexpr const char* kEmpty = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
constexpr const char* kAbc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
constexpr const char* k448Bits = "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1";
constexpr const char* k896Bits = "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1";
constexpr const char* kMillionA =
    "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0";

constexpr std::string_view k448BitMessage =
    "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
constexpr std::string_view k896BitMessage =
    "abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnop"
    "qrsmnopqrstnopqrstu";

// The piece sizes the streaming tests cut a message into: one byte, a few, a block, a
// block and one over.
constexpr std::size_t kPieceSizes[] = {1, 7, 64, 65};

struct LengthCase {
    std::size_t length;
    const char* digest;
};

// hashlib.sha256(b"a" * n), n either side of the padding boundaries: 55 bytes is the
// most a single block holds with the 0x80 byte and the length, 56 forces a second
// block, 63/64/65 are the block edge, 119/120 and 127/128/129 the same one block on.
constexpr LengthCase kRepeatedA[] = {
    {1, "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb"},
    {55, "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318"},
    {56, "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a"},
    {57, "f13b2d724659eb3bf47f2dd6af1accc87b81f09f59f2b75e5c0bed6589dfe8c6"},
    {63, "7d3e74a05d7db15bce4ad9ec0658ea98e3f06eeecf16b4c6fff2da457ddc2f34"},
    {64, "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb"},
    {65, "635361c48bb9eab14198e76ea8ab7f1a41685d6ad62aa9146d301d4f17eb0ae0"},
    {119, "31eba51c313a5c08226adf18d4a359cfdfd8d2e816b13f4af952f7ea6584dcfb"},
    {120, "2f3d335432c70b580af0e8e1b3674a7c020d683aa5f73aaaedfdc55af904c21c"},
    {121, "e9615320128cc7a3d6078e9af05603188e5ccbf0d07d8b735d3df5e8e0c1281f"},
    {127, "c57e9278af78fa3cab38667bef4ce29d783787a2f731d4e12200270f0c32320a"},
    {128, "6836cf13bac400e9105071cd6af47084dfacad4e5e302c94bfed24e013afb73e"},
    {129, "c12cb024a2e5551cca0e08fce8f1c5e314555cc3fef6329ee994a3db752166ae"},
};

// hashlib.sha256(pattern of n bytes). Lengths 262144 and 524288 are one and two
// whole file-reading chunks (256 KiB), 1048593 is four chunks and 17 bytes over.
constexpr const char* kPattern1048593 =
    "0f08b3a8f00fd6361440a1187acb2c83bb3e1f8cb5c395d541bcb11a3b408c4b";
constexpr LengthCase kPattern[] = {
    {0, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"},
    {1, "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d"},
    {55, "576a1bf8d4478657e6dc4af9398544765c2a92cde28478b019235cfed315fc09"},
    {56, "9b20501dfd1d99161c257950f3444f3e49230c351c5c8e0943ef369f85f5205d"},
    {63, "30b345906b493f06f69444b6521113511c242f30e29840462950035043682f1e"},
    {64, "d8bc63b4fc1156e5e7d95a418b9bf54cd3174bedbc2db40f74895349b229b3c0"},
    {65, "1ee23b0fbcaecc1aff4a9e8f1645f35ab2c8e13609cd73b68df8b5e3f63ce073"},
    {127, "67d79933e3c9aa8e89f481e071cfca1e9a16c09d7263b5efa3bb01f1a9f8d065"},
    {128, "54c9eb041badfd7064645067b107661fed6113197ce2dd066ba69618abd3732f"},
    {129, "5b55715b51c54661ed0cbd75ba5b00cf1ffee3b7c113a636eca613d9425116a6"},
    {100000, "55af394c980c7a7fb68aa904c4afdd93d76e5f826487105fc06f92a25bab8cbe"},
    {262144, "23295ac6e56186bdc6715065c52588ed68859187befcff701de609c7841ab38f"},
    {524288, "202f75b6b7bea0f70d6fe412355f10ea2aaf3acd569eb2b55f70b69d4ba4f9f3"},
    {1048593, kPattern1048593},
};

// hashlib.sha256 of the pattern for p in range(64 * 1024 * 1024).
constexpr const char* kPattern64MiB =
    "35a477c6d711cfbfc35e7857d6d975474c8efb1e8c7fe956bdefc5e7caecc6a2";

// hashlib.sha256 of 2**29 + 1 zero bytes: 2**32 + 8 bits, the first message whose
// length does not fit the 32 bits a careless implementation keeps it in.
constexpr const char* kZeros2Pow29Plus1 =
    "7c40fe5ce847740d0f0d0cdde3949d6585804cdec3ae61a15b923165699c8137";

} // namespace

// ---- the standard's own messages --------------------------------------------------------

TEST(Sha256, TheStandardsOwnMessagesGiveTheStandardsDigests)
{
    EXPECT_EQ(sha256Hex(""), kEmpty);
    EXPECT_EQ(sha256Hex("abc"), kAbc);
    EXPECT_EQ(sha256Hex(k448BitMessage), k448Bits);
    EXPECT_EQ(sha256Hex(k896BitMessage), k896Bits);
    EXPECT_EQ(sha256Hex(std::string(1000000, 'a')), kMillionA);
}

TEST(Sha256, TheStandardsOwnMessagesGiveTheSameDigestsWhenStreamed)
{
    Sha256 hash;
    hash.update("a");
    hash.update("");
    hash.update("bc");
    EXPECT_EQ(hash.finishHex(), kAbc);

    // One million 'a' as the standard describes it: a million single-byte updates.
    Sha256 million;
    for (int i = 0; i < 1000000; ++i) {
        million.update("a");
    }
    EXPECT_EQ(million.finishHex(), kMillionA);

    EXPECT_EQ(hashInPieces(k896BitMessage, 17), k896Bits);
}

TEST(Sha256, MessagesEitherSideOfThePaddingBoundariesGiveHashlibsDigests)
{
    for (const LengthCase& c : kRepeatedA) {
        EXPECT_EQ(sha256Hex(std::string(c.length, 'a')), c.digest) << c.length << " x 'a'";
    }
    for (const LengthCase& c : kPattern) {
        if (c.length <= 100000) {
            EXPECT_EQ(sha256Hex(patternBytes(0, c.length)), c.digest)
                << c.length << " bytes of the pattern";
        }
    }
}

// ---- streaming equals one-shot -----------------------------------------------------------

TEST(Sha256, EveryCutOfAMessageGivesTheDigestOfTheWholeMessage)
{
    // 200 bytes cross three block edges; one cut into two pieces at every position,
    // including the ends (an empty first or second piece).
    const std::string message = patternBytes(0, 200);
    const std::string whole = sha256Hex(message);
    for (std::size_t cut = 0; cut <= message.size(); ++cut) {
        Sha256 hash;
        hash.update(std::string_view(message).substr(0, cut));
        hash.update(std::string_view(message).substr(cut));
        EXPECT_EQ(hash.finishHex(), whole) << "cut at " << cut;
    }
}

TEST(Sha256, PiecesOfOneSevenSixtyFourAndSixtyFiveBytesGiveTheOneShotDigest)
{
    // Every length from 0 to 300 reaches each padding case (the length field fitting
    // in the last block or forcing one more) and every offset of the waiting block.
    for (std::size_t length = 0; length <= 300; ++length) {
        const std::string message = patternBytes(0, length);
        const std::string whole = sha256Hex(message);
        for (const std::size_t piece : kPieceSizes) {
            ASSERT_EQ(hashInPieces(message, piece), whole)
                << length << " bytes in pieces of " << piece;
        }
    }
}

TEST(Sha256, PiecesOfDifferentSizesInOneMessageGiveHashlibsDigest)
{
    // The pattern's 1048593 bytes cut 1, 7, 64, 65 and then 1000 at a time, repeating:
    // every kind of piece meets every offset of the waiting block, and the whole is
    // checked against hashlib, not against this program's one-shot.
    const std::string message = patternBytes(0, 1048593);
    constexpr std::size_t kPieces[] = {1, 7, 64, 65, 1000};
    Sha256 hash;
    std::size_t at = 0;
    for (std::size_t i = 0; at < message.size(); ++i) {
        const std::size_t take = std::min(kPieces[i % std::size(kPieces)], message.size() - at);
        hash.update(std::string_view(message).substr(at, take));
        at += take;
    }
    EXPECT_EQ(hash.finishHex(), kPattern1048593);
}

TEST(Sha256, AnEmptyUpdateAndAViewOfNothingChangeNothing)
{
    Sha256 hash;
    hash.update(std::string_view());
    hash.update("abc");
    hash.update(std::string_view(nullptr, 0));
    EXPECT_EQ(hash.finishHex(), kAbc);
    EXPECT_EQ(Sha256().finishHex(), kEmpty);
}

TEST(Sha256, TheBytesAreReadWhereverTheyLieWithNoAlignmentRequired)
{
    // The same 200 bytes at each offset of a larger buffer: a block is read straight
    // from the caller's memory, which is not aligned to anything.
    const std::string message = patternBytes(0, 200);
    for (std::size_t offset = 0; offset < 8; ++offset) {
        std::string buffer(offset, '#');
        buffer += message;
        EXPECT_EQ(sha256Hex(std::string_view(buffer).substr(offset)), sha256Hex(message)) << offset;
    }
}

// ---- large input -------------------------------------------------------------------------

TEST(Sha256, SixtyFourMebibytesStreamedGiveHashlibsDigestWithoutHoldingThem)
{
    // 64 MiB generated a piece at a time (a prime number of bytes, so no piece starts
    // on a block edge after the first): only one piece is ever in memory.
    constexpr std::uint64_t kTotal = 64ull * 1024 * 1024;
    constexpr std::size_t kPiece = 1000003;
    Sha256 hash;
    std::string piece;
    for (std::uint64_t at = 0; at < kTotal; at += kPiece) {
        const auto count = static_cast<std::size_t>(std::min<std::uint64_t>(kPiece, kTotal - at));
        piece = patternBytes(at, count);
        hash.update(piece);
    }
    EXPECT_EQ(hash.finishHex(), kPattern64MiB);
}

TEST(Sha256, AMessageOfMoreThanFourGibibitsHasItsWholeLengthPadded)
{
    // 2**29 + 1 zero bytes is 2**32 + 8 bits: the high word of the length field is 1.
    // An implementation that kept the length in 32 bits would hash the length as 8.
    const std::string zeros(1u << 20, '\0');
    Sha256 hash;
    for (int i = 0; i < 512; ++i) {
        hash.update(zeros);
    }
    hash.update(std::string_view(zeros).substr(0, 1));
    EXPECT_EQ(hash.finishHex(), kZeros2Pow29Plus1);
}

// ---- the object -------------------------------------------------------------------------

TEST(Sha256, FinishStartsTheNextMessageFromScratch)
{
    Sha256 hash;
    hash.update("abc");
    EXPECT_EQ(hash.finishHex(), kAbc);
    // Nothing of "abc" survives: the next digest is the empty message's...
    EXPECT_EQ(hash.finishHex(), kEmpty);
    // ...and the next message is hashed as if the object were new.
    hash.update(k448BitMessage);
    EXPECT_EQ(hash.finishHex(), k448Bits);
}

TEST(Sha256, ResetForgetsWhatWasAppended)
{
    Sha256 hash;
    hash.update(std::string(1000, 'x')); // leaves bytes waiting in the block as well
    hash.reset();
    hash.update("abc");
    EXPECT_EQ(hash.finishHex(), kAbc);

    Sha256 untouched;
    untouched.reset();
    EXPECT_EQ(untouched.finishHex(), kEmpty);
}

TEST(Sha256, FinishGivesTheDigestBytesAndFinishHexTheSameAsLowerCaseText)
{
    Sha256 hash;
    hash.update("abc");
    const Sha256Digest digest = hash.finish();
    // The first and last bytes of ba7816bf...15ad, and the whole against the text.
    EXPECT_EQ(digest.front(), 0xba);
    EXPECT_EQ(digest[1], 0x78);
    EXPECT_EQ(digest.back(), 0xad);

    const auto parsed = parseSha256Hex(kAbc);
    ASSERT_TRUE(parsed.ok());
    EXPECT_EQ(digest, parsed.value());

    // Lower case, 64 characters, only 0-9a-f: an upper-case digit would make a
    // manifest comparison a byte comparison that fails.
    const auto lowerHex = [](char c) { return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f'); };
    for (const std::string& text :
         {sha256Hex("abc"), sha256Hex(""), sha256Hex(std::string(1000, 'z'))}) {
        ASSERT_EQ(text.size(), 64u);
        EXPECT_TRUE(std::all_of(text.begin(), text.end(), lowerHex)) << text;
    }
}

// ---- files -------------------------------------------------------------------------------

TEST(Sha256, AFileGivesHashlibsDigestAtEveryLengthAroundTheBlockAndTheReadingChunk)
{
    Scratch scratch("lengths");
    for (const LengthCase& c : kPattern) {
        const fs::path file = scratch.root / ("pattern-" + std::to_string(c.length) + ".bin");
        writeFile(file, patternBytes(0, c.length));
        const auto digest = sha256FileHex(file);
        ASSERT_TRUE(digest.ok()) << c.length << ": " << digest.error().describe();
        EXPECT_EQ(digest.value(), c.digest) << c.length << " bytes";
    }
}

TEST(Sha256, AFileIsHashedAsTheBytesItHoldsNotAsText)
{
    // Line ends, NUL and the 0x1A that ends a text file on some systems must reach the
    // hash unchanged. "a\r\nb\0c\x1a" is worked out the same way as the rest:
    // hashlib.sha256(b"a\r\nb\x00c\x1a").
    Scratch scratch("binary");
    const fs::path file = scratch.root / "binary.bin";
    writeFile(file, std::string_view("a\r\nb\0c\x1a", 7));
    const auto digest = sha256FileHex(file);
    ASSERT_TRUE(digest.ok()) << digest.error().describe();
    EXPECT_EQ(digest.value(), sha256Hex(std::string_view("a\r\nb\0c\x1a", 7)));
    // Also `printf 'a\r\nb\0c\x1a' | openssl dgst -sha256`.
    EXPECT_EQ(digest.value(), "657e370c762fabff180fa0a764e3cf7fd4548d387480203751a76358c7060974");
}

TEST(Sha256, AFileThatIsNotThereIsNotFoundAndNamesThePath)
{
    Scratch scratch("missing");
    const fs::path missing = scratch.root / "absent.bin";
    const auto digest = sha256FileHex(missing);
    ASSERT_FALSE(digest.ok());
    EXPECT_EQ(digest.error().code, ErrorCode::NotFound);
    EXPECT_NE(digest.error().context.find("absent.bin"), std::string::npos)
        << digest.error().describe();
}

TEST(Sha256, ADirectoryIsNotFoundRatherThanTheDigestOfAnEmptyFile)
{
    Scratch scratch("directory");
    const auto digest = sha256FileHex(scratch.root);
    ASSERT_FALSE(digest.ok()) << "a directory hashed as " << digest.value();
    EXPECT_EQ(digest.error().code, ErrorCode::NotFound);
}

// ---- the text form -----------------------------------------------------------------------

TEST(Sha256, ParseReadsSixtyFourLowerCaseHexDigitsIntoTheirBytes)
{
    const auto parsed = parseSha256Hex(kEmpty);
    ASSERT_TRUE(parsed.ok()) << parsed.error().describe();
    // e3 b0 c4 42 ... 78 52 b8 55, read off the digest above two characters at a time.
    EXPECT_EQ(parsed.value()[0], 0xe3);
    EXPECT_EQ(parsed.value()[1], 0xb0);
    EXPECT_EQ(parsed.value()[2], 0xc4);
    EXPECT_EQ(parsed.value()[3], 0x42);
    EXPECT_EQ(parsed.value()[28], 0x78);
    EXPECT_EQ(parsed.value()[29], 0x52);
    EXPECT_EQ(parsed.value()[30], 0xb8);
    EXPECT_EQ(parsed.value()[31], 0x55);

    const auto zeros = parseSha256Hex(std::string(64, '0'));
    ASSERT_TRUE(zeros.ok());
    EXPECT_EQ(zeros.value(), Sha256Digest{});
    const auto ones = parseSha256Hex(std::string(64, 'f'));
    ASSERT_TRUE(ones.ok());
    EXPECT_TRUE(std::all_of(ones.value().begin(), ones.value().end(),
                            [](std::uint8_t b) { return b == 0xff; }));
}

TEST(Sha256, ParseRefusesTheWrongLengthUpperCaseAndEveryOtherCharacter)
{
    const std::string good = kAbc;
    for (const std::string& bad : {
             std::string(), good.substr(0, 63), good + "0", std::string(32, '0'),
             std::string(128, '0'),
             " " + good.substr(1),                       // a space
             good.substr(0, 63) + "g",                   // one past 'f'
             good.substr(0, 63) + "G",                   //
             "0x" + good.substr(2),                      // a prefix in place of two digits
             good.substr(0, 10) + "B" + good.substr(11), // upper case, refused: one spelling
             good.substr(0, 20) + std::string(1, '\0') + good.substr(21),
             good.substr(0, 63) + "\n", // a trailing line end
         }) {
        const auto parsed = parseSha256Hex(bad);
        ASSERT_FALSE(parsed.ok()) << "accepted: " << bad;
        EXPECT_EQ(parsed.error().code, ErrorCode::ParseFailure);
    }
}
