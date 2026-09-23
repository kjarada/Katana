// The Layers dialog as a non-modal window beside the drawing: it reloads once,
// from the event loop, when the drawing changes anywhere; it asks for names
// in a prompt row and takes a colour typed or picked in a dialog it opens
// without waiting; and it outlives its Document without touching it.

#include <gtest/gtest.h>

#include <optional>
#include <string>
#include <vector>

#include <QApplication>
#include <QCheckBox>
#include <QColorDialog>
#include <QDoubleSpinBox>
#include <QFrame>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QTableWidget>

#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"
#include "layer_manager.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::entity::Color;
using katana::entity::Layer;
using katana::qt::LayerManagerDialog;

namespace {

template <typename T> T* child(QWidget& parent, const char* name)
{
    T* found = parent.findChild<T*>(QString::fromLatin1(name));
    EXPECT_NE(found, nullptr) << "no " << name;
    return found;
}

void click(QWidget& dialog, const char* name)
{
    auto* button = child<QPushButton>(dialog, name);
    ASSERT_NE(button, nullptr);
    button->click();
}

// Types `text` into the prompt and presses OK.
void answer(QWidget& dialog, const QString& text)
{
    auto* name = child<QLineEdit>(dialog, "layerPromptName");
    ASSERT_NE(name, nullptr);
    name->setText(text);
    click(dialog, "layerPromptOk");
}

std::vector<std::string> tableNames(QWidget& dialog)
{
    std::vector<std::string> names;
    auto* table = child<QTableWidget>(dialog, "layerTable");
    for (int row = 0; table != nullptr && row < table->rowCount(); ++row) {
        names.push_back(table->item(row, 0)->text().toStdString());
    }
    return names;
}

// Layer "survey" with a colour and a weight, and a log of what the dialog
// said.
struct LayersModeless : ::testing::Test {
    Document document;
    std::vector<std::pair<QString, bool>> said{};
    LayerManagerDialog::Log log = [this](const QString& message, bool isError) {
        said.emplace_back(message, isError);
    };

    void SetUp() override
    {
        Layer survey;
        survey.name = "survey";
        survey.color = Color{0, 200, 120, 255};
        survey.lineWeight = 0.35;
        ASSERT_TRUE(document.execute(katana::commands::createLayer(survey)).ok());
    }

    const Layer& stored(const char* name) const { return *document.model().layers.find(name); }
};

} // namespace

TEST_F(LayersModeless, ACommandRunElsewhereReloadsTheTableOnceFromTheEventLoopKeepingTheSelection)
{
    LayerManagerDialog dialog(document, log);
    dialog.selectLayer("survey");
    ASSERT_EQ(dialog.selectedLayer(), "survey");

    ASSERT_TRUE(document.execute(katana::commands::createLayer(Layer{"roads"})).ok());
    ASSERT_TRUE(document.execute(katana::commands::createLayer(Layer{"kerbs"})).ok());
    // Not inside the command: the reload is deferred (docs/cad.md).
    EXPECT_EQ(tableNames(dialog), (std::vector<std::string>{"0", "survey"}));
    katana::qt::test::processEvents();
    EXPECT_EQ(tableNames(dialog), (std::vector<std::string>{"0", "kerbs", "roads", "survey"}));
    EXPECT_EQ(dialog.selectedLayer(), "survey") << "the selection survives the reload";

    // And an undo made anywhere is seen the same way.
    ASSERT_TRUE(document.undo().ok());
    katana::qt::test::processEvents();
    EXPECT_EQ(tableNames(dialog), (std::vector<std::string>{"0", "roads", "survey"}));
}

TEST_F(LayersModeless, NewAndNewChildAskForTheNameInTheDialogAndSelectWhatTheyMade)
{
    LayerManagerDialog dialog(document, log);
    auto* prompt = child<QFrame>(dialog, "layerPrompt");
    ASSERT_NE(prompt, nullptr);
    EXPECT_TRUE(prompt->isHidden());

    click(dialog, "layerNew");
    // A row inside the dialog, not a box: nothing modal is open.
    EXPECT_FALSE(prompt->isHidden());
    EXPECT_EQ(QApplication::activeModalWidget(), nullptr);
    answer(dialog, "  design/surface  "); // the blanks either end are never meant
    EXPECT_TRUE(prompt->isHidden());
    ASSERT_TRUE(document.model().layers.contains("design/surface"));
    katana::qt::test::processEvents();
    EXPECT_EQ(dialog.selectedLayer(), "design/surface");

    dialog.selectLayer("design");
    click(dialog, "layerNewChild");
    EXPECT_FALSE(prompt->isHidden());
    answer(dialog, "tin1");
    EXPECT_TRUE(document.model().layers.contains("design/tin1"));
    katana::qt::test::processEvents();
    EXPECT_EQ(dialog.selectedLayer(), "design/tin1");
}

TEST_F(LayersModeless, RenameOrMoveOffersThePathAndTakesTheLayerAndItsEntitiesWithIt)
{
    ASSERT_TRUE(document
                    .execute(katana::commands::createPoint(katana::geometry::Point2(1, 1),
                                                           {"survey", "", {}}))
                    .ok());
    const katana::entity::EntityId point = document.lastCreatedEntities().front();
    LayerManagerDialog dialog(document, log);
    dialog.selectLayer("survey");
    click(dialog, "layerMove");
    auto* name = child<QLineEdit>(dialog, "layerPromptName");
    ASSERT_NE(name, nullptr);
    EXPECT_EQ(name->text(), "survey") << "the path to edit, not an empty field";
    name->setText("site/survey");
    Q_EMIT name->returnPressed(); // Enter takes the answer, as OK does

    EXPECT_FALSE(document.model().layers.contains("survey"));
    EXPECT_EQ(document.model().entities.find(point)->layer, "site/survey");
    katana::qt::test::processEvents();
    EXPECT_EQ(dialog.selectedLayer(), "site/survey");
}

TEST_F(LayersModeless, ARefusedNameIsSaidOnTheStatusLineAndInTheLogAndChangesNothing)
{
    LayerManagerDialog dialog(document, log);
    const std::size_t steps = document.history().undoCount();
    click(dialog, "layerNew");
    answer(dialog, "a//b");
    EXPECT_EQ(document.history().undoCount(), steps);
    EXPECT_FALSE(document.model().layers.contains("a//b"));
    auto* status = child<QLabel>(dialog, "layerStatus");
    ASSERT_NE(status, nullptr);
    EXPECT_TRUE(status->text().contains("empty level")) << status->text().toStdString();
    ASSERT_FALSE(said.empty());
    EXPECT_TRUE(said.back().second) << "an error";
}

TEST_F(LayersModeless, AColourIsTypedAsHexOrPickedInADialogThatDoesNotBlock)
{
    LayerManagerDialog dialog(document, log);
    dialog.selectLayer("survey");
    auto* text = child<QLineEdit>(dialog, "layerColourText");
    ASSERT_NE(text, nullptr);
    EXPECT_EQ(text->text(), "#00C878") << "0, 200, 120";

    // Something that is not a colour is refused, and the field shows the
    // last good one again.
    text->setText("orange-ish");
    Q_EMIT text->editingFinished();
    EXPECT_EQ(text->text(), "#00C878");
    EXPECT_TRUE(said.back().second);

    // Typed: #ff8000 is 255, 128, 0, and is shown as the model writes it.
    text->setText("#ff8000");
    Q_EMIT text->editingFinished();
    EXPECT_EQ(text->text(), "#FF8000");
    click(dialog, "layerSave");
    EXPECT_EQ(stored("survey").color, (Color{255, 128, 0, 255}));
    EXPECT_EQ(stored("survey").lineWeight, 0.35) << "only the colour was edited";

    // Picked: the button returns at once - the dialog is opened, not
    // exec()'d - and its answer arrives when it is accepted.
    katana::qt::test::processEvents();
    click(dialog, "layerColour");
    auto* picker = dialog.findChild<QColorDialog*>("layerColourDialog");
    ASSERT_NE(picker, nullptr);
    EXPECT_TRUE(picker->isVisible());
    picker->setCurrentColor(QColor(0, 0, 255));
    picker->accept(); // QColorDialog::done emits colorSelected, as OK does
    EXPECT_EQ(text->text(), "#0000FF");
    click(dialog, "layerSave");
    EXPECT_EQ(stored("survey").color, (Color{0, 0, 255, 255}));
}

TEST_F(LayersModeless, UnsavedEditsSurviveAReloadUnlessTheirLayerChangedUnderneath)
{
    LayerManagerDialog dialog(document, log);
    dialog.selectLayer("survey");
    auto* weight = child<QDoubleSpinBox>(dialog, "layerWeight");
    ASSERT_NE(weight, nullptr);
    weight->setValue(0.5);

    // Another layer's creation reloads the table; the form's edit stays.
    ASSERT_TRUE(document.execute(katana::commands::createLayer(Layer{"other"})).ok());
    katana::qt::test::processEvents();
    EXPECT_EQ(weight->value(), 0.5);
    click(dialog, "layerSave");
    EXPECT_EQ(stored("survey").lineWeight, 0.5);

    // Survey itself hidden elsewhere: the form shows the layer as it is now,
    // so a Save cannot write the old visibility back.
    weight->setValue(1.0);
    Layer hidden = stored("survey");
    hidden.visible = false;
    ASSERT_TRUE(document.execute(katana::commands::updateLayer(hidden)).ok());
    katana::qt::test::processEvents();
    EXPECT_FALSE(child<QCheckBox>(dialog, "layerVisible")->isChecked());
    EXPECT_EQ(weight->value(), 0.5);
}

TEST_F(LayersModeless, TheDialogMayOutliveItsDocumentAndThenNeitherReadsNorChangesIt)
{
    // The Document dies and a stranger is built in the same storage: a
    // dialog still reading through its pointer would show the stranger's
    // layer, rather than crash in a way that may or may not happen.
    std::optional<Document> slot(std::in_place);
    LayerManagerDialog dialog(*slot, log);
    slot.reset();
    slot.emplace();
    Layer probe;
    probe.name = "stranger";
    probe.lineWeight = 2.0;
    ASSERT_TRUE(slot->execute(katana::commands::createLayer(probe)).ok());
    katana::qt::test::processEvents();
    const std::size_t strangerSteps = slot->history().undoCount();
    EXPECT_EQ(tableNames(dialog), (std::vector<std::string>{"0"})) << "no reload from it";

    // Selecting a row, as a click does, loads no form: the stranger's "0"
    // would show its 0.25 mm, where the untouched field shows 0.
    dialog.selectLayer("0");
    EXPECT_EQ(child<QDoubleSpinBox>(dialog, "layerWeight")->value(), 0.0);
    // ...and from then on every control but Close is off...
    EXPECT_FALSE(child<QPushButton>(dialog, "layerNew")->isEnabled());
    EXPECT_FALSE(child<QPushButton>(dialog, "layerSave")->isEnabled());
    EXPECT_FALSE(child<QLineEdit>(dialog, "layerColourText")->isEnabled());
    EXPECT_TRUE(child<QPushButton>(dialog, "closeButton")->isEnabled());
    // ...and a handler reached all the same acts on nothing.
    Q_EMIT child<QPushButton>(dialog, "layerNew")->clicked();
    EXPECT_TRUE(child<QFrame>(dialog, "layerPrompt")->isHidden());
    Q_EMIT child<QPushButton>(dialog, "layerDelete")->clicked();
    Q_EMIT child<QPushButton>(dialog, "layerCurrent")->clicked();
    EXPECT_EQ(slot->history().undoCount(), strangerSteps);
    EXPECT_EQ(slot->currentLayer(), "0");
    EXPECT_TRUE(child<QLabel>(dialog, "layerStatus")->text().contains("has closed"));
}
