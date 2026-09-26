// A job's written files, put in place when it applies (staged_files.hpp).

#include "staged_files.hpp"

#include <atomic>
#include <string>
#include <system_error>
#include <utility>
#include <vector>

#if defined(_WIN32)
#include <process.h>
#else
#include <unistd.h>
#endif

#include "../import_records.hpp"

namespace katana::app::geo {

using katana::core::ErrorCode;
using katana::core::makeError;

namespace {

long long processId()
{
#if defined(_WIN32)
    return static_cast<long long>(_getpid());
#else
    return static_cast<long long>(getpid());
#endif
}

} // namespace

StagedFiles::StagedFiles(std::filesystem::path folder, std::filesystem::path target)
    : folder_(std::move(folder)), target_(std::move(target)), writeTo_(folder_ / target_.filename())
{
}

StagedFiles::~StagedFiles()
{
    std::error_code error;
    std::filesystem::remove_all(folder_, error);
}

katana::core::Result<std::shared_ptr<StagedFiles>>
StagedFiles::beside(const std::filesystem::path& target)
{
    if (target.filename().empty()) {
        return makeError(ErrorCode::InvalidArgument, "a file to write is named", pathText(target));
    }
    const std::filesystem::path parent =
        target.parent_path().empty() ? std::filesystem::path(".") : target.parent_path();
    // One process may stage several at once (two jobs), and two processes
    // may write into one folder: the id and a count keep them apart.
    static std::atomic<std::uint64_t> made{0};
    const std::filesystem::path folder =
        parent / (".katana-staging-" + std::to_string(processId()) + "-" + std::to_string(++made));
    std::error_code error;
    if (!std::filesystem::create_directory(folder, error) || error) {
        return makeError(ErrorCode::FileExportFailure,
                         "cannot write beside it: " +
                             (error ? error.message() : std::string("the folder is in the way")),
                         pathText(target));
    }
    return std::shared_ptr<StagedFiles>(new StagedFiles(folder, target));
}

katana::core::Status StagedFiles::place() const
{
    const std::filesystem::path parent =
        target_.parent_path().empty() ? std::filesystem::path(".") : target_.parent_path();
    std::error_code error;
    std::vector<std::filesystem::path> written;
    for (const auto& entry : std::filesystem::directory_iterator(folder_, error)) {
        written.push_back(entry.path());
    }
    if (error) {
        return makeError(ErrorCode::FileExportFailure, "what was written cannot be read back: " +
                                                           error.message(),
                         pathText(folder_));
    }
    for (const std::filesystem::path& from : written) {
        const std::filesystem::path to = parent / from.filename();
        std::filesystem::rename(from, to, error);
        if (error) {
            // A rename onto a file that exists is refused on some systems:
            // the file the export replaces goes first, as a writer asked to
            // replace it would have removed it.
            error.clear();
            std::filesystem::remove(to, error);
            std::filesystem::rename(from, to, error);
        }
        if (error) {
            return makeError(ErrorCode::FileExportFailure,
                             "cannot put the written file in place: " + error.message(),
                             pathText(to));
        }
    }
    return {};
}

katana::core::Error StagedFiles::asTarget(katana::core::Error error) const
{
    const std::filesystem::path parent =
        target_.parent_path().empty() ? std::filesystem::path(".") : target_.parent_path();
    // The file first, then the folder alone (a shapefile's .dbf beside it),
    // in each spelling a writer may have used.
    const std::vector<std::pair<std::string, std::string>> spellings{
        {pathText(writeTo_), pathText(target_)},
        {writeTo_.string(), target_.string()},
        {writeTo_.generic_string(), target_.generic_string()},
        {pathText(folder_), pathText(parent)},
        {folder_.string(), parent.string()},
        {folder_.generic_string(), parent.generic_string()},
    };
    for (std::string* text : {&error.message, &error.context}) {
        for (const auto& [from, to] : spellings) {
            for (std::size_t at = text->find(from); !from.empty() && at != std::string::npos;
                 at = text->find(from, at + to.size())) {
                text->replace(at, from.size(), to);
            }
        }
    }
    return error;
}

} // namespace katana::app::geo
