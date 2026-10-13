// STUB. Task T8 writes stage() and preflight() of include/katana/update/stage.hpp.

#include "katana/update/stage.hpp"

#include "not_implemented.hpp"

namespace katana::update {

core::Status stage(const Fetch&, const FeedConfig&, const Manifest&, const Plan&,
                   const std::filesystem::path&, std::stop_token, ProgressFn)
{
    return detail::notImplemented("stage", "T8");
}

core::Result<PreparedApply> preflight(const Manifest&, const Install&, const std::filesystem::path&,
                                      const std::filesystem::path&, Launcher&)
{
    return detail::notImplemented("preflight", "T8");
}

} // namespace katana::update
