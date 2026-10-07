#pragma once

// What a PART of a customisation is written as: its codes for a colleague, two
// of its symbols, its linestyles without its rules.
//
// One rule, for everything that writes less than the whole: CUSTOMISE EXPORT
// with a kind word or ONLY (customisation_verbs.hpp), the Survey Code Manager's
// Export Codes and the Symbol Library's Export Selected. The verb and the two
// managers were written side by side and cut a session down by two rules that
// disagreed - about the colours, about whose notice travels and about what a
// part is based on - so the same symbols exported from the window and from
// the command line were two different files.
//
//   name, description,  the customisation's own, as they are. A notice is
//   notice              "carried with the data and shown to whoever uses it"
//                       (entity/customisation.hpp), and a part of a
//                       customisation is its author's data too.
//   sources             those that brought what is WRITTEN, each said to have
//                       brought that alone, with its own notice:
//                         definitions  a written definition came from it
//                                      (its LineStyle::source)
//                         rules        the rules are written, and it brought
//                                      some
//                         neither      it brought neither kind - a table of
//                                      colours - and the part carries a colour
//                       A file's sources are taken at their word where it is
//                       loaded: each becomes a source of that session BY NAME,
//                       and a project's warning that a customisation is
//                       missing goes by name too. One listed with nothing of
//                       it written would pass for loaded.
//   the notice of a     written with the part's own, after it, each line
//   source left out     once. Nothing says whose a colour is, and an author's
//                       notice is never dropped by an export.
//   colours             those the written rules and the written definitions'
//                       pens NAME - a rule's colour, its symbol's and its
//                       text's; a pen stroke - compared as colour names are
//                       (entity::foldColourName). Without them the part would
//                       draw elsewhere in other colours than here; the rest
//                       would overwrite colours of whoever loads it that
//                       nothing in the file uses.
//   linework,           absent: a part says nothing of them, so loading it
//   automation          leaves the session's own alone. "A file of symbols for
//                       a colleague must not reset their control codes"
//                       (entity/customisation.hpp), nor a file of codes.
//   basedOn             absent: a part is not an edition of the built-in, to
//                       be told from another edition at the next start.
//
// The WHOLE customisation is not a part and is not asked of this: an export
// with no kind word, and CUSTOMISE KEEP, write everything as it is.

#include "katana/entity/customisation.hpp"

namespace katana::cad {

// `session` cut down to the part a write with `written` holds, to be handed to
// entity::customisationToJson WITH THE SAME `written`.
//
// The definitions and the rules themselves are left whole: the writer chooses
// among them by `written` and refuses a name it cannot write (one the library
// lacks, or one whose kind is not being written), and what is counted here as
// written is chosen by the same rule - a definition of a kind `written` names
// and, when `written.only` is not empty, of a name in it; the rules when
// `written.codes`. A name `only` gives that the writer would refuse is simply
// not counted.
//
// It reads nothing but the two values, so a session edited in a buffer - the
// Survey Code Manager's rules, not yet applied - is a part like any other:
// the caller puts the buffer in `session.map` first.
[[nodiscard]] katana::entity::Customisation
customisationPart(katana::entity::Customisation session,
                  const katana::entity::CustomisationWriteOptions& written);

// What a load of DEFINITIONS alone (CUSTOMISE DEFINITIONS, which the Symbol
// Library's Import Definitions runs) takes of a file: its definitions and its
// colours. Its survey code rules are the Survey Code Manager's to review; its
// control codes and switches are settings of whoever wrote the file.
struct DefinitionsOfFile {
    katana::entity::Customisation taken{};
    // What was left out, counted for the reply: the rules, and whether the
    // file said anything of the linework codes or the automation.
    std::size_t rulesLeft = 0;
    bool lineworkLeft = false;
    bool automationLeft = false;
};

// `file` without its rules, linework codes and automation - and without the
// sources that brought it only rules. A source it lists becomes one of the
// session's, by name, and is taken off what the open project is missing
// (Document::installCustomisation): one left in with its rules flag cleared
// would pass for loaded with none of its rules here.
//
//   a source that brought definitions     kept, and said to have brought those
//                                         ALONE: its rules are not taken
//   one that brought rules alone          dropped, its notice with it
//   one that brought neither (a table     kept only when the file has colours,
//   of colours)                           which the load takes
//
// It goes by what the file SAYS each source brought - the load takes every
// definition of the file - where a part being WRITTEN goes by where each
// written definition came from (customisationPart).
[[nodiscard]] DefinitionsOfFile definitionsOfFile(katana::entity::Customisation file);

} // namespace katana::cad
