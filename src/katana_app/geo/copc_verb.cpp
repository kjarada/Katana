// COPC (docs/interop.md, "Point-cloud level of detail"): a point cloud file
// rewritten whole as a Cloud Optimised Point Cloud, every point kept, so a
// later IMPORT can ask for a level of detail instead of decimating.
//
//   COPC <source> <destination.copc.laz>
//
// Each path one word or quoted. The conversion is the work, written beside
// its place and moved there by the apply (staged_files.hpp): a cancelled
// COPC leaves no half-made file.

#include <filesystem>
#include <memory>
#include <string>

#include "../import_records.hpp"
#include "katana/pointcloud/point_cloud_engine.hpp"
#include "staged_files.hpp"
#include "verb_table.hpp"

namespace katana::app::geo {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

constexpr const char* kUsage = "usage: COPC <source> <destination.copc.laz>";

} // namespace

Result<Prepared> prepareCopc(Context&, const Tokens& tokens, std::string_view)
{
    if (tokens.size() != 3) {
        return makeError(ErrorCode::InvalidArgument, kUsage);
    }
    const std::filesystem::path source = pathFromText(tokens[1]);
    const std::filesystem::path destination = pathFromText(tokens[2]);
    Prepared prepared;
    prepared.title = "COPC " + pathText(source.filename());
    prepared.work = [source, destination](const std::stop_token& stop,
                                          const Progress&) -> Result<Apply> {
        if (stop.stop_requested()) {
            return makeError(ErrorCode::InvalidState, "cancelled");
        }
        auto staged = StagedFiles::beside(destination);
        if (!staged) {
            return staged.error();
        }
        if (auto converted =
                katana::pointcloud::PointCloudEngine{}.convertToCopc(source, (*staged)->writeTo());
            !converted) {
            return converted.error();
        }
        if (stop.stop_requested()) {
            return makeError(ErrorCode::InvalidState, "cancelled");
        }
        const std::shared_ptr<StagedFiles> files = *staged;
        const std::string reply = "converted source=" + recordText(pathText(source)) +
                                  " file=" + recordText(pathText(destination)) + " format=copc";
        return Apply([files, reply](Context&) -> Result<std::string> {
            if (auto placed = files->place(); !placed) {
                return placed.error();
            }
            return reply;
        });
    };
    return prepared;
}

} // namespace katana::app::geo
