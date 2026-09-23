// DocumentWatcher: a manager dialog's one way of hearing from the Document -
// one delivery per event-loop turn, saying which of the model, the library,
// the survey map, the selection and the current layer/style moved.

#include <gtest/gtest.h>

#include <filesystem>
#include <memory>
#include <vector>

#include <QCoreApplication>

#include "customisation/document_watcher.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"

using katana::cad::Document;
using katana::qt::DocumentChanges;
using katana::qt::DocumentWatcher;

namespace {

// What a watcher delivered, in order.
struct Deliveries {
    std::vector<DocumentChanges> calls{};
    DocumentWatcher::Callback callback()
    {
        return [this](const DocumentChanges& changes) { calls.push_back(changes); };
    }
};

void runEventLoopTurn() { QCoreApplication::processEvents(); }

katana::entity::Layer layerNamed(const char* name)
{
    katana::entity::Layer layer;
    layer.name = name;
    return layer;
}

} // namespace

TEST(DocumentWatcher, ThreeNotificationsInOneTurnAreDeliveredOnceAndOnlyFromTheEventLoop)
{
    Document document;
    Deliveries seen;
    DocumentWatcher watcher(document, seen.callback());

    ASSERT_TRUE(document.execute(katana::commands::createLayer(layerNamed("a"))).ok());
    ASSERT_TRUE(document.execute(katana::commands::createLayer(layerNamed("b"))).ok());
    document.notifySelectionChanged();
    // Nothing yet: delivering inside the notification is what a dialog must
    // never do (docs/cad.md).
    EXPECT_TRUE(seen.calls.empty());
    EXPECT_TRUE(watcher.pending());

    runEventLoopTurn();
    ASSERT_EQ(seen.calls.size(), 1U);
    EXPECT_FALSE(watcher.pending());

    // And nothing more on the next turn: the three were one.
    runEventLoopTurn();
    EXPECT_EQ(seen.calls.size(), 1U);
}

TEST(DocumentWatcher, EachFlagIsSetByItsOwnChangeAndByNothingElse)
{
    Document document;
    Deliveries seen;
    DocumentWatcher watcher(document, seen.callback());

    // A command: the model, and nothing else.
    ASSERT_TRUE(document.execute(katana::commands::createLayer(layerNamed("walls"))).ok());
    runEventLoopTurn();
    ASSERT_EQ(seen.calls.size(), 1U);
    EXPECT_EQ(seen.calls.back(), (DocumentChanges{.model = true}));

    // A library: its generation moves; the model does not.
    document.setStyleLibrary(katana::entity::StyleLibrary{});
    runEventLoopTurn();
    ASSERT_EQ(seen.calls.size(), 2U);
    EXPECT_EQ(seen.calls.back(), (DocumentChanges{.library = true}));

    document.setSurveyMap(katana::entity::SurveyMap{});
    runEventLoopTurn();
    ASSERT_EQ(seen.calls.size(), 3U);
    EXPECT_EQ(seen.calls.back(), (DocumentChanges{.surveyMap = true}));

    // A selection: its own flag, not the model's.
    ASSERT_TRUE(document.execute(katana::commands::createPoint({1.0, 2.0})).ok());
    runEventLoopTurn();
    ASSERT_EQ(seen.calls.size(), 4U);
    EXPECT_EQ(seen.calls.back(), (DocumentChanges{.model = true}));
    document.selection().set(document.lastCreatedEntities());
    document.notifySelectionChanged();
    runEventLoopTurn();
    ASSERT_EQ(seen.calls.size(), 5U);
    EXPECT_EQ(seen.calls.back(), (DocumentChanges{.selection = true}));

    // The current layer.
    ASSERT_TRUE(document.setCurrentLayer("walls").ok());
    runEventLoopTurn();
    ASSERT_EQ(seen.calls.size(), 6U);
    EXPECT_EQ(seen.calls.back(), (DocumentChanges{.current = true}));

    // A notification that changed none of them is still delivered, flagless.
    document.notifySelectionChanged();
    runEventLoopTurn();
    ASSERT_EQ(seen.calls.size(), 7U);
    EXPECT_FALSE(seen.calls.back().any());
}

TEST(DocumentWatcher, AnUndoFollowedByANewCommandInOneTurnIsAModelChange)
{
    // The case a per-turn comparison misses. Worked by hand: after creating
    // layer "a" the history is (undo 1, redo 0) and there are two layers
    // ("0" and "a"). Undo gives (0, 1) and one layer; creating "b" gives
    // (1, 0) and two layers again - every count where it started, and a
    // different drawing ("b", not "a").
    Document document;
    ASSERT_TRUE(document.execute(katana::commands::createLayer(layerNamed("a"))).ok());
    Deliveries seen;
    DocumentWatcher watcher(document, seen.callback());

    ASSERT_TRUE(document.undo().ok());
    ASSERT_TRUE(document.execute(katana::commands::createLayer(layerNamed("b"))).ok());
    ASSERT_EQ(document.history().undoCount(), 1U);
    ASSERT_EQ(document.history().redoCount(), 0U);
    runEventLoopTurn();

    ASSERT_EQ(seen.calls.size(), 1U);
    EXPECT_TRUE(seen.calls.back().model);
}

TEST(DocumentWatcher, ReopeningAnUnchangedProjectFromAFreshDrawingIsAModelChange)
{
    // The fingerprint's blind spot, gone with Document::modelRevision: an
    // empty drawing saved as a project and reopened has no history before
    // or after, the same size of every table and the same project directory
    // - and it is still a new drawing, whose views must reload.
    const std::filesystem::path directory =
        std::filesystem::temp_directory_path() / "katana-qt-watcher-reopen" / "same.katana";
    std::filesystem::remove_all(directory.parent_path());
    Deliveries seen;
    {
        // Its own scope: Windows will not delete the project's open database.
        Document document;
        ASSERT_TRUE(document.saveAs(directory).ok());
        DocumentWatcher watcher(document, seen.callback());

        const auto opened = document.open(directory);
        ASSERT_TRUE(opened.ok()) << opened.error().describe();
        runEventLoopTurn();
    }
    ASSERT_EQ(seen.calls.size(), 1U);
    EXPECT_TRUE(seen.calls.back().model);
    std::filesystem::remove_all(directory.parent_path());
}

TEST(DocumentWatcher, ADocumentDestroyedWithADeliveryQueuedIsNeverReadAndNothingIsDelivered)
{
    Deliveries seen;
    std::unique_ptr<DocumentWatcher> watcher;
    {
        Document document;
        watcher = std::make_unique<DocumentWatcher>(document, seen.callback());
        EXPECT_TRUE(watcher->documentAlive());
        ASSERT_TRUE(document.execute(katana::commands::createLayer(layerNamed("a"))).ok());
        ASSERT_TRUE(watcher->pending());
    }
    EXPECT_FALSE(watcher->documentAlive());
    runEventLoopTurn(); // the queued delivery runs here, and must not touch the Document
    EXPECT_TRUE(seen.calls.empty());
    watcher.reset(); // and the watcher then goes quietly
}

TEST(DocumentWatcher, AWatcherDestroyedWithADeliveryQueuedTakesTheDeliveryWithIt)
{
    Document document;
    Deliveries seen;
    auto watcher = std::make_unique<DocumentWatcher>(document, seen.callback());
    ASSERT_TRUE(document.execute(katana::commands::createLayer(layerNamed("a"))).ok());
    watcher.reset();
    runEventLoopTurn();
    EXPECT_TRUE(seen.calls.empty());
    // And its registration went with it: the Document notifies nobody.
    ASSERT_TRUE(document.execute(katana::commands::createLayer(layerNamed("b"))).ok());
    runEventLoopTurn();
    EXPECT_TRUE(seen.calls.empty());
}
