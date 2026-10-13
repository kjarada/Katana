// Raw deflate with a cap (include/katana/update/inflate.hpp).
//
// Every expectation is worked out outside the code under test (CLAUDE.md, section 4):
//   - the members of the fixture releases were deflated by Python's zipfile, and their
//     SHA-256 values and sizes are the ones tools/update_make_manifest.py computed with
//     hashlib;
//   - the other streams were made with Python's zlib (compressobj, wbits -15) and checked to
//     decompress there, or - the hostile ones - written bit by bit from RFC 1951 (3.2.6, the
//     fixed Huffman code) and checked with Python's zlib.decompress(data, -15), which names
//     the error each one must raise ("invalid block type", "invalid distance too far back",
//     "invalid stored block lengths");
//   - sizes and contents of the big outputs are given by a rule (byte i is i % 251; a run of
//     zeros), not by running the code.

#include <gtest/gtest.h>

#include <algorithm>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <regex>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/sha256.hpp"
#include "katana/update/inflate.hpp"

using namespace katana;
namespace fs = std::filesystem;

namespace {

std::string fromHex(std::string_view hex)
{
    std::string bytes;
    for (std::size_t i = 0; i + 1 < hex.size(); i += 2) {
        bytes.push_back(static_cast<char>(std::stoi(std::string(hex.substr(i, 2)), nullptr, 16)));
    }
    return bytes;
}

std::string readFile(const fs::path& path)
{
    std::ifstream in(path, std::ios::binary);
    return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
}

// ---- the fixture releases (tests/update/fixtures/release/<name>) --------------------------

struct Member {
    std::string path;
    std::uint64_t csize = 0;
    std::uint64_t offset = 0;
    std::uint64_t size = 0;
    std::string sha256;
};

// One member to a line in the manifest the producer writes (sorted keys, LF).
std::vector<Member> membersOf(const std::string& release, std::string& zipBytes)
{
    const fs::path folder = fs::path(KATANA_UPDATE_FIXTURES) / "release" / release;
    const std::string manifest = readFile(folder / "Katana-update-win64.json");
    static const std::regex line(
        R"re("csize":([0-9]+),(?:"exec":true,)?"offset":([0-9]+),)re"
        R"re("path":"([^"]+)","sha256":"([0-9a-f]{64})","size":([0-9]+))re");
    std::vector<Member> members;
    for (std::sregex_iterator it(manifest.begin(), manifest.end(), line), end; it != end; ++it) {
        members.push_back({(*it)[3].str(), std::stoull((*it)[1].str()), std::stoull((*it)[2].str()),
                           std::stoull((*it)[5].str()), (*it)[4].str()});
    }
    for (const fs::directory_entry& entry : fs::directory_iterator(folder)) {
        if (entry.path().extension() == ".zip") {
            zipBytes = readFile(entry.path());
        }
    }
    return members;
}

// ---- a deflate stream written by hand -----------------------------------------------------

// RFC 1951, 3.2.6, the fixed Huffman code: one block holding the literal 0, then `matches`
// copies of "258 bytes from distance 1", then (when `last` is 3 to 10) one more copy of
// `last` bytes: 1 + 258 * matches + last zero bytes in 3 + 8 + 13 * matches + 12 + 7 bits.
// This is how a 64 MB output is a few hundred kilobytes.
class BitWriter {
  public:
    void lsb(unsigned value, int count)
    {
        for (int i = 0; i < count; ++i) {
            bits_.push_back((value >> i) & 1U);
        }
    }
    void huffman(unsigned code, int count)
    {
        for (int i = count - 1; i >= 0; --i) {
            bits_.push_back((code >> i) & 1U);
        }
    }
    std::string bytes() const
    {
        std::string out((bits_.size() + 7) / 8, '\0');
        for (std::size_t i = 0; i < bits_.size(); ++i) {
            out[i / 8] =
                static_cast<char>(static_cast<unsigned char>(out[i / 8]) | (bits_[i] << (i % 8)));
        }
        return out;
    }

  private:
    std::vector<unsigned> bits_;
};

std::string zeroRun(std::size_t matches, unsigned last = 0)
{
    BitWriter w;
    w.lsb(1, 1);        // BFINAL
    w.lsb(1, 2);        // BTYPE 01: fixed Huffman
    w.huffman(0x30, 8); // literal 0: codes 0x30 + value for 0..143
    for (std::size_t i = 0; i < matches; ++i) {
        w.huffman(0xC0 + 5, 8); // length code 285 = 258 bytes (codes 280..287 are 0xC0 + n - 280)
        w.huffman(0, 5);        // distance code 0 = 1 back
    }
    if (last != 0) {
        w.huffman(last - 3 + 1, 7); // length codes 257..264 are 3..10 bytes: the 7-bit codes 1..8
        w.huffman(0, 5);
    }
    w.huffman(0, 7); // end of block (256)
    return w.bytes();
}

// ---- streams made with Python's zlib ------------------------------------------------------

// b"Hello, Katana update!\n" * 40, level 6 (880 bytes in 34).
const std::string kHelloPlain = [] {
    std::string text;
    for (int i = 0; i < 40; ++i) {
        text += "Hello, Katana update!\n";
    }
    return text;
}();
const std::string kHello =
    fromHex("f348cdc9c9d751f04e2c49cc4b54282d48492c4955e4f218151d151d15d5a1be2800");

// b"" : the empty final block, 03 00.
const std::string kEmpty = fromHex("0300");

// b"stored bytes" at level 0: one stored block (RFC 1951, 3.2.4).
const std::string kStored = fromHex("010c00f3ff73746f726564206279746573");

// b"abc", a full flush (an empty stored block), b"def": two blocks, the first not final.
const std::string kBlocks = fromHex("4a4c4a06000000ffff4b494d0300");

// 502 bytes of text and a run of the printable ASCII range, level 9 (a dynamic-code block);
// sha256 077b7592ff7244e65e4b7c15c4bf0202ae4682b4a9fa35e1c43fcf4805799a8d (hashlib).
const std::string kText =
    fromHex("0bc94855282ccd4cce56482aca2fcf5348cbaf50c82acd2d2856c82f4b2d5228014ae72456552aa4e4a7eb"
            "2984d04c71"
            "4022505d6ea54212505179664986425a66592a50aa2a354f2127b3b034bf08a837bd9804850a8a4aca2aaa"
            "6aea1a9a5a"
            "da3aba7afa068646c626a666e6169656d636b676f60e8e4ece2eae6eee1e9e5ede3ebe7efe018141c121a1"
            "61e1119151"
            "d131b171f1098949c929a969e9199959d939b979f9058545c525a565e5159555d535b57543db7800");
constexpr std::string_view kTextSha256 =
    "077b7592ff7244e65e4b7c15c4bf0202ae4682b4a9fa35e1c43fcf4805799a8d";

// bytes((i % 251) for i in range(300000)), level 9: 1482 bytes in dynamic blocks.
const std::string kPattern =
    fromHex("edcf539610000000c0cdb6b9d9b66ddbb66dd766dbb66ddbb66d5b5fdda33773830908163c44c850a1c384"
            "0d173e42c4"
            "4891a3448d163d46cc58b1e3c48d173f41c244890393244d963c45ca54a9d3a44d973e43c64c99b364cd96"
            "3d47ce5cb9"
            "f3e4cd97bf40c142858b142d56bc44c952a5cb942d57be42c54a95ab54ad56bd46cd5ab5ebd4ad57bf41c3"
            "468d9b346d"
            "d6bc45cb56addbb46dd7be43c74e9dbb74edd6bd47cf5ebdfbf4edd77fc0c04183870c1d16347cc4c851a3"
            "c78c1d377e"
            "c2c44993a74c9d367dc6cc59b3e7cc9d377fc1c2458b972c5db67cc5ca55abd7ac5db77ec3c64d9bb76cdd"
            "b67dc7ce5d"
            "bbf7ecddb7ffc0c143878f1c3d76fcc4c953a7cf9c3d77fec2c54b97af5cbd76fdc6cd5bb7efdcbd77ffc1"
            "c3478f9f3c"
            "7df6fcc5cb57afdfbc7df7fec3c74f9fbf7cfdf6fdc7cf5fbffffc0d505757575757575757575757575757"
            "5757575757"
            "57575757575757575757575757575757575757575757575757575757575757575757575757575757575757"
            "5757575757"
            "57575757575757575757575757575757575757575757575757575757575757575757575757575757575757"
            "5757575757"
            "57575757575757575757575757575757575757575757575757575757575757575757575757575757575757"
            "5757575757"
            "57575757575757575757575757575757575757575757575757575757575757575757575757575757575757"
            "5757575757"
            "57575757575757575757575757575757575757575757575757575757575757575757575757575757575757"
            "5757575757"
            "57575757575757575757575757575757575757575757575757575757575757575757575757575757575757"
            "5757575757"
            "57575757575757575757575757575757575757575757575757575757575757575757575757575757575757"
            "5757575757"
            "57575757575757575757575757575757575757575757575757575757575757575757575757575757575757"
            "5757575757"
            "57575757575757575757575757575757575757575757575757575757575757575757575757575757575757"
            "5757575757"
            "57575757575757575757575757575757575757575757575757575757575757575757575757575757575757"
            "5757575757"
            "57575757575757575757575757575757575757575757575757575757575757575757575757575757575757"
            "5757575757"
            "57575757575757575757575757575757575757575757575757575757575757575757575757575757575757"
            "5757575757"
            "57575757575757575757575757575757575757575757575757575757575757575757575757575757575757"
            "5757575757"
            "57575757575757575757575757575757575757575757575757575757575757575757575757575757575757"
            "5757575757"
            "57575757575757575757575757575757575757575757575757575757575757575757575757575757575757"
            "5757575757"
            "57575757575757575757575757575757575757575757575757575757575757575757575757575757575757"
            "5757575757"
            "57575757575757575757575757575757575757575757575757575757575757575757575757575757575757"
            "5757575757"
            "57575757575757575757575757575757575757575757575757575757575757575757575757575757575757"
            "5757575757"
            "57575757575757575757575757575757575757575757575757575757575757575757575757575757575757"
            "5757575757"
            "57575757575757575757575757575757575757575757575757575757575757575757575757575757575757"
            "5757575757"
            "57575757575757575757575757575757575757575757575757575757575757575757575757575757575757"
            "5757575757"
            "57575757575757575757575757575757575757575757575757575757575757575757575757575757575757"
            "5757575757"
            "57575757575757575757575757575757575757575757575757575757575757575757575757575757575757"
            "5757575757"
            "57575757575757575757575757575757575757575757575757575757575757575757575757ffffeaff00");
// hashlib.sha256 of the 300000 bytes.
constexpr std::string_view kPatternSha256 =
    "3c65ea93424a9c362fec0e3a69ea36031e8a358441479dd665cc6110eabe7b08";

// Collects what the sink is given.
struct Collected {
    std::string bytes;
    std::vector<std::size_t> blocks;
    update::InflateSink sink()
    {
        return [this](std::string_view block) {
            blocks.push_back(block.size());
            bytes.append(block);
            return core::Status{};
        };
    }
};

core::ErrorCode codeOf(const core::Status& status)
{
    return status.error().code;
}

} // namespace

// ---- good input ---------------------------------------------------------------------------

TEST(Inflate, EveryMemberOfTheFixtureReleasesInflatesToTheBytesTheManifestHashes)
{
    // Two releases, 9 and 10 members, deflated by zipfile; the hashes are hashlib's.
    const struct {
        const char* release;
        std::size_t count;
    } releases[] = {{"v1", 9}, {"v2", 10}};
    for (const auto& release : releases) {
        std::string zip;
        const std::vector<Member> members = membersOf(release.release, zip);
        ASSERT_EQ(members.size(), release.count) << release.release;
        ASSERT_FALSE(zip.empty());
        for (const Member& member : members) {
            ASSERT_LE(member.offset + member.csize, zip.size()) << member.path;
            const auto data = update::inflateRawToString(
                std::string_view(zip).substr(member.offset, member.csize), member.size);
            ASSERT_TRUE(data.ok())
                << release.release << " " << member.path << ": " << data.error().describe();
            EXPECT_EQ(data->size(), member.size) << member.path;
            EXPECT_EQ(core::sha256Hex(*data), member.sha256) << member.path;
        }
    }
}

TEST(Inflate, AnEmptyFileIsTheTwoByteEmptyFinalBlock)
{
    const auto data = update::inflateRawToString(kEmpty, 0);
    ASSERT_TRUE(data.ok()) << data.error().describe();
    EXPECT_TRUE(data->empty());
    Collected collected;
    EXPECT_TRUE(update::inflateRaw(kEmpty, 0, collected.sink()).ok());
    EXPECT_TRUE(collected.blocks.empty()) << "the sink is not called with an empty block";
}

TEST(Inflate, TextCompressedByZlibComesBackWhole)
{
    const auto data = update::inflateRawToString(kHello, kHelloPlain.size());
    ASSERT_TRUE(data.ok()) << data.error().describe();
    EXPECT_EQ(*data, kHelloPlain);
}

TEST(Inflate, AStoredBlockIsCopiedThrough)
{
    const auto data = update::inflateRawToString(kStored, 12);
    ASSERT_TRUE(data.ok()) << data.error().describe();
    EXPECT_EQ(*data, "stored bytes");
}

TEST(Inflate, ABlockThatIsNotFinalIsFollowedToTheNext)
{
    const auto data = update::inflateRawToString(kBlocks, 6);
    ASSERT_TRUE(data.ok()) << data.error().describe();
    EXPECT_EQ(*data, "abcdef");
}

TEST(Inflate, ADynamicCodeBlockInflatesToTheBytesHashlibHashed)
{
    const auto data = update::inflateRawToString(kText, 502);
    ASSERT_TRUE(data.ok()) << data.error().describe();
    EXPECT_EQ(data->size(), 502u);
    EXPECT_EQ(core::sha256Hex(*data), kTextSha256);
}

TEST(Inflate, AnOutputOfSeveralBlocksIsHandedOverInOrderInBoundedPieces)
{
    Collected collected;
    ASSERT_TRUE(update::inflateRaw(kPattern, 300000, collected.sink()).ok());
    EXPECT_EQ(collected.bytes.size(), 300000u);
    EXPECT_EQ(core::sha256Hex(collected.bytes), kPatternSha256);
    EXPECT_GT(collected.blocks.size(), 1u)
        << "the output is streamed, not held and handed over whole";
    for (const std::size_t size : collected.blocks) {
        EXPECT_GT(size, 0u);
        EXPECT_LE(size, 1u << 20) << "a block is bounded, whatever the output";
    }
    // Byte i is i % 251 (the rule that built the stream), whatever the block boundaries were.
    std::size_t wrong = 0;
    for (std::size_t i = 0; i < collected.bytes.size(); ++i) {
        wrong += static_cast<unsigned char>(collected.bytes[i]) != i % 251;
    }
    EXPECT_EQ(wrong, 0u);
}

TEST(Inflate, ALongRunOfZerosWrittenByHandInflatesToExactlyThatManyZeros)
{
    // 1 + 258 * 2000 bytes, from a stream written in this file from RFC 1951.
    const std::string stream = zeroRun(2000);
    EXPECT_LT(stream.size(), 4000u);
    const auto data = update::inflateRawToString(stream, 1 + 258 * 2000);
    ASSERT_TRUE(data.ok()) << data.error().describe();
    EXPECT_EQ(data->size(), 516001u);
    EXPECT_EQ(std::count(data->begin(), data->end(), '\0'), 516001);
}

// ---- input that ends early or is not deflate ----------------------------------------------

TEST(Inflate, EveryProperPrefixOfAStreamIsRefusedAsTruncated)
{
    struct Case {
        const std::string* stream;
        std::uint64_t size;
    };
    const Case cases[] = {
        {&kHello, 880}, {&kText, 502}, {&kBlocks, 6}, {&kStored, 12}, {&kPattern, 300000}};
    std::size_t cuts = 0;
    std::size_t possible = 0;
    for (const Case& c : cases) {
        possible += c.stream->size();
        for (std::size_t length = 0; length < c.stream->size(); ++length) {
            const core::Status status =
                update::inflateRaw(std::string_view(*c.stream).substr(0, length), c.size,
                                   [](std::string_view) { return core::Status{}; });
            ASSERT_FALSE(status.ok()) << "length " << length << " of " << c.stream->size();
            EXPECT_EQ(codeOf(status), core::ErrorCode::ParseFailure)
                << "length " << length << " of " << c.stream->size();
            ++cuts;
        }
    }
    EXPECT_EQ(cuts, possible) << "every cut point of every stream was tried";
    EXPECT_GT(cuts, 1700u);
}

TEST(Inflate, NoInputAtAllIsATruncatedStreamEvenForAnEmptyFile)
{
    EXPECT_EQ(codeOf(update::inflateRaw("", 0, [](std::string_view) { return core::Status{}; })),
              core::ErrorCode::ParseFailure);
    EXPECT_EQ(codeOf(update::inflateRaw("", 5, [](std::string_view) { return core::Status{}; })),
              core::ErrorCode::ParseFailure);
}

TEST(Inflate, DataThatIsNotDeflateIsRefused)
{
    // Python's zlib.decompress(data, -15) refuses each of these, with the reason in the comment.
    const struct {
        const char* what;
        std::string bytes;
    } cases[] = {
        {"reserved block type 11 (invalid block type)", fromHex("07")},
        {"all ones (invalid block type)", fromHex("ffffffff")},
        {"a stored block whose NLEN is not LEN's complement (invalid stored block lengths)",
         fromHex("01050000006162636465")},
        {"a copy from before the start of the output (invalid distance too far back)",
         fromHex("4b044200")},
        {"text (invalid code lengths set)", "this is not deflate data at all"},
    };
    for (const auto& c : cases) {
        const auto data = update::inflateRawToString(c.bytes, 10);
        ASSERT_FALSE(data.ok()) << c.what;
        EXPECT_EQ(data.error().code, core::ErrorCode::ParseFailure) << c.what;
    }
}

// ---- the signed size: too long, too short, a lie ------------------------------------------

TEST(Inflate, AMemberThatInflatesToMoreThanItsSignedSizeIsCutOffAndRefused)
{
    Collected collected;
    const core::Status status = update::inflateRaw(kHello, 879, collected.sink());
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(codeOf(status), core::ErrorCode::InvalidArgument);
    EXPECT_LE(collected.bytes.size(), 879u) << "not one byte past the cap reached the sink";
}

TEST(Inflate, AMemberSignedAsEmptyThatHoldsDataIsRefusedBeforeAnyOfItIsDelivered)
{
    Collected collected;
    const core::Status status = update::inflateRaw(kHello, 0, collected.sink());
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(codeOf(status), core::ErrorCode::InvalidArgument);
    EXPECT_TRUE(collected.bytes.empty());
}

TEST(Inflate, AMemberThatInflatesToLessThanItsSignedSizeIsRefused)
{
    for (const std::uint64_t claimed : {881ull, 1000000ull, UINT64_MAX}) {
        const auto data = update::inflateRawToString(kHello, claimed);
        // The largest claims are refused up front by the string form, the others by the
        // length check: both are InvalidArgument, and both are before the data is believed.
        ASSERT_FALSE(data.ok()) << claimed;
        EXPECT_EQ(data.error().code, core::ErrorCode::InvalidArgument) << claimed;
    }
    // The streaming form takes any claim: it never allocates from it.
    EXPECT_EQ(codeOf(update::inflateRaw(kHello, UINT64_MAX,
                                        [](std::string_view) { return core::Status{}; })),
              core::ErrorCode::InvalidArgument);
    EXPECT_EQ(
        codeOf(update::inflateRaw(kEmpty, 1, [](std::string_view) { return core::Status{}; })),
        core::ErrorCode::InvalidArgument)
        << "an empty member signed as one byte";
}

TEST(Inflate, AStreamThatWouldFillTheDiskIsStoppedAtTheCap)
{
    // 1 + 258 * 260000 = 67 MB of zeros from 420 KB of input, against a signed size of 1000.
    const std::string bomb = zeroRun(260000);
    ASSERT_LT(bomb.size(), 500000u);
    Collected collected;
    const core::Status status = update::inflateRaw(bomb, 1000, collected.sink());
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(codeOf(status), core::ErrorCode::InvalidArgument);
    EXPECT_LE(collected.bytes.size(), 1000u);
    // And the string form, which holds its output, holds no more than the cap either.
    const auto text = update::inflateRawToString(bomb, 1000);
    ASSERT_FALSE(text.ok());
    EXPECT_EQ(text.error().code, core::ErrorCode::InvalidArgument);
}

TEST(Inflate, ACapExactlyAtTheStreamLengthAcceptsAndOneLessRefuses)
{
    const std::string stream = zeroRun(300); // 1 + 258 * 300 = 77401 bytes, more than one block
    ASSERT_TRUE(update::inflateRawToString(stream, 77401).ok());
    EXPECT_FALSE(update::inflateRawToString(stream, 77400).ok());
    EXPECT_FALSE(update::inflateRawToString(stream, 77402).ok());
    // The same where the output ends exactly on, and one past, the 64 KiB (65536) a block of
    // working memory might hold: 1 + 258 * 254 = 65533, plus a copy of 3 or of 4 bytes.
    for (const unsigned last : {3u, 4u}) {
        const std::string edge = zeroRun(254, last);
        const std::uint64_t size = 65533 + last;
        ASSERT_TRUE(update::inflateRawToString(edge, size).ok()) << size;
        EXPECT_FALSE(update::inflateRawToString(edge, size - 1).ok()) << size;
        EXPECT_FALSE(update::inflateRawToString(edge, size + 1).ok()) << size;
    }
}

TEST(Inflate, BytesAfterTheEndOfTheStreamAreRefused)
{
    for (const std::string& tail : {std::string(1, '\0'), kEmpty, std::string("garbage after")}) {
        const auto data = update::inflateRawToString(kHello + tail, kHelloPlain.size());
        ASSERT_FALSE(data.ok());
        EXPECT_EQ(data.error().code, core::ErrorCode::InvalidArgument);
        EXPECT_NE(data.error().message.find("after the end"), std::string::npos)
            << data.error().message;
    }
}

// ---- the sink -----------------------------------------------------------------------------

TEST(Inflate, ASinkThatFailsStopsTheInflationAndItsErrorComesBackUnchanged)
{
    int calls = 0;
    const core::Status status =
        update::inflateRaw(kPattern, 300000, [&calls](std::string_view) -> core::Status {
            if (++calls == 2) {
                return core::makeError(core::ErrorCode::FileExportFailure, "disk full",
                                       "stage/bin/x");
            }
            return {};
        });
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, core::ErrorCode::FileExportFailure);
    EXPECT_EQ(status.error().message, "disk full");
    EXPECT_EQ(status.error().context, "stage/bin/x");
    EXPECT_EQ(calls, 2) << "nothing is delivered after a refusal";
}

TEST(Inflate, AMemberLargerThanTheStringFormHoldsIsRefusedBeforeAnyWork)
{
    const auto data = update::inflateRawToString(kHello, 1ull << 40);
    ASSERT_FALSE(data.ok());
    EXPECT_EQ(data.error().code, core::ErrorCode::InvalidArgument);
}
