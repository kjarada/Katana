#pragma once

// The one secret the update system can hold: a read-only access token for a
// private release repository, for people who update from one. While
// kjarada/Katana is public (the default) no token is needed and none is asked
// for.
//
// Rules, each tested (docs/update.md):
//   - NEVER compiled into a binary.
//   - NEVER in a verb: verb lines are logged, so a token on one would be
//     written to the log. `UPDATE TOKEN STATUS` prints present|absent only;
//     `UPDATE TOKEN CLEAR` removes it; there is no verb that sets or shows it.
//     The window's Settings > Updates page writes it through write() and
//     shows only that one is stored.
//   - NEVER in a message, a log line or an error context: describe a token as
//     "token" and nothing else. redactToken() below is what a log line that
//     has to mention a header passes it through.
//   - Sent only over https to exactly api.github.com, never along a redirect
//     (feed.hpp, FetchRequest::bearerToken).
//
// Where it lives: the environment variable KATANA_UPDATE_TOKEN (read-only;
// the one source on Linux and macOS, and an override everywhere) and, on
// Windows, the Credential Manager under the target name "Katana/update-token"
// (CredReadW, CredWriteW, CredDeleteW), written by the settings page.
// Precedence when both exist is the caller's (feed.hpp: environment first).

#include <memory>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"

namespace katana::update {

inline constexpr std::string_view kTokenEnvironmentVariable = "KATANA_UPDATE_TOKEN";
inline constexpr std::string_view kCredentialTarget = "Katana/update-token";

class CredentialStore {
  public:
    virtual ~CredentialStore() = default;

    // The stored token; NotFound when there is none (not an error to report
    // loudly: "absent" is the normal state). Never logs the value.
    [[nodiscard]] virtual core::Result<std::string> read() const = 0;

    // Stores the token (replacing one). Unsupported where the store is
    // read-only (the environment).
    [[nodiscard]] virtual core::Status write(std::string_view token) = 0;

    // Removes it; success when none was stored. Unsupported where read-only.
    [[nodiscard]] virtual core::Status clear() = 0;

    // "environment" or "credential manager": where it came from, for a
    // sentence that names the place but never the value.
    [[nodiscard]] virtual std::string_view where() const = 0;
};

// KATANA_UPDATE_TOKEN, read through core::environmentVariable. Everywhere.
[[nodiscard]] std::unique_ptr<CredentialStore> makeEnvironmentCredentialStore();

// Windows: the Credential Manager. Elsewhere: the environment store (there is
// no keyring in this version).
[[nodiscard]] std::unique_ptr<CredentialStore> makeSystemCredentialStore();

// `text` with every occurrence of `token` replaced by "***" (and a
// non-empty token shorter than 4 characters still replaced). For the one place
// a log line must quote a header or a URL.
[[nodiscard]] std::string redactToken(std::string_view text, std::string_view token);

} // namespace katana::update
