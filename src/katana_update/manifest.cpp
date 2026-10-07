// STUB. Task T4 writes the manifest reader (nlohmann_json, PRIVATE to this
// library) against include/katana/update/manifest.hpp.

#include "katana/update/manifest.hpp"

#include "not_implemented.hpp"

namespace katana::update {

std::string_view toString(FileClass)
{
    detail::notYet("toString(FileClass)", "T4");
}

core::Result<FileClass> parseFileClass(std::string_view)
{
    return detail::notImplemented("parseFileClass", "T4");
}

core::Result<Manifest> parseManifest(std::string_view)
{
    return detail::notImplemented("parseManifest", "T4");
}

} // namespace katana::update
