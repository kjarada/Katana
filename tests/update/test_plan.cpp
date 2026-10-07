// PLACEHOLDER from the update skeleton. Task T6 writes this file, against
// include/katana/update/plan.hpp. It must cover, each expectation worked out outside
// this program (an RFC, `openssl`, Python's zipfile, a hand calculation) and never read
// off the code under test (CLAUDE.md, section 4):
//
//   - an install equal to the manifest: nothing changed; one file differing; one absent;
//     a seed present and absent
//   - the totals (download bytes are compressed sizes) and the free-space figure
//   - cancellation between files; a root that is not a directory
//
// Skipped, not passed: the file is in the suite so that task T6 only fills it, and
// a skipped case is visible in every run until it does.

#include <gtest/gtest.h>

#include "katana/update/plan.hpp"

TEST(Plan, ThePlannerIsTestedByTaskT6)
{
    GTEST_SKIP() << "placeholder: task T6 replaces this file";
}
