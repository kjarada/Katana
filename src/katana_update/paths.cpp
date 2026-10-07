// STUB. Task T4 writes the path rules of include/katana/update/paths.hpp.

#include "katana/update/paths.hpp"

#include "not_implemented.hpp"

namespace katana::update {

core::Status checkManifestPath(std::string_view)
{
    return detail::notImplemented("checkManifestPath", "T4");
}

core::Status checkManifestPaths(std::span<const std::string>, std::span<const std::uint64_t>)
{
    return detail::notImplemented("checkManifestPaths", "T4");
}

core::Status checkJournalPath(std::string_view)
{
    return detail::notImplemented("checkJournalPath", "T4");
}

bool isReservedName(std::string_view)
{
    detail::notYet("isReservedName", "T4");
}

} // namespace katana::update
