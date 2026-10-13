// stage() and preflight() (include/katana/update/stage.hpp): downloading what changed, byte
// for verified byte, and proving the helper starts before the window closes.
//
// Where the expectations come from (CLAUDE.md, section 4). Two payloads are served:
//
//   - the T3 fixture zip (tests/update/fixtures/release): a REAL zip made by the Python
//     producer. The files an update from v1 to v2 must leave are the fixtures' install/v2 files,
//     which are on disk beside it, and the offsets and sizes of the members are the numbers
//     written in the signed v2 manifest. Python's zipfile is the oracle that those are right
//     (tool_tests/test_make_manifest.py).
//   - a payload built here from a specification: members as RFC 1951 section 3.2.4 STORED blocks
//     (a header byte, LEN, NLEN, the bytes), which needs no compressor, so the data the engine's
//     zlib inflates was not made by zlib. The layout is written down as arithmetic so that the
//     requests a correct engine makes are worked out, not observed.
//
// The server is a hop function behind update::makeFetch, so what is exercised is stage(), the
// planner's ranges, the transport's rules and the inflater together, with no network.
//
// The stage folder is worked through the harness's portable file system (apply_harness.hpp,
// StdFs under a HookFs), handed to the forms of stage() and preflight() that take one: the
// system's own (makeSystemFsOps) is written for Windows only so far, and these tests are of the
// staging, which is the same code on every platform. The forms without the argument - what the
// programs call - are run once through the file system that ships, at the end, on Windows.
// Nothing is ever started: the launcher is a fake, so the "helper" a test puts in an install is
// any bytes at the helper's name.

#include <gtest/gtest.h>

#include <algorithm>
#include <filesystem>
#include <fstream>
#include <functional>
#include <memory>
#include <optional>
#include <set>
#include <stdexcept>
#include <stop_token>
#include <string>
#include <string_view>
#include <vector>

#include "apply_harness.hpp"
#include "katana/core/path_text.hpp"
#include "katana/core/sha256.hpp"
#include "katana/update/stage.hpp"
#include "katana/update/state.hpp"
#include "test_key.hpp"

using namespace katana;
using namespace katana::update;
namespace h = katana::update::harness;
namespace fs = std::filesystem;

namespace {

constexpr std::string_view kSecretToken = "ghp_SECRET-TOKEN-0123456789";

// ---- a payload built from a specification ---------------------------------------------------------------

std::string pattern(std::size_t size, unsigned seed)
{
    std::string bytes(size, '\0');
    unsigned x = seed * 2654435761u + 1;
    for (char& c : bytes) {
        x = x * 1664525u + 1013904223u;
        c = static_cast<char>(x >> 24);
    }
    return bytes;
}

// RFC 1951, 3.2.4: BFINAL, BTYPE 00, then LEN, NLEN (little-endian, one's complement) and LEN
// bytes, in blocks of at most 65535.
std::string storedDeflate(std::string_view bytes)
{
    std::string out;
    std::size_t at = 0;
    do {
        const std::size_t n = std::min<std::size_t>(65535, bytes.size() - at);
        const bool last = at + n >= bytes.size();
        out.push_back(last ? '\x01' : '\x00');
        out.push_back(static_cast<char>(n & 0xFF));
        out.push_back(static_cast<char>((n >> 8) & 0xFF));
        out.push_back(static_cast<char>(~n & 0xFF));
        out.push_back(static_cast<char>((~n >> 8) & 0xFF));
        out.append(bytes.substr(at, n));
        at += n;
    } while (at < bytes.size());
    return out;
}

struct Member {
    Member(std::string path_, std::string content_, FileClass fileClass_ = FileClass::Lib,
           std::size_t gapBefore_ = 0, int sizeLie_ = 0, std::string hashOf_ = {})
        : path(std::move(path_)), content(std::move(content_)), fileClass(fileClass_),
          gapBefore(gapBefore_), sizeLie(sizeLie_), hashOf(std::move(hashOf_))
    {
    }

    std::string path;
    std::string content;
    FileClass fileClass;
    std::size_t gapBefore; // bytes of the payload that belong to no member, before this one
    int sizeLie;           // added to the `size` the manifest signs
    std::string hashOf;    // when set, the signed hash is of THIS, not of the content
};

struct Placed {
    std::string path;
    std::string content;
    std::uint64_t offset = 0;
    std::uint64_t csize = 0;
};

constexpr std::uint64_t kPayloadHead = 44; // where the first member starts, as in a zip
constexpr std::uint64_t kPayloadTail = 100;
constexpr std::uint64_t kSequence = 2000000000;

struct Built {
    std::string payload;
    std::string json;
    std::vector<Placed> placed;
    Manifest manifest; // parseManifest of json, with the bytes a verified manifest carries
    std::string signatureLine;

    [[nodiscard]] const Placed& at(std::string_view path) const
    {
        for (const Placed& one : placed) {
            if (one.path == path) {
                return one;
            }
        }
        throw std::logic_error("no such member: " + std::string(path));
    }
};

std::string className(FileClass fileClass)
{
    return h::className(fileClass);
}

// Members must be given in path order (a manifest's files are sorted).
Built build(const std::vector<Member>& members, std::uint32_t applier = 1)
{
    Built built;
    built.payload = std::string(kPayloadHead, '\x50');
    std::string files;
    for (const Member& member : members) {
        built.payload += pattern(member.gapBefore, 7);
        const std::string deflated = storedDeflate(member.content);
        Placed placed{member.path, member.content, built.payload.size(), deflated.size()};
        built.payload += deflated;
        built.placed.push_back(placed);
        if (!files.empty()) {
            files += ",\n";
        }
        files += "{\"class\":\"" + className(member.fileClass) + "\",\"csize\":" +
                 std::to_string(placed.csize) + ",\"offset\":" + std::to_string(placed.offset) +
                 ",\"path\":\"" + member.path + "\",\"sha256\":\"" +
                 core::sha256Hex(member.hashOf.empty() ? member.content : member.hashOf) +
                 "\",\"size\":" +
                 std::to_string(static_cast<long long>(member.content.size()) + member.sizeLie) +
                 "}";
    }
    built.payload += std::string(kPayloadTail, '\x51');
    built.json =
        "{\"applier\":" + std::to_string(applier) +
        ",\"channel\":\"stable\",\"expires\":\"2030-01-01T00:00:00Z\",\"files\":[\n" + files +
        "\n],\"notes_url\":\"https://github.com/kjarada/Katana/releases/tag/v1.0.0\","
        "\"payload\":{\"codec\":\"deflate\",\"name\":\"Katana-1.0.0-win64.update.zip\",\"size\":" +
        std::to_string(built.payload.size()) +
        "},\"product\":\"katana\",\"published\":\"2026-10-01T00:00:00Z\",\"schema\":1,"
        "\"sequence\":" + std::to_string(kSequence) +
        ",\"tag\":\"v1.0.0\",\"target\":\"win64\",\"version\":\"1.0.0\"}\n";
    core::Result<Manifest> parsed = parseManifest(built.json);
    if (!parsed) {
        throw std::logic_error("the built manifest does not parse: " + parsed.error().describe());
    }
    built.manifest = std::move(parsed).value();
    // stage() does not verify (verifyManifest did, upstream); it needs the bytes that were
    // signed and the line that signed them, to write beside the files. Any line will do here.
    built.signatureLine = "ed25519 d75a980182b10ab7 " + std::string(86, 'A') + "==\n";
    built.manifest.signedJson = built.json;
    built.manifest.signatureLine = built.signatureLine;
    return built;
}

// ---- the layout the multi-range tests use ------------------------------------------------------------------
//
//   44          head
//   44          bin/a.dll      3000 bytes, one stored block:                 csize 3000 + 5 = 3005
//   3049        500 bytes of gap
//   3549        bin/b.dll      40 bytes:                                      csize 45
//   3594        2 MiB of gap (2097152, more than the 1 MiB at which ranges merge)
//   2100746     share/big.bin  3 MiB + 17 = 3145745 bytes: 48 blocks of 65535 (3145680) and a
//               block of 65, so 49 blocks:                                  csize 3145745 + 245 = 3145990
//   5246736     1.5 MiB of gap (1572864)
//   6819600     share/c.txt    10 bytes:                                      csize 15
//   6819615     tail 100
//
// So three requests: [44, 3594) length 3550 (a.dll and b.dll, 500 apart, are one range),
// [2100746, 5246736) length 3145990, and [6819600, 6819615) length 15.

std::vector<Member> threeRanges()
{
    return {
        {"bin/a.dll", pattern(3000, 1), FileClass::Lib, 0},
        {"bin/b.dll", pattern(40, 2), FileClass::Lib, 500},
        {"share/big.bin", pattern(3 * 1024 * 1024 + 17, 3), FileClass::Data, 2 * 1024 * 1024},
        {"share/c.txt", pattern(10, 4), FileClass::Data, 1536 * 1024},
    };
}

struct Range {
    std::uint64_t start;
    std::uint64_t length;
    bool operator==(const Range&) const = default;
};

// ---- the server --------------------------------------------------------------------------------------------

std::optional<Range> rangeOf(const HttpHop& hop)
{
    for (const std::string& header : hop.headers) {
        constexpr std::string_view kPrefix = "Range: bytes=";
        if (header.starts_with(kPrefix)) {
            const std::string spec = header.substr(kPrefix.size());
            const std::size_t dash = spec.find('-');
            const std::uint64_t first = std::stoull(spec.substr(0, dash));
            const std::uint64_t last = std::stoull(spec.substr(dash + 1));
            return Range{first, last - first + 1};
        }
    }
    return std::nullopt;
}

// Serves a payload as a release host does: 206 and a Content-Range for a range, 200 for none.
class PayloadServer {
  public:
    explicit PayloadServer(std::string payload) : payload_(std::move(payload)) {}

    // Called with the index of the hop (from 0) before it is answered.
    std::function<void(std::size_t)> before;
    // Changes the body the server sends back for a range.
    std::function<void(std::string&)> mutate;
    // Answers instead, for a hop it wants to break.
    std::function<std::optional<core::Result<HttpHopReply>>(std::size_t, const HttpHop&)> custom;
    bool ignoreRange = false;
    std::string url; // when set, every hop must be to this address

    HopFn hop()
    {
        return [this](const HttpHop& one, std::stop_token, ProgressFn progress)
                   -> core::Result<HttpHopReply> {
            const std::size_t index = hops.size();
            hops.push_back(one);
            // Taken out of the optional here, once: the build is -O3 -Werror, and an optional
            // of a plain type that is read after calls the optimiser cannot see through is
            // where -Wmaybe-uninitialized fires.
            const std::optional<Range> named = rangeOf(one);
            const bool ranged = named.has_value();
            const Range range = named.value_or(Range{0, 0});
            if (ranged) {
                ranges.push_back(range);
            }
            if (before) {
                before(index);
            }
            if (!url.empty()) {
                EXPECT_EQ(one.url, url);
            }
            if (custom) {
                if (auto answer = custom(index, one)) {
                    return *answer;
                }
            }
            HttpHopReply reply;
            if (!ranged || ignoreRange) {
                reply.status = 200;
                reply.body = payload_;
            } else {
                const std::uint64_t last =
                    std::min<std::uint64_t>(range.start + range.length, payload_.size());
                reply.status = 206;
                reply.body = payload_.substr(range.start, last - range.start);
                reply.contentRange = "bytes " + std::to_string(range.start) + "-" +
                                     std::to_string(last - 1) + "/" +
                                     std::to_string(payload_.size());
            }
            reply.contentType = "application/zip";
            if (mutate) {
                mutate(reply.body);
            }
            // The real hop abandons a transfer at maxBytes, keeps none of it, and says what the
            // answer was (HttpHopReply::overran).
            if (reply.body.size() > one.maxBytes) {
                reply.body.clear();
                reply.overran = true;
                return reply;
            }
            if (progress) {
                progress(reply.body.size() / 2, reply.body.size());
                progress(reply.body.size(), reply.body.size());
            }
            return reply;
        };
    }

    std::vector<HttpHop> hops;
    std::vector<Range> ranges;

  private:
    std::string payload_;
};

// ---- the rig -----------------------------------------------------------------------------------------------

struct Rig {
    explicit Rig(const std::string& label) : scratch(label)
    {
        root = scratch / "R";
        stateDir = scratch / "U";
        stageDir = stateDir / ("stage-" + std::to_string(kSequence));
        fs::create_directories(root);
        install.root = root;
        install.target = "win64";
        install.kind = InstallKind::Archive;
        install.version = parseVersion("0.9.0").value();
        install.sequence = 1790000000;
        install.hasInstalledRecord = true;
        feed.repo = "kjarada/Katana";
        feed.target = "win64";
        feed.userAgent = "Katana/0.9.0 (win64)";
    }

    void useManifest(Manifest next)
    {
        manifest = std::move(next);
        const core::Result<Plan> planned = plan(manifest, install, std::stop_token{});
        ASSERT_TRUE(planned.ok()) << planned.error().describe();
        thePlan = planned.value();
    }
    void use(const Built& built) { useManifest(built.manifest); }

    core::Status run(const Fetch& fetch, std::stop_token stop = {})
    {
        progress.clear();
        return stage(fetch, feed, manifest, thePlan, stageDir, stop,
                     [this](std::uint64_t done, std::uint64_t total) {
                         progress.emplace_back(done, total);
                     },
                     hooked);
    }

    h::Scratch scratch;
    h::Path root;
    h::Path stateDir;
    h::Path stageDir;
    Install install;
    FeedConfig feed;
    Manifest manifest;
    Plan thePlan;
    std::vector<std::pair<std::uint64_t, std::uint64_t>> progress;
    // The disk, and over it the hooks a test breaks a write or names the free space with.
    h::StdFs disk;
    h::HookFs hooked{disk};
};

// Every regular file under `folder`, as '/'-separated paths relative to it.
std::set<std::string> filesUnder(const fs::path& folder)
{
    std::set<std::string> found;
    std::error_code ec;
    if (!fs::exists(folder, ec)) {
        return found;
    }
    for (fs::recursive_directory_iterator it(folder, ec), end; !ec && it != end; it.increment(ec)) {
        if (it->is_regular_file(ec)) {
            found.insert(it->path().lexically_relative(folder).generic_string());
        }
    }
    return found;
}

std::set<std::string> names(std::initializer_list<const char*> list)
{
    return {list.begin(), list.end()};
}

std::string reasonOf(const core::Status& status)
{
    return status.ok() ? std::string("ok") : fetchFailureReason(status.error());
}

// ---- the fixture release (T3's real zip) ----------------------------------------------------------------------

struct FixtureRig : Rig {
    explicit FixtureRig(const std::string& label, const std::string& zipFolder = "v2",
                        bool withRandomBin = true)
        : Rig(label), v2Zip(h::readText(h::fixtureFolder() / "release" / zipFolder /
                                        "Katana-0.10.0-win64.update.zip"))
    {
        h::copyTree(h::fixtureFolder() / "install" / "v1", root);
        if (!withRandomBin) {
            fs::remove(root / "share/katana/random.bin");
        }
        const std::string json =
            h::readText(h::fixtureFolder() / "release/v2/Katana-update-win64.json");
        const std::string sig =
            h::readText(h::fixtureFolder() / "release/v2/Katana-update-win64.json.sig");
        const core::Result<Manifest> verified = verifyManifest(json, sig, testTrust(), install);
        EXPECT_TRUE(verified.ok()) << (verified.ok() ? "" : verified.error().describe());
        if (verified.ok()) {
            use2(verified.value());
        }
    }
    void use2(Manifest next) { useManifest(std::move(next)); }

    [[nodiscard]] std::string v2File(const std::string& path) const
    {
        return h::readText(h::fixtureFolder() / "install" / "v2" / path);
    }

    std::string v2Zip;
};

constexpr std::string_view kV2Payload =
    "https://github.com/kjarada/Katana/releases/download/v0.10.0/Katana-0.10.0-win64.update.zip";

// ---- the happy path, on the real zip -------------------------------------------------------------------------

TEST(Stage, AnUpdateFromV1ToV2FetchesOnlyTheChangedFilesInOneRangeAndStagesThemVerified)
{
    FixtureRig rig("stage_v1_v2");
    PayloadServer server(rig.v2Zip);
    server.url = std::string(kV2Payload);
    const core::Status status = rig.run(makeFetch(server.hop()));
    ASSERT_TRUE(status.ok()) << status.error().describe();

    // Changed between v1 and v2 (tools/update_make_fixtures.py): bin/katana.exe, share/katana/
    // readme.txt; arriving: bin/libnew-2.dll, share/katana/news.txt, share/katana/help/index.txt.
    // etc/katana/settings.ini changed too, but it is a seed and v1 has it: it is never read, and so
    // never replaced. katana_cli.exe, libcore-1.dll, empty.txt and random.bin are the same bytes.
    EXPECT_EQ(rig.thePlan.changedFiles, 5u);
    EXPECT_EQ(filesUnder(rig.stageDir),
              names({"bin/katana.exe", "bin/libnew-2.dll", "share/katana/help/index.txt",
                     "share/katana/news.txt", "share/katana/readme.txt", "manifest.json",
                     "manifest.json.sig"}));
    for (const char* path : {"bin/katana.exe", "bin/libnew-2.dll", "share/katana/help/index.txt",
                             "share/katana/news.txt", "share/katana/readme.txt"}) {
        EXPECT_EQ(h::readText(rig.stageDir / path), rig.v2File(path)) << path;
    }

    // The manifest of v2 lists these members of the payload (offset, csize): katana.exe (44, 85),
    // libnew-2.dll (418, 70), help/index.txt (599, 75), news.txt (725, 54), readme.txt (2938, 55).
    // The gaps are far below 1 MiB, so they are ONE request from 44 to 2938 + 55 = 2993.
    ASSERT_EQ(server.ranges.size(), 1u);
    EXPECT_EQ(server.ranges[0], (Range{44, 2993 - 44}));
    EXPECT_EQ(server.hops.size(), 1u);
    EXPECT_EQ(server.hops[0].maxBytes, 2949u) << "maxBytes is the range length";

    // The staged record is the signed manifest, byte for byte.
    EXPECT_EQ(h::readText(rig.stageDir / "manifest.json"),
              h::readText(h::fixtureFolder() / "release/v2/Katana-update-win64.json"));
    EXPECT_EQ(h::readText(rig.stageDir / "manifest.json.sig"),
              h::readText(h::fixtureFolder() / "release/v2/Katana-update-win64.json.sig"));
}

TEST(Stage, ProgressCountsTheCompressedBytesToFetchFromZeroToTheTotal)
{
    FixtureRig rig("stage_progress");
    PayloadServer server(rig.v2Zip);
    ASSERT_TRUE(rig.run(makeFetch(server.hop())).ok());
    ASSERT_GE(rig.progress.size(), 3u);
    EXPECT_EQ(rig.progress.front(), (std::pair<std::uint64_t, std::uint64_t>{0, 2949}));
    EXPECT_EQ(rig.progress.back(), (std::pair<std::uint64_t, std::uint64_t>{2949, 2949}));
    std::uint64_t last = 0;
    for (const auto& [done, total] : rig.progress) {
        EXPECT_EQ(total, 2949u);
        EXPECT_GE(done, last) << "progress never goes back";
        EXPECT_LE(done, total);
        last = done;
    }
}

TEST(Stage, ARepeatedStageFetchesNothingAndWritesTheSameRecord)
{
    FixtureRig rig("stage_repeat");
    PayloadServer first(rig.v2Zip);
    ASSERT_TRUE(rig.run(makeFetch(first.hop())).ok());
    PayloadServer second(rig.v2Zip);
    ASSERT_TRUE(rig.run(makeFetch(second.hop())).ok());
    EXPECT_TRUE(second.hops.empty()) << "every file is staged and verified: nothing to ask for";
    EXPECT_EQ(rig.progress.back().first, rig.progress.back().second);
    EXPECT_TRUE(h::isPresent(rig.stageDir / "manifest.json"));
    EXPECT_TRUE(h::isPresent(rig.stageDir / "manifest.json.sig"));
    EXPECT_EQ(filesUnder(rig.stageDir).size(), 7u);
}

// ---- tampering and damage ------------------------------------------------------------------------------------------

TEST(Stage, ATamperedMemberIsRefusedAndNothingUnverifiedIsKept)
{
    // T3's bad/tampered-member: v2's zip with one byte of share/katana/random.bin changed, and
    // v2's genuine manifest. The install here lacks random.bin, so it is among the changes.
    FixtureRig rig("stage_tampered", "bad/tampered-member", /*withRandomBin=*/false);
    PayloadServer server(rig.v2Zip);
    const core::Status status = rig.run(makeFetch(server.hop()));
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, core::ErrorCode::InvalidArgument);
    EXPECT_EQ(reasonOf(status), "staged_hash");
    EXPECT_NE(status.error().context.find("share/katana/random.bin"), std::string::npos);

    EXPECT_FALSE(h::isPresent(rig.stageDir / "share/katana/random.bin"));
    EXPECT_FALSE(h::isPresent(rig.stageDir / "manifest.json")) << "an unfinished stage is not complete";
    EXPECT_FALSE(h::isPresent(rig.stageDir / "manifest.json.sig"));
    // What was verified before the bad member (in the payload's order) stays, and is right.
    for (const std::string& path : filesUnder(rig.stageDir)) {
        EXPECT_EQ(h::readText(rig.stageDir / path), rig.v2File(path)) << path;
    }
    EXPECT_FALSE(h::isPresent(rig.stageDir / ".partial" / "x"));
    EXPECT_TRUE(filesUnder(rig.stageDir / ".partial").empty()) << "no half file is left";

    // The same server, repaired, is resumed from where it stopped: only what is missing is asked.
    PayloadServer repaired(h::readText(h::fixtureFolder() / "release/v2/Katana-0.10.0-win64.update.zip"));
    ASSERT_TRUE(rig.run(makeFetch(repaired.hop())).ok());
    ASSERT_EQ(repaired.ranges.size(), 1u);
    // random.bin (832, 2053) and readme.txt (2938, 55) were not staged: [832, 2993).
    EXPECT_EQ(repaired.ranges[0], (Range{832, 2993 - 832}));
    EXPECT_EQ(h::readText(rig.stageDir / "share/katana/random.bin"), rig.v2File("share/katana/random.bin"));
}

TEST(Stage, AMemberThatIsNotTheSignedFileIsRefusedWhateverIsWrongWithIt)
{
    struct Case {
        const char* what;
        Member bad;
        const char* reason;
    };
    const std::vector<Case> cases = {
        {"a hash that is not the content's",
         {"bin/x.dll", pattern(100, 9), FileClass::Lib, 0, 0, "another file"}, "staged_hash"},
        {"a signed size one byte more than the data",
         {"bin/x.dll", pattern(100, 9), FileClass::Lib, 0, +1}, "staged_corrupt"},
        {"a signed size one byte less than the data",
         {"bin/x.dll", pattern(100, 9), FileClass::Lib, 0, -1}, "staged_corrupt"},
    };
    for (const Case& test : cases) {
        Rig rig("stage_bad_member");
        const Built built = build({{"bin/a.dll", pattern(50, 8), FileClass::Lib, 0}, test.bad});
        rig.use(built);
        PayloadServer server(built.payload);
        const core::Status status = rig.run(makeFetch(server.hop()));
        ASSERT_FALSE(status.ok()) << test.what;
        EXPECT_EQ(status.error().code, core::ErrorCode::InvalidArgument) << test.what;
        EXPECT_EQ(reasonOf(status), test.reason) << test.what;
        EXPECT_FALSE(h::isPresent(rig.stageDir / "bin/x.dll")) << test.what;
        EXPECT_FALSE(h::isPresent(rig.stageDir / "manifest.json")) << test.what;
        EXPECT_EQ(h::readText(rig.stageDir / "bin/a.dll"), built.at("bin/a.dll").content)
            << test.what << ": the verified file before it stays";
        EXPECT_TRUE(filesUnder(rig.stageDir / ".partial").empty()) << test.what;
    }
}

TEST(Stage, DamagedDeflateDataIsRefusedAsCorrupt)
{
    Rig rig("stage_corrupt");
    const Built built = build({{"bin/a.dll", pattern(500, 5), FileClass::Lib, 0}});
    rig.use(built);
    PayloadServer server(built.payload);
    // The stored block's NLEN must be the complement of LEN: flip a bit of it.
    server.mutate = [&](std::string& body) { body[3] = static_cast<char>(body[3] ^ 0x01); };
    const core::Status status = rig.run(makeFetch(server.hop()));
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(reasonOf(status), "staged_corrupt");
    EXPECT_TRUE(filesUnder(rig.stageDir).empty());
}

// ---- the network's answers ---------------------------------------------------------------------------------------------------

TEST(Stage, AServerThatIgnoresRangeIsRefusedBeforeItFloodsTheDisk)
{
    FixtureRig rig("stage_ignore_range");
    PayloadServer server(rig.v2Zip);
    server.ignoreRange = true; // answers every range with the whole 3791-byte zip and a 200
    const core::Status status = rig.run(makeFetch(server.hop()));
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, core::ErrorCode::Unsupported);
    EXPECT_EQ(status.error().message, std::string(kPartialDownloadsRefused));
    EXPECT_EQ(reasonOf(status), "partial_downloads");
    ASSERT_EQ(server.hops.size(), 1u) << "and no second try";
    EXPECT_EQ(server.hops[0].maxBytes, 2949u) << "the cut-off is the range, not the payload";
    EXPECT_FALSE(h::isPresent(rig.stageDir / "manifest.json"));
    EXPECT_TRUE(filesUnder(rig.stageDir).empty());
}

TEST(Stage, TheServersRefusalsAreReportedWithTheirStatus)
{
    struct Case {
        int status;
        core::ErrorCode code;
        const char* reason;
    };
    for (const Case& test : {Case{403, core::ErrorCode::FileImportFailure, "http_403"},
                             Case{404, core::ErrorCode::NotFound, "http_404"},
                             Case{429, core::ErrorCode::FileImportFailure, "http_429"},
                             Case{416, core::ErrorCode::FileImportFailure, "http_416"},
                             Case{503, core::ErrorCode::FileImportFailure, "http_503"}}) {
        FixtureRig rig("stage_status");
        PayloadServer server(rig.v2Zip);
        server.custom = [&](std::size_t, const HttpHop&) -> std::optional<core::Result<HttpHopReply>> {
            HttpHopReply reply;
            reply.status = test.status;
            reply.body = "<html>no</html>";
            return core::Result<HttpHopReply>(reply);
        };
        const core::Status status = rig.run(makeFetch(server.hop()));
        ASSERT_FALSE(status.ok()) << test.status;
        EXPECT_EQ(status.error().code, test.code) << test.status;
        EXPECT_EQ(reasonOf(status), test.reason);
        EXPECT_TRUE(filesUnder(rig.stageDir).empty()) << test.status;
    }
}

TEST(Stage, AWebPageInPlaceOfTheRangeIsAPortal)
{
    FixtureRig rig("stage_portal");
    PayloadServer server(rig.v2Zip);
    server.custom = [](std::size_t, const HttpHop&) -> std::optional<core::Result<HttpHopReply>> {
        HttpHopReply reply;
        reply.status = 200;
        reply.contentType = "text/html";
        reply.body = "<html>Please sign in</html>";
        return core::Result<HttpHopReply>(reply);
    };
    const core::Status status = rig.run(makeFetch(server.hop()));
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(reasonOf(status), "html_page");
}

TEST(Stage, ATruncatedOrMisplacedRangeIsRefusedAndStagesNothing)
{
    {
        FixtureRig rig("stage_truncated");
        PayloadServer server(rig.v2Zip);
        server.mutate = [](std::string& body) { body.pop_back(); }; // Content-Range still says all of it
        const core::Status status = rig.run(makeFetch(server.hop()));
        ASSERT_FALSE(status.ok());
        EXPECT_EQ(status.error().code, core::ErrorCode::FileImportFailure);
        EXPECT_EQ(reasonOf(status), "bad_range") << "a 206 that is not the range, not a status";
        EXPECT_TRUE(filesUnder(rig.stageDir).empty());
    }
    {
        // The server's file is another length than the release names: it may have been replaced.
        FixtureRig rig("stage_total");
        PayloadServer server(rig.v2Zip + "extra");
        const core::Status status = rig.run(makeFetch(server.hop()));
        ASSERT_FALSE(status.ok());
        EXPECT_EQ(reasonOf(status), "bad_range");
        EXPECT_TRUE(filesUnder(rig.stageDir).empty());
    }
    {
        // Another part of the file than the one asked for.
        FixtureRig rig("stage_shifted");
        PayloadServer server(rig.v2Zip);
        server.custom = [&](std::size_t, const HttpHop& hop) -> std::optional<core::Result<HttpHopReply>> {
            const Range range = rangeOf(hop).value_or(Range{0, 0});
            HttpHopReply reply;
            reply.status = 206;
            reply.body = rig.v2Zip.substr(range.start + 1, range.length);
            reply.contentRange = "bytes " + std::to_string(range.start + 1) + "-" +
                                 std::to_string(range.start + range.length) + "/" +
                                 std::to_string(rig.v2Zip.size());
            return core::Result<HttpHopReply>(reply);
        };
        EXPECT_EQ(reasonOf(rig.run(makeFetch(server.hop()))), "bad_range");
        EXPECT_TRUE(filesUnder(rig.stageDir).empty());
    }
}

TEST(Stage, ATransportThatHandsOverABodyOfTheWrongLengthIsRefusedByTheStageItself)
{
    // makeFetch holds a range to its length; a Fetch of another making (a test's, a later
    // transport's) may not, and the slices cut from the body are only right if it is that long.
    FixtureRig rig("stage_short_range");
    for (const std::size_t length : {std::size_t{0}, std::size_t{2948}, std::size_t{2950}}) {
        std::size_t asked = 0;
        const Fetch careless = [&](const FetchRequest&, std::stop_token,
                                   ProgressFn) -> core::Result<std::string> {
            ++asked;
            return std::string(length, 'x'); // the range is 2949 bytes
        };
        const core::Status status = rig.run(careless);
        ASSERT_FALSE(status.ok()) << length;
        EXPECT_EQ(status.error().code, core::ErrorCode::FileImportFailure) << length;
        EXPECT_EQ(reasonOf(status), "short_range") << length;
        EXPECT_EQ(asked, 1u) << length;
        EXPECT_TRUE(filesUnder(rig.stageDir).empty()) << length;
    }
}

// ---- several ranges, cancellation and resume ------------------------------------------------------------------------

TEST(Stage, MembersFarApartAreSeparateRangesAndNearOnesAreMerged)
{
    Rig rig("stage_ranges");
    const Built built = build(threeRanges());
    rig.use(built);
    PayloadServer server(built.payload);
    const core::Status status = rig.run(makeFetch(server.hop()));
    ASSERT_TRUE(status.ok()) << status.error().describe();

    // The arithmetic of the layout (see above threeRanges): worked out here from the placed
    // members, not from what mergeRanges returned.
    const Placed& a = built.at("bin/a.dll");
    const Placed& b = built.at("bin/b.dll");
    const Placed& big = built.at("share/big.bin");
    const Placed& c = built.at("share/c.txt");
    EXPECT_EQ(a.offset, 44u);
    EXPECT_EQ(b.offset + b.csize, 3594u);
    EXPECT_EQ(big.offset, 2100746u);
    EXPECT_EQ(big.csize, 3145990u);
    EXPECT_EQ(c.offset, 6819600u);
    EXPECT_EQ(c.csize, 15u);
    ASSERT_EQ(server.ranges.size(), 3u);
    EXPECT_EQ(server.ranges[0], (Range{44, 3550}));
    EXPECT_EQ(server.ranges[1], (Range{2100746, 3145990}));
    EXPECT_EQ(server.ranges[2], (Range{6819600, 15}));
    for (const Placed& placed : built.placed) {
        EXPECT_EQ(h::readText(rig.stageDir / placed.path), placed.content) << placed.path;
    }
    // Progress: from 0 to the sum of the three requests.
    EXPECT_EQ(rig.progress.front().first, 0u);
    EXPECT_EQ(rig.progress.back(), (std::pair<std::uint64_t, std::uint64_t>{3149555, 3149555}));
}

TEST(Stage, ACancelledStageLeavesOnlyVerifiedFilesAndTheNextOneResumes)
{
    Rig rig("stage_cancel");
    const Built built = build(threeRanges());
    rig.use(built);

    // Cancelled while the second request is being answered (the stop arrives before its members
    // are written).
    std::stop_source source;
    PayloadServer server(built.payload);
    server.before = [&](std::size_t index) {
        if (index == 1) {
            source.request_stop();
        }
    };
    const core::Status status = rig.run(makeFetch(server.hop()), source.get_token());
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, core::ErrorCode::InvalidState);
    EXPECT_EQ(reasonOf(status), "cancelled");

    // The first request's members are complete and right; nothing of the second is under a final
    // name, no half file, no staged record.
    EXPECT_EQ(filesUnder(rig.stageDir), names({"bin/a.dll", "bin/b.dll"}));
    EXPECT_EQ(h::readText(rig.stageDir / "bin/a.dll"), built.at("bin/a.dll").content);
    EXPECT_EQ(h::readText(rig.stageDir / "bin/b.dll"), built.at("bin/b.dll").content);
    EXPECT_FALSE(h::isPresent(rig.stageDir / ".partial"));

    // Resumed: the two ranges still missing are the only requests made.
    PayloadServer second(built.payload);
    ASSERT_TRUE(rig.run(makeFetch(second.hop())).ok());
    ASSERT_EQ(second.ranges.size(), 2u);
    EXPECT_EQ(second.ranges[0], (Range{2100746, 3145990}));
    EXPECT_EQ(second.ranges[1], (Range{6819600, 15}));
    // and the progress starts where the first left off: the 3005 + 45 bytes of a.dll and b.dll, which
    // are staged, and ends at those plus the two requests (the 500-byte gap is not asked for again).
    EXPECT_EQ(rig.progress.front(), (std::pair<std::uint64_t, std::uint64_t>{3050, 3149055}));
    EXPECT_EQ(filesUnder(rig.stageDir).size(), 6u);
    for (const Placed& placed : built.placed) {
        EXPECT_EQ(h::readText(rig.stageDir / placed.path), placed.content) << placed.path;
    }
}

TEST(Stage, ATransferThatIsCancelledInsideTheFetchLeavesNoHalfFile)
{
    Rig rig("stage_cancel_inside");
    const Built built = build(threeRanges());
    rig.use(built);
    PayloadServer server(built.payload);
    server.custom = [](std::size_t index, const HttpHop&) -> std::optional<core::Result<HttpHopReply>> {
        if (index == 2) {
            return core::Result<HttpHopReply>(core::makeError(core::ErrorCode::InvalidState, "cancelled"));
        }
        return std::nullopt;
    };
    const core::Status status = rig.run(makeFetch(server.hop()));
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(reasonOf(status), "cancelled");
    // Two ranges were staged, the third was cancelled: every name under the folder is a good file.
    EXPECT_EQ(filesUnder(rig.stageDir), names({"bin/a.dll", "bin/b.dll", "share/big.bin"}));
    for (const std::string& path : filesUnder(rig.stageDir)) {
        for (const Placed& placed : built.placed) {
            if (placed.path == path) {
                EXPECT_EQ(h::readText(rig.stageDir / path), placed.content) << path;
            }
        }
    }
}

TEST(Stage, AStopRequestedBeforeTheStartDoesNothing)
{
    Rig rig("stage_stopped");
    const Built built = build(threeRanges());
    rig.use(built);
    PayloadServer server(built.payload);
    std::stop_source source;
    source.request_stop();
    const core::Status status = rig.run(makeFetch(server.hop()), source.get_token());
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(reasonOf(status), "cancelled");
    EXPECT_TRUE(server.hops.empty());
    EXPECT_FALSE(h::isPresent(rig.stageDir)) << "not even the folder";
}

TEST(Stage, AStopRequestedBetweenTwoRequestsMakesNoFurtherRequest)
{
    Rig rig("stage_stop_between");
    const Built built = build(threeRanges());
    rig.use(built);
    PayloadServer server(built.payload);
    std::stop_source source;
    // The progress calls are: the start (1), the two the server makes while it answers the first
    // request (2, 3), and the report after that request's files are staged (4). The stop arrives
    // on the fourth: after the first request, before the second.
    int calls = 0;
    const core::Status status =
        stage(makeFetch(server.hop()), rig.feed, rig.manifest, rig.thePlan, rig.stageDir,
              source.get_token(),
              [&](std::uint64_t, std::uint64_t) {
                  if (++calls == 4) {
                      source.request_stop();
                  }
              },
              rig.hooked);
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(reasonOf(status), "cancelled");
    EXPECT_EQ(server.hops.size(), 1u) << "no request is made once the stop is seen";
    EXPECT_EQ(filesUnder(rig.stageDir), names({"bin/a.dll", "bin/b.dll"}));
}

TEST(Stage, AFileThatChangedOnDiskSinceItWasStagedIsFetchedAgain)
{
    Rig rig("stage_rot");
    const Built built = build(threeRanges());
    rig.use(built);
    PayloadServer first(built.payload);
    ASSERT_TRUE(rig.run(makeFetch(first.hop())).ok());

    // The same size, one byte different (a bad sector, an editor); and a file of another size.
    std::string rotten = built.at("bin/b.dll").content;
    rotten[5] = static_cast<char>(rotten[5] ^ 0x40);
    h::writeText(rig.stageDir / "bin/b.dll", rotten);
    h::writeText(rig.stageDir / "share/c.txt", "too short");
    PayloadServer second(built.payload);
    ASSERT_TRUE(rig.run(makeFetch(second.hop())).ok());
    // b.dll is in the first range and c.txt in the third; each alone is a range of its own now.
    ASSERT_EQ(second.ranges.size(), 2u);
    EXPECT_EQ(second.ranges[0], (Range{built.at("bin/b.dll").offset, built.at("bin/b.dll").csize}));
    EXPECT_EQ(second.ranges[1], (Range{6819600, 15}));
    for (const Placed& placed : built.placed) {
        EXPECT_EQ(h::readText(rig.stageDir / placed.path), placed.content) << placed.path;
    }
}

TEST(Stage, ARecordOfAnEarlierCompleteStageDoesNotSurviveAFailedRun)
{
    Rig rig("stage_stale_record");
    const Built built = build(threeRanges());
    rig.use(built);
    PayloadServer first(built.payload);
    ASSERT_TRUE(rig.run(makeFetch(first.hop())).ok());
    ASSERT_TRUE(h::isPresent(rig.stageDir / "manifest.json"));

    // A file goes missing and the network is down: the run fails, and must not leave a stage that
    // still says it is complete.
    fs::remove(rig.stageDir / "share/c.txt");
    PayloadServer broken(built.payload);
    broken.custom = [](std::size_t, const HttpHop&) -> std::optional<core::Result<HttpHopReply>> {
        return core::Result<HttpHopReply>(core::makeError(
            core::ErrorCode::FileImportFailure, "no network", "status=0; https://github.com/x"));
    };
    const core::Status status = rig.run(makeFetch(broken.hop()));
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(reasonOf(status), "offline");
    EXPECT_FALSE(h::isPresent(rig.stageDir / "manifest.json"));
    EXPECT_FALSE(h::isPresent(rig.stageDir / "manifest.json.sig"));
}

TEST(Stage, AFileThatCannotBePutInItsPlaceFailsTheStageAndLeavesNoHalfFile)
{
    // The disk refuses the rename of bin/b.dll to its final name (error 5, as a file somebody
    // holds open gives). a.dll, before it in the same range, is staged; b.dll is under no name
    // at all; and the run that follows a repaired disk asks only for what is missing.
    Rig rig("stage_rename_fails");
    const Built built = build(threeRanges());
    rig.use(built);
    rig.hooked.beforeRename = [](const h::Path&, const h::Path& to) -> std::optional<core::Status> {
        if (to.filename() == "b.dll") {
            return h::injected(5, to);
        }
        return std::nullopt;
    };
    PayloadServer server(built.payload);
    const core::Status status = rig.run(makeFetch(server.hop()));
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, core::ErrorCode::FileExportFailure);
    EXPECT_EQ(reasonOf(status), "write_failed");
    // The token, then the manifest's path, then the file system's own number (fsops.hpp).
    EXPECT_TRUE(status.error().context.starts_with("reason=write_failed; path=bin/b.dll; os_error=5 "))
        << status.error().context;
    EXPECT_EQ(server.hops.size(), 1u) << "nothing more is fetched into a folder that refuses";
    EXPECT_EQ(filesUnder(rig.stageDir), names({"bin/a.dll"}));
    EXPECT_FALSE(h::isPresent(rig.stageDir / ".partial"));

    rig.hooked.beforeRename = nullptr;
    PayloadServer second(built.payload);
    ASSERT_TRUE(rig.run(makeFetch(second.hop())).ok());
    // b.dll alone (a.dll, 500 bytes before it, is staged), then the two far ranges.
    ASSERT_EQ(second.ranges.size(), 3u);
    EXPECT_EQ(second.ranges[0], (Range{3549, 45}));
    EXPECT_EQ(second.ranges[1], (Range{2100746, 3145990}));
    EXPECT_EQ(second.ranges[2], (Range{6819600, 15}));
    for (const Placed& placed : built.placed) {
        EXPECT_EQ(h::readText(rig.stageDir / placed.path), placed.content) << placed.path;
    }
}

TEST(Stage, ProgressNeverGoesBackwardsOrPastTheTotalWhateverTheTransportReports)
{
    // One member of 100 bytes: a stored block of 105, so one request of 105 bytes. The transport
    // counts 80 bytes, starts the request again (a dropped connection: 10), and then reports more
    // than there is.
    Rig rig("stage_progress_restart");
    const Built built = build({{"bin/a.dll", pattern(100, 1), FileClass::Lib, 0}});
    rig.use(built);
    PayloadServer server(built.payload);
    const HopFn hop = [&](const HttpHop& one, std::stop_token stop,
                          ProgressFn progress) -> core::Result<HttpHopReply> {
        progress(80, 105);
        progress(10, 105);
        progress(100000, 0);
        return server.hop()(one, stop, ProgressFn{});
    };
    ASSERT_TRUE(rig.run(makeFetch(hop)).ok());
    ASSERT_GE(rig.progress.size(), 5u);
    EXPECT_EQ(rig.progress[0], (std::pair<std::uint64_t, std::uint64_t>{0, 105}));
    EXPECT_EQ(rig.progress[1], (std::pair<std::uint64_t, std::uint64_t>{80, 105}));
    EXPECT_EQ(rig.progress[2], (std::pair<std::uint64_t, std::uint64_t>{80, 105}))
        << "the restart is not shown as a step back";
    EXPECT_EQ(rig.progress[3], (std::pair<std::uint64_t, std::uint64_t>{105, 105}))
        << "and no more than the request holds";
    EXPECT_EQ(rig.progress.back(), (std::pair<std::uint64_t, std::uint64_t>{105, 105}));
}

TEST(Stage, TheStagedRecordIsTheSignedBytesAndTheSignatureLineExactly)
{
    Rig rig("stage_record");
    const Built built = build(threeRanges());
    rig.use(built);
    PayloadServer server(built.payload);
    ASSERT_TRUE(rig.run(makeFetch(server.hop())).ok());
    EXPECT_EQ(h::readText(stagedManifestPath(rig.stageDir)), built.json);
    EXPECT_EQ(h::readText(stagedSignaturePath(rig.stageDir)), built.signatureLine);
    EXPECT_EQ(stagedManifestPath(rig.stageDir).filename().string(), "manifest.json");
    EXPECT_EQ(stagedSignaturePath(rig.stageDir).filename().string(), "manifest.json.sig");
}

// ---- free space and refused input ----------------------------------------------------------------------------------------------

TEST(Stage, NotEnoughFreeSpaceIsRefusedBeforeAnythingIsFetched)
{
    Rig rig("stage_space");
    const Built built = build(threeRanges());
    rig.use(built);
    rig.thePlan.requiredFreeBytes = ~0ull; // more than any volume has
    PayloadServer server(built.payload);
    const core::Status status = rig.run(makeFetch(server.hop()));
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, core::ErrorCode::FileExportFailure);
    EXPECT_EQ(reasonOf(status), "low_disk_space");
    EXPECT_NE(status.error().context.find("need="), std::string::npos);
    EXPECT_NE(status.error().context.find("free="), std::string::npos);
    EXPECT_TRUE(server.hops.empty());
    EXPECT_TRUE(filesUnder(rig.stageDir).empty());
}

TEST(Stage, TheFreeSpaceNeededIsThePlansFigureLessWhatIsAlreadyStagedToTheByte)
{
    // The four files of threeRanges() are 3000 + 40 + 3145745 + 10 = 3148795 bytes, and 2.6 times
    // that is 8186867 exactly (3148795 * 26 = 81868670). A volume with one byte less is refused
    // before anything is asked for; a volume with exactly that much is not.
    const Built built = build(threeRanges());
    {
        Rig rig("stage_space_exact");
        rig.use(built);
        ASSERT_EQ(rig.thePlan.requiredFreeBytes, 8186867u);
        rig.hooked.freeBytes = 8186866;
        PayloadServer refused(built.payload);
        const core::Status status = rig.run(makeFetch(refused.hop()));
        ASSERT_FALSE(status.ok());
        EXPECT_EQ(reasonOf(status), "low_disk_space");
        EXPECT_EQ(status.error().context,
                  "reason=low_disk_space; need=8186867 bytes; free=8186866 bytes");
        EXPECT_TRUE(refused.hops.empty());

        rig.hooked.freeBytes = 8186867;
        PayloadServer enough(built.payload);
        EXPECT_TRUE(rig.run(makeFetch(enough.hop())).ok());
    }
    {
        // a.dll and b.dll (3040 bytes) are staged by a run that is then cancelled. They are on
        // the disk already, so what the next run needs free is 8186867 - 3040 = 8183827.
        Rig rig("stage_space_resumed");
        rig.use(built);
        std::stop_source source;
        PayloadServer first(built.payload);
        first.before = [&](std::size_t index) {
            if (index == 1) {
                source.request_stop();
            }
        };
        ASSERT_FALSE(rig.run(makeFetch(first.hop()), source.get_token()).ok());
        ASSERT_EQ(filesUnder(rig.stageDir), names({"bin/a.dll", "bin/b.dll"}));

        rig.hooked.freeBytes = 8183826;
        PayloadServer refused(built.payload);
        const core::Status status = rig.run(makeFetch(refused.hop()));
        ASSERT_FALSE(status.ok());
        EXPECT_EQ(status.error().context,
                  "reason=low_disk_space; need=8183827 bytes; free=8183826 bytes");
        EXPECT_TRUE(refused.hops.empty());
        EXPECT_EQ(filesUnder(rig.stageDir), names({"bin/a.dll", "bin/b.dll"}))
            << "what was staged is kept for the day there is room";

        rig.hooked.freeBytes = 8183827;
        PayloadServer enough(built.payload);
        EXPECT_TRUE(rig.run(makeFetch(enough.hop())).ok());
    }
}

TEST(Stage, APlanOfTwoAndAHalfTimesTheChangedBytesIsThePlansFigureAndItIsHonoured)
{
    // plan() computes ceil(2.6 * changedBytes) (plan.hpp); stage() takes that figure as given and
    // needs it free, less what is already staged.
    Rig rig("stage_figure");
    const Built built = build(threeRanges());
    rig.use(built);
    const std::uint64_t changed = 3000 + 40 + (3 * 1024 * 1024 + 17) + 10;
    EXPECT_EQ(rig.thePlan.changedBytes, changed);
    EXPECT_EQ(rig.thePlan.requiredFreeBytes, (changed * 26 + 9) / 10);
}

TEST(Stage, AnUnverifiedManifestOrAPlanThatIsNotItsIsRefused)
{
    Rig rig("stage_refused");
    const Built built = build(threeRanges());
    rig.use(built);
    PayloadServer server(built.payload);
    const Fetch fetch = makeFetch(server.hop());

    Manifest unverified = built.manifest;
    unverified.signedJson.clear();
    EXPECT_EQ(reasonOf(stage(fetch, rig.feed, unverified, rig.thePlan, rig.stageDir, {}, {}, rig.hooked)),
              "unverified_manifest");
    unverified = built.manifest;
    unverified.signatureLine.clear();
    EXPECT_EQ(reasonOf(stage(fetch, rig.feed, unverified, rig.thePlan, rig.stageDir, {}, {}, rig.hooked)),
              "unverified_manifest");

    Manifest notify = built.manifest;
    notify.notifyOnly = true;
    EXPECT_EQ(reasonOf(stage(fetch, rig.feed, notify, rig.thePlan, rig.stageDir, {}, {}, rig.hooked)), "notify_only");
    Manifest noPayload = built.manifest;
    noPayload.payload.reset();
    EXPECT_EQ(reasonOf(stage(fetch, rig.feed, noPayload, rig.thePlan, rig.stageDir, {}, {}, rig.hooked)), "notify_only");

    Plan other = rig.thePlan;
    other.files.pop_back();
    EXPECT_EQ(stage(fetch, rig.feed, built.manifest, other, rig.stageDir, {}, {}, rig.hooked).error().code,
              core::ErrorCode::InvalidArgument);
    Plan wild = rig.thePlan;
    wild.files[0].index = 99;
    EXPECT_EQ(stage(fetch, rig.feed, built.manifest, wild, rig.stageDir, {}, {}, rig.hooked).error().code,
              core::ErrorCode::InvalidArgument);
    EXPECT_EQ(stage(fetch, rig.feed, built.manifest, rig.thePlan, fs::path{}, {}, {}, rig.hooked).error().code,
              core::ErrorCode::InvalidArgument);
    EXPECT_EQ(stage(Fetch{}, rig.feed, built.manifest, rig.thePlan, rig.stageDir, {}, {}, rig.hooked).error().code,
              core::ErrorCode::InvalidArgument);
    EXPECT_TRUE(server.hops.empty());
}

TEST(Stage, NothingToChangeStagesTheRecordAlone)
{
    // Every file is already installed and right: no request, and a complete stage all the same.
    Rig rig("stage_nothing");
    const Built built = build({{"bin/a.dll", pattern(100, 1), FileClass::Lib, 0}});
    h::writeText(rig.root / "bin/a.dll", built.at("bin/a.dll").content);
    rig.use(built);
    EXPECT_EQ(rig.thePlan.changedFiles, 0u);
    PayloadServer server(built.payload);
    ASSERT_TRUE(rig.run(makeFetch(server.hop())).ok());
    EXPECT_TRUE(server.hops.empty());
    EXPECT_EQ(filesUnder(rig.stageDir), names({"manifest.json", "manifest.json.sig"}));
}

TEST(Stage, AnEmptyFileIsStagedLikeAnyOther)
{
    Rig rig("stage_empty_file");
    const Built built = build({{"share/empty.txt", "", FileClass::Data, 0},
                               {"share/one.txt", "x", FileClass::Data, 0}});
    rig.use(built);
    PayloadServer server(built.payload);
    ASSERT_TRUE(rig.run(makeFetch(server.hop())).ok());
    EXPECT_EQ(h::readText(rig.stageDir / "share/empty.txt"), "");
    EXPECT_TRUE(h::isPresent(rig.stageDir / "share/empty.txt"));
    EXPECT_EQ(h::readText(rig.stageDir / "share/one.txt"), "x");
}

// ---- one stage at a time ---------------------------------------------------------------------------------------------------------------

TEST(Stage, TheLockOfAStageFolderIsAFileBesideIt)
{
    // stage.hpp: the folder's own name with ".lock" after it, beside the folder - so the folder
    // holds the release's files and the two records, and nothing else.
    EXPECT_EQ(stageLockPath(fs::path("U") / "stage-2000000000"),
              fs::path("U") / "stage-2000000000.lock");
    EXPECT_EQ(stageLockPath(fs::path("U/stage-5/")), fs::path("U/stage-5.lock"))
        << "a trailing separator names the same folder";
}

TEST(Stage, ASecondStageOfTheSameFolderIsRefusedAsBusyAndTouchesNothing)
{
    Rig rig("stage_busy");
    const Built built = build(threeRanges());
    rig.use(built);
    // What another run - the window's, while katana_cli starts this one - has in the folder: a
    // member it is writing, and the record of an earlier complete run.
    h::writeText(rig.stageDir / ".partial" / "2", "another run's half file");
    h::writeText(rig.stageDir / "manifest.json", "another run's record");
    // That run holds the folder's lock. (The harness's file system keeps a lock exclusive
    // between holders, as the real one does between processes.)
    core::Result<std::unique_ptr<FileLock>> other = rig.disk.tryLock(stageLockPath(rig.stageDir));
    ASSERT_TRUE(other.ok()) << other.error().describe();

    PayloadServer server(built.payload);
    const core::Status status = rig.run(makeFetch(server.hop()));
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, core::ErrorCode::AlreadyExists);
    EXPECT_EQ(reasonOf(status), "stage_busy");
    EXPECT_TRUE(server.hops.empty()) << "nothing is asked of the network";
    // Neither the other run's temporary file nor its record was deleted or rewritten.
    EXPECT_EQ(filesUnder(rig.stageDir), names({".partial/2", "manifest.json"}));
    EXPECT_EQ(h::readText(rig.stageDir / ".partial" / "2"), "another run's half file");
    EXPECT_EQ(h::readText(rig.stageDir / "manifest.json"), "another run's record");

    // The other run ends. The same call now goes through and leaves a complete stage ...
    other.value().reset();
    PayloadServer second(built.payload);
    const core::Status again = rig.run(makeFetch(second.hop()));
    ASSERT_TRUE(again.ok()) << again.error().describe();
    EXPECT_EQ(second.ranges.size(), 3u);
    EXPECT_EQ(h::readText(rig.stageDir / "manifest.json"), built.json);
    EXPECT_FALSE(h::isPresent(rig.stageDir / ".partial"));
    // ... and so does one straight after it: the lock is let go when stage() returns.
    PayloadServer third(built.payload);
    EXPECT_TRUE(rig.run(makeFetch(third.hop())).ok());
}

TEST(Stage, WhatStandsInAStagedFilesWayIsRemovedAndTheFileIsStaged)
{
    // Somebody left a folder, with files in it, where bin/b.dll goes, and a file where the
    // folder `share` goes. A stage that stopped at either would stop at it on every later run
    // as well; the names under the stage folder are the stage's (stage.hpp), so they go and
    // every name under the folder is again a verified file.
    Rig rig("stage_in_the_way");
    const Built built = build(threeRanges());
    rig.use(built);
    h::writeText(rig.stageDir / "bin/b.dll" / "inner" / "x.txt", "in the way");
    h::writeText(rig.stageDir / "bin/b.dll" / "y.txt", "in the way too");
    h::writeText(rig.stageDir / "share", "a file named like a folder");

    PayloadServer server(built.payload);
    const core::Status status = rig.run(makeFetch(server.hop()));
    ASSERT_TRUE(status.ok()) << status.error().describe();
    EXPECT_EQ(filesUnder(rig.stageDir),
              names({"bin/a.dll", "bin/b.dll", "share/big.bin", "share/c.txt", "manifest.json",
                     "manifest.json.sig"}));
    for (const Placed& placed : built.placed) {
        EXPECT_EQ(h::readText(rig.stageDir / placed.path), placed.content) << placed.path;
    }
}

// ---- older stage folders ---------------------------------------------------------------------------------------------------------------

TEST(Stage, FoldersOfOlderReleasesAreSweptAndNothingElseIs)
{
    Rig rig("stage_sweep");
    const Built built = build({{"bin/a.dll", pattern(100, 1), FileClass::Lib, 0}});
    rig.use(built);
    const h::Path older1 = rig.stateDir / "stage-1";
    const h::Path older2 = rig.stateDir / "stage-1999999999";
    const h::Path newer = rig.stateDir / "stage-2000000001";
    const h::Path notANumber = rig.stateDir / "stage-abc";
    const h::Path bare = rig.stateDir / "stage-";
    const h::Path other = rig.stateDir / "run-0123abcd";
    const h::Path aFile = rig.stateDir / "stage-5";
    h::writeText(older1 / "bin/old.dll", "old");
    h::writeText(older2 / "share/deep/er/old.txt", "old");
    h::writeText(newer / "bin/new.dll", "newer than this release: not ours to delete");
    h::writeText(notANumber / "x", "x");
    h::writeText(bare / "x", "x");
    h::writeText(other / "katana_updater.exe", "a helper copy");
    h::writeText(aFile, "a file named like a folder");

    PayloadServer server(built.payload);
    ASSERT_TRUE(rig.run(makeFetch(server.hop())).ok());
    EXPECT_FALSE(h::isPresent(older1));
    EXPECT_FALSE(h::isPresent(older2));
    EXPECT_TRUE(h::isPresent(newer / "bin/new.dll"));
    EXPECT_TRUE(h::isPresent(notANumber / "x"));
    EXPECT_TRUE(h::isPresent(bare / "x"));
    EXPECT_TRUE(h::isPresent(other / "katana_updater.exe"));
    EXPECT_TRUE(h::isPresent(aFile)) << "only folders are swept";
    EXPECT_TRUE(h::isPresent(rig.stageDir / "bin/a.dll"));
}

TEST(Stage, AnOlderFolderThatAnotherProgramIsFillingIsLeftAndTheLockFilesGoWithTheirFolders)
{
    Rig rig("stage_sweep_locked");
    const Built built = build({{"bin/a.dll", pattern(100, 1), FileClass::Lib, 0}});
    rig.use(built);
    const h::Path busy = rig.stateDir / "stage-7";
    const h::Path idle = rig.stateDir / "stage-8";
    // The helper deletes the folder of a release it installed, and not the lock beside it.
    const h::Path orphanLock = rig.stateDir / "stage-9.lock";
    const h::Path newerLock = rig.stateDir / "stage-2000000001.lock";
    h::writeText(busy / "bin/half.dll", "being downloaded");
    h::writeText(idle / "bin/old.dll", "old");
    h::writeText(stageLockPath(idle), "");
    h::writeText(orphanLock, "");
    h::writeText(newerLock, "");
    core::Result<std::unique_ptr<FileLock>> filling = rig.disk.tryLock(stageLockPath(busy));
    ASSERT_TRUE(filling.ok()) << filling.error().describe();

    PayloadServer server(built.payload);
    ASSERT_TRUE(rig.run(makeFetch(server.hop())).ok());
    EXPECT_TRUE(h::isPresent(busy / "bin/half.dll")) << "another program holds its lock";
    EXPECT_TRUE(h::isPresent(stageLockPath(busy)));
    EXPECT_FALSE(h::isPresent(idle));
    EXPECT_FALSE(h::isPresent(stageLockPath(idle)));
    EXPECT_FALSE(h::isPresent(orphanLock));
    EXPECT_TRUE(h::isPresent(newerLock)) << "a newer release's is not this one's to delete";
    EXPECT_TRUE(h::isPresent(stageLockPath(rig.stageDir)))
        << "the lock's file stays when the lock is let go";

    // That program ends: the next stage sweeps its folder as well.
    filling.value().reset();
    PayloadServer second(built.payload);
    ASSERT_TRUE(rig.run(makeFetch(second.hop())).ok());
    EXPECT_FALSE(h::isPresent(busy));
    EXPECT_FALSE(h::isPresent(stageLockPath(busy)));
}

// ---- the private-repository route -----------------------------------------------------------------------------------------------------------

TEST(Stage, OnTheApiRouteThePayloadIsFoundByTagAndTheTokenNeverReachesTheStorageHost)
{
    Rig rig("stage_api");
    const Built built = build(threeRanges());
    rig.use(built);
    rig.feed.route = FeedRoute::Api;
    rig.feed.token = std::string(kSecretToken);

    PayloadServer storage(built.payload);
    std::vector<HttpHop> seen;
    const std::string signedUrl = "https://objects.example.org/blob/abc?sp=r&sig=SIGNATURE-IN-QUERY";
    const HopFn hop = [&](const HttpHop& one, std::stop_token stop, ProgressFn progress)
        -> core::Result<HttpHopReply> {
        seen.push_back(one);
        HttpHopReply reply;
        if (one.url == "https://api.github.com/repos/kjarada/Katana/releases/tags/v1.0.0") {
            reply.status = 200;
            reply.contentType = "application/json";
            reply.body = "{\"assets\":[{\"id\":5,\"name\":\"Katana-update-win64.json\"},"
                         "{\"id\":777,\"name\":\"Katana-1.0.0-win64.update.zip\",\"size\":" +
                         std::to_string(built.payload.size()) + "}]}";
            return reply;
        }
        if (one.url == "https://api.github.com/repos/kjarada/Katana/releases/assets/777") {
            reply.status = 302;
            reply.location = signedUrl;
            return reply;
        }
        EXPECT_EQ(one.url, signedUrl);
        return storage.hop()(one, stop, progress);
    };
    const core::Status status = rig.run(makeFetch(hop));
    ASSERT_TRUE(status.ok()) << status.error().describe();

    // 1 lookup, then for each of the 3 ranges: the API hop and the storage hop.
    ASSERT_EQ(seen.size(), 1u + 3u * 2u);
    const std::string bearer = "Authorization: Bearer " + std::string(kSecretToken);
    const auto has = [](const HttpHop& one, std::string_view line) {
        return std::find(one.headers.begin(), one.headers.end(), line) != one.headers.end();
    };
    EXPECT_TRUE(has(seen[0], bearer));
    EXPECT_TRUE(has(seen[0], "Accept: application/vnd.github+json"));
    for (std::size_t i = 1; i < seen.size(); ++i) {
        const bool apiHop = i % 2 == 1;
        EXPECT_EQ(has(seen[i], bearer), apiHop) << seen[i].url;
        EXPECT_TRUE(has(seen[i], "Accept: application/octet-stream")) << seen[i].url;
        EXPECT_TRUE(rangeOf(seen[i]).has_value()) << seen[i].url;
    }
    for (const Placed& placed : built.placed) {
        EXPECT_EQ(h::readText(rig.stageDir / placed.path), placed.content) << placed.path;
    }
}

TEST(Stage, TheApiRouteRefusesWhatDoesNotAddUp)
{
    const Built built = build(threeRanges());
    const auto lookup = [&](const std::string& body, FeedRoute route = FeedRoute::Api,
                            std::string token = std::string(kSecretToken)) {
        Rig rig("stage_api_bad");
        rig.use(built);
        rig.feed.route = route;
        rig.feed.token = std::move(token);
        std::size_t hops = 0;
        const HopFn hop = [&](const HttpHop&, std::stop_token, ProgressFn) -> core::Result<HttpHopReply> {
            ++hops;
            HttpHopReply reply;
            reply.status = 200;
            reply.body = body;
            return reply;
        };
        return std::make_pair(reasonOf(rig.run(makeFetch(hop))), hops);
    };
    EXPECT_EQ(lookup("{\"assets\":[]}").first, "asset_missing");
    EXPECT_EQ(lookup("{\"assets\":[{\"id\":1,\"name\":\"Katana-1.0.0-win64.update.zip\",\"size\":12}]}").first,
              "payload_mismatch")
        << "the listing's length is not the signed one: the release may have been replaced";
    EXPECT_EQ(lookup("not json").first, "malformed");
    const auto noToken = lookup("{}", FeedRoute::Api, "");
    EXPECT_EQ(noToken.first, "no_token");
    EXPECT_EQ(noToken.second, 0u) << "no token, no request";
}

TEST(Stage, TheDownloadRouteNeverCarriesATokenEvenWhenTheConfigHoldsOne)
{
    Rig rig("stage_download_token");
    const Built built = build(threeRanges());
    rig.use(built);
    rig.feed.token = std::string(kSecretToken); // route stays Download
    PayloadServer server(built.payload);
    server.url = "https://github.com/kjarada/Katana/releases/download/v1.0.0/Katana-1.0.0-win64.update.zip";
    ASSERT_TRUE(rig.run(makeFetch(server.hop())).ok());
    ASSERT_EQ(server.hops.size(), 3u);
    for (const HttpHop& hop : server.hops) {
        for (const std::string& header : hop.headers) {
            EXPECT_EQ(header.find(kSecretToken), std::string::npos) << header;
            EXPECT_NE(header.rfind("Authorization", 0), 0u);
        }
        EXPECT_EQ(hop.userAgent, "Katana/0.9.0 (win64)");
    }
}

TEST(Stage, NoErrorOfAStageNamesTheToken)
{
    Rig rig("stage_token_errors");
    const Built built = build(threeRanges());
    rig.use(built);
    rig.feed.route = FeedRoute::Api;
    rig.feed.token = std::string(kSecretToken);
    for (const int statusCode : {401, 403, 404, 429}) {
        const HopFn hop = [&](const HttpHop&, std::stop_token, ProgressFn) -> core::Result<HttpHopReply> {
            HttpHopReply reply;
            reply.status = statusCode;
            return reply;
        };
        const core::Status status = rig.run(makeFetch(hop));
        ASSERT_FALSE(status.ok());
        EXPECT_EQ(status.error().describe().find(kSecretToken), std::string::npos) << statusCode;
    }
}

// ---- preflight ----------------------------------------------------------------------------------------------------------------------------------

// R with an installed helper, and a state folder.
struct PreflightRig : Rig {
    explicit PreflightRig(const std::string& label) : Rig(label)
    {
        helperPath = installedProgram(root, "katana_updater");
        h::writeText(helperPath, "INSTALLED HELPER, protocol 1");
        h::writeText(root / "share/katana/install.json", "{\"schema\":1,\"target\":\"win64\"}");
        fs::create_directories(stateDir);
    }

    [[nodiscard]] std::vector<std::string> runFolders() const
    {
        std::vector<std::string> found;
        std::error_code ec;
        for (const auto& entry : fs::directory_iterator(stateDir, ec)) {
            const std::string name = core::pathToUtf8(entry.path().filename());
            if (name.starts_with("run-")) {
                found.push_back(name);
            }
        }
        return found;
    }

    h::Path helperPath;
};

Manifest manifestWithHelper(std::uint32_t applier, std::string_view helperContent, const h::Path& root)
{
    const std::string helperName = core::pathToUtf8(installedProgram(root, "katana_updater").filename());
    const Built built = build({{"bin/" + helperName, std::string(helperContent), FileClass::Program, 0},
                               {"share/other.txt", "o", FileClass::Data, 0}},
                              applier);
    return built.manifest;
}

TEST(Preflight, TheInstalledHelperIsCopiedToTheRunFolderAndRunWithSelftest)
{
    PreflightRig rig("preflight_ok");
    h::FakeLauncher launcher;
    const Manifest manifest = manifestWithHelper(1, "STAGED HELPER", rig.root);
    const core::Result<PreparedApply> prepared =
        preflight(manifest, rig.install, rig.stageDir, rig.stateDir, launcher, rig.hooked);
    ASSERT_TRUE(prepared.ok()) << prepared.error().describe();

    EXPECT_TRUE(isValidTransactionId(prepared->transaction));
    EXPECT_EQ(prepared->runDirectory, runPath(rig.stateDir, prepared->transaction));
    EXPECT_EQ(prepared->helper.parent_path(), prepared->runDirectory);
    EXPECT_EQ(prepared->helper.filename(), rig.helperPath.filename());
    EXPECT_EQ(h::readText(prepared->helper), "INSTALLED HELPER, protocol 1")
        << "the installed helper, because the manifest needs no newer protocol";

    ASSERT_EQ(launcher.ran.size(), 1u);
    EXPECT_EQ(launcher.ran[0].executable, prepared->helper);
    EXPECT_EQ(launcher.ran[0].arguments, (std::vector<std::string>{"--selftest"}));
    EXPECT_TRUE(launcher.started.empty()) << "preflight starts nothing that outlives it";
    EXPECT_EQ(rig.runFolders().size(), 1u);
    // Nothing of the install was touched.
    EXPECT_EQ(h::readText(rig.helperPath), "INSTALLED HELPER, protocol 1");
}

TEST(Preflight, ANewerProtocolRunsTheStagedHelperAfterItsHashIsChecked)
{
    PreflightRig rig("preflight_staged");
    h::FakeLauncher launcher;
    const Manifest manifest = manifestWithHelper(kApplierProtocol + 1, "STAGED HELPER protocol 2", rig.root);
    const std::string name = core::pathToUtf8(rig.helperPath.filename());

    // Not staged yet: refused, with no run folder left behind.
    auto missing = preflight(manifest, rig.install, rig.stageDir, rig.stateDir, launcher, rig.hooked);
    ASSERT_FALSE(missing.ok());
    EXPECT_EQ(fetchFailureReason(missing.error()), "helper_missing");
    EXPECT_TRUE(rig.runFolders().empty());
    EXPECT_TRUE(launcher.ran.empty());

    // Staged but not the signed bytes: refused.
    h::writeText(rig.stageDir / "bin" / name, "SOMETHING ELSE");
    auto wrong = preflight(manifest, rig.install, rig.stageDir, rig.stateDir, launcher, rig.hooked);
    ASSERT_FALSE(wrong.ok());
    EXPECT_EQ(fetchFailureReason(wrong.error()), "helper_missing");
    EXPECT_TRUE(rig.runFolders().empty());

    // Staged and right: that one runs, not the installed one.
    h::writeText(rig.stageDir / "bin" / name, "STAGED HELPER protocol 2");
    const auto prepared = preflight(manifest, rig.install, rig.stageDir, rig.stateDir, launcher, rig.hooked);
    ASSERT_TRUE(prepared.ok()) << prepared.error().describe();
    EXPECT_EQ(h::readText(prepared->helper), "STAGED HELPER protocol 2");
    ASSERT_EQ(launcher.ran.size(), 1u);
    EXPECT_EQ(launcher.ran[0].executable, prepared->helper);
    EXPECT_EQ(h::readText(rig.helperPath), "INSTALLED HELPER, protocol 1") << "the install is untouched";
}

TEST(Preflight, AStagedHelperThatChangesBetweenTheCheckAndTheCopyIsNotRun)
{
    // The stage folder is the user's to write. Here the staged helper is the signed one when it
    // is hashed there, and another file by the time it is copied: it is swapped at the last
    // moment there is, as the copy begins. What must be the signed helper is the COPY in the run
    // folder - the file that would run. First a swap that changes the length, then one that
    // keeps it (24 bytes each): it is the hash that is held, not the size.
    for (const std::string swapped : {"ANOTHER PROGRAM ALTOGETHER", "STAGED HELPER protocol 3"}) {
        PreflightRig rig("preflight_swapped");
        h::FakeLauncher launcher;
        const Manifest manifest =
            manifestWithHelper(kApplierProtocol + 1, "STAGED HELPER protocol 2", rig.root);
        h::writeText(rig.stageDir / "bin" / rig.helperPath.filename(), "STAGED HELPER protocol 2");
        rig.hooked.beforeCopy = [&](const h::Path& from,
                                    const h::Path&) -> std::optional<core::Status> {
            h::writeText(from, swapped);
            return std::nullopt; // and the copy is made, of what is there now
        };
        const auto prepared =
            preflight(manifest, rig.install, rig.stageDir, rig.stateDir, launcher, rig.hooked);
        ASSERT_FALSE(prepared.ok()) << swapped;
        EXPECT_EQ(prepared.error().code, core::ErrorCode::InvalidState) << swapped;
        EXPECT_EQ(fetchFailureReason(prepared.error()), "helper_missing") << swapped;
        EXPECT_TRUE(launcher.ran.empty()) << "what was copied is not the signed helper: " << swapped;
        EXPECT_TRUE(rig.runFolders().empty()) << swapped;
    }
}

TEST(Preflight, ANewerProtocolWithNoHelperInTheManifestIsRefused)
{
    PreflightRig rig("preflight_no_helper");
    h::FakeLauncher launcher;
    const Built built = build({{"share/other.txt", "o", FileClass::Data, 0}}, kApplierProtocol + 1);
    const auto prepared = preflight(built.manifest, rig.install, rig.stageDir, rig.stateDir, launcher, rig.hooked);
    ASSERT_FALSE(prepared.ok());
    EXPECT_EQ(fetchFailureReason(prepared.error()), "helper_missing");
    EXPECT_TRUE(rig.runFolders().empty());
}

TEST(Preflight, AHelperThatFailsItsSelftestIsReportedAndLeavesNoRunFolder)
{
    PreflightRig rig("preflight_fail");
    const Manifest manifest = manifestWithHelper(1, "x", rig.root);

    {
        h::FakeLauncher launcher;
        launcher.onRun = [](const SpawnRequest&) -> core::Result<SpawnResult> {
            SpawnResult result;
            result.exitCode = 3;
            result.output = "selftest: OpenSSL could not be initialised\n";
            return result;
        };
        const auto prepared = preflight(manifest, rig.install, rig.stageDir, rig.stateDir, launcher, rig.hooked);
        ASSERT_FALSE(prepared.ok());
        EXPECT_EQ(fetchFailureReason(prepared.error()), "helper_failed");
        EXPECT_NE(prepared.error().context.find("exit=3"), std::string::npos);
        EXPECT_NE(prepared.error().context.find("OpenSSL could not be initialised"), std::string::npos);
        EXPECT_TRUE(rig.runFolders().empty());
    }
    {
        h::FakeLauncher launcher;
        launcher.onRun = [](const SpawnRequest&) -> core::Result<SpawnResult> {
            SpawnResult result;
            result.timedOut = true;
            return result;
        };
        const auto prepared = preflight(manifest, rig.install, rig.stageDir, rig.stateDir, launcher, rig.hooked);
        ASSERT_FALSE(prepared.ok());
        EXPECT_EQ(fetchFailureReason(prepared.error()), "helper_timeout");
        EXPECT_TRUE(rig.runFolders().empty());
    }
    {
        h::FakeLauncher launcher;
        launcher.onRun = [](const SpawnRequest&) -> core::Result<SpawnResult> {
            return core::makeError(core::ErrorCode::FileImportFailure, "CreateProcess failed: 193");
        };
        const auto prepared = preflight(manifest, rig.install, rig.stageDir, rig.stateDir, launcher, rig.hooked);
        ASSERT_FALSE(prepared.ok());
        EXPECT_EQ(fetchFailureReason(prepared.error()), "helper_not_started");
        EXPECT_NE(prepared.error().message.find("193"), std::string::npos);
        EXPECT_TRUE(rig.runFolders().empty());
    }
}

TEST(Preflight, AnInstallWithoutAHelperIsRefusedBeforeAnythingIsCreated)
{
    PreflightRig rig("preflight_absent");
    fs::remove(rig.helperPath);
    h::FakeLauncher launcher;
    const Manifest manifest = manifestWithHelper(1, "x", rig.root);
    const auto prepared = preflight(manifest, rig.install, rig.stageDir, rig.stateDir, launcher, rig.hooked);
    ASSERT_FALSE(prepared.ok());
    EXPECT_EQ(fetchFailureReason(prepared.error()), "helper_missing");
    EXPECT_TRUE(rig.runFolders().empty());
    EXPECT_TRUE(launcher.ran.empty());

    EXPECT_EQ(preflight(manifest, rig.install, rig.stageDir, fs::path{}, launcher, rig.hooked).error().code,
              core::ErrorCode::InvalidArgument);
}

TEST(Preflight, EachRunGetsItsOwnTransactionAndFolder)
{
    PreflightRig rig("preflight_twice");
    h::FakeLauncher launcher;
    const Manifest manifest = manifestWithHelper(1, "x", rig.root);
    const auto first = preflight(manifest, rig.install, rig.stageDir, rig.stateDir, launcher, rig.hooked);
    const auto second = preflight(manifest, rig.install, rig.stageDir, rig.stateDir, launcher, rig.hooked);
    ASSERT_TRUE(first.ok());
    ASSERT_TRUE(second.ok());
    EXPECT_NE(first->transaction, second->transaction);
    EXPECT_NE(first->runDirectory, second->runDirectory);
    EXPECT_EQ(rig.runFolders().size(), 2u);
}

TEST(Preflight, AHelperThatCannotBeCopiedIsAWriteFailureThatLeavesNoRunFolderAndStartsNothing)
{
    PreflightRig rig("preflight_copy_fails");
    h::FakeLauncher launcher;
    const Manifest manifest = manifestWithHelper(1, "x", rig.root);
    rig.hooked.beforeCopy = [](const h::Path&, const h::Path& to) -> std::optional<core::Status> {
        return h::injected(112, to); // ERROR_DISK_FULL
    };
    const auto prepared =
        preflight(manifest, rig.install, rig.stageDir, rig.stateDir, launcher, rig.hooked);
    ASSERT_FALSE(prepared.ok());
    EXPECT_EQ(prepared.error().code, core::ErrorCode::FileExportFailure);
    EXPECT_EQ(fetchFailureReason(prepared.error()), "write_failed");
    EXPECT_NE(prepared.error().context.find("os_error=112"), std::string::npos)
        << prepared.error().context;
    EXPECT_TRUE(rig.runFolders().empty());
    EXPECT_TRUE(launcher.ran.empty()) << "a helper that was not copied is not started";
}

// ---- once through the file system that ships --------------------------------------------------

#ifdef _WIN32

// Everything above hands stage() and preflight() the harness's portable file system. The
// programs call the forms without one, which use makeSystemFsOps: long paths, and a rename that
// waits out a virus scanner. The fixture release once through those, from an empty stage to a
// preflighted helper, and once more to see the resume find its own files.
TEST(StageOnTheSystemFileSystem, TheFixtureReleaseIsStagedResumedAndItsHelperPreflighted)
{
    FixtureRig rig("stage_system_fs");
    std::vector<std::pair<std::uint64_t, std::uint64_t>> progress;
    const ProgressFn record = [&](std::uint64_t done, std::uint64_t total) {
        progress.emplace_back(done, total);
    };

    PayloadServer server(rig.v2Zip);
    const core::Status status = stage(makeFetch(server.hop()), rig.feed, rig.manifest,
                                      rig.thePlan, rig.stageDir, std::stop_token{}, record);
    ASSERT_TRUE(status.ok()) << status.error().describe();
    EXPECT_EQ(filesUnder(rig.stageDir),
              names({"bin/katana.exe", "bin/libnew-2.dll", "share/katana/help/index.txt",
                     "share/katana/news.txt", "share/katana/readme.txt", "manifest.json",
                     "manifest.json.sig"}));
    for (const char* path : {"bin/katana.exe", "bin/libnew-2.dll", "share/katana/help/index.txt",
                             "share/katana/news.txt", "share/katana/readme.txt"}) {
        EXPECT_EQ(h::readText(rig.stageDir / path), rig.v2File(path)) << path;
    }
    EXPECT_EQ(h::readText(rig.stageDir / "manifest.json"),
              h::readText(h::fixtureFolder() / "release/v2/Katana-update-win64.json"));
    ASSERT_EQ(server.ranges.size(), 1u);
    EXPECT_EQ(server.ranges[0], (Range{44, 2949}));
    ASSERT_FALSE(progress.empty());
    EXPECT_EQ(progress.back(), (std::pair<std::uint64_t, std::uint64_t>{2949, 2949}));

    // One staged file goes missing: the second run finds the other four by their hashes and
    // asks for that file and no other.
    ASSERT_TRUE(fs::remove(rig.stageDir / "share/katana/news.txt"));
    PayloadServer second(rig.v2Zip);
    ASSERT_TRUE(stage(makeFetch(second.hop()), rig.feed, rig.manifest, rig.thePlan, rig.stageDir,
                      std::stop_token{}, ProgressFn{})
                    .ok());
    ASSERT_EQ(second.ranges.size(), 1u);
    EXPECT_EQ(second.ranges[0], (Range{725, 54})) << "news.txt: offset 725, csize 54";
    EXPECT_EQ(h::readText(rig.stageDir / "share/katana/news.txt"),
              rig.v2File("share/katana/news.txt"));

    // The helper: any bytes at its name (the launcher is a fake; nothing is started).
    const h::Path installedHelper = installedProgram(rig.root, "katana_updater");
    h::writeText(installedHelper, "THE INSTALLED HELPER");
    h::FakeLauncher launcher;
    const core::Result<PreparedApply> prepared =
        preflight(rig.manifest, rig.install, rig.stageDir, rig.stateDir, launcher);
    ASSERT_TRUE(prepared.ok()) << prepared.error().describe();
    EXPECT_EQ(prepared->runDirectory, runPath(rig.stateDir, prepared->transaction));
    EXPECT_EQ(h::readText(prepared->helper), "THE INSTALLED HELPER");
    ASSERT_EQ(launcher.ran.size(), 1u);
    EXPECT_EQ(launcher.ran[0].executable, prepared->helper);
    EXPECT_EQ(launcher.ran[0].arguments, (std::vector<std::string>{"--selftest"}));
}

#endif // _WIN32

} // namespace
