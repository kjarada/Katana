// PLACEHOLDER from the update skeleton. Task T7 writes this file, against
// include/katana/update/apply.hpp. It must cover, each expectation worked out outside
// this program (an RFC, `openssl`, Python's zipfile, a hand calculation) and never read
// off the code under test (CLAUDE.md, section 4):
//
//   - a tree of dozens of files updated, and the whole-tree hash equal to the new version's
//   - every refusal of step 1 and 3; a holder under the root (Blocked); a locked file; the
//     health outcomes
//   - what is removed: only a file of the installed manifest, unchanged, absent from the new one
//   - the staged manifest and signature (stagedManifestPath, stagedSignaturePath) are read and
//     verified with the helper's own keys; missing or unverified is Refused "staged_manifest"
//   - a rollback that itself fails (an injected error at the first restoring rename) is
//     RollbackIncomplete with reason "rollback_failed", never RolledBack
//   - parseHelperArguments: a tx that fails isValidTransactionId, a missing, an unknown and a
//     repeated option are refused; helperArguments and parseHelperArguments round-trip a root
//     with a space and a non-ASCII letter in it
//
// Skipped, not passed: the file is in the suite so that task T7 only fills it, and
// a skipped case is visible in every run until it does.

#include <gtest/gtest.h>

#include "katana/update/apply.hpp"

TEST(Apply, TheSwapIsTestedByTaskT7)
{
    GTEST_SKIP() << "placeholder: task T7 replaces this file";
}
