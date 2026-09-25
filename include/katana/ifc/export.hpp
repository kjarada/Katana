#pragma once

// A drawing, its alignments, its surfaces and a subsurface utility
// investigation -> an IFC 4.3 file (IFC4X3_ADD2), with no third-party library.
//
// WHAT GOES WHERE (classification.hpp decides the classes; docs/ifc.md has
// the full tables):
//
//   IfcProject                the project, its units (metre, radian) and one
//                             3D model context with Axis, Body, FootPrint and
//                             Annotation subcontexts
//   IfcProjectedCRS +         the project's coordinate system, when it has an
//   IfcMapConversion          EPSG code. Coordinates are written relative to a
//                             local origin near the work, and the conversion
//                             gives that origin's easting, northing and height,
//                             so the file is both georeferenced and drawable:
//                             a viewer holding MGA coordinates in single
//                             precision would have a metre of resolution.
//   IfcSite                   holds every element and annotation
//
//   an alignment              IfcAlignment aggregated into the project, with
//                             its business logic - IfcAlignmentHorizontal and
//                             IfcAlignmentVertical nesting IfcAlignmentSegment
//                             design parameters (LINE, CIRCULARARC, CLOTHOID;
//                             CONSTANTGRADIENT, PARABOLICARC), each layout
//                             closed by a zero-length segment - and its
//                             geometry: an IfcCompositeCurve of IfcCurveSegment
//                             (IfcLine, IfcCircle, IfcClothoid) and, with a
//                             profile, an IfcGradientCurve over it
//                             (IfcLine, IfcPolynomialCurve). Stations are
//                             IfcReferent STATION with Pset_Stationing at the
//                             start, the end and every key station.
//
//   a service (AS 5488)       one IfcDistributionSystem per service, grouping
//                             one segment element per graded segment - the
//                             standard grades segments, not services - of the
//                             class classifyUtilityRun gives, and an
//                             IfcAnnotation SURVEY for every located point.
//                             Each carries its AS 5488 grade (the
//                             AS5488_QualityLevel property set, Pset_Uncertainty
//                             and an IfcClassificationReference to its quality
//                             level) and the delivery schema's attributes
//                             (TfNSW_UtilitySchema) with the values the
//                             schedule wrote.
//
//   a drawing entity          the class classifyEntity gives: an IfcKerb, an
//                             IfcRailing, a pipe or a pit, or an IfcAnnotation.
//                             12d drainage strings become pipes pit to pit and
//                             the pits chambers, in one system per string.
//   a surface                 IfcGeographicElement TERRAIN, an
//                             IfcTriangulatedFaceSet
//
// NOTHING IS INVENTED. A service segment is drawn in 3D - its centre line as
// the Axis and, with a size, a swept disk as its Body - only where a level is
// known at both ends and can be brought to the centre of the service. Where
// it cannot, the segment has a 2D FootPrint and no Body: a line drawn at a
// guessed depth would be believed. Absent heights are absent in every
// representation, never zero.
//
// WHY A WRITER OF OUR OWN. IFC is text (ISO 10303-21), and what the export
// needs is the mapping, not a model library: the one IFC toolkit that writes
// 4.3 alignments is IfcOpenShell, whose C++ core is a large dependency to
// take for the writing of a text file, and which would be a second model of
// the drawing beside Katana's own. So, like the DXF and 12d writers, this
// module needs nothing above the domain model and builds with
// -DKATANA_BUILD_IO=OFF. IfcOpenShell is used instead where it is worth most -
// as the independent judge of what is written (tools/check_ifc.py, the
// cli.ifc_* tests).

#include <cstddef>
#include <filesystem>
#include <map>
#include <optional>
#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/entity/model.hpp"
#include "katana/ifc/classification.hpp"
#include "katana/math/vec3.hpp"
#include "katana/survey/subsurface/delivery_schema.hpp"
#include "katana/survey/subsurface/utility_network.hpp"

namespace katana::terrain {
class TinSurface;
}

namespace katana::ifc {

// The project's coordinate system, as IfcProjectedCRS takes it. Written only
// when `name` is an EPSG code ("EPSG:7856"): IFC4X3_ADD2 names a system by
// its EPSG code and has no attribute for a definition in WKT, and a name no
// validator can resolve is georeferencing in appearance only.
struct Georeference {
    std::string name;          // "EPSG:7856"
    std::string description;   // "GDA2020 / MGA zone 56"
    std::string geodeticDatum; // "GDA2020"; empty to leave it to the code
    std::string verticalDatum; // "EPSG:5711" (AHD height); empty when not known
    std::string mapProjection; // "UTM"; empty to leave it to the code
    std::string mapZone;       // "56"
};

// A surface to export, by name. The surfaces are session data, not the
// model's (docs/cad.md), so the caller passes the ones it has.
struct SurfaceInput {
    std::string name;
    const katana::terrain::TinSurface* surface = nullptr;
};

// A subsurface utility investigation: the services as survey/subsurface read
// them, graded here with `grading`.
struct UtilityInput {
    std::vector<survey::subsurface::UtilityLine> lines;
    survey::subsurface::GradingSettings grading{};
    // The delivery schema the schedule was written to (delivery_schema.hpp),
    // when the caller has it. With it, the delivery property set lists the
    // schema's attributes in its order, each described by the schema's label,
    // and a value from a listed domain is an IfcPropertyEnumeratedValue
    // referring to that domain; without it, the property set holds what the
    // schedule has, by its column names. Not owned.
    const survey::subsurface::DeliverySchema* schema = nullptr;
    std::string sourceName; // the schedule's file name, for provenance
};

struct ExportInput {
    const entity::Model* model = nullptr; // may be null: utilities or surfaces alone
    std::vector<SurfaceInput> surfaces;
    std::optional<UtilityInput> utilities;
};

struct ExportOptions {
    std::string projectName = "Untitled";
    std::string projectDescription;
    std::string siteName = "Site";
    // FILE_NAME in the header: the file's name, when it was written (ISO 8601,
    // the caller's clock - never read here, so that an export is a function
    // of its inputs), by whom, and the program that wrote it.
    std::string fileName;
    std::string timestamp = "1970-01-01T00:00:00";
    std::string author;
    std::string organisation;
    std::string applicationVersion;

    Georeference georeference;
    // Where the file's (0, 0, 0) is in the project's coordinates. nullopt:
    // chosen from the data - the south-west corner of everything exported,
    // rounded down to 100 m in plan, at height 0 - when there is a
    // coordinate system to record it in, and (0, 0, 0) when there is not,
    // since a shift nothing records would move the survey.
    std::optional<katana::math::Vec3> localOrigin;
    // Seeds every GlobalId (guidFor): the same project exported twice gives
    // each object the same GlobalId, and two projects different ones. The
    // application passes something that identifies the project; empty uses
    // the project name.
    std::string guidNamespace;

    // Only these entities, and only these layers; empty for all.
    std::vector<entity::EntityId> entities;
    std::vector<std::string> layers;
    // The rules drawing entities are classified by; empty uses
    // defaultClassificationRules().
    std::vector<ClassificationRule> rules;
    bool exportEntities = true;
    bool exportAlignments = true;
};

struct IfcExport {
    std::string text;              // the file (writeIfc); empty from writeIfcFile
    std::size_t bytesWritten = 0;
    std::size_t instances = 0;     // STEP entity instances
    // How many of each rooted class were written, by the schema's name:
    // "IfcPipeSegment" -> 12. What the report prints, and what a test
    // asserts "no proxies" with.
    std::map<std::string, std::size_t> classes;
    std::size_t alignments = 0;
    std::size_t services = 0;          // distribution systems from the schedule
    std::size_t serviceSegments = 0;   // graded segments written as elements
    std::size_t segmentsIn3d = 0;      // of which drawn with an Axis in 3D
    std::size_t locatedPoints = 0;     // IfcAnnotation SURVEY of the schedule
    std::size_t entitiesWritten = 0;   // drawing entities written
    std::size_t entitiesSkipped = 0;
    std::size_t surfaces = 0;
    std::vector<std::string> warnings;
};

// InvalidArgument for a model entity or alignment that cannot be written
// (a non-finite coordinate reaching the writer), with the object named.
[[nodiscard]] katana::core::Result<IfcExport> writeIfc(const ExportInput& input,
                                                       const ExportOptions& options = {});
// Writes to `path`, replacing it; the file is written under a temporary
// name and renamed into place, so a failed write leaves nothing behind.
// FileExportFailure when it cannot be written.
[[nodiscard]] katana::core::Result<IfcExport> writeIfcFile(const ExportInput& input,
                                                           const std::filesystem::path& path,
                                                           const ExportOptions& options = {});

// True for a path ending ".ifc", any letter case.
[[nodiscard]] bool isIfcPath(const std::filesystem::path& path);

} // namespace katana::ifc
