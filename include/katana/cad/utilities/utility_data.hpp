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
// line's first point's, or lack one it has.
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

// The services of `data` graded again from their points with `options`. Each
// service is drawn under the prefix it was drawn under - read from its
// points' layer "<prefix>/<type>/points", else its runs' "<prefix>/<type>/
// QL-X" - and under options.layerPrefix when neither says. All or nothing: a
// service that cannot be graded refuses the whole regrade by name.
[[nodiscard]] katana::core::Result<UtilityRegrade>
planUtilityRegrade(const katana::entity::Model& model, const UtilityData& data,
                   const UtilityDrawOptions& options = {});

// ---- the design CLEARANCE is measured against ----------------------------------------------

// Proposed works drawn as an entity: a line or a polyline (a closed one
// returns to its start). Levels: `level` at every vertex when given; else the
// entity's own heights (entity::heightsOf), where it has them; else none.
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
