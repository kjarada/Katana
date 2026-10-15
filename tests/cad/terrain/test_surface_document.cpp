// The drawing's surfaces and their display styles in the Document
// (document.hpp, "surfaces"; docs/terrain.md, "Surfaces in the drawing"):
// one store, every change one undo step that puts back the exact store, a
// generation that only rises, cleared by New, saved and opened with the
// project.

#include <gtest/gtest.h>

#include <cstdint>
#include <filesystem>
#include <memory>
#include <span>
#include <string>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/terrain/surface_style.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/storage/sqlite_database.hpp"
#include "katana/terrain/tin_codec.hpp"
#include "surface_test_support.hpp"
#include "terrain/surface_document_io.hpp"

namespace fs = std::filesystem;
using katana::cad::Document;
using katana::cad::DocumentChange;
using katana::cad::SurfaceStyleSet;
using katana::cad::builtInSurfaceStyles;
using katana::cad::surface_testing::ScratchDirectory;
using katana::cad::surface_testing::tiltedSquare;
using katana::cad::surface_testing::with;
using katana::core::ErrorCode;
using katana::core::Status;
using katana::terrain::SurfaceStore;

namespace {

struct Recorder {
    std::vector<std::uint32_t> parts;
    Document::ListenerHandle handle;

    explicit Recorder(Document& document)
        : handle(document.addListener(
              [this](const DocumentChange& change) { parts.push_back(change.parts); }))
    {
    }
    [[nodiscard]] DocumentChange last() const { return DocumentChange{.parts = parts.back()}; }
};

// Every surface's name, in order.
std::vector<std::string> names(const Document& document)
{
    std::vector<std::string> out;
    for (const auto& named : document.surfaces().all()) {
        out.push_back(named.name);
    }
    return out;
}

// A copy of the built-ins with one style of the test's own.
SurfaceStyleSet withStyle(const std::string& name)
{
    SurfaceStyleSet set = builtInSurfaceStyles();
    katana::cad::SurfaceDisplayStyle style;
    style.name = name;
    style.showTriangles = true;
    style.contours.interval = 0.25;
    set.styles.push_back(style);
    return set;
}

} // namespace

TEST(SurfaceDocument, ANewDocumentHasNoSurfacesAndTheBuiltInStyles)
{
    const Document document;
    EXPECT_TRUE(document.surfaces().empty());
    EXPECT_TRUE(document.surfaceStyles() == builtInSurfaceStyles());
    EXPECT_TRUE(document.surfaceStylesStatus().ok());
}

TEST(SurfaceDocument, SettingTheSurfacesIsOneUndoStepAndUndoPutsBackTheExactStore)
{
    Document document;
    const auto ground = tiltedSquare(100.0, 1.0);
    ASSERT_TRUE(document.setSurfaces(with({}, "ground", ground), "SURFACE FROM").ok());
    EXPECT_EQ(document.history().undoCount(), 1u);
    EXPECT_EQ(document.history().undoName(), "SURFACE FROM");
    EXPECT_TRUE(document.isModified());
    const SurfaceStore one = document.surfaces();

    ASSERT_TRUE(document.setSurfaces(with(one, "design", tiltedSquare(101.0)), "SURFACE COPY").ok());
    EXPECT_EQ(names(document), (std::vector<std::string>{"ground", "design"}));

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.surfaces().all(), one.all()) << "the very surface, by its pointer";
    EXPECT_EQ(document.surfaces().find("ground")->surface, ground);
    ASSERT_TRUE(document.undo().ok());
    EXPECT_TRUE(document.surfaces().empty());
    ASSERT_TRUE(document.redo().ok());
    EXPECT_EQ(document.surfaces().all(), one.all());
}

TEST(SurfaceDocument, TheGenerationRisesOnEveryChangeUndoAndRedoIncludedAndNeverFalls)
{
    Document document;
    std::vector<std::uint64_t> seen{document.surfacesGeneration()};
    const auto step = [&] { seen.push_back(document.surfacesGeneration()); };
    ASSERT_TRUE(document.setSurfaces(with({}, "ground", tiltedSquare(0.0))).ok());
    step();
    SurfaceStore hidden = document.surfaces();
    ASSERT_TRUE(hidden.setVisible("ground", false).ok());
    ASSERT_TRUE(document.setSurfaces(hidden).ok());
    step();
    ASSERT_TRUE(document.undo().ok());
    step();
    ASSERT_TRUE(document.undo().ok());
    step();
    ASSERT_TRUE(document.redo().ok());
    step();
    document.newDocument();
    step();
    for (std::size_t i = 1; i < seen.size(); ++i) {
        EXPECT_GT(seen[i], seen[i - 1]) << "change " << i;
    }
}

TEST(SurfaceDocument, AStoreEqualToTheDrawingsRecordsNothing)
{
    Document document;
    ASSERT_TRUE(document.setSurfaces(with({}, "ground", tiltedSquare(0.0))).ok());
    const std::uint64_t generation = document.surfacesGeneration();
    // A copy changed and changed back is still the same surfaces, though its
    // revision has moved twice.
    SurfaceStore same = document.surfaces();
    ASSERT_TRUE(same.setVisible("ground", false).ok());
    ASSERT_TRUE(same.setVisible("ground", true).ok());
    ASSERT_TRUE(document.setSurfaces(same).ok());
    EXPECT_EQ(document.history().undoCount(), 1u);
    EXPECT_EQ(document.surfacesGeneration(), generation);
    auto command = document.surfacesCommand(document.surfaces());
    ASSERT_TRUE(command.ok());
    EXPECT_TRUE(*command == nullptr);
}

TEST(SurfaceDocument, ASurfaceInAStyleTheDrawingDoesNotHaveIsRefusedAndNothingChanges)
{
    Document document;
    const auto refused = document.setSurfaces(with({}, "ground", tiltedSquare(0.0), "No Such"));
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(document.surfaces().empty());
    EXPECT_EQ(document.history().undoCount(), 0u);
    // Named in any case, as a line names it.
    EXPECT_TRUE(document.setSurfaces(with({}, "ground", tiltedSquare(0.0), "contours")).ok());
}

TEST(SurfaceDocument, AStyleASurfaceIsDrawnInCannotBeTakenAway)
{
    Document document;
    ASSERT_TRUE(document.setSurfaceStyles(withStyle("Fine")).ok());
    ASSERT_TRUE(document.setSurfaces(with({}, "ground", tiltedSquare(0.0), "Fine")).ok());
    const auto refused = document.setSurfaceStyles(builtInSurfaceStyles());
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(refused.error().message.find("ground"), std::string::npos)
        << refused.error().describe();
    EXPECT_NE(document.surfaceStyles().find("Fine"), nullptr);
}

TEST(SurfaceDocument, AStyleSetThatCannotBeDrawnIsRefused)
{
    Document document;
    SurfaceStyleSet broken = builtInSurfaceStyles();
    broken.styles[0].contours.interval = 0.0;
    EXPECT_EQ(document.setSurfaceStyles(broken).error().code, ErrorCode::InvalidArgument);
    SurfaceStyleSet noDefault = builtInSurfaceStyles();
    noDefault.defaultStyle = "Nothing";
    EXPECT_EQ(document.setSurfaceStyles(noDefault).error().code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(document.surfaceStyles() == builtInSurfaceStyles());
    EXPECT_EQ(document.history().undoCount(), 0u);
}

TEST(SurfaceDocument, SettingTheStylesIsOneUndoStepThatPutsBackTheExactSet)
{
    Document document;
    const std::uint64_t generation = document.surfaceStylesGeneration();
    ASSERT_TRUE(document.setSurfaceStyles(withStyle("Fine"), "SURFACESTYLE NEW").ok());
    EXPECT_EQ(document.history().undoName(), "SURFACESTYLE NEW");
    EXPECT_GT(document.surfaceStylesGeneration(), generation);
    ASSERT_TRUE(document.undo().ok());
    EXPECT_TRUE(document.surfaceStyles() == builtInSurfaceStyles());
    ASSERT_TRUE(document.redo().ok());
    EXPECT_TRUE(document.surfaceStyles() == withStyle("Fine"));
    // The same set again is no step.
    ASSERT_TRUE(document.setSurfaceStyles(withStyle("Fine")).ok());
    EXPECT_EQ(document.history().undoCount(), 1u);
}

TEST(SurfaceDocument, ARenamedStyleAndTheSurfacesDrawnInItMoveInOneStep)
{
    Document document;
    ASSERT_TRUE(document.setSurfaceStyles(withStyle("Fine")).ok());
    ASSERT_TRUE(document.setSurfaces(with({}, "ground", tiltedSquare(0.0), "Fine")).ok());
    SurfaceStyleSet renamed = withStyle("Finer");
    SurfaceStore moved = document.surfaces();
    ASSERT_TRUE(moved.setStyle("ground", "Finer").ok());
    const std::size_t steps = document.history().undoCount();
    ASSERT_TRUE(document.setSurfacesAndStyles(moved, renamed, "SURFACESTYLE RENAME").ok());
    EXPECT_EQ(document.history().undoCount(), steps + 1);
    EXPECT_EQ(document.surfaces().find("ground")->style, "Finer");
    EXPECT_NE(document.surfaceStyles().find("Finer"), nullptr);
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.surfaces().find("ground")->style, "Fine");
    EXPECT_NE(document.surfaceStyles().find("Fine"), nullptr);
}

TEST(SurfaceDocument, EveryChangeIsNotifiedAsTheDrawingsSurfacesOrStyles)
{
    Document document;
    Recorder recorder(document);
    ASSERT_TRUE(document.setSurfaces(with({}, "ground", tiltedSquare(0.0))).ok());
    ASSERT_EQ(recorder.parts.size(), 1u);
    EXPECT_EQ(recorder.parts.back(),
              std::uint32_t{DocumentChange::Surfaces | DocumentChange::History});
    EXPECT_TRUE(recorder.last().changesDrawing());
    ASSERT_TRUE(document.undo().ok());
    EXPECT_TRUE(recorder.last().has(DocumentChange::Surfaces));
    ASSERT_TRUE(document.redo().ok());
    EXPECT_TRUE(recorder.last().has(DocumentChange::Surfaces));
    ASSERT_TRUE(document.setSurfaceStyles(withStyle("Fine")).ok());
    EXPECT_EQ(recorder.parts.back(),
              std::uint32_t{DocumentChange::SurfaceStyles | DocumentChange::History});
    document.newDocument();
    EXPECT_TRUE(recorder.last().has(DocumentChange::Replaced));
    EXPECT_TRUE(recorder.last().has(DocumentChange::Surfaces));
    EXPECT_TRUE(recorder.last().has(DocumentChange::SurfaceStyles));
}

TEST(SurfaceDocument, NewClearsTheSurfacesAndPutsBackTheBuiltInStyles)
{
    Document document;
    ASSERT_TRUE(document.setSurfaceStyles(withStyle("Fine")).ok());
    ASSERT_TRUE(document.setSurfaces(with({}, "ground", tiltedSquare(0.0), "Fine")).ok());
    document.newDocument();
    EXPECT_TRUE(document.surfaces().empty());
    EXPECT_TRUE(document.surfaceStyles() == builtInSurfaceStyles());
    EXPECT_FALSE(document.history().canUndo());
}

TEST(SurfaceDocument, SurfacesAndStylesSurviveASaveAndAnOpenExactly)
{
    const ScratchDirectory scratch("save-open");
    const fs::path project = scratch.path / "surfaces.katana";
    const auto ground = tiltedSquare(100.125, 0.1);
    const auto design = tiltedSquare(98.0, -2.0, 5.0, 2.5, 2.5);
    {
        Document document;
        ASSERT_TRUE(document.setSurfaceStyles(withStyle("Fine")).ok());
        SurfaceStore store = with({}, "ground", ground, "", true, "point cloud site.laz");
        store = with(std::move(store), "Design Pad", design, "Fine", false, "drawing (4 entities)");
        ASSERT_TRUE(document.setSurfaces(store).ok());
        const Status saved = document.saveAs(project);
        ASSERT_TRUE(saved.ok()) << saved.error().describe();
        EXPECT_FALSE(document.isModified());
    }
    Document reopened;
    const Status opened = reopened.open(project);
    ASSERT_TRUE(opened.ok()) << opened.error().describe();
    EXPECT_FALSE(reopened.isModified());
    ASSERT_EQ(names(reopened), (std::vector<std::string>{"ground", "Design Pad"}));
    const auto& first = reopened.surfaces().all()[0];
    const auto& second = reopened.surfaces().all()[1];
    EXPECT_TRUE(*first.surface == *ground) << "every vertex, triangle and flag";
    EXPECT_TRUE(*second.surface == *design);
    EXPECT_EQ(first.source, "point cloud site.laz");
    EXPECT_EQ(first.style, "");
    EXPECT_TRUE(first.visible);
    EXPECT_EQ(second.style, "Fine");
    EXPECT_FALSE(second.visible);
    EXPECT_TRUE(reopened.surfaceStyles() == withStyle("Fine"));
    EXPECT_TRUE(reopened.surfaceStylesStatus().ok());
    // The styles' key is the Document's, written at the save: not a key the
    // metadata carries for someone else.
    EXPECT_FALSE(reopened.metadata().unknownKeys.contains("surface_styles"));
}

TEST(SurfaceDocument, OpeningAProjectWithNoneLeavesNoneAndTheBuiltIns)
{
    const ScratchDirectory scratch("open-none");
    const fs::path project = scratch.path / "plain.katana";
    {
        Document document;
        ASSERT_TRUE(document.saveAs(project).ok());
    }
    Document reopened;
    ASSERT_TRUE(reopened.setSurfaces(with({}, "left over", tiltedSquare(0.0))).ok());
    ASSERT_TRUE(reopened.open(project).ok());
    EXPECT_TRUE(reopened.surfaces().empty()) << "the open replaces the drawing's surfaces";
    EXPECT_TRUE(reopened.surfaceStyles() == builtInSurfaceStyles());
}

TEST(SurfaceDocument, StylesANewerKatanaWroteAreKeptAndNotOverwritten)
{
    const ScratchDirectory scratch("newer-styles");
    const fs::path project = scratch.path / "newer.katana";
    {
        Document document;
        ASSERT_TRUE(document.saveAs(project).ok());
    }
    const std::string newer = R"({"format":"katana-surface-styles","version":99,"styles":[]})";
    {
        auto raw = katana::storage::SqliteDatabase::open(project / "project.db");
        ASSERT_TRUE(raw.ok());
        ASSERT_TRUE(raw->execute("UPDATE metadata SET value = '" + newer +
                                 "' WHERE key = 'surface_styles'")
                        .ok());
    }
    Document document;
    ASSERT_TRUE(document.open(project).ok());
    EXPECT_EQ(document.surfaceStylesStatus().error().code, ErrorCode::Unsupported);
    EXPECT_TRUE(document.surfaceStyles() == builtInSurfaceStyles()) << "the built-ins stand in";
    const auto refused = document.setSurfaceStyles(withStyle("Fine"));
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::CommandRejected);
    // A surface is still made, in the styles there are.
    ASSERT_TRUE(document.setSurfaces(with({}, "ground", tiltedSquare(0.0), "Contours")).ok());
    ASSERT_TRUE(document.save().ok());
    auto raw = katana::storage::SqliteDatabase::open(project / "project.db");
    ASSERT_TRUE(raw.ok());
    auto select = raw->prepare("SELECT value FROM metadata WHERE key = 'surface_styles'");
    ASSERT_TRUE(select.ok());
    ASSERT_TRUE(select->step().valueOr(false));
    EXPECT_EQ(select->columnText(0), newer) << "written back as it was";
}

TEST(SurfaceDocument, ASurfaceTheProjectCannotReadRefusesTheOpenAndLeavesTheDrawing)
{
    const ScratchDirectory scratch("newer-surface");
    const fs::path project = scratch.path / "newer.katana";
    {
        Document document;
        ASSERT_TRUE(document.setSurfaces(with({}, "ground", tiltedSquare(0.0))).ok());
        ASSERT_TRUE(document.saveAs(project).ok());
    }
    {
        // The codec's version word (bytes 4 to 7) made one newer than this
        // build writes.
        std::string bytes = katana::terrain::encodeTin(*tiltedSquare(0.0));
        bytes[4] = static_cast<char>(katana::terrain::kTinCodecVersion + 1);
        auto raw = katana::storage::SqliteDatabase::open(project / "project.db");
        ASSERT_TRUE(raw.ok());
        auto update = raw->prepare("UPDATE surfaces SET data = ?1");
        ASSERT_TRUE(update.ok());
        ASSERT_TRUE(update
                        ->bind(1, std::span<const std::byte>(
                                      reinterpret_cast<const std::byte*>(bytes.data()),
                                      bytes.size()))
                        .ok());
        ASSERT_TRUE(update->run().ok());
    }
    Document document;
    ASSERT_TRUE(document.setSurfaces(with({}, "mine", tiltedSquare(5.0))).ok());
    const Status opened = document.open(project);
    ASSERT_FALSE(opened.ok());
    EXPECT_EQ(opened.error().code, ErrorCode::Unsupported);
    EXPECT_NE(opened.error().message.find("ground"), std::string::npos) << opened.error().describe();
    EXPECT_EQ(names(document), (std::vector<std::string>{"mine"})) << "the drawing as it was";
}

TEST(SurfaceDocument, AnImportOfEntitiesAndSurfacesIsOneStepThroughTheCommand)
{
    Document document;
    // Made through a const Document, as a tool holding one makes it: making
    // the command changes nothing.
    const Document& seen = document;
    auto command = seen.surfacesCommand(with({}, "ground", tiltedSquare(0.0)), "IMPORT");
    EXPECT_TRUE(document.surfaces().empty());
    ASSERT_TRUE(command.ok());
    ASSERT_TRUE(*command != nullptr);
    auto transaction = std::make_unique<katana::commands::Transaction>("IMPORT site.12da");
    transaction->add(katana::commands::createPoint(katana::geometry::Point2(1.0, 2.0)));
    transaction->add(std::move(*command));
    ASSERT_TRUE(document.execute(std::move(transaction)).ok());
    EXPECT_EQ(document.history().undoCount(), 1u);
    EXPECT_EQ(document.surfaces().all().size(), 1u);
    EXPECT_EQ(document.model().entities.size(), 1u);
    ASSERT_TRUE(document.undo().ok());
    EXPECT_TRUE(document.surfaces().empty());
    EXPECT_EQ(document.model().entities.size(), 0u);
}

// ---- text that is not UTF-8, and a style set that is not the form -------------------------

namespace {

// The text of the metadata row `surface_styles` as a saved project holds it.
std::string storedStyles(const fs::path& project)
{
    auto raw = katana::storage::SqliteDatabase::open(project / "project.db");
    EXPECT_TRUE(raw.ok());
    auto select = raw->prepare("SELECT value FROM metadata WHERE key = 'surface_styles'");
    EXPECT_TRUE(select.ok());
    EXPECT_TRUE(select->step().valueOr(false));
    return select->columnText(0);
}

} // namespace

TEST(SurfaceDocument, AStyleSetWhoseTextIsNotUtf8IsRefusedSoASaveNeverMeetsIt)
{
    // katana_cli on Windows hands the verbs the ANSI code page: "Pente" with a
    // degree sign arrives as 0xB0, and "cafe" with an accent as 0xE9. Neither is
    // UTF-8, and the set is written to the project as JSON, whose writer threw
    // out of Document::save (a process exit with the session's work lost).
    const ScratchDirectory scratch("ansi-styles");
    Document document;
    ASSERT_TRUE(document.setSurfaces(with({}, "ground", tiltedSquare(0.0))).ok());

    const Status badName = document.setSurfaceStyles(withStyle("Pente\xB0"));
    ASSERT_FALSE(badName.ok());
    EXPECT_EQ(badName.error().code, ErrorCode::InvalidArgument);

    SurfaceStyleSet badDescription = withStyle("Fine");
    badDescription.styles.back().description = "caf\xE9";
    const Status described = document.setSurfaceStyles(badDescription);
    ASSERT_FALSE(described.ok());
    EXPECT_EQ(described.error().code, ErrorCode::InvalidArgument);

    EXPECT_TRUE(document.surfaceStyles() == builtInSurfaceStyles()) << "nothing changed";
    EXPECT_EQ(document.history().undoCount(), 1u) << "and no step was recorded for it";
    // The save is therefore still a Status, not an exception.
    EXPECT_NO_THROW({
        const Status saved = document.saveAs(scratch.path / "p.katana");
        EXPECT_TRUE(saved.ok()) << saved.error().describe();
    });
}

TEST(SurfaceDocument, ASurfaceNameAnEarlierBuildStoredAsAnsiBytesOrWithAQuoteIsRepairedOnOpen)
{
    // Projects the build before this one could write: a name typed at a
    // katana_cli on Windows (0xE9 for e-acute) and one with a double quote that
    // an importer let through. Opening must not fail for them - the person could
    // not rename what a line cannot name - and must not leave a name the JSON
    // writers throw on.
    const ScratchDirectory scratch("repair-names");
    const fs::path project = scratch.path / "old.katana";
    {
        Document document;
        SurfaceStore store = with({}, "ground", tiltedSquare(0.0));
        store = with(std::move(store), "other", tiltedSquare(1.0));
        store = with(std::move(store), "clean", tiltedSquare(2.0));
        ASSERT_TRUE(document.setSurfaces(std::move(store)).ok());
        ASSERT_TRUE(document.saveAs(project).ok());
    }
    {
        auto raw = katana::storage::SqliteDatabase::open(project / "project.db");
        ASSERT_TRUE(raw.ok());
        // X'636166E9' is the four bytes of "caf" and 0xE9, stored as text exactly as
        // SQLite is given them (it does not check).
        ASSERT_TRUE(raw->execute("UPDATE surfaces SET name = CAST(X'636166E9' AS TEXT) "
                                 "WHERE name = 'ground'")
                        .ok());
        ASSERT_TRUE(raw->execute("UPDATE surfaces SET name = 'PAD \"A\"' WHERE name = 'other'").ok());
    }
    Document document;
    ASSERT_TRUE(document.open(project).ok());
    // 0xE9 read as Windows-1252 is U+00E9, UTF-8 C3 A9; the quote an apostrophe.
    EXPECT_EQ(names(document), (std::vector<std::string>{"caf\xC3\xA9", "PAD 'A'", "clean"}));
    // And the repaired project is stable: saved and opened again, nothing moves.
    ASSERT_TRUE(document.save().ok());
    Document again;
    ASSERT_TRUE(again.open(project).ok());
    EXPECT_EQ(names(again), names(document));
}

TEST(SurfaceDocument, ASurfaceSourceAndStyleAnEarlierBuildStoredAsAnsiBytesAreReadAsWindows1252)
{
    // Free text a katana_cli on Windows typed in the ANSI code page: a source
    // "raster caf" + 0xE9 (e-acute) and a style named "Pente" + 0xB0 (the
    // degree sign). Neither is UTF-8, and the project's own JSON replies and
    // its next save throw on such bytes, so open reads them as the
    // Windows-1252 they were: U+00E9 is C3 A9, U+00B0 is C2 B0. The name above
    // is repaired the same way; these two were read with no test that
    // mutated the conversion away.
    const ScratchDirectory scratch("repair-text");
    const fs::path project = scratch.path / "old.katana";
    {
        Document document;
        ASSERT_TRUE(document.setSurfaces(with({}, "ground", tiltedSquare(0.0))).ok());
        ASSERT_TRUE(document.saveAs(project).ok());
    }
    {
        auto raw = katana::storage::SqliteDatabase::open(project / "project.db");
        ASSERT_TRUE(raw.ok());
        // X'72617374657220636166E9' is "raster caf" and 0xE9; X'50656E7465B0' is
        // "Pente" and 0xB0, stored as text exactly as SQLite is given them.
        ASSERT_TRUE(raw->execute("UPDATE surfaces SET source = CAST(X'72617374657220636166E9' AS TEXT), "
                                 "style = CAST(X'50656E7465B0' AS TEXT) WHERE name = 'ground'")
                        .ok());
    }
    Document document;
    ASSERT_TRUE(document.open(project).ok());
    const auto* ground = document.surfaces().find("ground");
    ASSERT_NE(ground, nullptr);
    EXPECT_EQ(ground->source, "raster caf\xC3\xA9");
    EXPECT_EQ(ground->style, "Pente\xC2\xB0");
    // The project is stable: saved and opened again, nothing moves, nothing throws.
    ASSERT_TRUE(document.save().ok());
    Document again;
    ASSERT_TRUE(again.open(project).ok());
    EXPECT_EQ(again.surfaces().find("ground")->source, ground->source);
    EXPECT_EQ(again.surfaces().find("ground")->style, ground->style);
}

TEST(SurfaceDocument, TheSaveRefusesAStyleSetTheJsonWriterCouldNotCarryRatherThanRewriteItsText)
{
    // Document::setSurfaceStyles refuses such a set on the way in and a set
    // read on open passed the same check, so the guard in the save is not
    // reached through a Document; it is a function of the set
    // (surfaceStylesMetadataValue), handed the sets the door refuses.
    using katana::cad::detail::surfaceStylesMetadataValue;
    const auto badName = surfaceStylesMetadataValue(withStyle("Pente\xB0"));
    ASSERT_FALSE(badName.ok());
    EXPECT_EQ(badName.error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(badName.error().context, "field=name");

    SurfaceStyleSet described = withStyle("Fine");
    described.styles.back().description = "caf\xE9";
    const auto badDescription = surfaceStylesMetadataValue(described);
    ASSERT_FALSE(badDescription.ok());
    EXPECT_EQ(badDescription.error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(badDescription.error().context, "field=description");

    // A set the check refuses for another reason is refused as well: two with one name.
    SurfaceStyleSet twice = withStyle("Fine");
    twice.styles.push_back(twice.styles.back());
    EXPECT_FALSE(surfaceStylesMetadataValue(twice).ok());

    // And what it lets through: nothing for the built-ins, the set's JSON for another.
    const auto builtIn = surfaceStylesMetadataValue(builtInSurfaceStyles());
    ASSERT_TRUE(builtIn.ok());
    EXPECT_EQ(*builtIn, "");
    const auto custom = surfaceStylesMetadataValue(withStyle("Fine"));
    ASSERT_TRUE(custom.ok()) << custom.error().describe();
    EXPECT_EQ(*custom, katana::cad::surfaceStylesToJson(withStyle("Fine")));
    EXPECT_NE(custom->find("Fine"), std::string::npos);
}

TEST(SurfaceDocument, AStyleSetWhoseFormatIsNotAStringLeavesTheBuiltInsInPlaceAndTheTextKept)
{
    // A "format" of a number, null, an object or an array made nlohmann's value()
    // throw type_error.302 out of Document::open (a process exit in katana_cli
    // and in the window). The documented outcome is the built-ins standing in
    // with a ParseFailure status and the damaged text kept as it was.
    const ScratchDirectory scratch("format-type");
    const fs::path project = scratch.path / "damaged.katana";
    {
        Document document;
        ASSERT_TRUE(document.saveAs(project).ok());
    }
    for (const std::string damaged :
         {R"({"format":5,"version":1,"styles":[]})", R"({"format":null,"version":1,"styles":[]})",
          R"({"format":{},"version":1,"styles":[]})", R"({"format":["katana-surface-styles"],"version":1})",
          R"({"format":true,"version":1})", R"([1,2,3])", R"("katana-surface-styles")"}) {
        {
            auto raw = katana::storage::SqliteDatabase::open(project / "project.db");
            ASSERT_TRUE(raw.ok());
            ASSERT_TRUE(raw->execute("UPDATE metadata SET value = '" + damaged +
                                     "' WHERE key = 'surface_styles'")
                            .ok());
        }
        Document document;
        Status opened;
        EXPECT_NO_THROW(opened = document.open(project)) << damaged;
        ASSERT_TRUE(opened.ok()) << damaged << ": " << opened.error().describe();
        ASSERT_FALSE(document.surfaceStylesStatus().ok()) << damaged;
        EXPECT_EQ(document.surfaceStylesStatus().error().code, ErrorCode::ParseFailure) << damaged;
        EXPECT_TRUE(document.surfaceStyles() == builtInSurfaceStyles()) << damaged;
        ASSERT_TRUE(document.save().ok());
        EXPECT_EQ(storedStyles(project), damaged) << "a set that cannot be read is written back as it was";
    }
}
