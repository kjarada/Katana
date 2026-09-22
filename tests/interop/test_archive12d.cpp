#include <gtest/gtest.h>

#include <filesystem>
#include <fstream>
#include <sstream>
#include <string>

#include "katana/gis/zip_container.hpp"
#include "katana/interop/archive12d.hpp"
#include "katana/interop/import.hpp"

namespace interop = katana::interop;
using katana::core::ErrorCode;
using katana::geometry::Point2;

namespace {

class TempDir {
  public:
    explicit TempDir(const std::string& name)
        : path_(std::filesystem::temp_directory_path() / ("katana-archive12d-" + name))
    {
        std::filesystem::remove_all(path_);
        std::filesystem::create_directories(path_);
    }
    ~TempDir()
    {
        std::error_code ignored;
        std::filesystem::remove_all(path_, ignored);
    }
    TempDir(const TempDir&) = delete;
    TempDir& operator=(const TempDir&) = delete;

    [[nodiscard]] std::filesystem::path operator/(const std::string& name) const
    {
        return path_ / name;
    }

  private:
    std::filesystem::path path_;
};

std::filesystem::path fixture(const std::string& name)
{
    return std::filesystem::path(KATANA_ARCHIVE12D_TEST_DATA) / name;
}

std::string bytesOf(const std::filesystem::path& path)
{
    std::ifstream file(path, std::ios::binary);
    std::ostringstream out;
    out << file.rdbuf();
    return out.str();
}

void write(const std::filesystem::path& path, const std::string& bytes)
{
    std::ofstream file(path, std::ios::binary);
    file << bytes;
}

katana::entity::Model smallDrawing()
{
    katana::entity::Model model;
    katana::entity::Entity point;
    point.geometry = katana::entity::PointGeometry{Point2(502000.5, 6960000.25)};
    point.properties["elevation"] = 32.5;
    EXPECT_TRUE(model.entities.add(point).ok());
    katana::entity::Entity line;
    line.geometry = katana::geometry::Polyline2{
        {Point2(0.0, 0.0), Point2(10.0, 0.0), Point2(10.0, 5.0)}, true};
    EXPECT_TRUE(model.entities.add(line).ok());
    return model;
}

} // namespace

TEST(ZipContainer, WhatIsWrittenIsReadBackAndTheSecondWritingReplacesTheFirst)
{
    const TempDir dir("zip");
    const auto archive = dir / "a.12daz";
    ASSERT_TRUE(katana::gis::writeZip(archive, "first.12da", "one").ok());
    ASSERT_TRUE(katana::gis::writeZip(archive, "second.12da", std::string(100000, 'x')).ok());

    const auto members = katana::gis::listZip(archive);
    ASSERT_TRUE(members.ok()) << members.error().describe();
    // A directory cached from the first archive would still say "first".
    ASSERT_EQ(members->size(), 1u);
    EXPECT_EQ((*members)[0].name, "second.12da");
    EXPECT_EQ((*members)[0].size, 100000u);
    const auto bytes = katana::gis::readZipMember(archive, "second.12da", 1u << 20);
    ASSERT_TRUE(bytes.ok()) << bytes.error().describe();
    EXPECT_EQ(*bytes, std::string(100000, 'x'));
    // A hundred thousand identical bytes deflate to almost nothing; a stored
    // member would not.
    EXPECT_LT(std::filesystem::file_size(archive), 2000u);
}

TEST(ZipContainer, TheArchiveIsTheKindTwelveDModelWrites)
{
    // ZIP APPNOTE 4.3.7, the local file header: signature 50 4B 03 04, then
    // version (2 bytes), flags (2), and the compression method (2) - 8 is
    // DEFLATE, which is what a .12daz written by 12d Model uses.
    const TempDir dir("zip-header");
    const auto archive = dir / "a.12daz";
    ASSERT_TRUE(katana::gis::writeZip(archive, "a.12da", "model \"M\"\n").ok());
    const std::string bytes = bytesOf(archive);
    ASSERT_GE(bytes.size(), 10u);
    EXPECT_EQ(bytes.substr(0, 4), std::string("PK\x03\x04", 4));
    EXPECT_EQ(static_cast<unsigned char>(bytes[8]), 8u);
    EXPECT_EQ(static_cast<unsigned char>(bytes[9]), 0u);
}

TEST(ZipContainer, FailuresAreReportedNotGuessedAround)
{
    const TempDir dir("zip-bad");
    EXPECT_EQ(katana::gis::listZip(dir / "missing.12daz").error().code,
              ErrorCode::FileImportFailure);

    write(dir / "text.12daz", "this is not a zip archive at all");
    EXPECT_FALSE(katana::gis::listZip(dir / "text.12daz").ok());

    const auto archive = dir / "a.12daz";
    ASSERT_TRUE(katana::gis::writeZip(archive, "a.12da", "0123456789").ok());
    EXPECT_EQ(katana::gis::readZipMember(archive, "other.12da", 100).error().code,
              ErrorCode::NotFound);
    // The limit is checked against the directory, before anything is inflated.
    const auto tooBig = katana::gis::readZipMember(archive, "a.12da", 9);
    ASSERT_FALSE(tooBig.ok());
    EXPECT_NE(tooBig.error().message.find("limit"), std::string::npos);
    EXPECT_TRUE(katana::gis::readZipMember(archive, "a.12da", 10).ok());

    for (const char* name : {"", "../escape.12da", "/rooted.12da", "C:\\drive.12da"}) {
        EXPECT_EQ(katana::gis::writeZip(dir / "b.12daz", name, "x").error().code,
                  ErrorCode::InvalidArgument)
            << name;
    }
}

TEST(ZipContainer, AnEmptyMemberIsAnEmptyString)
{
    const TempDir dir("zip-empty");
    const auto archive = dir / "a.12daz";
    ASSERT_TRUE(katana::gis::writeZip(archive, "a.12da", "").ok());
    const auto bytes = katana::gis::readZipMember(archive, "a.12da", 100);
    ASSERT_TRUE(bytes.ok()) << bytes.error().describe();
    EXPECT_TRUE(bytes->empty());
}

TEST(Archive12dKinds, BothExtensionsAreRecognisedWhateverTheirCaseAndNoOther)
{
    for (const char* name : {"job.12da", "job.12daz", "JOB.12DA", "Job.12Daz"}) {
        EXPECT_EQ(interop::kindForPath(name), interop::SourceKind::Archive12d) << name;
    }
    EXPECT_FALSE(interop::isZippedArchive12d("job.12da"));
    EXPECT_TRUE(interop::isZippedArchive12d("job.12DAZ"));
    // There is no such format; it was accepted for a while by mistake.
    EXPECT_NE(interop::kindForPath("job.12dz"), interop::SourceKind::Archive12d);
    EXPECT_FALSE(interop::isZippedArchive12d("job.12dz"));
    EXPECT_NE(interop::kindForPath("job.12d"), interop::SourceKind::Archive12d);
}

TEST(Archive12dImport, AZippedArchiveGivesWhatThePlainFileGives)
{
    const auto plain = interop::importArchive12d(fixture("all_geometries.12da"));
    const auto zipped = interop::importArchive12d(fixture("all_geometries.12daz"));
    ASSERT_TRUE(plain.ok()) << plain.error().describe();
    ASSERT_TRUE(zipped.ok()) << zipped.error().describe();
    // The member is NOT named after the zip: the importer has to find it.
    EXPECT_EQ(zipped->memberName, "comprehensive-all-geometries.12da");
    EXPECT_TRUE(plain->memberName.empty());

    EXPECT_EQ(zipped->entities.size(), plain->entities.size());
    EXPECT_EQ(zipped->alignments.size(), plain->alignments.size());
    EXPECT_EQ(zipped->tally, plain->tally);
    EXPECT_EQ(zipped->archiveVersion, "15.01.08.60");
    EXPECT_EQ(zipped->encoding, "UTF-8");
    // What the fixture is known to hold (counted by hand in its own header):
    // one tin of 4 triangles and a cloud of 3 points.
    ASSERT_EQ(zipped->surfaces.size(), 1u);
    EXPECT_EQ(zipped->surfaces[0].surface.triangleCount(), 4u);
    ASSERT_EQ(zipped->clouds.size(), 1u);
    EXPECT_EQ(zipped->clouds[0].points.size(), 3u);
    EXPECT_EQ(zipped->clouds[0].name, "LiDAR Patch");
    EXPECT_FALSE(zipped->clouds[0].bounds.empty());
    EXPECT_EQ(zipped->clouds[0].bounds.minX, 503700.0);
    EXPECT_EQ(zipped->clouds[0].bounds.maxZ, 30.7);
}

TEST(Archive12dImport, EveryEntityIsOneTheModelWillAccept)
{
    // The point of validating inside the importer: createEntities is atomic,
    // so ONE bad entity would cost the user the whole file.
    auto imported = interop::importArchive12d(fixture("all_geometries.12da"));
    ASSERT_TRUE(imported.ok());
    katana::entity::Model model;
    for (const katana::entity::Layer& layer : imported->layersNeeded) {
        ASSERT_TRUE(model.layers.add(layer).ok()) << layer.name;
    }
    for (const katana::entity::Style& style : imported->stylesNeeded) {
        ASSERT_TRUE(model.styles.add(style).ok()) << style.name;
    }
    ASSERT_FALSE(imported->entities.empty());
    ASSERT_FALSE(imported->stylesNeeded.empty()) << "the fixture names linestyles";
    for (const katana::entity::Entity& entity : imported->entities) {
        EXPECT_TRUE(model.layers.contains(entity.layer)) << entity.layer;
        EXPECT_TRUE(entity.style.empty() || model.styles.contains(entity.style)) << entity.style;
        EXPECT_TRUE(model.entities.add(entity).ok());
    }
    for (const katana::entity::Alignment& alignment : imported->alignments) {
        EXPECT_TRUE(model.alignments.add(alignment).ok()) << alignment.name;
    }
}

TEST(Archive12dImport, FailuresNameTheFileAndTheLine)
{
    const TempDir dir("import-bad");
    EXPECT_EQ(interop::importArchive12d(dir / "missing.12da").error().code,
              ErrorCode::FileImportFailure);

    write(dir / "broken.12da", "model \"M\"\nstring super {\n data_3d { 1 2 }\n}\n");
    const auto broken = interop::importArchive12d(dir / "broken.12da");
    ASSERT_FALSE(broken.ok());
    EXPECT_EQ(broken.error().code, ErrorCode::ParseFailure);
    EXPECT_NE(broken.error().context.find("broken.12da"), std::string::npos);
    EXPECT_NE(broken.error().context.find("line 3"), std::string::npos) << broken.error().describe();

    write(dir / "plain.12daz", "model \"M\"\n");
    EXPECT_FALSE(interop::importArchive12d(dir / "plain.12daz").ok()) << "not a zip";

    ASSERT_TRUE(katana::gis::writeZip(dir / "empty.12daz", "readme.txt", "no archive here").ok());
    const auto none = interop::importArchive12d(dir / "empty.12daz");
    ASSERT_FALSE(none.ok());
    EXPECT_NE(none.error().message.find("no .12da"), std::string::npos);

    interop::Archive12dImportOptions tiny;
    tiny.maxBytes = 10;
    const auto tooBig = interop::importArchive12d(fixture("all_geometries.12da"), tiny);
    ASSERT_FALSE(tooBig.ok());
    EXPECT_NE(tooBig.error().message.find("limit"), std::string::npos);
    EXPECT_FALSE(interop::importArchive12d(fixture("all_geometries.12daz"), tiny).ok());
}

TEST(Archive12dImport, AnEmptyFileIsAnEmptyImportNotAnError)
{
    const TempDir dir("import-empty");
    write(dir / "empty.12da", "");
    const auto imported = interop::importArchive12d(dir / "empty.12da");
    ASSERT_TRUE(imported.ok()) << imported.error().describe();
    EXPECT_TRUE(imported->entities.empty());
    EXPECT_TRUE(imported->bounds.empty());
}

TEST(Archive12dExport, APlainArchiveIsUtf16WithAMarkByDefaultAndUtf8OnRequest)
{
    const TempDir dir("export-encoding");
    const auto model = smallDrawing();
    ASSERT_TRUE(interop::exportArchive12d(model, {}, dir / "wide.12da").ok());
    const std::string wide = bytesOf(dir / "wide.12da");
    ASSERT_GE(wide.size(), 4u);
    EXPECT_EQ(wide.substr(0, 2), std::string("\xFF\xFE", 2)) << "the mark 12d Model writes";
    EXPECT_EQ(wide[2], '/');
    EXPECT_EQ(wide[3], '\0');

    interop::Archive12dExportOptions options;
    options.utf16 = false;
    ASSERT_TRUE(interop::exportArchive12d(model, {}, dir / "narrow.12da", options).ok());
    EXPECT_EQ(bytesOf(dir / "narrow.12da").substr(0, 2), "//");

    const auto back = interop::importArchive12d(dir / "wide.12da");
    ASSERT_TRUE(back.ok()) << back.error().describe();
    EXPECT_EQ(back->encoding, "UTF-16 little-endian");
    EXPECT_TRUE(back->warnings.empty()) << back->warnings.front();
}

TEST(Archive12dExport, AZippedExportHoldsOneMemberNamedAfterTheFileAndImportsBack)
{
    const TempDir dir("export-zip");
    const auto model = smallDrawing();
    const auto exported = interop::exportArchive12d(model, {}, dir / "Site Survey.12daz");
    ASSERT_TRUE(exported.ok()) << exported.error().describe();
    EXPECT_EQ(exported->entitiesWritten, 2u);

    const auto members = katana::gis::listZip(dir / "Site Survey.12daz");
    ASSERT_TRUE(members.ok());
    ASSERT_EQ(members->size(), 1u);
    EXPECT_EQ((*members)[0].name, "Site Survey.12da");
    EXPECT_EQ((*members)[0].size, exported->bytesWritten);

    const auto back = interop::importArchive12d(dir / "Site Survey.12daz");
    ASSERT_TRUE(back.ok()) << back.error().describe();
    ASSERT_EQ(back->entities.size(), 2u);
    EXPECT_EQ(std::get<katana::entity::PointGeometry>(back->entities[0].geometry).position,
              Point2(502000.5, 6960000.25));
    EXPECT_EQ(std::get<double>(back->entities[0].properties.at("elevation")), 32.5);
    const auto& ring = std::get<katana::geometry::Polyline2>(back->entities[1].geometry);
    EXPECT_TRUE(ring.closed);
    EXPECT_EQ(ring.vertices.size(), 3u);
    EXPECT_EQ(std::get<std::string>(back->entities[0].metadata.at("source")), "Site Survey.12daz");
}

TEST(Archive12dExport, ANameThatIsNotAnArchivesIsRefusedBeforeAnythingIsWritten)
{
    const TempDir dir("export-name");
    const auto model = smallDrawing();
    const auto wrong = interop::exportArchive12d(model, {}, dir / "drawing.dxf");
    ASSERT_FALSE(wrong.ok());
    EXPECT_EQ(wrong.error().code, ErrorCode::InvalidArgument);
    EXPECT_FALSE(std::filesystem::exists(dir / "drawing.dxf"));
}

TEST(Archive12dExport, AnOriginShiftTakenOnImportIsGivenBackOnExport)
{
    const TempDir dir("export-shift");
    interop::Archive12dImportOptions in;
    in.originShift = katana::geometry::Vec2(502000.0, 6960000.0);
    auto imported = interop::importArchive12d(fixture("all_geometries.12da"), in);
    ASSERT_TRUE(imported.ok());
    // The first entity is the permanent mark at 502000, 6960000.
    EXPECT_EQ(std::get<katana::entity::PointGeometry>(imported->entities.at(0).geometry).position,
              Point2(0.0, 0.0));
    EXPECT_LT(imported->bounds.max.x, 2000.0);
    ASSERT_EQ(imported->surfaces.size(), 1u);
    EXPECT_LT(imported->surfaces[0].surface.bounds().max.x, 2000.0) << "surfaces move with it";
    ASSERT_EQ(imported->clouds.size(), 1u);
    EXPECT_EQ(imported->clouds[0].bounds.minX, 1700.0) << "and so do clouds";

    katana::entity::Model model;
    for (const auto& layer : imported->layersNeeded) {
        ASSERT_TRUE(model.layers.add(layer).ok());
    }
    ASSERT_TRUE(model.entities.add(imported->entities.at(0)).ok());
    interop::Archive12dExportOptions out;
    out.originShift = in.originShift;
    ASSERT_TRUE(interop::exportArchive12d(model, {}, dir / "back.12da", out).ok());
    const auto back = interop::importArchive12d(dir / "back.12da");
    ASSERT_TRUE(back.ok());
    EXPECT_EQ(std::get<katana::entity::PointGeometry>(back->entities.at(0).geometry).position,
              Point2(502000.0, 6960000.0));
}
