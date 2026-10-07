// The environment credential store, redactToken, and what both credential stores share:
// the one rule for what a token may be. include/katana/update/credential.hpp has the contract.
//
// A token is a secret, so nothing here puts one anywhere but into the Result a caller asked
// for: not into a message, a context, an exception or a log line. An error describes the
// token as "token" and gives its LENGTH at most. The buffers this file owns are zeroed before
// they are freed (the standard library may keep a copy a caller's string made; that is the
// interface's cost, a std::string).

#include "katana/update/credential.hpp"

#include <cstddef>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/core/path_text.hpp"
#include "katana/core/text_encoding.hpp"

namespace katana::update {

namespace detail {

// The most bytes a token may have. The Windows Credential Manager refuses a blob over
// 5 * 512 = 2560 bytes (CRED_MAX_CREDENTIAL_BLOB_SIZE in the Windows SDK's wincred.h; mingw-w64's
// copy says 512, which is not what the system enforces - measured on Windows 11 on 2026-10-08:
// 2560 accepted, 2561 refused with error 1783), and a token the store could not hold is not one
// to accept from the environment either. GitHub advises storing its tokens in 255 characters (its
// 2021 note on the new token formats), far inside this.
inline constexpr std::size_t kMaxTokenBytes = 2560;

// Overwrites `size` bytes at `data` with zeros in a way the compiler may not remove as a dead
// store (a write through a volatile pointer is an observable side effect).
void secureZero(void* data, std::size_t size)
{
    volatile unsigned char* byte = static_cast<volatile unsigned char*>(data);
    for (std::size_t i = 0; i < size; ++i) {
        byte[i] = 0;
    }
}

void secureZero(std::string& text)
{
    secureZero(text.data(), text.size());
}

// `raw` without the white space around it: a token pasted into a field or exported from
// `$(cat file)` arrives with a newline, and none of those characters can be part of one. A view
// into `raw`; nothing is copied.
[[nodiscard]] std::string_view trimmedToken(std::string_view raw)
{
    const auto isSpace = [](char c) { return c == ' ' || (c >= '\t' && c <= '\r'); };
    while (!raw.empty() && isSpace(raw.front())) {
        raw.remove_prefix(1);
    }
    while (!raw.empty() && isSpace(raw.back())) {
        raw.remove_suffix(1);
    }
    return raw;
}

// `token` itself when it can be used as a token, else why not. A token goes into an HTTP header
// (`Authorization: Bearer <token>`), so a control character - above all a carriage return or a
// line feed, which would end the header and begin another - is refused wherever the token comes
// from. Non-ASCII text is allowed: the store keeps whatever valid UTF-8 it is given, and the
// server is the judge of what it accepts. `source` goes into the error's context and must not be
// the token.
[[nodiscard]] core::Result<std::string_view> usableToken(std::string_view token,
                                                         std::string_view source)
{
    if (token.empty()) {
        return core::makeError(core::ErrorCode::InvalidArgument, "the token is empty",
                               std::string(source));
    }
    if (token.size() > kMaxTokenBytes) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "the token is too long: " + std::to_string(token.size()) +
                                   " bytes, and at most " + std::to_string(kMaxTokenBytes) +
                                   " can be kept",
                               std::string(source));
    }
    for (const char c : token) {
        const auto byte = static_cast<unsigned char>(c);
        if (byte < 0x20 || byte == 0x7F) {
            return core::makeError(core::ErrorCode::InvalidArgument,
                                   "the token holds a control character, which an HTTP header "
                                   "cannot carry",
                                   std::string(source));
        }
    }
    if (!core::isValidUtf8(token)) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "the token is not valid UTF-8 text", std::string(source));
    }
    return token;
}

} // namespace detail

namespace {

class EnvironmentCredentialStore final : public CredentialStore {
  public:
    [[nodiscard]] core::Result<std::string> read() const override
    {
        const std::string name(kTokenEnvironmentVariable);
        std::string raw = core::environmentVariable(name.c_str());
        const std::string_view trimmed = detail::trimmedToken(raw);
        if (trimmed.empty()) {
            // Unset, empty, or only white space: the normal state, not a fault.
            detail::secureZero(raw);
            return core::makeError(core::ErrorCode::NotFound,
                                   "no token: " + name + " is not set", name);
        }
        const core::Result<std::string_view> usable = detail::usableToken(trimmed, name);
        if (!usable) {
            // The same error with a sentence that says where the value is: the caller shows it
            // to a person who must know which variable to correct. The text of the value is
            // never in it.
            core::Error error = usable.error();
            error.code = core::ErrorCode::InvalidState;
            error.message = name + " is set but is not a usable token: " + error.message;
            detail::secureZero(raw);
            return error;
        }
        std::string token(*usable);
        detail::secureZero(raw);
        return token;
    }

    [[nodiscard]] core::Status write(std::string_view) override
    {
        return core::makeError(core::ErrorCode::Unsupported,
                               "the token is read from the environment variable " +
                                   std::string(kTokenEnvironmentVariable) +
                                   ", which Katana does not set: set it in the environment that "
                                   "starts Katana",
                               "environment");
    }

    [[nodiscard]] core::Status clear() override
    {
        return core::makeError(core::ErrorCode::Unsupported,
                               "the token is read from the environment variable " +
                                   std::string(kTokenEnvironmentVariable) +
                                   ", which Katana does not change: remove it from the "
                                   "environment that starts Katana",
                               "environment");
    }

    [[nodiscard]] std::string_view where() const override { return "environment"; }
};

} // namespace

std::unique_ptr<CredentialStore> makeEnvironmentCredentialStore()
{
    return std::make_unique<EnvironmentCredentialStore>();
}

#ifndef _WIN32
// There is no keyring in this version: elsewhere the environment is the only place a token can
// come from (credential_win.cpp has the Credential Manager's store).
std::unique_ptr<CredentialStore> makeSystemCredentialStore()
{
    return makeEnvironmentCredentialStore();
}
#endif

std::string redactToken(std::string_view text, std::string_view token)
{
    if (token.empty()) {
        return std::string(text);
    }
    constexpr std::string_view kMask = "***";
    // Every occurrence is covered, and occurrences that overlap are covered together: with the
    // token "aa", "aaa" holds two, and masking only the first would leave the third "a" - part of
    // the token - in the line. Occurrences that merely touch ("tokentoken") are two masks.
    std::vector<std::pair<std::size_t, std::size_t>> covered; // [begin, end) of each mask
    for (std::size_t from = 0;;) {
        const std::size_t hit = text.find(token, from);
        if (hit == std::string_view::npos) {
            break;
        }
        if (!covered.empty() && hit < covered.back().second) {
            covered.back().second = hit + token.size();
        } else {
            covered.emplace_back(hit, hit + token.size());
        }
        from = hit + 1;
    }
    std::string redacted;
    redacted.reserve(text.size());
    std::size_t copied = 0; // text before this has been written out
    for (const auto& [begin, end] : covered) {
        redacted.append(text.substr(copied, begin - copied));
        redacted.append(kMask);
        copied = end;
    }
    redacted.append(text.substr(copied));
    return redacted;
}

} // namespace katana::update
