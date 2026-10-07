// PLACEHOLDER from the update skeleton. Task T6 writes this file, against
// include/katana/update/ranges.hpp. It must cover, each expectation worked out outside
// this program (an RFC, `openssl`, Python's zipfile, a hand calculation) and never read
// off the code under test (CLAUDE.md, section 4):
//
//   - adjacent members merge; a gap of exactly the limit merges and one byte more does not
//   - no range exceeds the cap; a member over the cap is a range of its own
//   - overlap, zero length, unsorted input; every member is in exactly one range
//
// Skipped, not passed: the file is in the suite so that task T6 only fills it, and
// a skipped case is visible in every run until it does.

#include <gtest/gtest.h>

#include "katana/update/ranges.hpp"

TEST(Ranges, TheRangeMergerIsTestedByTaskT6)
{
    GTEST_SKIP() << "placeholder: task T6 replaces this file";
}
