#pragma once

// Reaching web services through GDAL, behind a Katana interface (Rule 4).
//
// GDAL already carries an HTTP client - libcurl, with the proxy, certificate
// and retry handling every one of its web drivers (WMS, WMTS, WFS, OAPIF,
// /vsicurl/) uses - so Katana does not link a second one. A request goes
// through CPLHTTPFetchEx, which means the proxy a person configured for GDAL
// (GDAL_HTTP_PROXY, or the ordinary HTTPS_PROXY environment libcurl reads) and
// the certificate bundle it trusts (CURL_CA_BUNDLE, SSL_CERT_FILE) apply to
// Katana's own requests and to GDAL's reads alike: one network stack, one set
// of settings to get right.
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

// The lower-case hexadecimal SHA-256 of `bytes` (FIPS 180-4). The disk
// cache names its files with it, so two different requests cannot share a
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
