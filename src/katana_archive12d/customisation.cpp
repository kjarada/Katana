#include "katana/archive12d/customisation.hpp"

#include <algorithm>
#include <fstream>
#include <set>
#include <sstream>
#include <utility>

#include "katana/archive12d/map_file.hpp"
#include "katana/archive12d/style_library.hpp"
#include "katana/archive12d/text_encoding.hpp"

namespace katana::archive12d {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;

// A style library's first real content is a block header - `worldstyle "..."`
// - and the files open with a wall of `//` comments, so looking for the
// keyword anywhere is what tells them apart. A mapfile is XML and says
// <map_file>.
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
    return kind == CustomisationFile::MapFile ? "mapfile" : "style library";
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
                     "this is neither a 12d mapfile nor a linestyle or symbol library");
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
    const auto decoded = decodeText(buffer.str());
    if (!decoded) {
        return makeError(decoded.error().code, "cannot read " + path.filename().string() + ": " +
                                                   decoded.error().describe());
    }
    const auto kind = customisationKind(decoded->text);
    if (!kind) {
        return makeError(kind.error().code, path.filename().string() + ": " +
                                                kind.error().describe());
    }

    LoadedFile loaded;
    loaded.path = path;
    loaded.kind = *kind;
    const std::string where = path.filename().string() + ": ";

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
        auto read = readStyleLibraryInto(std::move(into.library), decoded->text);
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
