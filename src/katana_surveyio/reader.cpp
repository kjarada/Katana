#include "katana/surveyio/reader.hpp"

#include <cstdint>
#include <exception>
#include <fstream>
#include <memory>
#include <new>
#include <system_error>
#include <utility>

#include "katana/surveyio/delimited_points.hpp"

namespace katana::surveyio {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

char lowerAscii(char c)
{
    return (c >= 'A' && c <= 'Z') ? static_cast<char>(c - 'A' + 'a') : c;
}

bool equalsIgnoringAsciiCase(std::string_view a, std::string_view b)
{
    if (a.size() != b.size()) {
        return false;
    }
    for (std::size_t i = 0; i < a.size(); ++i) {
        if (lowerAscii(a[i]) != lowerAscii(b[i])) {
            return false;
        }
    }
    return true;
}

// The sibling rule, in one place: a reader may ask for a NAME and nothing
// else. Anything survey::sourceFileName() would change - a directory part,
// "..", a drive letter, an alternate data stream - came from the file, and a
// file must not choose what this program reads (data_model.hpp explains).
katana::core::Status checkSiblingName(std::string_view name)
{
    if (name.empty() || katana::survey::sourceFileName(name) != name) {
        return makeError(ErrorCode::InvalidArgument,
                         "refused to read '" + std::string(name) +
                             "': a file named inside a survey file is looked for by its name "
                             "alone, beside the file, and this is not a plain file name");
    }
    return {};
}

std::filesystem::path pathFromUtf8(std::string_view name)
{
    // A name inside a field file is UTF-8 (or ASCII, which is UTF-8). Building
    // the path from char would read it in the Windows ANSI code page instead.
    const std::u8string utf8(reinterpret_cast<const char8_t*>(name.data()), name.size());
    return std::filesystem::path(utf8);
}

std::string nameOf(const std::filesystem::path& path)
{
    const std::u8string utf8 = path.filename().u8string();
    return std::string(reinterpret_cast<const char*>(utf8.data()), utf8.size());
}

} // namespace

katana::survey::ReportInput reportInputFor(const ReadResult& read, const std::string& fileName)
{
    katana::survey::ReportInput input;
    input.fileName = fileName;
    for (const SiblingFile& sibling : read.siblingsRead) {
        input.siblingFiles.push_back(sibling.name);
    }
    input.formatId = read.formatId;
    const auto descriptor = formatRegistry().find(read.formatId);
    input.formatName = descriptor ? descriptor->humanName : read.formatId;
    input.parserVersion = read.parserVersion;
    input.recordsRead = read.recordsRead;
    input.recordsSkipped = read.recordsSkipped;
    input.warnings.reserve(read.warnings.size());
    for (const ReadWarning& warning : read.warnings) {
        katana::survey::SourceRecord source;
        source.format = input.formatName;
        source.fileName = warning.fileName.empty() ? fileName : warning.fileName;
        source.recordNumber = warning.record;
        input.warnings.push_back({warning.message, std::move(source)});
    }
    input.notCarried = read.notCarried;
    return input;
}

std::string describe(const ReadWarning& warning)
{
    std::string text = warning.fileName;
    if (warning.record != 0) {
        text += text.empty() ? "record " : " record ";
        text += std::to_string(warning.record);
    }
    if (!text.empty()) {
        text += ": ";
    }
    text += warning.message;
    return text;
}

Result<ReadResult> readSurvey(const FormatRegistry& registry, std::string_view formatId,
                              std::string_view bytes, std::string_view fileName,
                              const ReadOptions& options)
{
    Result<FormatDescriptor> descriptor = registry.find(formatId);
    if (!descriptor) {
        return descriptor.error();
    }
    const FormatReader reader = registry.reader(formatId);
    if (reader == nullptr) {
        return makeError(ErrorCode::Unsupported,
                         "Katana recognises " + descriptor->humanName +
                             " files but cannot import them in one step",
                         formatId == kDelimitedPointsFormatId
                             ? "a coordinate file is read through its column layout"
                             : "no reader is registered for this format");
    }
    const std::string name = katana::survey::sourceFileName(fileName);
    if (bytes.size() > kMaxSurveyFileBytes) {
        return makeError(ErrorCode::InvalidArgument,
                         name + " is " + std::to_string(bytes.size()) +
                             " bytes, larger than the " + std::to_string(kMaxSurveyFileBytes) +
                             " bytes a survey file may be");
    }

    // Every sibling the reader fetches goes through here: checked, counted and
    // kept, bytes and all, so a survey job can store exactly what was read.
    auto fetched = std::make_shared<std::vector<SiblingFile>>();
    ReadOptions guarded = options;
    if (options.siblings) {
        guarded.siblings = [lookup = options.siblings,
                            fetched](std::string_view sibling) -> Result<std::string> {
            if (katana::core::Status status = checkSiblingName(sibling); !status.ok()) {
                return status.error();
            }
            for (const SiblingFile& already : *fetched) {
                if (already.name == sibling) {
                    return already.bytes;
                }
            }
            if (fetched->size() >= kMaxSiblingFiles) {
                return makeError(ErrorCode::InvalidArgument,
                                 "refused to read '" + std::string(sibling) + "': one import may " +
                                     "read at most " + std::to_string(kMaxSiblingFiles) +
                                     " other files");
            }
            Result<std::string> read = lookup(sibling);
            if (!read) {
                return read.error();
            }
            if (read->size() > kMaxSurveyFileBytes) {
                return makeError(ErrorCode::InvalidArgument,
                                 std::string(sibling) + " is larger than a survey file may be");
            }
            fetched->push_back(SiblingFile{std::string(sibling), *read});
            return std::move(read).value();
        };
    }

    Result<ReadResult> result = makeError(ErrorCode::Internal, "the reader did not run");
    try {
        result = reader(bytes, name, guarded);
    } catch (const std::bad_alloc&) {
        return makeError(ErrorCode::Internal,
                         "ran out of memory reading " + name + " as " + descriptor->humanName);
    } catch (const std::exception& error) {
        return makeError(ErrorCode::Internal,
                         "the " + descriptor->humanName + " reader failed on " + name + ": " +
                             error.what());
    }
    if (!result) {
        return result.error();
    }
    if (katana::core::Status status = katana::survey::validateProject(result->project);
        !status.ok()) {
        return makeError(ErrorCode::FileImportFailure,
                         "the " + descriptor->humanName + " reader produced data from " + name +
                             " that does not hold together: " + status.error().message,
                         status.error().context);
    }
    result->formatId = descriptor->id;
    result->parserVersion = descriptor->parserVersion;
    result->siblingsRead = std::move(*fetched);
    return result;
}

SiblingLookup siblingsInFolder(std::filesystem::path folder, std::size_t maxBytes)
{
    return [folder = std::move(folder), maxBytes](std::string_view name) -> Result<std::string> {
        namespace fs = std::filesystem;
        if (katana::core::Status status = checkSiblingName(name); !status.ok()) {
            return status.error();
        }
        std::error_code error;
        fs::path candidate = folder / pathFromUtf8(name);
        if (!fs::is_regular_file(candidate, error)) {
            candidate.clear();
            for (fs::directory_iterator entry(folder, error), end; !error && entry != end;
                 entry.increment(error)) {
                std::error_code typeError;
                if (entry->is_regular_file(typeError) &&
                    equalsIgnoringAsciiCase(nameOf(entry->path()), name)) {
                    candidate = entry->path();
                    break;
                }
            }
        }
        if (candidate.empty()) {
            return makeError(ErrorCode::NotFound,
                             "there is no file called '" + std::string(name) +
                                 "' beside the file being read");
        }
        const std::uintmax_t size = fs::file_size(candidate, error);
        if (error) {
            return makeError(ErrorCode::FileImportFailure,
                             "could not read the size of '" + std::string(name) + "'",
                             error.message());
        }
        if (size > maxBytes) {
            return makeError(ErrorCode::InvalidArgument,
                             "'" + std::string(name) + "' is " + std::to_string(size) +
                                 " bytes, larger than the " + std::to_string(maxBytes) +
                                 " bytes allowed");
        }
        std::ifstream stream(candidate, std::ios::binary);
        std::string bytes(static_cast<std::size_t>(size), '\0');
        if (!stream || !stream.read(bytes.data(), static_cast<std::streamsize>(bytes.size()))) {
            return makeError(ErrorCode::FileImportFailure,
                             "could not read '" + std::string(name) + "'");
        }
        return bytes;
    };
}

SiblingLookup siblingsInMemory(std::vector<SiblingFile> files)
{
    return [files = std::move(files)](std::string_view name) -> Result<std::string> {
        if (katana::core::Status status = checkSiblingName(name); !status.ok()) {
            return status.error();
        }
        for (const SiblingFile& file : files) {
            if (file.name == name) {
                return file.bytes;
            }
        }
        for (const SiblingFile& file : files) {
            if (equalsIgnoringAsciiCase(file.name, name)) {
                return file.bytes;
            }
        }
        return makeError(ErrorCode::NotFound,
                         "there is no file called '" + std::string(name) + "' in the stored job");
    };
}

ImportResult toImportResult(ReadResult result)
{
    ImportResult imported;
    imported.project = std::move(result.project);
    imported.formatId = std::move(result.formatId);
    imported.recordsRead = result.recordsRead;
    imported.recordsSkipped = result.recordsSkipped;
    imported.warnings.reserve(result.warnings.size() + result.notCarried.size());
    for (const ReadWarning& warning : result.warnings) {
        imported.warnings.push_back(describe(warning));
    }
    for (const std::string& missing : result.notCarried) {
        imported.warnings.push_back("not in the file: " + missing);
    }
    return imported;
}

} // namespace katana::surveyio
