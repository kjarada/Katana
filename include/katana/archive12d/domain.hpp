#pragma once

// A 12d Archive <-> Katana's domain model.
//
// IMPORT: WHERE EACH ELEMENT GOES.
//
//   model                  a layer. 12d model names are already `/`-separated
//                          tree names, which is exactly what a Katana layer
//                          path is, so "Stage 1/Water/Drainage" arrives as
//                          that nested layer.
//   string super (and      one vertex: a Point. Otherwise a Polyline, with
//   2d 3d 4d pipe          arcs chorded to `curveTolerance` - Polyline2 has no
//   polyline face          arc segments. Transitions are chorded too.
//   interface)
//   string arc             an Arc; `string circle` and `string feature` a Circle.
//   string text, and the   Text entities.
//   vertex text of a 4d
//   or super string
//   string plot_frame      the sheet rectangle, as a closed Polyline.
//   string drainage        the line as a Polyline, each pit as a Point
//                          carrying its name, type and size; pipes as
//                          properties of the line.
//   string super_alignment a named Alignment when the geometry can be
//   alignment, pipeline    expressed by PIs (see below); always ALSO a
//                          Polyline of the solved centreline, so nothing is
//                          invisible when it cannot.
//   tin, full_tin          a terrain::TinSurface of the visible triangles.
//   super_tin              reported; the member tins are what is imported.
//   primitive_3d/trimesh   NOT imported - Katana has no mesh entity. Counted
//                          and reported, never silently dropped.
//   string las_cloud_data  a list of points for the caller to make a
//                          reference layer from (this layer cannot see the
//                          point cloud types; interop can).
//
// HEIGHTS. Entities are 2D. A string whose vertices share one height carries
// it as the `elevation` property, which is what Surface From Drawing reads.
// One whose heights differ carries them all in `elevations`, space separated,
// in vertex order - a text property because the property model has no list
// type, and written with enough digits to read back exactly. Null heights are
// the word `null` in that list.
//
// STYLES. A 12d linestyle name becomes a Katana Style of that name, which
// the entity carries as its style - the same field a drawn entity's style
// lives in, so the style panel, the STYLE verb and the plotter all see it.
// The importer lists the styles it needs (`stylesNeeded`) and the caller
// creates the missing ones in the same transaction as the layers. A 12da
// says nothing about what a linestyle looks like, so a new one is continuous
// at the default weight, described as coming from 12d, for the user to
// finish in the style manager.
//
// ATTRIBUTES become entity properties, typed as they were (integer, real,
// text); a group flattens into its members' names, `Group/Sub/Name`. What 12d
// knows about a string that Katana has no field for - its name, colour NAME,
// chainage, breakline, point ids - goes in `metadata` under `12d.*`, and
// export reads it back from there, so a string that is imported and exported
// unchanged is the string it was.
//
// ALIGNMENTS. Katana defines an alignment by its PIs and derives the elements;
// a 12d super alignment stores the solved elements and, separately, however
// the designer constructed them. Where the construction is the IP method the
// PIs are taken directly. Otherwise they are RECONSTRUCTED from the solved
// elements - each run of spiral/arc/spiral between two straights is one PI at
// the intersection of those straights - and the reconstruction is then
// CHECKED: the alignment is solved by Katana and every vertex 12d recorded
// must lie within `alignmentTolerance` of it. A compound or reverse curve, an
// alignment that starts on an arc, or a transition Katana does not have (it
// has the clothoid) fails that check or never reaches it, and the alignment
// is then imported as its polyline only, with a warning that says why.

#include <cstddef>
#include <cstdint>
#include <optional>
#include <string>
#include <vector>

#include "katana/archive12d/archive.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/model.hpp"
#include "katana/entity/tables.hpp"
#include "katana/geometry/mesh.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::archive12d {

// Property and metadata keys, in one place so import and export cannot
// disagree about them.
inline constexpr std::string_view kElevationProperty = "elevation";
inline constexpr std::string_view kElevationsProperty = "elevations";
inline constexpr std::string_view kMetaName = "12d.name";
// Which vertex of its string a point came from, one-based. Set only where a
// `breakline point` string of several vertices was split into one point per
// vertex - see the import - so that they can be told apart and put back in
// the order the string had them.
inline constexpr std::string_view kMetaVertex = "12d.vertex";
inline constexpr std::string_view kMetaColour = "12d.colour";
inline constexpr std::string_view kMetaChainage = "12d.chainage";
inline constexpr std::string_view kMetaBreakline = "12d.breakline";
inline constexpr std::string_view kMetaElement = "12d.element";
inline constexpr std::string_view kMetaPointIds = "12d.point_ids";
inline constexpr std::string_view kMetaSource = "source";
// Every scalar a string carried that Katana has no field for, as
// `12d.x.<key>`; export writes them back as they were.
inline constexpr std::string_view kMetaExtraPrefix = "12d.x.";
// A vertex symbol (manual 1.5.8.4.10) is a linestyle drawn at a vertex, with
// a colour, a size, a rotation, an offset and a raise. On a POINT it is the
// point's whole appearance, so the point takes the symbol's linestyle as its
// style (the Style carries the shape and the size) and the symbol's colour
// as its colour; what remains of the block, and only where it is not the
// default, is `12d.symbol.<key>`. The string's own linestyle, which 12d
// writes on a point beside the symbol's, is kept as kMetaStringStyle when
// the two differ so that export can put it back. On a LINE the vertices
// have no style of their own; every block is kept as `12d.symbol.<key>`
// holding one value per block, the way segment colours are kept, and the
// import says the symbols are not drawn.
inline constexpr std::string_view kMetaSymbolPrefix = "12d.symbol.";
// What a text annotation says that Katana's TextGeometry has no field for,
// as `12d.text.<key>`: the slant and width factor (Katana text has neither),
// the justification and the sizing mode it was placed by, and an offset in
// paper millimetres, which has no model-unit meaning without a plot scale.
// Kept so that a text imported and exported unchanged is the text it was.
inline constexpr std::string_view kMetaTextPrefix = "12d.text.";
inline constexpr std::string_view kMetaStringStyle = "12d.string_style";

struct ImportOptions {
    // Largest distance a chord may stand off the arc or transition it
    // replaces, in model units. Stated rather than hidden: it is a lossy step.
    double curveTolerance = 0.001;
    // How far a vertex recorded by 12d may lie from the alignment Katana
    // solves from the reconstructed PIs before the reconstruction is rejected.
    // 10 mm: 12d's default "clothoid" is a series approximation of the spiral
    // Katana computes exactly, and the two differ by a few millimetres on a
    // tight curve - while a wrongly reconstructed PI is out by metres.
    double alignmentTolerance = 0.010;
    // Prefixed to every layer, as a parent path: "survey" puts model "Kerb" on
    // layer "survey/Kerb". Empty: models become top-level layers.
    std::string layerPrefix;
    // Subtracted from every plan coordinate. See interop::VectorImportOptions.
    std::optional<katana::geometry::Vec2> originShift;
    bool attributesAsProperties = true;
    // Written into every entity's metadata as `source`.
    std::string sourceName;
};

struct ImportedSurface {
    std::string name;
    std::string colour;
    katana::terrain::TinSurface surface;
    std::size_t trianglesInFile = 0;
    std::size_t trianglesNulled = 0; // nulled, construction, or null-height
};

// A `primitive_3d` (manual 1.4.9): a mesh of triangles that is NOT a
// surface - it may be closed, may overhang, and 12d writes pipes, pits and
// structures as one. Held beside the surfaces in the session, for the same
// reason and with the same consequence: it is not an entity and is not
// undoable.
struct ImportedMesh {
    std::string name;
    std::string layer;     // the 12d model, as a layer path
    std::string colourName; // 12d's name for it, kept whatever Katana makes of it
    std::optional<katana::entity::Color> color;
    katana::geometry::TriangleMesh mesh;
    // One per face where the file colours its faces (`face_infos` +
    // `face_flags`), otherwise empty. The name is kept beside the RGB so a
    // colour Katana has no RGB for still goes back out as it came in.
    std::vector<std::string> faceColourNames;
    std::vector<std::optional<katana::entity::Color>> faceColors;
    katana::entity::PropertyMap properties;
    // Read, reported, and not modelled: an edge list is a drawing decision
    // 12d makes about ITS viewer, and per-vertex and per-edge infos likewise.
    std::size_t edgesInFile = 0;
    std::size_t vertexInfosInFile = 0;
    std::size_t edgeInfosInFile = 0;
};

struct ImportedCloudPoint {
    double x = 0.0;
    double y = 0.0;
    double z = 0.0;
    std::uint16_t intensity = 0;
    std::uint8_t classification = 0;
};

struct ImportedCloud {
    std::string name;
    std::vector<ImportedCloudPoint> points;
    std::string referenceFile; // a ref_data cloud: the LAS file it names
};

// How many of each element the archive held, and what became of them.
struct ElementTally {
    std::string keyword; // "string super", "full_tin" ...
    std::size_t read = 0;
    std::size_t imported = 0;

    friend bool operator==(const ElementTally&, const ElementTally&) = default;
};

struct DomainImport {
    std::vector<katana::entity::Entity> entities;
    // Layers the entities reference, in first-use order, with the colour of
    // the first string seen on each where that colour is one Katana knows.
    std::vector<katana::entity::Layer> layersNeeded;
    // Styles the entities reference, in first-use order: one per distinct 12d
    // linestyle name. The caller creates those the drawing lacks.
    std::vector<katana::entity::Style> stylesNeeded;
    std::vector<katana::entity::Alignment> alignments;
    std::vector<ImportedSurface> surfaces;
    std::vector<ImportedMesh> meshes;
    std::vector<ImportedCloud> clouds;
    // Super tins, as (name, member names): nothing to import, but a caller
    // may want to say they were there.
    std::vector<SuperTin> superTins;

    katana::geometry::Box2 bounds;
    std::vector<ElementTally> tally;
    std::vector<std::string> warnings;
};

// Never fails on CONTENT - an element that cannot be represented is a warning
// and a tally of 0 imported, because one bad string must not cost the user the
// other ten thousand. Fails with InvalidArgument for bad options.
[[nodiscard]] katana::core::Result<DomainImport> toDomain(const Archive& archive,
                                                          const ImportOptions& options = {});

// ---- export ----------------------------------------------------------------

struct ExportSurface {
    std::string name;
    const katana::terrain::TinSurface* surface = nullptr;
};

// A mesh to write back as a `primitive_3d`. The face colours, where given,
// become the `face_infos` table and the one-based `face_flags` that index
// it, which is how 12d carries them.
struct ExportMesh {
    std::string name;
    std::string layer;
    std::string colourName;
    const katana::geometry::TriangleMesh* mesh = nullptr;
    std::vector<std::string> faceColourNames;
};

struct ExportOptions {
    // Empty exports the whole model; otherwise only these entities.
    std::vector<katana::entity::EntityId> entities;
    // Added back to every plan coordinate, undoing an import's originShift.
    std::optional<katana::geometry::Vec2> originShift;
    bool propertiesAsAttributes = true;
    bool includeAlignments = true;
    // Chord tolerance for the solved centreline written with each alignment.
    double curveTolerance = 0.001;
};

struct DomainExport {
    Archive archive;
    std::size_t entitiesWritten = 0;
    std::size_t entitiesSkipped = 0;
    std::size_t alignmentsWritten = 0;
    std::size_t surfacesWritten = 0;
    std::size_t meshesWritten = 0;
    std::vector<std::string> warnings;
};

// Points, lines, polylines, arcs, circles and text all have a 12da form and are
// written. Dimensions do not: they are skipped and counted.
[[nodiscard]] katana::core::Result<DomainExport>
fromDomain(const katana::entity::Model& model, const std::vector<ExportSurface>& surfaces,
           const ExportOptions& options = {}, const std::vector<ExportMesh>& meshes = {});

// ---- shared pieces, exposed for testing -------------------------------------

// A 12d model name as a valid Katana layer path. Tree separators are kept;
// what a layer path cannot hold (an empty level, "." or "..", a control
// character, a backslash) is replaced, and the result always passes
// entity::validateLayerPath. An empty name becomes "12d".
[[nodiscard]] std::string layerPathForModel(std::string_view modelName,
                                            std::string_view prefix = {});

// The Katana symbol a 12d point linestyle is best drawn with, from its name:
// "ELEC Pole - Power" is a pole, "DRAIN Gully Pit Point" a manhole, "STNS
// Default MX Survey Mark" a target, "TOPO Natural Surface Point" a cross.
// A 12da carries only the NAME of a linestyle - what it looks like lives in
// the 12d project - so this is a reading of the name, made once when the
// style is created and changed in the style manager if it is wrong. Never
// empty: a name that says nothing is a circle.
[[nodiscard]] std::string_view symbolForLinestyle(std::string_view name);

// The RGB of one of 12d Model's standard colour names, matched without regard
// to case. nullopt for any other name: colours are defined per project in
// 12d, and the archive carries only the name, so an unknown one ("pen 025",
// "vis concrete") is left ByLayer rather than guessed at.
[[nodiscard]] std::optional<katana::entity::Color> standardColour(std::string_view name);
// The standard name nearest to `colour` - what export writes for an entity
// whose colour did not come from 12d in the first place.
[[nodiscard]] std::string nearestStandardColour(const katana::entity::Color& colour);

// The heights of an entity's `count` vertices, from the `elevations` list or
// the single `elevation` that import writes (see HEIGHTS above). nullopt where
// there is none. A list whose length is not `count` describes some other
// geometry - a vertex has been added or removed since - and is ignored rather
// than applied to the wrong vertices; the single elevation then stands in.
//
// Here, and public, because there must be ONE reader of that list: export
// uses it, and so does the desktop application's Surface From Drawing, which
// is what makes an imported 3D string usable as a breakline.
[[nodiscard]] std::vector<std::optional<double>>
entityHeights(const katana::entity::Entity& entity, std::size_t count);

// Splits a list written by import (`12d.segment_colours`, `12d.interface_modes`
// ...): blank-separated, an item with a blank in it in double quotes with the
// 12da escapes. The inverse of what import joins, exposed so export and a
// test read the same list the same way.
[[nodiscard]] std::vector<std::string> splitList(std::string_view text);

// The vertices of a string as a plan polyline, arcs and transitions chorded to
// `tolerance`. Empty when the string has no vertices.
[[nodiscard]] std::vector<katana::geometry::Point2>
chordedPlan(const std::vector<Vertex>& vertices, const std::vector<Segment>& segments,
            bool closed, double tolerance);

} // namespace katana::archive12d
