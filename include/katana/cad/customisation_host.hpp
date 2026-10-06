#pragma once

// Where a session's customisation comes from when it starts: the built-in one
// and the user's kept one, and the one function that chooses between them.
//
// A Document starts EMPTY and installs nothing by itself - a test, a tool and
// a session given no host all draw plain lines, and say so. A front end that
// wants the program's customisation hands over a HOST: the built-in (which a
// build may not have) and the path of the file the user keeps their own in
// (which a front end may not have: only the desktop has a per-user place).
// startCustomisation then does what each front end once did for itself, in
// its own words and with its own omissions.
//
// THE BUILT-IN is a Katana customisation file compiled into the program
// (CMake's KATANA_BUILTIN_CUSTOMISATION, #embed). The file is not part of the
// repository, so a build made from a clean checkout has none, and that is a
// state the program runs in, not a fault.
//
// THE SEAM. The environment variable KATANA_BUILTIN_CUSTOMISATION says what
// "the built-in" is for one run, whatever was compiled in:
//
//   (unset, empty)  the compiled-in customisation, when there is one
//   none            no built-in at all
//   <file>          that Katana customisation file IS the built-in
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

// The customisation compiled into this program, parsed once, at the first
// call. None, with no problem, in a build that has none. One that does not
// parse is reported in `problem` and never thrown: a program whose built-in
// is damaged still starts, and says so.
[[nodiscard]] const BuiltInCustomisation& compiledInCustomisation();

// The built-in of THIS RUN: what the seam says, else the compiled-in one -
// builtInCustomisationFor of the environment variable's value and
// compiledInCustomisation(). THIS, and not compiledInCustomisation(), is what
// a front end asks for the host it hands over: once, at start-up. The
// variable is read at each call. Never throws.
[[nodiscard]] BuiltInCustomisation builtInCustomisation();

// The rule of the seam, apart from the two things it is a rule about, so that
// it can be tested whatever this build compiled in and whatever the
// environment holds. `seam` is the variable's value - empty when it is not
// set - and `compiledIn` what the program has:
//
//   (empty)   `compiledIn`, as it is - its problem too, when it has one
//   none      no built-in and no problem, in any letter case
//   <file>    that Katana customisation file, read (readCustomisationFile).
//             One that does not read gives none and the reason, naming the
//             variable and the file - never `compiledIn` in its place, which
//             would pass a test that named a fixture and got something else.
//
// The file is named as text: UTF-8, or the narrow bytes an environment gave
// (core::pathFromUtf8, which takes both and throws on neither).
[[nodiscard]] BuiltInCustomisation builtInCustomisationFor(std::string_view seam,
                                                           const BuiltInCustomisation& compiledIn);

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
    BuiltInCustomisation builtIn{};
    // Where the user's own customisation is kept between runs. Empty: this
    // front end keeps none. The file need not exist.
    std::filesystem::path keptFile{};
};

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
    // not this built-in's name and digest. It is installed all the same - it
    // is the user's - and they may want to know that the built-in has moved
    // on. False when the kept file does not say, and when there is no
    // built-in to have moved on.
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
