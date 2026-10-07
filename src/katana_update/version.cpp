// Task T4 fills in parseVersion and toString. programVersionText is already
// real: it is how the skeleton proves that project(Katana VERSION) reaches this
// library (test_skeleton.cpp), which katana_cli's own build never did.

#include "katana/update/version.hpp"

#include "not_implemented.hpp"

#ifndef KATANA_VERSION
#error "KATANA_VERSION is defined for katana_update by src/katana_update/CMakeLists.txt"
#endif

namespace katana::update {

core::Result<Version> parseVersion(std::string_view)
{
    return detail::notImplemented("parseVersion", "T4");
}

std::string toString(const Version&)
{
    detail::notYet("toString(Version)", "T4");
}

std::string_view programVersionText()
{
    return KATANA_VERSION;
}

core::Result<Version> programVersion()
{
    return parseVersion(programVersionText());
}

} // namespace katana::update
