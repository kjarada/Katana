#pragma once

// A DXF file -> entities, layers and linetypes, with no third-party library.
//
// WHY A READER OF OUR OWN. DXF used to come in through GDAL's vector driver,
// which reads a drawing as GIS features and loses what a CAD user exchanges a
// DXF for: an arc became a 9-vertex polyline, a circle an OPEN polyline, text
// a point. This module reads the entities themselves, so an ARC is an Arc.
// It sits where the 12d archive module does - entity and geometry below it,
// nothing above - and so builds with -DKATANA_BUILD_IO=OFF, which puts a
// hand-written parser of untrusted text under the sanitizer job.
//
// WHERE EACH ENTITY GOES.
//
//   LINE                a Line. One of zero length, a Point: some programs
//                       draw a surveyed dot that way, and dropping it loses
//                       the dot.
//   ARC, CIRCLE         an Arc, a Circle.
//   LWPOLYLINE,         a Polyline, closed flag kept. Arc segments (bulges)
//   POLYLINE (2D, 3D)   are chorded to `curveTolerance` - Polyline2 has no arc
//                       segments yet - and each one chorded is counted.
//                       Heights: a 3D polyline's per-vertex Z, and a 2D one's
//                       elevation, go through entity::setHeights.
//   POINT               a Point; a non-zero Z is its elevation.
//   TEXT, MTEXT         Text: string, height, rotation. MTEXT formatting
//                       codes are stripped (plainMText) and each paragraph
//                       becomes a Text of its own, because a Text is one line.
//   INSERT, MINSERT     the block's entities, expanded (nested blocks too),
//                       with the insert's attributes as properties of every
//                       entity it produced. A visible attribute is also a Text.
//   DIMENSION           its picture block, expanded: lines, arrows and text.
//   ATTDEF              nothing: it is the template the ATTRIB replaced.
//   anything else       counted in the tally as read and not imported.
//
// Paper space entities are counted and not imported: a Katana drawing is the
// model. Entities in a tilted plane (an extrusion direction that is not +Z or
// -Z) are projected onto the plan, circles and arcs as chorded polylines, and
// counted.
//
// NEVER FAILS ON CONTENT. An entity that cannot be represented is a warning
// and a tally, because one bad entity must not cost the user the other ten
// thousand. It fails only when the input is not a DXF file at all (no
// SECTION), is a binary DXF, or a group code line is not a number - past that
// point the pairs cannot be told apart and nothing after it can be trusted.

#include <cstddef>
#include <filesystem>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/tables.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::dxf {

// Metadata keys this reader writes and the writer reads back.
inline constexpr std::string_view kMetaSource = "source";
// The block an entity was expanded from, outermost first ("SITE/TREE").
inline constexpr std::string_view kMetaBlock = "dxf.block";
// An entity's own linetype and lineweight (hundredths of a millimetre) where
// they are not ByLayer. Katana has no per-entity field for either yet.
inline constexpr std::string_view kMetaLinetype = "dxf.linetype";
inline constexpr std::string_view kMetaLineweight = "dxf.lineweight";
// The registered application name of the extended data this module writes
// and reads, for what the format's own groups cannot hold: a layer's full
// Katana path (a DXF layer name cannot hold a "/"), the heights of an entity
// known at only some of its vertices (the format has no "no height"), and a
// colour no index is (R2000 has no true colour). Each follows its word: the
// path its text, the heights the `elevations` list's text in pieces, the
// colour "#RRGGBB".
inline constexpr std::string_view kApplicationName = "KATANA";
inline constexpr std::string_view kExtendedPath = "path";
inline constexpr std::string_view kExtendedHeights = "elevations";
inline constexpr std::string_view kExtendedColour = "colour";

struct ImportOptions {
    // Largest distance a chord may stand off the arc it replaces, in model
    // units - a polyline's bulges, and a circle or arc in a tilted plane.
    // Stated rather than hidden: it is a lossy step.
    double curveTolerance = 0.001;
    // Subtracted from every plan coordinate. See interop::VectorImportOptions.
    std::optional<katana::geometry::Vec2> originShift;
    // Written into every entity's metadata as `source`; empty writes nothing.
    std::string sourceName;
    // Blocks inside blocks deeper than this are not expanded, and said so. A
    // block that inserts itself is caught separately, at any depth.
    std::size_t maximumBlockDepth = 16;
    // Stops a hostile or broken file (a MINSERT of a billion copies) from
    // exhausting memory: expansion ends there, with a warning.
    std::size_t maximumEntities = 20'000'000;
};

// How many of one DXF entity kind the file held, and how many entities the
// import made from them (an INSERT makes many, a skipped kind none).
struct EntityTally {
    std::string kind;
    std::size_t read = 0;
    std::size_t imported = 0;

    friend bool operator==(const EntityTally&, const EntityTally&) = default;
};

struct DxfImport {
    std::vector<katana::entity::Entity> entities;
    // The LAYER table in file order, then any layer an entity names that the
    // table lacks (with the defaults), so the caller creates exactly these.
    std::vector<katana::entity::Layer> layers;
    // The LTYPE table's patterns that Katana can draw, in file order. The
    // built-in ByLayer, ByBlock and Continuous are not listed.
    std::vector<katana::entity::Linetype> linetypes;
    katana::geometry::Box2 bounds;
    // $ACADVER ("AC1015") and the release it names ("R2000"); empty when the
    // file has no header, as a minimal R12 file need not.
    std::string version;
    std::string release;
    std::vector<EntityTally> tally;
    // Arc segments of polylines replaced by chords.
    std::size_t arcsChorded = 0;
    std::vector<std::string> warnings;
};

// `bytes` is the whole file. UTF-8 is used as it is; anything else is read as
// the file's $DWGCODEPAGE says, which for every Western code page means
// Windows-1252. \U+XXXX escapes become the characters they name.
[[nodiscard]] katana::core::Result<DxfImport> readDxf(std::string_view bytes,
                                                      const ImportOptions& options = {});
[[nodiscard]] katana::core::Result<DxfImport> readDxfFile(const std::filesystem::path& path,
                                                          const ImportOptions& options = {});

// True for a path ending in .dxf, in any letter case: the one test the front
// ends route by, so that the window and the command line agree on it.
[[nodiscard]] bool isDxfPath(const std::filesystem::path& path);

// The release an $ACADVER names: "AC1009" -> "R12", "AC1015" -> "R2000",
// "AC1032" -> "R2018". Empty for one this does not know.
[[nodiscard]] std::string_view releaseName(std::string_view acadver);

} // namespace katana::dxf
