#pragma once

// What a surveyor's field code becomes (PLAN.MD 20.3, slice 3).
//
// A code is not a label, it is an instruction. `WM01` is a water main: put it
// in model SURVEY SERVICES, colour it "sui water potable", join it into a
// line rather than leaving it a point, draw it with the "WATR Main"
// linestyle. `AC01` is a bollard and gets a symbol instead. A survey code
// file (.mapfile) is the table that says so, and this is that table.
//
// A rule is keyed by a code pattern which is either EXACT ("PABB") or a
// PREFIX ("WM*"). The two mapfiles of the reference customisation hold 1,624
// rules over 1,364 distinct keys between them, and not one uses a wildcard
// anywhere but at the end - so a key of any other shape is refused rather
// than matched approximately.
//
// A mapfile is written in SECTIONS, and a rule carries only what its section
// is about: where the code goes, or the symbol it gets, or the attributes it
// carries, or how it is drawn as a pipe. So several rules can match one code
// - 213 keys in the detail mapfile do - and a code's treatment is the UNION
// of them. `lookup` is what combines them; see docs/survey_coding.md for how
// ties are settled and on what evidence.

#include <array>
#include <cstddef>
#include <functional>
#include <optional>
#include <string>
#include <string_view>
#include <unordered_map>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::entity {

// Whether a code's points are joined into a string or left as points. The
// files write both cases ("Line" and "line"), so it is parsed without regard to
// case.
enum class SurveyBreakline { Line, Point };

[[nodiscard]] const char* toString(SurveyBreakline breakline);
[[nodiscard]] std::optional<SurveyBreakline> parseSurveyBreakline(std::string_view text);

// One attribute a rule attaches. The value is kept VERBATIM because it may
// name another attribute rather than be a value: the mapfiles use
// "$PipeDiameter" and "$CulvBankHeight", and resolving those needs the entity
// the rule is being applied to.
struct SurveyAttribute {
    std::string type{}; // "text" or "integer", the file's own word
    std::string name{};
    std::string value{};

    friend bool operator==(const SurveyAttribute&, const SurveyAttribute&) = default;
};

// `<symbol_data>`: the symbol a code gets, by name in the symbol library.
struct SurveySymbol {
    std::string style{};  // a name in the loaded StyleLibrary
    std::string colour{}; // a standard colour name; empty means the string's own
    double size = 0.0;    // 0 means the library definition's own size
    double rotation = 0.0;
    double offset = 0.0; // across the string
    double raise = 0.0;  // up

    friend bool operator==(const SurveySymbol&, const SurveySymbol&) = default;
};

// `<textstyle_data>`: how a code's text is drawn.
struct SurveyTextStyle {
    std::string textstyle{};
    std::string colour{};
    std::string type{}; // "paper" or "world", as written
    double size = 0.0;
    std::string justifyX{};
    std::string justifyY{};
    double offset = 0.0;
    double raise = 0.0;
    double angle = 0.0;
    double slant = 0.0;
    double widthFactor = 1.0; // `x_factor`
    bool underline = false;
    bool strikeout = false;
    bool italic = false;
    std::string weight{}; // "0" or "Normal" in these files: a pen, not a number

    friend bool operator==(const SurveyTextStyle&, const SurveyTextStyle&) = default;
};

// How a coded string is drawn as a pipe or a culvert, from one of the three
// `*pipe_data` sections. `size1` and `size2` are kept as TEXT because they
// are "0.375" in one rule and "$PipeDiameter" in another, and which it is
// cannot be decided without the entity.
struct SurveyPipe {
    std::string justify{}; // "Obvert", "Invert", "Centre"
    std::string shape{};   // "diameter", "culvert"
    std::string size1{};
    std::string size2{};
    bool active = false;

    friend bool operator==(const SurveyPipe&, const SurveyPipe&) = default;
};

// Which section of the mapfile a rule came from. Kept because it is the
// section, not the fields, that says what a rule is ABOUT - `<map_attributes>`
// means attributes on the string inside `string_attribute_data` and
// attributes on each vertex inside `vertex_attribute_data`. A section added
// here needs its word in kSurveySectionWords below.
enum class SurveySection {
    Map,             // map_data: model, colour, breakline, linestyle, weight
    VertexSymbol,    // vertex_symbol_data and vertex_symbol_data_v9
    VertexTextStyle, // vertex_textstyle_data
    Pipe,            // pipe_data
    VertexPipe,      // vertex_pipe_data
    SegmentPipe,     // segment_pipe_data
    StringAttribute, // string_attribute_data
    VertexAttribute, // vertex_attribute_data
    Tinable,         // tinable_data
};

// One section and the word for it.
struct SurveySectionWord {
    SurveySection section;
    const char* word;
};

// The word for each section, in the enumeration's order: what a reply and a
// label call it ("rule #2 KT* (feature)", the code manager's Section list)
// and what a Katana customisation file holds as a rule's "sets". ONE table,
// so that what a person reads in the window is what they write in the file.
// A word changed or added here is a new version of that format
// (docs/customisation.md).
//
// The element names in the comments above (`map_data`) are a survey code
// file's, and are not these: they are kept with that file's reader and
// writer, which are the only code to use them.
inline constexpr std::array<SurveySectionWord, 9> kSurveySectionWords{{
    {SurveySection::Map, "feature"},
    {SurveySection::VertexSymbol, "symbol"},
    {SurveySection::VertexTextStyle, "text"},
    {SurveySection::Pipe, "pipe"},
    {SurveySection::VertexPipe, "vertexPipe"},
    {SurveySection::SegmentPipe, "segmentPipe"},
    {SurveySection::StringAttribute, "attributes"},
    {SurveySection::VertexAttribute, "vertexAttributes"},
    {SurveySection::Tinable, "surface"},
}};

// The word kSurveySectionWords gives the section.
[[nodiscard]] const char* toString(SurveySection section);

// Everything a rule can say. A rule sets only what its section is about; an
// unset field is one it says nothing about, which is what lets rules combine.
struct SurveyRule {
    std::string key{}; // as written: "WM*" or "PABB"
    SurveySection section = SurveySection::Map;

    std::string model{}; // the rule's model, which becomes a Katana layer
    std::string colour{};
    std::string linestyle{};
    // "0" and "Normal" both appear and both mean the default pen, so this is
    // kept as text rather than forced into a number it is not.
    std::string weight{};
    std::string group{};
    std::string comment{};
    std::optional<SurveyBreakline> breakline{};
    std::optional<bool> tinable{};
    std::optional<bool> hide{};

    std::optional<SurveySymbol> symbol{};
    std::optional<SurveyTextStyle> textStyle{};
    std::optional<SurveyPipe> pipe{};        // the string as a pipe
    std::optional<SurveyPipe> vertexPipe{};  // each vertex
    std::optional<SurveyPipe> segmentPipe{}; // each segment

    std::vector<SurveyAttribute> attributes{};        // on the string
    std::vector<SurveyAttribute> vertexAttributes{};  // on each vertex
    std::vector<SurveyAttribute> segmentAttributes{}; // on each segment

    friend bool operator==(const SurveyRule&, const SurveyRule&) = default;
};

// How a code met the map, by the most specific key that matched it. Kept
// apart from "matched" because the reference mapfiles carry `*` rules - 16 in
// each pipe section alone - that every code meets, so "some rule matched" is
// true of a typo too (audit CAD-05).
enum class SurveyMatchKind {
    Exact,        // a key equal to the code
    Prefix,       // a key such as "WM*", but not the bare `*`
    FallbackOnly, // nothing but the bare `*`
    None,         // no rule at all
};

[[nodiscard]] const char* toString(SurveyMatchKind kind);

// What a code resolves to: the rules that matched, combined.
struct SurveyMatch {
    SurveyRule resolved{}; // `key` is the most specific key that matched
    // Every key that contributed, most specific first.
    std::vector<std::string> keys{};
    SurveyMatchKind kind = SurveyMatchKind::None;

    [[nodiscard]] bool empty() const { return keys.empty(); }
    // Whether the map has something to say about THIS code rather than about
    // every code: a rule more specific than `*` matched, or the combination
    // names a model, a linestyle or a symbol (the lead's decision D5). A code
    // only `*` answers, with nothing but its attributes, is fallback-only - in
    // practice a code nobody wrote a rule for, most often a typo.
    [[nodiscard]] bool matched() const
    {
        return kind == SurveyMatchKind::Exact || kind == SurveyMatchKind::Prefix ||
               !resolved.model.empty() || !resolved.linestyle.empty() ||
               (resolved.symbol.has_value() && !resolved.symbol->style.empty());
    }
};

// Besides text being text and numbers being finite: a key has no surrounding
// whitespace (a field code never has, so such a key can match nothing a
// surveyor types), and a model, when there is one, is a valid layer path
// (it becomes one when the code is applied, and failing there would fail the
// whole application over one rule).
[[nodiscard]] katana::core::Status validate(const SurveyRule& rule);

// A rule's identity is its INDEX, and its index is also its precedence: among
// rules of equal specificity the earlier wins. So the editing calls below are
// by index, and every one of them - add included - INVALIDATES what match()
// and lookup() handed out before it: match() returns pointers into the rule
// vector, and an index from matchIndices() may now name another rule. A
// caller holding either across an edit must ask again. (A cache keys on
// cad::Document::surveyMapGeneration, never on a SurveyRule*.)
class SurveyMap {
  public:
    // Rules are kept in the order they were read. Several rules with one key
    // is the normal case - that is how a mapfile is written, one section per
    // aspect - and where two do set the same field the EARLIER wins, so that
    // loading a second mapfile adds to the first rather than overriding it.
    [[nodiscard]] katana::core::Status add(SurveyRule rule);

    // ---- editing, by index --------------------------------------------------
    // Each fails with InvalidArgument (a rule validate() refuses) or NotFound
    // (an index past the end), and then leaves the map exactly as it was.
    //
    // A copy rather than a reference: an editor reads a rule into a form, and
    // a reference would dangle at the first edit.
    [[nodiscard]] katana::core::Result<SurveyRule> at(std::size_t index) const;
    [[nodiscard]] katana::core::Status replace(std::size_t index, SurveyRule rule);
    [[nodiscard]] katana::core::Status remove(std::size_t index);
    // Before the rule now at `index`; `index == size()` appends.
    [[nodiscard]] katana::core::Status insert(std::size_t index, SurveyRule rule);
    // Afterwards the rule is AT `to`, the others keeping their relative
    // order: [A B C D] move(0, 2) gives [B C A D]. Order is precedence, so
    // this is how an editor says which of two equal keys wins.
    [[nodiscard]] katana::core::Status move(std::size_t from, std::size_t to);

    // Every rule whose key matches, most specific first: an exact key before
    // a prefix, a longer prefix before a shorter, `*` last. Rules of equal
    // specificity keep the order they were read.
    [[nodiscard]] std::vector<const SurveyRule*> match(std::string_view code) const;
    // The same rules as indices into rules(), in the same order - what an
    // explanation cites, since a rule has no identity but its place.
    [[nodiscard]] std::vector<std::size_t> matchIndices(std::string_view code) const;
    // Those rules combined: for each field, the most specific rule that says
    // anything about it wins. Attributes accumulate instead of overriding.
    [[nodiscard]] SurveyMatch lookup(std::string_view code) const;

    [[nodiscard]] const std::vector<SurveyRule>& rules() const { return rules_; }
    [[nodiscard]] std::size_t size() const { return rules_.size(); }
    [[nodiscard]] bool empty() const { return rules_.empty(); }
    // Every distinct key, in name order.
    [[nodiscard]] std::vector<std::string> keys() const;
    // Every linestyle and symbol name the rules reference, in name order -
    // what a library must provide for this mapfile to draw.
    [[nodiscard]] std::vector<std::string> stylesReferenced() const;

    // Equal when the rules are, in the same order: order is precedence, so
    // two maps holding the same rules in another order can resolve a code
    // differently and are not equal.
    friend bool operator==(const SurveyMap& a, const SurveyMap& b) { return a.rules_ == b.rules_; }

  private:
    // Rule indices by key, so that a lookup is a handful of hash probes - one
    // for the exact key and one per prefix length of the code - instead of a
    // test of every rule: applying 1,624 rules to 20,000 points was 32 million
    // key comparisons. Derived from rules_ and rebuilt by every edit but add;
    // each list is ascending, which is read order, which is what breaks ties.
    struct KeyHash {
        using is_transparent = void;
        [[nodiscard]] std::size_t operator()(std::string_view text) const
        {
            return std::hash<std::string_view>{}(text);
        }
    };
    using KeyIndex =
        std::unordered_map<std::string, std::vector<std::size_t>, KeyHash, std::equal_to<>>;

    void indexRule(std::size_t at);
    void reindex();

    std::vector<SurveyRule> rules_{};
    KeyIndex exact_{};    // "PABB" -> the rules keyed exactly so
    KeyIndex prefixes_{}; // "WM" for "WM*", "" for the bare `*`
};

} // namespace katana::entity
