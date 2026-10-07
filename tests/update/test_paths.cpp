// PLACEHOLDER from the update skeleton. Task T4 writes this file, against
// include/katana/update/paths.hpp. It must cover, each expectation worked out outside
// this program (an RFC, `openssl`, Python's zipfile, a hand calculation) and never read
// off the code under test (CLAUDE.md, section 4):
//
//   - each rule of paths.hpp with a name that breaks only that rule, and the plain names that pass
//   - device names with and without an extension, 8.3 names, trailing dot and space,
//     case-folded duplicates
//   - the file and byte caps, and the reserved names wherever they sit
//
// Skipped, not passed: the file is in the suite so that task T4 only fills it, and
// a skipped case is visible in every run until it does.

#include <gtest/gtest.h>

#include "katana/update/paths.hpp"

TEST(Paths, EveryRuleOfTheNameCheckIsTestedByTaskT4)
{
    GTEST_SKIP() << "placeholder: task T4 replaces this file";
}
