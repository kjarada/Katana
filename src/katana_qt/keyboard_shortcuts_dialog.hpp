#pragma once

// Help > Keyboard Shortcuts: every key the window answers to, where the
// command is in the menus and what it does, searchable; a key two commands
// share - which Qt then gives to neither - is marked, from the same check the
// headless --check-shortcuts runs (MainWindow::shortcutClashes).
//
// The dialog is non-modal and changes nothing. Object names:
//   keyboardShortcutsDialog    the dialog (helpKeyboardShortcuts)
//   keyboardShortcutsSearch    the filter: every word must occur in a row
//   keyboardShortcutsTable     Key | Command | Menu | What it does; a key in a
//                              clash is marked "(clash)" and drawn in the
//                              error colour
//   keyboardShortcutsClashes   the clashes, or that there are none
//   keyboardShortcutsCount     how many rows the filter shows
//   keyboardShortcutsClose     close

#include <QDialog>
#include <QString>
#include <QStringList>

#include <vector>

class QLabel;
class QLineEdit;
class QTableWidget;

namespace katana::qt {

struct ShortcutRow {
    // QKeySequence::PortableText ("Ctrl+Shift+E"), as shortcutClashes names
    // a key.
    QString key;
    QString command; // as the menu shows it, without its '&'
    QString menu;    // "File", "Draw > Circle"; "(no menu)" for a key no menu shows
    QString tip;     // the status tip
};

class KeyboardShortcutsDialog final : public QDialog {
  public:
    // `clashes` are shortcutClashes' lines, "Ctrl+L: formatLayers, Line".
    KeyboardShortcutsDialog(std::vector<ShortcutRow> rows, QStringList clashes,
                            QWidget* parent = nullptr);

    void setFilter(const QString& text);
    [[nodiscard]] int shownRows() const;

  private:
    std::vector<ShortcutRow> rows_;
    QLineEdit* search_ = nullptr;
    QTableWidget* table_ = nullptr;
    QLabel* count_ = nullptr;
};

} // namespace katana::qt
