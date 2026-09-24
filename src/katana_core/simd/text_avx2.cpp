// AVX2 text kernels. Compiled with -mavx2 -mfma by katana_add_simd_sources.
//
// A KERNEL FILE: raw pointers in, raw pointers out, and nothing with external
// linkage but the katana_avx2_ entries. It includes only intrinsics, <cstddef>,
// <cstdint> and its declarations, because any inline function from a shared
// header compiled here would come out as AVX2 code that the linker may hand to
// a baseline caller (docs/performance.md, "The inline-copy hazard").
// tools/check_simd_kernels.cmake enforces both rules, on the source and on the
// object file.

#include <immintrin.h>

#include <cstddef>
#include <cstdint>

#include "text_kernels.hpp"

namespace {

// 32 code units, 64 bytes: two 256-bit loads that pack into one 256-bit store.
constexpr std::size_t kUnitsPerStep = 32;

inline __m256i load(const unsigned char* at)
{
    return _mm256_loadu_si256(reinterpret_cast<const __m256i*>(at));
}

// ---- UTF-8 validation ------------------------------------------------------
//
// The "lookup" algorithm of J. Keiser and D. Lemire, "Validating UTF-8 in less
// than one instruction per byte", Software: Practice and Experience 51(5),
// 2021. Every byte is judged against the three before it, 32 at a time, with
// no branch on the data: nothing depends on where one character ends and the
// next begins, which is what makes text dense with accented, Cyrillic or CJK
// characters nearly as fast as ASCII. The byte loop it must equal is
// kernels::validUtf8Scalar.
//
// Each error UTF-8 can have is one bit. Three 16-entry tables, indexed by the
// previous byte's high nibble, its low nibble and this byte's high nibble,
// each give the errors that nibble ALLOWS; a pair of bytes is wrong when all
// three allow one error. The tables are the paper's; the tests hold them to
// the byte loop over every pair of bytes and every lead with representative
// followers at every block edge (tests/core/test_simd_text.cpp).
constexpr std::uint8_t kTooShort = 1 << 0;  // a lead, then ASCII or another lead
constexpr std::uint8_t kTooLong = 1 << 1;   // ASCII, then a continuation
constexpr std::uint8_t kOverlong3 = 1 << 2; // E0 80-9F
constexpr std::uint8_t kTooLarge = 1 << 3;  // F4 90-BF; F5-FF then 90-BF
constexpr std::uint8_t kSurrogate = 1 << 4; // ED A0-BF, which would be U+D800-DFFF
constexpr std::uint8_t kOverlong2 = 1 << 5; // C0 or C1, then a continuation
// F5-FF then 80-8F, and F0 80-8F: one bit, because F0 and F5-FF differ in
// their low nibble, which the second table tells apart.
constexpr std::uint8_t kTooLarge1000 = 1 << 6;
constexpr std::uint8_t kOverlong4 = 1 << 6;
constexpr std::uint8_t kTwoConts = 1 << 7; // a continuation, then a continuation
// The errors that the high nibbles alone decide.
constexpr std::uint8_t kCarry = kTooShort | kTooLong | kTwoConts;
constexpr std::uint8_t kLarge = kTooLarge | kTooLarge1000;

// The same 16 bytes in both 128-bit halves, because VPSHUFB looks up per half.
inline __m256i table(const std::uint8_t (&entries)[16])
{
    return _mm256_broadcastsi128_si256(
        _mm_loadu_si128(reinterpret_cast<const __m128i*>(entries)));
}

// Each byte's N-th predecessor: the last N bytes of `prior`, then the first
// 32 - N of `input`.
template <int N>
inline __m256i previous(__m256i input, __m256i prior)
{
    return _mm256_alignr_epi8(input, _mm256_permute2x128_si256(prior, input, 0x21), 16 - N);
}

class Utf8Checker {
public:
    Utf8Checker()
    {
        const std::uint8_t firstHigh[16] = {
            kTooLong, kTooLong, kTooLong, kTooLong, // 0___: ASCII
            kTooLong, kTooLong, kTooLong, kTooLong,
            kTwoConts, kTwoConts, kTwoConts, kTwoConts, // 10__: a continuation
            kTooShort | kOverlong2,                     // 1100
            kTooShort,                                  // 1101
            kTooShort | kOverlong3 | kSurrogate,        // 1110
            kTooShort | kLarge | kOverlong4};           // 1111
        const std::uint8_t firstLow[16] = {
            kCarry | kOverlong3 | kOverlong2 | kOverlong4, // ____0000
            kCarry | kOverlong2,                           // ____0001
            kCarry,                                        // ____0010
            kCarry,                                        // ____0011
            kCarry | kTooLarge,                            // ____0100
            kCarry | kLarge,                               // ____0101
            kCarry | kLarge,                               // ____0110
            kCarry | kLarge,                               // ____0111
            kCarry | kLarge,                               // ____1000
            kCarry | kLarge,                               // ____1001
            kCarry | kLarge,                               // ____1010
            kCarry | kLarge,                               // ____1011
            kCarry | kLarge,                               // ____1100
            kCarry | kLarge | kSurrogate,                  // ____1101
            kCarry | kLarge,                               // ____1110
            kCarry | kLarge};                              // ____1111
        const std::uint8_t secondHigh[16] = {
            kTooShort, kTooShort, kTooShort, kTooShort, // 0___: ASCII
            kTooShort, kTooShort, kTooShort, kTooShort,
            kTooLong | kOverlong2 | kTwoConts | kOverlong3 | kTooLarge1000 | kOverlong4, // 1000
            kTooLong | kOverlong2 | kTwoConts | kOverlong3 | kTooLarge,                  // 1001
            kTooLong | kOverlong2 | kTwoConts | kSurrogate | kTooLarge,                  // 1010
            kTooLong | kOverlong2 | kTwoConts | kSurrogate | kTooLarge,                  // 1011
            kTooShort, kTooShort, kTooShort, kTooShort}; // 11__: a lead
        // A block left open at its end: 1111____ third from last, 111_____
        // second from last, or 11______ last, begins a character the block
        // does not finish. Those are the bytes above these limits.
        const std::uint8_t openAtEnd[32] = {
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 0xF0 - 1, 0xE0 - 1, 0xC0 - 1};
        firstHigh_ = table(firstHigh);
        firstLow_ = table(firstLow);
        secondHigh_ = table(secondHigh);
        openAtEnd_ = load(openAtEnd);
    }

    // Judges the 32 bytes that follow the last ones judged, or begin the text.
    void check(__m256i input)
    {
        const __m256i nibble = _mm256_set1_epi8(0x0F);
        const auto high = [&](__m256i v) {
            return _mm256_and_si256(_mm256_srli_epi16(v, 4), nibble);
        };
        const __m256i prev1 = previous<1>(input, prior_);
        const __m256i pairErrors = _mm256_and_si256(
            _mm256_and_si256(_mm256_shuffle_epi8(firstHigh_, high(prev1)),
                             _mm256_shuffle_epi8(firstLow_, _mm256_and_si256(prev1, nibble))),
            _mm256_shuffle_epi8(secondHigh_, high(input)));
        // Two continuations in a row are right exactly where a three-byte lead
        // is two back or a four-byte lead three back. Less 0x60, only E0-FF
        // reach 0x80; less 0x70, only F0-FF do.
        const __m256i third = _mm256_subs_epu8(previous<2>(input, prior_),
                                               _mm256_set1_epi8(static_cast<char>(0xE0 - 0x80)));
        const __m256i fourth = _mm256_subs_epu8(previous<3>(input, prior_),
                                                _mm256_set1_epi8(static_cast<char>(0xF0 - 0x80)));
        const __m256i wanted = _mm256_and_si256(_mm256_or_si256(third, fourth),
                                                _mm256_set1_epi8(static_cast<char>(0x80)));
        errors_ = _mm256_or_si256(errors_, _mm256_xor_si256(wanted, pairErrors));
        open_ = _mm256_subs_epu8(input, openAtEnd_);
        prior_ = input;
    }

    // 32 bytes of ASCII after the last judged: a character left open before
    // them is cut short.
    void ascii(__m256i input)
    {
        errors_ = _mm256_or_si256(errors_, open_);
        open_ = _mm256_setzero_si256();
        prior_ = input;
    }

    // Judges the text's last 32 bytes, `prior` being the 32 before them. Some
    // were judged already; judging a byte twice gives the same answer, and it
    // lets the text end without a partial block.
    void last(__m256i input, __m256i prior)
    {
        prior_ = prior;
        check(input);
    }

    [[nodiscard]] bool failed() const { return _mm256_testz_si256(errors_, errors_) == 0; }

    // At the end of the text: no error, and no character left open.
    [[nodiscard]] bool valid() const
    {
        const __m256i either = _mm256_or_si256(errors_, open_);
        return _mm256_testz_si256(either, either) != 0;
    }

private:
    __m256i firstHigh_;
    __m256i firstLow_;
    __m256i secondHigh_;
    __m256i openAtEnd_;
    __m256i errors_ = _mm256_setzero_si256();
    __m256i prior_ = _mm256_setzero_si256(); // before the text, as if ASCII
    __m256i open_ = _mm256_setzero_si256();
};

} // namespace

extern "C" std::size_t katana_avx2_narrow_ascii_utf16(const unsigned char* in, std::size_t units,
                                                      int littleEndian, char* out)
{
    // A unit is ASCII when its value is below 0x80. Read as 16-bit
    // little-endian lanes, a little-endian unit is the lane itself, so any bit
    // of 0xFF80 set means "not ASCII"; a big-endian unit has its bytes swapped
    // in the lane, so the bits to test are 0x80FF.
    const __m256i notAscii = _mm256_set1_epi16(
        static_cast<short>(littleEndian != 0 ? 0xFF80 : 0x80FF));
    std::size_t i = 0;
    for (; i + kUnitsPerStep <= units; i += kUnitsPerStep) {
        __m256i low = load(in + 2 * i);
        __m256i high = load(in + 2 * i + 32);
        if (_mm256_testz_si256(_mm256_or_si256(low, high), notAscii) == 0) {
            break; // this block holds a non-ASCII unit: found below, one at a time
        }
        if (littleEndian == 0) {
            low = _mm256_srli_epi16(low, 8);
            high = _mm256_srli_epi16(high, 8);
        }
        // Every lane is now 0-0x7F, so the saturating pack is an exact
        // narrowing. It packs within 128-bit halves - [low 0-7, high 0-7 |
        // low 8-15, high 8-15] - and the permute (0xD8: quarters 0, 2, 1, 3)
        // puts the 32 bytes back in order.
        const __m256i packed = _mm256_permute4x64_epi64(_mm256_packus_epi16(low, high), 0xD8);
        _mm256_storeu_si256(reinterpret_cast<__m256i*>(out + i), packed);
    }
    // The tail, or the block that held a non-ASCII unit up to that unit.
    for (; i < units; ++i) {
        const unsigned first = in[2 * i];
        const unsigned second = in[2 * i + 1];
        const unsigned unit = littleEndian != 0 ? (first | (second << 8)) : ((first << 8) | second);
        if (unit >= 0x80) {
            break;
        }
        out[i] = static_cast<char>(unit);
    }
    return i;
}

extern "C" int katana_avx2_valid_utf8(const unsigned char* bytes, std::size_t size)
{
    if (size < 64) {
        return -1; // the last block would begin before the text
    }
    Utf8Checker checker;
    std::size_t i = 0;
    for (; i + 64 <= size; i += 64) {
        const __m256i first = load(bytes + i);
        const __m256i second = load(bytes + i + 32);
        if (_mm256_movemask_epi8(_mm256_or_si256(first, second)) == 0) {
            checker.ascii(second);
            continue;
        }
        checker.check(first);
        checker.check(second);
        // Once wrong, always wrong: stop, as the byte loop does, rather than
        // read the rest of a file that is not UTF-8 at all.
        if (checker.failed()) {
            return 0;
        }
    }
    if (i + 32 <= size) {
        const __m256i block = load(bytes + i);
        if (_mm256_movemask_epi8(block) == 0) {
            checker.ascii(block);
        } else {
            checker.check(block);
        }
        i += 32;
    }
    if (i < size) {
        checker.last(load(bytes + size - 32), load(bytes + size - 64));
    }
    return checker.valid() ? 1 : 0;
}
