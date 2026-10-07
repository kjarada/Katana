// STUB. Task T6 writes raw-deflate inflation over zlib (linked to this library,
// statically, in src/katana_update/CMakeLists.txt) with the output cap of
// include/katana/update/inflate.hpp.

#include "katana/update/inflate.hpp"

#include "not_implemented.hpp"

namespace katana::update {

core::Status inflateRaw(std::string_view, std::uint64_t, const InflateSink&)
{
    return detail::notImplemented("inflateRaw", "T6");
}

core::Result<std::string> inflateRawToString(std::string_view, std::uint64_t)
{
    return detail::notImplemented("inflateRawToString", "T6");
}

} // namespace katana::update
