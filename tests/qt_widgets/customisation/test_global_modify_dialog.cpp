// GlobalModifyDialog: the form reads into the scope, filter and change that
// cad::planGlobalModify takes (whose own tests are
// tests/cad/customisation/test_global_modify.cpp), and its buttons preview,
// select and apply them. Driven by object name, as a person or the headless
// driver would, on a small drawing laid out in the fixture's comment.

#include <gtest/gtest.h>

#include <string>
#include <utility>
#include <vector>

#include <QCheckBox>
#include <QComboBox>
#include <QDoubleSpinBox>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QPushButton>
#include <QRadioButton>

#include "customisation/customisation_context.hpp"
#include "customisation/global_modify_dialog.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/layer_overrides.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::entity::Color;
using katana::entity::EntityId;
using katana::entity::Layer;
using katana::geometry::Point2;
using katana::qt::CustomisationContext;
using katana::qt::GlobalModifyDialog;
using katana::qt::GlobalModifyView;
namespace cmd = katana::commands;

namespace {

constexpr Color kGreen{0, 128, 0, 255};

// Two points on "survey" (p1 coded TREE, p2 coded POLE), a line on "roads".
struct DialogFixture {
    Document document;
    CustomisationContext context;
    std::vector<std::pair<QString, bool>> logged;
    EntityId p1 = 0, p2 = 0, road = 0;

    DialogFixture()
    {
        for (const char* name : {"survey", "roads"}) {
            Layer layer;
            layer.name = name;
            EXPECT_TRUE(document.execute(cmd::createLayer(layer)).ok());
        }
        p1 = create(cmd::createPoint(Point2(0, 0), {"survey", "", {}}));
        p2 = create(cmd::createPoint(Point2(10, 0), {"survey", "", {}}));
        road = create(cmd::createLine(Point2(0, 5), Point2(20, 5), {"roads", "", {}}));
        EXPECT_TRUE(
            document.execute(cmd::setEntityProperty({p1}, "code", std::string("TREE"))).ok());
        EXPECT_TRUE(
            document.execute(cmd::setEntityProperty({p2}, "code", std::string("POLE"))).ok());
        context.document = &document;
        context.log = [this](const QString& text, bool isError) {
            logged.emplace_back(text, isError);
        };
    }

    EntityId create(cmd::CommandPtr command)
    {
        EXPECT_TRUE(document.execute(std::move(command)).ok());
        return document.lastCreatedEntities().front();
    }

    const katana::entity::Entity& entity(EntityId id) const
    {
        return *document.model().entities.find(id);
    }
};

template <class Widget> Widget* find(GlobalModifyDialog& dialog, const char* name)
{
    auto* widget = dialog.findChild<Widget*>(QString::fromLatin1(name));
    EXPECT_NE(widget, nullptr) << name;
    return widget;
}

void check(GlobalModifyDialog& dialog, const char* name)
{
    if (auto* box = find<QCheckBox>(dialog, name)) {
        box->setChecked(true);
    }
}

void checkLayer(GlobalModifyDialog& dialog, const QString& name)
{
    auto* list = find<QListWidget>(dialog, "globalModifyLayers");
    for (int i = 0; i < list->count(); ++i) {
        if (list->item(i)->text() == name) {
            list->item(i)->setCheckState(Qt::Checked);
        }
    }
}

} // namespace

TEST(GlobalModifyDialog, EveryFieldStartsUntickedAndItsEditorDisabled)
{
    DialogFixture fixture;
    GlobalModifyDialog dialog(fixture.context);
    auto* colour = find<QLineEdit>(dialog, "globalModifyColour");
    EXPECT_FALSE(colour->isEnabled());
    check(dialog, "globalModifySetColour");
    EXPECT_TRUE(colour->isEnabled());
    // Nothing ticked is asking for no change, which the plan refuses; the
    // preview says how many match instead.
    find<QCheckBox>(dialog, "globalModifySetColour")->setChecked(false);
    find<QRadioButton>(dialog, "globalModifyScopeDrawing")->setChecked(true);
    EXPECT_TRUE(dialog.preview());
    EXPECT_EQ(dialog.summaryText(), "3 entities match. Tick a field to change.");
}

TEST(GlobalModifyDialog, ApplyToCheckedLayersWithAFilterIsOneUndoStep)
{
    DialogFixture fixture;
    GlobalModifyDialog dialog(fixture.context);
    find<QRadioButton>(dialog, "globalModifyScopeLayers")->setChecked(true);
    checkLayer(dialog, "survey");
    find<QLineEdit>(dialog, "globalModifyFilterProperty")->setText("code");
    find<QLineEdit>(dialog, "globalModifyFilterValue")->setText("tr*");
    check(dialog, "globalModifySetColour");
    find<QLineEdit>(dialog, "globalModifyColour")->setText("#008000");
    check(dialog, "globalModifySetLayerWeight");
    find<QDoubleSpinBox>(dialog, "globalModifyLayerWeight")->setValue(0.5);

    ASSERT_TRUE(dialog.preview());
    EXPECT_EQ(dialog.summaryText(), "1 entity matched; changing 1 entity and 1 layer.");
    const std::size_t undoBefore = fixture.document.history().undoCount();

    find<QPushButton>(dialog, "globalModifyApply")->click();

    EXPECT_EQ(fixture.entity(fixture.p1).color, kGreen);
    EXPECT_EQ(fixture.entity(fixture.p2).color, std::nullopt);
    EXPECT_EQ(fixture.document.model().layers.find("survey")->lineWeight, 0.5);
    EXPECT_EQ(fixture.document.history().undoCount(), undoBefore + 1);
    ASSERT_FALSE(fixture.logged.empty());
    EXPECT_FALSE(fixture.logged.back().second);
    EXPECT_TRUE(fixture.logged.back().first.startsWith("Global Modify: 1 entity matched"));
}

TEST(GlobalModifyDialog, AViewScopeTakesTheViewsOwnHiddenLayersAndItsAreaOnScreen)
{
    DialogFixture fixture;
    GlobalModifyDialog dialog(fixture.context);
    katana::cad::LayerOverrides hidesSurvey;
    hidesSurvey.hide("survey");
    dialog.views = [&] {
        return std::vector<GlobalModifyView>{
            GlobalModifyView{1, QStringLiteral("Plan 1"), &hidesSurvey,
                             katana::geometry::Box2(Point2(-1, 4), Point2(5, 6))},
            GlobalModifyView{2, QStringLiteral("3D 1"), nullptr, std::nullopt}};
    };
    dialog.reload();
    find<QRadioButton>(dialog, "globalModifyScopeView")->setChecked(true);

    // Plan 1 hides "survey": only the road is drawn there.
    auto scope = dialog.scope();
    ASSERT_TRUE(scope.ok());
    auto matched = katana::cad::matchEntities(fixture.document, *scope, {});
    ASSERT_TRUE(matched.ok());
    EXPECT_EQ(*matched, (std::vector<EntityId>{fixture.road}));

    // The 3D view hides nothing of its own, and has no area on screen to
    // limit to: asking for one is refused, naming why.
    find<QComboBox>(dialog, "globalModifyView")->setCurrentIndex(1);
    matched = katana::cad::matchEntities(fixture.document, *dialog.scope(), {});
    EXPECT_EQ(matched->size(), 3u);
    find<QCheckBox>(dialog, "globalModifyOnScreen")->setChecked(true);
    EXPECT_FALSE(dialog.scope().ok());
}

TEST(GlobalModifyDialog, SelectMatchesSelectsWhatTheScopeAndFilterTake)
{
    DialogFixture fixture;
    GlobalModifyDialog dialog(fixture.context);
    find<QRadioButton>(dialog, "globalModifyScopeDrawing")->setChecked(true);
    find<QCheckBox>(dialog, "globalModifyTypePoint")->setChecked(true);
    EXPECT_TRUE(dialog.selectMatches());
    EXPECT_EQ(fixture.document.selection().ids(), (std::vector<EntityId>{fixture.p1, fixture.p2}));
    EXPECT_EQ(dialog.summaryText(), "2 entities selected.");
}

TEST(GlobalModifyDialog, TextThatDoesNotReadIsSaidInTheSummaryAndChangesNothing)
{
    DialogFixture fixture;
    GlobalModifyDialog dialog(fixture.context);
    fixture.document.selection().set({fixture.p1});
    check(dialog, "globalModifySetColour");
    find<QLineEdit>(dialog, "globalModifyColour")->setText("greenish");
    const std::size_t undoBefore = fixture.document.history().undoCount();
    EXPECT_FALSE(dialog.apply());
    EXPECT_TRUE(dialog.summaryText().contains("#RRGGBB")) << dialog.summaryText().toStdString();
    EXPECT_EQ(fixture.document.history().undoCount(), undoBefore);
    ASSERT_FALSE(fixture.logged.empty());
    EXPECT_TRUE(fixture.logged.back().second);
}

TEST(GlobalModifyDialog, ASymbolAndAStyleSettingGoThroughTheirOwnTabs)
{
    DialogFixture fixture;
    GlobalModifyDialog dialog(fixture.context);
    fixture.document.selection().set({fixture.p1, fixture.p2, fixture.road});
    check(dialog, "globalModifySetSymbol");
    find<QComboBox>(dialog, "globalModifySymbol")->setCurrentText("tree");
    ASSERT_TRUE(dialog.apply()) << dialog.summaryText().toStdString();
    const std::string style = fixture.entity(fixture.p1).style;
    ASSERT_FALSE(style.empty());
    EXPECT_EQ(fixture.entity(fixture.p2).style, style);
    EXPECT_EQ(fixture.entity(fixture.road).style, "");
    EXPECT_TRUE(dialog.summaryText().contains("1 entity is not a point"));

    // Now the style they wear: its symbol size, through the Styles tab.
    find<QCheckBox>(dialog, "globalModifySetSymbol")->setChecked(false);
    check(dialog, "globalModifySetStyleSymbolSize");
    find<QDoubleSpinBox>(dialog, "globalModifyStyleSymbolSize")->setValue(2.0);
    ASSERT_TRUE(dialog.apply()) << dialog.summaryText().toStdString();
    EXPECT_EQ(fixture.document.model().styles.find(style)->symbolSize, 2.0);
}

TEST(GlobalModifyDialog, TheDialogOutlivesItsDocumentAndThenDoesNothing)
{
    auto fixture = std::make_unique<DialogFixture>();
    GlobalModifyDialog dialog(fixture->context);
    fixture.reset();
    EXPECT_FALSE(dialog.preview());
    EXPECT_FALSE(dialog.apply());
    EXPECT_FALSE(dialog.selectMatches());
}
