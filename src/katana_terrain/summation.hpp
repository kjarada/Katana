#pragma once

// The compensated sum used for every area and volume in this module. It is
// katana::math::CompensatedSum, aliased here so that the call sites in
// volume.cpp and tin_surface.cpp read as they always have; the implementation
// moved to the public header when the corridor quantities in cad needed the
// same one, and a second copy would have been the defect CLAUDE.md section 1
// names.

#include "katana/math/summation.hpp"

namespace katana::terrain::detail {

using CompensatedSum = katana::math::CompensatedSum;

} // namespace katana::terrain::detail
