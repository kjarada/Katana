#pragma once

// File > Import IFC and File > Export IFC (docs/ifc.md, "In the window").
//
// The choices of an IFC exchange and nothing else: neither dialog reads or
// writes a file, and neither calls the IFC module to do so. Each writes the
// line its choices mean - EXPORT, IMPORT, INFO or IFC RULES, with
// ifc/front_end.hpp's grammar (formatExportLine, formatImportLine) - shows
// it as it is edited, and hands it to the window's command line through its
// context, where it is echoed and run exactly as if it had been typed, by
// the code katana_cli's line runs. So there is nothing a dialog can do that
// a typed line, a script or an agent cannot. Both are non-modal and kept by
// the window - a path is typed or browsed for in a field rather than asked
// for up front - so that a headless run drives them by name (--dialog
// fileExportIfc, --fill, --press) and nothing waits on a box no one can see.
//
// The export dialog's table previews what the file will hold: for each layer,
// service, alignment and surface, how many objects become which IFC class and
// why. It is read from the reply of the line with PREVIEW added - the
// writer's own account (ifc::IfcExport::tally), made by writing the file in
// memory, and read by the one reader of that reply (ifc::readExportObjects) -
// so what is shown is what is written, and an agent asking PREVIEW gets the
// same rows. A class that is wrong for a project's layers is put right with a
// rules file (ifc::parseClassificationRules), which IFC RULES writes from the
// defaults.
//
// Object names, the interface tests and the headless driver use:
//   fileExportIfcDialog   ifcExportFile, ifcExportBrowse, ifcExportEntities,
//                         ifcExportSelectedOnly, ifcExportAlignments,
//                         ifcExportSurfaces, ifcExportSchedule,
//                         ifcExportScheduleBrowse, ifcExportSchema,
//                         ifcExportSchemaBrowse, ifcExportSpacing,
//                         ifcExportRules, ifcExportRulesBrowse,
//                         ifcExportSaveRules, ifcExportCrs, ifcExportCommand,
//                         ifcExportClasses, ifcExportCheck, ifcExportPreview,
//                         ifcExportExport, ifcExportClose
//   fileImportIfcDialog   ifcImportFile, ifcImportBrowse, ifcImportDescribe,
//                         ifcImportSummary, ifcImportLocal,
//                         ifcImportAlignments, ifcImportElements,
//                         ifcImportSurfaces, ifcImportTolerance,
//                         ifcImportTakeCrs, ifcImportCommand, ifcImportCheck,
//                         ifcImportImport, ifcImportClose

#include <cstddef>
#include <functional>
#include <optional>
#include <string>
#include <vector>

#include <QDialog>
#include <QString>

#include "katana/core/error.hpp"
#include "katana/ifc/export.hpp"
#include "katana/ifc/front_end.hpp"
#include "katana/ifc/import.hpp"

class QCheckBox;
class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;
class QShowEvent;
class QTableWidget;

namespace katana::qt {

// What the export dialog shows of the window, read each time it is shown.
struct IfcExportState {
    std::size_t entities = 0;
    std::size_t selected = 0;
    std::size_t alignments = 0;
    std::size_t surfaces = 0;
    QString coordinateSystem;   // the project's, as a person reads it; empty for none
    bool georeferenced = false; // it has an EPSG code, which the file needs
};

struct IfcExportContext {
    std::function<IfcExportState()> state;
    // The window's command line: `line` echoed and run as a typed line is,
    // its reply (key=value records) or its error - both in the log.
    std::function<katana::core::Result<std::string>(const QString& line)> runLine;
    // True in a headless session, where Browse asks no one and says so.
    std::function<bool()> headless;
};

struct IfcImportContext {
    std::function<katana::core::Result<std::string>(const QString& line)> runLine;
    std::function<bool()> headless;
};

class IfcExportDialog final : public QDialog {
  public:
    explicit IfcExportDialog(IfcExportContext context, QWidget* parent = nullptr);

    // The choices as they stand, as the verb's arguments; meaningful when
    // check() is empty.
    [[nodiscard]] katana::ifc::ExportArguments arguments() const;
    // The line Export runs: arguments() written out, or empty while check()
    // says what stops it.
    [[nodiscard]] QString line() const;
    // What stops an export, as a sentence; empty when nothing does.
    [[nodiscard]] QString check() const;
    // The same, but for the file: what stops a preview, which writes none.
    [[nodiscard]] QString checkWithoutFile() const;
    void setFile(const QString& path);
    // Reads the window's state again - counts, selection, coordinate system.
    // The window calls it as its drawing changes, and the dialog before each
    // preview and export, so a choice is never judged by counts that were.
    void refresh();
    // The preview, or the account of the export just written, as the table
    // shows it: one row per ClassTally.
    void showTally(const std::vector<katana::ifc::ClassTally>& tally);
    // Fills the table from a reply's object records, and returns them;
    // nullopt, having said why, when they do not read.
    std::optional<std::vector<katana::ifc::ClassTally>> showReply(const std::string& reply);

  protected:
    void showEvent(QShowEvent* event) override;

  private:
    void recheck();
    void browse(QLineEdit* field, const QString& title, const QString& filter, bool save);
    void preview();
    void exportFile();
    void saveRules();
    void say(const QString& text, bool isError);

    IfcExportContext context_;
    IfcExportState state_;
    QLineEdit* file_ = nullptr;
    QCheckBox* entities_ = nullptr;
    QCheckBox* selectedOnly_ = nullptr;
    QCheckBox* alignments_ = nullptr;
    QCheckBox* surfaces_ = nullptr;
    QLineEdit* schedule_ = nullptr;
    QLineEdit* schema_ = nullptr;
    QLineEdit* spacing_ = nullptr;
    QLineEdit* rules_ = nullptr;
    QLabel* crs_ = nullptr;
    QLineEdit* command_ = nullptr;
    QTableWidget* classes_ = nullptr;
    QLabel* check_ = nullptr;
    QPushButton* preview_ = nullptr;
    QPushButton* export_ = nullptr;
    // What the check label last said of its own accord, so that a result
    // (written, or refused) is not overwritten until a field changes.
    bool showingResult_ = false;
};

class IfcImportDialog final : public QDialog {
  public:
    explicit IfcImportDialog(IfcImportContext context, QWidget* parent = nullptr);

    [[nodiscard]] katana::ifc::ImportArguments arguments() const;
    [[nodiscard]] QString line() const;
    [[nodiscard]] QString check() const;
    void setFile(const QString& path);
    // Shows what the file holds, as Describe does (INFO's reply), from a
    // description made elsewhere (--import-options makes it first).
    void setSummary(const QString& text);

  private:
    void recheck();
    void describe();
    void importFile();
    void say(const QString& text, bool isError);

    IfcImportContext context_;
    QLineEdit* file_ = nullptr;
    QPlainTextEdit* summary_ = nullptr;
    QCheckBox* local_ = nullptr;
    QCheckBox* alignments_ = nullptr;
    QCheckBox* elements_ = nullptr;
    QCheckBox* surfaces_ = nullptr;
    QLineEdit* tolerance_ = nullptr;
    QCheckBox* takeCrs_ = nullptr;
    QLineEdit* command_ = nullptr;
    QLabel* check_ = nullptr;
    QPushButton* import_ = nullptr;
    bool showingResult_ = false;
};

} // namespace katana::qt
