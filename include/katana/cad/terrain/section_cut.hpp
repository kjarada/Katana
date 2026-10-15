#pragma once

// Cutting a section of the DRAWING: its shown surfaces along an alignment, with a
// design profile beside them when there is one (docs/terrain.md, "Hidden is a matter
// of the picture"; docs/cad.md, "One section routine for both callers").
//
// A section is a picture of the surfaces that were shown when it was cut. This is the
// one place that decides which those are and at what interval they are sampled, so
// Terrain > Cut Section and the re-cut of an open section view, when the drawing's
// surfaces change (MainWindow::recutSections), cannot differ in either. Headless, so
// that the recipe a view keeps - the section's own alignment and its design profile -
// is tested without a window.

#include "katana/cad/section.hpp"
#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad {

class Document;

// Stations across a section's alignment at the least, and the finest interval between
// them: two thousand across, floored at 100 mm. Fine enough to read off, and the
// surface breaks are added on top so the interval never decides the accuracy of a
// ridge or a toe. The floor is the section's own: a finer interval over a short line
// costs time and changes nothing a person can read.
inline constexpr double kSectionStations = 2000.0;
inline constexpr double kLeastSectionInterval = 0.1;

// The interval a section along `length` of alignment is cut at:
// max(length / kSectionStations, kLeastSectionInterval).
[[nodiscard]] double drawingSectionInterval(double length);

struct DrawingSection {
    Section section;
    // The interval the section was cut at, for the log line that reports it.
    double interval = 0.0;
};

// The drawing's shown surfaces cut along `alignment`, with `design` as one more series
// when given. Hidden surfaces are left out: a section is a picture, and hiding a
// surface takes it out of every picture.
//
// Fails with
//   NotFound       the drawing has no surface,
//   InvalidState   every surface of the drawing is hidden,
// and with extractSection's and appendDesignProfile's own errors (InvalidArgument for
// an alignment of fewer than two distinct vertices, an empty design name ...).
[[nodiscard]] katana::core::Result<DrawingSection>
cutDrawingSection(const Document& document, const katana::geometry::Polyline2& alignment,
                  const SectionDesign* design = nullptr);

} // namespace katana::cad
