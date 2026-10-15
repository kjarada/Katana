#pragma once

// Volumes on the command line (docs/volumes.md): the verbs a person, a script,
// katana_cli and an agent through katana_mcp type to measure earthworks
// between the drawing's surfaces, and the window's Terrain > Volumes builds.
//
//   VOLUME SURFACES <base> <top> [INSIDE <polygon> [EACH]] [bulk=<f>] [shrink=<f>]
//          [csv=<file> [OVERWRITE]]
//   VOLUME DATUM <surface> <level> [INSIDE <polygon> [EACH]] [bulk=] [shrink=] [csv=]
//   VOLUME STOCKPILE <surface> INSIDE <polygon> [base=boundary|lowest] [level=<z>] [EACH]
//          [bulk=] [shrink=] [csv=]
//   VOLUME SECTIONS <base> <top> ALIGNMENT <name> [interval=10] [from=] [to=] [left=]
//          [right=] [method=endarea|prismoidal|both] [curvature=on|off] [bulk=] [shrink=]
//          [csv=]
//   VOLUME GRID <base> <top> cell=<m> [INSIDE <polygon>] [bulk=] [shrink=] [csv=]
//   SURFACE DIFFERENCE <base> <top> [NAME <new>] [PREVIEW]
//   SURFACE ENVELOPE <a> <b> MAX|MIN [NAME <new>] [PREVIEW]
//
//   <polygon> := #id [#id ...] | x,y[,z] x,y[,z] x,y[,z] ... | FROM <scope> [WHERE k=v ...]
//                (read as drawing_shapes.hpp reads it; a ring tagged as a hole joins the
//                polygon it lies in, its area is taken out of it, and the polygons
//                record says holes=<how many joined>)
//
// Words are case-insensitive; a surface or alignment name with a blank is
// quoted. The key=value options are read wherever they stand - none of them
// is a WHERE key - and each is refused by name where the method takes none.
//
// THE READING OF CUT AND FILL is compareSurfaces' throughout: the first
// surface named is the existing one (the base), the second the design (the
// top); fill is where the top is above the base, cut where it is below, and
// net = fill - cut in every record (positive: material to bring in). A level
// is the design for DATUM and the base for STOCKPILE, so what stands above a
// level is cut, and a pile above its base is cut - what removes it. Factors
// are applied in the reply, never to the geometry: bulk= multiplies the cut,
// shrink= the fill, each as given, and the record adds cut.adjusted=,
// fill.adjusted= and net.adjusted=.
//
// The VOLUME verbs change nothing (csv= writes a file, refusing one already
// there unless OVERWRITE; with a scope that takes no polygon it writes nothing
// and says so). SURFACE DIFFERENCE and ENVELOPE add a surface to the drawing -
// ONE undo step - and with PREVIEW say what they would add; the difference is
// drawn in the Cut Fill style, the envelope (real levels) in the default.

#include <cstddef>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/cad/terrain/drawing_shapes.hpp"
#include "katana/core/error.hpp"

namespace katana::cad {

// True for VOLUME, and for SURFACE DIFFERENCE and SURFACE ENVELOPE (`args`
// are the words after the verb), in any case. The surfaces' own SURFACE
// words are not claimed.
[[nodiscard]] bool isVolumeVerb(std::string_view verb, const std::vector<std::string>& args);

// Runs one of them: `tokens` is the whole line split into words with the
// quotes removed (CommandInterpreter::tokenize), the verb first. `views`
// answers a FROM VIEW scope; headless it is empty and VIEW is refused, naming
// AREA. Returns the reply records, or an error saying what was refused.
[[nodiscard]] katana::core::Result<std::string>
runVolumeVerb(Document& document, const std::vector<std::string>& tokens,
              const ScopeViewProvider& views = {});

// Every form and option, for HELP VOLUME.
[[nodiscard]] std::string volumeVerbHelp();

// ---- the pieces the verbs are made of, for front ends and tests -----------------------------

// Reads a <polygon> from `words` at `at` and leaves `at` after it: one or
// more #ids, three or more typed corners x,y[,z], or FROM and a scope - the
// drawing's polygons as the surface edits read them too (drawing_shapes.hpp,
// which says what each kind of entity gives), with these verbs' refusals. A
// scope that takes no closed outline gives no polygon - said, not refused;
// an #id that is no entity is NotFound, one that is no polygon
// InvalidGeometry; fewer than three corners ParseFailure. A polygon's
// heights are what STOCKPILE base=boundary uses, and `chorded` what EACH's
// chorded=yes says.
[[nodiscard]] katana::core::Result<DrawingShapes>
readVolumePolygons(const Document& document, const std::vector<std::string>& words,
                   std::size_t& at, const ScopeViewProvider& views);

} // namespace katana::cad
