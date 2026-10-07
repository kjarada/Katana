// The Windows Credential Manager store of include/katana/update/credential.hpp. CMake compiles
// this file on Windows only.
//
// One generic credential (CRED_TYPE_GENERIC) under the target name "Katana/update-token",
// persisted for the machine (CRED_PERSIST_LOCAL_MACHINE: it survives log-off and reboot, and is
// not roamed to other machines, as CRED_PERSIST_ENTERPRISE would). The token is the blob, as
// UTF-8 bytes. Read also takes a blob that is UTF-16LE text, because that is what `cmdkey
// /generic:Katana/update-token /user:x /pass:<token>` writes (measured on Windows 11,
// 2026-10-08: the password "abc" is stored as 61 00 62 00 63 00).
// UTF-8 text has no NUL inside it, so a blob that has one inside is UTF-16, and the two cannot be
// confused (blobText).
//
// The token is never logged and never in an error. The blob this file builds and the one the
// system hands back are zeroed before they are released.

#include "katana/update/credential.hpp"

#include <cstddef>
#include <memory>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/path_text.hpp"
#include "katana/core/text_encoding.hpp"

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#include <wincred.h>

namespace katana::update {

namespace detail {

// credential_env.cpp has the one rule for what a token may be. Declared here by hand because the
// header that would carry them is not this file's to change; the linker is what holds the two
// signatures to each other.
[[nodiscard]] std::string_view trimmedToken(std::string_view raw);
[[nodiscard]] core::Result<std::string_view> usableToken(std::string_view token,
                                                         std::string_view source);
void secureZero(std::string& text);

// The same store under another target name, so that a test never touches the one a person's
// token is in. Not in the header: nothing but a test has a reason to name another target. `target`
// is UTF-8, not empty, at most 32767 characters (CRED_MAX_GENERIC_TARGET_NAME_LENGTH in
// wincred.h) - a precondition, std::invalid_argument when broken.
[[nodiscard]] std::unique_ptr<CredentialStore> makeCredentialManagerStore(std::string_view target);

} // namespace detail

namespace {

// Error codes that mean "the Credential Manager cannot be used in this session", as opposed to a
// fault in what was asked: a service or batch logon with no profile loaded, a locked-down or
// stripped Windows image. A caller (the update window) says so in a sentence, and a test skips.
// The numbers are from Microsoft's "System Error Codes" list:
//   5 ERROR_ACCESS_DENIED, 50 ERROR_NOT_SUPPORTED, 120 ERROR_CALL_NOT_IMPLEMENTED,
//   1312 ERROR_NO_SUCH_LOGON_SESSION, 1355 ERROR_NO_SUCH_DOMAIN.
[[nodiscard]] bool isUnavailable(DWORD code)
{
    switch (code) {
    case ERROR_ACCESS_DENIED:
    case ERROR_NOT_SUPPORTED:
    case ERROR_CALL_NOT_IMPLEMENTED:
    case ERROR_NO_SUCH_LOGON_SESSION:
    case ERROR_NO_SUCH_DOMAIN:
        return true;
    default:
        return false;
    }
}

[[nodiscard]] core::Error failure(DWORD code, std::string_view action, const std::string& target)
{
    const std::string number = std::to_string(static_cast<unsigned long>(code));
    if (isUnavailable(code)) {
        return core::makeError(core::ErrorCode::Unsupported,
                               "the Windows Credential Manager is not available in this session "
                               "(Windows error " + number + ")",
                               target);
    }
    return core::makeError(core::ErrorCode::Internal,
                           "the Windows Credential Manager could not " + std::string(action) +
                               " the token (Windows error " + number + ")",
                           target);
}

// What CredReadW allocated, zeroed and freed with CredFree.
struct CredentialFree {
    void operator()(CREDENTIALW* credential) const
    {
        if (credential->CredentialBlob != nullptr) {
            SecureZeroMemory(credential->CredentialBlob, credential->CredentialBlobSize);
        }
        CredFree(credential);
    }
};

// The text a blob holds, still unchecked. UTF-8 is what write() stores. A NUL is no part of UTF-8
// text, so a blob that has one inside it is UTF-16LE (cmdkey's form) and is decoded; NULs at the
// END only are the terminator of a C string and are dropped.
[[nodiscard]] core::Result<std::string> blobText(const BYTE* bytes, std::size_t size)
{
    std::string_view blob(reinterpret_cast<const char*>(bytes), size);
    std::string_view withoutTerminator = blob;
    while (!withoutTerminator.empty() && withoutTerminator.back() == '\0') {
        withoutTerminator.remove_suffix(1);
    }
    if (withoutTerminator.find('\0') == std::string_view::npos) {
        // No NUL inside: UTF-8 (and a one-character UTF-16 token, "a" as 61 00, which is "a" read
        // either way).
        return std::string(withoutTerminator);
    }
    // UTF-16LE: whole code units only, and a terminator some tools append is not part of the token.
    while (blob.size() >= 2 && blob[blob.size() - 1] == '\0' && blob[blob.size() - 2] == '\0') {
        blob.remove_suffix(2);
    }
    if (blob.size() % 2 != 0) {
        return core::makeError(core::ErrorCode::InvalidState,
                               "the stored token is neither UTF-8 nor UTF-16 text");
    }
    auto decoded = core::decodeTextAs(blob, core::TextEncoding::Utf16LittleEndian);
    if (!decoded) {
        return core::makeError(core::ErrorCode::InvalidState,
                               "the stored token is not well-formed UTF-16 text");
    }
    return std::move(decoded->text);
}

class CredentialManagerStore final : public CredentialStore {
  public:
    explicit CredentialManagerStore(std::string_view target)
        : targetUtf8_(target), target_(core::pathFromUtf8(target).wstring())
    {
        if (target.empty() || target_.empty() ||
            target_.size() > CRED_MAX_GENERIC_TARGET_NAME_LENGTH || !core::isValidUtf8(target)) {
            throw std::invalid_argument(
                "a credential target is non-empty UTF-8 of at most 32767 characters");
        }
    }

    [[nodiscard]] core::Result<std::string> read() const override
    {
        PCREDENTIALW found = nullptr;
        if (CredReadW(target_.c_str(), CRED_TYPE_GENERIC, 0, &found) == FALSE) {
            const DWORD code = GetLastError();
            if (code == ERROR_NOT_FOUND) {
                return core::makeError(core::ErrorCode::NotFound, "no token is stored",
                                       targetUtf8_);
            }
            return failure(code, "read", targetUtf8_);
        }
        const std::unique_ptr<CREDENTIALW, CredentialFree> credential(found);
        if (credential->CredentialBlobSize == 0) {
            return core::makeError(core::ErrorCode::NotFound,
                                   "no token is stored: the credential is empty", targetUtf8_);
        }
        core::Result<std::string> text =
            blobText(credential->CredentialBlob, credential->CredentialBlobSize);
        if (!text) {
            core::Error error = text.error();
            error.context = targetUtf8_;
            return error;
        }
        const std::string_view trimmed = detail::trimmedToken(*text);
        if (trimmed.empty()) {
            detail::secureZero(*text);
            return core::makeError(core::ErrorCode::NotFound,
                                   "no token is stored: the credential is empty", targetUtf8_);
        }
        const core::Result<std::string_view> usable = detail::usableToken(trimmed, targetUtf8_);
        if (!usable) {
            core::Error error = usable.error();
            error.code = core::ErrorCode::InvalidState;
            error.message = "the stored token is not usable: " + error.message;
            detail::secureZero(*text);
            return error;
        }
        std::string token(*usable);
        detail::secureZero(*text);
        return token;
    }

    [[nodiscard]] core::Status write(std::string_view token) override
    {
        const core::Result<std::string_view> usable =
            detail::usableToken(detail::trimmedToken(token), targetUtf8_);
        if (!usable) {
            return usable.error();
        }
        // CredWriteW takes a mutable pointer and copies what it points at; this copy is what is
        // zeroed after.
        std::vector<BYTE> blob(usable->begin(), usable->end());
        std::wstring target = target_;
        CREDENTIALW credential{};
        credential.Type = CRED_TYPE_GENERIC;
        credential.TargetName = target.data();
        credential.Comment =
            const_cast<LPWSTR>(L"Katana: read-only access token for update downloads");
        credential.CredentialBlobSize = static_cast<DWORD>(blob.size());
        credential.CredentialBlob = blob.data();
        credential.Persist = CRED_PERSIST_LOCAL_MACHINE;
        credential.UserName = const_cast<LPWSTR>(L"Katana");
        const BOOL stored = CredWriteW(&credential, 0);
        const DWORD code = stored != FALSE ? 0 : GetLastError();
        SecureZeroMemory(blob.data(), blob.size());
        if (stored == FALSE) {
            return failure(code, "store", targetUtf8_);
        }
        return {};
    }

    [[nodiscard]] core::Status clear() override
    {
        if (CredDeleteW(target_.c_str(), CRED_TYPE_GENERIC, 0) == FALSE) {
            const DWORD code = GetLastError();
            if (code == ERROR_NOT_FOUND) {
                return {}; // nothing was stored: the state asked for
            }
            return failure(code, "remove", targetUtf8_);
        }
        return {};
    }

    [[nodiscard]] std::string_view where() const override { return "credential manager"; }

  private:
    std::string targetUtf8_;
    std::wstring target_;
};

} // namespace

namespace detail {

std::unique_ptr<CredentialStore> makeCredentialManagerStore(std::string_view target)
{
    return std::make_unique<CredentialManagerStore>(target);
}

} // namespace detail

std::unique_ptr<CredentialStore> makeSystemCredentialStore()
{
    return detail::makeCredentialManagerStore(kCredentialTarget);
}

} // namespace katana::update
