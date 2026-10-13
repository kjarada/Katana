// stage() and preflight() of include/katana/update/stage.hpp.
//
// stage(): the plan says which manifest files differ; the ranges of the plan's members are
// fetched one request each; every member is inflated (capped at its signed size), hashed while
// it is written to a temporary name, and only a member whose SHA-256 is the signed one is renamed
// to its final place under <stageDir>. So a name under the stage folder is a verified file, at
// every instant, whatever the network, the disk or the user does; a cancelled or failed stage
// leaves some verified files and no half file, and the next run finds them by their hash and
// fetches only what is missing. The signed manifest and its signature are written last of all:
// their presence is "complete".
//
// Nothing here trusts what it fetched: a member is believed only by its hash, which is in the
// signed manifest, and the helper checks the staged files by hash again before it copies them
// (apply.hpp, step 3). That second check is also why a member is written by a plain stream and
// not flushed to the disk: a staged file a power cut left short fails its hash at the swap,
// nothing is touched, and the next download replaces it.
//
// The folder is worked through an FsOps (fsops.hpp), the system's unless one is given. A member
// is hashed while it is written, by a stream, so the FsOps must be one over the real disk; what
// it adds is the real file system's long paths and its patience with a virus scanner on Windows,
// and the tests' ability to run the same code on every platform and to break a write.
//
// "Hashed while it is written" is a statement about ONE writer: the hash is of the stream, and
// what is renamed is the file. So one stage() at a time fills a folder - a lock beside it, held
// for the whole call (stage.hpp, stageLockPath) - and a second is refused before it touches
// anything, rather than sharing the first one's temporary files.

#include "katana/update/stage.hpp"

#include <algorithm>
#include <charconv>
#include <cstdint>
#include <fstream>
#include <iterator>
#include <memory>
#include <optional>
#include <string>
#include <system_error>
#include <utility>
#include <vector>

#include "katana/core/path_text.hpp"
#include "katana/core/sha256.hpp"
#include "katana/update/fsops.hpp"
#include "katana/update/inflate.hpp"
#include "katana/update/ranges.hpp"
#include "katana/update/state.hpp"
#include "long_path.hpp"
#include "remove_tree.hpp"

namespace katana::update {

namespace {

namespace fs = std::filesystem;

// Where members are written before they are known to be right. A first component no manifest
// path can have (paths.hpp: bin, share, etc or lib), so it can never be the name of a file the
// stage is supposed to hold.
constexpr std::string_view kScratchFolder = ".partial";

// An Error of this file's own, with the reason token fetchFailureReason reads back.
core::Error stageError(core::ErrorCode code, std::string message, std::string_view reason,
                       std::string_view extra = {})
{
    std::string context = "reason=" + std::string(reason);
    if (!extra.empty()) {
        context += "; ";
        context += extra;
    }
    return core::makeError(code, std::move(message), std::move(context));
}

core::Error cancelledError()
{
    return stageError(core::ErrorCode::InvalidState, "cancelled", "cancelled");
}

// The stage folder refused something. `why` is the file system's own Error: its message is
// joined to the sentence, and its context (`os_error=<n>` and the path, fsops.hpp) follows the
// token, where a person looking for the cause finds the number.
core::Error writeFailed(std::string what, const core::Error& why,
                        std::string_view manifestPath = {})
{
    std::string extra;
    if (!manifestPath.empty()) {
        extra = "path=" + std::string(manifestPath);
    }
    if (!why.context.empty()) {
        if (!extra.empty()) {
            extra += "; ";
        }
        extra += why.context;
    }
    return stageError(core::ErrorCode::FileExportFailure, std::move(what) + ": " + why.message,
                      "write_failed", extra);
}

fs::path finalPathOf(const fs::path& stageDir, const ManifestFile& file)
{
    return stageDir / core::pathFromUtf8(file.path);
}

// The staged file at `path` is exactly the manifest's file: a plain file of the signed size
// whose SHA-256 is the signed one. Anything else - absent, a link, another size, another hash -
// is not staged. The hash is read from the disk every time: a file's name proves nothing.
bool isVerified(const FsOps& fsOps, const fs::path& path, const ManifestFile& file)
{
    const core::Result<EntryInfo> info = fsOps.stat(path);
    if (!info.ok() || info.value().kind != EntryKind::File || info.value().reparsePoint ||
        info.value().size != file.size) {
        return false;
    }
    const core::Result<std::string> hash = core::sha256FileHex(detail::openable(path));
    return hash.ok() && hash.value() == file.sha256;
}

// What FsOps::tryLock answers when another holder has the lock (fsops.hpp).
bool isHeld(const core::Error& error)
{
    return error.code == core::ErrorCode::InvalidState && error.message == "held";
}

// `name` is "stage-<n>" with n a number below `sequence`: the folder of a release this one
// outranks.
bool isOlderStageName(std::string_view name, std::uint64_t sequence)
{
    constexpr std::string_view kPrefix = "stage-";
    if (!name.starts_with(kPrefix) || name.size() == kPrefix.size() || name.size() > 40) {
        return false;
    }
    const std::string_view digits = name.substr(kPrefix.size());
    if (!std::all_of(digits.begin(), digits.end(), [](char c) { return c >= '0' && c <= '9'; })) {
        return false;
    }
    std::uint64_t older = 0;
    const auto parsed = std::from_chars(digits.data(), digits.data() + digits.size(), older);
    return parsed.ec == std::errc{} && older < sequence;
}

// Removes the folders `stage-<n>` beside the one being filled whose n is below this release's
// sequence - what a helper that died left, and the stage of every release this one outranks -
// and the lock file of each (stageLockPath). A folder whose lock another program holds is
// being filled by it now, and is left. A folder of a newer sequence, or any other name, is not
// touched. Best effort.
void sweepOlderStages(FsOps& fsOps, const fs::path& stageDir, std::uint64_t sequence)
{
    const fs::path parent = stageDir.parent_path();
    if (parent.empty()) {
        return;
    }
    const core::Result<std::vector<fs::path>> children = fsOps.listDirectory(parent);
    if (!children.ok()) {
        return;
    }
    // The folders to sweep, each once: named by themselves, or by a lock file that outlived its
    // folder (the helper deletes the folder of a release it installed, not the lock beside it).
    constexpr std::string_view kLockSuffix = ".lock";
    std::vector<fs::path> folders;
    for (const fs::path& child : children.value()) {
        std::string name = core::pathToUtf8(child.filename());
        if (name.ends_with(kLockSuffix)) {
            name.resize(name.size() - kLockSuffix.size());
        }
        if (!isOlderStageName(name, sequence)) {
            continue;
        }
        fs::path folder = parent / core::pathFromUtf8(name);
        const bool listed = std::find(folders.begin(), folders.end(), folder) != folders.end();
        if (folder != stageDir && !listed) {
            folders.push_back(std::move(folder));
        }
    }
    for (const fs::path& folder : folders) {
        const fs::path lockPath = stageLockPath(folder);
        {
            const core::Result<std::unique_ptr<FileLock>> lock = fsOps.tryLock(lockPath);
            if (!lock.ok() && isHeld(lock.error())) {
                continue;
            }
            // Only a folder: a file somebody named like one is not this program's.
            const core::Result<EntryInfo> info = fsOps.stat(folder);
            if (info.ok() && info.value().kind == EntryKind::Directory) {
                std::vector<fs::path> leftovers;
                detail::removeTree(fsOps, folder, leftovers);
            }
        }
        // Let go first: a lock that is held cannot be deleted, by its holder either.
        (void)fsOps.removeFile(lockPath);
    }
}

// Whatever stands in a staged file's place goes: nothing (the usual case), a file that failed
// its hash, or a folder somebody put there, with all that is in it. A link is removed, never
// entered (removeTree).
core::Status clearPlace(FsOps& fsOps, const fs::path& place)
{
    const core::Result<EntryInfo> there = fsOps.stat(place);
    if (!there.ok() || there.value().kind != EntryKind::Directory) {
        return fsOps.removeFile(place); // success when nothing is there
    }
    std::vector<fs::path> leftovers;
    detail::removeTree(fsOps, place, leftovers);
    if (!leftovers.empty()) {
        return core::makeError(core::ErrorCode::FileExportFailure,
                               "a folder in a staged file's place could not be removed",
                               "os_error=0 " + core::pathToUtf8(leftovers.front()));
    }
    return {};
}

// The names between the stage folder and a staged file must be folders of the stage. One that
// is something else - a file, or a link, which would send the member to wherever it points -
// goes, so that the folder can be made. Only names INSIDE `stageDir` are looked at: the walk
// is over the manifest path's own parts, never upwards from the file.
core::Status clearParents(FsOps& fsOps, const fs::path& stageDir, const ManifestFile& file)
{
    const fs::path relative = core::pathFromUtf8(file.path);
    fs::path folder = stageDir;
    for (auto part = relative.begin(); part != relative.end(); ++part) {
        if (std::next(part) == relative.end()) {
            break; // the file's own name
        }
        folder /= *part;
        const core::Result<EntryInfo> info = fsOps.stat(folder);
        if (!info.ok() || info.value().kind == EntryKind::Missing) {
            break; // nothing below it either; making the folders says what is wrong, if anything
        }
        if (info.value().kind == EntryKind::Directory && !info.value().reparsePoint) {
            continue;
        }
        std::vector<fs::path> leftovers;
        detail::removeTree(fsOps, folder, leftovers);
        if (!leftovers.empty()) {
            return core::makeError(core::ErrorCode::FileExportFailure,
                                   "what stands where a folder of the stage belongs could not "
                                   "be removed",
                                   "os_error=0 " + core::pathToUtf8(leftovers.front()));
        }
        break;
    }
    return {};
}

// The scratch folder goes however stage() ends: on success it is empty, on failure it holds the
// member that was being written.
class ScratchFolder {
  public:
    ScratchFolder(FsOps& fsOps, fs::path path) : fsOps_(fsOps), path_(std::move(path)) {}
    ScratchFolder(const ScratchFolder&) = delete;
    ScratchFolder& operator=(const ScratchFolder&) = delete;
    ~ScratchFolder()
    {
        std::vector<fs::path> leftovers;
        detail::removeTree(fsOps_, path_, leftovers);
    }
    [[nodiscard]] const fs::path& path() const { return path_; }

  private:
    FsOps& fsOps_;
    fs::path path_;
};

// Inflates `compressed` to the member's final place, if and only if it is the signed file.
core::Status stageMember(FsOps& fsOps, const fs::path& scratch, const fs::path& stageDir,
                         std::size_t index, const ManifestFile& file, std::string_view compressed,
                         std::stop_token stop)
{
    const fs::path temporary = scratch / std::to_string(index);
    const fs::path target = finalPathOf(stageDir, file);
    if (core::Status removed = fsOps.removeFile(temporary); !removed.ok()) {
        return writeFailed("the stage folder could not be prepared", removed.error(), file.path);
    }

    std::ofstream out(detail::openable(temporary), std::ios::binary | std::ios::trunc);
    if (!out) {
        return stageError(core::ErrorCode::FileExportFailure,
                          "a downloaded file could not be written to the stage folder",
                          "write_failed", "path=" + file.path);
    }
    core::Sha256 hasher;
    const InflateSink sink = [&](std::string_view block) -> core::Status {
        if (stop.stop_requested()) {
            return cancelledError();
        }
        hasher.update(block);
        out.write(block.data(), static_cast<std::streamsize>(block.size()));
        if (!out) {
            return stageError(core::ErrorCode::FileExportFailure,
                              "the stage folder could not be written (is the disk full?)",
                              "write_failed", "path=" + file.path);
        }
        return {};
    };
    const core::Status inflated = inflateRaw(compressed, file.size, sink);
    out.close();
    if (!inflated.ok()) {
        (void)fsOps.removeFile(temporary);
        const core::Error& why = inflated.error();
        // The sink's own failures (cancelled, the disk), and zlib failing to run at all, which
        // says nothing about the member: passed on as they are. Everything else is what the
        // inflater found wrong with the bytes.
        if (why.code == core::ErrorCode::InvalidState ||
            why.code == core::ErrorCode::FileExportFailure ||
            why.code == core::ErrorCode::Internal) {
            return why;
        }
        return stageError(core::ErrorCode::InvalidArgument,
                          "a downloaded file is damaged and could not be unpacked: " + why.message,
                          "staged_corrupt", "path=" + file.path);
    }
    if (out.fail()) {
        (void)fsOps.removeFile(temporary);
        return stageError(core::ErrorCode::FileExportFailure,
                          "the stage folder could not be written (is the disk full?)",
                          "write_failed", "path=" + file.path);
    }
    // The only comparison that matters: the bytes are the signed ones. Nothing unverified is
    // ever given a final name.
    if (hasher.finishHex() != file.sha256) {
        (void)fsOps.removeFile(temporary);
        return stageError(core::ErrorCode::InvalidArgument,
                          "a downloaded file is not the one the release signed", "staged_hash",
                          "path=" + file.path);
    }

    if (core::Status cleared = clearParents(fsOps, stageDir, file); !cleared.ok()) {
        (void)fsOps.removeFile(temporary);
        return writeFailed("the stage folder could not be prepared", cleared.error(), file.path);
    }
    if (core::Status made = fsOps.makeDirectories(target.parent_path()); !made.ok()) {
        (void)fsOps.removeFile(temporary);
        return writeFailed("the stage folder could not be prepared", made.error(), file.path);
    }
    // A rename never replaces (fsops.hpp). What is there is a file that failed its hash, or
    // something that was never the stage's - a folder, a link: whatever it is, it goes.
    if (core::Status removed = clearPlace(fsOps, target); !removed.ok()) {
        (void)fsOps.removeFile(temporary);
        return writeFailed("what is in a staged file's place could not be removed",
                           removed.error(), file.path);
    }
    if (core::Status moved = fsOps.renameFile(temporary, target); !moved.ok()) {
        (void)fsOps.removeFile(temporary);
        return writeFailed("a verified file could not be put in its place", moved.error(),
                           file.path);
    }
    if (file.executable) {
        if (core::Status marked = fsOps.setExecutable(target); !marked.ok()) {
            // The file stays: it is the signed bytes, and the swap and the preflight each mark
            // their own copy (apply.cpp, preflight below). But a folder that refuses this is
            // not one to go on writing into.
            return writeFailed("a staged program could not be made runnable", marked.error(),
                               file.path);
        }
    }
    return {};
}

// <stageDir>/<name>, through the scratch folder and a rename.
core::Status writeStagedRecord(FsOps& fsOps, const fs::path& scratch, const fs::path& target,
                               std::string_view bytes)
{
    const fs::path temporary = scratch / target.filename();
    if (core::Status written = fsOps.writeDurable(temporary, bytes); !written.ok()) {
        return writeFailed("the staged manifest could not be written", written.error());
    }
    if (core::Status removed = clearPlace(fsOps, target); !removed.ok()) {
        return writeFailed("an earlier staged manifest could not be removed", removed.error());
    }
    if (core::Status moved = fsOps.renameFile(temporary, target); !moved.ok()) {
        return writeFailed("the staged manifest could not be put in its place", moved.error());
    }
    return {};
}

// Where the payload is fetched from, and with what.
struct PayloadSource {
    std::string url;
    std::string accept;
    std::string bearerToken;
};

core::Result<PayloadSource> resolvePayloadSource(const Fetch& fetch, const FeedConfig& feed,
                                                 const Manifest& manifest, std::stop_token stop)
{
    PayloadSource source;
    if (feed.route == FeedRoute::Download) {
        source.url = payloadUrl(feed, manifest);
        return source;
    }
    // The private route: the release the manifest names (the one that is `latest` may have moved
    // on since the check), its assets, and the payload's id among them.
    if (feed.token.empty()) {
        return stageError(core::ErrorCode::InvalidArgument,
                          "the private-repository route needs an access token", "no_token");
    }
    FetchRequest lookup;
    lookup.url = releaseByTagUrl(feed, manifest.tag);
    lookup.accept = "application/vnd.github+json";
    lookup.bearerToken = feed.token;
    lookup.maxBytes = kMaxReleaseJsonBytes;
    lookup.userAgent = feed.userAgent;
    core::Result<std::string> release = fetch(lookup, stop, {});
    if (!release) {
        return release.error();
    }
    core::Result<std::vector<ReleaseAsset>> assets = parseReleaseAssets(release.value());
    if (!assets) {
        return stageError(core::ErrorCode::ParseFailure, assets.error().message, "malformed");
    }
    const auto at = std::find_if(assets.value().begin(), assets.value().end(),
                                 [&](const ReleaseAsset& asset) {
                                     return asset.name == manifest.payload->name;
                                 });
    if (at == assets.value().end()) {
        return stageError(core::ErrorCode::NotFound,
                          "the release has no file named " + manifest.payload->name,
                          "asset_missing");
    }
    // The size the listing gives is not signed; the signed one is what the ranges are held to.
    // A listing that disagrees says the asset is not the one the manifest describes.
    if (at->size != 0 && at->size != manifest.payload->size) {
        return stageError(core::ErrorCode::InvalidState,
                          "the release's file is not the length its manifest names; the release "
                          "may have been replaced",
                          "payload_mismatch");
    }
    source.url = assetApiUrl(feed, at->id);
    source.accept = "application/octet-stream";
    source.bearerToken = feed.token;
    return source;
}

#ifndef _WIN32
// Linux and macOS have no file system of the update's own yet (fsops_posix.cpp): every operation
// of the system's is one refusal, with the code Unsupported. Worked through, that refusal comes
// back as `write_failed` from stage() - a folder that could not be written - and as
// `helper_missing` from preflight(), neither of which is what is wrong. So the forms of both that
// use the system's file system put one question to it first and hand its refusal on as it is,
// under the token `platform`, before a folder is asked for or a request is sent. Where the file
// system is written stat() does not answer Unsupported, and nothing here stands in the way.
std::optional<core::Error> refusedByThePlatform(const FsOps& system, const fs::path& folder)
{
    const core::Result<EntryInfo> asked = system.stat(folder);
    if (asked.ok() || asked.error().code != core::ErrorCode::Unsupported) {
        return std::nullopt;
    }
    return stageError(core::ErrorCode::Unsupported, asked.error().message, "platform");
}
#endif

} // namespace

core::Status stage(const Fetch& fetch, const FeedConfig& feed, const Manifest& manifest,
                   const Plan& plan, const fs::path& stageDir, std::stop_token stop,
                   ProgressFn progress)
{
    const std::unique_ptr<FsOps> system = makeSystemFsOps();
#ifndef _WIN32
    if (const std::optional<core::Error> refused = refusedByThePlatform(*system, stageDir)) {
        return *refused;
    }
#endif
    return stage(fetch, feed, manifest, plan, stageDir, std::move(stop), std::move(progress),
                 *system);
}

core::Status stage(const Fetch& fetch, const FeedConfig& feed, const Manifest& manifest,
                   const Plan& plan, const fs::path& stageDir, std::stop_token stop,
                   ProgressFn progress, FsOps& fsOps)
{
    if (!fetch) {
        return core::makeError(core::ErrorCode::InvalidArgument, "stage was given no transport");
    }
    if (stageDir.empty()) {
        return core::makeError(core::ErrorCode::InvalidArgument, "stage was given no folder");
    }
    // Only what verifyManifest returned is staged: the helper is handed these bytes to verify
    // again, and a manifest that was never verified has none (manifest.hpp).
    if (manifest.signedJson.empty() || manifest.signatureLine.empty()) {
        return stageError(core::ErrorCode::InvalidArgument,
                          "only a verified manifest can be staged", "unverified_manifest");
    }
    if (manifest.notifyOnly || !manifest.payload) {
        return stageError(core::ErrorCode::Unsupported,
                          "this release is an announcement and has nothing to download",
                          "notify_only");
    }
    if (plan.files.size() != manifest.files.size()) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "the plan is not the plan of this manifest",
                               std::to_string(plan.files.size()) + " planned, " +
                                   std::to_string(manifest.files.size()) + " listed");
    }
    if (stop.stop_requested()) {
        return cancelledError();
    }

    if (core::Status made = fsOps.makeDirectories(stageDir); !made.ok()) {
        return writeFailed("the stage folder could not be created", made.error());
    }
    // One stage() at a time in this folder (stage.hpp): two would share its temporary files, and
    // each would delete the other's. Taken before anything in the folder is touched, and held to
    // the end of this function - it is declared before the scratch folder, which is therefore
    // emptied while the lock is still held.
    const core::Result<std::unique_ptr<FileLock>> lock = fsOps.tryLock(stageLockPath(stageDir));
    if (!lock.ok()) {
        if (isHeld(lock.error())) {
            return stageError(core::ErrorCode::AlreadyExists,
                              "another copy of Katana is downloading this update",
                              "stage_busy");
        }
        return writeFailed("the stage folder could not be locked", lock.error());
    }
    // A staged manifest says "complete" (stage.hpp); this run has not completed yet, so a
    // record of an earlier one must not keep saying so if this one fails.
    for (const fs::path& stale : {stagedManifestPath(stageDir), stagedSignaturePath(stageDir)}) {
        if (core::Status removed = clearPlace(fsOps, stale); !removed.ok()) {
            return writeFailed("an earlier staged manifest could not be removed",
                               removed.error());
        }
    }
    sweepOlderStages(fsOps, stageDir, manifest.sequence);

    ScratchFolder scratch(fsOps, stageDir / core::pathFromUtf8(kScratchFolder));
    {
        std::vector<fs::path> leftovers;
        detail::removeTree(fsOps, scratch.path(), leftovers); // a crashed run's member
    }
    if (core::Status made = fsOps.makeDirectories(scratch.path()); !made.ok()) {
        return writeFailed("the stage folder could not be prepared", made.error());
    }

    // What is already staged and right is skipped: the resume.
    std::vector<RangeMember> missing;
    std::uint64_t stagedBytes = 0;      // size of the files already there
    std::uint64_t stagedCompressed = 0; // csize of the same
    for (const PlannedFile& planned : plan.files) {
        if (!planned.changed) {
            continue;
        }
        if (planned.index >= manifest.files.size()) {
            return core::makeError(core::ErrorCode::InvalidArgument,
                                   "the plan names a file the manifest does not list",
                                   std::to_string(planned.index));
        }
        const ManifestFile& file = manifest.files[planned.index];
        if (isVerified(fsOps, finalPathOf(stageDir, file), file)) {
            stagedBytes += file.size;
            stagedCompressed += file.compressedSize;
        } else {
            missing.push_back(RangeMember{planned.index, file.offset, file.compressedSize});
        }
        if (stop.stop_requested()) {
            return cancelledError();
        }
    }

    // Free space first (design, section 2): 2.6 times the changed bytes, less what the stage
    // already holds, which is on the disk and so already out of the figure the volume reports.
    const std::uint64_t needed =
        plan.requiredFreeBytes > stagedBytes ? plan.requiredFreeBytes - stagedBytes : 0;
    if (needed > 0) {
        const core::Result<std::uint64_t> free = fsOps.freeSpace(stageDir);
        if (free.ok() && free.value() < needed) {
            return stageError(core::ErrorCode::FileExportFailure,
                              "there is not enough free disk space to download the update",
                              "low_disk_space",
                              "need=" + std::to_string(needed) +
                                  " bytes; free=" + std::to_string(free.value()) + " bytes");
        }
        // An unreadable figure does not stop the download: the writes will say if the disk is
        // full.
    }

    const std::vector<MergedRange> ranges = mergeRanges(missing);
    std::uint64_t total = stagedCompressed;
    for (const MergedRange& range : ranges) {
        total += range.length;
    }
    // Never backwards, never past the total: the transport starts a request again after a
    // dropped connection, and its count with it.
    std::uint64_t shown = 0;
    const auto report = [&](std::uint64_t done) {
        shown = std::max(shown, std::min(done, total));
        if (progress) {
            progress(shown, total);
        }
    };
    report(stagedCompressed);

    if (!ranges.empty()) {
        core::Result<PayloadSource> source = resolvePayloadSource(fetch, feed, manifest, stop);
        if (!source) {
            return source.error();
        }
        std::uint64_t done = stagedCompressed;
        for (const MergedRange& range : ranges) {
            if (stop.stop_requested()) {
                return cancelledError();
            }
            FetchRequest request;
            request.url = source.value().url;
            request.rangeStart = range.offset;
            request.rangeLength = range.length;
            request.payloadSize = manifest.payload->size;
            // The server may not send more than was asked for: a server that ignores `Range`
            // and starts the whole payload is cut off after this many bytes.
            request.maxBytes = range.length;
            request.accept = source.value().accept;
            request.bearerToken = source.value().bearerToken;
            request.userAgent = feed.userAgent;
            const std::uint64_t base = done;
            const core::Result<std::string> body =
                fetch(request, stop,
                      [&](std::uint64_t received, std::uint64_t) {
                          report(base + std::min(received, range.length));
                      });
            if (!body) {
                return body.error();
            }
            if (body.value().size() != range.length) {
                return stageError(core::ErrorCode::FileImportFailure,
                                  "the download server sent " + std::to_string(body.value().size()) +
                                      " bytes for a range of " + std::to_string(range.length),
                                  "short_range");
            }
            for (const RangeMember& member : range.members) {
                if (stop.stop_requested()) {
                    return cancelledError();
                }
                // mergeRanges puts every member inside its range; checked all the same, for it
                // is the line between the server's bytes and a read past them.
                if (member.offset < range.offset ||
                    member.offset - range.offset > range.length ||
                    member.length > range.length - (member.offset - range.offset)) {
                    return core::makeError(core::ErrorCode::Internal,
                                           "a file lies outside the range that holds it",
                                           manifest.files[member.fileIndex].path);
                }
                const std::string_view slice = std::string_view(body.value())
                                                   .substr(member.offset - range.offset,
                                                           member.length);
                if (core::Status staged =
                        stageMember(fsOps, scratch.path(), stageDir, member.fileIndex,
                                    manifest.files[member.fileIndex], slice, stop);
                    !staged.ok()) {
                    return staged;
                }
            }
            done += range.length;
            report(done);
        }
    }
    if (stop.stop_requested()) {
        return cancelledError();
    }

    // Last: the signed manifest, then its signature. Their presence is "complete".
    if (core::Status written = writeStagedRecord(fsOps, scratch.path(),
                                                 stagedManifestPath(stageDir),
                                                 manifest.signedJson);
        !written.ok()) {
        return written;
    }
    if (core::Status written = writeStagedRecord(fsOps, scratch.path(),
                                                 stagedSignaturePath(stageDir),
                                                 manifest.signatureLine);
        !written.ok()) {
        return written;
    }
    if (core::Status synced = fsOps.syncDirectory(stageDir); !synced.ok()) {
        return writeFailed("the stage folder could not be flushed to the disk", synced.error());
    }
    report(total);
    return {};
}

// ---- preflight --------------------------------------------------------------------------------

namespace {

constexpr std::size_t kOutputShown = 240;

// The first words of what the helper printed, for the reason: bounded, printable.
std::string excerptOf(std::string_view output)
{
    std::string out;
    for (const char c : output.substr(0, kOutputShown)) {
        const auto byte = static_cast<unsigned char>(c);
        out.push_back(byte >= 0x20 && byte < 0x7F ? c : ' ');
    }
    return out;
}

// The run folder goes, and what a lock keeps there is left for the next start (state.hpp).
void removeRunFolder(FsOps& fsOps, const fs::path& stateDir, const fs::path& runDir)
{
    std::vector<fs::path> leftovers;
    detail::removeTree(fsOps, runDir, leftovers);
    for (const fs::path& leftover : leftovers) {
        (void)appendCleanup(stateDir, leftover, fsOps);
    }
}

} // namespace

core::Result<PreparedApply> preflight(const Manifest& manifest, const Install& install,
                                      const fs::path& stageDir, const fs::path& stateDir,
                                      Launcher& launcher)
{
    const std::unique_ptr<FsOps> system = makeSystemFsOps();
#ifndef _WIN32
    if (const std::optional<core::Error> refused = refusedByThePlatform(*system, stateDir)) {
        return *refused;
    }
#endif
    return preflight(manifest, install, stageDir, stateDir, launcher, *system);
}

core::Result<PreparedApply> preflight(const Manifest& manifest, const Install& install,
                                      const fs::path& stageDir, const fs::path& stateDir,
                                      Launcher& launcher, FsOps& fsOps)
{
    if (stateDir.empty()) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "preflight was given no state folder");
    }

    // Which helper runs the swap. The installed one speaks the protocol this build speaks; a
    // manifest that needs a newer one carries it, and it is the staged copy that runs, after its
    // hash is checked against the signed one (the stage folder is user-writable).
    const fs::path installedHelper = installedProgram(install.root, "katana_updater");
    fs::path source = installedHelper;
    // The manifest's entry of the helper, when it is the staged one that runs: what its copy is
    // held to, below.
    const ManifestFile* signedHelper = nullptr;
    if (manifest.applier > kApplierProtocol) {
        const std::string wanted = "bin/" + core::pathToUtf8(installedHelper.filename());
        const auto at = std::find_if(manifest.files.begin(), manifest.files.end(),
                                     [&](const ManifestFile& file) { return file.path == wanted; });
        if (at == manifest.files.end()) {
            return stageError(core::ErrorCode::InvalidState,
                              "this release needs a newer update helper than the installed one "
                              "and does not carry one",
                              "helper_missing");
        }
        signedHelper = &*at;
        source = finalPathOf(stageDir, *signedHelper);
        // Before anything is created: a helper that is not there, or not the signed one, is
        // found without leaving a run folder to clear away.
        if (!isVerified(fsOps, source, *signedHelper)) {
            return stageError(core::ErrorCode::InvalidState,
                              "the release's update helper is not staged, or is not the one the "
                              "release signed",
                              "helper_missing", "path=" + signedHelper->path);
        }
    } else {
        const core::Result<EntryInfo> info = fsOps.stat(source);
        if (!info.ok() || info.value().kind != EntryKind::File) {
            return stageError(core::ErrorCode::NotFound,
                              "the update helper is not installed",
                              "helper_missing", "path=" + core::pathToUtf8(source));
        }
    }

    const std::string transaction = newTransactionId();
    const fs::path runDir = runPath(stateDir, transaction);
    if (core::Status made = fsOps.makeDirectories(runDir); !made.ok()) {
        return writeFailed("the update helper's folder could not be created", made.error());
    }
    const fs::path helper = runDir / source.filename();
    if (core::Status copied = fsOps.copyFile(source, helper); !copied.ok()) {
        removeRunFolder(fsOps, stateDir, runDir);
        return writeFailed("the update helper could not be copied", copied.error());
    }
    // The copy is the file that will run, so the copy is what must be the signed helper. The
    // stage folder is user-writable: the file that was hashed there a moment ago and the file
    // that was just copied from there need not be the same bytes.
    if (signedHelper != nullptr && !isVerified(fsOps, helper, *signedHelper)) {
        removeRunFolder(fsOps, stateDir, runDir);
        return stageError(core::ErrorCode::InvalidState,
                          "the release's update helper changed while it was being copied; the "
                          "copy is not the one the release signed",
                          "helper_missing", "path=" + signedHelper->path);
    }
    if (core::Status marked = fsOps.setExecutable(helper); !marked.ok()) {
        removeRunFolder(fsOps, stateDir, runDir);
        return writeFailed("the update helper could not be made runnable", marked.error());
    }

    // It starts: the copy is scanned now, while the window is still open, and a helper that
    // cannot run here is found before anything is closed.
    SpawnRequest request;
    request.executable = helper;
    request.arguments = {"--selftest"};
    request.workingDirectory = runDir;
    const core::Result<SpawnResult> ran = launcher.run(request);
    if (!ran.ok()) {
        removeRunFolder(fsOps, stateDir, runDir);
        return stageError(core::ErrorCode::FileImportFailure,
                          "the update helper could not be started: " + ran.error().message,
                          "helper_not_started");
    }
    if (ran.value().timedOut) {
        removeRunFolder(fsOps, stateDir, runDir);
        return stageError(core::ErrorCode::FileImportFailure,
                          "the update helper did not finish its self-test in time",
                          "helper_timeout");
    }
    if (ran.value().exitCode != 0) {
        removeRunFolder(fsOps, stateDir, runDir);
        return stageError(core::ErrorCode::FileImportFailure,
                          "the update helper's self-test failed", "helper_failed",
                          "exit=" + std::to_string(ran.value().exitCode) + "; " +
                              excerptOf(ran.value().output));
    }

    PreparedApply prepared;
    prepared.transaction = transaction;
    prepared.runDirectory = runDir;
    prepared.helper = helper;
    return prepared;
}

} // namespace katana::update
