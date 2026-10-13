#include "update_transport.hpp"

#include <string>
#include <utility>

#if defined(KATANA_WITH_INTEROP)
#include "katana/gis/web_access.hpp"
#include "katana/update/version.hpp"
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

// The Error of a hop that got no answer, written again from what gis::httpExchange said, in
// the form update::HopFn asks for: the hop's own words and NOT the address (update::makeFetch
// writes the address, without its query).
//
// gis's contexts are "<address>: <libcurl's message>", the address through gis::redactUrl,
// which hides the query parameters whose NAMES look like secrets and keeps the rest. That is
// right for a map service's key and wrong here: the address a release redirects to is a signed
// one, all of it query, under names redactUrl has never heard of. So of gis's context only what
// follows the address is passed on - libcurl's own words ("Could not resolve host: ...", "SSL
// certificate problem: ..."), which are what tell a refused certificate from a missing network.
// (makeFetch does not rely on this: it cuts the query from anything a hop quotes.)
core::Error hopError(const core::Error& error)
{
    switch (error.code) {
    case core::ErrorCode::InvalidState: // cancelled
    case core::ErrorCode::InvalidArgument:
        return core::makeError(error.code, error.message);
    default:
        break;
    }
    // No HTTP answer at all: `status=0` first (update/feed.hpp, the Fetch conventions). An
    // address that update::makeFetch let through holds no blank, so the first ": " is the one
    // gis wrote after it.
    std::string words;
    if (const std::size_t at = error.context.find(": "); at != std::string::npos) {
        words = error.context.substr(at + 2);
    }
    return core::makeError(core::ErrorCode::FileImportFailure, error.message,
                           words.empty() ? std::string("status=0") : "status=0; " + words);
}

core::Result<update::HttpHopReply> hopOverGis(const update::HttpHop& hop, std::stop_token stop,
                                              update::ProgressFn progress)
{
    gis::HttpRequest request;
    request.url = hop.url;
    // Every request names its program (web_access.hpp). The engine's requests carry
    // "Katana/<version> (<target>)" (update::defaultFeedConfig); one that came without says at
    // least which program and version it is, never libcurl's or GDAL's own name.
    request.userAgent = hop.userAgent.empty()
                            ? "Katana/" + std::string(update::programVersionText())
                            : hop.userAgent;
    // update::hopHeaders built these: the token is among them on the first hop of a request to
    // api.github.com and on no other. gis::httpExchange adds nothing of its own to them - no
    // credential a person configured for GDAL's requests goes with an exchange.
    request.headers = hop.headers;
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
        return hopError(exchange.error());
    }
    update::HttpHopReply reply;
    reply.status = exchange.value().status;
    reply.contentType = std::move(exchange.value().contentType);
    reply.location = std::move(exchange.value().location);
    reply.contentRange = std::move(exchange.value().contentRange);
    reply.body = std::move(exchange.value().body);
    // gis ended the transfer at hop.maxBytes and kept none of the body (web_access.hpp). For a
    // 200 or 206 that is an answer the engine must not take for the file, and the status and the
    // content type say which kind of wrong it is (update::HttpHopReply::overran). An error's
    // page cut at its first 64 KiB is still just that error's page.
    reply.overran = exchange.value().truncated && (reply.status == 200 || reply.status == 206);
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
        // The token says why (update::fetchFailureReason reads it); without one an Unsupported
        // would read as an answer that was too large.
        return core::makeError(core::ErrorCode::Unsupported,
                               "this build of Katana has no network access", "reason=no_network");
    };
}

#endif

} // namespace katana::app
