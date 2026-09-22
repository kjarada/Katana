// A 12d trimesh becomes a mesh in the session and goes back out as one
// (PLAN.MD 20.2, slice 4).
#include <gtest/gtest.h>

#include <algorithm>
#include <string>

#include "katana/archive12d/domain.hpp"
#include "katana/archive12d/reader.hpp"
#include "katana/archive12d/writer.hpp"
#include "katana/entity/model.hpp"

namespace a12 = katana::archive12d;
using katana::geometry::Point2;

namespace {

// These fixtures are hand-written, and a hand-written 12da inherits the
// format's CURRENT BREAKLINE TYPE, whose default is `point` (commands,
// 1.4.4). A fixture that means a line has to say so, exactly as every string
// in a real archive does - 12d writes the flag on all 25,659 strings of a
// production file and leaves nothing to the default. Rather than repeat it in
// every fixture, the helper states it once at file level, which is the
// format's own way of saying it; a fixture that wants POINTS says
// `breakline point` inside the string and overrides this.
constexpr const char* kBreaklineLine = "breakline line\n";

a12::DomainImport import(const std::string& text)
{
    auto archive = a12::readArchive(kBreaklineLine + text);
    EXPECT_TRUE(archive.ok()) << (archive.ok() ? "" : archive.error().describe());
    auto domain = a12::toDomain(archive.ok() ? *archive : a12::Archive{}, {});
    EXPECT_TRUE(domain.ok()) << (domain.ok() ? "" : domain.error().describe());
    return domain.ok() ? std::move(*domain) : a12::DomainImport{};
}

bool anyContains(const std::vector<std::string>& warnings, const std::string& needle)
{
    return std::any_of(warnings.begin(), warnings.end(),
                       [&](const std::string& w) { return w.find(needle) != std::string::npos; });
}

std::string joined(const std::vector<std::string>& warnings)
{
    std::string out;
    for (const auto& w : warnings) {
        out += "\n  " + w;
    }
    return out;
}

// A square pyramid, as 12d writes a stockpile: four base corners and an apex,
// four triangles. Indices in the file are ONE-based.
constexpr const char* kPyramid = R"(primitive_3d {
  name Stockpile
  model "Site Mesh"
  colour brown
  attributes { text "Material" "Crushed Rock" }
  trimesh_3d {
    info { flag 0 key 1 colour brown name Stockpile }
    vertices {
      0 0 0
      20 0 0
      20 20 0
      0 20 0
      10 10 10
    }
    faces { 1 2 5  2 3 5  3 4 5  4 1 5 }
  }
})";

} // namespace

TEST(MeshImport, ATrimeshBecomesAMeshInTheSessionWithItsNameLayerColourAndAttributes)
{
    const auto domain = import(kPyramid);
    ASSERT_EQ(domain.meshes.size(), 1u);
    const a12::ImportedMesh& mesh = domain.meshes[0];
    EXPECT_EQ(mesh.name, "Stockpile");
    EXPECT_EQ(mesh.layer, "Site Mesh") << "a 12d model is a layer path";
    EXPECT_EQ(mesh.colourName, "brown");
    EXPECT_EQ(mesh.color, a12::standardColour("brown"));
    EXPECT_EQ(std::get<std::string>(mesh.properties.at("Material")), "Crushed Rock");

    // Five vertices, four triangles; the apex is 10 above a 20 x 20 base.
    ASSERT_EQ(mesh.mesh.vertices.size(), 5u);
    EXPECT_EQ(mesh.mesh.triangleCount(), 4u);
    EXPECT_EQ(mesh.mesh.vertices[4], katana::geometry::Point3(10, 10, 10));
    // One-based in the file, zero-based here: face 1 is vertices 1, 2 and 5.
    EXPECT_EQ(mesh.mesh.faces[0], (std::array<std::uint32_t, 3>{0, 1, 4}));
    const auto box = mesh.mesh.bounds();
    EXPECT_EQ(box.min, katana::geometry::Point3(0, 0, 0));
    EXPECT_EQ(box.max, katana::geometry::Point3(20, 20, 10));
    // Each side of the pyramid spans a 20 m base edge and rises 10 m over
    // the 10 m to the middle: half of 20 by sqrt(10^2 + 10^2) = 141.42...
    EXPECT_NEAR(mesh.mesh.area(), 4.0 * 0.5 * 20.0 * std::sqrt(200.0), 1e-9);
    // Its footprint is the base square.
    EXPECT_EQ(mesh.mesh.planHull().size(), 4u);
    EXPECT_TRUE(domain.warnings.empty()) << joined(domain.warnings);
}

TEST(MeshImport, PerFaceColoursComeFromTheOneBasedFlagsIntoTheInfoTable)
{
    // Manual 1.4.9: face_flags index face_infos from 1, and 0 means none.
    const auto domain = import(R"(primitive_3d { name Painted
  trimesh_3d {
    vertices { 0 0 0  1 0 0  1 1 0  0 1 0 }
    faces { 1 2 3  1 3 4 }
    face_infos { 1 255 red first  2 255 blue second }
    face_flags { 2 1 }
  } })");
    ASSERT_EQ(domain.meshes.size(), 1u);
    const a12::ImportedMesh& mesh = domain.meshes[0];
    ASSERT_EQ(mesh.faceColourNames.size(), 2u);
    EXPECT_EQ(mesh.faceColourNames[0], "blue") << "flag 2 is the SECOND info";
    EXPECT_EQ(mesh.faceColourNames[1], "red");
    EXPECT_EQ(mesh.faceColors[0], a12::standardColour("blue"));
    EXPECT_EQ(mesh.faceColors[1], a12::standardColour("red"));
}

TEST(MeshImport, AMeshWithNoColouredFaceHasNoTableRatherThanATableOfBlanks)
{
    const auto domain = import(R"(primitive_3d { name Plain
  trimesh_3d { vertices { 0 0 0  1 0 0  1 1 0 } faces { 1 2 3 } } })");
    ASSERT_EQ(domain.meshes.size(), 1u);
    EXPECT_TRUE(domain.meshes[0].faceColourNames.empty())
        << "a table read by position with every entry blank is not a table";
    EXPECT_TRUE(domain.meshes[0].faceColors.empty());
}

TEST(MeshImport, AFaceThatRepeatsAVertexOrUsesOneWithNoHeightIsDroppedAndSaidSo)
{
    // Three sound faces, and two that name a point in space they cannot: one
    // repeats a vertex, one uses a vertex with no height (12d's null).
    const auto domain = import(R"(primitive_3d { name Broken
  trimesh_3d {
    vertices { 0 0 0  1 0 0  1 1 0  0 1 0  2 2 null }
    faces { 1 2 3  1 3 4  2 3 4  1 1 2  1 2 5 }
  } })");
    ASSERT_EQ(domain.meshes.size(), 1u);
    EXPECT_EQ(domain.meshes[0].mesh.triangleCount(), 3u) << "the three sound faces";
    EXPECT_TRUE(validate(domain.meshes[0].mesh).ok()) << "what is imported is always valid";
    EXPECT_TRUE(anyContains(domain.warnings, "2 mesh faces were dropped"))
        << joined(domain.warnings);
}

TEST(MeshImport, AFaceNamingAVertexThatDoesNotExistIsRefusedByTheReaderNotTheImport)
{
    // The check belongs where it can say which line: an index past the end is
    // a broken FILE, not a face to drop quietly.
    const auto archive = a12::readArchive(R"(primitive_3d { name Broken
  trimesh_3d { vertices { 0 0 0  1 0 0  1 1 0 } faces { 1 2 99 } } })");
    ASSERT_FALSE(archive.ok());
    EXPECT_NE(archive.error().describe().find("names point 99"), std::string::npos)
        << archive.error().describe();
}

TEST(MeshImport, AMeshWithNoSoundFaceIsNotImportedAndIsCounted)
{
    // Every vertex null: nothing of it stands anywhere in space.
    const auto domain = import(R"(primitive_3d { name Hopeless
  trimesh_3d { vertices { 0 0 null  1 0 null  1 1 null } faces { 1 2 3 } } })");
    EXPECT_TRUE(domain.meshes.empty());
    EXPECT_TRUE(anyContains(domain.warnings, "1 trimeshes have no triangle"))
        << joined(domain.warnings);
    const auto tally = std::find_if(domain.tally.begin(), domain.tally.end(),
                                    [](const auto& t) { return t.keyword == "primitive_3d"; });
    ASSERT_NE(tally, domain.tally.end());
    EXPECT_EQ(tally->read, 1u);
    EXPECT_EQ(tally->imported, 0u);
}

TEST(MeshExport, AMeshGoesBackOutAsAPrimitive3dWithItsFacesOneBasedAgain)
{
    const auto first = import(kPyramid);
    ASSERT_EQ(first.meshes.size(), 1u);
    a12::ExportMesh out;
    out.name = first.meshes[0].name;
    out.layer = first.meshes[0].layer;
    out.colourName = first.meshes[0].colourName;
    out.mesh = &first.meshes[0].mesh;

    const katana::entity::Model empty;
    auto exported = a12::fromDomain(empty, {}, {}, {out});
    ASSERT_TRUE(exported.ok()) << (exported.ok() ? "" : exported.error().describe());
    EXPECT_EQ(exported->meshesWritten, 1u);

    // Through the writer and back: the same mesh.
    auto back = a12::readArchive(a12::writeArchive(exported->archive));
    ASSERT_TRUE(back.ok()) << (back.ok() ? "" : back.error().describe());
    auto again = a12::toDomain(*back, {});
    ASSERT_TRUE(again.ok());
    ASSERT_EQ(again->meshes.size(), 1u);
    const a12::ImportedMesh& round = again->meshes[0];
    EXPECT_EQ(round.name, "Stockpile");
    EXPECT_EQ(round.layer, "Site Mesh");
    EXPECT_EQ(round.colourName, "brown");
    EXPECT_EQ(round.mesh.vertices, first.meshes[0].mesh.vertices);
    EXPECT_EQ(round.mesh.faces, first.meshes[0].mesh.faces);
}

TEST(MeshExport, FaceColoursBecomeOneInfoPerDistinctColourAndFlagsThatIndexIt)
{
    katana::geometry::TriangleMesh mesh;
    mesh.vertices = {katana::geometry::Point3(0, 0, 0), katana::geometry::Point3(1, 0, 0),
                     katana::geometry::Point3(1, 1, 0), katana::geometry::Point3(0, 1, 0)};
    mesh.faces = {{0, 1, 2}, {0, 2, 3}};
    a12::ExportMesh out;
    out.name = "Painted";
    out.mesh = &mesh;
    // Both faces red: one info, not two, because an info is a colour and not
    // a face.
    out.faceColourNames = {"red", "red"};

    const katana::entity::Model empty;
    auto exported = a12::fromDomain(empty, {}, {}, {out});
    ASSERT_TRUE(exported.ok());
    const auto& written = std::get<a12::Trimesh>(exported->archive.elements.at(0));
    ASSERT_EQ(written.faceInfos.size(), 1u);
    EXPECT_EQ(written.faceInfos[0].colour, "red");
    EXPECT_EQ(written.faceFlags, (std::vector<std::uint32_t>{1, 1}));
    // And one-based indices in the file itself.
    EXPECT_NE(a12::writeArchive(exported->archive).find("faces"), std::string::npos);

    // Two colours and one uncoloured face: two infos, and a 0 for the face
    // that has none.
    out.faceColourNames = {"red", "blue"};
    mesh.faces.push_back({1, 2, 3});
    out.faceColourNames.push_back("");
    auto second = a12::fromDomain(empty, {}, {}, {out});
    ASSERT_TRUE(second.ok());
    const auto& twice = std::get<a12::Trimesh>(second->archive.elements.at(0));
    ASSERT_EQ(twice.faceInfos.size(), 2u);
    EXPECT_EQ(twice.faceFlags, (std::vector<std::uint32_t>{1, 2, 0}));
}
