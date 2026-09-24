#pragma once

// Which customisation files are loaded, and what a project records of them.
//
// The customisation is SESSION data (decision D1): the library and the survey
// map live on the Document, are not undoable and are not saved in the
// project. What a project does keep is a RECORD of the files it was drawn
// with, by name (storage::ProjectMetadata::customisation), so that opening it
// where they are not loaded can say which are missing rather than drawing
// plain lines without a word. The front ends (the window and the command
// line) keep the list of loaded files with these functions: cad may not see
// archive12d, which reads the files, so a file arrives here as its name and
// kind alone.

#include <cstdint>
#include <filesystem>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "katana/entity/style_library.hpp"

namespace katana::cad {

// One loaded file, by name (never a path: a project travels between
// machines).
struct CustomisationSource {
    std::string name{};
    // A linestyle or symbol library (.4d); otherwise a survey code file.
    bool library = false;

    friend bool operator==(const CustomisationSource&, const CustomisationSource&) = default;
};

// Adds a load to `loaded`, which is in load order. A file loaded again moves
// to the end, where the load puts it. `replacedLibrary` / `replacedMap`: the
// load REPLACED that kind (archive12d::LoadMode::Replace, and it brought
// some), so the earlier files of that kind no longer contribute and go.
void recordCustomisationLoad(std::vector<CustomisationSource>& loaded,
                             const std::vector<CustomisationSource>& load, bool replacedLibrary,
                             bool replacedMap);

// The names a project records: the loaded files in load order - a library
// file only while some definition of `library` still comes from it (a later
// file can have replaced every one) - then, in name order, any definition's
// LineStyle::source no loaded file accounts for.
[[nodiscard]] std::vector<std::string>
customisationRecord(const katana::entity::StyleLibrary& library,
                    const std::vector<CustomisationSource>& loaded);

// A file that is now loaded under a name other than the one a project may have
// recorded for it. `formerName` is sourceNameHash of that earlier name, not the
// name itself: see builtinRenames for why.
struct RenamedSource {
    std::uint64_t formerName = 0;
    std::string_view now{};
};

// FNV-1a (64-bit) of the name's bytes, exactly as spelt: the key RenamedSource
// knows an earlier name by.
[[nodiscard]] std::uint64_t sourceNameHash(std::string_view name);

// The files of the customisation compiled into Katana, each with the name it
// had before the built-in customisation was given general names (the names
// ProjectMetadata::customisation of a project saved before then holds), in
// load order. The earlier names carried the name of the customisation's
// publisher, which has no place in this source, so they are known by hash.
[[nodiscard]] std::span<const RenamedSource> builtinRenames();

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
// nothing is, would be a false alarm nobody could clear. A person's own file
// that happens to share a former name exactly is answered the same way,
// which a warning can afford. The three-argument form uses builtinRenames.
[[nodiscard]] std::vector<std::string>
customisationNotLoaded(const std::vector<std::string>& recorded,
                       const katana::entity::StyleLibrary& library,
                       const std::vector<CustomisationSource>& loaded);
[[nodiscard]] std::vector<std::string>
customisationNotLoaded(const std::vector<std::string>& recorded,
                       const katana::entity::StyleLibrary& library,
                       const std::vector<CustomisationSource>& loaded,
                       std::span<const RenamedSource> renamed);

// A load's files, each once, in the order first named: read twice, a file's
// every rule would sit in the map twice, since both copies are the load's and
// the merge keeps them all. `repeats` holds each later naming, as named, for
// the front end to say it was read once. Both front ends (the window's
// CUSTOMISE and Format > Load, the command line's CUSTOMISE) go through this,
// so they cannot come to differ on the same line again.
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
// no longer missing, and the session is now the judge of them.
void noteCustomisationLoaded(std::vector<std::string>& missingAtOpen,
                             const std::vector<CustomisationSource>& load);

// The record a save writes: customisationRecord(library, loaded) - what this
// session draws the drawing with - then, in the project's order, the names
// the project already `recorded` that are still in `missingAtOpen`. A file
// the open found missing and no load has brought since is one this session
// cannot judge: saving without it must not forget it, or every later open,
// anywhere, would draw plain lines without a word. One this session loaded
// and then replaced, it has judged, and it goes. `recorded` is the
// document's current record, so a list left over from an earlier drawing
// keeps nothing a new one never recorded.
[[nodiscard]] std::vector<std::string>
customisationRecordToSave(const std::vector<std::string>& recorded,
                          const std::vector<std::string>& missingAtOpen,
                          const katana::entity::StyleLibrary& library,
                          const std::vector<CustomisationSource>& loaded);

// Whether a typed `line` (the verb SAVE and its arguments) can save: with one
// argument, a project directory, or with none when the drawing already has a
// project. The arguments are split as the command interpreter splits them -
// on blanks, double quotes grouping and removed, an unclosed quote refused.
// A front end writes the record into the metadata only when this holds:
// Document::setMetadata marks the drawing modified, and a SAVE that could
// not go ahead would otherwise leave a drawing nobody touched asking to be
// saved.
[[nodiscard]] bool typedSaveHasDestination(std::string_view line, bool hasProject);

} // namespace katana::cad
