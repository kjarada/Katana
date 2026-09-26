#pragma once

// Shared between the katana_io sources, never installed: the names GDAL is
// asked to open a path by (geo/formats.cpp), so that GdalDataset::open and
// the routing of a file (gis::identifyContent) look into an archive the
// same way.

#include <filesystem>
#include <string>
#include <vector>

namespace katana::gis::detail {

struct OpenNames {
    // In the order to try them: the path itself, UTF-8 with forward slashes;
    // then, for an archive GDAL does not open as it is (.zip, .kmz, .tar,
    // .tgz), its inside as a folder (/vsizip/{<path>}), and the one member
    // of it that is a dataset; for one compressed file (.gz), its inside
    // (/vsigzip/<path>).
    std::vector<std::string> names;
    // Set when the archive holds several datasets and none can be chosen:
    // the refusal, naming them and how to name one.
    std::string ambiguity;
};

[[nodiscard]] OpenNames openNames(const std::filesystem::path& path);

} // namespace katana::gis::detail
