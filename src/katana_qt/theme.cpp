#include "theme.hpp"

#include <QApplication>
#include <QFontDatabase>
#include <QFontMetrics>
#include <QPainter>
#include <QPalette>
#include <QProxyStyle>
#include <QString>
#include <QStyleOptionMenuItem>

#include <algorithm>
#include <cmath>

namespace katana::qt::theme {

QColor viewport() { return QColor(0x1e, 0x23, 0x29); }
QColor window() { return QColor(0x23, 0x28, 0x2f); }
QColor panel() { return QColor(0x2a, 0x30, 0x38); }
QColor raised() { return QColor(0x32, 0x3a, 0x44); }
QColor hover() { return QColor(0x3a, 0x44, 0x50); }
QColor border() { return QColor(0x3d, 0x47, 0x53); }

QColor text() { return QColor(0xd7, 0xdd, 0xe5); }
QColor textMuted() { return QColor(0x8b, 0x97, 0xa5); }
QColor textDisabled() { return QColor(0x5c, 0x67, 0x73); }

QColor accent() { return QColor(0x4c, 0x9f, 0xfe); }
QColor accentText() { return QColor(0x0f, 0x14, 0x19); }
QColor error() { return QColor(0xff, 0x6b, 0x6b); }

namespace {

TextSize chosenSize = TextSize::Standard;

// The platform's own font, as Qt had it before the theme touched it: what
// every size is a step from. Taken on first use, which apply() makes before
// it sets any font.
const QFont& platformFont()
{
    static const QFont font = QApplication::font();
    return font;
}

// The point size of `font`, from its pixel size where it was given in pixels
// (at 96 dpi, the ratio Qt itself assumes: 72 points an inch over 96 pixels).
double pointsOf(const QFont& font)
{
    return font.pointSizeF() > 0.0 ? font.pointSizeF() : font.pixelSize() * 72.0 / 96.0;
}

// A menu's section title: small capitals, muted, so it reads as a heading and
// never as an item - and never as a disabled one, which is the other muted
// text a menu holds.
QFont sectionFont(const QFont& base)
{
    QFont font = base;
    font.setPointSizeF(std::max(6.0, pointsOf(base) * 0.85));
    font.setBold(true);
    font.setCapitalization(QFont::AllUppercase);
    font.setLetterSpacing(QFont::PercentageSpacing, 104.0);
    return font;
}

// Fusion, with two things it does not do.
//
// Menu sections. The Survey, Terrain and GIS menus are grouped under titled
// sections ("Surfaces", "Point Cloud - PDAL") and not one title was ever
// seen: Fusion says it does not support sections (SH_Menu_SupportsSections),
// so QMenu drew each as a plain separator, and the stylesheet's own
// QMenu::separator rule painted over it besides. This style says it does and
// draws the title, and the stylesheet leaves separators to it.
//
// Choice lists. For a list that cannot be typed into Fusion drops a popup
// the height of every item (SH_ComboBox_Popup), ignoring maxVisibleItems - a
// drawing's two hundred styles in the Style box ran off the screen. This
// style asks for the scrolling list instead.
class KatanaStyle final : public QProxyStyle {
  public:
    KatanaStyle() : QProxyStyle(QStringLiteral("Fusion")) {}

    int styleHint(StyleHint hint, const QStyleOption* option, const QWidget* widget,
                  QStyleHintReturn* data) const override
    {
        switch (hint) {
        case SH_Menu_SupportsSections:
            return 1;
        case SH_ComboBox_Popup:
            return 0;
        default:
            return QProxyStyle::styleHint(hint, option, widget, data);
        }
    }

    QSize sizeFromContents(ContentsType type, const QStyleOption* option, const QSize& size,
                           const QWidget* widget) const override
    {
        if (const auto* item = separatorOf(type, option)) {
            if (item->text.isEmpty()) {
                return {size.width(), kSeparatorHeight};
            }
            const QFontMetrics metrics(sectionFont(item->font));
            return {std::max(size.width(), metrics.horizontalAdvance(item->text) + 4 * kIndent),
                    metrics.height() + kSectionPadding};
        }
        return QProxyStyle::sizeFromContents(type, option, size, widget);
    }

    void drawControl(ControlElement element, const QStyleOption* option, QPainter* painter,
                     const QWidget* widget) const override
    {
        const auto* item = separatorOf(element == CE_MenuItem ? CT_MenuItem : CT_CustomBase,
                                       option);
        if (item == nullptr) {
            QProxyStyle::drawControl(element, option, painter, widget);
            return;
        }
        painter->save();
        const QRect area = item->rect;
        if (item->text.isEmpty()) {
            painter->setPen(QPen(border(), 1));
            painter->drawLine(area.left() + kIndent, area.center().y(), area.right() - kIndent,
                              area.center().y());
        } else {
            // The title alone, no rule: the capitals and the space above
            // already part it from the items before.
            painter->setFont(sectionFont(item->font));
            painter->setPen(textMuted());
            // Below the middle: a title belongs to the items after it.
            painter->drawText(area.adjusted(kIndent, kSectionPadding / 2, -kIndent, 0),
                              Qt::AlignLeft | Qt::AlignVCenter | Qt::TextSingleLine, item->text);
        }
        painter->restore();
    }

  private:
    // A plain separator's height, and a title's space above and below its
    // text; the indent is the stylesheet's QMenu::item padding-left, so a
    // title starts where the items' text would without an icon.
    static constexpr int kSeparatorHeight = 9;
    static constexpr int kSectionPadding = 10;
    static constexpr int kIndent = 12;

    static const QStyleOptionMenuItem* separatorOf(ContentsType type, const QStyleOption* option)
    {
        if (type != CT_MenuItem) {
            return nullptr;
        }
        const auto* item = qstyleoption_cast<const QStyleOptionMenuItem*>(option);
        return item != nullptr && item->menuItemType == QStyleOptionMenuItem::Separator ? item
                                                                                         : nullptr;
    }
};

} // namespace

// The stylesheet is assembled from the tokens rather than written with colour
// literals, so that the tokens above are the ONLY place a colour is decided.
QString styleSheet()
{
    const auto c = [](const QColor& color) { return color.name(); };
    QString css = R"css(
        QMainWindow, QDialog { background: %window%; }
        /* The splitter between docks. Five pixels because the old 1 px line
           had to be hit to the pixel to be dragged; lit in the accent under
           the pointer so that it is plain it can be. */
        QMainWindow::separator { background: %window%; width: 5px; height: 5px; }
        QMainWindow::separator:hover { background: %accent%; }

        QMenuBar { background: %window%; color: %text%; border-bottom: 1px solid %border%; }
        QMenuBar::item { padding: 5px 10px; background: transparent; }
        QMenuBar::item:selected { background: %hover%; border-radius: 4px; }
        /* -qt-style-features: the stylesheet otherwise draws a menu's
           separators itself, with its parent's code, whenever the menu has a
           background colour - and that code draws no section titles. Saying
           the base style can draw on this background hands them back to
           KatanaStyle; the items are still the stylesheet's (QMenu::item). */
        QMenu { background: %panel%; color: %text%; border: 1px solid %border%; padding: 4px;
                -qt-style-features: background-color; }
        QMenu::item { padding: 5px 28px 5px 12px; border-radius: 4px; }
        QMenu::item:selected { background: %accent%; color: %accentText%; }
        QMenu::item:disabled { color: %textDisabled%; }
        /* No QMenu::separator rule: KatanaStyle draws separators and section
           titles, and a rule here would paint over the titles. */
        QMenu::icon { padding-left: 6px; }
        /* A checked item's icon framed as a checked toolbar button is. The
           stylesheet draws an item with an icon by its QMenu::icon rule and
           no check mark at all, so Grid, Object Snap, Link This View and
           every other toggle looked the same on as off - pixel for pixel. */
        QMenu::icon:checked { background: %raised%; border: 1px solid %accent%;
                              border-radius: 3px; }

        QToolBar { background: %window%; border: none; border-bottom: 1px solid %border%;
                   padding: 3px 6px; spacing: 2px; }
        QToolBar:left, QToolBar:right { border-bottom: none; border-right: 1px solid %border%;
                                        padding: 6px 3px; }
        QToolBar::separator { background: %border%; width: 1px; height: 1px; margin: 4px 5px; }
        QToolButton { background: transparent; border: 1px solid transparent; border-radius: 5px;
                      padding: 4px; color: %text%; }
        QToolButton:hover { background: %hover%; }
        QToolButton:pressed { background: %raised%; }
        QToolButton:checked { background: %raised%; border: 1px solid %accent%; }
        QToolButton:disabled { color: %textDisabled%; }

        /* Every dock wears a DockTitleBar (dock_chrome.hpp), which draws its
           own buttons; the ::title rule is for a dock made without one.
           The border is the frame of a FLOATING dock, which has no native
           one: Qt takes its width as PM_DockWidgetFrameWidth, insets the
           title bar and contents by it, and resizes the window from it.
           Fusion's own is 1 px - an edge nobody can find with the mouse.
           4 px is the reach of Qt's resize handler (measured offscreen: a
           press 1 to 4 px in from the edge resizes, 5 px does not). Docked,
           a dock has no frame, and with a title bar widget QDockWidget draws
           none at all: DockChrome paints the floating one. */
        QDockWidget { color: %textMuted%; border: 4px solid %border%; }
        QDockWidget::title { background: %window%; padding: 6px 10px; text-align: left;
                             border-bottom: 1px solid %border%; }
        QToolButton[chrome="button"], QToolButton[chrome="close"] {
            padding: 2px; border: 1px solid transparent; border-radius: 4px; }
        QToolButton[chrome="button"]:hover { background: %hover%; }
        QToolButton[chrome="close"]:hover { background: %error%; }
        QToolButton[chrome="button"]:focus, QToolButton[chrome="close"]:focus {
            border: 1px solid %accent%; }
        /* A checked title-bar button (a view's Link) framed as a checked
           toolbar button is. The rule for every chrome button above has the
           same weight as QToolButton:checked and comes later, so without
           this one it took the frame away and a checked button looked like
           any other. */
        QToolButton[chrome="button"]:checked { background: %raised%; border: 1px solid %accent%; }
        /* A Link waiting for a second view - a link of one, which moves
           nothing yet - in a dashed frame: it looked exactly like a working
           link, so the first of the two clicks looked like the last. */
        QToolButton[chrome="button"][waiting="true"]:checked { border: 1px dashed %accent%; }
        QToolButton[chrome="button"]::menu-indicator { subcontrol-position: right center;
                                                       subcontrol-origin: padding; }
        QToolButton[filtered="true"] { border: 1px solid %accent%; }
        QToolBar#MinimisedToolBar { border: none; border-top: 1px solid %border%;
                                    padding: 2px 6px; }
        QToolBar#MinimisedToolBar QToolButton { padding: 3px 8px; }
        QLabel#MinimisedLabel, QLabel#ViewLayersNote { color: %textMuted%; }
        /* A toolbar's name before its buttons (MainWindow::makeToolBar):
           muted, so it reads as a heading and never as a button. */
        QLabel#toolBarName { color: %textMuted%; font-weight: 600; padding: 0 6px 0 4px; }
        QFrame#ViewLayersPopup { background: %panel%; border: 1px solid %border%; }

        QTreeView, QListView, QTableView, QPlainTextEdit, QTextEdit {
            background: %panel%; color: %text%; border: none; outline: 0;
            selection-background-color: %accent%; selection-color: %accentText%;
            alternate-background-color: %window%; }
        QTreeView::item, QListView::item { padding: 3px 2px; }
        QTreeView::item:hover, QListView::item:hover { background: %hover%; }
        QHeaderView::section { background: %window%; color: %textMuted%; border: none;
                               border-bottom: 1px solid %border%; padding: 5px 8px; }

        QLineEdit, QComboBox, QAbstractSpinBox {
            background: %raised%; color: %text%; border: 1px solid %border%; border-radius: 4px;
            padding: 4px 6px; selection-background-color: %accent%;
            selection-color: %accentText%; }
        QLineEdit:focus, QComboBox:focus, QAbstractSpinBox:focus { border: 1px solid %accent%; }
        QComboBox QAbstractItemView { background: %panel%; color: %text%;
                                      border: 1px solid %border%;
                                      selection-background-color: %accent%;
                                      selection-color: %accentText%; }

        QPushButton { background: %raised%; color: %text%; border: 1px solid %border%;
                      border-radius: 4px; padding: 5px 14px; min-width: 64px; }
        QPushButton:hover { background: %hover%; }
        QPushButton:default { border: 1px solid %accent%; }
        QPushButton:disabled { color: %textDisabled%; }

        QStatusBar { background: %window%; color: %textMuted%; border-top: 1px solid %border%; }
        QStatusBar::item { border: none; }
        QStatusBar QLabel { color: %textMuted%; padding: 0 8px; }

        QScrollBar:vertical { background: %panel%; width: 11px; margin: 0; }
        QScrollBar:horizontal { background: %panel%; height: 11px; margin: 0; }
        QScrollBar::handle { background: %border%; border-radius: 4px; min-height: 24px;
                             min-width: 24px; margin: 2px; }
        QScrollBar::handle:hover { background: %textDisabled%; }
        QScrollBar::add-line, QScrollBar::sub-line { width: 0; height: 0; }
        QScrollBar::add-page, QScrollBar::sub-page { background: none; }

        QToolTip { background: %panel%; color: %text%; border: 1px solid %border%;
                   padding: 4px 6px; }
        QSplitter::handle { background: %border%; }
        QTabBar::tab { background: %window%; color: %textMuted%; padding: 6px 12px;
                       border: none; }
        QTabBar::tab:selected { color: %text%; border-bottom: 2px solid %accent%; }
        QTabBar::tab:hover { color: %text%; }
        QCheckBox, QRadioButton, QLabel, QGroupBox { color: %text%; }
    )css";
    css.replace("%window%", c(window()));
    css.replace("%panel%", c(panel()));
    css.replace("%raised%", c(raised()));
    css.replace("%hover%", c(hover()));
    css.replace("%border%", c(border()));
    css.replace("%textDisabled%", c(textDisabled()));
    css.replace("%textMuted%", c(textMuted()));
    css.replace("%accentText%", c(accentText()));
    css.replace("%accent%", c(accent()));
    css.replace("%error%", c(error()));
    css.replace("%text%", c(text()));
    return css;
}

QStyle* makeStyle() { return new KatanaStyle; }

std::string_view toString(TextSize size)
{
    switch (size) {
    case TextSize::Small:
        return "small";
    case TextSize::Standard:
        return "standard";
    case TextSize::Large:
        return "large";
    case TextSize::ExtraLarge:
        return "extra-large";
    }
    return "standard";
}

std::optional<TextSize> textSizeFrom(std::string_view text)
{
    for (const TextSize size :
         {TextSize::Small, TextSize::Standard, TextSize::Large, TextSize::ExtraLarge}) {
        if (text == toString(size)) {
            return size;
        }
    }
    return std::nullopt;
}

int textSizeSteps(TextSize size)
{
    switch (size) {
    case TextSize::Small:
        return -1;
    case TextSize::Standard:
        return 0;
    case TextSize::Large:
        return 2;
    case TextSize::ExtraLarge:
        return 4;
    }
    return 0;
}

QFont uiFont()
{
    QFont font = platformFont();
    font.setPointSizeF(std::max(6.0, pointsOf(platformFont()) + textSizeSteps(chosenSize)));
    return font;
}

QFont monospaceFont(double larger)
{
    const QFont fixed = QFontDatabase::systemFont(QFontDatabase::FixedFont);
    QFont font = fixed;
    // The platform's fixed font is Courier New on Windows, wider and lighter
    // than the chrome beside it; these are drawn for screens. The platform's
    // is kept last, so a machine with none of them still gets fixed pitch.
    font.setFamilies({QStringLiteral("Cascadia Mono"), QStringLiteral("Consolas"),
                      QStringLiteral("DejaVu Sans Mono"), QStringLiteral("Liberation Mono"),
                      QStringLiteral("Menlo"), fixed.family()});
    font.setStyleHint(QFont::TypeWriter);
    font.setFixedPitch(true);
    font.setPointSizeF(std::max(6.0, pointsOf(uiFont()) + larger));
    return font;
}

QFont overlayFont(int pixels)
{
    QFont font = uiFont();
    const double scale = pointsOf(uiFont()) / pointsOf(platformFont());
    font.setPixelSize(std::max(1, static_cast<int>(std::lround(pixels * scale))));
    return font;
}

TextSize textSize() { return chosenSize; }

void setTextSize(QApplication& application, TextSize size)
{
    (void)platformFont(); // taken before the first change, whoever calls first
    chosenSize = size;
    // Without a class name, which also drops the fonts the platform set for
    // single classes - on Windows the menus, the menu bar, tooltips and
    // message boxes each had their own - so the whole window is one font.
    application.setFont(uiFont());
}

void apply(QApplication& application)
{
    // Fusion, because it is the one built-in style that honours a palette
    // completely and looks the same on every platform. The native Windows
    // style ignores most of a dark palette and draws light controls into it.
    // KatanaStyle is Fusion with menu section titles and scrolling choice
    // lists (above).
    (void)platformFont();
    application.setStyle(makeStyle());
    setTextSize(application, TextSize::Standard);

    QPalette palette;
    palette.setColor(QPalette::Window, window());
    palette.setColor(QPalette::WindowText, text());
    palette.setColor(QPalette::Base, panel());
    palette.setColor(QPalette::AlternateBase, window());
    palette.setColor(QPalette::Text, text());
    palette.setColor(QPalette::Button, raised());
    palette.setColor(QPalette::ButtonText, text());
    palette.setColor(QPalette::ToolTipBase, panel());
    palette.setColor(QPalette::ToolTipText, text());
    palette.setColor(QPalette::Highlight, accent());
    palette.setColor(QPalette::HighlightedText, accentText());
    palette.setColor(QPalette::PlaceholderText, textMuted());
    palette.setColor(QPalette::Link, accent());
    palette.setColor(QPalette::BrightText, error());
    for (const QPalette::ColorRole role :
         {QPalette::WindowText, QPalette::Text, QPalette::ButtonText}) {
        palette.setColor(QPalette::Disabled, role, textDisabled());
    }
    application.setPalette(palette);
    application.setStyleSheet(styleSheet());
}

} // namespace katana::qt::theme
