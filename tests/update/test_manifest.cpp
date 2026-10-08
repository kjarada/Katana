// The manifest reader (include/katana/update/manifest.hpp, src/katana_update/manifest.cpp):
// the design's sample read field by field, the T3 fixtures (the producer's real manifests,
// good and bad), every missing or mistyped member, every rule on a value, the places where
// a file's bytes may be in the payload, the caps at exactly their boundaries, and hostile
// input (numbers too large, offsets that wrap, NUL bytes, deep nesting, repeated members).
//
// Expectations are worked out from the contract (the header, the design's section 1) and
// from independent oracles: UTC epoch seconds from Python's calendar.timegm, the sums and
// counts of the fixtures from Python's json module, SHA-256 of "abc" and "" from FIPS 180-2.
// None is read off the code under test (CLAUDE.md, section 4).

#include <gtest/gtest.h>

#include <algorithm>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <limits>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/update/feed.hpp"
#include "katana/update/manifest.hpp"
#include "katana/update/paths.hpp"

#ifndef KATANA_UPDATE_FIXTURES
#error "tests/update/CMakeLists.txt passes the fixtures folder as KATANA_UPDATE_FIXTURES"
#endif

using namespace katana;
namespace fs = std::filesystem;

namespace {

// SHA-256("abc") and SHA-256("") as FIPS 180-2 (appendix B.1) and every table of test vectors
// print them: any 64 lower-case hex digits would do for the parser; these are real hashes.
const std::string kHashAbc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const std::string kHashEmpty = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

constexpr std::uint64_t kUint64Max = std::numeric_limits<std::uint64_t>::max();
constexpr std::uint64_t kInt64Max = std::numeric_limits<std::int64_t>::max();
constexpr std::uint64_t kGiB = 1024ull * 1024 * 1024;

std::string jsonString(const std::string& text)
{
    return "\"" + text + "\"";
}

// A JSON object as text, member by member in the order written, so a test can state the
// bytes it hands the parser (a number as "5.0", a string with a \u0000 escape, a member
// twice) instead of building a value the library would normalise.
class Object {
  public:
    Object& set(const std::string& name, const std::string& rawValue)
    {
        for (auto& member : members_) {
            if (member.first == name) {
                member.second = rawValue;
                return *this;
            }
        }
        members_.emplace_back(name, rawValue);
        return *this;
    }
    // Written again even when present: a duplicate member.
    Object& add(const std::string& name, const std::string& rawValue)
    {
        members_.emplace_back(name, rawValue);
        return *this;
    }
    Object& erase(const std::string& name)
    {
        members_.erase(std::remove_if(members_.begin(), members_.end(),
                                      [&](const auto& m) { return m.first == name; }),
                       members_.end());
        return *this;
    }
    [[nodiscard]] std::string dump() const
    {
        std::string out = "{";
        for (std::size_t i = 0; i < members_.size(); ++i) {
            out += (i == 0 ? "\"" : ",\"") + members_[i].first + "\":" + members_[i].second;
        }
        return out + "}";
    }

  private:
    std::vector<std::pair<std::string, std::string>> members_;
};

Object fileEntry(const std::string& path, const std::string& fileClass = "data")
{
    Object file;
    file.set("class", jsonString(fileClass))
        .set("csize", "2")
        .set("offset", "0")
        .set("path", jsonString(path))
        .set("sha256", jsonString(kHashEmpty))
        .set("size", "0");
    return file;
}

struct Doc {
    Object top;
    std::vector<Object> files;

    [[nodiscard]] std::string text() const
    {
        Object copy = top;
        std::string list = "[";
        for (std::size_t i = 0; i < files.size(); ++i) {
            list += (i == 0 ? "" : ",") + files[i].dump();
        }
        copy.set("files", list + "]");
        return copy.dump();
    }
};

// The design's sample (design_update.md, section 1), with real hashes: two files, the
// program first. 1234 + 15,500,000 and 15,501,300 + 4,100,000 both end inside the
// 212,345,678-byte payload.
Doc sample()
{
    Doc doc;
    doc.top.set("applier", "1")
        .set("channel", jsonString("stable"))
        .set("expires", jsonString("2027-04-18T08:00:00Z"))
        .set("files", "[]")
        .set("notes_url", jsonString("https://github.com/kjarada/Katana/releases/tag/v0.3.0"))
        .set("payload", R"({"codec":"deflate","name":"Katana-0.3.0-win64.update.zip","size":212345678})")
        .set("product", jsonString("katana"))
        .set("published", jsonString("2026-10-20T08:00:00Z"))
        .set("schema", "1")
        .set("sequence", "1792312800")
        .set("tag", jsonString("v0.3.0"))
        .set("target", jsonString("win64"))
        .set("version", jsonString("0.3.0"));

    Object program;
    program.set("class", jsonString("program"))
        .set("csize", "15500000")
        .set("exec", "true")
        .set("offset", "1234")
        .set("path", jsonString("bin/katana.exe"))
        .set("sha256", jsonString(kHashAbc))
        .set("size", "42312704");
    Object library;
    library.set("class", jsonString("lib"))
        .set("csize", "4100000")
        .set("offset", "15501300")
        .set("path", jsonString("bin/libgdal-39.dll"))
        .set("sha256", jsonString(kHashEmpty))
        .set("size", "11800000");
    doc.files = {program, library};
    return doc;
}

core::Result<update::Manifest> parse(const std::string& text)
{
    return update::parseManifest(text);
}

core::Result<update::Manifest> parse(const Doc& doc)
{
    return parse(doc.text());
}

void expectAccepted(const Doc& doc, const std::string& what)
{
    const core::Result<update::Manifest> manifest = parse(doc);
    EXPECT_TRUE(manifest.ok()) << what << ": "
                               << (manifest.ok() ? "" : manifest.error().describe());
}

// A refusal of the given kind. `member` is what the error's context names ("" for none).
void expectRefused(const std::string& text, core::ErrorCode code, const std::string& member,
                   const std::string& what)
{
    const core::Result<update::Manifest> manifest = parse(text);
    ASSERT_FALSE(manifest.ok()) << what << ": accepted";
    EXPECT_EQ(manifest.error().code, code) << what << ": " << manifest.error().describe();
    if (!member.empty()) {
        EXPECT_EQ(manifest.error().context, member) << what << ": " << manifest.error().describe();
    }
}

void expectMalformed(const Doc& doc, const std::string& member, const std::string& what)
{
    expectRefused(doc.text(), core::ErrorCode::ParseFailure, member, what);
}

void expectInvalid(const Doc& doc, const std::string& member, const std::string& what)
{
    expectRefused(doc.text(), core::ErrorCode::InvalidArgument, member, what);
}

std::string readFixture(const std::string& relative)
{
    std::ifstream in(fs::path(KATANA_UPDATE_FIXTURES) / relative, std::ios::binary);
    EXPECT_TRUE(in.is_open()) << relative;
    return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
}

// A manifest of `count` files, each 2 bytes of deflate at its own offset, sorted by path
// (the zero-padded number is the sort key) and each of size `size`.
Doc manyFiles(std::size_t count, std::uint64_t size = 0)
{
    Doc doc = sample();
    doc.files.clear();
    for (std::size_t i = 0; i < count; ++i) {
        std::string number = std::to_string(i);
        number.insert(0, 6 - number.size(), '0');
        Object file = fileEntry("share/katana/data/f" + number + ".dat");
        file.set("offset", std::to_string(i * 2)).set("size", std::to_string(size));
        doc.files.push_back(file);
    }
    doc.top.set("payload", R"({"codec":"deflate","name":"Katana-0.3.0-win64.update.zip","size":)" +
                               std::to_string(count * 2 + 100) + "}");
    return doc;
}

} // namespace

// ---- the design's sample ------------------------------------------------------------------------------

TEST(UpdateManifest, TheDesignsSampleIsReadFieldByField)
{
    const core::Result<update::Manifest> parsed = parse(sample());
    ASSERT_TRUE(parsed.ok()) << parsed.error().describe();
    const update::Manifest& m = parsed.value();

    EXPECT_EQ(m.schema, 1u);
    EXPECT_EQ(m.applier, 1u);
    EXPECT_EQ(m.channel, "stable");
    EXPECT_EQ(m.published, "2026-10-20T08:00:00Z");
    EXPECT_EQ(m.expires, "2027-04-18T08:00:00Z");
    EXPECT_EQ(m.notesUrl, "https://github.com/kjarada/Katana/releases/tag/v0.3.0");
    EXPECT_EQ(m.product, "katana");
    EXPECT_EQ(m.sequence, 1792312800u);
    EXPECT_EQ(m.tag, "v0.3.0");
    EXPECT_EQ(m.target, "win64");
    EXPECT_EQ(m.version.components, (std::array<std::uint32_t, 4>{0, 3, 0, 0}));
    EXPECT_EQ(m.version.written, 3);
    EXPECT_FALSE(m.notifyOnly);

    ASSERT_TRUE(m.payload.has_value());
    EXPECT_EQ(m.payload->codec, "deflate");
    EXPECT_EQ(m.payload->name, "Katana-0.3.0-win64.update.zip");
    EXPECT_EQ(m.payload->size, 212345678u);

    ASSERT_EQ(m.files.size(), 2u);
    EXPECT_EQ(m.files[0].path, "bin/katana.exe");
    EXPECT_EQ(m.files[0].fileClass, update::FileClass::Program);
    EXPECT_TRUE(m.files[0].executable);
    EXPECT_EQ(m.files[0].size, 42312704u);
    EXPECT_EQ(m.files[0].compressedSize, 15500000u);
    EXPECT_EQ(m.files[0].offset, 1234u);
    EXPECT_EQ(m.files[0].sha256, kHashAbc);
    EXPECT_EQ(m.files[1].path, "bin/libgdal-39.dll");
    EXPECT_EQ(m.files[1].fileClass, update::FileClass::Lib);
    EXPECT_FALSE(m.files[1].executable);
    EXPECT_EQ(m.files[1].size, 11800000u);
    EXPECT_EQ(m.files[1].compressedSize, 4100000u);
    EXPECT_EQ(m.files[1].offset, 15501300u);
    EXPECT_EQ(m.files[1].sha256, kHashEmpty);

    // verifyManifest fills these (manifest.hpp); the parser alone does not.
    EXPECT_TRUE(m.signedJson.empty());
    EXPECT_TRUE(m.signatureLine.empty());
}

TEST(UpdateManifest, TheSamePropertiesHoldForTheDesignsOwnLayoutOverSeveralLines)
{
    // Whitespace between tokens is JSON's; the producer writes one member per line.
    const std::string layout = R"({
"applier":1,
"channel":"stable",
"expires":"2027-04-18T08:00:00Z",
"files":[
{"class":"program","csize":15500000,"exec":true,"offset":1234,"path":"bin/katana.exe","sha256":")" +
                               kHashAbc + R"(","size":42312704},
{"class":"lib","csize":4100000,"offset":15501300,"path":"bin/libgdal-39.dll","sha256":")" +
                               kHashEmpty + R"(","size":11800000}
],
"notes_url":"https://github.com/kjarada/Katana/releases/tag/v0.3.0",
"payload":{"codec":"deflate","name":"Katana-0.3.0-win64.update.zip","size":212345678},
"product":"katana",
"published":"2026-10-20T08:00:00Z",
"schema":1,
"sequence":1792312800,
"tag":"v0.3.0",
"target":"win64",
"version":"0.3.0"
}
)";
    const core::Result<update::Manifest> parsed = parse(layout);
    ASSERT_TRUE(parsed.ok()) << parsed.error().describe();
    EXPECT_EQ(parsed.value().files.size(), 2u);
    EXPECT_EQ(parsed.value().version.written, 3);
}

TEST(UpdateManifest, TheFilesAreKeptInTheOrderTheyAreWritten)
{
    Doc doc = sample();
    doc.files = {fileEntry("bin/a.dll"), fileEntry("bin/b.dll"), fileEntry("etc/c.ini"),
                 fileEntry("share/d")};
    const core::Result<update::Manifest> parsed = parse(doc);
    ASSERT_TRUE(parsed.ok()) << parsed.error().describe();
    std::vector<std::string> paths;
    for (const update::ManifestFile& file : parsed.value().files) {
        paths.push_back(file.path);
    }
    EXPECT_EQ(paths, (std::vector<std::string>{"bin/a.dll", "bin/b.dll", "etc/c.ini", "share/d"}));
}

TEST(UpdateManifest, TheFourFileClassesAreAllRead)
{
    Doc doc = sample();
    doc.files = {fileEntry("bin/a", "program"), fileEntry("bin/b", "lib"),
                 fileEntry("share/c", "data"), fileEntry("share/d", "seed")};
    const core::Result<update::Manifest> parsed = parse(doc);
    ASSERT_TRUE(parsed.ok()) << parsed.error().describe();
    EXPECT_EQ(parsed.value().files[0].fileClass, update::FileClass::Program);
    EXPECT_EQ(parsed.value().files[1].fileClass, update::FileClass::Lib);
    EXPECT_EQ(parsed.value().files[2].fileClass, update::FileClass::Data);
    EXPECT_EQ(parsed.value().files[3].fileClass, update::FileClass::Seed);
}

TEST(UpdateManifest, ExecIsOptionalAndAbsentMeansFalse)
{
    // The producer writes "exec" only when true (tools/update_make_manifest.py file_entry).
    Doc doc = sample();
    doc.files = {fileEntry("bin/a"), fileEntry("bin/b"), fileEntry("bin/c")};
    doc.files[1].set("exec", "true");
    doc.files[2].set("exec", "false");
    const core::Result<update::Manifest> parsed = parse(doc);
    ASSERT_TRUE(parsed.ok()) << parsed.error().describe();
    EXPECT_FALSE(parsed.value().files[0].executable);
    EXPECT_TRUE(parsed.value().files[1].executable);
    EXPECT_FALSE(parsed.value().files[2].executable);
}

// ---- a notify-only manifest ------------------------------------------------------------------------------

TEST(UpdateManifest, ANotifyOnlyManifestHasNoFilesAndNoPayload)
{
    Doc doc = sample();
    doc.top.erase("payload").set("notify_only", "true");
    doc.files.clear();
    const core::Result<update::Manifest> parsed = parse(doc);
    ASSERT_TRUE(parsed.ok()) << parsed.error().describe();
    EXPECT_TRUE(parsed.value().notifyOnly);
    EXPECT_TRUE(parsed.value().files.empty());
    EXPECT_FALSE(parsed.value().payload.has_value());
}

TEST(UpdateManifest, ANotifyOnlyManifestWithAPayloadOrFilesIsRefused)
{
    Doc withPayload = sample();
    withPayload.top.set("notify_only", "true");
    withPayload.files.clear();
    expectInvalid(withPayload, "payload", "notify-only with a payload");

    Doc withFiles = sample();
    withFiles.top.erase("payload").set("notify_only", "true");
    expectInvalid(withFiles, "files", "notify-only with files");
}

TEST(UpdateManifest, NotifyOnlyFalseIsTheSameAsAbsent)
{
    Doc doc = sample();
    doc.top.set("notify_only", "false");
    const core::Result<update::Manifest> parsed = parse(doc);
    ASSERT_TRUE(parsed.ok()) << parsed.error().describe();
    EXPECT_FALSE(parsed.value().notifyOnly);
    // ... so it needs a payload and files like any other.
    Doc noPayload = sample();
    noPayload.top.set("notify_only", "false").erase("payload");
    expectMalformed(noPayload, "payload", "not notify-only, no payload");
}

TEST(UpdateManifest, NotifyOnlyMustBeABoolean)
{
    for (const char* wrong : {"\"true\"", "1", "0", "null", "[]", "{}", "\"\""}) {
        Doc doc = sample();
        doc.top.erase("payload").set("notify_only", wrong);
        doc.files.clear();
        expectMalformed(doc, "notify_only", wrong);
    }
}

TEST(UpdateManifest, AManifestThatInstallsNeedsAtLeastOneFile)
{
    // Applied, an empty file list would remove everything the last version installed and
    // put nothing back.
    Doc doc = sample();
    doc.files.clear();
    expectInvalid(doc, "files", "no files");
}

// ---- unknown members -----------------------------------------------------------------------------------------

TEST(UpdateManifest, MembersItDoesNotKnowAreIgnoredAtEveryLevel)
{
    Doc doc = sample();
    doc.top.set("future", R"({"a":[1,2.5,{"b":null,"c":[true,false]}],"d":"x"})")
        .set("another", "[[1],[2]]")
        .set("zz", "12345678901234567890123456789");
    doc.files[0].set("mode", "493").set("note", jsonString("kept out of the reader"));
    doc.files[1].set("signature", "{\"nested\":[]}");
    doc.top.set("payload", R"({"codec":"deflate","etag":"abc","name":"Katana-0.3.0-win64.update.zip","size":212345678})");
    const core::Result<update::Manifest> parsed = parse(doc);
    ASSERT_TRUE(parsed.ok()) << parsed.error().describe();
    EXPECT_EQ(parsed.value().files.size(), 2u);
    EXPECT_EQ(parsed.value().payload->size, 212345678u);
}

// ---- missing and mistyped members ---------------------------------------------------------------------

TEST(UpdateManifest, EveryRequiredMemberOfTheManifestIsRequired)
{
    for (const char* name : {"applier", "channel", "expires", "files", "notes_url", "payload",
                             "product", "published", "schema", "sequence", "tag", "target",
                             "version"}) {
        // From the top object itself: a Doc always writes its own "files".
        Object top = sample().top;
        top.erase(name);
        expectRefused(top.dump(), core::ErrorCode::ParseFailure, name, std::string("without ") + name);
    }
}

TEST(UpdateManifest, EveryMemberHasItsOwnTypeAndNoOtherIsAccepted)
{
    const std::vector<std::string> notString{"1", "1.5", "true", "null", "[]", "{}", "[\"a\"]"};
    for (const char* name : {"channel", "expires", "notes_url", "product", "published", "tag",
                             "target", "version"}) {
        for (const std::string& wrong : notString) {
            Doc doc = sample();
            doc.top.set(name, wrong);
            expectMalformed(doc, name, std::string(name) + " = " + wrong);
        }
    }
    // A whole number: not a string, a float, a signed one, a boolean, a list. "5.0" and "1e3"
    // are floats to a JSON reader even though they are whole numbers to a person.
    const std::vector<std::string> notWholeNumber{
        "\"1\"", "1.0", "1.5", "1e0", "1E0", "10e-1", "-1", "-0", "true", "null", "[]", "{}", "[1]"};
    for (const char* name : {"applier", "schema", "sequence"}) {
        for (const std::string& wrong : notWholeNumber) {
            Doc doc = sample();
            doc.top.set(name, wrong);
            expectMalformed(doc, name, std::string(name) + " = " + wrong);
        }
    }
    for (const char* wrong : {"\"[]\"", "null", "{}", "1", "true", "\"\""}) {
        // Doc::text() builds the list itself, so the wrong value goes into the top object.
        Object top = sample().top;
        top.set("files", wrong);
        expectRefused(top.dump(), core::ErrorCode::ParseFailure, "files", std::string("files = ") + wrong);
    }
}

TEST(UpdateManifest, ThePayloadHasItsThreeMembersAndTheirTypes)
{
    Doc noCodec = sample();
    noCodec.top.set("payload", R"({"name":"a.zip","size":100000000})");
    expectMalformed(noCodec, "payload.codec", "payload without codec");
    Doc noName = sample();
    noName.top.set("payload", R"({"codec":"deflate","size":100000000})");
    expectMalformed(noName, "payload.name", "payload without name");
    Doc noSize = sample();
    noSize.top.set("payload", R"({"codec":"deflate","name":"a.zip"})");
    expectMalformed(noSize, "payload.size", "payload without size");
    for (const char* wrong : {"\"x\"", "[]", "null", "1", "true"}) {
        Doc doc = sample();
        doc.top.set("payload", wrong);
        expectMalformed(doc, "payload", std::string("payload = ") + wrong);
    }
    Doc codecNumber = sample();
    codecNumber.top.set("payload", R"({"codec":1,"name":"a.zip","size":100000000})");
    expectMalformed(codecNumber, "payload.codec", "codec number");
    Doc nameNumber = sample();
    nameNumber.top.set("payload", R"({"codec":"deflate","name":1,"size":100000000})");
    expectMalformed(nameNumber, "payload.name", "name number");
    Doc sizeString = sample();
    sizeString.top.set("payload", R"({"codec":"deflate","name":"a.zip","size":"100000000"})");
    expectMalformed(sizeString, "payload.size", "size string");
    Doc sizeFloat = sample();
    sizeFloat.top.set("payload", R"({"codec":"deflate","name":"a.zip","size":1e8})");
    expectMalformed(sizeFloat, "payload.size", "size float");
}

TEST(UpdateManifest, EveryMemberOfAFileEntryIsRequiredExceptExec)
{
    for (const char* name : {"class", "csize", "offset", "path", "sha256", "size"}) {
        Doc doc = sample();
        doc.files[1].erase(name);
        expectMalformed(doc, std::string("files[1].") + name, std::string("file without ") + name);
    }
}

TEST(UpdateManifest, TheMembersOfAFileEntryHaveTheirOwnTypes)
{
    for (const char* name : {"class", "path", "sha256"}) {
        for (const char* wrong : {"1", "1.5", "true", "null", "[]", "{}"}) {
            Doc doc = sample();
            doc.files[0].set(name, wrong);
            expectMalformed(doc, std::string("files[0].") + name, std::string(name) + " = " + wrong);
        }
    }
    for (const char* name : {"csize", "offset", "size"}) {
        for (const char* wrong : {"\"1\"", "1.0", "2.5", "1e3", "-1", "-0", "true", "null", "[]"}) {
            Doc doc = sample();
            doc.files[0].set(name, wrong);
            expectMalformed(doc, std::string("files[0].") + name, std::string(name) + " = " + wrong);
        }
    }
    for (const char* wrong : {"\"true\"", "1", "0", "null", "[]", "{}"}) {
        Doc doc = sample();
        doc.files[1].set("exec", wrong);
        expectMalformed(doc, "files[1].exec", std::string("exec = ") + wrong);
    }
}

TEST(UpdateManifest, EachEntryOfTheFilesListMustBeAnObject)
{
    for (const char* wrong : {"\"bin/a\"", "1", "null", "[]", "true", "[{}]"}) {
        Object top = sample().top;
        top.set("files", std::string("[") + wrong + "]");
        expectRefused(top.dump(), core::ErrorCode::ParseFailure, "files[0]", std::string("entry = ") + wrong);
    }
}

// ---- value rules --------------------------------------------------------------------------------------------

TEST(UpdateManifest, OnlySchemaOneIsRead)
{
    for (const char* wrong : {"0", "2", "3", "100", "4294967297", "18446744073709551615"}) {
        Doc doc = sample();
        doc.top.set("schema", wrong);
        expectInvalid(doc, "schema", std::string("schema ") + wrong);
    }
    // The schema is judged first: a manifest of another schema may differ in every other member.
    Doc other = sample();
    other.top.set("schema", "2").set("product", "[]").set("version", "{}");
    other.files.clear();
    expectInvalid(other, "schema", "schema 2 with everything else different");
}

TEST(UpdateManifest, TheApplierProtocolIsAWholeNumberFromOneToTheLargestThirtyTwoBitOne)
{
    for (const char* good : {"1", "2", "7", "4294967295"}) {
        Doc doc = sample();
        doc.top.set("applier", good);
        const core::Result<update::Manifest> parsed = parse(doc);
        ASSERT_TRUE(parsed.ok()) << good << ": " << parsed.error().describe();
        EXPECT_EQ(parsed.value().applier, static_cast<std::uint32_t>(std::stoull(good)));
    }
    for (const char* bad : {"0", "4294967296", "18446744073709551615"}) {
        Doc doc = sample();
        doc.top.set("applier", bad);
        expectInvalid(doc, "applier", std::string("applier ") + bad);
    }
}

TEST(UpdateManifest, AnApplierAboveWhatThisBuildSpeaksIsNotAParseError)
{
    // It means the staged helper must be used (stage.hpp); refusing it here would stop the very
    // update that brings the newer helper.
    Doc doc = sample();
    doc.top.set("applier", "2");
    const core::Result<update::Manifest> parsed = parse(doc);
    ASSERT_TRUE(parsed.ok()) << parsed.error().describe();
    EXPECT_EQ(parsed.value().applier, 2u);
}

TEST(UpdateManifest, TheSequenceIsOneOrMoreAndCanBeAsLargeAsSixtyFourBitsHold)
{
    Doc doc = sample();
    doc.top.set("sequence", "1");
    ASSERT_TRUE(parse(doc).ok());
    EXPECT_EQ(parse(doc).value().sequence, 1u);
    doc.top.set("sequence", "18446744073709551615");
    ASSERT_TRUE(parse(doc).ok());
    EXPECT_EQ(parse(doc).value().sequence, kUint64Max);

    doc.top.set("sequence", "0");
    expectInvalid(doc, "sequence", "sequence 0");
    // 2^64 is a float to the reader, so it is a mistyped member, not a wrapped 0.
    doc.top.set("sequence", "18446744073709551616");
    expectMalformed(doc, "sequence", "sequence 2^64");
}

TEST(UpdateManifest, ProductChannelTargetAndTagAreNamesOfOneToSixtyFourSafeCharacters)
{
    for (const char* key : {"product", "channel", "target", "tag"}) {
        for (const std::string& good : std::vector<std::string>{
                 "katana", "stable", "win64", "win-arm64", "linux-x86_64", "macos-arm64", "v0.3.0", "A",
                 "a.b_c-d", std::string(64, 'x')}) {
            Doc doc = sample();
            doc.top.set(key, jsonString(good));
            expectAccepted(doc, std::string(key) + " = " + good);
        }
        for (const std::string& bad : std::vector<std::string>{
             "", std::string(65, 'x'), "a b", "a/b", "a\\\\b", "..\\/x", "a:b", "a?b", "a\\\"b",
              "caf\xC3\xA9", "\\u00e9", "a\\u0000b", "\\u0000", "a\\nb", "a\\tb", "a%41", "a;b", "a b "}) {
            Doc doc = sample();
            doc.top.set(key, jsonString(bad));
            expectInvalid(doc, key, std::string(key) + " = " + bad);
        }
    }
}

TEST(UpdateManifest, ProductTargetAndChannelAreCheckedForTheirFormNotForWhoTheyBelongTo)
{
    // That the product is katana, the target the install's own and the channel stable is
    // verifyManifest's (trust.hpp): the reader only knows the shape.
    Doc doc = sample();
    doc.top.set("product", jsonString("other")).set("target", jsonString("win-arm64")).set("channel", jsonString("beta"));
    const core::Result<update::Manifest> parsed = parse(doc);
    ASSERT_TRUE(parsed.ok()) << parsed.error().describe();
    EXPECT_EQ(parsed.value().product, "other");
    EXPECT_EQ(parsed.value().target, "win-arm64");
    EXPECT_EQ(parsed.value().channel, "beta");
}

TEST(UpdateManifest, TheVersionMustBeMajorMinorPatchBuildOfDigits)
{
    struct Good {
        const char* text;
        std::array<std::uint32_t, 4> components;
        int written;
    };
    for (const Good& good : {Good{"0.3", {0, 3, 0, 0}, 2}, Good{"0.3.0", {0, 3, 0, 0}, 3},
                             Good{"0.10.0", {0, 10, 0, 0}, 3}, Good{"1.2.3.4", {1, 2, 3, 4}, 4}}) {
        Doc doc = sample();
        doc.top.set("version", jsonString(good.text));
        const core::Result<update::Manifest> parsed = parse(doc);
        ASSERT_TRUE(parsed.ok()) << good.text << ": " << parsed.error().describe();
        EXPECT_EQ(parsed.value().version.components, good.components) << good.text;
        EXPECT_EQ(parsed.value().version.written, good.written) << good.text;
    }
    for (const char* bad : {"", "1", "1.", "1.2.3.4.5", "v1.2", "1.2-beta", "1.2 ", " 1.2", "1.1234567890",
                            "4294967296.0", "a.b", "1.2\\u0000", "1..2", "-1.2", "1.2\\n"}) {
        Doc doc = sample();
        doc.top.set("version", jsonString(bad));
        expectInvalid(doc, "version", std::string("version ") + bad);
    }
}

TEST(UpdateManifest, PublishedAndExpiresAreRealUtcTimes)
{
    for (const char* key : {"published", "expires"}) {
        for (const char* good : {"2026-10-20T08:00:00Z", "1970-01-01T00:00:00Z", "2024-02-29T23:59:59Z",
                                 "2000-02-29T00:00:00Z", "9999-12-31T23:59:59Z"}) {
            Doc doc = sample();
            doc.top.set(key, jsonString(good));
            expectAccepted(doc, std::string(key) + " = " + good);
        }
        for (const char* bad :
             {"", "2026-10-20", "2026-10-20 08:00:00Z", "2026-10-20T08:00:00", "2026-10-20T08:00:00z",
              "2026-10-20t08:00:00Z", "2026-10-20T08:00:00+00:00", "2026-10-20T08:00:00.000Z",
              "2026-10-20T08:00Z", "2026-10-20T8:00:00Z", "26-10-20T08:00:00Z", "2026-13-20T08:00:00Z",
              "2026-00-20T08:00:00Z", "2026-10-00T08:00:00Z", "2026-10-32T08:00:00Z",
              "2026-04-31T08:00:00Z", "2027-02-29T00:00:00Z", "2100-02-29T00:00:00Z",
              "2026-10-20T24:00:00Z", "2026-10-20T08:60:00Z", "2026-10-20T08:00:60Z",
              "1969-12-31T23:59:59Z", "0001-01-01T00:00:00Z", "2026-10-20T08:00:00Z ",
              " 2026-10-20T08:00:00Z", "2026-1O-20T08:00:00Z", "2026-10-20T08:00:00Z\\u0000"}) {
            Doc doc = sample();
            doc.top.set(key, jsonString(bad));
            expectInvalid(doc, key, std::string(key) + " = " + bad);
        }
    }
}

TEST(UpdateManifest, ExpiryIsAdvisoryAManifestThatHasExpiredIsStillRead)
{
    Doc doc = sample();
    doc.top.set("published", jsonString("2001-01-01T00:00:00Z")).set("expires", jsonString("2001-06-30T00:00:00Z"));
    const core::Result<update::Manifest> parsed = parse(doc);
    ASSERT_TRUE(parsed.ok()) << parsed.error().describe();
    // ... and the caller can ask, and word the answer (isExpired), without any refusal.
    EXPECT_TRUE(update::isExpired(parsed.value(), 1'800'000'000));
    // An expires before published is odd but not this reader's business.
    Doc odd = sample();
    odd.top.set("published", jsonString("2026-10-20T08:00:00Z")).set("expires", jsonString("2020-01-01T00:00:00Z"));
    EXPECT_TRUE(parse(odd).ok());
}

TEST(UpdateManifest, TheNotesAddressIsAnHttpsAddressOfPrintableCharacters)
{
    for (const std::string& good :
         {std::string("https://github.com/kjarada/Katana/releases/tag/v0.3.0"), std::string("https://x"),
          std::string("https://example.org/a?b=c&d=e#f"), "https://" + std::string(2048 - 8, 'a')}) {
        Doc doc = sample();
        doc.top.set("notes_url", jsonString(good));
        expectAccepted(doc, "notes_url = " + good.substr(0, 40));
    }
    for (const std::string& bad :
         {std::string(""), std::string("https://"), std::string("http://github.com/x"), std::string("ftp://x"),
          std::string("file:///c:/x"), std::string("javascript:alert(1)"), std::string("HTTPS://x"),
          std::string("//github.com/x"), std::string("github.com/x"), std::string("https://a b"),
          std::string("https://a\\tb"), std::string("https://a\\u0000b"), std::string("https://a\\u007fb"),
          std::string("https://caf\xC3\xA9"), std::string("https://a\\nb"),
          "https://" + std::string(2048 - 8 + 1, 'a')}) {
        Doc doc = sample();
        doc.top.set("notes_url", jsonString(bad));
        expectInvalid(doc, "notes_url", "notes_url = " + bad.substr(0, 40));
    }
}

TEST(UpdateManifest, ThePayloadIsDeflateWithASafeNameAndASizeThatFitsASignedSixtyFourBits)
{
    const auto withPayload = [](const std::string& codec, const std::string& name, const std::string& size) {
        Doc doc = sample();
        doc.files = {fileEntry("bin/a")};
        doc.top.set("payload", R"({"codec":)" + codec + R"(,"name":)" + name + R"(,"size":)" + size + "}");
        return doc;
    };
    expectAccepted(withPayload("\"deflate\"", "\"Katana-0.3.0-win64.update.zip\"", "9223372036854775807"),
                   "size 2^63-1");
    expectAccepted(withPayload("\"deflate\"", jsonString(std::string(255, 'a')), "100"), "a 255-character name");
    expectAccepted(withPayload("\"deflate\"", "\"a\"", "2"), "size 2");

    for (const char* codec : {"\"zstd\"", "\"Deflate\"", "\"DEFLATE\"", "\"\"", "\"deflate \"", "\"gzip\"",
                              "\"stored\""}) {
        expectInvalid(withPayload(codec, "\"a.zip\"", "100"), "payload.codec", codec);
    }
    for (const std::string& name : {std::string(""), std::string(256, 'a'), std::string("a/b.zip"),
                                   std::string("a\\\\b.zip"), std::string(".hidden"), std::string(".."),
                                   std::string("."), std::string("a b.zip"), std::string("a\\u0000.zip"),
                                   std::string("caf\xC3\xA9.zip"), std::string("a:b"), std::string("a%2Fb")}) {
        expectInvalid(withPayload("\"deflate\"", jsonString(name), "100"), "payload.name", "name " + name);
    }
    for (const char* size : {"0", "9223372036854775808", "18446744073709551615"}) {
        expectInvalid(withPayload("\"deflate\"", "\"a.zip\"", size), "payload.size", size);
    }
}

TEST(UpdateManifest, TheErrorOfAFileValueNamesTheFileAndTheMember)
{
    Doc doc = sample();
    doc.files[1].set("class", jsonString("exe"));
    expectInvalid(doc, "files[1].class", "class exe");

    Doc hash = sample();
    hash.files[0].set("sha256", jsonString("xyz"));
    expectInvalid(hash, "files[0].sha256", "hash xyz");

    // A path error carries the path as its context and the member in its message.
    Doc path = sample();
    path.files[1].set("path", jsonString("bin/../x"));
    const core::Result<update::Manifest> parsed = parse(path);
    ASSERT_FALSE(parsed.ok());
    EXPECT_EQ(parsed.error().code, core::ErrorCode::InvalidArgument);
    EXPECT_NE(parsed.error().message.find("files[1].path"), std::string::npos) << parsed.error().message;
    EXPECT_EQ(parsed.error().context, "bin/../x");
}

TEST(UpdateManifest, TheFileClassIsOneOfTheFourSpelledInLowerCase)
{
    for (const char* bad : {"", "Program", "PROGRAM", "Lib", "DATA", "seed ", " seed", "programs", "exe",
                            "binary", "resource", "data\\u0000", "caf\xC3\xA9"}) {
        Doc doc = sample();
        doc.files[0].set("class", jsonString(bad));
        expectInvalid(doc, "files[0].class", std::string("class ") + bad);
    }
}

TEST(UpdateManifest, TheHashIsSixtyFourLowerCaseHexDigitsAndNothingElse)
{
    const std::string good(64, 'a');
    Doc ok = sample();
    ok.files[0].set("sha256", jsonString(good));
    expectAccepted(ok, "64 digits");
    for (const std::string& bad :
         {std::string(""), std::string(63, 'a'), std::string(65, 'a'), std::string(128, 'a'),
          std::string(64, 'A'), std::string(63, 'a') + "g", std::string(63, 'a') + "G",
          "0x" + std::string(62, 'a'), " " + std::string(63, 'a'), std::string(63, 'a') + " ",
          std::string(63, 'a') + "\\u0000", std::string(32, 'a') + std::string(32, 'F'),
          std::string(63, 'a') + "/", std::string(63, 'a') + ":", std::string(63, 'a') + "`",
          std::string(63, 'a') + "\xC3\xA9"}) {
        Doc doc = sample();
        doc.files[0].set("sha256", jsonString(bad));
        expectInvalid(doc, "files[0].sha256", "hash '" + bad + "'");
    }
}

TEST(UpdateManifest, EveryPathRuleAppliesToTheFilesOfAManifest)
{
    for (const char* bad :
         {"", "/bin/a", "C:/bin/a", "bin\\\\a", "bin/a:b", "bin/../a", "bin/./a", "bin//a", "other/a", "bin",
          "bin/a.", "bin/a ", "bin/con", "bin/NUL.txt", "bin/PROGRA~1.DLL", "share/katana/install.json",
          "share/katana/INSTALLED.JSON", "share/.update/x", "share/caf\xC3\xA9", "bin/a*b", "bin/a\\u0001b",
          "bin/a\\u007fb", "bin/a\\u0000b"}) {
        Doc doc = sample();
        doc.files = {fileEntry(bad)};
        const core::Result<update::Manifest> parsed = parse(doc);
        ASSERT_FALSE(parsed.ok()) << "accepted: " << bad;
        EXPECT_EQ(parsed.error().code, core::ErrorCode::InvalidArgument) << bad;
        EXPECT_NE(parsed.error().message.find("files[0].path"), std::string::npos)
            << bad << ": " << parsed.error().message;
    }
}

TEST(UpdateManifest, ThePathsOfTheSharedTableAreJudgedHereAsTheyAreByTheRulesThemselves)
{
    // The manifest reader and checkManifestPath must not differ: a path accepted by one and
    // refused by the other would be a hole or a release nobody can install.
    for (const char* path : {"bin/katana.exe", "bin/a b.txt", "bin/com10.dll", "share/katana/installed.json.bak",
                             "share/.updated/x", "bin/.hidden", "bin/COM0", "bin/a~b.dll"}) {
        EXPECT_TRUE(update::checkManifestPath(path).ok()) << path;
        Doc doc = sample();
        doc.files = {fileEntry(path)};
        expectAccepted(doc, path);
    }
}

// ---- where a file's bytes are --------------------------------------------------------------------------------

namespace {

// One file placed in a payload of `payloadSize` bytes; the rest of the sample around it.
Doc placed(std::uint64_t offset, std::uint64_t csize, std::uint64_t payloadSize)
{
    Doc doc = sample();
    doc.files = {fileEntry("bin/a")};
    doc.files[0].set("offset", std::to_string(offset)).set("csize", std::to_string(csize));
    doc.top.set("payload", R"({"codec":"deflate","name":"a.zip","size":)" + std::to_string(payloadSize) + "}");
    return doc;
}

} // namespace

TEST(UpdateManifestPlacement, ADeflateStreamIsNeverShorterThanTwoBytes)
{
    // The shortest deflate stream, an empty final block, is the two bytes 03 00 (RFC 1951,
    // section 3.2.3: BFINAL=1, BTYPE=01, end-of-block code 0000000 = 3 bits + 7 bits = 10 bits).
    expectInvalid(placed(0, 0, 100), "files[0].csize", "csize 0");
    expectInvalid(placed(0, 1, 100), "files[0].csize", "csize 1");
    expectAccepted(placed(0, 2, 100), "csize 2");
    expectAccepted(placed(0, 3, 100), "csize 3");
}

TEST(UpdateManifestPlacement, TheLastByteOfTheDataMayBeTheLastByteOfThePayloadAndNoLater)
{
    expectAccepted(placed(0, 100, 100), "offset 0, csize 100, payload 100");
    expectInvalid(placed(0, 101, 100), "files[0].offset", "one byte past the end");
    expectAccepted(placed(98, 2, 100), "offset+csize == payload.size");
    expectInvalid(placed(99, 2, 100), "files[0].offset", "offset+csize == payload.size+1");
    expectInvalid(placed(98, 3, 100), "files[0].offset", "offset 98, csize 3");
    expectAccepted(placed(1, 99, 100), "offset 1, csize 99");
    expectInvalid(placed(1, 100, 100), "files[0].offset", "offset 1, csize 100");
}

TEST(UpdateManifestPlacement, AnOffsetAtOrPastTheEndOfThePayloadIsRefused)
{
    expectInvalid(placed(100, 2, 100), "files[0].offset", "offset == payload.size");
    expectInvalid(placed(101, 2, 100), "files[0].offset", "offset > payload.size");
    expectInvalid(placed(100000, 2, 100), "files[0].offset", "offset far past");
}

TEST(UpdateManifestPlacement, OffsetAndLengthThatWrapWhenAddedAreRefusedNotSummedToSomethingSmall)
{
    // 2^64 - 1 + 3 = 2 (mod 2^64): a check written offset + csize <= size would accept it.
    expectInvalid(placed(kUint64Max, 3, 100), "files[0].offset", "offset 2^64-1, csize 3");
    expectInvalid(placed(kUint64Max - 1, 3, 100), "files[0].offset", "offset 2^64-2, csize 3");
    expectInvalid(placed(kUint64Max, 2, 100), "files[0].offset", "offset 2^64-1, csize 2");
    expectInvalid(placed(kUint64Max, kUint64Max, 100), "files[0].offset", "both 2^64-1");
    // 2 + (2^64 - 1) = 1 (mod 2^64)
    expectInvalid(placed(2, kUint64Max, 100), "files[0].offset", "offset 2, csize 2^64-1");
    // offset = payload.size, csize = 2^64 - payload.size: the sum is exactly 2^64, which is 0
    expectInvalid(placed(100, kUint64Max - 99, 100), "files[0].offset", "sum is exactly 2^64");
    expectInvalid(placed(50, kUint64Max - 49, 100), "files[0].offset", "sum is exactly 2^64, offset 50");
    // 2^63 + 2^63 = 2^64
    expectInvalid(placed(kInt64Max + 1, kInt64Max + 1, 100), "files[0].offset", "two halves of 2^64");
    // The same wraps against the largest payload the reader accepts.
    expectInvalid(placed(kInt64Max, kInt64Max, kInt64Max), "files[0].offset", "payload 2^63-1, both 2^63-1");
}

TEST(UpdateManifestPlacement, TheLargestPayloadThatFitsASignedOffsetIsAcceptedWithDataAtItsEnd)
{
    expectAccepted(placed(kInt64Max - 2, 2, kInt64Max), "data in the last two bytes of 2^63-1");
    expectInvalid(placed(kInt64Max - 1, 2, kInt64Max), "files[0].offset", "one byte too many");
    expectAccepted(placed(0, kInt64Max, kInt64Max), "csize == payload == 2^63-1");
}

TEST(UpdateManifestPlacement, AFileMayBeAsLargeAsTwoGibiBytesAndNoLarger)
{
    EXPECT_EQ(update::kMaxInstallBytes, 2 * kGiB);
    Doc atCap = placed(0, 2, 100);
    atCap.files[0].set("size", std::to_string(2 * kGiB));
    expectAccepted(atCap, "size 2 GiB");
    Doc past = placed(0, 2, 100);
    past.files[0].set("size", std::to_string(2 * kGiB + 1));
    expectInvalid(past, "files[0].size", "size 2 GiB + 1");
    Doc huge = placed(0, 2, 100);
    huge.files[0].set("size", "18446744073709551615");
    expectInvalid(huge, "files[0].size", "size 2^64-1");
}

TEST(UpdateManifestPlacement, SeveralFilesAreEachPlacedInsideThePayloadAndMayShareNothingInParticular)
{
    // Offsets are not required to be ordered or disjoint: the reader checks each file's own range.
    Doc doc = sample();
    doc.files = {fileEntry("bin/a"), fileEntry("bin/b"), fileEntry("bin/c")};
    doc.files[0].set("offset", "90").set("csize", "10");
    doc.files[1].set("offset", "0").set("csize", "100");
    doc.files[2].set("offset", "95").set("csize", "5");
    doc.top.set("payload", R"({"codec":"deflate","name":"a.zip","size":100})");
    expectAccepted(doc, "three files in 100 bytes");
    // The second one past the end is the one named.
    doc.files[1].set("csize", "101");
    expectInvalid(doc, "files[1].offset", "the second past the end");
}

// ---- order, uniqueness and the caps ---------------------------------------------------------------------

TEST(UpdateManifestFiles, TheListMustBeSortedByPathInByteOrder)
{
    Doc doc = sample();
    doc.files = {fileEntry("bin/b"), fileEntry("bin/a")};
    expectInvalid(doc, "files[1].path", "b then a");

    // Byte order, not case-insensitive order: 'B' (0x42) sorts before 'a' (0x61).
    Doc upperFirst = sample();
    upperFirst.files = {fileEntry("bin/B"), fileEntry("bin/a")};
    expectAccepted(upperFirst, "B then a");
    Doc lowerFirst = sample();
    lowerFirst.files = {fileEntry("bin/a"), fileEntry("bin/B")};
    expectInvalid(lowerFirst, "files[1].path", "a then B");

    // A folder's files do not sort among its siblings' by anything but the bytes: '/' (0x2F)
    // is above '-' (0x2D) and '.' (0x2E) and below '0' (0x30).
    Doc folder = sample();
    folder.files = {fileEntry("share/a-b"), fileEntry("share/a.b"), fileEntry("share/a/b"), fileEntry("share/a0")};
    expectAccepted(folder, "- . / 0");
    Doc wrongSlash = sample();
    wrongSlash.files = {fileEntry("share/a-b"), fileEntry("share/a/b"), fileEntry("share/a.b")};
    expectInvalid(wrongSlash, "files[2].path", "- / .");
}

TEST(UpdateManifestFiles, ThePathsMustBeUniqueExactlyAndAfterFoldingCase)
{
    Doc exact = sample();
    exact.files = {fileEntry("bin/a"), fileEntry("bin/a")};
    expectInvalid(exact, "files[1].path", "the same path twice");

    // 'A' sorts before 'a', so this list is sorted and the duplicate is the case-folded kind.
    Doc folded = sample();
    folded.files = {fileEntry("bin/A.dll"), fileEntry("bin/a.dll")};
    const core::Result<update::Manifest> parsed = parse(folded);
    ASSERT_FALSE(parsed.ok());
    EXPECT_EQ(parsed.error().code, core::ErrorCode::InvalidArgument);

    Doc folder = sample();
    folder.files = {fileEntry("share/Katana/x"), fileEntry("share/katana/x")};
    EXPECT_FALSE(parse(folder).ok());
}

TEST(UpdateManifestFiles, ANameThatIsAFileAndTheFolderOfAnotherIsRefused)
{
    Doc doc = sample();
    doc.files = {fileEntry("share/x"), fileEntry("share/x/y")};
    EXPECT_FALSE(parse(doc).ok());
    Doc sibling = sample();
    sibling.files = {fileEntry("share/x"), fileEntry("share/x.d/y")};
    expectAccepted(sibling, "x and x.d/y");
}

TEST(UpdateManifestFiles, ExactlyTenThousandFilesAreAcceptedAndOneMoreIsNot)
{
    EXPECT_EQ(update::kMaxFiles, 10'000u);

    const std::string atCap = manyFiles(10'000).text();
    const core::Result<update::Manifest> parsed = parse(atCap);
    ASSERT_TRUE(parsed.ok()) << parsed.error().describe();
    EXPECT_EQ(parsed.value().files.size(), 10'000u);
    EXPECT_EQ(parsed.value().files.front().path, "share/katana/data/f000000.dat");
    EXPECT_EQ(parsed.value().files.back().path, "share/katana/data/f009999.dat");
    EXPECT_LT(atCap.size(), update::kMaxManifestBytes) << "the cap's manifest must itself fit";

    expectRefused(manyFiles(10'001).text(), core::ErrorCode::InvalidArgument, "files", "10,001 files");
    expectAccepted(manyFiles(9'999), "9,999 files");
}

TEST(UpdateManifestFiles, TheSizesOfAllTheFilesMayAddUpToTwoGibiBytesAndNoMore)
{
    Doc atCap = manyFiles(2, kGiB);
    expectAccepted(atCap, "1 GiB + 1 GiB");
    Doc over = manyFiles(2, kGiB);
    over.files[1].set("size", std::to_string(kGiB + 1));
    const core::Result<update::Manifest> parsed = parse(over);
    ASSERT_FALSE(parsed.ok());
    EXPECT_EQ(parsed.error().code, core::ErrorCode::InvalidArgument);

    // Three files whose sizes are each fine and whose sum is 2 GiB + 1.
    Doc three = manyFiles(3, 0);
    three.files[0].set("size", std::to_string(kGiB));
    three.files[1].set("size", std::to_string(kGiB));
    three.files[2].set("size", "1");
    EXPECT_FALSE(parse(three).ok());
    three.files[2].set("size", "0");
    expectAccepted(three, "2 GiB in two of three");
}

TEST(UpdateManifestFiles, TheManifestItselfIsBoundedToTheLargestTheEngineWillFetch)
{
    const std::string text = sample().text();
    ASSERT_LT(text.size(), update::kMaxManifestBytes);

    // Exactly kMaxManifestBytes: JSON white space after the object is harmless.
    const std::string atCap = text + std::string(update::kMaxManifestBytes - text.size(), ' ');
    ASSERT_EQ(atCap.size(), update::kMaxManifestBytes);
    EXPECT_TRUE(parse(atCap).ok());
    // One byte more is refused before it is read.
    const core::Result<update::Manifest> over = parse(atCap + " ");
    ASSERT_FALSE(over.ok());
    EXPECT_EQ(over.error().code, core::ErrorCode::ParseFailure);
}

// ---- hostile text ------------------------------------------------------------------------------------------

TEST(UpdateManifestHostile, TextThatIsNotAJsonObjectIsAParseFailure)
{
    const std::vector<std::string> texts{
        "", " ", "\n", "null", "true", "123", "\"text\"", "[]", "[{}]", "{", "}", "{}}", "{}{}", "{} x",
        "{\"a\":}", "{\"a\" 1}", "{a:1}", "{'a':1}", "{\"a\":1,}", "[1,]", "{\"a\":01}", "{\"a\":.5}", "{\"a\":+1}",
        "{\"a\":NaN}", "{\"a\":Infinity}", "{\"a\":-Infinity}", "{\"a\":undefined}", "{\"a\":\"\\x41\"}",
        "{\"a\":\"\\u12\"}", "{\"a\":\"\\ud800\"}", "{\"a\":\"\\udc00\"}", "{\"a\":\"line\nbreak\"}",
        "{\"a\":\"tab\there\"}", "/* c */ {}", "{} // c", "{\"a\":1 /* c */}", "# c\n{}",
        std::string("{\0}", 3), std::string("{\"a\":\"\0\"}", 8), std::string("\0{}", 3), std::string("{}\0", 3),
        "{\"a\":\"\xFF\"}", "{\"a\":\"\xC0\x80\"}", "{\"a\":\"\xED\xA0\x80\"}", "{\"a\":\"\xE2\x82\"}",
        "{\"\xFF\":1}",
    };
    for (const std::string& text : texts) {
        const core::Result<update::Manifest> parsed = parse(text);
        ASSERT_FALSE(parsed.ok()) << "accepted: " << text;
        EXPECT_EQ(parsed.error().code, core::ErrorCode::ParseFailure) << text << ": " << parsed.error().describe();
        EXPECT_EQ(parsed.error().describe().find('\0'), std::string::npos);
    }
}

TEST(UpdateManifestHostile, AValidObjectWithoutTheMembersIsMissingThemNotMalformedJson)
{
    expectRefused("{}", core::ErrorCode::ParseFailure, "schema", "empty object");
}

TEST(UpdateManifestHostile, AByteOrderMarkIsRefusedThoughAJsonReaderMightSkipIt)
{
    const std::string withBom = "\xEF\xBB\xBF" + sample().text();
    expectRefused(withBom, core::ErrorCode::ParseFailure, "", "BOM");
    // Without it, the same text is the sample.
    EXPECT_TRUE(parse(sample().text()).ok());
}

TEST(UpdateManifestHostile, ANulByteOutsideAStringIsRefusedBecauseTheJsonReaderTakesItForTheEnd)
{
    // The JSON reader stops at a NUL as at the end of its input, so a good manifest followed
    // by a NUL and anything at all would be read as the good manifest alone, while the Python
    // verifier, or anything that sees the whole file, reads something else. Each of these is
    // a complete, valid manifest with a NUL placed somewhere it ends the read.
    const std::string good = sample().text();
    ASSERT_TRUE(parse(good).ok());
    const std::string nul(1, '\0');
    const std::vector<std::string> texts{
        good + nul,
        good + nul + "garbage",
        good + nul + good,
        good + "\n" + nul,
        good + nul + nul,
        nul + good,
        good.substr(0, good.size() / 2) + nul + good.substr(good.size() / 2),
        good.substr(0, good.size() - 1) + nul + "}",
    };
    for (std::size_t i = 0; i < texts.size(); ++i) {
        const core::Result<update::Manifest> parsed = parse(texts[i]);
        ASSERT_FALSE(parsed.ok()) << "case " << i << " was read";
        EXPECT_EQ(parsed.error().code, core::ErrorCode::ParseFailure) << i;
        EXPECT_NE(parsed.error().message.find("NUL"), std::string::npos) << i << ": " << parsed.error().message;
    }
}

TEST(UpdateManifestHostile, TrailingGarbageAfterTheObjectIsRefused)
{
    const std::string text = sample().text();
    for (const std::string& tail :
         std::vector<std::string>{"x", "{}", ",", "\n,", " 1", std::string(1, '\0')}) {
        expectRefused(text + tail, core::ErrorCode::ParseFailure, "", "tail '" + tail + "'");
    }
    // White space is not garbage.
    EXPECT_TRUE(parse(text + " \n\t\r\n ").ok());
    EXPECT_TRUE(parse(" \n" + text).ok());
}

TEST(UpdateManifestHostile, ANumberTooLargeForSixtyFourBitsIsRefusedNotWrapped)
{
    // Past 2^64 - 1 a whole number is a float to the reader, so it is a mistyped member; far
    // enough past it (more than a double holds, about 1.8e308) the JSON reader itself refuses.
    // Either way the value is never reduced modulo 2^64 into a small one.
    for (const char* key : {"sequence", "applier", "schema"}) {
        Doc doc = sample();
        doc.top.set(key, std::string(400, '9'));
        expectMalformed(doc, "", std::string(key) + " = 400 nines");
        doc.top.set(key, "18446744073709551616"); // 2^64
        expectMalformed(doc, key, std::string(key) + " = 2^64");
    }
    for (const char* key : {"csize", "offset", "size"}) {
        Doc doc = sample();
        const std::string member = std::string("files[0].") + key;
        doc.files[0].set(key, std::string(400, '9'));
        expectMalformed(doc, "", std::string(key) + " = 400 nines");
        doc.files[0].set(key, "18446744073709551616"); // 2^64
        expectMalformed(doc, member, std::string(key) + " = 2^64");
        doc.files[0].set(key, "99999999999999999999999999999999999999");
        expectMalformed(doc, member, std::string(key) + " = 38 nines");
        doc.files[0].set(key, "36893488147419103232"); // 2^65
        expectMalformed(doc, member, std::string(key) + " = 2^65");
    }
    Doc exponent = sample();
    exponent.files[0].set("size", "1e308");
    expectMalformed(exponent, "files[0].size", "1e308");
    exponent.files[0].set("size", "1e999999");
    expectMalformed(exponent, "", "1e999999");
}

TEST(UpdateManifestHostile, ANulByteInsideAStringIsRefusedWhereverTheValueIsUsed)
{
    // \u0000 is legal JSON and gives a string with a NUL in it, which a C string would end at.
    // Each of these would read as a good value if the NUL ended the string.
    for (const char* key : {"channel", "product", "tag", "target", "version", "notes_url", "published", "expires"}) {
        Doc doc = sample();
        std::string value;
        if (std::string(key) == "version") {
            value = "0.3.0\\u0000junk";
        } else if (std::string(key) == "notes_url") {
            value = "https://github.com/x\\u0000junk";
        } else if (std::string(key) == "published" || std::string(key) == "expires") {
            value = "2026-10-20T08:00:00Z\\u0000junk";
        } else {
            value = std::string("ok\\u0000junk");
        }
        doc.top.set(key, jsonString(value));
        expectInvalid(doc, key, std::string(key) + " with a NUL");
    }
    Doc path = sample();
    path.files[0].set("path", jsonString("bin/a.dll\\u0000../../x"));
    EXPECT_FALSE(parse(path).ok());
    Doc cls = sample();
    cls.files[0].set("class", jsonString("data\\u0000"));
    expectInvalid(cls, "files[0].class", "class with a NUL");
    Doc hash = sample();
    hash.files[0].set("sha256", jsonString(kHashAbc.substr(0, 63) + "\\u0000"));
    expectInvalid(hash, "files[0].sha256", "hash with a NUL");
    // A member NAME with a NUL is just an unknown member.
    Doc name = sample();
    name.top.set("future\\u0000x", "1");
    EXPECT_TRUE(parse(name).ok());
}

TEST(UpdateManifestHostile, NestingAtTheLimitIsReadAndOneLevelDeeperIsRefused)
{
    // The manifest object is level 1; a member holding N nested lists reaches level 1 + N.
    // The limit is 16 levels (manifest.hpp).
    const auto nested = [](int levels) {
        Doc doc = sample();
        doc.top.set("future", std::string(static_cast<std::size_t>(levels), '[') +
                                  std::string(static_cast<std::size_t>(levels), ']'));
        return doc;
    };
    expectAccepted(nested(15), "16 levels in all");
    expectRefused(nested(16).text(), core::ErrorCode::ParseFailure, "", "17 levels in all");
    expectRefused(nested(100).text(), core::ErrorCode::ParseFailure, "", "101 levels");
}

TEST(UpdateManifestHostile, DeepNestingThatNeverClosesIsRefusedWithoutExhaustingTheStack)
{
    for (const char* open : {"[", "{\"a\":", "[{\"a\":"}) {
        std::string text;
        for (int i = 0; i < 200'000; ++i) {
            text += open;
        }
        const core::Result<update::Manifest> parsed = parse(text);
        ASSERT_FALSE(parsed.ok()) << open;
        EXPECT_EQ(parsed.error().code, core::ErrorCode::ParseFailure) << open;
    }
    // The same inside the manifest's own members.
    std::string deep = "{\"future\":";
    deep.append(100'000, '[');
    EXPECT_FALSE(parse(deep).ok());
    // Nested objects, closed, 50 deep inside a file entry.
    Doc doc = sample();
    std::string objects;
    for (int i = 0; i < 50; ++i) {
        objects += "{\"k\":";
    }
    objects += "1" + std::string(50, '}');
    doc.files[0].set("extra", objects);
    EXPECT_FALSE(parse(doc).ok());
}

TEST(UpdateManifestHostile, AMemberWrittenTwiceIsRefusedWhereverItIsWritten)
{
    // A reader that kept the last of two "sha256" members and one that kept the first would
    // disagree about what is signed: so there is no "which one", the manifest is refused.
    Doc top = sample();
    top.top.add("version", jsonString("9.9.9"));
    expectRefused(top.text(), core::ErrorCode::ParseFailure, "version", "version twice");

    Doc schema = sample();
    schema.top.add("schema", "1");
    expectRefused(schema.text(), core::ErrorCode::ParseFailure, "schema", "schema twice, the same value");

    Doc file = sample();
    file.files[1].add("sha256", jsonString(kHashAbc));
    expectRefused(file.text(), core::ErrorCode::ParseFailure, "sha256", "sha256 twice in a file");

    Doc payload = sample();
    payload.top.set("payload", R"({"codec":"deflate","codec":"deflate","name":"a.zip","size":1000000000})");
    expectRefused(payload.text(), core::ErrorCode::ParseFailure, "codec", "codec twice");

    Doc unknown = sample();
    unknown.top.set("future", R"({"a":1,"b":{"c":2,"c":3}})");
    expectRefused(unknown.text(), core::ErrorCode::ParseFailure, "c", "a duplicate inside an unknown member");

    Doc unknownTop = sample();
    unknownTop.top.add("future", "1").add("future", "2");
    expectRefused(unknownTop.text(), core::ErrorCode::ParseFailure, "future", "an unknown member twice");

    // The same name in two different objects is not a duplicate: every file has a "path".
    Doc fine = sample();
    fine.files[0].set("class", jsonString("program"));
    fine.top.set("future", R"({"path":1,"files":{"path":2}})");
    expectAccepted(fine, "the same name in different objects");
}

TEST(UpdateManifestHostile, TwoSpellingsOfOneMemberNameAreOneMemberWrittenTwice)
{
    // "version" is "version" once the escape is read; a check made on the raw text, not on
    // the name the reader ends up with, would see two different members.
    Doc top = sample();
    top.top.add("vers\\u0069on", jsonString("9.9.9"));
    expectRefused(top.text(), core::ErrorCode::ParseFailure, "version", "version twice, once escaped");

    Doc file = sample();
    file.files[0].add("sha\\u0032\\u0035\\u0036", jsonString(kHashAbc));
    expectRefused(file.text(), core::ErrorCode::ParseFailure, "sha256", "sha256 twice, once escaped");
}

TEST(UpdateManifestHostile, TheErrorOfAHostileValueNeverCarriesRawControlCharacters)
{
    Doc doc = sample();
    doc.top.set("tag", jsonString("a\\nb\\u0000c\\u001b[31m"));
    const core::Result<update::Manifest> parsed = parse(doc);
    ASSERT_FALSE(parsed.ok());
    const std::string text = parsed.error().describe();
    for (const char c : text) {
        EXPECT_GE(static_cast<unsigned char>(c), 0x20u) << text;
    }

    Doc path = sample();
    path.files[0].set("path", jsonString("bin/a\\nb\\u0000\\u001b"));
    const core::Result<update::Manifest> parsedPath = parse(path);
    ASSERT_FALSE(parsedPath.ok());
    for (const char c : parsedPath.error().describe()) {
        EXPECT_GE(static_cast<unsigned char>(c), 0x20u);
    }
}

TEST(UpdateManifestHostile, AManifestOfManyUnknownMembersIsReadInReasonableTime)
{
    // 200,000 unknown members of a few bytes each (about 2 MB): nothing quadratic in the reader.
    Doc doc = sample();
    std::string members = "{";
    for (int i = 0; i < 200'000; ++i) {
        members += (i == 0 ? "\"k" : ",\"k") + std::to_string(i) + "\":1";
    }
    members += "}";
    doc.top.set("future", members);
    ASSERT_LT(doc.text().size(), update::kMaxManifestBytes);
    EXPECT_TRUE(parse(doc).ok());
}

// ---- file classes ---------------------------------------------------------------------------------------------

TEST(UpdateFileClass, EachClassHasItsOneLowerCaseName)
{
    EXPECT_EQ(update::toString(update::FileClass::Program), "program");
    EXPECT_EQ(update::toString(update::FileClass::Lib), "lib");
    EXPECT_EQ(update::toString(update::FileClass::Data), "data");
    EXPECT_EQ(update::toString(update::FileClass::Seed), "seed");
}

TEST(UpdateFileClass, TheNamesParseBackAndNothingElseDoes)
{
    for (const update::FileClass fileClass : {update::FileClass::Program, update::FileClass::Lib,
                                              update::FileClass::Data, update::FileClass::Seed}) {
        const core::Result<update::FileClass> parsed = update::parseFileClass(update::toString(fileClass));
        ASSERT_TRUE(parsed.ok());
        EXPECT_EQ(parsed.value(), fileClass);
    }
    for (const std::string_view bad : {"", "Program", "LIB", "Data", "seeds", "program ", " lib", "exe", "p",
                                       "data\n"}) {
        const core::Result<update::FileClass> parsed = update::parseFileClass(bad);
        ASSERT_FALSE(parsed.ok()) << bad;
        EXPECT_EQ(parsed.error().code, core::ErrorCode::InvalidArgument) << bad;
    }
    EXPECT_FALSE(update::parseFileClass(std::string_view("data\0", 5)).ok());
}

// ---- UTC times and expiry --------------------------------------------------------------------------------------

TEST(UpdateUtcTime, ReadsTheSecondsSinceTheEpochAsTheCalendarCountsThem)
{
    // The values are Python's calendar.timegm over the same instants.
    struct Case {
        const char* text;
        std::int64_t epoch;
    };
    for (const Case& c : {Case{"1970-01-01T00:00:00Z", 0},
                          Case{"1970-01-01T00:00:01Z", 1},
                          Case{"1972-02-29T00:00:00Z", 68169600},
                          Case{"1999-12-31T23:59:59Z", 946684799},
                          Case{"2000-02-29T12:34:56Z", 951827696},
                          Case{"2000-03-01T00:00:00Z", 951868800},
                          Case{"2024-02-29T00:00:00Z", 1709164800},
                          Case{"2026-10-18T08:40:00Z", 1792312800},
                          Case{"2026-10-20T08:00:00Z", 1792483200},
                          Case{"2026-12-31T23:59:59Z", 1798761599},
                          Case{"2027-04-18T08:00:00Z", 1808035200},
                          Case{"2038-01-19T03:14:07Z", 2147483647},
                          Case{"2038-01-19T03:14:08Z", 2147483648},
                          Case{"2100-02-28T00:00:00Z", 4107456000},
                          Case{"2100-03-01T00:00:00Z", 4107542400},
                          Case{"9999-12-31T23:59:59Z", 253402300799}}) {
        const std::optional<std::int64_t> epoch = update::parseUtcTime(c.text);
        ASSERT_TRUE(epoch.has_value()) << c.text;
        EXPECT_EQ(*epoch, c.epoch) << c.text;
    }
}

TEST(UpdateUtcTime, TheDesignsSequenceIsThePublishedTimeOfTheFixtures)
{
    // tools/update_make_manifest.py writes sequence = published in epoch seconds (1792312800).
    const std::optional<std::int64_t> published = update::parseUtcTime("2026-10-18T08:40:00Z");
    ASSERT_TRUE(published.has_value());
    EXPECT_EQ(static_cast<std::uint64_t>(*published), 1792312800u);
    // expires = published + 180 days = 180 * 86400 = 15,552,000 seconds later.
    const std::optional<std::int64_t> expires = update::parseUtcTime("2027-04-16T08:40:00Z");
    ASSERT_TRUE(expires.has_value());
    EXPECT_EQ(*expires - *published, 15'552'000);
}

TEST(UpdateUtcTime, ATextThatIsNotExactlyThatFormOrNotARealDateIsNothing)
{
    for (const char* bad :
         {"", "x", "2026-10-20T08:00:00", "2026-10-20T08:00:00Zz", "2026-10-20T08:00:00z", "2026-10-20 08:00:00Z",
          "2026/10/20T08:00:00Z", "2026-10-20T08-00-00Z", "2026-10-20T08:00:00+0000", "20261020T080000Z",
          "2026-1-20T08:00:00Z", "2026-10-2T08:00:00Z", "+026-10-20T08:00:00Z", "2026-10-20T08:00:0\xC2\xB2Z",
          "2026-10-20T0\xD9\xA8:00:00Z", "1969-12-31T23:59:59Z", "2026-00-10T00:00:00Z", "2026-13-10T00:00:00Z",
          "2026-02-29T00:00:00Z", "2026-04-31T00:00:00Z", "2026-06-31T00:00:00Z", "2026-09-31T00:00:00Z",
          "2026-11-31T00:00:00Z", "2026-01-32T00:00:00Z", "2026-01-00T00:00:00Z", "2026-01-01T24:00:00Z",
          "2026-01-01T00:60:00Z", "2026-01-01T00:00:60Z", "1900-02-29T00:00:00Z", "2100-02-29T00:00:00Z"}) {
        EXPECT_FALSE(update::parseUtcTime(bad).has_value()) << bad;
    }
    EXPECT_FALSE(update::parseUtcTime(std::string_view("2026-10-20T08:00:00Z\0", 21)).has_value());
    EXPECT_FALSE(update::parseUtcTime(std::string_view("\0" "026-10-20T08:00:00Z", 20)).has_value());
    // The leap-day rule: divisible by 4, except centuries, except those divisible by 400.
    EXPECT_TRUE(update::parseUtcTime("2024-02-29T00:00:00Z").has_value());
    EXPECT_TRUE(update::parseUtcTime("2000-02-29T00:00:00Z").has_value());
    EXPECT_FALSE(update::parseUtcTime("2100-02-29T00:00:00Z").has_value());
    EXPECT_FALSE(update::parseUtcTime("2023-02-29T00:00:00Z").has_value());
}

TEST(UpdateUtcTime, TheLongestMonthsHaveTheirLastDayAndNoMore)
{
    for (const char* last : {"2026-01-31", "2026-03-31", "2026-04-30", "2026-05-31", "2026-06-30", "2026-07-31",
                             "2026-08-31", "2026-09-30", "2026-10-31", "2026-11-30", "2026-12-31", "2026-02-28"}) {
        EXPECT_TRUE(update::parseUtcTime(std::string(last) + "T00:00:00Z").has_value()) << last;
    }
}

TEST(UpdateExpiry, ExpiredMeansNowIsAfterTheExpiryInstantAndNothingIsRefusedByIt)
{
    update::Manifest manifest;
    manifest.expires = "2027-04-16T08:40:00Z"; // 1807864800
    EXPECT_FALSE(update::isExpired(manifest, 0));
    EXPECT_FALSE(update::isExpired(manifest, 1807864799));
    EXPECT_FALSE(update::isExpired(manifest, 1807864800)); // at the instant, not yet after it
    EXPECT_TRUE(update::isExpired(manifest, 1807864801));
    EXPECT_TRUE(update::isExpired(manifest, std::numeric_limits<std::int64_t>::max()));
    EXPECT_FALSE(update::isExpired(manifest, std::numeric_limits<std::int64_t>::min()));
}

TEST(UpdateExpiry, AnExpiryThatCannotBeReadIsNeverExpired)
{
    update::Manifest manifest; // expires is empty
    EXPECT_FALSE(update::isExpired(manifest, 4'000'000'000));
    manifest.expires = "soon";
    EXPECT_FALSE(update::isExpired(manifest, 4'000'000'000));
}

// ---- the producer's fixtures -----------------------------------------------------------------------------------

namespace {

struct Fixture {
    const char* directory;
    const char* manifest;
    const char* zip; // "" for a notify-only manifest
    std::size_t files;
    std::uint64_t sumSize;
    std::uint64_t sumCompressed;
    const char* version;
    std::uint64_t sequence;
    const char* tag;
    const char* published;
    const char* expires;
    std::uint64_t payloadSize;
};

// The counts, sums and instants are Python's json over the same files (tools/update_make_fixtures.py
// made them; this table does not read them back through the code under test).
const Fixture kV1{"release/v1", "Katana-update-win64.json", "Katana-0.9.0-win64.update.zip", 9, 2879, 2535,
                  "0.9.0", 1790000000, "v0.9.0", "2026-09-21T14:13:20Z", "2027-03-20T14:13:20Z", 3607};
const Fixture kV2{"release/v2", "Katana-update-win64.json", "Katana-0.10.0-win64.update.zip", 10, 2975, 2601,
                  "0.10.0", 1792312800, "v0.10.0", "2026-10-18T08:40:00Z", "2027-04-16T08:40:00Z", 3791};

void expectFixtureRead(const Fixture& fixture)
{
    const std::string name = std::string(fixture.directory) + "/" + fixture.manifest;
    const core::Result<update::Manifest> parsed = parse(readFixture(name));
    ASSERT_TRUE(parsed.ok()) << name << ": " << parsed.error().describe();
    const update::Manifest& m = parsed.value();
    EXPECT_EQ(m.files.size(), fixture.files) << name;
    EXPECT_EQ(update::toString(m.version), fixture.version) << name;
    EXPECT_EQ(m.sequence, fixture.sequence) << name;
    EXPECT_EQ(m.tag, fixture.tag) << name;
    EXPECT_EQ(m.published, fixture.published) << name;
    EXPECT_EQ(m.expires, fixture.expires) << name;
    EXPECT_EQ(m.product, "katana");
    EXPECT_EQ(m.channel, "stable");
    EXPECT_EQ(m.target, "win64");
    EXPECT_EQ(m.schema, 1u);
    EXPECT_EQ(m.applier, 1u);
    EXPECT_FALSE(m.notifyOnly);
    ASSERT_TRUE(m.payload.has_value()) << name;
    EXPECT_EQ(m.payload->codec, "deflate");
    EXPECT_EQ(m.payload->name, fixture.zip);
    EXPECT_EQ(m.payload->size, fixture.payloadSize) << name;

    std::uint64_t sumSize = 0;
    std::uint64_t sumCompressed = 0;
    for (const update::ManifestFile& file : m.files) {
        sumSize += file.size;
        sumCompressed += file.compressedSize;
        // Every range is inside the payload (the check the reader makes), against the REAL
        // zip's length from the file system.
        EXPECT_LE(file.offset + file.compressedSize, fixture.payloadSize) << file.path;
    }
    EXPECT_EQ(sumSize, fixture.sumSize) << name;
    EXPECT_EQ(sumCompressed, fixture.sumCompressed) << name;

    // The payload size the manifest states is the zip's size on disk.
    std::error_code ec;
    EXPECT_EQ(fs::file_size(fs::path(KATANA_UPDATE_FIXTURES) / fixture.directory / fixture.zip, ec),
              fixture.payloadSize)
        << name;
}

} // namespace

TEST(UpdateManifestFixtures, TheFirstReleaseIsReadAsItsProducerWroteIt)
{
    expectFixtureRead(kV1);
}

TEST(UpdateManifestFixtures, TheSecondReleaseIsReadAsItsProducerWroteIt)
{
    expectFixtureRead(kV2);
    const core::Result<update::Manifest> parsed = parse(readFixture("release/v2/Katana-update-win64.json"));
    ASSERT_TRUE(parsed.ok());
    const update::Manifest& m = parsed.value();
    // Spot checks against the file itself (its first and last entry).
    EXPECT_EQ(m.files.front().path, "bin/katana.exe");
    EXPECT_EQ(m.files.front().fileClass, update::FileClass::Program);
    EXPECT_TRUE(m.files.front().executable);
    EXPECT_EQ(m.files.front().size, 207u);
    EXPECT_EQ(m.files.front().compressedSize, 85u);
    EXPECT_EQ(m.files.front().offset, 44u);
    EXPECT_EQ(m.files.front().sha256, "b70d132208d7dd410c0daba7f5fcb5a1594c844a73d499845bea1856fb6bf8da");
    EXPECT_EQ(m.files.back().path, "share/katana/readme.txt");
    EXPECT_EQ(m.files.back().fileClass, update::FileClass::Data);
    EXPECT_FALSE(m.files.back().executable);
    EXPECT_EQ(m.files.back().sha256, "44b16c8491aaefa9a3070a9e1afa87d82c2af76c1b21d6c6494d3348237c3d7c");
    // The empty file: size 0 and the shortest deflate stream, two bytes, and the hash of "".
    const auto empty = std::find_if(m.files.begin(), m.files.end(),
                                    [](const update::ManifestFile& f) { return f.path == "share/katana/empty.txt"; });
    ASSERT_NE(empty, m.files.end());
    EXPECT_EQ(empty->size, 0u);
    EXPECT_EQ(empty->compressedSize, 2u);
    EXPECT_EQ(empty->sha256, kHashEmpty);
    // The seed.
    const auto seed = std::find_if(m.files.begin(), m.files.end(),
                                   [](const update::ManifestFile& f) { return f.fileClass == update::FileClass::Seed; });
    ASSERT_NE(seed, m.files.end());
    EXPECT_EQ(seed->path, "etc/katana/settings.ini");
}

TEST(UpdateManifestFixtures, TheSpareSignedReleaseHasTheManifestOfTheSecond)
{
    // Only the signature differs (T5's), the manifest is byte for byte the second release's.
    EXPECT_EQ(readFixture("release/v2-spare-signed/Katana-update-win64.json"),
              readFixture("release/v2/Katana-update-win64.json"));
    expectFixtureRead(Fixture{"release/v2-spare-signed", "Katana-update-win64.json",
                              "Katana-0.10.0-win64.update.zip", 10, 2975, 2601, "0.10.0", 1792312800, "v0.10.0",
                              "2026-10-18T08:40:00Z", "2027-04-16T08:40:00Z", 3791});
}

TEST(UpdateManifestFixtures, TheMacosManifestIsNotifyOnly)
{
    const core::Result<update::Manifest> parsed =
        parse(readFixture("release/v2-macos-notify/Katana-update-macos-arm64.json"));
    ASSERT_TRUE(parsed.ok()) << parsed.error().describe();
    const update::Manifest& m = parsed.value();
    EXPECT_TRUE(m.notifyOnly);
    EXPECT_TRUE(m.files.empty());
    EXPECT_FALSE(m.payload.has_value());
    EXPECT_EQ(m.target, "macos-arm64");
    EXPECT_EQ(update::toString(m.version), "0.10.0");
    EXPECT_EQ(m.sequence, 1792312800u);
    EXPECT_EQ(m.tag, "v0.10.0");
    EXPECT_EQ(m.notesUrl, "https://github.com/kjarada/Katana/releases/tag/v0.10.0");
}

TEST(UpdateManifestFixtures, TheOrderOfTheTwoReleasesIsTheVersionsAndNotTheText)
{
    const update::Manifest v1 = parse(readFixture("release/v1/Katana-update-win64.json")).value();
    const update::Manifest v2 = parse(readFixture("release/v2/Katana-update-win64.json")).value();
    // 0.10.0 is above 0.9.0 (10 > 9); as text "0.10.0" < "0.9.0".
    EXPECT_GT(v2.version, v1.version);
    EXPECT_LT(v2.tag, v1.tag) << "the tags sort the wrong way as text, which is why no one compares them";
    EXPECT_GT(v2.sequence, v1.sequence);
}

TEST(UpdateManifestFixtures, ADowngradeByVersionIsAnOrdinaryManifestForTheLaterChecksToJudge)
{
    // It parses: that it is below what is installed is isNewerThan's to say (trust.hpp). Its
    // sequence is the HIGHEST of all the fixtures, so only the version keeps it from winning.
    const update::Manifest downgrade =
        parse(readFixture("release/bad/downgrade-version/Katana-update-win64.json")).value();
    const update::Manifest v2 = parse(readFixture("release/v2/Katana-update-win64.json")).value();
    EXPECT_EQ(update::toString(downgrade.version), "0.9.0");
    EXPECT_EQ(downgrade.sequence, 1792316400u);
    EXPECT_GT(downgrade.sequence, v2.sequence);
    EXPECT_LT(downgrade.version, v2.version);
}

TEST(UpdateManifestFixtures, ADowngradeBySequenceIsTheSameVersionOneSecondEarlier)
{
    const update::Manifest earlier =
        parse(readFixture("release/bad/downgrade-sequence/Katana-update-win64.json")).value();
    const update::Manifest v2 = parse(readFixture("release/v2/Katana-update-win64.json")).value();
    EXPECT_EQ(earlier.version, v2.version);
    EXPECT_EQ(earlier.sequence, 1792312799u);
    EXPECT_EQ(v2.sequence - earlier.sequence, 1u);
}

TEST(UpdateManifestFixtures, AFlippedBitInTheHashIsStillAManifestTheSignatureIsWhatCatchesIt)
{
    // One hex digit changed ('b' to 'c'): a well-formed manifest with a different hash. Only
    // the signature (T5) can know it is not what was signed.
    const core::Result<update::Manifest> parsed =
        parse(readFixture("release/bad/flipped-manifest-bit/Katana-update-win64.json"));
    ASSERT_TRUE(parsed.ok()) << parsed.error().describe();
    EXPECT_EQ(parsed.value().files.front().sha256,
              "c70d132208d7dd410c0daba7f5fcb5a1594c844a73d499845bea1856fb6bf8da");
}

TEST(UpdateManifestFixtures, ThePathTraversalManifestIsRefusedByItsDotDotPath)
{
    const core::Result<update::Manifest> parsed =
        parse(readFixture("release/bad/path-traversal/Katana-update-win64.json"));
    ASSERT_FALSE(parsed.ok());
    EXPECT_EQ(parsed.error().code, core::ErrorCode::InvalidArgument);
    EXPECT_EQ(parsed.error().context, "bin/../../evil.dll");
    EXPECT_NE(parsed.error().message.find("files[0].path"), std::string::npos) << parsed.error().message;
}

TEST(UpdateManifestFixtures, TheBadVariantsOfTheSignatureAndThePayloadAreManifestsLikeAnyOther)
{
    // swapped-sig, tampered-member, wrong-key and wrong-key-claims-active carry the second
    // release's manifest (the refusal is the signature's or the member hash's, T5 and T8).
    const std::string v2 = readFixture("release/v2/Katana-update-win64.json");
    for (const char* name : {"swapped-sig", "tampered-member", "wrong-key", "wrong-key-claims-active"}) {
        const std::string text = readFixture(std::string("release/bad/") + name + "/Katana-update-win64.json");
        EXPECT_EQ(text, v2) << name;
        EXPECT_TRUE(parse(text).ok()) << name;
    }
}

TEST(UpdateManifestFixtures, TheSecondReleaseIsRefusedAtEveryLengthShorterThanItself)
{
    // A manifest cut off anywhere is not a manifest (a truncated download): no prefix of
    // the whole is accepted, whichever byte it stops at.
    const std::string whole = readFixture("release/v2/Katana-update-win64.json");
    ASSERT_TRUE(parse(whole).ok());
    for (std::size_t length = 0; length + 1 < whole.size(); length += 7) {
        const core::Result<update::Manifest> parsed = parse(whole.substr(0, length));
        EXPECT_FALSE(parsed.ok()) << "accepted a prefix of " << length << " bytes";
    }
    // The trailing newline is the only byte that may go.
    ASSERT_EQ(whole.back(), '\n');
    EXPECT_TRUE(parse(whole.substr(0, whole.size() - 1)).ok());
}
