#pragma once

// The start screen (docs/desktop.md, "The start screen"): what this session
// starts with, chosen as it starts - the customisation its survey codes are
// drawn by, and the coordinate system the drawing is in. An interactive start
// with no project to open shows it, until "Show this at start" is cleared;
// File > Start Screen opens it at any time.
//
// A thin front end over two verbs, as every dialog is: Start builds the
// lines - CUSTOMISE BUILTIN <name>, or CUSTOMISE [REPLACE] "<file>", then
// CUSTOMISE KEEP when the choice is to be what every start gives, then
// CRS SET <text> or CRS CLEAR - shows them (startLine) and hands them one by
// one to the window's command runner. Each is logged and undone as if it had
// been typed, and katana_cli and katana_mcp make the same choices with the
// same lines. A choice that is what the session already has makes no line.
//
// It reads no settings and writes none: the window gives it what was chosen
// last (StartChoices) and keeps what is chosen now (coordinateSystem,
// showAtStart). Non-modal, so a headless run fills it by its object names.

#include <vector>

#include <QDialog>
#include <QString>
#include <QStringList>

#include "command_runner.hpp"
#include "katana/cad/customisation_host.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/error.hpp"

class QCheckBox;
class QComboBox;
class QLabel;
class QLineEdit;
class QPushButton;

namespace katana::qt {

struct StartChoices {
    // The coordinate system chosen last, as the CRS verb takes it; empty:
    // local coordinates.
    QString coordinateSystem;
    // Whether an interactive start shows the screen.
    bool showAtStart = true;
    // Where the window keeps the session's customisation between runs. Empty
    // when it keeps none, and "Use it at every start" is then not offered.
    QString keptFile;
};

class StartDialog final : public QDialog {
  public:
    // `runner` runs each line Start builds - the window's executor; without
    // one (a test) a line runs through a CommandInterpreter on `document`,
    // the same verbs. `builtIns` are the built-ins offered, those compiled
    // into the program unless a test says otherwise.
    StartDialog(katana::cad::Document& document, StartChoices last, CommandRunner runner = {},
                QWidget* parent = nullptr,
                const std::vector<katana::cad::BuiltInEntry>& builtIns = katana::cad::builtIns());

    // The lines the choices make, in the order Start runs them; none when
    // they change nothing. What is wrong with the choices, when something is:
    // no file named, a coordinate system that is none.
    [[nodiscard]] katana::core::Result<QStringList> lines() const;
    // Runs the lines, stopping at the first one refused, every reply in
    // startStatus; the screen closes when all ran. False when a line was
    // refused or the choices are wrong (said in startStatus too).
    bool start();
    // The coordinate system chosen, as the CRS verb takes it (empty: local
    // coordinates), and whether the screen is to show at the next start: for
    // the window to keep.
    [[nodiscard]] QString coordinateSystem() const;
    [[nodiscard]] bool showAtStart() const;

  private:
    void refresh();
    [[nodiscard]] VerbOutcome run(const QString& line);

    katana::cad::Document& document_;
    CommandRunner runner_;
    QString keptFile_;
    // The built-in the session is as installed; empty when it is none.
    QString activeBuiltIn_;
    QComboBox* customisation_ = nullptr;
    QLineEdit* file_ = nullptr;
    QPushButton* browse_ = nullptr;
    QCheckBox* replace_ = nullptr;
    QCheckBox* keep_ = nullptr;
    QComboBox* crs_ = nullptr;
    QLabel* crsCheck_ = nullptr;
    QLabel* line_ = nullptr;
    QLabel* status_ = nullptr;
    QCheckBox* showAtStart_ = nullptr;
    QPushButton* start_ = nullptr;
};

} // namespace katana::qt
