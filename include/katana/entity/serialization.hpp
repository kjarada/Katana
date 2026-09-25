#pragma once

// Text serialisation of the entity model, shared by project storage and by the
// structured command API. The format is JSON; the JSON library is an
// implementation detail and does not appear in this interface.
//
// Doubles are written with the shortest representation that parses back to the
// identical bit pattern, so save/load round trips are exact.
//
//   Point      {"type":"Point","position":[x,y]}
//   Line       {"type":"Line","start":[x,y],"end":[x,y]}
//   Arc        {"type":"Arc","center":[x,y],"radius":r,"startAngle":a,"sweep":s}
//   Polyline   {"type":"Polyline","closed":false,"vertices":[[x,y],...]}
//   Circle     {"type":"Circle","center":[x,y],"radius":r}
//   Text       {"type":"Text","position":[x,y],"text":"...","height":h,"rotation":a}
//   Dimension  {"type":"Dimension","start":[x,y],"end":[x,y],"offset":o,"textOverride":"..."}
//
// Parsing validates the geometry; malformed or invalid input yields ParseFailure
// or InvalidGeometry, never a partially filled value.

#include <string>
#include <string_view>

#include "katana/core/error.hpp"
#include "katana/entity/annotation.hpp"
#include "katana/entity/entity.hpp"

namespace katana::entity {

// The writers return Result because nlohmann's dump() THROWS on a string that
// is not valid UTF-8, and these are called from paths that return Status and
// promise not to throw. Invalid text is rejected before it ever reaches the
// model (see isValidUtf8), so a failure here means a defect rather than bad
// user input - but a save path must not be able to abort the process either
// way, so the possibility is expressed in the type.
[[nodiscard]] katana::core::Result<std::string> geometryToJson(const Geometry& geometry);
[[nodiscard]] katana::core::Result<Geometry> geometryFromJson(std::string_view json);

[[nodiscard]] katana::core::Result<std::string> propertiesToJson(const PropertyMap& properties);
[[nodiscard]] katana::core::Result<PropertyMap> propertiesFromJson(std::string_view json);

// {"id":1,"layer":"0","style":"","color":"#RRGGBB"|null,"visible":true,
//  "geometry":{...},"properties":{...},"metadata":{...}}
[[nodiscard]] katana::core::Result<std::string> entityToJson(const Entity& entity);
[[nodiscard]] katana::core::Result<Entity> entityFromJson(std::string_view json);

// A label style's definition (everything but its name and kind, which the
// project store keeps as columns) as versioned JSON: {"version": 1, ...},
// each member written only when it differs from LabelStyle's default, and
// a member left out read as that default - so a member added later needs no
// schema migration, and a style written by this build reads the same in the
// next. A version newer than this build's is refused (Unsupported) rather
// than read wrongly.
[[nodiscard]] katana::core::Result<std::string> labelStyleDefinitionToJson(const LabelStyle& style);
// Fills everything but `name` and `kind` of `style` from `json`.
[[nodiscard]] katana::core::Status labelStyleDefinitionFromJson(std::string_view json,
                                                                LabelStyle& style);

} // namespace katana::entity
