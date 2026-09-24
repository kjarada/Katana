#pragma once

// Which SIMD kernels this process runs: detected once, overridable for tests.
//
// WHY RUNTIME DISPATCH. The shipped program is built for baseline x86-64
// (SSE2), so it starts on any 64-bit PC. A machine with AVX2 gets 4 doubles or
// 32 bytes per instruction from a handful of kernels compiled for it; a machine
// without gets the scalar reference. The choice is made HERE, once, and every
// kernel family asks for it - never by compiling ordinary code with -mavx2,
// which lets the linker hand AVX2 copies of shared inline functions to baseline
// callers (docs/performance.md, "The inline-copy hazard").
//
// WHY THE LEVEL CAN BE CHANGED. Every kernel is bit-identical to its scalar
// reference - that is the contract, and the tests hold each one to it - so the
// level decides only how fast a result arrives, never what it is. That is what
// makes it safe to switch at any moment, and what lets a test run both paths in
// one process and compare them.
//
// The override: the environment variable KATANA_SIMD, read once at the first
// question, set to `scalar` or `avx2`. `scalar` is how a test proves that a
// machine without AVX2 is served correctly on a machine that has it.

#include <string>
#include <string_view>

#include "katana/core/error.hpp"

namespace katana::core {

// Ordered: a higher level can run everything a lower one can.
enum class SimdLevel { Scalar, Avx2 };

[[nodiscard]] const char* toString(SimdLevel level);

// What the processor AND the operating system support. AVX2 is reported only
// with FMA beside it (every processor with one has had the other) and only
// when the OS saves the 256-bit registers across context switches, which the
// compiler's own detection checks; a CPU flag without that would fault.
[[nodiscard]] SimdLevel detectedSimdLevel();

// `requested` is the text of KATANA_SIMD. Empty means "whatever was detected".
// InvalidArgument for a word that names no level, and for a level the
// processor cannot run - asking for AVX2 on a machine without it must fail
// loudly, not crash with an illegal instruction and not quietly run scalar.
[[nodiscard]] Result<SimdLevel> chooseSimdLevel(std::string_view requested, SimdLevel detected);

// How the level in force was arrived at, for a diagnostic line or a log.
struct SimdSelection {
    SimdLevel detected = SimdLevel::Scalar;
    SimdLevel active = SimdLevel::Scalar;
    // Empty when the detected level is in force. Otherwise why not: the
    // override that was applied, or the reason an override was refused.
    std::string note;
};

// Decided once, on the first call from any thread, from detectedSimdLevel()
// and KATANA_SIMD. A refused override leaves the detected level in force and
// says so in `note`, and is written once to stderr - it is a developer's
// switch, and a developer who mistyped it must not believe the scalar path
// was tested when it was not.
[[nodiscard]] const SimdSelection& simdSelection();

// The level kernels dispatch to now. Cheap: one relaxed atomic load after the
// first call.
[[nodiscard]] SimdLevel activeSimdLevel();

// Changes the level for the whole process and returns the previous one.
// InvalidArgument when the processor cannot run `level`. For tests and
// benchmarks: because every level gives bit-identical results, a switch while
// kernels are running on other threads cannot change any result.
[[nodiscard]] Result<SimdLevel> setSimdLevel(SimdLevel level);

} // namespace katana::core
