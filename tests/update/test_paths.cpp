// PLACEHOLDER from the update skeleton. Task T4 writes this file, against
// include/katana/update/paths.hpp. It must cover, each expectation worked out outside
// this program (an RFC, `openssl`, Python's zipfile, a hand calculation) and never read
// off the code under test (CLAUDE.md, section 4):
//
//   - each rule of paths.hpp with a name that breaks only that rule, and the plain names that pass
//   - device names with and without an extension, 8.3 names, trailing dot and space,
//     case-folded duplicates
//   - the file and byte caps, and the reserved names wherever they sit AND IN ANY ASCII CASE
//     (share/katana/INSTALL.JSON, Share/Katana/Installed.Manifest.JSON, a .UPDATE component)
//   - ASCII only: a non-ASCII name, a control character and DEL are each refused, the plain
//     ASCII names of the real tree pass (the review that added the rule checked the tree)
//   - checkJournalPath: the manifest rules, plus exactly the three records of kRecordPaths
//     (any case); install.json and every other reserved name are refused there
//   - the shared table: read ${KATANA_UPDATE_FIXTURES}/path_rules.txt (the format is in its
//     header: ok/no, a percent-escaped path, why) and fail on any line checkManifestPath
//     answers differently. tools/update_make_manifest.py and tools/update_verify_release.py are
//     already held to the same file (tests/update/tool_tests), so the engine and the tools agree
//     by construction. The table has no COM0 / LPT0 line: on the Windows 11 the producer's author
//     tested, both create ordinary files, so paths.hpp refuses COM1 to COM9 and LPT1 to LPT9 only
//     unless the owner rules otherwise.
//
// Skipped, not passed: the file is in the suite so that task T4 only fills it, and
// a skipped case is visible in every run until it does.

#include <gtest/gtest.h>

#include "katana/update/paths.hpp"

TEST(Paths, EveryRuleOfTheNameCheckIsTestedByTaskT4)
{
    GTEST_SKIP() << "placeholder: task T4 replaces this file";
}
