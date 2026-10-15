// A view's Layers popup (src/katana_qt/view_layers_popup.*): it sets nothing itself.
// Every box and button builds the VIEWS line a person would type and hands it to the
// window's command runner, so a click is logged as a typed line is and is refused by
// the verb as a typed one is (docs/desktop.md, "Per-view layers"). What the lines are
// is held without a window; what the popup does with them in a workspace whose runner
// records every line and runs it on the drawing's interpreter, as the window's does.
//
// The layer trees are the ones the existing popup tests use; what a line leaves hidden
// is worked from LayerOverrides' contract (cad/layer_overrides.hpp), as the VIEWS tests do.

#include <gtest/gtest.h>

#include <QMainWindow>
#include <QToolButton>
#include <QTreeWidget>
#include <QTreeWidgetItem>

#include <set>
#include <string>
#include <vector>

#include "command_runner.hpp"
#include "dock_chrome.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"
#include "katana/interop/reference_data.hpp"
#include "view_layers_popup.hpp"
#include "view_workspace.hpp"
#include "widget_harness.hpp"

namespace {

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::cad::ViewId;
using katana::core::ErrorCode;
using katana::qt::DockChrome;
using katana::qt::VerbOutcome;
using katana::qt::viewLayersLine;
using katana::qt::ViewLayersPopup;
using katana::qt::viewReferencesLine;
using katana::qt::ViewWorkspace;
using katana::qt::test::processEvents;

// The window's arrangement: a workspace over a drawing whose command runner keeps every
// line it is handed and runs it on the drawing's interpreter - or refuses it, when asked.
struct PopupSite {
    Document document;
    QMainWindow window;
    ViewWorkspace* views = nullptr;
    DockChrome* chrome = nullptr;
    CommandInterpreter interpreter{document};
    QStringList lines;
    bool refuse = false;

    PopupSite()
    {
        views = new ViewWorkspace(document, &window);
        window.setCentralWidget(views);
        chrome = new DockChrome(window);
        views->setChrome(chrome);
        interpreter.setViewHost([this] { return &views->verbHost(); });
        views->setCommandRunner([this](const QString& line) {
            lines << line;
            VerbOutcome outcome;
            if (refuse) {
                outcome.error = "refused by the test";
                return outcome;
            }
            const auto reply = interpreter.run(line.toStdString());
            outcome.ok = reply.ok();
            if (reply.ok()) {
                outcome.reply = QString::fromStdString(*reply);
            } else {
                outcome.error = QString::fromStdString(reply.error().describe());
            }
            return outcome;
        });
        window.resize(800, 600);
        window.show();
        processEvents();
    }

    void addLayers(std::initializer_list<const char*> names)
    {
        for (const char* name : names) {
            katana::entity::Layer layer;
            layer.name = name;
            ASSERT_TRUE(document.execute(katana::commands::createLayer(layer)).ok()) << name;
        }
    }

    [[nodiscard]] ViewId plan() const { return views->viewSet().activeId(); }
    [[nodiscard]] const katana::cad::ViewState& state() const
    {
        return *views->viewSet().find(plan());
    }
};

using Hidden = std::set<std::string, std::less<>>;

} // namespace

// ---- the lines, held without a window -----------------------------------------------------

TEST(ViewLayersLines, APlainLayerIsAWordOfTheVerbsOwnForm)
{
    EXPECT_EQ(*viewLayersLine("HIDE", 2, {"design/road"}), "VIEWS HIDE 2 design/road");
    EXPECT_EQ(*viewLayersLine("SHOW", 12, {"asbuilt"}), "VIEWS SHOW 12 asbuilt");
    EXPECT_EQ(*viewLayersLine("ISOLATE", 3, {"design/surface"}), "VIEWS ISOLATE 3 design/surface");
    // Several: a word each, which the verb reads as a comma list of one name apiece.
    EXPECT_EQ(*viewLayersLine("HIDE", 1, {"a", "b", "c/d"}), "VIEWS HIDE 1 a b c/d");
}

TEST(ViewLayersLines, ANameWithABlankIsQuotedAsEveryDialogQuotesAWord)
{
    EXPECT_EQ(*viewLayersLine("HIDE", 2, {"as built"}), "VIEWS HIDE 2 \"as built\"");
    EXPECT_EQ(*viewLayersLine("HIDE", 2, {"as built", "road"}), "VIEWS HIDE 2 \"as built\" road");
}

TEST(ViewLayersLines, ANameWithACommaOrOneOfTheVerbsOwnWordsIsWrittenAfterLAYER)
{
    // A comma is the break between two layers in a comma list, so a name that holds one
    // goes after LAYER, which takes every word whole; and ALL, LAYER and REFERENCE are
    // the verb's own words in any case.
    EXPECT_EQ(*viewLayersLine("HIDE", 2, {"a,b"}), "VIEWS HIDE 2 LAYER a,b");
    EXPECT_EQ(*viewLayersLine("SHOW", 2, {"ALL"}), "VIEWS SHOW 2 LAYER ALL");
    EXPECT_EQ(*viewLayersLine("SHOW", 2, {"all"}), "VIEWS SHOW 2 LAYER all");
    EXPECT_EQ(*viewLayersLine("HIDE", 2, {"Layer"}), "VIEWS HIDE 2 LAYER Layer");
    EXPECT_EQ(*viewLayersLine("HIDE", 2, {"reference"}), "VIEWS HIDE 2 LAYER reference");
    EXPECT_EQ(*viewLayersLine("ISOLATE", 2, {"x,y"}), "VIEWS ISOLATE 2 LAYER x,y");
    // One awkward name puts the whole list after LAYER, where each word is still one name.
    EXPECT_EQ(*viewLayersLine("HIDE", 2, {"a", "b,c", "d e"}), "VIEWS HIDE 2 LAYER a b,c \"d e\"");
    // A name that merely contains such a word is plain.
    EXPECT_EQ(*viewLayersLine("HIDE", 2, {"allotment", "layers"}), "VIEWS HIDE 2 allotment layers");
}

TEST(ViewLayersLines, ANameNoLineCanCarryIsRefusedNamingIt)
{
    // The tokenizer has no escape inside a quoted word: a double quote or a line break in a
    // layer's name cannot be on a line at all, and is refused rather than cut short.
    for (const char* name : {"say \"hi\"", "two\nlines"}) {
        const auto line = viewLayersLine("HIDE", 2, {name});
        ASSERT_FALSE(line.ok()) << name;
        EXPECT_EQ(line.error().code, ErrorCode::InvalidArgument) << name;
        EXPECT_NE(line.error().message.find("layer name"), std::string::npos) << line.error().message;
    }
    // Refused whole: a good name beside it is not written alone.
    EXPECT_FALSE(viewLayersLine("HIDE", 2, {"road", "say \"hi\""}).ok());
}

TEST(ViewLayersLines, NoLayerAnActionThatIsNoneOfThreeOrTwoToIsolateIsRefused)
{
    EXPECT_EQ(viewLayersLine("HIDE", 2, {}).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(viewLayersLine("KEEP", 2, {"a"}).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(viewLayersLine("hide", 2, {"a"}).error().code, ErrorCode::InvalidArgument)
        << "the actions are the verb's words, written as the verb does";
    EXPECT_EQ(viewLayersLine("ISOLATE", 2, {"a", "b"}).error().code, ErrorCode::InvalidArgument);
}

TEST(ViewLayersLines, TheReferenceLayersAreNamedByTheirIds)
{
    EXPECT_EQ(viewReferencesLine("HIDE", 3, {7}), "VIEWS HIDE 3 REFERENCE 7");
    EXPECT_EQ(viewReferencesLine("SHOW", 3, {7, 12}), "VIEWS SHOW 3 REFERENCE 7 12");
}

// ---- the popup, in a workspace -------------------------------------------------------------

TEST(LayersPopup, TickingABoxRunsTheViewsLineAndTheLineSetsTheView)
{
    PopupSite site;
    site.addLayers({"BUILDING", "ROAD"});
    const ViewId plan = site.plan();
    const bool modified = site.document.isModified();
    const std::size_t undoable = site.document.history().undoCount();

    ViewLayersPopup* popup = site.views->showLayersPopup(plan);
    ASSERT_NE(popup, nullptr);
    QTreeWidgetItem* building = popup->itemFor("BUILDING");
    ASSERT_NE(building, nullptr);

    building->setCheckState(0, Qt::Unchecked);
    ASSERT_EQ(site.lines.size(), 1);
    EXPECT_EQ(site.lines.back(), QString("VIEWS HIDE %1 BUILDING").arg(plan));
    EXPECT_TRUE(site.state().layers.hides("BUILDING"));
    EXPECT_FALSE(site.state().layers.hides("ROAD"));
    EXPECT_EQ(building->checkState(0), Qt::Unchecked);

    building->setCheckState(0, Qt::Checked);
    ASSERT_EQ(site.lines.size(), 2);
    EXPECT_EQ(site.lines.back(), QString("VIEWS SHOW %1 BUILDING").arg(plan));
    EXPECT_FALSE(site.state().layers.hides("BUILDING"));

    // A view's own filter, still: a line that is no edit of the drawing.
    EXPECT_EQ(site.document.isModified(), modified);
    EXPECT_EQ(site.document.history().undoCount(), undoable);
    popup->close();
}

TEST(LayersPopup, ALineTheRunnerRefusesLeavesTheBoxWhereTheViewIs)
{
    PopupSite site;
    site.addLayers({"BUILDING"});
    ViewLayersPopup* popup = site.views->showLayersPopup(site.plan());
    ASSERT_NE(popup, nullptr);
    QTreeWidgetItem* building = popup->itemFor("BUILDING");
    ASSERT_NE(building, nullptr);

    site.refuse = true;
    building->setCheckState(0, Qt::Unchecked);
    EXPECT_EQ(site.lines.size(), 1) << "the line was handed over";
    EXPECT_FALSE(site.state().layers.hides("BUILDING")) << "and refused: the view is as it was";
    EXPECT_EQ(building->checkState(0), Qt::Checked) << "so the box says what the view does";
    popup->close();
}

TEST(LayersPopup, ALayerWhoseNameHoldsACommaIsHiddenAloneByALineThatCanSayIt)
{
    PopupSite site;
    site.addLayers({"a,b", "a", "b"});
    ViewLayersPopup* popup = site.views->showLayersPopup(site.plan());
    ASSERT_NE(popup, nullptr);
    QTreeWidgetItem* both = popup->itemFor("a,b");
    ASSERT_NE(both, nullptr);

    both->setCheckState(0, Qt::Unchecked);
    ASSERT_EQ(site.lines.size(), 1);
    EXPECT_EQ(site.lines.back(), QString("VIEWS HIDE %1 LAYER a,b").arg(site.plan()));
    // The layer called a,b, and neither a nor b, which a comma list would have hidden.
    EXPECT_EQ(site.state().layers.hidden(), Hidden{"a,b"});
    popup->close();
}

TEST(LayersPopup, ShowAllRunsOneLineWhenSomethingIsHiddenAndNoneWhenNothingIs)
{
    PopupSite site;
    site.addLayers({"BUILDING", "ROAD"});
    ViewLayersPopup* popup = site.views->showLayersPopup(site.plan());
    ASSERT_NE(popup, nullptr);
    auto* showAll = popup->findChild<QToolButton*>("ViewLayersShowAll");
    ASSERT_NE(showAll, nullptr);

    showAll->click();
    EXPECT_TRUE(site.lines.isEmpty()) << "nothing hidden, nothing to say";

    popup->itemFor("BUILDING")->setCheckState(0, Qt::Unchecked);
    popup->itemFor("ROAD")->setCheckState(0, Qt::Unchecked);
    ASSERT_EQ(site.lines.size(), 2);
    showAll->click();
    ASSERT_EQ(site.lines.size(), 3);
    EXPECT_EQ(site.lines.back(), QString("VIEWS SHOW %1 ALL").arg(site.plan()));
    EXPECT_TRUE(site.state().layers.empty());
    popup->close();
}

TEST(LayersPopup, HideOthersIsolatesTheLayerAndThenHidesAgainWhatThisViewHidBeneathIt)
{
    PopupSite site;
    site.addLayers({"ANNOT", "design/road", "design/surface/tin1", "survey/points"});
    ViewLayersPopup* popup = site.views->showLayersPopup(site.plan());
    ASSERT_NE(popup, nullptr);
    // The view hides tin1 already.
    popup->itemFor("design/surface/tin1")->setCheckState(0, Qt::Unchecked);
    ASSERT_EQ(site.lines.size(), 1);

    // design/surface is a node of the tree only. Isolated, the view hides its siblings at
    // each level: 0, ANNOT and survey at the root, design/road under design; Hide Others
    // then hides tin1 again, which isolating had shown.
    popup->layerTree()->setCurrentItem(popup->itemFor("design/surface"));
    ASSERT_TRUE(popup->hideOthers().ok());
    ASSERT_EQ(site.lines.size(), 3);
    EXPECT_EQ(site.lines[1], QString("VIEWS ISOLATE %1 design/surface").arg(site.plan()));
    EXPECT_EQ(site.lines[2], QString("VIEWS HIDE %1 design/surface/tin1").arg(site.plan()));
    EXPECT_EQ(site.state().layers.hidden(),
              (Hidden{"0", "ANNOT", "design/road", "survey", "design/surface/tin1"}));

    // Isolate alone replaces what the view hid: tin1 is shown again.
    popup->layerTree()->setCurrentItem(popup->itemFor("design/surface"));
    ASSERT_TRUE(popup->isolateSelected().ok());
    EXPECT_EQ(site.lines.back(), QString("VIEWS ISOLATE %1 design/surface").arg(site.plan()));
    EXPECT_EQ(site.state().layers.hidden(), (Hidden{"0", "ANNOT", "design/road", "survey"}));
    popup->close();
}

TEST(LayersPopup, IsolateOrHideOthersWithNothingSelectedRunsNoLine)
{
    PopupSite site;
    site.addLayers({"BUILDING"});
    ViewLayersPopup* popup = site.views->showLayersPopup(site.plan());
    ASSERT_NE(popup, nullptr);
    popup->layerTree()->setCurrentItem(nullptr);
    EXPECT_EQ(popup->isolateSelected().error().code, ErrorCode::NotFound);
    EXPECT_EQ(popup->hideOthers().error().code, ErrorCode::NotFound);
    EXPECT_TRUE(site.lines.isEmpty());
    popup->close();
}

TEST(LayersPopup, AnIsolateTheRunnerRefusesIsAStatusCarryingItsReason)
{
    PopupSite site;
    site.addLayers({"BUILDING", "ROAD"});
    ViewLayersPopup* popup = site.views->showLayersPopup(site.plan());
    ASSERT_NE(popup, nullptr);
    popup->layerTree()->setCurrentItem(popup->itemFor("ROAD"));
    site.refuse = true;
    const auto status = popup->isolateSelected();
    ASSERT_FALSE(status.ok());
    EXPECT_NE(status.error().message.find("refused by the test"), std::string::npos)
        << status.error().message;
    EXPECT_TRUE(site.state().layers.empty());
    popup->close();
}

TEST(LayersPopup, AReferenceLayersBoxRunsTheReferenceLine)
{
    PopupSite site;
    katana::interop::ReferenceData reference;
    katana::interop::RasterOverlay image;
    image.name = "ortho";
    image.width = 2;
    image.height = 2;
    image.rgba.assign(2 * 2 * 4, 255);
    const katana::interop::ReferenceId id = reference.add(image);
    site.views->setReferenceData(&reference);

    ViewLayersPopup* popup = site.views->showLayersPopup(site.plan());
    ASSERT_NE(popup, nullptr);
    QTreeWidget* tree = popup->referenceTree();
    ASSERT_EQ(tree->topLevelItemCount(), 1);
    tree->topLevelItem(0)->setCheckState(0, Qt::Unchecked);
    ASSERT_EQ(site.lines.size(), 1);
    EXPECT_EQ(site.lines.back(), QString("VIEWS HIDE %1 REFERENCE %2").arg(site.plan()).arg(id));
    EXPECT_TRUE(site.state().hiddenReferences.contains(id));
    EXPECT_EQ(site.views->hiddenCount(site.plan()), 1U) << "its Layers button says so";

    tree->topLevelItem(0)->setCheckState(0, Qt::Checked);
    EXPECT_EQ(site.lines.back(), QString("VIEWS SHOW %1 REFERENCE %2").arg(site.plan()).arg(id));
    EXPECT_TRUE(site.state().hiddenReferences.empty());

    // Show All takes the reference layers back as well as the layers.
    tree->topLevelItem(0)->setCheckState(0, Qt::Unchecked);
    auto* showAll = popup->findChild<QToolButton*>("ViewLayersShowAll");
    ASSERT_NE(showAll, nullptr);
    showAll->click();
    EXPECT_EQ(site.lines.back(), QString("VIEWS SHOW %1 ALL").arg(site.plan()));
    EXPECT_TRUE(site.state().hiddenReferences.empty());
    popup->close();
}
