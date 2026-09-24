#include <gtest/gtest.h>

#include <cstdint>
#include <fstream>
#include <iterator>
#include <random>
#include <string>

#include "katana/dxf/reader.hpp"
#include "katana/entity/entity_geometry.hpp"

namespace dxf = katana::dxf;

namespace {

std::string fixture(const char* name)
{
    std::ifstream file(std::string(KATANA_DXF_TEST_DATA) + "/" + name, std::ios::binary);
    return std::string(std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>());
}

// Whatever the reader said, it said it through the Result: a success holds
// only geometry the model accepts.
void expectSound(const katana::core::Result<dxf::DxfImport>& result)
{
    if (!result.ok()) {
        EXPECT_FALSE(result.error().message.empty());
        return;
    }
    for (const auto& entity : result->entities) {
        EXPECT_TRUE(katana::entity::validate(entity.geometry).ok());
        EXPECT_TRUE(katana::entity::isValidUtf8(entity.layer));
    }
}

} // namespace

TEST(DxfRobustness, SomethingThatIsNotADxfFileIsAnError)
{
    const auto result = dxf::readDxf("hello, world\nthis is not a drawing\n");
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, katana::core::ErrorCode::ParseFailure);
    EXPECT_NE(result.error().message.find("not a DXF file"), std::string::npos);
}

TEST(DxfRobustness, AnEmptyFileIsAnError)
{
    EXPECT_FALSE(dxf::readDxf("").ok());
}

TEST(DxfRobustness, ABinaryDxfIsRefusedByName)
{
    const auto result = dxf::readDxf(std::string("AutoCAD Binary DXF\r\n\x1a\0", 22));
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, katana::core::ErrorCode::Unsupported);
}

TEST(DxfRobustness, AGroupCodeThatIsNotANumberIsAnErrorThatSaysWhere)
{
    const auto result = dxf::readDxf("  0\nSECTION\n  2\nENTITIES\nten\nLINE\n");
    ASSERT_FALSE(result.ok());
    EXPECT_NE(result.error().message.find("line 5"), std::string::npos);
}

TEST(DxfRobustness, ATruncatedFileKeepsWhatWasReadAndSaysItIsTruncated)
{
    const std::string whole = fixture("r2000_site.dxf");
    // Cut inside the ENTITIES section, after the LWPOLYLINE and the MTEXT.
    const std::size_t cut = whole.find("  0\nLINE\n  5\n52");
    ASSERT_NE(cut, std::string::npos);
    const auto result = dxf::readDxf(std::string_view(whole).substr(0, cut + 7));
    ASSERT_TRUE(result.ok());
    // The polyline and the three lines of the MTEXT.
    EXPECT_EQ(result->entities.size(), 4u);
    ASSERT_FALSE(result->warnings.empty());
    EXPECT_NE(result->warnings.front().find("truncated"), std::string::npos);
}

TEST(DxfRobustness, ANumberThatIsNotANumberCostsOnlyItsEntity)
{
    const std::string text = "  0\nSECTION\n  2\nENTITIES\n"
                             "  0\nLINE\n  8\n0\n 10\nabc\n 20\n0\n 11\n1\n 21\n1\n"
                             "  0\nLINE\n  8\n0\n 10\n0\n 20\n0\n 11\n1\n 21\n1\n"
                             "  0\nENDSEC\n  0\nEOF\n";
    const auto result = dxf::readDxf(text);
    ASSERT_TRUE(result.ok());
    EXPECT_EQ(result->entities.size(), 1u);
    ASSERT_FALSE(result->warnings.empty());
}

TEST(DxfRobustness, ABlockThatInsertsItselfIsNotExpandedForever)
{
    const std::string text = "  0\nSECTION\n  2\nBLOCKS\n"
                             "  0\nBLOCK\n  2\nLOOP\n 10\n0\n 20\n0\n"
                             "  0\nLINE\n 10\n0\n 20\n0\n 11\n1\n 21\n0\n"
                             "  0\nINSERT\n  2\nLOOP\n 10\n5\n 20\n0\n"
                             "  0\nENDBLK\n  0\nENDSEC\n"
                             "  0\nSECTION\n  2\nENTITIES\n"
                             "  0\nINSERT\n  2\nLOOP\n 10\n0\n 20\n0\n"
                             "  0\nENDSEC\n  0\nEOF\n";
    const auto result = dxf::readDxf(text);
    ASSERT_TRUE(result.ok());
    // The block's own line, once; the insert of itself inside it is refused.
    EXPECT_EQ(result->entities.size(), 1u);
}

TEST(DxfRobustness, AHugeArrayInsertIsCappedNotAllocated)
{
    const std::string text = "  0\nSECTION\n  2\nBLOCKS\n"
                             "  0\nBLOCK\n  2\nDOT\n 10\n0\n 20\n0\n"
                             "  0\nPOINT\n 10\n0\n 20\n0\n"
                             "  0\nENDBLK\n  0\nENDSEC\n"
                             "  0\nSECTION\n  2\nENTITIES\n"
                             "  0\nINSERT\n  2\nDOT\n 10\n0\n 20\n0\n 70\n2000000000\n 71\n"
                             "2000000000\n 44\n1\n 45\n1\n"
                             "  0\nENDSEC\n  0\nEOF\n";
    dxf::ImportOptions options;
    options.maximumEntities = 1000;
    const auto result = dxf::readDxf(text, options);
    ASSERT_TRUE(result.ok());
    EXPECT_LE(result->entities.size(), 1000u);
}

// Thousands of damaged copies of the two fixtures - bytes changed, lines cut
// out, lines repeated, the file cut short - each read to the end. The
// generator is the standard's mt19937, whose sequence the standard fixes, so
// every run damages the same way and a failure reproduces.
TEST(DxfRobustness, DamagedInputNeverCrashesAndOnlyEverYieldsValidGeometry)
{
    std::mt19937 random(20260924u);
    const auto below = [&](std::size_t bound) {
        return bound == 0 ? std::size_t{0} : static_cast<std::size_t>(random() % bound);
    };
    for (const char* name : {"r12_survey.dxf", "r2000_site.dxf"}) {
        const std::string original = fixture(name);
        ASSERT_FALSE(original.empty()) << name;
        for (int round = 0; round < 1500; ++round) {
            std::string damaged = original;
            const int edits = 1 + static_cast<int>(below(8));
            for (int edit = 0; edit < edits && !damaged.empty(); ++edit) {
                const std::size_t at = below(damaged.size());
                switch (below(5)) {
                case 0: // a byte changed to anything
                    damaged[at] = static_cast<char>(random() & 0xFF);
                    break;
                case 1: { // a line cut out
                    const std::size_t end = damaged.find('\n', at);
                    damaged.erase(at, end == std::string::npos ? std::string::npos : end - at + 1);
                    break;
                }
                case 2: { // a line repeated
                    const std::size_t end = damaged.find('\n', at);
                    const std::string line =
                        damaged.substr(at, end == std::string::npos ? std::string::npos
                                                                    : end - at + 1);
                    damaged.insert(at, line);
                    break;
                }
                case 3: // cut short
                    damaged.resize(at);
                    break;
                default: // a digit where a digit was, so the pairs stay aligned
                    if (damaged[at] >= '0' && damaged[at] <= '9') {
                        damaged[at] = static_cast<char>('0' + below(10));
                    }
                    break;
                }
            }
            expectSound(dxf::readDxf(damaged));
        }
    }
}
