#include "update_transport.hpp"

#if defined(KATANA_WITH_INTEROP)
#include "katana/gis/web_access.hpp"
#endif

namespace katana::app {

#if defined(KATANA_WITH_INTEROP)

namespace {

// Seconds to make a connection. Not the request's: a range may take minutes to arrive, a
// connection that is not made in this time will not be made.
constexpr int kConnectTimeoutSeconds = 20;
// Attempts after the first, for a dropped connection or a 502/503/504 (gis::httpExchange). A
// download is resumed at file granularity (stage.hpp), so a few are enough here.
constexpr int kRetries = 2;

core::Result<update::HttpHopReply> hopOverGis(const update::HttpHop& hop, std::stop_token stop,
                                              update::ProgressFn progress)
{
    gis::HttpRequest request;
    request.url = hop.url;
    request.userAgent = hop.userAgent;
    request.headers = hop.headers; // the token is in here for api.github.com and nowhere else
    request.timeoutSeconds = hop.timeoutSeconds;
    request.connectTimeoutSeconds = kConnectTimeoutSeconds;
    request.retries = kRetries;
    request.maxBytes = hop.maxBytes;

    core::Result<gis::HttpExchange> exchange = gis::httpExchange(
        request, stop, [&progress](std::uint64_t received, std::uint64_t total) {
            if (progress) {
                progress(received, total);
            }
        });
    if (!exchange) {
        const core::Error& error = exchange.error();
        if (error.code != core::ErrorCode::FileImportFailure) {
            return error;
        }
        // No HTTP answer at all: `status=0` (feed.hpp), the address without its query, and the
        // transport's own words after the address, which are what tells a refused certificate
        // from a missing network.
        std::string detail;
        if (const std::size_t at = error.context.find(": "); at != std::string::npos) {
            detail = error.context.substr(at + 2);
        }
        return core::makeError(core::ErrorCode::FileImportFailure, error.message,
                               "status=0; " + update::displayUrl(hop.url) +
                                   (detail.empty() ? std::string() : "; " + detail));
    }
    update::HttpHopReply reply;
    reply.status = exchange.value().status;
    reply.contentType = std::move(exchange.value().contentType);
    reply.location = std::move(exchange.value().location);
    reply.contentRange = std::move(exchange.value().contentRange);
    reply.body = std::move(exchange.value().body);
    return reply;
}

} // namespace

update::Fetch makeUpdateFetch()
{
    return update::makeFetch(hopOverGis);
}

#else

update::Fetch makeUpdateFetch()
{
    return [](const update::FetchRequest&, std::stop_token,
              update::ProgressFn) -> core::Result<std::string> {
        return core::makeError(core::ErrorCode::Unsupported,
                               "this build of Katana has no network access");
    };
}

#endif

} // namespace katana::app
