#include "katana/gis/web_access.hpp"

#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <limits>
#include <optional>
#include <string>
#include <thread>

#include <cpl_error.h>
#include <cpl_http.h>
#include <cpl_minixml.h>
#include <cpl_string.h>

#include "gdal_registry.hpp"
#include "katana/core/text.hpp"

namespace katana::gis {
namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

constexpr std::uint64_t kMaxXmlBytes = 64ull * 1024 * 1024;

// What a transfer's callbacks share with the thread that started it.
struct TransferState {
    const std::stop_token* stop = nullptr;
    const TransferProgress* progress = nullptr;
    std::uint64_t maxBytes = 0;
    std::string body;
    bool tooLarge = false;
};

// libcurl's write callback, through GDAL: the body is collected here rather
// than in GDAL's own buffer, whose size is an int, so the limit is ours and
// is enforced as the bytes arrive. Returning less than was offered aborts
// the transfer.
size_t collect(void* buffer, size_t size, size_t count, void* state)
{
    auto& transfer = *static_cast<TransferState*>(state);
    const std::size_t bytes = size * count;
    if (transfer.body.size() + bytes > transfer.maxBytes) {
        transfer.tooLarge = true;
        return 0;
    }
    if (transfer.stop != nullptr && transfer.stop->stop_requested()) {
        return 0;
    }
    transfer.body.append(static_cast<const char*>(buffer), bytes);
    return bytes;
}

// GDAL's progress callback; returning FALSE is how a transfer is cancelled
// between packets, so a stopped job does not wait out a slow download.
int reportTransfer(double fraction, const char*, void* state)
{
    auto& transfer = *static_cast<TransferState*>(state);
    if (transfer.progress != nullptr && *transfer.progress) {
        const auto received = static_cast<std::uint64_t>(transfer.body.size());
        const std::uint64_t total =
            fraction > 0.0 ? static_cast<std::uint64_t>(static_cast<double>(received) / fraction)
                           : 0;
        (*transfer.progress)(received, total);
    }
    return transfer.stop != nullptr && transfer.stop->stop_requested() ? FALSE : TRUE;
}

// GDAL reports an HTTP failure in its error text ("HTTP error code : 404"),
// not a field: the status is read back out of it.
int statusFromError(const std::string& text)
{
    const std::size_t at = text.find("HTTP error code");
    if (at == std::string::npos) {
        return 0;
    }
    std::size_t digit = text.find_first_of("0123456789", at);
    if (digit == std::string::npos) {
        return 0;
    }
    int code = 0;
    while (digit < text.size() && text[digit] >= '0' && text[digit] <= '9' && code < 1000) {
        code = code * 10 + (text[digit] - '0');
        ++digit;
    }
    return code;
}

// Whether another attempt could succeed: rate limiting, a server's own
// trouble, and transport failures (a timeout, a reset). A 4xx is the
// request's fault and the same request would fail the same way.
bool worthRetrying(int status, int curlCode)
{
    if (status == 429 || (status >= 500 && status < 600)) {
        return true;
    }
    if (status != 0) {
        return false;
    }
    // CURLE_COULDNT_CONNECT 7, OPERATION_TIMEDOUT 28, SEND_ERROR 55,
    // RECV_ERROR 56, GOT_NOTHING 52, PARTIAL_FILE 18 - curl.h's numbering,
    // which GDAL passes through as nStatus.
    return curlCode == 7 || curlCode == 18 || curlCode == 28 || curlCode == 52 ||
           curlCode == 55 || curlCode == 56;
}

// Sleeps `seconds`, waking early if a stop is requested. True when it slept
// the whole time.
bool pause(double seconds, const std::stop_token& stop)
{
    const auto until = std::chrono::steady_clock::now() +
                       std::chrono::milliseconds(static_cast<long long>(seconds * 1000.0));
    while (std::chrono::steady_clock::now() < until) {
        if (stop.stop_requested()) {
            return false;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(50));
    }
    return !stop.stop_requested();
}

// The path a file:// URL names: "file:///C:/x" and "file:///home/x" both,
// with %XX escapes decoded.
std::filesystem::path pathOfFileUrl(std::string_view url)
{
    std::string_view rest = url.substr(7); // after "file://"
    if (rest.starts_with("localhost/")) {
        rest.remove_prefix(9);
    }
    std::string decoded;
    for (std::size_t i = 0; i < rest.size(); ++i) {
        if (rest[i] == '%' && i + 2 < rest.size()) {
            const auto hex = [](char c) -> int {
                if (c >= '0' && c <= '9') {
                    return c - '0';
                }
                if (c >= 'a' && c <= 'f') {
                    return c - 'a' + 10;
                }
                if (c >= 'A' && c <= 'F') {
                    return c - 'A' + 10;
                }
                return -1;
            };
            const int high = hex(rest[i + 1]);
            const int low = hex(rest[i + 2]);
            if (high >= 0 && low >= 0) {
                decoded.push_back(static_cast<char>(high * 16 + low));
                i += 2;
                continue;
            }
        }
        decoded.push_back(rest[i]);
    }
    // "/C:/data/x.tif" is a Windows drive path with the URL's leading slash.
    if (decoded.size() > 2 && decoded[0] == '/' && decoded[2] == ':') {
        decoded.erase(decoded.begin());
    }
    return std::filesystem::path(std::u8string(decoded.begin(), decoded.end()));
}

Result<HttpResponse> readFileUrl(const HttpRequest& request)
{
    const std::filesystem::path path = pathOfFileUrl(request.url);
    std::error_code error;
    if (!std::filesystem::is_regular_file(path, error)) {
        return makeError(ErrorCode::NotFound, "no file at this file URL", request.url);
    }
    const auto size = std::filesystem::file_size(path, error);
    if (error) {
        return makeError(ErrorCode::FileImportFailure, "could not read the file URL",
                         request.url);
    }
    if (size > request.maxBytes) {
        return makeError(ErrorCode::Unsupported,
                         "the answer is larger than the " +
                             std::to_string(request.maxBytes / (1024 * 1024)) +
                             " MB this request allows; ask for a smaller area",
                         request.url);
    }
    std::ifstream in(path, std::ios::binary);
    HttpResponse response;
    response.body.resize(static_cast<std::size_t>(size));
    in.read(response.body.data(), static_cast<std::streamsize>(size));
    if (!in) {
        return makeError(ErrorCode::FileImportFailure, "could not read the file URL",
                         request.url);
    }
    const std::string extension = katana::core::lowered(path.extension().string());
    response.contentType = extension == ".json" || extension == ".geojson" ? "application/json"
                           : extension == ".xml"                           ? "text/xml"
                           : extension == ".tif" || extension == ".tiff"   ? "image/tiff"
                           : extension == ".png"                           ? "image/png"
                           : extension == ".jpg" || extension == ".jpeg"   ? "image/jpeg"
                                                                           : "";
    return response;
}

// The first words of a failed answer's body, for the error's context: an
// ArcGIS or OGC exception report says in plain text what was wrong with the
// request, which is worth more than the status code.
std::string snippetOf(const std::string& body)
{
    std::string text;
    for (const char c : body.substr(0, 400)) {
        text.push_back(c == '\n' || c == '\r' || c == '\t' ? ' ' : c);
    }
    return std::string(katana::core::trimmed(text));
}

void convert(const CPLXMLNode* node, XmlElement& element)
{
    element.name = node->pszValue != nullptr ? node->pszValue : "";
    std::string text;
    for (const CPLXMLNode* child = node->psChild; child != nullptr; child = child->psNext) {
        switch (child->eType) {
        case CXT_Attribute:
            if (child->psChild != nullptr && child->psChild->pszValue != nullptr) {
                element.attributes[child->pszValue] = child->psChild->pszValue;
            } else {
                element.attributes[child->pszValue] = {};
            }
            break;
        case CXT_Text:
            if (child->pszValue != nullptr) {
                text += child->pszValue;
            }
            break;
        case CXT_Element: {
            XmlElement nested;
            convert(child, nested);
            element.children.push_back(std::move(nested));
            break;
        }
        default:
            break;
        }
    }
    element.text = std::string(katana::core::trimmed(text));
}

std::string_view localOf(std::string_view name)
{
    const std::size_t colon = name.rfind(':');
    return colon == std::string_view::npos ? name : name.substr(colon + 1);
}

} // namespace

// ---- HTTP -------------------------------------------------------------------

Result<HttpResponse> httpFetch(const HttpRequest& request, const std::stop_token& stop,
                               const TransferProgress& progress)
{
    const std::string lower = katana::core::lowered(request.url.substr(0, 8));
    if (lower.starts_with("file://")) {
        return readFileUrl(request);
    }
    if (!lower.starts_with("https://") && !lower.starts_with("http://")) {
        return makeError(ErrorCode::InvalidArgument,
                         "only http, https and file URLs can be fetched", redactUrl(request.url));
    }
    detail::ensureGdalRegistered();

    CPLStringList options;
    options.SetNameValue("TIMEOUT", std::to_string(std::max(1, request.timeoutSeconds)).c_str());
    options.SetNameValue("CONNECTTIMEOUT",
                         std::to_string(std::max(1, request.connectTimeoutSeconds)).c_str());
    // A stalled transfer - under 16 bytes a second for 30 seconds - ends
    // before the overall timeout would.
    options.SetNameValue("LOW_SPEED_LIMIT", "16");
    options.SetNameValue("LOW_SPEED_TIME", "30");
    // Retries are this function's, below: GDAL's own would sleep through a
    // cancellation.
    options.SetNameValue("MAX_RETRY", "0");
    if (!request.userAgent.empty()) {
        options.SetNameValue("USERAGENT", request.userAgent.c_str());
    }
    std::string headers;
    for (const std::string& header : request.headers) {
        headers += header + "\r\n";
    }
    if (!request.postBody.empty()) {
        options.SetNameValue("POSTFIELDS", request.postBody.c_str());
        headers += "Content-Type: " + request.postContentType + "\r\n";
    }
    if (!headers.empty()) {
        options.SetNameValue("HEADERS", headers.c_str());
    }

    const std::string shown = redactUrl(request.url);
    double delay = std::max(0.1, request.retryDelaySeconds);
    for (int attempt = 0;; ++attempt) {
        if (stop.stop_requested()) {
            return makeError(ErrorCode::InvalidState, "cancelled", shown);
        }
        TransferState transfer;
        transfer.stop = &stop;
        transfer.progress = &progress;
        transfer.maxBytes = request.maxBytes;

        CPLErrorReset();
        CPLPushErrorHandler(CPLQuietErrorHandler);
        CPLHTTPResult* result = CPLHTTPFetchEx(request.url.c_str(), options.List(),
                                               reportTransfer, &transfer, collect, &transfer);
        CPLPopErrorHandler();
        if (result == nullptr) {
            return makeError(ErrorCode::FileImportFailure, "the request could not be made",
                             shown + ": " + CPLGetLastErrorMsg());
        }
        const int curlCode = result->nStatus;
        const std::string error = result->pszErrBuf != nullptr ? result->pszErrBuf : "";
        const std::string contentType =
            result->pszContentType != nullptr ? result->pszContentType : "";
        CPLHTTPDestroyResult(result);

        if (stop.stop_requested()) {
            return makeError(ErrorCode::InvalidState, "cancelled", shown);
        }
        if (transfer.tooLarge) {
            return makeError(ErrorCode::Unsupported,
                             "the answer is larger than the " +
                                 std::to_string(request.maxBytes / (1024 * 1024)) +
                                 " MB this request allows; ask for a smaller area or a coarser "
                                 "resolution",
                             shown);
        }
        const int status = statusFromError(error);
        if (curlCode == 0 && error.empty()) {
            HttpResponse response;
            response.status = 200;
            response.contentType = contentType;
            response.body = std::move(transfer.body);
            return response;
        }
        if (attempt < request.retries && worthRetrying(status, curlCode)) {
            if (!pause(delay, stop)) {
                return makeError(ErrorCode::InvalidState, "cancelled", shown);
            }
            delay *= 2.0;
            continue;
        }
        const std::string context =
            shown + (status != 0 ? " (HTTP " + std::to_string(status) + ")" : "") +
            (error.empty() ? "" : ": " + error) +
            (transfer.body.empty() ? "" : " - " + snippetOf(transfer.body));
        if (status == 404) {
            return makeError(ErrorCode::NotFound, "the service has nothing at this address",
                             context);
        }
        if (status == 401 || status == 403) {
            return makeError(ErrorCode::FileImportFailure,
                             "the service refused the request (" + std::to_string(status) +
                                 "); it may need a key (ONLINE KEY) or not allow this use",
                             context);
        }
        if (status == 429) {
            return makeError(ErrorCode::FileImportFailure,
                             "the service is limiting requests (429); wait and try again, or "
                             "ask for a smaller area",
                             context);
        }
        if (error.find("CONNECT tunnel failed") != std::string::npos ||
            curlCode == 5 /* CURLE_COULDNT_RESOLVE_PROXY */) {
            return makeError(ErrorCode::FileImportFailure,
                             "the network's proxy refused the connection to this service; the "
                             "host may be blocked where Katana is running",
                             context);
        }
        if (curlCode == 6 /* CURLE_COULDNT_RESOLVE_HOST */) {
            return makeError(ErrorCode::FileImportFailure,
                             "the service's host name could not be found; check the address and "
                             "the network connection",
                             context);
        }
        if (curlCode == 28) {
            return makeError(ErrorCode::FileImportFailure,
                             "the service did not answer within " +
                                 std::to_string(request.timeoutSeconds) +
                                 " s; try a smaller area or later",
                             context);
        }
        return makeError(ErrorCode::FileImportFailure, "the request failed", context);
    }
}

bool isSecretParameter(std::string_view name)
{
    const std::string folded = katana::core::lowered(name);
    return folded == "key" || folded == "apikey" || folded == "api_key" || folded == "api" ||
           folded == "token" || folded == "access_token" || folded == "password" ||
           folded == "sig" || folded == "signature" || folded == "subscription-key" ||
           folded == "auth" || folded.ends_with("key") || folded.ends_with("token");
}

std::string redactUrl(std::string_view url)
{
    std::string text(url);
    // user:password@host
    if (const std::size_t scheme = text.find("://"); scheme != std::string::npos) {
        const std::size_t hostStart = scheme + 3;
        const std::size_t pathStart = text.find_first_of("/?#", hostStart);
        const std::size_t at = text.find('@', hostStart);
        if (at != std::string::npos && (pathStart == std::string::npos || at < pathStart)) {
            text.erase(hostStart, at + 1 - hostStart);
        }
    }
    const std::size_t query = text.find('?');
    if (query == std::string::npos) {
        return text;
    }
    std::string out = text.substr(0, query + 1);
    std::size_t at = query + 1;
    bool first = true;
    while (at <= text.size()) {
        std::size_t end = text.find('&', at);
        if (end == std::string::npos) {
            end = text.size();
        }
        const std::string pair = text.substr(at, end - at);
        const std::size_t equals = pair.find('=');
        const std::string name = katana::core::lowered(pair.substr(0, equals));
        const bool secret = isSecretParameter(name);
        if (!first) {
            out += '&';
        }
        first = false;
        out += secret && equals != std::string::npos ? pair.substr(0, equals) + "=***" : pair;
        if (end == text.size()) {
            break;
        }
        at = end + 1;
    }
    return out;
}

std::string percentEncode(std::string_view text)
{
    static constexpr char kHex[] = "0123456789ABCDEF";
    std::string out;
    out.reserve(text.size());
    for (const char c : text) {
        const auto byte = static_cast<unsigned char>(c);
        const bool unreserved = (byte >= 'A' && byte <= 'Z') || (byte >= 'a' && byte <= 'z') ||
                                (byte >= '0' && byte <= '9') || byte == '-' || byte == '.' ||
                                byte == '_' || byte == '~';
        if (unreserved) {
            out.push_back(c);
        } else {
            out.push_back('%');
            out.push_back(kHex[byte >> 4]);
            out.push_back(kHex[byte & 0x0F]);
        }
    }
    return out;
}

// FIPS 180-4 SHA-256. Written here because GDAL's CPL_SHA256 is not in its
// installed headers; the test checks it against the standard's own vectors
// ("abc", the empty string, the 448-bit message).
std::string sha256Hex(std::string_view bytes)
{
    static constexpr std::uint32_t kRound[64] = {
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2};
    std::uint32_t state[8] = {0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
                              0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19};
    const auto rotate = [](std::uint32_t value, int by) {
        return (value >> by) | (value << (32 - by));
    };
    std::string message(bytes);
    const std::uint64_t bitLength = static_cast<std::uint64_t>(bytes.size()) * 8;
    message.push_back(static_cast<char>(0x80));
    while (message.size() % 64 != 56) {
        message.push_back('\0');
    }
    for (int shift = 56; shift >= 0; shift -= 8) {
        message.push_back(static_cast<char>((bitLength >> shift) & 0xFF));
    }
    for (std::size_t block = 0; block < message.size(); block += 64) {
        std::uint32_t w[64];
        for (int i = 0; i < 16; ++i) {
            const auto byte = [&](int k) {
                return static_cast<std::uint32_t>(
                    static_cast<unsigned char>(message[block + static_cast<std::size_t>(4 * i + k)]));
            };
            w[i] = (byte(0) << 24) | (byte(1) << 16) | (byte(2) << 8) | byte(3);
        }
        for (int i = 16; i < 64; ++i) {
            const std::uint32_t s0 = rotate(w[i - 15], 7) ^ rotate(w[i - 15], 18) ^ (w[i - 15] >> 3);
            const std::uint32_t s1 = rotate(w[i - 2], 17) ^ rotate(w[i - 2], 19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16] + s0 + w[i - 7] + s1;
        }
        std::uint32_t a = state[0], b = state[1], c = state[2], d = state[3];
        std::uint32_t e = state[4], f = state[5], g = state[6], h = state[7];
        for (int i = 0; i < 64; ++i) {
            const std::uint32_t s1 = rotate(e, 6) ^ rotate(e, 11) ^ rotate(e, 25);
            const std::uint32_t choice = (e & f) ^ (~e & g);
            const std::uint32_t t1 = h + s1 + choice + kRound[i] + w[i];
            const std::uint32_t s0 = rotate(a, 2) ^ rotate(a, 13) ^ rotate(a, 22);
            const std::uint32_t majority = (a & b) ^ (a & c) ^ (b & c);
            const std::uint32_t t2 = s0 + majority;
            h = g;
            g = f;
            f = e;
            e = d + t1;
            d = c;
            c = b;
            b = a;
            a = t1 + t2;
        }
        state[0] += a;
        state[1] += b;
        state[2] += c;
        state[3] += d;
        state[4] += e;
        state[5] += f;
        state[6] += g;
        state[7] += h;
    }
    static constexpr char kHex[] = "0123456789abcdef";
    std::string out;
    out.reserve(64);
    for (const std::uint32_t word : state) {
        for (int shift = 28; shift >= 0; shift -= 4) {
            out.push_back(kHex[(word >> shift) & 0x0F]);
        }
    }
    return out;
}

// ---- XML --------------------------------------------------------------------

std::string_view XmlElement::localName() const
{
    return localOf(name);
}

const XmlElement* XmlElement::child(std::string_view local) const
{
    for (const XmlElement& nested : children) {
        if (nested.localName() == local) {
            return &nested;
        }
    }
    return nullptr;
}

std::vector<const XmlElement*> XmlElement::childrenNamed(std::string_view local) const
{
    std::vector<const XmlElement*> found;
    for (const XmlElement& nested : children) {
        if (nested.localName() == local) {
            found.push_back(&nested);
        }
    }
    return found;
}

std::string XmlElement::childText(std::string_view local) const
{
    const XmlElement* found = child(local);
    return found != nullptr ? found->text : std::string();
}

std::string XmlElement::attribute(std::string_view local) const
{
    for (const auto& [key, value] : attributes) {
        if (localOf(key) == local) {
            return value;
        }
    }
    return {};
}

Result<XmlElement> parseXml(std::string_view text)
{
    if (text.size() > kMaxXmlBytes) {
        return makeError(ErrorCode::Unsupported, "the XML document is larger than 64 MB",
                         std::to_string(text.size()) + " bytes");
    }
    const std::string owned(text);
    CPLErrorReset();
    CPLPushErrorHandler(CPLQuietErrorHandler);
    CPLXMLNode* root = CPLParseXMLString(owned.c_str());
    CPLPopErrorHandler();
    if (root == nullptr) {
        return makeError(ErrorCode::ParseFailure, "the document is not XML",
                         CPLGetLastErrorMsg());
    }
    // The document element: the first element sibling, after any <?xml?>
    // declaration and comments.
    const CPLXMLNode* element = root;
    while (element != nullptr &&
           (element->eType != CXT_Element || (element->pszValue != nullptr &&
                                              element->pszValue[0] == '?'))) {
        element = element->psNext;
    }
    if (element == nullptr) {
        CPLDestroyXMLNode(root);
        return makeError(ErrorCode::ParseFailure, "the XML document has no element");
    }
    XmlElement out;
    convert(element, out);
    CPLDestroyXMLNode(root);
    return out;
}

} // namespace katana::gis
