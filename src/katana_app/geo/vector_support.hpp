#pragma once

// What the curated vector verbs share (docs/geoprocessing.md, "V1" to "V5"):
// GIS BUFFER, DISSOLVE, OVERLAY, HULL, CLIP, CHECK, REPAIR, COVERAGE and SQL
// are thin modules over the bindings (bindings.hpp), and these are the pieces
// each of them would otherwise write again: reading a line's own words apart
// from its scope, running one vector algorithm on a feature set, measuring
// what came back, and applying a result as ONE undo step.
//
// Their own words are read the way MODIFY reads SET and PREVIEW: taken out of
// the line wherever they stand, and the rest handed to the ONE scope parser
// (cad::parseScopeWords). So options may follow a WHERE filter - "GIS BUFFER
// DRAWING WHERE TYPE=polyline distance=5" - and the filter still ends where
// its conditions do. An option key is never a WHERE key: that is why a
// result's layer is TO LAYER <path> (the one target parser's word) and not a
// layer= option, which WHERE's LAYER= would silently take for a filter.

#include <cstddef>
#include <map>
#include <optional>
#include <set>
#include <stop_token>
#include <string>
#include <string_view>
#include <vector>

#include "bindings.hpp"
#include "geo_verbs.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/commands/command.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/gis/processing.hpp"

namespace katana::app::geo::vector {

// ---- a line's own words ----------------------------------------------------------------------

// What a verb takes besides its scope.
struct WordRules {
    // Keys of key=value options, lower case: "distance", "side" ...
    std::vector<std::string> options;
    // Bare words, upper case: "PREVIEW", "REPLACE" ...
    std::vector<std::string> flags;
    // Whether TO <target> is taken.
    bool target = false;
    // The verb's usage, for the refusal of a word it does not know.
    std::string usage;
};

struct VerbWords {
    katana::cad::ScopeWords scope;
    // By lower-case key, the value as typed.
    std::map<std::string, std::string> options;
    // Upper case.
    std::set<std::string> flags;
    std::optional<Target> target;

    [[nodiscard]] bool has(std::string_view flag) const { return flags.contains(std::string(flag)); }
    [[nodiscard]] const std::string* option(std::string_view key) const;
};

// Reads words [begin, end) of `tokens`: the options, flags and TO clause
// `rules` names, wherever they stand, and the rest as ONE scope clause.
// InvalidArgument naming the word for an option or flag given twice, an
// option with no value, a TO where none is taken or given twice; the scope
// parser's refusal for its own words; and InvalidArgument with the usage for
// a word that is none of these. A word after LAYERS is always the layer list,
// so a layer may be called "preview".
[[nodiscard]] katana::core::Result<VerbWords> readVerbWords(const Tokens& tokens, std::size_t begin,
                                                            std::size_t end, const WordRules& rules);

// The option `key` as a finite number; nullopt when it was not given.
// InvalidArgument naming it when it was and does not read.
[[nodiscard]] katana::core::Result<std::optional<double>> numberOption(const VerbWords& words,
                                                                       std::string_view key);
// The option `key` as one of `choices` (any case, answered in lower case);
// `fallback` when it was not given. InvalidArgument listing the choices.
[[nodiscard]] katana::core::Result<std::string>
choiceOption(const VerbWords& words, std::string_view key, const std::vector<std::string>& choices,
             std::string_view fallback);
// "a,b,c" as its items, blanks trimmed and empty items dropped.
[[nodiscard]] std::vector<std::string> listOf(std::string_view text);

// The target of a verb that draws its result: TO LAYER <path>, else
// `fallback`. Unsupported for any other target, naming the verb.
[[nodiscard]] katana::core::Result<std::string> layerTarget(const VerbWords& words,
                                                            std::string_view verb,
                                                            std::string_view fallback);

// ---- the drawing as a feature set --------------------------------------------------------------

// The scope matched and converted with the project's coordinate system: what
// every vector verb binds. VIEW is the interpreter's (refused headless).
[[nodiscard]] katana::core::Result<BoundDrawing> bindScope(Context& context,
                                                           const katana::cad::ScopeWords& scope);

// The table named `name`, or null.
[[nodiscard]] const katana::gis::processing::FeatureTable*
tableNamed(const katana::gis::processing::FeatureSet& set, std::string_view name);
// The index of the field `name` in `table`, or nullopt.
[[nodiscard]] std::optional<std::size_t> fieldIndex(const katana::gis::processing::FeatureTable& table,
                                                    std::string_view name);
// Whether any table of `set` has the field.
[[nodiscard]] bool hasField(const katana::gis::processing::FeatureSet& set, std::string_view name);
// The entity a feature came from: its katana_id (under `field`, "katana_id"
// by default; layer-algebra writes "input_katana_id").
[[nodiscard]] std::optional<katana::entity::EntityId>
featureId(const katana::gis::processing::FeatureTable& table,
          const katana::gis::processing::Feature& feature, std::string_view field = "katana_id");

// One table of every feature of `tables`, called `name`: the fields are the
// union in first-seen order, a field whose type differs between tables
// becomes a String. How a result drawn on ONE layer is made from the points,
// lines and polygons tables an algorithm hands back.
[[nodiscard]] katana::gis::processing::FeatureTable
mergedTable(const std::vector<const katana::gis::processing::FeatureTable*>& tables,
            std::string name);

// ---- measures --------------------------------------------------------------------------------

// A ring's area, whichever way round it runs.
[[nodiscard]] double ringArea(const std::vector<katana::gis::GeoPoint>& ring);
// A polygon's area: its exterior less its holes. 0 for anything else.
[[nodiscard]] double geometryArea(const katana::gis::VectorGeometry& geometry);
// A line's length, or a polygon's perimeter (holes included); 0 for a point.
[[nodiscard]] double geometryLength(const katana::gis::VectorGeometry& geometry);
[[nodiscard]] double featureArea(const katana::gis::processing::Feature& feature);
[[nodiscard]] double featureLength(const katana::gis::processing::Feature& feature);
// Totals over a set, and how many features hold a geometry with no points
// (GEOS gives an empty polygon for a line buffered inwards).
struct Measures {
    std::size_t features = 0, empty = 0;
    double area = 0.0, length = 0.0;
};
[[nodiscard]] Measures measure(const katana::gis::processing::FeatureSet& set);
// The set without its empty geometries, and without tables left empty.
[[nodiscard]] katana::gis::processing::FeatureSet
withoutEmpty(katana::gis::processing::FeatureSet set);

// ---- running --------------------------------------------------------------------------------

// One vector algorithm on `input` (bound to its "input" argument), `tokens`
// being GDAL's own words, the output kept in memory. Its features - an empty
// set when it wrote none - and its warnings appended to `warnings`.
[[nodiscard]] katana::core::Result<katana::gis::processing::FeatureSet>
runVector(const std::vector<std::string>& path, const katana::gis::processing::FeatureSet& input,
          const std::vector<std::string>& tokens, const std::stop_token& stop,
          const Progress& progress, std::vector<std::string>& warnings,
          const std::vector<std::pair<std::string, katana::gis::processing::FeatureSet>>& more = {});

// A Progress that reports [from, to) of `progress`: steps run one after
// another share one bar.
[[nodiscard]] Progress progressSpan(const Progress& progress, double from, double to);

// ---- applying -------------------------------------------------------------------------------

// What an apply changed, for the output record.
struct Applied {
    std::size_t created = 0, updated = 0, deleted = 0, skipped = 0;
    std::vector<katana::entity::EntityId> createdIds;
};

// Executes `command` as ONE undo step (nothing for a null one), then frames
// what it created. The ids it created.
[[nodiscard]] katana::core::Result<std::vector<katana::entity::EntityId>>
executeStep(Context& context, katana::commands::CommandPtr command);

// output arg=output kind=vector target=layer layer=<layer> created= updated=
// deleted= skipped= - the record applyOutputs writes, so a curated verb's
// reply reads as the GDAL verb's does.
[[nodiscard]] std::string outputRecord(std::string_view layer, const Applied& applied);

// gis op=<op> seconds=<s> cancelled=no: a curated verb's first record.
[[nodiscard]] std::string gisRecord(std::string_view op, double seconds);
// A scope that took nothing, or a preview: said, nothing run.
[[nodiscard]] std::string notRunRecord(std::string_view op, std::string_view why);

// The records, one per line.
[[nodiscard]] std::string joined(const std::vector<std::string>& records);

// A prepared line that answers at once.
[[nodiscard]] Prepared answered(std::string title, std::string reply);

// The entities `ids` names, copied: what an in-place apply compares the
// drawing with (unchangedSince) before it changes anything.
[[nodiscard]] std::vector<katana::entity::Entity>
copiesOf(const katana::cad::Document& document, const std::vector<katana::entity::EntityId>& ids);

// A value of a feature's field as an entity property; nullopt for a null.
[[nodiscard]] std::optional<katana::entity::PropertyValue>
propertyOf(const katana::gis::processing::FieldValue& value);

// Markers of a check (GIS CHECK, GIS COVERAGE CHECK) on `layer`, replacing
// the markers of the same `kind` a previous run left there - as AUTOLABEL
// replaces its rule's labels - in ONE step: the old ones deleted, the layer
// made where missing, the new ones created, each tagged gis.marker=<kind>.
// Null when there is nothing to delete or create.
[[nodiscard]] katana::commands::CommandPtr
replaceMarkers(const katana::entity::Model& model, const std::string& layer, const std::string& kind,
               std::vector<katana::entity::Entity> markers, const std::string& commandName,
               std::size_t& removed);

} // namespace katana::app::geo::vector
