// Katana Standard: the original customisation tracked as
// resources/customisation/katana-standard.customisation.json
// (docs/katana_standard.md).
//
// The tests read the COMMITTED file with the strict reader and hold what the
// library promises, so a regenerated file that drifted from its design fails
// here and not in somebody's drawing. Every figure is from a source that is
// not this program:
//
//   * the counts of the twelve classes and the 245 codes are the taxonomy of
//     the design (3 + 3 + 5 + 4 + 3 + 10 + 3 + 3 + 3 + 5 + 4 + 3 = 49
//     subgroups; 18 + 19 + 17 + 21 + 10 + 60 + 16 + 15 + 17 + 23 + 20 + 9 =
//     245 codes), written down before the file was made;
//   * the stroke totals, the number of rules of each kind and the prompts are
//     counted by tools/katana_standard/census.py and
//     tools/customisation_census.py, which read the file with the standard
//     `json` module and share no code with Katana;
//   * contrast is the WCAG 2.x formula, a published standard;
//   * a hand-worked check says why a figure is what it is where it can: 245
//     codes make 245 feature rules and 245 surface rules; 125 of them draw a
//     symbol (every code but the 114 plain lines and the 6 text codes); 77
//     attributes are applied (60 utility codes say their service, the 11
//     stormwater pits and pipes say theirs, 6 disused ones say so) and every
//     other is a prompt; 140 codes carry attributes (the 245 less 105 with none).
//   * the palette's two floors are the WCAG contrast formula and the OKLab
//     distance of Bjorn Ottosson's published matrices, written out below.
//
// Nothing here names a definition, a code or a colour of the reference
// customisation: where the two are compared it is by count, at run time.

#include <gtest/gtest.h>

#include <algorithm>
#include <array>
#include <cctype>
#include <cmath>
#include <cstddef>
#include <fstream>
#include <iterator>
#include <map>
#include <regex>
#include <set>
#include <sstream>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/customisation_host.hpp"
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

// The committed file, read once. `problem` is set when it cannot be read, so
// that every test says why instead of crashing on an empty value.
struct Standard {
    std::string text;
    Customisation value;
    std::string problem;
};

const Standard& standard()
{
    static const Standard loaded = [] {
        Standard s;
        s.text = slurp(KATANA_STANDARD_JSON);
        if (s.text.empty()) {
            s.problem = std::string("no file at ") + KATANA_STANDARD_JSON;
            return s;
        }
        auto read = katana::entity::customisationFromJson(s.text);
        if (!read.ok()) {
            s.problem = read.error().describe();
            return s;
        }
        s.value = std::move(*read);
        return s;
    }();
    return loaded;
}

// The rules of one section, by key without its `*`.
std::map<std::string, const SurveyRule*> rulesOf(const Customisation& c, SurveySection section)
{
    std::map<std::string, const SurveyRule*> found;
    for (const SurveyRule& rule : c.map.rules()) {
        if (rule.section == section) {
            found.emplace(rule.key.substr(0, rule.key.size() - 1), &rule);
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
    const Standard& ks = standard();                                                               \
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
    EXPECT_EQ(file.library.size(), 186u);
    EXPECT_EQ(linestyles, 73u);
    EXPECT_EQ(symbols, 113u);
    EXPECT_EQ(atVertices, 113u) << "every symbol is at vertices and no linestyle is";
    // census.py: arc 84, circle 83, dot 38, draw 1113, move 586, text 43.
    EXPECT_EQ(strokes[StrokeOp::Arc], 84u);
    EXPECT_EQ(strokes[StrokeOp::Circle], 83u);
    EXPECT_EQ(strokes[StrokeOp::Dot], 38u);
    EXPECT_EQ(strokes[StrokeOp::Draw], 1113u);
    EXPECT_EQ(strokes[StrokeOp::Move], 586u);
    EXPECT_EQ(strokes[StrokeOp::Text], 43u);
    EXPECT_EQ(strokes[StrokeOp::Pen], 0u) << "no stroke names a pen: the rule gives the colour";
    EXPECT_EQ(katana::entity::styleGroups(file.library).size(), 49u);
    EXPECT_EQ(file.map.size(), 770u);
    EXPECT_EQ(file.map.keys().size(), 245u);
    EXPECT_EQ(file.colours.size(), 28u);
}

TEST(KatanaStandard, EveryClassHoldsTheNumberOfCodesItsTaxonomySays)
{
    KS_LOAD();
    std::map<char, std::size_t> perClass;
    for (const std::string& key : file.map.keys()) {
        ++perClass[key.front()];
    }
    const std::map<char, std::size_t> taxonomy{{'C', 18}, {'R', 19}, {'S', 17}, {'K', 21},
                                               {'T', 10}, {'U', 60}, {'V', 16}, {'B', 15},
                                               {'F', 17}, {'G', 23}, {'M', 20}, {'X', 9}};
    EXPECT_EQ(perClass, taxonomy);
    std::size_t total = 0;
    for (const auto& [letter, count] : taxonomy) {
        total += count;
    }
    EXPECT_EQ(total, 245u);
}

TEST(KatanaStandard, EveryKeyIsThreeCapitalLettersAndEveryCodeHasItsOwnPrefixRule)
{
    KS_LOAD();
    const std::regex shape("[A-Z]{3}\\*");
    std::set<std::pair<std::string, SurveySection>> seen;
    for (const SurveyRule& rule : file.map.rules()) {
        EXPECT_TRUE(std::regex_match(rule.key, shape)) << "key " << rule.key;
        EXPECT_TRUE(seen.emplace(rule.key, rule.section).second)
            << rule.key << " is written twice in one section";
    }
    // Every key owns exactly one feature rule and one surface rule, so a typo
    // answers with nothing and no rule is shadowed.
    EXPECT_EQ(rulesOf(file, SurveySection::Map).size(), 245u);
    EXPECT_EQ(rulesOf(file, SurveySection::Tinable).size(), 245u);
}

TEST(KatanaStandard, ARuleOfEachKindIsWrittenForTheCodesThatNeedIt)
{
    KS_LOAD();
    std::map<SurveySection, std::size_t> bySection;
    std::size_t lines = 0;
    std::size_t points = 0;
    for (const SurveyRule& rule : file.map.rules()) {
        ++bySection[rule.section];
        if (rule.section == SurveySection::Map) {
            (rule.breakline == katana::entity::SurveyBreakline::Line ? lines : points) += 1;
        }
    }
    EXPECT_EQ(bySection[SurveySection::Map], 245u);
    EXPECT_EQ(bySection[SurveySection::Tinable], 245u);
    EXPECT_EQ(bySection[SurveySection::Pipe], 9u);
    EXPECT_EQ(bySection[SurveySection::VertexTextStyle], 6u);
    EXPECT_EQ(bySection[SurveySection::StringAttribute], 140u);
    EXPECT_EQ(bySection[SurveySection::VertexPipe], 0u);
    EXPECT_EQ(bySection[SurveySection::SegmentPipe], 0u);
    EXPECT_EQ(bySection[SurveySection::VertexAttribute], 0u);
    // 123 codes draw a line: 114 plain lines and 9 that carry a symbol at
    // every vertex. The other 122 are points: 116 symbols and 6 texts.
    EXPECT_EQ(lines, 123u);
    EXPECT_EQ(points, 122u);
    // A symbol rule for every code but the 114 plain lines and the 6 texts:
    // 245 - 114 - 6.
    EXPECT_EQ(bySection[SurveySection::VertexSymbol], 125u);
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

TEST(KatanaStandard, EveryColourIsReadableOnThePlanGroundAndOnWhitePaper)
{
    KS_LOAD();
    const Color ground{0x1E, 0x23, 0x29, 255};
    const Color paper{255, 255, 255, 255};
    for (const auto& entry : file.colours.entries()) {
        // WCAG 2.x asks 3:1 of a graphical object against its ground.
        EXPECT_GE(contrast(entry.colour, ground), 3.0) << entry.name << " on the ground";
        EXPECT_GE(contrast(entry.colour, paper), 3.0) << entry.name << " on paper";
    }
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
    // Tree, planting, ground, building, wall and communications: a 0.13 to
    // 0.18 mm line of one of these is the faintest thing on a plan.
    for (const char* name : {"katana tree", "katana planting", "katana ground", "katana building",
                             "katana wall", "katana telecom"}) {
        const auto colour = file.colours.find(name);
        ASSERT_TRUE(colour.has_value()) << name;
        EXPECT_GE(contrast(*colour, ground), 3.25) << name << " on the ground";
        EXPECT_GE(contrast(*colour, paper), 3.25) << name << " on paper";
    }
}

TEST(KatanaStandard, ThreeKerbLinesAreThreePatternsAndTheReturnIsTheTopEdgeCurved)
{
    KS_LOAD();
    const auto map = rulesOf(file, SurveySection::Map);
    // The face's top edge and the return round a corner are one feature, so
    // they share the plain line; the gutter lip and the back edge are other
    // things and each has a pattern of its own, since no pen is applied yet.
    EXPECT_EQ(map.at("KKT")->linestyle, "continuous");
    EXPECT_EQ(map.at("KKR")->linestyle, "continuous");
    EXPECT_EQ(map.at("KKL")->linestyle, "Gutter Lip Line");
    EXPECT_EQ(map.at("KKB")->linestyle, "Kerb Back Line");
    for (const char* name : {"Gutter Lip Line", "Kerb Back Line"}) {
        const LineStyle* style = file.library.find(name);
        ASSERT_NE(style, nullptr) << name;
        EXPECT_FALSE(style->symbol) << name;
    }
}

TEST(KatanaStandard, EveryLayerIsThreeLevelsOfLowerCaseWordsAndThereAreOneHundredAndThirty)
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
    EXPECT_EQ(layers.size(), 130u);
}

TEST(KatanaStandard, EveryGroupIsTwoLevelsDeepAndThereAreFiftyInAll)
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
    // 49 subgroups of the taxonomy, each a rule group. The definitions' 49
    // are the same but for two: the text codes (Survey Control and
    // Annotation/Text and Notes) have no definition, and the shapes every
    // service shares have a group no code has (Utilities/All Services).
    // 49 + 1 = 50 groups in all.
    EXPECT_EQ(ofRules.size(), 49u);
    EXPECT_EQ(ofDefinitions.size(), 49u);
    EXPECT_EQ(everyGroup.size(), 50u);
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
    // census.py: 0.13 18, 0.18 46, 0.25 137, 0.35 40, 0.50 4 - and 0.70, the
    // sixth of the scale, reserved. Nine codes moved from the hairline to the
    // fine pen when the planting and ground colours were made faintest on
    // paper (the lawn, bed and crop edges and the six ground readings), so
    // 27 - 9 = 18 and 37 + 9 = 46.
    const std::map<std::string, std::size_t> expected{
        {"0.13", 18}, {"0.18", 46}, {"0.25", 137}, {"0.35", 40}, {"0.50", 4}};
    EXPECT_EQ(weights, expected);
    EXPECT_EQ(weights.count("0.70"), 0u);
    // Nothing in the planting or the ground colours is lighter than the fine pen.
    for (const auto& [key, rule] : rulesOf(file, SurveySection::Map)) {
        if (rule->colour == "katana planting" || rule->colour == "katana ground") {
            EXPECT_NE(rule->weight, "0.13") << key;
        }
    }
}

TEST(KatanaStandard, OnlyTheGroundShotAndTheGroundReadingAreHidden)
{
    KS_LOAD();
    std::set<std::string> hidden;
    for (const auto& [key, rule] : rulesOf(file, SurveySection::VertexSymbol)) {
        if (rule->hide.value_or(false)) {
            hidden.insert(key);
        }
    }
    // GPG and GPR: points shown by their level, not by a mark.
    EXPECT_EQ(hidden, (std::set<std::string>{"GPG", "GPR"}));
}

TEST(KatanaStandard, TheGroundAndTheHardEdgesGoIntoASurfaceAndEverythingElseDoesNot)
{
    KS_LOAD();
    std::set<std::string> yes;
    for (const auto& [key, rule] : rulesOf(file, SurveySection::Tinable)) {
        ASSERT_TRUE(rule->tinable.has_value()) << key << " says nothing of surfaces";
        if (*rule->tinable) {
            yes.insert(key);
        }
    }
    EXPECT_EQ(yes.size(), 33u);
    // By hand, one of each family the design lists: a ground shot, a spot
    // level, a kerb top, a gutter, a carriageway edge, a path edge, a bank
    // top, a water edge, a platform edge, a ballast edge.
    for (const char* key : {"GPG", "GPS", "KKT", "KKL", "RCE", "RPD", "GBB", "GWE", "TPE", "TTB"}) {
        EXPECT_EQ(yes.count(key), 1u) << key;
    }
    // And one of each that is not: the exclusion boundary (a surface's own
    // limit, not its shape), a title boundary, a fence, a tree, a control
    // mark, a water main.
    for (const char* key : {"GBX", "CBT", "FFP", "VTB", "MCC", "UWM"}) {
        EXPECT_EQ(yes.count(key), 0u) << key;
    }
}

TEST(KatanaStandard, NineCodesAreMeantAsPipesAndTheGravityOnesJustifyOnTheInvert)
{
    KS_LOAD();
    const auto pipes = rulesOf(file, SurveySection::Pipe);
    std::set<std::string> keys;
    for (const auto& [key, rule] : pipes) {
        keys.insert(key);
        ASSERT_TRUE(rule->pipe.has_value());
        EXPECT_TRUE(rule->pipe->active) << key;
        EXPECT_EQ(rule->pipe->size1.rfind('$', 0), 0u) << key << " sizes from an attribute";
    }
    // The stormwater pipe, the box culvert, the water, sewer (gravity),
    // pressure sewer, gas, recycled water, fire and fuel mains.
    EXPECT_EQ(keys, (std::set<std::string>{"KCP", "KCB", "UWM", "USM", "USR", "UGM", "URM", "UFM",
                                           "UPM"}));
    for (const char* key : {"KCP", "KCB", "USM"}) {
        EXPECT_EQ(pipes.at(key)->pipe->justify, "Invert") << key << " is gravity: its level is the invert";
    }
    for (const char* key : {"UWM", "USR", "UGM", "URM", "UFM", "UPM"}) {
        EXPECT_EQ(pipes.at(key)->pipe->justify, "Centre") << key << " is a pressure main";
    }
    EXPECT_EQ(pipes.at("KCB")->pipe->shape, "culvert");
    EXPECT_EQ(pipes.at("KCB")->pipe->size2, "$Height (mm)");
}

TEST(KatanaStandard, TheSixTextCodesAreStandardTextInMillimetresOfPaper)
{
    KS_LOAD();
    const auto texts = rulesOf(file, SurveySection::VertexTextStyle);
    std::set<std::string> keys;
    for (const auto& [key, rule] : texts) {
        keys.insert(key);
        ASSERT_TRUE(rule->textStyle.has_value());
        EXPECT_EQ(rule->textStyle->textstyle, "Standard") << key;
        EXPECT_EQ(rule->textStyle->type, "paper") << key;
        EXPECT_GT(rule->textStyle->size, 0.0) << key;
        EXPECT_LE(rule->textStyle->size, 5.0) << key << " is at most the heading's size";
    }
    EXPECT_EQ(keys, (std::set<std::string>{"MTL", "MTN", "MTH", "MTS", "MTR", "MTC"}));
}

TEST(KatanaStandard, EveryUtilityCodeNamesItsServiceAndADisusedCodeSaysSo)
{
    KS_LOAD();
    // The service letter and the word Katana's utility tools use for it.
    const std::map<char, std::string> service{
        {'W', "water"},        {'S', "sewer"},          {'G', "gas"},  {'E', "electricity"},
        {'C', "telecommunications"}, {'R', "recycled-water"}, {'F', "fire-service"},
        {'P', "fuel"},         {'I', "its"},            {'X', "unknown"}};
    std::set<std::string> disusedByLayer;
    std::set<std::string> disusedByStatus;
    std::size_t utilities = 0;
    std::size_t stormwaterTyped = 0;
    const auto attributes = rulesOf(file, SurveySection::StringAttribute);
    for (const auto& [key, feature] : rulesOf(file, SurveySection::Map)) {
        const auto found = attributes.find(key);
        const bool says = found != attributes.end() &&
                          std::any_of(found->second->attributes.begin(), found->second->attributes.end(),
                                      [](const auto& a) { return a.name == "utility.type"; });
        if (key.front() != 'U') {
            // Class K's stormwater pits and structures (KP) and pipes and
            // culverts (KC) are services too; its kerbs, channels and creeks
            // are not.
            const bool stormwater = key.front() == 'K' && (key[1] == 'P' || key[1] == 'C');
            EXPECT_EQ(says, stormwater) << key << (stormwater ? " is stormwater and must say so"
                                                              : " is not a utility and must not claim a service");
            if (stormwater && says) {
                EXPECT_TRUE(hasAttribute(*found->second, "utility.type", "stormwater")) << key;
                ++stormwaterTyped;
            }
            continue;
        }
        ++utilities;
        ASSERT_NE(found, attributes.end()) << key;
        const std::string& word = service.at(key[1]);
        EXPECT_TRUE(hasAttribute(*found->second, "utility.type", word)) << key;
        // utilities/<service word>/<family>: a coded service and UTILITY DRAW
        // share one tree.
        EXPECT_EQ(feature->model.rfind("utilities/" + word + "/", 0), 0u) << key << " " << feature->model;
        if (feature->model.ends_with("/disused")) {
            disusedByLayer.insert(key);
        }
        if (hasAttribute(*found->second, "utility.status", "disused")) {
            disusedByStatus.insert(key);
        }
    }
    EXPECT_EQ(utilities, 60u);
    // KP: gully, kerb inlet, junction, manhole, trap, headwall, culvert end,
    // subsoil inspection point (8); KC: pipe, box culvert, subsoil drain (3).
    EXPECT_EQ(stormwaterTyped, 11u);
    EXPECT_EQ(disusedByLayer, disusedByStatus);
    EXPECT_EQ(disusedByStatus,
              (std::set<std::string>{"UWD", "USD", "UGD", "UED", "UCD", "URD"}));
}

TEST(KatanaStandard, EveryBuriedUtilityLineAsksForItsOwnerItsConditionAndItsDepth)
{
    KS_LOAD();
    const auto attributes = rulesOf(file, SurveySection::StringAttribute);
    std::size_t lines = 0;
    for (const auto& [key, feature] : rulesOf(file, SurveySection::Map)) {
        if (key.front() != 'U' || feature->breakline != katana::entity::SurveyBreakline::Line) {
            continue;
        }
        ++lines;
        const SurveyRule& rule = *attributes.at(key);
        // The line of a service nobody has named has no owner to ask for or
        // condition to read; an overhead line has no depth.
        if (key != "UXL") {
            EXPECT_TRUE(asksFor(rule, "Owner")) << key;
            EXPECT_TRUE(asksFor(rule, "Condition")) << key;
        }
        if (key != "UXL" && key != "UEO" && key != "UCO") {
            EXPECT_TRUE(asksFor(rule, "Depth (m)")) << key;
        }
    }
    EXPECT_EQ(lines, 25u);
}

TEST(KatanaStandard, AnAttributeWithNoValueIsAPromptAndOnlyTheUtilityWordsAreApplied)
{
    KS_LOAD();
    std::size_t prompts = 0;
    std::size_t applied = 0;
    for (const auto& [key, rule] : rulesOf(file, SurveySection::StringAttribute)) {
        std::set<std::string> names;
        for (const auto& attribute : rule->attributes) {
            EXPECT_TRUE(attribute.type == "text" || attribute.type == "integer") << key;
            EXPECT_TRUE(names.insert(attribute.name).second) << key << " repeats " << attribute.name;
            if (attribute.value.empty()) {
                ++prompts;
            } else {
                ++applied;
                EXPECT_EQ(attribute.name.rfind("utility.", 0), 0u)
                    << key << " applies " << attribute.name << " and only utility.* words are applied";
            }
        }
    }
    // 60 utility codes say their service, 11 stormwater pits and pipes say
    // theirs and 6 disused ones say so.
    EXPECT_EQ(applied, 77u);
    EXPECT_EQ(prompts, 395u);
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
            if (stroke.op == StrokeOp::Circle || stroke.op == StrokeOp::Arc || stroke.op == StrokeOp::Dot) {
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
            EXPECT_TRUE(std::isupper(static_cast<unsigned char>(word.front())) || small.count(word) == 1)
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

TEST(KatanaStandard, ANumberedStringAnswersItsCodesRuleAndATypoAnswersNone)
{
    KS_LOAD();
    const auto& map = file.map;
    // UWM, UWM1 and UWM01 are one code: the digits are the string number.
    for (const char* name : {"UWM", "UWM1", "UWM01", "UWM12"}) {
        const auto match = map.lookup(name);
        EXPECT_TRUE(match.matched()) << name;
        EXPECT_EQ(match.resolved.model, "utilities/water/main") << name;
        EXPECT_EQ(match.resolved.linestyle, "Water Supply Pipe") << name;
        EXPECT_EQ(match.resolved.breakline, katana::entity::SurveyBreakline::Line) << name;
    }
    // A paling fence is a line with a post at every vertex.
    const auto fence = map.lookup("FFP2");
    EXPECT_EQ(fence.resolved.linestyle, "Paling Fence");
    ASSERT_TRUE(fence.resolved.symbol.has_value());
    EXPECT_EQ(fence.resolved.symbol->style, "Fence Post");
    // A gully pit is a point with a symbol.
    const auto pit = map.lookup("KPG");
    EXPECT_EQ(pit.resolved.breakline, katana::entity::SurveyBreakline::Point);
    ASSERT_TRUE(pit.resolved.symbol.has_value());
    EXPECT_EQ(pit.resolved.symbol->style, "Gully Pit");
    // The valve and the water main share a colour and not a layer.
    EXPECT_EQ(map.lookup("UWV").resolved.colour, map.lookup("UWM").resolved.colour);
    EXPECT_NE(map.lookup("UWV").resolved.model, map.lookup("UWM").resolved.model);
    // A code nobody wrote is answered by nothing: no family rule swallows it.
    for (const char* typo : {"UWQ", "ZZZ", "UW", "U", "KK", "XQ"}) {
        EXPECT_FALSE(map.lookup(typo).matched()) << typo;
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
        const std::string bare = key.substr(0, key.size() - 1);
        // Once in its table and once in the alphabetical index.
        const std::string row = "\n| `" + bare + "` |";
        const auto first = doc.find(row);
        ASSERT_NE(first, std::string::npos) << bare << " is not catalogued";
        EXPECT_NE(doc.find(row, first + 1), std::string::npos) << bare << " is not in the index";
    }
    EXPECT_EQ(missing, 0u);
}

TEST(KatanaStandard, NoNameOfItIsAnyNameInTheCompiledInBuiltIn)
{
    KS_LOAD();
    const katana::cad::BuiltInCustomisation& builtIn = katana::cad::compiledInCustomisation();
    if (!builtIn.customisation || builtIn.customisation->name == file.name) {
        GTEST_SKIP() << "this build has no other customisation compiled in to compare with";
    }
    // Counts only, found at run time: nothing here names what the other holds.
    // Katana Standard was made from a design of its own, so no definition
    // name, group path, survey code key or colour name of it is one of the
    // built-in's, whole.
    const Customisation& other = *builtIn.customisation;
    std::size_t sharedNames = 0;
    file.library.forEach([&](const LineStyle& style) { sharedNames += other.library.contains(style.name) ? 1 : 0; });
    EXPECT_EQ(sharedNames, 0u) << "definition names in common";

    std::set<std::string> otherGroups;
    for (const std::string& group : katana::entity::styleGroups(other.library)) {
        otherGroups.insert(group);
    }
    std::size_t sharedGroups = 0;
    for (const std::string& group : katana::entity::styleGroups(file.library)) {
        sharedGroups += otherGroups.count(group);
    }
    EXPECT_EQ(sharedGroups, 0u) << "group paths in common";

    std::set<std::string> otherKeys;
    for (const std::string& key : other.map.keys()) {
        otherKeys.insert(key);
    }
    std::size_t sharedKeys = 0;
    for (const std::string& key : file.map.keys()) {
        sharedKeys += otherKeys.count(key);
    }
    EXPECT_EQ(sharedKeys, 0u) << "survey code keys in common";

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
    std::size_t sharedValues = 0;
    for (const auto& entry : file.colours.entries()) {
        sharedValues += otherValues.count(entry.colour.toHex());
    }
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
}
