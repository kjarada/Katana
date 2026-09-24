#include "katana/core/cpu_features.hpp"

#include <atomic>
#include <cstdio>
#include <cstdlib>

namespace katana::core {

namespace {

SimdLevel probeProcessor()
{
#if defined(__GNUC__) && (defined(__x86_64__) || defined(__i386__))
    // Called explicitly because this may run from a static initialiser, before
    // libgcc's own constructor has filled in the model it reads.
    __builtin_cpu_init();
    // libgcc reports avx2 only when XGETBV says the OS saves the YMM state, so
    // this answers "may this process execute AVX2", not merely "does the
    // silicon have it".
    if (__builtin_cpu_supports("avx2") && __builtin_cpu_supports("fma")) {
        return SimdLevel::Avx2;
    }
#endif
    return SimdLevel::Scalar;
}

// Whether the kernels for `level` were compiled into this build at all. A build
// for another architecture, or one configured with KATANA_SIMD_KERNELS=OFF,
// has only the scalar references; detecting AVX2 there must not route a call to
// a kernel that does not exist.
constexpr bool built(SimdLevel level)
{
    switch (level) {
    case SimdLevel::Scalar:
        return true;
    case SimdLevel::Avx2:
#if defined(KATANA_HAVE_AVX2_KERNELS)
        return true;
#else
        return false;
#endif
    }
    return false;
}

SimdSelection decide()
{
    SimdSelection selection;
    const SimdLevel processor = probeProcessor();
    selection.detected = built(processor) ? processor : SimdLevel::Scalar;
    selection.active = selection.detected;

    const char* requested = std::getenv("KATANA_SIMD");
    if (requested == nullptr || *requested == '\0') {
        return selection;
    }
    const auto chosen = chooseSimdLevel(requested, selection.detected);
    if (chosen) {
        selection.active = *chosen;
        if (*chosen != selection.detected) {
            selection.note = std::string("KATANA_SIMD=") + requested +
                             " in force; the processor supports " + toString(selection.detected);
        }
        return selection;
    }
    selection.note = "KATANA_SIMD ignored: " + chosen.error().message;
    // Once per process, and only for a refused override: the person who set
    // the variable is the one reading stderr.
    std::fprintf(stderr, "katana: %s\n", selection.note.c_str());
    return selection;
}

const SimdSelection& selectionOnce()
{
    static const SimdSelection selection = decide();
    return selection;
}

std::atomic<SimdLevel>& activeLevel()
{
    static std::atomic<SimdLevel> level{selectionOnce().active};
    return level;
}

} // namespace

const char* toString(SimdLevel level)
{
    switch (level) {
    case SimdLevel::Scalar:
        return "scalar";
    case SimdLevel::Avx2:
        return "avx2";
    }
    return "unknown";
}

SimdLevel detectedSimdLevel() { return selectionOnce().detected; }

Result<SimdLevel> chooseSimdLevel(std::string_view requested, SimdLevel detected)
{
    if (requested.empty()) {
        return detected;
    }
    SimdLevel level = SimdLevel::Scalar;
    if (requested == "scalar") {
        level = SimdLevel::Scalar;
    } else if (requested == "avx2") {
        level = SimdLevel::Avx2;
    } else {
        return makeError(ErrorCode::InvalidArgument,
                         "'" + std::string(requested) +
                             "' is not a SIMD level; use scalar or avx2");
    }
    if (level > detected) {
        return makeError(ErrorCode::InvalidArgument,
                         std::string(toString(level)) +
                             " was asked for but this processor supports only " +
                             toString(detected));
    }
    return level;
}

const SimdSelection& simdSelection() { return selectionOnce(); }

SimdLevel activeSimdLevel() { return activeLevel().load(std::memory_order_relaxed); }

Result<SimdLevel> setSimdLevel(SimdLevel level)
{
    // The same rule, and the same words, as the environment override.
    const auto allowed = chooseSimdLevel(toString(level), detectedSimdLevel());
    if (!allowed) {
        return allowed.error();
    }
    return activeLevel().exchange(*allowed, std::memory_order_relaxed);
}

} // namespace katana::core
