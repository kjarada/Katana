// The round trip, on the fixtures (PLAN.MD 20.2, slice 9).
//
// Import a real archive, put everything it gave into a model, export that,
// and import the export: the second reading must hold what the first did.
// This is the test that would have caught every loss the earlier slices
// fixed one at a time - a style dropped, a colour overwritten, a symbol
// turned into metadata and left there - because it compares the WHOLE of
// what came back rather than the field someone thought to look at.
//
// The large archives are not in the repository (they are real project data);
// `katana_12da_probe --roundtrip` does the same comparison on those, and
// docs/interop.md records what it says about them.

#include <gtest/gtest.h>

#include <algorithm>
#include <charconv>
#include <cmath>
#include <filesystem>
#include <map>
#include <string>

#include "katana/archive12d/domain.hpp"
#include "katana/entity/model.hpp"
#include "katana/interop/archive12d.hpp"

namespace a12 = katana::archive12d;
namespace interop = katana::interop;

namespace {

class TempDir {
  public:
    explicit TempDir(const std::string& name)
        : path_(std::filesystem::temp_directory_path() / ("katana-roundtrip-" + name))
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

// Everything an import gave, put into a model the way the application does.
katana::entity::Model modelOf(const interop::Archive12dImportResult& imported)
{
    katana::entity::Model model;
    for (const auto& layer : imported.layersNeeded) {
        EXPECT_TRUE(model.layers.add(layer).ok()) << layer.name;
    }
    for (const auto& style : imported.stylesNeeded) {
        EXPECT_TRUE(model.styles.add(style).ok()) << style.name;
    }
    for (const auto& entity : imported.entities) {
        EXPECT_TRUE(model.entities.add(entity).ok());
    }
    for (const auto& alignment : imported.alignments) {
        EXPECT_TRUE(model.alignments.add(alignment).ok()) << alignment.name;
    }
    return model;
}

// What a string is, reduced to what must survive: everything the domain
// carries about it except the id, which is Katana's and not the file's.
struct Summary {
    std::string layer;
    std::string style;
    std::string type;
    std::size_t vertices = 0;
    katana::entity::PropertyMap properties;
    katana::entity::PropertyMap metadata;

    friend bool operator==(const Summary&, const Summary&) = default;
};

std::size_t vertexCount(const katana::entity::Geometry& geometry)
{
    struct Visitor {
        std::size_t operator()(const katana::entity::PointGeometry&) const { return 1; }
        std::size_t operator()(const katana::geometry::Segment2&) const { return 2; }
        std::size_t operator()(const katana::geometry::Polyline2& p) const
        {
            return p.vertices.size();
        }
        std::size_t operator()(const katana::geometry::Arc2&) const { return 0; }
        std::size_t operator()(const katana::geometry::Circle2&) const { return 0; }
        std::size_t operator()(const katana::entity::TextGeometry&) const { return 1; }
        std::size_t operator()(const katana::entity::DimensionGeometry&) const { return 2; }
        std::size_t operator()(const katana::entity::LabelGeometry&) const { return 0; }
        std::size_t operator()(const katana::entity::LeaderGeometry& l) const
        {
            return l.vertices.size();
        }
    };
    return std::visit(Visitor{}, geometry);
}

// Keyed by name and position so the comparison does not depend on the order
// the exporter happened to write things in.
std::multimap<std::string, Summary> summarise(const interop::Archive12dImportResult& imported)
{
    std::multimap<std::string, Summary> out;
    for (const auto& entity : imported.entities) {
        Summary summary;
        summary.layer = entity.layer;
        summary.style = entity.style;
        summary.type = katana::entity::toString(entity.type());
        summary.vertices = vertexCount(entity.geometry);
        summary.properties = entity.properties;
        // Heights at the precision the FILE is written to. A 12da carries
        // eight decimal places by default - 12d Model's own
        // (WriteOptions::decimalPlaces) - so a height that has been through
        // the format differs from one computed in memory in the tenth digit,
        // which is a nanometre of ground. Comparing the text of the doubles
        // would call that a difference; rounding both sides to what the
        // format holds compares what the file actually says.
        for (const char* key : {"elevation", "elevations"}) {
            const auto found = summary.properties.find(key);
            if (found == summary.properties.end()) {
                continue;
            }
            if (const auto* one = std::get_if<double>(&found->second)) {
                found->second = std::round(*one * 1e8) / 1e8;
                continue;
            }
            const auto* list = std::get_if<std::string>(&found->second);
            if (list == nullptr) {
                continue;
            }
            std::string rounded;
            std::size_t at = 0;
            while (at < list->size()) {
                const std::size_t space = list->find(' ', at);
                const std::string item =
                    list->substr(at, space == std::string::npos ? std::string::npos : space - at);
                double value = 0.0;
                const auto end = item.data() + item.size();
                const bool numeric =
                    std::from_chars(item.data(), end, value).ptr == end;
                rounded += (rounded.empty() ? "" : " ");
                rounded += numeric ? katana::entity::toString(katana::entity::PropertyValue(
                                         std::round(value * 1e8) / 1e8))
                                   : item;
                if (space == std::string::npos) {
                    break;
                }
                at = space + 1;
            }
            found->second = rounded;
        }
        summary.metadata = entity.metadata;
        // The source file is the file it came from, which differs by design.
        summary.metadata.erase("source");
        const auto name = summary.metadata.find("12d.name");
        std::string key = name == summary.metadata.end()
                              ? std::string()
                              : std::get<std::string>(name->second);
        out.emplace(key + "|" + summary.type + "|" + std::to_string(summary.vertices),
                    std::move(summary));
    }
    return out;
}

std::map<std::string, katana::entity::Style>
stylesByName(const interop::Archive12dImportResult& imported)
{
    std::map<std::string, katana::entity::Style> out;
    for (const auto& style : imported.stylesNeeded) {
        out.emplace(style.name, style);
    }
    return out;
}

// Import, export, import, export, import.
//
// The FIRST export normalises: the format has several spellings of one thing
// and Katana writes one of them. An arc goes out as a two-vertex super
// string (docs/interop.md), a drainage string as its line plus a point per
// pit, a full_tin as a tin. So the first reading and the second are not
// expected to match string for string - and everything after that is, since
// the drawing is then already in the form Katana writes.
//
// What the first pass must not do is LOSE anything: a style, a symbol on a
// style, a layer, a mesh, a mesh's face colours, or ground.
std::vector<interop::Archive12dImportResult> passes(const std::string& fixtureName, int count)
{
    TempDir directory(fixtureName);
    std::vector<interop::Archive12dImportResult> results;
    auto current = interop::importArchive12d(fixture(fixtureName));
    EXPECT_TRUE(current.ok()) << (current.ok() ? "" : current.error().describe());
    if (!current.ok()) {
        return results;
    }
    results.push_back(std::move(*current));
    for (int pass = 1; pass < count; ++pass) {
        const interop::Archive12dImportResult& previous = results.back();
        const katana::entity::Model model = modelOf(previous);
        std::vector<a12::ExportSurface> surfaces;
        for (const auto& surface : previous.surfaces) {
            surfaces.push_back(a12::ExportSurface{surface.name, &surface.surface});
        }
        std::vector<a12::ExportMesh> meshes;
        for (const auto& mesh : previous.meshes) {
            meshes.push_back(a12::ExportMesh{mesh.name, mesh.layer, mesh.colourName, &mesh.mesh,
                                             mesh.faceColourNames});
        }
        const auto out = directory / ("pass" + std::to_string(pass) + ".12da");
        const auto written = interop::exportArchive12d(model, surfaces, out, {}, meshes);
        EXPECT_TRUE(written.ok()) << (written.ok() ? "" : written.error().describe());
        auto next = interop::importArchive12d(out);
        EXPECT_TRUE(next.ok()) << (next.ok() ? "" : next.error().describe());
        if (!next.ok()) {
            break;
        }
        results.push_back(std::move(*next));
    }
    return results;
}

void roundTrip(const std::string& fixtureName)
{
    SCOPED_TRACE(fixtureName);
    const auto results = passes(fixtureName, 3);
    ASSERT_EQ(results.size(), 3u);
    const auto& first = results[0];
    const auto& second = results[1];
    const auto& third = results[2];

    // ---- nothing is lost on the way out ------------------------------------
    // By name, not in order: the list is in the order the styles were first
    // met, and the export writes the elements in its own order.
    EXPECT_EQ(stylesByName(second), stylesByName(first))
        << "every style, with its symbol and size, came back";
    EXPECT_EQ(second.layersNeeded.size(), first.layersNeeded.size()) << "layers";
    ASSERT_EQ(second.meshes.size(), first.meshes.size()) << "meshes";
    for (std::size_t i = 0; i < first.meshes.size(); ++i) {
        EXPECT_EQ(second.meshes[i].mesh.faces, first.meshes[i].mesh.faces)
            << "mesh " << first.meshes[i].name;
        EXPECT_EQ(second.meshes[i].mesh.vertices, first.meshes[i].mesh.vertices);
        EXPECT_EQ(second.meshes[i].faceColourNames, first.meshes[i].faceColourNames);
    }
    EXPECT_GE(second.entities.size(), first.entities.size())
        << "a string may become two (a drainage line and its pits), never none";
    EXPECT_GE(second.surfaces.size(), first.surfaces.size()) << "surfaces";
    for (std::size_t i = 0; i < first.surfaces.size() && i < second.surfaces.size(); ++i) {
        EXPECT_EQ(second.surfaces[i].surface.triangleCount(),
                  first.surfaces[i].surface.triangleCount())
            << "surface " << first.surfaces[i].name;
    }

    // ---- and the form Katana writes is stable ------------------------------
    // This is the assertion with teeth: once the drawing is in Katana's own
    // spelling, every further trip through the format must change nothing at
    // all. A field quietly dropped or added on export shows up here even
    // when the first pass could excuse it as normalisation.
    EXPECT_EQ(third.entities.size(), second.entities.size());
    EXPECT_EQ(stylesByName(third), stylesByName(second));
    EXPECT_EQ(third.layersNeeded.size(), second.layersNeeded.size());
    EXPECT_EQ(third.tally, second.tally) << "the same elements in the same numbers";
    ASSERT_EQ(third.meshes.size(), second.meshes.size());
    for (std::size_t i = 0; i < second.meshes.size(); ++i) {
        EXPECT_EQ(third.meshes[i].mesh.faces, second.meshes[i].mesh.faces);
        EXPECT_EQ(third.meshes[i].faceColourNames, second.meshes[i].faceColourNames);
    }

    const auto before = summarise(second);
    const auto after = summarise(third);
    EXPECT_EQ(before.size(), after.size());
    for (const auto& [key, summary] : before) {
        const auto range = after.equal_range(key);
        const bool found =
            std::any_of(range.first, range.second,
                        [&summary](const auto& pair) { return pair.second == summary; });
        EXPECT_TRUE(found) << "no string came back as: " << key << " on layer " << summary.layer
                           << " in style " << summary.style;
    }
}

} // namespace

TEST(RoundTrip, EveryGeometryComesBackAsItWent)
{
    roundTrip("all_geometries.12da");
}

TEST(RoundTrip, ADrainageStringComesBackWithItsPipesAndPits)
{
    roundTrip("drainage_strings.12da");
}

TEST(RoundTrip, SymbolsComeBackOnTheStylesThatCarryThem)
{
    roundTrip("symbols.12da");
}

TEST(RoundTrip, SeveralTinsComeBackAsSeveralSurfaces)
{
    roundTrip("multiple_tins.12da");
}

TEST(RoundTrip, ASuperTinComesBackWithItsMembers)
{
    roundTrip("super_tin.12da");
}

TEST(RoundTrip, AZippedArchiveRoundTripsAsItsPlainFormDoes)
{
    roundTrip("all_geometries.12daz");
}
