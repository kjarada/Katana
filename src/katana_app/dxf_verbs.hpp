#pragma once

// IMPORT and EXPORT of a .dxf, natively (katana_dxf, docs/dxf.md). Here
// rather than behind the interoperability guard because reading a DXF needs
// no third-party library: a build without GDAL still has it.
//
// Split as the geoprocessing executor runs a line (geo/geo_verbs.hpp): the
// file read or written by a pure step, then what it brought applied to the
// drawing as ONE undo step with the reply records (import_records.hpp). With
// GDAL, IMPORT and EXPORT are the executor's (geo/import_verb.cpp,
// geo/export_verb.cpp), which run these steps for a .dxf - as a job in the
// window; without it, runDxfVerb runs them back to back. One DXF import and
// one DXF export, whichever build and front end asks.

#include <filesystem>
#include <optional>
#include <string>
#include <string_view>

#include "katana/cad/document.hpp"
#include "katana/cad/import_placement.hpp"
#include "katana/core/error.hpp"
#include "katana/dxf/reader.hpp"
#include "katana/dxf/writer.hpp"
#include "katana/entity/model.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::app {

// Reads `file` with `shift` subtracted from every plan coordinate
// (cad/import_placement.hpp: the file is read again with the shift, so the
// one reader moves every kind of geometry alike). Pure: no drawing is seen.
[[nodiscard]] katana::core::Result<katana::dxf::DxfImport>
readDxfImport(const std::filesystem::path& file, std::optional<katana::geometry::Vec2> shift = {});

// The linetypes, layers and entities `imported` brings as ONE undo step, and
// the records: imported, then placed (when `placement` is not Keep), a tally
// per entity kind, the reader's warnings.
[[nodiscard]] katana::core::Result<std::string>
applyDxfImport(katana::cad::Document& document, katana::dxf::DxfImport&& imported,
               const std::filesystem::path& file, const katana::cad::ImportPlacement& placement,
               const katana::cad::ImportShift& placed);

// Writes `model` to `file`, its annotation drawn at `annotationScale` as the
// plan view draws it (cad/annotation/export_annotation.hpp). Pure: reads only
// `model`, so a front end may hand it a copy of its drawing to write on
// another thread.
[[nodiscard]] katana::core::Result<katana::dxf::DxfExport>
writeDxfExport(const katana::entity::Model& model, double annotationScale,
               const std::filesystem::path& file);

// The records of a DXF written: exported, then the writer's warnings.
// `file` is the name it is known by, which a staged write is not written to.
[[nodiscard]] std::string dxfExportRecords(const katana::dxf::DxfExport& written,
                                           const std::filesystem::path& file);

// The build without GDAL's IMPORT and EXPORT of a .dxf: `verb` is IMPORT or
// EXPORT, upper case; `path` the file and `placement` where an IMPORT puts
// what it reads, as CommandInterpreter::importArgument reads the line.
// nullopt when the path is not a .dxf, so the caller carries on; otherwise
// whether it worked, the records on stdout or the refusal on stderr.
[[nodiscard]] std::optional<bool> runDxfVerb(katana::cad::Document& document,
                                             std::string_view verb, const std::string& path,
                                             const katana::cad::ImportPlacement& placement);

} // namespace katana::app
