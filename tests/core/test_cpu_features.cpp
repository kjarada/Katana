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
    ASSERT_TRUE(chooseSimdLevel("scalar", SimdLevel::Avx2));
    EXPECT_EQ(*chooseSimdLevel("scalar", SimdLevel::Avx2), SimdLevel::Scalar);
    ASSERT_TRUE(chooseSimdLevel("scalar", SimdLevel::Scalar));
    EXPECT_EQ(*chooseSimdLevel("scalar", SimdLevel::Scalar), SimdLevel::Scalar);
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

TEST(SimdLevel, AWordThatNamesNoLevelIsRefused)
{
    for (const char* word : {"AVX2", "sse2", "avx512", "fast", " scalar"}) {
        const auto chosen = chooseSimdLevel(word, SimdLevel::Avx2);
        ASSERT_FALSE(chosen) << word;
        EXPECT_EQ(chosen.error().code, ErrorCode::InvalidArgument) << word;
    }
}

TEST(SimdLevel, TheLevelInForceIsNeverOneTheProcessorCannotRun)
{
    EXPECT_LE(katana::core::activeSimdLevel(), katana::core::detectedSimdLevel());
    EXPECT_LE(katana::core::simdSelection().active, katana::core::simdSelection().detected);
}

// Run by ctest both without KATANA_SIMD and with it set (simd_scalar.core and
// simd_avx2.core), so the variable is proved to reach the selection.
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
