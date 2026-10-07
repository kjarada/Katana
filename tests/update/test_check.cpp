// PLACEHOLDER from the update skeleton. Task T8 writes this file, against
// include/katana/update/check.hpp. It must cover, each expectation worked out outside
// this program (an RFC, `openssl`, Python's zipfile, a hand calculation) and never read
// off the code under test (CLAUDE.md, section 4):
//
//   - a fake Fetch answering: available, current, 403, 404, 429, a portal's HTML, truncation,
//     a bad signature
//   - the signature is fetched first and gates the manifest; the token goes only to api.github.com
//   - a downgrade, a replay, another product or target are not offered; a skipped or blocked
//     release; the SAME version on a fresh install is Current (isNewerThan, test_contract.cpp)
//   - the private-repository route: a 404 on the signature with a token and no feedBase retries
//     once on the Api route (release lookup, then the two assets by id with the right Accept)
//     and reports route == Api; a 404 without a token, or with a feedBase, does not retry;
//     the token is on every Api request and on no Download one
//   - every whole-body request carries a non-zero maxBytes (kMaxSignatureBytes,
//     kMaxManifestBytes, kMaxReleaseJsonBytes); an over-long body is refused, not truncated
//
// Skipped, not passed: the file is in the suite so that task T8 only fills it, and
// a skipped case is visible in every run until it does.

#include <gtest/gtest.h>

#include "katana/update/check.hpp"

TEST(Check, CheckIsTestedByTaskT8)
{
    GTEST_SKIP() << "placeholder: task T8 replaces this file";
}
