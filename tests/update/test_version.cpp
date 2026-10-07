// PLACEHOLDER from the update skeleton. Task T4 writes this file, against
// include/katana/update/version.hpp. It must cover, each expectation worked out outside
// this program (an RFC, `openssl`, Python's zipfile, a hand calculation) and never read
// off the code under test (CLAUDE.md, section 4):
//
//   - MAJOR.MINOR to MAJOR.MINOR.PATCH.BUILD; "0.10" above "0.9"; "0.3" equals "0.3.0"
//   - every refusal: empty, sign, space, empty component, five components, ten digits, a suffix
//   - the order, by component, and the text back as written
//
// Skipped, not passed: the file is in the suite so that task T4 only fills it, and
// a skipped case is visible in every run until it does.

#include <gtest/gtest.h>

#include "katana/update/version.hpp"

TEST(Version, ParsingAndOrderAreTestedByTaskT4)
{
    GTEST_SKIP() << "placeholder: task T4 replaces this file";
}
