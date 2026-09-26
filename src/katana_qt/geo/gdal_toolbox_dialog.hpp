#pragma once

// GIS > Processing - GDAL > GDAL Toolbox: every one of GDAL's algorithms in a
// window (docs/geoprocessing.md, "The toolbox"). The GDAL verb's menu item.
//
// Non-modal. On the left, GDAL's catalogue as a tree - group, then algorithm -
// filtered by a search that matches an algorithm's path, aliases and
// description; an algorithm that changes or removes existing data says
// "confirm" beside it. On the right, the chosen algorithm: its description
// and help, a binding picker for each dataset it reads (binding_picker.hpp),
// a form of its other arguments generated from what GDAL declares
// (argument_form.hpp), and where its output goes. The dialog writes the GDAL
// line all of that describes - shown, as it will run, in gdalToolboxCommand -
// and Run hands it to the window's one executor (geo_dialog_support.hpp): a
// background job with progress and Cancel, logged and kept in the history. It
// never runs an algorithm itself; the line it shows is the line an agent
// types, or sends to katana_gdal_run.
//
// The line is made by gdalToolboxLine, a pure function of what the fields
// hold:
//   GDAL <algorithm> [--name=value ...] [FROM [<arg>] <source>]...
//        [TO LAYER <path> | TO REFERENCE [<name>] | TO FILE <path> [FORMAT <driver>]
//         | TO SURFACE <name>] [CONFIRM] [OVERWRITE]
// The first required input is bound by a FROM without its name, as the verb
// binds it; every other input names its argument.
//
// Object names (tests/qt_widgets/geo/test_gdal_toolbox.cpp; the headless
// --dialog gdalToolbox, --fill and --press):
//   gdalToolboxDialog       the dialog; GIS > GDAL Toolbox (gdalToolbox)
//   gdalToolboxTabs         Algorithm, and Pipeline (pipeline_builder.hpp: its
//                           object names are there)
//   gdalToolboxSearch       the search; the best match is chosen as it is typed
//                           (an algorithm named exactly so, else the first shown)
//   gdalToolboxTree         the catalogue; a row's text is the algorithm's last word
//   gdalToolboxTitle        the algorithm's path and what running it needs
//   gdalToolboxDescription  GDAL's description of it
//   gdalToolboxHelp         its page in GDAL's documentation (headless: said,
//                           not opened)
//   gdalInput.<arg>.Kind ...  each dataset it reads (a picker; a list for an
//                           argument that takes several: gdalInput.<arg>.List,
//                           .Add, .Remove)
//   gdalArg.<arg>           each other argument (ArgumentForm); the Advanced,
//                           Esoteric and Common ones under gdalToolboxAdvanced
//   gdalOutput.kind         Layer | Reference | File | Surface, as the output can be
//   gdalOutput.name         the layer's path, the raster's name or the file's path
//   gdalOutput.format       FORMAT for a file (GDAL's suggestions)
//   gdalOutput.overwrite    OVERWRITE: replace a file already there
//   gdalToolboxConfirm      CONFIRM, shown only for an algorithm that changes or
//                           removes existing data; the line waits for it
//   gdalToolboxCommand, gdalToolboxPreview, gdalToolboxRun, gdalToolboxStatus,
//   gdalToolboxReply        (GeoRunPanel)

#include <QDialog>
#include <QString>
#include <QStringList>

#include <memory>
#include <string>
#include <utility>
#include <vector>

#include "geo/binding_picker.hpp"
#include "geo/geo_dialog_support.hpp"
#include "katana/core/error.hpp"
#include "katana/gis/processing.hpp"

class QCheckBox;
class QComboBox;
class QGroupBox;
class QLabel;
class QLineEdit;
class QPushButton;
class QShowEvent;
class QTabWidget;
class QTreeWidget;
class QTreeWidgetItem;
class QVBoxLayout;

namespace katana::qt {

class ArgumentForm;
class DocumentWatcher;
class PipelineBuilder;

// What the Algorithm tab's fields hold.
struct GdalToolboxForm {
    std::vector<std::string> path; // {"raster", "hillshade"}
    // ArgumentForm::words, or why they are none.
    QStringList arguments;
    QString argumentsError;
    // Each dataset input, in GDAL's order: its argument, whether it is the
    // first required one, and its source clauses (none: not given).
    struct Input {
        std::string arg;
        bool firstRequired = false;
        bool required = false;
        QStringList sources;
        QString error;
    };
    std::vector<Input> inputs;
    // The output: LAYER, REFERENCE, FILE, SURFACE, or empty when the
    // algorithm writes no dataset.
    QString outputKind;
    QString outputName;
    QString outputFormat;
    bool overwrite = false;
    // Whether the algorithm is Confirm, and whether Confirm is ticked.
    bool needsConfirm = false;
    bool confirm = false;
};

// The GDAL line `form` describes, exactly as it would be typed. InvalidArgument
// naming the field for an argument that does not read, a required input not
// given, a source the controls cannot say, a layer or file path left empty,
// and - until it is ticked - a Confirm algorithm's Confirm.
[[nodiscard]] katana::core::Result<QString> gdalToolboxLine(const GdalToolboxForm& form);

class GdalToolboxDialog final : public QDialog {
  public:
    explicit GdalToolboxDialog(GeoDialogContext context, QWidget* parent = nullptr);
    ~GdalToolboxDialog() override;
    GdalToolboxDialog(const GdalToolboxDialog&) = delete;
    GdalToolboxDialog& operator=(const GdalToolboxDialog&) = delete;

    // Chooses the algorithm at `path` ("raster hillshade"), as a click on its
    // row does. False for a path that names no algorithm.
    bool choose(const QString& path);
    // The algorithm chosen, "raster hillshade"; empty for none.
    [[nodiscard]] QString chosen() const;
    [[nodiscard]] GdalToolboxForm form() const;
    [[nodiscard]] GeoRunPanel& runPanel() const { return *panel_; }
    [[nodiscard]] ArgumentForm& arguments() const { return *arguments_; }
    [[nodiscard]] QTabWidget& tabs() const { return *tabs_; }
    [[nodiscard]] PipelineBuilder& pipeline() const { return *pipeline_; }
    [[nodiscard]] const GeoDialogContext& context() const { return context_; }
    // Remembers the algorithm chosen, per user, to choose it again next time
    // (the window's dialog does; a test's does not).
    void rememberChoices(bool remember) { remember_ = remember; }
    // Refills the pickers' rasters, surfaces and layers, keeping what is chosen.
    void reload();

  protected:
    void showEvent(QShowEvent* event) override;

  private:
    struct InputRow {
        katana::gis::processing::ArgSpec spec;
        BindingPicker* picker = nullptr; // one dataset
        BindingList* list = nullptr;     // several
    };

    void buildTree();
    void filter(const QString& text);
    void algorithmChosen(QTreeWidgetItem* item);
    void refresh();
    void showHelp();

    GeoDialogContext context_;
    QTabWidget* tabs_ = nullptr;
    QLineEdit* search_ = nullptr;
    QTreeWidget* tree_ = nullptr;
    QLabel* title_ = nullptr;
    QLabel* description_ = nullptr;
    QPushButton* help_ = nullptr;
    QGroupBox* inputsBox_ = nullptr;
    QVBoxLayout* inputsLayout_ = nullptr;
    ArgumentForm* arguments_ = nullptr;
    QGroupBox* outputBox_ = nullptr;
    QComboBox* outputKind_ = nullptr;
    QLineEdit* outputName_ = nullptr;
    QComboBox* outputFormat_ = nullptr;
    QCheckBox* overwrite_ = nullptr;
    QCheckBox* confirm_ = nullptr;
    GeoRunPanel* panel_ = nullptr;
    PipelineBuilder* pipeline_ = nullptr;
    katana::gis::processing::AlgorithmSpec spec_;
    std::vector<InputRow> inputs_;
    // The output kind chosen last, whose default name a new kind replaces.
    QString lastKind_;
    bool remember_ = false;
    bool choosing_ = false;
    std::unique_ptr<DocumentWatcher> watcher_;
};

// Opens (making it the first time) the window's GDAL Toolbox, on the
// algorithm last chosen.
GdalToolboxDialog& showGdalToolboxDialog(GeoWorkbench& workbench, QWidget& window);

} // namespace katana::qt
