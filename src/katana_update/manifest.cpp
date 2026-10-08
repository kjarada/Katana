// The manifest reader (include/katana/update/manifest.hpp). nlohmann_json is PRIVATE to this
// library: no third-party type is in a public header (CLAUDE.md, section 11).
//
// This runs on bytes whose signature has ALREADY verified (trust.hpp), so it is not the trust
// gate; it is what makes a signed manifest safe to act on. A signature says who wrote a
// value, not that the value is sane: a number that wraps when two are added, a path that
// climbs out of the install, an offset past the end of the zip are all things a correctly
// signed manifest could carry by mistake (a producer bug) as readily as by malice, and the
// rest of the engine takes the Manifest this returns at its word.
//
// Strict where a lax reader would be exploited, lenient where a newer producer may add:
//   - exact types ("5.0", "1e3", a boolean for a number, a number for a string are refused);
//   - a duplicate member anywhere, or nesting deeper than kMaxDepth, is refused (a reader
//     that took the last of two "sha256" members would disagree with one that took the first);
//   - members this reader does not know are ignored, but still checked for those two.

#include "katana/update/manifest.hpp"

#include <algorithm>
#include <array>
#include <cstddef>
#include <limits>
#include <nlohmann/json.hpp>
#include <optional>
#include <string>
#include <unordered_set>
#include <utility>
#include <vector>

#include "katana/update/feed.hpp"
#include "katana/update/paths.hpp"

namespace katana::update {

namespace {

using Json = nlohmann::json;

constexpr std::uint32_t kSchema = 1;
// The sample has 3 levels (object, files, one file). Room for a newer producer's nested
// members, and far below anything that could exhaust the stack.
constexpr int kMaxDepth = 16;
constexpr std::size_t kMaxTokenBytes = 64;
constexpr std::size_t kMaxPayloadNameBytes = 255;
constexpr std::size_t kMaxUrlBytes = 2048;
// The zip holds every file deflated alone, and the files add up to at most kMaxInstallBytes,
// so the zip is that and a little (maxCompressedBytes below, and its headers): twice the
// install cap is room for all of it, and no signed number can make the payload, or a range in
// it, larger than that. The offsets and lengths go to APIs with signed 64-bit offsets (HTTP
// range arithmetic, curl_off_t, std::streamoff) and stay far below them.
constexpr std::uint64_t kMaxPayloadBytes = 2 * kMaxInstallBytes;
// A deflate stream is never empty (manifest.hpp): the shortest, an empty final block, is the
// two bytes 03 00.
constexpr std::uint64_t kMinCompressedBytes = 2;

// The most a deflate stream of `size` bytes can be: zlib's worst case is the input and five
// bytes for each stored block (about 0.03%) and 13 bytes, so one part in a thousand and 64
// bytes leave room for any encoder. `size` is at most kMaxInstallBytes: nothing overflows.
constexpr std::uint64_t maxCompressedBytes(std::uint64_t size)
{
    return size + size / 1000 + 64;
}

core::Error malformed(std::string what, std::string_view member)
{
    return core::makeError(core::ErrorCode::ParseFailure, std::move(what), std::string(member));
}

core::Error invalid(std::string what, std::string_view member)
{
    return core::makeError(core::ErrorCode::InvalidArgument, std::move(what), std::string(member));
}

std::string memberName(std::string_view where, std::string_view key)
{
    return where.empty() ? std::string(key) : std::string(where) + "." + std::string(key);
}

// A fragment of a signed string for a message: bounded and with no byte that is not
// printable ASCII, so a NUL or a newline in a value cannot reshape a log line.
std::string excerpt(std::string_view text)
{
    constexpr std::size_t kShown = 64;
    std::string out;
    const std::size_t shown = std::min(text.size(), kShown);
    for (std::size_t i = 0; i < shown; ++i) {
        const auto c = static_cast<unsigned char>(text[i]);
        out.push_back(c >= 0x20 && c <= 0x7E ? static_cast<char>(c) : '?');
    }
    if (text.size() > shown) {
        out += "...";
    }
    return out;
}

// ---- the JSON, with the two refusals nlohmann does not make by itself -------------------

struct Refusal {
    core::Error error;
};

core::Result<Json> parseJson(std::string_view text)
{
    // One set of the keys seen so far for each object that is open.
    std::vector<std::unordered_set<std::string>> keysSeen;
    int depth = 0;
    const Json::parser_callback_t watch = [&](int, Json::parse_event_t event, Json& parsed) {
        switch (event) {
        case Json::parse_event_t::object_start:
        case Json::parse_event_t::array_start:
            if (++depth > kMaxDepth) {
                throw Refusal{malformed("the JSON is nested more than " +
                                            std::to_string(kMaxDepth) + " levels deep",
                                        "")};
            }
            if (event == Json::parse_event_t::object_start) {
                keysSeen.emplace_back();
            }
            break;
        case Json::parse_event_t::object_end:
            keysSeen.pop_back();
            --depth;
            break;
        case Json::parse_event_t::array_end:
            --depth;
            break;
        case Json::parse_event_t::key:
            if (!keysSeen.back().insert(parsed.get<std::string>()).second) {
                throw Refusal{malformed("a member is written twice in one object",
                                        excerpt(parsed.get<std::string>()))};
            }
            break;
        case Json::parse_event_t::value:
            break;
        }
        return true;
    };

    try {
        return Json::parse(text, watch, /*allow_exceptions=*/true, /*ignore_comments=*/false);
    } catch (const Refusal& refusal) {
        return refusal.error;
    } catch (const Json::parse_error& error) {
        // Not error.what(): nlohmann ends that text with "last read: '<the token>'", which is
        // the whole string or number it was in (megabytes of a manifest) and holds its raw
        // bytes, control characters and invalid UTF-8 among them. The place is enough.
        return malformed("not well-formed JSON: the reader stopped at byte " +
                             std::to_string(error.byte),
                         "");
    } catch (const Json::exception& error) {
        return malformed("the JSON could not be read (reader error " +
                             std::to_string(error.id) + ")",
                         "");
    } catch (const std::exception&) {
        return malformed("the JSON could not be read", "");
    }
}

// ---- reading members --------------------------------------------------------------------

// One JSON object and the name it goes by in messages ("" for the manifest itself, then
// "payload" or "files[3]"). Each reader gives ParseFailure for a missing or mistyped member,
// the contract's word for "this is not the shape of a manifest"; rules about VALUES are
// InvalidArgument and are the caller's.
class Members {
  public:
    Members(const Json& object, std::string where) : object_(object), where_(std::move(where)) {}

    [[nodiscard]] std::string name(std::string_view key) const { return memberName(where_, key); }

    // nullptr when the member is absent.
    [[nodiscard]] const Json* find(std::string_view key) const
    {
        const auto it = object_.find(std::string(key));
        return it == object_.end() ? nullptr : &*it;
    }

    [[nodiscard]] core::Result<std::string> string(std::string_view key) const
    {
        const Json* value = find(key);
        if (value == nullptr) {
            return missing(key);
        }
        if (!value->is_string()) {
            return malformed("the member must be a string", name(key));
        }
        return value->get<std::string>();
    }

    // An unsigned integer written as one: "5.0", "1e3" and "-0" are floats or signed to the
    // parser and are refused, as is a number past 2^64 - 1 (nlohmann reads it as a float).
    [[nodiscard]] core::Result<std::uint64_t> unsignedInteger(std::string_view key) const
    {
        const Json* value = find(key);
        if (value == nullptr) {
            return missing(key);
        }
        if (!value->is_number_unsigned()) {
            return malformed("the member must be a whole number from 0 to 18446744073709551615",
                             name(key));
        }
        return value->get<std::uint64_t>();
    }

    // Absent is false; present must be a boolean.
    [[nodiscard]] core::Result<bool> optionalBoolean(std::string_view key) const
    {
        const Json* value = find(key);
        if (value == nullptr) {
            return false;
        }
        if (!value->is_boolean()) {
            return malformed("the member must be true or false", name(key));
        }
        return value->get<bool>();
    }

    [[nodiscard]] core::Result<const Json*> array(std::string_view key) const
    {
        const Json* value = find(key);
        if (value == nullptr) {
            return missing(key);
        }
        if (!value->is_array()) {
            return malformed("the member must be a list", name(key));
        }
        return value;
    }

    [[nodiscard]] core::Error missing(std::string_view key) const
    {
        return malformed("the member is missing", name(key));
    }

  private:
    const Json& object_;
    std::string where_;
};

// ---- value rules --------------------------------------------------------------------------

bool isAlphanumeric(char c)
{
    return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9');
}

bool isNameCharacter(char c)
{
    return isAlphanumeric(c) || c == '.' || c == '_' || c == '-';
}

// product, channel, target, tag and the payload's name end up in a URL, a file name or a
// message; letters, digits and . _ - are all they need, and nothing else is let through. The
// first is a letter or a digit: "." and ".." are dot-segments of the URL
// (releases/download/<tag>/<name>) and a leading dot is a hidden file.
core::Status checkName(std::string_view value, std::size_t maxBytes, std::string_view member)
{
    if (value.empty() || value.size() > maxBytes) {
        return invalid("the value must be 1 to " + std::to_string(maxBytes) + " characters",
                       member);
    }
    if (!isAlphanumeric(value.front())) {
        return invalid("the value must start with a letter or a digit", member);
    }
    for (const char c : value) {
        if (!isNameCharacter(c)) {
            return invalid("the value may hold only letters, digits and . _ -", member);
        }
    }
    return {};
}

core::Status checkNotesUrl(std::string_view value, std::string_view member)
{
    constexpr std::string_view kScheme = "https://";
    if (!value.starts_with(kScheme) || value.size() == kScheme.size() ||
        value.size() > kMaxUrlBytes) {
        return invalid("the value must be an https:// address of at most 2048 characters",
                       member);
    }
    for (const char c : value) {
        const auto byte = static_cast<unsigned char>(c);
        if (byte <= 0x20 || byte >= 0x7F) {
            return invalid("the address may hold only printable ASCII and no space", member);
        }
    }
    return {};
}

bool isLowerHex64(std::string_view text)
{
    if (text.size() != 64) {
        return false;
    }
    for (const char c : text) {
        if (!((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f'))) {
            return false;
        }
    }
    return true;
}

// The days from 1970-01-01 to a civil date (H. Hinnant's days_from_civil): the proleptic
// Gregorian calendar, with March as the first month of the shifted year so that a leap day
// is the last day of it.
std::int64_t daysFromCivil(std::int64_t year, unsigned month, unsigned day)
{
    year -= month <= 2 ? 1 : 0;
    const std::int64_t era = (year >= 0 ? year : year - 399) / 400;
    const auto yearOfEra = static_cast<unsigned>(year - era * 400);
    const unsigned dayOfYear = (153 * (month > 2 ? month - 3 : month + 9) + 2) / 5 + day - 1;
    const unsigned dayOfEra = yearOfEra * 365 + yearOfEra / 4 - yearOfEra / 100 + dayOfYear;
    return era * 146097 + static_cast<std::int64_t>(dayOfEra) - 719468;
}

bool isLeapYear(unsigned year)
{
    return (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
}

} // namespace

// ---- file classes -----------------------------------------------------------------------

std::string_view toString(FileClass fileClass)
{
    switch (fileClass) {
    case FileClass::Program:
        return "program";
    case FileClass::Lib:
        return "lib";
    case FileClass::Data:
        return "data";
    case FileClass::Seed:
        return "seed";
    }
    return "lib"; // not reached: every enumerator is above
}

core::Result<FileClass> parseFileClass(std::string_view text)
{
    for (const FileClass fileClass :
         {FileClass::Program, FileClass::Lib, FileClass::Data, FileClass::Seed}) {
        if (text == toString(fileClass)) {
            return fileClass;
        }
    }
    return core::makeError(core::ErrorCode::InvalidArgument,
                           "not a file class (program, lib, data or seed)", excerpt(text));
}

// ---- UTC times -----------------------------------------------------------------------------

std::optional<std::int64_t> parseUtcTime(std::string_view text)
{
    // YYYY-MM-DDTHH:MM:SSZ: 20 bytes, the separators in fixed places, digits everywhere else.
    constexpr std::string_view kShape = "dddd-dd-ddTdd:dd:ddZ";
    if (text.size() != kShape.size()) {
        return std::nullopt;
    }
    for (std::size_t i = 0; i < kShape.size(); ++i) {
        if (kShape[i] == 'd' ? !(text[i] >= '0' && text[i] <= '9') : text[i] != kShape[i]) {
            return std::nullopt;
        }
    }
    const auto number = [&](std::size_t from, std::size_t length) {
        unsigned value = 0;
        for (std::size_t i = from; i < from + length; ++i) {
            value = value * 10 + static_cast<unsigned>(text[i] - '0');
        }
        return value;
    };
    const unsigned year = number(0, 4);
    const unsigned month = number(5, 2);
    const unsigned day = number(8, 2);
    const unsigned hour = number(11, 2);
    const unsigned minute = number(14, 2);
    const unsigned second = number(17, 2);

    if (year < 1970 || month < 1 || month > 12 || hour > 23 || minute > 59 || second > 59) {
        return std::nullopt;
    }
    constexpr std::array<unsigned, 12> kDaysInMonth{31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31};
    const unsigned daysInMonth = kDaysInMonth[month - 1] + (month == 2 && isLeapYear(year) ? 1 : 0);
    if (day < 1 || day > daysInMonth) {
        return std::nullopt;
    }
    return daysFromCivil(year, month, day) * 86400 + static_cast<std::int64_t>(hour) * 3600 +
           static_cast<std::int64_t>(minute) * 60 + static_cast<std::int64_t>(second);
}

bool isExpired(const Manifest& manifest, std::int64_t nowEpoch)
{
    // Advisory (manifest.hpp): a time that cannot be read is not "expired", and nothing
    // that calls this refuses an update because of the answer.
    const std::optional<std::int64_t> expires = parseUtcTime(manifest.expires);
    return expires && nowEpoch > *expires;
}

// ---- the manifest -----------------------------------------------------------------------------

namespace {

core::Result<std::string> readTime(const Members& members, std::string_view key)
{
    core::Result<std::string> text = members.string(key);
    if (!text) {
        return text;
    }
    if (!parseUtcTime(text.value())) {
        return invalid("the value must be a real UTC time written YYYY-MM-DDTHH:MM:SSZ, from "
                       "1970",
                       members.name(key));
    }
    return text;
}

core::Result<Payload> readPayload(const Json& json)
{
    if (!json.is_object()) {
        return malformed("the member must be an object", "payload");
    }
    const Members members(json, "payload");

    Payload payload;
    core::Result<std::string> codec = members.string("codec");
    if (!codec) {
        return codec.error();
    }
    // The only codec the apply code can inflate (the zip's members are raw deflate).
    if (codec.value() != "deflate") {
        return invalid("the codec must be \"deflate\"", members.name("codec"));
    }
    payload.codec = std::move(codec.value());

    core::Result<std::string> name = members.string("name");
    if (!name) {
        return name.error();
    }
    if (core::Status status = checkName(name.value(), kMaxPayloadNameBytes, members.name("name"));
        !status) {
        return status.error();
    }
    payload.name = std::move(name.value());

    core::Result<std::uint64_t> size = members.unsignedInteger("size");
    if (!size) {
        return size.error();
    }
    if (size.value() == 0 || size.value() > kMaxPayloadBytes) {
        return invalid("the size must be from 1 to 4294967296 (4 GiB)", members.name("size"));
    }
    payload.size = size.value();
    return payload;
}

core::Result<ManifestFile> readFile(const Json& json, std::size_t index,
                                    const std::optional<Payload>& payload)
{
    const std::string where = "files[" + std::to_string(index) + "]";
    if (!json.is_object()) {
        return malformed("the entry must be an object", where);
    }
    const Members members(json, where);
    ManifestFile file;

    core::Result<std::string> path = members.string("path");
    if (!path) {
        return path.error();
    }
    if (core::Status status = checkManifestPath(path.value()); !status) {
        const core::Error& rule = status.error();
        return core::makeError(core::ErrorCode::InvalidArgument,
                               members.name("path") + ": " + rule.message, rule.context);
    }
    file.path = std::move(path.value());

    core::Result<std::string> className = members.string("class");
    if (!className) {
        return className.error();
    }
    core::Result<FileClass> fileClass = parseFileClass(className.value());
    if (!fileClass) {
        return invalid(fileClass.error().message, members.name("class"));
    }
    file.fileClass = fileClass.value();

    core::Result<bool> executable = members.optionalBoolean("exec");
    if (!executable) {
        return executable.error();
    }
    file.executable = executable.value();

    core::Result<std::uint64_t> size = members.unsignedInteger("size");
    core::Result<std::uint64_t> compressed = members.unsignedInteger("csize");
    core::Result<std::uint64_t> offset = members.unsignedInteger("offset");
    if (!size) {
        return size.error();
    }
    if (!compressed) {
        return compressed.error();
    }
    if (!offset) {
        return offset.error();
    }
    if (size.value() > kMaxInstallBytes) {
        return invalid("a file larger than 2 GiB (2147483648 bytes)", members.name("size"));
    }
    if (compressed.value() < kMinCompressedBytes) {
        return invalid("a deflate stream is never shorter than two bytes", members.name("csize"));
    }
    // offset + csize <= payload.size, written so that nothing is added: with the payload at
    // most 2^63 - 1, "offset <= payload and csize <= payload - offset" cannot wrap, where
    // offset + csize can (offset 2^64 - 1 and csize 3 add up to 2).
    const std::uint64_t payloadSize = payload ? payload->size : 0;
    if (offset.value() > payloadSize || compressed.value() > payloadSize - offset.value()) {
        return invalid("the file's bytes are not inside the payload", members.name("offset"));
    }
    if (compressed.value() > maxCompressedBytes(size.value())) {
        return invalid("a deflate stream is not longer than its file by more than deflate's "
                       "worst case (one part in a thousand and 64 bytes)",
                       members.name("csize"));
    }
    file.size = size.value();
    file.compressedSize = compressed.value();
    file.offset = offset.value();

    core::Result<std::string> sha256 = members.string("sha256");
    if (!sha256) {
        return sha256.error();
    }
    if (!isLowerHex64(sha256.value())) {
        return invalid("the hash must be 64 lower-case hexadecimal digits", members.name("sha256"));
    }
    file.sha256 = std::move(sha256.value());
    return file;
}

core::Status checkFileList(const std::vector<ManifestFile>& files)
{
    for (std::size_t i = 1; i < files.size(); ++i) {
        const int order = files[i - 1].path.compare(files[i].path);
        if (order == 0) {
            return invalid("the same path is listed twice", "files[" + std::to_string(i) + "].path");
        }
        if (order > 0) {
            return invalid("files are not sorted by path", "files[" + std::to_string(i) + "].path");
        }
    }
    std::vector<std::string> paths;
    std::vector<std::uint64_t> sizes;
    paths.reserve(files.size());
    sizes.reserve(files.size());
    for (const ManifestFile& file : files) {
        paths.push_back(file.path);
        sizes.push_back(file.size);
    }
    return checkManifestPaths(paths, sizes);
}

} // namespace

core::Result<Manifest> parseManifest(std::string_view json)
{
    if (json.size() > kMaxManifestBytes) {
        return malformed("the manifest is larger than " + std::to_string(kMaxManifestBytes) +
                             " bytes",
                         std::to_string(json.size()) + " bytes");
    }
    // nlohmann would skip a byte-order mark; the contract says UTF-8 without one, and the
    // signed bytes are what is parsed.
    if (json.starts_with("\xEF\xBB\xBF")) {
        return malformed("the manifest starts with a byte-order mark", "");
    }
    // nlohmann's lexer takes a NUL byte for the end of its input, so "{...}" + NUL + anything
    // would be read as the object alone while every other reader (the Python verifier, a
    // C string) sees something else. JSON has no raw NUL anywhere (inside a string it is
    // written \u0000), so none is legal and none is let through.
    if (json.find('\0') != std::string_view::npos) {
        return malformed("the manifest holds a NUL byte", "");
    }

    core::Result<Json> parsed = parseJson(json);
    if (!parsed) {
        return parsed.error();
    }
    const Json& root = parsed.value();
    if (!root.is_object()) {
        return malformed("the manifest must be a JSON object", "");
    }
    const Members members(root, "");
    Manifest manifest;

    // The schema first: a manifest of another schema may differ in every member below.
    core::Result<std::uint64_t> schema = members.unsignedInteger("schema");
    if (!schema) {
        return schema.error();
    }
    if (schema.value() != kSchema) {
        return invalid("this program reads manifest schema 1 only", "schema");
    }
    manifest.schema = kSchema;

    core::Result<std::uint64_t> applier = members.unsignedInteger("applier");
    if (!applier) {
        return applier.error();
    }
    // The minimum helper protocol. A value above what this build speaks is NOT refused here:
    // it means the staged helper must be used (stage.hpp), which is the apply code's decision.
    if (applier.value() < 1 || applier.value() > std::numeric_limits<std::uint32_t>::max()) {
        return invalid("the applier protocol must be from 1 to 4294967295", "applier");
    }
    manifest.applier = static_cast<std::uint32_t>(applier.value());

    // product, target and channel are checked here for their form; that they are THIS
    // install's product, target and the stable channel is verifyManifest's (trust.hpp).
    struct NamedText {
        const char* key;
        std::string* into;
    };
    for (const NamedText& item :
         {NamedText{"product", &manifest.product}, NamedText{"channel", &manifest.channel},
          NamedText{"target", &manifest.target}, NamedText{"tag", &manifest.tag}}) {
        core::Result<std::string> text = members.string(item.key);
        if (!text) {
            return text.error();
        }
        if (core::Status status = checkName(text.value(), kMaxTokenBytes, item.key); !status) {
            return status.error();
        }
        *item.into = std::move(text.value());
    }

    core::Result<std::string> version = members.string("version");
    if (!version) {
        return version.error();
    }
    core::Result<Version> parsedVersion = parseVersion(version.value());
    if (!parsedVersion) {
        return invalid(parsedVersion.error().message, "version");
    }
    manifest.version = parsedVersion.value();

    core::Result<std::uint64_t> sequence = members.unsignedInteger("sequence");
    if (!sequence) {
        return sequence.error();
    }
    if (sequence.value() == 0) {
        return invalid("the sequence must be 1 or more (0 means none, in an install record)",
                       "sequence");
    }
    manifest.sequence = sequence.value();

    core::Result<std::string> published = readTime(members, "published");
    if (!published) {
        return published.error();
    }
    manifest.published = std::move(published.value());
    // Read for its form only; being in the past is not a refusal (isExpired).
    core::Result<std::string> expires = readTime(members, "expires");
    if (!expires) {
        return expires.error();
    }
    manifest.expires = std::move(expires.value());

    core::Result<std::string> notesUrl = members.string("notes_url");
    if (!notesUrl) {
        return notesUrl.error();
    }
    if (core::Status status = checkNotesUrl(notesUrl.value(), "notes_url"); !status) {
        return status.error();
    }
    manifest.notesUrl = std::move(notesUrl.value());

    core::Result<bool> notifyOnly = members.optionalBoolean("notify_only");
    if (!notifyOnly) {
        return notifyOnly.error();
    }
    manifest.notifyOnly = notifyOnly.value();

    const Json* payloadJson = members.find("payload");
    if (manifest.notifyOnly) {
        if (payloadJson != nullptr) {
            return invalid("a notify-only manifest has no payload", "payload");
        }
    } else {
        if (payloadJson == nullptr) {
            return members.missing("payload");
        }
        core::Result<Payload> payload = readPayload(*payloadJson);
        if (!payload) {
            return payload.error();
        }
        manifest.payload = std::move(payload.value());
    }

    core::Result<const Json*> list = members.array("files");
    if (!list) {
        return list.error();
    }
    const Json& entries = *list.value();
    if (manifest.notifyOnly && !entries.empty()) {
        return invalid("a notify-only manifest lists no files", "files");
    }
    // An installable manifest with no files would, applied, remove everything the last
    // version installed and put nothing back.
    if (!manifest.notifyOnly && entries.empty()) {
        return invalid("a manifest that installs lists at least one file", "files");
    }
    if (entries.size() > kMaxFiles) {
        return invalid("more than " + std::to_string(kMaxFiles) + " files", "files");
    }
    manifest.files.reserve(entries.size());
    for (std::size_t i = 0; i < entries.size(); ++i) {
        core::Result<ManifestFile> file = readFile(entries[i], i, manifest.payload);
        if (!file) {
            return file.error();
        }
        manifest.files.push_back(std::move(file.value()));
    }
    if (core::Status status = checkFileList(manifest.files); !status) {
        return status.error();
    }
    return manifest;
}

} // namespace katana::update
