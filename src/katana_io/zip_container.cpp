#include "katana/gis/zip_container.hpp"

#include <cpl_conv.h>
#include <cpl_error.h>
#include <cpl_string.h>
#include <cpl_vsi.h>

#include <algorithm>
#include <system_error>

namespace katana::gis {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;

// GDAL's name for a zip that does not end in ".zip": the archive's path in
// braces. Forward slashes, UTF-8 - which is what GDAL takes on every platform
// (GDAL_FILENAME_IS_UTF8 defaults to YES).
std::string vsiRoot(const std::filesystem::path& archive)
{
    const std::u8string generic = archive.generic_u8string();
    return "/vsizip/{" + std::string(generic.begin(), generic.end()) + "}";
}

std::string lastError()
{
    const char* message = CPLGetLastErrorMsg();
    return message != nullptr ? std::string(message) : std::string();
}

std::string withReason(const std::string& context)
{
    const std::string reason = lastError();
    return reason.empty() ? context : context + ": " + reason;
}

// Errors are returned, not printed: GDAL's default handler writes to stderr,
// which in the desktop application nobody is reading.
struct QuietErrors {
    QuietErrors()
    {
        CPLErrorReset();
        CPLPushErrorHandler(CPLQuietErrorHandler);
    }
    ~QuietErrors() { CPLPopErrorHandler(); }
    QuietErrors(const QuietErrors&) = delete;
    QuietErrors& operator=(const QuietErrors&) = delete;
};

} // namespace

katana::core::Result<std::vector<ZipMember>> listZip(const std::filesystem::path& archive)
{
    std::error_code ignored;
    if (!std::filesystem::is_regular_file(archive, ignored)) {
        return makeError(ErrorCode::FileImportFailure, "the file does not exist",
                         archive.string());
    }
    const QuietErrors quiet;
    const std::string root = vsiRoot(archive);
    char** names = VSIReadDirRecursive(root.c_str());
    if (names == nullptr) {
        return makeError(ErrorCode::FileImportFailure, "the file is not a ZIP archive",
                         withReason(archive.string()));
    }
    std::vector<ZipMember> members;
    for (char** name = names; *name != nullptr; ++name) {
        const std::string entry(*name);
        if (entry.empty() || entry.back() == '/') {
            continue; // a directory
        }
        VSIStatBufL status;
        if (VSIStatL((root + "/" + entry).c_str(), &status) != 0 || VSI_ISDIR(status.st_mode)) {
            continue;
        }
        members.push_back(ZipMember{entry, static_cast<std::uint64_t>(status.st_size)});
    }
    CSLDestroy(names);
    return members;
}

katana::core::Result<std::string> readZipMember(const std::filesystem::path& archive,
                                                std::string_view member, std::uint64_t maxBytes)
{
    auto members = listZip(archive);
    if (!members) {
        return members.error();
    }
    const auto found = std::find_if(members->begin(), members->end(),
                                    [&](const ZipMember& m) { return m.name == member; });
    if (found == members->end()) {
        return makeError(ErrorCode::NotFound, "the archive has no such member",
                         archive.string() + ": " + std::string(member));
    }
    if (found->size > maxBytes) {
        return makeError(ErrorCode::FileImportFailure,
                         "the member is larger than the limit of " + std::to_string(maxBytes) +
                             " bytes",
                         std::string(member) + " is " + std::to_string(found->size));
    }

    const QuietErrors quiet;
    VSILFILE* file = VSIFOpenL((vsiRoot(archive) + "/" + std::string(member)).c_str(), "rb");
    if (file == nullptr) {
        return makeError(ErrorCode::FileImportFailure, "the member could not be opened",
                         withReason(std::string(member)));
    }
    std::string bytes(static_cast<std::size_t>(found->size), '\0');
    const std::size_t read = bytes.empty() ? 0 : VSIFReadL(bytes.data(), 1, bytes.size(), file);
    VSIFCloseL(file);
    if (read != bytes.size()) {
        // The directory promised more than the stream held: a truncated or
        // corrupt archive, which must not be parsed as if it were whole.
        return makeError(ErrorCode::FileImportFailure,
                         "the member is truncated or corrupt: " + std::to_string(read) + " of " +
                             std::to_string(bytes.size()) + " bytes could be read",
                         std::string(member));
    }
    return bytes;
}

katana::core::Status writeZip(const std::filesystem::path& archive, std::string_view memberName,
                              std::string_view bytes)
{
    if (memberName.empty() || memberName.find_first_of("\\:") != std::string_view::npos ||
        memberName.front() == '/' || memberName.find("..") != std::string_view::npos) {
        return makeError(ErrorCode::InvalidArgument,
                         "a member name must be a plain relative path", std::string(memberName));
    }
    std::filesystem::path temporary = archive;
    temporary += ".writing";
    std::error_code error;
    std::filesystem::remove(temporary, error); // vsizip can create an archive, not update one

    {
        const QuietErrors quiet;
        VSILFILE* file =
            VSIFOpenL((vsiRoot(temporary) + "/" + std::string(memberName)).c_str(), "wb");
        if (file == nullptr) {
            return makeError(ErrorCode::FileExportFailure, "the archive could not be created",
                             withReason(archive.string()));
        }
        const std::size_t written =
            bytes.empty() ? 0 : VSIFWriteL(bytes.data(), 1, bytes.size(), file);
        // The central directory is written on close, so a failed close is a
        // failed archive however many bytes went in.
        const int closed = VSIFCloseL(file);
        if (written != bytes.size() || closed != 0) {
            const std::string reason = withReason(archive.string());
            std::filesystem::remove(temporary, error);
            return makeError(ErrorCode::FileExportFailure, "the archive could not be written",
                             reason);
        }
    }
    std::filesystem::rename(temporary, archive, error);
    if (error) {
        std::filesystem::remove(temporary, error);
        return makeError(ErrorCode::FileExportFailure, "the archive could not be moved into place",
                         archive.string());
    }
    return {};
}

} // namespace katana::gis
