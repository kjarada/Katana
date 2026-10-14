// Terrain > Surface Styles (terrain/surface_style_manager.hpp), driven by its
// object names as a person drives it. The dialog changes nothing itself: it
// writes the SURFACESTYLE (or SURFACE STYLE) line a person would type and
// hands it to the window's executor - so what is tested is each line, word
// for word, and, through the real interpreter, that the drawing then holds
// what the line said.

#include <gtest/gtest.h>

#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QDoubleSpinBox>
#include <QImage>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QPushButton>
#include <QStringList>

#include <memory>

#include "command_runner.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/terrain/surface_style.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_surface.hpp"
#include "terrain/surface_style_manager.hpp"

namespace {

using katana::cad::Document;
using katana::cad::SurfaceDisplayStyle;
using katana::cad::SurfaceFaces;
using katana::qt::SurfaceStyleManagerContext;
using katana::qt::SurfaceStyleManagerDialog;
using katana::qt::VerbOutcome;

template <typename T> T* child(QWidget& parent, const char* name)
{
    T* found = parent.findChild<T*>(QString::fromLatin1(name));
    EXPECT_NE(found, nullptr) << "no " << name;
    return found;
}

void click(QWidget& parent, const char* name)
{
    auto* button = child<QPushButton>(parent, name);
    ASSERT_NE(button, nullptr);
    ASSERT_TRUE(button->isEnabled()) << name << " is disabled";
    button->click();
    QApplication::processEvents();
}

void fill(QWidget& parent, const char* name, const QString& text)
{
    auto* field = child<QLineEdit>(parent, name);
    ASSERT_NE(field, nullptr);
    field->setText(text);
}

// The drawing's own two styles, "Style A" the default - so nothing here
// depends on what the built-in styles say - and a surface to assign.
void prepare(Document& document)
{
    katana::cad::SurfaceStyleSet set;
    SurfaceDisplayStyle a;
    a.name = "Style A";
    a.contours.show = true;
    a.contours.interval = 1.0;
    a.contours.labelMajor = false;
    SurfaceDisplayStyle b;
    b.name = "Style B";
    b.planFaces = SurfaceFaces::Elevation;
    set.styles = {a, b};
    set.defaultStyle = "Style A";
    ASSERT_TRUE(document.setSurfaceStyles(set).ok());
    auto tin = katana::terrain::TinSurface::create(
        {{0.0, 0.0, 0.0}, {10.0, 0.0, 10.0}, {10.0, 10.0, 10.0}, {0.0, 10.0, 0.0}}, {{0, 1, 2}, {0, 2, 3}});
    ASSERT_TRUE(tin.ok());
    katana::terrain::SurfaceStore store;
    ASSERT_TRUE(store.add({"ground", std::make_shared<const katana::terrain::TinSurface>(std::move(*tin)), "test"})
                    .ok());
    ASSERT_TRUE(document.setSurfaces(store).ok());
}

// The dialog with a runner that only writes each line down.
struct Recorded {
    Document document;
    QStringList lines;
    std::unique_ptr<SurfaceStyleManagerDialog> dialog;

    Recorded()
    {
        prepare(document);
        SurfaceStyleManagerContext context;
        context.document = &document;
        context.run = [this](const QString& line) {
            lines << line;
            return VerbOutcome{true, "done", {}};
        };
        dialog = std::make_unique<SurfaceStyleManagerDialog>(std::move(context));
    }
};

} // namespace

TEST(SurfaceStyleManager, ItListsTheDrawingsStylesAndNamesTheDefault)
{
    Recorded recorded;
    auto* list = child<QListWidget>(*recorded.dialog, "surfaceStyleList");
    ASSERT_NE(list, nullptr);
    ASSERT_EQ(list->count(), 2);
    EXPECT_EQ(list->item(0)->text(), "Style A");
    EXPECT_EQ(list->item(1)->text(), "Style B");
    // The default is chosen first, and said.
    EXPECT_EQ(recorded.dialog->currentStyle(), "Style A");
    EXPECT_TRUE(child<QLabel>(*recorded.dialog, "surfaceStyleDefaultNote")->text().contains("Style A"));
    auto* surfaces = child<QComboBox>(*recorded.dialog, "surfaceStyleSurface");
    ASSERT_EQ(surfaces->count(), 1);
    EXPECT_EQ(surfaces->itemText(0), "ground");
}

TEST(SurfaceStyleManager, ApplyRunsOneSetLineOfOnlyWhatTheFormChanged)
{
    Recorded recorded;
    QWidget& dialog = *recorded.dialog;
    ASSERT_TRUE(recorded.dialog->selectStyle("Style A"));
    // Untouched, there is nothing to apply.
    EXPECT_TRUE(recorded.dialog->applyLine().isEmpty());
    EXPECT_FALSE(child<QPushButton>(dialog, "surfaceStyleApply")->isEnabled());
    fill(dialog, "surfaceStyleInterval", "0.5");
    child<QCheckBox>(dialog, "surfaceStyleLabels")->setChecked(true);
    child<QComboBox>(dialog, "surfaceStyleFaces")->setCurrentText("slope");
    // In the verb's key order: faces, then the contours' interval and labels.
    const QString expected = "SURFACESTYLE SET \"Style A\" faces=slope interval=0.5 labels=on";
    EXPECT_EQ(recorded.dialog->applyLine(), expected);
    EXPECT_EQ(child<QLabel>(dialog, "surfaceStyleLine")->text(), expected);
    click(dialog, "surfaceStyleApply");
    ASSERT_EQ(recorded.lines.size(), 1);
    EXPECT_EQ(recorded.lines.front(), expected);
    EXPECT_EQ(child<QLabel>(dialog, "surfaceStyleStatus")->text(), "done");
}

TEST(SurfaceStyleManager, EachButtonBuildsItsOwnLine)
{
    Recorded recorded;
    QWidget& dialog = *recorded.dialog;
    fill(dialog, "surfaceStyleName", "My Style");
    ASSERT_TRUE(recorded.dialog->selectStyle("Style A"));
    click(dialog, "surfaceStyleNew");
    ASSERT_TRUE(recorded.dialog->selectStyle("Style A"));
    click(dialog, "surfaceStyleCopy");
    ASSERT_TRUE(recorded.dialog->selectStyle("Style A"));
    click(dialog, "surfaceStyleRename");
    ASSERT_TRUE(recorded.dialog->selectStyle("Style B"));
    click(dialog, "surfaceStyleSetDefault");
    ASSERT_TRUE(recorded.dialog->selectStyle("Style B"));
    click(dialog, "surfaceStyleDelete");
    ASSERT_TRUE(recorded.dialog->selectStyle("Style B"));
    click(dialog, "surfaceStyleAssign");
    const QStringList expected{
        "SURFACESTYLE NEW \"My Style\"",
        "SURFACESTYLE NEW \"My Style\" FROM \"Style A\"",
        "SURFACESTYLE RENAME \"Style A\" \"My Style\"",
        "SURFACESTYLE DEFAULT \"Style B\"",
        "SURFACESTYLE DELETE \"Style B\"",
        "SURFACE STYLE ground \"Style B\"",
    };
    EXPECT_EQ(recorded.lines, expected);
}

TEST(SurfaceStyleManager, AFieldThatDoesNotReadStopsApplyAndSaysWhy)
{
    Recorded recorded;
    QWidget& dialog = *recorded.dialog;
    fill(dialog, "surfaceStyleInterval", "half a metre");
    EXPECT_FALSE(child<QPushButton>(dialog, "surfaceStyleApply")->isEnabled());
    EXPECT_TRUE(child<QLabel>(dialog, "surfaceStyleLine")->text().startsWith("Cannot apply"));
    fill(dialog, "surfaceStyleInterval", "1");
    fill(dialog, "surfaceStyleTriangleColour", "grey-ish");
    EXPECT_FALSE(child<QPushButton>(dialog, "surfaceStyleApply")->isEnabled());
    fill(dialog, "surfaceStyleTriangleColour", "#808080");
    EXPECT_TRUE(child<QPushButton>(dialog, "surfaceStyleApply")->isEnabled());
    // New with no name is not offered.
    fill(dialog, "surfaceStyleName", "  ");
    EXPECT_FALSE(child<QPushButton>(dialog, "surfaceStyleNew")->isEnabled());
}

TEST(SurfaceStyleManager, ThePreviewDrawsTheFormsStyleBeforeItIsApplied)
{
    Recorded recorded;
    QWidget& dialog = *recorded.dialog;
    ASSERT_TRUE(recorded.dialog->selectStyle("Style A"));
    auto* preview = child<QWidget>(dialog, "surfaceStylePreview");
    ASSERT_NE(preview, nullptr);
    preview->resize(320, 220);
    const QImage contours = preview->grab().toImage();
    // The baseline the change is measured against: the sample surface's own
    // footprint in this preview, counted as the pixels opaque flat faces in a
    // colour drawn nowhere else cover. (It was a quarter of the whole
    // preview, a guess: the sample covers less than that at this size.)
    auto* opacity = child<QDoubleSpinBox>(dialog, "surfaceStyleOpacity");
    ASSERT_NE(opacity, nullptr);
    const double opacityAsShown = opacity->value();
    fill(dialog, "surfaceStyleFlatColour", "#FF00FF");
    opacity->setValue(1.0);
    child<QComboBox>(dialog, "surfaceStyleFaces")->setCurrentText("flat");
    const QImage flat = preview->grab().toImage();
    opacity->setValue(opacityAsShown);
    child<QComboBox>(dialog, "surfaceStyleFaces")->setCurrentText("elevation");
    const QImage faces = preview->grab().toImage();
    ASSERT_EQ(contours.size(), faces.size());
    ASSERT_EQ(flat.size(), faces.size());
    std::size_t footprint = 0;
    std::size_t changed = 0;
    for (int y = 0; y < faces.height(); ++y) {
        for (int x = 0; x < faces.width(); ++x) {
            footprint += flat.pixel(x, y) == qRgb(255, 0, 255) ? 1u : 0u;
            changed += contours.pixel(x, y) != faces.pixel(x, y) ? 1u : 0u;
        }
    }
    ASSERT_GT(footprint, 0u) << "the sample was not drawn";
    // Faces fill the sample where only lines were, so most of its footprint
    // changes; what does not is under the contour lines, drawn over the
    // faces alike in both - a few pixels' width along each, far less than
    // half of the footprint.
    EXPECT_GT(changed, footprint / 2) << "changed=" << changed << " footprint=" << footprint;
    EXPECT_TRUE(recorded.lines.isEmpty()); // nothing was run
    ASSERT_TRUE(recorded.dialog->formStyle().has_value());
    EXPECT_EQ(recorded.dialog->formStyle()->planFaces, SurfaceFaces::Elevation);
}

TEST(SurfaceStyleManager, TheSetWordsSayEachChangedFieldAsTheVerbReadsIt)
{
    SurfaceDisplayStyle from;
    from.name = "x";
    EXPECT_EQ(*katana::qt::surfaceStyleSetWords(from, from), QString());
    SurfaceDisplayStyle to = from;
    to.description = "two words";
    to.rangeMin = 2.5;
    to.stops = {{0.0, {0, 0, 0, 255}}, {10.0, {200, 100, 50, 255}}};
    to.shading = katana::cad::SurfaceStyle::ShadedWithEdges;
    EXPECT_EQ(*katana::qt::surfaceStyleSetWords(from, to),
              "description=\"two words\" stops=0:#000000;10:#C86432 min=2.5 shading=edges");
    // Clearing the range's start is the key with nothing after it.
    const auto back = katana::qt::surfaceStyleSetWords(to, from);
    ASSERT_TRUE(back.ok());
    EXPECT_TRUE(back->split(' ').contains("min=")) << back->toStdString();
    to.description = "a \"quoted\" word";
    EXPECT_FALSE(katana::qt::surfaceStyleSetWords(from, to).ok());
}

TEST(SurfaceStyleManager, ItsLinesChangeTheDrawingThroughTheInterpreter)
{
    Document document;
    prepare(document);
    katana::cad::CommandInterpreter interpreter(document);
    SurfaceStyleManagerContext context;
    context.document = &document;
    context.run = [&interpreter](const QString& line) {
        auto reply = interpreter.run(line.toStdString());
        return reply ? VerbOutcome{true, QString::fromStdString(*reply), {}}
                     : VerbOutcome{false, {}, QString::fromStdString(reply.error().describe())};
    };
    SurfaceStyleManagerDialog dialog(std::move(context));
    ASSERT_TRUE(dialog.selectStyle("Style A"));
    fill(dialog, "surfaceStyleInterval", "0.5");
    click(dialog, "surfaceStyleApply");
    const SurfaceDisplayStyle* applied = document.surfaceStyles().find("Style A");
    ASSERT_NE(applied, nullptr);
    EXPECT_EQ(applied->contours.interval, 0.5);
    // One step, undone like a typed one.
    ASSERT_TRUE(interpreter.run("UNDO").ok());
    EXPECT_EQ(document.surfaceStyles().find("Style A")->contours.interval, 1.0);
    // The dialog follows the drawing.
    QApplication::processEvents();
    EXPECT_EQ(child<QLineEdit>(dialog, "surfaceStyleInterval")->text(), "1");
}
