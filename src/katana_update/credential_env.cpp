// STUB. Task T13 writes the credential stores of include/katana/update/credential.hpp:
// here the environment store (KATANA_UPDATE_TOKEN, read-only), redactToken, and
// - on every platform but Windows - makeSystemCredentialStore, which is the
// environment store (credential_win.cpp has the Credential Manager's).

#include "katana/update/credential.hpp"

#include "not_implemented.hpp"

namespace katana::update {

std::unique_ptr<CredentialStore> makeEnvironmentCredentialStore()
{
    return std::make_unique<detail::UnimplementedCredentialStore>("environment", "T13");
}

#ifndef _WIN32
std::unique_ptr<CredentialStore> makeSystemCredentialStore()
{
    return makeEnvironmentCredentialStore();
}
#endif

std::string redactToken(std::string_view, std::string_view)
{
    detail::notYet("redactToken", "T13");
}

} // namespace katana::update
