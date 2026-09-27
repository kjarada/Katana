#include <gtest/gtest.h>

#include <cstdlib>
#include <string>

#include "katana/core/cpu_features.hpp"

using katana::core::chooseSimdLevel;
using katana::core::ErrorCode;
using katana::core::SimdLevel;

TEST(SimdLevel, AnEmptyRequestKeepsTheDetectedLevel)
{
    ASSERT_TRUE(chooseSimdLevel("", SimdLevel::Avx2));
    EXPECT_EQ(*chooseSimdLevel("", SimdLevel::Avx2), SimdLevel::Avx2);
    ASSERT_TRUE(chooseSimdLevel("", SimdLevel::Scalar));
    EXPECT_EQ(*chooseSimdLevel("", SimdLevel::Scalar), SimdLevel::Scalar);
}

TEST(SimdLevel, ScalarCanBeChosenOnAnyProcessor)
{
    for (const SimdLevel detected : {SimdLevel::Scalar, SimdLevel::Avx2, SimdLevel::Neon}) {
        const auto chosen = chooseSimdLevel("scalar", detected);
        ASSERT_TRUE(chosen) << katana::core::toString(detected);
        EXPECT_EQ(*chosen, SimdLevel::Scalar);
    }
}

TEST(SimdLevel, Avx2IsChosenWhereTheProcessorRunsIt)
{
    ASSERT_TRUE(chooseSimdLevel("avx2", SimdLevel::Avx2));
    EXPECT_EQ(*chooseSimdLevel("avx2", SimdLevel::Avx2), SimdLevel::Avx2);
}

TEST(SimdLevel, Avx2IsRefusedOnAProcessorWithoutItRatherThanFaultingOrRunningScalarQuietly)
{
    const auto chosen = chooseSimdLevel("avx2", SimdLevel::Scalar);
    ASSERT_FALSE(chosen);
    EXPECT_EQ(chosen.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(chosen.error().message.find("avx2"), std::string::npos) << chosen.error().message;
}

TEST(SimdLevel, NeonIsChosenWhereTheProcessorRunsIt)
{
    ASSERT_TRUE(chooseSimdLevel("neon", SimdLevel::Neon));
    EXPECT_EQ(*chooseSimdLevel("neon", SimdLevel::Neon), SimdLevel::Neon);
    ASSERT_TRUE(chooseSimdLevel("", SimdLevel::Neon));
    EXPECT_EQ(*chooseSimdLevel("", SimdLevel::Neon), SimdLevel::Neon);
}

TEST(SimdLevel, NeonIsRefusedOnAProcessorWithoutIt)
{
    // An x86-64 processor, with AVX2 or without: NEON is not its to run.
    for (const SimdLevel detected : {SimdLevel::Scalar, SimdLevel::Avx2}) {
        const auto chosen = chooseSimdLevel("neon", detected);
        ASSERT_FALSE(chosen) << katana::core::toString(detected);
        EXPECT_EQ(chosen.error().code, ErrorCode::InvalidArgument);
        EXPECT_NE(chosen.error().message.find("neon"), std::string::npos) << chosen.error().message;
    }
}

TEST(SimdLevel, Avx2IsRefusedOnANeonProcessorAlthoughItComesFirstInTheEnumeration)
{
    // The kernel levels belong to different architectures: an ARM processor
    // that runs NEON cannot run AVX2, whatever order the enumeration lists
    // them in. (Refused by `level > detected`, it would have been allowed.)
    const auto chosen = chooseSimdLevel("avx2", SimdLevel::Neon);
    ASSERT_FALSE(chosen);
    EXPECT_EQ(chosen.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(chosen.error().message.find("avx2"), std::string::npos) << chosen.error().message;
}

TEST(SimdLevel, AWordThatNamesNoLevelIsRefusedNamingTheWordsThatDo)
{
    for (const char* word : {"AVX2", "sse2", "avx512", "fast", " scalar", "NEON", "asimd", "neon "}) {
        for (const SimdLevel detected : {SimdLevel::Avx2, SimdLevel::Neon}) {
            const auto chosen = chooseSimdLevel(word, detected);
            ASSERT_FALSE(chosen) << word;
            EXPECT_EQ(chosen.error().code, ErrorCode::InvalidArgument) << word;
            EXPECT_NE(chosen.error().message.find("use scalar, avx2 or neon"), std::string::npos)
                << chosen.error().message;
        }
    }
}

TEST(SimdLevel, TheLevelInForceIsNeverOneTheProcessorCannotRun)
{
    // Scalar, or exactly the detected level: never ordered, because two
    // kernel levels are never both runnable.
    const SimdLevel detected = katana::core::detectedSimdLevel();
    const SimdLevel active = katana::core::activeSimdLevel();
    EXPECT_TRUE(active == SimdLevel::Scalar || active == detected) << katana::core::toString(active);
    const auto& selection = katana::core::simdSelection();
    EXPECT_TRUE(selection.active == SimdLevel::Scalar || selection.active == selection.detected);
}

TEST(SimdLevel, TheDetectedLevelBelongsToThisProcessorsArchitecture)
{
#if defined(__aarch64__) || defined(_M_ARM64)
    // NEON is in every AArch64 processor, so no probe: a build with the NEON
    // kernels detects Neon, and one without (KATANA_SIMD_KERNELS=OFF) Scalar.
    EXPECT_NE(katana::core::detectedSimdLevel(), SimdLevel::Avx2);
#if defined(KATANA_TEST_SIMD_KERNEL_SET_NEON)
    EXPECT_EQ(katana::core::detectedSimdLevel(), SimdLevel::Neon);
#endif
#else
    EXPECT_NE(katana::core::detectedSimdLevel(), SimdLevel::Neon);
#endif
}

// Run by ctest both without KATANA_SIMD and with it set (simd_scalar.core and
// simd_avx2.core or simd_neon.core), so the variable is proved to reach the
// selection.
TEST(SimdLevel, TheEnvironmentOverrideDecidesTheStartingLevel)
{
    const char* requested = std::getenv("KATANA_SIMD");
    const std::string text = requested == nullptr ? "" : requested;
    const auto& selection = katana::core::simdSelection();
    const auto expected = chooseSimdLevel(text, selection.detected);
    if (expected) {
        EXPECT_EQ(selection.active, *expected);
        EXPECT_EQ(selection.note.empty(), *expected == selection.detected) << selection.note;
    } else {
        EXPECT_EQ(selection.active, selection.detected);
        EXPECT_NE(selection.note.find("ignored"), std::string::npos) << selection.note;
    }
}

TEST(SimdLevel, SettingALevelReturnsThePreviousOneSoItCanBeRestored)
{
    const SimdLevel before = katana::core::activeSimdLevel();
    const auto previous = katana::core::setSimdLevel(SimdLevel::Scalar);
    ASSERT_TRUE(previous);
    EXPECT_EQ(*previous, before);
    EXPECT_EQ(katana::core::activeSimdLevel(), SimdLevel::Scalar);
    ASSERT_TRUE(katana::core::setSimdLevel(before));
    EXPECT_EQ(katana::core::activeSimdLevel(), before);
}

TEST(SimdLevel, LevelsHaveTheNamesTheOverrideAccepts)
{
    EXPECT_STREQ(katana::core::toString(SimdLevel::Scalar), "scalar");
    EXPECT_STREQ(katana::core::toString(SimdLevel::Avx2), "avx2");
    EXPECT_STREQ(katana::core::toString(SimdLevel::Neon), "neon");
    // And each name is accepted back as its level.
    for (const SimdLevel level : {SimdLevel::Scalar, SimdLevel::Avx2, SimdLevel::Neon}) {
        const auto chosen = chooseSimdLevel(katana::core::toString(level), level);
        ASSERT_TRUE(chosen) << katana::core::toString(level);
        EXPECT_EQ(*chosen, level);
    }
}

TEST(SimdLevel, OrdinaryCodeIsCompiledForTheBaselineSoTheProgramStartsOnAnyX64Machine)
{
    // Only *_avx2.cpp kernel files are compiled for AVX2, and nothing in them
    // runs before activeSimdLevel() allows it. If this file - compiled with the
    // flags every ordinary file gets - had AVX enabled, the program could
    // execute AVX instructions on a machine without them before any dispatch.
#if defined(__AVX__) || defined(__AVX2__)
    ADD_FAILURE() << "an ordinary source file was compiled with AVX enabled";
#endif
    SUCCEED();
}
