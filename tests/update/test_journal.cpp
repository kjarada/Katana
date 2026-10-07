// PLACEHOLDER from the update skeleton. Task T7 writes this file, against
// include/katana/update/journal.hpp. It must cover, each expectation worked out outside
// this program (an RFC, `openssl`, Python's zipfile, a hand calculation) and never read
// off the code under test (CLAUDE.md, section 4):
//
//   - encode and parse round trips, arguments with spaces and non-ASCII bytes
//   - a torn last line, a wrong crc, a gap in the numbers: the good prefix and tornTail
//   - the state each record leads to; needsRecovery
//   - the journal is not authenticated: Journal readers hand paths on unchecked, so the tests
//     of rollback (test_apply.cpp, test_recover.cpp) forge records that name ../, an absolute
//     path, installed.json spelled in another case and install.json
//
// Skipped, not passed: the file is in the suite so that task T7 only fills it, and
// a skipped case is visible in every run until it does.

#include <gtest/gtest.h>

#include "katana/update/journal.hpp"

TEST(Journal, TheJournalFormatIsTestedByTaskT7)
{
    GTEST_SKIP() << "placeholder: task T7 replaces this file";
}
