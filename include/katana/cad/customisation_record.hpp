#pragma once

// Which customisations are loaded, and what a project records of them.
//
// The customisation is SESSION data (decision D1): the library and the survey
// map live on the Document, are not undoable and are not saved in the
// project. What a project does keep is a RECORD of what it was drawn with, by
// name (storage::ProjectMetadata::customisation), so that opening it where
// that is not loaded can say which names are missing rather than drawing plain
// lines without a word.
//
// The Document keeps the list of what was loaded and applies these functions
// itself: Document::open works out what is missing and a save writes the
// record into what it saves (document.hpp, "the customisation's session
// state"). They are functions over plain lists so that the rules can be tested
// without a project on disk. A source arrives here as its name and what it
// brought, whoever read it.

#include <cstdint>
#include <filesystem>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "katana/entity/customisation.hpp"
#include "katana/entity/style_library.hpp"

namespace katana::cad {

// One loaded customisation, by name (never a path: a project travels between
// machines), with what it brought and its author's notice: {name, definitions,
// rules, notice}. It is the very entry a Katana customisation file keeps in
// its "sources" (entity/customisation.hpp), so the session's load record is
// written and read back without a second type.
//
// Two flags, where there was one `library` bool: a style library brought
// definitions and a survey code file rules, but one Katana customisation
// brings both, and a record that could say only one of them would lose the
// source at the first Replace of the other kind. A source's NAME is its
// identity: two entries of one name are one source.
using CustomisationSource = katana::entity::CustomisationSourceNote;

// Adds a load to `loaded`, which is in load order. A source loaded again
// moves to the end, where the load puts it, still bringing what it brought
// before as well. `replacedDefinitions` / `replacedRules`: the load REPLACED
// that kind (LoadMode::Replace, and it brought some), so what the earlier
// sources brought of it is gone - and a source that then brings neither kind
// goes with it.
void recordCustomisationLoad(std::vector<CustomisationSource>& loaded,
                             const std::vector<CustomisationSource>& load,
                             bool replacedDefinitions, bool replacedRules);

// The names a project records: the loaded sources in load order, then, in
// name order, any definition's LineStyle::source no loaded source accounts
// for. A source that brought definitions ALONE is recorded only while some
// definition of `library` still comes from it (a later load can have replaced
// every one); rules carry no source name, so a source that brought any is
// taken at its word.
[[nodiscard]] std::vector<std::string>
customisationRecord(const katana::entity::StyleLibrary& library,
                    const std::vector<CustomisationSource>& loaded);

// A source that is now loaded under a name other than the one a project may
// have recorded for it. `formerName` is sourceNameHash of that earlier name,
// not the name itself: see builtinRenames for why.
struct RenamedSource {
    std::uint64_t formerName = 0;
    // The name that has its place now. Empty answers nothing: the table of a
    // program with no built-in customisation has nothing to answer with.
    std::string now{};
    // Whether the earlier name is particular enough to be known by on its own.
    // A plain one - a name anyone's own file might have - is taken for the
    // renamed source only in the record of the set the rename was made to:
    // one that also holds ANOTHER earlier name of the same `set`, distinctive
    // or plain. Alone, or beside names of other sets, it is some other file,
    // and passing it as loaded would hide that it is missing.
    bool distinctive = true;
    // Which generation of earlier names this one belongs to. One customisation
    // has been recorded under several sets of names over time, and a record
    // made at one time holds the names of one set: names of two sets beside
    // each other are evidence of neither.
    int set = 0;
};

// FNV-1a (64-bit) of the name's bytes, exactly as spelt: the key RenamedSource
// knows an earlier name by.
[[nodiscard]] std::uint64_t sourceNameHash(std::string_view name);

// The names a project may have recorded for the built-in customisation before
// it was ONE customisation with a name of its own, each answered by that
// name, `builtIn`: eight entries in two sets.
//
// * Set 0: the four files it was first shipped as, in load order. Their names
//   carried the name of the customisation's publisher, which has no place in
//   this source, so they are known by hash. The second survey code file's
//   name was a plain one, without the publisher's, and is marked so
//   (RenamedSource::distinctive).
// * Set 1: the general names those four files were given next, in load
//   order - "linestyles.4d", "survey_codes.mapfile",
//   "survey_codes_names.mapfile" and "symbols.4d". All four are plain, names
//   anyone's own files might have, so each is answered only beside another of
//   the four: the record of the built-in holds all four, and a person's own
//   "symbols.4d" recorded alone is still that person's file, missing when it
//   is.
//
// The built-in's name is not written in this source. A caller that knows it
// passes it - the Document passes the name its host's built-in declares
// (CustomisationState::builtIn), and is the one caller a session has. The
// form without one takes the name of the customisation COMPILED INTO this
// program (compiledInCustomisation, customisation_host.hpp), which is empty,
// and so answers nothing, in a build that has none - and which is not the
// built-in of a run the seam gave another. It is for a caller with no
// Document to ask; the functions below take the table they are given, and
// have no form that guesses one.
[[nodiscard]] std::vector<RenamedSource> builtinRenames(std::string_view builtIn);
[[nodiscard]] std::vector<RenamedSource> builtinRenames();

// Of the names a project `recorded`, those that are not in
// customisationRecord(library, loaded), in the project's order: what to warn
// about when the project is opened. Names compare exactly: a file loaded
// under another spelling is reported as missing, which a warning can afford,
// where the reverse - a missing file passed as loaded - it could not.
//
// The one exception is a file RENAMED since the project was saved: a recorded
// name `renamed` knows is answered by the file that now has its place, and is
// missing only when that file is. A project saved before the built-in
// customisation was given general names records the old names, and what
// those files brought is loaded under the new ones; telling a person four
// files are missing, and keeping them in the record for ever after, when
// nothing is, would be a false alarm nobody could clear. A plain earlier name
// (RenamedSource::distinctive false) is answered this way only beside another
// earlier name of its set, so a person's own file of that name is still
// missing when it is. `renamed` is the caller's to give, here and below, and
// has no default: a table answers with ONE built-in's name, and only the
// session knows its own - the seam can make it another than the one compiled
// in, which is all a default could have answered with.
[[nodiscard]] std::vector<std::string>
customisationNotLoaded(const std::vector<std::string>& recorded,
                       const katana::entity::StyleLibrary& library,
                       const std::vector<CustomisationSource>& loaded,
                       std::span<const RenamedSource> renamed);

// A load's files, each once, in the order first named: read twice, a file's
// every rule would sit in the map twice, since both copies are the load's and
// the merge keeps them all. `repeats` holds each later naming, as named, for
// the front end to say it was read once. Both front ends (the window's CUSTOMISE
// and File > Settings > Import, the command line's CUSTOMISE) go through this, so
// they cannot come to differ on the same line again.
struct DistinctFiles {
    // Absolute and lexically normal: what is read.
    std::vector<std::filesystem::path> files{};
    std::vector<std::filesystem::path> repeats{};
};

// Two namings are one file when their absolute, lexically normal paths are
// equal, or when both exist and std::filesystem::equivalent says so (another
// letter case on Windows, a link). A pair it cannot tell apart is read twice,
// which doubles rules; it never drops a file.
[[nodiscard]] DistinctFiles distinctCustomisationFiles(
    const std::vector<std::filesystem::path>& named);

// After a load: the names opening the project found not loaded
// (`missingAtOpen`, from customisationNotLoaded) that this load brought are
// no longer missing, and the session is now the judge of them. A name the
// load brought under the name `renamed` gives it now counts as brought, as it
// does for customisationNotLoaded, so loading the files a warning asks for
// clears it; a plain earlier name counts only while another earlier name of
// its set is missing beside it, `missingAtOpen` being all of the record this
// sees.
void noteCustomisationLoaded(std::vector<std::string>& missingAtOpen,
                             const std::vector<CustomisationSource>& load,
                             std::span<const RenamedSource> renamed);

// The record a save writes: customisationRecord(library, loaded) - what this
// session draws the drawing with - then, in the project's order, the names
// the project already `recorded` that are still in `missingAtOpen` and that
// customisationNotLoaded still finds missing. A file the open found missing
// and no load has brought since is one this session cannot judge: saving
// without it must not forget it, or every later open, anywhere, would draw
// plain lines without a word. One this session loaded and then replaced, it
// has judged, and it goes; so does an earlier name whose file is loaded now
// under the name `renamed` gives it, judged against the whole record. A
// person's own file that shares a plain earlier name is kept while it is
// missing. `recorded` is the document's current record, so a list left over
// from an earlier drawing keeps nothing a new one never recorded.
[[nodiscard]] std::vector<std::string>
customisationRecordToSave(const std::vector<std::string>& recorded,
                          const std::vector<std::string>& missingAtOpen,
                          const katana::entity::StyleLibrary& library,
                          const std::vector<CustomisationSource>& loaded,
                          std::span<const RenamedSource> renamed);

// A definition of the built-in customisation that has another name than the
// one a drawing saved earlier may give it (as a style's linetype, or a placed
// symbol's name). `formerName` is sourceNameHash of the earlier name, for the
// reason builtinRenames gives.
struct RenamedDefinition {
    std::uint64_t formerName = 0;
    std::string_view now{};
};

// The built-in definitions renamed when the built-in customisation was given
// general names: those whose names began with its publisher's name, which
// they no longer carry. In the byte order of the names they have now.
[[nodiscard]] std::span<const RenamedDefinition> builtinDefinitionRenames();

// For a definition `name` that an exact lookup did not find: the name a
// rename in `renamed` gives it now, to look up in its place; empty when no
// rename knows `name`. Only a lookup that MISSED asks, so a definition that
// has the earlier name itself, from a person's own library, is still the one
// drawn. The one-argument form uses builtinDefinitionRenames.
[[nodiscard]] std::string_view definitionNameNow(std::string_view name);
[[nodiscard]] std::string_view definitionNameNow(std::string_view name,
                                                 std::span<const RenamedDefinition> renamed);

// Whether a typed `line` (the verb SAVE and its arguments) can save: with one
// argument, a project directory, or with none when the drawing already has a
// project. The arguments are split as the command interpreter splits them -
// on blanks, double quotes grouping and removed, an unclosed quote refused.
// A front end writes what it keeps in the metadata - the reference layers -
// only when this holds: Document::setMetadata marks the drawing modified, and
// a SAVE that could not go ahead would otherwise leave a drawing nobody
// touched asking to be saved. The customisation record was written that way
// too and needs no such guard now: the save itself writes it into what it
// saves (Document::save), and a save that does not go ahead writes nothing.
[[nodiscard]] bool typedSaveHasDestination(std::string_view line, bool hasProject);

} // namespace katana::cad
