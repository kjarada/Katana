// Every driver (docs/interop.md, "Formats"): the formats come from GDAL's
// own registry, a file is routed by what it holds, /vsi paths and archives
// open, and FORMATS says what this build reads and writes.
//
// What a format can do is GDAL's to say, so the expectations here are either
// read from GDAL's registry at run time (where a build may differ) or are
// facts of GDAL's drivers as its documentation states them - a GeoPackage
// holds rasters and vectors and GDAL writes both; GDAL has no ECW reader
// without the vendor's SDK - and the geometry read back is compared with the
// coordinates written, those of KMZ and GPX through PROJ.

#include <algorithm>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <iterator>
#include <string>
#include <variant>
#include <vector>

#include <gtest/gtest.h>
#include <nlohmann/json.hpp>

#include "geo/formats_verbs.hpp"
#include "geo/geo_verbs.hpp"
#include "geo/replies.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/entity/model.hpp"
#include "katana/gis/formats.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/gis/reproject.hpp"
#include "katana/gis/zip_container.hpp"
#include "katana/interop/export.hpp"
#include "katana/interop/import.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"

namespace {

namespace gis = katana::gis;
namespace geo = katana::app::geo;
namespace interop = katana::interop;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::entity::Model;
using katana::entity::PropertyValue;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;

const std::string kData = KATANA_GEO_TEST_DATA;

// The MGA zone 56 of GDA94: projected, metres, what a survey in Sydney is in.
constexpr const char* kMga56 = "EPSG:28356";

class TempDir {
  public:
    explicit TempDir(const std::string& name)
        : path_(std::filesystem::temp_directory_path() / ("katana-formats-" + name))
    {
        std::error_code ignored;
        std::filesystem::remove_all(path_, ignored);
        std::filesystem::create_directories(path_, ignored);
    }
    ~TempDir()
    {
        std::error_code ignored;
        std::filesystem::remove_all(path_, ignored);
    }
    TempDir(const TempDir&) = delete;
    TempDir& operator=(const TempDir&) = delete;
    [[nodiscard]] const std::filesystem::path& path() const { return path_; }
    [[nodiscard]] std::filesystem::path file(const std::string& name) const { return path_ / name; }

  private:
    std::filesystem::path path_;
};

std::string fileText(const std::filesystem::path& path)
{
    std::ifstream in(path, std::ios::binary);
    return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
}

void writeFile(const std::filesystem::path& path, const std::string& bytes)
{
    std::ofstream out(path, std::ios::binary);
    out << bytes;
}

void appendLittleEndian(std::string& out, std::uint32_t value, int bytes)
{
    for (int i = 0; i < bytes; ++i) {
        out += static_cast<char>((value >> (8 * i)) & 0xFFu);
    }
}

// One gzip member (RFC 1952) holding the bytes in stored deflate blocks (RFC
// 1951, 3.2.4), built here so that what GDAL reads is not what GDAL wrote.
std::string gzipped(const std::string& bytes)
{
    std::uint32_t crc = 0xFFFFFFFFu; // RFC 1952, 8: the CRC-32 of ISO 3309
    for (const char c : bytes) {
        crc ^= static_cast<unsigned char>(c);
        for (int bit = 0; bit < 8; ++bit) {
            crc = (crc >> 1) ^ (0xEDB88320u & (0u - (crc & 1u)));
        }
    }
    crc ^= 0xFFFFFFFFu;
    // ID1 ID2, CM = deflate, no flags, no time, no extra flags, OS unknown.
    std::string out = {'\x1f', '\x8b', '\x08', '\0', '\0', '\0', '\0', '\0', '\0', '\xff'};
    std::size_t at = 0;
    do {
        const std::size_t size = std::min<std::size_t>(bytes.size() - at, 65535);
        out += static_cast<char>(at + size == bytes.size() ? 1 : 0); // BFINAL; BTYPE 00, stored
        appendLittleEndian(out, static_cast<std::uint32_t>(size), 2);
        appendLittleEndian(out, static_cast<std::uint32_t>(~size & 0xFFFFu), 2);
        out.append(bytes, at, size);
        at += size;
    } while (at < bytes.size());
    appendLittleEndian(out, crc, 4);
    appendLittleEndian(out, static_cast<std::uint32_t>(bytes.size()), 4);
    return out;
}

// A tar of one file, in POSIX's ustar format (IEEE 1003.1-2001, "pax",
// ustar Interchange Format): a 512-byte header, the bytes to a whole block,
// two empty blocks.
std::string tarred(const std::string& name, const std::string& bytes)
{
    std::string header(512, '\0');
    const auto put = [&header](std::size_t at, const std::string& text) {
        header.replace(at, text.size(), text);
    };
    const auto octal = [](std::uint64_t value, std::size_t digits) {
        std::string text(digits, '0');
        for (std::size_t i = digits; i-- > 0 && value != 0; value /= 8) {
            text[i] = static_cast<char>('0' + value % 8);
        }
        return text;
    };
    put(0, name);
    put(100, octal(0644, 7));        // mode
    put(108, octal(0, 7));           // uid
    put(116, octal(0, 7));           // gid
    put(124, octal(bytes.size(), 11));
    put(136, octal(0, 11));          // mtime
    put(148, std::string(8, ' '));   // the checksum counts itself as spaces
    header[156] = '0';               // a regular file
    put(257, "ustar");
    put(263, "00");
    unsigned sum = 0;
    for (const char c : header) {
        sum += static_cast<unsigned char>(c);
    }
    put(148, octal(sum, 6));
    header[154] = '\0';
    header[155] = ' ';
    std::string out = header + bytes;
    out.append((512 - bytes.size() % 512) % 512, '\0');
    out.append(1024, '\0');
    return out;
}

bool contains(const std::vector<std::string>& values, const std::string& value)
{
    return std::ranges::find(values, value) != values.end();
}

Model modelWith(std::vector<Entity> entities)
{
    Model model;
    for (Entity& entity : entities) {
        if (!model.layers.contains(entity.layer)) {
            katana::entity::Layer layer;
            layer.name = entity.layer;
            EXPECT_TRUE(model.layers.add(layer).ok()) << entity.layer;
        }
        EXPECT_TRUE(model.entities.add(std::move(entity)).ok());
    }
    return model;
}

// A kerb (a segment) and a fence (a polyline) at survey coordinates, with a
// text, a real and an integer property each.
Model kerbAndFence(Entity& kerb, Entity& fence)
{
    kerb.geometry = Segment2{Point2(330000.0, 6250000.0), Point2(330030.0, 6250040.0)};
    kerb.properties["code"] = std::string("KERB");
    kerb.properties["width"] = 0.15;
    kerb.properties["count"] = std::int64_t{3};
    Polyline2 fenceLine;
    fenceLine.vertices = {Point2(330000.0, 6250100.0), Point2(330010.0, 6250105.0),
                          Point2(330020.0, 6250100.0)};
    fence.geometry = fenceLine;
    fence.properties["code"] = std::string("FENCE");
    fence.properties["width"] = 0.05;
    fence.properties["count"] = std::int64_t{12};
    return modelWith({kerb, fence});
}

std::vector<Point2> pointsOf(const Entity& entity)
{
    if (const auto* segment = std::get_if<Segment2>(&entity.geometry)) {
        return {segment->start, segment->end};
    }
    if (const auto* line = std::get_if<Polyline2>(&entity.geometry)) {
        return line->vertices;
    }
    return {};
}

const PropertyValue* property(const Entity& entity, const std::string& key)
{
    const auto found = entity.properties.find(key);
    return found == entity.properties.end() ? nullptr : &found->second;
}

// ---- the registry ---------------------------------------------------------------------------

TEST(Formats, FormatsListsGpkgAsReadWrite)
{
    // GDAL's GeoPackage driver reads and writes both tiles and features
    // (gdal.org/drivers/raster/gpkg.html, drivers/vector/gpkg.html).
    const gis::Format* gpkg = gis::findFormat("gpkg");
    ASSERT_NE(gpkg, nullptr) << "a driver is found whatever the case it is named in";
    EXPECT_EQ(gpkg->driver, "GPKG");
    EXPECT_EQ(gpkg->description, "GeoPackage");
    EXPECT_TRUE(gpkg->readRaster && gpkg->readVector && gpkg->writeRaster && gpkg->writeVector);
    EXPECT_TRUE(gpkg->virtualIo);
    EXPECT_TRUE(contains(gpkg->extensions, "gpkg"));
    EXPECT_TRUE(contains(interop::vectorExtensions(), "gpkg"));
    EXPECT_TRUE(contains(interop::rasterExtensions(), "gpkg"));
}

TEST(Formats, EcwIsNotOffered)
{
    // The hand-kept list offered .ecw, which only a GDAL built with the
    // vendor's SDK opens; the file dialog now offers what the registry has.
    const bool hasReader = gis::findFormat("ECW") != nullptr;
    EXPECT_EQ(contains(interop::rasterExtensions(), "ecw"), hasReader);
    EXPECT_EQ(contains(gis::readableExtensions(gis::DataKind::Raster), "ecw"), hasReader);
    if (!hasReader) {
        EXPECT_EQ(interop::kindForPath("ortho.ecw"), interop::SourceKind::Unknown)
            << "an extension no reader claims routes nowhere by its name";
    }
}

TEST(Formats, DriversThatAreNoFormatAreNotOffered)
{
    for (const char* driver : {"MEM", "DERIVED", "HTTP", "AIVector", "GPSBabel", "GNMFile"}) {
        EXPECT_EQ(gis::findFormat(driver), nullptr) << driver;
    }
    // Sidecars and generic extensions are opened through their data file.
    const auto vector = gis::readableExtensions(gis::DataKind::Vector);
    for (const char* extension : {"dbf", "shx", "prj", "txt", "xml"}) {
        EXPECT_FALSE(contains(vector, extension)) << extension;
    }
    EXPECT_TRUE(contains(vector, "shp"));
    EXPECT_TRUE(contains(vector, "fgb"));
    EXPECT_TRUE(std::ranges::is_sorted(vector));
    // What another importer takes by its extension is not offered as GDAL's.
    EXPECT_FALSE(contains(interop::rasterExtensions(), "e57"));
}

TEST(Formats, TheWriterForANameIsGdalsOwnChoiceWithKatanasOverlay)
{
    const auto writer = [](const char* name) {
        auto found = gis::vectorWriterFor(name);
        return found.ok() ? *found : "error: " + found.error().describe();
    };
    // GDAL's own ranking (GDALGetOutputDriversForDatasetName) ...
    EXPECT_EQ(writer("peg.kml"), "LIBKML");
    EXPECT_EQ(writer("peg.KMZ"), "LIBKML");
    EXPECT_EQ(writer("lots.json"), "GeoJSON");
    EXPECT_EQ(writer("lots.fgb"), "FlatGeobuf");
    EXPECT_EQ(writer("lots.shp"), "ESRI Shapefile");
    EXPECT_EQ(writer("lots.shp.zip"), "ESRI Shapefile") << "a compound extension is whole";
    // ... and Katana's where it differs: GDAL would write NASA's PDS4.
    EXPECT_EQ(writer("lots.xml"), "GML");

    auto none = gis::vectorWriterFor("lots");
    ASSERT_FALSE(none.ok());
    EXPECT_EQ(none.error().code, ErrorCode::InvalidArgument);
    // OSM has a reader and no writer; GPSBabel, which GDAL would pick, needs
    // a program Katana does not ship. A raster format writes no features.
    for (const char* name : {"roads.osm", "dem.xyz", "lots.nothing"}) {
        auto refused = gis::vectorWriterFor(name);
        ASSERT_FALSE(refused.ok()) << name;
        EXPECT_EQ(refused.error().code, ErrorCode::Unsupported) << name;
        EXPECT_NE(refused.error().message.find("no vector driver is registered for '."),
                  std::string::npos)
            << refused.error().message;
    }
    // GdalDataset's own question is the same one.
    EXPECT_EQ(*gis::GdalDataset::vectorDriverForPath("lots.fgb"), "FlatGeobuf");
}

TEST(Formats, TheSaveDialogOffersEveryWriterWithTheCommonOnesFirst)
{
    const auto choices = gis::vectorSaveChoices();
    ASSERT_FALSE(choices.empty());
    EXPECT_EQ(choices.front().extension, "shp");
    std::vector<std::string> extensions;
    for (const gis::SaveChoice& choice : choices) {
        EXPECT_FALSE(contains(extensions, choice.extension))
            << "each extension once: " << choice.extension;
        extensions.push_back(choice.extension);
        // What the dialog offers is what EXPORT writes that name with.
        auto writer = gis::vectorWriterFor("katana." + choice.extension);
        ASSERT_TRUE(writer.ok()) << choice.extension;
        EXPECT_EQ(*writer, choice.driver) << choice.extension;
    }
    for (const char* extension : {"geojson", "gpkg", "kml", "kmz", "dxf", "csv", "fgb", "gpx",
                                  "shp.zip", "tab"}) {
        EXPECT_TRUE(contains(extensions, extension)) << extension;
    }
    // The hand-kept table's six were always there; now they come first.
    const auto interopChoices = interop::vectorExportFormats();
    ASSERT_EQ(interopChoices.size(), choices.size());
    EXPECT_EQ(interopChoices[0].extension, "shp");
}

TEST(Formats, APathThatIsNoFileIsLeftToGdal)
{
    EXPECT_TRUE(gis::isVirtualPath("/vsizip/C:/data/lots.zip/lots.shp"));
    EXPECT_TRUE(gis::isVirtualPath("https://example.com/dem.tif"));
    EXPECT_TRUE(gis::isVirtualPath("PG:dbname=survey")) << "a driver's connection prefix";
    EXPECT_FALSE(gis::isVirtualPath("C:/data/lots.shp")) << "a drive is no connection prefix";
    EXPECT_FALSE(gis::isVirtualPath("lots.shp"));
    EXPECT_TRUE(gis::isRemotePath("/vsicurl/https://example.com/dem.tif"));
    EXPECT_TRUE(gis::isRemotePath("s3://bucket/dem.tif"));
    EXPECT_FALSE(gis::isRemotePath("/vsizip/lots.zip"));

    // A local file that is not there is still said so plainly ...
    auto missing = gis::GdalDataset::open("C:/no/such/folder/lots.shp");
    ASSERT_FALSE(missing.ok());
    EXPECT_EQ(missing.error().code, ErrorCode::NotFound);
    // ... but a /vsi path is GDAL's to open, not refused as a missing file.
    auto inMemory = gis::GdalDataset::open("/vsimem/katana-formats-nothing.geojson");
    ASSERT_FALSE(inMemory.ok());
    EXPECT_EQ(inMemory.error().code, ErrorCode::FileImportFailure)
        << inMemory.error().describe();
}

TEST(Formats, FormatOptionsAreTheDriversDeclaredLists)
{
    // gdal.org/drivers/vector/gpkg.html: LIST_ALL_TABLES=AUTO/YES/NO, for
    // vector; ZLEVEL for PNG tiles, 1 to 9.
    auto gpkg = gis::formatOptions("GPKG");
    ASSERT_TRUE(gpkg.ok()) << gpkg.error().describe();
    const auto named = [](const std::vector<gis::FormatOption>& options, const std::string& name) {
        const auto found = std::ranges::find_if(
            options, [&name](const gis::FormatOption& option) { return option.name == name; });
        return found == options.end() ? nullptr : &*found;
    };
    const gis::FormatOption* listAll = named(gpkg->open, "LIST_ALL_TABLES");
    ASSERT_NE(listAll, nullptr);
    EXPECT_EQ(listAll->type, "string-select");
    EXPECT_EQ(listAll->defaultValue, "AUTO");
    EXPECT_EQ(listAll->scope, "vector");
    EXPECT_EQ(listAll->choices, (std::vector<std::string>{"AUTO", "YES", "NO"}));
    const gis::FormatOption* zlevel = named(gpkg->open, "ZLEVEL");
    ASSERT_NE(zlevel, nullptr);
    EXPECT_EQ(zlevel->min, 1.0);
    EXPECT_EQ(zlevel->max, 9.0);
    // A shapefile's layer takes its encoding (drivers/vector/shapefile.html).
    auto shapefile = gis::formatOptions("ESRI Shapefile");
    ASSERT_TRUE(shapefile.ok());
    EXPECT_NE(named(shapefile->layerCreation, "ENCODING"), nullptr);
    // GeoTIFF's creation options include its compression.
    auto tiff = gis::formatOptions("GTiff");
    ASSERT_TRUE(tiff.ok());
    const gis::FormatOption* compress = named(tiff->creation, "COMPRESS");
    ASSERT_NE(compress, nullptr);
    EXPECT_TRUE(contains(compress->choices, "DEFLATE"));

    auto unknown = gis::formatOptions("NoSuchDriver");
    ASSERT_FALSE(unknown.ok());
    EXPECT_EQ(unknown.error().code, ErrorCode::NotFound);
}

// ---- formats the old tables could not reach -------------------------------------------------

TEST(Formats, FlatGeobufGeoParquetKmzGpxAndCsvWktRoundTrip)
{
    // Written by the writer each NAME picks - no driver given - and read
    // back: the geometry to the micrometre, or, in longitude and latitude
    // (KMZ, GPX), to 1e-9 degrees (0.1 mm) of PROJ's conversion; the fields
    // with their types where the format has them.
    struct Case {
        const char* file;
        const char* driver;
        bool lonLat;
        bool typed;
    };
    const std::vector<Case> cases = {{"lines.fgb", "FlatGeobuf", false, true},
                                     {"lines.parquet", "Parquet", false, true},
                                     {"lines.kmz", "LIBKML", true, true},
                                     {"lines.gpx", "GPX", true, false},
                                     {"lines.csv", "CSV", false, true}};
    Entity kerb;
    Entity fence;
    const Model model = kerbAndFence(kerb, fence);
    for (const Case& test : cases) {
        SCOPED_TRACE(test.file);
        if (gis::findFormat(test.driver) == nullptr) {
            // Linux distributions build GDAL without some drivers (Parquet
            // needs Arrow); a missing driver is the build's, not a defect.
            std::cout << "[ SKIPPED  ] " << test.driver << " is not in this GDAL build\n";
            continue;
        }
        const TempDir dir(std::string("round-trip-") + test.file);
        interop::VectorExportOptions options;
        options.projectionWkt = kMga56;
        const auto exported = interop::exportVector(model, dir.file(test.file), options);
        ASSERT_TRUE(exported.ok()) << exported.error().describe();
        EXPECT_EQ(exported->driver, test.driver);
        EXPECT_EQ(exported->featuresWritten, 2u);
        EXPECT_EQ(interop::kindForPath(dir.file(test.file)), interop::SourceKind::Vector);

        const auto imported = interop::importVector(dir.file(test.file));
        ASSERT_TRUE(imported.ok()) << imported.error().describe();
        // Two lines out, two entities in: GDAL reads a GPX's routes a second
        // time as a layer of their vertices (route_points), which is left
        // out and said, not made five points beside the lines.
        EXPECT_EQ(imported->entities.size(), 2u);
        if (std::string(test.driver) == "GPX") {
            const bool said = std::ranges::any_of(imported->warnings, [](const std::string& text) {
                return text.find("GPX layer 'route_points' left out") != std::string::npos;
            });
            EXPECT_TRUE(said);
        }
        std::vector<const Entity*> lines;
        for (const Entity& entity : imported->entities) {
            if (pointsOf(entity).size() >= 2) {
                lines.push_back(&entity);
            }
        }
        ASSERT_EQ(lines.size(), 2u);
        for (const Entity* back : lines) {
            const std::vector<Point2> got = pointsOf(*back);
            const Entity& original = got.size() == 2 ? kerb : fence;
            const std::vector<Point2> expected = pointsOf(original);
            ASSERT_EQ(got.size(), expected.size());
            for (std::size_t i = 0; i < got.size(); ++i) {
                Point2 want = expected[i];
                if (test.lonLat) {
                    const auto moved = gis::transformPoint(want.x, want.y, kMga56, "EPSG:4326");
                    ASSERT_TRUE(moved.ok());
                    want = Point2((*moved)[0], (*moved)[1]);
                }
                const double tolerance = test.lonLat ? 1e-9 : 1e-6;
                EXPECT_NEAR(got[i].x, want.x, tolerance);
                EXPECT_NEAR(got[i].y, want.y, tolerance);
            }
            // The code comes back wherever the format keeps it.
            const bool coded = std::ranges::any_of(back->properties, [&](const auto& entry) {
                return entry.second == original.properties.at("code");
            });
            EXPECT_TRUE(coded) << "the code field is kept";
            if (!test.typed) {
                continue;
            }
            const PropertyValue* width = property(*back, "width");
            const PropertyValue* count = property(*back, "count");
            ASSERT_NE(width, nullptr);
            ASSERT_NE(count, nullptr);
            ASSERT_TRUE(std::holds_alternative<double>(*width));
            EXPECT_NEAR(std::get<double>(*width), std::get<double>(original.properties.at("width")),
                        1e-12);
            EXPECT_EQ(*count, original.properties.at("count"));
        }
    }
}

TEST(Formats, AShapefileInsideAZipImportsThroughVsizip)
{
    const TempDir dir("zipped-shapefile");
    Entity kerb;
    Entity fence;
    const Model model = kerbAndFence(kerb, fence);
    interop::VectorExportOptions options;
    options.projectionWkt = kMga56;
    // GDAL writes a zipped shapefile itself, named .shp.zip.
    const auto exported = interop::exportVector(model, dir.file("lots.shp.zip"), options);
    ASSERT_TRUE(exported.ok()) << exported.error().describe();
    EXPECT_EQ(exported->driver, "ESRI Shapefile");

    // The same archive as a plain .zip, which GDAL does not open as it is.
    std::filesystem::copy_file(dir.file("lots.shp.zip"), dir.file("lots.zip"));
    auto members = gis::listZip(dir.file("lots.zip"));
    ASSERT_TRUE(members.ok()) << members.error().describe();
    std::string shp;
    for (const gis::ZipMember& member : *members) {
        if (member.name.ends_with(".shp")) {
            shp = member.name;
        }
    }
    ASSERT_FALSE(shp.empty()) << "the archive holds a .shp";

    // Routed by its inside, and imported through it.
    EXPECT_EQ(interop::kindForPath(dir.file("lots.zip")), interop::SourceKind::Vector);
    const auto whole = interop::importVector(dir.file("lots.zip"));
    ASSERT_TRUE(whole.ok()) << whole.error().describe();
    EXPECT_EQ(whole->entities.size(), 2u);

    // Named inside the archive, as GDAL names it: a /vsi path, which the
    // exists check used to refuse as a missing file.
    const std::string inside = "/vsizip/" + dir.file("lots.zip").generic_string() + "/" + shp;
    EXPECT_EQ(interop::kindForPath(inside), interop::SourceKind::Vector);
    const auto named = interop::importVector(inside);
    ASSERT_TRUE(named.ok()) << named.error().describe();
    ASSERT_EQ(named->entities.size(), 2u);
    for (const Entity& entity : named->entities) {
        const std::vector<Point2> got = pointsOf(entity);
        const std::vector<Point2> want = pointsOf(got.size() == 2 ? kerb : fence);
        ASSERT_EQ(got.size(), want.size());
        EXPECT_NEAR(got.front().x, want.front().x, 1e-6);
        EXPECT_NEAR(got.front().y, want.front().y, 1e-6);
    }
}

TEST(Formats, AnArchiveOfSeveralDatasetsIsRefusedNamingThem)
{
    const TempDir dir("two-datasets");
    const std::string lots = fileText(kData + "/lots.geojson");
    ASSERT_FALSE(lots.empty());
    const gis::ZipContent members[] = {{"a.geojson", lots}, {"b.geojson", lots}};
    ASSERT_TRUE(gis::writeZip(dir.file("two.zip"), members).ok());

    auto opened = gis::GdalDataset::open(dir.file("two.zip"));
    ASSERT_FALSE(opened.ok());
    EXPECT_EQ(opened.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(opened.error().message.find("2 datasets (a.geojson, b.geojson)"), std::string::npos)
        << opened.error().message;
    EXPECT_NE(opened.error().message.find("/vsizip/"), std::string::npos)
        << "the refusal says how to name one";

    // Named, one opens.
    const std::string one =
        "/vsizip/{" + dir.file("two.zip").generic_string() + "}/b.geojson";
    const auto imported = interop::importVector(one);
    ASSERT_TRUE(imported.ok()) << imported.error().describe();
    EXPECT_FALSE(imported->entities.empty());
}

TEST(Formats, AGzippedFileAndATarOpenByWhatTheyHold)
{
    // GDAL opens neither as it is: a .gz is read through /vsigzip - which
    // takes no braces, the defect this found - and a tar as a folder.
    const TempDir dir("compressed");
    const std::string lots = fileText(kData + "/lots.geojson");
    ASSERT_FALSE(lots.empty());
    writeFile(dir.file("lots.geojson.gz"), gzipped(lots));
    writeFile(dir.file("lots.tar"), tarred("lots.geojson", lots));
    for (const char* name : {"lots.geojson.gz", "lots.tar"}) {
        SCOPED_TRACE(name);
        EXPECT_EQ(interop::kindForPath(dir.file(name)), interop::SourceKind::Vector);
        const auto imported = interop::importVector(dir.file(name));
        ASSERT_TRUE(imported.ok()) << imported.error().describe();
        EXPECT_EQ(imported->entities.size(), 3u) << "the three lots of lots.geojson";
    }
}

TEST(Formats, AGpkgHoldingOnlyARasterRoutesToRasterImport)
{
    const TempDir dir("gpkg-raster");
    // GDAL writes the plane's grid as the GeoPackage's tiles; the name alone
    // says vector data, the content says raster.
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter{document};
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    geo::Context context{document, interpreter, reference, surfaces, dir.path(), {}, {}};
    const std::string gpkg = dir.file("plane.gpkg").generic_string();
    auto converted = geo::runNow(context, "GDAL raster convert \"" + kData + "/plane.asc\" \"" +
                                              gpkg + "\" --output-format GPKG");
    ASSERT_TRUE(converted.ok()) << converted.error().describe();

    auto content = gis::identifyContent(gpkg);
    ASSERT_TRUE(content.ok()) << content.error().describe();
    EXPECT_EQ(content->driver, "GPKG");
    EXPECT_TRUE(content->raster);
    EXPECT_FALSE(content->vector);
    EXPECT_EQ(interop::kindForPath(gpkg), interop::SourceKind::Raster);
    EXPECT_EQ(interop::kindForPath(dir.file("not-written.gpkg")), interop::SourceKind::Vector)
        << "only a name to go by: a GeoPackage is read as features";

    const auto raster = interop::importRaster(gpkg);
    ASSERT_TRUE(raster.ok()) << raster.error().describe();
    // plane.asc: 40 x 30 cells of 1 m from (0,0).
    EXPECT_EQ(raster->width, 40);
    EXPECT_EQ(raster->height, 30);
    EXPECT_NEAR(raster->geotransform[0], 0.0, 1e-9);
    EXPECT_NEAR(raster->geotransform[3], 30.0, 1e-9);
}

TEST(Formats, AnUnknownExtensionIsRoutedByContent)
{
    const TempDir dir("unknown-extension");
    std::filesystem::copy_file(kData + "/lot_with_hole.geojson", dir.file("lot.data"));
    std::filesystem::copy_file(kData + "/plane.asc", dir.file("plane.grid"));
    EXPECT_EQ(interop::kindForPath(dir.file("lot.data")), interop::SourceKind::Vector);
    EXPECT_EQ(interop::kindForPath(dir.file("plane.grid")), interop::SourceKind::Raster);
    const auto imported = interop::importVector(dir.file("lot.data"));
    ASSERT_TRUE(imported.ok()) << imported.error().describe();
    EXPECT_EQ(imported->entities.size(), 2u) << "the lot and its hole";
    const auto raster = interop::importRaster(dir.file("plane.grid"));
    ASSERT_TRUE(raster.ok()) << raster.error().describe();
    EXPECT_EQ(raster->width, 40);

    // Text no reader recognises routes nowhere, and says so.
    {
        std::ofstream out(dir.file("notes.data"), std::ios::binary);
        out << "these are not coordinates\n";
    }
    EXPECT_EQ(interop::kindForPath(dir.file("notes.data")), interop::SourceKind::Unknown);
    auto nothing = gis::identifyContent(dir.file("notes.data"));
    ASSERT_FALSE(nothing.ok());
    EXPECT_EQ(nothing.error().code, ErrorCode::FileImportFailure);
}

// ---- FORMATS ----------------------------------------------------------------------------------

class FormatsVerb : public ::testing::Test {
  protected:
    TempDir scratch{::testing::UnitTest::GetInstance()->current_test_info()->name()};
    katana::cad::Document document;
    katana::cad::CommandInterpreter interpreter{document};
    katana::interop::ReferenceData reference;
    katana::terrain::SurfaceStore surfaces;
    geo::Context context{document, interpreter, reference, surfaces, scratch.path(), {}, {}};

    katana::core::Result<std::string> run(const std::string& line)
    {
        return geo::runNow(context, line);
    }
};

TEST_F(FormatsVerb, FormatsListsWritableVectorDriversAsRecords)
{
    EXPECT_TRUE(geo::handles("FORMATS"));
    EXPECT_NE(geo::helpText().find("FORMATS [RASTER|VECTOR] [READ|WRITE]"), std::string::npos);
    auto reply = run("FORMATS VECTOR WRITE");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_NE(reply->find("format driver=FlatGeobuf kind=vector read=vector write=vector "
                          "extensions=fgb vsi=yes description=FlatGeobuf"),
              std::string::npos)
        << *reply;
    EXPECT_NE(reply->find("format driver=GPKG kind=raster,vector read=raster,vector "
                          "write=raster,vector extensions=gpkg,gpkg.zip vsi=yes"),
              std::string::npos);
    EXPECT_EQ(reply->find("driver=GTiff"), std::string::npos) << "a raster format writes no layers";
    EXPECT_NE(reply->find("\nlisted formats="), std::string::npos);
    EXPECT_NE(reply->find(" kind=vector capability=write filter= gdal=3."), std::string::npos);
    // Nothing changed, nothing ran: FORMATS answers at prepare.
    auto prepared = geo::prepare(context, "FORMATS");
    ASSERT_TRUE(prepared.ok());
    EXPECT_TRUE(prepared->reply.has_value());
    EXPECT_EQ(document.model().entities.size(), 0u);
}

TEST_F(FormatsVerb, FormatsFindsWordsInNamesDescriptionsAndExtensions)
{
    auto reply = run("FORMATS READ fgb");
    ASSERT_TRUE(reply.ok());
    EXPECT_NE(reply->find("format driver=FlatGeobuf "), std::string::npos) << *reply;
    // Every format listed holds the word: FlatGeobuf, and whatever else
    // claims it (GDAL's raster tile index reads a .gti.fgb).
    const auto records = geo::parseRecords(*reply);
    ASSERT_GE(records.size(), 2u);
    for (std::size_t i = 0; i + 1 < records.size(); ++i) {
        const std::string text = *records[i].get("driver") + " " +
                                 *records[i].get("description") + " " +
                                 *records[i].get("extensions");
        EXPECT_NE(text.find("fgb"), std::string::npos) << text;
    }
    EXPECT_NE(reply->find("listed formats=" + std::to_string(records.size() - 1) +
                          " kind=any capability=read filter=fgb"),
              std::string::npos)
        << *reply;
    auto none = run("FORMATS RASTER nothing-has-this-word");
    ASSERT_TRUE(none.ok());
    EXPECT_EQ(none->rfind("listed formats=0 kind=raster", 0), 0u) << *none;
}

TEST_F(FormatsVerb, FormatsOptionsGivesADriversOptions)
{
    auto reply = run("FORMATS OPTIONS GPKG");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_EQ(reply->rfind("format driver=GPKG ", 0), 0u) << *reply;
    EXPECT_NE(reply->find("option driver=GPKG list=open name=LIST_ALL_TABLES type=string-select "
                          "default=AUTO scope=vector choices=AUTO,YES,NO min= max= "
                          "description=\"Whether all tables"),
              std::string::npos)
        << *reply;
    EXPECT_NE(reply->find("option driver=GPKG list=open name=ZLEVEL type=int default=6 "
                          "scope=raster choices= min=1 max=9"),
              std::string::npos);
    EXPECT_NE(reply->find("\nlisted driver=GPKG open="), std::string::npos);
    // A driver's name of two words, as it is, unquoted or quoted.
    for (const char* line : {"FORMATS OPTIONS ESRI Shapefile", "FORMATS OPTIONS \"ESRI Shapefile\""}) {
        auto shapefile = run(line);
        ASSERT_TRUE(shapefile.ok()) << line;
        EXPECT_NE(shapefile->find("list=layer_creation name=ENCODING"), std::string::npos);
    }
    auto unknown = run("FORMATS OPTIONS NoSuchDriver");
    ASSERT_FALSE(unknown.ok());
    EXPECT_EQ(unknown.error().code, ErrorCode::NotFound);
    auto nameless = run("FORMATS OPTIONS");
    ASSERT_FALSE(nameless.ok());
    EXPECT_EQ(nameless.error().code, ErrorCode::InvalidArgument);
}

TEST_F(FormatsVerb, FormatsJsonIsTheRecordsAsData)
{
    auto reply = run("FORMATS VECTOR WRITE fgb JSON");
    ASSERT_TRUE(reply.ok());
    const nlohmann::json formats = nlohmann::json::parse(*reply);
    ASSERT_EQ(formats.size(), 1u) << *reply;
    EXPECT_EQ(formats[0]["driver"], "FlatGeobuf");
    EXPECT_EQ(formats[0]["kinds"], nlohmann::json({"vector"}));
    EXPECT_EQ(formats[0]["write"], nlohmann::json({"vector"}));
    EXPECT_EQ(formats[0]["extensions"], nlohmann::json({"fgb"}));
    EXPECT_EQ(formats[0]["vsi"], true);
    EXPECT_TRUE(formats[0]["help_url"].get<std::string>().starts_with("https://gdal.org/"));

    auto options = run("FORMATS OPTIONS GPKG JSON");
    ASSERT_TRUE(options.ok());
    const nlohmann::json described = nlohmann::json::parse(*options);
    EXPECT_EQ(described["format"]["driver"], "GPKG");
    EXPECT_FALSE(described["open_options"].empty());
    EXPECT_FALSE(described["creation_options"].empty());
}

TEST_F(FormatsVerb, FormatsRefusesTwoKindsOrTwoCapabilities)
{
    for (const char* line : {"FORMATS RASTER VECTOR", "FORMATS READ WRITE"}) {
        auto refused = run(line);
        ASSERT_FALSE(refused.ok()) << line;
        EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    }
    // A keyword quoted is a word to look for.
    auto quoted = run("FORMATS \"raster\"");
    ASSERT_TRUE(quoted.ok());
    EXPECT_NE(quoted->find("kind=any capability=any filter=raster"), std::string::npos) << *quoted;
}

} // namespace
