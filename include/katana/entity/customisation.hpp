#pragma once

// A customisation, and the Katana customisation format that keeps one.
//
// A customisation is what turns a surveyor's codes into a drawing: the
// linestyle and symbol definitions (StyleLibrary), the survey code rules
// (SurveyMap), the colours its names mean (ColourTable) and how linework is
// processed. It was three files in another program's formats; this is one
// value and one file of Katana's own:
//
//   {"format": "katana-customisation", "version": 1, "name": "NSW",
//    "colours": {"sui electricity": "#FF7F00"},
//    "linestyles": [{"name": "WATR Main", "strokes": [["move", 0, 0], ...]}],
//    "symbols": [...],
//    "codes": [{"key": "WM*", "sets": "feature", "layer": "SURVEY SERVICES"}]}
//
// docs/customisation.md is the format's specification: every member, why the
// reader is strict, what may never change without a new version, and the
// alternatives that were rejected. Three things about it shape this header:
//
// * It is STRICT. A member the format does not know, a member given twice and
//   a value of the wrong type are refused, naming the entry, where a lenient
//   reader would skip them: a customisation is edited by hand, and a typo that
//   vanished silently would also be gone from the copy Katana writes back.
// * It is LOSSLESS against this model. A customisation the writer accepts
//   reads back equal (operator== below), rule order and absent optionals
//   included; what it could not write back as itself it refuses to write.
// * It lives in the entity layer because both `cad`, which loads and edits a
//   customisation, and the converter of the older formats, which may not see
//   cad, must reach it (tools/check_layering.cmake). The JSON library stays
//   in the .cpp; everything here is text and core::Result.

#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/entity/colour_names.hpp"
#include "katana/entity/linework_codes.hpp"
#include "katana/entity/style_library.hpp"
#include "katana/entity/survey_map.hpp"

namespace katana::entity {

// The value of a file's "format" member, and the newest "version" this build
// reads and the one it writes.
inline constexpr std::string_view kCustomisationFormat = "katana-customisation";
inline constexpr int kCustomisationVersion = 1;

// A customisation's name: not empty, valid UTF-8, and with no line break, '/'
// or '\'. InvalidArgument otherwise. The name is written into a project as the
// record of what the drawing was drawn with - one name a line, never a path -
// and the project store refuses exactly these (storage::ProjectMetadata), so a
// name that passed here and failed there would make every SAVE of the session
// fail.
[[nodiscard]] katana::core::Status validateCustomisationName(std::string_view name);

// One customisation that went into this one, as the session's load record
// holds it: a customisation merged from two keeps both names, what each
// brought and each one's own notice.
struct CustomisationSourceNote {
    std::string name{};
    bool definitions = false; // it brought linestyle or symbol definitions
    bool rules = false;       // it brought survey code rules
    std::vector<std::string> notice{};

    friend bool operator==(const CustomisationSourceNote&,
                           const CustomisationSourceNote&) = default;
};

// The customisation an edited copy started from: its name, and the digest
// (customisationDigest) of the file it was read from, so that a copy kept by a
// user can be told from one made against an earlier edition of the same name.
struct CustomisationBase {
    std::string name{};
    std::string digest{};

    friend bool operator==(const CustomisationBase&, const CustomisationBase&) = default;
};

// What is applied to survey data without being asked for.
struct CustomisationAutomation {
    bool codesOnSurveyImport = true;    // code the points an import created
    bool lineworkOnSurveyImport = true; // then join the coded points into lines

    friend bool operator==(const CustomisationAutomation&,
                           const CustomisationAutomation&) = default;
};

struct Customisation {
    std::string name{};
    std::string description{};
    // The author's notice, a line an entry: a licence, a disclaimer. Carried
    // with the data and shown to whoever uses it.
    std::vector<std::string> notice{};
    std::vector<CustomisationSourceNote> sources{};
    std::optional<CustomisationBase> basedOn{};
    ColourTable colours{};
    // Absent means the customisation SAYS NOTHING about them, which is not the
    // same as giving the defaults: merged into a session, it leaves the
    // session's own alone. A file of symbols for a colleague must not reset
    // their control codes.
    std::optional<LineworkCodes> linework{};
    std::optional<CustomisationAutomation> automation{};
    // Each definition's `symbol` says which of the file's two arrays holds it,
    // and its `source` the customisation it came from.
    StyleLibrary library{};
    SurveyMap map{};

    // Equal when every member is, the library definition for definition and
    // the rules in the same order. (NamedTable has no operator== of its own:
    // a table also counts its revisions, which are not part of its value.)
    friend bool operator==(const Customisation& a, const Customisation& b);
};

// What a write holds of a customisation's definitions and rules. Its name,
// notice, sources, colours, linework and automation are written as the value
// has them - a caller that wants one left out clears it in a copy first.
struct CustomisationWriteOptions {
    bool linestyles = true; // the definitions not listed as symbols
    bool symbols = true;    // the definitions listed as symbols
    bool codes = true;      // the survey code rules
    // Only these definitions, by name; every definition when empty. A name the
    // library lacks fails the write (NotFound), and so does one whose kind the
    // two flags above leave out (InvalidArgument): a definition asked for by
    // name is never dropped silently.
    std::vector<std::string> only{};
};

// Reads the bytes of a Katana customisation file: UTF-8 with or without a byte
// order mark, or UTF-16 (core::decodeText).
//
// ParseFailure "not a Katana customisation file" for text that is not JSON or
// whose "format" is not kCustomisationFormat - a survey code file (.mapfile)
// and a style library (.4d) get exactly that, and so do bytes that are
// neither UTF-8 nor UTF-16 (no code page is guessed at). Unsupported for a
// "version" newer than kCustomisationVersion. ParseFailure, naming the entry
// and the member (`codes[57] "WM*": unknown member "linesytle"`), for a member
// the format does not know, a member given twice, a required member missing,
// a value of the wrong type, a word outside an enumeration and a stroke of
// the wrong length. InvalidArgument, naming the entry, for what is
// well-formed and still not a customisation: a bad name, two definitions of
// one name, a colour name the table refuses, and everything entity::validate
// refuses of a definition, a rule or the linework codes. Indices count from
// 0, as a JSON path does.
[[nodiscard]] katana::core::Result<Customisation> customisationFromJson(std::string_view text);

// Writes `customisation` as the text of a file: UTF-8, deterministic (the same
// value gives the same bytes), a definition's strokes one a line. Definitions
// are written in name order and rules in map order; docs/customisation.md,
// "Layout", says where every byte goes.
//
// InvalidArgument, naming the entry, for anything that would not read back as
// itself: a number that is not finite, text that is not UTF-8, a stroke
// carrying a member its kind does not use, a definition whose `texts` are not
// exactly its text strokes in order, an attribute whose type is neither "text"
// nor "integer", and whatever the reader would refuse (a bad name, linework
// codes validate refuses). NotFound or InvalidArgument for `options.only`, as
// said there.
[[nodiscard]] katana::core::Result<std::string>
customisationToJson(const Customisation& customisation,
                    const CustomisationWriteOptions& options = {});

// One definition as the format's text - the very object a file holds in its
// "linestyles" or "symbols" array, strokes one a line - for an editor to show
// and take back. What a file says of a definition from outside it is given
// here instead: `customisation` is the name it would sit under, so its source
// is written (as "from") only when it differs and read as that name when the
// text gives none; `symbol` is the array it would sit in.
//
// They fail as customisationToJson and customisationFromJson do, with the
// entry named `definition "<name>"`; reading also runs entity::validate, since
// no library is there to.
[[nodiscard]] katana::core::Result<std::string>
definitionToJson(const LineStyle& definition, std::string_view customisation = {});
[[nodiscard]] katana::core::Result<LineStyle>
definitionFromJson(std::string_view text, bool symbol, std::string_view customisation = {});

// The digest of a customisation file's bytes: FNV-1a, 64-bit, as 16 lower-case
// hexadecimal digits ("cbf29ce484222325" for no bytes). It tells two editions
// of one name apart (CustomisationBase); it is not a security hash and guards
// against nothing deliberate.
[[nodiscard]] std::string customisationDigest(std::string_view bytes);

} // namespace katana::entity
