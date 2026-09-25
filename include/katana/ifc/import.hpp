#pragma once

// An IFC file (IFC2X3, IFC4, IFC4X3) -> the drawing: entities, layers,
// alignments and surfaces, with no third-party library.
//
// WHAT COMES IN, AND AS WHAT:
//
//   IfcAlignment          a named Alignment, its PIs and PVIs RECONSTRUCTED
//                         from the IFC 4.3 business logic - each run of
//                         [CLOTHOID] CIRCULARARC [CLOTHOID] between two LINEs
//                         is one PI at the intersection of those lines; each
//                         PARABOLICARC one PVI at the intersection of its
//                         grades - and then CHECKED: Katana solves the
//                         reconstruction and every segment the file states
//                         must start within kAlignmentTolerance of where it
//                         does. Where the check fails, or the geometry has no
//                         PI form (it starts or ends on a curve, compounds
//                         two arcs, or uses a transition Katana does not have
//                         - it has the clothoid), the alignment comes in as a
//                         3D polyline of its exact geometry, and the warning
//                         says why. Its stations are taken from its
//                         Pset_Stationing referents.
//   an element or         its drawable shape - the Axis, else the FootPrint,
//   annotation            else the Annotation representation, else the centre
//                         line of a swept disk body - as Points, Lines, Arcs,
//                         Circles, Polylines and Texts in project coordinates,
//                         heights kept (entity::setHeights) and absent where
//                         the file has none. An element whose shape is only a
//                         solid Katana cannot draw comes in as a point at its
//                         placement, so it is not lost, and is counted.
//   a terrain             IfcGeographicElement TERRAIN (or any product whose
//                         body is a triangulated face set or TIN) becomes a
//                         surface for the caller to keep, as surfaces are
//                         session data.
//
// Every entity keeps what it was: `ifc.class`, `ifc.globalId`, `ifc.name`,
// `ifc.predefinedType`, `ifc.tag` and the source file in its metadata; its
// property sets and quantities as properties named "<set>/<property>" (the
// way a 12d attribute group flattens), its classifications as
// "Classification/<system>", the distribution system it serves as "System";
// its presentation layer as its layer, and its colour. What Katana itself
// wrote - Katana_Attributes and Katana_Provenance - goes back where it came
// from, so an export read back is the drawing it was.
//
// COORDINATES. Lengths are brought to metres from the file's length unit and
// angles to radians from its plane angle unit. A file with an IfcMapConversion
// is read into the coordinates of its projected system - the conversion's
// eastings, northings, height, rotation and scale applied - so that a
// georeferenced file lands where it is; IfcImport::coordinateSystem names the
// system (the caller decides whether the project takes it).
//
// WHAT IS NOT READ, AND SAID: solids other than swept disks and triangulated
// surfaces (Katana has no solid entity), IFC4X1-form alignments, cant, complex
// instances, and the axes of an IfcGrid (what is placed on a grid is placed
// where the grid puts it). Each is counted in `warnings`.

#include <cstddef>
#include <filesystem>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/commands/command.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/model.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::ifc {

// How far a reconstructed alignment may stand from the geometry the file
// states before the reconstruction is refused: the tolerance the 12d
// archive import checks its reconstructions to (docs/interop.md,
// "Alignments"), for the same reason.
inline constexpr double kAlignmentTolerance = 0.010;

struct ImportOptions {
    std::string sourceName; // recorded in each entity's metadata as "source"
    // Curves Katana has no exact kind for - an arc inside an indexed
    // polycurve, a transition of a kind other than the clothoid - are
    // chorded to within this of the true curve (model units).
    double curveTolerance = 0.001;
    // Subtracted from every plan coordinate, as the other importers' LOCAL
    // does; nullopt reads the file where it is.
    std::optional<katana::geometry::Vec2> originShift;
    bool importAlignments = true;
    bool importElements = true;
    bool importSurfaces = true;
};

struct ImportedSurface {
    std::string name;
    katana::terrain::TinSurface surface;
};

struct IfcImport {
    std::string schema;           // "IFC4X3_ADD2", as FILE_SCHEMA names it
    std::string coordinateSystem; // "EPSG:7856", when an IfcProjectedCRS names one
    std::vector<entity::Entity> entities;
    std::vector<entity::Layer> layers;         // every layer the entities are on
    std::vector<entity::Alignment> alignments; // named uniquely within the file
    std::vector<ImportedSurface> surfaces;
    // Of everything imported: the entities, the alignments' PIs and the
    // surfaces - what LOCAL moves to the origin.
    katana::geometry::Box2 bounds;
    // Products read, by class in the schema's spelling ("IfcPipeSegment").
    std::map<std::string, std::size_t> classes;
    std::size_t products = 0;         // elements and annotations read
    std::size_t productsImported = 0; // of which drawn with their shape
    std::size_t productsAsPoints = 0; // of which only a point at their placement
    std::size_t alignmentsAsPolylines = 0;
    std::vector<std::string> warnings;
};

// ParseFailure, naming the line, for text that is not an ISO 10303-21 file
// or that breaks its syntax; Unsupported for a schema that is not IFC.
[[nodiscard]] katana::core::Result<IfcImport> readIfc(std::string_view text,
                                                      const ImportOptions& options = {});
// NotFound when the file cannot be read.
[[nodiscard]] katana::core::Result<IfcImport> readIfcFile(const std::filesystem::path& path,
                                                          const ImportOptions& options = {});

// The import as ONE undoable step: the layers the model lacks (parents
// first), the alignments - renamed "<name> (2)" where the model already has
// one of the name, and said so in `imported.warnings` - and the entities,
// moved out of `imported`. Null when there is nothing to add.
[[nodiscard]] katana::commands::CommandPtr importCommand(IfcImport& imported,
                                                         const entity::Model& model);

} // namespace katana::ifc
