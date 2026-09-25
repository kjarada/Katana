#pragma once

// GIS > Online Data... (docs/gis_online.md): the window onto the provider
// catalogue.
//
// The dialog is non-modal and holds no state of its own beyond what is typed
// into it: the providers come from the catalogue the workbench owns, and an
// import is not performed here - the dialog builds the SAME
// interop::OnlineCommand the ONLINE IMPORT verb parses, and hands it to the
// workbench, which runs it as a background job exactly as it runs the typed
// verb. So there is no path through the dialog that an agent cannot take
// through the command line.
//
// Every control has an object name, which is how the tests
// (tests/qt_widgets/test_gis_online.cpp) and the headless --dialog, --fill
// and --press switches drive it:
//   onlineDataDialog      the dialog (the action onlineData + "Dialog")
//   onlineSearch          filter the tree as you type
//   onlineProviders       the tree: Australia > state > provider > layer, Global, Custom
//   onlineDetails         the chosen layer: kind, service, licence, attribution,
//                         coverage, key needed, verified
//   onlineArea            Current view / Drawing extents / Selection / Typed box
//   onlineBox             x0,y0,x1,y1 for a typed box
//   onlineBoxCrs          the typed box's CRS: the project's, or WGS 84 lon/lat
//   onlineResolutionAuto  choose the resolution from the area and the data
//   onlineResolution      ground units per pixel, for rasters
//   onlineTargetLayer     the Katana layer vectors go on
//   onlineTag             an OpenStreetMap tag test (building, highway=primary)
//   onlineFrom, onlineTo  acquisition dates for Sentinel-2
//   onlineCloud           the cloud ceiling, percent
//   onlineProjectCrs      what the project's CRS is, or that it has none
//   onlineCrs             a CRS to use when the project has none
//   onlineKey             this provider's key (never shown back)
//   onlineSaveKey         store it in the application's settings
//   onlineAddCustom       Add Custom Service... (a URL; its layers are discovered)
//   onlineCustomUrl       the URL for Add Custom Service
//   onlineImport          import the chosen layer
//   onlineStatus          what happened last
//   onlineClose           close

#include <QDialog>

#include <functional>
#include <string>

#include "katana/core/error.hpp"
#include "katana/interop/online_catalogue.hpp"
#include "katana/interop/online_verbs.hpp"

class QCheckBox;
class QComboBox;
class QDateEdit;
class QDoubleSpinBox;
class QLabel;
class QLineEdit;
class QPushButton;
class QTreeWidget;
class QTreeWidgetItem;

namespace katana::qt {

// What the dialog asks of whoever shows it; the workbench supplies these, a
// test supplies its own.
struct OnlineDialogContext {
    std::function<const katana::interop::OnlineCatalogue&()> catalogue;
    // The project's coordinate system, "EPSG:7856"; empty when it has none.
    std::function<std::string()> projectCrs;
    std::function<bool(const std::string& keyName)> hasKey;
    std::function<void(const std::string& keyName, const std::string& value)> saveKey;
    // Runs the command (an Import) in the background. An error is shown in
    // the status line; success means the job started.
    std::function<katana::core::Status(const katana::interop::OnlineCommand&)> run;
    // Discovers the service at a URL and adds it to the user catalogue, in
    // the background; the dialog is refreshed by the workbench when it lands.
    std::function<katana::core::Status(const std::string& url)> addCustom;
};

class OnlineDataDialog final : public QDialog {
  public:
    explicit OnlineDataDialog(OnlineDialogContext context, QWidget* parent = nullptr);

    // Rebuilds the tree from the catalogue, keeping the chosen layer.
    void refreshCatalogue();
    // Chooses a layer in the tree. False when there is no such layer.
    bool selectLayer(const std::string& provider, const std::string& layer);
    // The Import the dialog's fields describe. InvalidArgument, saying what is
    // missing, when no layer is chosen or the typed box does not parse.
    [[nodiscard]] katana::core::Result<katana::interop::OnlineCommand> command() const;
    void setStatus(const QString& text, bool isError = false);

  private:
    void showDetails();
    void applyFilter(const QString& text);
    void importChosen();
    void addCustomService();
    void saveKey();
    [[nodiscard]] const katana::interop::OnlineLayer* chosenLayer() const;

    OnlineDialogContext context_;
    QLineEdit* search_ = nullptr;
    QTreeWidget* tree_ = nullptr;
    QLabel* details_ = nullptr;
    QComboBox* area_ = nullptr;
    QLineEdit* box_ = nullptr;
    QComboBox* boxCrs_ = nullptr;
    QCheckBox* resolutionAuto_ = nullptr;
    QDoubleSpinBox* resolution_ = nullptr;
    QLineEdit* targetLayer_ = nullptr;
    QLineEdit* tag_ = nullptr;
    QDateEdit* from_ = nullptr;
    QDateEdit* to_ = nullptr;
    QCheckBox* useDates_ = nullptr;
    QDoubleSpinBox* cloud_ = nullptr;
    QLabel* projectCrs_ = nullptr;
    QLineEdit* crs_ = nullptr;
    QLineEdit* key_ = nullptr;
    QPushButton* saveKey_ = nullptr;
    QLineEdit* customUrl_ = nullptr;
    QPushButton* addCustom_ = nullptr;
    QPushButton* import_ = nullptr;
    QLabel* status_ = nullptr;
};

} // namespace katana::qt
