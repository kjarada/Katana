// PLACEHOLDER from the update skeleton. Task T8 writes this file, against
// include/katana/update/feed.hpp. It must cover, each expectation worked out outside
// this program (an RFC, `openssl`, Python's zipfile, a hand calculation) and never read
// off the code under test (CLAUDE.md, section 4):
//
//   - the defaults (kjarada/Katana, no feed); both empty is no auto-update
//   - parseReleaseAssets: assets[] with name, id and size; an absent assets[] is empty; an asset
//     without a name or an id, and JSON that is not an object, are ParseFailure
//   - (the addresses themselves, the Api route's included, are finished and tested in
//     test_contract.cpp; keep them there)
//
// Skipped, not passed: the file is in the suite so that task T8 only fills it, and
// a skipped case is visible in every run until it does.

#include <gtest/gtest.h>

#include "katana/update/feed.hpp"

TEST(Feed, TheFeedDefaultsAndTheReleaseListAreTestedByTaskT8)
{
    GTEST_SKIP() << "placeholder: task T8 replaces this file";
}
