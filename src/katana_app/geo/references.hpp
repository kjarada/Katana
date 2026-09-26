#pragma once

// The reference layers a project records (docs/interop.md, "Reference
// layers"): each layer's source and display settings, kept in the drawing's
// metadata (storage::ProjectMetadata::referenceLayers) as the records
// interop/reference_data.hpp writes, and read again when it opens (REFS
// RESTORE).

#include "geo_verbs.hpp"

namespace katana::app::geo {

// The drawing's metadata made to record the reference layers as they are
// now: what both front ends do before a SAVE that has somewhere to go, as
// they record the customisation. Taken at the save, not at each change, so
// a raster imported or hidden does not by itself make the drawing ask to be
// saved - reference data was session data, and File > New after an import
// still just drops it.
void recordReferences(Context& context);

// Whether the drawing's metadata records any reference layer: an OPEN then
// runs REFS RESTORE.
[[nodiscard]] bool recordsReferences(const Context& context);

} // namespace katana::app::geo
