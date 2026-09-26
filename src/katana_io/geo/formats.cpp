// The formats of this GDAL build, from its driver manager (formats.hpp,
// docs/interop.md "Formats").

#include "katana/gis/formats.hpp"

#include <algorithm>
#include <cctype>
#include <map>
#include <mutex>
#include <set>
#include <string>
#include <system_error>
#include <utility>
#include <vector>

#include <cpl_conv.h>
#include <cpl_error.h>
#include <cpl_minixml.h>
#include <cpl_string.h>
#include <cpl_vsi.h>
#include <gdal_priv.h>

#include "gdal_registry.hpp"
#include "geo/formats_detail.hpp"
#include "katana/core/text.hpp"

namespace katana::gis {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

// ---- the curated overlay ------------------------------------------------------------------
//
// Everything else is GDAL's. Each entry says why Katana differs.

// Drivers that are no format a person opens or saves:
//   MEM       memory only, what the bridge builds its inputs in;
//   DERIVED   a syntax for VRT pixel functions over another dataset;
//   HTTP      GDAL's wrapper that fetches a URL for another driver - a URL
//             reaches it without being named;
//   AIVector  sends the data to a remote AI service to interpret it;
//   GPSBabel  runs the gpsbabel program, which Katana does not ship, for
//             both reading and writing.
// A driver of neither rasters nor vectors (GNM networks, CPHD) is left out
// by the rule in formats(), not by name.
constexpr const char* kHidden[] = {"MEM", "DERIVED", "HTTP", "AIVector", "GPSBabel"};

// Where Katana's writer for an extension is not GDAL's first choice.
//   .xml  GDAL picks PDS4 (NASA's planetary data); in a survey or GIS
//         office an .xml of features is GML.
constexpr std::pair<const char*, const char*> kWriterFor[] = {{"xml", "GML"}};

// Extensions a file dialog does not offer as data: a shapefile's or a
// raster's sidecars and headers, which are opened through the file they go
// with, and generic ones only a rare format claims (VDV's .txt, ZMap's .dat,
// NSIDC's .bin, and the many .xml), which would fill a filter with files
// that are not data.
constexpr const char* kNotOffered[] = {"dbf", "shx", "prj", "cpg", "mid", "hdr", "aux", "ovr",
                                       "tfw", "wld", "txt", "dat", "bin", "xml"};

// The formats a save dialog lists first, in this order: the ones a survey or
// CAD office exchanges every day; everything else follows by name.
constexpr const char* kSaveFirst[] = {"ESRI Shapefile", "GeoJSON", "GPKG",   "LIBKML",
                                      "DXF",            "GML",     "CSV",    "FlatGeobuf",
                                      "Parquet",        "GPX",     "MapInfo File",
                                      "SQLite",         "OpenFileGDB"};

// A save dialog entry for more than a format's first extension, where the
// second is a different kind of file to its reader: a KMZ is a zipped KML,
// and a zipped shapefile is one file to hand over instead of four.
struct SaveAs {
    const char* driver;
    const char* extension;
    const char* description;
};
constexpr SaveAs kSaveAs[] = {
    {"ESRI Shapefile", "shp", "ESRI Shapefile"},
    {"ESRI Shapefile", "shp.zip", "ESRI Shapefile, zipped"},
    {"GeoJSON", "geojson", "GeoJSON"},
    {"LIBKML", "kml", "KML"},
    {"LIBKML", "kmz", "KMZ (zipped KML)"},
    {"MapInfo File", "tab", "MapInfo TAB"},
};

// ---- helpers --------------------------------------------------------------------------------

std::string lower(std::string_view text)
{
    return katana::core::lowered(std::string(text));
}

bool sameDriver(std::string_view a, std::string_view b)
{
    return lower(a) == lower(b);
}

template <std::size_t N>
bool listed(const char* const (&names)[N], std::string_view name)
{
    return std::ranges::any_of(names, [name](const char* entry) { return name == entry; });
}

std::vector<std::string> words(const char* text)
{
    std::vector<std::string> out;
    if (text == nullptr) {
        return out;
    }
    const CPLStringList split(CSLTokenizeString2(text, " ", 0));
    for (int i = 0; i < split.size(); ++i) {
        out.push_back(lower(split[i]));
    }
    return out;
}

bool flag(GDALDriver& driver, const char* key)
{
    const char* value = driver.GetMetadataItem(key);
    return value != nullptr && CPLTestBool(value);
}

// A name for GDAL: UTF-8, forward slashes (GDAL_FILENAME_IS_UTF8 is on by
// default, and a slash is a separator everywhere).
std::string gdalName(const std::filesystem::path& path)
{
    const std::string text = path.string();
    if (text.starts_with("/vsi") || text.find("://") != std::string::npos) {
        return text; // GDAL's own syntax; left exactly as written
    }
    const std::u8string generic = path.generic_u8string();
    return std::string(generic.begin(), generic.end());
}

// The file's extension, compound ones whole where a format claims them:
// "lots.shp.zip" is "shp.zip" to a registry that lists it, "zip" otherwise.
std::string extensionOf(const std::filesystem::path& path)
{
    const std::string name = lower(path.filename().string());
    std::string best;
    for (const Format& format : formats()) {
        for (const std::string& extension : format.extensions) {
            if (extension.size() > best.size() && name.size() > extension.size() + 1 &&
                name.ends_with("." + extension)) {
                best = extension;
            }
        }
    }
    if (!best.empty()) {
        return best;
    }
    const std::string plain = path.extension().string();
    return plain.empty() ? std::string() : lower(plain.substr(1));
}

Format readFormat(GDALDriver& driver)
{
    Format format;
    format.driver = driver.GetDescription();
    const char* longName = driver.GetMetadataItem(GDAL_DMD_LONGNAME);
    format.description = longName != nullptr ? longName : format.driver;
    format.raster = flag(driver, GDAL_DCAP_RASTER);
    format.vector = flag(driver, GDAL_DCAP_VECTOR);
    const bool open = flag(driver, GDAL_DCAP_OPEN);
    const bool create = flag(driver, GDAL_DCAP_CREATE);
    const bool copy = flag(driver, GDAL_DCAP_CREATECOPY);
    format.readRaster = format.raster && open;
    format.readVector = format.vector && open;
    // GDAL has no per-kind "create" for a driver of both kinds. A vector
    // writer is one that creates layers and their fields, which is what
    // writeTables does (MBTiles declares the fields and not the layers; S-57,
    // whose writer needs a catalogue of its own, neither). A driver of both
    // kinds writes rasters when it declares the data types it creates them
    // in - a file geodatabase declares none, since it reads a geodatabase's
    // rasters and writes only its tables. A driver of rasters alone writes
    // what it creates or copies, declared types or not (XYZ).
    format.writeVector = format.vector && create &&
                         (flag(driver, GDAL_DCAP_CREATE_LAYER) || flag(driver, GDAL_DCAP_CREATE_FIELD));
    const char* rasterTypes = driver.GetMetadataItem(GDAL_DMD_CREATIONDATATYPES);
    const bool declaresTypes = rasterTypes != nullptr && *rasterTypes != '\0';
    format.writeRaster =
        format.raster && (create || copy) && (!format.vector || declaresTypes);
    format.virtualIo = flag(driver, GDAL_DCAP_VIRTUALIO);
    format.extensions = words(driver.GetMetadataItem(GDAL_DMD_EXTENSIONS));
    if (format.extensions.empty()) {
        format.extensions = words(driver.GetMetadataItem(GDAL_DMD_EXTENSION));
    }
    if (const char* prefix = driver.GetMetadataItem(GDAL_DMD_CONNECTION_PREFIX)) {
        format.connectionPrefix = prefix;
    }
    if (const char* topic = driver.GetMetadataItem(GDAL_DMD_HELPTOPIC)) {
        const std::string page = topic;
        // GDAL gives a page of its site ("drivers/vector/gpkg.html").
        format.helpUrl = page.starts_with("http") ? page : "https://gdal.org/" + page;
    }
    return format;
}

std::vector<FormatOption> parseOptions(const char* xml)
{
    std::vector<FormatOption> options;
    if (xml == nullptr || *xml == '\0') {
        return options;
    }
    CPLXMLNode* root = CPLParseXMLString(xml);
    if (root == nullptr) {
        return options;
    }
    for (const CPLXMLNode* list = root; list != nullptr; list = list->psNext) {
        if (list->eType != CXT_Element) {
            continue;
        }
        for (const CPLXMLNode* node = list->psChild; node != nullptr; node = node->psNext) {
            if (node->eType != CXT_Element || !EQUAL(node->pszValue, "Option")) {
                continue;
            }
            FormatOption option;
            option.name = CPLGetXMLValue(node, "name", "");
            option.type = CPLGetXMLValue(node, "type", "");
            option.description = CPLGetXMLValue(node, "description", "");
            option.defaultValue = CPLGetXMLValue(node, "default", "");
            option.scope = CPLGetXMLValue(node, "scope", "");
            option.min = katana::core::parseFiniteDouble(CPLGetXMLValue(node, "min", ""));
            option.max = katana::core::parseFiniteDouble(CPLGetXMLValue(node, "max", ""));
            for (const CPLXMLNode* value = node->psChild; value != nullptr; value = value->psNext) {
                if (value->eType == CXT_Element && EQUAL(value->pszValue, "Value")) {
                    option.choices.emplace_back(CPLGetXMLValue(value, nullptr, ""));
                }
            }
            if (!option.name.empty()) {
                options.push_back(std::move(option));
            }
        }
    }
    CPLDestroyXMLNode(root);
    return options;
}

// Errors of GDAL's own raised while this is alive stay here: an
// identification tries names that fail on purpose.
struct QuietErrors {
    QuietErrors()
    {
        CPLErrorReset();
        CPLPushErrorHandler(CPLQuietErrorHandler);
    }
    ~QuietErrors() { CPLPopErrorHandler(); }
    QuietErrors(const QuietErrors&) = delete;
    QuietErrors& operator=(const QuietErrors&) = delete;
};

// Whether the dataset holds rasters: bands, or raster subdatasets (a
// GeoPackage of several tile tables has no bands of its own).
bool holdsRaster(GDALDataset& dataset)
{
    if (dataset.GetRasterCount() > 0) {
        return true;
    }
    const char* const* subdatasets = dataset.GetMetadata("SUBDATASETS");
    return subdatasets != nullptr && *subdatasets != nullptr;
}

} // namespace

// ---- the registry ---------------------------------------------------------------------------

const std::vector<Format>& formats()
{
    static const std::vector<Format> all = [] {
        detail::ensureGdalRegistered();
        std::vector<Format> out;
        GDALDriverManager* manager = GetGDALDriverManager();
        for (int i = 0; i < manager->GetDriverCount(); ++i) {
            GDALDriver* driver = manager->GetDriver(i);
            if (driver == nullptr || listed(kHidden, driver->GetDescription())) {
                continue;
            }
            Format format = readFormat(*driver);
            if (!format.raster && !format.vector) {
                continue; // neither kind of data: a network model, a phase history
            }
            out.push_back(std::move(format));
        }
        std::ranges::sort(out, [](const Format& a, const Format& b) {
            const std::string left = lower(a.driver);
            const std::string right = lower(b.driver);
            return left != right ? left < right : a.driver < b.driver;
        });
        return out;
    }();
    return all;
}

const Format* findFormat(std::string_view driver)
{
    for (const Format& format : formats()) {
        if (sameDriver(format.driver, driver)) {
            return &format;
        }
    }
    return nullptr;
}

Result<FormatOptions> formatOptions(std::string_view driver)
{
    const Format* format = findFormat(driver);
    if (format == nullptr) {
        return makeError(ErrorCode::NotFound, "this GDAL build has no format of that name",
                         std::string(driver));
    }
    GDALDriver* gdal = GetGDALDriverManager()->GetDriverByName(format->driver.c_str());
    if (gdal == nullptr) {
        return makeError(ErrorCode::NotFound, "this GDAL build has no format of that name",
                         std::string(driver));
    }
    FormatOptions options;
    options.open = parseOptions(gdal->GetMetadataItem(GDAL_DMD_OPENOPTIONLIST));
    options.creation = parseOptions(gdal->GetMetadataItem(GDAL_DMD_CREATIONOPTIONLIST));
    options.layerCreation = parseOptions(gdal->GetMetadataItem(GDAL_DS_LAYER_CREATIONOPTIONLIST));
    return options;
}

std::vector<std::string> readableExtensions(DataKind kind)
{
    std::set<std::string> extensions;
    for (const Format& format : formats()) {
        const bool reads = kind == DataKind::Raster ? format.readRaster : format.readVector;
        if (!reads) {
            continue;
        }
        for (const std::string& extension : format.extensions) {
            if (!listed(kNotOffered, extension)) {
                extensions.insert(extension);
            }
        }
    }
    return {extensions.begin(), extensions.end()};
}

Result<std::string> vectorWriterFor(const std::filesystem::path& path)
{
    const std::string extension = extensionOf(path);
    if (extension.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "cannot choose a vector driver: the path has no extension", path.string());
    }
    const auto offered = [](const char* driver) -> const Format* {
        const Format* format = findFormat(driver);
        return format != nullptr && format->writeVector ? format : nullptr;
    };
    for (const auto& [claimed, driver] : kWriterFor) {
        if (extension == claimed) {
            if (const Format* format = offered(driver)) {
                return format->driver;
            }
        }
    }
    // GDAL's own ranking of the writers that claim the name (LIBKML before
    // KML for a .kml, GeoJSON before JSON-FG for a .json), the first Katana
    // offers taken.
    const std::string name = "katana." + extension;
    const CPLStringList candidates(
        GDALGetOutputDriversForDatasetName(name.c_str(), GDAL_OF_VECTOR, FALSE, FALSE));
    for (int i = 0; i < candidates.size(); ++i) {
        if (const Format* format = offered(candidates[i])) {
            return format->driver;
        }
    }
    return makeError(ErrorCode::Unsupported,
                     "no vector driver is registered for '." + extension + "'", path.string());
}

std::vector<SaveChoice> vectorSaveChoices()
{
    std::vector<SaveChoice> choices;
    for (const Format& format : formats()) {
        if (!format.writeVector) {
            continue;
        }
        const auto writesIt = [&format](const std::string& extension) {
            const auto writer = vectorWriterFor("katana." + extension);
            return writer.ok() && *writer == format.driver;
        };
        bool curated = false;
        for (const SaveAs& entry : kSaveAs) {
            if (format.driver == entry.driver && writesIt(entry.extension)) {
                choices.push_back(SaveChoice{entry.description, entry.extension, format.driver});
                curated = true;
            }
        }
        if (curated) {
            continue;
        }
        for (const std::string& extension : format.extensions) {
            if (!listed(kNotOffered, extension) && writesIt(extension)) {
                choices.push_back(SaveChoice{format.description, extension, format.driver});
                break;
            }
        }
    }
    const auto rank = [](const SaveChoice& choice) {
        const auto* found = std::ranges::find_if(
            kSaveFirst, [&choice](const char* driver) { return choice.driver == driver; });
        return static_cast<std::size_t>(found - std::ranges::begin(kSaveFirst));
    };
    std::ranges::stable_sort(choices, [&rank](const SaveChoice& a, const SaveChoice& b) {
        const std::size_t left = rank(a);
        const std::size_t right = rank(b);
        if (left != right) {
            return left < right;
        }
        return lower(a.description) < lower(b.description);
    });
    return choices;
}

// ---- paths GDAL opens -----------------------------------------------------------------------

bool isRemotePath(const std::filesystem::path& path)
{
    const std::string text = path.string();
    // A URL's scheme (RFC 3986, 3.1): letters, digits, '+', '-', '.'; more
    // than one letter, so no drive ("C:") is taken for one.
    const std::size_t scheme = text.find("://");
    if (scheme != std::string::npos && scheme > 1 &&
        std::all_of(text.begin(), text.begin() + static_cast<std::ptrdiff_t>(scheme), [](char c) {
            return std::isalnum(static_cast<unsigned char>(c)) != 0 || c == '+' || c == '-' ||
                   c == '.';
        })) {
        return true;
    }
    for (const char* prefix : {"/vsicurl", "/vsis3", "/vsigs", "/vsiaz", "/vsiadls", "/vsioss",
                               "/vsiswift", "/vsiwebhdfs", "/vsihdfs"}) {
        if (text.starts_with(prefix)) {
            return true;
        }
    }
    return false;
}

bool isVirtualPath(const std::filesystem::path& path)
{
    const std::string text = path.string();
    if (text.starts_with("/vsi") || isRemotePath(path)) {
        return true;
    }
    // A connection string: a prefix of two letters or more, so a Windows
    // drive ("C:") is never taken for one.
    for (const Format& format : formats()) {
        const std::string& prefix = format.connectionPrefix;
        if (prefix.size() > 2 && text.size() >= prefix.size() &&
            lower(text.substr(0, prefix.size())) == lower(prefix)) {
            return true;
        }
    }
    return false;
}

namespace detail {

OpenNames openNames(const std::filesystem::path& path)
{
    OpenNames names;
    const std::string name = gdalName(path);
    names.names.push_back(name);
    if (name.starts_with("/vsi") || isRemotePath(path)) {
        return names; // already GDAL's own words for where to look
    }
    const std::string file = lower(path.filename().string());
    std::string root;
    if (file.ends_with(".zip") || file.ends_with(".kmz")) {
        root = "/vsizip/{" + name + "}";
    } else if (file.ends_with(".tar") || file.ends_with(".tgz") || file.ends_with(".tar.gz")) {
        root = "/vsitar/{" + name + "}";
    } else if (file.ends_with(".gz")) {
        // One file compressed: its inside IS the dataset. /vsigzip takes no
        // braces (measured: "/vsigzip/{<path>}" opens nothing); it has no
        // member path after the file to tell apart from the file's own.
        names.names.push_back("/vsigzip/" + name);
        return names;
    } else {
        return names;
    }
    // The archive as a folder first: a shapefile's four files, or several
    // shapefiles, open so as the layers of one dataset.
    names.names.push_back(root);
    // Otherwise the one member that is a dataset. Several are not guessed
    // between: the person names one.
    const QuietErrors quiet;
    const CPLStringList members(VSIReadDirRecursive(root.c_str()));
    std::vector<std::string> datasets;
    for (int i = 0; i < members.size(); ++i) {
        const std::string member = members[i];
        if (member.empty() || member.ends_with("/")) {
            continue;
        }
        const std::string extension = extensionOf(member);
        const bool data = !extension.empty() && !listed(kNotOffered, extension) &&
                          std::ranges::any_of(formats(), [&extension](const Format& format) {
                              return format.reads() &&
                                     std::ranges::find(format.extensions, extension) !=
                                         format.extensions.end();
                          });
        if (data) {
            datasets.push_back(member);
        }
    }
    if (datasets.size() == 1) {
        names.names.push_back(root + "/" + datasets.front());
    } else if (datasets.size() > 1) {
        std::string list;
        for (const std::string& member : datasets) {
            list += (list.empty() ? "" : ", ") + member;
        }
        names.ambiguity = "the archive holds " + std::to_string(datasets.size()) +
                          " datasets (" + list + "); name one as " + root + "/<member>";
    }
    return names;
}

} // namespace detail

Result<Content> identifyContent(const std::filesystem::path& path)
{
    detail::ensureGdalRegistered();
    std::error_code existsError;
    if (!isVirtualPath(path) && !std::filesystem::exists(path, existsError)) {
        return makeError(ErrorCode::NotFound, "file does not exist", path.string());
    }
    const detail::OpenNames names = detail::openNames(path);
    const QuietErrors quiet;
    std::string reason;
    for (const std::string& name : names.names) {
        GDALDriverH identified =
            GDALIdentifyDriverEx(name.c_str(), GDAL_OF_RASTER | GDAL_OF_VECTOR, nullptr, nullptr);
        if (identified == nullptr) {
            if (reason.empty() && CPLGetLastErrorMsg() != nullptr) {
                reason = CPLGetLastErrorMsg();
            }
            continue;
        }
        auto* driver = GDALDriver::FromHandle(identified);
        Content content;
        content.driver = driver->GetDescription();
        content.name = name;
        const bool raster = flag(*driver, GDAL_DCAP_RASTER);
        const bool vector = flag(*driver, GDAL_DCAP_VECTOR);
        if (raster != vector) {
            // A driver of one kind: its word is enough, and a large file is
            // not read twice (GeoJSON reads a whole file to open it).
            content.raster = raster;
            content.vector = vector;
            return content;
        }
        GDALDataset* dataset = GDALDataset::Open(
            name.c_str(), GDAL_OF_READONLY | GDAL_OF_RASTER | GDAL_OF_VECTOR, nullptr, nullptr,
            nullptr);
        if (dataset == nullptr) {
            if (CPLGetLastErrorMsg() != nullptr && *CPLGetLastErrorMsg() != '\0') {
                reason = CPLGetLastErrorMsg();
            }
            continue;
        }
        content.driver = dataset->GetDriver() != nullptr ? dataset->GetDriver()->GetDescription()
                                                         : content.driver;
        content.raster = holdsRaster(*dataset);
        content.vector = dataset->GetLayerCount() > 0;
        GDALClose(dataset);
        return content;
    }
    if (!names.ambiguity.empty()) {
        return makeError(ErrorCode::InvalidArgument, names.ambiguity, path.string());
    }
    return makeError(ErrorCode::FileImportFailure,
                     "GDAL recognises no format in '" + path.string() + "'", reason);
}

} // namespace katana::gis
