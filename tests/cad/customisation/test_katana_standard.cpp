// Katana Standard: the customisation tracked as
// resources/customisation/katana-standard.customisation.json
// (docs/katana_standard.md). Its survey code KEYS are the NSW customisation's,
// byte for byte, on the owner's decision; everything else in it is its own.
//
// The tests read the COMMITTED file with the strict reader and hold what the
// library promises, so a regenerated file that drifted from its design fails
// here and not in somebody's drawing. Every figure is from a source that is
// not this program:
//
//   * the counts of keys, rules, definitions and strokes, and the figures
//     below, are counted by tools/katana_standard/census.py and
//     tools/customisation_census.py, which read the file with the standard
//     `json` module and share no code with Katana, and by
//     tools/katana_standard/parity.py, which reads both files the same way:
//     632 keys; 1103 rules, of which 917 are the reference's own structure
//     (its 916 effective rules and the one symbol rule that AD* needs) and
//     186 are the service attribute rules this library adds; 221 definitions
//     (98 linestyles, 123 symbols); 28 colours;
//   * contrast is the WCAG 2.x formula, a published standard;
//   * a hand-worked check says why a figure is what it is where it can: 572
//     keys have a feature rule and 60 have none (52 that only set a surface
//     flag, the 7 text codes and the bare `*`); 360 of the 572 draw a line,
//     205 a point and 7 only name a layer; 203 symbol rules are the 202 points
//     with a symbol and the one line (BI*) with one; the 119 surface rules are
//     the reference's 52 surface-only keys and 67 feature-and-surface keys,
//     and every one says false; 244 attributes are applied (186 service
//     words, 12 model zones, 28 stated diameters and 18 stated heights) and
//     641 are prompts;
//   * the palette's two floors are the WCAG contrast formula and the OKLab
//     distance of Bjorn Ottosson's published matrices, written out below;
//   * the service colours are AS 5488's: the standard names them and gives no
//     sRGB, so the values are Katana's, and the one table of them is
//     utilityTypeColour (utility_drawing.hpp), which UTILITY DRAW draws in and
//     which the file's colours are held equal to.
//
// Nothing here names a definition, a layer, a group or a legend label of the
// NSW customisation: where the two are compared it is by key and by kind of
// entity, from the two tracked files, at run time. What else the two share is
// counted by tools/katana_standard/originality.py (nothing: no name, group,
// layer, label, colour name, attribute name or text style, and of colour
// values only the white of communications; a name is also compared with the
// other's names less their lead-in of capitals and with its legend labels,
// and a label with its names) and held here by count.

#include <gtest/gtest.h>

#include <algorithm>
#include <array>
#include <cctype>
#include <cmath>
#include <cstddef>
#include <fstream>
#include <iterator>
#include <map>
#include <optional>
#include <regex>
#include <set>
#include <sstream>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/cad/customisation_host.hpp"
#include "katana/cad/plot.hpp"
#include "katana/cad/utilities/utility_drawing.hpp"
#include "katana/entity/customisation.hpp"

namespace {

using katana::entity::Color;
using katana::entity::Customisation;
using katana::entity::LineStyle;
using katana::entity::StrokeOp;
using katana::entity::StyleUnits;
using katana::entity::SurveyRule;
using katana::entity::SurveySection;

std::string slurp(const std::string& path)
{
    std::ifstream in(path, std::ios::binary);
    return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
}

// A tracked customisation file, read once. `problem` is set when it cannot be
// read, so that every test says why instead of crashing on an empty value.
struct Loaded {
    std::string text;
    Customisation value;
    std::string problem;
};

Loaded load(const char* path)
{
    Loaded s;
    s.text = slurp(path);
    if (s.text.empty()) {
        s.problem = std::string("no file at ") + path;
        return s;
    }
    auto read = katana::entity::customisationFromJson(s.text);
    if (!read.ok()) {
        s.problem = read.error().describe();
        return s;
    }
    s.value = std::move(*read);
    return s;
}

const Loaded& standard()
{
    static const Loaded loaded = load(KATANA_STANDARD_JSON);
    return loaded;
}

// The NSW customisation, tracked beside it: the one thing it shares with this
// library is its keys, and these tests read it to hold that.
const Loaded& nsw()
{
    static const Loaded loaded = load(KATANA_NSW_JSON);
    return loaded;
}

// The rules of one section, by key as written (`AC*`, `PBMK`, `*`).
std::map<std::string, const SurveyRule*> rulesOf(const Customisation& c, SurveySection section)
{
    std::map<std::string, const SurveyRule*> found;
    for (const SurveyRule& rule : c.map.rules()) {
        if (rule.section == section) {
            found.emplace(rule.key, &rule);
        }
    }
    return found;
}

bool hasAttribute(const SurveyRule& rule, std::string_view name, std::string_view value)
{
    return std::any_of(rule.attributes.begin(), rule.attributes.end(),
                       [&](const auto& a) { return a.name == name && a.value == value; });
}

bool asksFor(const SurveyRule& rule, std::string_view name)
{
    return hasAttribute(rule, name, "");
}

bool hasAttributeNamed(const SurveyRule& rule, std::string_view name)
{
    return std::any_of(rule.attributes.begin(), rule.attributes.end(),
                       [&](const auto& a) { return a.name == name; });
}

// The first path level of a rule's group: the class it belongs to.
std::string classOf(const SurveyRule& rule)
{
    return rule.group.substr(0, rule.group.find('/'));
}

// WCAG 2.x relative luminance and contrast ratio.
double linear(unsigned channel)
{
    const double c = channel / 255.0;
    return c <= 0.04045 ? c / 12.92 : std::pow((c + 0.055) / 1.055, 2.4);
}

double luminance(const Color& c)
{
    return 0.2126 * linear(c.r) + 0.7152 * linear(c.g) + 0.0722 * linear(c.b);
}

double contrast(const Color& a, const Color& b)
{
    const double hi = std::max(luminance(a), luminance(b));
    const double lo = std::min(luminance(a), luminance(b));
    return (hi + 0.05) / (lo + 0.05);
}

// Distance in OKLab (Bjorn Ottosson, 2020: the published matrices), the
// space in which a step of 0.02 is about the least a person sees between two
// large patches. The palette's own check uses the same published figures.
double oklabDistance(const Color& a, const Color& b)
{
    const auto lab = [](const Color& c) {
        const double r = linear(c.r);
        const double g = linear(c.g);
        const double bl = linear(c.b);
        const double l = std::cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * bl);
        const double m = std::cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * bl);
        const double s = std::cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * bl);
        return std::array<double, 3>{0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
                                     1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
                                     0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s};
    };
    const auto p = lab(a);
    const auto q = lab(b);
    return std::sqrt((p[0] - q[0]) * (p[0] - q[0]) + (p[1] - q[1]) * (p[1] - q[1]) +
                     (p[2] - q[2]) * (p[2] - q[2]));
}

// Millimetres at most this far from a whole number of thousandths.
bool atMostThreeDecimals(double value)
{
    return std::fabs(value * 1000.0 - std::round(value * 1000.0)) < 1e-6;
}

// The words CLAUDE.md keeps out of new text, assembled so that this file does
// not hold them.
std::vector<std::string> forbiddenWords()
{
    return {std::string("1") + "2d", std::string("ex") + "ds", std::string("tf") + "nsw",
            std::string("transport for n") + "sw", std::string("n") + "sw"};
}

std::string lower(std::string text)
{
    std::transform(text.begin(), text.end(), text.begin(),
                   [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
    return text;
}

} // namespace

#define KS_LOAD()                                                                                  \
    const Loaded& ks = standard();                                                                 \
    ASSERT_TRUE(ks.problem.empty()) << ks.problem;                                                 \
    [[maybe_unused]] const Customisation& file = ks.value

TEST(KatanaStandard, TheCommittedFileIsReadByTheStrictReaderAndIsNamedKatanaStandard)
{
    KS_LOAD();
    EXPECT_EQ(file.name, "Katana Standard");
    EXPECT_EQ(file.notice.size(), 3u);
    EXPECT_FALSE(file.description.empty());
}

TEST(KatanaStandard, ItHoldsTheCountsOfItsIndependentCensus)
{
    KS_LOAD();
    std::size_t symbols = 0;
    std::size_t linestyles = 0;
    std::size_t atVertices = 0;
    std::map<StrokeOp, std::size_t> strokes;
    file.library.forEach([&](const LineStyle& style) {
        (style.symbol ? symbols : linestyles) += 1;
        atVertices += style.atVertices ? 1 : 0;
        for (const auto& stroke : style.strokes) {
            ++strokes[stroke.op];
        }
    });
    EXPECT_EQ(file.library.size(), 221u);
    EXPECT_EQ(linestyles, 98u);
    EXPECT_EQ(symbols, 123u);
    EXPECT_EQ(atVertices, 123u) << "every symbol is at vertices and no linestyle is";
    // census.py: arc 32, circle 118, dot 40, draw 1709, move 701, text 54.
    EXPECT_EQ(strokes[StrokeOp::Arc], 32u);
    EXPECT_EQ(strokes[StrokeOp::Circle], 118u);
    EXPECT_EQ(strokes[StrokeOp::Dot], 40u);
    EXPECT_EQ(strokes[StrokeOp::Draw], 1709u);
    EXPECT_EQ(strokes[StrokeOp::Move], 701u);
    EXPECT_EQ(strokes[StrokeOp::Text], 54u);
    EXPECT_EQ(strokes[StrokeOp::Pen], 0u) << "no stroke names a pen: the rule gives the colour";
    EXPECT_EQ(katana::entity::styleGroups(file.library).size(), 51u);
    EXPECT_EQ(file.map.size(), 1103u);
    EXPECT_EQ(file.map.keys().size(), 632u);
    EXPECT_EQ(file.colours.size(), 28u);
}

TEST(KatanaStandard, EveryClassHoldsTheNumberOfKeysItsGroupsSayAndSixtyKeysHaveNoFeatureRule)
{
    KS_LOAD();
    // A key's class is the first level of the group on its feature rule; the
    // keys that have none (the text codes, the surface-only keys and the bare
    // `*`) have no group in the file.
    std::map<std::string, std::size_t> perClass;
    for (const auto& [key, rule] : rulesOf(file, SurveySection::Map)) {
        ++perClass[classOf(*rule)];
    }
    // census.py "classes".
    const std::map<std::string, std::size_t> census{{"Boundaries and Cadastre", 23},
                                                    {"Roads and Pavements", 35},
                                                    {"Street Furniture and Signs", 36},
                                                    {"Kerbs and Drainage", 79},
                                                    {"Rail and Transit", 12},
                                                    {"Utilities", 118},
                                                    {"Vegetation and Landscape", 33},
                                                    {"Buildings and Structures", 33},
                                                    {"Fences and Walls", 8},
                                                    {"Terrain and Breaklines", 25},
                                                    {"Survey Control and Annotation", 39},
                                                    {"Miscellaneous", 16},
                                                    {"Design and Setting Out", 115}};
    EXPECT_EQ(perClass, census);
    std::size_t withFeature = 0;
    for (const auto& [name, count] : census) {
        withFeature += count;
    }
    EXPECT_EQ(withFeature, 572u);
    // 52 keys that only set a surface flag, the 7 text codes, and the bare `*`.
    EXPECT_EQ(file.map.keys().size() - withFeature, 60u);
    EXPECT_EQ(52u + 7u + 1u, 60u);
}

TEST(KatanaStandard, EveryKeyIsCapitalsAndDigitsWithOptionalStarAndNoRuleIsWrittenTwiceInASection)
{
    KS_LOAD();
    const std::regex shape("\\*|[A-Z0-9]+\\*?");
    std::set<std::pair<std::string, SurveySection>> seen;
    for (const SurveyRule& rule : file.map.rules()) {
        EXPECT_TRUE(std::regex_match(rule.key, shape)) << "key " << rule.key;
        EXPECT_TRUE(seen.emplace(rule.key, rule.section).second)
            << rule.key << " is written twice in one section";
    }
    // The three forms of key: 422 prefix keys, 209 exact keys, the bare `*`.
    std::size_t prefix = 0;
    std::size_t exact = 0;
    std::size_t bare = 0;
    for (const std::string& key : file.map.keys()) {
        if (key == "*") {
            ++bare;
        } else if (key.back() == '*') {
            ++prefix;
        } else {
            ++exact;
        }
    }
    EXPECT_EQ(prefix, 422u);
    EXPECT_EQ(exact, 209u);
    EXPECT_EQ(bare, 1u);
    // A key is the code of a feature or of a flag, never the empty one.
    EXPECT_EQ(rulesOf(file, SurveySection::Map).size(), 572u);
    EXPECT_EQ(rulesOf(file, SurveySection::Tinable).size(), 119u);
}

TEST(KatanaStandard, ARuleOfEachKindIsWrittenForTheCodesThatNeedIt)
{
    KS_LOAD();
    std::map<SurveySection, std::size_t> bySection;
    std::size_t lines = 0;
    std::size_t points = 0;
    std::size_t layerOnly = 0;
    for (const SurveyRule& rule : file.map.rules()) {
        ++bySection[rule.section];
        if (rule.section == SurveySection::Map) {
            if (!rule.breakline) {
                ++layerOnly;
            } else {
                (*rule.breakline == katana::entity::SurveyBreakline::Line ? lines : points) += 1;
            }
        }
    }
    EXPECT_EQ(bySection[SurveySection::Map], 572u);
    EXPECT_EQ(bySection[SurveySection::Tinable], 119u);
    EXPECT_EQ(bySection[SurveySection::Pipe], 1u);
    EXPECT_EQ(bySection[SurveySection::VertexPipe], 1u);
    EXPECT_EQ(bySection[SurveySection::SegmentPipe], 1u);
    EXPECT_EQ(bySection[SurveySection::VertexTextStyle], 7u);
    // 12 attribute rules the reference has, and 186 service rules this library adds.
    EXPECT_EQ(bySection[SurveySection::StringAttribute], 198u);
    EXPECT_EQ(bySection[SurveySection::VertexAttribute], 1u);
    // 572 keys with a feature rule: 360 lines, 205 points, 7 that only name the
    // layer of a text code (1* to 8*).
    EXPECT_EQ(lines, 360u);
    EXPECT_EQ(points, 205u);
    EXPECT_EQ(layerOnly, 7u);
    EXPECT_EQ(lines + points + layerOnly, 572u);
    // A symbol rule for every point but three that name none (POCP, PRTR and
    // the design key PLIG*), and for the one line that carries a symbol at
    // every vertex (BI*): 205 - 3 + 1.
    EXPECT_EQ(bySection[SurveySection::VertexSymbol], 203u);
}

TEST(KatanaStandard, EveryNameARuleUsesIsDefinedExceptThePlainLineAndEveryDefinitionIsUsed)
{
    KS_LOAD();
    std::set<std::string> referenced;
    for (const std::string& name : file.map.stylesReferenced()) {
        referenced.insert(name);
    }
    std::set<std::string> undefined;
    for (const std::string& name : referenced) {
        if (!file.library.contains(name)) {
            undefined.insert(name);
        }
    }
    EXPECT_EQ(undefined, (std::set<std::string>{"continuous"}))
        << "the one word that names no definition is Katana's plain line";
    std::size_t unused = 0;
    file.library.forEach([&](const LineStyle& style) {
        if (referenced.count(style.name) == 0) {
            ++unused;
            ADD_FAILURE() << style.name << " is defined and no code names it";
        }
    });
    EXPECT_EQ(unused, 0u);
    // A rule's linestyle is a linestyle and its symbol is a symbol, so
    // CODE CHECK has nothing to warn of.
    for (const SurveyRule& rule : file.map.rules()) {
        if (!rule.linestyle.empty() && rule.linestyle != "continuous") {
            const LineStyle* style = file.library.find(rule.linestyle);
            ASSERT_NE(style, nullptr) << rule.key;
            EXPECT_FALSE(style->symbol) << rule.key << " names a symbol as its linestyle";
        }
        if (rule.symbol && !rule.symbol->style.empty()) {
            const LineStyle* style = file.library.find(rule.symbol->style);
            ASSERT_NE(style, nullptr) << rule.key;
            EXPECT_TRUE(style->symbol) << rule.key << " names a linestyle as its symbol";
        }
    }
}

TEST(KatanaStandard, EveryColourARuleNamesIsInTheTableAndTheTableHoldsNoOther)
{
    KS_LOAD();
    std::set<std::string> used;
    for (const SurveyRule& rule : file.map.rules()) {
        if (!rule.colour.empty()) {
            used.insert(rule.colour);
        }
        if (rule.symbol && !rule.symbol->colour.empty()) {
            used.insert(rule.symbol->colour);
        }
        if (rule.textStyle && !rule.textStyle->colour.empty()) {
            used.insert(rule.textStyle->colour);
        }
    }
    std::set<std::string> table;
    for (const auto& entry : file.colours.entries()) {
        table.insert(entry.name);
    }
    EXPECT_EQ(used, table);
    EXPECT_EQ(table.size(), 28u);
}

TEST(KatanaStandard, EveryColourIsNamedByItsRoleAndNoStandardNameFoldsOntoIt)
{
    KS_LOAD();
    for (const auto& entry : file.colours.entries()) {
        EXPECT_EQ(entry.name.rfind("katana ", 0), 0u) << entry.name;
        EXPECT_EQ(entry.name, lower(entry.name)) << "colour names are lower case";
        EXPECT_FALSE(katana::entity::standardColour(entry.name).has_value())
            << entry.name << " is a standard colour name";
    }
}

TEST(KatanaStandard, EveryColourIsReadableOnThePlanGroundAndOnPaperExceptYellowGasAndCreamSewerWhichPrintLight)
{
    KS_LOAD();
    const Color ground{0x1E, 0x23, 0x29, 255};
    const Color paper{255, 255, 255, 255};
    const katana::cad::PlotSettings plot;
    // AS 5488's yellow and cream cannot be 3:1 on white and still be yellow and
    // cream, so the two keep the ground's 3:1 and print light. They are the only
    // colours excused, and a colour that needs no excuse is not given one.
    const std::set<std::string> printsLight{"katana gas", "katana sewer"};
    std::set<std::string> under;
    for (const auto& entry : file.colours.entries()) {
        // WCAG 2.x asks 3:1 of a graphical object against its ground.
        EXPECT_GE(contrast(entry.colour, ground), 3.0) << entry.name << " on the ground";
        // On paper it is the pen as the plot prints it: white prints black.
        const double onPaper = contrast(katana::cad::paperColour(entry.colour, plot), paper);
        if (onPaper < 3.0) {
            under.insert(entry.name);
        }
        if (printsLight.count(entry.name) == 0) {
            EXPECT_GE(onPaper, 3.0) << entry.name << " on paper";
        }
    }
    EXPECT_EQ(under, printsLight);
}

TEST(KatanaStandard, NoTwoColoursAreCloserThanFiveHundredthsInOKLabAndTheWarmClassesAreSixHundredthsApart)
{
    KS_LOAD();
    const auto entries = file.colours.entries();
    // The six warm classes, which sit in the narrow band of lightness that
    // both grounds leave and were placed by a search: each is at least 0.06
    // from every other colour, as colours.py holds it.
    const std::set<std::string> warm{"katana kerb",  "katana wall",  "katana contour",
                                     "katana fuel",  "katana fence", "katana breakline"};
    std::size_t warmPairs = 0;
    for (std::size_t i = 0; i < entries.size(); ++i) {
        for (std::size_t j = i + 1; j < entries.size(); ++j) {
            const double d = oklabDistance(entries[i].colour, entries[j].colour);
            EXPECT_GE(d, 0.05) << entries[i].name << " and " << entries[j].name;
            if (warm.count(entries[i].name) == 1 || warm.count(entries[j].name) == 1) {
                ++warmPairs;
                EXPECT_GE(d, 0.06) << entries[i].name << " and " << entries[j].name;
            }
        }
    }
    // Reaches the cases it claims to: six colours against the other 22, and
    // among themselves 15 pairs: 6 * 22 + 15.
    EXPECT_EQ(warmPairs, 147u);
}

TEST(KatanaStandard, TheClassesDrawnWithTheThinnestPensKeepAFloorOfThreeAndAQuarterOnBothGrounds)
{
    KS_LOAD();
    const Color ground{0x1E, 0x23, 0x29, 255};
    const Color paper{255, 255, 255, 255};
    const katana::cad::PlotSettings plot;
    // Tree, planting, ground, building, wall and communications: a 0.13 to
    // 0.18 mm line of one of these is the faintest thing on a plan.
    // Communications is AS 5488's white, so on paper it is the pen as the plot
    // prints it, black.
    for (const char* name : {"katana tree", "katana planting", "katana ground", "katana building",
                             "katana wall", "katana telecom"}) {
        const auto colour = file.colours.find(name);
        ASSERT_TRUE(colour.has_value()) << name;
        EXPECT_GE(contrast(*colour, ground), 3.25) << name << " on the ground";
        EXPECT_GE(contrast(katana::cad::paperColour(*colour, plot), paper), 3.25)
            << name << " on paper";
    }
}

TEST(KatanaStandard, TheKerbTopIsAPlainLineAndItsBackAndItsGutterLipHavePatternsOfTheirOwn)
{
    KS_LOAD();
    const auto map = rulesOf(file, SurveySection::Map);
    // The top edge of a kerb and the top edge of a median are hard edges drawn
    // plain; the back of the kerb and the lip of the gutter are other things
    // and each has a pattern of its own, since no pen is applied yet.
    EXPECT_EQ(map.at("KB*")->linestyle, "continuous");
    EXPECT_EQ(map.at("TM*")->linestyle, "continuous");
    EXPECT_EQ(map.at("LP*")->linestyle, "Gutter Lip Line");
    EXPECT_EQ(map.at("BK*")->linestyle, "Kerb Back Line");
    for (const char* name : {"Gutter Lip Line", "Kerb Back Line"}) {
        const LineStyle* style = file.library.find(name);
        ASSERT_NE(style, nullptr) << name;
        EXPECT_FALSE(style->symbol) << name;
    }
}

TEST(KatanaStandard, EveryLayerIsThreeLevelsOfLowerCaseWordsAndThereAreOneHundredAndSixty)
{
    KS_LOAD();
    const std::regex word("[a-z]+(-[a-z]+)*");
    std::set<std::string> layers;
    for (const auto& [key, rule] : rulesOf(file, SurveySection::Map)) {
        layers.insert(rule->model);
        std::vector<std::string> levels;
        std::stringstream split(rule->model);
        for (std::string level; std::getline(split, level, '/');) {
            levels.push_back(level);
        }
        EXPECT_EQ(levels.size(), 3u) << key << " layer " << rule->model;
        for (const std::string& level : levels) {
            EXPECT_TRUE(std::regex_match(level, word)) << key << " level '" << level << "'";
        }
    }
    EXPECT_EQ(layers.size(), 160u);
}

TEST(KatanaStandard, EveryGroupIsTwoLevelsDeepAndThereAreFiftyEightInAll)
{
    KS_LOAD();
    std::set<std::string> ofDefinitions;
    file.library.forEach([&](const LineStyle& style) { ofDefinitions.insert(style.group); });
    std::set<std::string> ofRules;
    for (const auto& [key, rule] : rulesOf(file, SurveySection::Map)) {
        ofRules.insert(rule->group);
    }
    std::set<std::string> everyGroup = ofDefinitions;
    everyGroup.insert(ofRules.begin(), ofRules.end());
    for (const std::string& group : everyGroup) {
        EXPECT_EQ(std::count(group.begin(), group.end(), '/'), 1) << group;
    }
    // 57 groups of the codes: 13 classes and their groups, every one of them
    // holding a code. The definitions' 51 are the groups that have a
    // definition; of the codes' 57, 7 have none (four design groups drawn
    // with plain lines or with another group's definitions - the road design
    // strings, earthworks, kerb and carriageway, and bridge design - the
    // paths, the land use areas and the text codes), and the shapes every
    // service shares have a group no code has (Utilities/All Services):
    // 57 - 7 + 1 = 51 groups with a definition and 57 + 1 = 58 in all.
    EXPECT_EQ(ofRules.size(), 57u);
    EXPECT_EQ(ofDefinitions.size(), 51u);
    EXPECT_EQ(57u - 7u + 1u, 51u);
    EXPECT_EQ(everyGroup.size(), 58u);
    EXPECT_EQ(ofRules.count("Utilities/All Services"), 0u);
    EXPECT_EQ(ofDefinitions.count("Utilities/All Services"), 1u);
    EXPECT_EQ(ofDefinitions.count("Survey Control and Annotation/Text and Notes"), 0u);
    EXPECT_EQ(ofRules.count("Survey Control and Annotation/Text and Notes"), 1u);
}

TEST(KatanaStandard, TheWeightsAreTheFiveOfTheScaleAndTheBoldOneIsUnused)
{
    KS_LOAD();
    std::map<std::string, std::size_t> weights;
    for (const auto& [key, rule] : rulesOf(file, SurveySection::Map)) {
        ++weights[rule->weight];
    }
    // census.py: 0.13 21, 0.18 180, 0.25 289, 0.35 79, 0.50 3 - and 0.70, the
    // sixth of the scale, reserved. They add to the 572 feature rules.
    const std::map<std::string, std::size_t> expected{
        {"0.13", 21}, {"0.18", 180}, {"0.25", 289}, {"0.35", 79}, {"0.50", 3}};
    EXPECT_EQ(weights, expected);
    EXPECT_EQ(21u + 180u + 289u + 79u + 3u, 572u);
    EXPECT_EQ(weights.count("0.70"), 0u);
    // Nothing in the planting or the ground colours is lighter than the fine pen.
    for (const auto& [key, rule] : rulesOf(file, SurveySection::Map)) {
        if (rule->colour == "katana planting" || rule->colour == "katana ground") {
            EXPECT_NE(rule->weight, "0.13") << key;
        }
    }
}

TEST(KatanaStandard, OnlyFourSymbolsAreHiddenAndTheyAreThoseTheOtherLibraryHides)
{
    KS_LOAD();
    std::set<std::string> hidden;
    for (const auto& [key, rule] : rulesOf(file, SurveySection::VertexSymbol)) {
        if (rule->hide.value_or(false)) {
            hidden.insert(key);
        }
    }
    // census.py "hideKeys": the four keys whose second symbol rule in the
    // other library says hide where the first leaves it unsaid, so that a
    // lookup there hides the symbol (parity.py, "nsw-fields-merged").
    EXPECT_EQ(hidden, (std::set<std::string>{"PBDY", "PBMZ", "PCRM", "PPSM"}));
}

TEST(KatanaStandard, EverySurfaceRuleKeepsItsCodeOutOfASurfaceAndThereAreOneHundredAndNineteen)
{
    KS_LOAD();
    const auto surfaces = rulesOf(file, SurveySection::Tinable);
    // The other library has 119 keys that set a surface flag (52 that set
    // nothing else, and 67 beside a feature rule) and every one says false.
    EXPECT_EQ(surfaces.size(), 119u);
    for (const auto& [key, rule] : surfaces) {
        ASSERT_TRUE(rule->tinable.has_value()) << key << " says nothing of surfaces";
        EXPECT_FALSE(*rule->tinable) << key;
    }
    EXPECT_EQ(52u + 67u, 119u);
    // By hand, a few of each family: a speed zone, a culvert, a pipe, a
    // trunk's own flag key, a control mark's family key, a hazard's flag key.
    for (const char* key : {"S9*", "D1*", "U3*", "PT04*", "PSS*", "PHAZ*", "CA*", "SO*"}) {
        EXPECT_EQ(surfaces.count(key), 1u) << key;
    }
    // And a few that are not: a kerb top, a title boundary, a water main.
    for (const char* key : {"KB*", "BT*", "WM*"}) {
        EXPECT_EQ(surfaces.count(key), 0u) << key;
    }
}

TEST(KatanaStandard, OnlyTheBareStarIsAPipeAndSizesItFromTheDiameterAttribute)
{
    KS_LOAD();
    const std::map<SurveySection, const char*> kinds{{SurveySection::Pipe, "pipe"},
                                                     {SurveySection::VertexPipe, "vertexPipe"},
                                                     {SurveySection::SegmentPipe, "segmentPipe"}};
    for (const auto& [section, word] : kinds) {
        const auto pipes = rulesOf(file, section);
        ASSERT_EQ(pipes.size(), 1u) << word;
        ASSERT_EQ(pipes.count("*"), 1u) << word;
        const SurveyRule& rule = *pipes.at("*");
        ASSERT_TRUE((section == SurveySection::Pipe     ? rule.pipe
                     : section == SurveySection::VertexPipe ? rule.vertexPipe
                                                            : rule.segmentPipe)
                        .has_value())
            << word;
        const auto& pipe = section == SurveySection::Pipe         ? *rule.pipe
                           : section == SurveySection::VertexPipe ? *rule.vertexPipe
                                                                  : *rule.segmentPipe;
        EXPECT_TRUE(pipe.active) << word;
        EXPECT_EQ(pipe.justify, "Obvert")
            << word << ": a string with a diameter is measured from its top";
        EXPECT_EQ(pipe.shape, "diameter") << word;
        EXPECT_EQ(pipe.size1, "$Diameter (mm)") << word << " sizes from an attribute";
        EXPECT_TRUE(pipe.size2.empty()) << word;
    }
}

TEST(KatanaStandard, TheSevenTextCodesAreStandardTextInMillimetresOfPaper)
{
    KS_LOAD();
    const auto texts = rulesOf(file, SurveySection::VertexTextStyle);
    std::map<std::string, double> sizes;
    for (const auto& [key, rule] : texts) {
        ASSERT_TRUE(rule->textStyle.has_value());
        EXPECT_EQ(rule->textStyle->textstyle, "Standard") << key;
        EXPECT_EQ(rule->textStyle->type, "paper") << key;
        EXPECT_EQ(rule->textStyle->colour, "katana note") << key;
        EXPECT_EQ(rule->textStyle->justifyX, "left") << key;
        EXPECT_EQ(rule->textStyle->justifyY, "bottom") << key;
        sizes[key] = rule->textStyle->size;
    }
    // The keys 1 to 5 and 7 are the sizes 1.5, 2.5, 3.5, 5, 7 and 10 mm; 8 is
    // the parcel label's 2.5 (census.py "textSizes").
    EXPECT_EQ(sizes, (std::map<std::string, double>{{"1", 1.5},
                                                    {"2", 2.5},
                                                    {"3", 3.5},
                                                    {"4", 5.0},
                                                    {"5", 7.0},
                                                    {"7", 10.0},
                                                    {"8", 2.5}}));
    // And each has a feature rule under its star that names the layer all text goes on.
    const auto features = rulesOf(file, SurveySection::Map);
    for (const char* key : {"1*", "2*", "3*", "4*", "5*", "7*", "8*"}) {
        ASSERT_EQ(features.count(key), 1u) << key;
        EXPECT_EQ(features.at(key)->model, "control/annotation/text") << key;
        EXPECT_FALSE(features.at(key)->breakline.has_value()) << key << " draws nothing of its own";
    }
}

TEST(KatanaStandard, EveryUtilityCodeNamesItsServiceAndNoOtherCodeClaimsOneButTheStormwaterOnes)
{
    KS_LOAD();
    std::set<std::string> disusedByStatus;
    std::size_t utilities = 0;
    std::size_t typedUtilities = 0;
    std::size_t stormwaterTyped = 0;
    std::map<std::string, std::size_t> perService;
    const auto attributes = rulesOf(file, SurveySection::StringAttribute);
    for (const auto& [key, feature] : rulesOf(file, SurveySection::Map)) {
        const auto found = attributes.find(key);
        const bool says =
            found != attributes.end() && hasAttributeNamed(*found->second, "utility.type");
        const bool inUtilities = feature->model.rfind("utilities/", 0) == 0;
        // The stormwater pits, structures, pipes and culverts are services too,
        // though they are catalogued with drainage.
        const bool stormwater = feature->model.rfind("drainage/pipes/", 0) == 0 ||
                                feature->model.rfind("drainage/structures/", 0) == 0;
        if (!inUtilities) {
            EXPECT_EQ(says, stormwater)
                << key
                << (stormwater ? " is stormwater and must say so"
                               : " is not a service and must not claim one");
            if (stormwater && says) {
                EXPECT_TRUE(hasAttribute(*found->second, "utility.type", "stormwater")) << key;
                ++stormwaterTyped;
            }
            continue;
        }
        ++utilities;
        // utilities/<service word>/<family>: a coded service and UTILITY DRAW share one tree.
        std::stringstream split(feature->model);
        std::string root;
        std::string service;
        std::getline(split, root, '/');
        std::getline(split, service, '/');
        if (key == "TS*") {
            // The reference gives it an attribute rule (its model zone), whose
            // entries are fixed, so it cannot also say its service.
            EXPECT_FALSE(says) << key;
            continue;
        }
        ASSERT_NE(found, attributes.end()) << key;
        EXPECT_TRUE(hasAttribute(*found->second, "utility.type", service))
            << key << " " << feature->model;
        ++typedUtilities;
        ++perService[service];
        if (hasAttributeNamed(*found->second, "utility.status")) {
            disusedByStatus.insert(key);
        }
    }
    EXPECT_EQ(utilities, 118u);
    EXPECT_EQ(typedUtilities, 117u);
    // The pits and structures of drainage and its pipes and culverts, all typed.
    EXPECT_EQ(stormwaterTyped, 69u);
    // The other library has no disused service, and neither has this one.
    EXPECT_TRUE(disusedByStatus.empty());
    // census.py "utilityTypes".
    EXPECT_EQ(perService, (std::map<std::string, std::size_t>{{"electricity", 23},
                                                              {"fire-service", 1},
                                                              {"fuel", 7},
                                                              {"gas", 15},
                                                              {"its", 4},
                                                              {"recycled-water", 8},
                                                              {"sewer", 13},
                                                              {"telecommunications", 22},
                                                              {"unknown", 8},
                                                              {"water", 16}}));
}

TEST(KatanaStandard, EveryServiceColourIsTheColourUtilityDrawUses)
{
    KS_LOAD();
    namespace sub = katana::survey::subsurface;
    // Every kind of service there is, by the word the codes say it is
    // (utility.type) and UTILITY DRAW names its layers by.
    const std::vector<sub::UtilityType> kinds{
        sub::UtilityType::Unknown,    sub::UtilityType::Electricity,
        sub::UtilityType::Telecommunications, sub::UtilityType::Gas,
        sub::UtilityType::Water,      sub::UtilityType::RecycledWater,
        sub::UtilityType::FireService, sub::UtilityType::Sewer,
        sub::UtilityType::Stormwater, sub::UtilityType::Fuel,
        sub::UtilityType::IntelligentTransport, sub::UtilityType::Other};
    std::map<std::string, sub::UtilityType> byWord;
    for (const sub::UtilityType kind : kinds) {
        byWord.emplace(std::string(katana::cad::utilities::utilityTypeWord(kind)), kind);
    }
    ASSERT_EQ(byWord.size(), 12u);

    const auto attributes = rulesOf(file, SurveySection::StringAttribute);
    std::set<sub::UtilityType> reached;
    std::size_t checked = 0;
    for (const auto& [key, feature] : rulesOf(file, SurveySection::Map)) {
        const auto found = attributes.find(key);
        if (found == attributes.end()) {
            continue;
        }
        for (const auto& attribute : found->second->attributes) {
            if (attribute.name != "utility.type") {
                continue;
            }
            // The 117 utility codes and the 69 stormwater pits, structures and pipes.
            ASSERT_EQ(byWord.count(attribute.value), 1u) << key << " says " << attribute.value;
            const sub::UtilityType kind = byWord.at(attribute.value);
            const Color expected = katana::cad::utilities::utilityTypeColour(kind);
            ASSERT_FALSE(feature->colour.empty()) << key << " draws in no colour";
            EXPECT_EQ(katana::entity::resolveColour(file.colours, feature->colour), expected)
                << key << " is " << attribute.value << " and draws in " << feature->colour;
            ++checked;
            reached.insert(kind);
        }
    }
    EXPECT_EQ(checked, 186u);
    // It reached every kind but "other", for which the library has no code,
    // so the loop is not passing for want of anything to compare.
    std::set<sub::UtilityType> expectedKinds(kinds.begin(), kinds.end());
    expectedKinds.erase(sub::UtilityType::Other);
    EXPECT_EQ(reached, expectedKinds);
}

TEST(KatanaStandard, EveryBuriedUtilityLineAsksForItsOwnerItsConditionAndItsDepthOrHeight)
{
    KS_LOAD();
    const auto attributes = rulesOf(file, SurveySection::StringAttribute);
    std::size_t lines = 0;
    std::size_t depth = 0;
    std::size_t height = 0;
    std::set<std::string> neither;
    for (const auto& [key, feature] : rulesOf(file, SurveySection::Map)) {
        if (feature->model.rfind("utilities/", 0) != 0 ||
            feature->breakline != katana::entity::SurveyBreakline::Line) {
            continue;
        }
        ++lines;
        const SurveyRule& rule = *attributes.at(key);
        if (key == "TS*") {
            // The telephone sump's only attribute rule is the model zone.
            EXPECT_FALSE(asksFor(rule, "Owner")) << key;
            continue;
        }
        EXPECT_TRUE(asksFor(rule, "Owner")) << key;
        EXPECT_TRUE(asksFor(rule, "Condition")) << key;
        if (asksFor(rule, "Depth (m)")) {
            ++depth;
        } else if (asksFor(rule, "Height (m)")) {
            ++height;
        } else {
            neither.insert(key);
        }
    }
    // 52 lines on a utilities layer. 38 are buried and ask for a depth; 4 are
    // strung above ground and ask for a height (UE*, UL*, UO*, OA*); the
    // outlines of structures and the stay wire ask for neither (a reservoir, a
    // pond, a pump-out station, a septic tank, a transformer cabinet, a pylon
    // footing, a telephone box, a junction box and a stay wire); and TS*.
    EXPECT_EQ(lines, 52u);
    EXPECT_EQ(depth, 38u);
    EXPECT_EQ(height, 4u);
    const std::set<std::string> outlines{"EC*", "EL*", "RE*", "SC*", "SP*",
                                         "SQ*", "ST*", "TX*", "US*"};
    EXPECT_EQ(neither, outlines);
    EXPECT_EQ(depth + height + neither.size() + 1u, lines);
}

TEST(KatanaStandard, AnAttributeWithNoValueIsAPromptAndOnlyAFewKindsAreApplied)
{
    KS_LOAD();
    std::size_t prompts = 0;
    std::size_t applied = 0;
    std::map<std::string, std::size_t> appliedNames;
    for (const auto& [key, rule] : rulesOf(file, SurveySection::StringAttribute)) {
        std::set<std::string> names;
        for (const auto& attribute : rule->attributes) {
            EXPECT_TRUE(attribute.type == "text" || attribute.type == "integer") << key;
            EXPECT_TRUE(names.insert(attribute.name).second)
                << key << " repeats " << attribute.name;
            if (attribute.value.empty()) {
                ++prompts;
            } else {
                ++applied;
                ++appliedNames[attribute.name];
                // Applied: the service word, a model zone, or a size the key itself
                // states (a whole number).
                const bool service = attribute.name == "utility.type";
                const bool zone = attribute.name == "Check Zone";
                const bool stated =
                    attribute.name == "Diameter (mm)" || attribute.name == "Height (mm)";
                EXPECT_TRUE(service || zone || stated) << key << " applies " << attribute.name;
                if (zone || stated) {
                    EXPECT_EQ(attribute.type, "integer") << key;
                    EXPECT_EQ(attribute.value.find_first_not_of("0123456789"), std::string::npos)
                        << key;
                }
            }
        }
    }
    // census.py: attributesApplied 244, attributesPrompted 641. The applied
    // are 186 service words, 12 model zones, 28 stated diameters (the 14
    // inverts PI01 to PI18 and the 14 pipes U1 to V8) and 18 stated heights
    // (the culverts D0 to D9 and the box culverts B0 to B9 that exist).
    EXPECT_EQ(applied, 244u);
    EXPECT_EQ(prompts, 641u);
    EXPECT_EQ(appliedNames, (std::map<std::string, std::size_t>{{"Check Zone", 12},
                                                                {"Diameter (mm)", 28},
                                                                {"Height (mm)", 18},
                                                                {"utility.type", 186}}));
    EXPECT_EQ(186u + 12u + 28u + 18u, 244u);
}

TEST(KatanaStandard, EveryDefinitionIsInPaperMillimetresAndStaysInsideTheBoundsOfTheDesign)
{
    KS_LOAD();
    file.library.forEach([&](const LineStyle& style) {
        EXPECT_EQ(style.units, StyleUnits::Paper) << style.name;
        EXPECT_EQ(style.factor, 1.0) << style.name;
        EXPECT_EQ(style.stretchMode, 0) << style.name;
        EXPECT_EQ(style.cycleMode, 0) << style.name;
        EXPECT_EQ(style.source, "Katana Standard") << style.name;
        EXPECT_FALSE(style.group.empty()) << style.name;
        const bool symbol = style.symbol;
        EXPECT_EQ(style.atVertices, symbol) << style.name;
        EXPECT_LE(style.strokes.size(), symbol ? 70u : 90u) << style.name;
        EXPECT_GE(style.strokes.size(), 2u) << style.name;
        if (symbol) {
            EXPECT_EQ(style.length, 0.0) << style.name << " is a symbol and has no period";
        } else {
            EXPECT_GT(style.length, 0.0) << style.name;
        }
        double cx = 0.0;
        double cy = 0.0;
        for (const auto& stroke : style.strokes) {
            if (stroke.op == StrokeOp::Move || stroke.op == StrokeOp::Draw) {
                cx = stroke.point.x;
                cy = stroke.point.y;
                EXPECT_TRUE(atMostThreeDecimals(cx) && atMostThreeDecimals(cy)) << style.name;
            }
            double lowX = cx;
            double highX = cx;
            double lowY = cy;
            double highY = cy;
            if (stroke.op == StrokeOp::Circle) {
                lowX -= stroke.radius;
                highX += stroke.radius;
                lowY -= stroke.radius;
                highY += stroke.radius;
            } else if (stroke.op == StrokeOp::Arc) {
                // The ends and the four axes the sweep crosses give the box.
                std::vector<double> angles{stroke.startAngle, stroke.endAngle};
                for (double a = -720.0; a <= 720.0; a += 90.0) {
                    if (a > std::min(stroke.startAngle, stroke.endAngle) &&
                        a < std::max(stroke.startAngle, stroke.endAngle)) {
                        angles.push_back(a);
                    }
                }
                const double pi = std::acos(-1.0);
                for (double a : angles) {
                    const double x = cx + std::fabs(stroke.radius) * std::cos(a * pi / 180.0);
                    const double y = cy + std::fabs(stroke.radius) * std::sin(a * pi / 180.0);
                    lowX = std::min(lowX, x);
                    highX = std::max(highX, x);
                    lowY = std::min(lowY, y);
                    highY = std::max(highY, y);
                }
            }
            if (stroke.op == StrokeOp::Circle || stroke.op == StrokeOp::Arc ||
                stroke.op == StrokeOp::Dot) {
                EXPECT_TRUE(atMostThreeDecimals(stroke.radius)) << style.name;
            }
            if (symbol) {
                continue;
            }
            // A cell closes on itself: x inside one period, y inside the band.
            EXPECT_GE(lowX, -1e-9) << style.name;
            EXPECT_LE(highX, style.length + 1e-9) << style.name;
            EXPECT_GE(lowY, -3.4 - 1e-9) << style.name;
            EXPECT_LE(highY, 3.4 + 1e-9) << style.name;
        }
    });
}

TEST(KatanaStandard, EveryDefinitionNameIsTitleCaseWithNoDigitAndNoStop)
{
    KS_LOAD();
    const std::set<std::string> small{"and", "of"};
    file.library.forEach([&](const LineStyle& style) {
        std::stringstream words(style.name);
        for (std::string word; std::getline(words, word, ' ');) {
            ASSERT_FALSE(word.empty()) << "'" << style.name << "'";
            EXPECT_TRUE(std::isupper(static_cast<unsigned char>(word.front())) ||
                        small.count(word) == 1)
                << style.name << ": '" << word << "'";
        }
        EXPECT_EQ(style.name.find_first_of("0123456789.-_/"), std::string::npos) << style.name;
    });
}

TEST(KatanaStandard, TheWriterWritesTheFileBackByteForByte)
{
    KS_LOAD();
    const auto written = katana::entity::customisationToJson(file);
    ASSERT_TRUE(written.ok()) << written.error().describe();
    EXPECT_EQ(written->size(), ks.text.size());
    EXPECT_TRUE(*written == ks.text) << "the committed bytes are not the writer's canonical form";
    // And once more: what the writer wrote reads back equal.
    const auto again = katana::entity::customisationFromJson(*written);
    ASSERT_TRUE(again.ok()) << again.error().describe();
    EXPECT_TRUE(*again == file);
}

TEST(KatanaStandard, TheFileSaysNothingOfLineworkOrAutomationAndNamesNoSource)
{
    KS_LOAD();
    // Absent is not the defaults: merged onto a session it must leave the
    // session's control words and switches alone.
    EXPECT_FALSE(file.linework.has_value());
    EXPECT_FALSE(file.automation.has_value());
    EXPECT_TRUE(file.sources.empty());
    EXPECT_FALSE(file.basedOn.has_value());
    EXPECT_EQ(ks.text.find("\"linework\""), std::string::npos);
    EXPECT_EQ(ks.text.find("\"automation\""), std::string::npos);
}

TEST(KatanaStandard, ANumberedStringAnswersItsKeysRuleAndATypoAnswersNone)
{
    KS_LOAD();
    const auto& map = file.map;
    // WM, WM1 and WM01 are one code: the digits are the string number.
    for (const char* name : {"WM", "WM1", "WM01", "WM12"}) {
        const auto match = map.lookup(name);
        EXPECT_TRUE(match.matched()) << name;
        EXPECT_EQ(match.resolved.model, "utilities/water/main") << name;
        EXPECT_EQ(match.resolved.linestyle, "Water Supply Pipe") << name;
        EXPECT_EQ(match.resolved.breakline, katana::entity::SurveyBreakline::Line) << name;
    }
    // A bulk bin is a line with a bin at every vertex: the one such key.
    const auto bin = map.lookup("BI2");
    EXPECT_EQ(bin.resolved.breakline, katana::entity::SurveyBreakline::Line);
    ASSERT_TRUE(bin.resolved.symbol.has_value());
    EXPECT_EQ(bin.resolved.symbol->style, "Litter Bin");
    // The general fence is a line with no symbol, drawn as posts on a wire.
    const auto fence = map.lookup("FE3");
    EXPECT_EQ(fence.resolved.linestyle, "Post and Wire Fence");
    EXPECT_FALSE(fence.resolved.symbol.has_value());
    // A gully pit is a point with a symbol, and an exact key: PGUL1 is none.
    const auto pit = map.lookup("PGUL");
    EXPECT_EQ(pit.kind, katana::entity::SurveyMatchKind::Exact);
    EXPECT_EQ(pit.resolved.breakline, katana::entity::SurveyBreakline::Point);
    ASSERT_TRUE(pit.resolved.symbol.has_value());
    EXPECT_EQ(pit.resolved.symbol->style, "Gully Grate");
    EXPECT_FALSE(map.lookup("PGUL1").matched());
    // The valve and the water main share a colour and not a layer.
    EXPECT_EQ(map.lookup("PWSV").resolved.colour, map.lookup("WM").resolved.colour);
    EXPECT_NE(map.lookup("PWSV").resolved.model, map.lookup("WM").resolved.model);
    // A longer prefix wins field by field over a shorter one: Z0* over Z*.
    const auto index = map.lookup("Z07");
    EXPECT_EQ(index.resolved.comment, "Index contour, regular");
    EXPECT_EQ(map.lookup("Z1").resolved.comment, "Heavy contour");
    // A code that no key's prefix fits is answered by the bare `*` alone, which
    // names no layer, linestyle or symbol, so it is no match. None of these
    // starts a key: PWSW (the keys of P are exact), QZ (only QQ* starts with Q),
    // WQ, W, U, and the digits 6 and 9 (the digit keys are 1 2 3 4 5 7 8).
    for (const char* typo : {"PWSW", "QZ", "WQ", "W", "U", "6", "9"}) {
        const auto match = map.lookup(typo);
        EXPECT_FALSE(match.matched()) << typo;
        EXPECT_EQ(match.kind, katana::entity::SurveyMatchKind::FallbackOnly) << typo;
    }
}

TEST(KatanaStandard, NoWordThatTheProjectKeepsOutOfNewTextIsInTheFile)
{
    KS_LOAD();
    const std::string text = lower(ks.text);
    for (const std::string& word : forbiddenWords()) {
        EXPECT_EQ(text.find(word), std::string::npos) << "the file holds a forbidden word";
    }
}

TEST(KatanaStandard, EveryDefinitionAndEveryCodeIsCataloguedInTheDocument)
{
    KS_LOAD();
    const std::string doc = slurp(KATANA_STANDARD_DOC);
    ASSERT_FALSE(doc.empty()) << "no document at " << KATANA_STANDARD_DOC;
    // The catalogue's tables open a LINE with the name, or the key, in
    // backticks. At the start of a line, so that a "Used by" cell holding the
    // one code that uses a definition does not count.
    std::size_t missing = 0;
    file.library.forEach([&](const LineStyle& style) {
        if (doc.find("\n| `" + style.name + "` |") == std::string::npos) {
            ++missing;
            ADD_FAILURE() << style.name << " is not catalogued";
        }
    });
    for (const std::string& key : file.map.keys()) {
        // Once in its table and once in the index by key.
        const std::string row = "\n| `" + key + "` |";
        const auto first = doc.find(row);
        ASSERT_NE(first, std::string::npos) << key << " is not catalogued";
        EXPECT_NE(doc.find(row, first + 1), std::string::npos) << key << " is not in the index";
    }
    EXPECT_EQ(missing, 0u);
    // A code is looked up by what it is called as often as by its key: every
    // legend label opens a line of the index by label, and the key that has
    // it is on that line.
    std::size_t labels = 0;
    for (const auto& [key, rule] : rulesOf(file, SurveySection::Map)) {
        ASSERT_FALSE(rule->comment.empty()) << key << " has a feature rule and no legend label";
        ++labels;
        const auto at = doc.find("\n| " + rule->comment + " | ");
        ASSERT_NE(at, std::string::npos) << key << ": '" << rule->comment << "' is not in the index";
        const auto end = doc.find('\n', at + 1);
        const std::string line = doc.substr(at, end == std::string::npos ? end : end - at);
        EXPECT_NE(line.find("`" + key + "`"), std::string::npos)
            << key << " is not on the line of its label '" << rule->comment << "'";
    }
    // The 572 keys that have a feature rule (census.py: rulesBySets feature).
    EXPECT_EQ(labels, 572u);
}

// ---- the keys of the NSW built-in ------------------------------------------------------------

TEST(KatanaStandard, EveryKeyOfTheNswBuiltInIsHereAndNoOtherKey)
{
    KS_LOAD();
    const Loaded& other = nsw();
    ASSERT_TRUE(other.problem.empty()) << other.problem;
    // Byte for byte: `AB`, `AB*`, `AB1*` and `*` are four keys. Both lists are
    // in name order.
    const std::vector<std::string> ours = file.map.keys();
    const std::vector<std::string> theirs = other.value.map.keys();
    // parity.py and tools/customisation_census.py: the other library has 632 keys.
    EXPECT_EQ(theirs.size(), 632u);
    std::vector<std::string> missing;
    std::set_difference(theirs.begin(), theirs.end(), ours.begin(), ours.end(),
                        std::back_inserter(missing));
    std::vector<std::string> extra;
    std::set_difference(ours.begin(), ours.end(), theirs.begin(), theirs.end(),
                        std::back_inserter(extra));
    EXPECT_TRUE(missing.empty()) << missing.size() << " keys are missing, the first "
                                 << (missing.empty() ? std::string() : missing.front());
    EXPECT_TRUE(extra.empty()) << extra.size() << " keys are extra, the first "
                               << (extra.empty() ? std::string() : extra.front());
    EXPECT_EQ(ours, theirs);
}

namespace {

// What a code makes of the rules that answer it, as a kind of entity: the
// things a field file's result depends on and a person does not see.
struct Kind {
    katana::entity::SurveyMatchKind matched = katana::entity::SurveyMatchKind::None;
    int breakline = -1; // -1 says nothing of it, 0 a point, 1 a line
    int tinable = -1;   // -1 says nothing of it, 0 stays out of a surface, 1 joins one
    bool hide = false;
    bool symbol = false;
    bool layerNamed = false;
    bool colourNamed = false;
    bool text = false;
    std::string textUnits;
    double textSize = 0.0;
    std::string textJustifyX;
    std::string textJustifyY;
    bool pipe = false;
    bool vertexPipe = false;
    bool segmentPipe = false;
    std::string pipeJustify;
    std::string pipeShape;
    bool pipeActive = false;
    bool pipeSize1 = false;
    bool pipeSize2 = false;
};

Kind kindOf(const Customisation& library, const std::string& code)
{
    const auto match = library.map.lookup(code);
    const SurveyRule& r = match.resolved;
    Kind k;
    k.matched = match.kind;
    if (r.breakline) {
        k.breakline = *r.breakline == katana::entity::SurveyBreakline::Line ? 1 : 0;
    }
    if (r.tinable) {
        k.tinable = *r.tinable ? 1 : 0;
    }
    k.hide = r.hide.value_or(false);
    k.symbol = r.symbol.has_value() && !r.symbol->style.empty();
    k.layerNamed = !r.model.empty();
    k.colourNamed = !r.colour.empty();
    if (r.textStyle) {
        k.text = true;
        k.textUnits = r.textStyle->type;
        k.textSize = r.textStyle->size;
        k.textJustifyX = r.textStyle->justifyX;
        k.textJustifyY = r.textStyle->justifyY;
    }
    k.pipe = r.pipe.has_value();
    k.vertexPipe = r.vertexPipe.has_value();
    k.segmentPipe = r.segmentPipe.has_value();
    if (r.pipe) {
        k.pipeJustify = r.pipe->justify;
        k.pipeShape = r.pipe->shape;
        k.pipeActive = r.pipe->active;
        k.pipeSize1 = !r.pipe->size1.empty();
        k.pipeSize2 = !r.pipe->size2.empty();
    }
    return k;
}

// The kinds of rule a key makes, in order of first appearance, without a
// second rule of a kind (the other library repeats rules and only the first
// of a kind sets a field). `extra` attribute rules this library adds to a
// service key the other gives none are left out when `dropExtraAttributes`.
std::vector<SurveySection> sectionsOf(const Customisation& library, const std::string& key,
                                      bool dropExtraAttributes, bool otherHasAttributes)
{
    std::vector<SurveySection> sections;
    for (const SurveyRule& rule : library.map.rules()) {
        if (rule.key != key) {
            continue;
        }
        if (std::find(sections.begin(), sections.end(), rule.section) == sections.end()) {
            sections.push_back(rule.section);
        }
    }
    if (dropExtraAttributes && !otherHasAttributes) {
        sections.erase(
            std::remove(sections.begin(), sections.end(), SurveySection::StringAttribute),
            sections.end());
    }
    return sections;
}

bool hasSection(const Customisation& library, const std::string& key, SurveySection section)
{
    return std::any_of(library.map.rules().begin(), library.map.rules().end(),
                       [&](const SurveyRule& r) { return r.key == key && r.section == section; });
}

} // namespace

TEST(KatanaStandard, FortyCodesAcrossAllClassesResolveToTheKindOfEntityTheNswBuiltInMakes)
{
    KS_LOAD();
    const Loaded& other = nsw();
    ASSERT_TRUE(other.problem.empty()) << other.problem;
    // Forty keys, chosen by hand to take every class (a class of Katana
    // Standard's own arrangement), every kind of rule and every oddity once or
    // more: a line, a point, a line that is also a point's symbol (BI*), a
    // key that only sets a surface flag, a hidden symbol (PBDY), a vertex
    // attribute (PNAL), a model zone (KB*, FE*, TS*), the text keys, the
    // layer-only key, the bare `*`, and prefix families that overlap (Z* and
    // Z0*, PT04 and PT04*). AD* is left out: it is the one key whose clean
    // form differs, and parity_exceptions.json says so.
    const std::vector<std::string> keys{
        "BT*",  "BE*",  "PBDY", "PNAL",                          // boundaries and cadastre
        "CZ*",  "AS*",  "PTMX",                                  // roads and pavements
        "S9*",  "AC*",  "BI*",  "PSGL",                          // street furniture and signs
        "KB*",  "U3*",  "PGUL", "POBV",                          // kerbs and drainage
        "RA*",  "PRBX",                                          // rail and transit
        "WM*",  "PWSV", "TS*",                                   // utilities
        "PT04", "PT04*", "JD*",                                  // vegetation
        "BU*",  "UT*",  "PFLG",                                  // buildings and structures
        "FE*",  "AG*",  "POST",                                  // fences and walls
        "Z0*",  "Z*",   "PSHT",                                  // terrain
        "PBMK", "4",    "8*",                                    // survey control and annotation
        "PHAZ", "HP*",                                           // miscellaneous
        "L8*",  "BF*",                                           // design and setting out
        "*",                                                     // the bare star
    };
    ASSERT_EQ(keys.size(), 40u);
    std::set<std::string> distinct(keys.begin(), keys.end());
    ASSERT_EQ(distinct.size(), 40u) << "the sample names a key twice";
    for (const std::string& key : keys) {
        SCOPED_TRACE("key " + key);
        ASSERT_TRUE(hasSection(file, key, SurveySection::Map) ||
                    hasSection(file, key, SurveySection::Tinable) ||
                    hasSection(file, key, SurveySection::VertexTextStyle) ||
                    hasSection(file, key, SurveySection::Pipe))
            << "the sample names a key this library lacks";
        ASSERT_FALSE(other.value.map.rules().empty());

        // 1. The same ordered kinds of rule. This library adds one attributes
        //    rule to a service key where the other has none; that is left out.
        const bool otherHasAttributes =
            hasSection(other.value, key, SurveySection::StringAttribute);
        EXPECT_EQ(sectionsOf(file, key, true, otherHasAttributes),
                  sectionsOf(other.value, key, false, true));

        // 2. The first attributes rule of the other library: the same entries
        //    (a type and whether a value is given), no more and no fewer.
        if (otherHasAttributes) {
            const SurveyRule* theirs = nullptr;
            for (const SurveyRule& r : other.value.map.rules()) {
                if (r.key == key && r.section == SurveySection::StringAttribute) {
                    theirs = &r;
                    break;
                }
            }
            const SurveyRule* ours = nullptr;
            for (const SurveyRule& r : file.map.rules()) {
                if (r.key == key && r.section == SurveySection::StringAttribute) {
                    ours = &r;
                    break;
                }
            }
            ASSERT_NE(theirs, nullptr);
            ASSERT_NE(ours, nullptr);
            ASSERT_EQ(ours->attributes.size(), theirs->attributes.size());
            for (std::size_t i = 0; i < theirs->attributes.size(); ++i) {
                EXPECT_EQ(ours->attributes[i].type, theirs->attributes[i].type) << "entry " << i;
                EXPECT_EQ(ours->attributes[i].value.empty(), theirs->attributes[i].value.empty())
                    << "entry " << i;
            }
        }

        // 3. The same code makes the same kind of entity. A code that the key
        //    answers: the key without its star, or the key itself.
        const std::string code = key == "*" ? std::string("QZ")
                                 : key.back() == '*' ? key.substr(0, key.size() - 1)
                                                     : key;
        const Kind a = kindOf(other.value, code);
        const Kind b = kindOf(file, code);
        EXPECT_EQ(b.matched, a.matched) << code;
        EXPECT_EQ(b.breakline, a.breakline) << code << ": a line in one and a point in the other";
        EXPECT_EQ(b.tinable, a.tinable) << code << ": the surface flag";
        EXPECT_EQ(b.hide, a.hide) << code;
        EXPECT_EQ(b.symbol, a.symbol) << code << ": a symbol in one and none in the other";
        EXPECT_EQ(b.layerNamed, a.layerNamed) << code;
        EXPECT_EQ(b.colourNamed, a.colourNamed) << code;
        EXPECT_EQ(b.text, a.text) << code;
        EXPECT_EQ(b.textUnits, a.textUnits) << code;
        EXPECT_EQ(b.textSize, a.textSize) << code;
        EXPECT_EQ(b.textJustifyX, a.textJustifyX) << code;
        EXPECT_EQ(b.textJustifyY, a.textJustifyY) << code;
        EXPECT_EQ(b.pipe, a.pipe) << code;
        EXPECT_EQ(b.vertexPipe, a.vertexPipe) << code;
        EXPECT_EQ(b.segmentPipe, a.segmentPipe) << code;
        EXPECT_EQ(b.pipeJustify, a.pipeJustify) << code;
        EXPECT_EQ(b.pipeShape, a.pipeShape) << code;
        EXPECT_EQ(b.pipeActive, a.pipeActive) << code;
        EXPECT_EQ(b.pipeSize1, a.pipeSize1) << code;
        EXPECT_EQ(b.pipeSize2, a.pipeSize2) << code;
    }
    // The sample reaches what it claims to: it holds a hidden symbol, a text
    // key, a pipe, a key with only a surface flag and a line with a symbol.
    EXPECT_TRUE(kindOf(file, "PBDY").hide);
    EXPECT_TRUE(kindOf(other.value, "PBDY").hide);
    EXPECT_TRUE(kindOf(file, "4").text);
    EXPECT_TRUE(kindOf(file, "QZ").pipe);
    EXPECT_EQ(kindOf(file, "PT04").tinable, 0);
    EXPECT_TRUE(kindOf(file, "BI1").symbol);
    EXPECT_EQ(kindOf(file, "BI1").breakline, 1);
}

namespace {

// The names of every attribute a library's rules apply or ask for, whichever
// list of a rule holds them.
std::set<std::string> attributeNamesOf(const Customisation& library)
{
    std::set<std::string> names;
    for (const SurveyRule& rule : library.map.rules()) {
        for (const auto* list :
             {&rule.attributes, &rule.vertexAttributes, &rule.segmentAttributes}) {
            for (const auto& attribute : *list) {
                names.insert(attribute.name);
            }
        }
    }
    return names;
}

// The text styles a library's text rules name.
std::set<std::string> textStylesOf(const Customisation& library)
{
    std::set<std::string> styles;
    for (const SurveyRule& rule : library.map.rules()) {
        if (rule.textStyle && !rule.textStyle->textstyle.empty()) {
            styles.insert(rule.textStyle->textstyle);
        }
    }
    return styles;
}

// The letters and digits of a name, in lower case, and nothing else: two names
// are the same name when these are, so a blank, a hyphen or a comma more or
// less does not make a name another name (originality.py, "squeeze"). ASCII
// by hand, so that the answer does not depend on the locale.
std::string squeezed(std::string_view text)
{
    std::string out;
    for (const char c : text) {
        if ((c >= 'a' && c <= 'z') || (c >= '0' && c <= '9')) {
            out.push_back(c);
        } else if (c >= 'A' && c <= 'Z') {
            out.push_back(static_cast<char>(c - 'A' + 'a'));
        }
    }
    return out;
}

// Every shorter form of a definition name that taking a lead-in off its front
// gives: a word of two or more capitals and digits, with or without a single
// capital before it, when more follows. "E WALL Stone Line" gives "Stone
// Line"; "AB CD Peg" gives "CD Peg" and "Peg"; "Stone Line" gives nothing
// (originality.py, "without_lead_ins").
std::vector<std::string> withoutLeadIns(const std::string& name)
{
    static const std::regex leadIn("^(?:[A-Z]\\s+)?[A-Z0-9]{2,}\\s+(?=\\S)");
    std::vector<std::string> shorter;
    std::string rest = name;
    for (;;) {
        std::string next =
            std::regex_replace(rest, leadIn, "", std::regex_constants::format_first_only);
        if (next == rest) {
            return shorter;
        }
        shorter.push_back(next);
        rest = std::move(next);
    }
}

// A legend label less the lead-ins the other library may give one: a "[KEY]"
// in brackets, then a single capital and a word of two to eight capitals and
// digits (originality.py, "clean_label").
std::string withoutLabelLeadIns(const std::string& comment)
{
    static const std::regex key("^\\[[^\\]]*\\]\\s*");
    static const std::regex leadIn("^[A-Z]\\s+[A-Z0-9]{2,8}\\s+");
    std::string text = comment;
    text.erase(0, text.find_first_not_of(" \t\r\n")); // all of it when it is all blanks
    text = std::regex_replace(text, key, "", std::regex_constants::format_first_only);
    return std::regex_replace(text, leadIn, "", std::regex_constants::format_first_only);
}

// Whether the survey code keys of the library compared with are promised to
// be this library's: they are the NSW customisation's, on the owner's
// decision, and nothing is said of any other library a build compiles in.
enum class Keys { TheSame, NotCompared };

// Counts only, found at run time: nothing here names what the other holds.
// Katana Standard was made so that nothing but the survey code keys is the
// other's: no definition name, group path, layer path, colour name, attribute
// name or text style of it is one of the other's, whole, and (with the NSW
// customisation) its keys are the other's exactly.
void expectNothingButTheKeysInCommonWith(const Customisation& file, const Customisation& other,
                                         Keys keys)
{
    std::size_t sharedNames = 0;
    file.library.forEach([&](const LineStyle& style) {
        sharedNames += other.library.contains(style.name) ? 1 : 0;
    });
    EXPECT_EQ(sharedNames, 0u) << "definition names in common";

    // Nor is a definition called what the other calls a thing in any of the
    // three ways it does: by a definition name (equal as letters and digits,
    // whatever the blanks and punctuation), by such a name less its lead-in of
    // capitals, or by a legend label. A symbol named after the other's label
    // for the thing would be the other's name for it as much as one named
    // after its symbol (tools/katana_standard/originality.py counts all three
    // at none, and a label of this library that is a definition name of the
    // other's at none too).
    std::set<std::string> otherNames;
    std::set<std::string> otherNamesLessLeadIn;
    other.library.forEach([&](const LineStyle& style) {
        otherNames.insert(squeezed(style.name));
        for (const std::string& shorter : withoutLeadIns(style.name)) {
            otherNamesLessLeadIn.insert(squeezed(shorter));
        }
    });
    std::set<std::string> otherLabels;
    for (const auto& rule : other.map.rules()) {
        otherLabels.insert(squeezed(withoutLabelLeadIns(rule.comment)));
    }
    for (std::set<std::string>* names : {&otherNames, &otherNamesLessLeadIn, &otherLabels}) {
        names->erase(std::string{}); // a name of punctuation alone is no name
    }
    std::size_t namesEqual = 0;
    std::size_t namesLessLeadIn = 0;
    std::size_t namesThatAreLabels = 0;
    std::size_t definitions = 0;
    file.library.forEach([&](const LineStyle& style) {
        const std::string name = squeezed(style.name);
        ++definitions;
        namesEqual += otherNames.count(name);
        namesLessLeadIn += otherNamesLessLeadIn.count(name);
        namesThatAreLabels += otherLabels.count(name);
    });
    EXPECT_GT(definitions, 0u);
    EXPECT_EQ(namesEqual, 0u) << "definition names in common, as letters and digits";
    EXPECT_EQ(namesLessLeadIn, 0u) << "definition names that are the other's less a lead-in";
    EXPECT_EQ(namesThatAreLabels, 0u) << "definition names that are legend labels of the other";

    std::set<std::string> otherGroups;
    for (const std::string& group : katana::entity::styleGroups(other.library)) {
        otherGroups.insert(group);
    }
    std::size_t sharedGroups = 0;
    for (const std::string& group : katana::entity::styleGroups(file.library)) {
        sharedGroups += otherGroups.count(group);
    }
    EXPECT_EQ(sharedGroups, 0u) << "group paths in common";

    // The keys are the one thing shared, and shared whole.
    if (keys == Keys::TheSame) {
        std::set<std::string> otherKeys;
        for (const std::string& key : other.map.keys()) {
            otherKeys.insert(key);
        }
        std::size_t sharedKeys = 0;
        for (const std::string& key : file.map.keys()) {
            sharedKeys += otherKeys.count(key);
        }
        EXPECT_EQ(sharedKeys, otherKeys.size()) << "survey code keys in common";
        EXPECT_EQ(sharedKeys, file.map.keys().size()) << "survey code keys in common";
    }

    // No attribute is called what the other calls one, and no text style is
    // the other's (tools/katana_standard/originality.py counts both at none).
    const std::set<std::string> otherAttributes = attributeNamesOf(other);
    std::size_t sharedAttributes = 0;
    for (const std::string& name : attributeNamesOf(file)) {
        sharedAttributes += otherAttributes.count(name);
    }
    EXPECT_EQ(sharedAttributes, 0u) << "attribute names in common";
    const std::set<std::string> otherTextStyles = textStylesOf(other);
    std::size_t sharedTextStyles = 0;
    for (const std::string& style : textStylesOf(file)) {
        sharedTextStyles += otherTextStyles.count(style);
    }
    EXPECT_EQ(sharedTextStyles, 0u) << "text styles in common";

    std::size_t sharedColours = 0;
    for (const auto& entry : file.colours.entries()) {
        sharedColours += other.colours.find(entry.name).has_value() ? 1 : 0;
    }
    EXPECT_EQ(sharedColours, 0u) << "colour names in common";

    // Nor is any colour VALUE one of the built-in's, whether from its table
    // or named by a rule (a name it keeps in the table, or a standard name).
    std::set<std::string> otherValues;
    for (const auto& entry : other.colours.entries()) {
        otherValues.insert(entry.colour.toHex());
    }
    for (const auto& rule : other.map.rules()) {
        for (const std::string& name :
             {rule.colour, rule.symbol ? rule.symbol->colour : std::string{},
              rule.textStyle ? rule.textStyle->colour : std::string{}}) {
            if (const auto colour = katana::entity::resolveColour(other.colours, name)) {
                otherValues.insert(colour->toHex());
            }
        }
    }
    // One value is not held to this: communications' own, which AS 5488 names
    // white and the one table (utilityTypeColour) therefore makes pure white.
    // White is the colour every drawing system starts from, so a built-in that
    // draws in it shares the value without sharing a design. The exemption is
    // that colour and no other white-ish one, it is one library colour, and the
    // other 27 values are what shows the two are independent: they stay at none
    // in common. (This is the one expectation of the colour work that was
    // loosened after a run showed the collision, and it is the owner's to
    // accept; docs/katana_standard.md says what the comparison excuses. A
    // near-white would also print black, but it would not be AS 5488's white.)
    const Color communications = katana::cad::utilities::utilityTypeColour(
        katana::survey::subsurface::UtilityType::Telecommunications);
    ASSERT_EQ(communications.toHex(), (Color{255, 255, 255, 255}.toHex()))
        << "AS 5488 communications is white";
    std::size_t sharedValues = 0;
    std::size_t exempt = 0;
    for (const auto& entry : file.colours.entries()) {
        if (entry.colour.toHex() == communications.toHex()) {
            ++exempt;
            continue;
        }
        sharedValues += otherValues.count(entry.colour.toHex());
    }
    EXPECT_EQ(exempt, 1u) << "the one white is communications";
    EXPECT_EQ(sharedValues, 0u) << "colour values in common";

    std::set<std::string> otherLayers;
    for (const auto& rule : other.map.rules()) {
        if (!rule.model.empty()) {
            otherLayers.insert(rule.model);
        }
    }
    std::size_t sharedLayers = 0;
    for (const auto& [key, rule] : rulesOf(file, SurveySection::Map)) {
        sharedLayers += otherLayers.count(rule->model);
    }
    EXPECT_EQ(sharedLayers, 0u) << "layer paths in common";

    // And no legend label is one of the other's: the labels are the comments
    // of the feature rules, the other's with its own lead-ins taken off (a
    // "[KEY]", then a single capital and a word of capitals), compared as
    // letters and digits. Nor is a label a definition name of the other's,
    // whole or less its lead-in.
    std::size_t sharedLabels = 0;
    std::size_t labelsThatAreNames = 0;
    for (const auto& [key, rule] : rulesOf(file, SurveySection::Map)) {
        const std::string label = squeezed(rule->comment);
        sharedLabels += otherLabels.count(label);
        labelsThatAreNames += otherNames.count(label) + otherNamesLessLeadIn.count(label);
    }
    EXPECT_EQ(sharedLabels, 0u) << "legend labels in common";
    EXPECT_EQ(labelsThatAreNames, 0u) << "legend labels that are definition names of the other";
}

} // namespace

TEST(KatanaStandard, NothingButTheKeysIsSharedWithAnyOtherCompiledInBuiltIn)
{
    KS_LOAD();
    // EVERY built-in this build compiled in but this one: Katana Standard is
    // the default now, so the other is an alternate, and a build may carry
    // several. Skipped only where there is none to compare with. The keys are
    // promised the same for the NSW customisation alone, which is known here
    // by the name the tracked file gives itself; of any other alternate a
    // build was given, nothing is said about its keys.
    const Loaded& tracked = nsw();
    ASSERT_TRUE(tracked.problem.empty()) << tracked.problem;
    std::size_t compared = 0;
    for (const katana::cad::BuiltInEntry& entry : katana::cad::builtIns()) {
        if (!entry.builtIn.customisation || entry.builtIn.customisation->name == file.name) {
            continue;
        }
        ++compared;
        SCOPED_TRACE("compared with the built-in number " + std::to_string(compared));
        const bool isTheNswOne = entry.builtIn.customisation->name == tracked.value.name;
        expectNothingButTheKeysInCommonWith(file, *entry.builtIn.customisation,
                                            isTheNswOne ? Keys::TheSame : Keys::NotCompared);
    }
    if (compared == 0) {
        GTEST_SKIP() << "this build has no other customisation compiled in to compare with";
    }
}

TEST(KatanaStandard, NothingButTheKeysIsSharedWithTheNswFileTrackedBesideIt)
{
    KS_LOAD();
    // The same comparison against the tracked file, so that it runs in every
    // build and not only in one that compiled the other built-in in.
    const Loaded& other = nsw();
    ASSERT_TRUE(other.problem.empty()) << other.problem;
    expectNothingButTheKeysInCommonWith(file, other.value, Keys::TheSame);
}
