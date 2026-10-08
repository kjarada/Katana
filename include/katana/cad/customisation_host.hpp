#pragma once

// Where a session's customisation comes from when it starts: the built-in one
// and the user's kept one, and the one function that chooses between them.
//
// A Document starts EMPTY and installs nothing by itself - a test, a tool and
// a session given no host all draw plain lines, and say so. A front end that
// wants the program's customisation hands over a HOST: the built-in (which a
// build may not have) and the path of the file the user keeps their own in
// (which a front end may not have: only the desktop has a per-user place).
// startCustomisation then does, once for every front end, what each would
// otherwise do in its own words and with its own omissions.
//
// THE BUILT-INS are Katana customisation files compiled into the program
// (CMake's KATANA_BUILTIN_CUSTOMISATION and KATANA_ALTERNATE_CUSTOMISATIONS,
// #embed). One of them is THE built-in, the DEFAULT: Katana Standard, which a
// program starts with when nothing says otherwise. The others are alternates,
// the NSW customisation among them, which a session may be given by name
// (CUSTOMISE BUILTIN <name>) or a run may start with (the seam, below). The
// files are tracked in the repository (resources/customisation), so a build
// made from a clean checkout has them; a path that names no file is simply not
// offered, and a build with none runs without one - a state the program runs
// in, not a fault.
//
// THE SEAM. The environment variable KATANA_BUILTIN_CUSTOMISATION says what
// "the built-in" is for one run, whatever was compiled in:
//
//   (unset, empty)  the default built-in, when there is one
//   none            no built-in at all
//   <name>          the compiled-in built-in of that name, in any letter case
//                   (a value with no / or \ that names one)
//   <file>          that Katana customisation file IS the built-in
//
// Tried in that order: `none`, then a name, then a file. A file in the working
// folder that is called like a built-in is given as ./name.
//
// It exists so that a test of a program is the same test on a machine whose
// build has the built-in and on one whose build has not: the suite sets `none`
// for programs that should start with nothing and names a small committed
// fixture where the built-in path itself is under test. It is read in ONE
// place, builtInCustomisation(); what it then means is
// builtInCustomisationFor's to say, so that the rule itself is tested the
// same on both machines.

#include <cstddef>
#include <filesystem>
#include <memory>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/customisation_state.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/customisation.hpp"

namespace katana::cad {

// A Katana customisation file, read: the customisation, and the digest of the
// bytes it was read from (entity::customisationDigest) - what tells one
// edition of a name from another.
struct CustomisationFile {
    katana::entity::Customisation customisation{};
    std::string digest{};
};

// Reads the file at `path` (entity::customisationFromJson has what a file
// may be refused for). NotFound when it cannot be opened, FileImportFailure
// when it cannot be read; a refusal of its contents keeps its own code and
// message, with the path put in front of its context.
[[nodiscard]] katana::core::Result<CustomisationFile>
readCustomisationFile(const std::filesystem::path& path);

// A built-in customisation as a host holds it.
struct BuiltInCustomisation {
    // Null: there is none. Shared rather than copied: it is eight hundred
    // definitions, and a host, a reset and a comparison all read the same one.
    std::shared_ptr<const katana::entity::Customisation> customisation{};
    // The digest of the bytes it was read from; empty when there is none.
    std::string digest{};
    // Why there is none although there should be one: the compiled-in bytes,
    // or the file the seam names, are not a customisation. Empty when there
    // is one, and when none was asked for - an absent built-in is not a
    // problem.
    std::string problem{};
};

// The DEFAULT built-in compiled into this program, parsed once, at the first
// call: KATANA_BUILTIN_CUSTOMISATION's file, and alone - the alternates are
// not parsed for it, so a start costs the default's bytes only. None, with no
// problem, in a build that has none. One that does not parse is reported in
// `problem` and never thrown: a program whose built-in is damaged still
// starts, and says so.
[[nodiscard]] const BuiltInCustomisation& compiledInCustomisation();

// One built-in a program offers.
struct BuiltInEntry {
    // The customisation's own name, which is what CUSTOMISE BUILTIN <name> and
    // the seam take, in any letter case. For a file that did not read, the
    // file's name.
    std::string name{};
    // The customisation's own description; empty when it has none.
    std::string description{};
    // The file the build compiled it in from, by its name alone. Empty for a
    // built-in that was not compiled in: a file the seam named, a test's
    // stand-in.
    std::string source{};
    // As a host holds it; `problem` says why `customisation` is null.
    BuiltInCustomisation builtIn{};
};

// EVERY built-in compiled into this program, parsed once, at the first call:
// the default first when the build has one (it IS compiledInCustomisation(),
// shared, not parsed twice), then the alternates in the order the build gave
// them. A file that does not parse is listed under its file name with the
// reason in `builtIn.problem`; a later file whose customisation has an earlier
// one's name (letter case ignored) is not offered, so that a name picks one.
// Empty in a build that has none. Never throws.
[[nodiscard]] const std::vector<BuiltInEntry>& builtIns();

// The entry of `entries` called `name`, letter case ignored; null when there
// is none.
[[nodiscard]] const BuiltInEntry* findBuiltIn(std::span<const BuiltInEntry> entries,
                                              std::string_view name);

// The built-in of THIS RUN: what the seam says, else the default -
// builtInCustomisationFor of the environment variable's value,
// compiledInCustomisation() and, where the value is a bare word, builtIns().
// THIS, and not compiledInCustomisation(), is what a front end asks for the
// host it hands over: once, at start-up. The variable is read at each call.
// Never throws.
[[nodiscard]] BuiltInCustomisation builtInCustomisation();

// The rule of the seam, apart from the things it is a rule about, so that it
// can be tested whatever this build compiled in and whatever the environment
// holds. `seam` is the variable's value - empty when it is not set -
// `compiledIn` the default the program has, and `named` the built-ins it
// offers by name:
//
//   (empty)   `compiledIn`, as it is - its problem too, when it has one
//   none      no built-in and no problem, in any letter case
//   <name>    a value with no '/' or '\' that is the name of an entry of
//             `named`, letter case ignored: that entry's built-in, as it is
//   <file>    that Katana customisation file, read (readCustomisationFile).
//             One that does not read gives none and the reason, naming the
//             variable and the file - never `compiledIn` in its place, which
//             would pass a test that named a fixture and got something else.
//             For a bare word, the reason also lists the names `named` has,
//             because a mistyped name is the likely cause.
//
// The file is named as text: UTF-8, or the narrow bytes an environment gave
// (core::pathFromUtf8, which takes both and throws on neither).
[[nodiscard]] BuiltInCustomisation builtInCustomisationFor(std::string_view seam,
                                                           const BuiltInCustomisation& compiledIn,
                                                           std::span<const BuiltInEntry> named = {});

// The name of the environment variable the seam is.
inline constexpr const char* kBuiltInCustomisationVariable = "KATANA_BUILTIN_CUSTOMISATION";

// The name of the environment variable that names the kept file: the only way
// a headless run, katana_cli or katana_mcp has one (a run that must be the
// same every time reads no per-user place), and what overrides the desktop's
// own. cad never reads it - a front end does, when it builds its host - but
// the name is here, beside the seam's, because CUSTOMISE KEEP says it to a
// session that was given no kept file.
inline constexpr const char* kKeptCustomisationVariable = "KATANA_CUSTOMISATION";

// What a front end hands over.
struct CustomisationHost {
    // The built-in of this run: what RESET gives and a start falls back to.
    BuiltInCustomisation builtIn{};
    // Where the user's own customisation is kept between runs. Empty: this
    // front end keeps none. The file need not exist.
    std::filesystem::path keptFile{};
    // The built-ins this host offers by name (CUSTOMISE BUILTIN <name>). Unset:
    // those compiled into the program, builtIns(), which is what every front
    // end wants and none sets. A test sets it to stand in for the build, so
    // that it is the same test on a machine whose build compiled in different
    // files.
    std::optional<std::vector<BuiltInEntry>> builtIns{};
};

// The built-ins `host` offers by name: its own list when it gave one, else
// those compiled into the program. In the build's order, the default first
// when it has one. The built-in of the run (host.builtIn) is one of them in a
// real run, whether it is the default or the seam named another by name; a
// seam that names a FILE makes a built-in that is none of them, which RESET
// still gives and no list shows.
[[nodiscard]] const std::vector<BuiltInEntry>& builtInsOf(const CustomisationHost& host);

// One built-in as a person or an agent is shown it: what it holds, and how it
// stands to this session. The one answer that CUSTOMISE BUILTIN, the JSON
// report (customisationJson) and Settings' chooser all read, so that the three
// cannot come to disagree.
struct BuiltInChoice {
    std::string name{};
    std::string description{};
    std::string source{};
    // Why it is not usable, when it is not; the counts are then 0.
    std::string problem{};
    std::size_t definitions = 0; // linestyles and symbols together
    std::size_t codes = 0;       // distinct keys among the rules
    std::size_t rules = 0;
    // The built-in of the run: the one RESET gives and the next start falls
    // back to. (CustomisationState::builtIn names it.)
    bool isDefault = false;
    // The session IS this built-in as installed: its origin is the built-in
    // and its name is this entry's. A kept or edited copy of it is not active;
    // it is based on it (CustomisationState::basedOn).
    bool isActive = false;
};

// `entries` as choices, in their order, against `document`'s session.
[[nodiscard]] std::vector<BuiltInChoice> builtInChoices(const Document& document,
                                                        std::span<const BuiltInEntry> entries);

// Installs `builtIn` as the session's customisation, origin BuiltIn, based on
// ITSELF - basedOn its own name and `builtIn.digest`, whatever its file says
// it was made from, and no basedOn at all when the host gave no digest. ONE
// place for that rule: a start with no kept file (startCustomisation) and
// CUSTOMISE RESET both install through it, and CUSTOMISE KEEP asks it what
// "the session is the built-in" means. `kept`: whether the next start would
// give this session (Document::installCustomisation).
//
// InvalidState when `builtIn` holds none; otherwise whatever
// Document::installCustomisation refuses, the Document left as it was.
[[nodiscard]] katana::core::Status installBuiltInCustomisation(Document& document,
                                                               const BuiltInCustomisation& builtIn,
                                                               bool kept);

// What startCustomisation did, for the front end to say in its own words.
struct CustomisationStart {
    // Kept, BuiltIn, or None when nothing was installed.
    CustomisationOrigin installed = CustomisationOrigin::None;
    std::string name{};
    std::size_t definitions = 0; // linestyles and symbols together
    std::size_t symbols = 0;     // those listed as symbols
    std::size_t rules = 0;
    std::size_t colours = 0;
    // The kept customisation was made from another built-in than this program
    // has, or another edition of it: it says what it is based on, and that is
    // neither the run's built-in nor any other the program offers, in the
    // edition it has (name and digest). A copy of an alternate the user chose
    // and kept (CUSTOMISE BUILTIN <name>, then KEEP) is made from a built-in
    // the program has, so it is not this. The kept customisation is installed
    // all the same - it is the user's - and they may want to know that the
    // built-in has moved on. False when the kept file does not say, and when
    // there is no built-in to have moved on.
    bool keptFromAnotherBuiltIn = false;
    // What went wrong, in sentences: the built-in that did not parse, the
    // kept file that did not read (the built-in then stands in for it).
    std::vector<std::string> problems{};
};

// Installs the customisation a session starts with: the kept file when it
// exists and reads (origin Kept), else the built-in (origin BuiltIn), else
// nothing, and the Document stays as it was. What it installs is `kept`: it
// is what the next start would give, the linework codes and the automation
// included (Document::installCustomisation). The built-in is installed based
// on ITSELF - basedOn its own name and the digest of its own bytes, whatever
// its file says it was made from - so that a copy kept from it later can be
// told from one made against another edition. (A built-in that was itself
// exported from a session names an earlier one; kept as that, every copy
// made from it would be reported as made from another built-in, at every
// start.)
//
// It also tells the Document the built-in's name, which answers the names a
// project recorded for the built-in's earlier files, and what this start
// found - the report's `problems` and `keptFromAnotherBuiltIn`
// (Document::setCustomisationStart) - so that a report of the session asked
// for later (customisationJson) still says what a front end said once.
[[nodiscard]] CustomisationStart startCustomisation(Document& document,
                                                    const CustomisationHost& host);

} // namespace katana::cad
