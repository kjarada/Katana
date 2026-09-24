#pragma once

// A deterministic survey drawing of about thirty thousand entities, for the
// plan-view paint and plot benchmarks.
//
// The owner's real archives are the drawings the plan view is slow on, but
// they are tens of megabytes of someone's survey and cannot be committed. This
// builds a drawing with the same make-up instead, measured from 'Test 4
// without tin' (27,886 entities, 179,671 vertices, 14,059 symbol stamps,
// 8,709 strings in library linestyles): coded points drawn with library
// symbols, strings in library linestyles, plain strings (contours) long
// enough to cross the whole site - the case clipping is for - labels, arcs,
// hatched lots and dimensions, at map-grid coordinates of the size real survey
// data has, so the float paths see the magnitudes they see in use.
//
// Deterministic: the same options give the same drawing, bit for bit, on
// every machine - the numbers come from a SplitMix64 stream and not from a
// standard distribution, whose output the standard does not fix.

#include <cstddef>
#include <cstdint>

#include "katana/cad/document.hpp"
#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::bench {

struct SurveyDrawingOptions {
    std::uint64_t seed = 20260924;
    // Easting and northing of the site's south-west corner: map-grid sized,
    // as the archives are.
    katana::geometry::Point2 origin{300000.0, 6250000.0};
    double widthMetres = 2000.0;
    double heightMetres = 1500.0;
    std::size_t codedPoints = 16000;   // each drawn with a library symbol
    std::size_t styledStrings = 10000; // each in a library linestyle, 6 to 17 vertices
    // Contours: 400 vertices each, evenly spaced across the site, so each is
    // several metres from the next as a 1 m interval on a gentle site is.
    std::size_t plainStrings = 250;
    std::size_t labels = 800;
    std::size_t arcs = 300;
    std::size_t hatchedLots = 150;
    std::size_t dimensions = 100;
};

// What was made, for a benchmark's counters and a test's expectations.
struct SurveyDrawingSummary {
    std::size_t entities = 0;
    std::size_t vertices = 0;
    std::size_t symbolStyles = 0;    // distinct styles that draw a library symbol
    std::size_t linestyleStyles = 0; // distinct styles that draw a library linestyle
    katana::geometry::Box2 extent;
};

// Builds the drawing into `document`, which should be empty and have its
// style library loaded: the library's symbols and linestyles are what the
// styles name (in name order, so the choice is deterministic too). With no
// library the styles fall back to the built-in point shapes and plain lines,
// and the drawing is still the same geometry. Fails with whatever a command
// refuses.
[[nodiscard]] katana::core::Result<SurveyDrawingSummary>
buildSurveyDrawing(katana::cad::Document& document, const SurveyDrawingOptions& options = {});

} // namespace katana::bench
