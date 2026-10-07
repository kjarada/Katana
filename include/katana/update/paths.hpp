#pragma once

// The rules a file name from a manifest must pass before anything is written
// under it (docs/update.md). One rule set, used by the engine when a manifest
// is verified and again by the helper program before it touches the install:
// the helper does not trust the engine's answer, only the same code.
//
// A manifest path is relative, uses '/', and its first component is one of
// kInstallRoots. It is refused when it
//   - is empty, absolute, or has a drive or a root ('C:', a leading '/');
//   - has an empty, "." or ".." component;
//   - contains a backslash, ':', any of * ? " < > |, or a control character
//     (code below 0x20 or 0x7F);
//   - has a component ending in '.' or ' ' (Windows drops them, so two names
//     become one);
//   - has a component that is a Windows device name (CON, PRN, AUX, NUL,
//     COM1-9, LPT1-9, with or without an extension, any case);
//   - has a component in 8.3 short form (contains '~' followed by digits);
//   - is one of the reserved names: install.json, installed.json,
//     installed.manifest.json, installed.manifest.json.sig, or has a
//     ".update" component. They are the updater's own records;
//   - is not valid UTF-8.
// And a whole manifest is refused when two paths are equal after case-folding
// (a file system that folds case would write one over the other), when it has
// more than kMaxFiles entries, or when the sizes add up to more than
// kMaxInstallBytes.

#include <array>
#include <cstddef>
#include <cstdint>
#include <span>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"

namespace katana::update {

inline constexpr std::array<std::string_view, 4> kInstallRoots{"bin", "share", "etc", "lib"};
inline constexpr std::size_t kMaxFiles = 10'000;
inline constexpr std::uint64_t kMaxInstallBytes = 2ull * 1024 * 1024 * 1024;

// One path against the rules above. InvalidArgument naming the rule; the
// context is the path.
[[nodiscard]] core::Status checkManifestPath(std::string_view path);

// Every path alone, then the whole: duplicates after case-folding and the
// count. `sizes` is parallel to `paths` and is checked against
// kMaxInstallBytes (and for overflow of the sum).
[[nodiscard]] core::Status checkManifestPaths(std::span<const std::string> paths,
                                              std::span<const std::uint64_t> sizes);

// True for the updater's own record names above, wherever they sit: such a
// file is never in a manifest and never removed.
[[nodiscard]] bool isReservedName(std::string_view path);

} // namespace katana::update
