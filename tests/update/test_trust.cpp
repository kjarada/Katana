// PLACEHOLDER from the update skeleton. Task T5 writes this file, against
// include/katana/update/trust.hpp. It must cover, each expectation worked out outside
// this program (an RFC, `openssl`, Python's zipfile, a hand calculation) and never read
// off the code under test (CLAUDE.md, section 4):
//
//   - verifyManifest in the order trust.hpp lists; the spare key verifies; an unknown key id
//   - a placeholder Trust refuses to verify; wrong product, target and channel
//   - isNewerThan: a higher version, the same version with a higher sequence, neither
//
// Skipped, not passed: the file is in the suite so that task T5 only fills it, and
// a skipped case is visible in every run until it does.

#include <gtest/gtest.h>

#include "katana/update/trust.hpp"

TEST(Trust, TheGateAndThePinnedKeysAreTestedByTaskT5)
{
    GTEST_SKIP() << "placeholder: task T5 replaces this file";
}
