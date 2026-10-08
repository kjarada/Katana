// The pinned keys and the one gate between bytes from the network and a Manifest
// (include/katana/update/trust.hpp). isNewerThan is in contract.cpp.
//
// The pinned keys reach THIS FILE ALONE as compile definitions
// (src/katana_update/CMakeLists.txt, from cmake/KatanaUpdateKeys.cmake), so a change of key
// recompiles one file:
//
//   KATANA_UPDATE_KEY_ACTIVE, KATANA_UPDATE_KEY_SPARE
//        64 lower-case hexadecimal digits each (an Ed25519 public key)
//   KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS
//        1 while cmake/KatanaUpdateKeys.cmake still holds the placeholders, else 0

#include "katana/update/trust.hpp"

#include "katana/update/feed.hpp"

#if !defined(KATANA_UPDATE_KEY_ACTIVE) || !defined(KATANA_UPDATE_KEY_SPARE) || \
    !defined(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS)
#error "the update keys are defined for trust.cpp by src/katana_update/CMakeLists.txt"
#endif

namespace katana::update {

namespace {

constexpr std::string_view kPinnedActive = KATANA_UPDATE_KEY_ACTIVE;
constexpr std::string_view kPinnedSpare = KATANA_UPDATE_KEY_SPARE;
// The only channel a build follows (design section 1); the signed channel member is held to it.
constexpr std::string_view kChannel = "stable";

constexpr bool isLowerHex(std::string_view text)
{
    for (const char c : text) {
        if (!((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f'))) {
            return false;
        }
    }
    return true;
}

static_assert(kPinnedActive.size() == 64, "KATANA_UPDATE_KEY_ACTIVE is 64 hexadecimal digits");
static_assert(kPinnedSpare.size() == 64, "KATANA_UPDATE_KEY_SPARE is 64 hexadecimal digits");
static_assert(isLowerHex(kPinnedActive), "KATANA_UPDATE_KEY_ACTIVE is lower-case hexadecimal");
static_assert(isLowerHex(kPinnedSpare), "KATANA_UPDATE_KEY_SPARE is lower-case hexadecimal");
static_assert(kPinnedActive != kPinnedSpare, "the active and the spare key are two keys");
static_assert(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS == 0 || KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS == 1,
              "KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS is 0 or 1");

Trust buildProductionTrust()
{
    Trust trust;
    trust.placeholder = KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS == 1;
    for (const std::string_view hex : {kPinnedActive, kPinnedSpare}) {
        const core::Result<PublicKey> key = parsePublicKeyHex(hex);
        if (!key) {
            // Not reachable: the asserts above hold both strings to the form the parser takes.
            // But if it were, the answer is a trust that verifies nothing, never a partial one.
            return Trust{{}, true};
        }
        trust.keys.push_back(TrustedKey{keyIdOf(key.value()), key.value()});
    }
    return trust;
}

} // namespace

const Trust& productionTrust()
{
    static const Trust trust = buildProductionTrust();
    return trust;
}

core::Result<Manifest> verifyManifest(std::string_view json, std::string_view sig, const Trust& trust,
                                      const Install& install)
{
    // Before any parsing or arithmetic: nothing is signed by an empty file, and what the
    // transport was allowed to hand over (kMaxManifestBytes) is also the most that is verified.
    if (json.empty()) {
        return core::makeError(core::ErrorCode::InvalidArgument, "the manifest is empty");
    }
    if (json.size() > kMaxManifestBytes) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "the manifest is larger than the " + std::to_string(kMaxManifestBytes) +
                                   " bytes this program accepts",
                               std::to_string(json.size()) + " bytes");
    }

    // 1. The signature line parses and names a key this build trusts. A listed key whose id is
    //    not the id of its bytes is a Trust somebody built wrongly, and counts as absent.
    const core::Result<SignatureLine> line = parseSignatureLine(sig);
    if (!line) {
        return line.error();
    }
    const TrustedKey* signer = nullptr;
    for (const TrustedKey& candidate : trust.keys) {
        if (candidate.id == line.value().keyId && keyIdOf(candidate.key) == candidate.id) {
            signer = &candidate;
            break;
        }
    }
    if (signer == nullptr) {
        return core::makeError(core::ErrorCode::Unsupported, "signed by a key this build does not trust",
                               "key " + line.value().keyId +
                                   (trust.placeholder ? "; this build carries placeholder update keys" : ""));
    }

    // 2. A trust flagged as placeholders verifies nothing, whatever it lists. Step 1 has already
    //    sent every real signer away from such a trust, saying why in its context; this is the
    //    backstop for a line that names a key the trust lists.
    if (trust.placeholder) {
        return core::makeError(core::ErrorCode::Unsupported, "this build carries placeholder update keys",
                               "key " + signer->id);
    }

    // 3. The signature over the exact bytes.
    if (const core::Status verified = verifyEd25519(signer->key, json, line.value().signature); !verified) {
        return verified.error();
    }

    // 4. Only now is the text read as a manifest.
    core::Result<Manifest> parsed = parseManifest(json);
    if (!parsed) {
        return parsed.error();
    }
    Manifest manifest = std::move(parsed).value();

    // 5. Signed, and for this program: a validly signed manifest of another product, another
    //    platform or another channel is still not an update for this install.
    if (manifest.product != install.product) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "the manifest is signed for another product",
                               "manifest '" + manifest.product + "', this install '" + install.product + "'");
    }
    if (manifest.target != install.target) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "the manifest is signed for another target",
                               "manifest '" + manifest.target + "', this install '" + install.target + "'");
    }
    if (manifest.channel != kChannel) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "the manifest is signed for another channel than stable",
                               "manifest '" + manifest.channel + "'");
    }

    manifest.signedJson = std::string(json);
    manifest.signatureLine = std::string(sig);
    return manifest;
}

} // namespace katana::update
