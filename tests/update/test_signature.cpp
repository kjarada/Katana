// PLACEHOLDER from the update skeleton. Task T5 writes this file, against
// include/katana/update/signature.hpp. It must cover, each expectation worked out outside
// this program (an RFC, `openssl`, Python's zipfile, a hand calculation) and never read
// off the code under test (CLAUDE.md, section 4):
//
//   - RFC 8032, section 7.1 vectors; a manifest signed by `openssl pkeyutl` (an independent oracle)
//   - a flipped bit in the message, in the signature; the wrong key; S not below the group order
//   - the all-zero key and signature that OpenSSL 3.6.5 accepted (signature.hpp says so)
//   - the signature line: well-formed, wrong algorithm, bad base64, wrong length, two lines
//
// Skipped, not passed: the file is in the suite so that task T5 only fills it, and
// a skipped case is visible in every run until it does.

#include <gtest/gtest.h>

#include "katana/update/signature.hpp"

TEST(Signature, Ed25519AgainstRfc8032IsTestedByTaskT5)
{
    GTEST_SKIP() << "placeholder: task T5 replaces this file";
}
