// PLACEHOLDER from the update skeleton. Task T8 writes this file, against
// include/katana/update/stage.hpp. It must cover, each expectation worked out outside
// this program (an RFC, `openssl`, Python's zipfile, a hand calculation) and never read
// off the code under test (CLAUDE.md, section 4):
//
//   - ranges fetched into the stage folder, each member inflated and hashed; a re-run skips
//     verified files
//   - a bad member hash leaves nothing unverified; a server that ignores Range is refused by
//     maxBytes
//   - free space checked first; cancellation; preflight copies the helper and runs --selftest
//   - manifest.json and manifest.json.sig are written into the stage folder LAST, through a
//     temporary name and a rename: absent after a stage that failed or was cancelled, byte for
//     byte the signed manifest and its signature after one that finished
//
// Skipped, not passed: the file is in the suite so that task T8 only fills it, and
// a skipped case is visible in every run until it does.

#include <gtest/gtest.h>

#include "katana/update/stage.hpp"

TEST(Stage, StagingAndPreflightAreTestedByTaskT8)
{
    GTEST_SKIP() << "placeholder: task T8 replaces this file";
}
