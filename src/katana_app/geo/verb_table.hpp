#pragma once

// The geoprocessing verbs the executor runs (docs/geoprocessing.md, "The verb
// table"): each a verb, an optional second word, the function that prepares
// its line and its usage. handles(), prepare() and helpText() read this table
// alone, so a verb is added by adding its row - HELP, the window's Command
// Reference and katana_help then name it with no other change.

#include <string>
#include <string_view>
#include <vector>

#include "bindings.hpp"
#include "geo_verbs.hpp"

namespace katana::app::geo {

using PrepareVerb = katana::core::Result<Prepared> (*)(Context& context, const Tokens& tokens,
                                                        std::string_view line);

struct VerbEntry {
    const char* verb;    // upper case
    const char* subverb; // upper case; "" takes any second word
    PrepareVerb prepare;
    const char* usage;   // for helpText: "VERB ...  what it does"
};

[[nodiscard]] const std::vector<VerbEntry>& verbTable();

// ---- F0: the GDAL verb (gdal_verbs.cpp) ----
[[nodiscard]] katana::core::Result<Prepared> prepareGdal(Context& context, const Tokens& tokens,
                                                         std::string_view line);
[[nodiscard]] std::string gdalUsage();

} // namespace katana::app::geo
