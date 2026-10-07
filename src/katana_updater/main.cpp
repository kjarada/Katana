// STUB. Task T9 writes the helper program: parse the command line
// (update::parseHelperArguments), clear OPENSSL_CONF, OPENSSL_MODULES and
// OPENSSL_ENGINES, then --selftest, --recover <R> or the apply phases of
// include/katana/update/apply.hpp, and the health watch.
//
// THE TRUST SEAM, which is the only thing this stub fixes. The helper takes the
// keys it verifies against from trustForThisBuild() and from nowhere else:
//   - katana_updater (installed) passes productionTrust(), the keys pinned at
//     build time.
//   - katana_updater_test (tests/update/CMakeLists.txt, never installed) is
//     these same sources compiled with KATANA_UPDATER_TEST_BUILD, a test key
//     passed in place of the pinned ones, and --die-at N (a FaultFs, T9).
// So no test key exists in a shipped helper, and nothing in the environment
// moves the trust root.
//
// The program is static (-static on Windows, src/katana_updater/CMakeLists.txt)
// and never runs from bin/: the engine copies it to <U>/run-<tx>/ first
// (stage.hpp, preflight), so at commit nothing under the install root is
// loaded.

#include <iostream>

#include "katana/update/trust.hpp"

#ifdef KATANA_UPDATER_TEST_BUILD
namespace katana::update {
// Defined by tests/update/test_key.cpp, which only the test executables link.
const Trust& testTrust();
} // namespace katana::update
#endif

namespace {

[[maybe_unused]] const katana::update::Trust& trustForThisBuild()
{
#ifdef KATANA_UPDATER_TEST_BUILD
    return katana::update::testTrust();
#else
    return katana::update::productionTrust();
#endif
}

} // namespace

int main()
{
    std::cerr << "katana_updater: not implemented (update skeleton; task T9)\n";
    return 2;
}
