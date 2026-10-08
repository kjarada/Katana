#pragma once

// Deleting a transaction's folder, through FsOps: INTERNAL to the update library and never
// installed. The apply deletes the folder of a transaction that ended (healthy, or rolled
// back), and recovery sweeps what a crash left of one; both go through this.
//
// It never follows a link: a link is removed, not entered, so a junction planted inside the
// folder cannot make the clean-up delete what the junction points at. Whatever cannot be
// deleted - a file something holds open - is reported in `leftovers`, not as a failure: a
// folder that will not go is not a half-swapped tree.
//
// `last` names one top-level entry to delete after all the rest. The journal goes last, so
// that a crash part way leaves a folder that says what it was (a journal in HEALTHY or
// ROLLED_BACK) and not an anonymous heap.

#include <cstddef>
#include <filesystem>
#include <optional>
#include <string_view>
#include <vector>

#include "katana/core/path_text.hpp"
#include "katana/update/fsops.hpp"

namespace katana::update::detail {

inline void removeTree(FsOps& fs, const std::filesystem::path& directory,
                       std::vector<std::filesystem::path>& leftovers, std::string_view last = {})
{
    const core::Result<EntryInfo> info = fs.stat(directory);
    if (!info.ok()) {
        leftovers.push_back(directory);
        return;
    }
    if (info->kind == EntryKind::Missing) {
        return;
    }
    if (info->kind != EntryKind::Directory) {
        if (!fs.removeFile(directory).ok()) {
            leftovers.push_back(directory);
        }
        return;
    }
    if (info->reparsePoint) {
        if (!fs.removeEmptyDirectory(directory).ok() && !fs.removeFile(directory).ok()) {
            leftovers.push_back(directory);
        }
        return;
    }
    const core::Result<std::vector<std::filesystem::path>> children = fs.listDirectory(directory);
    if (!children.ok()) {
        leftovers.push_back(directory);
        return;
    }
    const std::size_t before = leftovers.size();
    std::optional<std::filesystem::path> deferred;
    for (const std::filesystem::path& child : children.value()) {
        if (!last.empty() && child.filename() == core::pathFromUtf8(last)) {
            deferred = child;
            continue;
        }
        removeTree(fs, child, leftovers);
    }
    // The journal goes only when everything else has: while a file is still held, it stays and
    // keeps saying what the folder is.
    if (deferred && leftovers.size() == before) {
        removeTree(fs, *deferred, leftovers);
    }
    if (!fs.removeEmptyDirectory(directory).ok()) {
        leftovers.push_back(directory);
    }
}

} // namespace katana::update::detail
