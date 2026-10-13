#pragma once

// THE TEST KEYS. Two Ed25519 keys that are not secret: RFC 8032, section 7.1, TEST 1
// (the "active" key) and TEST 2 (the "spare"). Anyone can sign with them, which is
// the point - a test key must never be one that a release could be signed with.
//
// They exist in exactly three executables, none of which is installed:
// katana_update_tests, katana_update_e2e and katana_updater_test. The production
// programs pass productionTrust() (update/trust.hpp), the keys pinned at build time;
// nothing in the environment, a file or an option moves that. tools/check_install_rules.cmake
// refuses to install the two executables that carry these.
//
// The Python side signs with the same keys: tests/update/fixtures builds the PKCS#8
// private key from the 32-byte seed with the 16-byte prefix 302e020100300506032b657004220420
// (a PEM of it is NOT committed: *.pem is git-ignored, by design), and
// `openssl pkeyutl -sign -rawin` signs a manifest with it. (Measured: openssl refuses an
// EMPTY file with "Could not allocate 0 bytes for oneshot"; a manifest is never empty.)
// The seeds and public keys below were checked against OpenSSL: the public key it derives
// from each seed is the one the RFC prints.

#include <string_view>

#include "katana/update/trust.hpp"

namespace katana::update {

inline constexpr std::string_view kTestActiveSeedHex =
    "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
inline constexpr std::string_view kTestActivePublicHex =
    "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
inline constexpr std::string_view kTestSpareSeedHex =
    "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb";
inline constexpr std::string_view kTestSparePublicHex =
    "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c";

// The active key then the spare, ids as keyIdOf gives them (the first 16 hex digits),
// placeholder == false. The one definition of the test trust root.
[[nodiscard]] const Trust& testTrust();

} // namespace katana::update
