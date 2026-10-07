// STUB. Task T13 writes the Windows Credential Manager store of
// include/katana/update/credential.hpp: CredReadW, CredWriteW and CredDeleteW on
// the target kCredentialTarget, the token kept as UTF-8 bytes, never logged.
// CMake compiles this file on Windows only.

#include "katana/update/credential.hpp"

#include "not_implemented.hpp"

namespace katana::update {

std::unique_ptr<CredentialStore> makeSystemCredentialStore()
{
    return std::make_unique<detail::UnimplementedCredentialStore>("credential manager", "T13");
}

} // namespace katana::update
