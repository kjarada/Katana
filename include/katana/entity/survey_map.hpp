#pragma once

// What a surveyor's field code becomes (PLAN.MD 20.3, slice 3).
//
// A code is not a label, it is an instruction. `WM01` is a water main: put it
// in model SURVEY SERVICES, colour it "sui water potable", join it into a
// line rather than leaving it a point, draw it with the "WATR Main"
// linestyle. `AC01` is a bollard and gets a symbol instead. A 12d mapfile is
// the table that says so, and this is that table.
//
// A rule is keyed by a code pattern which is either EXACT ("PABB") or a
// PREFIX ("WM*"). The two Transport for NSW mapfiles hold 1,624 rules over
// 1,364 distinct keys between them, and not one uses a wildcard anywhere but
// at the end - so a key of any other shape is refused rather than matched
// approximately.
//
// A mapfile is written in SECTIONS, and a rule carries only what its section
// is about: where the code goes, or the symbol it gets, or the attributes it
// carries, or how it is drawn as a pipe. So several rules can match one code
// - 213 keys in the detail mapfile do - and a code's treatment is the UNION
// of them. `lookup` is what combines them; see docs/survey_coding.md for how
// ties are settled and on what evidence.

#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::entity {

// Whether a code's points are joined into a string or left as points. 12d
// writes both cases ("Line" and "line"), so it is parsed without regard to
// case.
enum class SurveyBreakline { Line, Point };

[[nodiscard]] const char* toString(SurveyBreakline breakline);
[[nodiscard]] std::optional<SurveyBreakline> parseSurveyBreakline(std::string_view text);

// One attribute a rule attaches. The value is kept VERBATIM because it may
// name another attribute rather than be a value: the mapfiles use
// "$PipeDiameter" and "$CulvBankHeight", and resolving those needs the entity
// the rule is being applied to.
struct SurveyAttribute {
    std::string type{}; // "text" or "integer", 12d's own word
    std::string name{};
    std::string value{};

    friend bool operator==(const SurveyAttribute&, const SurveyAttribute&) = default;
};

// `<symbol_data>`: the symbol a code gets, by name in the symbol library.
struct SurveySymbol {
    std::string style{};  // a name in the loaded StyleLibrary
    std::string colour{}; // a 12d colour name; empty means the string's own
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
// attributes on each vertex inside `vertex_attribute_data`.
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

[[nodiscard]] const char* toString(SurveySection section);
[[nodiscard]] std::optional<SurveySection> parseSurveySection(std::string_view element);

// Everything a rule can say. A rule sets only what its section is about; an
// unset field is one it says nothing about, which is what lets rules combine.
struct SurveyRule {
    std::string key{}; // as written: "WM*" or "PABB"
    SurveySection section = SurveySection::Map;

    std::string model{}; // 12d's model, which becomes a Katana layer
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

// What a code resolves to: the rules that matched, combined.
struct SurveyMatch {
    SurveyRule resolved{}; // `key` is the most specific key that matched
    // Every key that contributed, most specific first.
    std::vector<std::string> keys{};

    [[nodiscard]] bool empty() const { return keys.empty(); }
};

[[nodiscard]] katana::core::Status validate(const SurveyRule& rule);

class SurveyMap {
  public:
    // Rules are kept in the order they were read. Several rules with one key
    // is the normal case - that is how a mapfile is written, one section per
    // aspect - and where two do set the same field the EARLIER wins, so that
    // loading a second mapfile adds to the first rather than overriding it.
    [[nodiscard]] katana::core::Status add(SurveyRule rule);

    // Every rule whose key matches, most specific first: an exact key before
    // a prefix, a longer prefix before a shorter, `*` last. Rules of equal
    // specificity keep the order they were read.
    [[nodiscard]] std::vector<const SurveyRule*> match(std::string_view code) const;
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

  private:
    std::vector<SurveyRule> rules_{};
};

} // namespace katana::entity
