// A section viewport's scale in the sheet editor (src/katana_qt/sheet_editor,
// docs/plotting.md "Section smarts"): the scale box offers Auto, as a plan's
// does, and says in its tooltip what the automatic scale is now; the
// exaggeration box is disabled while it is chosen with the scale; choosing
// Auto, or a scale, is ONE undoable step.

#include <gtest/gtest.h>

#include <QComboBox>
#include <QDoubleSpinBox>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/commands/entity_commands.hpp"
#include "sheet_editor.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::qt::SheetEditor;
using katana::qt::SheetSource;
namespace plotting = katana::cad::plotting;
namespace cmd = katana::commands;

namespace {

// A road east from (10, 50) to (190, 50), its design rising from RL 20 to
// RL 29: 180 m of chainage and 9 m of level, with no surface to cut - the
// long section is its design alone.
void road(Document& document)
{
    katana::entity::Alignment alignment;
    alignment.name = "ROAD";
    alignment.horizontal.pis = {{Point2(10.0, 50.0)}, {Point2(190.0, 50.0)}};
    alignment.vertical =
        katana::geometry::VerticalAlignment{{{0.0, 20.0, 0.0}, {180.0, 29.0, 0.0}}};
    ASSERT_TRUE(document.execute(cmd::createAlignment(alignment)).ok());
}

// A sheet whose one viewport is a long section of the road over the whole
// A3 drawing area, at a fixed H 1:500 V 1:50.
void sheetWithLongSection(Document& document, bool automatic)
{
    plotting::Viewport section;
    section.id = "vp1";
    section.kind = plotting::ViewportKind::LongSection;
    section.rect = Box2(Point2(23.0, 35.0), Point2(410.0, 287.0));
    section.scale = 500.0;
    section.verticalExaggeration = 10.0;
    section.autoScale = automatic;
    section.autoCentre = true;
    section.source.alignment = "ROAD";
    plotting::Sheet sheet;
    sheet.id = "s1";
    sheet.name = "LONG SECTION";
    sheet.viewports = {section};
    ASSERT_TRUE(plotting::addSheet(document, sheet).ok());
}

const plotting::Viewport& onlyViewport(const Document& document)
{
    return document.sheetSet().sheets.at(0).viewports.at(0);
}

struct Shown {
    explicit Shown(Document& document)
        : editor(document, [&document] {
              SheetSource source;
              source.plan = katana::qt::planSourceOf(document);
              source.document = &document;
              source.revision = document.modelRevision();
              return source;
          })
    {
        editor.resize(1400, 900);
        editor.show();
        katana::qt::test::processEvents();
        editor.canvas()->select("vp1");
        katana::qt::test::processEvents();
    }
    SheetEditor editor;

    [[nodiscard]] QComboBox* scale() const
    {
        return editor.findChild<QComboBox*>(QStringLiteral("sheetViewportScale"));
    }
    [[nodiscard]] QDoubleSpinBox* exaggeration() const
    {
        return editor.findChild<QDoubleSpinBox*>(QStringLiteral("sheetViewportExaggeration"));
    }
};

} // namespace

TEST(SheetSectionEditor, AnAutomaticSectionShowsWhatItIsNowAndDisablesTheExaggeration)
{
    Document document;
    road(document);
    sheetWithLongSection(document, true);
    Shown shown(document);

    QComboBox* scale = shown.scale();
    ASSERT_NE(scale, nullptr);
    EXPECT_EQ(scale->currentText(), QStringLiteral("Auto"));
    // 180 m across the 363 mm plot needs 1:551, so 1:750; 9 m up its 210 mm
    // then fits ten times (docs/plotting.md's example).
    EXPECT_EQ(scale->toolTip(), QStringLiteral("Automatic: now H 1:750 V 1:75"));
    QDoubleSpinBox* exaggeration = shown.exaggeration();
    ASSERT_NE(exaggeration, nullptr);
    EXPECT_FALSE(exaggeration->isEnabled());
}

TEST(SheetSectionEditor, ChoosingAutoForASectionIsOneStep)
{
    Document document;
    road(document);
    sheetWithLongSection(document, false);
    Shown shown(document);

    QComboBox* scale = shown.scale();
    ASSERT_NE(scale, nullptr);
    EXPECT_NE(scale->currentText(), QStringLiteral("Auto"));
    QDoubleSpinBox* exaggeration = shown.exaggeration();
    ASSERT_NE(exaggeration, nullptr);
    EXPECT_TRUE(exaggeration->isEnabled());

    const std::size_t before = document.history().undoCount();
    scale->setCurrentIndex(scale->findText(QStringLiteral("Auto")));
    emit scale->textActivated(QStringLiteral("Auto"));
    katana::qt::test::processEvents();
    EXPECT_TRUE(onlyViewport(document).autoScale);
    EXPECT_EQ(document.history().undoCount(), before + 1);
    // The properties were built again, from the edited viewport.
    ASSERT_NE(shown.exaggeration(), nullptr);
    EXPECT_FALSE(shown.exaggeration()->isEnabled());

    ASSERT_TRUE(document.undo().ok());
    EXPECT_FALSE(onlyViewport(document).autoScale);
}
