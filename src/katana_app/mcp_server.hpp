#pragma once

// A Model Context Protocol server over a Session (session.hpp): what lets
// Claude - Claude Desktop, Claude Code, any MCP client - open a Katana
// project, draw and edit it, run surveys' calculations, import and export,
// and save it, with the very verbs a person types at katana_cli.
//
// The transport is MCP's stdio one: newline-delimited JSON-RPC 2.0 messages,
// one per line, in on stdin and out on stdout (mcp_main.cpp owns the streams).
// This class only turns a message into its reply, so the tests can drive it
// without a process. docs/mcp.md lists the tools and how to connect a client.

#include <optional>
#include <string>
#include <string_view>

#include "session.hpp"

namespace katana::app::mcp {

// The MCP revision this server is written to. A client asking for an older
// one it also speaks (kSupportedProtocolVersions in mcp_server.cpp) gets it.
inline constexpr std::string_view kLatestProtocolVersion = "2025-06-18";

class Server {
  public:
    // `version` is what serverInfo reports, the program's own version.
    Server(Session& session, std::string version);

    // One message in, its reply out: nullopt for a notification, which MCP
    // never answers. Never throws; a message that is not JSON-RPC is answered
    // with a JSON-RPC error, as the specification requires.
    [[nodiscard]] std::optional<std::string> handle(std::string_view message);

    // The protocol revision agreed at initialize, or the latest before it.
    [[nodiscard]] const std::string& protocolVersion() const { return protocolVersion_; }

  private:
    Session& session_;
    std::string version_;
    std::string protocolVersion_{kLatestProtocolVersion};
};

} // namespace katana::app::mcp
