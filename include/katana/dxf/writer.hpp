#pragma once

// Entities, layers and linetypes -> a DXF file (R2000, ASCII), with no
// third-party library.
//
// WHY A WRITER OF OUR OWN. GDAL's DXF driver took 116 s to write a 28 000
// entity survey drawing (about 2 ms a feature, CPU-bound), wrote every closed
// polyline and circle as a SOLID HATCH - a parcel arrived in the client's CAD
// program as a filled black shape - chorded every arc, and skipped text.
//
// WHAT IS WRITTEN.
//
//   Point                     POINT, with its elevation as Z.
//   Line                      LINE, heights as the two Z values.
//   Arc, Circle               ARC, CIRCLE: true curves, never chords.
//   Polyline                  LWPOLYLINE with the closed flag - NEVER a HATCH.
//                             One whose vertices differ in height is a 3D
//                             POLYLINE instead, since an LWPOLYLINE has one
//                             elevation; one height becomes its elevation.
//   Text                      TEXT, left of the baseline, height, rotation.
//                             A Text holding line breaks is one TEXT a line.
//   Dimension                 exploded: extension lines, the dimension line,
//                             two ticks and the measurement as TEXT.
//
// Layers go into the LAYER table with their colour (the nearest of the 255
// indexed colours - R2000 has no true colour), visibility, lock, linetype and
// lineweight; linetypes with patterns into LTYPE. A layer path is not a legal
// DXF layer name - "/" is one of the characters the format refuses in a
// symbol name - so "survey/kerb" is written "survey$kerb", the separator
// bound references use, and the full path goes beside it as extended data
// that this module's reader takes back. See layerNameFor.
//
// Entity colours are indexed colours too. Properties are not written: DXF has
// no place for them that other programs read.
//
// Strings are ASCII: a character outside it is written \U+XXXX, which every
// release from R2000 reads, and the degree, plus-minus and diameter signs as
// the %%d %%p %%c every release reads.

#include <cstddef>
#include <filesystem>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/model.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::dxf {

struct ExportOptions {
    // Empty exports the whole model; otherwise only these entities.
    std::vector<katana::entity::EntityId> entities;
    // Empty exports every layer; otherwise only entities on these layers.
    std::vector<std::string> layers;
    // Added back to every plan coordinate, undoing an import's originShift.
    std::optional<katana::geometry::Vec2> originShift;
};

struct DxfExport {
    std::string text;                  // the file (writeDxf); empty from writeDxfFile
    std::size_t entitiesWritten = 0;   // Katana entities written
    std::size_t dxfEntitiesWritten = 0; // DXF entities: a Dimension is several
    std::size_t entitiesSkipped = 0;
    std::size_t layersWritten = 0;
    std::size_t linetypesWritten = 0;
    std::size_t bytesWritten = 0;
    std::vector<std::string> warnings;
};

[[nodiscard]] katana::core::Result<DxfExport> writeDxf(const katana::entity::Model& model,
                                                       const ExportOptions& options = {});
// Writes to `path`, replacing it. Fails with FileExportFailure when the file
// cannot be written; a failed write leaves no half-written file behind it.
[[nodiscard]] katana::core::Result<DxfExport>
writeDxfFile(const katana::entity::Model& model, const std::filesystem::path& path,
             const ExportOptions& options = {});

// The DXF layer name a Katana layer path is written under: "/" becomes "$",
// and each other character the format refuses in a symbol name
// (< > \ " : ; ? * | = `) becomes "_". Never empty.
[[nodiscard]] std::string layerNameFor(std::string_view layerPath);

} // namespace katana::dxf
