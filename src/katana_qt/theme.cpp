#include "theme.hpp"

#include <QApplication>
#include <QPalette>
#include <QString>
#include <QStyleFactory>

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

// The stylesheet is assembled from the tokens rather than written with colour
// literals, so that the tokens above are the ONLY place a colour is decided.
QString styleSheet()
{
    const auto c = [](const QColor& color) { return color.name(); };
    QString css = R"css(
        QMainWindow, QDialog { background: %window%; }
        QMainWindow::separator { background: %border%; width: 1px; height: 1px; }

        QMenuBar { background: %window%; color: %text%; border-bottom: 1px solid %border%; }
        QMenuBar::item { padding: 5px 10px; background: transparent; }
        QMenuBar::item:selected { background: %hover%; border-radius: 4px; }
        QMenu { background: %panel%; color: %text%; border: 1px solid %border%; padding: 4px; }
        QMenu::item { padding: 5px 28px 5px 12px; border-radius: 4px; }
        QMenu::item:selected { background: %accent%; color: %accentText%; }
        QMenu::item:disabled { color: %textDisabled%; }
        QMenu::separator { height: 1px; background: %border%; margin: 4px 8px; }
        QMenu::icon { padding-left: 6px; }

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

        QDockWidget { color: %textMuted%; titlebar-close-icon: none; titlebar-normal-icon: none; }
        QDockWidget::title { background: %window%; padding: 6px 10px; text-align: left;
                             border-bottom: 1px solid %border%; }

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
    css.replace("%text%", c(text()));
    return css;
}

} // namespace

void apply(QApplication& application)
{
    // Fusion, because it is the one built-in style that honours a palette
    // completely and looks the same on every platform. The native Windows
    // style ignores most of a dark palette and draws light controls into it.
    application.setStyle(QStyleFactory::create("Fusion"));

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
