// The Leaders manager's presets (src/katana_qt/annotation/leader_manager.hpp,
// docs/annotation.md "In the window"), driven by object name as a person
// clicks it: a preset chosen writes its template into the note, the surface
// it reads chosen beside it, and Apply or Make stores it - the same edit
// LEADER preset= makes, one undo step.
//
// The surface is the plane z = 10 + 0.1 x + 0.2 y over 0..100 x 0..100 (two
// triangles; a TIN interpolates a plane exactly): 21 at (30, 40), 20 at
// (60, 20).

#include <gtest/gtest.h>

#include <memory>
#include <string>
#include <variant>

#include <QComboBox>
#include <QLabel>
#include <QPlainTextEdit>
#include <QTableWidget>

#include "annotation/leader_manager.hpp"
#include "katana/cad/annotation/leader_edit.hpp"
#include "katana/cad/annotation/leader_presets.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_surface.hpp"
#include "widget_harness.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::entity::EntityId;
using katana::entity::LeaderGeometry;
using katana::geometry::Point3;
using katana::qt::LeaderManagerDialog;
using katana::qt::test::processEvents;
namespace ann = katana::cad::annotation;
namespace terrain = katana::terrain;

namespace {

struct Drawing {
    Document document;
    CommandInterpreter interpreter{document};

    Drawing()
    {
        auto made = terrain::TinSurface::create(
            {Point3(0, 0, 10), Point3(100, 0, 20), Point3(100, 100, 40), Point3(0, 100, 30)},
            {terrain::TinTriangle{0, 1, 2}, terrain::TinTriangle{0, 2, 3}});
        EXPECT_TRUE(made.ok());
        terrain::SurfaceStore store;
        terrain::NamedSurface ground;
        ground.name = "ground";
        ground.surface = std::make_shared<const terrain::TinSurface>(std::move(*made));
        ground.source = "test plane";
        EXPECT_TRUE(store.add(std::move(ground)).ok());
        EXPECT_TRUE(document.setSurfaces(std::move(store), "TEST_SURFACES").ok());
    }

    EntityId run(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << " -> " << reply.error().describe();
        const auto made = document.lastCreatedEntities();
        return made.empty() ? 0 : made.front();
    }
    // A point at x,y with level z.
    EntityId point(const std::string& at, const std::string& z)
    {
        const EntityId id = run("POINT " + at);
        run("SELECT " + std::to_string(id));
        run("PROP SET elevation " + z);
        return id;
    }
    [[nodiscard]] const LeaderGeometry& leader(EntityId id) const
    {
        return std::get<LeaderGeometry>(document.model().entities.find(id)->geometry);
    }
    [[nodiscard]] std::string says(EntityId id) const
    {
        return ann::leaderSays(document.model(), leader(id), ann::leaderContext(document));
    }
    [[nodiscard]] std::size_t steps() const { return document.history().undoCount(); }
};

template <typename Widget> Widget* child(QWidget& dialog, const char* name)
{
    auto* widget = dialog.findChild<Widget*>(QString::fromLatin1(name));
    EXPECT_NE(widget, nullptr) << name;
    return widget;
}

QStringList itemsOf(QComboBox* box)
{
    QStringList items;
    for (int i = 0; i < box->count(); ++i) {
        items << box->itemText(i);
    }
    return items;
}

} // namespace

TEST(LeaderManagerPresets, AChosenPresetWritesItsTemplateAndApplyStoresItInOneStep)
{
    Drawing d;
    const EntityId pit = d.point("30,40", "25");
    const EntityId leader = d.run("LEADER #" + std::to_string(pit) + " 40,50 text=PIT");
    LeaderManagerDialog dialog(d.document);
    ASSERT_TRUE(dialog.showLeader(leader));
    auto* preset = child<QComboBox>(dialog, "leaderPreset");
    auto* reads = child<QComboBox>(dialog, "leaderPresetReads");
    EXPECT_TRUE(preset->currentData().toString().isEmpty()) << "a plain note is no preset";
    EXPECT_FALSE(reads->isEnabled());

    preset->setCurrentIndex(preset->findData(QStringLiteral("surface")));
    EXPECT_TRUE(reads->isEnabled());
    EXPECT_EQ(itemsOf(reads),
              (QStringList{QStringLiteral("(first visible surface)"), QStringLiteral("ground")}));
    EXPECT_EQ(child<QComboBox>(dialog, "leaderNoteKind")->currentData().toInt(),
              static_cast<int>(ann::LeaderNote::Kind::Template));
    EXPECT_EQ(child<QPlainTextEdit>(dialog, "leaderNote")->toPlainText(),
              QStringLiteral("{dsurface.ground:+.3f}"));
    // 25 above ground 21; the surface nobody chose is said.
    EXPECT_EQ(dialog.preview(), QStringLiteral("+4.000"));
    EXPECT_EQ(dialog.problem(), QStringLiteral("Reads ground, the first visible surface."));
    EXPECT_EQ(d.says(leader), "PIT") << "nothing stored before Apply";

    const std::size_t before = d.steps();
    ASSERT_TRUE(dialog.apply()) << dialog.problem().toStdString();
    EXPECT_EQ(d.steps(), before + 1);
    EXPECT_EQ(d.says(leader), "+4.000");
    EXPECT_EQ(d.leader(leader).text, "{dsurface.ground:+.3f}");
}

TEST(LeaderManagerPresets, ChoosePresetReadsTheNamedSurfaceAndRefusesWhatTheDrawingHasNot)
{
    Drawing d;
    const EntityId pit = d.point("30,40", "18");
    const EntityId leader = d.run("LEADER #" + std::to_string(pit) + " 40,50 text=PIT");
    LeaderManagerDialog dialog(d.document);
    ASSERT_TRUE(dialog.showLeader(leader));
    // 18 under ground 21: a cut of 3.
    ASSERT_TRUE(dialog.choosePreset(QStringLiteral("cutfill"), QStringLiteral("ground")));
    EXPECT_EQ(dialog.preview(), QStringLiteral("CUT 3.000"));
    EXPECT_TRUE(dialog.problem().isEmpty()) << "named, nothing was chosen for the user";
    EXPECT_FALSE(dialog.choosePreset(QStringLiteral("cutfill"), QStringLiteral("nope")));
    EXPECT_TRUE(dialog.problem().contains(QStringLiteral("no surface or alignment")))
        << dialog.problem().toStdString();
    EXPECT_FALSE(dialog.choosePreset(QStringLiteral("bogus")));
    EXPECT_TRUE(dialog.problem().contains(QStringLiteral("no such leader preset")))
        << dialog.problem().toStdString();
    // A preset that reads only its target offers nothing to read.
    ASSERT_TRUE(dialog.choosePreset(QStringLiteral("level")));
    EXPECT_FALSE(child<QComboBox>(dialog, "leaderPresetReads")->isEnabled());
    EXPECT_EQ(dialog.preview(), QStringLiteral("RL 18.000"));
}

TEST(LeaderManagerPresets, ALeaderShownSaysWhichPresetItIsUntilItsNoteIsTypedAway)
{
    Drawing d;
    const EntityId pit = d.point("30,40", "25");
    const EntityId leader =
        d.run("LEADER #" + std::to_string(pit) + " 40,50 preset=cutfill surface=ground");
    LeaderManagerDialog dialog(d.document);
    ASSERT_TRUE(dialog.showLeader(leader));
    auto* preset = child<QComboBox>(dialog, "leaderPreset");
    auto* reads = child<QComboBox>(dialog, "leaderPresetReads");
    EXPECT_EQ(preset->currentData().toString(), QStringLiteral("cutfill"));
    EXPECT_EQ(reads->currentData().toString(), QStringLiteral("ground"));
    EXPECT_EQ(dialog.preview(), QStringLiteral("FILL 4.000"));
    // Typed away from the preset's template: no preset, and nothing applied.
    child<QPlainTextEdit>(dialog, "leaderNote")->setPlainText(QStringLiteral("CUT {cut.ground}"));
    EXPECT_TRUE(preset->currentData().toString().isEmpty());
    EXPECT_FALSE(reads->isEnabled());
    EXPECT_EQ(d.says(leader), "FILL 4.000");
}

TEST(LeaderManagerPresets, ForSelectionMakesAPresetLeaderToEachSelectedInOneStep)
{
    Drawing d;
    // 25 over ground 21 at (30, 40); 30 over ground 20 at (60, 20).
    const EntityId a = d.point("30,40", "25");
    const EntityId b = d.point("60,20", "30");
    d.run("SELECT " + std::to_string(a) + " " + std::to_string(b));
    LeaderManagerDialog dialog(d.document);
    dialog.showTab(LeaderManagerDialog::Tab::ForSelection);
    processEvents();
    ASSERT_TRUE(dialog.chooseForPreset(QStringLiteral("surface")));
    EXPECT_EQ(child<QPlainTextEdit>(dialog, "leaderForNote")->toPlainText(),
              QStringLiteral("{dsurface.ground:+.3f}"));
    EXPECT_EQ(dialog.forPreview(), QStringLiteral("+4.000")) << "for the first selected";
    const std::size_t before = d.steps();
    ASSERT_TRUE(dialog.makeForSelection()) << dialog.problem().toStdString();
    EXPECT_EQ(d.steps(), before + 1);
    const auto made = d.document.lastCreatedEntities();
    ASSERT_EQ(made.size(), 2U);
    EXPECT_EQ(d.says(made[0]), "+4.000");
    EXPECT_EQ(d.says(made[1]), "+10.000");
}

TEST(LeaderManagerPresets, TheValuesOfferTheSurfacesValuesAtTheTip)
{
    Drawing d;
    const EntityId pit = d.point("30,40", "25");
    const EntityId leader = d.run("LEADER #" + std::to_string(pit) + " 40,50 preset=level");
    LeaderManagerDialog dialog(d.document);
    ASSERT_TRUE(dialog.showLeader(leader));
    auto* table = child<QTableWidget>(dialog, "leaderValues");
    QStringList rows;
    for (int row = 0; row < table->rowCount(); ++row) {
        rows << table->item(row, 0)->text() + QStringLiteral("=") + table->item(row, 1)->text();
    }
    EXPECT_TRUE(rows.contains(QStringLiteral("surface.ground=21.000")))
        << rows.join(", ").toStdString();
    EXPECT_TRUE(rows.contains(QStringLiteral("dsurface.ground=4.000")))
        << rows.join(", ").toStdString();
    // The list says what the leader says now.
    EXPECT_EQ(d.says(leader), "RL 25.000");
}
