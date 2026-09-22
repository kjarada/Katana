#include "validation.hpp"

#include <string>

#include "katana/entity/entity.hpp"

namespace katana::entity::detail {

katana::core::Status validateName(std::string_view name, const char* what)
{
    if (name.empty()) {
        return katana::core::makeError(katana::core::ErrorCode::InvalidArgument,
                                       std::string(what) + " name is empty");
    }
    if (!isValidUtf8(name)) {
        return katana::core::makeError(katana::core::ErrorCode::InvalidArgument,
                                       std::string(what) + " name is not valid UTF-8");
    }
    return {};
}

} // namespace katana::entity::detail
