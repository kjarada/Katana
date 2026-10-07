// PLACEHOLDER from the update skeleton. Task T7 writes this file, against
// include/katana/update/fault_fs.hpp. It must cover, each expectation worked out outside
// this program (an RFC, `openssl`, Python's zipfile, a hand calculation) and never read
// off the code under test (CLAUDE.md, section 4):
//
//   - crash before the k-th operation of a 40-file apply, for every k; recover; the tree hash
//     is v1 or v2
//   - a second loop crashing inside recovery; a third injecting error 32 at the k-th rename
//     (must roll back)
//   - each loop shown to reach the cases it claims: the k-th crash happens, and the counts add up
//
// Skipped, not passed: the file is in the suite so that task T7 only fills it, and
// a skipped case is visible in every run until it does.

#include <gtest/gtest.h>

#include "katana/update/fault_fs.hpp"

TEST(FaultLoops, TheThreeCrashLoopsAreRunByTaskT7)
{
    GTEST_SKIP() << "placeholder: task T7 replaces this file";
}
