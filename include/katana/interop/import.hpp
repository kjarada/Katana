#pragma once

// Import: external files -> the internal domain model (PLAN.MD Phase 20,
// "All importers should convert external data into the internal domain model").
//
// Nothing here writes to a Model. Vector import produces plain Entity values and
// the caller wraps them in commands::createEntities, so an import is one
// validated, atomic, undoable command like every other change. Raster and point
// cloud import produce reference data, which is not undoable by design - see
// reference_data.hpp for why.

#include <cstdint>
#include <filesystem>
#include <map>
#include <optional>
#include <string>
#include <vector>

#include "katana/entity/entity.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/gis/processing.hpp"
#include "katana/gis/reproject.hpp"
#include "katana/interop/reference_data.hpp"

namespace katana::interop {

// Archive12d is a 12d Archive (.12da or .12daz): not a vector file, since
// one can carry surfaces, alignments and point clouds too. See archive12d.hpp.
enum class SourceKind { Unknown, Vector, Raster, PointCloud, Archive12d };

[[nodiscard]] const char* toString(SourceKind kind);

// Which importer a path goes to (docs/interop.md, "Formats"). A .12da archive
// and a point cloud are known by their extensions (.12da, .las ...). Anything
// else is routed by what it HOLDS, found by GDAL (gis::identifyContent) when
// the path can be looked at - a local file or folder, a /vsi path: a
// GeoPackage of raster tiles is a Raster, a .zip of a shapefile Vector, a
// file with an unknown extension whatever GDAL finds in it. A path that
// cannot be looked at (not written yet, or a URL, whose look costs a round
// trip) is routed by its name, from GDAL's registry of extensions.
[[nodiscard]] SourceKind kindForPath(const std::filesystem::path& path);

// Extensions offered in the open dialog, without the leading dot. The vector
// and raster ones are those of GDAL's readers (gis::readableExtensions), so
// they are what this build of GDAL opens; the archives are those whose
// inside is opened (.zip, .tar, .tgz, .gz).
[[nodiscard]] std::vector<std::string> vectorExtensions();
[[nodiscard]] std::vector<std::string> rasterExtensions();
[[nodiscard]] std::vector<std::string> archiveExtensions();
[[nodiscard]] std::vector<std::string> pointCloudExtensions();

// ---- vector ---------------------------------------------------------------

struct VectorImportOptions {
    // Katana layer the imported entities are placed on. Empty means "one layer
    // per source layer, named after it", which is what a multi-layer GeoPackage
    // should do.
    std::string targetLayer;
    // A feature attribute that names the layer each entity belongs on,
    // matched case-insensitively; used only when `targetLayer` is empty.
    //
    // A DXF is ONE source layer to GDAL ("entities"), and the CAD layer of
    // each entity arrives as its `Layer` attribute - so without this a
    // 50 000-entity drawing lands on a single layer called "entities" and the
    // layer tree is useless. Katana's own exports write the same information
    // as `layer`, which the case-insensitive match also picks up, so a round
    // trip through GeoJSON or GeoPackage keeps its layers too. A feature
    // without the attribute, or with a blank one, falls back to the source
    // layer's name. Empty disables the lookup.
    std::string layerAttribute = "layer";
    // -1 imports every layer in the dataset.
    int sourceLayerIndex = -1;
    // 0 imports them all; otherwise at most this many of the file's features.
    std::uint64_t maxFeatures = 0;
    // Copy feature attributes onto the entities as properties, typed as the
    // file types them: integers, reals, booleans, text, and dates as ISO
    // 8601 text.
    bool attributesAsProperties = true;
    // A curve that is one arc or one circle becomes an Arc or a Circle; any
    // other curve (a line of several arcs) is chords, none further than this
    // from the curve, in the file's units - the 1 mm sagitta EXPORT uses.
    double curveTolerance = 0.001;
    // Subtracted from every coordinate. Survey data often sits at coordinates
    // where a double has only ~0.1 mm of resolution left; shifting to a local
    // origin restores precision for downstream editing. Recorded in the result
    // so the same shift can be undone on export.
    std::optional<katana::geometry::Vec2> originShift;
    // Moves every feature into this CRS (anything gis::crsToWkt reads:
    // "EPSG:7856", a WKT) before it becomes an entity. Empty, the default,
    // leaves coordinates as the file has them - a file import never
    // reprojects (docs/interop.md, "No reprojection"). The online import sets
    // it, because data a web service chose the CRS of is useless until it is
    // in the project's (docs/gis_online.md).
    std::string targetCrs;
    // The CRS of a layer that declares none, for targetCrs; empty refuses
    // such a layer rather than guess.
    std::string assumedSourceCrs;
    // The CRS the file's coordinates are in, whatever its layers declare
    // (IMPORT's srs=, as ogr2ogr's -s_srs): GDAL declares a GeoJSON without a
    // crs member WGS 84 (RFC 7946), so a fallback for layers that declare
    // none never reached one, and MGA coordinates were moved as degrees. A
    // layer that declares another system is said in a warning.
    std::string sourceCrs;
    // Keeps only features whose bounding box meets this box, given in the
    // source's own CRS (so before any reprojection): the online import's area
    // applied to a file that covers the world (Natural Earth).
    std::optional<katana::gis::CrsBox> sourceFilter;

    // ---- IMPORT's options (docs/interop.md, "Import options") ----
    // Only these of the file's layers, by name, ignoring case; empty reads
    // every layer (or sourceLayerIndex's). NotFound, naming the layers the
    // file has, for a name it does not. layers=.
    std::vector<std::string> layerNames;
    // An OGR SQL WHERE clause on every layer read ("kind = 'lot'"), which the
    // driver applies (gis::VectorReadOptions::attributeFilter). where=.
    std::string attributeFilter;
    // A SELECT run on the dataset, whose rows are imported in place of its
    // layers, as one layer named after the file; `sqlDialect` is ogrsql,
    // sqlite or empty for the driver's own. Refused with layerNames. sql=.
    std::string sql;
    std::string sqlDialect;
    // Only the features whose box meets this box, given in the DRAWING's
    // coordinates - targetCrs's when it is set, the file's own otherwise -
    // and handed to the driver as its spatial filter, moved into each layer's
    // CRS first when the import reprojects. IMPORT's scope.
    std::optional<katana::gis::CrsBox> area;
    // Features cut at the edges of `area` (clip with an AREA or a VIEW), or
    // by the areas of `clipShapes` (clip with the closed shapes a scope
    // takes), in the drawing's coordinates, by GDAL's vector clip. A feature
    // wholly outside goes; one across the edge keeps the part inside.
    bool clipToArea = false;
    std::optional<katana::gis::processing::FeatureSet> clipShapes;
    // Only these attributes become properties, matched ignoring case; empty
    // keeps every one. InvalidArgument naming a field no layer read has.
    // fields=.
    std::vector<std::string> fields;
    // The driver's open options, KEY=VALUE, each checked against the list
    // the driver declares (gis::checkOptions) before the file is opened with
    // them. oo=.
    std::vector<std::string> openOptions;
};

// Whether imported data sits so far from the drawing it is joining that the two
// cannot usefully be seen together (PLAN.MD Phase 20).
//
// This is the survey case, not a corner case. A DXF or GeoPackage in a
// projected CRS carries coordinates like (255440, 7410850); a drawing started
// from scratch sits near the origin. Merging them silently produces a drawing
// whose extents span seven million metres, in which the original content is a
// dot smaller than a pixel - and nothing says so, which is the kind of quiet
// wrongness PLAN.MD section 36 forbids.
struct PlacementAdvice {
    // True when one of the two would be invisible at a zoom that shows both.
    bool farApart = false;
    // Subtract from every imported coordinate to bring it alongside the
    // drawing. Zero when they already share a neighbourhood.
    katana::geometry::Vec2 suggestedShift{};
    // Distance between the two bounding boxes, 0 when they overlap.
    double separation = 0.0;
    // Ready to show. Empty when there is nothing worth saying.
    std::string message;
};

// `existing` may be empty, which is the common case of importing into a new
// drawing; the advice is then never "far apart", because there is nothing for
// the data to be far FROM.
[[nodiscard]] PlacementAdvice advisePlacement(const katana::geometry::Box2& existing,
                                              const katana::geometry::Box2& incoming);

struct VectorImportResult {
    std::vector<katana::entity::Entity> entities;
    // Katana layers the entities reference, in first-use order. The caller must
    // create any that do not exist yet, BEFORE executing the create command, or
    // the model will reject entities on an unknown layer.
    std::vector<std::string> layersNeeded;

    katana::geometry::Box2 bounds;
    std::uint64_t featuresRead = 0;    // the file's features read (and in the area)
    // The features the layers read hold, before any filter: the M of IMPORT
    // PREVIEW's "matched N of M" (N is featuresRead). For sql=, every layer's.
    std::uint64_t featuresInFile = 0;
    // Whether a filter, a scope or a statement chose what was read: a
    // result with no entities is then an answer ("matched nothing"), not a
    // failure to read the file.
    bool filtered = false;
    std::uint64_t featuresSkipped = 0; // of those, the ones no entity came of
    // Geometries left out, by what they were: "tin", "polyhedral surface",
    // "unsupported <GDAL's type>".
    std::map<std::string, std::uint64_t> skipped;
    std::string projectionWkt;
    // Non-fatal problems: unsupported geometry types, empty geometries,
    // attributes that could not be represented, and GDAL's own warnings.
    // Never silently dropped (PLAN.MD section 36).
    std::vector<std::string> warnings;
};

[[nodiscard]] katana::core::Result<VectorImportResult>
importVector(const std::filesystem::path& path, const VectorImportOptions& options = {});

// ---- raster ---------------------------------------------------------------

struct RasterImportOptions {
    // Neither dimension of the decimated display copy exceeds this. 4096 keeps
    // a full-screen overlay sharp while bounding memory at 64 MB of RGBA.
    int maxPixels = 4096;
    std::string name; // empty: the file stem
    // One band alone, 1-based, as grey (or through its colour table); 0 shows
    // the bands as the file says they make a picture. band=.
    int band = 0;
    // A dataset inside a container (a netCDF variable, a GeoPackage raster
    // table): its number, 1-based, in the order INFO lists them, or its name
    // - GDAL's whole name, or the part after its last ':'. Empty reads the
    // file itself. InvalidArgument listing what the file holds for one it
    // does not. subdataset=.
    std::string subdataset;
    // The coordinate system of a raster whose file declares none, recorded
    // on the reference layer; empty leaves it without. srs=.
    std::string assumedCrs;
};

[[nodiscard]] katana::core::Result<RasterOverlay>
importRaster(const std::filesystem::path& path, const RasterImportOptions& options = {});

// ---- point cloud ----------------------------------------------------------

struct PointCloudImportOptions {
    // Points to keep. The importer reads the header first and picks a
    // decimation step that meets this budget, so opening a billion-point file
    // costs the same as opening a small one.
    std::uint64_t budget = 2'000'000;
    std::optional<std::uint8_t> classification;
    std::optional<katana::pointcloud::PointCloudBounds> clip;
    // COPC only: read the octree levels no finer than this point spacing, in
    // the cloud's units, instead of decimating (PLAN.MD Phase 17). The budget
    // still caps what is kept. Refused with InvalidArgument for a file that
    // is not COPC, as PointCloudReadOptions::resolution is.
    std::optional<double> resolution;
    std::string name; // empty: the file stem
};

[[nodiscard]] katana::core::Result<PointCloudLayer>
importPointCloud(const std::filesystem::path& path, const PointCloudImportOptions& options = {});

// Turning a cloud into a surface is in terrain_io.hpp, with the other
// conversions between terrain and GDAL/PDAL data.

} // namespace katana::interop
