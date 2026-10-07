// PLACEHOLDER from the update skeleton. Task T2 writes this file: the NIST vectors
// ("abc", the empty message, the 448-bit message, one million 'a'), the same digest
// from every split of a message across update() calls, finishHex and reset, and
// sha256FileHex on files of 0, 1, 63, 64, 65 bytes and several blocks. Every
// expectation is worked out outside this program (FIPS 180-4's own examples,
// `openssl dgst -sha256`), never read off the code under test.
//
// Skipped, not passed: a test that says nothing is registered so that the file is
// in the suite when T2 arrives, and so that a skipped case is visible in the run.

#include <gtest/gtest.h>

#include "katana/core/sha256.hpp"

TEST(Sha256, TheStreamingHashAndItsVectorsAreWrittenByTaskT2)
{
    GTEST_SKIP() << "core/sha256.cpp is a stub until task T2 replaces it";
}
