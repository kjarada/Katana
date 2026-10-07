// The registry on a system that has none: there is no installer to have
// recorded a folder, no RunOnce and no Uninstall key, so nothing is read and a
// write succeeds by doing nothing - which is the correct, finished behaviour
// there, not a stub. Compiled into the update library everywhere except
// Windows (src/katana_update/CMakeLists.txt).

#include "katana/update/registry.hpp"

namespace katana::update {

namespace {

class NoRegistry final : public RegistryOps {
  public:
    [[nodiscard]] core::Result<std::optional<std::filesystem::path>>
    installerLocation() const override
    {
        return std::optional<std::filesystem::path>{};
    }
    [[nodiscard]] core::Status setRunOnce(std::string_view, std::string_view) override
    {
        return {};
    }
    [[nodiscard]] core::Status clearRunOnce(std::string_view) override { return {}; }
    [[nodiscard]] core::Status setInstalledVersion(std::string_view, std::string_view) override
    {
        return {};
    }
};

} // namespace

std::unique_ptr<RegistryOps> makeSystemRegistryOps()
{
    return std::make_unique<NoRegistry>();
}

} // namespace katana::update
