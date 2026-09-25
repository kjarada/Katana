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
//   onlineDetails         the chosen layer: kind, service, service CRS, version,
//                         limits, time, licence, attribution, coverage, key
//                         needed, verified
//   onlineCatalogue       a catalogue layer's search (shown only for one):
//   onlineCatalogueSearch   the words, as ONLINE LAYERS <provider> <words> takes them
//   onlineCatalogueRun      search
//   onlineCatalogueResults  what it found: title, dataset, format, address
//   onlineCatalogueAdd      add the chosen result's service, as ONLINE CUSTOM <url>
//   onlineArea            Current view / Drawing extents / Selection / Typed box
//   onlineBox             x0,y0,x1,y1 for a typed box
//   onlineBoxCrs          the typed box's CRS: the project's, or WGS 84 lon/lat
//   onlineResolutionAuto  choose the resolution from the area and the data
//   onlineResolution      ground units per pixel, for rasters
//   onlineTargetLayer     the Katana layer vectors go on
//   onlineTag             an OpenStreetMap tag test (building, highway=primary)
//   onlineFrom, onlineTo  acquisition dates for Sentinel-2
//   onlineCloud           the cloud ceiling, percent
//   onlineTimeDefault     the layer's own date (the latest), for a time-enabled layer
//   onlineTime            the date to ask for otherwise: time=YYYY-MM-DD
//   onlineAdvanced        the Advanced group:
//   onlineTimeoutOn         give up after onlineTimeout seconds: timeout=<s>
//   onlineTimeout
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
#include <optional>
#include <string>
#include <utility>

#include "katana/core/error.hpp"
#include "katana/interop/online_catalogue.hpp"
#include "katana/interop/online_requests.hpp"
#include "katana/interop/online_verbs.hpp"

class QCheckBox;
class QComboBox;
class QDateEdit;
class QDoubleSpinBox;
class QLabel;
class QLineEdit;
class QGroupBox;
class QPushButton;
class QSpinBox;
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
    // Runs the command (an Import, a catalogue search - ONLINE LAYERS - or
    // ONLINE CUSTOM) in the background. An error is shown in the status
    // line; success means the job started. A search's results come back
    // through showCatalogueResults.
    std::function<katana::core::Status(const katana::interop::OnlineCommand&)> run;
    // Discovers the service at a URL and adds it to the user catalogue, in
    // the background; the dialog is refreshed by the workbench when it lands.
    std::function<katana::core::Status(const std::string& url)> addCustom;
    // Opens the project coordinate system dialog, suggesting systems for the
    // typed box's centre when it is in longitude and latitude; true when the
    // project's coordinate system changed.
    std::function<bool(std::optional<std::pair<double, double>> place)> chooseProjectCrs;
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
    // Shows the project's coordinate system as it is now.
    void refreshProjectCrs();
    // What a catalogue search found - the workbench's answer to the ONLINE
    // LAYERS line it ran, the dialog's or a typed one - listed in
    // onlineCatalogueResults for Add to take a service from.
    void showCatalogueResults(const std::vector<katana::interop::CkanResource>& found);

    // A layer's limits in a person's words, by its kind of service: the
    // facts ONLINE INFO gives as max_zoom, page_size and the rest. Empty for
    // a service with none.
    [[nodiscard]] static QString limitsText(const katana::interop::OnlineLayer& layer);
    // Whether the layer has a date to choose: a time dimension, or {time} in
    // its address.
    [[nodiscard]] static bool hasTime(const katana::interop::OnlineLayer& layer);

  private:
    void showDetails();
    void applyFilter(const QString& text);
    void importChosen();
    void addCustomService();
    // Runs ONLINE LAYERS <provider> <words> for the chosen catalogue layer.
    void searchCatalogue();
    // Runs ONLINE CUSTOM <url> for the chosen result.
    void addCatalogueResult();
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
    QCheckBox* timeDefault_ = nullptr;
    QDateEdit* time_ = nullptr;
    QCheckBox* timeoutOn_ = nullptr;
    QSpinBox* timeout_ = nullptr;
    QGroupBox* catalogue_ = nullptr;
    QLineEdit* catalogueSearch_ = nullptr;
    QPushButton* catalogueRun_ = nullptr;
    QTreeWidget* catalogueResults_ = nullptr;
    QPushButton* catalogueAdd_ = nullptr;
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
