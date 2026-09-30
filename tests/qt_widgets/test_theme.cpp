// The theme (src/katana_qt/theme) and the icons: menu section titles are
// drawn under the stylesheet, a long choice list scrolls, the fonts are the
// ones the tokens say and follow the text size, and every icon paints.
//
// The style and stylesheet are put on the widgets under test, not on the
// application, so the other widget tests keep the look they were written
// against. Ink is counted against a baseline drawn the same way (a plain
// separator beside a titled one), never as an absolute number of pixels:
// fonts differ between Windows and Linux (CLAUDE.md, "Testing").

#include <gtest/gtest.h>

#include <QAbstractItemView>
#include <QAction>
#include <QApplication>
#include <QComboBox>
#include <QFontInfo>
#include <QImage>
#include <QMenu>
#include <QPainter>
#include <QStyle>
#include <QToolButton>

#include <algorithm>
#include <cmath>
#include <memory>

#include "dock_chrome.hpp"
#include "icons.hpp"
#include "theme.hpp"

namespace theme = katana::qt::theme;
using katana::qt::Icon;

namespace {

// Rows of `image` in `area` holding a pixel that is not `ground`, give or
// take the antialiasing of its edge: a line of text is several rows, a rule
// one or two, whatever the font.
int inkRows(const QImage& image, const QRect& area, const QColor& ground)
{
    const auto far = [&](QRgb pixel) {
        return std::abs(qRed(pixel) - ground.red()) > 24 ||
               std::abs(qGreen(pixel) - ground.green()) > 24 ||
               std::abs(qBlue(pixel) - ground.blue()) > 24;
    };
    int rows = 0;
    for (int y = area.top(); y <= area.bottom(); ++y) {
        for (int x = area.left(); x <= area.right(); ++x) {
            if (far(image.pixel(x, y))) {
                ++rows;
                break;
            }
        }
    }
    return rows;
}

// The application font put back when a test that changes the text size ends.
struct TextSizeGuard {
    ~TextSizeGuard() { theme::setTextSize(*qApp, theme::TextSize::Standard); }
};

} // namespace

TEST(Theme, AMenuSectionShowsItsTitleUnderTheStylesheet)
{
    // The Survey, Terrain and GIS menus had titled sections that drew as bare
    // lines: Fusion does not support sections, and the stylesheet drew the
    // separators itself. Both at once, as the application has them.
    const std::unique_ptr<QStyle> style(theme::makeStyle());
    QMenu menu;
    menu.setStyle(style.get());
    menu.setStyleSheet(theme::styleSheet());
    QAction* titled = menu.addSection("Surfaces");
    menu.addAction("Surface From Drawing");
    QAction* plain = menu.addSeparator();
    menu.addAction("Cut Section");
    menu.adjustSize();
    const QImage shot = menu.grab().toImage();
    if (qEnvironmentVariableIsSet("KATANA_THEME_TEST_SHOT")) {
        shot.save(qEnvironmentVariable("KATANA_THEME_TEST_SHOT"));
    }

    const QRect titleArea = menu.actionGeometry(titled);
    const QRect lineArea = menu.actionGeometry(plain);
    ASSERT_TRUE(titleArea.isValid());
    ASSERT_TRUE(lineArea.isValid());
    // Room for a line of text, where a plain separator is a sliver.
    EXPECT_GE(titleArea.height(), QFontMetrics(menu.font()).height());
    EXPECT_LT(lineArea.height(), titleArea.height());
    // The title is letters, rows of ink; the plain separator one rule. On
    // the menu's own ground, the panel colour of the stylesheet's QMenu rule.
    const int title = inkRows(shot, titleArea, theme::panel());
    const int line = inkRows(shot, lineArea, theme::panel());
    EXPECT_GE(line, 1) << "the plain separator draws its rule";
    EXPECT_LE(line, 2);
    EXPECT_GE(title, line + 4) << "title " << title << " rows, line " << line;
}

TEST(Theme, ALongChoiceListScrollsRatherThanRunningOffTheScreen)
{
    // Fusion drops a list that cannot be typed into as a popup the height of
    // every item, whatever maxVisibleItems says: a drawing's two hundred
    // styles ran off the screen.
    const std::unique_ptr<QStyle> style(theme::makeStyle());
    EXPECT_EQ(style->styleHint(QStyle::SH_ComboBox_Popup), 0);
    EXPECT_EQ(style->styleHint(QStyle::SH_Menu_SupportsSections), 1);

    QComboBox box;
    box.setStyle(style.get());
    for (int i = 0; i < 200; ++i) {
        box.addItem(QString("Style %1").arg(i));
    }
    box.show();
    box.showPopup();
    QApplication::processEvents();
    const int row = box.view()->sizeHintForRow(0);
    ASSERT_GT(row, 0);
    // maxVisibleItems rows and the frame, not two hundred.
    EXPECT_LE(box.view()->window()->height(), (box.maxVisibleItems() + 2) * row);
    box.hidePopup();
}

TEST(Theme, TheCommandLinesFontIsFixedPitchAndEveryFontFollowsTheTextSize)
{
    const TextSizeGuard guard;
    theme::setTextSize(*qApp, theme::TextSize::Standard);
    EXPECT_TRUE(QFontInfo(theme::monospaceFont()).fixedPitch())
        << QFontInfo(theme::monospaceFont()).family().toStdString();
    const double standard = theme::uiFont().pointSizeF();
    ASSERT_GT(standard, 0.0);
    EXPECT_DOUBLE_EQ(theme::monospaceFont().pointSizeF(), standard);
    EXPECT_DOUBLE_EQ(theme::monospaceFont(1.0).pointSizeF(), standard + 1.0);
    EXPECT_EQ(theme::overlayFont(12).pixelSize(), 12);

    // Large is two points more, everywhere, and the application font with it.
    theme::setTextSize(*qApp, theme::TextSize::Large);
    EXPECT_EQ(theme::textSize(), theme::TextSize::Large);
    EXPECT_DOUBLE_EQ(theme::uiFont().pointSizeF(), standard + 2.0);
    EXPECT_DOUBLE_EQ(QApplication::font().pointSizeF(), standard + 2.0);
    EXPECT_DOUBLE_EQ(theme::monospaceFont().pointSizeF(), standard + 2.0);
    // 12 px scaled as the points are.
    EXPECT_EQ(theme::overlayFont(12).pixelSize(),
              static_cast<int>(std::lround(12.0 * (standard + 2.0) / standard)));
}

TEST(Theme, TextSizesReadBackFromTheirNamesAndNothingElseDoes)
{
    for (const theme::TextSize size : {theme::TextSize::Small, theme::TextSize::Standard,
                                       theme::TextSize::Large, theme::TextSize::ExtraLarge}) {
        EXPECT_EQ(theme::textSizeFrom(theme::toString(size)), size);
    }
    EXPECT_FALSE(theme::textSizeFrom("huge").has_value());
    EXPECT_FALSE(theme::textSizeFrom("").has_value());
    EXPECT_FALSE(theme::textSizeFrom("Large").has_value()); // the stored spelling only
    EXPECT_EQ(theme::textSizeSteps(theme::TextSize::Small), -1);
    EXPECT_EQ(theme::textSizeSteps(theme::TextSize::ExtraLarge), 4);
}

namespace {

// How many pixels along the middle of the top edge of `image` - the straight
// part of a frame, clear of its rounded corners - are the accent's blue: blue
// at least twice red and not near black. A ratio, not a distance to the
// accent, because a dashed pen straddles the pixel row and lands at part
// coverage, which scales every channel alike; the theme's greys have blue
// within a few steps of red, and the accent #4c9ffe has 254 against 76.
int blueOnTopEdge(const QImage& image)
{
    int count = 0;
    for (int x = 5; x < image.width() - 5; ++x) {
        const QRgb pixel = image.pixel(x, 0);
        count += qBlue(pixel) > 2 * qRed(pixel) && qBlue(pixel) > 40 ? 1 : 0;
    }
    return count;
}

// A view bar's Link button as DockTitleBar makes it, under the theme,
// grabbed at its 22 px.
QImage linkButtonShot(bool checked, bool waiting)
{
    const std::unique_ptr<QStyle> style(theme::makeStyle());
    QWidget host;
    QToolButton* button = katana::qt::makeTitleBarButton(&host, Icon::ViewLinked,
                                                         "ViewLinkButton", "Linked", "tip");
    button->setStyle(style.get());
    button->setStyleSheet(theme::styleSheet());
    button->setCheckable(true);
    button->setChecked(checked);
    button->setProperty("waiting", waiting);
    button->style()->unpolish(button);
    button->style()->polish(button);
    return button->grab().toImage();
}

} // namespace

TEST(Theme, ACheckedTitleBarButtonIsFramedInTheAccentAndAnUncheckedOneIsNot)
{
    // A view's Link, checked, wears the accent frame a checked toolbar
    // button wears. The rule for every chrome button has the weight of
    // QToolButton:checked and came after it, so it took the frame away and
    // a checked Link looked like an unchecked one.
    const QImage checked = linkButtonShot(true, false);
    const QImage unchecked = linkButtonShot(false, false);
    ASSERT_GE(checked.width(), 22);
    const int straight = checked.width() - 10;
    EXPECT_EQ(blueOnTopEdge(checked), straight) << "a solid accent edge";
    EXPECT_EQ(blueOnTopEdge(unchecked), 0);
}

TEST(Theme, ALinkWaitingForASecondViewIsFramedDashedNotSolid)
{
    // A link of one moves nothing until a second view joins; its frame is
    // dashed, so the first of the two clicks does not look like the last.
    // Some of the straight edge is the accent (the dashes) and some is not
    // (the gaps between them).
    const QImage waiting = linkButtonShot(true, true);
    const int straight = waiting.width() - 10;
    const int dashes = blueOnTopEdge(waiting);
    EXPECT_GT(dashes, 0) << "the dashes";
    EXPECT_LT(dashes, straight) << "and the gaps";
}

TEST(Theme, ACheckedMenuItemWithAnIconIsFramedAndAnUncheckedOneIsNot)
{
    // Grid, Object Snap, Link This View: the stylesheet drew a checkable item
    // that has an icon with no check mark at all, and on looked exactly like
    // off, pixel for pixel. Two items alike but for the check, one above the
    // other, the unchecked one the baseline: what differs is the checked
    // one's frame - the accent's blue, round the icon, which is 16 px across
    // (Fusion's small icon) - and nothing else.
    const std::unique_ptr<QStyle> style(theme::makeStyle());
    QMenu menu;
    menu.setStyle(style.get());
    menu.setStyleSheet(theme::styleSheet());
    QAction* on = menu.addAction(katana::qt::icon(Icon::Grid), "Grid");
    on->setCheckable(true);
    on->setChecked(true);
    QAction* off = menu.addAction(katana::qt::icon(Icon::Grid), "Grid");
    off->setCheckable(true);
    menu.adjustSize();
    const QImage shot = menu.grab().toImage();
    const QRect onRow = menu.actionGeometry(on);
    const QRect offRow = menu.actionGeometry(off);
    ASSERT_EQ(onRow.size(), offRow.size());
    int differ = 0;
    int blue = 0;
    QRect framed;
    for (int y = 0; y < onRow.height(); ++y) {
        for (int x = 0; x < onRow.width(); ++x) {
            const QRgb a = shot.pixel(onRow.left() + x, onRow.top() + y);
            if (a == shot.pixel(offRow.left() + x, offRow.top() + y)) {
                continue;
            }
            ++differ;
            blue += qBlue(a) > 2 * qRed(a) && qBlue(a) > 40 ? 1 : 0;
            framed = framed.united(QRect(x, y, 1, 1));
        }
    }
    EXPECT_GT(differ, 0) << "checked and unchecked look alike";
    EXPECT_GT(blue, 0) << "the frame is the accent";
    EXPECT_GE(framed.width(), 16) << "round the icon";
    EXPECT_GE(framed.height(), 16) << "round the icon";
    EXPECT_LT(framed.right(), onRow.width() / 2) << "the icon's cell, not the words";
}

TEST(Icons, TheListHoldsEveryIconOnceAndEachPaintsSomething)
{
    // Declaration order, one of each: the contact sheet and this test see
    // every icon only if the list does.
    const auto& icons = katana::qt::allIcons();
    // The last enumerator, which moves whenever an icon is added at the end.
    ASSERT_EQ(icons.size(), static_cast<std::size_t>(Icon::ZoomSelection) + 1);
    for (std::size_t i = 0; i < icons.size(); ++i) {
        EXPECT_EQ(static_cast<std::size_t>(icons[i]), i);
    }
    // At a menu's 16 px, the smallest size it is shown at.
    for (const Icon which : icons) {
        QImage image(16, 16, QImage::Format_ARGB32_Premultiplied);
        image.fill(Qt::transparent);
        QPainter painter(&image);
        katana::qt::paintIcon(painter, which, QRectF(0, 0, 16, 16), Qt::white, Qt::blue);
        painter.end();
        int ink = 0;
        for (int y = 0; y < 16; ++y) {
            for (int x = 0; x < 16; ++x) {
                ink += qAlpha(image.pixel(x, y)) > 64 ? 1 : 0;
            }
        }
        // Something to see: the set's smallest mark, Minimise's dash, is 18
        // pixels at this size; an icon that paints nothing is 0.
        EXPECT_GE(ink, 12) << "icon " << static_cast<int>(which) << " has " << ink;
    }
}
