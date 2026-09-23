// The strict XML reader (include/katana/core/xml.hpp). It reads untrusted
// files, so its refusals matter at least as much as what it accepts: every
// documented refusal has a test here, and so does the depth cap, because a
// reader that crashes instead of reporting is a reader that cannot be trusted
// with a file somebody sent you.
//
// Every expectation below is worked out from the XML 1.0 specification (fifth
// edition): section 4.6 for the five predefined entities and their
// replacement text, 4.1 for character references, 2.7 for CDATA, and 3.1 for
// start, end and empty-element tags. None is captured from what this program
// printed.

#include <gtest/gtest.h>

#include <cstddef>
#include <string>
#include <string_view>

#include "katana/core/xml.hpp"

using namespace katana::core;

namespace {

// The diagnostics are part of the contract - a refusal nobody can act on is
// barely better than a crash - so the refusal tests assert on the wording.
// Plain gtest: nothing in this repository links gmock.
[[nodiscard]] bool mentions(const std::string& message, std::string_view phrase)
{
    return message.find(phrase) != std::string::npos;
}

// `<a0><a1>...</a1></a0>` with `elements` elements in it, so the innermost
// sits at depth `elements - 1` - the number `readXml` compares with
// kXmlMaxDepth.
[[nodiscard]] std::string nested(std::size_t elements)
{
    std::string text;
    for (std::size_t i = 0; i < elements; ++i) {
        text += "<a" + std::to_string(i) + ">";
    }
    for (std::size_t i = elements; i-- > 0;) {
        text += "</a" + std::to_string(i) + ">";
    }
    return text;
}

} // namespace

TEST(CoreXml, ReadsChildrenInDocumentOrderAndTrimsTheirText)
{
    const auto root = readXml("<root>\n  <first> one </first>\n  <second>two</second>\n</root>");
    ASSERT_TRUE(root.ok()) << root.error().describe();
    EXPECT_EQ(root->name, "root");
    ASSERT_EQ(root->children.size(), 2U);
    EXPECT_EQ(root->children[0].name, "first");
    EXPECT_EQ(root->children[1].name, "second");
    // Leading and trailing white space is gone; the text itself is untouched.
    EXPECT_EQ(root->childText("first"), "one");
    EXPECT_EQ(root->childText("second"), "two");
    // An element with children has no text of its own, not the white space
    // between them.
    EXPECT_EQ(root->text, "");
}

TEST(CoreXml, NestsToTheDepthOfTheDocument)
{
    const auto root = readXml("<a><b><c><d>deep</d></c></b></a>");
    ASSERT_TRUE(root.ok()) << root.error().describe();
    const XmlNode* b = root->child("b");
    ASSERT_NE(b, nullptr);
    const XmlNode* c = b->child("c");
    ASSERT_NE(c, nullptr);
    EXPECT_EQ(c->childText("d"), "deep");
    EXPECT_TRUE(c->has("d"));
    EXPECT_FALSE(c->has("b"));
}

TEST(CoreXml, FindsEveryChildOfAName)
{
    const auto root = readXml("<list><item>1</item><other/><item>2</item><item>3</item></list>");
    ASSERT_TRUE(root.ok()) << root.error().describe();
    const auto items = root->childrenNamed("item");
    ASSERT_EQ(items.size(), 3U);
    EXPECT_EQ(items[0]->text, "1");
    EXPECT_EQ(items[1]->text, "2");
    EXPECT_EQ(items[2]->text, "3");
    EXPECT_TRUE(root->childrenNamed("missing").empty());
}

TEST(CoreXml, AnEmptyElementAndAMissingOneAreTheSameAnswer)
{
    // Documented in the header: in a mapfile `<rotation/>` and no rotation at
    // all both mean "nothing said", so both must read as an empty string.
    const auto root = readXml("<item><rotation/></item>");
    ASSERT_TRUE(root.ok()) << root.error().describe();
    EXPECT_EQ(root->childText("rotation"), "");
    EXPECT_EQ(root->childText("scale"), "");
    // They are still distinguishable when a caller needs to know.
    EXPECT_TRUE(root->has("rotation"));
    EXPECT_FALSE(root->has("scale"));
}

TEST(CoreXml, ReadsAttributesInEitherQuoteInTheOrderWritten)
{
    const auto root = readXml("<p one=\"1\" two='2' three = \"3\" />");
    ASSERT_TRUE(root.ok()) << root.error().describe();
    ASSERT_EQ(root->attributes.size(), 3U);
    EXPECT_EQ(root->attributes[0].first, "one");
    EXPECT_EQ(root->attributes[0].second, "1");
    EXPECT_EQ(root->attributes[1].first, "two");
    EXPECT_EQ(root->attributes[1].second, "2");
    EXPECT_EQ(root->attributes[2].first, "three");
    EXPECT_EQ(root->attributes[2].second, "3");
    // An attribute value keeps its white space: only element text is trimmed.
    const auto spaced = readXml("<p name=\" kerb \"/>");
    ASSERT_TRUE(spaced.ok()) << spaced.error().describe();
    ASSERT_EQ(spaced->attributes.size(), 1U);
    EXPECT_EQ(spaced->attributes[0].second, " kerb ");
}

TEST(CoreXml, ASelfClosingElementIsAnElementWithNoTextAndNoChildren)
{
    const auto root = readXml("<set><line n='1'/><line n='2'/></set>");
    ASSERT_TRUE(root.ok()) << root.error().describe();
    ASSERT_EQ(root->children.size(), 2U);
    for (const XmlNode& line : root->children) {
        EXPECT_EQ(line.name, "line");
        EXPECT_TRUE(line.text.empty());
        EXPECT_TRUE(line.children.empty());
        ASSERT_EQ(line.attributes.size(), 1U);
    }
    EXPECT_EQ(root->children[0].attributes[0].second, "1");
    EXPECT_EQ(root->children[1].attributes[0].second, "2");
}

TEST(CoreXml, ACdataSectionIsTakenLiterally)
{
    // XML 1.0 section 2.7: inside CDATA, `<` and `&` are data, not markup.
    const auto root = readXml("<note><![CDATA[ a < b & c ]]></note>");
    ASSERT_TRUE(root.ok()) << root.error().describe();
    EXPECT_EQ(root->text, "a < b & c");
}

TEST(CoreXml, CdataAndPlainTextJoinIntoOneRun)
{
    const auto root = readXml("<note>before<![CDATA[<middle>]]>after</note>");
    ASSERT_TRUE(root.ok()) << root.error().describe();
    EXPECT_EQ(root->text, "before<middle>after");
}

TEST(CoreXml, ResolvesTheFivePredefinedEntitiesInTextAndInAttributes)
{
    // XML 1.0 section 4.6: amp lt gt quot apos, and nothing else is predefined.
    const auto root = readXml("<t a=\"&amp;&lt;&gt;&quot;&apos;\">&amp;&lt;&gt;&quot;&apos;</t>");
    ASSERT_TRUE(root.ok()) << root.error().describe();
    EXPECT_EQ(root->text, "&<>\"'");
    ASSERT_EQ(root->attributes.size(), 1U);
    EXPECT_EQ(root->attributes[0].second, "&<>\"'");
}

TEST(CoreXml, ResolvesDecimalAndHexadecimalCharacterReferences)
{
    // XML 1.0 section 4.1. U+00E9 is UTF-8 0xC3 0xA9 and U+20AC is 0xE2 0x82
    // 0xAC (Unicode 15.0, UTF-8 encoding table); 0x41 is 'A' in ASCII.
    const auto root = readXml("<t>&#65;&#x42;&#233;&#x20AC;</t>");
    ASSERT_TRUE(root.ok()) << root.error().describe();
    EXPECT_EQ(root->text, std::string("AB\xC3\xA9\xE2\x82\xAC"));
}

TEST(CoreXml, SkipsTheDeclarationCommentsAndProcessingInstructions)
{
    const auto root = readXml("<?xml version=\"1.0\"?>\n<!-- a note -->\n<?other pi?>\n"
                              "<root><!-- inside --><a>1</a><?pi inside?></root>");
    ASSERT_TRUE(root.ok()) << root.error().describe();
    EXPECT_EQ(root->name, "root");
    ASSERT_EQ(root->children.size(), 1U);
    EXPECT_EQ(root->childText("a"), "1");
}

TEST(CoreXml, AcceptsNestingUpToTheDepthCapAndRefusesOneMore)
{
    // The cap exists because the reader recurses: past it the answer must be
    // an error, never a blown stack.
    const auto deepest = readXml(nested(kXmlMaxDepth + 1));
    ASSERT_TRUE(deepest.ok()) << deepest.error().describe();

    const auto tooDeep = readXml(nested(kXmlMaxDepth + 2));
    ASSERT_FALSE(tooDeep.ok());
    EXPECT_EQ(tooDeep.error().code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(mentions(tooDeep.error().message, "nested more than 64 deep")) << tooDeep.error().message;
}

TEST(CoreXml, RefusesADoctypeRatherThanSkippingIt)
{
    // A doctype may redefine the entities used by the rest of the document, so
    // a reader that skipped it would be reading a document whose meaning it
    // has not seen.
    const auto root = readXml("<!DOCTYPE map SYSTEM \"map.dtd\">\n<map/>");
    ASSERT_FALSE(root.ok());
    EXPECT_EQ(root.error().code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(mentions(root.error().message, "document type declaration")) << root.error().message;

    // An internal subset, the form that carries entity definitions, is refused
    // by the same check.
    const auto withSubset = readXml("<!DOCTYPE map [ <!ENTITY x \"y\"> ]>\n<map/>");
    ASSERT_FALSE(withSubset.ok());
    EXPECT_TRUE(mentions(withSubset.error().message, "document type declaration")) << withSubset.error().message;
}

TEST(CoreXml, RefusesAnEntityItHasNoDefinitionFor)
{
    const auto root = readXml("<t>&nbsp;</t>");
    ASSERT_FALSE(root.ok());
    EXPECT_EQ(root.error().code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(mentions(root.error().message, "&nbsp;")) << root.error().message;
    EXPECT_TRUE(mentions(root.error().message, "entity this does not know")) << root.error().message;

    // In an attribute value too - the same resolver serves both.
    const auto attribute = readXml("<t a=\"&nbsp;\"/>");
    ASSERT_FALSE(attribute.ok());
    EXPECT_TRUE(mentions(attribute.error().message, "&nbsp;")) << attribute.error().message;
}

TEST(CoreXml, RefusesAnAmpersandThatStartsNoEntity)
{
    const auto root = readXml("<t>Jones & Son</t>");
    ASSERT_FALSE(root.ok());
    EXPECT_TRUE(mentions(root.error().message, "ampersand")) << root.error().message;
}

TEST(CoreXml, RefusesACharacterReferenceOutsideUnicode)
{
    // U+10FFFF is the last code point (Unicode 15.0, section 3.4 D9); 0x110000
    // is not a character.
    const auto root = readXml("<t>&#x110000;</t>");
    ASSERT_FALSE(root.ok());
    EXPECT_TRUE(mentions(root.error().message, "is not a character")) << root.error().message;
}

TEST(CoreXml, RefusesAnElementThatIsNeverClosed)
{
    const auto text = readXml("<root><a>one");
    ASSERT_FALSE(text.ok());
    EXPECT_EQ(text.error().code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(mentions(text.error().message, "\"a\" is never closed")) << text.error().message;

    // The same answer when the document ends inside the start tag.
    const auto tag = readXml("<root><a ");
    ASSERT_FALSE(tag.ok());
    EXPECT_TRUE(mentions(tag.error().message, "\"a\" is never closed")) << tag.error().message;
}

TEST(CoreXml, RefusesAMismatchedCloseTagAndNamesTheLine)
{
    const auto root = readXml("<a>\n  <b>\n  </a>\n");
    ASSERT_FALSE(root.ok());
    EXPECT_EQ(root.error().code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(mentions(root.error().message, "\"b\" is closed by \"a\"")) << root.error().message;
    // The line is what makes the message usable on a real file: the `</a>` is
    // on the third line of that document.
    EXPECT_EQ(root.error().context, "line 3");
}

TEST(CoreXml, RefusesAnUnclosedCommentOrCdataSection)
{
    const auto comment = readXml("<!-- never ends\n<root/>");
    ASSERT_FALSE(comment.ok());
    EXPECT_TRUE(mentions(comment.error().message, "comment is never closed")) << comment.error().message;

    const auto cdata = readXml("<root><![CDATA[ never ends </root>");
    ASSERT_FALSE(cdata.ok());
    EXPECT_TRUE(mentions(cdata.error().message, "CDATA section is never closed")) << cdata.error().message;
}

TEST(CoreXml, RefusesAMalformedAttribute)
{
    const auto bare = readXml("<p flag/>");
    ASSERT_FALSE(bare.ok());
    EXPECT_TRUE(mentions(bare.error().message, "has no value")) << bare.error().message;

    const auto unquoted = readXml("<p n=1/>");
    ASSERT_FALSE(unquoted.ok());
    EXPECT_TRUE(mentions(unquoted.error().message, "is not in quotes")) << unquoted.error().message;

    const auto unterminated = readXml("<p n=\"1 />");
    ASSERT_FALSE(unterminated.ok());
    EXPECT_TRUE(mentions(unterminated.error().message, "never closed")) << unterminated.error().message;
}

TEST(CoreXml, RefusesTextThatIsNotAnElementAtAll)
{
    const auto empty = readXml("");
    ASSERT_FALSE(empty.ok());
    EXPECT_TRUE(mentions(empty.error().message, "no element here")) << empty.error().message;

    const auto prose = readXml("this is not XML");
    ASSERT_FALSE(prose.ok());
    EXPECT_TRUE(mentions(prose.error().message, "no element here")) << prose.error().message;

    const auto nameless = readXml("<1>");
    ASSERT_FALSE(nameless.ok());
    EXPECT_TRUE(mentions(nameless.error().message, "tag has no name")) << nameless.error().message;
}
