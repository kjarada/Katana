#pragma once

// FORMATS (docs/interop.md, "Formats"): the formats this build of GDAL reads
// and writes, from its own registry (gis/formats.hpp), and the options each
// driver takes. The pieces the verb, the katana_formats tool and the
// katana://formats resource share, so the three cannot come to differ.
//
//   FORMATS [RASTER|VECTOR] [READ|WRITE] [<text>...] [JSON]
//   FORMATS OPTIONS <driver> [JSON]

#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include <nlohmann/json.hpp>

#include "bindings.hpp"
#include "geo_verbs.hpp"
#include "katana/gis/formats.hpp"

namespace katana::app::geo {

// The verb, its row in the verb table's I2 block (verb_table.cpp).
[[nodiscard]] katana::core::Result<Prepared> prepareFormats(Context& context, const Tokens& tokens,
                                                            std::string_view line);

struct FormatsQuery {
    std::optional<katana::gis::DataKind> kind; // RASTER or VECTOR; both when absent
    enum class Capability { Any, Read, Write };
    Capability capability = Capability::Any;
    // Words each of which the driver's name, description or an extension
    // holds, ignoring case.
    std::string text;
};

// The formats a query takes, in the registry's order (by driver name).
[[nodiscard]] std::vector<const katana::gis::Format*> chooseFormats(const FormatsQuery& query);

// format driver=GPKG kind=raster,vector read=raster,vector write=raster,vector
//        extensions=gpkg,gpkg.zip vsi=yes description=GeoPackage
[[nodiscard]] std::string formatRecord(const katana::gis::Format& format);
// listed formats=12 kind=vector capability=write filter=
[[nodiscard]] std::string formatsSummary(const FormatsQuery& query, std::size_t count);

// {driver, description, kinds:[...], read:[...], write:[...], extensions,
//  vsi, connection_prefix, help_url}
[[nodiscard]] nlohmann::json formatJson(const katana::gis::Format& format);
// {name, type, description, default, scope, choices, min, max}; absent
// members are null.
[[nodiscard]] nlohmann::json formatOptionJson(const katana::gis::FormatOption& option);

// option driver=GPKG list=open name=LIST_ALL_TABLES type=string-select
//        default=AUTO scope=vector choices=AUTO,YES,NO min= max= description="..."
[[nodiscard]] std::string optionRecord(const katana::gis::Format& format, std::string_view list,
                                       const katana::gis::FormatOption& option);
// The format's record, then its options' records: open, creation, layer
// creation - FORMATS OPTIONS <driver> without its summary.
[[nodiscard]] std::string optionsRecords(const katana::gis::Format& format,
                                         const katana::gis::FormatOptions& options);

// {driver, open_options:[...], creation_options:[...], layer_creation_options:[...]}
[[nodiscard]] nlohmann::json formatOptionsJson(const katana::gis::Format& format,
                                               const katana::gis::FormatOptions& options);

} // namespace katana::app::geo
