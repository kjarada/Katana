#include <gtest/gtest.h>

#include <string>

#include "katana/archive12d/reader.hpp"

namespace a12 = katana::archive12d;
using katana::core::ErrorCode;

namespace {

a12::Archive read(const std::string& text)
{
    auto archive = a12::readArchive(text);
    EXPECT_TRUE(archive.ok()) << (archive.ok() ? "" : archive.error().describe());
    return archive.ok() ? std::move(*archive) : a12::Archive{};
}

template <typename T> const T& only(const a12::Archive& archive)
{
    EXPECT_EQ(archive.elements.size(), 1u);
    return std::get<T>(archive.elements.at(0));
}

} // namespace

// ---- numbers ---------------------------------------------------------------

TEST(ParseReal, ReadsDecimalsExponentsAndSigns)
{
    EXPECT_EQ(a12::parseReal("502000.5"), 502000.5);
    EXPECT_EQ(a12::parseReal("-0.25"), -0.25);
    EXPECT_EQ(a12::parseReal("+8"), 8.0);
    EXPECT_EQ(a12::parseReal("1.5e3"), 1500.0);
    EXPECT_EQ(a12::parseReal(".5"), 0.5);
}

TEST(ParseReal, ReadsTheHexadecimalFloatsTwelveDWritesItsTinsIn)
{
    // C99 hexadecimal floating constants: mantissa in hex, exponent a power of
    // two. 0x1.8p+1 = 1.5 * 2 = 3; 0x1p-4 = 1/16. Both from the C standard's
    // own definition, and both exact in binary.
    EXPECT_EQ(a12::parseReal("0x1.8p+1"), 3.0);
    EXPECT_EQ(a12::parseReal("-0x1.8p+1"), -3.0);
    EXPECT_EQ(a12::parseReal("0x1p-4"), 0.0625);
    EXPECT_EQ(a12::parseReal("0x0.0p+0"), 0.0);
    // A value from a real full_tin: 0x1.6d4014ac08318p+17. Its leading digits
    // are 1.6d4 hex = 1 + 6/16 + 13/256 + 4/4096 = 1.4267578..., times 2^17 =
    // 131072, so it lies between 187008 and 187009.
    const auto easting = a12::parseReal("0x1.6d4014ac08318p+17");
    ASSERT_TRUE(easting.has_value());
    EXPECT_GT(*easting, 187008.0);
    EXPECT_LT(*easting, 187009.0);
}

TEST(ParseReal, RefusesWhatIsNotAFiniteNumber)
{
    EXPECT_FALSE(a12::parseReal("").has_value());
    EXPECT_FALSE(a12::parseReal("null").has_value());
    EXPECT_FALSE(a12::parseReal("inf").has_value());
    EXPECT_FALSE(a12::parseReal("nan").has_value());
    EXPECT_FALSE(a12::parseReal("12abc").has_value());
    EXPECT_FALSE(a12::parseReal("0x").has_value());
    EXPECT_FALSE(a12::parseReal("1e999").has_value()); // overflows a double
    EXPECT_FALSE(a12::parseReal("-").has_value());
}

TEST(ParseReal, ToleratesTheCommaOfAHandMadeRow)
{
    EXPECT_EQ(a12::parseReal("502000.5,"), 502000.5);
}

// ---- syntax (manual 1.1) ---------------------------------------------------

TEST(ReaderSyntax, CommentsBlankLinesAndLayoutDoNotMatter)
{
    const auto spread = read("string super {\n"
                             "  // a comment\n"
                             "  name \"A\"   // trailing comment\n\n"
                             "  data_3d {\n    1 2 3\n    4 5 6\n  }\n}\n");
    const auto packed = read("string super{name \"A\" data_3d{1 2 3 4 5 6}}");
    const auto& a = only<a12::VertexString>(spread);
    const auto& b = only<a12::VertexString>(packed);
    EXPECT_EQ(a.header.name, "A");
    ASSERT_EQ(a.vertices.size(), 2u);
    EXPECT_EQ(a.vertices, b.vertices);
    EXPECT_EQ(a.vertices[1], (a12::Vertex{4.0, 5.0, 6.0}));
}

TEST(ReaderSyntax, AKeywordAndItsValueMayBeOnDifferentLines)
{
    const auto archive = read("string super {\n name\n \"Kerb\"\n chainage\n 12.5\n data_2d { 0 0 }\n}");
    const auto& string = only<a12::VertexString>(archive);
    EXPECT_EQ(string.header.name, "Kerb");
    EXPECT_EQ(string.header.chainage, 12.5);
}

TEST(ReaderSyntax, KeywordsAreCaseInsensitive)
{
    const auto archive = read("MODEL \"M\" STRING Super { NAME \"A\" Data_3D { 1 2 3 } }");
    const auto& string = only<a12::VertexString>(archive);
    EXPECT_EQ(string.header.model, "M");
    EXPECT_EQ(string.header.name, "A");
    EXPECT_EQ(string.vertices.size(), 1u);
}

TEST(ReaderSyntax, QuotedTextKeepsItsSpacesCommentMarkersAndEscapes)
{
    // Manual 1.1: inside a text, \" is a double quote and \\ a backslash.
    const auto archive = read(R"(string super { name "a // b  \"c\" \\ d" data_2d { 0 0 } })");
    EXPECT_EQ(only<a12::VertexString>(archive).header.name, "a // b  \"c\" \\ d");
}

TEST(ReaderSyntax, ABackslashBeforeAnythingElseIsItself)
{
    const auto archive = read(R"(string super { name "C:\Temp\job" data_2d { 0 0 } })");
    EXPECT_EQ(only<a12::VertexString>(archive).header.name, R"(C:\Temp\job)");
}

TEST(ReaderSyntax, ATextNeverClosedIsAnErrorNamingItsLine)
{
    const auto archive = a12::readArchive("model \"A\"\nstring super {\n name \"unclosed\n}");
    ASSERT_FALSE(archive.ok());
    EXPECT_EQ(archive.error().code, ErrorCode::ParseFailure);
    EXPECT_NE(archive.error().context.find("line 3"), std::string::npos)
        << archive.error().describe();
}

TEST(ReaderSyntax, ABraceNeverClosedIsAnError)
{
    const auto archive = a12::readArchive("string super { name \"A\" data_3d { 1 2 3 }");
    ASSERT_FALSE(archive.ok());
    EXPECT_EQ(archive.error().code, ErrorCode::ParseFailure);
}

TEST(ReaderSyntax, ABraceThatClosesNothingIsAnError)
{
    const auto archive = a12::readArchive("model \"A\"\n}\n");
    ASSERT_FALSE(archive.ok());
    EXPECT_NE(archive.error().context.find("line 2"), std::string::npos);
}

TEST(ReaderSyntax, NestingBeyondTheLimitIsRefusedRatherThanOverflowingTheStack)
{
    std::string text = "string super { attributes ";
    for (int i = 0; i < 200; ++i) {
        text += "{ group ";
    }
    const auto archive = a12::readArchive(text);
    ASSERT_FALSE(archive.ok());
    EXPECT_NE(archive.error().message.find("nested"), std::string::npos);
}

TEST(ReaderSyntax, TextThatIsNotUtf8IsRefusedBeforeItCanReachTheModel)
{
    const auto archive = a12::readArchive("string super { name \"\xFF\" data_2d { 0 0 } }");
    ASSERT_FALSE(archive.ok());
    EXPECT_EQ(archive.error().code, ErrorCode::ParseFailure);
}

TEST(ReaderSyntax, TheEmptyFileIsAnEmptyArchive)
{
    const auto archive = read("");
    EXPECT_TRUE(archive.elements.empty());
    EXPECT_TRUE(archive.modelNames.empty());
    const auto commentsOnly = read("// nothing here\n\n// at all\n");
    EXPECT_TRUE(commentsOnly.elements.empty());
}

TEST(ReaderSyntax, ReadsUtf16AsTwelveDModelWritesIt)
{
    const std::string text = "model \"M\"\nstring super { name \"A\" data_3d { 1 2 3 } }\n";
    std::string bytes("\xFF\xFE", 2);
    for (const char ch : text) {
        bytes += ch;
        bytes += '\0';
    }
    const auto archive = a12::readArchiveBytes(bytes);
    ASSERT_TRUE(archive.ok()) << archive.error().describe();
    ASSERT_EQ(archive->elements.size(), 1u);
    EXPECT_EQ(std::get<a12::VertexString>(archive->elements[0]).header.name, "A");
    EXPECT_TRUE(archive->warnings.empty()) << "a marked file is not a guess";
}

// ---- commands (manual 1.4) ---------------------------------------------------

TEST(ReaderCommands, TheDefaultsAreTheOnesTheManualGives)
{
    // 1.4.1 "data", 1.4.2 "red", 1.4.4 "point", 1.4.5 -999.
    const auto archive = read("string super { data_3d { 1 2 -999 } }");
    const auto& string = only<a12::VertexString>(archive);
    EXPECT_EQ(string.header.model, "data");
    EXPECT_EQ(string.header.colour, "red");
    EXPECT_EQ(string.header.breakline, a12::Breakline::Point);
    EXPECT_FALSE(string.vertices.at(0).z.has_value()) << "-999 is the default null";
    EXPECT_EQ(archive.nullValue, -999.0);
}

TEST(ReaderCommands, ACommandHoldsUntilTheNextAndAStringMayOverrideIt)
{
    const auto archive = read("model \"A\" colour blue style \"Kerb\" breakline line\n"
                              "string super { data_2d { 0 0 } }\n"
                              "string super { model \"B\" colour green data_2d { 0 0 } }\n"
                              "string super { data_2d { 0 0 } }\n");
    ASSERT_EQ(archive.elements.size(), 3u);
    const auto& first = std::get<a12::VertexString>(archive.elements[0]);
    const auto& second = std::get<a12::VertexString>(archive.elements[1]);
    const auto& third = std::get<a12::VertexString>(archive.elements[2]);
    EXPECT_EQ(first.header.model, "A");
    EXPECT_EQ(first.header.colour, "blue");
    EXPECT_EQ(first.header.style, "Kerb");
    EXPECT_EQ(first.header.breakline, a12::Breakline::Line);
    EXPECT_EQ(second.header.model, "B");
    EXPECT_EQ(second.header.colour, "green");
    // The override was for that string only (manual 1.4.6).
    EXPECT_EQ(third.header.model, "A");
    EXPECT_EQ(third.header.colour, "blue");
    EXPECT_EQ(archive.modelNames, (std::vector<std::string>{"A", "B"}));
}

TEST(ReaderCommands, TheNullCommandChangesWhichHeightMeansNone)
{
    const auto archive = read("null -1\nstring super { data_3d { 0 0 -1  1 1 -999  2 2 null } }");
    const auto& string = only<a12::VertexString>(archive);
    ASSERT_EQ(string.vertices.size(), 3u);
    EXPECT_FALSE(string.vertices[0].z.has_value());
    EXPECT_EQ(string.vertices[1].z, -999.0) << "-999 is an ordinary height once null is -1";
    EXPECT_FALSE(string.vertices[2].z.has_value()) << "the null keyword is always null";
}

TEST(ReaderCommands, NullNullAsTwelveDModelWritesItLeavesTheValueAlone)
{
    const auto archive = read("null null\nstring super { data_3d { 0 0 -999 } }");
    EXPECT_EQ(archive.nullValue, -999.0);
    EXPECT_FALSE(only<a12::VertexString>(archive).vertices[0].z.has_value());
}

TEST(ReaderCommands, AStringMayCarryANullValueOfItsOwn)
{
    const auto archive = read("string super { null_value -5 data_3d { 0 0 -5 } }\n"
                              "string super { data_3d { 0 0 -5 } }");
    ASSERT_EQ(archive.elements.size(), 2u);
    EXPECT_FALSE(std::get<a12::VertexString>(archive.elements[0]).vertices[0].z.has_value());
    EXPECT_EQ(std::get<a12::VertexString>(archive.elements[1]).vertices[0].z, -5.0);
}

TEST(ReaderCommands, AModelDeclaredWithABlockCarriesAttributesAndBecomesCurrent)
{
    const auto archive = read("model { name \"telegraph poles\" attributes { integer \"wires\" 3 } }\n"
                              "string super { data_2d { 0 0 } }");
    ASSERT_EQ(archive.models.size(), 1u);
    EXPECT_EQ(archive.models[0].name, "telegraph poles");
    ASSERT_EQ(archive.models[0].attributes.size(), 1u);
    EXPECT_EQ(std::get<std::int64_t>(archive.models[0].attributes[0].value), 3);
    EXPECT_EQ(only<a12::VertexString>(archive).header.model, "telegraph poles");
}

TEST(ReaderCommands, ModelNamesThatDifferOnlyInCaseAreOneModel)
{
    // Manual 1.1: "FRED" is considered the same model name as "Fred".
    const auto archive = read("model \"Fred\" string super { data_2d { 0 0 } }\n"
                              "model \"FRED\" string super { data_2d { 0 0 } }");
    EXPECT_EQ(archive.modelNames, (std::vector<std::string>{"Fred"}));
}

TEST(ReaderCommands, ATwoWordColourIsReadWhetherOrNotItIsQuoted)
{
    const auto archive = read("string super { colour dark blue style \"WS\" data_2d { 0 0 } }\n"
                              "string super { colour \"light grey\" data_2d { 0 0 } }\n"
                              "string super { colour dark name \"N\" data_2d { 0 0 } }");
    ASSERT_EQ(archive.elements.size(), 3u);
    const auto& bare = std::get<a12::VertexString>(archive.elements[0]);
    EXPECT_EQ(bare.header.colour, "dark blue");
    EXPECT_EQ(bare.header.style, "WS");
    EXPECT_EQ(std::get<a12::VertexString>(archive.elements[1]).header.colour, "light grey");
    // `dark` followed by a keyword is a colour called dark, not "dark name".
    const auto& guarded = std::get<a12::VertexString>(archive.elements[2]);
    EXPECT_EQ(guarded.header.colour, "dark");
    EXPECT_EQ(guarded.header.name, "N");
}

TEST(ReaderCommands, TheExportSettingsTwelveDWritesAsCommentsAreKept)
{
    const auto archive = read("// archive_version \"15.01.08.60\"\n// decimal_places 8\n"
                              "// output_hex_floats false\n// just a remark 3\n"
                              "string super { data_2d { 0 0 } }\n// decimal_places 2\n");
    ASSERT_EQ(archive.headerSettings.size(), 3u);
    EXPECT_EQ(archive.headerSettings[0].key, "archive_version");
    EXPECT_EQ(archive.headerSettings[0].value, "15.01.08.60");
    EXPECT_EQ(archive.headerSettings[1].value, "8") << "settings after the first element are not the header";
}

// ---- attributes (manual 1.3) -------------------------------------------------

TEST(ReaderAttributes, EachTypeIsReadAsItsOwnType)
{
    const auto archive = read("string super { attributes {\n"
                              "  text \"pole id\" \"QMR-37\"\n  text street \"477 Boundary St\"\n"
                              "  real \"pole height\" 5.25\n  integer \"pole wires\" 3\n"
                              "} data_2d { 0 0 } }");
    const auto& attributes = only<a12::VertexString>(archive).header.attributes;
    ASSERT_EQ(attributes.size(), 4u);
    EXPECT_EQ(attributes[0].name, "pole id");
    EXPECT_EQ(std::get<std::string>(attributes[0].value), "QMR-37");
    EXPECT_EQ(attributes[1].name, "street");
    EXPECT_EQ(std::get<double>(attributes[2].value), 5.25);
    EXPECT_EQ(std::get<std::int64_t>(attributes[3].value), 3);
}

TEST(ReaderAttributes, GroupsNest)
{
    const auto archive = read("string super { attributes { group { name \"Asset\" attributes {\n"
                              "  text \"Owner\" \"Telstra\"\n"
                              "  group { name \"Size\" attributes { real \"Diameter\" 0.1 } }\n"
                              "} } text \"Date\" \"2024\" } data_2d { 0 0 } }");
    const auto& attributes = only<a12::VertexString>(archive).header.attributes;
    ASSERT_EQ(attributes.size(), 2u);
    EXPECT_EQ(attributes[0].name, "Asset");
    const auto& asset = std::get<a12::AttributeList>(attributes[0].value);
    ASSERT_EQ(asset.size(), 2u);
    const auto& size = std::get<a12::AttributeList>(asset[1].value);
    ASSERT_EQ(size.size(), 1u);
    EXPECT_EQ(std::get<double>(size[0].value), 0.1);
    EXPECT_EQ(attributes[1].name, "Date");
}

TEST(ReaderAttributes, ATypeTheManualDoesNotListIsKeptUnderItsOwnName)
{
    // Real exports carry `uid "this id" 5292`.
    const auto archive = read("string super { attributes { uid \"this id\" 5292 } data_2d { 0 0 } }");
    const auto& attribute = only<a12::VertexString>(archive).header.attributes.at(0);
    EXPECT_EQ(attribute.declaredType, "uid");
    EXPECT_EQ(std::get<std::int64_t>(attribute.value), 5292);
}

TEST(ReaderAttributes, AnAttributeWithANullValueIsCountedNotInvented)
{
    const auto archive = read("string super { attributes { real \"depth\" null integer \"n\" 2 }"
                              " data_2d { 0 0 } }");
    const auto& attributes = only<a12::VertexString>(archive).header.attributes;
    ASSERT_EQ(attributes.size(), 1u);
    EXPECT_EQ(attributes[0].name, "n");
    ASSERT_FALSE(archive.warnings.empty());
    EXPECT_NE(archive.warnings.back().find("1 attributes"), std::string::npos);
}

// ---- super strings (manual 1.5.8) ----------------------------------------------

TEST(ReaderSuperString, ATwoDimensionalStringTakesItsOneHeightFromZ)
{
    const auto archive = read("string super { z 31.25 closed true data_2d { 0 0  10 0  10 5 } }");
    const auto& string = only<a12::VertexString>(archive);
    EXPECT_TRUE(string.closed);
    EXPECT_EQ(string.constantZ, 31.25);
    ASSERT_EQ(string.vertices.size(), 3u);
    EXPECT_FALSE(string.vertices[0].z.has_value());
    EXPECT_EQ(string.segmentCount(), 3u) << "a closed string of n vertices has n segments";
}

TEST(ReaderSuperString, ClosedAcceptsEverySpellingTheManualLists)
{
    // 1.5.8: true is 1, T, t, Y, y or a word starting so; false 0, F, f, N, n.
    for (const char* yes : {"1", "T", "t", "Y", "y", "true", "Yes"}) {
        const auto archive = read(std::string("string super { closed ") + yes + " data_2d { 0 0 } }");
        EXPECT_TRUE(only<a12::VertexString>(archive).closed) << yes;
    }
    for (const char* no : {"0", "F", "f", "N", "n", "false", "No"}) {
        const auto archive = read(std::string("string super { closed ") + no + " data_2d { 0 0 } }");
        EXPECT_FALSE(only<a12::VertexString>(archive).closed) << no;
    }
}

TEST(ReaderSuperString, RadiusAndMajorDataBecomeArcSegments)
{
    const auto archive = read("string super { data_2d { 0 0  2 0  4 0  6 0 }\n"
                              "radius_data { 0 -5 7.5 } major_data { 0 1 f } }");
    const auto& string = only<a12::VertexString>(archive);
    ASSERT_EQ(string.segments.size(), 3u);
    EXPECT_EQ(string.segments[0].kind, a12::SegmentKind::Straight);
    EXPECT_EQ(string.segments[1].kind, a12::SegmentKind::Arc);
    EXPECT_EQ(string.segments[1].radius, -5.0);
    EXPECT_TRUE(string.segments[1].major);
    EXPECT_EQ(string.segments[2].radius, 7.5);
    EXPECT_FALSE(string.segments[2].major);
}

TEST(ReaderSuperString, RadiiThatAreAllZeroMeanThereIsNoCurvedSegment)
{
    const auto archive = read("string super { data_2d { 0 0 2 0 4 0 } radius_data { 0 0 } major_data { 0 0 } }");
    EXPECT_TRUE(only<a12::VertexString>(archive).segments.empty());
}

TEST(ReaderSuperString, GeometryDataReadsEveryKindOfSegment)
{
    const auto archive = read("string super { data_2d { 0 0  1 0  2 0  3 0  4 0 } geometry_data {\n"
                              " straight { }\n arc { radius 80 major 1 }\n"
                              " spiral { type \"cubic parabola\" leading 0 l1 0 r1 0 a1 200.5 l2 80 r2 299 a2 192 }\n"
                              " curve { type clothoid leading 1 xorigin 1 yorigin 2 radius 50 length 30"
                              " start 5 end 25 angle 10 offset 1.5 mvalue 0 }\n} }");
    const auto& segments = only<a12::VertexString>(archive).segments;
    ASSERT_EQ(segments.size(), 4u);
    EXPECT_EQ(segments[0].kind, a12::SegmentKind::Straight);
    EXPECT_EQ(segments[1].kind, a12::SegmentKind::Arc);
    EXPECT_EQ(segments[1].radius, 80.0);
    EXPECT_TRUE(segments[1].major);
    EXPECT_EQ(segments[2].kind, a12::SegmentKind::Spiral);
    EXPECT_EQ(segments[2].parameters.text("type"), "cubic parabola");
    EXPECT_EQ(segments[2].parameters.boolean("leading"), false);
    EXPECT_EQ(segments[2].parameters.real("a1"), 200.5);
    EXPECT_EQ(segments[3].kind, a12::SegmentKind::Curve);
    EXPECT_EQ(segments[3].parameters.real("offset"), 1.5);
}

TEST(ReaderSuperString, ASegmentKindNobodyKnowsKeepsItsPlaceAsAStraight)
{
    // Dropping it would move every later segment onto the wrong vertices.
    const auto archive = read("string super { data_2d { 0 0 1 0 2 0 3 0 } geometry_data {\n"
                              " straight { } wiggle { amplitude 3 } arc { radius 9 major 0 } } }");
    const auto& segments = only<a12::VertexString>(archive).segments;
    ASSERT_EQ(segments.size(), 3u);
    EXPECT_EQ(segments[1].kind, a12::SegmentKind::Straight);
    EXPECT_EQ(segments[2].radius, 9.0);
    ASSERT_FALSE(archive.warnings.empty());
    EXPECT_NE(archive.warnings[0].find("wiggle"), std::string::npos);
}

TEST(ReaderSuperString, TheSupersededDataBlockCarriesRadiusAndBulgeOnEachVertex)
{
    const auto archive = read("string super { data { 0 0 1 12 0   20 0 1 0 0   40 10 1 -8 1 } }");
    const auto& string = only<a12::VertexString>(archive);
    ASSERT_EQ(string.vertices.size(), 3u);
    EXPECT_EQ(string.vertices[2], (a12::Vertex{40.0, 10.0, 1.0}));
    ASSERT_EQ(string.segments.size(), 3u);
    EXPECT_EQ(string.segments[0].radius, 12.0);
    EXPECT_EQ(string.segments[1].kind, a12::SegmentKind::Straight);
    EXPECT_EQ(string.segments[2].radius, -8.0);
    EXPECT_TRUE(string.segments[2].major);
}

TEST(ReaderSuperString, EveryBlockOfExtraInformationIsRead)
{
    const auto archive = read(R"(string super {
  data_3d { 0 0 1  10 0 2  10 10 3 }
  interval { chord_arc 0.01 distance 5 }
  colour_data { red "dark green" }
  point_data { 101 "A 7" 103 }
  diameter_data { 0.3 0.45 }
  justify invert
  vertex_tinable_data { 1 0 t }   segment_tinable_data { 1 0 }
  vertex_visible_data { 1 1 0 }   segment_visible_data { 0 1 }
  vertex_text_data { "a" "" "c" }
  vertex_annotate_value { angle 45 offset 1 raise 0.5 textstyle "Arial" worldsize 2 justify "top-left" colour blue }
  segment_text_value "seg"
  segment_annotate_data { properties { papersize 3 } properties { papersize 4 } }
  symbol_data { properties { style "Tree" size 2 rotation 0 } properties { } properties { style "Post" } }
  vertex_attribute_data { attributes { integer "n" 1 } attributes { } attributes { text "t" "x" } }
  segment_attribute_data { attributes { real "grade" 0.02 } attributes { } }
})");
    const auto& s = only<a12::VertexString>(archive);
    EXPECT_EQ(s.interval->real("chord_arc"), 0.01);
    EXPECT_EQ(s.segmentColours, (std::vector<std::string>{"red", "dark green"}));
    EXPECT_EQ(s.pointIds, (std::vector<std::string>{"101", "A 7", "103"}));
    EXPECT_EQ(s.diameters, (std::vector<double>{0.3, 0.45}));
    EXPECT_EQ(s.justify, "invert");
    EXPECT_EQ(s.vertexTinable, (std::vector<bool>{true, false, true}));
    EXPECT_EQ(s.segmentTinable, (std::vector<bool>{true, false}));
    EXPECT_EQ(s.vertexVisible, (std::vector<bool>{true, true, false}));
    EXPECT_EQ(s.segmentVisible, (std::vector<bool>{false, true}));
    EXPECT_EQ(s.vertexText, (std::vector<std::string>{"a", "", "c"}));
    ASSERT_TRUE(s.vertexAnnotation.has_value());
    EXPECT_EQ(s.vertexAnnotation->real("angle"), 45.0);
    EXPECT_EQ(s.vertexAnnotation->text("justify"), "top-left");
    EXPECT_EQ(s.vertexAnnotation->text("colour"), "blue");
    EXPECT_EQ(s.segmentTextValue, "seg");
    ASSERT_EQ(s.segmentAnnotations.size(), 2u);
    EXPECT_EQ(s.segmentAnnotations[1].real("papersize"), 4.0);
    ASSERT_EQ(s.symbols.size(), 3u);
    EXPECT_EQ(s.symbols[0].text("style"), "Tree");
    EXPECT_TRUE(s.symbols[1].empty());
    ASSERT_EQ(s.vertexAttributes.size(), 3u);
    EXPECT_TRUE(s.vertexAttributes[1].empty());
    EXPECT_EQ(std::get<std::string>(s.vertexAttributes[2].at(0).value), "x");
    ASSERT_EQ(s.segmentAttributes.size(), 2u);
    EXPECT_EQ(std::get<double>(s.segmentAttributes[0].at(0).value), 0.02);
    EXPECT_TRUE(archive.warnings.empty()) << archive.warnings.front();
    EXPECT_TRUE(archive.unrecognised.empty());
}

TEST(ReaderSuperString, PipeAndCulvertSizesAreReadInBothTheManualsFormAndTwelveDs)
{
    // The manual documents diameter_value / diameter_data; 12d Model 15 writes
    // pipe_value { diameter } / pipe_data { properties { diameter } }.
    const auto manual = read("string super { data_3d { 0 0 0 1 0 0 } diameter_value 0.375 }");
    EXPECT_EQ(only<a12::VertexString>(manual).diameter, 0.375);
    const auto real = read("string super { data_3d { 0 0 0 1 0 0 2 0 0 }\n"
                           "pipe_data { properties { diameter 0.225 } properties { diameter 0.3 } } }");
    EXPECT_EQ(only<a12::VertexString>(real).diameters, (std::vector<double>{0.225, 0.3}));
    const auto value = read("string super { data_3d { 0 0 0 1 0 0 } pipe_value { diameter 0.45 thickness 0.02 } }");
    const auto& string = only<a12::VertexString>(value);
    EXPECT_EQ(string.diameter, 0.45);
    EXPECT_EQ(string.header.extras.real("pipe_value.thickness"), 0.02) << "understood, but not lost";

    const auto culvert = read("string super { data_3d { 0 0 0 1 0 0 2 0 0 } culvert_value { width 0.9 height 0.55 }\n"
                              "culvert_data { properties { width 1 height 2 } properties { width 3 height 4 } } }");
    const auto& c = only<a12::VertexString>(culvert);
    EXPECT_EQ(c.culvert, (std::array<double, 2>{0.9, 0.55}));
    ASSERT_EQ(c.culverts.size(), 2u);
    EXPECT_EQ(c.culverts[1], (std::array<double, 2>{3.0, 4.0}));

    // Both on one string, as 12d Model 15 writes every culvert: kept, and not
    // complained about - the manual says it cannot happen, and it does.
    const auto both = read("string super { name EU data_3d { 0 0 0 1 0 0 }"
                           " pipe_data { properties { diameter 0.125 } } culvert_data { properties { width 0.5 height 0.4 } } }");
    EXPECT_EQ(only<a12::VertexString>(both).diameters, (std::vector<double>{0.125}));
    EXPECT_EQ(only<a12::VertexString>(both).culverts.at(0), (std::array<double, 2>{0.5, 0.4}));
    EXPECT_TRUE(both.warnings.empty()) << both.warnings.front();
}

TEST(ReaderSuperString, AListOfTheWrongLengthIsReportedNotSilentlyAccepted)
{
    const auto archive = read("string super { name \"S\" data_3d { 0 0 0  1 0 0  2 0 0 } point_data { 1 2 } }");
    ASSERT_EQ(archive.warnings.size(), 1u);
    EXPECT_NE(archive.warnings[0].find("point_data has 2 entries for 3 vertices"), std::string::npos)
        << archive.warnings[0];
}

TEST(ReaderSuperString, ValuesThatDoNotDivideIntoRowsAreAnErrorNotAGuess)
{
    const auto archive = a12::readArchive("string super {\n data_3d {\n 0 0 0\n 1 1\n }\n}");
    ASSERT_FALSE(archive.ok());
    EXPECT_EQ(archive.error().code, ErrorCode::ParseFailure);
    EXPECT_NE(archive.error().message.find("5 values"), std::string::npos) << archive.error().describe();
}

TEST(ReaderSuperString, AWordWhereANumberBelongsIsAnError)
{
    const auto archive = a12::readArchive("string super { data_3d { 0 zero 0 } }");
    ASSERT_FALSE(archive.ok());
    EXPECT_NE(archive.error().message.find("zero"), std::string::npos);
}

TEST(ReaderSuperString, OnlyAHeightMayBeNull)
{
    EXPECT_FALSE(a12::readArchive("string super { data_3d { null 0 0 } }").ok());
    EXPECT_TRUE(a12::readArchive("string super { data_3d { 0 0 null } }").ok());
}

// ---- what the manual does not describe -------------------------------------------

TEST(ReaderUnknown, AnUnknownBlockIsSkippedWholeAndCountedByPath)
{
    const auto archive = read("string super { name \"A\" drawables { label { data_2d { 9 9 } } }\n"
                              " data_2d { 1 2 } solid_fill { colour cyan } }\n"
                              "string super { data_2d { 3 4 } drawables { } }\n"
                              "drafting_element_table { table { rows 3 } }");
    ASSERT_EQ(archive.elements.size(), 2u);
    const auto& first = std::get<a12::VertexString>(archive.elements[0]);
    ASSERT_EQ(first.vertices.size(), 1u) << "the data_2d inside the unknown block is not the string's";
    EXPECT_EQ(first.vertices[0], (a12::Vertex{1.0, 2.0, std::nullopt}));
    EXPECT_EQ(archive.unrecognised.at("string super/drawables"), 2u);
    EXPECT_EQ(archive.unrecognised.at("string super/solid_fill"), 1u);
    EXPECT_EQ(archive.unrecognised.at("drafting_element_table"), 1u);
}

TEST(ReaderUnknown, AnUnknownScalarIsKeptInOrder)
{
    const auto archive = read("string super { weight 2 time_created \"07-Mar-2009 03:53:17\""
                              " data_2d { 0 0 } frobnicate 7 }");
    const auto& extras = only<a12::VertexString>(archive).header.extras;
    ASSERT_EQ(extras.size(), 3u);
    EXPECT_EQ(extras.fields()[0].key, "weight");
    EXPECT_EQ(extras.real("weight"), 2.0);
    EXPECT_EQ(extras.text("time_created"), "07-Mar-2009 03:53:17");
    EXPECT_TRUE(extras.find("time_created")->quoted);
    EXPECT_EQ(extras.integer("frobnicate"), 7);
}

TEST(ReaderUnknown, AnUnknownFlagDoesNotSwallowTheKeywordAfterIt)
{
    const auto archive = read("string super { mystery_flag name \"Kept\" data_2d { 0 0 } }");
    const auto& string = only<a12::VertexString>(archive);
    EXPECT_EQ(string.header.name, "Kept");
    EXPECT_TRUE(string.header.extras.contains("mystery_flag"));
}

TEST(ReaderUnknown, AnUnknownStringTypeIsJumpedOverAsTheManualIntends)
{
    // Manual 1.4.6: a reader "has a chance of jumping over the string by
    // looking for the ending curly brace".
    const auto archive = read("string hologram { data { 1 2 3 } }\nstring super { data_2d { 0 0 } }");
    EXPECT_EQ(archive.elements.size(), 1u);
    EXPECT_EQ(archive.unrecognised.at("string hologram"), 1u);
}

TEST(ReaderLimits, AFileWithMoreElementsThanTheLimitIsRefused)
{
    a12::ReadOptions options;
    options.maxElements = 2;
    std::string text;
    for (int i = 0; i < 4; ++i) {
        text += "string super { data_2d { 0 0 } }\n";
    }
    const auto archive = a12::readArchive(text, options);
    ASSERT_FALSE(archive.ok());
    EXPECT_NE(archive.error().message.find("limit"), std::string::npos);
}

TEST(ReaderLimits, ABlockWithMoreValuesThanTheLimitIsRefused)
{
    a12::ReadOptions options;
    options.maxValuesPerBlock = 5;
    const auto archive = a12::readArchive("string super { data_3d { 1 2 3 4 5 6 7 8 9 } }", options);
    ASSERT_FALSE(archive.ok());
    EXPECT_NE(archive.error().message.find("limit"), std::string::npos);
}
