// INFO <file> (docs/interop.md, "Dataset information"): what a GIS file or a
// point cloud holds, read without importing it - interop::describeSource, the
// description GIS > Dataset Information shows too. The read is the work, so
// the window reads a large file on a worker.
//
//   INFO <file>
//
// INFO <id> stays the interpreter's - an entity - unless a file of that name
// exists; INFO #<id> is always the entity (takesInfo).

#include <filesystem>
#include <string>
#include <system_error>

#include "../import_records.hpp"
#include "katana/core/text.hpp"
#include "katana/interop/dataset_info.hpp"
#include "verb_table.hpp"

namespace katana::app::geo {

namespace interop = katana::interop;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

constexpr const char* kUsage = "usage: INFO <file> | INFO <id>";

} // namespace

bool takesInfo(const Tokens& tokens)
{
    // INFO 12 is the interpreter's: an entity, described. Every INFO was once
    // taken for a file, so katana_describe_entity answered "file does not
    // exist" in every build with GDAL. A file that is really called 12 is
    // still described - but INFO #12 is always the entity: Entity
    // Information and katana_describe_entity send it, and a file called 1 or
    // #1 beside the program (a mistyped shell 2>1 makes one) once turned both
    // into "no importer reads files named ''".
    if (tokens.size() == 2 && katana::cad::CommandInterpreter::isEntityId(tokens[1])) {
        if (tokens[1].starts_with('#')) {
            return false;
        }
        std::error_code error;
        return std::filesystem::exists(pathFromText(tokens[1]), error);
    }
    return true;
}

Result<Prepared> prepareInfo(Context&, const Tokens&, std::string_view line)
{
    const std::string path = restOfLine(line);
    if (path.empty()) {
        return makeError(ErrorCode::InvalidArgument, kUsage);
    }
    const std::filesystem::path file = pathFromText(path);
    Prepared prepared;
    prepared.title = "INFO " + pathText(file.filename());
    prepared.work = [file](const std::stop_token&, const Progress&) -> Result<Apply> {
        auto description = interop::describeSource(file);
        if (!description) {
            return description.error();
        }
        const std::string text(katana::core::trimmed(interop::formatDescription(*description)));
        return Apply([text](Context&) -> Result<std::string> { return text; });
    };
    return prepared;
}

} // namespace katana::app::geo
