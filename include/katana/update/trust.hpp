#pragma once

// Who this program trusts to sign an update, and the one function that turns
// bytes from the network into a Manifest.
//
// Two Ed25519 public keys are pinned at build time (cmake/KatanaUpdateKeys.cmake):
// `active`, whose private half CI signs releases with, and an offline `spare`.
// Either verifies. Rotation: ship a build that pins a new spare; if the active
// key leaks, sign a release with the spare that drops it. Lose both and an
// installed copy cannot update until it is installed again by hand.
//
// THE TRUST ROOT IS A PARAMETER, never a global the environment can move.
// productionTrust() is what katana, katana_cli, katana_mcp and katana_updater
// pass; only the test executables (katana_update_tests, katana_update_e2e,
// katana_updater_test, never installed) pass a test key
// (tests/update/test_key.hpp). So no test key exists in a shipped binary, and
// no variable, file or flag at run time changes which signatures count.
//
// PLACEHOLDERS. Until the owner runs the key steps in docs/update.md the
// pinned keys are placeholders: 32 bytes that spell "PLACEHOLDER-..." in ASCII
// and that are not the encoding of any curve point, so no private key can
// match them. A Trust built from them has `placeholder == true`, and
// verifyManifest REFUSES to verify against it (Unsupported, "this build
// carries placeholder update keys"), so a development build can neither be
// updated nor forged for. KATANA_REQUIRE_UPDATE_KEYS (ON in release.yml) makes
// the configure step fail while the placeholders are in place, so a release can
// never ship with them.

#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/update/install.hpp"
#include "katana/update/manifest.hpp"
#include "katana/update/signature.hpp"

namespace katana::update {

struct TrustedKey {
    std::string id; // keyIdOf(key)
    PublicKey key{};
};

struct Trust {
    std::vector<TrustedKey> keys; // active, spare
    bool placeholder = false;     // true: pinned keys are the placeholders (see above)
};

// The keys this build pinned: `active` then `spare`, from
// KATANA_UPDATE_KEY_ACTIVE and KATANA_UPDATE_KEY_SPARE
// (src/katana_update/CMakeLists.txt passes them to trust.cpp alone).
[[nodiscard]] const Trust& productionTrust();

// The whole gate. `json` and `sig` are the two files of a release as
// downloaded. In this order, stopping at the first failure:
//   0. `json` is not empty and not above kMaxManifestBytes (InvalidArgument), whatever
//      `sig` holds;
//   1. the signature line parses (ParseFailure);
//   2. trust is not a placeholder (Unsupported, "this build carries placeholder update
//      keys"): asked before the key is looked up, so that a development build says why
//      it refuses every release and not that it does not know the signer;
//   3. the line's key id names a key in `trust` (otherwise Unsupported "signed by a key
//      this build does not trust");
//   4. verifyEd25519 over the EXACT bytes of `json`;
//   5. only now parseManifest;
//   6. the signed product equals install.product, target equals
//      install.target, channel is "stable";
// and the result carries signedJson and signatureLine. It does NOT decide
// "newer than installed" - check() does, with the installed (version,
// sequence) - because the helper repeats that test itself from its own copy.
// Failures: InvalidArgument (an empty or oversize manifest, a signature that does not verify, a
// manifest for another product, target or channel), ParseFailure (a signature line or a manifest
// that does not parse), Unsupported (a key this build does not trust, or placeholder keys), and
// Internal when the check could not be made: libcrypto itself failed, or the key the line names
// is no usable public key (a Trust built wrongly: a mistyped pin, say). That says nothing about
// the manifest, so a caller must treat it as "could not check", never as "forged". When several
// listed keys share the named id, a key that accepts wins, then an Internal, then the first
// refusal. Callers that tell these apart do it on the code, not on the message text.
[[nodiscard]] core::Result<Manifest> verifyManifest(std::string_view json, std::string_view sig,
                                                    const Trust& trust, const Install& install);

// True when the manifest is above what is installed: a higher version; or the same
// version with a higher sequence, but only when the install has a record of its own
// sequence (Install::hasInstalledRecord). A fresh install has none - the zip carries
// no sequence to compare with - so the same version is NOT newer: otherwise every
// fresh install would be offered the very release it was made from. A lower version
// is never newer. The one place the order is written (check() and the helper both
// call it). FINISHED, with tests, in src/katana_update/contract.cpp: task T5 keeps it.
[[nodiscard]] bool isNewerThan(const Manifest& manifest, const Install& install);

} // namespace katana::update
