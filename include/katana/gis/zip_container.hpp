#pragma once

// A ZIP archive as a container of named byte strings.
//
// Here for one reason: a .12daz is a standard ZIP holding a single
// DEFLATE-compressed .12da (established from an archive written by 12d Model
// itself - member "Appin Rd V5.12da", method 8, made-by DOS). The 12da reader
// (katana_archive12d) deliberately links no third-party library, so the
// container is opened here, behind the same wall as everything else that
// needs GDAL, and only bytes cross it (PLAN.MD Rule 4).
//
// REJECTED: a small inflate of our own inside katana_archive12d. It would have
// made .12daz available with -DKATANA_BUILD_IO=OFF, which nobody needs - that
// configuration has no importer at all - at the price of a second
// implementation of something zlib already does, written by us, on untrusted
// input.

#include <cstdint>
#include <filesystem>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::gis {

struct ZipMember {
    std::string name;       // path within the archive, '/'-separated
    std::uint64_t size = 0; // uncompressed
};

// FileImportFailure when the file is missing or is not a ZIP.
[[nodiscard]] katana::core::Result<std::vector<ZipMember>>
listZip(const std::filesystem::path& archive);

// The uncompressed bytes of one member. A member larger than `maxBytes` is
// refused BEFORE it is inflated: the size comes from the archive's directory,
// and a few kilobytes of zip can claim to hold gigabytes.
[[nodiscard]] katana::core::Result<std::string> readZipMember(const std::filesystem::path& archive,
                                                              std::string_view member,
                                                              std::uint64_t maxBytes);

// Writes a NEW archive holding the one member, DEFLATE-compressed, replacing
// any file already at `archive`. It is written beside the target and renamed
// over it, so a failure half-way leaves the old file, not half a new one.
[[nodiscard]] katana::core::Status writeZip(const std::filesystem::path& archive,
                                            std::string_view memberName, std::string_view bytes);

} // namespace katana::gis
