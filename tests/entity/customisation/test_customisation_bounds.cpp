// The bounds a Katana customisation file is held to (customisation.hpp,
// kCustomisationMost*): the reader refuses a file past them, naming the entry
// and the member as it names every other refusal, and the writer refuses what
// would read back as one of those.
//
// The expectations are written by hand from the bounds - a number's size at
// most 1000000000, an arc's sweep at most 720 degrees, 100000 strokes to a
// definition, 1000 bytes to a name or a text, 1000000 rules to a file - not
// read off a run. Each bound is tried exactly AT it (accepted) and one past it
// (refused).

#include <gtest/gtest.h>

#include <cstddef>
#include <string>
#include <string_view>

#include "customisation_fixture.hpp"
#include "katana/entity/customisation.hpp"

using katana::core::Error;
using katana::core::ErrorCode;
using katana::entity::Customisation;
using katana::entity::CustomisationWriteOptions;
using katana::entity::LineStyle;
using katana::entity::Stroke;
using katana::entity::StrokeOp;
using katana::testing::customisationWith;
using katana::testing::readCustomisation;
using katana::testing::refusalOf;

namespace {

// What every refusal of a number past the bound says after its member.
constexpr const char* kLarger =
    "larger than 1000000000 in size, the most any number of a customisation holds";
constexpr const char* kLong =
    "longer than 1000 bytes, the most a name or text of a customisation holds";
constexpr const char* kSweeps = "sweeps more than 720 degrees (two turns), the most an arc holds";

// A customisation whose one symbol, S, has these strokes (written as the file
// writes them, comma separated).
std::string symbolWith(const std::string& strokes)
{
    return R"("symbols": [{"name": "S", "atVertices": true, "strokes": [)" + strokes + "]}]";
}

// A customisation whose one symbol, S, has these members besides its name.
std::string symbolMembers(const std::string& members)
{
    return R"("symbols": [{"name": "S", )" + members + "}]";
}

std::string repeated(std::string_view piece, std::size_t times)
{
    std::string text;
    text.reserve(piece.size() * times + times);
    for (std::size_t i = 0; i < times; ++i) {
        text += i == 0 ? "" : ",";
        text += piece;
    }
    return text;
}

bool startsWith(const std::string& text, std::string_view start)
{
    return text.compare(0, start.size(), start) == 0;
}

bool endsWith(const std::string& text, std::string_view end)
{
    return text.size() >= end.size() && text.compare(text.size() - end.size(), end.size(), end) == 0;
}

} // namespace

// ---- an arc's sweep ----------------------------------------------------------------------

TEST(CustomisationBounds, AnArcMaySweepTwoTurnsEitherWayRoundAndNoMore)
{
    // 720 exactly, forwards, backwards and about zero: two full turns each.
    for (const char* arc : {R"(["arc", 1, 0, 720])", R"(["arc", 1, 720, 0])",
                            R"(["arc", 1, -360, 360])"}) {
        const Customisation read = readCustomisation(customisationWith(symbolWith(arc)));
        EXPECT_EQ(read.library.size(), 1u) << arc;
    }
    // A hair past it, in each direction, is refused naming the stroke.
    for (const char* arc : {R"(["arc", 1, 0, 720.5])", R"(["arc", 1, 720.5, 0])",
                            R"(["arc", 1, -1e9, 1e9])"}) {
        const Error error = refusalOf(customisationWith(symbolWith(arc)));
        EXPECT_EQ(error.code, ErrorCode::ParseFailure) << arc;
        EXPECT_EQ(error.message, std::string(R"(symbols[0] "S" strokes[0]: "arc" )") + kSweeps)
            << arc;
        EXPECT_NE(error.context.find("arc"), std::string::npos) << error.context;
    }
}

// ---- a number's size ---------------------------------------------------------------------

TEST(CustomisationBounds, AStrokesNumberMayBeABillionInSizeAndNoMore)
{
    for (const char* stroke : {R"(["move", 1000000000, -1000000000])", R"(["circle", 1000000000])",
                               R"(["dot", -1000000000])"}) {
        EXPECT_EQ(readCustomisation(customisationWith(symbolWith(stroke))).library.size(), 1u)
            << stroke;
    }
    const struct {
        const char* stroke;
        const char* word;
    } past[] = {{R"(["move", 1000000001, 0])", "move"},
                {R"(["draw", 0, -1.2e29])", "draw"},
                {R"(["circle", 1e10])", "circle"},
                {R"(["dot", -2e9])", "dot"},
                // The review's own: a sweep of 1e308 degrees is a number past
                // the bound before it is a sweep past it.
                {R"(["arc", 1, 0, 1e308])", "arc"},
                {R"(["arc", 1e12, 0, 90])", "arc"}};
    for (const auto& each : past) {
        const Error error = refusalOf(customisationWith(symbolWith(each.stroke)));
        EXPECT_EQ(error.code, ErrorCode::ParseFailure) << each.stroke;
        EXPECT_EQ(error.message, std::string(R"(symbols[0] "S" strokes[0]: ")") + each.word +
                                     "\" has a number " + kLarger)
            << each.stroke;
    }
}

TEST(CustomisationBounds, ADefinitionsOwnNumbersAreHeldToTheSameBound)
{
    EXPECT_EQ(readCustomisation(customisationWith(
                                    symbolMembers(R"("length": 1000000000, "factor": 1000000000,
                                                    "origin": [1000000000, -1000000000],
                                                    "anchors": [[0, 0], [1000000000, 0]],
                                                    "strokes": [])")))
                  .library.size(),
              1u);
    const struct {
        const char* members;
        const char* member;
    } past[] = {{R"("length": 1e10)", "length"},
                {R"("factor": -1e10)", "factor"},
                {R"("origin": [0, 1e10])", "origin"},
                {R"("anchors": [[0, 0], [-1e10, 0]])", "anchors"}};
    for (const auto& each : past) {
        const Error error = refusalOf(customisationWith(symbolMembers(each.members)));
        EXPECT_EQ(error.code, ErrorCode::ParseFailure) << each.members;
        EXPECT_EQ(error.message, std::string(R"(symbols[0] "S": ")") + each.member + "\" is " +
                                     kLarger)
            << each.members;
    }
}

TEST(CustomisationBounds, ATextStrokesNumbersAndARulesAreHeldToItToo)
{
    const Error height = refusalOf(customisationWith(
        symbolWith(R"(["text", {"text": "N", "height": 1e10}])")));
    EXPECT_EQ(height.code, ErrorCode::ParseFailure);
    EXPECT_EQ(height.message,
              std::string(R"(symbols[0] "S" strokes[0]: "height" is )") + kLarger);

    const Error extra = refusalOf(customisationWith(
        symbolWith(R"(["text", {"text": "N", "extra": [0, 1e10, 0]}])")));
    EXPECT_EQ(extra.code, ErrorCode::ParseFailure);
    EXPECT_EQ(extra.message,
              std::string(R"(symbols[0] "S" strokes[0]: "extra" has a number )") + kLarger);

    // A rule's own: the size of its symbol.
    const Error size = refusalOf(customisationWith(
        R"("codes": [{"key": "A*", "sets": "symbol", "symbol": {"name": "X", "size": 1e10}}])"));
    EXPECT_EQ(size.code, ErrorCode::ParseFailure);
    EXPECT_TRUE(startsWith(size.message, R"(codes[0] "A*")")) << size.message;
    EXPECT_TRUE(endsWith(size.message, std::string(R"("size" is )") + kLarger)) << size.message;
    // And at the bound it reads.
    EXPECT_EQ(readCustomisation(customisationWith(
                                    R"("codes": [{"key": "A*", "sets": "symbol",
                                                  "symbol": {"name": "X", "size": 1000000000}}])"))
                  .map.size(),
              1u);
}

// ---- strokes in a definition -------------------------------------------------------------

TEST(CustomisationBounds, ADefinitionMayHoldAHundredThousandStrokesAndNotOneMore)
{
    const std::string stroke = R"(["move", 0, 0])";
    const Customisation atTheBound =
        readCustomisation(customisationWith(symbolWith(repeated(stroke, 100000))));
    EXPECT_EQ(atTheBound.library.find("S")->strokes.size(), 100000u);

    const Error error = refusalOf(customisationWith(symbolWith(repeated(stroke, 100001))));
    EXPECT_EQ(error.code, ErrorCode::ParseFailure);
    // The stroke that is one too many is the 100001st: counted from 0, 100000.
    EXPECT_EQ(error.message,
              R"(symbols[0] "S" strokes[100000]: a definition holds at most 100000 strokes, )"
              "and this is one more");
}

TEST(CustomisationBounds, ADefinitionReadOnItsOwnIsHeldToTheStrokeBoundToo)
{
    const std::string stroke = R"(["move", 0, 0])";
    const std::string text = R"({"name": "D", "strokes": [)" + repeated(stroke, 100001) + "]}";
    const auto read = katana::entity::definitionFromJson(text, true);
    ASSERT_FALSE(read.ok());
    EXPECT_EQ(read.error().message,
              R"(definition "D" strokes[100000]: a definition holds at most 100000 strokes, )"
              "and this is one more");
    // And a number past the bound, as a file says it.
    const auto beyond = katana::entity::definitionFromJson(
        R"({"name": "D", "length": 5e9, "strokes": []})", false);
    ASSERT_FALSE(beyond.ok());
    EXPECT_EQ(beyond.error().message, std::string(R"(definition "D": "length" is )") + kLarger);
}

// ---- names and texts ---------------------------------------------------------------------

TEST(CustomisationBounds, ANameOrATextMayBeAThousandBytesAndNoMore)
{
    const std::string fits(1000, 'A');
    const std::string past(1001, 'B');
    // At the bound, each place a text can sit: a definition's name, a rule's
    // key and comment, a pen, a text stroke's words, a colour's name and a
    // notice's line.
    {
        const std::string members = symbolWith(R"(["pen", ")" + fits + R"("])") + ", " +
                                    R"("codes": [{"key": ")" + fits +
                                    R"(", "sets": "feature", "comment": ")" + fits + R"("}], )" +
                                    R"("colours": {")" + fits + R"(": "#102030"}, )" +
                                    R"("notice": [")" + fits + R"("])";
        EXPECT_EQ(readCustomisation(customisationWith(members)).map.size(), 1u);
    }
    // One past it, each refused naming its entry (cut, as a name that long
    // must be to be of any use in a message) and its member.
    struct Case {
        std::string members;
        std::string entryStart;
        std::string endsWithText;
    };
    // A name is shown cut at 240 bytes, with "..." inside the quotes.
    const std::string bShown = std::string(240, 'B') + "...";
    const Case cases[] = {
        {R"("symbols": [{"name": ")" + past + R"(", "strokes": []}])", R"(symbols[0] ")" + bShown + "\"",
         std::string(R"("name" is )") + kLong},
        {R"("codes": [{"key": ")" + past + R"(", "sets": "feature"}])", R"(codes[0] ")" + bShown + "\"",
         std::string(R"("key" is )") + kLong},
        {R"("codes": [{"key": "K", "sets": "feature", "comment": ")" + past + R"("}])",
         R"(codes[0] "K")", std::string(R"("comment" is )") + kLong},
        {symbolWith(R"(["pen", ")" + past + R"("])"), R"(symbols[0] "S" strokes[0])",
         std::string(R"("pen" is )") + kLong},
        {symbolWith(R"(["text", {"text": ")" + past + R"("}])"), R"(symbols[0] "S" strokes[0])",
         std::string(R"("text" is )") + kLong},
        {R"("colours": {")" + past + R"(": "#102030"})", "colours",
         std::string("a colour's name is ") + kLong},
        {R"("notice": [")" + past + R"("])", "top level",
         std::string(R"("notice" holds a line that is )") + kLong},
    };
    for (const Case& each : cases) {
        const Error error = refusalOf(customisationWith(each.members));
        EXPECT_EQ(error.code, ErrorCode::ParseFailure) << each.entryStart;
        EXPECT_TRUE(startsWith(error.message, each.entryStart + ": "))
            << error.message.substr(0, 300);
        EXPECT_TRUE(endsWith(error.message, each.endsWithText)) << error.message.substr(0, 300);
        // Never the whole of the text: a refusal about a megabyte is not one.
        EXPECT_LT(error.message.size(), 600u) << each.entryStart;
    }
}

TEST(CustomisationBounds, AFilesOwnNameIsHeldToItToo)
{
    const std::string name(1001, 'N');
    const std::string text =
        R"({"format": "katana-customisation", "version": 1, "name": ")" + name + R"("})";
    const Error error = refusalOf(text);
    EXPECT_EQ(error.code, ErrorCode::ParseFailure);
    EXPECT_EQ(error.message, std::string(R"(top level: "name" is )") + kLong);
}

// ---- rules in a file ---------------------------------------------------------------------

TEST(CustomisationBounds, AFileMayHoldAMillionRulesAndNotOneMore)
{
    // `{}` is no rule (it has no key), so each file is refused - the first at
    // its first entry, which is how it is known that the bound did not fire at
    // exactly a million, and the second by the bound, before any is read.
    const std::string rule = "{}";
    const Error atTheBound =
        refusalOf(customisationWith(R"("codes": [)" + repeated(rule, 1000000) + "]"));
    EXPECT_EQ(atTheBound.message, R"(codes[0]: has no "key", which it must)");

    const Error past = refusalOf(customisationWith(R"("codes": [)" + repeated(rule, 1000001) + "]"));
    EXPECT_EQ(past.code, ErrorCode::ParseFailure);
    EXPECT_EQ(past.message,
              R"(top level: "codes" holds more than 1000000 rules, the most a customisation holds)");
}

// ---- the writer is held to the same, so that what it writes reads back --------------------

namespace {

Customisation holding(LineStyle definition)
{
    Customisation customisation;
    customisation.name = "W";
    definition.source = "W";
    const auto added = customisation.library.add(std::move(definition));
    EXPECT_TRUE(added.ok()) << (added.ok() ? std::string{} : added.error().describe());
    return customisation;
}

Error writeRefusal(const Customisation& customisation)
{
    const auto written = katana::entity::customisationToJson(customisation, {});
    EXPECT_FALSE(written.ok()) << "written without complaint";
    return written.ok() ? Error{} : written.error();
}

LineStyle named(std::string name)
{
    LineStyle style;
    style.name = std::move(name);
    return style;
}

} // namespace

TEST(CustomisationBounds, TheWriterRefusesANumberPastTheBound)
{
    LineStyle style = named("N");
    style.strokes = {Stroke{.op = StrokeOp::Draw, .point = {2.0e9, 0.0}}};
    const Error error = writeRefusal(holding(style));
    EXPECT_EQ(error.code, ErrorCode::InvalidArgument);
    EXPECT_EQ(error.message, std::string(R"(linestyles[0] "N" strokes[0]: "draw" is )") + kLarger);

    // And at the bound, a definition is written and reads back as itself.
    style.strokes = {Stroke{.op = StrokeOp::Draw, .point = {1.0e9, -1.0e9}}};
    const Customisation held = holding(style);
    const auto written = katana::entity::customisationToJson(held, {});
    ASSERT_TRUE(written.ok()) << written.error().describe();
    EXPECT_TRUE(readCustomisation(*written) == held);
}

TEST(CustomisationBounds, TheWriterRefusesAnArcSweepingMoreThanTwoTurns)
{
    LineStyle style = named("N");
    style.strokes = {Stroke{.op = StrokeOp::Arc, .radius = 1.0, .startAngle = 0.0, .endAngle = 721.0}};
    const Error error = writeRefusal(holding(style));
    EXPECT_EQ(error.code, ErrorCode::InvalidArgument);
    EXPECT_EQ(error.message, std::string(R"(linestyles[0] "N" strokes[0]: this arc )") + kSweeps);

    style.strokes = {Stroke{.op = StrokeOp::Arc, .radius = 1.0, .startAngle = 0.0, .endAngle = 720.0}};
    const Customisation held = holding(style);
    const auto written = katana::entity::customisationToJson(held, {});
    ASSERT_TRUE(written.ok()) << written.error().describe();
    EXPECT_TRUE(readCustomisation(*written) == held);
}

TEST(CustomisationBounds, TheWriterRefusesTooManyStrokesAndATextOverAThousandBytes)
{
    LineStyle style = named("N");
    style.strokes.assign(100001, Stroke{.op = StrokeOp::Move});
    const Error strokes = writeRefusal(holding(style));
    EXPECT_EQ(strokes.code, ErrorCode::InvalidArgument);
    EXPECT_EQ(strokes.message,
              R"(linestyles[0] "N": the definition holds 100001 strokes, and a definition holds )"
              "at most 100000");
    style.strokes.assign(100000, Stroke{.op = StrokeOp::Move});
    const Customisation held = holding(style);
    ASSERT_TRUE(katana::entity::customisationToJson(held, {}).ok());

    const Error name = writeRefusal(holding(named(std::string(1001, 'L'))));
    EXPECT_EQ(name.code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(startsWith(name.message, R"(linestyles[0] ")" + std::string(240, 'L')))
        << name.message.substr(0, 300);
    EXPECT_TRUE(endsWith(name.message, std::string(R"("name" is )") + kLong))
        << name.message.substr(0, 300);
    EXPECT_LT(name.message.size(), 600u);
}
