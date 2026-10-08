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
//   - contains a byte outside printable ASCII (0x20 to 0x7E), so no non-ASCII name,
//     no control character and no DEL: ASCII only is a rule on purpose. What two
//     Unicode spellings of one name, or two cases of one non-ASCII letter, mean is
//     the file system's business and differs between NTFS, APFS and ext4 (normalisation,
//     case folding tables), so a rule that said "equal after folding" could be wrong on
//     the machine that applies it. Every file Katana ships has an ASCII name (checked
//     over the tree when this was written; the producer, tools/update_make_manifest.py,
//     refuses one that does not, so a release cannot be made that no install accepts);
//   - contains a backslash, ':', or any of * ? " < > |;
//   - has a component ending in '.' or ' ' (Windows drops them, so two names
//     become one);
//   - has a component that is a Windows device name (CON, PRN, AUX, NUL,
//     COM1-9, LPT1-9, with or without an extension, any case);
//   - has a component in 8.3 short form (contains '~' followed by digits);
//   - has a final component that is one of the reserved names - install.json,
//     installed.json, installed.manifest.json, installed.manifest.json.sig - or has
//     a ".update" component, ANY CASE: on NTFS and APFS share/katana/INSTALL.JSON is
//     install.json. They are the updater's own records. (The design's pattern
//     installed*.json is narrowed to these four literal names: the helper writes
//     exactly these, and a name outside the list, installed.json.bak, is a file like
//     any other.)
// And a whole manifest is refused when two paths are equal after ASCII case-folding
// (a file system that folds case would write one over the other), when a path is also
// the folder of another (bin/a and bin/a/b, also after folding: no volume holds both, so
// an install that applied the set would fail half way), when it has more than kMaxFiles
// entries, or when the sizes add up to more than kMaxInstallBytes. Only the letters A to Z
// fold: '[' and '{' differ by the same bit and are two names. The cost of the set checks is the
// total length of the paths times a logarithm, whatever the paths look like; no cap on the
// length of one path is part of the rules (the table and both Python tools would have to
// carry it), so a signed manifest may hold one path of megabytes and it is read in linear time.

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

// The three records the helper itself writes at commit and so journals (REPLACE, with the
// old copy under old/): they are reserved names, so checkManifestPath refuses them, yet a
// journal legitimately carries them.
inline constexpr std::array<std::string_view, 3> kRecordPaths{
    "share/katana/installed.json", "share/katana/installed.manifest.json",
    "share/katana/installed.manifest.json.sig"};

// What a path read back from a journal must pass (journal.hpp: the journal is not
// authenticated): checkManifestPath, or exactly one of kRecordPaths, spelled as the helper
// writes it. NOT compared ignoring case, unlike the reserved names: on a case-sensitive
// file system "Share/katana/installed.json" is another place, outside bin, share, etc and
// lib, and journal.hpp promises that a forged line cannot reach one. A record spelled in
// another case is refused here, and skipped like any forged line.
[[nodiscard]] core::Status checkJournalPath(std::string_view path);

// What the path of a DIRECTORY (an MKDIR record) read back from a journal must pass: the
// same as a file's under checkManifestPath, and also a bare install folder - "etc", "lib" -
// which a release that brings that folder for the first time makes, and which rollback must
// remove again. Never one of kRecordPaths (they are files). A bare folder stays refused by
// checkJournalPath, for the records that name a FILE: a forged "REMOVE bin" must not delete
// a whole install folder.
[[nodiscard]] core::Status checkJournalDirectory(std::string_view path);

// True for the updater's own record names above, wherever they sit and in any ASCII
// case ("Share/Katana/INSTALL.JSON" is one): such a file is never in a manifest and
// never removed. Takes a whole path or a bare name.
[[nodiscard]] bool isReservedName(std::string_view path);

} // namespace katana::update
