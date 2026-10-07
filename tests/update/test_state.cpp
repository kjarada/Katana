// PLACEHOLDER from the update skeleton. Task T6 writes this file, against
// include/katana/update/state.hpp. It must cover, each expectation worked out outside
// this program (an RFC, `openssl`, Python's zipfile, a hand calculation) and never read
// off the code under test (CLAUDE.md, section 4):
//
//   - install.json, installed.json and the installed manifest read; none present is a
//     development tree
//   - state.json and last.json round trips; absent, malformed and newer-schema files
//   - the state directory by platform and by KATANA_UPDATE_STATE_DIR
//   - Install::hasInstalledRecord is true exactly when installed.json is present, and a fresh
//     install then reads as (programVersion(), 0); last.json round trip with status
//     "rollback_incomplete"
//   - installRootOfRunningProgram: the running executable's path through
//     installRootOfExecutable (already finished and tested, contract.cpp); the whole state
//     directory contract
//   - appendCleanup and processCleanupList: the list is NOT trusted - a line outside the state
//     directory and <root>/.update is dropped untouched, one that resolves outside through a
//     reparse point or a symlink is not followed, a locked file stays listed, an absent list
//     is OK
//
// Skipped, not passed: the file is in the suite so that task T6 only fills it, and
// a skipped case is visible in every run until it does.

#include <gtest/gtest.h>

#include "katana/update/state.hpp"

TEST(State, TheRecordsAndTheStateFileAreTestedByTaskT6)
{
    GTEST_SKIP() << "placeholder: task T6 replaces this file";
}
