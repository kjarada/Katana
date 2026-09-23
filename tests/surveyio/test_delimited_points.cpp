#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <limits>
#include <optional>
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
using katana::survey::SurveyPoint;

// Delimited point files: reading, writing and the layout template.
//
// Every fixture puts the northing near 5 000 000 and the easting near 500 000,
// an order of magnitude apart, so that a transposed reading cannot pass any
// assertion by coincidence (docs/survey.md, "The two ways survey data gets
// silently corrupted"). The values are chosen exact in binary - quarters,
// eighths, sixteenths - so that "the file says 5000000.25" and "the model holds
// 5000000.25" can be compared with ==, and the expected values are the numbers
// written in the fixture, not anything captured from a run.

namespace {

constexpr double kNorthing = 5000000.25;
constexpr double kEasting = 500000.5;
constexpr double kElevation = 101.75;

DelimitedImportOptions inMetres()
{
    DelimitedImportOptions options;
    options.unit = LinearUnit::Metres;
    return options;
}

DelimitedLayout layoutOf(std::string_view text)
{
    const Result<DelimitedLayout> layout = parseLayoutTemplate(text);
    EXPECT_TRUE(layout.ok()) << (layout.ok() ? "" : layout.error().describe());
    return layout.value();
}

Result<ImportResult> readText(std::string_view text, std::string_view layoutTemplateText,
                              std::string_view fileName = "points.csv")
{
    return parseDelimitedPoints(text, layoutOf(layoutTemplateText), fileName, inMetres());
}

bool contains(std::string_view haystack, std::string_view needle)
{
    return haystack.find(needle) != std::string_view::npos;
}

bool anyWarningContains(const ImportResult& result, std::string_view needle)
{
    for (const std::string& warning : result.warnings) {
        if (contains(warning, needle)) {
            return true;
        }
    }
    return false;
}

std::string errorOf(const Result<ImportResult>& result)
{
    return result.ok() ? std::string("(no error)") : result.error().describe();
}

} // namespace

// ---- Presets ---------------------------------------------------------------------

TEST(DelimitedPointsPresets, EveryPresetReadsTheCoordinatesFromTheColumnsItsLettersName)
{
    // One point - id 7, N 5000000.25, E 500000.5, Z 101.75, "Kerb" - written by
    // hand in each preset's column order. A preset that swapped northing and
    // easting would put 500000.5 in the northing and fail here.
    struct Case {
        ColumnPreset preset;
        std::string_view line;
        std::string_view id; // what the point must be called
        bool hasElevation;
        std::string_view description;
    };
    const std::vector<Case> cases = {
        {ColumnPreset::PNEZD, "7,5000000.25,500000.5,101.75,Kerb", "7", true, "Kerb"},
        {ColumnPreset::PENZD, "7,500000.5,5000000.25,101.75,Kerb", "7", true, "Kerb"},
        {ColumnPreset::PNEZ, "7,5000000.25,500000.5,101.75", "7", true, ""},
        {ColumnPreset::PENZ, "7,500000.5,5000000.25,101.75", "7", true, ""},
        // No id column: the point is named by its line, which is line 1.
        {ColumnPreset::NEZ, "5000000.25,500000.5,101.75", "1", true, ""},
        {ColumnPreset::ENZ, "500000.5,5000000.25,101.75", "1", true, ""},
        {ColumnPreset::PNE, "7,5000000.25,500000.5", "7", false, ""},
        {ColumnPreset::PEN, "7,500000.5,5000000.25", "7", false, ""},
    };
    ASSERT_EQ(cases.size(), allColumnPresets().size()); // every preset, no more

    for (const Case& c : cases) {
        SCOPED_TRACE(toString(c.preset));
        const DelimitedLayout layout = presetLayout(c.preset, Delimiter::Comma, 0);
        const Result<ImportResult> result =
            parseDelimitedPoints(c.line, layout, "points.csv", inMetres());
        ASSERT_TRUE(result.ok()) << errorOf(result);
        ASSERT_EQ(result->project.points.size(), 1u);
        const SurveyPoint& point = result->project.points.front();
        EXPECT_EQ(point.id, c.id);
        EXPECT_EQ(point.northing, kNorthing);
        EXPECT_EQ(point.easting, kEasting);
        EXPECT_EQ(point.elevation.has_value(), c.hasElevation);
        if (c.hasElevation) {
            EXPECT_EQ(*point.elevation, kElevation);
        }
        EXPECT_EQ(point.description, c.description);
    }
}

TEST(DelimitedPointsPresets, APresetIsNamedByItsLettersInAnyCaseAndNothingElse)
{
    for (const ColumnPreset preset : allColumnPresets()) {
        const Result<ColumnPreset> named = columnPresetNamed(toString(preset));
        ASSERT_TRUE(named.ok());
        EXPECT_EQ(named.value(), preset);
    }
    EXPECT_EQ(columnPresetNamed("penzd").value(), ColumnPreset::PENZD);
    EXPECT_EQ(columnPresetNamed("PNEZC").error().code, ErrorCode::NotFound);
    EXPECT_EQ(columnPresetNamed("").error().code, ErrorCode::NotFound);

    const std::vector<ColumnRole> pnezd = presetColumns(ColumnPreset::PNEZD);
    const std::vector<ColumnRole> expected = {ColumnRole::PointId, ColumnRole::Northing,
                                              ColumnRole::Easting, ColumnRole::Elevation,
                                              ColumnRole::Description};
    EXPECT_EQ(pnezd, expected);
}

// ---- Layout validation and templates ------------------------------------------------

TEST(DelimitedPointsLayout, ALayoutWithoutBothHorizontalCoordinatesOrWithARoleTwiceIsRefused)
{
    DelimitedLayout layout;
    layout.columns = {ColumnRole::PointId, ColumnRole::Northing, ColumnRole::Elevation};
    EXPECT_EQ(validateLayout(layout).error().code, ErrorCode::InvalidArgument);

    layout.columns = {ColumnRole::Easting, ColumnRole::Elevation};
    EXPECT_FALSE(validateLayout(layout).ok());

    layout.columns = {ColumnRole::Northing, ColumnRole::Easting, ColumnRole::Easting};
    const katana::core::Status twice = validateLayout(layout);
    ASSERT_FALSE(twice.ok());
    EXPECT_TRUE(contains(twice.error().message, "two easting columns (2 and 3)"));

    // Ignore may repeat: it is not a role, it is the absence of one.
    layout.columns = {ColumnRole::Ignore, ColumnRole::Northing, ColumnRole::Ignore,
                      ColumnRole::Easting};
    EXPECT_TRUE(validateLayout(layout).ok());

    layout.columns.clear();
    EXPECT_FALSE(validateLayout(layout).ok());
}

TEST(DelimitedPointsLayout, ACommentPrefixThatCouldSwallowALineOfDataIsRefused)
{
    DelimitedLayout layout = presetLayout(ColumnPreset::PNEZ, Delimiter::Comma, 0);
    // Each of these can begin a line of data, which would then vanish as a
    // comment: an id "12", a coordinate "-3.5", "+7", ".5".
    for (const std::string prefix : {"1", "-", "+", ".", "9x"}) {
        layout.commentPrefix = prefix;
        EXPECT_FALSE(validateLayout(layout).ok()) << prefix;
    }
    // A blank is trimmed away before the prefix is looked for; a quote opens a
    // field; ';' ends a template part; the delimiter makes an empty first field
    // a comment.
    for (const std::string prefix : {"# x", "\"", ";", ",", "\t#"}) {
        layout.commentPrefix = prefix;
        EXPECT_FALSE(validateLayout(layout).ok()) << prefix;
    }
    for (const std::string prefix : {"#", "//", "REM", "!"}) {
        layout.commentPrefix = prefix;
        EXPECT_TRUE(validateLayout(layout).ok()) << prefix;
    }
}

TEST(DelimitedPointsTemplate, TheBriefsExampleTemplateReadsAsTheLayoutItSpells)
{
    const Result<DelimitedLayout> layout = parseLayoutTemplate("P,N,E,Z,D;delimiter=comma;header=1");
    ASSERT_TRUE(layout.ok()) << layout.error().describe();
    EXPECT_EQ(layout->columns, presetColumns(ColumnPreset::PNEZD));
    EXPECT_EQ(layout->delimiter, Delimiter::Comma);
    EXPECT_EQ(layout->headerLines, 1u);
    // The two optional keys, absent, mean what the header documents.
    EXPECT_EQ(layout->quoting, Quoting::DoubleQuote);
    EXPECT_EQ(layout->commentPrefix, "");

    // Case and blanks round the parts are not meaning.
    const Result<DelimitedLayout> loose =
        parseLayoutTemplate(" p , n , e , z , d ; Delimiter = COMMA ; header = 1 ");
    ASSERT_TRUE(loose.ok()) << loose.error().describe();
    EXPECT_EQ(loose.value(), layout.value());
}

TEST(DelimitedPointsTemplate, ALayoutSurvivesTheTripThroughItsTemplateText)
{
    std::vector<DelimitedLayout> layouts;
    layouts.push_back(presetLayout(ColumnPreset::PNEZD, Delimiter::Comma, 1));
    DelimitedLayout aligned = presetLayout(ColumnPreset::ENZ, Delimiter::Whitespace, 0);
    aligned.quoting = Quoting::None;
    aligned.commentPrefix = "#";
    layouts.push_back(aligned);
    DelimitedLayout wide;
    wide.columns = {ColumnRole::PointId,   ColumnRole::Ignore,    ColumnRole::Northing,
                    ColumnRole::Easting,   ColumnRole::Elevation, ColumnRole::Code,
                    ColumnRole::Ignore,    ColumnRole::Description};
    wide.delimiter = Delimiter::Tab;
    wide.headerLines = 3;
    wide.commentPrefix = "//";
    layouts.push_back(wide);
    layouts.push_back(presetLayout(ColumnPreset::PEN, Delimiter::Semicolon, 12));

    for (const DelimitedLayout& layout : layouts) {
        const std::string text = layoutTemplate(layout);
        SCOPED_TRACE(text);
        const Result<DelimitedLayout> back = parseLayoutTemplate(text);
        ASSERT_TRUE(back.ok()) << back.error().describe();
        EXPECT_EQ(back.value(), layout);
    }
    // The canonical text states every key, so a saved template never depends on
    // what "absent" meant in the version of the program that saved it.
    EXPECT_EQ(layoutTemplate(wide),
              "P,-,N,E,Z,C,-,D;delimiter=tab;header=3;quote=double;comment=//");
    EXPECT_EQ(layoutTemplate(aligned), "E,N,Z;delimiter=whitespace;header=0;quote=none;comment=#");
}

TEST(DelimitedPointsTemplate, AMalformedTemplateIsRejectedRatherThanReadAsSomethingClose)
{
    const std::vector<std::string_view> malformed = {
        "",                                                  // nothing at all
        "P,N,E,Z",                                           // no delimiter, no header count
        "P,N,E,Z;delimiter=comma",                           // no header count
        "P,N,E,Z;header=1",                                  // no delimiter
        "PNEZ;delimiter=comma;header=1",                     // a preset name is not a column list
        "P,N,Q,Z;delimiter=comma;header=1",                  // no such column letter
        "P,N,X,Z;delimiter=comma;header=1",                  // X is not "ignore"
        "P,N,,Z;delimiter=comma;header=1",                   // an empty column
        "P,N,E;delimiter=pipe;header=1",                     // no such delimiter
        "P,N,E;delimiter=space;header=1",                    // not "whitespace" by another name
        "P,N,E;delimiter=comma;header=-1",                   // a negative count
        "P,N,E;delimiter=comma;header=1.5",                  // not a count
        "P,N,E;delimiter=comma;header=one",                  // not a count
        "P,N,E;delimiter=comma;header=",                     // no value
        "P,N,E;delimiter=comma;header=99999999999999999999", // beyond any count
        "P,N,E;delimiter=comma;header=1;header=2",           // which one?
        "P,N,E;delimiter=comma;delimiter=comma;header=1",    // even when they agree
        "P,N,E;delimiter=comma;header=1;quote=single",       // no such quoting
        "P,N,E;delimiter=comma;header=1;decimal=comma",      // no such key
        "P,N,E;delimiter=comma;header=1;;",                  // an empty part
        "P,N,E;delimiter=comma;header=1;",                   // an empty last part
        "P,N,E;delimiter comma;header=1",                    // not key=value
        "P,N,E;delimiter=comma;header=1;comment=1",          // a prefix that begins a number
        "P,N,E;delimiter=comma;header=1;comment=,",          // the delimiter as a prefix
        "P,N,Z;delimiter=comma;header=1",                    // no easting column
        "P,N,E,N;delimiter=comma;header=1",                  // two northings
    };
    for (const std::string_view text : malformed) {
        const Result<DelimitedLayout> layout = parseLayoutTemplate(text);
        EXPECT_FALSE(layout.ok()) << "'" << text << "' was read as "
                                  << (layout.ok() ? layoutTemplate(layout.value()) : "");
        if (!layout.ok()) {
            EXPECT_EQ(layout.error().code, ErrorCode::InvalidArgument) << text;
        }
    }
}

// ---- Import ----------------------------------------------------------------------

TEST(DelimitedPointsImport, APointFileReadsIntoTheModelWithItsProvenance)
{
    const std::string text = "Point,Northing,Easting,Elevation,Description\r\n"
                             "1,5000000.25,500000.5,101.75,Kerb\r\n"
                             "2,5000010.125,500020.375,99.5,Fence\r\n";
    const std::string templateText = "P,N,E,Z,D;delimiter=comma;header=1";
    // A path a file (or a careless caller) supplied: only the name may survive.
    const Result<ImportResult> result = readText(text, templateText, "..\\..\\jobs\\site survey.csv");
    ASSERT_TRUE(result.ok()) << errorOf(result);

    EXPECT_EQ(result->formatId, "delimited-points");
    EXPECT_EQ(result->recordsRead, 2u);
    EXPECT_EQ(result->recordsSkipped, 0u);
    EXPECT_TRUE(result->warnings.empty()) << result->warnings.front();

    const katana::survey::SurveyProject& project = result->project;
    ASSERT_EQ(project.points.size(), 2u);
    const SurveyPoint& second = project.points[1];
    EXPECT_EQ(second.id, "2");
    EXPECT_EQ(second.northing, 5000010.125);
    EXPECT_EQ(second.easting, 500020.375);
    ASSERT_TRUE(second.elevation.has_value());
    EXPECT_EQ(*second.elevation, 99.5);
    EXPECT_EQ(second.description, "Fence");
    EXPECT_EQ(second.code, "");
    // A plain coordinate list does not say how its coordinates came to be.
    EXPECT_EQ(second.coordinateSource, katana::survey::CoordinateSource::Unknown);

    // Provenance: the file NAME, the line, and the layout it was read with -
    // enough to repeat the import exactly.
    EXPECT_EQ(second.source.fileName, "site survey.csv");
    EXPECT_EQ(second.source.recordNumber, 3u); // header on line 1, point 2 on line 3
    EXPECT_EQ(second.source.format, "Delimited text points (CSV, TXT)");
    EXPECT_EQ(second.source.formatVersion, layoutTemplate(layoutOf(templateText)));
    EXPECT_EQ(second.source.manufacturer, "Generic");
    EXPECT_EQ(project.source.fileName, "site survey.csv");
    EXPECT_EQ(project.source.recordNumber, 0u);

    // Transforms nothing, declares nothing it was not told (PLAN.MD 45.2).
    EXPECT_TRUE(project.coordinateSystem.unknown);
    EXPECT_EQ(project.coordinateSystem.epsgCode, 0);
    EXPECT_EQ(project.units.linear, LinearUnit::Metres);
    EXPECT_EQ(project.units.angular, katana::survey::AngularUnit::Unknown);
    EXPECT_TRUE(katana::survey::validateProject(project).ok());
}

TEST(DelimitedPointsImport, AnEmptyElevationIsAbsentAndNeverZero)
{
    // Line 2's elevation field is empty; line 3 ends before it; line 4 is a
    // real height of zero, which must stay a height.
    const std::string text = "1,5000000.25,500000.5,,Kerb\n"
                             "2,5000000.25,500010.5,  ,Kerb\n"
                             "3,5000000.25,500020.5\n"
                             "4,5000000.25,500030.5,0,Datum\n";
    const Result<ImportResult> result = readText(text, "P,N,E,Z,D;delimiter=comma;header=0");
    ASSERT_TRUE(result.ok()) << errorOf(result);
    const std::vector<SurveyPoint>& points = result->project.points;
    ASSERT_EQ(points.size(), 4u);
    EXPECT_FALSE(points[0].elevation.has_value());
    EXPECT_FALSE(points[1].elevation.has_value());
    EXPECT_FALSE(points[2].elevation.has_value());
    ASSERT_TRUE(points[3].elevation.has_value());
    EXPECT_EQ(*points[3].elevation, 0.0);

    EXPECT_TRUE(anyWarningContains(*result, "3 of 4 point(s) have no elevation"));
    EXPECT_TRUE(anyWarningContains(*result, "first is on line 1"));
    EXPECT_TRUE(anyWarningContains(*result, "1 line(s) end before the layout's last column"));

    // In a whitespace file an empty field has to be written "" to exist at all.
    const Result<ImportResult> aligned =
        readText("1 5000000.25 500000.5 \"\" Kerb\n", "P,N,E,Z,D;delimiter=whitespace;header=0");
    ASSERT_TRUE(aligned.ok()) << errorOf(aligned);
    EXPECT_FALSE(aligned->project.points.front().elevation.has_value());
    EXPECT_EQ(aligned->project.points.front().description, "Kerb");
}

TEST(DelimitedPointsImport, QuotedFieldsMayHoldTheDelimiterDoubledQuotesAndLineBreaks)
{
    const std::string text = "1,5000000.25,500000.5,101.75,\"Kerb, north side\"\r\n"
                             "2,5000000.25,500000.5,101.75,\"6\"\" PVC\"\r\n"
                             "3,5000000.25,500000.5,101.75,\"two\r\nlines\"\r\n"
                             "4,5000000.25,500000.5,101.75,6\" PVC\r\n"
                             "\"5\" , 5000000.25,500000.5,101.75,\"  padded  \"\r\n";
    const Result<ImportResult> result = readText(text, "P,N,E,Z,D;delimiter=comma;header=0");
    ASSERT_TRUE(result.ok()) << errorOf(result);
    const std::vector<SurveyPoint>& points = result->project.points;
    ASSERT_EQ(points.size(), 5u);
    EXPECT_EQ(points[0].description, "Kerb, north side");
    EXPECT_EQ(points[1].description, "6\" PVC"); // RFC 4180: "" inside quotes is one quote
    EXPECT_EQ(points[2].description, "two\r\nlines");
    // A quote that does not open a field is a character (the header says why).
    EXPECT_EQ(points[3].description, "6\" PVC");
    // Blanks round a quoted field are padding; blanks inside it are content.
    EXPECT_EQ(points[4].id, "5");
    EXPECT_EQ(points[4].description, "  padded  ");

    // The record that spans lines 3 and 4 is numbered by the line it starts
    // on, and the next record is on line 5, not 4.
    EXPECT_EQ(points[2].source.recordNumber, 3u);
    EXPECT_EQ(points[3].source.recordNumber, 5u);
    EXPECT_EQ(points[4].source.recordNumber, 6u);

    // With quoting off a quote is just a character, even at the start.
    const Result<ImportResult> plain =
        readText("\"7\",5000000.25,500000.5\n", "P,N,E;delimiter=comma;header=0;quote=none");
    ASSERT_TRUE(plain.ok()) << errorOf(plain);
    EXPECT_EQ(plain->project.points.front().id, "\"7\"");
}

TEST(DelimitedPointsImport, AByteOrderMarkIsNotPartOfTheFirstPointId)
{
    const std::string text = "\xEF\xBB\xBF" "1,5000000.25,500000.5\r\n";
    const Result<ImportResult> result = readText(text, "P,N,E;delimiter=comma;header=0");
    ASSERT_TRUE(result.ok()) << errorOf(result);
    EXPECT_EQ(result->project.points.front().id, "1");
    EXPECT_EQ(result->project.metadata.at("text encoding"),
              katana::core::toString(katana::core::TextEncoding::Utf8WithBom));
}

TEST(DelimitedPointsImport, CrLfLfAndLoneCrEachEndALine)
{
    const std::string text = "1,5000000.25,500000.5\r\n"
                             "2,5000000.25,500010.5\n"
                             "3,5000000.25,500020.5\r"
                             "4,5000000.25,500030.5";
    const Result<ImportResult> result = readText(text, "P,N,E;delimiter=comma;header=0");
    ASSERT_TRUE(result.ok()) << errorOf(result);
    ASSERT_EQ(result->project.points.size(), 4u);
    for (std::size_t i = 0; i < 4; ++i) {
        EXPECT_EQ(result->project.points[i].source.recordNumber, i + 1);
        EXPECT_EQ(result->project.points[i].easting, 500000.5 + 10.0 * static_cast<double>(i));
    }
}

TEST(DelimitedPointsImport, Utf8NamesCodesAndDescriptionsArriveUnchanged)
{
    const std::string text = "Pünkt-7,5000000.25,500000.5,101.75,Bäume,東京 station\n";
    const Result<ImportResult> result = readText(text, "P,N,E,Z,C,D;delimiter=comma;header=0");
    ASSERT_TRUE(result.ok()) << errorOf(result);
    const SurveyPoint& point = result->project.points.front();
    EXPECT_EQ(point.id, "Pünkt-7");
    EXPECT_EQ(point.code, "Bäume");
    EXPECT_EQ(point.description, "東京 station");
    EXPECT_TRUE(result->warnings.empty()); // UTF-8 validates, so nothing was guessed
}

TEST(DelimitedPointsImport, AUtf16SpreadsheetExportIsReadThroughTheSharedDecoder)
{
    // Excel's "Unicode text" is UTF-16LE with a mark, tab-delimited.
    const Result<std::string> bytes = katana::core::encodeUtf16LittleEndian(
        "Point\tNorthing\tEasting\r\nPünkt\t5000000.25\t500000.5\r\n");
    ASSERT_TRUE(bytes.ok());
    const Result<ImportResult> result = readText(bytes.value(), "P,N,E;delimiter=tab;header=1");
    ASSERT_TRUE(result.ok()) << errorOf(result);
    ASSERT_EQ(result->project.points.size(), 1u);
    EXPECT_EQ(result->project.points.front().id, "Pünkt");
    EXPECT_EQ(result->project.points.front().northing, kNorthing);
}

TEST(DelimitedPointsImport, AnInferredWindows1252EncodingIsWarnedAboutAndAStatedOneIsNot)
{
    // 0xE9 alone is not UTF-8; in Windows-1252 it is U+00E9, "é".
    const std::string text = "1,5000000.25,500000.5,,Caf\xE9\n";
    const Result<ImportResult> guessed = readText(text, "P,N,E,Z,D;delimiter=comma;header=0");
    ASSERT_TRUE(guessed.ok()) << errorOf(guessed);
    EXPECT_EQ(guessed->project.points.front().description, "Café");
    EXPECT_TRUE(anyWarningContains(*guessed, "inferred as Windows-1252"));

    DelimitedImportOptions stated = inMetres();
    stated.encoding = katana::core::TextEncoding::Windows1252;
    const Result<ImportResult> told = parseDelimitedPoints(
        text, layoutOf("P,N,E,Z,D;delimiter=comma;header=0"), "points.csv", stated);
    ASSERT_TRUE(told.ok()) << errorOf(told);
    EXPECT_EQ(told->project.points.front().description, "Café");
    EXPECT_FALSE(anyWarningContains(*told, "inferred"));
}

TEST(DelimitedPointsImport, FeetAreConvertedToMetresExactlyAndTheUnitIsRecorded)
{
    // International foot = 381/1250 m exactly (math/unit_ratio.hpp). Worked:
    //   1000 ft * 381 / 1250 = 381000 / 1250 = 304.8 m
    //   2000 ft * 381 / 1250 = 762000 / 1250 = 609.6 m
    //     10 ft * 381 / 1250 =   3810 / 1250 = 3.048 m
    // Each is one correctly rounded division of exact integers, so it is the
    // double nearest the true value - the same double the literal denotes.
    DelimitedImportOptions options;
    options.unit = LinearUnit::Feet;
    const Result<ImportResult> result = parseDelimitedPoints(
        "1,1000,2000,10\n", layoutOf("P,N,E,Z;delimiter=comma;header=0"), "feet.csv", options);
    ASSERT_TRUE(result.ok()) << errorOf(result);
    const SurveyPoint& point = result->project.points.front();
    EXPECT_EQ(point.northing, 304.8);
    EXPECT_EQ(point.easting, 609.6);
    EXPECT_EQ(*point.elevation, 3.048);
    EXPECT_EQ(result->project.units.linear, LinearUnit::Feet);
}

TEST(DelimitedPointsImport, AnUnstatedUnitIsRefusedBecauseNoneIsAssumed)
{
    const Result<ImportResult> result =
        parseDelimitedPoints("1,5000000.25,500000.5\n", layoutOf("P,N,E;delimiter=comma;header=0"),
                             "points.csv", DelimitedImportOptions{});
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, ErrorCode::InvalidArgument);
}

TEST(DelimitedPointsImport, BlankAndCommentLinesAreSkippedAndCommentsCounted)
{
    const std::string text = "# exported from the field\n"
                             "\n"
                             "1,5000000.25,500000.5\n"
                             "   \t \n"
                             "  # a comment may be indented\n"
                             "2,5000000.25,500010.5\n";
    const Result<ImportResult> result = readText(text, "P,N,E;delimiter=comma;header=0;comment=#");
    ASSERT_TRUE(result.ok()) << errorOf(result);
    ASSERT_EQ(result->project.points.size(), 2u);
    EXPECT_EQ(result->project.points[0].source.recordNumber, 3u);
    EXPECT_EQ(result->project.points[1].source.recordNumber, 6u);
    EXPECT_TRUE(anyWarningContains(*result, "2 comment line(s) starting '#' were skipped"));
}

TEST(DelimitedPointsImport, EveryDelimiterSplitsItsOwnFile)
{
    struct Case {
        std::string_view templateText;
        std::string_view text;
    };
    const std::vector<Case> cases = {
        {"P,N,E,Z,D;delimiter=tab;header=0", "1\t5000000.25\t500000.5\t101.75\tKerb, north\n"},
        {"P,N,E,Z,D;delimiter=semicolon;header=0", "1;5000000.25;500000.5;101.75;Kerb, north\n"},
        {"P,N,E,Z,D;delimiter=comma;header=0", "1, 5000000.25 ,500000.5,101.75,Kerb north\n"},
        // A column-aligned listing: runs of blanks, blanks at both ends.
        {"P,N,E,Z,D;delimiter=whitespace;header=0",
         "   1    5000000.25\t 500000.5    101.75   \"Kerb, north\"   \n"},
    };
    for (const Case& c : cases) {
        SCOPED_TRACE(c.templateText);
        const Result<ImportResult> result = readText(c.text, c.templateText);
        ASSERT_TRUE(result.ok()) << errorOf(result);
        const SurveyPoint& point = result->project.points.front();
        EXPECT_EQ(point.id, "1");
        EXPECT_EQ(point.northing, kNorthing);
        EXPECT_EQ(point.easting, kEasting);
        EXPECT_EQ(*point.elevation, kElevation);
        EXPECT_TRUE(point.description == "Kerb, north" || point.description == "Kerb north")
            << point.description;
    }
}

TEST(DelimitedPointsImport, WithoutAnIdColumnEachPointIsNamedByItsLineAndAWarningSaysSo)
{
    const std::string text = "Northing,Easting,Elevation\n"
                             "5000000.25,500000.5,101.75\n"
                             "\n"
                             "5000010.125,500020.375,99.5\n";
    const Result<ImportResult> result = readText(text, "N,E,Z;delimiter=comma;header=1");
    ASSERT_TRUE(result.ok()) << errorOf(result);
    ASSERT_EQ(result->project.points.size(), 2u);
    EXPECT_EQ(result->project.points[0].id, "2");
    EXPECT_EQ(result->project.points[1].id, "4"); // line 3 is blank
    EXPECT_TRUE(anyWarningContains(*result, "named by the number of the line it is on"));
}

TEST(DelimitedPointsImport, AnIgnoredColumnIsReadPastAndTrailingEmptyFieldsAreNotData)
{
    const std::string text = "1,2026-09-23,5000000.25,500000.5,,,\n";
    const Result<ImportResult> result = readText(text, "P,-,N,E;delimiter=comma;header=0");
    ASSERT_TRUE(result.ok()) << errorOf(result);
    EXPECT_EQ(result->project.points.front().northing, kNorthing);
    EXPECT_EQ(result->project.points.front().easting, kEasting);
}

TEST(DelimitedPointsImport, AFileOfOnlyAHeaderIsAnEmptyImportAndSaysSo)
{
    const Result<ImportResult> result =
        readText("Point,Northing,Easting\r\n", "P,N,E;delimiter=comma;header=1");
    ASSERT_TRUE(result.ok()) << errorOf(result);
    EXPECT_TRUE(result->project.points.empty());
    EXPECT_EQ(result->recordsRead, 0u);
    EXPECT_TRUE(anyWarningContains(*result, "no points"));

    const Result<ImportResult> nothing = readText("", "P,N,E;delimiter=comma;header=0");
    ASSERT_TRUE(nothing.ok()) << errorOf(nothing);
    EXPECT_TRUE(nothing->project.points.empty());
}

// ---- Import errors: each names its line and column, and nothing is imported ------

TEST(DelimitedPointsImportErrors, AMalformedNumberNamesItsLineAndColumn)
{
    // Line 3's easting has letter Os for zeros.
    const std::string text = "Point,Northing,Easting\n"
                             "1,5000000.25,500000.5\n"
                             "2,5000000.25,5OOOOO.5\n";
    const Result<ImportResult> result = readText(text, "P,N,E;delimiter=comma;header=1");
    ASSERT_FALSE(result.ok()); // and so no partial import either
    EXPECT_EQ(result.error().code, ErrorCode::ParseFailure);
    EXPECT_TRUE(contains(result.error().message, "line 3, column 3 (easting)"))
        << result.error().message;
    EXPECT_TRUE(contains(result.error().message, "'5OOOOO.5' is not a number"));
    EXPECT_EQ(result.error().context, "points.csv");
}

TEST(DelimitedPointsImportErrors, NumbersThatAreNotPlainDecimalsAreRefusedNotRead)
{
    // A decimal comma, trailing text, a non-finite value, hexadecimal, two signs:
    // each could be read as SOME number, and each would be a guess.
    for (const std::string_view bad : {"5000000,25", "5000000.25m", "inf", "nan", "0x4C4B40",
                                       "+-5000000", "1e999"}) {
        const std::string text = "1;" + std::string(bad) + ";500000.5\n";
        const Result<ImportResult> result = readText(text, "P,N,E;delimiter=semicolon;header=0");
        ASSERT_FALSE(result.ok()) << bad;
        EXPECT_TRUE(contains(result.error().message, "line 1, column 2 (northing)"))
            << result.error().message;
    }
}

TEST(DelimitedPointsImportErrors, AnEmptyNorthingOrEastingIsAnError)
{
    const Result<ImportResult> northing =
        readText("1,,500000.5\n", "P,N,E;delimiter=comma;header=0");
    ASSERT_FALSE(northing.ok());
    EXPECT_TRUE(contains(northing.error().message, "line 1, column 2 (northing): empty"))
        << northing.error().message;

    const Result<ImportResult> easting =
        readText("1,5000000.25, \n", "P,N,E;delimiter=comma;header=0");
    ASSERT_FALSE(easting.ok());
    EXPECT_TRUE(contains(easting.error().message, "line 1, column 3 (easting): empty"))
        << easting.error().message;
}

TEST(DelimitedPointsImportErrors, ALineTooShortToReachARequiredColumnIsAnError)
{
    const Result<ImportResult> result =
        readText("1,5000000.25,500000.5,101.75\n2,5000000.25\n", "P,N,E,Z;delimiter=comma;header=0");
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, ErrorCode::ParseFailure);
    EXPECT_TRUE(contains(result.error().message, "line 2, column 3 (easting): missing"))
        << result.error().message;
}

TEST(DelimitedPointsImportErrors, TextPastTheLastColumnIsAnErrorRatherThanACutDescription)
{
    // An unquoted comma in the description splits it into a sixth field; taking
    // the first five would import "Kerb" and lose " north side" silently.
    const Result<ImportResult> result = readText("1,5000000.25,500000.5,101.75,Kerb, north side\n",
                                                 "P,N,E,Z,D;delimiter=comma;header=0");
    ASSERT_FALSE(result.ok());
    EXPECT_TRUE(contains(result.error().message, "line 1: 6 fields where the layout has 5"))
        << result.error().message;
    EXPECT_TRUE(contains(result.error().message, "column 6 holds 'north side'"));
}

TEST(DelimitedPointsImportErrors, AnEmptyPointIdIsAnErrorAndNoIdIsInvented)
{
    const Result<ImportResult> result =
        readText("1,5000000.25,500000.5\n\"\",5000000.25,500010.5\n",
                 "P,N,E;delimiter=comma;header=0");
    ASSERT_FALSE(result.ok());
    EXPECT_TRUE(contains(result.error().message, "line 2, column 1 (point id): empty"))
        << result.error().message;
}

TEST(DelimitedPointsImportErrors, ARepeatedPointIdIsAnErrorNamingBothLines)
{
    const std::string text = "Point,Northing,Easting\n"
                             "A1,5000000.25,500000.5\n"
                             "A2,5000000.25,500010.5\n"
                             "A1,5000000.25,500020.5\n";
    const Result<ImportResult> result = readText(text, "P,N,E;delimiter=comma;header=1");
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, ErrorCode::AlreadyExists);
    EXPECT_TRUE(contains(result.error().message, "line 4, column 1 (point id)"))
        << result.error().message;
    EXPECT_TRUE(contains(result.error().message, "the point on line 2"));
}

TEST(DelimitedPointsImportErrors, AQuoteThatNeverClosesNamesTheLineItOpensOn)
{
    const Result<ImportResult> result =
        readText("1,5000000.25,500000.5,\"Kerb\n2,5000000.25,500010.5,Fence\n",
                 "P,N,E,D;delimiter=comma;header=0");
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, ErrorCode::ParseFailure);
    EXPECT_TRUE(contains(result.error().message, "line 1, column 4"))
        << result.error().message;
    EXPECT_TRUE(contains(result.error().message, "never closed"));
}

TEST(DelimitedPointsImportErrors, TextAfterAClosingQuoteIsAnError)
{
    const Result<ImportResult> result =
        readText("1,5000000.25,500000.5,\"Kerb\" north\n", "P,N,E,D;delimiter=comma;header=0");
    ASSERT_FALSE(result.ok());
    EXPECT_TRUE(contains(result.error().message, "line 1, column 4"))
        << result.error().message;
    EXPECT_TRUE(contains(result.error().message, "follows the closing quote"));
}

TEST(DelimitedPointsImportErrors, AnUnusableLayoutIsRefusedBeforeAnythingIsRead)
{
    DelimitedLayout layout;
    layout.columns = {ColumnRole::PointId, ColumnRole::Northing};
    const Result<ImportResult> result =
        parseDelimitedPoints("1,5000000.25\n", layout, "points.csv", inMetres());
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, ErrorCode::InvalidArgument);
}

TEST(DelimitedPointsImportErrors, BytesThatClaimAnEncodingTheyAreNotInAreAnError)
{
    // A UTF-8 byte order mark followed by a byte that cannot be UTF-8: a
    // truncated or corrupt file, not something to decode to anything plausible.
    const Result<ImportResult> result =
        readText("\xEF\xBB\xBF" "1,5000000.25,500000.5,,Caf\xE9\n", "P,N,E,Z,D;delimiter=comma;header=0");
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, ErrorCode::ParseFailure);
}

// ---- Export ----------------------------------------------------------------------

namespace {

SurveyPoint pointOf(std::string id, double northing, double easting,
                    std::optional<double> elevation, std::string code = {},
                    std::string description = {})
{
    SurveyPoint point;
    point.id = std::move(id);
    point.northing = northing;
    point.easting = easting;
    point.elevation = elevation;
    point.code = std::move(code);
    point.description = std::move(description);
    return point;
}

DelimitedExportOptions exportOptions(int decimals, LinearUnit unit = LinearUnit::Metres)
{
    DelimitedExportOptions options;
    options.decimals = decimals;
    options.unit = unit;
    return options;
}

} // namespace

TEST(DelimitedPointsExport, FixedDecimalsAndAnAbsentElevationWrittenAsAnEmptyField)
{
    const std::vector<SurveyPoint> points = {
        pointOf("1", 5000000.25, 500000.5, 101.75, "KB", "Kerb"),
        pointOf("2", 5000010.125, 500020.375, std::nullopt),
    };
    const Result<std::string> text = writeDelimitedPoints(
        points, presetLayout(ColumnPreset::PNEZD, Delimiter::Comma, 1), exportOptions(3));
    ASSERT_TRUE(text.ok()) << text.error().describe();
    // Worked by hand: every value is exact in binary, so three places are just
    // its digits padded with zeros; point 2's elevation is absent - empty,
    // never 0.000.
    EXPECT_EQ(text.value(), "Point,Northing,Easting,Elevation,Description\r\n"
                            "1,5000000.250,500000.500,101.750,Kerb\r\n"
                            "2,5000010.125,500020.375,,\r\n");

    // Whitespace cannot hold an empty field unquoted.
    const Result<std::string> aligned = writeDelimitedPoints(
        points, presetLayout(ColumnPreset::PENZ, Delimiter::Whitespace, 0), exportOptions(2));
    ASSERT_TRUE(aligned.ok()) << aligned.error().describe();
    // Two places of 500020.375 and 5000010.125: both are exact in binary, so
    // both are exact ties, and to_chars rounds a tie as printf does under IEEE
    // 754 - to the even last digit: .375 -> .38, .125 -> .12.
    EXPECT_EQ(aligned.value(), "1 500000.50 5000000.25 101.75\r\n"
                               "2 500020.38 5000010.12 \"\"\r\n");
}

TEST(DelimitedPointsExport, AValueThatRoundsToZeroIsWrittenWithoutASign)
{
    const std::vector<SurveyPoint> points = {pointOf("1", -0.0004, 0.0, -0.0)};
    const Result<std::string> text = writeDelimitedPoints(
        points, presetLayout(ColumnPreset::PNEZ, Delimiter::Comma, 0), exportOptions(3));
    ASSERT_TRUE(text.ok()) << text.error().describe();
    EXPECT_EQ(text.value(), "1,0.000,0.000,0.000\r\n");
}

TEST(DelimitedPointsExport, FeetAreWrittenFromMetresExactly)
{
    // metres * 1250 / 381 (math::fromMetres with the international foot).
    // The double 304.8 is 304.80000000000001137; times 1250 that is
    // 381000.0000000000142, and half an ulp at 381000 is 2^-35 = 2.9e-11, so the
    // product rounds to 381000 exactly and 381000 / 381 = 1000. Likewise 609.6
    // is 609.60000000000002274; times 1250, 762000.0000000000284, inside half an
    // ulp (2^-34 = 5.8e-11) of 762000, which / 381 = 2000. So the file says
    // 1000.000 and 2000.000, not 999.999...
    const std::vector<SurveyPoint> points = {pointOf("1", 304.8, 609.6, std::nullopt)};
    const Result<std::string> text =
        writeDelimitedPoints(points, presetLayout(ColumnPreset::PNE, Delimiter::Comma, 0),
                             exportOptions(3, LinearUnit::Feet));
    ASSERT_TRUE(text.ok()) << text.error().describe();
    EXPECT_EQ(text.value(), "1,1000.000,2000.000\r\n");
}

TEST(DelimitedPointsExport, FieldsThatWouldBeMisreadAreQuoted)
{
    DelimitedLayout layout;
    layout.columns = {ColumnRole::PointId, ColumnRole::Northing, ColumnRole::Easting,
                      ColumnRole::Description};
    layout.commentPrefix = "#";
    const std::vector<SurveyPoint> points = {
        pointOf("#1", 1.0, 2.0, std::nullopt, {}, "Kerb, north"),
        pointOf("2", 1.0, 2.0, std::nullopt, {}, "6\" PVC"),
        pointOf("3", 1.0, 2.0, std::nullopt, {}, " padded"),
        pointOf("4", 1.0, 2.0, std::nullopt, {}, "two\nlines"),
    };
    const Result<std::string> text = writeDelimitedPoints(points, layout, exportOptions(0));
    ASSERT_TRUE(text.ok()) << text.error().describe();
    EXPECT_EQ(text.value(), "\"#1\",1,2,\"Kerb, north\"\r\n"
                            "2,1,2,\"6\"\" PVC\"\r\n"
                            "3,1,2,\" padded\"\r\n"
                            "4,1,2,\"two\nlines\"\r\n");
}

TEST(DelimitedPointsExport, WhatCannotBeWrittenIsRefusedNamingThePoint)
{
    const std::vector<SurveyPoint> points = {pointOf("1", kNorthing, kEasting, kElevation)};
    const DelimitedLayout pnez = presetLayout(ColumnPreset::PNEZ, Delimiter::Comma, 0);

    // Unset decimals and unit: the surveyor's statement, not this writer's.
    EXPECT_EQ(writeDelimitedPoints(points, pnez, DelimitedExportOptions{}).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_FALSE(writeDelimitedPoints(points, pnez, exportOptions(-1)).ok());
    EXPECT_FALSE(writeDelimitedPoints(points, pnez, exportOptions(kMaximumExportDecimals + 1)).ok());
    EXPECT_TRUE(writeDelimitedPoints(points, pnez, exportOptions(kMaximumExportDecimals)).ok());
    EXPECT_FALSE(
        writeDelimitedPoints(points, pnez, exportOptions(3, LinearUnit::Unknown)).ok());

    // Two header lines: this writer has one header line to write.
    EXPECT_FALSE(writeDelimitedPoints(points, presetLayout(ColumnPreset::PNEZ, Delimiter::Comma, 2),
                                      exportOptions(3))
                     .ok());

    // Ids the reader would refuse.
    const std::vector<SurveyPoint> repeated = {pointOf("A", 1, 2, 3), pointOf("A", 4, 5, 6)};
    const Result<std::string> twice = writeDelimitedPoints(repeated, pnez, exportOptions(3));
    ASSERT_FALSE(twice.ok());
    EXPECT_TRUE(contains(twice.error().message, "'A' is used twice (points 1 and 2)"));
    const std::vector<SurveyPoint> unnamed = {pointOf("", 1, 2, 3)};
    EXPECT_FALSE(writeDelimitedPoints(unnamed, pnez, exportOptions(3)).ok());
    // ...unless the layout writes no id, when neither matters.
    EXPECT_TRUE(writeDelimitedPoints(repeated, presetLayout(ColumnPreset::NEZ, Delimiter::Comma, 0),
                                     exportOptions(3))
                    .ok());

    const std::vector<SurveyPoint> infinite = {
        pointOf("1", std::numeric_limits<double>::infinity(), 2, 3)};
    const Result<std::string> notFinite = writeDelimitedPoints(infinite, pnez, exportOptions(3));
    ASSERT_FALSE(notFinite.ok());
    EXPECT_TRUE(contains(notFinite.error().message, "point '1': its northing"));

    // With quoting off, a field that needs quotes cannot be written at all.
    DelimitedLayout plain = presetLayout(ColumnPreset::PNEZD, Delimiter::Comma, 0);
    plain.quoting = Quoting::None;
    const std::vector<SurveyPoint> comma = {pointOf("1", 1, 2, 3, {}, "Kerb, north")};
    const Result<std::string> unquotable = writeDelimitedPoints(comma, plain, exportOptions(3));
    ASSERT_FALSE(unquotable.ok());
    EXPECT_TRUE(contains(unquotable.error().message, "point '1': its description"))
        << unquotable.error().message;
    DelimitedLayout alignedPlain = presetLayout(ColumnPreset::PNEZ, Delimiter::Whitespace, 0);
    alignedPlain.quoting = Quoting::None;
    const std::vector<SurveyPoint> noHeight = {pointOf("1", 1, 2, std::nullopt)};
    EXPECT_FALSE(writeDelimitedPoints(noHeight, alignedPlain, exportOptions(3)).ok());
}

TEST(DelimitedPointsExport, WhatExportWritesImportReadsBackUnchanged)
{
    // Every awkward value: a comment prefix at the start of an id, blanks inside
    // and round an id, a comma, a quote and a line break in a description, a
    // semicolon in a code, UTF-8, an absent elevation and a real zero one.
    // Coordinates are exact at four places (sixteenths), so rounding to four
    // places changes nothing and the comparison can be exact.
    const std::vector<SurveyPoint> points = {
        pointOf("1", 5000000.25, 500000.5, 101.75, "KB", "Kerb, north side"),
        pointOf("#2", 5000010.125, 500020.375, std::nullopt, "", "6\" PVC"),
        pointOf("Pünkt 3", 5000020.5, 500040.0625, -0.5, "Bäume", "line one\nline two"),
        pointOf(" padded ", 5000030.0, 500060.0, 0.0, "; semi", ""),
    };
    const std::vector<std::string_view> templates = {
        "P,N,E,Z,D;delimiter=comma;header=1",
        "P,E,N,Z,C,D;delimiter=whitespace;header=0",
        "P,N,E,Z,C,D;delimiter=tab;header=1;comment=#",
        "P,-,N,E,Z,C,D;delimiter=semicolon;header=0",
    };
    for (const std::string_view templateText : templates) {
        SCOPED_TRACE(templateText);
        const DelimitedLayout layout = layoutOf(templateText);
        const Result<std::string> text = writeDelimitedPoints(points, layout, exportOptions(4));
        ASSERT_TRUE(text.ok()) << text.error().describe();
        const Result<ImportResult> back =
            parseDelimitedPoints(text.value(), layout, "export.csv", inMetres());
        ASSERT_TRUE(back.ok()) << errorOf(back) << "\n" << text.value();
        ASSERT_EQ(back->project.points.size(), points.size());
        const bool codes = std::find(layout.columns.begin(), layout.columns.end(),
                                     ColumnRole::Code) != layout.columns.end();
        for (std::size_t i = 0; i < points.size(); ++i) {
            const SurveyPoint& original = points[i];
            const SurveyPoint& read = back->project.points[i];
            EXPECT_EQ(read.id, original.id);
            EXPECT_EQ(read.northing, original.northing);
            EXPECT_EQ(read.easting, original.easting);
            EXPECT_EQ(read.elevation, original.elevation);
            EXPECT_EQ(read.description, original.description);
            EXPECT_EQ(read.code, codes ? original.code : std::string{});
        }
    }
}

// ---- The format in the registry -----------------------------------------------------

TEST(DelimitedPointsFormat, TheFormatIsRegisteredAndStatesThatItCarriesPointsOnly)
{
    // Registered from its own translation unit and present in the process-wide
    // registry - which also proves the archive is linked whole (see
    // src/katana_surveyio/CMakeLists.txt), since nothing references the object.
    const Result<FormatDescriptor> format = formatRegistry().find(kDelimitedPointsFormatId);
    ASSERT_TRUE(format.ok());
    EXPECT_EQ(format.value(), delimitedPointsFormat());

    FormatContent pointsOnly;
    pointsOnly.points = true;
    EXPECT_EQ(format->reads, pointsOnly);
    EXPECT_TRUE(format->canImport);
    EXPECT_TRUE(format->canExport);
    EXPECT_EQ(format->manufacturer, Manufacturer::Generic);
    EXPECT_EQ(describeFormat(format.value()),
              "Delimited text points (CSV, TXT), import: yes, export: yes, parser: 1.0");
}
