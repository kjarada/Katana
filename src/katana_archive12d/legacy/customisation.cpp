#include "customisation.hpp"

#include <fstream>
#include <set>
#include <sstream>
#include <utility>

#include "map_file.hpp"
#include "style_library.hpp"
#include "katana/core/text_encoding.hpp"

namespace katana::archive12d {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;

// A style library's first real content is a block header - `worldstyle "..."`
// - and the files open with a wall of `//` comments, so looking for the
// keyword anywhere is what tells them apart. A survey code file (a mapfile)
// is XML and says <map_file>.
[[nodiscard]] bool looksLikeStyleLibrary(std::string_view text)
{
    for (const std::string_view keyword : {"worldstyle", "paperstyle", "twoptstyle"}) {
        if (text.find(keyword) != std::string_view::npos) {
            return true;
        }
    }
    return false;
}

} // namespace

const char* toString(CustomisationFile kind)
{
    return kind == CustomisationFile::MapFile ? "survey code file" : "style library";
}

katana::core::Result<CustomisationFile> customisationKind(std::string_view text)
{
    if (text.find("<map_file") != std::string_view::npos) {
        return CustomisationFile::MapFile;
    }
    if (looksLikeStyleLibrary(text)) {
        return CustomisationFile::StyleLibrary;
    }
    return makeError(ErrorCode::InvalidArgument,
                     "this is neither a survey code file nor a linestyle or symbol library");
}

std::vector<std::string> Customisation::unresolvedStyles() const
{
    std::set<std::string> missing;
    for (const std::string& name : map.stylesReferenced()) {
        if (!library.contains(name)) {
            missing.insert(name);
        }
    }
    return {missing.begin(), missing.end()};
}

katana::core::Result<Customisation> readCustomisationInto(Customisation into,
                                                          const std::filesystem::path& path)
{
    std::ifstream file(path, std::ios::binary);
    if (!file) {
        return makeError(ErrorCode::NotFound, "cannot open this file", path.string());
    }
    std::ostringstream buffer;
    buffer << file.rdbuf();
    // The NAME, as UTF-8: it is stamped on every definition and shown to a
    // person, and path::string() would turn a non-ASCII name into whatever
    // the Windows code page makes of it.
    const std::u8string name = path.filename().u8string();
    auto read = readCustomisationBytes(std::move(into),
                                       std::string_view(reinterpret_cast<const char*>(name.data()),
                                                        name.size()),
                                       buffer.str());
    if (read) {
        read->files.back().path = path; // the caller's path, for a caller that reopens it
    }
    return read;
}

katana::core::Result<Customisation> readCustomisationBytes(Customisation into, std::string_view name,
                                                           std::string_view bytes)
{
    const std::string where = std::string(name) + ": ";
    const auto decoded = katana::core::decodeText(bytes);
    if (!decoded) {
        return makeError(decoded.error().code, "cannot read " + std::string(name) + ": " +
                                                   decoded.error().describe());
    }
    const auto kind = customisationKind(decoded->text);
    if (!kind) {
        return makeError(kind.error().code, where + kind.error().describe());
    }

    LoadedFile loaded;
    loaded.path = std::filesystem::path(std::u8string(name.begin(), name.end()));
    loaded.kind = *kind;

    if (*kind == CustomisationFile::MapFile) {
        const std::size_t before = into.map.size();
        auto read = readMapFileInto(std::move(into.map), decoded->text);
        if (!read) {
            return makeError(read.error().code, where + read.error().describe());
        }
        into.map = std::move(read->map);
        loaded.read = into.map.size() - before;
        for (const std::string& warning : read->warnings) {
            into.warnings.push_back(where + warning);
        }
    } else {
        const std::size_t before = into.library.size();
        auto read = readStyleLibraryInto(std::move(into.library), decoded->text, name);
        if (!read) {
            return makeError(read.error().code, where + read.error().describe());
        }
        into.library = std::move(read->library);
        loaded.read = into.library.size() - before + read->replaced;
        loaded.replaced = read->replaced;
        for (const std::string& warning : read->warnings) {
            into.warnings.push_back(where + warning);
        }
    }
    into.files.push_back(std::move(loaded));
    return into;
}

katana::core::Result<Customisation>
readCustomisation(const std::vector<std::filesystem::path>& paths)
{
    Customisation customisation;
    for (const std::filesystem::path& path : paths) {
        auto next = readCustomisationInto(std::move(customisation), path);
        if (!next) {
            return next.error();
        }
        customisation = std::move(*next);
    }
    return customisation;
}

} // namespace katana::archive12d
