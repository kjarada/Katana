#pragma once

// Which 12d customisation files are loaded, and what a project records of them.
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

#include <string>
#include <vector>

#include "katana/entity/style_library.hpp"

namespace katana::cad {

// One loaded file, by name (never a path: a project travels between
// machines).
struct CustomisationSource {
    std::string name{};
    // A linestyle or symbol library (.4d); otherwise a mapfile.
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

// Of the names a project `recorded`, those that are not in
// customisationRecord(library, loaded), in the project's order: what to warn
// about when the project is opened. Names compare exactly: a file loaded
// under another spelling is reported as missing, which a warning can afford,
// where the reverse - a missing file passed as loaded - it could not.
[[nodiscard]] std::vector<std::string>
customisationNotLoaded(const std::vector<std::string>& recorded,
                       const katana::entity::StyleLibrary& library,
                       const std::vector<CustomisationSource>& loaded);

} // namespace katana::cad
