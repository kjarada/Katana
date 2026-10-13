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
//
// To regenerate the Python-derived literals (about 100 lines of Python from RFC 8032 sections
// 5.1 and 6 and nothing else; check it against TEST 1 to 3 first): an order-8 point is L * Q for
// a curve point Q found by decoding small y values, and its other-sign and other-y forms are the
// same points with x or y negated; a non-canonical alias is y + p written in 255 bits
// (p = 2^255 - 19); the mixed-order key is TEST 1's A plus one order-8 point, encoded; "S + L" is
// TEST 2's S plus L, little-endian, in the signature's second half (it still satisfies
// 8*S*B = 8*R + 8*h*A, which is why only the explicit S < L test refuses it). The count for the
// all-zero key is `openssl pkeyutl -verify -pubin -rawin` over the 256 one-byte messages, with a
// 64-byte zero sigfile and the SPKI header 302a300506032b6570032100 before 32 zero bytes.

#include <gtest/gtest.h>

#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <optional>
#include <string>
#include <string_view>
#include <system_error>
#include <thread>
#include <vector>

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#else
#include <sys/wait.h>
#endif

#include "katana/core/path_text.hpp"
#include "katana/update/feed.hpp"
#include "katana/update/process.hpp"
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

// Through the project's reader, so that a checkout under a folder named in any script opens.
std::string readFixture(const std::string& relative)
{
    const core::Result<std::string> bytes =
        core::readFileBytes(core::pathFromUtf8(std::string(KATANA_UPDATE_FIXTURES) + "/" + relative));
    EXPECT_TRUE(bytes.ok()) << relative << ": " << (bytes.ok() ? "" : bytes.error().describe());
    return bytes.ok() ? bytes.value() : std::string();
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

TEST(Signature, AMessageWithNoDataPointerIsTheEmptyMessage)
{
    // TEST 1 signs the empty message, and bytesFromHex("") hands over an empty std::string, whose
    // data() is a real pointer. A default string_view's is null, which is what a caller with
    // nothing to say may pass; verifyEd25519 must still verify it as the empty message and not
    // hand a null pointer to OpenSSL. (Measured: OpenSSL 3.6.5 itself accepts a null pointer with a
    // length of 0, so on this OpenSSL the substitution in verifyEd25519 is a safeguard and this case
    // pins the behaviour; on a library that refuses the null it is what keeps TEST 1 verifying.)
    const std::string_view nothing;
    ASSERT_EQ(nothing.data(), nullptr) << "the premise: this view has no data pointer";
    const RfcVector& one = kRfcVectors[0];
    const core::Status status =
        update::verifyEd25519(keyFromHex(one.publicKey), nothing, signatureFromHex(one.signature));
    EXPECT_TRUE(status.ok()) << (status.ok() ? "" : status.error().describe());
    // And it is the empty message and nothing else: another key's signature of another message
    // does not verify for it, and TEST 1's signature does not verify for a message of one byte.
    expectRefused(update::verifyEd25519(keyFromHex(kRfcVectors[1].publicKey), nothing,
                                        signatureFromHex(kRfcVectors[1].signature)),
                  kDoesNotMatch);
    expectRefused(update::verifyEd25519(keyFromHex(one.publicKey), std::string_view("\0", 1),
                                        signatureFromHex(one.signature)),
                  kDoesNotMatch);
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
    // through to OpenSSL, which finds it does not belong to this message. A comparison that is
    // off by one the other way (S + 1 against L, or "equal" declared one byte too early) would
    // refuse it for its size instead.
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
        // The key check on its own (what the trust gate asks of a pinned key) says the same.
        expectRefused(update::checkPublicKey(keyFromHex(key.hex)), kKeyNotAccepted);
    }
}

TEST(Signature, TheAllZeroKeyAndSignatureThatOpensslAcceptsAreRefused)
{
    // Measured with `openssl pkeyutl -verify -pubin -rawin` (3.6.5): the all-zero public key
    // (a point of order 4) and the all-zero signature verify one-byte messages 9, 14, 17, 19, 26,
    // 27, 39, 47, 49, 51 ... 239 (64 of 256), because the equation 8*S*B = 8*R + 8*h*A has only
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
        const core::Status checked = update::checkPublicKey(keyFromHex(vector.publicKey));
        EXPECT_TRUE(checked.ok()) << vector.name << ": " << (checked.ok() ? "" : checked.error().describe());
    }
}

TEST(Signature, TwentyFourKeysGeneratedAndSignedByOpensslAreAcceptedAndVerify)
{
    // The check that a key is a prime-order point is arithmetic of this file's own, and a false
    // refusal of the owner's real key would leave every installed copy unable to update, with
    // nothing in the build to say so. Three RFC keys are too few samples for that: these are 24
    // keys made by `openssl genpkey -algorithm ed25519` with a signature each by
    // `openssl pkeyutl -sign -rawin` over the message printed. The keys are throw-away (their
    // secrets were not kept) and each signature was checked back with `pkeyutl -verify`. Such
    // keys are what the pinned ones will be.
    struct Generated {
        const char* publicKey;
        const char* message;
        const char* signature;
    };
    static constexpr Generated kGenerated[] = {
    {"8f4b3d1e44e9e2ad69f61922143dd1d601c2b6a482576bb8a3b2aab6c23c36d0",
     "katana pinned-key acceptance 00",
     "71f67902866966899e9bd2e5146d0c71cbfb8fd7f47062da3f11640becd460a29143e60bbc4aa828cdf6cff6f90792fb9fb1e4edf83ecf91b1df18a6ac7c2901"},
    {"90f81fa3df0c6ee3240d22bc1d98bf02e57de454037185969b6770b731cf8d8d",
     "katana pinned-key acceptance 01",
     "bc10c225b36b54f7da3c31d8ed2b9abcdc9d9adddb54848dada9319c26df4cc1e80b9adb0d5027e0ec403fc5efa85ac10336a5c1567c64c211ca58fbcc133305"},
    {"327505024baf2b72195b33ca5279b3a8ee4a14230d3e8173d9f548b732908102",
     "katana pinned-key acceptance 02",
     "2c004fa0f34a4a87c1326762e546b1d42566b1ca5d0a4f127c76ecfeadbaf6a7d1588e0278bc6d829f2abf782eb11493609a9de0f874ca0789bac2abab026508"},
    {"4a76c73dc2e9723de4513e4e4941bbfcdcfa44a8b217a101a98af8a244b40b04",
     "katana pinned-key acceptance 03",
     "67eb5d05f383c72d01c36d1654085a8933753686efaa87a6b85d704e8ac047cc9bd7c517009ceb9a22e3f0cb35f255b9427e6b12cc858a49e37a65b1d1b00e0e"},
    {"6848403760627b5ee74dd4a814be716041e39b88f9d8f40bf877fa6ee42bd531",
     "katana pinned-key acceptance 04",
     "adb9715e4968a2bb0d7037c525d788b9c1e650bc2e939d3162eef1ca405ee41e666caf9975492f6dc57d0e5f523b760ce89eb2316b36e9b41f55d59cc9afe504"},
    {"1dde638a3b8de773720464e9fe89422143f547d92a0385233e8d7ad01a8bf794",
     "katana pinned-key acceptance 05",
     "8e1b5d8829eab19899ed548f01d5d858f629bcdd689615ab9a006aeed462b1784e5165d45006a2a9e4a2d0503a575a16e7a79faaae964176ed2166c592ee110f"},
    {"d167bf3b67fd2b35a966e9d92faca7e99b6a93374aa9e78714729b959460ebf1",
     "katana pinned-key acceptance 06",
     "470349a0c91d2991606a3c1b46d4607214beee50b4b00a9aacedea31b190a6e0c429b7052f3c4eb42834521e949841f043cebce5e54b0e5cec3fec0909db8b05"},
    {"db80f56b5f28b86b1ed8f603905439084f6091176c54581b78f061f74cf62ff1",
     "katana pinned-key acceptance 07",
     "da7ebe7845a191807386a0913c0538d55539b332c97076c137c71fef3045815a45682c657b52c00dd5e8b9afef536c2b7ab8e2dc10f40218103b382e8def3f0f"},
    {"f9c5a68f13722d22e401d1a35468bdf46ed29c3bd2d3009e4977ef210d8b3845",
     "katana pinned-key acceptance 08",
     "84256eb77ed1270ac38f853f67763b87719486ade4ad30e58cfc239c049e1f4d84ed423df6f63e0a0ed4ac6f9224bd5006a75ebf2c6e69ed2ca2b9d4dfb41c02"},
    {"c873020c2511c531458ed9f70b9cd951ec5eb6e132b8398fedfd89d05a5f5696",
     "katana pinned-key acceptance 09",
     "53113e3617c3e655493591c2a4fbe125e219bb571216e2aef15f5f0a39c224c02ac766a9d2766e558b1fffc6f3e2ebacf310e0723fa6e0f308494412ee796705"},
    {"bc5ab8b95569f114b4518d24803deafdcab4eb4192cae0f7248dc59c4371284d",
     "katana pinned-key acceptance 10",
     "c4fb49b3ee9d7acb18e5a0294e0e772dfde95e84d923ddb72c4aad501a7f94476270ff5549ad61fc98895932d24fd6c554fd6e0213c1bc0d2248236b2b311407"},
    {"a6666db5dde6e1cfb23e0ac88b7846af6b6fe2b756afaa4eed4489107e518eed",
     "katana pinned-key acceptance 11",
     "e1bfc8fcd5bcd9a02020ac7677bfa2b683e85ca8b83503c21edecbe70b99cbd8eb52c8d882cdfacc6a486d9428b213d3e148ccc0c116774d47351ea128233c00"},
    {"180cb369001c685ddeb100d9d2a91e06e5b84cf82e62480c943169becd5f06cd",
     "katana pinned-key acceptance 12",
     "00e42c762ac8521700d74702fea4d0e0dd4d15eae12a899f6e0b21d91afe7cd3cdc8607c0394b4a1aea5c7aeff93bd1fc0c82c9456970842c0eea9b2f4934704"},
    {"9f098d312c327f70a7fab43cb06566521770634490cfd2ddcbbd2579f7b8f25f",
     "katana pinned-key acceptance 13",
     "56e73ce09321869d232752df1665cd796f81db99aa8802d8d50d6d49db2209d4c60d987ad88b89b12e990b4ac370118e871f5e6bf39a27262b5dd6c338097800"},
    {"515f1ab9696e39c0382ecc45fc2115b2a2120440b6bda4e82126dd6021793307",
     "katana pinned-key acceptance 14",
     "89fd9650cfa9f56b5c601b37bdf6758b988c319ad85afc89a550b348c17045d0e2d29978046ff0199908d11fe72f9c8acc2c8493c1aaa45a23c6c8369a4c830e"},
    {"013b7e24fa650062ad46a338c498d2e6d68e4af4349df1508277bdbc3faa7875",
     "katana pinned-key acceptance 15",
     "6ade83b3e43ef779fcdbedf3a920f9b5a43c647e7f49f26dc05ecb89322d85f93d283e8eaffc9b8159abc87b51ff6d6eccaf31b2d295af845fc9a6d6dc4fe60a"},
    {"e67f6d8f6963d780d71a83e62e931f3afaa3a7e806f15d3fad8a5a0aa418eec3",
     "katana pinned-key acceptance 16",
     "f6a3ab1b06009338b05486a547a9f1a6f30d17a183fdea8de8c3cb53b60a1f3386c1f433082f882f92e24e01ed971f7669d8c970626d34a33535f48c046f810e"},
    {"d0bd9a1528b97f1afae87e081d898db8a26b6f9615176a004d3734d77da4b862",
     "katana pinned-key acceptance 17",
     "91278ba50a82e7af34310c22f09d821a9676293ed4dc7277db862b8127a7dd253a75195b138b45cf5c8eb0065472744619443863bda5a2a5814e80dacb5d7001"},
    {"cfbff8e9016695eb3a486c05a31267df8f149a84d735fc9ba68a130f7791fff9",
     "katana pinned-key acceptance 18",
     "d1c86294658844bd0a7a2dd1bdbab07f69ed4bee37d370548a1f71eaff9ab654692aa6d4eee57919dfb37fa6df98f815f7bc98f3492089116319c1c3241f1d00"},
    {"85f717e404472727b03a826d6e877ad49a8240a351c6abf6c6d961a997252c33",
     "katana pinned-key acceptance 19",
     "d605b3ca4c2e08164b74ddd62e3d1cfd10fef287f6d66425f0ea2dde3b6baaee17459e1a8419abd58e754b5adc947c21d68ea03134ea6b997df186a65ce9c00c"},
    {"08615244369c8e9ea481bc746941b2e22e7ff9f530ed55ac6299571000409559",
     "katana pinned-key acceptance 20",
     "bf907c35577d5d94197933709d3bde53dc30cb8323d251b6af74d46a90077c3791b1c8565124abc0745b9b0c7124f2884394680fdc8337b09681def93bac710b"},
    {"e4fae266737fde6b9300329bb2c3b3727a60e4298414d829aef96dbeba088ea3",
     "katana pinned-key acceptance 21",
     "027f4b16abbc02945cc6edc72d2595e866edcbc53221aad48a850eb7ddac50dcea442599b02f9d8ca0981c04522f39f3ca1dd8694c2d509f7122626589d5eb0b"},
    {"44f87831819905fd429073ec69cccb029084aeda11c18fd0e596906bd5b62add",
     "katana pinned-key acceptance 22",
     "ef242155b147f2d85027358e7a083da6834e51694d47a3da3bfadf604fd673f49572a5d8754a266d7549594cdd77d41992f0f6962367b4c985a6bc92f9d5440f"},
    {"81eb584872bc2e1bc9a490ceebd233204aaa86924402ee14b5fa3b6bbba9e346",
     "katana pinned-key acceptance 23",
     "f26a8919ae496c2ed956db0509901ba7e069727950abd57b94697aa9e7d8dc5969878fa8e4b2e7025c5881e5a12e0355872a335900ef81523b96463c727f5202"},
    };
    for (const Generated& entry : kGenerated) {
        const update::PublicKey key = keyFromHex(entry.publicKey);
        const update::SignatureBytes signature = signatureFromHex(entry.signature);
        const core::Status good = update::verifyEd25519(key, entry.message, signature);
        EXPECT_TRUE(good.ok()) << entry.publicKey << ": " << (good.ok() ? "" : good.error().describe());
        EXPECT_TRUE(update::checkPublicKey(key).ok()) << entry.publicKey;

        // One bit of the message: a MISMATCH for this key, so the key was accepted as a key.
        std::string altered = entry.message;
        altered[0] = static_cast<char>(altered[0] ^ 1);
        const core::Status bad = update::verifyEd25519(key, altered, signature);
        ASSERT_FALSE(bad.ok()) << entry.publicKey;
        EXPECT_TRUE(contains(bad.error().message, kDoesNotMatch)) << bad.error().describe();
        EXPECT_FALSE(contains(bad.error().message, kKeyNotAccepted)) << bad.error().describe();
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
// The one that marks the program started again by the configuration test at the end of this file.
constexpr const char* kHostileChildMarker = "KATANA_T5_HOSTILE_CHILD";
// The same for the program started by the inheritance test.
constexpr const char* kInheritanceChildMarker = "KATANA_T5_INHERITANCE_CHILD";
// Every variable a test of this file sets, for the guard to put back whatever way the test ends
// (an ASSERT returns early).
constexpr const char* kVariablesTheTestsSet[] = {"OPENSSL_CONF",        "OPENSSL_MODULES",
                                                 "OPENSSL_ENGINES",     "OPENSSL_CONF_INCLUDE",
                                                 "KATANA_T5_UNRELATED", kHostileChildMarker,
                                                 kInheritanceChildMarker};

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

// Puts the variables back as they were: other tests in this process read the environment.
class RestoreOpenSslVariables {
  public:
    RestoreOpenSslVariables()
    {
        for (std::size_t i = 0; i < std::size(kVariablesTheTestsSet); ++i) {
            if (const char* value = std::getenv(kVariablesTheTestsSet[i])) {
                saved_[i] = value;
            }
        }
    }
    RestoreOpenSslVariables(const RestoreOpenSslVariables&) = delete;
    RestoreOpenSslVariables& operator=(const RestoreOpenSslVariables&) = delete;
    ~RestoreOpenSslVariables()
    {
        for (std::size_t i = 0; i < std::size(kVariablesTheTestsSet); ++i) {
            if (saved_[i]) {
                setVariable(kVariablesTheTestsSet[i], saved_[i]->c_str());
            } else {
                removeVariable(kVariablesTheTestsSet[i]);
            }
        }
    }

  private:
    std::optional<std::string> saved_[std::size(kVariablesTheTestsSet)];
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

// ---- OPENSSL_INIT_NO_LOAD_CONFIG: a configuration file must not decide what verifies ---------------
//
// OpenSSL reads the file OPENSSL_CONF names when it first initialises, once per process, and the
// file decides which providers load. The one written below loads the base provider alone, which has
// no Ed25519: a library that read it would find no algorithm to verify with, and a file that loads
// a provider of the attacker's choosing would decide the verdict itself. verifyEd25519 initialises
// libcrypto with OPENSSL_INIT_NO_LOAD_CONFIG, and this is the case that fails if that flag goes.
//
// It cannot be proved in this process: by the time any test runs, an earlier one may have
// initialised libcrypto already, with or without the variable. So the case starts this program
// again on this one case, with the variable set and nothing having called libcrypto. The child
// does not call hardenOpenSslEnvironment either, which would clear the variable and hide the flag:
// that function is tested above, and its clearing is a second line of defence, not this one.
// (Measured with the flag set to 0: the child fails, verifyEd25519 answering Internal because
// EVP_PKEY_new_raw_public_key finds no Ed25519.)

namespace {

std::filesystem::path thisProgram()
{
    std::filesystem::path program;
    std::error_code ignored;
#ifdef _WIN32
    const std::vector<std::string> arguments = update::utf8Arguments(0, nullptr);
    if (!arguments.empty()) {
        program = core::pathFromUtf8(arguments.front());
    }
    if (!program.empty() && !std::filesystem::exists(program, ignored) && program.extension().empty()) {
        program += ".exe"; // started as `katana_update_tests` from a command prompt
    }
#else
    if (std::filesystem::exists("/proc/self/exe", ignored)) {
        program = std::filesystem::read_symlink("/proc/self/exe", ignored);
    } else if (!::testing::internal::GetArgvs().empty()) {
        program = ::testing::internal::GetArgvs().front();
    }
#endif
    return program.empty() ? program : std::filesystem::absolute(program, ignored);
}

struct ScratchFolder {
    std::filesystem::path path;
    ScratchFolder()
    {
        std::error_code ignored;
        const auto stamp = std::chrono::steady_clock::now().time_since_epoch().count();
        path = std::filesystem::temp_directory_path(ignored) / ("katana_t5_" + std::to_string(stamp));
        std::filesystem::create_directories(path, ignored);
    }
    ScratchFolder(const ScratchFolder&) = delete;
    ScratchFolder& operator=(const ScratchFolder&) = delete;
    ~ScratchFolder()
    {
        std::error_code ignored;
        std::filesystem::remove_all(path, ignored);
    }
};

// Runs `program --gtest_filter=<filter>` with `folder` as its working directory, both of its
// streams into `output`; its exit code, or -1 when it could not be started or did not end within
// two minutes. The folder is how the child names its configuration file by a relative, ASCII-only
// path whatever the user's profile folder is called: OPENSSL_CONF reaches OpenSSL in the ANSI code
// page, so an absolute path under a profile named outside ASCII could not be named to it.
//
// No shell on Windows: the program is started directly, so a checkout or temporary folder whose
// name holds & % ^ or ( ) is not read by cmd.exe as anything but a name. (A Windows path cannot
// hold a double quote, so the one pair around the program's name is the whole of the quoting.)
// The child inherits this process's environment as it is now, which is what the cases rely on.
#ifdef _WIN32
int runProgramOnOneCase(const std::filesystem::path& program, const std::string& filter,
                        const std::filesystem::path& folder, const std::filesystem::path& output)
{
    SECURITY_ATTRIBUTES inheritable{};
    inheritable.nLength = sizeof(inheritable);
    inheritable.bInheritHandle = TRUE;
    const HANDLE file = CreateFileW(output.wstring().c_str(), GENERIC_WRITE, FILE_SHARE_READ, &inheritable,
                                    CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (file == INVALID_HANDLE_VALUE) {
        return -1;
    }
    STARTUPINFOW startup{};
    startup.cb = sizeof(startup);
    startup.dwFlags = STARTF_USESTDHANDLES; // no standard input: the child is given none
    startup.hStdOutput = file;
    startup.hStdError = file;
    std::wstring command = L"\"" + program.wstring() + L"\" --gtest_color=no --gtest_filter=" +
                           std::wstring(filter.begin(), filter.end());
    const std::wstring workingFolder = folder.wstring();
    PROCESS_INFORMATION process{};
    const BOOL started = CreateProcessW(nullptr, command.data(), nullptr, nullptr, TRUE, 0, nullptr,
                                        workingFolder.c_str(), &startup, &process);
    CloseHandle(file); // the child holds its own copy
    if (!started) {
        return -1;
    }
    int exitCode = -1;
    if (WaitForSingleObject(process.hProcess, 120000) == WAIT_OBJECT_0) {
        DWORD code = 0;
        if (GetExitCodeProcess(process.hProcess, &code)) {
            exitCode = static_cast<int>(code);
        }
    } else {
        TerminateProcess(process.hProcess, 1);
    }
    CloseHandle(process.hThread);
    CloseHandle(process.hProcess);
    return exitCode;
}
#else
// 'text' for a POSIX shell: a quote inside it is closed, escaped and opened again.
std::string shellQuoted(const std::string& text)
{
    std::string quoted = "'";
    for (const char c : text) {
        quoted += c == '\'' ? std::string("'\''") : std::string(1, c);
    }
    return quoted + "'";
}

int runProgramOnOneCase(const std::filesystem::path& program, const std::string& filter,
                        const std::filesystem::path& folder, const std::filesystem::path& output)
{
    const std::string command = "cd " + shellQuoted(folder.string()) + " && " + shellQuoted(program.string()) +
                                " --gtest_color=no --gtest_filter=" + filter + " > " +
                                shellQuoted(output.string()) + " 2>&1";
    const int status = std::system(command.c_str());
    return status != -1 && WIFEXITED(status) ? WEXITSTATUS(status) : -1;
}
#endif

} // namespace

TEST(Signature, AConfigurationFileNamedByTheEnvironmentDoesNotChangeWhatVerifies)
{
    if (std::getenv(kHostileChildMarker) != nullptr) {
        // The child: its first use of libcrypto. RFC 8032 TEST 1 to 3 must verify.
        for (const RfcVector& vector : kRfcVectors) {
            const core::Status status = update::verifyEd25519(
                keyFromHex(vector.publicKey), bytesFromHex(vector.message), signatureFromHex(vector.signature));
            EXPECT_TRUE(status.ok()) << vector.name << ": " << (status.ok() ? "" : status.error().describe());
        }
        return;
    }

    const std::filesystem::path program = thisProgram();
    ASSERT_TRUE(!program.empty() && std::filesystem::exists(program))
        << "cannot find this program: '" << program.string() << "'";
    const ScratchFolder scratch;
    ASSERT_TRUE(std::filesystem::is_directory(scratch.path));
    const std::filesystem::path configuration = scratch.path / "base_provider_only.cnf";
    const std::filesystem::path output = scratch.path / "child_output.txt";
    {
        std::ofstream file(configuration, std::ios::binary);
        file << "openssl_conf = katana_t5_init\n"
                "[katana_t5_init]\n"
                "providers = katana_t5_providers\n"
                "[katana_t5_providers]\n"
                "base = katana_t5_base\n"
                "[katana_t5_base]\n"
                "activate = 1\n";
        ASSERT_TRUE(file.good());
    }

    const RestoreOpenSslVariables restore;
    // Relative names, resolved against the child's working directory (the scratch folder).
    setVariable("OPENSSL_CONF", "base_provider_only.cnf");
    setVariable("OPENSSL_MODULES", "no_modules_here");
    setVariable("OPENSSL_ENGINES", "no_engines_here");
    setVariable(kHostileChildMarker, "1");

    const ::testing::TestInfo* const info = ::testing::UnitTest::GetInstance()->current_test_info();
    const std::string filter = std::string(info->test_suite_name()) + "." + info->name();
    const int exitCode = runProgramOnOneCase(program, filter, scratch.path, output);
    const core::Result<std::string> childOutput = core::readFileBytes(output);
    const std::string text =
        childOutput.ok() ? childOutput.value() : "(no output: " + childOutput.error().describe() + ")";

    EXPECT_EQ(exitCode, 0) << text;
    // One case ran in the child (a filter that matched nothing would also exit 0) and it passed.
    EXPECT_TRUE(contains(text, "[  PASSED  ] 1 test.")) << text;
    EXPECT_FALSE(contains(text, "SKIPPED")) << text;
}

// ---- what a program this one starts inherits ---------------------------------------------------------
//
// signature.hpp promises that after hardenOpenSslEnvironment() a program the process starts holds
// none of the three variables, which is what the helper's relaunch of katana relies on. Reading them
// back with getenv proves only this process's view of its environment; this case starts the program
// again after the call, and the child reports what IT was handed.

TEST(Signature, HardeningLeavesNoneOfTheThreeVariablesToAProgramThisOneStarts)
{
    if (std::getenv(kInheritanceChildMarker) != nullptr) {
        // The child. What it holds is what it inherited.
        for (const char* name : kOpenSslVariables) {
            EXPECT_EQ(std::getenv(name), nullptr) << name << " reached the child";
        }
#ifdef _WIN32
        for (const char* name : {"openssl_conf", "OpenSSL_Modules", "OPENSSL_engines"}) {
            EXPECT_EQ(std::getenv(name), nullptr) << name << " reached the child";
        }
#endif
        // The controls: the environment was inherited at all, and a variable of another name
        // survived the call.
        ASSERT_NE(std::getenv("KATANA_T5_UNRELATED"), nullptr) << "the child inherited nothing";
        EXPECT_STREQ(std::getenv("KATANA_T5_UNRELATED"), "inherited");
        return;
    }

    const std::filesystem::path program = thisProgram();
    ASSERT_TRUE(!program.empty() && std::filesystem::exists(program))
        << "cannot find this program: '" << program.string() << "'";
    const ScratchFolder scratch;
    ASSERT_TRUE(std::filesystem::is_directory(scratch.path));
    const std::filesystem::path output = scratch.path / "child_output.txt";

    const RestoreOpenSslVariables restore;
    for (const char* name : kOpenSslVariables) {
        setVariable(name, "somewhere-an-attacker-chose");
    }
#ifdef _WIN32
    // Spelled in another case, which is the same variable on Windows.
    setVariable("openssl_conf", "somewhere-an-attacker-chose");
#endif
    setVariable("KATANA_T5_UNRELATED", "inherited");
    setVariable(kInheritanceChildMarker, "1");
    ASSERT_TRUE(update::hardenOpenSslEnvironment().ok());

    const ::testing::TestInfo* const info = ::testing::UnitTest::GetInstance()->current_test_info();
    const std::string filter = std::string(info->test_suite_name()) + "." + info->name();
    const int exitCode = runProgramOnOneCase(program, filter, scratch.path, output);
    const core::Result<std::string> childOutput = core::readFileBytes(output);
    const std::string text =
        childOutput.ok() ? childOutput.value() : "(no output: " + childOutput.error().describe() + ")";

    EXPECT_EQ(exitCode, 0) << text;
    EXPECT_TRUE(contains(text, "[  PASSED  ] 1 test.")) << text;
    EXPECT_FALSE(contains(text, "SKIPPED")) << text;
}
