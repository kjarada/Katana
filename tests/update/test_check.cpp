// PLACEHOLDER from the update skeleton. Task T8 writes this file, against
// include/katana/update/check.hpp. It must cover, each expectation worked out outside
// this program (an RFC, `openssl`, Python's zipfile, a hand calculation) and never read
// off the code under test (CLAUDE.md, section 4):
//
//   - a fake Fetch answering: available, current, 403, 404, 429, a portal's HTML, truncation,
//     a bad signature
//   - the signature is fetched first and gates the manifest; the token goes only to api.github.com
//   - a downgrade, a replay, another product or target are not offered; a skipped or blocked
//     release
//
// Skipped, not passed: the file is in the suite so that task T8 only fills it, and
// a skipped case is visible in every run until it does.

#include <gtest/gtest.h>

#include "katana/update/check.hpp"

TEST(Check, CheckIsTestedByTaskT8)
{
    GTEST_SKIP() << "placeholder: task T8 replaces this file";
}
