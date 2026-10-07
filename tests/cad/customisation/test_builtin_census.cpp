// The customisation compiled into the program, when it is the reference one.
//
// The reference customisation is third-party material under its author's own
// licence and is not in the repository, so a build from a clean checkout has
// none compiled in and every test here SKIPS: the suite must stay green
// without it. Where it is compiled in - cad::compiledInCustomisation() holds a
// customisation named "NSW" - these are the figures it must hold.
//
// Where the figures come from: not from this program. They were counted by
// two scripts that use none of its code and were run again for this test -
// tools/customisation_census.py over the compiled-in file and
// tools/reference_census.py over the four legacy files it was converted from,
// given in the order they are converted in; they agree figure for figure,
// except that the conversion took the publisher's leading word off the front
// of names and group paths, which is what a conversion is for. Nothing here
// spells a definition, a group, a layer, a code or a comment of it: counts
// only, and a property that a test can find without naming anything.
//
// What a build has compiled in is asked of the build, never of the
// environment: the seam KATANA_BUILTIN_CUSTOMISATION changes what a program's
// front end starts with, and not what was compiled in.

#include <gtest/gtest.h>

#include <algorithm>
#include <cstddef>
#include <map>
#include <string>
#include <vector>

#include "katana/cad/customisation_host.hpp"
#include "katana/entity/customisation.hpp"
#include "katana/entity/style_library.hpp"

namespace {

using katana::entity::Customisation;
using katana::entity::LineStyle;

// The compiled-in customisation when it is the reference one; null otherwise.
const Customisation* referenceBuiltIn()
{
    const katana::cad::BuiltInCustomisation& compiledIn = katana::cad::compiledInCustomisation();
    if (!compiledIn.customisation || compiledIn.customisation->name != "NSW") {
        return nullptr;
    }
    return compiledIn.customisation.get();
}

} // namespace

TEST(BuiltInCensus, TheCompiledInReferenceCustomisationHoldsTheFiguresOfItsCensus)
{
    const Customisation* const nsw = referenceBuiltIn();
    if (nsw == nullptr) {
        GTEST_SKIP() << "this build has no reference customisation compiled in";
    }
    std::size_t symbols = 0;
    std::size_t linestyles = 0;
    std::size_t atVertices = 0;
    std::size_t strokes = 0;
    nsw->library.forEach([&](const LineStyle& style) {
        (style.symbol ? symbols : linestyles) += 1;
        atVertices += style.atVertices ? 1 : 0;
        strokes += style.strokes.size();
    });
    // 792 definitions: 471 listed as symbols and 321 not. The census counts
    // the symbol library's 474 names and the linestyle library's 321 and says
    // three are in both; the library read last keeps them, so those three are
    // linestyles: 474 - 3 = 471.
    EXPECT_EQ(nsw->library.size(), 792u);
    EXPECT_EQ(symbols, 471u);
    EXPECT_EQ(linestyles, 321u);
    EXPECT_EQ(symbols + linestyles, nsw->library.size());
    EXPECT_EQ(atVertices, 155u);
    EXPECT_EQ(katana::entity::styleGroups(nsw->library).size(), 71u);
    // The census's `opsInLibrary`: 17,020 + 17,222 + 104 + 312 + 178 + 342 +
    // 514 strokes, text strokes included.
    EXPECT_EQ(strokes, 35692u);
    // 725 + 899 rules of the two survey code files, in 632 distinct keys.
    EXPECT_EQ(nsw->map.size(), 1624u);
    EXPECT_EQ(nsw->map.keys().size(), 632u);
    EXPECT_EQ(nsw->colours.size(), 7u);

    // Five of the names the rules use are defined by no library: the plain
    // continuous lines "0" and "1", and three the customisation expects to be
    // supplied elsewhere.
    std::vector<std::string> unresolved;
    for (const std::string& name : nsw->map.stylesReferenced()) {
        if (!nsw->library.contains(name)) {
            unresolved.push_back(name);
        }
    }
    EXPECT_EQ(unresolved.size(), 5u);
    EXPECT_NE(std::find(unresolved.begin(), unresolved.end(), "0"), unresolved.end());
    EXPECT_NE(std::find(unresolved.begin(), unresolved.end(), "1"), unresolved.end());
}

// Each definition says which customisation it came from by that
// customisation's NAME - never a file, never a path - so a project that
// records it records "NSW".
TEST(BuiltInCensus, EveryDefinitionIsFromTheCustomisationItselfByItsName)
{
    const Customisation* const nsw = referenceBuiltIn();
    if (nsw == nullptr) {
        GTEST_SKIP() << "this build has no reference customisation compiled in";
    }
    std::size_t elsewhere = 0;
    nsw->library.forEach(
        [&](const LineStyle& style) { elsewhere += style.source == nsw->name ? 0 : 1; });
    EXPECT_EQ(elsewhere, 0u);
    EXPECT_FALSE(katana::cad::compiledInCustomisation().digest.empty());
}

// A group path says what a definition is. A word put in front of most of them
// says only whose the customisation was, and is not wanted: no first word - up
// to the first blank - may begin the group path of more than half the
// definitions. Nothing here names such a word; the test finds it if there is
// one. (It was a test of the legacy built-in's four files, and the property
// is the data's, so it is kept here.)
TEST(BuiltInCensus, NoWordBeginsTheGroupPathOfMostDefinitions)
{
    const Customisation* const nsw = referenceBuiltIn();
    if (nsw == nullptr) {
        GTEST_SKIP() << "this build has no reference customisation compiled in";
    }
    std::map<std::string, std::size_t> firstWords;
    std::size_t grouped = 0;
    nsw->library.forEach([&](const LineStyle& style) {
        if (style.group.empty()) {
            return;
        }
        ++grouped;
        ++firstWords[style.group.substr(0, style.group.find(' '))];
    });
    ASSERT_GT(grouped, 0u) << "the definitions are grouped";
    for (const auto& [word, count] : firstWords) {
        EXPECT_LE(count * 2, grouped)
            << "a word begins " << count << " of " << grouped << " group paths";
    }
}
