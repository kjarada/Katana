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
// Layers go into the LAYER table with their colour, visibility, lock,
// linetype and lineweight; linetypes with patterns into LTYPE. Colours, the
// layers' and the entities', are the nearest of the 255 indexed colours:
// R2000 has no true colour.
//
// WHAT THE FORMAT CANNOT HOLD goes beside the entity or layer as extended
// data under the registered application KATANA, which other programs pass by
// and this module's reader takes back (reader.hpp, kExtendedPath and after):
// a layer's path - "/" is one of the characters the format refuses in a
// symbol name, so "survey/kerb" is written "survey$kerb", the separator bound
// references use (see layerNameFor); a colour no index is exactly; and the
// heights of an entity known at only some of its vertices, which is written
// in plan, since the format has no "no height" and a Z of 0 would be a false
// level.
//
// Properties are not written: DXF has no place for them that other programs
// read.
//
// Strings are ASCII: a character outside it is written \U+XXXX, which every
// release from R2000 reads, and the degree, plus-minus and diameter signs as
// the %%d %%p %%c every release reads.

#include <cstddef>
#include <filesystem>
#include <map>
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
    // The scale paper-sized annotation is written at (docs/annotation.md): a
    // 2.5 mm text is 2.5 x scale / 1000 model units in the file, since a DXF
    // TEXT has one height. The application passes its annotation scale.
    double annotationScale = 1000.0;
    // Annotation the caller has drawn out for this export, by entity: an
    // entity found here is written as these shapes - lines, polylines and
    // single-line texts, on its layer and in its colour - instead of as
    // itself. It is how a label (whose words and place are worked out for a
    // view by katana_cad's placer, which this module may not see), a
    // dimension of a kind other than aligned and a leader's arrowhead and
    // callout frame reach the file: cad/annotation/export_annotation.hpp
    // draws them, at annotationScale, and the front ends pass them. An entry
    // with no shapes writes nothing (a label with no room at the scale).
    // Null writes those kinds as before: labels and the other dimension kinds
    // skipped with a warning, a leader as its line, landing and note.
    const std::map<katana::entity::EntityId, std::vector<katana::entity::Geometry>>* drawn =
        nullptr;
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
