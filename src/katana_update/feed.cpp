// STUB. Task T8 writes the feed addresses of include/katana/update/feed.hpp.
// The build-time defaults are in place now (src/katana_update/CMakeLists.txt
// passes them to the library): KATANA_UPDATE_REPO, "kjarada/Katana" unless the
// configure says otherwise, and KATANA_UPDATE_FEED, empty unless it does.

#include "katana/update/feed.hpp"

#include "not_implemented.hpp"

#if !defined(KATANA_UPDATE_REPO) || !defined(KATANA_UPDATE_FEED)
#error "KATANA_UPDATE_REPO and KATANA_UPDATE_FEED are defined by src/katana_update/CMakeLists.txt"
#endif

namespace katana::update {

FeedConfig defaultFeedConfig(const std::string&)
{
    detail::notYet("defaultFeedConfig", "T8");
}

std::string manifestUrl(const FeedConfig&)
{
    detail::notYet("manifestUrl", "T8");
}

std::string signatureUrl(const FeedConfig&)
{
    detail::notYet("signatureUrl", "T8");
}

std::string payloadUrl(const FeedConfig&, const Manifest&)
{
    detail::notYet("payloadUrl", "T8");
}

} // namespace katana::update
