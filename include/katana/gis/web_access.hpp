#pragma once

// Reaching web services through GDAL, behind a Katana interface (Rule 4).
//
// GDAL already carries an HTTP client - libcurl, with the proxy, certificate
// and retry handling every one of its web drivers (WMS, WMTS, WFS, OAPIF,
// /vsicurl/) uses - so Katana brings no second one. A request goes
// through CPLHTTPFetchEx, which means the proxy a person configured for GDAL
// (GDAL_HTTP_PROXY, or the ordinary HTTPS_PROXY environment libcurl reads) and
// the certificate bundle it trusts (CURL_CA_BUNDLE, SSL_CERT_FILE) apply to
// Katana's own requests and to GDAL's reads alike: one network stack, one set
// of settings to get right.
//
// The one request CPLHTTPFetchEx cannot make - a request that does not follow
// a redirect, httpExchange below - is made on a libcurl handle of this file's
// own, which GDAL's CPLHTTPSetOptions configures. For that this library links
// libcurl by name (src/katana_io/CMakeLists.txt), and it must be THE libcurl
// GDAL itself loads: a handle made by one copy of the library and configured
// by another is not a handle. One libcurl in the link is a thing the build has
// to hold, on every platform.
//
// Nothing here knows what a web map service IS. Building the URLs, paging,
// caching and choosing what to ask for belongs to katana::interop
// (online_*.hpp); this layer moves bytes, parses XML into plain values and
// hashes, and names no GDAL type.
//
// Threading: every function is safe to call from any thread; a fetch blocks
// its caller until it ends, is cancelled, or times out, which is why the
// desktop application calls it from a background job (src/katana_qt/jobs.hpp)
// and never from the GUI thread.

#include <cstdint>
#include <functional>
#include <map>
#include <stop_token>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::gis {

struct HttpRequest {
    std::string url;
    // Non-empty makes the request a POST with this body (a STAC search, an
    // Overpass query too long for a URL).
    std::string postBody;
    std::string postContentType = "application/json";
    // Extra request headers, "Name: value" each. A key a service wants in a
    // header goes here and so never appears in a URL that might be logged.
    std::vector<std::string> headers;
    // Every request names its program: the OpenStreetMap tile and Overpass
    // policies require a User-Agent that identifies the application, and a
    // publisher who sees trouble in its logs should be able to tell whose.
    std::string userAgent;
    // Seconds for the whole transfer, and for the connection alone. Both
    // bounded, always: a service that accepts a connection and then sends
    // nothing would otherwise hold a job for ever.
    int timeoutSeconds = 120;
    int connectTimeoutSeconds = 20;
    // Attempts after the first, for the failures worth another try - 429 and
    // 5xx answers, timeouts, a dropped connection - with the delay doubling
    // from retryDelaySeconds each time. A 4xx other than 429 is the request's
    // own fault and is never retried.
    int retries = 3;
    double retryDelaySeconds = 1.0;
    // The largest body accepted. The transfer is abandoned the moment it is
    // exceeded, so an unexpected multi-gigabyte answer costs this much memory
    // and no more.
    std::uint64_t maxBytes = 256ull * 1024 * 1024;
};

struct HttpResponse {
    int status = 0; // 200, or 0 for a file:// URL
    std::string contentType;
    std::string body; // bytes, not necessarily text
};

// Bytes received so far and the total the server announced (0 when it did
// not). Called from the fetching thread.
using TransferProgress = std::function<void(std::uint64_t received, std::uint64_t total)>;

// Fetches `request.url`. `file://` URLs are read from disk without libcurl,
// which is how the tests exercise every caller of this function with no
// network. Fails with:
//   InvalidArgument  - a URL that is not http, https or file;
//   NotFound         - a 404, or a file:// path that does not exist;
//   FileImportFailure- any other HTTP or transport failure after the retries,
//                      its context the status and the server's first words;
//   InvalidState     - cancelled through `stop` (the message says so);
//   Unsupported      - the body exceeded request.maxBytes.
// Error messages and contexts carry the URL through `redactUrl`, never raw,
// so a key in a query string does not reach a log.
[[nodiscard]] katana::core::Result<HttpResponse> httpFetch(const HttpRequest& request,
                                                           const std::stop_token& stop = {},
                                                           const TransferProgress& progress = {});

// ONE exchange, as the wire had it. httpFetch follows redirects inside libcurl, re-sending
// the request's headers to wherever it is sent, and GDAL's CPLHTTPFetch has no switch to stop
// it (checked in libgdal 3.13: no such option exists). A caller that must decide, hop by
// hop, which headers go where - the update engine, whose access token must reach
// api.github.com and never the storage host that address redirects to - uses this instead:
//   - the request is sent once to request.url (http or https; GET only, so postBody must be
//     empty); a redirect is NOT followed, its Location is returned;
//   - every HTTP status that arrives is a result, 3xx, 4xx and 5xx included: the caller reads
//     it. Only a failure to get an HTTP answer is an error;
//   - a body is read up to a bound and no further: request.maxBytes for a 200 or 206, 64 KiB
//     for any other status (an error's page, of which the beginning is all anybody reads). An
//     answer that is longer is ended the moment it passes the bound - the rest is not waited
//     for - and is still a result, with `truncated` set: its status and headers arrived, and
//     they are what tell a caller a sign-in page from a file that is too long. Of a 200 or 206
//     that was cut NOTHING is handed over (`body` is empty): a beginning must never pass for
//     the file. Of any other status the first 64 KiB are;
//   - the body is the server's bytes as it holds them: no Accept-Encoding is sent and nothing
//     is decompressed, so a `Range` header asks for, and a count measures, those bytes;
//   - retries (request.retries, doubling from retryDelaySeconds) are for what is worth another
//     try: a dropped or timed-out connection, and a 502, 503 or 504 answer. A 429 is returned
//     at once - a rate limit that says wait an hour is not helped by a second.
// Proxy, certificates, timeouts and the User-Agent are set exactly as httpFetch sets them
// (GDAL's own option handling is applied to the same libcurl handle), with two exceptions.
//   - An exchange sends what its request names and NOTHING a person configured for GDAL's own
//     requests: no ~/.netrc entry (GDAL_HTTP_NETRC), no GDAL_HTTP_USERPWD, no GDAL_HTTP_AUTH or
//     GDAL_HTTP_BEARER, no GDAL_HTTP_HEADERS or GDAL_HTTP_HEADER_FILE header, no cookie, no
//     client certificate; and CPL_CURL_VERBOSE does not print its headers. Those are settings
//     for a map service, and a hop of a caller that follows redirects by hand may be any host.
//     The proxy and the proxy's own credentials stay: they are how the network is reached.
//   - A request with an Authorization (or Proxy-Authorization) header always has the server's
//     certificate checked, even where GDAL_HTTP_UNSAFESSL turns the check off for everything
//     else, so that a credential is never handed to an unverified server.
// Errors: InvalidArgument (not http or https, or a body), InvalidState (cancelled),
// FileImportFailure (no HTTP answer: explainHttpFailure's words - except a timeout, which says
// how long was waited and gives no advice about map areas - with the address through
// redactUrl and then libcurl's own message in the context, "<address>: <message>"). The
// request's headers are never put in an error or a log. redactUrl keeps a query it does not
// know for a secret, so a caller whose addresses are secret as a whole (a signed download
// address) takes the message from the context and leaves the address.
struct HttpExchange {
    int status = 0;            // the HTTP status of the last response on the connection
    std::string contentType;   // "" when none
    std::string location;      // the Location header, "" when none
    std::string contentRange;  // the Content-Range header, "" when none
    std::string body;
    // The answer was longer than its bound and was ended there (see above): `body` is empty
    // for a 200 or 206, and the first 64 KiB for any other status.
    bool truncated = false;
};
[[nodiscard]] katana::core::Result<HttpExchange>
httpExchange(const HttpRequest& request, const std::stop_token& stop = {},
             const TransferProgress& progress = {});

// Whether these request headers ("Name: value" each) hand over credentials: one of them is
// Authorization or Proxy-Authorization (RFC 9110, 11.6.2 and 11.7.2), the name in any case.
// httpExchange's rule for when the certificate check cannot be turned off, public so that it is
// tested without a server.
[[nodiscard]] bool carriesCredentials(const std::vector<std::string>& headers);

// A transfer that failed after its retries, as httpFetch saw it.
struct HttpFailure {
    int status = 0;   // the HTTP status, 0 when none arrived
    int curlCode = 0; // libcurl's CURLcode, 0 for none
    std::string error; // libcurl's (or GDAL's) message
    std::string body;  // what the server sent, if anything
    int timeoutSeconds = 0;
};

// What a failure means, in words a person can act on - a key, the
// network's proxy, a service that answers only from its own country, the
// certificates https:// is checked against - with `context` (the redacted
// URL, the status and the server's first words) kept beside it. httpFetch's
// own classification, public so that every case is tested without a network.
[[nodiscard]] katana::core::Error explainHttpFailure(const HttpFailure& failure,
                                                     std::string context);

// The URL with the value of every query parameter whose name suggests a
// secret (key, apikey, api_key, token, access_token, password, sig,
// signature, and anything ending in "key" or "token") replaced by "***", and
// any user:password@ removed. For messages and logs; the request itself is
// sent unredacted.
[[nodiscard]] std::string redactUrl(std::string_view url);
// Whether a query parameter of this name holds a secret, by redactUrl's rule
// (also "api", "subscription-key" and "auth", which services use for keys).
[[nodiscard]] bool isSecretParameter(std::string_view name);

// Percent-encodes everything outside RFC 3986's unreserved set, for a query
// parameter's value. A space is %20, never '+', which some servers read
// literally.
[[nodiscard]] std::string percentEncode(std::string_view text);

// The lower-case hexadecimal SHA-256 of `bytes` (FIPS 180-4); it forwards to
// core::sha256Hex (core/sha256.hpp), which holds the one implementation. The
// disk cache names its files with it, so two different requests cannot share a
// file and a key in a URL never becomes part of a file name.
[[nodiscard]] std::string sha256Hex(std::string_view bytes);

// ---- XML ------------------------------------------------------------------

// One element of a parsed document, with namespace prefixes kept as written
// ("wms:Layer"); `localName` drops the prefix, which is what a capabilities
// reader compares, because services disagree about prefixes and never about
// local names.
struct XmlElement {
    std::string name;
    std::map<std::string, std::string> attributes;
    // The element's own text, the concatenation of its text children,
    // trimmed. Empty for an element with only child elements.
    std::string text;
    std::vector<XmlElement> children;

    [[nodiscard]] std::string_view localName() const;
    // The first child with this local name, or null.
    [[nodiscard]] const XmlElement* child(std::string_view local) const;
    // Every child with this local name, in document order.
    [[nodiscard]] std::vector<const XmlElement*> childrenNamed(std::string_view local) const;
    // The text of the first child with this local name; empty when absent.
    [[nodiscard]] std::string childText(std::string_view local) const;
    // An attribute by its local name (xlink:href is found as "href").
    [[nodiscard]] std::string attribute(std::string_view local) const;
};

// Parses an XML document with GDAL's own parser (CPLParseXMLString) into the
// value tree above; the root returned is the document element. ParseFailure,
// with the parser's message, for text that is not XML. A document larger than
// 64 MB is refused before parsing: no capabilities document is that size, and
// the tree costs several times the text.
[[nodiscard]] katana::core::Result<XmlElement> parseXml(std::string_view text);

} // namespace katana::gis
