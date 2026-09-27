// NEON text kernels, for 64-bit ARM. Compiled by katana_add_simd_sources into
// an object of their own; NEON is the AArch64 baseline, so no -m flag.
//
// A KERNEL FILE, held to the rules of text_avx2.cpp: raw pointers in, raw
// pointers out, nothing with external linkage but the katana_neon_ entries,
// no includes but <arm_neon.h>, <cstddef>, <cstdint> and the declarations.
// tools/check_simd_kernels.cmake enforces them on the source and the object.
//
// The algorithms are text_avx2.cpp's, 16 bytes a register instead of 32, and
// they walk the text in the same steps - 32 UTF-16 units, 64 bytes of UTF-8 -
// so the block edges the tests put their cases on are this kernel's edges too.

#include <arm_neon.h>

#include <cstddef>
#include <cstdint>

#include "text_kernels.hpp"

namespace {

// 32 code units, 64 bytes: two de-interleaving loads of 16 units each.
constexpr std::size_t kUnitsPerStep = 32;

// Whether any byte of v is non-zero: the across-vector maximum, one
// instruction on AArch64.
inline bool any(uint8x16_t v) { return vmaxvq_u8(v) != 0; }

// ---- UTF-8 validation ------------------------------------------------------
//
// The "lookup" algorithm of J. Keiser and D. Lemire, "Validating UTF-8 in less
// than one instruction per byte", Software: Practice and Experience 51(5),
// 2021, as text_avx2.cpp has it: every byte judged against the three before
// it, with no branch on the data. The error bits and the three tables are
// text_avx2.cpp's; TBL looks a byte up in a 16-byte table as VPSHUFB does in
// each 128-bit half, so the tables are the same 16 bytes.
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
constexpr std::uint8_t kCarry = kTooShort | kTooLong | kTwoConts;
constexpr std::uint8_t kLarge = kTooLarge | kTooLarge1000;

// Each byte's N-th predecessor: the last N bytes of `prior`, then the first
// 16 - N of `input`.
template <int N> inline uint8x16_t previous(uint8x16_t input, uint8x16_t prior)
{
    return vextq_u8(prior, input, 16 - N);
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
        const std::uint8_t openAtEnd[16] = {255, 255, 255, 255, 255, 255, 255,      255,
                                            255, 255, 255, 255, 255, 0xF0 - 1, 0xE0 - 1, 0xC0 - 1};
        firstHigh_ = vld1q_u8(firstHigh);
        firstLow_ = vld1q_u8(firstLow);
        secondHigh_ = vld1q_u8(secondHigh);
        openAtEnd_ = vld1q_u8(openAtEnd);
    }

    // Judges the 16 bytes that follow the last ones judged, or begin the text.
    void check(uint8x16_t input)
    {
        const uint8x16_t prev1 = previous<1>(input, prior_);
        // A shift right by 4 of a byte is its high nibble; TBL gives 0 for an
        // index past 15, which no nibble is.
        const uint8x16_t pairErrors =
            vandq_u8(vandq_u8(vqtbl1q_u8(firstHigh_, vshrq_n_u8(prev1, 4)),
                              vqtbl1q_u8(firstLow_, vandq_u8(prev1, vdupq_n_u8(0x0F)))),
                     vqtbl1q_u8(secondHigh_, vshrq_n_u8(input, 4)));
        // Two continuations in a row are right exactly where a three-byte lead
        // is two back or a four-byte lead three back. Less 0x60, only E0-FF
        // reach 0x80; less 0x70, only F0-FF do.
        const uint8x16_t third = vqsubq_u8(previous<2>(input, prior_), vdupq_n_u8(0xE0 - 0x80));
        const uint8x16_t fourth = vqsubq_u8(previous<3>(input, prior_), vdupq_n_u8(0xF0 - 0x80));
        const uint8x16_t wanted = vandq_u8(vorrq_u8(third, fourth), vdupq_n_u8(0x80));
        errors_ = vorrq_u8(errors_, veorq_u8(wanted, pairErrors));
        open_ = vqsubq_u8(input, openAtEnd_);
        prior_ = input;
    }

    // 16 bytes of ASCII after the last judged: a character left open before
    // them is cut short.
    void ascii(uint8x16_t input)
    {
        errors_ = vorrq_u8(errors_, open_);
        open_ = vdupq_n_u8(0);
        prior_ = input;
    }

    // Judges the text's last 16 bytes, `prior` being the 16 before them.
    // Some were judged already; judging a byte twice gives the same answer,
    // and it lets the text end without a partial block.
    void last(uint8x16_t input, uint8x16_t prior)
    {
        prior_ = prior;
        check(input);
    }

    [[nodiscard]] bool failed() const { return any(errors_); }

    // At the end of the text: no error, and no character left open.
    [[nodiscard]] bool valid() const { return !any(vorrq_u8(errors_, open_)); }

  private:
    uint8x16_t firstHigh_;
    uint8x16_t firstLow_;
    uint8x16_t secondHigh_;
    uint8x16_t openAtEnd_;
    uint8x16_t errors_ = vdupq_n_u8(0);
    uint8x16_t prior_ = vdupq_n_u8(0); // before the text, as if ASCII
    uint8x16_t open_ = vdupq_n_u8(0);
};

// ASCII is the bytes below 0x80: the maximum of the block says so.
inline bool isAscii(uint8x16_t block) { return vmaxvq_u8(block) < 0x80; }

// A 16-byte block, judged or passed over as ASCII.
inline void judge(Utf8Checker& checker, uint8x16_t block)
{
    if (isAscii(block)) {
        checker.ascii(block);
    } else {
        checker.check(block);
    }
}

} // namespace

extern "C" std::size_t katana_neon_narrow_ascii_utf16(const unsigned char* in, std::size_t units,
                                                      int littleEndian, char* out)
{
    std::size_t i = 0;
    for (; i + kUnitsPerStep <= units; i += kUnitsPerStep) {
        // LD2 de-interleaves: val[0] holds the first byte of each unit, val[1]
        // the second. The byte order decides which is the high one.
        const uint8x16x2_t a = vld2q_u8(in + 2 * i);
        const uint8x16x2_t b = vld2q_u8(in + 2 * i + 32);
        const int lowIndex = littleEndian != 0 ? 0 : 1;
        const uint8x16_t lowA = a.val[lowIndex];
        const uint8x16_t lowB = b.val[lowIndex];
        const uint8x16_t highA = a.val[1 - lowIndex];
        const uint8x16_t highB = b.val[1 - lowIndex];
        // A unit is ASCII when its high byte is 0 and its low byte below 0x80.
        const uint8x16_t top = vdupq_n_u8(0x80);
        const uint8x16_t notAscii = vorrq_u8(vorrq_u8(highA, highB),
                                             vandq_u8(vorrq_u8(lowA, lowB), top));
        if (any(notAscii)) {
            break; // this block holds a non-ASCII unit: found below, one at a time
        }
        vst1q_u8(reinterpret_cast<std::uint8_t*>(out + i), lowA);
        vst1q_u8(reinterpret_cast<std::uint8_t*>(out + i + 16), lowB);
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

extern "C" int katana_neon_valid_utf8(const unsigned char* bytes, std::size_t size)
{
    if (size < 64) {
        return -1; // the contract text_avx2.cpp set, and kValidateMinimum keeps
    }
    Utf8Checker checker;
    std::size_t i = 0;
    for (; i + 64 <= size; i += 64) {
        const uint8x16_t b0 = vld1q_u8(bytes + i);
        const uint8x16_t b1 = vld1q_u8(bytes + i + 16);
        const uint8x16_t b2 = vld1q_u8(bytes + i + 32);
        const uint8x16_t b3 = vld1q_u8(bytes + i + 48);
        if (isAscii(vorrq_u8(vorrq_u8(b0, b1), vorrq_u8(b2, b3)))) {
            checker.ascii(b3);
            continue;
        }
        checker.check(b0);
        checker.check(b1);
        checker.check(b2);
        checker.check(b3);
        // Once wrong, always wrong: stop, as the byte loop does, rather than
        // read the rest of a file that is not UTF-8 at all.
        if (checker.failed()) {
            return 0;
        }
    }
    for (; i + 16 <= size; i += 16) {
        judge(checker, vld1q_u8(bytes + i));
    }
    if (i < size) {
        checker.last(vld1q_u8(bytes + size - 16), vld1q_u8(bytes + size - 32));
    }
    return checker.valid() ? 1 : 0;
}
