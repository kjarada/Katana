#pragma once

// GDAL's algorithms and their arguments as JSON (docs/geoprocessing.md,
// "Schemas"): what GDAL HELP ... JSON prints, what katana_gdal_catalogue and
// katana_gdal_describe return, and the JSON Schema an agent fills
// katana_gdal_run's arguments by. Generated from the argument specs GDAL
// declares at run time, never written by hand, so a GDAL upgrade that adds an
// argument or moves a bound changes the schema with it.

#include <nlohmann/json.hpp>

#include "katana/gis/processing.hpp"

namespace katana::app::geo {

// {path:[...], name, description, aliases, policy, url, container}
[[nodiscard]] nlohmann::json algorithmJson(const katana::gis::processing::AlgorithmInfo& info);

// {name, short_name, aliases, type, required, positional, category, default,
//  choices, min:{value,inclusive}, max:{...}, count:{min,max}, dataset:{kinds,
//  accepts, sources}, depends_on, exclusion_group, dependency_group, input,
//  output, description}; absent members are null.
[[nodiscard]] nlohmann::json argumentJson(const katana::gis::processing::ArgSpec& arg);

// The JSON Schema of katana_gdal_run's `arguments`: one property per argument
// that is not a dataset, typed, with GDAL's choices as an enum, its bounds as
// minimum/exclusiveMinimum and maximum/exclusiveMaximum, list counts as
// minItems/maxItems, and its default.
[[nodiscard]] nlohmann::json argumentsSchema(const katana::gis::processing::AlgorithmSpec& spec);

// The JSON Schema of katana_gdal_run's `inputs`: one property per input
// dataset argument, a Source object or, for a list, an array of them.
[[nodiscard]] nlohmann::json inputsSchema(const katana::gis::processing::AlgorithmSpec& spec);

// The JSON Schema of one Source: {scope, area, layers, only, where} |
// {raster} | {surface, cell} | {file, layer}.
[[nodiscard]] nlohmann::json sourceSchema();

// {algorithm:{...}, arguments:[...], arguments_schema, inputs_schema,
//  gdal_usage}: katana_gdal_describe's answer and GDAL HELP ... JSON.
[[nodiscard]] nlohmann::json describeJson(const katana::gis::processing::AlgorithmSpec& spec);

} // namespace katana::app::geo
