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

#include <algorithm>
#include <optional>
#include <string>

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

// verifyEd25519 under a key the TRUST lists. A key that is no usable public key is not a bad
// signature: it is a trust that was built wrongly (a mistyped pin), and it would refuse every
// release of every install. verifyEd25519 answers that with InvalidArgument, the code of a
// forged signature, which the callers hold to be unreachable for a genuine release; so the key
// is checked first and its failure is reported as Internal, "could not check", with its reason.
core::Status verifyWithPinnedKey(const PublicKey& key, std::string_view signedBytes, const SignatureBytes& signature)
{
    const core::Status usable = checkPublicKey(key);
    if (!usable.ok()) {
        const core::Error& why = usable.error();
        return core::makeError(core::ErrorCode::Internal, "a key of the update trust cannot be used: " + why.message,
                               why.context);
    }
    return verifyEd25519(key, signedBytes, signature);
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

    // The bytes this call works on, taken once: the signature is checked over these, the manifest
    // is read from these, and these are what the result carries, so "the bytes that were verified
    // are the bytes that are kept" holds by construction, whoever else can write to the caller's
    // buffer (a mapped file, a buffer another thread refills). The line is copied up to one byte
    // past its cap, which is all parseSignatureLine needs to refuse an oversize one.
    const std::string signedJson(json);
    const std::string signatureLine(sig.substr(0, std::min(sig.size(), static_cast<std::size_t>(kMaxSignatureBytes) + 1)));

    // 1. The signature line parses.
    const core::Result<SignatureLine> line = parseSignatureLine(signatureLine);
    if (!line) {
        return line.error();
    }

    // 2. A trust flagged as placeholders verifies nothing, whatever it lists, and for such a build
    //    that is the reason an update is refused: it is said here, before the lookup that would
    //    otherwise answer "signed by a key this build does not trust" to every real release.
    if (trust.placeholder) {
        return core::makeError(core::ErrorCode::Unsupported, "this build carries placeholder update keys",
                               "key " + line.value().keyId);
    }

    // 3. The line names a key this build trusts. A listed key whose id is not the id of its bytes
    //    is a Trust somebody built wrongly, and counts as absent.
    // 4. The signature over the exact bytes. Every listed key with the named id is tried, in the
    //    order listed (active, then spare): two keys share an id with probability 2^-64, but a
    //    Trust is a parameter and need not come from the generator. What is reported: a key that
    //    accepts wins; else a failure to check (Internal) over a refusal, so that "could not
    //    look" is never hidden behind "forged"; else the first refusal.
    std::optional<core::Status> outcome;
    for (const TrustedKey& candidate : trust.keys) {
        if (candidate.id != line.value().keyId || keyIdOf(candidate.key) != candidate.id) {
            continue;
        }
        core::Status attempt = verifyWithPinnedKey(candidate.key, signedJson, line.value().signature);
        const bool accepted = attempt.ok();
        const bool outranks = !accepted && outcome && !outcome->ok() &&
                              attempt.error().code == core::ErrorCode::Internal &&
                              outcome->error().code != core::ErrorCode::Internal;
        if (accepted || !outcome || outranks) {
            outcome = std::move(attempt);
        }
        if (accepted) {
            break;
        }
    }
    if (!outcome) {
        return core::makeError(core::ErrorCode::Unsupported, "signed by a key this build does not trust",
                               "key " + line.value().keyId);
    }
    if (!outcome->ok()) {
        return outcome->error();
    }

    // 5. Only now is the text read as a manifest.
    core::Result<Manifest> parsed = parseManifest(signedJson);
    if (!parsed) {
        return parsed.error();
    }
    Manifest manifest = std::move(parsed).value();

    // 6. Signed, and for this program: a validly signed manifest of another product, another
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

    manifest.signedJson = signedJson;
    manifest.signatureLine = signatureLine;
    return manifest;
}

} // namespace katana::update
