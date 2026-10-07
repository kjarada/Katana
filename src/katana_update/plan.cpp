// STUB. Task T6 writes the planner of include/katana/update/plan.hpp (it hashes
// with core::Sha256, which task T2 provides).

#include "katana/update/plan.hpp"

#include "not_implemented.hpp"

namespace katana::update {

core::Result<Plan> plan(const Manifest&, const Install&, std::stop_token)
{
    return detail::notImplemented("plan", "T6");
}

} // namespace katana::update
