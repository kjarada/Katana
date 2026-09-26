#pragma once

// Drawing data to and from the tables GDAL's algorithms read and write
// (docs/geoprocessing.md, "Bindings"): the ONE conversion between entities
// and features. What a scope took becomes a FeatureSet an algorithm reads; the
// features an algorithm returns become ONE undoable command on the drawing.
//
// Entities to features (drawingDataset):
//   - three tables, `points`, `lines` and `polygons`, an empty one omitted;
//   - fields: `katana_id` first (Integer64, the key a result is joined back
//     by), then `layer`, `style`, `colour` and `type`, then the entities'
//     properties, typed and sorted by key - a key whose type differs between
//     entities becomes a String, and says so;
//   - a closed polyline or a circle is an area (closedAsPolygons). Nested
//     closed polylines are NOT holes - a building inside a lot is not a hole
//     in the lot - only a ring tagged `source.ring=hole` (what IMPORT writes)
//     or `gis.ring=hole` (what a result writes) joins the exterior it lies in,
//     the one with its `gis.part` (`source.part`, IMPORT's) when it has one;
//   - arcs and circles are chords within curveTolerance (the export's 1 mm
//     sagitta by default);
//   - Z only when every vertex has a height (absent is not zero); with
//     requireHeights a heightless entity is left out and counted;
//   - text, dimensions, labels and leaders have no feature; they are left out
//     and counted by kind;
//   - every table carries the project's coordinate system.
//
// Features to the drawing (resultCommand): one commands::Transaction.
//   Create          entities on the target layer (a table of several goes to
//                   <layer>/<table>), layers made where missing; typed fields
//                   become typed properties, with gis.op=<algorithm> and
//                   gis.source=<katana_id> when a feature carries one; a
//                   polygon's rings are closed polylines tagged gis.ring and
//                   gis.part. No area or length is written: a measure kept on
//                   an entity goes stale when it is edited, so replies carry
//                   measures instead.
//   UpdateGeometry  the geometry of the entity each katana_id names; its id,
//                   and so its labels and associations, are kept.
//   SetProperties   the fields as properties of the entity each katana_id names.
//   deleteSources   the entities the features came from are deleted (REPLACE).

#include <cstddef>
#include <map>
#include <optional>
#include <string>
#include <vector>

#include "katana/commands/command.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/model.hpp"
#include "katana/gis/processing.hpp"

namespace katana::interop::geo {

struct DrawingDatasetOptions {
    // The largest distance a chord may stray from an arc or a circle, in model
    // units.
    double curveTolerance = 0.001;
    // Leave out, and count as "heightless", an entity without a height at
    // every vertex: what an algorithm that reads Z (gridding, rasterize --3d)
    // must be given, since a missing height read as 0 is a wrong surface.
    bool requireHeights = false;
    bool closedAsPolygons = true;
    std::string crsWkt; // the project's coordinate system; empty for none
    // Every feature in ONE table, `tableName`, whatever its kind: a file's
    // layer that holds points, lines and areas together (EXPORT). Its kind
    // is theirs when they share one, else Unknown.
    bool oneTable = false;
    std::string tableName = "features";
    // `style`, `colour` and `type` after `katana_id` and `layer`: what an
    // algorithm's result is filtered and styled by. EXPORT leaves them out -
    // a file handed to someone else carries each entity's id and layer, as it
    // always has, and the drawing's styling stays the drawing's.
    bool styleFields = true;
    // The entities' properties as fields, after those.
    bool properties = true;
};

struct DrawingDatasetStats {
    std::size_t matched = 0; // the ids asked for
    std::size_t used = 0;    // entities that went into a feature
    std::size_t points = 0, lines = 0, polygons = 0; // features written
    std::map<std::string, std::size_t> skipped; // reason -> count
    std::vector<std::string> warnings;
};

struct DrawingDataset {
    katana::gis::processing::FeatureSet set;
    DrawingDatasetStats stats;
};

// The entities `ids` names, in that order, as tables. An id the model does
// not hold is counted as skipped "missing". Never fails for what the scope
// took; InvalidArgument for a curve tolerance that is not positive.
[[nodiscard]] katana::core::Result<DrawingDataset>
drawingDataset(const katana::entity::Model& model, const std::vector<katana::entity::EntityId>& ids,
               const DrawingDatasetOptions& options = {});

enum class ResultMode { Create, UpdateGeometry, SetProperties };

struct ResultOptions {
    // Create: where the entities go. Several tables go to <layer>/<table>.
    std::string targetLayer;
    ResultMode mode = ResultMode::Create;
    // gis.op on what is created: the algorithm, "raster contour".
    std::string operation;
    // Put before each field's name when it becomes a property ("zone." ->
    // zone.mean); empty for none.
    std::string propertyPrefix;
    // Delete the entities the features came from (gis.source): REPLACE.
    bool deleteSources = false;
    // The transaction's name: the verb line, so the history says what ran.
    std::string commandName;
};

struct ResultPlan {
    // nullptr when there is nothing to do: an empty transaction is refused
    // by the command stack, and a result of nothing changes nothing.
    katana::commands::CommandPtr command;
    std::size_t created = 0, updated = 0, deleted = 0, skipped = 0;
    std::vector<std::string> layers;   // the layers written to, in first-use order
    std::vector<std::string> warnings; // what could not be carried, said
};

// The one command that applies `set` to `model` as `options` asks. It reads
// the model and changes nothing: the caller executes the command, which is
// how a result becomes one undo step. InvalidArgument for a target layer that
// is not a valid layer path, or a mode that needs katana_id on a table
// without one.
[[nodiscard]] katana::core::Result<ResultPlan>
resultCommand(const katana::entity::Model& model, const katana::gis::processing::FeatureSet& set,
              const ResultOptions& options);

// The field names drawingDataset writes before the properties, which a
// result does not turn back into properties: katana_id, layer, style,
// colour, type.
[[nodiscard]] bool isBookkeepingField(const std::string& name);

// One feature geometry as the entity geometries it becomes: the ONE way a
// feature becomes entities, for a result (resultCommand) and for IMPORT
// alike (docs/interop.md, "Conversion").
//   - a point is a point;
//   - a line of two points is a Line, a longer one a polyline, closed when
//     it ends where it began at the same height (a DXF circle and a closed
//     LWPOLYLINE arrive so);
//   - a curve that is one arc is an Arc, and one that is a whole circle a
//     Circle; any other curve is chords within `curveTolerance`, by the rule
//     EXPORT makes its chords by, and counted;
//   - an area is its rings, each a closed polyline, `ring` saying which is
//     the exterior and which a hole (a circle alone is a Circle).
// Consecutive repeated points are dropped (a zero-length segment is one the
// model refuses), and a height that goes with one is counted as lost.
struct FeaturePiece {
    katana::entity::Geometry geometry;
    // One per vertex; an arc's two ends; a circle's one.
    std::vector<std::optional<double>> heights;
    std::optional<std::string> ring; // "exterior" or "hole", for an area's rings
};

struct FeaturePieceOptions {
    // Subtracted from every coordinate: IMPORT's local origin.
    std::optional<katana::geometry::Vec2> originShift;
    double curveTolerance = 0.001;
};

struct FeaturePieceCounts {
    std::size_t degenerate = 0;  // geometries with too few distinct points to draw
    std::size_t heightsLost = 0; // points dropped on the one before them in plan, at another height
    std::size_t curvesMadeChords = 0;
};

[[nodiscard]] std::vector<FeaturePiece> featurePieces(const katana::gis::VectorGeometry& geometry,
                                                      const FeaturePieceOptions& options,
                                                      FeaturePieceCounts& counts);

} // namespace katana::interop::geo
