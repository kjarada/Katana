#include <gtest/gtest.h>

#include <string>
#include <string_view>
#include <vector>

#include "katana/core/text_encoding.hpp"
#include "katana/surveyio/delimited_points.hpp"
#include "katana/surveyio/detect.hpp"

using namespace katana::surveyio;
using katana::core::ErrorCode;
using katana::core::Result;
using katana::survey::LinearUnit;

// Proposing a delimited file's layout, and detecting the format at all.
//
// The property under test throughout is PLAN.MD 45.5: the order of northing and
// easting is decided only by a header that names them. Fixtures keep northing
// near 5 000 000 and easting near 500 000 so that a transposition is visible.

namespace {

LayoutProposal propose(std::string_view text, bool truncated = false)
{
    const Result<LayoutProposal> proposal = proposeLayout(text, truncated);
    EXPECT_TRUE(proposal.ok()) << (proposal.ok() ? "" : proposal.error().describe());
    return proposal.value();
}

std::vector<ColumnRole> roles(std::string_view letters)
{
    std::vector<ColumnRole> columns;
    for (const char letter : letters) {
        switch (letter) {
        case 'P':
            columns.push_back(ColumnRole::PointId);
            break;
        case 'N':
            columns.push_back(ColumnRole::Northing);
            break;
        case 'E':
            columns.push_back(ColumnRole::Easting);
            break;
        case 'Z':
            columns.push_back(ColumnRole::Elevation);
            break;
        case 'C':
            columns.push_back(ColumnRole::Code);
            break;
        case 'D':
            columns.push_back(ColumnRole::Description);
            break;
        default:
            columns.push_back(ColumnRole::Ignore);
            break;
        }
    }
    return columns;
}

bool contains(std::string_view haystack, std::string_view needle)
{
    return haystack.find(needle) != std::string_view::npos;
}

FormatDescriptor specificFormat()
{
    FormatDescriptor format;
    format.id = "vendor-csv";
    format.humanName = "A vendor's CSV export";
    format.manufacturer = Manufacturer::Other;
    format.reads.points = true;
    format.canImport = true;
    format.parserVersion = "1.0";
    return format;
}

// The weakest a specific format can be and still be identified: exactly at the
// bar. If the generic probe cannot come within the margin of THIS, it cannot
// come within the margin of any identified format.
FormatSignature justIdentified(const ProbeInput&)
{
    return {kIdentifiedConfidence, "a signature only that vendor writes"};
}

FormatRegistry delimitedOnly()
{
    FormatRegistry registry;
    EXPECT_TRUE(registry.add(delimitedPointsFormat(), &probeDelimitedPoints).ok());
    return registry;
}

} // namespace

// ---- A header decides the order ------------------------------------------------------

TEST(DelimitedLayoutProposal, AHeaderNamingEastingFirstDecidesAnEastingFirstLayout)
{
    const std::string text = "Point,Easting,Northing,Elevation,Code\r\n"
                             "1,500000.5,5000000.25,101.75,KB\r\n"
                             "2,500020.375,5000010.125,99.5,FL\r\n";
    const LayoutProposal proposal = propose(text);
    ASSERT_EQ(proposal.outcome(), LayoutProposalOutcome::Decided) << proposal.summary();
    const Result<DelimitedLayout> layout = proposal.layout();
    ASSERT_TRUE(layout.ok());
    EXPECT_EQ(layout->columns, roles("PENZC"));
    EXPECT_EQ(layout->delimiter, Delimiter::Comma);
    EXPECT_EQ(layout->headerLines, 1u);

    // And reading with it puts each number where the header said.
    DelimitedImportOptions options;
    options.unit = LinearUnit::Metres;
    const Result<ImportResult> result = parseDelimitedPoints(text, *layout, "points.csv", options);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->project.points.front().northing, 5000000.25);
    EXPECT_EQ(result->project.points.front().easting, 500000.5);
    EXPECT_EQ(result->project.points.front().code, "KB");
}

TEST(DelimitedLayoutProposal, AHeaderNamingNorthingFirstDecidesANorthingFirstLayout)
{
    const std::string text = "PT   N             E           Z\n"
                             "1    5000000.25    500000.5    101.75\n"
                             "2    5000010.125   500020.375  99.5\n";
    const LayoutProposal proposal = propose(text);
    ASSERT_EQ(proposal.outcome(), LayoutProposalOutcome::Decided) << proposal.summary();
    EXPECT_EQ(proposal.layout()->columns, roles("PNEZ"));
    EXPECT_EQ(proposal.layout()->delimiter, Delimiter::Whitespace);
    EXPECT_EQ(proposal.layout()->headerLines, 1u);
    EXPECT_TRUE(contains(proposal.summary(), "northing in column 2, easting in column 3"))
        << proposal.summary();
}

TEST(DelimitedLayoutProposal, HeaderWordsAreReadWithoutCaseUnitsOrPunctuation)
{
    const std::string text = "Point No.;NORTHING (m);easting_[m];R.L.;Feature Code;Remarks\n"
                             "1;5000000.25;500000.5;101.75;KB;Kerb\n";
    const LayoutProposal proposal = propose(text);
    ASSERT_EQ(proposal.outcome(), LayoutProposalOutcome::Decided) << proposal.summary();
    EXPECT_EQ(proposal.layout()->columns, roles("PNEZCD"));
    EXPECT_EQ(proposal.layout()->delimiter, Delimiter::Semicolon);
}

TEST(DelimitedLayoutProposal, AnUnknownHeaderWordIgnoresItsColumnAndSaysSo)
{
    const std::string text = "Point,Date,Northing,Easting\n"
                             "1,2026-09-23,5000000.25,500000.5\n";
    const LayoutProposal proposal = propose(text);
    ASSERT_EQ(proposal.outcome(), LayoutProposalOutcome::Decided) << proposal.summary();
    EXPECT_EQ(proposal.layout()->columns, roles("P-NE"));
    EXPECT_TRUE(contains(proposal.summary(), "column 2 ('Date')")) << proposal.summary();
}

// ---- Nothing else decides it ---------------------------------------------------------

TEST(DelimitedLayoutProposal, AFileWithNoHeaderIsUncertainAndOffersBothCoordinateOrders)
{
    const std::string text = "1,5000000.25,500000.5,101.75,Kerb\n"
                             "2,5000010.125,500020.375,99.5,Fence\n";
    const LayoutProposal proposal = propose(text);
    ASSERT_EQ(proposal.outcome(), LayoutProposalOutcome::Uncertain);
    // The only way to a usable layout is to choose one.
    ASSERT_FALSE(proposal.layout().ok());
    EXPECT_EQ(proposal.layout().error().code, ErrorCode::NotFound);

    // Both five-column orders fit exactly and come first; the rest fit only by
    // ignoring columns. Nothing in "5000000.25" says it is a northing, so the
    // two orders are both offered and neither is preferred on the numbers.
    const std::vector<LayoutCandidate>& candidates = proposal.candidates();
    ASSERT_GE(candidates.size(), 2u);
    EXPECT_EQ(candidates[0].layout.columns, roles("PNEZD"));
    EXPECT_EQ(candidates[1].layout.columns, roles("PENZD"));
    for (const LayoutCandidate& candidate : candidates) {
        EXPECT_EQ(candidate.layout.headerLines, 0u);
        EXPECT_EQ(candidate.layout.delimiter, Delimiter::Comma);
        EXPECT_TRUE(contains(candidate.evidence, "no header")) << candidate.evidence;
    }
    EXPECT_TRUE(contains(proposal.summary(), "northing or easting")) << proposal.summary();
}

TEST(DelimitedLayoutProposal, XAndYAreUncertainWithXAsEastingRankedFirst)
{
    const std::string text = "X,Y,Z\n"
                             "500000.5,5000000.25,101.75\n";
    const LayoutProposal proposal = propose(text);
    ASSERT_EQ(proposal.outcome(), LayoutProposalOutcome::Uncertain) << proposal.summary();
    ASSERT_EQ(proposal.candidates().size(), 2u);
    EXPECT_EQ(proposal.candidates()[0].layout.columns, roles("ENZ"));
    EXPECT_EQ(proposal.candidates()[1].layout.columns, roles("NEZ"));
    EXPECT_EQ(proposal.candidates()[0].layout.headerLines, 1u);
    EXPECT_TRUE(contains(proposal.summary(), "Gauss-Kruger")) << proposal.summary();
    EXPECT_FALSE(proposal.layout().ok());
}

TEST(DelimitedLayoutProposal, AHeaderThatDoesNotNameTheCoordinatesDoesNotDecideThem)
{
    const std::string text = "Col1,Col2,Col3,Col4\n"
                             "1,5000000.25,500000.5,101.75\n";
    const LayoutProposal proposal = propose(text);
    ASSERT_EQ(proposal.outcome(), LayoutProposalOutcome::Uncertain);
    ASSERT_GE(proposal.candidates().size(), 2u);
    EXPECT_EQ(proposal.candidates()[0].layout.columns, roles("PNEZ"));
    EXPECT_EQ(proposal.candidates()[1].layout.columns, roles("PENZ"));
    EXPECT_EQ(proposal.candidates()[0].layout.headerLines, 1u); // the header is still skipped

    // One named coordinate is not an order either.
    const LayoutProposal half = propose("Point,Northing,Other\n1,5000000.25,500000.5\n");
    EXPECT_EQ(half.outcome(), LayoutProposalOutcome::Uncertain);
}

TEST(DelimitedLayoutProposal, RowsThatContradictTheHeaderMakeItUncertain)
{
    // Line 3's northing is text, so the header cannot be right for every row
    // (or the file is damaged); either way a person should look.
    const std::string text = "Point,Northing,Easting,Elevation\n"
                             "1,5000000.25,500000.5,101.75\n"
                             "2,abc,500010.5,99.5\n";
    const LayoutProposal proposal = propose(text);
    ASSERT_EQ(proposal.outcome(), LayoutProposalOutcome::Uncertain);
    ASSERT_EQ(proposal.candidates().size(), 1u);
    EXPECT_EQ(proposal.candidates()[0].layout.columns, roles("PNEZ"));
    EXPECT_TRUE(contains(proposal.summary(), "line 3, column 2 (northing)")) << proposal.summary();
}

TEST(DelimitedLayoutProposal, LatitudeAndLongitudeAreRefusedRatherThanReadAsGridCoordinates)
{
    const LayoutProposal proposal =
        propose("Point,Latitude,Longitude,Height\n1,-33.865143,151.2099,58.5\n");
    EXPECT_EQ(proposal.outcome(), LayoutProposalOutcome::Uncertain);
    EXPECT_TRUE(proposal.candidates().empty());
    EXPECT_TRUE(contains(proposal.summary(), "Latitude")) << proposal.summary();
}

// ---- The rest is inferred -----------------------------------------------------------

TEST(DelimitedLayoutProposal, TheDelimiterIsFoundFromTheSample)
{
    struct Case {
        std::string_view text;
        Delimiter expected;
    };
    const std::vector<Case> cases = {
        {"P\tN\tE\n1\t5000000.25\t500000.5\n", Delimiter::Tab},
        {"P;N;E\n1;5000000.25;500000.5\n", Delimiter::Semicolon},
        {"P,N,E\n1,5000000.25,500000.5\n", Delimiter::Comma},
        {"P N E\n1 5000000.25 500000.5\n", Delimiter::Whitespace},
        // Spaces inside a CSV's descriptions do not make it a whitespace file.
        {"P,N,E,D\n1,5000000.25,500000.5,Kerb line\n2,5000000.25,500010.5,Fence line\n",
         Delimiter::Comma},
        // Nor do commas inside a whitespace file's quoted descriptions.
        {"P N E D\n1 5000000.25 500000.5 \"Kerb, north\"\n", Delimiter::Whitespace},
    };
    for (const Case& c : cases) {
        SCOPED_TRACE(c.text);
        const LayoutProposal proposal = propose(c.text);
        ASSERT_EQ(proposal.outcome(), LayoutProposalOutcome::Decided) << proposal.summary();
        EXPECT_EQ(proposal.layout()->delimiter, c.expected);
    }
}

TEST(DelimitedLayoutProposal, DecimalCommasAreReportedRatherThanReadAsTwoFields)
{
    const LayoutProposal proposal = propose("1;5000000,25;500000,5;101,75\n"
                                            "2;5000010,125;500020,375;99,5\n");
    EXPECT_EQ(proposal.outcome(), LayoutProposalOutcome::Uncertain);
    EXPECT_TRUE(proposal.candidates().empty());
    EXPECT_TRUE(contains(proposal.summary(), "decimal commas")) << proposal.summary();
}

TEST(DelimitedLayoutProposal, TitleAndCommentLinesAboveTheHeaderAreSkippedWithIt)
{
    const std::string text = "Job: Site A\r\n"
                             "# exported 2026-09-23\r\n"
                             "\r\n"
                             "Point,Northing,Easting\r\n"
                             "1,5000000.25,500000.5\r\n"
                             "# a note between rows\r\n"
                             "2,5000010.125,500020.375\r\n";
    const LayoutProposal proposal = propose(text);
    ASSERT_EQ(proposal.outcome(), LayoutProposalOutcome::Decided) << proposal.summary();
    EXPECT_EQ(proposal.layout()->headerLines, 4u);
    EXPECT_EQ(proposal.layout()->commentPrefix, "#");

    DelimitedImportOptions options;
    options.unit = LinearUnit::Metres;
    const Result<ImportResult> result =
        parseDelimitedPoints(text, *proposal.layout(), "points.csv", options);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->project.points.size(), 2u);
}

TEST(DelimitedLayoutProposal, AFileWithNoRowsOfNumbersHasNoCandidates)
{
    const LayoutProposal prose = propose("The quick brown fox\njumps over the lazy dog\n");
    EXPECT_EQ(prose.outcome(), LayoutProposalOutcome::Uncertain);
    EXPECT_TRUE(prose.candidates().empty());

    // A row of numbers followed by a line that is not one: no delimiter makes
    // every row a row, and the summary names the line that broke it.
    const LayoutProposal broken =
        propose("1,5000000.25,500000.5\n2,5000000.25,500010.5\nTotal: two points\n");
    EXPECT_EQ(broken.outcome(), LayoutProposalOutcome::Uncertain);
    EXPECT_TRUE(broken.candidates().empty());
    EXPECT_TRUE(contains(broken.summary(), "line 3")) << broken.summary();
}

TEST(DelimitedLayoutProposal, NothingToLookAtIsEmpty)
{
    EXPECT_EQ(propose("").outcome(), LayoutProposalOutcome::Empty);
    EXPECT_EQ(propose("\r\n  \r\n").outcome(), LayoutProposalOutcome::Empty);
    EXPECT_EQ(propose("# only a comment\n").outcome(), LayoutProposalOutcome::Empty);
    EXPECT_FALSE(propose("").layout().ok());
}

TEST(DelimitedLayoutProposal, ATruncatedSampleIsReadUpToItsLastWholeLine)
{
    // UTF-16LE cut at an odd byte, as a 64 KiB detection sample can be. Read as
    // a whole file that is a corrupt file; read as a sample it is the lines
    // before the cut.
    const Result<std::string> encoded = katana::core::encodeUtf16LittleEndian(
        "Point\tNorthing\tEasting\r\n1\t5000000.25\t500000.5\r\n2\t5000010.125\t500020.375\r\n");
    ASSERT_TRUE(encoded.ok());
    const std::string cut = encoded.value().substr(0, encoded.value().size() - 5);
    ASSERT_EQ(cut.size() % 2, 1u);

    EXPECT_FALSE(proposeLayout(cut, false).ok());
    const LayoutProposal proposal = propose(cut, true);
    ASSERT_EQ(proposal.outcome(), LayoutProposalOutcome::Decided) << proposal.summary();
    EXPECT_EQ(proposal.layout()->columns, roles("PNE"));

    // A UTF-8 sample cut inside a character is still UTF-8. Read whole, the
    // lone lead byte at the end would make the sample invalid UTF-8 and send it
    // to the Windows-1252 fallback, and the header word "Höhe" would come out
    // mangled. The columns would match either way, so the check is on the word
    // the summary quotes back.
    const std::string utf8 = "Point,Northing,Easting,Höhe\n1,5000000.25,500000.5,101.75\n2,5\xC3";
    const LayoutProposal cutUtf8 = propose(utf8, true);
    ASSERT_EQ(cutUtf8.outcome(), LayoutProposalOutcome::Decided) << cutUtf8.summary();
    EXPECT_EQ(cutUtf8.layout()->columns, roles("PNE-"));
    EXPECT_TRUE(contains(cutUtf8.summary(), "'Höhe'")) << cutUtf8.summary();
}

TEST(DelimitedLayoutProposal, OnlyTheStartOfALargeFileIsDecoded)
{
    // A file marked UTF-8 with a byte that cannot be UTF-8 far past the first
    // kProbeBytes. Decoded whole it is a corrupt file (the premise, checked
    // first); the proposal decodes only its start and so still proposes - the
    // corrupt byte is the parser's to report, with its line.
    std::string text = "\xEF\xBB\xBF" "Point,Northing,Easting\r\n";
    for (int i = 0; text.size() < 3 * kProbeBytes; ++i) {
        text += "PT" + std::to_string(100000 + i) + ",5000000.25,500000.5\r\n";
    }
    text += "PT-bad,5000000.25,500000.5,\xFF\r\n";
    ASSERT_FALSE(katana::core::decodeText(text).ok());

    const LayoutProposal proposal = propose(text);
    ASSERT_EQ(proposal.outcome(), LayoutProposalOutcome::Decided) << proposal.summary();
    EXPECT_EQ(proposal.layout()->columns, roles("PNE"));
}

TEST(DelimitedLayoutProposal, AnExportedFileProposesTheLayoutItWasWrittenWith)
{
    std::vector<katana::survey::SurveyPoint> points(2);
    points[0].id = "1";
    points[0].northing = 5000000.25;
    points[0].easting = 500000.5;
    points[0].elevation = 101.75;
    points[0].description = "Kerb";
    points[1].id = "2";
    points[1].northing = 5000010.125;
    points[1].easting = 500020.375;
    for (const ColumnPreset preset : allColumnPresets()) {
        SCOPED_TRACE(toString(preset));
        const DelimitedLayout layout = presetLayout(preset, Delimiter::Comma, 1);
        DelimitedExportOptions options;
        options.decimals = 3;
        options.unit = LinearUnit::Metres;
        const Result<std::string> text = writeDelimitedPoints(points, layout, options);
        ASSERT_TRUE(text.ok()) << text.error().describe();
        const LayoutProposal proposal = propose(text.value());
        ASSERT_EQ(proposal.outcome(), LayoutProposalOutcome::Decided) << proposal.summary();
        EXPECT_EQ(proposal.layout().value(), layout);
    }
}

// ---- Detection -----------------------------------------------------------------------

TEST(DelimitedPointsDetection, AHeaderlessNumericFileIsUncertainRatherThanClaimed)
{
    const std::string bytes = "1,5000000.25,500000.5,101.75\n2,5000010.125,500020.375,99.5\n";
    const FormatRegistry registry = delimitedOnly();
    const Detection detection = detectFormat(probeOf(bytes, "points.csv"), registry);
    EXPECT_EQ(detection.outcome(), DetectionOutcome::Uncertain);
    ASSERT_EQ(detection.candidates().size(), 1u);
    EXPECT_EQ(detection.candidates()[0].formatId, kDelimitedPointsFormatId);
    EXPECT_EQ(detection.candidates()[0].confidence, kDelimitedHeaderlessConfidence);
    EXPECT_FALSE(detection.format().ok());
}

TEST(DelimitedPointsDetection, EvenAHeaderNamingTheCoordinatesOnlyMakesItACandidate)
{
    const std::string bytes = "Point,Northing,Easting\n1,5000000.25,500000.5\n";
    const FormatRegistry registry = delimitedOnly();
    const Detection detection = detectFormat(probeOf(bytes, "points.txt"), registry);
    EXPECT_EQ(detection.outcome(), DetectionOutcome::Uncertain);
    ASSERT_EQ(detection.candidates().size(), 1u);
    EXPECT_EQ(detection.candidates()[0].confidence, kDelimitedHeaderConfidence);
    // Stronger evidence than no header, still below the bar.
    EXPECT_GT(kDelimitedHeaderConfidence, kDelimitedHeaderlessConfidence);
    EXPECT_LT(kDelimitedHeaderConfidence, kIdentifiedConfidence);
}

TEST(DelimitedPointsDetection, ASpecificFormatOutranksTheGenericReadingOfTheSameFile)
{
    FormatRegistry registry = delimitedOnly();
    ASSERT_TRUE(registry.add(specificFormat(), &justIdentified).ok());

    // The file this probe likes best - a header naming the coordinates - against
    // a specific format that only just clears the bar: identified, not
    // ambiguous, with the generic reading still listed second.
    const std::string bytes = "Point,Northing,Easting\n1,5000000.25,500000.5\n";
    const Detection detection = detectFormat(probeOf(bytes, "export.csv"), registry);
    ASSERT_EQ(detection.outcome(), DetectionOutcome::Identified) << detection.summary();
    EXPECT_EQ(detection.format()->id, "vendor-csv");
    ASSERT_EQ(detection.candidates().size(), 2u);
    EXPECT_EQ(detection.candidates()[1].formatId, kDelimitedPointsFormatId);
}

TEST(DelimitedPointsDetection, WhatIsNotRowsOfNumbersIsRuledOut)
{
    const std::vector<std::string> notPoints = {
        "The quick brown fox\njumps over the lazy dog\n",
        // A Leica GSI-16 block: numbers inside words, never a field that is one.
        "*110001+0000000000000001 81..00+0000000000500000 82..00+0000000005000000\n",
        "<?xml version=\"1.0\"?>\n<LandXML>\n<CgPoints>\n</CgPoints>\n</LandXML>\n",
        std::string("\x00\x01\x02\x03\xFF\xFE\x00\x13", 8),
    };
    for (const std::string& bytes : notPoints) {
        const FormatSignature signature = probeDelimitedPoints(probeOf(bytes, "job.txt"));
        EXPECT_EQ(signature.confidence, 0.0) << bytes;
    }
}

TEST(DelimitedPointsDetection, TheProcessWideRegistryNeverIdentifiesAPlainCoordinateList)
{
    // Whatever else is linked in, a list of numbers is not claimed by this
    // format, and this format is among the candidates offered for it.
    const std::string bytes = "1 5000000.25 500000.5 101.75\n2 5000010.125 500020.375 99.5\n";
    const Detection detection = detectFormat(probeOf(bytes, "points.txt"));
    bool offered = false;
    for (const FormatCandidate& candidate : detection.candidates()) {
        if (candidate.formatId == kDelimitedPointsFormatId) {
            offered = true;
            EXPECT_LT(candidate.confidence, kIdentifiedConfidence);
        }
    }
    EXPECT_TRUE(offered);
    if (detection.outcome() == DetectionOutcome::Identified) {
        EXPECT_NE(detection.format()->id, kDelimitedPointsFormatId);
    }
}
