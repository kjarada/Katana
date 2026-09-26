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
// Whether a line of the verb is the executor's at all: a verb shared with the
// interpreter answers false for the lines that are the interpreter's.
using TakesLine = bool (*)(const Tokens& tokens);

struct VerbEntry {
    const char* verb;    // upper case
    const char* subverb; // upper case; "" takes any second word
    PrepareVerb prepare;
    const char* usage;   // for helpText: "VERB ...  what it does"
    // Null takes every line of the verb. INFO's leaves INFO <id>, an entity,
    // to the interpreter.
    TakesLine takes = nullptr;
};

[[nodiscard]] const std::vector<VerbEntry>& verbTable();

// ---- F0: the GDAL verb (gdal_verbs.cpp) ----
[[nodiscard]] katana::core::Result<Prepared> prepareGdal(Context& context, const Tokens& tokens,
                                                         std::string_view line);
[[nodiscard]] std::string gdalUsage();

// ---- I0: IMPORT, EXPORT, INFO, REFS, COPC (import_verb.cpp ... copc_verb.cpp) ----
[[nodiscard]] katana::core::Result<Prepared> prepareImport(Context& context, const Tokens& tokens,
                                                           std::string_view line);
[[nodiscard]] katana::core::Result<Prepared> prepareExport(Context& context, const Tokens& tokens,
                                                           std::string_view line);
[[nodiscard]] katana::core::Result<Prepared> prepareInfo(Context& context, const Tokens& tokens,
                                                         std::string_view line);
[[nodiscard]] bool takesInfo(const Tokens& tokens);
[[nodiscard]] katana::core::Result<Prepared> prepareRefs(Context& context, const Tokens& tokens,
                                                         std::string_view line);
[[nodiscard]] katana::core::Result<Prepared> prepareCopc(Context& context, const Tokens& tokens,
                                                         std::string_view line);

} // namespace katana::app::geo
