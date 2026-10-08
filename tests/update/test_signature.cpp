// Ed25519 verification and the signature line (include/katana/update/signature.hpp).
//
// Every expectation here comes from OUTSIDE the code under test (CLAUDE.md, section 4):
//
//   - RFC 8032, section 7.1, TEST 1 to 3: the public key, message and signature the RFC prints.
//     The same values were re-derived from the RFC's secret keys by a pure-Python implementation
//     written from RFC 8032 sections 5.1 and 6 (not by this library), and TEST 2 is also accepted
//     by `openssl pkeyutl -verify -rawin`.
//   - The base64 of those signatures, the group order L and the eight points of small order with
//     their non-canonical aliases, a mixed-order point and an off-curve y: the same Python,
//     which decodes points as RFC 8032, 5.1.3 does and finds each small-order point as L * Q for
//     a point Q of the curve. S + L of TEST 2 is a signature the verification equation holds for
//     (shown in that Python) and that RFC 8032, 5.1.7 forbids.
//   - tests/update/fixtures/release/**: manifests signed by the `openssl` command line
//     (tools/update_make_fixtures.py, task T3), the independent signer of the whole system.
//   - The all-zero public key with an all-zero signature, which `openssl pkeyutl -verify` (3.6.5)
//     ACCEPTS for the one-byte messages 9, 14, 17, 19, 26, 27, 39, 47 ... (measured over all 256).
//
// What each case would catch is named in its comment: removing the explicit S check, the
// key check or the strict base64 makes the case after it fail.

#include <gtest/gtest.h>

#include <algorithm>
#include <array>
#include <atomic>
#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iterator>
#include <optional>
#include <string>
#include <string_view>
#include <thread>
#include <vector>

#include "katana/update/feed.hpp"
#include "katana/update/signature.hpp"

#ifndef KATANA_UPDATE_FIXTURES
#error "tests/update/CMakeLists.txt defines KATANA_UPDATE_FIXTURES"
#endif

using namespace katana;

namespace {

// ---- bytes the tests build for themselves, without the library under test ------------------

int nibble(char c)
{
    return c <= '9' ? c - '0' : c - 'a' + 10; // the literals below are lower-case hexadecimal
}

std::string bytesFromHex(std::string_view hex)
{
    std::string bytes;
    for (std::size_t i = 0; i + 1 < hex.size(); i += 2) {
        bytes.push_back(static_cast<char>(nibble(hex[i]) * 16 + nibble(hex[i + 1])));
    }
    return bytes;
}

template <std::size_t N> std::array<std::uint8_t, N> arrayFromHex(std::string_view hex)
{
    const std::string bytes = bytesFromHex(hex);
    std::array<std::uint8_t, N> out{};
    EXPECT_EQ(bytes.size(), N);
    std::copy_n(bytes.begin(), std::min(bytes.size(), N), out.begin());
    return out;
}

update::PublicKey keyFromHex(std::string_view hex)
{
    return arrayFromHex<32>(hex);
}

update::SignatureBytes signatureFromHex(std::string_view hex)
{
    return arrayFromHex<64>(hex);
}

std::string encodeBase64(const std::string& bytes)
{
    constexpr std::string_view alphabet =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    std::string text;
    for (std::size_t i = 0; i < bytes.size(); i += 3) {
        const std::size_t left = bytes.size() - i;
        const std::uint32_t b0 = static_cast<std::uint8_t>(bytes[i]);
        const std::uint32_t b1 = left > 1 ? static_cast<std::uint8_t>(bytes[i + 1]) : 0;
        const std::uint32_t b2 = left > 2 ? static_cast<std::uint8_t>(bytes[i + 2]) : 0;
        const std::uint32_t bits = (b0 << 16) | (b1 << 8) | b2;
        text.push_back(alphabet[(bits >> 18) & 63]);
        text.push_back(alphabet[(bits >> 12) & 63]);
        text.push_back(left > 1 ? alphabet[(bits >> 6) & 63] : '=');
        text.push_back(left > 2 ? alphabet[bits & 63] : '=');
    }
    return text;
}

std::string readFixture(const std::string& relative)
{
    std::ifstream in(std::string(KATANA_UPDATE_FIXTURES) + "/" + relative, std::ios::binary);
    EXPECT_TRUE(in.good()) << relative;
    return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
}

// ---- RFC 8032, section 7.1 ------------------------------------------------------------------

struct RfcVector {
    const char* name;
    const char* publicKey;
    const char* message; // hex
    const char* signature;
    const char* signatureBase64; // from the Python, base64.b64encode
};

constexpr RfcVector kRfcVectors[] = {
    {"TEST 1", "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a", "",
     "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
     "5VZDAMNgrHKQhuLMgG6CioSHfx645dl02HPgZSJJAVVfuIIVkKM7rMYeOXAc+bRr0lv18FlbviRlUUFDjnoQCw=="},
    {"TEST 2", "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c", "72",
     "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
     "kqAJqfDUyrhyDoILX2QlQKKye1QWUD+Ps3YiI+vbadoIWsHkPhWZbkWPNhPQ8R2MOHsurrQwKu6wDSkWErsMAA=="},
    {"TEST 3", "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025", "af82",
     "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a",
     "YpHWV97sJAJIJ+acOr4BowzlSKKEdDpEXjaA19taw6wY/5tTjRbykK5n92CYTcZZSnwV6XFu0o3AJ77O6h7ECg=="},
};

constexpr const char* kTest1Public = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
constexpr const char* kTest2Public = "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c";
constexpr const char* kTest3Public = "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025";

bool contains(const std::string& text, std::string_view part)
{
    return text.find(part) != std::string::npos;
}

// A refusal of the signature or the key: InvalidArgument, naming `why` (header: "the message
// says which").
void expectRefused(const core::Status& status, std::string_view why)
{
    ASSERT_FALSE(status.ok()) << "accepted, but it must be refused: " << why;
    EXPECT_EQ(status.error().code, core::ErrorCode::InvalidArgument) << status.error().describe();
    EXPECT_TRUE(contains(status.error().message, why)) << status.error().describe();
}

constexpr std::string_view kNotBelowOrder = "S is not below the group order";
constexpr std::string_view kKeyNotAccepted = "public key is not accepted";
constexpr std::string_view kDoesNotMatch = "does not match";

} // namespace

// ---- key ids and public keys ----------------------------------------------------------------

TEST(Signature, TheKeyIdIsTheFirstSixteenLowerCaseHexDigitsOfTheKey)
{
    // The digits are the RFC's own: the public key printed in section 7.1, cut after 8 bytes.
    EXPECT_EQ(update::keyIdOf(keyFromHex(kTest1Public)), "d75a980182b10ab7");
    EXPECT_EQ(update::keyIdOf(keyFromHex(kTest2Public)), "3d4017c3e843895a");
    EXPECT_EQ(update::keyIdOf(keyFromHex(kTest3Public)), "fc51cd8e6218a1a3");

    // A leading zero byte keeps both of its digits, and the digits are lower case.
    update::PublicKey zeros{};
    zeros[1] = 0x0a;
    zeros[2] = 0xff;
    zeros[7] = 0xbc;
    EXPECT_EQ(update::keyIdOf(zeros), "000aff00000000bc");
    EXPECT_EQ(update::keyIdOf(keyFromHex(std::string(64, 'f'))), "ffffffffffffffff");
}

TEST(Signature, APublicKeyParsesFromExactlySixtyFourLowerCaseHexDigits)
{
    for (const RfcVector& vector : kRfcVectors) {
        const auto parsed = update::parsePublicKeyHex(vector.publicKey);
        ASSERT_TRUE(parsed.ok()) << vector.name;
        EXPECT_EQ(parsed.value(), keyFromHex(vector.publicKey)) << vector.name;
    }
    const std::string good = kTest1Public;
    const std::vector<std::string> refused = {
        "",
        good.substr(0, 63),
        good + "0",
        std::string(good.size() - 1, 'a') + "g",
        "D75A980182B10AB7D54BFED3C964073A0EE172F3DAA62325AF021A68F707511A", // upper case
        good.substr(0, 63) + " ",
        " " + good.substr(1),
        "0x" + good.substr(2),
        good.substr(0, 10) + std::string(1, '\0') + good.substr(11),
    };
    for (const std::string& text : refused) {
        const auto parsed = update::parsePublicKeyHex(text);
        ASSERT_FALSE(parsed.ok()) << "accepted: '" << text << "'";
        EXPECT_EQ(parsed.error().code, core::ErrorCode::ParseFailure);
    }
}

// ---- the signature line ---------------------------------------------------------------------

TEST(Signature, ASignatureLineOfTheRfcVectorsParsesToTheRfcBytes)
{
    // The base64 is the Python's, the bytes are the RFC's: a decoder that is off by one bit
    // anywhere in 64 bytes cannot agree with both.
    for (const RfcVector& vector : kRfcVectors) {
        const std::string line = std::string("ed25519 ") + update::keyIdOf(keyFromHex(vector.publicKey)) +
                                 " " + vector.signatureBase64 + "\n";
        const auto parsed = update::parseSignatureLine(line);
        ASSERT_TRUE(parsed.ok()) << vector.name << ": " << parsed.error().describe();
        EXPECT_EQ(parsed.value().algorithm, "ed25519");
        EXPECT_EQ(parsed.value().keyId, std::string(vector.publicKey).substr(0, 16));
        EXPECT_EQ(parsed.value().signature, signatureFromHex(vector.signature)) << vector.name;
    }
}

TEST(Signature, TheSignatureLineOfAnOpensslSignedFixtureParsesToTheBytesPythonDecodes)
{
    // v2's line, decoded by Python's base64 module.
    const auto parsed = update::parseSignatureLine(readFixture("release/v2/Katana-update-win64.json.sig"));
    ASSERT_TRUE(parsed.ok()) << parsed.error().describe();
    EXPECT_EQ(parsed.value().keyId, "d75a980182b10ab7");
    EXPECT_EQ(parsed.value().signature,
              signatureFromHex("b26c00f6aed5840972037d63273b2c3ab93fc427d89ff40d3e4cc8418ec35bb173c232a2ec"
                               "fd835f38eaefb951f17d0807a6b1bed24420ac72d7accdf8ddd709"));
}

TEST(Signature, AllowsOneLineEndOfLfOrCrlfOrNone)
{
    const std::string line = readFixture("release/v2/Katana-update-win64.json.sig");
    ASSERT_EQ(line.back(), '\n'); // the producer writes the LF
    const std::string bare = line.substr(0, line.size() - 1);
    const auto reference = update::parseSignatureLine(bare);
    ASSERT_TRUE(reference.ok()) << reference.error().describe();
    for (const std::string& text : {line, bare, bare + "\r\n"}) {
        const auto parsed = update::parseSignatureLine(text);
        ASSERT_TRUE(parsed.ok()) << parsed.error().describe();
        EXPECT_EQ(parsed.value().signature, reference.value().signature);
        EXPECT_EQ(parsed.value().keyId, reference.value().keyId);
    }
}

TEST(Signature, AMalformedSignatureLineIsRefused)
{
    const std::string fixture = readFixture("release/v2/Katana-update-win64.json.sig");
    ASSERT_GE(fixture.size(), 114u);
    const std::string algorithm = fixture.substr(0, 7);
    const std::string keyId = fixture.substr(8, 16);
    const std::string encoded = fixture.substr(25, 88);
    ASSERT_EQ(algorithm, "ed25519");
    ASSERT_EQ(fixture[7], ' ');
    ASSERT_EQ(fixture[24], ' ');
    ASSERT_EQ(encoded.substr(86), "==");
    const auto assemble = [&](const std::string& a, const std::string& k, const std::string& e) {
        return a + " " + k + " " + e;
    };
    const std::string good = assemble(algorithm, keyId, encoded);
    ASSERT_TRUE(update::parseSignatureLine(good).ok()) << "the control line must parse";

    const std::string sixtyFour = bytesFromHex(
        "b26c00f6aed5840972037d63273b2c3ab93fc427d89ff40d3e4cc8418ec35bb173c232a2ecfd835f38eaefb951f17d"
        "0807a6b1bed24420ac72d7accdf8ddd709");
    ASSERT_EQ(sixtyFour.size(), 64u);
    ASSERT_EQ(encodeBase64(sixtyFour), encoded) << "the test's own encoder agrees with the fixture";

    // The last data character carries two bits and four that must be zero ('Q' is 010000);
    // 'R' is 010001, which a lenient decoder reads as the same 64 bytes.
    std::string nonCanonical = encoded;
    ASSERT_EQ(nonCanonical[85], 'Q');
    nonCanonical[85] = 'R';
    // URL-safe spellings of '+' and '/' (this signature has both).
    std::string urlSafe = encoded;
    ASSERT_NE(urlSafe.find('+'), std::string::npos);
    ASSERT_NE(urlSafe.find('/'), std::string::npos);
    std::replace(urlSafe.begin(), urlSafe.end(), '+', '-');
    std::replace(urlSafe.begin(), urlSafe.end(), '/', '_');
    std::string padInTheMiddle = encoded;
    padInTheMiddle[10] = '=';

    const std::vector<std::pair<std::string, std::string>> refused = {
        // the algorithm
        {"another algorithm", assemble("ed448", keyId, encoded)},
        {"upper-case algorithm", assemble("ED25519", keyId, encoded)},
        {"a longer algorithm word", assemble("ed25519x", keyId, encoded)},
        {"no algorithm", assemble("", keyId, encoded)},
        // the key id
        {"a key id one digit short", assemble(algorithm, keyId.substr(0, 15), encoded)},
        {"a key id one digit long", assemble(algorithm, keyId + "a", encoded)},
        {"an upper-case key id", assemble(algorithm, "D75A980182B10AB7", encoded)},
        {"a key id that is not hexadecimal", assemble(algorithm, "d75a980182b10abg", encoded)},
        {"no key id", assemble(algorithm, "", encoded)},
        // the signature text
        {"63 bytes (84 characters)", assemble(algorithm, keyId, encodeBase64(sixtyFour.substr(0, 63)))},
        // 88 characters, so the length test alone passes them: 65 bytes end in one pad and 66 in none
        {"65 bytes (88 characters, one pad)", assemble(algorithm, keyId, encodeBase64(sixtyFour + "x"))},
        {"66 bytes (88 characters, no pad)", assemble(algorithm, keyId, encodeBase64(sixtyFour + "xy"))},
        {"67 bytes (92 characters)", assemble(algorithm, keyId, encodeBase64(sixtyFour + "xyz"))},
        {"32 bytes", assemble(algorithm, keyId, encodeBase64(sixtyFour.substr(0, 32)))},
        {"no signature", assemble(algorithm, keyId, "")},
        {"the padding cut off", assemble(algorithm, keyId, encoded.substr(0, 86))},
        {"one pad where two belong", assemble(algorithm, keyId, encoded.substr(0, 87))},
        {"three pads", assemble(algorithm, keyId, encoded.substr(0, 85) + "===")},
        {"a pad in the middle", assemble(algorithm, keyId, padInTheMiddle)},
        {"non-canonical last character", assemble(algorithm, keyId, nonCanonical)},
        {"the URL-safe alphabet", assemble(algorithm, keyId, urlSafe)},
        {"a character outside the alphabet", assemble(algorithm, keyId, encoded.substr(0, 40) + "*" + encoded.substr(41))},
        {"a non-ASCII byte", assemble(algorithm, keyId, encoded.substr(0, 40) + "\xC3" + encoded.substr(41))},
        // the separators
        {"two spaces after the algorithm", algorithm + "  " + keyId + " " + encoded},
        {"two spaces after the key id", algorithm + " " + keyId + "  " + encoded},
        {"a tab for a space", algorithm + "\t" + keyId + " " + encoded},
        {"a space inside the signature", assemble(algorithm, keyId, encoded.substr(0, 44) + " " + encoded.substr(44))},
        {"a leading space", " " + good},
        {"a trailing space", good + " "},
        {"a trailing space and LF", good + " \n"},
        {"a trailing tab", good + "\t"},
        {"two fields", algorithm + " " + encoded},
        {"four fields", good + " extra"},
        // the lines
        {"nothing", ""},
        {"an empty line", "\n"},
        {"only CRLF", "\r\n"},
        {"two line ends", good + "\n\n"},
        {"two CRLF", good + "\r\n\r\n"},
        {"a bare CR", good + "\r"},
        {"a CR before the LF twice", good + "\r\r\n"},
        {"a leading empty line", "\n" + good},
        {"the line twice", good + "\n" + good + "\n"},
        {"a second line of anything", good + "\nnote\n"},
        {"a NUL after the line", good + std::string(1, '\0')},
        {"a NUL inside the key id", algorithm + " " + keyId.substr(0, 4) + std::string(1, '\0') + keyId.substr(5) + " " + encoded},
        {"longer than the cap", std::string(update::kMaxSignatureBytes + 1, 'a')},
        {"a valid line padded to the cap", good + std::string(update::kMaxSignatureBytes - good.size() + 1, '\n')},
    };
    for (const auto& [what, text] : refused) {
        const auto parsed = update::parseSignatureLine(text);
        ASSERT_FALSE(parsed.ok()) << "accepted: " << what;
        EXPECT_EQ(parsed.error().code, core::ErrorCode::ParseFailure) << what;
    }
}

TEST(Signature, AnOversizeOrTwoLineSignatureFileIsRefusedForThatReason)
{
    // The other checks would refuse these as well (the text would not be base64), so the
    // reason is what shows the cheap checks ran first: the cap is on bytes before any
    // scanning, and a second line is named as one.
    const std::string good = readFixture("release/v2/Katana-update-win64.json.sig");
    const auto oversize = update::parseSignatureLine(good + std::string(update::kMaxSignatureBytes, 'x'));
    ASSERT_FALSE(oversize.ok());
    EXPECT_EQ(oversize.error().code, core::ErrorCode::ParseFailure);
    EXPECT_TRUE(contains(oversize.error().message, "longer than")) << oversize.error().describe();

    for (const std::string& text : {good + good, good + "\n", good + "note\n", std::string("x\r") + good, good.substr(0, 40) + "\r" + good.substr(40)}) {
        const auto parsed = update::parseSignatureLine(text);
        ASSERT_FALSE(parsed.ok());
        EXPECT_EQ(parsed.error().code, core::ErrorCode::ParseFailure);
        EXPECT_TRUE(contains(parsed.error().message, "more than one line")) << parsed.error().describe();
    }
}

TEST(Signature, EverySignatureFileOfTheFixturesParsesAndNamesTheKeyItsGeneratorSaid)
{
    // tools/update_make_fixtures.py: what signed each, by its key's first 16 digits.
    struct Expected {
        const char* folder;
        const char* manifest;
        const char* keyId;
    };
    const Expected expected[] = {
        {"v1", "Katana-update-win64.json", "d75a980182b10ab7"},
        {"v2", "Katana-update-win64.json", "d75a980182b10ab7"},
        {"v2-spare-signed", "Katana-update-win64.json", "3d4017c3e843895a"},
        {"v2-macos-notify", "Katana-update-macos-arm64.json", "d75a980182b10ab7"},
        {"bad/wrong-key", "Katana-update-win64.json", "fc51cd8e6218a1a3"},
        {"bad/wrong-key-claims-active", "Katana-update-win64.json", "d75a980182b10ab7"}, // the lie
        {"bad/flipped-manifest-bit", "Katana-update-win64.json", "d75a980182b10ab7"},
        {"bad/swapped-sig", "Katana-update-win64.json", "d75a980182b10ab7"},
        {"bad/tampered-member", "Katana-update-win64.json", "d75a980182b10ab7"},
        {"bad/path-traversal", "Katana-update-win64.json", "d75a980182b10ab7"},
        {"bad/downgrade-sequence", "Katana-update-win64.json", "d75a980182b10ab7"},
        {"bad/downgrade-version", "Katana-update-win64.json", "d75a980182b10ab7"},
    };
    for (const Expected& item : expected) {
        const auto parsed = update::parseSignatureLine(
            readFixture(std::string("release/") + item.folder + "/" + item.manifest + ".sig"));
        ASSERT_TRUE(parsed.ok()) << item.folder << ": " << parsed.error().describe();
        EXPECT_EQ(parsed.value().keyId, item.keyId) << item.folder;
        EXPECT_EQ(parsed.value().algorithm, "ed25519");
    }
}

// ---- RFC 8032 known answers -----------------------------------------------------------------

TEST(Signature, Rfc8032Section71Tests1To3Verify)
{
    for (const RfcVector& vector : kRfcVectors) {
        const core::Status status = update::verifyEd25519(keyFromHex(vector.publicKey),
                                                          bytesFromHex(vector.message),
                                                          signatureFromHex(vector.signature));
        EXPECT_TRUE(status.ok()) << vector.name << ": " << (status.ok() ? "" : status.error().describe());
    }
}

TEST(Signature, EachRfcVectorFailsWhenAnyBitOfTheMessageIsFlipped)
{
    // TEST 2 and 3 sign 8 and 16 bits. TEST 1 signs the empty message, which has no bit to
    // flip; that one is held by ASignatureIsForItsOwnMessageAndKeyOnly, which lengthens it.
    for (const RfcVector& vector : kRfcVectors) {
        const std::string message = bytesFromHex(vector.message);
        for (std::size_t bit = 0; bit < message.size() * 8; ++bit) {
            std::string altered = message;
            altered[bit / 8] = static_cast<char>(static_cast<unsigned char>(altered[bit / 8]) ^ (1u << (bit % 8)));
            expectRefused(update::verifyEd25519(keyFromHex(vector.publicKey), altered,
                                                signatureFromHex(vector.signature)),
                          kDoesNotMatch);
        }
    }
}

TEST(Signature, EachRfcVectorFailsWhenAnyBitOfTheSignatureOrTheKeyIsFlipped)
{
    // All 512 bits of the signature and all 256 of the key, for all three vectors: 2304
    // refusals. Which reason differs (a flip in S can leave S above L; a flip in the key can
    // leave no point, or a point of the wrong order), but no flip may verify.
    for (const RfcVector& vector : kRfcVectors) {
        const update::PublicKey key = keyFromHex(vector.publicKey);
        const update::SignatureBytes signature = signatureFromHex(vector.signature);
        const std::string message = bytesFromHex(vector.message);
        ASSERT_TRUE(update::verifyEd25519(key, message, signature).ok()) << vector.name << ": the control";
        for (std::size_t bit = 0; bit < signature.size() * 8; ++bit) {
            update::SignatureBytes altered = signature;
            altered[bit / 8] = static_cast<std::uint8_t>(altered[bit / 8] ^ (1u << (bit % 8)));
            const core::Status status = update::verifyEd25519(key, message, altered);
            ASSERT_FALSE(status.ok()) << vector.name << ": signature bit " << bit << " flipped, still accepted";
            EXPECT_EQ(status.error().code, core::ErrorCode::InvalidArgument);
        }
        for (std::size_t bit = 0; bit < key.size() * 8; ++bit) {
            update::PublicKey altered = key;
            altered[bit / 8] = static_cast<std::uint8_t>(altered[bit / 8] ^ (1u << (bit % 8)));
            const core::Status status = update::verifyEd25519(altered, message, signature);
            ASSERT_FALSE(status.ok()) << vector.name << ": key bit " << bit << " flipped, still accepted";
            EXPECT_EQ(status.error().code, core::ErrorCode::InvalidArgument);
        }
    }
}

TEST(Signature, ASignatureIsForItsOwnMessageAndKeyOnly)
{
    const RfcVector& one = kRfcVectors[0];
    const RfcVector& two = kRfcVectors[1];
    const RfcVector& three = kRfcVectors[2];

    // Another vector's key, message or signature.
    expectRefused(update::verifyEd25519(keyFromHex(two.publicKey), bytesFromHex(one.message), signatureFromHex(one.signature)),
                  kDoesNotMatch);
    expectRefused(update::verifyEd25519(keyFromHex(one.publicKey), bytesFromHex(two.message), signatureFromHex(two.signature)),
                  kDoesNotMatch);
    expectRefused(update::verifyEd25519(keyFromHex(three.publicKey), bytesFromHex(two.message), signatureFromHex(two.signature)),
                  kDoesNotMatch);
    expectRefused(update::verifyEd25519(keyFromHex(two.publicKey), bytesFromHex(three.message), signatureFromHex(two.signature)),
                  kDoesNotMatch);

    // The empty message is not a one-byte one, and a message is not its own extension.
    expectRefused(update::verifyEd25519(keyFromHex(two.publicKey), "", signatureFromHex(two.signature)), kDoesNotMatch);
    expectRefused(update::verifyEd25519(keyFromHex(two.publicKey), std::string("\x72\x00", 2), signatureFromHex(two.signature)),
                  kDoesNotMatch);
    expectRefused(update::verifyEd25519(keyFromHex(one.publicKey), std::string(1, '\0'), signatureFromHex(one.signature)),
                  kDoesNotMatch);
    expectRefused(update::verifyEd25519(keyFromHex(one.publicKey), std::string(1 << 20, 'x'), signatureFromHex(one.signature)),
                  kDoesNotMatch);
}

// ---- S below L ------------------------------------------------------------------------------

TEST(Signature, ASignatureWhoseSIsNotBelowTheGroupOrderIsRefusedForThat)
{
    const RfcVector& vector = kRfcVectors[1];
    const update::PublicKey key = keyFromHex(vector.publicKey);
    const std::string message = bytesFromHex(vector.message);
    const std::string r = bytesFromHex(vector.signature).substr(0, 32);
    const auto signatureWith = [&](const std::string& s) {
        update::SignatureBytes out{};
        const std::string bytes = r + s;
        std::copy_n(bytes.begin(), 64, out.begin());
        return out;
    };

    // The control: TEST 2's own S verifies.
    ASSERT_TRUE(update::verifyEd25519(key, message, signatureFromHex(vector.signature)).ok());

    // S + L: the verification equation still holds for it (the Python checked), so a verifier
    // that only runs the equation accepts a second signature for the same message. This is
    // the case that fails if the explicit comparison with L is removed AND the linked
    // OpenSSL accepts it; 3.6.5 itself refuses it, so the reason below (not "does not match")
    // is what shows the comparison here ran.
    const std::string sPlusL = bytesFromHex("f52db7415978abc61b2c2eb6aeebfca0387b2eaeb4302aeeb00d291612bb0c10");
    expectRefused(update::verifyEd25519(key, message, signatureWith(sPlusL)), kNotBelowOrder);
    // S + 2L (the top three bits of the last byte are no longer zero).
    expectRefused(update::verifyEd25519(key, message, signatureWith(bytesFromHex("e201ad9e73dbbd1ef2c825598de5dbb5387b2eaeb4302aeeb00d291612bb0c20"))),
                  kNotBelowOrder);
    // S = L exactly, little-endian: edd3f55c1a631258d69cf7a2def9de14 then zeros, then 0x10.
    const std::string order = bytesFromHex("edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010");
    expectRefused(update::verifyEd25519(key, message, signatureWith(order)), kNotBelowOrder);
    // The largest 256-bit value, and the smallest above L (L + 1).
    expectRefused(update::verifyEd25519(key, message, signatureWith(std::string(32, '\xff'))), kNotBelowOrder);
    std::string orderPlusOne = order;
    orderPlusOne[0] = static_cast<char>(orderPlusOne[0] + 1); // 0xed + 1, no carry
    expectRefused(update::verifyEd25519(key, message, signatureWith(orderPlusOne)), kNotBelowOrder);
    // Only the top byte above: 0x11 where L has 0x10, whatever lies below it.
    std::string topByte = std::string(31, '\0') + std::string(1, '\x11');
    expectRefused(update::verifyEd25519(key, message, signatureWith(topByte)), kNotBelowOrder);
    // Every byte equal to L's but one that is higher, at each position (a comparison that
    // looks at the wrong end, or stops at the first difference upward, gets one of these wrong).
    for (std::size_t i = 0; i < 32; ++i) {
        std::string s = order;
        if (static_cast<unsigned char>(s[i]) == 0xff) {
            continue;
        }
        s[i] = static_cast<char>(static_cast<unsigned char>(s[i]) + 1);
        // Raising a byte of L raises S above L; lower bytes are L's own.
        expectRefused(update::verifyEd25519(key, message, signatureWith(s)), kNotBelowOrder);
    }
}

TEST(Signature, TheLargestScalarBelowTheGroupOrderIsRefusedAsAMismatchNotForItsSize)
{
    // L - 1 is a legal S (RFC 8032, 5.1.7 says S < L), so the explicit comparison lets it
    // through to OpenSSL, which finds it does not belong to this message. A comparison that
    // used <= would refuse it for its size instead.
    const RfcVector& vector = kRfcVectors[1];
    const std::string r = bytesFromHex(vector.signature).substr(0, 32);
    const std::string s = bytesFromHex("ecd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010");
    update::SignatureBytes signature{};
    const std::string bytes = r + s;
    std::copy_n(bytes.begin(), 64, signature.begin());
    expectRefused(update::verifyEd25519(keyFromHex(vector.publicKey), bytesFromHex(vector.message), signature),
                  kDoesNotMatch);
    // And the smallest scalars, 0 and 1, are below L as well.
    for (const char low : {'\0', '\1'}) {
        std::string tiny(32, '\0');
        tiny[0] = low;
        const std::string both = r + tiny;
        update::SignatureBytes small{};
        std::copy_n(both.begin(), 64, small.begin());
        expectRefused(update::verifyEd25519(keyFromHex(vector.publicKey), bytesFromHex(vector.message), small),
                      kDoesNotMatch);
    }
}

// ---- the public key: a point of the prime-order subgroup ---------------------------------------

TEST(Signature, APublicKeyThatIsNotAPrimeOrderPointIsRefusedWhateverTheSignature)
{
    // Computed in the Python, not by this library: the eight points with 8P = the identity (as
    // L * Q for curve points Q), the aliases RFC 8032, 5.1.3 forbids, a point of mixed order, and
    // a y for which no x exists. Each is paired with TEST 2's genuine signature and message:
    // refused for the key, since the equation has nothing to say about it.
    struct Key {
        const char* what;
        const char* hex;
    };
    const Key keys[] = {
        // the torsion subgroup
        {"order 4, y = 0", "0000000000000000000000000000000000000000000000000000000000000000"},
        {"order 4, y = 0, other x", "0000000000000000000000000000000000000000000000000000000000000080"},
        {"order 1, the neutral element", "0100000000000000000000000000000000000000000000000000000000000000"},
        {"order 8", "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05"},
        {"order 8, other x", "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc85"},
        {"order 8, other y", "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a"},
        {"order 8, other y and x", "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac03fa"},
        {"order 2, y = p - 1", "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f"},
        // not canonical
        {"y = p", "edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f"},
        {"y = p, sign bit set", "edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"},
        {"y = p + 1", "eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f"},
        {"x = 0 with the sign bit set (identity)", "0100000000000000000000000000000000000000000000000000000000000080"},
        {"x = 0 with the sign bit set (y = p - 1)", "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"},
        {"all ones", "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"},
        // a point of the curve of order 8 * L: TEST 1's key plus an order-8 point
        {"mixed order", "9158312a9a8d6e3b34c891d6d61444f8b8211c5117ebad15bdb0bd68b07e0245"},
        // no point
        {"y = 2, no x", "0200000000000000000000000000000000000000000000000000000000000000"},
        {"y = 7, no x", "0700000000000000000000000000000000000000000000000000000000000000"},
        // the placeholders of cmake/KatanaUpdateKeys.cmake: "PLACEHOLDER-ACTIVE-NOT-A-KEY-00-" and
        // "PLACEHOLDER-SPARE-NOT-A-KEY--00-" in ASCII (the Python: neither decodes)
        {"the active placeholder", "504c414345484f4c4445522d4143544956452d4e4f542d412d4b45592d30302d"},
        {"the spare placeholder", "504c414345484f4c4445522d53504152452d4e4f542d412d4b45592d2d30302d"},
    };
    const RfcVector& vector = kRfcVectors[1];
    for (const Key& key : keys) {
        expectRefused(update::verifyEd25519(keyFromHex(key.hex), bytesFromHex(vector.message), signatureFromHex(vector.signature)),
                      kKeyNotAccepted);
        // ... and with a signature of zeros, the shape of a forgery.
        expectRefused(update::verifyEd25519(keyFromHex(key.hex), bytesFromHex(vector.message), update::SignatureBytes{}),
                      kKeyNotAccepted);
    }
}

TEST(Signature, TheAllZeroKeyAndSignatureThatOpensslAcceptsAreRefused)
{
    // Measured with `openssl pkeyutl -verify -pubin -rawin` (3.6.5): the all-zero public key
    // (a point of order 4) and the all-zero signature verify one-byte messages 9, 14, 17, 19, 26,
    // 27, 39, 47, 49, 51 ... 239 (65 of 256), because the equation 8*S*B = 8*R + 8*h*A has only
    // multiples of the order-4 point in it. This is a forgery for a build that pins the
    // zero key; the key check is what stops it. All 256 messages, so that it does not depend on
    // which of them the equation happens to hold for.
    const update::PublicKey zeroKey{};
    const update::SignatureBytes zeroSignature{};
    for (int byte = 0; byte < 256; ++byte) {
        expectRefused(update::verifyEd25519(zeroKey, std::string(1, static_cast<char>(byte)), zeroSignature),
                      kKeyNotAccepted);
    }
    // The same signature under a real key is a plain mismatch, so the refusal above is about the key.
    expectRefused(update::verifyEd25519(keyFromHex(kTest2Public), std::string(1, '\x09'), zeroSignature), kDoesNotMatch);
}

TEST(Signature, TheRfcKeysAreAcceptedAsPrimeOrderPoints)
{
    // A key the check refuses would make every pinned key useless: a good signature by a
    // genuine key verified above; here the same key with a wrong signature is a MISMATCH, not a
    // key refusal.
    for (const RfcVector& vector : kRfcVectors) {
        update::SignatureBytes wrong = signatureFromHex(vector.signature);
        wrong[0] = static_cast<std::uint8_t>(wrong[0] ^ 1u);
        const core::Status status = update::verifyEd25519(keyFromHex(vector.publicKey), bytesFromHex(vector.message), wrong);
        ASSERT_FALSE(status.ok());
        EXPECT_FALSE(contains(status.error().message, kKeyNotAccepted)) << vector.name << ": " << status.error().describe();
    }
}

// ---- manifests signed by the openssl command line ----------------------------------------------

TEST(Signature, FixtureManifestsSignedByOpensslVerifyUnderExactlyTheKeyThatSignedThem)
{
    const update::PublicKey active = keyFromHex(kTest1Public);
    const update::PublicKey spare = keyFromHex(kTest2Public);
    const update::PublicKey other = keyFromHex(kTest3Public);
    struct Case {
        const char* folder;
        const char* manifest;
        bool byActive, bySpare, byOther;
    };
    const Case cases[] = {
        {"v1", "Katana-update-win64.json", true, false, false},
        {"v2", "Katana-update-win64.json", true, false, false},
        {"v2-spare-signed", "Katana-update-win64.json", false, true, false},
        {"v2-macos-notify", "Katana-update-macos-arm64.json", true, false, false},
        // wrong in the signature
        {"bad/flipped-manifest-bit", "Katana-update-win64.json", false, false, false},
        {"bad/swapped-sig", "Katana-update-win64.json", false, false, false},
        {"bad/wrong-key", "Katana-update-win64.json", false, false, true},
        {"bad/wrong-key-claims-active", "Katana-update-win64.json", false, false, true},
        // validly signed, wrong elsewhere (paths, the zip, the order): the signature layer passes
        // them and the layers that own those rules refuse them
        {"bad/tampered-member", "Katana-update-win64.json", true, false, false},
        {"bad/path-traversal", "Katana-update-win64.json", true, false, false},
        {"bad/downgrade-sequence", "Katana-update-win64.json", true, false, false},
        {"bad/downgrade-version", "Katana-update-win64.json", true, false, false},
    };
    for (const Case& item : cases) {
        const std::string folder = std::string("release/") + item.folder + "/";
        const std::string manifest = readFixture(folder + item.manifest);
        const auto line = update::parseSignatureLine(readFixture(folder + item.manifest + ".sig"));
        ASSERT_TRUE(line.ok()) << item.folder;
        const auto& signature = line.value().signature;
        EXPECT_EQ(update::verifyEd25519(active, manifest, signature).ok(), item.byActive) << item.folder << " under the active key";
        EXPECT_EQ(update::verifyEd25519(spare, manifest, signature).ok(), item.bySpare) << item.folder << " under the spare key";
        EXPECT_EQ(update::verifyEd25519(other, manifest, signature).ok(), item.byOther) << item.folder << " under the key nobody trusts";
    }
}

TEST(Signature, AFlippedBitOrAChangedLengthAnywhereInASignedManifestIsRefused)
{
    // The exact bytes are signed: no whitespace, line end or length is "equivalent".
    const std::string manifest = readFixture("release/v2/Katana-update-win64.json");
    const auto line = update::parseSignatureLine(readFixture("release/v2/Katana-update-win64.json.sig"));
    ASSERT_TRUE(line.ok());
    const update::PublicKey key = keyFromHex(kTest1Public);
    const update::SignatureBytes& signature = line.value().signature;
    ASSERT_TRUE(update::verifyEd25519(key, manifest, signature).ok()) << "the control";
    ASSERT_GT(manifest.size(), 1500u);

    std::size_t flips = 0;
    for (std::size_t offset = 0; offset < manifest.size(); offset += 37) {
        std::string altered = manifest;
        altered[offset] = static_cast<char>(static_cast<unsigned char>(altered[offset]) ^ (1u << (offset % 8)));
        expectRefused(update::verifyEd25519(key, altered, signature), kDoesNotMatch);
        ++flips;
    }
    EXPECT_GT(flips, 40u); // the loop reached many offsets, the first and (at 37 * k) others
    std::string lastByte = manifest;
    lastByte.back() = static_cast<char>(lastByte.back() ^ 1);
    expectRefused(update::verifyEd25519(key, lastByte, signature), kDoesNotMatch);

    expectRefused(update::verifyEd25519(key, manifest.substr(0, manifest.size() - 1), signature), kDoesNotMatch);
    expectRefused(update::verifyEd25519(key, manifest.substr(1), signature), kDoesNotMatch);
    expectRefused(update::verifyEd25519(key, manifest + "\n", signature), kDoesNotMatch);
    expectRefused(update::verifyEd25519(key, manifest + " ", signature), kDoesNotMatch);
    expectRefused(update::verifyEd25519(key, "", signature), kDoesNotMatch);
    std::string crlf;
    for (const char c : manifest) {
        if (c == '\n') {
            crlf += '\r';
        }
        crlf += c;
    }
    expectRefused(update::verifyEd25519(key, crlf, signature), kDoesNotMatch);
}

// ---- thread safety and the environment ----------------------------------------------------------

TEST(Signature, VerificationIsSafeFromManyThreadsAtOnce)
{
    const update::PublicKey key = keyFromHex(kRfcVectors[1].publicKey);
    const update::SignatureBytes good = signatureFromHex(kRfcVectors[1].signature);
    update::SignatureBytes bad = good;
    bad[5] = static_cast<std::uint8_t>(bad[5] ^ 0x10);

    std::atomic<int> wrongAnswers{0};
    std::atomic<int> verifications{0};
    std::vector<std::thread> threads;
    for (int t = 0; t < 8; ++t) {
        threads.emplace_back([&] {
            for (int i = 0; i < 25; ++i) {
                if (!update::verifyEd25519(key, "\x72", good).ok()) {
                    ++wrongAnswers;
                }
                if (update::verifyEd25519(key, "\x72", bad).ok()) {
                    ++wrongAnswers;
                }
                if (update::verifyEd25519(key, "\x73", good).ok()) {
                    ++wrongAnswers;
                }
                verifications += 3;
            }
        });
    }
    for (std::thread& thread : threads) {
        thread.join();
    }
    EXPECT_EQ(verifications.load(), 8 * 25 * 3);
    EXPECT_EQ(wrongAnswers.load(), 0);
}

namespace {

constexpr const char* kOpenSslVariables[] = {"OPENSSL_CONF", "OPENSSL_MODULES", "OPENSSL_ENGINES"};

void setVariable(const char* name, const char* value)
{
#ifdef _WIN32
    _putenv_s(name, value);
#else
    setenv(name, value, 1);
#endif
}

void removeVariable(const char* name)
{
#ifdef _WIN32
    _putenv_s(name, "");
#else
    unsetenv(name);
#endif
}

// Puts the three variables back as they were: other tests in this process read the environment.
class RestoreOpenSslVariables {
  public:
    RestoreOpenSslVariables()
    {
        for (std::size_t i = 0; i < std::size(kOpenSslVariables); ++i) {
            if (const char* value = std::getenv(kOpenSslVariables[i])) {
                saved_[i] = value;
            }
        }
    }
    RestoreOpenSslVariables(const RestoreOpenSslVariables&) = delete;
    RestoreOpenSslVariables& operator=(const RestoreOpenSslVariables&) = delete;
    ~RestoreOpenSslVariables()
    {
        for (std::size_t i = 0; i < std::size(kOpenSslVariables); ++i) {
            if (saved_[i]) {
                setVariable(kOpenSslVariables[i], saved_[i]->c_str());
            } else {
                removeVariable(kOpenSslVariables[i]);
            }
        }
    }

  private:
    std::optional<std::string> saved_[std::size(kOpenSslVariables)];
};

} // namespace

TEST(Signature, HardeningRemovesTheThreeOpenSslVariablesAndVerificationStillWorks)
{
    const RestoreOpenSslVariables restore;
    for (const char* name : kOpenSslVariables) {
        setVariable(name, "somewhere-an-attacker-chose");
        ASSERT_NE(std::getenv(name), nullptr) << name << ": the test could not set it";
    }
    setVariable("OPENSSL_CONF_INCLUDE", "left alone"); // another variable: not this function's
    setVariable("KATANA_T5_UNRELATED", "left alone");

    const core::Status hardened = update::hardenOpenSslEnvironment();
    ASSERT_TRUE(hardened.ok()) << hardened.error().describe();
    for (const char* name : kOpenSslVariables) {
        EXPECT_EQ(std::getenv(name), nullptr) << name;
    }
    ASSERT_NE(std::getenv("KATANA_T5_UNRELATED"), nullptr);
    EXPECT_STREQ(std::getenv("KATANA_T5_UNRELATED"), "left alone");
    ASSERT_NE(std::getenv("OPENSSL_CONF_INCLUDE"), nullptr);
    removeVariable("OPENSSL_CONF_INCLUDE");
    removeVariable("KATANA_T5_UNRELATED");

    // Idempotent, and the library still does its job afterwards.
    EXPECT_TRUE(update::hardenOpenSslEnvironment().ok());
    EXPECT_TRUE(update::verifyEd25519(keyFromHex(kRfcVectors[1].publicKey), bytesFromHex(kRfcVectors[1].message),
                                      signatureFromHex(kRfcVectors[1].signature))
                    .ok());
}

TEST(Signature, HardeningWithNothingSetChangesNothing)
{
    const RestoreOpenSslVariables restore;
    for (const char* name : kOpenSslVariables) {
        removeVariable(name);
    }
    EXPECT_TRUE(update::hardenOpenSslEnvironment().ok());
    for (const char* name : kOpenSslVariables) {
        EXPECT_EQ(std::getenv(name), nullptr) << name;
    }
}

#ifdef _WIN32
TEST(Signature, HardeningRemovesTheVariablesWhateverTheirCaseOnWindows)
{
    // Windows matches environment names without regard to case, so a variable spelled
    // "openssl_modules" is the one OpenSSL reads. (tools/update_make_manifest.py tests the
    // same for the openssl it runs.)
    const RestoreOpenSslVariables restore;
    setVariable("openssl_conf", "x");
    setVariable("OpenSSL_Modules", "y");
    setVariable("OPENSSL_engines", "z");
    ASSERT_NE(std::getenv("OPENSSL_CONF"), nullptr) << "the premise: Windows folds the case";

    ASSERT_TRUE(update::hardenOpenSslEnvironment().ok());
    for (const char* name : {"OPENSSL_CONF", "openssl_conf", "OPENSSL_MODULES", "openssl_modules",
                             "OPENSSL_ENGINES", "openssl_engines"}) {
        EXPECT_EQ(std::getenv(name), nullptr) << name;
    }
}
#endif
