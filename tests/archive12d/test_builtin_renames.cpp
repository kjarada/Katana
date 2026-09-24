// The renames the drawing side knows (katana::cad's customisation record)
// against the built-in customisation itself: the names they give now must be
// the ones the built-in files and definitions have, or a project or drawing
// saved before the rename would find nothing. Compiled only when cad is
// configured beside archive12d (tests/archive12d/CMakeLists.txt).

#include <gtest/gtest.h>

#include <string>
#include <vector>

#include "katana/archive12d/customisation.hpp"
#include "katana/cad/customisation_record.hpp"

namespace a12 = katana::archive12d;

namespace {

bool builtInCustomisationPresent()
{
    const a12::Customisation& built = a12::builtinCustomisation();
    return !built.files.empty() || !built.errors.empty();
}

} // namespace

// Each file rename names a built-in file, in the order they load.
TEST(BuiltInRenames, TheFileRenamesNameTheBuiltInFilesInLoadOrder)
{
    if (!builtInCustomisationPresent()) {
        GTEST_SKIP() << "this build has no customisation compiled in";
    }
    std::vector<std::string> files;
    for (const a12::LoadedFile& file : a12::builtinCustomisation().files) {
        files.push_back(file.path.filename().string());
    }
    std::vector<std::string> renamed;
    for (const katana::cad::RenamedSource& rename : katana::cad::builtinRenames()) {
        renamed.emplace_back(rename.now);
    }
    EXPECT_EQ(renamed, files);
}

// Each definition rename gives a name the built-in library defines, and no
// built-in definition is itself known to the table as an earlier name, so the
// retry after a missed lookup can only ever land on a definition that is there.
TEST(BuiltInRenames, EveryRenamedDefinitionIsInTheBuiltInLibraryUnderTheNameItHasNow)
{
    if (!builtInCustomisationPresent()) {
        GTEST_SKIP() << "this build has no customisation compiled in";
    }
    const katana::entity::StyleLibrary& library = a12::builtinCustomisation().library;
    for (const katana::cad::RenamedDefinition& rename : katana::cad::builtinDefinitionRenames()) {
        EXPECT_NE(library.find(rename.now), nullptr) << rename.now;
    }
    std::size_t known = 0;
    library.forEach([&](const katana::entity::LineStyle& style) {
        known += katana::cad::definitionNameNow(style.name).empty() ? 0 : 1;
    });
    EXPECT_EQ(known, 0u);
}
