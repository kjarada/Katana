#pragma once

// What a Document knows of the customisation it is drawn with, beyond the
// definitions and the survey code rules themselves.
//
// The library and the survey map were all a Document held, and each front end
// kept the rest - which files were loaded, which names a project recorded
// that were not - in lists of its own. Two lists in two programs had already
// come to differ (after OPEN then NEW, one still named the old project's
// files), and a CUSTOMISE verb in the shared interpreter can reach neither.
// So the session's whole customisation is here, on the Document, beside the
// library and the map it describes.
//
// Like them it is SESSION data: not undoable, not saved in the project, kept
// across a new or an opened drawing (document.hpp).

#include <optional>
#include <string>
#include <vector>

#include "katana/cad/customisation_record.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/customisation.hpp"

namespace katana::cad {

// Where the customisation in a session came from.
enum class CustomisationOrigin {
    None,    // nothing has been installed: an empty Document
    BuiltIn, // the customisation that is part of the program
    Kept,    // the user's own kept customisation, read at start-up
    Loaded,  // a customisation loaded in this session
    Edited,  // changed in this session: an editor, or one of the setters
};

// "none", "builtIn", "kept", "loaded", "edited" - the words a reply names an
// origin by.
[[nodiscard]] const char* toString(CustomisationOrigin origin);

struct CustomisationState {
    // Empty until a customisation is installed. A session whose library or
    // map was set directly keeps the name it had.
    std::string name{};
    CustomisationOrigin origin = CustomisationOrigin::None;
    std::string description{};
    // The author's notice of the customisation itself, a line an entry. Each
    // source keeps its own beside it.
    std::vector<std::string> notice{};
    // What went into the session, in load order: each source's name, whether
    // it brought definitions, whether it brought rules, and its notice.
    std::vector<CustomisationSource> sources{};
    // The customisation an edited or kept copy started from, when it says.
    std::optional<katana::entity::CustomisationBase> basedOn{};
    // The colours the customisation's own names mean. A name is resolved
    // against this and then the standard names (colour_lookup.hpp).
    katana::entity::ColourTable colours{};
    // How the linework control codes are spelled: the defaults until a
    // customisation, or a setter, says otherwise.
    katana::entity::LineworkCodes linework{};
    // What is applied to survey data without being asked for.
    katana::entity::CustomisationAutomation automation{};
    // The session is what the next start would give: what start-up installed,
    // or what was last kept. False from the first change after either.
    bool kept = false;
    // The names the open project recorded that this session did not have when
    // it was opened, less those loaded since (customisation_record.hpp). A
    // save keeps them in the record; a new drawing has none.
    std::vector<std::string> missingAtOpen{};
    // The name the host's built-in customisation declares, whether or not it
    // is what is installed; empty when the host has none. It is what the
    // names a project recorded for the built-in's earlier files are answered
    // with (builtinRenames).
    std::string builtIn{};
    // What went wrong when the session's customisation was STARTED
    // (startCustomisation, customisation_host.hpp), a sentence each: a
    // built-in that did not read, a kept file that did not read and what
    // started in its place. A front end says them once, at the start, where
    // its errors go - which a client of katana_mcp never reads, so that an
    // agent whose kept file was refused was told `origin: builtIn` and no
    // more. Kept here they are in the report whenever it is asked for
    // (customisationJson, "start"). They describe the start: nothing after
    // it changes them, and a session never started with a host has none.
    std::vector<std::string> startProblems{};
    // The kept customisation the session started with was made from another
    // built-in than the host has, or another edition of it
    // (CustomisationStart::keptFromAnotherBuiltIn). Of the start, as above.
    bool keptFromAnotherBuiltIn = false;

    friend bool operator==(const CustomisationState&, const CustomisationState&) = default;
};

// Everything about `customisation` that would make a session of it unfit to
// be KEPT or RECORDED, each an InvalidArgument whose message begins with the
// part it is about:
//
//   its name: ...                          validateCustomisationName refuses it
//   the name of one of its sources: ...    the same, of a source it lists
//   the source of its definition "X": ...  the same, of a definition's
//                                          LineStyle::source, when it has one
//   its linework codes: ...                entity::validate refuses them
//   its basedOn: ...                       the format's writer refuses it: a
//                                          name as above, or a digest that is
//                                          not one
//
// Empty: it can be installed. A project records the name, each source's name
// and each definition's source, a line each, and the project store refuses a
// name it could not read back - after which EVERY save of the session fails;
// the kept file is written by the format's writer, which refuses the rest. A
// customisation read from a file has passed all of this already
// (entity::customisationFromJson); one built in code has not.
// Document::installCustomisation refuses on the first of them, and
// mergeCustomisation lists them all, for each customisation of a load.
[[nodiscard]] std::vector<katana::core::Error>
customisationFaults(const katana::entity::Customisation& customisation);

} // namespace katana::cad
