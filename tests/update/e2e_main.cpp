// katana_update_e2e: the update engine driven from a command line, for the end-to-end tests
// (tests/update/update_e2e.py, which also starts tools/update_fake_feed.py and the test helper,
// katana_updater_test). Never installed: tools/check_install_rules.cmake refuses it by name.
//
// WHY IT EXISTS. The programs that ship cannot be given another trust root or another feed: the
// keys are pinned at build time and the engine takes them as a parameter (trust.hpp). So a test
// that wants the real engine against a release it signed itself needs a program that passes the
// TEST key (test_key.hpp) - this one - and a helper built the same way. It links katana_update and
// katana_core and nothing else, so it has no libcurl: the transport is the engine's own
// update::makeFetch (redirects, the token's rule, the answer of a range - feed.hpp) over a hop
// function written here on plain sockets.
//
// THE WIRE REACHES 127.0.0.1 AND NOTHING ELSE. The Connection below has no way to name another
// host: it connects to the loopback address and a port. An http:// address must spell 127.0.0.1
// (no name is looked up); an https:// address is carried only where --connect says which loopback
// port stands in for its host, in clear, and is refused otherwise. So no test can leave the
// machine through this program, whatever a manifest, a redirect or a mistake names
// (CLAUDE.md, section 4: a test may start its own server on 127.0.0.1).
//
// COMMAND LINES (UTF-8, read with update::utf8Arguments; every folder absolute):
//
//   katana_update_e2e check|plan|stage --root R --state U (--feed URL | --github OWNER/NAME)
//                     [--token T] [--connect HOST=PORT]... [--even-if-not-newer]
//       check   asks the feed (update::check) and applies what U/state.json holds (blocked,
//               skipped) the way the window does
//       plan    check, then hashes the install against the manifest (update::plan)
//       stage   plan, then downloads what changed into U/stage-<sequence> (update::stage)
//       --feed URL          a feed of the owner's own: http://127.0.0.1:<port>[/folder]
//       --github OWNER/NAME GitHub's addresses for that repository, which --connect must route
//       --token T           FeedConfig::token: the private-repository route's token
//       --connect HOST=PORT requests the engine addresses to https://HOST go, in clear, to
//                           127.0.0.1:PORT (what curl calls --connect-to, without TLS): a fake
//                           stands in for that host. It is how the rule "the token goes to
//                           api.github.com and to no address a redirect names" is tested here.
//       --even-if-not-newer plan and stage a release that verified but is not above what is
//                           installed, which the window never does: the way a test hands the
//                           helper a downgrade, to see the helper refuse it by itself
//   katana_update_e2e preflight --root R --state U --stage S
//       reads the staged manifest back through verifyManifest and runs update::preflight: the
//       helper is copied to U/run-<tx> and its --selftest is run. No network.
//   katana_update_e2e launch --root R --state U --stage S [--relaunch <path|none>]
//                     [--wait-pid <n|self>]
//       preflight, then update::launchApply: the helper copy is started detached and this
//       program ends, as the window does. By default the helper waits for THIS process and
//       starts R/bin/katana afterwards.
//   katana_update_e2e recover --root R --state U
//       update::recover(R), the first lines of main() of every program. It finds the helper copy
//       through update::stateDirectory(), so KATANA_UPDATE_STATE_DIR must name U (refused
//       otherwise: a test must not look into the person's own update folder).
//
// WHAT IT PRINTS (standard output, key=value, one a line, a control character as %XX):
//   always       target= kind= installed= installed_sequence=
//   check        update=available|current|unreachable|unsupported|blocked latest= sequence=
//                reason= route=download|api notes_url= detail=
//   plan         files=<changed>/<total> download_bytes= changed_bytes= required_free_bytes=
//                changed=<path> for each changed file, range=<offset>+<length> for each request
//   stage        stage=<folder> progress=<done>/<total> progress_reports=<n>
//                progress_steady=true|false (it never went backwards nor past its total: stage.hpp)
//                staged=<version>; `staged=none` when nothing was offered
//   preflight    stage= version= transaction= run= helper=
//   launch       preflight's, then wait_pid= relaunch= launched=<transaction>
//   recover      recover=ok
//   a failure    error=<ErrorCode> reason=<token of update::fetchFailureReason> message= context=
//
// EXIT CODES: 0 the call was made (a feed that cannot be read is an ANSWER, update=unreachable,
// not a failure); 1 the engine answered with an error (the error= lines say which); 2 the
// command line is wrong; 3 an exception.

#include <algorithm>
#include <array>
#include <charconv>
#include <cstdint>
#include <exception>
#include <filesystem>
#include <initializer_list>
#include <iostream>
#include <map>
#include <memory>
#include <optional>
#include <stop_token>
#include <string>
#include <string_view>
#include <system_error>
#include <utility>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/core/path_text.hpp"
#include "katana/core/text.hpp"
#include "katana/update/apply.hpp"
#include "katana/update/check.hpp"
#include "katana/update/feed.hpp"
#include "katana/update/install.hpp"
#include "katana/update/manifest.hpp"
#include "katana/update/plan.hpp"
#include "katana/update/process.hpp"
#include "katana/update/ranges.hpp"
#include "katana/update/recover.hpp"
#include "katana/update/registry.hpp"
#include "katana/update/stage.hpp"
#include "katana/update/state.hpp"
#include "katana/update/trust.hpp"
#include "katana/update/version.hpp"
#include "test_key.hpp"

// The sockets, last: the Windows headers define a great many names as macros.
#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <winsock2.h>
#include <ws2tcpip.h>
#else
#include <arpa/inet.h>
#include <cerrno>
#include <netinet/in.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <unistd.h>
#endif

namespace {

namespace upd = katana::update;
namespace core = katana::core;
using Path = std::filesystem::path;

constexpr int kExitOk = 0;
constexpr int kExitFailed = 1;
constexpr int kExitUsage = 2;
constexpr int kExitInternal = 3;

constexpr std::string_view kUsage =
    "usage: katana_update_e2e check|plan|stage --root R --state U (--feed URL | --github OWNER/NAME)\n"
    "                         [--token T] [--connect HOST=PORT]... [--even-if-not-newer]\n"
    "       katana_update_e2e preflight --root R --state U --stage S\n"
    "       katana_update_e2e launch --root R --state U --stage S [--relaunch <path|none>]\n"
    "                         [--wait-pid <n|self>]\n"
    "       katana_update_e2e recover --root R --state U\n";

// ---- what is printed ----------------------------------------------------------------------------

// `text` on one line: a control character becomes %XX, so that a value read from a file or a
// server cannot make one record look like two.
std::string oneLine(std::string_view text)
{
    constexpr std::string_view hex = "0123456789ABCDEF";
    std::string out;
    out.reserve(text.size());
    for (const char c : text) {
        const auto byte = static_cast<unsigned char>(c);
        if (byte < 0x20 || byte == 0x7F) {
            out += '%';
            out += hex[byte >> 4];
            out += hex[byte & 0x0F];
        } else {
            out += c;
        }
    }
    return out;
}

void say(std::string_view key, std::string_view value)
{
    std::cout << key << '=' << oneLine(value) << '\n';
}

void sayNumber(std::string_view key, std::uint64_t value)
{
    std::cout << key << '=' << value << '\n';
}

// The error= lines of a failure, and the exit code that goes with them.
int sayError(const core::Error& error)
{
    say("error", core::toString(error.code));
    say("reason", upd::fetchFailureReason(error));
    say("message", error.message);
    say("context", error.context);
    std::cout << std::flush;
    return kExitFailed;
}

// ---- the wire: one request to 127.0.0.1, redirects not followed ------------------------------------

#ifdef _WIN32
using Socket = SOCKET;
const Socket kNoSocket = INVALID_SOCKET;
int lastSocketError()
{
    return WSAGetLastError();
}
#else
using Socket = int;
const Socket kNoSocket = -1;
int lastSocketError()
{
    return errno;
}
#endif

// Winsock must be started before the first socket; elsewhere there is nothing to start.
class SocketsLibrary {
  public:
    SocketsLibrary()
    {
#ifdef _WIN32
        WSADATA data{};
        started_ = WSAStartup(MAKEWORD(2, 2), &data) == 0;
#endif
    }
    SocketsLibrary(const SocketsLibrary&) = delete;
    SocketsLibrary& operator=(const SocketsLibrary&) = delete;
    ~SocketsLibrary()
    {
#ifdef _WIN32
        if (started_) {
            WSACleanup();
        }
#endif
    }
    [[nodiscard]] bool started() const { return started_; }

  private:
    bool started_ = true;
};

// A TCP connection to the loopback address. It takes a port and no host: there is no way to ask
// it for another machine.
class Connection {
  public:
    Connection() = default;
    Connection(const Connection&) = delete;
    Connection& operator=(const Connection&) = delete;
    ~Connection() { shut(); }

    [[nodiscard]] bool open(int port, int timeoutSeconds)
    {
        socket_ = ::socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
        if (socket_ == kNoSocket) {
            return false;
        }
        // A feed that stops answering ends the request; it does not hang the test.
#ifdef _WIN32
        const DWORD limit = static_cast<DWORD>(timeoutSeconds) * 1000;
        (void)::setsockopt(socket_, SOL_SOCKET, SO_RCVTIMEO, reinterpret_cast<const char*>(&limit),
                           sizeof(limit));
        (void)::setsockopt(socket_, SOL_SOCKET, SO_SNDTIMEO, reinterpret_cast<const char*>(&limit),
                           sizeof(limit));
#else
        timeval limit{};
        limit.tv_sec = timeoutSeconds;
        (void)::setsockopt(socket_, SOL_SOCKET, SO_RCVTIMEO, &limit, sizeof(limit));
        (void)::setsockopt(socket_, SOL_SOCKET, SO_SNDTIMEO, &limit, sizeof(limit));
#ifdef SO_NOSIGPIPE
        const int on = 1;
        (void)::setsockopt(socket_, SOL_SOCKET, SO_NOSIGPIPE, &on, sizeof(on));
#endif
#endif
        sockaddr_in to{};
        to.sin_family = AF_INET;
        to.sin_port = htons(static_cast<std::uint16_t>(port));
        to.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        return ::connect(socket_, reinterpret_cast<const sockaddr*>(&to), sizeof(to)) == 0;
    }

    [[nodiscard]] bool sendAll(std::string_view bytes)
    {
        while (!bytes.empty()) {
#ifdef _WIN32
            const int sent = ::send(socket_, bytes.data(), static_cast<int>(bytes.size()), 0);
#else
#ifdef MSG_NOSIGNAL
            const int flags = MSG_NOSIGNAL; // a feed that closed early is an error, not a signal
#else
            const int flags = 0;
#endif
            const ssize_t sent = ::send(socket_, bytes.data(), bytes.size(), flags);
#endif
            if (sent <= 0) {
                return false;
            }
            bytes.remove_prefix(static_cast<std::size_t>(sent));
        }
        return true;
    }

    // The bytes read; 0 when the feed closed the connection; below 0 for an error or a timeout.
    [[nodiscard]] long long receive(char* buffer, std::size_t size)
    {
#ifdef _WIN32
        return ::recv(socket_, buffer, static_cast<int>(size), 0);
#else
        return ::recv(socket_, buffer, size, 0);
#endif
    }

  private:
    void shut()
    {
        if (socket_ == kNoSocket) {
            return;
        }
#ifdef _WIN32
        ::closesocket(socket_);
#else
        ::close(socket_);
#endif
        socket_ = kNoSocket;
    }

    Socket socket_ = kNoSocket;
};

// An address taken apart far enough to send it: update::makeFetch has already held it to
// isFetchableUrl, so this reads and does not judge.
struct Address {
    bool https = false;
    std::string authority; // as written: what the Host header carries
    std::string host;      // lower case, without the port
    int port = 0;          // 0: none written
    std::string target;    // the path and the query, "/" when the address has neither
};

std::optional<Address> parseAddress(std::string_view url)
{
    Address address;
    const std::size_t mark = url.find("://");
    if (mark == std::string_view::npos) {
        return std::nullopt;
    }
    const std::string scheme = core::lowered(url.substr(0, mark));
    if (scheme != "http" && scheme != "https") {
        return std::nullopt;
    }
    address.https = scheme == "https";
    const std::string_view rest = url.substr(mark + 3);
    const std::size_t end = rest.find_first_of("/?#");
    const std::string_view authority = rest.substr(0, end);
    std::string_view target = end == std::string_view::npos ? std::string_view{} : rest.substr(end);
    target = target.substr(0, target.find('#')); // a fragment is never sent
    if (authority.empty() || authority.find('@') != std::string_view::npos ||
        authority.find('[') != std::string_view::npos) {
        return std::nullopt; // user-info, or an IPv6 literal, which this client does not speak
    }
    address.authority = std::string(authority);
    const std::size_t colon = authority.find(':');
    address.host = core::lowered(authority.substr(0, colon));
    if (colon != std::string_view::npos) {
        const std::string_view digits = authority.substr(colon + 1);
        const auto parsed = std::from_chars(digits.data(), digits.data() + digits.size(), address.port);
        if (parsed.ec != std::errc{} || parsed.ptr != digits.data() + digits.size() ||
            address.port < 1 || address.port > 65535) {
            return std::nullopt;
        }
    }
    if (address.host.empty()) {
        return std::nullopt;
    }
    if (target.empty()) {
        address.target = "/";
    } else if (target.front() == '?') {
        address.target = "/" + std::string(target);
    } else {
        address.target = std::string(target);
    }
    return address;
}

// What a hop says when no HTTP answer arrived: FileImportFailure, `status=0` first, then the
// hop's own words and never the address (feed.hpp, HopFn).
core::Error noAnswer(std::string message, const std::string& words)
{
    return core::makeError(core::ErrorCode::FileImportFailure, std::move(message),
                           "status=0; " + words);
}

struct Head {
    int status = 0;
    std::string contentType;
    std::string location;
    std::string contentRange;
    std::optional<std::uint64_t> contentLength;
    bool transferEncoded = false;
};

// The status line and the headers of an answer, without the blank line that ends them.
std::optional<Head> parseHead(std::string_view text)
{
    Head head;
    const std::size_t firstEnd = text.find("\r\n");
    const std::string_view statusLine = text.substr(0, firstEnd);
    // "HTTP/1.1 206 Partial Content": the version, one blank, three digits.
    if (!statusLine.starts_with("HTTP/1.") || statusLine.size() < 12 || statusLine[8] != ' ' ||
        (statusLine.size() > 12 && statusLine[12] != ' ')) {
        return std::nullopt;
    }
    const std::string_view code = statusLine.substr(9, 3);
    const auto parsed = std::from_chars(code.data(), code.data() + code.size(), head.status);
    if (parsed.ec != std::errc{} || parsed.ptr != code.data() + code.size() || head.status < 100) {
        return std::nullopt;
    }
    std::string_view rest =
        firstEnd == std::string_view::npos ? std::string_view{} : text.substr(firstEnd + 2);
    while (!rest.empty()) {
        const std::size_t end = rest.find("\r\n");
        const std::string_view line = rest.substr(0, end);
        rest = end == std::string_view::npos ? std::string_view{} : rest.substr(end + 2);
        const std::size_t colon = line.find(':');
        if (colon == std::string_view::npos) {
            continue;
        }
        const std::string name = core::lowered(core::trimmed(line.substr(0, colon)));
        const std::string_view value = core::trimmed(line.substr(colon + 1));
        if (name == "content-type") {
            head.contentType = std::string(value);
        } else if (name == "location") {
            head.location = std::string(value);
        } else if (name == "content-range") {
            head.contentRange = std::string(value);
        } else if (name == "transfer-encoding") {
            head.transferEncoded = true;
        } else if (name == "content-length") {
            std::uint64_t length = 0;
            const auto counted = std::from_chars(value.data(), value.data() + value.size(), length);
            if (counted.ec != std::errc{} || counted.ptr != value.data() + value.size()) {
                return std::nullopt;
            }
            head.contentLength = length;
        }
    }
    return head;
}

// Which loopback port stands in for an https host (--connect), by the host in lower case.
using Routes = std::map<std::string, int>;

// The headers of an answer are a few hundred bytes; one that sends this many and no blank line
// is not speaking HTTP.
constexpr std::size_t kMaxHeadBytes = 64 * 1024;
// One request on this machine takes milliseconds. The engine's own allowance for a hop is an
// hour for a slow line (feed.hpp); a test waits this long at most.
constexpr int kMaxTimeoutSeconds = 120;

// update::HopFn over a plain socket: HTTP/1.1, one request a connection, no redirect followed
// (update::makeFetch follows them and rebuilds the headers for each hop, which is the code under
// test). The body ends at Content-Length, or at the end of the connection when the answer names
// no length; a transfer coding is not spoken (the fake feed never uses one).
core::Result<upd::HttpHopReply> hopOverLoopback(const Routes& routes, const upd::HttpHop& hop,
                                                const std::stop_token& stop,
                                                const upd::ProgressFn& progress)
{
    if (!upd::isFetchableUrl(hop.url, /*allowLoopbackHttp=*/true)) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "this address cannot be fetched");
    }
    const std::optional<Address> address = parseAddress(hop.url);
    if (!address) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "this address cannot be fetched by the test client");
    }
    int port = 0;
    if (address->https) {
        const auto route = routes.find(address->host);
        if (route == routes.end() || address->port != 0) {
            return noAnswer("the test client reaches the loopback address only",
                            "no --connect names the host " + address->host);
        }
        port = route->second;
    } else {
        if (address->host != "127.0.0.1") {
            return noAnswer("the test client reaches the loopback address only",
                            "an http address must spell 127.0.0.1");
        }
        port = address->port == 0 ? 80 : address->port;
    }
    if (stop.stop_requested()) {
        return core::makeError(core::ErrorCode::InvalidState, "cancelled");
    }

    Connection connection;
    if (!connection.open(port, std::clamp(hop.timeoutSeconds, 1, kMaxTimeoutSeconds))) {
        return noAnswer("no connection to the feed",
                        "os_error=" + std::to_string(lastSocketError()));
    }
    std::string request = "GET " + address->target + " HTTP/1.1\r\nHost: " + address->authority + "\r\n";
    if (!hop.userAgent.empty()) {
        request += "User-Agent: " + hop.userAgent + "\r\n";
    }
    request += "Accept-Encoding: identity\r\nConnection: close\r\n";
    // update::hopHeaders built these, the token among them when the rule allows it: sent as they
    // are, so that what the feed logs is what the engine decided.
    for (const std::string& header : hop.headers) {
        request += header + "\r\n";
    }
    request += "\r\n";
    if (!connection.sendAll(request)) {
        return noAnswer("the request could not be sent",
                        "os_error=" + std::to_string(lastSocketError()));
    }

    std::string data; // the head, then the body
    std::size_t bodyStart = std::string::npos;
    Head head;
    std::array<char, 64 * 1024> buffer{};
    while (true) {
        if (bodyStart != std::string::npos) {
            const std::uint64_t body = data.size() - bodyStart;
            if (progress) {
                progress(body, head.contentLength.value_or(0));
            }
            if (head.contentLength && body >= *head.contentLength) {
                break; // all of it
            }
            if (body > hop.maxBytes) {
                break; // more than was asked for: the transfer is abandoned here
            }
        }
        if (stop.stop_requested()) {
            return core::makeError(core::ErrorCode::InvalidState, "cancelled");
        }
        const long long got = connection.receive(buffer.data(), buffer.size());
        if (got < 0) {
            return noAnswer("the feed did not answer in time, or the connection failed",
                            "os_error=" + std::to_string(lastSocketError()));
        }
        if (got == 0) {
            break; // the feed closed the connection
        }
        data.append(buffer.data(), static_cast<std::size_t>(got));
        if (bodyStart == std::string::npos) {
            const std::size_t blank = data.find("\r\n\r\n");
            if (blank == std::string::npos) {
                if (data.size() > kMaxHeadBytes) {
                    return noAnswer("the feed's answer is not HTTP", "no end of the headers");
                }
                continue;
            }
            const std::optional<Head> answered = parseHead(std::string_view(data).substr(0, blank));
            if (!answered) {
                return noAnswer("the feed's answer is not HTTP", "a malformed status or length");
            }
            if (answered->transferEncoded) {
                return noAnswer("the feed's answer uses a transfer coding the test client does "
                                "not speak",
                                "Transfer-Encoding");
            }
            head = *answered;
            bodyStart = blank + 4;
        }
    }
    if (bodyStart == std::string::npos) {
        return noAnswer("the connection closed before an answer arrived",
                        std::to_string(data.size()) + " bytes");
    }

    upd::HttpHopReply reply;
    reply.status = head.status;
    reply.contentType = std::move(head.contentType);
    reply.location = std::move(head.location);
    reply.contentRange = std::move(head.contentRange);
    reply.body = data.substr(bodyStart);
    const bool success = reply.status == 200 || reply.status == 206;
    if (head.contentLength && reply.body.size() > *head.contentLength) {
        reply.body.resize(static_cast<std::size_t>(*head.contentLength));
    }
    if (reply.body.size() > hop.maxBytes) {
        if (success) {
            // Not the answer, and never handed on (feed.hpp, HttpHopReply::overran): the status
            // and the content type say what kind of wrong it is.
            reply.overran = true;
            reply.body.clear();
        } else {
            // An error's page cut short is still that error's page.
            reply.body.resize(static_cast<std::size_t>(hop.maxBytes));
        }
    } else if (success && head.contentLength && reply.body.size() < *head.contentLength) {
        // The connection dropped inside the body: no answer, as libcurl's "transfer closed with
        // outstanding read data remaining" is none for the hop the programs use.
        return noAnswer("the connection closed before the whole answer had arrived",
                        std::to_string(reply.body.size()) + " of " +
                            std::to_string(*head.contentLength) + " bytes");
    }
    return reply;
}

// ---- the command line ---------------------------------------------------------------------------

struct Arguments {
    std::string command;
    std::map<std::string, std::string> values; // an option that takes a value, once
    Routes routes;                             // --connect, any number of times
    bool evenIfNotNewer = false;

    [[nodiscard]] bool has(const std::string& option) const { return values.contains(option); }
    [[nodiscard]] std::string value(const std::string& option) const
    {
        const auto found = values.find(option);
        return found == values.end() ? std::string() : found->second;
    }
};

core::Error usageError(std::string message, std::string context = {})
{
    return core::makeError(core::ErrorCode::InvalidArgument, std::move(message),
                           std::move(context));
}

bool isOneOf(std::string_view word, std::initializer_list<std::string_view> words)
{
    return std::find(words.begin(), words.end(), word) != words.end();
}

// `words` is the command line without argv[0].
core::Result<Arguments> parseArguments(const std::vector<std::string>& words)
{
    Arguments arguments;
    if (words.empty()) {
        return usageError("no command: say what to do");
    }
    arguments.command = words.front();
    if (!isOneOf(arguments.command, {"check", "plan", "stage", "preflight", "launch", "recover"})) {
        return usageError("unknown command", arguments.command);
    }
    for (std::size_t i = 1; i < words.size(); ++i) {
        const std::string& option = words[i];
        if (option == "--even-if-not-newer") {
            arguments.evenIfNotNewer = true;
            continue;
        }
        if (!isOneOf(option, {"--root", "--state", "--feed", "--github", "--token", "--connect",
                              "--stage", "--relaunch", "--wait-pid"})) {
            return usageError("unknown option", option);
        }
        if (i + 1 >= words.size()) {
            return usageError("option without a value", option);
        }
        const std::string& value = words[++i];
        if (option == "--connect") {
            // HOST=PORT: the loopback port that stands in for an https host.
            const std::size_t equals = value.find('=');
            int port = 0;
            const char* const digits = value.data() + (equals == std::string::npos ? 0 : equals + 1);
            const char* const end = value.data() + value.size();
            const auto parsed = std::from_chars(digits, end, port);
            if (equals == std::string::npos || equals == 0 || parsed.ec != std::errc{} ||
                parsed.ptr != end || port < 1 || port > 65535) {
                return usageError("--connect takes HOST=PORT", value);
            }
            arguments.routes[core::lowered(std::string_view(value).substr(0, equals))] = port;
            continue;
        }
        if (!arguments.values.emplace(option, value).second) {
            return usageError("option given twice", option);
        }
    }

    for (const char* required : {"--root", "--state"}) {
        if (arguments.value(required).empty()) {
            return usageError("missing option", required);
        }
    }
    const bool asksTheFeed = isOneOf(arguments.command, {"check", "plan", "stage"});
    if (asksTheFeed) {
        if (arguments.has("--feed") == arguments.has("--github")) {
            return usageError("give --feed or --github, and not both");
        }
        if (arguments.has("--feed")) {
            // The one kind of feed this program fetches from by itself (feed.hpp: plain http is
            // let through on the loopback only); anything else needs --github and --connect.
            const std::string feed = arguments.value("--feed");
            const std::optional<Address> address = parseAddress(feed);
            if (!upd::isLoopbackHttpUrl(feed) ||
                !upd::isFetchableUrl(feed, /*allowLoopbackHttp=*/true) || !address ||
                address->host != "127.0.0.1") {
                return usageError("--feed is an http address on the loopback: "
                                  "http://127.0.0.1:<port>",
                                  feed);
            }
        }
    } else if (arguments.has("--feed") || arguments.has("--github") || arguments.has("--token") ||
               !arguments.routes.empty() || arguments.evenIfNotNewer) {
        return usageError("this command asks no feed", arguments.command);
    }
    const bool needsTheStage = isOneOf(arguments.command, {"preflight", "launch"});
    if (needsTheStage != arguments.has("--stage")) {
        return usageError(needsTheStage ? "missing option" : "this command takes no stage folder",
                          "--stage");
    }
    if (arguments.command != "launch" && (arguments.has("--relaunch") || arguments.has("--wait-pid"))) {
        return usageError("only launch takes --relaunch and --wait-pid", arguments.command);
    }
    return arguments;
}

// An option that names a folder: absolute, because the helper is told these very paths and a
// relative one would depend on a working folder nobody chose.
core::Result<Path> absoluteFolder(const Arguments& arguments, const std::string& option)
{
    const Path path = core::pathFromUtf8(arguments.value(option));
    if (!path.is_absolute()) {
        return usageError(option + " must be an absolute path", arguments.value(option));
    }
    return path;
}

// ---- what the commands share ----------------------------------------------------------------------

// The registry of this program: none. The install's kind is then `archive` whatever the machine
// has installed, and nothing of the machine's is read or written.
class NoRegistry final : public upd::RegistryOps {
  public:
    [[nodiscard]] core::Result<std::optional<Path>> installerLocation() const override
    {
        return std::optional<Path>{};
    }
    [[nodiscard]] core::Status setRunOnce(std::string_view, std::string_view) override { return {}; }
    [[nodiscard]] core::Status clearRunOnce(std::string_view) override { return {}; }
    [[nodiscard]] core::Status setInstalledVersion(std::string_view, std::string_view) override
    {
        return {};
    }
};

void sayInstall(const upd::Install& install)
{
    say("target", install.target);
    say("kind", upd::toString(install.kind));
    say("installed", upd::toString(install.version));
    sayNumber("installed_sequence", install.sequence);
}

// The staged release, read back the way `UPDATE STATUS` and the helper read it: through the
// signature check, with the test trust.
core::Result<upd::Manifest> readStagedManifest(const Path& stage, const upd::Install& install)
{
    // The token first in the context, where update::fetchFailureReason reads it: the helper's
    // own word for a stage it will not apply (apply.hpp).
    const auto refused = [](const core::Error& why) {
        return core::makeError(why.code, why.message, "reason=staged_manifest; " + why.context);
    };
    const core::Result<std::string> json = core::readFileBytes(upd::stagedManifestPath(stage));
    if (!json) {
        return refused(json.error());
    }
    const core::Result<std::string> signature =
        core::readFileBytes(upd::stagedSignaturePath(stage));
    if (!signature) {
        return refused(signature.error());
    }
    core::Result<upd::Manifest> verified =
        upd::verifyManifest(json.value(), signature.value(), upd::testTrust(), install);
    if (!verified) {
        return refused(verified.error());
    }
    return verified;
}

// stage()'s progress, held to what stage.hpp promises of it.
struct ProgressWatch {
    std::uint64_t reports = 0;
    std::uint64_t done = 0;
    std::uint64_t total = 0;
    bool steady = true;

    void see(std::uint64_t nowDone, std::uint64_t nowTotal)
    {
        if (nowDone < done || nowDone > nowTotal || (reports > 0 && nowTotal != total)) {
            steady = false;
        }
        ++reports;
        done = nowDone;
        total = nowTotal;
    }
};

// ---- check, plan, stage ---------------------------------------------------------------------------

int runFeedCommand(const Arguments& arguments, const upd::Install& install, const Path& state)
{
    upd::FeedConfig feed = upd::defaultFeedConfig(install.target);
    if (arguments.has("--feed")) {
        feed.feedBase = arguments.value("--feed");
        while (!feed.feedBase.empty() && feed.feedBase.back() == '/') {
            feed.feedBase.pop_back(); // "a base URL without a trailing '/'" (feed.hpp)
        }
    } else {
        feed.feedBase.clear();
        feed.repo = arguments.value("--github");
    }
    feed.token = arguments.value("--token");

    const SocketsLibrary sockets;
    if (!sockets.started()) {
        return sayError(core::makeError(core::ErrorCode::Internal, "the sockets could not be started"));
    }
    const Routes routes = arguments.routes;
    const upd::Fetch fetch = upd::makeFetch(
        [routes](const upd::HttpHop& hop, std::stop_token stop, upd::ProgressFn progress) {
            return hopOverLoopback(routes, hop, stop, progress);
        });

    core::Result<upd::CheckResult> checked =
        upd::check(fetch, feed, install, upd::testTrust(), std::stop_token{});
    if (!checked) {
        return sayError(checked.error());
    }
    upd::CheckResult result = std::move(checked).value();
    // What the person decided and what failed here before, as the window applies it. A state
    // file that cannot be read is the defaults (state.hpp), and said.
    const core::Result<upd::UpdateState> recorded = upd::readState(state);
    if (recorded) {
        upd::applyUpdateState(result, recorded.value());
    } else {
        say("state_error", recorded.error().describe());
    }
    say("update", upd::toString(result.status));
    say("latest", result.latestVersion);
    sayNumber("sequence", result.manifest ? result.manifest->sequence : 0);
    say("reason", result.reason);
    say("route", result.route == upd::FeedRoute::Api ? "api" : "download");
    say("notes_url", result.notesUrl);
    say("detail", result.detail);
    if (arguments.command == "check") {
        return kExitOk;
    }

    const bool offered = result.status == upd::CheckStatus::Available;
    const bool forced = arguments.evenIfNotNewer && result.manifest.has_value() &&
                        result.status == upd::CheckStatus::Current;
    if (!offered && !forced) {
        if (arguments.command == "stage") {
            say("staged", "none");
        }
        return kExitOk;
    }
    const upd::Manifest& manifest = *result.manifest;
    const core::Result<upd::Plan> planned = upd::plan(manifest, install, std::stop_token{});
    if (!planned) {
        return sayError(planned.error());
    }
    const upd::Plan& plan = planned.value();
    say("files", std::to_string(plan.changedFiles) + "/" + std::to_string(plan.totalFiles));
    sayNumber("download_bytes", plan.downloadBytes);
    sayNumber("changed_bytes", plan.changedBytes);
    sayNumber("required_free_bytes", plan.requiredFreeBytes);
    for (const upd::PlannedFile& file : plan.files) {
        if (file.changed && file.index < manifest.files.size()) {
            say("changed", manifest.files[file.index].path);
        }
    }
    for (const upd::MergedRange& range : plan.ranges) {
        say("range", std::to_string(range.offset) + "+" + std::to_string(range.length));
    }
    if (arguments.command == "plan") {
        return kExitOk;
    }

    // The payload is fetched the way the manifest was (check.hpp, CheckResult::route).
    feed.route = result.route;
    const Path stageDirectory = upd::stagePath(state, manifest.sequence);
    say("stage", core::pathToUtf8(stageDirectory));
    ProgressWatch watch;
    const core::Status staged =
        upd::stage(fetch, feed, manifest, plan, stageDirectory, std::stop_token{},
                   [&watch](std::uint64_t done, std::uint64_t total) { watch.see(done, total); });
    say("progress", std::to_string(watch.done) + "/" + std::to_string(watch.total));
    sayNumber("progress_reports", watch.reports);
    say("progress_steady", watch.steady ? "true" : "false");
    if (!staged.ok()) {
        return sayError(staged.error());
    }
    say("staged", upd::toString(manifest.version));
    return kExitOk;
}

// ---- preflight, launch ----------------------------------------------------------------------------

int runApplyCommand(const Arguments& arguments, const upd::Install& install, const Path& state)
{
    const core::Result<Path> stage = absoluteFolder(arguments, "--stage");
    if (!stage) {
        std::cerr << "katana_update_e2e: " << stage.error().describe() << '\n' << kUsage;
        return kExitUsage;
    }
    // Read before anything is started, so that a wrong value starts nothing.
    upd::ApplyRequest request;
    request.root = install.root;
    request.stageDirectory = stage.value();
    request.stateDirectory = state;
    request.relaunch = upd::installedProgram(install.root, "katana");
    if (arguments.has("--relaunch")) {
        const std::string relaunch = arguments.value("--relaunch");
        request.relaunch = relaunch == "none" ? Path() : core::pathFromUtf8(relaunch);
        if (!request.relaunch.empty() && !request.relaunch.is_absolute()) {
            std::cerr << "katana_update_e2e: --relaunch must be an absolute path or none\n" << kUsage;
            return kExitUsage;
        }
    }
    // The window's own flow: the helper waits for the program that started it to be gone.
    request.waitPid = upd::makeSystemProcessOps()->currentPid();
    const std::string waitPid = arguments.value("--wait-pid");
    if (!waitPid.empty() && waitPid != "self") {
        const auto parsed =
            std::from_chars(waitPid.data(), waitPid.data() + waitPid.size(), request.waitPid);
        if (parsed.ec != std::errc{} || parsed.ptr != waitPid.data() + waitPid.size()) {
            std::cerr << "katana_update_e2e: --wait-pid takes a process id or self\n" << kUsage;
            return kExitUsage;
        }
    }

    const core::Result<upd::Manifest> manifest = readStagedManifest(stage.value(), install);
    if (!manifest) {
        return sayError(manifest.error());
    }
    say("stage", core::pathToUtf8(stage.value()));
    say("version", upd::toString(manifest.value().version));

    const std::unique_ptr<upd::Launcher> launcher = upd::makeSystemLauncher();
    const core::Result<upd::PreparedApply> prepared =
        upd::preflight(manifest.value(), install, stage.value(), state, *launcher);
    if (!prepared) {
        return sayError(prepared.error());
    }
    say("transaction", prepared.value().transaction);
    say("run", core::pathToUtf8(prepared.value().runDirectory));
    say("helper", core::pathToUtf8(prepared.value().helper));
    if (arguments.command == "preflight") {
        return kExitOk;
    }

    request.helper = prepared.value().helper;
    request.transaction = prepared.value().transaction;
    sayNumber("wait_pid", request.waitPid);
    say("relaunch", request.relaunch.empty() ? std::string("none") : core::pathToUtf8(request.relaunch));
    // Flushed before the helper exists: it waits for this process to be gone, and whoever reads
    // these lines must not wait for the helper.
    std::cout << std::flush;
    if (const core::Status launched = upd::launchApply(request, *launcher); !launched.ok()) {
        return sayError(launched.error());
    }
    say("launched", request.transaction);
    return kExitOk;
}

// ---- recover --------------------------------------------------------------------------------------

int runRecover(const upd::Install& install, const Path& state)
{
    // recover() finds the helper copy through stateDirectory(), not through an argument. Held to
    // the folder this run was given, so that a test which forgot the variable does not go
    // looking in the person's own update folder.
    std::error_code ec;
    const Path library = upd::stateDirectory();
    if (library.empty() || !std::filesystem::equivalent(library, state, ec) || ec) {
        std::cerr << "katana_update_e2e: recover needs KATANA_UPDATE_STATE_DIR to name the folder "
                     "given as --state\n";
        return kExitUsage;
    }
    if (const core::Status recovered = upd::recover(install.root); !recovered.ok()) {
        return sayError(recovered.error());
    }
    say("recover", "ok");
    return kExitOk;
}

int run(int argc, char** argv)
{
    std::vector<std::string> words = upd::utf8Arguments(argc, argv);
    if (!words.empty()) {
        words.erase(words.begin());
    }
    const core::Result<Arguments> parsed = parseArguments(words);
    if (!parsed) {
        std::cerr << "katana_update_e2e: " << parsed.error().describe() << '\n' << kUsage;
        return kExitUsage;
    }
    const Arguments& arguments = parsed.value();
    const core::Result<Path> root = absoluteFolder(arguments, "--root");
    const core::Result<Path> state = absoluteFolder(arguments, "--state");
    for (const core::Result<Path>* folder : {&root, &state}) {
        if (!folder->ok()) {
            std::cerr << "katana_update_e2e: " << folder->error().describe() << '\n' << kUsage;
            return kExitUsage;
        }
    }

    const NoRegistry registry{};
    const core::Result<upd::Install> install = upd::readInstall(root.value(), registry);
    if (!install) {
        return sayError(install.error());
    }
    sayInstall(install.value());

    int code = kExitOk;
    if (arguments.command == "recover") {
        code = runRecover(install.value(), state.value());
    } else if (arguments.command == "preflight" || arguments.command == "launch") {
        code = runApplyCommand(arguments, install.value(), state.value());
    } else {
        code = runFeedCommand(arguments, install.value(), state.value());
    }
    std::cout << std::flush;
    return code;
}

} // namespace

int main(int argc, char** argv)
{
    try {
        return run(argc, argv);
    } catch (const std::exception& error) {
        std::cerr << "katana_update_e2e: unexpected error: " << error.what() << '\n';
        return kExitInternal;
    }
}
