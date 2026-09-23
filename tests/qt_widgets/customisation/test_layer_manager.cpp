// The Layers dialog's half of audit QT-02: a layer naming a 12d linestyle no
// loaded library defines - and a hatch pattern and dimension style the model
// lacks - keeps every name through Save, and an unedited Save is no command.

#include <gtest/gtest.h>

#include <string>

#include <QComboBox>
#include <QDoubleSpinBox>
#include <QPushButton>
#include <QTableWidget>

#include "customisation/name_picker.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"
#include "layer_manager.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::entity::Layer;
using katana::qt::LayerManagerDialog;
using katana::qt::NamePicker;

namespace {

// Layer "survey" as a 12d import leaves it when its customisation is not
// loaded: a linetype, a hatch pattern and a dimension style that nothing in
// this drawing defines, and a weight of 0.125 mm, which the dialog's
// two-decimal weight box shows as 0.13 (0.125 rounds half away from zero).
struct LayerDialog : ::testing::Test {
    Document document;
    Layer survey;

    void SetUp() override
    {
        survey.name = "survey";
        survey.linetype = "Old 12d Kerb";
        survey.hatchPattern = "Old 12d Hatch";
        survey.dimensionStyle = "Old 12d Dimensions";
        survey.lineWeight = 0.125;
        survey.color = katana::entity::Color{0, 200, 120, 255};
        const auto status = document.execute(katana::commands::createLayer(survey));
        ASSERT_TRUE(status.ok()) << status.error().describe();
    }

    const Layer& stored() const { return *document.model().layers.find("survey"); }

    static void selectLayer(LayerManagerDialog& dialog, const QString& name)
    {
        auto* table = dialog.findChild<QTableWidget*>("layerTable");
        ASSERT_NE(table, nullptr);
        for (int row = 0; row < table->rowCount(); ++row) {
            if (table->item(row, 0) != nullptr && table->item(row, 0)->text() == name) {
                table->selectRow(row);
                return;
            }
        }
        FAIL() << "no row for " << name.toStdString();
    }
};

} // namespace

TEST_F(LayerDialog, SavingALayerNamingUndefinedNamesUneditedKeepsEveryNameAndPushesNothing)
{
    LayerManagerDialog dialog(document, [](const QString&, bool) {});
    selectLayer(dialog, "survey");
    const std::size_t steps = document.history().undoCount();
    auto* save = dialog.findChild<QPushButton*>("layerSave");
    ASSERT_NE(save, nullptr);
    save->click();

    EXPECT_EQ(stored().linetype, "Old 12d Kerb");
    EXPECT_EQ(stored().hatchPattern, "Old 12d Hatch");
    EXPECT_EQ(stored().dimensionStyle, "Old 12d Dimensions");
    EXPECT_EQ(stored().lineWeight, 0.125);
    EXPECT_EQ(stored(), survey);
    EXPECT_EQ(document.history().undoCount(), steps);

    // The field shows the stored name, marked, not a neighbour's. By RTTI:
    // NamePicker has no Q_OBJECT, so qobject_cast cannot tell it apart.
    auto* linetype = dynamic_cast<NamePicker*>(dialog.findChild<QComboBox*>("layerLinetype"));
    ASSERT_NE(linetype, nullptr);
    EXPECT_EQ(linetype->currentName(), "Old 12d Kerb");
    EXPECT_FALSE(linetype->currentIsDefined());
}

TEST_F(LayerDialog, EditingTheWeightAloneSavesItAndStillKeepsTheUndefinedNames)
{
    LayerManagerDialog dialog(document, [](const QString&, bool) {});
    selectLayer(dialog, "survey");
    auto* weight = dialog.findChild<QDoubleSpinBox*>("layerWeight");
    ASSERT_NE(weight, nullptr);
    weight->setValue(0.5);
    const std::size_t steps = document.history().undoCount();
    dialog.findChild<QPushButton*>("layerSave")->click();

    EXPECT_EQ(document.history().undoCount(), steps + 1);
    EXPECT_EQ(stored().lineWeight, 0.5);
    EXPECT_EQ(stored().linetype, "Old 12d Kerb");
    EXPECT_EQ(stored().hatchPattern, "Old 12d Hatch");
    EXPECT_EQ(stored().dimensionStyle, "Old 12d Dimensions");
    EXPECT_EQ(stored().color, survey.color);
}
