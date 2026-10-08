#pragma once

// The signed manifest of one release and one target: what every install file
// must hash to, and where each file's bytes are in the release's payload zip.
// The unit of update is the FILE, decided by SHA-256, never the version.
//
// On the wire (UTF-8, LF, keys sorted, files sorted by path), for example:
//
//   {"applier":1,"channel":"stable","expires":"2027-04-18T08:00:00Z","files":[
//    {"class":"program","csize":15500000,"exec":true,"offset":1234,
//     "path":"bin/katana.exe","sha256":"<64 hex>","size":42312704}, ...],
//    "notes_url":"https://github.com/<repo>/releases/tag/v0.3.0",
//    "payload":{"codec":"deflate","name":"Katana-0.3.0-win64.update.zip","size":212345678},
//    "product":"katana","published":"2026-10-20T08:00:00Z","schema":1,
//    "sequence":1792312800,"tag":"v0.3.0","target":"win64","version":"0.3.0"}
//
// It is signed as those exact bytes (signature.hpp, trust.hpp) and PARSED ONLY
// AFTER the signature has verified: parseManifest below is for trust.cpp (and
// for tests of the parser itself), and nothing that reads a network answer
// may call it on bytes it has not verified.
//
// Members this contract adds to the design's sample (T1's decisions, to be
// kept in step by the producer, tools/update_make_manifest.py):
//   "notify_only": true   a manifest that announces a release without
//                         offering to install it (the macOS bundle cannot be
//                         patched file by file). It has "files":[] and no
//                         "payload". Absent means false.
// Unknown members are ignored, so a newer producer may add more.

#include <cstdint>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/update/version.hpp"

namespace katana::update {

// How a file is treated at the swap. The helper commits data, then lib, then
// program (so a program that cannot load is the last thing replaced); a seed
// is written only when the file is absent, so a user's edit survives.
enum class FileClass { Program, Lib, Data, Seed };

[[nodiscard]] std::string_view toString(FileClass fileClass); // "program", "lib", "data", "seed"
[[nodiscard]] core::Result<FileClass> parseFileClass(std::string_view text);

struct ManifestFile {
    std::string path;               // root-relative, '/', passes paths.hpp
    FileClass fileClass = FileClass::Lib;
    bool executable = false;        // "exec": set the execute bit where there is one
    std::uint64_t size = 0;         // bytes once inflated
    std::uint64_t compressedSize = 0; // "csize": bytes of raw deflate data in the payload
    std::uint64_t offset = 0;       // where that deflate data starts in the payload zip
    std::string sha256;             // 64 lower-case hex digits of the inflated bytes
};

struct Payload {
    std::string codec;              // "deflate"
    std::string name;               // the asset, Katana-<ver>-<token>.update.zip
    std::uint64_t size = 0;         // the whole zip; it is never fetched whole
};

struct Manifest {
    std::uint32_t schema = 1;
    std::uint32_t applier = 1;      // the minimum helper protocol that can apply it
    std::string channel;            // "stable"
    std::string published;          // UTC, RFC 3339
    std::string expires;            // advisory: "the information is old", never a refusal
    std::string notesUrl;
    std::optional<Payload> payload; // absent only when notifyOnly
    std::string product;            // "katana"
    // UTC epoch seconds at build: a re-run outranks its first attempt.
    std::uint64_t sequence = 0;
    std::string tag;                // "v0.3.0"
    std::string target;             // win64 win-arm64 linux-x86_64 linux-aarch64 macos-arm64
    Version version;
    bool notifyOnly = false;
    std::vector<ManifestFile> files; // sorted by path, none reserved, all pass paths.hpp

    // Set by verifyManifest only (parseManifest leaves them empty): the exact
    // bytes that were signed and the signature line that covered them. The
    // helper writes them beside the install as installed.manifest.json(.sig),
    // the record a later update decides removals from.
    std::string signedJson;
    std::string signatureLine;
};

// Parses and validates a manifest: well-formed JSON of the shape above; schema
// 1; every file passing checkManifestPaths; hashes of the right form (64 lower-case
// hex); the version parsing; payload present unless notifyOnly; files sorted and
// unique. And, for every file, where its bytes are in the payload, so that nothing
// that is signed can send a request outside the zip or make a range the length of
// the whole disk:
//   - csize >= 2: a deflate stream is never empty, even for an empty file (the
//     shortest, an empty final block, is the two bytes 03 00; Python's zipfile
//     writes exactly that, measured);
//   - offset + csize <= payload.size, computed so that it cannot wrap (offset <=
//     payload.size and csize <= payload.size - offset); a notify-only manifest has
//     no files, so there is nothing to place;
//   - size <= kMaxInstallBytes for each file (the sum is checkManifestPaths's).
// Every member of the sample above is required with its exact type (a whole number is
// not "5.0" or "1e3"), except "exec" (absent is false: the producer writes it only when
// true), "notify_only" (absent is false) and "payload" (absent exactly when notify-only).
// Members it does not know are ignored; a member written twice, nesting deeper than 16
// levels, a byte-order mark, a NUL byte anywhere in the text (a JSON reader takes it for the
// end of its input) and an input over kMaxManifestBytes (feed.hpp) are refused.
// Beyond what the header says: sequence >= 1; product, channel, target and tag are 1 to 64
// letters, digits or . _ - (that they are THIS install's is verifyManifest's); notes_url is
// https://; published and expires are real UTC times (see isExpired: expiry is never an
// error); payload.codec is "deflate" and payload.size is 1 to 2^63-1; a manifest that
// installs lists at least one file and a notify-only one none; `applier` is any whole
// number from 1 to 2^32-1 (one above kApplierProtocol is not refused: the apply code
// decides to use the staged helper).
// ParseFailure for malformed JSON or a missing or mistyped member; InvalidArgument for a
// value that breaks a rule. The context names the member (files[3].path).
[[nodiscard]] core::Result<Manifest> parseManifest(std::string_view json);

// "YYYY-MM-DDTHH:MM:SSZ" (UTC, as the producer writes `published` and `expires`) as seconds
// since 1970-01-01T00:00:00Z. nullopt unless it is exactly that form and a real date from
// 1970 (so 2027-02-29 and a month 13 are not times, and neither is a leap second, :60).
[[nodiscard]] std::optional<std::int64_t> parseUtcTime(std::string_view text);

// `expires` is advisory (design section 1): when it has passed, the dialog says the
// information is old and the update is still offered. So this is a question for the
// caller to word, never a refusal: false while `nowEpoch` is not after `expires`, and
// false when `expires` cannot be read (parseManifest has already refused such a time).
[[nodiscard]] bool isExpired(const Manifest& manifest, std::int64_t nowEpoch);

} // namespace katana::update
