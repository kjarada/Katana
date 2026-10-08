// Ed25519 verification (include/katana/update/signature.hpp, design section 3).
//
// The signature itself is checked by OpenSSL's EVP interface (EVP_DigestVerify with a null
// digest, which is how Ed25519 is spelled there). Three things are checked HERE, before
// OpenSSL is asked, because OpenSSL's answer alone is not the answer this program needs:
//
//   1. S below the group order L (RFC 8032, 5.1.7). Without it a valid signature has a
//      second form, S + L, that verifies too; this OpenSSL (3.6.5) rejects it, an older or
//      replaced one may not, and the rule must not depend on which one a build was linked to.
//   2. A public key that is the canonical encoding of a point of the prime-order subgroup,
//      and not the neutral element. Measured with the openssl command line (3.6.5): the
//      all-zero key, an all-zero signature and any of about a quarter of the one-byte
//      messages (9, 14, 17, 19, 26 ...) verify, because that key is a point of order 4 and
//      the verification equation then holds for some messages whatever the signer knew. The
//      pinned keys are generated and pass; the rule is for a key a build was handed wrongly.
//   3. OPENSSL_init_crypto(OPENSSL_INIT_NO_LOAD_CONFIG) before the first use, so that a
//      configuration file never decides what "verified" means.
//
// The point arithmetic below is RFC 8032, 5.1.4 (extended homogeneous coordinates, a = -1) on
// OpenSSL's BN type, so there is no second big-number library; it runs once per signature
// check, a few thousand modular multiplications. The key is public, so it need not run in
// constant time.

#include "katana/update/signature.hpp"

#include <openssl/bn.h>
#include <openssl/crypto.h>
#include <openssl/err.h>
#include <openssl/evp.h>

#include <algorithm>
#include <cstdlib>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/update/feed.hpp"

namespace katana::update {

namespace {

constexpr std::string_view kAlgorithm = "ed25519";
constexpr std::size_t kKeyIdDigits = 16;
// 64 bytes make 88 base64 characters: 21 groups of three bytes, then one byte and two pads.
constexpr std::size_t kSignatureBase64Length = 88;

constexpr bool isLowerHexDigit(char c)
{
    return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f');
}

constexpr int hexValue(char c)
{
    return c <= '9' ? c - '0' : c - 'a' + 10;
}

bool isAllLowerHex(std::string_view text)
{
    for (const char c : text) {
        if (!isLowerHexDigit(c)) {
            return false;
        }
    }
    return true;
}

// ---- base64, strict ----------------------------------------------------------------------

int base64Value(char c)
{
    if (c >= 'A' && c <= 'Z') {
        return c - 'A';
    }
    if (c >= 'a' && c <= 'z') {
        return c - 'a' + 26;
    }
    if (c >= '0' && c <= '9') {
        return c - '0' + 52;
    }
    if (c == '+') {
        return 62;
    }
    if (c == '/') {
        return 63;
    }
    return -1;
}

// The standard alphabet, padded, one string, and CANONICAL: the bits an incomplete last group
// does not use are zero, so there is exactly one text per byte string and a signature file
// cannot be altered without the bytes changing. No whitespace, no URL-safe alphabet.
std::optional<std::vector<std::uint8_t>> decodeBase64Canonical(std::string_view text)
{
    if (text.empty() || text.size() % 4 != 0) {
        return std::nullopt;
    }
    std::size_t pads = 0;
    if (text.back() == '=') {
        pads = text[text.size() - 2] == '=' ? 2 : 1;
    }
    std::vector<std::uint8_t> bytes;
    bytes.reserve(text.size() / 4 * 3);
    for (std::size_t group = 0; group < text.size(); group += 4) {
        const bool last = group + 4 == text.size();
        const std::size_t padsHere = last ? pads : 0;
        std::uint32_t bits = 0;
        for (std::size_t i = 0; i < 4; ++i) {
            const bool isPad = i >= 4 - padsHere;
            const int value = isPad ? (text[group + i] == '=' ? 0 : -1) : base64Value(text[group + i]);
            if (value < 0) {
                return std::nullopt; // not in the alphabet, or '=' before the end, or a pad too short
            }
            bits = (bits << 6) | static_cast<std::uint32_t>(value);
        }
        const std::uint32_t unusedBits = padsHere == 2 ? 0xFFFFu : (padsHere == 1 ? 0xFFu : 0u);
        if ((bits & unusedBits) != 0) {
            return std::nullopt; // non-canonical
        }
        bytes.push_back(static_cast<std::uint8_t>(bits >> 16));
        if (padsHere < 2) {
            bytes.push_back(static_cast<std::uint8_t>(bits >> 8));
        }
        if (padsHere < 1) {
            bytes.push_back(static_cast<std::uint8_t>(bits));
        }
    }
    return bytes;
}

core::Error badLine(std::string reason)
{
    return core::makeError(core::ErrorCode::ParseFailure, "the signature line " + std::move(reason),
                           "expected `ed25519 <16 hex digits> <88 base64 characters>` and one line end");
}

// ---- OpenSSL, set up once ----------------------------------------------------------------

// Initialised on first use, from any thread (a function-local static is initialised once, and
// the others wait). 1 on success.
bool opensslIsInitialised()
{
    static const bool initialised = OPENSSL_init_crypto(OPENSSL_INIT_NO_LOAD_CONFIG, nullptr) == 1;
    return initialised;
}

// OpenSSL's error queue is per thread and is only ever read by someone who asks. A refused
// signature leaves entries in it that nobody here reads, so they are cleared on the way out
// rather than left for the next libcrypto call on this thread to find.
struct ClearErrorQueueOnExit {
    ClearErrorQueueOnExit() = default;
    ClearErrorQueueOnExit(const ClearErrorQueueOnExit&) = delete;
    ClearErrorQueueOnExit& operator=(const ClearErrorQueueOnExit&) = delete;
    ~ClearErrorQueueOnExit() { ERR_clear_error(); }
};

// ---- the public key: a point of the prime-order subgroup ---------------------------------

// Thrown by Field when a BN call fails (memory); caught in classifyPublicKey. It never leaves
// this file.
struct BigNumFailure {};

struct BigNumDeleter {
    void operator()(BIGNUM* number) const { BN_free(number); }
};
struct BigNumContextDeleter {
    void operator()(BN_CTX* context) const { BN_CTX_free(context); }
};
using Num = std::unique_ptr<BIGNUM, BigNumDeleter>;

// p = 2^255 - 19, the field of Curve25519 (RFC 7748, section 4.1).
constexpr std::string_view kPrimeHex = "7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffed";
// L = 2^252 + 27742317777372353535851937790883648493, the order of the base point's subgroup
// (RFC 8032, section 5.1).
constexpr std::string_view kGroupOrderHex =
    "1000000000000000000000000000000014def9dea2f79cd65812631a5cf5d3ed";
static_assert(kPrimeHex.size() == 64 && kGroupOrderHex.size() == 64);
// The same L as bytes, little-endian, the order the signature stores S in.
constexpr std::uint8_t kGroupOrderLittleEndian[32] = {
    0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10};

class Field {
  public:
    Field() : context_(BN_CTX_new())
    {
        if (!context_) {
            throw BigNumFailure{};
        }
        prime_ = fromHex(kPrimeHex);
        order_ = fromHex(kGroupOrderHex);
        // d = -121665 / 121666 (RFC 8032, 5.1)
        d_ = mul(sub(zero(), fromWord(121665)), inv(fromWord(121666)));
        twoD_ = add(d_, d_);
    }

    [[nodiscard]] const BIGNUM* order() const { return order_.get(); }
    [[nodiscard]] const Num& d() const { return d_; }
    [[nodiscard]] const Num& twoD() const { return twoD_; }

    [[nodiscard]] Num zero() const { return fromWord(0); }
    [[nodiscard]] Num one() const { return fromWord(1); }
    [[nodiscard]] Num fromWord(BN_ULONG word) const
    {
        Num number = fresh();
        if (BN_set_word(number.get(), word) != 1) {
            throw BigNumFailure{};
        }
        return number;
    }
    [[nodiscard]] Num fromHex(std::string_view hex) const
    {
        const std::string terminated(hex);
        BIGNUM* raw = nullptr;
        if (BN_hex2bn(&raw, terminated.c_str()) == 0) {
            throw BigNumFailure{};
        }
        return Num(raw);
    }
    [[nodiscard]] Num copy(const Num& a) const
    {
        BIGNUM* raw = BN_dup(a.get());
        if (raw == nullptr) {
            throw BigNumFailure{};
        }
        return Num(raw);
    }
    [[nodiscard]] Num fromLittleEndian(const std::uint8_t* bytes, std::size_t count) const
    {
        BIGNUM* raw = BN_lebin2bn(bytes, static_cast<int>(count), nullptr);
        if (raw == nullptr) {
            throw BigNumFailure{};
        }
        return Num(raw);
    }

    // Every operation takes and returns numbers in [0, p).
    [[nodiscard]] Num add(const Num& a, const Num& b) const
    {
        Num result = fresh();
        check(BN_mod_add(result.get(), a.get(), b.get(), prime_.get(), context_.get()));
        return result;
    }
    [[nodiscard]] Num sub(const Num& a, const Num& b) const
    {
        Num result = fresh();
        check(BN_mod_sub(result.get(), a.get(), b.get(), prime_.get(), context_.get()));
        return result;
    }
    [[nodiscard]] Num mul(const Num& a, const Num& b) const
    {
        Num result = fresh();
        check(BN_mod_mul(result.get(), a.get(), b.get(), prime_.get(), context_.get()));
        return result;
    }
    [[nodiscard]] Num inv(const Num& a) const
    {
        BIGNUM* raw = BN_mod_inverse(nullptr, a.get(), prime_.get(), context_.get());
        if (raw == nullptr) {
            throw BigNumFailure{};
        }
        return Num(raw);
    }
    // A square root of `a` modulo p, or an empty pointer when `a` is not a square (and then
    // BN_mod_sqrt has left an entry in the error queue, cleared by the caller's guard).
    [[nodiscard]] Num sqrt(const Num& a) const
    {
        return Num(BN_mod_sqrt(nullptr, a.get(), prime_.get(), context_.get()));
    }

    [[nodiscard]] bool isAtLeastPrime(const Num& a) const { return BN_cmp(a.get(), prime_.get()) >= 0; }
    [[nodiscard]] static bool isZero(const Num& a) { return BN_is_zero(a.get()) == 1; }
    [[nodiscard]] static bool equal(const Num& a, const Num& b) { return BN_cmp(a.get(), b.get()) == 0; }

  private:
    static Num fresh()
    {
        BIGNUM* raw = BN_new();
        if (raw == nullptr) {
            throw BigNumFailure{};
        }
        return Num(raw);
    }
    static void check(int status)
    {
        if (status != 1) {
            throw BigNumFailure{};
        }
    }

    std::unique_ptr<BN_CTX, BigNumContextDeleter> context_;
    Num prime_;
    Num order_;
    Num d_;
    Num twoD_;
};

// Extended homogeneous coordinates (X : Y : Z : T) with x = X/Z, y = Y/Z and xy = T/Z.
struct Point {
    Num x, y, z, t;
};

// RFC 8032, 5.1.4. The addition is complete for this curve (a is a square, d is not), so it
// is right for every pair of points, doubling and the neutral element included.
Point add(const Field& f, const Point& p, const Point& q)
{
    const Num a = f.mul(f.sub(p.y, p.x), f.sub(q.y, q.x));
    const Num b = f.mul(f.add(p.y, p.x), f.add(q.y, q.x));
    const Num c = f.mul(f.mul(p.t, f.twoD()), q.t);
    const Num d = f.mul(f.add(p.z, p.z), q.z);
    const Num e = f.sub(b, a);
    const Num ff = f.sub(d, c);
    const Num g = f.add(d, c);
    const Num h = f.add(b, a);
    return Point{f.mul(e, ff), f.mul(g, h), f.mul(ff, g), f.mul(e, h)};
}

Point twice(const Field& f, const Point& p)
{
    const Num a = f.mul(p.x, p.x);
    const Num b = f.mul(p.y, p.y);
    const Num c = f.add(f.mul(p.z, p.z), f.mul(p.z, p.z));
    const Num h = f.add(a, b);
    const Num xy = f.add(p.x, p.y);
    const Num e = f.sub(h, f.mul(xy, xy));
    const Num g = f.sub(a, b);
    const Num ff = f.add(c, g);
    return Point{f.mul(e, ff), f.mul(g, h), f.mul(ff, g), f.mul(e, h)};
}

enum class KeyVerdict {
    Accepted,
    NonCanonical,   // y >= p, or x = 0 with the sign bit set (RFC 8032, 5.1.3)
    NotOnTheCurve,  // no x for this y
    NeutralElement, // the identity: (0, 1)
    NotPrimeOrder,  // a point of small order, or of mixed order (L * A is not the identity)
    Failure,        // libcrypto could not allocate
};

// Decodes the key as RFC 8032, 5.1.3 does (its refusals of a y at or above p, of a y with no x
// and of "-0" included) and then requires L * A = the identity. Every point
// of the curve is a point of the prime-order subgroup plus a point of the 8-element torsion
// subgroup, and L is odd and prime to 8, so L * A is the identity exactly when the torsion part
// is, i.e. when A is in the prime-order subgroup (or is the identity, refused before).
KeyVerdict classifyPublicKey(const PublicKey& key)
{
    try {
        const Field f;
        const bool signBit = (key[31] & 0x80) != 0;
        PublicKey magnitude = key;
        magnitude[31] = static_cast<std::uint8_t>(magnitude[31] & 0x7F);

        const Num y = f.fromLittleEndian(magnitude.data(), magnitude.size());
        if (f.isAtLeastPrime(y)) {
            return KeyVerdict::NonCanonical;
        }
        const Num one = f.one();
        const Num ySquared = f.mul(y, y);
        const Num numerator = f.sub(ySquared, one);                 // y^2 - 1
        const Num denominator = f.add(f.mul(f.d(), ySquared), one); // d y^2 + 1
        if (Field::isZero(denominator)) {
            return KeyVerdict::NotOnTheCurve; // not reachable (d is a non-square), but never divide by it
        }
        const Num xSquared = f.mul(numerator, f.inv(denominator));
        Num x = f.sqrt(xSquared);
        if (!x) {
            return KeyVerdict::NotOnTheCurve;
        }
        if (Field::isZero(x)) {
            if (signBit) {
                return KeyVerdict::NonCanonical; // "-0" has no encoding
            }
            if (Field::equal(y, one)) {
                return KeyVerdict::NeutralElement;
            }
        }
        // The sign bit picks x or -x. Whether the point lies in the prime-order subgroup is the
        // same for both (-A = A reflected is in it exactly when A is), so the root is used as
        // it came.

        Num xy = f.mul(x, y);
        const Point base{std::move(x), f.copy(y), f.one(), std::move(xy)};
        Point multiple{f.zero(), f.one(), f.one(), f.zero()};
        for (int bit = BN_num_bits(f.order()) - 1; bit >= 0; --bit) {
            multiple = twice(f, multiple);
            if (BN_is_bit_set(f.order(), bit) == 1) {
                multiple = add(f, multiple, base);
            }
        }
        // The identity in extended coordinates: X = 0 and Y = Z.
        const bool identity = Field::isZero(multiple.x) && Field::equal(multiple.y, multiple.z);
        return identity ? KeyVerdict::Accepted : KeyVerdict::NotPrimeOrder;
    } catch (const BigNumFailure&) {
        return KeyVerdict::Failure;
    }
}

core::Error refusedKey(KeyVerdict verdict, const PublicKey& key)
{
    const char* why = "";
    switch (verdict) {
    case KeyVerdict::NonCanonical:
        why = "not a canonical encoding (RFC 8032, 5.1.3)";
        break;
    case KeyVerdict::NotOnTheCurve:
        why = "not the encoding of a point of the curve";
        break;
    case KeyVerdict::NeutralElement:
        why = "the neutral element";
        break;
    case KeyVerdict::NotPrimeOrder:
        why = "a point of small or mixed order, not of the prime-order subgroup";
        break;
    case KeyVerdict::Failure:
        return core::makeError(core::ErrorCode::Internal, "the public key could not be checked: libcrypto failed",
                               "key " + keyIdOf(key));
    case KeyVerdict::Accepted:
        break;
    }
    return core::makeError(core::ErrorCode::InvalidArgument,
                           std::string("the public key is not accepted: ") + why, "key " + keyIdOf(key));
}

// S, the last 32 bytes of the signature as a little-endian number, compared with L from the top.
bool scalarIsBelowGroupOrder(const SignatureBytes& signature)
{
    for (std::size_t i = 32; i-- > 0;) {
        const std::uint8_t s = signature[32 + i];
        const std::uint8_t order = kGroupOrderLittleEndian[i];
        if (s != order) {
            return s < order;
        }
    }
    return false; // S == L
}

} // namespace

std::string keyIdOf(const PublicKey& key)
{
    constexpr std::string_view digits = "0123456789abcdef";
    std::string id;
    id.reserve(kKeyIdDigits);
    for (std::size_t i = 0; i < kKeyIdDigits / 2; ++i) {
        id.push_back(digits[key[i] >> 4]);
        id.push_back(digits[key[i] & 0x0F]);
    }
    return id;
}

core::Result<PublicKey> parsePublicKeyHex(std::string_view hex)
{
    PublicKey key{};
    if (hex.size() != 2 * key.size() || !isAllLowerHex(hex)) {
        return core::makeError(core::ErrorCode::ParseFailure,
                               "a public key is 64 lower-case hexadecimal digits",
                               "got " + std::to_string(hex.size()) + " characters");
    }
    for (std::size_t i = 0; i < key.size(); ++i) {
        key[i] = static_cast<std::uint8_t>(hexValue(hex[2 * i]) * 16 + hexValue(hex[2 * i + 1]));
    }
    return key;
}

core::Result<SignatureLine> parseSignatureLine(std::string_view text)
{
    if (text.size() > kMaxSignatureBytes) {
        return badLine("is longer than the " + std::to_string(kMaxSignatureBytes) + " bytes allowed");
    }
    // One line, ended by at most one LF or CRLF. A stray CR or a second LF is a second line.
    std::string_view line = text;
    if (line.size() >= 2 && line.substr(line.size() - 2) == "\r\n") {
        line.remove_suffix(2);
    } else if (!line.empty() && line.back() == '\n') {
        line.remove_suffix(1);
    }
    if (line.empty()) {
        return badLine("is empty");
    }
    if (line.find_first_of("\r\n") != std::string_view::npos) {
        return badLine("is more than one line");
    }

    // Exactly two single spaces. Anything else (a tab, a doubled or trailing space, a missing
    // field) leaves a token that fails its own test below or too few tokens.
    const std::size_t firstSpace = line.find(' ');
    const std::size_t secondSpace =
        firstSpace == std::string_view::npos ? std::string_view::npos : line.find(' ', firstSpace + 1);
    if (secondSpace == std::string_view::npos || line.find(' ', secondSpace + 1) != std::string_view::npos) {
        return badLine("does not have exactly three fields separated by single spaces");
    }
    const std::string_view algorithm = line.substr(0, firstSpace);
    const std::string_view keyId = line.substr(firstSpace + 1, secondSpace - firstSpace - 1);
    const std::string_view encoded = line.substr(secondSpace + 1);

    if (algorithm != kAlgorithm) {
        return badLine("names an algorithm other than ed25519");
    }
    if (keyId.size() != kKeyIdDigits || !isAllLowerHex(keyId)) {
        return badLine("has a key id that is not 16 lower-case hexadecimal digits");
    }
    if (encoded.size() != kSignatureBase64Length) {
        return badLine("has a signature that is not 88 base64 characters (64 bytes)");
    }
    const std::optional<std::vector<std::uint8_t>> bytes = decodeBase64Canonical(encoded);
    if (!bytes) {
        return badLine("has a signature that is not canonical padded base64");
    }
    // 88 characters are 64 bytes only with two pads; with one pad or none they are 65 or 66.
    if (bytes->size() != SignatureBytes{}.size()) {
        return badLine("has a signature of " + std::to_string(bytes->size()) + " bytes, not 64");
    }

    SignatureLine parsed;
    parsed.algorithm = std::string(algorithm);
    parsed.keyId = std::string(keyId);
    std::copy(bytes->begin(), bytes->end(), parsed.signature.begin());
    return parsed;
}

core::Status verifyEd25519(const PublicKey& key, std::string_view message, const SignatureBytes& signature)
{
    const ClearErrorQueueOnExit clearErrors;
    const std::string context = "key " + keyIdOf(key);

    // Before the first libcrypto call of any kind (the key check's big numbers are libcrypto
    // too), so that no other call can start the library with its defaults first.
    if (!opensslIsInitialised()) {
        return core::makeError(core::ErrorCode::Internal, "libcrypto could not be initialised", context);
    }
    // Then S, which needs nothing, and the key; and only then OpenSSL's verdict on the signature.
    if (!scalarIsBelowGroupOrder(signature)) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "the signature is refused: S is not below the group order L (RFC 8032, 5.1.7)",
                               context);
    }
    if (const KeyVerdict verdict = classifyPublicKey(key); verdict != KeyVerdict::Accepted) {
        return refusedKey(verdict, key);
    }

    std::unique_ptr<EVP_PKEY, decltype(&EVP_PKEY_free)> publicKey(
        EVP_PKEY_new_raw_public_key(EVP_PKEY_ED25519, nullptr, key.data(), key.size()), &EVP_PKEY_free);
    std::unique_ptr<EVP_MD_CTX, decltype(&EVP_MD_CTX_free)> verifier(EVP_MD_CTX_new(), &EVP_MD_CTX_free);
    if (!publicKey || !verifier ||
        EVP_DigestVerifyInit(verifier.get(), nullptr, nullptr, nullptr, publicKey.get()) != 1) {
        return core::makeError(core::ErrorCode::Internal, "OpenSSL could not set up the verification", context);
    }

    // An empty message has no data() to hand over; RFC 8032's TEST 1 signs exactly that.
    constexpr unsigned char kNothing = 0;
    const auto* data = message.empty() ? &kNothing : reinterpret_cast<const unsigned char*>(message.data());
    const int status =
        EVP_DigestVerify(verifier.get(), signature.data(), signature.size(), data, message.size());
    if (status == 1) {
        return {};
    }
    if (status == 0) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "the signature does not match the message under this key", context);
    }
    // Below zero is OpenSSL failing to run the check, which says nothing about the signature:
    // not the code of a bad signature, for a caller that tells "forged" from "could not look".
    return core::makeError(core::ErrorCode::Internal, "OpenSSL could not complete the verification", context);
}

core::Status hardenOpenSslEnvironment()
{
    // The three variables that make OpenSSL read a file or load code from a place the
    // environment names (a configuration, a provider module, an engine). The same list as
    // tools/update_make_manifest.py gives the openssl it runs.
    constexpr const char* kVariables[] = {"OPENSSL_CONF", "OPENSSL_MODULES", "OPENSSL_ENGINES"};
    for (const char* name : kVariables) {
#ifdef _WIN32
        // An empty value removes the variable; the C runtime matches the name without regard to
        // case, as Windows does.
        _putenv_s(name, "");
#else
        unsetenv(name);
#endif
    }
    if (!opensslIsInitialised()) {
        return core::makeError(core::ErrorCode::Internal, "libcrypto could not be initialised");
    }
    return {};
}

} // namespace katana::update
