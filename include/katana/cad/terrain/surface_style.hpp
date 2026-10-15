#pragma once

// How a TIN surface is drawn: a named DISPLAY STYLE saying what the plan view
// and the 3D view show of it - its triangles, its outline, its points and
// breaklines, its faces coloured flat or by elevation, slope or aspect, and
// its contours - and in which colours (docs/surface_display.md).
//
// Styles are the drawing's: kept in the Document (Document::surfaceStyles),
// saved with the project as versioned JSON, changed as one undo step, and
// assigned to a surface by name (terrain::NamedSurface::style). A surface
// with no style of its own is drawn in the set's default. Several surfaces
// may share a style, and one surface may be shown in another style by
// assigning it - "multiple styles" is several of these, not several fields
// on a surface.
//
// The 3D view's shading (SurfaceStyle below) lives here, its one home; the
// scene (cad/scene.hpp) reads it from here.

#include <cstddef>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/terrain/surface_store.hpp"

namespace katana::cad {

// How a surface is shaded in the 3D view.
enum class SurfaceStyle {
    Hidden,
    Wireframe,       // triangle edges only
    Shaded,          // filled triangles, lit
    ShadedWithEdges, // both; the edges fade out as the triangles shrink on screen
    // The default: ShadedWithEdges up to kDenseSurfaceTriangles, Shaded above.
    // A dense TIN's edges are all the eye can see at any useful zoom - they
    // covered 73% of the pixels of a 229k-triangle survey, which drew as a
    // black slab - and costing a line per edge they doubled the frame.
    Automatic,
};

// Past this many triangles an Automatic surface is drawn Shaded.
inline constexpr std::size_t kDenseSurfaceTriangles = 50000;

// The most bands a banded ramp may have, and the most stops a style may list
// (a banded set of stops is a band a stop). The 3D view cuts each triangle at
// every band edge it crosses, so its geometry grows by about 643 triangles a
// band on the 21 182-triangle sample terrain (measured: 28 078 triangles at 10
// bands, 3 235 844 at 5 000), and a legend shows 16 rows however many there
// are. 100 bands is a 0.14 m band on that terrain's 13.9 m of relief, and is
// the Surface Styles form's own range - one number for the form, the line and
// a project file.
inline constexpr int kMaximumSurfaceBands = 100;

// What colours a surface's faces.
enum class SurfaceFaces {
    None,      // no faces: lines only (the plan view's default)
    Flat,      // SurfaceDisplayStyle::flatColour
    Elevation, // the ramp, or the stops, over the heights
    Slope,     // the ramp, or the stops, over the slope in percent
    Aspect,    // by the direction a face looks, in eight sectors from north
};

// A value and its colour: a stop of a smooth ramp, or the lower bound of a
// band. In order of value.
struct ColourStop {
    double value = 0.0;
    katana::entity::Color colour{};

    friend bool operator==(const ColourStop&, const ColourStop&) = default;
};

struct SurfaceContours {
    bool show = false;
    double interval = 1.0; // the minor interval, in drawing units
    int majorEvery = 5;    // every Nth level is major; 0 for none
    double base = 0.0;     // the level the intervals count from
    katana::entity::Color minorColour{160, 120, 80, 255};
    katana::entity::Color majorColour{110, 70, 40, 255};
    bool labelMajor = false; // the level written along each major contour

    friend bool operator==(const SurfaceContours&, const SurfaceContours&) = default;
};

struct SurfaceDisplayStyle {
    std::string name;
    std::string description{};

    // ---- the plan view ----
    bool showTriangles = false;
    katana::entity::Color triangleColour{150, 150, 150, 255};
    bool showBoundary = true;
    katana::entity::Color boundaryColour{60, 160, 60, 255};
    bool showPoints = false;
    katana::entity::Color pointColour{200, 60, 60, 255};
    bool showBreaklines = false; // the constrained edges
    katana::entity::Color breaklineColour{220, 120, 0, 255};
    SurfaceFaces planFaces = SurfaceFaces::None;
    double planOpacity = 0.6; // of the faces over what is beneath, 0 to 1
    SurfaceContours contours{};

    // ---- colouring by value, the plan and the 3D view alike ----
    // A ramp by name (surfaceRampNames), stretched over the range; or, when
    // `stops` is not empty, the stops, which override it.
    std::string ramp = "terrain";
    // One colour per band instead of a smooth blend.
    bool banded = false;
    int bands = 10; // when banded and there are no stops
    std::vector<ColourStop> stops{};
    // The values the ramp spans. Absent: the surface's own heights (or, in
    // the 3D view, every visible surface's, so one height is one colour on
    // every surface - the scene's rule, docs/render.md) for Elevation, and
    // 0 to the steepest face for Slope.
    std::optional<double> rangeMin{};
    std::optional<double> rangeMax{};
    katana::entity::Color flatColour{170, 170, 160, 255};

    // ---- the 3D view ----
    SurfaceStyle shading = SurfaceStyle::Automatic;
    SurfaceFaces faces3d = SurfaceFaces::Elevation;

    friend bool operator==(const SurfaceDisplayStyle&, const SurfaceDisplayStyle&) = default;
};

struct SurfaceStyleSet {
    // The JSON form's version (surfaceStylesToJson); a newer one is refused
    // with Unsupported rather than guessed at.
    static constexpr int kVersion = 1;

    std::vector<SurfaceDisplayStyle> styles{};
    // What a surface with no style of its own is drawn in.
    std::string defaultStyle = "Standard";

    // Case-insensitive; nullptr when there is none of that name.
    [[nodiscard]] const SurfaceDisplayStyle* find(std::string_view name) const;

    friend bool operator==(const SurfaceStyleSet&, const SurfaceStyleSet&) = default;
};

// The styles a drawing has before any is made: Standard (the default), and
// styles for triangles, contours, elevation bands, slope bands, a cut/fill
// map and an outline only. What Document::surfaceStyles answers for a
// project that never saved any.
[[nodiscard]] SurfaceStyleSet builtInSurfaceStyles();

// The style `surface` is drawn in: its own when the set has it, else the
// set's default, else the built-in Standard. Never null: a surface naming a
// style since deleted is still drawn.
[[nodiscard]] const SurfaceDisplayStyle& styleOf(const SurfaceStyleSet& styles,
                                                 const katana::terrain::NamedSurface& surface);

// InvalidArgument, naming the field, for a style that cannot be drawn: an
// empty name, a contour interval that is not finite and positive, an opacity
// outside 0 to 1, fewer than two or more than kMaximumSurfaceBands bands, more
// than that many stops, stops out of order or not finite, a range that is not
// finite or empty, an unknown ramp.
[[nodiscard]] katana::core::Status checkSurfaceStyle(const SurfaceDisplayStyle& style);

// checkSurfaceStyle for every style, and the set as a whole: at least one
// style, no two of one name compared without case (a line names a style in
// any case), and a default that is one of them. InvalidArgument naming the
// style. What Document::setSurfaceStyles and surfaceStylesFromJson refuse.
[[nodiscard]] katana::core::Status checkSurfaceStyleSet(const SurfaceStyleSet& styles);

// The words a line (SURFACESTYLE ... faces= shading=) and the JSON form say
// these in: none, flat, elevation, slope, aspect; and automatic, shaded,
// edges (ShadedWithEdges), wireframe, hidden. Read without case; nullopt for
// any other word.
[[nodiscard]] std::string_view surfaceFacesWord(SurfaceFaces faces);
[[nodiscard]] std::optional<SurfaceFaces> parseSurfaceFaces(std::string_view word);
[[nodiscard]] std::string_view surfaceShadingWord(SurfaceStyle shading);
[[nodiscard]] std::optional<SurfaceStyle> parseSurfaceShading(std::string_view word);

// The versioned JSON form, as Document keeps it: written with the keys in a
// fixed order, so the same set is the same text.
[[nodiscard]] std::string surfaceStylesToJson(const SurfaceStyleSet& styles);
// ParseFailure for text that is not the form; Unsupported for a newer
// version; InvalidArgument (checkSurfaceStyle) for a style that cannot be
// drawn.
[[nodiscard]] katana::core::Result<SurfaceStyleSet> surfaceStylesFromJson(std::string_view text);

// The named colour ramps a style may use, the default first: terrain,
// earth, diverging (cut blue to fill red), slope (green to red), grey,
// spectral.
[[nodiscard]] std::span<const std::string_view> surfaceRampNames();
// The colour at `t` (clamped to 0 to 1) along ramp `ramp`; an unknown name is
// the first ramp's. The styles' ramp table: the 3D scene and the plan view both
// colour through it.
[[nodiscard]] katana::entity::Color surfaceRampColour(std::string_view ramp, double t);

} // namespace katana::cad
