// STUB. Task T8 writes check() of include/katana/update/check.hpp.

#include "katana/update/check.hpp"

#include "not_implemented.hpp"

namespace katana::update {

std::string_view toString(CheckStatus)
{
    detail::notYet("toString(CheckStatus)", "T8");
}

core::Result<CheckResult> check(const Fetch&, const FeedConfig&, const Install&, const Trust&,
                                std::stop_token)
{
    return detail::notImplemented("check", "T8");
}

} // namespace katana::update
