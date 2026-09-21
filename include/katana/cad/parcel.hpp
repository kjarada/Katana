#pragma once

// Parcels: the courses of a boundary as bearings and distances, its area, and
// the words a title document carries (PLAN.MD Phase 21).
//
// A parcel here is any closed polyline in the drawing. NOTHING IS STORED: the
// report is computed from the geometry on demand, so it can never disagree
// with the boundary it describes, and there is no second table to keep in
// step when a corner is moved. Labels, when asked for, are ordinary text
// entities the user owns afterwards - they are a snapshot, and say so by
// being editable like any other text.
//
// Conventions, all inherited rather than invented here:
//   * x is easting and y is northing, as everywhere in the drawing; the
//     survey layer's Coordinate2 is (northing, easting), and the conversion
//     happens in exactly one place, in parcel.cpp.
//   * Azimuths are clockwise from grid north, from survey::inverse.
//   * Bearings are the quadrant form surveyors read, N 45°30'15" E, from
//     survey::formatBearing, to whole seconds - the precision a deed quotes.
//   * Distances are horizontal, in model units, to three decimals.

#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad {

struct ParcelCourse {
    geometry::Point2 from;
    geometry::Point2 to;
    double azimuth = 0.0;  // radians clockwise from north, [0, 2 pi)
    double distance = 0.0; // model units
    std::string bearing;   // quadrant form, whole seconds
};

struct ParcelReport {
    std::vector<ParcelCourse> courses; // in boundary order, closing course last
    double area = 0.0;                 // square model units, always >= 0
    double perimeter = 0.0;
    geometry::Point2 centroid;
    bool clockwise = false; // the order the boundary was drawn in
};

// Fails with InvalidGeometry for an open polyline, fewer than three distinct
// vertices, or a degenerate (collinear) boundary; InvalidArgument for a
// non-finite coordinate. Consecutive coincident vertices are dropped rather
// than reported as zero-length courses with no bearing.
[[nodiscard]] core::Result<ParcelReport> parcelReport(const geometry::Polyline2& boundary);

// The description a deed carries:
//   "<name>: Beginning at E 100.000 N 200.000; thence N 45°30'15" E, 120.000
//    m; thence ...; to the point of beginning. Containing 5000.000 m²
//    (0.5000 ha)."
// One course per "thence", in order. The area is given in both square metres
// and hectares because titles quote whichever the jurisdiction uses.
[[nodiscard]] std::string legalDescription(const ParcelReport& report, std::string_view name);

// Text entities labelling each course with its bearing over its distance,
// placed just inside the boundary at the course midpoint and rotated to read
// along the course - flipped where needed so no label is upside down - plus
// the area at the centroid. `height` is the text height in model units.
// Fails with InvalidArgument for a non-positive height.
[[nodiscard]] core::Result<std::vector<entity::TextGeometry>>
parcelLabels(const ParcelReport& report, double height);

} // namespace katana::cad
