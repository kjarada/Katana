#pragma once

// The records of the GIS verbs - IMPORT, EXPORT, INFO, REFS, COPC
// (docs/interop.md, "Replies") - that more than one verb writes, and the
// reading of any record as JSON for the MCP tools that hand them to an agent
// as structured content.
//
//   reference id=1 kind=raster name=terrain width=120 height=90 ...
//   reference id=2 kind=pointcloud name=scan points=40000 source_points=40000 ...
//   surface name=ground triangles=1200 points=640 bounds=... zmin=... zmax=... source=...

#include <string>

#include <nlohmann/json.hpp>

#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"
#include "replies.hpp"

namespace katana::app::geo {

// One reference layer as REFS and IMPORT give it.
[[nodiscard]] std::string referenceRecord(const katana::interop::RasterOverlay& raster);
[[nodiscard]] std::string referenceRecord(const katana::interop::PointCloudLayer& cloud);

// One named surface: surfaceRecord, terrain_verbs.hpp's.

// A record as a JSON object: {"record": kind, field: value ...}. The kind is
// every word before the first key= ("ifc exported"). A field is a number when
// its text is one, true or false for yes and no, an array of numbers for a
// comma list of them (bounds, always four or null; a scope's area; a cell of
// 4,3) - except the fields that are always words (a file, a name, a
// sentence), which stay strings whatever they hold: a layer called 12 is a
// name.
[[nodiscard]] nlohmann::json recordJson(const Record& record);

// Every record of a reply, as recordJson makes each.
[[nodiscard]] nlohmann::json recordsJson(std::string_view reply);

} // namespace katana::app::geo
