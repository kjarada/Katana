#pragma once

// Ed25519 (RFC 8032) over the manifest's exact bytes, verified in this
// process by OpenSSL's EVP interface (EVP_DigestVerify with a null digest),
// never by a shell-out. OPENSSL_init_crypto(OPENSSL_INIT_NO_LOAD_CONFIG) runs
// before the first use, and the helper program clears OPENSSL_CONF,
// OPENSSL_MODULES and OPENSSL_ENGINES, so nothing in the environment can
// change what "verified" means.
//
// The signature file is one line: `ed25519 <keyid> <base64>`, where <keyid> is
// keyIdOf(public key) - the first 16 lower-case hex digits of the 32-byte
// public key - and <base64> is the 64-byte signature (standard alphabet,
// padded). A trailing LF or CRLF is allowed.
//
// What the verifier must refuse beyond what OpenSSL does. Measured while
// writing this skeleton: OpenSSL 3.6.5 returned "valid" for an all-zero
// public key and an all-zero signature on a one-byte message, because the
// zero encoding is a point of small order and the verification equation then
// holds for some messages. So: a signature whose S is not below the group
// order L (RFC 8032, 5.1.7), and a public key that is not a canonical
// encoding of a point of the prime-order subgroup. The pinned keys are
// generated, so they pass; the rule exists so that a key the build was handed
// wrongly cannot be forged for. (The placeholder keys, trust.hpp, are chosen
// to be no point at all.)

#include <array>
#include <cstdint>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"

namespace katana::update {

using PublicKey = std::array<std::uint8_t, 32>;
using SignatureBytes = std::array<std::uint8_t, 64>;

struct SignatureLine {
    std::string algorithm; // "ed25519"
    std::string keyId;     // 16 lower-case hex digits
    SignatureBytes signature{};
};

// The first 16 lower-case hexadecimal digits of the key's 32 bytes.
[[nodiscard]] std::string keyIdOf(const PublicKey& key);

// 64 lower-case hexadecimal digits -> the key. ParseFailure otherwise.
[[nodiscard]] core::Result<PublicKey> parsePublicKeyHex(std::string_view hex);

// `ed25519 <keyid> <base64>` -> its parts. ParseFailure for another algorithm,
// a malformed key id, base64 that is not 64 bytes, or more than one line.
[[nodiscard]] core::Result<SignatureLine> parseSignatureLine(std::string_view text);

// OK when `signature` is a valid Ed25519 signature of `message` under `key`.
// InvalidArgument when it is not (the message says which: key not accepted,
// S out of range, mismatch) - never a crash on any input, and never OK for a
// message that was altered by one bit. Internal, and only then, when OpenSSL
// itself fails (libcrypto cannot be initialised, a context cannot be made,
// the check cannot run): that says nothing about the signature.
[[nodiscard]] core::Status verifyEd25519(const PublicKey& key, std::string_view message,
                                         const SignatureBytes& signature);

// The first thing the helper's main() does (src/katana_updater, task T9): removes OPENSSL_CONF,
// OPENSSL_MODULES and OPENSSL_ENGINES from this process's environment and initialises libcrypto
// without reading a configuration file. Not called by katana, katana_cli or katana_mcp: they
// share a process with libcurl and Qt, which read those variables for their own reasons.
// verifyEd25519 makes the same initialisation on its first use, so a program that skips this
// still never loads a configuration through this library - provided nothing else in the process
// initialised libcrypto first. Not thread-safe against other threads reading the environment, so
// call it before starting any. Idempotent. Internal if libcrypto cannot be initialised.
[[nodiscard]] core::Status hardenOpenSslEnvironment();

} // namespace katana::update
