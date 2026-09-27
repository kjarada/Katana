#pragma once

// A file a job writes, put in place only when the job applies
// (docs/interop.md, "EXPORT and COPC as jobs").
//
// A job's work may not change anything a person can see: a cancelled job
// never applies, whenever the cancel came (src/katana_qt/jobs.hpp). The
// writers - GDAL's, the DXF and 12d archive writers, PDAL's COPC - cannot be
// stopped part way, so EXPORT and COPC write into a folder of their own
// beside the target, and their apply moves what was written into place. A
// job cancelled after its write leaves nothing: the folder is removed with
// the job.
//
// Beside the target, not in a scratch folder elsewhere, so the move is a
// rename on the same volume. A folder, not a scratch name, because a
// shapefile is several files named by its stem, a writer takes its driver
// from the extension, and a .12daz names its member after the file.

#include <filesystem>
#include <memory>

#include "katana/core/error.hpp"

namespace katana::app::geo {

class StagedFiles {
  public:
    // FileExportFailure when the folder cannot be made: the target's folder
    // does not exist, or cannot be written.
    [[nodiscard]] static katana::core::Result<std::shared_ptr<StagedFiles>>
    beside(const std::filesystem::path& target);

    // Removes the folder and whatever is still in it.
    ~StagedFiles();
    StagedFiles(const StagedFiles&) = delete;
    StagedFiles& operator=(const StagedFiles&) = delete;

    // Where the writer writes: the folder, under the target's own name.
    [[nodiscard]] const std::filesystem::path& writeTo() const { return writeTo_; }

    // Every file written moved beside the target, each replacing one of the
    // same name. FileExportFailure naming the file that could not be moved.
    [[nodiscard]] katana::core::Status place() const;

    // A writer's failure as the person asked for it: the staging folder's
    // path, in the message or the context, is the target's. An export
    // refused for its CRS named ".katana-staging-19705-1/f.kml", a folder no
    // one made and that is gone by the time they read it.
    [[nodiscard]] katana::core::Error asTarget(katana::core::Error error) const;

  private:
    StagedFiles(std::filesystem::path folder, std::filesystem::path target);

    std::filesystem::path folder_;
    std::filesystem::path target_;
    std::filesystem::path writeTo_;
};

} // namespace katana::app::geo
