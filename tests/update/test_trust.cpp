// The pinned keys and the gate between downloaded bytes and a Manifest (include/katana/update/
// trust.hpp). The order of the gate is the header's: the signature line parses and names a
// trusted key; the trust is not a placeholder; the signature verifies over the exact bytes;
// only then is the text read as a manifest; and then product, target and channel are held to
// the install. Each case here says which of those it reaches.
//
// Expectations come from outside the code under test (CLAUDE.md, section 4): the T3 fixtures
// signed by the `openssl` command line, the RFC 8032 section 7.1 public keys, and one manifest
// written below and signed with RFC 8032 TEST 1's secret key by a pure-Python implementation of
// RFC 8032 (checked against the RFC's own vectors, and accepted by `openssl pkeyutl -verify
// -rawin`). No test key is in any program that ships: the Trust is a PARAMETER and these tests
// build theirs (tests/update/test_key.hpp).
//
// The cases that need the manifest PARSER (task T4) start with requireManifestParser(): until
// that task lands parseManifest is a stub that says "not implemented", and they are skipped,
// visibly, rather than failed or faked. The cases that stop before the parse - everything about
// the signature, the keys and the placeholders - run now.

#include <gtest/gtest.h>

#include <algorithm>
#include <cstdint>
#include <fstream>
#include <iterator>
#include <string>
#include <string_view>

#include "katana/update/feed.hpp"
#include "katana/update/manifest.hpp"
#include "katana/update/signature.hpp"
#include "katana/update/trust.hpp"
#include "test_key.hpp"

#ifndef KATANA_UPDATE_FIXTURES
#error "tests/update/CMakeLists.txt defines KATANA_UPDATE_FIXTURES"
#endif

using namespace katana;

namespace {

std::string readFixture(const std::string& relative)
{
    std::ifstream in(std::string(KATANA_UPDATE_FIXTURES) + "/" + relative, std::ios::binary);
    EXPECT_TRUE(in.good()) << relative;
    return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
}

struct Release {
    std::string json;
    std::string sig;
};

Release readRelease(const std::string& folder, const std::string& target = "win64")
{
    const std::string base = "release/" + folder + "/Katana-update-" + target + ".json";
    return Release{readFixture(base), readFixture(base + ".sig")};
}

update::Install installFor(const std::string& target)
{
    update::Install install;
    install.target = target;
    install.kind = update::InstallKind::Archive;
    return install; // product "katana" by default
}

update::PublicKey keyFromHex(std::string_view hex)
{
    update::PublicKey key{};
    for (std::size_t i = 0; i < key.size(); ++i) {
        const auto digit = [](char c) { return c <= '9' ? c - '0' : c - 'a' + 10; };
        key[i] = static_cast<std::uint8_t>(digit(hex[2 * i]) * 16 + digit(hex[2 * i + 1]));
    }
    return key;
}

// parseManifest is task T4's. While it is the skeleton's stub, it answers any text with
// Unsupported "not implemented: parseManifest"; the cases that need a real parse skip.
bool manifestParserIsImplemented()
{
    static const bool implemented = [] {
        const auto parsed = update::parseManifest("{}");
        return parsed.ok() || parsed.error().message.rfind("not implemented", 0) != 0;
    }();
    return implemented;
}

#define REQUIRE_THE_MANIFEST_PARSER()                                                                       \
    do {                                                                                                    \
        if (!manifestParserIsImplemented()) {                                                               \
            GTEST_SKIP() << "parseManifest is still the skeleton's stub; this case runs when task T4 lands"; \
        }                                                                                                   \
    } while (false)

bool contains(const std::string& text, std::string_view part)
{
    return text.find(part) != std::string::npos;
}

// The macOS notify-only manifest of the fixtures with its channel changed to "beta" (and
// compacted to one line), signed with TEST 1's secret key by the Python of the header comment.
// A beta manifest is validly signed: only the channel rule can refuse it.
constexpr std::string_view kBetaManifest =
    R"({"applier":1,"channel":"beta","expires":"2027-04-16T08:40:00Z","files":[],"notes_url":"https://github.com/kjarada/Katana/releases/tag/v0.10.0","notify_only":true,"product":"katana","published":"2026-10-18T08:40:00Z","schema":1,"sequence":1792312800,"tag":"v0.10.0","target":"macos-arm64","version":"0.10.0"})";
constexpr std::string_view kBetaSignatureLine =
    "ed25519 d75a980182b10ab7 H8tVyMWKZx9U2xHSecYJ/WDTV5rlUxfDb+gKHSJPwXIxsSYPOOmmYsM4I4JO1ZgLrqo7MHSj2NF48WrFff0dDA==\n";

// A text that is not a manifest, signed with TEST 1's secret key by the same Python (and
// accepted by `openssl pkeyutl -verify -rawin`): the signature is good, so only the parser can
// refuse it, and it is only asked after the signature.
constexpr std::string_view kSignedGarbage = "This text is signed, but it is not a manifest.\n";
constexpr std::string_view kSignedGarbageLine =
    "ed25519 d75a980182b10ab7 B5eWfJ4fKMnDEO0NgevDBR824jSWQi6h5cE3S36DSHkmFOJdo8TGUb/vbwaOpsgUHkch+viB0GrrQO2gbifgBw==\n";

// "PLACEHOLDER-..." as cmake/KatanaUpdateKeys.cmake spells them (the ASCII of the words).
constexpr std::string_view kPlaceholderActiveHex = "504c414345484f4c4445522d4143544956452d4e4f542d412d4b45592d30302d";
constexpr std::string_view kPlaceholderSpareHex = "504c414345484f4c4445522d53504152452d4e4f542d412d4b45592d2d30302d";

update::Trust trustOf(std::initializer_list<std::string_view> hexKeys, bool placeholder)
{
    update::Trust trust;
    for (const std::string_view hex : hexKeys) {
        const update::PublicKey key = keyFromHex(hex);
        trust.keys.push_back(update::TrustedKey{std::string(hex.substr(0, 16)), key});
    }
    trust.placeholder = placeholder;
    return trust;
}

} // namespace

// ---- the keys this build pins ------------------------------------------------------------------

TEST(Trust, TheBuildPinsTwoDistinctKeysAndNoneOfThemIsAPublishedTestKey)
{
    const update::Trust& trust = update::productionTrust();
    ASSERT_EQ(trust.keys.size(), 2u); // active, then spare
    EXPECT_NE(trust.keys[0].key, trust.keys[1].key);
    if (!trust.placeholder) {
        // Real keys differ in their first 8 bytes with probability 1 - 2^-64. (The two
        // placeholders both begin "PLACEHOLD", so they share an id; they verify nothing.)
        EXPECT_NE(trust.keys[0].id, trust.keys[1].id);
    }
    for (const update::TrustedKey& key : trust.keys) {
        EXPECT_EQ(key.id, update::keyIdOf(key.key));
        EXPECT_EQ(key.id.size(), 16u);
    }
    // The three public keys of RFC 8032 section 7.1: anyone holds their private halves, and
    // cmake/KatanaUpdateKeys.cmake refuses to pin them. A trust that held one would be one
    // that anybody could sign for.
    for (const std::string_view published : {"d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
                                             "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
                                             "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025"}) {
        EXPECT_NE(trust.keys[0].key, keyFromHex(published));
        EXPECT_NE(trust.keys[1].key, keyFromHex(published));
    }
    // The one object, whoever asks and whenever.
    EXPECT_EQ(&update::productionTrust(), &trust);
}

TEST(Trust, TheBuildSaysItsKeysAreThePlaceholdersExactlyWhenTheySpellThemAndThenVerifiesNothing)
{
    // The rule of cmake/KatanaUpdateKeys.cmake, which fails the configure when the flag and the
    // bytes disagree: "PLACEHOLDER" in ASCII is the first 11 bytes of a placeholder key.
    const update::Trust& trust = update::productionTrust();
    ASSERT_EQ(trust.keys.size(), 2u);
    const std::string_view word = "PLACEHOLDER";
    const bool spelled = std::equal(word.begin(), word.end(), trust.keys[0].key.begin()) ||
                         std::equal(word.begin(), word.end(), trust.keys[1].key.begin());
    EXPECT_EQ(trust.placeholder, spelled);

    // A genuinely signed release, from a trusted-by-the-tests key, gets nothing from this build
    // whichever it is: with placeholders the trust is refused outright; with real keys the
    // test key is simply not among them.
    const Release release = readRelease("v2");
    const auto verified = update::verifyManifest(release.json, release.sig, trust, installFor("win64"));
    ASSERT_FALSE(verified.ok()) << "a test-signed manifest verified under the production keys";
    EXPECT_EQ(verified.error().code, core::ErrorCode::Unsupported) << verified.error().describe();

    if (trust.placeholder) {
        // Every verification FAILS: not one byte of any signature is checked against them.
        for (const update::TrustedKey& key : trust.keys) {
            const core::Status status = update::verifyEd25519(key.key, "x", update::SignatureBytes{});
            ASSERT_FALSE(status.ok());
            EXPECT_TRUE(contains(status.error().message, "public key is not accepted")) << status.error().describe();
        }
    }
}

// ---- placeholders ------------------------------------------------------------------------------

TEST(Trust, ThePlaceholderKeysAreNoPointsSoNoSignatureVerifiesUnderThem)
{
    // The two strings in cmake/KatanaUpdateKeys.cmake as they are today, as keys: for each, a
    // genuine signature (TEST 1's, over the v2 manifest) and an all-zero one are refused for
    // the key.
    const Release release = readRelease("v2");
    const auto line = update::parseSignatureLine(release.sig);
    ASSERT_TRUE(line.ok());
    for (const std::string_view hex : {kPlaceholderActiveHex, kPlaceholderSpareHex}) {
        for (const update::SignatureBytes& signature : {line.value().signature, update::SignatureBytes{}}) {
            const core::Status status = update::verifyEd25519(keyFromHex(hex), release.json, signature);
            ASSERT_FALSE(status.ok());
            EXPECT_EQ(status.error().code, core::ErrorCode::InvalidArgument);
            EXPECT_TRUE(contains(status.error().message, "public key is not accepted")) << status.error().describe();
        }
    }
}

TEST(Trust, ATrustFlaggedAsPlaceholderRefusesEvenASignatureThatWouldVerify)
{
    // The test keys are real keys, so under the flag alone nothing but the flag stops the
    // good v2 release: step 2 of the gate, reached because the line names a key in the trust.
    update::Trust trust = update::testTrust();
    trust.placeholder = true;
    const Release release = readRelease("v2");
    const auto verified = update::verifyManifest(release.json, release.sig, trust, installFor("win64"));
    ASSERT_FALSE(verified.ok());
    EXPECT_EQ(verified.error().code, core::ErrorCode::Unsupported);
    EXPECT_TRUE(contains(verified.error().message, "placeholder update keys")) << verified.error().describe();
}

TEST(Trust, APlaceholderTrustTellsAnUnknownSignerThatTheBuildCarriesPlaceholders)
{
    // A development build is asked about a real release: signed by a key that is not one of
    // the placeholders. Unsupported, and the context says why that happened here.
    const update::Trust placeholders = trustOf({kPlaceholderActiveHex, kPlaceholderSpareHex}, true);
    const Release release = readRelease("v2");
    const auto verified = update::verifyManifest(release.json, release.sig, placeholders, installFor("win64"));
    ASSERT_FALSE(verified.ok());
    EXPECT_EQ(verified.error().code, core::ErrorCode::Unsupported);
    EXPECT_TRUE(contains(verified.error().message, "does not trust")) << verified.error().describe();
    EXPECT_TRUE(contains(verified.error().context, "placeholder update keys")) << verified.error().describe();
}

TEST(Trust, APlaceholderKeyNamedByTheSignatureLineStillVerifiesNothingEvenWithTheFlagOff)
{
    // A trust whose keys are the placeholders but whose flag is wrongly off (the configure step
    // refuses that pairing, but the engine does not rely on it): a line naming a placeholder's
    // id finds the key, and the key is refused in the signature layer, not accepted.
    const update::Trust trust = trustOf({kPlaceholderActiveHex, kPlaceholderSpareHex}, false);
    const Release release = readRelease("v2");
    const std::string sig = "ed25519 504c414345484f4c " + release.sig.substr(25);
    const auto verified = update::verifyManifest(release.json, sig, trust, installFor("win64"));
    ASSERT_FALSE(verified.ok());
    EXPECT_EQ(verified.error().code, core::ErrorCode::InvalidArgument);
    EXPECT_TRUE(contains(verified.error().message, "public key is not accepted")) << verified.error().describe();
}

// ---- the gate, in the header's order, as far as it goes without a parser -----------------------

TEST(Trust, AnEmptyManifestIsRefusedBeforeAnythingElseIsLooked)
{
    // Not even the signature line is read: a garbage one still gives the manifest's reason.
    const auto verified = update::verifyManifest("", "not a signature line", update::testTrust(), installFor("win64"));
    ASSERT_FALSE(verified.ok());
    EXPECT_EQ(verified.error().code, core::ErrorCode::InvalidArgument);
    EXPECT_TRUE(contains(verified.error().message, "manifest is empty")) << verified.error().describe();
}

TEST(Trust, AManifestAboveTheCapIsRefusedAndOneAtTheCapIsNot)
{
    const std::string over(update::kMaxManifestBytes + 1, '{');
    const auto tooBig = update::verifyManifest(over, "not a signature line", update::testTrust(), installFor("win64"));
    ASSERT_FALSE(tooBig.ok());
    EXPECT_EQ(tooBig.error().code, core::ErrorCode::InvalidArgument);
    EXPECT_TRUE(contains(tooBig.error().message, "larger than")) << tooBig.error().describe();

    // Exactly the cap passes the size gate and meets the next one: the signature line.
    const std::string atCap(update::kMaxManifestBytes, '{');
    const auto atTheLimit = update::verifyManifest(atCap, "not a signature line", update::testTrust(), installFor("win64"));
    ASSERT_FALSE(atTheLimit.ok());
    EXPECT_EQ(atTheLimit.error().code, core::ErrorCode::ParseFailure) << atTheLimit.error().describe();
}

TEST(Trust, AMalformedSignatureLineIsAParseFailureBeforeAnyKeyIsLookedUp)
{
    const Release release = readRelease("v2");
    for (const std::string& sig : {std::string(), std::string("\n"), std::string("ed25519 d75a980182b10ab7\n"),
                                   release.sig + release.sig, "ED25519" + release.sig.substr(7)}) {
        const auto verified = update::verifyManifest(release.json, sig, update::testTrust(), installFor("win64"));
        ASSERT_FALSE(verified.ok()) << "accepted: '" << sig << "'";
        EXPECT_EQ(verified.error().code, core::ErrorCode::ParseFailure) << verified.error().describe();
    }
}

TEST(Trust, ASignerThisBuildDoesNotTrustIsUnsupportedWhateverTheSignature)
{
    const Release release = readRelease("bad/wrong-key"); // signed by TEST 3, named as TEST 3
    const auto verified = update::verifyManifest(release.json, release.sig, update::testTrust(), installFor("win64"));
    ASSERT_FALSE(verified.ok());
    EXPECT_EQ(verified.error().code, core::ErrorCode::Unsupported);
    EXPECT_TRUE(contains(verified.error().message, "signed by a key this build does not trust")) << verified.error().describe();
    EXPECT_TRUE(contains(verified.error().context, "fc51cd8e6218a1a3")) << verified.error().describe();

    // Even though that signature is perfectly good: pinning the signer's key makes it verify at
    // the signature layer, so it was the trust that refused it, not the arithmetic.
    update::Trust withTheOtherKey = update::testTrust();
    withTheOtherKey.keys.push_back(update::TrustedKey{
        "fc51cd8e6218a1a3", keyFromHex("fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025")});
    const auto line = update::parseSignatureLine(release.sig);
    ASSERT_TRUE(line.ok());
    EXPECT_TRUE(update::verifyEd25519(withTheOtherKey.keys.back().key, release.json, line.value().signature).ok());
}

TEST(Trust, ATrustWithNoKeysTrustsNoSigner)
{
    const Release release = readRelease("v2");
    const auto verified = update::verifyManifest(release.json, release.sig, update::Trust{}, installFor("win64"));
    ASSERT_FALSE(verified.ok());
    EXPECT_EQ(verified.error().code, core::ErrorCode::Unsupported);
}

TEST(Trust, AListedKeyWhoseIdIsNotTheIdOfItsBytesIsNotATrustedSigner)
{
    // A Trust built wrongly: TEST 1's bytes under an id that is not theirs. A line naming that
    // id finds the entry, and the entry is ignored rather than believed.
    update::Trust trust;
    trust.keys.push_back(update::TrustedKey{"0000000000000000", update::testTrust().keys[0].key});
    const Release release = readRelease("v2");
    const std::string sig = "ed25519 0000000000000000 " + release.sig.substr(25);
    const auto verified = update::verifyManifest(release.json, sig, trust, installFor("win64"));
    ASSERT_FALSE(verified.ok());
    EXPECT_EQ(verified.error().code, core::ErrorCode::Unsupported);
    EXPECT_TRUE(contains(verified.error().message, "does not trust")) << verified.error().describe();
}

TEST(Trust, AFlippedBitIsRefusedByTheSignatureBeforeTheTextIsParsed)
{
    // The answer is the signature's, InvalidArgument "does not match" - not the parser's. Held
    // by the code of the error: the skeleton's parse stub and a real one both answer something
    // else (Unsupported, ParseFailure), so a verifier that parsed first would fail this.
    const Release release = readRelease("bad/flipped-manifest-bit");
    const auto verified = update::verifyManifest(release.json, release.sig, update::testTrust(), installFor("win64"));
    ASSERT_FALSE(verified.ok());
    EXPECT_EQ(verified.error().code, core::ErrorCode::InvalidArgument);
    EXPECT_TRUE(contains(verified.error().message, "does not match")) << verified.error().describe();
}

TEST(Trust, ATextThatIsNotAManifestAndIsNotSignedIsRefusedForItsSignatureNotItsShape)
{
    // The order again, from the other side: with a parser that rejects this text, a gate that
    // parsed first would answer ParseFailure here. The answer is the signature's.
    const Release release = readRelease("v2");
    const auto verified = update::verifyManifest("this is not json", release.sig, update::testTrust(), installFor("win64"));
    ASSERT_FALSE(verified.ok());
    EXPECT_EQ(verified.error().code, core::ErrorCode::InvalidArgument) << verified.error().describe();
    EXPECT_TRUE(contains(verified.error().message, "does not match")) << verified.error().describe();
}

TEST(Trust, ASignedTextThatIsNotAManifestPassesTheSignatureAndIsRefusedByTheParser)
{
    REQUIRE_THE_MANIFEST_PARSER();
    // The signature verifies (checked at the signature layer first), so the refusal is the parse.
    const auto line = update::parseSignatureLine(kSignedGarbageLine);
    ASSERT_TRUE(line.ok());
    ASSERT_TRUE(update::verifyEd25519(update::testTrust().keys[0].key, kSignedGarbage, line.value().signature).ok());

    const auto verified = update::verifyManifest(kSignedGarbage, kSignedGarbageLine, update::testTrust(), installFor("win64"));
    ASSERT_FALSE(verified.ok());
    EXPECT_EQ(verified.error().code, core::ErrorCode::ParseFailure) << verified.error().describe();
    EXPECT_FALSE(contains(verified.error().message, "does not match")) << verified.error().describe();
}

TEST(Trust, ASignatureCopiedFromAnotherReleaseIsRefused)
{
    // v2's manifest with v1's genuine signature (same key, same file name): the signature is
    // for the other manifest's bytes.
    const Release swapped = readRelease("bad/swapped-sig");
    const auto verified = update::verifyManifest(swapped.json, swapped.sig, update::testTrust(), installFor("win64"));
    ASSERT_FALSE(verified.ok());
    EXPECT_EQ(verified.error().code, core::ErrorCode::InvalidArgument);
    EXPECT_TRUE(contains(verified.error().message, "does not match")) << verified.error().describe();

    // And the two real releases' signatures are not interchangeable: v1's manifest under v2's line.
    const Release v1 = readRelease("v1");
    const Release v2 = readRelease("v2");
    const auto crossed = update::verifyManifest(v1.json, v2.sig, update::testTrust(), installFor("win64"));
    ASSERT_FALSE(crossed.ok());
    EXPECT_EQ(crossed.error().code, core::ErrorCode::InvalidArgument);
}

TEST(Trust, ASignatureByAnotherKeyUnderTheNameOfAPinnedOneIsRefused)
{
    // TEST 3 signed it and the line says "d75a..." (the active key's id): the id selects the
    // key to verify with, and under that key the signature is wrong.
    const Release liar = readRelease("bad/wrong-key-claims-active");
    const auto verified = update::verifyManifest(liar.json, liar.sig, update::testTrust(), installFor("win64"));
    ASSERT_FALSE(verified.ok());
    EXPECT_EQ(verified.error().code, core::ErrorCode::InvalidArgument);
    EXPECT_TRUE(contains(verified.error().message, "does not match")) << verified.error().describe();
}

TEST(Trust, TheActiveKeysSignatureIsNotAcceptedAsTheSpareSOrTheOtherWayAround)
{
    // The id picks the key. v2 (active) with its line's id changed to the spare's, and the
    // spare-signed release with the active's: both signatures are good, for the wrong key.
    const Release active = readRelease("v2");
    const std::string asSpare = "ed25519 3d4017c3e843895a " + active.sig.substr(25);
    const auto first = update::verifyManifest(active.json, asSpare, update::testTrust(), installFor("win64"));
    ASSERT_FALSE(first.ok());
    EXPECT_EQ(first.error().code, core::ErrorCode::InvalidArgument);

    const Release spare = readRelease("v2-spare-signed");
    const std::string asActive = "ed25519 d75a980182b10ab7 " + spare.sig.substr(25);
    const auto second = update::verifyManifest(spare.json, asActive, update::testTrust(), installFor("win64"));
    ASSERT_FALSE(second.ok());
    EXPECT_EQ(second.error().code, core::ErrorCode::InvalidArgument);
}

// ---- the whole gate, with the parser (task T4) ---------------------------------------------------

TEST(Trust, ARealReleaseSignedByTheActiveKeyVerifiesAndCarriesTheBytesThatWereSigned)
{
    REQUIRE_THE_MANIFEST_PARSER();
    const Release release = readRelease("v2");
    const auto verified = update::verifyManifest(release.json, release.sig, update::testTrust(), installFor("win64"));
    ASSERT_TRUE(verified.ok()) << verified.error().describe();
    const update::Manifest& manifest = verified.value();
    // The producer's own v2 (tools/update_make_fixtures.py): Katana 0.10.0 for win64, 10 files.
    EXPECT_EQ(manifest.product, "katana");
    EXPECT_EQ(manifest.target, "win64");
    EXPECT_EQ(manifest.channel, "stable");
    EXPECT_EQ(manifest.tag, "v0.10.0");
    EXPECT_EQ(manifest.files.size(), 10u);
    // The record the helper keeps for later removals is the bytes as downloaded, so that it can
    // be verified again from the file.
    EXPECT_EQ(manifest.signedJson, release.json);
    EXPECT_EQ(manifest.signatureLine, release.sig);
}

TEST(Trust, TheSpareKeyVerifiesToo)
{
    REQUIRE_THE_MANIFEST_PARSER();
    const Release release = readRelease("v2-spare-signed");
    const auto verified = update::verifyManifest(release.json, release.sig, update::testTrust(), installFor("win64"));
    ASSERT_TRUE(verified.ok()) << verified.error().describe();
    EXPECT_EQ(verified.value().signedJson, release.json);
    EXPECT_EQ(verified.value().signatureLine, release.sig);
}

TEST(Trust, ASpareOnlyTrustVerifiesTheSpareAndRefusesTheActive)
{
    REQUIRE_THE_MANIFEST_PARSER();
    update::Trust spareOnly;
    spareOnly.keys.push_back(update::testTrust().keys[1]);
    const Release spare = readRelease("v2-spare-signed");
    EXPECT_TRUE(update::verifyManifest(spare.json, spare.sig, spareOnly, installFor("win64")).ok());
    const Release active = readRelease("v2");
    const auto refused = update::verifyManifest(active.json, active.sig, spareOnly, installFor("win64"));
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, core::ErrorCode::Unsupported); // the active key was dropped
}

TEST(Trust, ALineEndOfCrlfIsAcceptedAndKeptAsReceived)
{
    REQUIRE_THE_MANIFEST_PARSER();
    const Release release = readRelease("v2");
    const std::string crlf = release.sig.substr(0, release.sig.size() - 1) + "\r\n";
    const auto verified = update::verifyManifest(release.json, crlf, update::testTrust(), installFor("win64"));
    ASSERT_TRUE(verified.ok()) << verified.error().describe();
    EXPECT_EQ(verified.value().signatureLine, crlf);
}

TEST(Trust, ANotifyOnlyManifestVerifiesForItsOwnTarget)
{
    REQUIRE_THE_MANIFEST_PARSER();
    const Release release = readRelease("v2-macos-notify", "macos-arm64");
    const auto verified = update::verifyManifest(release.json, release.sig, update::testTrust(), installFor("macos-arm64"));
    ASSERT_TRUE(verified.ok()) << verified.error().describe();
    EXPECT_TRUE(verified.value().notifyOnly);
    EXPECT_TRUE(verified.value().files.empty());
}

TEST(Trust, AValidlySignedManifestOfAnotherTargetIsNotAnUpdateForThisInstall)
{
    REQUIRE_THE_MANIFEST_PARSER();
    const Release release = readRelease("v2");
    for (const char* target : {"win-arm64", "linux-x86_64", "linux-aarch64", "macos-arm64", "", "WIN64", "win64 "}) {
        const auto verified = update::verifyManifest(release.json, release.sig, update::testTrust(), installFor(target));
        ASSERT_FALSE(verified.ok()) << "'" << target << "'";
        EXPECT_EQ(verified.error().code, core::ErrorCode::InvalidArgument) << verified.error().describe();
        EXPECT_TRUE(contains(verified.error().message, "another target")) << verified.error().describe();
    }
}

TEST(Trust, AValidlySignedManifestOfAnotherProductIsNotAnUpdateForThisInstall)
{
    REQUIRE_THE_MANIFEST_PARSER();
    const Release release = readRelease("v2");
    update::Install install = installFor("win64");
    install.product = "other";
    const auto verified = update::verifyManifest(release.json, release.sig, update::testTrust(), install);
    ASSERT_FALSE(verified.ok());
    EXPECT_EQ(verified.error().code, core::ErrorCode::InvalidArgument);
    EXPECT_TRUE(contains(verified.error().message, "another product")) << verified.error().describe();
}

TEST(Trust, AValidlySignedManifestOfAnotherChannelIsRefused)
{
    REQUIRE_THE_MANIFEST_PARSER();
    // The control first: the same install takes the stable notify-only manifest of the
    // fixtures, so the refusal below is about the channel and not the shape of this one.
    const Release stable = readRelease("v2-macos-notify", "macos-arm64");
    ASSERT_TRUE(update::verifyManifest(stable.json, stable.sig, update::testTrust(), installFor("macos-arm64")).ok());

    const auto verified = update::verifyManifest(kBetaManifest, kBetaSignatureLine, update::testTrust(), installFor("macos-arm64"));
    ASSERT_FALSE(verified.ok());
    EXPECT_EQ(verified.error().code, core::ErrorCode::InvalidArgument);
    EXPECT_TRUE(contains(verified.error().message, "another channel")) << verified.error().describe();
}

TEST(Trust, TheBetaManifestIsGenuinelySignedSoOnlyItsChannelCanRefuseIt)
{
    // Not parser-dependent: the literal above really is signed by TEST 1 (so the channel case
    // refuses a good signature, not a bad one), and altering one byte of it is refused.
    const auto line = update::parseSignatureLine(kBetaSignatureLine);
    ASSERT_TRUE(line.ok());
    const update::PublicKey active = update::testTrust().keys[0].key;
    EXPECT_TRUE(update::verifyEd25519(active, kBetaManifest, line.value().signature).ok());
    std::string altered(kBetaManifest);
    altered[altered.find("beta")] = 'B';
    EXPECT_FALSE(update::verifyEd25519(active, altered, line.value().signature).ok());
}
