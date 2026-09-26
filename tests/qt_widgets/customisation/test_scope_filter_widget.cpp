// ScopeFilterWidget (src/katana_qt/customisation/scope_filter_widget.hpp):
// the "Apply to" and "Only those that match" controls Global Modify and the
// utilities dialog share. What is tested: every control carries the name its
// dialog gives it; the controls read into the ModifyScope and ModifyFilter
// matchEntities takes (Global Modify's own tests drive that through its
// dialog); every scope and every filter is said in the words of the shared
// grammar (cad::formatScopeWords); and those words, read back by the one
// parser, take exactly what the controls take - so a dialog's line cannot act
// on something else than its controls show. Driven by object name, as a
// person or the headless driver would, on a small drawing laid out in the
// fixture's comment; every expectation is worked out by hand from it.

#include <gtest/gtest.h>

#include <optional>
#include <string>
#include <utility>
#include <vector>

#include <QCheckBox>
#include <QComboBox>
#include <QLineEdit>
#include <QListWidget>
#include <QRadioButton>

#include "customisation/scope_filter_widget.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/layer_overrides.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/cad/view_set.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::cad::ScopeView;
using katana::cad::ScopeViewProvider;
using katana::core::ErrorCode;
using katana::entity::EntityId;
using katana::entity::Layer;
using katana::entity::TextGeometry;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::qt::ScopeChoice;
using katana::qt::ScopeFilterView;
using katana::qt::ScopeFilterWidget;
namespace cmd = katana::commands;

namespace {

// The drawing:
//
//   layer          entity    where           property
//   survey         pA point  (0,0)           code=TREE1
//   survey         pB point  (10,0)          code=POLE
//   survey/trees   pC point  (20,0)          code=TREE2
//   roads          ln line   (0,5)-(30,5)    -
//   roads          tx text   at (0,10)       -           "CH 100"
//
// and two views: Plan 1 (id 1) hides "survey" and shows x -1..12, y -1..6;
// 3D 1 (id 2) hides nothing and has no area on screen.
struct Fixture {
    Document document;
    EntityId pA = 0, pB = 0, pC = 0, ln = 0, tx = 0;
    katana::cad::LayerOverrides hidesSurvey;
    std::vector<ScopeFilterView> open;

    Fixture()
    {
        for (const char* name : {"survey", "survey/trees", "roads"}) {
            Layer layer;
            layer.name = name;
            EXPECT_TRUE(document.execute(cmd::createLayer(layer)).ok());
        }
        pA = create(cmd::createPoint(Point2(0, 0), {"survey", "", {}}));
        pB = create(cmd::createPoint(Point2(10, 0), {"survey", "", {}}));
        pC = create(cmd::createPoint(Point2(20, 0), {"survey/trees", "", {}}));
        ln = create(cmd::createLine(Point2(0, 5), Point2(30, 5), {"roads", "", {}}));
        TextGeometry text;
        text.position = Point2(0, 10);
        text.text = "CH 100";
        tx = create(cmd::createText(text, {"roads", "", {}}));
        for (const auto& [id, code] :
             {std::pair{pA, "TREE1"}, std::pair{pB, "POLE"}, std::pair{pC, "TREE2"}}) {
            EXPECT_TRUE(
                document.execute(cmd::setEntityProperty({id}, "code", std::string(code))).ok());
        }
        hidesSurvey.hide("survey");
        open = {ScopeFilterView{1, "Plan 1", &hidesSurvey, Box2(Point2(-1, -1), Point2(12, 6))},
                ScopeFilterView{2, "3D 1", nullptr, std::nullopt}};
    }

    EntityId create(cmd::CommandPtr command)
    {
        EXPECT_TRUE(document.execute(std::move(command)).ok());
        const auto created = document.lastCreatedEntities();
        return created.empty() ? 0 : created.front();
    }

    // The window's answer for VIEW, from the same views the widget lists.
    [[nodiscard]] ScopeViewProvider provider() const
    {
        return [this](std::optional<std::uint32_t> id) -> katana::core::Result<ScopeView> {
            for (const ScopeFilterView& view : open) {
                if (id && view.id == *id) {
                    ScopeView answer;
                    answer.id = view.id;
                    if (view.hidden != nullptr) {
                        answer.layers = *view.hidden;
                    }
                    answer.area = view.onScreen;
                    return answer;
                }
            }
            return katana::core::makeError(ErrorCode::NotFound, "no such view");
        };
    }
};

template <class Widget> Widget* find(ScopeFilterWidget& widget, const QString& name)
{
    auto* found = widget.findChild<Widget*>(name);
    EXPECT_NE(found, nullptr) << name.toStdString();
    return found;
}

void press(ScopeFilterWidget& widget, const char* name)
{
    if (auto* radio = find<QRadioButton>(widget, QString::fromLatin1(name))) {
        radio->click();
    }
}

void tick(ScopeFilterWidget& widget, const char* name, bool on = true)
{
    if (auto* box = find<QCheckBox>(widget, QString::fromLatin1(name))) {
        box->setChecked(on);
    }
}

void type(ScopeFilterWidget& widget, const char* name, const QString& text)
{
    if (auto* edit = find<QLineEdit>(widget, QString::fromLatin1(name))) {
        edit->setText(text);
    }
}

void tickLayer(ScopeFilterWidget& widget, const QString& name)
{
    auto* list = find<QListWidget>(widget, "gmLayers");
    for (int i = 0; i < list->count(); ++i) {
        if (list->item(i)->text() == name) {
            list->item(i)->setCheckState(Qt::Checked);
        }
    }
}

std::string words(const ScopeFilterWidget& widget)
{
    const auto line = widget.verbWords();
    EXPECT_TRUE(line.ok()) << (line.ok() ? "" : line.error().describe());
    return line.ok() ? line->toStdString() : std::string{};
}

std::string refusal(const ScopeFilterWidget& widget)
{
    const auto line = widget.verbWords();
    EXPECT_FALSE(line.ok()) << "said as " << (line.ok() ? line->toStdString() : "");
    return line.ok() ? std::string{} : line.error().message;
}

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

// A widget named "gm" over the fixture's drawing and views.
struct Bench {
    Fixture fixture;
    ScopeFilterWidget widget{"gm"};

    Bench()
    {
        widget.views = [this] { return fixture.open; };
        widget.reload(fixture.document.model());
    }
};

} // namespace

TEST(ScopeFilterWidget, EveryControlCarriesTheNameItsDialogGivesIt)
{
    for (const QString& prefix : {QStringLiteral("globalModify"), QStringLiteral("utility")}) {
        ScopeFilterWidget widget(prefix);
        EXPECT_EQ(widget.objectName(), prefix + "Scope");
        for (const char* part :
             {"ScopeGroup", "FilterGroup", "ScopeSelection", "ScopeView", "ScopeLayers",
              "ScopeDrawing", "View", "OnScreen", "Layers", "Sublayers", "TypePoint", "TypeLine",
              "TypeArc", "TypePolyline", "TypeCircle", "TypeText", "TypeDimension", "FilterLayer",
              "FilterStyle", "FilterColour", "FilterProperty", "FilterValue", "FilterText",
              "DrawnOnly"}) {
            EXPECT_NE(widget.findChild<QWidget*>(prefix + part), nullptr)
                << (prefix + part).toStdString();
        }
    }
}

TEST(ScopeFilterWidget, EveryGeometryKindHasATypeBoxAndSaysItsNameInTheWordsTheGrammarReads)
{
    // The boxes are counted off the variant: the list once stopped at
    // Dimension, so a label, a leader and the drawing system's curve
    // polyline, ellipse and spline could be filtered by TYPE= but not here.
    Bench bench;
    ScopeFilterWidget& widget = bench.widget;
    for (const char* part : {"TypeLabel", "TypeLeader", "TypeCurvePolyline", "TypeEllipse",
                             "TypeSpline"}) {
        EXPECT_NE(widget.findChild<QCheckBox*>(QStringLiteral("gm") + part), nullptr) << part;
    }
    press(widget, "gmScopeDrawing");
    tick(widget, "gmTypeCurvePolyline");
    tick(widget, "gmTypeSpline");
    const std::string said = words(widget);
    EXPECT_EQ(said, "DRAWING WHERE TYPE=curvepolyline,spline");
    const auto tokens = CommandInterpreter::tokenize(said);
    ASSERT_TRUE(tokens.ok());
    std::size_t at = 0;
    const auto read = katana::cad::parseScopeWords(*tokens, at);
    ASSERT_TRUE(read.ok()) << said << ": " << read.error().describe();
    const auto filter = widget.filter();
    ASSERT_TRUE(filter.ok());
    EXPECT_EQ(read->filter.types, filter->types) << "the words read back as the boxes say";
}

TEST(ScopeFilterWidget, OnlyTheChosenScopesOwnControlsAreLive)
{
    Bench bench;
    ScopeFilterWidget& widget = bench.widget;
    EXPECT_EQ(widget.choice(), ScopeChoice::Selection);
    EXPECT_FALSE(find<QComboBox>(widget, "gmView")->isEnabled());
    EXPECT_FALSE(find<QListWidget>(widget, "gmLayers")->isEnabled());
    press(widget, "gmScopeView");
    EXPECT_EQ(widget.choice(), ScopeChoice::View);
    EXPECT_TRUE(find<QComboBox>(widget, "gmView")->isEnabled());
    EXPECT_TRUE(find<QCheckBox>(widget, "gmOnScreen")->isEnabled());
    EXPECT_FALSE(find<QListWidget>(widget, "gmLayers")->isEnabled());
    widget.setChoice(ScopeChoice::Layers);
    EXPECT_TRUE(find<QListWidget>(widget, "gmLayers")->isEnabled());
    EXPECT_TRUE(find<QCheckBox>(widget, "gmSublayers")->isEnabled());
    EXPECT_FALSE(find<QComboBox>(widget, "gmView")->isEnabled());
}

TEST(ScopeFilterWidget, EachScopeIsSaidInTheWordsTheGrammarReads)
{
    Bench bench;
    ScopeFilterWidget& widget = bench.widget;
    EXPECT_EQ(words(widget), "SELECTION");
    press(widget, "gmScopeDrawing");
    EXPECT_EQ(words(widget), "DRAWING");

    // The checked layers, in the list's order (names ascending), with or
    // without their sublayers; none ticked is refused, as scope() refuses it.
    press(widget, "gmScopeLayers");
    EXPECT_TRUE(contains(refusal(widget), "tick at least one layer"));
    tickLayer(widget, "survey");
    tickLayer(widget, "roads");
    EXPECT_EQ(words(widget), "LAYERS roads,survey");
    tick(widget, "gmSublayers", false);
    EXPECT_EQ(words(widget), "LAYERS roads,survey ONLY");

    // A view by its id: what it shows on screen, or its layers anywhere. The
    // list shows the id beside the title, since the number in a title counts
    // only the views of its kind: 3D 1 is VIEW 2.
    press(widget, "gmScopeView");
    auto* views = find<QComboBox>(widget, "gmView");
    ASSERT_EQ(views->count(), 2);
    EXPECT_EQ(views->itemText(0), "Plan 1 (VIEW 1)");
    EXPECT_EQ(views->itemText(1), "3D 1 (VIEW 2)");
    views->setCurrentIndex(0);
    EXPECT_EQ(words(widget), "VIEW 1 EXTENTS");
    tick(widget, "gmOnScreen");
    EXPECT_EQ(words(widget), "VIEW 1");
    // The 3D view has no area on screen: asked for one, refused as scope()
    // refuses it; without, its layers anywhere.
    find<QComboBox>(widget, "gmView")->setCurrentIndex(1);
    EXPECT_TRUE(contains(refusal(widget), "only a plan view has an area on screen"));
    EXPECT_FALSE(widget.scope().ok());
    tick(widget, "gmOnScreen", false);
    EXPECT_EQ(words(widget), "VIEW 2 EXTENTS");
    // A view closed since it was chosen names nothing.
    bench.fixture.open.pop_back();
    const auto closed = widget.verbWords();
    ASSERT_FALSE(closed.ok());
    EXPECT_EQ(closed.error().code, ErrorCode::NotFound);
}

TEST(ScopeFilterWidget, WithNoWorkspaceTheViewScopeIsReadButNamesNoViewALineCanCarry)
{
    ScopeFilterWidget widget("gm");
    Document document;
    widget.reload(document.model());
    ASSERT_EQ(find<QComboBox>(widget, "gmView")->count(), 1);
    EXPECT_EQ(find<QComboBox>(widget, "gmView")->itemText(0), "Whole drawing view");
    press(widget, "gmScopeView");
    // What Global Modify reads: a view that hides nothing of its own.
    const auto scope = widget.scope();
    ASSERT_TRUE(scope.ok());
    EXPECT_EQ(scope->view, nullptr);
    EXPECT_FALSE(scope->area.has_value());
    const auto line = widget.verbWords();
    ASSERT_FALSE(line.ok());
    EXPECT_EQ(line.error().code, ErrorCode::InvalidState);
}

TEST(ScopeFilterWidget, EachFilterIsSaidInTheWordsTheGrammarReads)
{
    Bench bench;
    ScopeFilterWidget& widget = bench.widget;
    press(widget, "gmScopeDrawing");
    tick(widget, "gmTypePoint");
    tick(widget, "gmTypeLine");
    EXPECT_EQ(words(widget), "DRAWING WHERE TYPE=point,line");
    tick(widget, "gmTypePoint", false);
    tick(widget, "gmTypeLine", false);

    type(widget, "gmFilterLayer", " survey/*, roads ");
    EXPECT_EQ(words(widget), "DRAWING WHERE LAYER=survey/*,roads");
    type(widget, "gmFilterLayer", "");
    type(widget, "gmFilterStyle", "ByLayer");
    EXPECT_EQ(words(widget), "DRAWING WHERE STYLE=ByLayer");
    type(widget, "gmFilterStyle", "");
    type(widget, "gmFilterColour", "#ff8000");
    EXPECT_EQ(words(widget), "DRAWING WHERE COLOUR=#FF8000");
    type(widget, "gmFilterColour", "bylayer");
    EXPECT_EQ(words(widget), "DRAWING WHERE COLOUR=ByLayer");
    type(widget, "gmFilterColour", "orange");
    EXPECT_TRUE(contains(refusal(widget), "the filter's colour: type #RRGGBB or ByLayer"));
    type(widget, "gmFilterColour", "");

    // A value with no property is a value any property holds, as Global
    // Modify reads it.
    type(widget, "gmFilterValue", "TREE*");
    EXPECT_EQ(words(widget), "DRAWING WHERE PROP=:TREE*");
    // A property whose name has a ':' cannot be said: PROP= ends the name at
    // the first one, so the words would take another filter. Refused.
    type(widget, "gmFilterValue", "");
    type(widget, "gmFilterProperty", "addr:street");
    EXPECT_TRUE(contains(refusal(widget), "cannot be written as PROP=")) << refusal(widget);
    type(widget, "gmFilterValue", "TREE*");
    type(widget, "gmFilterProperty", "code");
    EXPECT_EQ(words(widget), "DRAWING WHERE PROP=code:TREE*");
    type(widget, "gmFilterValue", "");
    EXPECT_EQ(words(widget), "DRAWING WHERE PROP=code");
    type(widget, "gmFilterProperty", "");

    // A word with a blank in it is quoted whole, as the command line reads it.
    type(widget, "gmFilterText", "CH *");
    EXPECT_EQ(words(widget), "DRAWING WHERE \"TEXT=CH *\"");
    type(widget, "gmFilterText", "");
    tick(widget, "gmDrawnOnly");
    EXPECT_EQ(words(widget), "DRAWING WHERE DRAWN");

    // All at once, in the grammar's order, after the scope.
    press(widget, "gmScopeLayers");
    tickLayer(widget, "survey");
    tick(widget, "gmTypePoint");
    type(widget, "gmFilterLayer", "survey/*");
    type(widget, "gmFilterStyle", "ByLayer");
    type(widget, "gmFilterColour", "ByLayer");
    type(widget, "gmFilterProperty", "code");
    type(widget, "gmFilterValue", "TREE*");
    type(widget, "gmFilterText", "*");
    EXPECT_EQ(words(widget), "LAYERS survey WHERE TYPE=point LAYER=survey/* STYLE=ByLayer "
                             "COLOUR=ByLayer PROP=code:TREE* TEXT=* DRAWN");
}

// The point of verbWords: the line a dialog runs acts on exactly what its
// controls show. Each configuration's words are read back by the grammar's
// one parser and resolved with the window's answer for VIEW, and take the
// same entities as the controls' own scope and filter.
TEST(ScopeFilterWidget, TheWordsTakeWhatTheControlsTake)
{
    Bench bench;
    Fixture& fixture = bench.fixture;
    ScopeFilterWidget& widget = bench.widget;
    fixture.document.selection().set({fixture.pB, fixture.ln});
    const ScopeViewProvider views = fixture.provider();

    const auto same = [&](const std::vector<EntityId>& expected, const char* what) {
        const auto scope = widget.scope();
        const auto filter = widget.filter();
        ASSERT_TRUE(scope.ok() && filter.ok()) << what;
        const auto byControls = katana::cad::matchEntities(fixture.document, *scope, *filter);
        ASSERT_TRUE(byControls.ok()) << what;
        EXPECT_EQ(*byControls, expected) << what;

        const auto line = widget.verbWords();
        ASSERT_TRUE(line.ok()) << what;
        const auto tokens = CommandInterpreter::tokenize(line->toStdString());
        ASSERT_TRUE(tokens.ok()) << what;
        std::size_t at = 0;
        const auto parsed = katana::cad::parseScopeWords(*tokens, at);
        ASSERT_TRUE(parsed.ok()) << what << ": " << line->toStdString();
        EXPECT_EQ(at, tokens->size()) << what << ": every word is the scope's";
        const auto byWords = katana::cad::matchScope(fixture.document, *parsed, views);
        ASSERT_TRUE(byWords.ok()) << what;
        EXPECT_EQ(byWords->matched, expected) << what << ": " << line->toStdString();
    };

    same({fixture.pB, fixture.ln}, "the selection");
    press(widget, "gmScopeDrawing");
    same({fixture.pA, fixture.pB, fixture.pC, fixture.ln, fixture.tx}, "the drawing");
    tick(widget, "gmTypePoint");
    type(widget, "gmFilterProperty", "code");
    type(widget, "gmFilterValue", "tree*");
    same({fixture.pA, fixture.pC}, "the drawing's trees");
    tick(widget, "gmTypePoint", false);
    type(widget, "gmFilterProperty", "");
    same({fixture.pA, fixture.pC}, "a value any property holds");
    type(widget, "gmFilterValue", "");

    press(widget, "gmScopeLayers");
    tickLayer(widget, "survey");
    same({fixture.pA, fixture.pB, fixture.pC}, "survey and its sublayer");
    tick(widget, "gmSublayers", false);
    same({fixture.pA, fixture.pB}, "survey only");

    // Plan 1 hides survey: its layers anywhere are the roads; on screen
    // (x -1..12, y -1..6) the line alone, the text at y 10 being off it.
    press(widget, "gmScopeView");
    find<QComboBox>(widget, "gmView")->setCurrentIndex(0);
    same({fixture.ln, fixture.tx}, "Plan 1 anywhere");
    tick(widget, "gmOnScreen");
    same({fixture.ln}, "Plan 1 on screen");
    type(widget, "gmFilterText", "CH *");
    same({}, "Plan 1 on screen, texts CH *: nothing, which is an answer");
    tick(widget, "gmOnScreen", false);
    same({fixture.tx}, "Plan 1 anywhere, texts CH *");
    type(widget, "gmFilterText", "");
    find<QComboBox>(widget, "gmView")->setCurrentIndex(1);
    same({fixture.pA, fixture.pB, fixture.pC, fixture.ln, fixture.tx}, "3D 1 anywhere");
}

TEST(ScopeFilterWidget, AnEditIsReportedAReloadIsNotAndKeepsWhatIsTicked)
{
    Bench bench;
    ScopeFilterWidget& widget = bench.widget;
    int changes = 0;
    widget.onChanged = [&changes] { ++changes; };
    type(widget, "gmFilterText", "CH");
    EXPECT_EQ(changes, 1);
    changes = 0;
    press(widget, "gmScopeLayers");
    EXPECT_EQ(changes, 1);
    tickLayer(widget, "roads");
    EXPECT_EQ(changes, 2);

    // A layer made since is listed; roads stays ticked, and nothing is
    // reported for the refill.
    Layer kerbs;
    kerbs.name = "kerbs";
    ASSERT_TRUE(bench.fixture.document.execute(cmd::createLayer(kerbs)).ok());
    changes = 0;
    widget.reload(bench.fixture.document.model());
    EXPECT_EQ(changes, 0);
    auto* list = find<QListWidget>(widget, "gmLayers");
    QStringList ticked;
    QStringList listed;
    for (int i = 0; i < list->count(); ++i) {
        listed << list->item(i)->text();
        if (list->item(i)->checkState() == Qt::Checked) {
            ticked << list->item(i)->text();
        }
    }
    EXPECT_TRUE(listed.contains("kerbs"));
    EXPECT_EQ(ticked, QStringList{"roads"});
    EXPECT_EQ(words(widget), "LAYERS roads WHERE TEXT=CH");
}

TEST(ScopeFilterWidget, AWorkspacesViewsAreOfferedWithAPlanViewsAreaOnScreen)
{
    katana::cad::ViewSet set;
    katana::cad::ViewState& plan = set.add(katana::cad::ViewKind::Plan);
    plan.plan.center = Point2(15, 0);
    plan.plan.scale = 10.0;
    plan.plan.widthPixels = 200.0;
    plan.plan.heightPixels = 20.0;
    plan.layers.hide("roads");
    katana::cad::ViewState& model = set.add(katana::cad::ViewKind::Model3D);
    const std::vector<ScopeFilterView> views = katana::qt::scopeFilterViews(set);
    ASSERT_EQ(views.size(), 2u);
    EXPECT_EQ(views[0].id, plan.id);
    EXPECT_EQ(views[0].title, "Plan 1");
    EXPECT_EQ(views[0].hidden, &plan.layers);
    // x 15 -+ 100/10, y 0 -+ 10/10, by hand.
    ASSERT_TRUE(views[0].onScreen.has_value());
    EXPECT_EQ(views[0].onScreen->min, Point2(5, -1));
    EXPECT_EQ(views[0].onScreen->max, Point2(25, 1));
    EXPECT_EQ(views[1].id, model.id);
    EXPECT_EQ(views[1].hidden, &model.layers);
    EXPECT_FALSE(views[1].onScreen.has_value());
}
