// STUB. Task T12 writes katana_update_e2e: the engine driven from the command line by
// tests/update/update_e2e.py against tools/update_fake_feed.py - check, plan, stage,
// preflight and launchApply, with the TEST key (test_key.hpp) as the trust root, each
// result printed as key=value. A separate executable from the same sources as the
// engine, never installed (tools/check_install_rules.cmake refuses it), because the
// production programs have no way to be given another key.

#include <iostream>

#include "test_key.hpp"

int main()
{
    // Names the seam T12 uses, so the executable cannot link without it.
    [[maybe_unused]] const katana::update::Trust& trust = katana::update::testTrust();
    std::cerr << "katana_update_e2e: not implemented (update skeleton; task T12)\n";
    return 2;
}
