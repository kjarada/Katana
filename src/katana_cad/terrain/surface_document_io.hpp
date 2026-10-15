#pragma once

// The one piece of a project's surface state that the door cannot reach with
// the input it guards against (surface_document.cpp; docs/terrain.md,
// "Surfaces in the drawing"). Private to katana_cad and its tests.
//
// Document::setSurfaceStyles refuses a style set the JSON writer cannot carry
// (text that is not UTF-8) on the way in, and a set read on open passed the
// same check, so the set a save meets always passes it: the guard in the
// save is defence for the day a path forgets, and no test could hand it the
// set it refuses through Document. It is a function of plain data so that one
// can.

#include <string>

#include "katana/cad/terrain/surface_style.hpp"
#include "katana/core/error.hpp"

namespace katana::cad::detail {

// The value a project's metadata keeps the surface styles under
// (Document::writeSurfaceState): empty for the built-in set, else the set's
// JSON. A set checkSurfaceStyleSet refuses - text that is not UTF-8 is the
// case the JSON writer would otherwise turn into U+FFFD in the person's
// project, silently - is that refusal, with the style or field named, and the
// save is a Status, never an exception and never a rewritten name.
[[nodiscard]] katana::core::Result<std::string>
surfaceStylesMetadataValue(const SurfaceStyleSet& styles);

} // namespace katana::cad::detail
