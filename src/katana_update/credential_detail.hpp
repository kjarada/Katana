#pragma once

// PRIVATE to src/katana_update and to tests/update/test_credential.cpp: what the two credential
// sources (credential_env.cpp, credential_win.cpp) share between themselves, and the seams of the
// Windows store that a test needs. Not under include/ because nothing outside the update layer's
// own sources and its test has a reason to name any of it; the public contract is
// katana/update/credential.hpp. One declaration here replaces the copies each file used to
// carry, which the linker could not hold to the definition (it holds the parameter types, and in
// the Itanium and MinGW schemes not the return type).

#include <cstddef>
#include <memory>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"
#include "katana/update/credential.hpp"

namespace katana::update::detail {

// The most bytes a token may have. The Windows Credential Manager refuses a blob over
// 5 * 512 = 2560 bytes (CRED_MAX_CREDENTIAL_BLOB_SIZE in the Windows SDK's wincred.h; mingw-w64's
// copy says 512, which is not what the system enforces - measured on Windows 11 on 2026-10-08:
// 2560 accepted, 2561 refused with error 1783), and a token the store could not hold is not one
// to accept from the environment either. GitHub advises storing its tokens in 255 characters (its
// 2021 note on the new token formats), far inside this.
inline constexpr std::size_t kMaxTokenBytes = 2560;

// `token` itself when it can be used as a token, else why not (InvalidArgument; the callers
// re-code it InvalidState when what they read is not usable). The caller trims it first
// (core::trimmed: a token pasted into a field or exported from `$(cat file)` arrives with a
// newline, and none of those characters can be part of one). A token goes into an HTTP header
// (`Authorization: Bearer <token>`), so a control character - 0x00 to 0x1F and 0x7F, above all a
// carriage return or a line feed, which would end the header and begin another - is refused
// wherever the token comes from. So is a space inside it: a token is one word, and a person who
// pastes "Bearer ghp_..." would otherwise send "Authorization: Bearer Bearer ghp_..." and meet a
// 401 that says nothing of the cause. Non-ASCII text is allowed: the store keeps whatever valid
// UTF-8 it is given, and the server is the judge of what it accepts. `source` goes into the
// error's context and must not be the token. Defined in credential_env.cpp, which is compiled on
// every platform.
[[nodiscard]] core::Result<std::string_view> usableToken(std::string_view token,
                                                         std::string_view source);

#ifdef _WIN32

// The same store under another target name, so that a test never touches the one a person's
// token is in. `target` is UTF-8, not empty, at most 32767 characters
// (CRED_MAX_GENERIC_TARGET_NAME_LENGTH in wincred.h) - a precondition, std::invalid_argument when
// broken. Defined in credential_win.cpp.
[[nodiscard]] std::unique_ptr<CredentialStore> makeCredentialManagerStore(std::string_view target);

// What the Win32 error `code` of a Credential Manager call means to a caller, as an Error that
// names `action` ("read", "store", "remove") and carries `target` as its context. The codes that
// mean "the Credential Manager cannot be used in this session", as opposed to a fault in what was
// asked, are Unsupported: a service or batch logon with no profile loaded, a locked-down or
// stripped Windows image. A caller (the update window) says so in a sentence, and a test skips.
// The numbers are from Microsoft's "System Error Codes" list:
//   5 ERROR_ACCESS_DENIED, 50 ERROR_NOT_SUPPORTED, 120 ERROR_CALL_NOT_IMPLEMENTED,
//   1312 ERROR_NO_SUCH_LOGON_SESSION, 1355 ERROR_NO_SUCH_DOMAIN.
// Every other code is Internal. The number is in the message, so access denied is not hidden by
// being called "not available". A seam for a test, which cannot make a session without a
// Credential Manager. `code` is a DWORD, which is `unsigned long` on every Windows toolchain;
// <windows.h> stays out of this header.
[[nodiscard]] core::Error credentialError(unsigned long code, std::string_view action,
                                          const std::string& target);

// The wide target name `store` passes to the system, or empty when `store` is not a Credential
// Manager store. A seam for a test, which checks the name a person's `cmdkey` command has to use
// without reading or writing the credential under it.
[[nodiscard]] std::wstring credentialTargetOf(const CredentialStore& store);

#endif // _WIN32

} // namespace katana::update::detail
