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
// What a file that begins with '<' or "//" - another program's format - is told
// beside that, with nothing of the JSON parser's account of the character.
constexpr const char* kOlderFormat =
    "not a Katana customisation file; the older formats are converted with "
    "katana_customisation_convert (a developer's tool, not installed; see docs/customisation.md)";

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
                            "<map_data><item><key>WM*</key><model>TEST SERVICES</model>"
                            "<colour>blue</colour></item></map_data></map_file>\n";
    const Error plain = refusalOf(xml);
    EXPECT_EQ(plain.code, ErrorCode::ParseFailure);
    EXPECT_EQ(plain.message, kOlderFormat);
    EXPECT_TRUE(plain.context.empty()) << "the parser's own sentence is not shown: " << plain.context;

    const auto utf16 = katana::core::encodeUtf16LittleEndian(xml);
    ASSERT_TRUE(utf16.ok());
    const Error wide = refusalOf(*utf16);
    EXPECT_EQ(wide.code, ErrorCode::ParseFailure);
    EXPECT_EQ(wide.message, kOlderFormat);
    EXPECT_TRUE(wide.context.empty()) << wide.context;
}

TEST(CustomisationRefusals, AStyleLibraryInItsOwnTextIsNotAKatanaCustomisationFile)
{
    const Error error = refusalOf("// a linestyle library\n"
                                  "worldstyle \"TEST Water Main\" {\n"
                                  "    group \"Test/Water\"\n"
                                  "    length 2.5\n"
                                  "    move 0 0\n"
                                  "    draw 1.5 0\n"
                                  "}\n");
    EXPECT_EQ(error.code, ErrorCode::ParseFailure);
    EXPECT_EQ(error.message, kOlderFormat);
    EXPECT_TRUE(error.context.empty()) << error.context;
}

TEST(CustomisationRefusals, OnlyAFirstCharacterOfMarkupOrACommentSendsAFileToTheConverter)
{
    // After blanks and a byte order mark, as JSON allows blanks: the first
    // character that is not one decides.
    for (const char* text : {"  \n\t<map/>", "\xEF\xBB\xBF<?xml version=\"1.0\"?>", "\r\n// notice\n"}) {
        const Error error = refusalOf(text);
        EXPECT_EQ(error.message, kOlderFormat) << text;
        EXPECT_TRUE(error.context.empty()) << text;
    }
    // A single slash is no comment, a '<' later in a file is not its first
    // character, and a file of the older style libraries that begins with a
    // word is not told about the converter: it is only not a customisation,
    // with the parser's account beside it as ever.
    for (const char* text : {"/ not a comment", "{ \"a\": < }", "worldstyle \"A\" { }"}) {
        const Error error = refusalOf(text);
        EXPECT_EQ(error.message, kNotACustomisation) << text;
        EXPECT_FALSE(error.context.empty()) << text;
    }
    // A definition read on its own is not told of the converter either: its
    // text is no file of another program.
    const auto definition = katana::entity::definitionFromJson("<x/>", true);
    ASSERT_FALSE(definition.ok());
    EXPECT_EQ(definition.error().message.find("katana_customisation_convert"), std::string::npos)
        << definition.error().describe();
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

// A byte order mark is a promise about the bytes behind it. When they break
// it the file is still "bytes that are neither UTF-8 nor UTF-16", and gets the
// one message every such file gets, with what is wrong beside it - not the
// decoder's own message under some other headline.
TEST(CustomisationRefusals, BytesThatBreakTheirOwnByteOrderMarkAreNotAKatanaCustomisationFile)
{
    const std::string sound = R"({"format": "katana-customisation", "version": 1, "name": "T"})";

    // EF BB BF says UTF-8; E9 alone ("Café" in Windows-1252) is not.
    const Error marked = refusalOf(
        "\xEF\xBB\xBF{\"format\": \"katana-customisation\", \"version\": 1, \"name\": \"Caf\xE9\"}");
    EXPECT_EQ(marked.code, ErrorCode::ParseFailure);
    EXPECT_EQ(marked.message, kNotACustomisation);
    EXPECT_NE(marked.context.find("UTF-8"), std::string::npos) << marked.describe();

    // FF FE says UTF-16, which is two bytes a unit: one byte more is a file
    // cut short.
    auto utf16 = katana::core::encodeUtf16LittleEndian(sound);
    ASSERT_TRUE(utf16.ok());
    ASSERT_EQ(utf16->size() % 2, 0u);
    const Error odd = refusalOf(*utf16 + "}");
    EXPECT_EQ(odd.code, ErrorCode::ParseFailure);
    EXPECT_EQ(odd.message, kNotACustomisation);
    EXPECT_NE(odd.context.find("UTF-16"), std::string::npos) << odd.describe();

    // A high surrogate (D83D, little-endian 3D D8) with no low one after it,
    // as the file's last unit.
    const Error half = refusalOf(*utf16 + std::string("\x3D\xD8", 2));
    EXPECT_EQ(half.code, ErrorCode::ParseFailure);
    EXPECT_EQ(half.message, kNotACustomisation);
    EXPECT_NE(half.context.find("UTF-16"), std::string::npos) << half.describe();

    // The same goes for one definition's text.
    const auto definition = katana::entity::definitionFromJson("\xEF\xBB\xBF{\"name\": \"Caf\xE9\"}",
                                                               false);
    ASSERT_FALSE(definition.ok());
    EXPECT_EQ(definition.error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(definition.error().message, "not a definition in the Katana customisation format");
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

TEST(CustomisationRefusals, ANameThatCouldNotBeToldFromAnotherOrSeenIsRefused)
{
    // A name is an identity: a project records it and is matched against it.
    // A blank at either end makes two names that look like one; a tab, an
    // escape (U+001B), a NUL and a delete (U+007F) are characters nobody can
    // see in a list. Each is written here as JSON escapes it.
    for (const char* name :
         {"NSW ", " NSW", " ", "a\\tb", "a\\u001b[31mb", "a\\u0000b", "a\\u007f"}) {
        const std::string text =
            std::string(R"({"format": "katana-customisation", "version": 1, "name": ")") + name +
            "\"}";
        const Error error = refusalOf(text);
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument) << text;
        expectNames(error, "top level", "name");
    }
    // A blank INSIDE a name is part of it.
    EXPECT_EQ(readCustomisation(
                  R"({"format": "katana-customisation", "version": 1, "name": "Site set 2026"})")
                  .name,
              "Site set 2026");
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
            // a blank at an end, and a character that cannot be seen
            {R"("sources": [{"name": "Site "}])", R"(sources[0] "Site ")", "name"},
            {R"("basedOn": {"name": " NSW", "digest": "0123456789abcdef"})", "basedOn", "name"},
            {R"("linestyles": [{"name": "A", "from": "a\tb"}])", R"(linestyles[0] "A")", "from"},
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
                      {"key": "WM*", "sets": "feature", "linesytle": "TEST Water Main"}])",
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
        R"("codes": [{"key": "WM*", "sets": "feature", "model": "TEST SERVICES"}])"));
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
            // Inside a stroke the stroke is as far as a place goes ("How an
            // entry is named"): the same `strokes[1]` an unknown member of
            // that object is refused under, above.
            {R"("linestyles": [{"name": "L", "strokes": [["move", 0, 0],
                                                        ["text", {"text": "a", "text": "b"}]]}])",
             R"(linestyles[0] "L" strokes[1])", "text"},
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
            // ... and one value too many is as wrong as one too few: a third
            // coordinate or a third anchor would otherwise be dropped unsaid
            {R"("linestyles": [{"name": "G", "origin": [1, 2, 3]}])", R"(linestyles[0] "G")",
             "origin"},
            {R"("linestyles": [{"name": "G", "anchors": [[0, 0], [1, 1], [2, 2]]}])",
             R"(linestyles[0] "G")", "anchors"},
            {R"("linestyles": [{"name": "G", "anchors": [[0, 0, 0], [1, 1]]}])",
             R"(linestyles[0] "G")", "anchors"},
            {R"("linestyles": [{"name": "G", "anchors": []}])", R"(linestyles[0] "G")", "anchors"},
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
            // a fourth number is not three
            {testValveWith(R"(["text", {"extra": [0, 0, 0, 0]}])"),
             R"(symbols[0] "TEST Valve" strokes[9])", "extra"},
            {testValveWith(R"(["text", {"extra": 0}])"), R"(symbols[0] "TEST Valve" strokes[9])",
             "extra"},
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
    // Lower case, '_' and '-' as a blank, "gray" as "grey": spellings of one
    // name. The members of a JSON object have no order, so "the second of the
    // two" cannot be the file's second: the names are gone through in the
    // order of their bytes, the later of the two in THAT order is the entry
    // refused, and both spellings are beside it, the earlier first. By hand:
    // 'S' (53) sorts before 's' (73), ' ' (20) before '-' (2D), and 'O' (4F)
    // before 'o' (6F).
    struct Alike {
        const char* colours;
        const char* earlier;
        const char* refused;
    };
    for (const Alike& alike :
         {Alike{R"({"SUI Gas": "#FFFF00", "sui_gas": "#FF0000"})", "SUI Gas", "sui_gas"},
          // the same two the other way round in the file: the same answer
          Alike{R"({"sui_gas": "#FF0000", "SUI Gas": "#FFFF00"})", "SUI Gas", "sui_gas"},
          Alike{R"({"sui gas": "#FFFF00", "sui-gas": "#FF0000"})", "sui gas", "sui-gas"},
          Alike{R"({"off gray": "#808080", "Off Grey": "#808081"})", "Off Grey", "off gray"}}) {
        const Error error =
            refusalOf(customisationWith(std::string("\"colours\": ") + alike.colours));
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument) << alike.colours;
        EXPECT_EQ(error.message.rfind(std::string("colours \"") + alike.refused + "\": ", 0), 0u)
            << error.describe();
        EXPECT_EQ(error.context,
                  std::string("\"") + alike.earlier + "\" and \"" + alike.refused + "\"")
            << error.describe();
    }
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
    // with a blank in it could never be one token at all. Which controls are
    // at fault is beside the refusal, in the words entity::validate says it
    // (test_linework_codes.cpp): the two that clash, the earlier first, with
    // the earlier's spelling - "join" given the default spelling of "end"
    // clashes with "end" - or the one control and its spelling.
    struct Clash {
        const char* linework;
        const char* context;
    };
    for (const Clash& clash : {Clash{R"({"start": "ST", "end": "st"})", "start and end are \"ST\""},
                               Clash{R"({"close": "C L"})", "close=\"C L\""},
                               Clash{R"({"join": "END"})", "end and join are \"END\""}}) {
        const Error error =
            refusalOf(customisationWith(std::string("\"linework\": ") + clash.linework));
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument) << clash.linework;
        EXPECT_EQ(error.message.rfind("linework: ", 0), 0u) << error.describe();
        EXPECT_EQ(error.context, clash.context) << error.describe();
    }
}

TEST(CustomisationRefusals, ANegativeSizeIsRefusedAsNegativeAndNotAsNotFinite)
{
    // 0 is "the definition's own size" for a symbol and "not said" for a
    // text; below it there is no size to draw at. But -1 is a finite number,
    // and the refusal must say what is wrong with it, not something else.
    for (const char* rule :
         {R"({"key": "A", "sets": "symbol", "symbol": {"name": "P", "size": -1}})",
          R"({"key": "A", "sets": "text", "text": {"style": "ISO", "size": -0.5}})"}) {
        const Error error = refusalOf(customisationWith(std::string("\"codes\": [") + rule + "]"));
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument) << rule;
        EXPECT_EQ(error.message.rfind(R"(codes[0] "A": )", 0), 0u) << error.describe();
        EXPECT_NE(error.message.find("size cannot be negative"), std::string::npos)
            << error.describe();
        EXPECT_EQ(error.message.find("finite"), std::string::npos) << error.describe();
    }

    // The model's own check says the two apart as well: a size that is not a
    // number at all is "not finite", one below zero is "negative".
    SurveyRule symbol;
    symbol.key = "A";
    symbol.symbol = katana::entity::SurveySymbol{};
    symbol.symbol->size = std::numeric_limits<double>::quiet_NaN();
    auto status = katana::entity::validate(symbol);
    ASSERT_FALSE(status.ok());
    EXPECT_NE(status.error().message.find("finite"), std::string::npos) << status.error().describe();
    symbol.symbol->size = -1.0;
    status = katana::entity::validate(symbol);
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().message, "a symbol's size cannot be negative");
    SurveyRule text;
    text.key = "A";
    text.textStyle = katana::entity::SurveyTextStyle{};
    text.textStyle->size = -std::numeric_limits<double>::infinity();
    status = katana::entity::validate(text);
    ASSERT_FALSE(status.ok());
    EXPECT_NE(status.error().message.find("finite"), std::string::npos) << status.error().describe();
    text.textStyle->size = -0.5;
    status = katana::entity::validate(text);
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().message, "a text style's size cannot be negative");

    // Every other number of the two may be below zero, and negative zero is
    // not below it.
    const Customisation read = readCustomisation(customisationWith(
        R"("codes": [{"key": "A", "sets": "symbol",
                      "symbol": {"size": -0.0, "rotation": -45, "offset": -0.2, "raise": -0.1},
                      "text": {"size": -0.0, "offset": -0.5, "raise": -0.25, "angle": -30,
                               "slant": -10}}])"));
    ASSERT_EQ(read.map.size(), 1u);
    EXPECT_EQ(read.map.rules()[0].symbol->rotation, -45.0);
    EXPECT_EQ(read.map.rules()[0].symbol->raise, -0.1);
    EXPECT_EQ(read.map.rules()[0].textStyle->angle, -30.0);
    EXPECT_EQ(read.map.rules()[0].textStyle->slant, -10.0);
}

// ---- numbers a double cannot hold -----------------------------------------------------------

namespace {

constexpr const char* kTooLarge = "has a number too large to hold";
constexpr const char* kTooSmall =
    "has a number too small to hold: it is not zero, and would be read as 0";

} // namespace

// 1e400 is JSON, and past the largest double (about 1.8e308). The JSON library
// stops at such a number; what the reader then says is what it would say of
// any value of the wrong kind - the entry and the member - with the number and
// where it stands beside it.
TEST(CustomisationRefusals, ANumberTooLargeToHoldIsRefusedNamingTheEntryTheMemberAndThePlace)
{
    // The place is worked out by hand. Line 2 is
    //    ` "linestyles": [{"name": "A", "length": 1e400}]}`
    // one blank, then "linestyles" in its quotes (12 characters, columns 2 to
    // 13), `: [{` (14 to 17), "name" (18 to 23), `: ` (24, 25), "A" (26 to
    // 28), `, ` (29, 30), "length" (31 to 38), `: ` (39, 40), and 1e400 in
    // columns 41 to 45. The column given is that of the number's LAST
    // character, which is where a JSON reader stands once it has read it.
    const Error error =
        refusalOf("{\"format\": \"katana-customisation\", \"version\": 1, \"name\": \"T\",\n"
                  " \"linestyles\": [{\"name\": \"A\", \"length\": 1e400}]}");
    EXPECT_EQ(error.code, ErrorCode::ParseFailure);
    EXPECT_EQ(error.message, std::string(R"(linestyles[0] "A": "length" )") + kTooLarge);
    EXPECT_EQ(error.context, "1e400 at line 2, column 45");

    struct Large {
        std::string members;
        std::string message;
        std::string number;
    };
    const std::vector<Large> cases = {
        // below the least as well as above the greatest
        {R"("linestyles": [{"name": "A", "factor": -1e400}])",
         std::string(R"(linestyles[0] "A": "factor" )") + kTooLarge, "-1e400"},
        // just past the largest double, 1.7976931348623157e308
        {R"("linestyles": [{"name": "A", "length": 2e308}])",
         std::string(R"(linestyles[0] "A": "length" )") + kTooLarge, "2e308"},
        // in a list under a member: the member is still what is named
        {R"("linestyles": [{"name": "A", "origin": [0, 1e400]}])",
         std::string(R"(linestyles[0] "A": "origin" )") + kTooLarge, "1e400"},
        {R"("linestyles": [{"name": "A", "anchors": [[0, 0], [1e999, 0]]}])",
         std::string(R"(linestyles[0] "A": "anchors" )") + kTooLarge, "1e999"},
        // in a stroke, and in the object and the list a text stroke holds
        {testValveWith(R"(["move", 1e400, 0])"),
         std::string(R"(symbols[0] "TEST Valve" strokes[9]: this stroke )") + kTooLarge, "1e400"},
        {testValveWith(R"(["text", {"height": 1E+400}])"),
         std::string(R"(symbols[0] "TEST Valve" strokes[9]: "height" )") + kTooLarge, "1E+400"},
        {testValveWith(R"(["text", {"extra": [0, 1e400, 0]}])"),
         std::string(R"(symbols[0] "TEST Valve" strokes[9]: "extra" )") + kTooLarge, "1e400"},
        // in a part of a rule
        {R"("codes": [{"key": "A", "sets": "symbol", "symbol": {"name": "P", "size": 1e400}}])",
         std::string(R"(codes[0] "A" symbol: "size" )") + kTooLarge, "1e400"},
        {R"("codes": [{"key": "A", "sets": "text", "text": {"slant": 1e400}}])",
         std::string(R"(codes[0] "A" text: "slant" )") + kTooLarge, "1e400"},
        // where no number belongs at all, it is still the number that stopped the reading
        {R"("description": 1e400)", std::string(R"(top level: "description" )") + kTooLarge,
         "1e400"},
        // an entry whose name comes after the number has no name to show yet
        {R"("linestyles": [{"length": 1e400, "name": "A"}])",
         std::string(R"(linestyles[0]: "length" )") + kTooLarge, "1e400"},
        // bare among the strokes, it is in the list that "strokes" is
        {testValveWith("1e400"),
         std::string(R"(symbols[0] "TEST Valve": "strokes" )") + kTooLarge, "1e400"},
        // in an entry of a list inside a rule, and in the file's own objects
        {R"("codes": [{"key": "A", "sets": "attributes", "attributes": [{"type": "text", "name": "N"}, {"type": "text", "name": "M", "value": 1e400}]}])",
         std::string(R"(codes[0] "A" attributes[1]: "value" )") + kTooLarge, "1e400"},
        {R"("linework": {"start": "S", "end": 1e400})",
         std::string(R"(linework: "end" )") + kTooLarge, "1e400"},
        {R"("colours": {"sui gas": 1e400})", std::string(R"(colours: "sui gas" )") + kTooLarge,
         "1e400"},
        {R"("sources": [{"name": "A"}, {"name": "B", "rules": 1e400}])",
         std::string(R"(sources[1] "B": "rules" )") + kTooLarge, "1e400"},
    };
    for (const Large& large : cases) {
        const Error refused = refusalOf(customisationWith(large.members));
        EXPECT_EQ(refused.code, ErrorCode::ParseFailure) << large.members;
        EXPECT_EQ(refused.message, large.message) << large.members;
        EXPECT_EQ(refused.context.rfind(large.number + " at line 1, column ", 0), 0u)
            << refused.describe();
    }

    // The largest double itself, and its negative, are numbers the JSON library
    // holds, and so are no number "too large to hold"; they are past the
    // format's own bound (customisation.hpp, kCustomisationMostMagnitude), and
    // refused for that, by the member (test_customisation_bounds.cpp tries the
    // bound from both sides).
    const Error length = refusalOf(customisationWith(
        R"("linestyles": [{"name": "A", "length": 1.7976931348623157e308}])"));
    EXPECT_EQ(length.code, ErrorCode::ParseFailure);
    EXPECT_EQ(length.message,
              R"(linestyles[0] "A": "length" is larger than 1000000000 in size, the most any )"
              "number of a customisation holds");
    const Error origin = refusalOf(customisationWith(
        R"("linestyles": [{"name": "A", "origin": [-1.7976931348623157e308, 0]}])"));
    EXPECT_EQ(origin.message,
              R"(linestyles[0] "A": "origin" is larger than 1000000000 in size, the most any )"
              "number of a customisation holds");
}

// The reading stops AT such a number, so what a file is told depends on what
// it had said by then. The writer puts "format" and "version" first, and so
// does anyone who starts from a file it wrote.
TEST(CustomisationRefusals, ANumberTooLargeBeforeTheFileSaysWhatItIsIsNotAKatanaCustomisationFile)
{
    for (const char* text :
         {// the same file with its members the other way round
          R"({"linestyles": [{"name": "A", "length": 1e400}], "format": "katana-customisation", "version": 1, "name": "T"})",
          // JSON of some other kind
          R"({"type": "FeatureCollection", "bbox": [0, 0, 1e400, 1]})",
          R"({"format": "katana-sheets", "version": 1, "scale": 1e400})",
          // not an object at all
          "1e400", "[1e400]", "[[0, -1e400]]"}) {
        const Error error = refusalOf(text);
        EXPECT_EQ(error.code, ErrorCode::ParseFailure) << text;
        EXPECT_EQ(error.message, kNotACustomisation) << text;
        EXPECT_NE(error.context.find("1e400 at line 1, column "), std::string::npos)
            << error.describe();
    }

    // A file that has said it is NEWER is told that, as for anything else a
    // newer file may hold.
    const Error newer = refusalOf(
        R"({"format": "katana-customisation", "version": 2, "name": "T", "limit": 1e400})");
    EXPECT_EQ(newer.code, ErrorCode::Unsupported);
    EXPECT_NE(newer.message.find("newer version of Katana"), std::string::npos) << newer.describe();
}

// One definition's text has no "format" to say first: it is a definition as
// soon as it is an object, and the number is refused by its member, or by its
// stroke, under the definition's own name.
TEST(CustomisationRefusals, ANumberTooLargeInADefinitionsTextIsRefusedTheSameWay)
{
    using katana::entity::definitionFromJson;
    const auto member = definitionFromJson(R"({"name": "A", "length": 1e400})", false);
    ASSERT_FALSE(member.ok());
    EXPECT_EQ(member.error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(member.error().message, std::string(R"(definition "A": "length" )") + kTooLarge);
    // {"name": "A", "length": 1e400} - by hand, the number's last character
    // is in column 29: `{"name": "A", ` is 14 characters, `"length": ` 10
    // more, and the number the next 5.
    EXPECT_EQ(member.error().context, "1e400 at line 1, column 29");

    const auto stroke = definitionFromJson(
        R"({"name": "A", "strokes": [["move", 0, 0], ["circle", -1e400]]})", true);
    ASSERT_FALSE(stroke.ok());
    EXPECT_EQ(stroke.error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(stroke.error().message,
              std::string(R"(definition "A" strokes[1]: this stroke )") + kTooLarge);
    EXPECT_EQ(stroke.error().context.rfind("-1e400 at line 1, column ", 0), 0u)
        << stroke.error().describe();

    const auto text = definitionFromJson(
        R"({"name": "A", "strokes": [["text", {"extra": [0, 0, 1e400]}]]})", true);
    ASSERT_FALSE(text.ok());
    EXPECT_EQ(text.error().message,
              std::string(R"(definition "A" strokes[0]: "extra" )") + kTooLarge);

    // Before the name is read there is none to show; and a text that is not
    // an object is not a definition at all.
    const auto unnamed = definitionFromJson(R"({"factor": 1e400, "name": "A"})", false);
    ASSERT_FALSE(unnamed.ok());
    EXPECT_EQ(unnamed.error().message, std::string(R"(definition: "factor" )") + kTooLarge);
    for (const char* notOne : {"1e400", "[1e400]"}) {
        const auto refused = definitionFromJson(notOne, false);
        ASSERT_FALSE(refused.ok()) << notOne;
        EXPECT_EQ(refused.error().code, ErrorCode::ParseFailure) << notOne;
        EXPECT_EQ(refused.error().message, "not a definition in the Katana customisation format")
            << notOne;
        EXPECT_NE(refused.error().context.find("1e400 at line 1, column "), std::string::npos)
            << refused.error().describe();
    }
}

// 1e-400 is below the smallest double (about 4.9e-324). The JSON library
// reads it as 0 and says nothing - and 0 is "not said" for a length, "the
// definition's own" for a size, and refused for a factor in words that blame
// a zero nobody wrote.
TEST(CustomisationRefusals, ANumberTooSmallToHoldIsRefusedRatherThanReadAsZero)
{
    struct Small {
        std::string members;
        std::string message;
        std::string context; // the number, or the stroke, as the file wrote it
    };
    const std::string zeros(400, '0'); // 0.000...0001, written without an exponent
    const std::vector<Small> cases = {
        {R"("linestyles": [{"name": "A", "length": 1e-400}])",
         std::string(R"(linestyles[0] "A": "length" )") + kTooSmall, "1e-400"},
        {R"("linestyles": [{"name": "A", "factor": 1E-400}])",
         std::string(R"(linestyles[0] "A": "factor" )") + kTooSmall, "1E-400"},
        {R"("linestyles": [{"name": "A", "origin": [-1e-400, 0]}])",
         std::string(R"(linestyles[0] "A": "origin" )") + kTooSmall, "[-1e-400,0]"},
        {R"("linestyles": [{"name": "A", "anchors": [[0, 0], [0, 2.5e-999]]}])",
         std::string(R"(linestyles[0] "A": "anchors" )") + kTooSmall, "[0,2.5e-999]"},
        {testValveWith(R"(["move", 1e-400, 0])"),
         std::string(R"(symbols[0] "TEST Valve" strokes[9]: this stroke )") + kTooSmall,
         R"(["move",1e-400,0])"},
        {testValveWith(R"(["arc", 1, 0, -1e-400])"),
         std::string(R"(symbols[0] "TEST Valve" strokes[9]: this stroke )") + kTooSmall,
         R"(["arc",1,0,-1e-400])"},
        {testValveWith(R"(["text", {"angle": 1e-400}])"),
         std::string(R"(symbols[0] "TEST Valve" strokes[9]: "angle" )") + kTooSmall, "1e-400"},
        {testValveWith(R"(["text", {"extra": [0, 0, 1e-999]}])"),
         std::string(R"(symbols[0] "TEST Valve" strokes[9]: "extra" )") + kTooSmall,
         "[0,0,1e-999]"},
        {R"("codes": [{"key": "A", "sets": "symbol", "symbol": {"rotation": 0.)" + zeros + "1}}]",
         std::string(R"(codes[0] "A" symbol: "rotation" )") + kTooSmall, "0." + zeros + "1"},
        {R"("codes": [{"key": "A", "sets": "text", "text": {"size": 1e-400}}])",
         std::string(R"(codes[0] "A" text: "size" )") + kTooSmall, "1e-400"},
    };
    for (const Small& small : cases) {
        const Error error = refusalOf(customisationWith(small.members));
        EXPECT_EQ(error.code, ErrorCode::ParseFailure) << small.members;
        EXPECT_EQ(error.message, small.message) << small.members;
        // What is shown of a value is cut at 60 characters.
        const std::string shown = small.context.size() > 60 ? small.context.substr(0, 60) + "..."
                                                            : small.context;
        EXPECT_EQ(error.context, shown) << small.members;
    }

    // Where no number belongs, or only a whole one, it is refused as any
    // other value there is - and shown as the file wrote it, not as 0.
    const Error named = refusalOf(customisationWith(R"("description": 1e-400)"));
    EXPECT_EQ(named.code, ErrorCode::ParseFailure);
    EXPECT_EQ(named.message, R"(top level: "description" must be text, in double quotes)");
    EXPECT_EQ(named.context, "1e-400");
    const Error whole =
        refusalOf(customisationWith(R"("linestyles": [{"name": "A", "cycleMode": 1e-400}])"));
    EXPECT_EQ(whole.code, ErrorCode::ParseFailure);
    EXPECT_EQ(whole.message, R"(linestyles[0] "A": "cycleMode" must be a whole number)");
    EXPECT_EQ(whole.context, "1e-400");
    // ... and where a list of strokes, or one stroke of it, belongs.
    const Error strokes =
        refusalOf(customisationWith(R"("linestyles": [{"name": "A", "strokes": 1e-400}])"));
    EXPECT_EQ(strokes.code, ErrorCode::ParseFailure);
    EXPECT_EQ(strokes.message, R"(linestyles[0] "A": "strokes" must be a list, written [...])");
    EXPECT_EQ(strokes.context, "1e-400");
    const Error stroke = refusalOf(customisationWith(testValveWith("1e-400")));
    EXPECT_EQ(stroke.code, ErrorCode::ParseFailure);
    EXPECT_EQ(stroke.message.rfind(R"(symbols[0] "TEST Valve" strokes[9]: a stroke is a list)", 0),
              0u)
        << stroke.describe();
    EXPECT_EQ(stroke.context, "1e-400");
    const Error pen = refusalOf(customisationWith(testValveWith(R"(["pen", 1e-400])")));
    EXPECT_EQ(pen.code, ErrorCode::ParseFailure);
    expectNames(pen, R"(symbols[0] "TEST Valve" strokes[9])", "pen");
    EXPECT_EQ(pen.context, R"(["pen",1e-400])");

    // One definition's text is read by the same code.
    const auto definition =
        katana::entity::definitionFromJson(R"({"name": "A", "factor": 1e-400})", false);
    ASSERT_FALSE(definition.ok());
    EXPECT_EQ(definition.error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(definition.error().message, std::string(R"(definition "A": "factor" )") + kTooSmall);
}

TEST(CustomisationRefusals, ZeroWrittenAtLengthIsZeroAndTheSmallestNumbersADoubleHoldsAreRead)
{
    // Every digit of each of these is 0, whatever its exponent says: they
    // ARE zero, and the sign of the last two is kept.
    const Customisation zeros = readCustomisation(customisationWith(
        R"("linestyles": [{"name": "A", "length": 0e-400, "origin": [0.000, 0E5],
                           "anchors": [[-0.0e-400, 0.0e999], [-0e-999, 0]]}])"));
    const LineStyle* a = zeros.library.find("A");
    ASSERT_NE(a, nullptr);
    EXPECT_EQ(a->length, 0.0);
    EXPECT_EQ(a->origin.x, 0.0);
    EXPECT_EQ(a->origin.y, 0.0);
    EXPECT_EQ(a->anchor1.x, 0.0);
    EXPECT_TRUE(std::signbit(a->anchor1.x));
    EXPECT_EQ(a->anchor1.y, 0.0);
    EXPECT_FALSE(std::signbit(a->anchor1.y));
    EXPECT_TRUE(std::signbit(a->anchor2.x));

    // 5e-324 is the smallest number a double holds (2^-1074) and 1e-320 one
    // of the few thousand just above it, held to fewer digits: neither is
    // zero, and each is read as the nearest double there is.
    const Customisation tiny = readCustomisation(customisationWith(
        R"("linestyles": [{"name": "A", "length": 5e-324,
                           "strokes": [["move", 1e-320, -5e-324]]}])"));
    const LineStyle* b = tiny.library.find("A");
    ASSERT_NE(b, nullptr);
    EXPECT_EQ(b->length, std::numeric_limits<double>::denorm_min());
    ASSERT_EQ(b->strokes.size(), 1u);
    EXPECT_GT(b->strokes[0].point.x, 0.0);
    EXPECT_LT(b->strokes[0].point.x, 1.1e-320);
    EXPECT_EQ(b->strokes[0].point.y, -std::numeric_limits<double>::denorm_min());
}

// ---- values nested deep ---------------------------------------------------------------------

namespace {

// A value may nest as deep as its text is long: 100,000 "[" are a file of
// 200 KB. Far past where anything that goes down a level of the program's
// stack for a level of the value runs out of stack.
constexpr std::size_t kDeep = 100000;

// `kDeep` lists one inside the other, the innermost empty: [[[...]]].
std::string deepLists()
{
    return std::string(kDeep, '[') + std::string(kDeep, ']');
}

// `kDeep` objects one inside the other: {"a":{"a":...{"a":0}...}}.
std::string deepObjects()
{
    std::string text;
    text.reserve(kDeep * 6 + 1);
    for (std::size_t i = 0; i < kDeep; ++i) {
        text += "{\"a\":";
    }
    text += '0';
    text.append(kDeep, '}');
    return text;
}

} // namespace

TEST(CustomisationRefusals, AValueNestedAHundredThousandDeepIsRefusedLikeAnyOtherOfTheWrongKind)
{
    const std::string lists = deepLists();
    const std::string objects = deepObjects();

    // Where the file has not said it is a customisation: its "format" is the
    // deep value, or the text itself is.
    for (const std::string& text : {"{\"format\": " + lists + "}", "{\"format\": " + objects + "}",
                                    lists, objects}) {
        const Error error = refusalOf(text);
        EXPECT_EQ(error.code, ErrorCode::ParseFailure);
        EXPECT_EQ(error.message, kNotACustomisation);
    }

    // Anywhere else it is a value of the wrong kind, refused by the entry and
    // the member exactly as a shallow one is.
    const std::vector<Refused> cases = {
        {"\"description\": " + objects, "top level", "description"},
        {"\"sources\": " + objects, "top level", "sources"},
        {"\"linestyles\": [{\"name\": \"A\", \"group\": " + lists + "}]", R"(linestyles[0] "A")",
         "group"},
        {"\"linestyles\": [{\"name\": \"A\", \"origin\": " + lists + "}]", R"(linestyles[0] "A")",
         "origin"},
        {"\"linestyles\": [{\"name\": \"A\", \"strokes\": " + objects + "}]", R"(linestyles[0] "A")",
         "strokes"},
        {"\"symbols\": [{\"name\": \"A\", \"strokes\": [[\"move\", " + lists + ", 0]]}]",
         R"(symbols[0] "A" strokes[0])", "move"},
        {"\"symbols\": [{\"name\": \"A\", \"strokes\": [[\"pen\", " + objects + "]]}]",
         R"(symbols[0] "A" strokes[0])", "pen"},
        {"\"symbols\": [{\"name\": \"A\", \"strokes\": [[\"text\", {\"height\": " + lists + "}]]}]",
         R"(symbols[0] "A" strokes[0])", "height"},
        {"\"symbols\": [{\"name\": \"A\", \"strokes\": [[\"text\", {\"extra\": [0, 0, " + lists +
             "]}]]}]",
         R"(symbols[0] "A" strokes[0])", "extra"},
        {"\"codes\": [{\"key\": \"A\", \"sets\": \"feature\", \"layer\": " + lists + "}]",
         R"(codes[0] "A")", "layer"},
        {"\"codes\": [{\"key\": \"A\", \"sets\": " + objects + "}]", R"(codes[0] "A")", "sets"},
        {"\"codes\": [{\"key\": \"A\", \"sets\": \"attributes\", \"attributes\": [{\"type\": "
         "\"text\", \"name\": " +
             lists + "}]}]",
         R"(codes[0] "A" attributes[0])", "name"},
        {"\"zzz\": " + lists, "top level", "zzz"},
        // the first of two is thrown away as the second is put in
        {"\"description\": " + lists + ", \"description\": " + objects, "top level", "description"},
    };
    for (const Refused& refused : cases) {
        const Error error = refusalOf(customisationWith(refused.members));
        EXPECT_EQ(error.code, ErrorCode::ParseFailure) << refused.entry << " " << refused.member;
        expectNames(error, refused.entry, refused.member);
        // What a refusal shows of a value is cut short however much there is.
        EXPECT_LE(error.context.size(), 240u) << refused.entry << " " << refused.member;
    }

    // The file's own name, and what is shown of a value: its first 60
    // characters.
    const Error named = refusalOf(R"({"format": "katana-customisation", "version": 1, "name": )" +
                                  lists + "}");
    EXPECT_EQ(named.code, ErrorCode::ParseFailure);
    EXPECT_EQ(named.message, R"(top level: "name" must be text, in double quotes)");
    EXPECT_EQ(named.context, std::string(60, '[') + "...");
    const Error described = refusalOf(customisationWith("\"description\": " + objects));
    // {"a": is five characters, so twelve of them are the first sixty.
    std::string twelve;
    for (int i = 0; i < 12; ++i) {
        twelve += "{\"a\":";
    }
    EXPECT_EQ(described.context, twelve + "...");

    // And where an object or a stroke should be.
    for (const auto& [members, entry] : std::vector<std::pair<std::string, std::string>>{
             {"\"linestyles\": [" + lists + "]", "linestyles[0]"},
             {"\"codes\": [{\"key\": \"A\", \"sets\": \"symbol\", \"symbol\": " + lists + "}]",
              R"(codes[0] "A" symbol)"},
             {"\"basedOn\": " + lists, "basedOn"}}) {
        const Error error = refusalOf(customisationWith(members));
        EXPECT_EQ(error.code, ErrorCode::ParseFailure) << entry;
        EXPECT_EQ(error.message, entry + ": must be an object, written {...}");
    }
    for (const std::string& stroke : {lists, objects}) {
        const Error error = refusalOf(
            customisationWith("\"symbols\": [{\"name\": \"A\", \"strokes\": [[\"move\", 0, 0], " +
                              stroke + "]}]"));
        EXPECT_EQ(error.code, ErrorCode::ParseFailure);
        EXPECT_EQ(error.message.rfind(R"(symbols[0] "A" strokes[1]: )", 0), 0u) << error.message;
    }
    const Error colour = refusalOf(customisationWith("\"colours\": {\"sui gas\": " + lists + "}"));
    EXPECT_EQ(colour.code, ErrorCode::ParseFailure);
    EXPECT_EQ(colour.message.rfind(R"(colours "sui gas": )", 0), 0u) << colour.message;
}

TEST(CustomisationRefusals, ADefinitionsTextNestedAHundredThousandDeepIsRefusedTheSameWay)
{
    const std::string lists = deepLists();
    const auto notOne = katana::entity::definitionFromJson(lists, false);
    ASSERT_FALSE(notOne.ok());
    EXPECT_EQ(notOne.error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(notOne.error().message, "not a definition in the Katana customisation format");

    const auto member =
        katana::entity::definitionFromJson("{\"name\": \"A\", \"group\": " + lists + "}", false);
    ASSERT_FALSE(member.ok());
    EXPECT_EQ(member.error().code, ErrorCode::ParseFailure);
    expectNames(member.error(), R"(definition "A")", "group");

    const auto stroke = katana::entity::definitionFromJson(
        "{\"name\": \"A\", \"strokes\": [[\"circle\", " + deepObjects() + "]]}", false);
    ASSERT_FALSE(stroke.ok());
    EXPECT_EQ(stroke.error().code, ErrorCode::ParseFailure);
    expectNames(stroke.error(), R"(definition "A" strokes[0])", "circle");
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
    arc.endAngle = 0.5; // 719.5 degrees back: within the two turns an arc may sweep
    Stroke dot = strokeOf(StrokeOp::Dot); // radius 0: the smallest drawable
    Stroke pen = strokeOf(StrokeOp::Pen); // an empty pen name is still a pen
    Stroke text = strokeOf(StrokeOp::Text);
    text.text = 0;
    definition.strokes = {arc, dot, pen, text};
    definition.texts = {StrokeText{"", 0, 0, "", "", 1, {1e-300, -1e9, 0.1}}};
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
