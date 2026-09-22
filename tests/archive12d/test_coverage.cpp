#include <gtest/gtest.h>

#include <algorithm>
#include <map>
#include <set>
#include <string>

#include "katana/archive12d/coverage.hpp"
#include "katana/archive12d/domain.hpp"
#include "katana/archive12d/reader.hpp"

namespace a12 = katana::archive12d;

namespace {

// The smallest instance of each element the manual defines that still says
// something - one per row of elementCoverage(). Keyed by the row's keyword.
const std::map<std::string, std::string>& minimalInstances()
{
    static const std::map<std::string, std::string> instances = {
        {"model", "model \"M\" string super { data_2d { 0 0 } }"},
        {"colour", "colour blue string super { data_2d { 0 0 } }"},
        {"style", "style \"Kerb\" string super { data_2d { 0 0 } }"},
        {"breakline", "breakline line string super { data_2d { 0 0 } }"},
        {"null", "null -1 string super { data_3d { 0 0 -1 } }"},
        {"attributes", "string super { attributes { text \"a\" \"b\" } data_2d { 0 0 } }"},
        {"project_attributes", "project_attributes { text \"a\" \"b\" }"},
        {"tin", "tin { name \"T\" points { 0 0 0 1 0 0 0 1 0 } triangles { 1 3 2 } }"},
        {"full_tin", "full_tin { name \"T\" points { 0 0 0 0 9 0 9 9 0 9 0 0 1 1 1 2 1 1 1 2 1 }"
                     " triangles { 5 7 6 } neighbours { 0 0 0 } nulling { 2 } }"},
        {"super_tin", "super_tin { name \"S\" tins { \"T\" } }"},
        {"primitive_3d", "primitive_3d { name M colour red trimesh_3d { vertices { 0 0 0 1 0 0 0 1 0 }"
                         " faces { 1 2 3 } } }"},
        {"string arc", "string arc { radius 1 xcentre 0 ycentre 0 zcentre 0 xstart 1 ystart 0 zstart 0"
                       " xend 0 yend 1 zend 0 }"},
        {"string circle", "string circle { radius 1 xcentre 0 ycentre 0 zcentre 0 }"},
        {"string drainage", "string drainage { data { 0 0 0 0 0  1 1 0 0 0 } pit { name \"P\" x 0 y 0 z 1 } }"},
        {"string face", "string face { data { 0 0 0  1 0 0  1 1 0 } }"},
        {"string feature", "string feature { radius 1 xcentre 0 ycentre 0 zcentre 0 }"},
        {"string interface", "string interface { data { 0 0 0 -1  1 1 0 1 } }"},
        {"string plot_frame", "string plot_frame { width 420 height 297 scale 500 xorigin 0 yorigin 0 }"},
        {"string super", "string super { data_3d { 0 0 0  1 1 0 } }"},
        {"string super_alignment", "string super_alignment { name \"A\" horizontal_parts {"
                                   " ip { id 100 x 0 y 0 } ip { id 200 x 9 y 0 } } }"},
        {"string text", "string text { x 0 y 0 z 0 text \"T\" }"},
        {"string 2d", "string 2d { z 1 data { 0 0  1 1 } }"},
        {"string 3d", "string 3d { data { 0 0 0  1 1 0 } }"},
        {"string 4d", "string 4d { data { 0 0 0 \"a\"  1 1 0 \"b\" } }"},
        {"string pipe", "string pipe { diameter 0.3 data { 0 0 0  1 1 0 } }"},
        {"string polyline", "string polyline { data { 0 0 0 0 0  1 1 0 0 0 } }"},
        {"string alignment", "string alignment { name \"A\" hipdata { 0 0 0  9 0 0 } }"},
        {"string pipeline", "string pipeline { name \"P\" diameter 0.3 hipdata { 0 0 0  9 0 0 } }"},
        {"string las_cloud_data", "string las_cloud_data { data { format v10_p0"
                                  " points_v10_p0 { p { x 0 y 0 z 0 } } } }"},
    };
    return instances;
}

std::size_t thingsImported(const a12::DomainImport& domain)
{
    return domain.entities.size() + domain.alignments.size() + domain.surfaces.size() +
           domain.clouds.size() + domain.meshes.size();
}

} // namespace

TEST(Coverage, EveryElementOfTheFormatIsReadAndNoneIsLeftUnrecognised)
{
    std::set<std::string> seen;
    for (const a12::ElementCoverage& row : a12::elementCoverage()) {
        const std::string keyword(row.keyword);
        EXPECT_TRUE(seen.insert(keyword).second) << keyword << " is listed twice";
        const auto instance = minimalInstances().find(keyword);
        ASSERT_NE(instance, minimalInstances().end())
            << keyword << " is in the coverage table but this test has no instance of it";

        const auto archive = a12::readArchive(instance->second);
        ASSERT_TRUE(archive.ok()) << keyword << ": " << archive.error().describe();
        EXPECT_TRUE(archive->unrecognised.empty())
            << keyword << " is claimed as covered but the reader skipped "
            << archive->unrecognised.begin()->first;
        EXPECT_TRUE(archive->warnings.empty()) << keyword << ": " << archive->warnings.front();

        // An element that IS one (rather than a command or a block inside one)
        // must come out under its own keyword.
        const bool isElement = keyword.starts_with("string ") || keyword == "tin" ||
                               keyword == "full_tin" || keyword == "super_tin" ||
                               keyword == "primitive_3d";
        if (isElement) {
            ASSERT_EQ(archive->elements.size(), 1u) << keyword;
            EXPECT_EQ(a12::elementKeyword(archive->elements[0]), keyword);
        }

        // And the handling the table claims is the handling it gets.
        const auto domain = a12::toDomain(*archive);
        ASSERT_TRUE(domain.ok()) << keyword;
        if (row.handling == a12::Handling::ReadOnly) {
            EXPECT_EQ(thingsImported(*domain), 0u) << keyword << " is listed as read-only";
        } else {
            EXPECT_GT(thingsImported(*domain), 0u) << keyword << " is listed as imported";
        }
    }
    EXPECT_EQ(seen.size(), minimalInstances().size())
        << "this test holds an instance of an element the coverage table does not list";
}

TEST(Coverage, TheTableListsEveryStringTypeTheManualDefines)
{
    // Manual 1.5: the current types 1.5.1 - 1.5.10, the superseded 1.5.11 -
    // 1.5.17, and the LAS cloud 1.5.18 - eighteen in all.
    std::size_t strings = 0;
    std::set<std::string> sections;
    for (const a12::ElementCoverage& row : a12::elementCoverage()) {
        if (row.keyword.starts_with("string ")) {
            ++strings;
            EXPECT_TRUE(sections.insert(std::string(row.manualSection)).second)
                << "two string types claim section " << row.manualSection;
        }
    }
    EXPECT_EQ(strings, 18u);
    for (int section = 1; section <= 18; ++section) {
        EXPECT_TRUE(sections.contains("1.5." + std::to_string(section))) << "1.5." << section;
    }
}
