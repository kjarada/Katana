// What the Katana customisation format refuses (docs/customisation.md, "The
// reader is strict"): each refusal by its error code and by the entry and the
// member it names. The entries are named as the document says - the list, the
// place in it counted from 0, and the entry's own name: `codes[1] "WM*"`.
//
// The expectations are written by hand from those rules, not read off a run.

#include <gtest/gtest.h>

#include <cmath>
#include <limits>
#include <string>
#include <vector>

#include "customisation_fixture.hpp"
#include "katana/core/text_encoding.hpp"
#include "katana/entity/customisation.hpp"

using katana::core::Error;
using katana::core::ErrorCode;
using katana::entity::Customisation;
using katana::entity::CustomisationBase;
using katana::entity::CustomisationSourceNote;
using katana::entity::CustomisationWriteOptions;
using katana::entity::LineStyle;
using katana::entity::LineworkCodes;
using katana::entity::Stroke;
using katana::entity::StrokeOp;
using katana::entity::StrokeText;
using katana::entity::SurveyAttribute;
using katana::entity::SurveyPipe;
using katana::entity::SurveyRule;
using katana::testing::customisationWith;
using katana::testing::expectNames;
using katana::testing::readCustomisation;
using katana::testing::refusalOf;

namespace {

constexpr const char* kNotACustomisation = "not a Katana customisation file";

// What a refusal is checked against: the code, the entry and the member.
struct Refused {
    std::string members; // of a customisation otherwise sound
    std::string entry;
    std::string member;
};

void expectRefused(const std::vector<Refused>& cases, ErrorCode code)
{
    for (const Refused& refused : cases) {
        const Error error = refusalOf(customisationWith(refused.members));
        EXPECT_EQ(error.code, code) << refused.members << "\n" << error.describe();
        expectNames(error, refused.entry, refused.member);
    }
}

// A customisation holding one definition, which the writer is then asked for.
Customisation holding(LineStyle definition)
{
    Customisation customisation;
    customisation.name = "W";
    definition.source = "W";
    const auto added = customisation.library.add(std::move(definition));
    EXPECT_TRUE(added.ok()) << (added.ok() ? std::string{} : added.error().describe());
    return customisation;
}

Customisation holding(SurveyRule rule)
{
    Customisation customisation;
    customisation.name = "W";
    const auto added = customisation.map.add(std::move(rule));
    EXPECT_TRUE(added.ok()) << (added.ok() ? std::string{} : added.error().describe());
    return customisation;
}

Error writeRefusal(const Customisation& customisation,
                   const CustomisationWriteOptions& options = {})
{
    const auto written = katana::entity::customisationToJson(customisation, options);
    EXPECT_FALSE(written.ok()) << "written without complaint:\n"
                               << (written.ok() ? *written : std::string{});
    return written.ok() ? Error{} : written.error();
}

Stroke strokeOf(StrokeOp op)
{
    Stroke stroke;
    stroke.op = op;
    return stroke;
}

LineStyle named(std::string name)
{
    LineStyle definition;
    definition.name = std::move(name);
    return definition;
}

} // namespace

// ---- what is not a customisation file at all ------------------------------------------------

TEST(CustomisationRefusals, TextThatIsNotJsonIsNotAKatanaCustomisationFile)
{
    for (const char* text :
         {"", "   \n", "{", "not json", "{\"format\": \"katana-customisation\",}",
          "{\"format\": \"katana-customisation\", \"version\": 1, \"name\": \"T\"} x"}) {
        const Error error = refusalOf(text);
        EXPECT_EQ(error.code, ErrorCode::ParseFailure) << text;
        EXPECT_EQ(error.message, kNotACustomisation) << text;
    }
}

TEST(CustomisationRefusals, JsonThatDoesNotSayItIsOneIsNotAKatanaCustomisationFile)
{
    // No format; another format; a format that is not text; not an object.
    for (const char* text : {"{}", R"({"version": 1, "name": "T"})",
                             R"({"format": "katana-sheets", "version": 1, "name": "T"})",
                             R"({"format": "Katana-Customisation", "version": 1, "name": "T"})",
                             R"({"format": 1, "version": 1, "name": "T"})", "[]", "7",
                             "\"katana-customisation\"", "null"}) {
        const Error error = refusalOf(text);
        EXPECT_EQ(error.code, ErrorCode::ParseFailure) << text;
        EXPECT_EQ(error.message, kNotACustomisation) << text;
    }
}

TEST(CustomisationRefusals, MalformedJsonSaysWhereItStopsBeingJson)
{
    // The third line lacks the comma after its member. The message is the one
    // every text that is not JSON gets; the place is beside it.
    const Error error = refusalOf("{\n"
                                  "  \"format\": \"katana-customisation\",\n"
                                  "  \"version\": 1\n"
                                  "  \"name\": \"T\"\n"
                                  "}\n");
    EXPECT_EQ(error.code, ErrorCode::ParseFailure);
    EXPECT_EQ(error.message, kNotACustomisation);
    EXPECT_NE(error.context.find("line 4"), std::string::npos)
        << "the name on line 4 is where a comma was expected: " << error.describe();
}

// A survey code file of the older kind is XML, and is commonly UTF-16.
TEST(CustomisationRefusals, ASurveyCodeFileInXmlIsNotAKatanaCustomisationFile)
{
    const std::string xml = "<?xml version=\"1.0\"?>\n"
                            "<map_file><version>11.0</version>\n"
                            "<map_data><item><key>WM*</key><model>SURVEY SERVICES</model>"
                            "<colour>blue</colour></item></map_data></map_file>\n";
    const Error plain = refusalOf(xml);
    EXPECT_EQ(plain.code, ErrorCode::ParseFailure);
    EXPECT_EQ(plain.message, kNotACustomisation);

    const auto utf16 = katana::core::encodeUtf16LittleEndian(xml);
    ASSERT_TRUE(utf16.ok());
    const Error wide = refusalOf(*utf16);
    EXPECT_EQ(wide.code, ErrorCode::ParseFailure);
    EXPECT_EQ(wide.message, kNotACustomisation);
}

TEST(CustomisationRefusals, AStyleLibraryInItsOwnTextIsNotAKatanaCustomisationFile)
{
    const Error error = refusalOf("// a linestyle library\n"
                                  "worldstyle \"WATR Main\" {\n"
                                  "    group \"Survey/WATR\"\n"
                                  "    length 2.5\n"
                                  "    move 0 0\n"
                                  "    draw 1.5 0\n"
                                  "}\n");
    EXPECT_EQ(error.code, ErrorCode::ParseFailure);
    EXPECT_EQ(error.message, kNotACustomisation);
}

// ---- encodings ------------------------------------------------------------------------------

TEST(CustomisationRefusals, AFileWithAUtf8ByteOrderMarkIsRead)
{
    // "Zürich Süd": ü is C3 BC in UTF-8.
    const std::string text =
        "{\"format\": \"katana-customisation\", \"version\": 1, \"name\": \"Z\xC3\xBCrich S\xC3\xBC"
        "d\"}";
    const Customisation marked = readCustomisation("\xEF\xBB\xBF" + text);
    EXPECT_EQ(marked.name, "Z\xC3\xBCrich S\xC3\xBC"
                           "d");
    EXPECT_TRUE(marked == readCustomisation(text));
}

TEST(CustomisationRefusals, AFileInUtf16IsRead)
{
    // What an editor on Windows saves as "Unicode": UTF-16LE behind FF FE.
    const std::string text =
        "{\"format\": \"katana-customisation\", \"version\": 1, \"name\": \"Z\xC3\xBCrich\","
        " \"linestyles\": [{\"name\": \"A\", \"strokes\": [[\"move\", 0.5, -2]]}]}";
    const auto utf16 = katana::core::encodeUtf16LittleEndian(text);
    ASSERT_TRUE(utf16.ok());
    ASSERT_EQ(static_cast<unsigned char>((*utf16)[0]), 0xFF);
    ASSERT_EQ(static_cast<unsigned char>((*utf16)[1]), 0xFE);
    const Customisation read = readCustomisation(*utf16);
    EXPECT_EQ(read.name, "Z\xC3\xBCrich");
    ASSERT_NE(read.library.find("A"), nullptr);
    EXPECT_EQ(read.library.find("A")->strokes.at(0).point.y, -2.0);
    EXPECT_TRUE(read == readCustomisation(text));
}

TEST(CustomisationRefusals, BytesThatAreNotUtf8AreRefusedRatherThanGuessedAt)
{
    // "Café" saved by an editor in the Windows-1252 code page: E9 alone is
    // not UTF-8. Elsewhere such bytes are decoded as that code page; here the
    // name would become some other name without a word said.
    const Error error =
        refusalOf("{\"format\": \"katana-customisation\", \"version\": 1, \"name\": \"Caf\xE9\"}");
    EXPECT_EQ(error.code, ErrorCode::ParseFailure);
    EXPECT_EQ(error.message, kNotACustomisation);
    EXPECT_NE(error.context.find("UTF-8"), std::string::npos) << error.describe();
}

// ---- the version ----------------------------------------------------------------------------

TEST(CustomisationRefusals, ANewerVersionIsUnsupportedWhateverElseTheFileHolds)
{
    const Error newer =
        refusalOf(R"({"format": "katana-customisation", "version": 2, "name": "T"})");
    EXPECT_EQ(newer.code, ErrorCode::Unsupported);
    EXPECT_NE(newer.message.find("newer version of Katana"), std::string::npos) << newer.describe();
    EXPECT_NE(newer.context.find("version 2"), std::string::npos) << newer.describe();

    // A member this build has never heard of is what a newer file would hold;
    // it must be told the file is newer, not that the member is unknown. And
    // a version no 32-bit number holds is newer still.
    for (const char* text :
         {R"({"format": "katana-customisation", "version": 2, "name": "T", "layers": []})",
          R"({"format": "katana-customisation", "version": 7, "codes": "none"})",
          R"({"format": "katana-customisation", "version": 99999999999, "name": "T"})"}) {
        EXPECT_EQ(refusalOf(text).code, ErrorCode::Unsupported) << text;
    }
}

TEST(CustomisationRefusals, AVersionThatIsNotAWholeNumberFromOneIsRefused)
{
    for (const char* version : {"1.5", "1.0", "true", "\"1\"", "null", "0", "-1", "[1]"}) {
        const std::string text = std::string(R"({"format": "katana-customisation", "version": )") +
                                 version + R"(, "name": "T"})";
        const Error error = refusalOf(text);
        EXPECT_EQ(error.code, ErrorCode::ParseFailure) << text;
        expectNames(error, "top level", "version");
    }
    const Error missing = refusalOf(R"({"format": "katana-customisation", "name": "T"})");
    EXPECT_EQ(missing.code, ErrorCode::ParseFailure);
    expectNames(missing, "top level", "version");
}

// ---- the name -------------------------------------------------------------------------------

TEST(CustomisationRefusals, ACustomisationWithNoNameIsRefused)
{
    const Error error = refusalOf(R"({"format": "katana-customisation", "version": 1})");
    EXPECT_EQ(error.code, ErrorCode::ParseFailure) << "a member that must be there is not";
    expectNames(error, "top level", "name");

    const Error number =
        refusalOf(R"({"format": "katana-customisation", "version": 1, "name": 7})");
    EXPECT_EQ(number.code, ErrorCode::ParseFailure);
    expectNames(number, "top level", "name");
}

TEST(CustomisationRefusals, ANameThatAProjectCouldNotRecordIsRefused)
{
    // '/', '\', a line break and nothing at all: each well-formed, and each a
    // name the project store would refuse at the next SAVE.
    for (const char* name : {"Roads/2026", "Roads\\\\2026", "two\\nlines", ""}) {
        const std::string text =
            std::string(R"({"format": "katana-customisation", "version": 1, "name": ")") + name +
            "\"}";
        const Error error = refusalOf(text);
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument) << text;
        expectNames(error, "top level", "name");
    }
}

TEST(CustomisationRefusals, TheNamesOfSourcesBasesAndDefinitionSourcesAreHeldToTheSameRule)
{
    expectRefused(
        {
            {R"("sources": [{"name": "Fine"}, {"name": "a/b", "rules": true}])",
             R"(sources[1] "a/b")", "name"},
            {R"("basedOn": {"name": "a\\b", "digest": "0123456789abcdef"})", "basedOn", "name"},
            {R"("linestyles": [{"name": "A", "from": "x/y"}])", R"(linestyles[0] "A")", "from"},
            {R"("symbols": [{"name": "A"}, {"name": "B", "from": "two\nlines"}])",
             R"(symbols[1] "B")", "from"},
        },
        ErrorCode::InvalidArgument);
}

TEST(CustomisationRefusals, ABaseWithoutASoundDigestIsRefused)
{
    // Sixteen digits, hexadecimal, lower case: one short, one upper, one not hex.
    for (const char* digest : {"0123456789abcde", "0123456789ABCDEF", "0123456789abcdeg", ""}) {
        const Error error = refusalOf(customisationWith(
            std::string(R"("basedOn": {"name": "NSW", "digest": ")") + digest + "\"}"));
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument) << digest;
        expectNames(error, "basedOn", "digest");
    }
    expectRefused({{R"("basedOn": {"name": "NSW"})", "basedOn", "digest"},
                   {R"("basedOn": {"digest": "0123456789abcdef"})", "basedOn", "name"}},
                  ErrorCode::ParseFailure);
}

// ---- unknown members ------------------------------------------------------------------------

// At every level an object can sit at. A lenient reader would skip each of
// these; here "linesytle" is a refusal rather than a rule that draws nothing.
TEST(CustomisationRefusals, AnUnknownMemberIsRefusedAtEveryLevelNamingTheEntry)
{
    const std::vector<Refused> cases = {
        {R"("colors": {})", "top level", "colors"},
        {R"("sources": [{"name": "A", "defs": true}])", R"(sources[0] "A")", "defs"},
        {R"("basedOn": {"name": "A", "digest": "0123456789abcdef", "date": "x"})", "basedOn",
         "date"},
        {R"("linework": {"begin": "ST"})", "linework", "begin"},
        {R"("automation": {"codes": true})", "automation", "codes"},
        {R"("linestyles": [{"name": "A"}, {"name": "B", "colour": "red"}])", R"(linestyles[1] "B")",
         "colour"},
        {R"("symbols": [{"name": "TEST Valve", "mode": "vertex"}])", R"(symbols[0] "TEST Valve")",
         "mode"},
        {R"("symbols": [{"name": "V", "strokes": [["move", 0, 0], ["text", {"hieght": 1}]]}])",
         R"(symbols[0] "V" strokes[1])", "hieght"},
        {R"("codes": [{"key": "A", "sets": "feature"},
                      {"key": "WM*", "sets": "feature", "linesytle": "WATR Main"}])",
         R"(codes[1] "WM*")", "linesytle"},
        {R"("codes": [{"key": "AC*", "sets": "symbol", "symbol": {"name": "P", "scale": 2}}])",
         R"(codes[0] "AC*" symbol)", "scale"},
        {R"("codes": [{"key": "T*", "sets": "text", "text": {"style": "ISO", "bold": true}}])",
         R"(codes[0] "T*" text)", "bold"},
        {R"("codes": [{"key": "P*", "sets": "pipe", "pipe": {"diameter": "0.3"}}])",
         R"(codes[0] "P*" pipe)", "diameter"},
        {R"("codes": [{"key": "P*", "sets": "vertexPipe", "vertexPipe": {"diameter": "0.3"}}])",
         R"(codes[0] "P*" vertexPipe)", "diameter"},
        {R"("codes": [{"key": "P*", "sets": "segmentPipe", "segmentPipe": {"diameter": "0.3"}}])",
         R"(codes[0] "P*" segmentPipe)", "diameter"},
        {R"("codes": [{"key": "A*", "sets": "attributes", "attributes": [
                          {"type": "text", "name": "Owner"},
                          {"type": "text", "name": "Zone", "units": "m"}]}])",
         R"(codes[0] "A*" attributes[1])", "units"},
        {R"("codes": [{"key": "A*", "sets": "vertexAttributes", "vertexAttributes": [
                          {"type": "text", "name": "Zone", "units": "m"}]}])",
         R"(codes[0] "A*" vertexAttributes[0])", "units"},
        {R"("codes": [{"key": "A*", "sets": "segmentPipe", "segmentAttributes": [
                          {"type": "text", "name": "Zone", "units": "m"}]}])",
         R"(codes[0] "A*" segmentAttributes[0])", "units"},
    };
    for (const Refused& refused : cases) {
        const Error error = refusalOf(customisationWith(refused.members));
        EXPECT_EQ(error.code, ErrorCode::ParseFailure) << refused.members;
        EXPECT_EQ(error.message, refused.entry + ": unknown member \"" + refused.member + "\"")
            << refused.members;
    }
}

TEST(CustomisationRefusals, AMemberOfTheOlderFormatsIsUnknownHere)
{
    // "model" is what a survey code file calls a rule's layer; the format's
    // word is "layer", and the old one is not quietly taken for it.
    const Error error = refusalOf(customisationWith(
        R"("codes": [{"key": "WM*", "sets": "feature", "model": "SURVEY SERVICES"}])"));
    EXPECT_EQ(error.code, ErrorCode::ParseFailure);
    EXPECT_EQ(error.message, R"(codes[0] "WM*": unknown member "model")");
    // The refusal lists what the entry may hold, so the right word is at hand.
    EXPECT_NE(error.context.find("layer"), std::string::npos) << error.describe();
}

// ---- a member given twice -------------------------------------------------------------------

TEST(CustomisationRefusals, ASecondCodesListDoesNotSilentlyReplaceTheFirst)
{
    // Two rules, then a "codes" pasted again at the end holding one. A JSON
    // reader left to itself keeps the last and says nothing.
    const Error error = refusalOf(customisationWith(
        R"("codes": [{"key": "A", "sets": "feature"}, {"key": "B", "sets": "feature"}],
           "symbols": [], "codes": [{"key": "C", "sets": "feature"}])"));
    EXPECT_EQ(error.code, ErrorCode::ParseFailure);
    expectNames(error, "top level", "codes");
    EXPECT_NE(error.message.find("twice"), std::string::npos) << error.describe();
}

TEST(CustomisationRefusals, AMemberGivenTwiceIsRefusedAtEveryLevelNamingTheEntry)
{
    expectRefused(
        {
            {R"("name": "U")", "top level", "name"},
            {R"("colours": {"sui gas": "#FFFF00", "sui gas": "#FF0000"})", "colours", "sui gas"},
            {R"("linework": {"start": "S", "start": "ST"})", "linework", "start"},
            {R"("sources": [{"name": "A"}, {"name": "B", "rules": true, "rules": false}])",
             R"(sources[1] "B")", "rules"},
            {R"("symbols": [{"name": "TEST Valve", "group": "a", "group": "b"}])",
             R"(symbols[0] "TEST Valve")", "group"},
            {R"("linestyles": [{"name": "L", "strokes": [["move", 0, 0],
                                                        ["text", {"text": "a", "text": "b"}]]}])",
             R"(linestyles[0] "L" strokes[1][1])", "text"},
            {R"("codes": [{"key": "A", "sets": "feature"},
                          {"key": "WM*", "sets": "feature", "colour": "red", "colour": "blue"}])",
             R"(codes[1] "WM*")", "colour"},
            {R"("codes": [{"key": "AC*", "sets": "symbol", "symbol": {"size": 1, "size": 2}}])",
             R"(codes[0] "AC*" symbol)", "size"},
            {R"("codes": [{"key": "A*", "sets": "attributes", "attributes": [
                              {"type": "text", "name": "a", "name": "b"}]}])",
             R"(codes[0] "A*" attributes[0])", "name"},
        },
        ErrorCode::ParseFailure);
}

// ---- values of the wrong type ---------------------------------------------------------------

TEST(CustomisationRefusals, AWholeNumberMemberGivenAFractionIsRefusedRatherThanCutShort)
{
    expectRefused(
        {{R"("linestyles": [{"name": "G", "stretchMode": 2.7}])", R"(linestyles[0] "G")",
          "stretchMode"},
         {R"("linestyles": [{"name": "G", "cycleMode": 1.0}])", R"(linestyles[0] "G")",
          "cycleMode"},
         {R"("symbols": [{"name": "G", "cycleMode": 1e2}])", R"(symbols[0] "G")", "cycleMode"}},
        ErrorCode::ParseFailure);
}

TEST(CustomisationRefusals, AWholeNumberMemberGivenTrueIsRefusedRatherThanReadAsOne)
{
    expectRefused({{R"("linestyles": [{"name": "G", "stretchMode": true}])", R"(linestyles[0] "G")",
                    "stretchMode"},
                   {R"("linestyles": [{"name": "G", "cycleMode": false}])", R"(linestyles[0] "G")",
                    "cycleMode"},
                   {R"("linestyles": [{"name": "G", "cycleMode": "1"}])", R"(linestyles[0] "G")",
                    "cycleMode"}},
                  ErrorCode::ParseFailure);
}

TEST(CustomisationRefusals, AWholeNumberTooLargeToHoldIsRefused)
{
    // One past the largest and one before the smallest a 32-bit int holds.
    expectRefused({{R"("linestyles": [{"name": "G", "stretchMode": 2147483648}])",
                    R"(linestyles[0] "G")", "stretchMode"},
                   {R"("linestyles": [{"name": "G", "cycleMode": -2147483649}])",
                    R"(linestyles[0] "G")", "cycleMode"}},
                  ErrorCode::ParseFailure);
    // The two ends themselves are read.
    const Customisation ends = readCustomisation(customisationWith(
        R"("linestyles": [{"name": "G", "stretchMode": 2147483647, "cycleMode": -2147483648}])"));
    EXPECT_EQ(ends.library.find("G")->stretchMode, std::numeric_limits<int>::max());
    EXPECT_EQ(ends.library.find("G")->cycleMode, std::numeric_limits<int>::min());
}

TEST(CustomisationRefusals, AValueOfTheWrongKindIsRefusedNamingItsMember)
{
    expectRefused(
        {
            // text that must be text: a weight and a pipe size are kept as
            // written ("0", "Normal", "$PipeDiameter") and are never numbers
            {R"("codes": [{"key": "A", "sets": "feature", "weight": 0}])", R"(codes[0] "A")",
             "weight"},
            {R"("codes": [{"key": "A", "sets": "pipe", "pipe": {"size1": 0.375}}])",
             R"(codes[0] "A" pipe)", "size1"},
            {R"("codes": [{"key": "A", "sets": "feature", "layer": ["a"]}])", R"(codes[0] "A")",
             "layer"},
            {R"("description": 5)", "top level", "description"},
            {R"("linestyles": [{"name": "G", "group": null}])", R"(linestyles[0] "G")", "group"},
            // numbers that must be numbers
            {R"("linestyles": [{"name": "G", "length": "2.5"}])", R"(linestyles[0] "G")", "length"},
            {R"("linestyles": [{"name": "G", "factor": true}])", R"(linestyles[0] "G")", "factor"},
            {R"("codes": [{"key": "A", "sets": "symbol", "symbol": {"size": "big"}}])",
             R"(codes[0] "A" symbol)", "size"},
            {R"("codes": [{"key": "A", "sets": "text", "text": {"slant": null}}])",
             R"(codes[0] "A" text)", "slant"},
            // true and false that must be just that
            {R"("linestyles": [{"name": "G", "atVertices": 1}])", R"(linestyles[0] "G")",
             "atVertices"},
            {R"("codes": [{"key": "A", "sets": "symbol", "hide": "yes"}])", R"(codes[0] "A")",
             "hide"},
            {R"("codes": [{"key": "A", "sets": "surface", "surface": 0}])", R"(codes[0] "A")",
             "surface"},
            {R"("codes": [{"key": "A", "sets": "pipe", "pipe": {"active": "yes"}}])",
             R"(codes[0] "A" pipe)", "active"},
            {R"("automation": {"codesOnSurveyImport": "on"})", "automation", "codesOnSurveyImport"},
            {R"("sources": [{"name": "A", "rules": 1}])", R"(sources[0] "A")", "rules"},
            // points, lines of text and lists
            {R"("linestyles": [{"name": "G", "origin": [1]}])", R"(linestyles[0] "G")", "origin"},
            {R"("linestyles": [{"name": "G", "origin": [1, "2"]}])", R"(linestyles[0] "G")",
             "origin"},
            {R"("linestyles": [{"name": "G", "anchors": [[0, 0]]}])", R"(linestyles[0] "G")",
             "anchors"},
            {R"("linestyles": [{"name": "G", "anchors": [[0, 0], [1]]}])", R"(linestyles[0] "G")",
             "anchors"},
            {R"("linestyles": [{"name": "G", "strokes": {"move": [0, 0]}}])",
             R"(linestyles[0] "G")", "strokes"},
            {R"("notice": "one line")", "top level", "notice"},
            {R"("notice": ["one", 2])", "top level", "notice"},
            {R"("sources": [{"name": "A", "notice": "one line"}])", R"(sources[0] "A")", "notice"},
            {R"("sources": {"name": "A"})", "top level", "sources"},
            {R"("linestyles": {"name": "A"})", "top level", "linestyles"},
            {R"("symbols": "none")", "top level", "symbols"},
            {R"("codes": {"key": "A"})", "top level", "codes"},
            {R"("colours": ["red"])", "top level", "colours"},
            {R"("codes": [{"key": "A", "sets": "attributes", "attributes": {"type": "text"}}])",
             R"(codes[0] "A")", "attributes"},
            {R"("linework": {"start": 5})", "linework", "start"},
        },
        ErrorCode::ParseFailure);
}

TEST(CustomisationRefusals, AnEntryThatIsNotAnObjectIsRefusedByItsPlace)
{
    for (const auto& [members, entry] : std::vector<std::pair<std::string, std::string>>{
             {R"("linestyles": [{"name": "A"}, "B"])", "linestyles[1]"},
             {R"("symbols": [[]])", "symbols[0]"},
             {R"("codes": [{"key": "A", "sets": "feature"}, 7])", "codes[1]"},
             {R"("sources": [null])", "sources[0]"},
             {R"("basedOn": "NSW")", "basedOn"},
             {R"("linework": [])", "linework"},
             {R"("automation": true)", "automation"},
             {R"("codes": [{"key": "A", "sets": "symbol", "symbol": "Peg"}])",
              R"(codes[0] "A" symbol)"},
             {R"("codes": [{"key": "A", "sets": "attributes", "attributes": ["Owner"]}])",
              R"(codes[0] "A" attributes[0])"}}) {
        const Error error = refusalOf(customisationWith(members));
        EXPECT_EQ(error.code, ErrorCode::ParseFailure) << members;
        EXPECT_EQ(error.message, entry + ": must be an object, written {...}") << members;
    }
}

// ---- words outside an enumeration -----------------------------------------------------------

TEST(CustomisationRefusals, AWordTheFormatDoesNotHaveIsRefusedNamingItsMember)
{
    expectRefused(
        {
            {R"("linestyles": [{"name": "G", "units": "metric"}])", R"(linestyles[0] "G")",
             "units"},
            // the words are matched as written: no case is folded
            {R"("linestyles": [{"name": "G", "units": "Paper"}])", R"(linestyles[0] "G")", "units"},
            {R"("linestyles": [{"name": "G", "units": 1}])", R"(linestyles[0] "G")", "units"},
            {R"("codes": [{"key": "A", "sets": "feature", "draw": "polyline"}])", R"(codes[0] "A")",
             "draw"},
            {R"("codes": [{"key": "A", "sets": "feature", "draw": "Line"}])", R"(codes[0] "A")",
             "draw"},
            {R"("codes": [{"key": "A", "sets": "features"}])", R"(codes[0] "A")", "sets"},
            // the older format's word for the same section is not this one's
            {R"("codes": [{"key": "A", "sets": "map_data"}])", R"(codes[0] "A")", "sets"},
            {R"("codes": [{"key": "A", "sets": 1}])", R"(codes[0] "A")", "sets"},
        },
        ErrorCode::ParseFailure);
}

TEST(CustomisationRefusals, ARuleMustSayItsKeyAndWhatItSets)
{
    // No default for "sets": a symbol rule read as a feature rule would, in a
    // merge, replace that key's layer and colour rule.
    expectRefused({{R"("codes": [{"sets": "feature"}])", "codes[0]", "key"},
                   {R"("codes": [{"key": "A"}])", R"(codes[0] "A")", "sets"},
                   {R"("codes": [{"key": "A", "sets": "feature"}, {"key": "B", "layer": "L"}])",
                    R"(codes[1] "B")", "sets"}},
                  ErrorCode::ParseFailure);
}

TEST(CustomisationRefusals, AnAttributeIsTextOrAnIntegerAndNothingElse)
{
    expectRefused(
        {
            {R"("codes": [{"key": "A", "sets": "attributes", "attributes": [
                              {"type": "real", "name": "Depth", "value": "1.2"}]}])",
             R"(codes[0] "A" attributes[0])", "type"},
            {R"("codes": [{"key": "A", "sets": "vertexAttributes", "vertexAttributes": [
                              {"type": "text", "name": "a"}, {"type": "Text", "name": "b"}]}])",
             R"(codes[0] "A" vertexAttributes[1])", "type"},
            {R"("codes": [{"key": "A", "sets": "segmentPipe", "segmentAttributes": [
                              {"name": "b"}]}])",
             R"(codes[0] "A" segmentAttributes[0])", "type"},
            {R"("codes": [{"key": "A", "sets": "attributes", "attributes": [
                              {"type": 3, "name": "b"}]}])",
             R"(codes[0] "A" attributes[0])", "type"},
            {R"("codes": [{"key": "A", "sets": "attributes", "attributes": [{"type": "text"}]}])",
             R"(codes[0] "A" attributes[0])", "name"},
            {R"("codes": [{"key": "A", "sets": "attributes", "attributes": [
                              {"type": "integer", "name": "Zone", "value": 1}]}])",
             R"(codes[0] "A" attributes[0])", "value"},
        },
        ErrorCode::ParseFailure);
    // An attribute with a name that is empty is well-formed and still not an
    // attribute: entity::validate refuses it.
    const Error unnamed = refusalOf(customisationWith(
        R"("codes": [{"key": "A", "sets": "attributes", "attributes": [{"type": "text", "name": ""}]}])"));
    EXPECT_EQ(unnamed.code, ErrorCode::InvalidArgument);
    EXPECT_EQ(unnamed.message.rfind(R"(codes[0] "A": )", 0), 0u) << unnamed.describe();
}

// ---- strokes --------------------------------------------------------------------------------

namespace {

// A symbol whose strokes are nine sound ones and then `tenth`: strokes[9].
std::string testValveWith(std::string_view tenth)
{
    std::string members = R"("symbols": [{"name": "TEST Valve", "strokes": [)";
    for (int i = 0; i < 9; ++i) {
        members += R"(["draw", 1, 0], )";
    }
    members += tenth;
    members += "]}]";
    return members;
}

} // namespace

TEST(CustomisationRefusals, AStrokeOfTheWrongLengthIsRefusedNamingTheStroke)
{
    // Each kind with a value too few or too many. An arc is a radius and two
    // angles; move and draw a point; circle and dot a radius; pen a name; text
    // one object.
    for (const char* stroke :
         {R"(["arc", 1, 2])", R"(["arc", 1, 2, 3, 4])", R"(["move", 1])", R"(["move", 1, 2, 3])",
          R"(["draw", 0])", R"(["circle"])", R"(["circle", 1, 2])", R"(["dot"])",
          R"(["dot", 1, 2])", R"(["pen"])", R"(["pen", "red", "blue"])", R"(["text"])",
          R"(["text", {}, {}])"}) {
        const Error error = refusalOf(customisationWith(testValveWith(stroke)));
        EXPECT_EQ(error.code, ErrorCode::ParseFailure) << stroke;
        const std::string_view text(stroke);
        const std::string_view word = text.substr(2, text.find('"', 2) - 2); // ["arc" -> arc
        expectNames(error, R"(symbols[0] "TEST Valve" strokes[9])", word);
    }
}

TEST(CustomisationRefusals, AStrokeThatIsNotOneOfTheSevenKindsIsRefused)
{
    for (const char* stroke : {R"(["curve", 1, 2])", R"(["Move", 0, 0])", R"(["colour", "red"])"}) {
        const Error error = refusalOf(customisationWith(testValveWith(stroke)));
        EXPECT_EQ(error.code, ErrorCode::ParseFailure) << stroke;
        const std::string_view text(stroke);
        expectNames(error, R"(symbols[0] "TEST Valve" strokes[9])",
                    text.substr(2, text.find('"', 2) - 2));
    }
    // And what is not a stroke's shape at all.
    for (const char* stroke : {R"("move 0 0")", "[]", "[0, 0]", R"({"move": [0, 0]})", "null"}) {
        const Error error = refusalOf(customisationWith(testValveWith(stroke)));
        EXPECT_EQ(error.code, ErrorCode::ParseFailure) << stroke;
        EXPECT_EQ(error.message.rfind(R"(symbols[0] "TEST Valve" strokes[9]: )", 0), 0u)
            << error.describe();
    }
}

TEST(CustomisationRefusals, AStrokeWithAValueOfTheWrongKindIsRefused)
{
    expectRefused(
        {
            {testValveWith(R"(["move", "0", 0])"), R"(symbols[0] "TEST Valve" strokes[9])", "move"},
            {testValveWith(R"(["draw", 0, null])"), R"(symbols[0] "TEST Valve" strokes[9])",
             "draw"},
            {testValveWith(R"(["arc", 1, true, 3])"), R"(symbols[0] "TEST Valve" strokes[9])",
             "arc"},
            {testValveWith(R"(["circle", [1]])"), R"(symbols[0] "TEST Valve" strokes[9])",
             "circle"},
            {testValveWith(R"(["dot", "0"])"), R"(symbols[0] "TEST Valve" strokes[9])", "dot"},
            {testValveWith(R"(["pen", 35])"), R"(symbols[0] "TEST Valve" strokes[9])", "pen"},
            {testValveWith(R"(["text", {"height": "1.5"}])"),
             R"(symbols[0] "TEST Valve" strokes[9])", "height"},
            {testValveWith(R"(["text", {"extra": [0, 0]}])"),
             R"(symbols[0] "TEST Valve" strokes[9])", "extra"},
            {testValveWith(R"(["text", {"extra": [0, 0, "0"]}])"),
             R"(symbols[0] "TEST Valve" strokes[9])", "extra"},
        },
        ErrorCode::ParseFailure);
    const Error text = refusalOf(customisationWith(testValveWith(R"(["text", "W"])")));
    EXPECT_EQ(text.code, ErrorCode::ParseFailure);
    EXPECT_EQ(text.message.rfind(R"(symbols[0] "TEST Valve" strokes[9]: )", 0), 0u)
        << text.describe();
}

// ---- well-formed, and still not a customisation ---------------------------------------------

TEST(CustomisationRefusals, TwoDefinitionsOfOneNameAreRefused)
{
    const Error within = refusalOf(
        customisationWith(R"("linestyles": [{"name": "A"}, {"name": "B"}, {"name": "A"}])"));
    EXPECT_EQ(within.code, ErrorCode::InvalidArgument);
    EXPECT_EQ(within.message.rfind(R"(linestyles[2] "A": )", 0), 0u) << within.describe();

    // The two lists are one library: a name is taken once across both.
    const Error across = refusalOf(customisationWith(
        R"("linestyles": [{"name": "A"}], "symbols": [{"name": "B"}, {"name": "A"}])"));
    EXPECT_EQ(across.code, ErrorCode::InvalidArgument);
    EXPECT_EQ(across.message.rfind(R"(symbols[1] "A": )", 0), 0u) << across.describe();
    EXPECT_NE(across.message.find("\"linestyles\""), std::string::npos)
        << "it says where the other one is: " << across.describe();

    // Names are compared with regard to case, as everywhere in the model.
    EXPECT_EQ(
        readCustomisation(customisationWith(R"("linestyles": [{"name": "A"}, {"name": "a"}])"))
            .library.size(),
        2u);
}

TEST(CustomisationRefusals, TwoColourNamesThatFoldToOneAreRefused)
{
    // Lower case, '_' and '-' as a blank: three spellings of one name.
    for (const char* colours : {R"({"SUI Gas": "#FFFF00", "sui_gas": "#FF0000"})",
                                R"({"sui gas": "#FFFF00", "sui-gas": "#FF0000"})",
                                R"({"off gray": "#808080", "Off Grey": "#808081"})"}) {
        const Error error = refusalOf(customisationWith(std::string("\"colours\": ") + colours));
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument) << colours;
        EXPECT_EQ(error.message.rfind("colours \"", 0), 0u) << error.describe();
    }
    // Whichever of the two is met second is the one refused; both are named.
    const Error both =
        refusalOf(customisationWith(R"("colours": {"SUI Gas": "#FFFF00", "sui_gas": "#FF0000"})"));
    EXPECT_NE(both.describe().find("SUI Gas"), std::string::npos) << both.describe();
    EXPECT_NE(both.describe().find("sui_gas"), std::string::npos) << both.describe();
}

TEST(CustomisationRefusals, AColourNameThatIsAStandardNameIsRefused)
{
    // "red" in any spelling the fold reads as red, and a standard name of two
    // words; a table that redefined one would draw one name in two colours.
    expectRefused(
        {{R"("colours": {"red": "#FF0001"})", R"(colours "red")", "red"},
         {R"("colours": {"sui gas": "#FFFF00", "Red": "#FF0000"})", R"(colours "Red")", "red"},
         {R"("colours": {"Dark_Gray": "#404040"})", R"(colours "Dark_Gray")", "dark grey"}},
        ErrorCode::InvalidArgument);
}

TEST(CustomisationRefusals, AColourThatIsNotHexadecimalIsRefused)
{
    expectRefused({{R"("colours": {"sui gas": "yellow"})", R"(colours "sui gas")", "yellow"},
                   {R"("colours": {"sui gas": "#FFFF0"})", R"(colours "sui gas")", "#FFFF0"},
                   {R"("colours": {"sui gas": "#GGFF00"})", R"(colours "sui gas")", "#GGFF00"}},
                  ErrorCode::ParseFailure);
    const Error number = refusalOf(customisationWith(R"("colours": {"sui gas": 16776960})"));
    EXPECT_EQ(number.code, ErrorCode::ParseFailure);
    EXPECT_EQ(number.message.rfind(R"(colours "sui gas": )", 0), 0u) << number.describe();

    // Either case of digit is read, and a fourth pair is the opacity.
    const Customisation read = readCustomisation(
        customisationWith(R"("colours": {"sui gas": "#ffFF00", "glass": "#0102037f"})"));
    EXPECT_EQ(read.colours.find("sui gas"), (katana::entity::Color{255, 255, 0, 255}));
    EXPECT_EQ(read.colours.find("glass"), (katana::entity::Color{1, 2, 3, 127}));
}

TEST(CustomisationRefusals, AKeyWithAWildcardAnywhereButLastIsRefused)
{
    for (const char* key : {"W*M", "*WM", "W**", "WM?"}) {
        const Error error = refusalOf(customisationWith(
            std::string(R"("codes": [{"key": "A", "sets": "feature"}, {"key": ")") + key +
            R"(", "sets": "feature"}])"));
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument) << key;
        const std::string entry = "codes[1] \"" + std::string(key) + "\": ";
        EXPECT_EQ(error.message.rfind(entry, 0), 0u) << error.describe();
        EXPECT_NE(error.message.find("key", entry.size()), std::string::npos)
            << "it is the key that is wrong: " << error.describe();
    }
}

TEST(CustomisationRefusals, AKeyWithBlanksAroundItIsRefusedRatherThanTrimmed)
{
    for (const char* key : {" WM*", "WM* ", "\\tWM"}) {
        const Error error = refusalOf(customisationWith(std::string(R"("codes": [{"key": ")") +
                                                        key + R"(", "sets": "feature"}])"));
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument) << key;
        EXPECT_EQ(error.message.rfind("codes[0] \"", 0), 0u) << error.describe();
        EXPECT_NE(error.message.find("key"), std::string::npos) << error.describe();
    }
    const Error empty =
        refusalOf(customisationWith(R"("codes": [{"key": "", "sets": "feature"}])"));
    EXPECT_EQ(empty.code, ErrorCode::InvalidArgument);
    EXPECT_EQ(empty.message.rfind(R"(codes[0] "": )", 0), 0u) << empty.describe();
}

TEST(CustomisationRefusals, ALayerThatIsNotALayerPathIsRefused)
{
    // An empty level, a leading separator, "..", and a level that is a blank.
    for (const char* layer : {"SURVEY//SERVICES", "/SURVEY", "SURVEY/..", "SURVEY/ /X"}) {
        const Error error = refusalOf(customisationWith(
            std::string(R"("codes": [{"key": "WM*", "sets": "feature", "layer": ")") + layer +
            "\"}]"));
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument) << layer;
        expectNames(error, R"(codes[0] "WM*")", "layer");
    }
    // A path of several levels is one.
    EXPECT_EQ(
        readCustomisation(
            customisationWith(
                R"("codes": [{"key": "WM*", "sets": "feature", "layer": "SURVEY/SERVICES/WATER"}])"))
            .map.rules()
            .at(0)
            .model,
        "SURVEY/SERVICES/WATER");
}

TEST(CustomisationRefusals, WhatValidateRefusesOfADefinitionIsRefusedNamingIt)
{
    // A factor of 0, a negative length, a negative text height: numbers the
    // format can write and the model will not take.
    for (const char* definition :
         {R"({"name": "D", "factor": 0})", R"({"name": "D", "factor": -1})",
          R"({"name": "D", "length": -2.5})",
          R"({"name": "D", "strokes": [["text", {"height": -1}]]})"}) {
        const Error error = refusalOf(
            customisationWith(std::string(R"("linestyles": [{"name": "C"}, )") + definition + "]"));
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument) << definition;
        EXPECT_EQ(error.message.rfind(R"(linestyles[1] "D": )", 0), 0u) << error.describe();
    }
    const Error unnamed = refusalOf(customisationWith(R"("symbols": [{"name": ""}])"));
    EXPECT_EQ(unnamed.code, ErrorCode::InvalidArgument);
    EXPECT_EQ(unnamed.message.rfind(R"(symbols[0] "": )", 0), 0u) << unnamed.describe();
    const Error noName = refusalOf(customisationWith(R"("symbols": [{"group": "G"}])"));
    EXPECT_EQ(noName.code, ErrorCode::ParseFailure);
    expectNames(noName, "symbols[0]", "name");
}

TEST(CustomisationRefusals, LineworkCodesThatCouldNotBeToldApartAreRefused)
{
    // "st" and "ST" are one token to the reader of a field code; a spelling
    // with a blank in it could never be one token at all.
    for (const char* linework :
         {R"({"start": "ST", "end": "st"})", R"({"close": "C L"})", R"({"join": "END"})"}) {
        const Error error = refusalOf(customisationWith(std::string("\"linework\": ") + linework));
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument) << linework;
        EXPECT_EQ(error.message.rfind("linework: ", 0), 0u) << error.describe();
    }
}

// ---- what the writer refuses ----------------------------------------------------------------

TEST(CustomisationWriter, ANumberThatIsNotFiniteIsRefusedNamingTheStroke)
{
    // entity::validate does not look at a text's three trailing numbers, so a
    // library can hold one that is not a number at all; JSON cannot.
    for (const double bad :
         {std::numeric_limits<double>::quiet_NaN(), std::numeric_limits<double>::infinity(),
          -std::numeric_limits<double>::infinity()}) {
        LineStyle definition = named("N");
        definition.strokes.push_back(strokeOf(StrokeOp::Move));
        Stroke text = strokeOf(StrokeOp::Text);
        text.text = 0;
        definition.strokes.push_back(text);
        definition.texts.push_back(StrokeText{"W", 0, 1, "", "", 1, {0, bad, 0}});
        const Error error = writeRefusal(holding(definition));
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument);
        expectNames(error, R"(linestyles[0] "N" strokes[1])", "extra");
    }
}

TEST(CustomisationWriter, TextThatIsNotUtf8IsRefusedNamingWhereItIs)
{
    // FF is never a byte of UTF-8. Each of these is text entity::validate
    // does not check, so each can reach the writer.
    Customisation described;
    described.name = "W";
    described.description = "caf\xFF";
    Error error = writeRefusal(described);
    EXPECT_EQ(error.code, ErrorCode::InvalidArgument);
    expectNames(error, "top level", "description");

    Customisation noticed;
    noticed.name = "W";
    noticed.notice = {"fine", "caf\xFF"};
    error = writeRefusal(noticed);
    EXPECT_EQ(error.code, ErrorCode::InvalidArgument);
    expectNames(error, "top level", "notice");

    Customisation sourced;
    sourced.name = "W";
    sourced.sources = {CustomisationSourceNote{"S", true, false, {"caf\xFF"}}};
    error = writeRefusal(sourced);
    EXPECT_EQ(error.code, ErrorCode::InvalidArgument);
    expectNames(error, R"(sources[0] "S")", "notice");

    SurveyRule pipe;
    pipe.key = "P*";
    pipe.pipe = SurveyPipe{"Obvert", "", "", "", false};
    pipe.pipe->justify = "caf\xFF";
    error = writeRefusal(holding(pipe));
    EXPECT_EQ(error.code, ErrorCode::InvalidArgument);
    expectNames(error, R"(codes[0] "P*" pipe)", "justify");

    SurveyRule text;
    text.key = "T*";
    text.textStyle = katana::entity::SurveyTextStyle{};
    text.textStyle->justifyX = "caf\xFF";
    error = writeRefusal(holding(text));
    EXPECT_EQ(error.code, ErrorCode::InvalidArgument);
    expectNames(error, R"(codes[0] "T*" text)", "justifyX");

    // A definition's source is not validated by the library either.
    Customisation from;
    from.name = "W";
    LineStyle definition = named("D");
    definition.source = "caf\xFF";
    ASSERT_TRUE(from.library.add(definition).ok());
    error = writeRefusal(from);
    EXPECT_EQ(error.code, ErrorCode::InvalidArgument);
    expectNames(error, R"(linestyles[0] "D")", "from");
}

TEST(CustomisationWriter, AStrokeCarryingAMemberItsKindDoesNotUseIsRefused)
{
    // The model has one flat stroke for all seven kinds, and validate lets any
    // member be set on any of them; a file has a place only for the ones the
    // kind uses. Each of these would come back without the stray member.
    struct Stray {
        Stroke stroke;
        const char* kind;
        const char* what;
    };
    std::vector<Stray> cases;
    Stroke stroke = strokeOf(StrokeOp::Move);
    stroke.radius = 1;
    cases.push_back({stroke, "move", "a radius"});
    stroke = strokeOf(StrokeOp::Draw);
    stroke.endAngle = 90;
    cases.push_back({stroke, "draw", "an angle"});
    stroke = strokeOf(StrokeOp::Circle);
    stroke.point = {1, 0};
    cases.push_back({stroke, "circle", "a point"});
    stroke = strokeOf(StrokeOp::Circle);
    stroke.pen = "red";
    cases.push_back({stroke, "circle", "a pen"});
    stroke = strokeOf(StrokeOp::Dot);
    stroke.startAngle = 1;
    cases.push_back({stroke, "dot", "an angle"});
    stroke = strokeOf(StrokeOp::Pen);
    stroke.pen = "red";
    stroke.radius = 2;
    cases.push_back({stroke, "pen", "a radius"});
    stroke = strokeOf(StrokeOp::Arc);
    stroke.point = {0, 1};
    cases.push_back({stroke, "arc", "a point"});
    // Negative zero is a value the kind does not use too: == calls it 0, and
    // the bits would not come back.
    stroke = strokeOf(StrokeOp::Move);
    stroke.radius = -0.0;
    cases.push_back({stroke, "move", "a radius"});

    for (const Stray& stray : cases) {
        LineStyle definition = named("S");
        definition.strokes = {strokeOf(StrokeOp::Move), stray.stroke};
        const Error error = writeRefusal(holding(definition));
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument) << stray.kind;
        EXPECT_EQ(error.message.rfind(R"(linestyles[0] "S" strokes[1]: )", 0), 0u)
            << error.describe();
        EXPECT_NE(
            error.message.find(std::string("this ") + stray.kind + " stroke carries " + stray.what),
            std::string::npos)
            << error.describe();
    }

    // A stroke that is not a text and names one.
    LineStyle named1 = named("S");
    Stroke naming = strokeOf(StrokeOp::Draw);
    naming.text = 0;
    named1.strokes = {naming};
    named1.texts = {StrokeText{}};
    const Error error = writeRefusal(holding(named1));
    EXPECT_EQ(error.code, ErrorCode::InvalidArgument);
    EXPECT_EQ(error.message.rfind(R"(linestyles[0] "S" strokes[0]: )", 0), 0u) << error.describe();
}

TEST(CustomisationWriter, TextsThatAreNotExactlyTheTextStrokesInOrderAreRefused)
{
    const auto textStroke = [](std::size_t index) {
        Stroke stroke = strokeOf(StrokeOp::Text);
        stroke.text = index;
        return stroke;
    };
    // A text no stroke places: it would be gone on the way back.
    LineStyle unplaced = named("T");
    unplaced.strokes = {strokeOf(StrokeOp::Move)};
    unplaced.texts = {StrokeText{}};
    Error error = writeRefusal(holding(unplaced));
    EXPECT_EQ(error.code, ErrorCode::InvalidArgument);
    EXPECT_EQ(error.message.rfind(R"(linestyles[0] "T": )", 0), 0u) << error.describe();
    EXPECT_NE(error.message.find("1 text and 0 text strokes"), std::string::npos)
        << error.describe();

    // One text placed by two strokes: it would come back as two texts.
    LineStyle shared = named("T");
    shared.strokes = {textStroke(0), textStroke(0)};
    shared.texts = {StrokeText{}};
    error = writeRefusal(holding(shared));
    EXPECT_EQ(error.code, ErrorCode::InvalidArgument);
    EXPECT_EQ(error.message.rfind(R"(linestyles[0] "T" strokes[1]: )", 0), 0u) << error.describe();

    // Two texts placed in the other order: the file keeps a text inside its
    // stroke, so the texts would come back swapped.
    LineStyle swapped = named("T");
    swapped.strokes = {textStroke(1), textStroke(0)};
    swapped.texts = {StrokeText{"a"}, StrokeText{"b"}};
    error = writeRefusal(holding(swapped));
    EXPECT_EQ(error.code, ErrorCode::InvalidArgument);
    EXPECT_EQ(error.message.rfind(R"(linestyles[0] "T" strokes[0]: )", 0), 0u) << error.describe();

    // In step, the same two are written.
    LineStyle sound = named("T");
    sound.strokes = {textStroke(0), strokeOf(StrokeOp::Move), textStroke(1)};
    sound.texts = {StrokeText{"a"}, StrokeText{"b"}};
    EXPECT_TRUE(katana::entity::customisationToJson(holding(sound)).ok());
}

TEST(CustomisationWriter, AnAttributeOfAnyOtherTypeIsRefusedBecauseItCouldNotBeReadBack)
{
    for (const char* type : {"real", "", "Text"}) {
        SurveyRule rule;
        rule.key = "A*";
        rule.attributes = {SurveyAttribute{"text", "Owner", "x"}};
        rule.vertexAttributes = {SurveyAttribute{"integer", "N", "1"},
                                 SurveyAttribute{type, "Depth", "1.2"}};
        const Error error = writeRefusal(holding(rule));
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument) << type;
        expectNames(error, R"(codes[0] "A*" vertexAttributes[1])", "type");
    }
}

TEST(CustomisationWriter, WhatTheReaderWouldRefuseOfTheHeaderIsNotWritten)
{
    // Each of these, written, would be a file Katana could not open again.
    Customisation unnamed;
    Error error = writeRefusal(unnamed);
    EXPECT_EQ(error.code, ErrorCode::InvalidArgument);
    expectNames(error, "top level", "name");

    Customisation path;
    path.name = "Roads/2026";
    error = writeRefusal(path);
    EXPECT_EQ(error.code, ErrorCode::InvalidArgument);
    expectNames(error, "top level", "name");

    Customisation source;
    source.name = "W";
    source.sources = {CustomisationSourceNote{"fine"}, CustomisationSourceNote{"a\\b"}};
    error = writeRefusal(source);
    EXPECT_EQ(error.code, ErrorCode::InvalidArgument);
    expectNames(error, R"(sources[1] "a\\b")", "name");

    Customisation base;
    base.name = "W";
    base.basedOn = CustomisationBase{"NSW", "not a digest"};
    error = writeRefusal(base);
    EXPECT_EQ(error.code, ErrorCode::InvalidArgument);
    expectNames(error, "basedOn", "digest");

    Customisation linework;
    linework.name = "W";
    linework.linework = LineworkCodes{};
    linework.linework->end = "ST"; // the start's spelling
    error = writeRefusal(linework);
    EXPECT_EQ(error.code, ErrorCode::InvalidArgument);
    EXPECT_EQ(error.message.rfind("linework: ", 0), 0u) << error.describe();

    Customisation from;
    from.name = "W";
    LineStyle definition = named("D");
    definition.source = "x/y";
    ASSERT_TRUE(from.library.add(definition).ok());
    error = writeRefusal(from);
    EXPECT_EQ(error.code, ErrorCode::InvalidArgument);
    expectNames(error, R"(linestyles[0] "D")", "from");
}

// The claim the writer's refusals rest on: whatever it does write, the reader
// takes back. Shown here for the values nearest the refusals above - the
// members a kind DOES use at awkward values, texts in step, both attribute
// types, a source of another name and of none.
TEST(CustomisationWriter, WhatItDoesWriteNextToEachRefusalReadsBackEqual)
{
    Customisation customisation;
    customisation.name = "W";
    LineStyle definition = named("Edge");
    definition.source = "";
    Stroke arc = strokeOf(StrokeOp::Arc);
    arc.radius = -1.75;
    arc.startAngle = 720;
    arc.endAngle = -0.5;
    Stroke dot = strokeOf(StrokeOp::Dot); // radius 0: the smallest drawable
    Stroke pen = strokeOf(StrokeOp::Pen); // an empty pen name is still a pen
    Stroke text = strokeOf(StrokeOp::Text);
    text.text = 0;
    definition.strokes = {arc, dot, pen, text};
    definition.texts = {StrokeText{"", 0, 0, "", "", 1, {1e-300, -1e300, 0.1}}};
    ASSERT_TRUE(customisation.library.add(definition).ok());
    SurveyRule rule;
    rule.key = "A*";
    rule.segmentAttributes = {SurveyAttribute{"integer", "N", ""},
                              SurveyAttribute{"text", "N", "the same name twice is kept"}};
    ASSERT_TRUE(customisation.map.add(rule).ok());

    const auto written = katana::entity::customisationToJson(customisation);
    ASSERT_TRUE(written.ok()) << written.error().describe();
    EXPECT_TRUE(readCustomisation(*written) == customisation) << *written;
}
