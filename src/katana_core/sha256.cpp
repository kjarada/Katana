// STUB. The declaration in include/katana/core/sha256.hpp is final; task T2
// replaces this file with FIPS 180-4 (the code now in gis::sha256Hex,
// src/katana_io/web_access.cpp, moved and made streaming) and then makes the
// gis function forward here. Until then nothing may call it for a digest: it
// throws, as core's Result does on misuse, rather than return one that is
// wrong.

#include "katana/core/sha256.hpp"

#include <stdexcept>

namespace katana::core {

namespace {

[[noreturn]] void notYet(const char* what)
{
    throw std::logic_error(std::string("not implemented: ") + what +
                           " (skeleton stub, task T2 writes core/sha256.cpp)");
}

} // namespace

Sha256::Sha256() = default;

void Sha256::update(std::string_view)
{
    notYet("Sha256::update");
}

Sha256Digest Sha256::finish()
{
    notYet("Sha256::finish");
}

std::string Sha256::finishHex()
{
    notYet("Sha256::finishHex");
}

void Sha256::reset()
{
    notYet("Sha256::reset");
}

std::string sha256Hex(std::string_view)
{
    notYet("sha256Hex");
}

Result<std::string> sha256FileHex(const std::filesystem::path& path)
{
    return makeError(ErrorCode::Unsupported,
                     "not implemented: sha256FileHex (skeleton stub, task T2)", path.string());
}

Result<Sha256Digest> parseSha256Hex(std::string_view)
{
    return makeError(ErrorCode::Unsupported,
                     "not implemented: parseSha256Hex (skeleton stub, task T2)");
}

} // namespace katana::core
