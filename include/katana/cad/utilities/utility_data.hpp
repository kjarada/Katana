#pragma once

// The drawing as the schedule (docs/subsurface_utilities.md, "Drawing data"):
// what UTILITY DRAW put in the document, read back, so that every UTILITY
// verb acts on what is drawn as well as on a schedule file - by the shared
// scope and filter words (scope_verbs.hpp), as Global Modify acts.
//
// A drawn point carries its whole row of the schedule: its place along its
// line, the geometry that is its position, its level, depth and their
// reference, surface, method, uncertainties, claimed level, what it
// verifies, the path to the next point and the schema's own fields, and its
// line's attributes (utility_drawing.hpp, keys). So the POINTS ARE THE
// SCHEDULE: a point moved in CAD, or a method or a level edited in the
// property panel, is the schedule edited. The runs are derived output -
// UTILITY REGRADE draws them again from the points.
//
// A scope that takes part of a line takes the WHOLE line, because the
// grading is per line: a run of it, one of its points, or anything else
// carrying its id brings in every point of it in the drawing, and the reply
// says how many lines were completed so. What carries no utility data is
// ignored, and counted. A line whose points do not agree - two at one place
// along it, one missing its place or its method, two that give its owner
// differently - is refused by name: a guess at which point is right would be
// a schedule nobody wrote.

#include <cstddef>
#include <optional>
#include <span>
#include <string>
#include <vector>

#include "katana/cad/utilities/utility_drawing.hpp"
#include "katana/commands/command.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/model.hpp"
#include "katana/entity/tables.hpp"
#include "katana/survey/subsurface/clearance.hpp"
#include "katana/survey/subsurface/utility_network.hpp"

namespace katana::cad::utilities {

// One service as the drawing holds it.
struct DrawnService {
    katana::survey::subsurface::UtilityLine line;
    // Its points in order along it: line.vertices[i] was read from points[i].
    std::vector<katana::entity::EntityId> points;
    // Its runs: the polylines carrying its id, ascending.
    std::vector<katana::entity::EntityId> runs;
    // The scope took some, not all, of its points - or none, only a run.
    bool completed = false;
    // The settings it was graded with when drawn (keys::kSpacing,
    // keys::kMinimumCover); empty when its points do not say.
    std::optional<double> spacing{};
    std::optional<double> minimumCover{};
};

struct UtilityData {
    // In the order drawn: by the id of each service's first point, which is
    // schedule order for services one UTILITY DRAW made.
    std::vector<DrawnService> services;
    // What the scope took that carries no utility data.
    std::size_t ignored = 0;
    // Services the scope took part of, read whole.
    std::size_t completed = 0;

    [[nodiscard]] std::vector<katana::survey::subsurface::UtilityLine> lines() const;
};

// The services any entity of `matched` belongs to (its utility.line), each
// read whole from its points in the whole of `model`. InvalidArgument naming
// the line, and the point by its vertex id and entity id, for: a service
// with no points left; a point with no place along the line, or a place
// another point of the line has; a point with no vertex id or one another
// point has; a point with no method, or a method, level reference, claimed
// level or path that does not read; a number that is not a finite number, a
// depth above the surface; a point whose line attributes differ from its
// line's first point's, or lack one it has. And, naming the point and the
// lines whose attributes it carries, for a point that is a vertex of a
// schedule (it has a utility.vertex or a utility.order) but no utility.line,
// or an empty one - a line deleted by hand: in `matched`, or anywhere when
// it carries the attributes of a line `matched` takes, which read without it
// would be read short of a vertex.
[[nodiscard]] katana::core::Result<UtilityData>
readUtilityData(const katana::entity::Model& model,
                std::span<const katana::entity::EntityId> matched);

// The keys a UTILITY reply adds to the scope record (scopeRecord):
// " lines=4 completed=1 ignored=3".
[[nodiscard]] std::string utilityDataKeys(const UtilityData& data);

// ---- REGRADE ------------------------------------------------------------------------------

struct UtilityRegrade {
    // What the services grade as now, drawn: for the reply, whose first
    // record carries the bounds a front end frames.
    UtilityDrawing drawing;
    // Services whose runs or points change.
    std::size_t changed = 0;
    // ONE step named kUtilityRegradeStep: each changed service's runs
    // removed and drawn again, its points' graded properties (utility.*)
    // replaced and anything else on them - a person's own properties, style,
    // colour - left as it is, and the layers and linetypes a new run needs.
    // A point on its type's points layer under the service's prefix follows
    // an edited type to that type's layer; a point a person put elsewhere
    // stays there. Null when nothing would change, so no empty undo step is
    // pushed.
    katana::commands::CommandPtr command;
};

// What UTILITY REGRADE was told. A setting given grades every service with
// it; one left out grades each service with the one it was drawn with
// (DrawnService::spacing, minimumCover), else the default - so a regrade of
// what nobody edited changes nothing, whatever SPACING and MINCOVER it was
// drawn with, and a regrade after one edit changes only what the edit does.
struct UtilityRegradeOptions {
    std::optional<double> spacing{};
    std::optional<double> minimumCover{};
    // The prefix a service is drawn under when neither its points' layer nor
    // its runs' says one.
    std::string layerPrefix{kDefaultUtilityLayerPrefix};
};

// The services of `data` graded again from their points with `options`. Each
// service is drawn under the prefix it was drawn under - read from its
// points' layer "<prefix>/<type>/points", else its runs' "<prefix>/<type>/
// QL-X" - and under options.layerPrefix when neither says. All or nothing: a
// service that cannot be graded refuses the whole regrade by name.
[[nodiscard]] katana::core::Result<UtilityRegrade>
planUtilityRegrade(const katana::entity::Model& model, const UtilityData& data,
                   const UtilityRegradeOptions& options = {});

// ---- services from surveyed or imported geometry -----------------------------------------
//
// What UTILITY DRAW <scope> reads (docs/subsurface_utilities.md, "Services from
// surveyed and imported geometry"): the located services as a survey or an
// import left them in the drawing - a coded survey's strings, a .12da archive's,
// a DXF's, a shapefile's or an IFC file's lines - rather than as a schedule
// file. Each line or open polyline in scope is one service, its vertices the
// located vertices in order; a point in scope on a vertex gives that vertex
// its point number and anything it says of itself. What the geometry cannot
// say - how it was located, to what uncertainty, what kind of service - comes
// from utility.* properties on the entities (keys, utility_drawing.hpp: set by
// MODIFY ... SET PROP=, the property panel, or an import's own attributes of
// those names) and, for what they leave unsaid, from the verb's options.

// What the heights of the geometry are the heights of.
enum class GeometryHeights {
    // The ground over the service: a detection marked on the surface and shot
    // there, which is how most located services are surveyed. The default,
    // because it errs the safe way: a service level read as the surface
    // leaves the vertex without a level - QL-B at best, cover not computed -
    // where a surface read as the service would claim a measured level at
    // ground and a cover of nothing.
    Surface,
    // The service itself, on its level reference: shot on the pipe in a
    // pothole, or a design or as-constructed model's centre line.
    Service,
    // Neither: the heights are not to be used.
    Unused,
};

// "surface", "service", "none": the word HEIGHTS and keys::kHeights take.
[[nodiscard]] std::string_view geometryHeightsWord(GeometryHeights heights);
// Those words in any case; nullopt for anything else.
[[nodiscard]] std::optional<GeometryHeights> parseGeometryHeights(std::string_view text);

// What UTILITY DRAW <scope> says for every service and vertex that does not
// say it itself. Nothing given is nothing assumed: no method refuses the
// vertex that has none, no uncertainty is "not assessed", no type is Unknown.
struct GeometryServiceOptions {
    std::optional<katana::survey::subsurface::UtilityType> type{};
    std::optional<katana::survey::subsurface::LocationMethod> method{};
    std::optional<double> horizontalUncertainty{};
    std::optional<double> verticalUncertainty{};
    std::optional<katana::survey::subsurface::LevelReference> levelReference{};
    // What is known between each vertex and the next.
    std::optional<katana::survey::subsurface::PathEvidence> path{};
    std::string owner{};
    std::string material{};
    std::optional<double> diameter{}; // metres
    std::optional<katana::survey::subsurface::UtilityStatus> status{};
    GeometryHeights heights = GeometryHeights::Surface;
    // An import's own attributes read as schedule columns, {column, property}:
    // {"type", "ASSET_TYPE"} reads an entity's ASSET_TYPE as its type, as a
    // schedule's type column is read. The column by its schedule name or an
    // alias (subsurface::utilityCsvColumnNamed); "line" and "point" name the
    // service and the vertex. The entity's own utility.* property, where it
    // has one, wins over a field read so: it is what a person set here.
    std::vector<std::pair<std::string, std::string>> fields{};
};

struct GeometryServices {
    // In the order drawn: by the id of the entity each was read from.
    std::vector<katana::survey::subsurface::UtilityLine> lines;
    // lines[i] was read from sources[i].
    std::vector<katana::entity::EntityId> sources;
    // Points in scope that gave a vertex its point number and values.
    std::size_t points = 0;
    // Points in scope on no vertex of a line taken: not a service on their own.
    std::size_t loose = 0;
    // In scope and drawn already: a drawn service's run or point, or an entity
    // a service was drawn from before (keys::kSource).
    std::size_t drawn = 0;
    // What cannot be a service: a closed polyline (an outline - a pit, a
    // building, a parcel - not a run), an arc, a circle, a text, a label ...
    std::size_t ignored = 0;
};

// The services the lines and open polylines of `matched` are, each read whole
// from the entity; the points of `matched` supply their vertices.
//
// A service's id is the entity's name - a field that names it (FIELDS
// line=), else the first code or name codePropertyCandidates() finds on it
// (survey_coding.hpp), a coded survey's string name or a .12da archive
// string's - else "#<entity id>"; a name two entities in scope share, or a
// service drawn already has, is told apart as "<name>#<entity id>". A
// vertex's id is the point number (FIELDS point=, else the survey import's
// "point" property) of the point on it, else "<line id>-<n>".
//
// Each value is the most particular said: the point's own utility.*
// property, then the line entity's, then `options`; and a height - the
// line's at the vertex, else its point's - is a surface level or a service
// level as the point's keys::kHeights, else the line's, else options.heights
// says, where the point does not give that level itself. The line's
// attributes (type, owner, material, diameter, status, configuration,
// description, and a delivery schema's line fields) come from the line
// entity and `options`, never from a point.
//
// A line entity's depth (keys::kDepth, or a depth field) is every vertex's:
// a record of a main laid at 0.9 m says so of all of it.
//
// InvalidArgument for a field whose column the schedule does not know, or
// which is where the geometry already is (easting, northing) or is read only
// from a schedule (size); and, naming the line and the entity: two points on
// one vertex (within tolerance::kCoordinate); a vertex no point, line or
// option gives a method; a value on an entity that does not read, as
// readUtilityData would refuse it on a drawn point; a vertex id twice in one
// line; a point that is a vertex of a schedule but has no line
// (readUtilityData's stray point).
[[nodiscard]] katana::core::Result<GeometryServices>
readGeometryServices(const katana::entity::Model& model,
                     std::span<const katana::entity::EntityId> matched,
                     const GeometryServiceOptions& options = {});

// The keys a UTILITY DRAW <scope> reply adds to the scope record
// (scopeRecord): " lines=2 points=9 loose=1 drawn=0 ignored=3".
[[nodiscard]] std::string geometryServiceKeys(const GeometryServices& services);

// The drawing of `services`, as drawUtilities draws a schedule, each point
// carrying the entity its line was read from (keys::kSource).
[[nodiscard]] katana::core::Result<UtilityDrawing>
drawGeometryServices(const GeometryServices& services, const UtilityDrawOptions& options = {});

// ---- the design CLEARANCE is measured against ----------------------------------------------

// Proposed works drawn as an entity: a line, a polyline (a closed one
// returns to its start) or a curve polyline (its arcs as chords within a
// millimetre). Levels: `level` at every vertex when given; else the
// entity's own heights (entity::heightsOf, or a curve polyline's vertices,
// interpolated along a segment), where it has them; else none.
// The id is "#<entity id>". InvalidArgument for any other kind of entity.
[[nodiscard]] katana::core::Result<katana::survey::subsurface::DesignAlignment>
designFromEntity(const katana::entity::Entity& entity, std::optional<double> level);

// Within a millimetre: the clearance report's figures are to the millimetre.
inline constexpr double kDesignChordTolerance = 0.001;

// Proposed works along a document alignment: its horizontal geometry,
// chorded to `tolerance` (geometry::sagittaChordCount's rule), and levels
// from its design profile where it has one - sampled at every key station of
// the profile and closely enough inside each vertical curve that the
// straight lines between samples stay within `tolerance` of the parabola.
// Stations the profile does not reach have no level. The id is the
// alignment's name. Fails with whatever solveAlignment or solveProfile
// reports.
[[nodiscard]] katana::core::Result<katana::survey::subsurface::DesignAlignment>
designFromAlignment(const katana::entity::Alignment& alignment,
                    double tolerance = kDesignChordTolerance);

// The stations designFromAlignment samples, ascending, one per vertex: for a
// test to hold each chord to the curve it stands for.
[[nodiscard]] katana::core::Result<std::vector<double>>
designStations(const katana::entity::Alignment& alignment,
               double tolerance = kDesignChordTolerance);

} // namespace katana::cad::utilities
