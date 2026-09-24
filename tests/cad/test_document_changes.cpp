// What a Document notification says changed (DocumentChange).
//
// A listener told only "something changed" has to rebuild everything it
// holds, and the 3D view did exactly that on every selection click: 217-768 ms
// on a real TIN archive to redraw a highlight. These tests pin down the
// payload a listener can skip work by: which parts a notification names, that
// a click names the selection ALONE, that an edit names its entities, and
// that the listeners written before the payload existed still run for
// everything.

#include <gtest/gtest.h>

#include <filesystem>
#include <string>
#include <system_error>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"

namespace cmd = katana::commands;
namespace fs = std::filesystem;
using katana::cad::Document;
using katana::cad::DocumentChange;
using katana::entity::ChangeEvent;
using katana::entity::ChangeKind;
using katana::entity::EntityId;
using katana::entity::Layer;
using katana::geometry::Point2;

namespace {

// Everything one ChangeListener was told, copied: the span in a change is
// valid only during the call.
struct Recorded {
    std::uint32_t parts = 0;
    std::vector<ChangeEvent> entities;
};

struct Recorder {
    std::vector<Recorded> calls;
    Document::ListenerHandle handle;

    explicit Recorder(Document& document)
        : handle(document.addListener([this](const DocumentChange& change) {
              calls.push_back({change.parts, {change.entities.begin(), change.entities.end()}});
          }))
    {
    }

    [[nodiscard]] const Recorded& last() const { return calls.back(); }
};

EntityId mustCreatePoint(Document& document, Point2 at)
{
    const auto status = document.execute(cmd::createPoint(at));
    EXPECT_TRUE(status.ok()) << status.error().describe();
    const auto created = document.lastCreatedEntities();
    return created.empty() ? katana::entity::kInvalidEntityId : created.front();
}

} // namespace

TEST(DocumentChanges, ASelectionChangeIsReportedAsTheSelectionAloneAndChangesNoDrawing)
{
    Document document;
    const EntityId id = mustCreatePoint(document, Point2(0.0, 0.0));
    Recorder recorder(document);

    document.selection().set({id});
    document.notifySelectionChanged();

    ASSERT_EQ(recorder.calls.size(), 1u);
    const DocumentChange change{.parts = recorder.last().parts};
    EXPECT_EQ(change.parts, std::uint32_t{DocumentChange::Selection});
    EXPECT_TRUE(change.selectionOnly());
    EXPECT_FALSE(change.changesDrawing()) << "a click must not make a view rebuild its drawing";
    EXPECT_TRUE(recorder.last().entities.empty());
}

TEST(DocumentChanges, CreatingAnEntityReportsItsIdAndTheHistoryButNoTableOrSelection)
{
    Document document;
    Recorder recorder(document);

    const EntityId id = mustCreatePoint(document, Point2(1.0, 2.0));

    ASSERT_EQ(recorder.calls.size(), 1u);
    EXPECT_EQ(recorder.last().parts,
              std::uint32_t{DocumentChange::Entities | DocumentChange::History});
    ASSERT_EQ(recorder.last().entities.size(), 1u);
    EXPECT_EQ(recorder.last().entities[0].kind, ChangeKind::EntityAdded);
    EXPECT_EQ(recorder.last().entities[0].id, id);
}

TEST(DocumentChanges, DeletingASelectedEntityReportsTheEntityAndTheSelectionItLeft)
{
    Document document;
    const EntityId kept = mustCreatePoint(document, Point2(0.0, 0.0));
    const EntityId deleted = mustCreatePoint(document, Point2(5.0, 0.0));
    document.selection().set({kept, deleted});
    Recorder recorder(document);

    ASSERT_TRUE(document.execute(cmd::deleteEntities({deleted})).ok());

    ASSERT_EQ(recorder.calls.size(), 1u);
    EXPECT_EQ(recorder.last().parts,
              std::uint32_t{DocumentChange::Entities | DocumentChange::Selection |
                            DocumentChange::History});
    ASSERT_EQ(recorder.last().entities.size(), 1u);
    EXPECT_EQ(recorder.last().entities[0].kind, ChangeKind::EntityRemoved);
    EXPECT_EQ(recorder.last().entities[0].id, deleted);
    EXPECT_TRUE(document.selection().contains(kept));
    EXPECT_FALSE(document.selection().contains(deleted));
}

TEST(DocumentChanges, DeletingAnUnselectedEntityDoesNotReportTheSelection)
{
    Document document;
    const EntityId selected = mustCreatePoint(document, Point2(0.0, 0.0));
    const EntityId other = mustCreatePoint(document, Point2(5.0, 0.0));
    document.selection().set({selected});
    Recorder recorder(document);

    ASSERT_TRUE(document.execute(cmd::deleteEntities({other})).ok());

    ASSERT_EQ(recorder.calls.size(), 1u);
    EXPECT_FALSE(DocumentChange{.parts = recorder.last().parts}.has(DocumentChange::Selection));
}

TEST(DocumentChanges, ALayerCommandReportsTheLayersAndNoEntities)
{
    Document document;
    Recorder recorder(document);

    ASSERT_TRUE(document.execute(cmd::createLayer(Layer{"Survey"})).ok());

    ASSERT_EQ(recorder.calls.size(), 1u);
    EXPECT_EQ(recorder.last().parts,
              std::uint32_t{DocumentChange::Layers | DocumentChange::History});
    EXPECT_TRUE(recorder.last().entities.empty());
    EXPECT_TRUE(DocumentChange{.parts = recorder.last().parts}.changesDrawing())
        << "a layer's colour is part of how the drawing looks";
}

TEST(DocumentChanges, UndoingTheCurrentLayersCreationReportsTheLayersAndTheCurrentLayer)
{
    Document document;
    ASSERT_TRUE(document.execute(cmd::createLayer(Layer{"Survey"})).ok());
    ASSERT_TRUE(document.setCurrentLayer("Survey").ok());
    Recorder recorder(document);

    ASSERT_TRUE(document.undo().ok());

    ASSERT_EQ(recorder.calls.size(), 1u);
    EXPECT_EQ(recorder.last().parts,
              std::uint32_t{DocumentChange::Layers | DocumentChange::CurrentAttributes |
                            DocumentChange::History});
    EXPECT_EQ(document.currentLayer(), "0");
}

TEST(DocumentChanges, EachTableCommandReportsItsOwnTableOnly)
{
    Document document;
    katana::cad::CommandInterpreter interpreter(document);
    Recorder recorder(document);

    ASSERT_TRUE(interpreter.run("STYLE NEW Kerb").ok());
    ASSERT_EQ(recorder.calls.size(), 1u);
    EXPECT_EQ(recorder.last().parts,
              std::uint32_t{DocumentChange::Styles | DocumentChange::History});

    // Three PIs, no curves: the alignment table alone.
    ASSERT_TRUE(interpreter.run("ALIGN NEW road 0,0 100,0 100,100").ok());
    ASSERT_EQ(recorder.calls.size(), 2u);
    EXPECT_EQ(recorder.last().parts,
              std::uint32_t{DocumentChange::Alignments | DocumentChange::History});
    EXPECT_TRUE(recorder.last().entities.empty());
}

TEST(DocumentChanges, TheCurrentLayerAndStyleAreReportedAsTheCurrentAttributesAlone)
{
    Document document;
    ASSERT_TRUE(document.execute(cmd::createLayer(Layer{"Survey"})).ok());
    Recorder recorder(document);

    ASSERT_TRUE(document.setCurrentLayer("Survey").ok());

    ASSERT_EQ(recorder.calls.size(), 1u);
    EXPECT_EQ(recorder.last().parts, std::uint32_t{DocumentChange::CurrentAttributes});
    EXPECT_FALSE(DocumentChange{.parts = recorder.last().parts}.changesDrawing());
}

TEST(DocumentChanges, MetadataLibraryAndSurveyMapEachReportTheirOwnPart)
{
    Document document;
    Recorder recorder(document);

    auto metadata = document.metadata();
    metadata.description = "a corridor survey";
    document.setMetadata(metadata);
    ASSERT_EQ(recorder.calls.size(), 1u);
    EXPECT_EQ(recorder.last().parts, std::uint32_t{DocumentChange::Metadata});
    EXPECT_FALSE(DocumentChange{.parts = recorder.last().parts}.changesDrawing());

    document.setStyleLibrary({});
    ASSERT_EQ(recorder.calls.size(), 2u);
    EXPECT_EQ(recorder.last().parts, std::uint32_t{DocumentChange::StyleLibrary});
    EXPECT_TRUE(DocumentChange{.parts = recorder.last().parts}.changesDrawing())
        << "library linestyles and symbols are what styled entities are drawn with";

    document.setSurveyMap({});
    ASSERT_EQ(recorder.calls.size(), 3u);
    EXPECT_EQ(recorder.last().parts, std::uint32_t{DocumentChange::SurveyMap});
}

TEST(DocumentChanges, ANewDocumentReportsEveryDrawingPartSoAnEntityListenerRebuilds)
{
    Document document;
    const EntityId id = mustCreatePoint(document, Point2(0.0, 0.0));
    document.selection().set({id});
    Recorder recorder(document);

    document.newDocument();

    ASSERT_EQ(recorder.calls.size(), 1u);
    const DocumentChange change{.parts = recorder.last().parts};
    EXPECT_TRUE(change.has(DocumentChange::Replaced));
    // A listener that knows nothing of Replaced still sees what it tests for.
    EXPECT_TRUE(change.has(DocumentChange::Entities));
    EXPECT_TRUE(change.has(DocumentChange::Selection));
    EXPECT_TRUE(change.has(DocumentChange::Layers));
    EXPECT_TRUE(change.has(DocumentChange::Alignments));
    EXPECT_TRUE(change.has(DocumentChange::Metadata));
    EXPECT_TRUE(change.has(DocumentChange::History));
    // The customisation is kept across a new drawing, so it is not named.
    EXPECT_FALSE(change.has(DocumentChange::StyleLibrary));
    EXPECT_FALSE(change.has(DocumentChange::SurveyMap));
    EXPECT_FALSE(change.selectionOnly());
    EXPECT_TRUE(recorder.last().entities.empty())
        << "a replaced drawing does not list its entities";
}

TEST(DocumentChanges, AnOpenReportsAReplacedDrawingAndTheFirstCommandAfterItNoTables)
{
    const fs::path directory =
        fs::temp_directory_path() / "katana-cad-tests-document-changes" / "opened.katana";
    fs::remove_all(directory.parent_path());
    {
        Document saved;
        ASSERT_TRUE(saved.execute(cmd::createLayer(Layer{"Survey"})).ok());
        (void)mustCreatePoint(saved, Point2(3.0, 4.0));
        ASSERT_TRUE(saved.saveAs(directory).ok());
    }

    // Scoped: the open project holds its database file until the Document
    // goes, and Windows will not remove a file that is open.
    {
        Document document;
        Recorder recorder(document);
        const auto opened = document.open(directory);
        ASSERT_TRUE(opened.ok()) << opened.error().describe();
        ASSERT_EQ(recorder.calls.size(), 1u);
        EXPECT_TRUE(DocumentChange{.parts = recorder.last().parts}.has(DocumentChange::Replaced));

        // The tables loaded by the open are not a change the next command made.
        (void)mustCreatePoint(document, Point2(5.0, 6.0));
        ASSERT_EQ(recorder.calls.size(), 2u);
        EXPECT_EQ(recorder.last().parts,
                  std::uint32_t{DocumentChange::Entities | DocumentChange::History});

        ASSERT_TRUE(document.save().ok());
        ASSERT_EQ(recorder.calls.size(), 3u);
        EXPECT_EQ(recorder.last().parts, std::uint32_t{DocumentChange::Saved});
    }

    std::error_code ignored;
    fs::remove_all(directory.parent_path(), ignored);
}

TEST(DocumentChanges, ARefusedCommandNotifiesNobody)
{
    Document document;
    Recorder recorder(document);

    EXPECT_FALSE(document.execute(cmd::createCircle(Point2(0.0, 0.0), -1.0)).ok());

    EXPECT_TRUE(recorder.calls.empty());
}

TEST(DocumentChanges, PlainAndTypedListenersAreBothCalledInTheOrderTheyWereAdded)
{
    // The listeners written before the payload existed take no argument. They
    // must still run for every notification - a selection click included -
    // and in their place in the order.
    Document document;
    std::vector<std::string> order;
    const auto first = document.addListener([&] { order.emplace_back("plain"); });
    const auto second = document.addListener([&](const DocumentChange& change) {
        order.emplace_back("typed " + std::to_string(change.parts));
    });

    (void)mustCreatePoint(document, Point2(0.0, 0.0));
    document.notifySelectionChanged();

    // Entities | History = 1 + 1024 = 1025; Selection = 2.
    const std::vector<std::string> expected{"plain", "typed 1025", "plain", "typed 2"};
    EXPECT_EQ(order, expected);
}
