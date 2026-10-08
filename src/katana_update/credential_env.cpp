// The environment credential store, redactToken, and what both credential stores share:
// the one rule for what a token may be. include/katana/update/credential.hpp has the contract.
//
// A token is a secret, so nothing here puts one anywhere but into the Result a caller asked
// for: not into a message, a context, an exception or a log line. An error describes the
// token as "token" and gives its LENGTH at most. The buffers this file owns are zeroed before
// they are freed (the standard library may keep a copy a caller's string made; that is the
// interface's cost, a std::string).
//
// What read() can return, for a caller that looks a token up quietly (the automatic check, the
// `UPDATE TOKEN STATUS` verb): NotFound is the normal "none"; InvalidState is a value that is
// there and cannot be used (a line break in it, too long, a space inside); on Windows the
// Credential Manager adds Unsupported (not usable in this session) and Internal (a system
// fault). Every one of them means "no token to send" to such a caller; whether to say so, and
// whether an unusable variable falls back to the Credential Manager, is the caller's to decide.
// The header lists the same set.

#include "katana/update/credential.hpp"

#include "credential_detail.hpp"

#include <cstddef>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/core/path_text.hpp"
#include "katana/core/text.hpp"
#include "katana/core/text_encoding.hpp"

namespace katana::update {

namespace {

// Overwrites the text of `text` with zeros in a way the compiler may not remove as a dead store (a
// write through a volatile pointer is an observable side effect). Local to this file: the Windows
// store zeroes its own buffers with SecureZeroMemory.
void secureZero(std::string& text)
{
    volatile char* byte = text.data();
    for (std::size_t i = 0; i < text.size(); ++i) {
        byte[i] = '\0';
    }
}

} // namespace

namespace detail {

// The rule and the reasons for it are on the declaration (credential_detail.hpp).
core::Result<std::string_view> usableToken(std::string_view token, std::string_view source)
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
    if (token.find(' ') != std::string_view::npos) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "the token holds a space: give the token alone, without \"Bearer\" "
                               "or any other word before it",
                               std::string(source));
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
        const std::string_view trimmed = core::trimmed(raw);
        if (trimmed.empty()) {
            // Unset, empty, or only white space: the normal state, not a fault.
            secureZero(raw);
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
            secureZero(raw);
            return error;
        }
        std::string token(*usable);
        secureZero(raw);
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
