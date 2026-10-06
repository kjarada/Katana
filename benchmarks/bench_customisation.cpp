// What a customisation costs to read and to write in the Katana customisation
// format (entity/customisation.hpp; docs/customisation.md has the numbers).
//
// The customisation is GENERATED, in code, to the size and the mix of the
// reference one - 792 definitions (474 of them symbols) holding 35,684
// strokes, 17,014 moves, 17,220 draws, 104 arcs, 312 circles, 178 dots, 342
// pens and 514 texts, with coordinates of three decimals as such files have,
// and 1,624 rules over 632 keys in the reference mix of kinds. The reference
// customisation itself is third-party material and is in no clone, and a
// measurement has to be repeatable by anyone; the figures are those of
// tools/reference_census.py.
//
// Reading is what a start-up pays, once: the customisation compiled into the
// program is parsed on first use. So besides the whole file it is read in its
// two halves - the definitions alone and the rules alone - and the numbers of
// its strokes are converted on their own, which is where the time goes: a
// definition is almost nothing but numbers, about 71,000 of them.
//
// Where the build has a customisation to compile in (the file
// KATANA_BUILTIN_CUSTOMISATION names, git-ignored and on the owner's machine
// only), the same is measured on that very file, read from disk here.

#include <benchmark/benchmark.h>

#include <algorithm>
#include <charconv>
#include <cstddef>
#include <cstdlib>
#include <fstream>
#include <sstream>
#include <string>
#include <utility>
#include <vector>

#include "katana/core/text.hpp"
#include "katana/entity/customisation.hpp"

using namespace katana::entity;

namespace {

// The reference customisation's size (tools/reference_census.py).
constexpr std::size_t kDefinitions = 792;
constexpr std::size_t kSymbols = 474;
constexpr std::size_t kAtVertices = 157;
constexpr std::size_t kGroups = 71;
constexpr std::size_t kRules = 1624;
constexpr std::size_t kKeys = 632;

struct KindCount {
    StrokeOp op;
    std::size_t count;
};
constexpr KindCount kStrokeMix[] = {
    {StrokeOp::Move, 17014}, {StrokeOp::Draw, 17220}, {StrokeOp::Arc, 104}, {StrokeOp::Circle, 312},
    {StrokeOp::Dot, 178},    {StrokeOp::Pen, 342},    {StrokeOp::Text, 514},
};

struct SectionCount {
    SurveySection section;
    std::size_t count;
};
constexpr SectionCount kRuleMix[] = {
    {SurveySection::Map, 1029},           {SurveySection::VertexSymbol, 401},
    {SurveySection::Tinable, 119},        {SurveySection::VertexTextStyle, 14},
    {SurveySection::Pipe, 16},            {SurveySection::VertexPipe, 16},
    {SurveySection::SegmentPipe, 16},     {SurveySection::StringAttribute, 12},
    {SurveySection::VertexAttribute, 1},
};

// `counts` dealt out evenly: each kind spread over the whole run, so that no
// definition is all arcs and no stretch of the rules all one section.
template <typename Kind, typename Count, std::size_t N>
std::vector<Kind> dealt(const Count (&counts)[N], Kind Count::* kind)
{
    std::size_t total = 0;
    for (const Count& entry : counts) {
        total += entry.count;
    }
    std::vector<std::pair<double, Kind>> placed;
    placed.reserve(total);
    for (const Count& entry : counts) {
        for (std::size_t i = 0; i < entry.count; ++i) {
            placed.emplace_back((static_cast<double>(i) + 0.5) / static_cast<double>(entry.count),
                                entry.*kind);
        }
    }
    std::stable_sort(placed.begin(), placed.end(),
                     [](const auto& a, const auto& b) { return a.first < b.first; });
    std::vector<Kind> order;
    order.reserve(total);
    for (const auto& entry : placed) {
        order.push_back(entry.second);
    }
    return order;
}

// A coordinate of three decimals between -10 and 10, different for every `n`.
double coordinate(std::size_t n, std::size_t step)
{
    return (static_cast<double>((n * step) % 20001) - 10000.0) / 1000.0;
}

std::string numbered(const char* prefix, std::size_t n)
{
    std::string digits = std::to_string(n);
    return std::string(prefix) + std::string(4 - std::min<std::size_t>(4, digits.size()), '0') +
           digits;
}

std::string colourName(std::size_t n)
{
    return numbered("site colour ", n);
}

std::string definitionName(std::size_t i)
{
    return numbered(i < kSymbols ? "GEN Symbol " : "GEN Linestyle ", i);
}

LineStyle generatedDefinition(std::size_t i, const std::vector<StrokeOp>& kinds)
{
    LineStyle style;
    style.name = definitionName(i);
    style.group = numbered("Generated/G", i % kGroups);
    style.source = "Generated";
    style.symbol = i < kSymbols;
    style.atVertices = i < kAtVertices;
    if (!style.symbol) {
        // The linestyles in the reference mix of units: 237 paper, 45 between
        // two points, the rest in world units.
        const std::size_t line = i - kSymbols;
        style.units = line < 237 ? StyleUnits::Paper
                                 : line < 282 ? StyleUnits::TwoPoint : StyleUnits::World;
        style.length = 12.5;
        if (style.units == StyleUnits::TwoPoint) {
            style.anchor2 = {4.0, 0.0};
            style.stretchMode = 2;
            style.cycleMode = 1;
        }
    }
    const std::size_t first = i * kinds.size() / kDefinitions;
    const std::size_t last = (i + 1) * kinds.size() / kDefinitions;
    for (std::size_t n = first; n < last; ++n) {
        Stroke stroke;
        stroke.op = kinds[n];
        switch (stroke.op) {
        case StrokeOp::Move:
        case StrokeOp::Draw:
            stroke.point = {coordinate(n, 37), coordinate(n, 53)};
            break;
        case StrokeOp::Arc:
            stroke.radius = coordinate(n, 37);
            stroke.startAngle = static_cast<double>(n % 360);
            stroke.endAngle = static_cast<double>((n * 7) % 360);
            break;
        case StrokeOp::Circle:
        case StrokeOp::Dot:
            stroke.radius = static_cast<double>(n % 4000) / 1000.0;
            break;
        case StrokeOp::Pen:
            stroke.pen = n % 6 == 0 ? "view_colour" : "pen 035";
            break;
        case StrokeOp::Text: {
            StrokeText text;
            text.text = "W";
            text.height = 1.5;
            text.justify = "middle-centre";
            text.font = "Arial";
            text.widthFactor = 0.8;
            text.unnamed = {0.0, -0.3, 0.035};
            stroke.text = style.texts.size();
            style.texts.push_back(std::move(text));
            break;
        }
        }
        style.strokes.push_back(std::move(stroke));
    }
    return style;
}

SurveyRule generatedRule(std::size_t i, SurveySection section)
{
    SurveyRule rule;
    const std::size_t key = i % kKeys; // 632 keys of two letters: 26 * 26 is 676
    rule.key = {static_cast<char>('A' + key / 26), static_cast<char>('A' + key % 26), '*'};
    rule.section = section;
    rule.comment = "[" + rule.key + "] Generated rule " + std::to_string(i);
    const SurveyPipe pipe{"Invert", "diameter", "$PipeDiameter", "0.375", true};
    const std::vector<SurveyAttribute> attributes{{"text", "Source", "generated"},
                                                  {"integer", "Class", "2"}};
    switch (section) {
    case SurveySection::Map:
        rule.model = "SURVEY GENERATED";
        rule.colour = i % 5 == 0 ? colourName(i % 7) : "red";
        rule.breakline = i % 3 == 0 ? SurveyBreakline::Point : SurveyBreakline::Line;
        rule.linestyle = definitionName(kSymbols + i % (kDefinitions - kSymbols));
        rule.weight = "0";
        rule.group = "SURVEY - GENERATED";
        break;
    case SurveySection::VertexSymbol:
        rule.symbol = SurveySymbol{definitionName(i % kSymbols), "white", 1.5, 0.0, 0.0, 0.0};
        rule.hide = false;
        break;
    case SurveySection::Tinable:
        rule.tinable = i % 2 == 0;
        break;
    case SurveySection::VertexTextStyle: {
        SurveyTextStyle text;
        text.textstyle = "Standard";
        text.colour = "white";
        text.type = "paper";
        text.size = 2.5;
        text.justifyX = "left";
        text.justifyY = "bottom";
        text.weight = "Normal";
        rule.textStyle = text;
        break;
    }
    case SurveySection::Pipe:
        rule.pipe = pipe;
        rule.attributes = attributes;
        break;
    case SurveySection::VertexPipe:
        rule.vertexPipe = pipe;
        rule.vertexAttributes = attributes;
        break;
    case SurveySection::SegmentPipe:
        rule.segmentPipe = pipe;
        rule.segmentAttributes = attributes;
        break;
    case SurveySection::StringAttribute:
        rule.attributes = attributes;
        break;
    case SurveySection::VertexAttribute:
        rule.vertexAttributes = attributes;
        break;
    }
    return rule;
}

// The generated customisation; `sound` is false if anything it was to hold
// was refused, so that a benchmark of half a customisation cannot pass for
// one of the whole.
struct Generated {
    Customisation customisation{};
    std::size_t strokes = 0;
    bool sound = true;
};

const Generated& generated()
{
    static const Generated made = [] {
        Generated out;
        Customisation& customisation = out.customisation;
        customisation.name = "Generated";
        customisation.description = "A customisation of the reference one's size";
        // Seven colour names of its own, as the reference one has.
        for (std::size_t n = 0; n < 7; ++n) {
            out.sound = customisation.colours.add(colourName(n), Color{0, 112, 255, 255}).ok() &&
                        out.sound;
        }
        const std::vector<StrokeOp> kinds = dealt(kStrokeMix, &KindCount::op);
        for (std::size_t i = 0; i < kDefinitions; ++i) {
            LineStyle style = generatedDefinition(i, kinds);
            out.strokes += style.strokes.size();
            out.sound = customisation.library.add(std::move(style)).ok() && out.sound;
        }
        const std::vector<SurveySection> sections = dealt(kRuleMix, &SectionCount::section);
        for (std::size_t i = 0; i < sections.size(); ++i) {
            out.sound = customisation.map.add(generatedRule(i, sections[i])).ok() && out.sound;
        }
        out.sound = out.sound && customisation.library.size() == kDefinitions &&
                    customisation.map.size() == kRules && out.strokes == kinds.size();
        return out;
    }();
    return made;
}

// Which part of a customisation a text holds.
enum class Part { Whole, Definitions, Rules };

const char* label(Part part)
{
    switch (part) {
    case Part::Whole:
        return "whole";
    case Part::Definitions:
        return "definitions alone";
    case Part::Rules:
        break;
    }
    return "rules alone";
}

CustomisationWriteOptions holding(Part part)
{
    CustomisationWriteOptions options;
    options.linestyles = options.symbols = part != Part::Rules;
    options.codes = part != Part::Definitions;
    return options;
}

// What the text measured holds, so that a later run is compared like for like.
void countersOf(benchmark::State& state, const Customisation& customisation, Part part,
                std::size_t bytes)
{
    std::size_t strokes = 0;
    customisation.library.forEach([&](const LineStyle& s) { strokes += s.strokes.size(); });
    const bool definitions = part != Part::Rules;
    const bool rules = part != Part::Definitions;
    state.counters["bytes"] = static_cast<double>(bytes);
    state.counters["definitions"] =
        static_cast<double>(definitions ? customisation.library.size() : 0);
    state.counters["strokes"] = static_cast<double>(definitions ? strokes : 0);
    state.counters["rules"] = static_cast<double>(rules ? customisation.map.size() : 0);
}

// Writing `customisation`, whole or a part of it.
void writing(benchmark::State& state, const Customisation& customisation)
{
    const Part part = static_cast<Part>(state.range(0));
    const CustomisationWriteOptions options = holding(part);
    // Once outside the timing: that it can be written at all, and how long
    // the text is.
    const auto written = customisationToJson(customisation, options);
    if (!written) {
        state.SkipWithError(written.error().describe());
        return;
    }
    for (auto _ : state) {
        auto text = customisationToJson(customisation, options);
        benchmark::DoNotOptimize(text);
    }
    state.SetLabel(label(part));
    countersOf(state, customisation, part, written->size());
}

// Reading the text `customisation` is written as, whole or a part of it.
void reading(benchmark::State& state, const Customisation& customisation)
{
    const Part part = static_cast<Part>(state.range(0));
    const auto text = customisationToJson(customisation, holding(part));
    if (!text) {
        state.SkipWithError(text.error().describe());
        return;
    }
    if (const auto once = customisationFromJson(*text); !once) {
        state.SkipWithError(once.error().describe());
        return;
    }
    for (auto _ : state) {
        auto back = customisationFromJson(*text);
        benchmark::DoNotOptimize(back);
    }
    state.SetLabel(label(part));
    countersOf(state, customisation, part, text->size());
}

void BM_CustomisationWriteGenerated(benchmark::State& state)
{
    if (!generated().sound) {
        state.SkipWithError("the generated customisation is not the one described");
        return;
    }
    writing(state, generated().customisation);
}
BENCHMARK(BM_CustomisationWriteGenerated)->DenseRange(0, 2)->Unit(benchmark::kMillisecond);

void BM_CustomisationReadGenerated(benchmark::State& state)
{
    if (!generated().sound) {
        state.SkipWithError("the generated customisation is not the one described");
        return;
    }
    reading(state, generated().customisation);
}
BENCHMARK(BM_CustomisationReadGenerated)->DenseRange(0, 2)->Unit(benchmark::kMillisecond);

// ---- where reading's time goes: the numbers ------------------------------------------------------
//
// Every coordinate, radius and angle of the generated strokes as the text the
// writer gives it, converted back to a double the two ways there are: the C
// library's strtod, which is what the JSON library hands each number to, and
// std::from_chars. Nothing else is done, so the difference between the two is
// what a reader with a number scanner of its own would save.

const std::vector<std::string>& strokeNumbers()
{
    static const std::vector<std::string> numbers = [] {
        std::vector<std::string> out;
        generated().customisation.library.forEach([&](const LineStyle& style) {
            for (const Stroke& stroke : style.strokes) {
                switch (stroke.op) {
                case StrokeOp::Move:
                case StrokeOp::Draw:
                    out.push_back(katana::core::formatExactReal(stroke.point.x));
                    out.push_back(katana::core::formatExactReal(stroke.point.y));
                    break;
                case StrokeOp::Arc:
                    out.push_back(katana::core::formatExactReal(stroke.radius));
                    out.push_back(katana::core::formatExactReal(stroke.startAngle));
                    out.push_back(katana::core::formatExactReal(stroke.endAngle));
                    break;
                case StrokeOp::Circle:
                case StrokeOp::Dot:
                    out.push_back(katana::core::formatExactReal(stroke.radius));
                    break;
                case StrokeOp::Pen:
                case StrokeOp::Text:
                    break;
                }
            }
        });
        return out;
    }();
    return numbers;
}

void BM_CustomisationStrokeNumbersByStrtod(benchmark::State& state)
{
    const std::vector<std::string>& numbers = strokeNumbers();
    for (auto _ : state) {
        double sum = 0.0;
        for (const std::string& number : numbers) {
            sum += std::strtod(number.c_str(), nullptr);
        }
        benchmark::DoNotOptimize(sum);
    }
    state.counters["numbers"] = static_cast<double>(numbers.size());
}
BENCHMARK(BM_CustomisationStrokeNumbersByStrtod)->Unit(benchmark::kMillisecond);

void BM_CustomisationStrokeNumbersByFromChars(benchmark::State& state)
{
    const std::vector<std::string>& numbers = strokeNumbers();
    for (auto _ : state) {
        double sum = 0.0;
        for (const std::string& number : numbers) {
            double value = 0.0;
            static_cast<void>(std::from_chars(number.data(), number.data() + number.size(), value));
            sum += value;
        }
        benchmark::DoNotOptimize(sum);
    }
    state.counters["numbers"] = static_cast<double>(numbers.size());
}
BENCHMARK(BM_CustomisationStrokeNumbersByFromChars)->Unit(benchmark::kMillisecond);

// ---- the customisation the build compiles in -----------------------------------------------------

#ifdef KATANA_BENCH_BUILTIN_CUSTOMISATION

struct BuiltIn {
    std::string text{};
    Customisation customisation{};
    std::string problem{}; // why it could not be read; empty when it was
};

const BuiltIn& builtIn()
{
    static const BuiltIn read = [] {
        BuiltIn out;
        std::ifstream file(KATANA_BENCH_BUILTIN_CUSTOMISATION, std::ios::binary);
        if (!file) {
            out.problem = "cannot open " KATANA_BENCH_BUILTIN_CUSTOMISATION;
            return out;
        }
        std::ostringstream buffer;
        buffer << file.rdbuf();
        out.text = std::move(buffer).str();
        auto customisation = customisationFromJson(out.text);
        if (!customisation) {
            out.problem = customisation.error().describe();
            return out;
        }
        out.customisation = std::move(*customisation);
        return out;
    }();
    return read;
}

void BM_CustomisationWriteBuiltIn(benchmark::State& state)
{
    if (!builtIn().problem.empty()) {
        state.SkipWithError(builtIn().problem);
        return;
    }
    writing(state, builtIn().customisation);
}
BENCHMARK(BM_CustomisationWriteBuiltIn)->DenseRange(0, 2)->Unit(benchmark::kMillisecond);

void BM_CustomisationReadBuiltIn(benchmark::State& state)
{
    if (!builtIn().problem.empty()) {
        state.SkipWithError(builtIn().problem);
        return;
    }
    reading(state, builtIn().customisation);
}
BENCHMARK(BM_CustomisationReadBuiltIn)->DenseRange(0, 2)->Unit(benchmark::kMillisecond);

// The file itself, as it is on disk: what a start-up parses.
void BM_CustomisationReadBuiltInFile(benchmark::State& state)
{
    if (!builtIn().problem.empty()) {
        state.SkipWithError(builtIn().problem);
        return;
    }
    for (auto _ : state) {
        auto back = customisationFromJson(builtIn().text);
        benchmark::DoNotOptimize(back);
    }
    countersOf(state, builtIn().customisation, Part::Whole, builtIn().text.size());
}
BENCHMARK(BM_CustomisationReadBuiltInFile)->Unit(benchmark::kMillisecond);

#endif // KATANA_BENCH_BUILTIN_CUSTOMISATION

} // namespace
