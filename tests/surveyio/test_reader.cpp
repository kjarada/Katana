#include <gtest/gtest.h>

#include <atomic>
#include <filesystem>
#include <fstream>
#include <stdexcept>
#include <string>
#include <vector>

#include "katana/surveyio/delimited_points.hpp"
#include "katana/surveyio/format.hpp"
#include "katana/surveyio/reader.hpp"

using namespace katana::surveyio;
using katana::core::ErrorCode;
using katana::core::Result;

namespace {

FormatSignature probeNothing(const ProbeInput&)
{
    return ruledOut();
}

FormatDescriptor fakeFormat(std::string id, bool canImport = true)
{
    FormatDescriptor format;
    format.id = std::move(id);
    format.humanName = "Fake instrument";
    format.manufacturer = Manufacturer::Other;
    format.reads = {.points = true, .observations = true, .stations = true};
    format.canImport = canImport;
    format.canExport = !canImport;
    format.parserVersion = "0.9";
    format.extensions = {"fak"};
    return format;
}

katana::survey::SurveyPoint point(std::string id)
{
    katana::survey::SurveyPoint p;
    p.id = std::move(id);
    p.northing = 100.0;
    p.easting = 200.0;
    return p;
}

// One point per line of input, named by the line; the sibling "extra.fak",
// when asked for and present, adds one more.
Result<ReadResult> readLines(std::string_view bytes, std::string_view fileName,
                             const ReadOptions& options)
{
    ReadResult result;
    std::size_t line = 0;
    std::size_t start = 0;
    while (start < bytes.size()) {
        const std::size_t cut = bytes.find('\n', start);
        const std::string_view text = bytes.substr(start, cut - start);
        start = cut == std::string_view::npos ? bytes.size() : cut + 1;
        ++line;
        if (text.empty()) {
            result.warnings.push_back(ReadWarning{std::string(fileName), line, "empty record"});
            ++result.recordsSkipped;
            continue;
        }
        result.project.points.push_back(point(std::string(text)));
        ++result.recordsRead;
    }
    if (options.siblings) {
        Result<std::string> extra = options.siblings("extra.fak");
        if (extra) {
            result.project.points.push_back(point(*extra));
        } else {
            result.warnings.push_back(ReadWarning{"extra.fak", 0, extra.error().message});
        }
    }
    result.notCarried.push_back("no instrument heights");
    return result;
}

// Asks for a sibling by a path taken from "the file" and reports what it got.
Result<ReadResult> readTraversal(std::string_view, std::string_view, const ReadOptions& options)
{
    ReadResult result;
    Result<std::string> sibling = options.siblings("..\\..\\secret.txt");
    result.warnings.push_back(ReadWarning{
        "", 0, sibling.ok() ? std::string("read it") : sibling.error().message});
    return result;
}

Result<ReadResult> readDuplicates(std::string_view, std::string_view, const ReadOptions&)
{
    ReadResult result;
    result.project.points = {point("1"), point("1")};
    return result;
}

Result<ReadResult> readThrowing(std::string_view, std::string_view, const ReadOptions&)
{
    throw std::runtime_error("record 3 made the reader throw");
}

FormatRegistry registryWith(FormatReader reader)
{
    FormatRegistry registry;
    EXPECT_TRUE(registry.add(fakeFormat("fake-lines"), &probeNothing, reader).ok());
    return registry;
}

} // namespace

TEST(SurveyReader, TheRegistryAcceptsAReaderBesideAProbe)
{
    FormatRegistry registry;
    ASSERT_TRUE(registry.add(fakeFormat("fake-lines"), &probeNothing, &readLines).ok());
    ASSERT_TRUE(registry.add(fakeFormat("fake-detect-only"), &probeNothing).ok());
    EXPECT_EQ(registry.reader("fake-lines"), &readLines);
    EXPECT_EQ(registry.reader("fake-detect-only"), nullptr);
    EXPECT_EQ(registry.reader("no-such-format"), nullptr);
}

TEST(SurveyReader, AReaderForAFormatThatCannotImportIsRefused)
{
    FormatRegistry registry;
    const auto status = registry.add(fakeFormat("fake-export", false), &probeNothing, &readLines);
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::InvalidArgument);
}

TEST(SurveyReader, ReadSurveyDispatchesToTheReaderAndStampsTheFormat)
{
    const FormatRegistry registry = registryWith(&readLines);
    const auto read = readSurvey(registry, "fake-lines", "A\n\nB\n", "C:\\jobs\\site.fak");
    ASSERT_TRUE(read.ok()) << read.error().describe();
    ASSERT_EQ(read->project.points.size(), 2U);
    EXPECT_EQ(read->project.points[1].id, "B");
    EXPECT_EQ(read->recordsRead, 2U);
    EXPECT_EQ(read->recordsSkipped, 1U);
    EXPECT_EQ(read->formatId, "fake-lines");
    EXPECT_EQ(read->parserVersion, "0.9");
    ASSERT_EQ(read->warnings.size(), 1U);
    // The reader was handed the file NAME, never the path.
    EXPECT_EQ(read->warnings[0].fileName, "site.fak");
    EXPECT_EQ(read->warnings[0].record, 2U);
    EXPECT_EQ(describe(read->warnings[0]), "site.fak record 2: empty record");
    EXPECT_TRUE(read->siblingsRead.empty());
}

TEST(SurveyReader, SiblingFilesAreServedByNameAndRecordedWithTheirBytes)
{
    const FormatRegistry registry = registryWith(&readLines);
    ReadOptions options;
    options.siblings = siblingsInMemory({SiblingFile{"EXTRA.FAK", "X"}});
    const auto read = readSurvey(registry, "fake-lines", "A\n", "site.fak", options);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    ASSERT_EQ(read->project.points.size(), 2U);
    EXPECT_EQ(read->project.points[1].id, "X"); // found ignoring ASCII case
    ASSERT_EQ(read->siblingsRead.size(), 1U);
    EXPECT_EQ(read->siblingsRead[0], (SiblingFile{"extra.fak", "X"}));
}

TEST(SurveyReader, AMissingSiblingIsNotFoundAndTheReaderCarriesOn)
{
    const FormatRegistry registry = registryWith(&readLines);
    ReadOptions options;
    options.siblings = siblingsInMemory({});
    const auto read = readSurvey(registry, "fake-lines", "A\n", "site.fak", options);
    ASSERT_TRUE(read.ok());
    ASSERT_EQ(read->warnings.size(), 1U);
    EXPECT_NE(read->warnings[0].message.find("no file called 'extra.fak'"), std::string::npos);
}

TEST(SurveyReader, ASiblingNamedByAPathIsRefusedBeforeAnyLookupRuns)
{
    std::atomic<int> lookups{0};
    ReadOptions options;
    options.siblings = [&lookups](std::string_view) -> Result<std::string> {
        ++lookups;
        return std::string("secret");
    };
    const FormatRegistry registry = registryWith(&readTraversal);
    const auto read = readSurvey(registry, "fake-lines", "x", "site.fak", options);
    ASSERT_TRUE(read.ok());
    EXPECT_EQ(lookups.load(), 0);
    EXPECT_NE(read->warnings.at(0).message.find("refused"), std::string::npos);
    EXPECT_TRUE(read->siblingsRead.empty());
}

TEST(SurveyReader, AReaderThatReturnsAnInconsistentProjectIsAnImportFailure)
{
    const FormatRegistry registry = registryWith(&readDuplicates);
    const auto read = readSurvey(registry, "fake-lines", "x", "site.fak");
    ASSERT_FALSE(read.ok());
    EXPECT_EQ(read.error().code, ErrorCode::FileImportFailure);
    EXPECT_NE(read.error().message.find("Fake instrument"), std::string::npos);
}

TEST(SurveyReader, AReaderThatThrowsIsReportedNotFatal)
{
    const FormatRegistry registry = registryWith(&readThrowing);
    const auto read = readSurvey(registry, "fake-lines", "x", "site.fak");
    ASSERT_FALSE(read.ok());
    EXPECT_EQ(read.error().code, ErrorCode::Internal);
    EXPECT_NE(read.error().message.find("record 3"), std::string::npos);
}

TEST(SurveyReader, AnUnknownFormatIsNotFoundAndAFormatWithoutAReaderIsUnsupported)
{
    FormatRegistry registry;
    ASSERT_TRUE(registry.add(fakeFormat("fake-detect-only"), &probeNothing).ok());
    EXPECT_EQ(readSurvey(registry, "no-such-format", "x", "a.fak").error().code,
              ErrorCode::NotFound);
    EXPECT_EQ(readSurvey(registry, "fake-detect-only", "x", "a.fak").error().code,
              ErrorCode::Unsupported);
}

TEST(SurveyReader, TheCoordinateFileFormatIsReadThroughItsLayoutNotAReader)
{
    const auto read =
        readSurvey(formatRegistry(), kDelimitedPointsFormatId, "1,100,200\n", "points.csv");
    ASSERT_FALSE(read.ok());
    EXPECT_EQ(read.error().code, ErrorCode::Unsupported);
    EXPECT_NE(read.error().context.find("column layout"), std::string::npos);
}

TEST(SurveyReader, AFolderLookupFindsAFileBesideTheJobIgnoringCaseAndNothingElse)
{
    namespace fs = std::filesystem;
    const fs::path root = fs::temp_directory_path() / "katana-surveyio-reader-test";
    fs::remove_all(root);
    fs::create_directories(root / "job");
    {
        std::ofstream(root / "job" / "JOB.X01", std::ios::binary) << "dbx bytes";
        std::ofstream(root / "outside.txt", std::ios::binary) << "not yours";
    }
    const SiblingLookup lookup = siblingsInFolder(root / "job");
    const auto found = lookup("job.x01");
    ASSERT_TRUE(found.ok()) << found.error().describe();
    EXPECT_EQ(*found, "dbx bytes");
    EXPECT_EQ(lookup("..\\outside.txt").error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(lookup("../outside.txt").error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(lookup("..").error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(lookup("missing.x02").error().code, ErrorCode::NotFound);
    // A cap smaller than the file refuses it.
    EXPECT_EQ(siblingsInFolder(root / "job", 4)("JOB.X01").error().code,
              ErrorCode::InvalidArgument);
    fs::remove_all(root);
}

TEST(SurveyReader, AReadResultBecomesTheWizardsImportResultWithNothingLost)
{
    ReadResult read;
    read.project.points = {point("1")};
    read.formatId = "fake-lines";
    read.recordsRead = 1;
    read.recordsSkipped = 2;
    read.warnings = {ReadWarning{"a.fak", 7, "bad angle"}, ReadWarning{"", 0, "no header"}};
    read.notCarried = {"no instrument heights"};
    const ImportResult imported = toImportResult(read);
    EXPECT_EQ(imported.formatId, "fake-lines");
    EXPECT_EQ(imported.project.points.size(), 1U);
    EXPECT_EQ(imported.recordsSkipped, 2U);
    EXPECT_EQ(imported.warnings, (std::vector<std::string>{"a.fak record 7: bad angle", "no header",
                                                           "not in the file: no instrument heights"}));
}
