#pragma once

// CUSTOMISE: the session's customisation on the text command line
// (docs/customisation.md, "The verbs"). The interpreter's, so the window's
// command line, katana_cli, katana_mcp and an AI agent run ONE family; it was
// written twice, by hand, in the two front ends, because the readers of the
// older files lived where cad could not see them, and the two had come to
// differ (one refused a load with problems, the other installed it).
//
//   CUSTOMISE                            what is loaded, as records
//   CUSTOMISE JSON                       the same as one JSON object
//   CUSTOMISE <file>...                  merge Katana customisation files in
//   CUSTOMISE REPLACE <file>...          in the place of each kind they bring
//   CUSTOMISE DEFINITIONS <file>...      merge their definitions and colours alone
//   CUSTOMISE EXPORT <file> [CODES] [LINESTYLES] [SYMBOLS] [NAME <name>]
//                                        [ONLY <definition>...]
//   CUSTOMISE RESET                      the host's built-in customisation
//   CUSTOMISE KEEP                       write the session to the kept file
//   CUSTOMISE REVERT                     read the kept file again
//   CUSTOMISE REMOVE <definition>... [FORCE]
//   CUSTOMISE REMOVE CODE <key>...
//   CUSTOMISE SET <key>=<value>...       auto.codes, auto.linework, linework.*
//
// A KEYWORD IS THE WHOLE FIRST WORD, in any case. The command line removes
// quotes before a verb sees its words, so "a quoted word is a file" - the
// rule the session's own parser kept - cannot be kept here; a file that is
// literally called as a keyword is given with its directory (./json), as a
// file called as a scope word is (scope_verbs.hpp). The interpreter hands
// over the words after the CANONICAL verb, so CUSTOMIZE is the same family.
//
// THE FRAGMENT IS THE ONE DOOR FOR AN EDIT. Nothing here edits a rule or a
// definition in place: a customisation file holding only what changes is
// merged in (CUSTOMISE <file>), which replaces a definition by its name and
// a key's rules by their section. REMOVE exists because a merge cannot
// delete, and SET because the switches and the control codes are settings,
// not data.
//
// THE HOST is what a front end hands the interpreter - the built-in
// customisation and the path of the kept file (customisation_host.hpp). A
// session given none (a test's bare Document, a tool) has everything here
// but RESET, KEEP and REVERT, which are refused by name.
//
// Every reply is key=value records, one a line (recordValue,
// scope_verbs.hpp), a file written with '/' in its path; no reply to a line
// that succeeded holds the word a script fails on.

#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/customisation_host.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/error.hpp"

namespace katana::cad {

// What the family keeps between lines, beside the Document.
struct CustomisationVerbContext {
    // Empty: no front end handed a host over.
    std::optional<CustomisationHost> host{};
    // The kept file as this session last read or wrote it: the digest of its
    // bytes (entity::customisationDigest), or nothing when it was not there.
    // CUSTOMISE KEEP refuses to write over a file that is another now: a
    // second Katana kept its own there, and writing would lose it silently.
    std::optional<std::string> keptDigest{};
};

// The context of a session that begins now: `host`, and its kept file as it
// is at this moment. So hand it over where the session starts - BEFORE
// startCustomisation reads the file, not after: read the other way round, a
// file another Katana wrote between the two would be taken for the one this
// session saw.
[[nodiscard]] CustomisationVerbContext customisationVerbContext(CustomisationHost host);

// True for CUSTOMISE, in any case. (CUSTOMIZE is the interpreter's alias.)
[[nodiscard]] bool isCustomisationVerb(std::string_view verb);

// Runs one CUSTOMISE line: `args` are the words AFTER the verb, the quotes
// removed (CommandInterpreter::tokenize). A refused line changes nothing:
// neither the session nor a file. `context` is updated by KEEP and REVERT.
[[nodiscard]] katana::core::Result<std::string>
runCustomisationVerb(Document& document, const std::vector<std::string>& args,
                     CustomisationVerbContext& context);

// Every word of the family and every reply, for HELP CUSTOMISE.
[[nodiscard]] std::string customisationVerbHelp();

} // namespace katana::cad
