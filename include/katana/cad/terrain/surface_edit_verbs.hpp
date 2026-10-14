#pragma once

// Editing the drawing's surfaces on the command line (docs/surface_editing.md):
// the verbs a person, a script, katana_cli, katana_mcp and the window's own
// tools and dialogs run, so each edit has one code path.
//
//   SURFACE EDIT <name> DELETE TRIANGLE x,y
//                       DELETE TRIANGLES INSIDE|OUTSIDE <polygon>
//                       DELETE LONG <length> | DELETE THIN <degrees>
//                       DELETE POINT x,y [reach=<m>] | DELETE POINTS INSIDE <polygon>
//                       FLIP x,y
//                       ADD POINT x,y,z [x,y,z ...] | ADD POINTS <points>
//                       BREAKLINE <line>
//                       BOUNDARY <polygon> | HOLE <polygon>
//                       RAISE <dz> | LOWER <dz> | SETLEVEL <z> | SCALE <factor> [base=<z>]
//                           [INSIDE <polygon>]
//                       SMOOTH [passes=3] [weight=0.5] [INSIDE <polygon>]
//                       THIN <tolerance>
//   SURFACE MERGE <a> <b> [<c> ...] NAME <new>      (later surfaces win)
//   SURFACE PASTE <top> INTO <base> [NAME <new>]    (in place unless NAME)
//   SURFACE CLIP <name> <polygon> NAME <new>
//   SURFACE OFFSET <name> <dz> NAME <new>
//
// <polygon>, <line> and <points> are the drawing's data, given three ways:
//   #id [#id ...]                 entities by id;
//   x,y[,z] x,y[,z] ...           coordinates typed in (a polygon's three or
//                                 more corners, a line's vertices - a z is the
//                                 vertex's level, none drapes it on the surface
//                                 - or the x,y,z of points);
//   FROM <scope> [WHERE k=v ...]  what the shared scope takes (scope_verbs.hpp),
//                                 the reply saying what it took (scope= matched=).
// A polygon is a closed polyline, a closed curve polyline (its arcs chorded)
// or a circle; a line a line, polyline or curve polyline (closed ones are
// closed breaklines), its levels its `elevation`/`elevations`; points are
// what a surface is built from (cad::geo::surfaceInput): points and the
// vertices of levelled lines, a heightless one left out and counted. BOUNDARY,
// CLIP and an INSIDE take one polygon; HOLE takes each it is given in turn;
// DELETE ... INSIDE|OUTSIDE takes the union. The verb's own options come
// before FROM, since everything after FROM is the scope's.
//
// Words are case-insensitive; a name with blanks is quoted. Every edit is ONE
// undo step through Document::setSurfaces, named after it (SURFACE_EDIT_FLIP,
// SURFACE_PASTE ...); an edit that changes nothing is no step. PREVIEW,
// anywhere after the surface's name, works the edit out and changes nothing.
// The reply is records, one a line:
//
//   scope=drawing matched=3 polygons=2 skipped=1      (only for FROM)
//   surface name=ground operation=hole triangles_before=200 triangles_after=188
//       vertices_before=121 vertices_after=125 points_added=4 points_removed=0
//       relevelled=0 moved=0 held=0 crossings=4 changed=yes   [preview=yes]
//
// largest_change= follows for RAISE, SETLEVEL, SCALE, SMOOTH and THIN (the
// largest change of level; for THIN the bound on the distance from the
// original). The terrain functions behind each are tin_edit.hpp's.

#include <cstdint>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/core/error.hpp"
#include "katana/terrain/surface_store.hpp"

namespace katana::cad {

// True for SURFACE EDIT, MERGE, PASTE, CLIP and OFFSET, in any case: the
// whole line, its first two words. The other SURFACE words are other
// modules' (the surfaces module's RENAME, STYLE ...; the volumes' DIFFERENCE
// and ENVELOPE; the geoprocessing executor's LIST, FROM ...).
[[nodiscard]] bool isSurfaceEditVerb(const std::vector<std::string>& tokens);

// What a surface edit line would do, worked out and not done: the reply, and
// the drawing's surfaces after it - none when it changes nothing or the line
// says PREVIEW. `views` answers the scope word VIEW (the window's; none
// headless).
struct SurfaceEditPlan {
    std::string reply;
    std::optional<katana::terrain::SurfaceStore> surfaces;
    std::string stepName;
};
[[nodiscard]] katana::core::Result<SurfaceEditPlan>
planSurfaceEdit(const Document& document, const std::vector<std::string>& tokens,
                const ScopeViewProvider& views = {});

// Runs one line: `tokens` is the whole line split into words with the quotes
// removed (CommandInterpreter::tokenize). The plan, committed as one undo
// step. Fails, changing nothing, with
//   ParseFailure     a word the grammar does not take, naming it;
//   NotFound         a surface or an entity (#id) the drawing has not;
//   AlreadyExists    a NAME already in use;
//   InvalidArgument  an entity of the wrong kind, a scope that took none,
//                    a number out of range;
// and the terrain edit's own refusal (tin_edit.hpp), with what was asked.
[[nodiscard]] katana::core::Result<std::string>
runSurfaceEditVerb(Document& document, const std::vector<std::string>& tokens,
                   const ScopeViewProvider& views = {});

// Every word, for HELP SURFACE EDIT.
[[nodiscard]] std::string surfaceEditVerbHelp();
// Two lines for the interpreter's own HELP.
[[nodiscard]] std::string surfaceEditHelpSummary();

// ---- the surface the window's edit tools act on ------------------------------------
//
// The Surface Edit toolbar's choice, or a tool's Surface option: one for the
// program, as the edit tools' EditDefaults are, and not the drawing's - which
// surface a person is editing is not part of the drawing.

// The surface the tools are pointed at by name; empty: none chosen.
[[nodiscard]] const std::string& surfaceEditTarget();
void setSurfaceEditTarget(std::string name);

// The surface a tool edits in `document`: the chosen one when the drawing
// has it, else the first visible surface, else the first. NotFound when the
// drawing has no surface at all.
[[nodiscard]] katana::core::Result<std::string> activeEditSurface(const Document& document);

// `name` as a line writes it: quoted when it has a blank or is empty.
[[nodiscard]] std::string surfaceNameWord(std::string_view name);

} // namespace katana::cad
