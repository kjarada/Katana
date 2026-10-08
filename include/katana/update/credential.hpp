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

    // The stored token, without the white space and the invisible characters at its ends
    // (ASCII blanks, a no-break or zero-width space, a direction mark, a byte-order mark: what
    // a token copied from a web page or a chat arrives with). Never logs the value.
    //
    // The errors, which a caller must tell apart:
    //   NotFound      there is none: unset, empty or only ASCII blanks. "Absent" is the normal
    //                 state, so it is not an error to report loudly.
    //   InvalidState  there is something, and it is not a usable token (a control character,
    //                 an inner space such as a pasted "Bearer " prefix, or an invisible
    //                 character inside it, over 2560 bytes, not UTF-8; a value of only
    //                 invisible characters too). The message names the place, never the
    //                 value, so a person can correct it.
    //   Unsupported   Windows only: the Credential Manager cannot be used in this session
    //                 (a service or batch logon with no profile, a stripped image).
    //   Internal      Windows only: any other Credential Manager failure; the Windows error
    //                 number is in the message.
    // A caller that has more than one source (feed.hpp: environment first, then this) treats
    // EVERY error other than NotFound as "no token from this source", says so once, and goes
    // on to the next source: a bad KATANA_UPDATE_TOKEN must not hide a valid token in the
    // Credential Manager, and a broken Credential Manager must not stop an update from a
    // public repository.
    [[nodiscard]] virtual core::Result<std::string> read() const = 0;

    // Stores the token (replacing one). The same token rule as read(): InvalidArgument for
    // an empty, over-long, control-character, inner-space, invisible-character or non-UTF-8
    // token. Unsupported where the store is read-only (the environment), and for an unusable
    // Credential Manager; Internal for its other failures.
    [[nodiscard]] virtual core::Status write(std::string_view token) = 0;

    // Removes it; success when none was stored. Unsupported where read-only or where the
    // Credential Manager cannot be used; Internal for its other failures.
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
