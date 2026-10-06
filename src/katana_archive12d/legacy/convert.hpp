#pragma once

// Converting a customisation from the older formats: style libraries (.4d)
// and survey code files (.mapfile), in load order, into ONE Katana
// customisation (entity/customisation.hpp, docs/customisation.md).
//
// This is a developer's tool, not part of the product. It is the only thing
// that turns the older files into the Katana customisation format, and it is
// built as the program katana_customisation_convert (convert_main.cpp), which is not
// installed and which none of katana, katana_cli and katana_mcp links. What it
// does is here, as functions, so that it is tested like any other code; the
// program only reads its command line.
//
// THE ORDER OF THE FILES IS THEIR MEANING. A later library wins a definition
// both give (entity::addOrReplace); an earlier survey code file wins a field
// both set, because its rules come first and order is precedence
// (entity::SurveyMap). The result holds the definitions after that, and the
// rules in the order they were read. A definition that lost its place is
// never lost silently: each is named in the report (ReplacedDefinition).
//
// It converts from what the READERS made of the files (StyleLibrary,
// SurveyMap), never from the text: the readers apply defaults and skip what
// they cannot read, and a second reading of the same text here would be a
// second opinion of what a file means.

#include <cstddef>
#include <filesystem>
#include <optional>
#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/entity/customisation.hpp"

namespace katana::archive12d {

// One file as its bytes, in any encoding core::decodeText reads. `name` is
// what it is reported by, and for a style library it also says whether its
// definitions are symbols: the older format holds one kind a file and says
// which only by the file's name (archive12d::readStyleLibrary).
struct LegacyFile {
    std::string name{};
    std::string bytes{};
};

struct ConvertOptions {
    // The customisation's name (entity::validateCustomisationName). Every
    // definition's source becomes it: once converted, the files a definition
    // was read from are no longer what it came from.
    std::string name{};
    std::string description{};
    // Files whose LEADING `//` comment block is the author's notice, each
    // line kept as written with the marker and one blank after it taken off.
    // A block that is the same as the one before it is taken once.
    std::vector<LegacyFile> noticeFrom{};
    // A colour table: lines of `R G B <index> "name"`, whatever follows the
    // name ignored. It gives a colour to every name a rule uses - its own,
    // its symbol's, its text's - that the standard names lack, matched by
    // entity::foldColourName.
    //
    // A plot pen ("pen 035": `pen`, digits and at most one letter) is never
    // taken from it. Such a name is a pen of the plotter the files were
    // written for and not a colour of the drawing; unresolved, it leaves the
    // entity's own colour alone, which is how these codes have always drawn.
    std::optional<LegacyFile> colours{};
    // Words taken, with the blanks after them, off the FRONT of every
    // definition name, every group path, every rule's group and every rule's
    // linestyle and symbol name - so that a rule still names the definition
    // it named. Matched as written (case is not folded), at most one word a
    // text: the first given that begins it. A text that is nothing but the
    // word keeps it.
    std::vector<std::string> stripLeadingWords{};
    // Words taken out of rule comments wherever they stand as a whole word,
    // with the blanks around them (one is left where both sides had one).
    std::vector<std::string> removeWords{};
};

// A definition that another of its name took the place of: two libraries gave
// the name, or one gave it twice. The customisation holds the LAST given, and
// this says what went.
//
// It matters because the older formats keep linestyles and symbols in separate
// files, where one name may be both, and a customisation has one definition a
// name: when a symbol library is read after the linestyle library, a rule that
// draws a line with such a name is left naming a symbol. The counts of rules
// say which of the two the rules were written for, so that the person
// converting can give the libraries in the order that keeps it.
struct ReplacedDefinition {
    std::string name{};        // as the files give it, before any word is taken off
    std::string keptFrom{};    // the file whose definition the customisation holds
    std::string droppedFrom{}; // the file whose definition went
    bool keptAsSymbol = false;
    bool droppedAsSymbol = false;
    // The two are not one definition, leaving out where each came from and
    // which kind its file made it.
    bool differs = false;
    std::size_t linestyleRules = 0; // rules naming it as their linestyle
    std::size_t symbolRules = 0;    // rules naming it as their symbol

    friend bool operator==(const ReplacedDefinition&, const ReplacedDefinition&) = default;
};

// A STANDARD colour name the rules use, to which the colour table gives
// another colour than the standard one. The standard colour stands - a
// customisation's table may not redefine a standard name
// (entity::ColourTable::add says why) - so the table's colour cannot be
// carried, and is said here instead of being dropped without a word.
struct StandardColourKept {
    std::string name{};     // as the rules first write it
    std::string standard{}; // "#RRGGBB": what the name draws
    std::string table{};    // "#RRGGBB": what the table gives it

    friend bool operator==(const StandardColourKept&, const StandardColourKept&) = default;
};

// What a conversion did, for the person who ran it to check against what they
// know of the files. toText gives it as key=value lines.
struct ConvertReport {
    std::string name{};
    std::size_t files = 0;
    std::size_t definitions = 0;
    std::size_t symbols = 0;    // definitions listed as symbols
    std::size_t linestyles = 0; // the others
    std::size_t atVertices = 0;
    std::size_t groups = 0; // distinct group paths
    std::size_t strokes = 0;
    // Every definition that went because a later one has its name, in the
    // order they went.
    std::vector<ReplacedDefinition> replaced{};
    std::size_t rules = 0;
    std::size_t keys = 0; // distinct rule keys
    // Colour names the table gave a colour, and those left without one (not a
    // standard name, and a plot pen or not in the table); each as first
    // written, in the order of the folded names.
    std::vector<std::string> coloursResolved{};
    std::vector<std::string> coloursUnresolved{};
    // In the order of the folded names.
    std::vector<StandardColourKept> coloursKeptStandard{};
    std::size_t namesRenamed = 0;      // definition names that lost a leading word
    std::size_t groupsRenamed = 0;     // definition group paths that did
    std::size_t referencesRenamed = 0; // rule linestyle and symbol names that did
    std::size_t ruleGroupsRenamed = 0; // rule groups that did
    std::size_t commentsChanged = 0;   // rule comments a word was taken out of
    // Linestyle and symbol names the rules use that no definition has, in
    // name order. Not an error: a customisation need not be self-contained
    // ("0" is the plain continuous line).
    std::vector<std::string> unresolvedReferences{};
    std::size_t noticeLines = 0;
    // What the readers of the older formats said of the files, and what the
    // conversion found in them itself, each prefixed with its file:
    // * a file that gave nothing - a style library with no definition, a
    //   survey code file with no rule;
    // * a file that is neither UTF-8 nor marked as UTF-16, so that its
    //   characters outside ASCII are what an inferred encoding makes of them
    //   (core::decodeText), with the encoding and how many they are;
    // * a definition kept as one kind in place of one of the other kind
    //   (ReplacedDefinition) when rules use the name as the kind that went.
    // None of them fails a conversion: each is for the person converting to
    // read.
    std::vector<std::string> warnings{};
    std::size_t bytes = 0; // of the text written
    std::string output{};  // where convertLegacyFiles wrote it; empty when nowhere
};

struct Conversion {
    katana::entity::Customisation customisation{};
    // entity::customisationToJson of it: UTF-8, lines ended by a line feed.
    std::string json{};
    ConvertReport report{};
};

// Converts `files`, in the order given, into one customisation.
//
// Fails, changing nothing, with:
// * InvalidArgument when no name is given or the name is not one a
//   customisation may have, when there are no files, when a word to strip or
//   to remove is empty, and for a file that is neither a style library nor a
//   survey code file (the file is named);
// * whatever a reader fails with for a file it cannot read at all (a quoted
//   text never closed, XML that is not well formed), the file named;
// * InvalidArgument when taking a leading word off would give two definitions
//   one name (both are named), or would make a rule name a definition it did
//   not name before (the rule, the name and the definition are named);
// * InvalidArgument for a notice file with no leading `//` block, for a line
//   of the colour table that is not `R G B <index> "name"` (the line is
//   numbered), and for a name a rule uses that the table gives two colours.
[[nodiscard]] katana::core::Result<Conversion>
convertLegacyCustomisation(const std::vector<LegacyFile>& files, const ConvertOptions& options);

// What katana_customisation_convert is asked on its command line: the same,
// with files named by path.
struct ConvertRequest {
    std::string name{};                             // --name
    std::string description{};                      // --description
    std::vector<std::filesystem::path> noticeFrom{}; // --notice-from, repeatable
    std::filesystem::path colours{};                // --colours; empty: no table
    std::vector<std::string> stripLeadingWords{};   // --strip-leading-word, repeatable
    std::vector<std::string> removeWords{};         // --remove-word, repeatable
    std::filesystem::path output{};                 // -o; empty: nothing is written
    std::vector<std::filesystem::path> files{};     // the files, in load order
};

// Reads the files a request names, converts them, and writes the text to
// `request.output` (its directory is made if it is not there). NotFound,
// naming the path, for a file that cannot be opened - before anything is
// written; FileExportFailure when the output cannot be written.
//
// The text is written beside the output first (`<output>.partial`) and then
// put in its place, so a write that fails leaves the file that was there:
// the output is what a build compiles in, and half of a new one would be
// compiled in instead.
[[nodiscard]] katana::core::Result<Conversion> convertLegacyFiles(const ConvertRequest& request);

// The report as key=value lines, one fact a line, a text value quoted as
// core::replyQuoted does. A list is its count (`colours_unresolved=1`) and
// then a line an entry (`colour_unresolved="pen 018"`); an entry of more than
// one fact is a record of them on its line:
//
//   definition_replaced="Gate" kept="site_symbols.4d" kept_as=symbol dropped="site_lines.4d" dropped_as=linestyle differs=yes linestyle_rules=2 symbol_rules=0
//   colour_kept_standard="brown" standard="#A52A2A" table="#964B00"
[[nodiscard]] std::string toText(const ConvertReport& report);

} // namespace katana::archive12d
