#pragma once

// 12d Archive files - .12da, and the zipped .12daz - in and out (PLAN.MD
// Phase 20 and 20.2).
//
// The format itself lives in katana_archive12d, which needs no third-party
// library and so can be built and sanitized without GDAL. This is only the
// part that touches a FILE: reading its bytes, opening the ZIP container of a
// .12daz, and turning the point clouds a 12da can carry into reference layers,
// which archive12d cannot see.
//
// A 12da is not a vector file like the others. One import can bring entities,
// the layers they sit on, named alignments, surfaces and point clouds, and
// they do not all live in the same place: entities, layers and alignments go
// through commands and are undoable; surfaces and clouds are session data the
// front end owns. So this returns them separately and the caller places each.

#include <cstdint>
#include <filesystem>
#include <optional>
#include <string>
#include <vector>

#include "katana/archive12d/domain.hpp"
#include "katana/entity/model.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/interop/reference_data.hpp"

namespace katana::interop {

// Without the leading dot: 12da and 12daz. There is no ".12dz"; it was
// accepted for a while by mistake.
[[nodiscard]] std::vector<std::string> archive12dExtensions();
[[nodiscard]] bool isZippedArchive12d(const std::filesystem::path& path);

struct Archive12dImportOptions {
    // Subtracted from every plan coordinate. See VectorImportOptions.
    std::optional<katana::geometry::Vec2> originShift;
    // Parent layer for every model; empty puts models at the top level.
    std::string layerPrefix;
    double curveTolerance = 0.001;
    // The largest file, or zip member once inflated, that will be read. 12da
    // is text and is held in memory whole; the biggest real export seen is
    // 58 MB (29 MB of text once decoded). A gigabyte is room for anything
    // plausible and a bound on what a hostile zip can ask for.
    std::uint64_t maxBytes = 1ull << 30;
};

struct Archive12dImportResult {
    std::vector<katana::entity::Entity> entities;
    std::vector<katana::entity::Layer> layersNeeded;
    // One per distinct 12d linestyle; the caller creates those the drawing
    // lacks, in the same transaction as the layers.
    std::vector<katana::entity::Style> stylesNeeded;
    // Names are unique within the import; the caller must still rename any
    // that the drawing already has, or createAlignment will refuse it.
    std::vector<katana::entity::Alignment> alignments;
    std::vector<katana::archive12d::ImportedSurface> surfaces;
    // 12d trimeshes: session data like the surfaces, held by the caller.
    std::vector<katana::archive12d::ImportedMesh> meshes;
    std::vector<PointCloudLayer> clouds;
    std::vector<katana::archive12d::ElementTally> tally;

    katana::geometry::Box2 bounds;
    std::string encoding;       // as detected: "UTF-16 little-endian" ...
    std::string archiveVersion; // 12d Model's archive_version, when the file says
    std::string memberName;     // the .12da inside a .12daz; empty for a plain file
    std::vector<std::string> warnings;
};

[[nodiscard]] katana::core::Result<Archive12dImportResult>
importArchive12d(const std::filesystem::path& path, const Archive12dImportOptions& options = {});

struct Archive12dExportOptions {
    std::vector<katana::entity::EntityId> entities; // empty: the whole model
    std::optional<katana::geometry::Vec2> originShift;
    // UTF-16 little-endian with a byte order mark is what 12d Model writes, and
    // so what it is certain to read. UTF-8 is for a file meant to be read, or
    // diffed, by people and other tools.
    bool utf16 = true;
    int decimalPlaces = 8;
};

struct Archive12dExportResult {
    std::size_t entitiesWritten = 0;
    std::size_t entitiesSkipped = 0;
    std::size_t alignmentsWritten = 0;
    std::size_t surfacesWritten = 0;
    std::uint64_t bytesWritten = 0;
    std::vector<std::string> warnings;
};

// The extension decides the container: .12daz is zipped, holding one member
// named after the file with the extension .12da, as 12d Model's are.
[[nodiscard]] katana::core::Result<Archive12dExportResult>
exportArchive12d(const katana::entity::Model& model,
                 const std::vector<katana::archive12d::ExportSurface>& surfaces,
                 const std::filesystem::path& path, const Archive12dExportOptions& options = {});

} // namespace katana::interop
