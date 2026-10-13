// A probe that proves the way the helper is LINKED, which the helper's own import check
// proves for one of its two libraries only.
//
// katana_updater must import nothing but the operating system's DLLs, and the way to that is
// STATIC OpenSSL and zlib (src/katana_update/CMakeLists.txt, katana_update_third_party) plus
// -static. The helper's own listing (test katana_updater_imports_only_os_dlls) speaks for
// libcrypto, which it carries: it checks every signature itself. For zlib it says nothing.
// The helper never inflates - a release is inflated when it is staged, by the program that
// downloads it, and the helper moves what was staged - so the linker takes no member of
// libz.a into it, and tests/update/check_updater_imports.cmake would pass on a link line
// that named zlib's DLL. This program calls what the engine calls of the two libraries -
// Ed25519 verification through EVP, raw inflate - and zlib's crc32 as well, through the same
// third-party target and the same link options, so the import check on IT is a check of the
// whole link line and the only evidence for zlib, and its run is a check that the static
// libraries work (a link that succeeded with the wrong flags can still fail to start or to
// verify).
//
// Never installed (tools/check_install_rules.cmake): a test executable, like the other two.
// Its expectations are not this program's output: RFC 8032, section 7.1, TEST 2 (the key, the
// one-byte message 0x72 and the signature, which `openssl pkeyutl -verify -rawin` also
// accepts - checked when this was written); the standard CRC-32 check value for "123456789";
// and the raw deflate stream of "abc" as zlib itself writes it.

#include <array>
#include <cstdint>
#include <cstdio>
#include <cstring>

#include <openssl/crypto.h>
#include <openssl/evp.h>
#include <zlib.h>

namespace {

constexpr std::array<std::uint8_t, 32> kPublicKey = {
    0x3d, 0x40, 0x17, 0xc3, 0xe8, 0x43, 0x89, 0x5a, 0x92, 0xb7, 0x0a, 0xa7, 0x4d, 0x1b, 0x7e, 0xbc,
    0x9c, 0x98, 0x2c, 0xcf, 0x2e, 0xc4, 0x96, 0x8c, 0xc0, 0xcd, 0x55, 0xf1, 0x2a, 0xf4, 0x66, 0x0c};
constexpr std::array<std::uint8_t, 64> kSignature = {
    0x92, 0xa0, 0x09, 0xa9, 0xf0, 0xd4, 0xca, 0xb8, 0x72, 0x0e, 0x82, 0x0b, 0x5f, 0x64, 0x25, 0x40,
    0xa2, 0xb2, 0x7b, 0x54, 0x16, 0x50, 0x3f, 0x8f, 0xb3, 0x76, 0x22, 0x23, 0xeb, 0xdb, 0x69, 0xda,
    0x08, 0x5a, 0xc1, 0xe4, 0x3e, 0x15, 0x99, 0x6e, 0x45, 0x8f, 0x36, 0x13, 0xd0, 0xf1, 0x1d, 0x8c,
    0x38, 0x7b, 0x2e, 0xae, 0xb4, 0x30, 0x2a, 0xee, 0xb0, 0x0d, 0x29, 0x16, 0x12, 0xbb, 0x0c, 0x00};

// 1 when `signature` verifies `message` under the RFC's TEST 2 key, else 0; -1 when the library
// could not even be set up.
int verify(std::uint8_t messageByte)
{
    EVP_PKEY* key = EVP_PKEY_new_raw_public_key(EVP_PKEY_ED25519, nullptr, kPublicKey.data(),
                                                kPublicKey.size());
    EVP_MD_CTX* context = EVP_MD_CTX_new();
    int result = -1;
    if (key != nullptr && context != nullptr &&
        EVP_DigestVerifyInit(context, nullptr, nullptr, nullptr, key) == 1) {
        result = EVP_DigestVerify(context, kSignature.data(), kSignature.size(), &messageByte, 1);
    }
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    return result;
}

} // namespace

int main()
{
    int failures = 0;
    const auto expect = [&failures](bool ok, const char* what) {
        if (!ok) {
            std::fprintf(stderr, "link probe: FAILED: %s\n", what);
            ++failures;
        }
    };

    expect(OPENSSL_init_crypto(OPENSSL_INIT_NO_LOAD_CONFIG, nullptr) == 1,
           "OPENSSL_init_crypto without the configuration file");

    // RFC 8032 TEST 2 verifies; the same signature over a message that differs in one bit
    // (0x72 ^ 0x01) must not.
    expect(verify(0x72) == 1, "RFC 8032 TEST 2 signature verifies");
    expect(verify(0x73) == 0, "the same signature over a message altered by one bit is refused");

    // Raw deflate (no zlib header), the form every payload member has: "abc" is 4b 4c 4a 06 00.
    constexpr std::array<std::uint8_t, 5> kDeflate = {0x4b, 0x4c, 0x4a, 0x06, 0x00};
    std::array<std::uint8_t, 16> inflated{};
    z_stream stream{};
    int status = inflateInit2(&stream, -15);
    expect(status == Z_OK, "inflateInit2 for a raw stream");
    if (status == Z_OK) {
        stream.next_in = const_cast<Bytef*>(kDeflate.data());
        stream.avail_in = kDeflate.size();
        stream.next_out = inflated.data();
        stream.avail_out = static_cast<uInt>(inflated.size());
        status = inflate(&stream, Z_FINISH);
        expect(status == Z_STREAM_END, "the raw stream ends");
        expect(stream.total_out == 3 && std::memcmp(inflated.data(), "abc", 3) == 0,
               "the raw stream inflates to abc");
        inflateEnd(&stream);
    }

    // The CRC-32 check value of the standard (ISO 3309, as used by zip and the journal).
    const char* const kCheck = "123456789";
    expect(crc32(0, reinterpret_cast<const Bytef*>(kCheck), 9) == 0xCBF43926u,
           "crc32(\"123456789\") is the standard check value 0xCBF43926");

    return failures;
}
