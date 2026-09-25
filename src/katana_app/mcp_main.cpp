// katana_mcp: the Katana engine as a Model Context Protocol server, so that
// Claude can run engineering projects in Katana (docs/mcp.md).
//
//   katana_mcp                         serve MCP on stdin/stdout
//   katana_mcp --project <directory>   ... with that project already open
//   katana_mcp --help
//
// An MCP client (Claude Desktop, Claude Code) starts it and talks to it over
// its standard streams; it is not run by hand. Diagnostics go to stderr, which
// the clients keep as the server's log.

#include <cstdio>
#include <iostream>
#include <string>

#if defined(_WIN32)
#include <fcntl.h>
#include <io.h>
#else
#include <unistd.h>
#endif

#include "mcp_server.hpp"
#include "session.hpp"

#ifndef KATANA_VERSION
#define KATANA_VERSION "0.0.0"
#endif

namespace {

// The protocol's own stdout, taken before anything else can write to it.
//
// stdout belongs to the protocol: one stray line - a library's printf, a
// warning written straight to the file descriptor - and the client reads a
// message that is not JSON and drops the connection. Server::handle captures
// std::cout around each command, but that cannot catch C stdio or anything
// below it. So file descriptor 1 is duplicated for the protocol and then
// pointed at stderr: whatever else writes to "stdout" lands in the log.
std::FILE* takeProtocolStream()
{
    std::fflush(stdout);
#if defined(_WIN32)
    const int protocol = _dup(_fileno(stdout));
    if (protocol < 0 || _dup2(_fileno(stderr), _fileno(stdout)) != 0) {
        return nullptr;
    }
    // Binary, both ways: a message ends in "\n", never "\r\n".
    (void)_setmode(protocol, _O_BINARY);
    (void)_setmode(_fileno(stdin), _O_BINARY);
    return _fdopen(protocol, "wb");
#else
    const int protocol = dup(fileno(stdout));
    if (protocol < 0 || dup2(fileno(stderr), fileno(stdout)) < 0) {
        return nullptr;
    }
    return fdopen(protocol, "w");
#endif
}

void send(std::FILE* protocol, const std::string& message)
{
    std::fwrite(message.data(), 1, message.size(), protocol);
    std::fputc('\n', protocol);
    std::fflush(protocol);
}

void usage(std::ostream& out)
{
    out << "usage: katana_mcp [--project <directory>]\n\n"
           "Serves the Katana engine to an MCP client (Claude Desktop, Claude Code) over\n"
           "stdin and stdout. Started by the client, not by hand; see docs/mcp.md.\n\n"
           "  --project <directory>  open this Katana project when the session starts\n";
}

} // namespace

int main(int argc, char* argv[])
{
    std::string project;
    for (int i = 1; i < argc; ++i) {
        const std::string argument = argv[i];
        if (argument == "-h" || argument == "--help") {
            usage(std::cout);
            return 0;
        }
        if (argument == "--project" && i + 1 < argc) {
            project = argv[++i];
            continue;
        }
        std::cerr << "error: unexpected argument " << argument << "\n";
        usage(std::cerr);
        return 2;
    }

    std::FILE* protocol = takeProtocolStream();
    if (protocol == nullptr) {
        std::cerr << "error: katana_mcp could not take its standard output for the protocol\n";
        return 1;
    }
    // std::cout now reaches stderr, as everything but the protocol must.
    katana::app::Session session(argc > 0 ? argv[0] : nullptr);
    if (!project.empty()) {
        if (project.find('"') != std::string::npos) {
            std::cerr << "error: a project path may not contain a double quote\n";
            return 2;
        }
        if (!session.run("OPEN \"" + project + "\"")) {
            return 1;
        }
    }
    std::cout.flush();
    std::cerr << "katana_mcp " << KATANA_VERSION << ": serving MCP on stdio\n";

    katana::app::mcp::Server server(session, KATANA_VERSION);
    for (std::string line; std::getline(std::cin, line);) {
        if (!line.empty() && line.back() == '\r') {
            line.pop_back();
        }
        if (line.find_first_not_of(" \t") == std::string::npos) {
            continue;
        }
        if (const auto answer = server.handle(line)) {
            send(protocol, *answer);
        }
    }
    std::fclose(protocol);
    return 0;
}
