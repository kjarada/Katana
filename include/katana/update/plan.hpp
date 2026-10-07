#pragma once

// What an update would change, worked out by hashing the installed files
// FRESH - never from a cache: a stale cache would skip a file that changed,
// and the swap would then ship a mixed tree. A worker runs it when the dialog
// shows a newer version and again when the person presses Update.
//
// A file is KEPT when the installed file exists and its SHA-256 equals the
// manifest's. Otherwise it is CHANGED and is fetched. A `seed` file is
// changed only when it is absent (an edit by the user survives). A file
// listed in the manifest that cannot be read is changed.
//
// Removals are not planned here: only the helper decides them, from the
// installed manifest it verified itself (apply.hpp), never from anything the
// engine computed.

#include <cstddef>
#include <cstdint>
#include <stop_token>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/update/install.hpp"
#include "katana/update/manifest.hpp"
#include "katana/update/ranges.hpp"

namespace katana::update {

// Free space needed, in changed bytes: the staged copy, the copy under new/, and old/.
inline constexpr double kFreeSpaceFactor = 2.6;

struct PlannedFile {
    std::size_t index = 0;   // into Manifest::files
    bool changed = false;
};

struct Plan {
    std::vector<PlannedFile> files;       // one per manifest file, in manifest order
    std::uint64_t totalFiles = 0;
    std::uint64_t changedFiles = 0;
    std::uint64_t changedBytes = 0;       // sum of `size` of the changed files
    // Sum of `compressedSize` of the changed files: what the dialog quotes.
    std::uint64_t downloadBytes = 0;
    std::uint64_t requiredFreeBytes = 0;  // ceil(kFreeSpaceFactor * changedBytes)
    std::vector<MergedRange> ranges;      // the requests that fetch the changed files
};

// Hashes every file under install.root that the manifest lists. Checks `stop`
// between files; cancelled -> InvalidState "cancelled". A root that is not a
// directory is NotFound. Cost: about 2 s per 500 MB on the owner's machine.
[[nodiscard]] core::Result<Plan> plan(const Manifest& manifest, const Install& install,
                                      std::stop_token stop);

} // namespace katana::update
