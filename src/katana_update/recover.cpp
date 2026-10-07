// STUB. Task T7 writes recovery of include/katana/update/recover.hpp.

#include "katana/update/recover.hpp"

#include "not_implemented.hpp"

namespace katana::update {

core::Status recover(const std::filesystem::path&)
{
    return detail::notImplemented("recover", "T7");
}

core::Status recoverInProcess(const std::filesystem::path&, const ApplyServices&)
{
    return detail::notImplemented("recoverInProcess", "T7");
}

} // namespace katana::update
