// STUB. Task T5 writes Ed25519 verification over OpenSSL's EVP interface
// (include/katana/update/signature.hpp says what it must refuse beyond what
// OpenSSL does). OpenSSL is linked to this library already, statically
// (src/katana_update/CMakeLists.txt), so that the helper program has no DLL
// but the operating system's.

#include "katana/update/signature.hpp"

#include "not_implemented.hpp"

namespace katana::update {

std::string keyIdOf(const PublicKey&)
{
    detail::notYet("keyIdOf", "T5");
}

core::Result<PublicKey> parsePublicKeyHex(std::string_view)
{
    return detail::notImplemented("parsePublicKeyHex", "T5");
}

core::Result<SignatureLine> parseSignatureLine(std::string_view)
{
    return detail::notImplemented("parseSignatureLine", "T5");
}

core::Status verifyEd25519(const PublicKey&, std::string_view, const SignatureBytes&)
{
    return detail::notImplemented("verifyEd25519", "T5");
}

} // namespace katana::update
