#pragma once

// File > Import IFC and File > Export IFC (docs/ifc.md, "In the window").
//
// The choices of an IFC exchange and nothing else: neither dialog reads or
// writes a file. Each hands what was chosen to the window through its
// context, which runs the code the IMPORT and EXPORT verbs run, so a dialog,
// a typed line and katana_cli cannot mean different files
// (ifc/front_end.hpp). Both are non-modal and kept by the window - a path is
// typed or browsed for in a field rather than asked for up front - so that a
// headless run drives them by name (--dialog fileExportIfc, --fill, --press)
// and nothing waits on a box no one can see.
//
// The export dialog's table previews what the file will hold: for each layer,
// service, alignment and surface, how many objects become which IFC class and
// why. It is the writer's own account (ifc::IfcExport::tally), made by writing
// the file in memory, so what is shown is what is written - and a class that is
// wrong for a project's layers is put right with a rules file
// (ifc::parseClassificationRules), which the dialog can start from the
// defaults.
//
// Object names, the interface tests and the headless driver use:
//   fileExportIfcDialog   ifcExportFile, ifcExportBrowse, ifcExportEntities,
//                         ifcExportSelectedOnly, ifcExportAlignments,
//                         ifcExportSurfaces, ifcExportSchedule,
//                         ifcExportScheduleBrowse, ifcExportSchema,
//                         ifcExportSchemaBrowse, ifcExportSpacing,
//                         ifcExportRules, ifcExportRulesBrowse,
//                         ifcExportSaveRules, ifcExportCrs, ifcExportClasses,
//                         ifcExportCheck, ifcExportPreview, ifcExportExport,
//                         ifcExportClose
//   fileImportIfcDialog   ifcImportFile, ifcImportBrowse, ifcImportDescribe,
//                         ifcImportSummary, ifcImportLocal,
//                         ifcImportAlignments, ifcImportElements,
//                         ifcImportSurfaces, ifcImportTolerance,
//                         ifcImportTakeCrs, ifcImportCheck, ifcImportImport,
//                         ifcImportClose

#include <cstddef>
#include <functional>
#include <optional>

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

// What an export is asked for: the verb's arguments, and what only the
// window can choose - which of its objects go.
struct IfcExportRequest {
    katana::ifc::ExportArguments arguments;
    bool entities = true;      // the drawing's entities
    bool selectedOnly = false; // of them, the selected ones
    bool alignments = true;
    bool surfaces = true; // the session's surfaces
};

// What an import is asked for.
struct IfcImportRequest {
    katana::ifc::ImportArguments arguments;
    bool alignments = true;
    bool elements = true; // elements and annotations
    bool surfaces = true; // kept in the session
    double curveTolerance = katana::ifc::ImportOptions{}.curveTolerance;
    // Whether the project takes the file's coordinate system when it has
    // none: nullopt asks (and a headless session only says how).
    std::optional<bool> takeCoordinateSystem;
};

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
    // What `request` would write, no file touched: the account the preview
    // shows. Its error is the dialog's to show; nothing is logged.
    std::function<katana::core::Result<katana::ifc::IfcExport>(const IfcExportRequest&)> preview;
    // Writes the file and reports it in the log; the report, or the error
    // (logged too).
    std::function<katana::core::Result<katana::ifc::IfcExport>(const IfcExportRequest&)> run;
    // Writes the default classification rules to `path`, as a project's
    // starting point.
    std::function<katana::core::Status(const QString& path)> saveDefaultRules;
    // True in a headless session, where Browse asks no one and says so.
    std::function<bool()> headless;
};

// What became of an import: done, or declined by the person at a question
// (the far-apart one), or failed - the last two said apart, since a cancel
// is not a failure.
enum class IfcImportOutcome { Imported, Cancelled, Failed };

struct IfcImportContext {
    // What the file holds, read but not imported, as a person reads it.
    std::function<katana::core::Result<QString>(const QString& path)> describe;
    // Imports, reporting in the log.
    std::function<IfcImportOutcome(const IfcImportRequest&)> run;
    std::function<bool()> headless;
};

class IfcExportDialog final : public QDialog {
  public:
    explicit IfcExportDialog(IfcExportContext context, QWidget* parent = nullptr);

    // The choices as they stand; meaningful when check() is empty.
    [[nodiscard]] IfcExportRequest request() const;
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

    [[nodiscard]] IfcImportRequest request() const;
    [[nodiscard]] QString check() const;
    void setFile(const QString& path);
    // Shows what the file holds, as Describe does, from a description made
    // elsewhere (--import-options makes it before the dialog).
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
    QLabel* check_ = nullptr;
    QPushButton* import_ = nullptr;
    bool showingResult_ = false;
};

} // namespace katana::qt
