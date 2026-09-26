#pragma once

// The formats this build of GDAL reads and writes, read from GDAL's own
// driver manager rather than from tables of extensions kept by hand
// (docs/interop.md, "Formats"). Three hand-kept tables - the extensions the
// import dialog offered, the drivers EXPORT wrote with, the formats the save
// dialog listed - had drifted apart (a CSV could be written and not read) and
// offered ECW, for which the toolchain has no driver, while FlatGeobuf,
// GeoParquet, KMZ, GPX and a zipped shapefile, all of which GDAL has, could
// not be opened at all.
//
// What GDAL knows comes from the registry; what Katana prefers is a small
// curated overlay in formats.cpp, each entry with its reason:
//   - drivers that are no format at all are hidden (MEM, DERIVED, HTTP ...);
//   - where two writers claim an extension, GDAL's own choice for the name
//     (GDALGetOutputDriversForDatasetName, what `gdal vector convert` uses)
//     decides, and the overlay only where Katana's differs;
//   - a writer that needs another program (GPSBabel) is not offered.
//
// And a file is routed by what it HOLDS, found by GDAL, not by the letters
// after its dot: a GeoPackage of raster tiles is a raster, a .zip of a
// shapefile is vector data, a file with an extension no table knows is
// whatever GDAL finds in it.
//
// No GDAL type appears here (docs/interop.md, "Layering").

#include <filesystem>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::gis {

// One of GDAL's drivers, as Katana offers it.
struct Format {
    std::string driver;      // GDAL's short name: "GPKG", "ESRI Shapefile"
    std::string description; // GDAL's long name: "GeoPackage"
    bool raster = false;     // holds rasters
    bool vector = false;     // holds vector layers
    bool readRaster = false;
    bool readVector = false;
    // Writes a raster: creates one, or copies a finished one (PNG, AAIGrid),
    // which GdalDataset::writeRaster does by building it in memory first.
    bool writeRaster = false;
    // Writes vector layers: creates a dataset and layers in it. A driver
    // that can only copy a whole dataset (GMLAS) cannot take Katana's tables.
    bool writeVector = false;
    // Opens a path inside an archive or on the web (/vsizip, /vsicurl).
    bool virtualIo = false;
    // Lower case, without the dot, in GDAL's order; compound ones whole
    // ("shp.zip"). Empty for a database or a web service.
    std::vector<std::string> extensions;
    // The prefix of a connection string the driver opens ("PG:"), for one
    // that is no file.
    std::string connectionPrefix;
    std::string helpUrl; // GDAL's page for it; empty when it names none

    [[nodiscard]] bool reads() const { return readRaster || readVector; }
    [[nodiscard]] bool writes() const { return writeRaster || writeVector; }
};

// Every format of this build that Katana offers, in the order of their
// driver names ignoring case - stable from one run to the next, unlike
// GDAL's registration order, which depends on the plugins found. Read once.
[[nodiscard]] const std::vector<Format>& formats();

// One format by its driver name, ignoring case; nullptr when this build has
// no such driver, or Katana does not offer it.
[[nodiscard]] const Format* findFormat(std::string_view driver);

// One option a driver takes, as GDAL declares it.
struct FormatOption {
    std::string name;
    // GDAL's own: "boolean", "int", "integer", "float", "string",
    // "string-select" ... as the driver spells it.
    std::string type;
    std::string description;
    std::string defaultValue; // empty when GDAL declares none
    // "raster", "vector" or "raster,vector" where the driver says which
    // kind of data the option is for; empty when it says nothing.
    std::string scope;
    std::vector<std::string> choices; // a string-select's values
    std::optional<double> min;
    std::optional<double> max;
};

// A driver's options for opening a dataset (IMPORT's oo=), creating one
// (EXPORT's co=) and creating a layer in one (lco=): what an option given
// to GDAL is checked against, and what a person is shown.
struct FormatOptions {
    std::vector<FormatOption> open;
    std::vector<FormatOption> creation;
    std::vector<FormatOption> layerCreation;
};

// NotFound for a driver findFormat does not find.
[[nodiscard]] katana::core::Result<FormatOptions> formatOptions(std::string_view driver);

// Which of a driver's lists an option is checked against: IMPORT's oo=,
// EXPORT's co= and lco=.
enum class OptionList { Open, Creation, LayerCreation };

// `given`, KEY=VALUE each, checked against the driver's own list of that
// kind (formatOptions): InvalidArgument naming the first that is not
// KEY=VALUE, whose key the driver does not declare - listing the keys it
// does - or whose value is not one of a string-select's choices. GDAL itself
// only warns of an option it does not know and carries on without it, so a
// misspelt one (RFC7964=YES) was written as if it had worked. Keys and
// choices ignore case, as GDAL's do. A driver that declares no list of that
// kind is refused any option, saying so. NotFound for a driver findFormat
// does not find. Nothing given is always good.
[[nodiscard]] katana::core::Status checkOptions(std::string_view driver, OptionList which,
                                                const std::vector<std::string>& given);

enum class DataKind { Raster, Vector };

// The extensions of the formats that read data of `kind`, for a file
// dialog's filter: sorted, each once, without the ones a sidecar or a
// header file has (.dbf, .shx, .prj, .hdr ...) and without the generic ones
// only a rare format claims (.txt, .dat, .bin), which would fill the filter
// with files that are not data. A file of any extension is still opened by
// what it holds when a person picks it through "All files".
[[nodiscard]] std::vector<std::string> readableExtensions(DataKind kind);

// What a save dialog offers for vector data: one entry per format and file
// kind a person saves as (KML and KMZ are two, a shapefile and a zipped one
// two), the common formats first. `driver` is what EXPORT writes that
// extension with (vectorWriterFor).
struct SaveChoice {
    std::string description;
    std::string extension; // without the dot
    std::string driver;
};
[[nodiscard]] std::vector<SaveChoice> vectorSaveChoices();

// The driver EXPORT writes a vector path with: GDAL's own choice for the
// name - its extension, compound ones whole (".shp.zip" is a zipped
// shapefile, ".kml" LIBKML) - where Katana's overlay does not differ.
// InvalidArgument for a path with no extension; Unsupported, naming the
// extension, when no writer Katana offers claims it.
[[nodiscard]] katana::core::Result<std::string> vectorWriterFor(const std::filesystem::path& path);

// True for a name GDAL opens that is no file on this disk: a /vsi path
// (/vsizip/, /vsicurl/ ...), a URL (https://, s3:// ...), or a connection
// string one of GDAL's drivers takes ("PG:dbname=..."). Whether one
// exists is GDAL's to find out; std::filesystem cannot say.
[[nodiscard]] bool isVirtualPath(const std::filesystem::path& path);

// True for a web address (http://, https://, ftp://, s3:// ...), or a
// /vsicurl-style path to one: opening it costs a round trip to a server.
[[nodiscard]] bool isRemotePath(const std::filesystem::path& path);

// What a dataset holds, found by GDAL: the driver that recognises it, and
// whether it holds rasters (bands, or raster subdatasets), vector layers or
// both. The driver's identification alone decides where the driver holds
// one kind only; a driver of both kinds (GeoPackage, MBTiles, PDF, a file
// geodatabase) has the dataset opened to see which it holds. An archive GDAL
// does not open as it is (a .zip of a shapefile) is looked into, and `name`
// says how it is opened. NotFound for a local path that does not exist;
// FileImportFailure, with GDAL's message, for a file no driver recognises.
struct Content {
    std::string driver;
    std::string name; // what GDAL opens: the path, or /vsizip/<path>[/<member>]
    bool raster = false;
    bool vector = false;
};
[[nodiscard]] katana::core::Result<Content> identifyContent(const std::filesystem::path& path);

} // namespace katana::gis
