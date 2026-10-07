// PLACEHOLDER from the update skeleton. Task T7 writes this file, against
// include/katana/update/recover.hpp. It must cover, each expectation worked out outside
// this program (an RFC, `openssl`, Python's zipfile, a hand calculation) and never read
// off the code under test (CLAUDE.md, section 4):
//
//   - a journal in each of Planned, Committing and RollingBack is rolled back; HEALTHY and
//     ROLLED_BACK are left alone
//   - a live lock holder is waited for and not touched; nothing to do costs one directory listing
//   - a forged journal (a path that fails checkJournalPath, a folder name that fails
//     isValidTransactionId) makes recovery do less - the record is skipped and reported - and
//     touches nothing outside bin, share, etc and lib of the root
//   - a recovery that cannot finish returns an Error naming `katana_updater --recover <R>`
//
// Skipped, not passed: the file is in the suite so that task T7 only fills it, and
// a skipped case is visible in every run until it does.

#include <gtest/gtest.h>

#include "katana/update/recover.hpp"

TEST(Recover, RecoveryIsTestedByTaskT7)
{
    GTEST_SKIP() << "placeholder: task T7 replaces this file";
}
