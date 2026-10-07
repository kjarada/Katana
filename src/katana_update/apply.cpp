// STUB. Task T7 writes the swap of include/katana/update/apply.hpp: lock and
// verify, wait, plan, journal, commit by renames, rollback from the disk,
// health. apply() answers Refused ("nothing was touched", which is true of a
// stub) rather than pretend to have done anything.

#include "katana/update/apply.hpp"

#include "not_implemented.hpp"

namespace katana::update {

std::vector<std::string> helperArguments(const ApplyRequest&)
{
    detail::notYet("helperArguments", "T7");
}

core::Result<ApplyRequest> parseHelperArguments(std::span<const std::string>)
{
    return detail::notImplemented("parseHelperArguments", "T7");
}

std::filesystem::path installedProgram(const std::filesystem::path&, std::string_view)
{
    detail::notYet("installedProgram", "T7");
}

std::string newTransactionId()
{
    detail::notYet("newTransactionId", "T7");
}

ApplyResult apply(const ApplyRequest&, const ApplyServices&, std::stop_token)
{
    ApplyResult result;
    result.outcome = ApplyOutcome::Refused;
    result.reason = "not_implemented";
    result.detail = detail::notImplemented("apply", "T7").describe();
    return result;
}

core::Status rollback(const std::filesystem::path&, const std::string&, const ApplyServices&)
{
    return detail::notImplemented("rollback", "T7");
}

core::Status launchApply(const ApplyRequest&)
{
    return detail::notImplemented("launchApply", "T7");
}

core::Status launchApply(const ApplyRequest&, Launcher&)
{
    return detail::notImplemented("launchApply", "T7");
}

} // namespace katana::update
