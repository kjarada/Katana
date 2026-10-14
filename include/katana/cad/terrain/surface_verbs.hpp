#pragma once

// The drawing's surfaces and their display styles on the command line
// (docs/terrain.md, "Surfaces in the drawing"): the verbs a person, a script,
// katana_cli and an agent type to rename, copy, show, hide and restyle a
// surface, and to make and edit the styles it is drawn in.
//
//   SURFACE RENAME <name> <new>
//   SURFACE STYLE <name> <style>          ("" for the drawing's default style)
//   SURFACE SHOW <name>... | ALL
//   SURFACE HIDE <name>... | ALL
//   SURFACE COPY <name> <new>
//   SURFACESTYLE LIST [JSON] | INFO <style> | NEW <style> [FROM <style>] [key=value ...]
//              | SET <style> key=value ... | DELETE <style> | RENAME <style> <new>
//              | DEFAULT <style>
//
// Words are case-insensitive; a name with blanks is quoted. Every change is
// ONE undo step through Document::setSurfaces / setSurfaceStyles /
// setSurfacesAndStyles, a refused one changes nothing, and a reply is
// key=value records, one a line. The geoprocessing executor keeps the other
// SURFACE words that are its (LIST, INFO, REMOVE, FROM, EXPORT; docs/
// geoprocessing.md), and the surface edits and volumes theirs; this module
// claims only the words above (isSurfaceVerb).

#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/terrain/surface_style.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/colour_names.hpp"

namespace katana::cad {

// True for a line this module runs: SURFACESTYLE, and SURFACE followed by
// RENAME, STYLE, SHOW, HIDE or COPY. `tokens` is the whole line split into
// words (CommandInterpreter::tokenize), the verb first.
[[nodiscard]] bool isSurfaceVerb(const std::vector<std::string>& tokens);

// Runs one of them on `document`. The reply, or an error saying what was
// refused and why.
[[nodiscard]] katana::core::Result<std::string>
runSurfaceVerb(Document& document, const std::vector<std::string>& tokens);

// One surface as every reply that names one writes it - SURFACE LIST and
// INFO, SURFACE FROM, an IMPORT that brings surfaces, and the verbs here -
// with heights and bounds to the millimetre and the style it is drawn in
// (styleOf):
//   surface name=ground triangles=2 points=4 bounds=0.000,0.000,10.000,10.000
//           zmin=100.000 zmax=101.000 source="drawing (4 entities)" style=Standard
//           visible=yes
[[nodiscard]] std::string surfaceRecord(const katana::terrain::NamedSurface& surface,
                                        const SurfaceStyleSet& styles);

// For an importer that brings surfaces with its entities (an IFC file; the
// geoprocessing IMPORT of an archive does the same through its staging
// store): `command` - the import's entities, which may be null - and
// `surfaces` as ONE undo step, named after the command, or `stepName` when
// there is none. Each surface joins the drawing's under its own name, or
// "name (2)" when that is taken. The surfaces' records (surfaceRecord), in
// their order; on a refusal nothing changes.
[[nodiscard]] katana::core::Result<std::vector<std::string>>
importSurfaces(Document& document, katana::commands::CommandPtr command,
               std::vector<katana::terrain::NamedSurface> surfaces, std::string stepName);

// Every word and key, for HELP SURFACESTYLE.
[[nodiscard]] std::string surfaceVerbHelp();
// The lines the general HELP gives these verbs.
[[nodiscard]] std::string surfaceVerbSummary();

// ---- a style's fields as key=value, for the verbs and the window's style manager ----

// Sets the field `key` names (lower case, as SURFACESTYLE NEW and SET take
// it) from `value`: description=, triangles=on|off, trianglecolour=,
// boundary=, boundarycolour=, points=, pointcolour=, breaklines=,
// breaklinecolour=, faces=none|flat|elevation|slope|aspect, opacity=0..1,
// contours=on|off, interval=, major=, base=, minorcolour=, majorcolour=,
// labels=on|off, ramp=<name>, banded=on|off, bands=, stops=v:colour;v:colour
// ... (empty clears), min=, max= (empty clears), flatcolour=,
// shading=automatic|shaded|edges|wireframe|hidden,
// faces3d=flat|elevation|slope|aspect. A colour is #RRGGBB, #RRGGBBAA or a
// colour name `colours` or the standard names know. InvalidArgument naming
// the key for a key there is none of or a value it cannot take; the style is
// left as it was. Whether the style as a whole can be drawn is
// checkSurfaceStyle's to say.
[[nodiscard]] katana::core::Status
setSurfaceStyleOption(SurfaceDisplayStyle& style, std::string_view key, std::string_view value,
                      const katana::entity::ColourTable& colours = {});

// Every field of `style` as the key=value pairs setSurfaceStyleOption takes,
// in the order above: what SURFACESTYLE INFO prints, and typed back after
// SURFACESTYLE SET <style> gives the same style.
[[nodiscard]] std::vector<std::pair<std::string, std::string>>
surfaceStyleOptions(const SurfaceDisplayStyle& style);

} // namespace katana::cad
