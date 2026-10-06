#include "katana/entity/linework_codes.hpp"

#include <algorithm>
#include <array>

#include "katana/core/text.hpp"

namespace katana::entity {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;

std::span<const LineworkCodeMember> lineworkCodeMembers()
{
    static constexpr std::array<LineworkCodeMember, 7> kMembers{{
        {"start", &LineworkCodes::start},
        {"end", &LineworkCodes::end},
        {"close", &LineworkCodes::close},
        {"arcStart", &LineworkCodes::arcStart},
        {"arcEnd", &LineworkCodes::arcEnd},
        {"join", &LineworkCodes::join},
        {"rectangle", &LineworkCodes::rectangle},
    }};
    return kMembers;
}

Status validate(const LineworkCodes& codes)
{
    const auto all = lineworkCodeMembers();
    for (std::size_t i = 0; i < all.size(); ++i) {
        const std::string& spelling = codes.*all[i].spelling;
        if (std::any_of(spelling.begin(), spelling.end(), katana::core::isAsciiSpace)) {
            return makeError(ErrorCode::InvalidArgument,
                             "a linework control code cannot contain a blank: codes are split "
                             "on blanks, so it could never be matched",
                             std::string(all[i].name) + "=\"" + spelling + "\"");
        }
        for (std::size_t j = i + 1; j < all.size() && !spelling.empty(); ++j) {
            if (katana::core::equalsIgnoringCase(spelling, codes.*all[j].spelling)) {
                return makeError(ErrorCode::InvalidArgument,
                                 "two linework controls are spelled alike, so the token would "
                                 "mean both",
                                 std::string(all[i].name) + " and " + std::string(all[j].name) +
                                     " are \"" + spelling + "\"");
            }
        }
    }
    return {};
}

} // namespace katana::entity
