#pragma once

// GIS > Processing - GDAL > Formats... (docs/interop.md, "Formats"): every
// format this build of GDAL reads and writes, and a driver's options, as the
// FORMATS verb answers - read-only, non-modal, changing nothing.
//
// The table is the verb's: the dialog builds the FORMATS line its choices
// make, shows it (gisFormatsCommand), and fills the table from the records
// the one geoprocessing executor prepares for it. FORMATS changes nothing
// and answers at prepare, so nothing runs as a job and the command log is
// not filled with a hundred records at every keystroke; gisFormatsRun puts
// the line and its records in the log through the window's executor
// (GeoServices::run), for a person who wants them there or in a script.
//
// Object names:
//   gisFormatsDialog      the dialog (action gisFormats)
//   gisFormatsKind        Raster and vector | Raster | Vector
//   gisFormatsCapability  Reads or writes | Reads | Writes
//   gisFormatsFilter      words each row holds: a name, a description, an extension
//   gisFormatsTable       Driver | Description | Kinds | Reads | Writes | Extensions | /vsi
//   gisFormatsCount       how many formats the line lists, and GDAL's version
//   gisFormatsCommand     the FORMATS line the table shows
//   gisFormatsOptions     the selected driver's options (FORMATS OPTIONS <driver>):
//                         List | Name | Type | Default | Choices | Description
//   gisFormatsRun         runs the line through the window's executor
//   gisFormatsClose       close

#include <QDialog>
#include <QString>

class QComboBox;
class QLabel;
class QLineEdit;
class QTableWidget;

namespace katana::qt {

class GeoMenus;
class GeoWorkbench;

// The menu item, action gisFormats under GIS > Processing - GDAL: it shows
// the one FormatsDialog of the window, made the first time and kept.
void addFormatsItem(GeoMenus& menus, GeoWorkbench& workbench);

class FormatsDialog final : public QDialog {
  public:
    explicit FormatsDialog(GeoWorkbench& workbench, QWidget* parent = nullptr);

    // The FORMATS line the choices make.
    [[nodiscard]] QString line() const;
    // Rows of the formats table.
    [[nodiscard]] int formatCount() const;

  private:
    void refresh();
    void showOptions();

    GeoWorkbench& workbench_;
    QComboBox* kind_ = nullptr;
    QComboBox* capability_ = nullptr;
    QLineEdit* filter_ = nullptr;
    QTableWidget* table_ = nullptr;
    QLabel* count_ = nullptr;
    QLabel* command_ = nullptr;
    QTableWidget* options_ = nullptr;
};

} // namespace katana::qt
