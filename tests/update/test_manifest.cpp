// PLACEHOLDER from the update skeleton. Task T4 writes this file, against
// include/katana/update/manifest.hpp. It must cover, each expectation worked out outside
// this program (an RFC, `openssl`, Python's zipfile, a hand calculation) and never read
// off the code under test (CLAUDE.md, section 4):
//
//   - the design's sample manifest read field by field; a notify-only one
//   - every missing or mistyped member; schema other than 1; unsorted or duplicate files;
//     a bad hash
//   - unknown members ignored
//
// Skipped, not passed: the file is in the suite so that task T4 only fills it, and
// a skipped case is visible in every run until it does.

#include <gtest/gtest.h>

#include "katana/update/manifest.hpp"

TEST(Manifest, TheReaderAndItsRefusalsAreTestedByTaskT4)
{
    GTEST_SKIP() << "placeholder: task T4 replaces this file";
}
