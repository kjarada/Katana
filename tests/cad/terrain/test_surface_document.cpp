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
