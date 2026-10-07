// PLACEHOLDER from the update skeleton. Task T6 writes this file, against
// include/katana/update/inflate.hpp. It must cover, each expectation worked out outside
// this program (an RFC, `openssl`, Python's zipfile, a hand calculation) and never read
// off the code under test (CLAUDE.md, section 4):
//
//   - members made by Python zipfile (an independent oracle) inflate to their bytes
//   - a truncated stream, garbage, a stream longer than the signed size, one shorter,
//     trailing bytes
//   - the sink failing part way
//
// Skipped, not passed: the file is in the suite so that task T6 only fills it, and
// a skipped case is visible in every run until it does.

#include <gtest/gtest.h>

#include "katana/update/inflate.hpp"

TEST(Inflate, RawDeflateAndItsCapAreTestedByTaskT6)
{
    GTEST_SKIP() << "placeholder: task T6 replaces this file";
}
